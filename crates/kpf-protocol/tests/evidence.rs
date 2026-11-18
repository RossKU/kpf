//! The evidence builders (P5, P6, P7) on the in-memory chain: every input, the pairs, KOB's orders and the token program,
//! passes the script engine; mutations are refused. Keys are the TEST-ONLY keys of `kpf_protocol::fixtures`.

use kaspa_consensus_core::Hash;
use kpf_protocol::build::{Funding, Opts};
use kpf_protocol::evidence::*;
use kpf_protocol::fixtures::*;
use kpf_protocol::kob::*;
use kpf_protocol::sim::{Chain, Live};
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use kpf_protocol::verify::verify;

const KAS: u64 = 100_000_000;
const L: u8 = 21;
const S: u8 = 22;
const F: u8 = 24;
const MAKER: u8 = 41;
const BUYER: u8 = 42;
const N: i64 = 1_000_000_000; // the whole pair: 1,000 tokens
const LIQ_ASK: i64 = 940_000_000;
const LIQ_BID: i64 = 1_150_000_000;

fn keys() -> KeySet {
    keyset(&[L, S, F, MAKER, BUYER, 21, 22, 23, 25, 26])
}

fn opts() -> Opts {
    Opts::default()
}

struct W {
    chain: Chain,
    m: Market,
    seq: u8,
}

impl W {
    fn new() -> W {
        let mut chain = Chain::new();
        chain.advance(1_000);
        W { chain, m: kob_market(), seq: 0 }
    }

    /// The executor's funding: one big UTXO.
    fn funding(&mut self) -> Funding {
        let k = pubkey(F);
        Funding { utxos: vec![self.chain.mint(&k, 200_000_000 * KAS)], change_key: k }
    }

    fn pair(&mut self, tag: u8) -> LivePair {
        let mut p = pair(pubkey(L), pubkey(S));
        p.m = self.m.clone();
        self.seq += 1;
        let daa = self.chain.daa;
        self.chain.mint_obj(Obj::Pair(p.clone()), p.floor() as u64, Hash::from_bytes([tag; 32]), daa).pair()
    }

    fn ask(&mut self, price: i64, amount: i64) -> Evidence {
        let id = Hash::from_bytes([0xe1; 32]);
        let a = ask_of(&self.m, pubkey(MAKER), price, amount);
        let daa = self.chain.daa;
        let order = self.chain.mint_kob(KobObj::Ask(a), 10 * KAS, id, daa).ask();
        let custody = self.chain.mint_kob(KobObj::Token(TokenState::covid(amount, id, self.m.ext)), 20_000_000, self.m.token_cov_id, daa).token();
        Evidence::Ask { order, custody }
    }

    fn bid(&mut self, price: i64, amount: i64) -> Evidence {
        let id = Hash::from_bytes([0xe2; 32]);
        let b = bid_of(&self.m, pubkey(MAKER), price);
        let value = bid_escrow(&b, amount);
        let daa = self.chain.daa;
        let order = self.chain.mint_kob(KobObj::Bid(b), value, id, daa).bid();
        Evidence::Bid { order }
    }

    /// The executor's own tokens (what it sells into a bid).
    fn tokens(&mut self, amount: i64) -> LiveToken {
        let daa = self.chain.daa;
        self.chain.mint_kob(KobObj::Token(TokenState::p2pk(amount, pubkey(F), self.m.ext)), 20_000_000, self.m.token_cov_id, daa).token()
    }

    /// Lets the evidence rest as long as the market requires.
    fn rest(&mut self) {
        self.chain.advance(self.m.min_rest_daa as u64 + 50);
    }

    fn run(&mut self, b: &Built) -> (Signed, Vec<Live>) {
        let s = b.sign(&keys()).expect("sign");
        let v = verify(&s.tx, &s.entries);
        assert!(v.is_ok(), "verify failed: {:?}", v.errors());
        let live = self.chain.apply_signed(&s).expect("apply");
        (s, live)
    }
}

fn eu(order: Evidence, seller: Option<LiveToken>) -> EvidenceUse {
    EvidenceUse { order, fill: None, seller, now: None, allow_dead: false }
}


fn out_of<'a>(s: &'a Signed, label: &str) -> (usize, &'a kaspa_consensus_core::tx::TransactionOutput) {
    let i = s.output_labels.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("no output labelled {label}: {:?}", s.output_labels));
    (i, &s.tx.outputs[i])
}

fn pairs_of(live: &[Live]) -> Vec<LivePair> {
    live.iter().filter_map(|l| if let Live::Pair(p) = l { Some(p.clone()) } else { None }).collect()
}

fn q(n: i64, r: i64, scale: i64, c: i64) -> i64 {
    kpf_protocol::payoff::q(n, r, scale, c)
}

/// Labels of outputs a covenant bounds from below (a one-sompi cut must be refused by some input).
fn constrained(label: &str) -> bool {
    ["long paid", "short paid", "winner paid", "evidence ask: maker paid", "evidence bid: tokens delivered", "evidence bid: rest of the escrow"]
        .iter()
        .any(|p| label.starts_with(p))
        || label.contains("remainder")
        || label.contains("part (entrant)")
        || label.starts_with("entrant order")
        || label.starts_with("evidence ask continuation")
        || label.starts_with("evidence ask custody rest")
        || label.starts_with("evidence bid continuation")
}

/// Every party output one sompi lower is refused by an input (the change absorbs the sompi, so the transaction stays
/// balanced), and the bid's continuation, pinned exactly, is refused one sompi higher.
fn assert_outputs_are_least(b: &Built) {
    for i in 0..b.tx.outputs.len() {
        let l = &b.output_labels[i];
        // An output the builder raised to a floor (or to a token carrier) is above what its covenant needs.
        if !constrained(l) || b.tx.outputs[i].value <= TOKEN_OUT_VALUE {
            continue;
        }
        let mut m = b.clone();
        m.tx.outputs[i].value -= 1;
        let s = m.sign(&keys()).expect("sign mutant");
        let v = verify(&s.tx, &s.entries);
        assert!(!v.is_ok(), "output {i} ({l}) one sompi lower was accepted");
        assert!(v.inputs.iter().any(|r| r.is_err()), "output {i} ({l}): refused only at transaction level: {:?}", v.errors());
    }
    if let Some(i) = b.output_labels.iter().position(|l| l.starts_with("evidence bid continuation")) {
        let mut m = b.clone();
        m.tx.outputs[i].value += 1;
        let s = m.sign(&keys()).expect("sign mutant");
        assert!(!verify(&s.tx, &s.entries).is_ok(), "the bid's continuation is pinned exactly");
    }
}

/// A mutation of the built transaction (before signing) that some input must refuse; `who` is an input that must fail.
fn refused(b: &Built, who: usize, f: impl Fn(&mut Built)) {
    let mut m = b.clone();
    f(&mut m);
    let s = m.sign(&keys()).expect("sign mutant");
    let v = verify(&s.tx, &s.entries);
    assert!(!v.is_ok(), "the mutant was accepted");
    assert!(v.inputs[who].is_err(), "input {who} accepted the mutant (errors: {:?})", v.errors());
}

fn set_arg(b: &mut Built, input: usize, arg: usize, a: Arg) {
    match &mut b.plans[input] {
        Plan::Pair { args, .. } | Plan::Kob { args, .. } | Plan::Open { args, .. } => args[arg] = a,
        _ => panic!("not a contract input"),
    }
}

// ---------------------------------------------------------------- P5

#[test]
fn p5_long_closes_at_a_bid_whole_and_in_part() {
    for n in [N, 400_000_000] {
        let mut w = W::new();
        let l = w.pair(0x31);
        let bid_at = 1_000_000_000; // 10 KAS
        let e = w.bid(bid_at, 1_500_000_000);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        let b = close_on_evidence(&l, LONG, n, &eu(e, Some(seller)), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        // The long is paid q(n, 10 - 9) = n tokens x 1 KAS.
        let (_, lp) = out_of(&s, "long paid");
        let want = q(n, bid_at - 900_000_000, SCALE, 0) as u64;
        assert!(lp.value >= want, "long payout {} vs {want}", lp.value);
        let rest = pairs_of(&live);
        if n == N {
            assert!(rest.is_empty());
        } else {
            assert_eq!(rest.len(), 1);
            assert_eq!(rest[0].p.size, N - n);
        }
        assert!(s.output_labels.iter().any(|l| l.starts_with("evidence bid continuation")));
        println!("p5 long on a bid n={n}: size {} fee {} mass {:?} units {:?}", s.mass.size, s.fee, s.mass, s.units);
    }
}

#[test]
fn p5_short_closes_at_an_ask_partial_and_filled_out() {
    for (ask_amount, n) in [(1_500_000_000i64, N), (N, N), (1_500_000_000, 400_000_000)] {
        let mut w = W::new();
        let l = w.pair(0x31);
        let ask_at = 1_050_000_000; // 10.5 KAS
        let e = w.ask(ask_at, ask_amount);
        w.rest();
        let f = w.funding();
        let b = close_on_evidence(&l, SHORT, n, &eu(e, None), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        let (_, sp) = out_of(&s, "short paid");
        let want = q(n, 1_200_000_000 - ask_at, SCALE, 0) as u64;
        assert!(sp.value >= want, "short payout {} vs {want}", sp.value);
        let filled_out = ask_amount == n;
        assert_eq!(s.output_labels.iter().any(|l| l.starts_with("evidence ask continuation")), !filled_out);
        // The executor bought n tokens (the least the block needs): its token holder is among the created objects.
        let toks: Vec<_> = live.iter().filter_map(|x| if let Live::Kob(k) = x { Some(k.clone()) } else { None }).collect();
        assert!(toks.iter().any(|k| matches!(&k.obj, KobObj::Token(t) if t.amount == n && t.owner == pubkey(F) && t.scheme == SCHEME_P2PK)));
        println!("p5 short on an ask (ask {ask_amount}, n {n}): size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
    }
}

// ---------------------------------------------------------------- P6

#[test]
fn p6_liquidates_a_long_at_an_ask_and_a_short_at_a_bid() {
    // A long at an ask of 9.4 KAS (trigger 9.47).
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    w.rest();
    let f = w.funding();
    let b = liquidate(&[(l.clone(), N)], &eu(e, None), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (s, _) = w.run(&b);
    // The long gets nothing (a filler holds its slot); the short gets the whole value less the fee.
    let (i, o) = out_of(&s, "filler (pair slot)");
    assert_eq!(i, 0);
    assert_eq!(o.value, FILLER_VALUE);
    let (_, sp) = out_of(&s, "short paid");
    let fee = kpf_protocol::payoff::liq_fee(&l.p, N, LIQ_ASK, LONG) as u64;
    assert!(sp.value >= l.value - fee, "the winner gets the value less the fee");
    println!("p6 long: size {} fee {} mass {:?} units {:?}", s.mass.size, s.fee, s.mass, s.units);

    // A short at a bid of 11.5 KAS (trigger 11.43).
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.bid(LIQ_BID, 1_500_000_000);
    let seller = w.tokens(1_500_000_000);
    w.rest();
    let f = w.funding();
    let b = liquidate(&[(l.clone(), N)], &eu(e, Some(seller)), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (s, _) = w.run(&b);
    let (_, lp) = out_of(&s, "long paid");
    let fee = kpf_protocol::payoff::liq_fee(&l.p, N, LIQ_BID, SHORT) as u64;
    assert!(lp.value >= l.value - fee);
    assert!(!s.output_labels.iter().any(|l| l.starts_with("short paid")), "the short is the loser and gets 0: no output for it");
    println!("p6 short: size {} fee {} mass {:?} units {:?}", s.mass.size, s.fee, s.mass, s.units);
}

#[test]
fn p6_part_of_a_pair_keeps_a_remainder() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    w.rest();
    let f = w.funding();
    let b = liquidate(&[(l.clone(), 300_000_000)], &eu(e, None), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (_, live) = w.run(&b);
    let rest = pairs_of(&live);
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].p.size, 700_000_000);
    assert_eq!((rest[0].p.bl, rest[0].p.bs), (l.p.bl, l.p.bs));
}

#[test]
fn p6_blocks_of_pairs_on_one_evidence() {
    for k in [2usize, 4, 8] {
        let mut w = W::new();
        let pairs: Vec<LivePair> = (0..k).map(|i| w.pair(0x40 + i as u8)).collect();
        // k_bps = 10^4: the block needs a fill of k x 1,000 tokens.
        let e = w.ask(LIQ_ASK, (k as i64 + 1) * N);
        w.rest();
        let f = w.funding();
        let parts: Vec<(LivePair, i64)> = pairs.iter().map(|p| (p.clone(), N)).collect();
        let b = liquidate(&parts, &eu(e.clone(), None), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        println!("p6 block of {k}: size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
        assert!(pairs_of(&live).is_empty());
    }
    // Nine pairs are refused by the builder (the block scan covers 8).
    let mut w = W::new();
    let pairs: Vec<LivePair> = (0..9).map(|i| w.pair(0x40 + i)).collect();
    let e = w.ask(LIQ_ASK, 90_000_000_000);
    w.rest();
    let f = w.funding();
    let parts: Vec<(LivePair, i64)> = pairs.iter().map(|p| (p.clone(), N)).collect();
    let err = liquidate(&parts, &eu(e, None), &f, &opts()).unwrap_err().to_string();
    assert!(err.contains("at most 8"), "{err}");
}

// ---------------------------------------------------------------- P7

fn entrant(w: &mut W, side: i64, tag: u8) -> LiveOpen {
    let o = if side == LONG { long_order(pubkey(23)) } else { short_order(pubkey(25)) }.with_market(w.m.clone());
    let daa = w.chain.daa;
    w.chain.mint_obj(Obj::Open(o.clone()), o.escrow() as u64, Hash::from_bytes([tag; 32]), daa).open()
}

#[test]
fn p7_liquidates_by_transfer_whole_and_in_part() {
    for (n, k) in [(N, 0), (400_000_000, 0), (400_000_000, 1)] {
        let mut w = W::new();
        let l = w.pair(0x31);
        let o = entrant(&mut w, LONG, 0x71);
        let e = w.ask(LIQ_ASK, 1_500_000_000);
        w.rest();
        let f = w.funding();
        // The loser (the long) is sold at 9.5 KAS (>= the ask 9.4); the entrant's own bound is 9.5 - 1 = 8.5 <= bl 9.
        let b = liquidate_transfer(&l, n, 950_000_000, &o, k, &eu(e, None), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        let ps = pairs_of(&live);
        let part = ps.iter().find(|p| p.p.size == n).expect("the part");
        assert_eq!(part.p.long_key, pubkey(23), "the entrant is the new long");
        assert_eq!(part.p.short_key, pubkey(S), "the winner keeps its position");
        assert_eq!(part.p.bl, 950_000_000 - o.o.my_rate);
        assert_eq!(ps.len(), if n == N { 1 } else { 2 });
        println!("p7 n={n} k={k}: size {} fee {} mass {:?} units {:?}", s.mass.size, s.fee, s.mass, s.units);
    }
}

#[test]
fn p7_short_loser_sold_at_a_bid() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let o = entrant(&mut w, SHORT, 0x72);
    let e = w.bid(LIQ_BID, 1_500_000_000);
    let seller = w.tokens(1_500_000_000);
    w.rest();
    let f = w.funding();
    // x <= the bid 11.5; the entrant's bound 11.4 + 2 = 13.4 >= bs 12 (no downgrade).
    let b = liquidate_transfer(&l, N, 1_140_000_000, &o, 0, &eu(e, Some(seller)), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (_, live) = w.run(&b);
    let ps = pairs_of(&live);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].p.short_key, pubkey(25));
    assert_eq!(ps[0].p.long_key, pubkey(L));
}

// ---------------------------------------------------------------- refusals

#[test]
fn builders_refuse_what_the_contracts_refuse() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let ask = w.ask(LIQ_ASK, 1_500_000_000);
    let bid = w.bid(LIQ_BID, 1_500_000_000);
    let seller = w.tokens(1_500_000_000);
    w.rest();
    let f = w.funding();
    let err = |r: kpf_protocol::Result<Built>| r.err().expect("refused").to_string();
    // A long closes at a bid, not an ask; a short at an ask.
    assert!(err(close_on_evidence(&l, LONG, N, &eu(ask.clone(), None), &f, &opts())).contains("closes at a resting bid"));
    assert!(err(close_on_evidence(&l, SHORT, N, &eu(bid.clone(), Some(seller.clone())), &f, &opts())).contains("closes at a resting ask"));
    // A short is liquidated by a bid; the base pair's short is not liquidatable at 10 KAS.
    assert!(err(liquidate(&[(l.clone(), N)], &eu(w.bid(1_000_000_000, 1_500_000_000), Some(seller.clone())), &f, &opts())).contains("not liquidatable"));
    // A long is not liquidatable at an ask above its trigger.
    assert!(err(liquidate(&[(l.clone(), N)], &eu(w.ask(1_000_000_000, 1_500_000_000), None), &f, &opts())).contains("not liquidatable"));
    // The evidence must cover k x n: an ask of 500 tokens cannot serve a 1,000-token part.
    assert!(err(liquidate(&[(l.clone(), N)], &eu(w.ask(LIQ_ASK, 500_000_000), None), &f, &opts())).contains("fewer than"));
    // A bid needs tokens to sell into it (a pair whose short is liquidatable at 11.5: bs 10).
    let l2 = {
        let mut p = pair(pubkey(L), pubkey(S));
        p.m = w.m.clone();
        p.bl = 800_000_000;
        p.bs = 1_000_000_000;
        let daa = w.chain.daa;
        w.chain.mint_obj(Obj::Pair(p.clone()), p.floor() as u64, Hash::from_bytes([0x33; 32]), daa).pair()
    };
    assert!(err(liquidate(&[(l2.clone(), N)], &eu(bid.clone(), None), &f, &opts())).contains("tokens to sell"));
    // The seller must hold enough.
    let small = w.tokens(10_000_000);
    assert!(err(liquidate(&[(l2.clone(), N)], &eu(bid.clone(), Some(small)), &f, &opts())).contains("holds"));
    // An entrant of the wrong side, a price on the wrong side of the quote, a downgrade.
    let short_entrant = entrant(&mut w, SHORT, 0x72);
    assert!(err(liquidate_transfer(&l, N, 950_000_000, &short_entrant, 0, &eu(ask.clone(), None), &f, &opts())).contains("long order"));
    let long_entrant = entrant(&mut w, LONG, 0x71);
    assert!(err(liquidate_transfer(&l, N, 930_000_000, &long_entrant, 0, &eu(ask.clone(), None), &f, &opts())).contains("x >= the evidence ask"));
    assert!(err(liquidate_transfer(&l, N, 1_050_000_000, &long_entrant, 0, &eu(ask.clone(), None), &f, &opts())).contains("no-downgrade"));
    // Empty, repeated and invalid parts.
    assert!(err(liquidate(&[], &eu(ask.clone(), None), &f, &opts())).contains("no pair"));
    assert!(err(liquidate(&[(l.clone(), N), (l.clone(), N)], &eu(w.ask(LIQ_ASK, 5_000_000_000), None), &f, &opts())).contains("covenant id of an earlier pair"));
    assert!(err(liquidate(&[(l.clone(), 1)], &eu(ask.clone(), None), &f, &opts())).contains("not a valid part"));
    // The evidence of another market (the order prices another token).
    let mut other = w.m.clone();
    other.token_cov_id = Hash::from_bytes([0x99; 32]);
    let mut l3 = l.clone();
    l3.p.m = other;
    assert!(err(liquidate(&[(l3, N)], &eu(ask, None), &f, &opts())).contains("another token"));
}

#[test]
fn mutations_are_refused_by_the_pair() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.bid(1_000_000_000, 1_500_000_000);
    let seller = w.tokens(1_500_000_000);
    w.rest();
    let f = w.funding();
    let b = close_on_evidence(&l, LONG, N, &eu(e, Some(seller)), &f, &opts()).unwrap();
    // The unmutated transaction is accepted.
    let s = b.sign(&keys()).unwrap();
    assert!(verify(&s.tx, &s.entries).is_ok());
    // The block's evidence fill one base unit below k x n.
    let lock = b.tx.lock_time;
    refused(&b, 0, |m| set_arg(m, 1, 0, bv(&sm8(N - 1))));
    // A wrong q, path or side; a non-owner's signature.
    refused(&b, 0, |m| set_arg(m, 0, 1, bv(&sm8(1))));
    refused(&b, 0, |m| set_arg(m, 0, 2, iv(6)));
    refused(&b, 0, |m| set_arg(m, 0, 4, iv(SHORT)));
    refused(&b, 0, |m| set_arg(m, 0, 3, Arg::Sig(pubkey(S))));
    // The lock time one DAA short of the rest window.
    refused(&b, 0, |m| m.tx.lock_time = lock - 1);
    // The long's payout one sompi short.
    refused(&b, 0, |m| m.tx.outputs[0].value -= 1);
    // The pair names an input that is not the evidence.
    refused(&b, 0, |m| set_arg(m, 0, 5, iv(0)));
}

// ---------------------------------------------------------------- KOB shapes

#[test]
fn a_terminal_bid_returns_the_rest_of_its_escrow() {
    // A bid funded for 1,000 tokens plus 3 KAS: the fill leaves less than one more minimum fill, so it ends and the
    // rest of the escrow (>= 1 KAS) goes back to its maker at the reverse slot of the bid's input index.
    let mut w = W::new();
    let l = w.pair(0x31);
    let id = Hash::from_bytes([0xe2; 32]);
    let b = bid_of(&w.m, pubkey(MAKER), 1_000_000_000);
    let value = (b.used(N) + b.delivery_carrier + 300_000_000) as u64;
    let daa = w.chain.daa;
    let order = w.chain.mint_kob(KobObj::Bid(b), value, id, daa).bid();
    let seller = w.tokens(N);
    w.rest();
    let f = w.funding();
    let built = close_on_evidence(&l, LONG, N, &eu(Evidence::Bid { order }, Some(seller)), &f, &opts()).unwrap();
    assert!(built.output_labels.iter().any(|l| l.starts_with("evidence bid: rest of the escrow")));
    assert!(!built.output_labels.iter().any(|l| l.starts_with("evidence bid continuation")));
    assert_outputs_are_least(&built);
    let (s, _) = w.run(&built);
    // The refund sits at M - 1 - 1 (the bid is input 1).
    let i = s.output_labels.iter().position(|l| l.starts_with("evidence bid: rest of the escrow")).unwrap();
    assert_eq!(i, s.tx.outputs.len() - 2);
}

#[test]
fn an_ioc_ask_is_filled_out() {
    let mut w = W::new();
    let mut m2 = w.m.clone();
    m2.min_rest_daa = 100; // the IOC kill is 600 DAA after the order became fillable
    w.m = m2.clone();
    let l = w.pair(0x35);
    let id = Hash::from_bytes([0xe1; 32]);
    let mut a = ask_of(&w.m, pubkey(MAKER), 1_050_000_000, 1_200_000_000);
    a.tif = 1;
    let daa = w.chain.daa;
    let order = w.chain.mint_kob(KobObj::Ask(a), 10 * KAS, id, daa).ask();
    let custody = w.chain.mint_kob(KobObj::Token(TokenState::covid(1_200_000_000, id, w.m.ext)), 20_000_000, w.m.token_cov_id, daa).token();
    w.chain.advance(150);
    let f = w.funding();
    let built = close_on_evidence(&l, SHORT, N, &eu(Evidence::Ask { order, custody }, None), &f, &opts()).unwrap();
    // The whole 1,200 tokens are taken (an IOC order ends with its fill).
    assert!(!built.output_labels.iter().any(|l| l.starts_with("evidence ask continuation")));
    let (_, live) = w.run(&built);
    assert!(live.iter().any(|x| matches!(x, Live::Kob(k) if matches!(&k.obj, KobObj::Token(t) if t.amount == 1_200_000_000))));
}

#[test]
fn a_twap_evidence_order_needs_its_interval_as_the_relative_lock() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let id = Hash::from_bytes([0xe2; 32]);
    let mut b = bid_of(&w.m, pubkey(MAKER), 1_000_000_000);
    b.interval = 200;
    let value = bid_escrow(&b, 1_500_000_000);
    let daa = w.chain.daa;
    let order = w.chain.mint_kob(KobObj::Bid(b), value, id, daa).bid();
    let seller = w.tokens(1_500_000_000);
    w.rest();
    w.chain.advance(300);
    let f = w.funding();
    let built = close_on_evidence(&l, LONG, N, &eu(Evidence::Bid { order }, Some(seller)), &f, &opts()).unwrap();
    assert_eq!(built.tx.inputs[1].sequence, 200, "KobBid.fill requires its UTXO to be `interval` DAA old (CSV)");
    w.run(&built);
}

// ---------------------------------------------------------------- the decoders and the check

#[test]
fn state_codecs_round_trip_and_foreign_scripts_are_explained() {
    let m = kob_market();
    let a = ask_of(&m, pubkey(MAKER), 123_456_789, 456);
    assert_eq!(a.state_bytes().len(), 276);
    assert_eq!(AskState::decode_redeem(&a.redeem()).unwrap(), a);
    let b = bid_of(&m, pubkey(MAKER), 789);
    assert_eq!(b.state_bytes().len(), 285);
    assert_eq!(BidState::decode_redeem(&b.redeem()).unwrap(), b);
    let mut t = TokenState::covid(55, Hash::from_bytes([7; 32]), m.ext);
    t.guard = [3; 32];
    assert_eq!(t.state_bytes().len(), 112);
    assert_eq!(TokenState::decode_redeem(&t.redeem()).unwrap(), t);
    assert_eq!(KobObj::decode(&a.redeem()).unwrap(), KobObj::Ask(a.clone()));
    assert_eq!(KobObj::decode(&t.redeem()).unwrap(), KobObj::Token(t.clone()));
    // A KPF script and a look-alike (one suffix byte changed) are not KOB orders, and the message says what differs.
    let p = pair(pubkey(L), pubkey(S));
    let e = decode_order(&p.redeem()).unwrap_err().to_string();
    assert!(e.contains("not an instance of the KobAsk"), "{e}");
    let mut fake = a.redeem();
    let last = fake.len() - 5;
    fake[last] ^= 1;
    let e = decode_order(&fake).unwrap_err().to_string();
    assert!(e.contains("template hash is") && e.contains("pinned"), "{e}");
    // A foreign token program is refused with the reason.
    let mut other = m.clone();
    other.token_tpl_hash = Hash::from_bytes([9; 32]);
    assert!(supports_token_program(&other).unwrap_err().contains("not KOB's reference KCC-20"));
    assert!(supports_token_program(&m).is_ok());
    // Spent-state truncation is an error, not a panic.
    assert!(AskState::decode_state(&a.state_bytes()[..100]).is_err());
}

fn ask_with(w: &mut W, f: impl Fn(&mut AskState, &mut TokenState)) -> Evidence {
    let id = Hash::from_bytes([0xe1; 32]);
    let mut a = ask_of(&w.m, pubkey(MAKER), LIQ_ASK, 1_500_000_000);
    let mut t = TokenState::covid(1_500_000_000, id, w.m.ext);
    f(&mut a, &mut t);
    let daa = w.chain.daa;
    let order = w.chain.mint_kob(KobObj::Ask(a), 10 * KAS, id, daa).ask();
    let custody = w.chain.mint_kob(KobObj::Token(t), 20_000_000, w.m.token_cov_id, daa).token();
    Evidence::Ask { order, custody }
}

fn bid_with(w: &mut W, f: impl Fn(&mut BidState)) -> Evidence {
    let id = Hash::from_bytes([0xe2; 32]);
    let mut b = bid_of(&w.m, pubkey(MAKER), LIQ_BID);
    f(&mut b);
    let value = bid_escrow(&b, 1_500_000_000);
    let daa = w.chain.daa;
    Evidence::Bid { order: w.chain.mint_kob(KobObj::Bid(b), value, id, daa).bid() }
}

/// The check calls `ev` unusable for the word, and the contracts agree: the unchecked transaction is refused by an input.
fn assert_unusable(w: &mut W, ev: Evidence, word: &str) {
    let l = w.pair(0x31);
    let rep = check_evidence(&ev, &w.m, None);
    let text = rep.problems.join("; ");
    assert!(text.contains(word), "expected `{word}` among the problems, got: {text}");
    assert!(!rep.usable());
    let seller = w.tokens(9_000_000_000);
    w.rest();
    let f = w.funding();
    let eu = EvidenceUse { order: ev, fill: None, seller: Some(seller), now: None, allow_dead: false };
    let leg = Leg { pair: l, n: N, act: Act::Liq { side: if eu.order.is_ask() { LONG } else { SHORT } } };
    // The builder refuses it, in the same words.
    let e = evidence_tx(std::slice::from_ref(&leg), &eu, &f, &opts()).err().expect("the builder refuses").to_string();
    assert!(e.contains("cannot serve as evidence") && e.contains(word), "{e}");
    // The contracts refuse it too.
    let b = evidence_tx_unchecked(&[leg], &eu, &f, &opts()).expect("unchecked build");
    let s = b.sign(&keys()).expect("sign");
    let v = verify(&s.tx, &s.entries);
    assert!(v.inputs.iter().any(|r| r.is_err()), "the engine accepted evidence the check calls unusable ({word})");
}

#[test]
fn the_check_says_why_an_order_cannot_serve_and_the_contracts_agree() {
    // The market of the first cases: rest 700 DAA, so an IOC order (killed 600 DAA after it became fillable) cannot rest.
    let mut w = W::new();
    w.m.min_rest_daa = 700;
    let e = ask_with(&mut w, |a, _| a.expiry_daa = 1_000 + 300);
    assert_unusable(&mut w, e, "soft expiry");
    let e = ask_with(&mut w, |a, _| a.tif = 1);
    assert_unusable(&mut w, e, "IOC/FOK");
    let e = bid_with(&mut w, |b| b.tif = 2);
    assert_unusable(&mut w, e, "IOC/FOK");

    let mut w = W::new();
    let e = ask_with(&mut w, |a, t| {
        a.ext = Hash::from_bytes([0x66; 32]);
        t.ext = Hash::from_bytes([0x66; 32]);
    });
    assert_unusable(&mut w, e, "extension commitment");
    let e = ask_with(&mut w, |a, _| a.slope = 5);
    assert_unusable(&mut w, e, "price moves");
    let e = bid_with(&mut w, |b| b.slope = 5);
    assert_unusable(&mut w, e, "price moves");
    let e = ask_with(&mut w, |a, _| a.interval = -5);
    assert_unusable(&mut w, e, "negative");
    let e = ask_with(&mut w, |a, _| a.scale = 1_000);
    assert_unusable(&mut w, e, "scale");
    let e = ask_with(&mut w, |a, _| a.token_cov_id = Hash::from_bytes([0x77; 32]));
    assert_unusable(&mut w, e, "another token");
    let e = bid_with(&mut w, |b| b.token_cov_id = Hash::from_bytes([0x77; 32]));
    assert_unusable(&mut w, e, "another token");
    let e = ask_with(&mut w, |_, t| t.owner = [0x55; 32]);
    assert_unusable(&mut w, e, "not owned by this ask");
    let e = ask_with(&mut w, |_, t| t.amount -= 1);
    assert_unusable(&mut w, e, "custody holds");
    // A usable order passes, with its first usable DAA and what one fill of all of it covers.
    let e = ask_with(&mut w, |_, _| {});
    let r = check_evidence(&e, &w.m, None);
    assert!(r.usable(), "{:?}", r.reasons());
    assert_eq!(r.rest_end, e.daa() + 600);
    assert_eq!(r.covers, 1_500_000_000, "k = 1: a fill of all of it covers parts of the same total");
}

#[test]
fn the_rest_window_is_reported_against_the_daa_asked_about() {
    let mut w = W::new();
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    let r = check_evidence(&e, &w.m, Some(1_599));
    assert!(r.problems.is_empty());
    assert_eq!(r.waiting.len(), 1);
    assert!(r.waiting[0].contains("has not rested long enough") && r.waiting[0].contains("1600"), "{:?}", r.waiting);
    assert!(check_evidence(&e, &w.m, Some(1_600)).usable());
    // A transaction one DAA early is refused by the pair (the lock time proves less than the rest).
    let l = w.pair(0x31);
    w.rest();
    let f = w.funding();
    let b = liquidate(&[(l, N)], &eu(e, None), &f, &opts()).unwrap();
    assert_eq!(b.tx.lock_time, 1_600);
    refused(&b, 0, |m| m.tx.lock_time = 1_599);
    // Past the soft expiry the contract may still accept it within the pair's clock (the residual of DESIGN 4.5c), but the
    // check reports it dead and the builders refuse it unless `allow_dead` is set. Premise changed by review 3 (F1): this
    // was only a note ("use it now or not at all").
    let mut w = W::new();
    let e = ask_with(&mut w, |a, _| a.expiry_daa = 1_000 + 2_000);
    let r = check_evidence(&e, &w.m, Some(5_000));
    assert!(!r.usable() && r.problems.is_empty() && r.dead.iter().any(|n| n.contains("dead") && n.contains("soft expiry")), "{:?}", r.dead);
    let l = w.pair(0x31);
    w.rest();
    let f = w.funding();
    let mut u = eu(e, None);
    u.now = Some(5_000);
    let err = liquidate(&[(l.clone(), N)], &u, &f, &opts()).unwrap_err().to_string();
    assert!(err.contains("dead") && err.contains("--allow-dead-evidence"), "{err}");
    u.allow_dead = true;
    assert!(liquidate(&[(l, N)], &u, &f, &opts()).is_ok(), "the sweeping-test switch builds it");
}

#[test]
fn what_the_evidence_lets_a_pair_do() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let mk = w.m.clone();
    let rep = |e: &Evidence| check_evidence(e, &mk, None);
    let ask = w.ask(LIQ_ASK, 1_500_000_000);
    let v = pair_verdicts(&rep(&ask), &l.p);
    let by = |v: &[Verdict], p: &str| v.iter().find(|x| x.path.contains(p)).cloned().unwrap();
    assert!(by(&v, "the short closes").ok && !by(&v, "the long closes").ok);
    assert!(by(&v, "the long is liquidated").ok && !by(&v, "the short is liquidated").ok);
    assert!(by(&v, "the long is liquidated").why.contains("below the long's trigger"));
    let high = w.ask(1_000_000_000, 1_500_000_000);
    let v = pair_verdicts(&rep(&high), &l.p);
    assert!(!by(&v, "the long is liquidated").ok && by(&v, "the long is liquidated").why.contains("not below the long's trigger"));
    let bid = w.bid(LIQ_BID, 1_500_000_000);
    let v = pair_verdicts(&rep(&bid), &l.p);
    assert!(by(&v, "the long closes").ok && by(&v, "the short is liquidated").ok && !by(&v, "the long is liquidated").ok);
}

// ---------------------------------------------------------------- golden vectors

/// (name, txid, sha256 of the RPC JSON) of each shape: fixed TEST-ONLY keys and fixed inputs give fixed transactions
/// (signing is deterministic). `KPF_GOLDEN=print` shows the values after an intended change.
const GOLDEN: &[(&str, &str, &str)] = &[
    ("P5 long closes 1,000 at a 10 KAS bid", "28732120b43d24883a7a9c33dc774d5c780a731686d279a0cfd746b39b3aa8ff", "3b81d13016b0259d771a95eb8b352db1ddccaa6f7ef627f10d102dd9f1c83e68"),
    ("P5 short closes 400 at a 10.5 KAS ask", "dc5a5fff88b4a6d06e617be77ce3f3c43f153e59bd03202eb1fce05c57cf8a1d", "7355c09b9937fc029aa63039f19010fb606ea73ec8dde4ebd835ff1745e2b059"),
    ("P6 liquidates a long at a 9.4 KAS ask", "95732c2a28d69e435a347cf4385921dd94031d1a8dc864b93d5fab8b815ac656", "c3b070f9933d72b9d87cea973553c492e11d677b57d981aa00d5373395369733"),
    ("P6 liquidates a short at an 11.5 KAS bid", "ff2bf110dd6591643fd21ebffb2e63310680817f6168df44430fb93b8a52d1ef", "ac9ef5d9486654607b4339ff14b3f40bbab63b0bc50881277cc9ac859e9e5eb7"),
    ("P6 block of 3 longs on one 9.4 KAS ask", "20221e762c53a72b256c38bf9ae789cb8110b36ebcb013e0289eb0bfeefa8cfe", "d2f09d05ae9199317e9bff524e5703032f793ad58f7b9a1b8613409d3b9177dc"),
    ("P7 sells 400 of a liquidated long at 9.5 KAS", "7a03bc2e80b35fcdbe6af5f51d7db648a7f77a4ab6f27fefcd04fab930829834", "0874d488606a56da11c688d276b78526479873ab6c232e08da2774a691e85c6f"),
];

fn sha(v: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(serde_json::to_string(v).unwrap().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn golden_evidence_paths() {
    let mut rows: Vec<(String, String, String)> = vec![];
    let mut add = |name: &str, b: Built| {
        let s = b.sign(&keys()).unwrap();
        assert!(verify(&s.tx, &s.entries).is_ok(), "{name}");
        rows.push((name.to_string(), s.txid().to_string(), sha(&rpc_json(&s.tx))));
    };
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.bid(1_000_000_000, 1_500_000_000);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        add("P5 long closes 1,000 at a 10 KAS bid", close_on_evidence(&l, LONG, N, &eu(e, Some(seller)), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.ask(1_050_000_000, 1_500_000_000);
        w.rest();
        let f = w.funding();
        add("P5 short closes 400 at a 10.5 KAS ask", close_on_evidence(&l, SHORT, 400_000_000, &eu(e, None), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.ask(LIQ_ASK, 1_500_000_000);
        w.rest();
        let f = w.funding();
        add("P6 liquidates a long at a 9.4 KAS ask", liquidate(&[(l, N)], &eu(e, None), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.bid(LIQ_BID, 1_500_000_000);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        add("P6 liquidates a short at an 11.5 KAS bid", liquidate(&[(l, N)], &eu(e, Some(seller)), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let pairs: Vec<(LivePair, i64)> = (0..3).map(|i| (w.pair(0x40 + i), N)).collect();
        let e = w.ask(LIQ_ASK, 4 * N);
        w.rest();
        let f = w.funding();
        add("P6 block of 3 longs on one 9.4 KAS ask", liquidate(&pairs, &eu(e, None), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let o = entrant(&mut w, LONG, 0x71);
        let e = w.ask(LIQ_ASK, 1_500_000_000);
        w.rest();
        let f = w.funding();
        add("P7 sells 400 of a liquidated long at 9.5 KAS", liquidate_transfer(&l, 400_000_000, 950_000_000, &o, 0, &eu(e, None), &f, &opts()).unwrap());
    }
    if std::env::var("KPF_GOLDEN").as_deref() == Ok("print") {
        for (n, id, h) in &rows {
            println!("    (\"{n}\", \"{id}\", \"{h}\"),");
        }
        return;
    }
    assert_eq!(rows.len(), GOLDEN.len());
    for ((n, id, h), (gn, gid, gh)) in rows.iter().zip(GOLDEN) {
        assert_eq!(n, gn);
        assert_eq!(id, gid, "{n}: transaction id changed (KPF_GOLDEN=print shows the new values)");
        assert_eq!(h, gh, "{n}: full transaction (signatures, budgets, storage mass) changed");
    }
}

#[test]
fn the_default_sale_price_is_the_best_one_the_entrant_allows() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let lo = entrant(&mut w, LONG, 0x71);
    // Long loser: at most min(limit 10.5, bl + rate 10, bs - minCounterRate 10.5, entry 10, health at the ask
    // floor(9.4 * 0.95) + rate 1 = 9.93) = 9.93 KAS. Premise changed by review 3 (F3, DESIGN 4.5c): the default was 10, at
    // which the entrant's bound 9 equals the loser's and the same ask already liquidates it.
    assert_eq!(p7_default_x(&l.p, LONG, LIQ_ASK, &lo.o).unwrap(), 993_000_000);
    let e = p7_default_x(&l.p, LONG, 1_010_000_000, &lo.o).unwrap_err().to_string();
    assert!(e.contains("no price works"), "{e}");
    // Short loser: at least max(limit 9.5, bs - rate 10, bl + minCounterRate 9.5, entry 10, health at the bid
    // ceil(11.5 * 1.05) - rate 2 = 10.075) = 10.075 KAS (was 10 before review 3).
    let so = entrant(&mut w, SHORT, 0x72);
    assert_eq!(p7_default_x(&l.p, SHORT, LIQ_BID, &so.o).unwrap(), 1_007_500_000);
    assert!(p7_default_x(&l.p, SHORT, 990_000_000, &so.o).is_err());
    // The default is accepted by the contracts.
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    w.rest();
    let f = w.funding();
    let x = p7_default_x(&l.p, LONG, LIQ_ASK, &lo.o).unwrap();
    let b = liquidate_transfer(&l, N, x, &lo, 0, &eu(e, None), &f, &opts()).unwrap();
    w.run(&b);
}

#[test]
fn one_ask_serves_a_closing_short_and_a_liquidated_long_together() {
    let mut w = W::new();
    let (a, b) = (w.pair(0x31), w.pair(0x32));
    let e = w.ask(LIQ_ASK, 3_000_000_000);
    w.rest();
    let f = w.funding();
    let legs = [Leg { pair: a, n: N, act: Act::Close { side: SHORT } }, Leg { pair: b, n: 400_000_000, act: Act::Liq { side: LONG } }];
    let built = evidence_tx(&legs, &eu(e, None), &f, &opts()).unwrap();
    assert_outputs_are_least(&built);
    let (s, live) = w.run(&built);
    assert_eq!(pairs_of(&live).len(), 1, "the liquidated pair keeps a remainder; the closed one is gone");
    // The fill covers k x (1,000 + 400) tokens.
    assert!(s.output_labels.iter().any(|l| l.starts_with("tokens bought")));
}
