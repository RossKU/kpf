//! Review 3 (docs/audit/REVIEW-3.md), tooling: the library the `kpf` command line builds with.

use kpf_protocol::evidence::p7_default_x;
use kpf_protocol::fixtures::{long_order, pair, pubkey, short_order};
use kpf_protocol::payoff::{long_liquidatable, short_liquidatable};
use kpf_protocol::state::{LONG, SHORT};

/// R3-T1 (with R3-F3). `p7_default_x`, the price `kpf liquidate-transfer` uses when `--x` is not given, is the
/// highest x the entrant allows, and in the DESIGN 5 pair (TESTNET.md step 23: bl 9, bs 12, E 10, ask 9.4) the
/// binding bound is no-downgrade (bl + myRate). The entrant's new bound is then exactly the loser's bl, so the
/// evidence ask of the same transaction already makes the entrant liquidatable, and DESIGN 4.5b gives its whole
/// margin to the winner at the next liquidation. The tool builds this by default and prints no warning.
/// Who loses: the maker of the entrant KpfOpen, its margin q(n, myRate) (1,000 KAS for 1,000 tokens).
#[test]
fn exploit_default_p7_price_enters_the_order_already_liquidatable() {
    let p = pair(pubkey(21), pubkey(22));
    let quote = 940_000_000;
    assert!(long_liquidatable(&p, quote));
    let lo = long_order(pubkey(31));
    let x = p7_default_x(&p, LONG, quote, &lo).unwrap();
    assert_eq!(x, 1_000_000_000);
    let mut part = p.clone();
    part.bl = x - lo.my_rate;
    part.long_key = lo.owner;
    assert_eq!(part.bl, p.bl, "the entrant gets exactly the loser's bankruptcy price");
    assert!(long_liquidatable(&part, quote), "so the same ask liquidates it");
    // Mirror: a short loser sold to the fixture short order.
    let b = 1_150_000_000;
    assert!(short_liquidatable(&p, b));
    let so = short_order(pubkey(32));
    let x = p7_default_x(&p, SHORT, b, &so).unwrap();
    let mut part = p.clone();
    part.bs = x + so.my_rate;
    assert_eq!((x, part.bs), (1_000_000_000, p.bs), "the short entrant gets exactly the loser's bs");
    assert!(short_liquidatable(&part, b), "so the same bid liquidates it");
}
