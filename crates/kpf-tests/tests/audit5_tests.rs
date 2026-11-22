//! Review 5 (docs/audit/REVIEW-5.md): attacks on guaranteed fast settlement (DESIGN 4.5f) and its interaction with the
//! slow path, after the fixes of DESIGN 4.5g. Each test is a `holds_` (the engine refuses the attack, or the wronged party
//! is paid in full) or an `exploit_` (the attack works; who loses what is stated). The real KOB order and token contracts
//! run in every evidence transaction.
//!
//! Market and pair as in `kpf_guarantee_tests` (DESIGN 5 worked example: 1,000 tokens, bl 9, bs 12 KAS, lambda 1.1,
//! R 600, W 6,000, D 6,600, G 1,200, g 2 %, mmr 5 %).
//!
//! Review 5's three exploits are now holds: F1 `holds_the_restitution_is_the_cover_whoever_disputes_with_whichever_order`
//! (was `exploit_the_first_disputer_prices_the_restitution_at_its_own_boundary_order`), F2
//! `holds_a_look_alike_part_with_an_out_of_range_market_cannot_be_guaranteed` (was
//! `exploit_a_look_alike_part_with_an_out_of_range_market_takes_the_guarantors_lock`) with the two drift tests of the
//! shared market check; F3 is in `kpf-cli/tests/audit5_cli.rs`. The attacks on the fix itself (the wronged party is now
//! paid the whole cover) follow under "attacks on the fix".

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
fn gk() -> [u8; 32] {
    xonly(&guarantor_kp())
}

const FULL: i64 = 1_000_000_000;
const HALF: i64 = 500_000_000;
const LIQ_ASK: i64 = 940_000_000;
const LIQ_BID: i64 = 1_150_000_000;
const CL_BID: i64 = 1_050_000_000;
const CL_ASK: i64 = 1_100_000_000;
const REF_LIQ_LONG: i64 = 966_315_790;
const REF_LIQ_SHORT: i64 = 1_120_000_000;
const REF_CL_LONG: i64 = 1_029_000_000;
const REF_CL_SHORT: i64 = 1_122_000_000;
const KAS: i64 = 100_000_000;

fn lv_of(p: &Pair, tag: u8) -> Live {
    live(p, tag).at(1_000)
}

fn close_of(l: &Live, n: i64, side: i64) -> PairLeg {
    let signer = if side == LONG { lk() } else { sk() };
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

fn close_long(l: &Live) -> PairLeg {
    close_of(l, FULL, LONG)
}

fn liq_of(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn liq(l: &Live, side: i64) -> PairLeg {
    liq_of(l, FULL, side)
}

fn dispute(ls: &[LiveLk], e: &Evidence, f: &Keypair) -> Draft {
    let mut d = dp_draft(ls, e, f, 6);
    d.lock = d.lock.max(ls.iter().map(|l| l.daa).max().unwrap_or(0));
    d
}

/// An ask at `price` for `fill` tokens made by `who`, filled by `who` itself (its tokens return to it, its KAS too).
fn own_ask(who: &Keypair, price: i64, fill: i64) -> Evidence {
    let mut e = Evidence::ask(price, fill);
    e.ask = kpf::kob::ask_of(&market(), xonly(who), price, fill);
    e.maker = *who;
    e.counter = *who;
    e
}

/// A bid at `price` for `fill` tokens made by `who`, filled with `who`'s own tokens.
fn own_bid(who: &Keypair, price: i64, fill: i64) -> Evidence {
    let mut e = Evidence::bid(price, fill);
    e.bid = kpf::kob::bid_of(&market(), xonly(who), price);
    e.maker = *who;
    e.counter = *who;
    e
}

/// The four claims on the base pair: (leg, proposal quote, the counter quote at the refutation bound, one far beyond).
fn claims(l: &Live, n: i64) -> Vec<(PairLeg, i64, Evidence, Evidence)> {
    vec![
        (close_of(l, n, LONG), CL_BID, Evidence::ask(REF_CL_LONG, n), Evidence::ask(900_000_000, n)),
        (close_of(l, n, SHORT), CL_ASK, Evidence::bid(REF_CL_SHORT, n), Evidence::bid(1_200_000_000, n)),
        (liq_of(l, n, LONG), LIQ_ASK, Evidence::bid(REF_LIQ_LONG, n), Evidence::bid(1_200_000_000, n)),
        (liq_of(l, n, SHORT), LIQ_BID, Evidence::ask(REF_LIQ_SHORT, n), Evidence::ask(900_000_000, n)),
    ]
}

/// A guarantee of `w` by `owner` (funded by `f`), and its lock at F = P + G (a liquidation) or P + 1 (a close).
fn guaranteed_by(w: &LivePend, owner: &[u8; 32], f: &Keypair) -> (Draft, Transaction, LiveLk) {
    let mut d = guarantee_draft(w, owner, f, 5);
    if w.w.kind == KIND_LIQ {
        d.set_args(0, guarantee_args(2, owner));
    }
    let tx = accept(&d);
    let at = w.daa + if w.w.kind == KIND_LIQ { grace() } else { 1 };
    let lock = live_locks(&tx, &d).remove(0).at(at);
    (d, tx, lock)
}

/// What the wronged party held in the claimed part at price `p` before the claim: its side of the pair a refutation
/// restores (the loser of a liquidation, the other party of a close).
fn held_at(w: &LivePend, p: i64) -> i64 {
    let r = w.w.restore_pair().expect("restore state");
    let victim_side = if w.w.kind == KIND_LIQ { w.w.side } else { 3 - w.w.side };
    if victim_side == LONG {
        r.long_at(p)
    } else {
        r.short_at(p)
    }
}

/// What the fast settlement itself paid the wronged party.
fn settled_to_victim(w: &LivePend) -> i64 {
    if w.w.kind == KIND_LIQ {
        0
    } else {
        let (lp, sp) = w.w.guarantee_close(w.value);
        if w.w.side == LONG {
            sp
        } else {
            lp
        }
    }
}

fn price_grid() -> Vec<i64> {
    let mut g: Vec<i64> = (0..=300).map(|i| i * 10_000_000).collect();
    for b in [900_000_000, 1_200_000_000, REF_LIQ_LONG, REF_LIQ_SHORT, REF_CL_LONG, REF_CL_SHORT, CL_BID, CL_ASK, LIQ_ASK, LIQ_BID] {
        g.extend([b - 1, b, b + 1]);
    }
    g.push(PRICE_MAX);
    g
}

// ================================================================ F1: the restitution is the cover

/// F1 (High), fixed (DESIGN 4.5g). The restitution was priced at the counter order the first disputer chose, so a
/// party of the claim could dispute first with its own order at the refutation bound and leave the wronged party the
/// bound's value (210 KAS instead of 1,500 KAS on the close below, 663.16 KAS instead of 3,000 KAS on the liquidation),
/// the rest burned. Now every valid dispute pays the wronged party the whole cover, whoever disputes (a third party, a
/// party of the claim, the guarantor, the victim itself) and with whichever valid counter order (the market's, one at
/// the bound filled by the disputer itself, one far beyond).
///
/// Close: the long closes at a bid of 10.5 and is its own guarantor (lock 1,650 KAS); the short is paid 1,500.2 KAS at
/// once and 1,500 KAS by any dispute: 3,000.2 KAS, its whole side. The long that disputes with its own ask at 10.29 ends
/// with 1,590 - 1,650 + 150 = 90 KAS (its refund, as at finalize at the true price 9.00): it gains nothing, and the short
/// loses nothing. Liquidation: the long's at 9.40, guaranteed by the winner; every dispute pays the long 3,000 KAS.
#[test]
fn holds_the_restitution_is_the_cover_whoever_disputes_with_whichever_order() {
    // ---- close
    let l = lv_of(&pair(&lk(), &sk()), 0x61);
    let w = pend(&close_long(&l), CL_BID);
    let (_, _, lock) = guaranteed_by(&w, &xonly(&lk()), &lk());
    let (lp, sp) = w.w.guarantee_close(w.value);
    assert_eq!((lp, sp, lock.value as i64), (1_590 * KAS, 150_020_000_000, 1_650 * KAS));
    let disputes = [
        (Evidence::ask(900_000_000, FULL), fk()),
        (own_ask(&lk(), REF_CL_LONG, FULL), lk()),
        (own_ask(&sk(), REF_CL_LONG, FULL), sk()),
        (own_ask(&guarantor_kp(), 1_000_000_000, FULL), guarantor_kp()),
    ];
    for (e, who) in &disputes {
        let t = accept(&dispute(&[lock.clone()], e, who));
        assert_eq!(t.outputs[0].script_public_key, p2pk_x(&w.w.short_key));
        assert_eq!(t.outputs[0].value as i64, 1_500 * KAS, "the short is paid the cover at {}", e.price());
        assert_eq!(sp + t.outputs[0].value as i64, 300_020_000_000, "the short ends with its whole side");
        assert!(t.outputs.iter().all(|o| o.script_public_key != kpf_protocol::lock::burn_spk()), "nothing of the short's burns");
    }
    let long_net = lp - lock.value as i64 + lock.l.reward();
    assert_eq!(long_net, w.w.refund, "the long that disputes its own close gains nothing over finalize at 9.00");

    // ---- liquidation
    let l = lv_of(&pair(&lk(), &sk()), 0x62);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let (_, _, lock) = guaranteed_by(&w, &xonly(&sk()), &sk());
    let disputes = [
        (Evidence::bid(1_200_000_000, FULL), fk()),
        (own_bid(&sk(), REF_LIQ_LONG, FULL), sk()),
        (own_bid(&lk(), REF_LIQ_LONG, FULL), lk()),
        (Evidence::bid(1_000_000_000, FULL), proposer_kp()),
    ];
    for (e, who) in &disputes {
        let t = accept(&dispute(&[lock.clone()], e, who));
        assert_eq!(t.outputs[0].script_public_key, p2pk_x(&w.w.long_key));
        assert_eq!(t.outputs[0].value as i64, 3_000 * KAS, "the long is paid its whole range at {}", e.price());
    }
    // The winner as guarantor and disputer: it keeps the long's side (worth at most 3,000 KAS) and the reward, loses the
    // lock: at most 0.
    assert!(3_000 * KAS + lock.l.reward() - lock.value as i64 <= 0);
}

/// F1, at every price: for each of the four claims, whole and partial, the wronged party's restitution is the cover and
/// never less than what the claim took from it at any price of a grid over [0, priceMax] (its side's value there less
/// what the settlement paid it); with the settlement it ends with at least its side of the claimed part at every price.
/// The same amount is paid on the counter order at the bound and on one far beyond it.
#[test]
fn holds_the_restitution_never_falls_below_what_the_claim_took_at_any_price() {
    let l = lv_of(&pair(&lk(), &sk()), 0x67);
    let grid = price_grid();
    for n in [FULL, HALF, MIN_SIZE] {
        for (g, quote, at_bound, far) in claims(&l, n) {
            let w = pend(&g, quote);
            let lk_ = lock_of(&w, &gk(), w.daa + grace());
            assert_eq!(lk_.l.restitution(), lk_.l.cover);
            for &p in &grid {
                assert!(lk_.l.victim_pay() >= lk_.l.taken_at(p), "n {n}, kind {} side {}: H < what the claim took at {p}", w.w.kind, w.w.side);
                assert!(settled_to_victim(&w) + lk_.l.victim_pay() >= held_at(&w, p), "n {n}: the victim ends below its side at {p}");
            }
            if n == FULL {
                let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
                let a = accept(&dispute(&[lock.clone()], &at_bound, &fk()));
                let b = accept(&dispute(&[lock.clone()], &far, &fk()));
                assert_eq!(a.outputs[0], b.outputs[0], "the same restitution at the bound and far beyond it");
                assert_eq!(a.outputs[0].value as i64, lock.l.cover);
            }
        }
    }
}

// ================================================================ F2: the market check of guarantee

/// F2 (High), fixed (DESIGN 4.5g). A look-alike pending part (a genesis of the KpfPending template with consistent
/// restore, cure and forfeit hashes) whose market block has mmrBps = -20,000 and gBps = 20,000. Before: the tooling
/// called it undisputable, `KpfPending.guarantee` (which re-checked only lambda and D) settled it, and any ask above bs
/// then disputed the lock: the guarantor lost all 3,300 KAS. Now the library refuses the part (`check_pending` validates
/// the market: `check_guarantee` fails before anything is said about disputes) and the chain refuses the guarantee
/// (`KpfPending.checkMarket`, KpfOpen's bounds); each of the two values alone is refused too. The slow path of the
/// look-alike stays its own author's business. A real market's part with the same claim is guaranteed and is not
/// disputable by that ask.
#[test]
fn holds_a_look_alike_part_with_an_out_of_range_market_cannot_be_guaranteed() {
    for (mmr, g) in [(-20_000, 20_000), (-20_000, 200), (500, 20_000), (-1, 200), (500, 10_000)] {
        let mut p = pair(&lk(), &sk());
        p.m.mmr_bps = mmr;
        p.m.g_bps = g;
        assert!(p.m.validate().is_err(), "no real market has these parameters");
        let l = lv_of(&p, 0x63);
        let w = pend(&liq(&l, SHORT), LIQ_BID);
        let lw = kpf_protocol::tx::LivePending { w: w.w.clone(), value: w.value, id: w.id, op: w.op, daa: w.daa };
        assert!(w.w.hashes_match(), "its hashes match the pair its state describes");
        let err = kpf_protocol::evidence::check_guarantee(&lw, &gk()).expect_err("the guarantee builder refuses it");
        assert!(err.to_string().contains("market"), "{err}");
        assert!(kpf_protocol::evidence::check_pending(&lw).is_err(), "and so does every command that reads a pending part");
        refused(&guarantee_draft(&w, &gk(), &fk(), 5), 0);
    }
    // Control: a real market's part with the same claim.
    let attacker = keypair(66);
    let l2 = lv_of(&pair(&lk(), &sk()), 0x64);
    let w2 = pend(&liq(&l2, SHORT), LIQ_BID);
    assert!(w2.w.contradiction_bound().is_some());
    let (_, _, lock2) = guaranteed_by(&w2, &gk(), &fk());
    refused(&dispute(&[lock2], &own_ask(&attacker, 1_300_000_000, FULL), &attacker), 0);
}

/// The bound text of a `BEGIN market bounds` .. `END market bounds` block.
fn market_block(src: &str) -> String {
    let src = src.replace("\r\n", "\n");
    let a = src.find("// BEGIN market bounds").expect("BEGIN marker");
    let b = src.find("// END market bounds").expect("END marker");
    src[a..b].to_string()
}

fn constant(src: &str, name: &str) -> Option<String> {
    let key = format!("int constant {name} = ");
    src.lines().find_map(|l| l.trim().strip_prefix(&key).map(|v| v.trim_end_matches(';').to_string()))
}

/// F2, drift (DESIGN 4.5g "the bounds KpfOpen and KpfPair enforce"). `KpfPending.checkMarket` is the same text as
/// `KpfOpen.checkMarket`, and every constant it names has the same value in both contracts: neither can be relaxed or
/// tightened alone.
#[test]
fn holds_the_market_check_is_one_text_in_kpf_open_and_kpf_pending() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let open = std::fs::read_to_string(root.join("legacy/KpfOpen.sil")).unwrap();
    let pending = std::fs::read_to_string(root.join("legacy/KpfPending.sil")).unwrap();
    let (a, b) = (market_block(&open), market_block(&pending));
    assert_eq!(a, b, "KpfOpen.checkMarket and KpfPending.checkMarket differ");
    assert!(a.contains("function checkMarket()"));
    let mut names: Vec<String> = a
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.len() > 2 && w.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()) && w.chars().next().unwrap().is_ascii_uppercase())
        .map(String::from)
        .collect();
    names.sort();
    names.dedup();
    let names: Vec<String> = names.into_iter().filter(|n| constant(&open, n).is_some()).collect();
    assert!(names.len() >= 9, "the block names its constants: {names:?}");
    for n in &names {
        assert_eq!(constant(&open, n), constant(&pending, n), "constant {n} differs between KpfOpen and KpfPending");
    }
    // Both contracts call it on the paths that rely on it.
    assert!(open.contains("checkMarket();") && pending.contains("checkMarket();"));
}

/// The test market's orders on market `m` (rates of 6 and 12 KAS in a market of high maintenance, as in
/// `audit_tag_tests`), each escrow 10 KAS above what a fill spends (so a market with a negative carrier still builds).
fn orders_on(m: &Market) -> (LiveOpen, LiveOpen) {
    let mut lo = long_order(&keypair(21));
    lo.m = m.clone();
    let mut so = short_order(&keypair(22));
    so.m = m.clone();
    if m.mmr_bps > 1_000 {
        (lo.my_rate, so.my_rate) = (600_000_000, 1_200_000_000);
    }
    (
        LiveOpen { value: lo.escrow() as u64 + 1_000_000_000, o: lo, id: Hash::from_bytes([0x81; 32]), op: outpoint(0x81) },
        LiveOpen { value: so.escrow() as u64 + 1_000_000_000, o: so, id: Hash::from_bytes([0x82; 32]), op: outpoint(0x82) },
    )
}

/// Market blocks one field off the test market, inside and outside every bound of the shared check.
fn market_variants() -> Vec<(String, Market)> {
    let m = market();
    let mut v = vec![("the test market".to_string(), m.clone())];
    let mut add = |name: &str, f: &dyn Fn(&mut Market)| {
        let mut x = m.clone();
        f(&mut x);
        v.push((name.to_string(), x));
    };
    // Outside.
    add("scale above 10^9", &|x| x.scale = 1_000_000_001);
    add("priceMax 0", &|x| x.price_max = 0);
    add("priceMax above 10^14", &|x| x.price_max = PRICE_MAX + 1);
    add("carrier negative", &|x| x.carrier = -1);
    add("minSize 0", &|x| x.min_size = 0);
    add("idleDaa negative", &|x| x.idle_daa = -1);
    add("idleDaa at 2^32", &|x| x.idle_daa = 4_294_967_296);
    add("kBps 0", &|x| x.k_bps = 0);
    add("kBps above 10^5", &|x| x.k_bps = 100_001);
    add("mmrBps negative", &|x| x.mmr_bps = -1);
    add("mmrBps 5000", &|x| x.mmr_bps = 5_000);
    add("liqFeeBps negative", &|x| x.liq_fee_bps = -1);
    add("liqFeeBps above 10^4", &|x| x.liq_fee_bps = 10_001);
    add("keeperFeeMin negative", &|x| x.keeper_fee_min = -1);
    add("minRestDaa negative", &|x| x.min_rest_daa = -1);
    add("minRestDaa at a quarter of the lock-time threshold", &|x| x.min_rest_daa = 125_000_000_000);
    add("bondBps negative", &|x| x.bond_bps = -1);
    add("bondBps above 10^4", &|x| x.bond_bps = 10_001);
    add("bondMin negative", &|x| x.bond_min = -1);
    add("bondMin above 10^12", &|x| x.bond_min = 1_000_000_000_001);
    add("kcBps 0", &|x| x.kc_bps = 0);
    add("kcBps above 10^5", &|x| x.kc_bps = 100_001);
    add("gBps 0", &|x| x.g_bps = 0);
    add("gBps 10^4", &|x| x.g_bps = 10_000);
    add("challengeDaa one below 2 minRestDaa", &|x| x.challenge_daa = 2 * x.min_rest_daa - 1);
    add("challengeDaa at 2^32", &|x| x.challenge_daa = 4_294_967_296);
    add("minEvidenceLife negative", &|x| x.min_evidence_life = -1);
    add("minEvidenceLife + minRestDaa above 90 days", &|x| x.min_evidence_life = 77_760_000 - x.min_rest_daa + 1);
    add("lockBps below 10^4", &|x| x.lock_bps = 9_999);
    add("lockBps above 3 * 10^4", &|x| x.lock_bps = 30_001);
    add("disputeDaa one below 2 minRestDaa", &|x| x.dispute_daa = 2 * x.min_rest_daa - 1);
    add("disputeDaa at 2^32", &|x| x.dispute_daa = 4_294_967_296);
    add("graceDaa negative", &|x| x.grace_daa = -1);
    add("graceDaa above challengeDaa", &|x| x.grace_daa = x.challenge_daa + 1);
    add("tplPrefixLen negative", &|x| x.tpl_prefix_len = -1);
    add("tplSuffixLen negative", &|x| x.tpl_suffix_len = -1);
    add("review 5 F2 (mmrBps -20,000, gBps 20,000)", &|x| (x.mmr_bps, x.g_bps) = (-20_000, 20_000));
    // Inside, at the bounds.
    add("mmrBps 0", &|x| x.mmr_bps = 0);
    add("mmrBps 4999", &|x| x.mmr_bps = 4_999);
    add("gBps 1", &|x| x.g_bps = 1);
    add("gBps 9999", &|x| x.g_bps = 9_999);
    add("kcBps 10^5", &|x| x.kc_bps = 100_000);
    add("lockBps 10^4", &|x| x.lock_bps = 10_000);
    add("lockBps 3 * 10^4", &|x| x.lock_bps = 30_000);
    add("disputeDaa 2 minRestDaa", &|x| x.dispute_daa = 2 * x.min_rest_daa);
    add("challengeDaa 2 minRestDaa, graceDaa equal to it", &|x| (x.challenge_daa, x.grace_daa) = (2 * x.min_rest_daa, 2 * x.min_rest_daa));
    add("graceDaa 0", &|x| x.grace_daa = 0);
    add("liqFeeBps 10^4, keeperFeeMin 0", &|x| (x.liq_fee_bps, x.keeper_fee_min) = (10_000, 0));
    add("bondBps 0, bondMin 0", &|x| (x.bond_bps, x.bond_min) = (0, 0));
    add("minRestDaa 0 (and D 0)", &|x| (x.min_rest_daa, x.dispute_daa) = (0, 0));
    add("tpl cut 0", &|x| (x.tpl_prefix_len, x.tpl_suffix_len) = (0, 0));
    v
}

/// F2, drift: `Market::validate`, `KpfOpen` (a fill between two orders of the market) and `KpfPending.guarantee` (of a
/// close on a pair of the market) accept exactly the same market blocks: for every variant one field off the test market,
/// inside and outside each bound of the shared check, the three verdicts agree. A bound relaxed or tightened in one of
/// them alone fails here.
#[test]
fn holds_kpf_open_kpf_pending_and_the_library_accept_the_same_market_blocks() {
    let mut inside = 0;
    for (k, (name, m)) in market_variants().into_iter().enumerate() {
        let lib = m.validate().is_ok();
        let (lo, so) = orders_on(&m);
        let (tx, e) = open_draft(&lo, &so, FULL, 1_000_000_000, &fk(), 30 + k as u32).build();
        let r = run(&tx, &e).unwrap();
        let open_ok = r[0].is_ok() && r[1].is_ok();
        assert_eq!(r[0].is_ok(), r[1].is_ok(), "{name}: the two orders disagree");
        let mut p = pair(&lk(), &sk());
        p.m = m.clone();
        let w = pend(&close_of(&lv_of(&p, 0x70), FULL, LONG), CL_BID);
        let (tx, e) = guarantee_draft(&w, &gk(), &fk(), 80 + k as u32).build();
        let pending_ok = run(&tx, &e).map(|r| r[0].is_ok()).unwrap_or(false);
        assert_eq!((lib, open_ok, pending_ok), (lib, lib, lib), "{name}: Market::validate {lib}, KpfOpen {open_ok}, KpfPending.guarantee {pending_ok}");
        inside += usize::from(lib);
    }
    assert!(inside >= 14, "the variants inside the bounds were accepted by all three: {inside}");
}

// ================================================================ holds (from the review)

/// The dispute window is measured on the lock's own UTXO score F and the claim's P only: the DAA score of the
/// disputer's funding input (very old, or unknown, read as -1) and of the counter order (unaccepted, read as -1:
/// counted from P, never earlier) move nothing. At the window's edge F + D - R the same dispute is accepted whatever
/// the funding score, and one DAA of F later refused.
#[test]
fn holds_the_window_ignores_funding_scores_and_an_unaccepted_counter_order_counts_from_p() {
    let l = lv_of(&pair(&lk(), &sk()), 0x65);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let g = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&g);
    let lock = live_locks(&tx, &g).remove(0);
    let c = Evidence::bid(1_000_000_000, FULL);
    let rest_end = w.daa + rest_daa();
    let f_edge = rest_end + rest_daa() - dispute_window();
    for fund_daa in [0u64, 1, u64::MAX] {
        let mut d = dispute(&[lock.clone().at(f_edge)], &c, &fk());
        d.lock = rest_end;
        let last = d.ins.len() - 1;
        d.ins[last].utxo.block_daa_score = fund_daa;
        accept(&d);
        let mut d = dispute(&[lock.clone().at(f_edge - 1)], &c, &fk());
        d.lock = rest_end;
        d.ins[last].utxo.block_daa_score = fund_daa;
        refused(&d, 0);
    }
    // A counter order the node has not accepted (score -1) counts from P: one DAA short of P + R refused.
    let unaccepted = c.clone().placed(u64::MAX);
    let mut d = dispute(&[lock.clone().at(w.daa + grace())], &unaccepted, &fk());
    d.lock = rest_end;
    accept(&d);
    d.lock = rest_end - 1;
    refused(&d, 0);
}

/// Once a lock exists the slow path's entries cannot act on the claim, and a second lock cannot be made for it: the
/// pending UTXO is spent. A cure signed by the loser after the guarantee names an outpoint that no longer exists; on
/// the forfeited part (a KpfPair) there is no cure entry, and its owner (both sides the winner's) cannot be made to
/// repay anything by a second guarantee of the same claim (the guarantee entry exists only on KpfPending).
#[test]
fn holds_no_second_guarantee_or_late_cure_reaches_a_fast_settled_claim() {
    let l = lv_of(&pair(&lk(), &sk()), 0x66);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let g = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&g);
    let part = live_pairs(&tx, &g).remove(0).at(w.daa + grace());
    let pair_entries: Vec<String> = pair_base().contracts.values().next().unwrap().entries.keys().cloned().collect();
    assert!(!pair_entries.iter().any(|e| e == "guarantee" || e == "cure" || e == "challenge" || e == "finalize"));
    assert_eq!(part.p.long_key, part.p.short_key, "one owner: not liquidatable again (DESIGN 4.5e)");
    let lock = live_locks(&tx, &g).remove(0);
    assert_eq!(lock.l.claim_id, w.id);
}

// ================================================================ attacks on the fix (the whole cover to the wronged party)

/// Attack on the fix: the three roles of a claim (proposer, both sides, disputer) played by one attacker with two keys
/// against an honest guarantor, on a book where nobody fills a mispriced order within R (a thin book). The attacker
/// owns both sides of a pair (two keys: the chain cannot tell), proposes a liquidation of its own long at 9.40 (true:
/// the market is there), an honest guarantor settles it (lock 3,300 KAS), and the attacker disputes with its own bid
/// at the refutation bound 9.66315790, filled with its own tokens. It keeps both sides (the forfeited part) and the
/// proposer's payout, and collects the cover 3,000 KAS as the "wronged" long and the reward 300 KAS as the executor:
/// +3,300 KAS less its offer (0 here), exactly the guarantor's loss. Before DESIGN 4.5g the same attack paid
/// 663.16 + 300 = 963.16 KAS and burned 2,336.84 KAS; the guarantor lost the same 3,300 KAS. On a close (the attacker
/// closes its long at 10.5, guaranteed, then disputes as the short with its own ask at 10.29) it takes the cover 1,500 KAS
/// and the reward 150 KAS: the whole 1,650 KAS lock (360 KAS before). Accepted as the thin-book exposure (DESIGN 4.5f
/// residual risk 1, 4.5g): an honest guarantor loses lambda x cover + offer per claim on such a book, now all of it to
/// the attacker; it must not guarantee where arbitrage does not refill a mispriced order within R.
#[test]
fn exploit_a_two_key_attacker_on_a_thin_book_takes_an_honest_guarantors_whole_lock() {
    // Liquidation.
    let l = lv_of(&pair(&lk(), &sk()), 0x71);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let (d, tx, lock) = guaranteed_by(&w, &gk(), &fk());
    let (fv, pay) = w.w.guarantee_forfeit(w.value);
    assert_eq!(d.outs[1].value as i64, fv, "the forfeited part (both sides the attacker's short key)");
    let t = accept(&dispute(&[lock.clone()], &own_bid(&lk(), REF_LIQ_LONG, FULL), &lk()));
    assert_eq!(t.outputs[0].script_public_key, p2pk_x(&xonly(&lk())));
    let gain = t.outputs[0].value as i64 + lock.l.reward();
    assert_eq!(gain, 3_300 * KAS, "the attacker takes the whole lock");
    assert_eq!(gain, lock.value as i64, "which is exactly the honest guarantor's loss");
    // What it put in and got back from the claim itself: the pair and the bond in, the forfeited part and B + fee out.
    assert_eq!(fv + pay, w.value as i64 - w.w.offer, "the settlement returns the attacker its pending value less the offer");
    let _ = tx;
    // Close.
    let l = lv_of(&pair(&lk(), &sk()), 0x72);
    let w = pend(&close_long(&l), CL_BID);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    let t = accept(&dispute(&[lock.clone()], &own_ask(&sk(), REF_CL_LONG, FULL), &sk()));
    assert_eq!(t.outputs[0].value as i64 + lock.l.reward(), 1_650 * KAS, "the whole close lock");
    assert_eq!(lock.value as i64, 1_650 * KAS);
}

/// Attack on the fix: a party that arranges to be closed or liquidated by a truthful claim and then has it disputed,
/// on a book that is NOT thin. A dispute needs an order of the opposite kind beyond the claim by the margin g, resting R
/// from P. Orders at the true market, and one time short of the bound, are refused; an order at the bound is mispriced
/// by at least g (a bid above the market, an ask below it), and any fill of it by an arbitrageur re-creates its UTXO
/// (a KOB continuation), whose rest restarts: one placed or re-created less than R before the dispute is refused. So on a
/// book where somebody fills a mispriced order within R the party cannot make the counter-evidence, whatever it is paid;
/// a truthful claim there is at risk only from the market itself moving past the bound within [P, F + D - R] (the
/// guarantor's rebound risk, DESIGN 4.5f), with the same loss as before the fix (the whole lock).
#[test]
fn holds_a_truthful_claim_on_a_book_that_refills_mispriced_orders_has_no_dispute() {
    let l = lv_of(&pair(&lk(), &sk()), 0x73);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    refused(&dispute(&[lock.clone()], &own_bid(&lk(), LIQ_ASK, FULL), &lk()), 0);
    refused(&dispute(&[lock.clone()], &own_bid(&lk(), REF_LIQ_LONG - 1, FULL), &lk()), 0);
    // The bound bid, last re-created (by a partial fill) at `at`: it counts only from at + R.
    let now = NOW;
    for (at, ok) in [(now - rest_daa(), true), (now - rest_daa() + 1, false)] {
        let mut d = dispute(&[lock.clone()], &own_bid(&lk(), REF_LIQ_LONG, FULL).placed(at), &lk());
        d.lock = now;
        if ok {
            accept(&d);
        } else {
            refused(&d, 0);
        }
    }
    // A close of the long at the market 10.5: the short needs an ask <= 10.29.
    let w = pend(&close_long(&lv_of(&pair(&lk(), &sk()), 0x74)), CL_BID);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    refused(&dispute(&[lock.clone()], &own_ask(&sk(), CL_BID, FULL), &sk()), 0);
    refused(&dispute(&[lock.clone()], &own_ask(&sk(), REF_CL_LONG + 1, FULL), &sk()), 0);
}

/// Attack on the fix: a cover that exceeds what the lock holds. The lock holds at least ceil(lambda x cover) + offer and
/// 0.1 KAS, so restitution (the cover, or 0.02 KAS when the cover is below dust) plus the reward never exceeds it, for
/// every lambda in [1, 3], every offer and covers from 1 sompi to the KAS supply; the burn is never negative. The
/// network fee of a dispute comes from the executor's own input (the reward is what no output must hold). In the engine:
/// the smallest disputable close (one token, closed at 9.19 KAS, cover 0.19 KAS, lock 0.209 KAS) pays the cover and the
/// reward exactly, and a partial close of 500 tokens likewise.
#[test]
fn holds_the_lock_always_holds_the_cover_and_the_reward() {
    let base = pair(&lk(), &sk());
    for lam in [10_000, 11_000, 15_000, 30_000] {
        for cover in [0, 1, 1_999_999, 2_000_000, 9_999_999, 19_000_000, 300_000_000_000, 2_900_000_000_000_000_000] {
            for offer in [0, 1, 8_998_000_000] {
                let mut l = lock_of(&pend(&liq(&lv_of(&base, 0x75), LONG), LIQ_ASK), &gk(), 1).l;
                (l.m.lock_bps, l.cover) = (lam, cover);
                let lock = (kpf_protocol::lock::ratio_up(cover, lam) + offer).max(kpf_protocol::lock::LOCK_FLOOR);
                assert!(l.victim_pay() + l.reward() <= lock, "lambda {lam}, cover {cover}, offer {offer}");
                assert!(l.victim_pay() >= cover);
                assert!(l.burn(lock as u64) >= 0);
            }
        }
    }
    // The smallest disputable close: one token closed by the long at 9.19 (just above bl / (1 - g)).
    let mut p = base.clone();
    p.size = MIN_SIZE;
    let l = lv_of(&p, 0x76);
    let w = pend(&close_of(&l, MIN_SIZE, LONG), 919_000_000);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    assert_eq!((lock.l.cover, lock.value as i64), (19_000_000, 20_900_000));
    let bound = lock.l.contradiction_bound().expect("disputable");
    let t = accept(&dispute(&[lock.clone()], &Evidence::ask(bound, MIN_SIZE), &fk()));
    assert_eq!(t.outputs[0].value as i64, 19_000_000);
    assert_eq!(lock.value as i64 - lock.l.cover - lock.l.reward(), 0);
}

/// Attack on the fix: a partial liquidation whose cover is computed on the wrong size. The cover is the pending part's
/// range q(n, bs - bl) for its own n (500 tokens: 1,500 KAS), not the pair's (3,000 KAS): a guarantee with a lock that
/// names the pair's range is refused (the lock state must be the one KpfPending.guarantee writes), and a dispute pays the
/// loser 1,500 KAS, at least its side of the claimed part at every price; the remainder of the pair (still the loser's) is
/// not touched by the claim or the dispute.
#[test]
fn holds_a_partial_liquidations_cover_is_its_own_parts_range() {
    let l = lv_of(&pair(&lk(), &sk()), 0x77);
    let w = pend(&liq_of(&l, HALF, LONG), LIQ_ASK);
    assert_eq!(w.w.n, HALF);
    let mut wrong = guarantee_draft(&w, &gk(), &fk(), 5);
    let mut big = w.w.lock_of(gk(), w.id, w.daa, w.value);
    big.cover = 3_000 * KAS;
    wrong.outs[2] = out(3_300 * KAS as u64, p2sh(&big.art()), None);
    refused(&wrong, 0);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    assert_eq!(lock.l.cover, 1_500 * KAS);
    let t = accept(&dispute(&[lock.clone()], &Evidence::bid(1_200_000_000, HALF), &fk()));
    assert_eq!(t.outputs[0].value as i64, 1_500 * KAS);
    for p in price_grid() {
        assert!(1_500 * KAS >= held_at(&w, p));
    }
}

/// Attack on the fix: the wronged party is also the disputer and collects the cover and the reward. That is the
/// design's watcher reward (DESIGN 4.5f): the reward is what any disputer would get, so the honest guarantor's loss is
/// the same whoever disputes (the whole lock), and the wronged party's restitution is the same too; disputing itself
/// adds exactly R to the victim and takes it from the third party that would otherwise have disputed. On a truthful
/// claim it still needs counter-evidence (`holds_a_truthful_claim_on_a_book_that_refills_mispriced_orders_has_no_dispute`).
#[test]
fn holds_a_victim_that_disputes_its_own_lock_gains_only_the_reward_any_disputer_gets() {
    let l = lv_of(&pair(&lk(), &sk()), 0x78);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    let c = Evidence::bid(1_000_000_000, FULL);
    let third = dispute(&[lock.clone()], &c, &fk());
    let by_victim = dispute(&[lock.clone()], &c, &lk());
    let (t1, t2) = (accept(&third), accept(&by_victim));
    assert_eq!(t1.outputs[0], t2.outputs[0], "the same restitution");
    let to_guarantor = |t: &Transaction| t.outputs.iter().filter(|o| o.script_public_key == p2pk_x(&gk())).count();
    assert_eq!((to_guarantor(&t1), to_guarantor(&t2)), (0, 0), "the guarantor loses its whole lock in both");
    let (c1, c2) = (t1.outputs[third.change.unwrap()].value as i64, t2.outputs[by_victim.change.unwrap()].value as i64);
    assert_eq!(t2.outputs[by_victim.change.unwrap()].script_public_key, p2pk_spk(&lk()));
    assert_eq!(c1, c2, "the victim as disputer keeps exactly what a third party would have kept");
    assert!(c1 - F_FUNDS as i64 + FEE as i64 >= lock.l.reward(), "the reward");
}

/// Attack on the fix: cover paid to a key that changed between the proposal and the dispute. The wronged party's key is
/// the pending part's own (fixed by KpfPair.propose; KpfPending has no entry that rewrites a key) and is copied into the
/// lock at the guarantee; the lock is immutable. Here the short of a pair whose long proposes a partial close of 500
/// tokens sells the short side of the remainder (a transfer to a new key) while the claim is fast-settled and
/// disputable; the dispute pays the old short key, the one that held the claimed part, and the new owner holds only the
/// remainder, which the claim never touched.
#[test]
fn holds_the_cover_goes_to_the_key_that_held_the_claimed_part_after_a_sale_of_the_rest() {
    let pending_entries: Vec<String> = kpf_protocol::artifacts::pending_base().contracts.values().next().unwrap().entries.keys().cloned().collect();
    let mut sorted = pending_entries.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["challenge", "cure", "finalize", "guarantee"], "no pending entry rewrites a key");
    let mut p = pair(&lk(), &sk());
    (p.short_bid, p.short_min, p.short_tip) = (1_100_000_000, 0, 0);
    let l = lv_of(&p, 0x79);
    let g = close_of(&l, HALF, LONG);
    let pd = ev_draft(&[g.clone()], &Evidence::bid(CL_BID, HALF), &fk(), 1);
    let ptx = accept(&pd);
    let rest = live_pairs(&ptx, &pd).remove(0).at(1_000);
    assert_eq!((rest.p.size, rest.p.short_key), (HALF, xonly(&sk())));
    let w = pend(&g, CL_BID);
    let (_, _, lock) = guaranteed_by(&w, &gk(), &fk());
    assert_eq!(lock.l.victim, xonly(&sk()));
    // The short sells the remainder's short side to a new key.
    let buyer = keypair(16);
    let t = TransferSpec { side: SHORT, n: HALF, x: 1_100_000_000, nb: 1_200_000_000, ek: xonly(&buyer), et: ZERO, k: 0 };
    let td = transfer_draft(&rest, &t, &Entrant::Direct(buyer), &fk(), 11);
    let ttx = accept(&td);
    let sold = live_pairs(&ttx, &td).remove(0);
    assert_eq!(sold.p.short_key, xonly(&buyer));
    let d = accept(&dispute(&[lock.clone()], &Evidence::ask(REF_CL_LONG, HALF), &fk()));
    assert_eq!(d.outputs[0].script_public_key, p2pk_x(&xonly(&sk())), "the cover goes to the key that held the claimed part");
    assert_eq!(d.outputs[0].value as i64, lock.l.cover);
    assert!(d.outputs.iter().all(|o| o.script_public_key != p2pk_x(&xonly(&buyer))), "the buyer of the rest is paid nothing");
}
