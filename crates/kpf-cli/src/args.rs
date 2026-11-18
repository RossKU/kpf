//! A small argument parser (no dependency): `kpf <command> [--flag value]... [--switch]...`, and exact decimal amounts.

use std::collections::BTreeMap;

/// Flags that take no value.
const SWITCHES: &[&str] = &[
    "submit", "json", "brief", "priority", "show-secret", "unlist", "help", "example", "no-save", "allow-in-repo", "update",
    "allow-dead-evidence",
];

/// Parsed arguments of one command.
#[derive(Debug, Default)]
pub struct Args {
    pub command: String,
    flags: BTreeMap<String, Vec<String>>,
    pub positional: Vec<String>,
}

impl Args {
    pub fn parse(argv: &[String]) -> Result<Args, String> {
        let mut a = Args::default();
        let mut it = argv.iter().peekable();
        while let Some(x) = it.next() {
            if let Some(name) = x.strip_prefix("--") {
                if name.is_empty() {
                    a.positional.extend(it.by_ref().cloned());
                    break;
                }
                let (name, inline) = match name.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (name, None),
                };
                if SWITCHES.contains(&name) {
                    if inline.is_some() {
                        return Err(format!("--{name} takes no value"));
                    }
                    a.flags.entry(name.to_string()).or_default().push(String::new());
                } else {
                    let v = match inline {
                        Some(v) => v,
                        None => it.next().cloned().ok_or_else(|| format!("--{name} needs a value"))?,
                    };
                    a.flags.entry(name.to_string()).or_default().push(v);
                }
            } else if a.command.is_empty() {
                a.command = x.clone();
            } else {
                a.positional.push(x.clone());
            }
        }
        Ok(a)
    }

    pub fn has(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.flags.get(name).and_then(|v| v.last()).map(|s| s.as_str())
    }

    pub fn all(&self, name: &str) -> Vec<&str> {
        self.flags.get(name).map(|v| v.iter().map(|s| s.as_str()).collect()).unwrap_or_default()
    }

    pub fn need(&self, name: &str) -> Result<&str, String> {
        self.get(name).ok_or_else(|| format!("--{name} is required"))
    }

    /// Rejects flags the command does not know (typos must not be ignored when money is involved).
    pub fn only(&self, allowed: &[&str]) -> Result<(), String> {
        for k in self.flags.keys() {
            if !allowed.contains(&k.as_str()) && !COMMON.contains(&k.as_str()) {
                return Err(format!("unknown option --{k} for `{}`", self.command));
            }
        }
        Ok(())
    }
}

/// Options every command accepts.
const COMMON: &[&str] = &["help", "network"];

/// Whether `unit` is a power of ten (1, 10, 100, ...): then amounts are decimals with log10(unit) places.
pub fn is_pow10(unit: i64) -> bool {
    let mut u = unit;
    if u <= 0 {
        return false;
    }
    while u % 10 == 0 {
        u /= 10;
    }
    u == 1
}

/// An exact decimal to an integer of `unit` parts per whole, or an integer with the suffix `s` (sompi or base units,
/// no scaling). `"1.5"` with unit 10^8 is 150000000; `"150000000s"` is itself. Any `unit` > 0 works exactly (review 3
/// T2, DESIGN 4.5c): the value is `whole * unit + frac * unit / 10^digits`, and a decimal that is not a whole number of
/// parts (`"0.3"` of a unit of 5) is refused instead of rounded. A unit that is a power of ten keeps the rule "at most
/// log10(unit) decimal places".
pub fn parse_amount(s: &str, unit: i64) -> Result<i64, String> {
    let t = s.trim();
    if let Some(raw) = t.strip_suffix('s') {
        return raw.trim().parse::<i64>().map_err(|_| format!("`{s}` is not an integer amount"));
    }
    if unit <= 0 {
        return Err(format!("the unit {unit} is not positive"));
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t, ""),
    };
    if whole.is_empty() && frac.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("`{s}` is not a decimal number"));
    }
    let frac = frac.trim_end_matches('0');
    if frac.len() > 18 {
        return Err(format!("`{s}` has more than 18 decimal places; use the `s` suffix for exact integers"));
    }
    let w: i128 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| format!("`{s}` is too large"))? };
    let f: i128 = if frac.is_empty() { 0 } else { frac.parse().map_err(|_| format!("`{s}` is too large"))? };
    let den = 10i128.pow(frac.len() as u32);
    let parts = f * unit as i128;
    if parts % den != 0 {
        return Err(if is_pow10(unit) {
            format!("`{s}` has more than {} decimal places (unit 1/{unit}); use the `s` suffix for exact integers", (unit as f64).log10().round() as usize)
        } else {
            format!("`{s}` is not a whole number of base units: a whole token is {unit} base units, so the fraction must be a multiple of 1/{unit}; use the `s` suffix for exact base units")
        });
    }
    let v = w.checked_mul(unit as i128).and_then(|x| x.checked_add(parts / den)).ok_or_else(|| format!("`{s}` is too large"))?;
    let v = i64::try_from(v).map_err(|_| format!("`{s}` is too large"))?;
    Ok(if neg { -v } else { v })
}

/// KAS (decimal, 10^8 sompi) or sompi with the `s` suffix.
pub fn kas(s: &str) -> Result<i64, String> {
    parse_amount(s, 100_000_000)
}

/// Formats sompi as KAS with up to 8 decimals, trailing zeros trimmed.
pub fn fmt_kas(v: i128) -> String {
    fmt_units(v, 100_000_000)
}

/// Formats an integer of `unit` parts per whole, exactly (review 3 T2). A power-of-ten unit prints a decimal with
/// trailing zeros trimmed. Any other unit prints the exact decimal when the fraction has one (`500000` of 400,000 is
/// `"1.25"`), else the whole part and the exact fraction (`4` of 3 is `"1 1/3"`).
pub fn fmt_units(v: i128, unit: i128) -> String {
    let neg = v < 0;
    let a = v.unsigned_abs() as i128;
    if unit <= 0 {
        return format!("{v} (unit {unit})");
    }
    let (w, f) = (a / unit, a % unit);
    let s = if is_pow10(unit.min(i64::MAX as i128) as i64) {
        let digits = (unit as f64).log10().round() as usize;
        let mut s = if digits == 0 { format!("{w}") } else { format!("{w}.{f:0digits$}") };
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        s
    } else if f == 0 {
        format!("{w}")
    } else {
        // f / unit has a finite decimal iff the reduced denominator has no prime factor but 2 and 5.
        let g = gcd(f, unit);
        let (mut num, den) = (f / g, unit / g);
        let mut d = den;
        for p in [2, 5] {
            while d % p == 0 {
                d /= p;
            }
        }
        if d == 1 {
            let mut digits = String::new();
            while num != 0 && digits.len() < 40 {
                num *= 10;
                digits.push(char::from(b'0' + (num / den) as u8));
                num %= den;
            }
            format!("{w}.{digits}")
        } else {
            format!("{w} {}/{}", f / g, den)
        }
    };
    if neg {
        format!("-{s}")
    } else {
        s
    }
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[&str]) -> Result<Args, String> {
        Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn flags_switches_and_positionals() {
        let a = p(&["open", "--long", "a.json", "--submit", "--key=k1", "--key", "k2", "x"]).unwrap();
        assert_eq!(a.command, "open");
        assert_eq!(a.get("long"), Some("a.json"));
        assert!(a.has("submit"));
        assert_eq!(a.all("key"), vec!["k1", "k2"]);
        assert_eq!(a.positional, vec!["x"]);
        assert!(p(&["open", "--long"]).is_err());
        assert!(a.only(&["long", "submit", "key"]).is_ok());
        assert!(a.only(&["long"]).is_err());
    }

    #[test]
    fn decimals_are_exact() {
        assert_eq!(kas("1.5").unwrap(), 150_000_000);
        assert_eq!(kas("0.00000001").unwrap(), 1);
        assert_eq!(kas("12").unwrap(), 1_200_000_000);
        assert_eq!(kas("150000000s").unwrap(), 150_000_000);
        assert_eq!(parse_amount("1000.5", 1_000_000).unwrap(), 1_000_500_000);
        assert!(kas("0.000000001").is_err());
        assert!(kas("1.2.3").is_err());
        assert!(kas("abc").is_err());
        assert!(kas("").is_err());
        assert!(kas("99999999999999999999").is_err());
        assert_eq!(fmt_kas(150_000_000), "1.5");
        assert_eq!(fmt_kas(100_000_000), "1");
        assert_eq!(fmt_kas(-1), "-0.00000001");
        assert_eq!(fmt_units(1_000_500_000, 1_000_000), "1000.5");
    }
}
