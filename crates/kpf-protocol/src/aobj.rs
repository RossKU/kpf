//! The objects of an account and of the reserve as whoever keeps them writes them down (the chain holds a hash): an
//! account, a shard, a note to a shard, and a statement on an account or a shard, disputed at a point or not. A
//! statement is kept with what it was stated on and the bytes it stated: an answer shows both.
//!
//! The bytes: a kind (20 an account, 21 a shard, 22 a note that legs left, 23 a statement, 24 a statement disputed
//! at a point, 25 a note of the rest), then
//! - an account: its terms (90), the count of its legs (one byte), the legs (111 each), its moving part (138 or 171)
//! - a shard: its terms (90), its four rows (408), its moving part (171)
//! - a note that legs left: the shard's id, the count of its legs (one byte), the legs (111 each), the user's key,
//!   what is asked (pushed); a note of the rest: the shard's id, the key, the amount (pushed)
//! - a statement: the length of what it is stated on (two bytes) and that, as above; window, lateness, kind, time,
//!   bond, held (pushed); the claimer's key; the count of its outputs and their 41 bytes each; the length of what
//!   it states (two bytes) and that
//! - a statement disputed: the same, then the disputer's key and the point (pushed)

use crate::committee::blake2b;
use crate::cross::{XLeg, XTerms};
use crate::error::{Error, Result};
use crate::covered::CSaid;
use crate::reserve::{Draw, Left, Rest, Shard, ShardRow, SHARD_CARRIER, SHARD_ROWS};
use crate::state::int8;
use crate::thin::{leg_bytes, ATake, Account, Outs, Pointed, Stated, KIND_ACCOUNT_LEAVE, KIND_ACCOUNT_TAKE, KIND_DRAW};

/// What a statement is stated on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    Account(Account),
    Shard(Shard),
}

impl Host {
    pub fn hash(&self) -> [u8; 32] {
        match self {
            Host::Account(a) => a.hash(),
            Host::Shard(s) => s.hash(),
        }
    }

    fn obj(&self) -> AObj {
        match self {
            Host::Account(a) => AObj::Account(a.clone()),
            Host::Shard(s) => AObj::Shard(*s),
        }
    }
}

/// An object of an account or of the reserve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AObj {
    Account(Account),
    Shard(Shard),
    /// The note that legs left a covered account.
    Left(Left),
    /// What such a note asked and its shard did not hold.
    Rest(Rest),
    /// The note that a leg of a covered account was marked.
    Mark(crate::reserve::MarkNote),
    /// The ticket of a token leg that left a covered account.
    Ticket(crate::covered::Ticket),
    /// The note of a payment of what a covered account owed its shard.
    Owed(crate::reserve::Owed),
    Stated { on: Host, st: Stated, said: Vec<u8> },
    Pointed { on: Host, q: Pointed, said: Vec<u8> },
}

/// What a statement's outputs make where it is the rule's: the object it was stated on as it goes on, and the notes
/// it writes. Each is one of the statement's outputs (by its hash); of a statement that is not the rule's, the
/// outputs that are nobody's model are not here.
pub fn outcome(on: &Host, st: &Stated, said: &[u8]) -> Vec<AObj> {
    let mut v = vec![];
    match on {
        Host::Shard(s) => {
            if let Some(d) = Draw::read(said) {
                v.push(AObj::Shard(s.drawn(d.amount, st.t0)));
            }
        }
        Host::Account(x) if x.shard.is_some() => {
            // (A covered account's own kinds: what the end of the statement makes of it, and the note it writes.)
            let Some(s) = CSaid::read(said) else { return v };
            if let Some(Some(o)) = x.c_end(st.kind, &s, &st.claimer, st.held, st.t0) {
                v.extend(o.after.map(AObj::Account));
                v.extend(o.note.map(AObj::Left));
                v.extend(o.split.map(AObj::Account));
                v.extend(o.mark.map(AObj::Mark));
                v.extend(o.ticket.map(AObj::Ticket));
                v.extend(o.owed.map(AObj::Owed));
            }
        }
        Host::Account(x) => {
            let Some(tk) = ATake::read(said) else { return v };
            if st.kind == KIND_ACCOUNT_TAKE {
                if let Some(next) = x.taken(tk.side, tk.amount, &tk.rows) {
                    v.push(AObj::Account(next));
                }
            } else if let Some(stepped) = x.stepped(st.kind, tk.side, tk.skip, &tk.rows, &st.claimer) {
                v.push(AObj::Account(stepped.after));
            }
        }
    }
    v.retain(|o| st.outs.list().iter().any(|e| !e.key && e.to == o.hash()));
    v
}

fn i9(v: i64) -> Vec<u8> {
    [&[8u8][..], &crate::state::sm8(v)].concat()
}

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len()).ok_or_else(|| bad("it ends early"))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn key(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().expect("32 bytes"))
    }
    /// A key or a hash as a script pushes it: 0x20 and 32 bytes.
    fn p33(&mut self) -> Result<[u8; 32]> {
        if self.take(1)? != [0x20] {
            return Err(bad("a push of 32 bytes"));
        }
        self.key()
    }
    /// An integer as a script pushes it: 0x08 and 8 bytes.
    fn int(&mut self) -> Result<i64> {
        if self.take(1)? != [0x08] {
            return Err(bad("a push of 8 bytes"));
        }
        Ok(int8(self.take(8)?))
    }
    fn len2(&mut self) -> Result<usize> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]) as usize)
    }
    fn end(&self) -> Result<()> {
        if self.at == self.b.len() { Ok(()) } else { Err(bad("bytes after its end")) }
    }
}

fn bad(why: &str) -> Error {
    Error::Decode(format!("not the state of an account, a shard, a note or a statement: {why}"))
}

fn terms_of(r: &mut Rd) -> Result<XTerms> {
    Ok(XTerms { term: r.int()?, mmr_bps: r.int()?, g_bps: r.int()?, liq_fee_bps: r.int()?, keeper_fee_min: r.int()?, carrier: r.int()?, idle_daa: r.int()?, grace_daa: r.int()?, late_daa: r.int()?, quiet_daa: r.int()? })
}

/// A shard's row: its key (the market, a day, a kind, a strike), its scale and limit, its net, its gross, and
/// what its legs were entered at.
fn row_of(r: &mut Rd) -> Result<ShardRow> {
    let key = crate::books::Key { market: r.p33()?, day: r.int()?, of: r.int()?, strike: r.int()? };
    Ok(ShardRow { key, scale: r.int()?, price_max: r.int()?, net: r.int()?, gross: r.int()?, entered: r.int()? })
}

/// The moving part's fields: the two keys, the two margins, the lease, the lateness, the dispute period, the bond
/// rate and the two asks; and the shard's id where the part is 171 bytes.
#[allow(clippy::type_complexity)]
fn moving_of(r: &mut Rd, len: usize) -> Result<([u8; 32], [u8; 32], [i64; 8], Option<[u8; 32]>)> {
    let (a, b) = (r.p33()?, r.p33()?);
    let mut v = [0i64; 8];
    for x in v.iter_mut() {
        *x = r.int()?;
    }
    let sid = match len {
        138 => None,
        171 => Some(r.p33()?),
        _ => return Err(bad("a moving part of 138 or 171 bytes")),
    };
    Ok((a, b, v, sid))
}

fn leg_of(r: &mut Rd) -> Result<XLeg> {
    Ok(XLeg { market: r.p33()?, committee: r.p33()?, scale: r.int()?, price_max: r.int()?, size: r.int()?, dir: r.int()?, entry: r.int()? })
}

fn account_of(r: &mut Rd) -> Result<Account> {
    let t = terms_of(r)?;
    let n = r.take(1)?[0] as usize;
    let mut legs = Vec::with_capacity(n);
    for _ in 0..n {
        legs.push(leg_of(r)?);
    }
    let len = r.b.len() - r.at;
    let (a_key, b_key, v, shard) = moving_of(r, len)?;
    if v[3] != t.late_daa {
        return Err(bad("the moving part's lateness is not the terms'"));
    }
    Ok(Account { t, legs, a_key, b_key, a_margin: v[0], b_margin: v[1], expiry: v[2], dispute_daa: v[4], bond_bps: v[5], a_min_counter: v[6], b_min_counter: v[7], shard })
}

fn shard_of(r: &mut Rd) -> Result<Shard> {
    let t = terms_of(r)?;
    let mut rows = [ShardRow::EMPTY; SHARD_ROWS];
    for row in rows.iter_mut() {
        *row = row_of(r)?;
    }
    let (owner, none, v, sid) = moving_of(r, 171)?;
    if none != [0; 32] || v[3] != t.late_daa {
        return Err(bad("a shard has no second key"));
    }
    // (The claims' three numbers, where they are kept: a shard kept before they were is one of none.)
    let claims = if r.at < r.b.len() { crate::reserve::Claims { paid1: r.int()?, owed2: r.int()?, paid2: r.int()? } } else { crate::reserve::Claims::default() };
    Ok(Shard { t, rows, owner, held: v[0], expiry: v[2], dispute_daa: v[4], bond_bps: v[5], sid: sid.expect("171 bytes"), basis: v[6], called: v[7], owed: v[1], claims })
}

fn host_bytes(h: &Host) -> Vec<u8> {
    let b = h.obj().encode();
    [&(b.len() as u16).to_le_bytes()[..], &b].concat()
}

fn stated_bytes(on: &Host, st: &Stated, said: &[u8]) -> Vec<u8> {
    [host_bytes(on), i9(st.window), i9(st.late), i9(st.kind), i9(st.t0), i9(st.bond), i9(st.held), st.claimer.to_vec(), vec![st.outs.list().len() as u8], st.outs.bytes(), (said.len() as u16).to_le_bytes().to_vec(), said.to_vec()].concat()
}

fn stated_of(r: &mut Rd) -> Result<(Host, Stated, Vec<u8>)> {
    let n = r.len2()?;
    let on = match AObj::decode(r.take(n)?)? {
        AObj::Account(a) => Host::Account(a),
        AObj::Shard(s) => Host::Shard(s),
        _ => return Err(bad("a statement is on an account or a shard")),
    };
    let (window, late, kind, t0, bond, held) = (r.int()?, r.int()?, r.int()?, r.int()?, r.int()?, r.int()?);
    let claimer = r.key()?;
    let n = r.take(1)?[0] as usize;
    let outs = Outs::read(r.take(41 * n)?).ok_or_else(|| bad("the outputs of a statement"))?;
    let n = r.len2()?;
    let said = r.take(n)?.to_vec();
    // What is stated is of the statement's kind and of what it is stated on.
    let of_kind = match (&on, kind) {
        (Host::Shard(_), KIND_DRAW) => Draw::read(&said).is_some(),
        (Host::Account(x), KIND_ACCOUNT_TAKE..=KIND_ACCOUNT_LEAVE) => x.shard.is_none() && ATake::read(&said).is_some(),
        (Host::Account(x), k) if crate::covered::c_kind(k) => x.shard.is_some() && CSaid::read(&said).is_some(),
        _ => false,
    };
    if !of_kind {
        return Err(bad("what is stated is not of the statement's kind, or the kind is not one of what it is stated on"));
    }
    Ok((on.clone(), Stated { position: on.hash(), window, late, kind, t0, arg: blake2b(&said), outs, bond, claimer, held }, said))
}

impl AObj {
    /// The hash the object's script carries.
    pub fn hash(&self) -> [u8; 32] {
        match self {
            AObj::Account(a) => a.hash(),
            AObj::Shard(s) => s.hash(),
            AObj::Left(n) => n.hash(),
            AObj::Rest(n) => n.hash(),
            AObj::Mark(n) => n.hash(),
            AObj::Ticket(n) => n.hash(),
            AObj::Owed(n) => n.hash(),
            AObj::Stated { st, .. } => st.hash(),
            AObj::Pointed { q, .. } => q.hash(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            AObj::Account(a) => if a.shard.is_some() { "covered account" } else { "account" },
            AObj::Shard(_) => "shard",
            AObj::Left(_) => "note that legs left",
            AObj::Rest(_) => "note of the rest",
            AObj::Mark(_) => "note that a leg was marked",
            AObj::Ticket(_) => "ticket of a token leg",
            AObj::Owed(_) => "note of a payment of what an account owed",
            AObj::Stated { on: Host::Shard(_), .. } => "stated draw",
            AObj::Stated { .. } => "statement",
            AObj::Pointed { on: Host::Shard(_), .. } => "disputed draw",
            AObj::Pointed { .. } => "disputed statement",
        }
    }

    /// The least the object's output holds for its state to be paid as it says.
    pub fn least_value(&self) -> i64 {
        match self {
            AObj::Account(a) => a.pot(),
            AObj::Shard(s) => s.held + SHARD_CARRIER,
            AObj::Left(n) => n.value(),
            AObj::Rest(n) => n.value(),
            AObj::Mark(n) => n.value(),
            AObj::Ticket(n) => n.value(),
            AObj::Owed(n) => n.value(),
            AObj::Stated { st, .. } => st.held + st.bond,
            AObj::Pointed { q, .. } => q.st.held + 2 * q.st.bond,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            AObj::Account(a) => [vec![20], a.terms(), vec![a.legs.len() as u8], a.legs.iter().flat_map(leg_bytes).collect(), a.moving()].concat(),
            // (The claims' numbers where there are any: a shard that has none is kept as before they were.)
            AObj::Shard(s) => [vec![21], s.terms(), s.rows_bytes(), s.moving(), if s.claims == crate::reserve::Claims::default() { vec![] } else { s.claims.bytes() }].concat(),
            AObj::Left(n) => [vec![22], n.sid.to_vec(), vec![n.legs.len() as u8], n.legs.iter().flat_map(leg_bytes).collect(), n.key.to_vec(), i9(n.asked)].concat(),
            AObj::Rest(n) => [vec![25], n.sid.to_vec(), n.key.to_vec(), i9(n.amount)].concat(),
            AObj::Mark(n) => [vec![29], n.sid.to_vec(), leg_bytes(&n.leg), i9(n.entry), n.key.to_vec(), i9(n.asked)].concat(),
            AObj::Ticket(n) => [vec![30], n.chain.to_vec(), n.key.to_vec()].concat(),
            AObj::Owed(n) => [vec![31], n.sid.to_vec(), i9(n.amount)].concat(),
            AObj::Stated { on, st, said } => [vec![23], stated_bytes(on, st, said)].concat(),
            AObj::Pointed { on, q, said } => [vec![24], stated_bytes(on, &q.st, said), q.disputer.to_vec(), i9(q.point)].concat(),
        }
    }

    /// Reads `encode`'s bytes back, refusing anything that is not exactly an object's state.
    pub fn decode(b: &[u8]) -> Result<AObj> {
        let (kind, rest) = b.split_first().ok_or_else(|| bad("empty"))?;
        let mut r = Rd { b: rest, at: 0 };
        let o = match kind {
            20 => {
                let a = account_of(&mut r)?;
                if a.legs.len() > 255 {
                    return Err(bad("more legs than are counted"));
                }
                AObj::Account(a)
            }
            21 => AObj::Shard(shard_of(&mut r)?),
            22 => {
                let sid = r.key()?;
                let n = r.take(1)?[0] as usize;
                let mut legs = Vec::with_capacity(n);
                for _ in 0..n {
                    legs.push(leg_of(&mut r)?);
                }
                AObj::Left(Left { sid, legs, key: r.key()?, asked: r.int()? })
            }
            25 => AObj::Rest(Rest { sid: r.key()?, key: r.key()?, amount: r.int()? }),
            31 => AObj::Owed(crate::reserve::Owed { sid: r.key()?, amount: r.int()? }),
            30 => AObj::Ticket(crate::covered::Ticket { chain: r.key()?, key: r.key()? }),
            29 => AObj::Mark(crate::reserve::MarkNote { sid: r.key()?, leg: leg_of(&mut r)?, entry: r.int()?, key: r.key()?, asked: r.int()? }),
            23 => {
                let (on, st, said) = stated_of(&mut r)?;
                AObj::Stated { on, st, said }
            }
            24 => {
                let (on, st, said) = stated_of(&mut r)?;
                AObj::Pointed { on, q: Pointed { st, disputer: r.key()?, point: r.int()? }, said }
            }
            _ => return Err(bad("its kind")),
        };
        r.end()?;
        if o.encode() != b {
            return Err(bad("it is not written as the object writes itself"));
        }
        Ok(o)
    }
}
