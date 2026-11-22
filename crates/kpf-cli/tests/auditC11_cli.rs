//! Review C11 of the committee family (docs/audit/REVIEW-C11.md): a partly signed transaction as its writer could
//! have made it, read and signed by `cosign`. The harness (`H`, `setup`, ...) is that of committee_cli.rs.
//!
//! `c11_exploit_*`: the command prints and signs what it should have refused or flagged.

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

use kpf_protocol::oracle_build::OPartial;

fn read_partial(path: &str) -> (Value, OPartial) {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let part: OPartial = serde_json::from_value(v["partial"].clone()).expect("a partial transaction");
    (v, part)
}

/// The file as its writer could have made it: the transaction edited, its storage mass and id set again. (The
/// writer of this file signs nothing: every input is the co-signer's.)
fn write_partial(path: &str, mut v: Value, mut part: OPartial) {
    let rep = kpf_protocol::tx::masses(&part.tx, &part.entries);
    part.tx.set_storage_mass(rep.storage);
    part.tx.finalize();
    v["partial"] = serde_json::to_value(&part).unwrap();
    std::fs::write(path, serde_json::to_string(&v).unwrap()).unwrap();
}

/// C11-F1 and C11-F2, the tool's half, as fixed. `cosign` named an output an object by its script alone: the account
/// `x-open --partial` writes, with the binding taken off its output, was printed word for word as the honest one and
/// signed (51.2 KAS of the co-signer's in a UTXO nothing spends), and the listing after the signing used the file's
/// labels. Now an object is its script and a covenant id the transaction gives it, and every word is the tool's.
#[test]
fn c11_fixed_cosign_refuses_an_account_whose_output_carries_no_covenant_id() {
    let mut h = H::new("c11");
    let (exec, _members, _committee, _epoch_table1, market1) = setup(&mut h);
    let me = h.key("me");
    let addr = |h: &H, k: &str| h.ok(&["address", "--key", k]).split_whitespace().find(|w| w.contains(':')).expect("an address").to_string();
    let (exec_addr, me_addr) = (addr(&h, &exec), addr(&h, &me));
    let offer = h.funding(1_000);
    let req = h.p("request.json");
    h.ok(&["x-open", "--market1", &market1, "--size1", "10", "--dir1", "long", "--x1", "11", "--a", &me_addr, "--b", &exec_addr, "--a-margin", "1", "--b-margin", "50", "--daa", "33500", "--funder", &exec_addr, "--funding", &offer, "--partial", &req]);
    let honest = h.ok(&["cosign", "--tx", &req, "--key", &exec, "--out-dir", &h.p("honest")]);
    assert!(honest.contains("YOU SIGN THIS") && honest.contains("in an object (covenant id ") && honest.contains("margin: a 1 KAS, b 50 KAS"), "{honest}");

    // The same file, the account's output without its covenant binding.
    let (v, mut part) = read_partial(&req);
    let bound = part.tx.outputs.iter().filter(|o| o.covenant.is_some()).count();
    assert_eq!(bound, 1, "one output of the transaction carries a covenant id: the account");
    part.tx.outputs.iter_mut().for_each(|o| o.covenant = None);
    let bare = h.p("bare.json");
    write_partial(&bare, v, part);
    // (As fixed after this round: an object is its script and a covenant id the transaction gives it; without the id
    // nothing is signed.)
    let (ok, out, err) = h.run(&["cosign", "--tx", &bare, "--key", &exec, "--out-dir", &h.p("bare")]);
    assert!(!ok && err.contains("REFUSED") && err.contains("carries no covenant id") && err.contains("Nothing was signed"), "{out}\n{err}");
    assert!(!out.contains("not submitted") && !out.contains("txid"), "{out}");

    // The co-signer's change sent to another key, and the file's title the writer's words: printed, signed, no warning.
    let (mut v, mut part) = read_partial(&req);
    let mine = part.entries[part.open[0].0].script_public_key.clone();
    let changed = part.tx.outputs.iter_mut().filter(|o| o.script_public_key == mine).map(|o| o.script_public_key = kpf_protocol::tx::p2pk_spk(&[7u8; 32])).count();
    assert_eq!(changed, 1);
    v["title"] = json!("a refund to you: nothing leaves your wallet");
    let away = h.p("away.json");
    write_partial(&away, v, part);
    let (ok, out, err) = h.run(&["cosign", "--tx", &away, "--key", &exec, "--out-dir", &h.p("away")]);
    assert!(ok, "{out}{err}");
    // What the co-signer is shown: the writer's title; the output by its script, with no warning; one sum for "the
    // objects, the other outputs and the fee"; and then, in the listing of the signed transaction, the same output
    // under the label the file carries for it: "change".
    assert!(out.contains("the writer calls it \"a refund to you: nothing leaves your wallet\"; what follows is read from the transaction"), "{out}");
    assert!(out.contains("out 0: 948.797917 KAS to ANOTHER ADDRESS: "), "{out}");
    assert!(out.contains("you put in 1000 KAS: 0 KAS come back to you, 51.2 KAS go into objects in which you have a part, 0 KAS into objects in which you have none, 948.797917 KAS to other addresses or unknown scripts"), "{out}");
    assert!(out.contains("WARNING: 948.797917 KAS go to addresses or scripts that are neither yours nor an object printed above"), "{out}");
    assert!(out.contains("out 0: to another address 948.797917 KAS") && !out.contains("change"), "{out}");
}
