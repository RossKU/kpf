//! Review T1b, round 2, the command line (`lib-*`, `pos-*`): a library's genesis file that is not a sound library
//! is refused by `lib-check` and by `pos-open`; what round 2 measured in the engine and through the builders (a claim
//! of a signed price long past loses its bond; a side that liquidates and disputes itself has both bonds back) is what the
//! tool builds and verifies. Offline: every transaction is a dry run on files. A test named `t1b_finding_...` asserts
//! an acceptance and documents it (docs/audit/REVIEW-T1b.md).

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
        let dir = std::env::temp_dir().join(format!("kpf-t1b-cli-{tag}-{}", std::process::id()));
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
    /// A builder funded by `funder` (100,000 KAS, fresh) into a fresh out-dir; the out-dir of its dry run.
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
    /// A builder that must be refused, funded as `build` funds one.
    fn refused(&mut self, funder: &str, args: &[&str]) -> String {
        let fund = self.funding(100_000);
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--funder", funder, "--funding", &fund]);
        self.fail(&a)
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
}

/// The transaction file a dry run wrote.
fn tx_file(dir: &str) -> String {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).find(|p| p.to_string_lossy().ends_with("-tx.json")).unwrap_or_else(|| panic!("no transaction in {dir}")).display().to_string()
}
fn read(f: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap()
}
/// The outputs of a transaction file, wherever the file keeps the transaction.
fn outputs(v: &mut Value) -> &mut Vec<Value> {
    let at = if v.get("outputs").is_some() { "" } else if v.get("tx").is_some_and(|t| t.get("outputs").is_some()) { "tx" } else { "transaction" };
    let t = if at.is_empty() { v } else { &mut v[at] };
    t["outputs"].as_array_mut().unwrap_or_else(|| panic!("the file has no outputs"))
}
fn pubkey(key_file: &str) -> String {
    read(key_file)["publicKey"].as_str().expect("a key file's publicKey").to_string()
}
/// What the transaction of a dry run pays to a key, over all its outputs (sompi).
fn paid(dir: &str, key_file: &str) -> u64 {
    let mut v = read(&tx_file(dir));
    let k = pubkey(key_file);
    outputs(&mut v).iter().filter(|o| o["scriptPublicKey"].as_str().is_some_and(|s| s.contains(&k))).map(|o| o["value"].as_u64().unwrap()).sum()
}

const FEED: &str = "kpf feed definition v1\nquantity: example underlying\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\ndisagreement: a signed price is true if within 0.5 % of the median\n";
const KAS: u64 = 100_000_000;

/// A market (its committee only created: nothing here needs a signed price), a library of one copy, a registry of it, and
/// a position, entered through an anchor of the registry by both keys, of
/// `me` (long 10 units at 10, 40 KAS) against `exec` (60 KAS), dispute period 50, bond a tenth: 10 KAS.
struct Setup {
    h: H,
    me: String,
    exec: String,
    market: String,
    lib_dir: String,
    genesis: String,
    position: String,
    /// Round 8: the file of the registry's genesis, an anchor of it, and the library's copy of KpfTPlace.
    registry: String,
    anchor: String,
    place: String,
    /// Round 9: the library's copy of KpfTOpen (a position enters by it; an order by KpfTPlace).
    open: String,
}
fn setup(tag: &str) -> Setup {
    let mut h = H::new(tag);
    let funder = h.key("funder");
    let (me, exec) = (h.key("me"), h.key("exec"));
    let committee_file = h.p("committee.json");
    h.build(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file]);
    let feed = h.p("feed.txt");
    std::fs::write(&feed, FEED).unwrap();
    let market = h.p("market.json");
    h.ok(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]);
    let lib_dir = h.build(&funder, &["lib-create", "--copies", "1"]);
    let genesis = tx_file(&lib_dir);
    let bd = h.build(&funder, &["registry-create", "--lib", &genesis, "--copies", "2"]);
    let (registry, anchor) = (tx_file(&bd), h.obj(&bd, "anchor", 33_060));
    let place_s = h.obj(&lib_dir, "script-KpfTPlace", 33_050);
    let open_s = h.obj(&lib_dir, "script-KpfTOpen", 33_050);
    let d = h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "10", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33100"]);
    let position = h.obj(&d, "position", 33_100);
    Setup { h, me, exec, market, lib_dir, genesis, position, registry, anchor, place: place_s, open: open_s }
}
impl Setup {
    fn script(&self, name: &str) -> String {
        self.h.obj(&self.lib_dir, &format!("script-{name}"), 33_050)
    }
}

/// Round 1, F4, at the command line: `lib-check` and `pos-open` read the genesis file and refuse one that writes
/// anything but the family's scripts under the id, in whatever way the file is changed.
#[test]
fn t1b_fixed_the_command_line_opens_no_position_in_a_library_it_cannot_check() {
    let mut s = setup("lib");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let good = s.h.ok(&["lib-check", "--lib", &genesis]);
    assert!(good.contains("its genesis writes the family's scripts and nothing else"), "{good}");
    let base = read(&genesis);
    let mut v = base.clone();
    let outs = outputs(&mut v).clone();
    let scripts: Vec<usize> = (0..outs.len()).filter(|i| !outs[*i]["covenant"].is_null()).collect();
    let plain: Vec<usize> = (0..outs.len()).filter(|i| outs[*i]["covenant"].is_null()).collect();
    // (Round 31, F55 fix: a one-copy library writes a second KpfTCite, so one more script output than the count.)
    assert_eq!(scripts.len(), kpf_protocol::thin_artifacts::SCRIPTS.len() + 1, "one copy of each of the scripts, and a second KpfTCite");
    assert!(!plain.is_empty(), "the funder's change");
    let cov = outs[scripts[0]]["covenant"].clone();
    let key_spk = outs[plain[0]]["scriptPublicKey"].clone();
    let mut cases: Vec<(&str, Value)> = vec![];
    let mut case = |what: &'static str, f: &dyn Fn(&mut Vec<Value>)| {
        let mut v = base.clone();
        f(outputs(&mut v));
        cases.push((what, v));
    };
    // One output more under the id: a key's (whoever holds the key takes every object of the library).
    case("a key's output more under the id", &|o| o.push(json!({ "value": 10 * KAS, "scriptPublicKey": key_spk, "verboseData": null, "covenant": cov })));
    // The funder's change put under the id.
    case("the change under the id", &|o| o[plain[0]]["covenant"] = cov.clone());
    // A script's place taken by a key's output.
    case("a key's output in a script's place", &|o| o[scripts[3]]["scriptPublicKey"] = key_spk.clone());
    // A copy more of a script (the id is no longer that of the outputs).
    case("a copy more of a script", &|o| { let c = o[scripts[0]].clone(); o.push(c); });
    // A script short.
    case("a script short", &|o| { o.remove(scripts[5]); });
    // Two scripts in each other's place (the id is of the outputs at their indices).
    case("two scripts in each other's place", &|o| o.swap(scripts[0], scripts[1]));
    // Another id on every output; another id on one; another authorizing input on one, on all.
    let other = "5a".repeat(32);
    case("another id on every output", &|o| scripts.iter().for_each(|i| o[*i]["covenant"]["covenantId"] = json!(other)));
    case("another id on one output", &|o| o[scripts[2]]["covenant"]["covenantId"] = json!(other));
    case("another authorizing input on one output", &|o| o[scripts[2]]["covenant"]["authorizingInput"] = json!(1));
    case("an authorizing input that is not there", &|o| scripts.iter().for_each(|i| o[*i]["covenant"]["authorizingInput"] = json!(7)));
    // A script holding another value (the id is of the outputs' values too).
    case("a script of another value", &|o| o[scripts[0]]["value"] = json!(o[scripts[0]]["value"].as_u64().unwrap() + 1));
    // No output under an id at all.
    case("nothing under an id", &|o| o.iter_mut().for_each(|x| x["covenant"] = Value::Null));
    for (i, (what, v)) in cases.iter().enumerate() {
        let f = s.h.p(&format!("bad-genesis-{i}.json"));
        std::fs::write(&f, serde_json::to_string(v).unwrap()).unwrap();
        let (ok, out, err) = s.h.run(&["lib-check", "--lib", &f]);
        assert!(!ok, "lib-check takes {what}:\n{out}");
        assert!(!out.contains("nothing else"), "{what}: {out}");
        let err2 = s.h.refused(&me, &["pos-open", "--lib", &f, "--anchor", &s.anchor.clone(), "--script", &s.open.clone(), "--registry", &s.registry.clone(), "--key", &exec, "--market1", &market, "--size1", "10", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--daa", "33100"]);
        println!("T1b F4 cli: {what}: {}", err.lines().next().unwrap_or("").chars().take(150).collect::<String>());
        assert!(!err2.is_empty());
    }
    // An operation's transaction (it writes a script of the library back beside the position) is not a library.
    let s_add = s.script("KpfTAdd");
    let d = s.h.build(&me, &["pos-add-margin", "--position", &s.position, "--script", &s_add, "--side", "a", "--amount", "5"]);
    s.h.fail(&["lib-check", "--lib", &tx_file(&d)]);
    // A file that is no transaction.
    let junk = s.h.p("junk.json");
    std::fs::write(&junk, "{}").unwrap();
    s.h.fail(&["lib-check", "--lib", &junk]);
    println!("T1b F4 cli: {} genesis files that are not a library refused by lib-check and by pos-open", cases.len());
}

/// Round 1, F3, at the command line: a liquidation and a claim of a signed price long past are ended by a third key, which
/// has half the bond; the other half goes to the side named (the party the claim was made against); the maker has
/// nothing back.
#[test]
fn t1b_fixed_the_command_line_shares_the_bond_of_a_claim_not_in_time() {
    let mut s = setup("late");
    let (me, exec, position) = (s.me.clone(), s.exec.clone(), s.position.clone());
    let (liquidator, ender) = (s.h.key("liquidator"), s.h.key("ender"));
    let bond = 10 * KAS;
    // A liquidation of the time 100, landing at 33,510.
    let d = s.h.build(&liquidator, &["pos-liquidate", "--position", &position, "--script", &s.script("KpfTLiq"), "--side", "a", "--t0", "100", "--p1", "10", "--fee", "1", "--add", "5"]);
    let lq = s.h.obj(&d, "liquidation", 33_510);
    assert!(s.h.refused(&ender, &["pos-settle", "--claim", &lq, "--script", &s.script("KpfTLiqSettle")]).contains("late"));
    assert!(s.h.refused(&ender, &["pos-dispute", "--claim", &lq, "--script", &s.script("KpfTLiqDispute")]).contains("late"));
    let d = s.h.build(&ender, &["pos-late", "--claim", &lq, "--script", &s.h.obj(&d, "script-KpfTLiq", 33_510)]);
    assert_eq!(paid(&d, &liquidator), 0, "nothing back to the liquidator");
    assert_eq!(paid(&d, &me), 0, "nothing to the side named (round 3: that half stays in the script)");
    assert_eq!(paid(&d, &exec), 0);
    let to_ender = paid(&d, &ender);
    assert!(to_ender > 100_000 * KAS && to_ender < 100_000 * KAS + bond / 2, "the ender: its funding less the fee, and half the bond: {to_ender}");
    // A declared claim of the time 100 by the user.
    let d = s.h.build(&me, &["pos-close", "--position", &position, "--script", &s.script("KpfTClose"), "--side", "a", "--t0", "100", "--p1", "10"]);
    let claim = s.h.obj(&d, "claim", 33_510);
    assert!(s.h.refused(&ender, &["pos-settle", "--claim", &claim, "--script", &s.script("KpfTSettle")]).contains("late"));
    let d = s.h.build(&ender, &["pos-late", "--claim", &claim, "--script", &s.script("KpfTLate")]);
    assert_eq!(paid(&d, &me), 0, "nothing back to the claimer");
    assert_eq!(paid(&d, &exec), bond / 2, "half to the party the claim was made against");
    println!("T1b F3 cli: {}", s.h.last.lines().filter(|l| l.contains("bond") || l.contains("pays")).collect::<Vec<_>>().join(" | "));
}

/// FINDING (Medium, F32) at the command line; F2b of round 2, fixed in round 3, open again by the rule of round 23.
/// The user liquidates its own side with a second key, falsely, disputes that with a third, and after a dispute
/// period the lapse is run: both keys have their bonds back, and the position was held for the fees. While the claim
/// stood the tool built nothing else on the position.
#[test]
fn t1b_finding_f32_the_command_line_self_liquidation_and_self_dispute_costs_the_fees() {
    let mut s = setup("hold");
    let (me, exec, position) = (s.me.clone(), s.exec.clone(), s.position.clone());
    let (second, third, watcher) = (s.h.key("second"), s.h.key("third"), s.h.key("watcher"));
    let bond = 10 * KAS;
    let d = s.h.build(&second, &["pos-liquidate", "--position", &position, "--script", &s.script("KpfTLiq"), "--side", "a", "--t0", "33500", "--p1", "10", "--fee", "1", "--add", "5"]);
    assert!(s.h.last.contains("CHECK: FALSE"), "{}", s.h.last);
    let lq = s.h.obj(&d, "liquidation", 33_510);
    let d = s.h.build(&third, &["pos-dispute", "--claim", &lq, "--script", &s.script("KpfTLiqDispute")]);
    let (dl, s_ldis) = (s.h.obj(&d, "disputed liquidation", 33_510), s.h.obj(&d, "script-KpfTLiqDispute", 33_510));
    // Nothing else is built on it: no second dispute, no cure, no settlement, no end as late; and nothing on the
    // position, which is not there.
    s.h.refused(&watcher, &["pos-dispute", "--claim", &dl, "--script", &s_ldis]);
    assert!(s.h.refused(&me, &["pos-cure", "--claim", &dl, "--script", &s.script("KpfTCure")]).contains("disputed"));
    s.h.refused(&watcher, &["pos-settle", "--claim", &dl, "--script", &s.script("KpfTLiqSettle")]);
    s.h.refused(&watcher, &["pos-late", "--claim", &dl, "--script", &s.script("KpfTLiq")]);
    s.h.refused(&exec, &["pos-close", "--position", &dl, "--script", &s.script("KpfTClose"), "--side", "b", "--t0", "33500", "--p1", "10"]);
    s.h.refused(&watcher, &["pos-liquidate", "--position", &dl, "--script", &s.script("KpfTLiq"), "--side", "a", "--t0", "33500", "--p1", "5.5"]);
    // A dispute period later, anyone runs the lapse.
    let d = s.h.build(&watcher, &["pos-lapse", "--claim", &dl, "--script", &s_ldis]);
    assert!(s.h.last.contains("the position again"), "{}", s.h.last);
    assert_eq!((paid(&d, &third), paid(&d, &second)), (bond, bond), "FINDING F32 (if this fails the finding is fixed): both bonds back");
    assert_eq!(paid(&d, &me) + paid(&d, &exec), 0);
    println!("T1b FINDING F32 cli: self-liquidation and self-dispute: bonds posted 2 x {bond}, back {}: nothing lost but the fees", paid(&d, &third) + paid(&d, &second));
}

/// Round 3: the commands round 2 did not reach that need no signed price (`pos-exit`, its settlement and its early
/// end, `pos-close --mutual`), and the tool's refusal of an object file whose UTXO holds less than its state says.
#[test]
fn t1b_r3_the_command_line_exit_close_both_and_an_object_that_holds_too_little() {
    let mut s = setup("exit");
    let (me, exec, position) = (s.me.clone(), s.exec.clone(), s.position.clone());
    let (second, watcher) = (s.h.key("second"), s.h.key("watcher"));
    let bond = 10 * KAS;
    // "No price since 33,600", landing at 33,650: settled at the entries after the exit time by anyone.
    let d = s.h.build(&me, &["pos-exit", "--position", &position, "--script", &s.script("KpfTExit"), "--side", "a", "--since", "33600"]);
    let q = s.h.obj(&d, "exit claim", 33_650);
    let d = s.h.build(&watcher, &["pos-settle", "--claim", &q, "--script", &s.script("KpfTExitEnd")]);
    assert_eq!(paid(&d, &me), 40 * KAS + bond, "the claimer: its margin and its bond");
    assert!(paid(&d, &exec) >= 60 * KAS, "the other party: its margin");
    // The same claim landing before its own `since`: ended by the user's second key, half the bond to the other party.
    let early = s.h.obj(&d_of(&q), "exit claim", 33_550);
    s.h.refused(&watcher, &["pos-settle", "--claim", &early, "--script", &s.script("KpfTExitEnd")]);
    let d = s.h.build(&second, &["pos-late", "--claim", &early, "--script", &s.script("KpfTExit")]);
    assert_eq!(paid(&d, &exec), bond / 2, "half to the party held");
    assert_eq!(paid(&d, &me), 0);
    // The two parties close together, paid by a third key; one signature is not enough.
    s.h.refused(&watcher, &["pos-close", "--mutual", "--position", &position, "--script", &s.script("KpfTBoth"), "--to-a", "45", "--key", &me]);
    let d = s.h.build(&watcher, &["pos-close", "--mutual", "--position", &position, "--script", &s.script("KpfTBoth"), "--to-a", "45", "--key", &me, "--key", &exec]);
    assert_eq!(paid(&d, &me), 45 * KAS);
    assert!(paid(&d, &exec) >= 55 * KAS);
    // A position file whose UTXO holds less than the two margins: refused on read (round 2's note).
    let mut v = read(&position);
    let held = v["amount"].as_u64().unwrap_or_else(|| panic!("an object file amount: {:?}", v.as_object().map(|o| o.keys().collect::<Vec<_>>())));
    assert!(held >= 100 * KAS);
    v["amount"] = json!(100 * KAS - 1);
    let poor = s.h.p("poor-position.json");
    std::fs::write(&poor, serde_json::to_string(&v).unwrap()).unwrap();
    let err = s.h.fail(&["pos-show", "--object", &poor]);
    s.h.refused(&me, &["pos-close", "--position", &poor, "--script", &s.script("KpfTClose"), "--side", "a", "--t0", "33500", "--p1", "10"]);
    v["amount"] = json!(100 * KAS);
    std::fs::write(&poor, serde_json::to_string(&v).unwrap()).unwrap();
    s.h.ok(&["pos-show", "--object", &poor]);
    println!("T1b R3 cli: a position holding a sompi less than its two margins: {}", err.lines().next().unwrap_or("").chars().take(200).collect::<String>());
}

/// The directory of a file.
fn d_of(file: &str) -> String {
    PathBuf::from(file).parent().unwrap().display().to_string()
}

/// Every object file of `dir` whose name ends with `kind`, its DAA set to `daa`.
fn objs(dir: &str, kind: &str, daa: u64) -> Vec<String> {
    let want = format!("-{}.json", kind.replace(' ', "-"));
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(&want)).collect();
    all.sort();
    all.iter()
        .map(|f| {
            let mut v = read(&f.display().to_string());
            v["daa"] = json!(daa);
            std::fs::write(f, serde_json::to_string_pretty(&v).unwrap()).unwrap();
            f.display().to_string()
        })
        .collect()
}
/// The output of a dry run's transaction that an object file of it names: its index, checked to hold the file's
/// amount under the file's covenant id.
fn output_of(dir: &str, file: &str) -> usize {
    let f = read(file);
    let op = &f["outpoint"];
    let index = op.get("index").and_then(Value::as_u64).or_else(|| op.as_str().and_then(|s| s.rsplit(':').next()).and_then(|s| s.parse().ok())).unwrap_or_else(|| panic!("an object file's outpoint: {op}")) as usize;
    let mut t = read(&tx_file(dir));
    let o = outputs(&mut t)[index].clone();
    assert_eq!(o["value"].as_u64(), f["amount"].as_u64(), "{file}: the amount of output {index}");
    assert_eq!(o["covenant"]["covenantId"].as_str(), f["covenantId"].as_str(), "{file}: the covenant id of output {index}");
    index
}

/// Round 4, O7 and the finding of O5 at the command line: orders placed, shown, matched in part (the files of what
/// rests name the right outputs), matched whole, taken back, returned; an order file that is not its script's state
/// refused; and an order past its expiry matched as any other.
#[test]
fn t1b_o7_orders_at_the_command_line() {
    let mut s = setup("orders");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let matcher = s.h.key("matcher");
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let open_s = s.open.clone();
    let place = |s: &mut Setup, who: &str, side: &str, size: &str, limit: &str, margin: &str, expiry: &str, daa: u64, more: &[&str]| -> String {
        let mut a = vec!["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", side, "--size", size, "--limit", limit, "--margin", margin, "--tip", "1", "--expiry", expiry];
        a.extend(more);
        let d = s.h.build(who, &a);
        s.h.obj(&d, "order", daa)
    };
    let sell = place(&mut s, &exec, "short", "10", "10", "200", "40000", 33_900, &[]);
    let buy = place(&mut s, &me, "long", "4", "10.1", "80", "40000", 33_900, &[]);
    let shown = s.h.ok(&["pos-show", "--object", &sell]);
    assert!(shown.to_lowercase().contains("order"), "{shown}");
    // 3 of the buy's 4 against the sell's 10: a position, and two orders that rest, each file naming its output.
    let d = s.h.build(&matcher, &["order-match", "--long", &buy, "--short", &sell, "--script", &s.script("KpfTMatch"), "--size", "3", "--price", "10"]);
    let position = s.h.obj(&d, "position", 33_950);
    let rests = objs(&d, "order", 33_950);
    assert_eq!(rests.len(), 2, "what rests of each order");
    let (ia, i1, i2) = (output_of(&d, &position), output_of(&d, &rests[0]), output_of(&d, &rests[1]));
    assert!(ia != i1 && ia != i2 && i1 != i2);
    let (a, r1, r2) = (read(&position), read(&rests[0]), read(&rests[1]));
    // Round 8: the two orders, the position and what rests of each are of one id, the registry's; under it the position is
    // written first, then what rests of the long order, then what rests of the short.
    let long_id = read(&buy)["covenantId"].clone();
    assert_eq!(read(&sell)["covenantId"], long_id, "two orders placed through one anchor are of one registry");
    let (long_rest, short_rest) = if i1 < i2 { (rests[0].clone(), rests[1].clone()) } else { (rests[1].clone(), rests[0].clone()) };
    assert_eq!((a["covenantId"].clone(), r1["covenantId"].clone(), r2["covenantId"].clone()), (long_id.clone(), long_id.clone(), long_id.clone()));
    assert!(output_of(&d, &position) < output_of(&d, &long_rest) && output_of(&d, &long_rest) < output_of(&d, &short_rest), "the position, the long's rest, the short's rest");
    assert_eq!(a["amount"].as_u64(), Some(120 * KAS), "the position: 3/4 of 80 KAS and 3/10 of 200");
    assert_eq!(read(&long_rest)["amount"].as_u64(), Some(20 * KAS + KAS / 4));
    assert_eq!(read(&short_rest)["amount"].as_u64(), Some(140 * KAS + 7 * KAS / 10));
    assert_eq!((paid(&d, &me), paid(&d, &exec)), (0, 0), "both orders rest: nothing back to their owners");
    for f in [&position, &long_rest, &short_rest] {
        s.h.ok(&["pos-show", "--object", f]);
    }
    // The position of a match is a position: its owner adds margin to it while the order of the same id rests.
    let d2 = s.h.build(&me, &["pos-add-margin", "--position", &position, "--script", &s.script("KpfTAdd"), "--side", "a", "--amount", "5"]);
    assert_eq!(read(&s.h.obj(&d2, "position", 34_000))["covenantId"], long_id);
    // What rests of the buy is matched whole against what rests of the sell: the buy is gone, the sell rests on.
    let s_match = s.h.obj(&d, "script-KpfTMatch", 33_950);
    let d3 = s.h.build(&matcher, &["order-match", "--long", &long_rest, "--short", &short_rest, "--script", &s_match, "--size", "1", "--price", "10"]);
    let rests3 = objs(&d3, "order", 34_000);
    assert_eq!(rests3.len(), 1);
    output_of(&d3, &rests3[0]);
    assert_eq!(read(&rests3[0])["covenantId"], read(&sell)["covenantId"]);
    // More than is left, a price beyond a limit, the two orders in each other's place: refused.
    s.h.refused(&matcher, &["order-match", "--long", &long_rest, "--short", &short_rest, "--script", &s_match, "--size", "2", "--price", "10"]);
    s.h.refused(&matcher, &["order-match", "--long", &long_rest, "--short", &short_rest, "--script", &s_match, "--size", "1", "--price", "10.2"]);
    s.h.refused(&matcher, &["order-match", "--long", &long_rest, "--short", &short_rest, "--script", &s_match, "--size", "1", "--price", "9.9"]);
    s.h.refused(&matcher, &["order-match", "--long", &short_rest, "--short", &long_rest, "--script", &s_match, "--size", "1", "--price", "10"]);
    // Taken back by its owner (paid by another key: the whole value to the owner), by nobody else; returned by anyone.
    let s_cancel = s.script("KpfTCancel");
    let left = read(&rests3[0])["amount"].as_u64().unwrap();
    s.h.refused(&matcher, &["order-cancel", "--order", &rests3[0], "--script", &s_cancel]);
    let d4 = s.h.build(&matcher, &["order-cancel", "--order", &rests3[0], "--script", &s_cancel, "--key", &exec]);
    assert_eq!(paid(&d4, &exec), left, "the order's whole value to its owner");
    let d5 = s.h.build(&matcher, &["order-expire", "--order", &rests3[0], "--script", &s_cancel]);
    assert_eq!(paid(&d5, &exec), left);
    // An order file whose state is another order's, or holds less than its margin and tip: refused on read.
    let mut v = read(&long_rest);
    v["thin"] = read(&short_rest)["thin"].clone();
    let forged = s.h.p("forged-order.json");
    std::fs::write(&forged, serde_json::to_string(&v).unwrap()).unwrap();
    s.h.fail(&["pos-show", "--object", &forged]);
    let mut v = read(&short_rest);
    v["amount"] = json!(v["amount"].as_u64().unwrap() - 1);
    std::fs::write(&forged, serde_json::to_string(&v).unwrap()).unwrap();
    let poor = s.h.run(&["pos-show", "--object", &forged]);
    println!("T1b O7 cli: an order file holding a sompi less than its margin and tip is {}", if poor.0 { "read" } else { "refused" });
    // Round 4, F7, fixed (the tool): order-match refuses two orders past their expiry when it knows the time (--daa),
    // unless --past-expiry; without --daa (offline, the time unknown) it still matches, which is the design note.
    let old_sell = place(&mut s, &exec, "short", "2", "10", "40", "100", 33_900, &[]);
    let old_buy = place(&mut s, &me, "long", "2", "10.1", "40", "100", 33_900, &[]);
    let base: Vec<&str> = vec!["order-match", "--long", &old_buy, "--short", &old_sell, "--script", &s_match, "--size", "2", "--price", "10.1"];
    let mut with_daa = base.clone();
    with_daa.extend(["--daa", "33900"]);
    assert!(s.h.refused(&matcher, &with_daa).to_lowercase().contains("expir"), "order-match past the expiry with --daa is refused");
    let mut forced = with_daa.clone();
    forced.push("--past-expiry");
    let df = s.h.build(&matcher, &forced);
    assert_eq!(read(&s.h.obj(&df, "position", 34_100))["amount"].as_u64(), Some(80 * KAS), "--past-expiry builds it");
    // Before the expiry --daa does not refuse.
    let mut early = base.clone();
    early.extend(["--daa", "99"]);
    s.h.build(&matcher, &early);
    // Offline (no --daa), the time is unknown and it still matches.
    let d6 = s.h.build(&matcher, &base);
    assert_eq!(read(&s.h.obj(&d6, "position", 34_100))["amount"].as_u64(), Some(80 * KAS));
    println!("T1b F7 fixed (cli): past the expiry, order-match refuses with --daa and builds with --past-expiry; offline it matches");
    // Round 4, F6, fixed (the tool): an order asks the owner's own margin of the other side by default, so a buy of a
    // sompi of margin is refused against a sell that asks 40 KAS.
    let ask_sell = place(&mut s, &exec, "short", "2", "10", "40", "40000", 33_900, &[]);
    // A sompi of margin needs a 0.2 KAS tip to meet the order minimum, so place it directly.
    let thin_buy = {
        let d = s.h.build(&me, &["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", "long", "--size", "2", "--limit", "10.1", "--margin", "0.00000001", "--tip", "0.2", "--expiry", "40000"]);
        s.h.obj(&d, "order", 33_900)
    };
    assert!(s.h.refused(&matcher, &["order-match", "--long", &thin_buy, "--short", &ask_sell, "--script", &s_match, "--size", "2", "--price", "10"]).to_lowercase().contains("ask") || s.h.last.to_lowercase().contains("margin"), "a sompi of margin against an ask of 40 KAS is refused");
    // An order placed with --min-counter-margin 0 asks nothing: a sompi of margin is let in (the owner's choice).
    let open_sell = place(&mut s, &exec, "short", "2", "10", "40", "40000", 33_900, &["--min-counter-margin", "0"]);
    s.h.build(&matcher, &["order-match", "--long", &thin_buy, "--short", &open_sell, "--script", &s_match, "--size", "2", "--price", "10"]);
    println!("T1b F6 fixed (cli): a buy of a sompi is refused against a sell asking 40 KAS, let in against one asking 0");
    // Round 4, F8, fixed (the tool): order-place defaults --min-fill to a tenth of the size for a resting order and to the
    // whole size for --no-rest. A match below the default least is refused.
    let rest20 = place(&mut s, &me, "long", "20", "10.1", "400", "40000", 33_900, &[]);
    let sell20 = place(&mut s, &exec, "short", "20", "10", "400", "40000", 33_900, &["--min-counter-margin", "0"]);
    // The tenth is 2 units; a fill of 1 unit is below it. (A matcher asking 0 of its own side is fine.)
    assert!(s.h.refused(&matcher, &["order-match", "--long", &rest20, "--short", &sell20, "--script", &s_match, "--size", "1", "--price", "10", "--past-expiry"]).to_lowercase().contains("least") || s.h.last.to_lowercase().contains("at least"), "a fill below the defaulted least is refused");
    let norest = place(&mut s, &me, "long", "4", "10.1", "80", "40000", 33_900, &["--no-rest"]);
    assert!(s.h.refused(&matcher, &["order-match", "--long", &norest, "--short", &sell20, "--script", &s_match, "--size", "3", "--price", "10", "--past-expiry"]).to_lowercase().contains("least") || s.h.last.to_lowercase().contains("at least"), "a --no-rest order defaults least to the whole size");
    println!("T1b F8 fixed (cli): order-place defaults the least fill, so a one-unit fill of a resting order and a part fill of a --no-rest order are refused");
}

/// The merge at the command line (round 5), in the registry of round 8: two positions of the same two parties and of one
/// registry merge into one holding the two pots less MERGE_TIP, sent by anyone with no key of either party; positions of
/// two registries are not merged; a position is not merged with itself; a claim is not merged. F11 at the command line: a
/// party alone opens no position in the other's name, and an anchor is not passed for one of another registry.
#[test]
fn t1b_m_the_merge_at_the_command_line() {
    let mut s = setup("merge");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let open_s = s.open.clone();
    let third = s.h.key("third");
    let tip = 1_000_000u64; // MERGE_TIP
    let open = |s: &mut Setup, anchor: &str, registry: &str, x1: &str, am: &str, bm: &str, daa: u64| -> String {
        let d = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", anchor, "--script", &open_s, "--registry", registry, "--key", &exec, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", x1, "--a", &me, "--b", &exec, "--a-margin", am, "--b-margin", bm, "--dispute-daa", "50", "--daa", &daa.to_string()]);
        s.h.obj(&d, "position", daa)
    };
    let a1 = open(&mut s, &anchor, &registry, "10", "40", "60", 33_200);
    let a2 = open(&mut s, &anchor, &registry, "11", "50", "50", 33_200);
    assert_eq!(read(&a1)["covenantId"], read(&a2)["covenantId"], "two positions entered through one anchor are of one registry");
    let (p1, p2) = (read(&a1)["amount"].as_u64().unwrap(), read(&a2)["amount"].as_u64().unwrap());
    // Anyone merges them: a third key funds and signs, with no key of either party.
    let d = s.h.build(&third, &["pos-merge", "--position", &a1, "--with", &a2, "--script", &s.script("KpfTMerge")]);
    let one = s.h.obj(&d, "position", 33_300);
    assert_eq!(read(&one)["amount"].as_u64(), Some(p1 + p2 - tip), "the merged pot is the two less MERGE_TIP");
    assert_eq!(read(&one)["covenantId"], read(&a1)["covenantId"], "the merged position is of the same registry");
    assert_eq!(paid(&d, &me) + paid(&d, &exec), 0, "neither party is paid by a merge");
    // A position is not merged with itself; a claim is not merged.
    let f = s.h.funding(1_000);
    assert!(s.h.fail(&["pos-merge", "--position", &a1, "--with", &a1, "--script", &s.script("KpfTMerge"), "--funder", &exec, "--funding", &f]).to_lowercase().contains("itself"));
    let claim = {
        let d = s.h.build(&me, &["pos-close", "--position", &a1, "--script", &s.script("KpfTClose"), "--side", "a", "--t0", "33250", "--p1", "10"]);
        s.h.obj(&d, "claim", 33_260)
    };
    let f = s.h.funding(1_000);
    s.h.fail(&["pos-merge", "--position", &claim, "--with", &a2, "--script", &s.script("KpfTMerge"), "--funder", &exec, "--funding", &f]);
    // Another registry of the same library: a position entered there is not merged with one of the first, and its anchor
    // is not passed for one of the first registry.
    let bd2 = s.h.build(&third, &["registry-create", "--lib", &genesis, "--copies", "1"]);
    let (registry2, anchor2) = (tx_file(&bd2), s.h.obj(&bd2, "anchor", 33_060));
    let b2 = open(&mut s, &anchor2, &registry2, "11", "50", "50", 33_200);
    assert_ne!(read(&b2)["covenantId"], read(&a1)["covenantId"]);
    let refused = s.h.refused(&third, &["pos-merge", "--position", &a1, "--with", &b2, "--script", &s.script("KpfTMerge")]);
    assert!(refused.to_lowercase().contains("registry"), "{refused}");
    let wrong = s.h.refused(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor2, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33200"]);
    assert!(wrong.contains("not of the registry"), "{wrong}");
    // F11 at the command line: a party alone writes no position of itself and the other from a quarter of the price.
    let alone = s.h.refused(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", "2.5", "--a", &me, "--b", &exec, "--a-margin", "1", "--b-margin", "1", "--dispute-daa", "50", "--daa", "33200"]);
    assert!(alone.to_lowercase().contains("signature") || alone.to_lowercase().contains("key"), "{alone}");
    let other_key = s.h.refused(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &third, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", "2.5", "--a", &me, "--b", &exec, "--a-margin", "1", "--b-margin", "1", "--dispute-daa", "50", "--daa", "33200"]);
    assert!(!other_key.is_empty());
    println!("T1b M cli: two positions of one registry merge to their pots less MERGE_TIP = {tip}, sent by a third key; of two registries, with itself, with a claim: refused; a position at a false entry is not opened by one party alone: {}", alone.lines().next().unwrap_or("").chars().take(140).collect::<String>());
}

/// Round 6's F10 at the command line, closed in round 8 at the way in: the counterparty opens no mirror position in
/// the user's name without the user's key.
#[test]
fn t1b_fixed_the_command_line_opens_no_mirror_position_without_the_users_key() {
    let mut s = setup("novate");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let open_s = s.open.clone();
    let third = s.h.key("third");
    // The mirror, by the counterparty, in the user's name, at an entry of nothing: with its own key only, and with a
    // third key beside its own.
    let mirror = ["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--market1", &market, "--size1", "5", "--dir1", "long", "--x1", "0", "--a", &exec, "--b", &me, "--a-margin", "100", "--b-margin", "1", "--dispute-daa", "50", "--daa", "33200"];
    let refused = s.h.refused(&exec, &mirror);
    assert!(refused.to_lowercase().contains("signature") || refused.to_lowercase().contains("key"), "{refused}");
    let mut with_third = mirror.to_vec();
    with_third.extend(["--key", &third]);
    s.h.refused(&exec, &with_third);
    // With the user's key it is a position the user signed.
    let mut with_me = mirror.to_vec();
    with_me.extend(["--key", &me]);
    s.h.build(&exec, &with_me);
    println!("T1b F10 fixed (cli): a mirror position is not opened without the key of the user it names: {}", refused.lines().next().unwrap_or("").chars().take(160).collect::<String>());
}

/// A novation of two positions of one registry at the command line: the user itself sends it, or anyone, with no key of
/// the user (round 8).
#[test]
fn t1b_n_a_novation_of_honest_positions_at_the_command_line() {
    let mut s = setup("novate-ok");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let open_s = s.open.clone();
    let (third, anyone) = (s.h.key("third"), s.h.key("anyone"));
    // U = me long against S = exec (X); L = third long against U (Y), both at 10, so the transfer is near zero.
    let dx = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "60", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33200"]);
    let x = s.h.obj(&dx, "position", 33_300);
    let dy = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &third, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", "10", "--a", &third, "--b", &me, "--a-margin", "60", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33200"]);
    let y = s.h.obj(&dy, "position", 33_300);
    let d = s.h.build(&me, &["pos-novate", "--position", &x, "--with", &y, "--script", &s.script("KpfTNovate")]);
    assert_eq!(objs(&d, "position", 33_400).len(), 1, "one position of the two that stay");
    let d = s.h.build(&anyone, &["pos-novate", "--position", &x, "--with", &y, "--script", &s.script("KpfTNovate")]);
    assert_eq!(objs(&d, "position", 33_400).len(), 1);
    assert!(paid(&d, &me) > 119 * KAS, "the user is paid its two margins, less the sender's share");
    // X and Y the same file is refused.
    let f = s.h.funding(1_000);
    s.h.fail(&["pos-novate", "--position", &x, "--with", &x, "--script", &s.script("KpfTNovate"), "--funder", &me, "--funding", &f]);
    println!("T1b N cli: a novation builds for the user and for anyone, with no key of the user; X and Y the same position is refused");
}

/// F12 (Medium in round 7), fixed in the tool: `pos-open` asks of whoever takes a party's place what that party
/// posts now, unless told otherwise. The user's counterparty is not left facing a KAS where it faced 100: the
/// novation is refused. Opened with `--b-min-counter-margin 0` (the counterparty asks nothing), the same novation builds: at the
/// script an ask of nothing protects nothing, by design.
#[test]
fn t1b_fixed_the_command_line_opens_an_position_that_asks_what_the_other_posts() {
    let mut s = setup("asks");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let open_s = s.open.clone();
    let second = s.h.key("second");
    let x_args = ["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "5", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "100", "--b-margin", "100", "--dispute-daa", "50", "--daa", "33200"];
    let dx = s.h.build(&me, &x_args);
    let x = s.h.obj(&dx, "position", 33_300);
    // The user's second key long against the user at the same entry, a KAS each: both keys are the user's.
    let dy = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &second, "--market1", &market, "--size1", "5", "--dir1", "long", "--x1", "10", "--a", &second, "--b", &me, "--a-margin", "1", "--b-margin", "1", "--dispute-daa", "50", "--daa", "33200"]);
    let y = s.h.obj(&dy, "position", 33_300);
    let refused = s.h.refused(&me, &["pos-novate", "--position", &x, "--with", &y, "--script", &s.script("KpfTNovate")]);
    assert!(refused.contains("asked"), "{refused}");
    // The same position opened asking nothing for the counterparty: the user takes its 100 KAS out, and the
    // counterparty faces the second key's one KAS.
    let mut x0_args = x_args.to_vec();
    x0_args.extend(["--b-min-counter-margin", "0"]);
    let dx0 = s.h.build(&me, &x0_args);
    let x0 = s.h.obj(&dx0, "position", 33_300);
    let d = s.h.build(&me, &["pos-novate", "--position", &x0, "--with", &y, "--script", &s.script("KpfTNovate")]);
    assert!(paid(&d, &me) > 100 * KAS, "the user has its margins back");
    println!("T1b F12 fixed (cli): by default the novation that leaves the counterparty facing 1 KAS is refused: {}", refused.lines().next().unwrap_or("").chars().take(200).collect::<String>());
}

/// FINDING (Medium, F14). `lib-check --registry` and `--registry` on `pos-open` / `order-place` check a registry's genesis and
/// refuse what is not one; but `--registry` is optional, and without it both commands build with a WARNING on standard
/// output. A user handed an anchor of a registry whose genesis also wrote a position in its name (the engine side:
/// `t1b_b2_a_registry_whose_genesis_wrote_an_position_is_the_false_merge_again` in auditT1_builders.rs) enters it. The
/// library's own check refuses unless `--lib-unchecked` is given; the registry's only warns.
#[test]
fn t1b_fixed_the_command_line_enters_no_registry_it_did_not_check() {
    let mut s = setup("registry");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let open_s = s.open.clone();
    // The check itself.
    assert!(s.h.ok(&["lib-check", "--lib", &genesis, "--registry", &registry]).contains("its genesis writes anchors of this library and nothing else"));
    assert!(s.h.fail(&["lib-check", "--lib", &genesis, "--registry", &genesis]).contains("not an anchor"), "a library's genesis is not a registry's");
    let base = read(&registry);
    let mut v = base.clone();
    let outs = outputs(&mut v).clone();
    let anchors: Vec<usize> = (0..outs.len()).filter(|i| !outs[*i]["covenant"].is_null()).collect();
    let plain: Vec<usize> = (0..outs.len()).filter(|i| outs[*i]["covenant"].is_null()).collect();
    assert_eq!(anchors.len(), 2);
    let key_spk = outs[plain[0]]["scriptPublicKey"].clone();
    let lib_outs = outputs(&mut read(&genesis)).clone();
    let script_spk = lib_outs.iter().find(|o| !o["covenant"].is_null()).unwrap()["scriptPublicKey"].clone();
    let cov = outs[anchors[0]]["covenant"].clone();
    let other = "5a".repeat(32);
    let mut cases: Vec<(&str, Value)> = vec![];
    let mut case = |what: &'static str, f: &dyn Fn(&mut Vec<Value>)| {
        let mut v = base.clone();
        f(outputs(&mut v));
        cases.push((what, v));
    };
    case("a key's output in an anchor's place", &|o| o[anchors[1]]["scriptPublicKey"] = key_spk.clone());
    case("a script of the library in an anchor's place", &|o| o[anchors[1]]["scriptPublicKey"] = script_spk.clone());
    case("an output more under the id", &|o| o.push(json!({ "value": KAS, "scriptPublicKey": script_spk, "verboseData": null, "covenant": cov })));
    case("an anchor more (the id is no longer that of the outputs)", &|o| { let c = o[anchors[0]].clone(); o.push(c); });
    case("an anchor short", &|o| { o.remove(anchors[1]); });
    case("another id on every anchor", &|o| anchors.iter().for_each(|i| o[*i]["covenant"]["covenantId"] = json!(other)));
    case("another id on one anchor", &|o| o[anchors[1]]["covenant"]["covenantId"] = json!(other));
    case("an anchor of another value", &|o| o[anchors[0]]["value"] = json!(o[anchors[0]]["value"].as_u64().unwrap() + 1));
    case("nothing under an id", &|o| o.iter_mut().for_each(|x| x["covenant"] = Value::Null));
    let order = ["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--market1", &market, "--dispute-daa", "50", "--side", "long", "--size", "2", "--limit", "10", "--margin", "40", "--tip", "1", "--expiry", "40000"];
    let open = ["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--key", &exec, "--market1", &market, "--size1", "3", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33200"];
    for (i, (what, v)) in cases.iter().enumerate() {
        let f = s.h.p(&format!("bad-registry-{i}.json"));
        std::fs::write(&f, serde_json::to_string(v).unwrap()).unwrap();
        let (ok, out, _) = s.h.run(&["lib-check", "--lib", &genesis, "--registry", &f]);
        assert!(!ok, "lib-check --registry takes {what}:\n{out}");
        for cmd in [&order[..], &open[..]] {
            let mut a = cmd.to_vec();
            a.extend(["--registry", &f]);
            assert!(!s.h.refused(&me, &a).is_empty(), "{what}");
        }
    }
    // With the registry's genesis given, both build and say the registry was checked.
    for cmd in [&order[..], &open[..]] {
        let mut a = cmd.to_vec();
        a.extend(["--registry", &registry]);
        s.h.build(&me, &a);
        assert!(s.h.last.contains("its genesis writes anchors of this library and nothing else") && !s.h.last.contains("WARNING: the registry"), "{}", s.h.last);
    }
    // Without it both are refused (fixed after round 8 by the author of the fix), unless the user says it checked.
    for cmd in [&order[..], &open[..]] {
        assert!(s.h.refused(&me, cmd).contains("REFUSED: the anchor's registry is not checked"));
        let mut a = cmd.to_vec();
        a.push("--registry-unchecked");
        s.h.build(&me, &a);
        assert!(s.h.last.contains("WARNING") && s.h.last.contains("NOT checked"), "{}", s.h.last);
    }
    println!("T1b F14 fixed (cli): {} files that are not this library's registry refused with --registry; without --registry order-place and pos-open are refused unless --registry-unchecked", cases.len());
}

/// The join at the command line (round 9): two positions of the same two users in two markets become one position of
/// two legs, with both users' keys (round 10; a third key may pay for it), the second's keys in the same places or turned.
/// The position written is not merged, not novated and not joined again; margin is added to it and a close is
/// declared on it at two prices.
#[test]
fn t1b_j_the_join_at_the_command_line() {
    let mut s = setup("join");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, open_s, registry) = (s.anchor.clone(), s.open.clone(), s.registry.clone());
    let third = s.h.key("third");
    let tip = 1_000_000u64; // MERGE_TIP
    let feed2 = s.h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = s.h.p("market2.json");
    s.h.ok(&["c-market", "--committee", &s.h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    // A position of `a` long against `b`, funded by `me`, the other key given.
    let open = |s: &mut Setup, market: &str, a: &str, b: &str, size: &str, margin: &str| -> String {
        let other = if a == me { b } else { a };
        let d = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", other, "--market1", market, "--size1", size, "--dir1", "long", "--x1", "10", "--a", a, "--b", b, "--a-margin", margin, "--b-margin", margin, "--dispute-daa", "50", "--daa", "33200"]);
        s.h.obj(&d, "position", 33_200)
    };
    let x = open(&mut s, &market, &me, &exec, "5", "100");
    let y = open(&mut s, &market2, &me, &exec, "3", "60");
    let yt = open(&mut s, &market2, &exec, &me, "3", "60");
    let (p1, p2) = (read(&x)["amount"].as_u64().unwrap(), read(&y)["amount"].as_u64().unwrap());
    let s_join = s.script("KpfTJoin");
    // Round 10: a join carries both users' signatures. A third key alone, or with one user's key, builds nothing.
    let alone = s.h.refused(&third, &["pos-join", "--position", &x, "--with", &y, "--script", &s_join]);
    assert!(alone.to_lowercase().contains("signature") || alone.to_lowercase().contains("key"), "{alone}");
    s.h.refused(&third, &["pos-join", "--position", &x, "--with", &y, "--script", &s_join, "--key", &me]);
    s.h.refused(&third, &["pos-join", "--position", &x, "--with", &y, "--script", &s_join, "--key", &exec]);
    s.h.refused(&me, &["pos-join", "--position", &x, "--with", &y, "--script", &s_join]);
    let d = s.h.build(&third, &["pos-join", "--position", &x, "--with", &y, "--script", &s_join, "--key", &me, "--key", &exec]);
    let one = s.h.obj(&d, "position", 33_300);
    assert_eq!(read(&one)["amount"].as_u64(), Some(p1 + p2 - tip), "the joined position holds the two less the sender's share");
    assert_eq!(read(&one)["covenantId"], read(&x)["covenantId"], "of the same registry");
    assert_eq!(paid(&d, &me) + paid(&d, &exec), 0, "neither party is paid by a join");
    let shown = s.h.ok(&["pos-show", "--object", &one]);
    s.h.build(&third, &["pos-join", "--position", &x, "--with", &yt, "--script", &s_join, "--key", &me, "--key", &exec]);
    // One of the two pays for it itself, with the other's key.
    s.h.build(&me, &["pos-join", "--position", &x, "--with", &y, "--script", &s_join, "--key", &exec]);
    // Not joined: two positions of one market, a position with itself, and a joined position with anything.
    let x2 = open(&mut s, &market, &me, &exec, "2", "40");
    let y2 = open(&mut s, &market2, &me, &exec, "2", "40");
    assert!(s.h.refused(&third, &["pos-join", "--position", &x, "--with", &x2, "--script", &s_join, "--key", &me, "--key", &exec]).contains("one market"));
    s.h.refused(&third, &["pos-join", "--position", &x, "--with", &x, "--script", &s_join, "--key", &me, "--key", &exec]);
    let again = s.h.refused(&third, &["pos-join", "--position", &one, "--with", &y2, "--script", &s_join, "--key", &me, "--key", &exec]);
    assert!(again.contains("two legs already"), "{again}");
    s.h.refused(&third, &["pos-join", "--position", &x2, "--with", &one, "--script", &s_join, "--key", &me, "--key", &exec]);
    // The joined position is not merged with a position of either market, and its user is not taken out of it.
    s.h.refused(&third, &["pos-merge", "--position", &one, "--with", &x2, "--script", &s.script("KpfTMerge")]);
    s.h.refused(&third, &["pos-merge", "--position", &x2, "--with", &one, "--script", &s.script("KpfTMerge")]);
    let sold = open(&mut s, &market, &third, &me, "5", "100");
    let out = s.h.build(&third, &["pos-novate", "--position", &x, "--with", &sold, "--script", &s.script("KpfTNovate")]);
    assert!(paid(&out, &me) > 199 * KAS, "before the join the user leaves market 1 by a sale, with its margins");
    s.h.refused(&third, &["pos-novate", "--position", &one, "--with", &sold, "--script", &s.script("KpfTNovate")]);
    // What is left: margin is added to it, and a close of the whole is declared at two prices.
    s.h.build(&me, &["pos-add-margin", "--position", &one, "--script", &s.script("KpfTAdd"), "--side", "a", "--amount", "5"]);
    s.h.build(&me, &["pos-close", "--position", &one, "--script", &s.script("KpfTClose"), "--side", "a", "--t0", "33250", "--p1", "10", "--p2", "10"]);
    println!("T1b J cli: two positions of two markets (the second's keys either way) are joined with both users' keys into one holding the two less {tip}; a third key alone or with one user's key builds nothing; the joined position is not joined again, not merged, not novated; margin is added and a close declared on it. pos-show: {}", shown.lines().next().unwrap_or("").chars().take(160).collect::<String>());
}

/// Round 11 at the command line: a cross position is written the other way round (users, legs) by a third key; an
/// position of the same two users in its first leg's market is folded into it by a third key; and its first leg
/// leaves to the third user of a second position with the keys of the position's two users (round 12; a third key may
/// pay). F16 at the command line, fixed: the second position is the user's own second key's, opened by the two of them
/// at the leg's own entry with the share of what the one that stays asked; without the key of the one that stays
/// the tool builds no leave.
#[test]
fn t1b_l_turn_fold_leave_at_the_command_line() {
    let mut s = setup("leave");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, open_s, registry) = (s.anchor.clone(), s.open.clone(), s.registry.clone());
    let (third, second) = (s.h.key("third"), s.h.key("second"));
    let feed2 = s.h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = s.h.p("market2.json");
    s.h.ok(&["c-market", "--committee", &s.h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    // A position of `a` long against `b` from `x1`, funded by `me`, the other key given.
    let open = |s: &mut Setup, market: &str, a: &str, b: &str, x1: &str, am: &str, bm: &str| -> String {
        let other = if a == me { b } else { a };
        let d = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", other, "--market1", market, "--size1", "10", "--dir1", "long", "--x1", x1, "--a", a, "--b", b, "--a-margin", am, "--b-margin", bm, "--dispute-daa", "50", "--daa", "33200"]);
        s.h.obj(&d, "position", 33_200)
    };
    // `me` long 10 units of each market against `exec`, 20 KAS each a leg; joined by both.
    let x1 = open(&mut s, &market, &me, &exec, "10", "20", "20");
    let x2 = open(&mut s, &market2, &me, &exec, "10", "20", "20");
    let dj = s.h.build(&third, &["pos-join", "--position", &x1, "--with", &x2, "--script", &s.script("KpfTJoin"), "--key", &me, "--key", &exec]);
    let cross = s.h.obj(&dj, "position", 33_300);
    let held = read(&cross)["amount"].as_u64().unwrap();
    // The turn, by a third key: the same value, the same registry.
    for mode in ["legs", "users"] {
        let d = s.h.build(&third, &["pos-turn", "--position", &cross, "--script", &s.script("KpfTTurn"), "--mode", mode]);
        let t = s.h.obj(&d, "position", 33_500);
        assert_eq!((read(&t)["amount"].as_u64(), read(&t)["covenantId"].clone()), (Some(held), read(&cross)["covenantId"].clone()), "a turn of the {mode}");
        assert_eq!(paid(&d, &me) + paid(&d, &exec), 0);
    }
    s.h.refused(&third, &["pos-turn", "--position", &cross, "--script", &s.script("KpfTTurn"), "--mode", "both"]);
    s.h.refused(&third, &["pos-turn", "--position", &x1, "--script", &s.script("KpfTTurn"), "--mode", "legs"]);
    // The fold, by a third key: a position of the same two users in the first leg's market.
    let more = open(&mut s, &market, &me, &exec, "10.4", "8", "8");
    let d = s.h.build(&third, &["pos-fold", "--position", &cross, "--with", &more, "--script", &s.script("KpfTFold")]);
    let folded = s.h.obj(&d, "position", 33_400);
    assert_eq!(read(&folded)["amount"].as_u64(), Some(held + read(&more)["amount"].as_u64().unwrap() - 1_000_000), "the folded position holds the two less the sender's share");
    // Not folded: a position of the second leg's market (the legs are turned first), a position with a third user.
    let other_market = open(&mut s, &market2, &me, &exec, "10", "8", "8");
    s.h.refused(&third, &["pos-fold", "--position", &cross, "--with", &other_market, "--script", &s.script("KpfTFold")]);
    let with_third = open(&mut s, &market, &third, &me, "10", "20", "1");
    s.h.refused(&third, &["pos-fold", "--position", &cross, "--with", &with_third, "--script", &s.script("KpfTFold")]);
    // F16 at the command line. `me`'s second key long against `me` in market 1 from the leg's own entry (10), posting
    // 20 KAS: the share of the 40 KAS `exec` asked of whoever takes `me`'s place. Opened by `me`'s two keys alone.
    let mine = open(&mut s, &market, &second, &me, "10", "20", "1");
    // Round 12: a leave carries the signatures of the position's two users. A third key alone, with the user's key
    // only (F16: the one that stays did not sign), or with the other's only (F17) builds nothing.
    let alone = s.h.refused(&third, &["pos-leave", "--position", &cross, "--with", &mine, "--script", &s.script("KpfTLeave")]);
    assert!(alone.to_lowercase().contains("signature") || alone.to_lowercase().contains("key"), "{alone}");
    s.h.refused(&third, &["pos-leave", "--position", &cross, "--with", &mine, "--script", &s.script("KpfTLeave"), "--key", &me, "--key", &second]);
    s.h.refused(&third, &["pos-leave", "--position", &cross, "--with", &mine, "--script", &s.script("KpfTLeave"), "--key", &exec, "--key", &second]);
    s.h.refused(&me, &["pos-leave", "--position", &cross, "--with", &mine, "--script", &s.script("KpfTLeave")]);
    let d = s.h.build(&third, &["pos-leave", "--position", &cross, "--with", &mine, "--script", &s.script("KpfTLeave"), "--key", &me, "--key", &exec]);
    let positions = objs(&d, "position", 33_400);
    assert_eq!(positions.len(), 2, "what stays and the position of the second key against the one that stays");
    assert!(paid(&d, &me) > 0 && paid(&d, &me) < KAS, "the user is paid its own margin in the second position, less the sender's share: {}", paid(&d, &me));
    assert_eq!(paid(&d, &exec), 0);
    let total: u64 = positions.iter().map(|f| read(f)["amount"].as_u64().unwrap()).sum();
    assert_eq!(total + paid(&d, &me) + 1_500_000, held + read(&mine)["amount"].as_u64().unwrap(), "nothing is made or lost but the sender's share");
    // With a KAS less than the share behind it the rule refuses; a position of one leg is not left.
    let thin = open(&mut s, &market, &second, &me, "10", "19", "1");
    let refused = s.h.refused(&third, &["pos-leave", "--position", &cross, "--with", &thin, "--script", &s.script("KpfTLeave"), "--key", &me, "--key", &exec]);
    assert!(refused.contains("asked"), "{refused}");
    assert!(s.h.refused(&third, &["pos-leave", "--position", &x1, "--with", &mine, "--script", &s.script("KpfTLeave"), "--key", &me, "--key", &exec]).contains("one leg"));
    println!("T1b L cli: a third key turns a cross position (legs, users) and folds a position of the same two users into it; a leave builds with both users' keys and not without the one that stays or without the user; one KAS less behind it: {}", refused.lines().next().unwrap_or("").chars().take(200).collect::<String>());
}

// ================================================================ round 14: signing on two machines

fn address_of(h: &H, key: &str) -> String {
    h.ok(&["address", "--key", key]).split_whitespace().find(|w| w.starts_with("kaspatest:")).expect("an address").to_string()
}
fn txid_line(out: &str) -> String {
    out.lines().find(|l| l.starts_with("txid ")).expect("a txid").split_whitespace().nth(1).unwrap().to_string()
}

/// F19 (Low in round 14), fixed in round 15 at the command line. A file that lists the co-signer's own coin among the
/// signatures a script waits for is REFUSED and nothing is signed; so is one that says an object's input waits for a
/// signature. The honest file is signed as before ("YOU SIGN THIS" twice: the coin and the script), the position it
/// writes is remarked on for what it holds beyond its state, and the writer's NOTE says that taking a signature back
/// is a race and that for an opening it is the signer's own coin.
///
/// What also holds: a place beyond the script, the script's input left to a key, another library in the file, a
/// state of the pool changed, the inputs not looked up without `--inputs-unchecked`: each refused, nothing signed.
#[test]
fn t1b_fixed_f19_the_co_signer_signs_no_coin_the_file_does_not_call_left_to_it() {
    let mut s = setup("cosign");
    let (me, you, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, open_s, registry) = (s.anchor.clone(), s.open.clone(), s.registry.clone());
    let you_at = address_of(&s.h, &you);
    let (fa, fb) = (s.h.funding(1_000), s.h.funding(61));
    let (p1, out_a, out_b) = (s.h.p("open.partial.json"), s.h.p("machine-a"), s.h.p("machine-b"));
    let out = s.h.ok(&["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &me, "--market1", &market, "--size1", "10", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &you_at, "--a-margin", "40", "--b-margin", "60", "--b-funding", &fb, "--dispute-daa", "50", "--daa", "33100", "--funder", &me, "--funding", &fa, "--out-dir", &out_a, "--partial", &p1]);
    assert!(out.contains("WAITS FOR THE SIGNATURE OF") && out.contains("LEFT TO SIGN"), "{out}");
    assert!(out.contains("whichever lands first stands") && out.contains("an anchor is not yours to move, so for an opening it is your coin"), "the NOTE: {out}");
    let cosign = |file: &str| -> Vec<String> { ["cosign", "--tx", file, "--lib", &genesis, "--registry", &registry, "--key", &you, "--inputs-unchecked", "--out-dir", &out_b].iter().map(|x| x.to_string()).collect() };
    let run = |h: &H, args: &[String]| h.run(&args.iter().map(|x| x.as_str()).collect::<Vec<_>>());
    // The honest file.
    let (ok, honest, err) = run(&s.h, &cosign(&p1));
    assert!(ok, "{err}\n{honest}");
    assert_eq!(honest.matches("YOU SIGN THIS").count(), 2, "the coin left to the co-signer, and the script's signature: {honest}");
    assert!(honest.contains("in plain coins you put in 61 KAS and are paid 1 KAS"), "{honest}");
    assert!(honest.contains("more than its state says (its carrier)"), "the position's carrier is remarked on: {honest}");
    // The same file with the co-signer's coin moved from `open` to `open_sigs`.
    let v = read(&p1);
    let open = v["partial"]["open"].as_array().expect("the open inputs").clone();
    assert_eq!(open.len(), 1, "the counterparty's coin is left to it");
    let ci = open[0][0].as_u64().unwrap() as usize;
    let place: Vec<u8> = [vec![65u8], vec![0xEE; 65]].concat();
    let refuse = |h: &H, what: &str, f: &dyn Fn(&mut Value), want: &str| {
        let mut t = v.clone();
        f(&mut t);
        let file = h.p("bad.partial.json");
        std::fs::write(&file, serde_json::to_string(&t).unwrap()).unwrap();
        let (ok, out, err) = run(h, &cosign(&file));
        assert!(!ok && (err.contains(want) || out.contains(want)), "{what}: {err}\n{out}");
        assert!(!out.contains("not submitted: a dry run"), "{what}: nothing is signed");
    };
    refuse(&s.h, "the co-signer's coin among the signatures a script waits for", &|t| {
        t["partial"]["open"] = json!([]);
        t["partial"]["open_sigs"].as_array_mut().unwrap().push(json!([ci, 1, open[0][1]]));
        let cur = t["partial"]["tx"]["inputs"][ci]["signatureScript"].clone();
        t["partial"]["tx"]["inputs"][ci]["signatureScript"] = if cur.is_string() { json!(place.iter().map(|b| format!("{b:02x}")).collect::<String>()) } else { json!(place) };
    }, "is a plain coin, and the file lists it among the signatures a script waits for. Nothing was signed");
    refuse(&s.h, "the coin among them and still left to its key", &|t| { t["partial"]["open_sigs"].as_array_mut().unwrap().push(json!([ci, 1, open[0][1]])); }, "REFUSED");
    refuse(&s.h, "the object's input said to wait for a signature", &|t| { t["partial"]["open_sigs"].as_array_mut().unwrap().push(json!([0, 0, open[0][1]])); }, "is an object, and the file says it waits for a signature");
    refuse(&s.h, "a place beyond the script", &|t| { t["partial"]["open_sigs"][0][1] = json!(100_000); }, "no place for the signature");
    let si = v["partial"]["open_sigs"][0][0].as_u64().unwrap();
    refuse(&s.h, "the script's input left to a key", &|t| { t["partial"]["open"].as_array_mut().unwrap().push(json!([si, open[0][1]])); }, "REFUSED");
    refuse(&s.h, "another library in the file", &|t| { t["library"] = json!("5a".repeat(32)); }, "REFUSED");
    refuse(&s.h, "a state of the pool changed", &|t| { let x = t["states"][0].as_str().unwrap().to_string(); let last = if x.ends_with('0') { "1" } else { "0" }; t["states"][0] = json!(format!("{}{last}", &x[..x.len() - 1])); }, "");
    let mut online = cosign(&p1);
    online.retain(|x| x != "--inputs-unchecked");
    let (ok, _, err) = run(&s.h, &online);
    assert!(!ok && err.contains("REFUSED: the inputs must be looked up at the node"), "{err}");
    println!("T1b F19 fixed (cli): a coin listed among a script's signatures, and an object said to wait for one, are refused and nothing is signed; the honest file is signed with YOU SIGN THIS twice and the position's carrier remarked on");
}

/// Round 14, (b): the holder of a file with the last signature sends it when it likes. The NOTE's way to take a
/// signature back is sound: the close by both that the counterparty signed and passed on spends the position's
/// outpoint, and the counterparty's own `pos-add-margin` spends the same outpoint: whichever lands first voids the
/// other. (It is a race, not a recall: if the holder sends first, the close stands.)
#[test]
fn t1b_r14_a_signature_passed_on_is_taken_back_by_spending_the_position() {
    let mut s = setup("recall");
    let (me, you) = (s.me.clone(), s.exec.clone());
    let genesis = s.genesis.clone();
    let payer = s.h.key("payer");
    let payer_at = address_of(&s.h, &payer);
    let (fe, p2, p3) = (s.h.funding(1_000), s.h.p("close.partial.json"), s.h.p("close.partial2.json"));
    let (out_a, out_b) = (s.h.p("machine-a"), s.h.p("machine-b"));
    let position = s.position.clone();
    let out = s.h.ok(&["pos-close", "--mutual", "--position", &position, "--script", &s.script("KpfTBoth"), "--to-a", "45", "--key", &me, "--funder", &payer_at, "--funding", &fe, "--out-dir", &out_a, "--partial", &p2]);
    assert!(out.contains("To take your signature back, spend one of its inputs"), "{out}");
    let out = s.h.ok(&["cosign", "--tx", &p2, "--lib", &genesis, "--key", &you, "--inputs-unchecked", "--out-dir", &out_b, "--out", &p3]);
    assert!(out.contains("still to be signed by") && out.contains(&payer_at) && out.contains("To take your signature back"), "{out}");
    // The counterparty's own way out: add a sompi... a KAS to its margin; the position is written anew.
    let d = s.h.build(&you, &["pos-add-margin", "--position", &position, "--script", &s.script("KpfTAdd"), "--side", "b", "--amount", "1"]);
    let outpoint = read(&position)["outpoint"].clone();
    let txid = outpoint["txid"].as_str().or(outpoint["transactionId"].as_str()).or(outpoint.as_str()).map(|x| x.split(':').next().unwrap().to_string()).unwrap_or_else(|| panic!("the position's outpoint: {outpoint}"));
    let (held, add) = (std::fs::read_to_string(&p3).unwrap(), std::fs::read_to_string(tx_file(&d)).unwrap());
    assert!(held.contains(&txid) && add.contains(&txid), "the file held and the added margin spend the same position");
    // The executor signs last: the transaction is whole and would be sent. So until then the position is the
    // counterparty's to move.
    let last = s.h.ok(&["cosign", "--tx", &p3, "--lib", &genesis, "--key", &payer, "--inputs-unchecked", "--out-dir", &out_b]);
    assert!(last.contains("YOU SIGN THIS") && last.contains("not submitted: a dry run") && !last.contains("FAILED"), "{last}");
    // Signing again what is signed: nothing waits for that key.
    assert!(s.h.fail(&["cosign", "--tx", &p3, "--lib", &genesis, "--key", &you, "--inputs-unchecked", "--out-dir", &out_b]).contains("nothing in this transaction waits for a key given"));
    println!("T1b R14 (b): the close the counterparty signed and passed on, and its own pos-add-margin, spend the same position ({txid}): the first to land voids the other");
}

/// Round 15: the join and the leave from two machines at the command line (offline). One user writes the file with
/// its own key (`--partial`), the other reads it and signs (`cosign`): no registry is asked (nothing enters one), each
/// object is shown with "yours in it", the co-signer signs once (the script's signature), and what it sends is the
/// transaction one machine with both keys makes (the same txid). A key that is no signer's is asked nothing; the
/// third user of a leave does not sign.
#[test]
fn t1b_r15_a_join_and_a_leave_from_two_machines_at_the_command_line() {
    let mut s = setup("apart");
    let (me, you, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, open_s, registry) = (s.anchor.clone(), s.open.clone(), s.registry.clone());
    let third = s.h.key("third");
    let feed2 = s.h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = s.h.p("market2.json");
    s.h.ok(&["c-market", "--committee", &s.h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    let open = |s: &mut Setup, market: &str, a: &str, b: &str, x1: &str, am: &str, bm: &str| -> String {
        let other = if a == me { b } else { a };
        let d = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", other, "--market1", market, "--size1", "10", "--dir1", "long", "--x1", x1, "--a", a, "--b", b, "--a-margin", am, "--b-margin", bm, "--dispute-daa", "50", "--daa", "33200"]);
        s.h.obj(&d, "position", 33_200)
    };
    let x1 = open(&mut s, &market, &me, &you, "10", "20", "20");
    let x2 = open(&mut s, &market2, &me, &you, "10", "20", "20");
    let (s_join, s_leave) = (s.script("KpfTJoin"), s.script("KpfTLeave"));
    let (out_a, out_b, out_1) = (s.h.p("machine-a"), s.h.p("machine-b"), s.h.p("one-machine"));
    // ---- the join: `me` writes, `you` signs.
    let (fj, pj) = (s.h.funding(1_000), s.h.p("join.partial.json"));
    let join = ["pos-join", "--position", &x1, "--with", &x2, "--script", &s_join, "--funder", &me, "--funding", &fj];
    // (With `--partial` a key file given as `--funder` only names the payer: what signs here is `--key`.)
    let mut write: Vec<&str> = join.to_vec();
    write.extend(["--key", &me, "--out-dir", &out_a, "--partial", &pj]);
    let out = s.h.ok(&write);
    assert!(out.contains("(partly signed)") && out.contains("WAITS FOR THE SIGNATURE OF") && !out.contains("LEFT TO SIGN"), "{out}");
    let signed = s.h.ok(&["cosign", "--tx", &pj, "--lib", &genesis, "--key", &you, "--inputs-unchecked", "--out-dir", &out_b]);
    assert_eq!(signed.matches("YOU SIGN THIS").count(), 1, "the script's signature and nothing else: {signed}");
    assert!(signed.matches("yours in it").count() >= 3 && !signed.contains("NONE OF THIS OBJECT'S KEYS IS YOURS") && signed.contains("not submitted: a dry run") && !signed.contains("FAILED"), "{signed}");
    assert!(signed.contains("in plain coins you put in 0 KAS and are paid 0 KAS"), "{signed}");
    let mut whole: Vec<&str> = join.to_vec();
    whole.extend(["--key", &you, "--out-dir", &out_1]);
    let one = s.h.ok(&whole);
    assert_eq!(txid_line(&signed), txid_line(&one), "the join of two machines is the join of one");
    // A key that is no signer's is asked nothing; the writer's own key has nothing left to sign.
    assert!(s.h.fail(&["cosign", "--tx", &pj, "--lib", &genesis, "--key", &third, "--inputs-unchecked", "--out-dir", &out_b]).contains("nothing in this transaction waits for a key given"));
    assert!(s.h.fail(&["cosign", "--tx", &pj, "--lib", &genesis, "--key", &me, "--inputs-unchecked", "--out-dir", &out_b]).contains("nothing in this transaction waits for a key given"));
    // ---- the leave: `me` sold its first leg to `third`; `you` (the one that stays) writes, `me` signs.
    let cross = s.h.obj(&format!("{out_1}/dry-run"), "position", 33_300);
    let sold = open(&mut s, &market, &third, &me, "10.2", "25", "12");
    let (fl, pl) = (s.h.funding(1_000), s.h.p("leave.partial.json"));
    let leave = ["pos-leave", "--position", &cross, "--with", &sold, "--script", &s_leave, "--funder", &you, "--funding", &fl];
    let mut write: Vec<&str> = leave.to_vec();
    write.extend(["--key", &you, "--out-dir", &out_b, "--partial", &pl]);
    let out = s.h.ok(&write);
    assert!(out.contains("(partly signed)") && out.contains("WAITS FOR THE SIGNATURE OF"), "{out}");
    // The third user does not sign a leave.
    assert!(s.h.fail(&["cosign", "--tx", &pl, "--lib", &genesis, "--key", &third, "--inputs-unchecked", "--out-dir", &out_a]).contains("nothing in this transaction waits for a key given"));
    let signed = s.h.ok(&["cosign", "--tx", &pl, "--lib", &genesis, "--key", &me, "--inputs-unchecked", "--out-dir", &out_a]);
    assert_eq!(signed.matches("YOU SIGN THIS").count(), 1, "{signed}");
    assert!(signed.contains("yours in it") && signed.contains("NONE OF THIS OBJECT'S KEYS IS YOURS") && signed.contains("not submitted: a dry run") && !signed.contains("FAILED"), "the position of the third user and the one that stays is not the seller's: {signed}");
    assert!(signed.contains("(you)"), "the seller is paid its margin in the second position: {signed}");
    let mut whole: Vec<&str> = leave.to_vec();
    whole.extend(["--key", &me, "--out-dir", &out_1]);
    let one = s.h.ok(&whole);
    assert_eq!(txid_line(&signed), txid_line(&one), "the leave of two machines is the leave of one");
    println!("T1b R15 cli: a join and a leave written on one machine and signed on another are the transactions one machine makes ({}, {})", txid_line(&signed), txid_line(&one));
}

// ================================================================ round 16: the executor's loop, offline

/// A folder for `exec-run`, and a run of one round over it without a node (a trial: nothing is sent).
fn exec_round(h: &H, folder: &str, genesis: &str, registry: &str, funder: &str, funding: &str, daa: &str, extra: &[&str]) -> String {
    let mut a = vec!["exec-run", "--dir", folder, "--lib", genesis, "--registry", registry, "--funder", funder, "--funding", funding, "--daa", daa];
    a.extend(extra);
    h.ok(&a)
}
fn put_in(folder: &str, file: &str) -> String {
    let to = std::path::Path::new(folder).join(std::path::Path::new(file).file_name().unwrap());
    std::fs::copy(file, &to).unwrap();
    to.display().to_string()
}
fn set_daa(file: &str, daa: u64) {
    let mut v = read(file);
    v["daa"] = json!(daa);
    std::fs::write(file, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

/// Item 3, the executor's price. Two orders that cross are matched at the limit of the one whose UTXO is the older.
/// A sell at 10 that stood first and a buy at 11 that came to it: 10. The same two with the sell's UTXO the younger
/// (what a fill of a part of it, by its owner's second key, makes of it: what rests is written anew): 11. The whole
/// overlap of the two limits goes to whoever renewed its order last: here 1 KAS a unit, 3 KAS on 3 units. (The script
/// takes any price between the two limits from whoever sends the match, so a user that matches itself has the same
/// without this: the limit is an order's only protection.) And item 2 for a match: what the round says the sender is
/// left is what the transaction pays its key less the coin it spent.
#[test]
fn t1b_design_the_executors_price_is_the_limit_of_the_order_that_was_not_renewed_last() {
    let mut s = setup("price");
    let (me, you, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let payer = s.h.key("payer");
    let place = |s: &mut Setup, who: &str, side: &str, limit: &str, tip: &str, daa: u64| -> String {
        let d = s.h.build(who, &["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", side, "--size", "3", "--limit", limit, "--margin", "12", "--tip", tip, "--expiry", "40000"]);
        s.h.obj(&d, "order", daa)
    };
    let folder = s.h.p("folder");
    std::fs::create_dir_all(&folder).unwrap();
    let sell = put_in(&folder, &place(&mut s, &you, "short", "10", "1", 33_900));
    let _buy = put_in(&folder, &place(&mut s, &me, "long", "11", "1", 33_950));
    let f = s.h.funding(50);
    // (A trial writes what it built under the folder's own `dry-run/`.)
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("match 3 units at 10 ") && out.contains("1 sent"), "the sell stood first: its limit: {out}");
    // What the sender is left: the line, and the transaction.
    let line = out.lines().find(|l| l.contains("the sender is left")).expect("the gain's line").to_string();
    let left: f64 = line.split("the sender is left ").nth(1).unwrap().split_whitespace().next().unwrap().parse().expect("a number of KAS");
    let got = paid(&format!("{folder}/dry-run"), &payer) as i64 - 50 * KAS as i64;
    assert_eq!((left * KAS as f64).round() as i64, got, "the gain the round states is the transaction's: {line}");
    assert!(got > 0 && got < 2 * KAS as i64, "the two tips less the fee: {got}");
    // The sell renewed after the buy came: the buy is now the one that "stood first".
    set_daa(&sell, 33_960);
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("match 3 units at 11 ") && out.contains("1 sent"), "the buy's limit: {out}");
    println!("T1b DESIGN price: a sell at 10 and a buy at 11 are matched at 10 while the sell's UTXO is the older and at 11 once it is the younger: 3 KAS on 3 units to the seller; the sender is left {got} sompi either way");
}

/// F21 (Low in round 16), fixed in round 17. A match whose two tips are not above the gain asked is not built: 4
/// buys and 4 sells that cross and carry no tip are passed over in a round (no "left alone", nothing built), and two
/// paying orders of other users are matched in the same round. Pairs whose tips are above nothing but below the fee
/// are built once and left alone (that they are not built again in a later round of the same run needs a node: read
/// in `Round::send`, not run). Round 18: the pre-check counts what an order holds beyond its margin and tip. A pair
/// without tips in which an order used up holds 5 KAS more is matched, and the sender is left the 5 KAS less the fee.
/// The same excess in an order that is NOT used up stays in what rests of it (`order-match`: the resting order holds
/// it, the sender is paid nothing): there the pre-check is above what the match pays, the round builds it once and
/// leaves it alone.
#[test]
fn t1b_fixed_f21_a_round_builds_no_match_whose_tips_are_not_above_the_gain_asked() {
    let mut s = setup("stall");
    let (me, you, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, place_s, registry) = (s.anchor.clone(), s.place.clone(), s.registry.clone());
    let (payer, third, fourth) = (s.h.key("payer"), s.h.key("third"), s.h.key("fourth"));
    let place = |s: &mut Setup, who: &str, side: &str, limit: &str, tip: &str, daa: u64| -> String {
        let d = s.h.build(who, &["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", side, "--size", "3", "--limit", limit, "--margin", "12", "--tip", tip, "--expiry", "40000"]);
        s.h.obj(&d, "order", daa)
    };
    let f = s.h.funding(50);
    let k = 4u64;
    // ---- no tip: passed over, nothing built.
    let folder = s.h.p("folder");
    std::fs::create_dir_all(&folder).unwrap();
    let mut first_buy = String::new();
    for i in 0..k {
        let b = put_in(&folder, &place(&mut s, &me, "long", &format!("{}", 20 + i), "0", 33_900 + i));
        if i == 0 {
            first_buy = b;
        }
        put_in(&folder, &place(&mut s, &you, "short", &format!("{}", 5 - i as i64), "0", 33_920 + i));
    }
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("0 sent") && out.matches("left alone").count() == 0 && !out.contains("the sender is left"), "no pair without a tip is built: {out}");
    assert!(out.contains(&format!("{} objects live of {} in the folder", 2 * k, 2 * k)), "{out}");
    // Round 18: the first buy holding 5 KAS more than its margin and tip. Used up in a match, that excess is the
    // sender's, and the pre-check counts it: the pair is matched and the sender is left the 5 KAS less the fee.
    let mut v = read(&first_buy);
    let held = v["amount"].as_u64().unwrap();
    v["amount"] = json!(held + 5 * KAS);
    std::fs::write(&first_buy, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("1 sent") && out.matches("left alone").count() == 0 && out.matches("the sender is left").count() == 1 && out.contains("match 3 units at"), "the pair that leaves the sender the excess is matched: {out}");
    let line = out.lines().find(|l| l.contains("the sender is left")).expect("the gain's line").to_string();
    let left: f64 = line.split("the sender is left ").nth(1).unwrap().split_whitespace().next().unwrap().parse().expect("a number of KAS");
    let got = paid(&format!("{folder}/dry-run"), &payer) as i64 - 50 * KAS as i64;
    assert_eq!((left * KAS as f64).round() as i64, got, "the gain the round states is the transaction's: {line}");
    assert!(got < 5 * KAS as i64 && got > 5 * KAS as i64 - 2_000_000, "the excess less the fee: {got}");
    std::fs::remove_dir_all(format!("{folder}/dry-run")).unwrap();
    v["amount"] = json!(held);
    std::fs::write(&first_buy, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    // Two orders of other users that cross and pay: matched in the same round.
    put_in(&folder, &place(&mut s, &third, "long", "10.1", "1", 33_950));
    put_in(&folder, &place(&mut s, &fourth, "short", "10", "1", 33_940));
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("1 sent") && out.contains("match 3 units at"), "a paying order is matched in the same round: {out}");
    // ---- tips above nothing and below the fee: built once each and left alone.
    let folder2 = s.h.p("folder2");
    std::fs::create_dir_all(&folder2).unwrap();
    for i in 0..2u64 {
        put_in(&folder2, &place(&mut s, &me, "long", &format!("{}", 20 + i), "0.0001", 33_900 + i));
        put_in(&folder2, &place(&mut s, &you, "short", &format!("{}", 5 - i as i64), "0.0001", 33_920 + i));
    }
    let out = exec_round(&s.h, &folder2, &genesis, &registry, &payer, &f, "34000", &[]);
    assert_eq!(out.matches("left alone").count(), 4, "each pair whose tips do not pay the fee is built and left alone once: {out}");
    assert!(out.contains("0 sent"), "{out}");
    // With a gain asked above their tips they are not built either.
    let out = exec_round(&s.h, &folder2, &genesis, &registry, &payer, &f, "34000", &["--min-gain", "0.001"]);
    assert_eq!(out.matches("left alone").count(), 0, "{out}");
    // ---- round 18, the excess of an order that is not used up: a buy of 3 units holding 5 KAS more, a sell of 2.
    let d = s.h.build(&me, &["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", "long", "--size", "3", "--limit", "20", "--margin", "12", "--tip", "0", "--expiry", "40000"]);
    let big = s.h.obj(&d, "order", 33_900);
    let d = s.h.build(&you, &["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", "short", "--size", "2", "--limit", "5", "--margin", "8", "--tip", "0", "--expiry", "40000"]);
    let small = s.h.obj(&d, "order", 33_920);
    let mut v = read(&big);
    let held = v["amount"].as_u64().unwrap();
    assert_eq!(held, 12 * KAS, "the buy holds its margin and no tip");
    v["amount"] = json!(held + 5 * KAS);
    std::fs::write(&big, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    // The match itself: what rests of the buy holds the excess (a third of its margin and the 5 KAS), and the sender
    // is paid nothing (its coin comes back less the fee).
    let matcher = s.h.key("matcher");
    let dm = s.h.build(&matcher, &["order-match", "--long", &big, "--short", &small, "--script", &s.script("KpfTMatch"), "--size", "2", "--price", "5"]);
    let rest = s.h.obj(&dm, "order", 34_000);
    assert_eq!(read(&rest)["amount"].as_u64(), Some(4 * KAS + 5 * KAS), "the excess stays in what rests of the order");
    let back = paid(&dm, &matcher) as i64 - 100_000 * KAS as i64;
    assert!(back < 0 && back > -2_000_000, "the sender of a match that uses up neither excess is left only the fee to pay: {back}");
    // The round: its pre-check counts the 5 KAS (above what the match pays), so the match is built, and left alone.
    let folder3 = s.h.p("folder3");
    std::fs::create_dir_all(&folder3).unwrap();
    put_in(&folder3, &big);
    put_in(&folder3, &small);
    let out = exec_round(&s.h, &folder3, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("0 sent") && out.matches("left alone").count() == 1 && out.contains("match 2 units at"), "built once and left alone: {out}");
    let line = out.lines().find(|l| l.contains("left alone")).unwrap().to_string();
    let would: f64 = line.split("leave the sender ").nth(1).unwrap().split(" KAS").next().unwrap().parse().expect("a number of KAS");
    assert_eq!((would * KAS as f64).round() as i64, back, "what the round says the sender would be left is what the match pays: {line}");
    // Without the excess the same pair is not built.
    v["amount"] = json!(held);
    std::fs::write(format!("{folder3}/{}", std::path::Path::new(&big).file_name().unwrap().to_string_lossy()), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let out = exec_round(&s.h, &folder3, &genesis, &registry, &payer, &f, "34000", &[]);
    assert!(out.contains("0 sent") && out.matches("left alone").count() == 0, "{out}");
    println!("T1b F21 fixed: {} orders without a tip: no transaction built in a round; a pair whose only pay is the excess of an order used up is matched (the sender is left {got} sompi); the excess of an order that rests stays in it (the sender is left {back} sompi: built once, left alone); paying orders are matched in the same round", 2 * k);
}

/// Round 17, what `alone` remembers; fixed in round 18. A set left alone is remembered with the script that was tried
/// on it. Two positions of the same two users with their sides the other way round can be novated and can be folded,
/// and a round tries the novation first, on the same two outpoints in the same order as the fold. With a gain asked
/// that neither pays, the novation is left alone (in both orders) and the fold is built all the same, and left alone
/// in its turn. (In round 17 the fold was not built behind the novation.)
#[test]
fn t1b_fixed_a_novation_left_alone_does_not_hide_the_fold_of_the_same_two_positions() {
    let mut s = setup("alone");
    let (me, exec, market, genesis) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone());
    let (anchor, open_s, registry) = (s.anchor.clone(), s.open.clone(), s.registry.clone());
    let payer = s.h.key("payer");
    // The position of the setup: `me` long 10 against `exec`. Its mirror: `exec` long 4 against `me`.
    let d = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "4", "--dir1", "long", "--x1", "10", "--a", &exec, "--b", &me, "--a-margin", "30", "--b-margin", "30", "--dispute-daa", "50", "--daa", "33100"]);
    let y = s.h.obj(&d, "position", 33_110);
    let folder = s.h.p("folder");
    std::fs::create_dir_all(&folder).unwrap();
    put_in(&folder, &s.position.clone());
    put_in(&folder, &y);
    let f = s.h.funding(50);
    let lines = |out: &str, word: &str| -> Vec<String> { out.lines().filter(|l| l.trim_start().starts_with(word)).map(|l| l.trim().to_string()).collect() };
    // What a line says the sender would be left, in KAS.
    let gain = |line: &str| -> f64 { line.split("leave the sender ").nth(1).and_then(|r| r.split(" KAS").next()).and_then(|x| x.parse().ok()).unwrap_or_else(|| panic!("no gain in `{line}`")) };
    let fold_only = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &["--do", "merge", "--min-gain", "5"]);
    let folds = lines(&fold_only, "fold (");
    assert!(folds.len() == 1 && folds[0].contains("left alone") && fold_only.contains("0 sent"), "the fold alone is built and left alone: {fold_only}");
    let nov_only = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &["--do", "novate", "--min-gain", "5"]);
    let novs = lines(&nov_only, "novate (");
    assert!(novs.len() == 2 && novs.iter().all(|l| l.contains("left alone")), "the novation is built in both orders and left alone: {nov_only}");
    // Both asked for: the novation is left alone, and the fold of the same two outpoints is built and left alone too.
    let both = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &["--do", "novate,merge", "--min-gain", "5"]);
    assert!(lines(&both, "novate (").len() == 2 && lines(&both, "fold (").len() == 1 && both.matches("left alone").count() == 3 && both.contains("0 sent"), "the fold is built behind a novation left alone: {both}");
    assert_eq!(lines(&both, "fold ("), folds, "the same fold as when it is asked for alone");
    // With a gain asked between the two, the novation is sent and the two positions are then no longer free for the
    // fold; the fold asked for alone is left alone.
    let (gf, gn) = (gain(&folds[0]), gain(&novs[0]));
    assert!(gn > gf && gf > 0.0, "the novation pays its sender more than the fold: {gn} and {gf}");
    let between = format!("{:.6}", (gf + gn) / 2.0);
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &["--do", "novate,merge", "--min-gain", &between]);
    assert!(out.contains("1 sent") && lines(&out, "novate (").len() == 1 && lines(&out, "fold (").is_empty(), "between the two gains the novation is sent: {out}");
    let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &["--do", "merge", "--min-gain", &between]);
    assert!(out.contains("0 sent") && lines(&out, "fold (").len() == 1, "and the fold is left alone: {out}");
    println!("T1b R18 alone fixed: a novation left alone ({gn} KAS to the sender) does not hide the fold of the same two positions ({gf} KAS): both are built, each left alone once");
}

/// Round 19, the oracle fees at the command line. A library made with `--committee` has a fee pool, and `lib-check` says whose the
/// oracle fees are and what a copy keeps; a genesis file whose payload names another committee or another floor than its
/// fee pool's, or none, or that has a second fee pool, a key's output or nothing in the fee pool's place, is refused (and no
/// position is opened in it). In the library as made, an operation leaves 0.002 KAS in the copy it used, and an
/// executor's round with nothing to sweep sends nothing. `fee-sweep` and `fee-pay` read the node: not run here.
#[test]
fn t1b_r19_a_library_with_a_fee_pool_at_the_command_line() {
    let mut s = setup("fee pool");
    let (me, exec, market) = (s.me.clone(), s.exec.clone(), s.market.clone());
    let funder = s.h.key("funder2");
    let committee = "52".repeat(32);
    let ld = s.h.build(&funder, &["lib-create", "--copies", "1", "--value", "7", "--committee", &committee]);
    let genesis = tx_file(&ld);
    let good = s.h.ok(&["lib-check", "--lib", &genesis]);
    assert!(good.contains("its genesis writes the family's scripts and nothing else") && good.contains(&format!("for the committee {committee} (a copy keeps 7 KAS)")), "{good}");
    let base = read(&genesis);
    let text = serde_json::to_string(&base).unwrap();
    // (Round 51: 6 KAS a copy; at 5 the genesis of a library of 63 scripts is above the storage mass of a block.
    // 17.8e: 7 KAS, with 75 scripts.)
    let floor = "0027b92900000000";
    assert!(text.contains(&format!("{committee}{floor}")), "the payload names the committee and the floor: {}", &text[..text.len().min(300)]);
    let mut v = base.clone();
    let outs = outputs(&mut v).clone();
    let bound: Vec<usize> = (0..outs.len()).filter(|i| !outs[*i]["covenant"].is_null()).collect();
    let plain: Vec<usize> = (0..outs.len()).filter(|i| outs[*i]["covenant"].is_null()).collect();
    // (Round 31, F55 fix: a one-copy library writes a second KpfTCite, so the scripts, that extra copy, and the fee pool.)
    assert_eq!(bound.len(), kpf_protocol::thin_artifacts::SCRIPTS.len() + 2, "one copy of each of the scripts, a second KpfTCite, and the fee pool");
    let fee_pool = *bound.last().unwrap();
    assert_eq!(outs[fee_pool]["value"].as_u64(), Some(7 * KAS), "the fee pool holds what a copy holds");
    let key_spk = outs[plain[0]]["scriptPublicKey"].clone();
    let mut cases: Vec<(&str, String)> = vec![
        ("a payload that names another committee", text.replace(&format!("{committee}{floor}"), &format!("{}{floor}", "53".repeat(32)))),
        ("a payload that names another floor", text.replace(&format!("{committee}{floor}"), &format!("{committee}0165cd1d00000000"))),
        ("a payload cut short", text.replace(&format!("{committee}{floor}"), &committee)),
    ];
    let mut case = |what: &'static str, f: &dyn Fn(&mut Vec<Value>)| {
        let mut v = base.clone();
        f(outputs(&mut v));
        cases.push((what, serde_json::to_string(&v).unwrap()));
    };
    case("a second fee pool", &|o| { let c = o[fee_pool].clone(); o.push(c); });
    case("a key's output in the fee pool's place", &|o| o[fee_pool]["scriptPublicKey"] = key_spk.clone());
    case("a script of the family in the fee pool's place", &|o| o[fee_pool]["scriptPublicKey"] = o[bound[0]]["scriptPublicKey"].clone());
    case("the fee pool of another value", &|o| o[fee_pool]["value"] = json!(7 * KAS + 1));
    let n = cases.len();
    for (i, (what, t)) in cases.iter().enumerate() {
        assert_ne!(*t, text, "{what}: the file is changed");
        let f = s.h.p(&format!("bad-fee pool-{i}.json"));
        std::fs::write(&f, t).unwrap();
        let (ok, out, err) = s.h.run(&["lib-check", "--lib", &f]);
        assert!(!ok && !out.contains("nothing else"), "lib-check takes {what}:\n{out}");
        println!("T1b R19 cli: {what}: {}", err.lines().next().unwrap_or("").chars().take(140).collect::<String>());
    }
    // ---- the library as made: a registry of it, a position, a margin added. The copy used goes on with 0.002 KAS more.
    let bd = s.h.build(&funder, &["registry-create", "--lib", &genesis, "--copies", "1"]);
    let (registry, anchor) = (tx_file(&bd), s.h.obj(&bd, "anchor", 33_060));
    let open_s = s.h.obj(&ld, "script-KpfTOpen", 33_050);
    let d = s.h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "10", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33100"]);
    let position = s.h.obj(&d, "position", 33_100);
    // What a dry run's transaction writes under the library's id: the copy it used, going on.
    let lib_id = outs[bound[0]]["covenant"]["covenantId"].clone();
    let copy_value = |dir: &str| -> Vec<u64> { outputs(&mut read(&tx_file(dir))).iter().filter(|o| o["covenant"]["covenantId"] == lib_id).map(|o| o["value"].as_u64().unwrap()).collect() };
    assert_eq!(copy_value(&d), vec![7 * KAS + 200_000], "the opening left its oracle fees in the copy of KpfTOpen");
    let add_s = s.h.obj(&ld, "script-KpfTAdd", 33_050);
    let d = s.h.build(&me, &["pos-add-margin", "--position", &position, "--script", &add_s, "--side", "a", "--amount", "5"]);
    assert_eq!(copy_value(&d), vec![7 * KAS + 200_000], "the margin added left its oracle fees in the copy of KpfTAdd");
    // The copy as it then stands, used again: another 0.002 KAS (the copy's file with what it now holds).
    let position2 = s.h.obj(&d, "position", 33_200);
    let mut c = read(&add_s);
    c["amount"] = json!(7 * KAS + 200_000);
    let again = s.h.p("script-again.json");
    std::fs::write(&again, serde_json::to_string_pretty(&c).unwrap()).unwrap();
    let d = s.h.build(&me, &["pos-add-margin", "--position", &position2, "--script", &again, "--side", "a", "--amount", "5"]);
    assert_eq!(copy_value(&d), vec![7 * KAS + 400_000]);
    // ---- an executor's round over a folder of that library, offline: the copies are the genesis' (nothing gathered).
    let folder = s.h.p("folder");
    std::fs::create_dir_all(&folder).unwrap();
    put_in(&folder, &position);
    let f = s.h.funding(50);
    let payer = s.h.key("payer");
    for extra in [vec![], vec!["--do", "sweep"]] {
        let out = exec_round(&s.h, &folder, &genesis, &registry, &payer, &f, "34000", &extra);
        assert!(out.contains("0 sent") && !out.contains("swept"), "nothing to sweep at the genesis' values: {out}");
    }
    // Without a node the two commands of the oracle fees say so.
    for cmd in ["fee-sweep", "fee-pay"] {
        assert!(s.h.fail(&[cmd, "--lib", &genesis, "--funder", &payer, "--funding", &f]).contains("read at the node"));
    }
    println!("T1b R19 cli: {n} genesis files whose fee pool is not the one their payload names refused by lib-check; an opening and two added margins leave 0.002 KAS each in the copy used; an executor's round with nothing gathered sweeps nothing");
}

// ---------------------------------------------------------------- round 21: the watchtower of exec-run, and what stands under a committee

/// A committee of three members (30, 22 and 15 KAS: member 0 is in every quorum of 67 %), rolled into the epoch
/// [33,000, 36,000), a market of it (a claim lands within 100 DAA of its time), a library, a registry, and a position of
/// `me` (long 10 units at 10, 40 KAS) against `exec` (60 KAS), dispute period 50.
struct GW {
    h: H,
    me: String,
    exec: String,
    members: Vec<String>,
    epoch_table: String,
    market: String,
    genesis: String,
    registry: String,
    lib_dir: String,
    anchor: String,
    position: String,
    /// The committee as it stands after the roll into the epoch [33,000, 36,000).
    committee: String,
}
fn gsetup(tag: &str, copies: &str) -> GW {
    let mut h = H::new(tag);
    let funder = h.key("funder");
    let (me, exec) = (h.key("me"), h.key("exec"));
    let members: Vec<String> = (0..3).map(|i| h.key(&format!("member{i}"))).collect();
    let committee_file = h.p("committee.json");
    let d = h.build(&funder, &["committee-create", "--daa", "30000", "--out", &committee_file]);
    let mut committee = h.obj(&d, "committee", 30_000);
    for (i, kas) in [(0usize, "30"), (1, "22"), (2, "15")] {
        let d = h.build(&funder, &["committee-join", "--committee", &committee, "--genesis", &committee_file, "--member", &i.to_string(), "--amount", kas, "--owner", &members[i]]);
        committee = h.obj(&d, "committee", 30_100 + i as u64);
    }
    let d = h.build(&funder, &["committee-roll", "--committee", &committee, "--daa", "33000"]);
    let epoch_table = h.obj(&d, "epoch table", 33_000);
    let committee = h.obj(&d, "committee", 33_000);
    let feed = h.p("feed.txt");
    std::fs::write(&feed, FEED).unwrap();
    let market = h.p("market.json");
    h.ok(&["c-market", "--committee", &committee_file, "--feed", &feed, "--out", &market]);
    let lib_dir = h.build(&funder, &["lib-create", "--copies", copies]);
    let genesis = tx_file(&lib_dir);
    let bd = h.build(&funder, &["registry-create", "--lib", &genesis, "--copies", "2"]);
    let (registry, anchor) = (tx_file(&bd), h.obj(&bd, "anchor", 33_060));
    let open_s = h.obj(&lib_dir, "script-KpfTOpen", 33_050);
    let d = h.build(&me, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &open_s, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "10", "--dir1", "long", "--x1", "10", "--a", &me, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33100"]);
    let position = h.obj(&d, "position", 33_100);
    GW { h, me, exec, members, epoch_table, market, genesis, registry, lib_dir, anchor, position, committee }
}
impl GW {
    fn dir(&self, name: &str) -> String {
        let d = self.h.p(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    /// A signed price signed by member `i`, in `into`, under the name a member's runner gives it or `name`.
    fn signed_price(&self, into: &str, i: usize, t0: u64, price: &str, name: Option<&str>) -> String {
        let f = match name {
            Some(n) => format!("{into}/{n}"),
            None => format!("{into}/price-{t0}-member{i}.json"),
        };
        self.h.ok(&["price-sign", "--market", &self.market, "--t0", &t0.to_string(), "--price", price, "--key", &self.members[i], "--out", &f]);
        f
    }
    fn all(&self, into: &str, t0: u64, price: &str) -> Vec<String> {
        (0..3).map(|i| self.signed_price(into, i, t0, price, None)).collect()
    }
    /// A round of the watchtower for `me` over `folder`, with the signed prices of `signed prices`, at `daa`.
    fn guard(&self, folder: &str, signed_prices: &str, funding: &str, daa: &str, extra: &[&str]) -> String {
        let mut a = vec!["exec-run", "--dir", folder, "--lib", &self.genesis, "--registry", &self.registry, "--funder", &self.me, "--funding", funding, "--daa", daa, "--do", "watch", "--signed-prices", signed_prices, "--epoch-table", &self.epoch_table];
        a.extend(extra);
        self.h.ok(&a)
    }
    fn script(&self, name: &str) -> String {
        self.h.obj(&self.lib_dir, &format!("script-{name}"), 33_050)
    }
    /// Member `i` signs without the tool's own record of what it signed (`price-sign` refuses a second price for one
    /// signed price from the key's record; a member that means to sign two keeps none).
    fn forget(&self, i: usize) {
        let _ = std::fs::remove_file(format!("{}.signed.jsonl", self.members[i]));
    }
}
const CLAIMED: &str = "claimed at the last one (11 KAS at DAA 33400)";
const NOTHING: &str = "no price a quorum signed in the last 100 DAA";
const TWICE: &str = "two prices for one time are proof against it";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
/// A signed price as `price-sign` writes one, signed with a member's key file for the committee of the time file `like`, and for
/// its market or the one given: what a member that does not use the tool's own record can sign (sompi a unit).
fn craft(key_file: &str, like: &str, market: Option<[u8; 32]>, t0: i64, price: i64) -> String {
    let secret = unhex(read(key_file)["secretKey"].as_str().expect("a key file's secret"));
    let kp = secp256k1::Keypair::from_secret_key(&secp256k1::Secp256k1::new(), &secp256k1::SecretKey::from_slice(&secret).expect("a secret key"));
    let l = read(like);
    let committee: [u8; 32] = unhex(l["committeeId"].as_str().unwrap()).try_into().unwrap();
    let m: [u8; 32] = market.unwrap_or_else(|| unhex(l["market"].as_str().unwrap()).try_into().unwrap());
    let t = kpf_protocol::committee::sign_price(&kp, &committee, &m, t0, price);
    serde_json::to_string(&json!({ "format": "kpf-signed-price/1", "committeeId": hex(&committee), "market": hex(&m), "t0": t0, "price": price, "key": hex(&t.key), "sig": hex(&t.sig) })).unwrap()
}

/// F25 (High in round 21), fixed in round 22. The watchtower looked at the 60 newest signed prices of its folder for one a quorum
/// signed; sixty signed prices newer than the last quorum's, each signed by one member, or sixty files that are no signed prices, hid
/// it. It now looks at every signed price of the time a claim may land in. The attack as it was built: the last quorum's
/// signed price is 70 DAA old, and 60 newer signed prices of the smallest member alone (or 60 files of `{}`) are in the folder: the
/// position is claimed at the last quorum's signed price all the same.
#[test]
fn t1b_fixed_f25_ticks_no_quorum_signed_do_not_hide_the_last_one_a_quorum_did() {
    let mut g = gsetup("flood", "1");
    let (folder, signed_prices) = (g.dir("folder"), g.dir("signed prices"));
    put_in(&folder, &g.position);
    g.all(&signed_prices, 33_300, "10.5");
    g.all(&signed_prices, 33_400, "11");
    let f = g.h.funding(50);
    let out = g.guard(&folder, &signed_prices, &f, "33470", &[]);
    assert!(out.contains(CLAIMED) && out.contains("1 sent"), "the watchtower as it is meant: {out}");
    let (flood, junk) = (g.dir("signed prices-flood"), g.dir("signed prices-junk"));
    for dir in [&flood, &junk] {
        for i in 0..3 {
            put_in(dir, &format!("{signed_prices}/price-33400-member{i}.json"));
            put_in(dir, &format!("{signed_prices}/price-33300-member{i}.json"));
        }
    }
    for t0 in 33_401..=33_460u64 {
        g.signed_price(&flood, 2, t0, "11", None);
        std::fs::write(format!("{junk}/price-{t0}-x.json"), "{}").unwrap();
    }
    // (A folder of its own for each, so that nothing kept from the round above helps.)
    for (what, dir) in [("signed by the smallest member", &flood), ("that are no signed prices", &junk)] {
        let fresh = g.dir(&format!("folder-{}", if what.starts_with("signed") { "flood" } else { "junk" }));
        put_in(&fresh, &g.position);
        let out = g.guard(&fresh, dir, &f, "33470", &[]);
        assert!(out.contains(CLAIMED) && out.contains("1 sent"), "F25 is open again: 60 newer signed prices {what} hide the last quorum's: {out}");
    }
    println!("T1b F25 fixed: 60 signed prices newer than the last a quorum signed, each of one member (or 60 files that are no signed prices): the position is claimed at the last quorum's signed price");
}

/// F26 (High in round 21), fixed in round 22. The watchtower proved a disputed claim of its key only if the price it then
/// collected for the claim's signed price was the claim's price; another signed price of the time, arriving later, moved what
/// it collected and the claim lapsed. It now attests exactly the claim's price from every signed price it has. The
/// attack as it was built: a claim at 11 from two members (a quorum), disputed, and the third member's 11.008 for the
/// same time (inside the spread) arriving after: proved.
#[test]
fn t1b_fixed_f26_the_guard_proves_its_claim_at_the_price_it_states() {
    let mut g = gsetup("proof", "1");
    let (me, exec, epoch_table, position) = (g.me.clone(), g.exec.clone(), g.epoch_table.clone(), g.position.clone());
    let two = g.dir("signed prices-two");
    let files = [g.signed_price(&two, 0, 33_400, "11", None), g.signed_price(&two, 1, 33_400, "11", None)];
    let folder = g.dir("folder");
    put_in(&folder, &position);
    let f = g.h.funding(50);
    assert!(g.guard(&folder, &two, &f, "33450", &[]).contains(CLAIMED));
    let (s_decl, s_disp) = (g.script("KpfTClose"), g.script("KpfTDispute"));
    let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_decl, "--side", "a", "--epoch-table1", &epoch_table, "--signed-price1", &files[0], "--signed-price1", &files[1]]);
    let claim = g.h.obj(&d, "claim", 33_410);
    let d = g.h.build(&exec, &["pos-dispute", "--claim", &claim, "--script", &s_disp]);
    let dis = g.h.obj(&d, "disputed claim", 33_420);
    // The third member's price is there before the watchtower ever reads the signed price for this claim (a folder of its own:
    // nothing kept from another round).
    let three = g.dir("signed prices-three");
    for x in &files {
        put_in(&three, x);
    }
    g.signed_price(&three, 2, 33_400, "11.008", None);
    let folder2 = g.dir("folder2");
    put_in(&folder2, &dis);
    let out = g.guard(&folder2, &three, &f, "33440", &[]);
    assert!(out.contains("proved with the committee's signatures") && out.contains("1 sent"), "F26 is open again: {out}");
    // A claim of the key declared by hand from the three files (at whatever price the command collects) is proved
    // too, and so is the claim at 11 with only the third member's price beside ONE of the two (15 and 30 of 67 KAS
    // are a quorum, and 11.008 is within the spread of 11).
    let all: Vec<String> = (0..3).map(|i| format!("{three}/price-33400-member{i}.json")).collect();
    let mut args = vec!["pos-close", "--position", &position, "--script", &s_decl, "--side", "a", "--epoch-table1", &epoch_table];
    for t in &all {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let by_hand = g.h.obj(&d, "claim", 33_410);
    let d = g.h.build(&exec, &["pos-dispute", "--claim", &by_hand, "--script", &s_disp]);
    let folder3 = g.dir("folder3");
    put_in(&folder3, &g.h.obj(&d, "disputed claim", 33_420));
    assert!(g.guard(&folder3, &three, &f, "33440", &[]).contains("proved with the committee's signatures"), "a claim declared by hand from the three");
    let odd = g.dir("signed prices-odd");
    put_in(&odd, &all[0]);
    put_in(&odd, &all[2]);
    let folder4 = g.dir("folder4");
    put_in(&folder4, &dis);
    assert!(g.guard(&folder4, &odd, &f, "33440", &[]).contains("proved with the committee's signatures"), "the claim at 11 from 11 and 11.008");
    // Without a quorum within the spread of the claim's price: said, with the reason.
    let one = g.dir("signed prices-one");
    put_in(&one, &all[1]);
    put_in(&one, &all[2]);
    let folder5 = g.dir("folder5");
    put_in(&folder5, &dis);
    let out = g.guard(&folder5, &one, &f, "33440", &[]);
    assert!(out.contains("are no quorum") && out.contains("lapses") && out.contains("0 sent"), "{out}");
    println!("T1b F26 fixed: a claim at 11 (two members) is proved with the third member's 11.008 in the folder; a claim declared by hand from the three, and the claim at 11 from 11 and 11.008, are proved; without a quorum near its price the watchtower says so");
}

/// F27 (High in round 21), fixed in round 22. An attestation took the first signed price given of each member, so a
/// member's second price for a signed price, read first or written over its first, took the signed price's quorum away. Every signed
/// price the watchtower has read is now kept under a name of its own and all of a member's prices are used. The attack as
/// it was built: every member signed 11 at the last signed price and the watchtower read it; member 0 then signs 20 for that time,
/// written over its first file or under a name read before it. The position is claimed at the signed price and the disputed
/// claim made at it is proved, and the watchtower says the member signed twice.
#[test]
fn t1b_fixed_f27_a_members_second_signed_price_takes_nothing_away() {
    let mut g = gsetup("twice", "1");
    let (me, exec, epoch_table, position) = (g.me.clone(), g.exec.clone(), g.epoch_table.clone(), g.position.clone());
    let signed_prices = g.dir("signed prices");
    g.all(&signed_prices, 33_300, "10.5");
    let last = g.all(&signed_prices, 33_400, "11");
    let folder = g.dir("folder");
    put_in(&folder, &position);
    let f = g.h.funding(50);
    let out = g.guard(&folder, &signed_prices, &f, "33450", &[]);
    assert!(out.contains(CLAIMED) && !out.contains(TWICE), "{out}");
    let (s_decl, s_disp) = (g.script("KpfTClose"), g.script("KpfTDispute"));
    let mut args = vec!["pos-close", "--position", &position, "--script", &s_decl, "--side", "a", "--epoch-table1", &epoch_table];
    for t in &last {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let claim = g.h.obj(&d, "claim", 33_410);
    let d = g.h.build(&exec, &["pos-dispute", "--claim", &claim, "--script", &s_disp]);
    let dis = g.h.obj(&d, "disputed claim", 33_420);
    let folder2 = g.dir("folder2");
    put_in(&folder2, &dis);
    assert!(g.guard(&folder2, &signed_prices, &f, "33440", &[]).contains("proved with the committee's signatures"));
    let (over, first) = (g.dir("signed prices-over"), g.dir("signed prices-first"));
    for dir in [&over, &first] {
        for e in std::fs::read_dir(&signed_prices).unwrap() {
            put_in(dir, &e.unwrap().path().display().to_string());
        }
    }
    g.forget(0);
    let second = g.signed_price(&over, 0, 33_400, "20", Some("second.json"));
    std::fs::copy(&second, format!("{over}/price-33400-member0.json")).unwrap();
    std::fs::remove_file(&second).unwrap();
    g.forget(0);
    g.signed_price(&first, 0, 33_400, "20", Some("price-33400-a.json"));
    for (what, dir) in [("written over its first", &over), ("read before its first", &first)] {
        let out = g.guard(&folder, dir, &f, "33450", &[]);
        assert!(out.contains(CLAIMED) && out.contains("1 sent"), "F27 is open again: a second price {what}: the position: {out}");
        assert!(out.contains(TWICE) && out.contains("signed 2 prices for the time 33400 (11 KAS and 20 KAS)") || out.contains("(20 KAS and 11 KAS)"), "the member is said to have signed twice: {out}");
        let out = g.guard(&folder2, dir, &f, "33440", &[]);
        assert!(out.contains("proved with the committee's signatures") && out.contains("1 sent"), "F27 is open again: a second price {what}: the disputed claim: {out}");
    }
    // What was read is kept in the folder, under a name of the time, the market, the member and the price.
    let kept: Vec<String> = std::fs::read_dir(format!("{folder}/prices-kept")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    let kept_prices: Vec<u64> = kept.iter().map(|n| read(&format!("{folder}/prices-kept/{n}"))["price"].as_u64().unwrap()).collect();
    assert!(kept.iter().all(|n| n.starts_with("price-33400-")) && kept_prices.contains(&1_100_000_000) && kept_prices.contains(&2_000_000_000), "{kept:?} {kept_prices:?}");
    println!("T1b F27 fixed: member 0's second signed price for the last signed price, written over its first or read before it, takes nothing away: the position is claimed at the signed price, the claim made at it is proved, and the member is said to have signed twice; {} prices kept", kept.len());
}

/// F28 (Medium in round 21), fixed in round 22 (the part a single round shows). The watchtower's epoch tables were read once;
/// they are now read at the start of every round (from the code: `Watchtower::open` in `turn`), and a signed price no epoch table given
/// covers is said to be that. With the epoch table of the epoch that ended at 36,000 alone, signed prices of 36,100 signed by every
/// member give nothing to claim by, and the watchtower says an epoch table is missing; with the next epoch's epoch table beside it in
/// `--epoch-tables`, the position is claimed at 36,100.
#[test]
fn t1b_fixed_f28_a_price_of_an_epoch_no_epoch_table_covers_is_said_and_the_new_epoch_table_is_taken() {
    let mut g = gsetup("rolled", "1");
    let (folder, signed_prices) = (g.dir("folder"), g.dir("signed prices"));
    put_in(&folder, &g.position);
    let f = g.h.funding(50);
    g.all(&signed_prices, 35_900, "11");
    assert!(g.guard(&folder, &signed_prices, &f, "35950", &[]).contains("claimed at the last one (11 KAS at DAA 35900)"));
    let next = g.dir("signed prices-next");
    g.all(&next, 36_100, "11");
    let folder2 = g.dir("folder2");
    put_in(&folder2, &g.position);
    let out = g.guard(&folder2, &next, &f, "36150", &[]);
    assert!(out.contains("no price a quorum signed in the last 100 DAA") && out.contains("1 of them are of a time no epoch table given covers") && out.contains("0 sent"), "{out}");
    // The committee rolls; the new epoch's epoch table lands in the folder of epoch tables.
    let funder = g.h.key("roller");
    let committee = g.committee.clone();
    let d = g.h.build(&funder, &["committee-roll", "--committee", &committee, "--daa", "36000"]);
    let epoch_table2 = g.h.obj(&d, "epoch table", 36_000);
    let epoch_tables = g.dir("epoch tables");
    put_in(&epoch_tables, &g.epoch_table);
    let run = |g: &GW, daa: &str| g.h.ok(&["exec-run", "--dir", &folder2, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.me, "--funding", &f, "--daa", daa, "--do", "watch", "--signed-prices", &next, "--epoch-tables", &epoch_tables]);
    assert!(run(&g, "36150").contains("no epoch table given covers"));
    put_in(&epoch_tables, &epoch_table2);
    let out = run(&g, "36150");
    assert!(out.contains("claimed at the last one (11 KAS at DAA 36100)") && out.contains("1 sent"), "F28 is open again: {out}");
    println!("T1b F28 fixed: with the old epoch's epoch table alone the watchtower says no epoch table covers the new signed prices; with the new epoch's epoch table in --epoch-tables the position is claimed at 36,100");
}

/// F29 (Low in round 21), fixed in round 22. `--max-exposure` was checked for each pair against what stood under the
/// committee when the round began. A round now adds what it has matched. The attack as it was built: half of a stake of
/// 67 KAS is 33.5 KAS, and of two pairs of 24 KAS the first is matched and the second is left unmatched.
#[test]
fn t1b_fixed_f29_the_limit_under_a_committee_counts_what_the_round_has_matched() {
    let mut g = gsetup("limit", "1");
    let (me, exec, market, genesis, registry, anchor, epoch_table) = (g.me.clone(), g.exec.clone(), g.market.clone(), g.genesis.clone(), g.registry.clone(), g.anchor.clone(), g.epoch_table.clone());
    let (third, fourth, payer) = (g.h.key("third"), g.h.key("fourth"), g.h.key("payer"));
    let place_s = g.script("KpfTPlace");
    let folder = g.dir("folder");
    for (who, side, limit, daa) in [(&me, "long", "10.1", 33_900u64), (&exec, "short", "10", 33_910), (&third, "long", "10.2", 33_920), (&fourth, "short", "9.9", 33_930)] {
        let d = g.h.build(who, &["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--dispute-daa", "50", "--side", side, "--size", "3", "--limit", limit, "--margin", "12", "--tip", "1", "--expiry", "40000"]);
        put_in(&folder, &g.h.obj(&d, "order", daa));
    }
    let coins = g.h.p("two-coins.json");
    std::fs::write(&coins, serde_json::to_string(&json!([{ "txid": format!("{:064x}", 0x5eed_aa01u64), "index": 0, "amount": 50 * KAS, "daa": 0 }, { "txid": format!("{:064x}", 0x5eed_aa02u64), "index": 0, "amount": 50 * KAS, "daa": 0 }])).unwrap()).unwrap();
    let run = |g: &GW, max: &str| g.h.ok(&["exec-run", "--dir", &folder, "--lib", &genesis, "--registry", &registry, "--funder", &payer, "--funding", &coins, "--daa", "34000", "--do", "match", "--epoch-table", &epoch_table, "--max-exposure", max]);
    let out = run(&g, "0.3");
    assert!(out.contains("0 sent") && out.contains("left unmatched"), "{out}");
    let out = run(&g, "0.5");
    assert!(out.contains("1 sent") && out.contains("left unmatched") && !out.contains("no copy of KpfTMatch is free"), "F29 is open again: the second pair of the round passes the limit: {out}");
    // A limit both pairs are within: the second goes on to be built (offline a round holds one copy of the script).
    let out = run(&g, "1");
    assert!(out.contains("1 sent") && out.contains("no copy of KpfTMatch is free") && !out.contains("left unmatched"), "{out}");
    println!("T1b F29 fixed: a limit of 33.5 KAS and two pairs of 24 KAS: the first is matched and the second left unmatched in the same round");
}

/// Round 22, the best price and the dispute period. Of the prices members signed that a quorum stands near, the watchtower states
/// the one that gives its side most: a member's lower second price does not lower it, a higher one within the spread
/// raises it, and the claim it states is the one it proves, also after the file of that price has gone from the
/// signed prices' folder (it is kept). A signed price exactly as old as a claim may be is too old to claim at and said so; a signed price a
/// DAA older is no longer looked at.
#[test]
fn t1b_r22_the_guard_states_the_best_signed_price_and_proves_what_it_states() {
    let mut g = gsetup("best", "1");
    let (exec, position) = (g.exec.clone(), g.position.clone());
    let (folder, signed_prices) = (g.dir("folder"), g.dir("signed prices"));
    put_in(&folder, &position);
    let real = g.all(&signed_prices, 33_400, "11");
    let f = g.h.funding(50);
    // Member 0 also signs 10.99: the user is long, and is claimed for at 11.
    std::fs::write(format!("{signed_prices}/price-33400-low.json"), craft(&g.members[0], &real[0], None, 33_400, 1_099_000_000)).unwrap();
    let out = g.guard(&folder, &signed_prices, &f, "33450", &[]);
    assert!(out.contains(CLAIMED) && out.contains(TWICE), "a lower second price does not lower the claim: {out}");
    // And 11.01, within the spread of the others' 11: claimed at 11.01.
    let high = format!("{signed_prices}/price-33400-high.json");
    std::fs::write(&high, craft(&g.members[0], &real[0], None, 33_400, 1_101_000_000)).unwrap();
    let folder = g.dir("folder-high");
    put_in(&folder, &position);
    let out = g.guard(&folder, &signed_prices, &f, "33450", &[]);
    assert!(out.contains("claimed at the last one (11.01 KAS at DAA 33400)") && out.contains("signed 3 prices"), "{out}");
    // The claim the watchtower itself wrote, disputed by the counterparty: proved; and again with the file of 11.01 gone.
    let claim = g.h.obj(&format!("{folder}/dry-run"), "claim", 33_455);
    assert_eq!(read(&claim)["amount"].as_u64().is_some(), true);
    let s_disp = g.script("KpfTDispute");
    let d = g.h.build(&exec, &["pos-dispute", "--claim", &claim, "--script", &s_disp]);
    let dis = g.h.obj(&d, "disputed claim", 33_460);
    // (The same folder: the round above kept what it read.)
    std::fs::remove_file(format!("{folder}/{}", std::path::Path::new(&position).file_name().unwrap().to_string_lossy())).unwrap();
    put_in(&folder, &dis);
    assert!(g.guard(&folder, &signed_prices, &f, "33470", &[]).contains("proved with the committee's signatures"));
    std::fs::remove_file(&high).unwrap();
    let out = g.guard(&folder, &signed_prices, &f, "33470", &[]);
    assert!(out.contains("proved with the committee's signatures") && out.contains("1 sent"), "what the watchtower stated it proves from what it kept: {out}");
    // ---- the dispute period.
    let folder2 = g.dir("folder2");
    put_in(&folder2, &position);
    let plain = g.dir("signed prices-plain");
    for x in &real {
        put_in(&plain, x);
    }
    let out = g.guard(&folder2, &plain, &f, "33500", &[]);
    assert!(out.contains("it cannot be claimed at that price any more") && out.contains("0 sent"), "a signed price as old as a claim may be: {out}");
    let out = g.guard(&folder2, &plain, &f, "33501", &[]);
    assert!(out.contains(NOTHING) && out.contains("of 0 signed prices of that time there") && out.contains("0 sent"), "a signed price a DAA older: {out}");
    // A silence that leaves no time is said.
    assert!(g.guard(&folder2, &plain, &f, "33450", &["--silence", "90"]).contains("it is never claimed"));
    println!("T1b R22: the watchtower claims at the highest signed price a quorum stands near (11 with a second price of 10.99, 11.01 with one of 11.01), proves the claim it wrote, also with that price's file gone; a signed price 100 DAA old is too old and said so, one 101 DAA old is not looked at");
}

/// Round 22, the cost of a round, measured. Files that are no signed prices under signed price names of the time looked at, names of
/// times long past, and one member's many prices for one time: the watchtower still claims, and a round takes this long
/// (the binary of a test build). The last grows with the square of the number of prices one member signed for one
/// signed price, each of which is proof against the member.
#[test]
fn t1b_design_what_a_full_folder_of_ticks_costs_a_round() {
    let mut g = gsetup("cost", "1");
    let (folder, signed_prices) = (g.dir("folder"), g.dir("signed prices"));
    put_in(&folder, &g.position);
    let real = g.all(&signed_prices, 33_400, "11");
    let f = g.h.funding(50);
    let timed = |g: &GW, what: &str| -> u128 {
        let t = std::time::Instant::now();
        let out = g.guard(&folder, &signed_prices, &f, "33450", &[]);
        assert!(out.contains(CLAIMED), "{what}: {out}");
        t.elapsed().as_millis()
    };
    let plain = timed(&g, "the three signed prices");
    // 3,000 files of `{}` over the 50 newer signed prices, 20,000 names of signed prices long past, one file of 8 kB and one above.
    for i in 0..3_000u64 {
        std::fs::write(format!("{signed_prices}/price-{}-junk{i}.json", 33_401 + i % 50), "{}").unwrap();
    }
    for i in 0..20_000u64 {
        std::fs::write(format!("{signed_prices}/price-{}-old{i}.json", 1_000 + i), "{}").unwrap();
    }
    std::fs::write(format!("{signed_prices}/price-33400-big.json"), format!("{{\"x\":\"{}\"}}", "a".repeat(8_100))).unwrap();
    std::fs::write(format!("{signed_prices}/price-33400-bigger.json"), format!("{{\"x\":\"{}\"}}", "a".repeat(100_000))).unwrap();
    let junk = timed(&g, "23,000 files that are no signed prices");
    // Member 2 signs 400 prices for the time, far from the others' (no quorum near any).
    for i in 0..400i64 {
        std::fs::write(format!("{signed_prices}/price-33400-many{i}.json"), craft(&g.members[2], &real[0], None, 33_400, 3_000_000_000 + i * 10_000_000)).unwrap();
    }
    let many = timed(&g, "400 prices of one member for the time");
    let kept = std::fs::read_dir(format!("{folder}/prices-kept")).unwrap().count();
    assert!(kept >= 403, "every signed price read is kept: {kept}");
    println!("T1b DESIGN (round 22) the cost of a watchtower's round: {plain} ms with three signed prices; {junk} ms with 3,000 files of no signed price in the time looked at and 20,000 names of times past; {many} ms with 400 prices of one member for one time ({kept} files kept, none ever removed)");
}

/// Round 21, what holds at the command line. A file that is no signed price, a signed price of another market or of a key that is no
/// member, a signed price whose signature is not the member's, and a signed price of a time to come add nothing to a quorum and stop
/// nothing; the settle of the watchtower and the settle of the round send one transaction; `--do all` runs without the
/// watchtower when no signed prices are given, and the default round is as it was.
#[test]
fn t1b_r21_the_guard_counts_only_what_members_signed_for_this_market_and_this_price() {
    let mut g = gsetup("counts", "1");
    let (me, epoch_table, position) = (g.me.clone(), g.epoch_table.clone(), g.position.clone());
    let signed_prices = g.dir("signed prices");
    // Members 1 and 2 signed 11 at 33,400 (37 of 67 KAS: no quorum); member 0 signed 10.5 at 33,300 with them.
    g.all(&signed_prices, 33_300, "10.5");
    let s1 = g.signed_price(&signed_prices, 1, 33_400, "11", None);
    g.signed_price(&signed_prices, 2, 33_400, "11", None);
    let folder = g.dir("folder");
    put_in(&folder, &position);
    let f = g.h.funding(50);
    let quorum_at_400 = |out: &str| out.contains("DAA 33400");
    assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])));
    // What is added in member 0's place, none of which is member 0's signature of a price of this market at this signed price:
    // a file that is no JSON, an empty object, member 1's signed price under member 0's name, a signed price of a key that is no member,
    // member 0's signed price of another signed price's time under this signed price's name, member 0's signed price of this signed price with its price changed
    // in the file (the signature is of another price).
    std::fs::write(format!("{signed_prices}/price-33400-member0.json"), "not json").unwrap();
    assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])), "a file that is no JSON");
    std::fs::write(format!("{signed_prices}/price-33400-member0.json"), "{}").unwrap();
    assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])), "an empty object");
    std::fs::copy(&s1, format!("{signed_prices}/price-33400-member0.json")).unwrap();
    assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])), "member 1's signed price twice");
    let stranger = g.h.key("stranger");
    g.h.ok(&["price-sign", "--market", &g.market, "--t0", "33400", "--price", "11", "--key", &stranger, "--out", &format!("{signed_prices}/price-33400-member0.json")]);
    assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])), "a key that is no member");
    g.signed_price(&signed_prices, 0, 33_390, "11", Some("price-33400-member0.json"));
    assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])), "member 0's signed price of another time under this signed price's name");
    let honest = g.signed_price(&signed_prices, 0, 33_400, "10.2", Some("price-33400-member0.json"));
    let forged = std::fs::read_to_string(&honest).unwrap();
    let mut v: Value = serde_json::from_str(&forged).unwrap();
    let price_key = v.as_object().unwrap().iter().find(|(_, x)| x.as_u64() == Some(1_020_000_000) || x.as_str() == Some("10.2")).map(|(k, _)| k.clone());
    if let Some(k) = &price_key {
        v[k.as_str()] = if v[k.as_str()].is_string() { json!("11") } else { json!(1_100_000_000u64) };
        std::fs::write(&honest, serde_json::to_string(&v).unwrap()).unwrap();
        assert!(!quorum_at_400(&g.guard(&folder, &signed_prices, &f, "33450", &[])), "member 0's signature of 10.2 with 11 written in the file");
    }
    // Member 0's own time of 11: the quorum, and the claim.
    g.forget(0);
    g.signed_price(&signed_prices, 0, 33_400, "11", Some("price-33400-member0.json"));
    assert!(g.guard(&folder, &signed_prices, &f, "33450", &[]).contains(CLAIMED));
    // ---- a claim of the key past its dispute period: the watchtower's settle and the round's settle send one transaction.
    let s_decl = g.script("KpfTClose");
    let all: Vec<String> = (0..3).map(|i| format!("{signed_prices}/price-33400-member{i}.json")).collect();
    let mut args = vec!["pos-close", "--position", &position, "--script", &s_decl, "--side", "a", "--epoch-table1", &epoch_table];
    for t in &all {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let claim = g.h.obj(&d, "claim", 33_410);
    let folder2 = g.dir("folder2");
    put_in(&folder2, &claim);
    let out = g.h.ok(&["exec-run", "--dir", &folder2, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &me, "--funding", &f, "--daa", "33470", "--do", "watch,settle", "--signed-prices", &signed_prices, "--epoch-table", &epoch_table]);
    assert!(out.contains("watchtower: claim") && out.contains("1 sent") && !out.contains("no copy of KpfTSettle"), "{out}");
    // ---- `--do all` without signed prices runs without the guard; with signed prices it guards; the default round is as it was.
    let out = g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &me, "--funding", &f, "--daa", "33450", "--do", "all"]);
    assert!(out.contains("0 sent") && !out.contains("watch"), "{out}");
    let out = g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &me, "--funding", &f, "--daa", "33450", "--do", "all", "--signed-prices", &signed_prices, "--epoch-table", &epoch_table]);
    assert!(out.contains(CLAIMED) && out.contains("1 sent"), "{out}");
    let out = g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &me, "--funding", &f, "--daa", "33450"]);
    assert!(out.contains("0 sent") && !out.contains("watch"), "{out}");
    println!("T1b R21: six files in a member's place that are not its signature of this market's price at this signed price add nothing to a quorum (the price forged in the file tried: {}); the watchtower's settle and the round's are one transaction; `--do all` guards only with signed prices", price_key.is_some());
}

/// F30 (Medium in round 22), fixed in round 23. A kept price's file was named by prefixes of what was signed, and a
/// member's price for another market whose id began with the same 4 bytes took the name of its price for the real
/// one. The name is now a hash of everything signed (the committee, the market, the key, the time, the price). The
/// attack as it was built: every member signs 11 for the market; member 0 also signs 11 for a market id that differs
/// in its last byte, read first; then member 0's file is written over by its second price. The first price for the
/// real market was kept under its own name, and the position is claimed at the time all the same.
#[test]
fn t1b_fixed_f30_a_price_for_another_market_does_not_take_the_kept_name_of_the_real_one() {
    let mut g = gsetup("kept", "1");
    let (folder, prices) = (g.dir("folder"), g.dir("prices"));
    put_in(&folder, &g.position);
    let real = g.all(&prices, 33_400, "11");
    let market: [u8; 32] = unhex(read(&real[0])["market"].as_str().unwrap()).try_into().unwrap();
    let mut other = market;
    other[31] ^= 0xff;
    std::fs::write(format!("{prices}/price-33400-a.json"), craft(&g.members[0], &real[0], Some(other), 33_400, 1_100_000_000)).unwrap();
    let f = g.h.funding(50);
    let out = g.guard(&folder, &prices, &f, "33450", &[]);
    assert!(out.contains(CLAIMED) && !out.contains(TWICE), "both are there: claimed, and the member is not said to have signed twice (two markets): {out}");
    // What was kept: four prices, member 0's for each of the two markets among them.
    let member0 = read(&real[0])["key"].as_str().unwrap().to_string();
    let kept: Vec<Value> = std::fs::read_dir(format!("{folder}/prices-kept")).unwrap().map(|e| read(&e.unwrap().path().display().to_string())).collect();
    let of0: Vec<&str> = kept.iter().filter(|v| v["key"].as_str() == Some(member0.as_str())).map(|v| v["market"].as_str().unwrap()).collect();
    assert_eq!((kept.len(), of0.len()), (4, 2), "every signed price read is kept under a name of its own");
    assert!(of0.contains(&hex(&market).as_str()) && of0.contains(&hex(&other).as_str()));
    // Member 0's second price for the real market written over its first, as in F27.
    std::fs::write(&real[0], craft(&g.members[0], &real[1], None, 33_400, 2_000_000_000)).unwrap();
    let out = g.guard(&folder, &prices, &f, "33450", &[]);
    assert!(out.contains(CLAIMED) && out.contains("1 sent") && out.contains(TWICE), "F30 is open again: {out}");
    println!("T1b F30 fixed: member 0's price for another market with the same first bytes is kept beside its price for the real one; with its file written over by a second price the position is still claimed, and the member is said to have signed twice");
}

/// F31 (Low in round 22), fixed in round 23. One file in `--epoch-tables` that is no epoch table failed the whole
/// round. It is now passed over and said. The attack as it was built: a round that only matches and settles, a good
/// table and a half-written file beside it: the round runs.
#[test]
fn t1b_fixed_f31_a_bad_file_among_the_epoch_tables_is_passed_over() {
    let mut g = gsetup("badtable", "1");
    let folder = g.dir("folder");
    put_in(&folder, &g.position);
    let tables = g.dir("tables");
    put_in(&tables, &g.epoch_table);
    let f = g.h.funding(50);
    let args: Vec<String> = ["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.me, "--funding", &f, "--daa", "33450", "--epoch-tables", &tables, "--do", "match,settle"].iter().map(|s| s.to_string()).collect();
    let run = |g: &GW| g.h.run(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let (ok, out, _) = run(&g);
    assert!(ok && out.contains("0 sent"), "{out}");
    let name = "half-epoch-table.json".to_string();
    let bad = format!("{tables}/{name}");
    std::fs::write(&bad, "{\"format\": \"kpf-").unwrap();
    let (ok, out, err) = run(&g);
    assert!(ok && out.contains("0 sent"), "F31 is open again: a half-written file among the epoch tables fails the round: {err}");
    assert!(out.contains("passed over among the epoch tables") && out.contains(&name), "the file is said: {out} {err}");
    // The stake is still known from the good table.
    assert!(out.contains("the committee's stake is 67 KAS"), "{out}");
    println!("T1b F31 fixed: a half-written file among the epoch tables is passed over and said; the round runs with the tables that read");
}

/// A round of `exec-run --do dispute` for the key `who` over `folder`, 20 DAA after the claims' time.
fn dispute_round(g: &GW, folder: &str, who: &str, funding: &str, extra: &[&str]) -> String {
    let mut a = vec!["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", who, "--funding", funding, "--daa", "33420", "--do", "dispute", "--expiry-guard", "5"];
    a.extend(extra);
    g.h.ok(&a)
}
/// Two rounds of an executor with a price of its own for `market`: one at DAA 33,390, before the claims' time
/// (33,400), reading `p1`, and one at 33,420 reading `p2`. A claim is judged only between two readings (round 25):
/// the first round judges nothing, and what is returned is the second round's output.
fn own_rounds(g: &GW, folder: &str, who: &str, funding: &str, market: &str, p1: &str, p2: &str, extra: &[&str]) -> String {
    let run = |daa: &str, p: &str| -> String {
        let mut a = vec!["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", who, "--funding", funding, "--daa", daa, "--do", "dispute", "--expiry-guard", "5", "--market1", market, "--price", p];
        a.extend(extra);
        g.h.ok(&a)
    };
    let first = run("33390", p1);
    assert!(first.contains("0 sent"), "a round before the claim's time judges nothing: {first}");
    run("33420", p2)
}
const BY_OWN: &str = "disputed (not within the rule at any price near this executor's own)";
const BY_SIGNED: &str = "disputed (not within the rule at the prices the committee signed for its time)";

/// F33 (High in round 23), fixed in round 24; the control brought to round 25. `exec-run --do dispute` judged a claim
/// by the prices it states. It now judges by the committee's signed prices or by readings of a price of its own
/// around the claim's time, and with neither disputes nothing. The bait as it was built (a close stating 10.1 and
/// 44 KAS, which 10.5 gives): not disputed without a price, nor with readings of 10.5 each side of its time. THE
/// POSITIVE CONTROL: a close of 50 KAS, which no price near 10.5 gives, is disputed by those readings. With one
/// reading only (a single round), or a price source that says nothing in the round after the claim, it is left.
#[test]
fn t1b_fixed_f33_exec_run_disputes_no_claim_by_the_price_it_states() {
    let mut g = gsetup("bait", "1");
    let (me, position, market) = (g.me.clone(), g.position.clone(), g.market.clone());
    let exec = g.h.key("keeper");
    let s_close = g.script("KpfTClose");
    let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--t0", "33400", "--p1", "10.1", "--amount", "44"]);
    let bait = g.h.obj(&d, "claim", 33_410);
    let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--t0", "33400", "--p1", "11", "--amount", "50"]);
    let false_claim = g.h.obj(&d, "claim", 33_410);
    let f = g.h.funding(50);
    let folder_of = |g: &GW, name: &str, claim: &str| -> String {
        let d = g.dir(name);
        put_in(&d, claim);
        d
    };
    // The bait.
    let folder = folder_of(&g, "bait-none", &bait);
    let out = dispute_round(&g, &folder, &exec, &f, &[]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "F33 is open again: the executor disputes by the price a claim states: {out}");
    let out = own_rounds(&g, &folder_of(&g, "bait-own", &bait), &exec, &f, &market, "10.5", "10.5", &[]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "the bait with readings of 10.5: {out}");
    // The positive control: a claim no price near the executor's readings proves (10.5 gives 45; 0.8 % more, 45.84).
    let out = own_rounds(&g, &folder_of(&g, "false-own", &false_claim), &exec, &f, &market, "10.5", "10.5", &[]);
    assert!(out.contains(BY_OWN) && out.contains("1 sent"), "a false claim is disputed by the executor's own readings around its time: {out}");
    let own = std::fs::read_to_string(format!("{}/own-prices", g.h.p("false-own"))).unwrap();
    let lines: Vec<Vec<&str>> = own.lines().map(|l| l.split(' ').collect()).collect();
    assert!(lines.len() == 2 && lines.iter().all(|l| l.len() == 5 && l[0].len() == 16 && l[0] == lines[0][0]), "two readings, each with its market's tag: {own}");
    assert_eq!((&lines[0][1..], &lines[1][1..]), (&["33390", "1050000000", "1050000000", "1050000000"][..], &["33420", "1050000000", "1050000000", "1050000000"][..]), "the time, the price, and the price again as its low and high");
    // One reading only (a single round after the claim): the claim's time is not between two readings.
    let single = folder_of(&g, "false-single", &false_claim);
    let out = dispute_round(&g, &single, &exec, &f, &["--market1", &market, "--price", "10.5"]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "one reading does not judge: {out}");
    // Without a price of its own it is left.
    assert!(dispute_round(&g, &folder_of(&g, "false-none", &false_claim), &exec, &f, &[]).contains("0 sent"));
    // A price with no market named, or with two: said, and nothing is judged.
    let out = dispute_round(&g, &folder_of(&g, "false-nomarket", &false_claim), &exec, &f, &["--price", "10.5"]);
    assert!(out.contains("give exactly one --market1") && out.contains("0 sent"), "{out}");
    // A price file that says nothing in the round after the claim: said, no second reading, nothing disputed. Then
    // the file says 10.5 again, and the claim is disputed.
    let pf = g.h.p("price.txt");
    let filed = folder_of(&g, "false-file", &false_claim);
    let round = |g: &GW, daa: &str| g.h.ok(&["exec-run", "--dir", &filed, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &exec, "--funding", &f, "--daa", daa, "--do", "dispute", "--expiry-guard", "5", "--market1", &market, "--price-file", &pf]);
    std::fs::write(&pf, "10.5\n").unwrap();
    assert!(round(&g, "33390").contains("0 sent"));
    for junk in ["", "ten", "-3", "0"] {
        std::fs::write(&pf, junk).unwrap();
        let out = round(&g, "33420");
        assert!(out.contains("0 sent") && out.contains("no price of this executor's own this round"), "a price file holding `{junk}`: {out}");
    }
    std::fs::write(&pf, "10.5\n").unwrap();
    assert!(round(&g, "33420").contains(BY_OWN));
    // A margin that is no whole number of basis points is refused at the start.
    for bad in ["wide", "-5", "1.5"] {
        assert!(g.h.fail(&["exec-run", "--dir", &single, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &exec, "--funding", &f, "--daa", "33420", "--do", "dispute", "--dispute-margin-bps", bad]).contains("--dispute-margin-bps"), "{bad}");
    }
    println!("T1b F33 fixed: a close stating 10.1 and what 10.5 gives is not disputed without a price nor with readings of 10.5; a close of 50 KAS is disputed by readings of 10.5 each side of its time, and not by one reading, by none, by a price without its market, or after a price file that said nothing");
}

/// F34 (Medium in round 23), fixed in round 24; F38 (Medium in round 24), fixed in round 25. The executor disputed a
/// claim as soon as one price a quorum stands near refuted it, then asked only the prices members SIGNED for a
/// proof. It now disputes only where one attestable price refutes and none proves, the ends of the band among them.
/// The attacks as they were built (members sign 10.5, 10.5 and 10.508): a close of 45.08 KAS (what 10.508 gives) and
/// a close of 45.1 KAS (what 10.51 gives, between the highest signed price and the band's end at 10.5105) are not
/// disputed. A close of 45.11 KAS, a hundredth above what the end gives, and one of 46 KAS are.
#[test]
fn t1b_fixed_f34_f38_exec_run_does_not_dispute_what_an_attestable_price_proves() {
    let mut g = gsetup("band", "1");
    let (me, position, table) = (g.me.clone(), g.position.clone(), g.epoch_table.clone());
    let exec = g.h.key("keeper");
    let prices = g.dir("prices");
    g.signed_price(&prices, 0, 33_400, "10.5", None);
    g.signed_price(&prices, 1, 33_400, "10.5", None);
    g.signed_price(&prices, 2, 33_400, "10.508", None);
    let s_close = g.script("KpfTClose");
    let f = g.h.funding(50);
    let round = |g: &mut GW, amount: &str| -> String {
        let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--t0", "33400", "--p1", "10.51", "--amount", amount]);
        let folder = g.dir(&format!("folder-{amount}"));
        put_in(&folder, &g.h.obj(&d, "claim", 33_410));
        dispute_round(g, &folder, &exec, &f, &["--signed-prices", &prices, "--epoch-table", &table])
    };
    for (amount, what) in [("45.08", "F34"), ("45.1", "F38"), ("45.10510510", "F38, the band's end to the sompi")] {
        let out = round(&mut g, amount);
        assert!(out.contains("0 sent") && !out.contains("disputed"), "{what} is open again: a close of {amount} KAS: {out}");
    }
    for amount in ["45.11", "46"] {
        let out = round(&mut g, amount);
        assert!(out.contains(BY_SIGNED) && out.contains("1 sent"), "a close of {amount} KAS, which no attestable price gives: {out}");
    }
    println!("T1b F34 and F38 fixed: members sign 10.5, 10.5 and 10.508: closes of what 10.508, 10.51 and the band's end (10.5105) give are not disputed; closes of 45.11 and 46 KAS are");
}

/// F35 (High in round 24), fixed in round 25. The executor's one price judged the claims of every market. It is now
/// the price of one market, named with it. The attack as it was built: a true close on a second market (the
/// committee signed 50.5 there) and an executor whose price, of the first market, is 10: not disputed. And the
/// executor told its price is the second market's, with readings of 10 there: it disputes (its price is then simply
/// wrong for that market: what a price source is trusted with).
#[test]
fn t1b_fixed_f35_the_executors_price_judges_the_claims_of_its_market_only() {
    let mut g = gsetup("markets", "1");
    let (me, exec, table, market) = (g.me.clone(), g.exec.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let feed2 = g.h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = g.h.p("market2.json");
    g.h.ok(&["c-market", "--committee", &g.h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    let open_s = g.script("KpfTOpen");
    let d = g.h.build(&me, &["pos-open", "--lib", &g.genesis.clone(), "--anchor", &g.anchor.clone(), "--script", &open_s, "--registry", &g.registry.clone(), "--key", &exec, "--market1", &market2, "--size1", "10", "--dir1", "long", "--x1", "50", "--a", &me, "--b", &exec, "--a-margin", "400", "--b-margin", "600", "--dispute-daa", "50", "--daa", "33100"]);
    let position2 = g.h.obj(&d, "position", 33_100);
    let prices = g.dir("prices2");
    let signed: Vec<String> = (0..3)
        .map(|i| {
            let f = format!("{prices}/price-33400-member{i}.json");
            g.h.ok(&["price-sign", "--market", &market2, "--t0", "33400", "--price", "50.5", "--key", &g.members[i], "--out", &f]);
            f
        })
        .collect();
    let s_close = g.script("KpfTClose");
    let mut args = vec!["pos-close", "--position", &position2, "--script", &s_close, "--side", "a", "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let claim = g.h.obj(&d, "claim", 33_410);
    let f = g.h.funding(5_000);
    let folder = g.dir("folder");
    put_in(&folder, &claim);
    let out = own_rounds(&g, &folder, &keeper, &f, &market, "10", "10", &[]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "F35 is open again: a claim of another market is judged by the executor's price: {out}");
    // The same readings said to be the second market's.
    let folder2 = g.dir("folder2");
    put_in(&folder2, &claim);
    let out = own_rounds(&g, &folder2, &keeper, &f, &market2, "10", "10", &[]);
    assert!(out.contains(BY_OWN) && out.contains("1 sent"), "{out}");
    println!("T1b F35 fixed: an executor whose price of 10 is its own market's does not dispute a true close on another market (the committee signed 50.5 there)");
}

/// F36 (Medium in round 24), fixed in round 25 as it was built. The executor judged a claim by the price of now. It
/// now judges by its readings around the claim's time. The attack as it was built (the committee signed 10.5 for the
/// claim's time; 20 DAA later the executor's price is 10.2): with a reading of 10.5 before the claim's time and one
/// of 10.2 after it, the true close is not disputed; with the reading of 10.2 alone it is not judged.
#[test]
fn t1b_fixed_f36_a_claim_is_judged_by_the_readings_around_its_time() {
    let mut g = gsetup("moved", "1");
    let (me, position, table, market) = (g.me.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let prices = g.dir("prices");
    let signed = g.all(&prices, 33_400, "10.5");
    let s_close = g.script("KpfTClose");
    let mut args = vec!["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let claim = g.h.obj(&d, "claim", 33_410);
    let f = g.h.funding(50);
    let folder = g.dir("folder");
    put_in(&folder, &claim);
    let out = own_rounds(&g, &folder, &keeper, &f, &market, "10.5", "10.2", &[]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "F36 is open again: {out}");
    let single = g.dir("single");
    put_in(&single, &claim);
    let out = dispute_round(&g, &single, &keeper, &f, &["--market1", &market, "--price", "10.2"]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "the price of now alone judges nothing: {out}");
    println!("T1b F36 fixed: the committee signed 10.5 for the claim's time; an executor that read 10.5 before it and 10.2 after it does not dispute the true close, and one that has the 10.2 alone does not judge it");
}

/// F39 (Medium in round 25), as it stands after round 26: by design, the limit of a price source that says one
/// number. A claim is judged by the executor's readings each side of its time and by the margin around them. The
/// claimer picks the time: where the price stood further than the margin (0.8 % by default) from both readings in
/// between, a true close of that time is disputed and proved. Shown: the executor reads 10.2 ten DAA before the
/// claim's time and 10.2 twenty DAA after it; the committee signed 10.5 for the time between. A source that says the
/// least and the most since it was last asked does not have this
/// (`t1b_fixed_f39_a_source_that_says_low_and_high_covers_a_move_between_two_readings`).
#[test]
fn t1b_design_f39_a_source_of_one_number_does_not_see_a_move_between_two_readings() {
    let mut g = gsetup("spike", "1");
    let (me, position, table, market) = (g.me.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let prices = g.dir("prices");
    let signed = g.all(&prices, 33_400, "10.5");
    let s_close = g.script("KpfTClose");
    let mut args = vec!["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let claim = g.h.obj(&d, "claim", 33_410);
    let folder = g.dir("folder");
    put_in(&folder, &claim);
    let f = g.h.funding(50);
    let out = own_rounds(&g, &folder, &keeper, &f, &market, "10.2", "10.2", &[]);
    assert!(out.contains(BY_OWN) && out.contains("1 sent"), "a source of one number: the move between two readings is not seen: {out}");
    let dis = g.h.obj(&format!("{folder}/dry-run"), "disputed claim", 33_425);
    let s_prove = g.script("KpfTProve");
    let mut args = vec!["pos-prove", "--claim", &dis, "--script", &s_prove, "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    assert_eq!(paid(&d, &keeper), 0, "the executor's bond is gone");
    // A margin that covers the move leaves it.
    let wide = g.dir("wide");
    put_in(&wide, &claim);
    let out = own_rounds(&g, &wide, &keeper, &f, &market, "10.2", "10.2", &["--dispute-margin-bps", "400"]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "{out}");
    println!("T1b DESIGN F39 (a source of one number): readings of 10.2 ten DAA before and twenty after the claim's time, the committee's 10.5 in between: the true close is disputed and proved; a margin of 4 % leaves it");
}

/// F37 (Low in round 24), fixed in round 25. A wide margin stepped over the band a liquidation is within. The range
/// is now tried in steps of a quarter of the tolerance whatever the margin. The attack as it was built (the committee
/// signed 6; the executor's price is 6.075 and its margin 2,000 bps): the rule's liquidation is not disputed. THE
/// POSITIVE CONTROL for a liquidation: with readings of 10, where the side is far above its maintenance, it is
/// disputed.
#[test]
fn t1b_fixed_f37_a_wide_margin_does_not_step_over_the_band_a_liquidation_is_within() {
    let mut g = gsetup("wide", "1");
    let (position, table, market) = (g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let (keeper, liquidator) = (g.h.key("keeper"), g.h.key("liquidator"));
    let prices = g.dir("prices");
    let signed = g.all(&prices, 33_400, "6");
    let s_liq = g.script("KpfTLiq");
    let mut args = vec!["pos-liquidate", "--position", &position, "--script", &s_liq, "--side", "a", "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&liquidator, &args);
    let lq = g.h.obj(&d, "liquidation", 33_410);
    let f = g.h.funding(50);
    let mut n = 0;
    let mut rounds = |g: &GW, p: &str, extra: &[&str]| -> String {
        n += 1;
        let folder = g.dir(&format!("folder{n}"));
        put_in(&folder, &lq);
        own_rounds(g, &folder, &keeper, &f, &market, p, p, extra)
    };
    let out = rounds(&g, "6.02", &[]);
    assert!(out.contains("0 sent") && !out.contains("disputed"), "the default margin: {out}");
    for margin in ["2000", "500"] {
        let out = rounds(&g, "6.075", &["--dispute-margin-bps", margin]);
        if margin == "2000" || margin == "500" {
            assert!(out.contains("0 sent") && !out.contains("disputed"), "F37 is open again: a margin of {margin} bps steps over the band: {out}");
        }
    }
    let out = rounds(&g, "10", &[]);
    assert!(out.contains("disputed (not within the rule at any price near this executor's own)") && out.contains("1 sent"), "a liquidation of a side that is nowhere near its maintenance at the executor's readings is disputed: {out}");
    println!("T1b F37 fixed: with a margin of 2,000 or 500 bps an executor whose readings are 6.075 does not dispute the rule's liquidation at the committee's 6; with readings of 10 it disputes it");
}

/// A round of `exec-run --do dispute` at `daa` with the executor's own price of `market` from `source` (a flag and
/// its value: `--price`, `--price-file` or `--price-cmd`).
fn sourced_round(g: &GW, folder: &str, who: &str, funding: &str, market: &str, daa: &str, source: (&str, &str)) -> String {
    g.h.ok(&["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", who, "--funding", funding, "--daa", daa, "--do", "dispute", "--expiry-guard", "5", "--market1", market, source.0, source.1])
}

/// F39 (Medium in round 25), with the first of its directions in round 26. A price source may say three numbers:
/// the price, and the least and the most since it was last asked. The move of F39 as it was built (the executor reads
/// 10.2 before the claim's time and 10.2 after it; the committee signed 10.5 for the time between): with the later
/// reading saying `10.2 10.2 10.5`, from a file or from a command, the true close is left. A close of 50 KAS is still
/// disputed by the same readings. What the new reading of a source opens: a file of two numbers or of four, three
/// with the price outside its low and high, a low of 0, and a command that prints a price and two numbers that are
/// not its low and high are each said and give no reading (nothing is judged that round); a command that prints a
/// price and two numbers that happen to lie each side of it is read as a low and a high, which widens what the
/// executor allows and never narrows it.
#[test]
fn t1b_fixed_f39_a_source_that_says_low_and_high_covers_a_move_between_two_readings() {
    let mut g = gsetup("lowhigh", "1");
    let (me, position, table, market) = (g.me.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let prices = g.dir("prices");
    let signed = g.all(&prices, 33_400, "10.5");
    let s_close = g.script("KpfTClose");
    let mut args = vec!["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let truth = g.h.obj(&d, "claim", 33_410);
    let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--t0", "33400", "--p1", "11", "--amount", "50"]);
    let false_claim = g.h.obj(&d, "claim", 33_410);
    let f = g.h.funding(50);
    let pf = g.h.p("price.txt");
    let mut n = 0;
    // Two rounds over a folder of its own holding `claim`: 10.2 from the file at 33,390, then `later` at 33,420.
    let mut rounds = |g: &GW, claim: &str, later: (&str, &str)| -> String {
        n += 1;
        let folder = g.dir(&format!("folder{n}"));
        put_in(&folder, claim);
        std::fs::write(&pf, "10.2\n").unwrap();
        assert!(sourced_round(g, &folder, &keeper, &f, &market, "33390", ("--price-file", &pf)).contains("0 sent"));
        if later.0 == "file" {
            std::fs::write(&pf, later.1).unwrap();
            sourced_round(g, &folder, &keeper, &f, &market, "33420", ("--price-file", &pf))
        } else {
            sourced_round(g, &folder, &keeper, &f, &market, "33420", ("--price-cmd", later.1))
        }
    };
    let left = |out: &str| out.contains("0 sent") && !out.contains("disputed");
    // The low and the high said: the true close is left, the false one disputed.
    for later in [("file", "10.2 10.2 10.5\n"), ("cmd", "echo 10.2 10.2 10.5")] {
        let out = rounds(&g, &truth, later);
        assert!(left(&out), "F39 with a source that says low and high ({later:?}): {out}");
        assert!(rounds(&g, &false_claim, later).contains(BY_OWN), "a close of 50 KAS with {later:?}");
    }
    // One number, as before: the move is not seen (the limit of such a source).
    for later in [("file", "10.2\n"), ("cmd", "echo 10.2 KAS now")] {
        assert!(rounds(&g, &truth, later).contains(BY_OWN), "{later:?}");
    }
    // What is no reading: said, and nothing judged that round.
    for later in [("file", "10.2 10.5\n"), ("file", "10.2 10.2 10.5 10.6\n"), ("file", "10.2 10.3 10.5\n"), ("file", "10.6 10.2 10.5\n"), ("file", "10.2 0 10.5\n"), ("file", "10.2 low high\n"), ("cmd", "echo 10.2 2026 11")] {
        let out = rounds(&g, &false_claim, later);
        assert!(left(&out) && out.contains("no price of this executor's own this round"), "{later:?} is taken as a reading: {out}");
    }
    // A command's price and two numbers that are not its low and high but lie each side of it: read as them.
    let out = rounds(&g, &false_claim, ("cmd", "echo 10.2 9 12"));
    assert!(left(&out), "a low of 9 and a high of 12 allow more, not less: {out}");
    println!("T1b F39 (round 26): with the later reading `10.2 10.2 10.5` (a file, a command) the true close at 10.5 is left and a close of 50 KAS disputed; with one number the move is not seen; 7 sources that are no reading are said and judge nothing; a command's `10.2 9 12` is read as a low and a high and allows more");
}

/// F40 (Low in round 25), fixed in round 26. A reading in `own-prices` now names its market, and a line of another
/// market, or not of five fields, is not read. The attack as it was built (two rounds with the first market's price
/// of 10 around the claim's time, then the executor started with the second market in the same folder): the second
/// market's true close is not judged. Lines of the old form written by hand are not read. What stays, by design: the
/// file is the executor's own, and lines written into it by hand in its own form, with its market's tag, are
/// believed.
#[test]
fn t1b_fixed_f40_a_reading_is_of_the_market_it_was_read_for() {
    let mut g = gsetup("carried", "1");
    let (me, exec, table, market) = (g.me.clone(), g.exec.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let feed2 = g.h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = g.h.p("market2.json");
    g.h.ok(&["c-market", "--committee", &g.h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    let open_s = g.script("KpfTOpen");
    let d = g.h.build(&me, &["pos-open", "--lib", &g.genesis.clone(), "--anchor", &g.anchor.clone(), "--script", &open_s, "--registry", &g.registry.clone(), "--key", &exec, "--market1", &market2, "--size1", "10", "--dir1", "long", "--x1", "50", "--a", &me, "--b", &exec, "--a-margin", "400", "--b-margin", "600", "--dispute-daa", "50", "--daa", "33100"]);
    let position2 = g.h.obj(&d, "position", 33_100);
    let prices = g.dir("prices2");
    let signed: Vec<String> = (0..3)
        .map(|i| {
            let f = format!("{prices}/price-33400-member{i}.json");
            g.h.ok(&["price-sign", "--market", &market2, "--t0", "33400", "--price", "50.5", "--key", &g.members[i], "--out", &f]);
            f
        })
        .collect();
    let s_close = g.script("KpfTClose");
    let mut args = vec!["pos-close", "--position", &position2, "--script", &s_close, "--side", "a", "--epoch-table1", &table];
    for t in &signed {
        args.extend(["--signed-price1", t.as_str()]);
    }
    let d = g.h.build(&me, &args);
    let claim = g.h.obj(&d, "claim", 33_405);
    let f = g.h.funding(5_000);
    let folder = g.dir("folder");
    put_in(&folder, &claim);
    let round = |g: &GW, folder: &str, daa: &str, market: &str, p: &str| sourced_round(g, folder, &keeper, &f, market, daa, ("--price", p));
    assert!(round(&g, &folder, "33390", &market, "10").contains("0 sent"));
    assert!(round(&g, &folder, "33410", &market, "10").contains("0 sent"));
    let out = round(&g, &folder, "33420", &market2, "50.5");
    assert!(out.contains("0 sent") && !out.contains("disputed"), "F40 is open again: the second market's claim is judged by the first market's readings: {out}");
    // The file: three lines of five fields, two tags.
    let lines: Vec<Vec<String>> = std::fs::read_to_string(format!("{folder}/own-prices")).unwrap().lines().map(|l| l.split(' ').map(str::to_string).collect()).collect();
    assert!(lines.len() == 3 && lines.iter().all(|l| l.len() == 5 && l[0].len() == 16), "{lines:?}");
    assert!(lines[0][0] == lines[1][0] && lines[0][0] != lines[2][0], "a reading names its market: {lines:?}");
    assert_eq!(lines[2][1..], ["33420", "5050000000", "5050000000", "5050000000"].map(str::to_string));
    let tag2 = lines[2][0].clone();
    // Lines of the old form, and lines with the first market's tag, written by hand: not read.
    for (what, seed) in [("the old form", "33390 1000000000\n33410 1000000000\n".to_string()), ("the other market's tag", format!("{0} 33390 1000000000 1000000000 1000000000\n{0} 33410 1000000000 1000000000 1000000000\n", lines[0][0]))] {
        let seeded = g.dir(&format!("seeded-{}", what.len()));
        put_in(&seeded, &claim);
        std::fs::write(format!("{seeded}/own-prices"), seed).unwrap();
        let out = round(&g, &seeded, "33420", &market2, "50.5");
        assert!(out.contains("0 sent") && !out.contains("disputed"), "lines of {what} are read: {out}");
    }
    // Lines in the file's own form with this market's tag are the executor's readings, whoever wrote them.
    let own = g.dir("seeded-own");
    put_in(&own, &claim);
    std::fs::write(format!("{own}/own-prices"), format!("{tag2} 33390 1000000000 1000000000 1000000000\n{tag2} 33410 1000000000 1000000000 1000000000\n")).unwrap();
    assert!(round(&g, &own, "33420", &market2, "50.5").contains(BY_OWN), "the file is the executor's own");
    println!("T1b F40 fixed: readings of one market do not judge another market's claim after the executor is started with it in the same folder; lines of the old form or of another market's tag are not read; lines in the file's own form with the market's tag are believed, by design");
}

// ---------------------------------------------------------------- round 27: exec-run's take and answer

/// An order of `who` in the registry of `g`, for the market of `g`: `side`, `size` units at `limit`, a tip of 1 KAS.
fn bait(g: &mut GW, who: &str, side: &str, size: &str, limit: &str, margin: &str, extra: &[&str]) -> String {
    let (genesis, anchor, registry, market) = (g.genesis.clone(), g.anchor.clone(), g.registry.clone(), g.market.clone());
    let place_s = g.script("KpfTPlace");
    let mut a = vec!["order-place", "--lib", &genesis, "--anchor", &anchor, "--script", &place_s, "--registry", &registry, "--market1", &market, "--side", side, "--size", size, "--limit", limit, "--margin", margin, "--tip", "1", "--expiry", "40000"];
    if !extra.contains(&"--dispute-daa") {
        a.extend(["--dispute-daa", "50"]);
    }
    a.extend(extra);
    let d = g.h.build(who, &a);
    g.h.obj(&d, "order", 33_900)
}
/// A round of `exec-run --do take` for `who` at DAA 34,000 with its own price `price` of the market of `g`.
fn take_round(g: &GW, folder: &str, who: &str, funding: &str, price: (&str, &str), max: &str, extra: &[&str]) -> String {
    let mut a = vec!["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", who, "--funding", funding, "--daa", "34000", "--do", "take", "--market1", &g.market, "--dispute-daa", "50", "--take-max", max, price.0, price.1];
    a.extend(extra);
    g.h.ok(&a)
}
/// What a round of `take` says when it has decided to answer an order and goes to place its own: without a node it
/// finds no anchor to place it through, and says so. (An offline round places nothing: this line is all it shows of
/// the decision.)
const TAKEN: &str = "take: no anchor to place an order through";
const LEFT_BY_CAP: &str = "is left: this executor's own margin stands at";

/// Round 27, `exec-run --do take` offline: the control, and what holds. A buy order of somebody else that rests 2 %
/// above the executor's own price is answered (the round goes to place its own order the other way); one inside the
/// spread, one of its own key, one without the executor's price, and one when its margin is at the cap are not.
/// Without `--take-max` the action is refused.
#[test]
fn t1b_r27_take_answers_an_order_the_spread_better_than_its_own_price_and_no_other() {
    let mut g = gsetup("take", "1");
    let (third, keeper) = (g.h.key("third"), g.h.key("keeper"));
    let f = g.h.funding(1_000);
    let folder_of = |g: &GW, name: &str, files: &[&str]| -> String {
        let d = g.dir(name);
        for x in files {
            put_in(&d, x);
        }
        d
    };
    let good = bait(&mut g, &third, "long", "3", "10.2", "12", &[]);
    let near = bait(&mut g, &third, "long", "3", "10.05", "12", &[]);
    let sell = bait(&mut g, &third, "short", "3", "9.8", "12", &[]);
    let own = bait(&mut g, &keeper, "long", "3", "10.2", "12", &[]);
    // The control: a buy at 10.2 against the executor's 10, a sell at 9.8.
    for (name, order) in [("buy", &good), ("sell", &sell)] {
        let out = take_round(&g, &folder_of(&g, name, &[order]), &keeper, &f, ("--price", "10"), "500", &[]);
        assert!(out.contains(TAKEN), "an order 2 % better than the executor's price is answered: {out}");
    }
    // Not answered.
    let quiet = |out: &str| !out.contains(TAKEN) && out.contains("0 sent");
    assert!(quiet(&take_round(&g, &folder_of(&g, "near", &[&near]), &keeper, &f, ("--price", "10"), "500", &[])), "half a percent is inside the spread of 1 %");
    assert!(take_round(&g, &folder_of(&g, "near-tight", &[&near]), &keeper, &f, ("--price", "10"), "500", &["--take-spread-bps", "20"]).contains(TAKEN), "and outside a spread of 0.2 %");
    assert!(quiet(&take_round(&g, &folder_of(&g, "own", &[&own]), &keeper, &f, ("--price", "10"), "500", &[])), "its own order");
    assert!(quiet(&take_round(&g, &folder_of(&g, "dear", &[&good]), &keeper, &f, ("--price", "10.15"), "500", &[])), "a buy at 10.2 is half a percent above 10.15");
    let pf = g.h.p("price.txt");
    std::fs::write(&pf, "").unwrap();
    let out = take_round(&g, &folder_of(&g, "silent", &[&good]), &keeper, &f, ("--price-file", &pf), "500", &[]);
    assert!(quiet(&out) && out.contains("take: no price of this executor's own this round"), "{out}");
    // The cap: the executor's margin of 60 KAS in a position is all of a cap of 60 KAS.
    let exec = g.exec.clone();
    let out = take_round(&g, &folder_of(&g, "capped", &[&good, &g.position.clone()]), &exec, &f, ("--price", "10"), "60", &[]);
    assert!(quiet(&out) && out.contains(LEFT_BY_CAP), "{out}");
    // Asked for without a cap: refused.
    assert!(g.h.fail(&["exec-run", "--dir", &folder_of(&g, "nocap", &[&good]), "--lib", &g.genesis, "--registry", &g.registry, "--funder", &keeper, "--funding", &f, "--daa", "34000", "--do", "take", "--market1", &g.market, "--price", "10"]).contains("--take-max"));
    println!("T1b R27 take: a buy 2 % above and a sell 2 % below the executor's price are answered; an order inside the spread, its own, one seen without a price, and one at the cap are left; without --take-max the action is refused");
}

/// F42 (Medium in round 27), fixed in round 28. The cap of `take` did not count the executor's margin in a position
/// under a claim. It counts every object the executor is a party of. The attack as it was built: 60 KAS of its own in
/// a position under a declared close and `--take-max 60`: the order is left, as with the position itself; and so
/// under an exit claim of the other party.
#[test]
fn t1b_fixed_f42_the_cap_of_take_counts_margin_in_a_claimed_position() {
    let mut g = gsetup("takecap", "1");
    let (me, exec, position) = (g.me.clone(), g.exec.clone(), g.position.clone());
    let third = g.h.key("third");
    let good = bait(&mut g, &third, "long", "3", "10.2", "12", &[]);
    let (s_close, s_exit) = (g.script("KpfTClose"), g.script("KpfTExit"));
    let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--t0", "33950", "--p1", "10"]);
    let claim = g.h.obj(&d, "claim", 33_960);
    let d = g.h.build(&me, &["pos-exit", "--position", &position, "--script", &s_exit, "--side", "a", "--since", "33900"]);
    let exit = g.h.obj(&d, "exit claim", 33_960);
    let f = g.h.funding(1_000);
    for (n, (what, file)) in [("the position", &position), ("the position under a declared close", &claim), ("the position under an exit claim", &exit)].into_iter().enumerate() {
        let folder = g.dir(&format!("held{n}"));
        put_in(&folder, &good);
        put_in(&folder, file);
        let out = take_round(&g, &folder, &exec, &f, ("--price", "10"), "60", &[]);
        assert!(out.contains(LEFT_BY_CAP) && !out.contains(TAKEN), "F42 is open again: {what}: {out}");
        // With room for the order beside it, it is answered.
        assert!(take_round(&g, &folder, &exec, &f, ("--price", "10"), "80", &[]).contains(TAKEN), "{what}, a cap of 80 KAS");
    }
    println!("T1b F42 fixed: 60 KAS of the executor's own in a position, or in that position under a declared close or an exit claim, fill a cap of 60 KAS; with a cap of 80 KAS the order is answered");
}

/// F43 (Medium in round 27) and F44 (Medium in round 27), fixed in round 28; and what was read in round 27 of an
/// order that asks more of its counterparty than it posts. `take` counted as a way out any position of the market;
/// it counts those of the order's own template. It read the price of the round; it reads the low and the high its
/// source says. The attacks as they were built: long 10 units with a cap of 1 KAS, a buy order at the executor's own
/// price of another dispute period than the position's is left (one of the position's own is answered; and one of
/// the other template 2 % above the price is answered as any order is, within the cap, and left by a cap of 1 KAS);
/// with a source that says `10 10 10.5` a buy order at 10.2 is left (one at 10.7 is answered). A buy order of 12 KAS
/// of margin that asks 200 KAS of its counterparty is left.
#[test]
fn t1b_fixed_f43_f44_take_leaves_what_is_no_way_out_and_what_its_source_saw_above() {
    let mut g = gsetup("takeout", "1");
    let (me, position) = (g.me.clone(), g.position.clone());
    let (third, keeper) = (g.h.key("third"), g.h.key("keeper"));
    let same = bait(&mut g, &third, "long", "3", "10", "12", &[]);
    let other = bait(&mut g, &third, "long", "3", "10", "12", &["--dispute-daa", "60"]);
    let other_good = bait(&mut g, &third, "long", "3", "10.2", "12", &["--dispute-daa", "60"]);
    let good = bait(&mut g, &third, "long", "3", "10.2", "12", &[]);
    let higher = bait(&mut g, &third, "long", "3", "10.7", "12", &[]);
    let asking = bait(&mut g, &third, "long", "3", "10.2", "12", &["--min-counter-margin", "200"]);
    let f = g.h.funding(1_000);
    let mut n = 0;
    let mut folder = |g: &GW, files: &[&str]| -> String {
        n += 1;
        let d = g.dir(&format!("f{n}"));
        for x in files {
            put_in(&d, x);
        }
        d
    };
    // F43. (A round reads the template an order's file carries; it is run with the order's dispute period too.)
    assert!(take_round(&g, &folder(&g, &[&same, &position]), &me, &f, ("--price", "10"), "1", &[]).contains(TAKEN), "an order of its position's template lets it out");
    let d60 = ["--dispute-daa", "60"];
    let out = take_round(&g, &folder(&g, &[&other, &position]), &me, &f, ("--price", "10"), "1", &d60);
    assert!(!out.contains(TAKEN), "F43 is open again: an order of another template is answered as a way out: {out}");
    let out = take_round(&g, &folder(&g, &[&other_good, &position]), &me, &f, ("--price", "10"), "1", &d60);
    assert!(!out.contains(TAKEN) && out.contains(LEFT_BY_CAP) && out.contains("stands at 40 KAS"), "an order of another template is under the cap, the position's margin counted: {out}");
    assert!(!take_round(&g, &folder(&g, &[&other, &position]), &me, &f, ("--price", "10"), "1", &[]).contains(TAKEN), "F43 is open again (the round as it was run in round 27)");
    assert!(take_round(&g, &folder(&g, &[&other_good, &position]), &me, &f, ("--price", "10"), "500", &d60).contains(TAKEN), "and answered by the spread within it (so the template is one the round reads)");
    // F44.
    let pf = g.h.p("price.txt");
    std::fs::write(&pf, "10 10 10.5\n").unwrap();
    let out = take_round(&g, &folder(&g, &[&good]), &keeper, &f, ("--price-file", &pf), "500", &[]);
    assert!(!out.contains(TAKEN), "F44 is open again: a buy at 10.2 under a high of 10.5 is answered: {out}");
    assert!(take_round(&g, &folder(&g, &[&higher]), &keeper, &f, ("--price-file", &pf), "500", &[]).contains(TAKEN), "a buy at 10.7 is 1 % above the high");
    std::fs::write(&pf, "10\n").unwrap();
    assert!(take_round(&g, &folder(&g, &[&good]), &keeper, &f, ("--price-file", &pf), "500", &[]).contains(TAKEN), "and a buy at 10.2 against a source that says 10");
    // The way out is asked against the high too: long 10 units, a buy at 10.2 under a high of 10.5 is no way out.
    std::fs::write(&pf, "10 10 10.5\n").unwrap();
    assert!(!take_round(&g, &folder(&g, &[&good, &position]), &me, &f, ("--price-file", &pf), "1", &[]).contains(TAKEN), "a way out below the high its source saw");
    // An order that asks more than it posts.
    let out = take_round(&g, &folder(&g, &[&asking]), &keeper, &f, ("--price", "10"), "500", &[]);
    assert!(!out.contains(TAKEN), "an order of 12 KAS that asks 200 KAS of its counterparty is answered: {out}");
    println!("T1b F43 and F44 fixed: an order of another template is no way out (left at the price, under the cap, answered by the spread within it); a buy inside what the source saw is left and one above it answered; an order that asks more of its counterparty than it posts is left");
}

/// Rounds 27 and 28, `exec-run --do answer` offline: the control, and F45 (Medium in round 27), fixed in round 28. A
/// member signs for a disputed claim's time only with a reading of its own price each side of the time, within a
/// round, and only if those readings (their lows and highs counted) are no further apart than the committee's spread;
/// once. The control: readings of 10.5 ten DAA before and twenty after: it signs 10.5 for the time, one file, and
/// nothing more when run again. F45 as it was built (10.2 before, `10.2 10.2 10.5` after): it signs nothing and says
/// why. It signs nothing with one reading, for a claim nobody disputed, with readings 300 DAA away, without an epoch
/// table of the committee, or without its key.
#[test]
fn t1b_fixed_f45_a_member_signs_where_its_readings_around_the_time_agree() {
    let mut g = gsetup("answer", "1");
    let (me, exec, position, table, market) = (g.me.clone(), g.exec.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let member = g.members[0].clone();
    let (s_close, s_disp) = (g.script("KpfTClose"), g.script("KpfTDispute"));
    let d = g.h.build(&me, &["pos-close", "--position", &position, "--script", &s_close, "--side", "a", "--t0", "33400", "--p1", "10.5", "--amount", "44"]);
    let claim = g.h.obj(&d, "claim", 33_410);
    let d = g.h.build(&exec, &["pos-dispute", "--claim", &claim, "--script", &s_disp]);
    let dis = g.h.obj(&d, "disputed claim", 33_415);
    let f = g.h.funding(50);
    let pf = g.h.p("price.txt");
    let round = |g: &GW, folder: &str, prices: &str, daa: &str, says: &str, key: bool| -> String {
        std::fs::write(&pf, says).unwrap();
        let mut a = vec!["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &member, "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price-file", &pf, "--signed-prices", prices];
        a.extend(["--epoch-table", table.as_str()]);
        if key {
            a.extend(["--member-key", member.as_str()]);
        }
        g.h.ok(&a)
    };
    let signed_in = |dir: &str| -> Vec<Value> { std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("price-")).map(|p| read(&p.display().to_string())).collect() };
    let pair = |g: &GW, name: &str, file: &str| -> (String, String) {
        let (folder, prices) = (g.dir(&format!("{name}-folder")), g.dir(&format!("{name}-prices")));
        put_in(&folder, file);
        (folder, prices)
    };
    // The control.
    let (folder, prices) = pair(&g, "calm", &dis);
    let out = round(&g, &folder, &prices, "33390", "10.5", true);
    assert!(!out.contains("answer: signed") && out.contains("on both sides"), "one reading signs nothing yet: {out}");
    let out = round(&g, &folder, &prices, "33420", "10.5", true);
    assert!(out.contains("answer: signed 10.5 KAS for DAA 33400") && out.contains("its reading of DAA 33390"), "the control: {out}");
    let signed = signed_in(&prices);
    assert_eq!((signed.len(), signed[0]["t0"].as_i64(), signed[0]["price"].as_i64()), (1, Some(33_400), Some(1_050_000_000)));
    assert!(!round(&g, &folder, &prices, "33440", "10.5", true).contains("answer: signed") && signed_in(&prices).len() == 1, "once");
    // F45 as built.
    let (folder, prices) = pair(&g, "moved", &dis);
    round(&g, &folder, &prices, "33390", "10.2", true);
    let out = round(&g, &folder, &prices, "33420", "10.2 10.2 10.5", true);
    assert!(!out.contains("answer: signed") && out.contains("further apart than the committee's spread") && signed_in(&prices).is_empty(), "F45 is open again: {out}");
    // Two prices further apart than the spread (0.1 %), with no low or high said.
    let (folder, prices) = pair(&g, "stepped", &dis);
    round(&g, &folder, &prices, "33390", "10.5", true);
    assert!(!round(&g, &folder, &prices, "33420", "10.52", true).contains("answer: signed") && signed_in(&prices).is_empty(), "10.5 before and 10.52 after");
    // Within the spread: signed, the nearer reading's price.
    let (folder, prices) = pair(&g, "near", &dis);
    round(&g, &folder, &prices, "33390", "10.5", true);
    assert!(round(&g, &folder, &prices, "33420", "10.505 10.5 10.505", true).contains("answer: signed 10.5 KAS for DAA 33400"));
    // Not asked, not watched, no table, no key.
    let (folder, prices) = pair(&g, "undisputed", &claim);
    round(&g, &folder, &prices, "33390", "10.5", true);
    assert!(!round(&g, &folder, &prices, "33420", "10.5", true).contains("answer:") && signed_in(&prices).is_empty(), "a claim nobody disputed asks nothing");
    let (folder, prices) = pair(&g, "far", &dis);
    round(&g, &folder, &prices, "33700", "10.5", true);
    assert!(round(&g, &folder, &prices, "33900", "10.5", true).contains("no reading of its own price within a round of it on both sides") && signed_in(&prices).is_empty());
    let (folder, prices) = pair(&g, "notable", &dis);
    std::fs::write(&pf, "10.5").unwrap();
    let out = g.h.fail(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &member, "--funding", &f, "--daa", "33420", "--do", "answer", "--market1", &market, "--price-file", &pf, "--signed-prices", &prices, "--member-key", &member]);
    assert!(out.contains("--epoch-table") && signed_in(&prices).is_empty(), "without an epoch table the round is refused: {out}");
    let (folder, prices) = pair(&g, "nokey", &dis);
    round(&g, &folder, &prices, "33390", "10.5", false);
    assert!(round(&g, &folder, &prices, "33420", "10.5", false).contains("give --member-key") && signed_in(&prices).is_empty());
    println!("T1b F45 fixed: readings of 10.5 each side of a disputed claim's time: the member signs 10.5 for it, once; readings from 10.2 to 10.5, or 10.5 and 10.52: nothing; readings 0.05 % apart: signed; nothing for an undisputed claim, for readings 300 DAA away, or without its key; without an epoch table the round is refused");
}

/// F46 (High in round 27), fixed in round 28: the control and the fix. An exit claim of a member's market is answered:
/// each member signs for the first multiple of 300 DAA at or after the claim landed (33,600 for a claim that landed
/// at 33,400), with a reading each side of it. Three members on three machines with rounds at different times and
/// prices 0.05 % apart (11, 11.005, 11.01) sign for the one time. With members 1 and 2 (37 of 67 KAS, no quorum) the
/// claim is left; with member 0 too a round under `--do dispute` ends it, the sender has half of the claimer's bond
/// and the other party the other half. Under `--do watch` the other party's own round ends it, and a third key's
/// does not.
#[test]
fn t1b_fixed_f46_an_exit_claim_is_answered_and_ended() {
    let mut g = gsetup("answerexit", "1");
    let (me, exec, position, table, market) = (g.me.clone(), g.exec.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let s_exit = g.script("KpfTExit");
    let d = g.h.build(&me, &["pos-exit", "--position", &position, "--script", &s_exit, "--side", "a", "--since", "33380"]);
    let exit = g.h.obj(&d, "exit claim", 33_400);
    let f = g.h.funding(50);
    let prices = g.dir("prices");
    // Each member on its own machine: its own folder, the folder of signed prices shared.
    let member_round = |g: &GW, i: usize, daa: &str, price: &str| -> String {
        let folder = g.dir(&format!("member{i}"));
        put_in(&folder, &exit);
        g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.members[i], "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price", price, "--signed-prices", &prices, "--epoch-table", &table, "--member-key", &g.members[i]])
    };
    let files = |dir: &str| std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("price-33600-")).count();
    let end_round = |g: &GW, name: &str, who: &str, does: &str| -> (String, String) {
        let folder = g.dir(name);
        put_in(&folder, &exit);
        (g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", who, "--funding", &f, "--daa", "33750", "--do", does, "--signed-prices", &prices, "--epoch-table", &table]), format!("{folder}/dry-run"))
    };
    const ENDED: &str = "ended: the committee signed after DAA 33380";
    // Nothing signed yet: the claim is left.
    let (out, _) = end_round(&g, "end0", &keeper, "dispute");
    assert!(out.contains("0 sent") && !out.contains(ENDED), "{out}");
    // Members 1 and 2: no quorum.
    for (i, before, after, price) in [(1usize, "33500", "33700", "11.005"), (2, "33590", "33610", "11.01")] {
        assert!(!member_round(&g, i, before, price).contains("answer: signed"), "before the grid time nothing is signed");
        let out = member_round(&g, i, after, price);
        assert!(out.contains(&format!("answer: signed {price} KAS for DAA 33600")), "F46 is open again: member {i} does not answer the exit claim: {out}");
    }
    assert_eq!(files(&prices), 2);
    let (out, _) = end_round(&g, "end2", &keeper, "dispute");
    assert!(out.contains("0 sent") && !out.contains(ENDED), "two members that are no quorum end nothing: {out}");
    // Member 0: the quorum, and the end.
    member_round(&g, 0, "33450", "11");
    assert!(member_round(&g, 0, "33650", "11").contains("answer: signed 11 KAS for DAA 33600"));
    let (out, dry) = end_round(&g, "end3", &keeper, "dispute");
    assert!(out.contains(ENDED) && out.contains("1 sent"), "F46 is open again: the claim is not ended with a quorum's prices: {out}");
    let (other, sender) = (paid(&dry, &exec) as i64, paid(&dry, &keeper) as i64 - 50 * KAS as i64);
    assert!(other > 0 && (other - sender).abs() <= other / 20, "half of the claimer's bond to the other party ({other}), half to whoever ended it ({sender}, less the fee)");
    // The watchtower: the other party's own round ends it; a third key's does not.
    let (out, _) = end_round(&g, "end4", &exec, "watch");
    assert!(out.contains(ENDED) && out.contains("1 sent"), "the other party's watchtower: {out}");
    let (out, _) = end_round(&g, "end5", &keeper, "watch");
    assert!(out.contains("0 sent") && !out.contains(ENDED), "a watchtower of another key: {out}");
    println!("T1b F46 fixed: three members with rounds at different times sign 11, 11.005 and 11.01 for DAA 33,600, the grid time after an exit claim that landed at 33,400; with two of them (no quorum) the claim is left, with the three a round ends it: {other} sompi to the other party, {sender} to the sender after its fee");
}

/// By design, read and run (round 28): a member answers an exit claim of a position it is itself a party of as any
/// other: the round does not look at who the parties are. Member 0 (30 of 67 KAS, in every quorum) is long against
/// `exec` and makes the exit claim: run as a member it signs for the grid time, and with the other two the claim is
/// ended. A member that is the claimer is free not to run: with members 1 and 2 alone there is no quorum and the
/// claim is left. That is the committee's rule of 67 % (more than a third of the weight, silent, ends nothing), which
/// an exit claim is the answer to; here the silent one is the party that gains by the silence.
#[test]
fn t1b_design_a_member_that_is_a_party_answers_its_own_exit_claim_or_is_silent() {
    let mut g = gsetup("answerown", "1");
    let (exec, table, market, genesis, anchor, registry) = (g.exec.clone(), g.epoch_table.clone(), g.market.clone(), g.genesis.clone(), g.anchor.clone(), g.registry.clone());
    let m0 = g.members[0].clone();
    let keeper = g.h.key("keeper");
    let (s_open, s_exit) = (g.script("KpfTOpen"), g.script("KpfTExit"));
    let d = g.h.build(&m0, &["pos-open", "--lib", &genesis, "--anchor", &anchor, "--script", &s_open, "--registry", &registry, "--key", &exec, "--market1", &market, "--size1", "10", "--dir1", "long", "--x1", "10", "--a", &m0, "--b", &exec, "--a-margin", "40", "--b-margin", "60", "--dispute-daa", "50", "--daa", "33100"]);
    let position = g.h.obj(&d, "position", 33_100);
    let d = g.h.build(&m0, &["pos-exit", "--position", &position, "--script", &s_exit, "--side", "a", "--since", "33380"]);
    let exit = g.h.obj(&d, "exit claim", 33_400);
    let f = g.h.funding(50);
    let prices = g.dir("prices");
    let member = |g: &GW, i: usize| -> String {
        let folder = g.dir(&format!("member{i}"));
        put_in(&folder, &exit);
        let mut out = String::new();
        for daa in ["33500", "33700"] {
            out = g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.members[i], "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price", "11", "--signed-prices", &prices, "--epoch-table", &table, "--member-key", &g.members[i]]);
        }
        out
    };
    let end = |g: &GW, name: &str| -> String {
        let folder = g.dir(name);
        put_in(&folder, &exit);
        g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &keeper, "--funding", &f, "--daa", "33750", "--do", "dispute", "--signed-prices", &prices, "--epoch-table", &table])
    };
    for i in [1usize, 2] {
        assert!(member(&g, i).contains("answer: signed 11 KAS for DAA 33600"));
    }
    let out = end(&g, "silent");
    assert!(out.contains("0 sent") && !out.contains("ended"), "members 1 and 2 are no quorum: {out}");
    assert!(member(&g, 0).contains("answer: signed 11 KAS for DAA 33600"), "the member that made the claim signs as any other when it runs");
    assert!(end(&g, "spoken").contains("ended: the committee signed after DAA 33380"));
    println!("T1b design: member 0 (30 of 67 KAS) makes an exit claim of its own position: run as a member it signs for the grid time and the claim is ended; silent, the other two are no quorum and the claim is left");
}

/// F47 (Medium in round 28), fixed in round 29. An exit claim was asked one time; it is asked every multiple of
/// 300 DAA from the first up to now (the newest ten), every round while it stands. The attacks as they were built: a
/// member whose readings around 33,600 are 1 % apart signs nothing for it and signs 33,900 at its next calm round; a
/// member started at 33,700 signs 33,900. And three members that each miss a different time (member 0 the first,
/// member 1 the second, member 2 the third) leave the first time with members 1 and 2 (37 of 67 KAS, no quorum) and
/// the second with members 0 and 2 (45, a quorum), the third with members 0 and 1 (52): a round ends the claim.
#[test]
fn t1b_fixed_f47_an_exit_claim_is_asked_every_grid_time_while_it_stands() {
    let mut g = gsetup("answergrid", "1");
    let (me, position, table, market) = (g.me.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let keeper = g.h.key("keeper");
    let s_exit = g.script("KpfTExit");
    let d = g.h.build(&me, &["pos-exit", "--position", &position, "--script", &s_exit, "--side", "a", "--since", "33380"]);
    let exit = g.h.obj(&d, "exit claim", 33_400);
    let f = g.h.funding(50);
    let pf = g.h.p("price.txt");
    let round = |g: &GW, i: usize, folder: &str, prices: &str, daa: &str, says: &str| -> String {
        std::fs::write(&pf, says).unwrap();
        g.h.ok(&["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.members[i], "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price-file", &pf, "--signed-prices", prices, "--epoch-table", &table, "--member-key", &g.members[i]])
    };
    let times = |dir: &str| -> Vec<String> {
        let mut t: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("price-")).map(|n| n.split('-').nth(1).unwrap().to_string()).collect();
        t.sort();
        t
    };
    // The two cases of F47 as they were built.
    for (name, rounds, signs_at) in [("moved", vec![("33500", "11"), ("33700", "11 11 11.1"), ("33900", "11")], "33900"), ("late", vec![("33700", "11"), ("33900", "11")], "33900")] {
        let (folder, prices) = (g.dir(&format!("{name}-folder")), g.dir(&format!("{name}-prices")));
        put_in(&folder, &exit);
        for (daa, says) in rounds {
            let out = round(&g, 0, &folder, &prices, daa, says);
            assert_eq!(out.contains("answer: signed"), daa == signs_at, "{name} at {daa}: {out}");
            if name == "moved" && daa == "33700" {
                assert!(out.contains("further apart than the committee's spread"), "{out}");
            }
        }
        assert_eq!(times(&prices), ["33900"], "F47 is open again: {name}");
    }
    // Three members, each missing a different time: a wide reading after it.
    let prices = g.dir("prices");
    let end = |g: &GW, name: &str| -> String {
        let folder = g.dir(name);
        put_in(&folder, &exit);
        g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &keeper, "--funding", &f, "--daa", "34400", "--do", "dispute", "--signed-prices", &prices, "--epoch-table", &table])
    };
    for (i, wide_at) in [(0usize, "33700"), (1, "34000"), (2, "34300")] {
        let folder = g.dir(&format!("member{i}"));
        put_in(&folder, &exit);
        for daa in ["33500", "33700", "33800", "34000", "34100", "34300"] {
            round(&g, i, &folder, &prices, daa, if daa == wide_at { "11 11 11.1" } else { "11" });
        }
        if i == 1 {
            // Members 0 and 1 so far: 33,600 has member 1 alone, 33,900 member 0 alone, 34,200 both.
            assert_eq!(times(&prices), ["33600", "33900", "34200", "34200"]);
        }
    }
    // (Round 30: after members 0 and 1, 34,200 already holds a quorum (52 of 67 KAS). The F50 fix of round 29 stops a
    // member signing further times of a claim once a quorum of one of its times is in the folder it writes to, so
    // member 2 signs nothing more: the folder stays at the four signatures members 0 and 1 left.)
    assert_eq!(times(&prices), ["33600", "33900", "34200", "34200"], "member 2 adds nothing: 34,200 already has a quorum (F50)");
    let out = end(&g, "end");
    assert!(out.contains("ended: the committee signed after DAA 33380") && out.contains("1 sent"), "a time a quorum signed ends the claim: {out}");
    println!("T1b F47 fixed: a member that cannot sign 33,600 signs 33,900; one started at 33,700 signs 33,900; three members that each miss a different one of three times give 34,200 a quorum by members 0 and 1, after which member 2 signs nothing more (F50); the claim is ended by a time a quorum signed");
}

/// F48 (Low in round 28), fixed in round 29. An exit claim that landed before its own `since` is ended by a quorum's
/// price of a time at or after `since`, under `--do watch` (the other party's key) and `--do dispute`; with no such
/// price at hand those rounds send nothing, and `--do late` ends it as before.
#[test]
fn t1b_fixed_f48_an_exit_claim_that_landed_before_its_since_is_ended_by_a_price_after_it() {
    let mut g = gsetup("exitearly", "1");
    let (me, exec, position, table, market) = (g.me.clone(), g.exec.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let s_exit = g.script("KpfTExit");
    let d = g.h.build(&me, &["pos-exit", "--position", &position, "--script", &s_exit, "--side", "a", "--since", "34200"]);
    let exit = g.h.obj(&d, "exit claim", 33_400);
    let f = g.h.funding(50);
    let (prices, none) = (g.dir("prices"), g.dir("none"));
    let round = |g: &GW, name: &str, does: &str, prices: &str| -> String {
        let folder = g.dir(name);
        put_in(&folder, &exit);
        g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &exec, "--funding", &f, "--daa", "34400", "--do", does, "--signed-prices", prices, "--epoch-table", &table])
    };
    for i in 0..3 {
        let folder = g.dir(&format!("member{i}"));
        put_in(&folder, &exit);
        let mut out = String::new();
        for daa in ["33900", "34100", "34300"] {
            out = g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.members[i], "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price", "11", "--signed-prices", &prices, "--epoch-table", &table, "--member-key", &g.members[i]]);
            // No time before the claim's since (34,200) is ever asked. Members 0 and 1 sign 34,200; by the time
            // member 2 runs, 34,200 holds a quorum (52 of 67 KAS) and the F50 fix of round 29 stops it signing any
            // further time of the claim, so it signs nothing at all.
            assert_eq!(out.contains("answer: signed"), daa == "34300" && i < 2, "no time before since, and member 2 skipped once a quorum stands: {daa}, member {i}: {out}");
        }
        assert_eq!(out.contains("answer: signed 11 KAS for DAA 34200"), i < 2, "members 0 and 1 sign 34,200; member 2 is skipped by the F50 fix: {out}");
    }
    for does in ["watch", "dispute"] {
        let out = round(&g, &format!("n-{does}"), does, &none);
        assert!(out.contains("0 sent") && !out.contains("ended"), "with no price nothing is sent: --do {does}: {out}");
        let out = round(&g, &format!("p-{does}"), does, &prices);
        assert!(out.contains("ended: the committee signed after DAA 34200") && out.contains("1 sent"), "F48 is open again: --do {does}: {out}");
    }
    let out = round(&g, "late", "late", &none);
    assert!(out.contains("of a time to come ended") && out.contains("1 sent"), "{out}");
    println!("T1b F48 fixed: an exit claim that landed 800 DAA before its since is asked for 34,200 and no earlier time; with the three members' prices of it, --do watch and --do dispute end it; with none they send nothing and --do late ends it");
}

/// F49 (High in round 28), fixed in round 29. `take` answers only orders of the templates the executor was started
/// with. The attack as it was built: an executor started for a dispute period of 50 DAA and the default exit time and
/// bond leaves a buy order 2 % above its price of a template with a dispute period of 1 DAA, with an exit time of
/// 1 DAA, with a bond of 1 basis point, and answers the same order of its own template. Started for the dispute
/// period of 1 DAA (its operator's choice) it answers that order and leaves the one of 50. Started with no
/// `--dispute-daa` (the default, 600) it leaves the order of 50.
#[test]
fn t1b_fixed_f49_take_answers_only_orders_of_the_templates_it_was_started_with() {
    let mut g = gsetup("taketerms", "1");
    let (third, keeper) = (g.h.key("third"), g.h.key("keeper"));
    let f = g.h.funding(1_000);
    let mut n = 0;
    let mut one = |g: &GW, order: &str| -> String {
        n += 1;
        let d = g.dir(&format!("t{n}"));
        put_in(&d, order);
        d
    };
    // A round of take started with exactly these terms (no flag twice).
    let started = |g: &GW, folder: &str, terms: &[&str]| -> (bool, String, String) {
        let mut a = vec!["exec-run", "--dir", folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &keeper, "--funding", &f, "--daa", "34000", "--do", "take", "--take-max", "500", "--price", "10"];
        if !terms.contains(&"--market1") {
            a.extend(["--market1", g.market.as_str()]);
        }
        a.extend(terms);
        g.h.run(&a)
    };
    let d50 = ["--dispute-daa", "50"];
    let own = bait(&mut g, &third, "long", "3", "10.2", "12", &[]);
    assert!(started(&g, &one(&g, &own), &d50).1.contains(TAKEN), "the order of the template it was started with");
    let terms = [("a dispute period of 1 DAA", ["--dispute-daa", "1"]), ("an exit time of 1 DAA", ["--exit-daa", "1"]), ("a bond of 1 basis point", ["--bond-bps", "1"])];
    for (what, extra) in &terms {
        let order = bait(&mut g, &third, "long", "3", "10.2", "12", extra);
        let (ok, out, _) = started(&g, &one(&g, &order), &d50);
        assert!(ok && !out.contains(TAKEN) && out.contains("0 sent"), "F49 is open again: {what}: {out}");
        // Started for those terms, it answers it: the terms are its operator's.
        let both: Vec<&str> = if extra[0] == "--dispute-daa" { extra.to_vec() } else { d50.iter().chain(extra.iter()).copied().collect() };
        assert!(started(&g, &one(&g, &order), &both).1.contains(TAKEN), "started for {what}");
    }
    assert!(!started(&g, &one(&g, &own), &["--dispute-daa", "1"]).1.contains(TAKEN), "started for 1 DAA it leaves the order of 50");
    // No --dispute-daa: the default of 600.
    let (ok, out, _) = started(&g, &one(&g, &own), &[]);
    assert!(ok && !out.contains(TAKEN), "started with the default dispute period it leaves the order of 50: {out}");
    // A flag given twice, and two markets.
    let twice = started(&g, &one(&g, &own), &["--dispute-daa", "50", "--dispute-daa", "1"]);
    let feed2 = g.h.p("feed2.txt");
    std::fs::write(&feed2, FEED.replace("example underlying", "another underlying")).unwrap();
    let market2 = g.h.p("market2.json");
    g.h.ok(&["c-market", "--committee", &g.h.p("committee.json"), "--feed", &feed2, "--out", &market2]);
    let two = started(&g, &one(&g, &own), &["--dispute-daa", "50", "--market1", &g.market, "--market1", &market2]);
    assert!(twice.0 && !twice.1.contains(TAKEN), "a flag given twice: the later one counts, and the order of 50 is left");
    assert!(started(&g, &one(&g, &own), &["--dispute-daa", "1", "--dispute-daa", "50"]).1.contains(TAKEN), "and the other way about it is answered");
    assert!(two.0 && !two.1.contains(TAKEN) && two.1.contains("give exactly one --market1"), "with two markets the executor has no price of its own and take answers nothing: {}", two.1);
    println!("T1b F49 fixed: an executor started for a dispute period of 50 DAA answers the order of that template and leaves those of a dispute period of 1 DAA, an exit time of 1 DAA and a bond of 1 basis point; started for each of those it answers it; started for 1 DAA or for the default it leaves the order of 50; of a flag given twice the later counts; with two markets it has no price of its own and answers nothing");
}

/// F50 (Low in round 29), fixed since (the thin-family commit added the skip of `xcmds.rs`, "Once the folder holds a
/// quorum's prices of a time at or after `since`, the claim is ended by them and no further time is signed"). A
/// member signed EVERY grid time while an exit claim stood, also after a quorum of one time was at hand. Now, once
/// the folder it writes to holds a quorum's prices of one of the claim's times, the member signs no further time.
/// Shown: members 0 and 1 give 33,600 a quorum (52 of 67 KAS); member 2, seeing it, signs nothing; member 0 run on
/// to DAA 34,900 signs nothing more; and the claim is ended by the 33,600 quorum. The journal still holds: into a
/// folder with no quorum of the claim the member signs every grid time (the same price again where it signed one),
/// and from a second folder with another reading it is refused a time it signed.
///
/// (Round 30: the round-29 `t1b_finding_f50` asserted the hole open — member 0 signing four more times. That is the
/// behaviour the fix removed, so the test's own body said it would fail once fixed; the code is right and the test
/// was stale. Rewritten here to verify the fix.)
#[test]
fn t1b_fixed_f50_a_member_signs_no_grid_time_once_a_quorum_of_one_stands() {
    let mut g = gsetup("answermany", "1");
    let (me, exec, position, table, market) = (g.me.clone(), g.exec.clone(), g.position.clone(), g.epoch_table.clone(), g.market.clone());
    let s_exit = g.script("KpfTExit");
    let d = g.h.build(&me, &["pos-exit", "--position", &position, "--script", &s_exit, "--side", "a", "--since", "33380"]);
    let exit = g.h.obj(&d, "exit claim", 33_400);
    let f = g.h.funding(50);
    let prices = g.dir("prices");
    let round = |g: &GW, i: usize, daa: &str, into: &str| -> String {
        let folder = g.dir(&format!("member{i}"));
        put_in(&folder, &exit);
        g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.members[i], "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price", "11", "--signed-prices", into, "--epoch-table", &table, "--member-key", &g.members[i]])
    };
    let files = |dir: &str| std::fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("price-")).count();
    for i in 0..3 {
        round(&g, i, "33500", &prices);
        // Members 0 and 1 sign 33,600; member 2, by then seeing a quorum of 33,600, signs nothing (the fix).
        assert_eq!(round(&g, i, "33700", &prices).contains("answer: signed 11 KAS for DAA 33600"), i < 2, "member {i} at 33,700");
    }
    assert_eq!(files(&prices), 2, "a quorum of 33,600 from members 0 and 1; member 2 added nothing");
    let mut signed = 0;
    for daa in ["33800", "34000", "34100", "34300", "34400", "34600", "34700", "34900"] {
        signed += round(&g, 0, daa, &prices).matches("answer: signed").count();
    }
    assert_eq!((signed, files(&prices)), (0, 2), "F50 is open again: member 0 signs a further time of a claim whose 33,600 already has a quorum");
    // The 33,600 quorum ends the claim.
    let folder = g.dir("ender");
    put_in(&folder, &exit);
    let out = g.h.ok(&["exec-run", "--dir", &folder, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &exec, "--funding", &f, "--daa", "34900", "--do", "dispute", "--signed-prices", &prices, "--epoch-table", &table]);
    assert!(out.contains("ended: the committee signed after DAA 33380") && out.contains("1 sent"), "the 33,600 quorum ends the claim: {out}");
    // The same key run from a second folder with other readings (11.005) is refused the time it signed at 11.
    let (second, third) = (g.dir("second"), g.dir("third"));
    put_in(&second, &exit);
    let mut out = String::new();
    for daa in ["33500", "33700"] {
        out = g.h.ok(&["exec-run", "--dir", &second, "--lib", &g.genesis, "--registry", &g.registry, "--funder", &g.members[0], "--funding", &f, "--daa", daa, "--do", "answer", "--market1", &market, "--price", "11.005", "--signed-prices", &third, "--epoch-table", &table, "--member-key", &g.members[0]]);
    }
    assert!(out.contains("answer: not signed for DAA 33600") && files(&third) == 0, "the key's journal refuses another price for a time it signed: {out}");
    println!("T1b F50 fixed: members 0 and 1 give 33,600 a quorum; member 2 and then member 0 (run on to 34,900) sign no further time of the claim, and the 33,600 quorum ends it; from a second folder with another reading member 0 is refused 33,600 by its key's journal");
}

/// Round 53. The old form of a covered account, at the command line. A library the command creates has 57
/// scripts and none of the seven that went; `acct-leg` and `acct-out` are no commands. On a covered account
/// opened by the command: `--b-margin` at its opening and `acct-add --side b` are refused; `acct-take`,
/// `acct-leave` and `acct-liq` state nothing (by either side); and the clearing party ends a user's account of no
/// leg by its own key, the coins to the user.
#[test]
fn t1b_r53_the_old_form_of_a_covered_account_is_gone_from_the_command_line() {
    let mut s = setup("r53-old-form");
    let (me, exec, market, genesis, registry, anchor) = (s.me.clone(), s.exec.clone(), s.market.clone(), s.genesis.clone(), s.registry.clone(), s.anchor.clone());
    let mut names: Vec<String> = std::fs::read_dir(&s.lib_dir).unwrap().filter_map(|e| e.unwrap().file_name().to_string_lossy().split("-script-").nth(1).map(|n| n.trim_end_matches(".json").to_string())).collect();
    names.sort();
    names.dedup();
    let gone = ["KpfTLegAdd", "KpfTEnter", "KpfTStepEnter", "KpfTLegOut", "KpfTLegMove", "KpfTHand", "KpfTNoteApply"];
    // (Round 54: 61, with the four of the call; round 56: 63, with the two of section 15.)
    // (Round 59: 64, with the split of 16.2; round 60: 66, with the two of a mark.)
    // (Review T5, docs 17.8b: 71, with `KpfTShardRow`, the answer of a row of a draw's or a call's trace.)
    // (72 with `KpfTRest`: an operation a script, docs 17.8e.)
    // (76 with `KpfTDay`: docs 17.8g, A.)
    assert_eq!(names.len(), 76, "{names:?}");
    assert!(names.iter().any(|n| n == "KpfTShardRow"), "{names:?}");
    assert!(gone.iter().all(|g| !names.iter().any(|n| n == g)), "{names:?}");
    for cmd in ["acct-leg", "acct-out"] {
        let (ok, out, err) = s.h.run(&[cmd, "--account", "nothing.json"]);
        assert!(!ok && (err.contains("unknown") || err.contains(cmd)), "{cmd}: {out} {err}");
    }
    // A shard of 50 KAS and a covered account of 20 against it.
    let (shard_s, open_c, add_c, change, end_s) = (s.script("KpfTShardOpen"), s.script("KpfTOpenC"), s.script("KpfTAddC"), s.script("KpfTChange"), s.script("KpfTAcctEnd"));
    let d = s.h.build(&exec, &["shard-open", "--lib", &genesis, "--anchor", &anchor, "--script", &shard_s, "--registry", &registry, "--market1", &market, "--held", "50", "--dispute-daa", "50"]);
    let shard = s.h.obj(&d, "shard", 33_100);
    let seed = s.h.last.split("seed ").nth(1).and_then(|x| x.get(..64)).expect("the shard's seed").to_string();
    let open = ["acct-open", "--shard", &shard, "--anchor", &anchor, "--script", &open_c, "--registry", &registry, "--seed", &seed, "--market1", &market, "--key", &exec, "--a", &me, "--b", &exec, "--a-margin", "20", "--dispute-daa", "50"];
    let mut with_b = open.to_vec();
    with_b.extend(["--b-margin", "2"]);
    let err = s.h.refused(&me, &with_b);
    assert!(err.contains("b-margin"), "{err}");
    let d = s.h.build(&me, &open);
    let account = s.h.obj(&d, "covered account", 33_110);
    let err = s.h.refused(&exec, &["acct-add", "--account", &account, "--script", &add_c, "--side", "b", "--amount", "3"]);
    assert!(err.contains("nothing of a second party's"), "{err}");
    let mut refused = 2;
    for (cmd, who, side) in [("acct-take", &me, "a"), ("acct-take", &exec, "b"), ("acct-leave", &me, "a"), ("acct-leave", &exec, "b"), ("acct-liq", &exec, "a"), ("acct-liq", &me, "b")] {
        let mut args = vec![cmd, "--account", &account, "--script", &change, "--side", side, "--prices", "13", "--t0", "33150"];
        args.extend(if cmd == "acct-take" { ["--amount", "1"] } else { ["--leg", "1"] });
        let err = s.h.refused(who, &args);
        assert!(err.contains("kinds of its own"), "{cmd} --side {side}: {err}");
        refused += 1;
    }
    // The clearing party ends the user's account of no leg: the coins are the user's.
    let d = s.h.build(&exec, &["acct-end", "--account", &account, "--script", &end_s, "--side", "b"]);
    assert_eq!(paid(&d, &me), 20 * KAS, "the user's 20 KAS, whoever ended it");
    println!("T1b R53 the command line: a library of 63 scripts and none of the seven; acct-leg and acct-out are no commands; {refused} commands of the old form refused on a covered account; the clearing party ends a user's account of no leg and the user is paid its 20 KAS");
}
