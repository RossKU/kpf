//! Review 5 (docs/audit/REVIEW-5.md), tooling half: the `kpf` binary driven offline against the in-memory chain
//! (`kpf_protocol::sim::Chain`, every input run in the script engine), as `lifecycle.rs` does.
//!
//! - `holds_guarantee_refuses_a_look_alike_part_before_it_says_anything_about_disputes` (F2, fixed; was
//!   `exploit_guarantee_tells_the_guarantor_that_no_quote_disputes_a_look_alike_part`).
//! - `holds_watch_shows_the_fast_path_first_and_the_cure_before_the_grace_ends` (F3, fixed; was
//!   `exploit_watch_promises_the_window_and_the_cure_while_a_guarantee_can_settle_now`), with wording note 1.

use std::path::PathBuf;
use std::process::Command;

use kaspa_consensus_core::tx::TransactionOutpoint;
use kpf_protocol::sim::Chain;
use kpf_protocol::state::{decode_redeem, Decoded, KIND_LIQ, SHORT};
use kpf_protocol::tx::{tx_from_rpc_json, Obj};
use serde_json::{json, Value};

const KAS: u64 = 100_000_000;

struct H {
    dir: PathBuf,
    chain: Chain,
    steps: usize,
}

impl Drop for H {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl H {
    fn new(tag: &str) -> H {
        let dir = std::env::temp_dir().join(format!("kpf-audit5-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut chain = Chain::new();
        chain.advance(1_000);
        H { dir, chain, steps: 0 }
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

    fn key(&self, name: &str) -> (String, [u8; 32]) {
        let path = self.p(&format!("{name}.key.json"));
        self.ok(&["keygen", "--out", &path]);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        (path, kpf_protocol::error::hex32(v["publicKey"].as_str().unwrap()).unwrap())
    }

    fn funding(&self, name: &str, key: &[u8; 32]) -> String {
        let mut utxos = self.chain.key_utxos(key);
        utxos.sort_by_key(|u| std::cmp::Reverse(u.amount));
        utxos.truncate(1);
        let list: Vec<Value> = utxos
            .iter()
            .map(|u| json!({"txid": u.outpoint.transaction_id.to_string(), "index": u.outpoint.index, "amount": u.amount, "daa": u.daa}))
            .collect();
        let path = self.p(&format!("funding-{name}.json"));
        std::fs::write(&path, serde_json::to_string(&list).unwrap()).unwrap();
        path
    }

    /// Runs a builder command, applies its transaction to the chain, returns (stdout, the object files it created).
    fn step(&mut self, args: &[&str]) -> (String, Vec<String>) {
        self.steps += 1;
        let out = self.p(&format!("out{}", self.steps));
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--out-dir", &out]);
        let stdout = self.ok(&a);
        let txfile = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().path()).find(|p| p.to_string_lossy().ends_with("-tx.json")).expect("tx file");
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&txfile).unwrap()).unwrap();
        assert_eq!(doc["verification"]["ok"], true);
        let tx = tx_from_rpc_json(&doc["rpc"]).unwrap();
        let files: Vec<String> = doc["created"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        let mut created = vec![];
        for f in &files {
            let o: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
            let redeem = kpf_protocol::error::from_hex(o["redeem"].as_str().unwrap()).unwrap();
            let idx = o["outpoint"]["index"].as_u64().unwrap() as usize;
            created.push((idx, match decode_redeem(&redeem) {
                Ok(Decoded::Pair(p)) => Obj::Pair(p),
                Ok(Decoded::Open(o)) => Obj::Open(o),
                Ok(Decoded::Pending(w)) => Obj::Pending(w),
                Ok(Decoded::Lock(l)) => Obj::Lock(l),
                Err(_) => continue,
            }));
        }
        if tx.lock_time > self.chain.daa {
            let d = tx.lock_time - self.chain.daa;
            self.chain.advance(d);
        }
        self.chain.apply(&tx, &created).unwrap_or_else(|e| panic!("the chain refused {args:?}: {e}"));
        for f in &files {
            let mut o: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
            o["daa"] = json!(self.chain.daa);
            if o.get("custody").is_some_and(|c| !c.is_null()) {
                o["custody"]["daa"] = json!(self.chain.daa);
            }
            std::fs::write(f, serde_json::to_string_pretty(&o).unwrap()).unwrap();
        }
        (stdout, files)
    }
}

fn object_kind(f: &str) -> String {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
    v["kind"].as_str().unwrap().to_string()
}

fn outpoint_of(f: &str) -> TransactionOutpoint {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
    let txid = kpf_protocol::error::hex32(v["outpoint"]["txid"].as_str().unwrap()).unwrap();
    TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes(txid), index: v["outpoint"]["index"].as_u64().unwrap() as u32 }
}

fn kob_file(h: &H, name: &str, live: &kpf_protocol::sim::KobLive, custody: Option<&str>) -> String {
    let path = h.p(name);
    let op = format!("{}:{}", live.op.transaction_id, live.op.index);
    let (amount, daa, id) = (live.value.to_string(), live.daa.to_string(), kpf_protocol::error::to_hex(&live.id.as_bytes()));
    let redeem = kpf_protocol::error::to_hex(&live.obj.redeem());
    let mut args = vec!["kob-file", "--redeem", &redeem, "--outpoint", &op, "--amount", &amount, "--daa", &daa, "--covenant-id", &id, "--out", &path];
    if let Some(c) = custody {
        args.extend(["--custody", c]);
    }
    h.ok(&args);
    path
}

/// F2 (High), fixed (DESIGN 4.5g). An honest guarantor runs `kpf guarantee` on a pending-part file someone hands it: a
/// look-alike KpfPending (a genesis UTXO of the pinned template, its restore, cure and forfeit hashes computed from its
/// own state, its value at least q(n, bs - bl) + refund) whose market block has mmrBps = -20,000 and gBps = 20,000. Before
/// the fix the command printed "no quote in 0..=priceMax disputes this claim" and built the guarantee, and any ask above
/// bs then took the guarantor's lock. Now the command refuses the part before it says anything about disputes (its
/// market fails the bounds every order checks), builds nothing, and `inspect` warns that the part is a look-alike.
#[test]
fn holds_guarantee_refuses_a_look_alike_part_before_it_says_anything_about_disputes() {
    let mut h = H::new("lookalike");
    let (gk, g) = h.key("guarantor");
    h.chain.mint(&g, 100_000 * KAS);
    let attacker = kpf_protocol::fixtures::pubkey(66);
    let mut m = kpf_protocol::fixtures::kob_market();
    m.mmr_bps = -20_000;
    m.g_bps = 20_000;
    assert!(m.validate().is_err());
    let mut p = kpf_protocol::fixtures::pair(attacker, kpf_protocol::fixtures::pubkey(67));
    p.m = m;
    let bond = kpf_protocol::pending::bond_of(&p, p.size);
    let offer = bond - kpf_protocol::state::PLAIN_DUST;
    let po = kpf_protocol::pending::propose_out(&p, p.floor() as u64, p.size, KIND_LIQ, SHORT, p.bs - 1, attacker, 1_000_000_000_000, offer);
    let w = po.pending;
    assert!(w.hashes_match());
    let id = kaspa_consensus_core::Hash::from_bytes([0xd5; 32]);
    let daa = h.chain.daa;
    let live = h.chain.mint_obj(Obj::Pending(w.clone()), po.pending_value as u64, id, daa);
    let op = match &live {
        kpf_protocol::sim::Live::Pending(lp) => lp.op,
        _ => unreachable!(),
    };
    let file = h.p("handed-over-pending.json");
    let doc = json!({
        "outpoint": {"txid": op.transaction_id.to_string(), "index": op.index},
        "amount": po.pending_value, "daa": daa,
        "covenantId": kpf_protocol::error::to_hex(&id.as_bytes()),
        "redeem": kpf_protocol::error::to_hex(&w.redeem()),
    });
    std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    h.chain.advance(w.m.grace_daa as u64);
    let fund = h.funding("g", &g);
    let outdir = h.p("out-lookalike");
    let (ok, out, err) = h.run(&["guarantee", "--pending", &file, "--funder", &gk, "--funding", &fund, "--out-dir", &outdir]);
    assert!(!ok, "the guarantee is refused: {out}");
    assert!(err.contains("market block fails the bounds"), "{err}");
    assert!(!out.contains("disputes this claim") && !out.contains("no quote"), "nothing is said about disputes: {out}");
    let built = std::fs::read_dir(&outdir).map(|mut d| d.next().is_some()).unwrap_or(false);
    assert!(!built, "nothing is built");
    let shown = h.ok(&["inspect", &file]);
    assert!(shown.contains("WARNING: the market block fails the bounds"), "{shown}");
}

/// F3 (Medium), fixed (DESIGN 4.5g). The loser of a pending liquidation that relies on `kpf watch`. Before the fix, at
/// P + G `watch` said "finalize from DAA P + W" and offered the cure without a word about the fast path, while a
/// guarantor could settle the claim at once and remove the cure. Now `watch` states, first, that a guarantor can settle
/// the claim once the cure grace has passed and when (DAA P + G), then the cure with that deadline, then the slow path;
/// at P + G it says the fast settlement is possible now; the JSON carries `guaranteeFrom`; the proposal itself says it
/// can be fast-settled. Wording note 1: once the claim is fast-settled, the lock's headline gives the last DAA a new
/// counter order can be placed (the rest deadline less R), not the rest deadline as the deadline to act.
#[test]
fn holds_watch_shows_the_fast_path_first_and_the_cure_before_the_grace_ends() {
    use kpf_protocol::fixtures::{ask_of, kob_market};
    use kpf_protocol::kob::*;

    let mut h = H::new("watch");
    let m = kob_market();
    let market = h.p("market.json");
    std::fs::write(&market, serde_json::to_string(&m.to_json()).unwrap()).unwrap();
    let (lk, _l) = h.key("long");
    let (sk, _s) = h.key("short");
    let (fk, f) = h.key("executor");
    let (gk, g) = h.key("guarantor");
    let (_mk, maker) = h.key("maker");
    for _ in 0..3 {
        h.chain.mint(&f, 100_000 * KAS);
    }
    h.chain.mint(&g, 100_000 * KAS);
    let fund = h.funding("f", &f);
    let (_, lo) = h.step(&[
        "create-order", "--market", &market, "--side", "long", "--size", "2000", "--limit", "10.5", "--rate", "1", "--min-counter-rate", "1.5", "--min-fill", "100",
        "--tip", "0.01", "--expiry", "1000000000", "--owner", &lk, "--funder", &fk, "--funding", &fund,
    ]);
    let fund = h.funding("f", &f);
    let (_, so) = h.step(&[
        "create-order", "--market", &market, "--side", "short", "--size", "1000", "--limit", "9.5", "--rate", "2", "--min-counter-rate", "0.5", "--min-fill", "100",
        "--tip", "0.01", "--expiry", "1000000000", "--owner", &sk, "--funder", &fk, "--funding", &fund,
    ]);
    let fund = h.funding("f", &f);
    let (_, opened) = h.step(&["open", "--long", &lo[0], "--short", &so[0], "--n", "1000", "--x", "10", "--funder", &fk, "--funding", &fund]);
    let pair = opened.iter().find(|p| object_kind(p) == "pair").cloned().expect("pair");
    // A stale ask at 9.40 and the liquidation proposal on it.
    let now = h.chain.daa;
    let ask_id = kaspa_consensus_core::Hash::from_bytes([0xe1; 32]);
    let ask = h.chain.mint_kob(KobObj::Ask(ask_of(&m, maker, 940_000_000, 1_500_000_000)), 10 * KAS, ask_id, now);
    let custody = h.chain.mint_kob(KobObj::Token(TokenState::covid(1_500_000_000, ask_id, m.ext)), 20_000_000, m.token_cov_id, now);
    let custody_file = kob_file(&h, "custody.json", &custody, None);
    let ask_file = kob_file(&h, "ask.json", &ask, Some(&custody_file));
    h.chain.advance(650);
    let fund = h.funding("f", &f);
    let (proposed, claimed) = h.step(&["propose-liquidate", "--pair", &pair, "--evidence", &ask_file, "--funder", &fk, "--funding", &fund]);
    assert!(proposed.contains("it can also be fast-settled") && proposed.contains("no cure"), "{proposed}");
    let pending = claimed.iter().find(|p| object_kind(p) == "pending").cloned().expect("pending");
    let p_daa = h.chain.daa;
    let from = p_daa + m.grace_daa as u64;
    // Right after the proposal: the fast-path deadline first, then the cure with that deadline, then the slow path.
    let now = p_daa + 10;
    let out = h.ok(&["watch", "--pair", &pair, "--pending", &pending, "--daa", &now.to_string()]);
    let fast = out.find(&format!("FAST SETTLEMENT: a guarantor may settle this liquidation once the cure grace has passed: from DAA {from}, {} DAA", from - now));
    let cure = out.find(&format!("CURE before DAA {from}"));
    let slow = out.find("slow path: finalize from DAA");
    let (fast, cure, slow) = (fast.expect(&out), cure.expect(&out), slow.expect(&out));
    assert!(fast < cure && cure < slow, "fast path, then the cure, then the slow path: {out}");
    assert!(out.contains("no cure and no challenge"), "{out}");
    let v: Value = serde_json::from_str(&h.ok(&["watch", "--pair", &pair, "--pending", &pending, "--daa", &now.to_string(), "--json"])).unwrap();
    assert_eq!(v["pairs"][0]["pending"][0]["guaranteeFrom"], from);
    assert_eq!(v["pairs"][0]["pending"][0]["guaranteeInDaa"], from - now);
    // At P + G: possible now.
    h.chain.advance(m.grace_daa as u64);
    let now = h.chain.daa;
    let out = h.ok(&["watch", "--pair", &pair, "--pending", &pending, "--daa", &now.to_string()]);
    assert!(out.contains("possible NOW"), "{out}");
    // A guarantor settles it; the lock's headline gives the last DAA to place a new counter order.
    let fund = h.funding("g", &g);
    let (_, settled) = h.step(&["guarantee", "--pending", &pending, "--funder", &gk, "--funding", &fund]);
    let lock = settled.iter().find(|p| object_kind(p) == "lock").cloned().expect("the lock");
    assert!(h.chain.entry(&outpoint_of(&pending)).is_none(), "the cure now has nothing to spend");
    let f_daa = h.chain.daa;
    let place_by = f_daa as i64 + m.dispute_daa - 2 * m.min_rest_daa;
    let out = h.ok(&["watch", "--pair", &pair, "--lock", &lock, "--daa", &f_daa.to_string()]);
    assert!(out.contains(&format!("a new counter order must be placed by DAA {place_by}")), "{out}");
}
