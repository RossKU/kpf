//! Review C3 of the committee family (docs/audit/REVIEW-C3.md): the `kpf` binary driven offline, as
//! `committee_cli.rs` drives it (synthetic funding files, the DAA a node would report written into each object file,
//! nothing submitted).
//!
//! `c3_fixed_*`: what review C3 measured as a gap and the commands now refuse (the old numbers in a comment).
//! `c3_bounded_*`: what a command does that its user should be told, measured.
//! `c3_holds_*`: a mistake or an attack that the command refuses.

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

impl H {
    fn new(tag: &str) -> H {
        let dir = std::env::temp_dir().join(format!("kpf-audit-c3-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        H { dir, n: 0, last: String::new() }
    }
    fn p(&self, name: &str) -> String {
        self.dir.join(name).display().to_string()
    }
    fn run(&self, args: &[&str]) -> (bool, String, String) {
        let o = Command::new(env!("CARGO_BIN_EXE_kpf")).args(args).output().expect("run kpf");
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
        format!("{out}{err}")
    }
    fn key(&self, name: &str) -> String {
        let path = self.p(&format!("{name}.key.json"));
        self.ok(&["keygen", "--out", &path]);
        path
    }
    fn funding(&mut self, kas: u64) -> String {
        self.n += 1;
        let txid = format!("{:064x}", 0xc3c3_0000u64 + self.n as u64);
        let path = self.p(&format!("funding-{}.json", self.n));
        std::fs::write(&path, serde_json::to_string(&json!([{ "txid": txid, "index": 0, "amount": kas * 100_000_000, "daa": 0 }])).unwrap()).unwrap();
        path
    }
    /// The arguments of a builder with a fresh funding file and out-dir, and that out-dir's dry-run directory.
    fn funded(&mut self, funder: &str, args: &[&str]) -> (Vec<String>, String) {
        let fund = self.funding(100_000);
        self.n += 1;
        let out = self.p(&format!("out{}", self.n));
        let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        a.extend(["--funder".into(), funder.into(), "--funding".into(), fund, "--out-dir".into(), out.clone()]);
        (a, format!("{out}/dry-run"))
    }
    fn build(&mut self, funder: &str, args: &[&str]) -> String {
        let (a, dir) = self.funded(funder, args);
        let stdout = self.ok(&a.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(stdout.contains("not submitted"), "{stdout}");
        self.last = stdout;
        dir
    }
    fn refuse(&mut self, funder: &str, args: &[&str]) -> String {
        let (a, _) = self.funded(funder, args);
        self.fail(&a.iter().map(String::as_str).collect::<Vec<_>>())
    }
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

const FEED: &str = "kpf feed definition v1\nquantity: example underlying\nunit: KAS per whole unit\n";

struct S {
    funder: String,
    members: Vec<String>,
    committee_file: String,
    committee: String,
    epoch_table: String,
    market: String,
    feed: String,
}

/// A committee with three members (30, 22, 15 KAS), rolled into [33,000, 36,000), and the default market on it.
fn setup(h: &mut H) -> S {
    let funder = h.key("funder");
    let members: Vec<String> = (0..3).map(|i| h.key(&format!("member{i}"))).collect();
    let committee_file = h.p("committee.json");
    let d = h.build(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file]);
    let mut committee = h.obj(&d, "committee", 30_000);
    for (i, kas) in [(0, "30"), (1, "22"), (2, "15")] {
        committee = h.step(&funder, &["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", &i.to_string(), "--amount", kas, "--owner", &members[i]], "committee", 30_100 + i as u64);
    }
    let d = h.build(&funder, &["committee-roll", "--committee", &committee, "--daa", "33000"]);
    let epoch_table = h.obj(&d, "epoch table", 33_000);
    let committee = h.obj(&d, "committee", 33_000);
    let feed = h.p("feed.txt");
    std::fs::write(&feed, FEED).unwrap();
    let market = h.p("market.json");
    h.ok(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]);
    S { funder, members, committee_file, committee, epoch_table, market, feed }
}

fn signed_prices(h: &H, s: &S, who: &[usize], t0: u64, price: &str) -> Vec<String> {
    let mut v = vec![];
    for i in who {
        let f = h.p(&format!("price-{t0}-{i}-{price}.json"));
        h.ok(&["price-sign", "--market", &s.market, "--t0", &t0.to_string(), "--price", price, "--key", &s.members[*i], "--out", &f, "--second-price"]);
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

/// Two orders of 10 units at 10 KAS (the long's rate 1, the short's 2), opened at DAA 33,400: the pair [9, 12].
fn open_pair(h: &mut H, s: &S, long: &str, short: &str) -> String {
    let lo = h.step(&s.funder, &["c-order", "--market", &s.market, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "9000000", "--owner", long], "committee order", 33_100);
    let so = h.step(&s.funder, &["c-order", "--market", &s.market, "--side", "short", "--size", "10", "--limit", "10", "--rate", "2", "--expiry", "9000000", "--owner", short], "committee order", 33_100);
    h.step(&s.funder, &["c-open", "--long", &lo, "--short", &so, "--n", "10", "--x", "10", "--daa", "33400", "--market", &s.market], "committee pair", 33_400)
}

#[test]
fn c3_fixed_c_order_refuses_a_market_file_nobody_could_check_and_an_expiry_already_past() {
    // Review C3 (c3_bounded_c_order_escrows_into_a_market_file_that_c_check_refuses): the market file with its committee's
    // genesis facts cut out, which `c-check` refuses, was escrowed into without a word (11.2 KAS), and so was an order
    // whose `--expiry` (100) lay before the present (33,100).
    let mut h = H::new("order");
    let s = setup(&mut h);
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&s.market).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("committee");
    let bare = h.p("market-without-committee.json");
    std::fs::write(&bare, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    assert!(h.fail(&["c-check", "--market", &bare, "--feed", &s.feed]).contains("REFUSED: the market file has no committee genesis facts"));
    let owner = h.key("owner");
    let order = ["c-order", "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--owner", &owner];
    // The steps that used to succeed.
    let err = h.refuse(&s.funder, &[&order[..], &["--market", &bare, "--expiry", "9000000"]].concat());
    assert!(err.contains("REFUSED: the market file has no committee genesis facts"), "{err}");
    let err = h.refuse(&s.funder, &[&order[..], &["--market", &s.market, "--expiry", "100", "--daa", "33100"]].concat());
    assert!(err.contains("REFUSED") && err.contains("expiry"), "{err}");
    // Said aloud, the unchecked file is escrowed into; and the checked one with an expiry ahead.
    h.build(&s.funder, &[&order[..], &["--market", &bare, "--expiry", "9000000", "--unchecked"]].concat());
    assert!(h.last.contains("escrow 11.2 KAS"), "{}", h.last);
    h.build(&s.funder, &[&order[..], &["--market", &s.market, "--expiry", "33101", "--daa", "33100"]].concat());
    // Offline with no clock the expiry is taken as typed.
    h.build(&s.funder, &[&order[..], &["--market", &s.market, "--expiry", "100"]].concat());
}

#[test]
fn c3_fixed_committee_roll_with_steps_is_compared_with_the_clock() {
    // Review C3 (c3_bounded_committee_roll_with_steps_reads_no_clock): at 33,100 `--steps 2` built the roll to [39,000,
    // 42,000), an epoch that had not begun, and at 90,000 `--steps 1` wrote the epoch tables of [36,000, 39,000) for 4 KAS
    // without a word.
    let mut h = H::new("roll");
    let s = setup(&mut h);
    let err = h.refuse(&s.funder, &["committee-roll", "--committee", &s.committee, "--steps", "2", "--daa", "33100"]);
    assert!(err.contains("REFUSED: --steps 2 is the epoch from DAA 39000, which has not begun (now 33100)"), "{err}");
    h.build(&s.funder, &["committee-roll", "--committee", &s.committee, "--steps", "1", "--daa", "90000"]);
    assert!(h.last.contains("epoch [36000, 39000) with 4 epoch tables") && h.last.contains("WARNING"), "{}", h.last);
    h.build(&s.funder, &["committee-roll", "--committee", &s.committee, "--daa", "90000"]);
    assert!(h.last.contains("epoch [90000, 93000) with 4 epoch tables") && !h.last.contains("WARNING"), "{}", h.last);
    // The step that holds the present is not warned about.
    h.build(&s.funder, &["committee-roll", "--committee", &s.committee, "--steps", "1", "--daa", "36100"]);
    assert!(h.last.contains("epoch [36000, 39000) with 4 epoch tables") && !h.last.contains("WARNING"), "{}", h.last);
}

#[test]
fn c3_holds_the_claim_commands_refuse_what_would_be_refused_or_lost() {
    let mut h = H::new("claims");
    let s = setup(&mut h);
    let (long, short) = (h.key("long"), h.key("short"));
    let pair = open_pair(&mut h, &s, &long, &short);
    // Signed prices of two DAAs: the signed price is the first file's; the other member's price for another signed price is not counted.
    // (Members 1 and 2 hold 37 of 67 KAS; a price needs 44.89.)
    let mut mixed = signed_prices(&h, &s, &[1, 2], 33_500, "10.5");
    mixed.extend(signed_prices(&h, &s, &[0], 33_501, "10.5"));
    let err = h.refuse(&s.funder, &with(&["c-close", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table, "--key", &long], &mixed));
    assert!(err.contains("no price") && err.contains("members holding 37 KAS signed the time 33500"), "{err}");
    // A close signed by the other party's key.
    let good = signed_prices(&h, &s, &[0, 1, 2], 33_500, "10.5");
    let err = h.refuse(&s.funder, &with(&["c-close", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table, "--key", &short], &good));
    assert!(!err.is_empty());
    // A liquidation at a price that does not trigger; one that does (9.4), landed in time, and its cure by the wrong key.
    let err = h.refuse(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table], &good));
    assert!(err.contains("does not liquidate the long"), "{err}");
    let low = signed_prices(&h, &s, &[0, 1, 2], 33_500, "9.4");
    let w = h.step(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table], &low), "committee pending part", 33_510);
    let err = h.refuse(&s.funder, &["c-cure", "--pending", &w, "--key", &short]);
    assert!(!err.is_empty());
    h.build(&s.funder, &["c-cure", "--pending", &w, "--key", &long]);
    assert!(h.last.contains("cured: the bound moves"), "{}", h.last);
    // The same claim landed 101 DAA after its time: `c-finalize` says VOID and restores, at once.
    let void = h.step(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table], &low), "committee pending part", 33_601);
    h.build(&s.funder, &["c-finalize", "--pending", &void]);
    assert!(h.last.contains("VOID: the claim landed 101 DAA after its time"), "{}", h.last);
    // An order accepted too late for the pair's lease (6,081,400) does not take its listed side: the command names the
    // renewal. An order accepted earlier does.
    let listed = h.step(&s.funder, &["c-amend", "--pair", &pair, "--side", "long", "--price", "10", "--key", &long], "committee pair", 33_500);
    let taker = h.key("taker");
    let young = h.step(&s.funder, &["c-order", "--market", &s.market, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "9000000", "--owner", &taker], "committee order", 3_100_000);
    let err = h.refuse(&s.funder, &["c-take", "--pair", &listed, "--order", &young, "--x", "10", "--market", &s.market]);
    assert!(err.contains("ends less than half a term after the order was accepted") && err.contains("c-keep-alive"), "{err}");
    let old = h.step(&s.funder, &["c-order", "--market", &s.market, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "9000000", "--owner", &taker], "committee order", 33_450);
    h.build(&s.funder, &["c-take", "--pair", &listed, "--order", &old, "--x", "10", "--market", &s.market]);
    assert!(h.last.contains("pair [9 .. 12] KAS of 10 units"), "{}", h.last);
}

#[test]
fn c3_fixed_c_cure_refuses_a_void_claim() {
    // Review C3 (c3_bounded_c_cure_cures_a_void_claim_and_c_liquidate_offline_checks_no_age): the loser's `c-cure` of a
    // void claim was built without a word (0.115 KAS a unit of margin added, the pair unlisted), where `c-finalize`
    // restores the pair as it was.
    let mut h = H::new("void");
    let s = setup(&mut h);
    let (long, short) = (h.key("long"), h.key("short"));
    let pair = open_pair(&mut h, &s, &long, &short);
    // Fixed after review C4 (C4-N4): offline the present is what `--daa` says, and the claim commands compare the signed price
    // with it as they do with a node: a liquidation on a signed price 2,000 DAA old is refused, and one with 10 DAA left to
    // land is built with a warning. Without any clock nothing can be compared: that one is built (and lands void here).
    let low = signed_prices(&h, &s, &[0, 1, 2], 33_500, "9.4");
    let err = h.refuse(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table, "--daa", "35500"], &low));
    assert!(err.contains("REFUSED: the signed price is 2000 DAA old"), "{err}");
    h.build(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table, "--daa", "33590"], &low));
    assert!(h.last.contains("WARNING: the signed price is 90 DAA old and the claim must land within 10 DAA"), "{}", h.last);
    let d = h.build(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table], &low));
    assert!(h.last.contains("liquidation: the fee") && !h.last.contains("WARNING"), "{}", h.last);
    let void = h.obj(&d, "committee pending part", 35_500);
    let (ok, out, _) = h.run(&["c-watch", "--object", &void, "--daa", "35501"]);
    assert!(!ok && out.contains("VOID: landed at DAA 35500") && out.contains("ACT: it can be finalized now"), "{out}");
    // The step that used to succeed.
    let err = h.refuse(&s.funder, &["c-cure", "--pending", &void, "--key", &long]);
    assert!(err.contains("REFUSED: this claim is void") && err.contains("c-finalize"), "{err}");
    // A claim that landed in time is cured as before.
    let d = h.build(&s.funder, &with(&["c-liquidate", "--pair", &pair, "--side", "long", "--epoch-table", &s.epoch_table], &low));
    let valid = h.obj(&d, "committee pending part", 33_510);
    h.build(&s.funder, &["c-cure", "--pending", &valid, "--key", &long]);
    assert!(h.last.contains("cured: the bound moves"), "{}", h.last);
}

#[test]
fn c3_fixed_the_signer_refuses_a_second_price_for_a_price_into_any_file() {
    let mut h = H::new("signed prices");
    let s = setup(&mut h);
    let (a, b) = (h.p("a.json"), h.p("b.json"));
    h.ok(&["price-sign", "--market", &s.market, "--t0", "33500", "--price", "10", "--key", &s.members[0], "--out", &a]);
    assert!(h.fail(&["price-sign", "--market", &s.market, "--t0", "33500", "--price", "11", "--key", &s.members[0], "--out", &a]).contains("a second price for one time"));
    // The same second price into another file was signed (C3-N2, C4-N2): the guard was the file the first price was
    // written to. Now the key's journal refuses it; the same price again is signed, and `--second-price` overrides.
    assert!(h.fail(&["price-sign", "--market", &s.market, "--t0", "33500", "--price", "11", "--key", &s.members[0], "--out", &b]).contains("this key already signed 10 KAS for the time 33500"));
    h.ok(&["price-sign", "--market", &s.market, "--t0", "33500", "--price", "10", "--key", &s.members[0], "--out", &b]);
    let out = h.ok(&["price-sign", "--market", &s.market, "--t0", "33500", "--price", "11", "--key", &s.members[0], "--out", &b, "--second-price"]);
    assert!(out.contains("signed: 11 KAS at DAA 33500"), "{out}");
    // Without `--daa` or `--node` a signed price of any DAA is signed.
    let out = h.ok(&["price-sign", "--market", &s.market, "--t0", "400000000000", "--price", "10", "--key", &s.members[0], "--out", &h.p("far.json")]);
    assert!(out.contains("at DAA 400000000000"), "{out}");
}

#[test]
fn c3_holds_the_watcher_raises_an_expired_order_and_a_refund_pays_the_owner_all_but_the_fee() {
    let mut h = H::new("refund");
    let s = setup(&mut h);
    let owner = h.key("owner");
    let o = h.step(&s.funder, &["c-order", "--market", &s.market, "--side", "long", "--size", "10", "--limit", "10", "--rate", "1", "--expiry", "40000", "--owner", &owner], "committee order", 33_100);
    assert!(h.ok(&["c-watch", "--object", &o, "--daa", "39999"]).contains("resting"));
    let (ok, out, err) = h.run(&["c-watch", "--object", &o, "--daa", "40000"]);
    assert!(!ok && out.contains("ACT: the order has expired and can still be filled") && err.contains("ACT NOW"), "{out}{err}");
    // The refund: 11.2 KAS escrowed, 11.1 to the owner, 0.1 (the market's keeper fee floor) to whoever sends it.
    h.build(&s.funder, &["c-refund", "--order", &o]);
    assert!(h.last.contains("owner 11.1 KAS"), "{}", h.last);
    // An exit before its release: the transaction carries the release as its lock time, and the command says from when.
    let committee = h.step(&s.funder, &["committee-leave", "--committee", &s.committee, "--member", "2", "--key", &s.members[2]], "exit", 33_200);
    h.build(&s.funder, &["exit-withdraw", "--exit", &committee, "--daa", "33300"]);
    assert!(h.last.contains("15 KAS to") && h.last.contains("from DAA 108000"), "{}", h.last);
    let _ = &s.committee_file;
}
