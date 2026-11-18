//! Review 7 (docs/audit/REVIEW-7.md), tooling: the builders behind `kpf deposit` and `kpf contest`
//! (`oracle_build::deposit`, `contest`) and the wallet's trace (`trace_dispute`, `trace_claim`) on the pinned
//! artifacts, offline. Since the fix of R7-F2 a commitment builder takes an explicit `Trust`: a trace of the claim (and
//! of the dispute) or `Untraced`, which the commands accept only with `--accept-untraced` and without a node.

use kaspa_consensus_core::hashing::covenant_id::covenant_id;
use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{CovenantBinding, Transaction, TransactionId, TransactionInput, TransactionOutpoint, TransactionOutput};
use kaspa_consensus_core::Hash;
use kaspa_txscript::pay_to_script_hash_script;
use kpf_protocol::judge::{Js, STATUS_FREE, UNIT, YES};
use kpf_protocol::oracle::*;
use kpf_protocol::oracle_artifacts::kit;
use kpf_protocol::oracle_build::*;
use kpf_protocol::tx::{p2pk_spk, pubkey_of, KeySet, KeyUtxo, MIN_FEE_RATE, TX_VERSION};

const KAS: u64 = 100_000_000;

fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}

/// A forger's genesis: one input of its own, the outputs `outs` (the covenant id computed over `group`).
fn genesis(outs: Vec<TransactionOutput>, group: &[usize], who: TransactionOutpoint) -> (Transaction, Hash) {
    let mut outs = outs;
    for &j in group {
        outs[j].covenant = None;
    }
    let id = covenant_id(who, group.iter().map(|&j| (j as u32, &outs[j])));
    for &j in group {
        outs[j].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: id });
    }
    let tx = Transaction::new(TX_VERSION, vec![TransactionInput::new_with_compute_budget(who, vec![], 0, 0)], outs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);
    (tx, id)
}

/// R7-F2, fixed. The forged dispute of the review (a dispute written by a genesis under the forger's own id, a tally no
/// receipt backs) can no longer be committed to by mistake: the trace fails in every arrangement the forger controls,
/// a trace of another claim is refused by the builder, and the builder takes no implicit trust (the commands accept
/// `Untraced` only with `--accept-untraced` and without a node, printing what was not checked:
/// `ocmds::tests::the_trace_gate_of_contest_and_deposit`, `oracle_cli`).
#[test]
fn holds_deposit_refuses_a_dispute_its_trace_does_not_reach() {
    let m = samples::market();
    let jm = m.judge_market();
    let (staker, funder) = ([71u8; 32], [60u8; 32]);
    let mut keys = KeySet::new();
    keys.add_secret(&staker).unwrap();
    keys.add_secret(&funder).unwrap();
    let (sk, fk) = (pubkey_of(&staker).unwrap(), pubkey_of(&funder).unwrap());
    let anchor = 50_000_000i64;
    let k = kit();
    // The forgery: a dispute (any spk hash, any anchor, a tally with no receipts) written by the forger's genesis.
    let forged = Js::opened(jm, [0xab; 32], anchor, 10 * UNIT);
    let dspk = pay_to_script_hash_script(&k.stake.redeem(&forged.state_bytes()));
    let (g, gid) = genesis(vec![TransactionOutput::new(50_000_000, dspk.clone())], &[0], op(9));
    let dispute = OLive { s: forged.clone(), value: 50_000_000, id: Some(gid), op: TransactionOutpoint { transaction_id: g.id(), index: 0 }, daa: anchor as u64 };
    // (1) The genesis as both the opening and the claim's birth: no contest input, no pending part.
    assert!(trace_dispute(&dispute, &g, &g).is_err());
    // (2) A genesis group of a pending-looking part, a contested part, a receipt and the dispute under one id (all the
    // forger's): the claim's id is not born alone, so the trace fails whatever the opening says.
    let p = samples::pair(&m);
    let (w, _) = p.pending_of(k, p.size, kpf_protocol::state::KIND_CLOSE, kpf_protocol::state::LONG, 1_000_000_000, 1).unwrap();
    let w = w.made_by(&op(8));
    let pspk = pay_to_script_hash_script(&w.redeem(k));
    let (g2, gid2) = genesis(vec![TransactionOutput::new(1_000 * KAS, pspk), TransactionOutput::new(50_000_000, dspk)], &[0, 1], op(8));
    assert!(trace_claim(gid2, &g2, &w.redeem(k)).unwrap_err().to_string().contains("not by exactly one"));
    // (3) A pending part born alone but with no pair input naming it: not proposed by a pair.
    let (g3, gid3) = genesis(vec![TransactionOutput::new(1_000 * KAS, pay_to_script_hash_script(&w.redeem(k)))], &[0], op(8));
    assert!(trace_claim(gid3, &g3, &w.redeem(k)).unwrap_err().to_string().contains("did not run KpfPairO.propose"));
    // The builder: a trace of another claim is refused; the forged dispute cannot be traced at all.
    let stake = OLive { s: Js::free(sk, jm), value: 130 * KAS, id: None, op: op(2), daa: 0 };
    assert_eq!(stake.s.status, STATUS_FREE);
    let f = KeyUtxo { outpoint: op(3), amount: 10 * KAS, daa: 0, key: fk };
    let other = ClaimTrace { claim_id: Hash::from_bytes([0x77; 32]), birth: g.id(), out: 0, pair_in: op(1), proposed: w.clone() };
    let err = deposit(&dispute, YES, std::slice::from_ref(&stake), std::slice::from_ref(&f), &fk, MIN_FEE_RATE, &Trust::Traced(&other)).unwrap_err();
    assert!(err.to_string().contains("another claim"), "{err}");
    // `Untraced` still builds (the acknowledged path of `--accept-untraced`): the builder no longer decides alone.
    let (b, after) = deposit(&dispute, YES, &[stake], &[f], &fk, MIN_FEE_RATE, &Trust::Untraced).expect("acknowledged");
    assert_eq!(after.side, YES);
    let sg = b.sign(&keys).expect("signed");
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok());
    let _ = p2pk_spk(&fk);
}
