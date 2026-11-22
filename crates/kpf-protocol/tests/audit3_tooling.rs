//! Review 3 (docs/audit/REVIEW-3.md), tooling: the library the `kpf` command line builds with.
//!
//! Premise changed by DESIGN 4.5d: R3-T1 (the default P7 price `p7_default_x`) is moot, liquidation by transfer is gone
//! and with it the entrant chosen in an evidence transaction. The tests here hold the helpers the two-step tools use to
//! the contracts' rules: the counter quote that refutes a claim, the cure bound, the forfeit bound.

use kpf_protocol::fixtures::{pair, pubkey};
use kpf_protocol::payoff::{long_liquidatable, q, short_liquidatable};
use kpf_protocol::pending::{claim_fee, cure_bound_of, forfeit_bound_of, pending_of};
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG, SHORT};

/// The refutation bound `contradiction_bound` (what `kpf watch` prints) is exactly where `contradicts` flips, for each
/// claim of the DESIGN 5 pair (g = 2 %): 9.6632 for a long's liquidation, 11.20 for a short's, 0.98 of a long's close
/// bid, 1.02 of a short's close ask.
#[test]
fn holds_refutation_bound_is_where_the_contract_rule_flips() {
    let p = pair(pubkey(21), pubkey(22));
    for (kind, side, quote, bound) in [
        (KIND_LIQ, LONG, 940_000_000, 966_315_790),
        (KIND_LIQ, SHORT, 1_150_000_000, 1_120_000_000),
        (KIND_CLOSE, LONG, 1_050_000_000, 1_029_000_000),
        (KIND_CLOSE, SHORT, 1_100_000_000, 1_122_000_000),
    ] {
        let w = pending_of(&p, p.size, kind, side, quote, pubkey(23));
        assert_eq!(w.contradiction_bound(), Some(bound), "kind {kind} side {side}");
        assert!(w.contradicts(bound));
        let one_short = if w.counter_is_ask() { bound + 1 } else { bound - 1 };
        assert!(!w.contradicts(one_short));
    }
}

/// The cure bound is the nearest bound at which no quote within the margin g of the claim's own quote liquidates the
/// side (premise changed by DESIGN 4.5e: before review 4 it was the nearest bound at which the claim's own quote no longer
/// did), and the forfeit bound is the nearest inward bound at which the narrowed range and the fee fit the part (unique,
/// one time wider does not fit).
#[test]
fn holds_cure_and_forfeit_bounds_match_the_contract_rules() {
    let p = pair(pubkey(21), pubkey(22));
    // Long at 9.40, g 2 %: the reach 9.212; the cure bound 8.7514 = 9.212 * 0.95.
    let reach = kpf_protocol::pending::cure_reach_of(&p.m, LONG, 940_000_000);
    let cb = cure_bound_of(&p.m, LONG, 940_000_000).unwrap();
    assert_eq!((reach, cb), (921_200_000, 875_140_000));
    let mut c = p.clone();
    c.bl = cb;
    assert!(long_liquidatable(&p, 940_000_000) && !long_liquidatable(&c, 940_000_000) && !long_liquidatable(&c, reach));
    assert!(long_liquidatable(&c, reach - 1), "a quote beyond g of the claim still liquidates the cured part");
    c.bl = cb + 1;
    assert!(long_liquidatable(&c, reach), "one time less margin and the quote at the reach liquidates again");
    // Short at 11.50: the reach 11.73; the cure bound 12.3165.
    let reach = kpf_protocol::pending::cure_reach_of(&p.m, SHORT, 1_150_000_000);
    let cb = cure_bound_of(&p.m, SHORT, 1_150_000_000).unwrap();
    assert_eq!((reach, cb), (1_173_000_000, 1_231_650_000));
    let mut c = p.clone();
    c.bs = cb;
    assert!(!short_liquidatable(&c, 1_150_000_000) && !short_liquidatable(&c, reach));
    assert!(short_liquidatable(&c, reach + 1));
    c.bs = cb - 1;
    assert!(short_liquidatable(&c, reach));
    for (side, quote) in [(LONG, 940_000_000), (SHORT, 1_150_000_000)] {
        let fee = claim_fee(&p, p.size, side, quote);
        let fb = forfeit_bound_of(&p, p.size, side, fee);
        let w = if side == LONG { p.bs - fb } else { fb - p.bl };
        let part = p.range_of(p.size);
        assert!(q(p.size, w, p.m.scale, 0) + fee <= part);
        assert!(q(p.size, w + 1, p.m.scale, 0) + fee > part);
    }
    // No cure for a short whose cure bound would exceed priceMax.
    assert_eq!(cure_bound_of(&p.m, SHORT, p.m.price_max), None);
}
