//! The stake judge of oracle-only markets (`docs/DESIGN-ORACLE.md` 2, `docs/DESIGN-ORACLE-2.md` 1.7,
//! `docs/DESIGN-ORACLE-IMPL.md` second slice) as a pure model of `contracts/KpfStake.sil` and of the stand-in claim
//! `contracts/stubs/KpfClaimStub.sil`.
//!
//! The judge measures only what each side deploys in one dispute: stakers put aged judge stake behind "true" (YES,
//! the claim stands) or "false" (NO); the side that leads when a round's window closes wins; the lead changes only when
//! the trailing side reaches `mu` times the leader's total; the losers' stake is forfeited to the winners pro rata,
//! less a burn. No contract can know a global total, so nothing here is a vote of all stake.
//!
//! The model describes whole transactions ([`JTx`]: inputs with the UTXO each spends and how, outputs with their
//! scripts) and [`verify`] returns, per input, exactly what that input's script accepts (every `require`, and a failure
//! on any 64-bit overflow). The engine tests build the same [`JTx`] as a real transaction and require the same verdict
//! input by input (`judge_attack_tests`), so the model and the contracts are held together step by step; the property
//! tests (`tests/judge_model.rs`) play whole games on the model through [`World`].

use std::collections::BTreeMap;

use silverscript_abi::ArtifactValue;

use crate::state::{encode, to_args, F, BPS, LOCK_TIME_THRESHOLD};

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
fn div(a: i64, b: i64) -> Result<i64, Refusal> {
    chk(b != 0, "division by zero")?;
    a.checked_div(b).ok_or("overflow")
}
fn rem(a: i64, b: i64) -> Result<i64, Refusal> {
    chk(b != 0, "division by zero")?;
    a.checked_rem(b).ok_or("overflow")
}

/// The payout weight unit: 10 KAS. Weights are whole units so that every pro-rata product stays below `2^63`
/// (`DESIGN-ORACLE.md` 1.2); the lead rule and every amount paid are exact sompi.
pub const UNIT: i64 = 1_000_000_000;
/// Statuses of `KpfStake` (disjoint from `KpfLock`'s 1 and `KpfBond`'s 2 and 3, so the templates can later merge).
pub const STATUS_FREE: i64 = 4;
pub const STATUS_UNBONDING: i64 = 5;
pub const STATUS_RECEIPT: i64 = 6;
pub const STATUS_DISPUTE: i64 = 7;
pub const STATUS_RESOLVED: i64 = 8;
/// The two sides: YES, the claim stands; NO, the claim is false.
pub const YES: i64 = 1;
pub const NO: i64 = 2;
/// Bounds of the judge parameters (`KpfStake.checkJudge`, the same text in the stand-in claim).
pub const MU_MIN_BPS: i64 = 11_000;
pub const MU_MAX_BPS: i64 = 15_000;
pub const BURN_MAX_BPS: i64 = 5_000;
pub const MIN_STAKE_MAX: i64 = 1_000_000_000_000;
/// Upper bounds of the judge's windows (review 6, R6-F4), at 10 DAA per second: a round at most 3 days, stake age at
/// most 30 days, unbonding at most 90 days. With them no market within the bounds makes a contest impossible by its
/// judge alone (10,000 KAS of stake at most, aged a month at most, frozen three days a round at most).
pub const ROUND_MAX: i64 = 2_592_000;
pub const AGE_MAX: i64 = 25_920_000;
pub const UNBOND_MAX: i64 = 77_760_000;
/// Stakes per deposit and receipts per settlement (`KpfStake.deposit`, `settle`: hand-unrolled).
pub const BATCH: i64 = 7;
/// A burn below this is not created (it stays with the executor, as `KpfLock`'s, DESIGN 4.5f deviation 6).
pub const BURN_DUST: i64 = 2_000_000;
/// The stake state's length (from redeem byte 1).
pub const STATE_LEN: usize = 270;
/// Sighash types: a commitment may be signed with either (experiment Z13); every other owner entry needs ALL.
pub const SIGHASH_ALL: u8 = 0x01;
pub const SIGHASH_SINGLE_ACP: u8 = 0x84;
/// Statuses of the stand-in claim.
pub const CLAIM_PENDING: i64 = 1;
pub const CLAIM_CONTESTED: i64 = 2;
pub const CLAIM_UPHELD: i64 = 3;
pub const CLAIM_REFUTED: i64 = 4;

/// The judge parameters of a market (`KpfStake` state `muBps .. minStake`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeParams {
    /// The lead factor `mu`: the trailing side takes the lead when its total reaches `ceil(mu x leader's)`.
    pub mu_bps: i64,
    /// `M_age`: a stake counts only if its UTXO was accepted at least this many DAA before the claim it judges.
    pub age: i64,
    /// `U`: the least age of an unbonding stake before it can leave (CSV).
    pub unbond: i64,
    /// `D_r`: a round's window, from the lead change (or the opening) to the earliest resolution.
    pub round: i64,
    /// `phi`: the share of the losers' stake that is burned, in basis points.
    pub burn_bps: i64,
    /// The least value of one stake UTXO.
    pub min_stake: i64,
}

impl JudgeParams {
    /// `KpfStake.checkJudge`.
    // DESIGN: the note bounds mu by 1 < mu <= 1.5 (DESIGN-ORACLE-2.md 1.2). With mu close to 1 a dispute can run
    // log_mu(stake deployed / opening stake) rounds of D_r each (about 230 000 rounds at mu = 1.0001 for a 10^9 range);
    // here mu >= 1.1, so a dispute ends within log_1.1(range) <= 230 rounds even if both sides escalate to the supply.
    // The note's M_age >= 7 days is a DAA count that depends on the network's rate; the contract requires only
    // M_age, U >= D_r (the long delay is at least one round), and since review 6 the upper bounds above.
    pub fn check(&self) -> Result<(), Refusal> {
        let p = self;
        chk(p.mu_bps >= MU_MIN_BPS && p.mu_bps <= MU_MAX_BPS, "mu out of range")?;
        chk(p.round > 0 && p.round <= ROUND_MAX, "round window")?;
        chk(p.age >= p.round && p.age <= AGE_MAX, "stake age")?;
        chk(p.unbond >= p.round && p.unbond <= UNBOND_MAX, "unbonding delay")?;
        chk(p.burn_bps >= 0 && p.burn_bps <= BURN_MAX_BPS, "burn")?;
        chk(p.min_stake >= UNIT && p.min_stake <= MIN_STAKE_MAX, "minimum stake")
    }

    /// Testnet defaults at 10 DAA per second: mu 1.25 (the note's recommendation), `M_age` = `U` = 7 days, `D_r` 12 h,
    /// burn 10 %, 100 KAS per stake.
    pub fn testnet() -> JudgeParams {
        JudgeParams { mu_bps: 12_500, age: 6_048_000, unbond: 6_048_000, round: 432_000, burn_bps: 1_000, min_stake: 10_000_000_000 }
    }

    pub(crate) fn fields(&self) -> Vec<F> {
        vec![F::I(self.mu_bps), F::I(self.age), F::I(self.unbond), F::I(self.round), F::I(self.burn_bps), F::I(self.min_stake)]
    }

    /// The total the trailing side needs to take the lead from a leader holding `l`: `ceil(l x mu)`, split product.
    pub fn need(&self, l: i64) -> Result<i64, Refusal> {
        add(mul(div(l, BPS)?, self.mu_bps)?, div(add(mul(rem(l, BPS)?, self.mu_bps)?, BPS - 1)?, BPS)?)
    }

    /// The burn of a losing total `l`: `floor(l x phi)`, split product.
    pub fn burn(&self, l: i64) -> Result<i64, Refusal> {
        add(mul(div(l, BPS)?, self.burn_bps)?, div(mul(rem(l, BPS)?, self.burn_bps)?, BPS)?)
    }
}

/// `floor(u x lnet / wu)` for a weight `u <= wu` in units and an amount `lnet` in sompi, without any product above
/// `2^63` (`KpfStake.share`): with `lnet = a x UNIT + b`, `x = u x a`, `y = u x b`, the result is
/// `(x / wu) x UNIT + ((x % wu) x UNIT + y) / wu`.
pub fn share(u: i64, lnet: i64, wu: i64) -> Result<i64, Refusal> {
    chk(wu > 0, "no winning weight")?;
    let a = div(lnet, UNIT)?;
    let b = rem(lnet, UNIT)?;
    let x = mul(u, a)?;
    let y = mul(u, b)?;
    add(mul(div(x, wu)?, UNIT)?, div(add(mul(rem(x, wu)?, UNIT)?, y)?, wu)?)
}

/// The judge block a stake shares byte for byte with its disputes and with the claims that accept its judgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeMarket {
    pub id: [u8; 32],
    pub p: JudgeParams,
}

impl JudgeMarket {
    pub(crate) fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.id)];
        f.extend(self.p.fields());
        f
    }
    /// The 87 bytes compared between a stake, a dispute and a claim.
    pub fn bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
}

/// A `KpfStake` UTXO's state. One layout for every status; fields a status does not use are 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Js {
    /// The staker (free, unbonding, receipt); zero in a dispute.
    pub owner: [u8; 32],
    pub status: i64,
    pub m: JudgeMarket,
    /// Dispute, resolved: blake3 of the contested claim's script public key (which UTXO the outcome goes to).
    pub spk_hash: [u8; 32],
    /// Receipt: its side. Dispute: the leader. Resolved: the winner.
    pub side: i64,
    /// Receipt: the stake it counts, sompi.
    pub amt: i64,
    /// Receipt: the round it joined. Dispute, resolved: the number of lead changes so far.
    pub round: i64,
    /// Dispute, resolved: totals per side, sompi, and payout weights per side, units.
    pub yes: i64,
    pub no: i64,
    pub yes_u: i64,
    pub no_u: i64,
    /// Receipt, dispute, resolved: the claim's own DAA score when the dispute opened; a stake counts only if it was
    /// accepted `M_age` before it.
    pub anchor: i64,
    /// Dispute: the round's start, `-1` while it is this UTXO's own DAA score. Resolved: the last round's start.
    pub start: i64,
    /// Resolved: winning weight not yet paid (units), losing stake not yet forfeited (sompi), the pool's own value at
    /// resolution (its carrier, the executor's at close).
    pub w_left: i64,
    pub l_left: i64,
    pub base: i64,
}

impl Js {
    fn blank(owner: [u8; 32], status: i64, m: JudgeMarket) -> Js {
        Js { owner, status, m, spk_hash: [0; 32], side: 0, amt: 0, round: 0, yes: 0, no: 0, yes_u: 0, no_u: 0, anchor: 0, start: 0, w_left: 0, l_left: 0, base: 0 }
    }
    /// A free stake of `owner`.
    pub fn free(owner: [u8; 32], m: JudgeMarket) -> Js {
        Js::blank(owner, STATUS_FREE, m)
    }
    /// The receipt a stake of value `v` becomes when it joins `side` in round `round` of the dispute anchored at `anchor`.
    pub fn receipt(owner: [u8; 32], m: JudgeMarket, side: i64, v: i64, round: i64, anchor: i64) -> Js {
        Js { side, amt: v, round, anchor, ..Js::blank(owner, STATUS_RECEIPT, m) }
    }
    /// The dispute an opening stake of value `v` creates.
    pub fn opened(m: JudgeMarket, spk_hash: [u8; 32], anchor: i64, v: i64) -> Js {
        Js { spk_hash, side: NO, no: v, no_u: v / UNIT, anchor, start: -1, ..Js::blank([0; 32], STATUS_DISPUTE, m) }
    }

    fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.owner), F::I(self.status)];
        f.extend(self.m.fields());
        f.push(F::B(self.spk_hash));
        for x in [self.side, self.amt, self.round, self.yes, self.no, self.yes_u, self.no_u, self.anchor, self.start, self.w_left, self.l_left, self.base] {
            f.push(F::I(x));
        }
        f
    }
    /// The encoded state (270 bytes, from redeem byte 1).
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    /// The constructor arguments of `KpfStake`.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        to_args(&self.fields())
    }
    /// Total and weight of a side.
    pub fn total(&self, side: i64) -> i64 {
        if side == YES {
            self.yes
        } else {
            self.no
        }
    }
    pub fn weight(&self, side: i64) -> i64 {
        if side == YES {
            self.yes_u
        } else {
            self.no_u
        }
    }
}

/// The other side.
pub fn other(side: i64) -> i64 {
    if side == YES {
        NO
    } else {
        YES
    }
}

/// The stand-in claim's state (`contracts/stubs/KpfClaimStub.sil`, after its build constants).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimStub {
    pub m: JudgeMarket,
    pub owner: [u8; 32],
    pub status: i64,
}

impl ClaimStub {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([F::B(self.owner), F::I(self.status)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_state(&self) -> Vec<ArtifactValue> {
        to_args(&self.fields())
    }
}

// ---------------------------------------------------------------- transactions

/// What an output (or the UTXO an input spends) is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Script {
    Stake(Js),
    Claim(ClaimStub),
    /// A plain P2PK output of the key with this seed (the tests' `keypair(seed)`).
    Key(u8),
    /// P2SH(OP_RETURN), unspendable (DESIGN 4.5f, Z7).
    Burn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JOut {
    pub script: Script,
    pub value: i64,
    /// The covenant id it carries (32 non-zero bytes).
    pub cov: Option<[u8; 32]>,
}

/// How an input is spent: the entry and its arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spend {
    /// `KpfStake.open(claimIdx, cOut, dOut, s)`.
    Open { claim: i64, c_out: i64, d_out: i64 },
    /// `KpfStake.commit(side, round, anchor, s)`.
    Commit { side: i64, round: i64, anchor: i64 },
    Unbond,
    Withdraw,
    /// `KpfStake.deposit(side, first, k)`.
    Deposit { side: i64, first: i64, k: i64 },
    /// `KpfStake.resolve(claimIdx)`.
    Resolve { claim: i64 },
    /// `KpfStake.settle(first, k)`.
    Settle { first: i64, k: i64 },
    /// `KpfStake.close(j)`: the burn output's index.
    Close { j: i64 },
    /// `KpfStake.redeem(d)`.
    Redeem { d: i64 },
    /// `KpfClaimStub.contest(u, dOut)`.
    Contest { u: i64, d_out: i64 },
    /// `KpfClaimStub.decide(d)`.
    Decide { d: i64 },
    /// `KpfClaimStub.close(s)`.
    ClaimClose,
    /// A plain key's signature.
    Key,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JIn {
    /// The world's handle of the UTXO spent.
    pub id: u64,
    pub utxo: JOut,
    /// The UTXO's DAA score; `-1` for a parent not yet accepted.
    pub daa: i64,
    /// The committed sequence (what `this.ageDaa` proves).
    pub seq: i64,
    pub spend: Spend,
    /// Who signs (a key seed), and with which sighash type.
    pub signer: Option<u8>,
    pub sighash: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JTx {
    pub ins: Vec<JIn>,
    pub outs: Vec<JOut>,
    /// The lock time (what `tx.daa` proves).
    pub lock: i64,
}

/// What the model cannot compute itself: the hash a script reads of a script public key (`blake3` of the versioned
/// bytes), which needs the compiled templates. The engine tests supply the real one.
pub trait Env {
    fn spk_hash(&self, s: &Script) -> [u8; 32];
}

/// An environment for the model alone: any injective stand-in for the hash.
pub struct FakeEnv;

impl Env for FakeEnv {
    fn spk_hash(&self, s: &Script) -> [u8; 32] {
        *blake3::hash(format!("{s:?}").as_bytes()).as_bytes()
    }
}

/// The x-only key of a seed (`SecretKey [seed; 32]`, as the tests' `keypair`).
pub fn key_of(seed: u8) -> [u8; 32] {
    static KEYS: std::sync::OnceLock<Vec<[u8; 32]>> = std::sync::OnceLock::new();
    // Seeds 0 and 255 are not valid secret keys (0, and above the group order); they stand for "no key" (zero bytes).
    KEYS.get_or_init(|| (0..=255u8).map(|s| crate::tx::pubkey_of(&[s; 32]).unwrap_or([0; 32])).collect())[seed as usize]
}

impl JTx {
    fn in_id(&self, i: usize) -> Option<[u8; 32]> {
        self.ins[i].utxo.cov
    }
    fn out_id(&self, j: usize) -> Option<[u8; 32]> {
        self.outs.get(j).and_then(|o| o.cov)
    }
    fn cov_in(&self, id: [u8; 32]) -> usize {
        self.ins.iter().filter(|i| i.utxo.cov == Some(id)).count()
    }
    fn cov_out(&self, id: [u8; 32]) -> usize {
        self.outs.iter().filter(|o| o.cov == Some(id)).count()
    }
    fn index(&self, i: i64) -> Result<usize, Refusal> {
        chk(i >= 0 && (i as usize) < self.ins.len(), "input index out of range")?;
        Ok(i as usize)
    }
    fn out(&self, j: i64) -> Result<&JOut, Refusal> {
        chk(j >= 0 && (j as usize) < self.outs.len(), "output index out of range")?;
        Ok(&self.outs[j as usize])
    }
    fn stake_out(&self, j: i64, want: &Js) -> Result<&JOut, Refusal> {
        let o = self.out(j)?;
        chk(o.script == Script::Stake(want.clone()), "output is not the expected stake state")?;
        Ok(o)
    }

    /// Whether the covenant bindings are valid (`CovenantsContext::from_tx`): every output's id is carried by an
    /// input (this model has no genesis: the dispute and its receipts take the claim's id).
    pub fn bindings_ok(&self) -> bool {
        self.outs.iter().all(|o| match o.cov {
            None => true,
            Some(id) => self.cov_in(id) > 0,
        })
    }
}

fn signed(tx: &JTx, me: usize, owner: &[u8; 32], types: &[u8]) -> Result<(), Refusal> {
    let i = &tx.ins[me];
    chk(i.signer.map(key_of).as_ref() == Some(owner), "no signature of the owner")?;
    chk(types.contains(&i.sighash), "sighash type not accepted")
}

/// The verdict of every input of `tx`: what its script accepts. A transaction whose bindings are invalid is refused as
/// a whole (every input `Err`).
pub fn verify(tx: &JTx, env: &dyn Env) -> Vec<Result<(), Refusal>> {
    if !tx.bindings_ok() {
        return vec![Err("invalid covenant bindings"); tx.ins.len()];
    }
    (0..tx.ins.len()).map(|me| verify_input(tx, me, env)).collect()
}

/// Every input accepts.
pub fn accepted(tx: &JTx, env: &dyn Env) -> Result<(), (usize, Refusal)> {
    for (i, r) in verify(tx, env).into_iter().enumerate() {
        r.map_err(|e| (i, e))?;
    }
    Ok(())
}

fn verify_input(tx: &JTx, me: usize, env: &dyn Env) -> Result<(), Refusal> {
    let inp = &tx.ins[me];
    match (&inp.utxo.script, inp.spend) {
        // A plain key accepts its own signature of any valid sighash type.
        (Script::Key(seed), Spend::Key) => chk(inp.signer == Some(*seed), "key signature"),
        (Script::Stake(s), sp) => stake_entry(tx, me, s, sp, env),
        (Script::Claim(c), sp) => claim_entry(tx, me, c, sp),
        _ => Err("no such entry"),
    }
}

/// The state of input `i` read as this template (a `KpfStake` UTXO), or a refusal.
fn stake_in(tx: &JTx, i: usize) -> Result<&Js, Refusal> {
    match &tx.ins[i].utxo.script {
        Script::Stake(s) => Ok(s),
        _ => Err("input is not a stake template"),
    }
}

fn stake_entry(tx: &JTx, me: usize, s: &Js, sp: Spend, env: &dyn Env) -> Result<(), Refusal> {
    s.m.p.check()?;
    let p = &s.m.p;
    let v_in = tx.ins[me].utxo.value;
    match sp {
        // DESIGN: the dispute and its receipts take the claim's covenant id (DESIGN-ORACLE-2.md 1.7, "same id"), not a
        // genesis id of their own (DESIGN-ORACLE.md 2): every output of the id is then counted by OpCovOutputCount and
        // no twin scan is needed; a genesis id would also make the dispute's state (the contested claim's script hash)
        // and the claim's (its dispute's id) refer to each other. Receipts carry the anchor so that receipts of an
        // earlier dispute of the same lineage never settle against a later one.
        // DESIGN: the opening stake is aged judge stake like every other (the note: "an opening deposit ... any KAS"),
        // and age is measured against the claim's own DAA score (the anchor), not at the deposit (the note's CSV): under
        // the note's rule a dispute that runs longer than M_age admits capital bonded after the claim was made.
        Spend::Open { claim, c_out, d_out } => {
            chk(s.status == STATUS_FREE, "not a free stake")?;
            signed(tx, me, &s.owner, &[SIGHASH_ALL])?;
            chk(v_in >= p.min_stake, "stake below the minimum")?;
            let c = tx.index(claim)?;
            chk(c != me, "the claim is this input")?;
            let cid = tx.in_id(c).ok_or("the claim has no covenant id")?;
            chk(tx.cov_in(cid) == 1, "another input of the claim's id")?;
            chk(tx.cov_out(cid) == 3, "not exactly three outputs of the claim's id")?;
            let anchor = tx.ins[c].daa;
            chk(anchor >= 0, "the claim is not accepted")?;
            let d = tx.ins[me].daa;
            chk(d >= 0, "stake not accepted")?;
            chk(add(d, p.age)? <= anchor, "stake not aged before the claim")?;
            chk(c_out != d_out && c_out != me as i64 && d_out != me as i64, "outputs not distinct")?;
            let co = tx.out(c_out)?;
            chk(co.cov == Some(cid), "the claim's continuation does not carry its id")?;
            let h = env.spk_hash(&co.script);
            let r = tx.stake_out(me as i64, &Js::receipt(s.owner, s.m, NO, v_in, 0, anchor))?;
            chk(r.cov == Some(cid), "the receipt does not carry the claim's id")?;
            chk(r.value >= v_in, "the receipt holds less than the stake")?;
            let dd = tx.stake_out(d_out, &Js::opened(s.m, h, anchor, v_in))?;
            chk(dd.cov == Some(cid), "the dispute does not carry the claim's id")?;
            Ok(())
        }
        // DESIGN: a commitment may be signed SINGLE|ANYONECANPAY (experiment Z13: the signature fixes its own output's
        // covenant binding), so a staker's consent is not bound to the dispute's outpoint and aggregators can chain
        // deposits without re-signing; every other owner entry requires SIGHASH_ALL.
        Spend::Commit { side, round, anchor } => {
            chk(s.status == STATUS_FREE, "not a free stake")?;
            signed(tx, me, &s.owner, &[SIGHASH_ALL, SIGHASH_SINGLE_ACP])?;
            chk(side == YES || side == NO, "no such side")?;
            let r = tx.stake_out(me as i64, &Js::receipt(s.owner, s.m, side, v_in, round, anchor))?;
            chk(r.cov.is_some(), "the receipt carries no covenant id")?;
            chk(r.value >= v_in, "the receipt holds less than the stake")
        }
        Spend::Unbond => {
            chk(s.status == STATUS_FREE, "not a free stake")?;
            signed(tx, me, &s.owner, &[SIGHASH_ALL])?;
            let o = tx.stake_out(me as i64, &Js { status: STATUS_UNBONDING, ..s.clone() })?;
            chk(o.value >= v_in, "value taken")
        }
        Spend::Withdraw => {
            chk(s.status == STATUS_UNBONDING, "not unbonding")?;
            signed(tx, me, &s.owner, &[SIGHASH_ALL])?;
            chk(tx.ins[me].seq >= p.unbond, "unbonding delay")
        }
        // DESIGN: only the trailing side deposits, and the window moves only when the lead changes. The note lets either
        // side deposit and restarts the window on every deposit, bounded by a minimum step eps: a leader could raise the
        // bar in the last DAA of a window, and either side could prolong a dispute at a cost of eps per restart. Here the
        // bar is fixed for the round, rounds are bounded by mu alone, and eps is not needed.
        Spend::Deposit { side, first, k } => {
            chk(s.status == STATUS_DISPUTE, "not an open dispute")?;
            let id = tx.in_id(me).ok_or("no covenant id")?;
            chk(tx.cov_in(id) == 1, "another input of the dispute's id")?;
            chk(side == YES || side == NO, "no such side")?;
            chk(side != s.side, "the leader cannot add stake")?;
            chk((1..=BATCH).contains(&k), "batch size")?;
            chk(first >= 0, "first")?;
            let end = add(first, k)?;
            chk(end <= tx.ins.len() as i64, "stake range beyond the inputs")?;
            chk((me as i64) < first || (me as i64) >= end, "the dispute inside the stake range")?;
            chk(tx.cov_out(id) == 1 + k as usize, "not exactly one output of the id per stake plus the continuation")?;
            let mut sum = 0i64;
            let mut units = 0i64;
            for i in first..end {
                let iu = i as usize;
                let t = stake_in(tx, iu)?;
                chk(t.status == STATUS_FREE, "a stake that is not free")?;
                chk(t.m == s.m, "a stake of another judge")?;
                let v = tx.ins[iu].utxo.value;
                chk(v >= p.min_stake, "stake below the minimum")?;
                let d = tx.ins[iu].daa;
                chk(d >= 0, "stake not accepted")?;
                chk(add(d, p.age)? <= s.anchor, "stake not aged before the claim")?;
                let r = tx.stake_out(i, &Js::receipt(t.owner, s.m, side, v, s.round, s.anchor))?;
                chk(r.cov == Some(id), "the receipt does not carry the dispute's id")?;
                chk(r.value >= v, "the receipt holds less than the stake")?;
                sum = add(sum, v)?;
                units = add(units, v / UNIT)?;
            }
            let mut n = s.clone();
            let t2 = add(s.total(side), sum)?;
            let u2 = add(s.weight(side), units)?;
            if side == YES {
                n.yes = t2;
                n.yes_u = u2;
            } else {
                n.no = t2;
                n.no_u = u2;
            }
            if t2 >= p.need(s.total(s.side))? {
                n.side = side;
                n.round = add(s.round, 1)?;
                n.start = -1;
            } else if s.start == -1 {
                let own = tx.ins[me].daa;
                chk(own >= 0, "dispute not accepted")?;
                n.start = own;
            }
            let o = tx.stake_out(me as i64, &n)?;
            chk(o.cov == Some(id), "the continuation does not carry the id")?;
            chk(o.value >= v_in, "value taken from the dispute")
        }
        // DESIGN: the verdict is delivered in the resolving transaction, to the UTXO whose script hash the opening
        // recorded: separately, a pool could close before the claim learned its verdict, or the verdict could go to
        // another UTXO of the claim's id (a pair's parts share an id).
        Spend::Resolve { claim } => {
            chk(s.status == STATUS_DISPUTE, "not an open dispute")?;
            let id = tx.in_id(me).ok_or("no covenant id")?;
            chk(tx.cov_in(id) == 2, "not exactly the dispute and its claim")?;
            chk(tx.cov_out(id) == 2, "not exactly two outputs of the id")?;
            let st = if s.start == -1 { tx.ins[me].daa } else { s.start };
            chk(st >= 0, "round start unknown")?;
            let e = add(st, p.round)?;
            chk(e < LOCK_TIME_THRESHOLD, "window end beyond the DAA lock-time range")?;
            chk(tx.lock >= e, "the round's window has not closed")?;
            let c = tx.index(claim)?;
            chk(c != me, "the claim is this input")?;
            chk(tx.in_id(c) == Some(id), "the claim input is not of the dispute's id")?;
            chk(env.spk_hash(&tx.ins[c].utxo.script) == s.spk_hash, "not the contested claim")?;
            let w = s.side;
            let n = Js { status: STATUS_RESOLVED, start: st, w_left: s.weight(w), l_left: s.total(other(w)), base: v_in, ..s.clone() };
            let o = tx.stake_out(me as i64, &n)?;
            chk(o.cov == Some(id), "the continuation does not carry the id")?;
            chk(o.value >= v_in, "value taken from the dispute")
        }
        // DESIGN: receipts hold the stake itself (the note: tiny receipt records beside a dispute UTXO that holds every
        // stake, whose dust would cost about 10^12 / value of storage mass each); losers are forfeited into the pool and
        // winners paid from it, in any order. Weights are whole 10-KAS units of the amount counted (the note's units),
        // so u x lnet / wu needs no product above 2^63; the lead rule and every payment are exact sompi.
        Spend::Settle { first, k } => {
            chk(s.status == STATUS_RESOLVED, "not resolved")?;
            let id = tx.in_id(me).ok_or("no covenant id")?;
            chk((1..=BATCH).contains(&k), "batch size")?;
            chk(tx.cov_in(id) == 1 + k as usize, "not exactly the pool and the receipts")?;
            chk(tx.cov_out(id) == 1, "not exactly one output of the id")?;
            chk(first >= 0, "first")?;
            let end = add(first, k)?;
            chk(end <= tx.ins.len() as i64, "receipt range beyond the inputs")?;
            chk((me as i64) < first || (me as i64) >= end, "the pool inside the receipt range")?;
            let w = s.side;
            let wu = s.weight(w);
            let l = s.total(other(w));
            let lnet = sub(l, p.burn(l)?)?;
            let mut wl = s.w_left;
            let mut ll = s.l_left;
            let mut gained = 0i64;
            let mut paid = 0i64;
            for i in first..end {
                let iu = i as usize;
                chk(tx.in_id(iu) == Some(id), "a receipt of another dispute id")?;
                let t = stake_in(tx, iu)?;
                chk(t.status == STATUS_RECEIPT, "not a receipt")?;
                chk(t.m == s.m, "a receipt of another judge")?;
                chk(t.anchor == s.anchor, "a receipt of another dispute of this claim")?;
                let r = tx.ins[iu].utxo.value;
                if t.side == w {
                    let u = t.amt / UNIT;
                    let sh = share(u, lnet, wu)?;
                    let o = tx.stake_out(i, &Js::free(t.owner, s.m))?;
                    chk(o.value >= add(r, sh)?, "winner paid less than its receipt and share")?;
                    wl = sub(wl, u)?;
                    paid = add(paid, sh)?;
                } else {
                    ll = sub(ll, t.amt)?;
                    gained = add(gained, r)?;
                }
            }
            chk(wl >= 0 && ll >= 0, "more settled than counted")?;
            let pool = sub(add(v_in, gained)?, paid)?;
            chk(pool >= 0, "the pool cannot pay")?;
            let o = tx.stake_out(me as i64, &Js { w_left: wl, l_left: ll, ..s.clone() })?;
            chk(o.cov == Some(id), "the continuation does not carry the id")?;
            chk(o.value >= pool, "value taken from the pool")
        }
        Spend::Close { j } => {
            chk(s.status == STATUS_RESOLVED, "not resolved")?;
            let id = tx.in_id(me).ok_or("no covenant id")?;
            chk(tx.cov_in(id) == 1, "another input of the id")?;
            chk(tx.cov_out(id) == 0, "an output of the id remains")?;
            chk(s.w_left == 0 && s.l_left == 0, "receipts not settled")?;
            let b = sub(v_in, s.base)?;
            if b >= BURN_DUST {
                let o = tx.out(j)?;
                chk(o.script == Script::Burn, "no burn output")?;
                chk(o.value >= b, "burn below the pool's remainder")?;
            }
            Ok(())
        }
        Spend::Redeem { d } => {
            chk(s.status == STATUS_RECEIPT, "not a receipt")?;
            let id = tx.in_id(me).ok_or("no covenant id")?;
            let di = tx.index(d)?;
            chk(di != me, "the pool is this input")?;
            chk(tx.in_id(di) == Some(id), "the pool is not of this receipt's id")?;
            let t = stake_in(tx, di)?;
            chk(t.status == STATUS_RESOLVED, "the dispute is not resolved")?;
            chk(t.anchor == s.anchor, "another dispute of this claim")
        }
        _ => Err("no such entry"),
    }
}

fn claim_entry(tx: &JTx, me: usize, c: &ClaimStub, sp: Spend) -> Result<(), Refusal> {
    c.m.p.check()?;
    let id = tx.in_id(me).ok_or("no covenant id")?;
    match sp {
        Spend::Contest { u, d_out } => {
            chk(c.status == CLAIM_PENDING, "not pending")?;
            chk(tx.cov_in(id) == 1, "another input of the id")?;
            chk(tx.cov_out(id) == 3, "not exactly the claim, the dispute and the receipt")?;
            let ui = tx.index(u)?;
            let t = stake_in(tx, ui)?;
            chk(t.status == STATUS_FREE, "the opener is not a free stake")?;
            chk(t.m == c.m, "a stake of another judge")?;
            chk(tx.ins[ui].spend == Spend::Open { claim: me as i64, c_out: me as i64, d_out }, "the opener does not open this claim")?;
            chk(d_out >= 0 && tx.out_id(d_out as usize) == Some(id), "the dispute does not carry the claim's id")?;
            let o = tx.out(me as i64)?;
            chk(o.script == Script::Claim(ClaimStub { status: CLAIM_CONTESTED, ..c.clone() }), "continuation")?;
            chk(o.cov == Some(id), "continuation id")?;
            chk(o.value >= tx.ins[me].utxo.value, "value taken")
        }
        Spend::Decide { d } => {
            chk(c.status == CLAIM_CONTESTED, "not contested")?;
            chk(tx.cov_in(id) == 2, "not exactly the claim and its dispute")?;
            chk(tx.cov_out(id) == 2, "not exactly two outputs of the id")?;
            let di = tx.index(d)?;
            chk(tx.in_id(di) == Some(id), "the dispute is not of this claim's id")?;
            let t = stake_in(tx, di)?;
            chk(t.status == STATUS_DISPUTE, "not an open dispute")?;
            chk(t.m == c.m, "a dispute of another judge")?;
            chk(tx.ins[di].spend == Spend::Resolve { claim: me as i64 }, "the dispute does not resolve onto this claim")?;
            let st = if t.side == YES { CLAIM_UPHELD } else { CLAIM_REFUTED };
            let o = tx.out(me as i64)?;
            chk(o.script == Script::Claim(ClaimStub { status: st, ..c.clone() }), "continuation")?;
            chk(o.cov == Some(id), "continuation id")?;
            chk(o.value >= tx.ins[me].utxo.value, "value taken")
        }
        Spend::ClaimClose => {
            chk(c.status != CLAIM_CONTESTED, "contested")?;
            chk(tx.cov_in(id) == 1, "another input of the id")?;
            chk(tx.cov_out(id) == 0, "an output of the id remains")?;
            signed(tx, me, &c.owner, &[SIGHASH_ALL])
        }
        _ => Err("no such entry"),
    }
}

// ---------------------------------------------------------------- a world of UTXOs

/// A live UTXO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Utxo {
    pub out: JOut,
    pub daa: i64,
}

/// The UTXO set a game plays in, and the transactions it applies. Handles are never reused.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub utxos: BTreeMap<u64, Utxo>,
    pub next: u64,
}

impl World {
    pub fn add(&mut self, out: JOut, daa: i64) -> u64 {
        let h = self.next;
        self.next += 1;
        self.utxos.insert(h, Utxo { out, daa });
        h
    }
    /// An input spending handle `h`.
    pub fn input(&self, h: u64, spend: Spend, signer: Option<u8>, sighash: u8, seq: i64) -> JIn {
        let u = &self.utxos[&h];
        JIn { id: h, utxo: u.out.clone(), daa: u.daa, seq, spend, signer, sighash }
    }
    /// Applies `tx`, accepted at DAA `daa`: removes its inputs, adds its outputs, returns their handles. The caller
    /// checks acceptance first; this refuses a spent or unknown input.
    pub fn apply(&mut self, tx: &JTx, daa: i64) -> Result<Vec<u64>, String> {
        for i in &tx.ins {
            if !self.utxos.contains_key(&i.id) {
                return Err(format!("input {} is spent or unknown", i.id));
            }
        }
        for i in &tx.ins {
            self.utxos.remove(&i.id);
        }
        Ok(tx.outs.iter().map(|o| self.add(o.clone(), daa)).collect())
    }
    /// The sum of every UTXO's value.
    pub fn total(&self) -> i64 {
        self.utxos.values().map(|u| u.out.value).sum()
    }
}

/// The outcome of a dispute played to the end on the model: what each receipt was paid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payout {
    pub owner: [u8; 32],
    pub side: i64,
    pub staked: i64,
    pub paid: i64,
}

/// What every receipt of a resolved dispute `r` is paid, in settlement order (for the conservation checks): winners
/// their receipt value plus `share`, losers nothing.
pub fn payouts(r: &Js, receipts: &[(Js, i64)]) -> Result<Vec<Payout>, Refusal> {
    let w = r.side;
    let l = r.total(other(w));
    let lnet = sub(l, r.m.p.burn(l)?)?;
    let wu = r.weight(w);
    receipts
        .iter()
        .map(|(t, v)| {
            let paid = if t.side == w { add(*v, share(t.amt / UNIT, lnet, wu)?)? } else { 0 };
            Ok(Payout { owner: t.owner, side: t.side, staked: *v, paid })
        })
        .collect()
}

// ---------------------------------------------------------------- transaction shapes

/// The fee every builder leaves out of the funder's change.
pub const FEE: i64 = 1_000_000;

/// A table where disputes are played: a world, the judge market, the funder's key seed. The builders return the
/// transaction shape each path uses (indices are those the engine tests and the tooling use); acceptance is
/// [`verify`]'s business.
#[derive(Clone, Debug)]
pub struct Arena {
    pub w: World,
    pub m: JudgeMarket,
    pub funder: u8,
}

impl Arena {
    pub fn new(m: JudgeMarket, funder: u8) -> Arena {
        Arena { w: World::default(), m, funder }
    }
    /// A plain funding UTXO of the funder (1,000,000 KAS).
    pub fn fund(&mut self) -> u64 {
        self.w.add(JOut { script: Script::Key(self.funder), value: 100_000_000_000_000, cov: None }, 0)
    }
    /// A free stake of key `seed`, accepted at `daa`.
    pub fn stake(&mut self, seed: u8, value: i64, daa: i64) -> u64 {
        self.w.add(JOut { script: Script::Stake(Js::free(key_of(seed), self.m)), value, cov: None }, daa)
    }
    /// A pending stand-in claim of covenant id `id`, accepted at `daa`.
    pub fn claim(&mut self, id: [u8; 32], owner: u8, daa: i64) -> u64 {
        let c = ClaimStub { m: self.m, owner: key_of(owner), status: CLAIM_PENDING };
        self.w.add(JOut { script: Script::Claim(c), value: 100_000_000, cov: Some(id) }, daa)
    }
    fn funder_in(&mut self) -> JIn {
        let f = self.fund();
        self.w.input(f, Spend::Key, Some(self.funder), SIGHASH_ALL, 0)
    }
    fn change(&self, v: i64) -> JOut {
        JOut { script: Script::Key(self.funder), value: v - FEE, cov: None }
    }
    pub fn js(&self, h: u64) -> Js {
        match &self.w.utxos[&h].out.script {
            Script::Stake(s) => s.clone(),
            s => panic!("not a stake: {s:?}"),
        }
    }

    /// Open: in 0 the claim (`contest(1, 2)`), 1 the opener's stake (`open(0, 0, 2)`, signed by `seed`), 2 the funder;
    /// out 0 the contested claim, 1 the opener's receipt, 2 the dispute (value `base`, the funder's), 3 change.
    pub fn open_tx(&mut self, claim: u64, stake: u64, seed: u8, base: i64, env: &dyn Env) -> JTx {
        let cu = self.w.utxos[&claim].clone();
        let su = self.w.utxos[&stake].clone();
        let id = cu.out.cov.expect("claim id");
        let c = match &cu.out.script {
            Script::Claim(c) => c.clone(),
            s => panic!("not a claim: {s:?}"),
        };
        let s = match &su.out.script {
            Script::Stake(s) => s.clone(),
            x => panic!("not a stake: {x:?}"),
        };
        let cont = JOut { script: Script::Claim(ClaimStub { status: CLAIM_CONTESTED, ..c }), value: cu.out.value, cov: Some(id) };
        let h = env.spk_hash(&cont.script);
        let f = self.funder_in();
        let fv = f.utxo.value;
        let ins = vec![
            self.w.input(claim, Spend::Contest { u: 1, d_out: 2 }, None, SIGHASH_ALL, 0),
            self.w.input(stake, Spend::Open { claim: 0, c_out: 0, d_out: 2 }, Some(seed), SIGHASH_ALL, 0),
            f,
        ];
        let outs = vec![
            cont,
            JOut { script: Script::Stake(Js::receipt(s.owner, s.m, NO, su.out.value, 0, cu.daa)), value: su.out.value, cov: Some(id) },
            JOut { script: Script::Stake(Js::opened(s.m, h, cu.daa, su.out.value)), value: base, cov: Some(id) },
            self.change(fv - base),
        ];
        JTx { ins, outs, lock: 0 }
    }

    /// Deposit: in 0 the dispute (`deposit(side, 1, k)`), 1..=k the stakes (`commit`, each signed by its seed with
    /// `sighash`), k + 1 the funder; out 0 the continuation, 1..=k the receipts, k + 1 change. The continuation is the
    /// state the rule implies (when the rule refuses the step, the dispute's state is copied: the transaction is then
    /// refused by the dispute's input).
    pub fn deposit_tx(&mut self, dispute: u64, side: i64, stakes: &[(u64, u8)], sighash: u8) -> JTx {
        let du = self.w.utxos[&dispute].clone();
        let d = self.js(dispute);
        let id = du.out.cov.expect("dispute id");
        let mut ins = vec![self.w.input(dispute, Spend::Deposit { side, first: 1, k: stakes.len() as i64 }, None, SIGHASH_ALL, 0)];
        let mut outs = vec![JOut { script: Script::Stake(d.clone()), value: du.out.value, cov: Some(id) }];
        for (h, seed) in stakes {
            let su = self.w.utxos[h].clone();
            let owner = match &su.out.script {
                Script::Stake(s) => s.owner,
                _ => [0; 32],
            };
            ins.push(self.w.input(*h, Spend::Commit { side, round: d.round, anchor: d.anchor }, Some(*seed), sighash, 0));
            outs.push(JOut { script: Script::Stake(Js::receipt(owner, self.m, side, su.out.value, d.round, d.anchor)), value: su.out.value, cov: Some(id) });
        }
        let f = self.funder_in();
        outs.push(self.change(f.utxo.value));
        ins.push(f);
        let mut tx = JTx { ins, outs, lock: 0 };
        if let Some(n) = next_dispute(&tx, &d) {
            tx.outs[0].script = Script::Stake(n);
        }
        tx
    }

    /// Resolve and deliver: in 0 the dispute (`resolve(1)`), 1 the claim (`decide(0)`), 2 the funder; out 0 the resolved
    /// pool, 1 the decided claim, 2 change; lock time `lock`.
    pub fn resolve_tx(&mut self, dispute: u64, claim: u64, lock: i64) -> JTx {
        let du = self.w.utxos[&dispute].clone();
        let cu = self.w.utxos[&claim].clone();
        let d = self.js(dispute);
        let st = if d.start == -1 { du.daa } else { d.start };
        let w = d.side;
        let r = Js { status: STATUS_RESOLVED, start: st, w_left: d.weight(w), l_left: d.total(other(w)), base: du.out.value, ..d.clone() };
        let c = match &cu.out.script {
            Script::Claim(c) => c.clone(),
            s => panic!("not a claim: {s:?}"),
        };
        let decided = ClaimStub { status: if w == YES { CLAIM_UPHELD } else { CLAIM_REFUTED }, ..c };
        let f = self.funder_in();
        let fv = f.utxo.value;
        let ins = vec![
            self.w.input(dispute, Spend::Resolve { claim: 1 }, None, SIGHASH_ALL, 0),
            self.w.input(claim, Spend::Decide { d: 0 }, None, SIGHASH_ALL, 0),
            f,
        ];
        let outs = vec![
            JOut { script: Script::Stake(r), value: du.out.value, cov: du.out.cov },
            JOut { script: Script::Claim(decided), value: cu.out.value, cov: cu.out.cov },
            self.change(fv),
        ];
        JTx { ins, outs, lock }
    }

    /// Settle: in 0 the pool (`settle(1, k)`), 1..=k the receipts (`redeem(0)`), winners first, k + 1 the funder; out 0
    /// the pool's continuation, then each winner's free stake (its receipt value plus its share) at its own index,
    /// then change. Receipts are reordered winners first so that losers need no output.
    pub fn settle_tx(&mut self, pool: u64, receipts: &[u64]) -> JTx {
        let pu = self.w.utxos[&pool].clone();
        let p = self.js(pool);
        let w = p.side;
        let mut rs: Vec<u64> = receipts.to_vec();
        rs.sort_by_key(|h| if self.js(*h).side == w { 0 } else { 1 });
        let l = p.total(other(w));
        let lnet = l - p.m.p.burn(l).unwrap_or(0);
        let mut ins = vec![self.w.input(pool, Spend::Settle { first: 1, k: rs.len() as i64 }, None, SIGHASH_ALL, 0)];
        let mut outs = vec![JOut { script: Script::Stake(p.clone()), value: pu.out.value, cov: pu.out.cov }];
        let (mut wl, mut ll, mut gained, mut paid) = (p.w_left, p.l_left, 0i64, 0i64);
        for h in &rs {
            let t = self.js(*h);
            let v = self.w.utxos[h].out.value;
            ins.push(self.w.input(*h, Spend::Redeem { d: 0 }, None, SIGHASH_ALL, 0));
            if t.side == w {
                let u = t.amt / UNIT;
                let sh = share(u, lnet, p.weight(w)).unwrap_or(0);
                outs.push(JOut { script: Script::Stake(Js::free(t.owner, self.m)), value: v + sh, cov: None });
                wl -= u;
                paid += sh;
            } else {
                ll -= t.amt;
                gained += v;
            }
        }
        outs[0] = JOut { script: Script::Stake(Js { w_left: wl, l_left: ll, ..p }), value: pu.out.value + gained - paid, cov: pu.out.cov };
        let f = self.funder_in();
        outs.push(self.change(f.utxo.value));
        ins.push(f);
        JTx { ins, outs, lock: 0 }
    }

    /// Close: in 0 the pool (`close(1)`), 1 the funder; out 0 change (with the pool's base), 1 the burn (when due).
    pub fn close_tx(&mut self, pool: u64) -> JTx {
        let pu = self.w.utxos[&pool].clone();
        let p = self.js(pool);
        let b = pu.out.value - p.base;
        let f = self.funder_in();
        let fv = f.utxo.value;
        let ins = vec![self.w.input(pool, Spend::Close { j: 1 }, None, SIGHASH_ALL, 0), f];
        let mut outs = vec![self.change(fv + p.base)];
        if b >= BURN_DUST {
            outs.push(JOut { script: Script::Burn, value: b, cov: None });
        } else {
            outs[0].value += b;
        }
        JTx { ins, outs, lock: 0 }
    }

    /// Unbond: in 0 the stake, 1 the funder; out 0 the unbonding stake, 1 change.
    pub fn unbond_tx(&mut self, stake: u64, seed: u8) -> JTx {
        let su = self.w.utxos[&stake].clone();
        let s = self.js(stake);
        let f = self.funder_in();
        let fv = f.utxo.value;
        let ins = vec![self.w.input(stake, Spend::Unbond, Some(seed), SIGHASH_ALL, 0), f];
        let outs = vec![JOut { script: Script::Stake(Js { status: STATUS_UNBONDING, ..s }), value: su.out.value, cov: None }, self.change(fv)];
        JTx { ins, outs, lock: 0 }
    }

    /// Withdraw: in 0 the unbonding stake (sequence `seq`), 1 the funder; out 0 the owner's key, 1 change.
    pub fn withdraw_tx(&mut self, stake: u64, seed: u8, seq: i64) -> JTx {
        let su = self.w.utxos[&stake].clone();
        let f = self.funder_in();
        let fv = f.utxo.value;
        let ins = vec![self.w.input(stake, Spend::Withdraw, Some(seed), SIGHASH_ALL, seq), f];
        let outs = vec![JOut { script: Script::Key(seed), value: su.out.value, cov: None }, self.change(fv)];
        JTx { ins, outs, lock: 0 }
    }

    /// Applies an accepted transaction at DAA `daa`; returns the outputs' handles.
    pub fn apply(&mut self, tx: &JTx, daa: i64) -> Vec<u64> {
        self.w.apply(tx, daa).expect("inputs live")
    }
}

/// The dispute state a deposit `tx` (dispute at input 0) implies, if the rule accepts the stakes' sum; `None` if any
/// arithmetic step fails.
pub fn next_dispute(tx: &JTx, d: &Js) -> Option<Js> {
    let (side, first, k) = match tx.ins[0].spend {
        Spend::Deposit { side, first, k } => (side, first, k),
        _ => return None,
    };
    let mut sum = 0i64;
    let mut units = 0i64;
    for i in first..first + k {
        let v = tx.ins.get(i as usize)?.utxo.value;
        sum = sum.checked_add(v)?;
        units = units.checked_add(v / UNIT)?;
    }
    let mut n = d.clone();
    let t2 = d.total(side).checked_add(sum)?;
    let u2 = d.weight(side).checked_add(units)?;
    if side == YES {
        n.yes = t2;
        n.yes_u = u2;
    } else {
        n.no = t2;
        n.no_u = u2;
    }
    if side != d.side && t2 >= d.m.p.need(d.total(d.side)).ok()? {
        n.side = side;
        n.round = d.round.checked_add(1)?;
        n.start = -1;
    } else if d.start == -1 {
        n.start = tx.ins[0].daa;
    }
    Some(n)
}
