//! Errors of the protocol library.

/// Everything a codec, builder, signer or verifier can refuse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A value outside what the contracts accept, or an inconsistent request.
    #[error("{0}")]
    Invalid(String),
    /// The funding inputs do not cover the outputs and the fee.
    #[error("insufficient funds: need {need} sompi, have {have} sompi")]
    Insufficient { need: u64, have: u64 },
    /// A state or script that does not decode as a KPF contract instance.
    #[error("decode: {0}")]
    Decode(String),
    /// A key missing from the key set, or a signature that does not verify.
    #[error("signing: {0}")]
    Signing(String),
    /// The script engine or a transaction-level rule refused the transaction.
    #[error("engine: {0}")]
    Engine(String),
    /// The request names a network KPF does not run on.
    #[error("{0}")]
    Network(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}

/// Lower-case hex of `b`.
pub fn to_hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

/// Bytes of a hex string (upper or lower case, optional `0x`).
pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() % 2 != 0 {
        return invalid("hex string of odd length");
    }
    let nib = |c: u8| -> Result<u8> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => invalid(format!("not a hex digit: {:?}", c as char)),
        }
    };
    s.as_bytes().chunks(2).map(|p| Ok(nib(p[0])? << 4 | nib(p[1])?)).collect()
}

/// A 32-byte value from hex.
pub fn hex32(s: &str) -> Result<[u8; 32]> {
    from_hex(s)?.try_into().map_err(|_| Error::Invalid("expected 32 bytes of hex".into()))
}

/// Sompi as a decimal KAS string (up to 8 decimals, trailing zeros trimmed).
pub fn kas_str(v: i128) -> String {
    let neg = v < 0;
    let a = v.unsigned_abs();
    let (w, f) = (a / 100_000_000, a % 100_000_000);
    let mut s = format!("{w}.{f:08}");
    s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if neg {
        format!("-{s}")
    } else {
        s
    }
}
