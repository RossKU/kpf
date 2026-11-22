//! Committee markets (`docs/DESIGN-COMMITTEE.md`): the staked committee that signs prices, its epoch tables and exits, signed prices and
//! attestations, as states with their codecs and as a pure model of every entry of `KpfCommittee`, `KpfEpochTable` and
//! `KpfExit` (what the contract accepts and what it writes).
//!
//! | object | state | bytes |
//! |---|---|---|
//! | committee | `epochDaa, exitDaa, minMemberStake, quorumBps, spreadBps, minTotal, epoch tables, epochTableValue, from` (ints), the table, the holds, the judge block, `chargeBond, chargeDaa` | 1274 |
//! | epoch table | the table, `total, from, until, quorumBps, spreadBps` | 648 |
//! | exit | `ownerKey, amount, release`, the hold, the judge block, `chargeBond, chargeDaa` | 191 |
//! | charge | `status, clock, committeeId, stakeKey, payKey, market, t0, price, atStake`, the judge block, `chargeBond, chargeDaa` | 284 |
//!
//! The table is one 600-byte push (`0x4d 0x58 0x02` + 15 x (x-only key 32, weight as an 8-byte script number)).
//! Fifteen members: a node admits at most 15 signature operations in an input's script and counts an epoch table's data
//! signatures, one a member (live run 3).

use secp256k1::{Keypair, Message, Secp256k1, XOnlyPublicKey};
use silverscript_abi::ArtifactValue;

use crate::error::{Error, Result};
use crate::judge::{JudgeMarket, JudgeParams};
use crate::state::{encode, sm8, to_args, Cursor, F, BPS, CSV_LIMIT, LOCK_TIME_THRESHOLD};

pub const MEMBERS: usize = 15;
pub const TABLE_LEN: usize = 600;
pub const COMMITTEE_STATE_LEN: usize = 1274;
pub const EPOCH_TABLE_STATE_LEN: usize = 648;
pub const EXIT_STATE_LEN: usize = 191;
pub const CHARGE_STATE_LEN: usize = 284;
/// How many charges made after a charge its witness can name (`DEPTH`).
pub const HOLD_DEPTH: usize = 8;
/// The holds of the members: one hash a member, zero for none.
pub type Holds = [[u8; 32]; MEMBERS];
pub const NO_HOLD: [u8; 32] = [0; 32];
pub const NO_HOLDS: Holds = [NO_HOLD; MEMBERS];
/// The ids added to a stake's holds after a charge, in order (zero: none).
pub type Later = [[u8; 32]; HOLD_DEPTH];
pub const NO_LATER: Later = [NO_HOLD; HOLD_DEPTH];
pub const CHARGE_PENDING: i64 = 1;
pub const CHARGE_CONTESTED: i64 = 2;
pub const CHARGE_UPHELD: i64 = 3;
pub const CHARGE_REFUTED: i64 = 4;
pub const CHARGE_LAPSED: i64 = 5;
/// What a signed price's digest starts with.
pub const PRICE_TAG: &[u8] = b"kpf-tick-1";
pub const KAS: i64 = 100_000_000;
pub const MEMBER_MAX: i64 = 250_000_000_000_000_000;
pub const TOTAL_MAX: i64 = 4_000_000_000_000_000_000;
pub const MAX_EPOCH_TABLES: i64 = 6;
pub const EPOCH_TABLE_MIN: i64 = 100_000_000;
pub const EPOCH_TABLE_MAX: i64 = 10_000_000_000;
pub const EPOCH_MIN: i64 = 100;
pub const SPREAD_MAX: i64 = 1_000;
/// The age `join`, `topUp` and `leave` need of the committee UTXO (`KpfCommittee.REST_DAA`): a roll needs none.
pub const REST_DAA: u64 = 20;
/// A slashed stake: the sender of the proof keeps one part in `REWARD_DIV`, the rest is burned.
pub const REWARD_DIV: i64 = 10;
/// A burn below this is not required (`BURN_DUST`).
pub const BURN_DUST: i64 = 2_000_000;
/// The largest price an epoch table attests (`KpfEpochTable.PRICE_LIMIT`).
pub const PRICE_LIMIT: i64 = 900_000_000_000_000;
pub const ROLL_MAX: i64 = 1_048_576;

/// Why the model refuses (a contract `require`); for messages only.
pub type Refusal = &'static str;

fn chk(ok: bool, why: Refusal) -> std::result::Result<(), Refusal> {
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

/// blake2b-256, the contracts' `blake2b` (the hash of a P2SH script public key).
pub fn blake2b(data: &[u8]) -> [u8; 32] {
    kaspa_txscript::pay_to_script_hash_script(data).script()[2..34].try_into().expect("P2SH script")
}

// ---------------------------------------------------------------- the table

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub key: [u8; 32],
    pub weight: i64,
}

impl Member {
    pub const EMPTY: Member = Member { key: [0; 32], weight: 0 };
    pub fn taken(&self) -> bool {
        self.weight > 0
    }
}

pub type Table = [Member; MEMBERS];

pub const EMPTY_TABLE: Table = [Member::EMPTY; MEMBERS];

pub fn table_bytes(t: &Table) -> Vec<u8> {
    let mut b = Vec::with_capacity(TABLE_LEN);
    for s in t {
        b.extend_from_slice(&s.key);
        b.extend_from_slice(&sm8(s.weight));
    }
    b
}

/// The table as its state push.
fn table_push(t: &Table) -> Vec<u8> {
    let mut b = vec![0x4d, 0x58, 0x02];
    b.extend_from_slice(&table_bytes(t));
    b
}

fn read_table(c: &mut Cursor) -> Result<Table> {
    let s = c.b.get(c.at..c.at + 3 + TABLE_LEN).ok_or_else(|| Error::Decode("state ends inside the member table".into()))?;
    if s[..3] != [0x4d, 0x58, 0x02] {
        return Err(Error::Decode(format!("byte {}: expected the 600-byte table push", c.at)));
    }
    let mut t = EMPTY_TABLE;
    for (i, member) in t.iter_mut().enumerate() {
        let e = &s[3 + 40 * i..3 + 40 * i + 40];
        member.key.copy_from_slice(&e[..32]);
        let mut m: [u8; 8] = e[32..40].try_into().expect("8 bytes");
        let neg = m[7] & 0x80 != 0;
        m[7] &= 0x7f;
        let mag = u64::from_le_bytes(m);
        if mag > i64::MAX as u64 || (neg && mag == 0) {
            return Err(Error::Decode(format!("member {i}: weight is not in canonical form")));
        }
        member.weight = if neg { -(mag as i64) } else { mag as i64 };
    }
    c.at += 3 + TABLE_LEN;
    Ok(t)
}

pub fn total_of(t: &Table) -> i64 {
    t.iter().map(|s| s.weight).sum()
}

// ---------------------------------------------------------------- holds and charges

/// The holds `h` with the charge `cid` added: `blake2b(h || cid)`.
pub fn hold_add(h: &[u8; 32], cid: &[u8; 32]) -> [u8; 32] {
    let mut b = h.to_vec();
    b.extend_from_slice(cid);
    blake2b(&b)
}

/// The holds `a` with the ids of `later` added in order (`fold`).
pub fn hold_fold(a: &[u8; 32], later: &Later) -> [u8; 32] {
    later.iter().filter(|x| **x != NO_HOLD).fold(*a, |h, x| hold_add(&h, x))
}

/// What shows `cid` to be among the holds made of `ids` (the charges of one stake that are not settled, in the order
/// they were made): the holds before it and the ids after it. `None`: it is not among them, or more than
/// [`HOLD_DEPTH`] charges were made after it (they must be settled first).
pub fn hold_witness(ids: &[[u8; 32]], cid: &[u8; 32]) -> Option<([u8; 32], Later)> {
    let at = ids.iter().position(|x| x == cid)?;
    let rest = &ids[at + 1..];
    if rest.len() > HOLD_DEPTH {
        return None;
    }
    let a = ids[..at].iter().fold(NO_HOLD, |h, x| hold_add(&h, x));
    let mut later = NO_LATER;
    later[..rest.len()].copy_from_slice(rest);
    Some((a, later))
}

/// The holds `h` without `cid`, which the witness shows to be among them (`held`).
pub fn hold_without(h: &[u8; 32], cid: &[u8; 32], a: &[u8; 32], later: &Later) -> std::result::Result<[u8; 32], Refusal> {
    chk(*cid != NO_HOLD, "the charge has no covenant id")?;
    chk(hold_fold(&hold_add(a, cid), later) == *h, "the charge is not among the stake's holds with this witness")?;
    Ok(hold_fold(a, later))
}

/// What a refuted charge takes of a stake `w` (`takeOf`, reviews C5 and C6): at most the NO total of its verdict.
/// When less than 1 KAS would be left: all of `w` if this is the stake's last hold, all but 1 KAS while it has
/// `others` (the other charges are settled against that).
pub fn take_of(w: i64, no: i64, others: bool) -> std::result::Result<i64, Refusal> {
    chk(no > 0, "the verdict has no NO total")?;
    let s = w.min(no);
    Ok(if w - s >= KAS {
        s
    } else if others {
        (w - KAS).max(0)
    } else {
        w
    })
}

/// The author's share of a slash that takes `s` (`rewardOf`): a tenth of the lesser of the stake as it was charged
/// and the NO total, and no more than `s`.
pub fn reward_of(s: i64, at_stake: i64, no: i64) -> std::result::Result<i64, Refusal> {
    chk(at_stake > 0, "the charge records no stake")?;
    Ok((at_stake.min(no) / REWARD_DIV).min(s))
}

/// What a held stake `w` keeps when it is shown two prices (`keptOf`, review C5): a tenth, at least 1 KAS.
pub fn kept_of(w: i64) -> std::result::Result<i64, Refusal> {
    let rem = (w / REWARD_DIV).max(KAS);
    chk(w > rem, "a held stake of 1 KAS or less keeps all of it until its charges are settled")?;
    Ok(rem)
}

pub fn later_bytes(l: &Later) -> Vec<u8> {
    l.concat()
}

/// What a charge on a stake of the committee costs, how long it waits for a contest, and who judges it (`judge`,
/// `chargeBond`, `chargeDaa`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeParams {
    pub judge: JudgeMarket,
    pub bond: i64,
    pub daa: i64,
}

impl ChargeParams {
    /// `checkCharge` (the judge block's own bounds are the judge's: [`ChargeParams::judge_ok`]).
    pub fn check(&self) -> std::result::Result<(), Refusal> {
        chk(self.bond >= KAS && self.bond <= MEMBER_MAX, "chargeBond out of range")?;
        chk(self.daa > 0 && self.daa < CSV_LIMIT, "chargeDaa out of range")
    }
    /// `KpfStake.checkJudge` of the judge block: a block that fails it gets no stake, so no charge is ever contested.
    pub fn judge_ok(&self) -> bool {
        self.judge.p.check().is_ok()
    }
    /// Testnet defaults: the judge's (a stake of 100 KAS aged 7 days, rounds of 12 hours), a bond of 10 KAS, an hour
    /// to contest.
    pub fn testnet() -> ChargeParams {
        ChargeParams { judge: JudgeMarket { id: blake2b(b"kpf committee judge 1"), p: JudgeParams::testnet() }, bond: 10 * KAS, daa: 36_000 }
    }
    /// The 107 bytes a committee, its exits and its charges share: the judge block as one push, `chargeBond`, `chargeDaa`.
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut b = vec![0x4c, 0x57];
        b.extend_from_slice(&self.judge.bytes());
        b.extend_from_slice(&encode(&[F::I(self.bond), F::I(self.daa)]));
        b
    }
    pub fn args(&self) -> Vec<ArtifactValue> {
        let mut a = vec![ArtifactValue::Bytes(self.judge.bytes())];
        a.extend(to_args(&[F::I(self.bond), F::I(self.daa)]));
        a
    }
    fn read(c: &mut Cursor) -> Result<ChargeParams> {
        let s = c.b.get(c.at..c.at + 89).ok_or_else(|| Error::Decode("state ends inside the judge block".into()))?;
        if s[..2] != [0x4c, 0x57] {
            return Err(Error::Decode(format!("byte {}: expected the 87-byte judge block push", c.at)));
        }
        let mut j = Cursor { b: &s[2..], at: 0 };
        let judge = JudgeMarket { id: j.b32()?, p: JudgeParams { mu_bps: j.int()?, age: j.int()?, unbond: j.int()?, round: j.int()?, burn_bps: j.int()?, min_stake: j.int()? } };
        j.done()?;
        c.at += 89;
        Ok(ChargeParams { judge, bond: c.int()?, daa: c.int()? })
    }
}

/// The build constants of `KpfCommittee` and `KpfExit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RConsts {
    pub epoch_table_tpl: [u8; 32],
    pub exit_tpl: [u8; 32],
    pub charge_tpl: [u8; 32],
    pub charge_pre: i64,
    pub charge_suf: i64,
}

impl RConsts {
    fn charge_args(&self) -> Vec<ArtifactValue> {
        let mut a = vec![ArtifactValue::Bytes(self.charge_tpl.to_vec())];
        a.extend(to_args(&[F::I(self.charge_pre), F::I(self.charge_suf)]));
        a
    }
}

/// The build constants of `KpfCharge`: the judge's template and entry tags (the oracle family's), the lengths of the
/// committee's and the exit's scripts and the tags of their `slash` and `clear`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeConsts {
    pub judge_tpl: [u8; 32],
    pub judge_pre: i64,
    pub judge_suf: i64,
    pub open_tag: [u8; 4],
    pub resolve_tag: [u8; 4],
    pub committee_len: i64,
    pub exit_len: i64,
    pub committee_slash: [u8; 4],
    pub committee_clear: [u8; 4],
    pub exit_slash: [u8; 4],
    pub exit_clear: [u8; 4],
}

/// A charge that a key signed a false price (`KpfCharge`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Charge {
    pub status: i64,
    /// Contested: the DAA score the charge was accepted at. Decided: the NO total of the verdict. Pending: 0.
    pub clock: i64,
    pub committee_id: [u8; 32],
    pub stake_key: [u8; 32],
    pub pay_key: [u8; 32],
    pub market: [u8; 32],
    pub t0: i64,
    pub price: i64,
    /// What the stake held when it was charged.
    pub at_stake: i64,
    pub p: ChargeParams,
}

impl Charge {
    /// The charge `KpfCommittee.charge` and `KpfExit.charge` write.
    pub fn new(committee_id: [u8; 32], stake_key: [u8; 32], pay_key: [u8; 32], market: [u8; 32], t0: i64, price: i64, at_stake: i64, p: ChargeParams) -> Charge {
        Charge { status: CHARGE_PENDING, clock: 0, committee_id, stake_key, pay_key, market, t0, price, at_stake, p }
    }
    fn fields(&self) -> Vec<F> {
        vec![F::I(self.status), F::I(self.clock), F::B(self.committee_id), F::B(self.stake_key), F::B(self.pay_key), F::B(self.market), F::I(self.t0), F::I(self.price), F::I(self.at_stake)]
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut b = encode(&self.fields());
        b.extend_from_slice(&self.p.state_bytes());
        b
    }
    pub fn ctor(&self, k: &ChargeConsts) -> Vec<ArtifactValue> {
        let by = |t: &[u8]| ArtifactValue::Bytes(t.to_vec());
        let mut a = vec![by(&k.judge_tpl)];
        a.extend(to_args(&[F::I(k.judge_pre), F::I(k.judge_suf)]));
        a.extend([by(&k.open_tag), by(&k.resolve_tag)]);
        a.extend(to_args(&[F::I(k.committee_len), F::I(k.exit_len)]));
        a.extend([by(&k.committee_slash), by(&k.committee_clear), by(&k.exit_slash), by(&k.exit_clear)]);
        a.extend(to_args(&self.fields()));
        a.extend(self.p.args());
        a
    }
    pub fn decode_state(b: &[u8]) -> Result<Charge> {
        let mut c = Cursor { b, at: 0 };
        let w = Charge {
            status: c.int()?,
            clock: c.int()?,
            committee_id: c.b32()?,
            stake_key: c.b32()?,
            pay_key: c.b32()?,
            market: c.b32()?,
            t0: c.int()?,
            price: c.int()?,
            at_stake: c.int()?,
            p: ChargeParams::read(&mut c)?,
        };
        c.done()?;
        Ok(w)
    }
    /// `KpfCharge.contest`, of a charge accepted at `born`.
    pub fn contested(&self, born: i64) -> std::result::Result<Charge, Refusal> {
        chk(self.status == CHARGE_PENDING, "only a pending charge is contested")?;
        self.p.check()?;
        Ok(Charge { status: CHARGE_CONTESTED, clock: born, ..*self })
    }
    /// `KpfCharge.decide`: the judge's leader, the DAA score the contest landed at, and the dispute's NO total.
    pub fn decided(&self, yes_leads: bool, landed: i64, no_total: i64) -> std::result::Result<Charge, Refusal> {
        chk(no_total >= 0, "a negative NO total")?;
        chk(self.status == CHARGE_CONTESTED, "only a contested charge is decided")?;
        let v = if landed >= self.clock + self.p.daa {
            CHARGE_LAPSED
        } else if yes_leads {
            CHARGE_UPHELD
        } else {
            CHARGE_REFUTED
        };
        Ok(Charge { status: v, clock: no_total, ..*self })
    }
    /// What this refuted charge takes of a stake `w` that has `others` (other holds) or not, and its author's share
    /// of that.
    pub fn takes(&self, w: i64, others: bool) -> std::result::Result<(i64, i64), Refusal> {
        chk(self.status == CHARGE_REFUTED, "only a refuted charge slashes")?;
        chk(w > 0, "nothing to slash")?;
        let s = take_of(w, self.clock, others)?;
        Ok((s, reward_of(s, self.at_stake, self.clock)?))
    }
    /// How a charge of this status, accepted at `born`, leaves its stake at a lock time of `lock`: `Some(true)` it is
    /// slashed, `Some(false)` the hold ends, `None` not yet.
    pub fn outcome(&self, born: i64, lock: i64) -> Option<bool> {
        match self.status {
            CHARGE_REFUTED => Some(true),
            CHARGE_UPHELD | CHARGE_LAPSED => Some(false),
            CHARGE_PENDING if lock >= born + self.p.daa => Some(false),
            _ => None,
        }
    }
    /// Where the bond goes when the hold ends: the charged key if the signed price was upheld, the burn script otherwise.
    pub fn bond_to_key(&self) -> bool {
        self.status == CHARGE_UPHELD
    }
}

// ---------------------------------------------------------------- the committee

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitteeParams {
    pub epoch: i64,
    pub exit: i64,
    pub min_member_stake: i64,
    pub quorum_bps: i64,
    /// How far a signed price may lie from the price it attests, in basis points of that price.
    pub spread_bps: i64,
    pub min_total: i64,
    pub epoch_tables: i64,
    pub epoch_table_value: i64,
    /// The judge of its charges and what a charge costs.
    pub charge: ChargeParams,
}

impl CommitteeParams {
    /// `KpfCommittee.checkParams`.
    pub fn check(&self) -> std::result::Result<(), Refusal> {
        chk(self.epoch >= EPOCH_MIN && self.epoch < CSV_LIMIT, "epochDaa out of range")?;
        chk(self.exit >= 0 && self.exit < CSV_LIMIT, "exitDaa out of range")?;
        chk(self.min_member_stake >= KAS && self.min_member_stake <= MEMBER_MAX, "minMemberStake out of range")?;
        chk(self.quorum_bps > BPS / 2 && self.quorum_bps <= BPS, "quorumBps must be above a half and at most all")?;
        chk(self.spread_bps >= 0 && self.spread_bps <= SPREAD_MAX, "spreadBps out of range")?;
        chk(self.min_total >= self.min_member_stake && self.min_total <= TOTAL_MAX, "minTotal out of range")?;
        chk(self.epoch_tables >= 1 && self.epoch_tables <= MAX_EPOCH_TABLES, "epoch tables out of range")?;
        chk(self.epoch_table_value >= EPOCH_TABLE_MIN && self.epoch_table_value <= EPOCH_TABLE_MAX, "epochTableValue out of range")?;
        self.charge.check()
    }

    /// The testnet defaults (docs/DESIGN-COMMITTEE.md 6): epochs of 5 minutes, exits of 2 hours, a quorum of 67 %, four
    /// epoch tables of 1 KAS (an output with a covenant id costs about 4 x 10^12 / value grams of storage mass, KIP-9: four
    /// epoch tables of 0.2 KAS are 800,000 grams, above the block limit of 500,000, and a roll that writes them is never mined).
    pub fn testnet() -> CommitteeParams {
        CommitteeParams { epoch: 3_000, exit: 72_000, min_member_stake: 10 * KAS, quorum_bps: 6_700, spread_bps: 10, min_total: 30 * KAS, epoch_tables: 4, epoch_table_value: KAS, charge: ChargeParams::testnet() }
    }

    fn fields(&self) -> Vec<F> {
        vec![F::I(self.epoch), F::I(self.exit), F::I(self.min_member_stake), F::I(self.quorum_bps), F::I(self.spread_bps), F::I(self.min_total), F::I(self.epoch_tables), F::I(self.epoch_table_value)]
    }

    /// The signed weight a price needs of `total`: `ceil(total x quorumBps / 10^4)`.
    pub fn quorum_of(&self, total: i64) -> i64 {
        quorum_of(total, self.quorum_bps)
    }
}

pub fn quorum_of(total: i64, quorum_bps: i64) -> i64 {
    ((total as i128 * quorum_bps as i128 + BPS as i128 - 1) / BPS as i128) as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Committee {
    pub p: CommitteeParams,
    pub from: i64,
    pub members: Table,
    /// The charges each member's stake is held for.
    pub holds: Holds,
}

/// The stake of a member on its way out (`KpfExit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    pub key: [u8; 32],
    pub amount: i64,
    pub release: i64,
    /// The charges this stake is held for (zero: none).
    pub hold: [u8; 32],
    /// The committee's judge block and charge parameters.
    pub charge: ChargeParams,
}

impl Exit {
    /// A stake on its way out that is held for nothing, under the committee parameters `p`.
    pub fn of(key: [u8; 32], amount: i64, release: i64, p: &CommitteeParams) -> Exit {
        Exit { key, amount, release, hold: NO_HOLD, charge: p.charge }
    }
    fn fields(&self) -> Vec<F> {
        vec![F::B(self.key), F::I(self.amount), F::I(self.release), F::B(self.hold)]
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut b = encode(&self.fields());
        b.extend_from_slice(&self.charge.state_bytes());
        b
    }
    /// Constructor arguments: the build constants (the charge's template), then the state.
    pub fn ctor(&self, k: &RConsts) -> Vec<ArtifactValue> {
        let mut a = k.charge_args();
        a.extend(to_args(&self.fields()));
        a.extend(self.charge.args());
        a
    }
    pub fn decode_state(b: &[u8]) -> Result<Exit> {
        let mut c = Cursor { b, at: 0 };
        let e = Exit { key: c.b32()?, amount: c.int()?, release: c.int()?, hold: c.b32()?, charge: ChargeParams::read(&mut c)? };
        c.done()?;
        Ok(e)
    }
    /// `KpfExit.charge`: this stake held for the charge `cid` too.
    pub fn charged(&self, cid: &[u8; 32]) -> std::result::Result<Exit, Refusal> {
        chk(self.amount > 0, "nothing to charge")?;
        self.charge.check()?;
        Ok(Exit { hold: hold_add(&self.hold, cid), ..*self })
    }
    /// `KpfExit.slash`: the refuted charge `ch` (of id `cid`) is among the holds. What is left of the exit (`None`:
    /// nothing), what leaves it, and the author's share of that.
    pub fn slash(&self, ch: &Charge, cid: &[u8; 32], a: &[u8; 32], later: &Later) -> std::result::Result<(Option<Exit>, i64, i64), Refusal> {
        let rest = hold_without(&self.hold, cid, a, later)?;
        let (s, reward) = ch.takes(self.amount, rest != NO_HOLD)?;
        Ok(((s < self.amount).then_some(Exit { amount: self.amount - s, hold: rest, ..*self }), s, reward))
    }
    /// What `KpfExit.forfeit` leaves of a held exit (`None`: the exit has no hold and nothing is left).
    pub fn forfeit_rest(&self) -> std::result::Result<Option<Exit>, Refusal> {
        if self.hold == NO_HOLD {
            return Ok(None);
        }
        Ok(Some(Exit { amount: kept_of(self.amount)?, ..*self }))
    }
    /// `KpfExit.clear`: the hold for the charge `cid` ends.
    pub fn clear(&self, cid: &[u8; 32], a: &[u8; 32], later: &Later) -> std::result::Result<Exit, Refusal> {
        Ok(Exit { hold: hold_without(&self.hold, cid, a, later)?, ..*self })
    }
    /// `KpfExit.forfeit`: what must be burned of this stake once its key is shown to have signed two prices for one
    /// signed price (0: no burn output is required). An exit without a hold is gone (a tenth to the sender); a held one keeps
    /// a tenth under its holds ([`Exit::forfeit_rest`]) and the sender gets nothing.
    pub fn forfeit(&self) -> std::result::Result<i64, Refusal> {
        chk(self.amount > 0, "nothing to forfeit")?;
        Ok(match self.forfeit_rest()? {
            Some(x) => self.amount - x.amount,
            None => burn_of(self.amount),
        })
    }
    /// `KpfExit.withdraw` at a lock time of `lock`.
    pub fn withdraw(&self, lock: i64) -> std::result::Result<i64, Refusal> {
        chk(self.release >= 0 && self.release < LOCK_TIME_THRESHOLD, "release out of range")?;
        chk(lock >= self.release, "not released yet")?;
        chk(self.amount > 0, "nothing to withdraw")?;
        chk(self.hold == NO_HOLD, "the stake is held for a charge")?;
        Ok(self.amount)
    }
}

impl Committee {
    /// The committee a lineage must begin as: every member empty.
    pub fn empty(p: CommitteeParams, from: i64) -> Committee {
        Committee { p, from, members: EMPTY_TABLE, holds: NO_HOLDS }
    }

    pub fn state_bytes(&self) -> Vec<u8> {
        let mut f = self.p.fields();
        f.push(F::I(self.from));
        let mut b = encode(&f);
        b.extend_from_slice(&table_push(&self.members));
        b.extend_from_slice(&[0x4d, 0xe0, 0x01]);
        b.extend_from_slice(&self.holds.concat());
        b.extend_from_slice(&self.p.charge.state_bytes());
        b
    }

    /// Constructor arguments: the build constants (the epoch table's, the exit's and the charge's templates), then the
    /// state.
    pub fn ctor(&self, k: &RConsts) -> Vec<ArtifactValue> {
        let mut f = self.p.fields();
        f.push(F::I(self.from));
        let mut a = vec![ArtifactValue::Bytes(k.epoch_table_tpl.to_vec()), ArtifactValue::Bytes(k.exit_tpl.to_vec())];
        a.extend(k.charge_args());
        a.extend(to_args(&f));
        a.push(ArtifactValue::Bytes(table_bytes(&self.members)));
        a.push(ArtifactValue::Bytes(self.holds.concat()));
        a.extend(self.p.charge.args());
        a
    }

    pub fn decode_state(b: &[u8]) -> Result<Committee> {
        let mut c = Cursor { b, at: 0 };
        let (epoch, exit, min_member_stake, quorum_bps, spread_bps, min_total, epoch_tables, epoch_table_value) = (c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?);
        let from = c.int()?;
        let members = read_table(&mut c)?;
        let hs = c.b.get(c.at..c.at + 483).ok_or_else(|| Error::Decode("state ends inside the holds".into()))?;
        if hs[..3] != [0x4d, 0xe0, 0x01] {
            return Err(Error::Decode(format!("byte {}: expected the 480-byte push of the holds", c.at)));
        }
        let mut holds = NO_HOLDS;
        for (i, h) in holds.iter_mut().enumerate() {
            h.copy_from_slice(&hs[3 + 32 * i..35 + 32 * i]);
        }
        c.at += 483;
        let charge = ChargeParams::read(&mut c)?;
        c.done()?;
        Ok(Committee { p: CommitteeParams { epoch, exit, min_member_stake, quorum_bps, spread_bps, min_total, epoch_tables, epoch_table_value, charge }, from, members, holds })
    }

    pub fn total(&self) -> i64 {
        total_of(&self.members)
    }

    /// Does a committee UTXO of `value` back every weight of this table (what every entry checks of the table it writes,
    /// review C2)? A committee that did not begin empty may not.
    pub fn backed(&self, value: i64) -> bool {
        self.members.iter().all(|s| s.weight >= 0 && s.weight <= MEMBER_MAX) && self.total() <= value
    }

    /// When a stake leaving in the current epoch is released: the end of the epoch plus the exit time.
    pub fn release(&self) -> i64 {
        self.from + self.p.epoch + self.p.exit
    }

    fn exit_of(&self, i: usize) -> std::result::Result<Exit, Refusal> {
        let rel = self.release();
        chk(rel >= 0 && rel < LOCK_TIME_THRESHOLD, "release out of range")?;
        Ok(Exit { key: self.members[i].key, amount: self.members[i].weight, release: rel, hold: self.holds[i], charge: self.p.charge })
    }

    /// The least a newcomer must bid for member i: `minMemberStake` for an empty member, a twentieth more than the holder's
    /// weight for a taken one. `None`: the member cannot be taken (another member is empty or lighter).
    pub fn least_bid(&self, i: usize) -> Option<i64> {
        let w = self.members[i].weight;
        if w == 0 {
            return Some(self.p.min_member_stake);
        }
        if self.members.iter().any(|s| s.weight < w) {
            return None;
        }
        Some((w + w / 20).max(self.p.min_member_stake))
    }

    /// `KpfCommittee.join`: the committee after it, the exit of an outbid holder, and the change of the committee's value.
    pub fn join(&self, i: usize, key: [u8; 32], amount: i64) -> std::result::Result<(Committee, Option<Exit>, i64), Refusal> {
        chk(i < MEMBERS, "no such member")?;
        chk(amount >= self.p.min_member_stake, "below minMemberStake")?;
        chk(amount <= MEMBER_MAX, "above the member limit")?;
        let w = self.members[i].weight;
        chk(w >= 0, "negative weight")?;
        let mut exit = None;
        let mut delta = amount;
        if w > 0 {
            chk(self.members.iter().all(|s| s.weight >= w), "another member is empty or lighter")?;
            chk(amount >= w + w / 20, "the bid is not a twentieth above the holder's stake")?;
            exit = Some(self.exit_of(i)?);
            delta -= w;
        }
        let mut r = *self;
        r.members[i] = Member { key, weight: amount };
        r.holds[i] = NO_HOLD;
        Ok((r, exit, delta))
    }

    /// `KpfCommittee.topUp`.
    pub fn top_up(&self, i: usize, amount: i64) -> std::result::Result<Committee, Refusal> {
        chk(i < MEMBERS, "no such member")?;
        chk(amount >= KAS, "a top-up is at least 1 KAS")?;
        let w = self.members[i].weight;
        chk(w > 0, "the member is empty")?;
        chk(amount <= MEMBER_MAX - w, "above the member limit")?;
        let mut r = *self;
        r.members[i].weight = w + amount;
        Ok(r)
    }

    /// `KpfCommittee.leave` (the member's key signs).
    pub fn leave(&self, i: usize) -> std::result::Result<(Committee, Exit), Refusal> {
        chk(i < MEMBERS, "no such member")?;
        chk(self.members[i].weight > 0, "the member is empty")?;
        let exit = self.exit_of(i)?;
        let mut r = *self;
        r.members[i] = Member::EMPTY;
        r.holds[i] = NO_HOLD;
        Ok((r, exit))
    }

    /// `KpfCommittee.forfeit`: member i signed two prices for one time. The committee with the member empty, the stake that
    /// leaves it, and what of it must be burned (0: no burn output is required).
    pub fn forfeit(&self, i: usize) -> std::result::Result<(Committee, i64, i64), Refusal> {
        chk(i < MEMBERS, "no such member")?;
        let w = self.members[i].weight;
        chk(w > 0, "the member is empty")?;
        let mut r = *self;
        if self.holds[i] != NO_HOLD {
            // Review C5 (C5-F2): a held member keeps a tenth under its holds; all that leaves is burned.
            let rem = kept_of(w)?;
            r.members[i].weight = rem;
            return Ok((r, w - rem, w - rem));
        }
        r.members[i] = Member::EMPTY;
        Ok((r, w, burn_of(w)))
    }

    /// `KpfCommittee.charge`: member i's stake held for the charge `cid` too.
    pub fn charged(&self, i: usize, cid: &[u8; 32]) -> std::result::Result<Committee, Refusal> {
        self.p.charge.check()?;
        chk(i < MEMBERS, "no such member")?;
        chk(self.members[i].weight > 0, "the member is empty")?;
        let mut r = *self;
        r.holds[i] = hold_add(&self.holds[i], cid);
        Ok(r)
    }

    /// `KpfCommittee.slash`: the refuted charge `ch` (of id `cid`) is among member i's holds. The committee after it, the
    /// stake that leaves the member, and the author's share of that (the rest is burned). A member that keeps stake keeps
    /// its other holds.
    pub fn slash(&self, i: usize, ch: &Charge, cid: &[u8; 32], a: &[u8; 32], later: &Later) -> std::result::Result<(Committee, i64, i64), Refusal> {
        chk(i < MEMBERS, "no such member")?;
        let rest = hold_without(&self.holds[i], cid, a, later)?;
        let w = self.members[i].weight;
        chk(w > 0, "the member is empty")?;
        let (s, reward) = ch.takes(w, rest != NO_HOLD)?;
        let mut r = *self;
        if s == w {
            r.members[i] = Member::EMPTY;
            r.holds[i] = NO_HOLD;
        } else {
            r.members[i].weight = w - s;
            r.holds[i] = rest;
        }
        Ok((r, s, reward))
    }

    /// `KpfCommittee.clear`: the hold of member i for the charge `cid` ends.
    pub fn clear(&self, i: usize, cid: &[u8; 32], a: &[u8; 32], later: &Later) -> std::result::Result<Committee, Refusal> {
        chk(i < MEMBERS, "no such member")?;
        let mut r = *self;
        r.holds[i] = hold_without(&self.holds[i], cid, a, later)?;
        Ok(r)
    }

    /// The least `m` a roll at a present of `now` can take the epoch to, so that the new epoch holds `now`; `None`
    /// while the current epoch has not ended.
    pub fn roll_steps(&self, now: i64) -> Option<i64> {
        let m = (now - self.from) / self.p.epoch;
        (m >= 1).then_some(m)
    }

    /// `KpfCommittee.roll` at a lock time of `lock`, from a committee UTXO of `value`: the committee and the epoch table it writes
    /// `epoch tables` copies of.
    pub fn roll(&self, m: i64, lock: i64, value: i64) -> std::result::Result<(Committee, EpochTable), Refusal> {
        self.p.check()?;
        chk(m >= 1 && m <= ROLL_MAX, "m out of range")?;
        let nf = self.from + m * self.p.epoch;
        chk(nf >= 0 && nf + self.p.epoch < LOCK_TIME_THRESHOLD, "epoch out of range")?;
        chk(lock >= nf, "the new epoch has not begun")?;
        chk(self.members.iter().all(|s| s.weight >= 0 && s.weight <= MEMBER_MAX), "a weight out of range")?;
        let total = self.total();
        chk(total >= self.p.min_total, "the total stake is below minTotal")?;
        chk(total <= value, "the committee's value does not back its members")?;
        let mut r = *self;
        r.from = nf;
        Ok((r, EpochTable { members: self.members, total, from: nf, until: nf + self.p.epoch, quorum_bps: self.p.quorum_bps, spread_bps: self.p.spread_bps }))
    }
}

// ---------------------------------------------------------------- epoch tables, signed prices, attestations

/// One epoch's copy of the table (`KpfEpochTable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpochTable {
    pub members: Table,
    pub total: i64,
    pub from: i64,
    pub until: i64,
    pub quorum_bps: i64,
    pub spread_bps: i64,
}

impl EpochTable {
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut b = table_push(&self.members);
        b.extend_from_slice(&encode(&[F::I(self.total), F::I(self.from), F::I(self.until), F::I(self.quorum_bps), F::I(self.spread_bps)]));
        b
    }
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = vec![ArtifactValue::Bytes(table_bytes(&self.members))];
        a.extend(to_args(&[F::I(self.total), F::I(self.from), F::I(self.until), F::I(self.quorum_bps), F::I(self.spread_bps)]));
        a
    }
    pub fn decode_state(b: &[u8]) -> Result<EpochTable> {
        let mut c = Cursor { b, at: 0 };
        let members = read_table(&mut c)?;
        let t = EpochTable { members, total: c.int()?, from: c.int()?, until: c.int()?, quorum_bps: c.int()?, spread_bps: c.int()? };
        c.done()?;
        Ok(t)
    }
    /// Does the epoch table serve signed prices at `t0`?
    pub fn covers(&self, t0: i64) -> bool {
        t0 >= self.from && t0 < self.until
    }
    /// The signed weight a price needs.
    pub fn quorum(&self) -> i64 {
        quorum_of(self.total, self.quorum_bps)
    }
    /// Does a signed price lie within the spread of `p` (`|price - p| x 10^4 <= p x spreadBps`)?
    pub fn near(&self, price: i64, p: i64) -> bool {
        (price as i128 - p as i128).abs() * BPS as i128 <= p as i128 * self.spread_bps as i128
    }
    /// From when anyone may retire the epoch table.
    pub fn retire_from(&self) -> i64 {
        self.until + (self.until - self.from)
    }
}

/// What of a slashed stake `w` must be burned: all but a tenth, nothing when that is below the dust.
pub fn burn_of(w: i64) -> i64 {
    let b = w - w / REWARD_DIV;
    if b >= BURN_DUST {
        b
    } else {
        0
    }
}

/// Two signed signed prices that are proof against their member: one key, one time, two different prices, both signatures valid.
pub fn equivocation(a: &SignedPrice, b: &SignedPrice, committee_id: &[u8; 32], market: &[u8; 32]) -> bool {
    a.key == b.key && a.t0 == b.t0 && a.price != b.price && price_verifies(a, committee_id, market) && price_verifies(b, committee_id, market)
}

/// What a member signs for a signed price: `blake2b("kpf-tick-1" || committee id || market id || t0 || price)`.
pub fn price_digest(committee_id: &[u8; 32], market: &[u8; 32], t0: i64, price: i64) -> [u8; 32] {
    let mut m = PRICE_TAG.to_vec();
    m.extend_from_slice(committee_id);
    m.extend_from_slice(market);
    m.extend_from_slice(&sm8(t0));
    m.extend_from_slice(&sm8(price));
    blake2b(&m)
}

/// One member's signed price for a signed price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedPrice {
    pub key: [u8; 32],
    pub t0: i64,
    pub price: i64,
    pub sig: [u8; 64],
}

/// Signs a signed price (deterministic Schnorr, no auxiliary randomness; a data signature has no sighash byte).
pub fn sign_price(kp: &Keypair, committee_id: &[u8; 32], market: &[u8; 32], t0: i64, price: i64) -> SignedPrice {
    let d = price_digest(committee_id, market, t0, price);
    let sig = Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(d), kp);
    SignedPrice { key: kp.x_only_public_key().0.serialize(), t0, price, sig: *sig.as_ref() }
}

/// Is the signature the key's for this signed price?
pub fn price_verifies(t: &SignedPrice, committee_id: &[u8; 32], market: &[u8; 32]) -> bool {
    let Ok(key) = XOnlyPublicKey::from_slice(&t.key) else { return false };
    let Ok(sig) = secp256k1::schnorr::Signature::from_slice(&t.sig) else { return false };
    Secp256k1::new().verify_schnorr(&sig, &Message::from_digest(price_digest(committee_id, market, t.t0, t.price)), &key).is_ok()
}

/// The arguments of `KpfEpochTable.attest`: a price for a signed price and the signed prices that make it a median.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attestation {
    pub market: [u8; 32],
    pub t0: i64,
    pub p: i64,
    pub bits: i64,
    pub prices: [i64; MEMBERS],
    pub sigs: [[u8; 64]; MEMBERS],
}

impl Attestation {
    /// The attestation of a signed price from signed prices. `p` is a price near which the most weight signed (the middle one
    /// of the candidates: each signed price and the two ends of the band it allows), and every member of the epoch table with a
    /// valid signed price for `t0` within the spread of `p` is included (the first one given per member). Refused when the epoch table does not cover `t0` or no price has a quorum of
    /// the weight within the spread of it.
    pub fn collect(epoch_table: &EpochTable, committee_id: &[u8; 32], market: &[u8; 32], t0: i64, signed_prices: &[SignedPrice]) -> std::result::Result<Attestation, Refusal> {
        chk(epoch_table.covers(t0), "the epoch table does not cover this signed price")?;
        let mut signed: Vec<(usize, &SignedPrice)> = vec![];
        for (i, member) in epoch_table.members.iter().enumerate() {
            if !member.taken() {
                continue;
            }
            if let Some(t) = signed_prices.iter().find(|t| t.key == member.key && t.t0 == t0 && t.price >= 0 && t.price <= PRICE_LIMIT && price_verifies(t, committee_id, market)) {
                signed.push((i, t));
            }
        }
        let weight_near = |p: i64| -> i64 { signed.iter().filter(|(_, t)| epoch_table.near(t.price, p)).map(|(i, _)| epoch_table.members[*i].weight).sum() };
        // A price x is within the spread of p for p in [x B / (B + s), x B / (B - s)]: the weight near p changes only
        // at those ends, so they and the signed prices are every candidate worth trying (review C2, C2-F6: with the
        // signed prices alone, members at 10.00 and 10.08 were attested by the epoch table at 10.04 and by this function not
        // at all).
        let (b, sp) = (BPS as i128, epoch_table.spread_bps.clamp(0, BPS - 1) as i128);
        let mut prices: Vec<i64> = vec![];
        for (_, t) in &signed {
            let x = t.price as i128;
            for c in [x, (x * b + b + sp - 1) / (b + sp), x * b / (b - sp)] {
                if c >= 0 && c <= PRICE_LIMIT as i128 {
                    prices.push(c as i64);
                }
            }
        }
        prices.sort();
        prices.dedup();
        let most = prices.iter().map(|p| weight_near(*p)).max().ok_or("no member of the epoch table signed this signed price")?;
        let at_most: Vec<i64> = prices.into_iter().filter(|p| weight_near(*p) == most).collect();
        let (p, w) = (at_most[at_most.len() / 2], most);
        chk(w >= epoch_table.quorum(), "the weight that signed prices within the spread of one price is below the quorum")?;
        Ok(Attestation::of(epoch_table, market, t0, p, &signed.iter().filter(|(_, t)| epoch_table.near(t.price, p)).map(|(i, t)| (*i, **t)).collect::<Vec<_>>()))
    }

    /// An attestation of `p` that presents exactly the given members' signed prices, whether or not the epoch table accepts it.
    pub fn of(_epoch_table: &EpochTable, market: &[u8; 32], t0: i64, p: i64, members: &[(usize, SignedPrice)]) -> Attestation {
        let mut a = Attestation { market: *market, t0, p, bits: 0, prices: [0; MEMBERS], sigs: [[0; 64]; MEMBERS] };
        for (i, t) in members {
            a.bits |= 1 << i;
            a.prices[*i] = t.price;
            a.sigs[*i] = t.sig;
        }
        a
    }

    /// The same signatures with another price claimed.
    pub fn claiming(mut self, p: i64) -> Attestation {
        self.p = p;
        self
    }

    /// The six fixed-width arguments of `attest`: market, t0, p, bits, prices (15 x 8), sigs (15 x 64).
    pub fn args(&self) -> Vec<ArtifactValue> {
        let mut prices = Vec::with_capacity(8 * MEMBERS);
        let mut sigs = Vec::with_capacity(64 * MEMBERS);
        for i in 0..MEMBERS {
            prices.extend_from_slice(&sm8(self.prices[i]));
            sigs.extend_from_slice(&self.sigs[i]);
        }
        let b = ArtifactValue::Bytes;
        vec![b(self.market.to_vec()), b(sm8(self.t0).to_vec()), b(sm8(self.p).to_vec()), b(sm8(self.bits).to_vec()), b(prices), b(sigs)]
    }

    /// `KpfEpochTable.attest` at a lock time of `lock`: the signed weight, or why the epoch table refuses.
    pub fn check(&self, epoch_table: &EpochTable, committee_id: &[u8; 32], lock: i64) -> std::result::Result<i64, Refusal> {
        chk(epoch_table.covers(self.t0), "the signed price is outside the epoch table's window")?;
        chk(self.t0 >= 0 && self.t0 < LOCK_TIME_THRESHOLD, "t0 out of range")?;
        chk(lock >= self.t0, "a signed price of the future")?;
        chk(self.p >= 0 && self.p <= PRICE_LIMIT, "price out of range")?;
        chk(epoch_table.spread_bps >= 0 && epoch_table.spread_bps <= BPS, "spreadBps out of range")?;
        chk(epoch_table.total > 0 && epoch_table.total <= TOTAL_MAX, "total out of range")?;
        chk(self.bits >= 0 && self.bits < 1 << MEMBERS, "bits beyond the members")?;
        let mut w = 0i64;
        for i in 0..MEMBERS {
            if self.bits >> i & 1 == 0 {
                continue;
            }
            let member = &epoch_table.members[i];
            chk(member.weight > 0, "an empty member signed")?;
            let t = SignedPrice { key: member.key, t0: self.t0, price: self.prices[i], sig: self.sigs[i] };
            chk(price_verifies(&t, committee_id, &self.market), "a signature does not verify")?;
            chk((self.prices[i] as i128 - self.p as i128).abs() <= PRICE_LIMIT as i128 && epoch_table.near(self.prices[i], self.p), "a signed price lies outside the spread of the price claimed")?;
            w += member.weight;
            chk(w <= epoch_table.total, "more weight than the total")?;
        }
        chk(w >= epoch_table.quorum(), "below the quorum")?;
        Ok(w)
    }
}
