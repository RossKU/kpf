//! The commands of committee markets (docs/DESIGN-COMMITTEE.md): the committee (`committee-*`, `epoch-table-retire`,
//! `exit-withdraw`), signed signed prices (`price-sign`, `price-check`), and markets priced by a committee's epoch tables (`c-*`). Offline
//! unless `--node` is given, nothing submitted without `--submit`, every transaction verified locally first, as every
//! other command. A command with `--node` makes one query per object file it is given, one for the funder's UTXOs and
//! at most three for the DAA score and the submission; none of them polls.

use std::path::PathBuf;

use kaspa_consensus_core::tx::TransactionOutpoint;
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::*;
use kpf_protocol::cross::{Cross, XLeg, XPending, XSettlement, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::error::{from_hex, to_hex};
use kpf_protocol::network::{p2pk_address, p2sh_address, Network};
use kpf_protocol::oracle::feed_hash_of;
use kpf_protocol::oracle_build::{OLive, OObj};
use kpf_protocol::payoff::q;
use kpf_protocol::state::{BPS, KIND_CLOSE, KIND_LIQ, KIND_TRIM, LONG, SHORT};
use serde_json::{json, Value};

use crate::args::*;
use crate::cmds::{connect, e, int, int_or, network, price, price_or, side, R};
use crate::files::*;
use crate::keys;
use crate::node::{Node, WsTransport};
use crate::ocmds::{finish, funding, hex32, live_of, load, now, only, rate};

const DAA_PER_SECOND: f64 = 10.0;

/// What `c-watch`'s error starts with when something needs an action now (exit code 3).
pub const ACT_NOW: &str = "ACT NOW";

/// A span of DAA as time, for other modules.
pub(crate) fn secs_of(daa: i64) -> String {
    secs(daa)
}

fn secs(daa: i64) -> String {
    let s = daa as f64 / DAA_PER_SECOND;
    if s.abs() >= 7200.0 {
        format!("{:.1} h", s / 3600.0)
    } else if s.abs() >= 120.0 {
        format!("{:.1} min", s / 60.0)
    } else {
        format!("{s:.1} s")
    }
}

type N = Option<Node<WsTransport>>;

fn cload(a: &Args, flag: &str, node: &mut N, net: Network) -> R<(ObjFile, CObj)> {
    match load(a, flag, node, net)? {
        (f, OObj::C(c)) => Ok((f, c)),
        (_, o) => Err(format!("--{flag} is a {}, not an object of the committee family", o.kind())),
    }
}

macro_rules! loader {
    ($name:ident, $variant:ident, $ty:ty, $what:literal) => {
        pub(crate) fn $name(a: &Args, flag: &str, node: &mut N, net: Network) -> R<OLive<$ty>> {
            match cload(a, flag, node, net)? {
                (f, CObj::$variant(x)) => live_of(&f, x),
                (_, o) => Err(format!("--{flag} is a {}, not a {}", o.kind(), $what)),
            }
        }
    };
}
loader!(committee_of, Committee, Committee, "committee");
loader!(epoch_table_of, EpochTable, EpochTable, "epoch-table");
loader!(exit_of, Exit, Exit, "exit");
loader!(order_of, Order, COpen, "committee order");
loader!(pair_of, Pair, CPair, "committee pair");
loader!(pending_of, Pending, CPending, "committee pending part");
loader!(charge_of, Charge, Charge, "charge");
loader!(cross_of, Cross, Cross, "cross account");
loader!(xpending_of, PendingX, XPending, "cross pending claim");

// ---------------------------------------------------------------- the committee

fn params_json(p: &CommitteeParams) -> Value {
    let j = &p.charge.judge;
    json!({
        "epochDaa": p.epoch, "exitDaa": p.exit, "minMemberStake": p.min_member_stake, "quorumBps": p.quorum_bps, "spreadBps": p.spread_bps, "minTotal": p.min_total, "epochTables": p.epoch_tables, "epochTableValue": p.epoch_table_value,
        "chargeBond": p.charge.bond, "chargeDaa": p.charge.daa,
        "judge": { "id": to_hex(&j.id), "muBps": j.p.mu_bps, "ageDaa": j.p.age, "unbondDaa": j.p.unbond, "roundDaa": j.p.round, "burnBps": j.p.burn_bps, "minStake": j.p.min_stake },
    })
}

fn params_from(v: &Value) -> R<CommitteeParams> {
    let g = |k: &str| v.get(k).and_then(Value::as_i64).ok_or(format!("committee parameters: no `{k}`"));
    let jv = v.get("judge").ok_or("committee parameters: no `judge`")?;
    let j = |k: &str| jv.get(k).and_then(Value::as_i64).ok_or(format!("committee parameters: no `judge.{k}`"));
    let judge = kpf_protocol::judge::JudgeMarket {
        id: hex32(jv.get("id").and_then(Value::as_str).ok_or("committee parameters: no `judge.id`")?)?,
        p: kpf_protocol::judge::JudgeParams { mu_bps: j("muBps")?, age: j("ageDaa")?, unbond: j("unbondDaa")?, round: j("roundDaa")?, burn_bps: j("burnBps")?, min_stake: j("minStake")? },
    };
    Ok(CommitteeParams {
        epoch: g("epochDaa")?,
        exit: g("exitDaa")?,
        min_member_stake: g("minMemberStake")?,
        quorum_bps: g("quorumBps")?,
        spread_bps: g("spreadBps")?,
        min_total: g("minTotal")?,
        epoch_tables: g("epochTables")?,
        epoch_table_value: g("epochTableValue")?,
        charge: ChargeParams { judge, bond: g("chargeBond")?, daa: g("chargeDaa")? },
    })
}

/// The facts a wallet recomputes a committee id from: the genesis outpoint that authorized it, the output index and value
/// of the empty committee, its parameters and first epoch.
#[derive(Clone, Debug)]
pub(crate) struct Genesis {
    auth: TransactionOutpoint,
    index: u32,
    value: u64,
    p: CommitteeParams,
    from: i64,
}

impl Genesis {
    fn id(&self) -> Hash {
        committee_id_of(&self.auth, self.index, self.value, &Committee::empty(self.p, self.from))
    }
    fn to_json(&self) -> Value {
        json!({ "format": "kpf-committee/1", "committeeId": to_hex(&self.id().as_bytes()), "genesis": { "authorizedBy": outpoint_json(&self.auth), "index": self.index, "value": self.value, "from": self.from }, "params": params_json(&self.p) })
    }
    fn from_json(v: &Value) -> R<Genesis> {
        let v = v.get("committee").unwrap_or(v);
        let g = v.get("genesis").ok_or("no `genesis` (the facts the committee id is recomputed from)")?;
        let n = |k: &str| g.get(k).and_then(Value::as_i64).ok_or(format!("committee genesis: no `{k}`"));
        let gen = Genesis { auth: outpoint_from(g.get("authorizedBy").ok_or("committee genesis: no `authorizedBy`")?)?, index: n("index")? as u32, value: n("value")? as u64, p: params_from(v.get("params").ok_or("no committee `params`")?)?, from: n("from")? };
        if let Some(id) = v.get("committeeId").and_then(Value::as_str) {
            if hex32(id)? != gen.id().as_bytes() {
                return Err("the committee id is not the id of an empty committee with this genesis and these parameters: REFUSED (a committee that did not begin empty can hold forged epoch tables)".into());
            }
        }
        Ok(gen)
    }
}

fn committee_words(p: &CommitteeParams) -> String {
    let mut words = committee_words_plain(p);
    let c = &p.charge;
    words.push_str(&format!("\ncharges: a bond of {} KAS, {} DAA ({}) to contest; judged by stakes of at least {} KAS aged {} DAA under the judge block {}, in rounds of {} DAA", fmt_kas(c.bond as i128), c.daa, secs(c.daa), fmt_kas(c.judge.p.min_stake as i128), c.judge.p.age, &to_hex(&c.judge.id)[..16], c.judge.p.round));
    for w in charge_warnings(p) {
        words.push_str(&format!("\nWARNING: {w}"));
    }
    words
}

fn committee_words_plain(p: &CommitteeParams) -> String {
    format!(
        "epochs of {} DAA ({}), a member's stake is released {} DAA ({}) after its last epoch, a member needs at least {} KAS, a price needs {} % of the weight to have signed within {} % of it, no epoch table below {} KAS of total stake, {} epoch tables an epoch of {} KAS each",
        p.epoch,
        secs(p.epoch),
        p.exit,
        secs(p.exit),
        fmt_kas(p.min_member_stake as i128),
        p.quorum_bps as f64 / 100.0,
        p.spread_bps as f64 / 100.0,
        fmt_kas(p.min_total as i128),
        p.epoch_tables,
        fmt_kas(p.epoch_table_value as i128)
    )
}

/// `committee-create`: a new, empty committee. Writes the committee's object file and `committee.json` (its id and the genesis
/// facts a wallet recomputes the id from).
pub fn committee_create_cmd(a: &Args) -> R<()> {
    only(a, &["epoch-daa", "exit-daa", "min-member-stake", "quorum-bps", "spread-bps", "min-total", "epoch-tables", "epoch-table-value", "from", "out", "charge-bond", "charge-daa", "judge-id", "judge-mu-bps", "judge-age-daa", "judge-unbond-daa", "judge-round-daa", "judge-burn-bps", "judge-min-stake"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let d = CommitteeParams::testnet();
    let amt = |name: &str, dv: i64| -> R<i64> { if a.has(name) { kas(a.need(name)?) } else { Ok(dv) } };
    let p = CommitteeParams {
        epoch: int_or(a, "epoch-daa", d.epoch)?,
        exit: int_or(a, "exit-daa", d.exit)?,
        min_member_stake: amt("min-member-stake", d.min_member_stake)?,
        quorum_bps: int_or(a, "quorum-bps", d.quorum_bps)?,
        spread_bps: int_or(a, "spread-bps", d.spread_bps)?,
        min_total: amt("min-total", d.min_total)?,
        epoch_tables: int_or(a, "epoch-tables", d.epoch_tables)?,
        epoch_table_value: amt("epoch-table-value", d.epoch_table_value)?,
        charge: ChargeParams {
            judge: kpf_protocol::judge::JudgeMarket {
                id: if a.has("judge-id") { hex32(a.need("judge-id")?)? } else { d.charge.judge.id },
                p: kpf_protocol::judge::JudgeParams {
                    mu_bps: int_or(a, "judge-mu-bps", d.charge.judge.p.mu_bps)?,
                    age: int_or(a, "judge-age-daa", d.charge.judge.p.age)?,
                    unbond: int_or(a, "judge-unbond-daa", d.charge.judge.p.unbond)?,
                    round: int_or(a, "judge-round-daa", d.charge.judge.p.round)?,
                    burn_bps: int_or(a, "judge-burn-bps", d.charge.judge.p.burn_bps)?,
                    min_stake: amt("judge-min-stake", d.charge.judge.p.min_stake)?,
                },
            },
            bond: amt("charge-bond", d.charge.bond)?,
            daa: int_or(a, "charge-daa", d.charge.daa)?,
        },
    };
    p.check().map_err(|x| format!("the parameters fail the contract's bounds: {x}"))?;
    p.charge.judge.p.check().map_err(|x| format!("the judge block fails the judge's bounds ({x}): no stake could judge this committee's charges"))?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    // The first epoch starts at the present, rounded down to a whole epoch: epochs are fixed by arithmetic.
    let from = if a.has("from") { int(a, "from")? } else { (now(a, &mut node)? as i64 / p.epoch) * p.epoch };
    let (b, _) = create_committee(p, from, &fu, &fk, rate(a)?).map_err(e)?;
    let index = b.created.first().map(|c| c.0 as u32).ok_or("no committee output")?;
    let gen = Genesis { auth: b.tx.inputs[0].previous_outpoint, index, value: b.tx.outputs[index as usize].value, p, from };
    let out = PathBuf::from(a.get("out").unwrap_or("kpf-out/committee.json"));
    write_json(&out, &gen.to_json())?;
    println!("committee id   {}", to_hex(&gen.id().as_bytes()));
    println!("written     {} (the genesis facts: give this file to `c-market` and to anyone who must check the committee)", out.display());
    println!("{}", committee_words(&p));
    println!("first epoch from DAA {from}; nobody can sign a price until members hold {} KAS and someone runs `committee-roll`", fmt_kas(p.min_total as i128));
    finish(a, "committee-create: a new, empty committee", &b, &keys, net, &mut node)
}

/// `committee-check`: the wallet's check of a committee file (and, with `--committee-utxo`, of the committee's live object).
pub fn committee_check_cmd(a: &Args) -> R<()> {
    a.only(&["committee", "object", "node", "json"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let gen = Genesis::from_json(&read_json(a.need("committee")?)?)?;
    gen.p.check().map_err(|x| format!("REFUSED: the committee's parameters fail the contract's bounds: {x}"))?;
    println!("committee id   {}", to_hex(&gen.id().as_bytes()));
    println!("OK: the id is that of one empty committee with these parameters, authorized by {}:{} (so every epoch table of the id descends from an empty committee)", gen.auth.transaction_id, gen.auth.index);
    println!("{}", committee_words(&gen.p));
    if a.has("object") {
        let r = committee_of(a, "object", &mut node, net)?;
        if r.id != Some(gen.id()) {
            return Err("REFUSED: the committee object does not carry this committee id".into());
        }
        if r.s.p != gen.p {
            return Err("REFUSED: the committee object's parameters differ from the genesis'".into());
        }
        show_committee(&r, None);
    }
    Ok(())
}

fn show_committee(r: &OLive<Committee>, t: Option<i64>) {
    let total = r.s.total();
    println!("epoch from DAA {} to {}; total stake {} KAS (a price needs {} KAS of it); committee value {} KAS", r.s.from, r.s.from + r.s.p.epoch, fmt_kas(total as i128), fmt_kas(r.s.p.quorum_of(total) as i128), fmt_kas(r.value as i128));
    for (i, s) in r.s.members.iter().enumerate() {
        let bid = match r.s.least_bid(i) {
            Some(b) => format!("least bid {} KAS", fmt_kas(b as i128)),
            None => "cannot be outbid now (a member is empty, or another is lighter)".into(),
        };
        if s.taken() {
            println!("  member {i:>2}  {} KAS ({:.2} %)  key {}  {bid}", fmt_kas(s.weight as i128), 100.0 * s.weight as f64 / total.max(1) as f64, to_hex(&s.key));
        } else {
            println!("  member {i:>2}  empty  {bid}");
        }
    }
    if let Some(t) = t {
        match r.s.roll_steps(t) {
            Some(m) => println!("the epoch has ended: `committee-roll` moves it by {m} to DAA {} (no epoch table covers the present until then)", r.s.from + m * r.s.p.epoch),
            None => println!("the epoch ends in {} DAA ({})", r.s.from + r.s.p.epoch - t, secs(r.s.from + r.s.p.epoch - t)),
        }
    }
}

/// `committee-show`: the member table, each member's share and what it takes to outbid it.
pub fn committee_show_cmd(a: &Args) -> R<()> {
    a.only(&["committee", "node", "daa"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let r = committee_of(a, "committee", &mut node, net)?;
    let t = if a.has("daa") || node.is_some() { Some(now(a, &mut node)? as i64) } else { None };
    println!("committee {}", r.id.map(|h| h.to_string()).unwrap_or_default());
    println!("{}", committee_words(&r.s.p));
    show_committee(&r, t);
    Ok(())
}

/// Review C2 (C2-F5): who puts KAS into a committee checks first that it began empty (`--genesis COMMITTEE.json`, the facts
/// its id is recomputed from) and that its value covers its table. `--unchecked` skips the genesis facts, never the
/// backing.
fn checked_committee(a: &Args, r: &OLive<Committee>) -> R<()> {
    if !r.s.backed(r.value as i64) {
        return Err(format!("REFUSED: the committee holds {} KAS and its members claim {} KAS: it did not begin empty, and stake put into it would pay a forged member's exit", fmt_kas(r.value as i128), fmt_kas(r.s.total() as i128)));
    }
    match a.get("genesis") {
        Some(g) => {
            let gen = Genesis::from_json(&read_json(g)?)?;
            if r.id != Some(gen.id()) || r.s.p != gen.p {
                return Err("REFUSED: the committee object is not the committee of these genesis facts (another id, or other parameters)".into());
            }
            for w in charge_warnings(&gen.p) {
                println!("WARNING: {w}");
            }
            Ok(())
        }
        None if a.has("unchecked") => {
            println!("WARNING: --unchecked: nothing shows that this committee began empty");
            Ok(())
        }
        None => Err("give --genesis COMMITTEE.json (the genesis facts `committee-create` wrote: they show the committee began empty), or --unchecked".into()),
    }
}

/// `committee-join`: take member `--member` for `--owner` (default the funder) with `--amount` KAS of stake.
pub fn committee_join_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "member", "amount", "owner", "genesis", "unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let r = committee_of(a, "committee", &mut node, net)?;
    checked_committee(a, &r)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let i = int(a, "member")? as usize;
    let key = match a.get("owner") {
        Some(o) => keys::party(o, net)?,
        None => fk,
    };
    let amount = kas(a.need("amount")?)?;
    if i < MEMBERS {
        match r.s.least_bid(i) {
            Some(b) if amount < b => return Err(format!("REFUSED: member {i} needs at least {} KAS", fmt_kas(b as i128))),
            None => return Err(format!("REFUSED: member {i} cannot be taken now: a member is outbid only when no member is empty and none is lighter (see `committee-show`)")),
            _ => {}
        }
    }
    let (b, _, exit) = join(&r, i, key, amount, &fu, &fk, rate(a)?).map_err(e)?;
    if let Some(x) = exit {
        println!("member {i} is outbid: its holder's {} KAS leave as an exit released at DAA {}", fmt_kas(x.amount as i128), x.release);
    }
    println!("the member signs from the next `committee-roll` (the epoch tables of the current epoch keep the old table); its key is {}", to_hex(&key));
    finish(a, "committee-join", &b, &keys, net, &mut node)
}

/// `committee-topup`: add `--amount` KAS to a taken member.
pub fn committee_topup_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "member", "amount", "genesis", "unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let r = committee_of(a, "committee", &mut node, net)?;
    checked_committee(a, &r)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, _) = top_up(&r, int(a, "member")? as usize, kas(a.need("amount")?)?, &fu, &fk, rate(a)?).map_err(e)?;
    finish(a, "committee-topup", &b, &keys, net, &mut node)
}

/// `committee-leave`: the member's key (`--key`) leaves; its stake becomes an exit.
pub fn committee_leave_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "member"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let r = committee_of(a, "committee", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, _, x) = leave(&r, int(a, "member")? as usize, &fu, &fk, rate(a)?).map_err(e)?;
    println!("the member can still sign until its epoch ends at DAA {}; its {} KAS are released at DAA {} (`exit-withdraw`)", r.s.from + r.s.p.epoch, fmt_kas(x.amount as i128), x.release);
    finish(a, "committee-leave", &b, &keys, net, &mut node)
}

/// `committee-roll`: start the epoch that holds the present (or `--steps M` epochs on) and write its epoch tables.
pub fn committee_roll_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "steps", "wait"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let r = committee_of(a, "committee", &mut node, net)?;
    // `--wait`: an unattended roller. The epoch's end is arithmetic, so the node is asked once now and once after
    // each sleep (two or three times an epoch); `while :; do kpf committee-roll --committee "$(ls -t out/*-committee.json | head -1)" --wait --submit ... || sleep 30; done`
    // keeps a committee in epoch tables.
    if a.has("wait") && !a.has("steps") {
        if node.is_none() {
            return Err("--wait needs --node".into());
        }
        // Live run 6: the DAA score does not advance by exactly ten a second (the first sleep ended 16 DAA early and
        // the roll was refused), so the score is read again after every sleep, never sooner than five seconds later.
        let end = r.s.from + r.s.p.epoch;
        loop {
            let t = now(a, &mut node)? as i64;
            if t >= end {
                break;
            }
            let secs = ((end - t) as u64 / 10 + 1).max(5);
            println!("waiting {secs} s for the epoch to end at DAA {end}");
            std::thread::sleep(std::time::Duration::from_secs(secs));
        }
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let m = if a.has("steps") {
        let m = int(a, "steps")?;
        if a.has("daa") || node.is_some() {
            let t = now(a, &mut node)? as i64;
            let nf = r.s.from + m * r.s.p.epoch;
            if nf > t {
                return Err(format!("REFUSED: --steps {m} is the epoch from DAA {nf}, which has not begun (now {t})"));
            }
            if nf + r.s.p.epoch <= t {
                println!("WARNING: --steps {m} is the epoch [{nf}, {}), which is over (now {t}): its epoch tables serve no signed price of the present", nf + r.s.p.epoch);
            }
        }
        m
    } else {
        let t = now(a, &mut node)? as i64;
        r.s.roll_steps(t).ok_or(format!("the epoch has not ended: it runs to DAA {} (now {t}); nothing to roll", r.s.from + r.s.p.epoch))?
    };
    let (b, r2, tk) = roll(&r, m, &fu, &fk, rate(a)?).map_err(e)?;
    println!("epoch [{}, {}) with {} epoch tables; total stake {} KAS, a price needs {} KAS of it", tk.from, tk.until, r2.p.epoch_tables, fmt_kas(tk.total as i128), fmt_kas(tk.quorum() as i128));
    finish(a, "committee-roll", &b, &keys, net, &mut node)
}

/// `epoch-table-retire`: remove an epoch table a whole epoch after its end; its value goes to the funder.
pub fn epoch_table_retire_cmd(a: &Args) -> R<()> {
    only(a, &["epoch-table"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let t = epoch_table_of(a, "epoch-table", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "epoch-table-retire", &retire(&t, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `exit-withdraw`: pay a released exit to its key (anyone may).
pub fn exit_withdraw_cmd(a: &Args) -> R<()> {
    only(a, &["exit"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = exit_of(a, "exit", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    println!("{} KAS to {} from DAA {}", fmt_kas(x.s.amount as i128), p2pk_address(&x.s.key, net), x.s.release);
    finish(a, "exit-withdraw", &withdraw(&x, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

// ---------------------------------------------------------------- signed prices

fn price_json(t: &SignedPrice, committee: &[u8; 32], market: &[u8; 32]) -> Value {
    json!({ "format": "kpf-signed-price/1", "committeeId": to_hex(committee), "market": to_hex(market), "t0": t.t0, "price": t.price, "key": to_hex(&t.key), "sig": to_hex(&t.sig) })
}

fn price_from(path: &str, committee: &[u8; 32], market: &[u8; 32]) -> R<SignedPrice> {
    let v = read_json(path)?;
    let h = |k: &str| v.get(k).and_then(Value::as_str).ok_or(format!("{path}: no `{k}`"));
    if hex32(h("committeeId")?)? != *committee {
        return Err(format!("{path}: a signed price of another committee"));
    }
    if hex32(h("market")?)? != *market {
        return Err(format!("{path}: a signed price of another market"));
    }
    let n = |k: &str| v.get(k).and_then(Value::as_i64).ok_or(format!("{path}: no `{k}`"));
    let sig: [u8; 64] = from_hex(h("sig")?).map_err(e)?.try_into().map_err(|_| format!("{path}: the signature is not 64 bytes"))?;
    let t = SignedPrice { key: hex32(h("key")?)?, t0: n("t0")?, price: n("price")?, sig };
    if !price_verifies(&t, committee, market) {
        return Err(format!("{path}: the signature does not verify"));
    }
    Ok(t)
}

/// A signed price file as it is, with the committee and market it names (for a proof against a member, which needs no market file).
pub(crate) fn price_raw(path: &str) -> R<(SignedPrice, [u8; 32], [u8; 32])> {
    let v = read_json(path)?;
    let h = |k: &str| v.get(k).and_then(Value::as_str).ok_or(format!("{path}: no `{k}`"));
    let (committee, market) = (hex32(h("committeeId")?)?, hex32(h("market")?)?);
    Ok((price_from(path, &committee, &market)?, committee, market))
}

fn proof(a: &Args) -> R<(SignedPrice, SignedPrice, [u8; 32], [u8; 32])> {
    let f = a.all("signed-price");
    if f.len() != 2 {
        return Err("give the two signed signed prices: --signed-price A.json --signed-price B.json (one member, one time, two prices)".into());
    }
    let (ta, ra, ma) = price_raw(f[0])?;
    let (tb, rb, mb) = price_raw(f[1])?;
    if ra != rb || ma != mb || !equivocation(&ta, &tb, &ra, &ma) {
        return Err("REFUSED: not a proof: the two signed prices must be of one committee, one market, one key and one DAA, with two different prices".into());
    }
    Ok((ta, tb, ra, ma))
}

/// `member-forfeit`: a member signed two prices for one time: anyone slashes it with the two signed signed prices. A tenth of the
/// stake is the sender's, the rest is burned.
pub fn member_forfeit_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "member", "signed-price"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let r = committee_of(a, "committee", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (ta, tb, rid, market) = proof(a)?;
    if r.id.map(|h| h.as_bytes()) != Some(rid) {
        return Err("REFUSED: the signed prices are of another committee".into());
    }
    let i = match a.get("member") {
        Some(_) => int(a, "member")? as usize,
        None => r.s.members.iter().position(|s| s.taken() && s.key == ta.key).ok_or("no member of the committee holds the key that signed (if it left, slash its exit: `exit-forfeit`)")?,
    };
    let (b, _) = forfeit_member(&r, i, &market, &ta, &tb, &fu, &fk, rate(a)?).map_err(e)?;
    if r.s.holds[i] == NO_HOLD {
        println!("member {i} signed {} and {} KAS for the time {}: its {} KAS are forfeited", fmt_kas(ta.price as i128), fmt_kas(tb.price as i128), ta.t0, fmt_kas(r.s.members[i].weight as i128));
    } else {
        let (_, left, _) = r.s.forfeit(i).map_err(|x| x.to_string())?;
        println!("member {i} signed {} and {} KAS for the time {}. It is HELD for a charge: {} KAS of its {} KAS are burned, {} KAS stay under its holds for its charges, and the sender of this proof is paid nothing", fmt_kas(ta.price as i128), fmt_kas(tb.price as i128), ta.t0, fmt_kas(left as i128), fmt_kas(r.s.members[i].weight as i128), fmt_kas((r.s.members[i].weight - left) as i128));
    }
    finish(a, "member-forfeit: a member that signed two prices for one time", &b, &keys, net, &mut node)
}

/// `exit-forfeit`: the same proof against a stake on its way out.
pub fn exit_forfeit_cmd(a: &Args) -> R<()> {
    only(a, &["exit", "signed-price"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = exit_of(a, "exit", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (ta, tb, rid, market) = proof(a)?;
    if x.id.map(|h| h.as_bytes()) != Some(rid) {
        return Err("REFUSED: the signed prices are of another committee".into());
    }
    let b = forfeit_exit(&x, &market, &ta, &tb, &fu, &fk, rate(a)?).map_err(e)?;
    match x.s.forfeit_rest().map_err(|y| y.to_string())? {
        None => println!("the exit's key signed {} and {} KAS for the time {}: its {} KAS are forfeited", fmt_kas(ta.price as i128), fmt_kas(tb.price as i128), ta.t0, fmt_kas(x.s.amount as i128)),
        Some(k) => println!("the exit's key signed {} and {} KAS for the time {}. It is HELD for a charge: {} KAS of its {} KAS are burned, {} KAS stay under its holds for its charges, and the sender of this proof is paid nothing", fmt_kas(ta.price as i128), fmt_kas(tb.price as i128), ta.t0, fmt_kas((x.s.amount - k.amount) as i128), fmt_kas(x.s.amount as i128), fmt_kas(k.amount as i128)),
    }
    finish(a, "exit-forfeit: a stake whose key signed two prices for one time", &b, &keys, net, &mut node)
}

/// What the judge rules on (review C5, C5-F4: one statement everywhere).
const PRICE_RULE: &str = "The judge's question: is the signed price TRUE? YES if the price it names lies within the market's g of the feed at its DAA score, by the market's feed definition (a closed session has no true price); NO otherwise. g is the market's tolerance, at least four times the committee's spread: honest members that differ by their sources are not false.";

/// What is wrong with a committee's charge parameters, for whoever joins it or trades on it (review C5, C5-F5).
fn charge_warnings(p: &CommitteeParams) -> Vec<String> {
    let mut w = vec![];
    let c = &p.charge;
    if let Err(x) = c.judge.p.check() {
        w.push(format!("the judge block fails the judge's bounds ({x}): no stake can judge this committee's charges, so no member is ever slashed for a false price"));
    }
    if c.daa < 600 {
        w.push(format!("chargeDaa is {} ({}): a contest must land within that of a charge, so none will, and no member is ever slashed for a false price", c.daa, secs(c.daa)));
    }
    if c.bond < KAS {
        w.push("chargeBond is below 1 KAS: holding a member's stake costs next to nothing".into());
    }
    w
}

/// The member of the committee that holds `key`, or `--member`.
fn member_of(a: &Args, r: &Committee, key: &[u8; 32]) -> R<usize> {
    match a.get("member") {
        Some(_) => Ok(int(a, "member")? as usize),
        None => r.members.iter().position(|s| s.taken() && s.key == *key).ok_or("no member of the committee holds that key (if it left, give its exit: --exit)".into()),
    }
}

/// `charge`: a member (or a stake on its way out) signed a false price for a signed price: anyone charges it with the signed
/// signed price and the bond. The judge's stake decides; the stake is held until then.
pub fn charge_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "exit", "member", "signed-price", "pay-to"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (t, _, market) = price_raw(a.need("signed-price")?)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let pay = match a.get("pay-to") {
        Some(p) => keys::party(p, net)?,
        None => fk,
    };
    let (b, c, cid, held_before) = if a.has("exit") {
        let x = exit_of(a, "exit", &mut node, net)?;
        let (b, c, cid) = charge(&Staked::Exit(&x), &market, &t, pay, &fu, &fk, rate(a)?).map_err(e)?;
        (b, c, cid, x.s.hold != NO_HOLD)
    } else {
        let r = committee_of(a, "committee", &mut node, net)?;
        let i = member_of(a, &r.s, &t.key)?;
        let (b, c, cid) = charge(&Staked::Member(&r, i), &market, &t, pay, &fu, &fk, rate(a)?).map_err(e)?;
        (b, c, cid, r.s.holds[i] != NO_HOLD)
    };
    println!("charged: {} signed {} KAS for the time {} of market {}", to_hex(&c.stake_key), fmt_kas(c.price as i128), c.t0, &to_hex(&c.market)[..16]);
    if held_before {
        println!("note: this stake already had a hold: it is settled with the ids of its unsettled charges in the order they were made (`hold-witness`): keep them");
    }
    println!("charge id   {}", to_hex(&cid.as_bytes()));
    println!("the bond {} KAS: back with a tenth of the stake if the judge refutes the signed price; the charged key's if it upholds it; burned if nobody contests the charge within {} DAA ({})", fmt_kas(c.p.bond as i128), c.p.daa, secs(c.p.daa));
    println!("the stake goes on signing and is not withdrawn until the charge is settled. Contest it with judge stake accepted {} DAA before it lands (`charge-contest`): a verdict takes no more of the stake than the NO side had at risk, and pays you a tenth of that.", c.p.judge.p.age);
    println!("{PRICE_RULE}");
    finish(a, "charge: a stake's key signed a false price", &b, &keys, net, &mut node)
}

/// `judge-stake`: a free stake of the committee's judge. It counts on charges accepted `ageDaa` after it.
pub fn judge_stake_cmd(a: &Args) -> R<()> {
    only(a, &["committee", "value", "owner"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let gen = Genesis::from_json(&read_json(a.need("committee")?)?)?;
    let jm = gen.p.charge.judge;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let owner = match a.get("owner") {
        Some(p) => keys::party(p, net)?,
        None => fk,
    };
    let b = judge_stake(&jm, owner, kas(a.need("value")?)? as u64, &fu, &fk, rate(a)?).map_err(e)?;
    println!("a stake of the judge {} (at least {} KAS a stake): it judges charges accepted {} DAA ({}) after it; a round is {} DAA; leaving takes {} DAA", &to_hex(&jm.id)[..16], fmt_kas(jm.p.min_stake as i128), jm.p.age, secs(jm.p.age), jm.p.round, jm.p.unbond);
    finish(a, "judge-stake", &b, &keys, net, &mut node)
}

/// `charge-contest`: aged judge stake opens the dispute on a pending charge, on NO ("the signed price is false").
pub fn charge_contest_cmd(a: &Args) -> R<()> {
    only(a, &["charge", "stake", "base"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let ch = charge_of(a, "charge", &mut node, net)?;
    let st = crate::ocmds::stake_at(a.need("stake")?, &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    if a.has("daa") || node.is_some() {
        let t = now(a, &mut node)? as i64;
        if t >= ch.daa as i64 + ch.s.p.daa {
            println!("WARNING: the charge has waited {} DAA, its chargeDaa is {}: a contest landing now lapses whatever the judge says (the bond is burned, the stake is not slashed)", t - ch.daa as i64, ch.s.p.daa);
        }
    }
    let base = if a.has("base") { kas(a.need("base")?)? as u64 } else { 50_000_000 };
    let (b, _, _) = charge_contest(&ch, &st, base, &fu, &fk, rate(a)?).map_err(e)?;
    println!("contested: {} KAS on NO (\"{} did not sign a true price: {} KAS is not within the committee's spread of the feed at DAA {}\")", fmt_kas(st.value as i128), &to_hex(&ch.s.stake_key)[..16], fmt_kas(ch.s.price as i128), ch.s.t0);
    println!("{PRICE_RULE}");
    println!("Contest only a false signed price: a true signed price answered costs the whole contest stake. Other stakes answer with `charge-answer`; `charge-decide` delivers the verdict after the round. The verdict takes at most the NO total ({} KAS now) of the stake.", fmt_kas(st.value as i128));
    finish(a, "charge-contest", &b, &keys, net, &mut node)
}

/// `charge-answer`: judge stake joins the trailing side of a charge's dispute. The charge is shown to be one of the
/// committee's (among the holds of the stake given), which is what a trace is for a claim of the oracle family.
pub fn charge_answer_cmd(a: &Args) -> R<()> {
    only(a, &["dispute", "charge", "side", "stake", "committee", "exit", "member", "before", "later"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let ch = charge_of(a, "charge", &mut node, net)?;
    let d = match crate::ocmds::load(a, "dispute", &mut node, net)? {
        (f, kpf_protocol::oracle_build::OObj::Stake(s)) => live_of(&f, s)?,
        (_, o) => return Err(format!("--dispute is a {}, not a judge's dispute", o.kind())),
    };
    let cid = ch.id.ok_or("the charge has no covenant id")?;
    if d.id != Some(cid) || ch.s.status != CHARGE_CONTESTED {
        return Err("REFUSED: the dispute is not of this contested charge".into());
    }
    let before = if a.has("before") { hex32(a.need("before")?)? } else { NO_HOLD };
    let later = later_of(a)?;
    let hold = if a.has("exit") {
        exit_of(a, "exit", &mut node, net)?.s.hold
    } else if a.has("committee") {
        let r = committee_of(a, "committee", &mut node, net)?;
        r.s.holds[member_of(a, &r.s, &ch.s.stake_key)?]
    } else {
        return Err("give the stake the charge is held by (--committee R.json [--member I], or --exit E.json): it shows the charge to be the committee's own, and no stake should judge another".into());
    };
    hold_without(&hold, &cid.as_bytes(), &before, &later).map_err(|x| format!("REFUSED: {x}: this is not a charge the committee wrote against that stake (or give --before and --later: `hold-witness`)"))?;
    let sd = match a.need("side")? {
        "yes" => kpf_protocol::judge::YES,
        "no" => kpf_protocol::judge::NO,
        s => return Err(format!("--side yes|no, not `{s}`")),
    };
    let stakes = a.all("stake").into_iter().map(|p| crate::ocmds::stake_at(p, &mut node, net)).collect::<R<Vec<_>>>()?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, n) = kpf_protocol::oracle_build::deposit(&d, sd, &stakes, &fu, &fk, rate(a)?, &kpf_protocol::oracle_build::Trust::Untraced).map_err(e)?;
    println!("the charge: {} signed {} KAS for the time {} of market {} (a charge of the committee's own: it is among that stake's holds)", to_hex(&ch.s.stake_key), fmt_kas(ch.s.price as i128), ch.s.t0, &to_hex(&ch.s.market)[..16]);
    println!("after the answer: {} leads, YES {} KAS, NO {} KAS", if n.side == kpf_protocol::judge::YES { "YES" } else { "NO" }, fmt_kas(n.yes as i128), fmt_kas(n.no as i128));
    if n.side != sd {
        let (mine, theirs) = if sd == kpf_protocol::judge::YES { (n.yes, n.no) } else { (n.no, n.yes) };
        let bar = ch.s.p.judge.p.need(theirs).map_err(|x| x.to_string())?;
        println!("WARNING: this answer does NOT take the lead: your side has {} KAS and needs {} KAS ({} % of the other side). A stake on the side that loses is lost: add {} KAS of aged stake in this answer, or do not send it", fmt_kas(mine as i128), fmt_kas(bar as i128), ch.s.p.judge.p.mu_bps as f64 / 100.0, fmt_kas((bar - mine) as i128));
    }
    println!("{PRICE_RULE}");
    finish(a, "charge-answer", &b, &keys, net, &mut node)
}

/// `charge-decide`: once the round has closed, the dispute resolves and its verdict reaches the charge.
pub fn charge_decide_cmd(a: &Args) -> R<()> {
    only(a, &["charge", "dispute"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let ch = charge_of(a, "charge", &mut node, net)?;
    let d = match crate::ocmds::load(a, "dispute", &mut node, net)? {
        (f, kpf_protocol::oracle_build::OObj::Stake(s)) => live_of(&f, s)?,
        (_, o) => return Err(format!("--dispute is a {}, not a judge's dispute", o.kind())),
    };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, c, _) = charge_decide(&d, &ch, &fu, &fk, rate(a)?).map_err(e)?;
    println!(
        "{}",
        match c.status {
            CHARGE_REFUTED => "the verdict: NO leads, the signed price is refuted: `charge-settle` slashes the stake",
            CHARGE_UPHELD => "the verdict: YES leads, the signed price is upheld: `charge-settle` ends the hold and pays the bond to the charged key",
            _ => "the contest came after chargeDaa: the charge lapsed whatever the judge says: `charge-settle` ends the hold and burns the bond",
        }
    );
    finish(a, "charge-decide", &b, &keys, net, &mut node)
}

fn later_of(a: &Args) -> R<Later> {
    let mut l = NO_LATER;
    if let Some(s) = a.get("later") {
        let ids: Vec<&str> = s.split(',').filter(|x| !x.is_empty()).collect();
        if ids.len() > HOLD_DEPTH {
            return Err(format!("at most {HOLD_DEPTH} charges after it: settle the later ones first"));
        }
        for (i, x) in ids.iter().enumerate() {
            l[i] = hex32(x)?;
        }
    }
    Ok(l)
}

/// `charge-settle`: a charge leaves its stake: slashed if the judge refuted the signed price, the hold ended otherwise.
pub fn charge_settle_cmd(a: &Args) -> R<()> {
    only(a, &["charge", "committee", "exit", "member", "before", "later"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let ch = charge_of(a, "charge", &mut node, net)?;
    let before = if a.has("before") { hex32(a.need("before")?)? } else { NO_HOLD };
    let later = later_of(a)?;
    let lock = if a.has("daa") || node.is_some() {
        now(a, &mut node)?
    } else if ch.s.status == CHARGE_PENDING {
        return Err("a pending charge lapses by the clock: give --daa or --node".into());
    } else {
        0
    };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    const WITNESS: &str = " (a stake held for several charges is settled with --before and --later: `hold-witness`)";
    let (b, out) = if a.has("exit") {
        let x = exit_of(a, "exit", &mut node, net)?;
        charge_settle(&Staked::Exit(&x), &ch, &before, &later, lock, &fu, &fk, rate(a)?).map_err(|x| format!("{x}{WITNESS}"))?
    } else {
        let r = committee_of(a, "committee", &mut node, net)?;
        let i = member_of(a, &r.s, &ch.s.stake_key)?;
        charge_settle(&Staked::Member(&r, i), &ch, &before, &later, lock, &fu, &fk, rate(a)?).map_err(|x| format!("{x}{WITNESS}"))?
    };
    match out {
        Settled::Slashed { stake, taken, reward, burned } if taken == stake => println!("slashed: the stake of {} KAS is gone: {} KAS and the bond to the charge's author, {} KAS burned", fmt_kas(stake as i128), fmt_kas(reward as i128), fmt_kas(burned as i128)),
        Settled::Slashed { stake, taken, reward, burned } => println!("slashed: {} KAS of the stake of {} KAS (a verdict takes no more than its NO side had at risk: {} KAS): {} KAS and the bond to the charge's author, {} KAS burned; the stake keeps {} KAS (and its other holds, if it has any)", fmt_kas(taken as i128), fmt_kas(stake as i128), fmt_kas(ch.s.clock as i128), fmt_kas(reward as i128), fmt_kas(burned as i128), fmt_kas((stake - taken) as i128)),
        Settled::Cleared { bond_to_key: true } => println!("cleared: the signed price was upheld: the hold ends and the bond {} KAS goes to the charged key", fmt_kas(ch.value as i128)),
        Settled::Cleared { bond_to_key: false } => println!("cleared: the charge lapsed: the hold ends and the bond {} KAS is burned", fmt_kas(ch.value as i128)),
    }
    finish(a, "charge-settle", &b, &keys, net, &mut node)
}

/// `hold-witness`: what shows one charge to be among a stake's holds, from the ids of its unsettled charges in the
/// order they were made.
pub fn hold_witness_cmd(a: &Args) -> R<()> {
    a.only(&["ids", "of"])?;
    let ids: Vec<[u8; 32]> = a.need("ids")?.split(',').filter(|x| !x.is_empty()).map(hex32).collect::<R<_>>()?;
    let of = hex32(a.need("of")?)?;
    let (b, l) = hold_witness(&ids, &of).ok_or(format!("the charge is not among --ids, or more than {HOLD_DEPTH} charges were made after it (settle those first)"))?;
    let later: Vec<String> = l.iter().filter(|x| **x != NO_HOLD).map(|x| to_hex(x)).collect();
    println!("holds       {}", to_hex(&ids.iter().fold(NO_HOLD, |h, x| hold_add(&h, x))));
    println!("--before {}{}", to_hex(&b), if later.is_empty() { String::new() } else { format!(" --later {}", later.join(",")) });
    Ok(())
}

/// Signs `p` for the time `t0` with the key of `key_path` and writes the signed price file (`out`, or
/// `kpf-out/price-<t0>-<key>.json`). What the key signed is kept in a journal beside the key file, and a second price
/// for a signed price it holds is refused (`second` overrides).
pub(crate) fn sign_and_write(key_path: &str, kp: &secp256k1::Keypair, m: &CMarket, t0: i64, p: i64, out: Option<PathBuf>, second: bool) -> R<(SignedPrice, PathBuf)> {
    // Review C4 (C4-N2): what this key signed is kept in a journal beside the key file (no secret in it), and a second
    // price for a signed price it holds is refused wherever the first was written: with the forfeit, one sompi of difference
    // costs the member its stake.
    let journal = PathBuf::from(format!("{}.signed.jsonl", key_path));
    let (jr, jm) = (to_hex(&m.committee_id), to_hex(&m.id()));
    let before = std::fs::read_to_string(&journal).unwrap_or_default();
    let mut known = false;
    for v in before.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if v["committee"].as_str() == Some(jr.as_str()) && v["market"].as_str() == Some(jm.as_str()) && v["t0"].as_i64() == Some(t0) {
            match v["price"].as_i64() {
                Some(q) if q == p => known = true,
                Some(q) if !second => {
                    return Err(format!("REFUSED: this key already signed {} KAS for the time {t0} of this market ({}); a second price for one time is proof against the member and costs it its stake", fmt_kas(q as i128), journal.display()));
                }
                _ => {}
            }
        }
    }
    let t = sign_price(kp, &m.committee_id, &m.id(), t0, p);
    if !known {
        use std::io::Write as _;
        // The journal is written before the signature leaves the tool: a signature nobody recorded is the risk.
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&journal).map_err(|x| format!("{}: {x} (the signer's journal must be writable: nothing is signed without it)", journal.display()))?;
        writeln!(f, "{}", json!({ "committee": jr, "market": jm, "t0": t0, "price": p })).map_err(e)?;
    }
    let out = PathBuf::from(out.map(|o| o.display().to_string()).unwrap_or(format!("kpf-out/price-{t0}-{}.json", &to_hex(&t.key)[..8])));
    // A member signs one price a signed price: a second price for the same time is proof against it, so the tool does not write
    // one over the first (review C2, C2-N2).
    if let Ok(old) = price_from(&out.display().to_string(), &m.committee_id, &m.id()) {
        if old.key == t.key && old.t0 == t0 && old.price != p && !second {
            return Err(format!("REFUSED: {} already holds this member's price {} KAS for the time {t0}; a second price for one time is proof against the member", out.display(), fmt_kas(old.price as i128)));
        }
    }
    write_json(&out, &price_json(&t, &m.committee_id, &m.id()))?;
    Ok((t, out))
}

/// The feed's price now, for an unattended member: a file (`--price-file`, rewritten by whatever follows the sources), a
/// command whose first word of output is the price (`--price-cmd`), or a constant (`--price`, for trials).
pub(crate) fn feed_price(a: &Args) -> R<i64> {
    feed_prices(a).map(|(p, _, _)| p)
}

/// The feed's price now and, where the source says them, the least and the most since it was last asked: a file holds
/// one number or three (`price low high`); a command's output begins with one or with three. A source that says one
/// is read as `(p, p, p)`.
pub(crate) fn feed_prices(a: &Args) -> R<(i64, i64, i64)> {
    let limit = |p: i64| if (0..=PRICE_LIMIT).contains(&p) { Ok(p) } else { Err(format!("the price is outside [0, {}] KAS, the most an epoch table attests", fmt_kas(PRICE_LIMIT as i128))) };
    let three = |w: &[&str], what: &str| -> R<(i64, i64, i64)> {
        let n = |x: &str| kas(x).map_err(|e| format!("{what}: {e}")).and_then(limit);
        let (p, lo, hi) = (n(w[0])?, n(w[1])?, n(w[2])?);
        if lo <= p && p <= hi {
            Ok((p, lo, hi))
        } else {
            Err(format!("{what}: `price low high` with the price outside its low and high"))
        }
    };
    if let Some(f) = a.get("price-file") {
        let text = std::fs::read_to_string(f).map_err(|x| format!("{f}: {x}"))?;
        let w: Vec<&str> = text.split_whitespace().collect();
        return match w.len() {
            3 => three(&w, f),
            _ => kas(text.trim()).map_err(|x| format!("{f}: {x}")).and_then(limit).map(|p| (p, p, p)),
        };
    }
    if let Some(c) = a.get("price-cmd") {
        let (sh, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
        let o = std::process::Command::new(sh).args([flag, c]).output().map_err(|x| format!("--price-cmd: {x}"))?;
        if !o.status.success() {
            return Err(format!("--price-cmd failed ({})", o.status));
        }
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let w: Vec<&str> = text.split_whitespace().collect();
        // (Three numbers at the start are a price with its low and high; anything else after the first word is the
        // command's own talk, as before.)
        if w.len() >= 3 && kas(w[1]).is_ok() && kas(w[2]).is_ok() {
            return three(&w, "--price-cmd");
        }
        let p = kas(w.first().ok_or("--price-cmd printed nothing")?).map_err(|x| format!("--price-cmd: {x}")).and_then(limit)?;
        return Ok((p, p, p));
    }
    let p = limit(price(a, "price")?)?;
    Ok((p, p, p))
}

/// `member-run`: a member signs unattended. Every `--grid` DAA (default 100: ten seconds) it signs the feed's price for
/// the signed price at the last whole multiple of the grid, so members that never talk sign the same signed prices, and writes the signed price
/// file into `--dir` (a directory claimants read, or that is synced to them). The node is asked for the DAA score
/// once every `--sync-secs` (default 60) and the clock counts between. When the price source fails nothing is
/// signed: a market without signed prices is closed, not wrong.
pub fn member_run_cmd(a: &Args) -> R<()> {
    a.only(&["market", "key", "dir", "price", "price-file", "price-cmd", "grid", "count", "once", "sync-secs", "keep", "lag", "node", "daa"])?;
    let mut node = connect(a)?;
    let m = cmarket_from(a.need("market")?)?.0;
    let key_path = a.need("key")?;
    let k = keys::load(std::path::Path::new(key_path))?;
    let kp = secp256k1::Keypair::from_seckey_slice(&secp256k1::Secp256k1::new(), &k.secret).map_err(e)?;
    let dir = PathBuf::from(a.get("dir").unwrap_or("kpf-out/signed-prices"));
    let (grid, lag, keep) = (int_or(a, "grid", 100)?, int_or(a, "lag", 20)?, int_or(a, "keep", 200)?);
    if grid < 10 || lag < 0 || keep < 1 {
        return Err("--grid is at least 10 DAA, --lag at least 0, --keep at least 1".into());
    }
    if !(a.has("price") || a.has("price-file") || a.has("price-cmd")) {
        return Err("give the feed: --price-file FILE, --price-cmd COMMAND or --price P".into());
    }
    let count = if a.has("once") { 1 } else { int_or(a, "count", 0)? };
    let sync = int_or(a, "sync-secs", 60)?.max(10) as u64;
    let live = node.is_some();
    if !live && !a.has("daa") {
        return Err("give --node URL (the DAA score is read once a --sync-secs), or --daa N for a trial without a clock".into());
    }
    let mut base = now(a, &mut node)? as i64;
    let mut at = std::time::Instant::now();
    let key8 = to_hex(&kp.x_only_public_key().0.serialize())[..8].to_string();
    println!("member {key8}…: signing market {} every {grid} DAA into {}; the node is asked once every {sync} s", &to_hex(&m.id())[..16], dir.display());
    let (mut signed, mut failed, mut rounds, mut last) = (0i64, 0i64, 0i64, -1i64);
    loop {
        if live && at.elapsed().as_secs() >= sync {
            base = now(a, &mut node)? as i64;
            at = std::time::Instant::now();
        }
        // Between two readings of the node the score is counted at 9.5 DAA a second, below the nominal ten (review
        // C8, C8-F4; live run 6 measured 9.95): a signed price is signed `lag` DAA after its time and never before it, and a
        // signed price of the future is one a charge can call false.
        let est = if live { base + (at.elapsed().as_millis() as i64) * 95 / 10_000 } else { base };
        let t0 = ((est - lag) / grid) * grid;
        if t0 > last && t0 >= 0 {
            last = t0;
            rounds += 1;
            let out = dir.join(format!("price-{t0}-{key8}.json"));
            match feed_price(a).and_then(|p| sign_and_write(key_path, &kp, &m, t0, p, Some(out), false)) {
                Ok((t, out)) => {
                    signed += 1;
                    println!("signed: {} KAS at DAA {t0}   {}", fmt_kas(t.price as i128), out.display());
                }
                Err(x) => {
                    failed += 1;
                    println!("NOT SIGNED for DAA {t0}: {x}");
                }
            }
            // Old signed prices of this key leave the directory (a claim uses a signed price within the market's lateDaa).
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for f in rd.flatten() {
                    let name = f.file_name().to_string_lossy().into_owned();
                    let old = name.strip_prefix("price-").and_then(|r| r.strip_suffix(&format!("-{key8}.json"))).and_then(|n| n.parse::<i64>().ok()).is_some_and(|n| n < t0 - keep * grid);
                    if old {
                        let _ = std::fs::remove_file(f.path());
                    }
                }
            }
            if count > 0 && rounds >= count {
                break;
            }
        }
        if live {
            let next = t0 + grid + lag;
            std::thread::sleep(std::time::Duration::from_millis((((next - est).max(1)) as u64 * 100).min(sync * 1000)));
        } else {
            base += grid;
        }
    }
    println!("{signed} signed price(s) signed, {failed} not signed");
    if signed == 0 {
        return Err("no signed price was signed".into());
    }
    Ok(())
}

/// `price-sign`: a member signs its price for a signed price of a market. Offline with `--t0`; with `--node` the signed price is the
/// node's present DAA (one query).
pub fn price_sign_cmd(a: &Args) -> R<()> {
    a.only(&["market", "price", "t0", "key", "out", "node", "daa", "second-price"])?;
    let mut node = connect(a)?;
    let m = cmarket_from(a.need("market")?)?.0;
    let k = keys::load(std::path::Path::new(a.need("key")?))?;
    let kp = secp256k1::Keypair::from_seckey_slice(&secp256k1::Secp256k1::new(), &k.secret).map_err(e)?;
    let present = if a.has("daa") || node.is_some() { Some(now(a, &mut node)? as i64) } else { None };
    let t0 = if a.has("t0") { int(a, "t0")? } else { present.ok_or("give --t0 DAA, or --node / --daa for the present")? };
    // A member signs no signed price of the future: the epoch table refuses one, and a signed future price is a claim waiting to be made.
    if present.is_some_and(|n| t0 > n) {
        return Err(format!("REFUSED: the time {t0} lies after the present DAA {}: a member signs what the feed was, not what it will be", present.unwrap_or(0)));
    }
    let p = price(a, "price")?;
    // Review C2 (C2-F4): a price above the market's priceMax is signed as it is (a pair counts it as priceMax); only
    // the epoch table's own limit bounds it.
    if p < 0 || p > PRICE_LIMIT {
        return Err(format!("the price is outside [0, {}] KAS, the most an epoch table attests", fmt_kas(PRICE_LIMIT as i128)));
    }
    if p > m.p.price_max {
        println!("note: {} KAS is above this market's priceMax ({} KAS): a pair counts it as priceMax", fmt_kas(p as i128), fmt_kas(m.p.price_max as i128));
    }
    let out = a.get("out").map(PathBuf::from);
    let (t, out) = sign_and_write(a.need("key")?, &kp, &m, t0, p, out, a.has("second-price"))?;
    println!("signed: {} KAS at DAA {t0} for market {} by {}", fmt_kas(p as i128), &to_hex(&m.id())[..16], to_hex(&t.key));
    println!("written     {}", out.display());
    println!("A member signs one price a signed price. Two prices for one time are proof against the member: anyone slashes its stake with them (`member-forfeit`).");
    Ok(())
}

/// The attestation a command presents: the signed prices given with `--signed-price` (one file a member), for the time `--t0` (default:
/// the signed price of the first file).
fn attestation(a: &Args, m: &CMarket, epoch_table: &OLive<EpochTable>) -> R<Attestation> {
    let files = a.all("signed-price");
    if files.is_empty() {
        return Err("give the members' signed prices: --signed-price FILE (once a member; `price-sign` writes them)".into());
    }
    let signed_prices: Vec<SignedPrice> = files.iter().map(|f| price_from(f, &m.committee_id, &m.id())).collect::<R<_>>()?;
    let t0 = int_or(a, "t0", signed_prices[0].t0)?;
    let rid = epoch_table.id.map(|h| h.as_bytes()).unwrap_or([0; 32]);
    if rid != m.committee_id {
        return Err("REFUSED: the epoch table is not of this market's committee".into());
    }
    let att = Attestation::collect(&epoch_table.s, &rid, &m.id(), t0, &signed_prices).map_err(|x| {
        let signed: i64 = epoch_table.s.members.iter().filter(|s| signed_prices.iter().any(|t| t.key == s.key && t.t0 == t0)).map(|s| s.weight).sum();
        format!("no price: {x} (the epoch table serves signed prices [{}, {}); members holding {} KAS signed the time {t0}, and a price needs {} KAS within {} % of it)", epoch_table.s.from, epoch_table.s.until, fmt_kas(signed as i128), fmt_kas(epoch_table.s.quorum() as i128), epoch_table.s.spread_bps as f64 / 100.0)
    })?;
    println!("attested: {} KAS at DAA {t0}: {} members signed within {} % of it; a claim with it must land by DAA {} ({} after the signed price)", fmt_kas(att.p as i128), att.bits.count_ones(), epoch_table.s.spread_bps as f64 / 100.0, t0 + m.p.late_daa, secs(m.p.late_daa));
    Ok(att)
}

/// `price-check`: what price a set of signed signed prices attests on an epoch table, without building anything.
pub fn price_check_cmd(a: &Args) -> R<()> {
    a.only(&["market", "epoch-table", "signed-price", "t0", "node"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let m = cmarket_from(a.need("market")?)?.0;
    let t = epoch_table_of(a, "epoch-table", &mut node, net)?;
    attestation(a, &m, &t).map(|_| ())
}

// ---------------------------------------------------------------- markets

/// A market file: the block (216 bytes, hex), its id, the parameters in plain form, and the committee's genesis facts.
fn cmarket_json(m: &CMarket, gen: Option<&Genesis>) -> Value {
    let p = &m.p;
    let mut v = json!({
        "format": "kpf-committee-market/1",
        "id": to_hex(&m.id()),
        "block": to_hex(&m.bytes()),
        "committeeId": to_hex(&m.committee_id),
        "feedHash": to_hex(&m.feed_hash),
        "pair": {
            "termDaa": p.term, "scale": p.scale, "priceMax": p.price_max, "mmrBps": p.mmr_bps, "gBps": p.g_bps, "liqFeeBps": p.liq_fee_bps,
            "keeperFeeMin": p.keeper_fee_min, "minSize": p.min_size, "carrier": p.carrier, "idleDaa": p.idle_daa, "graceDaa": p.grace_daa,
            "lateDaa": p.late_daa, "quietDaa": p.quiet_daa,
        },
    });
    if let Some(g) = gen {
        v["committee"] = g.to_json();
    }
    v
}

/// The market of a market file (decoded from its block, the id checked) and its committee's genesis facts when it has them.
/// A market file as a co-signer's reference (review C15, C15-F2): the market of a file that carries its committee's
/// genesis facts, as `c-check` reads them. A file without them says nothing of how the committee began.
pub(crate) fn checked_market(path: &str) -> R<CMarket> {
    match cmarket_from(path)? {
        (m, Some(_)) => Ok(m),
        (_, None) => Err(format!("REFUSED: {path} carries no genesis facts of its committee: it is not a market file one has checked (`c-check`), and a committee that did not begin empty can hold forged epoch tables")),
    }
}

/// Whether an account is what `x-open` and `x-swap` write for the markets given (review C15, C15-F1): each leg in one
/// of them with that market's scale and price limit, and the terms the more careful of its markets' (`terms_of`). An
/// account may carry any terms its script accepts; a co-signer holding the market files accepts these alone.
pub(crate) fn account_of_markets(x: &Cross, known: &[CMarket]) -> Result<(), String> {
    let mut ms = vec![];
    for (i, l) in x.legs.iter().enumerate().filter(|(_, l)| l.size > 0) {
        let m = known.iter().find(|m| m.id() == l.market && m.committee_id == l.committee).ok_or(format!("leg {} is priced in a market that is none of those given", i + 1))?;
        if l.scale != m.p.scale || l.price_max != m.p.price_max {
            return Err(format!("leg {} does not carry its market's scale and price limit", i + 1));
        }
        ms.push(m);
    }
    let want = terms_of(ms.first().ok_or("the account has no leg")?, ms.get(1).copied());
    if x.t != want {
        return Err(format!("its terms are not those of its markets: it carries {:?} and its markets give {:?}", x.t, want));
    }
    Ok(())
}

pub(crate) fn cmarket_from(path: &str) -> R<(CMarket, Option<Genesis>)> {
    let v = read_json(path)?;
    let block = from_hex(v.get("block").and_then(Value::as_str).ok_or(format!("{path}: no `block`"))?).map_err(e)?;
    let m = CMarket::decode(&block).map_err(|x| format!("{path}: {x}"))?;
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        if hex32(id)? != m.id() {
            return Err(format!("{path}: the `id` is not the block's id (blake3 of its body)"));
        }
    }
    if v.get("pair").is_some() && cmarket_json(&m, None)["pair"] != v["pair"] {
        return Err(format!("{path}: the plain parameters differ from the block's: the block is what the contracts read"));
    }
    let gen = match v.get("committee") {
        Some(r) => {
            let g = Genesis::from_json(r).map_err(|x| format!("{path}: {x}"))?;
            if g.id().as_bytes() != m.committee_id {
                return Err(format!("{path}: REFUSED: the market names another committee than the genesis facts give"));
            }
            Some(g)
        }
        None => None,
    };
    Ok((m, gen))
}

fn market_words(m: &CMarket) -> String {
    let p = &m.p;
    format!(
        "Prices come only from epoch tables of committee {}: no party states a price, and without the signatures of its quorum nothing settles.\n\
         A claim must land within {} DAA ({}) of the time it uses; the tolerance g is {} % (inside every liquidation trigger and against every closer).\n\
         Maintenance margin {} %; a liquidation can be cured for {} DAA ({}); its proposer is paid {} % of the notional (at least {} KAS).\n\
         A pair's lease is {} DAA ({}); anyone renews it with any signed signed price; {} DAA ({}) after an unrenewed lease anyone settles the pair at its entry price.\n\
         A party entry needs the pair to be {} DAA ({}) old. What a lying quorum can take is every pair of the market: check the committee (`committee-show`).\n",
        to_hex(&m.committee_id),
        p.late_daa,
        secs(p.late_daa),
        p.g_bps as f64 / 100.0,
        p.mmr_bps as f64 / 100.0,
        p.grace_daa,
        secs(p.grace_daa),
        p.liq_fee_bps as f64 / 100.0,
        fmt_kas(p.keeper_fee_min as i128),
        p.term,
        secs(p.term),
        p.idle_daa,
        secs(p.idle_daa),
        p.quiet_daa,
        secs(p.quiet_daa)
    )
}

/// Review C2 (C2-F1): whoever builds a claim chooses the price within the committee's spread of what a quorum signed, and
/// the market's g is what absorbs that. The pair refuses an epoch table whose spread leaves g no margin; so does the tool.
fn spread_or_refuse(m: &CMarket, r: &CommitteeParams) -> R<()> {
    if m.spread_ok(r.spread_bps) {
        // Review C6 (C6-N2): a committee whose members cannot be slashed for a false price is said to be one here too.
        for w in charge_warnings(r) {
            println!("WARNING: {w}");
        }
        return Ok(());
    }
    Err(format!(
        "REFUSED: the committee's spread is {} % and the market's tolerance g {} %: g must be at least {} times the spread (a claimant chooses the price within the spread, and g is what that may cost a party); no epoch table of this committee prices this market",
        r.spread_bps as f64 / 100.0,
        m.p.g_bps as f64 / 100.0,
        SPREAD_MARGIN
    ))
}

/// Parameters inside the contracts' bounds that still make a poor market (review C1, C1-N1; review C2, C2-N1).
fn market_warnings(m: &CMarket, r: &CommitteeParams) -> Vec<String> {
    let p = &m.p;
    let mut w = vec![];
    if p.quiet_daa * 4 >= p.term {
        w.push(format!("quietDaa ({}) is a quarter of the term or more: renewals alone can keep every party entry out", p.quiet_daa));
    }
    if p.mmr_bps <= p.g_bps {
        w.push(format!("the maintenance margin ({} %) is not above g ({} %): a side can be bankrupt before it can be liquidated", p.mmr_bps as f64 / 100.0, p.g_bps as f64 / 100.0));
    }
    if p.late_daa > r.exit {
        w.push(format!("lateDaa ({}) is longer than the committee's exit time ({}): a member's stake is released while its signed prices can still settle claims", p.late_daa, r.exit));
    }
    if p.keeper_fee_min > p.carrier {
        w.push("the proposer's least fee is above the carrier: liquidating in the smallest parts pays more than the fee rate".into());
    }
    if p.idle_daa == 0 {
        w.push("idleDaa is 0: a pair is settled at its entry the moment its lease ends".into());
    }
    if p.late_daa < 30 {
        w.push(format!("lateDaa is {} ({}): a claim must be signed, built and accepted within that of its time", p.late_daa, secs(p.late_daa)));
    }
    if p.late_daa > 600 {
        w.push(format!("lateDaa is {} ({}): a claimant chooses among every signed price of that long", p.late_daa, secs(p.late_daa)));
    }
    if p.grace_daa == 0 {
        w.push("graceDaa is 0: a liquidation is final in the block after it lands; the loser cannot cure".into());
    }
    if r.spread_bps == 0 {
        w.push("the committee's spread is 0: a price needs a quorum to have signed the very same number".into());
    }
    if r.quorum_bps < 6_667 {
        w.push(format!("the committee's quorum is {} %: less than two thirds of the weight dictates every price", r.quorum_bps as f64 / 100.0));
    }
    if r.epoch < 600 {
        w.push(format!("the committee's epochs are {} DAA ({}): somebody must roll that often", r.epoch, secs(r.epoch)));
    }
    w
}

/// `c-market`: a market block from a committee file and a feed definition (testnet defaults, each overridable).
pub fn market_cmd(a: &Args) -> R<()> {
    a.only(&["committee", "feed", "out", "term-daa", "scale", "price-max", "mmr-bps", "g-bps", "liq-fee-bps", "keeper-fee-min", "min-size", "carrier", "idle-daa", "grace-daa", "late-daa", "quiet-daa"])?;
    let gen = Genesis::from_json(&read_json(a.need("committee")?)?)?;
    let feed = std::fs::read(a.need("feed")?).map_err(|x| format!("--feed: {x}"))?;
    let d = CMarket::testnet(gen.id().as_bytes(), feed_hash_of(&feed));
    let amt = |name: &str, dv: i64| -> R<i64> { if a.has(name) { kas(a.need(name)?) } else { Ok(dv) } };
    let m = CMarket {
        committee_id: d.committee_id,
        feed_hash: d.feed_hash,
        p: CParams {
            term: int_or(a, "term-daa", d.p.term)?,
            scale: int_or(a, "scale", d.p.scale)?,
            price_max: price_or(a, "price-max", d.p.price_max)?,
            mmr_bps: int_or(a, "mmr-bps", d.p.mmr_bps)?,
            g_bps: int_or(a, "g-bps", d.p.g_bps)?,
            liq_fee_bps: int_or(a, "liq-fee-bps", d.p.liq_fee_bps)?,
            keeper_fee_min: amt("keeper-fee-min", d.p.keeper_fee_min)?,
            min_size: int_or(a, "min-size", d.p.min_size)?,
            carrier: amt("carrier", d.p.carrier)?,
            idle_daa: int_or(a, "idle-daa", d.p.idle_daa)?,
            grace_daa: int_or(a, "grace-daa", d.p.grace_daa)?,
            late_daa: int_or(a, "late-daa", d.p.late_daa)?,
            quiet_daa: int_or(a, "quiet-daa", d.p.quiet_daa)?,
        },
    };
    m.check().map_err(|x| format!("the parameters fail the contracts' bounds: {x}"))?;
    spread_or_refuse(&m, &gen.p)?;
    let out = PathBuf::from(a.get("out").unwrap_or("kpf-out/committee-market.json"));
    write_json(&out, &cmarket_json(&m, Some(&gen)))?;
    println!("market id   {}", to_hex(&m.id()));
    println!("feed hash   {}", to_hex(&m.feed_hash));
    println!("written     {}", out.display());
    print!("{}", market_words(&m));
    for w in market_warnings(&m, &gen.p) {
        println!("WARNING: {w}");
    }
    Ok(())
}

/// `c-check`: the wallet's check of a market file: the id is the block's, the feed text is the one hashed, the
/// parameters are within the contracts' bounds, and the committee began empty.
pub fn check_cmd(a: &Args) -> R<()> {
    a.only(&["market", "feed"])?;
    let (m, gen) = cmarket_from(a.need("market")?)?;
    m.check().map_err(|x| format!("REFUSED: the market fails the contracts' bounds: {x}"))?;
    if let Some(f) = a.get("feed") {
        let feed = std::fs::read(f).map_err(|x| format!("--feed: {x}"))?;
        if feed_hash_of(&feed) != m.feed_hash {
            return Err("REFUSED: the feed definition is not the one this market hashes".into());
        }
        println!("OK: the feed definition is the market's");
    }
    match gen {
        Some(g) => {
            g.p.check().map_err(|x| format!("REFUSED: the committee's parameters fail the contract's bounds: {x}"))?;
            spread_or_refuse(&m, &g.p)?;
            println!("OK: the committee id is that of one empty committee ({})", committee_words(&g.p));
            for w in market_warnings(&m, &g.p) {
                println!("WARNING: {w}");
            }
        }
        None => return Err("REFUSED: the market file has no committee genesis facts: its committee cannot be checked (a committee that did not begin empty can hold forged epoch tables)".into()),
    }
    println!("market id   {}", to_hex(&m.id()));
    print!("{}", market_words(&m));
    Ok(())
}

// ---------------------------------------------------------------- orders and the open

/// `c-order`: an order of one side, escrowing its margin, its tip and a carrier.
pub fn order_cmd(a: &Args) -> R<()> {
    only(a, &["market", "side", "size", "limit", "rate", "expiry", "owner", "min-counter-rate", "min-fill", "tip", "active-from", "escrow", "unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (m, gen) = cmarket_from(a.need("market")?)?;
    // Review C3 (C3-F2): an order escrows into the market its file names; a file `c-check` would refuse is refused here.
    match &gen {
        Some(g) => spread_or_refuse(&m, &g.p)?,
        None if a.has("unchecked") => println!("WARNING: --unchecked: the market file has no committee genesis facts; nothing shows its committee began empty"),
        None => return Err("REFUSED: the market file has no committee genesis facts (`c-check` refuses it): its committee cannot be checked. --unchecked escrows anyway".into()),
    }
    m.check().map_err(|x| format!("REFUSED: the market fails the contracts' bounds: {x}"))?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let sc = m.p.scale;
    let size = parse_amount(a.need("size")?, sc)?;
    let o = COpen {
        m,
        side: side(a.need("side")?)?,
        owner: match a.get("owner") {
            Some(o) => keys::party(o, net)?,
            None => fk,
        },
        size_left: size,
        limit: price(a, "limit")?,
        my_rate: price(a, "rate")?,
        min_counter_rate: price_or(a, "min-counter-rate", 0)?,
        min_fill: if a.has("min-fill") { parse_amount(a.need("min-fill")?, sc)? } else { size },
        tip: price_or(a, "tip", 0)?,
        active_from: int_or(a, "active-from", 0)?,
        expiry_daa: int(a, "expiry")?,
    };
    if !o.part_ok(size) {
        return Err(format!("REFUSED: no fill can take this order: its size is below the market's minimum or its margin q(size, rate) = {} KAS is below the contracts' part floor of 0.2 KAS", fmt_kas(o.margin_of(size) as i128)));
    }
    if a.has("daa") || node.is_some() {
        let t = now(a, &mut node)? as i64;
        if o.expiry_daa <= t {
            return Err(format!("REFUSED: --expiry {} is not after the present DAA {t}: the order could be refunded at once, and filled until someone does", o.expiry_daa));
        }
    }
    let need = o.spend(size) as u64 + 100_000_000;
    let esc = if a.has("escrow") { kas(a.need("escrow")?)? as u64 } else { need };
    println!("escrow {} KAS (a full fill takes {} KAS of it: the margin, the tip and one carrier)", fmt_kas(esc as i128), fmt_kas(o.spend(size) as i128));
    finish(a, "c-order", &create_order(&o, esc, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// Review C2 (C2-F5): whoever enters a position enters the market its object names. `--market M.json` is the market
/// file the entrant checked (`c-check`: its committee began empty, its feed text is the one hashed); an object of another
/// market is refused, and without the flag the tool says that nothing was compared.
fn known_market(a: &Args, m: &CMarket) -> R<()> {
    match a.get("market") {
        Some(f) => {
            let (mine, gen) = cmarket_from(f)?;
            if mine != *m {
                return Err(format!("REFUSED: the object is of market {}, not of the market file's {}", to_hex(&m.id()), to_hex(&mine.id())));
            }
            if gen.is_none() {
                println!("WARNING: the market file has no committee genesis facts: run `c-check` on a file that has them");
            }
            Ok(())
        }
        None => {
            println!("WARNING: no --market file: the object's market {} and its committee were not compared with one you checked", &to_hex(&m.id())[..16]);
            Ok(())
        }
    }
}

fn lease_of(a: &Args, borns: &[u64], term: i64, t: u64) -> R<i64> {
    let least = borns.iter().max().copied().unwrap_or(0) as i64 + term;
    let xe = if a.has("lease") { int(a, "lease")? } else { (t as i64 + term).max(least) };
    if xe < least {
        return Err(format!("REFUSED: --lease {xe} is less than a term after the youngest order was accepted (DAA {least})"));
    }
    Ok(xe)
}

fn pair_line(p: &CPair) -> String {
    let sc = p.m.p.scale as i128;
    format!(
        "pair [{} .. {}] KAS of {} units at {} KAS; the long is liquidated below {} KAS, the short above {} KAS; lease until DAA {}",
        fmt_kas(p.bl as i128),
        fmt_kas(p.bs as i128),
        fmt_units(p.size as i128, sc),
        fmt_kas(p.entry as i128),
        fmt_kas(p.trigger_of(LONG) as i128),
        fmt_kas(p.trigger_of(SHORT) as i128),
        p.expiry
    )
}

/// `c-open`: two orders open a pair.
pub fn open_cmd(a: &Args) -> R<()> {
    only(a, &["long", "short", "n", "x", "lease", "market"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (lo, so) = (order_of(a, "long", &mut node, net)?, order_of(a, "short", &mut node, net)?);
    known_market(a, &lo.s.m)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    let xe = lease_of(a, &[lo.daa, so.daa], lo.s.m.p.term, t)?;
    let (b, p) = open(&lo, &so, parse_amount(a.need("n")?, lo.s.m.p.scale)?, price(a, "x")?, xe, t, &fu, &fk, rate(a)?).map_err(e)?;
    println!("{}", pair_line(&p));
    finish(a, "c-open", &b, &keys, net, &mut node)
}

/// `c-open-taker`: the funder takes the other side of an order with the margin rate `--rate`.
pub fn open_taker_cmd(a: &Args) -> R<()> {
    only(a, &["order", "n", "x", "rate", "lease", "market"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let o = order_of(a, "order", &mut node, net)?;
    known_market(a, &o.s.m)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    let xe = lease_of(a, &[o.daa], o.s.m.p.term, t)?;
    let (b, p) = open_taker(&o, parse_amount(a.need("n")?, o.s.m.p.scale)?, price(a, "x")?, price(a, "rate")?, fk, xe, t, &fu, &fk, rate(a)?).map_err(e)?;
    println!("{}", pair_line(&p));
    finish(a, "c-open-taker", &b, &keys, net, &mut node)
}

/// `c-cancel`: the order's owner takes its escrow back.
pub fn cancel_cmd(a: &Args) -> R<()> {
    only(a, &["order"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let o = order_of(a, "order", &mut node, net)?;
    let keys = crate::cmds::funder_and_keys(a, false)?.expect("keys").keys;
    finish(a, "c-cancel", &cancel(&o, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `c-refund`: anyone returns an expired order's escrow to its owner.
pub fn refund_cmd(a: &Args) -> R<()> {
    only(a, &["order"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let o = order_of(a, "order", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "c-refund", &refund(&o, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

// ---------------------------------------------------------------- claims

/// Review C4 (C4-N4): a claim whose signed price is older than lateDaa would land void, with `--daa` as with a node, and with
/// a margin for the landing.
fn price_in_time(a: &Args, node: &mut N, t0: i64, late: i64) -> R<()> {
    if a.has("daa") || node.is_some() {
        let age = now(a, node)? as i64 - t0;
        if age > late {
            return Err(format!("REFUSED: the signed price is {age} DAA old, more than the market's lateDaa {late}: a claim with it would land void (the part restored, a split's carrier lost). Use a fresh signed price"));
        }
        if late - age < 30.min(late / 2) {
            println!("WARNING: the signed price is {age} DAA old and the claim must land within {} DAA ({}): if it lands later it is void", late - age, secs(late - age));
        }
    }
    Ok(())
}

/// The price beyond which `side` of a pair can be liquidated (the tolerance g inside the trigger), for display: a
/// long below `bl / ((1 - m)(1 + g))`, a short above `bs / ((1 + m)(1 - g))`. Within two sompi of the last price the
/// contract's rounded rule liquidates at (review C8, C8-N2); `long_liquidatable` and `short_liquidatable` decide.
fn liq_line(p: &CPair, sd: i64) -> i64 {
    let (b, g, m) = (BPS as i128, p.m.p.g_bps as i128, p.m.p.mmr_bps as i128);
    if sd == LONG {
        (p.bl as i128 * b * b / ((b - m) * (b + g))) as i64
    } else {
        (p.bs as i128 * b * b / ((b + m) * (b - g))) as i64
    }
}

fn side_word(sd: i64) -> &'static str {
    if sd == LONG {
        "long"
    } else {
        "short"
    }
}

/// `c-trim`: the owner of a side takes out the margin it has above maintenance at the attested price (its key signs):
/// the side's bound moves inward to `--bound`, or by what frees `--take` KAS, or as far as the price allows (`--max`).
/// A claim like a close: `c-finalize` pays it at once if it landed within lateDaa of its time, and restores the pair
/// otherwise. Cross margin is this and `c-add-margin` on another pair.
pub fn trim_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "side", "epoch-table", "signed-price", "t0", "bound", "take", "max"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let lt = epoch_table_of(a, "epoch-table", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let sd = side(a.need("side")?)?;
    let att = attestation(a, &lp.s.m, &lt)?;
    price_in_time(a, &mut node, att.t0, lp.s.m.p.late_daa)?;
    let p = &lp.s;
    let px = att.p.min(p.m.p.price_max);
    let most = p.trim_bound_of(sd, px);
    let most_words = match most {
        Some(nb) => format!("the nearest bound the attested {} KAS allows is {} KAS, which frees {} KAS", fmt_kas(px as i128), fmt_kas(nb as i128), fmt_kas(p.trim_take(sd, nb) as i128)),
        None => format!("at the attested {} KAS the {} has nothing above its maintenance to take out", fmt_kas(px as i128), side_word(sd)),
    };
    let nb = match (a.get("bound"), a.get("take"), a.has("max")) {
        (Some(_), None, false) => price(a, "bound")?,
        (None, Some(_), false) => p.trim_bound_for(sd, price(a, "take")?).ok_or_else(|| format!("REFUSED: the pair's range does not hold that much; {most_words}"))?,
        (None, None, true) => most.ok_or_else(|| format!("REFUSED: {most_words}"))?,
        _ => return Err("give one of --bound P (the side's new bound), --take KAS (the amount to free) or --max (all the attested price allows)".into()),
    };
    let (b, w) = trim(&lp, &lt, &att, sd, nb, &fu, &fk, rate(a)?).map_err(|x| format!("{x}; {most_words}"))?;
    let (old, t) = (if sd == LONG { p.bl } else { p.bs }, p.trim_state(sd, nb));
    println!(
        "trim: the {}'s bound moves from {} to {} KAS and {} KAS are paid to {} when the claim is finalized (`c-finalize`, at once; void if it lands more than {} DAA after its time)",
        side_word(sd),
        fmt_kas(old as i128),
        fmt_kas(nb as i128),
        fmt_kas(p.trim_take(sd, nb) as i128),
        p2pk_address(&w.proposer, net),
        p.m.p.late_daa
    );
    println!("after it the {} can be liquidated {} {} KAS (now {} {} KAS); the size and the other side's bound do not change", side_word(sd), if sd == LONG { "below" } else { "above" }, fmt_kas(liq_line(&t, sd) as i128), if sd == LONG { "below" } else { "above" }, fmt_kas(liq_line(p, sd) as i128));
    if most == Some(nb) {
        println!("WARNING: this is the nearest bound the price allows: the side keeps no margin above its maintenance but the tolerance g, and a small move against it makes it liquidatable");
    }
    finish(a, "c-trim: margin above maintenance taken out at the attested price", &b, &keys, net, &mut node)
}

fn claim(a: &Args, kind: i64, title: &str) -> R<()> {
    only(a, &["pair", "side", "n", "epoch-table", "signed-price", "t0", "proposer"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let lt = epoch_table_of(a, "epoch-table", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let sd = side(a.need("side")?)?;
    let n = if a.has("n") { parse_amount(a.need("n")?, lp.s.m.p.scale)? } else { lp.s.size };
    let att = attestation(a, &lp.s.m, &lt)?;
    price_in_time(a, &mut node, att.t0, lp.s.m.p.late_daa)?;
    let proposer = match a.get("proposer") {
        Some(p) => keys::party(p, net)?,
        None => fk,
    };
    let (b, w) = propose(&lp, &lt, &att, kind, sd, n, proposer, &fu, &fk, rate(a)?).map_err(e)?;
    if kind == KIND_CLOSE {
        println!("close: paid at {} KAS (the attested price moved by g against the closer); settles at once with `c-finalize`", fmt_kas(w.close_price() as i128));
    } else {
        println!("liquidation: the fee {} KAS goes to {}; the loser can cure for {} DAA ({}), then `c-finalize`", fmt_kas(w.fee as i128), p2pk_address(&w.proposer, net), w.m.p.grace_daa, secs(w.m.p.grace_daa));
    }
    finish(a, title, &b, &keys, net, &mut node)
}

/// `c-close`: a party closes its side at the attested price (its key signs).
pub fn close_cmd(a: &Args) -> R<()> {
    claim(a, KIND_CLOSE, "c-close: a close at the attested price")
}

/// `c-liquidate`: anyone liquidates a side the attested price puts below its maintenance.
pub fn liquidate_cmd(a: &Args) -> R<()> {
    claim(a, KIND_LIQ, "c-liquidate: a liquidation at the attested price")
}

/// `c-finalize`: settle a pending claim, or restore a void one.
pub fn finalize_cmd(a: &Args) -> R<()> {
    only(a, &["pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, s) = finalize(&w, &fu, &fk, rate(a)?).map_err(e)?;
    match &s {
        CSettlement::Close { long, short } => println!("valid (landed {} DAA after its time): the long {} KAS, the short {} KAS", w.daa as i64 - w.s.t0, fmt_kas(*long as i128), fmt_kas(*short as i128)),
        CSettlement::Forfeit { value, fee, .. } => println!("valid: the winner keeps the part ({} KAS), the proposer's fee {} KAS", fmt_kas(*value as i128), fmt_kas(*fee as i128)),
        CSettlement::Restore { .. } => println!("VOID: the claim landed {} DAA after its time (more than lateDaa {}): the part is restored", w.daa as i64 - w.s.t0, w.s.m.p.late_daa),
        CSettlement::Trim { part, take, .. } => println!("valid (landed {} DAA after its time): {} KAS to the {}, whose bound is now {} KAS", w.daa as i64 - w.s.t0, fmt_kas(*take as i128), side_word(w.s.side), fmt_kas(if w.s.side == LONG { part.bl } else { part.bs } as i128)),
    }
    finish(a, "c-finalize", &b, &keys, net, &mut node)
}

/// `c-cure`: the loser of a pending liquidation adds the margin it needs.
pub fn cure_cmd(a: &Args) -> R<()> {
    only(a, &["pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    // Review C3 (C3-F3): a void claim needs no cure: `c-finalize` restores the pair as it was, for nothing.
    if !w.s.valid_at(w.daa as i64) {
        return Err(format!("REFUSED: this claim is void (it landed {} DAA after its time, more than lateDaa {}): do not add margin; `c-finalize` restores the pair as it was", w.daa as i64 - w.s.t0, w.s.m.p.late_daa));
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, c) = cure(&w, &fu, &fk, rate(a)?).map_err(e)?;
    println!("cured: the bound moves to {} KAS", fmt_kas(if w.s.side == LONG { c.bl } else { c.bs } as i128));
    finish(a, "c-cure", &b, &keys, net, &mut node)
}

// ---------------------------------------------------------------- the lease and the price-free entries

/// `c-keep-alive`: renew a pair's lease with any attested signed price of its market.
pub fn keep_alive_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "epoch-table", "signed-price", "t0"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let lt = epoch_table_of(a, "epoch-table", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let att = attestation(a, &lp.s.m, &lt)?;
    let (b, p) = keep_alive(&lp, &lt, &att, &fu, &fk, rate(a)?).map_err(e)?;
    println!("lease until DAA {} (was {})", p.expiry, lp.s.expiry);
    finish(a, "c-keep-alive", &b, &keys, net, &mut node)
}

/// `c-lapse-settle`: settle a pair nobody kept alive at its entry price.
pub fn lapse_cmd(a: &Args) -> R<()> {
    only(a, &["pair"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "c-lapse-settle", &lapse_settle(&lp, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `c-amend`: list (or unlist) a side for a hand-over.
pub fn amend_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "side", "price", "min-part", "tip", "unlist"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let sc = lp.s.m.p.scale;
    let (ask, min, tip) = if a.has("unlist") { (0, 0, 0) } else { (price(a, "price")?, if a.has("min-part") { parse_amount(a.need("min-part")?, sc)? } else { lp.s.size }, price_or(a, "tip", 0)?) };
    let (b, _) = amend(&lp, side(a.need("side")?)?, ask, min, tip, &fu, &fk, rate(a)?).map_err(e)?;
    finish(a, "c-amend", &b, &keys, net, &mut node)
}

/// `c-add-margin`: move a side's bound outward by `--delta` KAS per unit.
pub fn add_margin_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "side", "delta"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, p) = add_margin(&lp, side(a.need("side")?)?, price(a, "delta")?, &fu, &fk, rate(a)?).map_err(e)?;
    println!("{}", pair_line(&p));
    finish(a, "c-add-margin", &b, &keys, net, &mut node)
}

fn hand(a: &Args, assigned: bool) -> R<()> {
    only(a, &["pair", "side", "n", "x", "nb", "entrant", "market"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    known_market(a, &lp.s.m)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let sd = side(a.need("side")?)?;
    let n = if a.has("n") { parse_amount(a.need("n")?, lp.s.m.p.scale)? } else { lp.s.size };
    let px = price(a, "x")?;
    // The entrant's bound defaults to the leaver's own: the same margin.
    let nb = price_or(a, "nb", if sd == LONG { lp.s.bl } else { lp.s.bs })?;
    let entrant = match a.get("entrant") {
        Some(o) => keys::party(o, net)?,
        None => fk,
    };
    let (b, p) = if assigned { assign(&lp, sd, n, px, nb, entrant, &fu, &fk, rate(a)?) } else { transfer(&lp, sd, n, px, nb, entrant, &fu, &fk, rate(a)?) }.map_err(e)?;
    println!("{}", pair_line(&p));
    finish(a, if assigned { "c-assign: a side signed over in one transaction" } else { "c-transfer: a listed side changes hands" }, &b, &keys, net, &mut node)
}

/// `c-assign`: the owner of a side signs it over to `--entrant` (default the funder) at `--x`: the leaver's key
/// (`--key`) and the entrant's funding sign one transaction; the leaver is paid in it.
pub fn assign_cmd(a: &Args) -> R<()> {
    hand(a, true)
}

/// `c-transfer`: a listed side goes to `--entrant` (default the funder) at `--x` within its listing.
pub fn transfer_cmd(a: &Args) -> R<()> {
    hand(a, false)
}

/// `c-take`: a resting order takes a listed side.
pub fn take_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "order", "n", "x", "market"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    known_market(a, &lp.s.m)?;
    let lo = order_of(a, "order", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let n = if a.has("n") { parse_amount(a.need("n")?, lp.s.m.p.scale)? } else { lp.s.size.min(lo.s.size_left) };
    let (b, p) = take(&lp, &lo, n, price(a, "x")?, &fu, &fk, rate(a)?).map_err(e)?;
    println!("{}", pair_line(&p));
    finish(a, "c-take: an order takes a listed side", &b, &keys, net, &mut node)
}

/// `c-mutual-close`: both sides listed and crossing: anyone closes the pair at `--x`.
pub fn mutual_close_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "x"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "c-mutual-close", &mutual_close(&lp, price(a, "x")?, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `c-coop-close`: both owners sign any split.
pub fn coop_close_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "long-pay"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let keys = crate::cmds::funder_and_keys(a, false)?.expect("keys").keys;
    finish(a, "c-coop-close", &coop_close(&lp, kas(a.need("long-pay")?)? as u64, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

// ---------------------------------------------------------------- inspect, quote, watch

fn pair_state(p: &CPair, price_now: Option<i64>, t: Option<i64>) {
    println!("{}", pair_line(p));
    println!("long   {}   listing ask {} (min part {}, tip {})", to_hex(&p.long_key), fmt_kas(p.long_ask as i128), p.long_min, fmt_kas(p.long_tip as i128));
    println!("short  {}   listing bid {} (min part {}, tip {})", to_hex(&p.short_key), fmt_kas(p.short_bid as i128), p.short_min, fmt_kas(p.short_tip as i128));
    if let Some(f) = price_now {
        let state = if p.long_liquidatable(f) {
            "the LONG can be LIQUIDATED at this price"
        } else if p.short_liquidatable(f) {
            "the SHORT can be LIQUIDATED at this price"
        } else {
            "neither side can be liquidated at this price"
        };
        println!("at {} KAS: the long's side is worth {} KAS, the short's {} KAS; {state}", fmt_kas(f as i128), fmt_kas(p.long_at(f) as i128), fmt_kas(p.short_at(f) as i128));
    }
    match t {
        Some(t) if t >= p.lapse_from() as i64 => println!("LAPSED: anyone can settle this pair at its entry price now (`c-lapse-settle`)"),
        Some(t) if t >= p.expiry => println!("the lease ENDED {} DAA ago: in {} DAA ({}) anyone settles this pair at its entry price; a signed price renews it (`c-keep-alive`)", t - p.expiry, p.lapse_from() as i64 - t, secs(p.lapse_from() as i64 - t)),
        Some(t) => println!(
            "lease ends in {} DAA ({}); settled at the entry {} DAA ({}) later unless renewed; a signed price from DAA {} on renews it (`c-keep-alive`)",
            p.expiry - t,
            secs(p.expiry - t),
            p.m.p.idle_daa,
            secs(p.m.p.idle_daa),
            p.keep_alive_from()
        ),
        None => println!("lease until DAA {}; lapse settle from DAA {}; a signed price from DAA {} on renews it", p.expiry, p.lapse_from(), p.keep_alive_from()),
    }
}

fn pending_state(w: &CPending, landed: Option<u64>) {
    let kind = match w.kind {
        KIND_CLOSE => "close",
        KIND_TRIM => "trim",
        _ => "liquidation",
    };
    let sd = if w.side == LONG { "long" } else { "short" };
    println!("a {kind} of the {sd} at {} KAS for the time {} ({} units); proposer {}", fmt_kas(w.quote as i128), w.t0, fmt_units(w.n as i128, w.m.p.scale as i128), to_hex(&w.proposer));
    if w.kind == KIND_TRIM {
        println!("the {sd} takes out {} KAS: its bound moves from {} to {} KAS if the claim is valid", fmt_kas(w.pair_part().trim_take(w.side, w.cure_bound) as i128), fmt_kas(if w.side == LONG { w.bl } else { w.bs } as i128), fmt_kas(w.cure_bound as i128));
    }
    match landed {
        Some(p) if w.valid_at(p as i64) => println!("VALID: landed at DAA {p}, {} after its time; finalize from DAA {} (`c-finalize`){}", p as i64 - w.t0, p + w.finalize_age(p as i64), if w.kind == KIND_LIQ && w.cure_bound != 0 { format!("; the loser may cure to {} KAS until then (`c-cure`)", fmt_kas(w.cure_bound as i128)) } else { String::new() }),
        Some(p) => println!("VOID: landed at DAA {p}, {} after its time (more than lateDaa {}): `c-finalize` restores the part", p as i64 - w.t0, w.m.p.late_daa),
        None => println!("valid only if it landed by DAA {} (its DAA is unknown: give --node)", w.last_valid_landing()),
    }
}

/// The markets an object is priced in, each with the committee that prices it (review C14, C14-F1: an object in a
/// market of somebody's own committee is priced as that somebody likes), and the DAA after which it is no longer what it
/// says (an order's expiry, the end of a lease).
pub(crate) fn markets_of(c: &CObj) -> (Vec<([u8; 32], [u8; 32])>, Option<i64>) {
    let legs = |x: &Cross| x.legs.iter().filter(|l| l.size > 0).map(|l| (l.market, l.committee)).collect::<Vec<_>>();
    match c {
        CObj::Pair(p) => (vec![(p.m.id(), p.m.committee_id)], Some(p.expiry)),
        CObj::Pending(w) => (vec![(w.m.id(), w.m.committee_id)], None),
        CObj::Order(o) => (vec![(o.m.id(), o.m.committee_id)], Some(o.expiry_daa)),
        CObj::Cross(x) => (legs(x), Some(x.expiry)),
        CObj::PendingX(w) => (legs(&w.account), None),
        _ => (vec![], None),
    }
}

/// The keys an object of the committee family names as its parties (whose it is).
pub(crate) fn party_keys(c: &CObj) -> Vec<[u8; 32]> {
    match c {
        CObj::Pair(p) => vec![p.long_key, p.short_key],
        CObj::Pending(w) => vec![w.long_key, w.short_key, w.proposer],
        CObj::Order(o) => vec![o.owner],
        CObj::Exit(x) => vec![x.key],
        CObj::Committee(r) => r.members.iter().filter(|s| s.taken()).map(|s| s.key).collect(),
        CObj::EpochTable(_) => vec![],
        CObj::Charge(w) => vec![w.stake_key, w.pay_key],
        CObj::Cross(x) => vec![x.a_key, x.b_key],
        CObj::PendingX(w) => vec![w.a_key, w.b_key, w.proposer],
    }
}

/// What a charge says and, with the present, whether it needs an action now.
fn charge_state(w: &Charge, born: Option<u64>, t: Option<i64>, me: Option<[u8; 32]>, net: Network) -> bool {
    let charged = me == Some(w.stake_key);
    let what = match w.status {
        CHARGE_PENDING => "pending",
        CHARGE_CONTESTED => "with the judge",
        CHARGE_UPHELD => "upheld (the judge found the signed price true)",
        CHARGE_REFUTED => "refuted (the judge found the signed price false)",
        CHARGE_LAPSED => "lapsed (contested too late)",
        _ => "of an unknown status",
    };
    println!("a charge that {} signed {} KAS for the time {} of market {}: {what}", to_hex(&w.stake_key), fmt_kas(w.price as i128), w.t0, &to_hex(&w.market)[..16]);
    println!("  if the signed price is refuted the bond and a tenth of the stake go to {}", p2pk_address(&w.pay_key, net));
    match w.status {
        CHARGE_PENDING => {
            let Some(b) = born else { return false };
            let end = b as i64 + w.p.daa;
            println!("  it waits for a contest until DAA {end} (`charge-contest`, with judge stake accepted at least {} DAA before DAA {b})", w.p.judge.p.age);
            if t.is_some_and(|t| t >= end) {
                println!("ACT: nobody contested it: the hold can be ended (`charge-settle`); its bond is burned");
                return true;
            }
            if charged {
                println!("ACT: this charge is against your key. If it is contested and the signed price was true, answer on YES with aged judge stake (`charge-answer`): a contest nobody answers within a round is a verdict against you");
            }
            charged
        }
        CHARGE_CONTESTED => {
            println!("  the judge's dispute decides it (`charge-decide`, once its round has closed); a contest nobody answers is a verdict for NO");
            if charged {
                println!("ACT: this charge is against your key and is contested. If the signed price was true, answer on YES with aged judge stake now (`charge-answer`): the answer takes the contest stake");
            }
            charged
        }
        CHARGE_UPHELD => {
            println!("ACT: end the hold (`charge-settle`): the bond goes to the charged key");
            true
        }
        CHARGE_REFUTED => {
            println!("ACT: slash the stake (`charge-settle`): the verdict takes up to {} KAS, what its NO side had at risk. (If the stake is gone, slashed whole by another charge, nothing is left to settle and the bond stays in the charge.)", fmt_kas(w.clock as i128));
            true
        }
        CHARGE_LAPSED => {
            println!("ACT: end the hold (`charge-settle`): the bond is burned");
            true
        }
        _ => false,
    }
}

/// `inspect` of a committee object.
pub fn inspect_committee(a: &Args, c: &CObj, rs: &[u8], file: Option<&ObjFile>, net: Network) -> R<()> {
    if a.has("json") {
        let mut doc = json!({ "kind": c.kind(), "family": "committee", "network": net.name(), "address": p2sh_address(rs, net), "redeemBytes": rs.len(), "state": format!("{c:?}") });
        if let Some(f) = file {
            doc["utxo"] = json!({"outpoint": outpoint_json(&f.op), "amount": f.amount, "daa": f.daa, "covenantId": f.covenant_id.to_string()});
        }
        println!("{}", serde_json::to_string_pretty(&doc).map_err(e)?);
        return Ok(());
    }
    println!("{}   {}", c.kind(), p2sh_address(rs, net));
    match c {
        CObj::Committee(r) => {
            println!("{}", committee_words(&r.p));
            if let Some(f) = file {
                show_committee(&live_of(f, *r).unwrap_or(OLive { s: *r, value: f.amount, id: None, op: f.op, daa: 0 }), None);
            }
        }
        CObj::EpochTable(t) => {
            println!("serves signed prices [{}, {}); total {} KAS, a price needs {} KAS; retire from DAA {}", t.from, t.until, fmt_kas(t.total as i128), fmt_kas(t.quorum() as i128), t.retire_from());
            for (i, s) in t.members.iter().enumerate().filter(|(_, s)| s.taken()) {
                println!("  member {i:>2}  {} KAS  key {}", fmt_kas(s.weight as i128), to_hex(&s.key));
            }
        }
        CObj::Exit(x) => println!("{} KAS to {} from DAA {} (`exit-withdraw`){}", fmt_kas(x.amount as i128), p2pk_address(&x.key, net), x.release, if x.hold == NO_HOLD { "" } else { "; HELD for a charge: not withdrawn until its charges are settled" }),
        CObj::Charge(w) => {
            charge_state(w, file.and_then(|f| f.daa), None, None, net);
        }
        CObj::Order(o) => {
            let sc = o.m.p.scale as i128;
            println!("{} {} units, limit {} KAS, margin rate {} KAS, min counter rate {}, min fill {}, tip {}, expires at DAA {}; owner {}", if o.side == LONG { "long" } else { "short" }, fmt_units(o.size_left as i128, sc), fmt_kas(o.limit as i128), fmt_kas(o.my_rate as i128), fmt_kas(o.min_counter_rate as i128), fmt_units(o.min_fill as i128, sc), fmt_kas(o.tip as i128), o.expiry_daa, to_hex(&o.owner));
            println!("market {}   priced by the committee {}", to_hex(&o.m.id()), to_hex(&o.m.committee_id));
        }
        CObj::Pair(p) => {
            pair_state(p, None, None);
            println!("market {}   priced by the committee {}", to_hex(&p.m.id()), to_hex(&p.m.committee_id));
        }
        CObj::Pending(w) => pending_state(w, file.and_then(|f| f.daa)),
        CObj::Cross(x) => cross_state(x, None, None, net),
        CObj::PendingX(w) => xpending_state(w, file.and_then(|f| f.daa), file.map(|f| f.amount)),
    }
    if let Some(f) = file {
        println!("utxo {}:{}  {} KAS  DAA {}  covenant id {}", f.op.transaction_id, f.op.index, fmt_kas(f.amount as i128), crate::cmds::daa_str(f.daa), crate::cmds::short_hash(&f.covenant_id));
    }
    Ok(())
}

/// `c-watch`: the state of the given objects for the key `--me` (or everything, without it): pairs with their lease
/// and, with `--price` (the feed now), each side's value and whether it can be liquidated; pending claims with their
/// validity and when they can be finalized; exits and epoch tables with their times. One query per object with `--node`;
/// it does not poll. Exit code 3 when something needs an action now.
pub fn watch_cmd(a: &Args) -> R<()> {
    a.only(&["me", "object", "scan", "daa", "node", "price"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let me = match a.get("me") {
        Some(k) => Some(keys::party(k, net)?),
        None => None,
    };
    let t = if a.has("daa") || node.is_some() { Some(now(a, &mut node)? as i64) } else { None };
    let feed = if a.has("price") { Some(price(a, "price")?) } else { None };
    let mut files: Vec<String> = a.all("object").iter().map(|s| s.to_string()).collect();
    for d in a.all("scan") {
        for ent in std::fs::read_dir(d).map_err(|x| format!("--scan {d}: {x}"))?.flatten() {
            let p = ent.path();
            if p.extension().is_some_and(|x| x == "json") {
                files.push(p.to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    let mut act = 0;
    for path in &files {
        let Ok(mut f) = ObjFile::load(path) else { continue };
        let Ok(c) = CObj::decode(&f.redeem) else { continue };
        let mine = |keys: &[[u8; 32]]| me.is_none_or(|m| keys.contains(&m));
        let relevant = match &c {
            CObj::Pair(p) => mine(&[p.long_key, p.short_key]),
            CObj::Pending(w) => mine(&[w.long_key, w.short_key, w.proposer]),
            CObj::Order(o) => mine(&[o.owner]),
            CObj::Exit(x) => mine(&[x.key]),
            CObj::Committee(r) => me.is_none_or(|m| r.members.iter().any(|s| s.key == m)),
            CObj::EpochTable(_) => me.is_none(),
            CObj::Charge(w) => mine(&[w.stake_key, w.pay_key]),
            CObj::Cross(x) => mine(&[x.a_key, x.b_key]),
            CObj::PendingX(w) => mine(&[w.a_key, w.b_key, w.proposer]),
        };
        if !relevant {
            continue;
        }
        println!("-- {path}");
        if let Some(n) = node.as_mut() {
            if let Err(x) = crate::cmds::refresh(n, &mut f, net) {
                // Review C2 (C2-F3): a pair or a part that is gone was spent by somebody: a claim, a renewal, a hand-over.
                // The tool cannot follow it (a node lists UTXOs by address, and the next object's address depends on
                // its whole state), so it does not say "nothing to do": it says that something happened.
                println!("NOT AT THE NODE: {x}");
                if matches!(c, CObj::Pair(_) | CObj::Pending(_)) {
                    println!("ACT: this object was spent (a claim, a renewal, a hand-over or a settlement): find the transaction that spent it; nothing in this file is current");
                    act += 1;
                }
                continue;
            }
        }
        match &c {
            CObj::Pair(p) => {
                pair_state(p, feed, t);
                if let Some(px) = feed {
                    if p.long_liquidatable(px) || p.short_liquidatable(px) {
                        act += 1;
                    }
                }
                if t.is_some_and(|t| t >= p.lapse_from() as i64) {
                    act += 1;
                } else if t.is_some_and(|t| p.expiry - t <= p.m.p.term / 4) {
                    // The last quarter of the lease: a claim, a cure or a restore that lands after the lapse date is
                    // settled at the entry (C1-F7), which a party that wants its position avoids by renewing now.
                    println!("ACT: the lease is in its last quarter: renew it (`c-keep-alive`) or close");
                    act += 1;
                }
            }
            CObj::Pending(w) => {
                pending_state(w, f.daa);
                if let (Some(t), Some(p)) = (t, f.daa) {
                    if t as u64 >= p + w.finalize_age(p as i64) {
                        println!("ACT: it can be finalized now");
                        act += 1;
                    } else if w.kind == KIND_LIQ && w.cure_bound != 0 && me.is_some_and(|m| m == if w.side == LONG { w.long_key } else { w.short_key }) {
                        println!("ACT: your side is being liquidated: cure it before DAA {} (`c-cure`) or it is forfeited", p + w.finalize_age(p as i64));
                        act += 1;
                    }
                }
            }
            CObj::Charge(w) => {
                if charge_state(w, f.daa, t, me, net) {
                    act += 1;
                }
            }
            CObj::Exit(x) if x.hold != NO_HOLD => {
                println!("an exit of {} KAS released at DAA {}, HELD for a charge: it is not withdrawn until its charges are settled (`charge-settle`)", fmt_kas(x.amount as i128), x.release);
                if me == Some(x.key) {
                    println!("ACT: your stake on its way out is charged. If the signed price was true, answer its contest on YES with aged judge stake (`charge-answer --exit`): a contest nobody answers within a round is a verdict against you");
                    act += 1;
                }
            }
            CObj::Exit(x) => {
                println!("an exit of {} KAS released at DAA {}", fmt_kas(x.amount as i128), x.release);
                if t.is_some_and(|t| t >= x.release) {
                    println!("ACT: it can be withdrawn now (`exit-withdraw`)");
                    act += 1;
                }
            }
            CObj::Committee(r) => {
                if let Ok(l) = live_of(&f, *r) {
                    show_committee(&l, t);
                }
                // Review C5 (C5-F1): a contest nobody answers is a verdict, so a member must learn that it is charged.
                for (i, s) in r.members.iter().enumerate().filter(|(i, _)| r.holds[*i] != NO_HOLD) {
                    println!("member {i:>2} is HELD for a charge: its stake is not withdrawn until its charges are settled");
                    if me == Some(s.key) {
                        println!("ACT: your member is charged. Find the charge (the transaction that last spent the committee wrote it); if the signed price was true, answer its contest on YES with aged judge stake (`charge-answer`): a contest nobody answers within a round is a verdict against you");
                        act += 1;
                    }
                }
                if t.is_some_and(|t| r.roll_steps(t).is_some()) {
                    act += 1;
                }
            }
            CObj::EpochTable(tk) => println!("an epoch table of [{}, {}){}", tk.from, tk.until, if t.is_some_and(|t| t >= tk.retire_from()) { ": it can be retired (`epoch-table-retire`)" } else { "" }),
            CObj::Cross(x) => {
                cross_state(x, None, t, net);
                if t.is_some_and(|t| t >= x.lapse_from() as i64) {
                    act += 1;
                } else if t.is_some_and(|t| x.expiry - t <= x.t.term / 4) {
                    println!("ACT: the lease is in its last quarter: renew it (`x-keep-alive`) or close");
                    act += 1;
                }
            }
            CObj::PendingX(w) => {
                xpending_state(w, f.daa, Some(f.amount));
                if let (Some(t), Some(p)) = (t, f.daa) {
                    if t as u64 >= p + w.finalize_age(p as i64) {
                        println!("ACT: it can be finalized now (`x-finalize`)");
                        act += 1;
                    } else if w.kind == KIND_LIQ && me.is_some_and(|m| m == if w.side == SIDE_A { w.a_key } else { w.b_key }) {
                        println!("ACT: your side of the account is being liquidated: cure it before DAA {} (`x-cure`) or the whole account is forfeited", p + w.finalize_age(p as i64));
                        act += 1;
                    }
                }
            }
            CObj::Order(o) => {
                println!("an order of {} units resting (expires at DAA {})", fmt_units(o.size_left as i128, o.m.p.scale as i128), o.expiry_daa);
                // Review C2 (C2-F2): no contract can refuse a fill after the expiry; it binds only when a refund lands.
                if t.is_some_and(|t| t >= o.expiry_daa) {
                    println!("ACT: the order has expired and can still be filled: `c-cancel` (its owner) or `c-refund` (anyone, paid {} KAS of the escrow)", fmt_kas(o.m.p.keeper_fee_min as i128));
                    act += 1;
                }
            }
        }
    }
    if act > 0 {
        return Err(format!("{ACT_NOW}: {act} object(s) need an action now"));
    }
    Ok(())
}

/// One side of a pair as a position of an account.
struct Position {
    path: String,
    sd: i64,
    p: CPair,
    px: i64,
    value: i64,
    line: i64,
    dist_bps: i64,
    free: i64,
    /// What moves its liquidation line `--target-bps` from the price, when it is within `--alert-bps` of it: the
    /// distance its bound moves outward and the KAS that costs.
    need: Option<(i64, i64)>,
}

/// The side of `me` in a pair as a position at the price `px`: its worth, its liquidation line and how far that is,
/// its margin above maintenance, and what it needs when the line is within `alert` of the price.
fn position_of(path: &str, p: CPair, me: &[u8; 32], px: i64, alert: i64, target: i64) -> Position {
    let b = BPS as i128;
    let sd = if p.long_key == *me { LONG } else { SHORT };
    let (g, m) = (p.m.p.g_bps as i128, p.m.p.mmr_bps as i128);
    let line = liq_line(&p, sd);
    let dist_bps = if px > 0 { ((if sd == LONG { px - line } else { line - px }) as i128 * b / px as i128) as i64 } else { 0 };
    let free = p.trim_bound_of(sd, px).map_or(0, |nb| p.trim_take(sd, nb));
    let need = (dist_bps < alert).then(|| {
        // The bound whose liquidation line is `target` from the price.
        let delta = if sd == LONG {
            let want = px as i128 * (b - target as i128) / b;
            p.bl - (want * (b - m) * (b + g) / (b * b)).max(0) as i64
        } else {
            let want = px as i128 * (b + target as i128) / b;
            ((want * (b + m) * (b - g) + b * b - 1) / (b * b)).min(p.m.p.price_max as i128) as i64 - p.bs
        }
        .max(0);
        (delta, q(p.size, delta, p.m.p.scale, p.m.p.scale - 1))
    });
    let value = if sd == LONG { p.long_at(px) } else { p.short_at(px) };
    Position { path: path.to_string(), sd, p, px, value, line, dist_bps, free, need }
}

/// The JSON files of a directory (not of its subdirectories), sorted.
fn json_files(dir: &str) -> R<Vec<String>> {
    let mut v: Vec<String> = std::fs::read_dir(dir).map_err(|x| format!("{dir}: {x}"))?.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json")).map(|p| p.to_string_lossy().into_owned()).collect();
    v.sort();
    Ok(v)
}

/// The newest signed price of `m` in the directory `signed prices` that an epoch table of the directory `epoch tables` attests and that is no
/// older than half of lateDaa at the present `t` (a claim must land within lateDaa of its time): the epoch table's file,
/// the signed price files and the attested price.
fn newest_attested(m: &CMarket, signed_prices: &str, epoch_tables: &[&str], t: i64, node: &mut N, net: Network) -> R<(String, Vec<String>, Attestation)> {
    let mut by_t0: std::collections::BTreeMap<i64, Vec<String>> = Default::default();
    for f in json_files(signed_prices)? {
        let name = std::path::Path::new(&f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(t0) = name.strip_prefix("price-").and_then(|r| r.split('-').next()).and_then(|n| n.parse::<i64>().ok()) {
            by_t0.entry(t0).or_default().push(f);
        }
    }
    // The committee's epoch table files, the newest first: every attestation spends an epoch table and writes its file again, so
    // the older files of an epoch table are of spent UTXOs. With a node the first that is still there is used.
    let mut all: Vec<String> = vec![];
    for d in epoch_tables {
        all.extend(json_files(d)?);
    }
    let mut epoch_tables: Vec<(std::time::SystemTime, String, ObjFile, EpochTable)> = all
        .into_iter()
        .filter_map(|f| {
            let o = ObjFile::load(&f).ok()?;
            let at = std::fs::metadata(&f).and_then(|x| x.modified()).ok()?;
            match CObj::decode(&o.redeem).ok()? {
                CObj::EpochTable(tk) if o.covenant_id.as_bytes() == m.committee_id => Some((at, f, o, tk)),
                _ => None,
            }
        })
        .collect();
    epoch_tables.sort_by_key(|x| std::cmp::Reverse(x.0));
    let mut why = format!("no signed price of the market {} within {} DAA of the present in {signed_prices}", &to_hex(&m.id())[..16], m.p.late_daa / 2);
    for (t0, files) in by_t0.range(t - m.p.late_daa / 2..=t).rev() {
        let signed: Vec<(String, SignedPrice)> = files.iter().filter_map(|f| price_raw(f).ok().filter(|(_, r, mk)| *r == m.committee_id && *mk == m.id()).map(|(s, _, _)| (f.clone(), s))).collect();
        if signed.is_empty() {
            continue;
        }
        let mut live = None;
        for (_, tf, o, tk) in epoch_tables.iter_mut().filter(|(_, _, _, tk)| tk.from <= *t0 && *t0 < tk.until).take(8) {
            if node.as_mut().is_none_or(|n| crate::cmds::refresh(n, o, net).is_ok()) {
                live = Some((tf.clone(), *tk));
                break;
            }
        }
        let Some((tf, tk)) = live else {
            why = format!("no epoch table of the market's committee serves the time {t0} (or none of its files is of a UTXO at the node): roll the committee (`committee-roll`) and keep its epoch table files in the epoch tables directory");
            continue;
        };
        let list: Vec<SignedPrice> = signed.iter().map(|(_, s)| *s).collect();
        match Attestation::collect(&tk, &m.committee_id, &m.id(), *t0, &list) {
            Ok(att) => return Ok((tf, signed.into_iter().map(|(f, _)| f).collect(), att)),
            Err(x) => why = format!("the time {t0} has no price: {x}"),
        }
    }
    Err(why)
}

/// What `c-margin-run` trims beyond a position's need: the fees of the trim, its finalize and the margin added.
const FEE_FLOAT: i64 = 20_000_000;

/// What a round of `c-margin-run` remembers of a file between rounds: whether the node has shown its UTXO, and how
/// many rounds in a row it has not.
#[derive(Default)]
struct Seen {
    at_node: bool,
    misses: u32,
}

/// One round of `c-margin-run` at the present `t`: the number of transactions it built.
#[allow(clippy::too_many_arguments)]
fn cross_round(a: &Args, net: Network, node: &mut N, me: [u8; 32], t: i64, alert: i64, target: i64, seen: &mut std::collections::HashMap<String, Seen>, known: &mut std::collections::HashSet<String>) -> R<usize> {
    let (key, dir) = (a.need("key")?, a.need("dir")?);
    let (signed_prices, epoch_tables) = (a.get("signed-prices").unwrap_or(dir), a.get("epoch-tables").unwrap_or(dir));
    // A command of this tool, run in this process with the account's key as signer and funder, writing into `dir`.
    let sub = |args: &[&str]| -> R<()> {
        let mut v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        v.extend(["--key", key, "--funder", key, "--out-dir", dir].map(String::from));
        for k in ["node", "funding", "fee-rate", "daa"] {
            if let Some(x) = a.get(k) {
                v.extend([format!("--{k}"), x.to_string()]);
            }
        }
        if a.has("submit") {
            v.push("--submit".into());
        }
        println!("-- kpf {}", args.join(" "));
        crate::run(&v)
    };
    let mut done = 0;
    let (mut pairs, mut trim_pending) = (Vec::<(String, CPair)>::new(), false);
    // Review C8 (C8-F1): the positions of the key are the files the runner was started with and the files its own
    // transactions wrote. A pair is a script and a state, and anyone writes one that names this key: a file that
    // appears from elsewhere is not a position the key's owner opened, and margin put into it is its maker's.
    let before: Vec<String> = json_files(dir)?;
    let adopt = |known: &mut std::collections::HashSet<String>| {
        for f in json_files(dir).unwrap_or_default() {
            if !before.contains(&f) {
                known.insert(f);
            }
        }
    };
    for path in before.clone() {
        let Ok(mut f) = ObjFile::load(&path) else { continue };
        let Ok(c) = CObj::decode(&f.redeem) else { continue };
        let mine = match &c {
            CObj::Pair(p) => (p.long_key == me) != (p.short_key == me),
            CObj::Pending(w) => w.long_key == me || w.short_key == me,
            _ => false,
        };
        if !mine {
            continue;
        }
        if !known.contains(&path) {
            println!("{path}: NOT MANAGED: it appeared while the runner was running and the runner did not write it. If it is a position you opened, restart the runner");
            continue;
        }
        if let Some(n) = node.as_mut() {
            let s = seen.entry(path.clone()).or_default();
            if crate::cmds::refresh(n, &mut f, net).is_err() {
                // Spent (a claim, a hand-over, this runner's own transaction), or just written and not accepted
                // yet: out of the way once the node has shown it before, or has not shown it for three rounds.
                s.misses += 1;
                if s.at_node || s.misses >= 3 {
                    let gone = std::path::Path::new(dir).join("spent");
                    let _ = std::fs::create_dir_all(&gone);
                    if let Some(name) = std::path::Path::new(&path).file_name() {
                        let _ = std::fs::rename(&path, gone.join(name));
                    }
                    seen.remove(&path);
                }
                continue;
            }
            (s.at_node, s.misses) = (true, 0);
        }
        match c {
            CObj::Pending(w) => {
                let own = (if w.side == LONG { w.long_key } else { w.short_key }) == me;
                let Some(landed) = f.daa else { continue };
                if w.kind == KIND_TRIM {
                    trim_pending |= own;
                    // A trim settles (or, void, is restored) at once.
                    match sub(&["c-finalize", "--pending", &path]) {
                        Ok(()) => {
                            done += 1;
                            adopt(known);
                        }
                        Err(x) => println!("not finalized: {x}"),
                    }
                } else if w.kind == KIND_LIQ && own && w.valid_at(landed as i64) && w.cure_bound != 0 {
                    println!("a position of yours is being liquidated: curing it");
                    match sub(&["c-cure", "--pending", &path]) {
                        Ok(()) => {
                            done += 1;
                            adopt(known);
                        }
                        Err(x) => println!("NOT CURED: {x}"),
                    }
                }
            }
            CObj::Pair(p) => pairs.push((path, p)),
            _ => {}
        }
    }
    // The price of each market: the newest signed price an epoch table attests.
    let mut prices: Vec<([u8; 32], R<(String, Vec<String>, Attestation)>)> = vec![];
    for (_, p) in &pairs {
        if !prices.iter().any(|(id, _)| *id == p.m.id()) {
            prices.push((p.m.id(), newest_attested(&p.m, signed_prices, &[dir, epoch_tables], t, node, net)));
        }
    }
    let mut pos = Vec::<Position>::new();
    let mut proofs = Vec::<(String, Vec<String>)>::new();
    for (path, p) in pairs {
        match &prices.iter().find(|(id, _)| *id == p.m.id()).expect("a price or its absence for every market").1 {
            Ok((epoch_table, files, att)) => {
                // Review C8 (C8-F2): a pair nobody keeps alive settles at its entry, where a side trimmed past its
                // entry is owed nothing: the runner renews a lease in its second half, with the signed price it prices by.
                if t >= p.expiry - p.m.p.term / 2 && att.t0 >= p.keep_alive_from() {
                    let mut args = vec!["c-keep-alive", "--pair", path.as_str(), "--epoch-table", epoch_table.as_str()];
                    for tf in files {
                        args.extend(["--signed-price", tf.as_str()]);
                    }
                    match sub(&args) {
                        Ok(()) => {
                            done += 1;
                            adopt(known);
                            continue;
                        }
                        Err(e) => println!("lease not renewed: {e}"),
                    }
                }
                pos.push(position_of(&path, p, &me, att.p, alert, target));
                proofs.push((epoch_table.clone(), files.clone()));
            }
            Err(x) => println!("{path}: not watched this round: {x}"),
        }
    }
    let mut left: Vec<(usize, i64)> = pos.iter().enumerate().filter(|(_, x)| x.need.is_none() && x.free > 0).map(|(i, x)| (i, x.free)).collect();
    left.sort_by_key(|(_, f)| std::cmp::Reverse(*f));
    for x in pos.iter().filter(|x| x.need.is_some_and(|(d, _)| d > 0)) {
        let (delta, cost) = x.need.unwrap();
        println!("the {} of {} is {:.2} % from its liquidation at {} KAS: it needs {} KAS", side_word(x.sd), x.path, x.dist_bps as f64 / 100.0, fmt_kas(x.px as i128), fmt_kas(cost as i128));
        if t >= x.p.expiry - x.p.m.p.term / 4 {
            println!("REFUSED: no margin is put into {}: its lease is in its last quarter and was not renewed (a pair that lapses settles at its entry, whatever was added)", x.path);
            continue;
        }
        // From the wallet when it holds enough (what an earlier trim freed is there); otherwise from the positions
        // that have margin above maintenance, one trim at a time.
        match sub(&["c-add-margin", "--pair", &x.path, "--side", side_word(x.sd), "--delta", &fmt_kas(delta as i128)]) {
            Ok(()) => {
                done += 1;
                adopt(known);
                continue;
            }
            Err(e) => println!("not added from the wallet: {e}"),
        }
        if trim_pending {
            println!("a trim of yours is pending: its margin is added once it is paid");
            continue;
        }
        // With what the three transactions cost, so that the wallet's float for fees is not what pays the margin.
        let mut rest = cost + FEE_FLOAT;
        for (i, f) in left.iter_mut().filter(|(_, f)| *f > 0) {
            if rest <= 0 {
                break;
            }
            let take = rest.min(*f).max(kpf_protocol::state::PLAIN_DUST).min(*f);
            let s = &pos[*i];
            let (epoch_table, files) = &proofs[*i];
            let mut args = vec!["c-trim", "--pair", s.path.as_str(), "--side", side_word(s.sd), "--epoch-table", epoch_table.as_str()];
            let amount = fmt_kas(take as i128);
            args.extend(["--take", amount.as_str()]);
            for tf in files {
                args.extend(["--signed-price", tf.as_str()]);
            }
            match sub(&args) {
                Ok(()) => {
                    done += 1;
                    adopt(known);
                    *f -= take;
                    rest -= take;
                }
                Err(e) => println!("not trimmed: {e}"),
            }
        }
        if rest > 0 {
            println!("WARNING: {} KAS of what {} needs is not covered by the wallet or by the free margin of your other positions", fmt_kas(rest as i128), x.path);
        }
    }
    Ok(done)
}

/// `c-margin-run`: the positions of one key kept linked, unattended. Every `--every-secs` (default 30, at least 10) it
/// reads the object files of `--dir` that are positions of `--key` (one query each), prices each market by the newest
/// signed price of `--signed-prices` that an epoch table of `--epoch-tables` attests (both default to `--dir`), and acts: a position within
/// `--alert-bps` (default 300) of its liquidation gets the margin that moves the line `--target-bps` (default 1,000)
/// away, from the wallet if it holds it and otherwise by trimming the positions that have margin above maintenance
/// (`c-trim`, then `c-finalize`, then `c-add-margin` on the next rounds); a pending liquidation of a position is
/// cured; a pending trim is finalized. The key signs and funds everything, new object files are written into
/// `--dir` and spent ones moved to `--dir/spent`. Without `--submit` it builds and verifies only. While it runs the
/// positions are cross-margined; when it stops each is liquidated on its own.
pub fn cross_run_cmd(a: &Args) -> R<()> {
    a.only(&["key", "dir", "signed-prices", "epoch-tables", "alert-bps", "target-bps", "every-secs", "count", "once", "node", "daa", "submit", "funding", "fee-rate"])?;
    let net = network(a)?;
    let me = keys::party(a.need("key")?, net)?;
    a.need("dir")?;
    let (alert, target) = (int_or(a, "alert-bps", 300)?, int_or(a, "target-bps", 1_000)?);
    if alert < 0 || target <= alert || target >= BPS {
        return Err("--alert-bps is at least 0 and below --target-bps, which is below 10000".into());
    }
    let every = int_or(a, "every-secs", 30)?.max(10) as u64;
    let count = if a.has("once") { 1 } else { int_or(a, "count", 0)? };
    let mut node = connect(a)?;
    let live = node.is_some();
    if !live && !a.has("daa") {
        return Err("give --node URL, or --daa N for one round without a node".into());
    }
    println!("cross margin for {}: positions in {}, a round every {every} s", p2pk_address(&me, net), a.need("dir")?);
    let (mut rounds, mut seen) = (0i64, Default::default());
    let mut known: std::collections::HashSet<String> = json_files(a.need("dir")?)?.into_iter().collect();
    loop {
        rounds += 1;
        match now(a, &mut node) {
            Ok(t) => match cross_round(a, net, &mut node, me, t as i64, alert, target, &mut seen, &mut known) {
                Ok(n) => println!("round {rounds} at DAA {t}: {n} transaction(s)"),
                Err(x) => println!("round {rounds} at DAA {t}: {x}"),
            },
            Err(x) => println!("round {rounds}: the node did not answer: {x}"),
        }
        // Without a node: one round, or `--count` rounds of the same present (a trial).
        if (count > 0 && rounds >= count) || (!live && count == 0) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(every));
    }
    Ok(())
}

/// `c-account`: the positions of `--me` as one account, at the feed's price (`--price P` for every market, or
/// `--price-of MARKET=P` by the first hex digits of a market id): what each side is worth, where it is liquidated and
/// how far that is, and the margin it has above maintenance (what `c-trim` frees at that price). A position within
/// `--alert-bps` (default 300: 3 %) of its liquidation is named with what moves the line `--target-bps` (default
/// 1,000) away, and the positions whose free margin covers it: cross margin is moved, not shared, and a position is
/// liquidated on its own. Exit code 3 when a position needs margin. One query per object file with `--node`.
pub fn account_cmd(a: &Args) -> R<()> {
    a.only(&["me", "object", "scan", "price", "price-of", "alert-bps", "target-bps", "node"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let me = keys::party(a.need("me")?, net)?;
    let all = if a.has("price") { Some(price(a, "price")?) } else { None };
    let mut by_market: Vec<(String, i64)> = vec![];
    for s in a.all("price-of") {
        let (m, p) = s.split_once('=').ok_or("--price-of takes MARKET=P (the first hex digits of a market id, and its price)")?;
        by_market.push((m.to_lowercase(), kas(p)?));
    }
    let (alert, target) = (int_or(a, "alert-bps", 300)?, int_or(a, "target-bps", 1_000)?);
    if alert < 0 || target <= alert || target >= BPS {
        return Err("--alert-bps is at least 0 and below --target-bps, which is below 10000".into());
    }
    let mut files: Vec<String> = a.all("object").iter().map(|s| s.to_string()).collect();
    for d in a.all("scan") {
        for ent in std::fs::read_dir(d).map_err(|x| format!("--scan {d}: {x}"))?.flatten() {
            let p = ent.path();
            if p.extension().is_some_and(|x| x == "json") {
                files.push(p.to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    let (mut pos, mut claims) = (Vec::<Position>::new(), 0);
    for path in &files {
        let Ok(mut f) = ObjFile::load(path) else { continue };
        let Ok(c) = CObj::decode(&f.redeem) else { continue };
        let p = match c {
            CObj::Pair(p) if p.long_key == me || p.short_key == me => p,
            CObj::Pending(w) if w.long_key == me || w.short_key == me => {
                claims += 1;
                continue;
            }
            _ => continue,
        };
        if let Some(n) = node.as_mut() {
            if let Err(x) = crate::cmds::refresh(n, &mut f, net) {
                println!("-- {path}: NOT AT THE NODE ({x}): spent by a claim, a renewal, a hand-over or a settlement; not counted");
                continue;
            }
        }
        if p.long_key == p.short_key {
            println!("-- {path}: both sides are yours ({} KAS): no position, `c-coop-close` takes it out", fmt_kas(f.amount as i128));
            continue;
        }
        let id = to_hex(&p.m.id());
        let px = match by_market.iter().find(|(m, _)| id.starts_with(m.as_str())).map(|(_, v)| *v).or(all) {
            Some(v) => v,
            None => return Err(format!("no price for the market {}: give --price P or --price-of {}=P", &id[..16], &id[..8])),
        };
        pos.push(position_of(path, p, &me, px, alert, target));
    }
    if pos.is_empty() {
        println!("no position of this key in the files given");
        return Ok(());
    }
    println!("{:<5} {:>12} {:>14} {:>14} {:>14} {:>14} {:>9} {:>14}  file", "side", "units", "price", "bound", "worth", "liquidated at", "away", "free margin");
    for x in &pos {
        println!(
            "{:<5} {:>12} {:>14} {:>14} {:>14} {:>14} {:>8.2}% {:>14}  {}",
            side_word(x.sd),
            fmt_units(x.p.size as i128, x.p.m.p.scale as i128),
            fmt_kas(x.px as i128),
            fmt_kas(if x.sd == LONG { x.p.bl } else { x.p.bs } as i128),
            fmt_kas(x.value as i128),
            fmt_kas(x.line as i128),
            x.dist_bps as f64 / 100.0,
            fmt_kas(x.free as i128),
            x.path
        );
    }
    let (equity, free): (i64, i64) = (pos.iter().map(|x| x.value).sum(), pos.iter().map(|x| x.free).sum());
    println!("account: {} position(s) worth {} KAS; {} KAS of it is margin above maintenance (`c-trim` frees it at these prices)", pos.len(), fmt_kas(equity as i128), fmt_kas(free as i128));
    if claims > 0 {
        println!("{claims} pending claim(s) on your positions are not counted: `c-watch --me` says what they need");
    }
    let needy: Vec<&Position> = pos.iter().filter(|x| x.need.is_some_and(|(d, _)| d > 0)).collect();
    if needy.is_empty() {
        println!("no position is within {:.2} % of its liquidation", alert as f64 / 100.0);
        return Ok(());
    }
    let mut left: Vec<(usize, i64)> = pos.iter().enumerate().filter(|(_, x)| x.need.is_none() && x.free > 0).map(|(i, x)| (i, x.free)).collect();
    left.sort_by_key(|(_, f)| std::cmp::Reverse(*f));
    for x in &needy {
        let (delta, cost) = x.need.unwrap();
        println!(
            "ACT: the {} of {} is {:.2} % from its liquidation ({} KAS). {} KAS move its bound by {} and the line {:.0} % away:",
            side_word(x.sd),
            x.path,
            x.dist_bps as f64 / 100.0,
            fmt_kas(x.line as i128),
            fmt_kas(cost as i128),
            fmt_kas(delta as i128),
            target as f64 / 100.0
        );
        let mut rest = cost;
        for (i, f) in left.iter_mut().filter(|(_, f)| *f > 0) {
            if rest <= 0 {
                break;
            }
            let take = rest.min(*f);
            *f -= take;
            rest -= take;
            let s = &pos[*i];
            println!("  kpf c-trim --pair {} --side {} --take {} --epoch-table EPOCH_TABLE --signed-price TICKS... --key YOU    (then `c-finalize` on the pending part it writes)", s.path, side_word(s.sd), fmt_kas(take as i128));
        }
        if rest > 0 {
            println!("  the free margin of your other positions does not cover {} KAS of it: that comes from your wallet", fmt_kas(rest as i128));
        }
        println!("  kpf c-add-margin --pair {} --side {} --delta {} --key YOU", x.path, side_word(x.sd), fmt_kas(delta as i128));
    }
    Err(format!("{ACT_NOW}: {} position(s) need margin", needy.len()))
}

/// `c-quote`: what each side of a pair is worth at given prices, and where each side is liquidated.
pub fn quote_cmd(a: &Args) -> R<()> {
    a.only(&["pair", "prices", "node"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let p = &lp.s;
    println!("{}", pair_line(p));
    println!("{:>16} {:>16} {:>16}  state", "price", "long", "short");
    let prices: Vec<i64> = match a.get("prices") {
        Some(s) => s.split(',').map(kas).collect::<R<_>>()?,
        None => (0..=8).map(|i| p.bl + (p.bs - p.bl) * i / 8).collect(),
    };
    for f in prices {
        let st = if p.long_liquidatable(f) {
            "long liquidatable"
        } else if p.short_liquidatable(f) {
            "short liquidatable"
        } else {
            ""
        };
        println!("{:>16} {:>16} {:>16}  {st}", fmt_kas(f as i128), fmt_kas(p.long_at(f) as i128), fmt_kas(p.short_at(f) as i128));
    }
    Ok(())
}

// ---------------------------------------------------------------- the cross-margin account (docs/DESIGN-COMMITTEE.md 3b)

fn xside(s: &str) -> R<i64> {
    match s.to_ascii_lowercase().as_str() {
        "a" | "owner" | "1" => Ok(SIDE_A),
        "b" | "counterparty" | "2" => Ok(SIDE_B),
        _ => Err(format!("side must be a (the account's owner) or b (its counterparty), not `{s}`")),
    }
}

fn xside_word(sd: i64) -> &'static str {
    if sd == SIDE_A {
        "the owner (a)"
    } else {
        "the counterparty (b)"
    }
}

fn leg_line(i: usize, l: &XLeg) -> String {
    format!("leg {}: {} {} units of market {} from {} KAS", i + 1, if l.dir == LONG { "long" } else { "short" }, fmt_units(l.size as i128, l.scale as i128), &to_hex(&l.market)[..16], fmt_kas(l.entry as i128))
}

/// What an account says: its legs, its margins, and with prices what each party is owed and who can be liquidated.
pub(crate) fn cross_state(x: &Cross, prices: Option<[i64; 2]>, t: Option<i64>, net: Network) {
    println!("owner (a) {}   counterparty (b) {}", p2pk_address(&x.a_key, net), p2pk_address(&x.b_key, net));
    for (i, l) in x.legs.iter().enumerate().filter(|(_, l)| l.size > 0) {
        println!("{}", leg_line(i, l));
        // Review C9 (C9-F3): nothing on chain ties a leg's market to its committee; whoever is handed an account reads
        // both whole and compares them with the market file it checked (`c-check`).
        println!("       market {}   priced by the committee {}", to_hex(&l.market), to_hex(&l.committee));
    }
    println!("margin: a {} KAS, b {} KAS (the pot {} KAS); maintenance {} % of the notional, tolerance {} %", fmt_kas(x.a_margin as i128), fmt_kas(x.b_margin as i128), fmt_kas(x.pot() as i128), x.t.mmr_bps as f64 / 100.0, x.t.g_bps as f64 / 100.0);
    // Review C15 (C15-F1): every term, for an account carries its own.
    println!(
        "terms: a claim lands within {} DAA of its time ({}); a liquidation is cured within {} DAA ({}); its fee {} % of the notional, at least {} KAS; a lease of {} DAA, settled at the entries {} DAA after it; {} DAA of quiet before a party's entry; carrier {} KAS",
        x.t.late_daa,
        secs(x.t.late_daa),
        x.t.grace_daa,
        secs(x.t.grace_daa),
        x.t.liq_fee_bps as f64 / 100.0,
        fmt_kas(x.t.keeper_fee_min as i128),
        x.t.term,
        x.t.idle_daa,
        x.t.quiet_daa,
        fmt_kas(x.t.carrier as i128)
    );
    if let Some(p) = prices {
        let state = if x.liquidatable(SIDE_A, p) {
            "the OWNER (a) can be LIQUIDATED at these prices"
        } else if x.liquidatable(SIDE_B, p) {
            "the COUNTERPARTY (b) can be LIQUIDATED at these prices"
        } else {
            "neither party can be liquidated at these prices"
        };
        let v = x.view(p, None);
        let legs: Vec<String> = x.legs.iter().enumerate().filter(|(_, l)| l.size > 0).map(|(i, l)| format!("leg {} {} KAS", i + 1, fmt_kas(l.gain(x.counted(p)[i]) as i128))).collect();
        println!("at {} / {} KAS: a is owed {} KAS ({}), b {} KAS; the notional is {} KAS and its maintenance {} KAS; {state}", fmt_kas(p[0] as i128), fmt_kas(p[1] as i128), fmt_kas(x.a_at(p) as i128), legs.join(", "), fmt_kas(x.owed_at(SIDE_B, p) as i128), fmt_kas(v.notional as i128), fmt_kas(x.maint(v.notional) as i128));
        for sd in [SIDE_A, SIDE_B] {
            match x.close_pay(sd, p) {
                Some(v) => println!("  a close by {} pays it {} KAS (each price moved by the tolerance against it)", xside_word(sd), fmt_kas(v as i128)),
                None => println!("  {} cannot close at these prices: it would be below the maintenance", xside_word(sd)),
            }
        }
    }
    match t {
        Some(t) if t >= x.lapse_from() as i64 => println!("LAPSED: anyone can settle this account at its entries now (`x-lapse-settle`): a its margin, b the rest"),
        Some(t) if t >= x.expiry => println!("the lease ENDED {} DAA ago: in {} DAA ({}) anyone settles this account at its entries; a signed price renews it (`x-keep-alive`)", t - x.expiry, x.lapse_from() as i64 - t, secs(x.lapse_from() as i64 - t)),
        Some(t) => println!("lease ends in {} DAA ({}); a signed price of either leg's market from DAA {} on renews it (`x-keep-alive`)", x.expiry - t, secs(x.expiry - t), x.keep_alive_from()),
        None => println!("lease until DAA {}; lapse settle from DAA {}; a signed price from DAA {} on renews it", x.expiry, x.lapse_from(), x.keep_alive_from()),
    }
}

fn xpending_state(w: &XPending, landed: Option<u64>, value: Option<u64>) {
    let (kind, who) = (if w.kind == KIND_CLOSE { "close by" } else { "liquidation of" }, xside_word(w.side));
    println!("a {kind} {who} for the time {}; proposer {}", w.t0, to_hex(&w.proposer));
    if w.kind == KIND_CLOSE {
        println!("the closer is paid {} KAS, the other party the rest", fmt_kas(w.amount as i128));
    } else {
        println!("the proposer's fee is {} KAS and the other party takes the rest; the loser cures by adding {} KAS (`x-cure`)", fmt_kas(w.amount as i128), fmt_kas(w.cure_add as i128));
    }
    if !w.consistent(kpf_protocol::committee_artifacts::xkit()) {
        println!("WARNING: the account this claim carries is not the one its hashes bind: no account wrote it");
    }
    match (landed, value) {
        (Some(p), _) if w.valid_at(p as i64) => println!("VALID: landed at DAA {p}, {} after its time; finalize from DAA {} (`x-finalize`)", p as i64 - w.t0, p + w.finalize_age(p as i64)),
        (Some(p), _) => println!("VOID: landed at DAA {p}, {} after its time (more than lateDaa {}): `x-finalize` restores the account", p as i64 - w.t0, w.late_daa),
        (None, _) => println!("valid only if it landed by DAA {} (its DAA is unknown: give --node)", w.last_valid_landing()),
    }
}

/// The epoch table and the attestation of leg `n` (1 or 2) of an account: `--epoch-tableN T.json --tickN FILE...`.
pub(crate) fn x_att(a: &Args, n: usize, x: &Cross, node: &mut N, net: Network) -> R<(OLive<EpochTable>, Attestation)> {
    let l = &x.legs[n - 1];
    let epoch_table = epoch_table_of(a, &format!("epoch-table{n}"), node, net)?;
    let files = a.all(&format!("signed-price{n}"));
    if files.is_empty() {
        return Err(format!("give the members' signed prices of leg {n}: --signed-price{n} FILE (once a member)"));
    }
    let signed_prices: Vec<SignedPrice> = files.iter().map(|f| price_from(f, &l.committee, &l.market)).collect::<R<_>>()?;
    let t0 = int_or(a, "t0", signed_prices[0].t0)?;
    let rid = epoch_table.id.map(|h| h.as_bytes()).unwrap_or([0; 32]);
    if rid != l.committee {
        return Err(format!("REFUSED: --epoch-table{n} is not of leg {n}'s committee"));
    }
    let att = Attestation::collect(&epoch_table.s, &rid, &l.market, t0, &signed_prices).map_err(|e| format!("leg {n}: no price: {e} (the epoch table serves signed prices [{}, {}))", epoch_table.s.from, epoch_table.s.until))?;
    println!("attested (leg {n}): {} KAS at DAA {t0}: {} members signed within {} % of it", fmt_kas(att.p as i128), att.bits.count_ones(), epoch_table.s.spread_bps as f64 / 100.0);
    Ok((epoch_table, att))
}

/// The epoch tables and attestations of every leg of an account, for one time.
fn x_atts(a: &Args, x: &Cross, node: &mut N, net: Network) -> R<Vec<(OLive<EpochTable>, Attestation)>> {
    let mut v = vec![x_att(a, 1, x, node, net)?];
    if x.legs[1].size > 0 {
        v.push(x_att(a, 2, x, node, net)?);
        if v[1].1.t0 != v[0].1.t0 {
            return Err(format!("REFUSED: leg 1 is attested for the time {} and leg 2 for {}: an account is priced for one time (the members of both markets sign the same DAA: `member-run` puts them on one grid)", v[0].1.t0, v[1].1.t0));
        }
    }
    price_in_time(a, node, v[0].1.t0, x.t.late_daa)?;
    Ok(v)
}

/// The terms of an account from the markets of its legs: of each parameter the more careful of the two. The
/// tolerance is the larger g (review C9, C9-F3: each market's g is at least four times its committee's spread, and an
/// account whose g is below that for a leg accepts no claim), the maintenance the larger, the times a claim has the
/// shorter.
pub(crate) fn terms_of(m: &CMarket, m2: Option<&CMarket>) -> XTerms {
    let (p, q) = (&m.p, &m2.unwrap_or(m).p);
    XTerms {
        term: p.term.min(q.term),
        mmr_bps: p.mmr_bps.max(q.mmr_bps),
        g_bps: p.g_bps.max(q.g_bps),
        liq_fee_bps: p.liq_fee_bps.max(q.liq_fee_bps),
        keeper_fee_min: p.keeper_fee_min.max(q.keeper_fee_min),
        carrier: p.carrier.max(q.carrier),
        idle_daa: p.idle_daa.max(q.idle_daa),
        grace_daa: p.grace_daa.max(q.grace_daa),
        late_daa: p.late_daa.min(q.late_daa),
        quiet_daa: p.quiet_daa.max(q.quiet_daa),
    }
}

pub(crate) fn leg_of(m: &CMarket, size: i64, dir: i64, entry: i64) -> XLeg {
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size, dir, entry }
}

/// `x-open`: a new cross-margin account between `--a` (its owner) and `--b` (its counterparty), with one or two legs
/// in committee markets, funded by the funder. The terms are market 1's (maintenance, tolerance, fee, times).
pub fn x_open_cmd(a: &Args) -> R<()> {
    only(a, &["market1", "size1", "dir1", "x1", "market2", "size2", "dir2", "x2", "a", "b", "a-margin", "b-margin"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)? as i64;
    let x = cross_from_args(a, net, t)?;
    let b = x_open(&x, &fu, &fk, rate(a)?).map_err(e)?;
    cross_state(&x, Some([x.legs[0].entry, x.legs[1].entry]), Some(t), net);
    finish(a, "x-open: a cross-margin account", &b, &keys, net, &mut node)
}

/// (the terms, the market's id, its committee's id, its scale and its price limit) of a committee market's file: what an
/// order of the thin family takes its template from.
pub(crate) fn market_parts(path: &str) -> R<(XTerms, [u8; 32], [u8; 32], i64, i64)> {
    let m = cmarket_from(path)?.0;
    let l = leg_of(&m, 0, LONG, 0);
    Ok((terms_of(&m, None), l.market, l.committee, l.scale, l.price_max))
}

/// The account the flags of `x-open` describe (legs of one or two committee markets, the two parties, their margins),
/// its lease running from `t`. The thin family's `acct-open` takes the same flags.
pub(crate) fn cross_from_args(a: &Args, net: Network, t: i64) -> R<Cross> {
    let m1 = cmarket_from(a.need("market1")?)?.0;
    let mut legs = [leg_of(&m1, parse_amount(a.need("size1")?, m1.p.scale)?, side(a.need("dir1")?)?, price(a, "x1")?), XLeg::none()];
    let mut m2 = None;
    if a.has("market2") {
        let m = cmarket_from(a.need("market2")?)?.0;
        legs[1] = leg_of(&m, parse_amount(a.need("size2")?, m.p.scale)?, side(a.need("dir2")?)?, price(a, "x2")?);
        m2 = Some(m);
    }
    let x = Cross { t: terms_of(&m1, m2.as_ref()), legs, a_key: keys::party(a.need("a")?, net)?, b_key: keys::party(a.need("b")?, net)?, a_margin: price(a, "a-margin")?, b_margin: price(a, "b-margin")?, expiry: t + m1.p.term };
    Ok(x)
}

/// `x-swap`: the owner's side of one or two pairs becomes one cross-margin account against the funder (the executor),
/// in one transaction: the executor takes each pair over as it is (the other party of each pair keeps what it had),
/// and funds the account in which the owner holds both positions and one margin. `--x1`/`--x2` are the prices the two
/// agree (the account's entries); `--a-margin` what the owner is owed there (default: what its sides are worth at
/// those prices), `--b-margin` what the executor posts. The owner's key (`--key`) signs the pairs, the funder's the
/// funding: both key files on this machine.
pub fn x_swap_cmd(a: &Args) -> R<()> {
    only(a, &["pair1", "side1", "x1", "nb1", "pair2", "side2", "x2", "nb2", "a-margin", "b-margin"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let p1 = pair_of(a, "pair1", &mut node, net)?;
    let s1 = side(a.need("side1")?)?;
    let x1 = price(a, "x1")?;
    let mut pairs = vec![(p1.clone(), s1, price_or(a, "nb1", swap_bound(&p1.s, s1))?)];
    let mut legs = [leg_of(&p1.s.m, p1.s.size, s1, x1), XLeg::none()];
    let mut worth = side_worth(&p1.s, s1, x1);
    let mut m2 = None;
    if a.has("pair2") {
        let p2 = pair_of(a, "pair2", &mut node, net)?;
        m2 = Some(p2.s.m);
        let (s2, x2) = (side(a.need("side2")?)?, price(a, "x2")?);
        legs[1] = leg_of(&p2.s.m, p2.s.size, s2, x2);
        worth += side_worth(&p2.s, s2, x2);
        pairs.push((p2.clone(), s2, price_or(a, "nb2", swap_bound(&p2.s, s2))?));
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)? as i64;
    let a_key = if s1 == LONG { p1.s.long_key } else { p1.s.short_key };
    let a_margin = price_or(a, "a-margin", worth)?;
    let x = Cross { t: terms_of(&p1.s.m, m2.as_ref()), legs, a_key, b_key: fk, a_margin, b_margin: price(a, "b-margin")?, expiry: t + p1.s.m.p.term };
    let refs: Vec<(&OLive<CPair>, i64, i64)> = pairs.iter().map(|(p, s, nb)| (p, *s, *nb)).collect();
    let (b, parts) = x_swap(&refs, &x, &fu, &fk, rate(a)?).map_err(e)?;
    println!("the owner gives up position(s) worth {} KAS at the agreed prices and is owed {} KAS in the account ({} KAS is the counterparty's price for the swap)", fmt_kas(worth as i128), fmt_kas(a_margin as i128), fmt_kas((worth - a_margin) as i128));
    for p in &parts {
        println!("the counterparty holds: {}", pair_line(p));
    }
    cross_state(&x, Some([x.legs[0].entry, x.legs[1].entry]), Some(t), net);
    finish(a, "x-swap: positions become one cross-margin account", &b, &keys, net, &mut node)
}

/// `x-show`: an account at given prices: what each party is owed, the maintenance, who can be liquidated, what a
/// close pays.
pub fn x_show_cmd(a: &Args) -> R<()> {
    a.only(&["account", "price1", "price2", "node", "daa"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = cross_of(a, "account", &mut node, net)?;
    let t = if a.has("daa") || node.is_some() { Some(now(a, &mut node)? as i64) } else { None };
    let p = if a.has("price1") { Some([price(a, "price1")?, price_or(a, "price2", 0)?]) } else { None };
    if p.is_some() && x.s.legs[1].size > 0 && !a.has("price2") {
        return Err("the account has two legs: give --price1 and --price2".into());
    }
    cross_state(&x.s, p, t, net);
    println!("the account holds {} KAS", fmt_kas(x.value as i128));
    Ok(())
}

fn x_claim(a: &Args, kind: i64, title: &str) -> R<()> {
    only(a, &["account", "side", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2", "t0", "proposer"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = cross_of(a, "account", &mut node, net)?;
    let sd = xside(a.need("side")?)?;
    let atts = x_atts(a, &x.s, &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let proposer = match a.get("proposer") {
        Some(p) => keys::party(p, net)?,
        None => fk,
    };
    let refs: Vec<(&OLive<EpochTable>, &Attestation)> = atts.iter().map(|(t, at)| (t, at)).collect();
    let (b, w) = x_propose(&x, &refs, kind, sd, proposer, &fu, &fk, rate(a)?).map_err(e)?;
    if kind == KIND_CLOSE {
        println!("close: {} is paid {} KAS, the other party the rest of the account ({} KAS); settles at once with `x-finalize`", xside_word(sd), fmt_kas(w.amount as i128), fmt_kas((x.value as i64 - w.amount) as i128));
    } else {
        println!("liquidation of {}: the fee {} KAS goes to {}; the loser can cure for {} DAA ({}) by adding {} KAS (`x-cure`), then `x-finalize`", xside_word(sd), fmt_kas(w.amount as i128), p2pk_address(&w.proposer, net), w.grace_daa, secs(w.grace_daa), fmt_kas(w.cure_add as i128));
    }
    finish(a, title, &b, &keys, net, &mut node)
}

/// `x-close`: a party closes the account at the attested prices of its legs (its key signs).
pub fn x_close_cmd(a: &Args) -> R<()> {
    x_claim(a, KIND_CLOSE, "x-close: an account closed at the attested prices")
}

/// `x-liquidate`: anyone liquidates the side of an account the attested prices put below the maintenance.
pub fn x_liquidate_cmd(a: &Args) -> R<()> {
    x_claim(a, KIND_LIQ, "x-liquidate: an account liquidated at the attested prices")
}

/// `x-finalize`: settle a pending claim on an account, or restore a void one.
pub fn x_finalize_cmd(a: &Args) -> R<()> {
    only(a, &["pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = xpending_of(a, "pending", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, s) = x_finalize(&w, &fu, &fk, rate(a)?).map_err(e)?;
    let after = w.daa as i64 - w.s.t0;
    match &s {
        XSettlement::Close { a: pa, b: pb } => println!("valid (landed {after} DAA after its time): the owner {} KAS, the counterparty {} KAS", fmt_kas(*pa as i128), fmt_kas(*pb as i128)),
        XSettlement::Forfeit { fee, winner, rest } => println!("valid: {} KAS to the winner {}, the proposer's fee {} KAS", fmt_kas(*rest as i128), p2pk_address(winner, net), fmt_kas(*fee as i128)),
        XSettlement::Restore { .. } => println!("VOID: the claim landed {after} DAA after its time (more than lateDaa {}): the account is restored", w.s.late_daa),
    }
    finish(a, "x-finalize", &b, &keys, net, &mut node)
}

/// `x-cure`: the loser of a pending liquidation of an account adds the margin it needs.
pub fn x_cure_cmd(a: &Args) -> R<()> {
    only(a, &["pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = xpending_of(a, "pending", &mut node, net)?;
    if !w.s.valid_at(w.daa as i64) {
        return Err(format!("REFUSED: this claim is void (it landed {} DAA after its time, more than lateDaa {}): do not add margin; `x-finalize` restores the account as it was", w.daa as i64 - w.s.t0, w.s.late_daa));
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, c) = x_cure(&w, &fu, &fk, rate(a)?).map_err(e)?;
    println!("cured: {} KAS added; the margins are now a {} KAS, b {} KAS", fmt_kas(w.s.cure_add as i128), fmt_kas(c.a_margin as i128), fmt_kas(c.b_margin as i128));
    finish(a, "x-cure", &b, &keys, net, &mut node)
}

/// `x-add-margin`: a party adds `--amount` KAS to its margin of an account.
pub fn x_add_margin_cmd(a: &Args) -> R<()> {
    only(a, &["account", "side", "amount"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = cross_of(a, "account", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, c) = x_add_margin(&x, xside(a.need("side")?)?, price(a, "amount")?, &fu, &fk, rate(a)?).map_err(e)?;
    println!("margin: a {} KAS, b {} KAS", fmt_kas(c.a_margin as i128), fmt_kas(c.b_margin as i128));
    finish(a, "x-add-margin", &b, &keys, net, &mut node)
}

/// `x-keep-alive`: renew an account's lease with an attested signed price of one of its legs' markets.
pub fn x_keep_alive_cmd(a: &Args) -> R<()> {
    only(a, &["account", "leg", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2", "t0"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = cross_of(a, "account", &mut node, net)?;
    let leg = int_or(a, "leg", 1)? as usize;
    if !(1..=2).contains(&leg) || x.s.legs[leg - 1].size == 0 {
        return Err("--leg is 1 or 2, a leg the account has".into());
    }
    let (epoch_table, att) = x_att(a, leg, &x.s, &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, c) = x_keep_alive(&x, leg, &epoch_table, &att, &fu, &fk, rate(a)?).map_err(e)?;
    println!("lease until DAA {} (was {})", c.expiry, x.s.expiry);
    finish(a, "x-keep-alive", &b, &keys, net, &mut node)
}

/// `x-lapse-settle`: settle an account nobody kept alive at its entries.
pub fn x_lapse_cmd(a: &Args) -> R<()> {
    only(a, &["account"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = cross_of(a, "account", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "x-lapse-settle", &x_lapse_settle(&x, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `x-coop-close`: both parties end an account: the owner is paid `--a-pay`, the counterparty the rest less the fee.
pub fn x_coop_close_cmd(a: &Args) -> R<()> {
    a.only(&["account", "a-pay", "key", "node", "out-dir", "no-save", "submit", "fee-rate", "daa"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let x = cross_of(a, "account", &mut node, net)?;
    let keys = crate::cmds::funder_and_keys(a, false)?.map(|f| f.keys).ok_or("give both parties' keys: --key A --key B")?;
    let b = x_coop_close(&x, price(a, "a-pay")? as u64, rate(a)?).map_err(e)?;
    finish(a, "x-coop-close", &b, &keys, net, &mut node)
}
