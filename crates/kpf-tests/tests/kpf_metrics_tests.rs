//! Size and compute of every path of KpfPair, KpfPending and KpfOpen (DESIGN 8, X4; DESIGN 4.5d Y2), and of the
//! evidence transactions: a proposal, a challenge, a cure, a finalize, and the largest block of proposals on one
//! evidence fill that fits the limits.
//!
//! Columns: the template's script bytes, the input's signature-script bytes (arguments + redeem script), the script
//! units the input uses under consensus engine flags (signature checks charged), and the largest combined stack
//! (limit 244, `crypto/txscript/src/lib.rs:77-80` of rusty-kaspa v2.1.0). The transaction rows add the compute mass
//! of rusty-kaspa's own calculator with every input committing the smallest compute budget that covers it, the
//! storage mass (KIP-9), the estimated serialized size, and the relay fee at 100 sompi per gram, against the limits of
//! `vendor/kob/docs/spec/matcher.md` section 3.2 (compute and storage 500,000 each; at most 250,000 bytes).
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
const RATE: u64 = 100;

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

fn pending_len() -> usize {
    bytecode(kpf_protocol::artifacts::pending_base()).len()
}

/// A whole transaction: script units, compute mass (budgets covering each input's measured units), storage mass,
/// estimated size, and the relay fee (100 sompi per gram of the larger of compute and transient mass).
struct TxRow {
    name: String,
    inputs: usize,
    units: u64,
    compute: u64,
    storage: u64,
    bytes: u64,
    fee: u64,
}

fn tx_row(name: &str, tx: &Transaction, e: &[UtxoEntry]) -> TxRow {
    let m = run_metered(tx, e);
    let mut t = tx.clone();
    let mut total = 0;
    for (i, r) in m.iter().enumerate() {
        assert!(r.result.is_ok(), "{name} input {i}: {:?}", r.result);
        total += r.script_units;
        let b = ComputeBudget::checked_covering_script_units(ScriptUnits(r.script_units)).expect("budget").value();
        let inp = &tx.inputs[i];
        t.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, b);
    }
    let calc = MassCalculator::new_with_consensus_params(&MAINNET_PARAMS);
    let nc = calc.calc_non_contextual_masses(&t);
    let storage = calc.calc_contextual_masses(&PopulatedTransaction::new(&t, e.to_vec())).map(|m| m.storage_mass).unwrap_or(u64::MAX);
    let rep = kpf_protocol::tx::masses(&t, e);
    TxRow {
        name: name.to_string(),
        inputs: tx.inputs.len(),
        units: total,
        compute: nc.compute_mass,
        storage,
        bytes: transaction_estimated_serialized_size(&t),
        fee: kpf_protocol::tx::min_fee(&rep, RATE),
    }
}

fn fits(r: &TxRow) -> bool {
    r.compute <= COMPUTE_LIMIT && r.storage <= COMPUTE_LIMIT && r.bytes <= BYTES_LIMIT
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

/// The loser's or the other party's slot of an evidence draft may hold zero (no covenant constrains it); a zero-value
/// output has infinite storage mass (KIP-9 divides by the value), so such slots carry a 1 KAS filler, as the library's
/// builder writes them.
fn filled(mut d: Draft) -> Draft {
    for (i, o) in d.outs.iter_mut().enumerate() {
        if o.value == 0 && Some(i) != d.change {
            o.value = 100_000_000;
        }
    }
    d
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn close(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer: keypair(if side == LONG { 11 } else { 12 }) } }
}

#[test]
fn metrics_every_path() {
    let mut rows = vec![];
    let mut txs = vec![];
    let fk = f();
    let l = live(&base(), 0x31).at(1_000);
    let v = l.value;

    // KpfPair evidence-free paths.
    let d = amend_draft(&l, &keypair(11), LONG, (1_000_000_000, 100_000_000, 0), &fk, 1);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P1 amend", &tx, &e, 0, pair_len());

    let delta = 100_000_000;
    let mut m = l.clocked().cleared();
    m.bl -= delta;
    let need = q(l.p.size, delta, SCALE, SCALE - 1) as u64;
    let ins = [pair_input(&l, "addMargin", vec![Arg::Sig(keypair(11)), iv(LONG), iv(delta)]), funder(&fk, 2, F_FUNDS)];
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

    // Proposals (partial: the larger shape, a remainder continuation) and their second steps.
    let n = 400_000_000;
    let shapes: Vec<(&str, PairLeg, Evidence, Evidence)> = vec![
        ("P5 close long @bid", close(&l, n, LONG), Evidence::bid(1_050_000_000, n), Evidence::ask(1_029_000_000, n)),
        ("P5 close short @ask", close(&l, n, SHORT), Evidence::ask(1_100_000_000, n), Evidence::bid(1_122_000_000, n)),
        ("P6 liquidate long @ask", liq(&l, n, LONG), Evidence::ask(940_000_000, n), Evidence::bid(966_315_790, n)),
        ("P6 liquidate short @bid", liq(&l, n, SHORT), Evidence::bid(1_150_000_000, n), Evidence::ask(1_120_000_000, n)),
    ];
    for (name, g, e, c) in shapes {
        let kind = e.kind;
        let d = filled(ev_draft(&[g.clone()], &e, &fk, 6));
        let (tx, en) = d.build();
        measure(&mut rows, &format!("KpfPair propose {name}"), &tx, &en, 0, pair_len());
        txs.push(tx_row(&format!("propose {name} ({kind:?})"), &tx, &en));
        let w = pend(&g, e.price());
        let d = filled(ch_draft(&[w.clone()], &c, &fk, 7));
        let (tx, en) = d.build();
        measure(&mut rows, &format!("KpfPending challenge {name}"), &tx, &en, 0, pending_len());
        txs.push(tx_row(&format!("challenge {name} ({:?})", c.kind), &tx, &en));
        let d = filled(finalize_draft(&w, &fk, 8));
        let (tx, en) = d.build();
        measure(&mut rows, &format!("KpfPending finalize {name}"), &tx, &en, 0, pending_len());
        txs.push(tx_row(&format!("finalize {name}"), &tx, &en));
        if w.w.kind == KIND_LIQ {
            let signer = keypair(if w.w.side == LONG { 11 } else { 12 });
            let d = cure_draft(&w, &signer, &fk, 9);
            let (tx, en) = d.build();
            measure(&mut rows, &format!("KpfPending cure {name}"), &tx, &en, 0, pending_len());
            txs.push(tx_row(&format!("cure {name}"), &tx, &en));
        }
    }

    let d = idle_draft(&l, &fk, 10);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfPair P8 idleSettle", &tx, &e, 0, pair_len());

    let ins = [
        In {
            outpoint: l.op,
            utxo: UtxoEntry::new(v, p2sh(&l.p.art()), 0, false, Some(l.id)),
            spend: Spend::Signed { art: l.p.art(), entry: "coopClose", args: vec![Arg::Sig(keypair(11)), Arg::Sig(keypair(12))] },
        },
        funder(&fk, 11, F_FUNDS),
    ];
    let (tx, e) = build(&ins, vec![out(v, p2pk_spk(&keypair(11)), None), out(F_FUNDS - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfPair P9 coopClose", &tx, &e, 0, pair_len());

    // KpfOpen.
    let so = open_live(short_order(&keypair(22)), 0x73);
    let d = open_draft(&lo, &so, 400_000_000, 1_000_000_000, &fk, 12);
    let (tx, e) = d.build();
    measure(&mut rows, "KpfOpen fill (T1, long)", &tx, &e, 0, open_len());
    measure(&mut rows, "KpfOpen fill (T1, short)", &tx, &e, 1, open_len());
    let ins = [open_input(&lo, "cancel", vec![Arg::Sig(keypair(21))]), funder(&fk, 13, F_FUNDS)];
    let (tx, e) = build(&ins, vec![out(lo.value, p2pk_x(&lo.o.owner), None), out(F_FUNDS - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfOpen cancel", &tx, &e, 0, open_len());
    let mut ro = lo.clone();
    ro.o.expiry_daa = 0;
    let ins = [open_input(&ro, "refund", vec![]), funder(&fk, 14, F_FUNDS)];
    let (tx, e) = build(&ins, vec![out(ro.value, p2pk_x(&ro.o.owner), None), out(F_FUNDS - FEE, p2pk_spk(&fk), None)]);
    measure(&mut rows, "KpfOpen refund", &tx, &e, 0, open_len());

    println!("state bytes: KpfPair {}, KpfOpen {}, KpfPending {}", l.p.state_bytes().len(), lo.o.state_bytes().len(), pend(&liq(&l, n, LONG), 940_000_000).w.state_bytes().len());
    println!("{:<44} {:>7} {:>10} {:>13} {:>6}", "path (input)", "script", "sigscript", "script units", "stack");
    for r in &rows {
        println!("{:<44} {:>7} {:>10} {:>13} {:>6}", r.name, r.script, r.sigscript, r.units, r.stack);
    }
    println!();
    println!("{:<44} {:>6} {:>13} {:>9} {:>9} {:>8} {:>12}", "whole transaction", "inputs", "script units", "compute", "storage", "bytes", "fee (sompi)");
    for r in &txs {
        println!("{:<44} {:>6} {:>13} {:>9} {:>9} {:>8} {:>12}", r.name, r.inputs, r.units, r.compute, r.storage, r.bytes, r.fee);
        assert!(fits(r), "{} does not fit", r.name);
    }
}

/// The largest block of proposals on one evidence fill (DESIGN 4.5d Y2): partial liquidations, each with a remainder.
/// The scan of the block is unrolled to 8; the largest size that fits every limit is printed and must be at least 4.
#[test]
fn metrics_largest_proposal_block() {
    let n = 500_000_000;
    let mut largest = 0;
    for k in 1..=8usize {
        let legs: Vec<PairLeg> = (0..k).map(|i| liq(&live(&base(), 0x50 + i as u8).at(1_000), n, LONG)).collect();
        let d = filled(ev_draft(&legs, &Evidence::ask(940_000_000, k as i64 * n), &f(), 1));
        let (tx, e) = d.build();
        assert_accepted(&tx, &e);
        let r = tx_row(&format!("{k} proposals on one evidence"), &tx, &e);
        let m = run_metered(&tx, &e);
        assert!(m.iter().all(|x| x.max_stack < 244));
        println!(
            "{k} proposals: {} inputs, {} outputs, script units {}, compute {} / {COMPUTE_LIMIT}, storage {} / {COMPUTE_LIMIT}, bytes {} / {BYTES_LIMIT}, fee {} sompi{}",
            r.inputs,
            tx.outputs.len(),
            r.units,
            r.compute,
            r.storage,
            r.bytes,
            r.fee,
            if fits(&r) { "" } else { "  (does not fit)" }
        );
        if fits(&r) {
            largest = k;
        }
    }
    println!("largest proposal block that fits: {largest}");
    assert!(largest >= 4, "a block of at least 4 proposals must fit");
    // The largest challenge block: 8 pending parts on one counter fill.
    let ws: Vec<LivePend> = (0..8).map(|i| pend(&liq(&live(&base(), 0x60 + i as u8).at(1_000), n, LONG), 940_000_000)).collect();
    let d = filled(ch_draft(&ws, &Evidence::bid(966_315_790, 8 * n), &f(), 2));
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let r = tx_row("8 challenges on one counter-evidence", &tx, &e);
    println!("8 challenges: {} inputs, script units {}, compute {}, storage {}, bytes {}, fee {}", r.inputs, r.units, r.compute, r.storage, r.bytes, r.fee);
    assert!(fits(&r));
}
