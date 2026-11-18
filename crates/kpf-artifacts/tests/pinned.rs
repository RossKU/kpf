//! A fresh compile with the vendored compiler reproduces the committed artifacts byte for byte.
//! If this fails after a change to `contracts/KpfPair.sil` or `contracts/KpfOpen.sil` (or the compiler pin), regenerate:
//! `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`, review the diff and commit the result.

#[test]
fn committed_artifacts_equal_a_fresh_compile() {
    let diff = kpf_artifacts::check();
    assert!(diff.is_empty(), "{}\nregenerate with: cargo run -p kpf-artifacts -- write", diff.join("\n"));
}

/// The evidence templates are the ones KOB itself pins (`vendor/kob/contracts/orders/KobCondAsk.ctor.json`, and the
/// `KOB_*` constants of `crates/kpf-tests/tests/kpf/kob.rs`).
#[test]
fn kob_evidence_templates_are_kobs_pinned_ones() {
    let k = kpf_protocol::artifacts::kob_consts();
    let ask: [u8; 32] = [7, 11, 179, 178, 66, 92, 200, 0, 192, 46, 46, 148, 101, 36, 26, 139, 148, 171, 220, 233, 62, 113, 47, 211, 186, 241, 148, 116, 182, 59, 136, 124];
    let bid: [u8; 32] = [223, 205, 120, 241, 92, 218, 220, 203, 125, 182, 248, 9, 76, 180, 36, 33, 164, 9, 100, 42, 139, 40, 4, 21, 24, 166, 189, 46, 197, 75, 213, 239];
    assert_eq!((k.ask_tpl, k.ask_pre, k.ask_suf), (ask, 1, 1407));
    assert_eq!((k.bid_tpl, k.bid_pre, k.bid_suf), (bid, 1, 1343));
}
