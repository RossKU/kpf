//! Review C9 of the committee family (docs/audit/REVIEW-C9.md): the cross-margin account through the binary, offline.
//! The harness (`H`, `setup`, `signed prices`, `with`, `open_pair`) is that of committee_cli.rs.

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
        assert!(ok, "kpf {args:?} failed:\n{err}\n{out}");
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

fn masses(text: &str) -> Vec<String> {
    text.lines().filter(|l| l.contains("compute mass")).map(|l| l.trim().to_string()).collect()
}

/// The transactions of an account through the binary: their masses, and what the commands print of the account they
/// make. (The flow is that of committee_cli.rs; the numbers asserted are the masses' bounds.)
#[test]
fn c9_bounded_the_masses_of_the_swap_the_claims_and_their_settlements() {
    let mut h = H::new("c9x");
    let (exec, members, _committee, epoch_table1, market1) = setup(&mut h);
    let (me, other) = (h.key("me"), h.key("other"));
    let feed2 = h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = h.p("market2.json");
    h.ok(&["c-market", "--committee", &h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    let a = open_pair(&mut h, &exec, &market1, &me, &other);
    let b = open_pair(&mut h, &exec, &market2, &other, &me);
    let tdir = std::path::Path::new(&epoch_table1).parent().unwrap();
    let mut all: Vec<PathBuf> = std::fs::read_dir(tdir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with("-epoch-table.json")).collect();
    all.sort();
    let epoch_table2 = all.iter().map(|p| p.display().to_string()).find(|p| *p != epoch_table1).expect("a second epoch table");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&epoch_table2).unwrap()).unwrap();
    v["daa"] = json!(33_000);
    std::fs::write(&epoch_table2, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let sign = |h: &H, t0: u64, p1: &str, p2: &str, t2: u64| -> Vec<String> {
        let mut v = vec!["--epoch-table1".to_string(), epoch_table1.clone(), "--epoch-table2".to_string(), epoch_table2.clone()];
        for (n, (market, price, t)) in [(&market1, p1, t0), (&market2, p2, t2)].iter().enumerate() {
            for (i, k) in members.iter().enumerate() {
                let f = h.p(&format!("m{}-price-{t}-{i}-{price}.json", n + 1));
                h.ok(&["price-sign", "--market", market, "--t0", &t.to_string(), "--price", price, "--key", k, "--out", &f, "--second-price"]);
                v.push(format!("--signed-price{}", n + 1));
                v.push(f);
            }
        }
        v
    };
    let mut report = vec![];
    let x = h.step(&exec, &["x-swap", "--pair1", &a, "--side1", "long", "--x1", "11", "--pair2", &b, "--side2", "short", "--x2", "10.5", "--b-margin", "20", "--key", &me, "--daa", "33500"], "cross account", 33_500);
    report.push(("x-swap", masses(&h.last)));
    // What the owner gives up is what it is owed: two sides worth 20 and 15 KAS at the agreed prices, 35 KAS of margin,
    // the legs entered at those prices; the counterparty posts bounds no nearer than the owner's were (8.55 for 9,
    // 12.6 for 12), so the pairs' other parties hold wider ranges than before.
    assert!(h.last.contains("the owner gives up position(s) worth 35 KAS at the agreed prices and is owed 35 KAS in the account"), "{}", h.last);
    assert!(h.last.contains("pair [8.55 .. 12] KAS") && h.last.contains("pair [9 .. 12.6] KAS"), "{}", h.last);
    assert!(h.last.contains("leg 1: long 10 units") && h.last.contains("from 11 KAS") && h.last.contains("from 10.5 KAS"), "{}", h.last);
    let out = h.ok(&["x-show", "--account", &x, "--price1", "11", "--price2", "13"]);
    assert!(out.contains("a is owed 10 KAS (leg 1 0 KAS, leg 2 -25 KAS), b 45 KAS; the notional is 240 KAS and its maintenance 12 KAS"), "{out}");
    // A leg attested for another signed price than the other.
    let fund = h.funding(1_000);
    let odd = sign(&h, 33_600, "11", "13", 33_590);
    let err = h.fail(&with(&["x-liquidate", "--account", &x, "--side", "a", "--funder", &exec, "--funding", &fund], &odd));
    assert!(err.contains("REFUSED: leg 1 is attested for the time 33600 and leg 2 for 33590: an account is priced for one time"), "{err}");
    let t1 = sign(&h, 33_600, "11", "13", 33_600);
    let w = h.step(&exec, &with(&["x-liquidate", "--account", &x, "--side", "a"], &t1), "cross pending claim", 33_610);
    report.push(("x-liquidate", masses(&h.last)));
    h.build(&me, &["x-cure", "--pending", &w]);
    report.push(("x-cure", masses(&h.last)));
    h.build(&exec, &["x-finalize", "--pending", &w]);
    report.push(("x-finalize", masses(&h.last)));
    let t2 = sign(&h, 33_650, "13.5", "13", 33_650);
    h.build(&exec, &with(&["x-close", "--account", &x, "--side", "a", "--key", &me, "--daa", "33660"], &t2));
    report.push(("x-close", masses(&h.last)));
    for (what, m) in &report {
        println!("C9 mass {what}: {}", m.join(" | "));
        assert!(!m.is_empty(), "{what} printed no mass");
        // Every one is a standard transaction: under 100,000 grams of compute, of storage, and of twice its bytes
        // (the transient mass the fee is charged on).
        for l in m {
            let num = |after: &str| l.split(after).nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse::<u64>().ok()).unwrap_or_else(|| panic!("{l}"));
            let (compute, storage, bytes) = (num("compute mass "), num("storage mass "), num("bytes "));
            assert!(compute < 100_000 && storage < 100_000 && 2 * bytes < 100_000, "{what}: {l}");
            if *what == "x-swap" {
                assert!(2 * bytes > 50_000, "the swap is the heaviest: over half of a standard transaction by its bytes");
            }
        }
    }
}
