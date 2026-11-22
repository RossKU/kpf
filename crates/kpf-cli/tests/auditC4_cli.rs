//! Review C4 of the committee family (docs/audit/REVIEW-C4.md): `member-forfeit` and `exit-forfeit` through the binary,
//! offline, as `committee_cli.rs` drives it.
//!
//! `c4_holds_*`: what is not a proof is refused by the commands; what is one slashes the member or its exit.

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
        let dir = std::env::temp_dir().join(format!("kpf-audit-c4-cli-{tag}-{}", std::process::id()));
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
    /// Runs a builder with a fresh funding file and out-dir: (accepted, everything printed, the dry-run directory).
    fn build(&mut self, funder: &str, args: &[&str]) -> (bool, String, String) {
        self.n += 1;
        let fund = self.p(&format!("funding-{}.json", self.n));
        std::fs::write(&fund, serde_json::to_string(&json!([{ "txid": format!("{:064x}", 0xc4c4_0000u64 + self.n as u64), "index": 0, "amount": 100_000u64 * 100_000_000, "daa": 0 }])).unwrap()).unwrap();
        let out = self.p(&format!("out{}", self.n));
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--funder", funder, "--funding", &fund, "--out-dir", &out]);
        let (ok, text) = self.run(&a);
        (ok, text, format!("{out}/dry-run"))
    }
    fn obj(&self, dir: &str, kind: &str, daa: u64) -> String {
        let want = format!("-{kind}.json");
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(&want)).collect();
        all.sort();
        let f = all.first().unwrap_or_else(|| panic!("no {kind} in {dir}")).clone();
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        v["daa"] = json!(daa);
        std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        f.display().to_string()
    }
    fn step(&mut self, funder: &str, args: &[&str], kind: &str, daa: u64) -> String {
        let (ok, text, dir) = self.build(funder, args);
        assert!(ok, "kpf {args:?}:\n{text}");
        self.obj(&dir, kind, daa)
    }
}

#[test]
fn c4_holds_the_forfeit_commands_take_only_a_proof_against_the_key_they_slash() {
    let mut h = H::new("forfeit");
    let funder = h.key("funder");
    let members: Vec<String> = (0..3).map(|i| h.key(&format!("member{i}"))).collect();
    let committee_file = h.p("committee.json");
    let mut committee = h.step(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file], "committee", 30_000);
    for (i, kas) in [(0, "30"), (1, "22"), (2, "15")] {
        committee = h.step(&funder, &["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", &i.to_string(), "--amount", kas, "--owner", &members[i]], "committee", 30_100 + i as u64);
    }
    let feed = h.p("feed.txt");
    std::fs::write(&feed, "kpf feed definition v1\n").unwrap();
    let (market, other) = (h.p("market.json"), h.p("other.json"));
    assert!(h.run(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]).0);
    assert!(h.run(&["c-market", "--committee", &committee_file, "--feed", &feed, "--g-bps", "60", "--out", &other]).0);
    let signed_price = |h: &H, name: &str, m: &str, member: usize, t0: &str, price: &str| -> String {
        let f = h.p(name);
        let (ok, text) = h.run(&["price-sign", "--market", m, "--t0", t0, "--price", price, "--key", &members[member], "--out", &f, "--second-price"]);
        assert!(ok, "{text}");
        f
    };
    // Member 1 signed 10 and 10.5 for the time 33,500 (into two files: the signer's guard is one file's).
    let a = signed_price(&h, "a.json", &market, 1, "33500", "10");
    let b = signed_price(&h, "b.json", &market, 1, "33500", "10.5");
    // What is not a proof: one file twice, two signed prices, two markets, two members.
    let same_price_later = signed_price(&h, "c.json", &market, 1, "33501", "10.5");
    let other_market = signed_price(&h, "d.json", &other, 1, "33500", "10.5");
    let other_member = signed_price(&h, "e.json", &market, 0, "33500", "10.5");
    for (what, second) in [("one price twice", &a), ("two signed prices", &same_price_later), ("two markets", &other_market), ("two members", &other_member)] {
        let (ok, text, _) = h.build(&funder, &["member-forfeit", "--committee", &committee, "--signed-price", &a, "--signed-price", second]);
        assert!(!ok && text.contains("REFUSED: not a proof"), "{what}: {text}");
    }
    // A proof against member 1 does not slash member 0.
    let (ok, text, _) = h.build(&funder, &["member-forfeit", "--committee", &committee, "--member", "0", "--signed-price", &a, "--signed-price", &b]);
    assert!(!ok && text.contains("not a proof against this member"), "{text}");
    // The proof: member 1's 22 KAS are forfeited, 19.8 burned at the last output.
    let (ok, text, _) = h.build(&funder, &["member-forfeit", "--committee", &committee, "--signed-price", &a, "--signed-price", &b]);
    assert!(ok && text.contains("member 1 signed 10 and 10.5 KAS for the time 33500: its 22 KAS are forfeited") && text.contains("burned 19.8 KAS"), "{text}");
    // The member leaves first: the committee no longer holds the key, the exit does, and the same two files slash it.
    let (ok, text, dir) = h.build(&funder, &["committee-leave", "--committee", &committee, "--member", "1", "--key", &members[1]]);
    assert!(ok, "{text}");
    let (left, exit) = (h.obj(&dir, "committee", 30_200), h.obj(&dir, "exit", 30_200));
    let (ok, text, _) = h.build(&funder, &["member-forfeit", "--committee", &left, "--signed-price", &a, "--signed-price", &b]);
    assert!(!ok && text.contains("exit-forfeit"), "{text}");
    let (ok, text, _) = h.build(&funder, &["exit-forfeit", "--exit", &exit, "--signed-price", &a, "--signed-price", &b]);
    assert!(ok && text.contains("its 22 KAS are forfeited") && text.contains("burned 19.8 KAS"), "{text}");
    // Another member's exit is not slashed by it.
    let (ok, text, dir) = h.build(&funder, &["committee-leave", "--committee", &committee, "--member", "0", "--key", &members[0]]);
    assert!(ok, "{text}");
    let exit0 = h.obj(&dir, "exit", 30_200);
    let (ok, text, _) = h.build(&funder, &["exit-forfeit", "--exit", &exit0, "--signed-price", &a, "--signed-price", &b]);
    assert!(!ok && text.contains("not a proof against this exit's key"), "{text}");
}
