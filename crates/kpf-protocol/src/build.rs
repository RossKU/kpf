//! Transaction builders for every `KpfOpen` and `KpfPair` path that needs no price evidence (DESIGN 4.5).
//!
//! Each builder returns a [`Built`]: the unsigned transaction and the per-input signing data. The shapes are those the
//! engine tests exercise (`crates/kpf-tests`): every party output holds the least value its covenant accepts, so the
//! rest (tips, spare carriers, rounding slack) falls to the funder's change, and the funder pays the network fee.
//! Indices are the ones the tests use; the covenants accept any layout with the same rules (DESIGN 4.4).
//!
//! | builder | path |
//! |---|---|
//! | [`create_order`] | genesis of a `KpfOpen` from plain-key inputs |
//! | [`cancel_order`], [`refund_order`] | `KpfOpen.cancel`, `KpfOpen.refund` |
//! | [`open_orders`], [`open_taker`] | T1 open: order x order, order x signing taker |
//! | [`amend`] | P1 list / unlist |
//! | [`add_margin`] | P2 |
//! | [`transfer`] | P3 (order taker or signing taker) |
//! | [`close_vs_close`] | P4 close-versus-close |
//! | [`mutual_close`] | P4' both listings cross |
//! | [`coop_close`] | P9 cooperative close |
//! | [`idle_settle`] | P8 |
//!
//! The evidence paths (P5 unilateral close, P6 liquidation, P7 liquidation by transfer) are built in `crate::evidence`.

use kaspa_consensus_core::Hash;

use crate::artifacts::pair_template;
use crate::error::{invalid, Error, Result};
use crate::payoff::*;
use crate::state::*;
use crate::tx::*;

/// Fee options shared by every builder.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// Fee rate, sompi per gram (the relay floor is 100).
    pub fee_rate: u64,
    pub fee_mode: FeeMode,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { fee_rate: MIN_FEE_RATE, fee_mode: FeeMode::Relay }
    }
}

/// The funder: plain P2PK UTXOs that pay what the contracts do not and the network fee; the change goes to
/// `change_key`.
#[derive(Clone, Debug)]
pub struct Funding {
    pub utxos: Vec<KeyUtxo>,
    pub change_key: [u8; 32],
}

impl Draft {
    pub(crate) fn fund_inputs(&mut self, f: &Funding) -> Result<usize> {
        if f.utxos.is_empty() {
            return invalid("this transaction needs at least one funding UTXO (plain P2PK) to pay the fee");
        }
        let first = self.ins.len();
        for u in &f.utxos {
            self.add_in(u.outpoint, u.entry(), Plan::P2pk { key: u.key }, "funding");
        }
        Ok(first)
    }
    pub(crate) fn add_change(&mut self, key: &[u8; 32]) -> usize {
        let c = self.add_plain(0, key, "change");
        self.change = Some(c);
        c
    }
}

fn req(ok: bool, msg: impl FnOnce() -> String) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(msg()))
    }
}

// ---------------------------------------------------------------- orders

/// Creates a `KpfOpen` order: a genesis output (its covenant id is the hash of the first funding outpoint and the
/// output) holding the escrow `q(sizeLeft, myRate + tip) + 2C`. Funded by plain keys; nobody signs for the order.
pub fn create_order(o: &Open, funding: &Funding, opts: &Opts) -> Result<Built> {
    o.validate()?;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    let side = if o.side == LONG { "long" } else { "short" };
    let oi = d.add_open_out(o, o.escrow() as u64, None, format!("new {side} order, escrow"));
    let first = d.fund_inputs(funding)?;
    d.add_change(&funding.change_key);
    d.genesis.push((oi, first));
    d.finish()
}

/// `KpfOpen.cancel`: the owner (signature) takes the whole escrow, less the network fee, to `dest`.
pub fn cancel_order(lo: &LiveOpen, dest: &[u8; 32], opts: &Opts) -> Result<Built> {
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_open_in(lo, "cancel", vec![Arg::Sig(lo.o.owner)], "order (cancel)");
    let c = d.add_plain(0, dest, "escrow returned");
    d.fee_from(c);
    d.finish()
}

/// `KpfOpen.refund`: anyone, once the order is expired or 90 days idle; the whole escrow goes to the owner and the
/// funder pays the fee.
pub fn refund_order(lo: &LiveOpen, funding: &Funding, opts: &Opts) -> Result<Built> {
    let due = (lo.o.expiry_daa as u64).min(lo.daa + MAX_IDLE);
    req(due < LOCK_TIME_THRESHOLD as u64, || "refund time is above the lock-time threshold".into())?;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.lock = due;
    d.add_open_in(lo, "refund", vec![], "order (refund)");
    d.add_plain(lo.value, &lo.o.owner, "escrow returned to the owner");
    d.fund_inputs(funding)?;
    d.add_change(&funding.change_key);
    d.finish()
}

fn pair_state_template(m: &Market, n: i64, bl: i64, bs: i64, x: i64, lk: [u8; 32], sk: [u8; 32], tl: Hash, ts: Hash) -> Pair {
    Pair {
        m: m.clone(),
        size: n,
        bl,
        bs,
        entry: x,
        long_key: lk,
        short_key: sk,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: tl,
        tag_s: ts,
    }
}

/// The checks of `KpfOpen.fill` and `take` on the amount.
fn check_amount(o: &Open, n: i64) -> Result<()> {
    req(n > 0 && n <= o.size_left, || format!("n = {n} must be in 1..={} (sizeLeft)", o.size_left))?;
    req(n >= o.m.min_size, || format!("n = {n} is below minSize {}", o.m.min_size))?;
    req(n >= o.min_fill || n == o.size_left, || format!("n = {n} is below the order's minFill {} and does not take all that is left", o.min_fill))?;
    let left = o.size_left - n;
    req(left == 0 || left >= o.m.min_size, || format!("a fill of {n} would leave {left}, below minSize {}", o.m.min_size))
}

/// The order's continuation or refund output after a fill of n, at the next output index. Returns the index.
pub(crate) fn order_output(d: &mut Draft, lo: &LiveOpen, input: usize, n: i64) -> usize {
    let (rest, cont) = order_rest(&lo.o, lo.value, n);
    match cont {
        Some(c) => d.add_open_out(&c, rest, Some((input as u16, lo.id)), "order continuation"),
        None => d.add_plain(rest, &lo.o.owner, "order filled out: escrow returned to the owner"),
    }
}

/// T1 open, order x order: n base units at x between a long and a short `KpfOpen`. The funder authors the new
/// pair's genesis and keeps the tips and the spare carrier.
pub fn open_orders(lo: &LiveOpen, so: &LiveOpen, n: i64, x: i64, funding: &Funding, opts: &Opts) -> Result<Built> {
    req(lo.o.side == LONG && so.o.side == SHORT, || "open_orders needs a long order and a short order".into())?;
    req(lo.o.m == so.o.m, || "the two orders are of different markets".into())?;
    check_amount(&lo.o, n)?;
    check_amount(&so.o, n)?;
    req(x <= lo.o.limit, || format!("x = {x} is above the long order's limit {}", lo.o.limit))?;
    req(x >= so.o.limit, || format!("x = {x} is below the short order's limit {}", so.o.limit))?;
    let (bl, bs) = (x - lo.o.my_rate, x + so.o.my_rate);
    req(bl >= 0 && bs <= lo.o.m.price_max, || format!("range [{bl}, {bs}] is outside 0..=priceMax"))?;
    req(bs - x >= lo.o.min_counter_rate, || format!("the short's margin rate {} is below the long's minCounterRate {}", bs - x, lo.o.min_counter_rate))?;
    req(x - bl >= so.o.min_counter_rate, || format!("the long's margin rate {} is below the short's minCounterRate {}", x - bl, so.o.min_counter_rate))?;
    let p = pair_state_template(&lo.o.m, n, bl, bs, x, lo.o.owner, so.o.owner, lo.id, so.id);
    p.validate()?;
    let t = pair_template();
    let fa = |ck: &[u8; 32], ct: Hash| vec![iv(2), iv(n), iv(x), iv(bl), iv(bs), bv(ck), bv(&ct.as_bytes()), bv(&t.prefix), bv(&t.suffix)];
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_open_in(lo, "fill", fa(&so.o.owner, so.id), "long order (fill)");
    d.add_open_in(so, "fill", fa(&lo.o.owner, lo.id), "short order (fill)");
    let first = d.fund_inputs(funding)?;
    order_output(&mut d, lo, 0, n);
    order_output(&mut d, so, 1, n);
    let pi = d.add_pair_out(&p, p.floor() as u64, None, "new pair");
    d.genesis.push((pi, first));
    d.add_change(&funding.change_key);
    d.finish()
}

/// T1 open, order x signing taker: n base units at x between a `KpfOpen` and a taker that funds the other side from its
/// own plain UTXOs (the `funding`, signed by the taker's key). `taker_rate` is the taker's margin per whole token; the
/// new pair names `taker_key` as the owner of the opposite side.
pub fn open_taker(order: &LiveOpen, n: i64, x: i64, taker_rate: i64, taker_key: &[u8; 32], funding: &Funding, opts: &Opts) -> Result<Built> {
    let o = &order.o;
    check_amount(o, n)?;
    req(taker_rate > 0, || "the taker's margin rate must be > 0".into())?;
    req(taker_rate >= o.min_counter_rate, || format!("the taker's rate {taker_rate} is below the order's minCounterRate {}", o.min_counter_rate))?;
    let (bl, bs, lk, sk, tl, ts) = if o.side == LONG {
        req(x <= o.limit, || format!("x = {x} is above the long order's limit {}", o.limit))?;
        (x - o.my_rate, x + taker_rate, o.owner, *taker_key, order.id, ZERO)
    } else {
        req(x >= o.limit, || format!("x = {x} is below the short order's limit {}", o.limit))?;
        (x - taker_rate, x + o.my_rate, *taker_key, o.owner, ZERO, order.id)
    };
    req(bl >= 0 && bs <= o.m.price_max, || format!("range [{bl}, {bs}] is outside 0..=priceMax"))?;
    let p = pair_state_template(&o.m, n, bl, bs, x, lk, sk, tl, ts);
    p.validate()?;
    let t = pair_template();
    let args = vec![iv(1), iv(n), iv(x), iv(bl), iv(bs), bv(taker_key), bv(&ZERO.as_bytes()), bv(&t.prefix), bv(&t.suffix)];
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_open_in(order, "fill", args, "order (fill)");
    let first = d.fund_inputs(funding)?;
    order_output(&mut d, order, 0, n);
    let pi = d.add_pair_out(&p, p.floor() as u64, None, "new pair");
    d.genesis.push((pi, first));
    d.add_change(&funding.change_key);
    d.finish()
}

// ---------------------------------------------------------------- P1, P2

fn owner_of(p: &Pair, side: i64) -> Result<[u8; 32]> {
    match side {
        LONG => Ok(p.long_key),
        SHORT => Ok(p.short_key),
        _ => invalid("side must be 1 (long) or 2 (short)"),
    }
}

/// P1 amend: the owner of `side` sets its own listing (ask or bid, smallest part, tip per whole token; ask 0 unlists).
pub fn amend(l: &LivePair, side: i64, window: (i64, i64, i64), funding: &Funding, opts: &Opts) -> Result<Built> {
    let owner = owner_of(&l.p, side)?;
    req(window.0 >= 0 && window.0 <= l.p.m.price_max && window.1 >= 0 && window.2 >= 0, || "listing values out of range".into())?;
    let mut n = l.p.cleared();
    if side == LONG {
        (n.long_ask, n.long_min, n.long_tip) = window;
    } else {
        (n.short_bid, n.short_min, n.short_tip) = window;
    }
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_pair_in(l, "amend", vec![Arg::Sig(owner), iv(side), iv(window.0), iv(window.1), iv(window.2)], "pair (amend)");
    d.fund_inputs(funding)?;
    d.add_pair_out(&n, l.value, Some((0, l.id)), "pair continuation");
    d.add_change(&funding.change_key);
    d.finish()
}

/// P2 add margin: the long lowers `bl`, the short raises `bs`, by `delta`; the continuation's value grows by
/// `ceil(q(s, delta))`, paid by the funder.
pub fn add_margin(l: &LivePair, side: i64, delta: i64, funding: &Funding, opts: &Opts) -> Result<Built> {
    let owner = owner_of(&l.p, side)?;
    req(delta > 0, || "delta must be > 0".into())?;
    let mut n = l.p.cleared();
    if side == LONG {
        n.bl -= delta;
    } else {
        n.bs += delta;
    }
    req(n.bl >= 0 && n.bs <= n.m.price_max, || format!("new range [{}, {}] is outside 0..=priceMax", n.bl, n.bs))?;
    let value = (l.value as i64 + margin_cost(&l.p, delta)).max(n.floor()) as u64;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_pair_in(l, "addMargin", vec![Arg::Sig(owner), iv(side), iv(delta)], "pair (add margin)");
    d.fund_inputs(funding)?;
    d.add_pair_out(&n, value, Some((0, l.id)), "pair continuation");
    d.add_change(&funding.change_key);
    d.finish()
}

// ---------------------------------------------------------------- P3 transfer

/// Who takes the leaver's place in a transfer.
#[derive(Clone, Debug)]
pub enum Entrant {
    /// A `KpfOpen` of the leaver's side, taking with its own margin rate (`take`). The funding is the executor's.
    Open(LiveOpen),
    /// A plain-key taker that funds the part from its own UTXOs (the `funding`): its key and margin rate.
    Direct { key: [u8; 32], rate: i64 },
}

/// The arguments of `KpfOpen.take` for the part `part`, taken by an order of `side` for n at x.
pub(crate) fn take_args(part: &Pair, side: i64, part_out: i64, n: i64, x: i64) -> Vec<Arg> {
    let (bound, ck, w) = if side == LONG { (part.bs, part.short_key, part.short_window()) } else { (part.bl, part.long_key, part.long_window()) };
    let t = pair_template();
    vec![iv(part_out), iv(n), iv(x), iv(part.entry), iv(bound), bv(&ck), iv(w.0), iv(w.1), iv(w.2), bv(&t.prefix), bv(&t.suffix)]
}

fn transfer_spec(l: &LivePair, side: i64, n: i64, x: i64, ent: &Entrant, k: i64) -> Result<TransferSpec> {
    let (ek, et, rate) = match ent {
        Entrant::Open(lo) => {
            req(lo.o.side == side, || "the entrant order is of the other side".into())?;
            req(lo.o.m == l.p.m, || "the entrant order is of another market".into())?;
            (lo.o.owner, lo.id, lo.o.my_rate)
        }
        Entrant::Direct { key, rate } => (*key, ZERO, *rate),
    };
    req(rate > 0, || "the entrant's margin rate must be > 0".into())?;
    let nb = if side == LONG { x - rate } else { x + rate };
    Ok(TransferSpec { side, n, x, nb, ek, et, k })
}

/// P3 transfer: the leaver of `side` sells n at x (at or above its ask for a long, at or below its bid for a short) to
/// an entrant, who takes its place in a part of size n. Needs the leaver's listing to be live (see [`amend`]).
pub fn transfer(l: &LivePair, side: i64, n: i64, x: i64, ent: &Entrant, k: i64, funding: &Funding, opts: &Opts) -> Result<Built> {
    let p = &l.p;
    p.validate()?;
    req(n > 0 && n <= p.size && n >= p.m.min_size && (p.size - n == 0 || p.size - n >= p.m.min_size), || format!("n = {n} is not a valid part of size {}", p.size))?;
    req(k == 0 || k == 1, || "k must be 0 or 1".into())?;
    if side == LONG {
        req(p.long_ask > 0, || "the long is not listed (amend first)".into())?;
        req(x >= p.long_ask, || format!("x = {x} is below the long's ask {}", p.long_ask))?;
        req(n >= p.long_min || n == p.size, || format!("n = {n} is below the long's smallest part {}", p.long_min))?;
    } else {
        req(side == SHORT, || "side must be 1 or 2".into())?;
        req(p.short_bid > 0, || "the short is not listed (amend first)".into())?;
        req(x <= p.short_bid, || format!("x = {x} is above the short's bid {}", p.short_bid))?;
        req(n >= p.short_min || n == p.size, || format!("n = {n} is below the short's smallest part {}", p.short_min))?;
    }
    let t = transfer_spec(l, side, n, x, ent, k)?;
    // No-downgrade (DESIGN 3.4): the stayer's counterparty bound may not fall.
    if side == LONG {
        req(t.nb >= 0 && t.nb <= p.bl, || format!("the entrant's bound {} would raise bl above {} (no-downgrade)", t.nb, p.bl))?;
    } else {
        req(t.nb >= p.bs && t.nb <= p.m.price_max, || format!("the entrant's bound {} would lower bs below {} (no-downgrade)", t.nb, p.bs))?;
    }
    let to = transfer_out(p, l.value, &t);
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_pair_in(l, "transfer", vec![iv(side), iv(n), iv(x), iv(t.nb), bv(&t.ek), bv(&t.et.as_bytes()), iv(k)], "pair (transfer)");
    let leaver_label = if side == LONG { "leaver (long) paid" } else { "leaver (short) paid" };
    d.add_plain(to.pay as u64, &to.leaver, leaver_label);
    let part_out = if to.rest.is_some() && k == 1 { 3 } else { 2 };
    let change;
    match ent {
        Entrant::Open(lo) => {
            d.add_open_in(lo, "take", take_args(&to.part, side, part_out, n, x), "entrant order (take)");
            d.fund_inputs(funding)?;
            order_output(&mut d, lo, 1, n);
            change = None;
        }
        Entrant::Direct { .. } => {
            d.fund_inputs(funding)?;
            change = Some(d.add_change(&funding.change_key));
        }
    }
    let part_idx = |d: &mut Draft, label: &str| d.add_pair_out(&to.part, to.part_value as u64, Some((0, l.id)), label);
    match &to.rest {
        None => {
            part_idx(&mut d, "pair part (new owner)");
        }
        Some((r, rv)) => {
            let rest_o = |d: &mut Draft| d.add_pair_out(r, *rv as u64, Some((0, l.id)), "pair remainder");
            if k == 0 {
                part_idx(&mut d, "pair part (new owner)");
                rest_o(&mut d);
            } else {
                rest_o(&mut d);
                part_idx(&mut d, "pair part (new owner)");
            }
        }
    }
    if change.is_none() {
        d.add_change(&funding.change_key);
    }
    d.finish()
}

// ---------------------------------------------------------------- P4

/// P4 close-versus-close: `host`'s long listing is filled by `donor`'s short listing, n at x. The host's long leaves
/// and the donor's short leaves; the host's short and the donor's long form a new pair at `host`'s id. `e3` is the
/// entry price of the new pair (`E(donor) <= e3 <= E(host)`).
pub fn close_vs_close(h: &LivePair, d: &LivePair, c: &CloseSpec, funding: &Funding, opts: &Opts) -> Result<Built> {
    let (hp, dp) = (&h.p, &d.p);
    req(hp.m == dp.m, || "the two pairs are of different markets".into())?;
    req(c.n > 0 && c.n <= hp.size && c.n <= dp.size, || "n exceeds a pair's size".into())?;
    req(hp.long_ask > 0 && c.x >= hp.long_ask, || "the host's long is not listed at or below x".into())?;
    req(dp.short_bid > 0 && c.x <= dp.short_bid, || "the donor's short is not listed at or above x".into())?;
    req(dp.bl <= hp.bl, || format!("no-downgrade: the donor's bl {} must be <= the host's bl {}", dp.bl, hp.bl))?;
    req(hp.bs >= dp.bs, || format!("no-downgrade: the host's bs {} must be >= the donor's bs {}", hp.bs, dp.bs))?;
    req(dp.entry <= c.e3 && c.e3 <= hp.entry, || format!("e3 = {} must be within [{}, {}] (the donor's and the host's entry)", c.e3, dp.entry, hp.entry))?;
    let co = close_out(hp, h.value, dp, d.value, d.id, c);
    let w = dp.long_window();
    let w2 = hp.short_window();
    let mut dr = Draft::new(opts.fee_rate, opts.fee_mode);
    dr.add_pair_in(
        h,
        "closeHost",
        vec![iv(c.n), iv(c.x), iv(c.e3), iv(0), iv(dp.bl), bv(&dp.long_key), iv(w.0), iv(w.1), iv(w.2), bv(&d.id.as_bytes())],
        "host pair (long leaves)",
    );
    dr.add_pair_in(d, "closeDonor", vec![iv(c.n), iv(c.x), iv(c.e3), iv(2), iv(hp.bs), bv(&hp.short_key), iv(w2.0), iv(w2.1), iv(w2.2)], "donor pair (short leaves)");
    dr.fund_inputs(funding)?;
    dr.add_plain(co.host_pay as u64, &hp.long_key, "host long paid");
    dr.add_plain(co.donor_pay as u64, &dp.short_key, "donor short paid");
    dr.add_pair_out(&co.p3, co.p3_value as u64, Some((0, h.id)), "new pair (host short + donor long)");
    if let Some((r, rv)) = &co.host_rest {
        dr.add_pair_out(r, *rv as u64, Some((0, h.id)), "host remainder");
    }
    if let Some((r, rv)) = &co.donor_rest {
        dr.add_pair_out(r, *rv as u64, Some((1, d.id)), "donor remainder");
    }
    dr.add_change(&funding.change_key);
    dr.finish()
}

/// Appends the filler outputs the reverse slot needs: the party at output `M - 1 - i` must sit at or above the input
/// count. Fillers are plain outputs of 0.02 KAS to the funder.
fn pad_for_tail(d: &mut Draft, input: usize, key: &[u8; 32]) {
    // M = outs + 1 once the tail is added.
    while d.outs.len() + 1 < d.ins.len() + 1 + input {
        d.add_plain(DUST_OUTPUT_MIN, key, "filler (keeps the reverse payout slot clear of the inputs)");
    }
}

/// P4' mutual close: both listings of one pair cross at x; n closes for both (the long at output 0, the short at the
/// last output).
pub fn mutual_close(l: &LivePair, n: i64, x: i64, funding: &Funding, opts: &Opts) -> Result<Built> {
    let p = &l.p;
    req(p.long_ask > 0 && p.short_bid > 0, || "both sides must be listed".into())?;
    req(p.long_ask <= x && x <= p.short_bid, || format!("x = {x} must be within [{}, {}] (longAsk, shortBid)", p.long_ask, p.short_bid))?;
    req(n > 0 && n <= p.size && n >= p.m.min_size && (p.size - n == 0 || p.size - n >= p.m.min_size), || format!("n = {n} is not a valid part of size {}", p.size))?;
    req(n >= p.long_min || n == p.size, || "n is below the long's smallest part".into())?;
    req(n >= p.short_min || n == p.size, || "n is below the short's smallest part".into())?;
    let mo = mutual_out(p, l.value, n, x);
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_pair_in(l, "mutualClose", vec![iv(n), iv(x)], "pair (mutual close)");
    d.fund_inputs(funding)?;
    if mo.lp > 0 {
        d.add_plain(mo.lp as u64, &p.long_key, "long paid");
    }
    if let Some((r, rv)) = &mo.rest {
        d.add_pair_out(r, *rv as u64, Some((0, l.id)), "pair remainder");
    }
    d.add_change(&funding.change_key);
    if mo.sp > 0 {
        pad_for_tail(&mut d, 0, &funding.change_key);
        d.add_plain(mo.sp as u64, &p.short_key, "short paid");
    }
    d.finish()
}

/// P8 idle settle: anyone, once the UTXO is `idleDaa` old; the pair ends at its entry price.
pub fn idle_settle(l: &LivePair, funding: &Funding, opts: &Opts) -> Result<Built> {
    let p = &l.p;
    p.validate()?;
    let (lp, sp) = idle_out(p, l.value);
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    let i = d.add_pair_in(l, "idleSettle", vec![], "pair (idle settle)");
    d.ins[i].sequence = p.m.idle_daa as u64;
    d.fund_inputs(funding)?;
    if lp > 0 {
        d.add_plain(lp as u64, &p.long_key, "long paid (entry price)");
    }
    d.add_change(&funding.change_key);
    if sp > 0 {
        pad_for_tail(&mut d, 0, &funding.change_key);
        d.add_plain(sp as u64, &p.short_key, "short paid (entry price)");
    }
    d.finish()
}

/// Which owner of a pair pays the fee of a cooperative close without a funder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeeFrom {
    Long,
    Short,
}

/// P9 cooperative close: both owners sign; the pair's value is split as they choose. The long gets `long_pay`; the
/// short gets the rest. With no `funding` the owner named by `fee_from` pays the network fee out of its share; with
/// `funding` the funder pays it and the two shares add up to the pair's value.
pub fn coop_close(l: &LivePair, long_pay: u64, fee_from: FeeFrom, funding: Option<&Funding>, opts: &Opts) -> Result<Built> {
    let p = &l.p;
    req(long_pay <= l.value, || format!("long_pay {long_pay} exceeds the pair's value {}", l.value))?;
    let short_pay = l.value - long_pay;
    req(long_pay == 0 || long_pay >= DUST_OUTPUT_MIN, || format!("long_pay {long_pay} is below the 0.02 KAS minimum output"))?;
    req(short_pay == 0 || short_pay >= DUST_OUTPUT_MIN, || format!("the short's share {short_pay} is below the 0.02 KAS minimum output"))?;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_pair_in(l, "coopClose", vec![Arg::Sig(p.long_key), Arg::Sig(p.short_key)], "pair (cooperative close)");
    let li = (long_pay > 0).then(|| d.add_plain(long_pay, &p.long_key, "long paid"));
    let si = (short_pay > 0).then(|| d.add_plain(short_pay, &p.short_key, "short paid"));
    match funding {
        Some(f) => {
            d.fund_inputs(f)?;
            d.add_change(&f.change_key);
        }
        None => match (fee_from, li, si) {
            (FeeFrom::Long, Some(i), _) | (FeeFrom::Short, _, Some(i)) => d.fee_from(i),
            (_, Some(i), None) | (_, None, Some(i)) => d.fee_from(i),
            _ => return invalid("nothing to pay"),
        },
    }
    d.finish()
}
