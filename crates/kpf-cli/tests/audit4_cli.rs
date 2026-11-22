//! Review 4 (docs/audit/REVIEW-4.md), the `kpf` command line on the two-step evidence paths. Offline: no node, nothing
//! submitted; object files are written by hand as a counterparty or an indexer would hand them over.

use std::path::PathBuf;
use std::process::Command;

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::build::{Funding, Opts};
use kpf_protocol::error::to_hex;
use kpf_protocol::evidence::{challenge, EvidenceUse};
use kpf_protocol::fixtures::{bid_escrow, bid_of, kob_market, pair, pubkey};
use kpf_protocol::kob::{Evidence, KobObj, LiveBid, LiveToken, TokenState};
use kpf_protocol::pending::pending_of;
use kpf_protocol::state::{KIND_LIQ, LONG};
use kpf_protocol::tx::{KeyUtxo, LivePending};
use serde_json::json;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn op(b: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([b; 32]), index: 0 }
}

fn obj(path: &PathBuf, o: &TransactionOutpoint, amount: u64, daa: u64, cov: Hash, redeem: &[u8]) {
    let v = json!({
        "outpoint": { "txid": o.transaction_id.to_string(), "index": o.index },
        "amount": amount, "daa": daa, "covenantId": cov.to_string(), "redeem": to_hex(redeem),
    });
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

fn kpf(args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_kpf")).args(args).output().expect("run kpf");
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

/// R4-F2 (Medium), fixed (DESIGN 4.5e). `watch` and `evidence --pending` say a counter order refutes a pending claim only
/// when its quote contradicts by g, it has rested on the proposal's clock, AND it can fill k_c times the pending part, as
/// KpfPending.challenge requires (n_ev >= ceil(kcBps * n / 10^4)). A stale liquidation of 1,000 tokens and a refuting bid
/// at 9.80 that can buy only about 100 tokens: neither command says it refutes (both say why: fewer tokens than the
/// challenge must fill), `evidence --pending` exits non-zero, and the challenge builder refuses the order as before. The
/// same bid funded for 1,000 tokens is reported as refuting by both.
#[test]
fn holds_watch_does_not_report_an_order_that_cannot_cover_the_part() {
    let dir = Dir(std::env::temp_dir().join(format!("kpf-audit4-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    let m = kob_market();
    let mut p = pair(pubkey(1), pubkey(2));
    p.m = m.clone();
    let id = Hash::from_bytes([0x31; 32]);
    let pair_daa = 1_000u64;
    let w = pending_of(&p.clocked(pair_daa), p.size, KIND_LIQ, LONG, 940_000_000, pubkey(3));
    let p_daa = 2_000u64;
    let wv = (p.floor() + w.bond) as u64;
    let pf = dir.0.join("pair.json");
    let wf = dir.0.join("pending.json");
    let bf = dir.0.join("bid.json");
    obj(&pf, &op(0x41), p.floor() as u64, pair_daa, id, &p.redeem());
    obj(&wf, &op(0x42), wv, p_daa, id, &w.redeem());
    // A bid at 9.80 (above the refutation bound 9.6631579) funded for 100 tokens, placed at P.
    let b = bid_of(&m, pubkey(4), 980_000_000);
    let bv = bid_escrow(&b, 100_000_000);
    let bid_id = Hash::from_bytes([0xe7; 32]);
    obj(&bf, &op(0x43), bv, p_daa, bid_id, &KobObj::Bid(b.clone()).redeem());
    let now = (p_daa + m.min_rest_daa as u64 + 10).to_string();
    let (pf, wf, bf) = (pf.display().to_string(), wf.display().to_string(), bf.display().to_string());

    let (ok, out) = kpf(&["watch", "--pair", &pf, "--pending", &wf, "--order", &bf, "--daa", &now]);
    assert!(ok, "{out}");
    assert!(out.contains("PENDING") && !out.contains("REFUTES IT") && !out.contains("kpf challenge --pending"), "{out}");
    assert!(out.contains("does not refute it") && out.contains("a challenge must fill"), "{out}");
    let (ok, out) = kpf(&["evidence", "--order", &bf, "--pending", &wf, "--daa", &now]);
    assert!(!ok && !out.contains("REFUTES the claim now") && out.contains("a challenge must fill"), "{out}");
    // Control: the same bid funded for the whole part refutes it in both commands.
    let big = dir.0.join("bigbid.json");
    let big_s = big.display().to_string();
    obj(&big, &op(0x46), bid_escrow(&b, 1_000_000_000), p_daa, Hash::from_bytes([0xe8; 32]), &KobObj::Bid(b.clone()).redeem());
    let (ok, out) = kpf(&["watch", "--pair", &pf, "--pending", &wf, "--order", &big_s, "--daa", &now]);
    assert!(ok && out.contains("REFUTES IT") && out.contains("kpf challenge --pending"), "{out}");
    let (ok, out) = kpf(&["evidence", "--order", &big_s, "--pending", &wf, "--daa", &now]);
    assert!(ok && out.contains("REFUTES the claim now"), "{out}");
    // R4-F4 in `watch`: a look-alike of the same claim holding 1 KAS is not reported as a claim against the pair.
    let lf = dir.0.join("lookalike.json");
    obj(&lf, &op(0x47), 100_000_000, p_daa, id, &w.redeem());
    let lf = lf.display().to_string();
    let (ok, out) = kpf(&["watch", "--pair", &pf, "--pending", &lf, "--order", &big_s, "--daa", &now]);
    assert!(ok && out.contains("NOT a pending part of a real proposal") && !out.contains("REFUTES IT"), "{out}");
    let (ok, out) = kpf(&["evidence", "--order", &big_s, "--pending", &lf, "--daa", &now]);
    assert!(!ok && out.contains("look-alike"), "{out}");

    // What the contract needs: a fill of at least k_c * n = 1,000 tokens; the bid can buy about 100.
    let ev = Evidence::Bid { order: LiveBid { b, value: bv, id: bid_id, op: op(0x43), daa: p_daa } };
    assert!(ev.capacity() < w.n * m.kc_bps / 10_000, "capacity {} below k_c * n", ev.capacity());
    let lw = LivePending { w, value: wv, id, op: op(0x42), daa: p_daa };
    let seller = LiveToken { t: TokenState::p2pk(2_000_000_000, pubkey(5), m.ext), value: 20_000_000, id: m.token_cov_id, op: op(0x44), daa: p_daa };
    let funding = Funding { utxos: vec![KeyUtxo { outpoint: op(0x45), amount: 1_000_000 * 100_000_000, daa: p_daa, key: pubkey(5) }], change_key: pubkey(5) };
    let eu = EvidenceUse { order: ev, fill: None, seller: Some(seller), now: Some(p_daa + 100), allow_dead: false };
    let err = challenge(&[lw], &eu, &funding, &Opts::default()).unwrap_err().to_string();
    assert!(err.contains("fewer than the"), "the builder refuses the same order: {err}");
}

/// The review's note on two messages (`print_proposal`, `challenge_cmd`): a refuted part does not return "with a fresh
/// clock"; since commit 6a481d1 the restore keeps the pair's clock S, and since DESIGN 4.5e it returns unlisted. Neither
/// command can run offline without a funder, so the wording is checked in the source the binary is built from.
#[test]
fn holds_no_message_promises_a_fresh_clock_after_a_challenge() {
    let src = include_str!("../src/legacy/evcmds.rs");
    assert!(!src.contains("fresh clock"), "a message still promises a fresh clock after a challenge");
    assert_eq!(src.matches("clock S kept").count(), 2, "both messages say the clock S is kept");
}
