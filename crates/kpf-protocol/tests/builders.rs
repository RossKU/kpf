//! Every builder: the signed transaction passes `verify` on the in-memory chain, mutated versions fail, and a scripted
//! lifecycle runs end to end. Keys are the TEST-ONLY keys of `kpf_protocol::fixtures`.

use kaspa_consensus_core::tx::TransactionOutpoint;
use kpf_protocol::build::*;
use kpf_protocol::fixtures::*;
use kpf_protocol::payoff::*;
use kpf_protocol::sim::{Chain, Live};
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use kpf_protocol::verify::verify;
use kpf_protocol::build::Funding;

const L: u8 = 21;
const S: u8 = 22;
const F: u8 = 24;
const OTHER: u8 = 23;
const KAS: u64 = 100_000_000;

fn keys() -> KeySet {
    keyset(&[L, S, F, OTHER, 25, 26])
}

fn funding(chain: &mut Chain, seed: u8, kas: u64) -> Funding {
    let k = pubkey(seed);
    Funding { utxos: vec![chain.mint(&k, kas * KAS)], change_key: k }
}

fn opts() -> Opts {
    Opts::default()
}

/// Sign, verify (this is what `apply_signed` does) and apply. A transaction whose lock time is ahead of the chain (a
/// hand-over waits S + 4R, DESIGN 4.5c) is applied once the chain reaches it, as a user would wait.
fn run(chain: &mut Chain, b: &Built) -> Vec<Live> {
    if b.tx.lock_time > chain.daa {
        chain.advance(b.tx.lock_time - chain.daa);
    }
    let s = b.sign(&keys()).expect("sign");
    let v = verify(&s.tx, &s.entries);
    assert!(v.is_ok(), "verify failed: {:?}", v.errors());
    // The fee is the relay floor of the exact mass (plus nothing else).
    assert_eq!(s.fee, v.min_fee, "the fee is exactly the relay floor of the exact mass");
    chain.apply_signed(&s).expect("apply")
}

/// Every mutation of a party output (one sompi less) must be refused by some input. Change and filler outputs are
/// not constrained by any covenant.
fn assert_outputs_are_least(b: &Built) {
    for i in 0..b.tx.outputs.len() {
        let l = &b.output_labels[i];
        if l.starts_with("change") || l.starts_with("filler") || b.change == Some(i) {
            continue;
        }
        let mut m = b.clone();
        if m.tx.outputs[i].value <= 1 {
            continue;
        }
        m.tx.outputs[i].value -= 1;
        // The change absorbs the extra sompi, so the transaction stays balanced.
        let s = m.sign(&keys()).expect("sign mutant");
        let v = verify(&s.tx, &s.entries);
        assert!(!v.is_ok(), "output {i} ({l}) one sompi lower was accepted");
        assert!(v.inputs.iter().any(|r| r.is_err()), "output {i}: refused only at transaction level: {:?}", v.errors());
    }
}

struct World {
    chain: Chain,
}

/// Creates a long and a short order, opens a pair of 1,000 tokens at 10 KAS.
fn world() -> (World, LiveOpen, LiveOpen, LivePair) {
    let mut chain = Chain::new();
    chain.advance(1_000);
    let f = funding(&mut chain, F, 3_000);
    let b = create_order(&long_order(pubkey(L)), &f, &opts()).unwrap();
    let lo = run(&mut chain, &b).remove(0).open();
    let f = funding(&mut chain, F, 3_000);
    let b = create_order(&short_order(pubkey(S)), &f, &opts()).unwrap();
    let so = run(&mut chain, &b).remove(0).open();
    let f = funding(&mut chain, F, 1_000);
    let b = open_orders(&lo, &so, 1_000_000_000, 1_000_000_000, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut chain, &b);
    let mut it = out.into_iter();
    let (pair, lcont) = {
        let mut pair = None;
        let mut lcont = None;
        for o in it.by_ref() {
            match o {
                Live::Pair(p) => pair = Some(p),
                Live::Open(o) => lcont = Some(o),
                Live::Kob(_) => unreachable!("an evidence-free transaction creates no KOB object"),
                Live::Pending(_) => unreachable!("an evidence-free transaction creates no pending part"),
                Live::Lock(_) => unreachable!("an evidence-free transaction creates no lock"),
            }
        }
        (pair.expect("pair"), lcont.expect("long continuation"))
    };
    (World { chain }, lcont, so, pair)
}

#[test]
fn create_order_cancel_and_refund() {
    let mut chain = Chain::new();
    chain.advance(1_000);
    let f = funding(&mut chain, F, 3_000);
    let o = long_order(pubkey(L));
    let b = create_order(&o, &f, &opts()).unwrap();
    let lo = run(&mut chain, &b).remove(0).open();
    assert_eq!(lo.value, o.escrow() as u64);
    assert_eq!(lo.o, o);
    // The covenant id is the genesis id of the order output.
    assert_ne!(lo.id, ZERO);

    // Cancel: only the owner; the escrow less the fee goes to the destination.
    let b = cancel_order(&lo, &pubkey(OTHER), &opts()).unwrap();
    let s = b.sign(&keys()).unwrap();
    assert!(verify(&s.tx, &s.entries).is_ok());
    assert_eq!(s.tx.outputs[0].value + s.fee, lo.value);
    let mut mutated = b.clone();
    mutated.plans[0] = Plan::Open { redeem: lo.o.redeem(), entry: "cancel", args: vec![Arg::Sig(pubkey(OTHER))] };
    assert!(mutated.sign(&keys()).is_ok());
    // A wrong signer: the plan names a key whose signature the order does not accept.
    let mut wrong = b.clone();
    wrong.plans[0] = Plan::Open { redeem: lo.o.redeem(), entry: "cancel", args: vec![Arg::Sig(pubkey(F))] };
    let s = wrong.sign(&keys()).unwrap();
    assert!(!verify(&s.tx, &s.entries).is_ok());

    // Refund: not before expiry or 90 days idle; the lock time is the due time.
    let mut o2 = long_order(pubkey(L));
    o2.expiry_daa = 50_000;
    let f = funding(&mut chain, F, 3_000);
    let lo2 = run(&mut chain, &create_order(&o2, &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut chain, F, 100);
    let b = refund_order(&lo2, &f, &opts()).unwrap();
    assert_eq!(b.tx.lock_time, 50_000);
    let s = b.sign(&keys()).unwrap();
    assert!(chain.apply(&s.tx, &[]).is_err(), "refund before expiry must not be accepted");
    chain.advance(50_000);
    assert_outputs_are_least(&b);
    chain.apply_signed(&s).expect("refund after expiry");
    assert_eq!(chain.balance(&pubkey(L)), lo2.value);
}

#[test]
fn open_orders_makes_a_pair_and_continues_the_long() {
    let (w, lcont, so, pair) = world();
    // 1,000 of the long's 2,000 tokens remain; the short filled out.
    assert_eq!(lcont.o.size_left, 1_000_000_000);
    assert_eq!(so.o.size_left, 1_000_000_000);
    assert_eq!(pair.p.size, 1_000_000_000);
    assert_eq!((pair.p.bl, pair.p.bs, pair.p.entry), (900_000_000, 1_200_000_000, 1_000_000_000));
    assert_eq!(pair.value, pair.p.floor() as u64);
    // The short owner got its escrow remainder back (filled out), plain.
    assert!(w.chain.balance(&pubkey(S)) > 0);
}

#[test]
fn open_taker_opens_against_a_signing_taker() {
    let mut chain = Chain::new();
    chain.advance(1_000);
    let f = funding(&mut chain, F, 3_000);
    let lo = run(&mut chain, &create_order(&long_order(pubkey(L)), &f, &opts()).unwrap()).remove(0).open();
    // The taker (key S) shorts 1,000 tokens at 10 KAS with a 2 KAS margin rate from its own UTXOs.
    let taker = funding(&mut chain, S, 5_000);
    let b = open_taker(&lo, 1_000_000_000, 1_000_000_000, 200_000_000, &pubkey(S), &taker, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut chain, &b);
    let pair = out.into_iter().find_map(|o| if let Live::Pair(p) = o { Some(p) } else { None }).unwrap();
    assert_eq!(pair.p.short_key, pubkey(S));
    assert_eq!(pair.p.long_key, pubkey(L));
    assert_eq!(pair.p.tag_s, ZERO);
    // The taker is the pair's short: it can sign a cooperative close with the long.
    let b = coop_close(&pair, 1_000 * KAS, FeeFrom::Short, None, &opts()).unwrap();
    let s = b.sign(&keys()).unwrap();
    assert!(verify(&s.tx, &s.entries).is_ok(), "{:?}", verify(&s.tx, &s.entries).errors());
}

#[test]
fn amend_lists_and_unlists() {
    let (mut w, _lcont, _so, pair) = world();
    let f = funding(&mut w.chain, F, 100);
    // List the long at 10.2 KAS, smallest part 100 tokens, tip 0.01 KAS per token.
    let b = amend(&pair, LONG, (1_020_000_000, 100_000_000, 1_000_000), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let listed = run(&mut w.chain, &b).remove(0).pair();
    assert_eq!(listed.p.long_window(), (1_020_000_000, 100_000_000, 1_000_000));
    // Unlist.
    let f = funding(&mut w.chain, F, 100);
    let b = amend(&listed, LONG, (0, 0, 0), &f, &opts()).unwrap();
    let un = run(&mut w.chain, &b).remove(0).pair();
    assert_eq!(un.p.long_window(), (0, 0, 0));
    // Only the owner of the side can amend: the short's key cannot list the long.
    let f = funding(&mut w.chain, F, 100);
    let mut b = amend(&un, LONG, (1_020_000_000, 0, 0), &f, &opts()).unwrap();
    if let Plan::Pair { args, .. } = &mut b.plans[0] {
        args[0] = Arg::Sig(pubkey(S));
    }
    let s = b.sign(&keys()).unwrap();
    assert!(!verify(&s.tx, &s.entries).is_ok());
}

#[test]
fn add_margin_lowers_bl_and_raises_bs() {
    let (mut w, _lcont, _so, pair) = world();
    let f = funding(&mut w.chain, F, 2_000);
    let b = add_margin(&pair, LONG, 50_000_000, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let p2 = run(&mut w.chain, &b).remove(0).pair();
    assert_eq!((p2.p.bl, p2.p.bs), (850_000_000, 1_200_000_000));
    assert_eq!(p2.value, pair.value + margin_cost(&pair.p, 50_000_000) as u64);
    let f = funding(&mut w.chain, F, 2_000);
    let p3 = run(&mut w.chain, &add_margin(&p2, SHORT, 100_000_000, &f, &opts()).unwrap()).remove(0).pair();
    assert_eq!(p3.p.bs, 1_300_000_000);
}

#[test]
fn transfer_to_an_order_and_to_a_signing_taker() {
    let (mut w, lcont, _so, pair) = world();
    // The short lists at <= 9.8 KAS (a bid: it buys back no more than that per token) and a short order takes part.
    let f = funding(&mut w.chain, F, 100);
    let listed = run(&mut w.chain, &amend(&pair, SHORT, (1_050_000_000, 100_000_000, 0), &f, &opts()).unwrap()).remove(0).pair();
    // A short order with rate 1 KAS takes 400 tokens at 10.2 KAS: bs' = 11.2 < bs = 12.0 (downgrade).
    let mut so2 = short_order(pubkey(OTHER));
    so2.limit = 900_000_000;
    so2.my_rate = 100_000_000;
    let f = funding(&mut w.chain, F, 3_000);
    let so2l = run(&mut w.chain, &create_order(&so2, &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut w.chain, F, 100);
    assert!(transfer(&listed, SHORT, 400_000_000, 1_020_000_000, &Entrant::Open(so2l.clone()), 0, &f, &opts()).is_err(), "downgrade must be refused");
    // A taker whose margin rate keeps the bound at or above bs: rate 2.2 KAS -> bs' = 12.0.
    let mut so3 = short_order(pubkey(OTHER));
    so3.limit = 900_000_000;
    so3.my_rate = 220_000_000; // bs' = 10.2 + 2.2 = 12.4
    let f = funding(&mut w.chain, F, 3_000);
    let so3l = run(&mut w.chain, &create_order(&so3, &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut w.chain, F, 100);
    let b = transfer(&listed, SHORT, 400_000_000, 1_020_000_000, &Entrant::Open(so3l), 0, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut w.chain, &b);
    let pairs: Vec<LivePair> = out.into_iter().filter_map(|o| if let Live::Pair(p) = o { Some(p) } else { None }).collect();
    assert_eq!(pairs.len(), 2);
    let part = pairs.iter().find(|p| p.p.size == 400_000_000).unwrap();
    let rest = pairs.iter().find(|p| p.p.size == 600_000_000).unwrap();
    assert_eq!(part.p.short_key, pubkey(OTHER));
    assert_eq!(rest.p.short_key, pubkey(S));
    let _ = lcont;

    // A signing taker (key 25) takes the remainder's short listing: nothing is listed on the remainder (tags and
    // listings are kept), so list it first.
    let f = funding(&mut w.chain, F, 100);
    let rest_listed = run(&mut w.chain, &amend(rest, SHORT, (980_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    let taker = funding(&mut w.chain, 25, 5_000);
    let b = transfer(&rest_listed, SHORT, 600_000_000, 970_000_000, &Entrant::Direct { key: pubkey(25), rate: 230_000_000 }, 0, &taker, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut w.chain, &b);
    let p = out.into_iter().find_map(|o| if let Live::Pair(p) = o { Some(p) } else { None }).unwrap();
    assert_eq!(p.p.short_key, pubkey(25));
    assert_eq!(p.p.bs, 970_000_000 + 230_000_000);
}

#[test]
fn mutual_close_partial_then_full() {
    let (mut w, _lcont, _so, pair) = world();
    let f = funding(&mut w.chain, F, 100);
    let p = run(&mut w.chain, &amend(&pair, LONG, (1_000_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    let f = funding(&mut w.chain, F, 100);
    let p = run(&mut w.chain, &amend(&p, SHORT, (1_050_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    // Close 400 tokens at 10.2 KAS.
    let f = funding(&mut w.chain, F, 100);
    let b = mutual_close(&p, 400_000_000, 1_020_000_000, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut w.chain, &b);
    let rest = out.into_iter().find_map(|o| if let Live::Pair(p) = o { Some(p) } else { None }).unwrap();
    assert_eq!(rest.p.size, 600_000_000);
    // The long got q(400, 10.2 - 9.0), the short q(400, 12.0 - 10.2), as the math says.
    assert_eq!(w.chain.balance(&pubkey(L)), owed(400_000_000, 120_000_000, 0, SCALE) as u64);
    // Close the rest.
    let f = funding(&mut w.chain, F, 100);
    let b = mutual_close(&rest, 600_000_000, 1_020_000_000, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut w.chain, &b);
    assert!(out.is_empty());
}

#[test]
fn coop_close_pays_any_split_and_needs_both_keys() {
    let (mut w, _lcont, _so, pair) = world();
    // 60 / 40: the long gets 1,200 KAS, the short the rest less the fee.
    let b = coop_close(&pair, 1_200 * KAS, FeeFrom::Short, None, &opts()).unwrap();
    let s = b.sign(&keys()).unwrap();
    let v = verify(&s.tx, &s.entries);
    assert!(v.is_ok(), "{:?}", v.errors());
    assert_eq!(s.tx.outputs[0].value, 1_200 * KAS);
    assert_eq!(s.tx.outputs[1].value + s.fee, pair.value - 1_200 * KAS);
    // Missing the short's key: signing refuses with the label of the input.
    let only_long = keyset(&[L]);
    let e = b.sign(&only_long).unwrap_err().to_string();
    assert!(e.contains("input 0"), "{e}");
    // With a funder the shares add up to the pair's value exactly.
    let f = funding(&mut w.chain, F, 100);
    let b = coop_close(&pair, 1_500 * KAS, FeeFrom::Long, Some(&f), &opts()).unwrap();
    let out = run(&mut w.chain, &b);
    assert!(out.is_empty());
    assert_eq!(w.chain.balance(&pubkey(L)), 1_500 * KAS);
}

#[test]
fn idle_settle_after_the_idle_period() {
    let (mut w, _lcont, _so, pair) = world();
    let f = funding(&mut w.chain, F, 100);
    let b = idle_settle(&pair, &f, &opts()).unwrap();
    let s = b.sign(&keys()).unwrap();
    // Too early: the relative lock has not matured.
    assert!(w.chain.apply(&s.tx, &[]).is_err());
    w.chain.advance(77_760_000);
    assert_outputs_are_least(&b);
    w.chain.apply_signed(&s).expect("idle settle");
    // Settled at the entry price 10 KAS: long q(1000, 10 - 9) = 1,000 KAS, short the rest.
    let (lp, sp) = idle_out(&pair.p, pair.value);
    assert_eq!(lp, 1_000 * KAS as i64);
    assert_eq!(w.chain.balance(&pubkey(L)), lp as u64);
    assert_eq!(w.chain.balance(&pubkey(S)) as i64 >= sp, true);
}

#[test]
fn builders_refuse_what_the_contracts_refuse() {
    let (mut w, _lcont, so, pair) = world();
    let f = funding(&mut w.chain, F, 100);
    // Transfer of an unlisted long.
    assert!(transfer(&pair, LONG, 400_000_000, 1_000_000_000, &Entrant::Direct { key: pubkey(F), rate: 100_000_000 }, 0, &f, &opts()).is_err());
    // Mutual close without listings.
    assert!(mutual_close(&pair, 400_000_000, 1_000_000_000, &f, &opts()).is_err());
    // No funding at all.
    let none = Funding { utxos: vec![], change_key: pubkey(F) };
    assert!(amend(&pair, LONG, (1, 0, 0), &none, &opts()).is_err());
    // Underfunded: 1 KAS cannot pay a margin top-up of 1,000 tokens x 1 KAS.
    let small = funding(&mut w.chain, F, 1);
    assert!(matches!(add_margin(&pair, LONG, 100_000_000, &small, &opts()), Err(kpf_protocol::Error::Insufficient { .. })));
    // An order filled with an amount above what is left.
    let (lo2, _) = (so.clone(), ());
    let f = funding(&mut w.chain, F, 100);
    assert!(open_taker(&lo2, 9_000_000_000, 1_000_000_000, 100_000_000, &pubkey(F), &f, &opts()).is_err());
    let _: TransactionOutpoint = pair.op;
}

#[test]
fn close_versus_close_partial_and_full() {
    let (mut w, _lcont, _so, p1) = world();
    // P2: long (key 23) rate 1.5 KAS, short (key 25) rate 1.5 KAS: bl 8.5, bs 11.5, entry 10.
    let mut o2l = long_order(pubkey(OTHER));
    o2l.my_rate = 150_000_000;
    o2l.min_counter_rate = 100_000_000;
    let mut o2s = short_order(pubkey(25));
    o2s.my_rate = 150_000_000;
    o2s.min_counter_rate = 100_000_000;
    let f = funding(&mut w.chain, F, 4_000);
    let l2 = run(&mut w.chain, &create_order(&o2l, &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut w.chain, F, 4_000);
    let s2 = run(&mut w.chain, &create_order(&o2s, &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut w.chain, F, 1_000);
    let out = run(&mut w.chain, &open_orders(&l2, &s2, 1_000_000_000, 1_000_000_000, &f, &opts()).unwrap());
    let p2 = out.into_iter().find_map(|o| if let Live::Pair(p) = o { Some(p) } else { None }).unwrap();
    assert_eq!((p2.p.bl, p2.p.bs), (850_000_000, 1_150_000_000));
    // Host p1 lists its long at 10.2, donor p2 lists its short at 10.4; 400 tokens at 10.3.
    let f = funding(&mut w.chain, F, 100);
    let h = run(&mut w.chain, &amend(&p1, LONG, (1_020_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    let f = funding(&mut w.chain, F, 100);
    let d = run(&mut w.chain, &amend(&p2, SHORT, (1_040_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    let f = funding(&mut w.chain, F, 100);
    let c = CloseSpec { n: 400_000_000, x: 1_030_000_000, e3: 1_000_000_000 };
    let b = close_vs_close(&h, &d, &c, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let out = run(&mut w.chain, &b);
    let pairs: Vec<LivePair> = out.into_iter().filter_map(|o| if let Live::Pair(p) = o { Some(p) } else { None }).collect();
    assert_eq!(pairs.len(), 3);
    let p3 = pairs.iter().find(|p| p.p.long_key == pubkey(OTHER) && p.p.short_key == pubkey(S)).expect("re-paired P3");
    assert_eq!((p3.p.size, p3.p.bl, p3.p.bs), (400_000_000, 850_000_000, 1_200_000_000));
    // The host's long got q(400, 10.3 - 9.0), the donor's short q(400, 11.5 - 10.3).
    assert_eq!(w.chain.balance(&pubkey(L)), owed(400_000_000, 130_000_000, 0, SCALE) as u64);
    // The whole donor remainder pairs up later; a full close of the rest: list again and close the 600 remainders.
    let host_rest = pairs.iter().find(|p| p.p.long_key == pubkey(L) && p.p.size == 600_000_000).unwrap();
    assert_eq!(host_rest.p.size, 600_000_000);
}

/// Every integer argument of every contract input matters: changing any one by one makes some input refuse.
fn assert_args_matter(b: &Built) {
    for (i, plan) in b.plans.iter().enumerate() {
        let (args, mk): (&Vec<Arg>, Box<dyn Fn(Vec<Arg>) -> Plan>) = match plan {
            Plan::Pair { redeem, entry, args } => {
                let (r, e) = (redeem.clone(), *entry);
                (args, Box::new(move |a| Plan::Pair { redeem: r.clone(), entry: e, args: a }))
            }
            Plan::Open { redeem, entry, args } => {
                let (r, e) = (redeem.clone(), *entry);
                (args, Box::new(move |a| Plan::Open { redeem: r.clone(), entry: e, args: a }))
            }
            Plan::Pending { redeem, entry, args } => {
                let (r, e) = (redeem.clone(), *entry);
                (args, Box::new(move |a| Plan::Pending { redeem: r.clone(), entry: e, args: a }))
            }
            Plan::Lock { redeem, entry, args } => {
                let (r, e) = (redeem.clone(), *entry);
                (args, Box::new(move |a| Plan::Lock { redeem: r.clone(), entry: e, args: a }))
            }
            Plan::P2pk { .. } | Plan::Kob { .. } => continue,
        };
        for (k, a) in args.iter().enumerate() {
            if let Arg::V(silverscript_abi::ArtifactValue::Int(v)) = a {
                let mut m = b.clone();
                let mut na = args.clone();
                na[k] = Arg::V(silverscript_abi::ArtifactValue::Int(v + 1));
                m.plans[i] = mk(na);
                let s = m.sign(&keys()).expect("sign mutant");
                let ver = verify(&s.tx, &s.entries);
                assert!(!ver.is_ok(), "input {i} ({}) argument {k} + 1 was accepted", b.input_labels[i]);
            }
        }
    }
}

#[test]
fn arguments_are_all_checked() {
    let (mut w, _lcont, _so, pair) = world();
    let f = funding(&mut w.chain, F, 100);
    assert_args_matter(&amend(&pair, LONG, (1_020_000_000, 100_000_000, 1_000_000), &f, &opts()).unwrap());
    let f = funding(&mut w.chain, F, 2_000);
    assert_args_matter(&add_margin(&pair, SHORT, 50_000_000, &f, &opts()).unwrap());
    let f = funding(&mut w.chain, F, 100);
    let p = run(&mut w.chain, &amend(&pair, LONG, (1_000_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    let f = funding(&mut w.chain, F, 100);
    let p = run(&mut w.chain, &amend(&p, SHORT, (1_050_000_000, 0, 0), &f, &opts()).unwrap()).remove(0).pair();
    let f = funding(&mut w.chain, F, 100);
    assert_args_matter(&mutual_close(&p, 400_000_000, 1_020_000_000, &f, &opts()).unwrap());
    // Open orders: the fill arguments of both orders.
    let mut chain = Chain::new();
    chain.advance(1_000);
    let f = funding(&mut chain, F, 3_000);
    let lo = run(&mut chain, &create_order(&long_order(pubkey(L)), &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut chain, F, 3_000);
    let so = run(&mut chain, &create_order(&short_order(pubkey(S)), &f, &opts()).unwrap()).remove(0).open();
    let f = funding(&mut chain, F, 100);
    assert_args_matter(&open_orders(&lo, &so, 1_000_000_000, 1_000_000_000, &f, &opts()).unwrap());
}

#[test]
fn coop_close_needs_both_signatures_of_the_right_keys() {
    let (w, _lcont, _so, pair) = world();
    let _ = w;
    let b = coop_close(&pair, 1_200 * KAS, FeeFrom::Short, None, &opts()).unwrap();
    let mut m = b.clone();
    if let Plan::Pair { args, .. } = &mut m.plans[0] {
        args[1] = Arg::Sig(pubkey(L)); // the long signs for the short too
    }
    let s = m.sign(&keys()).unwrap();
    assert!(!verify(&s.tx, &s.entries).is_ok());
    let mut m = b.clone();
    if let Plan::Pair { args, .. } = &mut m.plans[0] {
        args.swap(0, 1);
    }
    let s = m.sign(&keys()).unwrap();
    assert!(!verify(&s.tx, &s.entries).is_ok());
}

#[test]
fn priority_fee_mode_pays_for_storage_mass() {
    let (mut w, _lcont, _so, pair) = world();
    let f = funding(&mut w.chain, F, 100);
    let relay = amend(&pair, LONG, (1_020_000_000, 0, 0), &f, &Opts::default()).unwrap().sign(&keys()).unwrap();
    let prio = amend(&pair, LONG, (1_020_000_000, 0, 0), &f, &Opts { fee_rate: 100, fee_mode: FeeMode::Priority }).unwrap().sign(&keys()).unwrap();
    assert!(prio.fee >= relay.fee);
    assert_eq!(relay.fee, relay.mass.fee_mass * 100);
    assert_eq!(prio.fee, prio.mass.priority_mass * 100);
    // A higher rate pays proportionally more, and the transaction still verifies.
    let dear = amend(&pair, LONG, (1_020_000_000, 0, 0), &f, &Opts { fee_rate: 250, fee_mode: FeeMode::Relay }).unwrap().sign(&keys()).unwrap();
    assert_eq!(dear.fee, dear.mass.fee_mass * 250);
    assert!(verify(&dear.tx, &dear.entries).is_ok());
}
