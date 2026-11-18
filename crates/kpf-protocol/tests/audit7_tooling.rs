//! Review 7 (docs/audit/REVIEW-7.md), tooling: the builder behind `kpf deposit` (`oracle_build::deposit`, called by
//! `crates/kpf-cli/src/ocmds.rs` `deposit_cmd` with nothing in between) on the pinned artifacts, offline.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::judge::{Js, STATUS_FREE, UNIT, YES};
use kpf_protocol::oracle::*;
use kpf_protocol::oracle_build::*;
use kpf_protocol::tx::{pubkey_of, KeySet, KeyUtxo, MIN_FEE_RATE};

const KAS: u64 = 100_000_000;

fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}

/// R7-F2. DESIGN-ORACLE-IMPL 13 and 24.2 (a) leave the forged dispute (a dispute written by a genesis, not opened on a
/// claim of a pair) to the staker's wallet, which "must trace the claim's id to a propose" before committing. The
/// shipped wallet does not: `kpf deposit --dispute FILE --stake ...` reads the dispute from the file alone (no node
/// lookup even with `--node`: `stake_of_path`), checks only that the side trails and the stakes are aged, and builds and
/// signs the staker's commitment into it. Here the dispute is a forgery under an id no claim ever had (its tally says
/// NO holds 10 KAS, with no NO receipt behind it): the builder accepts it, the signed transaction verifies in the
/// engine, and no warning exists anywhere on the path. After it lands the staker's 130 KAS is a receipt of a pool that
/// can never pay it (`judge_attack_tests::exploit_a_dispute_written_by_a_genesis_traps_the_stakers_who_join_it`).
/// Who loses what: an honest staker handed a dispute file by someone else loses every stake it commits (130 KAS here).
#[test]
fn exploit_deposit_commits_a_stake_into_a_dispute_no_opening_made() {
    let m = samples::market();
    let jm = m.judge_market();
    let (staker, funder) = ([71u8; 32], [60u8; 32]);
    let mut keys = KeySet::new();
    keys.add_secret(&staker).unwrap();
    keys.add_secret(&funder).unwrap();
    let (sk, fk) = (pubkey_of(&staker).unwrap(), pubkey_of(&funder).unwrap());
    let anchor = 50_000_000i64;
    // The forgery: any spk hash, any anchor, a tally with no receipts, under the forger's own fresh id.
    let forged = Js::opened(jm, [0xab; 32], anchor, 10 * UNIT);
    let dispute = OLive { s: forged, value: 50_000_000, id: Some(Hash::from_bytes([0xd7; 32])), op: op(1), daa: anchor as u64 };
    let stake = OLive { s: Js::free(sk, jm), value: 130 * KAS, id: None, op: op(2), daa: 0 };
    assert_eq!(stake.s.status, STATUS_FREE);
    let f = KeyUtxo { outpoint: op(3), amount: 10 * KAS, daa: 0, key: fk };
    let (b, after) = deposit(&dispute, YES, &[stake], &[f], &fk, MIN_FEE_RATE).expect("the builder accepts a forged dispute");
    assert_eq!(after.side, YES, "the staker takes the lead of a dispute that can never pay it");
    let sg = b.sign(&keys).expect("signed");
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "the engine accepts the commitment: {:?}", v.errors());
}
