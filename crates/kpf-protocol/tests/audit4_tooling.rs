//! Review 4 (docs/audit/REVIEW-4.md) and its fixes (DESIGN 4.5e), the library's two-step builders on the in-memory chain
//! (every input in the script engine). Keys are the TEST-ONLY keys of `kpf_protocol::fixtures`.

use kaspa_consensus_core::Hash;
use kpf_protocol::build::{Funding, Opts};
use kpf_protocol::evidence::{check_claim, check_pending, check_pending_of, cure, finalize, Act};
use kpf_protocol::fixtures::*;
use kpf_protocol::pending::pending_of;
use kpf_protocol::sim::Chain;
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use kpf_protocol::verify::verify;

const KAS: u64 = 100_000_000;

/// R4-F4 (Low), fixed (DESIGN 4.5e). A look-alike KpfPending (the real template under a genesis covenant id, here the
/// victim's own pair copied into a liquidation claim of its long with an attacker's proposer key, holding only 1 KAS) is
/// handed to the long as "your pair is being liquidated". Its hashes match its state, but it holds far less than the
/// q(n, bs - bl) + refund every genuine part holds: `cure` and `finalize` refuse it (so does `challenge`, through the
/// same check in the evidence builder), and with the user's pair file it is refused for its covenant id too. The same
/// claim with the value a genuine proposal writes, under the pair's id, passes, and the cure it builds makes the funder
/// pay exactly the added margin that the summary states (`cure_total`).
#[test]
fn holds_cure_refuses_an_underfunded_look_alike_pending() {
    let mut chain = Chain::new();
    chain.advance(1_000);
    let m = kob_market();
    let mut p = pair(pubkey(21), pubkey(22));
    p.m = m.clone();
    let w = pending_of(&p.clocked(900), p.size, KIND_LIQ, LONG, 940_000_000, pubkey(23));
    assert!(w.hashes_match(), "the look-alike's state is self-consistent");
    let look = chain.mint_obj(Obj::Pending(w.clone()), KAS, Hash::from_bytes([0x99; 32]), 1_000).pending();
    let fk = pubkey(24);
    let funding = Funding { utxos: vec![chain.mint(&fk, 100_000 * KAS)], change_key: fk };
    let err = cure(&look, &funding, &Opts::default()).unwrap_err().to_string();
    assert!(err.contains("look-alike"), "{err}");
    let err = finalize(&look, &funding, &Opts::default()).unwrap_err().to_string();
    assert!(err.contains("look-alike"), "{err}");
    assert!(check_pending(&look).is_err());
    // The genuine value, under another id than the user's pair: refused for its lineage when the pair is given.
    let pair_id = Hash::from_bytes([0x31; 32]);
    let v = (p.floor() + w.bond) as u64;
    assert!(v as i64 >= w.least_value());
    let other = chain.mint_obj(Obj::Pending(w.clone()), v, Hash::from_bytes([0x98; 32]), 1_000).pending();
    check_pending(&other).expect("a genuine value and matching hashes");
    let err = check_pending_of(&other, pair_id, &p).unwrap_err().to_string();
    assert!(err.contains("covenant id"), "{err}");
    let mut stranger = p.clone();
    stranger.long_key = pubkey(25);
    assert!(check_pending_of(&other, Hash::from_bytes([0x98; 32]), &stranger).is_err(), "other owners than the pair");
    check_pending_of(&other, Hash::from_bytes([0x98; 32]), &p).expect("the part of this pair");
    // A genuine part: the cure goes through, the funder pays what the summary says, and the proposer gets its refund.
    let fund = Funding { utxos: vec![chain.mint(&fk, 100_000 * KAS)], change_key: fk };
    let b = cure(&other, &fund, &Opts::default()).expect("a genuine cure");
    let s = b.sign(&keyset(&[21, 24])).expect("sign");
    assert!(verify(&s.tx, &s.entries).is_ok());
    let before = chain.balance(&fk);
    chain.apply_signed(&s).expect("apply");
    let paid = (before - chain.balance(&fk)) as i64;
    let total = w.cure_total(v);
    assert!(paid >= total && paid < total + KAS as i64, "the funder paid {paid}, the summary says {total} plus the network fee");
    assert_eq!(total, w.cure_add(), "a whole-pair part: the funder pays exactly the margin");
    assert_eq!(chain.balance(&pubkey(23)), w.refund as u64);
}

/// R4-F1 in the tooling: `propose-liquidate` refuses a pair whose two sides have one owner (the contract would refuse
/// it), with the reason; a close by that owner is still allowed.
#[test]
fn holds_tooling_refuses_to_liquidate_a_one_owner_pair() {
    let mut p = pair(pubkey(21), pubkey(21));
    p.m = kob_market();
    let err = check_claim(&p, p.size, &Act::Liq { side: LONG, proposer: pubkey(23) }, true, 1, "ask").unwrap_err().to_string();
    assert!(err.contains("one owner"), "{err}");
    check_claim(&p, p.size, &Act::Close { side: LONG }, false, 950_000_000, "bid").expect("the one owner may close");
}
