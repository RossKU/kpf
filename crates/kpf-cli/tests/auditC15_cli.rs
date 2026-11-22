//! Review C15 of the committee family (docs/audit/REVIEW-C15.md): `cosign --market` after the fixes of review C14.
//! The harness is that of committee_cli.rs.

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

fn write_partial(path: &str, mut v: Value, mut part: OPartial) {
    let rep = kpf_protocol::tx::masses(&part.tx, &part.entries);
    part.tx.set_storage_mass(rep.storage);
    part.tx.finalize();
    v["partial"] = serde_json::to_value(&part).unwrap();
    std::fs::write(path, serde_json::to_string(&v).unwrap()).unwrap();
}

/// Output 1 becomes the object `redeem`, a genesis authored by input 0.
fn put_object(part: &mut OPartial, redeem: Vec<u8>) {
    let mut o = part.tx.outputs[1].clone();
    o.script_public_key = kpf_protocol::tx::p2sh_spk(&redeem);
    o.covenant = None;
    let id = kaspa_consensus_core::hashing::covenant_id::covenant_id(part.tx.inputs[0].previous_outpoint, std::iter::once((1u32, &o)));
    o.covenant = Some(kaspa_consensus_core::tx::CovenantBinding { authorizing_input: 0, covenant_id: id });
    part.tx.outputs[1] = o;
    part.created = vec![(1, redeem, id)];
}

#[test]
fn c15_fixed_an_account_with_terms_that_are_not_its_markets_is_refused_and_every_term_is_printed() {
    let mut h = H::new("c15");
    let (exec, _members, _committee, _epoch_table1, market1) = setup(&mut h);
    let me = h.key("me");
    let addr = |h: &H, k: &str| h.ok(&["address", "--key", k]).split_whitespace().find(|w| w.contains(':')).expect("an address").to_string();
    let (exec_addr, me_addr) = (addr(&h, &exec), addr(&h, &me));
    let offer = h.funding(1_000);
    let base = h.p("base.json");
    h.ok(&["x-open", "--market1", &market1, "--size1", "10", "--dir1", "long", "--x1", "11", "--a", &me_addr, "--b", &exec_addr, "--a-margin", "50", "--b-margin", "50", "--daa", "33500", "--funder", &exec_addr, "--funding", &offer, "--partial", &base]);
    let read = |h: &H, f: &str, market: &str| h.run(&["cosign", "--tx", f, "--key", &exec, "--market", market, "--daa", "33600", "--out-dir", &h.p("signed")]);
    let (ok, honest, err) = read(&h, &base, &market1);
    assert!(ok, "{honest}{err}");
    // The same account, the same legs and margins; its own terms the writer's: any signed price of the last weeks serves a
    // claim, a liquidation is final at once, and its fee is the whole of what the loser is owed.
    let (v, mut part) = read_partial(&base);
    let k = kpf_protocol::committee_artifacts::xkit();
    let rs = part.created[0].1.clone();
    let mut c = kpf_protocol::cross::Cross::decode_state(&rs[1..1 + kpf_protocol::cross::CROSS_STATE_LEN]).expect("the account's state");
    assert_eq!((c.t.late_daa, c.t.grace_daa, c.t.liq_fee_bps), (100, 100, 50), "the terms `x-open` takes from the market");
    for late in [4_000_000_000i64, 25_920_000, 2_592_000, 864_000, 86_400] {
        c.t.late_daa = late;
        if c.check().is_ok() {
            break;
        }
    }
    c.t.grace_daa = 0;
    c.t.liq_fee_bps = 10_000;
    assert!(c.check().is_ok(), "{:?}", c.t);
    assert_eq!(c.t.late_daa, 864_000, "a day: the longest the script accepts");
    put_object(&mut part, c.redeem(k));
    let f = h.p("terms.json");
    write_partial(&f, v, part);
    // (As fixed after this round: with the market file, an account whose terms are not those of its markets is refused;
    // and every term is printed.)
    let (ok, out, err) = read(&h, &f, &market1);
    assert!(!ok && err.contains("REFUSED") && err.contains("its terms are not those of its markets"), "{out}{err}");
    assert!(honest.contains("terms: a claim lands within 100 DAA of its time") && honest.contains("a liquidation is cured within 100 DAA") && honest.contains("its fee 0.5 % of the notional"), "{honest}");

    // A market file without the committee's genesis facts as the co-signer's reference.
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&market1).unwrap()).unwrap();
    let had = m.as_object_mut().unwrap().remove("committee").is_some();
    let bare = h.p("market-bare.json");
    std::fs::write(&bare, serde_json::to_string(&m).unwrap()).unwrap();
    let (ok, out, err) = read(&h, &base, &bare);
    // (As fixed: a file without the committee's genesis facts is no reference.)
    assert!(had && !ok && err.contains("carries no genesis facts of its committee"), "{out}{err}");
}
