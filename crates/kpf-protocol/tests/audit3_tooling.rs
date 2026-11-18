//! Review 3 (docs/audit/REVIEW-3.md), tooling: the library the `kpf` command line builds with. After the fixes of
//! DESIGN 4.5c every test here asserts the refusal (or the safe default) the review asked for.

use kpf_protocol::evidence::{entrant_health_bound, p7_default_x};
use kpf_protocol::fixtures::{long_order, pair, pubkey, short_order};
use kpf_protocol::payoff::{long_liquidatable, short_liquidatable};
use kpf_protocol::state::{healthy_at, LONG, SHORT};

/// R3-T1 (with R3-F3), fixed. `p7_default_x`, the price `kpf liquidate-transfer` uses when `--x` is not given, used to
/// be the highest x the entrant allows (10 KAS in the DESIGN 5 pair, TESTNET.md step 23: bl 9, bs 12, E 10, ask 9.4),
/// which gave the entrant exactly the loser's bl, so the evidence ask of the same transaction already made it
/// liquidatable. It now also bounds x by the entrant's health at the quote (DESIGN 4.5c): x <= floor(9.4 * 0.95) + 1 =
/// 9.93, and the entrant it builds is not liquidatable at the quote. Mirrored for a short loser.
#[test]
fn holds_default_p7_price_keeps_the_entrant_healthy_at_the_quote() {
    let p = pair(pubkey(21), pubkey(22));
    let quote = 940_000_000;
    assert!(long_liquidatable(&p, quote));
    let lo = long_order(pubkey(31));
    let x = p7_default_x(&p, LONG, quote, &lo).unwrap();
    assert_eq!(x, 993_000_000);
    assert_eq!(x, entrant_health_bound(&p.m, LONG, quote, lo.my_rate));
    let mut part = p.clone();
    part.bl = x - lo.my_rate;
    part.long_key = lo.owner;
    assert!(part.bl < p.bl, "the entrant is deeper than the loser");
    assert!(!long_liquidatable(&part, quote), "and the same ask does not liquidate it");
    assert!(healthy_at(&p.m, LONG, part.bl, quote));
    // One sompi more and the entrant is liquidatable at the quote.
    assert!(!healthy_at(&p.m, LONG, x + 1 - lo.my_rate, quote));
    // Mirror: a short loser sold to the fixture short order.
    let b = 1_150_000_000;
    assert!(short_liquidatable(&p, b));
    let so = short_order(pubkey(32));
    let x = p7_default_x(&p, SHORT, b, &so).unwrap();
    let mut part = p.clone();
    part.bs = x + so.my_rate;
    assert_eq!(x, 1_007_500_000);
    assert!(part.bs > p.bs && !short_liquidatable(&part, b), "the short entrant is not liquidatable at the bid");
    assert!(!healthy_at(&p.m, SHORT, x - 1 + so.my_rate, b));
}

/// R3-T1: an entrant whose margin rate cannot be healthy at the quote at any admissible x is refused with a plain
/// reason, instead of being entered liquidatable.
#[test]
fn holds_default_p7_price_refuses_an_entrant_too_thin_for_the_quote() {
    let p = pair(pubkey(21), pubkey(22));
    let mut lo = long_order(pubkey(31));
    lo.my_rate = 40_000_000; // 0.4 KAS: below 5 % of the 9.4 ask
    let e = p7_default_x(&p, LONG, 940_000_000, &lo).unwrap_err().to_string();
    assert!(e.contains("liquidatable at the evidence ask") && e.contains("forfeit"), "{e}");
}
