//! Z4, the part measurable in this slice (docs/DESIGN-ORACLE-2.md 7; docs/DESIGN-ORACLE-IMPL.md): size, script units,
//! compute and storage mass of every `KpfBond` path, of a bind at open (stand-in order) and of a renewal touching 1,
//! 4 and 7 pairs (stand-in pairs), against the limits of `vendor/kob/docs/spec/matcher.md` 3.2 (compute and storage
//! 500,000 each, 250,000 bytes), as `kpf_metrics_tests` measures the book-market paths. The stand-in pair is about a
//! seventh of a real pair's script; the docs extrapolate the renewal of real pairs from `KpfPair`'s measured figures.
//!
//! Run with `--nocapture` to print the tables.

mod bondkit;
mod common;

use bondkit::*;
use common::*;
use kaspa_consensus_core::config::params::MAINNET_PARAMS;
use kaspa_consensus_core::mass::{transaction_estimated_serialized_size, ComputeBudget, MassCalculator, ScriptUnits};
use kaspa_consensus_core::tx::{PopulatedTransaction, Transaction, TransactionInput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::*;
use secp256k1::Keypair;

const SLOT_ID: Hash = Hash::from_bytes([0x51; 32]);
const K: u64 = 1_000_000_000;
const COMPUTE_LIMIT: u64 = 500_000;
const BYTES_LIMIT: u64 = 250_000;

fn uw() -> Keypair {
    keypair(31)
}

struct Row {
    name: String,
    inputs: usize,
    units: Vec<u64>,
    stack: usize,
    compute: u64,
    storage: u64,
    bytes: u64,
    fee: u64,
}

fn measure(name: &str, tx: &Transaction, e: &[UtxoEntry]) -> Row {
    let m = run_metered(tx, e);
    let mut t = tx.clone();
    let mut units = vec![];
    let mut stack = 0;
    for (i, r) in m.iter().enumerate() {
        assert!(r.result.is_ok(), "{name} input {i}: {:?}", r.result);
        assert!(r.max_stack < 244, "{name}: stack {}", r.max_stack);
        stack = stack.max(r.max_stack);
        units.push(r.script_units);
        let b = ComputeBudget::checked_covering_script_units(ScriptUnits(r.script_units)).expect("budget").value();
        let inp = &tx.inputs[i];
        t.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, b);
    }
    let calc = MassCalculator::new_with_consensus_params(&MAINNET_PARAMS);
    let nc = calc.calc_non_contextual_masses(&t);
    let storage = calc.calc_contextual_masses(&PopulatedTransaction::new(&t, e.to_vec())).map(|m| m.storage_mass).unwrap_or(u64::MAX);
    let rep = kpf_protocol::tx::masses(&t, e);
    Row {
        name: name.to_string(),
        inputs: tx.inputs.len(),
        units,
        stack,
        compute: nc.compute_mass,
        storage,
        bytes: transaction_estimated_serialized_size(&t),
        fee: kpf_protocol::tx::min_fee(&rep, 100),
    }
}

fn slot() -> LSlot {
    LSlot { op: outpoint(100), value: K, daa: 0, id: Some(SLOT_ID), b: Bond::new(xkey(&uw()), market(), 0) }
}

fn pair(n: u8) -> LPair {
    LPair {
        op: outpoint(110 + n),
        value: 200_000_000,
        daa: 1_000,
        id: Hash::from_bytes([0x60 + n; 32]),
        p: StubPair { m: market(), uw_id: SLOT_ID.as_bytes(), uw_key: xkey(&uw()), owner: xkey(&keypair(33)), cover: 1_000_000, expiry: 1_900 },
    }
}

/// A slot that counts the pairs' cover at 1,900 (and is 990 DAA old at 1,800).
fn bound(n: usize) -> LSlot {
    let s = slot();
    let cl: Vec<Claim> = (0..n).map(|i| Claim::input(i, 1_000_000)).collect();
    let b = s.b.bind(&s.ctx(1_000, 10), &BindArgs::new(1_000, 1_900, 8, &cl), K as i64).unwrap();
    LSlot { op: outpoint(101), daa: 810, b, ..s }
}

fn renewal(n: usize) -> (Transaction, Vec<UtxoEntry>) {
    let s = bound(n);
    let ps: Vec<LPair> = (0..n as u8).map(pair).collect();
    let cl: Vec<Claim> = (0..n).map(|i| Claim::input(i, 1_000_000)).collect();
    let a = BindArgs::new(1_800, 2_700, 8, &cl);
    let b2 = s.b.bind(&s.ctx(1_800, 990), &a, K as i64).unwrap();
    let p2: Vec<StubPair> = ps.iter().map(|p| StubPair { expiry: 2_700, ..p.p.clone() }).collect();
    let ks: Vec<i64> = (0..n as i64).collect();
    renew_tx(&ps, &ks, &p2, &s, &a, &b2, 1_800, 990, &uw())
}

#[test]
fn z4_size_and_mass_of_the_bond_paths() {
    let k = kit();
    let bond_len = k.bond_t.prefix.len() + k.bond_t.state_len + k.bond_t.suffix.len();
    let pair_len = k.pair_t.prefix.len() + k.pair_t.state_len + k.pair_t.suffix.len();
    let open_len = k.open_t.prefix.len() + k.open_t.state_len + k.open_t.suffix.len();
    println!("script bytes: KpfBond {bond_len} (1 + {} + {}), KpfPairStub {pair_len}, KpfOpenStub {open_len}", k.bond_t.state_len, k.bond_t.suffix.len());
    assert!(bond_len < 6_000, "within the note's estimate of a shared bond template (about 6 kB)");

    let mut rows: Vec<Row> = vec![];
    let s = slot();
    // bind alone, one claim
    let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 5)]);
    let b2 = s.b.bind(&s.ctx(1_000, 10), &a, K as i64).unwrap();
    let t = bind_alone(&s, &a, &b2, K, 1_000, 10, &uw());
    rows.push(measure("bind (slot alone, 1 claim)", &t.0, &t.1));
    // bind alone, seven claims
    let cl: Vec<Claim> = (0..7).map(|i| Claim::input(i, 5)).collect();
    let a7 = BindArgs::new(1_000, 1_900, 8, &cl);
    let b7 = s.b.bind(&s.ctx(1_000, 10), &a7, K as i64).unwrap();
    let t = bind_alone(&s, &a7, &b7, K, 1_000, 10, &uw());
    rows.push(measure("bind (slot alone, 7 claims)", &t.0, &t.1));
    // open: stand-in order + bind + funder
    let o = StubOpen { m: market(), uw_accept: [0; 32], owner: xkey(&keypair(33)), cover: 300_000_000 };
    let ao = BindArgs::new(1_000, 1_900, 8, &[Claim::output(1, 300_000_000)]);
    let bo = s.b.bind(&s.ctx(1_000, 10), &ao, K as i64).unwrap();
    let p = o.pair_for(SLOT_ID.as_bytes(), xkey(&uw()), 1_900);
    let t = open_tx(&o, Hash::from_bytes([0x53; 32]), 0, &s, &ao, &bo, &p, 1_000, 10, 0, 0);
    rows.push(measure("open (order + bind), T1' stand-in", &t.0, &t.1));
    for n in [1usize, 4, 7] {
        let t = renewal(n);
        rows.push(measure(&format!("renewal of {n} pair(s), one bind"), &t.0, &t.1));
    }
    // advance, split, unbond, withdraw, rely
    let sb = bound(1);
    let ba = sb.b.advance(&sb.ctx(2_100, 0), 11, K as i64).unwrap();
    let t = advance_tx(&sb, 11, &ba, K, 2_100);
    rows.push(measure("advance (anyone)", &t.0, &t.1));
    let mut c = sb.ctx(1_000, 190);
    c.cov_out = 2;
    let mut ra = [0i64; RING];
    ra[10] = 500_000;
    let (x, y) = sb.b.split(&c, &ra, 500_000_000, 500_000_000).unwrap();
    let t = split_tx(&sb, &ra, &x, 500_000_000, &y, 500_000_000, 1_000, 190, &uw());
    rows.push(measure("split", &t.0, &t.1));
    let bu = sb.b.unbond(&sb.ctx(2_100, 1_290), 11, K as i64).unwrap();
    let t = unbond_tx(&sb, 11, &bu, 2_100, 1_290, &uw());
    rows.push(measure("unbond (with 11 steps of advance)", &t.0, &t.1));
    let su = LSlot { b: bu, ..sb.clone() };
    let t = withdraw_tx(&su, 50, &uw(), false);
    rows.push(measure("withdraw", &t.0, &t.1));
    let t = rely_tx(&pair(0));
    rows.push(measure("rely (stand-in pair)", &t.0, &t.1));

    println!("| Transaction | Inputs | Script units per input | Compute mass | Storage mass | Bytes | Fee (sompi) |");
    println!("|---|---|---|---|---|---|---|");
    for r in &rows {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            r.name,
            r.inputs,
            r.units.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(" / "),
            r.compute,
            r.storage,
            r.bytes,
            r.fee
        );
        assert!(r.compute <= COMPUTE_LIMIT && r.storage <= COMPUTE_LIMIT && r.bytes <= BYTES_LIMIT, "{} fits", r.name);
    }
    println!("largest stack {}", rows.iter().map(|r| r.stack).max().unwrap());
}
