//! Review C5 (docs/audit/REVIEW-C5.md), on the engine with hand-made signature scripts: what `KpfCharge.settle` reads
//! of the input it names (a length, five bytes at 310, an 8-byte first push), and the layouts the committee, the exit,
//! the charge and the judge read in each other. The flows are in crates/kpf-protocol/tests/auditC5_builders.rs.
//!
//! `c5_holds_` an attack the engine refuses.

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::tx::UtxoEntry;
use kaspa_consensus_core::Hash;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::CObj;

const CID: Hash = Hash::from_bytes([0xc1; 32]);

fn burn_spk() -> kaspa_consensus_core::tx::ScriptPublicKey {
    kaspa_txscript::pay_to_script_hash_script(&[0x6a])
}

/// The fixture committee whose member 1 (22 KAS) is held for the charge CID.
fn held() -> LCommittee {
    let mut r = Committee { p: rparams(), from: 6_000, members: table(TOP3), holds: NO_HOLDS };
    r.holds[1] = hold_add(&NO_HOLD, &CID.as_bytes());
    committee(&r)
}

/// The charge CID on member 1 (it signed 20 for the time 6,500; it held 22 KAS), refuted with 22 KAS on NO, holding its bond.
fn refuted() -> Live<Charge> {
    let c = Charge { status: CHARGE_REFUTED, clock: 22 * KAS, ..Charge::new(rid(), x(&member_kp(1)), x(&keypair(77)), MARKET, 6_500, 20 * KAS, 22 * KAS, cparams()) };
    Live { op: outpoint(9), value: (5 * KAS) as u64, daa: 6_760, id: Some(CID), s: c }
}

fn charge_in(l: &Live<Charge>, r: i64) -> In {
    let art = charge_art(&l.s);
    In { outpoint: l.op, utxo: UtxoEntry::new(l.value, p2sh(&art), l.daa, false, l.id), spend: Spend::Signed { art, entry: "settle", args: vec![b8(r)] } }
}

/// In 0 the committee (`slash`), 1 the charge (`settle`), 2 the executor; out 0 the committee, 1 the author, 2 change, 3 the burn.
fn slash_tx(l: &LCommittee, ch: &Live<Charge>) -> T {
    let (r2, w, reward) = l.s.slash(1, &ch.s, &CID.as_bytes(), &NO_HOLD, &NO_LATER).unwrap();
    assert_eq!((w, reward), (22 * KAS, 22 * KAS / 10));
    let outs = vec![
        out(l.value - w as u64, committee_spk(&r2), bind(0, RID)),
        out(ch.value + (w / 10) as u64, p2pk_spk(&keypair(77)), None),
        change(1),
        out((w - w / 10) as u64, burn_spk(), None),
    ];
    build(&[committee_in(l, "slash", vec![b8(1), b8(1), bv(&NO_HOLD), bv(&[0u8; 256])]), charge_in(ch, 0), exec_in(60)], outs)
}

/// `orig` behind an 8-byte push of `index` and one push of filler that carries `tag` at offset 310 of the script, the
/// whole `total` bytes long: everything `KpfCharge.settle` reads of an input.
fn padded(orig: &[u8], index: u8, total: usize, tag: &[u8]) -> Vec<u8> {
    let n = total - 9 - 3 - orig.len();
    let mut junk = vec![0x11u8; n];
    junk[298..303].copy_from_slice(tag);
    let mut s = vec![0x08, index, 0, 0, 0, 0, 0, 0, 0, 0x4d];
    s.extend_from_slice(&(n as u16).to_le_bytes());
    s.extend_from_slice(&junk);
    s.extend_from_slice(orig);
    s
}

fn verdicts(t: &T) -> Vec<String> {
    run(&t.0, &t.1).expect("the covenant context").iter().map(|r| format!("{r:?}")).collect()
}

/// (b) `settle` accepts any input of the committee's id whose signature script has the length of a slash, the tag of
/// one at [310, 315) and its index in an 8-byte first push. A retiring epoch table and a withdrawing exit can be given all
/// three with filler below their own arguments, and the charge's script then PASSES: what refuses the transaction is
/// the engine's clean-stack rule on the epoch table and the exit, whose entries leave the filler on the stack. Without
/// filler nothing reaches the shape: no entry of the committee's three templates takes an argument of free length.
#[test]
fn c5_holds_a_epoch_table_or_an_exit_dressed_as_a_slash_passes_the_charge_and_fails_its_own_clean_stack() {
    let (l, ch) = (held(), refuted());
    let good = slash_tx(&l, &ch);
    must("the slash", &good);
    let ss = good.0.inputs[0].signature_script.clone();
    let committee_len = CObj::Committee(l.s).redeem().len();
    assert_eq!((committee_len, ss.len(), ss[0], ss[310]), (17_417, 318 + committee_len, 0x08, 0x04));
    let tag = ss[310..315].to_vec();

    // An epoch table of the committee retires beside the charge, dressed as the committee's slash.
    let (_, tk, _) = roll_outs(&committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: NO_HOLDS }), 1);
    let lt = epoch_table(&tk);
    let plain = build_ext(&[epoch_table_in(&lt, "retire", vec![]), charge_in(&ch, 0), exec_in(60)], vec![change(1)], 8_000, &[]);
    let v = verdicts(&plain);
    assert!(v[0].starts_with("Ok") && v[1].starts_with("Err"), "a retire as it is: the epoch table passes, the charge does not: {v:?}");
    let mut t = plain.clone();
    t.0.inputs[0].signature_script = padded(&plain.0.inputs[0].signature_script, 1, 318 + committee_len, &tag);
    let v = verdicts(&t);
    println!("an epoch table dressed as a slash: epoch table {}, charge {}", v[0], v[1]);
    assert!(v[1].starts_with("Ok"), "the charge reads nothing else of the input it names: {}", v[1]);
    assert!(v[0].contains("CleanStack"), "{}", v[0]);

    // An exit of the committee withdraws beside the charge, dressed the same way (to the committee's length: at the exit's
    // own length the five bytes at 310 are its real tag).
    let e = Exit { key: x(&member_kp(2)), amount: 15 * KAS, release: 6_500, hold: NO_HOLD, charge: rparams().charge };
    let le = Live { op: outpoint(3), value: (15 * KAS) as u64, daa: 0, id: Some(RID), s: e };
    let outs = vec![out(e.amount as u64, p2pk_spk(&member_kp(2)), None), change(1)];
    let plain = build_ext(&[exit_in(&le, "withdraw", vec![]), charge_in(&ch, 0), exec_in(60)], outs, 6_500, &[]);
    let v = verdicts(&plain);
    assert!(v[0].starts_with("Ok") && v[1].starts_with("Err"), "{v:?}");
    let exit_len = CObj::Exit(e).redeem().len();
    assert_eq!((exit_len, plain.0.inputs[0].signature_script.len()), (4_639, 8 + exit_len));
    let mut t = plain.clone();
    t.0.inputs[0].signature_script = padded(&plain.0.inputs[0].signature_script, 1, 318 + committee_len, &tag);
    let v = verdicts(&t);
    println!("an exit dressed as a slash: exit {}, charge {}", v[0], v[1]);
    assert!(v[1].starts_with("Ok") && v[0].contains("CleanStack"), "{v:?}");

    // The engine does not want minimal pushes: the real slash with its first push written with OP_PUSHDATA1 still
    // passes the committee's script. The charge refuses it (its byte 0 is not 0x08, its length is one more).
    let mut t = good.clone();
    let mut s = vec![0x4c];
    s.extend_from_slice(&ss);
    t.0.inputs[0].signature_script = s;
    let v = verdicts(&t);
    println!("the slash with a long first push: committee {}, charge {}", v[0], v[1]);
    assert!(v[0].starts_with("Ok") && v[1].starts_with("Err"), "{v:?}");
    // Nor minimal numbers: an `int` argument written as eight bytes (the first push of topUp as 08 01 00 .. 00) is
    // accepted, so an entry of the committee with an `int` first argument can begin as a slash does. What keeps it from
    // serving a charge is the length: with every push written the longest way, no entry but slash and clear puts
    // 318 bytes before the committee's script with its own tag at 310 (REVIEW-C5, "Checked and holds", the table of lengths).
    let tu = top_up_tx(&committee(&l.s), 1, KAS, |_| {});
    must("a top-up", &tu);
    let mut t = tu.clone();
    let orig = &tu.0.inputs[0].signature_script;
    assert_eq!(orig[0], 0x51, "member 1 is pushed as OP_1");
    let mut s = vec![0x08, 1, 0, 0, 0, 0, 0, 0, 0];
    s.extend_from_slice(&orig[1..]);
    t.0.inputs[0].signature_script = s;
    let v = verdicts(&t);
    println!("a top-up whose member index is eight bytes: {}", v[0]);
    assert!(v[0].starts_with("Ok"), "{v:?}");
    assert_eq!((t.0.inputs[0].signature_script[0], t.0.inputs[0].signature_script.len()), (0x08, tu.0.inputs[0].signature_script.len() + 8));
    // The charge at another index than the one the stake names: the stake reads the executor's input as its charge.
    let mut t = good.clone();
    t.0.inputs.swap(1, 2);
    t.1.swap(1, 2);
    assert!(!accepted(&t));
}

/// The offsets the three templates and the judge read in each other's scripts are the models' bytes.
#[test]
fn c5_holds_the_layouts_the_templates_read_in_each_other() {
    let (l, ch) = (held(), refuted());
    let p = cparams().state_bytes();
    assert_eq!(p.len(), 107);
    // The committee: the holds' payload from 688, the judge block and the charge parameters [1168, 1275).
    let rs = CObj::Committee(l.s).redeem();
    assert_eq!(&rs[688..1168], l.s.holds.concat().as_slice());
    assert_eq!(&rs[1168..1275], p.as_slice());
    // An exit: the hold's payload [53, 85), the same 107 bytes [85, 192).
    let e = Exit { key: x(&member_kp(1)), amount: 22 * KAS, release: 7_500, hold: l.s.holds[1], charge: rparams().charge };
    let es = CObj::Exit(e).redeem();
    assert_eq!((&es[53..85], &es[85..192]), (&l.s.holds[1][..], p.as_slice()));
    // A charge: 284 bytes of state from 1; the status [0, 9), the NO total's payload at 10, the charged key's at 52,
    // the paid key's at 85, the stake as it was charged at 169, the same 107 bytes at its end.
    let cs = CObj::Charge(ch.s).redeem();
    let st = &cs[1..285];
    assert_eq!(st, ch.s.state_bytes().as_slice());
    assert_eq!(&st[0..9], &[0x08, 4, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!((&st[19..51], &st[52..84], &st[85..117], &st[177..284]), (&rid()[..], &x(&member_kp(1))[..], &x(&keypair(77))[..], p.as_slice()));
    assert_eq!((&st[10..18], &st[169..177]), (&(22 * KAS).to_le_bytes()[..], &(22 * KAS).to_le_bytes()[..]));
    // The dispute's NO total, which the verdict copies: a KpfStake's state [199, 207).
    let d = kpf_protocol::judge::Js::opened(cparams().judge, [7; 32], 6_600, 22 * KAS).state_bytes();
    assert_eq!((d[198], &d[199..207]), (0x08, &(22 * KAS).to_le_bytes()[..]));
    // The judge block is a KpfStake's state [42, 129), byte for byte.
    let jm = cparams().judge;
    let js = kpf_protocol::judge::Js::free(x(&keypair(78)), jm).state_bytes();
    assert_eq!((js.len(), &js[42..129]), (270, &p[2..89]));
}
