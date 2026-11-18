//! Review 4 (docs/audit/REVIEW-4.md), the library's two-step builders on the in-memory chain (every input in the script
//! engine). Keys are the TEST-ONLY keys of `kpf_protocol::fixtures`.

use kaspa_consensus_core::Hash;
use kpf_protocol::build::{Funding, Opts};
use kpf_protocol::evidence::cure;
use kpf_protocol::fixtures::*;
use kpf_protocol::pending::pending_of;
use kpf_protocol::sim::Chain;
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use kpf_protocol::verify::verify;

const KAS: u64 = 100_000_000;

/// R4-F4 (Low). A look-alike KpfPending (the real template under a genesis covenant id, any state: here the victim's own
/// pair copied into a liquidation claim of its long with an attacker's proposer key, holding only 1 KAS) is handed to
/// the long as "your pair is being liquidated". `cure` builds and verifies the cure without a word: the state's hashes
/// match (`cure_pair` is Some), and nothing compares the part's value with the claim it describes (a genuine part holds
/// at least q(n, BS - BL) + refund) or its covenant id with a pair of the user. The long's funder pays the attacker the
/// `refund` and funds the whole cured pair's range (about 3,070 KAS), while the command's line says it "adds 70 KAS of
/// margin". The cured pair is a real KpfPair of the long and its short under the look-alike's id, so the short (if it is
/// the attacker, or colludes) receives the margin of a range the long paid for in full.
/// Who loses: the long that runs `kpf cure` on such a file: the refund outright, and the short's side of the range.
#[test]
fn exploit_cure_of_an_underfunded_look_alike_pending() {
    let mut chain = Chain::new();
    chain.advance(1_000);
    let m = kob_market();
    let mut p = pair(pubkey(21), pubkey(22));
    p.m = m.clone();
    let w = pending_of(&p.clocked(900), p.size, KIND_LIQ, LONG, 940_000_000, pubkey(23));
    assert!(w.hashes_match(), "a look-alike with a self-consistent state passes the tool's only check");
    let look = chain.mint_obj(Obj::Pending(w.clone()), KAS, Hash::from_bytes([0x99; 32]), 1_000).pending();
    let fk = pubkey(24);
    let funding = Funding { utxos: vec![chain.mint(&fk, 100_000 * KAS)], change_key: fk };
    let b = cure(&look, &funding, &Opts::default()).expect("the builder accepts the look-alike");
    let s = b.sign(&keyset(&[21, 24])).expect("sign");
    let v = verify(&s.tx, &s.entries);
    assert!(v.is_ok(), "the engine accepts it: {:?}", v.errors());
    let before = chain.balance(&fk);
    chain.apply_signed(&s).expect("apply");
    let paid = before - chain.balance(&fk);
    let said = w.cure_add() as u64;
    assert_eq!(chain.balance(&pubkey(23)), w.refund as u64, "the attacker's proposer key is repaid a refund it never posted");
    assert!(paid > 40 * said, "the funder paid {paid} sompi; the command says it adds {said}");
}
