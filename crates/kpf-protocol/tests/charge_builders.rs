//! The judged slash of the committee family (docs/DESIGN-COMMITTEE.md 5a) through the tool's builders on the pinned
//! artifacts: a charge on a member and on an exit, the judge's contest and verdict (the oracle family's `KpfStake`,
//! unchanged), and the three ways a charge leaves its stake: slashed (refuted), cleared to the charged key (upheld),
//! cleared and burned (lapsed). Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::*;
use kpf_protocol::judge::{JudgeMarket, JudgeParams, Js, YES};
use kpf_protocol::oracle_build::{deposit, OBuilt, OLive, OSigned, Trust};
use kpf_protocol::tx::{p2pk_spk, p2sh_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const MARKET: [u8; 32] = [0x4d; 32];

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
    println!("{what}: {} bytes of input 0's signature script, fee {} sompi", sg.tx.inputs[0].signature_script.len(), sg.fee);
    sg
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

struct World {
    rid: Hash,
    members: Vec<Keypair>,
    me: Keypair,
    judge1: Keypair,
    judge2: Keypair,
    keys: KeySet,
    jm: JudgeMarket,
    committee: OLive<Committee>,
}

/// A committee of three members (30, 22, 15 KAS) whose charges cost 5 KAS, wait 300 DAA for a contest and are judged by a
/// stake of at least 20 KAS aged 200 DAA, in rounds of 100 DAA.
fn world() -> World {
    let rid = Hash::from_bytes([0x52; 32]);
    let jm = JudgeMarket { id: [0x6a; 32], p: JudgeParams { mu_bps: 12_500, age: 200, unbond: 200, round: 100, burn_bps: 1_000, min_stake: 20 * KAS1 } };
    let rp = CommitteeParams { charge: ChargeParams { judge: jm, bond: 5 * KAS1, daa: 300 }, ..CommitteeParams::testnet() };
    let members: Vec<Keypair> = (0..3u8).map(|i| kp(101 + i)).collect();
    let (me, judge1, judge2) = (kp(77), kp(78), kp(79));
    let mut keys = KeySet::new();
    for k in members.iter().chain([&me, &judge1, &judge2]) {
        keys.add_keypair(*k);
    }
    let mut table = EMPTY_TABLE;
    for (i, w) in [30, 22, 15].into_iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: w * KAS1 };
    }
    let r = Committee { p: rp, from: 6_000, members: table, holds: NO_HOLDS };
    let committee = OLive { s: r, value: (67 * KAS1) as u64 + COMMITTEE_CARRIER, id: Some(rid), op: op(1), daa: 6_000 };
    World { rid, members, me, judge1, judge2, keys, jm, committee }
}

/// A free judge stake of `owner`, accepted at `daa`.
fn stake(w: &World, owner: &Keypair, kas: i64, n: u8, daa: u64) -> OLive<Js> {
    let b = judge_stake(&w.jm, xk(owner), (kas * KAS1) as u64, &[fund(xk(&w.me), n)], &xk(&w.me), MIN_FEE_RATE).expect("a judge stake");
    let sg = signed("judge stake", &b, &w.keys);
    nth(&sg, 1, Js::free(xk(owner), w.jm), daa)
}

#[test]
fn a_member_whose_price_the_judge_refutes_is_slashed() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    // Member 1 signed a price of 20 for the time 6,500 (the feed was 10). Anyone charges it, with the bond.
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let opener = stake(&w, &w.judge1, 20, 59, 6_000);
    let (b, c, cid) = charge(&Staked::Member(&w.committee, 1), &MARKET, &signed_price, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).expect("the charge");
    let sg = signed("committee-charge", &b, &w.keys);
    let held = w.committee.s.charged(1, &cid.as_bytes()).unwrap();
    assert_ne!(held.holds[1], NO_HOLD);
    assert_eq!((held.members, held.holds[0], held.holds[2]), (w.committee.s.members, NO_HOLD, NO_HOLD), "the member keeps its weight and goes on signing");
    let committee = nth(&sg, 0, held, 6_600);
    let ch = nth(&sg, 1, c, 6_600);
    assert_eq!((ch.value as i64, ch.id), (5 * KAS1, Some(cid)));
    // What is not a charge: a signed price of another member's key, of another market, with a broken signature.
    assert!(charge(&Staked::Member(&w.committee, 0), &MARKET, &signed_price, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).is_err());
    assert!(charge(&Staked::Member(&w.committee, 1), &[9; 32], &signed_price, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).is_err());
    // A held member cannot be settled while the charge is pending, and the member cannot take its stake out through an
    // exit: the exit carries the hold.
    assert!(charge_settle(&Staked::Member(&committee, 1), &ch, &NO_HOLD, &NO_LATER, 6_700, &[fund(mek, 61)], &mek, MIN_FEE_RATE).is_err());
    let (b, _, e) = leave(&committee, 1, &[fund(mek, 61)], &mek, MIN_FEE_RATE).expect("a held member leaves");
    let lsg = signed("committee-leave of a held member", &b, &w.keys);
    assert_eq!(e.hold, committee.s.holds[1]);
    assert!(withdraw(&nth(&lsg, 1, e, 6_700), &[fund(mek, 62)], &mek, MIN_FEE_RATE).is_err(), "a held exit is not withdrawn");

    // A judge stake accepted 200 DAA before the charge contests it: its 20 KAS on NO ("the signed price is false").
    let (b, c2, dsp) = charge_contest(&ch, &opener, 10_000_000, &[fund(mek, 63)], &mek, MIN_FEE_RATE).expect("the contest");
    let sg = signed("charge-contest", &b, &w.keys);
    assert_eq!((c2.status, c2.clock), (CHARGE_CONTESTED, 6_600));
    let (contested, dispute) = (nth(&sg, 0, c2, 6_650), nth(&sg, 2, dsp, 6_650));
    let young = stake(&w, &w.judge1, 20, 58, 6_500);
    assert!(charge_contest(&ch, &young, 10_000_000, &[fund(mek, 63)], &mek, MIN_FEE_RATE).is_err(), "a stake younger than the judge's age");
    // Nobody answers: once the round has closed the verdict is NO, and the charge is refuted.
    let (b, c3, _pool) = charge_decide(&dispute, &contested, &[fund(mek, 64)], &mek, MIN_FEE_RATE).expect("the verdict");
    let sg = signed("charge-decide", &b, &w.keys);
    assert_eq!(sg.tx.lock_time, 6_750);
    assert_eq!(c3.status, CHARGE_REFUTED);
    let decided = nth(&sg, 1, c3, 6_760);
    // The slash takes what the verdict's NO side had at risk, 20 of the member's 22 KAS (review C5): 2 KAS and the bond
    // to the charge's author, 18 KAS burned; the member keeps 2 KAS and no hold.
    let (b, out) = charge_settle(&Staked::Member(&committee, 1), &decided, &NO_HOLD, &NO_LATER, 6_800, &[fund(mek, 65)], &mek, MIN_FEE_RATE).expect("the slash");
    let sg = signed("charge-slash", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 22 * KAS1, taken: 20 * KAS1, reward: 2 * KAS1, burned: 18 * KAS1 });
    assert_eq!(sg.tx.outputs[0].value as i64, 47 * KAS1 + COMMITTEE_CARRIER as i64);
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (7 * KAS1, p2pk_spk(&mek)));
    let last = sg.tx.outputs.last().unwrap();
    assert_eq!((last.value as i64, last.script_public_key.clone()), (18 * KAS1, p2sh_spk(&[0x6a])));
    let (after, _, _) = committee.s.slash(1, &decided.s, &cid.as_bytes(), &NO_HOLD, &NO_LATER).unwrap();
    assert_eq!((after.members[1].weight, after.holds[1]), (2 * KAS1, NO_HOLD));
    // Not with another member, and not with a witness that is not the member's holds.
    assert!(charge_settle(&Staked::Member(&committee, 0), &decided, &NO_HOLD, &NO_LATER, 6_800, &[fund(mek, 65)], &mek, MIN_FEE_RATE).is_err());
    assert!(charge_settle(&Staked::Member(&committee, 1), &decided, &[7; 32], &NO_LATER, 6_800, &[fund(mek, 65)], &mek, MIN_FEE_RATE).is_err());
}

#[test]
fn an_upheld_charge_pays_its_bond_to_the_charged_key_and_a_lapsed_one_burns_it() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let member0 = xk(&w.members[0]);
    // Two charges on member 0, for two signed prices: its holds are a chain of the two ids.
    let (t1, t2) = (sign_price(&w.members[0], &rid, &MARKET, 6_500, 10 * KAS1), sign_price(&w.members[0], &rid, &MARKET, 6_501, 10 * KAS1));
    let (b, c1, id1) = charge(&Staked::Member(&w.committee, 0), &MARKET, &t1, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).expect("the first charge");
    let sg1 = signed("committee-charge 1", &b, &w.keys);
    let r1 = nth(&sg1, 0, w.committee.s.charged(0, &id1.as_bytes()).unwrap(), 6_600);
    let ch1 = nth(&sg1, 1, c1, 6_600);
    let (b, c2, id2) = charge(&Staked::Member(&r1, 0), &MARKET, &t2, mek, &[fund(mek, 61)], &mek, MIN_FEE_RATE).expect("the second charge");
    let sg2 = signed("committee-charge 2", &b, &w.keys);
    let r2 = nth(&sg2, 0, r1.s.charged(0, &id2.as_bytes()).unwrap(), 6_610);
    let ch2 = nth(&sg2, 1, c2, 6_610);
    let ids = [id1.as_bytes(), id2.as_bytes()];
    assert_eq!(r2.s.holds[0], hold_add(&hold_add(&NO_HOLD, &ids[0]), &ids[1]));

    // The first is contested (20 KAS on NO) and answered (25 KAS on YES, the lead): upheld.
    let (opener, answer) = (stake(&w, &w.judge1, 20, 57, 6_000), stake(&w, &w.judge2, 25, 56, 6_000));
    let (b, c1c, dsp) = charge_contest(&ch1, &opener, 10_000_000, &[fund(mek, 62)], &mek, MIN_FEE_RATE).expect("the contest");
    let sg = signed("charge-contest", &b, &w.keys);
    let (contested, dispute) = (nth(&sg, 0, c1c, 6_650), nth(&sg, 2, dsp, 6_650));
    let (b, dsp2) = deposit(&dispute, YES, &[answer], &[fund(mek, 63)], &mek, MIN_FEE_RATE, &Trust::Untraced).expect("the answer");
    let sg = signed("the judge's deposit on YES", &b, &w.keys);
    let dispute = nth(&sg, 0, dsp2, 6_700);
    let (b, c1d, _) = charge_decide(&dispute, &contested, &[fund(mek, 64)], &mek, MIN_FEE_RATE).expect("the verdict");
    let sg = signed("charge-decide", &b, &w.keys);
    assert_eq!(c1d.status, CHARGE_UPHELD);
    let decided = nth(&sg, 1, c1d, 6_810);
    // It is the first of two holds: shown with the holds before it (none) and the id after it.
    let (a, later) = hold_witness(&ids, &ids[0]).unwrap();
    assert!(charge_settle(&Staked::Member(&r2, 0), &decided, &NO_HOLD, &NO_LATER, 6_900, &[fund(mek, 65)], &mek, MIN_FEE_RATE).is_err(), "the witness of a single hold");
    let (b, out) = charge_settle(&Staked::Member(&r2, 0), &decided, &a, &later, 6_900, &[fund(mek, 65)], &mek, MIN_FEE_RATE).expect("the clear");
    let sg = signed("charge-clear of an upheld charge", &b, &w.keys);
    assert_eq!(out, Settled::Cleared { bond_to_key: true });
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (5 * KAS1, p2pk_spk(&member0)));
    let r3 = nth(&sg, 0, r2.s.clear(0, &ids[0], &a, &later).unwrap(), 6_900);
    assert_eq!(r3.s.holds[0], hold_add(&NO_HOLD, &ids[1]));
    assert_eq!(r3.s.members, w.committee.s.members);

    // The second is never contested: 300 DAA after it was accepted the hold ends and the bond is burned.
    assert!(charge_settle(&Staked::Member(&r3, 0), &ch2, &NO_HOLD, &NO_LATER, 6_909, &[fund(mek, 66)], &mek, MIN_FEE_RATE).is_err(), "a DAA early");
    let (b, out) = charge_settle(&Staked::Member(&r3, 0), &ch2, &NO_HOLD, &NO_LATER, 6_910, &[fund(mek, 66)], &mek, MIN_FEE_RATE).expect("the lapse");
    let sg = signed("charge-clear of a lapsed charge", &b, &w.keys);
    assert_eq!((out, sg.tx.lock_time), (Settled::Cleared { bond_to_key: false }, 6_910));
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (5 * KAS1, p2sh_spk(&[0x6a])));
    assert_eq!(r3.s.clear(0, &ids[1], &NO_HOLD, &NO_LATER).unwrap().holds, NO_HOLDS);
}

#[test]
fn a_stake_on_its_way_out_is_charged_held_and_slashed() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    // Member 2 signed a false price and left at once: its exit is charged for the time.
    let signed_price = sign_price(&w.members[2], &rid, &MARKET, 6_500, 20 * KAS1);
    let (b, _, e) = leave(&w.committee, 2, &[fund(mek, 60)], &mek, MIN_FEE_RATE).expect("the leave");
    let sg = signed("committee-leave", &b, &w.keys);
    let exit = nth(&sg, 1, e, 6_550);
    let opener = stake(&w, &w.judge1, 20, 59, 6_000);
    let (b, c, cid) = charge(&Staked::Exit(&exit), &MARKET, &signed_price, mek, &[fund(mek, 61)], &mek, MIN_FEE_RATE).expect("the charge of an exit");
    let sg = signed("exit-charge", &b, &w.keys);
    let held = nth(&sg, 0, exit.s.charged(&cid.as_bytes()).unwrap(), 6_600);
    let ch = nth(&sg, 1, c, 6_600);
    // Released or not, a held exit is not withdrawn.
    assert!(held.s.withdraw(held.s.release + 1_000_000).is_err());
    let (b, c2, dsp) = charge_contest(&ch, &opener, 10_000_000, &[fund(mek, 62)], &mek, MIN_FEE_RATE).expect("the contest");
    let sg = signed("charge-contest", &b, &w.keys);
    let (contested, dispute) = (nth(&sg, 0, c2, 6_650), nth(&sg, 2, dsp, 6_650));
    let (b, c3, _) = charge_decide(&dispute, &contested, &[fund(mek, 63)], &mek, MIN_FEE_RATE).expect("the verdict");
    let sg = signed("charge-decide", &b, &w.keys);
    let decided = nth(&sg, 1, c3, 6_760);
    let (b, out) = charge_settle(&Staked::Exit(&held), &decided, &NO_HOLD, &NO_LATER, 6_800, &[fund(mek, 64)], &mek, MIN_FEE_RATE).expect("the slash of an exit");
    let sg = signed("charge-slash of an exit", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 15 * KAS1, taken: 15 * KAS1, reward: 15 * KAS1 / 10, burned: 15 * KAS1 - 15 * KAS1 / 10 });
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (5 * KAS1 + 15 * KAS1 / 10, p2pk_spk(&mek)));
    assert_eq!(sg.tx.outputs.last().unwrap().script_public_key, p2sh_spk(&[0x6a]));
    assert!(sg.tx.outputs.iter().all(|o| o.covenant.is_none()), "nothing of the committee's id or the charge's is left");

    // A charge contested 300 DAA or more after it was accepted lapses whatever the judge says: its bond is burned and
    // the exit, cleared, is withdrawn.
    let signed_price = sign_price(&w.members[2], &rid, &MARKET, 6_501, 10 * KAS1);
    let (b, c, cid) = charge(&Staked::Exit(&exit), &MARKET, &signed_price, mek, &[fund(mek, 65)], &mek, MIN_FEE_RATE).expect("another charge");
    let sg = signed("exit-charge", &b, &w.keys);
    let held = nth(&sg, 0, exit.s.charged(&cid.as_bytes()).unwrap(), 6_600);
    let ch = nth(&sg, 1, c, 6_600);
    let (b, c2, dsp) = charge_contest(&ch, &opener, 10_000_000, &[fund(mek, 66)], &mek, MIN_FEE_RATE).expect("a late contest is still a contest");
    let sg = signed("charge-contest, late", &b, &w.keys);
    let (contested, dispute) = (nth(&sg, 0, c2, 6_900), nth(&sg, 2, dsp, 6_900));
    let (b, c3, _) = charge_decide(&dispute, &contested, &[fund(mek, 67)], &mek, MIN_FEE_RATE).expect("the verdict");
    let sg = signed("charge-decide", &b, &w.keys);
    assert_eq!(c3.status, CHARGE_LAPSED);
    let decided = nth(&sg, 1, c3, 7_010);
    let (b, out) = charge_settle(&Staked::Exit(&held), &decided, &NO_HOLD, &NO_LATER, 7_100, &[fund(mek, 68)], &mek, MIN_FEE_RATE).expect("the clear of an exit");
    let sg = signed("charge-clear of an exit", &b, &w.keys);
    assert_eq!(out, Settled::Cleared { bond_to_key: false });
    let cleared = nth(&sg, 0, held.s.clear(&cid.as_bytes(), &NO_HOLD, &NO_LATER).unwrap(), 7_100);
    assert_eq!(cleared.s, exit.s);
    let b = withdraw(&cleared, &[fund(mek, 69)], &mek, MIN_FEE_RATE).expect("the withdraw");
    signed("exit-withdraw", &b, &w.keys);
}

#[test]
fn the_engine_refuses_a_slash_that_burns_less_and_a_charge_settled_without_its_stake() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let opener = stake(&w, &w.judge1, 20, 59, 6_000);
    let (b, c, cid) = charge(&Staked::Member(&w.committee, 1), &MARKET, &signed_price, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("committee-charge", &b, &w.keys);
    let committee = nth(&sg, 0, w.committee.s.charged(1, &cid.as_bytes()).unwrap(), 6_600);
    let ch = nth(&sg, 1, c, 6_600);
    let (b, c2, dsp) = charge_contest(&ch, &opener, 10_000_000, &[fund(mek, 63)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("charge-contest", &b, &w.keys);
    let (contested, dispute) = (nth(&sg, 0, c2, 6_650), nth(&sg, 2, dsp, 6_650));
    let (b, c3, _) = charge_decide(&dispute, &contested, &[fund(mek, 64)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("charge-decide", &b, &w.keys);
    let decided = nth(&sg, 1, c3, 6_760);
    let (b, _) = charge_settle(&Staked::Member(&committee, 1), &decided, &NO_HOLD, &NO_LATER, 6_800, &[fund(mek, 65)], &mek, MIN_FEE_RATE).unwrap();
    let refused = |what: &str, edit: &dyn Fn(&mut OBuilt)| {
        let mut t = b.clone();
        edit(&mut t);
        match t.sign(&w.keys) {
            Err(_) => {}
            Ok(sg) => assert!(!kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "{what} was accepted"),
        }
    };
    let n = b.tx.outputs.len();
    refused("a sompi less burned", &|t| t.tx.outputs[n - 1].value -= 1);
    refused("the author's share to another key", &|t| t.tx.outputs[1].script_public_key = p2pk_spk(&xk(&w.judge2)));
    refused("a sompi less to the author", &|t| t.tx.outputs[1].value -= 1);
    refused("the committee keeping a sompi less", &|t| t.tx.outputs[0].value -= 1);
}

/// Review C5 (C5-F2): a charged member that forfeits itself with two prices takes nothing, and its charge is still
/// settled: the author gets its bond and the tenth of what it charged.
#[test]
fn a_charged_member_shown_two_prices_keeps_a_tenth_under_its_holds_and_the_charge_is_still_paid() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let opener = stake(&w, &w.judge1, 20, 59, 6_000);
    let (b, c, cid) = charge(&Staked::Member(&w.committee, 1), &MARKET, &signed_price, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("committee-charge", &b, &w.keys);
    assert_eq!(c.at_stake, 22 * KAS1);
    let committee = nth(&sg, 0, w.committee.s.charged(1, &cid.as_bytes()).unwrap(), 6_600);
    let ch = nth(&sg, 1, c, 6_600);
    // The member's own two prices for another signed price: 19.8 KAS burned, nobody paid, 2.2 KAS left under the hold.
    let (a1, a2) = (sign_price(&w.members[1], &rid, &MARKET, 6_400, 10 * KAS1), sign_price(&w.members[1], &rid, &MARKET, 6_400, 11 * KAS1));
    let member1 = xk(&w.members[1]);
    let (b, after) = forfeit_member(&committee, 1, &MARKET, &a1, &a2, &[fund(member1, 61)], &member1, MIN_FEE_RATE).expect("the forfeit of a held member");
    let sg = signed("member-forfeit of a held member", &b, &w.keys);
    assert_eq!((after.members[1].weight, after.holds[1]), (22 * KAS1 / 10, committee.s.holds[1]));
    assert_eq!(sg.tx.outputs[0].value as i64, committee.value as i64 - (22 * KAS1 - 22 * KAS1 / 10));
    assert_eq!(sg.tx.outputs.last().unwrap().value as i64, 22 * KAS1 - 22 * KAS1 / 10);
    let change = sg.tx.outputs[1].value as i64;
    assert_eq!(change, 1_000 * KAS1 - sg.fee as i64, "the sender of the proof is paid nothing");
    let forfeited = nth(&sg, 0, after, 6_620);
    // The same proof again takes nothing more than leaves 1 KAS; the charge runs its course and takes the rest.
    let (_, again) = forfeit_member(&forfeited, 1, &MARKET, &a1, &a2, &[fund(member1, 62)], &member1, MIN_FEE_RATE).expect("once more");
    assert_eq!(again.members[1].weight, KAS1);
    let (b, c2, dsp) = charge_contest(&ch, &opener, 50_000_000, &[fund(mek, 63)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("charge-contest", &b, &w.keys);
    let (contested, dispute) = (nth(&sg, 0, c2, 6_650), nth(&sg, 2, dsp, 6_650));
    let (b, c3, _) = charge_decide(&dispute, &contested, &[fund(mek, 64)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("charge-decide", &b, &w.keys);
    assert_eq!((c3.status, c3.clock), (CHARGE_REFUTED, 20 * KAS1), "the verdict records its NO total");
    let decided = nth(&sg, 1, c3, 6_760);
    let (b, out) = charge_settle(&Staked::Member(&forfeited, 1), &decided, &NO_HOLD, &NO_LATER, 6_800, &[fund(mek, 65)], &mek, MIN_FEE_RATE).expect("the slash of what is left");
    let sg = signed("charge-slash after a forfeit", &b, &w.keys);
    // The author's tenth is of the 20 KAS its verdict could take of the 22 KAS it charged: 2 KAS of the 2.2 left.
    assert_eq!(out, Settled::Slashed { stake: 22 * KAS1 / 10, taken: 22 * KAS1 / 10, reward: 2 * KAS1, burned: 2 * KAS1 / 10 });
    assert_eq!(sg.tx.outputs[1].value as i64, 7 * KAS1);
}

/// Review C6 (C6-F1): a verdict of the stake's own size does not take it whole while it has another hold: 1 KAS stays
/// for the other charge, which is settled against it (its bond is not locked).
#[test]
fn a_stake_with_another_hold_keeps_one_kas_for_it() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let member1 = xk(&w.members[1]);
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    // The accuser's charge, then the member's charge of itself for the same time (paid to itself).
    let (b, ca, ida) = charge(&Staked::Member(&w.committee, 1), &MARKET, &signed_price, mek, &[fund(mek, 60)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("the accuser's charge", &b, &w.keys);
    let r1 = nth(&sg, 0, w.committee.s.charged(1, &ida.as_bytes()).unwrap(), 6_600);
    let cha = nth(&sg, 1, ca, 6_600);
    let (b, cb, idb) = charge(&Staked::Member(&r1, 1), &MARKET, &signed_price, member1, &[fund(mek, 61)], &mek, MIN_FEE_RATE).unwrap();
    let sg = signed("the member's charge of itself", &b, &w.keys);
    let r2 = nth(&sg, 0, r1.s.charged(1, &idb.as_bytes()).unwrap(), 6_605);
    let chb = nth(&sg, 1, cb, 6_605);
    let ids = [ida.as_bytes(), idb.as_bytes()];
    // Both contested and refuted: the accuser's by 20 KAS, the member's own by a stake of its own size (22 KAS).
    let decide = |ch: &OLive<Charge>, kas: i64, n: u8| {
        let opener = stake(&w, &w.judge1, kas, n, 6_000);
        let (b, c2, dsp) = charge_contest(ch, &opener, 50_000_000, &[fund(mek, n + 1)], &mek, MIN_FEE_RATE).unwrap();
        let sg = signed("charge-contest", &b, &w.keys);
        let (contested, dispute) = (nth(&sg, 0, c2, 6_650), nth(&sg, 2, dsp, 6_650));
        let (b, c3, _) = charge_decide(&dispute, &contested, &[fund(mek, n + 2)], &mek, MIN_FEE_RATE).unwrap();
        let sg = signed("charge-decide", &b, &w.keys);
        nth(&sg, 1, c3, 6_760)
    };
    let (da, db) = (decide(&cha, 20, 70), decide(&chb, 22, 80));
    // The member's own slash lands first. It took all 22 KAS and zeroed the holds; now it takes 21 and leaves 1 KAS
    // under the accuser's hold.
    let (a, later) = hold_witness(&ids, &ids[1]).unwrap();
    let (b, out) = charge_settle(&Staked::Member(&r2, 1), &db, &a, &later, 6_800, &[fund(mek, 90)], &mek, MIN_FEE_RATE).expect("the member's own slash");
    let sg = signed("the member's slash of itself", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 22 * KAS1, taken: 21 * KAS1, reward: 22 * KAS1 / 10, burned: 21 * KAS1 - 22 * KAS1 / 10 });
    let (r3s, _, _) = r2.s.slash(1, &db.s, &ids[1], &a, &later).unwrap();
    assert_eq!((r3s.members[1].weight, r3s.holds[1]), (KAS1, hold_add(&NO_HOLD, &ids[0])));
    let r3 = nth(&sg, 0, r3s, 6_800);
    // The accuser's charge is settled against what is left: its bond back, and the last KAS.
    let (b, out) = charge_settle(&Staked::Member(&r3, 1), &da, &NO_HOLD, &NO_LATER, 6_810, &[fund(mek, 91)], &mek, MIN_FEE_RATE).expect("the accuser's slash");
    let sg = signed("the accuser's slash", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: KAS1, taken: KAS1, reward: KAS1, burned: 0 });
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (6 * KAS1, p2pk_spk(&mek)));
    let (r4, _, _) = r3.s.slash(1, &da.s, &ids[0], &NO_HOLD, &NO_LATER).unwrap();
    assert_eq!((r4.members[1], r4.holds[1]), (Member::EMPTY, NO_HOLD));
}
