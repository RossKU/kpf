//! Regenerates and checks the pinned compiled artifacts of `KpfPair` and `KpfOpen` (`contracts/legacy/artifacts/`).
//!
//! This is the only crate that links the SilverScript compiler (KOB's vendored v1.0.0, by path into `vendor/kob`).
//!
//! What is pinned (all under `contracts/legacy/artifacts/`):
//! - `KpfPair.json`, `KpfOpen.json`: the compiler's `SilAbiArtifact` of each contract, compiled with a fixed sample
//!   state (the template prefix and suffix, the ABI, the template hash);
//! - `manifest.json`: source and artifact digests, template hashes and cuts, and the `KobAsk` / `KobBid` evidence
//!   templates (hash, prefix and suffix length) that `KpfPair` is built with;
//! - `SHA256SUMS`: digests of the sources and of all of the above.
//!
//! Regenerate after any change to a contract (or the compiler pin) with
//! `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`; `check` (and `cargo test -p kpf-artifacts`) fails
//! on any difference between a fresh compile and the committed files.

use std::path::{Path, PathBuf};

use kpf_protocol::artifacts::{template_hash_of, KobConsts};
use kpf_protocol::state::{Lock, Market, Open, Pair, Pending, KIND_LIQ, LONG};
use kpf_protocol::tx::pubkey_of;
use serde_json::json;
use sha2::{Digest, Sha256};
use silverscript_abi::{ArtifactValue, SilAbiArtifact};
use silverscript_lang::compiler::{compile_to_sil_abi_artifact_with_options, CompileOptions};

/// The commit of the KOB checkout the compiler and the evidence templates come from (`README.md`).
pub const KOB_COMMIT: &str = "ed16148";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("KPF root")
}

fn read_norm(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).replace("\r\n", "\n")
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn b32(b: [u8; 32]) -> ArtifactValue {
    ArtifactValue::Bytes(b.to_vec())
}
fn int(v: i64) -> ArtifactValue {
    ArtifactValue::Int(v)
}

fn compile(name: &str, src: &str, ctor: &[ArtifactValue]) -> SilAbiArtifact {
    compile_to_sil_abi_artifact_with_options(src, ctor, CompileOptions::default()).unwrap_or_else(|e| panic!("compile {name}: {e:?}"))
}

fn sample_market() -> Market {
    Market {
        token_cov_id: kaspa_hash([0xc1; 32]),
        token_tpl_hash: kaspa_hash([0xc2; 32]),
        ext: kaspa_hash([0xc3; 32]),
        tpl_prefix_len: 1,
        tpl_suffix_len: 1,
        scale: 1_000_000,
        mmr_bps: 500,
        k_bps: 10_000,
        min_rest_daa: 600,
        liq_fee_bps: 50,
        keeper_fee_min: 1_000_000,
        min_size: 1_000_000,
        carrier: 20_000_000,
        idle_daa: 77_760_000,
        price_max: 100_000_000_000_000,
        bond_bps: 300,
        bond_min: 500_000_000,
        kc_bps: 10_000,
        g_bps: 200,
        challenge_daa: 6_000,
        min_evidence_life: 864_000,
        lock_bps: 11_000,
        dispute_daa: 6_600,
        grace_daa: 1_200,
    }
}

fn kaspa_hash(b: [u8; 32]) -> kaspa_consensus_core::Hash {
    kaspa_consensus_core::Hash::from_bytes(b)
}

/// The template facts of a compiled artifact: (hash, prefix length, suffix length).
fn facts(a: &SilAbiArtifact) -> ([u8; 32], i64, i64) {
    let c = &a.contracts.values().next().expect("one contract").compiled;
    let (pre, _, suf) = c.script_parts(&c.bytecode).expect("state span");
    assert_eq!(template_hash_of(pre, suf), c.template_hash, "template hash formula");
    (c.template_hash, pre.len() as i64, suf.len() as i64)
}

/// Everything `write` puts on disk, as (path relative to `contracts/`, content).
pub fn generate() -> Vec<(String, String)> {
    let r = root();
    let kob = r.join("vendor/kob/contracts/orders");
    let ask_src = read_norm(&kob.join("KobAsk.sil"));
    let bid_src = read_norm(&kob.join("KobBid.sil"));
    let pair_src = read_norm(&r.join("contracts/legacy/KpfPair.sil"));
    let open_src = read_norm(&r.join("contracts/legacy/KpfOpen.sil"));
    let pending_src = read_norm(&r.join("contracts/legacy/KpfPending.sil"));
    let lock_src = read_norm(&r.join("contracts/legacy/KpfLock.sil"));
    let m = sample_market();

    // KobAsk / KobBid: any valid constructor; the template does not depend on the state.
    let ask = compile(
        "KobAsk",
        &ask_src,
        &[
            vec![b32([3; 32]), b32(m.token_cov_id.as_bytes()), b32(m.token_tpl_hash.as_bytes())],
            [1, 1, m.scale, 1, 250_000_000, 0, 0, 0, 1_000_000_000_000, 0, 0, 0, 0, 0, 1, 10].map(int).to_vec(),
            vec![b32(m.ext.as_bytes())],
        ]
        .concat(),
    );
    let bid = compile(
        "KobBid",
        &bid_src,
        &[
            vec![b32([3; 32]), b32(m.token_cov_id.as_bytes()), b32(m.token_tpl_hash.as_bytes()), int(1), int(1), b32(m.ext.as_bytes())],
            [m.scale, 1_000_000, 250_000_000, 0, 0, 0, 1_000_000_000_000, 0, 0, 20_000_000, 0, 0, 0, 0, 1].map(int).to_vec(),
        ]
        .concat(),
    );
    let (at, ap, asf) = facts(&ask);
    let (bt, bp, bsf) = facts(&bid);
    let kc = KobConsts { ask_tpl: at, ask_pre: ap, ask_suf: asf, bid_tpl: bt, bid_pre: bp, bid_suf: bsf };

    let (k1, k2, k3) = (pubkey_of(&[1; 32]).unwrap(), pubkey_of(&[2; 32]).unwrap(), pubkey_of(&[3; 32]).unwrap());
    // KpfLock first: its template hash is a build constant of KpfPending (DESIGN 4.5f; it needs no pending template).
    let sample_lock = Lock {
        owner: k3,
        status: 1,
        m: m.clone(),
        claim_id: kaspa_hash([0x2d; 32]),
        kind: KIND_LIQ,
        side: LONG,
        n: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        quote: 940_000_000,
        clock: 1,
        cover: 300_000_000_000,
        victim: k1,
    };
    let lock = compile("KpfLock", &lock_src, &sample_lock.ctor_with(&kc));
    let (lt, _, _) = facts(&lock);
    // KpfPending next: its template hash is a build constant of KpfPair (DESIGN 4.5d; no cycle, it needs no pair
    // template).
    let sample_pending = Pending {
        m: m.clone(),
        kind: KIND_LIQ,
        side: LONG,
        n: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        quote: 940_000_000,
        cure_bound: 893_000_000,
        fee: 4_700_000_000,
        bond: 9_000_000_000,
        refund: 9_000_000_000,
        visible_until: 87_000_000,
        offer: 0,
        entry: 1_000_000_000,
        long_key: k1,
        short_key: k2,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        since: 1,
        proposer_key: k3,
        restore_hash: [0x2a; 32],
        cure_hash: [0x2b; 32],
        forfeit_hash: [0x2c; 32],
    };
    let pending = compile("KpfPending", &pending_src, &sample_pending.ctor_with(&kc, lt));
    let (wt, _, _) = facts(&pending);
    let sample_pair = Pair {
        m: m.clone(),
        size: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 1_000_000_000,
        long_key: k1,
        short_key: k2,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: kaspa_hash([0x1a; 32]),
        tag_s: kaspa_hash([0x1b; 32]),
        since: 0,
    };
    let pair = compile("KpfPair", &pair_src, &sample_pair.ctor_with(&kc, wt));
    let (pt, _, _) = facts(&pair);
    let sample_open = Open {
        m: m.clone(),
        side: LONG,
        owner: k1,
        size_left: 2_000_000_000,
        limit: 1_050_000_000,
        my_rate: 100_000_000,
        min_counter_rate: 150_000_000,
        min_fill: 100_000_000,
        tip: 1_000_000,
        active_from: 0,
        expiry_daa: 1_000_000_000,
    };
    let open = compile("KpfOpen", &open_src, &sample_open.ctor_with(pt));

    let json = |a: &SilAbiArtifact| format!("{}\n", serde_json::to_string(a).expect("artifact json"));
    let (pair_json, open_json, pending_json, lock_json) = (json(&pair), json(&open), json(&pending), json(&lock));
    let contract = |src: &str, file: &str, a: &SilAbiArtifact, art_json: &str| {
        let c = &a.contracts.values().next().expect("one contract").compiled;
        let (hash, pre, suf) = facts(a);
        let text = match file {
            "KpfPair.json" => &pair_src,
            "KpfOpen.json" => &open_src,
            "KpfLock.json" => &lock_src,
            _ => &pending_src,
        };
        json!({
            "source": src, "source_sha256": sha256_hex(text.as_bytes()),
            "artifact": file, "artifact_sha256": sha256_hex(art_json.as_bytes()),
            "template_hash": hex(&hash), "prefix_len": pre, "suffix_len": suf, "state_len": c.state_span.len,
        })
    };
    let kobc = |src: &str, text: &str, f: ([u8; 32], i64, i64)| {
        json!({ "source": src, "source_sha256": sha256_hex(text.as_bytes()), "template_hash": hex(&f.0), "prefix_len": f.1, "suffix_len": f.2 })
    };
    let manifest = json!({
        "format": "kpf-artifacts/1",
        "compiler": format!("silverscript-lang (vendor/kob @ {KOB_COMMIT}, SilverScript v1.0.0), CompileOptions::default(); {}", pair.compiler_version),
        "regenerate": "CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write",
        "kob_commit": KOB_COMMIT,
        "kob_ask": kobc("contracts/orders/KobAsk.sil", &ask_src, (at, ap, asf)),
        "kob_bid": kobc("contracts/orders/KobBid.sil", &bid_src, (bt, bp, bsf)),
        "kpf_pair": contract("contracts/legacy/KpfPair.sil", "KpfPair.json", &pair, &pair_json),
        "kpf_open": contract("contracts/legacy/KpfOpen.sil", "KpfOpen.json", &open, &open_json),
        "kpf_pending": contract("contracts/legacy/KpfPending.sil", "KpfPending.json", &pending, &pending_json),
        "kpf_lock": contract("contracts/legacy/KpfLock.sil", "KpfLock.json", &lock, &lock_json),
    });
    let manifest_text = format!("{}\n", serde_json::to_string_pretty(&manifest).expect("manifest"));
    let sums = [
        ("legacy/KpfPair.sil", pair_src.clone()),
        ("legacy/KpfOpen.sil", open_src.clone()),
        ("legacy/KpfPending.sil", pending_src.clone()),
        ("legacy/KpfLock.sil", lock_src.clone()),
        ("legacy/artifacts/KpfPair.json", pair_json.clone()),
        ("legacy/artifacts/KpfOpen.json", open_json.clone()),
        ("legacy/artifacts/KpfPending.json", pending_json.clone()),
        ("legacy/artifacts/KpfLock.json", lock_json.clone()),
        ("legacy/artifacts/manifest.json", manifest_text.clone()),
    ]
    .iter()
    .map(|(p, t)| format!("{}  {p}\n", sha256_hex(t.as_bytes())))
    .collect::<String>();
    let mut files = vec![
        ("legacy/artifacts/KpfPair.json".into(), pair_json),
        ("legacy/artifacts/KpfOpen.json".into(), open_json),
        ("legacy/artifacts/KpfPending.json".into(), pending_json),
        ("legacy/artifacts/KpfLock.json".into(), lock_json),
        ("legacy/artifacts/manifest.json".into(), manifest_text),
        ("legacy/artifacts/SHA256SUMS".into(), sums),
    ];
    files.extend(generate_oracle(&r));
    files.extend(generate_committee(&r));
    files.extend(generate_thin(&r));
    files
}

fn tag_of(a: &SilAbiArtifact, entry: &str) -> [u8; 4] {
    *a.contracts.values().next().expect("one contract").entries[entry].dispatch_tag.as_bytes()
}

/// The oracle-market family (docs/DESIGN-ORACLE-IMPL.md, third slice), pinned under `contracts/legacy/artifacts/oracle/` with
/// its own manifest and `SHA256SUMS`: the slot `KpfBond`, the judge `KpfStake`, and `KpfPendingO`, `KpfPairO`,
/// `KpfOpenO`, each compiled with the build constants of the ones it pins (KpfOpenO -> KpfPairO -> KpfPendingO ->
/// KpfStake; KpfPairO, KpfOpenO -> KpfBond).
fn generate_oracle(r: &Path) -> Vec<(String, String)> {
    use kpf_protocol::bond::Bond;
    use kpf_protocol::judge::Js;
    use kpf_protocol::oracle::{samples, OConsts};
    let names = ["KpfBond", "KpfStake", "KpfPendingO", "KpfPairO", "KpfOpenO"];
    let srcs: Vec<String> = names.iter().map(|n| read_norm(&r.join(format!("contracts/legacy/{n}.sil")))).collect();
    let om = samples::market();
    let k3 = pubkey_of(&[3; 32]).unwrap();
    let k1 = pubkey_of(&[1; 32]).unwrap();
    let bond = compile("KpfBond", &srcs[0], &Bond::new(k3, om.bond_market(), 0).ctor());
    let stake = compile("KpfStake", &srcs[1], &Js::free(k3, om.judge_market()).ctor());
    let (bt, bp, bsf) = facts(&bond);
    let (st, sp, ssf) = facts(&stake);
    let mut consts = OConsts {
        bond_tpl: bt,
        bond_pre: bp,
        bond_suf: bsf,
        bind_tag: tag_of(&bond, "bind"),
        judge_tpl: st,
        judge_pre: sp,
        judge_suf: ssf,
        open_tag: tag_of(&stake, "open"),
        resolve_tag: tag_of(&stake, "resolve"),
        pending_tpl: [0; 32],
        pair_tpl: [0; 32],
    };
    let sp_pair = samples::pair(&om);
    let pending = compile("KpfPendingO", &srcs[2], &samples::pending(&sp_pair).ctor_with(&consts));
    consts.pending_tpl = facts(&pending).0;
    let pair = compile("KpfPairO", &srcs[3], &sp_pair.ctor_with(&consts));
    consts.pair_tpl = facts(&pair).0;
    let open = compile("KpfOpenO", &srcs[4], &samples::order(&om, LONG, k1).ctor_with(&consts));
    let arts = [&bond, &stake, &pending, &pair, &open];
    let json = |a: &SilAbiArtifact| format!("{}\n", serde_json::to_string(a).expect("artifact json"));
    let jsons: Vec<String> = arts.iter().map(|a| json(a)).collect();
    let mut contracts = serde_json::Map::new();
    for (i, n) in names.iter().enumerate() {
        let c = &arts[i].contracts.values().next().expect("one contract").compiled;
        let (hash, pre, suf) = facts(arts[i]);
        contracts.insert(
            n.to_string(),
            json!({
                "source": format!("contracts/legacy/{n}.sil"), "source_sha256": sha256_hex(srcs[i].as_bytes()),
                "artifact": format!("{n}.json"), "artifact_sha256": sha256_hex(jsons[i].as_bytes()),
                "template_hash": hex(&hash), "prefix_len": pre, "suffix_len": suf, "state_len": c.state_span.len,
            }),
        );
    }
    let manifest = json!({
        "format": "kpf-artifacts/oracle-1",
        "compiler": format!("silverscript-lang (vendor/kob @ {KOB_COMMIT}, SilverScript v1.0.0), CompileOptions::default(); {}", pair.compiler_version),
        "regenerate": "CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write",
        "contracts": contracts,
        "tags": { "bind": hex(&consts.bind_tag), "open": hex(&consts.open_tag), "resolve": hex(&consts.resolve_tag) },
    });
    let manifest_text = format!("{}\n", serde_json::to_string_pretty(&manifest).expect("manifest"));
    let mut sums = String::new();
    for (i, n) in names.iter().enumerate() {
        sums.push_str(&format!("{}  legacy/{n}.sil\n", sha256_hex(srcs[i].as_bytes())));
    }
    for (i, n) in names.iter().enumerate() {
        sums.push_str(&format!("{}  legacy/artifacts/oracle/{n}.json\n", sha256_hex(jsons[i].as_bytes())));
    }
    sums.push_str(&format!("{}  legacy/artifacts/oracle/manifest.json\n", sha256_hex(manifest_text.as_bytes())));
    let mut files: Vec<(String, String)> = names.iter().zip(jsons).map(|(n, j)| (format!("legacy/artifacts/oracle/{n}.json"), j)).collect();
    files.push(("legacy/artifacts/oracle/manifest.json".into(), manifest_text));
    files.push(("legacy/artifacts/oracle/SHA256SUMS".into(), sums));
    files
}

/// The committee family (docs/DESIGN-COMMITTEE.md), pinned under `contracts/legacy/artifacts/committee/` with its own manifest
/// and `SHA256SUMS`: the committee `KpfCommittee`, its `KpfEpochTable` and `KpfExit`, and `KpfPendingC`, `KpfPairC`, `KpfOpenC`,
/// each compiled with the build constants of the ones it pins (KpfOpenC -> KpfPairC -> KpfPendingC, KpfEpochTable;
/// KpfCommittee -> KpfEpochTable, KpfExit).
fn generate_committee(r: &Path) -> Vec<(String, String)> {
    use kpf_protocol::cmarket::{samples, CConsts};
    let names = ["KpfEpochTable", "KpfExit", "KpfCommittee", "KpfPendingC", "KpfPairC", "KpfOpenC", "KpfCharge", "KpfPendingX", "KpfCrossC"];
    let srcs: Vec<String> = names.iter().map(|n| read_norm(&r.join(format!("contracts/legacy/{n}.sil")))).collect();
    let k1 = pubkey_of(&[1; 32]).unwrap();
    let epoch_table = compile("KpfEpochTable", &srcs[0], &samples::epoch_table().ctor());
    let (tt, tp, ts) = facts(&epoch_table);
    // The committee and the exit pin the charge's template; the charge names their scripts by their lengths and by the
    // tags of their `slash` and `clear` (a length and a tag do not depend on a hash, so nothing is circular): the three
    // are compiled until the charge's constants no longer change. The judge is the oracle family's pinned KpfStake.
    let ok = &kpf_protocol::oracle_artifacts::kit().consts;
    let len_of = |a: &SilAbiArtifact| a.contracts.values().next().expect("one contract").compiled.bytecode.len() as i64;
    let mut cc = kpf_protocol::committee::ChargeConsts {
        judge_tpl: ok.judge_tpl,
        judge_pre: ok.judge_pre,
        judge_suf: ok.judge_suf,
        open_tag: ok.open_tag,
        resolve_tag: ok.resolve_tag,
        committee_len: 9_000,
        exit_len: 2_000,
        committee_slash: [0; 4],
        committee_clear: [0; 4],
        exit_slash: [0; 4],
        exit_clear: [0; 4],
    };
    let mut rc = kpf_protocol::committee::RConsts { epoch_table_tpl: tt, exit_tpl: [0; 32], charge_tpl: [0; 32], charge_pre: 1, charge_suf: 1 };
    let mut built = None;
    let mut settled = false;
    for _ in 0..4 {
        let charge = compile("KpfCharge", &srcs[6], &samples::charge().ctor(&cc));
        let (ct, cp, cs) = facts(&charge);
        rc = kpf_protocol::committee::RConsts { charge_tpl: ct, charge_pre: cp, charge_suf: cs, ..rc };
        let exit = compile("KpfExit", &srcs[1], &samples::exit().ctor(&rc));
        rc.exit_tpl = facts(&exit).0;
        let committee = compile("KpfCommittee", &srcs[2], &samples::committee().ctor(&rc));
        let next = kpf_protocol::committee::ChargeConsts {
            committee_len: len_of(&committee),
            exit_len: len_of(&exit),
            committee_slash: tag_of(&committee, "slash"),
            committee_clear: tag_of(&committee, "clear"),
            exit_slash: tag_of(&exit, "slash"),
            exit_clear: tag_of(&exit, "clear"),
            ..cc
        };
        let done = next == cc;
        cc = next;
        built = Some((charge, exit, committee));
        if done {
            settled = true;
            break;
        }
    }
    let (charge, exit, committee) = built.expect("compiled");
    // Review C5 (C5-N2): the loop ended because a round changed nothing, not because it ran out of rounds; and the
    // charge compiled once more from the final constants is the one the committee and the exit pin.
    assert!(settled, "the charge's constants did not settle in four rounds");
    assert_eq!(facts(&compile("KpfCharge", &srcs[6], &samples::charge().ctor(&cc))).0, rc.charge_tpl, "the pinned charge is not the one compiled from the final constants");
    assert_eq!((cc.committee_len, cc.exit_len), (len_of(&committee), len_of(&exit)));
    let _ = &charge;
    let et = rc.exit_tpl;
    let mut consts = CConsts { epoch_table_tpl: tt, epoch_table_pre: tp, epoch_table_suf: ts, attest_tag: tag_of(&epoch_table, "attest"), exit_tpl: et, pending_tpl: [0; 32], pair_tpl: [0; 32] };
    let m = samples::market();
    let sp = samples::pair(&m);
    let pending = compile("KpfPendingC", &srcs[3], &samples::pending(&sp).ctor());
    consts.pending_tpl = facts(&pending).0;
    let pair = compile("KpfPairC", &srcs[4], &sp.ctor_with(&consts));
    consts.pair_tpl = facts(&pair).0;
    let open = compile("KpfOpenC", &srcs[5], &samples::order(&m, LONG, k1).ctor_with(&consts));
    // The cross-margin account pins its pending claim and the epoch table, as the pair does.
    let xs = kpf_protocol::cross::samples::cross();
    let pendx = compile("KpfPendingX", &srcs[7], &kpf_protocol::cross::samples::pending(&xs).ctor());
    let xc = kpf_protocol::cross::XConsts { pendx_tpl: facts(&pendx).0, epoch_table_tpl: tt, epoch_table_pre: tp, epoch_table_suf: ts, attest_tag: consts.attest_tag };
    let cross = compile("KpfCrossC", &srcs[8], &xs.ctor_with(&xc));
    let arts = [&epoch_table, &exit, &committee, &pending, &pair, &open, &charge, &pendx, &cross];
    let json = |a: &SilAbiArtifact| format!("{}\n", serde_json::to_string(a).expect("artifact json"));
    let jsons: Vec<String> = arts.iter().map(|a| json(a)).collect();
    let mut contracts = serde_json::Map::new();
    for (i, n) in names.iter().enumerate() {
        let c = &arts[i].contracts.values().next().expect("one contract").compiled;
        let (hash, pre, suf) = facts(arts[i]);
        contracts.insert(
            n.to_string(),
            json!({
                "source": format!("contracts/legacy/{n}.sil"), "source_sha256": sha256_hex(srcs[i].as_bytes()),
                "artifact": format!("{n}.json"), "artifact_sha256": sha256_hex(jsons[i].as_bytes()),
                "template_hash": hex(&hash), "prefix_len": pre, "suffix_len": suf, "state_len": c.state_span.len,
            }),
        );
    }
    let manifest = json!({
        "format": "kpf-artifacts/committee-1",
        "compiler": format!("silverscript-lang (vendor/kob @ {KOB_COMMIT}, SilverScript v1.0.0), CompileOptions::default(); {}", pair.compiler_version),
        "regenerate": "CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write",
        "contracts": contracts,
        "tags": {
            "attest": hex(&consts.attest_tag),
            "committee_slash": hex(&cc.committee_slash), "committee_clear": hex(&cc.committee_clear),
            "exit_slash": hex(&cc.exit_slash), "exit_clear": hex(&cc.exit_clear),
        },
        "lengths": { "committee": cc.committee_len, "exit": cc.exit_len },
    });
    let manifest_text = format!("{}\n", serde_json::to_string_pretty(&manifest).expect("manifest"));
    let mut sums = String::new();
    for (i, n) in names.iter().enumerate() {
        sums.push_str(&format!("{}  legacy/{n}.sil\n", sha256_hex(srcs[i].as_bytes())));
    }
    for (i, n) in names.iter().enumerate() {
        sums.push_str(&format!("{}  legacy/artifacts/committee/{n}.json\n", sha256_hex(jsons[i].as_bytes())));
    }
    sums.push_str(&format!("{}  legacy/artifacts/committee/manifest.json\n", sha256_hex(manifest_text.as_bytes())));
    let mut files: Vec<(String, String)> = names.iter().zip(jsons).map(|(n, j)| (format!("legacy/artifacts/committee/{n}.json"), j)).collect();
    files.push(("legacy/artifacts/committee/manifest.json".into(), manifest_text));
    files.push(("legacy/artifacts/committee/SHA256SUMS".into(), sums));
    files
}

/// The thin family, the main one (docs/DESIGN-COMMITTEE.md 8a), pinned under `contracts/artifacts/` with its manifest
/// and `SHA256SUMS`: `KpfSlot`, the one script of every object, and the scripts of the operations, each compiled as the
/// library holds it (version 1). An object's script is the slot's with two values of 32 bytes in it, the library's id
/// and the hash of the object's state: the manifest says where they are, found here by compiling with other values.
/// What stands after the state's hash in a price note's script (`byte[N] NOTE_SUF` of KpfTNote and KpfTCite).
const NOTE_SUF_LEN: usize = 55;
/// What stands between the state's hash and the writer's key there.
const NOTE_MID_LEN: usize = 1;

fn generate_thin(r: &Path) -> Vec<(String, String)> {
    let names = ["KpfSlot", "KpfTAdd", "KpfTBoth", "KpfTClose", "KpfTSettle", "KpfTLate", "KpfTDispute", "KpfTProve", "KpfTLiq", "KpfTLiqSettle", "KpfTCure", "KpfTLiqDispute", "KpfTLiqProve", "KpfTExit", "KpfTExitEnd", "KpfTExitRefute", "KpfTMatch", "KpfTCancel", "KpfTMerge", "KpfTNovate", "KpfTPlace", "KpfTOpen", "KpfTJoin", "KpfTTurn", "KpfTFold", "KpfTLeave", "KpfTReplace", "KpfTNote", "KpfTCite", "KpfTChange", "KpfTChangeSettle", "KpfTChangeDispute", "KpfTChangeLate", "KpfTTake", "KpfTKind", "KpfTPoint", "KpfTStepRow", "KpfTStepPrice", "KpfTStepEnd", "KpfTStepLiq", "KpfTShardOpen", "KpfTOpenC", "KpfTLegAdd", "KpfTAddC", "KpfTNoteApply", "KpfTAcctEnd", "KpfTShardEnd", "KpfTShardPrice", "KpfTDrawNote", "KpfTHand", "KpfTLegMove", "KpfTLegOut", "KpfTEnter", "KpfTStepEnter", "KpfTLeft", "KpfTLeftAll", "KpfTLeftVoid", "KpfTCRow", "KpfTCEnd", "KpfTEnterS", "KpfTCEnter", "KpfTEnterOff", "KpfTDrawEntry", "KpfTSweep", "KpfTPay", "KpfNote", "KpfFeePool"];
    let srcs: Vec<String> = names.iter().map(|n| read_norm(&r.join(format!("contracts/{n}.sil")))).collect();
    let code = |a: &SilAbiArtifact| a.contracts.values().next().expect("one contract").compiled.bytecode.clone();
    let (lib, state) = ([0x4c_u8; 32], [0x5a_u8; 32]);
    let slot_with = |l: [u8; 32], h: [u8; 32]| compile("KpfSlot", &srcs[0], &[ArtifactValue::Bytes(l.to_vec()), ArtifactValue::Bytes(h.to_vec())]);
    let slot = slot_with(lib, state);
    let s = code(&slot);
    let at = |other: Vec<u8>| -> usize {
        assert_eq!(other.len(), s.len(), "KpfSlot's length does not depend on its values");
        let d: Vec<usize> = (0..s.len()).filter(|i| s[*i] != other[*i]).collect();
        assert!(d.len() == 32 && d[31] - d[0] == 31, "a value of KpfSlot is one run of 32 bytes");
        d[0]
    };
    let lib_at = at(code(&slot_with([0x4d; 32], state)));
    let state_at = at(code(&slot_with(lib, [0x5b; 32])));
    assert_eq!(state_at, 2, "the scripts of the library read the state's hash at byte 2 of an object");
    let mut arts = vec![slot];
    // The proofs read the committee's epoch table by its pinned template (the committee family's artifacts on disk).
    let tk = &kpf_protocol::committee_artifacts::xkit().consts;
    let epoch_table = [ArtifactValue::Bytes(tk.epoch_table_tpl.to_vec()), ArtifactValue::Int(tk.epoch_table_pre), ArtifactValue::Int(tk.epoch_table_suf), ArtifactValue::Bytes(tk.attest_tag.to_vec()), ArtifactValue::Int(1)];
    // A price note is an object with two values, the hash of its state (where an object's is) and its writer's key:
    // its writer and its citation hold an output to its script by what stands before, between and after them.
    let note_src = &srcs[names.iter().position(|n| *n == "KpfNote").expect("KpfNote")];
    let owner = [0x6f_u8; 32];
    let note_with = |h: [u8; 32], o: [u8; 32]| code(&compile("KpfNote", note_src, &[ArtifactValue::Bytes(h.to_vec()), ArtifactValue::Bytes(o.to_vec())]));
    let note_code = note_with(state, owner);
    let note_at = |other: Vec<u8>| -> usize {
        assert_eq!(other.len(), note_code.len(), "KpfNote's length does not depend on its values");
        let d: Vec<usize> = (0..note_code.len()).filter(|i| note_code[*i] != other[*i]).collect();
        assert!(d.len() == 32 && d[31] - d[0] == 31, "a value of KpfNote is one run of 32 bytes");
        d[0]
    };
    let owner_at = note_at(note_with(state, [0x70; 32]));
    assert_eq!(note_at(note_with([0x5b; 32], owner)), state_at, "the note's state is where an object's is");
    assert_eq!((owner_at - state_at - 32, note_code.len() - owner_at - 32), (NOTE_MID_LEN, NOTE_SUF_LEN), "the note's script: KpfTNote and KpfTCite are written for these lengths");
    let note_parts = [ArtifactValue::Bytes(note_code[..state_at].to_vec()), ArtifactValue::Bytes(note_code[state_at + 32..owner_at].to_vec()), ArtifactValue::Bytes(note_code[owner_at + 32..].to_vec())];
    let note_writer = [epoch_table[0].clone(), epoch_table[1].clone(), epoch_table[2].clone(), epoch_table[3].clone(), note_parts[0].clone(), note_parts[1].clone(), note_parts[2].clone(), ArtifactValue::Int(1)];
    let cite = [note_parts[0].clone(), note_parts[1].clone(), note_parts[2].clone(), ArtifactValue::Int(1)];
    let note_ctor = [ArtifactValue::Bytes(state.to_vec()), ArtifactValue::Bytes(owner.to_vec())];
    // What reads a citation holds the citing input to KpfTCite's script (compiled before it in the list).
    let mut cite_spk: Vec<ArtifactValue> = vec![];
    for i in 1..names.len() {
        // The merge is compiled with what its sender keeps (the library's, the same for every account in it).
        let merge = [ArtifactValue::Int(kpf_protocol::thin::MERGE_TIP), ArtifactValue::Int(1)];
        let novate = [ArtifactValue::Int(kpf_protocol::thin::NOVATE_TIP), ArtifactValue::Int(1)];
        let purse = [ArtifactValue::Bytes(state.to_vec())];
        let ctor: &[ArtifactValue] = if names[i] == "KpfFeePool" { &purse } else if names[i] == "KpfNote" { &note_ctor } else if names[i] == "KpfTNote" { &note_writer } else if names[i] == "KpfTCite" { &cite } else if names[i] == "KpfTTake" || names[i] == "KpfTStepPrice" || names[i] == "KpfTShardPrice" { &cite_spk } else if names[i].ends_with("Prove") || names[i].ends_with("Refute") || names[i] == "KpfTPay" { &epoch_table } else if names[i] == "KpfTMerge" || names[i] == "KpfTJoin" || names[i] == "KpfTFold" { &merge } else if names[i] == "KpfTLeave" { &novate } else if names[i] == "KpfTNovate" { &novate } else { &[ArtifactValue::Int(1)] };
        arts.push(compile(names[i], &srcs[i], ctor));
        if names[i] == "KpfTCite" {
            let spk = kpf_protocol::tx::p2sh_spk(&code(arts.last().expect("KpfTCite")));
            cite_spk = vec![ArtifactValue::Bytes([&spk.version().to_le_bytes()[..], spk.script()].concat()), ArtifactValue::Int(1)];
        }
    }
    // The purse is an object as KpfSlot is, with one value: the scripts read its state's hash where they read an
    // object's.
    let purse_code = code(arts.last().expect("the purse"));
    let other = code(&compile("KpfFeePool", srcs.last().expect("the purse"), &[ArtifactValue::Bytes([0x5b; 32].to_vec())]));
    assert_eq!(purse_code.len(), other.len(), "KpfFeePool's length does not depend on its value");
    let d: Vec<usize> = (0..purse_code.len()).filter(|i| purse_code[*i] != other[*i]).collect();
    assert!(d.len() == 32 && d[0] == state_at && d[31] == state_at + 31, "the purse's state is where an object's is");
    let jsons: Vec<String> = arts.iter().map(|a| format!("{}\n", serde_json::to_string(a).expect("artifact json"))).collect();
    let mut contracts = serde_json::Map::new();
    for (i, n) in names.iter().enumerate() {
        contracts.insert(
            n.to_string(),
            json!({
                "source": format!("contracts/{n}.sil"), "source_sha256": sha256_hex(srcs[i].as_bytes()),
                "artifact": format!("{n}.json"), "artifact_sha256": sha256_hex(jsons[i].as_bytes()),
                "script_len": code(&arts[i]).len(),
            }),
        );
    }
    let manifest = json!({
        "format": "kpf-artifacts/thin-2",
        "compiler": format!("silverscript-lang (vendor/kob @ {KOB_COMMIT}, SilverScript v1.0.0), CompileOptions::default(); {}", arts[0].compiler_version),
        "regenerate": "CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write",
        "slot": { "lib_at": lib_at, "state_at": state_at, "sample_lib": hex(&lib), "sample_state": hex(&state) },
        "note": { "owner_at": owner_at, "sample_owner": hex(&owner) },
        "contracts": contracts,
    });
    let manifest_text = format!("{}\n", serde_json::to_string_pretty(&manifest).expect("manifest"));
    let mut sums = String::new();
    for (i, n) in names.iter().enumerate() {
        sums.push_str(&format!("{}  {n}.sil\n", sha256_hex(srcs[i].as_bytes())));
    }
    for (i, n) in names.iter().enumerate() {
        sums.push_str(&format!("{}  artifacts/{n}.json\n", sha256_hex(jsons[i].as_bytes())));
    }
    sums.push_str(&format!("{}  artifacts/manifest.json\n", sha256_hex(manifest_text.as_bytes())));
    let mut files: Vec<(String, String)> = names.iter().zip(jsons).map(|(n, j)| (format!("artifacts/{n}.json"), j)).collect();
    files.push(("artifacts/manifest.json".into(), manifest_text));
    files.push(("artifacts/SHA256SUMS".into(), sums));
    files
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Writes the generated files under `contracts/`.
pub fn write() -> Vec<String> {
    let r = root().join("contracts");
    generate()
        .into_iter()
        .map(|(rel, text)| {
            let p = r.join(&rel);
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
            }
            std::fs::write(p, text).unwrap_or_else(|e| panic!("{rel}: {e}"));
            rel
        })
        .collect()
}

/// The differences between a fresh compile and the committed files (empty when they agree).
pub fn check() -> Vec<String> {
    let r = root().join("contracts");
    generate()
        .into_iter()
        .filter_map(|(rel, text)| match std::fs::read_to_string(r.join(&rel)) {
            Ok(have) if have.replace("\r\n", "\n") == text => None,
            Ok(_) => Some(format!("contracts/{rel} differs from a fresh compile")),
            Err(e) => Some(format!("contracts/{rel}: {e}")),
        })
        .collect()
}
