//! Review 6 (docs/audit/REVIEW-6.md), tooling: `oracle-market` and `oracle-check` driven offline through the `kpf`
//! binary. No node is contacted and nothing is submitted.

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

/// F4 (Medium). A market whose judge cannot be reached passes the bounds and `oracle-check` prints no warning: judge
/// stakes of at least 1,000,000 KAS each (the bound's maximum), stake that counts only after 2^32 - 1 DAA (13.6 years
/// at 10 DAA/s, so in a new market no stake can count for 13.6 years) and rounds of 13.6 years (any contest freezes the
/// part that long). On such a market every false claim is uncontestable in practice, yet the wallet tells the user "A
/// false price stands unless someone contests it within 6.00 h" and raises nothing. Who loses: an honest user who opens
/// or rests an order there, trusting the check: up to its whole margin to the first false claim.
#[test]
fn exploit_oracle_check_raises_no_warning_on_an_unreachable_judge() {
    let d = Dir(std::env::temp_dir().join(format!("kpf-audit6-cli-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&d.0);
    std::fs::create_dir_all(&d.0).unwrap();
    let feed = d.0.join("feed.txt").display().to_string();
    std::fs::write(&feed, FEED).unwrap();
    let market = d.0.join("market.json").display().to_string();
    let (ok, out) = kpf(&[
        "oracle-market",
        "--feed",
        &feed,
        "--out",
        &market,
        "--min-stake",
        "1000000",
        "--stake-age-daa",
        "4294967295",
        "--round-daa",
        "4294967295",
        "--stake-unbond-daa",
        "4294967295",
    ]);
    assert!(ok, "the hostile judge passes the contracts' bounds: {out}");
    assert!(!out.contains("WARNING"), "oracle-market raises no warning: {out}");
    let (ok, out) = kpf(&["oracle-check", "--market", &market, "--feed", &feed]);
    assert!(ok, "{out}");
    assert!(!out.contains("WARNING"), "oracle-check raises no warning: {out}");
    assert!(out.contains("A false price stands unless someone contests it within 6.00 h"), "{out}");
    println!("{out}");
}
