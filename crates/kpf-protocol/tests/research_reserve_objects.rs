//! Step 4 of the architecture's order (`docs/ARCHITECTURE-CLEARING.md` 6 and 9): the reserve with objects, before any
//! script. R4 (`research_reserve.rs`) held in plain numbers; here the reserve is a shard with a state of its own,
//! accounts name the shard that covers them, and every operation is one transaction on the objects it spends, which
//! is refused or changes them. Nothing touches two objects at once but by a note: a closing account leaves the shard
//! notes (what it releases of the net, what the clearing side had left, what a user is still owed), and anyone
//! applies them to the shard one after another. That is what the chain allows (a shard is one output: every account
//! that had to spend it would wait for the others), and it is where the attacks are.
//!
//! One market, prices and amounts in sompi, sizes in units of 1 KAS at the price 1.

const KAS: i64 = 100_000_000;
const BPS: i64 = 10_000;

/// The rates of the form: a user's maintenance, the reserve's on the net and on the gross, and what the clearing
/// party posts in an account.
#[derive(Clone, Copy)]
struct Rates {
    user: i64,
    net: i64,
    gross: i64,
    posted: i64,
}

const RATES: Rates = Rates { user: 500, net: 500, gross: 20, posted: 50 };

fn rate(v: i64, bps: i64) -> i64 {
    (v as i128 * bps as i128 / BPS as i128) as i64
}

/// An account of a user against the clearing party: the user's units (above zero: the user is long), the entry, the
/// two margins, the shard that covers it.
#[derive(Clone, Debug)]
struct Acct {
    user: usize,
    size: i64,
    entry: i64,
    um: i64,
    cm: i64,
    shard: Option<usize>,
    open: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Note {
    /// A covered account ended: its units leave the shard's net and gross.
    Release { net: i64, gross: i64 },
    /// What the clearing side had left in an account that was closed against it, or swept.
    Deposit(i64),
    /// What an account's pot could not pay its user.
    Owed { user: usize, amount: i64 },
}

/// A reserve shard: what it holds, the clearing party's net and gross units over the accounts it covers, whether it
/// was called, and the notes not yet applied to it.
#[derive(Clone, Debug, Default)]
struct Shard {
    held: i64,
    net: i64,
    gross: i64,
    called: bool,
    notes: Vec<Note>,
}

impl Shard {
    fn requirement(&self, p: i64, r: Rates) -> i64 {
        rate(self.net.abs() * p, r.net) + rate(self.gross * p, r.gross)
    }
}

#[derive(Clone, Default)]
struct World {
    accts: Vec<Acct>,
    shards: Vec<Shard>,
    /// What each user was paid, and what the clearing party took out.
    paid: Vec<i64>,
    taken: i64,
    /// What entered from outside: margins posted and reserves funded.
    entered: i64,
}

type R = Result<(), &'static str>;

impl World {
    fn total(&self) -> i64 {
        self.accts.iter().filter(|a| a.open).map(|a| a.um + a.cm).sum::<i64>() + self.shards.iter().map(|s| s.held + s.notes.iter().map(|n| if let Note::Deposit(v) = n { *v } else { 0 }).sum::<i64>()).sum::<i64>() + self.paid.iter().sum::<i64>() + self.taken
    }

    /// An account is opened by both: the user's margin, and the clearing party's at its posted rate.
    fn open(&mut self, user: usize, size: i64, p: i64, um: i64, r: Rates) -> usize {
        let cm = rate(size.abs() * p, r.posted);
        self.entered += um + cm;
        if self.paid.len() <= user {
            self.paid.resize(user + 1, 0);
        }
        self.accts.push(Acct { user, size, entry: p, um, cm, shard: None, open: true });
        self.accts.len() - 1
    }

    fn fund(&mut self, s: usize, v: i64) {
        self.entered += v;
        self.shards[s].held += v;
    }

    /// The entries move to `p`: the gain since is margin. Refused where a side would stand below zero (the account
    /// is closed instead).
    fn mark(&mut self, a: usize, p: i64) -> R {
        let x = &mut self.accts[a];
        if !x.open {
            return Err("closed");
        }
        let g = (x.size as i128 * (p - x.entry) as i128) as i64;
        if x.um + g < 0 || x.cm - g < 0 {
            return Err("a side would stand below zero");
        }
        x.um += g;
        x.cm -= g;
        x.entry = p;
        Ok(())
    }

    /// An account is put under a shard at a price: it is marked there, the user stands at its maintenance, and the
    /// shard, with the account's units in its net and gross, holds its requirement.
    fn cover(&mut self, s: usize, a: usize, p: i64, r: Rates) -> R {
        if self.shards[s].called {
            return Err("the shard was called");
        }
        if self.accts[a].shard.is_some() {
            return Err("covered already");
        }
        self.mark(a, p)?;
        let x = &self.accts[a];
        if x.um < rate(x.size.abs() * p, r.user) {
            return Err("the user is below its maintenance");
        }
        let mut next = self.shards[s].clone();
        next.net -= x.size;
        next.gross += x.size.abs();
        if next.held < next.requirement(p, r) {
            return Err("the shard would hold less than its requirement");
        }
        self.shards[s] = next;
        self.accts[a].shard = Some(s);
        Ok(())
    }

    /// An account ends at `p`: the user is paid what it is owed of the pot; what the pot lacks is a note on the
    /// shard, and what the clearing side has left is a deposit there. In a covered account nothing is ever paid to
    /// the clearing party itself: the shard is where it is settled, and it draws from the shard what is above the
    /// requirement. (Paid to it directly, its gains in some accounts would leave while its losses in others are
    /// noted on a shard that never saw them.)
    fn close(&mut self, a: usize, p: i64) -> R {
        let x = self.accts[a].clone();
        if !x.open {
            return Err("closed");
        }
        let g = (x.size as i128 * (p - x.entry) as i128) as i64;
        let pot = x.um + x.cm;
        let owed = x.um + g;
        let to_user = owed.clamp(0, pot);
        self.paid[x.user] += to_user;
        let left = pot - to_user;
        self.accts[a].open = false;
        self.accts[a].um = 0;
        self.accts[a].cm = 0;
        match x.shard {
            Some(s) => {
                let sh = &mut self.shards[s];
                sh.notes.push(Note::Release { net: -x.size, gross: x.size.abs() });
                if owed > pot {
                    sh.notes.push(Note::Owed { user: x.user, amount: owed - pot });
                }
                sh.notes.push(Note::Deposit(left));
            }
            None => self.taken += left,
        }
        Ok(())
    }

    /// A user below its maintenance at `p` is closed by anyone.
    fn liquidate(&mut self, a: usize, p: i64, r: Rates) -> R {
        let x = &self.accts[a];
        let g = (x.size as i128 * (p - x.entry) as i128) as i64;
        if !x.open || x.um + g >= rate(x.size.abs() * p, r.user) {
            return Err("the user is not below its maintenance");
        }
        self.close(a, p)
    }

    /// Anyone: at a mark, what the clearing side has above what it posts goes to the shard.
    fn sweep(&mut self, a: usize, p: i64, r: Rates) -> R {
        self.mark(a, p)?;
        let x = &mut self.accts[a];
        let Some(s) = x.shard else { return Err("not covered") };
        let over = x.cm - rate(x.size.abs() * p, r.posted);
        if over <= 0 {
            return Err("nothing above what it posts");
        }
        x.cm -= over;
        self.shards[s].notes.push(Note::Deposit(over));
        Ok(())
    }

    /// Anyone applies the shard's oldest note.
    fn apply(&mut self, s: usize) -> R {
        let sh = &mut self.shards[s];
        if sh.notes.is_empty() {
            return Err("no note");
        }
        match sh.notes.remove(0) {
            Note::Release { net, gross } => {
                sh.net -= net;
                sh.gross -= gross;
            }
            Note::Deposit(v) => sh.held += v,
            Note::Owed { user, amount } => {
                let pay = amount.min(sh.held);
                sh.held -= pay;
                self.paid[user] += pay;
            }
        }
        Ok(())
    }

    fn apply_all(&mut self, s: usize) {
        while self.apply(s).is_ok() {}
    }

    /// Anyone: a shard that holds less than its requirement at `p` is called. It covers nothing more, and its
    /// accounts are closed against it.
    fn call(&mut self, s: usize, p: i64, r: Rates) -> R {
        let sh = &mut self.shards[s];
        if sh.held >= sh.requirement(p, r) {
            return Err("the shard holds its requirement");
        }
        sh.called = true;
        Ok(())
    }

    /// The clearing party draws down to its requirement at `p`. `stated`: the draw is a statement with a dispute
    /// period, refuted by any note of the shard that was not applied when it was stated.
    fn draw(&mut self, s: usize, v: i64, p: i64, r: Rates, stated: bool) -> R {
        let sh = &mut self.shards[s];
        if sh.called {
            return Err("the shard was called");
        }
        if stated && !sh.notes.is_empty() {
            return Err("a note of the shard is not applied: the draw is refuted");
        }
        if sh.held - v < sh.requirement(p, r) {
            return Err("the shard would hold less than its requirement");
        }
        sh.held -= v;
        self.taken += v;
        Ok(())
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

/// A book of `n` users under one shard at the price 1: `long` of them long, each of 10 to 30 units with a margin
/// of a tenth, and the shard funded with its requirement and `spare` over.
fn book(r: &mut Rng, n: usize, long: usize, spare: i64) -> World {
    let mut w = World::default();
    w.shards.push(Shard::default());
    let mut ids = vec![];
    for u in 0..n {
        let size = r.range(10, 30) * if u < long { 1 } else { -1 };
        ids.push(w.open(u, size, KAS, rate(size.abs() * KAS, 1_000), RATES));
    }
    // A shard holds its requirement after every cover, so a book is covered along a path: here the longs and the
    // shorts in turn, the shard funded for the highest requirement on that path, and what is above the book's own
    // requirement drawn again when all are covered.
    let (longs, shorts): (Vec<usize>, Vec<usize>) = ids.iter().partition(|a| w.accts[**a].size > 0);
    let mut order: Vec<usize> = vec![];
    for k in 0..longs.len().max(shorts.len()) {
        order.extend(longs.get(k).copied());
        order.extend(shorts.get(k).copied());
    }
    let (mut path, mut peak) = (Shard::default(), 0);
    for a in &order {
        path.net -= w.accts[*a].size;
        path.gross += w.accts[*a].size.abs();
        peak = peak.max(path.requirement(KAS, RATES));
    }
    w.fund(0, peak + spare);
    for a in order {
        w.cover(0, a, KAS, RATES).expect("covered");
    }
    let over = w.shards[0].held - w.shards[0].requirement(KAS, RATES) - spare;
    w.draw(0, over, KAS, RATES, true).expect("drawn to the requirement");
    w.entered -= over;
    w.taken -= over;
    w
}

/// The clearing party is gone at the price `p`: the shard is called if it can be, and every user's watchtower acts
/// for itself, in the order given. `sweep_first`: the accounts in which the clearing party gained are closed first.
fn fails(w: &mut World, p: i64, sweep_first: bool) {
    let _ = w.call(0, p, RATES);
    let mut order: Vec<usize> = (0..w.accts.len()).filter(|a| w.accts[*a].open).collect();
    if sweep_first {
        // The clearing side gains where the user loses.
        order.sort_by_key(|a| w.accts[*a].size * (p - w.accts[*a].entry));
    }
    for a in order {
        if w.liquidate(a, p, RATES).is_err() {
            w.close(a, p).expect("closed");
        }
        if !sweep_first {
            w.apply_all(0);
        }
    }
    w.apply_all(0);
}

/// What the users are owed in all at `p`, were every account paid in full.
fn owed(w: &World, p: i64) -> i64 {
    w.accts.iter().filter(|a| a.open).map(|a| (a.um + (a.size as i128 * (p - a.entry) as i128) as i64).max(0)).sum()
}

#[test]
fn nothing_is_made_or_lost_by_any_order_of_transactions() {
    let mut r = Rng(0x1234_5678_9abc_def1);
    for round in 0..200 {
        let (n, long) = (r.range(4, 30) as usize, 0);
        let long = r.range(long, n as i64) as usize;
        let spare = r.range(0, 50) * KAS;
        let mut w = book(&mut r, n, long, spare);
        let mut p = KAS;
        for _ in 0..60 {
            p = (p + p * r.range(-150, 150) / BPS).max(KAS / 4);
            let a = r.range(0, n as i64 - 1) as usize;
            let _ = match r.next() % 9 {
                0 => w.mark(a, p),
                1 => w.sweep(a, p, RATES),
                2 => w.liquidate(a, p, RATES),
                3 => w.close(a, p),
                4 => w.apply(0),
                5 => w.call(0, p, RATES),
                6 => w.draw(0, r.range(1, 20) * KAS, p, RATES, true),
                7 => { let v = r.range(1, 10) * KAS; w.fund(0, v); Ok(()) }
                _ => { let id = w.open(a, r.range(-20, 20).max(1), p, 3 * KAS, RATES); w.cover(0, id, p, RATES) }
            };
            assert_eq!(w.total(), w.entered, "round {round}: what is held, paid and taken is what entered");
            assert!(w.shards[0].held >= 0 && w.accts.iter().all(|a| a.um >= 0 && a.cm >= 0));
        }
    }
}

#[test]
fn every_user_is_paid_in_full_for_a_move_within_the_rate_when_gains_are_closed_first() {
    let mut r = Rng(0xfeed_face_cafe_beef);
    let (mut full, mut short_unordered) = (0, 0);
    for _ in 0..200 {
        let n = r.range(10, 60) as usize;
        let long = r.range(0, n as i64) as usize;
        let w0 = book(&mut r, n, long, 0);
        // A move of up to the reserve's rate on the net, either way, with the clearing party gone.
        let p = KAS + KAS * r.range(-RATES.net, RATES.net) / BPS;
        let due = owed(&w0, p);
        let mut w = w0.clone();
        fails(&mut w, p, true);
        assert_eq!(w.paid.iter().sum::<i64>(), due, "gains closed first: every user has what it is owed");
        full += 1;
        // In any other order somebody may be left short: the notes are paid as they come.
        let mut w = w0.clone();
        fails(&mut w, p, false);
        if w.paid.iter().sum::<i64>() < due {
            short_unordered += 1;
        }
        assert_eq!(w.total(), w.entered);
    }
    println!("{full} books paid in full with gains first; {short_unordered} of them left somebody short in the order of the accounts");
    assert!(short_unordered > 0, "the order matters");
}

#[test]
fn a_move_beyond_the_rate_is_shared_by_the_order_of_the_notes_and_never_paid_twice() {
    let mut r = Rng(7);
    let mut w = book(&mut r, 40, 40, 0);
    let p = KAS + KAS * 12 / 100;
    let due = owed(&w, p);
    let before = w.total();
    fails(&mut w, p, true);
    let paid: i64 = w.paid.iter().sum();
    assert!(paid < due, "a book all one way with a move of 12 %: the reserve of 5 % does not pay it");
    assert_eq!(w.shards[0].held, 0, "everything the shard held was paid");
    assert_eq!(w.total(), before);
    println!("a move of 12 % on a book all long: paid {} of {} KAS owed", paid / KAS, due / KAS);
}

#[test]
fn accounts_of_its_own_are_collateral_and_not_a_dress_when_a_user_keeps_the_reserves_rate() {
    // The clearing party is short 1,000 units against its users. It opens accounts with itself the other way so
    // that the shard's net is zero, and funds the shard for the gross alone.
    // With the user's maintenance below the reserve's rate the dress is cheaper than the reserve and leaves users
    // short; at the reserve's rate it costs what the reserve would and pays them in full.
    let (cheap, short) = run_with(100);
    let (fair, none) = run_with(500);
    println!("a dress at 1 % of margin: capital {} KAS, users short {} KAS; at 5 %: capital {} KAS, users short {} KAS", cheap / KAS, short / KAS, fair / KAS, none / KAS);
    assert!(short > 0 && none == 0);
    assert!(fair >= rate(1_000 * KAS, RATES.net), "a dress that pays costs at least the reserve on the net it hides");
    // And where a user keeps the reserve's rate, a side of its own with 1 % of margin is not covered at all: the
    // cover asks what a mark asks.
    let mut w = World::default();
    w.shards.push(Shard::default());
    w.fund(0, 1_000 * KAS);
    let own = w.open(0, -100, KAS, rate(100 * KAS, 100), RATES);
    assert!(w.cover(0, own, KAS, RATES).is_err());
}

/// As the closure of the test above, with the user's maintenance set to the dress's margin (the rule that fails) or
/// kept at the reserve's rate.
fn run_with(dress_margin_bps: i64) -> (i64, i64) {
    let rates = Rates { user: dress_margin_bps, ..RATES };
    let mut w = World::default();
    w.shards.push(Shard::default());
    let users: Vec<usize> = (0..10).map(|u| w.open(u, 100, KAS, 10 * KAS, RATES)).collect();
    let own: Vec<usize> = (0..10).map(|u| w.open(10 + u, -100, KAS, rate(100 * KAS, dress_margin_bps), RATES)).collect();
    // (Covered in turn, an account of its own and a user's, the shard holding the net of one account on the way.)
    w.fund(0, rate(2_000 * KAS, RATES.gross) + rate(100 * KAS, RATES.net));
    for k in 0..10 {
        w.cover(0, own[k], KAS, rates).expect("covered");
        w.cover(0, users[k], KAS, rates).expect("covered");
    }
    let capital = w.entered - 100 * KAS;
    let p = KAS * 105 / 100;
    let due: i64 = users.iter().map(|a| w.accts[*a].um + 100 * (p - KAS)).sum();
    let _ = w.call(0, p, rates);
    // Gains first: the accounts of its own, in which the clearing side gained.
    for a in own.iter().chain(&users) {
        if w.liquidate(*a, p, rates).is_err() {
            w.close(*a, p).expect("closed");
        }
    }
    w.apply_all(0);
    let paid: i64 = (0..10).map(|u| w.paid[u]).sum();
    (capital, due - paid)
}

#[test]
fn a_draw_is_a_statement_or_a_stale_net_empties_the_shard() {
    // A balanced book: 10 users long 100 and 10 short 100. The shard holds the gross rate and the spare the clearing
    // party funded on the way (the net of an account or two while the book was being covered).
    let build = || {
        let mut w = World::default();
        w.shards.push(Shard::default());
        let long: Vec<usize> = (0..10).map(|u| w.open(u, 100, KAS, 10 * KAS, RATES)).collect();
        let short: Vec<usize> = (0..10).map(|u| w.open(10 + u, -100, KAS, 10 * KAS, RATES)).collect();
        w.fund(0, rate(2_000 * KAS, RATES.gross) + 30 * KAS);
        for k in 0..10 {
            w.cover(0, long[k], KAS, RATES).unwrap();
            w.cover(0, short[k], KAS, RATES).unwrap();
        }
        (w, long, short)
    };
    // The shorts close with the clearing party's consent. The book is now 1,000 units one way, and the shard's net
    // says nothing of it until the notes of those closings are applied.
    let attack = |stated: bool| -> (i64, i64) {
        let (mut w, long, short) = build();
        for a in &short {
            w.close(*a, KAS).unwrap();
        }
        assert_eq!(w.shards[0].net, 0, "the shard's net is stale");
        // It draws at once, against the requirement of the stale net...
        let drew = if w.draw(0, 30 * KAS, KAS, RATES, stated).is_ok() { 30 * KAS } else { 0 };
        // ...then whoever watches applies the notes, and the price rises 5 % with the clearing party gone.
        w.apply_all(0);
        let p = KAS * 105 / 100;
        let due: i64 = long.iter().map(|a| w.accts[*a].um + 100 * (p - KAS)).sum();
        fails(&mut w, p, true);
        let paid: i64 = (0..10).map(|u| w.paid[u]).sum();
        (drew, due - paid)
    };
    let (drew, short) = attack(false);
    let (kept, short_stated) = attack(true);
    println!("a draw at once on a stale net: drew {} KAS, users short {} KAS; a draw that is stated: drew {} KAS, users short {} KAS", drew / KAS, short / KAS, kept / KAS, short_stated / KAS);
    assert!(drew > 0 && short > short_stated, "the draw at once took what the users are then short of");
    assert_eq!(kept, 0, "a stated draw is refuted by a note that was not applied");
    // And with the notes applied the shard is under its requirement for the net it now has: anyone calls it, and
    // it covers nothing more.
    let (mut w, _, short_accts) = build();
    for a in &short_accts {
        w.close(*a, KAS).unwrap();
    }
    w.apply_all(0);
    assert!(w.shards[0].held < w.shards[0].requirement(KAS, RATES));
    w.call(0, KAS, RATES).expect("called");
    let id = w.open(30, 10, KAS, KAS, RATES);
    assert!(w.cover(0, id, KAS, RATES).is_err(), "a called shard covers nothing more");
}

#[test]
fn an_account_is_covered_once_and_what_a_user_is_owed_is_noted_once() {
    let mut w = World::default();
    w.shards.push(Shard::default());
    w.shards.push(Shard::default());
    let a = w.open(0, 100, KAS, 10 * KAS, RATES);
    w.fund(0, 100 * KAS);
    w.fund(1, 100 * KAS);
    w.cover(0, a, KAS, RATES).unwrap();
    assert!(w.cover(1, a, KAS, RATES).is_err(), "an account names one shard");
    assert!(w.cover(0, a, KAS, RATES).is_err());
    // The price doubles: the pot cannot pay the user, and the rest is one note on its shard, made by the closing.
    w.close(a, 2 * KAS).unwrap();
    assert!(w.close(a, 2 * KAS).is_err(), "an account closes once");
    let owed: Vec<&Note> = w.shards[0].notes.iter().filter(|n| matches!(n, Note::Owed { .. })).collect();
    assert_eq!(owed.len(), 1);
    assert!(w.shards[1].notes.is_empty(), "the other shard owes nothing");
    w.apply_all(0);
    assert_eq!(w.paid[0], 10 * KAS + 100 * KAS, "the user's margin and its gain of 100: the pot and the shard");
    assert_eq!(w.total(), w.entered);
}

#[test]
fn a_book_is_covered_along_a_path_and_the_shard_holds_the_paths_peak() {
    // Ten users long 100 and ten short 100: the book's own requirement is the gross rate alone, 4 KAS. Covered the
    // longs first, the shard must hold 5 % of 1,000 on the way; covered in turn, 5 % of 100.
    let mut w = World::default();
    w.shards.push(Shard::default());
    let long: Vec<usize> = (0..10).map(|u| w.open(u, 100, KAS, 10 * KAS, RATES)).collect();
    let short: Vec<usize> = (0..10).map(|u| w.open(10 + u, -100, KAS, 10 * KAS, RATES)).collect();
    w.fund(0, rate(2_000 * KAS, RATES.gross) + rate(100 * KAS, RATES.net));
    let mut one_way = w.clone();
    let covered = long.iter().take_while(|a| one_way.cover(0, **a, KAS, RATES).is_ok()).count();
    assert_eq!(covered, 1, "the longs first: the second is refused with what covers the book in turn");
    for k in 0..10 {
        w.cover(0, long[k], KAS, RATES).unwrap();
        w.cover(0, short[k], KAS, RATES).unwrap();
    }
    assert_eq!((w.shards[0].net, w.shards[0].gross), (0, 2_000));
}
