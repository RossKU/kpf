//! Research, numbers only (docs/ARCHITECTURE-CLEARING.md 10): what a row of an account's trace would count for a
//! spot token, and why an option is not counted by a number of its own. No script and no object here.
//!
//! A row of a trace is a running sum over the legs at the prices stated. It sums two numbers: side A's gain (which a
//! take moves between the margins, moving the entries to the prices) and the legs' worth (which the maintenance is a
//! rate of).
//! - An option is counted in those two (built: `tests/thin_option.rs`): its row adds to the gain what it is worth
//!   if exercised at the price less what it was worth at its entry, and its entry moves with a take as a futures
//!   leg's does. The first form tried here counted it apart (a third number that a take does not move, the strike
//!   where the entry stands). The first test says why that was dropped.
//! - A spot token needs a number that is neither: `spot`, what A's tokens are worth at the prices stated less the
//!   discount. It is nobody's debt: it counts for A alone.
//!     A stands   where  a_margin + gain + spot >= floor
//!     B stands   where  b_margin - gain        >= floor
//!   The row built for options (112 bytes) has the place: what side A keeps whole beside its maintenance, which a
//!   token's worth is taken from (below zero where A holds tokens and no option).

const BPS: i64 = 10_000;
const MMR: i64 = 500;
const SCALE: i64 = 100_000_000;
const KAS1: i64 = 100_000_000;
const P10: i64 = 10 * KAS1;

fn worth(size: i64, p: i64) -> i64 {
    (size / SCALE) * p + (size % SCALE) * (p / SCALE) + ((size % SCALE) * (p % SCALE)) / SCALE
}

fn rate_up(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}

/// A's futures leg lost 20 and the call A holds is worth 30 exercised; A has 5 of margin. Its two legs made it 10.
#[test]
fn an_option_counted_apart_from_the_gain_forgives_a_loss_the_option_would_have_paid() {
    let (a, b) = (5 * KAS1, 100 * KAS1);
    let size = 10 * SCALE;
    let fut = worth(size, 8 * KAS1) - worth(size, P10);
    let call = (worth(size, 13 * KAS1) - worth(size, P10)).max(0);
    assert_eq!((fut, call), (-20 * KAS1, 30 * KAS1));

    // Counted apart: the futures leg is settled against A's margin alone, as far as the margin goes; what is beyond
    // it is the other party's loss (as with any leg that fell through its margin). The option's worth still stands
    // against the other party, whole.
    let paid = fut.clamp(-a, b);
    let (a1, b1) = (a + paid, b - paid);
    let forgiven = paid - fut;
    assert_eq!((a1, b1, forgiven), (0, 105 * KAS1, 15 * KAS1));
    let paid = call.clamp(-a1, b1);
    let (a2, b2) = (a1 + paid, b1 - paid);
    assert_eq!((a2 - a, b2 - b), (25 * KAS1, -25 * KAS1), "A is 25 up on legs that made it 10: the other party paid the 15 forgiven");

    // Counted in one sum (what is built): the loss is paid by the gain.
    let gain = fut + call;
    assert_eq!((a + gain, b - gain), (15 * KAS1, 90 * KAS1));
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Row {
    gain: i64,
    worth: i64,
    spot: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Leg {
    /// A is long (+1) or short (-1) `size` from `entry`.
    Fut { dir: i64, size: i64, entry: i64 },
    /// A's tokens, counted at `discount` bps less.
    Spot { size: i64, discount: i64 },
}

fn row_after(b: Row, l: &Leg, p: i64) -> Row {
    match *l {
        Leg::Fut { dir, size, entry } => Row { gain: b.gain + dir * (worth(size, p) - worth(size, entry)), worth: b.worth + worth(size, p), ..b },
        Leg::Spot { size, discount } => Row { spot: b.spot + worth(size, p) - rate_up(worth(size, p), discount), ..b },
    }
}

fn trace(legs: &[Leg], p: &[i64]) -> Row {
    legs.iter().zip(p).fold(Row::default(), |r, (l, p)| row_after(r, l, *p))
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

/// A token counts at its discount, for side A alone. Side B's standing does not read it. And where a liquidator
/// takes the tokens for their worth less a fee at a price that fell by less than the discount less the fee, the
/// account has at least what the tokens were counted at.
#[test]
fn a_token_counts_at_its_discount_for_its_holder_alone() {
    let mut r = Rng(0x7777_1111_3333_5555);
    for case in 0..3_000 {
        let discount = r.range(500, 3_000);
        let fee = r.range(0, discount / 2);
        let size = r.range(1, 80) * SCALE + r.range(0, SCALE - 1);
        let p = P10 / 10 * r.range(5, 15) + r.range(0, 999);
        let fut = Leg::Fut { dir: if r.range(0, 1) == 0 { 1 } else { -1 }, size: r.range(1, 50) * SCALE, entry: P10 };
        let (a, b) = (r.range(0, 50) * KAS1, r.range(0, 400) * KAS1);
        let prices = [P10 / 10 * r.range(8, 12), p];
        let (rw, ro) = (trace(&[fut, Leg::Spot { size, discount }], &prices), trace(&[fut], &prices[..1]));
        assert_eq!((rw.gain, rw.worth), (ro.gain, ro.worth), "case {case}: a token is no leg of the two: no gain, no worth to maintain");
        assert_eq!(b - rw.gain, b - ro.gain, "case {case}: side B's standing reads no token");
        let counted = rw.spot;
        // (The discount is rounded up: by less than one basis point's worth of it.)
        assert!(counted <= worth(size, p) - worth(size, p) / BPS * discount && counted >= worth(size, p) - (worth(size, p) / BPS + 1) * discount, "case {case}: counted at the discount");
        assert_eq!(a + rw.gain + rw.spot, a + ro.gain + counted);
        // The floor is the maintenance of the futures alone.
        assert_eq!(rate_up(rw.worth, MMR), rate_up(ro.worth, MMR));
        // The price falls by less than the discount less the fee; a liquidator takes the tokens for their worth
        // there less the fee, paid into A's margin.
        let fall = r.range(0, discount - fee);
        let q = p - p / BPS * fall;
        let w = worth(size, q);
        let paid = w - rate_up(w, fee);
        assert!(paid + 2 >= counted - worth(size, p) / BPS, "case {case}: sold at {q} for {paid}, counted at {counted} (discount {discount}, fee {fee}, fall {fall})");
    }
}
