//! Key files: generation (outside any repository, restrictive permissions), loading and naming a party.
//!
//! A key file is JSON `{"format":"kpf-key/1","network":"testnet-10","secretKey":"<hex>","publicKey":"<hex>","address":"kaspatest:..."}`.
//! Secret keys are never accepted on the command line (they would show in process listings) and never printed unless
//! `--show-secret` is given to `keygen`.

use std::path::{Path, PathBuf};

use kpf_protocol::error::{from_hex, hex32, to_hex};
use kpf_protocol::network::{p2pk_address, pubkey_of_address, Network};
use kpf_protocol::tx::{pubkey_of, KeySet};
use secp256k1::{Keypair, Secp256k1};
use serde_json::{json, Value};

pub struct KeyFile {
    pub secret: [u8; 32],
    pub public: [u8; 32],
    /// What `generate` actually did about the file's permissions, in words (review 3 T5); empty for a loaded key.
    pub permissions: String,
}

/// The home directory (`USERPROFILE` on Windows, `HOME` elsewhere).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// `~/.kpf/keys/<name>.json`.
pub fn default_key_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("--name may contain letters, digits, - and _ only".into());
    }
    let home = home_dir().ok_or("cannot find the home directory; give --out")?;
    Ok(home.join(".kpf").join("keys").join(format!("{name}.json")))
}

/// Whether `path` lies inside a git working tree (a `.git` in an ancestor directory).
pub fn inside_repository(path: &Path) -> bool {
    let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|c| c.join(path)).unwrap_or_else(|_| path.to_path_buf()) };
    abs.ancestors().skip(1).any(|d| d.join(".git").exists())
}

/// Creates a key file with a fresh secret. Refuses to overwrite, and refuses a path inside a git working tree.
pub fn generate(path: &Path, net: Network, allow_in_repo: bool) -> Result<KeyFile, String> {
    if inside_repository(path) && !allow_in_repo {
        return Err(format!(
            "{} is inside a git working tree; a key file must not live in a repository (pick a path outside it, or pass --allow-in-repo if you know what you are doing)",
            path.display()
        ));
    }
    if path.exists() {
        return Err(format!("{} exists; refusing to overwrite a key file", path.display()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let secp = Secp256k1::new();
    let kp = Keypair::new(&secp, &mut secp256k1::rand::thread_rng());
    let secret = kp.secret_bytes();
    let public = kp.x_only_public_key().0.serialize();
    let doc = json!({
        "format": "kpf-key/1", "network": net.name(), "secretKey": to_hex(&secret), "publicKey": to_hex(&public),
        "address": p2pk_address(&public, net),
    });
    let permissions = write_private(path, &serde_json::to_string_pretty(&doc).expect("json"))?;
    Ok(KeyFile { secret, public, permissions })
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok("the file was created with mode 600 (owner read and write only)".into())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> Result<String, String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    drop(f);
    // Windows: drop the inherited permissions and grant the current user only, and report what really happened (review 3
    // T5): the file starts with the folder's inherited permissions, and `icacls` may fail (a USERNAME it cannot resolve,
    // a domain account that needs DOMAIN\user).
    let warn = |why: String| {
        format!("WARNING: the file keeps the permissions it inherited from its folder ({why}); restrict it yourself, e.g. `icacls <file> /inheritance:r /grant:r <you>:F`, or keep it in a folder only you can read")
    };
    let Some(user) = std::env::var_os("USERNAME") else {
        return Ok(warn("USERNAME is not set, so no account to grant".into()));
    };
    let user = user.to_string_lossy().into_owned();
    let r = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("{user}:F"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(match r {
        Ok(s) if s.success() => format!("inherited permissions were removed and only the account {user} was granted access (icacls)"),
        Ok(s) => warn(format!("icacls failed with {s} for the account `{user}`")),
        Err(e) => warn(format!("icacls could not be run: {e}")),
    })
}

/// Loads a key file; checks that the public key belongs to the secret and the file is a testnet one.
pub fn load(path: &Path) -> Result<KeyFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("key file {}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("key file {}: {e}", path.display()))?;
    if let Some(n) = v.get("network").and_then(Value::as_str) {
        Network::parse(n).map_err(|e| format!("key file {}: {e}", path.display()))?;
    }
    let secret = hex32(v.get("secretKey").and_then(Value::as_str).ok_or_else(|| format!("key file {}: no secretKey", path.display()))?)
        .map_err(|e| format!("key file {}: secretKey: {e}", path.display()))?;
    let public = pubkey_of(&secret).map_err(|e| format!("key file {}: {e}", path.display()))?;
    if let Some(p) = v.get("publicKey").and_then(Value::as_str) {
        if from_hex(p).ok().as_deref() != Some(&public[..]) {
            return Err(format!("key file {}: publicKey does not belong to secretKey", path.display()));
        }
    }
    Ok(KeyFile { secret, public, permissions: String::new() })
}

/// A key set of the given key files.
pub fn keyset(paths: &[&str]) -> Result<KeySet, String> {
    let mut ks = KeySet::new();
    for p in paths {
        let k = load(Path::new(p))?;
        ks.add_secret(&k.secret).map_err(|e| e.to_string())?;
    }
    Ok(ks)
}

/// Names a party: a key file (its public key), a 64-hex x-only public key, or a testnet P2PK address.
pub fn party(s: &str, net: Network) -> Result<[u8; 32], String> {
    let p = Path::new(s);
    if p.is_file() {
        return load(p).map(|k| k.public);
    }
    if s.len() == 64 {
        if let Ok(k) = hex32(s) {
            return Ok(k);
        }
    }
    if s.contains(':') {
        return pubkey_of_address(s, net).map_err(|e| e.to_string());
    }
    Err(format!("`{s}` is neither a key file, a 64-hex public key nor a kaspatest: address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_writes_a_loadable_file_and_refuses_overwrite() {
        let dir = std::env::temp_dir().join(format!("kpf-keys-{}", std::process::id()));
        let path = dir.join("k.json");
        let k = generate(&path, Network::Testnet10, true).unwrap();
        let l = load(&path).unwrap();
        assert_eq!((k.secret, k.public), (l.secret, l.public));
        assert!(generate(&path, Network::Testnet10, true).is_err());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("kaspatest:"));
        assert_eq!(party(path.to_str().unwrap(), Network::Testnet10).unwrap(), k.public);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_of_parties() {
        let pk = kpf_protocol::fixtures::pubkey(5);
        assert_eq!(party(&to_hex(&pk), Network::Testnet10).unwrap(), pk);
        let addr = p2pk_address(&pk, Network::Testnet10);
        assert_eq!(party(&addr, Network::Testnet10).unwrap(), pk);
        assert!(party(&addr.replace("kaspatest:", "kaspa:"), Network::Testnet10).unwrap_err().contains("testnet-only"));
        assert!(party("nonsense", Network::Testnet10).is_err());
    }
}
