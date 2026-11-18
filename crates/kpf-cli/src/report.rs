//! Human-readable and JSON reports of a signed transaction: what each input and output is, who pays and receives
//! what, the fee and mass, and the engine verification of every input.

use std::collections::BTreeMap;

use kaspa_consensus_core::tx::UtxoEntry;
use kpf_protocol::network::{spk_address, Network};
use kpf_protocol::tx::*;
use kpf_protocol::verify::Verification;
use serde_json::{json, Value};

use crate::args::fmt_kas;

fn who(spk: &kaspa_consensus_core::tx::ScriptPublicKey, net: Network) -> String {
    spk_address(spk, net).unwrap_or_else(|| "(non-standard script)".into())
}

fn kind_of(e: &UtxoEntry) -> &'static str {
    if p2pk_key(&e.script_public_key).is_some() {
        "key"
    } else {
        "contract"
    }
}

/// The text report.
pub fn text(title: &str, s: &Signed, v: &Verification, net: Network, fee_rate: u64) -> String {
    let mut o = String::new();
    o.push_str(&format!("KPF transaction: {title}   (network {}, version {})\n", net.name(), s.tx.version));
    o.push_str(&format!("txid {}\n\n", s.txid()));
    o.push_str("inputs\n");
    for (i, e) in s.entries.iter().enumerate() {
        let units = v.inputs.get(i).and_then(|r| r.as_ref().ok()).map(|u| format!("{u} script units, budget {}", v.budgets[i])).unwrap_or_else(|| "FAILED".into());
        o.push_str(&format!(
            "  {i:>2}  {:<28} {:>18} KAS  {}  [{units}]\n",
            s.input_labels[i],
            fmt_kas(e.amount as i128),
            short(&who(&e.script_public_key, net))
        ));
    }
    o.push_str("outputs\n");
    for (i, out) in s.tx.outputs.iter().enumerate() {
        o.push_str(&format!(
            "  {i:>2}  {:<28} {:>18} KAS  {}{}\n",
            s.output_labels[i],
            fmt_kas(out.value as i128),
            short(&who(&out.script_public_key, net)),
            if out.covenant.is_some() { "  [covenant]" } else { "" }
        ));
    }
    o.push_str(&format!(
        "\nfee {} KAS at {fee_rate} sompi/gram   mass: compute {}, transient {} (normalized {}), storage {}   size {} bytes\n",
        fmt_kas(s.fee as i128),
        s.mass.compute,
        s.mass.transient,
        s.mass.transient_normalized,
        s.mass.storage,
        s.mass.size
    ));
    // Who pays and receives: net by address, plain keys and contract objects separately.
    let mut net_by: BTreeMap<String, i128> = BTreeMap::new();
    for e in &s.entries {
        *net_by.entry(format!("{} ({})", who(&e.script_public_key, net), kind_of(e))).or_default() -= e.amount as i128;
    }
    for out in &s.tx.outputs {
        let k = if p2pk_key(&out.script_public_key).is_some() { "key" } else { "contract" };
        *net_by.entry(format!("{} ({k})", who(&out.script_public_key, net))).or_default() += out.value as i128;
    }
    o.push_str("\nwho pays and receives (outputs minus inputs; the difference to zero is the network fee)\n");
    for (k, d) in &net_by {
        if *d != 0 {
            o.push_str(&format!("  {:>+22} KAS  {}\n", fmt_kas(*d), short_label(k)));
        }
    }
    o.push_str(&format!("  {:>22} KAS  network fee\n", fmt_kas(-(net_by.values().sum::<i128>()))));
    o.push('\n');
    if v.is_ok() {
        o.push_str(&format!("verification: every one of the {} inputs passed the script engine (consensus metering, covenant context, mass and fee checks)\n", s.tx.inputs.len()));
    } else {
        o.push_str("verification FAILED:\n");
        for e in v.errors() {
            o.push_str(&format!("  {e}\n"));
        }
    }
    o
}

fn short(a: &str) -> String {
    if a.len() > 30 && a.contains(':') {
        format!("{}...{}", &a[..16], &a[a.len() - 8..])
    } else {
        a.to_string()
    }
}

fn short_label(k: &str) -> String {
    match k.split_once(" (") {
        Some((a, rest)) => format!("{} ({}", short(a), rest),
        None => k.to_string(),
    }
}

/// The transaction file.
pub fn json_doc(title: &str, s: &Signed, v: &Verification, net: Network, created_files: &[String]) -> Value {
    let verification: Vec<Value> = v
        .inputs
        .iter()
        .enumerate()
        .map(|(i, r)| match r {
            Ok(u) => json!({"input": i, "ok": true, "scriptUnits": u, "computeBudget": v.budgets[i]}),
            Err(e) => json!({"input": i, "ok": false, "error": e}),
        })
        .collect();
    json!({
        "format": "kpf-tx/1",
        "title": title,
        "network": net.name(),
        "txid": s.txid().to_string(),
        "fee": s.fee,
        "mass": {"compute": s.mass.compute, "transient": s.mass.transient, "transientNormalized": s.mass.transient_normalized, "storage": s.mass.storage, "size": s.mass.size},
        "rpc": rpc_json(&s.tx),
        "hex": tx_hex(&s.tx),
        "entries": s.entries.iter().map(entry_json).collect::<Vec<_>>(),
        "inputs": s.input_labels,
        "outputs": s.output_labels,
        "created": created_files,
        "verification": {"ok": v.is_ok(), "inputs": verification, "errors": v.errors()},
    })
}
