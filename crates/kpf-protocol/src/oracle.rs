//! Oracle-only markets, the integration slice (`docs/DESIGN-ORACLE-IMPL.md`, third slice): the oracle market block,
//! the states of the oracle-market order, pair and pending part (`contracts/KpfOpenO.sil`, `KpfPairO.sil`,
//! `KpfPendingO.sil`) with their codecs, the bounds every contract enforces, and the math each entry applies (the
//! trigger with the tolerance g inside it, the close price, the bond, the cure and forfeit bounds, the settlement of
//! every branch, the renewal, the lapse). Every function names the contract text it mirrors; the engine tests
//! (`crates/kpf-tests/tests/oracle_*`) hold them together: a transaction built from these values is accepted, one sompi
//! or one tick less is refused.
//!
//! The market block (state `[0..309)` of every order, pair and pending part of a market):
//!
//! | bytes | field |
//! |---|---|
//! | `[0..33)` | the market id, `blake3` of `[33..309)` |
//! | `[33..87)` | the bond parameters of its slots (`KpfBond`'s, byte for byte with the id: the bond block `[0..87)`) |
//! | `[87..141)` | the judge parameters of its stakes (`KpfStake`'s: the judge block is `[0..33) + [87..141)`) |
//! | `[141..174)` | `blake3` of the human-readable feed definition |
//! | `[174..309)` | `scale, priceMax, mmrBps, gBps, liqFeeBps, keeperFeeMin, minSize, carrier, idleDaa, challengeDaa, lateDaa, quietDaa, sigmaBps, betaBps, bondMin` |

use silverscript_abi::ArtifactValue;

use crate::artifacts::Template;
use crate::bond::{BondMarket, BondParams, RING, RHO_MAX_BPS};
use crate::error::{Error, Result};
use crate::judge::{JudgeMarket, JudgeParams, BURN_MAX_BPS, MIN_STAKE_MAX, MU_MAX_BPS, MU_MIN_BPS, UNIT};
use crate::payoff::q;
use crate::state::{encode, to_args, Cursor, F, BOND_MIN_MAX, BPS, CSV_LIMIT, KIND_CLOSE, KIND_LIQ, LONG, MMR_CAP, PLAIN_DUST, PRICE_CAP, SCALE_MAX, SHORT};

/// Length of the oracle market block.
pub const MARKET_LEN: usize = 309;
/// State lengths (from redeem byte 1) of the three templates.
pub const OPEN_STATE_LEN: usize = 465;
pub const PAIR_STATE_LEN: usize = 723;
pub const PENDING_STATE_LEN: usize = 813;
/// Statuses of a pending part.
pub const PENDING: i64 = 1;
pub const CONTESTED: i64 = 2;
pub const UPHELD: i64 = 3;
pub const REFUTED: i64 = 4;
/// Claim references of a bind (`KpfBond.OUT_REF`).
pub const OUT_REF: i64 = 65_536;
/// The twin scan covers at most 8 outputs (`KpfOpenO.MAX_OUTPUTS`).
pub const MAX_OUTPUTS: usize = 8;

/// Why the model refuses (a contract `require`); for messages only.
pub type Refusal = &'static str;

fn chk(ok: bool, why: Refusal) -> std::result::Result<(), Refusal> {
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

/// The pair parameters of an oracle market (state `[174..309)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OParams {
    /// Base units per whole unit of the underlying.
    pub scale: i64,
    /// Prices are sompi per whole unit; the largest price.
    pub price_max: i64,
    pub mmr_bps: i64,
    /// The tolerance g inside the trigger and the close price.
    pub g_bps: i64,
    pub liq_fee_bps: i64,
    pub keeper_fee_min: i64,
    pub min_size: i64,
    pub carrier: i64,
    /// How long after the cover's liability ended (`expiry + tailDaa`) a lapsed pair may be settled (`lapseSettle`, CLTV).
    pub idle_daa: i64,
    /// W: the contest window from P, the earliest finalize (CSV).
    pub challenge_daa: i64,
    /// A claim landed more than this after its `t0` is void.
    pub late_daa: i64,
    /// The age every party entry of a pair needs (CSV).
    pub quiet_daa: i64,
    /// The bond of a claim, of its cover.
    pub sigma_bps: i64,
    /// The disputer's share of a refuted claim's bond.
    pub beta_bps: i64,
    /// The bond's floor.
    pub bond_min: i64,
}

impl OParams {
    fn fields(&self) -> Vec<F> {
        [
            self.scale,
            self.price_max,
            self.mmr_bps,
            self.g_bps,
            self.liq_fee_bps,
            self.keeper_fee_min,
            self.min_size,
            self.carrier,
            self.idle_daa,
            self.challenge_daa,
            self.late_daa,
            self.quiet_daa,
            self.sigma_bps,
            self.beta_bps,
            self.bond_min,
        ]
        .into_iter()
        .map(F::I)
        .collect()
    }
}

/// An oracle market block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OMarket {
    pub bond: BondParams,
    pub judge: JudgeParams,
    /// `blake3` of the human-readable feed definition.
    pub feed_hash: [u8; 32],
    pub p: OParams,
}

impl OMarket {
    fn body_fields(&self) -> Vec<F> {
        let mut f = self.bond.fields();
        f.extend(self.judge.fields());
        f.push(F::B(self.feed_hash));
        f.extend(self.p.fields());
        f
    }
    /// The market body (state `[33..309)`): every parameter and the feed hash.
    pub fn body(&self) -> Vec<u8> {
        encode(&self.body_fields())
    }
    /// The market id: `blake3` of the body (`checkMarket`'s first line).
    pub fn id(&self) -> [u8; 32] {
        *blake3::hash(&self.body()).as_bytes()
    }
    fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.id())];
        f.extend(self.body_fields());
        f
    }
    /// The 309 bytes of the block.
    pub fn bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    /// The bond block of the market's slots.
    pub fn bond_market(&self) -> BondMarket {
        BondMarket { id: self.id(), p: self.bond }
    }
    /// The judge block of the market's stakes.
    pub fn judge_market(&self) -> JudgeMarket {
        JudgeMarket { id: self.id(), p: self.judge }
    }

    /// `checkMarket` (one text in the three templates), with the bond and judge bounds.
    pub fn check(&self) -> std::result::Result<(), Refusal> {
        self.bond.check()?;
        self.judge.check()?;
        let p = &self.p;
        chk(p.scale > 0 && p.scale <= SCALE_MAX, "scale")?;
        chk(p.price_max > 0 && p.price_max <= PRICE_CAP, "priceMax")?;
        chk(p.carrier >= 0, "carrier")?;
        chk(p.min_size > 0, "minSize")?;
        chk(p.idle_daa >= 0 && p.idle_daa < CSV_LIMIT, "idleDaa")?;
        chk(p.mmr_bps >= 0 && p.mmr_bps < MMR_CAP, "mmrBps")?;
        chk(p.liq_fee_bps >= 0 && p.liq_fee_bps <= BPS, "liqFeeBps")?;
        chk(p.keeper_fee_min >= 0, "keeperFeeMin")?;
        chk(p.bond_min >= 0 && p.bond_min <= BOND_MIN_MAX, "bondMin")?;
        chk(p.g_bps > 0 && p.g_bps < BPS, "gBps")?;
        chk(p.challenge_daa > 0 && p.challenge_daa < self.bond.term, "challengeDaa must be positive and below the term")?;
        chk(p.late_daa >= 0 && p.late_daa < CSV_LIMIT, "lateDaa")?;
        chk(p.quiet_daa >= 0 && p.quiet_daa < CSV_LIMIT, "quietDaa")?;
        chk(p.sigma_bps >= 0 && p.sigma_bps <= BPS, "sigmaBps")?;
        chk(p.beta_bps >= 0 && p.beta_bps <= BPS, "betaBps")
    }

    /// The testnet defaults at 10 DAA per second for a slow underlying: the slot of slice A (rho 1, a 28-day term of 8
    /// steps, tail 3.25 days) and the judge of slice B1 (mu 1.25, 7-day age and unbonding, 12-hour rounds, 10 % burn,
    /// 100 KAS stakes); W 6 hours, a claim void if it lands more than 10 minutes after the time it prices, 60 s of
    /// quiet after every party entry, a lapsed pair settles at its entry after 7 days idle, the bond 10 % of the cover
    /// (at least 1 KAS), the disputer 20 % of a refuted bond; mmr 5 %, g 0.5 %, fee 0.5 %.
    pub fn testnet(feed_hash: [u8; 32]) -> OMarket {
        OMarket {
            bond: BondParams::testnet(),
            judge: JudgeParams::testnet(),
            feed_hash,
            p: OParams {
                scale: 100_000_000,
                price_max: 100_000_000_000_000,
                mmr_bps: 500,
                g_bps: 50,
                liq_fee_bps: 50,
                keeper_fee_min: 10_000_000,
                min_size: 1_000_000,
                carrier: 20_000_000,
                idle_daa: 6_048_000,
                challenge_daa: 216_000,
                late_daa: 6_000,
                quiet_daa: 600,
                sigma_bps: 1_000,
                beta_bps: 2_000,
                bond_min: 100_000_000,
            },
        }
    }

    /// The block decoded from its 309 bytes; refused when the id is not the hash of the body.
    pub fn decode(b: &[u8]) -> Result<OMarket> {
        if b.len() != MARKET_LEN {
            return Err(Error::Decode(format!("an oracle market block is {MARKET_LEN} bytes, not {}", b.len())));
        }
        let mut c = Cursor { b, at: 0 };
        let id = c.b32()?;
        let m = Self::read_body(&mut c)?;
        c.done()?;
        if m.id() != id {
            return Err(Error::Decode("the market id is not blake3 of the market body".into()));
        }
        Ok(m)
    }

    fn read_body(c: &mut Cursor) -> Result<OMarket> {
        let bond = BondParams { rho_bps: c.int()?, step: c.int()?, term: c.int()?, tail: c.int()?, rate: c.int()?, unbond: c.int()? };
        let judge = JudgeParams { mu_bps: c.int()?, age: c.int()?, unbond: c.int()?, round: c.int()?, burn_bps: c.int()?, min_stake: c.int()? };
        let feed_hash = c.b32()?;
        let p = OParams {
            scale: c.int()?,
            price_max: c.int()?,
            mmr_bps: c.int()?,
            g_bps: c.int()?,
            liq_fee_bps: c.int()?,
            keeper_fee_min: c.int()?,
            min_size: c.int()?,
            carrier: c.int()?,
            idle_daa: c.int()?,
            challenge_daa: c.int()?,
            late_daa: c.int()?,
            quiet_daa: c.int()?,
            sigma_bps: c.int()?,
            beta_bps: c.int()?,
            bond_min: c.int()?,
        };
        Ok(OMarket { bond, judge, feed_hash, p })
    }

    /// The bond of a claim on a part of `n` at cover rate `cr`: `max(bondMin, 0.02 KAS, q(n, floor(cr * sigma / 10^4)))`
    /// (`KpfPairO.propose`).
    pub fn bond_of(&self, n: i64, cr: i64) -> i64 {
        let r = (cr as i128 * self.p.sigma_bps as i128 / BPS as i128) as i64;
        q(n, r, self.p.scale, 0).max(self.p.bond_min).max(PLAIN_DUST)
    }

    /// The disputer's share of a refuted bond: `floor(B * beta / 10^4)` (split product), 0 below dust; and the
    /// defender's rest (`KpfPendingO.restorePaying`).
    pub fn bond_split(&self, bond: i64, refuted: bool) -> (i64, i64) {
        let mut share = if refuted { (bond / BPS) * self.p.beta_bps + ((bond % BPS) * self.p.beta_bps) / BPS } else { 0 };
        if share < PLAIN_DUST {
            share = 0;
        }
        let mut rest = bond - share;
        if rest < PLAIN_DUST {
            share = bond;
            rest = 0;
        }
        (share, rest)
    }
}

// ---------------------------------------------------------------- the feed definition

/// What a wallet checks of a human-readable feed definition before it shows a market: its hash is the block's
/// `feedHash`, and the text holds nothing that renders misleadingly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedCheck {
    pub hash: [u8; 32],
    /// Characters a wallet must not render as plain text: control characters other than line breaks and tabs,
    /// bidirectional overrides, zero-width and other invisible characters, and every character outside printable ASCII
    /// (homoglyphs of letters and digits).
    pub suspicious: Vec<(usize, char, &'static str)>,
    /// The lines a definition must carry (the note's 1.9: the quantity, the sources and how they combine, the time of a
    /// price, the session calendar, the disagreement rule), each found or missing.
    pub missing: Vec<&'static str>,
}

/// The keys a feed definition is expected to state, one per line as `key: value` (`docs/TESTNET.md`).
pub const FEED_KEYS: [&str; 6] = ["quantity", "unit", "sources", "time", "sessions", "disagreement"];

/// The feed hash of a definition: `blake3` of its bytes exactly as given (no normalisation: two texts that differ in one
/// byte are two feeds).
pub fn feed_hash_of(text: &[u8]) -> [u8; 32] {
    *blake3::hash(text).as_bytes()
}

/// The wallet's check of a feed definition (see [`FeedCheck`]).
pub fn check_feed(text: &[u8]) -> FeedCheck {
    let hash = feed_hash_of(text);
    let s = String::from_utf8_lossy(text);
    let mut suspicious = vec![];
    for (i, ch) in s.char_indices() {
        let why = match ch {
            '\n' | '\t' => continue,
            '\r' => "carriage return (a line can be overprinted)",
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => "bidirectional override (text renders in another order)",
            '\u{200b}'..='\u{200f}' | '\u{2060}'..='\u{2064}' | '\u{feff}' | '\u{00ad}' => "invisible character",
            '\u{fffd}' => "not valid UTF-8",
            c if c.is_control() => "control character",
            c if !c.is_ascii() => "outside printable ASCII (a homoglyph of a letter or digit is possible)",
            _ => continue,
        };
        suspicious.push((i, ch, why));
    }
    let lower = s.to_ascii_lowercase();
    let missing = FEED_KEYS.iter().copied().filter(|k| !lower.lines().any(|l| l.trim_start().starts_with(&format!("{k}:")))).collect();
    FeedCheck { hash, suspicious, missing }
}

// ---------------------------------------------------------------- the binding and the states

/// The binding of a pair to its underwriter (`KpfPairO` state `[411..504)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OBinding {
    /// The covenant id of the underwriter's slot.
    pub uw_id: [u8; 32],
    /// The slot's owner: the only key that can vouch a price for the pair.
    pub uw_key: [u8; 32],
    /// Cover per whole unit; the cover of a part of n units is `q(n, cover_rate)`.
    pub cover_rate: i64,
    /// X: the cover's expiry.
    pub expiry: i64,
    /// Premium per whole unit per term.
    pub premium: i64,
}

impl OBinding {
    fn fields(&self) -> Vec<F> {
        vec![F::B(self.uw_id), F::B(self.uw_key), F::I(self.cover_rate), F::I(self.expiry), F::I(self.premium)]
    }
    fn read(c: &mut Cursor) -> Result<OBinding> {
        Ok(OBinding { uw_id: c.b32()?, uw_key: c.b32()?, cover_rate: c.int()?, expiry: c.int()?, premium: c.int()? })
    }
}

/// The build constants of the three templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OConsts {
    pub bond_tpl: [u8; 32],
    pub bond_pre: i64,
    pub bond_suf: i64,
    pub bind_tag: [u8; 4],
    pub judge_tpl: [u8; 32],
    pub judge_pre: i64,
    pub judge_suf: i64,
    pub open_tag: [u8; 4],
    pub resolve_tag: [u8; 4],
    pub pending_tpl: [u8; 32],
    pub pair_tpl: [u8; 32],
}

impl OConsts {
    fn bond_args(&self) -> Vec<ArtifactValue> {
        vec![
            ArtifactValue::Bytes(self.bond_tpl.to_vec()),
            ArtifactValue::Int(self.bond_pre),
            ArtifactValue::Int(self.bond_suf),
            ArtifactValue::Bytes(self.bind_tag.to_vec()),
        ]
    }
    /// `KpfPendingO`'s build constants.
    pub fn pending_args(&self) -> Vec<ArtifactValue> {
        vec![
            ArtifactValue::Bytes(self.judge_tpl.to_vec()),
            ArtifactValue::Int(self.judge_pre),
            ArtifactValue::Int(self.judge_suf),
            ArtifactValue::Bytes(self.open_tag.to_vec()),
            ArtifactValue::Bytes(self.resolve_tag.to_vec()),
        ]
    }
    /// `KpfPairO`'s build constants.
    pub fn pair_args(&self) -> Vec<ArtifactValue> {
        let mut v = vec![ArtifactValue::Bytes(self.pending_tpl.to_vec())];
        v.extend(self.bond_args());
        v
    }
    /// `KpfOpenO`'s build constants.
    pub fn open_args(&self) -> Vec<ArtifactValue> {
        let mut v = vec![ArtifactValue::Bytes(self.pair_tpl.to_vec())];
        v.extend(self.bond_args());
        v
    }
}

/// The templates of the oracle family (the pinned ones, `crate::oracle_artifacts`, or a fresh compile in the tests).
#[derive(Clone, Debug)]
pub struct OKit {
    pub open: Template,
    pub pair: Template,
    pub pending: Template,
    pub bond: Template,
    pub stake: Template,
    pub consts: OConsts,
}

fn p2sh_hash(redeem: &[u8]) -> [u8; 32] {
    let spk = kaspa_txscript::pay_to_script_hash_script(redeem);
    spk.script()[2..34].try_into().expect("P2SH script")
}

/// An oracle-market order (`KpfOpenO` state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OOpen {
    pub m: OMarket,
    pub side: i64,
    pub owner: [u8; 32],
    pub size_left: i64,
    pub limit: i64,
    pub my_rate: i64,
    pub min_counter_rate: i64,
    pub min_fill: i64,
    pub tip: i64,
    pub active_from: i64,
    pub expiry_daa: i64,
    /// The slot id accepted; zero: any slot of the market.
    pub uw_accept: [u8; 32],
    pub prem_max: i64,
}

impl OOpen {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([
            F::I(self.side),
            F::B(self.owner),
            F::I(self.size_left),
            F::I(self.limit),
            F::I(self.my_rate),
            F::I(self.min_counter_rate),
            F::I(self.min_fill),
            F::I(self.tip),
            F::I(self.active_from),
            F::I(self.expiry_daa),
            F::B(self.uw_accept),
            F::I(self.prem_max),
        ]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_with(&self, c: &OConsts) -> Vec<ArtifactValue> {
        let mut a = c.open_args();
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn redeem(&self, k: &OKit) -> Vec<u8> {
        k.open.redeem(&self.state_bytes())
    }
    pub fn decode_state(b: &[u8]) -> Result<OOpen> {
        let mut c = Cursor { b, at: 0 };
        let m = read_market(&mut c)?;
        let o = OOpen {
            m,
            side: c.int()?,
            owner: c.b32()?,
            size_left: c.int()?,
            limit: c.int()?,
            my_rate: c.int()?,
            min_counter_rate: c.int()?,
            min_fill: c.int()?,
            tip: c.int()?,
            active_from: c.int()?,
            expiry_daa: c.int()?,
            uw_accept: c.b32()?,
            prem_max: c.int()?,
        };
        c.done()?;
        Ok(o)
    }
    /// This order's half of a premium (`KpfOpenO.fill`): the long `premium - floor(premium / 2)`, the short the floor.
    pub fn premium_half(&self, premium: i64) -> i64 {
        if self.side == LONG {
            premium - premium / 2
        } else {
            premium / 2
        }
    }
    /// What a fill of n takes from the escrow (`KpfOpenO.escrow`): `q(n, myRate + tip + half) + carrier`.
    pub fn fill_spend(&self, n: i64, premium: i64) -> i64 {
        q(n, self.my_rate + self.tip + self.premium_half(premium), self.m.p.scale, 0) + self.m.p.carrier
    }
    /// What a take of n takes from the escrow: `q(n, myRate + tip) + carrier`.
    pub fn take_spend(&self, n: i64) -> i64 {
        q(n, self.my_rate + self.tip, self.m.p.scale, 0) + self.m.p.carrier
    }
}

fn read_market(c: &mut Cursor) -> Result<OMarket> {
    let id = c.b32()?;
    let m = OMarket::read_body(c)?;
    if m.id() != id {
        return Err(Error::Decode("the market id is not blake3 of the market body".into()));
    }
    Ok(m)
}

/// An oracle-market pair (`KpfPairO` state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OPair {
    pub m: OMarket,
    pub size: i64,
    pub bl: i64,
    pub bs: i64,
    pub entry: i64,
    pub long_key: [u8; 32],
    pub short_key: [u8; 32],
    pub b: OBinding,
    pub long_ask: i64,
    pub long_min: i64,
    pub long_tip: i64,
    pub short_bid: i64,
    pub short_min: i64,
    pub short_tip: i64,
    /// Who created this UTXO (review 7), zero in every state but a fresh part's in its creating transaction.
    pub mk: Makers,
}

/// The maker commitment of an input (review 7, `KpfOpenO.me` / `KpfPairO.me`): blake2b-256 of its outpoint, the
/// transaction id's 32 bytes and then the index as 8 bytes little-endian. Unique per outpoint, so per input, even for
/// twins and split parts that share a covenant id.
pub fn maker_of(op: &kaspa_consensus_core::tx::TransactionOutpoint) -> [u8; 32] {
    let mut b = op.transaction_id.as_bytes().to_vec();
    b.extend_from_slice(&(op.index as u64).to_le_bytes());
    // P2SH's hash is blake2b-256 of the redeem script: the same function the contracts' `blake2b` computes.
    p2sh_hash(&b)
}

/// The makers of a pair state (review 7, R7-F1; `KpfPairO` state `[558..723)`): one field per role an input can create
/// a fresh part in, each the maker commitment (`maker_of`) of that input or zero. A fresh output is relied on by its
/// creating inputs only through the role field holding their own commitment, so two inputs in one role never accept one
/// output; every other state carries every field zero (no entry copies them).
///
/// | output | roles (the rest zero) |
/// |---|---|
/// | a pair opened by orders (`KpfOpenO.fill`) | `long` (the long order), `short` (the short order) |
/// | a part handed over to a signing taker (`KpfPairO.transfer`) | `parent` (the pair) |
/// | a part an order takes (`transfer` and `KpfOpenO.take`) | `parent` (the pair), `long` or `short` (the order) |
/// | P3 of close-versus-close (`closeHost`, `closeDonor`) | `host`, `donor` |
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Makers {
    pub long: [u8; 32],
    pub short: [u8; 32],
    pub parent: [u8; 32],
    pub host: [u8; 32],
    pub donor: [u8; 32],
}

impl Makers {
    pub fn is_zero(&self) -> bool {
        *self == Makers::default()
    }
    /// The fields in state order with their role names.
    pub fn roles(&self) -> [(&'static str, [u8; 32]); 5] {
        [("long", self.long), ("short", self.short), ("parent", self.parent), ("host", self.host), ("donor", self.donor)]
    }
}

impl OPair {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([F::I(self.size), F::I(self.bl), F::I(self.bs), F::I(self.entry), F::B(self.long_key), F::B(self.short_key)]);
        f.extend(self.b.fields());
        f.extend([self.long_ask, self.long_min, self.long_tip, self.short_bid, self.short_min, self.short_tip].map(F::I));
        f.extend([F::B(self.mk.long), F::B(self.mk.short), F::B(self.mk.parent), F::B(self.mk.host), F::B(self.mk.donor)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_with(&self, c: &OConsts) -> Vec<ArtifactValue> {
        let mut a = c.pair_args();
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn redeem(&self, k: &OKit) -> Vec<u8> {
        k.pair.redeem(&self.state_bytes())
    }
    /// The P2SH script hash of this state (what a pending part pays to with `new ScriptPubKeyP2SH(hash)`).
    pub fn script_hash(&self, k: &OKit) -> [u8; 32] {
        p2sh_hash(&self.redeem(k))
    }
    pub fn decode_state(b: &[u8]) -> Result<OPair> {
        let mut c = Cursor { b, at: 0 };
        let m = read_market(&mut c)?;
        let p = OPair {
            m,
            size: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            entry: c.int()?,
            long_key: c.b32()?,
            short_key: c.b32()?,
            b: OBinding::read(&mut c)?,
            long_ask: c.int()?,
            long_min: c.int()?,
            long_tip: c.int()?,
            short_bid: c.int()?,
            short_min: c.int()?,
            short_tip: c.int()?,
            mk: Makers { long: c.b32()?, short: c.b32()?, parent: c.b32()?, host: c.b32()?, donor: c.b32()? },
        };
        c.done()?;
        Ok(p)
    }
    pub fn range_of(&self, n: i64) -> i64 {
        q(n, self.bs - self.bl, self.m.p.scale, 0)
    }
    /// The least value of the pair: `q(s, bs - bl) + carrier`.
    pub fn floor(&self) -> i64 {
        self.range_of(self.size) + self.m.p.carrier
    }
    /// The cover of a part of n: `q(n, coverRate)`.
    pub fn cover_of(&self, n: i64) -> i64 {
        q(n, self.b.cover_rate, self.m.p.scale, 0)
    }
    /// The range wider than both maintenance bands (`bl * (10^4 + m) < bs * (10^4 - m)`).
    pub fn range_ok(&self) -> bool {
        let m = self.m.p.mmr_bps as i128;
        (self.bl as i128) * (BPS as i128 + m) < (self.bs as i128) * (BPS as i128 - m)
    }
    /// The part n of this pair (keys, binding, listings kept), tags cleared.
    pub fn rest_of(&self, n: i64) -> OPair {
        OPair { size: n, mk: Makers::default(), ..self.clone() }
    }
    fn unlisted_part(&self, n: i64) -> OPair {
        OPair { long_ask: 0, long_min: 0, long_tip: 0, short_bid: 0, short_min: 0, short_tip: 0, ..self.rest_of(n) }
    }
    /// The long's and the short's payoff at a price (carrier excluded).
    pub fn long_at(&self, p: i64) -> i64 {
        q(self.size, p.clamp(self.bl, self.bs) - self.bl, self.m.p.scale, 0)
    }
    pub fn short_at(&self, p: i64) -> i64 {
        q(self.size, self.bs - p.clamp(self.bl, self.bs), self.m.p.scale, 0)
    }
    /// Whether a price claim `p` liquidates the long (`KpfPairO.propose`):
    /// `ceil(p * (10^4 + g) / 10^4) * (10^4 - m) < bl * 10^4`, and the range is wider than both bands.
    pub fn long_liquidatable(&self, p: i64) -> bool {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let pt = (p as i128 * (b + g) + b - 1) / b;
        self.range_ok() && pt * (b - m) < self.bl as i128 * b
    }
    /// Whether a price claim `p` liquidates the short: `bs * 10^4 < floor(p * (10^4 - g) / 10^4) * (10^4 + m)`.
    pub fn short_liquidatable(&self, p: i64) -> bool {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let pt = p as i128 * (b - g) / b;
        self.range_ok() && (self.bs as i128) * b < pt * (b + m)
    }
    /// The trigger price T of a loser `side` from the pair's own state (as KpfPair).
    pub fn trigger_of(&self, side: i64) -> i64 {
        let m = self.m.p.mmr_bps;
        if side == LONG {
            self.bl * BPS / (BPS - m)
        } else {
            (self.bs * BPS + BPS + m - 1) / (BPS + m)
        }
    }
    /// The keeper fee of a liquidation of n at price p (`KpfPairO.propose`): `min(max(keeperFeeMin, q(n, floor(p *
    /// liqFeeBps / 10^4))), maintenance slice)`.
    pub fn fee_of(&self, n: i64, side: i64, p: i64) -> i64 {
        let sc = self.m.p.scale;
        let f = q(n, p * self.m.p.liq_fee_bps / BPS, sc, 0).max(self.m.p.keeper_fee_min);
        let t = self.trigger_of(side);
        let slice = if side == LONG { q(n, t - self.bl, sc, 0) } else { q(n, self.bs - t, sc, 0) };
        f.min(slice)
    }
    /// The cure bound of a liquidation claim at p (as KpfPair): `None` when there is no cure (the short's bound above
    /// priceMax, or the cured range above the cover rate).
    pub fn cure_bound_of(&self, side: i64, p: i64) -> Option<i64> {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let pp = p as i128;
        let (cb, range) = if side == LONG {
            let pg = (pp * (b - g) + b - 1) / b;
            let cb = pg * (b - m) / b;
            (cb, self.bs as i128 - cb)
        } else {
            let pg = pp * (b + g) / b;
            let cb = (pg * (b + m) + b - 1) / b;
            (cb, cb - self.bl as i128)
        };
        // Review 9 (R9-F3): measured against the winner's reach, not its bound.
        let reach = if side == LONG { self.short_reach() as i128 - cb } else { cb - self.long_reach() as i128 };
        let ok = (side == LONG || cb <= self.m.p.price_max as i128) && range <= self.b.cover_rate as i128 && reach <= self.b.cover_rate as i128;
        ok.then_some(cb as i64)
    }
    /// Review 9 (R9-F3, `KpfPairO.shortReach`): the outermost short bound the long must leave room for: the short's bound,
    /// or the end of its share `entry + floor(coverRate / 2)` if that is higher.
    pub fn short_reach(&self) -> i64 {
        (self.entry.clamp(0, self.m.p.price_max) + self.b.cover_rate / 2).max(self.bs)
    }
    /// `KpfPairO.longReach`: the long's bound, or the end of its share `entry - (coverRate - floor(coverRate / 2))` if
    /// that is lower.
    pub fn long_reach(&self) -> i64 {
        (self.entry.clamp(0, self.m.p.price_max) - (self.b.cover_rate - self.b.cover_rate / 2)).min(self.bl)
    }
    /// The furthest a side's bound may grow by a price-free entry (`addMargin`, an entrant's bound, P3's entering side):
    /// the long's lowest bound and the short's highest, within the range rule, the other side's reach and priceMax.
    pub fn growth_limit(&self, side: i64) -> i64 {
        if side == LONG {
            (self.short_reach() - self.b.cover_rate).max(0)
        } else {
            (self.long_reach() + self.b.cover_rate).min(self.m.p.price_max)
        }
    }
    /// The forfeit bound (as KpfPair): the loser's bound moved inward to the nearest bound at which the narrowed range
    /// plus the fee still fits the part.
    pub fn forfeit_bound_of(&self, n: i64, side: i64, fee: i64) -> i64 {
        let sc = self.m.p.scale;
        let part = self.range_of(n);
        let fits = |w: i64| q(n, w, sc, 0) + fee <= part;
        let (mut lo, mut hi) = (1i64, self.bs - self.bl);
        if fits(hi) {
            lo = hi;
        } else {
            while lo < hi {
                let mid = lo + (hi - lo + 1) / 2;
                if fits(mid) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
        }
        if side == LONG {
            self.bs - lo
        } else {
            self.bl + lo
        }
    }
    /// The renewal (`KpfPairO.renew`): the continuation (bounds moved inward by the premium, expiry + term, tags 0) and
    /// what may leave the pair, `q(s, bs - bl) - q(s, new range)`; `None` when the range would no longer be wider than
    /// both maintenance bands.
    pub fn renewed(&self) -> Option<(OPair, i64)> {
        let pr = self.b.premium;
        let mut r = OPair { mk: Makers::default(), ..self.clone() };
        r.bl = self.bl + pr - pr / 2;
        r.bs = self.bs - pr / 2;
        r.b.expiry = self.b.expiry + self.m.bond.term;
        if pr < 0 || r.bl >= r.bs || !r.range_ok() {
            return None;
        }
        let pay = self.range_of(self.size) - r.range_of(self.size);
        Some((r, pay))
    }
    /// Review 9 (R9-F1, `KpfPairO.renew`): a pair UTXO accepted at `utxo_daa` can be renewed only if that is no later
    /// than its expiry (a lock time is only a lower bound on the present; the UTXO's DAA score is exact). The refusal
    /// text when it cannot, `None` when it can.
    pub fn unrenewable_why(&self, utxo_daa: u64) -> Option<String> {
        if utxo_daa as i128 <= self.b.expiry as i128 {
            return None;
        }
        Some(format!(
            "the pair UTXO was accepted at DAA {utxo_daa}, after its cover's expiry {}: the contract renews only a pair UTXO accepted no later than its expiry (review 9), so a pair a party touched after the expiry, or one written after it, is never renewed again. Its cover has lapsed; anyone can settle it at its entry price from DAA {}",
            self.b.expiry,
            self.lapse_from()
        ))
    }
    /// The first DAA at which a lapsed pair can be settled at its entry (`lapseSettle`): idleDaa after the cover's
    /// liability ended, by the calendar (review 6, R6-F3: no longer the UTXO's age, which an amend restarted).
    pub fn lapse_from(&self) -> u64 {
        (self.b.expiry + self.m.bond.tail + self.m.p.idle_daa).max(0) as u64
    }
    /// `lapseSettle` (as KpfPair's idle settle): (long, short) of a pair of `value`.
    pub fn lapse_out(&self, value: u64) -> (i64, i64) {
        let e = self.entry.clamp(self.bl, self.bs);
        let v = value as i64;
        let lp = q(self.size, e - self.bl, self.m.p.scale, 0);
        let sp = v - lp;
        if lp < PLAIN_DUST {
            (0, v)
        } else if sp < PLAIN_DUST {
            (v, 0)
        } else {
            (lp, sp)
        }
    }

    /// The restore state of a claim on the part n: keys, bounds, entry and binding unchanged, unlisted, tags 0.
    pub fn restore_state(&self, n: i64) -> OPair {
        self.unlisted_part(n)
    }
    /// The cure state: the loser's bound at `cb`.
    pub fn cure_state(&self, n: i64, side: i64, cb: i64) -> OPair {
        let mut c = self.unlisted_part(n);
        if side == LONG {
            c.bl = cb;
        } else {
            c.bs = cb;
        }
        c
    }
    /// The forfeit state: both keys the winner's, the loser's bound at `fb`.
    pub fn forfeit_state(&self, n: i64, side: i64, fb: i64) -> OPair {
        let mut f = self.unlisted_part(n);
        let w = if side == LONG { self.short_key } else { self.long_key };
        f.long_key = w;
        f.short_key = w;
        if side == LONG {
            f.bl = fb;
        } else {
            f.bs = fb;
        }
        f
    }

    /// The pending part `propose` writes for a claim (kind, side, price p at t0) on the part n, and the forfeit bound
    /// argument `fb`. Refused (`None`) when the claim cannot be proposed: a liquidation that does not trigger, of a part
    /// whose keys are equal, or whose range is not wider than both bands.
    pub fn pending_of(&self, k: &OKit, n: i64, kind: i64, side: i64, p: i64, t0: i64) -> Option<(OPending, i64)> {
        let restore = self.restore_state(n);
        let (mut cure_bound, mut cure_hash, mut forfeit_hash, mut fee, mut fb) = (0, [0u8; 32], [0u8; 32], 0, if side == LONG { self.bl } else { self.bs });
        if kind == KIND_LIQ {
            if self.long_key == self.short_key {
                return None;
            }
            let trig = if side == LONG { self.long_liquidatable(p) } else { self.short_liquidatable(p) };
            if !trig {
                return None;
            }
            fee = self.fee_of(n, side, p);
            fb = self.forfeit_bound_of(n, side, fee);
            forfeit_hash = self.forfeit_state(n, side, fb).script_hash(k);
            if let Some(cb) = self.cure_bound_of(side, p) {
                cure_bound = cb;
                cure_hash = self.cure_state(n, side, cb).script_hash(k);
            }
        } else if kind != KIND_CLOSE {
            return None;
        }
        let bond = self.m.bond_of(n, self.b.cover_rate);
        let refund = bond + if n < self.size { self.m.p.carrier } else { 0 };
        Some((
            OPending {
                m: self.m,
                kind,
                side,
                n,
                bl: self.bl,
                bs: self.bs,
                quote: p,
                t0,
                cure_bound,
                fee,
                bond,
                refund,
                status: PENDING,
                clock: 0,
                disputer: [0; 32],
                entry: self.entry,
                long_key: self.long_key,
                short_key: self.short_key,
                b: self.b,
                long_ask: self.long_ask,
                long_min: self.long_min,
                long_tip: self.long_tip,
                short_bid: self.short_bid,
                short_min: self.short_min,
                short_tip: self.short_tip,
                restore_hash: restore.script_hash(k),
                cure_hash,
                forfeit_hash,
                mk_parent: [0; 32],
            },
            fb,
        ))
    }
    /// The least value of a pending part `propose` writes from a pair of `value`: `value + B` for the whole pair,
    /// `q(n, bs - bl) + refund` for a part (the proposer funds the part's carrier).
    pub fn pending_value(&self, value: u64, w: &OPending) -> i64 {
        if w.n == self.size {
            value as i64 + w.bond
        } else {
            self.range_of(w.n) + w.refund
        }
    }
    /// The least value of the remainder of a split of n from a pair of `value`.
    pub fn rest_value(&self, value: u64, n: i64) -> i64 {
        (q(self.size - n, self.bs - self.bl, self.m.p.scale, 0) + self.m.p.carrier).max(value as i64 - self.range_of(n))
    }
}

/// An oracle-market pending part (`KpfPendingO` state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OPending {
    pub m: OMarket,
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub bl: i64,
    pub bs: i64,
    pub quote: i64,
    pub t0: i64,
    pub cure_bound: i64,
    pub fee: i64,
    pub bond: i64,
    pub refund: i64,
    pub status: i64,
    /// P once contested (0 while pending).
    pub clock: i64,
    /// The opening staker once contested.
    pub disputer: [u8; 32],
    pub entry: i64,
    pub long_key: [u8; 32],
    pub short_key: [u8; 32],
    pub b: OBinding,
    pub long_ask: i64,
    pub long_min: i64,
    pub long_tip: i64,
    pub short_bid: i64,
    pub short_min: i64,
    pub short_tip: i64,
    pub restore_hash: [u8; 32],
    pub cure_hash: [u8; 32],
    pub forfeit_hash: [u8; 32],
    /// The maker (review 7): the outpoint commitment (`maker_of`) of the pair input whose propose wrote this part; zero
    /// in every later state (contest and decide clear it).
    pub mk_parent: [u8; 32],
}

/// What a valid or void claim's finalize pays, and to whom (outputs relative to the part's input index i and the
/// output count M).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settlement {
    /// kind 1: the long at output i, the short at M - 1 - i (0: no output).
    Close { long: i64, short: i64 },
    /// kind 2: the forfeited part (its state and least value), the underwriter B + fee at output i.
    Forfeit { part: OPair, value: i64, uw: i64 },
    /// Refuted or void: the restored part (least value), the defender at i, the disputer at M - 1 - i.
    Restore { part: OPair, value: i64, defender: i64, disputer: i64 },
}

impl OPending {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend(
            [self.kind, self.side, self.n, self.bl, self.bs, self.quote, self.t0, self.cure_bound, self.fee, self.bond, self.refund, self.status, self.clock]
                .map(F::I),
        );
        f.push(F::B(self.disputer));
        f.extend([F::I(self.entry), F::B(self.long_key), F::B(self.short_key)]);
        f.extend(self.b.fields());
        f.extend([self.long_ask, self.long_min, self.long_tip, self.short_bid, self.short_min, self.short_tip].map(F::I));
        f.extend([F::B(self.restore_hash), F::B(self.cure_hash), F::B(self.forfeit_hash), F::B(self.mk_parent)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_with(&self, c: &OConsts) -> Vec<ArtifactValue> {
        let mut a = c.pending_args();
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn redeem(&self, k: &OKit) -> Vec<u8> {
        k.pending.redeem(&self.state_bytes())
    }
    pub fn decode_state(b: &[u8]) -> Result<OPending> {
        let mut c = Cursor { b, at: 0 };
        let m = read_market(&mut c)?;
        let mut w = OPending {
            m,
            kind: c.int()?,
            side: c.int()?,
            n: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            quote: c.int()?,
            t0: c.int()?,
            cure_bound: c.int()?,
            fee: c.int()?,
            bond: c.int()?,
            refund: c.int()?,
            status: c.int()?,
            clock: c.int()?,
            disputer: c.b32()?,
            entry: 0,
            long_key: [0; 32],
            short_key: [0; 32],
            b: OBinding { uw_id: [0; 32], uw_key: [0; 32], cover_rate: 0, expiry: 0, premium: 0 },
            long_ask: 0,
            long_min: 0,
            long_tip: 0,
            short_bid: 0,
            short_min: 0,
            short_tip: 0,
            restore_hash: [0; 32],
            cure_hash: [0; 32],
            forfeit_hash: [0; 32],
            mk_parent: [0; 32],
        };
        w.entry = c.int()?;
        w.long_key = c.b32()?;
        w.short_key = c.b32()?;
        w.b = OBinding::read(&mut c)?;
        (w.long_ask, w.long_min, w.long_tip, w.short_bid, w.short_min, w.short_tip) = (c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?);
        w.restore_hash = c.b32()?;
        w.cure_hash = c.b32()?;
        w.forfeit_hash = c.b32()?;
        w.mk_parent = c.b32()?;
        c.done()?;
        Ok(w)
    }

    /// The pair state the part came from (a part of size n, with the listings it had): what restore, cure and forfeit
    /// are built from.
    pub fn pair_part(&self) -> OPair {
        OPair {
            m: self.m,
            size: self.n,
            bl: self.bl,
            bs: self.bs,
            entry: self.entry,
            long_key: self.long_key,
            short_key: self.short_key,
            b: self.b,
            long_ask: self.long_ask,
            long_min: self.long_min,
            long_tip: self.long_tip,
            short_bid: self.short_bid,
            short_min: self.short_min,
            short_tip: self.short_tip,
            mk: Makers::default(),
        }
    }

    /// The key of the claim's proposer: the closer for a close, the underwriter for a liquidation.
    pub fn proposer(&self) -> [u8; 32] {
        if self.kind == KIND_CLOSE {
            if self.side == LONG {
                self.long_key
            } else {
                self.short_key
            }
        } else {
            self.b.uw_key
        }
    }
    /// The party the claim could wrong: the other party of a close, the loser of a liquidation.
    pub fn defender(&self) -> [u8; 32] {
        let long = if self.kind == KIND_CLOSE { self.side == SHORT } else { self.side == LONG };
        if long {
            self.long_key
        } else {
            self.short_key
        }
    }

    /// This pending part as `propose` writes it from the pair input at outpoint `op`: its maker field names that input
    /// (review 7). `OPair::pending_of` leaves it zero, since a state alone does not know its outpoint.
    pub fn made_by(&self, op: &kaspa_consensus_core::tx::TransactionOutpoint) -> OPending {
        OPending { mk_parent: maker_of(op), ..self.clone() }
    }

    /// The claim's validity at P (`KpfPendingO.finalize`): `P + W <= X <= P + term + step` and `P - t0 <= lateDaa`.
    pub fn valid_at(&self, p: i64) -> bool {
        let (w, x) = (self.m.p.challenge_daa as i128, self.b.expiry as i128);
        let pp = p as i128;
        pp + w <= x && x <= pp + (self.m.bond.term + self.m.bond.step) as i128 && pp - self.t0 as i128 <= self.m.p.late_daa as i128
    }
    /// Whether a contest accepted at `c` (the contested part's own DAA score) of a claim accepted at `p` was in time.
    pub fn contest_in_time(&self, p: i64, c: i64) -> bool {
        (c as i128) < p as i128 + self.m.p.challenge_daa as i128
    }
    /// The contested state (`KpfPendingO.contest`).
    pub fn contested(&self, p: i64, disputer: [u8; 32]) -> OPending {
        OPending { status: CONTESTED, clock: p, disputer, mk_parent: [0; 32], ..self.clone() }
    }
    /// The decided state (`KpfPendingO.decide`): the verdict (`yes`: the claim stands), ignored when the contest at `c`
    /// was late.
    pub fn decided(&self, yes: bool, c: i64) -> OPending {
        let st = if yes || !self.contest_in_time(self.clock, c) { UPHELD } else { REFUTED };
        OPending { status: st, mk_parent: [0; 32], ..self.clone() }
    }
    /// The close price of a close claim: the vouched price moved by g against the closer, clamped.
    pub fn close_price(&self) -> i64 {
        let (b, g) = (BPS as i128, self.m.p.g_bps as i128);
        let pc = if self.side == LONG { self.quote as i128 * (b - g) / b } else { (self.quote as i128 * (b + g) + b - 1) / b };
        (pc as i64).clamp(self.bl, self.bs)
    }
    /// The first DAA at which this part (accepted at `utxo_daa`) can be finalized while pending.
    pub fn finalize_from(&self, utxo_daa: u64) -> u64 {
        utxo_daa.saturating_add(self.m.p.challenge_daa.max(0) as u64)
    }
    /// The last DAA at which a contest of a part accepted at `utxo_daa` is still in time.
    pub fn contest_until(&self, utxo_daa: u64) -> u64 {
        utxo_daa.saturating_add(self.m.p.challenge_daa.max(1) as u64) - 1
    }

    /// What finalize pays for a part of `value` whose P is `p` (`KpfPendingO.finalize`).
    pub fn settlement(&self, value: u64, p: i64) -> Settlement {
        let v = value as i64;
        let part = self.pair_part();
        if self.status == REFUTED || !self.valid_at(p) {
            let (disputer, defender) = self.m.bond_split(self.bond, self.status == REFUTED);
            return Settlement::Restore { part: part.restore_state(self.n), value: v - self.bond, defender, disputer };
        }
        if self.kind == KIND_CLOSE {
            let c = self.close_price();
            let sc = self.m.p.scale;
            let owe = if self.side == LONG { q(self.n, c - self.bl, sc, 0) } else { q(self.n, self.bs - c, sc, 0) };
            let (mut me, mut other) = (owe + self.refund, v - owe - self.refund);
            if other < PLAIN_DUST {
                me = v;
                other = 0;
            }
            if self.side == LONG {
                Settlement::Close { long: me, short: other }
            } else {
                Settlement::Close { long: other, short: me }
            }
        } else {
            let fb = part.forfeit_bound_of(self.n, self.side, self.fee);
            Settlement::Forfeit { part: part.forfeit_state(self.n, self.side, fb), value: v - self.bond - self.fee, uw: self.bond + self.fee }
        }
    }

    /// The cure (`KpfPendingO.cure`): the cured part and its least value from a part of `value`; `None` when there is
    /// no cure.
    pub fn cure(&self, value: u64) -> Option<(OPair, i64)> {
        if self.kind == KIND_CLOSE || self.cure_hash == [0; 32] {
            return None;
        }
        let part = self.pair_part();
        let sc = self.m.p.scale;
        let (c, add) = if self.side == LONG {
            (part.cure_state(self.n, LONG, self.cure_bound), q(self.n, self.bl - self.cure_bound, sc, sc - 1))
        } else {
            (part.cure_state(self.n, SHORT, self.cure_bound), q(self.n, self.cure_bound - self.bs, sc, sc - 1))
        };
        let floor = q(self.n, c.bs - c.bl, sc, 0) + self.m.p.carrier;
        Some((c, floor.max(value as i64 - self.refund + add)))
    }
}

/// The ring capacity a slot of value `v` gives at rho (`KpfBond.capacity`), re-exported for the tools.
pub fn capacity(v: i64, rho_bps: i64) -> i64 {
    crate::bond::capacity(v, rho_bps)
}

// ---------------------------------------------------------------- worst cases a wallet shows

/// The figures a wallet states for a market before an order is placed in it (`kpf oracle-check`), for a pair of
/// `size` units at entry `x` with the long's margin rate `rl` and the short's `rs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorstCase {
    /// Each side's margin: what a false claim that nobody disputes within the window can take from it.
    pub long_margin: i64,
    pub short_margin: i64,
    /// The bond a claim on the whole pair carries (what a refuted claim costs its proposer).
    pub bond: i64,
    /// The disputer's share of a refuted bond, and the defender's.
    pub disputer_share: i64,
    pub defender_share: i64,
    /// The contest window, the quiet time and the lateness limit, DAA.
    pub window: i64,
    /// What one underwriter of a slot of value `k` can underwrite at most: `rho x K`.
    pub rho_bps: i64,
    /// The premium of one term at `premium` per whole unit, and each side's half.
    pub term_premium: i64,
}

impl OMarket {
    /// The worst-case figures for a pair of `size` at entry `x` with rates `rl` (long) and `rs` (short) and cover rate
    /// `cr` (at least `rl + rs`), paying `premium` per whole unit per term.
    pub fn worst_case(&self, size: i64, rl: i64, rs: i64, cr: i64, premium: i64) -> WorstCase {
        let sc = self.p.scale;
        let bond = self.bond_of(size, cr);
        let (share, rest) = self.bond_split(bond, true);
        WorstCase {
            long_margin: q(size, rl, sc, 0),
            short_margin: q(size, rs, sc, 0),
            bond,
            disputer_share: share,
            defender_share: rest,
            window: self.p.challenge_daa,
            rho_bps: self.bond.rho_bps,
            term_premium: q(size, premium, sc, 0),
        }
    }
}

/// The smallest value at which a plain output can stand in a typical transaction of the family: its own storage mass
/// (`10^12 / value` grams, KIP-9) leaves a fifth of the block's storage limit (500,000) to the rest of the transaction.
/// The contracts' own floor for a payout is 0.02 KAS (`PLAIN_DUST`), whose storage mass alone is the whole limit.
pub const VIABLE_OUTPUT_MIN: u64 = 2_500_000;

/// What the KIP-9 storage mass means for a pair of `size` units at entry `x` with rates `rl`, `rs`, cover rate `cr` and
/// `premium` per term: the payouts a contract puts in an output of their own (each side's margin at a lapse or mutual
/// settle, a claim's bond at a cure or a settlement, the disputer's and the defender's shares of a refuted bond), and the
/// premium, which the tools fold into the underwriter's change. A payout below 0.02 KAS goes to the other party
/// (contract rule); one below `VIABLE_OUTPUT_MIN` is topped up by the funder (the tools); a premium below it needs the
/// underwriter as funder or one of its UTXOs.
pub fn payout_warnings(m: &OMarket, size: i64, rl: i64, rs: i64, cr: i64, premium: i64) -> Vec<String> {
    let w = m.worst_case(size, rl, rs, cr, premium);
    let kas = |v: i64| v as f64 / 1e8;
    let viable = VIABLE_OUTPUT_MIN as i64;
    let mut out = vec![];
    for (what, v) in [("the long's margin", w.long_margin), ("the short's margin", w.short_margin), ("a claim's bond", w.bond), ("the disputer's share of a refuted bond", w.disputer_share), ("the defender's share of a refuted bond", w.defender_share)] {
        if v > 0 && v < PLAIN_DUST {
            out.push(format!("{what} is {:.8} KAS, below 0.02 KAS: a contract pays a sum that small to the other party (or keeps it in the pair), so it is lost to its owner", kas(v)));
        } else if v >= PLAIN_DUST && v < viable {
            out.push(format!("{what} is {:.8} KAS: a contract pays it in an output of its own, and an output below about {:.3} KAS cannot be mined (KIP-9 storage mass); the tools top it up from the funder (at most {:.3} KAS)", kas(v), kas(viable), kas(viable - PLAIN_DUST)));
        }
    }
    if w.term_premium > 0 && w.term_premium < viable {
        out.push(format!("the premium of a term is {:.8} KAS, too small for an output of its own (about {:.3} KAS at least, KIP-9 storage mass): the open and every renewal must be funded from the underwriter's key (the premium joins its change) or carry one of its UTXOs (--uw-utxo)", kas(w.term_premium), kas(viable)));
    }
    out
}

/// What a wallet must say about a market whose parameters pass the bounds but leave a party exposed. The contracts
/// enforce only the bounds (every branch live); these are judgements no covenant can make. `daa_per_second` converts
/// windows to wall time (10 on testnet-10).
pub fn market_warnings(m: &OMarket, daa_per_second: i64) -> Vec<String> {
    let s = daa_per_second.max(1);
    let hours = |d: i64| d as f64 / s as f64 / 3600.0;
    let p = &m.p;
    let mut w = vec![];
    if p.challenge_daa < 3_600 * s {
        w.push(format!("the contest window is {:.2} h: a false claim against an offline party finalizes unless someone contests within it (the note asks at least 1 h)", hours(p.challenge_daa)));
    }
    if m.bond.rho_bps > 20_000 {
        w.push(format!("rho is {}: an underwriter may underwrite {} times its bond; a coalition of underwriters can falsely settle that multiple of what it holds", m.bond.rho_bps as f64 / 1e4, m.bond.rho_bps as f64 / 1e4));
    }
    if p.sigma_bps < 500 && p.bond_min < 100_000_000 {
        w.push(format!("a claim's bond is {} % of its cover with a floor of {} KAS: a refuted claim costs its proposer little", p.sigma_bps as f64 / 100.0, p.bond_min as f64 / 1e8));
    }
    if p.beta_bps > 5_000 {
        w.push(format!("the disputer takes {} % of a refuted bond; the wronged party gets the rest", p.beta_bps as f64 / 100.0));
    }
    if p.quiet_daa < 60 * s {
        w.push("the quiet time is under a minute: a party can respend its pair almost every block and void the underwriter's signed claims".into());
    }
    if p.late_daa > p.challenge_daa {
        w.push("a claim may land later after its price than the whole contest window".into());
    }
    if p.idle_daa < 24 * 3_600 * s {
        w.push(format!("a pair whose cover lapsed settles at its entry price after {:.1} h idle", hours(p.idle_daa)));
    }
    if m.judge.min_stake < 10_000_000_000 {
        w.push(format!("a judge stake may be as small as {} KAS: contests are cheap", m.judge.min_stake as f64 / 1e8));
    }
    if m.judge.age < 24 * 3_600 * s {
        w.push(format!("judge stake counts after {:.1} h: capital raised for a dispute counts soon", hours(m.judge.age)));
    }
    // Review 6 (R6-F4): a judge that cannot be reached in practice. The contracts bound each parameter (a round at most
    // 3 days, stake age 30 days, unbonding 90 days, the least stake 10,000 KAS at 10 DAA/s); the wallet warns well inside.
    let days = |d: i64| d as f64 / s as f64 / 86_400.0;
    let mut unreachable = vec![];
    if p.challenge_daa < 3_600 * s {
        unreachable.push(format!("the contest window is only {:.2} h", hours(p.challenge_daa)));
    }
    if m.judge.min_stake > 100_000_000_000 {
        w.push(format!("a judge stake must be at least {} KAS: few defenders can post it", m.judge.min_stake as f64 / 1e8));
        unreachable.push(format!("each stake must be at least {} KAS", m.judge.min_stake as f64 / 1e8));
    }
    if m.judge.age > 14 * 86_400 * s {
        w.push(format!("judge stake counts only {:.1} days after it is bonded: in a new market no claim can be contested for that long", days(m.judge.age)));
        unreachable.push(format!("stake must have been bonded {:.1} days before the claim", days(m.judge.age)));
    }
    if m.judge.round > 24 * 3_600 * s {
        w.push(format!("a dispute round lasts {:.1} days: every contest freezes the part at least that long", days(m.judge.round)));
    }
    if m.judge.unbond > 30 * 86_400 * s {
        w.push(format!("judge stake takes {:.1} days to leave: stakers' capital is locked long", days(m.judge.unbond)));
    }
    if !unreachable.is_empty() {
        w.push(format!("a contest within the window is not realistically possible here: {}; a false price will most likely stand", unreachable.join(", ")));
    }
    if p.mmr_bps < p.g_bps {
        w.push("the maintenance margin is smaller than the tolerance g".into());
    }
    w
}

/// Every bound of a pair state the contracts check on every entry (`KpfPairO.checkPair`) and the binding rules an
/// order writes; for the tools.
pub fn check_pair(p: &OPair) -> std::result::Result<(), Refusal> {
    p.m.check()?;
    let sc = p.m.p.scale;
    chk(p.size > 0 && p.size / sc <= (i64::MAX - p.m.p.carrier) / p.m.p.price_max - 1, "size")?;
    chk(p.bl >= 0 && p.bl < p.bs && p.bs <= p.m.p.price_max, "range")?;
    chk(p.b.cover_rate >= p.bs - p.bl && p.b.cover_rate <= p.m.p.price_max, "the cover rate does not cover the range")?;
    chk(p.b.uw_key != p.long_key && p.b.uw_key != p.short_key, "the underwriter is a party")
}

/// The sample states the pinned artifacts are compiled with (`kpf-artifacts`; a template does not depend on its state).
pub mod samples {
    use super::*;

    /// The feed definition of the sample market (and of the testnet runbook's example, `docs/TESTNET.md`).
    pub const FEED: &str = "kpf feed definition v1\nquantity: example slow underlying, published once per hour\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\ndisagreement: a claim is true if within 0.25 % of the median\n";

    pub fn market() -> OMarket {
        OMarket::testnet(feed_hash_of(FEED.as_bytes()))
    }
    pub fn pair(m: &OMarket) -> OPair {
        OPair {
            m: *m,
            size: 100_000_000_000,
            bl: 900_000_000,
            bs: 1_200_000_000,
            entry: 1_000_000_000,
            long_key: [1; 32],
            short_key: [2; 32],
            b: OBinding { uw_id: [0x51; 32], uw_key: [3; 32], cover_rate: 600_000_000, expiry: 24_192_000, premium: 2_000_000 },
            long_ask: 0,
            long_min: 0,
            long_tip: 0,
            short_bid: 0,
            short_min: 0,
            short_tip: 0,
            mk: Makers { long: [0x1a; 32], short: [0x1b; 32], parent: [0x1c; 32], host: [0x1d; 32], donor: [0x1e; 32] },
        }
    }
    pub fn pending(p: &OPair) -> OPending {
        OPending {
            m: p.m,
            kind: KIND_LIQ,
            side: LONG,
            n: p.size,
            bl: p.bl,
            bs: p.bs,
            quote: 900_000_000,
            t0: 1,
            cure_bound: 800_000_000,
            fee: 1_000_000,
            bond: 100_000_000,
            refund: 100_000_000,
            status: PENDING,
            clock: 0,
            disputer: [0; 32],
            entry: p.entry,
            long_key: p.long_key,
            short_key: p.short_key,
            b: p.b,
            long_ask: 0,
            long_min: 0,
            long_tip: 0,
            short_bid: 0,
            short_min: 0,
            short_tip: 0,
            restore_hash: [0x2a; 32],
            cure_hash: [0x2b; 32],
            forfeit_hash: [0x2c; 32],
            mk_parent: [0x2d; 32],
        }
    }
    pub fn order(m: &OMarket, side: i64, owner: [u8; 32]) -> OOpen {
        OOpen {
            m: *m,
            side,
            owner,
            size_left: 200_000_000_000,
            limit: if side == LONG { 1_050_000_000 } else { 950_000_000 },
            my_rate: if side == LONG { 100_000_000 } else { 200_000_000 },
            min_counter_rate: if side == LONG { 150_000_000 } else { 50_000_000 },
            min_fill: 10_000_000_000,
            tip: 0,
            active_from: 0,
            expiry_daa: 1_000_000_000,
            uw_accept: [0; 32],
            prem_max: 10_000_000,
        }
    }
}

/// Constants the contracts share with the library (checked by the tests against the sources).
pub mod consts {
    pub use super::{MARKET_LEN, OPEN_STATE_LEN, PAIR_STATE_LEN, PENDING_STATE_LEN};
    pub const RING: usize = super::RING;
    pub const RHO_MAX_BPS: i64 = super::RHO_MAX_BPS;
    pub const MU_MIN_BPS: i64 = super::MU_MIN_BPS;
    pub const MU_MAX_BPS: i64 = super::MU_MAX_BPS;
    pub const BURN_MAX_BPS: i64 = super::BURN_MAX_BPS;
    pub const MIN_STAKE_MAX: i64 = super::MIN_STAKE_MAX;
    pub const UNIT: i64 = super::UNIT;
}
