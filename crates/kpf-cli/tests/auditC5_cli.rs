//! Review C5 of the committee family (docs/audit/REVIEW-C5.md): the charge commands through the binary, offline, as
//! `committee_cli.rs` drives them.
//!
//! `c5_exploit_*`: the commands build the attack; `c5_bounded_*`: what a command says, measured; `c5_holds_*`: refused.

use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

struct H {
    dir: PathBuf,
    n: usize,
}

impl Drop for H {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl H {
    fn new(tag: &str) -> H {
        let dir = std::env::temp_dir().join(format!("kpf-audit-c5-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        H { dir, n: 0 }
    }
    fn p(&self, name: &str) -> String {
        self.dir.join(name).display().to_string()
    }
    fn run(&self, args: &[&str]) -> (bool, String) {
        let o = Command::new(env!("CARGO_BIN_EXE_kpf")).args(args).output().expect("run kpf");
        (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
    }
    fn key(&self, name: &str) -> String {
        let path = self.p(&format!("{name}.key.json"));
        assert!(self.run(&["keygen", "--out", &path]).0);
        path
    }
    fn funding(&mut self) -> String {
        self.n += 1;
        let fund = self.p(&format!("funding-{}.json", self.n));
        std::fs::write(&fund, serde_json::to_string(&json!([{ "txid": format!("{:064x}", 0xc5c5_0000u64 + self.n as u64), "index": 0, "amount": 100_000u64 * 100_000_000, "daa": 0 }])).unwrap()).unwrap();
        fund
    }
    /// Runs a builder with a fresh funding file and out-dir: (accepted, everything printed, the dry-run directory).
    fn build(&mut self, funder: &str, args: &[&str]) -> (bool, String, String) {
        let fund = self.funding();
        let out = self.p(&format!("out{}", self.n));
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--funder", funder, "--funding", &fund, "--out-dir", &out]);
        let (ok, text) = self.run(&a);
        (ok, text, format!("{out}/dry-run"))
    }
    fn obj(&self, dir: &str, kind: &str, daa: u64) -> String {
        let want = format!("{kind}.json");
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(&want)).collect();
        all.sort();
        let f = all.first().unwrap_or_else(|| panic!("no {kind} in {dir}")).clone();
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        v["daa"] = json!(daa);
        std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        f.display().to_string()
    }
    /// Builds, and the text and the directory of what was built.
    fn good(&mut self, funder: &str, args: &[&str]) -> (String, String) {
        let (ok, text, dir) = self.build(funder, args);
        assert!(ok, "kpf {args:?}:\n{text}");
        (text, dir)
    }
    fn step(&mut self, funder: &str, args: &[&str], kind: &str, daa: u64) -> String {
        let (_, dir) = self.good(funder, args);
        self.obj(&dir, kind, daa)
    }
}

struct S {
    funder: String,
    members: Vec<String>,
    judges: Vec<String>,
    committee_file: String,
    committee: String,
    market: String,
    /// Free stakes of the committee's judge, 20 KAS each, accepted at 30,000: of judges[0], judges[0], judges[1].
    stakes: Vec<String>,
}

/// A committee of three members (30, 22, 15 KAS) at DAA 30,100 whose charges cost 5 KAS, wait 300 DAA and are judged by
/// stakes of 20 KAS aged 200 DAA in rounds of 100; its default market.
fn setup(h: &mut H) -> S {
    let funder = h.key("funder");
    let members: Vec<String> = (0..3).map(|i| h.key(&format!("member{i}"))).collect();
    let judges: Vec<String> = (0..2).map(|i| h.key(&format!("judge{i}"))).collect();
    let committee_file = h.p("committee.json");
    let mut committee = h.step(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file, "--charge-bond", "5", "--charge-daa", "300", "--judge-age-daa", "200", "--judge-unbond-daa", "200", "--judge-round-daa", "100", "--judge-min-stake", "20"], "committee", 30_000);
    for (i, kas) in [(0, "30"), (1, "22"), (2, "15")] {
        committee = h.step(&funder, &["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", &i.to_string(), "--amount", kas, "--owner", &members[i]], "committee", 30_100 + i as u64);
    }
    let feed = h.p("feed.txt");
    std::fs::write(&feed, "kpf feed definition v1\n").unwrap();
    let market = h.p("market.json");
    assert!(h.run(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]).0);
    let mut stakes = vec![];
    for j in [0, 0, 1] {
        stakes.push(h.step(&funder, &["judge-stake", "--committee", &committee_file, "--value", "20", "--owner", &judges[j]], "judge-stake", 30_000));
    }
    S { funder, members, judges, committee_file, committee, market, stakes }
}

fn signed_price(h: &H, s: &S, name: &str, member: usize, t0: &str, price: &str) -> String {
    let f = h.p(name);
    let (ok, text) = h.run(&["price-sign", "--market", &s.market, "--t0", t0, "--price", price, "--key", &s.members[member], "--out", &f, "--second-price"]);
    assert!(ok, "{text}");
    f
}

struct Case {
    held: String,
    pending: String,
    contested: String,
    dispute: String,
    decided: String,
    verdict: String,
}

/// The signed price is charged (landing at 30,600), contested with `stake` (at 30,650) and, nobody answering, decided.
fn charged_and_judged(h: &mut H, s: &S, committee: &str, signed_price: &str, stake: usize) -> Case {
    let (_, d) = h.good(&s.funder, &["charge", "--committee", committee, "--signed-price", signed_price]);
    let (held, pending) = (h.obj(&d, "committee", 30_600), h.obj(&d, "charge", 30_600));
    let owner = if stake < 2 { &s.judges[0] } else { &s.judges[1] };
    let (_, d) = h.good(&s.funder, &["charge-contest", "--charge", &pending, "--stake", &s.stakes[stake], "--key", owner, "--daa", "30640"]);
    let (contested, dispute) = (h.obj(&d, "charge", 30_650), h.obj(&d, "out2-judge-stake", 30_650));
    let (verdict, d) = h.good(&s.funder, &["charge-decide", "--charge", &contested, "--dispute", &dispute]);
    Case { held, pending, contested, dispute, decided: h.obj(&d, "charge", 30_760), verdict }
}

/// C5-F1 from the command line, as it was: member 1 (22 KAS) signed 10 for a feed of 10, the signed price was charged,
/// contested with 20 KAS and, nobody answering, refuted; the member lost all 22 KAS, and no command told it to answer
/// (the committee's watcher did not show the hold; the charge's said "waits for a contest" and "with the judge").
/// Now the verdict takes the 20 KAS that were at risk, and the watcher tells the charged key to act.
#[test]
fn c5_fixed_a_charged_member_is_told_to_answer_and_an_unanswered_contest_takes_what_it_staked() {
    let mut h = H::new("true");
    let s = setup(&mut h);
    let t = signed_price(&h, &s, "true.json", 1, "30500", "10");
    let c = charged_and_judged(&mut h, &s, &s.committee, &t, 0);
    assert!(c.verdict.contains("the signed price is refuted"), "{}", c.verdict);
    for (what, o) in [("pending", &c.pending), ("contested", &c.contested)] {
        let (ok, out) = h.run(&["c-watch", "--object", o, "--daa", "30660", "--me", &s.members[1]]);
        assert!(!ok && out.contains("ACT: this charge is against your key") && out.contains("`charge-answer`"), "{what}: {out}");
        let (ok, out) = h.run(&["c-watch", "--object", o, "--daa", "30660"]);
        assert!(ok && !out.contains("ACT"), "{what}, to anyone else: {out}");
        let (ok, out) = h.run(&["c-watch", "--object", o, "--daa", "30660", "--me", &s.members[0]]);
        assert!(ok && !out.contains("ACT"), "{what}, to another member: {out}");
    }
    let (ok, out) = h.run(&["c-watch", "--object", &c.held, "--daa", "30660"]);
    assert!(ok && out.contains("member  1 is HELD for a charge"), "{out}");
    let (ok, out) = h.run(&["c-watch", "--object", &c.held, "--daa", "30660", "--me", &s.members[1]]);
    assert!(!ok && out.contains("ACT: your member is charged") && out.contains("a contest nobody answers within a round is a verdict against you"), "{out}");
    let (text, _) = h.good(&s.funder, &["charge-settle", "--charge", &c.decided, "--committee", &c.held, "--daa", "30800"]);
    assert!(text.contains("slashed: 20 KAS of the stake of 22 KAS") && text.contains("2 KAS and the bond to the charge's author, 18 KAS burned"), "{text}");
    let _ = (&c.dispute, &s.committee_file);
}

/// C5-F2 from the command line, as it was: the refuted member sent `member-forfeit` itself, took 2.2 KAS, and
/// `charge-settle` had no stake to name ("no member of the committee holds that key"): the author's 5 KAS stayed in the
/// charge. Now the member keeps a tenth under its holds, the sender gets nothing, and the charge is settled.
#[test]
fn c5_fixed_a_refuted_member_that_forfeits_itself_keeps_a_tenth_and_the_charge_is_settled() {
    let mut h = H::new("forfeit");
    let s = setup(&mut h);
    let t = signed_price(&h, &s, "false.json", 1, "30500", "20");
    let c = charged_and_judged(&mut h, &s, &s.committee, &t, 0);
    assert!(c.verdict.contains("the signed price is refuted"), "{}", c.verdict);
    let (a, b) = (signed_price(&h, &s, "a.json", 1, "30700", "10"), signed_price(&h, &s, "b.json", 1, "30700", "11"));
    let (text, d) = h.good(&s.members[1], &["member-forfeit", "--committee", &c.held, "--signed-price", &a, "--signed-price", &b]);
    assert!(text.contains("19.8 KAS of its 22 KAS are burned, 2.2 KAS stay under its holds") && text.contains("the sender of this proof is paid nothing"), "{text}");
    let kept = h.obj(&d, "committee", 30_790);
    let (text, _) = h.good(&s.funder, &["charge-settle", "--charge", &c.decided, "--committee", &kept, "--daa", "30800"]);
    assert!(text.contains("slashed: the stake of 2.2 KAS is gone: 2 KAS and the bond to the charge's author, 0.2 KAS burned"), "{text}");
}

/// (f) What the commands refuse and what they say.
#[test]
fn c5_bounded_the_charge_commands_at_their_edges() {
    let mut h = H::new("edges");
    let s = setup(&mut h);
    let t = signed_price(&h, &s, "true.json", 0, "30500", "10");
    let (text, d) = h.good(&s.funder, &["charge", "--committee", &s.committee, "--signed-price", &t]);
    let (held, pending) = (h.obj(&d, "committee", 30_600), h.obj(&d, "charge", 30_600));
    let id_a = text.lines().find(|l| l.starts_with("charge id")).unwrap().split_whitespace().last().unwrap().to_string();
    // A pending charge is not settled without the clock, nor before chargeDaa.
    let (ok, text, _) = h.build(&s.funder, &["charge-settle", "--charge", &pending, "--committee", &held]);
    assert!(!ok && text.contains("a pending charge lapses by the clock"), "{text}");
    let (ok, text, _) = h.build(&s.funder, &["charge-settle", "--charge", &pending, "--committee", &held, "--daa", "30899"]);
    assert!(!ok && text.contains("can still be contested until DAA 30900"), "{text}");
    // A second charge on a member that already has one: nothing says that only one of the two can ever be paid.
    let t2 = signed_price(&h, &s, "true2.json", 0, "30501", "10");
    let (text, _) = h.good(&s.funder, &["charge", "--committee", &held, "--signed-price", &t2]);
    // (Review C5, C5-N4: it said nothing.)
    assert!(text.contains("note: this stake already had a hold"), "{text}");
    // The witness: an id that is not among them, and more than eight after it.
    let ids: Vec<String> = (0..10).map(|i| format!("{:064x}", 0xaa00 + i)).collect();
    let (ok, text) = h.run(&["hold-witness", "--ids", &ids.join(","), "--of", &ids[0]]);
    assert!(!ok && text.contains("more than 8 charges were made after it"), "{text}");
    let (ok, text) = h.run(&["hold-witness", "--ids", &ids[1..].join(","), "--of", &ids[1]]);
    assert!(ok && text.contains("--later"), "{text}");
    let (ok, text) = h.run(&["hold-witness", "--ids", &ids[..3].join(","), "--of", &id_a]);
    assert!(!ok, "{text}");
    // A stake bonded from another committee file (another judge block) contests nothing here.
    let other_file = h.p("other.json");
    h.good(&s.funder, &["committee-create", "--daa", "30000", "--out", &other_file, "--charge-bond", "5", "--charge-daa", "300", "--judge-age-daa", "200", "--judge-unbond-daa", "200", "--judge-round-daa", "50", "--judge-min-stake", "20"]);
    let stray = h.step(&s.funder, &["judge-stake", "--committee", &other_file, "--value", "20", "--owner", &s.judges[0]], "judge-stake", 30_000);
    let (ok, text, _) = h.build(&s.funder, &["charge-contest", "--charge", &pending, "--stake", &stray, "--key", &s.judges[0], "--daa", "30640"]);
    assert!(!ok && text.contains("not a free stake of this committee's judge"), "{text}");
    // A late contest is built with a warning; the verdict is then "lapsed" whatever the judge says.
    let (text, d) = h.good(&s.funder, &["charge-contest", "--charge", &pending, "--stake", &s.stakes[0], "--key", &s.judges[0], "--daa", "30900"]);
    assert!(text.contains("WARNING: the charge has waited 300 DAA"), "{text}");
    let (contested, dispute) = (h.obj(&d, "charge", 30_910), h.obj(&d, "out2-judge-stake", 30_910));
    // What `deposit` tells a staker who answers a charge's dispute.
    let (ok, text, _) = h.build(&s.funder, &["deposit", "--dispute", &dispute, "--side", "yes", "--stake", &s.stakes[2], "--key", &s.judges[1]]);
    assert!(!ok && text.contains("refused without --node") && text.contains("--accept-untraced"), "{text}");
    let (ok, text, _) = h.build(&s.funder, &["deposit", "--dispute", &dispute, "--side", "yes", "--stake", &s.stakes[2], "--key", &s.judges[1], "--accept-untraced"]);
    // The answer to a charge's dispute is always untraced (the trace knows only a pair's claim), and the rule it
    // prints is the oracle family's (the market's g, at t0 and at the landing), not a signed price's (the committee's spread at t0).
    assert!(ok && text.contains("NOT CHECKED that the claim's id was born alone in a KpfPairO.propose"), "{text}");
    assert!(text.contains("after the deposit: NO leads, YES 20 KAS, NO 20 KAS"), "{text}");
    let (text, _) = h.good(&s.funder, &["charge-decide", "--charge", &contested, "--dispute", &dispute]);
    assert!(text.contains("the charge lapsed whatever the judge says"), "{text}");
    // A committee whose charges wait one DAA for a contest (no contest can land in time: its members are never slashed
    // for a false price). Review C5 (C5-F5): created, joined and given a market without a word.
    let weak = h.p("weak.json");
    let (text, d) = h.good(&s.funder, &["committee-create", "--daa", "30000", "--out", &weak, "--charge-daa", "1"]);
    assert!(text.contains("WARNING: chargeDaa is 1"), "{text}");
    let r = h.obj(&d, "committee", 30_000);
    let (text, _) = h.good(&s.funder, &["committee-join", "--committee", &r, "--genesis", &weak, "--member", "0", "--amount", "30", "--owner", &s.members[0]]);
    assert!(text.contains("WARNING: chargeDaa is 1"), "{text}");
    let _ = r;
}
