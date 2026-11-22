//! The committee, its epoch tables and exits in the script engine (docs/DESIGN-COMMITTEE.md 1, 2): every entry of `KpfCommittee`,
//! `KpfEpochTable` and `KpfExit`, what each writes and pays, and the negatives of every rule. States and values come from
//! the library model (`kpf_protocol::committee`); where a value is checked, one sompi less is refused.
//!
//! Fixture (`committeekit::rparams`): epochs of 1,000 DAA, exits of 500, members of at least 10 KAS, a quorum of 67 %
//! within 0.5 % of the price, no epoch table below 30 KAS of total stake, 3 epoch tables an epoch.

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kpf_protocol::committee::*;

fn full() -> Committee {
    Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS }
}

/// Members 0 to 2 only (30, 22 and 15 KAS).
fn three() -> Committee {
    let mut w = [0i64; MEMBERS];
    w[0] = 30;
    w[1] = 22;
    w[2] = 15;
    Committee { p: rparams(), from: 5_000, members: table(w), holds: kpf_protocol::committee::NO_HOLDS }
}

#[test]
fn the_states_are_the_models_bytes() {
    let k = ck();
    assert_eq!(k.committee_t.state_len, COMMITTEE_STATE_LEN);
    assert_eq!(k.epoch_table_t.state_len, EPOCH_TABLE_STATE_LEN);
    assert_eq!(k.exit_t.state_len, EXIT_STATE_LEN);
    assert_eq!(k.committee_t.prefix.len(), 1);
    // The compiler's sample instance is the model's encoding of the sample state, byte for byte.
    // (The pinned artifacts are compiled with the testnet defaults: `cmarket::samples`.)
    let r = Committee::empty(CommitteeParams::testnet(), 0);
    assert_eq!(k.committee_t.state_of(&bytecode(&k.committee)).unwrap(), &r.state_bytes()[..]);
    assert_eq!(Committee::decode_state(&r.state_bytes()).unwrap(), r);
    let sx = kpf_protocol::cmarket::samples::exit();
    assert_eq!(k.exit_t.state_of(&bytecode(&k.exit)).unwrap(), &sx.state_bytes()[..]);
    let sc = kpf_protocol::cmarket::samples::charge();
    assert_eq!(k.charge_t.state_len, CHARGE_STATE_LEN);
    assert_eq!(k.charge_t.state_of(&bytecode(&k.charge)).unwrap(), &sc.state_bytes()[..]);
    assert_eq!(Charge::decode_state(&sc.state_bytes()).unwrap(), sc);
    let r = Committee::empty(rparams(), 0);
    assert_eq!(Committee::decode_state(&r.state_bytes()).unwrap(), r);
    let f = full();
    assert_eq!(Committee::decode_state(&f.state_bytes()).unwrap(), f);
    let t = EpochTable { members: f.members, total: f.total(), from: 6_000, until: 7_000, quorum_bps: 6_700, spread_bps: 50 };
    assert_eq!(EpochTable::decode_state(&t.state_bytes()).unwrap(), t);
    assert_eq!(t.state_bytes().len(), EPOCH_TABLE_STATE_LEN);
    let e = Exit { key: [9; 32], amount: 5, release: 77, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge };
    assert_eq!(Exit::decode_state(&e.state_bytes()).unwrap(), e);
    println!(
        "scripts: KpfCommittee {} bytes, KpfEpochTable {} bytes, KpfExit {} bytes",
        bytecode(&k.committee).len(),
        bytecode(&k.epoch_table).len(),
        bytecode(&k.exit).len()
    );
}

#[test]
fn join_takes_an_empty_member() {
    let l = committee(&three());
    let key = x(&keypair(77));
    must("a 10 KAS stake takes member 5", &join_tx(&l, 5, key, 10 * KAS, |_| {}));
    refused("one sompi less than the stake enters the committee", &join_tx(&l, 5, key, 10 * KAS, |o| o[0].value -= 1), 0);
    refused("below minMemberStake", &join_tx(&l, 5, key, 10 * KAS - 1, |_| {}), 0);
    refused("member 15", &join_tx(&l, 15, key, 10 * KAS, |_| {}), 0);
    // The committee must have rested: REST_DAA after any change only a roll can spend it.
    refused("a committee one DAA short of its rest", &with_rest(REST_DAA - 1, || join_tx(&l, 5, key, 10 * KAS, |_| {})), 0);
    // The continuation is the model's state and nothing else: another member, another weight, another key, a moved epoch.
    let (good, _, _) = l.s.join(5, key, 10 * KAS).unwrap();
    for (what, bad) in [
        ("the stake written into member 6", { let mut r = l.s; r.members[6] = good.members[5]; r }),
        ("a larger weight than was paid", { let mut r = good; r.members[5].weight += 1; r }),
        ("another key", { let mut r = good; r.members[5].key = [1; 32]; r }),
        ("another member's weight raised", { let mut r = good; r.members[0].weight += KAS; r }),
        ("the epoch moved", { let mut r = good; r.from += 1; r }),
        ("a parameter changed", { let mut r = good; r.p.quorum_bps = 5_001; r }),
    ] {
        refused(what, &join_tx(&l, 5, key, 10 * KAS, |o| o[0].script_public_key = committee_spk(&bad)), 0);
    }
    // The continuation must carry the committee's id.
    let t = join_tx(&l, 5, key, 10 * KAS, |o| o[0].covenant = None);
    assert!(!accepted(&t), "a continuation without the id");
}

#[test]
fn join_outbids_only_the_least_member_of_a_full_table() {
    let l = committee(&full());
    let key = x(&keypair(77));
    // Member 7 holds 1 KAS, the least (with members 8 to 13); minMemberStake is 10 KAS, so the least bid is 10 KAS.
    assert_eq!(l.s.least_bid(7), Some(10 * KAS));
    assert_eq!(l.s.least_bid(0), None);
    let t = join_tx(&l, 7, key, 10 * KAS, |_| {});
    must("member 7 is outbid", &t);
    let (r, exit, delta) = l.s.join(7, key, 10 * KAS).unwrap();
    let e = exit.unwrap();
    assert_eq!((e.key, e.amount, e.release), (x(&member_kp(7)), KAS, 5_000 + 1_000 + 500));
    assert_eq!(delta, 9 * KAS);
    assert_eq!(r.members[7], Member { key, weight: 10 * KAS });
    // The holder's stake leaves whole, to its own key, released after the epoch and the exit time.
    refused("the exit holds one sompi less", &join_tx(&l, 7, key, 10 * KAS, |o| o[1].value -= 1), 0);
    refused("the committee keeps one sompi less", &join_tx(&l, 7, key, 10 * KAS, |o| o[0].value -= 1), 0);
    for (what, bad) in [
        ("the exit to another key", Exit { key: [1; 32], ..e }),
        ("the exit of a smaller amount", Exit { amount: e.amount - 1, ..e }),
        ("the exit released a DAA earlier", Exit { release: e.release - 1, ..e }),
        ("the exit released later", Exit { release: e.release + 1, ..e }),
    ] {
        refused(what, &join_tx(&l, 7, key, 10 * KAS, |o| o[1].script_public_key = exit_spk(&bad)), 0);
    }
    // A member that is not the least is not outbid, whatever the bid.
    refused("member 0 of 30 KAS for 1,000 KAS", &join_tx(&l, 0, key, 1_000 * KAS, |_| {}), 0);
    refused("member 5 of 2 KAS for 100 KAS", &join_tx(&l, 5, key, 100 * KAS, |_| {}), 0);
    // With a member empty, no taken member is outbid: the newcomer uses the free room.
    let mut holey = full();
    holey.members[14] = Member::EMPTY;
    refused("member 7 while member 14 is empty", &join_tx(&committee(&holey), 7, key, 100 * KAS, |_| {}), 0);
    must("the empty member 14", &join_tx(&committee(&holey), 14, key, 10 * KAS, |_| {}));
}

#[test]
fn a_bid_must_exceed_the_holder_by_a_twentieth() {
    // Every member 100 KAS: any member is a least member, and the least bid is 105 KAS.
    let l = committee(&Committee { p: rparams(), from: 0, members: table([100; MEMBERS]), holds: kpf_protocol::committee::NO_HOLDS });
    let key = x(&keypair(77));
    assert_eq!(l.s.least_bid(3), Some(105 * KAS));
    must("105 KAS", &join_tx(&l, 3, key, 105 * KAS, |_| {}));
    refused("one sompi below 105 KAS", &join_tx(&l, 3, key, 105 * KAS - 1, |_| {}), 0);
    refused("an equal bid", &join_tx(&l, 3, key, 100 * KAS, |_| {}), 0);
}

#[test]
fn top_up_and_leave() {
    let l = committee(&three());
    must("5 KAS more on member 1", &top_up_tx(&l, 1, 5 * KAS, |_| {}));
    refused("one sompi less enters the committee", &top_up_tx(&l, 1, 5 * KAS, |o| o[0].value -= 1), 0);
    refused("an empty member", &top_up_tx(&l, 9, 5 * KAS, |_| {}), 0);
    refused("nothing", &top_up_tx(&l, 1, 0, |_| {}), 0);
    refused("less than 1 KAS", &top_up_tx(&l, 1, KAS - 1, |_| {}), 0);
    must("1 KAS", &top_up_tx(&l, 1, KAS, |_| {}));
    refused("a committee one DAA short of its rest", &with_rest(REST_DAA - 1, || top_up_tx(&l, 1, 5 * KAS, |_| {})), 0);
    refused("a negative amount", &top_up_tx(&l, 1, -KAS, |_| {}), 0);
    assert_eq!(l.s.top_up(1, 5 * KAS).unwrap().members[1].weight, 27 * KAS);

    must("member 1 leaves", &leave_tx(&l, 1, &member_kp(1), |_| {}));
    let (r, e) = l.s.leave(1).unwrap();
    assert_eq!(r.members[1], Member::EMPTY);
    assert_eq!((e.amount, e.release), (22 * KAS, 6_500));
    refused("another member's key", &leave_tx(&l, 1, &member_kp(0), |_| {}), 0);
    refused("a committee one DAA short of its rest", &with_rest(REST_DAA - 1, || leave_tx(&l, 1, &member_kp(1), |_| {})), 0);
    refused("a stranger's key", &leave_tx(&l, 1, &keypair(77), |_| {}), 0);
    refused("an empty member", &leave_tx(&l, 9, &member_kp(9), |_| {}), 0);
    refused("the exit holds one sompi less", &leave_tx(&l, 1, &member_kp(1), |o| o[1].value -= 1), 0);
    refused("the committee keeps one sompi less", &leave_tx(&l, 1, &member_kp(1), |o| o[0].value -= 1), 0);
    refused("the exit to the leaver's other key", &leave_tx(&l, 1, &member_kp(1), |o| o[1].script_public_key = exit_spk(&Exit { key: [1; 32], ..e })), 0);
    refused("the exit released at once", &leave_tx(&l, 1, &member_kp(1), |o| o[1].script_public_key = exit_spk(&Exit { release: 0, ..e })), 0);
    // The member is cleared: a continuation that keeps it is refused.
    refused("the member kept", &leave_tx(&l, 1, &member_kp(1), |o| o[0].script_public_key = committee_spk(&l.s)), 0);
}

#[test]
fn roll_writes_the_epoch_tables_of_the_next_epoch() {
    let l = committee(&full());
    // The epoch is [5,000, 6,000): a roll needs a lock time of 6,000.
    must("m = 1 at 6,000", &roll_tx(&l, 1, 6_000, |_| {}));
    refused("m = 1 at 5,999", &roll_tx(&l, 1, 5_999, |_| {}), 0);
    must("m = 3 at 8,000", &roll_tx(&l, 3, 8_000, |_| {}));
    refused("m = 3 at 7,999", &roll_tx(&l, 3, 7_999, |_| {}), 0);
    refused("m = 0", &roll_tx(&l, 0, 9_000, |_| {}), 0);
    refused("m = -1", &roll_tx(&l, -1, 9_000, |_| {}), 0);
    let (r, t, _) = roll_outs(&l, 1);
    assert_eq!(l.s.roll(1, 6_000, l.value as i64).unwrap(), (r, t));
    assert_eq!((t.total, t.from, t.until, t.quorum()), (100 * KAS, 6_000, 7_000, 67 * KAS));
    // Exactly `epoch tables` epoch tables, each the table as it stands with its true sum, window and quorum, each funded.
    refused("two epoch tables", &roll_tx(&l, 1, 6_000, |o| { o.remove(3); }), 0);
    refused("four epoch tables", &roll_tx(&l, 1, 6_000, |o| { let x = o[3].clone(); o.insert(3, x); }), 0);
    refused("an epoch table of one sompi less", &roll_tx(&l, 1, 6_000, |o| o[2].value -= 1), 0);
    for (what, bad) in [
        ("an epoch table with a smaller total", EpochTable { total: t.total - 1, ..t }),
        ("an epoch table with a lower quorum", EpochTable { quorum_bps: 5_001, ..t }),
        ("an epoch table with a wider spread", EpochTable { spread_bps: 51, ..t }),
        ("an epoch table of the old epoch", EpochTable { from: 5_000, until: 6_000, ..t }),
        ("an epoch table with a longer window", EpochTable { until: t.until + 1, ..t }),
        ("an epoch table with another table", EpochTable { members: table([100; MEMBERS]), ..t }),
    ] {
        for j in 1..=3 {
            refused(what, &roll_tx(&l, 1, 6_000, |o| o[j].script_public_key = epoch_table_spk(&bad)), 0);
        }
    }
    refused("the committee keeps one sompi less", &roll_tx(&l, 1, 6_000, |o| o[0].value -= 1), 0);
    refused("the committee's epoch not moved", &roll_tx(&l, 1, 6_000, |o| o[0].script_public_key = committee_spk(&l.s)), 0);
    // The first output of the id is the committee: an epoch table in its place is refused.
    refused("an epoch table first", &roll_tx(&l, 1, 6_000, |o| o.swap(0, 1)), 0);
}

#[test]
fn roll_needs_the_minimum_stake_and_its_backing() {
    // Two members of 10 KAS: below minTotal (30 KAS).
    let mut w = [0i64; MEMBERS];
    w[0] = 10;
    w[1] = 10;
    let low = committee(&Committee { p: rparams(), from: 0, members: table(w), holds: kpf_protocol::committee::NO_HOLDS });
    refused("20 KAS of 30", &roll_tx(&low, 1, 1_000, |_| {}), 0);
    assert!(low.s.roll(1, 1_000, low.value as i64).is_err());
    // A committee whose value does not back its table writes no epoch table (a state only a genesis could make).
    let mut l = committee(&full());
    l.value = (l.s.total() - 1) as u64;
    refused("a table one sompi above the value", &roll_tx(&l, 1, 6_000, |_| {}), 0);
    l.value = l.s.total() as u64;
    must("a table exactly backed", &roll_tx(&l, 1, 6_000, |_| {}));
    // Parameters out of range: no epoch table.
    for (what, p) in [
        ("a quorum of a half", CommitteeParams { quorum_bps: 5_000, ..rparams() }),
        ("a quorum above all", CommitteeParams { quorum_bps: 10_001, ..rparams() }),
        ("no epoch tables", CommitteeParams { epoch_tables: 0, ..rparams() }),
        ("seven epoch tables", CommitteeParams { epoch_tables: 7, ..rparams() }),
        ("an epoch of 99 DAA", CommitteeParams { epoch: 99, ..rparams() }),
        ("an epoch table of 0.99 KAS", CommitteeParams { epoch_table_value: KAS - 1_000_000, ..rparams() }),
        ("a spread above 10 %", CommitteeParams { spread_bps: 1_001, ..rparams() }),
    ] {
        assert!(p.check().is_err(), "{what}");
        let l = committee(&Committee { p, from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
        assert!(!accepted(&roll_tx(&l, 1, 9_000_000, |_| {})), "{what}");
    }
}

fn the_epoch_table() -> LEpochTable {
    let (_, t, _) = roll_outs(&committee(&full()), 1);
    epoch_table(&t)
}

#[test]
fn attest_accepts_a_price_a_quorum_signed_near() {
    let l = the_epoch_table();
    let t = &l.s;
    // The three largest members (67 KAS of 100) sign 80,010, 80,000 and 79,990; the spread is 0.5 % (400 at 80,000).
    let s = signed_prices(6_500, &[(0, 80_010), (1, 80_000), (2, 79_990)]);
    let a = Attestation::collect(t, &rid(), &MARKET, 6_500, &s).unwrap();
    // The collector claims the middle of the prices all three are near.
    assert_eq!((a.p, a.bits), (80_000, 0b111));
    assert_eq!(a.check(t, &rid(), 6_500), Ok(67 * KAS));
    must("the three largest members", &attest_tx(&l, &a, 6_500, |_| {}));
    // Any price all three lie within 0.5 % of is attested: from 79,612 (80,010 is 398 above, its 0.5 % is 398.06) to
    // 80,391 (79,990 is 401 below, its 0.5 % is 401.96).
    for (p, ok) in [(79_611, false), (79_612, true), (80_000, true), (80_391, true), (80_392, false), (0, false)] {
        let b = a.clone().claiming(p);
        assert_eq!(b.check(t, &rid(), 6_500).is_ok(), ok, "price {p} (model)");
        assert_eq!(accepted(&attest_tx(&l, &b, 6_500, |_| {})), ok, "price {p}");
    }
    // With no spread every included member must have signed the price itself.
    let exact = epoch_table(&EpochTable { spread_bps: 0, ..*t });
    let same = Attestation::collect(&exact.s, &rid(), &MARKET, 6_500, &signed_prices(6_500, &[(0, 80_000), (1, 80_000), (2, 80_000)])).unwrap();
    must("three equal prices, no spread", &attest_tx(&exact, &same, 6_500, |_| {}));
    assert!(Attestation::collect(&exact.s, &rid(), &MARKET, 6_500, &s).is_err());
    refused("three different prices, no spread", &attest_tx(&exact, &a, 6_500, |_| {}), 0);
}

#[test]
fn attest_refuses_below_the_quorum_and_what_no_member_signed() {
    let l = the_epoch_table();
    let t = &l.s;
    let present = |list: &[(usize, i64)], p: i64| -> Attestation {
        let s = signed_prices(6_500, list);
        Attestation::of(t, &MARKET, 6_500, p, &list.iter().zip(&s).map(|((i, _), tk)| (*i, *tk)).collect::<Vec<_>>())
    };
    // 30 + 12 + 8 = 50 and 30 + 22 + 12 = 64 of 100.
    for set in [[0usize, 3, 4], [0, 1, 3]] {
        let list: Vec<(usize, i64)> = set.iter().map(|i| (*i, 80_000)).collect();
        assert!(Attestation::collect(t, &rid(), &MARKET, 6_500, &signed_prices(6_500, &list)).is_err());
        refused("below the quorum", &attest_tx(&l, &present(&list, 80_000), 6_500, |_| {}), 0);
    }
    let s = signed_prices(6_500, &[(0, 80_000), (1, 80_000), (2, 80_000)]);
    let a = Attestation::collect(t, &rid(), &MARKET, 6_500, &s).unwrap();
    must("the quorum", &attest_tx(&l, &a, 6_500, |_| {}));
    // Review C1 (C1-F1): members 1 and 3 (34 KAS) sign 98,400; members 0, 5 and 7 (33 KAS) signed the true 80,000. Presented
    // together they are 67 KAS, but no price is within the spread of both: neither the liars' price, nor the true one,
    // nor one between them is attested.
    let mixed = [(0usize, 80_000i64), (1, 98_400), (3, 98_400), (5, 80_000), (7, 80_000)];
    assert!(Attestation::collect(t, &rid(), &MARKET, 6_500, &signed_prices(6_500, &mixed)).is_err());
    for p in [98_400, 80_000, 89_200] {
        let b = present(&mixed, p);
        assert!(b.check(t, &rid(), 6_500).is_err(), "price {p} (model)");
        refused("half a quorum and honest signatures", &attest_tx(&l, &b, 6_500, |_| {}), 0);
    }
    // A signature by another key in member 1's place.
    let mut b = a.clone();
    b.sigs[1] = sign_price(&keypair(77), &rid(), &MARKET, 6_500, 80_000).sig;
    assert!(!accepted(&attest_tx(&l, &b, 6_500, |_| {})), "a forged signature");
    assert!(b.check(t, &rid(), 6_500).is_err());
    // Member 1's signature of another price, another signed price, another market, another committee.
    for (what, sig) in [
        ("another price", sign_price(&member_kp(1), &rid(), &MARKET, 6_500, 80_001).sig),
        ("another signed price", sign_price(&member_kp(1), &rid(), &MARKET, 6_501, 80_000).sig),
        ("another market", sign_price(&member_kp(1), &rid(), &[0x4e; 32], 6_500, 80_000).sig),
        ("another committee", sign_price(&member_kp(1), &[0x53; 32], &MARKET, 6_500, 80_000).sig),
    ] {
        let mut b = a.clone();
        b.sigs[1] = sig;
        assert!(!accepted(&attest_tx(&l, &b, 6_500, |_| {})), "{what}");
    }
    // A member counted twice is not possible (one bit a member); a bit beyond the table is refused.
    let mut b = a.clone();
    b.bits |= 1 << 15;
    refused("a sixteenth member", &attest_tx(&l, &b, 6_500, |_| {}), 0);
    // An empty member cannot sign: an epoch table of three members, and a signature in member 9's place.
    let (_, small, _) = roll_outs(&committee(&three()), 1);
    let ls = epoch_table(&small);
    let mut b = Attestation::collect(&small, &rid(), &MARKET, 6_500, &s).unwrap();
    must("three members of three", &attest_tx(&ls, &b, 6_500, |_| {}));
    b.bits |= 1 << 9;
    b.prices[9] = 80_000;
    b.sigs[9] = sign_price(&keypair(77), &rid(), &MARKET, 6_500, 80_000).sig;
    assert!(!accepted(&attest_tx(&ls, &b, 6_500, |_| {})), "an empty member signed");
}

#[test]
fn attest_serves_its_window_and_no_price_of_the_future() {
    let l = the_epoch_table();
    for (t0, ok) in [(5_999, false), (6_000, true), (6_999, true), (7_000, false)] {
        let s = signed_prices(t0, &[(0, 80_000), (1, 80_000), (2, 80_000)]);
        let a = Attestation::of(&l.s, &MARKET, t0, 80_000, &[(0, s[0]), (1, s[1]), (2, s[2])]);
        assert_eq!(accepted(&attest_tx(&l, &a, t0 as u64, |_| {})), ok, "t0 {t0}");
        assert_eq!(a.check(&l.s, &rid(), t0).is_ok(), ok, "t0 {t0} (model)");
    }
    let s = signed_prices(6_500, &[(0, 80_000), (1, 80_000), (2, 80_000)]);
    let a = Attestation::collect(&l.s, &rid(), &MARKET, 6_500, &s).unwrap();
    refused("a lock time one DAA before the signed price", &attest_tx(&l, &a, 6_499, |_| {}), 0);
    must("a lock time long after the signed price", &attest_tx(&l, &a, 9_999, |_| {}));
    // The epoch table continues unchanged, at its own output index, with its value and id.
    refused("one sompi less", &attest_tx(&l, &a, 6_500, |o| o[0].value -= 1), 0);
    let other = EpochTable { until: l.s.until + 1_000, ..l.s };
    refused("a longer window written", &attest_tx(&l, &a, 6_500, |o| o[0].script_public_key = epoch_table_spk(&other)), 0);
    assert!(!accepted(&attest_tx(&l, &a, 6_500, |o| o[0].covenant = None)), "the id dropped");
    assert!(!accepted(&attest_tx(&l, &a, 6_500, |o| o.swap(0, 1))), "the epoch table at another index");
}

#[test]
fn a_epoch_table_retires_a_window_after_its_end() {
    let l = the_epoch_table();
    assert_eq!(l.s.retire_from(), 8_000);
    must("at 8,000", &retire_tx(&l, 8_000, |_| {}));
    refused("at 7,999", &retire_tx(&l, 7_999, |_| {}), 0);
    refused("an output of the id", &retire_tx(&l, 8_000, |o| o.insert(0, out(1, epoch_table_spk(&l.s), bind(0, RID)))), 0);
}

#[test]
fn an_exit_pays_its_key_once_released() {
    let e = Exit { key: x(&member_kp(1)), amount: 22 * KAS, release: 6_500, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge };
    let l = Live { op: outpoint(3), value: (22 * KAS) as u64, daa: 0, id: Some(RID), s: e };
    must("at 6,500", &withdraw_tx(&l, 6_500, |_| {}));
    refused("at 6,499", &withdraw_tx(&l, 6_499, |_| {}), 0);
    assert_eq!(e.withdraw(6_500), Ok(22 * KAS));
    assert!(e.withdraw(6_499).is_err());
    refused("one sompi less", &withdraw_tx(&l, 6_500, |o| o[0].value -= 1), 0);
    refused("to another key", &withdraw_tx(&l, 6_500, |o| o[0].script_public_key = p2pk_spk(&keypair(77))), 0);
    refused("an output of the id", &withdraw_tx(&l, 6_500, |o| o.push(out(1, exit_spk(&e), bind(0, RID)))), 0);
}

/// A small deterministic generator (xorshift), so a failing walk is reproducible from its seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Random joins, top-ups, leaves and rolls: the engine accepts exactly what the model accepts, and the committee's value
/// is always its members' stake plus what it began with.
#[test]
fn the_engine_agrees_with_the_model_on_a_random_walk() {
    for seed in 1..=6u64 {
        let mut g = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut l = committee(&Committee::empty(rparams(), 0));
        let base = l.value as i64;
        let mut now = 0i64;
        let (mut ok, mut no) = (0, 0);
        for step in 0..120 {
            now += g.below(700) as i64;
            let i = g.below(16) as usize;
            let kp = keypair(1 + g.below(40) as u8);
            let amount = match g.below(4) {
                0 => 10 * KAS - 1,
                1 => 10 * KAS + g.below(50) as i64 * KAS,
                2 => l.s.members[i.min(14)].weight + l.s.members[i.min(14)].weight / 20,
                _ => g.below(400) as i64 * KAS,
            };
            let (t, next): (T, Option<Committee>) = match g.below(4) {
                0 => (join_tx(&l, i, x(&kp), amount, |_| {}), if i < MEMBERS { l.s.join(i, x(&kp), amount).ok().map(|r| r.0) } else { None }),
                1 if i < MEMBERS => (top_up_tx(&l, i, amount, |_| {}), l.s.top_up(i, amount).ok()),
                2 if i < MEMBERS => {
                    // The member's own key half of the time (the kit's members are held by whoever joined).
                    let holder = (1..=40u8).map(keypair).find(|k| x(k) == l.s.members[i].key);
                    let signer = if g.below(2) == 0 { holder.unwrap_or(kp) } else { kp };
                    let good = holder.is_some_and(|h| x(&h) == x(&signer));
                    (leave_tx(&l, i, &signer, |_| {}), if good { l.s.leave(i).ok().map(|r| r.0) } else { None })
                }
                _ => {
                    let m = 1 + g.below(3) as i64;
                    (roll_tx(&l, m, now as u64, |_| {}), l.s.roll(m, now, l.value as i64).ok().map(|r| r.0))
                }
            };
            assert_eq!(accepted(&t), next.is_some(), "seed {seed} step {step}: the engine and the model disagree");
            if let Some(r) = next {
                l = live(&t, 0, now as u64, &r);
                assert_eq!(l.value as i64, r.total() + base, "seed {seed} step {step}: the value is the stake");
                ok += 1;
            } else {
                no += 1;
            }
        }
        assert!(ok > 20 && no > 20, "seed {seed}: {ok} accepted, {no} refused");
    }
}

#[test]
fn a_committee_whose_value_does_not_cover_its_table_accepts_nothing() {
    // Review C2 (C2-F5): a state only a genesis could write. Every entry checks that the table it writes is covered.
    let key = x(&keypair(77));
    let mut short = committee(&three());
    short.value = (short.s.total() - 1) as u64;
    assert!(!short.s.backed(short.value as i64));
    refused("a join into a committee one sompi short", &join_tx(&short, 5, key, 10 * KAS, |_| {}), 0);
    refused("a top-up", &top_up_tx(&short, 1, 5 * KAS, |_| {}), 0);
    refused("a leave", &leave_tx(&short, 1, &member_kp(1), |_| {}), 0);
    refused("a roll", &roll_tx(&short, 1, 6_000, |_| {}), 0);
    // A forged member of 100 KAS over 1 KAS: the newcomer's stake cannot enter, so it cannot pay the forged member's exit.
    let mut forged = committee(&Committee { p: rparams(), from: 5_000, members: { let mut t = EMPTY_TABLE; t[0] = Member { key: x(&member_kp(0)), weight: 100 * KAS }; t }, holds: kpf_protocol::committee::NO_HOLDS });
    forged.value = KAS as u64;
    refused("a join beside a forged member", &join_tx(&forged, 1, key, 100 * KAS, |_| {}), 0);
    refused("the forged member's leave", &leave_tx(&forged, 0, &member_kp(0), |_| {}), 0);
    // Exactly covered: every entry works.
    let mut exact = committee(&three());
    exact.value = exact.s.total() as u64;
    must("a join into a committee exactly covered", &join_tx(&exact, 5, key, 10 * KAS, |_| {}));
    must("a leave", &leave_tx(&exact, 1, &member_kp(1), |_| {}));
}

// ---------------------------------------------------------------- stage 2: two prices for one time

fn burn_spk() -> kaspa_consensus_core::tx::ScriptPublicKey {
    kaspa_txscript::pay_to_script_hash_script(&[0x6a])
}

/// The arguments of a forfeit: the market, the signed price, and two signed prices (each by the key given).
fn proof(market: &[u8; 32], t0: (i64, i64), p: (i64, i64), keys: (&secp256k1::Keypair, &secp256k1::Keypair)) -> Vec<Arg> {
    let a = sign_price(keys.0, &rid(), market, t0.0, p.0);
    let b = sign_price(keys.1, &rid(), market, t0.1, p.1);
    vec![bv(&MARKET), b8(t0.0), b8(p.0), bv(&a.sig), b8(p.1), bv(&b.sig)]
}

fn member_forfeit_tx(l: &LCommittee, i: usize, args: Vec<Arg>, tweak: impl FnOnce(&mut Vec<kaspa_consensus_core::tx::TransactionOutput>)) -> T {
    let (r, w, burn) = l.s.forfeit(i).unwrap_or((l.s, 0, 0));
    let mut outs = vec![out((l.value as i64 - w) as u64, committee_spk(&r), bind(0, RID)), change(1), out(burn.max(1) as u64, burn_spk(), None)];
    tweak(&mut outs);
    let mut a = vec![iv(i as i64)];
    a.extend(args);
    build(&[committee_in(l, "forfeit", a), exec_in(60)], outs)
}

#[test]
fn a_member_that_signed_two_prices_for_one_price_is_slashed_by_anyone() {
    let l = committee(&three());
    let k1 = member_kp(1);
    let good = || proof(&MARKET, (6_500, 6_500), (80_000, 80_001), (&k1, &k1));
    // Member 1 holds 22 KAS: it is emptied, 19.8 KAS are burned, the sender keeps 2.2 KAS. No key and no rest is needed.
    let (r, w, burn) = l.s.forfeit(1).unwrap();
    assert_eq!((r.members[1], w, burn), (Member::EMPTY, 22 * KAS, 22 * KAS - 22 * KAS / 10));
    must("two prices for the time 6,500", &member_forfeit_tx(&l, 1, good(), |_| {}));
    refused("one sompi less burned", &member_forfeit_tx(&l, 1, good(), |o| o[2].value -= 1), 0);
    refused("the burn paid to a key", &member_forfeit_tx(&l, 1, good(), |o| o[2].script_public_key = p2pk_spk(&keypair(77))), 0);
    refused("the committee keeps one sompi less", &member_forfeit_tx(&l, 1, good(), |o| o[0].value -= 1), 0);
    refused("the member not emptied", &member_forfeit_tx(&l, 1, good(), |o| { o[0].script_public_key = committee_spk(&l.s); o[0].value = l.value; }), 0);
    // What is not a proof.
    for (what, args, member) in [
        ("the same price twice", proof(&MARKET, (6_500, 6_500), (80_000, 80_000), (&k1, &k1)), 1),
        ("two signed prices", proof(&MARKET, (6_500, 6_501), (80_000, 80_001), (&k1, &k1)), 1),
        ("the second signature by another member", proof(&MARKET, (6_500, 6_500), (80_000, 80_001), (&k1, &member_kp(0))), 1),
        ("both signatures by another member", proof(&MARKET, (6_500, 6_500), (80_000, 80_001), (&member_kp(0), &member_kp(0))), 1),
        ("signatures of another market", proof(&[0x4e; 32], (6_500, 6_500), (80_000, 80_001), (&k1, &k1)), 1),
        ("an empty member", proof(&MARKET, (6_500, 6_500), (80_000, 80_001), (&member_kp(9), &member_kp(9))), 9),
    ] {
        assert!(!accepted(&member_forfeit_tx(&l, member, args, |_| {})), "{what}");
    }
    // The committee's script holds one transaction signature and three data signatures (two here, one of `charge`):
    // within a node's 15.
    let t = member_forfeit_tx(&l, 1, good(), |_| {});
    assert_eq!(kpf_protocol::verify::data_sig_ops(&t.0.inputs[0].signature_script), 3);
}

#[test]
fn a_stake_on_its_way_out_is_slashed_by_the_same_proof() {
    let k1 = member_kp(1);
    let e = Exit { key: x(&k1), amount: 22 * KAS, release: 6_500, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge };
    let l = Live { op: outpoint(3), value: (22 * KAS) as u64, daa: 0, id: Some(RID), s: e };
    let burn = e.forfeit().unwrap();
    assert_eq!(burn, 22 * KAS - 22 * KAS / 10);
    let tx = |args: Vec<Arg>, tweak: &dyn Fn(&mut Vec<kaspa_consensus_core::tx::TransactionOutput>)| {
        let mut outs = vec![change(1), change(1), out(burn as u64, burn_spk(), None)];
        tweak(&mut outs);
        build(&[exit_in(&l, "forfeit", args), exec_in(60)], outs)
    };
    let good = || proof(&MARKET, (6_400, 6_400), (80_000, 79_000), (&k1, &k1));
    must("two prices by the exit's key, before or after its release", &tx(good(), &|_| {}));
    refused("one sompi less burned", &tx(good(), &|o| o[2].value -= 1), 0);
    refused("an output of the id", &tx(good(), &|o| o[0] = out(1, exit_spk(&e), bind(0, RID))), 0);
    assert!(!accepted(&tx(proof(&MARKET, (6_400, 6_400), (80_000, 79_000), (&member_kp(0), &member_kp(0))), &|_| {})), "another key's signatures");
    assert!(!accepted(&tx(proof(&MARKET, (6_400, 6_400), (80_000, 80_000), (&k1, &k1)), &|_| {})), "one price");
    // A stake too small to burn (below 0.02 KAS of burn) needs no burn output; none exists above minMemberStake.
    assert_eq!(burn_of(2_000_000), 0);
    assert_eq!(burn_of(3_000_000), 2_700_000);
}
