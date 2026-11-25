//! Research R4 (`docs/RESEARCH-UNIFIED-MARGIN.md`, section 8a): a clearing party's accounts behind one reserve that
//! holds a rate on its net position. A model in plain numbers, before any object or script: a book of accounts, a
//! move of the price, the clearing party doing nothing (it has failed), and whoever is owed settling for itself.
//! The questions: who is paid, in which order, and how much capital the form needs beside two-party margins.

const KAS: i64 = 100_000_000;

/// An account between a user and the clearing party, in one market, struck at the same entry as every other.
#[derive(Clone, Copy, Debug)]
struct Account {
    /// The user is long (the clearing party short) or short.
    user_long: bool,
    /// Units, and each side's margin in sompi.
    size: i64,
    user: i64,
    clearing: i64,
}

/// What a move of `d` sompi a unit does to an account when it is settled: what the user is owed, what the account's
/// pot pays it, what it lacks (the clearing party's failure), and what the clearing party's side is worth there.
fn settle(a: &Account, d: i64) -> (i64, i64, i64, i64) {
    let pot = a.user + a.clearing;
    let gain = if a.user_long { d } else { -d } * a.size;
    let owed = (a.user + gain).max(0);
    let paid = owed.min(pot);
    (owed, paid, owed - paid, pot - paid)
}

/// The book settled after a move, in the order `order` (indices of accounts), by a clearing party that has failed:
/// an account in which the user is owed more than the pot takes the rest from the reserve, as far as it reaches; an
/// account in which the clearing party's side is worth something pays that into the reserve when it is settled.
/// Returns what each account's user lacked in the end, and what is left in the reserve.
fn run(book: &[Account], reserve: i64, d: i64, order: &[usize]) -> (Vec<i64>, i64) {
    let mut held = reserve;
    let mut lacked = vec![0; book.len()];
    for &i in order {
        let (_, _, short, clearing_side) = settle(&book[i], d);
        held += clearing_side;
        let from_reserve = short.min(held);
        held -= from_reserve;
        lacked[i] = short - from_reserve;
    }
    (lacked, held)
}

struct Rng(u64);
impl Rng {
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        lo + (self.0 % (hi - lo) as u64) as i64
    }
}

/// A book of `n` accounts at the entry `e`: users long with probability `long_pct`, each posting `user_bps` of its
/// notional; the clearing party posting `gross_bps` of the notional in each account.
fn book(r: &mut Rng, n: usize, e: i64, long_pct: i64, user_bps: i64, gross_bps: i64) -> Vec<Account> {
    (0..n)
        .map(|_| {
            let size = r.range(1, 50);
            let notional = size * e;
            Account { user_long: r.range(0, 100) < long_pct, size, user: notional * user_bps / 10_000, clearing: notional * gross_bps / 10_000 }
        })
        .collect()
}

fn gross_net(b: &[Account], e: i64) -> (i64, i64) {
    let (l, s): (i64, i64) = b.iter().fold((0, 0), |(l, s), a| if a.user_long { (l + a.size, s) } else { (l, s + a.size) });
    ((l + s) * e, (l - s).abs() * e)
}

#[test]
fn r4_a_reserve_on_the_net_pays_every_user_up_to_its_rate_when_gains_are_settled_first() {
    let e = 10 * KAS;
    let mut r = Rng(0x243f_6a88_85a3_08d3);
    println!("R4 books of 200 accounts, users posting 20 %; the clearing party posts 0.5 % of each account and reserves 5 % of its net");
    println!("R4 {:>12} {:>10} {:>10} {:>16} {:>14} {:>12}", "long users %", "gross KAS", "net KAS", "two-party 5 %", "this form", "ratio");
    for long_pct in [50, 55, 60, 70, 85, 100] {
        let b = book(&mut r, 200, e, long_pct, 2_000, 50);
        let (gross, net) = gross_net(&b, e);
        let reserve = net * 500 / 10_000;
        let capital = reserve + b.iter().map(|a| a.clearing).sum::<i64>();
        let two_party = gross * 500 / 10_000;
        println!("R4 {:>12} {:>10} {:>10} {:>16} {:>14} {:>11.2}x", long_pct, gross / KAS, net / KAS, two_party / KAS, capital / KAS, two_party as f64 / capital as f64);
        // Moves of up to 5 % either way: with the accounts in which the clearing party gained settled first, every
        // user is paid in full, whatever the book.
        for bps in [-500i64, -300, -100, 100, 300, 500] {
            let d = e * bps / 10_000;
            let mut order: Vec<usize> = (0..b.len()).collect();
            order.sort_by_key(|i| settle(&b[*i], d).2);
            let (lacked, _) = run(&b, reserve, d, &order);
            assert!(lacked.iter().all(|x| *x == 0), "{long_pct} % long, a move of {bps} bps: {} unpaid", lacked.iter().sum::<i64>());
        }
        // Beyond the rate the reserve is short by what the algebra says: the move on the net, less what the clearing
        // party posted across the book and the reserve.
        for bps in [800i64, 1_500] {
            for sign in [1i64, -1] {
                let d = sign * e * bps / 10_000;
                let mut order: Vec<usize> = (0..b.len()).collect();
                order.sort_by_key(|i| settle(&b[*i], d).2);
                let (lacked, _) = run(&b, reserve, d, &order);
                let all: i64 = b.iter().map(|a| settle(a, d).2).sum();
                let assets: i64 = b.iter().map(|a| settle(a, d).3).sum::<i64>() + reserve;
                assert_eq!(lacked.iter().sum::<i64>(), (all - assets).max(0), "{long_pct} % long, {} bps", sign * bps);
            }
        }
    }
}

#[test]
fn r4_the_order_of_settling_decides_who_is_paid() {
    // The same book and move, the claims first and the gains last: the first claimants are paid from the reserve
    // alone and the later ones lack, though the book as a whole could pay them all. Whoever claims has therefore to
    // settle the accounts in which the clearing party gained before its own: the form must let anyone do that.
    let e = 10 * KAS;
    let mut r = Rng(0x1319_8a2e_0370_7344);
    let b = book(&mut r, 200, e, 55, 2_000, 50);
    let (_, net) = gross_net(&b, e);
    let reserve = net * 500 / 10_000;
    let d = e * 400 / 10_000;
    let mut gains_first: Vec<usize> = (0..b.len()).collect();
    gains_first.sort_by_key(|i| settle(&b[*i], d).2);
    let claims_first: Vec<usize> = gains_first.iter().rev().copied().collect();
    let (ok, _) = run(&b, reserve, d, &gains_first);
    let (bad, left) = run(&b, reserve, d, &claims_first);
    assert!(ok.iter().all(|x| *x == 0));
    let unpaid: i64 = bad.iter().sum();
    assert!(unpaid > 0, "claims before gains leave somebody short");
    println!("R4 the same book and a move of 4 %: gains settled first, nobody lacks; claims first, {} users lack {} KAS in all while {} KAS end up in the reserve", bad.iter().filter(|x| **x > 0).count(), unpaid / KAS, left / KAS);
    assert!(left >= unpaid, "what they lacked is in the reserve in the end: the order alone kept it from them");
}

#[test]
fn r4_a_user_that_cannot_pay_is_the_clearing_partys_loss() {
    // Users posting 2 % and a move of 5 %: on the losing side every user owes more than it posted, the clearing
    // party's gain there is capped at their margins, and the users on the other side lack that much.
    let e = 10 * KAS;
    let mut r = Rng(0xa409_3822_299f_31d0);
    let b = book(&mut r, 200, e, 50, 200, 50);
    let (_, net) = gross_net(&b, e);
    let reserve = net * 500 / 10_000;
    // (The move is the way the larger side of the book wins.)
    let longs: i64 = b.iter().filter(|a| a.user_long).map(|a| a.size).sum();
    let shorts: i64 = b.iter().filter(|a| !a.user_long).map(|a| a.size).sum();
    let up = longs >= shorts;
    let d = if up { 1 } else { -1 } * (e * 500 / 10_000);
    let mut order: Vec<usize> = (0..b.len()).collect();
    order.sort_by_key(|i| settle(&b[*i], d).2);
    let (lacked, _) = run(&b, reserve, d, &order);
    let unpaid: i64 = lacked.iter().sum();
    let uncollected: i64 = b.iter().filter(|a| a.user_long != up).map(|a| (d.abs() * a.size - a.user).max(0)).sum();
    assert!(unpaid > 0 && uncollected > 0);
    println!("R4 users posting 2 % and a move of 5 %: the losing users owe {} KAS beyond their margins; the winning users lack {} KAS", uncollected / KAS, unpaid / KAS);
    // With users liquidated before their margin is gone (posting 20 %), the same move leaves nobody short.
    let b = book(&mut Rng(0xa409_3822_299f_31d0), 200, e, 50, 2_000, 50);
    let (_, net) = gross_net(&b, e);
    let mut order: Vec<usize> = (0..b.len()).collect();
    order.sort_by_key(|i| settle(&b[*i], d).2);
    assert!(run(&b, net * 500 / 10_000, d, &order).0.iter().all(|x| *x == 0));
}

/// An account with its own entry (the others above share one): what a move to the price `p` does to it.
#[derive(Clone, Copy, Debug)]
struct Struck {
    a: Account,
    entry: i64,
}

fn at(s: &Struck, p: i64) -> (i64, i64, i64, i64) {
    settle(&s.a, p - s.entry)
}

/// The maintenance of an account's user at `p`: 5 % of its notional there.
fn maintenance(s: &Struck, p: i64) -> i64 {
    s.a.size * p * 500 / 10_000
}

/// What a shard must hold at the price `p` for the accounts it covers: 5 % of the net and 0.5 % of the gross.
fn requirement(book: &[Struck], p: i64) -> i64 {
    let (l, s): (i64, i64) = book.iter().fold((0, 0), |(l, s), x| if x.a.user_long { (l + x.a.size, s) } else { (l, s + x.a.size) });
    (l - s).abs() * p * 500 / 10_000 + (l + s) * p * 50 / 10_000
}

#[test]
fn r4_accounts_of_its_own_do_not_dress_the_net_once_anyone_marks_and_calls() {
    // A clearing party short 1,000 units against real users (long from 10, posting 20 %). To look flat it opens 1,000
    // units the other way with a key of its own, struck at 0.01 with 5 % of that notional: in units its net is
    // nothing, and a requirement counted in units at the accounts' own prices is nearly nothing.
    let (e, units) = (10 * KAS, 1_000i64);
    let real: Vec<Struck> = (0..20).map(|_| Struck { a: Account { user_long: true, size: units / 20, user: units / 20 * e * 2_000 / 10_000, clearing: units / 20 * e * 50 / 10_000 }, entry: e }).collect();
    let cheap = KAS / 100;
    let own = Struck { a: Account { user_long: false, size: units, user: units * cheap * 500 / 10_000, clearing: units * cheap * 50 / 10_000 }, entry: cheap };
    let mut book = real.clone();
    book.push(own);
    let dressed = requirement(&book, cheap).max(requirement(&book, e));
    let honest = requirement(&real, e);
    println!("R4 a net dressed with an account of its own: the shard is asked {} KAS where the real book asks {} KAS", dressed / KAS, honest / KAS);
    assert!(dressed * 5 < honest, "dressed, the requirement is a fraction of the real one");
    let held = dressed;

    // 1. Nobody marks. The price rises 5 %: the real users are owed 500 KAS beyond what the clearing party posted in
    //    their accounts; its own account's user side holds a sompi's worth of margin against a loss of 10,490 KAS.
    let p = e * 10_500 / 10_000;
    let owed: i64 = real.iter().map(|x| at(x, p).2).sum();
    let collectable = at(&own, p).3;
    let unpaid = (owed - collectable - held).max(0);
    println!("R4 unmarked, after a rise of 5 %: the users lack {} KAS; the account of its own gives {} KAS", unpaid / KAS, collectable / KAS);
    assert!(unpaid > 300 * KAS);

    // 2. Anyone marks at the cited price of 10, before any move. The account of its own has a user side far below its
    //    maintenance there: it is liquidated and leaves the net. What is left is the real book, and the shard holds a
    //    fraction of its requirement: it is called. At 10 every real user is owed what it posted, and its account's
    //    pot pays it: nobody lacks.
    let below = at(&own, e).0 < maintenance(&own, e);
    assert!(below, "the user side of the account of its own is below its maintenance at the cited price");
    let after: Vec<Struck> = book.iter().copied().filter(|x| at(x, e).0 >= maintenance(x, e)).collect();
    assert_eq!(after.len(), real.len());
    assert!(held < requirement(&after, e), "the shard is under its requirement: anyone calls it");
    assert!(after.iter().all(|x| at(x, e).2 == 0), "closed at the cited price, every user is paid by its own account");

    // 3. An account of its own that is struck at the market and holds a user's margin is not dress: it is collateral.
    //    With 5 % posted on its user side (the maintenance), a rise of 5 % is paid out of it.
    let backed = Struck { a: Account { user_long: false, size: units, user: units * e * 500 / 10_000, clearing: units * e * 50 / 10_000 }, entry: e };
    let mut book = real.clone();
    book.push(backed);
    let held = requirement(&book, e);
    let owed: i64 = real.iter().map(|x| at(x, p).2).sum();
    assert!(at(&backed, p).3 + held >= owed, "what the account of its own holds, and the reserve, pay the users");
    println!("R4 an account of its own struck at the market with the maintenance posted: the users are owed {} KAS and it gives {} KAS", owed / KAS, at(&backed, p).3 / KAS);
}
