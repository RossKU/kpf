//! Reviews C1 and C2 of the committee family (docs/audit/REVIEW-C1.md, REVIEW-C2.md): two seeded random walks in the
//! script engine, on the family as fixed after review C1.
//!
//! The committee walk drives joins, top-ups, leaves, rolls, withdrawals, retirements and attestations with arguments
//! drawn wide (out-of-range members, bids a sompi short, top-ups below 1 KAS, wrong signers, respends inside the committee's
//! rest, early lock times, signer sets below the quorum, signed prices outside the spread of the price claimed). Every
//! step is predicted by the library model (`kpf_protocol::committee`) and the engine must agree; every output of an
//! accepted transaction is pinned (one sompi less is refused); the stake is conserved; and a stake's release is never
//! earlier than the exit time after the last epoch table that holds its key.
//!
//! The market walk drives pair lineages of a market whose priceMax (13 KAS) the feed crosses: orders taken with no
//! pair and by a listed side of a pair, amend, addMargin, assign and transfer of a whole or a part, mutualClose,
//! coopClose, keepAlive, lapseSettle, closes and liquidations with fresh and stale signed prices on a whole or a part,
//! finalize, cure. The engine must agree with the prediction; every output is pinned; and a party that did not sign
//! keeps its value at the feed in every step but the two the note lists (a valid claim's settlement, the lapse).
//!
//! `C1_SEEDS=a..b` chooses the seeds (default 1..6), `C1_STEPS` the steps of each walk (default 220).
#![allow(non_snake_case)]

mod committeekit;
mod common;

use std::collections::{BTreeMap, HashMap};

use committeekit::*;
use common::*;
use kaspa_consensus_core::tx::TransactionOutput;
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::oracle::{maker_of, Makers};
use kpf_protocol::payoff::{owed, q};
use kpf_protocol::state::{BPS, KIND_CLOSE, KIND_LIQ, LOCK_TIME_THRESHOLD, LONG, SHORT};
use secp256k1::Keypair;

const KAS1: i64 = 100_000_000;

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 11) % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn pick<V: Copy>(&mut self, v: &[V]) -> V {
        v[self.below(v.len() as u64) as usize]
    }
}

fn seeds() -> Vec<u64> {
    let s = std::env::var("C1_SEEDS").unwrap_or_else(|_| "1..6".into());
    let (a, b) = s.split_once("..").expect("C1_SEEDS=a..b");
    (a.parse().expect("a")..=b.parse().expect("b")).collect()
}
fn steps() -> usize {
    std::env::var("C1_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(220)
}

type Tweak<'a> = &'a dyn Fn(&mut Vec<TransactionOutput>);
type Stats = BTreeMap<&'static str, (usize, usize)>;

fn hit(s: &mut Stats, what: &'static str, ok: bool) {
    let e = s.entry(what).or_insert((0, 0));
    if ok {
        e.0 += 1;
    } else {
        e.1 += 1;
    }
}

/// The transaction is accepted and every output of more than one sompi is pinned by a script.
fn must_pinned(what: &str, build: &dyn Fn(Tweak) -> T) -> T {
    let t = build(&|_| {});
    must(what, &t);
    for j in 0..t.0.outputs.len() {
        if t.0.outputs[j].value > 1 {
            assert!(!accepted(&build(&|o| o[j].value -= 1)), "{what}: output {j} one sompi short was accepted");
        }
    }
    t
}

// ---------------------------------------------------------------- the committee walk

fn committee_walk(seed: u64, steps: usize, n: &mut Stats) {
    let p = rparams();
    let mut rng = Rng::new(seed);
    let mut now: i64 = 5_000;
    let mut l: LCommittee = Live { op: outpoint(1), value: KAS1 as u64, daa: 0, id: Some(RID), s: Committee::empty(p, 5_000) };
    let base = l.value as i64;
    let (mut exits, mut epoch_tables): (Vec<LExit>, Vec<LEpochTable>) = (vec![], vec![]);
    let (mut paid_in, mut withdrawn) = (0i64, 0i64);
    let mut last_until: HashMap<[u8; 32], i64> = HashMap::new();
    let kp_of = |key: &[u8; 32]| (0..20).map(member_kp).find(|k| x(k) == *key);
    for _ in 0..steps {
        now += if rng.chance(25) { rng.below(15) } else { rng.below(300) } as i64;
        // The committee input's sequence is its true age, capped: below REST_DAA the three table entries are refused.
        let seq = ((now as u64).saturating_sub(l.daa)).min(REST_DAA);
        let rested = seq >= REST_DAA;
        let released = |e: &Exit, lu: &HashMap<[u8; 32], i64>| {
            if let Some(u) = lu.get(&e.key) {
                assert!(e.release >= u + p.exit, "seed {seed}: a stake released at {} while an epoch table holds its key until {u}", e.release);
            }
        };
        match rng.below(12) {
            0..=3 => {
                // Mostly the member a newcomer would take (an empty one, else the lightest), sometimes any, sometimes none.
                let lightest = (0..MEMBERS).min_by_key(|j| l.s.members[*j].weight).unwrap();
                let i = if rng.chance(65) { lightest } else { rng.below(MEMBERS as u64 + 1) as usize };
                let key = x(&member_kp(rng.below(20) as usize));
                let least = if i < MEMBERS { l.s.least_bid(i).unwrap_or(p.min_member_stake) } else { p.min_member_stake };
                let amount = match rng.below(5) {
                    0 => least - 1,
                    1 => least,
                    2 => least + rng.below(40) as i64 * KAS1,
                    3 => p.min_member_stake + rng.below(30) as i64 * KAS1,
                    _ => rng.below(3) as i64 - 1,
                };
                let build = |tw: Tweak| with_rest(seq, || join_tx(&l, i, key, amount, |o| tw(o)));
                match l.s.join(i, key, amount).ok().filter(|_| rested) {
                    Some((r, exit, _)) => {
                        let t = must_pinned("join", &build);
                        hit(n, if exit.is_some() { "join (outbidding)" } else { "join" }, true);
                        if let Some(e) = exit {
                            released(&e, &last_until);
                            exits.push(live(&t, 1, now as u64, &e));
                        }
                        l = live(&t, 0, now as u64, &r);
                        paid_in += amount;
                    }
                    None => {
                        assert!(!accepted(&build(&|_| {})), "seed {seed}: a join the model refuses (member {i}, {amount}, rest {seq})");
                        hit(n, if rested { "join" } else { "join (in the rest)" }, false);
                    }
                }
            }
            4 => {
                let i = rng.below(MEMBERS as u64) as usize;
                let amount = rng.pick(&[0, -5, 1, KAS1 - 1, KAS1, KAS1, 3 * KAS1, MEMBER_MAX]);
                let build = |tw: Tweak| with_rest(seq, || top_up_tx(&l, i, amount, |o| tw(o)));
                match l.s.top_up(i, amount).ok().filter(|_| rested) {
                    Some(r) => {
                        let t = must_pinned("topUp", &build);
                        l = live(&t, 0, now as u64, &r);
                        paid_in += amount;
                        hit(n, "top-up", true);
                    }
                    None => {
                        assert!(!accepted(&build(&|_| {})), "seed {seed}: a top-up the model refuses (member {i}, {amount}, rest {seq})");
                        hit(n, if rested { "top-up" } else { "top-up (in the rest)" }, false);
                    }
                }
            }
            5 => {
                let i = rng.below(MEMBERS as u64) as usize;
                let own = kp_of(&l.s.members[i].key);
                let signer = if rng.chance(85) { own.unwrap_or(member_kp(0)) } else { keypair(77) };
                let ok = l.s.members[i].taken() && own.map(|k| x(&k)) == Some(x(&signer)) && rested;
                if !l.s.members[i].taken() || rng.chance(60) {
                    continue;
                }
                let build = |tw: Tweak| with_rest(seq, || leave_tx(&l, i, &signer, |o| tw(o)));
                if ok {
                    let (r, e) = l.s.leave(i).expect("the model's leave");
                    let t = must_pinned("leave", &build);
                    released(&e, &last_until);
                    exits.push(live(&t, 1, now as u64, &e));
                    l = live(&t, 0, now as u64, &r);
                    hit(n, "leave", true);
                } else {
                    assert!(!accepted(&build(&|_| {})), "seed {seed}: a leave by another key or in the rest");
                    hit(n, if rested { "leave" } else { "leave (in the rest)" }, false);
                }
            }
            6 | 7 => {
                // A roll needs no rest: its input carries no sequence.
                let m = 1 + rng.below(3) as i64;
                let lock = if rng.chance(80) { now } else { now - rng.below(1_500) as i64 }.max(0);
                let build = |tw: Tweak| roll_tx(&l, m, lock as u64, |o| tw(o));
                match l.s.roll(m, lock, l.value as i64) {
                    Ok((r, tk)) => {
                        let t = must_pinned("roll", &build);
                        for k in 1..=p.epoch_tables as usize {
                            epoch_tables.push(live(&t, k, now as u64, &tk));
                        }
                        for s in tk.members.iter().filter(|s| s.taken()) {
                            let u = last_until.entry(s.key).or_insert(0);
                            *u = (*u).max(tk.until);
                        }
                        hit(n, if rested { "roll" } else { "roll (in the rest)" }, true);
                        l = live(&t, 0, now as u64, &r);
                    }
                    Err(_) => {
                        assert!(!accepted(&build(&|_| {})), "seed {seed}: a roll the model refuses (m {m}, lock {lock}, from {})", l.s.from);
                        hit(n, "roll", false);
                    }
                }
            }
            8 => {
                if exits.is_empty() {
                    continue;
                }
                let k = rng.below(exits.len() as u64) as usize;
                let e = exits[k].clone();
                let build = |tw: Tweak| withdraw_tx(&e, now as u64, |o| tw(o));
                if e.s.withdraw(now).is_ok() {
                    must_pinned("withdraw", &build);
                    withdrawn += e.s.amount;
                    exits.swap_remove(k);
                    hit(n, "withdraw", true);
                } else {
                    assert!(!accepted(&build(&|_| {})), "seed {seed}: an early withdraw");
                    hit(n, "withdraw", false);
                }
            }
            9 => {
                if epoch_tables.is_empty() {
                    continue;
                }
                let k = rng.below(epoch_tables.len() as u64) as usize;
                let tk = epoch_tables[k].clone();
                let t = retire_tx(&tk, now as u64, |_| {});
                if now >= tk.s.retire_from() {
                    must("retire", &t);
                    epoch_tables.swap_remove(k);
                    hit(n, "retire", true);
                } else {
                    assert!(!accepted(&t), "seed {seed}: an early retire");
                    hit(n, "retire", false);
                }
            }
            _ => {
                if epoch_tables.is_empty() {
                    continue;
                }
                let k = rng.below(epoch_tables.len() as u64) as usize;
                let tk = epoch_tables[k].clone();
                let t0 = tk.s.from - 3 + rng.below((tk.s.until - tk.s.from + 6) as u64) as i64;
                // Signed prices 0.4 % apart at most but for the last; the spread is 0.5 % of the price claimed.
                let prices = [996_000_000i64, 1_000_000_000, 1_004_000_000, 1_012_000_000];
                let claim = [996_000_000i64, 998_000_000, 1_000_000_000, 1_002_000_000, 1_004_000_000, 1_008_000_000, 1_012_000_000];
                let mut a = Attestation { market: MARKET, t0, p: rng.pick(&claim), bits: 0, prices: [0; MEMBERS], sigs: [[0; 64]; MEMBERS] };
                let share = 60 + rng.below(41);
                let odd = rng.below(4);
                for (i, s) in tk.s.members.iter().enumerate() {
                    if (s.taken() && rng.chance(share)) || rng.chance(2) {
                        let signer = kp_of(&s.key).unwrap_or(keypair(77));
                        let price = prices[if rng.chance(12) { rng.below(4) } else { rng.below(3).min(odd) } as usize];
                        a.bits |= 1 << i;
                        a.prices[i] = price;
                        a.sigs[i] = sign_price(&signer, &rid(), &MARKET, t0, price).sig;
                    }
                }
                let t = attest_tx(&tk, &a, now as u64, |_| {});
                if a.check(&tk.s, &rid(), now).is_ok() {
                    must_pinned("attest", &|tw: Tweak| attest_tx(&tk, &a, now as u64, |o| tw(o)));
                    epoch_tables[k] = live(&t, 0, now as u64, &tk.s);
                    hit(n, "attest", true);
                } else {
                    assert!(!accepted(&t), "seed {seed}: an attestation the model refuses");
                    hit(n, "attest", false);
                }
            }
        }
        // The committee holds its members' stake and what it began with; no stake appeared or vanished.
        assert_eq!(l.value as i64 - l.s.total(), base, "seed {seed}");
        assert_eq!(paid_in, l.s.total() + exits.iter().map(|e| e.s.amount).sum::<i64>() + withdrawn, "seed {seed}");
        for e in &exits {
            assert!(e.value as i64 >= e.s.amount);
        }
    }
}

fn report(what: &str, s: &Stats) {
    let line: Vec<String> = s.iter().map(|(k, (a, r))| format!("{k} {a}/{r}")).collect();
    println!("{what} (accepted/refused): {}", line.join("; "));
}

#[test]
fn c1_holds_the_committee_walk_agrees_with_the_model_and_conserves_the_stake() {
    let mut n = Stats::new();
    for seed in seeds() {
        committee_walk(seed, steps() * 2, &mut n);
    }
    report("committee walk", &n);
    for k in ["join", "join (outbidding)", "top-up", "leave", "roll", "withdraw", "retire", "attest"] {
        assert!(n.get(k).is_some_and(|v| v.0 > 0), "no accepted {k}");
    }
    for k in ["join (in the rest)", "top-up (in the rest)", "attest", "roll"] {
        assert!(n.get(k).is_some_and(|v| v.1 > 0), "no refused {k}");
    }
    assert!(n.get("roll (in the rest)").is_some_and(|v| v.0 > 0), "no roll inside the rest");
}

// ---------------------------------------------------------------- the market walk

#[derive(Clone)]
enum Obj {
    Pair(LPair),
    Pend(LPend),
}

fn key_kp(key: &[u8; 32]) -> Keypair {
    [long_kp(), short_kp(), exec_kp(), keypair(43), keypair(44)].into_iter().find(|k| x(k) == *key).expect("a key of the walk")
}

/// The walk's market: the fixture's with priceMax at 13 KAS, which the feed crosses.
fn wmarket() -> CMarket {
    CMarket { p: CParams { price_max: 1_300_000_000, ..cmarket().p }, ..cmarket() }
}

/// (long key, its value at the feed), (short key, the rest) of a pair of `value`.
fn pair_vals(p: &CPair, value: u64, f: i64) -> [([u8; 32], i64); 2] {
    let long = p.long_at(f);
    [(p.long_key, long), (p.short_key, value as i64 - long)]
}

/// A pending part valued as the pair it came from (the proposer's carrier is not a party's).
fn pend_vals(w: &CPending, value: u64, f: i64) -> [([u8; 32], i64); 2] {
    let long = q(w.n, f.clamp(w.bl, w.bs) - w.bl, w.m.p.scale, 0);
    [(w.long_key, long), (w.short_key, value as i64 - w.refund - long)]
}

fn add(map: &mut HashMap<[u8; 32], i64>, v: [([u8; 32], i64); 2]) {
    for (k, x) in v {
        *map.entry(k).or_insert(0) += x;
    }
}

fn paid(map: &mut HashMap<[u8; 32], i64>, t: &T) {
    for o in &t.0.outputs {
        let s = o.script_public_key.script();
        if o.covenant.is_none() && s.len() == 34 && s[0] == 0x20 && s[33] == 0xac && o.value > 1 {
            *map.entry(s[1..33].try_into().unwrap()).or_insert(0) += o.value as i64;
        }
    }
}

fn epoch_table_at(t0: i64) -> LEpochTable {
    let from = t0.div_euclid(1_000) * 1_000;
    epoch_table(&EpochTable { members: table(TOP3), total: 100 * KAS1, from, until: from + 1_000, quorum_bps: 6_700, spread_bps: 50 })
}

/// The three largest members sign `p` for the time `t0` of the walk's market.
fn att_w(t0: i64, p: i64) -> Attestation {
    let m = wmarket().id();
    let mut a = Attestation { market: m, t0, p, bits: 0, prices: [0; MEMBERS], sigs: [[0; 64]; MEMBERS] };
    for i in 0..3 {
        a.bits |= 1 << i;
        a.prices[i] = p;
        a.sigs[i] = sign_price(&member_kp(i), &rid(), &m, t0, p).sig;
    }
    a
}

/// The hand-over rule of `KpfPairC.handOver` for an entrant of `side` at `px` with the bound `nb`.
fn hand_over_ok(p: &CPair, side: i64, px: i64, nb: i64) -> bool {
    let (b, m) = (BPS as i128, p.m.p.mmr_bps as i128);
    if px < 0 || px > p.m.p.price_max {
        return false;
    }
    if side == LONG {
        nb >= 0 && nb <= p.bl && px as i128 * (b - m) >= nb as i128 * b
    } else {
        nb >= p.bs && nb <= p.m.p.price_max && nb as i128 * b >= px as i128 * (b + m)
    }
}

/// `KpfOpenC.take` for the order `o` accepted at `born`: the part (n, bl, bs, entry e, lease xe) at price px, the
/// transaction's lock time `lock`.
#[allow(clippy::too_many_arguments)]
fn take_ok(o: &COpen, born: u64, n: i64, px: i64, e: i64, bl: i64, bs: i64, xe: i64, lock: u64) -> bool {
    let (b, m) = (BPS as i128, o.m.p.mmr_bps as i128);
    let side_ok = if o.side == LONG { px <= o.limit && e >= px && bs - px >= o.min_counter_rate } else { px >= o.limit && e <= px && px - bl >= o.min_counter_rate };
    let healthy = px as i128 * (b - m) >= bl as i128 * b && bs as i128 * b >= px as i128 * (b + m) && (bl as i128) * (b + m) < (bs as i128) * (b - m);
    let lease = xe >= born as i64 + o.m.p.term / 2 && xe < LOCK_TIME_THRESHOLD && xe - o.m.p.term >= 0 && lock as i64 >= xe - o.m.p.term;
    o.part_ok(n) && side_ok && bl >= 0 && bl < bs && bs <= o.m.p.price_max && healthy && lease
}

fn new_order(m: &CMarket, side: i64, owner: &Keypair, serial: u8, born: u64, size: i64, limit: i64, my_rate: i64) -> LOrder {
    let o = COpen { m: *m, side, owner: x(owner), size_left: size, limit, my_rate, min_counter_rate: 0, min_fill: 1_000_000, tip: 0, active_from: 0, expiry_daa: 1_000_000_000 };
    Live { op: outpoint(200 + serial % 50), value: (o.spend(size) + KAS1) as u64, daa: born, id: Some(Hash::from_bytes([200 + serial % 50; 32])), s: o }
}

fn market_walk(seed: u64, steps: usize, n: &mut Stats) {
    let m = wmarket();
    let sc = m.p.scale;
    let me = x(&exec_kp());
    let mut rng = Rng::new(seed ^ 0xC1);
    let mut now: u64 = 16_100;
    let mut feed: i64 = 1_000_000_000;
    let mut objs: Vec<Obj> = vec![];
    let mut serial = 0u8;
    let k = kit();
    for _ in 0..steps {
        now += 1 + rng.below(60);
        if rng.chance(4) {
            now += 2_500;
        }
        feed = (feed + (rng.below(2_000_001) as i64 - 1_000_000) * 12).clamp(840_000_000, 1_400_000_000);
        if rng.chance(8) {
            // A jump: below the long's trigger, above the short's, above priceMax, or back to the middle.
            feed = rng.pick(&[915_000_000, 1_185_000_000, 1_360_000_000, 1_020_000_000, 1_000_000_000]);
        }
        serial = serial.wrapping_add(1);
        if objs.is_empty() || (objs.len() < 5 && rng.chance(12)) {
            if rng.chance(35) {
                let mut lp = pair(&CPair { expiry: now as i64 + 4_000 + rng.below(4_000) as i64, entry: feed.clamp(960_000_000, 1_130_000_000), ..sample_pair(&m) }, now);
                lp.op = outpoint(100 + serial % 100);
                lp.id = Some(Hash::from_bytes([100 + serial % 100; 32]));
                objs.push(Obj::Pair(lp));
                continue;
            }
            // An order taken with no pair: the taker (the executor's key) writes the pair alone.
            let side = rng.pick(&[LONG, SHORT]);
            let owner = keypair(rng.pick(&[43u8, 44]));
            let born = now - rng.below(6_000);
            let px = feed.min(1_200_000_000);
            let rate = rng.pick(&[100_000_000i64, 200_000_000, 60_000_000]);
            let lo = new_order(&m, side, &owner, serial, born, 1_000_000_000, px, rate);
            let health = if side == LONG { (px as i128 * 10_500 + 9_999) / 10_000 } else { px as i128 * 9_500 / 10_000 } as i64;
            let bound = match rng.below(10) {
                0 => px,
                1 => health + if side == LONG { -1 } else { 1 },
                2 | 3 => health,
                _ => health + if side == LONG { 1 } else { -1 } * rng.below(150_000_000) as i64,
            };
            let e = if rng.chance(8) { px + if side == LONG { -1 } else { 1 } } else { px };
            let xe = match rng.below(14) {
                0 => 0,
                1 => born as i64 + m.p.term / 2 - 1,
                2 | 3 => born as i64 + m.p.term / 2,
                4 => now as i64 + m.p.term + 1,
                5 => 499_999_999_999,
                6 | 7 => now as i64 + m.p.term,
                _ => now as i64 + m.p.term - rng.below(3_000) as i64,
            };
            let (bl, bs) = if side == LONG { (px - rate, bound) } else { (bound, px + rate) };
            let (lk, sk, mk) = if side == LONG { (x(&owner), me, Makers { long: maker_of(&lo.op), ..Makers::default() }) } else { (me, x(&owner), Makers { short: maker_of(&lo.op), ..Makers::default() }) };
            let part = CPair { m, size: lo.s.size_left, bl, bs, entry: e, long_key: lk, short_key: sk, expiry: xe, long_ask: 0, long_min: 0, long_tip: 0, short_bid: 0, short_min: 0, short_tip: 0, mk };
            let build = |tw: Tweak| {
                let mut outs = vec![out(lo.value - lo.s.spend(lo.s.size_left) as u64, key_spk(&lo.s.owner), None), out(part.floor().max(1) as u64, pair_spk(&part), None), change(1)];
                tw(&mut outs);
                seal(&mut outs, 1, 1, outpoint(60));
                let args = vec![iv(1), iv(lo.s.size_left), iv(px), iv(e), iv(bound), bv(&me), bv(&[0u8; 32]), iv(0), iv(0), iv(0), iv(xe), bv(&k.pair.prefix), bv(&k.pair.suffix)];
                build_ext(&[order_in(&lo, "take", args), exec_in(60)], outs, now, &[])
            };
            if take_ok(&lo.s, born, lo.s.size_left, px, e, bl, bs, xe, now) {
                let t = must_pinned("take without a pair", &build);
                objs.push(Obj::Pair(live(&t, 1, now, &part)));
                hit(n, "take (no pair)", true);
            } else {
                assert!(!accepted(&build(&|_| {})), "seed {seed}: a take without a pair the model refuses (side {side}, px {px}, e {e}, bound {bound}, xe {xe}, born {born}, now {now})");
                hit(n, "take (no pair)", false);
            }
            continue;
        }
        let i = rng.below(objs.len() as u64) as usize;
        let (mut before, mut after, mut cash): (HashMap<[u8; 32], i64>, HashMap<[u8; 32], i64>, HashMap<[u8; 32], i64>) = Default::default();
        // Keys that signed (or fund) this step, and what a passive key may lose in it.
        let mut signers: Vec<[u8; 32]> = vec![me];
        let mut allow: HashMap<[u8; 32], i64> = HashMap::new();
        let mut made: Vec<Obj> = vec![];
        let mut done = false;
        match objs[i].clone() {
            Obj::Pair(lp) => {
                let p = &lp.s;
                let age = now - lp.daa;
                let seq = age.min(30);
                add(&mut before, pair_vals(p, lp.value, feed));
                let side = if rng.chance(50) { LONG } else { SHORT };
                let (owner, other) = if side == LONG { (p.long_key, p.short_key) } else { (p.short_key, p.long_key) };
                let (listed, list_px, list_min) = if side == LONG { (p.long_ask > 0, p.long_ask, p.long_min) } else { (p.short_bid > 0, p.short_bid, p.short_min) };
                let part = match rng.below(4) {
                    0 => p.size / 2,
                    1 => (p.size / 4).max(1) + rng.below(1_000) as i64,
                    2 => 999_999,
                    _ => p.size,
                };
                let within = |px: i64| listed && if side == LONG { px >= list_px } else { px <= list_px };
                let mut act = rng.below(14);
                if listed && rng.chance(25) {
                    act = 4;
                }
                if p.long_ask > 0 && p.short_bid >= p.long_ask && rng.chance(40) {
                    act = 5;
                }
                match act {
                    0 => {
                        let signer = if rng.chance(85) { owner } else { other };
                        let ask = (feed + rng.pick(&[-30_000_000i64, 20_000_000]) * if side == LONG { 1 } else { -1 }).min(m.p.price_max);
                        let build = |tw: Tweak| amend_tx(&lp, side, ask, 1_000_000, 0, &key_kp(&signer), seq, |o| tw(o));
                        if signer == owner && age >= 20 {
                            let t = must_pinned("amend", &build);
                            let mut next = p.rest_of(p.size);
                            if side == LONG {
                                (next.long_ask, next.long_min, next.long_tip) = (ask, 1_000_000, 0);
                            } else {
                                (next.short_bid, next.short_min, next.short_tip) = (ask, 1_000_000, 0);
                            }
                            signers.push(owner);
                            made.push(Obj::Pair(live(&t, 0, now, &next)));
                            done = true;
                        } else {
                            assert!(!accepted(&build(&|_| {})), "seed {seed}: an amend by another key or too early");
                        }
                        hit(n, "amend", done);
                    }
                    1 => {
                        let delta = 1 + rng.below(60_000_000) as i64;
                        let build = |tw: Tweak| add_margin_tx(&lp, side, delta, &key_kp(&owner), seq, |o| tw(o));
                        let fits = if side == LONG { p.bl - delta >= 0 } else { p.bs + delta <= m.p.price_max };
                        if age >= 20 && fits {
                            let t = must_pinned("addMargin", &build);
                            let mut next = p.rest_of(p.size);
                            if side == LONG {
                                next.bl -= delta;
                            } else {
                                next.bs += delta;
                            }
                            signers.push(owner);
                            made.push(Obj::Pair(live(&t, 0, now, &next)));
                            done = true;
                        } else {
                            assert!(!accepted(&build(&|_| {})), "seed {seed}: a top-up of margin too early or beyond priceMax");
                        }
                        hit(n, "addMargin", done);
                    }
                    2 | 3 => {
                        // A hand-over to the executor's key: an assign (the leaver signs) or a transfer (a listed side).
                        let listing = if listed { rng.chance(70) } else { rng.chance(10) };
                        let px = if listing && listed { list_px } else { feed };
                        let nb = if side == LONG { p.bl - rng.below(3) as i64 * 30_000_000 + rng.below(2) as i64 } else { p.bs + rng.below(3) as i64 * 30_000_000 - rng.below(2) as i64 };
                        let ok = age >= 20 && p.split_ok(part) && hand_over_ok(p, side, px, nb) && (!listing || (within(px) && (part >= list_min || part == p.size)));
                        let build = |tw: Tweak| {
                            if listing {
                                transfer_tx(&lp, side, part, px, nb, seq, |o| tw(o))
                            } else {
                                assign_tx(&lp, side, part, px, nb, &key_kp(&owner), seq, |o| tw(o))
                            }
                        };
                        if ok {
                            let t = must_pinned("hand-over", &build);
                            let (_, written, _, _) = handed(&lp, side, part, px, nb, me, [0; 32], 0);
                            signers.push(owner);
                            made.push(Obj::Pair(live(&t, 1, now, &written)));
                            if part < p.size {
                                made.push(Obj::Pair(live(&t, 2, now, &p.rest_of(p.size - part))));
                            }
                            done = true;
                        } else {
                            assert!(!accepted(&build(&|_| {})), "seed {seed}: a hand-over the model refuses (listing {listing}, part {part}, px {px}, nb {nb}, age {age})");
                        }
                        hit(n, if listing { "transfer" } else { "assign" }, done);
                    }
                    4 => {
                        // An order takes the listed side: the order is written for the listing, with an age of its own.
                        if !listed {
                            continue;
                        }
                        let px = list_px;
                        let taker = keypair(rng.pick(&[43u8, 44]));
                        let rate = if side == LONG { px - p.bl } else { p.bs - px } + rng.pick(&[0i64, 0, 30_000_000, -1]);
                        if rate <= 0 {
                            continue;
                        }
                        let born = now - rng.below(7_000);
                        let lo = new_order(&m, side, &taker, serial, born, part + rng.pick(&[0i64, 5_000_000]), px, rate);
                        let nb = if side == LONG { px - rate } else { px + rate };
                        let (tbl, tbs) = if side == LONG { (nb, p.bs) } else { (p.bl, nb) };
                        let ok = age >= 20
                            && p.split_ok(part)
                            && hand_over_ok(p, side, px, nb)
                            && (part >= list_min || part == p.size)
                            && take_ok(&lo.s, born, part, px, p.entry, tbl, tbs, p.expiry, now);
                        let build = |tw: Tweak| take_tx(&lp, &lo, part, px, now, seq, |o| tw(o));
                        if ok {
                            let t = must_pinned("take of a listed side", &build);
                            let (_, written, _, _) = handed(&lp, side, part, px, nb, lo.s.owner, maker_of(&lo.op), 0);
                            signers.extend([owner, lo.s.owner]);
                            made.push(Obj::Pair(live(&t, 2, now, &written)));
                            if part < p.size {
                                made.push(Obj::Pair(live(&t, 3, now, &p.rest_of(p.size - part))));
                            }
                            done = true;
                        } else {
                            assert!(!accepted(&build(&|_| {})), "seed {seed}: a take the model refuses (side {side}, part {part}, px {px}, rate {rate}, entry {}, lease {}, born {born}, now {now})", p.entry, p.expiry);
                        }
                        hit(n, "take (listed side)", done);
                    }
                    5 => {
                        // Mutual close of the whole pair where the two listings cross.
                        let px = if p.long_ask > 0 && p.short_bid >= p.long_ask { (p.long_ask + p.short_bid) / 2 } else { feed.min(m.p.price_max) };
                        let lpay = owed(p.size, px - p.bl, p.long_tip, sc);
                        if lpay > 0 && lpay < 2_000_000 {
                            continue;
                        }
                        let ok = age >= 20 && p.long_ask > 0 && p.short_bid > 0 && px >= p.long_ask && px <= p.short_bid && px <= m.p.price_max;
                        let build = |tw: Tweak| mutual_tx(&lp, px, seq, |o| tw(o));
                        if ok {
                            let t = must_pinned("mutualClose", &build);
                            paid(&mut cash, &t);
                            // Both listed: each is paid at a price inside its own listing, the short the rest.
                            if p.long_key != p.short_key && p.long_key != me && p.short_key != me {
                                assert_eq!(cash.get(&p.long_key).copied().unwrap_or(0), lpay, "seed {seed}: the long's payout of a mutual close");
                                // (the rounding of the two shares, a sompi at most, is the builder's)
                                let spay = owed(p.size, p.bs - px, p.short_tip, sc) + lp.value as i64 - q(p.size, p.bs - p.bl, sc, 0);
                                assert!(lp.value as i64 - lpay - spay <= 1);
                                assert_eq!(cash.get(&p.short_key).copied().unwrap_or(0), spay, "seed {seed}: the short's payout of a mutual close");
                            }
                            signers.extend([p.long_key, p.short_key]);
                            done = true;
                        } else {
                            assert!(!accepted(&build(&|_| {})), "seed {seed}: a mutual close the model refuses");
                        }
                        hit(n, "mutualClose", done);
                    }
                    6 => {
                        // Cooperative close: both keys, any split; one key twice is refused unless it holds both sides.
                        if rng.chance(70) {
                            continue;
                        }
                        let both = rng.chance(60);
                        let (a, b) = if both { (key_kp(&p.long_key), key_kp(&p.short_key)) } else { (key_kp(&p.long_key), key_kp(&p.long_key)) };
                        let t = coop_tx(&lp, &a, &b, |_| {});
                        if both || p.long_key == p.short_key {
                            must("coopClose", &t);
                            signers.extend([p.long_key, p.short_key]);
                            done = true;
                        } else {
                            assert!(!accepted(&t), "seed {seed}: a cooperative close by one key");
                        }
                        hit(n, "coopClose", done);
                    }
                    7 => {
                        let t0 = now as i64 - 1;
                        let lt = epoch_table_at(t0);
                        let a = att_w(t0, feed);
                        let build = |tw: Tweak| keep_alive_tx(&lp, &lt, &a, now, |o| tw(o));
                        match p.kept_alive(t0) {
                            Some(next) => {
                                let t = must_pinned("keepAlive", &build);
                                made.push(Obj::Pair(live(&t, 0, now, &next)));
                                done = true;
                            }
                            None => assert!(!accepted(&build(&|_| {})), "seed {seed}: a renewal that does not move the lease by a quarter term"),
                        }
                        hit(n, "keepAlive", done);
                    }
                    8 => {
                        let build = |tw: Tweak| lapse_tx(&lp, now, |o| tw(o));
                        let (long, short) = p.lapse_out(lp.value);
                        if p.expiry + m.p.idle_daa >= LOCK_TIME_THRESHOLD {
                            continue;
                        }
                        if now >= p.lapse_from() {
                            let t = must_pinned("lapseSettle", &build);
                            paid(&mut cash, &t);
                            if p.long_key != p.short_key && p.long_key != me && p.short_key != me {
                                assert_eq!((cash.get(&p.long_key).copied().unwrap_or(0), cash.get(&p.short_key).copied().unwrap_or(0)), (long, short), "seed {seed}: the lapse payouts");
                            }
                            // The lapse is a listed cause: a side may lose its value at the feed less its value at the entry.
                            signers.extend([p.long_key, p.short_key]);
                            done = true;
                        } else {
                            assert!(!accepted(&build(&|_| {})), "seed {seed}: an early lapse settle");
                        }
                        hit(n, "lapseSettle", done);
                    }
                    9 | 10 => {
                        // A close by `side`: a fresh signed price at the feed (which may lie above priceMax), or one 150 DAA old.
                        let stale = rng.chance(30);
                        let t0 = now as i64 - if stale { 150 } else { 1 };
                        let lt = epoch_table_at(t0);
                        let pr = Propose { c: Claim { kind: KIND_CLOSE, side, n: part, p: feed, t0 }, a: att_w(t0, feed), signer: Some(key_kp(&owner)), proposer: owner, lock: now, seq };
                        let build = |tw: Tweak| propose_tx(&lp, &lt, &pr, |o| tw(o));
                        match p.pending_of(kit(), part, KIND_CLOSE, side, feed, t0, owner).filter(|_| age >= 20 && p.price_fresh(t0, lp.daa)) {
                            Some((w, _)) => {
                                let t = must_pinned("propose close", &build);
                                assert_eq!(w.quote, feed.min(m.p.price_max));
                                signers.push(owner);
                                made.push(Obj::Pend(live(&t, 0, now, &w.made_by(&lp.op))));
                                if part < p.size {
                                    made.push(Obj::Pair(live(&t, 2, now, &p.rest_of(p.size - part))));
                                }
                                done = true;
                            }
                            None => assert!(!accepted(&build(&|_| {})), "seed {seed}: a close the model refuses"),
                        }
                        hit(n, if stale { "close (stale signed price)" } else { "close" }, done);
                    }
                    _ => {
                        // A liquidation of `side`: a fresh signed price at the feed, or a stale one at a price that triggers.
                        let stale = rng.chance(35);
                        let t0 = now as i64 - if stale { 150 } else { 1 };
                        let price = if !stale { feed } else if side == LONG { p.bl * 9 / 10 + 1 } else { p.bs * 11 / 10 };
                        let lt = epoch_table_at(t0);
                        let pr = Propose { c: Claim { kind: KIND_LIQ, side, n: part, p: price, t0 }, a: att_w(t0, price), signer: None, proposer: me, lock: now, seq: 0 };
                        let build = |tw: Tweak| propose_tx(&lp, &lt, &pr, |o| tw(o));
                        match p.pending_of(kit(), part, KIND_LIQ, side, price, t0, me).filter(|_| p.price_fresh(t0, lp.daa)) {
                            Some((w, _)) => {
                                let t = must_pinned("propose liquidation", &build);
                                made.push(Obj::Pend(live(&t, 0, now, &w.made_by(&lp.op))));
                                if part < p.size {
                                    made.push(Obj::Pair(live(&t, 2, now, &p.rest_of(p.size - part))));
                                }
                                done = true;
                            }
                            None => assert!(!accepted(&build(&|_| {})), "seed {seed}: a liquidation the model refuses (side {side}, price {price}, t0 {t0}, born {})", lp.daa),
                        }
                        hit(n, if stale { "liquidation (stale signed price)" } else { "liquidation" }, done);
                    }
                }
            }
            Obj::Pend(lw) => {
                let w = &lw.s;
                let age = now - lw.daa;
                add(&mut before, pend_vals(w, lw.value, feed));
                let loser = if w.side == LONG { w.long_key } else { w.short_key };
                if w.kind == KIND_LIQ && rng.chance(30) {
                    let build = |tw: Tweak| cure_tx(&lw, &key_kp(&loser), |o| tw(o));
                    match w.cure(lw.value) {
                        Some((cured, _)) => {
                            let t = must_pinned("cure", &build);
                            signers.push(loser);
                            made.push(Obj::Pair(live(&t, 1, now, &cured)));
                            done = true;
                        }
                        None => assert!(!accepted(&build(&|_| {})), "seed {seed}: a cure where none is offered"),
                    }
                    hit(n, "cure", done);
                } else {
                    let seq = age.min(60);
                    let build = |tw: Tweak| finalize_tx(&lw, seq, |o| tw(o));
                    if age >= w.finalize_age(lw.daa as i64) {
                        let t = must_pinned("finalize", &build);
                        paid(&mut cash, &t);
                        let drift = q(w.n, (w.quote - feed.min(m.p.price_max)).abs(), sc, sc - 1);
                        match w.settlement(lw.value, lw.daa as i64) {
                            CSettlement::Close { .. } => {
                                // The closer signed the claim; the other party is paid at the attested price.
                                signers.push(w.proposer);
                                let other = if w.side == LONG { w.short_key } else { w.long_key };
                                allow.insert(other, drift);
                                hit(n, "settle: close", true);
                            }
                            CSettlement::Forfeit { part, value, fee } => {
                                // A true liquidation: the loser forfeits its side; the winner bears at most the fee.
                                assert!(if w.side == LONG { w.pair_part().long_liquidatable(w.quote) } else { w.pair_part().short_liquidatable(w.quote) });
                                let winner = part.long_key;
                                signers.push(loser);
                                allow.insert(winner, fee + drift);
                                made.push(Obj::Pair(live(&t, 1, now, &part)));
                                assert_eq!(t.0.outputs[1].value as i64, value);
                                hit(n, "settle: forfeit", true);
                            }
                            CSettlement::Restore { part, .. } => {
                                // A void claim: restored at once, with the listings the pair had.
                                assert_eq!((part.long_ask, part.short_bid), (w.long_ask, w.short_bid));
                                made.push(Obj::Pair(live(&t, 0, now, &part)));
                                hit(n, if age < m.p.grace_daa as u64 && w.kind == KIND_LIQ { "settle: restore (inside the grace)" } else { "settle: restore" }, true);
                            }
                            // The walk proposes closes and liquidations only (a trim, kind 3, came after it).
                            CSettlement::Trim { .. } => unreachable!("the walk proposes no trim"),
                        }
                        done = true;
                    } else {
                        assert!(!accepted(&build(&|_| {})), "seed {seed}: a valid liquidation finalized before the grace");
                        hit(n, "settle: forfeit", false);
                    }
                }
            }
        }
        if !done {
            continue;
        }
        for o in &made {
            match o {
                Obj::Pair(l) => add(&mut after, pair_vals(&l.s, l.value, feed)),
                Obj::Pend(l) => add(&mut after, pend_vals(&l.s, l.value, feed)),
            }
        }
        // Nobody loses value at the feed in a step it did not sign, beyond what the step's rule allows and the
        // rounding of a split (a few sompi).
        for (key, b) in &before {
            if signers.contains(key) {
                continue;
            }
            let a = after.get(key).copied().unwrap_or(0) + cash.get(key).copied().unwrap_or(0);
            assert!(a + allow.get(key).copied().unwrap_or(0) + 8 >= *b, "seed {seed} at {now}: a passive party's value at the feed fell from {b} to {a}");
        }
        objs.swap_remove(i);
        objs.extend(made);
    }
}

#[test]
fn c1_holds_the_market_walk_keeps_every_passive_partys_value_at_the_feed() {
    let mut n = Stats::new();
    for seed in seeds() {
        market_walk(seed, steps(), &mut n);
    }
    report("market walk", &n);
    for key in ["take (no pair)", "amend", "addMargin", "assign", "transfer", "keepAlive", "lapseSettle", "close", "close (stale signed price)", "liquidation (stale signed price)", "settle: close", "settle: restore"] {
        assert!(n.get(key).is_some_and(|v| v.0 > 0), "no accepted {key}");
    }
    for key in ["take (no pair)", "close (stale signed price)", "liquidation (stale signed price)", "assign"] {
        assert!(n.get(key).is_some_and(|v| v.1 > 0), "no refused {key}");
    }
}
