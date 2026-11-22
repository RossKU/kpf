//! Review C8 (docs/audit/REVIEW-C8.md): cross margin on the library's model. What `c-account` and `c-cross-run`
//! compute (the liquidation line; the free margin) against the model's own rules, and what the lapse pays a side that
//! was trimmed past its entry.
//!
//! `c8_bounded_` an accepted behaviour, measured; `c8_holds_` what was checked and holds.

use kpf_protocol::cmarket::samples;
use kpf_protocol::cmarket::*;
use kpf_protocol::state::{LONG, SHORT};

const KAS1: i64 = 100_000_000;

/// `ccmds::liq_line`, as `c-account`, `c-cross-run` and `c-trim` print it.
fn liq_line(p: &CPair, sd: i64) -> i64 {
    let (b, g, m) = (10_000i128, p.m.p.g_bps as i128, p.m.p.mmr_bps as i128);
    if sd == LONG {
        (p.bl as i128 * b * b / ((b - m) * (b + g))) as i64
    } else {
        (p.bs as i128 * b * b / ((b + m) * (b - g))) as i64
    }
}

/// The commands' liquidation line against the model's rule (`long_liquidatable`, `short_liquidatable`), over ten
/// thousand bounds: the last price that liquidates a long and the first that liquidates a short, found by search.
#[test]
fn c8_bounded_the_liquidation_line_the_commands_print_is_the_models_within_two_sompi() {
    let m = samples::market();
    let base = samples::pair(&m);
    let (mut worst_long, mut worst_short, mut exact) = (0i64, 0i64, 0);
    let mut x = 0x9e3779b97f4a7c15u64;
    for _ in 0..10_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let bl = 1 + (x % (40 * KAS1 as u64)) as i64;
        let bs = bl + bl / 2 + 1 + ((x >> 32) % (20 * KAS1 as u64)) as i64;
        let p = CPair { bl, bs, ..base.clone() };
        if bs > p.m.p.price_max || !p.range_ok() {
            continue;
        }
        // The long: liquidated at every price up to T, not above.
        let l = liq_line(&p, LONG);
        let mut t = l + 4;
        while t >= 0 && !p.long_liquidatable(t) {
            t -= 1;
        }
        assert!(t < 0 || !p.long_liquidatable(t + 1));
        worst_long = worst_long.max((t - l).abs());
        // The short: liquidated at every price from T, not below.
        let s = liq_line(&p, SHORT);
        let mut u = (s - 4).max(0);
        while !p.short_liquidatable(u) {
            u += 1;
        }
        worst_short = worst_short.max((u - s).abs());
        exact += (t == l && u == s) as u32;
    }
    println!("C8: the printed line against the model's last liquidating price: at most {worst_long} sompi off for a long, {worst_short} for a short ({exact} of the sample exact)");
    assert!(worst_long <= 2 && worst_short <= 2, "{worst_long} {worst_short}");
}

/// The free margin `c-account` shows is the model's limit, and the model's limit is the contract's rule: at the
/// nearest bound the side may close at the same price, a sompi nearer it may not, and the range rule is kept.
#[test]
fn c8_holds_the_free_margin_is_the_trim_the_model_accepts_and_no_more() {
    let m = samples::market();
    let p = samples::pair(&m);
    let mut seen = 0;
    for sd in [LONG, SHORT] {
        for step in 0..400 {
            let px = p.bl - p.bl / 10 + step * ((p.bs - p.bl) / 300 + 1);
            let Some(nb) = p.trim_bound_of(sd, px) else { continue };
            if !p.trim_ok(sd, px, nb) {
                // Nothing to take: then no bound at all is accepted at this price.
                let near = if sd == LONG { p.bl + 1 } else { p.bs - 1 };
                assert!(!p.trim_ok(sd, px, near) || p.trim_take(sd, near) < kpf_protocol::state::PLAIN_DUST);
                continue;
            }
            seen += 1;
            let t = p.trim_state(sd, nb);
            assert!(t.range_ok() && t.close_ok(sd, px.min(p.m.p.price_max)));
            let nearer = if sd == LONG { nb + 1 } else { nb - 1 };
            assert!(!p.trim_ok(sd, px, nearer), "{sd} at {px}: a sompi nearer than {nb}");
            assert!(p.trim_take(sd, nb) > 0 && p.trim_take(sd, nb) < p.range_of(p.size));
            // The amount asked for gives a bound that frees at least it, and no further in than the limit.
            let amount = p.trim_take(sd, nb) / 2;
            if let Some(b2) = p.trim_bound_for(sd, amount) {
                assert!(p.trim_take(sd, b2) >= amount, "{sd} at {px}: {} for {amount}", p.trim_take(sd, b2));
                assert!(if sd == LONG { b2 <= nb } else { b2 >= nb });
            }
        }
    }
    assert!(seen > 50, "{seen}");
}

/// C8-F1 on the model. A pair nobody keeps alive pays each side its worth at the entry clamped to the range. A long
/// trimmed past the entry is paid nothing; margin it adds afterwards, as long as its bound stays above the entry, is
/// the short's at the lapse.
#[test]
fn c8_bounded_a_side_trimmed_past_its_entry_gets_nothing_at_the_lapse_and_neither_does_what_it_adds() {
    let m = samples::market();
    let p = samples::pair(&m);
    let value = |q: &CPair| (q.range_of(q.size) + q.m.p.carrier) as u64;
    let (long0, short0) = p.lapse_out(value(&p));
    assert!(long0 > 0 && short0 > 0, "untrimmed, the lapse pays both: {long0} {short0}");
    // Trimmed to a tenth of the way past the entry (a price well above it allows that).
    let nb = p.entry + (p.bs - p.entry) / 10;
    let t = p.trim_state(LONG, nb);
    assert!(t.range_ok());
    let take = p.trim_take(LONG, nb);
    let (long1, short1) = t.lapse_out(value(&t));
    assert_eq!((long1, short1), (0, value(&t) as i64));
    // The long had its worth at the entry out already and more: `take` against `long0`.
    assert!(take > long0);
    // It adds margin back, half of the way to the entry: all of it is the short's at the lapse.
    let back = CPair { bl: nb - (nb - p.entry) / 2, ..t.clone() };
    let added = back.range_of(back.size) - t.range_of(t.size);
    let (long2, short2) = back.lapse_out(value(&back));
    assert_eq!((long2, short2 - short1), (0, added));
    println!("C8-F1 (model): untrimmed the lapse pays the long {} KAS; trimmed past the entry (it took {} KAS) it pays 0; {} KAS of margin added afterwards are the short's at the lapse", long0 as f64 / KAS1 as f64, take as f64 / KAS1 as f64, added as f64 / KAS1 as f64);
}
