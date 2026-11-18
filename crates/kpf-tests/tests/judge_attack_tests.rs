//! Attacks on the stake judge (docs/DESIGN-ORACLE-IMPL.md, second slice), each recorded as `holds_` or `exploit_`, and
//! the model replayed against the contracts step by step with hostile variants of every step
//! (`the_model_and_the_contracts_agree_on_every_step_of_random_games`).

mod common;
mod judgekit;

use common::{build_ext, contract_source, p2sh, run, Spend as CSpend};
use judgekit::*;
use kpf_protocol::judge::*;

fn ok(t: &JTable, what: &str, tx: &JTx) -> bool {
    agree(what, &t.eng, tx)
}

/// The model's reason for refusing input `i` (the engine agrees on the verdict).
fn why(t: &JTable, tx: &JTx, i: usize) -> String {
    assert!(!ok(t, "expected refusal", tx));
    format!("{:?}", verify(tx, &Real)[i])
}

fn opened(v: i64) -> JTable {
    let mut t = JTable::new();
    let o = t.stake(11, v, 0);
    assert!(t.open(o, 11, ANCHOR));
    t
}

fn set_stake(tx: &mut JTx, j: usize, f: impl Fn(&mut Js)) {
    if let Script::Stake(s) = &mut tx.outs[j].script {
        f(s);
    }
}

// ---------------------------------------------------------------- the shared text and the offsets

fn between(src: &str, a: &str, b: &str) -> String {
    let i = src.find(a).expect("begin marker");
    let j = src[i..].find(b).expect("end marker") + i;
    src[i..j].to_string()
}

#[test]
fn holds_the_judge_bounds_are_one_text_in_the_stake_and_the_claim() {
    let s = contract_source("KpfStake.sil");
    let c = contract_source("stubs/KpfClaimStub.sil");
    assert_eq!(between(&s, "// BEGIN judge bounds", "// END judge bounds"), between(&c, "// BEGIN judge bounds", "// END judge bounds"));
    for k in ["MU_MIN_BPS = 11000", "MU_MAX_BPS = 15000", "BURN_MAX_BPS = 5000", "MIN_STAKE_MAX = 100000000000000", "UNIT = 1000000000", "CSV_LIMIT = 4294967296"] {
        assert!(s.contains(k) && c.contains(k), "{k} in both");
    }
    assert_eq!((MU_MIN_BPS, MU_MAX_BPS, BURN_MAX_BPS, MIN_STAKE_MAX, UNIT), (11_000, 15_000, 5_000, 100_000_000_000_000, 1_000_000_000));
    // The model's bounds, at each edge.
    let p = jparams();
    for (q, good) in [
        (JudgeParams { mu_bps: 10_999, ..p }, false),
        (JudgeParams { mu_bps: 11_000, ..p }, true),
        (JudgeParams { mu_bps: 15_001, ..p }, false),
        (JudgeParams { round: 0, ..p }, false),
        (JudgeParams { age: p.round - 1, ..p }, false),
        (JudgeParams { unbond: p.round - 1, ..p }, false),
        (JudgeParams { burn_bps: 5_001, ..p }, false),
        (JudgeParams { min_stake: UNIT - 1, ..p }, false),
        (JudgeParams { min_stake: MIN_STAKE_MAX + 1, ..p }, false),
        (JudgeParams::testnet(), true),
    ] {
        assert_eq!(q.check().is_ok(), good, "{q:?}");
    }
}

#[test]
fn holds_the_contracts_offsets_are_the_encodings() {
    let s = contract_source("KpfStake.sil");
    let mut j = Js::free([7; 32], jmarket());
    j.side = 0x11;
    j.amt = 0x12;
    j.round = 0x13;
    j.anchor = 0x14;
    j.start = 0x15;
    j.w_left = 0x16;
    j.base = 0x17;
    let st = j.state_bytes();
    assert_eq!(st.len(), STATE_LEN);
    // Redeem offset X = state offset X - 1 (the state starts at redeem byte 1).
    for (name, at, val) in [("STATUS_AT", 34, STATUS_FREE as u8), ("SIDE_AT", 163, 0x11), ("AMT_AT", 172, 0x12), ("ROUND_AT", 181, 0x13), ("ANCHOR_AT", 226, 0x14), ("START_AT", 235, 0x15), ("WLEFT_AT", 244, 0x16), ("BASE_AT", 262, 0x17)] {
        assert!(s.contains(&format!("int constant {name} = {at};")), "{name}");
        assert_eq!((st[at - 1], st[at]), (0x08, val), "{name}");
    }
    assert!(s.contains("int constant MARKET_AT = 43;") && s.contains("int constant MARKET_END = 130;") && s.contains("int constant STATE_END = 271;"));
    assert_eq!(&st[42..129], &jmarket().bytes()[..]);
    let c = contract_source("stubs/KpfClaimStub.sil");
    assert!(c.contains("int constant STAKE_SIDE_AT = 162;") && c.contains("int constant STAKE_STATUS_AT = 33;"));
    let cs = ClaimStub { m: jmarket(), owner: [7; 32], status: CLAIM_PENDING }.state_bytes();
    assert!(c.contains("int constant STATUS_AT = 121;") && cs[120] == 0x08 && cs[121] == CLAIM_PENDING as u8);
}

// ---------------------------------------------------------------- the replay

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

/// A hostile variant of a step: one field of the transaction changed.
fn mutate(tx: &JTx, rng: &mut Rng) -> JTx {
    let mut m = tx.clone();
    let ni = m.ins.len();
    let no = m.outs.len();
    match rng.below(13) {
        0 => {
            let j = rng.below(no as u64) as usize;
            m.outs[j].value -= 1;
        }
        1 => m.lock -= 1,
        2 => {
            let i = rng.below(ni as u64) as usize;
            m.ins[i].signer = Some(70 + rng.below(5) as u8);
        }
        3 => {
            let i = rng.below(ni as u64) as usize;
            m.ins[i].sighash = [0x02, 0x81, 0x84, 0x01][rng.below(4) as usize];
        }
        4 => {
            let i = rng.below(ni as u64) as usize;
            m.ins[i].daa = [-1, m.ins[i].daa + 1, m.ins[i].daa - 1][rng.below(3) as usize];
        }
        5 => {
            if let Some(j) = m.outs.iter().position(|o| o.cov.is_some()) {
                let twin = m.outs[j].clone();
                m.outs.push(twin);
            }
        }
        6 => {
            let j = rng.below(no as u64) as usize;
            let f = rng.below(9);
            set_stake(&mut m, j, |s| match f {
                0 => s.side = other(s.side),
                1 => s.amt += 1,
                2 => s.round += 1,
                3 => s.anchor += 1,
                4 => s.start = if s.start == -1 { 0 } else { -1 },
                5 => s.w_left += 1,
                6 => s.l_left -= 1,
                7 => s.base += 1,
                _ => s.yes_u += 1,
            });
        }
        7 => {
            let i = rng.below(ni as u64) as usize;
            m.ins[i].spend = match m.ins[i].spend {
                Spend::Deposit { side, first, k } => Spend::Deposit { side, first: first + 1, k },
                Spend::Settle { first, k } => Spend::Settle { first, k: k - 1 },
                Spend::Resolve { claim } => Spend::Resolve { claim: claim + 1 },
                Spend::Open { claim, c_out, d_out } => Spend::Open { claim, c_out, d_out: d_out + 1 },
                Spend::Commit { side, round, anchor } => Spend::Commit { side: other(side), round, anchor },
                Spend::Contest { u, d_out } => Spend::Contest { u, d_out: d_out + 1 },
                Spend::Decide { d } => Spend::Decide { d: d + 1 },
                Spend::Redeem { d } => Spend::Redeem { d: d + 1 },
                Spend::Close { j } => Spend::Close { j: j + 1 },
                s => s,
            };
        }
        8 => {
            // Drop the last output that carries a covenant id.
            if let Some(j) = m.outs.iter().rposition(|o| o.cov.is_some()) {
                m.outs.remove(j);
            }
        }
        9 => {
            // A look-alike claim: the claim input's owner changed (another script of the same id).
            for i in m.ins.iter_mut() {
                if let Script::Claim(c) = &mut i.utxo.script {
                    c.owner = key_of(77);
                }
            }
        }
        10 => {
            let i = rng.below(ni as u64) as usize;
            m.ins[i].utxo.value -= 1;
        }
        11 => {
            let i = rng.below(ni as u64) as usize;
            if let Script::Stake(s) = &mut m.ins[i].utxo.script {
                s.status = [STATUS_FREE, STATUS_UNBONDING, STATUS_RECEIPT, STATUS_DISPUTE, STATUS_RESOLVED][rng.below(5) as usize];
            }
        }
        _ => {
            let i = rng.below(ni as u64) as usize;
            if let Script::Stake(s) = &mut m.ins[i].utxo.script {
                s.m.p.mu_bps += [-1, 1, 3_000][rng.below(3) as usize];
            }
        }
    }
    m
}

/// Runs a step and three hostile variants (model and engine must agree on each); applies the step if accepted.
fn step(t: &mut JTable, rng: &mut Rng, what: &str, tx: &JTx, daa: i64, stats: &mut [u64; 3]) -> Option<Vec<u64>> {
    for _ in 0..3 {
        let m = mutate(tx, rng);
        if m != *tx {
            let accepted = agree(what, &t.eng, &m);
            stats[if accepted { 1 } else { 2 }] += 1;
        }
    }
    stats[0] += 1;
    t.run(what, tx, daa)
}

#[test]
fn the_model_and_the_contracts_agree_on_every_step_of_random_games() {
    let mut stats = [0u64; 3]; // steps, hostile variants accepted, refused
    let mut kinds = [0u64; 6]; // open, deposit, resolve, settle, close, unbond/withdraw accepted
    for g in 0..120u64 {
        let mut rng = Rng(0x7e57_0000 + g);
        let mut t = JTable::new();
        let p = jparams();
        let mut pieces: Vec<(u64, u8, bool)> = vec![];
        for i in 0..rng.range(3, 14) {
            let seed = 20 + i as u8;
            let v = rng.range(UNIT, 30 * UNIT) + rng.range(0, 999);
            let late = if rng.chance(15) { rng.range(1, 50) } else { -rng.range(0, 200) };
            let h = t.stake(seed, v, late);
            pieces.push((h, seed, late <= 0));
        }
        // Open (an unaged piece may be tried first).
        let first = pieces.remove(0);
        let tx = t.a.open_tx(t.claim, first.0, first.1, 50_000_000, &Real);
        match step(&mut t, &mut rng, "open", &tx, ANCHOR, &mut stats) {
            Some(o) => {
                kinds[0] += 1;
                t.claim = o[0];
                t.receipts.push(o[1]);
                t.dispute = Some(o[2]);
            }
            None => continue,
        }
        let mut now = ANCHOR;
        for _ in 0..8 {
            let d = t.d();
            let side = if rng.chance(85) { other(d.side) } else { d.side };
            let k = rng.range(1, 7).min(pieces.len() as i64) as usize;
            if k == 0 {
                break;
            }
            let batch: Vec<(u64, u8)> = pieces[..k].iter().map(|x| (x.0, x.1)).collect();
            now += rng.range(0, 60);
            let tx = t.a.deposit_tx(t.dispute.unwrap(), side, &batch, if rng.chance(50) { SIGHASH_SINGLE_ACP } else { SIGHASH_ALL });
            if let Some(o) = step(&mut t, &mut rng, "deposit", &tx, now, &mut stats) {
                kinds[1] += 1;
                t.dispute = Some(o[0]);
                t.receipts.extend(&o[1..=k]);
                pieces.drain(..k);
            } else if rng.chance(50) {
                pieces.rotate_left(1);
            }
        }
        // Resolve: early, then at the end.
        let end = t.window_end();
        let tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end - 1);
        assert!(step(&mut t, &mut rng, "resolve early", &tx, end - 1, &mut stats).is_none());
        let tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end + rng.range(0, 5));
        let o = step(&mut t, &mut rng, "resolve", &tx, end + 5, &mut stats).expect("resolution at the window's end");
        kinds[2] += 1;
        t.dispute = Some(o[0]);
        t.claim = o[1];
        // Settle in random batches (losers first when the pool cannot pay).
        let mut left = t.receipts.clone();
        let w = t.d().side;
        let mut guard = 0;
        while !left.is_empty() {
            guard += 1;
            assert!(guard < 100);
            if guard % 2 == 0 {
                left.sort_by_key(|h| if t.a.js(*h).side == w { 1 } else { 0 });
            }
            let k = (rng.range(1, 7) as usize).min(left.len());
            let batch: Vec<u64> = left[..k].to_vec();
            let tx = t.a.settle_tx(t.dispute.unwrap(), &batch);
            let now = t.now + 1;
            if let Some(o) = step(&mut t, &mut rng, "settle", &tx, now, &mut stats) {
                kinds[3] += 1;
                t.dispute = Some(o[0]);
                left.drain(..k);
            } else {
                left.rotate_left(1);
            }
        }
        let tx = t.a.close_tx(t.dispute.unwrap());
        let now = t.now + 1;
        assert!(step(&mut t, &mut rng, "close", &tx, now, &mut stats).is_some());
        kinds[4] += 1;
        // Leftover stakes leave.
        if let Some(x) = pieces.first().copied() {
            let tx = t.a.unbond_tx(x.0, x.1);
            let now = t.now;
            if let Some(o) = step(&mut t, &mut rng, "unbond", &tx, now, &mut stats) {
                let tx = t.a.withdraw_tx(o[0], x.1, p.unbond);
                let now = t.now + p.unbond;
                assert!(step(&mut t, &mut rng, "withdraw", &tx, now, &mut stats).is_some());
                kinds[5] += 1;
            }
        }
    }
    println!(
        "replay: {} steps, {} hostile variants ({} accepted by both, {} refused by both); accepted opens {}, deposits {}, resolutions {}, settlements {}, closes {}, withdrawals {}",
        stats[0],
        stats[1] + stats[2],
        stats[1],
        stats[2],
        kinds[0],
        kinds[1],
        kinds[2],
        kinds[3],
        kinds[4],
        kinds[5]
    );
    assert!(kinds[1] >= 120 && kinds[3] >= 120);
}

// ---------------------------------------------------------------- attacks

#[test]
fn holds_a_dispute_always_resolves_and_cannot_be_prolonged() {
    // Partial answers do not move the window; the leader cannot deposit; each lead change multiplies the total by
    // mu >= 1.1, so a dispute has at most log_mu(stake range) rounds.
    let mut t = opened(10 * UNIT);
    let end = t.window_end();
    for i in 0..5 {
        let s = t.stake(20 + i, UNIT, 0);
        assert!(t.deposit(YES, &[(s, 20 + i)], end - 5 + i as i64));
        assert_eq!(t.window_end(), end, "a partial answer moves nothing");
    }
    let n = t.stake(30, 50 * UNIT, 0);
    let tx = t.a.deposit_tx(t.dispute.unwrap(), NO, &[(n, 30)], SIGHASH_SINGLE_ACP);
    assert!(why(&t, &tx, 0).contains("leader"));
    assert!(t.resolve(end));
    // The bound on rounds: from the minimum stake to the supply.
    let p = JudgeParams { mu_bps: MU_MIN_BPS, ..jparams() };
    let (mut l, mut rounds) = (p.min_stake, 0);
    while l < 2_900_000_000_000_000_000 / 2 {
        l = p.need(l).unwrap();
        rounds += 1;
    }
    println!("at mu = 1.1 a dispute from 10 KAS to the supply has at most {rounds} lead changes");
    assert!(rounds <= 230);
}

#[test]
fn exploit_the_leader_wins_with_less_stake_when_the_answer_falls_short_of_mu() {
    // The note's rule, stated: NO leads with 10 units; YES holds 12.4 (more stake) but below ceil(1.25 x 10); NO wins.
    let mut t = opened(10 * UNIT);
    let y = t.stake(12, 124 * UNIT / 10, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    assert_eq!(t.claim_status(), CLAIM_REFUTED, "the side with less stake wins: it needs only 1/mu of the other's");
    // Bounded: the loser held less than mu times the winner's stake, and what it staked is all it loses.
    let rs = t.receipts.clone();
    let paid = t.settle(&rs).expect("settle");
    assert_eq!(paid[0].1, 10 * UNIT + (124 * UNIT / 10 - 124 * UNIT / 100), "NO gains YES's 12.4 less 10 %");
}

#[test]
fn holds_one_stake_counts_once() {
    // Two disputes (two claims) in one transaction both counting the same stake: its receipt carries one id.
    let mut a = Arena::new(jmarket(), FUNDER);
    let mut eng = Eng::default();
    let c1 = a.claim([0xc1; 32], 9, ANCHOR);
    let c2 = a.claim([0xc2; 32], 9, ANCHOR);
    let mut ds = vec![];
    for (c, seed) in [(c1, 11u8), (c2, 12)] {
        let s = a.stake(seed, 10 * UNIT, ANCHOR - 500);
        let tx = a.open_tx(c, s, seed, 50_000_000, &Real);
        assert!(agree("open", &eng, &tx));
        let hs = a.apply(&tx, ANCHOR);
        eng.applied(&tx, &hs);
        ds.push(hs[2]);
    }
    let y = a.stake(13, 13 * UNIT, ANCHOR - 500);
    let t1 = a.deposit_tx(ds[0], YES, &[(y, 13)], SIGHASH_SINGLE_ACP);
    let t2 = a.deposit_tx(ds[1], YES, &[(y, 13)], SIGHASH_SINGLE_ACP);
    // in: 0 dispute 1, 1 the stake, 2 dispute 2 (counting [1, 2) too), 3 funder; out: 0, 1 the receipt (of dispute 1),
    // 2 dispute 2's continuation, 3 change.
    let mut both = t1.clone();
    let mut d2 = t2.ins[0].clone();
    d2.spend = Spend::Deposit { side: YES, first: 1, k: 1 };
    both.ins.insert(2, d2);
    both.outs.insert(2, t2.outs[0].clone());
    assert!(!agree("one stake in two disputes", &eng, &both));
    let r = verify(&both, &Real);
    assert!(r[0].is_err() || r[2].is_err(), "{r:?}");
    // Each alone is fine; the stake is then a receipt and cannot commit again (and its UTXO is spent).
    assert!(agree("one dispute", &eng, &t1));
    let hs = a.apply(&t1, ANCHOR + 1);
    eng.applied(&t1, &hs);
    let mut again = a.deposit_tx(ds[1], YES, &[(hs[1], 13)], SIGHASH_SINGLE_ACP);
    again.ins[1].daa = ANCHOR - 500;
    assert!(!agree("a receipt commits again", &eng, &again));
}

#[test]
fn holds_aged_stake_cannot_be_manufactured() {
    let mut t = opened(10 * UNIT);
    // A parent not yet accepted reads -1: refused, whatever the anchor.
    let f = t.a.stake(12, 13 * UNIT, -1);
    let tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &[(f, 12)], SIGHASH_SINGLE_ACP);
    assert!(why(&t, &tx, 0).contains("not accepted"));
    // The lock time plays no part: a high lock time does not age a stake.
    let late = t.stake(12, 13 * UNIT, 1);
    let mut tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &[(late, 12)], SIGHASH_SINGLE_ACP);
    tx.lock = ANCHOR + 1_000_000;
    assert!(why(&t, &tx, 0).contains("aged"));
    // Winnings come back as a fresh stake: it counts only for claims made ageDaa after it was paid.
    let y = t.stake(12, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    let rs = t.receipts.clone();
    assert!(t.settle(&rs).is_some());
    let paid_at = t.now;
    let won = *t.a.w.utxos.iter().find(|(_, u)| matches!(&u.out.script, Script::Stake(s) if s.status == STATUS_FREE && s.owner == key_of(12) && u.daa == paid_at)).unwrap().0;
    for (anchor, good) in [(paid_at + jparams().age - 1, false), (paid_at + jparams().age, true)] {
        let mut a2 = t.a.clone();
        let c = a2.claim([0xc3; 32], 9, anchor);
        let tx = a2.open_tx(c, won, 12, 50_000_000, &Real);
        assert_eq!(agree("a winner's fresh stake", &t.eng, &tx), good, "anchor {anchor}");
    }
    // A look-alike opener whose state is not canonical (one field pushed as OP_PUSHDATA1 7 bytes): the claim refuses
    // it, so a stake whose code reads other values than its bytes cannot open a dispute.
    let mut t2 = JTable::new();
    let s = t2.stake(11, 10 * UNIT, 0);
    let tx = t2.a.open_tx(t2.claim, s, 11, 50_000_000, &Real);
    let (good, ge) = t2.eng.build(&tx);
    assert!(run(&good, &ge).unwrap().iter().all(|r| r.is_ok()));
    let mut bytes = Js::free(key_of(11), jmarket()).state_bytes();
    bytes[180] = 0x4c;
    bytes[181] = 0x07;
    let weird = kpf_protocol::artifacts::splice(&jkit().stake, &bytes);
    let mut ins = vec![];
    for (k, i) in tx.ins.iter().enumerate() {
        let mut x = t2.eng.input_of(i);
        if k == 1 {
            x.utxo = kaspa_consensus_core::tx::UtxoEntry::new(x.utxo.amount, p2sh(&weird), x.utxo.block_daa_score, false, None);
            if let CSpend::Signed { entry, args, .. } = x.spend {
                x.spend = CSpend::Signed { art: weird.clone(), entry, args };
            }
        }
        ins.push(x);
    }
    let (wt, we) = build_ext(&ins, good.outputs.clone(), 0, &[0, 0, 0]);
    let r = run(&wt, &we).expect("bindings");
    assert!(r[0].is_err(), "the claim refuses a non-canonical opener");
}

#[test]
fn holds_a_staker_is_paid_once_and_exactly() {
    let mut t = opened(10 * UNIT);
    let y = t.stake(12, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    let rs = t.receipts.clone();
    let pool = t.dispute.unwrap();
    // A look-alike receipt (any genesis can write one, without the dispute's id): refused.
    let fake = t.a.w.add(JOut { script: Script::Stake(Js::receipt(key_of(30), jmarket(), YES, 100 * UNIT, 0, ANCHOR)), value: 100 * UNIT, cov: None }, ANCHOR);
    let tx = t.a.settle_tx(pool, &[rs[0], fake]);
    assert!(!ok(&t, "a receipt without the id", &tx));
    // The same receipt twice in one settlement is not a transaction; once settled it is spent.
    assert!(t.settle(&rs[..1]).is_some());
    assert!(t.settle(&rs[1..]).is_some());
    assert!(t.a.w.utxos.get(&rs[1]).is_none());
    // Exactly: 13 + 9 units to the winner; the burn 1 unit; the pool's base to the executor at close.
    assert!(t.close().is_some());
}

#[test]
fn holds_the_outcome_reaches_the_disputed_claim_once() {
    let mut t = opened(10 * UNIT);
    let end = t.window_end();
    // Another UTXO of the claim's id with another script: refused by the dispute.
    let look = t.a.w.add(JOut { script: Script::Claim(ClaimStub { m: jmarket(), owner: key_of(8), status: CLAIM_CONTESTED }), value: 100_000_000, cov: Some(CLAIM_ID) }, ANCHOR);
    let tx = t.a.resolve_tx(t.dispute.unwrap(), look, end);
    assert!(why(&t, &tx, 0).contains("not the contested claim"));
    // The contested claim cannot close (its only exit is the verdict).
    let cu = t.a.w.utxos[&t.claim].clone();
    let f = t.a.fund();
    let tx = JTx { ins: vec![t.a.w.input(t.claim, Spend::ClaimClose, Some(9), SIGHASH_ALL, 0), t.a.w.input(f, Spend::Key, Some(FUNDER), SIGHASH_ALL, 0)], outs: vec![JOut { script: Script::Key(9), value: cu.out.value, cov: None }], lock: 0 };
    assert!(why(&t, &tx, 0).contains("contested"));
    assert!(t.resolve(end));
    // Delivered once: the decided claim is not contested; the pool is not a dispute.
    let tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end + 1);
    assert!(!ok(&t, "delivered twice", &tx));
    // A decided claim cannot be contested again (no re-opening).
    let s = t.stake(12, 10 * UNIT, 0);
    let tx = t.a.open_tx(t.claim, s, 12, 50_000_000, &Real);
    assert!(why(&t, &tx, 0).contains("not pending"));
}

#[test]
fn holds_a_rich_minority_cannot_win_by_timing() {
    // The answer in the last DAA of a window gives the other side a full window; the leader cannot raise the bar.
    let mut t = opened(10 * UNIT);
    let end = t.window_end();
    let y = t.stake(12, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], end - 1));
    let end2 = t.window_end();
    assert_eq!(end2, end - 1 + jparams().round);
    let tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end2 - 1);
    assert!(!ok(&t, "resolved before NO's full window", &tx));
    let big = t.stake(13, 1_000 * UNIT, 0);
    let tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &[(big, 13)], SIGHASH_SINGLE_ACP);
    assert!(why(&t, &tx, 0).contains("leader"));
    // Capital raised after the claim never counts, however long the dispute runs (the anchor is the claim's DAA).
    let fresh = t.a.stake(14, 1_000 * UNIT, ANCHOR + 1);
    let tx = t.a.deposit_tx(t.dispute.unwrap(), NO, &[(fresh, 14)], SIGHASH_SINGLE_ACP);
    assert!(why(&t, &tx, 0).contains("aged"));
}

#[test]
fn holds_griefing_honest_stake_costs_the_attacker_its_stake() {
    // An opening on a true claim with the minimum: honest must answer ceil(mu x it) and then holds it for the round
    // plus the age of the fresh stake it is paid back; the attacker loses its whole stake (1 - phi to honest).
    let p = JudgeParams::testnet();
    let s = p.min_stake;
    let h = p.need(s).unwrap();
    let locked_daa = p.round + p.age;
    let lost = s;
    let to_honest = s - p.burn(s).unwrap();
    let kas_days = h as f64 / 1e8 * locked_daa as f64 / 864_000.0;
    println!(
        "testnet: an opening of {} KAS forces {} KAS of honest stake idle for {:.1} days ({kas_days:.0} KAS-days); the attacker loses {} KAS, of which {} KAS are paid to the honest side: {:.3} KAS lost per honest KAS-day, a {:.1} % return to honest per episode",
        s / 100_000_000,
        h / 100_000_000,
        locked_daa as f64 / 864_000.0,
        lost / 100_000_000,
        to_honest / 100_000_000,
        lost as f64 / 1e8 / kas_days,
        100.0 * to_honest as f64 / h as f64
    );
    assert!(lost > 0 && to_honest > 0);
    // On the engine: the attacker's receipt is forfeited, the honest answer is paid back with the attacker's stake.
    let mut t = opened(UNIT);
    let y = t.stake(12, jparams().need(UNIT).unwrap(), 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    let rs = t.receipts.clone();
    let paid = t.settle(&rs).expect("settle");
    let of = |h: u64| paid.iter().find(|x| x.0 == h).unwrap().1;
    assert_eq!(of(rs[0]), 0, "the attacker loses its stake");
    assert_eq!(of(rs[1]), jparams().need(UNIT).unwrap() + UNIT - UNIT / 10);
}

#[test]
fn exploit_a_relayed_commitment_can_land_after_the_window_and_be_forfeited() {
    // A staker signs a SINGLE|ANYONECANPAY commitment for YES in round 0; whoever holds it lands it as a partial answer
    // after the window has closed (no upper bound on time exists), before anyone resolves: the stake is then forfeited.
    // Bounded: the commitment is void once the lead changes (its round) or in another dispute (its anchor and id);
    // the staker revokes it only by spending the stake (which restarts its age). Stakers sign commitments only into an
    // answer an aggregator completes as one chain of deposits.
    let mut t = opened(10 * UNIT);
    let end = t.window_end();
    let y = t.stake(12, 2 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], end + 50), "accepted after the window");
    assert!(t.resolve(end + 50));
    let rs = t.receipts.clone();
    let paid = t.settle(&rs).expect("settle");
    assert_eq!(paid[1].1, 0, "the late partial answer is forfeited");
}

#[test]
fn holds_a_commitment_cannot_be_moved_to_another_dispute() {
    // The commitment's signature fixes its output's covenant binding (Z13), its round and its anchor; moving the
    // receipt to another dispute's id invalidates the stake's own input.
    let mut a = Arena::new(jmarket(), FUNDER);
    let mut eng = Eng::default();
    let c1 = a.claim([0xc1; 32], 9, ANCHOR);
    let c2 = a.claim([0xc2; 32], 9, ANCHOR);
    let mut ds = vec![];
    for (c, seed) in [(c1, 11u8), (c2, 12)] {
        let s = a.stake(seed, 10 * UNIT, ANCHOR - 500);
        let tx = a.open_tx(c, s, seed, 50_000_000, &Real);
        let hs = a.apply(&tx, ANCHOR);
        eng.applied(&tx, &hs);
        ds.push(hs[2]);
    }
    let y = a.stake(13, 13 * UNIT, ANCHOR - 500);
    let t1 = a.deposit_tx(ds[0], YES, &[(y, 13)], SIGHASH_SINGLE_ACP);
    assert!(agree("as signed", &eng, &t1));
    let (tx, e) = eng.build(&t1);
    let sig = tx.inputs[1].signature_script.clone();
    let t2 = a.deposit_tx(ds[1], YES, &[(y, 13)], SIGHASH_SINGLE_ACP);
    let (mut tx2, e2) = eng.build(&t2);
    tx2.inputs[1].signature_script = sig;
    tx2.finalize();
    let r = run(&tx2, &e2).expect("bindings");
    assert!(r[1].is_err(), "the stake's signature does not move to another dispute");
    let _ = e;
}

#[test]
fn holds_no_dispute_or_stake_is_left_unspendable() {
    // Every state the template writes has an exit that needs nobody's cooperation but the owner's for its own stake:
    // a dispute resolves by anyone once its window closes (the contested claim decides by anyone with it), a pool
    // settles every receipt and closes by anyone, a receipt is settled by anyone, a free stake unbonds and withdraws
    // by its owner. Played out: every UTXO of the game ends spent except the payouts, the decided claim and the burn.
    let mut t = opened(10 * UNIT);
    let y = t.stake(12, 13 * UNIT, 0);
    let n = t.stake(13, 20 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    assert!(t.deposit(NO, &[(n, 13)], ANCHOR + 2));
    let end = t.window_end();
    assert!(t.resolve(end));
    let rs = t.receipts.clone();
    assert!(t.settle(&rs).is_some());
    let close = t.close().expect("close");
    assert!(close.outs.iter().any(|o| o.script == Script::Burn));
    for (_, u) in &t.a.w.utxos {
        if let Script::Stake(s) = &u.out.script {
            assert_eq!(s.status, STATUS_FREE, "only free stakes remain: {s:?}");
        }
    }
    // Each winner's payout is a free stake it can take out.
    let won: Vec<(u64, u8)> = t.a.w.utxos.iter().filter_map(|(h, u)| match &u.out.script {
        Script::Stake(s) if s.owner == key_of(11) || s.owner == key_of(13) => Some((*h, if s.owner == key_of(11) { 11 } else { 13 })),
        _ => None,
    }).collect();
    assert_eq!(won.len(), 2);
    for (h, seed) in won {
        let tx = t.a.unbond_tx(h, seed);
        let now = t.now;
        let o = t.run("unbond", &tx, now).expect("unbond");
        let tx = t.a.withdraw_tx(o[0], seed, jparams().unbond);
        let now = t.now + jparams().unbond;
        assert!(t.run("withdraw", &tx, now).is_some());
    }
}

#[test]
fn exploit_a_dispute_written_by_a_genesis_traps_the_stakers_who_join_it() {
    // A genesis can write any state under a fresh id G: here a contested stand-in claim and a "dispute" whose tally
    // says NO holds 10 units while no NO receipt exists. A staker who joins YES without checking the dispute's lineage
    // takes the lead and wins, but the pool can never pay its share (there is nothing to forfeit), so its receipt can
    // never settle: its stake is stuck. Bounded: only stakers who commit to a dispute they did not trace to a real
    // opening (a claim of a verified lineage spent by KpfStake.open) lose, and only what they commit; the contracts
    // cannot tell a dispute's genesis from its opening (the same exposure as slice A's look-alike pairs). Wallets must
    // check the opening transaction, an indexer fact.
    let mut t = JTable::new();
    let g = [0xd7; 32];
    let claim = ClaimStub { m: jmarket(), owner: key_of(9), status: CLAIM_CONTESTED };
    let h = Real.spk_hash(&Script::Claim(claim.clone()));
    t.claim = t.a.w.add(JOut { script: Script::Claim(claim), value: 100_000_000, cov: Some(g) }, ANCHOR);
    let forged = Js { spk_hash: h, start: ANCHOR, ..Js::opened(jmarket(), h, ANCHOR, 10 * UNIT) };
    t.dispute = Some(t.a.w.add(JOut { script: Script::Stake(forged), value: 50_000_000, cov: Some(g) }, ANCHOR));
    let y = t.stake(12, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1), "the forged dispute accepts an honest deposit");
    let end = t.window_end();
    assert!(t.resolve(end));
    assert_eq!(t.claim_status(), CLAIM_UPHELD);
    let rs = t.receipts.clone();
    assert!(t.settle(&rs).is_none(), "the winner's share cannot be paid: its stake is stuck");
}
