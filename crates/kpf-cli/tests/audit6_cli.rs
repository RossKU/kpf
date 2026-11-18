//! Review 6 (docs/audit/REVIEW-6.md), tooling, after the fixes: `oracle-market` and `oracle-check` driven offline
//! through the `kpf` binary. No node is contacted and nothing is submitted. (`oracle-watch`'s reading of the DAA is
//! checked in `oracle_cli`.)

use std::path::PathBuf;
use std::process::Command;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn kpf(args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_kpf")).args(args).output().expect("run kpf");
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

const FEED: &str = "kpf feed definition v1\nquantity: example slow underlying, published once per hour\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\ndisagreement: a claim is true if within 0.25 % of the median\n";

fn dir(tag: &str) -> (Dir, String) {
    let d = Dir(std::env::temp_dir().join(format!("kpf-audit6-cli-{tag}-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&d.0);
    std::fs::create_dir_all(&d.0).unwrap();
    let feed = d.0.join("feed.txt").display().to_string();
    std::fs::write(&feed, FEED).unwrap();
    (d, feed)
}

/// R6-F4 (the review's `exploit_oracle_check_raises_no_warning_on_an_unreachable_judge`): the judge the review built
/// (1,000,000 KAS stakes, stake age, rounds and unbonding of 2^32 - 1 DAA) no longer passes the contracts' bounds, and
/// one beyond each new bound is refused by `oracle-market`, as by every contract of the family.
#[test]
fn holds_oracle_market_refuses_an_unreachable_judge() {
    let (d, feed) = dir("refuse");
    let market = d.0.join("market.json").display().to_string();
    let base = ["oracle-market", "--feed", &feed, "--out", &market];
    let (ok, out) = kpf(&[&base[..], &["--min-stake", "1000000", "--stake-age-daa", "4294967295", "--round-daa", "4294967295", "--stake-unbond-daa", "4294967295"][..]].concat());
    assert!(!ok && out.contains("the parameters fail the contracts' bounds"), "{out}");
    for (flag, beyond) in [("--min-stake", "10000.00000001"), ("--stake-age-daa", "25920001"), ("--round-daa", "2592001"), ("--stake-unbond-daa", "77760001")] {
        let (ok, out) = kpf(&[&base[..], &[flag, beyond][..]].concat());
        assert!(!ok && out.contains("the parameters fail the contracts' bounds"), "{flag} {beyond}: {out}");
    }
}

/// R6-F4: at the bounds the market is accepted, but `oracle-market` and `oracle-check` state every judge parameter in
/// plain words and warn that a contest within the window is not realistically possible.
#[test]
fn holds_oracle_check_states_every_judge_parameter_and_warns_on_a_hard_judge() {
    let (d, feed) = dir("bounds");
    let market = d.0.join("market.json").display().to_string();
    let (ok, out) = kpf(&["oracle-market", "--feed", &feed, "--out", &market, "--min-stake", "10000", "--stake-age-daa", "25920000", "--round-daa", "2592000", "--stake-unbond-daa", "77760000"]);
    assert!(ok, "the bounds themselves are admitted: {out}");
    assert!(out.contains("WARNING: a contest within the window is not realistically possible here"), "{out}");
    let (ok, out) = kpf(&["oracle-check", "--market", &market, "--feed", &feed]);
    assert!(ok, "{out}");
    for w in [
        "each stake is at least 10000 KAS",
        "a round of 72.00 h",
        "bonded 720.00 h before the claim",
        "a stake takes 2160.00 h to leave",
        "10 % of the losers' stake is burned",
        "WARNING: a judge stake must be at least 10000 KAS",
        "WARNING: judge stake counts only 30.0 days after it is bonded",
        "WARNING: a dispute round lasts 3.0 days",
        "WARNING: judge stake takes 90.0 days to leave",
        "WARNING: a contest within the window is not realistically possible here",
    ] {
        assert!(out.contains(w), "missing `{w}`: {out}");
    }
    println!("{out}");
}

/// Review 6, notes 1 and 2: the words state that a new market has no judge for its first M_age and that a late
/// contest delays a claim for a round at the cost of fees; the testnet defaults raise no contest warning.
#[test]
fn holds_oracle_check_states_the_new_market_and_the_late_contest() {
    let (d, feed) = dir("notes");
    let market = d.0.join("market.json").display().to_string();
    let (ok, out) = kpf(&["oracle-market", "--feed", &feed, "--out", &market]);
    assert!(ok, "{out}");
    let (ok, out) = kpf(&["oracle-check", "--market", &market, "--feed", &feed]);
    assert!(ok, "{out}");
    assert!(out.contains("A new market has no judge for its first 168.00 h"), "{out}");
    assert!(out.contains("A contest that lands after the window is ignored for the verdict"), "{out}");
    assert!(out.contains("amending the pair does not postpone it"), "{out}");
    assert!(!out.contains("not realistically possible"), "{out}");
}
