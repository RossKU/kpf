//! Review C4 of the committee family (docs/audit/REVIEW-C4.md): the first part of stage 2, `KpfCommittee.forfeit` and
//! `KpfExit.forfeit` (a key that signed two prices for one time loses its stake: nine tenths burned, a tenth to whoever
//! sends the proof), and the fixes of review C3 (in `auditC3_tests.rs`, `auditC3_cli.rs`).
//!
//! `c4_exploit_*`: the engine accepts a transaction that takes, freezes or breaks a rule the note states.
//! `c4_bounded_*`: an accepted behaviour whose cost or damage is measured here and that the note should state.
//! `c4_holds_*`: an attack that the contracts refuse.
#![allow(non_snake_case)]

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutput};
use kaspa_consensus_core::Hash;
use kpf_protocol::committee::*;
use kpf_protocol::state::sm8;
use secp256k1::{Keypair, Message, Secp256k1};

const KAS1: i64 = 100_000_000;
const FEED: i64 = 1_000_000_000;

fn burn_spk() -> ScriptPublicKey {
    kaspa_txscript::pay_to_script_hash_script(&[0x6a])
}

fn full() -> LCommittee {
    committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS })
}

/// A member key's signature over a signed price whose price is the given eight bytes (what `sign_price` signs for a number).
fn sign_raw(kp: &Keypair, committee: &[u8; 32], market: &[u8; 32], t0: &[u8], price: &[u8]) -> [u8; 64] {
    let msg = [PRICE_TAG, &committee[..], &market[..], t0, price].concat();
    *Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(blake2b(&msg)), kp).as_ref()
}

/// The six arguments of a forfeit after the member index: the market, the signed price, two prices and their signatures.
fn proof_for(committee: &[u8; 32], market: &[u8; 32], t0: (i64, i64), p: (i64, i64), keys: (&Keypair, &Keypair)) -> Vec<Arg> {
    let a = sign_price(keys.0, committee, market, t0.0, p.0);
    let b = sign_price(keys.1, committee, market, t0.1, p.1);
    vec![bv(&MARKET), b8(t0.0), b8(p.0), bv(&a.sig), b8(p.1), bv(&b.sig)]
}

fn proof(k: &Keypair) -> Vec<Arg> {
    proof_for(&rid(), &MARKET, (6_500, 6_500), (80_000, 80_001), (k, k))
}

/// Inputs: the committee (forfeit of member i), the sender. Outputs: the committee with the member emptied at 0, the sender's
/// change, the burn last.
fn member_forfeit_tx(l: &LCommittee, i: usize, args: Vec<Arg>, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (r, w, burn) = l.s.forfeit(i).unwrap_or((l.s, 0, 0));
    let id = l.id.unwrap();
    let mut outs = vec![out((l.value as i64 - w) as u64, committee_spk(&r), bind(0, id)), change(1), out(burn.max(1) as u64, burn_spk(), None)];
    tweak(&mut outs);
    let mut a = vec![iv(i as i64)];
    a.extend(args);
    build(&[committee_in(l, "forfeit", a), exec_in(60)], outs)
}

fn exit_forfeit_tx(l: &LExit, args: Vec<Arg>, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let burn = l.s.forfeit().unwrap_or(0);
    let mut outs = vec![change(1), change(1), out(burn.max(1) as u64, burn_spk(), None)];
    tweak(&mut outs);
    build_ext(&[exit_in(l, "forfeit", args), exec_in(60)], outs, lock, &[])
}

/// The pushes of a signature script (data pushes and the small-number opcodes), each as (raw encoding, data).
fn pushes(ss: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let (mut v, mut i) = (vec![], 0);
    while i < ss.len() {
        let op = ss[i];
        let (head, len) = match op {
            0x00 => (1, 0),
            0x01..=0x4b => (1, op as usize),
            0x4c => (2, ss[i + 1] as usize),
            0x4d => (3, u16::from_le_bytes([ss[i + 1], ss[i + 2]]) as usize),
            0x4f | 0x51..=0x60 => (1, 0),
            _ => panic!("not a push: {op:#x}"),
        };
        v.push((ss[i..i + head + len].to_vec(), ss[i + head..i + head + len].to_vec()));
        i += head + len;
    }
    v
}

/// `t` with the pushes `at` of input 0's signature script replaced by pushes of other bytes (of any length: the
/// kit's encoder writes only arguments of the declared widths).
fn with_pushes(t: &T, at: &[(usize, Vec<u8>)]) -> T {
    let mut tx = t.0.clone();
    let mut items = pushes(&tx.inputs[0].signature_script);
    for (j, data) in at {
        items[*j].0 = push(data);
    }
    tx.inputs[0].signature_script = items.into_iter().flat_map(|x| x.0).collect();
    tx.finalize();
    (tx, t.1.clone())
}

// ---------------------------------------------------------------- what is a proof

#[test]
fn c4_holds_only_two_prices_of_one_key_for_one_price_of_one_market_and_committee_slash_a_member() {
    let l = full();
    let k1 = member_kp(1);
    let good = member_forfeit_tx(&l, 1, proof(&k1), |_| {});
    must("a genuine proof", &good);
    // Signatures of the same two prices for another committee, another market, two signed prices; by another key; one price twice.
    for (what, args) in [
        ("signed for another committee", proof_for(&[0x53; 32], &MARKET, (6_500, 6_500), (80_000, 80_001), (&k1, &k1))),
        ("signed for another market", proof_for(&rid(), &[0x4e; 32], (6_500, 6_500), (80_000, 80_001), (&k1, &k1))),
        ("two signed prices", proof_for(&rid(), &MARKET, (6_500, 6_501), (80_000, 80_001), (&k1, &k1))),
        ("the second price by another member", proof_for(&rid(), &MARKET, (6_500, 6_500), (80_000, 80_001), (&k1, &member_kp(0)))),
        ("one price twice", proof_for(&rid(), &MARKET, (6_500, 6_500), (80_000, 80_000), (&k1, &k1))),
    ] {
        assert!(!accepted(&member_forfeit_tx(&l, 1, args, |_| {})), "{what}");
    }
    // The argument widths. The pushes of the signature script: the member, the market (32), the signed price (8), a price (8),
    // its signature (64), the other price (8), its signature (64), the entry's tag, the script.
    let items = pushes(&good.0.inputs[0].signature_script);
    assert_eq!(items[1..7].iter().map(|x| x.1.len()).collect::<Vec<_>>(), vec![32, 8, 8, 64, 8, 64]);
    let t0 = sm8(6_500);
    must("the same pushes written again", &with_pushes(&good, &[(2, t0.to_vec()), (3, sm8(80_000).to_vec())]));
    // The same signed bytes cut elsewhere: the signed price as 7 bytes and each price as 9 (the signed price's last byte first); the
    // market as 31 bytes and the signed price as 9. Both signatures verify over the same messages; the widths are refused.
    let split = |p: i64| [vec![t0[7]], sm8(p).to_vec()].concat();
    refused("the signed price of 7 bytes, the prices of 9", &with_pushes(&good, &[(2, t0[..7].to_vec()), (3, split(80_000)), (5, split(80_001))]), 0);
    refused("the market of 31 bytes, the signed price of 9", &with_pushes(&good, &[(1, MARKET[..31].to_vec()), (2, [vec![MARKET[31]], t0.to_vec()].concat())]), 0);
    // What that cut would have made of an honest member: one price (0.0003 KAS) signed for the two signed prices 327,680 and
    // 393,216, whose eight bytes agree in their first two. With the signed price given as those two bytes and each "price" as
    // the signed price's other six and the price, the two signatures would be of one "signed price" and two "prices".
    let (ta, tb, p) = (327_680i64, 393_216i64, 30_000i64);
    let (sa, sb) = (sign_price(&k1, &rid(), &MARKET, ta, p), sign_price(&k1, &rid(), &MARKET, tb, p));
    let cut = |t: i64| [sm8(t)[2..].to_vec(), sm8(p).to_vec()].concat();
    let honest = with_pushes(&good, &[(2, sm8(ta)[..2].to_vec()), (3, cut(ta)), (4, sa.sig.to_vec()), (5, cut(tb)), (6, sb.sig.to_vec())]);
    refused("an honest member's two signed prices read as one time and two prices", &honest, 0);
    // Zero has two encodings in eight bytes. A member that signed both (no honest signer writes the second) signed one
    // price: the contract compares the numbers.
    let neg_zero = [0u8, 0, 0, 0, 0, 0, 0, 0x80];
    let (z1, z2) = (sign_raw(&k1, &rid(), &MARKET, &t0, &sm8(0)), sign_raw(&k1, &rid(), &MARKET, &t0, &neg_zero));
    let zeros = with_pushes(&good, &[(3, sm8(0).to_vec()), (4, z1.to_vec()), (5, neg_zero.to_vec()), (6, z2.to_vec())]);
    refused("zero and negative zero", &zeros, 0);
    // The control: zero and one are two prices.
    let z3 = sign_raw(&k1, &rid(), &MARKET, &t0, &sm8(1));
    must("zero and one sompi", &with_pushes(&good, &[(3, sm8(0).to_vec()), (4, z1.to_vec()), (5, sm8(1).to_vec()), (6, z3.to_vec())]));
    // The burn script is the hash of the one-byte script OP_RETURN: nobody spends it.
    assert_eq!(burn_spk().script()[2..34], blake2b(&[0x6a]));
}

// ---------------------------------------------------------------- what a slash moves

#[test]
fn c4_holds_a_slash_moves_the_slashed_stake_and_nothing_else() {
    let l = full();
    let k1 = member_kp(1);
    let good = member_forfeit_tx(&l, 1, proof(&k1), |_| {});
    must("the forfeit of member 1 (22 KAS)", &good);
    // The committee keeps everything but the member's weight; 19.8 KAS are burned; the sender's tenth is 2.2 KAS.
    let (r, w, burn) = l.s.forfeit(1).unwrap();
    assert_eq!((w, burn, good.0.outputs[0].value as i64, good.0.outputs[2].value as i64), (22 * KAS1, 198 * KAS1 / 10, l.value as i64 - w, burn));
    assert!(r.backed(l.value as i64 - w) && r.members[1] == Member::EMPTY);
    refused("the burn one sompi short", &member_forfeit_tx(&l, 1, proof(&k1), |o| o[2].value -= 1), 0);
    refused("the burn not at the mirror index", &member_forfeit_tx(&l, 1, proof(&k1), |o| o.swap(1, 2)), 0);
    refused("the committee a sompi short", &member_forfeit_tx(&l, 1, proof(&k1), |o| o[0].value -= 1), 0);
    refused("another member emptied", &member_forfeit_tx(&l, 1, proof(&k1), |o| o[0].script_public_key = committee_spk(&l.s.forfeit(0).unwrap().0)), 0);
    refused("a second output of the id", &member_forfeit_tx(&l, 1, proof(&k1), |o| o.insert(1, out(KAS1 as u64, exit_spk(&Exit { key: x(&k1), amount: KAS1, release: 0, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge }), bind(0, RID)))), 0);

    // Two committees of two ids slashed in one transaction: each burn at its own mirror index; one burn output for both
    // is refused.
    let rid2 = Hash::from_bytes([0x53; 32]);
    let l2 = Live { op: outpoint(5), id: Some(rid2), ..l.clone() };
    let args2 = proof_for(&rid2.as_bytes(), &MARKET, (6_500, 6_500), (80_000, 80_001), (&k1, &k1));
    let ins = |a1: Vec<Arg>, a2: Vec<Arg>| {
        let with_member = |a: Vec<Arg>| [vec![iv(1)], a].concat();
        [committee_in(&l, "forfeit", with_member(a1)), committee_in(&l2, "forfeit", with_member(a2)), exec_in(60)]
    };
    let conts = || vec![out((l.value as i64 - w) as u64, committee_spk(&r), bind(0, RID)), out((l.value as i64 - w) as u64, committee_spk(&r), bind(1, rid2)), change(1)];
    let mut both = conts();
    both.extend([out(burn as u64, burn_spk(), None), out(burn as u64, burn_spk(), None)]);
    must("two slashes, two burns", &build(&ins(proof(&k1), args2.clone()), both));
    let mut shared = conts();
    shared.push(out(2 * burn as u64, burn_spk(), None));
    assert!(!accepted(&build(&ins(proof(&k1), args2), shared)), "one burn for two slashes");

    // A member's slash and its own exit's slash in one transaction (one id twice): both refuse.
    let (left, e) = l.s.leave(0).unwrap();
    let ex: LExit = Live { op: outpoint(6), value: e.amount as u64, daa: 0, id: Some(RID), s: e };
    let _ = left;
    let k0 = member_kp(0);
    let outs = vec![out((l.value as i64 - w) as u64, committee_spk(&r), bind(0, RID)), change(1), change(1), out(ex.s.forfeit().unwrap() as u64, burn_spk(), None), out(burn as u64, burn_spk(), None)];
    let t = build(&[committee_in(&l, "forfeit", [vec![iv(1)], proof(&k1)].concat()), exit_in(&ex, "forfeit", proof(&k0)), exec_in(60)], outs);
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[0].is_err() && v[1].is_err(), "a committee and an exit of one id slashed together");

    // A member that slashes itself gets its tenth at once: 2.2 KAS of 22. Leaving returns all 22 at 5,000 + 1,000 + 500.
    let (_, exit) = l.s.leave(1).unwrap();
    assert_eq!((w - burn, exit.amount, exit.release), (22 * KAS1 / 10, 22 * KAS1, 6_500));
}

// ---------------------------------------------------------------- a slashed member, a stake on its way out

#[test]
fn c4_bounded_a_slashed_member_stays_in_its_epochs_epoch_tables() {
    // The committee rolled at 6,000; its epoch tables hold the table of that moment. Member 1 (22 KAS of 100) is slashed at
    // 6,100: the committee's table loses it, the epoch's epoch tables do not. Until 7,000 its signature still counts for 22
    // KAS with no stake behind it, and a quorum still needs it (members 0 and 2 alone hold 45 of the 67).
    let l = committee(&Committee { p: rparams(), from: 6_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let k1 = member_kp(1);
    let t = member_forfeit_tx(&l, 1, proof(&k1), |_| {});
    must("the slash at 6,100", &t);
    let slashed = live(&t, 0, 6_100, &l.s.forfeit(1).unwrap().0);
    let lt = epoch_epoch_table();
    let m = cmarket().id();
    let votes = |members: &[usize]| {
        let mut a = Attestation { market: m, t0: 6_900, p: FEED, bits: 0, prices: [0; MEMBERS], sigs: [[0; 64]; MEMBERS] };
        for i in members {
            a.bits |= 1 << i;
            a.prices[*i] = FEED;
            a.sigs[*i] = sign_price(&member_kp(*i), &rid(), &m, 6_900, FEED).sig;
        }
        a
    };
    must("the slashed member's signature makes the quorum at 6,900", &attest_tx(&lt, &votes(&[0, 1, 2]), 6_900, |_| {}));
    refused("without it there is none", &attest_tx(&lt, &votes(&[0, 2]), 6_900, |_| {}), 0);
    // The next roll writes the table without it: 78 KAS, a quorum of 52.26.
    let roll = roll_tx(&slashed, 1, 7_000, |_| {});
    must("the roll at 7,000", &roll);
    let (_, tk) = slashed.s.roll(1, 7_000, slashed.value as i64).unwrap();
    assert_eq!((tk.total, tk.members[1], tk.quorum()), (78 * KAS1, Member::EMPTY, 5_226_000_000));
    // The epoch is 1,000 DAA here and 3,000 (5 minutes) on the testnet defaults: that long at most.
    assert_eq!(CommitteeParams::testnet().epoch, 3_000);
}

#[test]
fn c4_holds_a_stake_on_its_way_out_is_slashed_until_its_withdrawal_lands() {
    // Member 1 signed two prices and leaves before the proof lands: its 22 KAS are an exit released at 6,500.
    let l = full();
    let k1 = member_kp(1);
    let leave = leave_tx(&l, 1, &k1, |_| {});
    must("the leave", &leave);
    let (left, e) = l.s.leave(1).unwrap();
    let after = live(&leave, 0, 5_100, &left);
    let ex = live(&leave, 1, 5_100, &e);
    assert_eq!(e.release, 6_500);
    // The committee no longer holds the key; the exit does, before its release and after it, until the withdrawal lands.
    assert!(!accepted(&member_forfeit_tx(&after, 1, proof(&k1), |_| {})), "the empty member");
    must("the exit slashed before its release", &exit_forfeit_tx(&ex, proof(&k1), 5_200, |_| {}));
    must("the exit slashed after its release", &exit_forfeit_tx(&ex, proof(&k1), 9_000, |_| {}));
    must("the withdrawal, the other spend of the same UTXO", &withdraw_tx(&ex, 6_500, |_| {}));
    refused("no withdrawal before the release", &withdraw_tx(&ex, 6_499, |_| {}), 0);
    refused("the burn one sompi short", &exit_forfeit_tx(&ex, proof(&k1), 5_200, |o| o[2].value -= 1), 0);
    refused("the burn to a key", &exit_forfeit_tx(&ex, proof(&k1), 5_200, |o| o[2].script_public_key = p2pk_spk(&keypair(77))), 0);
    refused("an output of the committee's id", &exit_forfeit_tx(&ex, proof(&k1), 5_200, |o| o[0] = out(KAS1 as u64, exit_spk(&e), bind(0, RID))), 0);
    assert!(!accepted(&exit_forfeit_tx(&ex, proof(&member_kp(0)), 5_200, |_| {})), "a proof against another key");
    // An outbid member's exit is the same object: member 7 (1 KAS) outbid, its key's two prices burn 0.9 KAS of it.
    let join = join_tx(&l, 7, x(&keypair(77)), 10 * KAS1, |_| {});
    let (_, outbid, _) = l.s.join(7, x(&keypair(77)), 10 * KAS1).unwrap();
    let ox = live(&join, 1, 5_100, &outbid.unwrap());
    must("the outbid member's exit slashed", &exit_forfeit_tx(&ox, proof(&member_kp(7)), 5_200, |_| {}));
    assert_eq!(ox.s.forfeit().unwrap(), 90_000_000);
    // The time a proof has: the exit time after the end of the last epoch whose epoch tables hold the key: 500 DAA here,
    // 72,000 (2 hours) on the testnet defaults.
    assert_eq!(CommitteeParams::testnet().exit, 72_000);
}
