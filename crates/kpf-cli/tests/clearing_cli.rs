//! The commands of an account and of the reserve (`shard-*`, `acct-*`, `stated-*`, `draw-note`), offline: every
//! transaction is a dry run on files, built and verified by the tool. One story: a clearing party opens a shard, a
//! user opens a covered account against it and goes long, the price rises, the user's leg leaves by its statement
//! (disputed at its end and answered right on the way), the notes are applied and the shard pays what the pot could
//! not; then the clearing party draws, and a draw stated over a note that is not applied is ended.

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
        let dir = std::env::temp_dir().join(format!("kpf-clearing-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        H { dir, n: 0 }
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
        err
    }
    fn key(&self, name: &str) -> String {
        let path = self.p(&format!("{name}.key.json"));
        self.ok(&["keygen", "--out", &path]);
        path
    }
    fn funding(&mut self, kas: u64) -> String {
        self.n += 1;
        let txid = format!("{:064x}", 0x5eed_0000u64 + self.n as u64);
        let path = self.p(&format!("funding-{}.json", self.n));
        std::fs::write(&path, serde_json::to_string(&json!([{ "txid": txid, "index": 0, "amount": kas * 100_000_000, "daa": 0 }])).unwrap()).unwrap();
        path
    }
    /// A builder funded by `funder` (100,000 KAS, fresh) into a fresh out-dir: the out-dir of its dry run, and what
    /// it printed.
    fn build(&mut self, funder: &str, args: &[&str]) -> (String, String) {
        let fund = self.funding(100_000);
        self.n += 1;
        let out = self.p(&format!("out{}", self.n));
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--funder", funder, "--funding", &fund, "--out-dir", &out]);
        let stdout = self.ok(&a);
        assert!(stdout.contains("not submitted"), "{stdout}");
        (format!("{out}/dry-run"), stdout)
    }
    fn refused(&mut self, funder: &str, args: &[&str]) -> String {
        let fund = self.funding(100_000);
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--funder", funder, "--funding", &fund]);
        self.fail(&a)
    }
    /// The files of `dir` whose names end with `kind`, their DAA set to `daa`.
    fn objs(&self, dir: &str, kind: &str, daa: u64) -> Vec<String> {
        let want = format!("-{}.json", kind.replace(' ', "-"));
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(&want)).collect();
        all.sort();
        for f in &all {
            let mut v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
            v["daa"] = json!(daa);
            std::fs::write(f, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }
        all.iter().map(|f| f.display().to_string()).collect()
    }
    fn obj(&self, dir: &str, kind: &str, daa: u64) -> String {
        self.objs(dir, kind, daa).first().unwrap_or_else(|| panic!("no {kind} in {dir}")).clone()
    }
}

fn tx_file(dir: &str) -> String {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).find(|p| p.to_string_lossy().ends_with("-tx.json")).unwrap_or_else(|| panic!("no transaction in {dir}")).display().to_string()
}
fn read(f: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap()
}
fn outputs(v: &Value) -> &Vec<Value> {
    let at = if v.get("outputs").is_some() { "" } else if v.get("tx").is_some_and(|t| t.get("outputs").is_some()) { "tx" } else { "transaction" };
    let t = if at.is_empty() { v } else { &v[at] };
    t["outputs"].as_array().unwrap_or_else(|| panic!("the file has no outputs"))
}
/// What the transaction of a dry run pays to a key, over all its outputs (sompi).
fn paid(dir: &str, key_file: &str) -> u64 {
    let v = read(&tx_file(dir));
    let k = read(key_file)["publicKey"].as_str().expect("a key file's publicKey").to_string();
    outputs(&v).iter().filter(|o| o["scriptPublicKey"].as_str().is_some_and(|s| s.contains(&k))).map(|o| o["value"].as_u64().unwrap()).sum()
}
const FEED: &str = "kpf feed definition v1\nquantity: example underlying\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\n";
const KAS: u64 = 100_000_000;

#[test]
fn a_shard_a_covered_account_that_is_added_to_and_ended_and_a_draw() {
    let mut h = H::new("story");
    let funder = h.key("funder");
    let (me, exec, watch) = (h.key("me"), h.key("exec"), h.key("watch"));
    let committee_file = h.p("committee.json");
    h.build(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file]);
    let feed = h.p("feed.txt");
    std::fs::write(&feed, FEED).unwrap();
    let market = h.p("market.json");
    h.ok(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]);
    let (lib_dir, _) = h.build(&funder, &["lib-create", "--copies", "1"]);
    let genesis = tx_file(&lib_dir);
    let (bd, _) = h.build(&funder, &["registry-create", "--lib", &genesis, "--copies", "2"]);
    let (registry, anchors) = (tx_file(&bd), h.objs(&bd, "anchor", 33_060));
    let script = |h: &H, name: &str| h.obj(&lib_dir, &format!("script-{name}"), 33_050);

    // The clearing party opens a shard of 50 KAS.
    let (d, out) = h.build(&exec, &["shard-open", "--lib", &genesis, "--anchor", &anchors[0], "--script", &script(&h, "KpfTShardOpen"), "--registry", &registry, "--market1", &market, "--held", "50", "--dispute-daa", "50"]);
    assert!(out.contains("holds 50 KAS in it"), "{out}");
    let shard0 = h.obj(&d, "shard", 33_100);
    let seed = out.split("seed ").nth(1).and_then(|s| s.get(..64)).expect("the shard's seed").to_string();
    // A user opens a covered account against it with 20 KAS of its own. Another key than the shard's owner as side
    // B is refused; and a covered account has nothing of a second party's.
    let open_c = script(&h, "KpfTOpenC");
    let open = |b: &str, more: &[&str]| -> Vec<String> {
        let mut v = vec!["acct-open", "--shard", &shard0, "--anchor", &anchors[1], "--script", &open_c, "--registry", &registry, "--seed", &seed, "--market1", &market, "--key", b, "--a", &me, "--b", b, "--a-margin", "20", "--dispute-daa", "50"];
        v.extend(more);
        v.into_iter().map(String::from).collect()
    };
    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }
    let v = open(&watch, &[]);
    let err = h.refused(&me, &strs(&v));
    assert!(err.contains("is not the owner of --shard"), "{err}");
    let v = open(&exec, &["--b-margin", "2"]);
    let err = h.refused(&me, &strs(&v));
    assert!(err.contains("b-margin"), "{err}");
    let v = open(&exec, &[]);
    let (d, _) = h.build(&me, &strs(&v));
    let account0 = h.obj(&d, "covered account", 33_110);
    // The user adds 3 KAS; the clearing party adds to its shard, not to the account.
    let err = h.refused(&exec, &["acct-add", "--account", &account0, "--script", &script(&h, "KpfTAddC"), "--side", "b", "--amount", "3"]);
    assert!(err.contains("nothing of a second party's"), "{err}");
    let (d, out) = h.build(&me, &["acct-add", "--account", &account0, "--script", &script(&h, "KpfTAddC"), "--amount", "3"]);
    assert!(out.contains("23 KAS"), "{out}");
    let account = h.obj(&d, "covered account", 33_120);
    // (The statements of the old form are not made on a covered account: it has kinds of its own, whose commands
    // are not here yet. docs/ARCHITECTURE-CLEARING.md 14.8.)
    let err = h.refused(&me, &["acct-take", "--account", &account, "--script", &script(&h, "KpfTChange"), "--side", "a", "--prices", "13", "--t0", "33150", "--amount", "1"]);
    assert!(err.contains("kinds of its own"), "{err}");
    // The user ends the account, which has no leg: its 23 KAS.
    let (d, out) = h.build(&me, &["acct-end", "--account", &account, "--script", &script(&h, "KpfTAcctEnd"), "--side", "a"]);
    assert!(out.contains("ended: 23 KAS to its user"), "{out}");
    assert!(paid(&d, &me) >= 23 * KAS);

    // The clearing party draws what the shard holds above its requirement (no leg is in its books: all of its 50
    // KAS), by a statement that is applied after the dispute period.
    let (d, out) = h.build(&exec, &["shard-draw", "--shard", &shard0, "--script", &script(&h, "KpfTChange"), "--prices", "0,0,0,0", "--t0", "33330"]);
    assert!(out.contains("draws 50 KAS"), "{out}");
    let draw = h.obj(&d, "stated draw", 33_340);
    let out = h.ok(&["acct-show", "--object", &draw, "--prices", "0,0,0,0"]);
    assert!(out.contains("the statement is the rule's"), "{out}");
    let (d, _) = h.build(&watch, &["stated-settle", "--stated", &draw, "--script", &script(&h, "KpfTChangeSettle"), "--daa", "33500"]);
    assert_eq!(paid(&d, &exec), 50 * KAS + 5 * KAS, "the amount and the bond (a tenth of what the shard held)");
    // Another key than the owner's states no draw the tool builds.
    let err = h.refused(&watch, &["shard-draw", "--shard", &shard0, "--script", &script(&h, "KpfTChange"), "--prices", "0,0,0,0", "--t0", "33330", "--amount", "1"]);
    assert!(!err.is_empty());
}
