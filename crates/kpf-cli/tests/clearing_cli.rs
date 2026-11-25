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
/// The note of `dir` whose state says `what`.
fn note(h: &H, dir: &str, what: &str, daa: u64) -> String {
    h.objs(dir, "shard note", daa).into_iter().find(|f| read(f)["state"].as_str().is_some_and(|s| s.contains(what))).unwrap_or_else(|| panic!("no note with {what} in {dir}"))
}

const FEED: &str = "kpf feed definition v1\nquantity: example underlying\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\n";
const KAS: u64 = 100_000_000;

#[test]
fn a_shard_a_covered_account_a_leg_that_leaves_and_a_draw() {
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
    // A user opens a covered account against it: 20 KAS of its own, 2 of the clearing party's. Another key than
    // the shard's owner as side B is refused.
    let err = h.refused(&me, &["acct-open", "--shard", &shard0, "--anchor", &anchors[1], "--script", &script(&h, "KpfTOpenC"), "--registry", &registry, "--key", &watch, "--a", &me, "--b", &watch, "--a-margin", "20", "--b-margin", "2", "--dispute-daa", "50"]);
    assert!(err.contains("is not the owner of --shard"), "{err}");
    let (d, _) = h.build(&me, &["acct-open", "--shard", &shard0, "--anchor", &anchors[1], "--script", &script(&h, "KpfTOpenC"), "--registry", &registry, "--key", &exec, "--a", &me, "--b", &exec, "--a-margin", "20", "--b-margin", "2", "--dispute-daa", "50"]);
    let account0 = h.obj(&d, "covered account", 33_110);
    // The user goes long 10 units at 10; the note that the leg entered is applied to the shard.
    let (d, out) = h.build(&me, &["acct-leg", "--account", &account0, "--script", &script(&h, "KpfTLegAdd"), "--key", &exec, "--market", &market, "--size", "10", "--dir", "long", "--x", "10"]);
    assert!(out.contains("leg 1: A is long 10 units"), "{out}");
    let (account, join) = (h.obj(&d, "covered account", 33_120), h.obj(&d, "shard note", 33_120));
    let (d, out) = h.build(&watch, &["shard-note", "--shard", &shard0, "--note", &join, "--script", &script(&h, "KpfTNoteApply")]);
    assert!(out.contains("row 1") && out.contains("net -10 units, gross 10"), "{out}");
    let shard1 = h.obj(&d, "shard", 33_130);
    let out = h.ok(&["acct-show", "--object", &shard1, "--prices", "13,0,0,0"]);
    assert!(out.contains("KAS can be drawn"), "{out}");

    // The way out that waits for nobody: a second leg, short 4, enters and leaves at once by the two signatures, at
    // its entry (nothing moves between the margins). Each writes its note, and the shard counts as before.
    let (d, _) = h.build(&me, &["acct-leg", "--account", &account, "--script", &script(&h, "KpfTLegAdd"), "--key", &exec, "--market", &market, "--size", "4", "--dir", "short", "--x", "10"]);
    let (account2, join2) = (h.obj(&d, "covered account", 33_132), h.obj(&d, "shard note", 33_132));
    let (d, out) = h.build(&watch, &["shard-note", "--shard", &shard1, "--note", &join2, "--script", &script(&h, "KpfTNoteApply")]);
    assert!(out.contains("net -6 units, gross 14"), "{out}");
    let shard1 = h.obj(&d, "shard", 33_134);
    let (d, out) = h.build(&me, &["acct-out", "--account", &account2, "--script", &script(&h, "KpfTLegOut"), "--key", &exec, "--leg", "2", "--x", "10"]);
    assert!(out.contains("leaves at once") && !out.contains("leg 2:"), "{out}");
    let (account, release2) = (h.obj(&d, "covered account", 33_136), h.obj(&d, "shard note", 33_136));
    let (d, out) = h.build(&watch, &["shard-note", "--shard", &shard1, "--note", &release2, "--script", &script(&h, "KpfTNoteApply")]);
    assert!(out.contains("net -10 units, gross 10"), "{out}");
    let shard1 = h.obj(&d, "shard", 33_138);

    // The price is 13: the user states that its leg leaves. The leg gained 30 and the clearing side's margin is 2:
    // the pot pays 2 of the gain and the shard owes 28.
    let (d, out) = h.build(&me, &["acct-leave", "--account", &account, "--script", &script(&h, "KpfTChange"), "--side", "a", "--leg", "1", "--prices", "13", "--t0", "33150"]);
    assert!(out.contains("28 KAS are owed by the shard"), "{out}");
    let stated = h.obj(&d, "statement", 33_160);
    let out = h.ok(&["acct-show", "--object", &stated, "--prices", "13"]);
    assert!(out.contains("the statement is the rule's"), "{out}");
    let out = h.ok(&["acct-show", "--object", &stated, "--prices", "11"]);
    assert!(out.contains("WRONG at the point 2"), "{out}");
    // A dispute at the committee's prices is refused where the statement is the rule's; one that names a point is
    // made, and answered right: the statement stands again.
    let err = h.refused(&watch, &["stated-dispute", "--stated", &stated, "--script", &script(&h, "KpfTPoint"), "--prices", "13", "--daa", "33170"]);
    assert!(err.contains("the statement is the rule's"), "{err}");
    let (d, out) = h.build(&watch, &["stated-dispute", "--stated", &stated, "--script", &script(&h, "KpfTPoint"), "--point", "0", "--daa", "33170"]);
    assert!(out.contains("DISPUTED at the point 0"), "{out}");
    let pointed = h.obj(&d, "disputed statement", 33_170);
    let (d, out) = h.build(&me, &["stated-answer", "--stated", &pointed, "--script", &script(&h, "KpfTStepLiq"), "--daa", "33180"]);
    assert!(out.contains("the point 0 is RIGHT"), "{out}");
    let again = h.obj(&d, "statement", 33_180);
    // (Unanswered, the dispute lapses: the account again.)
    let (d, _) = h.build(&watch, &["stated-lapse", "--stated", &pointed, "--script", &script(&h, "KpfTPoint"), "--daa", "33300"]);
    assert_eq!(read(&h.obj(&d, "covered account", 33_300))["clearing"], read(&account)["clearing"]);
    // Applied after the dispute period: the account with no leg, the note that the leg left, the note of 28 owed.
    let (d, out) = h.build(&watch, &["stated-settle", "--stated", &again, "--script", &script(&h, "KpfTChangeSettle"), "--daa", "33300"]);
    assert!(out.contains("it writes notes to the shard"), "{out}");
    let ended = h.obj(&d, "covered account", 33_300);
    let (release, owed) = (note(&h, &d, "kind: 2", 33_300), note(&h, &d, "kind: 4", 33_300));
    let (d, _) = h.build(&watch, &["shard-note", "--shard", &shard1, "--note", &release, "--script", &script(&h, "KpfTNoteApply")]);
    let shard2 = h.obj(&d, "shard", 33_310);
    let (d, _) = h.build(&watch, &["shard-note", "--shard", &shard2, "--note", &owed, "--script", &script(&h, "KpfTNoteApply")]);
    assert_eq!(paid(&d, &me), 28 * KAS, "the shard pays the user what the pot could not");
    let shard3 = h.obj(&d, "shard", 33_320);
    // The user ends the account: its 22 KAS.
    let (d, _) = h.build(&me, &["acct-end", "--account", &ended, "--script", &script(&h, "KpfTAcctEnd"), "--side", "a"]);
    assert!(paid(&d, &me) >= 22 * KAS);

    // The clearing party draws what the shard holds above its requirement (no leg is left: all of its 22 KAS), by
    // a statement that is applied after the dispute period.
    let (d, out) = h.build(&exec, &["shard-draw", "--shard", &shard3, "--script", &script(&h, "KpfTChange"), "--prices", "0,0,0,0", "--t0", "33330"]);
    assert!(out.contains("draws 22 KAS"), "{out}");
    let draw = h.obj(&d, "stated draw", 33_340);
    let out = h.ok(&["acct-show", "--object", &draw, "--prices", "0,0,0,0"]);
    assert!(out.contains("the statement is the rule's"), "{out}");
    let (d, _) = h.build(&watch, &["stated-settle", "--stated", &draw, "--script", &script(&h, "KpfTChangeSettle"), "--daa", "33500"]);
    assert_eq!(paid(&d, &exec), (22 + 2) * KAS + 2 * KAS / 10, "the amount and the bond (a tenth of what the shard held)");
    // Another key than the owner's states no draw the tool builds.
    let err = h.refused(&watch, &["shard-draw", "--shard", &shard3, "--script", &script(&h, "KpfTChange"), "--prices", "0,0,0,0", "--t0", "33330", "--amount", "1"]);
    assert!(!err.is_empty());

    // A draw stated on the shard as it was before the leg's note was applied is ended by that note.
    let (d, _) = h.build(&exec, &["shard-draw", "--shard", &shard0, "--script", &script(&h, "KpfTChange"), "--prices", "0,0,0,0", "--t0", "33330", "--amount", "5"]);
    let early = h.obj(&d, "stated draw", 33_340);
    let (d, out) = h.build(&watch, &["draw-note", "--stated", &early, "--note", &join, "--script", &script(&h, "KpfTDrawNote")]);
    assert!(out.contains("ended: the shard again"), "{out}");
    // (Half of the owner's bond, a tenth of the 50 KAS the shard held, to whoever showed the note; its coin's change beside it.)
    assert!(out.contains("of the claimer's bond, 2.5 KAS to the funder"), "{out}");
    assert!(paid(&d, &watch) >= 5 * KAS / 2);
    assert_eq!(read(&h.obj(&d, "shard", 33_350))["clearing"], read(&shard0)["clearing"]);
}
