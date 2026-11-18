//! The two-step evidence builders (DESIGN 4.5d: propose, challenge, cure, finalize) on the in-memory chain: every input,
//! the pairs, the pending parts, KOB's orders and the token program, passes the script engine; mutations are refused.
//! Keys are the TEST-ONLY keys of `kpf_protocol::fixtures`.

use kaspa_consensus_core::Hash;
use kpf_protocol::build::{coop_close, FeeFrom, Funding, Opts};
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
        self.ask_id(price, amount, 0xe1)
    }

    fn ask_id(&mut self, price: i64, amount: i64, tag: u8) -> Evidence {
        let id = Hash::from_bytes([tag; 32]);
        let a = ask_of(&self.m, pubkey(MAKER), price, amount);
        let daa = self.chain.daa;
        let order = self.chain.mint_kob(KobObj::Ask(a), 10 * KAS, id, daa).ask();
        let custody = self.chain.mint_kob(KobObj::Token(TokenState::covid(amount, id, self.m.ext)), 20_000_000, self.m.token_cov_id, daa).token();
        Evidence::Ask { order, custody }
    }

    fn bid(&mut self, price: i64, amount: i64) -> Evidence {
        self.bid_id(price, amount, 0xe2)
    }

    fn bid_id(&mut self, price: i64, amount: i64, tag: u8) -> Evidence {
        let id = Hash::from_bytes([tag; 32]);
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
    let i = s.output_labels.iter().position(|l| l.contains(label)).unwrap_or_else(|| panic!("no output labelled {label}: {:?}", s.output_labels));
    (i, &s.tx.outputs[i])
}

fn pairs_of(live: &[Live]) -> Vec<LivePair> {
    live.iter().filter_map(|l| if let Live::Pair(p) = l { Some(p.clone()) } else { None }).collect()
}

fn pendings_of(live: &[Live]) -> Vec<LivePending> {
    live.iter().filter_map(|l| if let Live::Pending(p) = l { Some(p.clone()) } else { None }).collect()
}

fn q(n: i64, r: i64, scale: i64, c: i64) -> i64 {
    kpf_protocol::payoff::q(n, r, scale, c)
}

/// Labels of outputs a covenant bounds from below (a one-sompi cut must be refused by some input).
fn constrained(label: &str) -> bool {
    ["long paid", "short paid", "proposer", "evidence ask: maker paid", "evidence bid: tokens delivered", "evidence bid: rest of the escrow"]
        .iter()
        .any(|p| label.starts_with(p))
        || label.contains("remainder")
        || label.contains("pending part")
        || label.contains("restored pair")
        || label.starts_with("cured pair")
        || label.starts_with("forfeited part")
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
        Plan::Pair { args, .. } | Plan::Kob { args, .. } | Plan::Open { args, .. } | Plan::Pending { args, .. } => args[arg] = a,
        _ => panic!("not a contract input"),
    }
}

/// Proposes on `l` and returns the pending part (and the chain).
fn proposed(w: &mut W, l: &LivePair, member: Member, e: Evidence, seller: Option<LiveToken>) -> (Signed, LivePending, Vec<Live>) {
    let _ = l;
    let f = w.funding();
    let b = evidence_tx(&[member], &eu(e, seller), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (s, live) = w.run(&b);
    let p = pendings_of(&live).remove(0);
    (s, p, live)
}

// ---------------------------------------------------------------- P5: propose, then finalize

#[test]
fn p5_long_close_proposed_then_finalized_whole_and_in_part() {
    for n in [N, 400_000_000] {
        let mut w = W::new();
        let l = w.pair(0x31);
        let bid_at = 1_000_000_000; // 10 KAS
        let e = w.bid(bid_at, 1_500_000_000);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        let b = propose_close(&l, LONG, n, &eu(e, Some(seller)), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        let wp = pendings_of(&live).remove(0);
        assert_eq!((wp.w.kind, wp.w.side, wp.w.n, wp.w.quote), (KIND_CLOSE, LONG, n, bid_at));
        assert_eq!(pairs_of(&live).len(), usize::from(n < N), "a part keeps a remainder");
        assert!(s.output_labels.iter().any(|l| l.starts_with("evidence bid continuation")));
        println!("propose P5 long n={n}: size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
        // Finalize after W: the long is paid q(n, 10 - 9) plus its refund.
        let f = w.funding();
        let early = finalize(&wp, &f, &opts()).unwrap();
        let es = early.sign(&keys()).unwrap();
        assert!(w.chain.apply_signed(&es).is_err(), "the chain refuses a finalize before W (relative lock)");
        w.chain.advance(w.m.challenge_daa as u64);
        let f = w.funding();
        let b = finalize(&wp, &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, _) = w.run(&b);
        let (_, lp) = out_of(&s, "long paid");
        assert_eq!(lp.value as i64, q(n, bid_at - 900_000_000, SCALE, 0) + wp.w.refund);
        println!("finalize P5 long n={n}: size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
    }
}

#[test]
fn p5_short_close_proposed_partial_and_filled_out() {
    for (ask_amount, n) in [(1_500_000_000i64, N), (N, N), (1_500_000_000, 400_000_000)] {
        let mut w = W::new();
        let l = w.pair(0x31);
        let ask_at = 1_050_000_000; // 10.5 KAS
        let e = w.ask(ask_at, ask_amount);
        w.rest();
        let f = w.funding();
        let b = propose_close(&l, SHORT, n, &eu(e, None), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        let filled_out = ask_amount == n;
        assert_eq!(s.output_labels.iter().any(|l| l.starts_with("evidence ask continuation")), !filled_out);
        let toks: Vec<_> = live.iter().filter_map(|x| if let Live::Kob(k) = x { Some(k.clone()) } else { None }).collect();
        assert!(toks.iter().any(|k| matches!(&k.obj, KobObj::Token(t) if t.amount == n && t.owner == pubkey(F) && t.scheme == SCHEME_P2PK)));
        let wp = pendings_of(&live).remove(0);
        w.chain.advance(w.m.challenge_daa as u64);
        let f = w.funding();
        let (s, _) = w.run(&finalize(&wp, &f, &opts()).unwrap());
        let (_, sp) = out_of(&s, "short paid");
        assert_eq!(sp.value as i64, q(n, 1_200_000_000 - ask_at, SCALE, 0) + wp.w.refund);
    }
}

// ---------------------------------------------------------------- P6: propose, then forfeit and keep, then the winner

#[test]
fn p6_liquidation_proposed_then_forfeited_then_closed_by_the_winner() {
    for (side, n) in [(LONG, N), (LONG, 300_000_000), (SHORT, N)] {
        let mut w = W::new();
        let l = w.pair(0x31);
        let (e, seller) = if side == LONG { (w.ask(LIQ_ASK, 1_500_000_000), None) } else { (w.bid(LIQ_BID, 1_500_000_000), Some(w.tokens(1_500_000_000))) };
        w.rest();
        let f = w.funding();
        let b = propose_liquidate(&[(l.clone(), n)], &pubkey(F), &eu(e, seller), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        println!("propose P6 side {side} n={n}: size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
        let wp = pendings_of(&live).remove(0);
        assert_eq!(wp.w.fee, kpf_protocol::pending::claim_fee(&l.p, n, side, if side == LONG { LIQ_ASK } else { LIQ_BID }));
        w.chain.advance(w.m.challenge_daa as u64);
        let f = w.funding();
        let b = finalize(&wp, &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        let (_, pp) = out_of(&s, "proposer paid");
        assert_eq!(pp.value as i64, wp.w.bond + wp.w.fee);
        let fp = pairs_of(&live).into_iter().find(|p| p.p.long_key == p.p.short_key).expect("the forfeited part");
        let winner = if side == LONG { pubkey(S) } else { pubkey(L) };
        assert_eq!(fp.p.long_key, winner);
        // The winner closes it with its own two signatures, no funder.
        let b = coop_close(&fp, fp.value, FeeFrom::Long, None, &opts()).unwrap();
        w.run(&b);
        assert!(w.chain.balance(&winner) > 0);
    }
}

// ---------------------------------------------------------------- challenge and cure

/// A stale proposal (an ask at 9.40 while the market is at 10) is refuted by a third-party bid at 9.80 that rests R on
/// the proposal's clock: the part is restored with a fresh clock and the bond lands in the challenger's change.
#[test]
fn a_stale_proposal_is_refuted_by_a_challenge() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    w.rest();
    let (_, wp, _) = proposed(&mut w, &l, Member::Propose(Leg { pair: l.clone(), n: N, act: Act::Liq { side: LONG, proposer: pubkey(23) } }), e, None);
    // The counter bid: placed after the proposal, rests R.
    let cb = w.bid_id(980_000_000, 1_500_000_000, 0xe7);
    let seller = w.tokens(1_500_000_000);
    // Refused while it has not rested on the proposal's clock.
    let f = w.funding();
    let r = challenge(&[wp.clone()], &EvidenceUse { now: Some(w.chain.daa), ..eu(cb.clone(), Some(seller.clone())) }, &f, &opts()).unwrap();
    assert!(r.tx.lock_time >= wp.daa + w.m.min_rest_daa as u64);
    w.rest();
    let before = w.chain.balance(&pubkey(F));
    let f = w.funding();
    let b = challenge(&[wp.clone()], &eu(cb, Some(seller)), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (s, live) = w.run(&b);
    let restored = pairs_of(&live).remove(0);
    assert_eq!((restored.p.since, restored.p.long_key, restored.p.bl), (0, pubkey(L), l.p.bl));
    assert_eq!(restored.value as i64, wp.value as i64 - wp.w.bond);
    assert_eq!(w.chain.balance(&pubkey(23)), 0, "the refuted proposer gets nothing");
    let _ = (before, s);
    // A quote that does not contradict by g is refused by the builder, in plain words.
    let mut w = W::new();
    let l = w.pair(0x32);
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    w.rest();
    let (_, wp, _) = proposed(&mut w, &l, Member::Propose(Leg { pair: l.clone(), n: N, act: Act::Liq { side: LONG, proposer: pubkey(23) } }), e, None);
    let cb = w.bid_id(960_000_000, 1_500_000_000, 0xe7);
    let seller = w.tokens(1_500_000_000);
    w.rest();
    let f = w.funding();
    let err = challenge(&[wp], &eu(cb, Some(seller)), &f, &opts()).unwrap_err().to_string();
    assert!(err.contains("does not contradict") && err.contains("9.6631579"), "{err}");
}

#[test]
fn a_cure_moves_the_bound_and_repays_the_proposer() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.ask(LIQ_ASK, 1_500_000_000);
    w.rest();
    let (_, wp, _) = proposed(&mut w, &l, Member::Propose(Leg { pair: l.clone(), n: 400_000_000, act: Act::Liq { side: LONG, proposer: pubkey(23) } }), e, None);
    let f = w.funding();
    let b = cure(&wp, &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (s, live) = w.run(&b);
    let c = pairs_of(&live).remove(0);
    assert_eq!((c.p.bl, c.p.since), (893_000_000, l.clock()));
    assert_eq!(w.chain.balance(&pubkey(23)), wp.w.refund as u64);
    println!("cure: size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
    // A close cannot be cured.
    let mut w = W::new();
    let l = w.pair(0x33);
    let e = w.ask(1_050_000_000, 1_500_000_000);
    w.rest();
    let (_, wc, _) = proposed(&mut w, &l, Member::Propose(Leg { pair: l.clone(), n: N, act: Act::Close { side: SHORT } }), e, None);
    let f = w.funding();
    assert!(cure(&wc, &f, &opts()).unwrap_err().to_string().contains("only a pending liquidation"));
}

#[test]
fn p6_blocks_of_proposals_on_one_evidence() {
    for k in [2usize, 4, 8] {
        let mut w = W::new();
        let pairs: Vec<LivePair> = (0..k).map(|i| w.pair(0x40 + i as u8)).collect();
        // k_bps = 10^4: the block needs a fill of k x 1,000 tokens.
        let e = w.ask(LIQ_ASK, (k as i64 + 1) * N);
        w.rest();
        let f = w.funding();
        let parts: Vec<(LivePair, i64)> = pairs.iter().map(|p| (p.clone(), N)).collect();
        let b = propose_liquidate(&parts, &pubkey(F), &eu(e.clone(), None), &f, &opts()).unwrap();
        assert_outputs_are_least(&b);
        let (s, live) = w.run(&b);
        println!("propose block of {k}: size {} fee {} mass {:?}", s.mass.size, s.fee, s.mass);
        assert_eq!(pendings_of(&live).len(), k);
    }
    // Nine pairs are refused by the builder (the block scan covers 8).
    let mut w = W::new();
    let pairs: Vec<LivePair> = (0..9).map(|i| w.pair(0x40 + i)).collect();
    let e = w.ask(LIQ_ASK, 90_000_000_000);
    w.rest();
    let f = w.funding();
    let parts: Vec<(LivePair, i64)> = pairs.iter().map(|p| (p.clone(), N)).collect();
    let err = propose_liquidate(&parts, &pubkey(F), &eu(e, None), &f, &opts()).unwrap_err().to_string();
    assert!(err.contains("at most 8"), "{err}");
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
    let pk = pubkey(F);
    let err = |r: kpf_protocol::Result<Built>| r.err().expect("refused").to_string();
    // A long closes at a bid, not an ask; a short at an ask.
    assert!(err(propose_close(&l, LONG, N, &eu(ask.clone(), None), &f, &opts())).contains("closes at a resting bid"));
    assert!(err(propose_close(&l, SHORT, N, &eu(bid.clone(), Some(seller.clone())), &f, &opts())).contains("closes at a resting ask"));
    // A short is liquidated by a bid; the base pair's short is not liquidatable at 10 KAS.
    assert!(err(propose_liquidate(&[(l.clone(), N)], &pk, &eu(w.bid(1_000_000_000, 1_500_000_000), Some(seller.clone())), &f, &opts())).contains("not liquidatable"));
    // A long is not liquidatable at an ask above its trigger.
    assert!(err(propose_liquidate(&[(l.clone(), N)], &pk, &eu(w.ask(1_000_000_000, 1_500_000_000), None), &f, &opts())).contains("not liquidatable"));
    // The evidence must cover k x n: an ask of 500 tokens cannot serve a 1,000-token part.
    assert!(err(propose_liquidate(&[(l.clone(), N)], &pk, &eu(w.ask(LIQ_ASK, 500_000_000), None), &f, &opts())).contains("fewer than"));
    // A bid needs tokens to sell into it (a pair whose short is liquidatable at 11.5: bs 10).
    let l2 = {
        let mut p = pair(pubkey(L), pubkey(S));
        p.m = w.m.clone();
        p.bl = 800_000_000;
        p.bs = 1_000_000_000;
        let daa = w.chain.daa;
        w.chain.mint_obj(Obj::Pair(p.clone()), p.floor() as u64, Hash::from_bytes([0x33; 32]), daa).pair()
    };
    assert!(err(propose_liquidate(&[(l2.clone(), N)], &pk, &eu(bid.clone(), None), &f, &opts())).contains("tokens to sell"));
    // The seller must hold enough.
    let small = w.tokens(10_000_000);
    assert!(err(propose_liquidate(&[(l2.clone(), N)], &pk, &eu(bid.clone(), Some(small)), &f, &opts())).contains("holds"));
    // Empty, repeated and invalid parts.
    assert!(err(propose_liquidate(&[], &pk, &eu(ask.clone(), None), &f, &opts())).contains("no pair"));
    assert!(err(propose_liquidate(&[(l.clone(), N), (l.clone(), N)], &pk, &eu(w.ask(LIQ_ASK, 5_000_000_000), None), &f, &opts())).contains("covenant id of an earlier member"));
    assert!(err(propose_liquidate(&[(l.clone(), 1)], &pk, &eu(ask.clone(), None), &f, &opts())).contains("not a valid part"));
    // The evidence of another market (the order prices another token).
    let mut other = w.m.clone();
    other.token_cov_id = Hash::from_bytes([0x99; 32]);
    let mut l3 = l.clone();
    l3.p.m = other;
    assert!(err(propose_liquidate(&[(l3, N)], &pk, &eu(ask, None), &f, &opts())).contains("another token"));
}

#[test]
fn mutations_are_refused_by_the_pair() {
    let mut w = W::new();
    let l = w.pair(0x31);
    let e = w.bid(1_000_000_000, 1_500_000_000);
    let seller = w.tokens(1_500_000_000);
    w.rest();
    let f = w.funding();
    let b = propose_close(&l, LONG, N, &eu(e, Some(seller)), &f, &opts()).unwrap();
    // The unmutated transaction is accepted.
    let s = b.sign(&keys()).unwrap();
    assert!(verify(&s.tx, &s.entries).is_ok());
    // The block's evidence fill one base unit below k x n.
    let lock = b.tx.lock_time;
    refused(&b, 0, |m| set_arg(m, 1, 0, bv(&sm8(N - 1))));
    // A wrong q, kind or side; a non-owner's signature.
    refused(&b, 0, |m| set_arg(m, 0, 1, bv(&sm8(1))));
    refused(&b, 0, |m| set_arg(m, 0, 2, iv(2)));
    refused(&b, 0, |m| set_arg(m, 0, 4, iv(SHORT)));
    refused(&b, 0, |m| set_arg(m, 0, 3, Arg::Sig(pubkey(S))));
    // The lock time one DAA short of the rest window.
    refused(&b, 0, |m| m.tx.lock_time = lock - 1);
    // The pending part one sompi short.
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
    let built = propose_close(&l, LONG, N, &eu(Evidence::Bid { order }, Some(seller)), &f, &opts()).unwrap();
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
    m2.min_evidence_life = 0; // with the testnet L of 864,000 DAA no IOC order is evidence (DESIGN 4.5d)
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
    let built = propose_close(&l, SHORT, N, &eu(Evidence::Ask { order, custody }, None), &f, &opts()).unwrap();
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
    let built = propose_close(&l, LONG, N, &eu(Evidence::Bid { order }, Some(seller)), &f, &opts()).unwrap();
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
    let leg = Member::Propose(Leg { pair: l, n: N, act: Act::Liq { side: if eu.order.is_ask() { LONG } else { SHORT }, proposer: pubkey(F) } });
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
    let b = propose_liquidate(&[(l, N)], &pubkey(F), &eu(e, None), &f, &opts()).unwrap();
    assert_eq!(b.tx.lock_time, 1_600);
    refused(&b, 0, |m| m.tx.lock_time = 1_599);
    // Past the soft expiry the contract may still accept it within the pair's clock (the residual of DESIGN 4.5c), but the
    // check reports it dead and the builders refuse it unless `allow_dead` is set. Premise changed by review 3 (F1): this
    // was only a note ("use it now or not at all").
    let mut w = W::new();
    // The order lives through the end of its rest plus the evidence life L (DESIGN 4.5d), then dies.
    let life = w.m.min_evidence_life;
    let e = ask_with(&mut w, |a, _| a.expiry_daa = 1_000 + 600 + life + 2_000);
    let later = (1_000 + 600 + life + 5_000) as u64;
    let r = check_evidence(&e, &w.m, Some(1_000 + 600 + 864_000 + 5_000));
    assert!(!r.usable() && r.problems.is_empty() && r.dead.iter().any(|n| n.contains("dead") && n.contains("soft expiry")), "{:?}", r.dead);
    let l = w.pair(0x31);
    w.rest();
    let f = w.funding();
    let mut u = eu(e, None);
    u.now = Some(later);
    let err = propose_liquidate(&[(l.clone(), N)], &pubkey(F), &u, &f, &opts()).unwrap_err().to_string();
    assert!(err.contains("dead") && err.contains("--allow-dead-evidence"), "{err}");
    u.allow_dead = true;
    assert!(propose_liquidate(&[(l, N)], &pubkey(F), &u, &f, &opts()).is_ok(), "the sweeping-test switch builds it");
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
    ("propose: the long closes 1,000 at a 10 KAS bid", "e35ae05d9eeaccfcd94b9267fe55e34def468988cbc8766969c998211b1595ff", "7c28b627bdad4beee6dbfd5fbe4de67df12d763003f47d30f2d204f29e45e248"),
    ("finalize: the long's close", "abe4a75977fa7c383463ee1fcdd61bf1b526cab59c7eeea4bbd7e86752f5988b", "e9a202cdeef04cf40b72ad9e334318314943540fdca8f3a5e489d66bbc3623af"),
    ("propose: the short closes 400 at a 10.5 KAS ask", "d9f3495f67a3563460ad2883f30262f92533b5121e94488921b4db2371583297", "a9829cfdf25c964a006b5eb1c7a9a2894f919946431353ca568ca404fdc16a50"),
    ("propose: a long's liquidation at a 9.4 KAS ask", "7d35c44728a973e852ab716a4b9f35f6c895a2e00d6f62ccde8e374a803d5845", "9879865a53e2a3da2d1f41f4039090a18a21f2382690afcfe50f849747dbc65a"),
    ("cure: the long moves bl to 8.93", "f8b706bf9abf935f5686576a47c38609d776a5dc18013be41810738cafa20e24", "7031a9b3d60cadea40d516196bbbcd3f95387f9e6b52397d6cb1d72d0fa6c1f2"),
    ("challenge: a 9.80 KAS bid refutes it", "33d63949374e81c53952d423594cec43aefd377831bd4c54af454ac8457629e5", "5aeca24da1033b941a5ce66f0e0100f19dbda66c38933e233c5c9560cf2da4f0"),
    ("finalize: forfeit and keep", "39b97aae5958d5c0fda2c4ddda3becb24074475b8019eb5cae63bb48d525024b", "5322b49e20c34038e47bdd90f192bb2815788f1f6923e79e2ab59147c15c1f0a"),
    ("propose: a short's liquidation at an 11.5 KAS bid", "86c06f28d6407649554cb8d597891dbb41b9b69df931d19a5e76d302fbeeb1d3", "44ef8ceafedd97d4fec138cd690c024986430ed46bbd628d35cbe519f888550b"),
    ("propose: a block of 3 longs on one 9.4 KAS ask", "4d73acdd9c0f78352bc2a7e0a2814620816a6ddf2ae3264af2f19fc09951b49e", "0065812c2fe359305576a133ebb0497f151a58a72825924f48d98029b5216e7c"),
];

fn sha(v: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(serde_json::to_string(v).unwrap().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn golden_evidence_paths() {
    let mut rows: Vec<(String, String, String)> = vec![];
    let mut add = |name: &str, b: &Built| {
        let s = b.sign(&keys()).unwrap();
        assert!(verify(&s.tx, &s.entries).is_ok(), "{name}");
        rows.push((name.to_string(), s.txid().to_string(), sha(&rpc_json(&s.tx))));
        s
    };
    let pk = pubkey(F);
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.bid(1_000_000_000, 1_500_000_000);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        let b = propose_close(&l, LONG, N, &eu(e, Some(seller)), &f, &opts()).unwrap();
        let s = add("propose: the long closes 1,000 at a 10 KAS bid", &b);
        let live = w.chain.apply_signed(&s).unwrap();
        let wp = pendings_of(&live).remove(0);
        w.chain.advance(w.m.challenge_daa as u64);
        let f = w.funding();
        add("finalize: the long's close", &finalize(&wp, &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.ask(1_050_000_000, 1_500_000_000);
        w.rest();
        let f = w.funding();
        add("propose: the short closes 400 at a 10.5 KAS ask", &propose_close(&l, SHORT, 400_000_000, &eu(e, None), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.ask(LIQ_ASK, 1_500_000_000);
        w.rest();
        let f = w.funding();
        let b = propose_liquidate(&[(l, N)], &pk, &eu(e, None), &f, &opts()).unwrap();
        let s = add("propose: a long's liquidation at a 9.4 KAS ask", &b);
        let live = w.chain.apply_signed(&s).unwrap();
        let wp = pendings_of(&live).remove(0);
        let f = w.funding();
        add("cure: the long moves bl to 8.93", &cure(&wp, &f, &opts()).unwrap());
        let cb = w.bid_id(980_000_000, 1_500_000_000, 0xe7);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        add("challenge: a 9.80 KAS bid refutes it", &challenge(&[wp.clone()], &eu(cb, Some(seller)), &f, &opts()).unwrap());
        w.chain.advance(w.m.challenge_daa as u64);
        let f = w.funding();
        add("finalize: forfeit and keep", &finalize(&wp, &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let l = w.pair(0x31);
        let e = w.bid(LIQ_BID, 1_500_000_000);
        let seller = w.tokens(1_500_000_000);
        w.rest();
        let f = w.funding();
        add("propose: a short's liquidation at an 11.5 KAS bid", &propose_liquidate(&[(l, N)], &pk, &eu(e, Some(seller)), &f, &opts()).unwrap());
    }
    {
        let mut w = W::new();
        let pairs: Vec<(LivePair, i64)> = (0..3).map(|i| (w.pair(0x40 + i), N)).collect();
        let e = w.ask(LIQ_ASK, 4 * N);
        w.rest();
        let f = w.funding();
        add("propose: a block of 3 longs on one 9.4 KAS ask", &propose_liquidate(&pairs, &pk, &eu(e, None), &f, &opts()).unwrap());
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

/// One ask serves a closing short's proposal and a liquidated long's proposal together, and one counter bid refutes
/// two pending parts at once.
#[test]
fn one_print_serves_a_block_of_proposals_and_one_of_challenges() {
    let mut w = W::new();
    let (a, b) = (w.pair(0x31), w.pair(0x32));
    let e = w.ask(LIQ_ASK, 3_000_000_000);
    w.rest();
    let f = w.funding();
    let legs = [
        Member::Propose(Leg { pair: a, n: N, act: Act::Close { side: SHORT } }),
        Member::Propose(Leg { pair: b, n: 400_000_000, act: Act::Liq { side: LONG, proposer: pubkey(F) } }),
    ];
    let built = evidence_tx(&legs, &eu(e, None), &f, &opts()).unwrap();
    assert_outputs_are_least(&built);
    let (s, live) = w.run(&built);
    assert_eq!(pendings_of(&live).len(), 2);
    assert_eq!(pairs_of(&live).len(), 1, "the partial proposal keeps a remainder");
    assert!(s.output_labels.iter().any(|l| l.starts_with("tokens bought")));
    // Two liquidation claims refuted by one bid.
    let mut w = W::new();
    let (a, b) = (w.pair(0x33), w.pair(0x34));
    let e = w.ask(LIQ_ASK, 3_000_000_000);
    w.rest();
    let f = w.funding();
    let built = propose_liquidate(&[(a, N), (b, N)], &pubkey(23), &eu(e, None), &f, &opts()).unwrap();
    let (_, live) = w.run(&built);
    let ws = pendings_of(&live);
    let cb = w.bid_id(980_000_000, 2_500_000_000, 0xe7);
    let seller = w.tokens(2_500_000_000);
    w.rest();
    let f = w.funding();
    let b = challenge(&ws, &eu(cb, Some(seller)), &f, &opts()).unwrap();
    assert_outputs_are_least(&b);
    let (_, live) = w.run(&b);
    assert_eq!(pairs_of(&live).len(), 2);
}
