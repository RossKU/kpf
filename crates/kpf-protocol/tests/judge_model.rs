//! Property tests of the stake judge (`kpf_protocol::judge`; docs/DESIGN-ORACLE-IMPL.md, second slice), played on the
//! model's whole transactions: every step is a `JTx` that `verify` accepts or refuses input by input, exactly as the
//! contracts do (the engine replay holds the two together).
//!
//! - `judge_games_conserve_terminate_and_never_take_more_than_was_staked`: seeded random games (many stakers, uneven
//!   stakes with sub-unit remainders, both sides, unaged and leader stake that must be refused, early resolutions that
//!   must be refused, answers in the last DAA of a window, settlement in random order and batches). On every game:
//!   winners receive exactly what losers forfeited less the burn and the stated rounding; the game ends; no staker
//!   loses more than it staked; no winning receipt is paid less than it staked; the claim gets the leader's verdict.
//! - adversarial strategies: a late flood, splitting a stake, staking both sides, re-opening a decided claim.
//! - Z10: the measured threshold `f*` of an attacker holding a share `f` of the aged stake against honest stakers of
//!   whom a fraction `p_h` take part, against the note's `f* = p_h / (p_h + mu)`; and the drill of 100 parallel
//!   disputes with half participation and a patient attacker.

use std::collections::BTreeMap;

use kpf_protocol::judge::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
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
}

fn params() -> JudgeParams {
    JudgeParams { mu_bps: 12_500, age: 500, unbond: 300, round: 100, burn_bps: 1_000, min_stake: UNIT }
}

fn market(p: JudgeParams) -> JudgeMarket {
    JudgeMarket { id: [0x4a; 32], p }
}

const ENV: FakeEnv = FakeEnv;
/// The claim's DAA (the anchor): stakes accepted at or before `ANCHOR - age` count.
const ANCHOR: i64 = 10_000;
const FUNDER: u8 = 250;

/// One dispute on one stand-in claim, played on the model.
struct Table {
    a: Arena,
    claim: u64,
    dispute: Option<u64>,
    receipts: Vec<u64>,
    /// DAA of the last accepted transaction (time only moves forward).
    now: i64,
    rounds: i64,
}

impl Table {
    fn new(p: JudgeParams) -> Table {
        let mut a = Arena::new(market(p), FUNDER);
        let claim = a.claim([0xc1; 32], 9, ANCHOR);
        Table { a, claim, dispute: None, receipts: vec![], now: ANCHOR, rounds: 0 }
    }
    fn p(&self) -> JudgeParams {
        self.a.m.p
    }
    fn d(&self) -> Js {
        self.a.js(self.dispute.expect("open"))
    }
    /// Runs `tx` at DAA `daa`: applies it if every input accepts.
    fn run(&mut self, tx: &JTx, daa: i64) -> Option<Vec<u64>> {
        match accepted(tx, &ENV) {
            Ok(()) => {
                assert!(daa >= self.now);
                self.now = daa;
                Some(self.a.apply(tx, daa))
            }
            Err(_) => None,
        }
    }
    fn open(&mut self, stake: u64, seed: u8, daa: i64) -> bool {
        let tx = self.a.open_tx(self.claim, stake, seed, 50_000_000, &ENV);
        match self.run(&tx, daa) {
            Some(o) => {
                self.claim = o[0];
                self.receipts.push(o[1]);
                self.dispute = Some(o[2]);
                true
            }
            None => false,
        }
    }
    fn deposit(&mut self, side: i64, stakes: &[(u64, u8)], daa: i64) -> bool {
        let before = self.d().side;
        let tx = self.a.deposit_tx(self.dispute.unwrap(), side, stakes, SIGHASH_SINGLE_ACP);
        match self.run(&tx, daa) {
            Some(o) => {
                self.dispute = Some(o[0]);
                self.receipts.extend(&o[1..=stakes.len()]);
                if self.d().side != before {
                    self.rounds += 1;
                }
                true
            }
            None => false,
        }
    }
    /// The round's start and the earliest DAA a resolution can land.
    fn window_end(&self) -> i64 {
        let d = self.d();
        let h = self.dispute.unwrap();
        let st = if d.start == -1 { self.a.w.utxos[&h].daa } else { d.start };
        st + self.p().round
    }
    fn resolve(&mut self, lock: i64) -> bool {
        let tx = self.a.resolve_tx(self.dispute.unwrap(), self.claim, lock);
        let daa = lock.max(self.now);
        match self.run(&tx, daa) {
            Some(o) => {
                self.dispute = Some(o[0]);
                self.claim = o[1];
                true
            }
            None => false,
        }
    }
}

/// A staker's UTXO in a game.
#[derive(Clone, Copy, Debug)]
struct Piece {
    h: u64,
    seed: u8,
    v: i64,
    aged: bool,
}

/// Settles every receipt of a resolved table in random batches and order, then closes the pool. Returns (payout per
/// receipt handle, burn output value, value the executor kept at close beyond the base).
fn settle_all(t: &mut Table, rng: &mut Rng) -> (BTreeMap<u64, i64>, i64, i64) {
    let mut left: Vec<u64> = t.receipts.clone();
    let mut paid: BTreeMap<u64, i64> = BTreeMap::new();
    let mut guard = 0;
    while !left.is_empty() {
        guard += 1;
        assert!(guard < 10_000, "settlement does not terminate");
        let pool = t.dispute.unwrap();
        let k = (rng.range(1, BATCH) as usize).min(left.len());
        // A random batch; after a refusal (the pool cannot pay yet), losers first.
        let w = t.a.js(pool).side;
        let mut batch: Vec<u64> = vec![];
        let pick_losers = guard % 3 == 0;
        for _ in 0..k {
            let cand: Vec<usize> = (0..left.len()).filter(|i| !batch.contains(&left[*i]) && (!pick_losers || t.a.js(left[*i]).side != w)).collect();
            if cand.is_empty() {
                break;
            }
            batch.push(left[cand[rng.below(cand.len() as u64) as usize]]);
        }
        if batch.is_empty() {
            batch.push(left[0]);
        }
        let vals: Vec<(u64, i64, Js)> = batch.iter().map(|h| (*h, t.a.w.utxos[h].out.value, t.a.js(*h))).collect();
        let tx = t.a.settle_tx(pool, &batch);
        let daa = t.now + 1;
        if let Some(o) = t.run(&tx, daa) {
            t.dispute = Some(o[0]);
            // Winners first, at outputs 1.., in the builder's order.
            let mut j = 1;
            for h in tx.ins[1..tx.ins.len() - 1].iter().map(|i| i.id) {
                let (_, _, js) = vals.iter().find(|x| x.0 == h).unwrap().clone();
                if js.side == w {
                    paid.insert(h, tx.outs[j].value);
                    j += 1;
                } else {
                    paid.insert(h, 0);
                }
            }
            left.retain(|h| !batch.contains(h));
        }
    }
    let pool = t.dispute.unwrap();
    let base = t.a.js(pool).base;
    let pv = t.a.w.utxos[&pool].out.value;
    let tx = t.a.close_tx(pool);
    let daa = t.now + 1;
    assert!(t.run(&tx, daa).is_some(), "close after every receipt: {:?}", verify(&tx, &ENV));
    let burn = tx.outs.iter().filter(|o| o.script == Script::Burn).map(|o| o.value).sum::<i64>();
    (paid, burn, pv - base - burn)
}

/// One random game; returns (rounds, receipts).
fn random_game(seed: u64) -> (i64, usize) {
    let mut rng = Rng(seed);
    let mut p = params();
    p.mu_bps = [11_000, 12_500, 15_000][rng.below(3) as usize];
    p.burn_bps = [0, 1_000, 5_000][rng.below(3) as usize];
    let mut t = Table::new(p);
    let n = rng.range(2, 40) as usize;
    let mut pieces: Vec<Piece> = vec![];
    for i in 0..n {
        let seed = 10 + (i as u8);
        for _ in 0..rng.range(1, 3) {
            let v = rng.range(UNIT, 60 * UNIT) + if rng.chance(50) { rng.range(0, UNIT - 1) } else { 0 };
            let aged = !rng.chance(10);
            let daa = if aged { ANCHOR - p.age - rng.range(0, 1_000) } else { ANCHOR - p.age + rng.range(1, 400) };
            let h = t.a.stake(seed, v, daa);
            pieces.push(Piece { h, seed, v, aged });
        }
    }
    // Each staker leans to a side; some take both.
    let lean: Vec<i64> = (0..n).map(|_| if rng.chance(50) { YES } else { NO }).collect();
    let both: Vec<bool> = (0..n).map(|_| rng.chance(15)).collect();
    let side_ok = |pc: &Piece, s: i64| -> bool {
        let i = (pc.seed - 10) as usize;
        both[i] || lean[i] == s
    };
    // Open with an aged NO piece (an unaged one first, which must be refused).
    if let Some(u) = pieces.iter().find(|pc| !pc.aged && pc.v >= p.min_stake) {
        assert!(!t.open(u.h, u.seed, ANCHOR + 1), "an unaged stake opened a dispute");
    }
    let opener = match pieces.iter().position(|pc| pc.aged) {
        Some(i) => pieces.remove(i),
        None => return (0, 0),
    };
    assert!(t.open(opener.h, opener.seed, ANCHOR + 1));
    let mut staked: BTreeMap<u64, (i64, i64)> = BTreeMap::new(); // receipt -> (side, value)
    staked.insert(t.receipts[0], (NO, opener.v));
    let mut guard = 0;
    loop {
        guard += 1;
        assert!(guard < 5_000, "the game does not end");
        let d = t.d();
        let trailing = other(d.side);
        let end = t.window_end();
        // Hostile probes, all refused.
        if rng.chance(20) {
            if let Some(pc) = pieces.iter().find(|pc| pc.aged) {
                let tx = t.a.deposit_tx(t.dispute.unwrap(), d.side, &[(pc.h, pc.seed)], SIGHASH_SINGLE_ACP);
                assert!(accepted(&tx, &ENV).is_err(), "the leader added stake");
            }
        }
        if rng.chance(20) {
            if let Some(pc) = pieces.iter().find(|pc| !pc.aged) {
                let tx = t.a.deposit_tx(t.dispute.unwrap(), trailing, &[(pc.h, pc.seed)], SIGHASH_SINGLE_ACP);
                assert!(accepted(&tx, &ENV).is_err(), "an unaged stake counted");
            }
        }
        if rng.chance(30) {
            let tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end - 1);
            assert!(accepted(&tx, &ENV).is_err(), "resolved one DAA early");
        }
        // The trailing side answers in batches, or gives up.
        let avail: Vec<Piece> = pieces.iter().copied().filter(|pc| pc.aged && side_ok(pc, trailing)).collect();
        if avail.is_empty() || rng.chance(12) {
            break;
        }
        let k = (rng.range(1, BATCH) as usize).min(avail.len());
        let batch: Vec<Piece> = avail[..k].to_vec();
        // Sometimes in the last DAA of the window.
        let daa = if rng.chance(30) { (end - 1).max(t.now) } else { (t.now + rng.range(0, 20)).min(end - 1).max(t.now) };
        let before = t.receipts.len();
        let ok = t.deposit(trailing, &batch.iter().map(|pc| (pc.h, pc.seed)).collect::<Vec<_>>(), daa);
        assert!(ok, "an aged trailing deposit refused: {:?}", verify(&t.a.deposit_tx(t.dispute.unwrap(), trailing, &[(batch[0].h, batch[0].seed)], SIGHASH_SINGLE_ACP), &ENV));
        for (j, pc) in batch.iter().enumerate() {
            staked.insert(t.receipts[before + j], (trailing, pc.v));
            pieces.retain(|x| x.h != pc.h);
        }
    }
    // Resolve: early refused, at the window end accepted, twice refused.
    let end = t.window_end();
    assert!(!t.resolve(end - 1), "resolved one DAA early");
    let leader = t.d().side;
    let d = t.d();
    let old_dispute = t.dispute.unwrap();
    assert!(t.resolve(end), "resolution at the window's end");
    let again = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end + 10);
    assert!(accepted(&again, &ENV).is_err(), "resolved twice");
    assert!(!t.a.w.utxos.contains_key(&old_dispute));
    match &t.a.w.utxos[&t.claim].out.script {
        Script::Claim(c) => assert_eq!(c.status, if leader == YES { CLAIM_UPHELD } else { CLAIM_REFUTED }, "the claim gets the leader's verdict"),
        _ => panic!("claim"),
    }
    // Settle and check the accounting.
    let (paid, burn, kept) = settle_all(&mut t, &mut rng);
    let w = leader;
    let l = d.total(other(w));
    let lnet = l - p.burn(l).unwrap();
    let mut gain = 0i64;
    let mut winners = 0i64;
    for (h, (side, v)) in &staked {
        assert!(!t.a.w.utxos.contains_key(h), "a receipt left unsettled");
        let pay = paid[h];
        // A staker's only contribution is its receipt: it loses at most that, and a winner never less than it.
        assert!(pay >= 0);
        if *side == w {
            assert!(pay >= *v, "a winning receipt paid less than it staked");
            gain += pay - v;
            winners += 1;
        } else {
            assert_eq!(pay, 0, "a losing receipt paid");
        }
    }
    // Conservation: winners' gain + burn + what the executor kept == the losers' forfeit, exactly.
    assert_eq!(gain + burn + kept, l, "stake conserved (L {l}, gain {gain}, burn {burn}, kept {kept})");
    let dust = lnet - gain;
    assert!(dust >= 0 && dust < winners.max(1), "rounding dust {dust} below one sompi per winner");
    assert_eq!(burn + kept, p.burn(l).unwrap() + dust, "the burn is phi of the losers' stake plus the rounding");
    assert!(kept == 0 || (burn == 0 && kept < BURN_DUST), "the executor keeps a burn only below dust");
    assert!(t.rounds <= 230, "rounds bounded");
    (t.rounds, staked.len())
}

#[test]
fn judge_games_conserve_terminate_and_never_take_more_than_was_staked() {
    let mut total_rounds = 0;
    let mut total_receipts = 0;
    let mut most = 0;
    for seed in 0..300u64 {
        let (r, n) = random_game(0x5eed_0000 + seed);
        total_rounds += r;
        total_receipts += n;
        most = most.max(r);
    }
    println!("300 games: {total_receipts} receipts, {total_rounds} lead changes (most in one game {most})");
    assert!(total_receipts > 1_000);
}

// ---------------------------------------------------------------- adversarial strategies

/// Aged stake pieces for one key: `n` UTXOs of `v`.
fn pieces(t: &mut Table, seed: u8, n: usize, v: i64) -> Vec<(u64, u8)> {
    let daa = ANCHOR - t.p().age;
    (0..n).map(|_| (t.a.stake(seed, v, daa), seed)).collect()
}

#[test]
fn a_late_flood_gives_the_other_side_a_full_window_and_the_leader_cannot_raise_the_bar() {
    // Early answer and last-DAA answer reach the same outcome; neither lets the answered side lose by timing.
    for late in [false, true] {
        let mut t = Table::new(params());
        let o = pieces(&mut t, 11, 1, 10 * UNIT)[0];
        assert!(t.open(o.0, o.1, ANCHOR));
        let end = t.window_end();
        let yes = pieces(&mut t, 12, 2, 7 * UNIT);
        let daa = if late { end - 1 } else { ANCHOR + 1 };
        assert!(t.deposit(YES, &yes, daa), "YES answers 14 >= ceil(1.25 x 10) = 13 units");
        assert_eq!(t.d().side, YES);
        let end2 = t.window_end();
        assert_eq!(end2, daa + t.p().round, "the new round's window runs from the answer's own DAA");
        assert!(!t.resolve(end2 - 1), "NO keeps a full window after a last-DAA answer");
        // The leader (YES) cannot add more stake to raise NO's bar at the last moment.
        let more = pieces(&mut t, 13, 1, 50 * UNIT);
        let tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &more, SIGHASH_SINGLE_ACP);
        assert!(accepted(&tx, &ENV).is_err());
        // NO answers in the last DAA of its window one sompi short: it needs ceil(1.25 x 14) = 17.5 units in all,
        // and holds 10.
        let n = t.p().need(14 * UNIT).unwrap() - 10 * UNIT - 1;
        let short = pieces(&mut t, 14, 1, n);
        assert!(t.deposit(NO, &short, end2 - 1), "a partial answer is accepted, the lead stays");
        assert_eq!(t.d().side, YES);
        assert_eq!(t.window_end(), end2, "a partial answer does not move the window");
        assert!(t.resolve(end2));
        assert_eq!(t.a.js(t.dispute.unwrap()).side, YES);
    }
}

#[test]
fn one_sompi_under_the_lead_requirement_keeps_the_lead() {
    let mut t = Table::new(params());
    let o = pieces(&mut t, 11, 1, 10 * UNIT + 3)[0];
    assert!(t.open(o.0, o.1, ANCHOR));
    let need = t.p().need(10 * UNIT + 3).unwrap();
    assert_eq!(need, (10 * UNIT + 3) * 5 / 4 + 1);
    let a = pieces(&mut t, 12, 1, need - 1);
    assert!(t.deposit(YES, &a, ANCHOR + 1));
    assert_eq!(t.d().side, NO, "one sompi under keeps the lead");
    let b = pieces(&mut t, 13, 1, UNIT);
    assert!(t.deposit(YES, &b, ANCHOR + 2));
    assert_eq!(t.d().side, YES, "the sum of a side's deposits counts");
}

#[test]
fn splitting_a_stake_changes_nothing_but_unit_rounding() {
    // One opener of 100 units against YES stakers: one piece of 130 units, or 7 + 7 pieces of 9.3 units.
    let mut outcomes = vec![];
    for split in [false, true] {
        let mut t = Table::new(params());
        let o = pieces(&mut t, 11, 1, 100 * UNIT)[0];
        assert!(t.open(o.0, o.1, ANCHOR));
        if split {
            let a = pieces(&mut t, 12, 7, 93 * UNIT / 10);
            let b = pieces(&mut t, 12, 7, 93 * UNIT / 10);
            assert!(t.deposit(YES, &a, ANCHOR + 1));
            assert_eq!(t.d().side, NO);
            assert!(t.deposit(YES, &b, ANCHOR + 2));
        } else {
            let a = pieces(&mut t, 12, 1, 1302 * UNIT / 10);
            assert!(t.deposit(YES, &a, ANCHOR + 1));
        }
        assert_eq!(t.d().side, YES, "the same total takes the lead");
        let end = t.window_end();
        assert!(t.resolve(end));
        let mut rng = Rng(7);
        let (paid, _, _) = settle_all(&mut t, &mut rng);
        let total: i64 = paid.values().sum();
        outcomes.push(total);
    }
    // 130.2 units in one piece weigh 130; split, 14 x 9 = 126 of 130.2: the winner's share is the whole forfeit
    // either way (it is the only winner), so only the rounding dust can differ.
    assert!((outcomes[0] - outcomes[1]).abs() < 10, "{outcomes:?}");
}

#[test]
fn staking_both_sides_never_profits_and_never_loses_more_than_the_losing_stake() {
    // Key 11 opens NO with 10 units, then answers itself on YES with 13; nobody else comes; YES wins.
    let mut t = Table::new(params());
    let o = pieces(&mut t, 11, 1, 10 * UNIT)[0];
    assert!(t.open(o.0, o.1, ANCHOR));
    let y = pieces(&mut t, 11, 1, 13 * UNIT);
    assert!(t.deposit(YES, &y, ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    let mut rng = Rng(1);
    let (paid, burn, _) = settle_all(&mut t, &mut rng);
    let back: i64 = paid.values().sum();
    assert_eq!(back, 23 * UNIT - burn, "the staker gets its 23 units back less the burn of its own losing side");
    assert_eq!(burn, UNIT, "10 % of the 10 losing units");
}

#[test]
fn a_dispute_with_one_staker_resolves_for_it_and_returns_its_stake() {
    let mut t = Table::new(params());
    let o = pieces(&mut t, 11, 1, 10 * UNIT + 12_345)[0];
    assert!(t.open(o.0, o.1, ANCHOR));
    let end = t.window_end();
    assert_eq!(end, ANCHOR + t.p().round, "the opening's own DAA starts the first round");
    assert!(!t.resolve(end - 1));
    assert!(t.resolve(end));
    match &t.a.w.utxos[&t.claim].out.script {
        Script::Claim(c) => assert_eq!(c.status, CLAIM_REFUTED, "nobody answered: the side that staked wins"),
        _ => panic!(),
    }
    let mut rng = Rng(2);
    let (paid, burn, kept) = settle_all(&mut t, &mut rng);
    assert_eq!(paid.values().copied().collect::<Vec<_>>(), vec![10 * UNIT + 12_345]);
    assert_eq!((burn, kept), (0, 0));
}

#[test]
fn a_decided_claim_cannot_be_reopened() {
    let mut t = Table::new(params());
    let o = pieces(&mut t, 11, 1, 10 * UNIT)[0];
    assert!(t.open(o.0, o.1, ANCHOR));
    let end = t.window_end();
    assert!(t.resolve(end));
    let o2 = pieces(&mut t, 12, 1, 10 * UNIT)[0];
    let tx = t.a.open_tx(t.claim, o2.0, o2.1, 50_000_000, &ENV);
    assert!(accepted(&tx, &ENV).is_err(), "a refuted claim is not pending");
}

// ---------------------------------------------------------------- Z10

/// One dispute: an attacker with `a` sompi of aged stake opens NO on a true claim with all of it in one UTXO (or,
/// `patient`, with the minimum first and the rest when it must answer); honest stakers hold `h` in pieces of `piece`,
/// of which they deploy only what reaches the lead (an aggregator collects commitments first: a partial answer that
/// cannot reach the lead is never sent). Returns whether the attacker wins.
fn duel(p: JudgeParams, a: i64, h: i64, piece: i64, patient: bool) -> bool {
    let mut t = Table::new(p);
    let age_daa = ANCHOR - p.age;
    let mut att = a;
    let first = if patient { p.min_stake.min(a) } else { a };
    let o = t.a.stake(11, first, age_daa);
    assert!(t.open(o, 11, ANCHOR));
    att -= first;
    let mut honest = h;
    let mut daa = ANCHOR + 1;
    loop {
        let d = t.d();
        if d.side == NO {
            let need = p.need(d.no).unwrap() - d.yes;
            if need > honest {
                break;
            }
            // Deposit pieces until the lead flips.
            let mut have = 0;
            while have < need {
                let n = ((need - have + piece - 1) / piece).min(BATCH) as usize;
                let ps: Vec<(u64, u8)> = (0..n).map(|i| (t.a.stake(20 + (i as u8), piece, age_daa), 20 + i as u8)).collect();
                assert!(t.deposit(YES, &ps, daa));
                have += piece * n as i64;
                honest -= piece * n as i64;
            }
        } else {
            let need = p.need(d.yes).unwrap() - d.no;
            if need > att || att < p.min_stake {
                break;
            }
            // The attacker answers all in (one UTXO).
            let s = t.a.stake(12, att, age_daa);
            assert!(t.deposit(NO, &[(s, 12)], daa));
            att = 0;
        }
        daa += 1;
    }
    let end = t.window_end();
    assert!(t.resolve(end));
    t.a.js(t.dispute.unwrap()).side == NO
}

/// The least attacker share `f` (of `b` aged stake) that wins one dispute against honest participation `ph`, by
/// bisection on `f` (to 10^-4).
fn f_star(p: JudgeParams, ph: f64, patient: bool) -> f64 {
    let b = 2_000_000 * UNIT as i128;
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    while hi - lo > 1e-4 {
        let f = (lo + hi) / 2.0;
        let a = (b as f64 * f) as i64;
        let h = (b as f64 * (1.0 - f) * ph) as i64;
        if duel(p, a.max(p.min_stake), h, 1_000 * UNIT, patient) {
            hi = f;
        } else {
            lo = f;
        }
    }
    hi
}

#[test]
fn z10_the_measured_threshold_matches_the_notes_formula() {
    println!("| mu | p_h | f* formula p_h/(p_h + mu) | measured, all in | measured, patient |");
    println!("|---|---|---|---|---|");
    for mu in [11_000i64, 12_500, 15_000] {
        for ph in [1.0f64, 0.5, 0.25] {
            let p = JudgeParams { mu_bps: mu, ..params() };
            let m = mu as f64 / 1e4;
            let formula = ph / (ph + m);
            let all_in = f_star(p, ph, false);
            let patient = f_star(p, ph, true);
            println!("| {m} | {ph} | {:.4} | {:.4} | {:.4} |", formula, all_in, patient);
            // Honest pieces of 1,000 units on a total of 2 * 10^6 units: the honest side's granularity is 1/2000
            // of the stake, so the measured threshold lies within that of the formula.
            assert!((all_in - formula).abs() < 2e-3, "mu {m} p_h {ph}: {all_in} vs {formula}");
            assert!(patient >= all_in - 2e-3, "holding back never helps the attacker");
        }
    }
}

/// The drill of DESIGN-ORACLE-2.md 7 (Z10): 100 parallel disputes (one claim each), honest participation `ph`, a
/// patient attacker that first opens every dispute with `x` and, once honest stake is committed to every answer,
/// escalates as many disputes as its remaining stake allows. Honest stake committed to one dispute cannot serve another
/// (it is a receipt until that dispute resolves, and a fresh stake after); honest answers greedily while its free aged
/// reserve lasts. Returns the number of disputes the attacker wins.
fn drill(p: JudgeParams, a: i64, h: i64, x: i64) -> usize {
    let n = 100;
    let mut att = a;
    let mut reserve = h;
    let mut tallies: Vec<(i64, i64)> = vec![]; // (no, yes)
    for _ in 0..n {
        if att < x {
            break;
        }
        att -= x;
        tallies.push((x, 0));
    }
    // Honest answers every opening it can.
    for t in tallies.iter_mut() {
        let need = p.need(t.0).unwrap();
        if need <= reserve {
            reserve -= need;
            t.1 = need;
        }
    }
    // The attacker escalates the disputes honest leads, cheapest first, pass after pass; honest re-answers while it can.
    for _ in 0..64 {
        let mut order: Vec<usize> = (0..tallies.len()).filter(|i| tallies[*i].1 >= p.need(tallies[*i].0).unwrap()).collect();
        order.sort_by_key(|i| tallies[*i].1);
        let mut moved = false;
        for i in order {
            let (no, yes) = tallies[i];
            let need = p.need(yes).unwrap() - no;
            if need > att {
                break;
            }
            att -= need;
            tallies[i].0 += need;
            moved = true;
            let back = p.need(tallies[i].0).unwrap() - yes;
            if back <= reserve {
                reserve -= back;
                tallies[i].1 += back;
            }
        }
        if !moved {
            break;
        }
    }
    tallies.iter().filter(|t| t.0 > 0 && p.need(t.0).unwrap() > t.1).count()
}

#[test]
fn z10_drill_100_parallel_disputes_half_participation_patient_attacker() {
    let p = JudgeParams { mu_bps: 12_500, ..params() };
    let b = 10_000_000 * UNIT / 10; // 1,000,000 units
    let ph = 0.5;
    println!("| f | honest deployable | attacker | disputes won by the attacker (of 100), best opening size |");
    println!("|---|---|---|---|");
    let mut first_any = None;
    let mut first_all = None;
    for pct in (5..=70).step_by(1) {
        let f = pct as f64 / 100.0;
        let a = (b as f64 * f) as i64;
        let h = (b as f64 * (1.0 - f) * ph) as i64;
        // The attacker picks the opening size that wins most (a coarse search).
        let mut best = (0usize, 0i64);
        for k in 1..=200 {
            let x = (a / 100 / 200 * k).max(p.min_stake);
            let won = drill(p, a, h, x);
            if won > best.0 {
                best = (won, x);
            }
        }
        // And the all-in single dispute.
        let single = if p.need(a).unwrap() > h { 1 } else { 0 };
        let won = best.0.max(single);
        if won > 0 && first_any.is_none() {
            first_any = Some(f);
        }
        if won == 100 && first_all.is_none() {
            first_all = Some(f);
        }
        if pct % 5 == 0 || Some(f) == first_any || Some(f) == first_all {
            println!("| {f:.2} | {h} | {a} | {won} (x = {}) |", best.1);
        }
    }
    let m = 1.25;
    let one = ph / (ph + m);
    // Escalation pass after pass: honest answers every escalation while its reserve lasts, so the attacker ends each
    // dispute with mu times honest's last answer there; it takes all once A >= mu H, i.e. f >= mu p_h / (1 + mu p_h).
    let all = m * ph / (1.0 + m * ph);
    // Triage (honest abandons disputes it cannot hold to the end and defends the rest): against an even spread the
    // attacker wins 100 - H / (mu A / 100) disputes; it takes all only at A > 100 H / mu.
    let triage_all = 100.0 * ph / m / (1.0 + 100.0 * ph / m);
    println!(
        "f* to win one dispute: measured {:?}, formula {one:.4}; to win all 100 against greedy honest answers: measured {:?}, mu p_h/(1 + mu p_h) = {all:.4}; against triage {triage_all:.4}",
        first_any, first_all
    );
    let fa = first_any.expect("some f wins one");
    assert!((fa - one).abs() <= 0.011, "one dispute: {fa} vs {one}");
    let fl = first_all.expect("some f wins all");
    assert!((fl - all).abs() <= 0.011, "all 100: {fl} vs {all}");
}
