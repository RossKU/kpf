//! Two-step evidence (DESIGN 4.5d): `KpfPair.propose`, `KpfPending.challenge`, `cure` and `finalize`, positive and
//! negative, with the real KOB order and token contracts executing in the same transaction as the evidence tests do.
//!
//! The base pair is the worked example (1,000 tokens, bl 9, bs 12 KAS, mmr 5 %, g 2 %, W 6,000, R 600, L 864,000,
//! bond 300 bps of the range with a 5 KAS floor: 90 KAS for the whole pair). The long is liquidatable at an ask
//! <= 947,368,421, the short at a bid >= 1,142,857,143. A liquidation claim on the long is refuted by a bid b with
//! b * 9,500 >= 9 * 10^8 * 10,200, i.e. b >= 966,315,790; on the short by an ask a with a * 10,500 <=
//! 1.2 * 10^9 * 9,800, i.e. a <= 1,120,000,000. A close of the long at the bid 10.5 is refuted by an ask
//! a <= 10.5 * 0.98 = 10.29; of the short at the ask 11 by a bid b >= 11 * 1.02 = 11.22.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

fn accept(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "no sompi created");
    tx
}

fn refused(d: &Draft, i: usize) {
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, i);
}

fn fk() -> Keypair {
    keypair(14)
}
fn lk() -> Keypair {
    keypair(11)
}
fn sk() -> Keypair {
    keypair(12)
}

fn base() -> Pair {
    pair(&lk(), &sk())
}

/// The base pair in a UTXO of DAA 1,000 (its clock start S = 1,000).
fn lv(tag: u8) -> Live {
    live(&base(), tag).at(1_000)
}

const FULL: i64 = 1_000_000_000;
const HALF: i64 = 500_000_000;
const LIQ_ASK: i64 = 940_000_000;
const LIQ_BID: i64 = 1_150_000_000;
const CL_BID: i64 = 1_050_000_000;
const CL_ASK: i64 = 1_100_000_000;
/// The counter quotes at which each claim of the base pair is exactly refuted (see the module docs).
const REF_LIQ_LONG: i64 = 966_315_790;
const REF_LIQ_SHORT: i64 = 1_120_000_000;
const REF_CL_LONG: i64 = 1_029_000_000;
const REF_CL_SHORT: i64 = 1_122_000_000;

fn close(l: &Live, n: i64, side: i64) -> PairLeg {
    let signer = if side == LONG { lk() } else { sk() };
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn ask(p: i64, fill: i64) -> Evidence {
    Evidence::ask(p, fill)
}
fn bid(p: i64, fill: i64) -> Evidence {
    Evidence::bid(p, fill)
}

/// The four claims on the base pair, each with its proposal evidence and the counter quote that just refutes it.
fn claims(l: &Live, n: i64) -> Vec<(PairLeg, Evidence, Evidence)> {
    vec![
        (close(l, n, LONG), bid(CL_BID, n), ask(REF_CL_LONG, n)),
        (close(l, n, SHORT), ask(CL_ASK, n), bid(REF_CL_SHORT, n)),
        (liq(l, n, LONG), ask(LIQ_ASK, n), bid(REF_LIQ_LONG, n)),
        (liq(l, n, SHORT), bid(LIQ_BID, n), ask(REF_LIQ_SHORT, n)),
    ]
}

/// The counter quote one time short of refuting (in the claim's favour).
fn one_price_short(e: &Evidence) -> Evidence {
    let mut x = e.clone();
    match x.kind {
        Kind::Ask => x.ask.price += 1,
        Kind::Bid => {
            x.bid.price -= 1;
            x = Evidence::bid(x.bid.price, x.fill);
        }
    }
    x
}

// ================================================================ end to end

#[test]
fn propose_challenge_cure_finalize_end_to_end() {
    let l = lv(0x31);
    for n in [FULL, HALF] {
        for (g, e, c) in claims(&l, n) {
            accept(&ev_draft(&[g.clone()], &e, &fk(), 1));
            let w = pend(&g, e.price());
            accept(&ch_draft(&[w.clone()], &c, &fk(), 2));
            accept(&finalize_draft(&w, &fk(), 3));
            if w.w.kind == KIND_LIQ {
                let signer = if w.w.side == LONG { lk() } else { sk() };
                accept(&cure_draft(&w, &signer, &fk(), 4));
            }
        }
    }
}

/// The pending part written by a proposal, field by field: what the pair fixes and the values each branch pays.
#[test]
fn propose_writes_the_claim_and_the_three_return_states() {
    let l = lv(0x32);
    let s = l.clock();
    let g = liq(&l, HALF, LONG);
    let d = ev_draft(&[g], &ask(LIQ_ASK, HALF), &fk(), 1);
    accept(&d);
    let w = &d.pendings[0].1;
    assert_eq!((w.kind, w.side, w.n, w.bl, w.bs, w.quote), (KIND_LIQ, LONG, HALF, l.p.bl, l.p.bs, LIQ_ASK));
    assert_eq!(w.bond, 4_500_000_000, "300 bps of the part's range: q(500 tokens, 0.09 KAS)");
    assert_eq!(w.refund, w.bond + C, "a part carries the carrier its proposer funded");
    // DESIGN 4.5e: the cure clears the claim by g (9.40 * 0.98 * 0.95 = 8.7514; 8.93 before review 4).
    assert_eq!(w.cure_bound, 875_140_000);
    assert_eq!(w.fee, liq_fee(&l.p, HALF, LIQ_ASK, LONG));
    assert_eq!((w.since, w.proposer_key), (s, xonly(&proposer_kp())));
    // Every return state is unlisted (DESIGN 4.5e); the pending state itself records the pair's listings.
    assert_eq!((w.long_ask, w.short_bid), (l.p.long_ask, l.p.short_bid));
    let r = w.restore_pair().unwrap();
    assert_eq!((r.size, r.bl, r.bs, r.since, r.tag_l, r.tag_s), (HALF, l.p.bl, l.p.bs, s, ZERO, ZERO));
    assert_eq!((r.long_ask, r.long_min, r.long_tip, r.short_bid, r.short_min, r.short_tip), (0, 0, 0, 0, 0, 0));
    let c = w.cure_pair().unwrap();
    assert_eq!((c.bl, c.bs, c.since, c.long_key, c.long_ask, c.short_bid), (875_140_000, l.p.bs, s, l.p.long_key, 0, 0));
    let f = w.forfeit_pair().unwrap();
    assert_eq!((f.long_key, f.short_key, f.since, f.bs, f.long_ask, f.short_bid), (l.p.short_key, l.p.short_key, 0, l.p.bs, 0, 0));
    assert!(q(HALF, f.bs - f.bl, SCALE, 0) + w.fee <= l.p.range_of(HALF));
    assert!(q(HALF, f.bs - f.bl + 1, SCALE, 0) + w.fee > l.p.range_of(HALF));
    // The pending value: the part, its carrier and the bond (the remainder keeps the rest).
    assert_eq!(d.outs[0].value as i64, l.p.range_of(HALF) + C + w.bond);
}

// ================================================================ propose

#[test]
fn propose_with_any_field_of_the_claim_changed_is_refused() {
    let l = lv(0x33);
    for (g, e, _) in claims(&l, HALF) {
        let d = ev_draft(&[g], &e, &fk(), 1);
        accept(&d);
        let w = d.pendings[0].1.clone();
        let z = [0x5a; 32];
        let muts: Vec<Box<dyn Fn(&mut Pending)>> = vec![
            Box::new(|w| w.kind = 3 - w.kind),
            Box::new(|w| w.side = 3 - w.side),
            Box::new(|w| w.n += 1),
            Box::new(|w| w.bl -= 1),
            Box::new(|w| w.bs += 1),
            Box::new(|w| w.quote += 1),
            Box::new(|w| w.cure_bound += 1),
            Box::new(|w| w.fee += 1),
            Box::new(|w| w.bond -= 1),
            Box::new(|w| w.refund -= 1),
            Box::new(|w| w.entry += 1),
            Box::new(|w| w.long_key = xonly(&keypair(13))),
            Box::new(|w| w.short_key = xonly(&keypair(13))),
            Box::new(|w| w.long_ask = 1),
            Box::new(|w| w.short_tip = 1),
            Box::new(|w| w.since += 1),
            Box::new(|w| w.proposer_key = xonly(&keypair(13))),
            Box::new(move |w| w.restore_hash = z),
            Box::new(move |w| w.cure_hash = z),
            Box::new(move |w| w.forfeit_hash = z),
        ];
        for (k, m) in muts.iter().enumerate() {
            let mut w2 = w.clone();
            m(&mut w2);
            if w2 == w {
                continue;
            }
            let mut x = d.clone();
            x.outs[0].script_public_key = p2sh(&w2.art());
            let (tx, en) = x.build();
            assert!(run(&tx, &en).unwrap()[0].is_err(), "mutation {k} of the pending state must be refused");
        }
    }
}

#[test]
fn propose_with_another_pending_template_is_refused() {
    let l = lv(0x34);
    let d = ev_draft(&[liq(&l, FULL, LONG)], &ask(LIQ_ASK, FULL), &fk(), 1);
    accept(&d);
    // A template whose hash is not PENDING_TPL (one suffix byte changed), with the output built from it.
    let (pre, mut suf) = parts(kpf_protocol::artifacts::pending_base());
    let last = suf.len() - 1;
    suf[last] ^= 1;
    let mut x = d.clone();
    let mut a = x.args(0);
    a[ARG_PRE + 1] = bv(&suf);
    x.set_args(0, a);
    let w = &d.pendings[0].1;
    let rs = [pre.clone(), w.state_bytes(), suf.clone()].concat();
    x.outs[0].script_public_key = kaspa_txscript::pay_to_script_hash_script(&rs);
    refused(&x, 0);
    // The genuine template but another prefix length.
    let mut x = d.clone();
    let mut a = x.args(0);
    a[ARG_PRE] = bv(&[pre.clone(), vec![0x00]].concat());
    x.set_args(0, a);
    refused(&x, 0);
}

#[test]
fn propose_value_one_sompi_short_is_refused() {
    let l = lv(0x35);
    for (g, e, _) in claims(&l, FULL) {
        let d = ev_draft(&[g], &e, &fk(), 1);
        accept(&d);
        // Whole pair: the pending part holds this UTXO's value plus the bond.
        assert_eq!(d.outs[0].value, l.value + d.pendings[0].1.bond as u64);
        let mut x = d.clone();
        x.outs[0].value -= 1;
        refused(&x, 0);
    }
    for (g, e, _) in claims(&l, HALF) {
        let d = ev_draft(&[g], &e, &fk(), 1);
        accept(&d);
        // A part: its range, its carrier (funded by the proposer) and the bond; the remainder keeps the rest.
        for j in [0, d.pairs[0].0] {
            let mut x = d.clone();
            x.outs[j].value -= 1;
            refused(&x, 0);
        }
    }
}

#[test]
fn propose_ordinal_of_the_pending_part() {
    let l = lv(0x36);
    let g = liq(&l, HALF, LONG);
    let d = ev_draft(&[g], &ask(LIQ_ASK, HALF), &fk(), 1);
    accept(&d);
    // The remainder first and the pending part second: k = 1.
    let r = d.pairs[0].0;
    let mut x = d.clone();
    x.outs.swap(0, r);
    let mut a = x.args(0);
    a[ARG_K] = iv(1);
    x.set_args(0, a.clone());
    accept(&x);
    // k = 0 for that order, or k outside {0, 1}, is refused.
    for k in [0, 2, -1] {
        let mut y = x.clone();
        let mut a = a.clone();
        a[ARG_K] = iv(k);
        y.set_args(0, a);
        refused(&y, 0);
    }
}

/// The forfeit bound is an argument and must be the nearest bound at which the narrowed range and the fee fit the
/// part: one time wider does not fit, one time narrower is not the nearest, each refused even when the pending state is
/// written with the hash of that bound.
#[test]
fn propose_forfeit_bound_must_be_the_nearest() {
    let l = lv(0x37);
    for (side, e) in [(LONG, ask(LIQ_ASK, FULL)), (SHORT, bid(LIQ_BID, FULL))] {
        let d = ev_draft(&[liq(&l, FULL, side)], &e, &fk(), 1);
        accept(&d);
        let fb = d.args(0)[ARG_FB].clone();
        let fb = match fb {
            Arg::V(silverscript_abi::ArtifactValue::Int(v)) => v,
            _ => unreachable!(),
        };
        for bad_fb in [fb - 1, fb + 1] {
            let mut x = d.clone();
            let mut a = x.args(0);
            a[ARG_FB] = iv(bad_fb);
            x.set_args(0, a);
            refused(&x, 0);
            // With the forfeit hash of that bound in the state.
            let mut w = d.pendings[0].1.clone();
            w.forfeit_hash = kpf_protocol::pending::script_hash(&kpf_protocol::pending::forfeit_state(&l.clocked(), FULL, side, bad_fb));
            x.outs[0].script_public_key = p2sh(&w.art());
            refused(&x, 0);
        }
    }
}

#[test]
fn propose_liquidation_by_anyone_for_any_proposer_key() {
    let l = lv(0x38);
    for pk in [[7u8; 32], xonly(&lk()), xonly(&sk())] {
        let g = PairLeg { l: l.clone(), n: FULL, q: 0, act: Act::LiqBy { side: LONG, proposer: pk } };
        let d = ev_draft(&[g], &ask(LIQ_ASK, FULL), &fk(), 1);
        accept(&d);
        assert_eq!(d.pendings[0].1.proposer_key, pk);
    }
    // A close names the closer as the proposer whatever pk says.
    let d = ev_draft(&[close(&l, FULL, LONG)], &bid(CL_BID, FULL), &fk(), 1);
    assert_eq!(d.pendings[0].1.proposer_key, l.p.long_key);
    let mut x = d.clone();
    let mut w = d.pendings[0].1.clone();
    w.proposer_key = [7; 32];
    x.outs[0].script_public_key = p2sh(&w.art());
    refused(&x, 0);
}

#[test]
fn propose_market_window_outside_the_csv_range_is_refused() {
    for w in [-1, 4_294_967_296] {
        let mut p = base();
        p.m.challenge_daa = w;
        let l = live(&p, 0x39).at(1_000);
        refused(&ev_draft(&[liq(&l, FULL, LONG)], &ask(LIQ_ASK, FULL), &fk(), 1), 0);
    }
    let mut p = base();
    p.m.challenge_daa = 4_294_967_295;
    accept(&ev_draft(&[liq(&live(&p, 0x39).at(1_000), FULL, LONG)], &ask(LIQ_ASK, FULL), &fk(), 1));
}

/// minEvidenceLife (DESIGN 4.5d): the proposal evidence must stay fillable through its rest end plus L.
#[test]
fn propose_evidence_needs_the_evidence_life() {
    let l = lv(0x3a);
    let e = ask(LIQ_ASK, FULL);
    let rest_end = e.daa as i64 + rest_daa() as i64;
    let life = rest_end + market().min_evidence_life;
    for (expiry, good) in [(life, true), (life - 1, false)] {
        let mut x = e.clone();
        x.ask.expiry_daa = expiry;
        let d = ev_draft(&[liq(&l, FULL, LONG)], &x, &fk(), 1);
        if good {
            accept(&d);
        } else {
            refused(&d, 0);
        }
    }
    // An IOC or FOK order can never be evidence while L >= 600 (its kill is 600 after it became fillable).
    for tif in [1, 2] {
        refused(&ev_draft(&[close(&l, FULL, LONG)], &Evidence::ending_bid(CL_BID, FULL, tif), &fk(), 1), 0);
    }
}

// ================================================================ challenge

/// A stale proposal refuted (review 3 F1 under the two-step rule): the market is at 10 KAS; the short holds an ask
/// at 9.40 that rested on the pair's clock (still fillable, but nobody would sell there now). Under 4.5c it
/// liquidated the long at once; now it only proposes, and any fresh bid at or above 9.6632 that rested R on the
/// proposal's clock refutes it. The bond goes to the challenger and the part returns with the pair's clock S kept.
#[test]
fn stale_proposal_is_refuted() {
    let l = lv(0x3b);
    let g = liq(&l, FULL, LONG);
    let stale = ask(LIQ_ASK, FULL);
    accept(&ev_draft(&[g.clone()], &stale, &fk(), 1));
    let w = pend(&g, LIQ_ASK);
    // A third-party bid at 9.80 (fresh) refutes it.
    let d = ch_draft(&[w.clone()], &bid(980_000_000, FULL), &fk(), 2);
    let tx = accept(&d);
    assert_eq!(d.outs[0].value as i64, w.value as i64 - w.w.bond, "the restored pair holds the value less the bond");
    let restored = live_pairs(&tx, &d).remove(0);
    assert_eq!((restored.p.since, restored.p.bl, restored.p.long_key), (l.clock(), l.p.bl, l.p.long_key));
    // The restored pair keeps the clock S: the same stale ask (rested on S) may be proposed again at once, and is
    // refuted again (`holds_a_stale_proposer_pays_a_bond_every_round`).
    let t = NOW + 100;
    let r = restored.at(t);
    let mut d = ev_draft(&[liq(&r, FULL, LONG)], &stale, &fk(), 3);
    d.lock = t;
    accept(&d);
}

/// A truthful liquidation (the market really is below the trigger): no counter bid in the market contradicts it by g,
/// so a challenge with a bid one time below the refutation bound is refused; the claim finalizes.
#[test]
fn truthful_proposal_cannot_be_refuted() {
    let l = lv(0x3c);
    let g = liq(&l, FULL, LONG);
    let w = pend(&g, 930_000_000);
    // Bids around the market (9.3, 9.5) and up to one time below 9.6632 do not refute.
    for b in [930_000_000, 950_000_000, REF_LIQ_LONG - 1] {
        refused(&ch_draft(&[w.clone()], &bid(b, FULL), &fk(), 1), 0);
    }
    accept(&finalize_draft(&w, &fk(), 2));
}

/// The margin g at its boundary, for each of the four claims: the refutation quote is accepted, one time in the
/// claim's favour is refused.
#[test]
fn challenge_short_of_the_margin_by_one_price_is_refused() {
    let l = lv(0x3d);
    for (g, e, c) in claims(&l, FULL) {
        let w = pend(&g, e.price());
        accept(&ch_draft(&[w.clone()], &c, &fk(), 1));
        refused(&ch_draft(&[w.clone()], &one_price_short(&c), &fk(), 1), 0);
    }
    // The counter-evidence must be of the opposite kind: the same quote as an order of the claim's own kind is refused.
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    refused(&ch_draft(&[w], &ask(REF_LIQ_LONG, FULL), &fk(), 1), 0);
}

/// A close is refuted only by a quote that changes its payout: both quotes beyond the same bound compare as that bound
/// (DESIGN: clamped comparison), so a counter ask above bs does not refute a long's close at a bid above bs.
#[test]
fn challenge_of_a_close_compares_clamped_prices() {
    let l = lv(0x3e);
    let g = close(&l, FULL, LONG);
    let w = pend(&g, 1_500_000_000); // a bid at 15 KAS: the long gets its whole range
    refused(&ch_draft(&[w.clone()], &ask(1_300_000_000, FULL), &fk(), 1), 0);
    // An ask at 0.98 * bs does change the payout and refutes it.
    accept(&ch_draft(&[w], &ask(1_176_000_000, FULL), &fk(), 1));
}

/// Counter-evidence counts from the proposal's clock P (DESIGN 4.5d resolution 2): an order resting since long before
/// P qualifies once it has rested R from P; one placed after P counts from its own exposure.
#[test]
fn counter_evidence_counts_from_the_proposal_clock() {
    let l = lv(0x3f);
    let p_daa = 5_000_000;
    let w = pend_of(&liq(&l, FULL, LONG), LIQ_ASK, p_daa);
    let r = rest_daa();
    // Resting since DAA 1,000,000 (long before P).
    let old = bid(REF_LIQ_LONG, FULL).placed(1_000_000);
    let mut d = ch_draft(&[w.clone()], &old, &fk(), 1);
    d.lock = p_daa + r;
    accept(&d);
    d.lock = p_daa + r - 1;
    refused(&d, 0);
    // Placed after P: R from its own placement.
    let new = bid(REF_LIQ_LONG, FULL).placed(p_daa + 1_000);
    let mut d = ch_draft(&[w.clone()], &new, &fk(), 1);
    d.lock = p_daa + 1_000 + r;
    accept(&d);
    d.lock = p_daa + 1_000 + r - 1;
    refused(&d, 0);
}

/// Counter-evidence that rested before P but died before resting R on P, or with less than L of life left after its
/// rest on P, is refused; with exactly that life it is accepted (minEvidenceLife applies to counter-evidence too).
#[test]
fn counter_evidence_dead_or_short_lived_on_the_proposal_clock_is_refused() {
    let l = lv(0x40);
    let p_daa = 5_000_000;
    let w = pend_of(&liq(&l, FULL, LONG), LIQ_ASK, p_daa);
    let r = rest_daa() as i64;
    let life = p_daa as i64 + r + market().min_evidence_life;
    for (expiry, good) in [(life, true), (life - 1, false), (p_daa as i64 + r - 1, false), (p_daa as i64 - 1, false)] {
        let mut e = bid(REF_LIQ_LONG, FULL).placed(1_000_000);
        e.bid.expiry_daa = expiry;
        let mut d = ch_draft(&[w.clone()], &e, &fk(), 1);
        d.lock = (p_daa as i64 + r) as u64;
        if good {
            accept(&d);
        } else {
            refused(&d, 0);
        }
    }
}

/// The window is not a deadline (DESIGN 4.5d resolution 5): a challenge long after W, while nobody has finalized, is
/// accepted; the CSV of finalize is a lower bound only.
#[test]
fn challenge_after_the_window_but_before_finalize_is_accepted() {
    let l = lv(0x41);
    for (g, e, c) in claims(&l, FULL) {
        let w = pend_of(&g, e.price(), NOW - window() * 50);
        accept(&ch_draft(&[w], &c, &fk(), 1));
    }
}

#[test]
fn challenge_restored_value_or_state_wrong_is_refused() {
    let l = lv(0x42);
    for n in [FULL, HALF] {
        for (g, e, c) in claims(&l, n) {
            let w = pend(&g, e.price());
            let d = ch_draft(&[w.clone()], &c, &fk(), 1);
            accept(&d);
            let mut x = d.clone();
            x.outs[0].value -= 1;
            refused(&x, 0);
            // The restored state keeps S exactly: a restart (since 0), a clock moved forwards or backwards by one, or a
            // key changed is refused.
            let r = d.pairs[0].1.clone();
            assert_eq!(r.since, l.clock());
            for m in 0..4 {
                let mut y = r.clone();
                match m {
                    0 => y.since = 0,
                    1 => y.since = l.clock() + 1,
                    2 => y.since = l.clock() - 1,
                    _ => y.short_key = xonly(&keypair(13)),
                }
                let mut x = d.clone();
                x.set_pair(0, &y);
                refused(&x, 0);
            }
        }
    }
}

#[test]
fn challenge_fill_must_cover_kc_times_the_parts() {
    let l = lv(0x43);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    accept(&ch_draft(&[w.clone()], &bid(REF_LIQ_LONG, FULL), &fk(), 1));
    refused(&ch_draft(&[w], &bid(REF_LIQ_LONG, FULL - 1), &fk(), 1), 0);
    // k_c = 1.5 (independent of k = 1): the counter print must cover 1.5 n.
    let mut p = base();
    p.m.kc_bps = 15_000;
    let l = live(&p, 0x44).at(1_000);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    accept(&ch_draft(&[w.clone()], &bid(REF_LIQ_LONG, 1_500_000_000), &fk(), 1));
    refused(&ch_draft(&[w], &bid(REF_LIQ_LONG, 1_499_999_999), &fk(), 1), 0);
}

/// The proposer challenging itself (DESIGN 4.5d section 5c): it pays the counter fill and gets its own bond back as the
/// executor; the part returns restored, so the only external effect is the freeze while pending.
#[test]
fn proposer_challenging_itself_gets_only_its_own_bond_back() {
    let l = lv(0x45);
    let g = liq(&l, FULL, LONG);
    let w = pend(&g, LIQ_ASK);
    let d = ch_draft(&[w.clone()], &bid(REF_LIQ_LONG, FULL), &proposer_kp(), 1);
    let tx = accept(&d);
    assert_eq!(d.outs[0].value as i64, w.value as i64 - w.w.bond);
    let change = tx.outputs[d.change.unwrap()].value as i64;
    let mut control = d.clone();
    control.outs[0].value += w.w.bond as u64;
    let (ctx, _) = control.build();
    assert_eq!(change - ctx.outputs[d.change.unwrap()].value as i64, w.w.bond, "the bond lands in the challenger's change");
}

/// One counter print refutes several pending parts in one block, and one print may serve a proposal and a challenge
/// at once: an ask that liquidates one pair's long and refutes another part's close of the long.
#[test]
fn one_counter_print_serves_a_block() {
    let (a, b, c) = (lv(0x46), lv(0x47), lv(0x48));
    let wa = pend(&liq(&a, FULL, LONG), LIQ_ASK);
    let wb = pend(&liq(&b, HALF, LONG), LIQ_ASK);
    accept(&ch_draft(&[wa.clone(), wb.clone()], &bid(REF_LIQ_LONG, FULL + HALF), &fk(), 1));
    let d = ch_draft(&[wa, wb], &bid(REF_LIQ_LONG, FULL + HALF - 1), &fk(), 1);
    refused(&d, 0);
    refused(&d, 1);
    // A proposal and a challenge on one ask at 9.40: it liquidates c's long and refutes a close of b's long at 10.5.
    let wc = pend(&close(&b, FULL, LONG), CL_BID);
    let m = vec![Member::P(liq(&c, FULL, LONG)), Member::W(PendLeg { w: wc, q: 0 })];
    accept(&blk_draft(&m, &ask(LIQ_ASK, 2 * FULL), &fk(), 2));
    refused(&blk_draft(&m, &ask(LIQ_ASK, 2 * FULL - 1), &fk(), 2), 0);
}

/// A pending part and the remainder of its split share the covenant id; no pending entry may be spent with another
/// input of its id (the single-input guard of every entry).
#[test]
fn pending_part_and_its_remainder_cannot_be_co_spent() {
    let l = lv(0x49);
    let g = liq(&l, HALF, LONG);
    let d = ev_draft(&[g.clone()], &ask(LIQ_ASK, HALF), &fk(), 1);
    let tx = accept(&d);
    let w = live_pendings(&tx, &d).remove(0).at(NOW - rest_daa() - 10);
    let rest = live_pairs(&tx, &d).remove(0);
    assert_eq!(w.id, rest.id);
    // Challenge of the pending part next to a liquidation proposal of the remainder on the same ask print.
    let m = vec![Member::W(PendLeg { w: w.clone(), q: 0 }), Member::P(liq(&rest, HALF, LONG))];
    let dd = blk_draft(&m, &bid(REF_LIQ_LONG, 2 * HALF), &fk(), 2);
    let (t2, e2) = dd.build();
    let r = run(&t2, &e2).expect("covenant context");
    assert!(r[0].is_err() && r[1].is_err(), "two inputs of one covenant id are refused");
    // Finalize next to an amend of the remainder.
    let mut fd = finalize_draft(&w, &fk(), 3);
    let ad = amend_draft(&rest, &lk(), LONG, (1_000_000_000, 0, 0), &fk(), 4);
    fd.ins.insert(1, ad.ins[0].clone());
    fd.outs.insert(1, ad.outs[0].clone());
    fd.outs[1].covenant = bind(1, rest.id);
    fd.seqs = vec![window(), 0, 0];
    if let Some(c) = fd.change.as_mut() {
        *c += 1;
    }
    fd.pairs.clear();
    let (t3, e3) = fd.build();
    let r = run(&t3, &e3).expect("covenant context");
    assert!(r[0].is_err() && r[1].is_err());
}

// ================================================================ cure

#[test]
fn cure_exactly_at_and_one_price_short_of_the_bound() {
    let l = lv(0x4a);
    for n in [FULL, HALF] {
        // DESIGN 4.5e: the cure bound clears the claim by g (before review 4: 8.93 and 12.075).
        for (side, e, cb) in [(LONG, ask(LIQ_ASK, n), 875_140_000), (SHORT, bid(LIQ_BID, n), 1_231_650_000)] {
            let g = liq(&l, n, side);
            let w = pend(&g, e.price());
            assert_eq!(w.w.cure_bound, cb);
            let signer = if side == LONG { lk() } else { sk() };
            let d = cure_draft(&w, &signer, &fk(), 1);
            accept(&d);
            // One signed price short of the bound (less margin than needed): the cured state does not match.
            let mut c = d.pairs[0].1.clone();
            if side == LONG {
                c.bl += 1;
            } else {
                c.bs -= 1;
            }
            let mut x = d.clone();
            x.set_pair(1, &c);
            refused(&x, 0);
            // The added margin or the refund one sompi short.
            for j in [0, 1] {
                let mut x = d.clone();
                x.outs[j].value -= 1;
                refused(&x, 0);
            }
            // The other owner, or anyone else, cannot cure.
            for wrong in [if side == LONG { sk() } else { lk() }, fk()] {
                refused(&cure_draft(&w, &wrong, &fk(), 1), 0);
            }
        }
    }
}

/// The cured part keeps all the value of the pending part but the refund, plus the added margin: when n = s that
/// includes the pair's carrier and any excess, which the curer cannot take (DESIGN deviation in KpfPending.cure).
#[test]
fn cure_keeps_the_excess_and_moves_exactly_the_margin() {
    let mut l = lv(0x4b);
    l.value += 777_000_000; // 7.77 KAS above the floor
    let g = liq(&l, FULL, LONG);
    let w = pend(&g, LIQ_ASK);
    let d = cure_draft(&w, &lk(), &fk(), 1);
    accept(&d);
    let add = q(FULL, l.p.bl - 875_140_000, SCALE, SCALE - 1);
    assert_eq!(d.outs[1].value as i64, l.value as i64 + add);
    assert_eq!(d.outs[0].value as i64, w.w.bond);
}

#[test]
fn cure_of_a_close_or_of_a_claim_without_cure_is_refused() {
    let l = lv(0x4c);
    let w = pend(&close(&l, FULL, LONG), CL_BID);
    let mut d = finalize_draft(&w, &fk(), 1);
    d.ins[0] = pending_input(&w, "cure", vec![Arg::Sig(sk())]);
    d.seqs = vec![];
    refused(&d, 0);
    // A short liquidated at a bid whose cure bound would exceed priceMax: the claim has no cure. One token, so the
    // range of about 10^14 sompi stays fundable.
    let mut p = base();
    p.size = MIN_SIZE;
    p.bs = PRICE_MAX - 10_000_000;
    p.bl = 900_000_000;
    let l = live(&p, 0x4d).at(1_000);
    let g = liq(&l, MIN_SIZE, SHORT);
    let w = pend(&g, PRICE_MAX);
    assert_eq!((w.w.cure_bound, w.w.cure_hash), (0, [0; 32]));
    accept(&ev_draft(&[g], &bid(PRICE_MAX, MIN_SIZE), &fk(), 2));
    let mut d = finalize_draft(&w, &fk(), 3);
    d.ins[0] = pending_input(&w, "cure", vec![Arg::Sig(sk())]);
    d.seqs = vec![];
    refused(&d, 0);
    accept(&finalize_draft(&w, &fk(), 3));
}

/// After a cure (Y9), no quote within g of the claim liquidates the cured pair; a fresh quote beyond g does. Premise
/// changed by DESIGN 4.5e (review 4, F3): before, the cure cleared only the claim's own quote and one sompi lower
/// reopened it.
#[test]
fn cured_pair_is_safe_within_g_of_the_claimed_quote_only() {
    let l = lv(0x4e);
    let g = liq(&l, FULL, LONG);
    let w = pend(&g, LIQ_ASK);
    let d = cure_draft(&w, &lk(), &fk(), 1);
    let tx = accept(&d);
    let cured = live_pairs(&tx, &d).remove(0).at(NOW - 2 * rest_daa());
    assert_eq!(cured.p.since, l.clock(), "a cure keeps the clock S");
    let reach = kpf_protocol::pending::cure_reach_of(&l.p.m, LONG, LIQ_ASK);
    let e = ask(LIQ_ASK, FULL).placed(2_000);
    for price in [LIQ_ASK, LIQ_ASK - 1, reach] {
        let mut x = e.clone();
        x.ask.price = price;
        refused(&ev_draft(&[liq(&cured, FULL, LONG)], &x, &fk(), 2), 0);
    }
    let mut lower = e.clone();
    lower.ask.price = reach - 1;
    accept(&ev_draft(&[liq(&cured, FULL, LONG)], &lower, &fk(), 2));
}

// ================================================================ finalize

#[test]
fn finalize_one_daa_early_is_refused() {
    let l = lv(0x4f);
    for (g, e, _) in claims(&l, FULL) {
        let w = pend(&g, e.price());
        let d = finalize_draft(&w, &fk(), 1);
        accept(&d);
        let mut x = d.clone();
        x.seqs = vec![window() - 1, 0];
        refused(&x, 0);
    }
}

#[test]
fn finalize_payouts_one_sompi_short_are_refused() {
    let l = lv(0x50);
    for n in [FULL, HALF] {
        for (g, e, _) in claims(&l, n) {
            let w = pend(&g, e.price());
            let d = finalize_draft(&w, &fk(), 1);
            accept(&d);
            let paid: Vec<usize> = (0..d.outs.len()).filter(|&j| Some(j) != d.change && d.outs[j].value > 0).collect();
            for j in paid {
                let mut x = d.clone();
                x.outs[j].value -= 1;
                refused(&x, 0);
            }
        }
    }
}

/// A close's finalize: the closer gets its payoff at the clamped quote plus the refund; when the non-actor's share is
/// below dust it goes to the closer (a plain payout below 0.02 KAS is not created).
#[test]
fn finalize_close_dust_goes_to_the_closer() {
    let l = lv(0x51);
    // A long's close at a bid just below bs: the short is owed q(n, bs - b) = 0.005 KAS.
    let b = l.p.bs - 1_000;
    let g = close(&l, HALF, LONG);
    let w = pend(&g, b);
    let (lp, sp) = w.w.finalize_close(w.value);
    assert_eq!((lp, sp), (w.value as i64, 0));
    accept(&finalize_draft(&w, &fk(), 1));
}

/// Forfeit and keep (DESIGN 4.5d resolution 3), then the winner closes the forfeited part with its own two signatures.
#[test]
fn forfeit_and_keep_then_the_winner_closes_with_two_signatures() {
    let l = lv(0x52);
    for (side, e) in [(LONG, ask(LIQ_ASK, FULL)), (SHORT, bid(LIQ_BID, FULL))] {
        let w = pend(&liq(&l, FULL, side), e.price());
        let d = finalize_draft(&w, &fk(), 1);
        let tx = accept(&d);
        let fp = live_pairs(&tx, &d).remove(0).at(NOW + 7_000);
        let winner = if side == LONG { sk() } else { lk() };
        let ins = [
            pair_input(&fp, "coopClose", vec![Arg::Sig(winner), Arg::Sig(winner)]),
            funder(&fk(), 2, F_FUNDS),
        ];
        let (tx, en) = build(&ins, vec![out(fp.value, p2pk_spk(&winner), None), out(F_FUNDS - FEE, p2pk_spk(&fk()), None)]);
        assert_accepted(&tx, &en);
        // The loser's key no longer closes it.
        let loser = if side == LONG { lk() } else { sk() };
        let ins = [pair_input(&fp, "coopClose", vec![Arg::Sig(loser), Arg::Sig(winner)]), funder(&fk(), 3, F_FUNDS)];
        let (tx, en) = build(&ins, vec![out(fp.value, p2pk_spk(&loser), None), out(F_FUNDS - FEE, p2pk_spk(&fk()), None)]);
        assert_input_fails(&tx, &en, 0);
    }
}

/// Forfeit and keep, then the winner sells the forfeited side through an ordinary listing and a transfer to a resting
/// KpfOpen (P3), after the hand-over limit S + 4R of the forfeited part's fresh clock.
#[test]
fn forfeit_and_keep_then_the_winner_sells_the_side() {
    let l = lv(0x53);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let d = finalize_draft(&w, &fk(), 1);
    let tx = accept(&d);
    let t0 = NOW + 7_000;
    let fp = live_pairs(&tx, &d).remove(0).at(t0);
    // The winner (the short) lists the long side it now owns.
    let x = 1_000_000_000;
    let ad = amend_draft(&fp, &sk(), LONG, (x, 0, 0), &fk(), 2);
    let tx = accept(&ad);
    let listed = live_pairs(&tx, &ad).remove(0).at(t0 + 10);
    let lo = {
        let o = long_order(&keypair(21));
        LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([0x71; 32]), op: outpoint(0x71) }
    };
    let t = TransferSpec { side: LONG, n: FULL, x, nb: x - lo.o.my_rate, ek: lo.o.owner, et: lo.id, k: 0 };
    let td = transfer_draft(&listed, &t, &Entrant::Open(lo), &fk(), 3);
    assert_eq!(td.lock, t0 + 4 * rest_daa());
    let tx = accept(&td);
    // The winner was paid as the leaver and keeps its short; the entrant holds the long.
    assert_eq!(td.outs[0].script_public_key, p2pk_x(&l.p.short_key));
    let part = live_pairs(&tx, &td).into_iter().find(|p| p.p.long_key == xonly(&keypair(21))).unwrap();
    assert_eq!((part.p.short_key, part.p.bs), (l.p.short_key, l.p.bs));
    // One DAA before the hand-over limit: refused.
    let mut early = td.clone();
    early.lock -= 1;
    refused(&early, 0);
}

// ================================================================ actions against a pending part

/// The pending part has only challenge, cure, finalize and (DESIGN 4.5f) guarantee (DESIGN 4.5d section 5e): a second
/// proposal, a transfer, an
/// amend, an add margin, a close-versus-close half and an idle settle all spend it with the pair's script, which is not
/// its script; and its own ABI has no such entry. The remainder of the same split is an ordinary pair and keeps them.
#[test]
fn pair_paths_against_a_pending_part_are_refused() {
    let entries: Vec<String> = kpf_protocol::artifacts::pending_base().contracts.values().next().unwrap().entries.keys().cloned().collect();
    assert_eq!(entries, vec!["challenge".to_string(), "cure".to_string(), "finalize".to_string(), "guarantee".to_string()]);
    let l = lv(0x54);
    let g = liq(&l, HALF, LONG);
    let d = ev_draft(&[g.clone()], &ask(LIQ_ASK, HALF), &fk(), 1);
    let tx = accept(&d);
    let w = live_pendings(&tx, &d).remove(0).at(NOW);
    let rest = live_pairs(&tx, &d).remove(0).at(NOW);
    // The pair's entries on the pending UTXO: a pair state of the part as the redeem script.
    let as_pair = Live { p: w.w.restore_pair().unwrap(), value: w.value, id: w.id, op: w.op, daa: w.daa };
    let pending_spk = p2sh(&w.w.art());
    let mk = |dd: Draft| {
        let mut dd = dd;
        dd.ins[0].utxo.script_public_key = pending_spk.clone();
        dd
    };
    // A second proposal (a close by the long on a bid).
    let pd = ev_draft(&[close(&as_pair, HALF, LONG)], &bid(CL_BID, HALF), &fk(), 2);
    refused(&mk(pd), 0);
    // Amend, add margin, idle settle.
    refused(&mk(amend_draft(&as_pair, &lk(), LONG, (1_000_000_000, 0, 0), &fk(), 3)), 0);
    let mut idle = idle_draft(&as_pair, &fk(), 4);
    idle.ins[0].utxo.script_public_key = pending_spk.clone();
    refused(&idle, 0);
    // A transfer by a direct taker.
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    refused(&mk(transfer_draft(&as_pair, &t, &Entrant::Direct(keypair(16)), &fk(), 5)), 0);
    // Add margin by the threatened owner (only a cure moves its bound while pending).
    let delta = 50_000_000;
    let mut m = as_pair.p.clone();
    m.bl -= delta;
    let need = q(HALF, delta, SCALE, SCALE - 1) as u64;
    let ins = vec![pair_input(&as_pair, "addMargin", vec![Arg::Sig(lk()), iv(LONG), iv(delta)]), funder(&fk(), 7, F_FUNDS)];
    let outs = vec![out(w.value + need, p2sh(&m.art()), bind(0, w.id)), out(0, p2pk_spk(&fk()), None)];
    refused(&mk(Draft::new(ins, outs, Some(1))), 0);
    // Close-versus-close with the pending part as the host (its long listed in the pair state it presents).
    let mut hp = as_pair.clone();
    (hp.p.long_ask, hp.p.long_min) = (1_000_000_000, 0);
    let mut dp = base();
    (dp.bl, dp.bs, dp.short_bid, dp.size, dp.long_key, dp.short_key) = (850_000_000, 1_150_000_000, 1_050_000_000, HALF, xonly(&keypair(17)), xonly(&keypair(18)));
    let donor = live(&dp, 0x5f).at(1_000);
    let cvc = close_draft(&hp, &donor, &CloseSpec { n: HALF, x: 1_020_000_000, e3: 1_000_000_000 }, &fk(), 8);
    refused(&mk(cvc), 0);
    // The pending part's own finalize on the remainder's script is refused too, and the remainder keeps every pair path.
    let mut amend_rest = amend_draft(&rest, &lk(), LONG, (1_000_000_000, 0, 0), &fk(), 6);
    accept(&amend_rest);
    amend_rest.ins[0] = pending_input(&w, "finalize", vec![]);
    amend_rest.seqs = vec![window(), 0];
    let (t, e) = amend_rest.build();
    assert!(run(&t, &e).map(|r| r[0].is_err()).unwrap_or(true));
}

// ================================================================ bonds

/// Every bond ends with exactly one of the proposer or the challenger, in every branch: the proposer funds it at
/// propose; a challenge leaves it to the challenger (the executor's change, nothing to the proposer); a cure repays
/// the proposer `refund`; a finalize pays it with the fee (a liquidation) or with the closer's payout (a close).
#[test]
fn bond_conservation_in_every_branch() {
    let l = lv(0x55);
    let pk = xonly(&proposer_kp());
    for n in [FULL, HALF] {
        for (g, e, c) in claims(&l, n) {
            let d = ev_draft(&[g.clone()], &e, &fk(), 1);
            accept(&d);
            let w = pend(&g, e.price());
            let b = w.w.bond;
            // Propose: the pending part holds the bond on top of what it came from.
            let from = if n == FULL { l.value as i64 } else { l.p.range_of(n) + C };
            assert_eq!(d.outs[0].value as i64 - from, b);
            // Challenge: the restored pair holds the value less the bond, no output pays the proposer.
            let cd = ch_draft(&[w.clone()], &c, &fk(), 2);
            accept(&cd);
            assert_eq!(cd.outs[0].value as i64, w.value as i64 - b);
            let closer = if w.w.kind == KIND_CLOSE { Some(w.w.proposer_key) } else { None };
            assert!(cd.outs.iter().all(|o| o.script_public_key != p2pk_x(&pk) && closer.map_or(true, |k| o.script_public_key != p2pk_x(&k) || o.value as i64 != b)));
            // Finalize.
            let fd = finalize_draft(&w, &fk(), 3);
            accept(&fd);
            if w.w.kind == KIND_LIQ {
                assert_eq!(fd.outs[0].value as i64, b + w.w.fee);
                assert_eq!(fd.outs[0].script_public_key, p2pk_x(&pk));
                // Cure: the proposer gets exactly its refund (the bond, and the carrier it funded for a part).
                let signer = if w.w.side == LONG { lk() } else { sk() };
                let ud = cure_draft(&w, &signer, &fk(), 4);
                accept(&ud);
                assert_eq!(ud.outs[0].value as i64, w.w.refund);
                assert_eq!(w.w.refund, b + if n == FULL { 0 } else { C });
            } else {
                let (lp, sp) = w.w.finalize_close(w.value);
                let closer_pay = if w.w.side == LONG { lp } else { sp };
                let owe = closer_pay - w.w.refund;
                assert!(owe >= 0 && owe <= l.p.range_of(n));
            }
        }
    }
}

// ================================================================ blocks, boundaries, overflow

#[test]
fn several_proposals_on_one_print() {
    let legs: Vec<PairLeg> = (0..3).map(|i| liq(&lv(0x60 + i), FULL, LONG)).collect();
    accept(&ev_draft(&legs, &ask(LIQ_ASK, 3 * FULL), &fk(), 1));
    let d = ev_draft(&legs, &ask(LIQ_ASK, 3 * FULL - 1), &fk(), 1);
    for i in 0..3 {
        refused(&d, i);
    }
    let legs: Vec<PairLeg> = (0..8).map(|i| liq(&lv(0x68 + i), HALF, LONG)).collect();
    accept(&ev_draft(&legs, &ask(LIQ_ASK, 8 * HALF), &fk(), 2));
}

/// The largest pairs the contracts accept (scale 10^9, priceMax 10^14): propose, challenge and finalize do not overflow
/// for any pair whose value fits the KAS supply (v + bond <= 2 * 2.9 * 10^18 < 2^63).
#[test]
fn boundary_values_do_not_overflow() {
    let mut p = base();
    p.m.scale = 1_000_000_000;
    p.m.min_size = 1_000_000_000;
    p.m.price_max = PRICE_MAX;
    // 20,000 whole tokens over a range of 10^14 - 2 * 10^13: about 1.6 * 10^18 sompi.
    p.size = 20_000 * 1_000_000_000;
    (p.bl, p.bs, p.entry) = (10_000_000_000_000, 90_000_000_000_000, 50_000_000_000_000);
    let l = live(&p, 0x70).at(1_000);
    let n = p.size;
    let quote = 9_000_000_000_000;
    // The executor funds the bond (3 % of the range).
    let g = PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side: LONG } };
    let po = propose_of(&g, quote);
    assert!(po.pending_value > 1_000_000_000_000_000_000);
    let mut e = ask(quote, n);
    e.ask.scale = p.m.scale;
    let mut d = ev_draft(&[g.clone()], &e, &fk(), 1);
    let fi = d.change.unwrap();
    d.ins[fi].utxo.amount = 4_000_000_000_000_000_000;
    d.outs[0].value = po.pending_value as u64;
    let (tx, en) = d.build();
    let r = run(&tx, &en).expect("covenant context");
    assert!(r[0].is_ok(), "propose of the largest fundable pair: {:?}", r[0]);
    let w = pend(&g, quote);
    let mut fd = finalize_draft(&w, &fk(), 2);
    fd.ins[1].utxo.amount = F_FUNDS;
    let (tx, en) = fd.build();
    assert!(run(&tx, &en).unwrap()[0].is_ok());
}
