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
    /// The UTXO's DAA score; `None` when the file does not say (a just-built output, or a hand-written file). Every
    /// use that depends on it (the evidence rest, the evidence clock, the hand-over limit, an order's refund time)
    /// refuses an unknown DAA instead of reading it as 0 (review 3 T4).
    pub daa: Option<u64>,
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
            daa: match v.get("daa") {
                None | Some(Value::Null) => None,
                Some(d) => Some(d.as_u64().ok_or("`daa` is not a non-negative integer")?),
            },
            covenant_id: Hash::from_bytes(hex32(g("covenantId")?.as_str().unwrap_or("")).map_err(|e| format!("covenantId: {e}"))?),
            redeem: from_hex(g("redeem")?.as_str().unwrap_or("")).map_err(|e| format!("redeem: {e}"))?,
        })
    }

    /// The UTXO's DAA score, or a plain refusal when the file does not give it.
    pub fn known_daa(&self) -> Result<u64, String> {
        self.daa.ok_or_else(|| {
            format!(
                "the file of UTXO {}:{} has no `daa` (its DAA score is unknown, e.g. it was written before the transaction was accepted): pass --node, or run `kpf status --object FILE --node URL --update` once it is accepted. It is not read as 0: the evidence rest, the evidence clock and the hand-over limit depend on it",
                self.op.transaction_id, self.op.index
            )
        })
    }

    pub fn decoded(&self) -> Result<Decoded, String> {
        decode_redeem(&self.redeem).map_err(|e| e.to_string())
    }

    pub fn live_pair(&self) -> Result<LivePair, String> {
        match self.decoded()? {
            Decoded::Pair(p) => Ok(LivePair { p, value: self.amount, id: self.covenant_id, op: self.op, daa: self.known_daa()? }),
            Decoded::Open(_) => Err("this object is an order, not a pair".into()),
        }
    }

    pub fn live_open(&self) -> Result<LiveOpen, String> {
        match self.decoded()? {
            Decoded::Open(o) => Ok(LiveOpen { o, value: self.amount, id: self.covenant_id, op: self.op, daa: self.known_daa()? }),
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
            Err(_) => match kpf_protocol::kob::KobObj::decode(&self.redeem) {
                Ok(kpf_protocol::kob::KobObj::Ask(a)) => ("ask", a.to_json()),
                Ok(kpf_protocol::kob::KobObj::Bid(b)) => ("bid", b.to_json()),
                Ok(kpf_protocol::kob::KobObj::Token(t)) => ("token", t.to_json()),
                Err(_) => ("unknown", Value::Null),
            },
        };
        json!({
            "format": "kpf-object/1", "kind": kind, "network": net.name(), "outpoint": outpoint_json(&self.op),
            "amount": self.amount, "daa": self.daa, "covenantId": self.covenant_id.to_string(),
            "address": p2sh_address(&self.redeem, net), "redeem": to_hex(&self.redeem), "state": state,
        })
    }
}

/// A KOB order file (an evidence order, an executor's token UTXO): the UTXO as for a pair or an order, and for an ask
/// the custody UTXO that holds its tokens. The redeem script of a KOB order is not on chain until it is spent: take it
/// from KOB's indexer (or build it with `kpf kob-file`), and let `--node` refresh the UTXO facts.
#[derive(Clone, Debug)]
pub struct KobFile {
    pub obj: ObjFile,
    pub custody: Option<ObjFile>,
}

impl KobFile {
    pub fn load(path: &str) -> Result<KobFile, String> {
        let v = read_json(path)?;
        KobFile::from_json(&v).map_err(|e| format!("{path}: {e}"))
    }

    pub fn from_json(v: &Value) -> Result<KobFile, String> {
        let obj = ObjFile::from_json(v)?;
        let custody = match v.get("custody") {
            None | Some(Value::Null) => None,
            Some(c) => Some(ObjFile::from_json(c).map_err(|e| format!("custody: {e}"))?),
        };
        Ok(KobFile { obj, custody })
    }

    pub fn to_json(&self, net: Network) -> Value {
        let mut v = self.obj.to_json(net);
        if let Some(c) = &self.custody {
            v["custody"] = c.to_json(net);
        }
        v
    }

    /// The evidence this file describes: an ask with its custody, or a bid.
    pub fn evidence(&self) -> Result<kpf_protocol::kob::Evidence, String> {
        use kpf_protocol::kob::*;
        match decode_order(&self.obj.redeem).map_err(|e| e.to_string())? {
            KobOrder::Ask(a) => {
                let c = self.custody.as_ref().ok_or("an ask needs its custody UTXO: add a `custody` entry (the token UTXO owned by the ask's covenant id)")?;
                let t = TokenState::decode_redeem(&c.redeem).map_err(|e| format!("custody: {e}"))?;
                Ok(Evidence::Ask {
                    order: LiveAsk { a, value: self.obj.amount, id: self.obj.covenant_id, op: self.obj.op, daa: self.obj.known_daa()? },
                    custody: LiveToken { t, value: c.amount, id: c.covenant_id, op: c.op, daa: c.known_daa().map_err(|e| format!("custody: {e}"))? },
                })
            }
            KobOrder::Bid(b) => Ok(Evidence::Bid { order: LiveBid { b, value: self.obj.amount, id: self.obj.covenant_id, op: self.obj.op, daa: self.obj.known_daa()? } }),
        }
    }

    /// The token holder UTXO this file describes.
    pub fn live_token(&self) -> Result<kpf_protocol::kob::LiveToken, String> {
        let t = kpf_protocol::kob::TokenState::decode_redeem(&self.obj.redeem).map_err(|e| e.to_string())?;
        Ok(kpf_protocol::kob::LiveToken { t, value: self.obj.amount, id: self.obj.covenant_id, op: self.obj.op, daa: self.obj.known_daa()? })
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

/// Parses a UTXO list: an array of `{txid, index, amount, daa, covenantId?, scriptPublicKey?}` or of the node's
/// `{outpoint, utxoEntry}` entries
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
            // The simple shape (what `kpf utxos --out` writes) keeps the covenant flag and, when present, the script, so a
            // covenant-bound UTXO read back from the tool's own file is never funding (review 3 T3).
            let spk = e.get("scriptPublicKey").filter(|v| !v.is_null()).map(spk_value).transpose().map_err(|e| ctx(e.to_string()))?;
            let cov = match e.get("covenantId") {
                None | Some(Value::Null) => None,
                Some(c) => Some(Hash::from_bytes(hex32(c.as_str().unwrap_or("")).map_err(|e| ctx(format!("covenantId: {e}")))?)),
            };
            Ok(ListedUtxo {
                op: outpoint_from(e).map_err(ctx)?,
                amount: num(e.get("amount")).ok_or_else(|| ctx("no amount".into()))?,
                daa: num(e.get("daa").or_else(|| e.get("blockDaaScore"))).unwrap_or(0),
                spk,
                covenant_id: cov,
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
            daa: Some(55),
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
