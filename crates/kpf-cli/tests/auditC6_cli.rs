//! Review C6 of the committee family (docs/audit/REVIEW-C6.md): `charge-answer`, the watcher and the words of the
//! commands after the fixes of review C5, through the binary, offline. The harness is that of auditC5_cli.rs.
//!
//! `c6_holds_*`: refused; `c6_bounded_*`: what a command says, measured.

#![allow(dead_code)]

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
        let dir = std::env::temp_dir().join(format!("kpf-audit-c6-cli-{tag}-{}", std::process::id()));
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
        std::fs::write(&fund, serde_json::to_string(&json!([{ "txid": format!("{:064x}", 0xc6c6_0000u64 + self.n as u64), "index": 0, "amount": 100_000u64 * 100_000_000, "daa": 0 }])).unwrap()).unwrap();
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

thread_local! {
    static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn address_of(key_file: &str) -> String {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(key_file).unwrap()).unwrap();
    v["address"].as_str().map(|s| s.to_string()).unwrap_or_else(|| panic!("no address in the key file: {v}"))
}

/// (d) `charge-answer` deposits only on the dispute of a contested charge that is among the holds of the stake it is
/// shown, on a member and on the exit the member became; and what it says when the answer does not take the lead.
#[test]
fn c6_holds_charge_answer_takes_only_the_dispute_of_a_charge_the_stake_it_is_shown_holds() {
    let mut h = H::new("answer");
    let s = setup(&mut h);
    let big = h.step(&s.funder, &["judge-stake", "--committee", &s.committee_file, "--value", "25", "--owner", &s.judges[1]], "judge-stake", 30_000);
    // Member 1 signed a true signed price; it is charged and contested with 20 KAS. Member 0 likewise, on the committee as it then is.
    let t1 = signed_price(&h, &s, "t1.json", 1, "30500", "10");
    let (_, d) = h.good(&s.funder, &["charge", "--committee", &s.committee, "--signed-price", &t1]);
    let (held, pending) = (h.obj(&d, "committee", 30_600), h.obj(&d, "charge", 30_600));
    let (_, d) = h.good(&s.funder, &["charge-contest", "--charge", &pending, "--stake", &s.stakes[0], "--key", &s.judges[0], "--daa", "30640"]);
    let (contested, dispute) = (h.obj(&d, "charge", 30_650), h.obj(&d, "out2-judge-stake", 30_650));
    let t0 = signed_price(&h, &s, "t0.json", 0, "30500", "10");
    let (_, d) = h.good(&s.funder, &["charge", "--committee", &held, "--signed-price", &t0]);
    let (held2, pending0) = (h.obj(&d, "committee", 30_610), h.obj(&d, "charge", 30_610));
    let (_, d) = h.good(&s.funder, &["charge-contest", "--charge", &pending0, "--stake", &s.stakes[1], "--key", &s.judges[0], "--daa", "30640"]);
    let (contested0, dispute0) = (h.obj(&d, "charge", 30_650), h.obj(&d, "out2-judge-stake", 30_650));

    let answer = |h: &mut H, dispute: &str, charge: &str, stake: &str, more: &[&str]| {
        let mut a = vec!["charge-answer", "--dispute", dispute, "--charge", charge, "--side", "yes", "--stake", stake, "--key", &s.judges[1]];
        a.extend_from_slice(more);
        let (ok, text, dir) = h.build(&s.funder, &a);
        LAST.with(|l| *l.borrow_mut() = dir);
        (ok, text)
    };
    // The dispute of another charge; a pending charge; no stake shown; a committee that does not hold it.
    let (ok, text) = answer(&mut h, &dispute0, &contested, &big, &["--committee", &held2]);
    assert!(!ok && text.contains("REFUSED: the dispute is not of this contested charge"), "{text}");
    let (ok, text) = answer(&mut h, &dispute, &pending, &big, &["--committee", &held2]);
    assert!(!ok && text.contains("REFUSED: the dispute is not of this contested charge"), "{text}");
    let (ok, text) = answer(&mut h, &dispute, &contested, &big, &[]);
    assert!(!ok && text.contains("give the stake the charge is held by"), "{text}");
    let (ok, text) = answer(&mut h, &dispute, &contested, &big, &["--committee", &s.committee]);
    assert!(!ok && text.contains("this is not a charge the committee wrote against that stake"), "{text}");
    // The charge of member 0 shown against member 1's holds.
    let (ok, text) = answer(&mut h, &dispute0, &contested0, &big, &["--committee", &held2, "--member", "1"]);
    assert!(!ok && text.contains("this is not a charge the committee wrote against that stake"), "{text}");
    // An answer of 20 KAS against 20 KAS on NO does not take the lead (it needs 25): what the command says.
    let (ok, text) = answer(&mut h, &dispute, &contested, &s.stakes[2], &["--committee", &held2]);
    // (Review C6, C6-F4: it was built without a word.)
    assert!(ok && text.contains("after the answer: NO leads, YES 20 KAS, NO 20 KAS") && text.contains("WARNING: this answer does NOT take the lead: your side has 20 KAS and needs 25 KAS") && text.contains("add 5 KAS"), "{text}");
    // 25 KAS take it.
    let (ok, text) = answer(&mut h, &dispute, &contested, &big, &["--committee", &held2]);
    assert!(ok && text.contains("after the answer: YES leads, YES 25 KAS, NO 20 KAS") && text.contains("The judge's question: is the signed price TRUE?") && !text.contains("WARNING"), "{text}");
    // YES leads now: a second answer on YES is refused (only the trailing side is joined).
    let answered = h.obj(&LAST.with(|l| l.borrow().clone()), "out0-judge-stake", 30_670);
    let (ok, text) = answer(&mut h, &answered, &contested, &s.stakes[2], &["--committee", &held2]);
    assert!(!ok, "{text}");
    println!("an answer on the leading side: {}", text.lines().next().unwrap_or(""));
    // The member leaves: the committee no longer holds the key, its exit holds the charge.
    let (_, d) = h.good(&s.funder, &["committee-leave", "--committee", &held2, "--member", "1", "--key", &s.members[1]]);
    let (left, exit) = (h.obj(&d, "committee", 30_700), h.obj(&d, "exit", 30_700));
    let (ok, text) = answer(&mut h, &dispute, &contested, &big, &["--committee", &left]);
    assert!(!ok && text.contains("no member of the committee holds that key"), "{text}");
    let (ok, text) = answer(&mut h, &dispute, &contested, &big, &["--exit", &exit]);
    assert!(ok && text.contains("after the answer: YES leads, YES 25 KAS, NO 20 KAS"), "{text}");
    // The watcher, for a key given as an address.
    let me = address_of(&s.members[1]);
    let (ok, out) = h.run(&["c-watch", "--object", &contested, "--daa", "30660", "--me", &me]);
    assert!(!ok && out.contains("ACT: this charge is against your key and is contested"), "{out}");
    let (ok, out) = h.run(&["c-watch", "--object", &exit, "--daa", "30760", "--me", &me]);
    // The exit of that key (review C6, C6-F4: shown as held, nothing asked of its owner): raised.
    assert!(!ok && out.contains("HELD for a charge") && out.contains("ACT"), "{out}");
    let (ok, out) = h.run(&["c-watch", "--object", &exit, "--daa", "30760", "--me", &s.members[0]]);
    assert!(ok && !out.contains("ACT"), "another key's exit asks nothing of me: {out}");
}

/// C6-N1 and C6-N2, as they were: `member-forfeit` on a held member said "its 22 KAS are forfeited" (19.8 were burned,
/// 2.2 stayed, the sender was paid nothing), and `committee-join` and `c-market` said nothing of a committee whose charges
/// wait one DAA. Now the commands say what happens.
#[test]
fn c6_fixed_the_commands_say_what_a_held_forfeit_does_and_warn_of_a_committee_that_cannot_slash() {
    let mut h = H::new("words");
    let s = setup(&mut h);
    let t = signed_price(&h, &s, "false.json", 1, "30500", "20");
    let (_, d) = h.good(&s.funder, &["charge", "--committee", &s.committee, "--signed-price", &t]);
    let held = h.obj(&d, "committee", 30_600);
    let (a, b) = (signed_price(&h, &s, "a.json", 1, "30700", "10"), signed_price(&h, &s, "b.json", 1, "30700", "11"));
    let (text, d) = h.good(&s.funder, &["member-forfeit", "--committee", &held, "--signed-price", &a, "--signed-price", &b]);
    // 19.8 KAS are burned, 2.2 KAS stay with the member under its hold, and the sender is paid nothing: the command
    // says what it said before the fix.
    assert!(text.contains("It is HELD for a charge: 19.8 KAS of its 22 KAS are burned, 2.2 KAS stay under its holds") && text.contains("the sender of this proof is paid nothing"), "{text}");
    let kept = h.obj(&d, "committee", 30_710);
    let (ok, text, _) = h.build(&s.funder, &["member-forfeit", "--committee", &kept, "--signed-price", &a, "--signed-price", &b]);
    // The same two files again (C6-F2, stated): 1.2 KAS more burned, 1 KAS stays.
    assert!(ok && text.contains("It is HELD for a charge: 1.2 KAS of its 2.2 KAS are burned, 1 KAS stay"), "{text}");
    let weak = h.p("weak.json");
    let (_, d) = h.good(&s.funder, &["committee-create", "--daa", "30000", "--out", &weak, "--charge-daa", "1"]);
    let r = h.obj(&d, "committee", 30_000);
    let (text, _) = h.good(&s.funder, &["committee-join", "--committee", &r, "--genesis", &weak, "--member", "0", "--amount", "30", "--owner", &s.members[0]]);
    assert!(text.contains("WARNING: chargeDaa is 1"), "{text}");
    let (ok, text) = h.run(&["c-market", "--committee", &weak, "--feed", &h.p("feed.txt"), "--out", &h.p("weak-market.json")]);
    assert!(ok && text.contains("chargeDaa is 1"), "{text}");
    let (ok, text) = h.run(&["committee-show", "--committee", &r]);
    assert!(ok && text.contains("WARNING: chargeDaa is 1"), "{text}");
}
