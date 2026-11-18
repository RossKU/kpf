//! Size and compute of every path of KpfPair and KpfOpen (DESIGN 8, X4 for this slice), and of the worst
//! transaction built so far: a block of 8 liquidations on one KOB evidence fill.
//!
//! Columns: the template's script bytes, the input's signature-script bytes (arguments + redeem script), the script
//! units the input uses under consensus engine flags (signature checks charged), and the largest combined stack
//! (limit 244, `crypto/txscript/src/lib.rs:77-80` of rusty-kaspa v2.1.0). The transaction rows add the compute mass
//! of rusty-kaspa's own calculator with every input committing the smallest compute budget that covers it, and the
//! estimated serialized size, against the limits of `vendor/kob/docs/spec/matcher.md` section 3.2 (compute 500,000;
//! at most 250,000 bytes).
//!
//! Run with `--nocapture` to print the tables.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::config::params::MAINNET_PARAMS;
use kaspa_consensus_core::mass::{transaction_estimated_serialized_size, ComputeBudget, MassCalculator, ScriptUnits};
use kaspa_consensus_core::tx::{PopulatedTransaction, Transaction, TransactionInput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const COMPUTE_LIMIT: u64 = 500_000;
const BYTES_LIMIT: u64 = 250_000;

fn f() -> Keypair {
    keypair(14)
}

fn base() -> Pair {
    pair(&keypair(11), &keypair(12))
}

struct Row {
    name: String,
    script: usize,
    sigscript: usize,
    units: u64,
    stack: usize,
}

fn measure(rows: &mut Vec<Row>, name: &str, tx: &Transaction, e: &[UtxoEntry], idx: usize, script: usize) {
    let r = &run_metered(tx, e)[idx];
    assert!(r.result.is_ok(), "{name}: {:?}", r.result);
    assert!(r.max_stack < 244, "{name}: stack {}", r.max_stack);
    rows.push(Row { name: name.to_string(), script, sigscript: tx.inputs[idx].signature_script.len(), units: r.script_units, stack: r.max_stack });
}

fn pair_len() -> usize {
    bytecode(pair_base()).len()
}

fn open_len() -> usize {
    bytecode(open_base()).len()
}

/// Compute mass (budgets covering each input's measured units) and estimated size of an accepted transaction.
fn tx_mass(tx: &Transaction, e: &[UtxoEntry]) -> (u64, u64, u64) {
    let m = run_metered(tx, e);
    let mut t = tx.clone();
    let mut total = 0;
    for (i, r) in m.iter().enumerate() {
        assert!(r.result.is_ok(), "input {i}: {:?}", r.result);
        total += r.script_units;
        let b = ComputeBudget::checked_covering_script_units(ScriptUnits(r.script_units)).expect("budget").value();
        let inp = &tx.inputs[i];
        t.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, b);
    }
    let calc = MassCalculator::new_with_consensus_params(&MAINNET_PARAMS);
    let nc = calc.calc_non_contextual_masses(&t);
    (total, nc.compute_mass, transaction_estimated_serialized_size(&t))
}

fn listed(p: &Pair) -> Pair {
    let mut l = p.clone();
    (l.long_ask, l.long_min, l.long_tip) = (1_000_000_000, 100_000_000, 0);
    (l.short_bid, l.short_min, l.short_tip) = (1_100_000_000, 100_000_000, 0);
    l
}

fn open_live(o: Open, tag: u8) -> LiveOpen {
    LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([tag; 32]), op: outpoint(tag) }
}

#[test]
fn metrics_every_path() {
    let mut rows = vec![];
    let fk = f();
    let l = live(&base(), 0x31);
    let v = l.value;

    // KpfPair evidence-free paths.
    let d = amend_draft(&l, &keypair(11), LONG, (1_000_000_000, 100_000_000, 0), &fk, 1);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P1 amend", &tx, &e, 0, pair_len());

    let delta = 100_000_000;
    let mut m = l.p.cleared();
    m.bl -= delta;
    let need = q(l.p.size, delta, SCALE, SCALE - 1) as u64;
    let ins = [
        pair_input(&l, "addMargin", vec![Arg::Sig(keypair(11)), iv(LONG), iv(delta)]),
        funder(&fk, 2, F_FUNDS),
    ];
    let (tx, e) = build(&ins, vec![out(v + need, p2sh(&m.art()), bind(0, l.id)), out(F_FUNDS - need - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfPair P2 addMargin", &tx, &e, 0, pair_len());

    let ll = live(&listed(&base()), 0x32);
    let lo = open_live(long_order(&keypair(21)), 0x71);
    let x = 1_000_000_000;
    let t = TransferSpec { side: LONG, n: 400_000_000, x, nb: x - lo.o.my_rate, ek: lo.o.owner, et: lo.id, k: 0 };
    let d = transfer_draft(&ll, &t, &Entrant::Open(lo.clone()), &fk, 3);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P3 transfer (part)", &tx, &e, 0, pair_len());
    measure(&mut rows, "KpfOpen take (T2)", &tx, &e, 1, open_len());

    let host = live(&listed(&base()), 0x33);
    let mut dp = listed(&base());
    (dp.bl, dp.bs) = (850_000_000, 1_150_000_000);
    let donor = live(&dp, 0x34);
    let d = close_draft(&host, &donor, &CloseSpec { n: 400_000_000, x: 1_050_000_000, e3: 1_000_000_000 }, &fk, 4);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P4 closeHost (part)", &tx, &e, 0, pair_len());
    measure(&mut rows, "KpfPair P4 closeDonor (part)", &tx, &e, 1, pair_len());

    let d = mutual_draft(&ll, 400_000_000, 1_050_000_000, &fk, 5);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P4 mutualClose (part)", &tx, &e, 0, pair_len());

    // Evidence paths, partial (the larger shape: a remainder continuation).
    let n = 400_000_000;
    let shapes: Vec<(&str, PairLeg, Evidence)> = vec![
        ("KpfPair P5 close long @bid", PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side: LONG, signer: keypair(11) } }, Evidence::bid(1_050_000_000, n)),
        ("KpfPair P5 close short @ask", PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side: SHORT, signer: keypair(12) } }, Evidence::ask(1_100_000_000, n)),
        ("KpfPair P6 liquidate long @ask", PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side: LONG } }, Evidence::ask(940_000_000, n)),
        ("KpfPair P6 liquidate short @bid", PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side: SHORT } }, Evidence::bid(1_150_000_000, n)),
        (
            "KpfPair P7 liq-transfer long",
            PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side: LONG, x: 950_000_000, open: lo.clone(), k: 0 } },
            Evidence::ask(940_000_000, n),
        ),
        (
            "KpfPair P7 liq-transfer short",
            PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side: SHORT, x: 1_140_000_000, open: open_live(short_order(&keypair(22)), 0x72), k: 0 } },
            Evidence::bid(1_150_000_000, n),
        ),
    ];
    let mut kob_rows = vec![];
    for (name, g, ev) in shapes {
        let is_p7 = matches!(g.act, Act::LiqTransfer { .. });
        let kind = ev.kind;
        let d = ev_draft(&[g], &ev, &fk, 6);
        let (tx, e) = d.build();
        measure(&mut rows, name, &tx, &e, 0, pair_len());
        if is_p7 {
            measure(&mut rows, &format!("KpfOpen take ({})", &name[8..]), &tx, &e, 3, open_len());
        }
        let (u, c, b) = tx_mass(&tx, &e);
        kob_rows.push((format!("{name} ({kind:?} evidence)"), tx.inputs.len(), u, c, b));
    }

    let (lp, sp) = idle_out(&l);
    let _ = (lp, sp);
    let d = idle_draft(&l, &fk, 7);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P8 idleSettle", &tx, &e, 0, pair_len());

    let ins = [
        In {
            outpoint: l.op,
            utxo: UtxoEntry::new(v, p2sh(&l.p.art()), 0, false, Some(l.id)),
            spend: Spend::Signed { art: l.p.art(), entry: "coopClose", args: vec![Arg::Sig(keypair(11)), Arg::Sig(keypair(12))] },
        },
        funder(&fk, 8, F_FUNDS),
    ];
    let (tx, e) = build(&ins, vec![out(v, p2pk_spk(&keypair(11)), None), out(F_FUNDS - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfPair P9 coopClose", &tx, &e, 0, pair_len());

    // KpfOpen.
    let so = open_live(short_order(&keypair(22)), 0x73);
    let d = open_draft(&lo, &so, 400_000_000, 1_000_000_000, &fk, 9);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfOpen fill (T1, long)", &tx, &e, 0, open_len());
    measure(&mut rows, "KpfOpen fill (T1, short)", &tx, &e, 1, open_len());
    let ins = [open_input(&lo, "cancel", vec![Arg::Sig(keypair(21))]), funder(&fk, 10, F_FUNDS)];
    let (tx, e) = build(&ins, vec![out(lo.value, p2pk_x(&lo.o.owner), None), out(F_FUNDS - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfOpen cancel", &tx, &e, 0, open_len());
    let mut ro = lo.clone();
    ro.o.expiry_daa = 0;
    let ins = [open_input(&ro, "refund", vec![]), funder(&fk, 11, F_FUNDS)];
    let (tx, e) = build(&ins, vec![out(ro.value, p2pk_x(&ro.o.owner), None), out(F_FUNDS - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfOpen refund", &tx, &e, 0, open_len());

    println!("{:<36} {:>7} {:>10} {:>13} {:>6}", "path (input)", "script", "sigscript", "script units", "stack");
    for r in &rows {
        println!("{:<36} {:>7} {:>10} {:>13} {:>6}", r.name, r.script, r.sigscript, r.units, r.stack);
    }
    println!();
    println!("{:<48} {:>6} {:>13} {:>9} {:>8}", "whole transaction", "inputs", "script units", "compute", "bytes");
    for (name, ins, u, c, b) in &kob_rows {
        println!("{name:<48} {ins:>6} {u:>13} {c:>9} {b:>8}");
        assert!(*c <= COMPUTE_LIMIT && *b <= BYTES_LIMIT);
    }
}

/// The worst shape: 8 liquidations (partial, each with a remainder) on one ask fill with its custody and buyer.
#[test]
fn metrics_eight_liquidations_on_one_evidence() {
    let n = 500_000_000;
    let legs: Vec<PairLeg> = (0..8).map(|i| PairLeg { l: live(&base(), 0x50 + i as u8), n, q: 0, act: Act::Liq { side: LONG } }).collect();
    let mut d = ev_draft(&legs, &Evidence::ask(940_000_000, 8 * n), &f(), 1);
    // The loser's slot of each pair (a liquidated long is owed 0, so no covenant constrains it) holds a 1 KAS filler,
    // as the library's builder writes it: a zero-value output has no storage mass (KIP-9 divides by the value).
    for (i, o) in d.outs.iter_mut().enumerate() {
        if o.value == 0 && Some(i) != d.change {
            o.value = 100_000_000;
        }
    }
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let m = run_metered(&tx, &e);
    for (i, r) in m.iter().enumerate() {
        println!("input {i:>2}: sigscript {:>6} B  script units {:>9}  stack {:>3}", tx.inputs[i].signature_script.len(), r.script_units, r.max_stack);
        assert!(r.max_stack < 244);
    }
    let (u, c, b) = tx_mass(&tx, &e);
    let calc = MassCalculator::new_with_consensus_params(&MAINNET_PARAMS);
    let storage = calc.calc_contextual_masses(&PopulatedTransaction::new(&tx, e.clone())).map(|m| m.storage_mass).expect("storage mass");
    println!(
        "8 liquidations on one evidence: {} inputs, {} outputs, script units {u}, compute mass {c} / {COMPUTE_LIMIT}, storage mass {storage} / {COMPUTE_LIMIT}, bytes {b} / {BYTES_LIMIT}",
        tx.inputs.len(),
        tx.outputs.len()
    );
    assert!(c <= COMPUTE_LIMIT, "compute mass {c} above the limit");
    assert!(storage <= COMPUTE_LIMIT, "storage mass {storage} above the limit");
    assert!(b <= BYTES_LIMIT, "size {b} above the limit");
}
