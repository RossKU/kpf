//! The commands of an account of any number of legs and of the reserve (docs/ARCHITECTURE-CLEARING.md 4b and 6a). As
//! every other command: offline unless `--node` is given, nothing submitted without `--submit`, every transaction
//! verified locally first.
//!
//! A clearing party opens a shard (`shard-open`), funds it (`shard-fund`) and draws from it by a statement
//! (`shard-draw`). A user and the clearing party open a covered account (`acct-open`) and add legs to it
//! (`acct-leg`); each adds margin (`acct-add`). What a side gained is taken by a statement (`acct-take`), a leg
//! leaves by its party's statement (`acct-leave`) or by a step of a liquidation (`acct-liq`), and an account with no
//! leg is ended (`acct-end`). The notes these write are applied to the shard by anyone (`shard-note`). A statement
//! is applied after its dispute period (`stated-settle`), disputed at a point (`stated-dispute`), answered
//! (`stated-answer`), ended where nobody answered (`stated-lapse`), and a draw is ended by a note that is not
//! applied (`draw-note`). `acct-show` prints any of these objects, and of a statement what is wrong with it at
//! prices given.
//!
//! What two parties sign (`acct-open`, `acct-leg`) is signed here with both keys on one machine: these commands
//! write no partly signed file.

use kpf_protocol::aobj::{outcome, AObj, Host};
use kpf_protocol::committee::blake2b;
use kpf_protocol::cross::XLeg;
use kpf_protocol::error::{from_hex, hex32, to_hex};
use kpf_protocol::network::{p2pk_address, Network};
use kpf_protocol::oracle_build::{OBuilt, OLive};
use kpf_protocol::reserve::{Draw, Shard, NOTE_DEPOSIT, NOTE_JOIN, NOTE_OWED, NOTE_RELEASE, SHARD_ROWS};
use kpf_protocol::thin::{leg_bytes, ATake, Account, Pointed, Stated, TObj, KIND_ACCOUNT_LEAVE, KIND_ACCOUNT_LIQ, KIND_ACCOUNT_TAKE, KIND_DRAW};
use kpf_protocol::thin_artifacts as pinned;
use kpf_protocol::thin_build::{self as tb, Payer};
use kpf_protocol::tx::KeySet;
use serde_json::Value;

use crate::args::{fmt_kas, kas, parse_amount, Args};
use crate::ccmds::{cmarket_from, leg_of, terms_of};
use crate::cmds::{connect, e, int_or, network, price, price_or, side, with_node_facts, R};
use crate::files::{read_json, write_json, ObjFile};
use crate::ocmds::{finish_signed, funding, only, out_dir, rate};
use crate::tcmds::{anchor_of, file_of, lib_of, script_of, side_of, word, write_made, Made, N};

/// An object of an account or of the reserve read from its file.
struct ALoaded {
    f: ObjFile,
    lib: [u8; 32],
    obj: AObj,
}

/// The object of the file `--flag`, held to its script (and, with a node, to what the node lists).
fn aload(a: &Args, flag: &str, node: &mut N, net: Network) -> R<ALoaded> {
    let path = a.need(flag)?;
    let v = read_json(path)?;
    let f = with_node_facts(ObjFile::from_json(&v).map_err(|x| format!("{path}: {x}"))?, node, net)?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).ok_or_else(|| format!("{path}: no `{k}`: not the file of an account, a shard, a note or a statement"));
    let obj = AObj::decode(&from_hex(text("clearing")?).map_err(e)?).map_err(|x| format!("{path}: {x}"))?;
    let lib = hex32(text("library")?).map_err(e)?;
    if pinned::kit().read(&f.redeem) != Some((lib, obj.hash())) {
        return Err(format!("{path}: the state in the file is not the one its script carries"));
    }
    // No script ties an object's value to its state: an object that holds less than its state says pays whoever
    // claims first, up to what it holds.
    if (f.amount as i64) < obj.least_value() {
        return Err(format!("{path}: REFUSED: the object holds {} KAS and its state says at least {}", fmt_kas(f.amount as i128), fmt_kas(obj.least_value() as i128)));
    }
    Ok(ALoaded { f, lib, obj })
}

fn alive<T>(l: &ALoaded, s: T, need_daa: bool) -> R<OLive<T>> {
    let daa = if need_daa { l.f.known_daa()? } else { l.f.daa.unwrap_or(0) };
    Ok(OLive { s, value: l.f.amount, id: (l.f.covenant_id != Default::default()).then_some(l.f.covenant_id), op: l.f.op, daa })
}

fn account_of(a: &Args, node: &mut N, net: Network) -> R<(ALoaded, Account)> {
    let l = aload(a, "account", node, net)?;
    match l.obj.clone() {
        AObj::Account(x) => Ok((l, x)),
        o => Err(format!("--account is a {}, not an account", o.kind())),
    }
}

fn shard_of(a: &Args, node: &mut N, net: Network) -> R<(ALoaded, Shard)> {
    let l = aload(a, "shard", node, net)?;
    match l.obj {
        AObj::Shard(s) => Ok((l, s)),
        ref o => Err(format!("--shard is a {}, not a shard", o.kind())),
    }
}

/// The statement of `--stated`: what it is on, the statement, what it states, and who disputed which point.
#[allow(clippy::type_complexity)]
fn stated_of(a: &Args, node: &mut N, net: Network) -> R<(ALoaded, Host, Stated, Vec<u8>, Option<([u8; 32], i64)>)> {
    let l = aload(a, "stated", node, net)?;
    match l.obj.clone() {
        AObj::Stated { on, st, said } => Ok((l, on, st, said, None)),
        AObj::Pointed { on, q, said } => Ok((l, on, q.st, said, Some((q.disputer, q.point)))),
        o => Err(format!("--stated is a {}, not a statement", o.kind())),
    }
}

/// Signs, verifies, prints, saves and (with `--submit`) sends, then writes the files of what the transaction made
/// (under `dry-run/` unless the node accepted it): the objects among `objs` and the copies of the scripts `scripts`
/// are found in the transaction's outputs by their scripts.
#[allow(clippy::too_many_arguments)]
fn adone(a: &Args, title: &str, b: &OBuilt, keys: &KeySet, net: Network, node: &mut N, lib: &[u8; 32], objs: &[AObj], scripts: &[&'static str]) -> R<()> {
    if a.has("partial") {
        return Err(format!("{title} is not written partly signed: give both keys here (--key twice)"));
    }
    let sg = b.sign(keys).map_err(e)?;
    let submit = a.has("submit");
    let r = finish_signed(a, title, &sg, net, node, submit);
    if a.has("no-save") {
        return r;
    }
    let dir = out_dir(a, !(submit && r.is_ok()));
    let tag: String = sg.tx.id().to_string().chars().take(8).collect();
    let mut made = vec![];
    let mut used = vec![];
    for (i, o) in sg.tx.outputs.iter().enumerate() {
        let spk = &o.script_public_key;
        if let Some(n) = scripts.iter().find(|n| tb::script_spk(n).ok().as_ref() == Some(spk)) {
            made.push(Made::Script(i, n));
        } else if *spk == tb::object_spk(lib, &TObj::Anchor) {
            made.push(Made::Obj(i, TObj::Anchor));
        } else if let Some(k) = (0..objs.len()).find(|k| !used.contains(k) && tb::hash_spk(lib, &objs[*k].hash()) == *spk) {
            used.push(k);
            let obj = &objs[k];
            let mut v = file_of(sg.tx.id(), None, &sg.tx, i, pinned::kit().object(lib, &obj.hash()), net);
            v["kind"] = Value::from(obj.kind());
            v["clearing"] = Value::from(to_hex(&obj.encode()));
            v["library"] = Value::from(to_hex(lib));
            v["state"] = Value::from(format!("{obj:?}"));
            let p = dir.join(format!("{tag}-out{i}-{}.json", obj.kind().replace(' ', "-")));
            write_json(&p, &v)?;
            println!("written: {}", p.display());
        }
    }
    write_made(&sg.tx, &made, lib, net, &dir)?;
    if let Some(k) = (0..objs.len()).find(|k| !used.contains(k)) {
        println!("NOTE: no output of the transaction is the {} this command expected: no file of it is written", objs[k].kind());
    }
    r
}

/// Prices as a statement gives them: `--prices 10,10.5`, one a leg (or a row), in KAS.
fn prices_of(a: &Args, n: usize, what: &str) -> R<Vec<i64>> {
    let v = a.need("prices")?.split(',').map(|s| kas(s.trim())).collect::<R<Vec<i64>>>()?;
    if v.len() != n {
        return Err(format!("--prices gives {} prices and {what} has {n}: one each, separated by commas", v.len()));
    }
    Ok(v)
}

fn t0_of(a: &Args) -> R<i64> {
    a.need("t0")?.parse::<i64>().map_err(|_| "--t0 is the DAA score the statement prices at, an integer".to_string())
}

fn leg_line(l: &XLeg) -> String {
    let (units, market) = (crate::args::fmt_units(l.size as i128, l.scale as i128), &to_hex(&l.market)[..8]);
    match kpf_protocol::thin::option_of(l) {
        Some((c, strike)) => format!("{} {} on {units} units of market {market} struck at {} KAS, last counted at {} KAS", if c % 2 == 1 { "holds a" } else { "wrote a" }, if c <= kpf_protocol::thin::CALL_WRITTEN { "call" } else { "put" }, fmt_kas(strike as i128), fmt_kas(l.entry as i128)),
        None => format!("{} {units} units of market {market} from {} KAS", if l.dir == kpf_protocol::state::LONG { "long" } else { "short" }, fmt_kas(l.entry as i128)),
    }
}

/// The direction's place of a leg from `--dir`: long or short; or an option as side A has it (call, put: A holds
/// it; call-written, put-written: A wrote it), struck at `--strike`.
fn dir_of(a: &Args) -> R<i64> {
    use kpf_protocol::thin::{option_dir, CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN};
    let kind = match a.need("dir")?.to_ascii_lowercase().as_str() {
        "call" => CALL_HELD,
        "call-written" => CALL_WRITTEN,
        "put" => PUT_HELD,
        "put-written" => PUT_WRITTEN,
        d => {
            if a.has("strike") {
                return Err("--strike is an option's: --dir call, put, call-written or put-written".into());
            }
            return side(d).map_err(|_| format!("--dir is long or short, or an option as side A has it (call, put, call-written, put-written, with --strike), not `{d}`"));
        }
    };
    Ok(option_dir(kind, price(a, "strike")?))
}

fn show(o: &AObj, net: Network) {
    println!("{}: state hash {}", o.kind(), to_hex(&o.hash()));
    match o {
        AObj::Account(x) => show_account(x, net),
        AObj::Shard(s) => show_shard(s, net),
        AObj::Note(n) => {
            let what = match n.kind {
                NOTE_JOIN => format!("a leg entered: side B has {} units more in market {} (size {})", n.net, &to_hex(&n.head.market)[..8], n.gross),
                NOTE_RELEASE => format!("a leg left: side B has {} units less in market {} (size {})", n.net, &to_hex(&n.head.market)[..8], n.gross),
                NOTE_DEPOSIT => format!("a deposit of {} KAS", fmt_kas(n.amount as i128)),
                NOTE_OWED => format!("{} KAS owed to {} by an account of {}", fmt_kas(n.amount as i128), p2pk_address(&n.key, net), p2pk_address(&n.head.market, net)),
                _ => "of no kind".to_string(),
            };
            println!("  to the shard {}: {what}", to_hex(&n.sid));
        }
        AObj::Stated { on, st, said } => show_stated(on, st, said, None, net),
        AObj::Pointed { on, q, said } => show_stated(on, &q.st, said, Some((q.disputer, q.point)), net),
    }
}

fn show_account(x: &Account, net: Network) {
    println!("  A {} margin {} KAS", p2pk_address(&x.a_key, net), fmt_kas(x.a_margin as i128));
    println!("  B {} margin {} KAS", p2pk_address(&x.b_key, net), fmt_kas(x.b_margin as i128));
    for (i, l) in x.legs.iter().enumerate() {
        println!("  leg {}: A is {}", i + 1, leg_line(l));
    }
    if x.legs.is_empty() {
        println!("  no leg");
    }
    match x.shard {
        Some(sid) => println!("  covered by the shard {}: side B is paid by notes to it, and what the pot cannot pay A is owed by it", to_hex(&sid)),
        None => println!("  covered by no shard"),
    }
    println!("  a statement on it waits {} DAA and posts {} KAS", x.dispute_daa, fmt_kas(x.bond() as i128));
}

fn show_shard(s: &Shard, net: Network) {
    println!("  id {}", to_hex(&s.sid));
    println!("  owner {} holds {} KAS in it", p2pk_address(&s.owner, net), fmt_kas(s.held as i128));
    for (i, r) in s.rows.iter().enumerate() {
        if r.head.market != [0; 32] {
            println!("  row {}: market {}: net {} units, gross {}", i + 1, &to_hex(&r.head.market)[..8], crate::args::fmt_units(r.net as i128, r.head.scale as i128), crate::args::fmt_units(r.gross as i128, r.head.scale as i128));
        }
    }
    println!("  a draw waits {} DAA and posts {} KAS", s.dispute_daa, fmt_kas(s.bond() as i128));
}

fn show_stated(on: &Host, st: &Stated, said: &[u8], pointed: Option<([u8; 32], i64)>, net: Network) {
    let kind = match st.kind {
        KIND_ACCOUNT_TAKE => "a take",
        KIND_ACCOUNT_LIQ => "a step of a liquidation",
        KIND_ACCOUNT_LEAVE => "a leg leaves by its party's statement",
        KIND_DRAW => "a draw",
        _ => "of no kind",
    };
    println!("  {kind}, stated by {} for the time {} with a bond of {} KAS; applied after {} DAA undisputed", p2pk_address(&st.claimer, net), st.t0, fmt_kas(st.bond as i128), st.window);
    match on {
        Host::Shard(_) => {
            if let Some(d) = Draw::read(said) {
                println!("  draws {} KAS at the prices {}", fmt_kas(d.amount as i128), d.prices.iter().map(|p| fmt_kas(*p as i128)).collect::<Vec<_>>().join(" / "));
            }
        }
        Host::Account(_) => {
            if let Some(tk) = ATake::read(said) {
                println!("  side {}, amount {} KAS, leg that leaves: {}, prices {}", word(tk.side), fmt_kas(tk.amount as i128), if tk.skip == 0 { "none".to_string() } else { tk.skip.to_string() }, tk.rows.iter().map(|r| fmt_kas(r.a as i128)).collect::<Vec<_>>().join(" / "));
            }
        }
    }
    for (i, o) in st.outs.list().iter().enumerate() {
        println!("  output {i}: {} KAS to {}", fmt_kas(o.value as i128), if o.key { p2pk_address(&o.to, net) } else if st.outs.is_note(i) { format!("a note {}", &to_hex(&o.to)[..16]) } else { format!("the object as {}", &to_hex(&o.to)[..16]) });
    }
    if let Some((d, point)) = pointed {
        println!("  DISPUTED at the point {point} by {}", p2pk_address(&d, net));
    }
}

/// The first point of the statement that is wrong at the prices `c` (the committee's, as whoever looks has them).
fn wrong_point(on: &Host, st: &Stated, said: &[u8], c: &[i64]) -> R<Option<i64>> {
    match on {
        Host::Shard(s) => {
            let d = Draw::read(said).ok_or("the statement's bytes are not a draw's")?;
            let c: [i64; SHARD_ROWS] = c.try_into().map_err(|_| "a shard has four rows: four prices (0 for a row with no market)".to_string())?;
            Ok(s.draw_wrong_point(&d, &st.claimer, &st.outs, st.held, &c))
        }
        Host::Account(x) => {
            let tk = ATake::read(said).ok_or("the statement's bytes are not a take's or a step's")?;
            if c.len() != x.legs.len() {
                return Err(format!("the account has {} legs: as many prices", x.legs.len()));
            }
            Ok(x.wrong_point_of(st.kind, &tk, &st.claimer, &st.outs, st.held, c))
        }
    }
}

/// `acct-show`: an account, a shard, a note or a statement; and of a statement, with `--prices` (the committee's for
/// its time, one a leg or a row), the first point of it that is wrong.
pub fn show_cmd(a: &Args) -> R<()> {
    a.only(&["object", "node", "network", "prices"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = aload(a, "object", &mut node, net)?;
    show(&l.obj, net);
    println!("holds {} KAS, of library {}", fmt_kas(l.f.amount as i128), to_hex(&l.lib));
    let (on, st, said) = match &l.obj {
        AObj::Stated { on, st, said } => (on, st, said),
        AObj::Pointed { on, q, said } => (on, &q.st, said),
        AObj::Shard(s) if a.has("prices") => {
            let p: [i64; SHARD_ROWS] = prices_of(a, SHARD_ROWS, "a shard")?.try_into().expect("four");
            match s.requirement(&p) {
                Some(need) => println!("at these prices it is asked {} KAS and holds {}: {} KAS can be drawn", fmt_kas(need as i128), fmt_kas(s.held as i128), fmt_kas(s.draw_max(&p) as i128)),
                None => println!("at these prices its requirement is not computed (a market with no price within its limit, or numbers beyond what a script computes)"),
            }
            return Ok(());
        }
        _ => return Ok(()),
    };
    if a.has("prices") {
        let n = match on {
            Host::Shard(_) => SHARD_ROWS,
            Host::Account(x) => x.legs.len(),
        };
        match wrong_point(on, st, said, &prices_of(a, n, "the statement")?)? {
            None => println!("at these prices the statement is the rule's"),
            Some(p) => println!("at these prices the statement is WRONG at the point {p} (0 its end; on an account 2i-1 the row i, 2i the price of the row i; on a shard r the price of the row r): `kpf stated-dispute --stated FILE --point {p}`"),
        }
    }
    Ok(())
}

/// `shard-open`: the funder opens a reserve shard of the library `--lib` through an anchor of its registry, holding
/// `--held` KAS. Its terms are those of the market `--market1` (the maintenance rate is the rate on the net, the
/// liquidation fee's rate the rate on the gross); a draw from it waits `--dispute-daa` and posts `--bond-bps` of
/// what it holds.
pub fn shard_open_cmd(a: &Args) -> R<()> {
    only(a, &["market1", "held", "lib", "lib-unchecked", "dispute-daa", "bond-bps", "exit-daa", "anchor", "script", "registry", "registry-unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let m = cmarket_from(a.need("market1")?)?.0;
    let anchor = anchor_of(a, &lib, &mut node, net)?;
    let s = script_of(a, &lib, "KpfTShardOpen", &mut node, net)?;
    // (A shard is of one committee: the market's, whose prices every account under it is counted at.)
    let (b, shard) = tb::shard_open(&lib, &anchor, &s, terms_of(&m, None), m.committee_id, fk, price(a, "held")?, int_or(a, "exit-daa", 864_000)?, int_or(a, "dispute-daa", 600)?, int_or(a, "bond-bps", 1_000)?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Shard(shard), net);
    println!("seed {} (an account under this shard is opened with it and the market's file: acct-open --seed --market1; the shard's id is the hash of it, of the committee, and of the terms and periods every account under it has)", to_hex(&tb::shard_seed(&anchor)));
    adone(a, "shard-open", &b, &keys, net, &mut node, &lib, &[AObj::Shard(shard)], &["KpfTShardOpen"])
}

/// `shard-fund`: the funder adds `--amount` KAS to what the shard `--shard` holds.
pub fn shard_fund_cmd(a: &Args) -> R<()> {
    only(a, &["shard", "amount", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, shard) = shard_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTAddC", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next) = tb::shard_fund(&l.lib, &alive(&l, shard, false)?, &s, price(a, "amount")?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Shard(next), net);
    adone(a, "shard-fund", &b, &keys, net, &mut node, &l.lib, &[AObj::Shard(next)], &["KpfTAddC"])
}

/// `shard-draw`: the shard's owner states that it draws `--amount` KAS (default: all above the shard's requirement,
/// less `--keep`) at the prices `--prices` (four, one a row; 0 for a row with no market) of the time `--t0`. Applied
/// after the shard's dispute period (`stated-settle`). Apply every note to the shard first: a note that landed
/// before the statement and is not applied ends it and loses the bond.
pub fn shard_draw_cmd(a: &Args) -> R<()> {
    only(a, &["shard", "amount", "keep", "prices", "t0", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, shard) = shard_of(a, &mut node, net)?;
    let p: [i64; SHARD_ROWS] = prices_of(a, SHARD_ROWS, "a shard")?.try_into().expect("four");
    let amount = if a.has("amount") { price(a, "amount")? } else { (shard.draw_max(&p) - price_or(a, "keep", 0)?).max(0) };
    let s = script_of(a, &l.lib, "KpfTChange", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, st, d) = tb::shard_draw(&l.lib, &alive(&l, shard, false)?, &s, amount, p, t0_of(a)?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let o = AObj::Stated { on: Host::Shard(shard), st, said: d.said() };
    show(&o, net);
    adone(a, "shard-draw", &b, &keys, net, &mut node, &l.lib, &[o], &["KpfTChange"])
}

/// `shard-note`: anyone applies the note `--note` to its shard `--shard`: a leg entered or left (the shard's row of
/// its market changes), a deposit (the shard holds it), or what a user is owed (the shard pays it, as far as it
/// holds).
pub fn shard_note_cmd(a: &Args) -> R<()> {
    only(a, &["shard", "note", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, shard) = shard_of(a, &mut node, net)?;
    let nl = aload(a, "note", &mut node, net)?;
    let AObj::Note(note) = nl.obj else { return Err(format!("--note is a {}, not a note to a shard", nl.obj.kind())) };
    if nl.lib != l.lib {
        return Err("--note is of another library than the shard".into());
    }
    let s = script_of(a, &l.lib, "KpfTNoteApply", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next, rest) = tb::note_apply(&l.lib, &alive(&l, shard, false)?, &alive(&nl, note, false)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Shard(next), net);
    let mut objs = vec![AObj::Shard(next)];
    if let Some(r) = rest {
        println!("the shard paid what it held: {} KAS are still owed, and the note goes on with them", fmt_kas(r.amount as i128));
        objs.push(AObj::Note(r));
    }
    adone(a, "shard-note", &b, &keys, net, &mut node, &l.lib, &objs, &["KpfTNoteApply"])
}

/// `acct-open`: a covered account between `--a` (the user) and `--b` (the clearing party, the owner of the shard
/// `--shard`) in the shard's library, through an anchor of its registry, with the margins `--a-margin` and
/// `--b-margin` and no leg. Its terms are the shard's. Both sign: both keys are given here (`--key` twice).
pub fn open_cmd(a: &Args) -> R<()> {
    only(a, &["shard", "seed", "market1", "a", "b", "a-margin", "b-margin", "dispute-daa", "bond-bps", "exit-daa", "anchor", "script", "registry", "registry-unchecked", "lib", "lib-unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, shard) = shard_of(a, &mut node, net)?;
    let (a_key, b_key) = (crate::keys::party(a.need("a")?, net)?, crate::keys::party(a.need("b")?, net)?);
    if b_key != shard.owner {
        return Err("REFUSED: --b is not the owner of --shard: the shard applies no note of such an account and pays nothing it owes".into());
    }
    let x = Account { t: shard.t, legs: vec![], a_key, b_key, a_margin: price(a, "a-margin")?, b_margin: price_or(a, "b-margin", 0)?, expiry: int_or(a, "exit-daa", 864_000)?, dispute_daa: int_or(a, "dispute-daa", shard.dispute_daa)?, bond_bps: int_or(a, "bond-bps", shard.bond_bps)?, a_min_counter: 0, b_min_counter: 0, shard: Some(shard.sid) };
    // (An account has its shard's terms and periods: the dispute period and the bond rate are the shard's unless
    // given, and given otherwise the opening is refused. The shard's id is opened by its seed and its committee.)
    let seed = hex32(a.need("seed")?).map_err(e)?;
    let committee = cmarket_from(a.need("market1")?)?.0.committee_id;
    let anchor = anchor_of(a, &l.lib, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTOpenC", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::account_open_c(&l.lib, &anchor, &s, &x, seed, committee, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Account(x.clone()), net);
    adone(a, "acct-open", &b, &keys, net, &mut node, &l.lib, &[AObj::Account(x)], &["KpfTOpenC"])
}

/// `acct-leg`: a leg is added to the covered account `--account`: A is `--dir` (long or short) `--size` units of
/// `--market` from the price `--x`; or A holds or wrote an option on them (`--dir` call, put, call-written or
/// put-written, struck at `--strike`, counted from `--x`). A adds `--a-add` KAS of margin and B `--b-add`. Both sign (`--key` twice). The
/// note that the leg entered is written beside it: apply it to the shard (`shard-note`).
pub fn leg_cmd(a: &Args) -> R<()> {
    only(a, &["account", "market", "size", "dir", "strike", "x", "a-add", "b-add", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let m = cmarket_from(a.need("market")?)?.0;
    let leg = leg_of(&m, parse_amount(a.need("size")?, m.p.scale)?, dir_of(a)?, price(a, "x")?);
    let s = script_of(a, &l.lib, "KpfTLegAdd", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next, note) = tb::leg_add(&l.lib, &alive(&l, x, false)?, &s, leg, price_or(a, "a-add", 0)?, price_or(a, "b-add", 0)?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Account(next.clone()), net);
    adone(a, "acct-leg", &b, &keys, net, &mut node, &l.lib, &[AObj::Account(next), AObj::Note(note)], &["KpfTLegAdd"])
}

/// `acct-add`: `--side` (a or b) adds `--amount` KAS to its margin in the covered account `--account`.
pub fn add_cmd(a: &Args) -> R<()> {
    only(a, &["account", "side", "amount", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTAddC", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next) = tb::account_add(&l.lib, &alive(&l, x, false)?, &s, side_of(a.need("side")?)?, price(a, "amount")?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Account(next.clone()), net);
    adone(a, "acct-add", &b, &keys, net, &mut node, &l.lib, &[AObj::Account(next)], &["KpfTAddC"])
}

/// `acct-take`: the funder states that the entries of `--account` move to the prices `--prices` (one a leg) of the
/// time `--t0`, so that what `--side` gained since is margin of its own, and that `--amount` KAS of it are taken out
/// (default: the most the side takes, less `--keep`). The funder is the side's key; or anyone, for side B of a
/// covered account, whose amount is then a deposit to the account's shard.
pub fn take_cmd(a: &Args) -> R<()> {
    only(a, &["account", "side", "amount", "keep", "prices", "t0", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let sd = side_of(a.need("side")?)?;
    let p = prices_of(a, x.legs.len(), "the account")?;
    let amount = if a.has("amount") { price(a, "amount")? } else { (x.take_max(sd, &p) - price_or(a, "keep", 0)?).max(0) };
    let s = script_of(a, &l.lib, "KpfTChange", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, st, tk) = tb::account_take_by(&l.lib, &alive(&l, x.clone(), false)?, &s, sd, amount, &p, t0_of(a)?, fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let o = AObj::Stated { on: Host::Account(x), st, said: tk.said() };
    show(&o, net);
    adone(a, "acct-take", &b, &keys, net, &mut node, &l.lib, &[o], &["KpfTChange"])
}

/// `acct-out`: the leg `--leg` (1, 2, ...) leaves the covered account `--account` at once, by its two parties'
/// signatures (`--key` twice): the way out that waits for nobody. What the leg is worth is what the two agreed: the
/// price `--x` it is settled at (its gain from its entry; for an option, what it is worth exercised there less what
/// it was counted at), or the amount itself, `--pay` KAS to A or `--pay-b` KAS to B (the price of an option bought
/// back). What A owes moves from its margin to B's; what B owes it pays A from the funder's coins, in the same
/// transaction (B's margin in the account does not fall here: it leaves by the shard). A takes `--take` KAS out of
/// its margin with it and B adds `--b-add`. The note that the leg left is written beside it: apply it to the shard (`shard-note`).
/// No script reads a price here, and the clearing party signs or does not: a user it does not answer leaves by
/// `acct-leave` after the dispute period.
pub fn out_cmd(a: &Args) -> R<()> {
    only(a, &["account", "leg", "x", "pay", "pay-b", "take", "b-add", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let i = int_or(a, "leg", 1)?;
    let Some(leg) = usize::try_from(i - 1).ok().and_then(|k| x.legs.get(k)) else { return Err(format!("--leg {i}: the account has {} legs", x.legs.len())) };
    let pay = match (a.get("x").is_some(), a.get("pay").is_some() || a.get("pay-b").is_some()) {
        (true, false) => {
            let p = price(a, "x")?;
            if p <= 0 || p > leg.price_max {
                return Err("--x is a price above zero and within the market's limit".into());
            }
            kpf_protocol::thin::leg_gain_at(leg, p)
        }
        (false, true) => price_or(a, "pay", 0)? - price_or(a, "pay-b", 0)?,
        _ => return Err("give --x, the price the leg is settled at, or what the two agreed: --pay KAS (to A, from the funder's coins) or --pay-b KAS (from A's margin to B's)".into()),
    };
    println!("{} leaves at once: {} KAS {}", leg_line(leg), fmt_kas(pay.abs() as i128), if pay >= 0 { "are paid to A from the funder's coins" } else { "move from A's margin to B's" });
    let s = script_of(a, &l.lib, "KpfTLegOut", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next, note) = tb::leg_out(&l.lib, &alive(&l, x, false)?, i as usize, (-pay).max(0), pay.max(0), price_or(a, "take", 0)?, price_or(a, "b-add", 0)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&AObj::Account(next.clone()), net);
    adone(a, "acct-out", &b, &keys, net, &mut node, &l.lib, &[AObj::Account(next), AObj::Note(note)], &["KpfTLegOut"])
}

/// `acct-leave`: the owner of `--side` states that the leg `--leg` (1, 2, ...) leaves `--account` at the prices
/// `--prices` of the time `--t0`: its gain or loss is settled between the margins, and on a covered account what
/// the pot cannot pay side A is a note of what its shard owes.
pub fn leave_cmd(a: &Args) -> R<()> {
    only(a, &["account", "side", "leg", "prices", "t0", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let p = prices_of(a, x.legs.len(), "the account")?;
    let s = script_of(a, &l.lib, "KpfTChange", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, st, tk, stepped) = tb::account_leave(&l.lib, &alive(&l, x.clone(), false)?, &s, side_of(a.need("side")?)?, int_or(a, "leg", 1)?, &p, t0_of(a)?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    if stepped.owed > 0 {
        println!("the pot does not hold all of side A's gain: {} KAS are owed by the shard", fmt_kas(stepped.owed as i128));
    }
    let o = AObj::Stated { on: Host::Account(x), st, said: tk.said() };
    show(&o, net);
    adone(a, "acct-leave", &b, &keys, net, &mut node, &l.lib, &[o], &["KpfTChange"])
}

/// `acct-liq`: the funder states that `--side` of `--account` stands below its maintenance at the prices `--prices`
/// of the time `--t0`, and that the leg `--leg` leaves it: one step of a liquidation, for the liquidation fee of
/// the leg's worth.
pub fn liq_cmd(a: &Args) -> R<()> {
    only(a, &["account", "side", "leg", "prices", "t0", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let p = prices_of(a, x.legs.len(), "the account")?;
    let s = script_of(a, &l.lib, "KpfTChange", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, st, tk) = tb::account_liq(&l.lib, &alive(&l, x.clone(), false)?, &s, side_of(a.need("side")?)?, int_or(a, "leg", 1)?, &p, t0_of(a)?, fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let o = AObj::Stated { on: Host::Account(x), st, said: tk.said() };
    show(&o, net);
    adone(a, "acct-liq", &b, &keys, net, &mut node, &l.lib, &[o], &["KpfTChange"])
}

/// `acct-end`: `--side` ends the covered account `--account`, which has no leg: side A has its margin, and what the
/// clearing side had is a deposit to the shard.
pub fn end_cmd(a: &Args) -> R<()> {
    only(a, &["account", "side", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = account_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTAcctEnd", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, deposit) = tb::acct_end(&l.lib, &alive(&l, x.clone(), false)?, &s, side_of(a.need("side")?)?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("ended: {} KAS to side A{}", fmt_kas(x.a_margin as i128), deposit.map(|d| format!(", and a deposit of {} KAS to the shard", fmt_kas(d.amount as i128))).unwrap_or_default());
    let objs: Vec<AObj> = deposit.into_iter().map(AObj::Note).collect();
    adone(a, "acct-end", &b, &keys, net, &mut node, &l.lib, &objs, &["KpfTAcctEnd"])
}

/// `stated-settle`: anyone applies the statement `--stated` nobody disputed, once it is a dispute period old: its
/// outputs are written as it stated them, and the bond returns to the claimer.
pub fn settle_cmd(a: &Args) -> R<()> {
    only(a, &["stated", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, on, st, said, pointed) = stated_of(a, &mut node, net)?;
    if pointed.is_some() {
        return Err("the statement is disputed: it is answered (stated-answer) or lapses (stated-lapse)".into());
    }
    let s = script_of(a, &l.lib, "KpfTChangeSettle", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::stated_settle(&l.lib, &alive(&l, st, true)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let objs = outcome(&on, &st, &said);
    println!("applied as stated: the bond of {} KAS returns to the claimer", fmt_kas(st.bond as i128));
    if objs.iter().any(|o| matches!(o, AObj::Note(_))) {
        println!("it writes notes to the shard: apply each (`kpf shard-note --shard FILE --note FILE`)");
    }
    adone(a, "stated-settle", &b, &keys, net, &mut node, &l.lib, &objs, &["KpfTChangeSettle"])
}

/// `stated-dispute`: the funder disputes one point of the statement `--stated`, with a bond as large as the
/// claimer's: `--point N`, or the first point that is wrong at `--prices` (the committee's for the statement's time,
/// as the funder has them: one a leg, or four for a draw).
pub fn dispute_cmd(a: &Args) -> R<()> {
    only(a, &["stated", "point", "prices", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, on, st, said, pointed) = stated_of(a, &mut node, net)?;
    if pointed.is_some() {
        return Err("the statement is disputed already".into());
    }
    let point = if a.has("point") {
        int_or(a, "point", 0)?
    } else {
        let n = match &on {
            Host::Shard(_) => SHARD_ROWS,
            Host::Account(x) => x.legs.len(),
        };
        wrong_point(&on, &st, &said, &prices_of(a, n, "the statement")?)?.ok_or("REFUSED: at these prices the statement is the rule's: a dispute would be answered right and lose its bond")?
    };
    let s = script_of(a, &l.lib, "KpfTPoint", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, q) = tb::point_dispute(&l.lib, &alive(&l, st, true)?, &s, fk, point, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let o = AObj::Pointed { on, q, said };
    show(&o, net);
    adone(a, "stated-dispute", &b, &keys, net, &mut node, &l.lib, &[o], &["KpfTPoint"])
}

/// `stated-lapse`: a disputed statement nobody answered within the dispute period: the object again, both bonds back.
pub fn lapse_cmd(a: &Args) -> R<()> {
    only(a, &["stated", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, on, st, _, pointed) = stated_of(a, &mut node, net)?;
    let (disputer, point) = pointed.ok_or("the statement is not disputed: it is applied (stated-settle)")?;
    let s = script_of(a, &l.lib, "KpfTPoint", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::point_lapse(&l.lib, &alive(&l, Pointed { st, disputer, point }, true)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("lapsed: the object again, and both bonds back");
    adone(a, "stated-lapse", &b, &keys, net, &mut node, &l.lib, &[host_obj(&on)], &["KpfTPoint"])
}

fn host_obj(on: &Host) -> AObj {
    match on {
        Host::Account(x) => AObj::Account(x.clone()),
        Host::Shard(s) => AObj::Shard(*s),
    }
}

/// How a row of a trace is opened: the chain before it and the leg, of the account's own chain. A row that carries
/// no chain of the account is opened by nothing (and one that carries the seed of every chain needs no opening).
fn opening(x: &Account, g: &[u8; 32]) -> Option<([u8; 32], XLeg)> {
    let mut prev = [0u8; 32];
    for l in &x.legs {
        let next = blake2b(&[&prev[..], &leg_bytes(l)].concat());
        if next == *g {
            return Some((prev, *l));
        }
        prev = next;
    }
    (*g == [0; 32]).then(|| ([0; 32], x.legs.first().copied().unwrap_or_else(XLeg::none)))
}

/// `stated-answer`: anyone answers a point of the disputed statement `--stated`: the point disputed, or `--at N`,
/// another point that is wrong (a wrong point ends a disputed statement whichever was named). A price (on an
/// account the point 2i; on a draw the point r of a row with a market) is answered with the committee's, read in
/// the citation of `--note` (a price note's file; `--node`). Answered right, the statement stands again and the
/// claimer has the disputer's bond; answered wrong, the object again, and half of the claimer's bond to the funder.
pub fn answer_cmd(a: &Args) -> R<()> {
    only(a, &["stated", "at", "note", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, on, st, said, pointed) = stated_of(a, &mut node, net)?;
    let (disputer, point) = pointed.ok_or("the statement is not disputed: `kpf stated-dispute` first")?;
    let at = int_or(a, "at", point)?;
    let q = alive(&l, Pointed { st, disputer, point }, true)?;
    let mut cite = None;
    if a.has("note") {
        let (cited, commit) = crate::ncmds::cited_of(a.need("note")?, node.as_mut().ok_or("--node URL: a citation is read at the node")?)?;
        kpf_protocol::verify::set_chain_view(vec![(cited.witness.block, commit)]);
        cite = Some((crate::ncmds::copy_at(&l.lib, "KpfTCite", &mut node, net, &[])?, cited));
    }
    let pair = cite.as_ref().map(|(c, n)| (c, n));
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let p = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let (b, right, script) = match &on {
        Host::Shard(sh) => {
            let d = Draw::read(&said).ok_or("the statement's bytes are not a draw's")?;
            if at == 0 {
                let s = script_of(a, &l.lib, "KpfTShardEnd", &mut node, net)?;
                let (b, r) = tb::shard_end(&l.lib, &q, sh, &d, &s, fk, p, None).map_err(e)?;
                (b, r, "KpfTShardEnd")
            } else {
                let s = script_of(a, &l.lib, "KpfTShardPrice", &mut node, net)?;
                let (b, r) = tb::shard_price(&l.lib, &q, sh, &d, at, &s, pair, fk, p, None).map_err(e)?;
                (b, r, "KpfTShardPrice")
            }
        }
        Host::Account(x) => {
            let tk = ATake::read(&said).ok_or("the statement's bytes are not a take's or a step's")?;
            if at == 0 {
                let name = if st.kind == KIND_ACCOUNT_TAKE { "KpfTStepEnd" } else { "KpfTStepLiq" };
                let s = script_of(a, &l.lib, name, &mut node, net)?;
                let (b, r) = tb::step_end(&l.lib, &q, x, &tk, &s, fk, p, None).map_err(e)?;
                (b, r, name)
            } else if at % 2 == 1 || (at as usize + 1) / 2 > tk.rows.len() {
                // A row; or a point beyond the trace, which the row's script answers right.
                let i = ((at + 1) / 2) as usize;
                let g = tk.rows.get(i - 1).map(|r| r.g).unwrap_or([0; 32]);
                let (prev, leg) = opening(x, &g).ok_or("the row carries a chain that is none of the account's: nobody opens it. The row after it (or the end, if it is the last) is the point that is wrong: answer that one (--at)")?;
                let s = script_of(a, &l.lib, "KpfTStepRow", &mut node, net)?;
                let (b, r) = tb::step_row_at(&l.lib, &q, &tk, at, &prev, &leg, &s, fk, p, None).map_err(e)?;
                (b, r, "KpfTStepRow")
            } else {
                if at != point {
                    return Err("a price is answered only where it is the point disputed".into());
                }
                let (copy, cited) = pair.ok_or("a price is answered with the committee's: --note FILE (a price note of the leg's market at the statement's time) and --node URL")?;
                let s = script_of(a, &l.lib, "KpfTStepPrice", &mut node, net)?;
                let (b, r) = tb::step_price(&l.lib, &q, x, &tk, &s, (copy, cited), fk, p, None).map_err(e)?;
                (b, r, "KpfTStepPrice")
            }
        }
    };
    let objs = if right {
        println!("answered: the point {at} is RIGHT: the statement stands again, and the claimer has the disputer's bond");
        vec![AObj::Stated { on, st: st.again(), said }]
    } else {
        println!("answered: the point {at} is WRONG: the object again; half of the claimer's bond, {} KAS, to the funder", fmt_kas((st.bond / 2) as i128));
        vec![host_obj(&on)]
    };
    adone(a, "stated-answer", &b, &keys, net, &mut node, &l.lib, &objs, &[script, "KpfTCite"])
}

/// `draw-note`: anyone ends the stated draw `--stated` (disputed or not) by the note `--note` to its shard that
/// landed before the statement and is not applied: the shard again, the claimer's bond lost, half of it to the
/// funder.
pub fn draw_note_cmd(a: &Args) -> R<()> {
    only(a, &["stated", "note", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, on, st, _, pointed) = stated_of(a, &mut node, net)?;
    let Host::Shard(shard) = on else { return Err("--stated is not a draw: it is stated on an account".into()) };
    let nl = aload(a, "note", &mut node, net)?;
    let AObj::Note(note) = nl.obj else { return Err(format!("--note is a {}, not a note to a shard", nl.obj.kind())) };
    let s = script_of(a, &l.lib, "KpfTDrawNote", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::draw_note(&l.lib, &alive(&l, (), true)?, &st, pointed, &shard, &alive(&nl, note, true)?, &s, fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("ended: the shard again; of the claimer's bond, {} KAS to the funder", fmt_kas((st.bond / 2) as i128));
    adone(a, "draw-note", &b, &keys, net, &mut node, &l.lib, &[AObj::Shard(shard), AObj::Note(note)], &["KpfTDrawNote"])
}
