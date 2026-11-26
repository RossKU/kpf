//! The thin family (docs/DESIGN-COMMITTEE.md 8a): the model of its objects. An object on the chain is `KpfSlot` with
//! the hash of its state; the state itself is held by whoever acts on it and shown to the script of the operation.
//! This module says what the states are, how they hash, and what each operation pays, with the scripts' integer
//! arithmetic.
//!
//! - An [`Position`] is the cross-margin position of the committee family ([`Cross`]: terms, two legs, two keys, two
//!   margins, a lease) with the two values of the declared form: the dispute period and the bond rate. Its state is
//!   two parts: the *fixed* part (terms and legs, 312 bytes), shown only to the script that computes, and the *moving*
//!   part (keys, margins, lease, `lateDaa`, dispute period, bond rate, what each party asks of the other's margin: 138 bytes), shown to every script. The hash is
//!   `blake2b(push(blake2b(fixed)) + moving)`.
//! - A [`Claim`] is a position whose owner or counterparty declared what it is owed at the prices of a signed price, with a
//!   bond: `blake2b(push(position) + side, t0, a1, a2, amount, bond)`.
//! - A [`Disputed`] claim names who asked for its proof: `blake2b(push(claim) + push(disputer))`.

use crate::committee::blake2b;
use crate::cross::{Cross, CROSS_STATE_LEN, MARGIN_MAX, SIDE_A, SIDE_B};
use crate::error::{Error, Result};
use crate::state::{sm8, Cursor, BPS, LOCK_TIME_THRESHOLD};

/// The fixed part of a position's state: the terms and the two legs.
pub const FIXED_LEN: usize = 312;
/// The moving part: two keys, two margins, the lease, `lateDaa`, the dispute period, the bond rate.
pub const MOVING_LEN: usize = 138;
/// The longest dispute period (DAA): a day at ten blocks a second.
pub const WINDOW_MAX: i64 = 864_000;

fn p33(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], h].concat()
}

fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)].concat()
}

/// A position of the thin family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// Terms, legs, keys, margins and lease, as the committee family's cross-margin position.
    pub x: Cross,
    /// The DAA a claim waits for a dispute, and a disputed claim for its proof.
    pub dispute_daa: i64,
    /// The bond of a claim and of a dispute, in basis points of the pot.
    pub bond_bps: i64,
    /// The least margin A asked of its counterparty for this position (its order's `ask`, its share): whoever takes
    /// B's place posts as much. 0: nothing asked (a position the two opened directly).
    pub a_min_counter: i64,
    /// The same, B's of A.
    pub b_min_counter: i64,
}

impl Position {
    /// Refuses a position the scripts would misread or that could not be settled.
    pub fn check(&self) -> Result<()> {
        self.x.check()?;
        if !(1..=WINDOW_MAX).contains(&self.dispute_daa) {
            return Err(Error::Invalid(format!("the dispute period must be 1 to {WINDOW_MAX} DAA")));
        }
        if !(1..=crate::cmarket::TERM_MAX).contains(&self.x.expiry) {
            return Err(Error::Invalid("the exit time (the wait of an exit claim) must be 1 DAA to the longest term".into()));
        }
        if !(1..=BPS).contains(&self.bond_bps) {
            return Err(Error::Invalid("the bond rate must be 1 to 10000 basis points of the pot".into()));
        }
        if self.bond() <= 0 {
            return Err(Error::Invalid("the pot is too small for a bond: a claim on this position could not be disputed".into()));
        }
        if !(0..=MARGIN_MAX).contains(&self.a_min_counter) || !(0..=MARGIN_MAX).contains(&self.b_min_counter) {
            return Err(Error::Invalid("what a party asks of the other's margin is out of range".into()));
        }
        Ok(())
    }

    /// The position of a fixed and a moving part (what an opening publishes), checked.
    pub fn from_parts(fixed: &[u8], moving: &[u8]) -> Result<Position> {
        if fixed.len() != FIXED_LEN || moving.len() != MOVING_LEN {
            return Err(Error::Decode("a position's state is a fixed part of 312 bytes and a moving part of 138".into()));
        }
        let own = CROSS_STATE_LEN - FIXED_LEN;
        let x = Cross::decode_state(&[fixed, &moving[..own]].concat())?;
        let mut c = Cursor { b: &moving[own..], at: 0 };
        let (late, dispute_daa, bond_bps, a_min_counter, b_min_counter) = (c.int()?, c.int()?, c.int()?, c.int()?, c.int()?);
        if late != x.t.late_daa {
            return Err(Error::Decode("the moving part's lateDaa is not the terms'".into()));
        }
        let a = Position { x, dispute_daa, bond_bps, a_min_counter, b_min_counter };
        a.check()?;
        Ok(a)
    }

    pub fn fixed(&self) -> Vec<u8> {
        let s = self.x.state_bytes();
        debug_assert_eq!(s.len(), CROSS_STATE_LEN);
        s[..FIXED_LEN].to_vec()
    }

    pub fn fixed_hash(&self) -> [u8; 32] {
        blake2b(&self.fixed())
    }

    pub fn moving(&self) -> Vec<u8> {
        let s = self.x.state_bytes();
        [&s[FIXED_LEN..], &i9(self.x.t.late_daa)[..], &i9(self.dispute_daa), &i9(self.bond_bps), &i9(self.a_min_counter), &i9(self.b_min_counter)].concat()
    }

    /// The hash a position's object carries.
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.fixed_hash()), self.moving()].concat())
    }

    pub fn pot(&self) -> i64 {
        self.x.pot()
    }

    /// The longest epoch-independent wait a claim's proof can come after its time: the claim lands within `lateDaa`,
    /// is disputed within a dispute period, and proved within another. An epoch table serves a signed price of its epoch for one epoch more
    /// after the epoch ends, so a claim is provable to its end only if this fits (review T1).
    pub fn proof_span(&self) -> i64 {
        self.x.t.late_daa + 2 * self.dispute_daa
    }

    /// The bond of a claim on this position, and of its dispute (`KpfTClose`'s arithmetic).
    pub fn bond(&self) -> i64 {
        let pot = self.pot();
        (pot / BPS) * self.bond_bps + ((pot % BPS) * self.bond_bps) / BPS
    }

    pub fn key_of(&self, side: i64) -> Option<[u8; 32]> {
        match side {
            SIDE_A => Some(self.x.a_key),
            SIDE_B => Some(self.x.b_key),
            _ => None,
        }
    }

    /// `KpfTAdd`: the position after the owner of `side` added `delta` to its margin.
    pub fn added(&self, side: i64, delta: i64) -> Option<Position> {
        if delta <= 0 || delta > MARGIN_MAX {
            return None;
        }
        let mut x = self.x;
        let m = if side == SIDE_A { &mut x.a_margin } else if side == SIDE_B { &mut x.b_margin } else { return None };
        *m = m.checked_add(delta).filter(|v| *v <= MARGIN_MAX)?;
        Some(Position { x, ..*self })
    }

    /// What the rule of a close gives `side` at the prices `a` (the arithmetic `KpfTProve` runs), if a close is
    /// allowed there.
    pub fn owed(&self, side: i64, a: [i64; 2]) -> Option<i64> {
        self.x.close_pay(side, a)
    }

    /// The claim `side` makes at the prices `a` of the time `t0`, stating `amount` (true or not: the script of the
    /// declaration computes nothing).
    pub fn claim(&self, side: i64, t0: i64, a: [i64; 2], amount: i64) -> Option<Claim> {
        let ok = self.key_of(side).is_some() && (0..=self.pot()).contains(&amount) && (0..LOCK_TIME_THRESHOLD).contains(&t0) && a[0] >= 0 && a[1] >= 0 && self.bond() > 0;
        ok.then_some(Claim { position: *self, side, t0, a, amount, bond: self.bond() })
    }

    /// A liquidation of `side` as stated (true or not: the declaration computes nothing), within what its script takes.
    pub fn liq_stated(&self, side: i64, t0: i64, a: [i64; 2], fee: i64, add: i64, liquidator: [u8; 32]) -> Option<Liq> {
        let ok = self.key_of(side).is_some() && (0..LOCK_TIME_THRESHOLD).contains(&t0) && a[0] >= 0 && a[1] >= 0 && (0..=self.pot()).contains(&fee) && add > 0 && add <= MARGIN_MAX && self.bond() > 0;
        ok.then_some(Liq { position: *self, side, t0, a, fee, add, bond: self.bond(), liquidator })
    }

    /// The liquidation of `side` the rule gives at the prices `a` (the arithmetic of `KpfCrossC`'s liquidation): the
    /// side is below its maintenance even at the prices moved toward it and the other party is not; the fee is the
    /// keeper's, at most what the side is still owed; the cure adds what puts it at the maintenance at the prices
    /// moved against it.
    pub fn liquidation(&self, side: i64, t0: i64, a: [i64; 2], liquidator: [u8; 32]) -> Option<Liq> {
        if self.key_of(side).is_none() || !self.x.liquidatable(side, a) {
            return None;
        }
        self.liq_stated(side, t0, a, self.x.fee_of(side, a), self.x.cure_add(side, a), liquidator)
    }

    /// The time a "no price" claim on this position waits (DAA). The thin family reads the position's `expiry` field as
    /// this duration: it has no lease.
    pub fn exit_daa(&self) -> i64 {
        self.x.expiry
    }

    /// `KpfTExit`: the claim of `side` that the committee signed no price of one of the position's markets since the
    /// DAA `since`.
    pub fn exit(&self, side: i64, since: i64) -> Option<ExitClaim> {
        (self.key_of(side).is_some() && (0..LOCK_TIME_THRESHOLD).contains(&since) && self.bond() > 0).then_some(ExitClaim { position: *self, side, since, bond: self.bond() })
    }

    /// The liquidation of `side` a liquidator states from prices `a` of its own, so that it is within the rule
    /// (`Liq::within`) wherever the committee's prices are no further than the tolerance g from them: the side is
    /// liquidated even at its prices moved by g toward it; the fee is the least the rule gives over that span; the
    /// cure is the rule's at its prices as they are.
    pub fn liquidation_own(&self, side: i64, t0: i64, a: [i64; 2], liquidator: [u8; 32]) -> Option<Liq> {
        self.key_of(side)?;
        let x = &self.x;
        let (p, toward) = (x.counted(a), side == SIDE_A);
        let (near, far) = ([x.moved(0, p[0], toward), x.moved(1, p[1], toward)], [x.moved(0, p[0], !toward), x.moved(1, p[1], !toward)]);
        if !x.liquidatable(side, near) {
            return None;
        }
        let fee = [[near[0], near[1]], [near[0], far[1]], [far[0], near[1]], [far[0], far[1]]].iter().map(|q| x.fee_of(side, *q)).min()?;
        self.liq_stated(side, t0, a, fee, x.cure_at(side, a, None), liquidator).filter(|l| l.within(a))
    }

    /// The claim a party makes if it states what the rule gives.
    pub fn true_claim(&self, side: i64, t0: i64, a: [i64; 2]) -> Option<Claim> {
        self.claim(side, t0, a, self.owed(side, a)?)
    }
}

/// A declared claim: the position it is on and what was stated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    pub position: Position,
    pub side: i64,
    pub t0: i64,
    pub a: [i64; 2],
    pub amount: i64,
    pub bond: i64,
}

impl Claim {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.position.hash()), i9(self.side), i9(self.t0), i9(self.a[0]), i9(self.a[1]), i9(self.amount), i9(self.bond)].concat())
    }

    /// Whether the amount stated is the rule's: what a proof can show and a dispute tests.
    pub fn is_true(&self) -> bool {
        self.position.owed(self.side, self.a) == Some(self.amount)
    }

    /// Whether the claim is within the rule at the prices `c` the committee signed for its time (`KpfTProve`): a
    /// close is allowed there, and the amount is at most what the side is owed at those prices as they are. A
    /// disputed claim is proved if so and refuted if not: the two are each other's complement. (The prices a claim
    /// states are its claimer's own, and no script reads them: `true_claim` counts them the tolerance g worse for the
    /// claimer, which leaves that much between its own price and the committee's.)
    pub fn within(&self, c: [i64; 2]) -> bool {
        let x = &self.position.x;
        x.close_pay(self.side, c).is_some() && (0..=x.owed_at(self.side, c)).contains(&self.amount)
    }

    /// Whether a claim that landed at `landed` is in time (at its time or within `lateDaa` after it): one that is not
    /// (late, or stating a signed price still to come) is neither settled nor disputed, only ended (`KpfTLate`).
    pub fn in_time(&self, landed: u64) -> bool {
        (0..=self.position.x.t.late_daa).contains(&(landed as i64 - self.t0))
    }

    /// (to A, to B) of a claim's object holding `v` when it takes `mine` for the claimer.
    fn split(&self, v: i64, mine: i64) -> (i64, i64) {
        let mine = mine.min(v);
        if self.side == SIDE_A {
            (mine, v - mine)
        } else {
            (v - mine, mine)
        }
    }

    /// `KpfTSettle`: (to A, to B) of an undisputed claim holding `v`, once it is a dispute period old.
    pub fn settle_pay(&self, v: i64) -> (i64, i64) {
        self.split(v, self.amount + self.bond)
    }

    /// The party the claim was made against.
    pub fn other_key(&self) -> [u8; 32] {
        self.position.key_of(if self.side == SIDE_A { SIDE_B } else { SIDE_A }).expect("a side")
    }

    /// `KpfTLate`: (the position's value again, to who ends the claim, to the party it was made against) of a claim
    /// holding `v` that landed late or before its own time: its bond is shared (review T1b, F3).
    pub fn late_pay(&self, v: i64) -> Option<(i64, i64, i64)> {
        let half = self.bond / 2;
        (v >= self.bond).then_some((v - self.bond, self.bond - half, half))
    }

    /// `KpfTDispute.dispute`: the claim with `disputer`'s key, holding a second bond.
    pub fn disputed(&self, disputer: [u8; 32]) -> Disputed {
        Disputed { claim: *self, disputer }
    }
}

/// A claim somebody asked the proof of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disputed {
    pub claim: Claim,
    pub disputer: [u8; 32],
}

impl Disputed {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.claim.hash()), p33(&self.disputer)].concat())
    }

    /// `KpfTProve.prove`: (to A, to B, what stays in the script's copy) of a disputed claim holding `v` that was
    /// proved: the claimer has its amount, its bond and half of the disputer's; the other half is the committee's,
    /// which answered.
    pub fn prove_pay(&self, v: i64) -> (i64, i64, i64) {
        let kept = (self.claim.bond - self.claim.bond / 2).min(v);
        let (a, b) = self.claim.split(v - kept, self.claim.amount + 2 * self.claim.bond - kept);
        (a, b, kept)
    }

    /// `KpfTProve.answer` with the verdict 0: (the position's value again, to the disputer, to whoever shows the
    /// committee's price, what stays in the script's copy) of a disputed claim holding `v` that states more than the
    /// committee's prices give:
    /// the disputer has its bond back, and the claimer's bond is shared by the refuter and the committee.
    pub fn refute_pay(&self, v: i64) -> Option<(i64, i64, i64, i64)> {
        let (bond, half) = (self.claim.bond, self.claim.bond / 2);
        (v >= 2 * bond).then_some((v - 2 * bond, bond, half, bond - half))
    }

    /// `KpfTDispute.lapse`: (the position's value again, to the disputer, to the claimer) of a disputed claim holding
    /// `v` for or against which nobody showed the committee's price within the dispute period: nothing is decided,
    /// and each has its bond back.
    pub fn lapse_pay(&self, v: i64) -> Option<(i64, i64, i64)> {
        let bond = self.claim.bond;
        (v >= 2 * bond).then_some((v - 2 * bond, bond, bond))
    }

    /// The claimer's key.
    pub fn claimer_key(&self) -> [u8; 32] {
        self.claim.position.key_of(self.claim.side).expect("a side")
    }

    /// The key the other half is paid to.
    pub fn other_key(&self) -> [u8; 32] {
        if self.claim.side == SIDE_A {
            self.claim.position.x.b_key
        } else {
            self.claim.position.x.a_key
        }
    }
}

/// A declared liquidation: the position, the side liquidated, and what the liquidator stated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Liq {
    pub position: Position,
    /// The side liquidated.
    pub side: i64,
    pub t0: i64,
    pub a: [i64; 2],
    /// The liquidator's fee.
    pub fee: i64,
    /// What a cure adds to the side's margin.
    pub add: i64,
    pub bond: i64,
    pub liquidator: [u8; 32],
}

/// Where a liquidation claim stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiqState {
    Claimed,
    /// Somebody asked for its proof.
    Disputed([u8; 32]),
    /// Its liquidator proved it: it waits a dispute period again, and is not disputed twice.
    Proved,
}

impl Liq {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.position.hash()), i9(self.side), i9(self.t0), i9(self.a[0]), i9(self.a[1]), i9(self.fee), i9(self.add), i9(self.bond), p33(&self.liquidator)].concat())
    }

    /// Whether what was stated is the rule's at the prices stated.
    pub fn is_true(&self) -> bool {
        self.position.liquidation(self.side, self.t0, self.a, self.liquidator) == Some(*self)
    }

    /// Whether the liquidation is within the rule at the prices `c` the committee signed for its time
    /// (`KpfTLiqProve`): the side is liquidated there; the fee is at most the rule's; the cure is between what puts
    /// the side at the maintenance at the prices moved by g toward it (less would leave it liquidated again at once)
    /// and at the prices moved against it. A disputed liquidation is proved if so and refuted if not.
    pub fn within(&self, c: [i64; 2]) -> bool {
        let x = &self.position.x;
        x.liquidatable(self.side, c) && (0..=x.fee_of(self.side, c)).contains(&self.fee) && (x.cure_at(self.side, c, Some(true))..=x.cure_at(self.side, c, Some(false))).contains(&self.add)
    }

    pub fn in_time(&self, landed: u64) -> bool {
        (0..=self.position.x.t.late_daa).contains(&(landed as i64 - self.t0))
    }

    /// The side liquidated, and the other party.
    pub fn loser_key(&self) -> [u8; 32] {
        self.position.key_of(self.side).expect("a side")
    }

    pub fn winner_key(&self) -> [u8; 32] {
        self.position.key_of(if self.side == SIDE_A { SIDE_B } else { SIDE_A }).expect("a side")
    }

    /// `KpfTLiqSettle`: (to the liquidator, to the other party) of a claim holding `v` (the position and one bond).
    pub fn settle_pay(&self, v: i64) -> (i64, i64) {
        let mine = (self.fee + self.bond).min(v);
        (mine, v - mine)
    }

    /// `KpfTLiq.late`: (the position's value again, to who ends the claim, what stays in the script's copy) of a
    /// liquidation holding `v` that landed late or before its own time. The half no party receives is nobody's:
    /// a side can be its own liquidator under another key (review T1b, F2b).
    pub fn late_pay(&self, v: i64) -> Option<(i64, i64, i64)> {
        let half = self.bond / 2;
        (v >= self.bond).then_some((v - self.bond, self.bond - half, half))
    }

    /// `KpfTCure`: the position after the side added what the claim asks.
    pub fn cured(&self) -> Option<Position> {
        self.position.added(self.side, self.add)
    }
}

/// A liquidation claim on the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiqObj {
    pub liq: Liq,
    pub state: LiqState,
}

impl LiqObj {
    pub fn hash(&self) -> [u8; 32] {
        let h = self.liq.hash();
        match self.state {
            LiqState::Claimed => h,
            LiqState::Disputed(d) => blake2b(&[p33(&h), p33(&d)].concat()),
            LiqState::Proved => blake2b(&[i9(1), p33(&h)].concat()),
        }
    }

    /// The bonds the object holds beside the position: the liquidator's, and a disputer's while it is disputed.
    pub fn bonds(&self) -> i64 {
        self.liq.bond * if matches!(self.state, LiqState::Disputed(_)) { 2 } else { 1 }
    }

    /// The age `KpfTCure` asks of the claim: half a dispute period of one nobody disputed (it has stood open to a dispute
    /// that long: review T1b, F2), none of a proved one. A disputed claim is not cured.
    pub fn cure_age(&self) -> Option<i64> {
        match self.state {
            LiqState::Claimed => Some(self.liq.position.dispute_daa / 2),
            LiqState::Proved => Some(0),
            LiqState::Disputed(_) => None,
        }
    }

    /// The script's number for the state (`KpfTCure`): 0 a claim, 1 a disputed one, 2 a proved one.
    pub fn state_no(&self) -> i64 {
        match self.state {
            LiqState::Claimed => 0,
            LiqState::Disputed(_) => 1,
            LiqState::Proved => 2,
        }
    }

    /// `KpfTLiqProve.prove`: (to the liquidator, what stays in the script's copy) of the disputer's bond: half each,
    /// the copy's being the committee's, which answered.
    pub fn prove_pay(&self) -> (i64, i64) {
        let half = self.liq.bond / 2;
        (half, self.liq.bond - half)
    }

    /// `KpfTLiqProve.refute`: (the position's value again, to the disputer, to whoever shows the committee's prices,
    /// what stays in the script's copy) of a disputed liquidation holding `v` that is not within the rule.
    pub fn refute_pay(&self, v: i64) -> Option<(i64, i64, i64, i64)> {
        let (bond, half) = (self.liq.bond, self.liq.bond / 2);
        (matches!(self.state, LiqState::Disputed(_)) && v >= 2 * bond).then_some((v - 2 * bond, bond, half, bond - half))
    }

    /// `KpfTLiqDispute.lapse`: (the position's value again, to the disputer, to the liquidator) of a disputed
    /// liquidation holding `v` for or against which nobody showed the committee's prices within the dispute period:
    /// nothing is decided, and each has its bond back.
    pub fn lapse_pay(&self, v: i64) -> Option<(i64, i64, i64)> {
        let bond = self.liq.bond;
        (matches!(self.state, LiqState::Disputed(_)) && v >= 2 * bond).then_some((v - 2 * bond, bond, bond))
    }
}

/// A "no price" claim: a party says the committee stopped signing one of the position's markets at `since`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitClaim {
    pub position: Position,
    pub side: i64,
    pub since: i64,
    pub bond: i64,
}

impl ExitClaim {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(2), p33(&self.position.hash()), i9(self.side), i9(self.since), i9(self.bond)].concat())
    }

    /// Whether a claim that landed at `landed` speaks of the past: one whose `since` is after it is ended by anyone.
    pub fn of_the_past(&self, landed: u64) -> bool {
        self.since <= landed as i64
    }

    /// `KpfTExitEnd`: (to A, to B) of a claim holding `v` that nobody ended: each party its margin, the claimer its
    /// bond with it, and what else the position held with B's.
    pub fn settle_pay(&self, v: i64) -> (i64, i64) {
        let pa = (self.position.x.a_margin + if self.side == SIDE_A { self.bond } else { 0 }).min(v);
        (pa, v - pa)
    }

    /// `KpfTExitRefute`, `KpfTExit.early`: (the position's value again, to who ended the claim, to the party the claim
    /// was made against) of a claim holding `v`.
    pub fn end_pay(&self, v: i64) -> Option<(i64, i64, i64)> {
        let half = self.bond / 2;
        (v >= self.bond).then_some((v - self.bond, self.bond - half, half))
    }

    pub fn other_key(&self) -> [u8; 32] {
        self.position.key_of(if self.side == SIDE_A { SIDE_B } else { SIDE_A }).expect("a side")
    }
}

/// An object of the family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TObj {
    Position(Position),
    Claim(Claim),
    Disputed(Disputed),
    Liq(LiqObj),
    ExitClaim(ExitClaim),
    /// A stated change of a position (`KpfTChange`), disputed or not.
    Change(Change),
    Order(Order),
    /// An anchor of the library's registry: the way in (`KpfTPlace`). Its state is nothing but its kind.
    Anchor,
}

impl TObj {
    /// The hash the object's script carries.
    pub fn hash(&self) -> [u8; 32] {
        match self {
            TObj::Position(a) => a.hash(),
            TObj::Claim(c) => c.hash(),
            TObj::Disputed(d) => d.hash(),
            TObj::Liq(l) => l.hash(),
            TObj::ExitClaim(q) => q.hash(),
            TObj::Change(c) => c.hash(),
            TObj::Order(o) => o.hash(),
            TObj::Anchor => blake2b(&i9(7)),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            TObj::Position(_) => "position",
            TObj::Claim(_) => "claim",
            TObj::Disputed(_) => "disputed claim",
            TObj::Liq(l) => match l.state {
                LiqState::Claimed => "liquidation",
                LiqState::Disputed(_) => "disputed liquidation",
                LiqState::Proved => "proved liquidation",
            },
            TObj::ExitClaim(_) => "exit claim",
            TObj::Change(c) => if c.disputer.is_some() { "disputed change" } else { "stated change" },
            TObj::Order(_) => "order",
            TObj::Anchor => "anchor",
        }
    }

    /// The object's state as bytes, for whoever keeps it (the chain holds its hash): a kind byte (1 a position, 2 a
    /// claim, 3 a disputed claim), the position's fixed and moving parts, then what was stated (six pushed integers) and
    /// the disputer's key. Kind 6 is an order (the template's hash, the owner's key, eight pushed integers). Kind 4 is a liquidation: the position, seven pushed integers, the liquidator's key, and its
    /// state (0 claimed, 1 disputed with the disputer's key, 2 proved).
    pub fn encode(&self) -> Vec<u8> {
        let position = |a: &Position| [a.fixed(), a.moving()].concat();
        let said = |c: &Claim| [i9(c.side), i9(c.t0), i9(c.a[0]), i9(c.a[1]), i9(c.amount), i9(c.bond)].concat();
        match self {
            TObj::Position(a) => [vec![1], position(a)].concat(),
            TObj::Claim(c) => [vec![2], position(&c.position), said(c)].concat(),
            TObj::Disputed(d) => [vec![3], position(&d.claim.position), said(&d.claim), d.disputer.to_vec()].concat(),
            TObj::Liq(l) => {
                let q = &l.liq;
                let said = [i9(q.side), i9(q.t0), i9(q.a[0]), i9(q.a[1]), i9(q.fee), i9(q.add), i9(q.bond)].concat();
                let tail = match l.state {
                    LiqState::Claimed => vec![0],
                    LiqState::Disputed(d) => [vec![1], d.to_vec()].concat(),
                    LiqState::Proved => vec![2],
                };
                [vec![4], position(&q.position), said, q.liquidator.to_vec(), tail].concat()
            }
            TObj::ExitClaim(q) => [vec![5], position(&q.position), i9(q.side), i9(q.since), i9(q.bond)].concat(),
            // Kind 8, a stated change: the position, the kind and the time (two pushed integers), the 32 bytes stated,
            // the count of its outputs and their 41 bytes each, the bond (a pushed integer), the claimer's key, and
            // the disputer's if it is disputed.
            TObj::Change(c) => [vec![8], position(&c.position), i9(c.kind), i9(c.t0), c.take.said(), vec![c.outs.list().len() as u8], c.outs.bytes(), i9(c.bond), i9(c.held), c.claimer.to_vec(), c.disputer.map(|d| d.to_vec()).unwrap_or_default()].concat(),
            TObj::Order(o) => [vec![6], o.tpl.to_vec(), o.owner.to_vec(), i9(o.dir), i9(o.size), i9(o.limit), i9(o.margin), i9(o.tip), i9(o.expiry), i9(o.min_fill), i9(o.min_counter)].concat(),
            TObj::Anchor => vec![7],
        }
    }

    /// Reads `encode`'s bytes back, refusing anything that is not exactly an object's state.
    pub fn decode(b: &[u8]) -> Result<TObj> {
        let bad = |why: &str| Error::Decode(format!("not the state of an object of the thin family: {why}"));
        let (kind, rest) = b.split_first().ok_or_else(|| bad("empty"))?;
        if *kind == 7 {
            return if rest.is_empty() { Ok(TObj::Anchor) } else { Err(bad("wrong length")) };
        }
        if *kind == 6 {
            if rest.len() != 64 + 72 {
                return Err(bad("wrong length"));
            }
            let mut c = Cursor { b: &rest[64..], at: 0 };
            let (dir, size, limit, margin, tip, expiry, min_fill, min_counter) = (c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?, c.int()?);
            let o = Order { tpl: rest[..32].try_into().map_err(|_| bad("the template's hash"))?, owner: rest[32..64].try_into().map_err(|_| bad("the owner's key"))?, dir, size, limit, margin, tip, expiry, min_fill, min_counter };
            o.check().map_err(|_| bad("an order out of its bounds"))?;
            return Ok(TObj::Order(o));
        }
        if *kind == 8 {
            let at = FIXED_LEN + MOVING_LEN;
            if rest.len() < at + 18 + 32 + 1 {
                return Err(bad("wrong length"));
            }
            let position = Position::from_parts(&rest[..FIXED_LEN], &rest[FIXED_LEN..at])?;
            let mut c = Cursor { b: &rest[at..at + 18], at: 0 };
            let (kind, t0) = (c.int()?, c.int()?);
            let take = Take::read(&rest[at + 18..at + 50]).ok_or_else(|| bad("what is stated"))?;
            let n = rest[at + 50] as usize;
            let end = at + 51 + 41 * n;
            if n == 0 || n > OUTS_MAX || (rest.len() != end + 18 + 32 && rest.len() != end + 18 + 64) {
                return Err(bad("wrong length"));
            }
            let outs: Vec<Out> = rest[at + 51..end].chunks(41).map(|e| Out { key: e[0] != 0, to: e[1..33].try_into().expect("32 bytes"), value: i64::from_le_bytes(e[33..41].try_into().expect("8 bytes")) }).collect();
            if outs.iter().any(|o| o.value < 0) || rest[at + 51..end].chunks(41).any(|e| e[0] > 1) {
                return Err(bad("an output of a change"));
            }
            let mut c = Cursor { b: &rest[end..end + 18], at: 0 };
            let (bond, held) = (c.int()?, c.int()?);
            let claimer: [u8; 32] = rest[end + 18..end + 50].try_into().map_err(|_| bad("the claimer's key"))?;
            let disputer = if rest.len() == end + 82 { Some(rest[end + 50..].try_into().map_err(|_| bad("the disputer's key"))?) } else { None };
            if bond != position.bond() || !(0..LOCK_TIME_THRESHOLD).contains(&t0) || kind != KIND_TAKE {
                return Err(bad("a change the position could not carry (its kind, its time or its bond)"));
            }
            return Ok(TObj::Change(Change { position, kind, t0, take, outs: Outs::of(&outs).ok_or_else(|| bad("the outputs of a change"))?, bond, claimer, held, disputer }));
        }
        if *kind == 5 {
            let at = FIXED_LEN + MOVING_LEN;
            if rest.len() != at + 27 {
                return Err(bad("wrong length"));
            }
            let position = Position::from_parts(&rest[..FIXED_LEN], &rest[FIXED_LEN..at])?;
            let mut c = Cursor { b: &rest[at..], at: 0 };
            let (side, since, bond) = (c.int()?, c.int()?, c.int()?);
            let q = position.exit(side, since).filter(|q| q.bond == bond).ok_or_else(|| bad("an exit claim the position could not carry"))?;
            return Ok(TObj::ExitClaim(q));
        }
        if *kind == 4 {
            let at = FIXED_LEN + MOVING_LEN;
            if rest.len() != at + 63 + 32 + 1 && rest.len() != at + 63 + 32 + 1 + 32 {
                return Err(bad("wrong length"));
            }
            let position = Position::from_parts(&rest[..FIXED_LEN], &rest[FIXED_LEN..at])?;
            let mut c = Cursor { b: &rest[at..at + 63], at: 0 };
            let (side, t0, a, fee, add, bond) = (c.int()?, c.int()?, [c.int()?, c.int()?], c.int()?, c.int()?, c.int()?);
            let liquidator: [u8; 32] = rest[at + 63..at + 95].try_into().map_err(|_| bad("the liquidator's key"))?;
            let liq = position.liq_stated(side, t0, a, fee, add, liquidator).filter(|l| l.bond == bond).ok_or_else(|| bad("a liquidation the position could not carry"))?;
            let state = match (rest[at + 95], rest.len() - at - 96) {
                (0, 0) => LiqState::Claimed,
                (2, 0) => LiqState::Proved,
                (1, 32) => LiqState::Disputed(rest[at + 96..].try_into().map_err(|_| bad("the disputer's key"))?),
                _ => return Err(bad("a liquidation's state")),
            };
            return Ok(TObj::Liq(LiqObj { liq, state }));
        }
        let len = FIXED_LEN + MOVING_LEN + match kind {
            1 => 0,
            2 => 54,
            3 => 54 + 32,
            _ => return Err(bad("unknown kind")),
        };
        if rest.len() != len {
            return Err(bad("wrong length"));
        }
        let position = Position::from_parts(&rest[..FIXED_LEN], &rest[FIXED_LEN..FIXED_LEN + MOVING_LEN])?;
        if *kind == 1 {
            return Ok(TObj::Position(position));
        }
        let mut c = Cursor { b: &rest[FIXED_LEN + MOVING_LEN..FIXED_LEN + MOVING_LEN + 54], at: 0 };
        let claim = Claim { position, side: c.int()?, t0: c.int()?, a: [c.int()?, c.int()?], amount: c.int()?, bond: c.int()? };
        if position.claim(claim.side, claim.t0, claim.a, claim.amount) != Some(claim) {
            return Err(bad("a claim the position could not carry (its side, signed price, amount or bond)"));
        }
        if *kind == 2 {
            return Ok(TObj::Claim(claim));
        }
        let disputer: [u8; 32] = rest[len - 32..].try_into().map_err(|_| bad("the disputer's key"))?;
        Ok(TObj::Disputed(Disputed { claim, disputer }))
    }

    /// The least an object of this state holds: the position's two margins and the bonds posted on it. (The position's
    /// carrier, what it holds above the pot, is not asked for.)
    pub fn least_value(&self) -> i64 {
        match self {
            TObj::Position(a) => a.pot(),
            TObj::Claim(c) => c.position.pot() + c.bond,
            TObj::Disputed(d) => d.claim.position.pot() + 2 * d.claim.bond,
            TObj::Liq(l) => l.liq.position.pot() + l.bonds(),
            TObj::ExitClaim(q) => q.position.pot() + q.bond,
            TObj::Change(c) => c.position.pot() + c.bonds(),
            TObj::Order(o) => o.value(),
            TObj::Anchor => 0,
        }
    }

    /// The object's script in the library `lib`.
    pub fn script(&self, lib: &[u8; 32]) -> Vec<u8> {
        crate::thin_artifacts::kit().object(lib, &self.hash())
    }
}

// ---------------------------------------------------------------- a stated change

/// The kinds a change is stated of (`KINDS` of `KpfTChange`): each has a script that answers its dispute.
pub const KINDS: i64 = 5;
/// A take (`KpfTTake`).
pub const KIND_TAKE: i64 = 1;
/// The most outputs a change states.
pub const OUTS_MAX: usize = 4;

/// An output a change states: the position as it goes on with the state of the hash `to` (the first only), or a
/// payment to the key `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub key: bool,
    pub to: [u8; 32],
    pub value: i64,
}

/// The outputs a change states, one to four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outs {
    n: usize,
    e: [Out; OUTS_MAX],
    /// Which of them are notes to a shard: objects under the object's own id, of the state hash `to`.
    note: [bool; OUTS_MAX],
}

impl Outs {
    pub fn of(v: &[Out]) -> Option<Outs> {
        let all: Vec<(Out, bool)> = v.iter().map(|o| (*o, false)).collect();
        Outs::with_notes(&all)
    }

    /// The outputs, each with whether it is a note (a note is not the first, and its `to` is a state's hash).
    pub fn with_notes(v: &[(Out, bool)]) -> Option<Outs> {
        if v.is_empty() || v.len() > OUTS_MAX || v[0].1 || v[1..].iter().any(|(o, note)| !o.key && !note) || v.iter().any(|(o, note)| o.key && *note) {
            return None;
        }
        let mut e = [Out { key: true, to: [0; 32], value: 0 }; OUTS_MAX];
        let mut note = [false; OUTS_MAX];
        for (i, (o, is_note)) in v.iter().enumerate() {
            e[i] = *o;
            note[i] = *is_note;
        }
        Some(Outs { n: v.len(), e, note })
    }

    pub fn list(&self) -> &[Out] {
        &self.e[..self.n]
    }

    /// Whether the output `i` is a note to a shard.
    pub fn is_note(&self, i: usize) -> bool {
        i < self.n && self.note[i]
    }

    /// The outputs a statement shows, read back.
    pub fn read(b: &[u8]) -> Option<Outs> {
        if b.is_empty() || b.len() % 41 != 0 || b.chunks(41).any(|e| e[0] > 2) {
            return None;
        }
        let v: Vec<(Out, bool)> = b.chunks(41).map(|e| (Out { key: e[0] == 1, to: e[1..33].try_into().expect("32 bytes"), value: crate::state::int8(&e[33..41]) }, e[0] == 2)).collect();
        Outs::with_notes(&v).filter(|o| o.bytes() == b)
    }

    /// 41 bytes an output: a flag (0 the object, 1 a key, 2 a note), the 32 bytes, the value.
    pub fn bytes(&self) -> Vec<u8> {
        self.list().iter().enumerate().flat_map(|(i, o)| [&[if self.note[i] { 2 } else { o.key as u8 }][..], &o.to, &sm8(o.value)].concat()).collect()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&self.bytes())
    }

    pub fn total(&self) -> i64 {
        self.list().iter().map(|o| o.value).sum()
    }
}

/// A change of a position as it was stated (`KpfTChange`): what the position becomes (`outs`), why (`kind`, and
/// `arg`, the hash of what that kind states), the time the statement prices at, the bond and who stated it. Nothing
/// of it is checked when it is stated: it is disputed (`disputer`) and answered by the kind's script, applied after
/// the position's dispute period, or ended as late.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub position: Position,
    pub kind: i64,
    pub t0: i64,
    /// What is stated: a take (kind 1 is of a position, and whatever 32 bytes were shown read as one).
    pub take: Take,
    pub outs: Outs,
    pub bond: i64,
    pub claimer: [u8; 32],
    /// What the position held when the change was stated.
    pub held: i64,
    pub disputer: Option<[u8; 32]>,
}

impl Change {
    /// The hash of what is stated.
    pub fn arg(&self) -> [u8; 32] {
        self.take.arg()
    }

    /// The hash of the statement (`changeHash` of the scripts).
    pub fn stated_hash(&self) -> [u8; 32] {
        let a = &self.position;
        blake2b(&[i9(10), p33(&a.hash()), i9(self.kind), i9(self.t0), p33(&self.arg()), p33(&self.outs.hash()), i9(self.bond), p33(&self.claimer), i9(a.dispute_daa), i9(a.x.t.late_daa), i9(self.held)].concat())
    }

    pub fn hash(&self) -> [u8; 32] {
        match self.disputer {
            None => self.stated_hash(),
            Some(d) => blake2b(&[p33(&self.stated_hash()), p33(&d)].concat()),
        }
    }

    /// The bonds the object holds above the position's value.
    pub fn bonds(&self) -> i64 {
        if self.disputer.is_some() { 2 * self.bond } else { self.bond }
    }

    /// Whether the statement landed at its time or within the position's lateness after it.
    pub fn in_time(&self, landed: u64) -> bool {
        (0..=self.position.x.t.late_daa).contains(&(landed as i64 - self.t0))
    }

    pub fn disputed(&self, disputer: [u8; 32]) -> Change {
        Change { disputer: Some(disputer), ..*self }
    }

    /// The position the change's first output states, if `next` is it.
    pub fn becomes(&self, next: &Position) -> bool {
        self.outs.list().first().is_some_and(|o| !o.key && o.to == next.hash())
    }
}

/// What a take states (kind 1): the position's entries move to the prices `a`, so that what `side` gained since is
/// margin of its own, and `side` takes `amount` out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Take {
    pub side: i64,
    pub a: [i64; 2],
    pub amount: i64,
}

impl Take {
    /// What the statement shows: the side, the two prices and the amount, 8 bytes each.
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.side), sm8(self.a[0]), sm8(self.a[1]), sm8(self.amount)].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    /// The take a statement shows, read back (any 32 bytes read as one: a take that is nonsense is answered wrong).
    pub fn read(said: &[u8]) -> Option<Take> {
        if said.len() != 32 {
            return None;
        }
        let int = |i: usize| crate::state::int8(&said[8 * i..8 * i + 8]);
        Some(Take { side: int(0), a: [int(1), int(2)], amount: int(3) })
    }
}

/// The outputs of a take on a position holding `held`: the position after it holding the amount less, and the
/// amount to the claimer.
pub fn take_outs(after: &Position, claimer: &[u8; 32], held: i64, amount: i64) -> Option<Outs> {
    let mut v = vec![Out { key: false, to: after.hash(), value: held - amount }];
    if amount > 0 {
        v.push(Out { key: true, to: *claimer, value: amount });
    }
    Outs::of(&v)
}

impl Position {
    /// The position once a take is applied, with `KpfTTake`'s arithmetic: every used leg's entry is the price stated,
    /// A's gain there has moved to A's margin, and the taker's margin is the amount less. `None` when a margin would
    /// fall below zero.
    pub fn taken(&self, tk: &Take) -> Option<Position> {
        let legs = self.x.legs;
        let moved: i64 = (0..2).map(|i| { let l = &legs[i]; let d = l.worth(tk.a[i]) - l.worth(l.entry); if l.dir == crate::state::LONG { d } else { -d } }).sum();
        let (mut am, mut bm) = (self.x.a_margin + moved, self.x.b_margin - moved);
        if tk.side == SIDE_A {
            am -= tk.amount;
        } else {
            bm -= tk.amount;
        }
        if am < 0 || bm < 0 {
            return None;
        }
        let mut c = self.x;
        c.legs[0].entry = tk.a[0];
        if c.legs[1].size > 0 {
            c.legs[1].entry = tk.a[1];
        }
        c.a_margin = am;
        c.b_margin = bm;
        Some(Position { x: c, ..*self })
    }

    /// `KpfTTake`'s rule: whether a take stated by `claimer` with the outputs `outs`, on this position holding
    /// `held`, is within the rule at the committee's prices `c`.
    pub fn take_within(&self, tk: &Take, claimer: &[u8; 32], outs: &Outs, held: i64, c: [i64; 2]) -> bool {
        let (legs, g) = (self.x.legs, self.x.t.g_bps);
        let near = |a: i64, c: i64, pm: i64| {
            let p = c.min(pm);
            a > 0 && a >= p * (BPS - g) / BPS && a <= ((p * (BPS + g) + BPS - 1) / BPS).min(pm)
        };
        if !near(tk.a[0], c[0], legs[0].price_max) || (legs[1].size > 0 && !near(tk.a[1], c[1], legs[1].price_max)) || (legs[1].size <= 0 && tk.a[1] != 0) {
            return false;
        }
        let p = [c[0].min(legs[0].price_max), c[1].min(legs[1].price_max)];
        let pot = self.pot();
        let gain: i64 = (0..2).map(|i| { let l = &legs[i]; let d = l.worth(p[i]) - l.worth(l.entry); if l.dir == crate::state::LONG { d } else { -d } }).sum();
        let due_a = (self.x.a_margin + gain).clamp(0, pot);
        let m = self.x.maint(legs[0].worth(p[0]) + legs[1].worth(p[1]));
        let (key, ask, due) = match tk.side {
            SIDE_A => (self.x.a_key, self.b_min_counter, due_a),
            SIDE_B => (self.x.b_key, self.a_min_counter, pot - due_a),
            _ => return false,
        };
        let floor = m.max(ask).min(due);
        if key != *claimer || tk.amount < 0 || due - tk.amount < floor {
            return false;
        }
        match self.taken(tk) {
            Some(after) => take_outs(&after, claimer, held, tk.amount).is_some_and(|o| o == *outs),
            None => false,
        }
    }

    /// The most `side` takes out at the prices `c` (the committee's, or its own reading of them): what it is owed
    /// there above its floor.
    pub fn take_max(&self, side: i64, c: [i64; 2]) -> i64 {
        let legs = self.x.legs;
        let p = [c[0].min(legs[0].price_max), c[1].min(legs[1].price_max)];
        let pot = self.pot();
        let due_a = self.owed_gain(p).clamp(0, pot);
        let m = self.x.maint(legs[0].worth(p[0]) + legs[1].worth(p[1]));
        let (ask, due) = if side == SIDE_A { (self.b_min_counter, due_a) } else { (self.a_min_counter, pot - due_a) };
        (due - m.max(ask)).max(0)
    }

    /// The most `side` takes out when it states the prices `a`, whatever the committee's prices within the
    /// tolerance of them turn out to be: the least of `take_max` at the ends of that band (what a tool states by
    /// default: a take of more is refuted when the committee's price is a little worse for the taker than its own).
    pub fn take_safe(&self, side: i64, a: [i64; 2]) -> i64 {
        let g = self.x.t.g_bps;
        let ends = |p: i64| if p > 0 { vec![p * BPS / (BPS + g), p, p * BPS / (BPS - g) + 1] } else { vec![0] };
        let mut least = i64::MAX;
        for p0 in ends(a[0]) {
            for p1 in ends(a[1]) {
                least = least.min(self.take_max(side, [p0, p1]));
            }
        }
        least.max(0)
    }

    fn owed_gain(&self, p: [i64; 2]) -> i64 {
        let legs = self.x.legs;
        self.x.a_margin + (0..2).map(|i| { let l = &legs[i]; let d = l.worth(p[i]) - l.worth(l.entry); if l.dir == crate::state::LONG { d } else { -d } }).sum::<i64>()
    }

    /// The statement of a take on this position holding `held`, by the owner of `side`: what `KpfTChange` is told.
    /// Refused when the take could not be applied at its own prices (a margin below zero, a side that is none).
    pub fn take(&self, tk: Take, t0: i64, held: i64) -> Result<Change> {
        let claimer = self.key_of(tk.side).ok_or_else(|| Error::Invalid("a take is by side a (1) or b (2)".into()))?;
        let after = self.taken(&tk).filter(|_| tk.amount >= 0 && tk.amount <= held).ok_or_else(|| Error::Invalid("the take leaves a margin below zero at the prices it states".into()))?;
        after.check()?;
        let outs = take_outs(&after, &claimer, held, tk.amount).ok_or_else(|| Error::Invalid("the take's outputs".into()))?;
        if !(0..LOCK_TIME_THRESHOLD).contains(&t0) || self.bond() <= 0 {
            return Err(Error::Invalid("the take's time is out of range, or the position's pot is too small for a bond".into()));
        }
        Ok(Change { position: *self, kind: KIND_TAKE, t0, take: tk, outs, bond: self.bond(), claimer, held, disputer: None })
    }
}

// ---------------------------------------------------------------- accounts of any number of legs

/// A take on an account (`KpfTPoint` and the three answers of a point).
pub const KIND_ACCOUNT_TAKE: i64 = 2;
/// A step of a liquidation of an account: one leg leaves it, for a fee (`KpfTStepLiq`).
pub const KIND_ACCOUNT_LIQ: i64 = 3;
/// A leg that leaves an account by its party's own statement (`KpfTStepLiq`).
pub const KIND_ACCOUNT_LEAVE: i64 = 4;
/// A draw from a shard by its owner (`KpfTShardEnd`, `KpfTShardPrice`, `KpfTDrawNote`; `crate::reserve`).
pub const KIND_DRAW: i64 = 5;
/// The most legs the tools give an account (no script counts them: an answer is of one leg).
pub const LEGS_MAX: usize = 64;

/// A statement on an object that is shown as its hash (a position or an account): what `KpfTChange` wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stated {
    pub position: [u8; 32],
    pub window: i64,
    pub late: i64,
    pub kind: i64,
    pub t0: i64,
    pub arg: [u8; 32],
    pub outs: Outs,
    pub bond: i64,
    pub claimer: [u8; 32],
    /// What the object held when the change was stated (an answer counts from this, not from what it holds since).
    pub held: i64,
}

/// The mark, in the place of its lateness, of a statement that stands again after a point of it was answered
/// right: it was held to its time when it was disputed, and is not held to it again. Its lateness is kept beside
/// the mark (the place holds the two, added): the last time to dispute it is counted by it.
pub const LATE_AGAIN: i64 = 499_999_999_999;

impl Stated {
    pub fn hash(&self) -> [u8; 32] {
        self.hash_with_outs(&self.outs.hash())
    }

    /// The statement's hash with the hash of its outputs given (outputs as bytes that are not the model's).
    pub fn hash_with_outs(&self, outs: &[u8; 32]) -> [u8; 32] {
        blake2b(&[i9(10), p33(&self.position), i9(self.kind), i9(self.t0), p33(&self.arg), p33(outs), i9(self.bond), p33(&self.claimer), i9(self.window), i9(self.late), i9(self.held)].concat())
    }

    /// The statement as it stands again after a point of it was answered right.
    pub fn again(&self) -> Stated {
        Stated { late: if self.late < LATE_AGAIN { LATE_AGAIN + self.late } else { self.late }, ..*self }
    }

    /// The statement's lateness, standing again or not.
    pub fn lateness(&self) -> i64 {
        if self.late >= LATE_AGAIN {
            self.late - LATE_AGAIN
        } else {
            self.late
        }
    }

    /// The last time to dispute a point of it: its time, its lateness and a dispute period. A dispute that landed
    /// later is dismissed by anyone; a statement written at or after it is applied at once (`docs` 14.1).
    pub fn last_dispute(&self) -> i64 {
        self.t0 + self.lateness() + self.window
    }

    pub fn in_time(&self, landed: u64) -> bool {
        (0..=self.late).contains(&(landed as i64 - self.t0))
    }
}

/// A statement one point of which is disputed (`KpfTPoint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointed {
    pub st: Stated,
    pub disputer: [u8; 32],
    pub point: i64,
}

impl Pointed {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(11), p33(&self.st.hash()), p33(&self.disputer), i9(self.point)].concat())
    }
}

impl Change {
    /// The statement as the scripts that are not shown the position read it.
    pub fn as_stated(&self) -> Stated {
        let a = &self.position;
        Stated { position: a.hash(), window: a.dispute_daa, late: a.x.t.late_daa, kind: self.kind, t0: self.t0, arg: self.arg(), outs: self.outs, bond: self.bond, claimer: self.claimer, held: self.held }
    }
}

/// An account: a position of any number of legs. Its state is a position's with another fixed part, the hash of the
/// terms and of the root of its legs' chain, so that every script that is shown a position's fixed part as its hash
/// serves it as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub t: crate::cross::XTerms,
    pub legs: Vec<crate::cross::XLeg>,
    pub a_key: [u8; 32],
    pub b_key: [u8; 32],
    pub a_margin: i64,
    pub b_margin: i64,
    pub expiry: i64,
    pub dispute_daa: i64,
    pub bond_bps: i64,
    pub a_min_counter: i64,
    pub b_min_counter: i64,
    /// The shard that covers it (`docs/ARCHITECTURE-CLEARING.md` 6a): its moving part then ends with the push of the
    /// shard's id, 171 bytes in all, and side B is the clearing party.
    pub shard: Option<[u8; 32]>,
}

/// A row of a take's trace, 112 bytes: the price stated for a leg; over the legs up to it at the prices stated, A's
/// gain, the worth each side is asked the maintenance of (`wa`, `wb`: the futures legs', and the underlyings' of the
/// options the side wrote) and what each side keeps whole beside its maintenance (`ea`, `eb`: what the options it
/// holds were counted at); the chain of the legs up to it, and the same chain with every entry at its price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub a: i64,
    pub gain: i64,
    pub wa: i64,
    pub wb: i64,
    pub ea: i64,
    pub eb: i64,
    pub g: [u8; 32],
    pub h: [u8; 32],
}

/// The length of a row of a trace.
pub const ROW_LEN: usize = 112;

impl Row {
    /// What stands before the first row.
    pub const ZERO: Row = Row { a: 0, gain: 0, wa: 0, wb: 0, ea: 0, eb: 0, g: [0; 32], h: [0; 32] };

    pub fn bytes(&self) -> Vec<u8> {
        [&sm8(self.a)[..], &sm8(self.gain), &sm8(self.wa), &sm8(self.wb), &sm8(self.ea), &sm8(self.eb), &self.g, &self.h].concat()
    }

    /// (The worth `side` is asked the maintenance of, what it keeps whole beside it.)
    pub fn asked(&self, side: i64) -> (i64, i64) {
        if side == SIDE_A {
            (self.wa, self.ea)
        } else {
            (self.wb, self.eb)
        }
    }
}

fn rate_up(w: i64, rate: i64) -> i64 {
    (w / BPS) * rate + ((w % BPS) * rate + BPS - 1) / BPS
}

pub fn rows_bytes(rows: &[Row]) -> Vec<u8> {
    rows.iter().flat_map(|r| r.bytes()).collect()
}

/// The 111 bytes of a leg, as a position's fixed part has them.
pub fn leg_bytes(l: &crate::cross::XLeg) -> Vec<u8> {
    [p33(&l.market), p33(&l.committee), i9(l.scale), i9(l.price_max), i9(l.size), i9(l.dir), i9(l.entry)].concat()
}

/// The tag of the name a market's options have in a shard's row and in a note (`OPTION_TAG` of the scripts).
pub const OPTION_TAG: i64 = 13;
/// The tag of the name a market's futures legs have there (`FUTURES_TAG`).
pub const FUTURES_TAG: i64 = 14;
/// What an option is, in the last three bits of its direction's place: a call side A holds.
pub const CALL_HELD: i64 = 3;
/// A call side A wrote (side B holds it).
pub const CALL_WRITTEN: i64 = 4;
/// A put side A holds.
pub const PUT_HELD: i64 = 5;
/// A put side A wrote.
pub const PUT_WRITTEN: i64 = 6;

/// The direction's place of an option leg: its kind (3 to 6) in the last three bits, its strike above them.
pub fn option_dir(kind: i64, strike: i64) -> i64 {
    kind + 8 * strike
}

/// The kind and the strike of a leg that is an option (`None`: a futures leg), read as the scripts read it: a
/// number from 3 on whose last three bits are 3 to 6. Its entry is the price it was last counted at.
pub fn option_of(l: &crate::cross::XLeg) -> Option<(i64, i64)> {
    (l.dir >= 3 && (3..=6).contains(&(l.dir % 8))).then(|| (l.dir % 8, l.dir / 8))
}

/// The side that holds an option leg (`None`: a futures leg). An option leaves by its holder's side alone.
pub fn holder_of(l: &crate::cross::XLeg) -> Option<i64> {
    option_of(l).map(|(c, _)| if c % 2 == 1 { SIDE_A } else { SIDE_B })
}

/// Whether the direction's place of a leg says what a leg is: a direction, or an option whose strike is within its
/// market's limit (what `KpfTLegAdd` asks).
pub fn leg_kind_ok(l: &crate::cross::XLeg) -> bool {
    l.dir == crate::state::LONG || l.dir == crate::state::SHORT || option_of(l).is_some_and(|(_, k)| k >= 1 && k <= l.price_max)
}

/// Whether what a leg is worth at any price within its market's limit is computed without overflow: the bound a
/// position's leg is held to (`KpfTLegAdd` asks it of a leg that enters a covered account).
pub fn leg_size_ok(l: &crate::cross::XLeg) -> bool {
    l.scale > 0 && l.scale <= crate::state::SCALE_MAX && l.price_max > 0 && l.size > 0 && l.size <= MARGIN_MAX && l.size / l.scale <= MARGIN_MAX / l.price_max - 1
}

/// Whether the sums of a trace over these legs, at any prices within their markets' limits, stay within 64 bits.
/// The scripts hold a leg to its own bound and have no count of legs: this is held by the tool, which states on no
/// account beyond it and adds or moves no leg into one (review 34, F58).
pub fn legs_sum_ok(legs: &[crate::cross::XLeg]) -> bool {
    legs.iter().try_fold(0i64, |s, l| if l.scale > 0 { s.checked_add((l.size / l.scale).checked_add(1)?.checked_mul(l.price_max)?) } else { None }).is_some()
}

/// What a leg is worth to side A at the price `x`, from its entry: a futures leg's gain, or what an option is worth
/// if exercised there less what it was counted at (what a row of a trace adds to A's gain).
pub fn leg_gain_at(l: &crate::cross::XLeg, x: i64) -> i64 {
    leg_adds(l, x)[0]
}

/// What a leg's row adds at the price `a`: to A's gain, to the worth of side A and of side B, and to what A and B
/// keep whole. A futures leg: its worth at the price less at its entry to the gain (the other way where A is
/// short), and its worth to both sides'. An option: to the gain what it is worth if exercised at the price less
/// what it was at its entry (the other way where A wrote it); its underlying's worth to the worth of whoever wrote
/// it; and what it is worth exercised at the price to what its holder keeps whole.
fn leg_adds(l: &crate::cross::XLeg, a: i64) -> [i64; 5] {
    let w = l.worth(a);
    let we = l.worth(l.entry);
    match option_of(l) {
        None => [if l.dir == crate::state::LONG { w - we } else { we - w }, w, w, 0, 0],
        Some((c, strike)) => {
            let wk = l.worth(strike);
            let (va, ve) = if c >= PUT_HELD { ((wk - w).max(0), (wk - we).max(0)) } else { ((w - wk).max(0), (we - wk).max(0)) };
            if c % 2 == 1 {
                [va - ve, 0, w, va, 0]
            } else {
                [ve - va, w, 0, 0, va]
            }
        }
    }
}

/// The row after `before` for the leg `l` at the price `a`; `left_out`: the statement leaves this leg out of what
/// the account becomes (its second chain passes it by).
pub fn row_after(before: &Row, l: &crate::cross::XLeg, a: i64, left_out: bool) -> Row {
    let [d, ua, ub, xa, xb] = leg_adds(l, a);
    let h = if left_out { before.h } else { blake2b(&[&before.h[..], &leg_bytes(&crate::cross::XLeg { entry: a, ..*l })].concat()) };
    Row { a, gain: before.gain + d, wa: before.wa + ua, wb: before.wb + ub, ea: before.ea + xa, eb: before.eb + xb, g: blake2b(&[&before.g[..], &leg_bytes(l)].concat()), h }
}

/// `KpfTStepRow`: a row is opened by a chain `prev` and a leg that hash to the row's chain (`None` where they do
/// not: the script then takes neither verdict). The row is wrong where `prev` is not the chain of the row before
/// it, and else right exactly when it follows from that row and the leg.
pub fn row_judged(prev: &[u8; 32], l: &crate::cross::XLeg, before: &Row, row: &Row, left_out: bool) -> Option<bool> {
    row_judged_of(prev, l, before, row, left_out.then(|| (crate::reserve::Head::of(l), crate::reserve::SNote::units(l))), holder_of(l).unwrap_or(SIDE_A))
}

/// `KpfTStepRow` on a row of an entry (kind 6); `named`: the row is the one the statement names, and this is the
/// leg the statement shows as entering. That row is right only where the leg its chain is opened by is that leg,
/// is a leg (a size within what is computed at its market's limit, a direction or an option's kind and strike),
/// and has the row's price for its entry: it gains nothing as it enters, and it stays in the second chain.
pub fn enter_row_judged(prev: &[u8; 32], l: &crate::cross::XLeg, before: &Row, row: &Row, named: Option<&crate::cross::XLeg>) -> Option<bool> {
    if row.g == [0; 32] {
        return Some(false);
    }
    if blake2b(&[&prev[..], &leg_bytes(l)].concat()) != row.g {
        return None;
    }
    let enters = named.is_none_or(|x| x == l && leg_size_ok(l) && leg_kind_ok(l) && l.entry == row.a);
    Some(*prev == before.g && row.a > 0 && row.a <= l.price_max && enters && *row == row_after(before, l, row.a, false))
}

/// As `row_judged_in`, for a take, a step of a liquidation or a leg that leaves (kinds 2 to 4).
pub fn row_judged_of(prev: &[u8; 32], l: &crate::cross::XLeg, before: &Row, row: &Row, leave: Option<(crate::reserve::Head, i64)>, side: i64) -> Option<bool> {
    row_judged_in(KIND_ACCOUNT_LEAVE, prev, l, before, row, leave, side)
}

/// As `row_judged`, for a statement of the kind given; `leave`: the row is of the leg the statement names, and this
/// is what it says of that leg (the name of its row in the shard and side B's units), which the row holds to the
/// leg. In kinds 2 to 4 that leg leaves; in an entry (kind 6) it enters, and its row is right only where it is a
/// leg (a size within what is computed at its market's limit, a direction or an option's kind and strike) whose
/// entry is the row's price: it gains nothing as it enters.
/// (`leave` below: the row is of the leg that leaves, and this is what the statement says of that leg (its
/// market and side B's units in it), which the row holds to the leg; `side`: the side the statement is of (an
/// option leaves by its holder's side alone: the row of an option that leaves by the other is wrong).
pub fn row_judged_in(kind: i64, prev: &[u8; 32], l: &crate::cross::XLeg, before: &Row, row: &Row, leave: Option<(crate::reserve::Head, i64)>, side: i64) -> Option<bool> {
    // (A chain of 32 zero bytes is the seed of every chain and of no leg: nobody opens it, and a row that carries it
    // is wrong whatever is shown.)
    if row.g == [0; 32] {
        return Some(false);
    }
    if blake2b(&[&prev[..], &leg_bytes(l)].concat()) != row.g {
        return None;
    }
    if kind == KIND_ENTER {
        // (An entry shows the leg that enters whole, and its row is held to that leg: `enter_row_judged`. Here the
        // leg shown is taken to be the one given.)
        return enter_row_judged(prev, l, before, row, leave.map(|_| l));
    }
    let said = leave.is_none_or(|(head, units)| head == crate::reserve::Head::of(l) && units == crate::reserve::SNote::units(l) && holder_of(l).is_none_or(|h| h == side));
    Some(*prev == before.g && row.a > 0 && row.a <= l.price_max && said && *row == row_after(before, l, row.a, leave.is_some()))
}

/// As `row_judged`, opened by the chain of the row before (what an honest trace is opened by).
pub fn row_right(l: &crate::cross::XLeg, before: &Row, row: &Row, left_out: bool) -> Option<bool> {
    row_judged(&before.g, l, before, row, left_out)
}

/// `KpfTStepPrice`: whether the price of a row is within the tolerance `g_bps` of the committee's `c`.
pub fn price_right(l: &crate::cross::XLeg, row: &Row, c: i64, g_bps: i64) -> bool {
    let p = c.min(l.price_max);
    row.a > 0 && row.a >= p * (BPS - g_bps) / BPS && row.a <= ((p * (BPS + g_bps) + BPS - 1) / BPS).min(l.price_max)
}

/// What a leg's leaving leaves: the account, the claimer's fee, and what the shard is asked to pay A: what the pot
/// could not, less twice the tolerance of the legs' worth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stepped {
    pub after: Account,
    pub fee: i64,
    pub owed: i64,
}

/// A leg enters an account by a statement of its two parties (`KpfTEnter`).
pub const KIND_ENTER: i64 = 6;

/// What an entry states (kind 6): the side (1), nothing (0) and the place of the leg that enters (the last); of
/// that leg the name of its row in the shard and side B's units; what the shard's id is the hash of beside its
/// committee; and the trace, the entering leg's row last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AEnter {
    pub side: i64,
    pub amount: i64,
    pub skip: i64,
    /// The leg that enters, whole: whoever watches has it from the statement.
    pub leg: crate::cross::XLeg,
    pub seed: [u8; 32],
    pub rows: Vec<Row>,
}

/// A leg read back from its 111 bytes (`None`: bytes that are no leg's).
pub fn leg_read(b: &[u8]) -> Option<crate::cross::XLeg> {
    if b.len() != 111 || b[0] != 0x20 || b[33] != 0x20 || [66, 75, 84, 93, 102].iter().any(|i| b[*i] != 0x08) {
        return None;
    }
    let int = |i: usize| crate::state::int8(&b[i..i + 8]);
    Some(crate::cross::XLeg { market: b[1..33].try_into().ok()?, committee: b[34..66].try_into().ok()?, scale: int(67), price_max: int(76), size: int(85), dir: int(94), entry: int(103) })
}

impl AEnter {
    /// What the statement shows: 167 bytes (three numbers, the leg, the seed), then the rows.
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.side).to_vec(), sm8(self.amount).to_vec(), sm8(self.skip).to_vec(), leg_bytes(&self.leg), self.seed.to_vec(), rows_bytes(&self.rows)].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    /// The statement a transaction shows, read back.
    pub fn read(said: &[u8]) -> Option<AEnter> {
        if said.len() < 167 || (said.len() - 167) % ROW_LEN != 0 {
            return None;
        }
        let int = |b: &[u8]| crate::state::int8(b);
        let rows = said[167..].chunks(ROW_LEN).map(|r| Row { a: int(&r[..8]), gain: int(&r[8..16]), wa: int(&r[16..24]), wb: int(&r[24..32]), ea: int(&r[32..40]), eb: int(&r[40..48]), g: r[48..80].try_into().expect("32 bytes"), h: r[80..112].try_into().expect("32 bytes") }).collect();
        Some(AEnter { side: int(&said[..8]), amount: int(&said[8..16]), skip: int(&said[16..24]), leg: leg_read(&said[24..135])?, seed: said[135..167].try_into().expect("32 bytes"), rows })
    }
}

/// What a take on an account states, or a step of its liquidation, or a leg that leaves: the side, the amount (the
/// liquidator's fee), the trace of the prices stated, the leg that leaves the account (0: none, a take), and of that
/// leg its market and side B's units in it (what the note of its leaving is made of; `None`: no market, no units).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ATake {
    pub side: i64,
    pub amount: i64,
    pub rows: Vec<Row>,
    pub skip: i64,
    pub leave: Option<(crate::reserve::Head, i64)>,
}

impl ATake {
    /// The 92 bytes of the leg that leaves: its market's 84 and side B's units.
    fn leave_bytes(&self) -> Vec<u8> {
        let (head, units) = self.leave.unwrap_or((crate::reserve::Head::EMPTY, 0));
        [head.bytes(), sm8(units).to_vec()].concat()
    }

    /// What the statement shows: the side, the amount and the leg left out, 8 bytes each; the 92 bytes of that
    /// leg; then the trace.
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.side).to_vec(), sm8(self.amount).to_vec(), sm8(self.skip).to_vec(), self.leave_bytes(), rows_bytes(&self.rows)].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    /// The statement a transaction shows, read back (`None`: bytes that are no statement of an account's, or whose
    /// 92 bytes of the leaving leg are not a market's as a leg has it).
    pub fn read(said: &[u8]) -> Option<ATake> {
        if said.len() < 116 || (said.len() - 116) % ROW_LEN != 0 {
            return None;
        }
        let int = |b: &[u8]| crate::state::int8(b);
        let h = &said[24..108];
        if h[0] != 0x20 || h[33] != 0x20 || h[66] != 0x08 || h[75] != 0x08 {
            return None;
        }
        let head = crate::reserve::Head { market: h[1..33].try_into().expect("32 bytes"), committee: h[34..66].try_into().expect("32 bytes"), scale: int(&h[67..75]), price_max: int(&h[76..84]) };
        let units = int(&said[108..116]);
        let rows = said[116..].chunks(ROW_LEN).map(|r| Row { a: int(&r[..8]), gain: int(&r[8..16]), wa: int(&r[16..24]), wb: int(&r[24..32]), ea: int(&r[32..40]), eb: int(&r[40..48]), g: r[48..80].try_into().expect("32 bytes"), h: r[80..112].try_into().expect("32 bytes") }).collect();
        let leave = if head == crate::reserve::Head::EMPTY && units == 0 { None } else { Some((head, units)) };
        Some(ATake { side: int(&said[..8]), amount: int(&said[8..16]), skip: int(&said[16..24]), rows, leave })
    }
}

impl Account {
    /// A position with this account's terms and moving part, and no leg: what the bytes are taken from.
    fn shell(&self) -> Position {
        let none = crate::cross::XLeg::none();
        Position { x: Cross { t: self.t, legs: [none, none], a_key: self.a_key, b_key: self.b_key, a_margin: self.a_margin, b_margin: self.b_margin, expiry: self.expiry }, dispute_daa: self.dispute_daa, bond_bps: self.bond_bps, a_min_counter: self.a_min_counter, b_min_counter: self.b_min_counter }
    }

    /// The ten terms, 90 bytes.
    pub fn terms(&self) -> Vec<u8> {
        self.shell().fixed()[..90].to_vec()
    }

    pub fn moving(&self) -> Vec<u8> {
        match self.shard {
            None => self.shell().moving(),
            Some(sid) => [self.shell().moving(), p33(&sid)].concat(),
        }
    }

    /// The root of the legs' chain: `g_0` = 32 zero bytes, `g_i = blake2b(g_(i-1) + leg_i)`.
    pub fn legs_root(&self) -> [u8; 32] {
        self.legs.iter().fold([0u8; 32], |g, l| blake2b(&[&g[..], &leg_bytes(l)].concat()))
    }

    pub fn fixed_hash(&self) -> [u8; 32] {
        blake2b(&[self.terms(), p33(&self.legs_root())].concat())
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.fixed_hash()), self.moving()].concat())
    }

    pub fn pot(&self) -> i64 {
        self.a_margin + self.b_margin
    }

    pub fn bond(&self) -> i64 {
        self.shell().bond()
    }

    pub fn check(&self) -> Result<()> {
        let no = |why: &str| Err(Error::Invalid(format!("not an account: {why}")));
        if self.legs.is_empty() || self.legs.len() > LEGS_MAX {
            return no("it has one to sixty-four legs");
        }
        if self.legs.iter().any(|l| !leg_size_ok(l) || l.entry <= 0 || l.entry > l.price_max || !leg_kind_ok(l)) {
            return no("a leg has no size or one beyond what is computed at its market's limit, no scale, an entry out of its market's limit, or neither a direction nor an option's kind and strike");
        }
        // (The scripts hold a leg to its own bound and have no count of legs: the sums of a trace at the markets'
        // limits are held within 64 bits here, by the tool. Review 34, F58.)
        if !legs_sum_ok(&self.legs) {
            return no("its legs together are worth more at their markets' limits than a trace sums");
        }
        if self.a_margin < 0 || self.b_margin < 0 || self.a_margin > MARGIN_MAX || self.b_margin > MARGIN_MAX || self.bond() <= 0 {
            return no("a margin is out of range, or the pot is too small for a bond");
        }
        if !(1..=WINDOW_MAX).contains(&self.dispute_daa) || !(1..=BPS).contains(&self.bond_bps) {
            return no("the dispute period or the bond rate is out of range");
        }
        Ok(())
    }

    /// The trace of a take at the prices `a`, one a leg.
    pub fn trace(&self, a: &[i64]) -> Vec<Row> {
        self.trace_without(a, 0)
    }

    /// The trace at the prices `a` of a statement that leaves the leg `skip` out (0: none).
    pub fn trace_without(&self, a: &[i64], skip: i64) -> Vec<Row> {
        let mut rows: Vec<Row> = vec![];
        for (i, (l, p)) in self.legs.iter().zip(a).enumerate() {
            rows.push(row_after(rows.last().unwrap_or(&Row::ZERO), l, *p, skip == i as i64 + 1));
        }
        rows
    }

    /// The form a statement of kind 2 or 3 shows: the terms and the root of the legs.
    pub fn form(&self) -> Vec<u8> {
        [self.terms(), self.legs_root().to_vec()].concat()
    }

    /// What is asked of the side that takes, by the last row of the trace: the maintenance of the worth it is asked
    /// it of and the tolerance of it (the prices stated are only near the committee's), and what the options it
    /// holds were counted at; or what the other party asked of its margin.
    fn floor(&self, side: i64, last: &Row) -> i64 {
        let (w, e) = last.asked(side);
        (rate_up(w, self.t.mmr_bps + self.t.g_bps) + e).max(if side == SIDE_A { self.b_min_counter } else { self.a_min_counter })
    }

    /// The account once a take with this last row is applied (`KpfTStepEnd`'s arithmetic): `None` when a margin
    /// would fall below zero.
    pub fn taken(&self, side: i64, amount: i64, rows: &[Row]) -> Option<Account> {
        let last = rows.last()?;
        let (mut am, mut bm) = (self.a_margin + last.gain, self.b_margin - last.gain);
        if side == SIDE_A {
            am -= amount;
        } else {
            bm -= amount;
        }
        if am < 0 || bm < 0 || rows.len() != self.legs.len() {
            return None;
        }
        let legs = self.legs.iter().zip(rows).map(|(l, r)| crate::cross::XLeg { entry: r.a, ..*l }).collect();
        Some(Account { legs, a_margin: am, b_margin: bm, ..self.clone() })
    }

    /// The most `side` takes out at the prices `a` it states.
    pub fn take_max(&self, side: i64, a: &[i64]) -> i64 {
        let rows = self.trace(a);
        let Some(last) = rows.last() else { return 0 };
        let due_a = (self.a_margin + last.gain).clamp(0, self.pot());
        let due = if side == SIDE_A { due_a } else { self.pot() - due_a };
        (due - self.floor(side, last)).max(0)
    }

    /// `KpfTStepEnd`: whether the end of a take's trace is right for this account holding `held`.
    pub fn end_right(&self, tk: &ATake, claimer: &[u8; 32], outs: &Outs, held: i64) -> bool {
        let Some(last) = tk.rows.last() else { return false };
        if tk.skip != 0 {
            return false;
        }
        let key = match tk.side {
            SIDE_A => self.a_key,
            SIDE_B => self.b_key,
            _ => return false,
        };
        let pot = self.pot();
        let due_a = (self.a_margin + last.gain).clamp(0, pot);
        let due = if tk.side == SIDE_A { due_a } else { pot - due_a };
        let floor = self.floor(tk.side, last).min(due);
        // A covered account pays its side B nothing: a take for B is stated by anyone, and is a deposit.
        let deposit = self.shard.filter(|_| tk.side == SIDE_B);
        if last.g != self.legs_root() || last.wa < 0 || last.wb < 0 || (deposit.is_none() && key != *claimer) || tk.amount < 0 || due - tk.amount < floor {
            return false;
        }
        let (mut am, mut bm) = (self.a_margin + last.gain, self.b_margin - last.gain);
        if tk.side == SIDE_A {
            am -= tk.amount;
        } else {
            bm -= tk.amount;
        }
        if am < 0 || bm < 0 {
            return false;
        }
        // The next state by the last row's second chain (the script has no legs to rebuild).
        let next = Account { a_margin: am, b_margin: bm, ..self.clone() };
        let h = blake2b(&[p33(&blake2b(&[self.terms(), p33(&last.h)].concat())), next.moving()].concat());
        let mut v = vec![(Out { key: false, to: h, value: held - tk.amount }, false)];
        if tk.amount > 0 {
            v.push(match deposit {
                Some(sid) => {
                    let n = crate::reserve::SNote::deposit(sid, self.b_key, tk.amount);
                    (Out { key: false, to: n.hash(), value: n.value() }, true)
                }
                None => (Out { key: true, to: *claimer, value: tk.amount }, false),
            });
        }
        Outs::with_notes(&v).is_some_and(|o| o == *outs)
    }

    /// The first point of a stated take that is wrong at the committee's prices `c` (one a leg), for whoever
    /// disputes: 0 the end, 2i - 1 row i, 2i the price of row i. `None`: the statement is the rule's.
    pub fn wrong_point(&self, tk: &ATake, claimer: &[u8; 32], outs: &Outs, held: i64, c: &[i64]) -> Option<i64> {
        self.wrong_point_of(KIND_ACCOUNT_TAKE, tk, claimer, outs, held, c)
    }

    /// As `wrong_point`, for a statement of the kind given (a take, or a step of a liquidation).
    pub fn wrong_point_of(&self, kind: i64, tk: &ATake, claimer: &[u8; 32], outs: &Outs, held: i64, c: &[i64]) -> Option<i64> {
        let end = if kind == KIND_ACCOUNT_TAKE { self.end_right(tk, claimer, outs, held) } else { self.step_end_right(kind, tk, claimer, outs, held) };
        if !end {
            return Some(0);
        }
        self.rows_wrong_point(kind, tk, c)
    }

    /// The first row or price of a trace over this account's legs that is wrong at the committee's prices `c` (one
    /// a leg), its end being right: 2i - 1 row i, 2i the price of row i.
    pub fn rows_wrong_point(&self, kind: i64, tk: &ATake, c: &[i64]) -> Option<i64> {
        self.rows_wrong_point_by(&tk.rows, c, &|i, l, before, row| {
            let leave = (tk.skip == i as i64 + 1).then(|| tk.leave.unwrap_or((crate::reserve::Head::EMPTY, 0)));
            row_judged_in(kind, &before.g, l, before, row, leave, tk.side)
        })
    }

    /// As `rows_wrong_point`, each row judged by `judge` (the place of the row from 0, its leg, the row before, the row).
    pub fn rows_wrong_point_by(&self, rows: &[Row], c: &[i64], judge: &dyn Fn(usize, &crate::cross::XLeg, &Row, &Row) -> Option<bool>) -> Option<i64> {
        // The end is right: the last row's chain is the legs' root. Counted from the end, row k of n stands against
        // the account's chain of its leg m - (n - k): the last row against the root, the one before against the
        // chain before the last leg, and so on (a trace of more rows than legs runs out of legs: its first rows stand
        // against nothing). The last row that does not carry its chain is of no leg anybody can show. If it carries
        // the seed of every chain (32 zero bytes) it is wrong by that alone; else the row after it, which carries
        // the account's own chain, is opened by that chain's own opening, which is not the row before it: wrong.
        let (n, m) = (rows.len(), self.legs.len());
        let mut chain = vec![[0u8; 32]];
        for l in &self.legs {
            chain.push(blake2b(&[&chain[chain.len() - 1][..], &leg_bytes(l)].concat()));
        }
        let own = |k: usize| -> Option<[u8; 32]> { (k + m >= n + 1).then(|| chain[k + m - n]) };
        if let Some(k) = (1..=n).rev().find(|k| own(*k) != Some(rows[k - 1].g) || rows[k - 1].g == [0; 32]) {
            return Some(if rows[k - 1].g == [0; 32] || k == n { 2 * k as i64 - 1 } else { 2 * k as i64 + 1 });
        }
        // Every row carries the chain of its own leg (so the trace has a row a leg): each is held to its leg and to
        // the committee's price.
        for (i, l) in self.legs.iter().enumerate() {
            let before = if i == 0 { Row::ZERO } else { rows[i - 1] };
            if judge(i, l, &before, &rows[i]) != Some(true) {
                return Some(2 * i as i64 + 1);
            }
            if !price_right(l, &rows[i], c[i], self.t.g_bps) {
                return Some(2 * i as i64 + 2);
            }
        }
        None
    }

    /// A leg leaves the account, with `KpfTStepLiq`'s arithmetic: by a step of a liquidation (`kind` 3: `side` stands
    /// below its maintenance once the entries stand at the trace's prices; the claimer has the liquidation fee of
    /// the leg's worth) or by its party's own statement (`kind` 4: the claimer is the side's key, no fee, and the
    /// side stands at its maintenance, or the other party below its own). `None` where the rule does not allow it.
    pub fn stepped(&self, kind: i64, side: i64, skip: i64, rows: &[Row], claimer: &[u8; 32]) -> Option<Stepped> {
        let last = rows.last()?;
        if rows.len() != self.legs.len() || skip < 1 || skip as usize > rows.len() || (side != SIDE_A && side != SIDE_B) || last.wa < 0 || last.wb < 0 {
            return None;
        }
        let k = skip as usize;
        // The worth of the leg that leaves: what its row added to either side's worth (a futures leg adds it to
        // both, an option to whoever wrote it).
        let before = if k > 1 { rows[k - 2] } else { Row::ZERO };
        let (wka, wkb) = (rows[k - 1].wa - before.wa, rows[k - 1].wb - before.wb);
        if wka < 0 || wkb < 0 {
            return None;
        }
        let wk = wka.max(wkb);
        let other = if side == SIDE_A { SIDE_B } else { SIDE_A };
        let ((ws, es), (wo, eo)) = (last.asked(side), last.asked(other));
        let pot = self.pot();
        let raw = self.a_margin + last.gain;
        // What the pot cannot pay A, the entries standing at the prices stated; and what A's margin cannot pay B.
        let owed = (raw - pot).max(0);
        let lack = (-raw).max(0);
        let mut am = raw.clamp(0, pot);
        let mut bm = pot - am;
        let mine = if side == SIDE_A { am } else { bm };
        // The maintenance counted the tolerance less, and the tolerance more: the prices are only near the committee's.
        // And what the options a side holds were counted at is kept whole beside it.
        // What the shard is asked to pay: what is owed at any price within the tolerance (twice the tolerance of
        // the legs' worth could be owed to prices alone, where a leg entered and here; a futures leg is in both
        // sides' worth and is counted twice).
        let asked = (owed - rate_up(last.wa + last.wb, 2 * self.t.g_bps)).max(0);
        // less what of it the pot could not pay: a side is not below its maintenance for what it was not paid.
        let (es, eo) = if side == SIDE_A { ((es - owed).max(0), (eo - lack).max(0)) } else { ((es - lack).max(0), (eo - owed).max(0)) };
        let rate_lo = (self.t.mmr_bps - self.t.g_bps).max(0);
        let lo = rate_up(ws, rate_lo) + es;
        let lo_other = rate_up(wo, rate_lo) + eo;
        let fee = match kind {
            KIND_ACCOUNT_LIQ if mine < lo => ((wk / BPS) * self.t.liq_fee_bps + ((wk % BPS) * self.t.liq_fee_bps) / BPS).min(mine),
            KIND_ACCOUNT_LEAVE => {
                let key = if side == SIDE_A { self.a_key } else { self.b_key };
                if key != *claimer || (mine < rate_up(ws, self.t.mmr_bps + self.t.g_bps) + es && pot - mine >= lo_other) {
                    return None;
                }
                0
            }
            _ => return None,
        };
        if side == SIDE_A {
            am -= fee;
        } else {
            bm -= fee;
        }
        let legs = self.legs.iter().zip(rows).enumerate().filter(|(i, _)| *i + 1 != k).map(|(_, (l, r))| crate::cross::XLeg { entry: r.a, ..*l }).collect();
        Some(Stepped { after: Account { legs, a_margin: am, b_margin: bm, ..self.clone() }, fee, owed: asked })
    }

    /// A step of a liquidation (`stepped` of kind 3, by anyone): the account that is left and the liquidator's fee.
    pub fn liq_stepped(&self, side: i64, skip: i64, rows: &[Row]) -> Option<(Account, i64)> {
        self.stepped(KIND_ACCOUNT_LIQ, side, skip, rows, &[0; 32]).map(|s| (s.after, s.fee))
    }

    /// The outputs of a leg that left: the account by the last row's second chain holding the fee less, the fee to
    /// the claimer, and on a covered account the note that the leg left (made of what the statement says of the leg)
    /// and the note of what A is owed beyond the pot.
    fn step_outs(&self, st: &Stepped, tk: &ATake, claimer: &[u8; 32], held: i64) -> Option<Outs> {
        let last = tk.rows.last()?;
        let h = blake2b(&[p33(&blake2b(&[self.terms(), p33(&last.h)].concat())), st.after.moving()].concat());
        let mut v = vec![(Out { key: false, to: h, value: held - st.fee }, false)];
        if st.fee > 0 {
            v.push((Out { key: true, to: *claimer, value: st.fee }, false));
        }
        if let Some(sid) = self.shard {
            let (head, units) = tk.leave.unwrap_or((crate::reserve::Head::EMPTY, 0));
            let release = crate::reserve::SNote { sid, kind: crate::reserve::NOTE_RELEASE, head, net: units, gross: units.abs(), key: self.b_key, amount: 0 };
            v.push((Out { key: false, to: release.hash(), value: release.value() }, true));
            if st.owed > 0 {
                let owed = crate::reserve::SNote::owed(sid, self.a_key, self.b_key, st.owed);
                v.push((Out { key: false, to: owed.hash(), value: owed.value() }, true));
            }
        }
        Outs::with_notes(&v)
    }

    /// `KpfTStepLiq`: whether the end of a liquidation step's trace is right for this account holding `held`.
    pub fn liq_end_right(&self, tk: &ATake, claimer: &[u8; 32], outs: &Outs, held: i64) -> bool {
        self.step_end_right(KIND_ACCOUNT_LIQ, tk, claimer, outs, held)
    }

    /// `KpfTStepLiq`: whether the end of the trace of a leg that leaves (by a step of a liquidation, `kind` 3, or by
    /// its party's own statement, 4) is right for this account holding `held`.
    pub fn step_end_right(&self, kind: i64, tk: &ATake, claimer: &[u8; 32], outs: &Outs, held: i64) -> bool {
        let Some(last) = tk.rows.last() else { return false };
        if last.g != self.legs_root() {
            return false;
        }
        let Some(st) = self.stepped(kind, tk.side, tk.skip, &tk.rows, claimer) else { return false };
        tk.amount == st.fee && self.step_outs(&st, tk, claimer, held).is_some_and(|o| o == *outs)
    }

    /// The statement that the leg `skip` leaves this account holding `held`, at the prices `a`: by a step of a
    /// liquidation of `side` (`kind` 3, `claimer` anyone) or by the statement of `side`'s own key (`kind` 4).
    #[allow(clippy::too_many_arguments)]
    pub fn step(&self, kind: i64, side: i64, skip: i64, a: &[i64], t0: i64, held: i64, claimer: [u8; 32]) -> Result<(Stated, ATake, Stepped)> {
        // (An option leaves by its holder's side alone: the end's arithmetic does not see the leg, its row does.)
        if skip >= 1 && self.legs.get(skip as usize - 1).and_then(holder_of).is_some_and(|h| h != side) {
            return Err(Error::Invalid("an option leaves by its holder's side alone: by the holder's statement, or by a step of the liquidation of the holder".into()));
        }
        self.step_any(kind, side, skip, a, t0, held, claimer)
    }

    /// As `step`, whoever holds the leg: the statement whose end is right by its arithmetic and whose row of the
    /// leg is wrong where the leg is an option of the other side (for whoever tests that the row is answered so).
    #[allow(clippy::too_many_arguments)]
    pub fn step_any(&self, kind: i64, side: i64, skip: i64, a: &[i64], t0: i64, held: i64, claimer: [u8; 32]) -> Result<(Stated, ATake, Stepped)> {
        self.check()?;
        if a.len() != self.legs.len() || !(0..LOCK_TIME_THRESHOLD).contains(&t0) {
            return Err(Error::Invalid("a leg leaves by a statement of one price a leg, and a time".into()));
        }
        let rows = self.trace_without(a, skip);
        let st = self.stepped(kind, side, skip, &rows, &claimer).ok_or_else(|| Error::Invalid("the rule does not let this leg leave at the prices stated: the side is not below its maintenance (a step of a liquidation), or the claimer is not the side's key, or it stands below its maintenance while the other party does not (its own statement); or the leg is none of the account's".into()))?;
        let l = &self.legs[skip as usize - 1];
        let tk = ATake { side, amount: st.fee, rows, skip, leave: Some((crate::reserve::Head::of(l), crate::reserve::SNote::units(l))) };
        let outs = self.step_outs(&st, &tk, &claimer, held).ok_or_else(|| Error::Invalid("the step's outputs".into()))?;
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind, t0, arg: tk.arg(), outs, bond: self.bond(), claimer, held }, tk, st))
    }

    /// The account's lateness, dispute period and bond rate as its moving part has them (27 bytes): with its ten
    /// terms, what a shard's id is of beside its opening and its committee.
    pub fn periods(&self) -> Vec<u8> {
        self.moving()[93..120].to_vec()
    }

    /// `KpfTStepEnter`: whether the end of an entry is right for this account, before its outputs are looked at:
    /// the leg that enters is the last row's and the row before it carries the account's chain; the leg's
    /// committee is the one the account's shard is of, and the account's terms and periods are the shard's; the
    /// claimer is side A and the statement takes nothing; no
    /// margin is below zero once A's gain at the prices stated moved to it, and side A stands at its floor with
    /// the leg. Returns the two margins.
    pub fn enter_judged(&self, en: &AEnter, claimer: &[u8; 32]) -> Option<(i64, i64)> {
        let n = en.rows.len();
        let last = en.rows.last()?;
        let chain = if n >= 2 { en.rows[n - 2].g } else { [0; 32] };
        let sid = self.shard?;
        let (am, bm) = (self.a_margin + last.gain, self.b_margin - last.gain);
        let floor = (rate_up(last.wa, self.t.mmr_bps + self.t.g_bps) + last.ea).max(self.b_min_counter);
        let ok = en.skip == n as i64
            && en.side == SIDE_A
            && en.amount == 0
            && chain == self.legs_root()
            && self.a_key == *claimer
            && crate::reserve::shard_sid(&en.seed, &en.leg.committee, &self.terms(), &self.periods()) == sid
            && last.wa >= 0
            && last.wb >= 0
            && am >= 0
            && bm >= 0
            && am >= floor;
        ok.then_some((am, bm))
    }

    /// The outputs of an entry: the account with the leg and every entry at its price (by the last row's second
    /// chain), holding what it held, and the note that the leg entered, made of what the statement says of the leg.
    fn enter_outs(&self, en: &AEnter, am: i64, bm: i64, held: i64) -> Option<Outs> {
        let (last, sid) = (en.rows.last()?, self.shard?);
        let next = Account { a_margin: am, b_margin: bm, ..self.clone() };
        let h = blake2b(&[p33(&blake2b(&[self.terms(), p33(&last.h)].concat())), next.moving()].concat());
        let note = crate::reserve::SNote { gross: en.leg.size.abs(), ..crate::reserve::SNote::leg(crate::reserve::NOTE_JOIN, sid, &en.leg, self.b_key) };
        Outs::with_notes(&[(Out { key: false, to: h, value: held }, false), (Out { key: false, to: note.hash(), value: note.value() }, true)])
    }

    /// `KpfTStepEnter`: whether the end of an entry is right for this account holding `held`.
    pub fn enter_end_right(&self, en: &AEnter, claimer: &[u8; 32], outs: &Outs, held: i64) -> bool {
        self.enter_judged(en, claimer).and_then(|(am, bm)| self.enter_outs(en, am, bm, held)).is_some_and(|o| o == *outs)
    }

    /// The first point of a stated entry that is wrong at the committee's prices `c` (one a leg, the entering
    /// leg's last): 0 the end, 2i - 1 row i, 2i the price of row i. `None`: the entry is the rule's.
    pub fn enter_wrong_point(&self, en: &AEnter, claimer: &[u8; 32], outs: &Outs, held: i64, c: &[i64]) -> Option<i64> {
        if !self.enter_end_right(en, claimer, outs, held) {
            return Some(0);
        }
        let mut with = self.clone();
        with.legs.push(en.leg);
        with.rows_wrong_point_by(&en.rows, c, &|i, l, before, row| enter_row_judged(&before.g, l, before, row, (en.skip == i as i64 + 1).then_some(&en.leg)))
    }

    /// The statement that `leg` enters this covered account holding `held`, at the prices `a` (one a leg, the
    /// entering leg's last: its entry), by the account's two parties; `seed`: what the shard's id is the hash of
    /// beside its committee. Returns the statement, what it states, and the account as it will be.
    pub fn enter(&self, leg: crate::cross::XLeg, a: &[i64], t0: i64, held: i64, seed: [u8; 32]) -> Result<(Stated, AEnter, Account)> {
        let n = self.legs.len();
        if self.shard.is_none() || a.len() != n + 1 || n + 1 > LEGS_MAX || !(0..LOCK_TIME_THRESHOLD).contains(&t0) {
            return Err(Error::Invalid("a leg enters a covered account by a statement of one price a leg (the entering leg's last), and a time".into()));
        }
        let l = crate::cross::XLeg { entry: a[n], ..leg };
        let mut with = self.clone();
        with.legs.push(l);
        if !leg_size_ok(&l) || !leg_kind_ok(&l) || l.entry <= 0 || l.entry > l.price_max || !legs_sum_ok(&with.legs) {
            return Err(Error::Invalid("what enters is a leg: a size within what is computed at its market's limit, a direction or an option's kind and strike, and a price within the limit".into()));
        }
        let rows = with.trace(a);
        let en = AEnter { side: SIDE_A, amount: 0, skip: n as i64 + 1, leg: l, seed, rows };
        let Some((am, bm)) = self.enter_judged(&en, &self.a_key) else {
            return Err(Error::Invalid("the leg does not enter: its committee is not the one the account's shard is of, or the account's terms and periods are not the shard's; a margin would fall below zero at the prices stated; or side A would not stand at its maintenance with the leg (add margin first)".into()));
        };
        let outs = self.enter_outs(&en, am, bm, held).ok_or_else(|| Error::Invalid("the entry's outputs".into()))?;
        let legs = with.legs.iter().zip(&en.rows).map(|(x, r)| crate::cross::XLeg { entry: r.a, ..*x }).collect();
        let after = Account { legs, a_margin: am, b_margin: bm, ..self.clone() };
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_ENTER, t0, arg: en.arg(), outs, bond: self.bond(), claimer: self.a_key, held }, en, after))
    }

    /// The statement of a liquidation step on this account holding `held`: `liquidator` states that `side` stands
    /// below its maintenance at the prices `a` and that the leg `skip` leaves.
    pub fn liq_step(&self, side: i64, skip: i64, a: &[i64], t0: i64, held: i64, liquidator: [u8; 32]) -> Result<(Stated, ATake)> {
        self.step(KIND_ACCOUNT_LIQ, side, skip, a, t0, held, liquidator).map(|(st, tk, _)| (st, tk))
    }

    /// The statement of a take on this account holding `held`, by the owner of `side`, at the prices `a`.
    pub fn take(&self, side: i64, amount: i64, a: &[i64], t0: i64, held: i64) -> Result<(Stated, ATake)> {
        let claimer = match side {
            SIDE_A => self.a_key,
            SIDE_B => self.b_key,
            _ => return Err(Error::Invalid("a take is by side a (1) or b (2)".into())),
        };
        self.take_by(side, amount, a, t0, held, claimer)
    }

    /// As `take`, stated by `claimer`: the side's key; or anyone, for side B of a covered account, whose amount is
    /// then a deposit to the account's shard.
    pub fn take_by(&self, side: i64, amount: i64, a: &[i64], t0: i64, held: i64, claimer: [u8; 32]) -> Result<(Stated, ATake)> {
        self.check()?;
        if (side != SIDE_A && side != SIDE_B) || a.len() != self.legs.len() || !(0..LOCK_TIME_THRESHOLD).contains(&t0) {
            return Err(Error::Invalid("a take is by side a (1) or b (2), and states one price a leg, and a time".into()));
        }
        let tk = ATake { side, amount, rows: self.trace(a), skip: 0, leave: None };
        let after = self.taken(side, amount, &tk.rows).filter(|_| amount >= 0 && amount <= held).ok_or_else(|| Error::Invalid("the take leaves a margin below zero at the prices it states".into()))?;
        let mut v = vec![(Out { key: false, to: after.hash(), value: held - amount }, false)];
        if amount > 0 {
            v.push(match self.shard.filter(|_| side == SIDE_B) {
                Some(sid) => {
                    let n = crate::reserve::SNote::deposit(sid, self.b_key, amount);
                    (Out { key: false, to: n.hash(), value: n.value() }, true)
                }
                None => (Out { key: true, to: claimer, value: amount }, false),
            });
        }
        let outs = Outs::with_notes(&v).ok_or_else(|| Error::Invalid("the take's outputs".into()))?;
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_ACCOUNT_TAKE, t0, arg: tk.arg(), outs, bond: self.bond(), claimer, held }, tk))
    }
}

// ---------------------------------------------------------------- orders between users

/// The length of an order's template.
pub const TEMPLATE_LEN: usize = 321;
/// The least a part of an order that rests holds (`KpfTMatch`).
pub const ORDER_MIN: i64 = 20_000_000;
/// The most an order's size is (`KpfTMatch`: a share of its margin is computed without overflow).
pub const ORDER_SIZE_MAX: i64 = 2_000_000_000;

/// The position an order would become, all but its two keys, its size, its entry price and its two margins: the terms,
/// the leg's market, and the values of the declared form. Two orders match only if their templates are one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template {
    pub t: crate::cross::XTerms,
    pub market: [u8; 32],
    pub committee: [u8; 32],
    pub scale: i64,
    pub price_max: i64,
    /// The time an exit claim waits.
    pub exit: i64,
    pub dispute_daa: i64,
    pub bond_bps: i64,
}

impl Template {
    /// The template of a position of one leg (what an order for the same market and terms carries).
    pub fn of(a: &Position) -> Template {
        let l = &a.x.legs[0];
        Template { t: a.x.t, market: l.market, committee: l.committee, scale: l.scale, price_max: l.price_max, exit: a.x.expiry, dispute_daa: a.dispute_daa, bond_bps: a.bond_bps }
    }

    /// The position of a match: `long` its A, long `n` at `x`; `short` its B; the two margins.
    pub fn position(&self, long: [u8; 32], short: [u8; 32], n: i64, x: i64, am: i64, bm: i64) -> Position {
        let leg = crate::cross::XLeg { market: self.market, committee: self.committee, scale: self.scale, price_max: self.price_max, size: n, dir: crate::state::LONG, entry: x };
        Position { x: Cross { t: self.t, legs: [leg, crate::cross::XLeg::none()], a_key: long, b_key: short, a_margin: am, b_margin: bm, expiry: self.exit }, dispute_daa: self.dispute_daa, bond_bps: self.bond_bps, a_min_counter: 0, b_min_counter: 0 }
    }

    /// The 321 bytes `KpfTMatch` is shown: the terms and the leg's head, the empty second leg, and the four values of
    /// the moving part (the position's state with the leg's size, side and entry, the keys and the margins taken out).
    pub fn bytes(&self) -> Vec<u8> {
        let a = self.position([0; 32], [0; 32], 1, 0, 0, 0);
        let (f, m) = (a.fixed(), a.moving());
        [&f[..174], &f[201..], &m[84..120]].concat()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&self.bytes())
    }

    /// Reads `bytes` back, refusing anything that is not exactly a template.
    pub fn from_bytes(b: &[u8]) -> Result<Template> {
        if b.len() != TEMPLATE_LEN {
            return Err(Error::Decode("an order's template is 321 bytes".into()));
        }
        // The position's state with a leg of one unit and nothing else of its own.
        let state = [&b[..174], &i9(1)[..], &i9(crate::state::LONG), &i9(0), &b[174..285], &p33(&[0; 32]), &p33(&[0; 32]), &i9(0), &i9(0), &b[285..294]].concat();
        let x = Cross::decode_state(&state)?;
        let mut c = Cursor { b: &b[285..], at: 0 };
        let (exit, late, dispute_daa, bond_bps) = (c.int()?, c.int()?, c.int()?, c.int()?);
        let l = &x.legs[0];
        let t = Template { t: x.t, market: l.market, committee: l.committee, scale: l.scale, price_max: l.price_max, exit, dispute_daa, bond_bps };
        if late != x.t.late_daa || t.bytes() != b {
            return Err(Error::Decode("not a template: its lateDaa is not the terms', or it carries a second leg".into()));
        }
        Ok(t)
    }
}

/// An order: a user's margin and a tip for whoever matches it, waiting for the other side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Order {
    /// The hash of its template.
    pub tpl: [u8; 32],
    pub owner: [u8; 32],
    /// `LONG` (1) or `SHORT` (2).
    pub dir: i64,
    /// What is left of its size.
    pub size: i64,
    /// A long is filled at this price or below, a short at it or above.
    pub limit: i64,
    /// The owner's margin for what is left.
    pub margin: i64,
    /// The matcher's tip for what is left.
    pub tip: i64,
    /// From this DAA on anyone returns the order to its owner.
    pub expiry: i64,
    /// The least a match takes of it, unless it takes all that is left: an order of 10,000 with a least fill of 2,000
    /// becomes five positions at most.
    pub min_fill: i64,
    /// The least margin it asks of the other side for what is left (a match takes its share, as of the margin). An
    /// order that asks nothing is matched against a sompi (review T1b, F6): the tool asks the owner's own margin.
    pub min_counter: i64,
}

/// What a match leaves of an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rest {
    /// Nothing: it was filled whole and held nothing more.
    Nothing,
    /// The order with what is left, holding this much.
    Order(Order, i64),
}

/// A match of two orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matched {
    pub position: Position,
    /// The two tips' shares: whoever sends the match has them (no output is written for them).
    pub tips: i64,
    pub long: Rest,
    pub short: Rest,
}

impl Order {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(3), p33(&self.tpl), p33(&self.owner), i9(self.dir), i9(self.size), i9(self.limit), i9(self.margin), i9(self.tip), i9(self.expiry), i9(self.min_fill), i9(self.min_counter)].concat())
    }

    /// What a new order holds: its margin and its tip.
    pub fn value(&self) -> i64 {
        self.margin + self.tip
    }

    pub fn check(&self) -> Result<()> {
        let ok = (self.dir == crate::state::LONG || self.dir == crate::state::SHORT)
            && (1..=ORDER_SIZE_MAX).contains(&self.size)
            && self.limit > 0
            && (1..=MARGIN_MAX).contains(&self.margin)
            && (0..=MARGIN_MAX).contains(&self.tip)
            && (0..LOCK_TIME_THRESHOLD).contains(&self.expiry)
            && (0..=ORDER_SIZE_MAX).contains(&self.min_fill)
            && (0..=MARGIN_MAX).contains(&self.min_counter);
        if ok {
            Ok(())
        } else {
            Err(Error::Invalid("an order is long or short, of a size of 1 to 2,000,000,000 units, with a limit price and a margin above zero".into()))
        }
    }

    /// `n / size` of `v`, rounded down; all of it when `n` is the size (`KpfTMatch.share`).
    fn share(&self, v: i64, n: i64) -> i64 {
        (v / self.size) * n + ((v % self.size) * n) / self.size
    }

    /// (the margin's share, the tip's share, the share of what it asks of the other side, what is left) of this order
    /// holding `value` when `n` of it is matched.
    fn filled(&self, value: i64, n: i64) -> Result<(i64, i64, i64, Rest)> {
        let (m, f, q) = (self.share(self.margin, n), self.share(self.tip, n), self.share(self.min_counter, n));
        let left = value - m - f;
        if left < 0 {
            return Err(Error::Invalid("the order holds less than its margin and its tip".into()));
        }
        // What is left of an order rests (an order to be taken whole says so by its least fill). An order used up
        // holds nothing more, unless its owner gave it more than its margin and tip: that is the sender's.
        let rest = if self.size - n > 0 {
            if left < ORDER_MIN {
                return Err(Error::Invalid(format!("the match would leave an order of {left} sompi, too small to rest: fill it whole or leave more")));
            }
            Rest::Order(Order { size: self.size - n, margin: self.margin - m, tip: self.tip - f, min_counter: self.min_counter - q, ..*self }, left)
        } else {
            Rest::Nothing
        };
        Ok((m, f, q, rest))
    }
}

/// `KpfTMatch`: a long order holding `long_value` and a short one holding `short_value`, of the template `tpl`, matched
/// for `n` units at the price `x`. Refuses what the script would.
pub fn match_orders(tpl: &Template, long: &Order, long_value: i64, short: &Order, short_value: i64, n: i64, x: i64) -> Result<Matched> {
    long.check()?;
    short.check()?;
    let h = tpl.hash();
    if long.tpl != h || short.tpl != h {
        return Err(Error::Invalid("the two orders are not of this template: they do not mean the same position".into()));
    }
    if long.dir != crate::state::LONG || short.dir != crate::state::SHORT {
        return Err(Error::Invalid("a match takes a long order and a short one".into()));
    }
    if long.owner == short.owner {
        return Err(Error::Invalid("the two orders are one owner's: a position has two parties".into()));
    }
    if n <= 0 || n > long.size || n > short.size {
        return Err(Error::Invalid("the size matched is above zero and within what both orders have left".into()));
    }
    for o in [long, short] {
        if n < o.min_fill && n != o.size {
            return Err(Error::Invalid(format!("an order takes at least {} units a match, or all {} it has left", o.min_fill, o.size)));
        }
    }
    if x <= 0 || x > long.limit || x < short.limit || x > tpl.price_max {
        return Err(Error::Invalid(format!("the price is not above the long's limit ({}), not below the short's ({}) and within the market's", long.limit, short.limit)));
    }
    let (am, lf, lq, lrest) = long.filled(long_value, n)?;
    let (bm, sf, sq, srest) = short.filled(short_value, n)?;
    if bm < lq || am < sq {
        return Err(Error::Invalid(format!("a side posts less margin than the other order asks of it for this size (the long asks {lq} and is offered {bm}; the short asks {sq} and is offered {am})")));
    }
    if am <= 0 || bm <= 0 {
        return Err(Error::Invalid("a share of margin would be nothing: the size matched is too small".into()));
    }
    let position = Position { a_min_counter: lq, b_min_counter: sq, ..tpl.position(long.owner, short.owner, n, x, am, bm) };
    position.check()?;
    Ok(Matched { position, tips: lf + sf, long: lrest, short: srest })
}

// ---------------------------------------------------------------- the oracle fees

/// What every operation leaves in the copy of the script it used (`ORACLE_FEE` of every script of a library but the two
/// that collect): the committee's, about a third of what the operation pays its miner.
pub const ORACLE_FEE: i64 = 200_000;
/// The least a sweep moves out of a copy (`KpfTSweep.SWEEP_MIN`).
pub const SWEEP_MIN: i64 = 20_000_000;
/// Whoever sweeps a copy or pays the fee pool keeps one part in this of what it moves (`CUT`).
pub const ORACLE_FEE_CUT: i64 = 20;
/// Shares are counted in units of 0.01 KAS (`KpfTPay.UNIT`).
pub const ORACLE_FEE_UNIT: i64 = 1_000_000;
/// The most one payment shares (`KpfTPay.POT_MAX`).
pub const ORACLE_FEE_POT_MAX: i64 = 30_000_000_000_000;

/// The least a price note holds (`NOTE_VALUE` of `KpfTNote`); its writer takes it back (`KpfNote.reclaim`).
pub const NOTE_VALUE: i64 = 100_000_000;

/// A price the committee signed, as a note under a library's id states it (`KpfNote`, written by `KpfTNote` beside
/// the committee's attest and cited by `KpfTCite`): the committee, the market, the time, the price and the
/// committee's spread. `owner` is the key what the note holds returns to; it is in the note's script, not in its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub committee: [u8; 32],
    pub market: [u8; 32],
    pub t0: i64,
    pub price: i64,
    pub spread_bps: i64,
    pub owner: [u8; 32],
}

impl Note {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(9), p33(&self.committee), p33(&self.market), i9(self.t0), i9(self.price), i9(self.spread_bps)].concat())
    }

    /// What a reader of a citation finds at the head of the citing input's signature script after the chain block:
    /// the committee, the market, the time, the price and the spread, each as its push.
    pub fn served(&self) -> Vec<u8> {
        [p33(&self.committee), p33(&self.market), i9(self.t0), i9(self.price), i9(self.spread_bps)].concat()
    }
}

/// The state of the fee pool whose oracle fees are the committee `committee`'s, in a library whose copies (and the fee pool) go on
/// holding `floor`: what its genesis gave each.
pub fn fee_pool_hash(committee: &[u8; 32], floor: i64) -> [u8; 32] {
    blake2b(&[i9(8), p33(committee), i9(floor)].concat())
}

/// What a sweep of a copy holding `value` moves, and of that what reaches the fee pool (`KpfTSweep.sweep`); `None` when
/// the copy holds too little above its floor.
pub fn swept(value: i64, floor: i64) -> Option<(i64, i64)> {
    let moved = value - floor;
    (moved >= SWEEP_MIN).then_some((moved, moved - moved / ORACLE_FEE_CUT))
}

/// What a payment of a fee pool holding `value` takes out of it (the shares and the sender's twentieth of the pot) and
/// gives each member of `table` that has a weight, in the table's order (`KpfTPay.pay`): the pot less the sender's
/// part, by weight, in units of 0.01 KAS. What the rounding leaves stays in the fee pool.
pub fn oracle_fee_shares(value: i64, floor: i64, table: &crate::committee::Table, total: i64) -> Option<(i64, Vec<([u8; 32], i64)>)> {
    let pot = (value - floor).min(ORACLE_FEE_POT_MAX);
    let du = (pot - pot / ORACLE_FEE_CUT) / ORACLE_FEE_UNIT;
    let tu = total / ORACLE_FEE_UNIT;
    if du <= 0 || tu <= 0 {
        return None;
    }
    let mut v = vec![];
    for s in table {
        let w = s.weight / ORACLE_FEE_UNIT;
        if w > 0 {
            v.push((s.key, du.checked_mul(w)? / tu * ORACLE_FEE_UNIT));
        }
    }
    let paid: i64 = v.iter().map(|x| x.1).sum();
    Some((paid + pot / ORACLE_FEE_CUT, v))
}

// ---------------------------------------------------------------- two positions into one

/// What whoever merges two positions keeps of what they held (`KpfTMerge`, a constant of the library: half from each
/// party's margin). About the fee of the merge and a little more: nobody else is paid to tidy.
pub const MERGE_TIP: i64 = 1_000_000;

/// `KpfTMerge`: the position two positions of the same two parties become. The same terms and market, the keys in the
/// same places, both long for their A, one leg each. The sizes and each party's margins are put together; the entry
/// is the largest price at which the whole is worth no more than the two legs were at theirs, and the few sompi by
/// which that lowers A's cost go from A's margin to B's; each margin gives half of `MERGE_TIP`. Nobody signs it:
/// the two positions are of one registry, where a position is only by its parties' signatures (review T1b, F11).
pub fn merged(a: &Position, b: &Position) -> Result<Position> {
    let no = |why: &str| Err(Error::Invalid(format!("the two positions are not merged: {why}")));
    a.check()?;
    b.check()?;
    let (la, lb) = (&a.x.legs[0], &b.x.legs[0]);
    if a.x.a_key != b.x.a_key || a.x.b_key != b.x.b_key {
        return no("they are not of the same two parties in the same places (a position whose sides are the other way round is not merged yet)");
    }
    // (What each party asks of the other's margin is each position's own: the merged position carries the sums.)
    let same = Template::of(a) == Template::of(b) && a.x.legs[1] == b.x.legs[1] && a.x.legs[1].size == 0;
    if !same {
        return no("they are not of one market and the same terms, with one leg each");
    }
    if la.dir != crate::state::LONG || lb.dir != crate::state::LONG {
        return no("their legs are not both long for their A");
    }
    let n = la.size + lb.size;
    if lb.size <= 0 || n > ORDER_SIZE_MAX {
        return no("the two sizes together are out of range");
    }
    let c = la.worth(la.entry) + lb.worth(lb.entry);
    let e = (c / n) * la.scale + ((c % n) * la.scale) / n;
    let whole = crate::cross::XLeg { size: n, entry: e, ..*la };
    let delta = c - whole.worth(e);
    let half = MERGE_TIP / 2;
    let (am, bm) = (a.x.a_margin + b.x.a_margin - delta - half, a.x.b_margin + b.x.b_margin + delta - (MERGE_TIP - half));
    if e > la.price_max || delta < 0 || am <= 0 || bm <= 0 {
        return no("a margin would not cover the merger's share");
    }
    let mut x = a.x;
    x.legs[0] = whole;
    x.a_margin = am;
    x.b_margin = bm;
    let out = Position { x, a_min_counter: a.a_min_counter + b.a_min_counter, b_min_counter: a.b_min_counter + b.b_min_counter, ..*a };
    out.check()?;
    Ok(out)
}

// ---------------------------------------------------------------- two markets into one position

/// `KpfTJoin`: the cross position two positions of the same two users in two markets become. Each has one leg; the same
/// terms; the two keys in the same places or the other way round (then the second's leg is short for the first's A).
/// Each leg keeps its size and entry, so each party is owed, before the caps, what the two positions owed it; the
/// margins and asks are put together, and each margin gives half of `MERGE_TIP` to whoever sends it.
pub fn joined(x: &Position, y: &Position) -> Result<Position> {
    let no = |why: &str| Err(Error::Invalid(format!("the two positions are not joined: {why}")));
    x.check()?;
    y.check()?;
    let none = crate::cross::XLeg::none();
    if x.x.legs[1] != none || y.x.legs[1] != none {
        return no("one of them has two legs already");
    }
    if x.x.t != y.x.t || x.x.expiry != y.x.expiry || x.dispute_daa != y.dispute_daa || x.bond_bps != y.bond_bps {
        return no("they are not of the same terms");
    }
    let turned = if (x.x.a_key, x.x.b_key) == (y.x.a_key, y.x.b_key) {
        false
    } else if (x.x.a_key, x.x.b_key) == (y.x.b_key, y.x.a_key) {
        true
    } else {
        return no("they are not of the same two users");
    };
    let (lx, ly) = (&x.x.legs[0], &y.x.legs[0]);
    if lx.market == ly.market {
        return no("they are of one market (two positions of one market are merged, or cancel each other)");
    }
    if ly.dir != crate::state::LONG || ly.size <= 0 {
        return no("the second's leg is not long for its A");
    }
    let half = MERGE_TIP / 2;
    let (ja, jb, jaq, jbq) = if turned { (y.x.b_margin, y.x.a_margin, y.b_min_counter, y.a_min_counter) } else { (y.x.a_margin, y.x.b_margin, y.a_min_counter, y.b_min_counter) };
    let mut c = x.x;
    c.legs[1] = crate::cross::XLeg { dir: if turned { crate::state::SHORT } else { crate::state::LONG }, ..*ly };
    c.a_margin = x.x.a_margin + ja - half;
    c.b_margin = x.x.b_margin + jb - (MERGE_TIP - half);
    if c.a_margin <= 0 || c.b_margin <= 0 {
        return no("a margin would not cover the sender's share");
    }
    let out = Position { x: c, a_min_counter: x.a_min_counter + jaq, b_min_counter: x.b_min_counter + jbq, ..*x };
    out.check()?;
    Ok(out)
}

// ---------------------------------------------------------------- one side of a position of two legs

/// The age a position has before `KpfTTurn` takes it (whoever means to use the position is overtaken by a turn once
/// at most).
pub const TURN_AGE: u64 = 100;
/// The unit the two legs' entry notionals are counted in for the share of a position that leaves (`KpfTLeave`).
pub const WEIGHT_UNIT: i64 = 1_000_000;

/// `KpfTTurn`, mode 1: the same position with its two users in each other's places (each leg's direction turns).
pub fn turned_users(a: &Position) -> Position {
    let mut c = a.x;
    std::mem::swap(&mut c.a_key, &mut c.b_key);
    std::mem::swap(&mut c.a_margin, &mut c.b_margin);
    for l in c.legs.iter_mut() {
        if l.size > 0 {
            l.dir = crate::state::LONG + crate::state::SHORT - l.dir;
        }
    }
    Position { x: c, a_min_counter: a.b_min_counter, b_min_counter: a.a_min_counter, ..*a }
}

/// `KpfTTurn`, mode 2: the same position with its two legs in each other's places.
pub fn turned_legs(a: &Position) -> Result<Position> {
    if a.x.legs[1].size <= 0 {
        return Err(Error::Invalid("the position has one leg: there is nothing to change places with".into()));
    }
    let mut c = a.x;
    c.legs.swap(0, 1);
    Ok(Position { x: c, ..*a })
}

/// Whether two legs are of one market as a script reads it (market, committee, scale and price limit).
fn same_market(a: &crate::cross::XLeg, b: &crate::cross::XLeg) -> bool {
    (a.market, a.committee, a.scale, a.price_max) == (b.market, b.committee, b.scale, b.price_max)
}

fn same_terms(x: &Position, y: &Position) -> bool {
    x.x.t == y.x.t && x.x.expiry == y.x.expiry && x.dispute_daa == y.dispute_daa && x.bond_bps == y.bond_bps
}

/// `KpfTFold`: the position `y` (one leg, of the market of `x`'s first leg, the same two users) folded into `x`'s
/// first leg: added when it is the same way, set against it when it is the other (what cancels is realized between the
/// two margins). The margins and asks of each user are put together; each margin gives half of `MERGE_TIP`.
pub fn folded(x: &Position, y: &Position) -> Result<Position> {
    let no = |why: &str| Err(Error::Invalid(format!("the position is not folded into the other: {why}")));
    x.check()?;
    y.check()?;
    let none = crate::cross::XLeg::none();
    if y.x.legs[1] != none {
        return no("it has two legs");
    }
    if !same_terms(x, y) {
        return no("they are not of the same terms");
    }
    let turned = if (x.x.a_key, x.x.b_key) == (y.x.a_key, y.x.b_key) {
        false
    } else if (x.x.a_key, x.x.b_key) == (y.x.b_key, y.x.a_key) {
        true
    } else {
        return no("they are not of the same two users");
    };
    let (lx, ly) = (x.x.legs[0], y.x.legs[0]);
    if !same_market(&lx, &ly) {
        return no("it is not of the market of the other's first leg (`pos-turn --mode legs` changes the legs' places)");
    }
    if ly.dir != crate::state::LONG || ly.size <= 0 || lx.size <= 0 {
        return no("its leg is not long for its A");
    }
    let (long, short) = (crate::state::LONG, crate::state::SHORT);
    let dy = if turned { short } else { long };
    let (ja, jb, jaq, jbq) = if turned { (y.x.b_margin, y.x.a_margin, y.b_min_counter, y.a_min_counter) } else { (y.x.a_margin, y.x.b_margin, y.a_min_counter, y.b_min_counter) };
    let (nn, nd, ne, da);
    if dy == lx.dir {
        nn = lx.size + ly.size;
        if nn > ORDER_SIZE_MAX {
            return no("the two sizes together are out of range");
        }
        let c = lx.worth(lx.entry) + ly.worth(ly.entry);
        ne = (c / nn) * lx.scale + ((c % nn) * lx.scale) / nn;
        let delta = c - crate::cross::XLeg { size: nn, ..lx }.worth(ne);
        if ne > lx.price_max || delta < 0 {
            return no("the entry of the whole is out of range");
        }
        nd = lx.dir;
        da = if lx.dir == long { -delta } else { delta };
    } else {
        let m = lx.size.min(ly.size);
        let unit = crate::cross::XLeg { size: m, ..lx };
        let d0 = unit.worth(lx.entry) - unit.worth(ly.entry);
        da = if lx.dir == long { -d0 } else { d0 };
        if lx.size >= ly.size {
            nn = lx.size - ly.size;
            nd = lx.dir;
            ne = lx.entry;
        } else {
            nn = ly.size - lx.size;
            nd = dy;
            ne = ly.entry;
        }
    }
    let half = MERGE_TIP / 2;
    let mut c = x.x;
    c.a_margin = x.x.a_margin + ja + da - half;
    c.b_margin = x.x.b_margin + jb - da - (MERGE_TIP - half);
    if c.a_margin <= 0 || c.b_margin <= 0 {
        return no("a margin would not cover what cancels and the sender's share");
    }
    if nn > 0 {
        c.legs[0] = crate::cross::XLeg { size: nn, dir: nd, entry: ne, ..lx };
    } else if x.x.legs[1].size > 0 {
        c.legs = [x.x.legs[1], none];
    } else {
        return no("the two cancel whole and nothing is left: `pos-novate` ends them and pays both");
    }
    // What one margin took from the other is added to what the payer asks of the taker's replacement (review T1b,
    // F18): it stands behind what the taker still owes on what is left.
    let out = Position { x: c, a_min_counter: x.a_min_counter + jaq + (-da).max(0), b_min_counter: x.b_min_counter + jbq + da.max(0), ..*x };
    out.check()?;
    Ok(out)
}

/// What `KpfTLeave` makes of a position `x` of two legs (U its A, S its B) and a position `y` of one leg in which U
/// holds, against a third user L, the other way what it holds in `x`'s first leg.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaving {
    /// The units taken: all of the smaller of `x`'s first leg and `y`.
    pub n: i64,
    /// U's gain on them, which moves from S's margin to U's inside `x` (negative: the other way).
    pub gain: i64,
    /// `x` after, and what it holds.
    pub x_after: (Position, i64),
    /// L and S facing each other from `y`'s entry, and what it holds.
    pub z: (Position, i64),
    /// What is left of `y`, if anything.
    pub rest_y: Option<(Position, i64)>,
    /// What U is paid: its margin in `y` for the units, less the sender's share. Nothing of `x`.
    pub to_leaver: i64,
}

/// `KpfTLeave`. `rules`: the two rules that protect those who stay (`false` only to build what the script must refuse).
pub fn left(x: &Position, xv: i64, y: &Position, yv: i64, rules: bool) -> Result<Leaving> {
    let no = |why: &str| Err(Error::Invalid(format!("the leg does not leave the position: {why}")));
    x.check()?;
    y.check()?;
    let none = crate::cross::XLeg::none();
    let (lx, l2, ly) = (x.x.legs[0], x.x.legs[1], y.x.legs[0]);
    if l2.size <= 0 {
        return no("the position has one leg (`pos-novate` takes a user out of a position of one leg)");
    }
    if y.x.legs[1] != none || !same_terms(x, y) || !same_market(&lx, &ly) || ly.dir != crate::state::LONG {
        return no("the second position is not one of one leg, of the first's terms and of its first leg's market");
    }
    let (long, u, s) = (crate::state::LONG, x.x.a_key, x.x.b_key);
    // In y: L's margin and ask, U's margin and ask; and L.
    let (l, lmy, umy, lqy, uqy) = if lx.dir == long {
        if y.x.b_key != u {
            return no("the position's A is long in the leg, and is not the second position's B");
        }
        (y.x.a_key, y.x.a_margin, y.x.b_margin, y.a_min_counter, y.b_min_counter)
    } else {
        if y.x.a_key != u {
            return no("the position's A is short in the leg, and is not the second position's A");
        }
        (y.x.b_key, y.x.b_margin, y.x.a_margin, y.b_min_counter, y.a_min_counter)
    };
    if l == s {
        return no("the second position is of the same two users: `pos-fold` sets it against the leg");
    }
    if lx.size > ORDER_SIZE_MAX || l2.size > ORDER_SIZE_MAX || ly.size > ORDER_SIZE_MAX || xv < x.pot() || yv < y.pot() {
        return no("a size is out of range, or a position holds less than its margins");
    }
    let m = lx.size.min(ly.size);
    let (lms, ums, lq, uqs) = (part(lmy, m, ly.size), part(umy, m, ly.size), part(lqy, m, ly.size), part(uqy, m, ly.size));
    let unit = crate::cross::XLeg { size: m, ..lx };
    let g0 = unit.worth(ly.entry) - unit.worth(lx.entry);
    let gu = if lx.dir == long { g0 } else { -g0 };
    let (wm, wall) = (unit.worth(lx.entry) / WEIGHT_UNIT, (lx.worth(lx.entry) + l2.worth(l2.entry)) / WEIGHT_UNIT);
    if wall <= 0 || wall > ORDER_SIZE_MAX {
        return no("the position's two legs at their entries are out of the range the share is counted in");
    }
    let (ms, sq) = (part(x.x.b_margin, wm, wall), part(x.b_min_counter, wm, wall));
    if ms <= 0 || lms <= 0 {
        return no("the share that leaves is too small to carry a margin");
    }
    if rules {
        if lms < sq + gu.max(0) {
            return no(&format!("the one that comes in posts {lms}, less than the one that stays asked of its counterparty for that share ({sq}) and what it gives up ({})", gu.max(0)));
        }
        if ms < lq {
            return no(&format!("the one that stays brings {ms}, less than the one that comes in asked of its counterparty ({lq})"));
        }
    }
    let (am, bm) = (x.x.a_margin + gu, x.x.b_margin - gu - ms);
    if am <= 0 || bm <= 0 {
        return no("a margin of the position does not cover what the price moved on the leg");
    }
    if ums < NOVATE_TIP {
        return no("the user's margin in the second position does not cover the sender's share");
    }
    let mut c = x.x;
    if lx.size > m {
        c.legs[0].size = lx.size - m;
    } else {
        c.legs = [l2, none];
    }
    c.a_margin = am;
    c.b_margin = bm;
    // What stays keeps both asks whole, with what U's margin took from S's added to S's (F18).
    let xa = Position { x: c, a_min_counter: x.a_min_counter + (-gu).max(0), b_min_counter: x.b_min_counter + gu.max(0), ..*x };
    xa.check()?;
    let mut zc = x.x;
    zc.legs = [crate::cross::XLeg { size: m, dir: long, entry: ly.entry, ..lx }, none];
    let z = if lx.dir == long {
        zc.a_key = l;
        zc.b_key = s;
        zc.a_margin = lms;
        zc.b_margin = ms;
        Position { x: zc, a_min_counter: lq, b_min_counter: sq, ..*x }
    } else {
        zc.a_key = s;
        zc.b_key = l;
        zc.a_margin = ms;
        zc.b_margin = lms;
        Position { x: zc, a_min_counter: sq, b_min_counter: lq, ..*x }
    };
    z.check()?;
    let mut zv = lms + ms;
    let rest_y = if ly.size > m {
        let mut yc = y.x;
        yc.legs[0].size = ly.size - m;
        let r = if lx.dir == long {
            yc.a_margin -= lms;
            yc.b_margin -= ums;
            Position { x: yc, a_min_counter: y.a_min_counter - lq, b_min_counter: y.b_min_counter - uqs, ..*y }
        } else {
            yc.a_margin -= ums;
            yc.b_margin -= lms;
            Position { x: yc, a_min_counter: y.a_min_counter - uqs, b_min_counter: y.b_min_counter - lq, ..*y }
        };
        r.check()?;
        Some((r, yv - lms - ums))
    } else {
        zv += yv - y.pot();
        None
    };
    Ok(Leaving { n: m, gain: gu, x_after: (xa, xv - ms), z: (z, zv), rest_y, to_leaver: ums - NOVATE_TIP })
}

// ---------------------------------------------------------------- a user leaves; the two it faced face each other

/// What whoever sends a `KpfTNovate` keeps of what the user that leaves is paid (a constant of the library).
pub const NOVATE_TIP: i64 = 1_500_000;

/// What `KpfTNovate` makes of two positions X (U long against S) and Y (L long against U), holding `xv` and `yv`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Novation {
    /// The units taken: all of the smaller position.
    pub n: i64,
    /// What S's margin gives up to U (U's gain: bought at X's entry, sold at Y's); negative when S takes it.
    pub transfer: i64,
    /// L long against S from Y's entry, and what it holds; none when L and S are one user.
    pub z: Option<(Position, i64)>,
    /// What is left of X, and of Y (one of them at most), and what each holds.
    pub rest_x: Option<(Position, i64)>,
    pub rest_y: Option<(Position, i64)>,
    /// What U is paid (its two margins and the transfer, less the sender's share).
    pub to_leaver: i64,
    /// What S is paid when S and L are one user (else 0).
    pub to_stayer: i64,
}

/// What a re-mark leaves (research R2, `docs/RESEARCH-UNIFIED-MARGIN.md`; no script runs it yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marked {
    /// The position struck again: every leg's entry is the price, and the gain since the old entries has moved
    /// between the two margins.
    pub position: Position,
    /// What moved from B's margin to A's (A's gain at the prices; negative: from A's to B's).
    pub moved: i64,
    /// What each party took out.
    pub to_a: i64,
    pub to_b: i64,
}

/// A re-mark: the position's entries are moved to the prices `p` (the committee's, cited), so that what a party
/// gained since the old entries is margin of its own, and each party takes out `take_a`, `take_b` of what it then
/// has above its floor. At every price each party is worth exactly what it was, less what it took out.
///
/// A side that would stand below its maintenance at `p` is not re-marked (it is liquidated, or it adds). A party's
/// floor is the maintenance at `p` or what its counterparty asked of its margin when the position was made, whichever
/// is more: what is above it is the gain that is usable.
pub fn marked(x: &Position, p: [i64; 2], take_a: i64, take_b: i64) -> Result<Marked> {
    let no = |why: &str| Err(Error::Invalid(format!("the position is not re-marked: {why}")));
    x.check()?;
    let legs = x.x.legs;
    for (i, l) in legs.iter().enumerate() {
        if l.size > 0 && (p[i] <= 0 || p[i] > l.price_max) {
            return no("a price is not above zero and within its market's limit");
        }
    }
    let moved: i64 = legs.iter().enumerate().filter(|(_, l)| l.size > 0).map(|(i, l)| l.gain(p[i])).sum();
    let notional: i64 = legs.iter().enumerate().filter(|(_, l)| l.size > 0).map(|(i, l)| l.worth(p[i])).sum();
    let m = x.x.maint(notional);
    let (a1, b1) = (x.x.a_margin + moved, x.x.b_margin - moved);
    if a1 < m || b1 < m {
        return no(&format!("a side would stand below its maintenance at these prices (a {a1}, b {b1}, the maintenance {m}): it is liquidated or adds margin, not re-marked"));
    }
    // What a party keeps: the maintenance there, and what its counterparty asked of it.
    let (floor_a, floor_b) = (m.max(x.b_min_counter), m.max(x.a_min_counter));
    if take_a < 0 || take_b < 0 || a1 - take_a < floor_a.min(a1) || b1 - take_b < floor_b.min(b1) {
        return no(&format!("a party takes out more than it has above its floor (a has {a1} and keeps {floor_a}; b has {b1} and keeps {floor_b})"));
    }
    let mut c = x.x;
    for (i, l) in c.legs.iter_mut().enumerate() {
        if l.size > 0 {
            l.entry = p[i];
        }
    }
    c.a_margin = a1 - take_a;
    c.b_margin = b1 - take_b;
    let position = Position { x: c, ..*x };
    position.check()?;
    Ok(Marked { position, moved, to_a: take_a, to_b: take_b })
}

/// What `KpfTReplace` leaves: two orders matched into a position, the owner of one leaving it and the owner of the
/// other taking its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Replaced {
    /// What the price moved the leaver's way since the position's entry, for the units matched (negative: against
    /// it). The one that stays gives it up from its margin, and its entry becomes the match's price.
    pub gain: i64,
    /// What is left of the position, and what it holds.
    pub rest: Option<(Position, i64)>,
    /// The newcomer in the leaver's place against the one that stays, from the match's price, and what it holds;
    /// none when the newcomer is the one that stays (the two are paid).
    pub z: Option<(Position, i64)>,
    /// The user that leaves and what it is paid: its share of margin in the position, its order's, and the gain.
    pub leaver: [u8; 32],
    pub to_leaver: i64,
    /// The one that stays, and what it is paid when it is the newcomer too (else 0).
    pub stayer: [u8; 32],
    pub to_stayer: i64,
    /// The two tips' shares: whoever sends the transaction has them.
    pub tips: i64,
    pub long: Rest,
    pub short: Rest,
}

/// `KpfTReplace`. The position `x` (one leg, long for its A) holding `xv`; a long order and a short one of its
/// template, matched for `n` units at `price`; `side` is the party of `x` that leaves, by the order that is its own
/// and the other way (A by the short order, B by the long one). `rules`: the two conditions that protect the one that
/// stays and the newcomer, as in a novation (the script holds them; `false` builds what it must refuse).
#[allow(clippy::too_many_arguments)]
pub fn replaced(x: &Position, xv: i64, long: &Order, long_value: i64, short: &Order, short_value: i64, n: i64, price: i64, side: i64, rules: bool) -> Result<Replaced> {
    let no = |why: &str| Err(Error::Invalid(format!("the orders are not matched into the position: {why}")));
    x.check()?;
    long.check()?;
    short.check()?;
    let (tpl, leg) = (Template::of(x), &x.x.legs[0]);
    if long.tpl != tpl.hash() || short.tpl != tpl.hash() {
        return no("an order is not of the position's template (its market and terms)");
    }
    if long.dir != crate::state::LONG || short.dir != crate::state::SHORT {
        return no("it takes a long order and a short one");
    }
    if x.x.legs[1].size != 0 || leg.dir != crate::state::LONG || leg.size <= 0 || leg.size > ORDER_SIZE_MAX {
        return no("the position is not of one leg, long for its A, of a size in range");
    }
    if n <= 0 || n > long.size || n > short.size || n > leg.size {
        return no("the size matched is above zero and within what both orders and the position have");
    }
    for o in [long, short] {
        if n < o.min_fill && n != o.size {
            return no(&format!("an order takes at least {} units a match, or all {} it has left", o.min_fill, o.size));
        }
    }
    if price <= 0 || price > long.limit || price < short.limit || price > tpl.price_max {
        return no(&format!("the price is not above the long's limit ({}), not below the short's ({}) and within the market's", long.limit, short.limit));
    }
    let (lms, lf, lq, lrest) = long.filled(long_value, n)?;
    let (sms, sf, sq, srest) = short.filled(short_value, n)?;
    if lms <= 0 || sms <= 0 {
        return no("a share of margin would be nothing: the size matched is too small");
    }
    let (ams, bms) = (part(x.x.a_margin, n, leg.size), part(x.x.b_margin, n, leg.size));
    let (aqs, bqs) = (part(x.a_min_counter, n, leg.size), part(x.b_min_counter, n, leg.size));
    let unit = crate::cross::XLeg { size: n, ..*leg };
    let up = unit.worth(price) - unit.worth(leg.entry);
    // (the leaver, the one that stays, the newcomer; the leaver's two shares, the stayer's share and what it asked,
    // the newcomer's share and what it asks; the leaver's gain)
    let (uk, kk, nk, us, ks, kq, ns, nq, d) = match side {
        SIDE_A if short.owner == x.x.a_key => (x.x.a_key, x.x.b_key, long.owner, ams + sms, bms, bqs, lms, lq, up),
        SIDE_B if long.owner == x.x.b_key => (x.x.b_key, x.x.a_key, short.owner, bms + lms, ams, aqs, sms, sq, -up),
        SIDE_A | SIDE_B => return no("the party that leaves has no order the other way among the two (A leaves by the short order, B by the long one)"),
        _ => return no("the side that leaves is a or b"),
    };
    let u = us + d;
    if u < 0 {
        return no("the user that leaves has lost more than its two margins hold");
    }
    let zs = ks - d;
    if zs <= 0 || zs > crate::cross::MARGIN_MAX {
        return no("the margin of the one that stays does not cover what the price moved against it");
    }
    let same = nk == kk;
    if rules && !same {
        if ns < kq + d.max(0) {
            return no(&format!("the one that comes in posts {ns}, less than the one that stays asked of its counterparty ({kq}) and what it gives up ({})", d.max(0)));
        }
        if zs < nq {
            return no(&format!("the one that stays is left {zs}, less than the one that comes in asks of its counterparty ({nq})"));
        }
    }
    if xv < x.pot() {
        return no("the position holds less than its margins");
    }
    let whole = n == leg.size;
    let zv = zs + ns + if whole { xv - x.pot() } else { 0 };
    let z = if same {
        None
    } else {
        let mut c = x.x;
        c.legs[0].size = n;
        c.legs[0].entry = price;
        let (am, bm, aq, bq) = if side == SIDE_A { (ns, zs, nq, kq) } else { (zs, ns, kq, nq) };
        (c.a_key, c.b_key) = if side == SIDE_A { (nk, kk) } else { (kk, nk) };
        (c.a_margin, c.b_margin) = (am, bm);
        let a = Position { x: c, a_min_counter: aq, b_min_counter: bq, ..*x };
        a.check()?;
        Some((a, zv))
    };
    let rest = if whole {
        None
    } else {
        let mut c = x.x;
        c.legs[0].size = leg.size - n;
        c.a_margin -= ams;
        c.b_margin -= bms;
        let r = Position { x: c, a_min_counter: x.a_min_counter - aqs, b_min_counter: x.b_min_counter - bqs, ..*x };
        r.check()?;
        Some((r, xv - ams - bms))
    };
    Ok(Replaced { gain: d, rest, z, leaver: uk, to_leaver: u, stayer: kk, to_stayer: if same { zv } else { 0 }, tips: lf + sf, long: lrest, short: srest })
}

fn part(v: i64, n: i64, size: i64) -> i64 {
    (v / size) * n + ((v % size) * n) / size
}

/// `KpfTNovate`. X and Y are of one template, one leg each, both long for their A, and Y's B is X's A (U). `rules`:
/// the two rules that protect those who stay (`false` only to build what the script must refuse).
pub fn novated(x: &Position, xv: i64, y: &Position, yv: i64, rules: bool) -> Result<Novation> {
    let no = |why: &str| Err(Error::Invalid(format!("the user does not leave the two positions: {why}")));
    x.check()?;
    y.check()?;
    let (lx, ly) = (&x.x.legs[0], &y.x.legs[0]);
    if y.x.b_key != x.x.a_key {
        return no("the second position's B is not the first's A (the user that leaves is long in the first and short in the second)");
    }
    if Template::of(x) != Template::of(y) || x.x.legs[1] != y.x.legs[1] || x.x.legs[1].size != 0 {
        return no("they are not of one market and the same terms, with one leg each");
    }
    if lx.dir != crate::state::LONG || ly.dir != crate::state::LONG {
        return no("their legs are not both long for their A");
    }
    if lx.size <= 0 || ly.size <= 0 || lx.size > ORDER_SIZE_MAX || ly.size > ORDER_SIZE_MAX {
        return no("a size is out of range");
    }
    if xv < x.pot() || yv < y.pot() {
        return no("a position holds less than its margins");
    }
    let n = lx.size.min(ly.size);
    let (axs, bxs) = (part(x.x.a_margin, n, lx.size), part(x.x.b_margin, n, lx.size));
    let (ays, bys) = (part(y.x.a_margin, n, ly.size), part(y.x.b_margin, n, ly.size));
    let (sq, lq) = (part(x.b_min_counter, n, lx.size), part(y.a_min_counter, n, ly.size));
    let unit = crate::cross::XLeg { size: n, ..*lx };
    let tr = unit.worth(ly.entry) - unit.worth(lx.entry);
    let zb = bxs - tr;
    if zb <= 0 {
        return no("the margin of the one that stays short does not cover what the price moved against it");
    }
    let u = axs + bys + tr - NOVATE_TIP;
    if u < 0 {
        return no("the user that leaves has lost more than its two margins hold");
    }
    let same = y.x.a_key == x.x.b_key;
    if rules && !same {
        if ays < sq + tr.max(0) {
            return no(&format!("the one that comes in posts {ays}, less than the one that stays asked of its counterparty ({sq}) and what it gives up ({})", tr.max(0)));
        }
        if zb < lq {
            return no(&format!("the one that stays is left {zb}, less than the one that comes in asked of its counterparty ({lq})"));
        }
    }
    let mut zv = ays + zb;
    if n == lx.size {
        zv += xv - x.pot();
    }
    if n == ly.size {
        zv += yv - y.pot();
    }
    let z = if same {
        None
    } else {
        let mut c = y.x;
        c.legs[0].size = n;
        c.b_key = x.x.b_key;
        c.a_margin = ays;
        c.b_margin = zb;
        let a = Position { x: c, a_min_counter: lq, b_min_counter: sq, ..*y };
        a.check()?;
        Some((a, zv))
    };
    let rest = |a: &Position, v: i64, am: i64, bm: i64, aq: i64, bq: i64| -> Result<Option<(Position, i64)>> {
        let size = a.x.legs[0].size;
        if size == n {
            return Ok(None);
        }
        let mut c = a.x;
        c.legs[0].size = size - n;
        c.a_margin -= am;
        c.b_margin -= bm;
        let r = Position { x: c, a_min_counter: a.a_min_counter - aq, b_min_counter: a.b_min_counter - bq, ..*a };
        r.check()?;
        Ok(Some((r, v - am - bm)))
    };
    Ok(Novation {
        n,
        transfer: tr,
        z,
        rest_x: rest(x, xv, axs, bxs, part(x.a_min_counter, n, lx.size), sq)?,
        rest_y: rest(y, yv, ays, bys, lq, part(y.b_min_counter, n, ly.size))?,
        to_leaver: u,
        to_stayer: if same { zv } else { 0 },
    })
}

/// Sample instances (tests, documents).
pub mod samples {
    use super::Position;

    pub fn position() -> Position {
        Position { x: crate::cross::samples::cross(), dispute_daa: 600, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 }
    }
}
