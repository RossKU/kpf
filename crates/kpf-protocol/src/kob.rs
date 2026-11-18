//! KOB's side of a price-evidence transaction: the state codecs of `KobAsk`, `KobBid` and a KCC-20 holder (read from
//! the redeem scripts the pinned KOB artifacts define), live UTXOs of them, and the check of DESIGN 3.3 and 4.5b that
//! says whether an order can serve as evidence for a market.
//!
//! Nothing here is KOB code: the layouts are those of `vendor/kob/contracts/orders/KobAsk.sil`, `KobBid.sil` and
//! `kcc20/KCC20Ref.sil` (state spans of 276, 285 and 112 bytes), and `tests/pinned.rs` plus the engine tests in
//! `crates/kpf-tests` hold them to KOB's compiler output. The transaction builders are in [`crate::evidence`].

use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_txscript::pay_to_script_hash_script;
use serde_json::{json, Value};
use silverscript_abi::ArtifactValue;
use std::collections::BTreeMap;

use crate::artifacts::{ask_template, bid_template, kob_consts, template_hash_of, token_template};
use crate::error::{to_hex, Error, Result};
use crate::payoff::{clamp_price, long_liquidatable, q, short_liquidatable, trigger_of};
use crate::state::{Cursor, Market, Pair, BPS, F, LONG, MAX_IDLE, SHORT};

/// KCC-20 owner scheme 0: a P2PK Schnorr key.
pub const SCHEME_P2PK: u8 = 0x00;
/// KCC-20 owner scheme 4: a covenant id (an order's custody).
pub const SCHEME_COVID: u8 = 0x04;
/// The IOC/FOK kill: refundable by anyone this many DAA after the order became fillable.
pub const IOC_LIFE: i64 = 600;
/// The smallest KAS beyond the delivery carrier that a terminal bid fill returns to its maker as its own output.
pub const REFUND_MIN: i64 = 100_000_000;
/// Smallest value of an output that carries a covenant id (KIP-9 storage mass: two storage units), 0.08 KAS.
pub const COVENANT_OUTPUT_MIN: u64 = 8_000_000;

// ---------------------------------------------------------------- KCC-20 holder

/// A KCC-20 holder state (112 bytes encoded): `amount`, `owner`, owner scheme, borrow scheme, borrow guard, extension.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenState {
    pub amount: i64,
    pub owner: [u8; 32],
    pub scheme: u8,
    pub borrow: u8,
    pub guard: [u8; 32],
    pub ext: Hash,
}

impl TokenState {
    /// A holding owned by a P2PK key, borrowing disabled.
    pub fn p2pk(amount: i64, owner: [u8; 32], ext: Hash) -> TokenState {
        TokenState { amount, owner, scheme: SCHEME_P2PK, borrow: 0, guard: [0; 32], ext }
    }

    /// A custody owned by a covenant id (an ask's), borrowing disabled.
    pub fn covid(amount: i64, owner: Hash, ext: Hash) -> TokenState {
        TokenState { amount, owner: owner.as_bytes(), scheme: SCHEME_COVID, borrow: 0, guard: [0; 32], ext }
    }

    /// The state span: `0x08 amount | 0x20 owner | 0x01 scheme | 0x01 borrow | 0x20 guard | 0x20 extension`.
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut v = vec![0x08];
        v.extend_from_slice(&crate::state::sm8(self.amount));
        v.push(0x20);
        v.extend_from_slice(&self.owner);
        v.extend_from_slice(&[0x01, self.scheme, 0x01, self.borrow, 0x20]);
        v.extend_from_slice(&self.guard);
        v.push(0x20);
        v.extend_from_slice(&self.ext.as_bytes());
        v
    }

    /// Decodes a state span.
    pub fn decode_state(s: &[u8]) -> Result<TokenState> {
        if s.len() != 112 {
            return Err(Error::Decode(format!("a KCC-20 state is 112 bytes, this one {}", s.len())));
        }
        let mut c = Cursor { b: s, at: 0 };
        let amount = c.int()?;
        let owner = c.b32()?;
        if s[c.at] != 0x01 || s[c.at + 2] != 0x01 {
            return Err(Error::Decode("a KCC-20 state has 1-byte pushes for the two schemes".into()));
        }
        let (scheme, borrow) = (s[c.at + 1], s[c.at + 3]);
        c.at += 4;
        let guard = c.b32()?;
        let ext = c.hash()?;
        c.done()?;
        Ok(TokenState { amount, owner, scheme, borrow, guard, ext })
    }

    /// The redeem script of an instance of the pinned token program.
    pub fn redeem(&self) -> Vec<u8> {
        token_template().redeem(&self.state_bytes())
    }

    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    /// Decodes the redeem script of a UTXO of the pinned token program.
    pub fn decode_redeem(redeem: &[u8]) -> Result<TokenState> {
        match token_template().state_of(redeem) {
            Ok(s) => TokenState::decode_state(s),
            Err(e) => Err(Error::Decode(format!("not a UTXO of KOB's reference KCC-20 program ({e}); other token programs are not supported"))),
        }
    }

    /// An element of the leader's `transfer(next_states, witness)`.
    pub fn value(&self) -> ArtifactValue {
        let b = |x: &[u8]| ArtifactValue::Bytes(x.to_vec());
        let mut m = BTreeMap::new();
        m.insert("amount".to_string(), ArtifactValue::Int(self.amount));
        m.insert("owner".to_string(), b(&self.owner));
        m.insert("owner_scheme".to_string(), ArtifactValue::Byte(self.scheme));
        m.insert("borrow_scheme".to_string(), ArtifactValue::Byte(self.borrow));
        m.insert("borrow_guard".to_string(), b(&self.guard));
        m.insert("extension_commitment".to_string(), b(&self.ext.as_bytes()));
        ArtifactValue::Object(m)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KCC20Ref", "amount": self.amount, "owner": to_hex(&self.owner), "ownerScheme": self.scheme,
            "borrowScheme": self.borrow, "extensionCommitment": to_hex(&self.ext.as_bytes()),
        })
    }
}

// ---------------------------------------------------------------- KobAsk

/// `KobAsk` state (276 bytes): a resting offer to sell `amount_left` base units at `price` sompi per whole token.
#[derive(Clone, Debug, PartialEq)]
pub struct AskState {
    pub maker: [u8; 32],
    pub token_cov_id: Hash,
    pub token_tpl_hash: Hash,
    pub pre: i64,
    pub suf: i64,
    pub scale: i64,
    pub min_fill: i64,
    pub price: i64,
    pub tip: i64,
    pub tif: i64,
    pub active_from: i64,
    pub expiry_daa: i64,
    pub refund_tip: i64,
    pub interval: i64,
    pub max_fill: i64,
    pub slope: i64,
    pub price_end: i64,
    pub decay_step: i64,
    pub amount_left: i64,
    pub ext: Hash,
}

impl AskState {
    fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.maker), F::B(self.token_cov_id.as_bytes()), F::B(self.token_tpl_hash.as_bytes())];
        for x in [
            self.pre, self.suf, self.scale, self.min_fill, self.price, self.tip, self.tif, self.active_from, self.expiry_daa, self.refund_tip,
            self.interval, self.max_fill, self.slope, self.price_end, self.decay_step, self.amount_left,
        ] {
            f.push(F::I(x));
        }
        f.push(F::B(self.ext.as_bytes()));
        f
    }

    pub fn state_bytes(&self) -> Vec<u8> {
        crate::state::encode(&self.fields())
    }

    pub fn decode_state(s: &[u8]) -> Result<AskState> {
        let mut c = Cursor { b: s, at: 0 };
        let maker = c.b32()?;
        let token_cov_id = c.hash()?;
        let token_tpl_hash = c.hash()?;
        let mut i = [0i64; 16];
        for x in i.iter_mut() {
            *x = c.int()?;
        }
        let ext = c.hash()?;
        c.done()?;
        Ok(AskState {
            maker,
            token_cov_id,
            token_tpl_hash,
            pre: i[0],
            suf: i[1],
            scale: i[2],
            min_fill: i[3],
            price: i[4],
            tip: i[5],
            tif: i[6],
            active_from: i[7],
            expiry_daa: i[8],
            refund_tip: i[9],
            interval: i[10],
            max_fill: i[11],
            slope: i[12],
            price_end: i[13],
            decay_step: i[14],
            amount_left: i[15],
            ext,
        })
    }

    pub fn redeem(&self) -> Vec<u8> {
        ask_template().redeem(&self.state_bytes())
    }

    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    pub fn decode_redeem(redeem: &[u8]) -> Result<AskState> {
        AskState::decode_state(ask_template().state_of(redeem)?)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KobAsk", "maker": to_hex(&self.maker), "tokenCovId": to_hex(&self.token_cov_id.as_bytes()),
            "tokenTplHash": to_hex(&self.token_tpl_hash.as_bytes()), "scale": self.scale, "minFill": self.min_fill, "price": self.price,
            "tip": self.tip, "tif": self.tif, "activeFrom": self.active_from, "expiryDaa": self.expiry_daa, "interval": self.interval,
            "maxFill": self.max_fill, "slope": self.slope, "amountLeft": self.amount_left, "extensionCommitment": to_hex(&self.ext.as_bytes()),
        })
    }
}

// ---------------------------------------------------------------- KobBid

/// `KobBid` state (285 bytes): a resting offer to buy at `price` sompi per whole token, funded by the UTXO's KAS.
#[derive(Clone, Debug, PartialEq)]
pub struct BidState {
    pub maker: [u8; 32],
    pub token_cov_id: Hash,
    pub token_tpl_hash: Hash,
    pub pre: i64,
    pub suf: i64,
    pub ext: Hash,
    pub scale: i64,
    pub min_fill: i64,
    pub price: i64,
    pub tip: i64,
    pub tif: i64,
    pub active_from: i64,
    pub expiry_daa: i64,
    pub refund_tip: i64,
    pub reserve: i64,
    pub delivery_carrier: i64,
    pub interval: i64,
    pub max_fill: i64,
    pub slope: i64,
    pub price_end: i64,
    pub decay_step: i64,
}

impl BidState {
    fn ints(&self) -> [i64; 15] {
        [
            self.scale, self.min_fill, self.price, self.tip, self.tif, self.active_from, self.expiry_daa, self.refund_tip, self.reserve,
            self.delivery_carrier, self.interval, self.max_fill, self.slope, self.price_end, self.decay_step,
        ]
    }

    pub fn state_bytes(&self) -> Vec<u8> {
        let mut f = vec![
            F::B(self.maker),
            F::B(self.token_cov_id.as_bytes()),
            F::B(self.token_tpl_hash.as_bytes()),
            F::I(self.pre),
            F::I(self.suf),
            F::B(self.ext.as_bytes()),
        ];
        f.extend(self.ints().into_iter().map(F::I));
        crate::state::encode(&f)
    }

    pub fn decode_state(s: &[u8]) -> Result<BidState> {
        let mut c = Cursor { b: s, at: 0 };
        let maker = c.b32()?;
        let token_cov_id = c.hash()?;
        let token_tpl_hash = c.hash()?;
        let pre = c.int()?;
        let suf = c.int()?;
        let ext = c.hash()?;
        let mut i = [0i64; 15];
        for x in i.iter_mut() {
            *x = c.int()?;
        }
        c.done()?;
        Ok(BidState {
            maker,
            token_cov_id,
            token_tpl_hash,
            pre,
            suf,
            ext,
            scale: i[0],
            min_fill: i[1],
            price: i[2],
            tip: i[3],
            tif: i[4],
            active_from: i[5],
            expiry_daa: i[6],
            refund_tip: i[7],
            reserve: i[8],
            delivery_carrier: i[9],
            interval: i[10],
            max_fill: i[11],
            slope: i[12],
            price_end: i[13],
            decay_step: i[14],
        })
    }

    pub fn redeem(&self) -> Vec<u8> {
        bid_template().redeem(&self.state_bytes())
    }

    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    pub fn decode_redeem(redeem: &[u8]) -> Result<BidState> {
        BidState::decode_state(bid_template().state_of(redeem)?)
    }

    /// `used(n)` of the contract: the budget a fill of n consumes, `ceil(n * (price + tip) / scale)` (no rise).
    pub fn used(&self, n: i64) -> i64 {
        q(n, self.price + self.tip, self.scale, self.scale - 1)
    }

    /// `allIn` of the contract: what the maker pays at most, `floor(n * (price + tip) / scale)`.
    pub fn all_in(&self, n: i64) -> i64 {
        q(n, self.price + self.tip, self.scale, 0)
    }

    /// Whether the bid continues after a fill whose consumed budget leaves `left` of its escrow (GTC only).
    pub fn can_continue(&self, left: i64) -> bool {
        left - self.delivery_carrier - self.reserve >= self.used(self.min_fill)
    }

    /// The largest n whose budget leaves the delivery carrier and the reserve of an escrow of `value`; 0 if none.
    pub fn buying_power(&self, value: u64) -> i64 {
        let rate = self.price + self.tip;
        let budget = value as i128 - self.delivery_carrier as i128 - self.reserve as i128;
        if rate <= 0 || budget <= 0 || self.scale <= 0 {
            return 0;
        }
        let n = (budget * self.scale as i128 / rate as i128).min(i64::MAX as i128) as i64;
        let mut n = n;
        // Guard the integer division against a rounding off-by-one: used(n) must fit the budget.
        while n > 0 && self.used(n) as i128 > budget {
            n -= 1;
        }
        n
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KobBid", "maker": to_hex(&self.maker), "tokenCovId": to_hex(&self.token_cov_id.as_bytes()),
            "tokenTplHash": to_hex(&self.token_tpl_hash.as_bytes()), "extensionCommitment": to_hex(&self.ext.as_bytes()), "scale": self.scale,
            "minFill": self.min_fill, "price": self.price, "tip": self.tip, "tif": self.tif, "activeFrom": self.active_from,
            "expiryDaa": self.expiry_daa, "reserve": self.reserve, "deliveryCarrier": self.delivery_carrier, "interval": self.interval,
            "maxFill": self.max_fill, "slope": self.slope,
        })
    }
}

// ---------------------------------------------------------------- decoding any KOB object

/// A KOB order of the pinned templates.
#[derive(Clone, Debug, PartialEq)]
pub enum KobOrder {
    Ask(AskState),
    Bid(BidState),
}

/// Why a script is not a `KobAsk` or `KobBid` of the templates `KpfPair` pins, in plain words: its size and the
/// template hash it would have if it were cut like each pinned template, against the pinned hash.
pub fn explain_foreign_script(redeem: &[u8]) -> String {
    let c = kob_consts();
    let (at, bt) = (ask_template(), bid_template());
    let hex = |h: &[u8]| to_hex(&h[..8]);
    let cut = |t: &crate::artifacts::Template| -> String {
        let (pre, suf) = (t.prefix.len(), t.suffix.len());
        if redeem.len() < pre + suf {
            return format!("shorter than the {} template ({} bytes)", t.name, pre + suf);
        }
        let h = template_hash_of(&redeem[..pre], &redeem[redeem.len() - suf..]);
        format!("cut like {} (prefix {pre}, suffix {suf}) its template hash is {}... against the pinned {}...", t.name, hex(&h), hex(&if t.name == "KobAsk" { c.ask_tpl } else { c.bid_tpl }))
    };
    format!(
        "the script ({} bytes) is not an instance of the KobAsk ({} bytes) or KobBid ({} bytes) template pinned in KpfPair: {}; {}. \
         KpfPair accepts evidence only from orders whose template hash is the pinned one",
        redeem.len(),
        at.prefix.len() + at.state_len + at.suffix.len(),
        bt.prefix.len() + bt.state_len + bt.suffix.len(),
        cut(at),
        cut(bt)
    )
}

/// Decodes the redeem script of a resting order: a `KobAsk` or a `KobBid` of the pinned templates.
pub fn decode_order(redeem: &[u8]) -> Result<KobOrder> {
    if ask_template().state_of(redeem).is_ok() {
        return AskState::decode_redeem(redeem).map(KobOrder::Ask);
    }
    if bid_template().state_of(redeem).is_ok() {
        return BidState::decode_redeem(redeem).map(KobOrder::Bid);
    }
    Err(Error::Decode(explain_foreign_script(redeem)))
}

/// A KOB object a transaction creates.
#[derive(Clone, Debug, PartialEq)]
pub enum KobObj {
    Ask(AskState),
    Bid(BidState),
    Token(TokenState),
}

impl KobObj {
    pub fn redeem(&self) -> Vec<u8> {
        match self {
            KobObj::Ask(a) => a.redeem(),
            KobObj::Bid(b) => b.redeem(),
            KobObj::Token(t) => t.redeem(),
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            KobObj::Ask(_) => "ask",
            KobObj::Bid(_) => "bid",
            KobObj::Token(_) => "token",
        }
    }
    /// Decodes a redeem script as any of the three.
    pub fn decode(redeem: &[u8]) -> Result<KobObj> {
        match decode_order(redeem) {
            Ok(KobOrder::Ask(a)) => Ok(KobObj::Ask(a)),
            Ok(KobOrder::Bid(b)) => Ok(KobObj::Bid(b)),
            Err(oe) => TokenState::decode_redeem(redeem).map(KobObj::Token).map_err(|te| Error::Decode(format!("{oe}; and {te}"))),
        }
    }
}

// ---------------------------------------------------------------- live UTXOs

macro_rules! live {
    ($name:ident, $state:ty, $f:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug)]
        pub struct $name {
            pub $f: $state,
            pub value: u64,
            /// The covenant id of the UTXO (an order's own id, or the token's id for a holder).
            pub id: Hash,
            pub op: TransactionOutpoint,
            pub daa: u64,
        }
        impl $name {
            pub fn entry(&self) -> UtxoEntry {
                UtxoEntry::new(self.value, self.$f.spk(), self.daa, false, Some(self.id))
            }
        }
    };
}

live!(LiveAsk, AskState, a, "A live `KobAsk` UTXO.");
live!(LiveBid, BidState, b, "A live `KobBid` UTXO.");
live!(LiveToken, TokenState, t, "A live KCC-20 holder UTXO (a custody, or tokens of a key).");

/// The evidence order, with the token custody an ask's fill spends.
#[derive(Clone, Debug)]
pub enum Evidence {
    Ask { order: LiveAsk, custody: LiveToken },
    Bid { order: LiveBid },
}

impl Evidence {
    pub fn is_ask(&self) -> bool {
        matches!(self, Evidence::Ask { .. })
    }
    pub fn name(&self) -> &'static str {
        if self.is_ask() {
            "ask"
        } else {
            "bid"
        }
    }
    /// The quote: the order's price, sompi per whole token.
    pub fn quote(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.price,
            Evidence::Bid { order } => order.b.price,
        }
    }
    pub fn op(&self) -> TransactionOutpoint {
        match self {
            Evidence::Ask { order, .. } => order.op,
            Evidence::Bid { order } => order.op,
        }
    }
    pub fn id(&self) -> Hash {
        match self {
            Evidence::Ask { order, .. } => order.id,
            Evidence::Bid { order } => order.id,
        }
    }
    pub fn value(&self) -> u64 {
        match self {
            Evidence::Ask { order, .. } => order.value,
            Evidence::Bid { order } => order.value,
        }
    }
    pub fn daa(&self) -> u64 {
        match self {
            Evidence::Ask { order, .. } => order.daa,
            Evidence::Bid { order } => order.daa,
        }
    }
    pub fn tif(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.tif,
            Evidence::Bid { order } => order.b.tif,
        }
    }
    pub fn active_from(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.active_from,
            Evidence::Bid { order } => order.b.active_from,
        }
    }
    pub fn expiry_daa(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.expiry_daa,
            Evidence::Bid { order } => order.b.expiry_daa,
        }
    }
    pub fn interval(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.interval,
            Evidence::Bid { order } => order.b.interval,
        }
    }
    pub fn min_fill(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.min_fill,
            Evidence::Bid { order } => order.b.min_fill,
        }
    }
    pub fn max_fill(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.max_fill,
            Evidence::Bid { order } => order.b.max_fill,
        }
    }
    /// Base units the order can fill now: an ask's `amountLeft`, a bid's buying power.
    pub fn capacity(&self) -> i64 {
        match self {
            Evidence::Ask { order, .. } => order.a.amount_left,
            Evidence::Bid { order } => order.b.buying_power(order.value),
        }
    }
    /// The DAA since which the order has rested at its quote as the contract measures it:
    /// `max(UTXO DAA + interval, activeFrom, an ask's custody DAA)`.
    pub fn exposed(&self) -> u64 {
        let mut e = self.daa() as i64 + self.interval();
        e = e.max(self.active_from());
        if let Evidence::Ask { custody, .. } = self {
            e = e.max(custody.daa as i64);
        }
        e.max(0) as u64
    }
    /// The first DAA at which the order has rested for the market's `minRestDaa`; the transaction's lock time must be
    /// at least this.
    pub fn rest_end(&self, m: &Market) -> u64 {
        self.exposed() + m.min_rest_daa.max(0) as u64
    }
    /// `exposed' = max(exposed, S)` for a pair whose evidence clock starts at `clock` (DESIGN 4.5c): only the rest on the
    /// pair's clock counts.
    pub fn exposed_on(&self, clock: Option<i64>) -> u64 {
        self.exposed().max(clock.unwrap_or(0).max(0) as u64)
    }
    /// The first DAA at which the order has rested `minRestDaa` on the pair's clock: the lock time a transaction using it
    /// for that pair needs.
    pub fn rest_end_on(&self, m: &Market, clock: Option<i64>) -> u64 {
        self.exposed_on(clock) + m.min_rest_daa.max(0) as u64
    }
    /// The last DAA at which conforming matchers still fill the order (KOB matcher.md 2.3): the earliest of its soft
    /// expiry, its IOC/FOK kill (`max(UTXO DAA, activeFrom) + 600`) and 90 days idle (`UTXO DAA + 77,760,000`). After it
    /// the order is hidden from every book: it is dead (review 3 F1).
    pub fn live_until(&self) -> i64 {
        let mut t = self.expiry_daa().min(self.daa() as i64 + MAX_IDLE as i64);
        if self.tif() != 0 {
            t = t.min((self.daa() as i64).max(self.active_from()) + IOC_LIFE);
        }
        t
    }
}

/// The least fill that covers `k * sum` (the evidence block rule of DESIGN 4.4, exact as in the contract).
pub fn needed_fill(m: &Market, sum: i64) -> i64 {
    (sum / BPS) * m.k_bps + ((sum % BPS) * m.k_bps + BPS - 1) / BPS
}

// ---------------------------------------------------------------- the check

/// What [`check_evidence`] found, in plain words.
#[derive(Clone, Debug)]
pub struct EvidenceReport {
    /// "ask" or "bid".
    pub kind: &'static str,
    pub quote: i64,
    /// Base units the order can fill now.
    pub capacity: i64,
    /// Most base units of pairs (sum of the `n` of the block) one fill of the whole capacity can cover: `capacity / k`.
    pub covers: i64,
    /// `exposed'`: the DAA since which the order counts as resting, on the pair's clock when one is given.
    pub exposed: u64,
    /// The first DAA a transaction may prove for this evidence (on the pair's clock when one is given).
    pub rest_end: u64,
    /// The pair's evidence clock start S the report was made for (DESIGN 4.5c), if any.
    pub clock: Option<i64>,
    /// The last DAA conforming matchers fill the order ([`Evidence::live_until`]).
    pub live_until: i64,
    /// Why the order is dead (soft-expired: hidden from every book) at the DAA checked. The builders refuse dead
    /// evidence unless told otherwise (`allow_dead`, for sweeping tests only).
    pub dead: Vec<String>,
    /// Reasons the order can never serve this market as it is.
    pub problems: Vec<String>,
    /// Reasons it cannot serve at the DAA asked about (it may later).
    pub waiting: Vec<String>,
    /// Things worth knowing that do not stop it.
    pub notes: Vec<String>,
}

impl EvidenceReport {
    pub fn usable(&self) -> bool {
        self.problems.is_empty() && self.waiting.is_empty() && self.dead.is_empty()
    }
    /// Every reason in one list, problems first.
    pub fn reasons(&self) -> Vec<String> {
        self.problems.iter().chain(self.waiting.iter()).chain(self.dead.iter()).cloned().collect()
    }
}

/// Checks an order against a market the way `KpfPair.evQuote` does (DESIGN 3.3, 4.5b), and says why it cannot serve.
/// `daa` is the DAA score at which the transaction would be proven; with `None` only the time-free conditions and the
/// first usable DAA are reported. No pair clock: see [`check_evidence_on`].
pub fn check_evidence(ev: &Evidence, m: &Market, daa: Option<u64>) -> EvidenceReport {
    check_evidence_on(ev, m, None, daa)
}

/// [`check_evidence`] for a pair whose evidence clock starts at `clock` (DESIGN 4.5c, `LivePair::clock`): the rest
/// counts only from `max(exposed, clock)`, the order must have stayed fillable until that rest ends, and the order is
/// reported dead (`dead`) when it is soft-expired at the DAA the transaction would land (the later of `daa` and the
/// end of the rest).
pub fn check_evidence_on(ev: &Evidence, m: &Market, clock: Option<i64>, daa: Option<u64>) -> EvidenceReport {
    let mut problems: Vec<String> = vec![];
    let mut waiting: Vec<String> = vec![];
    let mut notes: Vec<String> = vec![];
    let word = ev.name();
    let (token_cov, scale, ext, tpl, pre, suf) = match ev {
        Evidence::Ask { order, .. } => (order.a.token_cov_id, order.a.scale, order.a.ext, order.a.token_tpl_hash, order.a.pre, order.a.suf),
        Evidence::Bid { order } => (order.b.token_cov_id, order.b.scale, order.b.ext, order.b.token_tpl_hash, order.b.pre, order.b.suf),
    };
    if ev.id() == crate::state::ZERO {
        problems.push("the order UTXO carries no covenant id".into());
    }
    if token_cov != m.token_cov_id {
        problems.push(format!("the order is for another token: its token covenant id is {}..., the market's is {}...", to_hex(&token_cov.as_bytes()[..6]), to_hex(&m.token_cov_id.as_bytes()[..6])));
    }
    if scale != m.scale {
        problems.push(format!("the order prices a token of scale {scale}, the market's scale is {}", m.scale));
    }
    if ext != m.ext {
        problems.push("the order's extension commitment is not the market's (another sub-type of the token)".into());
    }
    if tpl != m.token_tpl_hash || pre != m.tpl_prefix_len || suf != m.tpl_suffix_len {
        problems.push("the order pins another token program template than the market".into());
    }
    let slope = match ev {
        Evidence::Ask { order, .. } => order.a.slope,
        Evidence::Bid { order } => order.b.slope,
    };
    if slope != 0 {
        problems.push("the order's price moves with time (decaying ask or rising bid): only a fixed quote is evidence".into());
    }
    if ev.interval() < 0 {
        problems.push(format!("the order's interval is negative ({})", ev.interval()));
    }
    if let Evidence::Ask { order, custody } = ev {
        let t = &custody.t;
        if custody.id != m.token_cov_id {
            problems.push("the custody UTXO is not a UTXO of the market's token (its covenant id differs)".into());
        }
        if t.scheme != SCHEME_COVID || t.owner != order.id.as_bytes() {
            problems.push("the custody is not owned by this ask's covenant id: the ask is not backed by these tokens".into());
        }
        if t.ext != m.ext {
            problems.push("the custody's extension commitment is not the market's".into());
        }
        if t.borrow != 0 {
            problems.push("the custody allows borrowing; KobAsk requires it disabled".into());
        }
        if t.amount != order.a.amount_left {
            problems.push(format!("the custody holds {} base units but the ask says {} are left: KobAsk refuses to fill", t.amount, order.a.amount_left));
        }
    }
    if ev.capacity() <= 0 {
        problems.push(match ev {
            Evidence::Ask { .. } => "the ask has nothing left to sell".to_string(),
            Evidence::Bid { .. } => "the bid's KAS cannot buy even one base unit (after its delivery carrier and reserve)".to_string(),
        });
    }
    let quote = ev.quote();
    if quote < 0 || quote > m.price_max {
        notes.push(format!("the quote {quote} is outside 0..=priceMax {}: it can close a pair (P5) but never liquidate one", m.price_max));
    }
    // Time (DESIGN 3.3, 4.5b, 4.5c; KpfPair.evQuote). On a pair's clock S the rest counts from exposed' = max(exposed, S).
    let raw_exposed = ev.exposed();
    let exposed = ev.exposed_on(clock);
    let rest_end = ev.rest_end_on(m, clock);
    let end = rest_end as i64;
    let on_clock = |what: &str| -> String {
        match clock {
            Some(s) if s as u64 > raw_exposed => format!("{what} (the order rested from DAA {raw_exposed}, but the pair's evidence clock starts at DAA {s}: only its rest from then counts)"),
            _ => what.to_string(),
        }
    };
    if let Some(s) = clock {
        if s as u64 > raw_exposed {
            notes.push(format!("the order rested before this pair's evidence clock started (DAA {s}): its rest counts from DAA {s} only (DESIGN 4.5c)"));
        }
    }
    if ev.expiry_daa() < end {
        problems.push(on_clock(&format!(
            "its soft expiry (DAA {}) falls before the end of the {} DAA rest window (DAA {rest_end}): matchers would already have hidden it",
            ev.expiry_daa(),
            m.min_rest_daa
        )));
    }
    if ev.tif() != 0 {
        let mut kill = ev.daa() as i64;
        kill = kill.max(ev.active_from());
        kill += IOC_LIFE;
        if kill < end {
            problems.push(on_clock(&format!("it is an IOC/FOK order and is killed (refundable by anyone) at DAA {kill}, before its rest window ends at DAA {rest_end}")));
        }
    }
    if rest_end > ev.daa() + MAX_IDLE {
        problems.push(on_clock(&format!("its rest window ends at DAA {rest_end}, after the 90 days (DAA {}) at which anyone may refund it", ev.daa() + MAX_IDLE)));
    }
    let live_until = ev.live_until();
    let mut dead = vec![];
    // The earliest DAA a transaction using it can land: the end of its rest (the lock time) or now, whichever is later.
    let at = daa.unwrap_or(0).max(rest_end) as i64;
    if at > live_until {
        let why = if at > ev.expiry_daa() {
            format!("its soft expiry DAA {} has passed", ev.expiry_daa())
        } else if ev.tif() != 0 && at > (ev.daa() as i64).max(ev.active_from()) + IOC_LIFE {
            format!("it is an IOC/FOK order killed at DAA {}", (ev.daa() as i64).max(ev.active_from()) + IOC_LIFE)
        } else {
            format!("it has been idle 90 days (since DAA {})", ev.daa())
        };
        dead.push(format!(
            "the order is dead at DAA {at}: {why}, so KOB's conforming matchers no longer fill it and no book shows it. A dead order is a past quote, not a live one: using it looks back to a price nobody can trade at now (DESIGN 4.5c). The tools refuse it; sweep it instead (anyone may fill it at its price, or refund it), or pass --allow-dead-evidence in a test"
        ));
    }
    if let Some(d) = daa {
        if d < rest_end {
            waiting.push(format!(
                "it has not rested long enough: it rested since DAA {exposed} and the market requires {} DAA, so it counts from DAA {rest_end} ({} DAA to go at DAA {d})",
                m.min_rest_daa,
                rest_end - d
            ));
        }
        if (ev.active_from().max(0) as u64) > d {
            waiting.push(format!("the order is not active before DAA {}", ev.active_from()));
        }
    }
    if let Evidence::Bid { order } = ev {
        if order.b.reserve < 0 || order.b.delivery_carrier < 0 || order.b.tip < 0 {
            problems.push("the bid has a negative reserve, delivery carrier or tip: KobBid refuses to fill".into());
        }
    }
    if let Evidence::Ask { order, .. } = ev {
        if order.a.price < order.a.tip || order.a.tip < 0 {
            problems.push("the ask's price is below its tip: KobAsk refuses to fill".into());
        }
    }
    let capacity = ev.capacity().max(0);
    let k = m.k_bps.max(1) as i128;
    let covers = (capacity as i128 * BPS as i128 / k).min(i64::MAX as i128) as i64;
    EvidenceReport { kind: word, quote, capacity, covers, exposed, rest_end, clock, live_until, dead, problems, waiting, notes }
}

/// What an evidence order lets each side of a pair do, with the reason when it cannot.
#[derive(Clone, Debug)]
pub struct Verdict {
    pub path: &'static str,
    pub ok: bool,
    pub why: String,
}

/// For a pair: whether the evidence's quote lets the long or the short close (P5) or be liquidated (P6/P7).
pub fn pair_verdicts(r: &EvidenceReport, p: &Pair) -> Vec<Verdict> {
    let is_ask = r.kind == "ask";
    let kas = |v: i64| crate::error::kas_str(v as i128);
    let mut out = vec![];
    // P5
    out.push(match (is_ask, r.quote) {
        (false, b) => Verdict {
            path: "the long closes (P5)",
            ok: true,
            why: format!("settles at the bid {} KAS, clamped to the range: price {}", kas(b), kas(clamp_price(p, b))),
        },
        (true, _) => Verdict { path: "the long closes (P5)", ok: false, why: "a long closes at a resting bid; this order is an ask".into() },
    });
    out.push(match (is_ask, r.quote) {
        (true, a) => Verdict {
            path: "the short closes (P5)",
            ok: true,
            why: format!("settles at the ask {} KAS, clamped to the range: price {}", kas(a), kas(clamp_price(p, a))),
        },
        (false, _) => Verdict { path: "the short closes (P5)", ok: false, why: "a short closes at a resting ask; this order is a bid".into() },
    });
    // P6 / P7
    let in_range = r.quote >= 0 && r.quote <= p.m.price_max;
    out.push(if !is_ask {
        Verdict { path: "the long is liquidated (P6, P7)", ok: false, why: "a long is liquidated by an ask; this order is a bid".into() }
    } else if !in_range {
        Verdict { path: "the long is liquidated (P6, P7)", ok: false, why: format!("the quote {} is outside 0..=priceMax", r.quote) }
    } else if long_liquidatable(p, r.quote) {
        Verdict { path: "the long is liquidated (P6, P7)", ok: true, why: format!("the ask {} KAS is below the long's trigger {} KAS", kas(r.quote), kas(trigger_of(p, LONG))) }
    } else {
        Verdict {
            path: "the long is liquidated (P6, P7)",
            ok: false,
            why: format!("the ask {} KAS is not below the long's trigger {} KAS (bl {} KAS, maintenance {} bps)", kas(r.quote), kas(trigger_of(p, LONG)), kas(p.bl), p.m.mmr_bps),
        }
    });
    out.push(if is_ask {
        Verdict { path: "the short is liquidated (P6, P7)", ok: false, why: "a short is liquidated by a bid; this order is an ask".into() }
    } else if !in_range {
        Verdict { path: "the short is liquidated (P6, P7)", ok: false, why: format!("the quote {} is outside 0..=priceMax", r.quote) }
    } else if short_liquidatable(p, r.quote) {
        Verdict { path: "the short is liquidated (P6, P7)", ok: true, why: format!("the bid {} KAS is above the short's trigger {} KAS", kas(r.quote), kas(trigger_of(p, SHORT))) }
    } else {
        Verdict {
            path: "the short is liquidated (P6, P7)",
            ok: false,
            why: format!("the bid {} KAS is not above the short's trigger {} KAS (bs {} KAS, maintenance {} bps)", kas(r.quote), kas(trigger_of(p, SHORT)), kas(p.bs), p.m.mmr_bps),
        }
    });
    out
}

/// Whether the market's token program is the one this tool can spend (KOB's reference KCC-20, 3 / 3 slots).
pub fn supports_token_program(m: &Market) -> std::result::Result<(), String> {
    let t = token_template();
    if m.token_tpl_hash.as_bytes() != t.hash || m.tpl_prefix_len != t.prefix.len() as i64 || m.tpl_suffix_len != t.suffix.len() as i64 {
        return Err(format!(
            "the market's token program (template {}...) is not KOB's reference KCC-20 (template {}...); the evidence builders spend only that program",
            to_hex(&m.token_tpl_hash.as_bytes()[..6]),
            to_hex(&t.hash[..6])
        ));
    }
    Ok(())
}
