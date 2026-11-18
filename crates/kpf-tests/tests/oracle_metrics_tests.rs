//! Numbers of the oracle-market integration (docs/DESIGN-ORACLE-IMPL.md, third slice): script and state bytes of every
//! contract; script units, compute and storage mass, bytes and relay fee of every transaction shape; the largest
//! multi-pair shapes that fit; the cost of one pair-month. Limits as `kpf_metrics_tests`: compute and storage mass
//! 500,000 each, 250,000 bytes; relay fee 100 sompi per gram; each input commits the smallest covering compute budget.
//! Run with `--nocapture` for the tables.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::config::params::MAINNET_PARAMS;
use kaspa_consensus_core::mass::{transaction_estimated_serialized_size, ComputeBudget, MassCalculator, ScriptUnits};
use kaspa_consensus_core::tx::{PopulatedTransaction, TransactionInput};
use kpf_protocol::bond::{BindArgs, Claim};
use kpf_protocol::judge::{UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;

const COMPUTE_LIMIT: u64 = 500_000;
const STORAGE_LIMIT: u64 = 500_000;
const BYTES_LIMIT: u64 = 250_000;

#[derive(Clone, Debug)]
struct Row {
    name: String,
    inputs: usize,
    units: Vec<u64>,
    compute: u64,
    storage: u64,
    bytes: u64,
    fee: u64,
}

fn measure(name: &str, t: &T) -> Row {
    let (tx, e) = t;
    let m = run_metered(tx, e);
    let mut t2 = tx.clone();
    let mut units = vec![];
    for (i, r) in m.iter().enumerate() {
        assert!(r.result.is_ok(), "{name} input {i}: {:?}", r.result);
        assert!(r.max_stack < 244, "{name}: stack {}", r.max_stack);
        units.push(r.script_units);
        let b = ComputeBudget::checked_covering_script_units(ScriptUnits(r.script_units)).expect("budget").value();
        let inp = &tx.inputs[i];
        t2.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, b);
    }
    let calc = MassCalculator::new_with_consensus_params(&MAINNET_PARAMS);
    let nc = calc.calc_non_contextual_masses(&t2);
    let storage = calc.calc_contextual_masses(&PopulatedTransaction::new(&t2, e.to_vec())).map(|m| m.storage_mass).unwrap_or(u64::MAX);
    let rep = kpf_protocol::tx::masses(&t2, e);
    let stack = m.iter().map(|r| r.max_stack).max().unwrap_or(0);
    println!("  ({name}: largest stack {stack})");
    Row { name: name.into(), inputs: tx.inputs.len(), units, compute: nc.compute_mass, storage, bytes: transaction_estimated_serialized_size(&t2), fee: kpf_protocol::tx::min_fee(&rep, 100) }
}

fn fits(r: &Row) -> bool {
    r.compute <= COMPUTE_LIMIT && r.storage <= STORAGE_LIMIT && r.bytes <= BYTES_LIMIT
}

fn print(rows: &[Row]) {
    println!("| Transaction | Inputs | Script units per input | Compute mass | Storage mass | Bytes | Fee (KAS) |");
    println!("|---|---|---|---|---|---|---|");
    for r in rows {
        let u: Vec<String> = r.units.iter().map(|x| x.to_string()).collect();
        println!("| {} | {} | {} | {} | {} | {} | {:.4} |", r.name, r.inputs, u.join(" / "), r.compute, r.storage, r.bytes, r.fee as f64 / 1e8);
    }
}

fn liq(p: i64, t0: i64) -> ClaimSpec {
    ClaimSpec { kind: LIQ, side: L, n: 1_000_000_000, p, t0, lock: t0 as u64 }
}

/// k pairs proposed in one transaction, vouched by one input of the underwriter: in 0..k the pairs, k the vouch, k+1
/// the executor; out 0..k the pending parts (each under a fresh id), then change.
fn multi_propose(pairs: &[LPair]) -> T {
    let k = kit();
    let kk = pairs.len();
    let mut ins = vec![];
    let mut outs = vec![];
    for (i, lp) in pairs.iter().enumerate() {
        let c = liq(920_000_000, 450);
        let (w, fb) = lp.s.pending_of(k, c.n, c.kind, c.side, c.p, c.t0).unwrap();
        let w = w.made_by(&lp.op);
        ins.push(pair_in(lp, "propose", vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.p), iv(c.t0), iv(kk as i64), bv(&[0u8; 65]), iv(fb), iv(i as i64), bv(&k.pending.prefix), bv(&k.pending.suffix)]));
        outs.push(out(lp.s.pending_value(lp.value, &w) as u64, pend_spk(&w), None));
    }
    ins.push(key_in(outpoint(210), 50_000_000_000_000, 0, &uw_kp()));
    ins.push(exec_in(211));
    outs.push(change(90_000_000_000));
    // Review 6: each pending part under its own fresh id, authored by its pair's input.
    for (i, lp) in pairs.iter().enumerate() {
        seal(&mut outs, i, i as u16, lp.op);
    }
    let seqs = vec![0u64; ins.len()];
    build_ext(&ins, outs, 450, &seqs)
}

/// k pairs renewed by one bind (claims 0..k-1 name the pair inputs): in 0..k the pairs, k the slot, k+1 the executor;
/// out 0..k the renewed pairs, k the slot, k+1 the premium, k+2 change.
fn multi_renew(pairs: &[LPair], sl: &LSlot, lock: u64, advance: i64) -> T {
    let kk = pairs.len();
    let claims: Vec<Claim> = pairs.iter().enumerate().map(|(i, lp)| Claim::input(i, lp.s.cover_of(lp.s.size))).collect();
    let a = BindArgs::new(lock as i64, pairs[0].s.b.expiry + pairs[0].s.m.bond.term, advance, &claims);
    let ctx = kpf_protocol::bond::Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).expect("the slot binds them all");
    let mut ins = vec![];
    let mut outs = vec![];
    let mut prem = 0;
    for (i, lp) in pairs.iter().enumerate() {
        let (r, pay) = lp.s.renewed().unwrap();
        prem += pay;
        ins.push(pair_in(lp, "renew", vec![iv(kk as i64), iv(i as i64)]));
        outs.push(out(lp.value - pay as u64, pair_spk(&r), bind(i as u16, lp.id.unwrap())));
    }
    ins.push(slot_in(sl, "bind", bind_args(&a, &uw_kp())));
    ins.push(exec_in(219));
    outs.push(out(sl.value, bond_spk(&b2), bind(kk as u16, sl.id.unwrap())));
    outs.push(out(prem as u64, key_spk(&sl.s.owner), None));
    outs.push(change(90_000_000_000));
    let mut seqs = vec![0u64; kk];
    seqs.push(10);
    seqs.push(0);
    build_ext(&ins, outs, lock, &seqs)
}

/// `count` pairs opened one after the other on one large slot (each its own open transaction).
fn many_pairs(count: usize) -> (Vec<LPair>, LSlot) {
    let mut sl = slot(K * 10);
    let mut pairs = vec![];
    for i in 0..count {
        let (lo, so) = (order(L, &long_kp(), 0x71 + 2 * i as u8, 0), order(S, &short_kp(), 0x72 + 2 * i as u8, 0));
        let (t, p, b2) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
        must("open", &t);
        pairs.push(live(&t, 2, OPEN_DAA, &p));
        sl = live(&t, 3, OPEN_DAA, &b2);
    }
    (pairs, sl)
}

#[test]
fn oracle_sizes_masses_and_fees() {
    let k = kit();
    let len = |t: &kpf_protocol::artifacts::Template| t.prefix.len() + t.state_len + t.suffix.len();
    for t in [&k.open, &k.pair, &k.pending, &k.bond, &k.stake] {
        println!("script bytes: {} {} (1 + {} + {})", t.name, len(t), t.state_len, t.suffix.len());
    }
    let mut rows = vec![];
    // Open.
    let (lo, so, sl) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let (t, p, b2) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
    rows.push(measure("open (2 orders, slot bind, funder)", &t));
    let lp = live(&t, 2, OPEN_DAA, &p);
    let lsl = live(&t, 3, OPEN_DAA, &b2);
    // Propose.
    let (t, w) = propose_tx(&lp, &liq(920_000_000, 450), &uw_kp(), None);
    rows.push(measure("propose liquidation (pair, vouch, funder)", &t));
    let lw = live(&t, 0, 500, &w);
    let c = ClaimSpec { kind: CLOSE, side: L, n: lp.s.size, p: 1_050_000_000, t0: 450, lock: 450 };
    let (t, wc) = propose_tx(&lp, &c, &uw_kp(), Some(&long_kp()));
    rows.push(measure("propose close (pair, vouch, funder)", &t));
    let lwc = live(&t, 0, 500, &wc);
    // Finalize.
    rows.push(measure("finalize liquidation (forfeit)", &finalize_tx(&lw, 500, 200).0));
    rows.push(measure("finalize close", &finalize_tx(&lwc, 500, 200).0));
    let void = live(&propose_tx(&lp, &liq(920_000_000, 899), &uw_kp(), None).0, 0, 1_001, &lp.s.pending_of(k, lp.s.size, LIQ, L, 920_000_000, 899).unwrap().0.made_by(&lp.op));
    rows.push(measure("finalize void (restore, bond to the defender)", &finalize_tx(&void, 1_001, 200).0));
    // Cure.
    rows.push(measure("cure", &cure_tx(&lw, &long_kp()).0));
    // Contest, defence, decide, restore, settle, close.
    let st = stake(70, 10 * UNIT as u64, 0, 0x90);
    let (t, cw, r, d) = contest_tx(&lw, &st, 70, 520);
    rows.push(measure("contest (pending, opener's stake, funder)", &t));
    let (lcw, lr, ld) = (live(&t, 0, 520, &cw), live(&t, 1, 520, &r), live(&t, 2, 520, &d));
    let (t, d2, rs) = deposit_tx(&ld, YES, &[(stake(31, 20 * UNIT as u64, 0, 0xa1), 31)], 530);
    rows.push(measure("deposit, 1 stake (the underwriter's defence)", &t));
    let ld2 = live(&t, 0, 530, &d2);
    let mut seven = vec![];
    for i in 0..7u8 {
        seven.push((stake(80 + i, 10 * UNIT as u64, 0, 0xb0 + i), 80 + i));
    }
    rows.push(measure("deposit, 7 stakes", &deposit_tx(&ld, YES, &seven, 530).0));
    let (t, pool, dw) = decide_tx(&ld2, &lcw, 630);
    rows.push(measure("resolve and decide (dispute, contested part, funder)", &t));
    let (ldw, lpool) = (live(&t, 1, 630, &dw), live(&t, 0, 630, &pool));
    rows.push(measure("finalize upheld", &finalize_tx(&ldw, 500, 0).0));
    let (rt, _, rdw) = decide_tx(&ld, &lcw, 620);
    must("refuted", &rt);
    rows.push(measure("finalize refuted (restore, defender, disputer)", &finalize_tx(&live(&rt, 1, 620, &rdw), 500, 0).0));
    let (t, p2, _) = settle_tx(&lpool, &[live(&deposit_tx(&ld, YES, &[(stake(31, 20 * UNIT as u64, 0, 0xa1), 31)], 530).0, 1, 530, &rs[0]), lr], 631);
    rows.push(measure("settle 1 winning + 1 losing receipt", &t));
    rows.push(measure("close the pool", &pool_close_tx(&live(&t, 0, 632, &p2)).0));
    // Renewal, lapse, the price-free exits, the take with a bind.
    let (t, _, _) = renew_tx(&lp, &lsl, 1_100, 8);
    rows.push(measure("renew 1 pair (pair, slot bind, funder)", &t));
    rows.push(measure("lapse settle", &lapse_tx(&lp, lp.s.lapse_from(), 0)));
    rows.push(measure("cooperative close", &coop_tx(&lp, 100_000_000_000)));
    let (t, a1) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    rows.push(measure("amend", &t));
    let lp1 = live(&t, 0, 400, &a1);
    rows.push(measure("transfer to a signing taker", &transfer_tx(&lp1, L, 1_000_000_000, 900_000_000, &keypair(45), 20).0));
    rows.push(measure("take by an order with the slot's bind", &take_tx(&lp1, &order(L, &keypair(46), 0x7a, 0), &lsl, 1_000_000_000, 400, true).0));
    let (t, a2) = amend_tx(&lp1, S, &short_kp(), 1_000_000_000, 20);
    must("amend short", &t);
    rows.push(measure("mutual close", &mutual_tx(&live(&t, 0, 420, &a2), 1_000_000_000, 20)));
    rows.push(measure("unbond the slot", &unbond_tx(&lsl, 12, 1_400)));
    print(&rows);
    for r in &rows {
        assert!(fits(r), "{} fits a block", r.name);
    }

    // The largest multi-pair shapes: proposals vouched by one input, renewals by one bind (at most 7 claims).
    let (pairs, big) = many_pairs(12);
    let mut multi = vec![];
    let mut largest = 0;
    // Review 6: every pending part is checked fresh by a scan of at most 8 outputs, so one transaction carries at most
    // 7 whole-pair proposals (7 pending parts and the change); 8 are refused.
    not_accepted("8 proposals in one transaction (9 outputs)", &multi_propose(&pairs[..8]));
    for n in [1usize, 4, 7] {
        let t = multi_propose(&pairs[..n]);
        must("multi propose", &t);
        let r = measure(&format!("{n} proposals, one vouch"), &t);
        if fits(&r) {
            largest = n;
        }
        multi.push(r);
    }
    for n in [1usize, 4, 7] {
        let t = multi_renew(&pairs[..n], &big, 1_100, 8);
        must("multi renew", &t);
        multi.push(measure(&format!("renewal of {n} pairs, one bind"), &t));
    }
    print(&multi);
    println!("largest proposal block measured that fits: {largest} pairs (the fresh-id scan of at most 8 outputs limits it)");
    assert!(multi.iter().filter(|r| r.name.starts_with("renewal")).all(fits), "7 renewals in one bind fit");

    // One pair-month (the term Lambda = 28 days on testnet): open, one renewal, one claim (propose and finalize), and a
    // close (here the claim is the close). Network fees in KAS, from the rows above; the premium is the pair's own.
    let get = |n: &str| rows.iter().find(|r| r.name.starts_with(n)).unwrap().fee;
    let fees = get("open") + get("renew 1") + get("propose close") + get("finalize close");
    println!("one pair-month, network fees: open + renewal + propose + finalize = {:.4} KAS", fees as f64 / 1e8);
    let m = OMarket::testnet([0; 32]);
    // The premium of the fixture pair (cover 6,000 KAS) at 10 % of the cover a year, for one 28-day term.
    let premium = 600_000_000_000i64 * 10 / 100 * 28 / 365;
    println!("premium at 10 % of the cover a year for one term: {:.2} KAS ({:.3} % of the cover; {} premium per whole unit per term)", premium as f64 / 1e8, premium as f64 * 100.0 / 600_000_000_000f64, premium / 1_000);
    let _ = m;
}
