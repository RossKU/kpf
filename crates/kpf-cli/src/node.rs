//! A minimal blocking Kaspa wRPC client (JSON encoding over a plain websocket): `getBlockDagInfo`,
//! `getUtxosByAddresses`, `submitTransaction`.
//!
//! The node URL is always given explicitly (`--node ws://host:18210`; 18210 is the JSON wRPC port of a testnet node).
//! There is no default node and no public resolver. Only `ws://` is supported: no TLS code is linked, so reach a remote
//! node through a tunnel you control. All logic is behind the [`Transport`] trait, so it is unit-tested offline with a
//! mock; the websocket transport is the only code that touches a socket and it is reached only with `--node`.
//!
//! Framing (workflow-rpc JSON, rusty-kaspa v2.1.0): `-> {"id":1,"method":"getUtxosByAddresses","params":{...}}`,
//! `<- {"id":1,"method":"getUtxosByAddresses","params":{...}}` or `<- {"id":1,"error":{"message":"..."}}`.
//! `getUtxosByAddresses` needs a node started with `--utxoindex`.

use std::net::TcpStream;
use std::time::Duration;

use kpf_protocol::network::Network;
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::files::{parse_utxos, ListedUtxo};

pub const TIMEOUT: Duration = Duration::from_secs(30);
/// Addresses per `getUtxosByAddresses` request.
pub const ADDRESS_CHUNK: usize = 50;

/// One request/response exchange.
pub trait Transport {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
}

/// Checks a node URL: explicit, `ws://`, no credentials in the clear in messages.
pub fn check_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("--node needs a ws:// URL (there is no default node)".into());
    }
    if u.starts_with("wss://") || u.starts_with("https://") {
        return Err("only ws:// node URLs are supported (no TLS is linked); reach a remote node through a tunnel you control".into());
    }
    if !u.starts_with("ws://") {
        return Err(format!("`{}` is not a ws:// URL", shown(u)));
    }
    Ok(())
}

/// A URL as messages show it: scheme, host and port only.
pub fn shown(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    if scheme.is_empty() {
        host.to_string()
    } else {
        format!("{scheme}://{host}")
    }
}

/// What one incoming frame means for the awaited request.
#[derive(Debug, PartialEq)]
pub enum Reply {
    Other,
    Ok(Value),
}

pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "id": id, "method": method, "params": params })
}

pub fn parse_reply(text: &str, id: u64) -> Result<Reply, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("unexpected reply (not JSON: {e})"))?;
    if v.get("id").and_then(Value::as_u64) != Some(id) {
        return Ok(Reply::Other);
    }
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        let msg = err.get("message").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| err.to_string());
        return Err(format!("node rejected the request: {msg}"));
    }
    match v.get("params") {
        Some(p) => Ok(Reply::Ok(p.clone())),
        None => Err(format!("unexpected reply: no params or error in {text}")),
    }
}

/// The websocket transport.
pub struct WsTransport {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
    url: String,
}

impl WsTransport {
    pub fn connect(url: &str) -> Result<WsTransport, String> {
        check_url(url)?;
        let (mut ws, _) = tungstenite::connect(url.trim()).map_err(|e| format!("cannot connect to the node {}: {e}", shown(url)))?;
        if let MaybeTlsStream::Plain(s) = ws.get_mut() {
            let _ = s.set_read_timeout(Some(TIMEOUT));
            let _ = s.set_write_timeout(Some(TIMEOUT));
        }
        Ok(WsTransport { ws, next: 1, url: url.to_string() })
    }
}

impl Transport for WsTransport {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.ws.send(Message::text(request(id, method, params).to_string())).map_err(|e| format!("node {}: {e}", shown(&self.url)))?;
        loop {
            match self.ws.read().map_err(|e| format!("node {}: {e}", shown(&self.url)))? {
                Message::Text(t) => match parse_reply(t.as_str(), id)? {
                    Reply::Other => continue,
                    Reply::Ok(p) => return Ok(p),
                },
                Message::Close(_) => return Err(format!("node {} closed the connection before answering", shown(&self.url))),
                _ => continue,
            }
        }
    }
}

/// Node operations on any transport.
pub struct Node<T: Transport> {
    pub t: T,
}

impl<T: Transport> Node<T> {
    pub fn new(t: T) -> Node<T> {
        Node { t }
    }

    /// The node must run testnet-10; anything else (mainnet above all) is refused before anything is signed or sent.
    pub fn check_network(&mut self) -> Result<u64, String> {
        let p = self.t.call("getBlockDagInfo", json!({}))?;
        let n = p.get("network").and_then(Value::as_str).ok_or("getBlockDagInfo names no network")?;
        Network::parse(n).map_err(|e| format!("the node runs `{n}`: {e}"))?;
        Ok(p.get("virtualDaaScore").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0))
    }

    /// A parameterless call, for `daa`.
    pub fn t_call(&mut self, method: &str) -> Result<Value, String> {
        self.t.call(method, json!({}))
    }

    pub fn utxos(&mut self, addresses: &[String]) -> Result<Vec<ListedUtxo>, String> {
        let mut out = vec![];
        for chunk in addresses.chunks(ADDRESS_CHUNK) {
            let p = self.t.call("getUtxosByAddresses", json!({ "addresses": chunk }))?;
            let entries = p.get("entries").cloned().unwrap_or(Value::Array(vec![]));
            out.extend(parse_utxos(&entries)?);
        }
        Ok(out)
    }

    /// Submits a transaction (`RpcTransaction` JSON) and returns the id the node reports.
    pub fn submit(&mut self, rpc_tx: &Value) -> Result<String, String> {
        let p = self.t.call("submitTransaction", json!({ "transaction": rpc_tx, "allowOrphan": false }))?;
        p.get("transactionId").and_then(Value::as_str).map(str::to_owned).ok_or_else(|| format!("no transactionId in {p}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A node that answers from a table and records what it was asked.
    #[derive(Default)]
    pub struct Mock {
        pub answers: BTreeMap<String, Value>,
        pub asked: Vec<(String, Value)>,
    }
    impl Transport for Mock {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
            self.asked.push((method.to_string(), params));
            self.answers.get(method).cloned().ok_or_else(|| format!("no answer for {method}"))
        }
    }

    #[test]
    fn urls_are_explicit_and_plain() {
        assert!(check_url("ws://127.0.0.1:18210").is_ok());
        assert!(check_url("").unwrap_err().contains("no default node"));
        assert!(check_url("wss://node.example").unwrap_err().contains("tunnel"));
        assert!(check_url("http://x").is_err());
        assert_eq!(shown("ws://user:secret@10.0.0.1:18210/path?k=v"), "ws://10.0.0.1:18210");
        assert!(WsTransport::connect("wss://x").is_err(), "refused before any socket is opened");
    }

    #[test]
    fn frames_parse() {
        let r = request(3, "getBlockDagInfo", json!({}));
        assert_eq!(r["id"], 3);
        assert_eq!(parse_reply(r#"{"id":9,"method":"x","params":{}}"#, 3).unwrap(), Reply::Other);
        assert_eq!(parse_reply(r#"{"id":3,"method":"x","params":{"a":1}}"#, 3).unwrap(), Reply::Ok(json!({"a": 1})));
        assert!(parse_reply(r#"{"id":3,"error":{"code":0,"message":"orphan"}}"#, 3).unwrap_err().contains("orphan"));
        assert!(parse_reply("nope", 3).is_err());
        assert!(parse_reply(r#"{"id":3}"#, 3).is_err());
    }

    #[test]
    fn network_must_be_testnet_10() {
        let mut m = Mock::default();
        m.answers.insert("getBlockDagInfo".into(), json!({"network": "testnet-10", "virtualDaaScore": "1234"}));
        let mut n = Node::new(m);
        assert_eq!(n.check_network().unwrap(), 1234);
        for bad in ["mainnet", "testnet-11", "simnet"] {
            let mut m = Mock::default();
            m.answers.insert("getBlockDagInfo".into(), json!({ "network": bad }));
            let e = Node::new(m).check_network().unwrap_err();
            assert!(e.contains("testnet-only"), "{bad}: {e}");
        }
    }

    #[test]
    fn utxos_and_submit_use_the_expected_calls() {
        let mut m = Mock::default();
        m.answers.insert(
            "getUtxosByAddresses".into(),
            json!({"entries": [{"address": "kaspatest:x", "outpoint": {"transactionId": "ab".repeat(32), "index": 1},
                                 "utxoEntry": {"amount": 900, "scriptPublicKey": "0000aa", "blockDaaScore": 7, "isCoinbase": false, "covenantId": null}}]}),
        );
        m.answers.insert("submitTransaction".into(), json!({"transactionId": "cd".repeat(32)}));
        let mut n = Node::new(m);
        let u = n.utxos(&["kaspatest:x".to_string()]).unwrap();
        assert_eq!((u.len(), u[0].amount, u[0].daa), (1, 900, 7));
        let id = n.submit(&json!({"version": 1})).unwrap();
        assert_eq!(id, "cd".repeat(32));
        assert_eq!(n.t.asked[0].0, "getUtxosByAddresses");
        assert_eq!(n.t.asked[1].0, "submitTransaction");
        assert_eq!(n.t.asked[1].1["allowOrphan"], false);
    }

    #[test]
    fn addresses_go_in_chunks() {
        let mut m = Mock::default();
        m.answers.insert("getUtxosByAddresses".into(), json!({"entries": []}));
        let addrs: Vec<String> = (0..120).map(|i| format!("kaspatest:{i}")).collect();
        let mut n = Node::new(m);
        n.utxos(&addrs).unwrap();
        assert_eq!(n.t.asked.len(), 3);
    }
}
