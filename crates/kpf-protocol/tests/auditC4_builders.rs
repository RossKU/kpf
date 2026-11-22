//! Review C4 of the committee family (docs/audit/REVIEW-C4.md): the tool's builders of the two forfeits and of the
//! scripts with the most signature checks, on the pinned artifacts, through `verify::verify` (which counts an input's
//! signature operations as a node does: the transaction signatures and every data signature of the script, at most 15).
//!
//! `c4_holds_*`: built, signed and verified.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::*;
use kpf_protocol::oracle::Makers;
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::tx::{p2sh_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use kpf_protocol::verify::data_sig_ops;
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const FEED: i64 = 1_000_000_000;

fn kp(seed: u8) -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[seed; 32]).expect("a secret key"))
}
fn xk(k: &Keypair) -> [u8; 32] {
    k.x_only_public_key().0.serialize()
}
fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}
fn fund(key: [u8; 32], n: u8) -> KeyUtxo {
    KeyUtxo { outpoint: op(n), amount: 1_000 * KAS1 as u64, daa: 0, key }
}
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    println!("{what}: data signatures in input 0's script {}, fee {} sompi", data_sig_ops(&sg.tx.inputs[0].signature_script), sg.fee);
    sg
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

#[test]
fn c4_holds_the_forfeits_and_a_epoch_table_of_fifteen_signers_pass_the_nodes_count_of_signature_operations() {
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let m = CMarket::testnet(rid.as_bytes(), [0xfe; 32]);
    let members: Vec<Keypair> = (0..MEMBERS as u8).map(|i| kp(101 + i)).collect();
    let me = kp(77);
    let mut keys = KeySet::new();
    keys.add_keypair(me);
    for k in &members {
        keys.add_keypair(*k);
    }
    let mek = xk(&me);
    // A full committee: fifteen members of 10 KAS.
    let mut table = EMPTY_TABLE;
    for (i, k) in members.iter().enumerate() {
        table[i] = Member { key: xk(k), weight: 10 * KAS1 };
    }
    let r = Committee { p: rp, from: 6_000, members: table, holds: kpf_protocol::committee::NO_HOLDS };
    let committee = OLive { s: r, value: (150 * KAS1) as u64 + COMMITTEE_CARRIER, id: Some(rid), op: op(1), daa: 6_000 };

    // Member 3 signed 10.00 and 10.01 for the time 6,500. Anyone slashes it: the committee's script holds two data
    // signatures and one transaction signature (leave's); a tenth to the sender, nine tenths at the burn script, last.
    let a = sign_price(&members[3], &rid.as_bytes(), &m.id(), 6_500, FEED);
    let b = sign_price(&members[3], &rid.as_bytes(), &m.id(), 6_500, FEED + 1_000_000);
    assert!(equivocation(&a, &b, &rid.as_bytes(), &m.id()));
    let (built, after) = forfeit_member(&committee, 3, &m.id(), &a, &b, &[fund(mek, 60)], &mek, MIN_FEE_RATE).expect("the forfeit of a member");
    let sg = signed("member-forfeit", &built, &keys);
    // (3 since the judged slash: the committee's `charge` checks one more data signature.)
    assert_eq!(data_sig_ops(&sg.tx.inputs[0].signature_script), 3);
    let last = sg.tx.outputs.last().unwrap();
    assert_eq!((last.value as i64, last.script_public_key.clone()), (9 * KAS1, p2sh_spk(&[0x6a])));
    assert_eq!(sg.tx.outputs[0].value as i64, 140 * KAS1 + COMMITTEE_CARRIER as i64);
    let spent: i64 = sg.entries.iter().map(|e| e.amount as i64).sum::<i64>() - sg.tx.outputs.iter().map(|o| o.value as i64).sum::<i64>();
    assert_eq!(spent, sg.fee as i64);
    let change = sg.tx.outputs[1].value as i64;
    assert_eq!(change, 1_000 * KAS1 + KAS1 - sg.fee as i64, "the sender's tenth, 1 KAS, less the fee");
    assert_eq!(after.members[3], Member::EMPTY);
    // Not a proof: the builder refuses another member's index, the same price twice, a signed price of another market.
    assert!(forfeit_member(&committee, 4, &m.id(), &a, &b, &[fund(mek, 60)], &mek, MIN_FEE_RATE).is_err());
    assert!(forfeit_member(&committee, 3, &m.id(), &a, &a, &[fund(mek, 60)], &mek, MIN_FEE_RATE).is_err());
    assert!(forfeit_member(&committee, 3, &[9; 32], &a, &b, &[fund(mek, 60)], &mek, MIN_FEE_RATE).is_err());

    // The same key's stake on its way out: the member leaves (the committee's one transaction signature), and the exit's
    // script (two data signatures) burns nine tenths of it, before and after its release alike.
    let (built, left, e) = leave(&committee, 3, &[fund(mek, 61)], &mek, MIN_FEE_RATE).expect("the leave");
    let sg = signed("committee-leave", &built, &keys);
    let _ = left;
    let exit = nth(&sg, 1, e, 6_100);
    let built = forfeit_exit(&exit, &m.id(), &a, &b, &[fund(mek, 62)], &mek, MIN_FEE_RATE).expect("the forfeit of an exit");
    let sg = signed("exit-forfeit", &built, &keys);
    assert_eq!(data_sig_ops(&sg.tx.inputs[0].signature_script), 3);
    assert_eq!(sg.tx.outputs.last().unwrap().value as i64, 9 * KAS1);
    let built = withdraw(&exit, &[fund(mek, 63)], &mek, MIN_FEE_RATE).expect("the withdraw");
    signed("exit-withdraw", &built, &keys);

    // The script with the most: an epoch table, fifteen data signatures whatever entry runs. A renewal in which every one
    // of the fifteen members signed, and the epoch table's retirement.
    let epoch_table = OLive { s: EpochTable { members: table, total: 150 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(2), daa: 6_000 };
    let signed_prices: Vec<SignedPrice> = members.iter().map(|k| sign_price(k, &rid.as_bytes(), &m.id(), 8_000, FEED)).collect();
    let att = Attestation::collect(&epoch_table.s, &rid.as_bytes(), &m.id(), 8_000, &signed_prices).expect("fifteen members");
    assert_eq!(att.bits.count_ones(), 15);
    let p = CPair { m, size: 100 * m.p.scale, bl: 900_000_000, bs: 1_200_000_000, entry: FEED, long_key: xk(&members[0]), short_key: xk(&members[1]), expiry: 4_000_000, long_ask: 0, long_min: 0, long_tip: 0, short_bid: 0, short_min: 0, short_tip: 0, mk: Makers::default() };
    let pair = OLive { value: p.floor() as u64, id: Some(Hash::from_bytes([0x50; 32])), op: op(10), daa: 6_100, s: p };
    let (built, _) = keep_alive(&pair, &epoch_table, &att, &[fund(mek, 64)], &mek, MIN_FEE_RATE).expect("a renewal");
    let sg = signed("c-keep-alive with fifteen signed members", &built, &keys);
    assert_eq!(data_sig_ops(&sg.tx.inputs[1].signature_script), 15);
    let built = retire(&epoch_table, &[fund(mek, 65)], &mek, MIN_FEE_RATE).expect("the retire");
    let sg = signed("epoch-table-retire", &built, &keys);
    assert_eq!(data_sig_ops(&sg.tx.inputs[0].signature_script), 15);
}
