//! Networks and addresses. KPF is a testnet-10 research prototype: nothing here builds, accepts or prints a mainnet
//! address. The mainnet prefix exists only as a constant so that it can be refused with a clear message.

use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_txscript::{extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script};

use crate::error::{Error, Result};

/// Address prefix of mainnet. Only used to refuse mainnet addresses.
pub const MAINNET_PREFIX: &str = "kaspa";
/// Address prefix of the test networks (testnet-10 among them).
pub const TESTNET_PREFIX: &str = "kaspatest";

/// The message every refusal of a mainnet request carries.
pub const TESTNET_ONLY: &str = "KPF is a testnet-only research prototype";

/// The networks KPF runs on: testnet-10 only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Network {
    #[default]
    Testnet10,
}

impl Network {
    pub const fn name(self) -> &'static str {
        "testnet-10"
    }
    pub const fn prefix(self) -> Prefix {
        Prefix::Testnet
    }

    /// Parses a network name. Everything but testnet-10 is refused; mainnet with the testnet-only message.
    pub fn parse(s: &str) -> Result<Network> {
        let n = s.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        let n = n.strip_prefix("kaspa-").unwrap_or(&n);
        match n {
            "testnet-10" | "testnet10" | "tn10" | "testnet" => Ok(Network::Testnet10),
            "mainnet" | "main" | "kaspa" => Err(Error::Network(format!("{TESTNET_ONLY}: refusing network `{s}`"))),
            other => Err(Error::Network(format!("{TESTNET_ONLY} (testnet-10 only): unknown network `{other}`"))),
        }
    }
}

/// An address as a string, for a P2PK (x-only key) script.
pub fn p2pk_address(key: &[u8; 32], net: Network) -> String {
    Address::new(net.prefix(), Version::PubKey, key).to_string()
}

/// The P2SH address of a redeem script.
pub fn p2sh_address(redeem: &[u8], net: Network) -> String {
    spk_address(&pay_to_script_hash_script(redeem), net).expect("a P2SH script has an address")
}

/// The address of a standard script public key, or `None` for a non-standard one.
pub fn spk_address(spk: &ScriptPublicKey, net: Network) -> Option<String> {
    extract_script_pub_key_address(spk, net.prefix()).ok().map(|a| a.to_string())
}

/// Parses an address; refuses every address that is not a testnet one.
pub fn parse_address(s: &str, net: Network) -> Result<Address> {
    let s = s.trim();
    let prefix = s.split(':').next().unwrap_or("");
    if prefix == MAINNET_PREFIX {
        return Err(Error::Network(format!("{TESTNET_ONLY}: refusing the mainnet address `{}`", short(s))));
    }
    if prefix != TESTNET_PREFIX {
        return Err(Error::Network(format!("{TESTNET_ONLY}: `{}` is not a {} address (expected `{TESTNET_PREFIX}:`)", short(s), net.name())));
    }
    Address::try_from(s).map_err(|e| Error::Invalid(format!("address `{}`: {e}", short(s))))
}

/// The x-only key of a testnet P2PK address.
pub fn pubkey_of_address(s: &str, net: Network) -> Result<[u8; 32]> {
    let a = parse_address(s, net)?;
    if a.version != Version::PubKey {
        return Err(Error::Invalid(format!("`{}` is not a P2PK (Schnorr) address", short(s))));
    }
    a.payload.as_slice().try_into().map_err(|_| Error::Invalid("address payload is not 32 bytes".into()))
}

/// The script public key of a testnet address.
pub fn address_spk(s: &str, net: Network) -> Result<ScriptPublicKey> {
    Ok(pay_to_address_script(&parse_address(s, net)?))
}

fn short(s: &str) -> String {
    if s.len() > 24 {
        format!("{}...{}", &s[..14], &s[s.len() - 6..])
    } else {
        s.to_string()
    }
}
