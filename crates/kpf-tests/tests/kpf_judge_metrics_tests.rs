//! Numbers of the stake judge (docs/DESIGN-ORACLE-IMPL.md, second slice): script and state bytes; script units,
//! compute and storage mass, bytes and relay fee of every transaction; how many stakers one transaction can pay; the
//! contention of one dispute. Limits as `kpf_metrics_tests` (vendor/kob/docs/spec/matcher.md 3.2): compute and storage
//! mass 500,000 each, 250,000 bytes; relay fee 100 sompi per gram. Run with `--nocapture` for the tables.

mod common;
mod judgekit;

use common::*;
use judgekit::*;
use kaspa_consensus_core::config::params::MAINNET_PARAMS;
use kaspa_consensus_core::mass::{transaction_estimated_serialized_size, ComputeBudget, MassCalculator, ScriptUnits};
use kaspa_consensus_core::tx::{PopulatedTransaction, TransactionInput};
use kpf_protocol::judge::*;

const COMPUTE_LIMIT: u64 = 500_000;
const BYTES_LIMIT: u64 = 250_000;

struct Row {
    name: String,
    inputs: usize,
    units: Vec<u64>,
    compute: u64,
    storage: u64,
    bytes: u64,
    fee: u64,
}

fn measure(name: &str, eng: &Eng, jt: &JTx) -> Row {
    assert!(agree(name, eng, jt), "{name} is accepted");
    let (tx, e) = eng.build(jt);
    let m = run_metered(&tx, &e);
    let mut t = tx.clone();
    let mut units = vec![];
    for (i, r) in m.iter().enumerate() {
        assert!(r.result.is_ok(), "{name} input {i}: {:?}", r.result);
        assert!(r.max_stack < 244, "{name}: stack {}", r.max_stack);
        units.push(r.script_units);
        let b = ComputeBudget::checked_covering_script_units(ScriptUnits(r.script_units)).expect("budget").value();
        let inp = &tx.inputs[i];
        t.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, b);
    }
    let calc = MassCalculator::new_with_consensus_params(&MAINNET_PARAMS);
    let nc = calc.calc_non_contextual_masses(&t);
    let storage = calc.calc_contextual_masses(&PopulatedTransaction::new(&t, e.to_vec())).map(|m| m.storage_mass).unwrap_or(u64::MAX);
    let rep = kpf_protocol::tx::masses(&t, &e);
    let stack = m.iter().map(|r| r.max_stack).max().unwrap_or(0);
    println!("  ({name}: largest stack {stack})");
    Row { name: name.into(), inputs: tx.inputs.len(), units, compute: nc.compute_mass, storage, bytes: transaction_estimated_serialized_size(&t), fee: kpf_protocol::tx::min_fee(&rep, 100) }
}

/// Stakes of 100 KAS (the testnet minimum) on the engine fixtures' judge; the pool's base 1 KAS.
const V: i64 = 10_000_000_000;

#[test]
fn judge_sizes_masses_and_fees() {
    let k = jkit();
    let stake_len = k.stake_t.prefix.len() + k.stake_t.state_len + k.stake_t.suffix.len();
    let claim_len = k.claim_t.prefix.len() + k.claim_t.state_len + k.claim_t.suffix.len();
    println!("script bytes: KpfStake {stake_len} (1 + {} + {}), KpfClaimStub {claim_len} (1 + {} + {})", k.stake_t.state_len, k.stake_t.suffix.len(), k.claim_t.state_len, k.claim_t.suffix.len());
    let mut rows = vec![];
    // Open.
    let mut t = JTable::new();
    let o = t.stake(11, V, 0);
    let tx = t.a.open_tx(t.claim, o, 11, 100_000_000, &Real);
    rows.push(measure("open (claim, opener's stake, funder)", &t.eng, &tx));
    let hs = t.run("open", &tx, ANCHOR).unwrap();
    t.claim = hs[0];
    t.receipts.push(hs[1]);
    t.dispute = Some(hs[2]);
    // Deposits of 1 and 7 stakes (partial answers; the second flips).
    let one = t.stake(20, V, 0);
    let tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &[(one, 20)], SIGHASH_SINGLE_ACP);
    rows.push(measure("deposit, 1 stake", &t.eng, &tx));
    assert!(t.deposit(YES, &[(one, 20)], ANCHOR + 1));
    let seven: Vec<(u64, u8)> = (0..7).map(|i| (t.stake(30 + i, V, 0), 30 + i)).collect();
    let tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &seven, SIGHASH_SINGLE_ACP);
    rows.push(measure("deposit, 7 stakes (takes the lead)", &t.eng, &tx));
    assert!(t.deposit(YES, &seven, ANCHOR + 2));
    // NO answers with 7 more stakes and leads; resolution.
    let back: Vec<(u64, u8)> = (0..7).map(|i| (t.stake(40 + i, 2 * V, 0), 40 + i)).collect();
    assert!(t.deposit(NO, &back, ANCHOR + 3));
    let end = t.window_end();
    let tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end);
    rows.push(measure("resolve and deliver (dispute, claim, funder)", &t.eng, &tx));
    assert!(t.resolve(end));
    // Settlements: 7 losers, then 1 winner, then 7 winners (NO won: the opener and 7 NO stakes).
    let rs = t.receipts.clone();
    let (losers, winners): (Vec<u64>, Vec<u64>) = rs.iter().partition(|h| t.a.js(**h).side == YES);
    let tx = t.a.settle_tx(t.dispute.unwrap(), &losers[..7]);
    rows.push(measure("settle, 7 losing receipts", &t.eng, &tx));
    assert!(t.settle(&losers[..7]).is_some());
    let tx = t.a.settle_tx(t.dispute.unwrap(), &losers[7..]);
    rows.push(measure("settle, 1 losing receipt", &t.eng, &tx));
    assert!(t.settle(&losers[7..]).is_some());
    let tx = t.a.settle_tx(t.dispute.unwrap(), &winners[..1]);
    rows.push(measure("settle, 1 winning receipt", &t.eng, &tx));
    assert!(t.settle(&winners[..1]).is_some());
    let tx = t.a.settle_tx(t.dispute.unwrap(), &winners[1..]);
    rows.push(measure("settle, 7 winning receipts", &t.eng, &tx));
    assert!(t.settle(&winners[1..]).is_some());
    let tx = t.a.close_tx(t.dispute.unwrap());
    rows.push(measure("close (burn)", &t.eng, &tx));
    // Unbond and withdraw.
    let s = t.stake(50, V, 0);
    let tx = t.a.unbond_tx(s, 50);
    rows.push(measure("unbond", &t.eng, &tx));
    let u = t.run("unbond", &tx, ANCHOR).unwrap()[0];
    let tx = t.a.withdraw_tx(u, 50, jparams().unbond);
    rows.push(measure("withdraw", &t.eng, &tx));

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
    // How many stakers one transaction can pay: the unrolled loop pays 7; the mass would allow more.
    let w7 = rows.iter().find(|r| r.name == "settle, 7 winning receipts").unwrap();
    let w1 = rows.iter().find(|r| r.name == "settle, 1 winning receipt").unwrap();
    let per = (w7.compute - w1.compute) / 6;
    let per_b = (w7.bytes - w1.bytes) / 6;
    let by_mass = 1 + (COMPUTE_LIMIT - w1.compute) / per;
    let by_bytes = 1 + (BYTES_LIMIT - w1.bytes) / per_b;
    println!("a winning receipt adds {per} compute mass and {per_b} bytes: by mass alone {} receipts would fit one settlement (the loop pays {BATCH})", by_mass.min(by_bytes));
    // Contention: one dispute's deposits are serialised through its UTXO.
    let d7 = rows.iter().find(|r| r.name == "deposit, 7 stakes (takes the lead)").unwrap();
    for n in [7u64, 100, 1_000] {
        let txs = n.div_ceil(BATCH as u64);
        println!("an answer by {n} stakers: {txs} chained deposits, about {} KAS of relay fees ({} sompi per staker)", (txs * d7.fee) as f64 / 1e8, d7.fee / BATCH as u64);
    }
}
