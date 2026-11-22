//! Review C14 of the committee family (docs/audit/REVIEW-C14.md): `cosign` after the fixes of review C13: the honest
//! two-machine flows, and what is left to the co-signer's reading. The harness is that of committee_cli.rs.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

struct H {
    dir: PathBuf,
    n: usize,
    last: String,
}

impl Drop for H {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kpf"))
}

impl H {
    fn new(tag: &str) -> H {
        let dir = std::env::temp_dir().join(format!("kpf-committee-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        H { dir, n: 0, last: String::new() }
    }
    fn p(&self, name: &str) -> String {
        self.dir.join(name).display().to_string()
    }
    fn run(&self, args: &[&str]) -> (bool, String, String) {
        let o = bin().args(args).output().expect("run kpf");
        (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
    }
    fn ok(&self, args: &[&str]) -> String {
        let (ok, out, err) = self.run(args);
        assert!(ok, "kpf {args:?} failed:
{err}
{out}");
        // `KPF_COST_LOG=FILE`: the size and the masses of every transaction the tests build, by command.
        if let Ok(f) = std::env::var("KPF_COST_LOG") {
            if let Some(l) = out.lines().find(|l| l.contains("compute mass")) {
                use std::io::Write as _;
                let mut w = std::fs::OpenOptions::new().create(true).append(true).open(f).unwrap();
                writeln!(w, "{}	{}", args[0], l).unwrap();
            }
        }
        out
    }
    fn fail(&self, args: &[&str]) -> String {
        let (ok, out, err) = self.run(args);
        assert!(!ok, "kpf {args:?} should have failed:\n{out}");
        err
    }
    fn key(&self, name: &str) -> String {
        let path = self.p(&format!("{name}.key.json"));
        self.ok(&["keygen", "--out", &path]);
        path
    }
    /// A funding file of one synthetic UTXO of `kas` KAS.
    fn funding(&mut self, kas: u64) -> String {
        self.n += 1;
        let txid = format!("{:064x}", 0x5eed_0000u64 + self.n as u64);
        let path = self.p(&format!("funding-{}.json", self.n));
        std::fs::write(&path, serde_json::to_string(&json!([{ "txid": txid, "index": 0, "amount": kas * 100_000_000, "daa": 0 }])).unwrap()).unwrap();
        path
    }
    /// Runs a builder (funded by `funder` with a fresh funding file) into a fresh out-dir; returns the out-dir of its
    /// dry run.
    fn build(&mut self, funder: &str, args: &[&str]) -> String {
        let fund = self.funding(100_000);
        self.n += 1;
        let out = self.p(&format!("out{}", self.n));
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--funder", funder, "--funding", &fund, "--out-dir", &out]);
        let stdout = self.ok(&a);
        assert!(stdout.contains("not submitted"), "{stdout}");
        self.last = stdout;
        format!("{out}/dry-run")
    }
    /// The object file of `dir` whose name ends with `kind`, its DAA set to `daa`.
    fn obj(&self, dir: &str, kind: &str, daa: u64) -> String {
        let want = format!("-{}.json", kind.replace(' ', "-"));
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(&want)).collect();
        all.sort();
        let f = all.first().unwrap_or_else(|| panic!("no {kind} in {dir}")).clone();
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        v["daa"] = json!(daa);
        std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        f.display().to_string()
    }
    fn step(&mut self, funder: &str, args: &[&str], kind: &str, daa: u64) -> String {
        let d = self.build(funder, args);
        self.obj(&d, kind, daa)
    }
}

const FEED: &str = "kpf feed definition v1\nquantity: example underlying\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\ndisagreement: a signed price is true if within 0.5 % of the median\n";

/// A committee with three members (30, 22 and 15 KAS: 67 KAS, each needed for a quorum of 67 %), rolled into the epoch
/// [33,000, 36,000), and a market priced by it. Returns (funder, member keys, committee object, epoch table, market file).
fn setup(h: &mut H) -> (String, Vec<String>, String, String, String) {
    let funder = h.key("funder");
    let members: Vec<String> = (0..3).map(|i| h.key(&format!("member{i}"))).collect();
    let committee_file = h.p("committee.json");
    let d = h.build(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file]);
    let mut committee = h.obj(&d, "committee", 30_000);
    assert!(h.last.contains("committee id"), "{}", h.last);
    let out = h.ok(&["committee-check", "--committee", &committee_file, "--object", &committee]);
    assert!(out.contains("OK: the id is that of one empty committee"), "{out}");
    for (i, kas) in [(0, "30"), (1, "22"), (2, "15")] {
        committee = h.step(&funder, &["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", &i.to_string(), "--amount", kas, "--owner", &members[i]], "committee", 30_100 + i as u64);
    }
    // Review C2: who puts KAS into a committee shows that it began empty, or says that it did not check.
    let fund0 = h.funding(1_000);
    assert!(h.fail(&["committee-join", "--committee", &committee, "--member", "3", "--amount", "10", "--funder", &funder, "--funding", &fund0]).contains("give --genesis"));
    let other = h.p("other-committee.json");
    h.build(&funder, &["committee-create", "--daa", "30000", "--epoch-daa", "6000", "--out", &other]);
    assert!(h.fail(&["committee-join", "--committee", &committee, "--genesis", &other, "--member", "3", "--amount", "10", "--funder", &funder, "--funding", &fund0]).contains("not the committee of these genesis facts"));
    // Below minMemberStake, and a taken member while others are empty.
    let fund = h.funding(1_000);
    assert!(h.fail(&["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", "3", "--amount", "9", "--funder", &funder, "--funding", &fund]).contains("at least 10 KAS"));
    assert!(h.fail(&["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", "0", "--amount", "100", "--funder", &funder, "--funding", &fund]).contains("cannot be taken now"));
    // Before the epoch ends there is nothing to roll.
    assert!(h.fail(&["committee-roll", "--committee", &committee, "--daa", "32999", "--funder", &funder, "--funding", &fund]).contains("has not ended"));
    let d = h.build(&funder, &["committee-roll", "--committee", &committee, "--daa", "33000"]);
    assert!(h.last.contains("epoch [33000, 36000) with 4 epoch tables"), "{}", h.last);
    let epoch_table = h.obj(&d, "epoch table", 33_000);
    let committee = h.obj(&d, "committee", 33_000);
    let show = h.ok(&["committee-show", "--committee", &committee, "--daa", "33100"]);
    assert!(show.contains("total stake 67 KAS") && show.contains("member  0  30 KAS"), "{show}");
    let feed = h.p("feed.txt");
    std::fs::write(&feed, FEED).unwrap();
    let market = h.p("market.json");
    let out = h.ok(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]);
    assert!(out.contains("market id") && out.contains("no party states a price"), "{out}");
    let out = h.ok(&["c-check", "--market", &market, "--feed", &feed]);
    assert!(out.contains("OK: the feed definition is the market's") && out.contains("OK: the committee id is that of one empty committee"), "{out}");
    assert!(!out.contains("WARNING"), "{out}");
    // Review C2 (C2-F1): a market whose g leaves the committee's spread no margin is refused (the committee's spread is
    // 0.1 %, so g must be at least 0.4 %).
    assert!(h.fail(&["c-market", "--committee", &committee_file, "--feed", &feed, "--g-bps", "39", "--out", &h.p("tight.json")]).contains("g must be at least 4 times the spread"));
    let out = h.ok(&["c-market", "--committee", &committee_file, "--feed", &feed, "--g-bps", "40", "--mmr-bps", "30", "--out", &h.p("edge.json")]);
    assert!(out.contains("WARNING: the maintenance margin"), "{out}");
    (funder, members, committee, epoch_table, market)
}

/// The three members sign `price` for the time `t0`; returns the `--signed-price FILE` arguments.
fn signed_prices(h: &H, market: &str, members: &[String], t0: u64, price: &str) -> Vec<String> {
    let mut v = vec![];
    for (i, k) in members.iter().enumerate() {
        let f = h.p(&format!("price-{t0}-{i}.json"));
        h.ok(&["price-sign", "--market", market, "--t0", &t0.to_string(), "--price", price, "--key", k, "--out", &f]);
        v.push("--signed-price".to_string());
        v.push(f);
    }
    v
}

fn with<'a>(base: &[&'a str], extra: &'a [String]) -> Vec<&'a str> {
    let mut v = base.to_vec();
    v.extend(extra.iter().map(String::as_str));
    v
}

/// Two orders of 10 units at 10 KAS (the long's rate 1, the short's 2), opened at DAA 33,400.
fn open_pair(h: &mut H, funder: &str, market: &str, long: &str, short: &str) -> String {
    let lo = h.step(funder, &["c-order", "--market", market, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "9000000", "--owner", long], "committee order", 33_100);
    let so = h.step(funder, &["c-order", "--market", market, "--side", "short", "--size", "10", "--limit", "10", "--rate", "2", "--expiry", "9000000", "--owner", short], "committee order", 33_100);
    let p = h.step(funder, &["c-open", "--long", &lo, "--short", &so, "--n", "10", "--x", "10", "--daa", "33400"], "committee pair", 33_400);
    assert!(h.last.contains("pair [9 .. 12] KAS of 10 units at 10 KAS") && h.last.contains("lease until DAA 6081400"), "{}", h.last);
    p
}

/// Builds `cmd` as a partly signed file funded by the executor's address and has the executor read and sign it:
/// (accepted, what `cosign` printed).
fn flow(h: &mut H, exec: &str, exec_addr: &str, name: &str, cmd: &[&str], keys: &[&str]) -> (bool, String) {
    let offer = h.funding(1_000);
    let f = h.p(&format!("{name}.json"));
    let mut args: Vec<&str> = cmd.to_vec();
    args.extend(["--funder", exec_addr, "--funding", &offer, "--partial", &f]);
    for k in keys {
        args.extend(["--key", k]);
    }
    let (ok, out, err) = h.run(&args);
    assert!(ok, "{name}: the builder: {out}{err}");
    let (ok, out, err) = h.run(&["cosign", "--tx", &f, "--key", exec, "--out-dir", &h.p(&format!("{name}-signed"))]);
    (ok, format!("{out}{err}"))
}

/// The honest two-machine flows after the refusals of review C13: none is refused, and each is told what it is.
#[test]
fn c14_holds_the_honest_partial_flows_are_read_and_signed() {
    let mut h = H::new("c14");
    let (exec, _members, _committee, _epoch_table1, market1) = setup(&mut h);
    let (me, other) = (h.key("me"), h.key("other"));
    let exec_addr = h.ok(&["address", "--key", &exec]).split_whitespace().find(|w| w.contains(':')).expect("an address").to_string();
    let a = open_pair(&mut h, &exec, &market1, &me, &other);
    let b = open_pair(&mut h, &exec, &market1, &other, &me);
    let (committee_file, committee2) = (h.p("committee.json"), h.p("committee2.json"));
    // The swap: two pairs written by the pairs' own inputs and an account written by the funder; a part in each.
    let (ok, out) = flow(&mut h, &exec, &exec_addr, "swap", &["x-swap", "--pair1", &a, "--side1", "long", "--x1", "11", "--pair2", &b, "--side2", "short", "--x2", "10.5", "--b-margin", "20", "--daa", "33500"], &[&me]);
    assert!(ok && !out.contains("REFUSED") && out.matches("your part of it at its entries:").count() == 3 && out.contains("20 KAS of the 55.2 KAS it holds"), "{out}");
    // An assign: the entrant's part of the pair, and the leaver's payout as what it is: money to another address.
    let (ok, out) = flow(&mut h, &exec, &exec_addr, "assign", &["c-assign", "--pair", &a, "--side", "long", "--x", "10.4"], &[&me]);
    assert!(ok && out.contains("your part of it at its entries: 10 KAS of the 30.2 KAS it holds") && out.contains("WARNING: 14 KAS go to addresses or scripts that are neither yours"), "{out}");
    let (ok, out) = flow(&mut h, &exec, &exec_addr, "order", &["c-order", "--market", &market1, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "9000000", "--owner", &exec_addr], &[]);
    assert!(ok && out.contains("long 10 units, limit 10 KAS, margin rate 1 KAS") && out.contains("yours in it") && !out.contains("WARNING"), "{out}");
    let (ok, out) = flow(&mut h, &exec, &exec_addr, "stake", &["judge-stake", "--committee", &committee_file, "--value", "100", "--owner", &exec_addr], &[]);
    assert!(ok && out.contains("in an object (no covenant id: its owner's key spends it)") && !out.contains("WARNING"), "{out}");
    let (ok, out) = flow(&mut h, &exec, &exec_addr, "committee", &["committee-create", "--daa", "30000", "--out", &committee2], &[]);
    assert!(ok && out.contains("NONE OF THIS OBJECT'S KEYS IS YOURS") && out.contains("WARNING: 1 KAS go into objects none of whose keys is yours"), "{out}");
}

/// C14-F1. What `cosign` prints of an order (and of a pair) has no market in it: not its id, not the committee that
/// prices it. An order for the co-signer's key in a market of the writer's own committee is printed, line for line, as
/// the same order in the market the co-signer knows; only the object's address and covenant id differ. The writer
/// fills it and attests what it likes.
#[test]
fn c14_fixed_an_order_is_printed_with_its_market_and_refused_in_another() {
    let mut h = H::new("c14m");
    let (exec, _members, _committee, _epoch_table1, market1) = setup(&mut h);
    let exec_addr = h.ok(&["address", "--key", &exec]).split_whitespace().find(|w| w.contains(':')).expect("an address").to_string();
    // The writer's own committee and a market it prices.
    let writer = h.key("writer");
    let own_committee = h.p("own-committee.json");
    h.step(&writer, &["committee-create", "--daa", "30000", "--out", &own_committee], "committee", 30_000);
    let feed = h.p("own-feed.txt");
    std::fs::write(&feed, FEED).unwrap();
    let own_market = h.p("own-market.json");
    h.ok(&["c-market", "--committee", &own_committee, "--feed", &feed, "--out", &own_market]);
    let id = |f: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(f).unwrap()).unwrap()["id"].as_str().map(|s| s.to_string()).unwrap_or_default();
    let order = |h: &mut H, name: &str, market: &str| flow(h, &exec, &exec_addr, name, &["c-order", "--market", market, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "9000000", "--owner", &exec_addr], &[]);
    let (ok1, known) = order(&mut h, "known", &market1);
    let (ok2, hostile) = order(&mut h, "hostile", &own_market);
    assert!(ok1 && ok2, "{known}\n{hostile}");
    // (As fixed after this round: each printout names its market and the committee that prices it, whole; and with the
    // market file the co-signer checked, an order of any other market is refused.)
    let (m1, m2) = (id(&market1), id(&own_market));
    assert!(m1.len() == 64 && m2.len() == 64 && m1 != m2, "{m1} {m2}");
    assert!(known.contains(&format!("market {m1}   priced by the committee ")) && hostile.contains(&format!("market {m2}   priced by the committee ")), "{known}\n{hostile}");
    assert!(known.contains("CHECK: priced in the market") && hostile.contains("CHECK: priced in the market"), "{hostile}");
    let again = |h: &H, file: &str| h.run(&["cosign", "--tx", file, "--key", &exec, "--market", &market1, "--out-dir", &h.p("again")]);
    let (ok, out, err) = again(&h, &h.p("known.json"));
    assert!(ok && !out.contains("CHECK:"), "{out}{err}");
    let (ok, out, err) = again(&h, &h.p("hostile.json"));
    assert!(!ok && err.contains("REFUSED") && err.contains("none of the markets given with --market") && err.contains(&m2), "{out}{err}");
}
