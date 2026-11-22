//! Review C16 of the committee family (docs/audit/REVIEW-C16.md): `cosign --market` on accounts of two markets, and
//! what it prints of each kind a funder may write. The harness is that of committee_cli.rs.

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

/// Builds `cmd` as a partly signed file funded by the executor's address; the executor reads it with `more`.
fn flow(h: &mut H, exec: &str, exec_addr: &str, name: &str, cmd: &[&str], keys: &[&str], more: &[&str]) -> (bool, String) {
    let offer = h.funding(1_000);
    let f = h.p(&format!("{name}.json"));
    let mut args: Vec<&str> = cmd.to_vec();
    args.extend(["--funder", exec_addr, "--funding", &offer, "--partial", &f]);
    for k in keys {
        args.extend(["--key", k]);
    }
    let (ok, out, err) = h.run(&args);
    assert!(ok, "{name}: the builder: {out}{err}");
    let dir = h.p(&format!("{name}-signed"));
    let mut c = vec!["cosign", "--tx", f.as_str(), "--key", exec, "--out-dir", dir.as_str()];
    c.extend_from_slice(more);
    let (ok, out, err) = h.run(&c);
    (ok, format!("{out}{err}"))
}

#[test]
fn c16_holds_an_account_of_two_markets_is_accepted_in_either_order_and_refused_without_both_files() {
    let mut h = H::new("c16");
    let (exec, _members, _committee, _epoch_table1, market1) = setup(&mut h);
    let (me, other) = (h.key("me"), h.key("other"));
    let exec_addr = h.ok(&["address", "--key", &exec]).split_whitespace().find(|w| w.contains(':')).expect("an address").to_string();
    // A second market of the same committee with other terms: a wider tolerance, a longer grace, a shorter lease.
    let feed2 = h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = h.p("market2.json");
    let (ok, out, err) = h.run(&["c-market", "--committee", &h.p("committee.json"), "--feed", &feed2, "--g-bps", "80", "--grace-daa", "300", "--out", &market2]);
    assert!(ok, "{out}{err}");
    let a = open_pair(&mut h, &exec, &market1, &me, &other);
    let b = open_pair(&mut h, &exec, &market2, &other, &me);
    let both = ["--market", market1.as_str(), "--market", market2.as_str(), "--daa", "33600"];
    let swap = |p1: &str, s1: &str, x1: &str, p2: &str, s2: &str, x2: &str| vec!["x-swap".to_string(), "--pair1".into(), p1.into(), "--side1".into(), s1.into(), "--x1".into(), x1.into(), "--pair2".into(), p2.into(), "--side2".into(), s2.into(), "--x2".into(), x2.into(), "--b-margin".into(), "20".into(), "--daa".into(), "33500".into()];
    let run = |h: &mut H, name: &str, cmd: Vec<String>, more: &[&str]| {
        let c: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        flow(h, &exec, &exec_addr, name, &c, &[&me], more)
    };
    let (ok, out) = run(&mut h, "two-markets", swap(&a, "long", "11", &b, "short", "10.5"), &both);
    // The terms are the more careful of the two markets' (a cure within 300 DAA: market 2's), and all are printed.
    assert!(ok && !out.contains("REFUSED") && out.contains("terms: a claim lands within 100 DAA of its time") && out.contains("a liquidation is cured within 300 DAA"), "{out}");
    let (ok, out) = run(&mut h, "other-order", swap(&b, "short", "10.5", &a, "long", "11"), &both);
    assert!(ok && !out.contains("REFUSED"), "{out}");
    let (ok, out) = run(&mut h, "one-file", swap(&a, "long", "11", &b, "short", "10.5"), &["--market", market1.as_str(), "--daa", "33600"]);
    assert!(!ok && out.contains("REFUSED") && out.contains("which is none of the markets given with --market"), "{out}");
}

use kpf_protocol::oracle_build::OPartial;

/// C16-F1. A free judge stake is printed as its owner alone: the judge block it is bonded under (which committee's
/// charges it judges, and how long it takes to leave) is not shown and is compared with nothing. A stake of the
/// co-signer's key, funded by the co-signer, under the longest unbonding time the script accepts (90 days where the
/// committee's judge has 7) is read with `--daa`, not refused, not warned, and line for line the honest stake's printout.
#[test]
fn c16_fixed_a_free_stake_is_printed_with_its_judge_block() {
    let mut h = H::new("c16s");
    let (exec, _members, _committee, _epoch_table1, _market1) = setup(&mut h);
    let exec_addr = h.ok(&["address", "--key", &exec]).split_whitespace().find(|w| w.contains(':')).expect("an address").to_string();
    let committee_file = h.p("committee.json");
    let offer = h.funding(1_000);
    let f = h.p("stake.json");
    h.ok(&["judge-stake", "--committee", &committee_file, "--value", "100", "--owner", &exec_addr, "--funder", &exec_addr, "--funding", &offer, "--partial", &f]);
    let read = |h: &H, f: &str| h.run(&["cosign", "--tx", f, "--key", &exec, "--daa", "33600", "--out-dir", &h.p("signed")]);
    let (ok, honest, err) = read(&h, &f);
    assert!(ok, "{honest}{err}");
    // The same stake under another judge block: another id, 90 days to leave.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    let mut part: OPartial = serde_json::from_value(v["partial"].clone()).expect("a partial transaction");
    let mut jm = kpf_protocol::committee::ChargeParams::testnet().judge;
    jm.id = [0x99; 32];
    jm.p.unbond = 77_760_000;
    assert!(jm.p.check().is_ok());
    let redeem = kpf_protocol::oracle_artifacts::kit().stake.redeem(&kpf_protocol::judge::Js::free(part.open[0].1, jm).state_bytes());
    part.tx.outputs[1].script_public_key = kpf_protocol::tx::p2sh_spk(&redeem);
    part.created = vec![(1, redeem, Default::default())];
    let rep = kpf_protocol::tx::masses(&part.tx, &part.entries);
    part.tx.set_storage_mass(rep.storage);
    part.tx.finalize();
    v["partial"] = serde_json::to_value(&part).unwrap();
    let g = h.p("stake-90.json");
    std::fs::write(&g, serde_json::to_string(&v).unwrap()).unwrap();
    let (ok, out, err) = read(&h, &g);
    assert!(ok && !out.contains("WARNING") && !out.contains("REFUSED"), "{out}{err}");
    // (As fixed after this round: the judge block is printed, its id whole and every parameter, to be compared with the
    // block the committee names; the two printouts differ where the blocks do.)
    assert!(honest.contains("CHECK: under the judge block ") && out.contains(&format!("CHECK: under the judge block {}", "99".repeat(32))) && out.contains("leaves 77760000 DAA after unbonding"), "{out}");
    assert!(!honest.contains(&"99".repeat(32)) && !honest.contains("77760000"), "{honest}");
}
