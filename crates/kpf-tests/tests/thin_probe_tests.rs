//! Probe T1: a position that carries no logic (`contracts/probes/T1Slot.sil`) moved by the script of one operation
//! presented as another input (`contracts/probes/T1Add.sil`, "the owner adds margin"). The question is the size: every
//! spend of a KpfPairC carries 13,337 bytes of script and of a KpfCrossC 7,045, whatever it does (an added margin is a
//! transaction of 13,824 and 7,532 bytes). The numbers are printed and the decision rests on them; the negatives show
//! that the wiring binds (the position accepts only its logic, the logic checks only the position that named it).

mod common;

use common::*;
use kaspa_consensus_core::tx::{TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::state::sm8;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

const XID: Hash = Hash::from_bytes([0x71; 32]);
const KAS1: u64 = 100_000_000;

fn blake2b(data: &[u8]) -> [u8; 32] {
    kaspa_txscript::pay_to_script_hash_script(data).script()[2..34].try_into().expect("P2SH script")
}

fn state_hash(a_key: &[u8; 32], a_margin: i64, b_margin: i64) -> [u8; 32] {
    let mut m = vec![0x20];
    m.extend_from_slice(a_key);
    m.push(0x08);
    m.extend_from_slice(&sm8(a_margin));
    m.push(0x08);
    m.extend_from_slice(&sm8(b_margin));
    blake2b(&m)
}

fn logic(version: i64) -> SilAbiArtifact {
    compile("probes/T1Add.sil", &[ArtifactValue::Int(version)])
}

fn slot(logic_a: &SilAbiArtifact, h: [u8; 32]) -> SilAbiArtifact {
    compile("probes/T1Slot.sil", &[ArtifactValue::Bytes(blake2b(&bytecode(logic_a)).to_vec()), ArtifactValue::Bytes(vec![0x99; 32]), ArtifactValue::Bytes(h.to_vec())])
}

fn owner() -> Keypair {
    keypair(31)
}
fn xkey(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
}

struct Shape {
    slot_logic: SilAbiArtifact,
    run_logic: SilAbiArtifact,
    named: i64,
    shown: (i64, i64),
    signer: Keypair,
    delta: i64,
}

/// Inputs: the position (go), the logic (run), funding. Outputs: the position with the new state's hash, the logic
/// as it was, change.
fn add_tx(sh: &Shape, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let a = xkey(&owner());
    let (am, bm) = (20 * KAS1 as i64, 30 * KAS1 as i64);
    let s0 = slot(&sh.slot_logic, state_hash(&a, am, bm));
    let s1 = slot(&sh.slot_logic, state_hash(&a, am + sh.delta, bm));
    let slot_len = bytecode(&s0).len() as i64;
    let value = 50 * KAS1 + 20_000_000;
    let ins = vec![
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&s0), 100, false, Some(XID)), spend: Spend::Signed { art: s0, entry: "go", args: vec![Arg::V(ArtifactValue::Int(1))] } },
        In {
            outpoint: outpoint(11),
            utxo: UtxoEntry::new(KAS1, p2sh(&sh.run_logic), 90, false, None),
            spend: Spend::Signed {
                art: sh.run_logic.clone(),
                entry: "run",
                args: vec![
                    Arg::V(ArtifactValue::Bytes(sm8(sh.named).to_vec())),
                    Arg::V(ArtifactValue::Int(slot_len)),
                    Arg::V(ArtifactValue::Bytes(a.to_vec())),
                    Arg::V(ArtifactValue::Int(sh.shown.0)),
                    Arg::V(ArtifactValue::Int(sh.shown.1)),
                    Arg::V(ArtifactValue::Int(sh.delta)),
                    Arg::Sig(sh.signer),
                ],
            },
        },
        In { outpoint: outpoint(12), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
    ];
    let mut outs = vec![out((value as i64 + sh.delta) as u64, p2sh(&s1), bind(0, XID)), out(KAS1, p2sh(&sh.run_logic), None), out(999 * KAS1, p2pk_spk(&keypair(60)), None)];
    tweak(&mut outs);
    build_ext(&ins, outs, 0, &[])
}

fn good() -> Shape {
    let l = logic(1);
    Shape { slot_logic: l.clone(), run_logic: l, named: 0, shown: (20 * KAS1 as i64, 30 * KAS1 as i64), signer: owner(), delta: 5 * KAS1 as i64 }
}

fn all_ok(t: &(kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>)) -> bool {
    run(&t.0, &t.1).map(|v| v.iter().all(|r| r.is_ok())).unwrap_or(false)
}
fn fails(t: &(kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>), i: usize) -> bool {
    run(&t.0, &t.1).map(|v| v[i].is_err()).unwrap_or(false)
}

#[test]
fn t1_a_position_without_logic_is_moved_by_the_script_of_one_operation() {
    let g = good();
    let t = add_tx(&g, |_| {});
    assert!(all_ok(&t), "{:?}", run(&t.0, &t.1));
    let a = xkey(&owner());
    let slot_bytes = bytecode(&slot(&g.slot_logic, state_hash(&a, 0, 0))).len();
    let logic_bytes = bytecode(&g.run_logic).len();
    let mut sized = t.0.clone();
    for i in sized.inputs.iter_mut() {
        i.compute_commit = kaspa_consensus_core::mass::ComputeBudget(2_000).into();
    }
    let size = kpf_protocol::tx::masses(&sized, &t.1).size;
    let scripts: Vec<usize> = t.0.inputs.iter().map(|i| i.signature_script.len()).collect();
    println!("T1: signature scripts by input (position, logic, funding): {scripts:?}");
    println!("T1: the position's script {slot_bytes} bytes, the logic of an added margin {logic_bytes} bytes, the whole transaction {size} bytes");
    println!("T1: against 7,532 bytes (KpfCrossC.addMargin) and 13,824 bytes (KpfPairC.addMargin): {:.1} and {:.1} times smaller", 7_532.0 / size as f64, 13_824.0 / size as f64);
    assert!(slot_bytes < 250 && logic_bytes < 900 && size < 2_000, "{slot_bytes} {logic_bytes} {size}");

    // The values: the position holds what was added, the logic goes on whole.
    assert!(fails(&add_tx(&g, |o| o[0].value -= 1), 1), "a position a sompi short");
    assert!(fails(&add_tx(&g, |o| o[1].value -= 1), 1), "a logic UTXO a sompi short");
    assert!(fails(&add_tx(&g, |o| o[1].script_public_key = p2pk_spk(&keypair(60))), 1), "the logic UTXO taken");
    // The state: the logic is shown the state the position's hash binds, and writes the hash of the new one.
    assert!(fails(&add_tx(&Shape { shown: (21 * KAS1 as i64, 30 * KAS1 as i64), ..good() }, |_| {}), 1), "another state shown");
    let other = slot(&g.slot_logic, state_hash(&a, 99, 99));
    assert!(fails(&add_tx(&g, |o| o[0].script_public_key = p2sh(&other)), 1), "another state written");
    assert!(fails(&add_tx(&Shape { delta: 0, ..good() }, |_| {}), 1), "nothing added");
    // The signature: the owner's, checked in the logic's script.
    assert!(fails(&add_tx(&Shape { signer: keypair(32), ..good() }, |_| {}), 1), "signed by another key");
}

#[test]
fn t1_the_position_accepts_only_its_logic_and_the_logic_checks_the_position_that_named_it() {
    // A script that is not among the position's (the same source, another constant): the position refuses it,
    // though that script itself is satisfied.
    let stranger = Shape { run_logic: logic(1), slot_logic: logic(2), ..good() };
    assert!(fails(&add_tx(&stranger, |_| {}), 0), "a logic the position does not name");
    // The position's own logic, checking another input than the position (the funding input, 2): the position
    // refuses a logic that does not name it.
    assert!(fails(&add_tx(&Shape { named: 2, ..good() }, |_| {}), 0), "a logic that names another input");
    // The logic naming itself.
    assert!(fails(&add_tx(&Shape { named: 1, ..good() }, |_| {}), 1), "a logic that names itself");
}

// ---------------------------------------------------------------- T2: a claim at attested prices

use kpf_protocol::cross::{Cross, XLeg, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::state::{LONG, SHORT};

const RID: Hash = Hash::from_bytes([0x52; 32]);
const M1: [u8; 32] = [0x11; 32];
const M2: [u8; 32] = [0x33; 32];

fn position() -> Cross {
    Cross {
        t: XTerms { term: 8_000, mmr_bps: 500, g_bps: 200, liq_fee_bps: 50, keeper_fee_min: 1_000_000, carrier: 20_000_000, idle_daa: 1_000, grace_daa: 50, late_daa: 100, quiet_daa: 20 },
        legs: [
            XLeg { market: M1, committee: [0x52; 32], scale: 1_000_000, price_max: 100_000_000_000_000, size: 1_000_000_000, dir: LONG, entry: 1_000_000_000 },
            XLeg { market: M2, committee: [0x52; 32], scale: 1_000_000, price_max: 100_000_000_000_000, size: 500_000_000, dir: SHORT, entry: 2_000_000_000 },
        ],
        a_key: xkey(&owner()),
        b_key: xkey(&keypair(33)),
        a_margin: 2_000 * KAS1 as i64,
        b_margin: 3_000 * KAS1 as i64,
        expiry: 14_000,
    }
}

fn push33(h: &[u8; 32]) -> Vec<u8> {
    [vec![0x20], h.to_vec()].concat()
}
fn push9(v: i64) -> Vec<u8> {
    [vec![0x08], sm8(v).to_vec()].concat()
}
fn note_hash(market: &[u8; 32], t0: i64, p: i64) -> [u8; 32] {
    blake2b(&[push33(market), push9(t0), push9(p)].concat())
}
fn pending_hash(side: i64, t0: i64, amount: i64, c: &Cross) -> [u8; 32] {
    blake2b(&[push9(1), push9(side), push9(t0), push9(amount), push9(c.t.late_daa), push33(&blake2b(&c.state_bytes()))].concat())
}

fn close_logic(version: i64) -> SilAbiArtifact {
    compile("probes/T2Close.sil", &[ArtifactValue::Int(version)])
}
fn slot2(l: &SilAbiArtifact, h: [u8; 32]) -> SilAbiArtifact {
    compile("probes/T2Slot.sil", &[ArtifactValue::Bytes(blake2b(&bytecode(l)).to_vec()), ArtifactValue::Bytes(vec![0x99; 32]), ArtifactValue::Bytes(h.to_vec())])
}

struct Close {
    side: i64,
    p: [i64; 2],
    noted: [i64; 2],
    amount: i64,
    signer: Keypair,
    note_id: Hash,
}

/// Inputs: the position (go), the logic (run), a note a leg (go), funding. Outputs: the pending claim (the position
/// with another state's hash), the logic, the two notes as they were, change.
fn close_tx(cl: &Close) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let c = position();
    let l = close_logic(1);
    let t0 = 6_500;
    let pos = slot2(&l, blake2b(&c.state_bytes()));
    let pend = slot2(&l, pending_hash(cl.side, t0, cl.amount, &c));
    let notes = [slot2(&l, note_hash(&M1, t0, cl.noted[0])), slot2(&l, note_hash(&M2, t0, cl.noted[1]))];
    let slot_len = bytecode(&pos).len() as i64;
    let value = c.floor() as u64;
    let who: Vec<u8> = [sm8(0), sm8(2), sm8(3)].concat();
    let go = |k: i64| vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(k))];
    let ins = vec![
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&pos), 6_100, false, Some(XID)), spend: Spend::Signed { art: pos, entry: "go", args: go(0) } },
        In {
            outpoint: outpoint(11),
            utxo: UtxoEntry::new(KAS1, p2sh(&l), 90, false, None),
            spend: Spend::Signed {
                art: l.clone(),
                entry: "run",
                args: vec![
                    Arg::V(ArtifactValue::Bytes(who)),
                    Arg::V(ArtifactValue::Int(slot_len)),
                    Arg::V(ArtifactValue::Bytes(c.state_bytes())),
                    Arg::V(ArtifactValue::Int(cl.side)),
                    Arg::V(ArtifactValue::Int(cl.p[0])),
                    Arg::V(ArtifactValue::Int(cl.p[1])),
                    Arg::V(ArtifactValue::Int(t0)),
                    Arg::Sig(cl.signer),
                ],
            },
        },
        In { outpoint: outpoint(12), utxo: UtxoEntry::new(KAS1, p2sh(&notes[0]), 6_490, false, Some(cl.note_id)), spend: Spend::Signed { art: notes[0].clone(), entry: "go", args: go(1) } },
        In { outpoint: outpoint(13), utxo: UtxoEntry::new(KAS1, p2sh(&notes[1]), 6_490, false, Some(cl.note_id)), spend: Spend::Signed { art: notes[1].clone(), entry: "go", args: go(2) } },
        In { outpoint: outpoint(14), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
    ];
    let outs = vec![
        out(value, p2sh(&pend), bind(0, XID)),
        out(KAS1, p2sh(&l), None),
        out(KAS1, p2sh(&notes[0]), bind(2, cl.note_id)),
        out(KAS1, p2sh(&notes[1]), bind(3, cl.note_id)),
        out(999 * KAS1, p2pk_spk(&keypair(60)), None),
    ];
    build_ext(&ins, outs, 0, &[])
}

#[test]
fn t2_a_close_at_attested_prices_reads_price_notes_and_carries_one_operations_script() {
    let c = position();
    // A closes at (11, 21): paid 2,070 KAS (the model's, as cross_tests has it).
    let p = [1_100_000_000, 2_100_000_000];
    let amount = c.close_pay(SIDE_A, p).unwrap();
    assert_eq!(amount, 2_070 * KAS1 as i64);
    let good = || Close { side: SIDE_A, p, noted: p, amount, signer: owner(), note_id: RID };
    let t = close_tx(&good());
    assert!(all_ok(&t), "{:?}", run(&t.0, &t.1));
    let l = close_logic(1);
    let mut sized = t.0.clone();
    for i in sized.inputs.iter_mut() {
        i.compute_commit = kaspa_consensus_core::mass::ComputeBudget(2_000).into();
    }
    let size = kpf_protocol::tx::masses(&sized, &t.1).size;
    let scripts: Vec<usize> = t.0.inputs.iter().map(|i| i.signature_script.len()).collect();
    println!("T2: the object's script {} bytes, the logic of a close {} bytes; signature scripts (position, logic, note, note, funding) {scripts:?}", bytecode(&slot2(&l, [0; 32])).len(), bytecode(&l).len());
    println!("T2: the whole claim {size} bytes, against 17,133 bytes (KpfCrossC with two epoch tables): {:.1} times smaller", 17_133.0 / size as f64);
    // B's close at the same prices, and the amounts: one sompi more to the closer is another state's hash.
    let b_pay = c.close_pay(SIDE_B, p).unwrap();
    assert!(all_ok(&close_tx(&Close { side: SIDE_B, amount: b_pay, signer: keypair(33), ..good() })), "B's close");
    assert!(fails(&close_tx(&Close { amount: amount + 1, ..good() }), 1), "a sompi more to the closer");
    assert!(fails(&close_tx(&Close { signer: keypair(33), ..good() }), 1), "signed by the other party");
    // The prices are the notes': a claim that names another price than the note holds, a note of another committee.
    assert!(fails(&close_tx(&Close { p: [1_110_000_000, p[1]], amount: c.close_pay(SIDE_A, [1_110_000_000, p[1]]).unwrap(), ..good() }), 1), "another price than the note's");
    assert!(fails(&close_tx(&Close { note_id: Hash::from_bytes([0x53; 32]), ..good() }), 1), "a note under another committee's id");
    // A close below the closer's maintenance, as in the position's own script.
    let low = [1_000_000_000, 2_290_000_000];
    assert!(c.close_pay(SIDE_A, low).is_none());
    assert!(fails(&close_tx(&Close { p: low, noted: low, amount: c.owed_at(SIDE_A, low), ..good() }), 1), "a close below the maintenance");
}

// ---------------------------------------------------------------- T3: a declared claim, settled if nobody disputes

fn moving(c: &Cross, dispute_daa: i64, bond_bps: i64) -> Vec<u8> {
    [push33(&c.a_key), push33(&c.b_key), push9(c.a_margin), push9(c.b_margin), push9(c.expiry), push9(c.t.late_daa), push9(dispute_daa), push9(bond_bps), push9(0), push9(0)].concat()
}
fn fixed_hash(c: &Cross) -> [u8; 32] {
    blake2b(&c.state_bytes()[..312])
}
fn position_hash(c: &Cross, mv: &[u8]) -> [u8; 32] {
    blake2b(&[push33(&fixed_hash(c)), mv.to_vec()].concat())
}
fn claim_hash(position: &[u8; 32], side: i64, t0: i64, a: [i64; 2], amount: i64, bond: i64) -> [u8; 32] {
    blake2b(&[push33(position), push9(side), push9(t0), push9(a[0]), push9(a[1]), push9(amount), push9(bond)].concat())
}
fn claim_logic() -> SilAbiArtifact {
    compile("probes/T3Claim.sil", &[ArtifactValue::Int(1)])
}
fn slot3(l: &SilAbiArtifact, h: [u8; 32]) -> SilAbiArtifact {
    let d = |b: u8| ArtifactValue::Bytes(vec![b; 32]);
    compile("probes/T3Slot.sil", &[ArtifactValue::Bytes(blake2b(&bytecode(l)).to_vec()), d(0x91), d(0x92), d(0x93), ArtifactValue::Bytes(h.to_vec())])
}

const DISPUTE_DAA: i64 = 50;
const BOND_BPS: i64 = 1_000;
const T0: i64 = 6_500;

struct Declared {
    side: i64,
    a: [i64; 2],
    amount: i64,
    signer: Keypair,
}

fn size_of(t: &(kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>)) -> u64 {
    let mut sized = t.0.clone();
    for i in sized.inputs.iter_mut() {
        i.compute_commit = kaspa_consensus_core::mass::ComputeBudget(2_000).into();
    }
    kpf_protocol::tx::masses(&sized, &t.1).size
}

/// The common arguments of both entries after `who` and the age: the object's length, the two parts of the position.
fn shown(c: &Cross, slot_len: i64, age: i64) -> Vec<Arg> {
    vec![
        Arg::V(ArtifactValue::Bytes([sm8(0), sm8(0), sm8(0)].concat())),
        Arg::V(ArtifactValue::Bytes(sm8(age).to_vec())),
        Arg::V(ArtifactValue::Int(slot_len)),
        Arg::V(ArtifactValue::Bytes(fixed_hash(c).to_vec())),
        Arg::V(ArtifactValue::Bytes(moving(c, DISPUTE_DAA, BOND_BPS))),
    ]
}

/// Inputs: the position (go), the logic (declare), funding. Outputs: the claim (the position's value and the bond), the
/// logic, change.
fn declare_tx(d: &Declared, bond_paid: i64) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let c = position();
    let l = claim_logic();
    let position = position_hash(&c, &moving(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let pos = slot3(&l, position);
    let claim = slot3(&l, claim_hash(&position, d.side, T0, d.a, d.amount, bond));
    let slot_len = bytecode(&pos).len() as i64;
    let value = c.floor() as u64;
    let mut args = shown(&c, slot_len, 0);
    args.extend([d.side, T0, d.a[0], d.a[1], d.amount].map(|v| Arg::V(ArtifactValue::Int(v))));
    args.push(Arg::Sig(d.signer));
    let go = vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(0))];
    let ins = vec![
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&pos), 6_100, false, Some(XID)), spend: Spend::Signed { art: pos, entry: "go", args: go } },
        In { outpoint: outpoint(11), utxo: UtxoEntry::new(KAS1, p2sh(&l), 90, false, None), spend: Spend::Signed { art: l.clone(), entry: "declare", args } },
        In { outpoint: outpoint(12), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
    ];
    let outs = vec![out((value as i64 + bond_paid) as u64, p2sh(&claim), bind(0, XID)), out(KAS1, p2sh(&l), None), out(400 * KAS1, p2pk_spk(&keypair(60)), None)];
    build_ext(&ins, outs, 0, &[])
}

/// Inputs: the claim (go, as old as the logic asks), the logic (settle), funding. Outputs: A at the claim's index, the
/// logic, change, B last.
fn settle_tx(d: &Declared, landed: u64, asked: i64, seq: u64, pay_a: i64) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let c = position();
    let l = claim_logic();
    let position = position_hash(&c, &moving(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let claim = slot3(&l, claim_hash(&position, d.side, T0, d.a, d.amount, bond));
    let slot_len = bytecode(&claim).len() as i64;
    let value = c.floor() as u64 + bond as u64;
    let mut args = shown(&c, slot_len, asked);
    args.extend([d.side, T0, d.a[0], d.a[1], d.amount, bond].map(|v| Arg::V(ArtifactValue::Int(v))));
    let go = vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(0))];
    let ins = vec![
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&claim), landed, false, Some(XID)), spend: Spend::Signed { art: claim, entry: "go", args: go } },
        In { outpoint: outpoint(11), utxo: UtxoEntry::new(KAS1, p2sh(&l), 90, false, None), spend: Spend::Signed { art: l.clone(), entry: "settle", args } },
        In { outpoint: outpoint(12), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
    ];
    let outs = vec![
        out(pay_a as u64, kpf_protocol::tx::p2pk_spk(&c.a_key), None),
        out(KAS1, p2sh(&l), None),
        out(999 * KAS1, p2pk_spk(&keypair(60)), None),
        out((value as i64 - pay_a) as u64, kpf_protocol::tx::p2pk_spk(&c.b_key), None),
    ];
    build_ext(&ins, outs, 0, &[seq])
}

#[test]
fn t3_a_declared_claim_is_paid_if_nobody_disputes_it_and_computes_nothing() {
    let c = position();
    let p = [1_100_000_000, 2_100_000_000];
    let amount = c.close_pay(SIDE_A, p).unwrap();
    let bond = c.pot() / 10;
    let good = || Declared { side: SIDE_A, a: p, amount, signer: owner() };
    // The declaration: the owner states what it is owed and posts a tenth of the pot.
    let d = declare_tx(&good(), bond);
    assert!(all_ok(&d), "{:?}", run(&d.0, &d.1));
    assert!(fails(&declare_tx(&good(), bond - 1), 1), "a bond a sompi short");
    assert!(fails(&declare_tx(&Declared { signer: keypair(33), ..good() }, bond), 1), "declared by the other party in the owner's name");
    assert!(fails(&declare_tx(&Declared { amount: c.pot() + 1, ..good() }, bond), 1), "more than the pot declared");
    // The settlement, once the dispute period has passed: the claimer its amount and its bond, the other the rest.
    let s = settle_tx(&good(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond);
    assert!(all_ok(&s), "{:?}", run(&s.0, &s.1));
    assert!(fails(&settle_tx(&good(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond + 1), 1), "a sompi more to the claimer");
    assert!(fails(&settle_tx(&good(), 6_510, DISPUTE_DAA - 1, DISPUTE_DAA as u64, amount + bond), 1), "the logic asks for less than the dispute period");
    assert!(fails(&settle_tx(&good(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64 - 1, amount + bond), 0), "the claim is younger than the logic asks");
    assert!(fails(&settle_tx(&good(), 6_601, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond), 1), "a claim that landed late");
    assert!(fails(&settle_tx(&Declared { amount: amount + KAS1 as i64, ..good() }, 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond), 0) || fails(&settle_tx(&Declared { amount: amount + KAS1 as i64, ..good() }, 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond), 1), "another amount than declared");
    let l = claim_logic();
    println!("T3: the object's script {} bytes, the declare-and-settle logic {} bytes", bytecode(&slot3(&l, [0; 32])).len(), bytecode(&l).len());
    println!("T3: the declaration {} bytes, the settlement {} bytes: together {} against 17,133 + 1,778 (KpfCrossC's claim and finalize)", size_of(&d), size_of(&s), size_of(&d) + size_of(&s));
}

// ---------------------------------------------------------------- T4: the same, the scripts named by a library's id

fn moving4(c: &Cross, dispute_daa: i64, bond_bps: i64) -> Vec<u8> {
    [push33(&c.a_key), push33(&c.b_key), push9(c.a_margin), push9(c.b_margin), push9(c.expiry), push9(c.t.late_daa), push9(dispute_daa), push9(bond_bps), push9(0), push9(0)].concat()
}
fn fixed_hash4(c: &Cross) -> [u8; 32] {
    blake2b(&c.state_bytes()[..312])
}
fn position_hash4(c: &Cross, mv: &[u8]) -> [u8; 32] {
    blake2b(&[push33(&fixed_hash4(c)), mv.to_vec()].concat())
}
fn claim_hash4(position: &[u8; 32], side: i64, t0: i64, a: [i64; 2], amount: i64, bond: i64) -> [u8; 32] {
    blake2b(&[push33(position), push9(side), push9(t0), push9(a[0]), push9(a[1]), push9(amount), push9(bond)].concat())
}
const LIB: Hash = Hash::from_bytes([0x4c; 32]);
fn logic4(entry: &str) -> SilAbiArtifact {
    compile(if entry == "declare" { "probes/T4Declare.sil" } else { "probes/T4Settle.sil" }, &[ArtifactValue::Int(1)])
}
fn slot4(h: [u8; 32]) -> SilAbiArtifact {
    compile("probes/T4Slot.sil", &[ArtifactValue::Bytes(LIB.as_bytes().to_vec()), ArtifactValue::Bytes(h.to_vec())])
}

fn size_of4(t: &(kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>)) -> u64 {
    let mut sized = t.0.clone();
    for i in sized.inputs.iter_mut() {
        i.compute_commit = kaspa_consensus_core::mass::ComputeBudget(2_000).into();
    }
    kpf_protocol::tx::masses(&sized, &t.1).size
}

/// The common arguments of both entries after `who` and the age: the object's length, the two parts of the position.
fn shown4(c: &Cross, slot_len: i64, age: i64) -> Vec<Arg> {
    vec![
        Arg::V(ArtifactValue::Bytes([sm8(0), sm8(0), sm8(0)].concat())),
        Arg::V(ArtifactValue::Bytes(sm8(age).to_vec())),
        Arg::V(ArtifactValue::Int(slot_len)),
        Arg::V(ArtifactValue::Bytes(fixed_hash4(c).to_vec())),
        Arg::V(ArtifactValue::Bytes(moving4(c, DISPUTE_DAA, BOND_BPS))),
    ]
}

/// Inputs: the position (go), the logic (declare), funding. Outputs: the claim (the position's value and the bond), the
/// logic, change.
fn declare_tx4(d: &Declared, bond_paid: i64) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let c = position();
    let l = logic4("declare");
    let position = position_hash4(&c, &moving4(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let pos = slot4(position);
    let claim = slot4(claim_hash4(&position, d.side, T0, d.a, d.amount, bond));
    let slot_len = bytecode(&pos).len() as i64;
    let value = c.floor() as u64;
    let mut args = shown4(&c, slot_len, 0);
    args.extend([d.side, T0, d.a[0], d.a[1], d.amount].map(|v| Arg::V(ArtifactValue::Int(v))));
    args.push(Arg::Sig(d.signer));
    let go = vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(0))];
    let ins = vec![
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&pos), 6_100, false, Some(XID)), spend: Spend::Signed { art: pos, entry: "go", args: go } },
        In { outpoint: outpoint(11), utxo: UtxoEntry::new(KAS1, p2sh(&l), 90, false, Some(LIB)), spend: Spend::Signed { art: l.clone(), entry: "declare", args } },
        In { outpoint: outpoint(12), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
    ];
    let outs = vec![out((value as i64 + bond_paid) as u64, p2sh(&claim), bind(0, XID)), out(KAS1, p2sh(&l), bind(1, LIB)), out(400 * KAS1, p2pk_spk(&keypair(60)), None)];
    build_ext(&ins, outs, 0, &[])
}

/// Inputs: the claim (go, as old as the logic asks), the logic (settle), funding. Outputs: A at the claim's index, the
/// logic, change, B last.
fn settle_tx4(d: &Declared, landed: u64, asked: i64, seq: u64, pay_a: i64) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let c = position();
    let l = logic4("settle");
    let position = position_hash4(&c, &moving4(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let claim = slot4(claim_hash4(&position, d.side, T0, d.a, d.amount, bond));
    let slot_len = bytecode(&claim).len() as i64;
    let value = c.floor() as u64 + bond as u64;
    let mut args = shown4(&c, slot_len, asked);
    args.extend([d.side, T0, d.a[0], d.a[1], d.amount, bond].map(|v| Arg::V(ArtifactValue::Int(v))));
    let go = vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(0))];
    let ins = vec![
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&claim), landed, false, Some(XID)), spend: Spend::Signed { art: claim, entry: "go", args: go } },
        In { outpoint: outpoint(11), utxo: UtxoEntry::new(KAS1, p2sh(&l), 90, false, Some(LIB)), spend: Spend::Signed { art: l.clone(), entry: "settle", args } },
        In { outpoint: outpoint(12), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
    ];
    let outs = vec![
        out(pay_a as u64, kpf_protocol::tx::p2pk_spk(&c.a_key), None),
        out(KAS1, p2sh(&l), bind(1, LIB)),
        out(999 * KAS1, p2pk_spk(&keypair(60)), None),
        out((value as i64 - pay_a) as u64, kpf_protocol::tx::p2pk_spk(&c.b_key), None),
    ];
    build_ext(&ins, outs, 0, &[seq])
}

#[test]
fn t4_one_script_an_operation_under_a_librarys_id() {
    let c = position();
    let p = [1_100_000_000, 2_100_000_000];
    let amount = c.close_pay(SIDE_A, p).unwrap();
    let bond = c.pot() / 10;
    let good = || Declared { side: SIDE_A, a: p, amount, signer: owner() };
    // The declaration: the owner states what it is owed and posts a tenth of the pot.
    let d = declare_tx4(&good(), bond);
    assert!(all_ok(&d), "{:?}", run(&d.0, &d.1));
    assert!(fails(&declare_tx4(&good(), bond - 1), 1), "a bond a sompi short");
    assert!(fails(&declare_tx4(&Declared { signer: keypair(33), ..good() }, bond), 1), "declared by the other party in the owner's name");
    assert!(fails(&declare_tx4(&Declared { amount: c.pot() + 1, ..good() }, bond), 1), "more than the pot declared");
    // The settlement, once the dispute period has passed: the claimer its amount and its bond, the other the rest.
    let s = settle_tx4(&good(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond);
    assert!(all_ok(&s), "{:?}", run(&s.0, &s.1));
    assert!(fails(&settle_tx4(&good(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond + 1), 1), "a sompi more to the claimer");
    assert!(fails(&settle_tx4(&good(), 6_510, DISPUTE_DAA - 1, DISPUTE_DAA as u64, amount + bond), 1), "the logic asks for less than the dispute period");
    assert!(fails(&settle_tx4(&good(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64 - 1, amount + bond), 0), "the claim is younger than the logic asks");
    assert!(fails(&settle_tx4(&good(), 6_601, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond), 1), "a claim that landed late");
    assert!(fails(&settle_tx4(&Declared { amount: amount + KAS1 as i64, ..good() }, 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond), 0) || fails(&settle_tx4(&Declared { amount: amount + KAS1 as i64, ..good() }, 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + bond), 1), "another amount than declared");
    println!("T4: the object's script {} bytes, the declaration's script {} bytes, the settlement's {} bytes", bytecode(&slot4([0; 32])).len(), bytecode(&logic4("declare")).len(), bytecode(&logic4("settle")).len());
    println!("T4: the declaration {} bytes, the settlement {} bytes (each with a funding input and a change output, about 180 bytes of it)", size_of4(&d), size_of4(&s));
}

// ---------------------------------------------------------------- T5: a dispute, its proof, a dispute nobody answers

fn lib_logic(file: &str) -> SilAbiArtifact {
    compile(file, &[ArtifactValue::Int(1)])
}
fn disputed_hash(c0: &[u8; 32], dkey: &[u8; 32]) -> [u8; 32] {
    blake2b(&[push33(c0), push33(dkey)].concat())
}
fn disputer() -> Keypair {
    keypair(40)
}

/// The arguments every script of the claim's life takes after the object's length: the two parts of the position
/// (the fixed part whole for the proof, its hash otherwise) and what was declared.
fn claim_args(c: &Cross, slot_len: i64, age: i64, who: [i64; 3], d: &Declared, whole: bool) -> Vec<Arg> {
    let bond = c.pot() / 10;
    let mut a = vec![
        Arg::V(ArtifactValue::Bytes(who.iter().flat_map(|w| sm8(*w)).collect())),
        Arg::V(ArtifactValue::Bytes(sm8(age).to_vec())),
        Arg::V(ArtifactValue::Int(slot_len)),
        Arg::V(ArtifactValue::Bytes(if whole { c.state_bytes()[..312].to_vec() } else { fixed_hash4(c).to_vec() })),
        Arg::V(ArtifactValue::Bytes(moving4(c, DISPUTE_DAA, BOND_BPS))),
    ];
    a.extend([d.side, T0, d.a[0], d.a[1], d.amount, bond].map(|v| Arg::V(ArtifactValue::Int(v))));
    a.push(Arg::V(ArtifactValue::Bytes(xkey(&disputer()).to_vec())));
    a
}

type Tx = (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>);

fn lib_in(l: &SilAbiArtifact, entry: &'static str, args: Vec<Arg>) -> In {
    In { outpoint: outpoint(11), utxo: UtxoEntry::new(KAS1, p2sh(l), 90, false, Some(LIB)), spend: Spend::Signed { art: l.clone(), entry, args } }
}
fn fund_in() -> In {
    In { outpoint: outpoint(14), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) }
}
fn go_in(art: SilAbiArtifact, value: u64, daa: u64, id: Hash, n: u8, k: i64) -> In {
    In { outpoint: outpoint(n), utxo: UtxoEntry::new(value, p2sh(&art), daa, false, Some(id)), spend: Spend::Signed { art, entry: "go", args: vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(k))] } }
}

/// The dispute of the claim `d` (landed at `landed`): the claim written again with the disputer's key and its bond.
fn dispute_tx(d: &Declared, landed: u64, bond_paid: i64) -> Tx {
    let c = position();
    let l = lib_logic("probes/T5Dispute.sil");
    let position = position_hash4(&c, &moving4(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let c0 = claim_hash4(&position, d.side, T0, d.a, d.amount, bond);
    let (claim, disputed) = (slot4(c0), slot4(disputed_hash(&c0, &xkey(&disputer()))));
    let slot_len = bytecode(&claim).len() as i64;
    let value = c.floor() as u64 + bond as u64;
    let ins = vec![go_in(claim, value, landed, XID, 10, 0), lib_in(&l, "dispute", claim_args(&c, slot_len, 0, [0, 0, 0], d, false)), fund_in()];
    let outs = vec![out((value as i64 + bond_paid) as u64, p2sh(&disputed), bind(0, XID)), out(KAS1, p2sh(&l), bind(1, LIB)), out(400 * KAS1, p2pk_spk(&keypair(60)), None)];
    build_ext(&ins, outs, 0, &[])
}

/// The end of a dispute nobody answered: the position restored, half of the claimer's bond to the other party, the
/// rest and its own bond to the disputer (the last output).
fn lapse_tx(d: &Declared, asked: i64, seq: u64, to_disputer: i64, to_other: i64) -> Tx {
    let c = position();
    let l = lib_logic("probes/T5Dispute.sil");
    let position = position_hash4(&c, &moving4(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let c0 = claim_hash4(&position, d.side, T0, d.a, d.amount, bond);
    let disputed = slot4(disputed_hash(&c0, &xkey(&disputer())));
    let slot_len = bytecode(&disputed).len() as i64;
    let value = c.floor() as u64 + 2 * bond as u64;
    let ins = vec![go_in(disputed, value, 6_520, XID, 10, 0), lib_in(&l, "lapse", claim_args(&c, slot_len, asked, [0, 0, 0], d, false)), fund_in()];
    let outs = vec![
        out(c.floor() as u64, p2sh(&slot4(position)), bind(0, XID)),
        out(KAS1, p2sh(&l), bind(1, LIB)),
        out(999 * KAS1, p2pk_spk(&keypair(60)), None),
        out(to_other as u64, kpf_protocol::tx::p2pk_spk(&c.b_key), None),
        out(to_disputer as u64, kpf_protocol::tx::p2pk_spk(&xkey(&disputer())), None),
    ];
    build_ext(&ins, outs, 0, &[seq])
}

/// The proof: the claim, the logic, a note a leg, funding; the claimer's amount and both bonds to A at the claim's
/// index, the rest to B last.
fn prove_tx(d: &Declared, noted: [i64; 2], pay_a: i64) -> Tx {
    let c = position();
    let l = lib_logic("probes/T5Prove.sil");
    let position = position_hash4(&c, &moving4(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let c0 = claim_hash4(&position, d.side, T0, d.a, d.amount, bond);
    let disputed = slot4(disputed_hash(&c0, &xkey(&disputer())));
    let notes = [slot4(note_hash(&M1, T0, noted[0])), slot4(note_hash(&M2, T0, noted[1]))];
    let slot_len = bytecode(&disputed).len() as i64;
    let value = c.floor() as u64 + 2 * bond as u64;
    let ins = vec![
        go_in(disputed, value, 6_520, XID, 10, 0),
        lib_in(&l, "prove", claim_args(&c, slot_len, 0, [0, 2, 3], d, true)),
        go_in(notes[0].clone(), KAS1, 6_490, RID, 12, 1),
        go_in(notes[1].clone(), KAS1, 6_490, RID, 13, 2),
        fund_in(),
    ];
    let outs = vec![
        out(pay_a as u64, kpf_protocol::tx::p2pk_spk(&c.a_key), None),
        out(KAS1, p2sh(&l), bind(1, LIB)),
        out(KAS1, p2sh(&notes[0]), bind(2, RID)),
        out(KAS1, p2sh(&notes[1]), bind(3, RID)),
        out(999 * KAS1, p2pk_spk(&keypair(60)), None),
        out((value as i64 - pay_a) as u64, kpf_protocol::tx::p2pk_spk(&c.b_key), None),
    ];
    build_ext(&ins, outs, 0, &[])
}

#[test]
fn t5_a_true_claim_takes_the_disputers_bond_and_a_false_one_loses_its_own() {
    let c = position();
    let p = [1_100_000_000, 2_100_000_000];
    let amount = c.close_pay(SIDE_A, p).unwrap();
    let bond = c.pot() / 10;
    let truth = || Declared { side: SIDE_A, a: p, amount, signer: owner() };
    // A true claim is disputed: the disputer posts a bond as large as the claimer's.
    let dis = dispute_tx(&truth(), 6_510, bond);
    assert!(all_ok(&dis), "{:?}", run(&dis.0, &dis.1));
    assert!(fails(&dispute_tx(&truth(), 6_510, bond - 1), 1), "a dispute a sompi short of its bond");
    assert!(fails(&dispute_tx(&truth(), 6_601, bond), 1), "a dispute of a claim that landed late");
    // The claimer proves it with the notes of the time: paid its amount and both bonds; the disputer has lost its own.
    let pr = prove_tx(&truth(), p, amount + 2 * bond);
    assert!(all_ok(&pr), "{:?}", run(&pr.0, &pr.1));
    assert!(fails(&prove_tx(&truth(), p, amount + 2 * bond + 1), 1), "a sompi more to the claimer");
    assert!(fails(&prove_tx(&truth(), [1_110_000_000, p[1]], amount + 2 * bond), 1), "notes of another price than declared");
    // A false claim: 100 KAS more than the rule gives, at the same prices. Nobody proves it, whatever notes are shown.
    let lie = || Declared { amount: amount + 100 * KAS1 as i64, ..truth() };
    assert!(all_ok(&dispute_tx(&lie(), 6_510, bond)), "a false claim is disputed as a true one is");
    assert!(fails(&prove_tx(&lie(), p, amount + 100 * KAS1 as i64 + 2 * bond), 1), "a false amount is not the rule's");
    // After the proof dispute period the position is restored; the claimer's bond is the disputer's and the other party's, by
    // halves, so that a claimer that disputes itself to hold the position pays the party it held.
    let la = lapse_tx(&lie(), DISPUTE_DAA, DISPUTE_DAA as u64, 2 * bond - bond / 2, bond / 2);
    assert!(all_ok(&la), "{:?}", run(&la.0, &la.1));
    assert!(fails(&lapse_tx(&lie(), DISPUTE_DAA, DISPUTE_DAA as u64 - 1, 2 * bond - bond / 2, bond / 2), 0), "before the proof dispute period has passed");
    assert!(fails(&lapse_tx(&lie(), DISPUTE_DAA - 1, DISPUTE_DAA as u64, 2 * bond - bond / 2, bond / 2), 1), "the logic asks for less than the dispute period");
    assert!(fails(&lapse_tx(&lie(), DISPUTE_DAA, DISPUTE_DAA as u64, 2 * bond - bond / 2 - 1, bond / 2), 1), "the disputer a sompi short");
    assert!(fails(&lapse_tx(&lie(), DISPUTE_DAA, DISPUTE_DAA as u64, 2 * bond - bond / 2, bond / 2 - 1), 1), "the other party a sompi short");
    assert!(fails(&lapse_tx(&lie(), DISPUTE_DAA, DISPUTE_DAA as u64, 2 * bond, 0), 1), "both bonds to the disputer");
    // The same end for a true claim nobody proved in time: a claimer answers a dispute or loses its bond.
    assert!(all_ok(&lapse_tx(&truth(), DISPUTE_DAA, DISPUTE_DAA as u64, 2 * bond - bond / 2, bond / 2)));
    // And what the form rests on: a false claim nobody disputes within the dispute period is paid as declared.
    let s = settle_tx4(&lie(), 6_510, DISPUTE_DAA, DISPUTE_DAA as u64, amount + 100 * KAS1 as i64 + bond);
    assert!(all_ok(&s), "an undisputed claim is paid, true or not: the dispute period is for somebody to look");
    println!("T5: the dispute's and the lapse's script {} bytes, the proof's {} bytes", bytecode(&lib_logic("probes/T5Dispute.sil")).len(), bytecode(&lib_logic("probes/T5Prove.sil")).len());
    println!("T5: the dispute {} bytes, the lapse {} bytes, the proof {} bytes", size_of4(&dis), size_of4(&la), size_of4(&pr));
}

// ---------------------------------------------------------------- a script positions for every place it names

/// A declaration whose script names the inputs `who`; with `extra`, a second object of the library (another position,
/// 500 KAS, under an id of its own) is spent beside it, naming itself at place 1, and its value goes to the change.
fn declare_naming(who: [i64; 3], extra: bool) -> Tx {
    let c = position();
    let l = logic4("declare");
    let position = position_hash4(&c, &moving4(&c, DISPUTE_DAA, BOND_BPS));
    let bond = c.pot() / 10;
    let p = [1_100_000_000, 2_100_000_000];
    let amount = c.close_pay(SIDE_A, p).unwrap();
    let pos = slot4(position);
    let claim = slot4(claim_hash4(&position, SIDE_A, T0, p, amount, bond));
    let slot_len = bytecode(&pos).len() as i64;
    let value = c.floor() as u64;
    let mut args = shown4(&c, slot_len, 0);
    args[0] = Arg::V(ArtifactValue::Bytes(who.iter().flat_map(|w| sm8(*w)).collect()));
    args.extend([SIDE_A, T0, p[0], p[1], amount].map(|v| Arg::V(ArtifactValue::Int(v))));
    args.push(Arg::Sig(owner()));
    let mut ins = vec![go_in(pos, value, 6_100, XID, 10, 0), lib_in(&l, "declare", args), fund_in()];
    if extra {
        ins.push(go_in(slot4([0x77; 32]), 500 * KAS1, 6_100, Hash::from_bytes([0x59; 32]), 15, 1));
    }
    let outs = vec![out(value + bond as u64, p2sh(&claim), bind(0, XID)), out(KAS1, p2sh(&l), bind(1, LIB)), out(900 * KAS1, p2pk_spk(&keypair(60)), None)];
    build_ext(&ins, outs, 0, &[])
}

#[test]
fn t6_an_object_named_at_a_place_the_script_does_not_read() {
    assert!(all_ok(&declare_naming([0, 0, 0], false)), "the plain declaration");
    // The second position is refused by the declaration's script, which reads one place and has the others name it.
    assert!(fails(&declare_naming([0, 3, 3], true), 1), "a position named at a place the declaration does not read");
    assert!(fails(&declare_naming([0, 3, 0], true), 1));
    assert!(fails(&declare_naming([0, 0, 3], false), 1), "a place that names an input the script does not read");
    // And an object the script does not name at all refuses for itself.
    assert!(fails(&declare_naming([0, 0, 0], true), 3), "a position beside a script that does not name it");
}

// ---------------------------------------------------------------- the family's own scripts (contracts/*.sil)

fn thin(name: &str) -> SilAbiArtifact {
    compile(&format!("{name}.sil"), &[ArtifactValue::Int(1)])
}
fn tslot(h: [u8; 32]) -> SilAbiArtifact {
    compile("KpfSlot.sil", &[ArtifactValue::Bytes(LIB.as_bytes().to_vec()), ArtifactValue::Bytes(h.to_vec())])
}
fn hash_of(c: &Cross) -> [u8; 32] {
    position_hash4(c, &moving4(c, DISPUTE_DAA, BOND_BPS))
}
fn ints(v: &[i64]) -> Vec<Arg> {
    v.iter().map(|x| Arg::V(ArtifactValue::Int(*x))).collect()
}

/// Margin added to `side`: `delta` stated, `paid` put in, signed by `signer`.
fn tadd_tx(side: i64, delta: i64, paid: i64, signer: Keypair) -> Tx {
    let c = position();
    let l = thin("KpfTAdd");
    let mut after = c.clone();
    if side == SIDE_A {
        after.a_margin += delta;
    } else {
        after.b_margin += delta;
    }
    let pos = tslot(hash_of(&c));
    let slot_len = bytecode(&pos).len() as i64;
    let value = c.floor() as u64;
    let mut args = shown4(&c, slot_len, 0);
    args.extend(ints(&[side, delta]));
    args.push(Arg::Sig(signer));
    let ins = vec![go_in(pos, value, 6_100, XID, 10, 0), lib_in(&l, "add", args), fund_in()];
    let outs = vec![out((value as i64 + paid) as u64, p2sh(&tslot(hash_of(&after))), bind(0, XID)), out(KAS1 + kpf_protocol::thin::ORACLE_FEE as u64, p2sh(&l), bind(1, LIB)), out(400 * KAS1, p2pk_spk(&keypair(60)), None)];
    build_ext(&ins, outs, 0, &[])
}

/// The two parties end the position as they agreed: everything to one address, no output of the position's id.
fn tboth_tx(sa: Keypair, sb: Keypair) -> Tx {
    let c = position();
    let l = thin("KpfTBoth");
    let pos = tslot(hash_of(&c));
    let slot_len = bytecode(&pos).len() as i64;
    let mut args = shown4(&c, slot_len, 0);
    args.push(Arg::Sig(sa));
    args.push(Arg::Sig(sb));
    let ins = vec![go_in(pos, c.floor() as u64, 6_100, XID, 10, 0), lib_in(&l, "both", args), fund_in()];
    let outs = vec![out(c.floor() as u64, p2pk_spk(&keypair(61)), None), out(KAS1 + kpf_protocol::thin::ORACLE_FEE as u64, p2sh(&l), bind(1, LIB)), out(999 * KAS1, p2pk_spk(&keypair(60)), None)];
    build_ext(&ins, outs, 0, &[])
}

/// A claim that landed at `landed` is ended as late by the second executor: the position restored, `to_other` to the
/// party the claim was made against (the output before the last), `to_ender` to who ended it (the last).
fn tlate_tx(landed: u64, to_ender: i64, to_other: i64) -> Tx {
    let c = position();
    let l = thin("KpfTLate");
    let p = [1_100_000_000, 2_100_000_000];
    let amount = c.close_pay(SIDE_A, p).unwrap();
    let bond = c.pot() / 10;
    let claim = tslot(claim_hash4(&hash_of(&c), SIDE_A, T0, p, amount, bond));
    let slot_len = bytecode(&claim).len() as i64;
    let value = c.floor() as u64 + bond as u64;
    let mut args = shown4(&c, slot_len, 0);
    args.extend(ints(&[SIDE_A, T0, p[0], p[1], amount, bond]));
    args.push(Arg::V(ArtifactValue::Bytes(xkey(&disputer()).to_vec())));
    let ins = vec![go_in(claim, value, landed, XID, 10, 0), lib_in(&l, "late", args), fund_in()];
    let outs = vec![
        out(c.floor() as u64, p2sh(&tslot(hash_of(&c))), bind(0, XID)),
        out(KAS1 + kpf_protocol::thin::ORACLE_FEE as u64, p2sh(&l), bind(1, LIB)),
        out(999 * KAS1, p2pk_spk(&keypair(60)), None),
        out(to_other as u64, kpf_protocol::tx::p2pk_spk(&c.b_key), None),
        out(to_ender as u64, kpf_protocol::tx::p2pk_spk(&xkey(&disputer())), None),
    ];
    build_ext(&ins, outs, 0, &[])
}

#[test]
fn the_familys_scripts_are_the_probes_and_three_operations_more() {
    // The promoted scripts were the probes' bytes until the oracle fees: each now asks its copy to go on with ORACLE_FEE more
    // and has the entry that lets the copy be swept (`release`), so each is the longer, and what the probes' tests
    // showed of the rules is shown of the family's scripts by the builders' tests. The object is the probe's still.
    for (probe, own) in [("T4Declare", "KpfTClose"), ("T4Settle", "KpfTSettle"), ("T5Dispute", "KpfTDispute")] {
        assert!(bytecode(&lib_logic(&format!("probes/{probe}.sil"))).len() < bytecode(&thin(own)).len(), "{own}");
    }
    assert_eq!(bytecode(&slot4([7; 32])), bytecode(&tslot([7; 32])));
    let kas = KAS1 as i64;
    // Margin: the owner of a side adds to it.
    let add = tadd_tx(SIDE_A, 100 * kas, 100 * kas, owner());
    assert!(all_ok(&add), "{:?}", run(&add.0, &add.1));
    assert!(all_ok(&tadd_tx(SIDE_B, 100 * kas, 100 * kas, keypair(33))), "the other side adds to its own");
    assert!(fails(&tadd_tx(SIDE_A, 100 * kas, 100 * kas - 1, owner()), 1), "a sompi less paid in than stated");
    assert!(fails(&tadd_tx(SIDE_A, 100 * kas, 100 * kas, keypair(33)), 1), "added in the owner's name by the other party");
    assert!(fails(&tadd_tx(SIDE_B, 100 * kas, 100 * kas, owner()), 1), "added to the other side");
    assert!(fails(&tadd_tx(SIDE_A, 0, 0, owner()), 1), "nothing added");
    assert!(fails(&tadd_tx(3, 100 * kas, 100 * kas, owner()), 1), "a side that is neither");
    // Both: the two signatures and nothing else.
    let both = tboth_tx(owner(), keypair(33));
    assert!(all_ok(&both), "{:?}", run(&both.0, &both.1));
    assert!(fails(&tboth_tx(owner(), owner()), 1), "one party twice");
    assert!(fails(&tboth_tx(keypair(33), owner()), 1), "the signatures in each other's place");
    // Late: a claim that landed after lateDaa, or before its own time, is ended by anyone; its bond is shared by who
    // ended it and the party it was made against (review T1b, F3), and the claimer has none of it back.
    let bond = position().pot() / 10;
    let (e, o) = (bond - bond / 2, bond / 2);
    let late = tlate_tx(6_601, e, o);
    assert!(all_ok(&late), "{:?}", run(&late.0, &late.1));
    assert!(all_ok(&tlate_tx(6_499, e, o)), "a claim that landed before its own time is ended the same way");
    assert!(fails(&tlate_tx(6_600, e, o), 1), "a claim that landed in time is not late");
    assert!(fails(&tlate_tx(6_500, e, o), 1), "nor is one that landed at its time");
    assert!(fails(&tlate_tx(6_601, e - 1, o), 1), "a sompi less to who ended it");
    assert!(fails(&tlate_tx(6_601, e, o - 1), 1), "a sompi less to the other party");
    println!("thin: scripts add {} both {} late {} bytes", bytecode(&thin("KpfTAdd")).len(), bytecode(&thin("KpfTBoth")).len(), bytecode(&thin("KpfTLate")).len());
    println!("thin: transactions add {} both {} late {} bytes", size_of4(&add), size_of4(&both), size_of4(&late));
}

// ---------------------------------------------------------------- the model (kpf_protocol::thin) against the scripts

#[test]
fn the_models_hashes_and_scripts_are_the_ones_the_engine_tests_use() {
    use kpf_protocol::thin::{Position, TObj};
    use kpf_protocol::thin_artifacts as pinned;
    let c = position();
    let a = Position { x: c, dispute_daa: DISPUTE_DAA, bond_bps: BOND_BPS, a_min_counter: 0, b_min_counter: 0 };
    a.check().unwrap();
    // The two parts and the three hashes, against the ones written out by hand above.
    // (The moving part ends with what each party asks of the other's margin: nothing, in these positions.)
    assert_eq!(a.moving(), moving4(&c, DISPUTE_DAA, BOND_BPS));
    assert_eq!(a.fixed_hash(), fixed_hash4(&c));
    assert_eq!(a.hash(), hash_of(&c));
    let p = [1_100_000_000, 2_100_000_000];
    let t = a.true_claim(SIDE_A, T0, p).unwrap();
    assert_eq!((t.amount, t.bond), (c.close_pay(SIDE_A, p).unwrap(), c.pot() / 10));
    assert_eq!(t.hash(), claim_hash4(&a.hash(), SIDE_A, T0, p, t.amount, t.bond));
    let d = t.disputed(xkey(&disputer()));
    assert_eq!(d.hash(), disputed_hash(&t.hash(), &xkey(&disputer())));
    // The pinned scripts are the compiled ones: the object with its two values, and the seven of a library.
    let lib: [u8; 32] = LIB.as_bytes();
    for o in [TObj::Position(a), TObj::Claim(t), TObj::Disputed(d)] {
        assert_eq!(o.script(&lib), bytecode(&tslot(o.hash())), "{}", o.kind());
    }
    // (The proofs are compiled with the epoch table's template: `kpf-artifacts check` holds them to their sources.)
    // (So is the merge, compiled with what its sender keeps.)
    // (So are the note's writer and the citation, compiled with the note's script, and what reads a citation, with
    // the citation's address.)
    for n in pinned::SCRIPTS.iter().filter(|n| !n.ends_with("Prove") && !n.ends_with("Refute") && !["KpfTMerge", "KpfTNovate", "KpfTJoin", "KpfTFold", "KpfTLeave", "KpfTPay", "KpfTNote", "KpfTCite", "KpfTTake", "KpfTShardPrice", "KpfTStepPrice"].contains(*n)) {
        assert_eq!(pinned::kit().script(n).unwrap(), &bytecode(&thin(n))[..], "{n}");
    }
    // What the model says a claim pays is what the scripts accepted above (the sums of T4 and T5).
    let v = c.floor() + t.bond;
    assert_eq!(t.settle_pay(v).0, t.amount + t.bond);
    assert_eq!(d.prove_pay(v + t.bond).0, t.amount + 2 * t.bond - (t.bond - t.bond / 2));
    assert_eq!(d.lapse_pay(v + t.bond), Some((c.floor(), t.bond, t.bond)));
}
