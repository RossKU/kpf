//! A transaction of the thin family read back: which script it ran, what it spent and what it made.
//!
//! The chain holds an object's hash and not its state, so a machine that did not write a transaction has no file of
//! what the transaction made. The arguments of the script that ran are in the transaction, though, and with them the
//! objects it spent are rebuilt and, by the same arithmetic the builders use ([`crate::thin`]), those it made.
//! Nothing here is taken on trust: an object spent is one whose hash is in the input's own script, and an object made
//! is one whose script is the output's. Whatever cannot be rebuilt is reported as not read.
//!
//! What the arguments do not carry: the fixed part of a position where a script is shown its hash only (the caller
//! gives those it knows: [`Known`]), and an order's template at its placement (its hash only).

use kaspa_consensus_core::tx::Transaction;

use crate::cross::{Cross, XLeg, SIDE_A, SIDE_B};
use crate::state::{LONG, SHORT};
use crate::thin::{folded, joined, left, match_orders, merged, novated, turned_legs, turned_users, Position, Claim, Disputed, Liq, LiqObj, LiqState, Order, ExitClaim, Rest, TObj, Template, FIXED_LEN};
use crate::thin_artifacts::{self as pinned, SLOT};
use crate::thin_build::{object_spk, read_opening, script_spk};

/// The pushes of a script that is nothing but pushes (a signature script), each as the bytes it leaves on the stack.
pub fn pushes(s: &[u8]) -> Option<Vec<Vec<u8>>> {
    let (mut at, mut out) = (0usize, vec![]);
    while at < s.len() {
        let op = s[at];
        at += 1;
        let n = match op {
            0x00 => 0,
            0x01..=0x4b => op as usize,
            0x4c => {
                let n = *s.get(at)? as usize;
                at += 1;
                n
            }
            0x4d => {
                let n = u16::from_le_bytes(s.get(at..at + 2)?.try_into().ok()?) as usize;
                at += 2;
                n
            }
            0x4e => {
                let n = u32::from_le_bytes(s.get(at..at + 4)?.try_into().ok()?) as usize;
                at += 4;
                n
            }
            0x4f => {
                out.push(vec![0x81]);
                continue;
            }
            0x51..=0x60 => {
                out.push(vec![op - 0x50]);
                continue;
            }
            _ => return None,
        };
        out.push(s.get(at..at.checked_add(n)?)?.to_vec());
        at += n;
    }
    Some(out)
}

/// A script number of at most 8 bytes.
fn num(b: &[u8]) -> Option<i64> {
    if b.len() > 8 {
        return None;
    }
    let mut v: i64 = 0;
    for (i, x) in b.iter().enumerate() {
        let x = if i + 1 == b.len() { x & 0x7f } else { *x };
        v |= (x as i64) << (8 * i);
    }
    Some(if b.last().is_some_and(|x| x & 0x80 != 0) { -v } else { v })
}

/// An entry of a script of the library as a transaction called it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub script: &'static str,
    pub entry: &'static str,
    args: Vec<(&'static str, Vec<u8>)>,
}

impl Call {
    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.args.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_slice())
    }
    pub fn int(&self, name: &str) -> Option<i64> {
        num(self.bytes(name)?)
    }
    pub fn key(&self, name: &str) -> Option<[u8; 32]> {
        self.bytes(name)?.try_into().ok()
    }
    fn ints<const N: usize>(&self, names: [&str; N]) -> Option<[i64; N]> {
        let mut out = [0i64; N];
        for (o, n) in out.iter_mut().zip(names) {
            *o = self.int(n)?;
        }
        Some(out)
    }
}

/// The call a signature script makes, if its last push is a script of the family's library.
pub fn call_of(sig_script: &[u8]) -> Option<Call> {
    let mut p = pushes(sig_script)?;
    let redeem = p.pop()?;
    let tag = p.pop()?;
    let script = pinned::manifest().contracts.keys().map(|k| k.as_str()).find(|n| *n != SLOT && pinned::kit().script(n) == Some(&redeem[..]))?;
    let abi = pinned::base(script)?;
    let (entry, e) = abi.contracts.values().next()?.entries.iter().find(|(_, e)| e.dispatch_tag.as_bytes()[..] == tag[..])?;
    if e.params.len() != p.len() {
        return None;
    }
    Some(Call { script, entry: entry.as_str(), args: e.params.iter().map(|q| q.name.as_str()).zip(p).collect() })
}

/// Of the object a signature script spends: the library's id, the state's hash, and the input of the script it
/// names (`KpfSlot.go(l, k)`: a transaction may run two scripts of the library, each with its own objects).
pub fn slot_call(sig_script: &[u8]) -> Option<([u8; 32], [u8; 32], Option<i64>)> {
    let p = pushes(sig_script)?;
    let (lib, state) = pinned::kit().read(p.last()?)?;
    // The arguments, the entry's tag, the script: l is the fourth from the end.
    let l = p.len().checked_sub(4).and_then(|i| p.get(i)).and_then(|b| num(b));
    Some((lib, state, l))
}

/// (the library's id, the state's hash) of the object a signature script spends, if it spends one.
pub fn slot_of(sig_script: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    pinned::kit().read(pushes(sig_script)?.last()?)
}

/// What the reader is told from outside the transaction.
pub trait Known {
    /// The object whose state hashes to `state`.
    fn object(&self, state: &[u8; 32]) -> Option<TObj>;
    /// The fixed part of a position that hashes to `h`.
    fn fixed(&self, h: &[u8; 32]) -> Option<Vec<u8>>;
    /// The template that hashes to `h`.
    fn template(&self, h: &[u8; 32]) -> Option<Template>;
}

/// Nothing known.
pub struct Nothing;
impl Known for Nothing {
    fn object(&self, _: &[u8; 32]) -> Option<TObj> {
        None
    }
    fn fixed(&self, _: &[u8; 32]) -> Option<Vec<u8>> {
        None
    }
    fn template(&self, _: &[u8; 32]) -> Option<Template> {
        None
    }
}

/// A transaction of the library, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxRead {
    pub script: &'static str,
    pub entry: &'static str,
    /// The objects spent: the input, the hash its script carries, and the object if it was rebuilt.
    pub spent: Vec<(usize, [u8; 32], Option<TObj>)>,
    /// The objects made: the output, the object, and the covenant id the output is under (the caller holds it to its
    /// registry's: an object under another id is of another registry, or of none).
    pub made: Vec<(usize, TObj, [u8; 32])>,
    /// Outputs under a covenant id that are neither a script of the library nor an object that was rebuilt.
    pub unread: Vec<usize>,
    /// A template the transaction shows whole (a match's).
    pub template: Option<Template>,
}

fn order_of(c: &Call, tpl: [u8; 32], dir: i64, names: [&str; 8]) -> Option<Order> {
    let owner = c.key(names[0])?;
    let [size, limit, margin, tip, expiry, min_fill, min_counter] = c.ints([names[1], names[2], names[3], names[4], names[5], names[6], names[7]])?;
    let o = Order { tpl, owner, dir, size, limit, margin, tip, expiry, min_fill, min_counter };
    o.check().ok().map(|_| o)
}

/// The second position of an operation on two (`y`), which the arguments give as what differs from the first: every
/// reading the arguments allow, of which the caller keeps the one whose hash the input carries.
fn seconds(x: &Position, leg: XLeg, third: Option<[u8; 32]>, m: [i64; 4]) -> Vec<Position> {
    let mut keys = vec![x.x.a_key, x.x.b_key];
    keys.extend(third);
    let mut out = vec![];
    for a in &keys {
        for b in &keys {
            if a == b {
                continue;
            }
            for dir in [leg.dir, if leg.dir == LONG { SHORT } else { LONG }] {
                let y = Position { x: Cross { legs: [XLeg { dir, ..leg }, XLeg::none()], a_key: *a, b_key: *b, a_margin: m[0], b_margin: m[1], ..x.x }, a_min_counter: m[2], b_min_counter: m[3], ..*x };
                if y.check().is_ok() {
                    out.push(y);
                }
            }
        }
    }
    out
}

/// One call of a script of the library: the objects it spent (`slots`: their inputs and hashes, in the script's
/// order), rebuilt where the arguments allow, and what it made of them by the builders' arithmetic (not yet held to
/// the outputs).
#[allow(clippy::type_complexity)]
fn read_call(tx: &Transaction, c: &Call, slots: &[(usize, [u8; 32])], amounts: &[Option<u64>], known: &dyn Known) -> (Vec<(usize, [u8; 32], Option<TObj>)>, Vec<TObj>, Option<Template>) {
    // What the arguments say was spent.
    let mut ins: Vec<TObj> = vec![TObj::Anchor];
    let mut template = None;
    let x = c.bytes("moving").and_then(|moving| {
        let fixed = match c.bytes("st") {
            Some(f) => f.to_vec(),
            None => known.fixed(&c.key("fixed")?)?,
        };
        Position::from_parts(&fixed, moving).ok()
    });
    let two = matches!(c.script, "KpfTMerge" | "KpfTFold" | "KpfTJoin" | "KpfTNovate" | "KpfTLeave");
    let mut y: Option<Position> = None;
    if let Some(x) = x {
        ins.push(TObj::Position(x));
        if !two {
            // The claims on the position, by the arguments that are there.
            if let Some([side, t0, a1, a2, amount, bond]) = c.ints(["side", "t0", "a1", "a2", "amount", "bond"]) {
                let claim = Claim { position: x, side, t0, a: [a1, a2], amount, bond };
                ins.push(TObj::Claim(claim));
                if let Some(d) = c.key("dkey") {
                    ins.push(TObj::Disputed(Disputed { claim, disputer: d }));
                }
            }
            if let (Some([side, t0, a1, a2, fee, add, bond]), Some(liquidator)) = (c.ints(["side", "t0", "a1", "a2", "fee", "add", "bond"]), c.key("lkey")) {
                let liq = Liq { position: x, side, t0, a: [a1, a2], fee, add, bond, liquidator };
                ins.push(TObj::Liq(LiqObj { liq, state: LiqState::Claimed }));
                ins.push(TObj::Liq(LiqObj { liq, state: LiqState::Proved }));
                if let Some(d) = c.key("dkey") {
                    ins.push(TObj::Liq(LiqObj { liq, state: LiqState::Disputed(d) }));
                }
            }
            if let Some([side, since, bond]) = c.ints(["side", "since", "bond"]) {
                ins.push(TObj::ExitClaim(ExitClaim { position: x, side, since, bond }));
            }
        } else if let Some((_, want)) = slots.get(1) {
            let cands = if c.script == "KpfTJoin" {
                // The second position's leg whole, in the first's fixed part (whose second leg is empty).
                let f = x.fixed();
                let leg = c.bytes("leg").filter(|l| l.len() == 111);
                let m = c.ints(["a2", "b2", "aq2", "bq2"]);
                match (leg, m) {
                    (Some(leg), Some(m)) => {
                        let shape = Position { x: Cross { a_margin: m[0], b_margin: m[1], ..x.x }, a_min_counter: m[2], b_min_counter: m[3], ..x };
                        let fixed = [&f[..90], leg, &f[201..FIXED_LEN]].concat();
                        let turned = Position { x: Cross { a_key: x.x.b_key, b_key: x.x.a_key, ..shape.x }, ..shape };
                        [shape, turned].iter().filter_map(|s| Position::from_parts(&fixed, &s.moving()).ok()).collect()
                    }
                    _ => vec![],
                }
            } else {
                let names = if c.script == "KpfTMerge" { ["n2", "e2", "a2", "b2", "aq2", "bq2"] } else { ["ny", "ey", "ay", "by", "aqy", "bqy"] };
                match c.ints(names) {
                    Some([n, e, a, b, aq, bq]) => seconds(&x, XLeg { size: n, entry: e, ..x.x.legs[0] }, c.key("lk"), [a, b, aq, bq]),
                    None => vec![],
                }
            };
            y = cands.into_iter().find(|v| v.hash() == *want).or_else(|| match known.object(want) {
                Some(TObj::Position(v)) => Some(v),
                _ => None,
            });
            ins.extend(y.map(TObj::Position));
        }
    }
    // Orders: a placement's, a cancellation's, a match's two.
    let placed = c.key("tplh").and_then(|t| order_of(c, t, c.int("dir")?, ["owner", "size", "limit", "margin", "tip", "expiry", "minFill", "minCounter"]));
    ins.extend(placed.map(TObj::Order));
    let mut pair = None;
    if c.script == "KpfTMatch" {
        if let Some(t) = c.bytes("tpl").and_then(|b| Template::from_bytes(b).ok()) {
            template = Some(t);
            let long = order_of(c, t.hash(), LONG, ["lk", "ls", "ll", "lm", "lt", "le", "ln", "la"]);
            let short = order_of(c, t.hash(), SHORT, ["sk", "ss", "sl", "sm", "st", "se", "sn", "sa"]);
            if let (Some(l), Some(s)) = (long, short) {
                ins.extend([TObj::Order(l), TObj::Order(s)]);
                pair = Some((t, l, s));
            }
        }
    }
    // A match into a position: the two orders are of the position's own template.
    let mut into = None;
    if c.script == "KpfTReplace" {
        if let Some(a) = x {
            let t = Template::of(&a);
            template = Some(t);
            let long = order_of(c, t.hash(), LONG, ["lk", "ls", "ll", "lm", "lt", "le", "ln", "la"]);
            let short = order_of(c, t.hash(), SHORT, ["sk", "ss", "sl", "sm", "stip", "se", "sn", "sa"]);
            if let (Some(l), Some(s)) = (long, short) {
                ins.extend([TObj::Order(l), TObj::Order(s)]);
                into = Some((l, s));
            }
        }
    }
    let spent: Vec<(usize, [u8; 32], Option<TObj>)> = slots.iter().map(|(i, s)| (*i, *s, ins.iter().find(|o| o.hash() == *s).copied().or_else(|| known.object(s)))).collect();
    let was = |k: usize| spent.get(k).and_then(|(_, _, o)| *o);
    let held = |k: usize| slots.get(k).and_then(|(i, _)| amounts.get(*i).copied().flatten()).map(|v| v as i64).or_else(|| was(k).map(|o| o.least_value()));

    // What it made, by the builders' own arithmetic.
    let position = |a: Position| vec![TObj::Position(a)];
    let side_of = |s: i64| (s == SIDE_A || s == SIDE_B).then_some(s);
    let outs: Vec<TObj> = match (c.script, c.entry, was(0)) {
        ("KpfTPlace", "place", _) => placed.map(TObj::Order).into_iter().collect(),
        // What the payload publishes, and what the reader knew of the fixed part: each is held to the output.
        ("KpfTOpen", "open", _) => [read_opening(&tx.payload), x].into_iter().flatten().map(TObj::Position).collect(),
        ("KpfTMatch", "match", _) => match (pair, c.ints(["n", "x"]), held(0), held(1)) {
            (Some((t, l, s)), Some([n, px]), Some(lv), Some(sv)) => match match_orders(&t, &l, lv, &s, sv, n, px) {
                Ok(m) => {
                    let mut v = vec![TObj::Position(m.position)];
                    for r in [m.long, m.short] {
                        if let Rest::Order(o, _) = r {
                            v.push(TObj::Order(o));
                        }
                    }
                    v
                }
                Err(_) => vec![],
            },
            _ => vec![],
        },
        ("KpfTAdd", "add", Some(TObj::Position(a))) => c.ints(["side", "delta"]).and_then(|[s, d]| a.added(side_of(s)?, d)).map(position).unwrap_or_default(),
        ("KpfTClose", "close", Some(TObj::Position(a))) => c.ints(["side", "t0", "a1", "a2", "amount"]).and_then(|[s, t0, a1, a2, v]| a.claim(s, t0, [a1, a2], v)).map(|k| vec![TObj::Claim(k)]).unwrap_or_default(),
        ("KpfTLate", "late", Some(TObj::Claim(k))) => position(k.position),
        ("KpfTDispute", "dispute", Some(TObj::Claim(k))) => c.key("dkey").map(|d| vec![TObj::Disputed(k.disputed(d))]).unwrap_or_default(),
        ("KpfTDispute", "lapse", Some(TObj::Disputed(d))) => position(d.claim.position),
        // An answer with the verdict 0 refutes the claim (the position again); with the verdict 1 it settles it.
        ("KpfTProve", "answer", Some(TObj::Disputed(d))) => if c.ints(["verdict"]) == Some([0]) { position(d.claim.position) } else { vec![] },
        ("KpfTLiq", "liquidate", Some(TObj::Position(a))) => match (c.ints(["side", "t0", "a1", "a2", "fee", "add"]), c.key("lkey")) {
            (Some([s, t0, a1, a2, fee, add]), Some(k)) => a.liq_stated(s, t0, [a1, a2], fee, add, k).map(|liq| vec![TObj::Liq(LiqObj { liq, state: LiqState::Claimed })]).unwrap_or_default(),
            _ => vec![],
        },
        ("KpfTLiq", "late", Some(TObj::Liq(l))) | ("KpfTLiqDispute", "lapse", Some(TObj::Liq(l))) => position(l.liq.position),
        ("KpfTCure", "cure", Some(TObj::Liq(l))) => l.liq.cured().map(position).unwrap_or_default(),
        ("KpfTLiqDispute", "dispute", Some(TObj::Liq(l))) => c.key("dkey").map(|d| vec![TObj::Liq(LiqObj { liq: l.liq, state: LiqState::Disputed(d) })]).unwrap_or_default(),
        ("KpfTLiqProve", "answer", Some(TObj::Liq(l))) => match c.ints(["verdict"]) {
            Some([1]) => vec![TObj::Liq(LiqObj { liq: l.liq, state: LiqState::Proved })],
            Some([0]) => position(l.liq.position),
            _ => vec![],
        },
        ("KpfTExit", "exit", Some(TObj::Position(a))) => c.ints(["side", "since"]).and_then(|[s, t]| a.exit(s, t)).map(|q| vec![TObj::ExitClaim(q)]).unwrap_or_default(),
        ("KpfTExit", "early", Some(TObj::ExitClaim(q))) | ("KpfTExitRefute", "refute", Some(TObj::ExitClaim(q))) => position(q.position),
        ("KpfTTurn", "turn", Some(TObj::Position(a))) => match c.int("mode") {
            Some(1) => position(turned_users(&a)),
            Some(2) => turned_legs(&a).map(position).unwrap_or_default(),
            _ => vec![],
        },
        ("KpfTMerge", "merge", Some(TObj::Position(a))) => y.and_then(|b| merged(&a, &b).ok()).map(position).unwrap_or_default(),
        ("KpfTFold", "fold", Some(TObj::Position(a))) => y.and_then(|b| folded(&a, &b).ok()).map(position).unwrap_or_default(),
        ("KpfTJoin", "join", Some(TObj::Position(a))) => y.and_then(|b| joined(&a, &b).ok()).map(position).unwrap_or_default(),
        ("KpfTNovate", "novate", Some(TObj::Position(a))) => match (y, held(0), held(1)) {
            (Some(b), Some(av), Some(bv)) => novated(&a, av, &b, bv, false).map(|n| [n.rest_x, n.z, n.rest_y].into_iter().flatten().map(|(o, _)| TObj::Position(o)).collect()).unwrap_or_default(),
            _ => vec![],
        },
        ("KpfTReplace", "replace", Some(TObj::Position(a))) => match (into, c.ints(["n", "x", "side"]), held(0), held(1), held(2)) {
            (Some((l, s)), Some([n, px, side]), Some(av), Some(lv), Some(sv)) => match crate::thin::replaced(&a, av, &l, lv, &s, sv, n, px, side, false) {
                Ok(r) => {
                    let mut v: Vec<TObj> = [r.rest, r.z].into_iter().flatten().map(|(o, _)| TObj::Position(o)).collect();
                    for o in [r.long, r.short] {
                        if let Rest::Order(o, _) = o {
                            v.push(TObj::Order(o));
                        }
                    }
                    v
                }
                Err(_) => vec![],
            },
            _ => vec![],
        },
        ("KpfTLeave", "leave", Some(TObj::Position(a))) => match (y, held(0), held(1)) {
            (Some(b), Some(av), Some(bv)) => left(&a, av, &b, bv, false).map(|l| [Some(l.x_after), Some(l.z), l.rest_y].into_iter().flatten().map(|(o, _)| TObj::Position(o)).collect()).unwrap_or_default(),
            _ => vec![],
        },
        _ => vec![],
    };

    (spent, outs, template)
}

/// Reads `tx` as a transaction of the library `lib`: `None` if it ran no script of it. `amounts` are what the inputs
/// held, where known (the arithmetic of a match, of a novation and of a leg that leaves needs those of the objects).
/// Where one is not known the least its object holds is tried: a wrong guess rebuilds no output (each is held to its
/// script), so it reads less and never wrongly.
pub fn read_tx(lib: &[u8; 32], tx: &Transaction, amounts: &[Option<u64>], known: &dyn Known) -> Option<TxRead> {
    // Every script of the library that ran: an input whose last push is a script of the family, which the
    // transaction writes again under the library's id.
    let calls: Vec<(usize, Call)> = tx
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(i, x)| call_of(&x.signature_script).map(|c| (i, c)))
        .filter(|(_, c)| script_spk(c.script).is_ok_and(|again| tx.outputs.iter().any(|o| o.script_public_key == again && o.covenant.map(|v| v.covenant_id.as_bytes()) == Some(*lib))))
        .collect();
    // What the transaction is named by: the script that ran an operation, not a copy that was let go beside it.
    let first = calls.iter().find(|(_, c)| c.entry != "release").or(calls.first()).map(|(_, c)| c.clone())?;
    let objects: Vec<(usize, [u8; 32], Option<i64>)> = tx.inputs.iter().enumerate().filter_map(|(i, x)| slot_call(&x.signature_script).filter(|s| s.0 == *lib).map(|s| (i, s.1, s.2))).collect();
    let (mut spent, mut outs, mut template) = (vec![], vec![], None);
    for (at, c) in &calls {
        // Its objects, in the order the script takes them: `who` names the input of its first and of its second,
        // wherever they stand in the transaction (review T1b, F20: a match with the sell at the lower input is a
        // match).
        let mine: Vec<(usize, [u8; 32])> = objects.iter().filter(|o| o.2 == Some(*at as i64) || (o.2.is_none() && calls.len() == 1)).map(|o| (o.0, o.1)).collect();
        let who: Vec<i64> = c.bytes("who").filter(|w| w.len() == 24).map(|w| w.chunks(8).filter_map(num).collect()).unwrap_or_default();
        let mut slots: Vec<(usize, [u8; 32])> = vec![];
        for i in &who {
            if let Some(s) = mine.iter().find(|s| s.0 as i64 == *i) {
                if !slots.contains(s) {
                    slots.push(*s);
                }
            }
        }
        let rest: Vec<(usize, [u8; 32])> = mine.iter().filter(|s| !slots.contains(s)).copied().collect();
        slots.extend(rest);
        let (s, o, t) = read_call(tx, c, &slots, amounts, known);
        spent.extend(s);
        outs.extend(o);
        template = template.or(t);
    }

    // Held to the transaction: an object made is one whose script an output carries, under a covenant id.
    let anchor = object_spk(lib, &TObj::Anchor);
    let scripts: Vec<_> = pinned::manifest().contracts.keys().filter(|k| k.as_str() != SLOT).filter_map(|k| script_spk(k).ok()).collect();
    let (mut made, mut unread) = (vec![], vec![]);
    for (i, o) in tx.outputs.iter().enumerate() {
        let Some(id) = o.covenant.map(|v| v.covenant_id.as_bytes()) else { continue };
        // Under the library's own id there are its scripts and its fee pool, and no object (`lib_check`).
        if scripts.contains(&o.script_public_key) || id == *lib {
            continue;
        }
        if o.script_public_key == anchor {
            made.push((i, TObj::Anchor, id));
            continue;
        }
        match outs.iter().find(|m| object_spk(lib, m) == o.script_public_key) {
            Some(m) => made.push((i, *m, id)),
            None => unread.push(i),
        }
    }
    Some(TxRead { script: first.script, entry: first.entry, spent, made, unread, template })
}
