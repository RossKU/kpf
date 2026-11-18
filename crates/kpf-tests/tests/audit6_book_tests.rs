//! Review 6, item 8: the root cause of R6-F1 (docs/audit/REVIEW-6.md) checked against the BOOK-market family
//! (`KpfOpen`, `KpfPair`, `KpfPending`, `KpfLock`), with no contract changed.
//!
//! The root cause: under rusty-kaspa v2.1.0 (`CovenantsContext::from_tx`) any input that carries a covenant id, even a
//! plain P2PK UTXO, authorizes continuation outputs of that id with any script. A party placed into a lineage whose id
//! someone else also holds (a "handle") can therefore have any object written under that id. In the oracle family the
//! judge authenticated its dispute and pool by the part's id alone, so the holder forged the verdict.
//!
//! The three entries that put a party into such a lineage exist in the book family too, and each is reproduced here
//! with the real scripts (every input runs in the engine with the covenant context, `common::run`):
//!   1. `KpfOpen.take` into a part whose id only has to be non-zero (the handle authorizes the part);
//!   2. `KpfPair.closeDonor` against a "host" that is the attacker's plain-key UTXO of an id it holds;
//!   3. a genesis look-alike `KpfPair` created together with a P2PK twin of its id, entered through its own `transfer`.
//! In all three the attack itself is ACCEPTED (the victim's side really lands under an id the attacker holds), and in
//! all three it is harmless: no book-family entry treats any UTXO as authentic because it carries the part's id.
//!   - Every entry of `KpfPair` and `KpfPending` (and `KpfOpen.fill`/`take`/`refund`) requires
//!     `OpCovInputCount(selfId) == 1`, so the handle can never be an input of a transaction that spends a genuine
//!     object of the id; and the continuation outputs they count with `OpCovOutputCount(selfId)` can then only be
//!     authorized by the genuine input itself.
//!   - No entry reads another input or output by its covenant id. The only foreign inputs ever read are the evidence
//!     order and the ask's token custody, both authenticated by template hash and P2SH of their redeem script
//!     (`tplState`); the custody's id is compared to the market's token id, not to the part's.
//!   - `KpfPending` settles from its own state only (restore, cure and forfeit are hashes fixed by the genuine
//!     `propose`), with counter-evidence that is a template-checked KOB order: there is no dispute object to forge.
//!   - `KpfLock` reads no covenant id at all (its `claimId` is informational); `guarantee` counts only the lock output
//!     of its own transaction, and a lock pays only its own value.
//! A forged `KpfPending` (or pair, or lock) written with the handle is funded by the attacker alone and can pay out only
//! its own value; it decides nothing about the genuine part.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const N: i64 = 1_000_000_000; // 1,000 tokens
const X: i64 = 1_000_000_000; // 10 KAS
const KAS: i64 = 100_000_000;

fn accept(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "no sompi created");
    tx
}

fn refused_at(d: &Draft, i: usize) {
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, i);
}

/// The attacker's key (the handle's owner; the counterparty of the victim in every route).
fn attacker() -> Keypair {
    keypair(50)
}

/// The victim: an offline party that signed only its own order (routes 1 and 3) or nothing (route 2's donor long).
fn victim() -> Keypair {
    keypair(46)
}

/// A plain P2PK UTXO of the attacker that carries a covenant id: the attacker's handle on that id.
#[derive(Clone)]
struct Handle {
    op: TransactionOutpoint,
    utxo: UtxoEntry,
}

impl Handle {
    fn id(&self) -> Hash {
        self.utxo.covenant_id.unwrap()
    }
    fn input(&self) -> In {
        In { outpoint: self.op, utxo: self.utxo.clone(), spend: Spend::P2pk(attacker()) }
    }
    /// The handle again: an output of a transaction whose input `auth` is this handle.
    fn again(&self, auth: u16) -> kaspa_consensus_core::tx::TransactionOutput {
        out(self.utxo.amount, p2pk_spk(&attacker()), bind(auth, self.id()))
    }
}

fn handle_at(tx: &Transaction, j: usize) -> Handle {
    let (op, utxo) = utxo_of(tx, j);
    Handle { op, utxo }
}

/// Step 0 (as `audit6_tests::make_handle`): an ordinary genesis of one P2PK output of the attacker's key bound to a
/// fresh id. Consensus accepts it.
fn make_handle() -> Handle {
    let ins = vec![p2pk_in(outpoint(0x30), 10_000_000_000, None, &attacker())];
    let mut outs = vec![out(5_000_000_000, p2pk_spk(&attacker()), None), out(4_000_000_000, p2pk_spk(&attacker()), None)];
    let g = genesis_id(outpoint(0x30), &outs, &[0]);
    outs[0].covenant = bind(0, g);
    let (tx, e) = build_ext(&ins, outs, 0, &[0]);
    assert_accepted(&tx, &e);
    handle_at(&tx, 0)
}

/// Adds the handle to a draft as one more input, with its continuation as one more output.
fn with_handle(d: &Draft, h: &Handle) -> Draft {
    let mut d = d.clone();
    let i = d.ins.len();
    d.ins.push(h.input());
    d.outs.push(h.again(i as u16));
    d
}

/// The handle writes any KpfPending state under the id it holds (out 0 the forged part, out 1 the handle again, out 2
/// the attacker's change), funded by the attacker's own input.
fn forge_pending(h: &Handle, w: &Pending, value: u64, daa: u64) -> LivePend {
    let funds = value + 100 * KAS as u64;
    let ins = vec![h.input(), p2pk_in(outpoint(0x31), funds, None, &attacker())];
    let outs = vec![out(value, p2sh(&w.art()), bind(0, h.id())), h.again(0), out(funds - value - FEE, p2pk_spk(&attacker()), None)];
    let (tx, e) = build_ext(&ins, outs, 0, &[0, 0]);
    assert_accepted(&tx, &e);
    let (op, _) = utxo_of(&tx, 0);
    LivePend { w: w.clone(), value, id: h.id(), op, daa }
}

/// What the attacker can do on a book-family part `lp` whose covenant id it holds through the handle `h`, and why
/// none of it costs the victim (the part's long, key `victim`) anything:
///   (a) a liquidation proposal of the victim's long with the handle co-spent is refused (single-input guard); the
///       same proposal without the handle is the ordinary keeper path, accepted;
///   (b) a finalize of the genuine pending part with the handle co-spent is refused;
///   (c) a KpfPending forged under the id with the handle (the genuine claim's state, the victim's long forfeited to
///       the attacker's short) is accepted and finalizes, but pays out only the value the attacker funded it with;
///   (d) the genuine claim is false (a bid at 9.6632 contradicts the ask at 9.40 by g), and the honest counter-evidence
///       challenge still refutes it: the part returns to the victim's restore state with all its value but the
///       proposer's bond. Nothing the handle wrote is read on the way.
fn handle_gives_no_power(lp: &Live, h: &Handle) {
    let r = rest_daa();
    assert_eq!(lp.id, h.id(), "the part and the attacker's P2PK UTXO share one covenant id");
    assert_eq!(lp.p.long_key, xonly(&victim()));
    assert_eq!(lp.p.short_key, xonly(&attacker()));
    let n = lp.p.size;
    let g = PairLeg { l: lp.clone(), n, q: 0, act: Act::Liq { side: LONG } };
    let pd = ev_draft(&[g], &Evidence::ask(940_000_000, n), &attacker(), 11);
    // (a)
    refused_at(&with_handle(&pd, h), 0);
    let tx = accept(&pd);
    let w = live_pendings(&tx, &pd).remove(0).at(NOW - r - 10);
    assert_eq!(w.id, h.id(), "the pending part is under the held id too");
    // (b)
    let fd = finalize_draft(&w, &attacker(), 12);
    refused_at(&with_handle(&fd, h), 0);
    // (c)
    let fw = forge_pending(h, &w.w, w.value, w.daa);
    let ffd = finalize_draft(&fw, &attacker(), 13);
    let ft = accept(&ffd);
    let fp = &ffd.pairs[0].1;
    assert_eq!((fp.long_key, fp.short_key), (xonly(&attacker()), xonly(&attacker())));
    let paid: u64 = ft.outputs.iter().filter(|o| o.script_public_key != p2pk_spk(&attacker())).map(|o| o.value).sum();
    assert!(paid <= fw.value, "the forgery pays out at most what the attacker put into it");
    // (d)
    let cd = ch_draft(&[w.clone()], &Evidence::bid(966_315_790, n), &keypair(60), 14);
    accept(&cd);
    let restored = &cd.pairs[0].1;
    assert_eq!(restored.long_key, xonly(&victim()), "the victim's long is restored");
    assert_eq!(cd.outs[0].value as i64, w.value as i64 - w.w.bond, "with all its value but the proposer's bond");
    assert_eq!(cd.outs[0].covenant.map(|c| c.covenant_id), Some(h.id()));
    println!(
        "victim's long margin {} KAS under a held id: false liquidation refuted, restored part holds {} KAS",
        lp.p.long_at(lp.p.entry) / KAS,
        cd.outs[0].value as f64 / KAS as f64
    );
}

// ---------------------------------------------------------------- route 1: KpfOpen.take into a held id

/// Route 1. A resting long order (owner offline) is taken into a part whose covenant id the attacker also holds through
/// a plain P2PK UTXO: `take` checks only that the part's id is not zero, so the engine ACCEPTS it (in 0 the handle, 1
/// the order's take, 2 the attacker's funds; out 0 the handle again, 1 the order's continuation, 2 the part under the
/// held id, its short the attacker's key).
/// Why it holds in the book family: the order's guarantees are all in the part's own script (its exact bytes, checked
/// by `take`), and nothing that later decides the part's fate is identified by the part's id: a claim is a
/// `KpfPending` written by the genuine part's own `propose`, refuted by a template-checked KOB order, and every pair
/// and pending entry is the only input of its id, so the handle can never join one. The victim's 1,000 KAS margin is
/// exactly as safe as in a pair opened by `fill`.
#[test]
fn holds_take_into_a_held_id_gives_the_handle_no_power() {
    let h = make_handle();
    let lo = {
        let o = long_order(&victim());
        LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([0x7a; 32]), op: outpoint(0x7a) }
    };
    let mut part = pair(&victim(), &attacker());
    part.m = lo.o.m.clone();
    (part.tag_l, part.tag_s) = (lo.id, ZERO);
    assert_eq!((part.bl, part.entry), (X - lo.o.my_rate, X));
    let ins = vec![h.input(), open_input(&lo, "take", take_args(&part, LONG, 2, N, X)), funder(&attacker(), 1, F_FUNDS)];
    let (oo, _) = order_out(&lo, N, 1);
    let outs = vec![h.again(0), oo, out(part.floor() as u64, p2sh(&part.art()), bind(0, h.id())), out(0, p2pk_spk(&attacker()), None)];
    let mut d = Draft::new(ins, outs, Some(3));
    d.pairs.push((2, part));
    let tx = accept(&d);
    let lp = live_pairs(&tx, &d).remove(0).at(1_000);
    assert_eq!(lp.p.long_at(lp.p.entry), 1_000 * KAS);
    handle_gives_no_power(&lp, &handle_at(&tx, 0));
}

// ---------------------------------------------------------------- route 2: closeDonor into a plain-key host

/// Route 2. A donor pair's listed short closes against a "host" that is not a pair at all but the attacker's P2PK UTXO
/// of an id it holds: `closeDonor` checks P3's script, its bounds (bs3 >= bs, e3 >= E) and its value, never the host,
/// so the engine ACCEPTS it and the donor's long, which signed nothing, moves into P3 under the attacker's id with the
/// attacker's key as its short (in 0 the handle, 1 the donor, 2 the attacker's funds; out 0 the handle again, 1 the
/// donor's short paid in full, 2 P3).
/// Why it holds in the book family: P3 is a fully funded KpfPair with the long's own bound, a better-or-equal short
/// bound and entry, and P3's script alone decides the long's fate; the host-side checks a real host would add protect
/// only the host's short (here the attacker's). No later object is authenticated by P3's id (see the module note), so
/// the handle on it is worthless: the donor's long, 1,000 KAS, is as safe as in its old pair.
#[test]
fn holds_close_donor_into_a_held_id_gives_the_handle_no_power() {
    let h = make_handle();
    let mut dp = pair(&victim(), &keypair(47));
    (dp.short_bid, dp.tag_l, dp.tag_s) = (X, ZERO, ZERO);
    let donor = Live { p: dp.clone(), value: dp.floor() as u64, id: Hash::from_bytes([0x5d; 32]), op: outpoint(0x5d), daa: 1_000 };
    let mut p3 = dp.cleared();
    p3.short_key = xonly(&attacker());
    (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
    p3.tag_l = donor.id;
    let args = vec![iv(N), iv(X), iv(dp.entry), iv(2), iv(dp.bs), bv(&xonly(&attacker())), iv(0), iv(0), iv(0)];
    let pay = owed(N, dp.bs - X, dp.short_tip, dp.m.scale) + donor.value as i64 - dp.range_of(N);
    let ins = vec![h.input(), pair_input(&donor, "closeDonor", args), funder(&attacker(), 2, F_FUNDS)];
    let outs = vec![h.again(0), out(pay as u64, p2pk_x(&dp.short_key), None), out(p3.floor() as u64, p2sh(&p3.art()), bind(0, h.id())), out(0, p2pk_spk(&attacker()), None)];
    let mut d = Draft::new(ins, outs, Some(3));
    d.pairs.push((2, p3));
    d.lock = donor.handover_from();
    let tx = accept(&d);
    let lp3 = live_pairs(&tx, &d).remove(0).at(1_000);
    assert_eq!((lp3.p.bl, lp3.p.bs, lp3.p.entry), (dp.bl, dp.bs, dp.entry), "the moving long keeps its terms");
    handle_gives_no_power(&lp3, &handle_at(&tx, 0));
}

// ---------------------------------------------------------------- route 3: a genesis look-alike pair with a P2PK twin

/// Route 3. The attacker writes a look-alike KpfPair (both sides its own key, its long listed at 10 KAS) by a genesis
/// together with a P2PK twin of the same id (no twin scan runs at a genesis no order takes part in); a victim's resting
/// long order takes the listing through the pair's own `transfer`, which the engine ACCEPTS, and the victim's part is
/// under the id the twin holds.
/// Why it holds in the book family: as in route 1, the twin can never be co-spent with the part or its pending part
/// (single-input guard) and nothing reads the id; so the book family needs no twin scan at a genesis, and the
/// look-alike limit ("an entrant through an order is protected on chain") is true here. The victim's 1,000 KAS margin
/// is as safe as in a pair opened by `fill`.
#[test]
fn holds_an_order_taking_a_genesis_pair_with_a_twin_gives_the_twin_no_power() {
    let mut gp = pair(&attacker(), &attacker());
    (gp.long_ask, gp.tag_l, gp.tag_s) = (X, ZERO, ZERO);
    let funds = 900_000 * KAS as u64;
    let ins = vec![p2pk_in(outpoint(0x35), funds, None, &attacker())];
    let mut outs = vec![out(gp.floor() as u64, p2sh(&gp.art()), None), out(10 * KAS as u64, p2pk_spk(&attacker()), None)];
    let rest = funds - gp.floor() as u64 - 10 * KAS as u64 - FEE;
    outs.push(out(rest, p2pk_spk(&attacker()), None));
    let g = genesis_id(outpoint(0x35), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let (gt, ge) = build_ext(&ins, outs, 0, &[0]);
    assert_accepted(&gt, &ge);
    let fake = Live { p: gp.clone(), value: gp.floor() as u64, id: g, op: utxo_of(&gt, 0).0, daa: 1_000 };
    let twin = handle_at(&gt, 1);
    assert_eq!(twin.id(), g);
    let lo = {
        let o = long_order(&victim());
        LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([0x7b; 32]), op: outpoint(0x7b) }
    };
    let t = TransferSpec { side: LONG, n: N, x: X, nb: X - lo.o.my_rate, ek: lo.o.owner, et: lo.id, k: 0 };
    let td = transfer_draft(&fake, &t, &Entrant::Open(lo), &attacker(), 3);
    let tx = accept(&td);
    let part = live_pairs(&tx, &td).remove(0).at(1_000);
    assert_eq!(part.p.long_at(part.p.entry), 1_000 * KAS);
    handle_gives_no_power(&part, &twin);
}

/// Control: the guard is what keeps the handle out. A pair whose long and short UTXOs are both genuine, under an id no
/// one else holds, cannot be written to from outside at all (the covenant context refuses an output of the id without
/// an input of it), and with a handle the forgery can be written but, as above, never co-spent with the genuine part.
#[test]
fn holds_a_held_id_cannot_join_any_pair_entry() {
    let h = make_handle();
    let mut p = pair(&victim(), &attacker());
    (p.tag_l, p.tag_s) = (ZERO, ZERO);
    let lp = Live { p: p.clone(), value: p.floor() as u64, id: h.id(), op: outpoint(0x5e), daa: 1_000 };
    // An idle settle (P8, anyone) and an amend with the handle co-spent: both refused at the pair input.
    refused_at(&with_handle(&idle_draft(&lp, &attacker(), 21), &h), 0);
    accept(&idle_draft(&lp, &attacker(), 22));
    refused_at(&with_handle(&amend_draft(&lp, &victim(), LONG, (X, 0, 0), &attacker(), 23), &h), 0);
    accept(&amend_draft(&lp, &victim(), LONG, (X, 0, 0), &attacker(), 24));
    // Without a handle, nobody can write under a pair's id.
    let ins = vec![p2pk_in(outpoint(0x36), 10 * KAS as u64, None, &attacker())];
    let outs = vec![out(9 * KAS as u64, p2pk_spk(&attacker()), bind(0, Hash::from_bytes([0x5f; 32])))];
    let (tx, e) = build_ext(&ins, outs, 0, &[0]);
    assert!(run(&tx, &e).is_err(), "no output of an id without an input of it");
}
