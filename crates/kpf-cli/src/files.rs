//! The JSON files the command line reads and writes: objects (a pair or an order with its UTXO), UTXO lists, markets
//! and transaction files.

use std::path::Path;

use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::error::{from_hex, hex32, to_hex};
use kpf_protocol::network::{p2sh_address, Network};
use kpf_protocol::state::{decode_redeem, Decoded, Market};
use kpf_protocol::tx::*;
use serde_json::{json, Value};

pub fn read_json(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path}: not JSON: {e}"))
}

pub fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v).expect("json") + "\n").map_err(|e| format!("{}: {e}", path.display()))
}

pub fn outpoint_json(op: &TransactionOutpoint) -> Value {
    json!({ "txid": op.transaction_id.to_string(), "index": op.index })
}

pub fn outpoint_from(v: &Value) -> Result<TransactionOutpoint, String> {
    let txid = v.get("txid").or_else(|| v.get("transactionId")).and_then(Value::as_str).ok_or("outpoint: no txid")?;
    let index = v.get("index").and_then(Value::as_u64).ok_or("outpoint: no index")?;
    Ok(TransactionOutpoint { transaction_id: TransactionId::from_bytes(hex32(txid).map_err(|e| e.to_string())?), index: index as u32 })
}

/// A KPF object with its UTXO: what a command needs to spend a pair or an order.
#[derive(Clone, Debug)]
pub struct ObjFile {
    pub op: TransactionOutpoint,
    pub amount: u64,
    pub daa: u64,
    pub covenant_id: Hash,
    pub redeem: Vec<u8>,
}

impl ObjFile {
    pub fn load(path: &str) -> Result<ObjFile, String> {
        let v = read_json(path)?;
        ObjFile::from_json(&v).map_err(|e| format!("{path}: {e}"))
    }

    pub fn from_json(v: &Value) -> Result<ObjFile, String> {
        let g = |k: &str| v.get(k).ok_or_else(|| format!("no `{k}`"));
        Ok(ObjFile {
            op: outpoint_from(g("outpoint")?)?,
            amount: g("amount")?.as_u64().ok_or("`amount` is not an integer")?,
            daa: v.get("daa").and_then(Value::as_u64).unwrap_or(0),
            covenant_id: Hash::from_bytes(hex32(g("covenantId")?.as_str().unwrap_or("")).map_err(|e| format!("covenantId: {e}"))?),
            redeem: from_hex(g("redeem")?.as_str().unwrap_or("")).map_err(|e| format!("redeem: {e}"))?,
        })
    }

    pub fn decoded(&self) -> Result<Decoded, String> {
        decode_redeem(&self.redeem).map_err(|e| e.to_string())
    }

    pub fn live_pair(&self) -> Result<LivePair, String> {
        match self.decoded()? {
            Decoded::Pair(p) => Ok(LivePair { p, value: self.amount, id: self.covenant_id, op: self.op, daa: self.daa }),
            Decoded::Open(_) => Err("this object is an order, not a pair".into()),
        }
    }

    pub fn live_open(&self) -> Result<LiveOpen, String> {
        match self.decoded()? {
            Decoded::Open(o) => Ok(LiveOpen { o, value: self.amount, id: self.covenant_id, op: self.op, daa: self.daa }),
            Decoded::Pair(_) => Err("this object is a pair, not an order".into()),
        }
    }

    pub fn spk(&self) -> ScriptPublicKey {
        p2sh_spk(&self.redeem)
    }

    pub fn to_json(&self, net: Network) -> Value {
        let (kind, state) = match self.decoded() {
            Ok(Decoded::Pair(p)) => ("pair", p.to_json()),
            Ok(Decoded::Open(o)) => ("order", o.to_json()),
            Err(_) => ("unknown", Value::Null),
        };
        json!({
            "format": "kpf-object/1", "kind": kind, "network": net.name(), "outpoint": outpoint_json(&self.op),
            "amount": self.amount, "daa": self.daa, "covenantId": self.covenant_id.to_string(),
            "address": p2sh_address(&self.redeem, net), "redeem": to_hex(&self.redeem), "state": state,
        })
    }
}

/// A UTXO as listed in a funding file or reported by a node.
#[derive(Clone, Debug)]
pub struct ListedUtxo {
    pub op: TransactionOutpoint,
    pub amount: u64,
    pub daa: u64,
    pub spk: Option<ScriptPublicKey>,
    pub covenant_id: Option<Hash>,
}

/// Parses a UTXO list: an array of `{txid, index, amount, daa}` or of the node's `{outpoint, utxoEntry}` entries
/// (or an object with an `entries` array).
pub fn parse_utxos(v: &Value) -> Result<Vec<ListedUtxo>, String> {
    let arr = match v {
        Value::Array(a) => a.as_slice(),
        Value::Object(o) if o.get("entries").is_some_and(Value::is_array) => o["entries"].as_array().expect("array").as_slice(),
        _ => return Err("expected a JSON array of UTXOs".into()),
    };
    arr.iter()
        .enumerate()
        .map(|(i, e)| {
            let ctx = |m: String| format!("UTXO {i}: {m}");
            if let (Some(op), Some(u)) = (e.get("outpoint"), e.get("utxoEntry")) {
                let spk = u.get("scriptPublicKey").map(spk_value).transpose().map_err(|e| ctx(e.to_string()))?;
                let cov = match u.get("covenantId") {
                    None | Some(Value::Null) => None,
                    Some(c) => Some(Hash::from_bytes(hex32(c.as_str().unwrap_or("")).map_err(|e| ctx(e.to_string()))?)),
                };
                return Ok(ListedUtxo {
                    op: outpoint_from(op).map_err(ctx)?,
                    amount: num(u.get("amount")).ok_or_else(|| ctx("no amount".into()))?,
                    daa: num(u.get("blockDaaScore")).unwrap_or(0),
                    spk,
                    covenant_id: cov,
                });
            }
            Ok(ListedUtxo {
                op: outpoint_from(e).map_err(ctx)?,
                amount: num(e.get("amount")).ok_or_else(|| ctx("no amount".into()))?,
                daa: num(e.get("daa").or_else(|| e.get("blockDaaScore"))).unwrap_or(0),
                spk: None,
                covenant_id: None,
            })
        })
        .collect()
}

/// A script public key as the node reports it: `"<version><script>"` hex, or `{version, script}`.
fn spk_value(v: &Value) -> kpf_protocol::Result<ScriptPublicKey> {
    match v {
        Value::String(s) => spk_from_hex(s),
        Value::Object(o) => {
            let version = o.get("version").and_then(Value::as_u64).unwrap_or(0) as u16;
            let script = from_hex(o.get("script").and_then(Value::as_str).unwrap_or(""))?;
            Ok(ScriptPublicKey::new(version, script.into()))
        }
        _ => Err(kpf_protocol::Error::Invalid("scriptPublicKey is neither a string nor an object".into())),
    }
}

fn num(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// The plain UTXOs of `key` among `list`: P2PK to the key (when the script is known) and carrying no covenant id.
pub fn funding_utxos(list: &[ListedUtxo], key: &[u8; 32]) -> Vec<KeyUtxo> {
    list.iter()
        .filter(|u| u.covenant_id.is_none() && u.spk.as_ref().map(|s| p2pk_key(s).as_ref() == Some(key)).unwrap_or(true))
        .map(|u| KeyUtxo { outpoint: u.op, amount: u.amount, daa: u.daa, key: *key })
        .collect()
}

pub fn market_from_file(path: &str) -> Result<Market, String> {
    let v = read_json(path)?;
    let v = v.get("market").unwrap_or(&v);
    Market::from_json(v).map_err(|e| format!("{path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kpf_protocol::fixtures::*;

    #[test]
    fn object_file_round_trips() {
        let p = pair(pubkey(1), pubkey(2));
        let o = ObjFile {
            op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([7; 32]), index: 2 },
            amount: p.floor() as u64,
            daa: 55,
            covenant_id: Hash::from_bytes([9; 32]),
            redeem: p.redeem(),
        };
        let j = o.to_json(Network::Testnet10);
        assert_eq!(j["kind"], "pair");
        let back = ObjFile::from_json(&j).unwrap();
        assert_eq!(back.op, o.op);
        assert_eq!(back.live_pair().unwrap().p, p);
        assert!(back.live_open().is_err());
    }

    #[test]
    fn utxo_lists_in_both_shapes() {
        let v = json!([
            {"txid": "11".repeat(32), "index": 1, "amount": 500, "daa": 9},
            {"outpoint": {"transactionId": "22".repeat(32), "index": 0},
             "utxoEntry": {"amount": "700", "scriptPublicKey": spk_hex(&p2pk_spk(&pubkey(3))), "blockDaaScore": 4, "covenantId": null}},
            {"outpoint": {"transactionId": "33".repeat(32), "index": 0},
             "utxoEntry": {"amount": 800, "scriptPublicKey": spk_hex(&p2pk_spk(&pubkey(3))), "blockDaaScore": 4, "covenantId": "44".repeat(32)}},
        ]);
        let l = parse_utxos(&v).unwrap();
        assert_eq!(l.len(), 3);
        let f = funding_utxos(&l, &pubkey(3));
        assert_eq!(f.iter().map(|u| u.amount).collect::<Vec<_>>(), vec![500, 700], "a covenant UTXO is not funding");
        assert!(funding_utxos(&l, &pubkey(4)).iter().all(|u| u.amount == 500), "known scripts of other keys are skipped");
        assert!(parse_utxos(&json!({"x": 1})).is_err());
    }
}
