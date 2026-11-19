//! Review 12, lead 4: what the economic walk (`econ_walk_tests.rs`) never produces, run over random pairs of the testnet
//! market in the engine. For each seed, random pairs (size, entry, each side's leverage, cover rate between the range and
//! 1.5 times it, the feed within both maintenance bands) and, for each pair:
//!
//! - a false close by each party at the price that pays it the whole range (the walk's false closes are look-back
//!   prices within 2 g of the feed, and every false claim of the walk is contested at once, or fails only through the
//!   judge): what it takes from the counterparty at the feed against the bond it risks, and the break-even contest
//!   probability `take / (take + bond)`;
//! - the E-exit probe as 29.1 states it ("landing as soon as the pair UTXO is quietDaa old"), with the counterparty's
//!   amend landing at that same DAA first (the walk's probe forges the pair's DAA score to be quiet-old and nothing ever
//!   races it): the close one DAA early is refused, and the amend restarts the clock;
//! - a true close contested by the counterparty with its own stake and not defended: the closer's bond to the
//!   counterparty (the walk's watcher contests only false claims and always wins).
//!
//! Seeds: `R12_SEEDS=a..b` (default 1..4), 60 pairs a seed.

mod common;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f64 {
        (self.next() % 1_000_000) as f64 / 1_000_000.0
    }
}

fn uop(tag: u32) -> TransactionOutpoint {
    let mut b = [0x9e; 32];
    b[..4].copy_from_slice(&tag.to_le_bytes());
    TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: tag }
}

fn close_seq(lp: &LPair, side: i64, p: i64, t0: i64, lock: u64, closer: &Keypair, seq: u64) -> T {
    let k = kit();
    let n = lp.s.size;
    let (w, fb) = lp.s.pending_of(k, n, CLOSE, side, p, t0).expect("a close the model accepts");
    let w = w.made_by(&lp.op);
    let args = vec![iv(n), iv(CLOSE), iv(side), iv(p), iv(t0), iv(1), Arg::Sig(*closer), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let fv: i64 = 1_000_000_000_000_000;
    let ins = vec![pair_in(lp, "propose", args), key_in(uop(0x7000_0000 + lp.op.index), fv as u64, 0, closer)];
    let pv = lp.s.pending_value(lp.value, &w);
    let mut outs = vec![out(pv as u64, pend_spk(&w), None), out((fv - (pv - lp.value as i64) - 10_000) as u64, p2pk_spk(closer), None)];
    place(&mut outs, 0, 0, lp.op, lp.id.unwrap(), Lineage::Fresh);
    build_ext(&ins, outs, lock, &[seq, 0])
}

#[test]
fn r12_bounded_random_pairs_false_closes_exit_races_and_undefended_contests() {
    let (a, b) = std::env::var("R12_SEEDS").ok().and_then(|s| s.split_once("..").map(|(x, y)| (x.parse().unwrap(), y.parse().unwrap()))).unwrap_or((1u64, 4u64));
    let m = OMarket::testnet(feed_hash_of(b"review 12: random pairs"));
    let (sc, g, mm, quiet, w_) = (m.p.scale, m.p.g_bps, m.p.mmr_bps, m.p.quiet_daa as u64, m.p.challenge_daa);
    let (mut be_min, mut be_max, mut pairs, mut races, mut undefended) = (1.0f64, 0.0f64, 0u32, 0u32, 0i64);
    let mut taken_total = 0i64;
    let mut tag = 0u32;
    for seed in a..=b {
        let mut r = Rng(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x2545_f491_4f6c_dd1d));
        for _ in 0..60 {
            tag += 1;
            let size = (1_000_000 + r.next() % 100_000_000) as i64;
            let e = ((1_000.0 + r.unit() * 199_000.0) * 1e8) as i64;
            let (ll, ls) = (1.5 + r.unit() * 48.5, 1.5 + r.unit() * 48.5);
            let bl = (e as f64 * (1.0 - 1.0 / ll)) as i64;
            let bs = (e as f64 * (1.0 + 1.0 / ls)) as i64;
            let cr = ((bs - bl) as f64 * (1.0 + r.unit() * 0.5)) as i64;
            let p = OPair {
                m,
                size,
                bl,
                bs,
                entry: e,
                long_key: x(&long_kp()),
                short_key: x(&short_kp()),
                b: OBinding { uw_id: [0x5e; 32], uw_key: x(&uw_kp()), cover_rate: cr, expiry: 100 * m.bond.step, premium: 0 },
                long_ask: 0,
                long_min: 0,
                long_tip: 0,
                short_bid: 0,
                short_min: 0,
                short_tip: 0,
                mk: Makers::default(),
            };
            // A feed at which both sides can close honestly at the feed (within their bands by g).
            let (lo_f, hi_f) = (bl as f64 / ((1.0 - mm as f64 / 1e4) * (1.0 - g as f64 / 1e4)) * 1.001, bs as f64 / ((1.0 + mm as f64 / 1e4) * (1.0 + g as f64 / 1e4)) * 0.999);
            if lo_f >= hi_f {
                continue;
            }
            let f = (lo_f + r.unit() * (hi_f - lo_f)) as i64;
            let d0 = 50_000_000u64;
            let lp = genesis_pair(p.clone(), p.floor() as u64, 0, d0);
            let lp = Live { op: uop(tag), id: Some(kaspa_consensus_core::Hash::from_bytes({
                let mut h = [0x5f; 32];
                h[..4].copy_from_slice(&tag.to_le_bytes());
                h
            })), ..lp };
            let now = d0 + quiet;
            pairs += 1;
            // A false close of each side at the price that pays it the whole range.
            for (side, kp) in [(L, long_kp()), (S, short_kp())] {
                let pf = if side == L { ((bs as i128 * 10_000 + 9_949) / (10_000 - g as i128)) as i64 } else { 1 };
                if !p.close_ok(side, pf) {
                    continue;
                }
                let t = close_seq(&lp, side, pf, now as i64, now, &kp, quiet);
                must("a false close paying the closer the range", &t);
                let w = p.pending_of(kit(), size, CLOSE, side, pf, now as i64).unwrap().0.made_by(&lp.op);
                let lw = live(&t, 0, now, &w);
                let (ft, s) = finalize_tx(&lw, now as i64, w_ as u64);
                must("finalize", &ft);
                let Settlement::Close { long, short } = s else { panic!() };
                let la = p.long_at(f);
                let (victim_fair, victim_got) = if side == L { (lp.value as i64 - la, short) } else { (la, long) };
                let take = victim_fair - victim_got;
                assert!(take >= 0);
                let be = take as f64 / (take + w.bond) as f64;
                taken_total += take;
                if take > 0 {
                    be_min = be_min.min(be);
                    be_max = be_max.max(be);
                }
            }
            // E-exit with the counterparty's amend at the same DAA.
            let (at, a2) = amend_tx(&lp, S, &short_kp(), 1, quiet);
            must("the counterparty's amend at the boundary", &at);
            let amended = live(&at, 0, now, &a2);
            if p.close_ok(L, f) {
                must("the long's close at the boundary (conflicts with the amend)", &close_seq(&lp, L, f, now as i64, now, &long_kp(), quiet));
                refused("the long's close before the amended pair is quiet", &close_seq(&amended, L, f, now as i64, now, &long_kp(), quiet - 1), 0);
                races += 1;
            }
            // A true close contested by the counterparty, undefended: the closer's bond goes to it.
            let w = p.pending_of(kit(), size, CLOSE, L, f, now as i64).unwrap().0;
            let refuted = w.contested(now as i64, x(&short_kp())).decided(false, now as i64 + 1);
            let Settlement::Restore { defender, disputer, .. } = refuted.settlement(p.floor() as u64 + w.bond as u64, now as i64) else { panic!() };
            assert_eq!(defender + disputer, w.bond);
            undefended += w.bond;
            let _ = sc;
        }
    }
    println!(
        "{pairs} random pairs (seeds {a}..{b}): false closes took {} KAS; break-even contest probability from {:.1} % to {:.1} %; {races} exits raced and lost to an amend at the quiet boundary; undefended contests of true closes moved {} KAS of bonds to the counterparty",
        taken_total / KAS,
        be_min * 100.0,
        be_max * 100.0,
        undefended / KAS
    );
    // The bound: a false close takes at most the counterparty's value at the feed, at most the range <= the cover, and
    // the bond is sigma of the cover, so the break-even never exceeds 1 / (1 + sigma).
    assert!(be_min > 0.0 && be_max <= 1.0 / (1.0 + m.p.sigma_bps as f64 / 10_000.0) + 1e-9);
}
