//! The underwriting slot of oracle-only markets (`docs/DESIGN-ORACLE-2.md` 1.3, `docs/DESIGN-ORACLE-IMPL.md`) as a pure
//! model: the bond parameters and their bounds, the ring by which a slot counts the cover it binds and releases it only
//! when the cover's liability has ended, and every entry of `contracts/KpfBond.sil` with exactly the contract's
//! acceptance rule (every `require`, and a failed script on any 64-bit overflow). The stand-in pair and opening order
//! of `contracts/stubs/` are modelled too, so that a transaction's acceptance can be predicted as a whole.
//!
//! The engine tests hold model and contract together step by step (`bond_attack_tests`: every hostile step generated
//! from the model is built as a transaction and run in the script engine; both must accept or both refuse, and an
//! accepted step must write the state the model predicts). The model alone carries the property tests Z3 and Z8
//! (`tests/bond_model.rs`), and adds the entries the template does not have yet (merge, reserve, release, pay).

use silverscript_abi::ArtifactValue;

use crate::state::{encode, sm8, to_args, F, BPS, CSV_LIMIT, LOCK_TIME_THRESHOLD};

/// Buckets in a ring (`KpfBond.RING`).
// DESIGN: the note's ring has 8 buckets of Lambda / 8. Advanced to the present, its last bucket's expiry lies at
// most 7 steps ahead, while a renewal in the last step before X needs X + Lambda (8 steps ahead of X, 9 of the
// renewal) and must keep the old bucket until X + T_tail: a ring of 8 cannot hold any renewal before expiry
// (`tests/bond_model.rs`, `z3_the_notes_eight_bucket_ring_cannot_hold_a_renewal`). The bound that makes every renewal
// fit is `Lambda + T_tail + 2 step <= RING x step` (`BondParams::check`); 12 buckets admit the note's own example
// (Lambda 28 days = 8 steps of 3.5 days, T_tail 3.25 days).
pub const RING: usize = 12;
/// Claims per bind (`KpfBond.bind`: `r0, c0 .. r6, c6`).
pub const CLAIMS: usize = 7;
/// Status of an active slot and of an unbonding one (`KpfBond.STATUS_SLOT`, `STATUS_UNBONDING`; 1 is `KpfLock`'s).
pub const STATUS_SLOT: i64 = 2;
pub const STATUS_UNBONDING: i64 = 3;
/// `rho <= 5` (`KpfBond.RHO_MAX_BPS`).
pub const RHO_MAX_BPS: i64 = 50_000;
/// The capacity is computed on `min(K, VALUE_CLAMP)` (`KpfBond.VALUE_CLAMP`).
// DESIGN: amounts are sompi, not the note's KAS units (floor(sompi / 10^8), cover rounded up): with rho <= 5 and K
// clamped at 10^18 sompi, rho x K <= 5 x 10^18 < 2^63, so cover is exact and "over by one sompi" is refused exactly.
pub const VALUE_CLAMP: i64 = 1_000_000_000_000_000_000;
/// Claim references: an input index below `OUT_REF`, `OUT_REF + j` for output `j`, all below `REF_LIMIT`.
pub const OUT_REF: i64 = 65_536;
pub const REF_LIMIT: i64 = 131_072;
/// The largest output count the opening order's twin scan covers (`KpfOpenStub.MAX_OUTPUTS`).
pub const MAX_OUTPUTS: usize = 8;
/// Length of the slot state (from redeem byte 1).
pub const BOND_STATE_LEN: usize = 236;

/// Why the model refuses a step (a contract `require` or an overflow); for messages only.
pub type Refusal = &'static str;

fn chk(ok: bool, why: Refusal) -> Result<(), Refusal> {
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

fn add(a: i64, b: i64) -> Result<i64, Refusal> {
    a.checked_add(b).ok_or("overflow")
}
fn sub(a: i64, b: i64) -> Result<i64, Refusal> {
    a.checked_sub(b).ok_or("overflow")
}
fn mul(a: i64, b: i64) -> Result<i64, Refusal> {
    a.checked_mul(b).ok_or("overflow")
}

/// The bond parameters of a market (`KpfBond` state `rhoBps .. unbondDaa`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BondParams {
    /// `rho` in basis points: counted cover `<= rho x K`.
    pub rho_bps: i64,
    /// One ring step, DAA.
    pub step: i64,
    /// The renewal period `Lambda`, a whole number of steps.
    pub term: i64,
    /// `T_tail = W + T_retro`: how long cover stays liable after its expiry.
    pub tail: i64,
    /// `a`: the least age of a slot before an owner entry (CSV).
    pub rate: i64,
    /// `U`: the least age of an unbonding slot before withdraw (CSV).
    pub unbond: i64,
}

impl BondParams {
    /// `KpfBond.checkBond` (the same text in the stand-in pair and order).
    pub fn check(&self) -> Result<(), Refusal> {
        let p = self;
        chk(p.rho_bps >= BPS, "rho below 1")?;
        chk(p.rho_bps <= RHO_MAX_BPS, "rho above 5")?;
        chk(p.step > 0, "step")?;
        chk(p.term >= p.step, "term below one step")?;
        chk(p.term % p.step == 0, "term not a whole number of steps")?;
        chk(p.term < CSV_LIMIT, "term")?;
        chk(p.tail >= 0, "tail")?;
        chk(p.tail < CSV_LIMIT, "tail")?;
        // term, tail < 2^32 and step <= term: no overflow below.
        chk(p.term + p.tail + 2 * p.step <= RING as i64 * p.step, "the ring cannot hold a renewal")?;
        chk(p.rate >= 0 && p.rate < CSV_LIMIT, "rate")?;
        chk(p.unbond >= 0 && p.unbond < CSV_LIMIT, "unbond")
    }

    /// The latest expiry a bind with lock-time bound `t` may choose: `t + term + step`.
    // DESIGN: the note's bind rule reads "X a ring boundary within the next Lambda; X >= lockTime + 7 Lambda / 8". With
    // X <= t + Lambda a renewal X -> X + Lambda is possible only from t >= X, i.e. after the cover has expired (when
    // claims are void), and the lower bound confines it to [X, X + Lambda / 8]; yet the note's capacity formula assumes
    // "renewal in the last step" before X. Here the window is `x <= t + Lambda + step` (renewal from X - step on) and
    // the slot sets no lower bound: the term a pair gets is the pair's and its orders' business.
    pub fn horizon(&self) -> i64 {
        self.term + self.step
    }

    /// The testnet defaults of this slice at 10 DAA per second: rho 1, a 28-day term of 8 steps of 3.5 days, a tail
    /// of `W` 6 h + `T_retro` 3 days, `a` = 60 s, `U` = 1 hour.
    pub fn testnet() -> BondParams {
        BondParams { rho_bps: 10_000, step: 3_024_000, term: 24_192_000, tail: 2_808_000, rate: 600, unbond: 36_000 }
    }

    pub(crate) fn fields(&self) -> Vec<F> {
        vec![F::I(self.rho_bps), F::I(self.step), F::I(self.term), F::I(self.tail), F::I(self.rate), F::I(self.unbond)]
    }
}

/// The market block a slot shares byte for byte with its pairs and orders: the market id and the bond parameters
/// (87 bytes in push form).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BondMarket {
    pub id: [u8; 32],
    pub p: BondParams,
}

impl BondMarket {
    pub(crate) fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.id)];
        f.extend(self.p.fields());
        f
    }
    /// The 87 bytes compared by `boundBy`.
    pub fn bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
}

/// `floor(min(v, VALUE_CLAMP) * rho_bps / 10^4)` (`KpfBond.capacity`).
pub fn capacity(v: i64, rho_bps: i64) -> i64 {
    let k = v.min(VALUE_CLAMP);
    (k / BPS) * rho_bps + ((k % BPS) * rho_bps) / BPS
}

/// The sum of a ring, every bucket `>= 0` (`KpfBond.ringSum`).
pub fn ring_sum(r: &[i64; RING]) -> Result<i64, Refusal> {
    let mut s = 0i64;
    for b in r {
        chk(*b >= 0, "negative bucket")?;
        s = add(s, *b)?;
    }
    Ok(s)
}

/// One claim of a bind: a claimant reference and its cover; `(-1, 0)` is unused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    pub r: i64,
    pub c: i64,
}

impl Claim {
    pub const NONE: Claim = Claim { r: -1, c: 0 };
    /// The claim of a renewing pair at input `i`.
    pub fn input(i: usize, c: i64) -> Claim {
        Claim { r: i as i64, c }
    }
    /// The claim of a pair created at output `j`.
    pub fn output(j: usize, c: i64) -> Claim {
        Claim { r: OUT_REF + j as i64, c }
    }
}

/// The arguments of `KpfBond.bind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindArgs {
    /// A lower bound on the present (`tx.daa >= t`).
    pub t: i64,
    /// The expiry the cover is counted at.
    pub x: i64,
    /// Ring steps to advance first.
    pub d: i64,
    pub claims: [Claim; CLAIMS],
}

impl BindArgs {
    pub fn new(t: i64, x: i64, d: i64, used: &[Claim]) -> BindArgs {
        let mut claims = [Claim::NONE; CLAIMS];
        claims[..used.len()].copy_from_slice(used);
        BindArgs { t, x, d, claims }
    }

    /// The 17 eight-byte arguments in entry order, as the ABI encodes `byte[8]` (script numbers padded to 8 bytes);
    /// the signature follows.
    pub fn arg_bytes(&self) -> Vec<[u8; 8]> {
        let mut v = vec![sm8(self.t), sm8(self.x), sm8(self.d)];
        for c in &self.claims {
            v.push(sm8(c.r));
            v.push(sm8(c.c));
        }
        v
    }
}

/// What a transaction lets an input of `KpfBond` see besides its own state: the lock time (`tx.daa`), this input's
/// committed sequence (`this.ageDaa`), the value of this input, the counts of inputs and outputs of its covenant id,
/// whether the owner's SIGHASH_ALL signature is valid, and whether the UTXO has a covenant id at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ctx {
    pub lock: i64,
    pub seq: i64,
    pub v_in: i64,
    pub cov_in: usize,
    pub cov_out: usize,
    pub signed: bool,
    pub has_id: bool,
}

/// A slot (`KpfBond` state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bond {
    pub owner: [u8; 32],
    pub status: i64,
    pub m: BondMarket,
    pub j0: i64,
    pub ring: [i64; RING],
}

impl Bond {
    /// A fresh active slot: empty ring at position `j0`.
    pub fn new(owner: [u8; 32], m: BondMarket, j0: i64) -> Bond {
        Bond { owner, status: STATUS_SLOT, m, j0, ring: [0; RING] }
    }

    fn head(&self) -> Vec<F> {
        let mut f = vec![F::B(self.owner), F::I(self.status)];
        f.extend(self.m.fields());
        f.push(F::I(self.j0));
        f
    }

    /// The ring field: 12 buckets of 8 bytes (script numbers), one 96-byte push.
    pub fn ring_bytes(ring: &[i64; RING]) -> Vec<u8> {
        ring.iter().flat_map(|b| sm8(*b)).collect()
    }

    /// The encoded state (236 bytes, from redeem byte 1): the header, market block and ring position as canonical
    /// pushes, then the ring as `OP_PUSHDATA1 96` and its 96 bytes.
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut v = encode(&self.head());
        v.extend_from_slice(&[0x4c, 0x60]);
        v.extend(Self::ring_bytes(&self.ring));
        v
    }

    /// The constructor arguments of `KpfBond` for this state.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = to_args(&self.head());
        a.push(ArtifactValue::Bytes(Self::ring_bytes(&self.ring)));
        a
    }

    /// The expiry of bucket `i`: `(j0 + i + 1) * step`, or `None` on overflow.
    pub fn expiry_of(&self, i: usize) -> Option<i64> {
        self.j0.checked_add(i as i64 + 1)?.checked_mul(self.m.p.step)
    }

    /// The bucket counting expiry `x`, if the ring holds it.
    pub fn bucket_of(&self, x: i64) -> Option<usize> {
        let s = self.m.p.step;
        if s <= 0 || x % s != 0 {
            return None;
        }
        let k = (x / s).checked_sub(self.j0)?.checked_sub(1)?;
        if (0..RING as i64).contains(&k) {
            Some(k as usize)
        } else {
            None
        }
    }

    /// The cover counted for expiry `x` (0 when the ring does not hold it).
    pub fn counted_at(&self, x: i64) -> i64 {
        self.bucket_of(x).map(|k| self.ring[k]).unwrap_or(0)
    }

    /// `KpfBond.advanced`: the ring moved by `d >= 0` steps, after proving with the lock time that the liability of
    /// the dropped buckets has ended.
    pub fn advanced(&self, d: i64, lock: i64) -> Result<[i64; RING], Refusal> {
        chk(d >= 0, "negative advance")?;
        if d > 0 {
            let e = add(mul(add(self.j0, d)?, self.m.p.step)?, self.m.p.tail)?;
            chk(e >= 0, "negative lock time")?;
            chk(e < LOCK_TIME_THRESHOLD, "liability end beyond the DAA lock-time range")?;
            chk(lock >= e, "liability not ended")?;
        }
        let mut out = [0i64; RING];
        if d < RING as i64 {
            let d = d as usize;
            out[..RING - d].copy_from_slice(&self.ring[d..]);
        }
        Ok(out)
    }

    fn single(ctx: &Ctx) -> Result<(), Refusal> {
        chk(ctx.cov_in == 1, "another input of the id")?;
        chk(ctx.cov_out == 1, "not exactly one output of the id")
    }

    fn owner_entry(&self, ctx: &Ctx) -> Result<(), Refusal> {
        chk(self.status == STATUS_SLOT, "not an active slot")?;
        chk(ctx.signed, "no owner signature")?;
        chk(ctx.seq >= self.m.p.rate, "rate limit")
    }

    /// `KpfBond.bind` with continuation value `v_out`: the slot it writes.
    pub fn bind(&self, ctx: &Ctx, a: &BindArgs, v_out: i64) -> Result<Bond, Refusal> {
        Self::single(ctx)?;
        self.m.p.check()?;
        self.owner_entry(ctx)?;
        let p = &self.m.p;
        chk(a.t >= 0 && a.t < LOCK_TIME_THRESHOLD, "t out of range")?;
        chk(ctx.lock >= a.t, "lock time below t")?;
        let mut r = self.advanced(a.d, ctx.lock)?;
        let j = add(self.j0, a.d)?;
        chk(a.x >= 0, "negative expiry")?;
        chk(a.x % p.step == 0, "expiry not a ring boundary")?;
        chk(a.x <= a.t + p.term + p.step, "expiry beyond the window")?;
        chk(a.x + p.tail < LOCK_TIME_THRESHOLD, "liability end beyond the DAA lock-time range")?;
        let k = sub(sub(a.x / p.step, j)?, 1)?;
        chk((0..RING as i64).contains(&k), "expiry outside the ring")?;
        chk(v_out >= ctx.v_in, "value taken from the slot")?;
        let mut last = -1i64;
        let mut live = true;
        let mut total = 0i64;
        for c in &a.claims {
            if c.r < 0 {
                chk(c.r == -1 && c.c == 0, "malformed unused claim")?;
                live = false;
            } else {
                chk(live, "a used claim after an unused one")?;
                chk(c.r > last, "claim refs not increasing")?;
                chk(c.r < REF_LIMIT, "claim ref out of range")?;
                chk(c.c >= 0 && c.c <= v_out, "claim cover out of range")?;
                last = c.r;
                total = add(total, c.c)?;
            }
        }
        // DESIGN: a genesis can write a negative bucket (the note's "honest-or-higher counters", 5.2, is false): adding
        // the claims to it would count less than they say. Found by Z3; `KpfBond.bind` refuses it too.
        chk(r[k as usize] >= 0, "negative bucket")?;
        r[k as usize] = add(r[k as usize], total)?;
        chk(ring_sum(&r)? <= capacity(v_out, p.rho_bps), "over the capacity")?;
        Ok(Bond { j0: j, ring: r, ..self.clone() })
    }

    /// `KpfBond.advance` (anyone).
    pub fn advance(&self, ctx: &Ctx, d: i64, v_out: i64) -> Result<Bond, Refusal> {
        Self::single(ctx)?;
        self.m.p.check()?;
        chk(self.status == STATUS_SLOT, "not an active slot")?;
        chk(d > 0, "advance of 0")?;
        let r = self.advanced(d, ctx.lock)?;
        chk(v_out >= ctx.v_in, "value taken from the slot")?;
        Ok(Bond { j0: add(self.j0, d)?, ring: r, ..self.clone() })
    }

    /// `KpfBond.split`: (child A, child B) for ring `a` and values `va`, `vb` (`ctx.cov_out` must be 2).
    pub fn split(&self, ctx: &Ctx, a: &[i64; RING], va: i64, vb: i64) -> Result<(Bond, Bond), Refusal> {
        chk(ctx.cov_in == 1, "another input of the id")?;
        chk(ctx.cov_out == 2, "not exactly two outputs of the id")?;
        self.m.p.check()?;
        self.owner_entry(ctx)?;
        let mut b = [0i64; RING];
        for i in 0..RING {
            b[i] = sub(self.ring[i], a[i])?;
        }
        chk(add(va, vb)? >= ctx.v_in, "value taken from the slot")?;
        chk(ring_sum(a)? <= capacity(va, self.m.p.rho_bps), "child A over its capacity")?;
        chk(ring_sum(&b)? <= capacity(vb, self.m.p.rho_bps), "child B over its capacity")?;
        Ok((Bond { ring: *a, ..self.clone() }, Bond { ring: b, ..self.clone() }))
    }

    /// `KpfBond.unbond`.
    pub fn unbond(&self, ctx: &Ctx, d: i64, v_out: i64) -> Result<Bond, Refusal> {
        Self::single(ctx)?;
        self.m.p.check()?;
        self.owner_entry(ctx)?;
        let r = self.advanced(d, ctx.lock)?;
        chk(ring_sum(&r)? == 0, "cover still counted")?;
        chk(v_out >= ctx.v_in, "value taken from the slot")?;
        Ok(Bond { status: STATUS_UNBONDING, j0: add(self.j0, d)?, ring: r, ..self.clone() })
    }

    /// `KpfBond.withdraw` (the value leaves the template).
    pub fn withdraw(&self, ctx: &Ctx) -> Result<(), Refusal> {
        chk(ctx.signed, "no owner signature")?;
        if ctx.has_id {
            self.m.p.check()?;
            chk(self.status == STATUS_UNBONDING, "not unbonding")?;
            chk(ctx.seq >= self.m.p.unbond, "unbonding delay")?;
            chk(ctx.cov_out == 0, "an output of the id remains")?;
        }
        Ok(())
    }

    // ---- entries of the note that the template does not have yet (model only; docs/DESIGN-ORACLE-IMPL.md)

    /// Merge of two children of one id (owner): values and buckets add. Both must be active and at the same ring
    /// position (an owner advances the older one first) and the result within the invariant.
    pub fn merge(&self, va: i64, other: &Bond, vb: i64) -> Result<(Bond, i64), Refusal> {
        self.m.p.check()?;
        chk(self.status == STATUS_SLOT && other.status == STATUS_SLOT, "not active slots")?;
        chk(self.owner == other.owner && self.m == other.m, "not children of one slot")?;
        chk(self.j0 == other.j0, "ring positions differ")?;
        let mut r = [0i64; RING];
        for i in 0..RING {
            chk(self.ring[i] >= 0 && other.ring[i] >= 0, "negative bucket")?;
            r[i] = add(self.ring[i], other.ring[i])?;
        }
        let v = add(va, vb)?;
        chk(ring_sum(&r)? <= capacity(v, self.m.p.rho_bps), "over the capacity")?;
        Ok((Bond { ring: r, ..self.clone() }, v))
    }

    /// Reserve (the opening of a dispute, next slices): `s` sompi leave this child into a reserved UTXO; the child keeps
    /// every bucket. Not an owner entry, so it is not refused for the invariant: a slashed slot counts more than
    /// `rho x` its remaining value until the cover expires, and cannot bind meanwhile (`bind` checks the invariant).
    // DESIGN: the note states the invariant `sum O <= rho K` "on every bind and owner split" and Z3 asks it "on every
    // reachable state". After a reservation it holds only as `cover relied on <= rho x (K + reserved)` (Z3 checks
    // that form); the reachable-state form holds for every lineage that was never reserved from.
    pub fn reserve(&self, v: i64, s: i64) -> Result<i64, Refusal> {
        chk(self.status == STATUS_SLOT, "not an active slot")?;
        chk(s > 0 && s <= v, "reservation out of range")?;
        Ok(v - s)
    }
}

/// The stand-in pair (`contracts/stubs/KpfPairStub.sil`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StubPair {
    pub m: BondMarket,
    pub uw_id: [u8; 32],
    pub uw_key: [u8; 32],
    pub owner: [u8; 32],
    pub cover: i64,
    pub expiry: i64,
}

impl StubPair {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([F::B(self.uw_id), F::B(self.uw_key), F::B(self.owner), F::I(self.cover), F::I(self.expiry)]);
        f
    }
    /// The encoded state (204 bytes).
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    /// The state fields as constructor arguments (after the build constants).
    pub fn ctor_state(&self) -> Vec<ArtifactValue> {
        to_args(&self.fields())
    }

    /// `KpfPairStub.rely` with this UTXO's DAA score `p`.
    pub fn rely(&self, p: i64, single: bool) -> Result<(), Refusal> {
        chk(single, "single input")?;
        self.m.p.check()?;
        chk(p >= 0, "unaccepted")?;
        chk(p <= self.expiry, "cover expired")?;
        chk(self.expiry <= add(add(p, self.m.p.term)?, self.m.p.step)?, "expiry beyond one period of the pair's own UTXO")
    }
}

/// The stand-in opening order (`contracts/stubs/KpfOpenStub.sil`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StubOpen {
    pub m: BondMarket,
    pub uw_accept: [u8; 32],
    pub owner: [u8; 32],
    pub cover: i64,
}

impl StubOpen {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([F::B(self.uw_accept), F::B(self.owner), F::I(self.cover)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_state(&self) -> Vec<ArtifactValue> {
        to_args(&self.fields())
    }
    /// The pair this order creates, bound by slot `uw_id` of owner `uw_key` at expiry `x`.
    pub fn pair_for(&self, uw_id: [u8; 32], uw_key: [u8; 32], x: i64) -> StubPair {
        StubPair { m: self.m, uw_id, uw_key, owner: self.owner, cover: self.cover, expiry: x }
    }
}

/// What `boundBy` reads of the slot input `u` (both stand-ins): the slot's state before the bind, its covenant id,
/// the bind's arguments, and whether the slot input is a canonical bind spend of the template (every argument a plain
/// 8-byte push, the signature 65 bytes, the bind's dispatch tag).
#[derive(Clone, Debug)]
pub struct SlotView<'a> {
    pub bond: &'a Bond,
    pub id: [u8; 32],
    pub args: &'a BindArgs,
    pub canonical_bind: bool,
}

/// `boundBy(u, k, x, ref, c)`: (the slot's id, its owner key).
pub fn bound_by(view: &SlotView, me: &BondMarket, k: i64, x: i64, r: i64, c: i64) -> Result<([u8; 32], [u8; 32]), Refusal> {
    chk(view.bond.status == STATUS_SLOT, "slot not active")?;
    chk(view.bond.m == *me, "slot of another market or parameters")?;
    chk(view.canonical_bind, "slot not spent by its bind entry")?;
    chk(view.args.x == x, "slot bound another expiry")?;
    chk((0..CLAIMS as i64).contains(&k), "claim index")?;
    let cl = view.args.claims[k as usize];
    chk(cl.r == r, "claim names another claimant")?;
    chk(cl.c >= c, "claim covers less")?;
    Ok((view.id, view.bond.owner))
}

impl StubPair {
    /// `KpfPairStub.renew` at input `me` with the slot view of input `u` and claim `k`: the continuation it writes.
    pub fn renew(&self, single: bool, me: usize, view: Option<&SlotView>, k: i64) -> Result<StubPair, Refusal> {
        chk(single, "single input")?;
        self.m.p.check()?;
        chk(self.cover > 0, "no cover")?;
        let x2 = add(self.expiry, self.m.p.term)?;
        let view = view.ok_or("no slot input")?;
        let (id, key) = bound_by(view, &self.m, k, x2, me as i64, self.cover)?;
        chk(id == self.uw_id, "another slot")?;
        chk(key == self.uw_key, "another underwriter")?;
        Ok(StubPair { expiry: x2, ..self.clone() })
    }
}

impl StubOpen {
    /// `KpfOpenStub.fill` creating the pair at output `pair_out` (with `twins` other outputs of its id and `outputs`
    /// outputs in all), bound by the slot view at claim `k` with expiry `x`: the pair it accepts.
    pub fn fill(&self, single: bool, pair_out: usize, view: Option<&SlotView>, k: i64, x: i64, outputs: usize, twins: usize) -> Result<StubPair, Refusal> {
        chk(single, "single input")?;
        self.m.p.check()?;
        chk(self.cover > 0, "no cover")?;
        let view = view.ok_or("no slot input")?;
        let (uw, key) = bound_by(view, &self.m, k, x, OUT_REF + pair_out as i64, self.cover)?;
        chk(uw != [0; 32], "id-less slot")?;
        chk(self.uw_accept == [0; 32] || self.uw_accept == uw, "slot not accepted")?;
        chk(outputs <= MAX_OUTPUTS, "too many outputs to scan")?;
        chk(twins == 0, "a twin of the pair")?;
        Ok(self.pair_for(uw, key, x))
    }
}
