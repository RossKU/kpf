//! Property tests of the underwriting model (`kpf_protocol::bond`; docs/DESIGN-ORACLE-IMPL.md).
//!
//! Z3 (docs/DESIGN-ORACLE-2.md 7): seeded random sequences of every entry (bind at open and at renewal, advance, split,
//! merge, unbond, withdraw, reserve, release, pay, genesis pairs and slots with any state, rely), with arguments that
//! are honest, near the boundaries, or hostile, played by a hostile underwriter, a hostile pair owner, or both
//! together. On every reachable state:
//! - every slot an entry has checked counts at most `rho x` its own value (`ring_sum <= capacity`), unless a
//!   reservation has since taken value out of it (a slash; the note's invariant holds at every bind and split, not
//!   after a slash, see the docs);
//! - for every lineage and every expiry still liable, the cover of the pairs that rely on it is at most what the
//!   lineage's slots count for that expiry; so the cover relied on is at most `rho x` the lineage's KAS, plus `rho x`
//!   what reservations took;
//! - no sompi is created: the lineage's KAS leaves only by withdraw (its owner) or by a paid reservation, and every
//!   value is accounted for against the external funding;
//! - a pair made by a genesis (never bound) can rely on its slot only within one period of its own creation.
//!
//! Z8: effective capacity over a simulated month of opens and renewals, against the note's `rho_eff = rho * Lambda /
//! (Lambda + T_tail + Lambda / 8)`.

use kpf_protocol::bond::*;

// ---------------------------------------------------------------- a seeded generator

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            lo
        } else {
            lo + self.below((hi - lo + 1) as u64) as i64
        }
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    /// A value near `v`: itself mostly, one either side, or something wild.
    fn near(&mut self, v: i64) -> i64 {
        match self.below(10) {
            0 => v.saturating_sub(1),
            1 => v.saturating_add(1),
            2 => self.range(-5, 5),
            3 => [i64::MAX, i64::MAX / 2, -i64::MAX, 1 << 40][self.below(4) as usize],
            _ => v,
        }
    }
}

// ---------------------------------------------------------------- the world

fn params() -> BondParams {
    BondParams { rho_bps: 20_000, step: 100, term: 800, tail: 200, rate: 10, unbond: 50 }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Underwriter,
    PairOwner,
    Both,
}

#[derive(Clone, Debug)]
struct SlotU {
    lin: usize,
    value: i64,
    daa: i64,
    b: Bond,
    has_id: bool,
    /// The invariant was established by an entry (bind, split, merge) and not broken by a reservation since.
    checked: bool,
}

#[derive(Clone, Debug)]
struct PairU {
    daa: i64,
    p: StubPair,
    value: i64,
    /// Its current expiry was counted by a bind of its slot (open or renewal), not written by a genesis.
    credited: bool,
    genesis_daa: Option<i64>,
}

struct World {
    now: i64,
    slots: Vec<SlotU>,
    pairs: Vec<PairU>,
    reserved: Vec<(usize, i64)>,
    lins: usize,
    reserved_total: Vec<i128>,
    paid: Vec<i128>,
    ext_in: i128,
    ext_out: i128,
    accepted: usize,
    by_op: [usize; 14],
    refused: usize,
}

fn lin_id(l: usize) -> [u8; 32] {
    [l as u8 + 1; 32]
}
fn lin_key(l: usize) -> [u8; 32] {
    [0xa0 + l as u8; 32]
}
fn market() -> BondMarket {
    BondMarket { id: [0x4d; 32], p: params() }
}

impl World {
    fn new() -> World {
        World { now: 1_000, slots: vec![], pairs: vec![], reserved: vec![], lins: 0, reserved_total: vec![], paid: vec![], ext_in: 0, ext_out: 0, accepted: 0, by_op: [0; 14], refused: 0 }
    }

    fn new_lineage(&mut self, value: i64, b: Bond, has_id: bool, checked: bool) -> usize {
        let l = self.lins;
        self.lins += 1;
        self.reserved_total.push(0);
        self.paid.push(0);
        self.ext_in += value as i128;
        self.slots.push(SlotU { lin: l, value, daa: self.now, b, has_id, checked });
        l
    }

    fn total(&self) -> i128 {
        self.slots.iter().map(|s| s.value as i128).sum::<i128>()
            + self.pairs.iter().map(|p| p.value as i128).sum::<i128>()
            + self.reserved.iter().map(|r| r.1 as i128).sum::<i128>()
    }

    /// The context of a transaction spending slot `s` alone (an id-less slot is counted under no id: X1).
    fn ctx(&self, s: &SlotU, signed: bool, cov_out: usize, lock: i64, seq: i64) -> Ctx {
        let (cov_in, cov_out) = if s.has_id { (1, cov_out) } else { (0, 0) };
        Ctx { lock, seq, v_in: s.value, cov_in, cov_out, signed, has_id: s.has_id }
    }

    fn lineage_value(&self, l: usize) -> i128 {
        self.slots.iter().filter(|s| s.lin == l).map(|s| s.value as i128).sum::<i128>() + self.reserved.iter().filter(|r| r.0 == l).map(|r| r.1 as i128).sum::<i128>()
    }

    /// Every invariant of Z3 on the current state.
    fn check(&self, why: &str) {
        let p = params();
        for s in &self.slots {
            if s.checked && s.b.status == STATUS_SLOT {
                let sum = ring_sum(&s.b.ring).expect("checked slots have no negative bucket");
                assert!(sum <= capacity(s.value, p.rho_bps), "{why}: slot over capacity {sum} > {}", capacity(s.value, p.rho_bps));
            }
        }
        for l in 0..self.lins {
            let relied: Vec<&PairU> = self.pairs.iter().filter(|q| q.credited && q.p.uw_id == lin_id(l) && q.p.expiry + p.tail > self.now).collect();
            let mut xs: Vec<i64> = relied.iter().map(|q| q.p.expiry).collect();
            xs.sort();
            xs.dedup();
            let mut total_relied = 0i128;
            for x in xs {
                let want: i128 = relied.iter().filter(|q| q.p.expiry == x).map(|q| q.p.cover as i128).sum();
                let counted: i128 = self.slots.iter().filter(|s| s.lin == l).map(|s| s.b.counted_at(x).max(0) as i128).sum();
                assert!(want <= counted, "{why}: lineage {l} expiry {x}: relied {want} > counted {counted}");
                total_relied += want;
            }
            let k: i128 = self.slots.iter().filter(|s| s.lin == l && s.b.status == STATUS_SLOT).map(|s| capacity(s.value, p.rho_bps) as i128).sum();
            let slashed = self.reserved_total[l] * p.rho_bps as i128 / 10_000;
            assert!(total_relied <= k + slashed, "{why}: lineage {l}: relied {total_relied} > rho K {k} + rho x reserved {slashed}");
            if self.reserved_total[l] == 0 {
                assert!(total_relied <= k, "{why}: lineage {l} never slashed: relied {total_relied} > rho K {k}");
            }
        }
        assert_eq!(self.total(), self.ext_in - self.ext_out, "{why}: sompi created or lost");
    }
}

/// The largest advance a bind at `lock` can prove for slot `b`.
fn max_advance(b: &Bond, lock: i64) -> i64 {
    let p = &b.m.p;
    // (j0 + d) * step + tail <= lock
    let d = (lock - p.tail).div_euclid(p.step).saturating_sub(b.j0);
    d.clamp(0, 64)
}

/// The latest boundary a bind at `t` may choose (saturating: a hostile `t` may be anything).
fn latest_x(p: &BondParams, t: i64) -> i64 {
    t.saturating_add(p.term + p.step) / p.step * p.step
}

fn step(w: &mut World, rng: &mut Rng, mode: Mode) {
    let p = params();
    let now = w.now;
    let uw_signs = mode != Mode::PairOwner || rng.chance(5);
    let op = rng.below(14);
    let acc0 = w.accepted;
    let lv: Vec<i128> = (0..w.lins).map(|l| w.lineage_value(l)).collect();
    let mut withdrew: Option<usize> = None;
    let mut paid_lin: Option<usize> = None;
    match op {
        // Open: a new pair bound by a slot.
        0 | 1 | 2 => {
            if w.slots.is_empty() {
                return;
            }
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            let lock = now - rng.range(0, 3) * if rng.chance(20) { 50 } else { 0 };
            let t = rng.near(lock);
            let d = rng.near(max_advance(&s.b, lock));
            let x = if rng.chance(80) { latest_x(&p, t) - p.step * rng.range(0, 2) } else { rng.near(latest_x(&p, t)) };
            let free = capacity(s.value, p.rho_bps) - ring_sum(&s.b.ring).unwrap_or(0);
            let c = match rng.below(4) {
                0 => free,
                1 => free.saturating_add(1),
                2 => rng.range(1, s.value.max(1)),
                _ => rng.near(free / 3),
            }
            .max(1);
            let pair_out = 1usize;
            let r = if rng.chance(90) { OUT_REF + pair_out as i64 } else { rng.near(OUT_REF) };
            let junk = if rng.chance(15) { vec![Claim { r: r.saturating_add(1), c: rng.range(0, 10) }] } else { vec![] };
            let claims: Vec<Claim> = [vec![Claim { r, c: rng.near(c) }], junk].concat();
            let a = BindArgs::new(t, x, d, &claims);
            let v_out = if rng.chance(10) { s.value - 1 } else { s.value + if rng.chance(10) { rng.range(0, 1_000) } else { 0 } };
            let ctx = w.ctx(&s, uw_signs, 1, lock, now - s.daa);
            let order = StubOpen { m: market(), uw_accept: if rng.chance(10) { lin_id(rng.below(3) as usize) } else { [0; 32] }, owner: [0x77; 32], cover: c };
            let b2 = s.b.bind(&ctx, &a, v_out);
            let view = SlotView { bond: &s.b, id: if s.has_id { lin_id(s.lin) } else { [0; 32] }, args: &a, canonical_bind: true };
            let twins = if rng.chance(5) { 1 } else { 0 };
            let fill = order.fill(true, pair_out, Some(&view), 0, x, 3 + twins, twins);
            match (b2, fill) {
                (Ok(b2), Ok(pair)) => {
                    let pv = rng.range(1, 1_000);
                    w.ext_in += (v_out - s.value) as i128 + pv as i128;
                    w.slots[i] = SlotU { value: v_out, daa: now, b: b2, checked: true, ..s };
                    w.pairs.push(PairU { daa: now, p: pair, value: pv, credited: true, genesis_daa: None });
                    w.accepted += 1;
                }
                _ => w.refused += 1,
            }
        }
        // Renewal of up to 7 pairs of one expiry by one slot.
        3 | 4 => {
            if w.slots.is_empty() || w.pairs.is_empty() {
                return;
            }
            // Mostly a pair in its renewal window, renewed by a slot of its own lineage.
            let due: Vec<usize> = (0..w.pairs.len()).filter(|&q| w.pairs[q].p.expiry - p.step <= now && now <= w.pairs[q].p.expiry + p.step).collect();
            let q0 = if !due.is_empty() && rng.chance(85) { due[rng.below(due.len() as u64) as usize] } else { rng.below(w.pairs.len() as u64) as usize };
            let x0 = w.pairs[q0].p.expiry;
            let own: Vec<usize> = (0..w.slots.len()).filter(|&k| lin_id(w.slots[k].lin) == w.pairs[q0].p.uw_id && w.slots[k].b.status == STATUS_SLOT).collect();
            let i = if !own.is_empty() && rng.chance(85) { own[rng.below(own.len() as u64) as usize] } else { rng.below(w.slots.len() as u64) as usize };
            let s = w.slots[i].clone();
            let mut idx: Vec<usize> = (0..w.pairs.len()).filter(|&q| (w.pairs[q].p.expiry == x0 && w.pairs[q].p.uw_id == w.pairs[q0].p.uw_id) || rng.chance(2)).collect();
            idx.truncate(rng.range(1, 7) as usize);
            let lock = now;
            let t = rng.near(lock);
            let x = rng.near(x0.saturating_add(p.term));
            let d = rng.near(max_advance(&s.b, lock));
            let mut claims: Vec<Claim> = idx.iter().enumerate().map(|(k, &q)| Claim { r: k as i64, c: rng.near(w.pairs[q].p.cover) }).collect();
            if rng.chance(10) && claims.len() > 1 {
                claims.swap(0, 1);
            }
            let a = BindArgs::new(t, x, d, &claims);
            let ctx = w.ctx(&s, uw_signs, 1, lock, now - s.daa);
            let b2 = s.b.bind(&ctx, &a, s.value);
            let view = SlotView { bond: &s.b, id: if s.has_id { lin_id(s.lin) } else { [0; 32] }, args: &a, canonical_bind: true };
            let renewed: Vec<Result<StubPair, Refusal>> = idx.iter().enumerate().map(|(k, &q)| w.pairs[q].p.renew(true, k, Some(&view), k as i64)).collect();
            if b2.is_ok() && renewed.iter().all(|r| r.is_ok()) {
                w.slots[i] = SlotU { daa: now, b: b2.unwrap(), checked: true, ..s };
                for (k, &q) in idx.iter().enumerate() {
                    w.pairs[q].p = renewed[k].clone().unwrap();
                    w.pairs[q].daa = now;
                    w.pairs[q].credited = true;
                }
                w.accepted += 1;
            } else {
                w.refused += 1;
            }
        }
        // Advance, by anyone.
        5 => {
            if w.slots.is_empty() {
                return;
            }
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            let d = rng.near(max_advance(&s.b, now).max(1));
            let ctx = w.ctx(&s, false, 1, now, 0);
            match s.b.advance(&ctx, d, s.value) {
                Ok(b2) => {
                    w.slots[i] = SlotU { daa: now, b: b2, ..s };
                    w.accepted += 1;
                }
                Err(_) => w.refused += 1,
            }
        }
        // Split.
        6 => {
            if w.slots.is_empty() {
                return;
            }
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            let mut a = [0i64; RING];
            for (k, b) in s.b.ring.iter().enumerate() {
                a[k] = match rng.below(5) {
                    0 => *b,
                    1 => b / 2,
                    2 => rng.near(*b),
                    _ => 0,
                };
            }
            let va = rng.range(0, s.value);
            // An output value is a u64 below the supply.
            let vb = rng.near(s.value - va).clamp(0, 2_900_000_000_000_000_000);
            let ctx = w.ctx(&s, uw_signs, 2, now, now - s.daa);
            match s.b.split(&ctx, &a, va, vb) {
                Ok((ba, bb)) => {
                    w.ext_in += (va + vb - s.value) as i128;
                    w.slots[i] = SlotU { value: va, daa: now, b: ba, checked: true, ..s.clone() };
                    w.slots.push(SlotU { value: vb, daa: now, b: bb, checked: true, ..s });
                    w.accepted += 1;
                }
                Err(_) => w.refused += 1,
            }
        }
        // Merge (model only).
        7 => {
            if w.slots.len() < 2 || !uw_signs {
                return;
            }
            let i = rng.below(w.slots.len() as u64) as usize;
            let j = rng.below(w.slots.len() as u64) as usize;
            if i == j || w.slots[i].lin != w.slots[j].lin {
                w.refused += 1;
                return;
            }
            let (a, b) = (w.slots[i].clone(), w.slots[j].clone());
            match a.b.merge(a.value, &b.b, b.value) {
                Ok((m, v)) => {
                    w.slots[i] = SlotU { value: v, daa: now, b: m, checked: true, ..a };
                    w.slots.remove(j);
                    w.accepted += 1;
                }
                Err(_) => w.refused += 1,
            }
        }
        // Unbond, then withdraw.
        8 => {
            if w.slots.is_empty() {
                return;
            }
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            if s.b.status == STATUS_UNBONDING || !s.has_id {
                let ctx = Ctx { lock: now, seq: now - s.daa, v_in: s.value, cov_in: 1, cov_out: 0, signed: uw_signs, has_id: s.has_id };
                if s.b.withdraw(&ctx).is_ok() {
                    w.ext_out += s.value as i128;
                    withdrew = Some(s.lin);
                    w.slots.remove(i);
                    w.accepted += 1;
                } else {
                    w.refused += 1;
                }
            } else {
                let d = rng.near(max_advance(&s.b, now));
                let ctx = w.ctx(&s, uw_signs, 1, now, now - s.daa);
                match s.b.unbond(&ctx, d, s.value) {
                    Ok(b2) => {
                        w.slots[i] = SlotU { daa: now, b: b2, ..s };
                        w.accepted += 1;
                    }
                    Err(_) => w.refused += 1,
                }
            }
        }
        // Reserve (a dispute opening, by anyone), release or pay (its resolution).
        9 => {
            if !w.reserved.is_empty() && rng.chance(60) {
                let r = w.reserved.remove(rng.below(w.reserved.len() as u64) as usize);
                if rng.chance(50) {
                    // Upheld: the reservation returns as an active child of the lineage with an empty ring.
                    let any = w.slots.iter().find(|s| s.lin == r.0).map(|s| s.b.clone());
                    let b = any.map(|b| Bond { status: STATUS_SLOT, ring: [0; RING], ..b }).unwrap_or_else(|| Bond::new(lin_key(r.0), market(), 0));
                    w.slots.push(SlotU { lin: r.0, value: r.1, daa: now, b, has_id: true, checked: true });
                } else {
                    // Refuted: paid to the victim, the disputer and the burn.
                    w.ext_out += r.1 as i128;
                    w.paid[r.0] += r.1 as i128;
                    paid_lin = Some(r.0);
                }
                w.accepted += 1;
            } else if !w.slots.is_empty() {
                let i = rng.below(w.slots.len() as u64) as usize;
                let s = w.slots[i].clone();
                let amt = rng.range(1, s.value.max(1));
                match s.b.reserve(s.value, amt) {
                    Ok(left) => {
                        w.slots[i].value = left;
                        w.slots[i].daa = now;
                        w.slots[i].checked = false;
                        w.reserved.push((s.lin, amt));
                        w.reserved_total[s.lin] += amt as i128;
                        w.accepted += 1;
                    }
                    Err(_) => w.refused += 1,
                }
            }
        }
        // A genesis pair naming a slot it was never bound by (the hostile pair owner, or a colluding underwriter).
        10 => {
            if w.lins == 0 || mode == Mode::Underwriter {
                return;
            }
            let l = rng.below(w.lins as u64) as usize;
            let x = rng.range(now - 300, now + 3 * p.term) / p.step * p.step;
            let pr = StubPair { m: market(), uw_id: lin_id(l), uw_key: lin_key(l), owner: [0x99; 32], cover: rng.range(1, 1 << 40), expiry: x };
            let v = rng.range(1, 100);
            w.ext_in += v as i128;
            w.pairs.push(PairU { daa: now, p: pr, value: v, credited: false, genesis_daa: Some(now) });
            w.accepted += 1;
        }
        // A genesis slot with any state (negative or huge buckets, a far ring position, any status), a new lineage.
        11 => {
            if w.lins >= 6 {
                return;
            }
            let mut b = Bond::new(lin_key(w.lins), market(), rng.near(now / p.step));
            for k in 0..RING {
                if rng.chance(20) {
                    let v = rng.range(-1_000, 1_000_000);
                    b.ring[k] = rng.near(v);
                }
            }
            if rng.chance(10) {
                b.status = rng.range(0, 4);
            }
            let forged = b.ring.iter().any(|x| *x != 0) || b.status != STATUS_SLOT;
            w.new_lineage(rng.range(1, 1 << 30), b, rng.chance(95), !forged);
        }
        // Rely (the next slice's price claim).
        12 => {
            if w.pairs.is_empty() {
                return;
            }
            let q = rng.below(w.pairs.len() as u64) as usize;
            let pr = &w.pairs[q];
            if pr.p.rely(pr.daa, true).is_ok() {
                if let Some(g) = pr.genesis_daa {
                    if !pr.credited {
                        assert!(pr.p.expiry <= g + p.term + p.step, "an uncounted pair relied beyond one period of its genesis");
                    }
                }
                w.pairs[q].daa = now;
                w.accepted += 1;
            } else {
                w.refused += 1;
            }
        }
        // Time passes.
        _ => {
            w.now += rng.range(1, 2 * p.step);
        }
    }
    // Conservation: only withdraw and a paid reservation take KAS out of a lineage; nothing else lowers it.
    for (l, v0) in lv.iter().enumerate() {
        let v1 = w.lineage_value(l);
        if Some(l) != withdrew && Some(l) != paid_lin {
            assert!(v1 >= *v0, "op {op}: lineage {l} lost value {v0} -> {v1}");
        }
    }
    w.by_op[op as usize] += w.accepted - acc0;
    w.check(&format!("op {op}"));
}

fn run_mode(seed: u64, mode: Mode, steps: usize) -> (usize, usize, [usize; 14]) {
    let mut rng = Rng(seed);
    let mut w = World::new();
    for l in 0..2 {
        let b = Bond::new(lin_key(l), market(), 0);
        w.new_lineage(1_000_000 * (l as i64 + 1), b, true, true);
    }
    for _ in 0..steps {
        step(&mut w, &mut rng, mode);
    }
    (w.accepted, w.refused, w.by_op)
}

#[test]
fn z3_no_reachable_state_counts_more_than_rho_k_or_creates_sompi() {
    let mut acc = 0;
    let mut by_op = [0usize; 14];
    let mut refd = 0;
    for seed in 0..48u64 {
        for mode in [Mode::Underwriter, Mode::PairOwner, Mode::Both] {
            let (a, r, ops) = run_mode(0x5eed_0000 + seed, mode, 3_000);
            for k in 0..14 {
                by_op[k] += ops[k];
            }
            acc += a;
            refd += r;
        }
    }
    println!("Z3: {acc} accepted and {refd} refused steps over 144 runs; accepted by kind (open x3, renew x2, advance, split, merge, unbond or withdraw, reserve or resolve, genesis pair, genesis slot, rely, time): {by_op:?}");
    for k in [0usize, 3, 5, 6, 7, 8, 9, 10, 12] {
        assert!(by_op[k] > 50, "step kind {k} accepted only {} times", by_op[k]);
    }
    assert!(acc > 10_000 && refd > 10_000, "both kinds of steps occur: {acc} / {refd}");
}

/// The note's ring of 8 buckets of `Lambda / 8` cannot hold a renewal in the last step (or any earlier one) when the
/// tail is positive: after the ring is advanced to the present, its last bucket's expiry is at most 7 steps ahead.
/// Shown on the model with a ring of 8 in use (the market bound `term + tail + 2 step <= 8 steps` fails for any term
/// of 8 steps; checked here by hand on the 12-bucket ring restricted to its first 8 buckets).
#[test]
fn z3_the_notes_eight_bucket_ring_cannot_hold_a_renewal() {
    let p = BondParams { rho_bps: 10_000, step: 100, term: 800, tail: 200, rate: 0, unbond: 0 };
    // The note's own numbers: Lambda = 8 steps, T_tail = 3.25 days of 3.5-day steps (here 2 steps; any tail > 0).
    assert!(p.term + p.tail + 2 * p.step > 8 * p.step, "the bound a ring of 8 would need fails");
    // A pair expiring at X = 1,600 renews at t = X - step = 1,500 to X' = 2,400. Advance as far as the lock proves.
    let b = Bond::new([1; 32], BondMarket { id: [0; 32], p }, 0);
    let d = max_advance(&b, 1_500);
    let j = b.j0 + d;
    // The 8th bucket of the advanced ring expires at (j + 8) * step.
    let top8 = (j + 8) * p.step;
    assert!(top8 < 2_400, "the 8-bucket ring's last expiry {top8} is below the renewal's 2,400");
    // And the old cover must stay until X + tail = 1,800, so the ring cannot be advanced past it either.
    assert!(j * p.step + p.tail <= 1_500 && (j + 1) * p.step + p.tail > 1_500);
    // With RING = 12 it fits.
    assert!((j + RING as i64) * p.step >= 2_400);
}

#[test]
fn z3_boundaries_of_the_bounds() {
    let mut p = BondParams { rho_bps: 50_000, step: 100, term: 800, tail: 200, rate: 0, unbond: 0 };
    assert!(p.check().is_ok(), "rho 5 and term + tail + 2 step = 12 steps");
    p.rho_bps = 50_001;
    assert!(p.check().is_err(), "rho above 5");
    p.rho_bps = 9_999;
    assert!(p.check().is_err(), "rho below 1");
    p.rho_bps = 10_000;
    p.tail = 201;
    assert!(p.check().is_err(), "the ring cannot hold a renewal");
    p.tail = 200;
    p.term = 850;
    assert!(p.check().is_err(), "term not a whole number of steps");
    // The capacity on the largest value: clamped, no overflow.
    assert_eq!(capacity(i64::MAX, 50_000), 5_000_000_000_000_000_000);
    assert_eq!(capacity(VALUE_CLAMP, 50_000), 5 * VALUE_CLAMP);
    assert_eq!(capacity(9_999, 10_000), 9_999);
    assert_eq!(capacity(10_001, 15_000), 15_001);
}

// ---------------------------------------------------------------- Z8

struct Z8 {
    /// Average and smallest (cover in force) / (cover the slot counts), over the measured month.
    avg: f64,
    worst: f64,
    /// The largest ring sum over the run, and renewals the slot refused.
    peak: i64,
    lapsed: usize,
}

/// `n` pairs of cover `c` open at evenly spread times over the first term (each with the latest expiry the window
/// allows) and renew forever, `renew_before(rng)` DAA before their expiry (within the last step), on one slot of
/// value `k`. Every hour the ring is read after advancing it to the present. Measured over the 30 days after a month.
fn z8_run(p: BondParams, k: i64, n: usize, c: i64, renew_before: &dyn Fn(&mut Rng) -> i64, seed: u64) -> Z8 {
    let mut rng = Rng(seed);
    let month = 30 * 864_000; // 30 days at 10 DAA per second
    let ctx = |lock: i64| Ctx { lock, seq: 1 << 31, v_in: k, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let start = p.tail + 2 * p.step;
    let mut b = Bond::new([1; 32], BondMarket { id: [0; 32], p }, 0);
    let mut opens: Vec<i64> = (0..n).map(|i| start + (i as i64 * p.term) / n as i64).collect();
    opens.reverse();
    // (expiry, renewal time)
    let mut pairs: Vec<(i64, i64)> = vec![];
    let signed_price = 36_000; // one hour
    let (mut sum, mut cnt, mut worst, mut peak, mut lapsed) = (0f64, 0f64, f64::MAX, 0i64, 0usize);
    let mut t = start;
    while t < start + 2 * month {
        while opens.last().is_some_and(|o| *o <= t) {
            opens.pop();
            let x = latest_x(&p, t);
            let a = BindArgs::new(t, x, max_advance(&b, t), &[Claim::output(1, c)]);
            match b.bind(&ctx(t), &a, k) {
                Ok(b2) => {
                    b = b2;
                    pairs.push((x, x - renew_before(&mut rng)));
                }
                Err(_) => lapsed += 1,
            }
        }
        let mut keep = vec![];
        for (x, at) in pairs.drain(..) {
            if t < at {
                keep.push((x, at));
                continue;
            }
            let a = BindArgs::new(t, x + p.term, max_advance(&b, t), &[Claim::input(0, c)]);
            match b.bind(&ctx(t), &a, k) {
                Ok(b2) => {
                    b = b2;
                    keep.push((x + p.term, x + p.term - renew_before(&mut rng)));
                }
                Err(_) => lapsed += 1,
            }
        }
        pairs = keep;
        let counted = ring_sum(&b.advanced(max_advance(&b, t), t).expect("advance")).expect("sum");
        peak = peak.max(counted);
        if t >= start + month {
            let live = pairs.iter().filter(|(x, _)| *x > t).count() as i64 * c;
            let r = live as f64 / counted as f64;
            sum += r;
            cnt += 1.0;
            worst = worst.min(r);
        }
        t += signed_price;
    }
    Z8 { avg: sum / cnt, worst, peak, lapsed }
}

#[test]
fn z8_effective_capacity_over_a_month_matches_the_formula() {
    let p = BondParams::testnet();
    assert!(p.check().is_ok());
    let (lam, tail, s) = (p.term as f64, p.tail as f64, p.step as f64);
    let (n, c) = (480usize, 200_000_000_000i64); // 480 pairs of 2,000 KAS each
    let big = n as i64 * c * 2; // a slot that never binds
    // (case, renewal lead before X, the formula with the mean lead, the formula with the largest lead)
    let note = lam / (lam + tail + s);
    let cases: [(&str, Box<dyn Fn(&mut Rng) -> i64>, f64, f64); 3] = [
        ("renewal at X - step (the note's case)", Box::new(move |_: &mut Rng| p.step), note, note),
        ("renewal uniform in the last step", Box::new(move |r: &mut Rng| r.range(1, p.step)), lam / (lam + tail + s / 2.0), note),
        ("renewal just before X", Box::new(|_: &mut Rng| 1), lam / (lam + tail), lam / (lam + tail)),
    ];
    for (i, (name, rb, f_avg, f_peak)) in cases.iter().enumerate() {
        let r = z8_run(p, big, n, c, rb.as_ref(), 10 + i as u64);
        assert_eq!(r.lapsed, 0, "{name}: an unbounded slot refuses nothing");
        // A slot exactly as large as the peak (rho = 1) serves the whole population; one sompi less does not.
        let fit = z8_run(p, r.peak, n, c, rb.as_ref(), 10 + i as u64);
        assert_eq!(fit.lapsed, 0, "{name}: K = peak serves every renewal");
        let short = z8_run(p, r.peak - 1, n, c, rb.as_ref(), 10 + i as u64);
        assert!(short.lapsed > 0, "{name}: K = peak - 1 refuses a bind");
        let eff = (n as i64 * c) as f64 / r.peak as f64;
        println!(
            "Z8 {name}: cover in force / counted, average {:.4} (formula with the mean lead {f_avg:.4}), worst hour {:.4}; effective multiple rho_eff / rho = in force / K needed = {eff:.4} (formula with the largest lead {f_peak:.4})",
            r.avg, r.worst
        );
        assert!((r.avg - f_avg).abs() / f_avg < 0.05, "{name}: average within 5 % of the formula");
        assert!((eff - f_peak).abs() / f_peak < 0.05, "{name}: effective multiple within 5 % of the formula");
    }
}
