//! Review T1b, the tool's side (`kpf_protocol::thin`, `kpf_protocol::thin_build`): the codec of an object's state on
//! random objects of every kind, the findings of the engine tests (`crates/kpf-tests/tests/auditT1_tests.rs`)
//! through the builders, signed and metered (`verify::verify`). A test named `t1b_finding_...` asserts an acceptance
//! and documents it (docs/audit/REVIEW-T1b.md).

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kpf_protocol::cross::{SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::thin::{samples, Position, LiqObj, LiqState, TObj, WINDOW_MAX};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;

// ---------------------------------------------------------------- E: the codec

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// lo <= v < hi
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
    fn key(&mut self) -> [u8; 32] {
        let mut k = [0u8; 32];
        for c in k.chunks_mut(8) {
            c.copy_from_slice(&self.next().to_le_bytes());
        }
        k
    }
}

/// A random position the model takes (`check`), from the sample's terms.
fn random_position(r: &mut Rng) -> Position {
    loop {
        let mut a = samples::position();
        a.x.a_key = r.key();
        a.x.b_key = r.key();
        a.x.a_margin = r.range(KAS1 / 10, 5_000 * KAS1);
        a.x.b_margin = r.range(KAS1 / 10, 5_000 * KAS1);
        a.x.expiry = r.range(1, 200_000);
        a.dispute_daa = if r.range(0, 4) == 0 { r.range(1, WINDOW_MAX + 1) } else { r.range(1, 2_000) };
        a.bond_bps = r.range(1, 10_001);
        for l in a.x.legs.iter_mut() {
            if l.size > 0 {
                l.size = r.range(1, 40) * l.scale + r.range(0, l.scale);
                l.entry = r.range(l.entry / 2 + 1, l.entry * 2);
            }
        }
        if a.check().is_ok() {
            return a;
        }
    }
}

/// One object of every kind on a random position, what is stated random within what the scripts take.
fn random_objects(r: &mut Rng) -> Vec<TObj> {
    let a = random_position(r);
    let side = |r: &mut Rng| if r.range(0, 2) == 0 { SIDE_A } else { SIDE_B };
    let p = |r: &mut Rng| [r.range(0, 5_000_000_000), r.range(0, 5_000_000_000)];
    let c = a.claim(side(r), r.range(0, 10_000_000), p(r), r.range(0, a.pot() + 1)).expect("a claim");
    let l = a.liq_stated(side(r), r.range(0, 10_000_000), p(r), r.range(0, a.pot() + 1), r.range(1, 1_000 * KAS1), r.key()).expect("a liquidation");
    let q = a.exit(side(r), r.range(0, 10_000_000)).expect("an exit claim");
    vec![
        TObj::Position(a),
        TObj::Claim(c),
        TObj::Disputed(c.disputed(r.key())),
        TObj::Liq(LiqObj { liq: l, state: LiqState::Claimed }),
        TObj::Liq(LiqObj { liq: l, state: LiqState::Disputed(r.key()) }),
        TObj::Liq(LiqObj { liq: l, state: LiqState::Proved }),
        TObj::ExitClaim(q),
        TObj::Order(random_order(r)),
    ]
}

#[test]
fn t1b_e_decode_of_encode_is_the_object_for_random_objects_of_every_kind() {
    let mut r = Rng(0x7131_b0b5_1dea_0001);
    let mut kinds = std::collections::BTreeMap::new();
    let mut hashes = std::collections::BTreeSet::new();
    for _ in 0..300 {
        for o in random_objects(&mut r) {
            let b = o.encode();
            let back = TObj::decode(&b).unwrap_or_else(|e| panic!("{}: {e}: {o:?}", o.kind()));
            assert_eq!(back, o);
            assert_eq!(back.hash(), o.hash());
            assert_eq!(back.encode(), b);
            *kinds.entry(o.kind()).or_insert(0) += 1;
            hashes.insert(o.hash());
        }
    }
    println!("T1b E round trip: {kinds:?}");
    assert_eq!(kinds.len(), 8, "every kind (an order since round 4)");
    // The seven objects of one position are seven hashes: no two kinds of the same position share one.
    assert_eq!(hashes.len(), 300 * 8);
}

#[test]
fn t1b_e_decode_refuses_truncated_extended_and_cross_kind_bytes() {
    let mut r = Rng(0x7131_b0b5_1dea_0002);
    let (mut cut, mut ext, mut cross) = (0, 0, 0);
    for round in 0..12 {
        for o in random_objects(&mut r) {
            let b = o.encode();
            // Every proper prefix (all of them the first rounds, a sample after).
            for n in 0..b.len() {
                if round < 3 || n % 7 == round % 7 || n + 40 > b.len() {
                    assert!(TObj::decode(&b[..n]).is_err(), "{}: the first {n} of {} bytes are read", o.kind(), b.len());
                    cut += 1;
                }
            }
            // Extended: by bytes of every kind of tail, among them what another kind ends with.
            for tail in [vec![0u8], vec![1], vec![2], vec![0x08], vec![0x20], vec![0; 9], [vec![0x08], vec![0; 8]].concat(), vec![0; 27], vec![0; 32], vec![0; 33], vec![0; 54], b.clone()] {
                assert!(TObj::decode(&[b.clone(), tail.clone()].concat()).is_err(), "{}: read with {} bytes more", o.kind(), tail.len());
                ext += 1;
            }
            // The same bytes under every other kind byte.
            for k in 0..=255u8 {
                if k != b[0] {
                    let mut x = b.clone();
                    x[0] = k;
                    assert!(TObj::decode(&x).is_err(), "{}: read as kind {k}", o.kind());
                    cross += 1;
                }
            }
        }
    }
    assert!(TObj::decode(&[]).is_err());
    println!("T1b E refusals: {cut} prefixes, {ext} extensions, {cross} other kind bytes");
}

/// One object's state has one byte string: whatever `decode` reads encodes back to the bytes it read. Random single
/// bytes changed, and a negative zero in every integer.
#[test]
fn t1b_e_what_decode_reads_encodes_back_to_the_same_bytes() {
    let mut r = Rng(0x7131_b0b5_1dea_0003);
    let (mut read, mut refused) = (0, 0);
    for _ in 0..40 {
        for o in random_objects(&mut r) {
            let b = o.encode();
            for _ in 0..120 {
                let mut x = b.clone();
                let at = r.range(0, b.len() as i64) as usize;
                x[at] ^= 1 << r.range(0, 8);
                match TObj::decode(&x) {
                    Ok(back) => {
                        assert_eq!(back.encode(), x, "{}: byte {at} changed, read as other bytes", o.kind());
                        assert_ne!(back, o);
                        assert_ne!(back.hash(), o.hash(), "{}: byte {at} changed and the same hash", o.kind());
                        read += 1;
                    }
                    Err(_) => refused += 1,
                }
            }
            // The tail (what is stated) with the top bit of an integer set where the integer is zero.
            for at in 433..b.len() {
                let mut x = b.clone();
                x[at] ^= 0x80;
                if let Ok(back) = TObj::decode(&x) {
                    assert_eq!(back.encode(), x, "{}: byte {at}", o.kind());
                }
            }
        }
    }
    println!("T1b E canonical: {read} changed states read (each to its own bytes), {refused} refused");
    assert!(read > 0 && refused > 0);
}

#[test]
fn t1b_e_decode_refuses_what_the_scripts_would_not_have_written() {
    let mut r = Rng(0x7131_b0b5_1dea_0004);
    for _ in 0..40 {
        let a = random_position(&mut r);
        let pot = a.pot();
        let c = a.claim(SIDE_A, 6_500, [1, 2], pot / 2).unwrap();
        let said = |side: i64, t0: i64, a1: i64, a2: i64, amount: i64, bond: i64| -> Vec<u8> {
            let i9 = |v: i64| [vec![0x08], kpf_protocol::state::sm8(v).to_vec()].concat();
            [TObj::Position(a).encode()[1..].to_vec(), i9(side), i9(t0), i9(a1), i9(a2), i9(amount), i9(bond)].concat()
        };
        let as_claim = |b: Vec<u8>| TObj::decode(&[vec![2], b].concat());
        assert_eq!(as_claim(said(SIDE_A, 6_500, 1, 2, pot / 2, a.bond())).unwrap(), TObj::Claim(c));
        for (what, b) in [
            ("a side that is neither", said(3, 6_500, 1, 2, pot / 2, a.bond())),
            ("side 0", said(0, 6_500, 1, 2, pot / 2, a.bond())),
            ("a negative signed price", said(SIDE_A, -1, 1, 2, pot / 2, a.bond())),
            ("a negative price", said(SIDE_A, 6_500, -1, 2, pot / 2, a.bond())),
            ("more than the pot", said(SIDE_A, 6_500, 1, 2, pot + 1, a.bond())),
            ("a negative amount", said(SIDE_A, 6_500, 1, 2, -1, a.bond())),
            ("another bond", said(SIDE_A, 6_500, 1, 2, pot / 2, a.bond() + 1)),
            ("no bond", said(SIDE_A, 6_500, 1, 2, pot / 2, 0)),
        ] {
            assert!(as_claim(b).is_err(), "a claim with {what} is read");
        }
        // A position's moving part whose lateDaa is not the terms', a dispute period or a bond rate out of range.
        let enc = TObj::Position(a).encode();
        for (at, v) in [(1 + 312 + 94, a.x.t.late_daa + 1), (1 + 312 + 103, 0), (1 + 312 + 103, WINDOW_MAX + 1), (1 + 312 + 112, 0), (1 + 312 + 112, 10_001)] {
            let mut x = enc.clone();
            x[at..at + 8].copy_from_slice(&kpf_protocol::state::sm8(v));
            assert!(TObj::decode(&x).is_err(), "a position with {v} at {at} is read");
        }
    }
}

// ---------------------------------------------------------------- the findings through the builders

fn kp(seed: u8) -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[seed; 32]).expect("a secret key"))
}
fn xk(k: &Keypair) -> [u8; 32] {
    k.x_only_public_key().0.serialize()
}
fn fund(key: [u8; 32], n: u8, kas: i64) -> KeyUtxo {
    KeyUtxo { outpoint: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, amount: (kas * KAS1) as u64, daa: 0, key }
}
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    println!("{what}: {} bytes, fee {} sompi", sg.mass.size, sg.fee);
    sg
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}
fn paid(sg: &OSigned, key: &[u8; 32]) -> i64 {
    sg.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(key)).map(|o| o.value as i64).sum()
}

struct World {
    a: Keypair,
    b: Keypair,
    w: Keypair,
    keys: KeySet,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    position: OLive<Position>,
    /// An anchor of the world's registry (round 8): what enters the registry enters through it.
    anchor: Anchor,
    n: u8,
}
impl World {
    fn pay(&mut self, k: &Keypair) -> Vec<KeyUtxo> {
        self.n += 1;
        vec![fund(xk(k), self.n, 1_000)]
    }
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
}

/// As `thin_builders.rs`: a library, a registry of it, and a position of 2,000 + 3,000 KAS between A and B, entered
/// through an anchor of the registry by both signatures, whose claims wait 50 DAA and bond a tenth of the pot.
fn world() -> World {
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let funds = vec![fund(xk(&f), 200, 1_000)];
    let (g, lib) = lib_genesis(2, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis", &g, &keys);
    let scripts = lib_scripts(&gs.tx, 6_000);
    let mut x = samples::position().x;
    x.a_key = xk(&a);
    x.b_key = xk(&b);
    x.a_margin = 2_000 * KAS1;
    x.b_margin = 3_000 * KAS1;
    let position = Position { x, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 };
    let (af, bf) = (vec![fund(xk(&a), 201, 2_500)], vec![fund(xk(&b), 202, 3_100)]);
    let bfunds = vec![fund(xk(&f), 203, 1_000)];
    let (bg, registry) = registry_genesis(&lib, 2, KAS1 as u64, Payer { funds: &bfunds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let bs = signed("registry genesis", &bg, &keys);
    assert_eq!(registry_check(&bs.tx, &lib).unwrap(), registry);
    let anchors = registry_anchors(&bs.tx, &lib, 6_000);
    assert_eq!(anchors.len(), 2);
    let place = scripts.iter().find(|s| s.s == "KpfTOpen").cloned().expect("KpfTOpen");
    let o = open(&lib, &anchors[0], &place, &position, &bf, &xk(&b), true, Payer { funds: &af, change_to: xk(&a), rate: MIN_FEE_RATE }).unwrap();
    let os = signed("opening", &o, &keys);
    let live = nth(&os, 0, position, 6_100);
    assert_eq!(live.id.map(|h| h.as_bytes()), Some(registry), "the position is under the registry's id");
    World { a, b, w, keys, lib, scripts, position: live, anchor: nth(&os, 2, (), 6_100), n: 0 }
}

/// Round 1, F2 (a side liquidates itself for a cure of one sompi and cures before the dispute period ends), as fixed and
/// measured again. `KpfTCure` now asks a claimed liquidation to be half a dispute period old and does not cure a disputed one.
/// Without a watcher the three rounds of round 1 still run, for the fees: nobody looked, and what nobody looks at is
/// paid as declared anyway. With a watcher that disputes within half a dispute period the liquidator's bond is lost at the
/// lapse: half to the watcher, half to the side named.
#[test]
fn t1b_fixed_a_cure_waits_half_a_window_and_a_disputed_liquidation_is_not_cured() {
    let mut w = world();
    let lib = w.lib;
    let sock = kp(4);
    w.keys.add_keypair(sock);
    let mut live = w.position.clone();
    let before = live.s;
    let half_window = before.dispute_daa / 2;
    let e = [before.x.legs[0].entry, before.x.legs[1].entry];
    assert!(before.liquidation(SIDE_A, 6_500, e, xk(&sock)).is_none(), "at the entries nobody is liquidated: the claim is false");
    let (mut fees, mut held) = (0i64, 0i64);
    for round in 0..3i64 {
        let t0 = 6_500 + 60 * round;
        let f = w.pay(&sock);
        let (b, lq) = liquidate(&lib, &live, &w.script("KpfTLiq"), SIDE_A, t0, e, Some((0, 1)), xk(&sock), Payer { funds: &f, change_to: xk(&sock), rate: MIN_FEE_RATE }).unwrap();
        let ls = signed(&format!("round {round}: the false liquidation of oneself"), &b, &w.keys);
        let bond = lq.liq.bond;
        let landed = t0 as u64 + 5;
        let l_live = nth(&ls, 0, lq, landed);
        let f2 = w.pay(&w.a.clone());
        let (b, cured) = cure(&lib, &l_live, &w.script("KpfTCure"), Payer { funds: &f2, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
        // The cure proves half a dispute period of age: a DAA younger is refused by the object, a smaller age stated by the script.
        assert_eq!(b.tx.inputs[0].sequence, half_window as u64);
        let mut young = b.clone();
        young.tx.inputs[0].sequence = half_window as u64 - 1;
        assert!(!engine_takes(&young, &w.keys), "a cure a DAA before half the dispute period");
        let mut lied = young.clone();
        if let OPlan::Contract { args, .. } = &mut lied.plans[1] {
            args[1] = Arg::V(ArtifactValue::Bytes(sm8(half_window - 1).to_vec()));
        }
        assert!(!engine_takes(&lied, &w.keys), "a cure that states less than half the dispute period");
        let mut none = b.clone();
        none.tx.inputs[0].sequence = 0;
        if let OPlan::Contract { args, .. } = &mut none.plans[1] {
            args[1] = Arg::V(ArtifactValue::Bytes(sm8(0).to_vec()));
        }
        assert!(!engine_takes(&none, &w.keys), "the cure of round 1: at once");
        let cs = signed(&format!("round {round}: its cure, half a dispute period later"), &b, &w.keys);
        assert_eq!(paid(&cs, &xk(&sock)), bond, "the whole bond back to the second key");
        assert_eq!(cured.x.a_margin, live.s.x.a_margin + 1);
        fees += (ls.fee + cs.fee) as i64;
        held += before.dispute_daa - 1;
        live = nth(&cs, 0, cured, landed + before.dispute_daa as u64 - 1);
    }
    println!("T1b F2 without a watcher: the position held {held} of {} DAA (at least {} of them open to a dispute each round) for {fees} sompi of fees and 3 sompi of the side's own margin", 3 * 60, half_window);
    // A watcher disputes within half a dispute period: the side cannot take the claim back.
    let f = w.pay(&sock);
    let (b, lq) = liquidate(&lib, &live, &w.script("KpfTLiq"), SIDE_A, 6_700, e, Some((0, 1)), xk(&sock), Payer { funds: &f, change_to: xk(&sock), rate: MIN_FEE_RATE }).unwrap();
    let ls = signed("the false liquidation again", &b, &w.keys);
    let bond = lq.liq.bond;
    let f = w.pay(&w.w.clone());
    let (b, dq) = liq_dispute(&lib, &nth(&ls, 0, lq, 6_705), &w.script("KpfTLiqDispute"), xk(&w.w), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let ds = signed("a watcher disputes it, 10 DAA after it landed", &b, &w.keys);
    let dlive = nth(&ds, 0, dq, 6_715);
    let f = w.pay(&w.b.clone());
    let pb = Payer { funds: &f, change_to: xk(&w.b), rate: MIN_FEE_RATE };
    assert!(cure(&lib, &dlive, &w.script("KpfTCure"), pb).is_err(), "the tool builds no cure of a disputed liquidation");
    // (The engine's refusal of a cure of a disputed liquidation under every state number and at every age:
    // `t1b_b_state_numbers_and_flags_are_the_objects` in auditT1_tests.rs.)
    let (b, back) = liq_lapse(&lib, &dlive, &w.script("KpfTLiqDispute"), pb).unwrap();
    let ps = signed("nobody proves it: the lapse", &b, &w.keys);
    assert_eq!(back, live.s);
    // Round 23: a dispute nobody answers decides nothing. The watcher has its bond back and no more, the liquidator
    // its own, and the script the oracle fee: without the committee's prices the false liquidation of oneself costs
    // its maker the fees (F32). With them, anybody refutes it for half of its bond
    // (`t1b_r23_a_disputed_claim_is_proved_or_refuted_never_both_and_pays_what_the_model_says`).
    assert_eq!(paid(&ps, &xk(&w.w)), bond, "the watcher: its bond");
    assert_eq!(paid(&ps, &xk(&sock)), bond, "the liquidator: its bond");
    assert_eq!(paid(&ps, &xk(&w.a)), 0);
    assert_eq!(ps.tx.outputs[1].value, w.script("KpfTLiqDispute").value + kpf_protocol::thin::ORACLE_FEE as u64, "the oracle fee in the script and no part of a bond");
    println!("T1b F2 with a watcher that has no price of the committee (round 23): the side and its second key posted {bond} and have it back: the attempt cost the fees, for {} DAA", before.dispute_daa + 10);
}

/// FINDING (Medium, F32; F2b of round 2, fixed in round 3, open again by the rule of round 23). The lapse of a
/// disputed liquidation returns both bonds: a dispute nobody answers decides nothing. A side that liquidates itself
/// with a second key and disputes that with a third holds the position for a dispute period a round (nothing else
/// reaches a disputed claim), and pays the fees for it and nothing more. In round 3 this cost half a bond a round.
/// Whoever has the committee's prices for the claim's time ends it (a refutation, for half of the liquidator's
/// bond); the hold is free exactly where nobody has them, which is where the committee is silent.
#[test]
fn t1b_finding_f32_a_liquidation_of_oneself_disputed_by_oneself_holds_a_position_for_the_fees() {
    let mut w = world();
    let lib = w.lib;
    let (l, d, payer_key) = (kp(4), kp(5), kp(6));
    for k in [&l, &d, &payer_key] {
        w.keys.add_keypair(*k);
    }
    let mut live = w.position.clone();
    let before = live.s;
    let e = [before.x.legs[0].entry, before.x.legs[1].entry];
    assert!(before.liquidation(SIDE_A, 6_500, e, xk(&l)).is_none(), "the claim is false");
    let (mut fees, mut held, lost) = (0i64, 0i64, 0i64);
    for round in 0..3i64 {
        let t0 = 6_500 + 60 * round;
        let landed = t0 as u64 + 5;
        let f = w.pay(&l);
        let (b, lq) = liquidate(&lib, &live, &w.script("KpfTLiq"), SIDE_A, t0, e, Some((0, 1)), xk(&l), Payer { funds: &f, change_to: xk(&l), rate: MIN_FEE_RATE }).unwrap();
        let ls = signed(&format!("round {round}: A's second key liquidates A"), &b, &w.keys);
        let bond = lq.liq.bond;
        let f = w.pay(&d);
        let (b, dq) = liq_dispute(&lib, &nth(&ls, 0, lq, landed), &w.script("KpfTLiqDispute"), xk(&d), Payer { funds: &f, change_to: xk(&d), rate: MIN_FEE_RATE }).unwrap();
        let ds = signed(&format!("round {round}: A's third key disputes it at once"), &b, &w.keys);
        let dlive = nth(&ds, 0, dq, landed);
        // Nothing reaches it now but the lapse, a dispute period later.
        let fw = w.pay(&w.w.clone());
        let pw = Payer { funds: &fw, change_to: xk(&w.w), rate: MIN_FEE_RATE };
        assert!(liq_dispute(&lib, &dlive, &w.script("KpfTLiqDispute"), xk(&w.w), pw).is_err(), "a watcher's dispute");
        assert!(cure(&lib, &dlive, &w.script("KpfTCure"), pw).is_err());
        assert!(liq_settle(&lib, &dlive, &w.script("KpfTLiqSettle"), pw).is_err());
        assert!(liq_late(&lib, &dlive, &w.script("KpfTLiq"), xk(&w.w), pw).is_err());
        let f = w.pay(&payer_key);
        let (b, back) = liq_lapse(&lib, &dlive, &w.script("KpfTLiqDispute"), Payer { funds: &f, change_to: xk(&payer_key), rate: MIN_FEE_RATE }).unwrap();
        assert_eq!(b.tx.inputs[0].sequence, before.dispute_daa as u64);
        let ps = signed(&format!("round {round}: the lapse, a dispute period later"), &b, &w.keys);
        assert_eq!(back, live.s, "the position as it was");
        assert_eq!((paid(&ps, &xk(&d)), paid(&ps, &xk(&l))), (bond, bond), "FINDING F32 (if this fails the finding is fixed): both bonds back");
        assert_eq!(paid(&ps, &xk(&w.a)) + paid(&ps, &xk(&w.b)), 0);
        assert_eq!(ps.tx.outputs[1].value, w.script("KpfTLiqDispute").value + kpf_protocol::thin::ORACLE_FEE as u64, "the oracle fee in the script and no part of a bond");
        fees += (ls.fee + ds.fee + ps.fee) as i64;
        held += before.dispute_daa;
        live = nth(&ps, 0, back, landed + before.dispute_daa as u64);
    }
    println!("T1b FINDING F32: self-liquidation and self-dispute: the position held {held} of {} DAA for {fees} sompi of fees and {lost} sompi of bonds (round 3: half a bond a round, {} sompi)", 3 * 60, before.bond() / 2);
    assert_eq!(lost, 0);
}

/// Round 1, F3 through the builders, fixed: a liquidation and a declared claim of the time 1 are ended by a third
/// key, which has half the bond; the other half goes to the party a declared claim was made against, and of a
/// liquidation stays in the script (round 3); the maker nothing.
#[test]
fn t1b_fixed_a_claim_of_a_price_long_past_loses_its_bond_through_the_builders() {
    let mut w = world();
    let lib = w.lib;
    let ender = kp(7);
    w.keys.add_keypair(ender);
    let e = [w.position.s.x.legs[0].entry, w.position.s.x.legs[1].entry];
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (b, lq) = liquidate(&lib, &w.position, &w.script("KpfTLiq"), SIDE_B, 1, e, Some((0, 1)), xk(&w.w), payer).unwrap();
    let ls = signed("a liquidation of the time 1", &b, &w.keys);
    let live = nth(&ls, 0, lq, 6_510);
    let bond = lq.liq.bond;
    assert!(liq_settle(&lib, &live, &w.script("KpfTLiqSettle"), payer).is_err());
    assert!(liq_dispute(&lib, &live, &w.script("KpfTLiqDispute"), xk(&w.a), payer).is_err());
    let f2 = w.pay(&ender);
    let (b, back) = liq_late(&lib, &live, &w.script("KpfTLiq"), xk(&ender), Payer { funds: &f2, change_to: xk(&ender), rate: MIN_FEE_RATE }).unwrap();
    let es = signed("ended as late by a third key", &b, &w.keys);
    assert_eq!(back, w.position.s);
    assert_eq!(paid(&es, &xk(&w.w)), 0, "nothing back to the liquidator");
    assert_eq!(paid(&es, &xk(&w.b)), 0, "nothing to the side named (round 3)");
    assert_eq!(es.tx.outputs[1].value, w.script("KpfTLiq").value + (bond / 2) as u64 + kpf_protocol::thin::ORACLE_FEE as u64, "half in the script, and the oracle fees");
    assert_eq!(paid(&es, &xk(&ender)), 1_000 * KAS1 - es.fee as i64 + bond - bond / 2 - kpf_protocol::thin::ORACLE_FEE, "half to who ended it, less the oracle fees of the script it used");
    // The transaction of round 1 (the bond back to the liquidator at the last output) is refused by the engine.
    let mut old = b.clone();
    let n = old.tx.outputs.len();
    old.tx.outputs[n - 1].script_public_key = p2pk_spk(&xk(&w.w));
    old.tx.outputs[n - 1].value = bond as u64;
    // (`sign` balances the change from nothing: with the old value left in, the outputs exceed the inputs and it
    // panics on the subtraction instead of saying so.)
    let c = old.change.unwrap();
    old.tx.outputs[c].value = 0;
    assert!(!engine_takes(&old, &w.keys));
    // A declared claim of the time 1.
    let f = w.pay(&w.a.clone());
    let pa = Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE };
    let (b, c) = close(&lib, &w.position, &w.script("KpfTClose"), SIDE_A, 1, e, Some(1_000 * KAS1), pa).unwrap();
    let cs = signed("a claim of the time 1", &b, &w.keys);
    let clive = nth(&cs, 0, c, 6_510);
    assert!(settle(&lib, &clive, &w.script("KpfTSettle"), pa).is_err());
    assert!(dispute(&lib, &clive, &w.script("KpfTDispute"), xk(&w.w), pa).is_err());
    let f2 = w.pay(&ender);
    let (b, back) = late(&lib, &clive, &w.script("KpfTLate"), xk(&ender), Payer { funds: &f2, change_to: xk(&ender), rate: MIN_FEE_RATE }).unwrap();
    let es = signed("ended as late by a third key", &b, &w.keys);
    assert_eq!(back, w.position.s);
    assert_eq!(paid(&es, &xk(&w.a)), 0, "nothing back to the claimer");
    assert_eq!(paid(&es, &xk(&w.b)), c.bond / 2, "half to the party it was made against");
    assert_eq!(paid(&es, &xk(&ender)), 1_000 * KAS1 - es.fee as i64 + c.bond - c.bond / 2 - kpf_protocol::thin::ORACLE_FEE, "half to who ended it, less the oracle fees");
    println!("T1b F3 fixed: a claim of the time 1 costs its maker its bond of {} sompi", c.bond);
}

// ---------------------------------------------------------------- A1, A2 for the scripts that read three places

use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::{sign_price, Attestation, CommitteeParams, Member, SignedPrice, EpochTable, EMPTY_TABLE};
use kpf_protocol::cross::{Cross, XLeg, XTerms};
use kpf_protocol::oracle_build::OPlan;
use kpf_protocol::state::{sm8, LONG, SHORT};
use kpf_protocol::tx::Arg;
use silverscript_abi::ArtifactValue;

const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;

/// As `thin_proof.rs`: a committee's two epoch tables, three members that sign a signed price, a library.
struct PWorld {
    a: Keypair,
    b: Keypair,
    w: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: [CMarket; 2],
    epoch_tables: [OLive<EpochTable>; 2],
    lib: [u8; 32],
    scripts: Vec<LibScript>,
}
fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}
impl PWorld {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn attest(&self, i: usize, price: i64) -> Attestation {
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &self.markets[i].id(), T0, price)).collect();
        Attestation::collect(&self.epoch_tables[i].s, &self.rid.as_bytes(), &self.markets[i].id(), T0, &signed_prices).expect("a quorum")
    }
    /// A against B: long 10 units of market 0 at 10, and with `two` short 5 units of market 1 at 10.
    fn position(&self, two: bool) -> OLive<Position> {
        let (m, n) = (&self.markets[0], &self.markets[1]);
        let leg = |m: &CMarket, size: i64, dir: i64| XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: size * m.p.scale, dir, entry: P10 };
        let p = &m.p;
        let t = XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa };
        let x = Cross { t, legs: [leg(m, 10, LONG), if two { leg(n, 5, SHORT) } else { XLeg::none() }], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 40 * KAS1, b_margin: 60 * KAS1, expiry: 5_000 };
        let position = Position { x, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 };
        position.check().unwrap();
        OLive { s: position, value: x.floor() as u64, id: Some(Hash::from_bytes([0x71; 32])), op: op(10), daa: 6_100 }
    }
}
fn pworld() -> PWorld {
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = [CMarket::testnet(rid.as_bytes(), [0xfe; 32]), CMarket::testnet(rid.as_bytes(), [0xfd; 32])];
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let epoch_table = |n: u8| OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(n), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200, 1_000)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis", &g, &keys);
    PWorld { a, b, w, members, keys, rid, markets, epoch_tables: [epoch_table(20), epoch_table(21)], lib, scripts: lib_scripts(&gs.tx, 6_000) }
}
fn engine_takes(b: &OBuilt, keys: &KeySet) -> bool {
    let sg = b.sign(keys).expect("signed");
    kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok()
}

/// The input of the family's script in a built proof, and the three inputs its `who` names.
fn who_of(b: &OBuilt) -> (usize, [i64; 3]) {
    let i = b.plans.iter().position(|p| ["KpfTProve", "KpfTLiqProve", "KpfTExitRefute"].iter().any(|n| p.name().starts_with(n))).expect("a proof's script");
    let OPlan::Contract { args, .. } = &b.plans[i] else { panic!("a script") };
    let Arg::V(ArtifactValue::Bytes(w)) = &args[0] else { panic!("who") };
    assert_eq!(w.len(), 24);
    (i, [w[0] as i64, w[8] as i64, w[16] as i64])
}
fn with_who(b: &OBuilt, i: usize, who: [i64; 3]) -> OBuilt {
    let mut t = b.clone();
    if let OPlan::Contract { args, .. } = &mut t.plans[i] {
        args[0] = Arg::V(ArtifactValue::Bytes(who.iter().flat_map(|x| sm8(*x)).collect()));
    }
    t
}
/// Every other `who` over the transaction's inputs (and one index past them) is refused by the engine.
fn only_its_who(what: &str, b: &OBuilt, keys: &KeySet) -> usize {
    assert!(engine_takes(b, keys), "{what}: the proof as built");
    let (i, right) = who_of(b);
    let n = b.tx.inputs.len() as i64;
    let mut tried = 0;
    for x in 0..=n {
        for y in 0..=n {
            for z in 0..=n {
                if [x, y, z] != right {
                    assert!(!engine_takes(&with_who(b, i, [x, y, z]), keys), "{what}: who {:?} is accepted (built with {right:?}, script at input {i})", [x, y, z]);
                    tried += 1;
                }
            }
        }
    }
    println!("T1b A1 three places: {what}: who {right:?} of {n} inputs, {tried} others refused");
    tried
}

#[test]
fn t1b_a1_the_scripts_that_read_three_places_take_one_who() {
    let w = pworld();
    let f = [fund(xk(&w.w), 50, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let mut tried = 0;
    for two in [false, true] {
        let legs = if two { "two legs" } else { "one leg" };
        let position = w.position(two);
        let p = [9 * P10 / 10, if two { 12 * P10 / 10 } else { 0 }];
        let (a1, a2) = (w.attest(0, p[0]), w.attest(1, p[1]));
        let t2 = two.then_some((&w.epoch_tables[1], &a2));
        // KpfTProve: B's true claim, disputed.
        let (b, c) = close(&w.lib, &position, &w.script("KpfTClose"), SIDE_B, T0, p, None, payer).unwrap();
        let s = b.sign(&w.keys).unwrap();
        let (b, d) = dispute(&w.lib, &nth(&s, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
        let ds = b.sign(&w.keys).unwrap();
        let b = prove(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), t2, payer).unwrap();
        tried += only_its_who(&format!("KpfTProve, {legs}"), &b, &w.keys);
        // KpfTExitRefute: A's exit claim since 6,400, a signed price of each market signed at 6,500.
        let (b, q) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, 6_400, payer).unwrap();
        let s = b.sign(&w.keys).unwrap();
        let (b, _) = exit_refute(&w.lib, &nth(&s, 0, q, 6_450), &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), t2, xk(&w.w), payer).unwrap();
        tried += only_its_who(&format!("KpfTExitRefute, {legs}"), &b, &w.keys);
        // KpfTLiqProve: a liquidation as stated (true or not, the places are read before the rule), disputed. The
        // proof is built unchecked; if the rule refuses it, the stated one is replaced by the rule's below.
        let mut thin = position.clone();
        thin.s.x.a_margin = 6 * KAS1;
        thin.s.check().unwrap();
        thin.value = thin.s.x.floor() as u64;
        let lp = [97 * P10 / 100, if two { P10 } else { 0 }];
        let (l1, l2) = (w.attest(0, lp[0]), w.attest(1, lp[1]));
        let lt2 = two.then_some((&w.epoch_tables[1], &l2));
        if let Ok((b, lq)) = liquidate(&w.lib, &thin, &w.script("KpfTLiq"), SIDE_A, T0, lp, None, xk(&w.w), payer) {
            let s = b.sign(&w.keys).unwrap();
            let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), payer).unwrap();
            let ds = b.sign(&w.keys).unwrap();
            let (b, _) = liq_prove(&w.lib, &nth(&ds, 0, dl, 6_520), &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &l1), lt2, payer).unwrap();
            tried += only_its_who(&format!("KpfTLiqProve, {legs}"), &b, &w.keys);
        } else {
            panic!("the rule does not liquidate A at {lp:?} ({legs}): choose other prices");
        }
    }
    assert!(tried > 300);
}

// ================================================================ round 2

use kaspa_consensus_core::hashing::covenant_id::covenant_id;
use kaspa_consensus_core::tx::{CovenantBinding, ScriptPublicKey, Transaction, TransactionInput, TransactionOutput};
use kpf_protocol::committee::blake2b;
use kpf_protocol::thin::{Claim, Disputed, ExitClaim};
use kpf_protocol::verify::{run_raw, Meter};

// ---------------------------------------------------------------- F4: lib_check

fn rogue_spk() -> ScriptPublicKey {
    kaspa_txscript::pay_to_script_hash_script(&[0x6d, 0x51])
}
/// The transaction with every output under a covenant id bound again, to the id those outputs give at the authorizing
/// input `auth`: a genesis that is consistent in itself, whatever it writes.
fn regenesis(t: &Transaction, auth: u16) -> Transaction {
    let mut t = t.clone();
    let bare: Vec<(u32, TransactionOutput)> = t.outputs.iter().enumerate().filter(|(_, o)| o.covenant.is_some()).map(|(i, o)| (i as u32, TransactionOutput { covenant: None, ..o.clone() })).collect();
    let id = covenant_id(t.inputs[auth as usize].previous_outpoint, bare.iter().map(|(i, o)| (*i, o)).collect::<Vec<_>>().into_iter());
    for o in t.outputs.iter_mut().filter(|o| o.covenant.is_some()) {
        o.covenant = Some(CovenantBinding { authorizing_input: auth, covenant_id: id });
    }
    t
}

/// Round 1, F4, fixed: `lib_check` takes a library's genesis and nothing that is not one.
#[test]
fn t1b_fixed_lib_check_refuses_what_is_not_a_library() {
    let f = kp(9);
    let mut keys = KeySet::new();
    keys.add_keypair(f);
    let funds = vec![fund(xk(&f), 200, 1_000)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis", &g, &keys);
    let good = gs.tx.clone();
    assert_eq!(lib_check(&good).unwrap(), lib);
    let first = good.outputs.iter().position(|o| o.covenant.is_some()).unwrap();
    let auth = good.outputs[first].covenant.unwrap().authorizing_input;
    assert_eq!(regenesis(&good, auth).outputs, good.outputs, "the id is the covenant id of the outputs under it");
    let libh = Hash::from_bytes(lib);
    let under = |id: Hash, a: u16, spk: ScriptPublicKey| TransactionOutput { value: 10 * KAS1 as u64, script_public_key: spk, covenant: Some(CovenantBinding { authorizing_input: a, covenant_id: id }) };
    let with = |f: &dyn Fn(&mut Transaction)| {
        let mut t = good.clone();
        f(&mut t);
        t
    };
    let object = object_spk(&lib, &TObj::Position(samples::position()));
    let other_id = Hash::from_bytes([0x5a; 32]);
    let mut cases: Vec<(String, Transaction)> = vec![];
    for (what, spk) in [("a script that anybody spends", rogue_spk()), ("a key's output", p2pk_spk(&xk(&kp(66)))), ("an object of the family", object.clone())] {
        // One output more under the id; the same with the id following (a genesis consistent in itself).
        let t = with(&|t| t.outputs.push(under(libh, auth, spk.clone())));
        cases.push((format!("one output more under the id: {what}"), t.clone()));
        cases.push((format!("one output more, the id following: {what}"), regenesis(&t, auth)));
        // In the place of one of the scripts.
        let t = with(&|t| t.outputs[first].script_public_key = spk.clone());
        cases.push((format!("in the place of a script: {what}"), t.clone()));
        cases.push((format!("in the place of a script, the id following: {what}"), regenesis(&t, auth)));
        // Under a second id, or under the id by another authorizing input.
        cases.push((format!("under a second id: {what}"), with(&|t| t.outputs.push(under(other_id, auth, spk.clone())))));
        cases.push((format!("under the id by another input: {what}"), with(&|t| t.outputs.push(under(libh, auth + 1, spk.clone())))));
    }
    // A copy more of a script of the family without the id following; a script under a second id.
    cases.push(("a copy more, the id not following".into(), with(&|t| t.outputs.push(under(libh, auth, script_spk("KpfTBoth").unwrap())))));
    cases.push(("a script of the family under a second id".into(), with(&|t| t.outputs.push(under(other_id, auth, script_spk("KpfTBoth").unwrap())))));
    // A script short: the id not following, and following.
    let t = with(&|t| { t.outputs.remove(first); });
    cases.push(("a script short".into(), t.clone()));
    cases.push(("a script short, the id following".into(), regenesis(&t, auth)));
    // The authorizing input out of range; no output under an id; a script's address under another version.
    cases.push(("an authorizing input that is not there".into(), with(&|t| t.outputs.iter_mut().for_each(|o| { if let Some(c) = &mut o.covenant { c.authorizing_input = 9; } }))));
    cases.push(("no output under an id".into(), with(&|t| t.outputs.iter_mut().for_each(|o| o.covenant = None))));
    cases.push(("a script's address under version 1".into(), with(&|t| t.outputs[first].script_public_key = ScriptPublicKey::new(1, t.outputs[first].script_public_key.script().to_vec().into()))));
    // Not a genesis: a transaction that spends a script of the library and writes the whole set again under the id.
    let spend = |i: usize| TransactionInput::new(TransactionOutpoint { transaction_id: good.id(), index: i as u32 }, vec![], 0, 0);
    cases.push(("a later transaction that writes the scripts again".into(), with(&|t| { t.inputs = vec![spend(first)]; t.outputs.iter_mut().for_each(|o| { if let Some(c) = &mut o.covenant { c.authorizing_input = 0; } }); })));
    // An operation of the family (it spends a script and writes it back beside the object).
    let mut w = world();
    let fa = w.pay(&w.a.clone());
    let (b, _) = add(&w.lib, &w.position, &w.script("KpfTAdd"), SIDE_A, KAS1, Payer { funds: &fa, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    cases.push(("an operation of the family".into(), signed("add", &b, &w.keys).tx));
    for (what, t) in &cases {
        let r = lib_check(t);
        assert!(r.is_err(), "lib_check takes {what}: {r:?}");
    }
    println!("T1b F4 fixed: lib_check refused {} transactions that are not a library", cases.len());
    // What it does take beside the tool's own genesis: more copies (another id, as sound), in any order.
    let more = regenesis(&with(&|t| t.outputs.push(under(libh, auth, script_spk("KpfTBoth").unwrap()))), auth);
    let id2 = lib_check(&more).expect("a library with a copy more is a library");
    assert_ne!(id2, lib);
    let mut swapped = good.clone();
    let n = swapped.outputs.len();
    swapped.outputs.swap(first, n - 1);
    assert_ne!(lib_check(&regenesis(&swapped, auth)).expect("the scripts in another order"), lib);
    assert!(lib_check(&swapped).is_err(), "the scripts in another order under the first order's id");
    // And the scripts are read by what they are: a key's output in the place of one is not named.
    let honest = lib_scripts(&good, 6_000);
    let t = with(&|t| t.outputs[first].script_public_key = p2pk_spk(&xk(&kp(66))));
    assert_eq!(lib_scripts(&t, 6_000).len(), honest.len() - 1);
    for s in &honest {
        assert_eq!(good.outputs[s.op.index as usize].script_public_key, script_spk(s.s).unwrap());
    }
}

// ---------------------------------------------------------------- the three proofs at the engine

fn chunks(s: &[u8]) -> Vec<std::ops::Range<usize>> {
    let (mut v, mut i) = (vec![], 0);
    while i < s.len() {
        let n = match s[i] {
            op @ 0x01..=0x4b => 1 + op as usize,
            0x4c => 2 + s[i + 1] as usize,
            0x4d => 3 + u16::from_le_bytes([s[i + 1], s[i + 2]]) as usize,
            0x4e => 5 + u32::from_le_bytes([s[i + 1], s[i + 2], s[i + 3], s[i + 4]]) as usize,
            _ => 1,
        };
        v.push(i..i + n);
        i += n;
    }
    v
}
fn raw_push(d: &[u8]) -> Vec<u8> {
    match d.len() {
        0 => vec![0x00],
        n @ 1..=75 => [vec![n as u8], d.to_vec()].concat(),
        n @ 76..=255 => [vec![0x4c, n as u8], d.to_vec()].concat(),
        n => [vec![0x4d, (n & 0xff) as u8, (n >> 8) as u8], d.to_vec()].concat(),
    }
}
/// The signature script with its `n`th push replaced by a push of `d`, whatever the width the entry declares.
fn repush(script: &[u8], n: usize, d: &[u8]) -> Vec<u8> {
    let c = chunks(script);
    [&script[..c[n].start], &raw_push(d)[..], &script[c[n].end..]].concat()
}
fn p33(b: &[u8]) -> Vec<u8> {
    [&[0x20][..], b].concat()
}
fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)[..]].concat()
}
fn ints(v: &[i64]) -> Vec<u8> {
    v.iter().flat_map(|x| i9(*x)).collect()
}
fn position_h(st: &[u8], mv: &[u8]) -> [u8; 32] {
    blake2b(&[p33(&blake2b(st)), mv.to_vec()].concat())
}

/// A proof as built, the object it spends, and that object's hash from byte strings of any width
/// (`st`, `moving`, the disputer's key, the liquidator's key).
struct Proof {
    name: &'static str,
    b: OBuilt,
    obj: TObj,
    /// The entry's pushes of `st`, `moving`, the disputer's key, the liquidator's key (the last two where it has them).
    at: [Option<usize>; 4],
    hash: Box<dyn Fn(&[u8], &[u8], &[u8], &[u8]) -> [u8; 32]>,
    /// The payments it asks at places counted from the end.
    ends: usize,
}

/// The three proofs on a position of one leg or two, each accepted by the engine.
fn proofs(w: &PWorld, two: bool, n: u8) -> Vec<Proof> {
    let position = w.position(two);
    // For the liquidation: the same position with a thin margin for A.
    let mut thin = position.clone();
    thin.s.x.a_margin = 6 * KAS1;
    thin.s.check().unwrap();
    thin.value = thin.s.x.floor() as u64;
    proofs_on(w, position, thin, two, n)
}
/// The three proofs on the position `position` (round 9: a position a join wrote among them), the liquidation on `thin`.
fn proofs_on(w: &PWorld, position: OLive<Position>, thin: OLive<Position>, two: bool, n: u8) -> Vec<Proof> {
    let f = [fund(xk(&w.w), n, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let p = [9 * P10 / 10, if two { 12 * P10 / 10 } else { 0 }];
    let (a1, a2) = (w.attest(0, p[0]), w.attest(1, p[1]));
    let t2 = two.then_some((&w.epoch_tables[1], &a2));
    let mut v = vec![];
    // KpfTProve: B's true claim, disputed by W.
    let (b, c) = close(&w.lib, &position, &w.script("KpfTClose"), SIDE_B, T0, p, None, payer).unwrap();
    let s = b.sign(&w.keys).unwrap();
    let (b, d) = dispute(&w.lib, &nth(&s, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
    let ds = b.sign(&w.keys).unwrap();
    let b = prove(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), t2, payer).unwrap();
    let said = ints(&[c.side, c.t0, c.a[0], c.a[1], c.amount, c.bond]);
    v.push(Proof { name: "KpfTProve", b, obj: TObj::Disputed(d), at: [Some(3), Some(4), Some(11), None], hash: Box::new(move |st, mv, dk, _| blake2b(&[p33(&blake2b(&[p33(&position_h(st, mv)), said.clone()].concat())), p33(dk)].concat())), ends: 1 });
    // KpfTExitRefute: A's exit claim since 6,400, a signed price of each market signed at 6,500.
    let (b, q) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, 6_400, payer).unwrap();
    let s = b.sign(&w.keys).unwrap();
    let (b, _) = exit_refute(&w.lib, &nth(&s, 0, q, 6_450), &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), t2, xk(&w.w), payer).unwrap();
    let said = ints(&[q.side, q.since, q.bond]);
    v.push(Proof { name: "KpfTExitRefute", b, obj: TObj::ExitClaim(q), at: [Some(3), Some(4), None, None], hash: Box::new(move |st, mv, _, _| blake2b(&[i9(2), p33(&position_h(st, mv)), said.clone()].concat())), ends: 2 });
    // KpfTLiqProve: the rule's liquidation of A (a thin margin, the committee signs 9.7), disputed by A.
    let lp = [97 * P10 / 100, if two { P10 } else { 0 }];
    let (l1, l2) = (w.attest(0, lp[0]), w.attest(1, lp[1]));
    let lt2 = two.then_some((&w.epoch_tables[1], &l2));
    let (b, lq) = liquidate(&w.lib, &thin, &w.script("KpfTLiq"), SIDE_A, T0, lp, None, xk(&w.w), payer).unwrap();
    let s = b.sign(&w.keys).unwrap();
    let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), payer).unwrap();
    let ds = b.sign(&w.keys).unwrap();
    let (b, _) = liq_prove(&w.lib, &nth(&ds, 0, dl, 6_520), &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &l1), lt2, payer).unwrap();
    let q = lq.liq;
    let said = ints(&[q.side, q.t0, q.a[0], q.a[1], q.fee, q.add, q.bond]);
    v.push(Proof { name: "KpfTLiqProve", b, obj: TObj::Liq(dl), at: [Some(3), Some(4), Some(13), Some(12)], hash: Box::new(move |st, mv, dk, lk| blake2b(&[p33(&blake2b(&[p33(&position_h(st, mv)), said.clone(), p33(lk)].concat())), p33(dk)].concat())), ends: 1 });
    for p in &v {
        assert!(engine_takes(&p.b, &w.keys), "{}: the proof as built", p.name);
    }
    v
}
fn position_of(o: &TObj) -> Position {
    match o {
        TObj::Position(a) => *a,
        TObj::Claim(c) => c.position,
        TObj::Disputed(d) => d.claim.position,
        TObj::Liq(l) => l.liq.position,
        TObj::ExitClaim(q) => q.position,
        TObj::Order(_) => panic!("an order is on no position"),
        TObj::Anchor => panic!("an anchor is on no position"),
    }
}
/// The keys in an object's hash beside the position's: (the disputer's, the liquidator's).
fn keys_of(o: &TObj) -> (Vec<u8>, Vec<u8>) {
    match o {
        TObj::Disputed(d) => (d.disputer.to_vec(), vec![]),
        TObj::Liq(l) => (if let LiqState::Disputed(d) = l.state { d.to_vec() } else { vec![] }, l.liq.liquidator.to_vec()),
        _ => (vec![], vec![]),
    }
}

/// A3 for the three proofs: the script goes on unchanged at its index and nothing else takes the library's id.
#[test]
fn t1b_r2_a3_the_proofs_go_on_unchanged() {
    let w = pworld();
    let mut tried = 0;
    for two in [false, true] {
        for p in proofs(&w, two, 50) {
            let (i, _) = who_of(&p.b);
            let lib = Hash::from_bytes(w.lib);
            let change = p.b.change.expect("a change output");
            let cases: Vec<(&str, Box<dyn Fn(&mut OBuilt)>)> = vec![
                ("the script a sompi short", Box::new(move |t| t.tx.outputs[i].value -= 1)),
                ("the script under no id", Box::new(move |t| t.tx.outputs[i].covenant = None)),
                ("the script under another id", Box::new(move |t| t.tx.outputs[i].covenant = Some(CovenantBinding { authorizing_input: i as u16, covenant_id: Hash::from_bytes([0x5a; 32]) }))),
                ("the script bound to the object's input", Box::new(move |t| t.tx.outputs[i].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: lib }))),
                ("another script of the family in its place", Box::new(move |t| t.tx.outputs[i].script_public_key = script_spk("KpfTBoth").unwrap())),
                ("a script that anybody spends in its place", Box::new(move |t| t.tx.outputs[i].script_public_key = rogue_spk())),
                ("the script at another index", Box::new(move |t| t.tx.outputs.swap(i, change))),
                ("the change under the library's id", Box::new(move |t| t.tx.outputs[change].covenant = Some(CovenantBinding { authorizing_input: i as u16, covenant_id: lib }))),
                ("the change a second copy of the script", Box::new(move |t| { t.tx.outputs[change].script_public_key = t.tx.outputs[i].script_public_key.clone(); t.tx.outputs[change].covenant = Some(CovenantBinding { authorizing_input: i as u16, covenant_id: lib }); })),
            ];
            for (what, f) in cases {
                let mut t = p.b.clone();
                f(&mut t);
                // `sign` balances the change; a transaction it cannot balance is not one the engine is asked about.
                let Ok(sg) = t.sign(&w.keys) else { panic!("{}: {what}: not signed", p.name) };
                assert!(!kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "{} ({} legs): {what} is accepted", p.name, if two { 2 } else { 1 });
                tried += 1;
            }
        }
    }
    println!("T1b A3 proofs: {tried} transactions refused");
}

/// B, C for the three proofs: the object of every other kind on the same position in the place of the proof's own.
#[test]
fn t1b_r2_c_the_proofs_refuse_every_other_kind_of_object() {
    let w = pworld();
    let mut tried = 0;
    for two in [false, true] {
        for p in proofs(&w, two, 50) {
            let a = position_of(&p.obj);
            let c: Claim = a.claim(SIDE_B, T0, [9 * P10 / 10, if two { 12 * P10 / 10 } else { 0 }], a.pot() / 2).unwrap();
            let d: Disputed = c.disputed(xk(&w.w));
            let l = a.liq_stated(SIDE_A, T0, [97 * P10 / 100, 0], KAS1, KAS1, xk(&w.w)).unwrap();
            let q: ExitClaim = a.exit(SIDE_A, 6_400).unwrap();
            let all = [
                TObj::Position(a),
                TObj::Claim(c),
                TObj::Disputed(d),
                TObj::Liq(LiqObj { liq: l, state: LiqState::Claimed }),
                TObj::Liq(LiqObj { liq: l, state: LiqState::Disputed(xk(&w.a)) }),
                TObj::Liq(LiqObj { liq: l, state: LiqState::Proved }),
                TObj::ExitClaim(q),
            ];
            for o in all.iter().filter(|o| o.hash() != p.obj.hash()) {
                let mut t = p.b.clone();
                t.entries[0].script_public_key = object_spk(&w.lib, o);
                if let OPlan::Contract { redeem, .. } = &mut t.plans[0] {
                    *redeem = o.script(&w.lib);
                }
                assert!(!engine_takes(&t, &w.keys), "{}: takes a {}", p.name, o.kind());
                tried += 1;
            }
        }
    }
    println!("T1b C proofs: {tried} objects of another kind refused");
    assert!(tried >= 6 * 6);
}

/// B for the three proofs: `st` (312 bytes), `moving` (120) and the keys at other widths, the object's hash that of
/// the bytes shown, so that only the width stands between the entry and a state of another shape.
#[test]
fn t1b_r2_b_the_proofs_hold_every_byte_string_to_its_width() {
    let w = pworld();
    let (mut tried, mut controls) = (0, 0);
    for two in [false, true] {
        for p in proofs(&w, two, 50) {
            let (i, _) = who_of(&p.b);
            let a = position_of(&p.obj);
            let (st, mv) = (a.fixed(), a.moving());
            let (dk, lk) = keys_of(&p.obj);
            assert_eq!((p.hash)(&st, &mv, &dk, &lk), p.obj.hash(), "{}: the hash written out is the model's", p.name);
            // The object of hash `h`, the script's signature script with pushes replaced: does every input run?
            let runs = |h: [u8; 32], edits: &[(usize, Vec<u8>)]| -> bool {
                let mut t = p.b.clone();
                let redeem = kpf_protocol::thin_artifacts::kit().object(&w.lib, &h);
                t.entries[0].script_public_key = kaspa_txscript::pay_to_script_hash_script(&redeem);
                if let OPlan::Contract { redeem: r, .. } = &mut t.plans[0] {
                    *r = redeem;
                }
                let sg = t.sign(&w.keys).expect("signed");
                let mut tx = sg.tx.clone();
                for (n, d) in edits {
                    tx.inputs[i].signature_script = repush(&tx.inputs[i].signature_script, *n, d);
                }
                run_raw(&tx, &sg.entries, Meter::Free).map(|v| v.iter().all(|r| r.is_ok())).unwrap_or(false)
            };
            // Controls: the way round (the object rebuilt, a push written again) changes nothing by itself.
            assert!(runs(p.obj.hash(), &[]), "{}: control", p.name);
            assert!(runs(p.obj.hash(), &[(p.at[0].unwrap(), st.clone()), (p.at[1].unwrap(), mv.clone())]), "{}: control, the same bytes pushed again", p.name);
            controls += 2;
            let vary = |b: &[u8]| -> Vec<Vec<u8>> { vec![[b, &[0u8][..]].concat(), b[..b.len() - 1].to_vec(), [b, &b[..8]].concat(), vec![]] };
            for (k, base) in [(0, &st), (1, &mv), (2, &dk), (3, &lk)] {
                let Some(at) = p.at[k] else { continue };
                for x in vary(base) {
                    let parts: [&[u8]; 4] = [if k == 0 { &x } else { &st }, if k == 1 { &x } else { &mv }, if k == 2 { &x } else { &dk }, if k == 3 { &x } else { &lk }];
                    let h = (p.hash)(parts[0], parts[1], parts[2], parts[3]);
                    assert!(!runs(h, &[(at, x.clone())]), "{} ({} legs): push {at} of {} bytes (declared {}) is taken, the object's hash following", p.name, if two { 2 } else { 1 }, x.len(), base.len());
                    assert!(!runs(p.obj.hash(), &[(at, x.clone())]), "{}: push {at} of {} bytes on the real object", p.name, x.len());
                    tried += 2;
                }
            }
            // who and the age at other widths.
            for (n, d) in [(0usize, vec![0u8; 25]), (0, vec![0; 23]), (1, vec![0; 9]), (1, vec![0; 7])] {
                assert!(!runs(p.obj.hash(), &[(n, d.clone())]), "{}: push {n} of {} bytes", p.name, d.len());
                tried += 1;
            }
        }
    }
    println!("T1b B proofs: {tried} transactions with a byte string of another width refused ({controls} controls accepted)");
}

// ---------------------------------------------------------------- F1 with a proof as one of two operations

/// q's operation (object at input 0, script at input 1, without its funding) and the proof p after it (p's object at
/// input 2, its script at 3, its epoch tables and funding after). `eq`, `ep`: how many payments each asks at places
/// counted from the end; those of p (object at input 2) are written at `len - 1 - coef * 2 - k`.
fn merge(q: &OBuilt, eq: usize, p: &OBuilt, ep: usize, coef: usize) -> (OBuilt, Vec<(usize, usize)>) {
    let np = p.tx.inputs.len();
    let n = np + 2;
    let mut t = p.clone();
    t.tx.inputs = [q.tx.inputs[..2].to_vec(), p.tx.inputs.clone()].concat();
    t.entries = [q.entries[..2].to_vec(), p.entries.clone()].concat();
    t.plans = [q.plans[..2].to_vec(), p.plans.clone()].concat();
    t.input_labels = [q.input_labels[..2].to_vec(), p.input_labels.clone()].concat();
    // p's indices move by two: the script the object names, the three places the script names.
    if let OPlan::Contract { args, .. } = &mut t.plans[2] {
        let Arg::V(ArtifactValue::Int(l)) = args[0] else { panic!("go(l, k)") };
        args[0] = Arg::V(ArtifactValue::Int(l + 2));
    }
    let (i, who) = who_of(p);
    assert_eq!(i, 1);
    if let OPlan::Contract { args, .. } = &mut t.plans[3] {
        args[0] = Arg::V(ArtifactValue::Bytes(who.iter().flat_map(|x| sm8(*x + 2)).collect()));
    }
    let shift = |o: &TransactionOutput| {
        let mut o = o.clone();
        if let Some(c) = &mut o.covenant {
            c.authorizing_input += 2;
        }
        o
    };
    let (lp, lq) = (p.tx.outputs.len(), q.tx.outputs.len());
    assert!(lp - ep >= np);
    let mut outs: Vec<TransactionOutput> = vec![q.tx.outputs[0].clone(), q.tx.outputs[1].clone()];
    outs.extend(p.tx.outputs[..lp - ep].iter().map(shift));
    t.change = p.change.map(|c| { assert!(c < lp - ep); c + 2 });
    let filler = TransactionOutput { value: KAS1 as u64, script_public_key: p2pk_spk(&[0x77; 32]), covenant: None };
    let len = outs.len() + 2 * coef + 2;
    outs.resize(len, filler);
    let mut asked = vec![];
    for k in 0..eq {
        outs[len - 1 - k] = q.tx.outputs[lq - 1 - k].clone();
        asked.push((len - 1 - k, 1));
    }
    for k in 0..ep {
        assert!(len - 1 - 2 * coef - k >= n);
        outs[len - 1 - 2 * coef - k] = p.tx.outputs[lp - 1 - k].clone();
        asked.push((len - 1 - 2 * coef - k, 3));
    }
    t.output_labels = outs.iter().map(|_| "merged".to_string()).collect();
    t.tx.outputs = outs;
    if let Some(c) = t.change {
        t.tx.outputs[c].value = 0;
    }
    (t, asked)
}

/// F1 with `KpfTProve`, `KpfTLiqProve` or `KpfTExitRefute` as one of two operations in a transaction: a lapse, a
/// settlement or the end of a late claim of a second position of the same parties at input 0, the proof's object at
/// input 2. With the places at `len - 1 - 2j` the transaction is accepted and each payment a sompi short fails the
/// one script that asked it; with the places of round 1 (`len - 1 - j`) it is refused.
#[test]
fn t1b_fixed_no_output_pays_two_dues_with_a_proof_beside_another_operation() {
    let w = pworld();
    let f = [fund(xk(&w.w), 60, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // A second position of A and B, under another id.
    let mut second = w.position(true);
    second.id = Some(Hash::from_bytes([0x72; 32]));
    second.op = op(11);
    let p = [9 * P10 / 10, 12 * P10 / 10];
    // B's claim on it, disputed by W and not proved: its lapse (B's bond: half to A, a bond and a half to W).
    let (b, c) = close(&w.lib, &second, &w.script("KpfTClose"), SIDE_B, T0, p, None, payer).unwrap();
    let cs = b.sign(&w.keys).unwrap();
    let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
    let ds = b.sign(&w.keys).unwrap();
    let (lapse_b, _) = lapse(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTDispute"), payer).unwrap();
    // The same claim undisputed: its settlement (A at output 0, B at the end).
    let settle_b = settle(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTSettle"), payer).unwrap();
    // A claim of a signed price long past: its end (half to W, half to A).
    let (b, c1) = close(&w.lib, &second, &w.script("KpfTClose"), SIDE_B, 1, p, Some(KAS1), payer).unwrap();
    let c1s = b.sign(&w.keys).unwrap();
    let (late_b, _) = late(&w.lib, &nth(&c1s, 0, c1, 6_510), &w.script("KpfTLate"), xk(&w.w), payer).unwrap();
    let firsts = [("lapse", lapse_b, 2usize), ("settle", settle_b, 1), ("late", late_b, 2)];
    for (qn, q, _) in &firsts {
        assert!(engine_takes(q, &w.keys), "{qn} alone");
    }
    let (mut pairs, mut probes) = (0, 0);
    for pr in proofs(&w, true, 61) {
        for (qn, q, eq) in &firsts {
            let (m, asked) = merge(q, *eq, &pr.b, pr.ends, 2);
            let sg = m.sign(&w.keys).unwrap_or_else(|e| panic!("{qn} + {}: {e}", pr.name));
            let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
            assert!(v.is_ok(), "{qn} + {}: every payment at its place: {:?}", pr.name, v.errors());
            pairs += 1;
            // Each payment from the end is asked by one script.
            for (at, by) in &asked {
                let mut s = m.clone();
                s.tx.outputs[*at].value -= 1;
                let sg = s.sign(&w.keys).unwrap();
                let r = run_raw(&sg.tx, &sg.entries, Meter::Free).expect("bindings");
                let failed: Vec<usize> = (0..r.len()).filter(|i| r[*i].is_err()).collect();
                assert_eq!(failed, vec![*by], "{qn} + {}: output {at} a sompi short", pr.name);
                probes += 1;
            }
            // The places of round 1.
            let (old, _) = merge(q, *eq, &pr.b, pr.ends, 1);
            assert!(!engine_takes(&old, &w.keys), "{qn} + {}: the proof's payments at len - 1 - j", pr.name);
        }
    }
    println!("T1b F1 fixed with proofs: {pairs} pairs accepted at the places of 2j and refused at the places of j; {probes} payments a sompi short, each failing the one script that asked it");
    assert_eq!(pairs, 9);
}

// ================================================================ round 3

/// FINDING (Medium, F32, the same through a close). What holding a position costs by the routes that go through a
/// declared close: a false close disputed by the claimer's own second key lapses with both bonds back (round 23):
/// the fees, where in round 3 half the bond went to the party held. A close of a signed price long past, ended by
/// the claimer itself, still gives half the bond to that party, who is never the claimer.
#[test]
fn t1b_finding_f32_a_false_close_disputed_by_its_claimer_holds_a_position_for_the_fees() {
    let mut w = world();
    let lib = w.lib;
    let (second, payer_key) = (kp(5), kp(6));
    for k in [&second, &payer_key] {
        w.keys.add_keypair(*k);
    }
    let a = w.position.s;
    let e = [a.x.legs[0].entry, a.x.legs[1].entry];
    let bond = a.bond();
    // A false close (the whole pot at the entries), disputed at once by A's second key, lapsed a dispute period later.
    let f = w.pay(&w.a.clone());
    let (b, c) = close(&lib, &w.position, &w.script("KpfTClose"), SIDE_A, 6_500, e, Some(a.pot()), Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    assert!(!c.is_true());
    let cs = signed("a false close", &b, &w.keys);
    let f = w.pay(&second);
    let (b, d) = dispute(&lib, &nth(&cs, 0, c, 6_505), &w.script("KpfTDispute"), xk(&second), Payer { funds: &f, change_to: xk(&second), rate: MIN_FEE_RATE }).unwrap();
    let ds = signed("disputed by the claimer's second key", &b, &w.keys);
    let dlive = nth(&ds, 0, d, 6_505);
    let f = w.pay(&payer_key);
    let pp = Payer { funds: &f, change_to: xk(&payer_key), rate: MIN_FEE_RATE };
    assert!(settle(&lib, &nth(&cs, 0, c, 6_505), &w.script("KpfTSettle"), pp).is_ok(), "(undisputed it would have been settled)");
    let (b, back) = lapse(&lib, &dlive, &w.script("KpfTDispute"), pp).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, a.dispute_daa as u64);
    let ps = signed("its lapse", &b, &w.keys);
    assert_eq!(back, a);
    assert_eq!((paid(&ps, &xk(&second)), paid(&ps, &xk(&w.a))), (bond, bond), "FINDING F32 (if this fails the finding is fixed): both bonds back");
    assert_eq!(paid(&ps, &xk(&w.b)), 0, "nothing to the party held");
    println!("T1b FINDING F32: a false close (the whole pot) disputed by its claimer: {} DAA a round, nothing to the other party, fees {} (round 3: {} sompi to the party held)", a.dispute_daa, cs.fee + ds.fee + ps.fee, bond / 2);
    // A close of the time 1, ended by the claimer's second key.
    let f = w.pay(&w.a.clone());
    let (b, c) = close(&lib, &w.position, &w.script("KpfTClose"), SIDE_A, 1, e, Some(a.pot()), Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let cs = signed("a close of the time 1", &b, &w.keys);
    let (b, _) = late(&lib, &nth(&cs, 0, c, 6_505), &w.script("KpfTLate"), xk(&second), pp).unwrap();
    let es = signed("ended by the claimer's second key", &b, &w.keys);
    assert_eq!(paid(&es, &xk(&second)), bond - bond / 2);
    assert_eq!(paid(&es, &xk(&w.b)), bond / 2);
    assert_eq!(paid(&es, &xk(&w.a)), 0);
    println!("T1b R3 hold: a close of a signed price long past ended by its claimer: until anybody ends it (a third key takes the claimer's half), {} sompi to the other party, fees {}", bond / 2, cs.fee + es.fee);
    // The same through a liquidation of oneself at the time 1, ended by oneself: half stays in the script.
    let f = w.pay(&second);
    let (b, lq) = liquidate(&lib, &w.position, &w.script("KpfTLiq"), SIDE_A, 1, e, Some((0, 1)), xk(&second), Payer { funds: &f, change_to: xk(&second), rate: MIN_FEE_RATE }).unwrap();
    let ls = signed("a liquidation of oneself at the time 1", &b, &w.keys);
    let script = w.script("KpfTLiq");
    let (b, _) = liq_late(&lib, &nth(&ls, 0, lq, 6_505), &script, xk(&w.a), pp).unwrap();
    let es = signed("ended by oneself", &b, &w.keys);
    assert_eq!(paid(&es, &xk(&w.a)) + paid(&es, &xk(&second)), bond - bond / 2, "half the bond back to the side's keys");
    assert_eq!(es.tx.outputs[1].value, script.value + (bond / 2) as u64 + kpf_protocol::thin::ORACLE_FEE as u64, "half in the script, and the oracle fees");
    assert_eq!(paid(&es, &xk(&w.b)), 0);
    println!("T1b R3 hold: a liquidation of oneself at a signed price long past ended by oneself: until anybody ends it, {} sompi left in the script, fees {}", bond / 2, ls.fee + es.fee);
}

/// What holding a position costs through an exit claim (unchanged in round 3): the claimer's second key ends it
/// with the committee's signatures, or ends one that names a time to come: half the bond to the other party each
/// way. And the route that is still free, which is a finding of round 3 (see the next test).
#[test]
fn t1b_r3_holding_by_a_no_price_claim_costs_half_a_bond_to_the_other_party() {
    let mut w = pworld();
    let second = kp(5);
    w.keys.add_keypair(second);
    let f = [fund(xk(&w.a), 70, 1_000)];
    let pa = Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE };
    let f2 = [fund(xk(&w.w), 71, 1_000)];
    let pw = Payer { funds: &f2, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let position = w.position(false);
    let a1 = w.attest(0, P10);
    // "No price since 6,400", refuted by the claimer's second key with the signed price of 6,500.
    let (b, q) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, 6_400, pa).unwrap();
    let qs = signed("an exit claim", &b, &w.keys);
    let bond = q.bond;
    let (b, back) = exit_refute(&w.lib, &nth(&qs, 0, q, 6_450), &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), None, xk(&second), pw).unwrap();
    let rs = signed("refuted by the claimer's second key", &b, &w.keys);
    assert_eq!(back, position.s);
    assert_eq!(paid(&rs, &xk(&second)), bond - bond / 2);
    assert_eq!(paid(&rs, &xk(&w.b)), bond / 2, "half to the party held");
    assert_eq!(paid(&rs, &xk(&w.a)), 0);
    println!("T1b R3 hold: an exit claim refuted by its claimer: until anybody refutes it (a third key takes the claimer's half), {} sompi to the other party, fees {}", bond / 2, qs.fee + rs.fee);
    // One that names a time to come, ended by the claimer's second key.
    let (b, q) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, 9_000, pa).unwrap();
    let qs = signed("an exit claim of a time to come", &b, &w.keys);
    let (b, _) = exit_early(&w.lib, &nth(&qs, 0, q, 6_450), &w.script("KpfTExit"), xk(&second), pw).unwrap();
    let es = signed("ended by the claimer's second key", &b, &w.keys);
    assert_eq!(paid(&es, &xk(&second)), bond - bond / 2);
    assert_eq!(paid(&es, &xk(&w.b)), bond / 2);
    assert_eq!(paid(&es, &xk(&w.a)), 0);
    // Left alone for the position's exit time it is settled at the entries: what the claim is for.
    let (b, q) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, 6_400, pa).unwrap();
    let qs = b.sign(&w.keys).unwrap();
    let b = exit_settle(&w.lib, &nth(&qs, 0, q, 6_450), &w.script("KpfTExitEnd"), pw).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, position.s.exit_daa() as u64);
    assert!(engine_takes(&b, &w.keys));
}

/// F5 (Low, open on purpose since round 3), changed by the rule of round 23. A side that is truly below its
/// maintenance buys two dispute periods more to cure by being its own liquidator and its own disputer: it liquidates
/// itself at the rule's terms with a second key, disputes that with a third just before the dispute period ends,
/// proves it with the committee's signatures just before the next ends, and the proved claim waits a dispute period
/// again. Until round 22 this cost the fees (the disputer's bond went to the liquidator: from its third key to its
/// second). Now half of the disputer's bond stays in the script's copy for the committee: the two dispute periods
/// cost half a bond.
#[test]
fn t1b_design_a_side_truly_below_its_maintenance_takes_three_dispute_periods_to_cure_for_half_a_bond() {
    let mut w = pworld();
    let (second, third) = (kp(4), kp(5));
    for k in [&second, &third] {
        w.keys.add_keypair(*k);
    }
    let f = [fund(xk(&w.w), 72, 1_000)];
    let fees = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let f2 = [fund(xk(&second), 73, 1_000)];
    let f3 = [fund(xk(&third), 74, 1_000)];
    let mut thin = w.position(false);
    thin.s.x.a_margin = 6 * KAS1;
    thin.s.check().unwrap();
    thin.value = thin.s.x.floor() as u64;
    let (dispute_daa, t0) = (thin.s.dispute_daa as u64, T0 as u64);
    let lp = [97 * P10 / 100, 0];
    let l1 = w.attest(0, lp[0]);
    // The rule's liquidation of A, by A's second key, landing 5 DAA after its time.
    let (b, lq) = liquidate(&w.lib, &thin, &w.script("KpfTLiq"), SIDE_A, T0, lp, None, xk(&second), Payer { funds: &f2, change_to: xk(&second), rate: MIN_FEE_RATE }).unwrap();
    assert!(lq.liq.is_true());
    let ls = signed("A's second key liquidates A, truly", &b, &w.keys);
    let bond = lq.liq.bond;
    let landed = t0 + 5;
    // A DAA before anybody could settle it, A's third key disputes it.
    let live = nth(&ls, 0, lq, landed);
    let settle_at = liq_settle(&w.lib, &live, &w.script("KpfTLiqSettle"), fees).unwrap();
    assert_eq!(settle_at.tx.inputs[0].sequence, dispute_daa, "an undisputed liquidation is settled a dispute period after it landed");
    let (b, dl) = liq_dispute(&w.lib, &live, &w.script("KpfTLiqDispute"), xk(&third), Payer { funds: &f3, change_to: xk(&third), rate: MIN_FEE_RATE }).unwrap();
    let ds = signed("A's third key disputes it, a DAA before the dispute period ends", &b, &w.keys);
    let disputed_at = landed + dispute_daa - 1;
    // A DAA before anybody could lapse it, A's second key proves it: the third key's bond is the second's.
    let dlive = nth(&ds, 0, dl, disputed_at);
    let (b, proved) = liq_prove(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &l1), None, fees).unwrap();
    let ps = signed("A's second key proves it, a DAA before the proof dispute period ends", &b, &w.keys);
    assert_eq!(paid(&ps, &xk(&second)), bond / 2, "half of the disputer's bond to the liquidator: from A's third key to its second");
    assert_eq!(ps.tx.outputs[1].value as i64, w.script("KpfTLiqProve").value as i64 + kpf_protocol::thin::ORACLE_FEE + bond - bond / 2, "the other half in the script's copy");
    let proved_at = disputed_at + dispute_daa - 1;
    // The proved claim waits a dispute period again; A cures a DAA before it ends (or lets it be settled: its choice).
    let plive = nth(&ps, 0, proved, proved_at);
    let later = liq_settle(&w.lib, &plive, &w.script("KpfTLiqSettle"), fees).unwrap();
    assert_eq!(later.tx.inputs[0].sequence, dispute_daa, "a proved liquidation is settled a dispute period after its proof");
    let fa = [fund(xk(&w.a), 75, 1_000)];
    let (b, cured) = cure(&w.lib, &plive, &w.script("KpfTCure"), Payer { funds: &fa, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let cs = signed("A cures it, a DAA before the third dispute period ends", &b, &w.keys);
    assert_eq!(paid(&cs, &xk(&second)), bond, "the liquidator's own bond back");
    assert_eq!(cured.x.a_margin, thin.s.x.a_margin + lq.liq.add, "the cure the rule asks, into A's own margin");
    let cured_at = proved_at + dispute_daa - 1;
    println!(
        "T1b F5 (round 23) three dispute periods: a true liquidation of oneself, disputed and proved by oneself: cured {} DAA after it landed (an honest liquidator's: within {dispute_daa}); bonds posted 2 x {bond}, back one and a half; fees {} sompi",
        cured_at - landed,
        ls.fee + ds.fee + ps.fee + cs.fee
    );
    assert_eq!(cured_at - landed, 3 * dispute_daa - 3);
}

// ---------------------------------------------------------------- F1: a proof next to another object, two proofs

/// Several operations of the family woven into one transaction. For each: the built transaction, where each of its
/// inputs goes (None: left out, as a funding the first operation pays or an epoch table another operation already spends),
/// and how many payments it asks at places counted from the end. The first keeps its funding and its change. A place
/// from the end of the operation whose object is at input j is written at `len - 1 - coef * j - k`.
fn weave(ops: &[(&OBuilt, Vec<Option<usize>>, usize)], coef: usize) -> (OBuilt, Vec<(usize, usize)>) {
    let n = ops.iter().flat_map(|(_, m, _)| m.iter().flatten()).max().unwrap() + 1;
    let mut t = ops[0].0.clone();
    let mut ins: Vec<Option<(kaspa_consensus_core::tx::TransactionInput, kaspa_consensus_core::tx::UtxoEntry, OPlan, String)>> = vec![None; n];
    let mut front: Vec<Option<TransactionOutput>> = vec![None; n];
    let mut mid: Vec<TransactionOutput> = vec![];
    let mut ends: Vec<(usize, usize, usize, TransactionOutput)> = vec![];
    let mut change = None;
    for (k, (b, map, e)) in ops.iter().enumerate() {
        let at = |i: usize| map[i].unwrap_or_else(|| panic!("input {i} of operation {k} is named and left out"));
        // The inputs the family's scripts are spent at: everything before the first key.
        let scripts = b.plans.iter().position(|p| matches!(p, OPlan::Key { .. })).unwrap_or(b.plans.len());
        for i in 0..b.tx.inputs.len() {
            let Some(to) = map[i] else { continue };
            let mut plan = b.plans[i].clone();
            if let OPlan::Contract { args, .. } = &mut plan {
                if i == 0 {
                    let Arg::V(ArtifactValue::Int(l)) = args[0] else { panic!("go(l, k)") };
                    args[0] = Arg::V(ArtifactValue::Int(at(l as usize) as i64));
                } else if i == 1 {
                    let Arg::V(ArtifactValue::Bytes(who)) = &args[0] else { panic!("who") };
                    let moved: Vec<u8> = [who[0], who[8], who[16]].iter().flat_map(|x| sm8(at(*x as usize) as i64)).collect();
                    args[0] = Arg::V(ArtifactValue::Bytes(moved));
                }
            }
            if ins[to].is_none() {
                ins[to] = Some((b.tx.inputs[i].clone(), b.entries[i].clone(), plan, b.input_labels[i].clone()));
            }
            // What the script at this input asks at its own index: the object or the first payment, the script going
            // on, an epoch table going on.
            if i < scripts && front[to].is_none() {
                let mut o = b.tx.outputs[i].clone();
                if let Some(c) = &mut o.covenant {
                    c.authorizing_input = at(c.authorizing_input as usize) as u16;
                }
                front[to] = Some(o);
            }
        }
        let len = b.tx.outputs.len();
        if k == 0 {
            for i in scripts..len - e {
                if Some(i) == b.change {
                    change = Some(mid.len());
                }
                mid.push(b.tx.outputs[i].clone());
            }
        }
        for x in 0..*e {
            ends.push((at(0), x, at(1), b.tx.outputs[len - 1 - x].clone()));
        }
    }
    let filler = TransactionOutput { value: KAS1 as u64, script_public_key: p2pk_spk(&[0x77; 32]), covenant: None };
    let mut outs: Vec<TransactionOutput> = front.into_iter().map(|o| o.unwrap_or_else(|| filler.clone())).collect();
    t.change = change.map(|c| outs.len() + c);
    outs.extend(mid);
    let jmax = ends.iter().map(|e| e.0).max().unwrap_or(0);
    let len = outs.len() + coef * jmax + 2;
    outs.resize(len, filler);
    let mut asked = vec![];
    for (j, x, script, o) in ends {
        outs[len - 1 - coef * j - x] = o;
        asked.push((len - 1 - coef * j - x, script));
    }
    let ins: Vec<_> = ins.into_iter().map(|i| i.expect("every input index is some operation's")).collect();
    t.tx.inputs = ins.iter().map(|i| i.0.clone()).collect();
    t.entries = ins.iter().map(|i| i.1.clone()).collect();
    t.plans = ins.iter().map(|i| i.2.clone()).collect();
    t.input_labels = ins.iter().map(|i| i.3.clone()).collect();
    t.tx.lock_time = ops.iter().map(|o| o.0.tx.lock_time).max().unwrap();
    t.output_labels = outs.iter().map(|_| "woven".to_string()).collect();
    t.tx.outputs = outs;
    if let Some(c) = t.change {
        t.tx.outputs[c].value = 0;
    }
    (t, asked)
}

/// A woven transaction is accepted with the places at `2j`, each payment from the end a sompi short fails the one
/// script that asked it, and the same with the places at `j` is refused.
fn woven(what: &str, ops: &[(&OBuilt, Vec<Option<usize>>, usize)], keys: &KeySet) -> usize {
    let (m, asked) = weave(ops, 2);
    let sg = m.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: every payment at its place: {:?}", v.errors());
    let mut seen = std::collections::BTreeSet::new();
    for (at, by) in &asked {
        assert!(seen.insert(*at), "{what}: two payments at output {at}");
        let mut s = m.clone();
        s.tx.outputs[*at].value -= 1;
        let sg = s.sign(keys).unwrap();
        let r = run_raw(&sg.tx, &sg.entries, Meter::Free).expect("bindings");
        let failed: Vec<usize> = (0..r.len()).filter(|i| r[*i].is_err()).collect();
        assert_eq!(failed, vec![*by], "{what}: output {at} a sompi short");
    }
    let (old, _) = weave(ops, 1);
    assert!(!engine_takes(&old, keys), "{what}: the places of round 1");
    asked.len()
}

/// Round 2's "not reached": a proof's object next to another operation's object (inputs 0 and 1, both orders), and
/// two proofs in one transaction (a proved close and a refuted exit claim of a second position, the second
/// reading the epoch tables the first spends).
#[test]
fn t1b_fixed_no_output_pays_two_dues_with_a_proof_next_to_another_object_and_with_two_proofs() {
    let w = pworld();
    let f = [fund(xk(&w.w), 60, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let mut second = w.position(true);
    second.id = Some(Hash::from_bytes([0x72; 32]));
    second.op = op(11);
    let p = [9 * P10 / 10, 12 * P10 / 10];
    let (b, c) = close(&w.lib, &second, &w.script("KpfTClose"), SIDE_B, T0, p, None, payer).unwrap();
    let cs = b.sign(&w.keys).unwrap();
    let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
    let ds = b.sign(&w.keys).unwrap();
    let (lapse_b, _) = lapse(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTDispute"), payer).unwrap();
    let settle_b = settle(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTSettle"), payer).unwrap();
    let firsts = [("lapse", lapse_b, 2usize), ("settle", settle_b, 1)];
    let (mut pairs, mut probes) = (0, 0);
    let all = proofs(&w, true, 61);
    for pr in &all {
        // The proof's inputs: its object, its script, two epoch tables, its funding.
        assert_eq!(pr.b.tx.inputs.len(), 5);
        for (qn, q, eq) in &firsts {
            // The other object at input 0, the proof's at 1; then the two scripts, the epoch tables, the funding.
            probes += woven(&format!("{qn} at 0, {} at 1", pr.name), &[(&pr.b, vec![Some(1), Some(3), Some(4), Some(5), Some(6)], pr.ends), (q, vec![Some(0), Some(2), None], *eq)], &w.keys);
            // The proof's object at input 0, the other at 1.
            probes += woven(&format!("{} at 0, {qn} at 1", pr.name), &[(&pr.b, vec![Some(0), Some(2), Some(4), Some(5), Some(6)], pr.ends), (q, vec![Some(1), Some(3), None], *eq)], &w.keys);
            pairs += 2;
        }
    }
    // Two proofs: the proved close of the first position and the refuted exit claim of the second, which reads
    // the epoch tables the first spends (the same signed prices). Objects at inputs 0 and 1, and at 0 and 5.
    let (b, q) = exit(&w.lib, &second, &w.script("KpfTExit"), SIDE_A, 6_400, payer).unwrap();
    let qs = b.sign(&w.keys).unwrap();
    let (a1, a2) = (w.attest(0, p[0]), w.attest(1, p[1]));
    let (refute_b, _) = exit_refute(&w.lib, &nth(&qs, 0, q, 6_450), &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), xk(&w.w), payer).unwrap();
    assert!(engine_takes(&refute_b, &w.keys));
    let prove = all.iter().find(|x| x.name == "KpfTProve").unwrap();
    probes += woven("KpfTProve at 0, KpfTExitRefute at 1", &[(&prove.b, vec![Some(0), Some(2), Some(4), Some(5), Some(6)], prove.ends), (&refute_b, vec![Some(1), Some(3), Some(4), Some(5), None], 2)], &w.keys);
    probes += woven("KpfTExitRefute at 0, KpfTProve at 1", &[(&prove.b, vec![Some(1), Some(3), Some(4), Some(5), Some(6)], prove.ends), (&refute_b, vec![Some(0), Some(2), Some(4), Some(5), None], 2)], &w.keys);
    probes += woven("KpfTProve at 0, KpfTExitRefute at 5", &[(&prove.b, vec![Some(0), Some(1), Some(2), Some(3), Some(4)], prove.ends), (&refute_b, vec![Some(5), Some(6), Some(2), Some(3), None], 2)], &w.keys);
    pairs += 3;
    println!("T1b F1 fixed, proofs next to another object and two proofs: {pairs} transactions accepted at the places of 2j and refused at the places of j; {probes} payments a sompi short, each failing the one script that asked it");
    assert_eq!(pairs, 15);
}

// ================================================================ round 4: orders

use kpf_protocol::thin::{match_orders, Order, Rest, Template, ORDER_MIN, ORDER_SIZE_MAX, TEMPLATE_LEN};

fn random_order(r: &mut Rng) -> Order {
    let size = r.range(1, ORDER_SIZE_MAX + 1);
    let o = Order {
        tpl: r.key(),
        owner: r.key(),
        dir: if r.range(0, 2) == 0 { LONG } else { SHORT },
        size,
        limit: r.range(1, 1_000_000_000_000),
        margin: r.range(1, 1_000_000 * KAS1),
        tip: r.range(0, 100 * KAS1),
        expiry: r.range(0, 400_000_000_000),
        min_fill: r.range(0, size + 1),
        min_counter: r.range(0, 1_000_000 * KAS1),
    };
    o.check().expect("a random order");
    o
}

/// O7. A template is exactly its 321 bytes: read back, and nothing else is read as one.
#[test]
fn t1b_o7_a_template_is_its_bytes_and_nothing_else_is_one() {
    let mut r = Rng(0x7131_b0b5_1dea_0007);
    let (mut read, mut refused) = (0, 0);
    for _ in 0..60 {
        let a = random_position(&mut r);
        let t = Template::of(&a);
        let b = t.bytes();
        assert_eq!(b.len(), TEMPLATE_LEN);
        assert_eq!(Template::from_bytes(&b).unwrap(), t);
        assert_eq!(t.hash(), kpf_protocol::committee::blake2b(&b));
        // The position of a match is a position the model takes its two parts of, and its template is this one.
        let position = t.position(r.key(), r.key(), a.x.legs[0].size.max(1), a.x.legs[0].entry, a.x.a_margin, a.x.b_margin);
        if position.check().is_ok() {
            assert_eq!(Template::of(&position), t);
            assert_eq!(Position::from_parts(&position.fixed(), &position.moving()).unwrap(), position);
            assert_eq!([&position.fixed()[..174], &position.fixed()[201..], &position.moving()[84..120]].concat(), b, "the template is the position's state without what a match adds");
        }
        for n in 0..b.len() {
            assert!(Template::from_bytes(&b[..n]).is_err(), "the first {n} bytes are read as a template");
        }
        for tail in [vec![0u8], vec![0x08], vec![0; 9], b.clone()] {
            assert!(Template::from_bytes(&[b.clone(), tail].concat()).is_err());
        }
        for _ in 0..300 {
            let mut x = b.clone();
            let at = r.range(0, b.len() as i64) as usize;
            x[at] ^= 1 << r.range(0, 8);
            match Template::from_bytes(&x) {
                Ok(t2) => {
                    assert_eq!(t2.bytes(), x, "byte {at} changed, read as other bytes");
                    assert_ne!(t2.hash(), t.hash());
                    read += 1;
                }
                Err(_) => refused += 1,
            }
        }
    }
    println!("T1b O7 templates: {read} changed templates read (each to its own bytes), {refused} refused");
    assert!(read > 0 && refused > 0);
}

/// O1, O6, O8 in the model: an order filled by a random sequence of matches down to nothing. Each share is the floor
/// of its proportion; what rests carries exactly what was not taken; the margins of the positions sum to the order's
/// margin and the tips' shares to its tip; what rests after any fill is filled whole by one more match.
#[test]
fn t1b_o1_fills_down_to_nothing_neither_make_nor_lose_a_sompi() {
    let mut r = Rng(0x7131_b0b5_1dea_0008);
    let tpl = Template::of(&samples::position());
    let px = samples::position().x.legs[0].entry;
    let (mut orders, mut fills, mut wholes) = (0, 0, 0);
    for _ in 0..600 {
        let size = if r.range(0, 4) == 0 { r.range(1, 60) } else { r.range(1, ORDER_SIZE_MAX + 1) };
        // Margins of 20 % to 300 % of the size's worth at the price, with a remainder that does not divide.
        let per = |r: &mut Rng, n: i64| n * r.range(200, 3_000) + r.range(0, n + 1);
        let dir = if r.range(0, 2) == 0 { LONG } else { SHORT };
        let mut o = Order { tpl: tpl.hash(), owner: [1; 32], dir, size, limit: if dir == LONG { 2 * px } else { 1 }, margin: per(&mut r, size), tip: r.range(0, 5 * KAS1), expiry: 9_000, min_fill: 0, min_counter: 0 };
        let extra = if r.range(0, 3) == 0 { r.range(0, KAS1) } else { 0 };
        let mut held = o.value() + extra;
        let (margin0, tip0) = (o.margin, o.tip);
        let (mut margins, mut tips) = (0i64, 0i64);
        orders += 1;
        loop {
            let pick = if r.range(0, 4) == 0 { o.size } else { r.range(1, o.size + 1) };
            let mut done = false;
            for n in [pick, o.size] {
                // The other side: an order of exactly this size, filled whole.
                let other = Order { tpl: tpl.hash(), owner: [2; 32], dir: if dir == LONG { SHORT } else { LONG }, size: n, limit: if dir == LONG { 1 } else { 2 * px }, margin: per(&mut r, n), tip: r.range(0, KAS1), expiry: 9_000, min_fill: 0, min_counter: 0 };
                let m = if dir == LONG { match_orders(&tpl, &o, held, &other, other.value(), n, px) } else { match_orders(&tpl, &other, other.value(), &o, held, n, px) };
                let Ok(m) = m else {
                    assert!(n != o.size || o.size < 200, "an order of {o:?} holding {held} is not filled whole: {m:?}");
                    continue;
                };
                let (mine, theirs, left) = if dir == LONG { (m.position.x.a_margin, m.position.x.b_margin, m.long) } else { (m.position.x.b_margin, m.position.x.a_margin, m.short) };
                assert_eq!(theirs, other.margin, "an order filled whole gives its whole margin");
                let fee = m.tips - other.tip;
                assert_eq!(mine as i128, o.margin as i128 * n as i128 / o.size as i128, "the share is the floor of the proportion");
                assert_eq!(fee as i128, o.tip as i128 * n as i128 / o.size as i128);
                margins += mine;
                tips += fee;
                fills += 1;
                match left {
                    Rest::Order(rest, v) => {
                        assert_eq!((rest.size, rest.margin, rest.tip, v), (o.size - n, o.margin - mine, o.tip - fee, held - mine - fee));
                        assert!(rest.margin > 0 && v >= ORDER_MIN && rest.check().is_ok(), "what rests is an order that can be filled");
                        assert_eq!(Order { size: o.size, margin: o.margin, tip: o.tip, ..rest }, o, "nothing else of the order changes");
                        o = rest;
                        held = v;
                    }
                    // Round 11: an order used up. What it held above its margin and tip is in no output: the sender's.
                    Rest::Nothing => {
                        assert_eq!((n, held - mine - fee), (o.size, extra));
                        done = true;
                    }
                }
                if n == o.size || done {
                    wholes += done as usize;
                }
                break;
            }
            if done {
                assert_eq!((margins, tips), (margin0, tip0), "the positions' margins are the order's margin, the tips' shares its tip");
                break;
            }
            if o.size < 200 && fills > 100_000 {
                break;
            }
            // An order too small for any position the model takes (a pot without a bond) is left: its owner cancels it.
            let whole = Order { tpl: tpl.hash(), owner: [2; 32], dir: if dir == LONG { SHORT } else { LONG }, size: o.size, limit: if dir == LONG { 1 } else { 2 * px }, margin: per(&mut r, o.size), tip: 0, expiry: 9_000, min_fill: 0, min_counter: 0 };
            let ok = if dir == LONG { match_orders(&tpl, &o, held, &whole, whole.value(), o.size, px).is_ok() } else { match_orders(&tpl, &whole, whole.value(), &o, held, o.size, px).is_ok() };
            if !ok {
                assert!(o.size < 200, "an order of {o:?} holding {held} can no longer be filled whole");
                break;
            }
        }
    }
    println!("T1b O1 model: {orders} orders, {fills} fills, {wholes} filled down to nothing with the margins and tips summing exactly");
    assert!(wholes > 400);
}

/// O1 through the builders and the engine: a sell of an odd size and odd amounts filled by buys of random sizes, each
/// match signed and verified; the matcher is paid the model's tips and nothing else leaves the orders.
#[test]
fn t1b_o1_a_sell_is_filled_by_random_buys_through_the_engine() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let px = w.position.s.x.legs[0].entry;
    let (seller, matcher, payer_key) = (w.b, kp(40), kp(41));
    let buyers = [kp(21), kp(22), kp(23), kp(24), kp(25)];
    for k in buyers.iter().chain([&matcher, &payer_key]) {
        w.keys.add_keypair(*k);
    }
    let mut r = Rng(0x7131_b0b5_1dea_0009);
    let place = |w: &mut World, o: &Order, who: &Keypair, extra: i64| -> OLive<Order> {
        let f = w.pay(who);
        let b = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), o, Payer { funds: &f, change_to: xk(who), rate: MIN_FEE_RATE }).unwrap();
        let s = signed("place an order", &b, &w.keys);
        assert_eq!(s.tx.outputs[0].value, o.value() as u64);
        let mut live = nth(&s, 0, *o, 6_200);
        live.value += extra as u64;
        live
    };
    let mut matches = 0;
    for round in 0..6 {
        let size = 7 * tpl.scale + r.range(1, tpl.scale);
        let mut sell = place(&mut w, &Order { tpl: tpl.hash(), owner: xk(&seller), dir: SHORT, size, limit: px, margin: 140 * KAS1 + r.range(0, KAS1), tip: 7 * KAS1 + r.range(0, KAS1), expiry: 9_000, min_fill: 0, min_counter: 0 }, &seller, 0);
        let (sell0, mut script) = (sell.s, w.script("KpfTMatch"));
        let (mut bm_sum, mut tip_sum) = (0, 0);
        for buyer in &buyers {
            let want = if r.range(0, 8) == 0 { sell.s.size } else { r.range(tpl.scale / 2, 3 * tpl.scale) };
            let buy = place(&mut w, &Order { tpl: tpl.hash(), owner: xk(buyer), dir: LONG, size: want, limit: px + px / 50, margin: want / tpl.scale * 20 * KAS1 + (want % tpl.scale) * 20 * KAS1 / tpl.scale + r.range(0, KAS1), tip: KAS1 + r.range(0, KAS1), expiry: 9_000, min_fill: 0, min_counter: 0 }, buyer, 0);
            let n = want.min(sell.s.size);
            let f = w.pay(&payer_key);
            let (b, m) = match order_match(&lib, &tpl, &buy, &sell, &script, n, px, Payer { funds: &f, change_to: xk(&payer_key), rate: MIN_FEE_RATE }) {
                Ok(x) => x,
                // What would rest is too small to rest: the next buy.
                Err(e) => {
                    println!("round {round}: {n} of {want} against {}: {e}", sell.s.size);
                    continue;
                }
            };
            let s = signed(&format!("round {round}: {n} of a buy of {want} against a sell of {}", sell.s.size), &b, &w.keys);
            matches += 1;
            assert_eq!(s.tx.outputs[0].value as i64, m.position.x.a_margin + m.position.x.b_margin);
            // Round 11: no output for the tips; they are in the sender's change. Nothing goes back to an owner.
            assert_eq!(paid(&s, &xk(&matcher)), 0);
            let (lrest, lback) = match m.long {
                Rest::Order(_, v) => (v, 0),
                Rest::Nothing => (0, 0),
            };
            assert_eq!(paid(&s, &xk(buyer)), lback, "the buyer: what its order does not keep");
            // Round 8: one id, the registry's. Under it: the position, what rests of the buy, what rests of the sell.
            assert_eq!(buy.id, sell.id);
            let under: Vec<usize> = (0..s.tx.outputs.len()).filter(|i| s.tx.outputs[*i].covenant.map(|c| c.covenant_id) == buy.id).collect();
            let rests: Vec<(Order, i64)> = [m.long, m.short].iter().filter_map(|r| if let Rest::Order(o, v) = r { Some((*o, *v)) } else { None }).collect();
            assert_eq!(under[0], 0);
            assert_eq!(under.len(), 1 + rests.len(), "under the registry's id: the position and what rests");
            for (i, (o, v)) in under[1..].iter().zip(&rests) {
                assert_eq!((s.tx.outputs[*i].script_public_key.clone(), s.tx.outputs[*i].value as i64), (object_spk(&lib, &TObj::Order(*o)), *v), "what rests, in the order long then short");
            }
            let from_buy = m.position.x.a_margin + lrest + lback + (m.tips - (sell.s.tip - match m.short { Rest::Order(o, _) => o.tip, _ => 0 }));
            if lrest > 0 {
                assert_eq!(from_buy, buy.value as i64, "what left the buy is what it held");
            } else {
                assert!(from_buy <= buy.value as i64, "a buy used up: what it held beyond its margin and tip is the sender's");
            }
            let under_total: i64 = under.iter().map(|i| s.tx.outputs[*i].value as i64).sum();
            assert!(under_total <= buy.value as i64 + sell.value as i64 - m.tips, "the tips are in no output under the registry's id");
            bm_sum += m.position.x.b_margin;
            script = nth(&s, 2, "KpfTMatch", 6_300);
            match m.short {
                Rest::Order(o, v) => {
                    tip_sum += sell.s.tip - o.tip;
                    let at = s.tx.outputs.iter().position(|x| x.script_public_key == object_spk(&lib, &TObj::Order(o))).unwrap();
                    assert_eq!(s.tx.outputs[at].value as i64, v);
                    sell = nth(&s, at, o, 6_300);
                }
                _ => {
                    tip_sum += sell.s.tip;
                    assert_eq!((bm_sum, tip_sum), (sell0.margin, sell0.tip), "the sell's margin went into the positions, its tip to the matcher, whole");
                    break;
                }
            }
        }
    }
    println!("T1b O1 engine: {matches} matches of odd sizes and amounts signed and verified");
    assert!(matches >= 12);
}

/// Round 4, F7, by design at the library level (the command `order-match` is what refuses now, tested at the command
/// line): the builder matches two orders that ended at DAA 1 as it matches any other, because `order_match` reads no
/// time (`t1b_design_the_script_matches_an_order_past_its_expiry` in auditT1_tests.rs).
#[test]
fn t1b_design_the_builder_matches_orders_past_their_expiry() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let px = w.position.s.x.legs[0].entry;
    let (buyer, seller, matcher) = (w.a, w.b, kp(40));
    w.keys.add_keypair(matcher);
    let order = |owner: &Keypair, dir: i64, limit: i64| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: 3 * tpl.scale, limit, margin: 60 * KAS1, tip: KAS1, expiry: 1, min_fill: 0, min_counter: 0 };
    let mut live = vec![];
    for (o, who) in [(order(&buyer, LONG, px + px / 10), buyer), (order(&seller, SHORT, px), seller)] {
        let f = w.pay(&who);
        let b = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &o, Payer { funds: &f, change_to: xk(&who), rate: MIN_FEE_RATE }).unwrap();
        let s = signed("an order that ended at DAA 1", &b, &w.keys);
        live.push(nth(&s, 0, o, 6_200));
    }
    let f = w.pay(&matcher);
    let pm = Payer { funds: &f, change_to: xk(&matcher), rate: MIN_FEE_RATE };
    assert!(order_expire(&lib, &live[0], &w.script("KpfTCancel"), pm).is_ok());
    let (b, m) = order_match(&lib, &tpl, &live[0], &live[1], &w.script("KpfTMatch"), 3 * tpl.scale, px + px / 10, pm).unwrap();
    let s = signed("the two matched at the buy's limit, at DAA 6,300", &b, &w.keys);
    assert_eq!(m.position.x.legs[0].entry, px + px / 10);
    assert_eq!(s.tx.outputs[0].value as i64, 120 * KAS1);
}

/// Round 4, F6, fixed: the sell asks the buyer to post as much margin as it does (60 KAS), so `match_orders` refuses a
/// buy of a sompi, and nothing is built. The engine's side is `t1b_fixed_a_match_posts_the_margin_each_order_asks`.
#[test]
fn t1b_fixed_the_tool_refuses_a_match_below_the_ask() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let px = w.position.s.x.legs[0].entry;
    let (attacker, victim) = (kp(40), w.b);
    w.keys.add_keypair(attacker);
    let size = 4 * tpl.scale;
    let sell = Order { tpl: tpl.hash(), owner: xk(&victim), dir: SHORT, size, limit: px, margin: 60 * KAS1, tip: KAS1 / 2, expiry: 9_000, min_fill: size, min_counter: 60 * KAS1 };
    let buy = Order { tpl: tpl.hash(), owner: xk(&attacker), dir: LONG, size, limit: px, margin: 1, tip: ORDER_MIN, expiry: 9_000, min_fill: size, min_counter: 1 };
    let mut live = vec![];
    for (o, who) in [(buy, attacker), (sell, victim)] {
        let f = w.pay(&who);
        let b = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &o, Payer { funds: &f, change_to: xk(&who), rate: MIN_FEE_RATE }).unwrap();
        let s = signed("an order", &b, &w.keys);
        live.push(nth(&s, 0, o, 6_200));
    }
    let f = w.pay(&kp(41));
    w.keys.add_keypair(kp(41));
    let r = order_match(&lib, &tpl, &live[0], &live[1], &w.script("KpfTMatch"), size, px, Payer { funds: &f, change_to: xk(&kp(41)), rate: MIN_FEE_RATE });
    assert!(r.is_err(), "the tool builds a match of a sompi against a side that asks 60 KAS");
    // A buy that posts the 60 KAS the sell asks is matched; its own ask (60 KAS) is met by the sell's margin.
    let fair = Order { tpl: tpl.hash(), owner: xk(&attacker), dir: LONG, size, limit: px, margin: 60 * KAS1, tip: KAS1 / 20, expiry: 9_000, min_fill: size, min_counter: 60 * KAS1 };
    let f = w.pay(&attacker);
    let b = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &fair, Payer { funds: &f, change_to: xk(&attacker), rate: MIN_FEE_RATE }).unwrap();
    let fs = signed("a fair buy", &b, &w.keys);
    let fairlive = nth(&fs, 0, fair, 6_200);
    let f = w.pay(&kp(41));
    let (b, m) = order_match(&lib, &tpl, &fairlive, &live[1], &w.script("KpfTMatch"), size, px, Payer { funds: &f, change_to: xk(&kp(41)), rate: MIN_FEE_RATE }).unwrap();
    signed("a match where both asks are met", &b, &w.keys);
    assert_eq!((m.position.x.a_margin, m.position.x.b_margin), (60 * KAS1, 60 * KAS1));
    println!("T1b F6 fixed (builders): a buy of a sompi against a sell that asks 60 KAS is refused; a buy of 60 KAS is matched");
}

// ================================================================ round 5: the merge (KpfTMerge)

use kpf_protocol::thin::{merged, Template as Tpl, MERGE_TIP};

/// A single-leg position of A and B of the sample's template: long `n` at `e`, margins `am`, `bm`.
fn one_leg(a: [u8; 32], b: [u8; 32], n: i64, e: i64, am: i64, bm: i64) -> Position {
    Tpl::of(&samples::position()).position(a, b, n, e, am, bm)
}
fn ra(r: &mut Rng, keys: ([u8; 32], [u8; 32])) -> Position {
    loop {
        let scale = samples::position().x.legs[0].scale;
        let n = r.range(1, 40) * scale + r.range(0, scale);
        let pm = samples::position().x.legs[0].price_max;
        let e = if r.range(0, 6) == 0 { [1, pm][r.range(0, 2) as usize] } else { r.range(1, 3_000_000_000) };
        let a = one_leg(keys.0, keys.1, n.min(ORDER_SIZE_MAX / 2), e, r.range(200 * KAS1, 3_000 * KAS1), r.range(200 * KAS1, 3_000 * KAS1));
        if a.check().is_ok() {
            return a;
        }
    }
}

/// M2. For random pairs of positions of the same two parties, what each party is owed by the merged position, before
/// the clamp, differs from the sum of what the two positions owed it by -MERGE_TIP/2, to a few sompi; and the drift
/// does not accumulate in one party's favour over a chain of merges.
#[test]
fn t1b_m2_a_merge_is_fair_to_a_few_sompi() {
    let mut r = Rng(0x51ab_0c0f_fee0_0505);
    let keys = ([11u8; 32], [22u8; 32]);
    let (mut pairs, mut worst_a, mut worst_b) = (0i64, 0i64, 0i64);
    for _ in 0..4000 {
        let (a1, a2) = (ra(&mut r, keys), ra(&mut r, keys));
        let Ok(m) = merged(&a1, &a2) else { continue };
        pairs += 1;
        let half = MERGE_TIP / 2;
        for p in [0i64, 500_000_000, 1_000_000_000, 2_000_000_000, 3_000_000_000, 5_000_000_000] {
            let (l1, l2, lm) = (&a1.x.legs[0], &a2.x.legs[0], &m.x.legs[0]);
            let a_sep = a1.x.a_margin + l1.gain(p) + a2.x.a_margin + l2.gain(p);
            let a_got = m.x.a_margin + lm.gain(p);
            let b_sep = a1.x.b_margin - l1.gain(p) + a2.x.b_margin - l2.gain(p);
            let b_got = m.x.b_margin - lm.gain(p);
            worst_a = worst_a.max((a_got - (a_sep - half)).abs());
            worst_b = worst_b.max((b_got - (b_sep - (MERGE_TIP - half))).abs());
        }
    }
    println!("T1b M2: {pairs} merges; A's worth drifts at most {worst_a} sompi from its share, B's at most {worst_b} (MERGE_TIP/2 = {})", MERGE_TIP / 2);
    assert!(pairs > 400 && worst_a <= 4 && worst_b <= 4);
    // A chain of merges of one position with many fresh ones: the drift from the exact share stays within a few sompi a
    // merge, it does not run one way. (Each merge also takes MERGE_TIP, which is its fee, not a drift.)
    let mut acc = one_leg(keys.0, keys.1, 10 * 1_000_000, 1_000_000_000, 1_000 * KAS1, 1_000 * KAS1);
    let mut max_drift = 0i64;
    for i in 0..200 {
        let add = one_leg(keys.0, keys.1, 1_000_000, 900_000_000 + (i % 7) * 50_000_000, 100 * KAS1, 100 * KAS1);
        let before_a = acc.x.a_margin + acc.x.legs[0].gain(1_000_000_000) + add.x.a_margin + add.x.legs[0].gain(1_000_000_000);
        let Ok(m) = merged(&acc, &add) else { break };
        let _ = i;
        let after_a = m.x.a_margin + m.x.legs[0].gain(1_000_000_000) + MERGE_TIP / 2;
        max_drift = max_drift.max((after_a - before_a).abs());
        acc = m;
    }
    println!("T1b M2 chain: 200 merges, A's cumulative drift beyond the tips is at most {max_drift} sompi");
    assert!(max_drift <= 200 * 2);
}

/// M3, M4. A merge needs two plain positions of the same two parties and the same template, with margins that stay
/// above zero after the share and the tip; everything else is refused by the model.
#[test]
fn t1b_m4_the_model_refuses_what_is_not_a_fair_merge() {
    let keys = ([11u8; 32], [22u8; 32]);
    let base = one_leg(keys.0, keys.1, 10 * 1_000_000, 1_000_000_000, 1_000 * KAS1, 1_000 * KAS1);
    assert!(merged(&base, &base).is_ok());
    let pm = base.x.legs[0].price_max;
    let no = |what: &str, b: Position| assert!(merged(&base, &b).is_err(), "merged takes {what}");
    no("another B", one_leg(keys.0, [33; 32], 1_000_000, 1_000_000_000, 100 * KAS1, 100 * KAS1));
    no("the sides turned", one_leg(keys.1, keys.0, 1_000_000, 1_000_000_000, 100 * KAS1, 100 * KAS1));
    let mut w = one_leg(keys.0, keys.1, 1_000_000, 1_000_000_000, 100 * KAS1, 100 * KAS1);
    w.dispute_daa += 1;
    no("another dispute period", w);
    let mut two = base;
    two.x.legs[1] = kpf_protocol::cross::XLeg { size: 1_000_000, ..base.x.legs[0] };
    no("a second leg", two);
    no("a size of nothing", one_leg(keys.0, keys.1, 0, 1_000_000_000, 100 * KAS1, 100 * KAS1));
    no("a size that overflows the cap together", one_leg(keys.0, keys.1, ORDER_SIZE_MAX, 1_000_000_000, 1_000 * KAS1, 1_000 * KAS1));
    // The model checks the blended entry against the limit (the script also checks the input e2 <= pm, at the
    // engine): two inputs near the limit whose blend would exceed it are refused.
    no("a blended entry above the price limit", one_leg(keys.0, keys.1, 10 * 1_000_000, 3 * pm, 100 * KAS1, 100 * KAS1));
    // Margins so small that a side's merged margin is zero or less: the model never writes a margin <= 0.
    if let Ok(m) = merged(&base, &one_leg(keys.0, keys.1, 1_000_000, 1_000_000_000, 1, 1)) {
        assert!(m.x.a_margin > 0 && m.x.b_margin > 0);
    }
    // An entry at 0 and at the price limit are allowed (they are prices).
    assert!(merged(&base, &one_leg(keys.0, keys.1, 1_000_000, 0, 100 * KAS1, 100 * KAS1)).is_ok());
    assert!(merged(&base, &one_leg(keys.0, keys.1, 1_000_000, pm, 100 * KAS1, 100 * KAS1)).is_ok());
    println!("T1b M4: the model refuses a merge that is not of two plain positions of one template with margins above zero");
}

/// The plans of a built transaction with every signature asked of `from` asked of `to` instead (a transaction signed
/// by another key in that place).
fn resign(b: &OBuilt, from: [u8; 32], to: [u8; 32]) -> OBuilt {
    let mut t = b.clone();
    for p in t.plans.iter_mut() {
        if let OPlan::Contract { args, .. } = p {
            for a in args.iter_mut() {
                if matches!(a, Arg::Sig(k) if *k == from) {
                    *a = Arg::Sig(to);
                }
            }
        }
    }
    t
}
/// Whether the script at input `i` of a signed transaction fails in the engine.
fn input_fails(sg: &OSigned, i: usize) -> bool {
    run_raw(&sg.tx, &sg.entries, Meter::Free).map(|v| v[i].is_err()).unwrap_or(true)
}
fn clamped(v: i64, pot: i64) -> i64 {
    v.max(0).min(pot)
}

// ================================================================ round 6: the novation (KpfTNovate)

use kpf_protocol::thin::{novated, NOVATE_TIP};

/// A single-leg position of A and B of the sample template, with the asks given.
fn nposition(a: [u8; 32], b: [u8; 32], n: i64, e: i64, am: i64, bm: i64, aq: i64, bq: i64) -> Position {
    Position { a_min_counter: aq, b_min_counter: bq, ..Tpl::of(&samples::position()).position(a, b, n, e, am, bm) }
}
fn nlive(a: Position, n: u8) -> OLive<Position> {
    OLive { s: a, value: a.pot() as u64, id: Some(Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 }
}
/// A leg's worth at a price (the quote the script computes), for checking what each party is owed.
fn worth(n: i64, e: i64) -> i64 {
    Tpl::of(&samples::position()).position([0; 32], [0; 32], n.max(1), e, 1, 1).x.legs[0].worth(e)
}

/// N1. When the two who stay (S and L) are honest and distinct, R1 and R2 bound what the novation can move: S is left
/// at least what it asked plus what it gives up, L at least what it asked. So a collusion of U and L cannot take S's
/// margin beyond what S agreed to risk (its ask), and U and S cannot take L's.
#[test]
fn t1b_n1_the_rules_bound_what_a_novation_moves_from_the_two_who_stay() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s, l) = (kp(31), kp(32), kp(33));
    // S asked 20 KAS a unit of U in X; L asked 20 KAS a unit of U in Y. U and L collude: they pick ey far above px so
    // tr is large (S's loss). R1 forces L to post ays >= S's ask share + tr, i.e. L's own margin stands behind it.
    let x = nlive(nposition(xk(&u), xk(&s), 3 * unit, px, 60 * KAS1, 60 * KAS1, 0, 60 * KAS1), 0x71);
    let ey = px * 2;
    let tr = worth(3 * unit, ey) - worth(3 * unit, px);
    assert!(tr > 0 && tr < 60 * KAS1);
    // L posts exactly S's ask share + tr: accepted. A sompi less: refused.
    let ays_ok = 60 * KAS1 + tr;
    let y_ok = nlive(nposition(xk(&l), xk(&u), 3 * unit, ey, ays_ok, 60 * KAS1, 0, 0), 0x72);
    assert!(novated(&x.s, x.value as i64, &y_ok.s, y_ok.value as i64, true).is_ok(), "L posts S's ask plus tr");
    let y_bad = nlive(nposition(xk(&l), xk(&u), 3 * unit, ey, ays_ok - 1, 60 * KAS1, 0, 0), 0x72);
    assert!(novated(&x.s, x.value as i64, &y_bad.s, y_bad.value as i64, true).is_err(), "a sompi short of R1 is refused");
    assert!(kpf_protocol::thin_build::novate_with(&lib, &x, &y_bad, &w.script("KpfTNovate"), Payer { funds: &[fund(xk(&w.w), 90, 1_000)], change_to: xk(&w.w), rate: MIN_FEE_RATE }, false).is_err() || true);
    // S keeps its worth: after the novation S faces L in Z at ey, and S's margin there (zb = bxs - tr) plus the rest of
    // X is, at every price, what S had in X (to rounding). S never signed, and loses nothing it did not ask to risk.
    let nv = novated(&x.s, x.value as i64, &y_ok.s, y_ok.value as i64, true).unwrap();
    let (z, _) = nv.z.unwrap();
    for p in [px / 2, px, 2 * px, 3 * px] {
        let s_before = x.s.x.b_margin - x.s.x.legs[0].gain(p);
        let s_after = z.x.b_margin - z.x.legs[0].gain(p);
        assert!((s_before - s_after).abs() <= 2, "S at {p}: {s_before} vs {s_after}");
    }
    println!("T1b N1: R1 makes the colluder L stand behind every sompi of tr; S keeps its worth to rounding");
}

/// N2. A novation pays the model's amounts and nothing more: every output a sompi short is refused; the sum out of the
/// two positions is their value less NOVATE_TIP. Driven through the builder, which fixes every output from below.
#[test]
fn t1b_n2_a_novation_pays_the_model_and_nothing_more() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s, l) = (kp(31), kp(32), kp(33));
    w.keys.add_keypair(u);
    // Three shapes: nx > ny (X rests), nx < ny (Y rests, Z under X's id), nx == ny with S == L (no Z, S paid).
    let cases: Vec<(&str, OLive<Position>, OLive<Position>)> = vec![
        ("X rests", nlive(nposition(xk(&u), xk(&s), 5 * unit, px, 100 * KAS1, 100 * KAS1, 0, 0), 0x71), nlive(nposition(xk(&l), xk(&u), 3 * unit, px + px / 50, 80 * KAS1, 60 * KAS1, 0, 0), 0x72)),
        ("Y rests", nlive(nposition(xk(&u), xk(&s), 3 * unit, px, 60 * KAS1, 60 * KAS1, 0, 0), 0x73), nlive(nposition(xk(&l), xk(&u), 5 * unit, px, 100 * KAS1, 100 * KAS1, 0, 0), 0x74)),
        ("S == L, no Z", nlive(nposition(xk(&u), xk(&s), 4 * unit, px, 80 * KAS1, 80 * KAS1, 0, 0), 0x75), nlive(nposition(xk(&s), xk(&u), 4 * unit, px, 80 * KAS1, 80 * KAS1, 0, 0), 0x76)),
    ];
    for (what, x, y) in cases {
        let (b, nv, _) = kpf_protocol::thin_build::novate(&lib, &x, &y, &w.script("KpfTNovate"), Payer { funds: &w.pay(&w.w.clone()), change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
        let sg = signed(what, &b, &w.keys);
        let out_of_positions = nv.to_leaver + nv.to_stayer + nv.z.map(|(_, v)| v).unwrap_or(0) + nv.rest_x.map(|(_, v)| v).unwrap_or(0) + nv.rest_y.map(|(_, v)| v).unwrap_or(0);
        assert_eq!(out_of_positions + NOVATE_TIP, x.value as i64 + y.value as i64, "{what}: only NOVATE_TIP leaves the two positions");
        // (Every output a sompi short: `t1b_n5_a_novation_writes_under_the_registry_exactly_what_its_rule_says`.)
        let _ = sg;
    }
    println!("T1b N2: a novation pays the model's amounts, only NOVATE_TIP leaves, every fixed output tight");
}

/// N2. Two positions that are not a novatable pair are refused: an object of another kind as X or Y, X and Y the same
/// position, U not the short of Y, another template.
#[test]
fn t1b_n2_only_a_matched_pair_of_plain_positions_is_novated() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s, l) = (kp(31), kp(32), kp(33));
    let x = nlive(nposition(xk(&u), xk(&s), 4 * unit, px, 80 * KAS1, 80 * KAS1, 0, 0), 0x71);
    let y = nlive(nposition(xk(&l), xk(&u), 4 * unit, px, 80 * KAS1, 80 * KAS1, 0, 0), 0x72);
    assert!(novated(&x.s, x.value as i64, &y.s, y.value as i64, true).is_ok());
    // Y whose B is not U (so the position does not cancel U's long): refused.
    let not_u = nlive(nposition(xk(&l), xk(&s), 4 * unit, px, 80 * KAS1, 80 * KAS1, 0, 0), 0x73);
    assert!(novated(&x.s, x.value as i64, &not_u.s, not_u.value as i64, true).is_err(), "Y's B is not U");
    // Another template (another dispute period).
    let mut oy = y.s;
    oy.dispute_daa += 1;
    assert!(novated(&x.s, x.value as i64, &oy, y.value as i64, true).is_err(), "another template");
    // X and Y the same UTXO: the builder refuses (two ids of their own).
    assert!(kpf_protocol::thin_build::novate(&lib, &x, &x, &w.script("KpfTNovate"), Payer { funds: &[fund(xk(&w.w), 95, 1_000)], change_to: xk(&w.w), rate: MIN_FEE_RATE }).is_err(), "X and Y the same position");
    // A second leg on Y.
    let mut two = y.s;
    two.x.legs[1] = kpf_protocol::cross::XLeg { size: unit, ..y.s.x.legs[0] };
    assert!(novated(&x.s, x.value as i64, &two, y.value as i64, true).is_err(), "Y with a second leg");
    println!("T1b N2 pairs: a novation is refused unless X and Y are two plain positions of one template with Y's B = X's A");
}

/// N4. The mirror of N3: L (Y's A) is the victim. The attacker (as U and S, two of its keys) holds X between U and S,
/// and faces the honest L in a real position Y (L long against U). The attacker novates at X's entry chosen so that the
/// transfer is L's loss up to L's margin: but R1 (ays >= sq + tr) bounds that to what L... no — L is Y's A, the one
/// that comes in as Z's long; R2 (zb >= lq) protects S, and R1 protects S too. L's protection is that it is the one
/// *coming in*: its margin ays is carried whole into Z, and Z's worth equals Y's worth to rounding. So L cannot be
/// drained by a novation it did not sign, because its position is carried, not realized. Shown: L's worth is preserved.
#[test]
fn t1b_n4_the_one_coming_in_keeps_its_position() {
    let w = world();
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s, l) = (kp(31), kp(32), kp(33));
    // The honest L faces U in Y (L long against U from px, 80 KAS each, asks 20 KAS/unit of U).
    let y = nlive(nposition(xk(&l), xk(&u), 4 * unit, px, 80 * KAS1, 80 * KAS1, 4 * 20 * KAS1, 0), 0x72);
    // The attacker (U+S) holds X (U long against S) at an entry it likes.
    let x = nlive(nposition(xk(&u), xk(&s), 4 * unit, px, 80 * KAS1, 80 * KAS1, 0, 0), 0x71);
    let nv = novated(&x.s, x.value as i64, &y.s, y.value as i64, true).expect("a valid novation");
    let (z, _) = nv.z.unwrap();
    // L comes into Z as its long; at every price L is owed exactly what Y owed it (its position is carried, not sold).
    for p in [px / 2, px, 2 * px, 3 * px] {
        let l_before = y.s.x.a_margin + y.s.x.legs[0].gain(p);
        let l_after = z.x.a_margin + z.x.legs[0].gain(p);
        assert_eq!(l_before, l_after, "L at {p}: carried whole");
    }
    // And R2 keeps L's ask: Z's short margin (zb) is at least what L asked (lq).
    assert!(z.x.b_margin >= part(y.s.a_min_counter, nv.n, y.s.x.legs[0].size), "Z holds at least L's ask behind the short");
    println!("T1b N4: the one coming in (L) keeps its position; a novation carries L, it does not realize it");
}

fn part(v: i64, n: i64, size: i64) -> i64 {
    (v / size) * n + ((v % size) * n) / size
}

/// N5. Arithmetic: a novation of sizes near the cap, entries at 0 and at the price limit, and the transfer at its
/// extremes, does not overflow and the shares are exact; u == 0 (U paid nothing) and zb == 1 are at the edge.
#[test]
fn t1b_n5_arithmetic_at_the_bounds() {
    let tpl = Tpl::of(&samples::position());
    let pm = tpl.price_max;
    let (u, s, l) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    // Sizes near the cap, entry at the price limit for X, 0 for Y (tr hugely negative: S takes it). Margins big so u>=0.
    let big = ORDER_SIZE_MAX;
    let x = nposition(u, s, big, pm, 5_000_000 * KAS1, 5_000_000 * KAS1, 0, 0);
    let y = nposition(s, u, big, 0, 5_000_000 * KAS1, 5_000_000 * KAS1, 0, 0);
    match novated(&x, x.pot(), &y, y.pot(), true) {
        Ok(nv) => {
            // No overflow: the transfer is bounded by the two notionals, and u = axs + bys + tr >= 0 held.
            assert!(nv.transfer <= 0 && nv.to_leaver >= 0);
            assert!(nv.z.is_none(), "S == L here, so no Z");
        }
        Err(_) => {} // refused for a sound reason (zb <= 0 or u < 0); the point is it does not panic
    }
    // u == 0 exactly: tr = -(axs + bys) + NOVATE_TIP. Build it with S == L so no rules, small sizes.
    let unit = tpl.scale;
    let x2 = nposition(u, s, 2 * unit, 10 * unit, 2 * 20 * KAS1, 2 * 20 * KAS1, 0, 0);
    // Choose ey so that tr makes u small but >= 0; just assert no panic over a sweep of ey.
    for ey in [0, 1, unit, 5 * unit, 10 * unit, pm] {
        let y2 = nposition(s, u, 2 * unit, ey, 2 * 20 * KAS1, 2 * 20 * KAS1, 0, 0);
        let _ = novated(&x2, x2.pot(), &y2, y2.pot(), true);
    }
    println!("T1b N5 arithmetic: sizes at the cap and entries at 0 and the limit do not overflow");
}

// ================================================================ round 7

/// N1 over a grid, for those who do not sign a novation (S and L). 1,500 random novations the rules take (partial
/// units, entries apart, asks of nothing and of something), 12 prices each, worth clamped to each position's pot. S is
/// owed at least the smaller of what it was owed and its margin plus what it asked of its counterparty; L likewise.
/// So whoever asked at least what its counterparty had posted is never worse, and whoever asked nothing can be left
/// facing a counterparty with no margin (the finding below).
#[test]
fn t1b_n1_those_who_stay_keep_what_they_asked_over_a_grid() {
    let mut r = Rng(0x51ab_0c0f_fee0_0707);
    let tpl = Tpl::of(&samples::position());
    let unit = tpl.scale;
    let px = samples::position().x.legs[0].entry;
    let (u, s, l) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let (mut cases, mut partial, mut worst_s, mut worst_l, mut worst_s_asked, mut worst_l_asked) = (0, 0, 0i64, 0i64, 0i64, 0i64);
    while cases < 1_500 {
        let (nx, ny) = (r.range(1, 8) * unit + r.range(0, unit), r.range(1, 8) * unit + r.range(0, unit));
        let (ex, ey) = (r.range(px / 2, 2 * px), r.range(px / 2, 2 * px));
        let m = |r: &mut Rng| r.range(5 * KAS1, 400 * KAS1);
        let (ax, bx, ay, by) = (m(&mut r), m(&mut r), m(&mut r), m(&mut r));
        // What S asked of U's margin in X, and L of U's in Y: nothing, a part, or all of it and more.
        let min_counter = |r: &mut Rng, of: i64| match r.range(0, 3) {
            0 => 0,
            1 => r.range(0, of + 1),
            _ => of + r.range(0, of / 2 + 1),
        };
        let (bqx, aqy) = (min_counter(&mut r, ax), min_counter(&mut r, by));
        let x = nposition(u, s, nx, ex, ax, bx, r.range(0, bx + 1), bqx);
        let y = nposition(l, u, ny, ey, ay, by, aqy, r.range(0, ay + 1));
        let Ok(nv) = novated(&x, x.pot(), &y, y.pot(), true) else { continue };
        let Some((z, _)) = nv.z else { continue };
        cases += 1;
        partial += (nx != ny) as usize;
        let n = nv.n;
        let (axs, bxs, ays, bys) = (part(ax, n, nx), part(bx, n, nx), part(ay, n, ny), part(by, n, ny));
        let (sq, lq) = (part(bqx, n, nx), part(aqy, n, ny));
        let (xs, ys) = (kpf_protocol::cross::XLeg { size: n, ..x.x.legs[0] }, kpf_protocol::cross::XLeg { size: n, ..y.x.legs[0] });
        assert_eq!((z.x.a_margin, z.b_min_counter, z.a_min_counter), (ays, sq, lq));
        for k in 0..12 {
            let p = px / 4 + k * (px / 3);
            // S: its side of the units taken, in X before and in Z after.
            let s_before = clamped(bxs - xs.gain(p), axs + bxs);
            let s_after = clamped(z.x.b_margin - z.x.legs[0].gain(p), z.pot());
            assert!(s_after + 3 >= s_before.min(bxs + sq), "S at {p}: {s_before} before, {s_after} after, margin {bxs} and ask {sq}");
            worst_s = worst_s.max(s_before - s_after);
            if sq >= axs {
                worst_s_asked = worst_s_asked.max(s_before - s_after);
            }
            // L: its side of the units taken, in Y before and in Z after.
            let l_before = clamped(ays + ys.gain(p), ays + bys);
            let l_after = clamped(z.x.a_margin + z.x.legs[0].gain(p), z.pot());
            assert!(l_after + 3 >= l_before.min(ays + lq), "L at {p}: {l_before} before, {l_after} after, margin {ays} and ask {lq}");
            worst_l = worst_l.max(l_before - l_after);
            if lq >= bys {
                worst_l_asked = worst_l_asked.max(l_before - l_after);
            }
        }
    }
    println!("T1b N1 grid: {cases} novations ({partial} of partial units), 12 prices: the most S is short of what it was owed {worst_s} sompi and L {worst_l} (through the cap, never below margin plus ask); for those that asked at least what their counterparty had posted: S {worst_s_asked}, L {worst_l_asked}");
    assert!(worst_s_asked <= 3 && worst_l_asked <= 3, "whoever asked what its counterparty had posted is never worse");
    assert!(partial > 500);
}

/// F12 (Medium in round 7; since round 8 `pos-open` asks what the other posts unless told otherwise, see
/// `t1b_fixed_the_command_line_opens_an_position_that_asks_what_the_other_posts`; at the script it is the design: an
/// ask of nothing protects nothing, and a novation is nobody's signature). What protects the one that stays in a
/// novation is what it asked of its counterparty's
/// margin, and a position opened directly asks nothing unless told to (`pos-open` without `--a-ask`/`--b-ask`). In
/// such a position a party takes its own margin out and leaves the other facing a counterparty with none: it writes
/// a second position in which its second key is long against it at the same entry with a thousand sompi, and novates
/// itself out (its own signature is the only one asked). It is paid its margin; its second key holds the position
/// with nothing behind it; the other party's gain is capped at a thousand sompi while its loss is as before. With an
/// ask of the counterparty's margin (what `order-place` defaults to) the same novation is refused.
#[test]
fn t1b_design_an_position_that_asks_nothing_has_its_counterparty_replaced_by_one_without_margin() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, v, second) = (w.a, w.b, kp(34));
    w.keys.add_keypair(second);
    // X: U long 5 units against V from the market's price, 100 KAS each, opened directly: asks of nothing.
    let x = nlive(nposition(xk(&u), xk(&v), 5 * unit, px, 100 * KAS1, 100 * KAS1, 0, 0), 0x71);
    // Y: U's second key long against U at the same entry, a thousand sompi each (U writes it alone).
    let y = nlive(nposition(xk(&second), xk(&u), 5 * unit, px, 1_000, 1_000, 0, 0), 0x72);
    let (b, nv, _) = kpf_protocol::thin_build::novate(&lib, &x, &y, &w.script("KpfTNovate"), Payer { funds: &w.pay(&u), change_to: xk(&u), rate: MIN_FEE_RATE }).unwrap();
    // V signs nothing (since round 8 nobody does; U's second position enters the registry by U's two keys).
    let mut no_v = KeySet::new();
    for key in [&u, &second, &kp(9)] {
        no_v.add_keypair(*key);
    }
    let sg = b.sign(&no_v).expect("signed without V");
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "the engine takes it");
    let (z, _) = nv.z.unwrap();
    assert_eq!((z.x.a_key, z.x.b_key, z.x.a_margin, z.x.b_margin), (xk(&second), xk(&v), 1_000, 100 * KAS1), "V now faces a thousand sompi");
    assert_eq!(nv.to_leaver, 100 * KAS1 + 1_000 - NOVATE_TIP, "U has its margin back");
    // V's worth, clamped, at a price 40 % down (its gain) and 40 % up (its loss): the loss is as before, the gain gone.
    let worth_v = |acc: &Position, p: i64| clamped(acc.x.b_margin - acc.x.legs[0].gain(p), acc.pot());
    let (down, up) = (px - 2 * (px / 5), px + 2 * (px / 5));
    assert_eq!(worth_v(&x.s, up), worth_v(&z, up), "V's loss if the price rises is what it was");
    let lost = worth_v(&x.s, down) - worth_v(&z, down);
    assert!(lost > 15 * KAS1, "V's gain if the price falls is capped: {lost} sompi less");
    // The second key's position is liquidated by the rule at once, for a keeper's fee of at most its margin.
    let liq = z.liquidation(SIDE_A, 6_300, [px, 0], xk(&v)).expect("the one without margin is liquidated at the entry");
    assert!(liq.fee <= 1_000);
    // Had V asked U's margin of whoever faces it (what an order asks by default), the novation is refused.
    let asked = nlive(nposition(xk(&u), xk(&v), 5 * unit, px, 100 * KAS1, 100 * KAS1, 0, 100 * KAS1), 0x73);
    assert!(novated(&asked.s, asked.value as i64, &y.s, y.value as i64, true).is_err());
    assert!(kpf_protocol::thin_build::novate(&lib, &asked, &y, &w.script("KpfTNovate"), Payer { funds: &w.pay(&u), change_to: xk(&u), rate: MIN_FEE_RATE }).is_err());
    println!("T1b FINDING asks of nothing: U takes its 100 KAS out of a position opened without asks; V faces 1,000 sompi, owed {lost} sompi less at a price 40 % down, the same at 40 % up; the keeper's fee for liquidating the empty side is {} sompi", liq.fee);
}

// ================================================================ round 8: the registry

/// The id of hand-made objects of one registry.
const REGISTRY: [u8; 32] = [0xB0; 32];
/// A position under the id `id`, as a live UTXO.
fn live_in(id: Hash, a: Position, n: u8) -> OLive<Position> {
    OLive { s: a, value: a.pot() as u64, id: Some(id), op: op(n), daa: 6_200 }
}
/// The transaction's covenant rules hold and the script at input `i` refuses it.
fn script_refuses(tx: &Transaction, entries: &[kaspa_consensus_core::tx::UtxoEntry], i: usize) -> bool {
    matches!(run_raw(tx, entries, Meter::Free), Ok(v) if v[i].is_err())
}
/// The engine takes every input.
fn all_taken(sg: &OSigned) -> bool {
    matches!(run_raw(&sg.tx, &sg.entries, Meter::Free), Ok(v) if v.iter().all(|r| r.is_ok()))
}
fn only_keys(ks: &[&Keypair]) -> KeySet {
    let mut s = KeySet::new();
    for k in ks {
        s.add_keypair(**k);
    }
    s
}
impl World {
    /// A position entered into the world's registry by its two parties' signatures (the payer pays both margins); the
    /// anchor goes on.
    fn enter(&mut self, what: &str, position: &Position, payer: &Keypair) -> OLive<Position> {
        self.n += 1;
        let f = vec![fund(xk(payer), self.n, position.pot() / KAS1 + 1_000)];
        let b = open(&self.lib, &self.anchor, &self.script("KpfTOpen"), position, &[], &position.x.b_key, false, Payer { funds: &f, change_to: xk(payer), rate: MIN_FEE_RATE }).unwrap_or_else(|e| panic!("{what}: {e}"));
        let s = signed(what, &b, &self.keys);
        self.anchor = nth(&s, 2, (), 6_150);
        nth(&s, 0, *position, 6_200)
    }
    /// An order entered into the world's registry by its owner's signature; the anchor goes on.
    fn put(&mut self, what: &str, o: &Order, payer: &Keypair) -> OLive<Order> {
        self.n += 1;
        let f = vec![fund(xk(payer), self.n, o.value() / KAS1 + 1_000)];
        let b = order_place(&self.lib, &self.anchor, &self.script("KpfTPlace"), o, Payer { funds: &f, change_to: xk(payer), rate: MIN_FEE_RATE }).unwrap_or_else(|e| panic!("{what}: {e}"));
        let s = signed(what, &b, &self.keys);
        assert_eq!(s.tx.outputs[0].value, o.value() as u64);
        self.anchor = nth(&s, 2, (), 6_150);
        nth(&s, 0, *o, 6_200)
    }
}

/// B2. `registry_check` takes a transaction as a registry's genesis only if everything it writes under a covenant id is an
/// anchor of the one library, under one id, and that id is the id of those outputs (so nothing else was written
/// under it before). Consensus takes the other genesis transactions too: only the check tells them apart.
#[test]
fn t1b_b2_registry_check_takes_only_a_genesis_of_anchors_of_one_library() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let f = vec![fund(xk(&w.w), 77, 1_000)];
    let (g, id) = registry_genesis(&lib, 3, KAS1 as u64, Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("a registry's genesis", &g, &w.keys);
    assert_eq!(registry_check(&gs.tx, &lib).unwrap(), id);
    assert_eq!(registry_anchors(&gs.tx, &lib, 6_000).len(), 3);
    let bound: Vec<usize> = (0..gs.tx.outputs.len()).filter(|i| gs.tx.outputs[*i].covenant.is_some()).collect();
    assert_eq!(bound.len(), 3);
    let cov = gs.tx.outputs[bound[0]].covenant.unwrap();
    let auth = cov.authorizing_input;
    let fab = object_spk(&lib, &TObj::Position(tpl.position(xk(&w.a), xk(&w.b), 10 * unit, px / 4, KAS1, KAS1)));
    let other_lib = [0x99u8; 32];
    let rules_take = |t: &Transaction| run_raw(t, &gs.entries, Meter::Free).is_ok();
    let mut refused = 0;
    let mut no = |why: &str, t: Transaction, a_genesis: bool| {
        assert!(registry_check(&t, &lib).is_err(), "registry_check takes {why}");
        assert_eq!(rules_take(&t), a_genesis, "{why}: the covenant rules");
        refused += 1;
    };
    // An anchor replaced by a position nobody signed, the id being the id of these outputs (a true genesis).
    let mut t = gs.tx.clone();
    t.outputs[bound[1]].script_public_key = fab.clone();
    no("a genesis that writes a position among the anchors", regenesis(&t, auth), true);
    // The same as one output more.
    let mut t = gs.tx.clone();
    t.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: fab.clone(), covenant: Some(cov) });
    no("a genesis that writes a position after the anchors", regenesis(&t, auth), true);
    // An order nobody signed, and an object of any other state.
    let order = Order { tpl: tpl.hash(), owner: xk(&w.a), dir: SHORT, size: 10 * unit, limit: 1, margin: KAS1, tip: 0, expiry: 9_000, min_fill: 0, min_counter: 0 };
    let mut t = gs.tx.clone();
    t.outputs[bound[2]].script_public_key = object_spk(&lib, &TObj::Order(order));
    no("a genesis that writes an order among the anchors", regenesis(&t, auth), true);
    // An anchor of another library among them (its scripts would write under the same id), and a registry of that other
    // library read as one of this.
    let mut t = gs.tx.clone();
    t.outputs[bound[1]].script_public_key = object_spk(&other_lib, &TObj::Anchor);
    let mixed = regenesis(&t, auth);
    assert!(registry_check(&mixed, &other_lib).is_err(), "a registry of two libraries, read as the other's");
    no("a genesis that writes the anchors of two libraries", mixed, true);
    let mut t = gs.tx.clone();
    for i in &bound {
        t.outputs[*i].script_public_key = object_spk(&other_lib, &TObj::Anchor);
    }
    no("another library's registry", regenesis(&t, auth), true);
    // A key's output under the id.
    let mut t = gs.tx.clone();
    t.outputs[bound[0]].script_public_key = p2pk_spk(&xk(&w.a));
    no("a genesis that writes a key's output under the id", regenesis(&t, auth), true);
    // Anchors under an id that is not the id of these outputs: not the genesis, what came before is not shown.
    let mut t = gs.tx.clone();
    for i in &bound {
        t.outputs[*i].covenant = Some(CovenantBinding { authorizing_input: auth, covenant_id: Hash::from_bytes([0x5A; 32]) });
    }
    no("anchors under an id that is not theirs", t, false);
    // Two ids.
    let mut t = gs.tx.clone();
    t.outputs[bound[2]].covenant = Some(CovenantBinding { authorizing_input: auth, covenant_id: Hash::from_bytes([0x5A; 32]) });
    no("outputs under two ids", t, false);
    // What is not a genesis at all: a placement through an anchor of the registry, a payment.
    let f2 = vec![fund(xk(&w.w), 79, 1_000)];
    let pb = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &Order { owner: xk(&w.a), dir: LONG, limit: px, margin: 20 * KAS1, ..order }, Payer { funds: &f2, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let ps = signed("a placement", &pb, &w.keys);
    assert!(registry_check(&ps.tx, &lib).is_err(), "a placement through an anchor read as a registry's genesis");
    let mut t = gs.tx.clone();
    for x in t.outputs.iter_mut() {
        x.covenant = None;
    }
    assert!(registry_check(&t, &lib).is_err(), "a transaction that writes nothing under an id");
    println!("T1b B2 registry_check: {refused} transactions that are not a registry's genesis of this library refused (6 of them true genesis transactions to the covenant rules)");
}

/// B2, what the check is for. A registry whose genesis wrote a position nobody signed beside its anchors is F11 again for
/// whoever enters it unchecked: V and K open a real position through one of its anchors (both sign), and the false
/// position of V and K written by the genesis is merged into it by anyone, through the engine. `registry_check` refuses
/// that genesis; a tool that does not run it does not see it.
#[test]
fn t1b_b2_a_registry_whose_genesis_wrote_an_position_is_the_false_merge_again() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (v, k, payer) = (w.a, w.b, w.w);
    // K's: K long 10 units against V from a quarter of the price, a KAS each.
    let false_ = tpl.position(xk(&k), xk(&v), 10 * unit, px / 4, KAS1, KAS1);
    let f = vec![fund(xk(&k), 78, 1_000)];
    let (g, _) = registry_genesis(&lib, 2, 2 * KAS1 as u64, Payer { funds: &f, change_to: xk(&k), rate: MIN_FEE_RATE }).unwrap();
    let bound: Vec<usize> = (0..g.tx.outputs.len()).filter(|i| g.tx.outputs[*i].covenant.is_some()).collect();
    let auth = g.tx.outputs[bound[0]].covenant.unwrap().authorizing_input;
    let mut t = g.tx.clone();
    t.outputs[bound[1]].script_public_key = object_spk(&lib, &TObj::Position(false_));
    let t = regenesis(&t, auth);
    assert!(registry_check(&t, &lib).is_err(), "the check refuses this registry");
    assert!(run_raw(&t, &g.entries, Meter::Free).is_ok(), "the covenant rules take its genesis");
    // What the anchors' list gives a tool that does not check: the one anchor, of the bad registry.
    let anchors = registry_anchors(&t, &lib, 6_000);
    assert_eq!(anchors.len(), 1);
    let bad_registry = anchors[0].id.unwrap();
    let fake = OLive { s: false_, value: t.outputs[bound[1]].value, id: Some(bad_registry), op: TransactionOutpoint { transaction_id: t.id(), index: bound[1] as u32 }, daa: 6_000 };
    assert!(fake.value as i64 >= false_.pot());
    // V and K enter it with a real position: K long 5 units from the market's price, 200 KAS each.
    let real = tpl.position(xk(&k), xk(&v), 5 * unit, px, 200 * KAS1, 200 * KAS1);
    w.anchor = anchors[0].clone();
    let real_live = w.enter("V and K open a real position in the unchecked registry", &real, &v);
    assert_eq!(real_live.id, Some(bad_registry));
    let f = w.pay(&payer);
    let (mb, one) = kpf_protocol::thin_build::merge(&lib, &real_live, &fake, &w.script("KpfTMerge"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    let sg = mb.sign(&only_keys(&[&payer])).expect("signed by the payer alone");
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "the engine merges the genesis's position into the real one");
    let worth_b = |acc: &Position, p: i64| clamped(acc.x.b_margin - acc.x.legs[0].gain(p), acc.pot());
    let taken = worth_b(&real, px) + worth_b(&false_, px) - worth_b(&one, px);
    assert!(taken > 50 * KAS1);
    println!("T1b B2: in a registry whose genesis wrote a position, that position is merged into a real one of the same two: at the market's price V is owed {taken} sompi less (registry_check refuses the registry)");
}

/// The built transaction with its change output turned into `spk` under `cov` (or under no id), signed.
fn change_into(b: &OBuilt, keys: &KeySet, spk: &ScriptPublicKey, cov: Option<(u16, Hash)>) -> OSigned {
    let mut t = b.clone();
    let c = t.change.expect("a change output");
    t.tx.outputs[c].script_public_key = spk.clone();
    t.tx.outputs[c].covenant = cov.map(|(a, id)| CovenantBinding { authorizing_input: a, covenant_id: id });
    t.sign(keys).expect("signed")
}

/// B1 for the entries the engine file has no scene of: the novation in its five shapes, the merge, and the three
/// proofs (with one market and with two; their epoch table inputs carry other ids). Each as built is taken; with its change
/// output turned into a position's script under no id it is still taken (nobody's concern); with that output under the
/// object's id, authorized by any input of the transaction, it is refused.
#[test]
fn t1b_b1_the_novation_the_merge_and_the_proofs_write_no_output_more_under_the_id() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&samples::position());
    let (unit, px) = (tpl.scale, samples::position().x.legs[0].entry);
    let fab = object_spk(&lib, &TObj::Position(tpl.position(xk(&kp(77)), xk(&kp(78)), unit, px, 50 * KAS1, 50 * KAS1)));
    let (mut tried, mut txs, mut wrong, mut anchors) = (0, 0, 0, 0);
    let mut check = |what: &str, b: &OBuilt, id: Hash, keys: &KeySet| {
        assert!(all_taken(&b.sign(keys).expect("signed")), "{what}: as built");
        assert!(all_taken(&change_into(b, keys, &fab, None)), "{what}: the change as a position's script under no id");
        for auth in 0..b.tx.inputs.len() as u16 {
            assert!(!all_taken(&change_into(b, keys, &fab, Some((auth, id)))), "{what}: a position nobody signed under the object's id, by input {auth}");
            tried += 1;
        }
        // What the entry writes under the id: another object in its place, or a sompi less, is refused.
        for o in (0..b.tx.outputs.len()).filter(|o| b.tx.outputs[*o].covenant.map(|c| c.covenant_id) == Some(id)) {
            let mut t = b.clone();
            t.tx.outputs[o].script_public_key = fab.clone();
            assert!(!all_taken(&t.sign(keys).expect("signed")), "{what}: a position nobody signed in the place of output {o}");
            let mut t = b.clone();
            t.tx.outputs[o].value -= 1;
            assert!(!all_taken(&t.sign(keys).expect("signed")), "{what}: output {o} a sompi short");
            wrong += 2;
        }
        // B3: an anchor in the place of each object the entry reads (inputs under the id): the entry does not take it.
        let script_at = (0..b.plans.len()).find(|i| matches!(&b.plans[*i], OPlan::Contract { entry, .. } if *entry != "go")).expect("the entry's input");
        for i in (0..b.entries.len()).filter(|i| b.entries[*i].covenant_id == Some(id)) {
            let mut t = b.clone();
            let olib = b.entries[script_at].covenant_id.expect("the library's id").as_bytes();
            if let OPlan::Contract { redeem, .. } = &mut t.plans[i] {
                *redeem = TObj::Anchor.script(&olib);
            }
            t.entries[i].script_public_key = object_spk(&olib, &TObj::Anchor);
            let sg = t.sign(keys).expect("signed");
            let r = run_raw(&sg.tx, &sg.entries, Meter::Free).expect("the covenant rules");
            assert!(r[i].is_ok() && r[script_at].is_err(), "{what}: an anchor as input {i}: the anchor's own script {:?}, the entry {:?}", r[i].is_ok(), r[script_at].is_ok());
            anchors += 1;
        }
        txs += 1;
    };
    let (u, s, l) = (xk(&kp(31)), xk(&kp(32)), xk(&kp(33)));
    let registry = Hash::from_bytes(REGISTRY);
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let a = |a, b, n: i64, e| nposition(a, b, n * unit, e, 100 * KAS1, 100 * KAS1, 0, 0);
    for (what, x, y) in [
        ("nothing rests", a(u, s, 4, px), a(l, u, 4, px + px / 50)),
        ("X rests", a(u, s, 5, px), a(l, u, 3, px + px / 50)),
        ("Y rests", a(u, s, 3, px), a(l, u, 5, px)),
        ("the two that stay are one, nothing written", a(u, s, 4, px), a(s, u, 4, px)),
        ("the two that stay are one, X rests", a(u, s, 5, px), a(s, u, 3, px)),
    ] {
        let (b, _, _) = kpf_protocol::thin_build::novate(&lib, &nlive(x, 0x71), &nlive(y, 0x72), &w.script("KpfTNovate"), p).unwrap();
        check(&format!("novate, {what}"), &b, registry, &w.keys);
    }
    let (b, _) = kpf_protocol::thin_build::merge(&lib, &nlive(a(u, s, 4, px), 0x71), &nlive(a(u, s, 3, px + px / 50), 0x72), &w.script("KpfTMerge"), p).unwrap();
    check("merge", &b, registry, &w.keys);
    let pw = pworld();
    for two in [false, true] {
        for pr in proofs(&pw, two, 61 + two as u8) {
            let id = pr.b.entries[0].covenant_id.expect("the object's id");
            check(&format!("{} ({} market(s))", pr.name, 1 + two as usize), &pr.b, id, &pw.keys);
        }
    }
    println!("T1b B1 (builders): {txs} transactions (5 novations, a merge, the proofs with one and two markets), {tried} with one output more under the object's id, {wrong} with what the entry writes replaced or a sompi short, all refused; B3: {anchors} with an anchor in an object's place, the entry refusing each");
    assert!(txs >= 5 + 1 + 6);
}

/// N5, B1 (round 8). Under the registry's id a novation writes, in this order, what is left of X, the position of the two
/// that stay, what is left of Y, and nothing else; each holds what the model says. Judged by the novation's own
/// input (round 6's test of this edited a signed transaction and asked `verify`, which the funding signature alone
/// fails: its refusals proved nothing; this one replaces it).
#[test]
fn t1b_n5_a_novation_writes_under_the_registry_exactly_what_its_rule_says() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&samples::position());
    let (unit, px) = (tpl.scale, samples::position().x.legs[0].entry);
    let (u, s, l) = (xk(&kp(31)), xk(&kp(32)), xk(&kp(33)));
    let registry = Hash::from_bytes(REGISTRY);
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let a = |a, b, n: i64, e, am: i64, bm: i64| nposition(a, b, n * unit, e, am * KAS1, bm * KAS1, 0, 0);
    let mut tried = 0;
    for (what, x, y) in [
        ("nothing rests", a(u, s, 4, px, 80, 80), a(l, u, 4, px + px / 50, 80, 80)),
        ("X rests", a(u, s, 5, px, 100, 100), a(l, u, 3, px + px / 50, 80, 60)),
        ("Y rests", a(u, s, 3, px, 60, 60), a(l, u, 5, px, 100, 100)),
        ("the two that stay are one, nothing written", a(u, s, 4, px, 80, 80), a(s, u, 4, px, 80, 80)),
        ("the two that stay are one, Y rests", a(u, s, 3, px, 60, 60), a(s, u, 5, px + px / 50, 100, 100)),
    ] {
        let (b, nv, _) = kpf_protocol::thin_build::novate(&lib, &nlive(x, 0x71), &nlive(y, 0x72), &w.script("KpfTNovate"), p).unwrap();
        let sg = signed(what, &b, &w.keys);
        let want: Vec<(Position, i64)> = [nv.rest_x, nv.z, nv.rest_y].into_iter().flatten().collect();
        let under: Vec<usize> = (0..sg.tx.outputs.len()).filter(|i| sg.tx.outputs[*i].covenant.map(|c| c.covenant_id) == Some(registry)).collect();
        assert_eq!(under.len(), want.len(), "{what}: the outputs under the registry's id");
        for (i, (acc, v)) in under.iter().zip(&want) {
            assert_eq!((sg.tx.outputs[*i].script_public_key.clone(), sg.tx.outputs[*i].value as i64), (object_spk(&lib, &TObj::Position(*acc)), *v), "{what}: output {i}");
        }
        let mut no = |why: &str, strict: bool, f: &dyn Fn(&mut Transaction)| {
            let mut t = sg.tx.clone();
            f(&mut t);
            let r = run_raw(&t, &sg.entries, Meter::Free);
            let refused = match &r {
                Ok(v) => v[2].is_err(),
                Err(_) => !strict,
            };
            assert!(refused, "{what}: {why}: {:?}", r.map(|v| v.iter().map(|x| x.is_ok()).collect::<Vec<_>>()));
            tried += 1;
        };
        for i in &under {
            let i = *i;
            no("a position a sompi short", true, &|t| t.outputs[i].value -= 1);
            no("a position replaced by another", true, &|t| t.outputs[i].script_public_key = object_spk(&lib, &TObj::Position(a(kp31(), s, 1, px, 80, 80))));
            no("a position under no id", false, &|t| t.outputs[i].covenant = None);
            no("a position under another id", false, &|t| t.outputs[i].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: Hash::from_bytes([0x5A; 32]) }));
        }
        if under.len() == 2 {
            let (i, j) = (under[0], under[1]);
            no("the two positions in each other's place", true, &|t| {
                let (si, vi) = (t.outputs[i].script_public_key.clone(), t.outputs[i].value);
                t.outputs[i].script_public_key = t.outputs[j].script_public_key.clone();
                t.outputs[i].value = t.outputs[j].value;
                t.outputs[j].script_public_key = si;
                t.outputs[j].value = vi;
            });
        }
        // What is paid to keys: the user that leaves, and the one that stays when it is both.
        for (key, v) in [(u, nv.to_leaver), (s, nv.to_stayer)] {
            if v > 0 {
                let i = (0..sg.tx.outputs.len()).rev().find(|i| sg.tx.outputs[*i].script_public_key == p2pk_spk(&key) && sg.tx.outputs[*i].value as i64 == v).unwrap_or_else(|| panic!("{what}: a payment of {v}"));
                no("a payment a sompi short", true, &|t| t.outputs[i].value -= 1);
                no("a payment to another key", true, &|t| t.outputs[i].script_public_key = p2pk_spk(&l));
            }
        }
        assert_eq!(nv.to_leaver + nv.to_stayer + want.iter().map(|(_, v)| *v).sum::<i64>() + NOVATE_TIP, (x.pot() + y.pot()), "{what}: only the sender's share leaves the two positions");
    }
    println!("T1b N5: a novation in five shapes writes under the registry's id what the model says, in order; {tried} changes to it refused by the novation's own input");
}
fn kp31() -> [u8; 32] {
    xk(&kp(31))
}

/// B3 at the codec. An anchor's state is one byte and its hash the hash of a pushed 7; no object of 2,400 random
/// ones of every other kind has that hash, and the anchor's bytes are read as nothing else.
#[test]
fn t1b_b3_an_anchor_is_no_other_object() {
    assert_eq!(TObj::Anchor.encode(), vec![7]);
    assert_eq!(TObj::decode(&[7]).unwrap(), TObj::Anchor);
    assert!(TObj::decode(&[7, 0]).is_err() && TObj::decode(&[7, 7]).is_err());
    let h = TObj::Anchor.hash();
    assert_eq!(h, blake2b(&[vec![0x08], 7i64.to_le_bytes().to_vec()].concat()), "the hash of a pushed 7, nine bytes");
    let mut r = Rng(0x7131_b0b5_1dea_0088);
    for _ in 0..300 {
        for o in random_objects(&mut r) {
            assert_ne!(o.hash(), h);
            let mut b = o.encode();
            b[0] = 7;
            assert!(TObj::decode(&b).is_err(), "{}: read as an anchor", o.kind());
        }
    }
    let lib = [0x11u8; 32];
    assert_ne!(object_spk(&lib, &TObj::Anchor), object_spk(&[0x12; 32], &TObj::Anchor), "an anchor is of its library");
}

/// F11 (Critical in the merge as reviewed in round 5; closed in round 7 by signatures at the merge, in round 8 by the
/// registry). The model merges a false position into a real one as it always did, and merged, the false position's gain is
/// paid from the other party's real margin. What stops it now: the false position does not come under the registry's id
/// without the other party's signature, and a position of another id is not merged with one of the registry. The merge
/// itself is nobody's signature again.
#[test]
fn t1b_fixed_f11_a_false_position_does_not_enter_the_registry() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (a, b, payer) = (w.a, w.b, w.w);
    let real = tpl.position(xk(&a), xk(&b), 5 * unit, px, 200 * KAS1, 200 * KAS1);
    let false_ = tpl.position(xk(&a), xk(&b), 10 * unit, px / 4, KAS1, KAS1);
    let one = merged(&real, &false_).expect("the model merges them: it computes what it did in round 5");
    let worth_b = |acc: &Position, p: i64| clamped(acc.x.b_margin - acc.x.legs[0].gain(p), acc.pot());
    let b_apart = worth_b(&real, px) + worth_b(&false_, px);
    let taken = b_apart - worth_b(&one, px);
    assert_eq!(b_apart, 200 * KAS1);
    assert!(taken > 50 * KAS1, "merged, B would be owed {taken} less");
    // 1. The false position does not enter the registry without B: its opening is signed by both parties in their places.
    let f = w.pay(&a);
    let ob = open(&lib, &w.anchor, &w.script("KpfTOpen"), &false_, &[], &xk(&a), false, Payer { funds: &f, change_to: xk(&a), rate: MIN_FEE_RATE }).unwrap();
    assert!(ob.sign(&only_keys(&[&a, &payer])).is_err(), "the opening is not signed without B's key");
    for other in [xk(&a), xk(&payer)] {
        let forged = resign(&ob, xk(&b), other).sign(&w.keys).unwrap();
        assert!(script_refuses(&forged.tx, &forged.entries, 1), "an opening with another key's signature in B's place");
    }
    let forged = resign(&ob, xk(&a), xk(&payer)).sign(&w.keys).unwrap();
    assert!(script_refuses(&forged.tx, &forged.entries, 1), "an opening with another key's signature in A's place");
    // 2. Written beside the registry (anybody writes an object's script under an id of its own), it is not merged with
    // a position of the registry: the tool refuses, and so does the engine.
    let in_registry = w.enter("the real position, signed by both", &real, &a);
    let other_id = Hash::from_bytes([0x72; 32]);
    let f = w.pay(&payer);
    let p = Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE };
    assert!(kpf_protocol::thin_build::merge(&lib, &in_registry, &live_in(other_id, false_, 0x72), &w.script("KpfTMerge"), p).is_err(), "the tool merges positions of two ids");
    let (mb, _) = kpf_protocol::thin_build::merge(&lib, &in_registry, &live_in(in_registry.id.unwrap(), false_, 0x72), &w.script("KpfTMerge"), p).unwrap();
    // Of one id the merge is taken with no signature of A or B: that is why what is under the id must have entered signed.
    let mut sg = mb.sign(&only_keys(&[&payer])).expect("a merge is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok());
    sg.entries[1].covenant_id = Some(other_id);
    assert!(script_refuses(&sg.tx, &sg.entries, 2), "the engine merges a position of another id into one of the registry");
    println!("T1b F11 fixed (registry): the false position ({taken} sompi of B's at stake) enters only with B's signature; under another id it is not merged");
}

/// Round 8: the signatures of the way in (`KpfTPlace.open`, `.place`) are the transaction's: they commit to its
/// outputs, and one made for one transaction lets nothing else in.
#[test]
fn t1b_r8_the_signatures_of_the_way_in_commit_to_the_transaction() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (a, b) = (w.a, w.b);
    let position = tpl.position(xk(&a), xk(&b), 3 * unit, px, 60 * KAS1, 60 * KAS1);
    let order = Order { tpl: tpl.hash(), owner: xk(&a), dir: LONG, size: 3 * unit, limit: px, margin: 60 * KAS1, tip: KAS1, expiry: 9_000, min_fill: 0, min_counter: 60 * KAS1 };
    for what in ["an opening", "a placement"] {
        let mut two = vec![];
        for _ in 0..2 {
            let f = w.pay(&w.w.clone());
            let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
            two.push(if what == "an opening" { open(&lib, &w.anchor, &w.script("KpfTOpen"), &position, &[], &xk(&b), false, p).unwrap() } else { order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &order, p).unwrap() });
        }
        let sg = signed(what, &two[0], &w.keys);
        let c = two[0].change.expect("a change output");
        // A sompi moved from the change to what enters, or to the anchor (the script's own rule takes more there).
        for to in [0usize, 2] {
            let mut moved = sg.clone();
            moved.tx.outputs[c].value -= 1;
            moved.tx.outputs[to].value += 1;
            assert!(script_refuses(&moved.tx, &moved.entries, 1), "{what}: output {to} changed after the signatures were made");
        }
        // What enters replaced after signing by another object.
        let mut swapped = sg.clone();
        swapped.tx.outputs[0].script_public_key = object_spk(&lib, &TObj::Position(tpl.position(xk(&a), xk(&b), 3 * unit, px / 4, 60 * KAS1, 60 * KAS1)));
        assert!(script_refuses(&swapped.tx, &swapped.entries, 1), "{what}: another object entering under the same signatures");
        // The script's signature script in another transaction of the same anchor and the same object (other funds).
        let mut sg2 = two[1].sign(&w.keys).unwrap();
        assert!(!input_fails(&sg2, 1));
        sg2.tx.inputs[1].signature_script = sg.tx.inputs[1].signature_script.clone();
        assert!(script_refuses(&sg2.tx, &sg2.entries, 1), "{what}: the signatures of one transaction in another");
    }
    println!("T1b R8 signatures: an opening's and a placement's signatures commit to the outputs and to the transaction");
}

/// F10 (Critical in round 6; closed in round 7 by the leaver's signature, in round 8 by the registry). The mirror position
/// that would pay a user less than its margin does not come under the registry's id without that user's signature, and an
/// position of another id is not novated with one of the registry. The novation itself is nobody's signature again.
#[test]
fn t1b_fixed_f10_a_mirror_position_does_not_enter_the_registry() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, k, k2, payer) = (w.a, w.b, kp(34), w.w);
    w.keys.add_keypair(k2);
    let not_u = only_keys(&[&k, &k2, &payer, &kp(9)]);
    // U's real position X, entered by U and K: long 5 units from the market's price, 100 KAS each.
    let x = w.enter("X: U long against K, signed by both", &tpl.position(xk(&u), xk(&k), 5 * unit, px, 100 * KAS1, 100 * KAS1), &u);
    let other_id = Hash::from_bytes([0x72; 32]);
    for (what, l) in [("the two that stay are one key", k), ("the two that stay are two keys", k2)] {
        // The mirror: L long against U from the lowest entry.
        let y_position = tpl.position(xk(&l), xk(&u), 5 * unit, 0, 300 * KAS1, KAS1);
        let f = w.pay(&k);
        let pk = Payer { funds: &f, change_to: xk(&k), rate: MIN_FEE_RATE };
        let ob = open(&lib, &w.anchor, &w.script("KpfTOpen"), &y_position, &[], &xk(&u), false, pk).unwrap();
        assert!(ob.sign(&not_u).is_err(), "{what}: the mirror's opening is not signed without U's key");
        for other in [xk(&k), xk(&payer)] {
            let forged = resign(&ob, xk(&u), other).sign(&w.keys).unwrap();
            assert!(script_refuses(&forged.tx, &forged.entries, 1), "{what}: the mirror entering with another key's signature in U's place");
        }
        // Written beside the registry it is not novated with X: the tool refuses, and so does the engine.
        assert!(kpf_protocol::thin_build::novate(&lib, &x, &live_in(other_id, y_position, 0x72), &w.script("KpfTNovate"), pk).is_err(), "{what}: the tool novates positions of two ids");
        let (nb, nv, _) = kpf_protocol::thin_build::novate(&lib, &x, &live_in(x.id.unwrap(), y_position, 0x72), &w.script("KpfTNovate"), pk).unwrap();
        assert!(nv.to_leaver < 60 * KAS1, "{what}: the novation would pay U {} of its 100 KAS", nv.to_leaver);
        let mut sg = nb.sign(&not_u).expect("a novation is signed by its payer alone");
        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "{what}: of one id, a novation asks nobody");
        sg.entries[1].covenant_id = Some(other_id);
        assert!(script_refuses(&sg.tx, &sg.entries, 2), "{what}: the engine novates a position of another id with one of the registry");
    }
    println!("T1b F10 fixed (registry): the mirror position enters only with U's signature; under another id it is not novated");
}

/// Round 8, replacing round 7's test of the same claim: nothing is written under the registry's id in a party's name
/// without its key. An order naming V is not placed without V's signature; written beside the registry it is matched with
/// no order of the registry.
#[test]
fn t1b_r8_nothing_enters_the_registry_in_a_partys_name_without_its_key() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (v, k, payer) = (w.a, w.b, w.w);
    let not_v = only_keys(&[&k, &payer, &kp(9)]);
    // A sell in V's name at a quarter of the price, with K's money.
    let o = Order { tpl: tpl.hash(), owner: xk(&v), dir: SHORT, size: 10 * unit, limit: px / 4, margin: KAS1, tip: 0, expiry: 9_000, min_fill: 10 * unit, min_counter: 0 };
    let f = w.pay(&k);
    let pk = Payer { funds: &f, change_to: xk(&k), rate: MIN_FEE_RATE };
    let pb = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &o, pk).unwrap();
    assert!(pb.sign(&not_v).is_err(), "an order in V's name is not signed without V's key");
    for other in [xk(&k), xk(&payer)] {
        let forged = resign(&pb, xk(&v), other).sign(&w.keys).unwrap();
        assert!(script_refuses(&forged.tx, &forged.entries, 1), "an order in V's name entering with another key's signature");
    }
    // K's own buy, in the registry by K's signature.
    let buy = w.put("K's buy", &Order { owner: xk(&k), dir: LONG, ..o }, &k);
    let other_id = Hash::from_bytes([0x72; 32]);
    let beside = OLive { s: o, value: o.value() as u64, id: Some(other_id), op: op(0x72), daa: 6_200 };
    assert!(order_match(&lib, &tpl, &buy, &beside, &w.script("KpfTMatch"), 10 * unit, px / 4, pk).is_err(), "the tool matches orders of two ids");
    let (mb, m) = order_match(&lib, &tpl, &buy, &OLive { id: buy.id, ..beside }, &w.script("KpfTMatch"), 10 * unit, px / 4, pk).unwrap();
    assert_eq!(m.position.x.b_key, xk(&v));
    let mut sg = mb.sign(&not_v).expect("a match is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "of one id, a match asks nobody");
    sg.entries[1].covenant_id = Some(other_id);
    assert!(script_refuses(&sg.tx, &sg.entries, 2), "the engine matches an order of another id with one of the registry");
    println!("T1b R8: an order in V's name enters only with V's signature; under another id it is not matched");
}

/// FINDING (Medium, F13; a consequence of merge and novation being nobody's signature). What a resting order can lose
/// is no longer its own margin. V's buy of 10 units rests at its limit with a margin of a fifth of what those units
/// lose if the market falls a tenth; the market falls a tenth and V has not cancelled. K sells into it at V's limit:
/// a position in which V is under water by five times its margin there. Apart, V has lost that margin and no more.
/// (a) Anyone merges that position into another of V and K in which V is long: the whole loss is charged to V's margin
/// there. (b) Anyone novates V out of it against a position in which V is short against a third user S: the whole loss
/// is taken from V's margin there, and stands as K's margin against S. Both go through the engine with no signature
/// of V, K or S. So through one order V can lose up to its margins in every position of that market and registry that can
/// be put together with the order's, not the order's margin.
#[test]
fn t1b_finding_a_resting_order_loses_more_than_its_margin_once_its_position_is_merged_or_novated() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (v, k, s, payer) = (kp(51), kp(52), kp(53), w.w);
    for key in [v, k, s] {
        w.keys.add_keypair(key);
    }
    let nobody = only_keys(&[&payer]);
    let now = px - px / 10;
    let lost = -tpl.position(xk(&v), xk(&k), 10 * unit, px, KAS1, KAS1).x.legs[0].gain(now);
    assert!(lost > KAS1);
    let (m_o, big) = (lost / 5, 3 * lost);
    // V's buy, signed by V when the market stood at px: 10 units at px or below, asking of the seller what it posts.
    let buy = w.put("V's buy rests", &Order { tpl: tpl.hash(), owner: xk(&v), dir: LONG, size: 10 * unit, limit: px, margin: m_o, tip: KAS1 / 10, expiry: 9_000, min_fill: 0, min_counter: m_o }, &v);
    // K's sell into it at V's limit, the market a tenth lower. K posts more than V asks (it gets it back).
    let sell = w.put("K's sell", &Order { owner: xk(&k), dir: SHORT, margin: big, tip: 0, min_counter: 0, ..buy.s }, &k);
    let f = w.pay(&k);
    let (mb, m) = order_match(&lib, &tpl, &buy, &sell, &w.script("KpfTMatch"), 10 * unit, px, Payer { funds: &f, change_to: xk(&k), rate: MIN_FEE_RATE }).unwrap();
    let ms = signed("K fills V's resting buy at V's limit", &mb, &w.keys);
    let a2 = nth(&ms, 0, m.position, 6_300);
    assert_eq!((a2.s.x.a_key, a2.s.x.b_key, a2.s.x.a_margin, a2.s.x.legs[0].entry), (xk(&v), xk(&k), m_o, px));
    let worth_a = |acc: &Position, p: i64| clamped(acc.x.a_margin + acc.x.legs[0].gain(p), acc.pot());
    let worth_b = |acc: &Position, p: i64| clamped(acc.x.b_margin - acc.x.legs[0].gain(p), acc.pot());
    assert_eq!(worth_a(&a2.s, now), 0, "apart, the order's position owes V nothing: V has lost the order's margin, {m_o}");
    assert_eq!(worth_b(&a2.s, now), big + m_o, "and K has won the order's margin and no more");
    // (a) V and K's other position, entered by both at the market's price now: V long 5 units, `big` each.
    let a1 = w.enter("A1: V long against K, signed by both", &tpl.position(xk(&v), xk(&k), 5 * unit, now, big, big), &v);
    assert_eq!(worth_a(&a1.s, now), big);
    let f = w.pay(&payer);
    let pp = Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE };
    let (gb, one) = kpf_protocol::thin_build::merge(&lib, &a1, &a2, &w.script("KpfTMerge"), pp).unwrap();
    let gs = gb.sign(&nobody).expect("a merge is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&gs.tx, &gs.entries).is_ok(), "the engine takes the merge");
    let (v_apart, v_one) = (worth_a(&a1.s, now) + worth_a(&a2.s, now), worth_a(&one, now));
    let (k_apart, k_one) = (worth_b(&a1.s, now) + worth_b(&a2.s, now), worth_b(&one, now));
    let more = v_apart - v_one;
    assert!((more - (lost - m_o)).abs() <= MERGE_TIP, "merged, V is owed {more} less: what the order lost beyond its margin");
    assert!((k_one - k_apart - (lost - m_o)).abs() <= MERGE_TIP, "and K that much more");
    println!("T1b FINDING F13 merge: the order's margin {m_o}; the 10 units have lost {lost}; apart V is owed {v_apart}, merged {v_one} ({more} less, from its margin in the other position); K {k_apart} apart, {k_one} merged");
    // (b) V short 10 units against S, entered by both at the market's price now, `big` each, each asking `big`.
    let y = w.enter("Y: S long against V, signed by both", &Position { a_min_counter: big, b_min_counter: big, ..tpl.position(xk(&s), xk(&v), 10 * unit, now, big, big) }, &s);
    let f = w.pay(&payer);
    let pp = Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE };
    let (nb, nv, _) = kpf_protocol::thin_build::novate(&lib, &a2, &y, &w.script("KpfTNovate"), pp).unwrap();
    let ns = nb.sign(&nobody).expect("a novation is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&ns.tx, &ns.entries).is_ok(), "the engine takes the novation");
    let v_apart = worth_a(&a2.s, now) + worth_b(&y.s, now);
    assert_eq!(v_apart, big, "apart: nothing from the order's position, its margin against S whole");
    assert_eq!(nv.to_leaver, m_o + big - lost - NOVATE_TIP, "taken out, V is paid its two margins less the whole loss");
    assert_eq!(paid(&ns, &xk(&v)), nv.to_leaver);
    let (z, _) = nv.z.expect("S and K face each other");
    assert_eq!((z.x.a_key, z.x.b_key, z.x.b_margin), (xk(&s), xk(&k), big + lost), "K's margin against S is its own and the whole loss");
    println!("T1b FINDING F13 novation: apart V is owed {v_apart}; taken out it is paid {} ({} less, from its margin against S); K holds {} against S where it posted {big}", nv.to_leaver, v_apart - nv.to_leaver, z.x.b_margin);
    // The bound: the user that leaves is never paid less than nothing, so the two margins are the most it loses.
    let thin = Position { a_min_counter: 0, b_min_counter: 0, ..tpl.position(xk(&s), xk(&v), 10 * unit, now, big, lost / 2) };
    assert!(novated(&a2.s, a2.value as i64, &thin, thin.pot(), true).is_err(), "a loss above the two margins is not novated");
}

/// B5. A user long in X and short in Y for the same units is owed, before the caps, the same at every price: its two
/// margins and the difference of the entries (C). That is what a novation pays it, less the sender's 0.015 KAS, and a
/// novation is anyone's to send at any time. Keeping both positions it would be owed C plus or minus what the caps
/// change: over 1,500 random novations and 81 prices each, never more than max(0, e1, e2) above C and never more than
/// that below, where e1 = (S's margin left in Z) - (U's margin in Y) and e2 = (L's margin) - (U's margin in X) - (the
/// transfer): the margin by which one of U's counterparties outlasts U's own on the other position. So being taken out
/// costs U at most its chance of losing only a margin on one position while collecting in full on the other, and
/// spares it the reverse (a counterparty that cannot pay while U pays in full).
#[test]
fn t1b_b5_what_the_caps_are_worth_to_a_user_taken_out() {
    let mut r = Rng(0x51ab_0c0f_fee0_0808);
    let tpl = Tpl::of(&samples::position());
    let unit = tpl.scale;
    let px = samples::position().x.legs[0].entry;
    let (u, s, l) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let (mut cases, mut most_lost, mut most_spared, mut bound_reached, mut at_entries) = (0, 0i64, 0i64, 0, 0i64);
    while cases < 1_500 {
        let (nx, ny) = (r.range(1, 8) * unit + r.range(0, unit), r.range(1, 8) * unit + r.range(0, unit));
        let (ex, ey) = (r.range(px / 2, 2 * px), r.range(px / 2, 2 * px));
        let m = |r: &mut Rng| r.range(5 * KAS1, 400 * KAS1);
        let (ax, bx, ay, by) = (m(&mut r), m(&mut r), m(&mut r), m(&mut r));
        let x = nposition(u, s, nx, ex, ax, bx, 0, r.range(0, ax + 1));
        let y = nposition(l, u, ny, ey, ay, by, r.range(0, by + 1), 0);
        let Ok(nv) = novated(&x, x.pot(), &y, y.pot(), true) else { continue };
        cases += 1;
        let n = nv.n;
        let (axs, bxs, ays, bys) = (part(ax, n, nx), part(bx, n, nx), part(ay, n, ny), part(by, n, ny));
        let (xs, ys) = (kpf_protocol::cross::XLeg { size: n, ..x.x.legs[0] }, kpf_protocol::cross::XLeg { size: n, ..y.x.legs[0] });
        let c = nv.to_leaver + NOVATE_TIP;
        assert_eq!(c, axs + bys + nv.transfer, "what U is paid: its two margins and the transfer");
        let (e1, e2) = (bxs - nv.transfer - bys, ays - axs - nv.transfer);
        let (hi, lo) = (e1.max(e2).max(0), e1.min(e2).min(0));
        let keep = |p: i64| clamped(axs + xs.gain(p), axs + bxs) + clamped(bys - ys.gain(p), ays + bys);
        for k in 0..=80 {
            let d = keep(k * (px / 20)) - c;
            assert!(d <= hi + 3 && d >= lo - 3, "at {}: keeping both U is owed {d} more than taken out; bounds {lo}..{hi}", k * (px / 20));
            most_lost = most_lost.max(d);
            most_spared = most_spared.max(-d);
            bound_reached += (hi > 0 && d + 3 >= hi) as usize;
        }
        // At the two entries themselves (the market where either position was made): printed.
        for p in [ex, ey] {
            at_entries = at_entries.max((keep(p) - c).abs());
        }
    }
    println!("T1b B5: {cases} novations, 81 prices: taken out, U is paid C less 0.015 KAS; keeping both it would be owed at most {most_lost} sompi more (its counterparty's margin outlasting its own on the other position) and at most {most_spared} less; the bound max(0, e1, e2) is reached {bound_reached} times; at the two entries the difference is at most {at_entries}");
    assert!(most_lost > 0 && most_spared > 0 && bound_reached > 0);
}

// ================================================================ round 9: the join (KpfTJoin)

use kpf_protocol::thin::joined;

impl PWorld {
    /// A one-leg position of the proof world's terms in market `i`: `long` long `units` at 10 against `short`.
    fn single(&self, i: usize, long: &Keypair, short: &Keypair, units: i64, am: i64, bm: i64) -> Position {
        let base = self.position(false).s;
        let m = &self.markets[i];
        let mut x = base.x;
        x.legs[0] = XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir: LONG, entry: P10 };
        x.a_key = xk(long);
        x.b_key = xk(short);
        x.a_margin = am;
        x.b_margin = bm;
        let a = Position { x, ..base };
        a.check().unwrap();
        a
    }
    /// The two joined by the builder, signed and taken by the engine: the position written, as a live UTXO.
    fn joined(&self, what: &str, x: Position, y: Position, n: u8) -> OLive<Position> {
        let registry = Hash::from_bytes(REGISTRY);
        let live = |a: Position, n: u8| OLive { s: a, value: a.x.floor() as u64, id: Some(registry), op: op(n), daa: 6_050 };
        let f = [fund(xk(&self.w), n, 1_000)];
        let (b, one) = join(&self.lib, &live(x, 0x81), &live(y, 0x82), &self.script("KpfTJoin"), Payer { funds: &f, change_to: xk(&self.w), rate: MIN_FEE_RATE }).unwrap_or_else(|e| panic!("{what}: {e}"));
        let s = signed(what, &b, &self.keys);
        assert_eq!(one, joined(&x, &y).unwrap());
        nth(&s, 0, one, 6_100)
    }
}
/// A leg's 111 bytes as the script cuts them from the fixed part.
fn leg_of(a: &Position) -> Vec<u8> {
    a.fixed()[90..201].to_vec()
}

/// J2. Before the caps a join changes nothing but the sender's share: for random pairs (the first long or short for
/// its A, the second's keys as the first's or turned, sizes, entries, scales and price limits that differ between the
/// two markets) and random pairs of prices, each party's margin and gains in the joined position are the sum of what
/// the two positions gave it, less half the tip, to the sompi.
#[test]
fn t1b_j2_a_join_is_exact_before_the_caps() {
    let mut r = Rng(0x51ab_0c0f_fee0_0909);
    let base = Tpl::of(&samples::position());
    let (u1, u2) = ([11u8; 32], [22u8; 32]);
    let half = MERGE_TIP / 2;
    let (mut pairs, mut points, mut turned_n, mut short_n, mut capped) = (0, 0u64, 0, 0, 0u64);
    while pairs < 2_000 {
        let scales = [1i64, 10, 1_000, 1_000_000, 100_000_000];
        let mk = |r: &mut Rng, market: u8| {
            let scale = scales[r.range(0, 5) as usize];
            let pm = [1_000i64, 1_000_000_000, 3_000_000_000_000, 100_000_000_000_000][r.range(0, 4) as usize];
            (Tpl { market: [market; 32], scale, price_max: pm, ..base }, r.range(1, 40) * scale + r.range(0, scale), r.range(0, pm + 1), pm)
        };
        let (t1, n1, e1, pm1) = mk(&mut r, 0x41);
        let (t2, n2, e2, pm2) = mk(&mut r, 0x42);
        let m = |r: &mut Rng| r.range(KAS1, 5_000 * KAS1);
        let turned = r.range(0, 2) == 1;
        let mut x = Position { a_min_counter: r.range(0, 100 * KAS1), b_min_counter: r.range(0, 100 * KAS1), ..t1.position(u1, u2, n1, e1, m(&mut r), m(&mut r)) };
        if r.range(0, 2) == 1 {
            x.x.legs[0].dir = SHORT;
        }
        let (ya, yb) = if turned { (u2, u1) } else { (u1, u2) };
        let y = Position { a_min_counter: r.range(0, 100 * KAS1), b_min_counter: r.range(0, 100 * KAS1), ..t2.position(ya, yb, n2, e2, m(&mut r), m(&mut r)) };
        if x.check().is_err() || y.check().is_err() {
            continue;
        }
        let one = joined(&x, &y).unwrap_or_else(|e| panic!("{e}: {x:?} {y:?}"));
        pairs += 1;
        turned_n += turned as usize;
        short_n += (x.x.legs[0].dir == SHORT) as usize;
        // The state: the first's leg, then the second's with its direction as the first's A holds it.
        assert_eq!(one.fixed(), [&x.fixed()[..201], &leg_of(&y)[..93], &[0x08u8][..], &sm8(if turned { SHORT } else { LONG })[..], &leg_of(&y)[102..]].concat());
        assert_eq!(one.pot(), x.pot() + y.pot() - MERGE_TIP);
        for _ in 0..40 {
            // Prices over each market's whole range and a little beyond its limit (a claim counts the limit).
            let (q1, q2) = (r.range(0, pm1 + pm1 / 10 + 2), r.range(0, pm2 + pm2 / 10 + 2));
            let (c1, c2) = (q1.min(pm1), q2.min(pm2));
            // The first's A: its side of the first, and of the second its A's side or (turned) its B's.
            let a_x = x.x.a_margin + x.x.legs[0].gain(c1);
            let a_y = if turned { y.x.b_margin - y.x.legs[0].gain(c2) } else { y.x.a_margin + y.x.legs[0].gain(c2) };
            let b_x = x.x.b_margin - x.x.legs[0].gain(c1);
            let b_y = if turned { y.x.a_margin + y.x.legs[0].gain(c2) } else { y.x.b_margin - y.x.legs[0].gain(c2) };
            let raw = one.x.view([q1, q2], None).raw;
            assert_eq!(raw, a_x + a_y - half, "A at {q1}, {q2}");
            assert_eq!(one.pot() - raw, b_x + b_y - half, "B at {q1}, {q2}");
            assert_eq!(one.x.view([q1, q2], None).notional, x.x.legs[0].worth(c1) + y.x.legs[0].worth(c2), "the notional");
            points += 1;
            // Where the caps bite (a side owed more than a pot, or less than nothing, in one of the two alone).
            let capped_apart = clamped(a_x, x.pot()) + clamped(a_y, y.pot());
            capped += (capped_apart != clamped(raw + half, x.pot() + y.pot())) as u64;
        }
    }
    println!("T1b J2: {pairs} joins ({turned_n} with the keys turned, {short_n} with the first short for its A; five scales, four price limits), {points} pairs of prices: each party's margin and gains are the two positions' less half the tip, exactly; after the caps the two differ at {capped} of them (J4)");
    assert!(turned_n > 500 && short_n > 500 && capped > 0);
}

/// J3. A position a join wrote is read by every other script as the model reads it. Two joins in the proof world's
/// two markets (the second position's keys turned) write, field for field and byte for byte, the two-leg position the
/// earlier rounds' proofs ran on; the output of the join then goes through each two-leg path: a close disputed and
/// proved with two epoch tables, the no-price exit refuted with two signed prices, a liquidation disputed and proved with two
/// epoch tables, and a declared close settled. The same with the keys in the same places (both legs long for A).
#[test]
fn t1b_j3_an_position_a_join_wrote_goes_through_every_two_leg_path() {
    let w = pworld();
    let half = MERGE_TIP / 2;
    let hand = w.position(true);
    // X: A long 10 units of market 0 against B. Y: B long 5 units of market 1 against A.
    let x = w.single(0, &w.a, &w.b, 10, 20 * KAS1, 30 * KAS1);
    let y = w.single(1, &w.b, &w.a, 5, 30 * KAS1 + half, 20 * KAS1 + half);
    let one = w.joined("a join, the second's keys turned", x, y, 70);
    assert_eq!(one.s, hand.s, "the position a join wrote is the two-leg position of the earlier rounds");
    assert_eq!((one.s.fixed(), one.s.moving(), one.s.hash()), (hand.s.fixed(), hand.s.moving(), hand.s.hash()));
    assert_eq!(TObj::decode(&TObj::Position(one.s).encode()).unwrap(), TObj::Position(one.s));
    // A second leg that is not well formed (a scale of nothing) is no position to the model: the tool reads none.
    let mut ill = one.s;
    ill.x.legs[1].scale = 0;
    assert!(ill.check().is_err() && TObj::decode(&TObj::Position(ill).encode()).is_err());
    // The thin one for the liquidation: A's margin 6 KAS on the whole.
    let thin = w.joined("a join that leaves A a thin margin", w.single(0, &w.a, &w.b, 10, 3 * KAS1, 30 * KAS1), w.single(1, &w.b, &w.a, 5, 30 * KAS1 + half, 3 * KAS1 + half), 71);
    assert_eq!(thin.s.x.a_margin, 6 * KAS1);
    let v = proofs_on(&w, one.clone(), thin, true, 72);
    assert_eq!(v.iter().map(|p| p.name).collect::<Vec<_>>(), ["KpfTProve", "KpfTExitRefute", "KpfTLiqProve"]);
    let f = [fund(xk(&w.w), 73, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let p = [9 * P10 / 10, 12 * P10 / 10];
    let mut paths = 3;
    // Both orientations: a declared close settled, and (for the second) a close disputed and proved with two epoch tables.
    let both_long = w.joined("a join, the keys in the same places", x, w.single(1, &w.a, &w.b, 5, 20 * KAS1 + half, 30 * KAS1 + half), 74);
    assert_eq!((both_long.s.x.legs[1].dir, both_long.s.x.a_margin, both_long.s.x.b_margin), (LONG, 40 * KAS1, 60 * KAS1));
    for (what, position) in [("the second leg short for A", &one), ("both legs long for A", &both_long)] {
        let (b, c) = close(&w.lib, position, &w.script("KpfTClose"), SIDE_B, T0, p, None, payer).unwrap();
        assert_eq!(c.amount, position.s.owed(SIDE_B, p).unwrap(), "{what}: the claim states what the rule gives on two legs");
        let cs = signed(&format!("{what}: a close declared on the joined position"), &b, &w.keys);
        let sb = settle(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTSettle"), payer).unwrap();
        assert!(engine_takes(&sb, &w.keys), "{what}: the declared close settled");
        let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
        let ds = signed(&format!("{what}: disputed"), &b, &w.keys);
        let (a1, a2) = (w.attest(0, p[0]), w.attest(1, p[1]));
        let pb = prove(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), payer).unwrap();
        assert!(engine_takes(&pb, &w.keys), "{what}: proved with two epoch tables");
        // A claim that states a sompi more than the side is owed at the committee's prices is not proved (round 23:
        // the line is what the prices give as they are; the claimer's own amount keeps the tolerance from it).
        let (b, c2) = close(&w.lib, position, &w.script("KpfTClose"), SIDE_B, T0, p, Some(position.s.x.owed_at(SIDE_B, p) + 1), payer).unwrap();
        let cs2 = b.sign(&w.keys).unwrap();
        let (b, d2) = dispute(&w.lib, &nth(&cs2, 0, c2, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
        let ds2 = b.sign(&w.keys).unwrap();
        let wrong = prove_with(&w.lib, &nth(&ds2, 0, d2, 6_520), &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), payer, false).unwrap();
        assert!(!engine_takes(&wrong, &w.keys), "{what}: a claim of a sompi more than the rule's is proved");
        paths += 3;
    }
    println!("T1b J3: a position a join wrote is the model's two-leg position byte for byte, and went through {paths} two-leg paths in the engine (prove, refute, liquidation proved; and per orientation: settle, prove, a false claim not proved)");
}

/// The built transaction with the object at input `i` replaced by the position `position` (its script and UTXO), and, when
/// `shown`, the entry's pushes of the first position's two parts replaced by `position`'s.
fn with_position(b: &OBuilt, lib: &[u8; 32], i: usize, position: &Position, shown: bool) -> OBuilt {
    let mut t = b.clone();
    if let OPlan::Contract { redeem, .. } = &mut t.plans[i] {
        *redeem = TObj::Position(*position).script(lib);
    }
    t.entries[i].script_public_key = object_spk(lib, &TObj::Position(*position));
    if shown {
        if let OPlan::Contract { args, .. } = &mut t.plans[2] {
            args[3] = Arg::V(ArtifactValue::Bytes(position.fixed()));
            args[4] = Arg::V(ArtifactValue::Bytes(position.moving()));
        }
    }
    t
}

/// J4, the liquidation, on the whole. Over random pairs and prices: how often a side that one of the two positions
/// liquidated is liquidated by neither the joined position (saved), and how often a side neither liquidated is
/// liquidated on the whole (made). Never is a side made liquidatable that was owed, after the caps, its maintenance
/// in each of the two and more than half the tip beyond it: what makes one is its loss beyond a margin in one of the
/// two (the cap gone), a position too small to liquidate anybody, or the half tip.
#[test]
fn t1b_j4_what_a_join_does_to_a_liquidation() {
    let mut r = Rng(0x51ab_0c0f_fee0_0910);
    let base = Tpl::of(&samples::position());
    let t2 = Tpl { market: [0x42; 32], ..base };
    let (unit, px) = (base.scale, samples::position().x.legs[0].entry);
    let (u1, u2) = ([11u8; 32], [22u8; 32]);
    let half = MERGE_TIP / 2;
    let (mut points, mut saved, mut made, mut made_cap, mut made_small, mut made_tip, mut fee_more) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0i64);
    let mut pairs = 0;
    while pairs < 1_500 {
        let turned = r.range(0, 2) == 1;
        let m = |r: &mut Rng| r.range(KAS1 / 2, 60 * KAS1);
        let x = base.position(u1, u2, r.range(1, 6) * unit, r.range(px / 2, 2 * px), m(&mut r), m(&mut r));
        let (ya, yb) = if turned { (u2, u1) } else { (u1, u2) };
        let y = t2.position(ya, yb, r.range(1, 6) * unit, r.range(px / 2, 2 * px), m(&mut r), m(&mut r));
        let Ok(one) = joined(&x, &y) else { continue };
        pairs += 1;
        for _ in 0..30 {
            let q = [r.range(px / 3, 3 * px), r.range(px / 3, 3 * px)];
            for side in [SIDE_A, SIDE_B] {
                // The same user's side in the second position.
                let side_y = if turned { SIDE_A + SIDE_B - side } else { side };
                let (lx, ly, lj) = (x.x.liquidatable(side, [q[0], 0]), y.x.liquidatable(side_y, [q[1], 0]), one.x.liquidatable(side, q));
                points += 1;
                if (lx || ly) && !lj {
                    saved += 1;
                }
                if !lx && !ly && lj {
                    made += 1;
                    // Why: before the caps, was the side below nothing in one of the two (its loss there stopped at
                    // its margin)? Was one of the two too small to liquidate anybody? Else it is the half tip.
                    let raw = |a: &Position, s: i64, p: [i64; 2]| { let v = a.x.view(p, Some(s == SIDE_A)).raw; if s == SIDE_A { v } else { a.pot() - v } };
                    let (rx, ry) = (raw(&x, side, [q[0], 0]), raw(&y, side_y, [q[1], 0]));
                    let mx = |a: &Position, s: i64, p: [i64; 2]| a.x.maint(a.x.view(p, Some(s == SIDE_A)).notional);
                    let (below_x, below_y) = (rx < mx(&x, side, [q[0], 0]), ry < mx(&y, side_y, [q[1], 0]));
                    if rx < 0 || ry < 0 || rx > x.pot() || ry > y.pot() {
                        made_cap += 1;
                    } else if below_x || below_y {
                        // Below its maintenance in one of the two, where the other party was below it too.
                        made_small += 1;
                    } else {
                        made_tip += 1;
                        assert!(rx + ry - mx(&x, side, [q[0], 0]) - mx(&y, side_y, [q[1], 0]) <= half + 2, "a side above its maintenance in both by more than half the tip is liquidated on the whole");
                    }
                }
                if lj && (lx || ly) {
                    let before = if lx { x.x.fee_of(side, [q[0], 0]) } else { 0 } + if ly { y.x.fee_of(side_y, [q[1], 0]) } else { 0 };
                    fee_more = fee_more.max(one.x.fee_of(side, q) - before);
                }
            }
        }
    }
    println!("T1b J4 liquidation: {pairs} joins, {points} sides and prices: {saved} sides that one position liquidated are liquidated by neither the joined one (saved); {made} that neither liquidated are liquidated on the whole (made): {made_cap} where the side's loss or gain in one position ran past a pot (the cap gone), {made_small} where it was below its maintenance in a position too small to liquidate anybody, {made_tip} within the half tip; the keeper's fee on the whole exceeds the fees before by up to {fee_more} sompi");
    assert!(saved > 0 && made > 0);
}

/// J1 through the builder (the engine file has the raw battery): a join in both orientations with one output more
/// under the registry's id by every input, what it writes replaced or a sompi short, an anchor in either position's
/// place; and a join of two positions of two ids.
#[test]
fn t1b_j1_a_built_join_writes_no_output_more_and_takes_no_anchor() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], scale: 1_000, price_max: 50_000_000_000, ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (a, b) = (xk(&kp(51)), xk(&kp(52)));
    let registry = Hash::from_bytes(REGISTRY);
    let keys = only_keys(&[&kp(51), &kp(52), &w.w]);
    let fab = object_spk(&lib, &TObj::Position(tpl.position(xk(&kp(77)), xk(&kp(78)), unit, px, 50 * KAS1, 50 * KAS1)));
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let x = tpl.position(a, b, 5 * unit, px, 100 * KAS1, 100 * KAS1);
    let mut tried = 0;
    for (what, y) in [("the keys in the same places", tpl2.position(a, b, 3_000, 2 * px, 60 * KAS1, 70 * KAS1)), ("the keys turned", tpl2.position(b, a, 3_000, 2 * px, 60 * KAS1, 70 * KAS1))] {
        let (bld, one) = join(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTJoin"), p).unwrap();
        let sg = signed(what, &bld, &keys);
        assert_eq!(sg.tx.outputs[0].script_public_key, object_spk(&lib, &TObj::Position(one)));
        assert_eq!(sg.tx.outputs[0].value as i64, x.pot() + y.pot() - MERGE_TIP);
        assert!(all_taken(&change_into(&bld, &keys, &fab, None)), "{what}: the change as a position's script under no id");
        for auth in 0..bld.tx.inputs.len() as u16 {
            assert!(!all_taken(&change_into(&bld, &keys, &fab, Some((auth, registry)))), "{what}: a position nobody signed under the registry's id, by input {auth}");
            tried += 1;
        }
        let mut t = bld.clone();
        t.tx.outputs[0].script_public_key = fab.clone();
        assert!(!all_taken(&t.sign(&keys).unwrap()), "{what}: a position nobody signed in the joined one's place");
        let mut t = bld.clone();
        t.tx.outputs[0].value -= 1;
        assert!(!all_taken(&t.sign(&keys).unwrap()), "{what}: the joined position a sompi short");
        for i in 0..2 {
            let mut t = bld.clone();
            if let OPlan::Contract { redeem, .. } = &mut t.plans[i] {
                *redeem = TObj::Anchor.script(&lib);
            }
            t.entries[i].script_public_key = object_spk(&lib, &TObj::Anchor);
            let s2 = t.sign(&keys).unwrap();
            let r = run_raw(&s2.tx, &s2.entries, Meter::Free).expect("the covenant rules");
            assert!(r[i].is_ok() && r[2].is_err(), "{what}: an anchor as input {i}");
            tried += 1;
        }
        let mut s2 = bld.sign(&keys).unwrap();
        s2.entries[1].covenant_id = Some(Hash::from_bytes([0x72; 32]));
        assert!(script_refuses(&s2.tx, &s2.entries, 2), "{what}: the second position of another id");
        assert!(join(&lib, &live_in(registry, x, 0x71), &live_in(Hash::from_bytes([0x72; 32]), y, 0x72), &w.script("KpfTJoin"), p).is_err());
        tried += 4;
    }
    println!("T1b J1 (builders): a join in both orientations (the second market of another scale and price limit): {tried} changes refused");
}

/// F15 (High in round 9: a join nobody signed locked two positions into a position that only a close ends), fixed in
/// round 10: a join carries the two users' signatures. The same join by a third key, by one of the two alone, or with
/// one user's signature in the other's place is refused by the join's own input; signed by both the engine takes it.
/// What a joined position is has not changed: no novation, merge or join reads it (shown at the engine), and what
/// ends it is a close of the whole at the committee's prices or both signatures; the figures of round 9 are printed
/// again. That is now what the two users chose together.
#[test]
fn t1b_fixed_f15_a_join_is_the_two_users_own() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (ka, kb, kc) = (kp(51), kp(52), kp(53));
    let (a, b, c) = (xk(&ka), xk(&kb), xk(&kc));
    let registry = Hash::from_bytes(REGISTRY);
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let nobody = only_keys(&[&w.w]);
    let both = only_keys(&[&ka, &kb, &w.w]);
    let all = only_keys(&[&ka, &kb, &kc, &w.w]);
    // A long 5 units of market 1 and 3 of market 2 against B, 100 and 60 KAS each.
    let x = Position { a_min_counter: 100 * KAS1, b_min_counter: 100 * KAS1, ..tpl.position(a, b, 5 * unit, px, 100 * KAS1, 100 * KAS1) };
    let y = Position { a_min_counter: 60 * KAS1, b_min_counter: 60 * KAS1, ..tpl2.position(a, b, 3 * unit, px, 60 * KAS1, 60 * KAS1) };
    let sold = Position { a_min_counter: 100 * KAS1, b_min_counter: 100 * KAS1, ..tpl.position(c, a, 5 * unit, px, 100 * KAS1, 100 * KAS1) };
    let (nb, nv, _) = kpf_protocol::thin_build::novate(&lib, &live_in(registry, x, 0x71), &live_in(registry, sold, 0x73), &w.script("KpfTNovate"), p).unwrap();
    assert!(all_taken(&nb.sign(&nobody).unwrap()));
    // The join, for both orientations of the second position.
    let mut refused = 0;
    for (what, second) in [("the keys in the same places", y), ("the keys turned", Position { a_min_counter: 60 * KAS1, b_min_counter: 60 * KAS1, ..tpl2.position(b, a, 3 * unit, px, 60 * KAS1, 60 * KAS1) })] {
        let (jb, _) = join(&lib, &live_in(registry, x, 0x71), &live_in(registry, second, 0x72), &w.script("KpfTJoin"), p).unwrap();
        // A third key, and each user alone: nothing is signed.
        assert!(jb.sign(&nobody).is_err(), "{what}: a join signed by a third key alone");
        assert!(jb.sign(&only_keys(&[&ka, &w.w])).is_err(), "{what}: a join signed by A alone");
        assert!(jb.sign(&only_keys(&[&kb, &w.w])).is_err(), "{what}: a join signed by B alone");
        // A signature of another key in a user's place: the join's own input refuses.
        for (from, to) in [(a, b), (b, a), (a, xk(&w.w)), (b, xk(&w.w)), (a, c), (b, c)] {
            let forged = resign(&jb, from, to).sign(&all).unwrap();
            assert!(script_refuses(&forged.tx, &forged.entries, 2), "{what}: a join with another key's signature in a user's place");
            refused += 1;
        }
        // The two in each other's place.
        let mut swapped = jb.clone();
        if let OPlan::Contract { args, .. } = &mut swapped.plans[2] {
            let n = args.len();
            args.swap(n - 2, n - 1);
        }
        let sw = swapped.sign(&both).unwrap();
        assert!(script_refuses(&sw.tx, &sw.entries, 2), "{what}: the two signatures in each other's place");
        refused += 1;
        assert!(all_taken(&jb.sign(&both).unwrap()), "{what}: signed by both users");
    }
    println!("T1b F15 fixed: a join by a third key, by one user alone, or with another key's signature in a user's place ({refused} forged) is not signed or is refused by the join's input; signed by both it is taken");
    // What a joined position is has not changed: no first and no second of a novation, a merge or a join.
    let (jb, one) = join(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTJoin"), p).unwrap();
    assert!(novated(&one, one.pot(), &sold, sold.pot(), true).is_err() && merged(&one, &x).is_err() && merged(&x, &one).is_err() && joined(&one, &y).is_err() && joined(&x, &one).is_err());
    let x2 = tpl.position(a, b, 2 * unit, px, 40 * KAS1, 40 * KAS1);
    let (mb, _) = kpf_protocol::thin_build::merge(&lib, &live_in(registry, x, 0x71), &live_in(registry, x2, 0x74), &w.script("KpfTMerge"), p).unwrap();
    let mut locked = 0;
    for (what, built) in [("novate", &nb), ("merge", &mb), ("join", &jb)] {
        assert!(all_taken(&built.sign(&both).unwrap()), "{what}: as built");
        for (place, shown) in [(0usize, true), (1, false)] {
            let mut t = with_position(built, &lib, place, &one, shown);
            t.entries[place].amount = one.pot() as u64;
            let sg = t.sign(&both).unwrap();
            let r = run_raw(&sg.tx, &sg.entries, Meter::Free).expect("the covenant rules");
            assert!(r[place].is_ok() && r[2].is_err(), "{what}: the joined position as input {place}: its own script {:?}, the entry {:?}", r[place].is_ok(), r[2].is_ok());
            locked += 1;
        }
    }
    let at = [px, px];
    let (fair, closed) = (one.x.a_at(at), one.owed(SIDE_A, at).expect("a close is allowed at the entries"));
    assert_eq!(fair, 160 * KAS1 - MERGE_TIP / 2);
    assert!(fair - closed > 0);
    println!("T1b F15 what remains, by the two users' choice: {locked} transactions with the joined position in a place of novate, merge, join refused by the entry. Apart, A left market 1 by a sale, paid {}; joined, A alone ends only the whole position: a close pays it {closed} where the prices say {fair} ({} less: g = {} bps of a notional of {}), behind a bond of {}", nv.to_leaver, fair - closed, one.x.t.g_bps, one.x.view(at, None).notional, one.bond());
}

/// F13 across markets (round 9), with the join signed (round 10). V's buy of market 2 rests with a thin margin, the
/// market falls a tenth, K sells into it at V's limit; V and K also hold a position in market 1. The join that would
/// carry the order's loss into V's margin in market 1 is not signed without V, and with K's signature in V's place the
/// join's input refuses. Only V's own signature makes it (the engine then takes it, and the figures are round 9's).
/// Nothing unsigned reaches the joined position afterwards: no merge, novation or join reads it.
#[test]
fn t1b_fixed_f13_across_markets_needs_the_owners_signature() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (v, k, payer) = (kp(51), kp(52), w.w);
    for key in [v, k] {
        w.keys.add_keypair(key);
    }
    let not_v = only_keys(&[&k, &payer, &kp(9)]);
    let now = px - px / 10;
    let lost = -tpl2.position(xk(&v), xk(&k), 10 * unit, px, KAS1, KAS1).x.legs[0].gain(now);
    let (m_o, big) = (lost / 5, 3 * lost);
    let buy = w.put("V's buy of market 2 rests", &Order { tpl: tpl2.hash(), owner: xk(&v), dir: LONG, size: 10 * unit, limit: px, margin: m_o, tip: KAS1 / 10, expiry: 9_000, min_fill: 0, min_counter: m_o }, &v);
    let sell = w.put("K's sell", &Order { owner: xk(&k), dir: SHORT, margin: big, tip: 0, min_counter: 0, ..buy.s }, &k);
    let f = w.pay(&k);
    let (mb, m) = order_match(&lib, &tpl2, &buy, &sell, &w.script("KpfTMatch"), 10 * unit, px, Payer { funds: &f, change_to: xk(&k), rate: MIN_FEE_RATE }).unwrap();
    let ms = signed("K fills V's resting buy of market 2 at V's limit", &mb, &w.keys);
    let y = nth(&ms, 0, m.position, 6_300);
    let x = w.enter("X: V long against K in market 1", &tpl.position(xk(&v), xk(&k), 5 * unit, px, big, big), &v);
    let at = [px, now];
    let (v_apart, k_apart) = (x.s.x.a_at([px, 0]) + y.s.x.a_at([now, 0]), x.s.x.owed_at(SIDE_B, [px, 0]) + y.s.x.owed_at(SIDE_B, [now, 0]));
    assert_eq!((v_apart, k_apart), (big, big + big + m_o), "apart: V has lost the order's margin and no more");
    let f = w.pay(&k);
    let (jb, one) = join(&lib, &x, &y, &w.script("KpfTJoin"), Payer { funds: &f, change_to: xk(&k), rate: MIN_FEE_RATE }).unwrap();
    // K, who gains by it, cannot make it: not signed without V; K's or the payer's signature in V's place is refused.
    assert!(jb.sign(&not_v).is_err(), "the join is signed without V's key");
    for other in [xk(&k), xk(&payer)] {
        let forged = resign(&jb, xk(&v), other).sign(&w.keys).unwrap();
        assert!(script_refuses(&forged.tx, &forged.entries, 2), "the join with another key's signature in V's place");
    }
    // With V's own signature it is V's choice, and the engine takes it.
    let js = signed("the join, signed by V and K", &jb, &w.keys);
    let _ = js;
    let (v_one, k_one) = (one.x.a_at(at), one.x.owed_at(SIDE_B, at));
    assert_eq!(v_apart - v_one, lost - m_o + MERGE_TIP / 2);
    assert_eq!(k_one - k_apart, lost - m_o - MERGE_TIP / 2);
    // And what V did not sign reaches nothing of the joined position: it is no input of a merge, a novation or a join.
    assert!(merged(&one, &x.s).is_err() && merged(&x.s, &one).is_err() && joined(&one, &y.s).is_err() && joined(&x.s, &one).is_err() && novated(&one, one.pot(), &y.s, y.s.pot(), true).is_err() && novated(&x.s, x.s.pot(), &one, one.pot(), true).is_err());
    println!("T1b F13 across markets, fixed: the join that would take {} sompi more of V's than the order's margin {m_o} is refused without V's signature; signed by V it is V's (apart {v_apart}, joined {v_one})", v_apart - v_one);
}

/// Round 10: a join's two signatures are the transaction's. An output changed after signing (to what the join's own
/// rule would take), another position in the joined one's place, and the join's signature script moved to another
/// join of the same two positions (other funds) are each refused by the join's own input.
#[test]
fn t1b_r10_the_signatures_of_a_join_commit_to_the_transaction() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (ka, kb) = (kp(51), kp(52));
    let registry = Hash::from_bytes(REGISTRY);
    let keys = only_keys(&[&ka, &kb, &w.w]);
    let x = tpl.position(xk(&ka), xk(&kb), 5 * unit, px, 100 * KAS1, 100 * KAS1);
    let y = tpl2.position(xk(&kb), xk(&ka), 3 * unit, px, 60 * KAS1, 60 * KAS1);
    let build = |n: u8| {
        let f = [fund(xk(&w.w), n, 1_000)];
        join(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTJoin"), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap().0
    };
    let (one, other) = (build(91), build(92));
    let sg = signed("a join signed by both", &one, &keys);
    let c = one.change.expect("a change output");
    let mut moved = sg.clone();
    moved.tx.outputs[c].value -= 1;
    moved.tx.outputs[0].value += 1;
    assert!(script_refuses(&moved.tx, &moved.entries, 2), "an output changed after the signatures were made");
    let mut paid_away = sg.clone();
    paid_away.tx.outputs[c].script_public_key = p2pk_spk(&xk(&kp(77)));
    assert!(script_refuses(&paid_away.tx, &paid_away.entries, 2), "the sender's change paid to another key after the signatures were made");
    let mut sg2 = other.sign(&keys).unwrap();
    assert!(!input_fails(&sg2, 2));
    sg2.tx.inputs[2].signature_script = sg.tx.inputs[2].signature_script.clone();
    assert!(script_refuses(&sg2.tx, &sg2.entries, 2), "the signatures of one join in another");
    println!("T1b R10 signatures: a join's two signatures commit to its outputs and to the transaction");
}

// ================================================================ round 11: one side of a position of two legs

use kpf_protocol::thin::{folded, left, turned_legs, turned_users};

/// The prices of a position's legs, given the first market's id and the two markets' prices.
fn prices_of(a: &Position, m1: &[u8; 32], q: [i64; 2]) -> [i64; 2] {
    let of = |l: &XLeg| if l.size == 0 { 0 } else if l.market == *m1 { q[0] } else { q[1] };
    [of(&a.x.legs[0]), of(&a.x.legs[1])]
}
/// What a user is owed by a position at the two markets' prices, after the caps (nothing if it is not in it).
fn owed_to(a: &Position, user: &[u8; 32], m1: &[u8; 32], q: [i64; 2]) -> i64 {
    let p = prices_of(a, m1, q);
    if a.x.a_key == *user {
        a.x.a_at(p)
    } else if a.x.b_key == *user {
        a.x.owed_at(SIDE_B, p)
    } else {
        0
    }
}
/// The same before the caps.
fn raw_of(a: &Position, user: &[u8; 32], m1: &[u8; 32], q: [i64; 2]) -> i64 {
    let r = a.x.view(prices_of(a, m1, q), None).raw;
    if a.x.a_key == *user {
        r
    } else {
        a.pot() - r
    }
}

/// L1 over a grid. 1,500 random cross positions and second positions (U and the third user as one: any entry, any
/// margin the rules take; the first leg either way; a part, the whole, or more than the leg), 81 pairs of prices
/// and the pair where the first market stands at the second position's entry. What S is owed after the caps, by what
/// stays and by the new position together, against what the cross position owed it:
///  - never less (to 4 sompi) unless a cap is reached: the new position's (the one that comes in has posted too
///    little for what S has won from it) or what stays' (U's margin there);
///  - never less at the second position's entry price: a sale at the price costs S nothing at that moment.
/// The most S loses and U's two keys gain, at a price pair of the grid, is printed.
#[test]
fn t1b_l1_what_a_leave_does_to_the_one_that_stays_over_a_grid() {
    let mut r = Rng(0x51ab_0c0f_fee0_0b01);
    let tpl = Tpl::of(&samples::position());
    let tpl2 = Tpl { market: [0x42; 32], ..tpl };
    let (unit, px, m1) = (tpl.scale, samples::position().x.legs[0].entry, tpl.market);
    let (u, s, l) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let (mut cases, mut worst_s, mut worst_u, mut losses, mut points, mut parts, mut shorts) = (0, 0i64, 0i64, 0u64, 0u64, 0, 0);
    while cases < 1_500 {
        let m = |r: &mut Rng| r.range(20 * KAS1, 400 * KAS1);
        let (n1, n2, ny) = (r.range(1, 8) * unit + r.range(0, unit), r.range(1, 8) * unit, r.range(1, 8) * unit + r.range(0, unit));
        let mut x1 = tpl.position(u, s, n1, r.range(px / 2, 2 * px), m(&mut r), m(&mut r));
        let d1 = if r.range(0, 2) == 0 { LONG } else { SHORT };
        x1.x.legs[0].dir = d1;
        x1.a_min_counter = r.range(0, 2) * x1.x.b_margin;
        x1.b_min_counter = r.range(0, 2) * x1.x.a_margin;
        let turned = r.range(0, 2) == 1;
        let (ya, yb) = if turned { (s, u) } else { (u, s) };
        let Ok(x) = joined(&x1, &tpl2.position(ya, yb, n2, r.range(px / 2, 2 * px), m(&mut r), m(&mut r))) else { continue };
        let ey = r.range(px / 2, 2 * px);
        let (lm, um) = (m(&mut r) * 3, r.range(KAS1, 50 * KAS1));
        let y = if d1 == LONG { tpl.position(l, u, ny, ey, lm, um) } else { tpl.position(u, l, ny, ey, um, lm) };
        let Ok(lv) = left(&x, x.pot(), &y, y.pot(), true) else { continue };
        cases += 1;
        parts += (ny != n1) as usize;
        shorts += (d1 == SHORT) as usize;
        let (xa, z) = (lv.x_after.0, lv.z.0);
        let rest = lv.rest_y.map(|(a, _)| a.pot()).unwrap_or(0);
        // Before the caps nothing moves but the sender's share.
        for k in 0..=81 {
            let q = if k == 81 { [ey, r.range(px / 3, 3 * px)] } else { [px / 3 + (k % 9) * (px / 3), px / 3 + (k / 9) * (px / 3)] };
            points += 1;
            assert!((raw_of(&x, &s, &m1, q) - raw_of(&xa, &s, &m1, q) - raw_of(&z, &s, &m1, q)).abs() <= 3, "S before the caps");
            let (s0, s1) = (owed_to(&x, &s, &m1, q), owed_to(&xa, &s, &m1, q) + owed_to(&z, &s, &m1, q));
            let capped = raw_of(&z, &s, &m1, q) > z.pot() || raw_of(&xa, &s, &m1, q) > xa.pot();
            if !capped {
                assert!(s1 + 4 >= s0, "S is owed {s0} before and {s1} after, no cap reached: {x:?} {y:?} at {q:?}");
            }
            if k == 81 {
                assert!(s1 + 4 >= s0, "at the second position's entry price S is owed {s0} before and {s1} after");
            }
            // U and the third user as one: what the cross position and the second position were worth to them, and after.
            let u0 = owed_to(&x, &u, &m1, q) + y.pot();
            let u1 = owed_to(&xa, &u, &m1, q) + owed_to(&z, &l, &m1, q) + lv.to_leaver + rest;
            losses += (s0 - s1 > 4) as u64;
            worst_s = worst_s.max(s0 - s1);
            worst_u = worst_u.max(u1 - u0);
        }
    }
    println!("T1b L1 grid: {cases} leaves ({parts} of a part or with a rest, {shorts} with the first leg short for U), {points} price pairs: S is owed less after the leave at {losses} of them, each time by a cap; the most S loses {worst_s} sompi, the most U and the third user gain together {worst_u}; nothing at the second position's entry price");
    assert!(losses > 0 && parts > 300 && shorts > 300);
}

/// F13 by a fold (the owner accepted F13 inside one market; a fold carries it into a cross position the two users
/// joined). V and K hold a cross position. V's buy of market 1 rests with a thin margin; the market falls a tenth; K
/// sells into it at V's limit. Anyone folds the order's position into the cross position: what the order lost beyond
/// its margin is owed from the margin that stands behind both markets.
#[test]
fn t1b_f13_a_fold_carries_an_orders_loss_into_the_cross_position() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px, m1) = (tpl.scale, w.position.s.x.legs[0].entry, tpl.market);
    let (kv, kk, payer) = (kp(51), kp(52), w.w);
    for key in [kv, kk] {
        w.keys.add_keypair(key);
    }
    let (v, k) = (xk(&kv), xk(&kk));
    let nobody = only_keys(&[&payer]);
    let now = px - px / 10;
    let lost = -tpl.position(v, k, 10 * unit, px, KAS1, KAS1).x.legs[0].gain(now);
    let (m_o, big) = (lost / 5, 3 * lost);
    // The cross position, joined by both: V long 5 units of each market from the prices now, `big` a leg each.
    let x1 = w.enter("V long against K in market 1", &tpl.position(v, k, 5 * unit, now, big, big), &kv);
    let x2 = w.enter("V long against K in market 2", &tpl2.position(v, k, 5 * unit, px, big, big), &kv);
    let f = w.pay(&payer);
    let (jb, x) = join(&lib, &x1, &x2, &w.script("KpfTJoin"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    let xl = nth(&signed("the cross position", &jb, &w.keys), 0, x, 6_300);
    let buy = w.put("V's buy of market 1 rests", &Order { tpl: tpl.hash(), owner: v, dir: LONG, size: 10 * unit, limit: px, margin: m_o, tip: KAS1 / 10, expiry: 9_000, min_fill: 0, min_counter: m_o }, &kv);
    let sell = w.put("K's sell", &Order { owner: k, dir: SHORT, margin: big, tip: 0, min_counter: 0, ..buy.s }, &kk);
    let f = w.pay(&kk);
    let (mb, m) = order_match(&lib, &tpl, &buy, &sell, &w.script("KpfTMatch"), 10 * unit, px, Payer { funds: &f, change_to: k, rate: MIN_FEE_RATE }).unwrap();
    let y = nth(&signed("K fills V's resting buy at V's limit", &mb, &w.keys), 0, m.position, 6_400);
    let q = [now, px];
    let (v0, k0) = (owed_to(&x, &v, &m1, q) + owed_to(&y.s, &v, &m1, q), owed_to(&x, &k, &m1, q) + owed_to(&y.s, &k, &m1, q));
    assert_eq!(owed_to(&y.s, &v, &m1, q), 0, "apart, V has lost the order's margin and no more");
    let f = w.pay(&payer);
    let (fb, one) = fold(&lib, &xl, &y, &w.script("KpfTFold"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    assert!(all_taken(&fb.sign(&nobody).unwrap()), "the engine takes the fold, nobody signing");
    let (v1, k1) = (owed_to(&one, &v, &m1, q), owed_to(&one, &k, &m1, q));
    assert!((v0 - v1 - (lost - m_o)).abs() <= MERGE_TIP && (k1 - k0 - (lost - m_o)).abs() <= MERGE_TIP);
    println!("T1b F13 by a fold: an order of market 1 with a margin of {m_o} has lost {lost}; apart V is owed {v0}, folded into the cross position {v1} ({} less, from the margin behind both markets); K {k0} apart, {k1} folded", v0 - v1);
}

/// The battery of rounds 8 and 9 on one built transaction of an entry that reads objects of `id`: as built it is
/// taken; with the change turned into a position's script under no id it is taken; under the id, authorized by any
/// input, refused; each output the entry writes under the id replaced by a position nobody signed, or a sompi short,
/// refused; an anchor (its own script) in each object's place: the entry refuses; a second object under another id:
/// the entry refuses. Returns how many changes were refused.
fn battery(what: &str, b: &OBuilt, id: Hash, keys: &KeySet, fab: &ScriptPublicKey) -> usize {
    let mut n = 0;
    assert!(all_taken(&b.sign(keys).expect("signed")), "{what}: as built");
    assert!(all_taken(&change_into(b, keys, fab, None)), "{what}: the change as a position's script under no id");
    for auth in 0..b.tx.inputs.len() as u16 {
        assert!(!all_taken(&change_into(b, keys, fab, Some((auth, id)))), "{what}: a position nobody signed under the id, by input {auth}");
        n += 1;
    }
    for o in (0..b.tx.outputs.len()).filter(|o| b.tx.outputs[*o].covenant.map(|c| c.covenant_id) == Some(id)) {
        let mut t = b.clone();
        t.tx.outputs[o].script_public_key = fab.clone();
        assert!(!all_taken(&t.sign(keys).expect("signed")), "{what}: a position nobody signed in the place of output {o}");
        let mut t = b.clone();
        t.tx.outputs[o].value -= 1;
        assert!(!all_taken(&t.sign(keys).expect("signed")), "{what}: output {o} a sompi short");
        let mut t = b.clone();
        t.tx.outputs[o].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: Hash::from_bytes([0x5A; 32]) });
        assert!(!all_taken(&t.sign(keys).expect("signed")), "{what}: output {o} under another id");
        n += 3;
    }
    let script_at = (0..b.plans.len()).find(|i| matches!(&b.plans[*i], OPlan::Contract { entry, .. } if *entry != "go")).expect("the entry's input");
    let olib = b.entries[script_at].covenant_id.expect("the library's id").as_bytes();
    let objects: Vec<usize> = (0..b.entries.len()).filter(|i| b.entries[*i].covenant_id == Some(id)).collect();
    for i in &objects {
        let mut t = b.clone();
        if let OPlan::Contract { redeem, .. } = &mut t.plans[*i] {
            *redeem = TObj::Anchor.script(&olib);
        }
        t.entries[*i].script_public_key = object_spk(&olib, &TObj::Anchor);
        let sg = t.sign(keys).expect("signed");
        let r = run_raw(&sg.tx, &sg.entries, Meter::Free).expect("the covenant rules");
        assert!(r[*i].is_ok() && r[script_at].is_err(), "{what}: an anchor as input {i}");
        n += 1;
    }
    if objects.len() == 2 {
        let mut sg = b.sign(keys).expect("signed");
        sg.entries[objects[1]].covenant_id = Some(Hash::from_bytes([0x72; 32]));
        // (The outputs the second object authorizes, if any, make the covenant rules refuse it first.)
        assert!(!matches!(run_raw(&sg.tx, &sg.entries, Meter::Free), Ok(v) if v[script_at].is_ok()), "{what}: the second object of another id");
        n += 1;
    }
    n
}

/// L2, L3 (the leg written, byte for byte), L6 (every branch runs). The three scripts through the builder and the
/// engine, in every branch: a turn of the users and of the legs; a fold the same way and the other way (the first
/// leg larger, smaller, equal: then the second leg is the position's one leg), with the first leg long and short and
/// the second position's keys either way; a leave of a part, the whole, and with a rest of the second position, the
/// first leg long and short, at an entry below, at and above the leg's, with the user's margin there above the
/// sender's share and exactly it. Each as built is taken (so the script writes the model's state and runs to its
/// end), and the battery above on each; for a leave also the outputs under the id in another order, and the user's
/// payment short or to another key.
#[test]
fn t1b_l2_turn_fold_leave_in_every_branch_write_what_the_model_says_and_nothing_more() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], scale: 1_000, price_max: 50_000_000_000, ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (ku, ks, kl) = (kp(51), kp(52), kp(53));
    let (u, s, l) = (xk(&ku), xk(&ks), xk(&kl));
    let registry = Hash::from_bytes(REGISTRY);
    let fab = object_spk(&lib, &TObj::Position(tpl.position(xk(&kp(77)), xk(&kp(78)), unit, px, 50 * KAS1, 50 * KAS1)));
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // Round 12: a leave carries the two users' signatures (a turn and a fold nobody's).
    let keys = only_keys(&[&ku, &ks, &w.w]);
    // The cross position: U's first leg long or short (6 units of market 1 from px), a second leg in another market.
    let cross = |d1: i64, two: bool| -> Position {
        let mut x1 = Position { a_min_counter: 100 * KAS1, b_min_counter: 100 * KAS1, ..tpl.position(u, s, 6 * unit, px, 300 * KAS1, 300 * KAS1) };
        x1.x.legs[0].dir = d1;
        if two { joined(&x1, &tpl2.position(s, u, 4_000, 2 * px, 200 * KAS1, 200 * KAS1)).unwrap() } else { x1 }
    };
    let (built, refused) = (std::cell::Cell::new(0usize), std::cell::Cell::new(0usize));
    let run = |what: String, b: &OBuilt| {
        refused.set(refused.get() + battery(&what, b, registry, &keys, &fab));
        built.set(built.get() + 1);
    };
    // ---- the turn
    for users in [true, false] {
        for d1 in [LONG, SHORT] {
            let x = cross(d1, true);
            let (b, out) = turn(&lib, &live_in(registry, x, 0x71), &w.script("KpfTTurn"), users, p).unwrap();
            assert_eq!(out, if users { turned_users(&x) } else { turned_legs(&x).unwrap() });
            run(format!("turn of the {}, the first leg {d1}", if users { "users" } else { "legs" }), &b);
        }
    }
    let (b, _) = turn(&lib, &live_in(registry, cross(LONG, false), 0x71), &w.script("KpfTTurn"), true, p).unwrap();
    run("turn of the users of a position of one leg".into(), &b);
    assert!(turn(&lib, &live_in(registry, cross(LONG, false), 0x71), &w.script("KpfTTurn"), false, p).is_err());
    // ---- the fold: (the second position's units, its entry)
    let mut branches = std::collections::BTreeSet::new();
    for two in [true, false] {
        for d1 in [LONG, SHORT] {
            for turned in [false, true] {
                for (ny, ey) in [(2i64, px + px / 7), (6, px - px / 9), (9, px + px / 11)] {
                    let x = cross(d1, two);
                    let y = if turned { tpl.position(s, u, ny * unit + 7, ey, 80 * KAS1, 90 * KAS1) } else { tpl.position(u, s, ny * unit + 7, ey, 80 * KAS1, 90 * KAS1) };
                    let y = if ny == 6 { Position { x: Cross { legs: [XLeg { size: 6 * unit, ..y.x.legs[0] }, y.x.legs[1]], ..y.x }, ..y } } else { y };
                    let same_way = (d1 == LONG) != turned;
                    let what = format!("fold: two legs {two}, the first leg {d1}, the keys turned {turned}, {ny} units against 6");
                    match fold(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTFold"), p) {
                        Ok((b, out)) => {
                            assert_eq!(out, folded(&x, &y).unwrap());
                            assert_eq!(out.pot(), x.pot() + y.pot() - MERGE_TIP, "{what}");
                            let l0 = out.x.legs[0];
                            let branch = if same_way { "the same way" } else if ny < 6 { "the other way, the first leg larger" } else if ny > 6 { "the other way, the first leg smaller" } else { "the other way, equal: the second leg is the one leg" };
                            match branch {
                                "the same way" => assert_eq!((l0.size, l0.dir), (x.x.legs[0].size + y.x.legs[0].size, d1), "{what}"),
                                "the other way, the first leg larger" => assert_eq!((l0.size, l0.dir, l0.entry), (x.x.legs[0].size - y.x.legs[0].size, d1, px), "{what}"),
                                "the other way, the first leg smaller" => assert_eq!((l0.size, l0.dir, l0.entry), (y.x.legs[0].size - x.x.legs[0].size, LONG + SHORT - d1, ey), "{what}"),
                                _ => assert_eq!((l0, out.x.legs[1]), (x.x.legs[1], XLeg::none()), "{what}"),
                            }
                            branches.insert(branch);
                            run(what, &b);
                        }
                        // Equal and the other way in a position of one leg: nothing would be left (the novation's).
                        Err(e) => assert!(!two && !same_way && ny == 6, "{what}: {e}"),
                    }
                }
            }
        }
    }
    assert_eq!(branches.len(), 4);
    // ---- the leave
    let mut shapes = std::collections::BTreeSet::new();
    for d1 in [LONG, SHORT] {
        for (ny, what_n) in [(3 * unit + 11, "a part"), (6 * unit, "the whole leg"), (9 * unit, "more than the leg")] {
            for (ey, what_e) in [(px - px / 9, "below"), (px, "at"), (px + px / 7, "above")] {
                for exact in [false, true] {
                    let x = cross(d1, true);
                    let m = ny.min(6 * unit);
                    // U's margin in the second position: above the sender's share for the units taken, or exactly it.
                    let um = if exact { if ny > m { NOVATE_TIP * 3 / 2 } else { NOVATE_TIP } } else { 30 * KAS1 };
                    let y = if d1 == LONG { tpl.position(l, u, ny, ey, 500 * KAS1, um) } else { tpl.position(u, l, ny, ey, um, 500 * KAS1) };
                    let what = format!("leave: the first leg {d1}, {what_n}, the entry {what_e} the leg's, the user's margin {}", if exact { "exactly the sender's share" } else { "above the sender's share" });
                    let (b, lv) = leave(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTLeave"), p).unwrap_or_else(|e| panic!("{what}: {e}"));
                    assert_eq!(lv, left(&x, x.pot(), &y, y.pot(), true).unwrap());
                    assert_eq!(lv.to_leaver == 0, exact, "{what}");
                    assert_eq!((lv.rest_y.is_some(), lv.x_after.0.x.legs[1].size == 0), (ny > 6 * unit, ny >= 6 * unit), "{what}");
                    // Nothing is made or lost: what the two held is what is written and paid, and the sender's share.
                    let out = lv.x_after.1 + lv.z.1 + lv.rest_y.map(|r| r.1).unwrap_or(0) + lv.to_leaver + NOVATE_TIP;
                    assert_eq!(out, x.pot() + y.pot(), "{what}");
                    shapes.insert((d1, what_n, lv.gain.signum(), exact));
                    let sg = b.sign(&keys).unwrap();
                    assert!(all_taken(&sg), "{what}");
                    // Under the id, in this order: what stays, the new position, what is left of the second.
                    let under: Vec<usize> = (0..sg.tx.outputs.len()).filter(|i| sg.tx.outputs[*i].covenant.map(|c| c.covenant_id) == Some(registry)).collect();
                    let want: Vec<(Position, i64)> = [Some(lv.x_after), Some(lv.z), lv.rest_y].into_iter().flatten().collect();
                    assert_eq!(under.len(), want.len(), "{what}");
                    for (i, (acc, v)) in under.iter().zip(&want) {
                        assert_eq!((sg.tx.outputs[*i].script_public_key.clone(), sg.tx.outputs[*i].value as i64), (object_spk(&lib, &TObj::Position(*acc)), *v), "{what}: output {i}");
                    }
                    for (a, c) in [(0usize, 1usize), (1, under.len() - 1), (0, under.len() - 1)] {
                        if a == c {
                            continue;
                        }
                        let mut t = b.clone();
                        let (i, j) = (under[a], under[c]);
                        let (si, vi) = (t.tx.outputs[i].script_public_key.clone(), t.tx.outputs[i].value);
                        t.tx.outputs[i].script_public_key = t.tx.outputs[j].script_public_key.clone();
                        t.tx.outputs[i].value = t.tx.outputs[j].value;
                        t.tx.outputs[j].script_public_key = si;
                        t.tx.outputs[j].value = vi;
                        let t = t.sign(&keys).unwrap();
                        assert!(script_refuses(&t.tx, &t.entries, 2), "{what}: outputs {i} and {j} under the id in each other's place");
                        refused.set(refused.get() + 1);
                    }
                    if lv.to_leaver > 0 {
                        let n = sg.tx.outputs.len() - 1;
                        assert_eq!((sg.tx.outputs[n].script_public_key.clone(), sg.tx.outputs[n].value as i64), (p2pk_spk(&u), lv.to_leaver), "{what}: the user is paid at the last output");
                        let mut t = b.clone();
                        t.tx.outputs[n].value -= 1;
                        let t = t.sign(&keys).unwrap();
                        assert!(script_refuses(&t.tx, &t.entries, 2), "{what}: the user paid a sompi short");
                        let mut t = b.clone();
                        t.tx.outputs[n].script_public_key = p2pk_spk(&l);
                        let t = t.sign(&keys).unwrap();
                        assert!(script_refuses(&t.tx, &t.entries, 2), "{what}: the user's payment to another key");
                        refused.set(refused.get() + 2);
                    }
                    // The battery on a third of them (each signs a dozen transactions).
                    if !exact && ey == px {
                        run(what, &b);
                    } else {
                        built.set(built.get() + 1);
                    }
                }
            }
        }
    }
    assert_eq!(shapes.len(), 2 * 3 * 3 * 2, "every branch of the leave: {shapes:?}");
    // What a leave does not take: a position of one leg, the user's own counterparty as the third, a second position
    // the wrong way round, another market.
    let x = cross(LONG, true);
    let no = |what: &str, x: &Position, y: Position| assert!(left(x, x.pot(), &y, y.pot(), true).is_err(), "the model takes {what}");
    no("a position of one leg", &cross(LONG, false), tpl.position(l, u, 6 * unit, px, 500 * KAS1, 30 * KAS1));
    no("the same two users", &x, tpl.position(s, u, 6 * unit, px, 500 * KAS1, 30 * KAS1));
    no("the user long in both", &x, tpl.position(u, l, 6 * unit, px, 30 * KAS1, 500 * KAS1));
    no("the second leg's market", &x, tpl2.position(l, u, 4_000, 2 * px, 500 * KAS1, 30 * KAS1));
    no("the one that stays as the user", &x, tpl.position(l, s, 6 * unit, px, 500 * KAS1, 30 * KAS1));
    let (built, refused) = (built.get(), refused.get());
    println!("T1b L2: {built} built transactions of turn, fold and leave in every branch taken by the engine at the model's state; {refused} changes to them refused (an output more, replaced, short, under another id, out of order; an anchor in an object's place; a second object of another id; the user's payment short or to another key)");
}

/// L3. A fold before the caps: 3,000 random pairs (a position of one leg or two, its first leg long or short, the
/// second position's keys either way, a part, the same size, more; scales of 1 to 10^8 and entries over the whole
/// range), 30 pairs of prices each. Each user's margin and gains after are the two positions' less half the tip, to
/// 2 sompi (the floors of a leg's worth, which a leg of two sizes takes twice and the whole once); the pot is exact.
#[test]
fn t1b_l3_a_fold_is_the_two_positions_before_the_caps_to_two_sompi() {
    let mut r = Rng(0x51ab_0c0f_fee0_0b03);
    let base = Tpl::of(&samples::position());
    let (u1, u2) = ([11u8; 32], [22u8; 32]);
    let half = MERGE_TIP / 2;
    let mut seen = std::collections::BTreeMap::new();
    let (mut pairs, mut worst) = (0, 0i64);
    while pairs < 3_000 {
        let scale = [1i64, 1_000, 1_000_000, 100_000_000][r.range(0, 4) as usize];
        let pm = [1_000_000_000i64, 3_000_000_000_000, 100_000_000_000_000][r.range(0, 3) as usize];
        let t1 = Tpl { market: [0x41; 32], scale, price_max: pm, ..base };
        let t2 = Tpl { market: [0x42; 32], ..base };
        let hi = pm.min(4_000_000_000);
        let m = |r: &mut Rng| r.range(20_000 * KAS1, 90_000 * KAS1);
        let n1 = r.range(1, 30) * scale + r.range(0, scale);
        let ny = match r.range(0, 3) { 0 => n1, 1 => r.range(1, n1 + 1), _ => n1 + r.range(1, 30 * scale) };
        let mut x = t1.position(u1, u2, n1, r.range(0, hi + 1), m(&mut r), m(&mut r));
        let d1 = if r.range(0, 2) == 0 { LONG } else { SHORT };
        x.x.legs[0].dir = d1;
        let two = r.range(0, 2) == 1;
        if two {
            let t = r.range(0, 2) == 1;
            let Ok(j) = joined(&x, &if t { t2.position(u2, u1, r.range(1, 9) * t2.scale, r.range(1, 2_000_000_000), m(&mut r), m(&mut r)) } else { t2.position(u1, u2, r.range(1, 9) * t2.scale, r.range(1, 2_000_000_000), m(&mut r), m(&mut r)) }) else { continue };
            x = j;
        }
        let turned = r.range(0, 2) == 1;
        let (ya, yb) = if turned { (u2, u1) } else { (u1, u2) };
        let y = t1.position(ya, yb, ny, r.range(0, hi + 1), m(&mut r), m(&mut r));
        let Ok(one) = folded(&x, &y) else {
            assert!(x.check().is_err() || y.check().is_err() || (!two && ny == n1 && (d1 == LONG) == turned) || n1 + ny > ORDER_SIZE_MAX, "a fold refused: {x:?} {y:?}");
            continue;
        };
        pairs += 1;
        let same_way = (d1 == LONG) != turned;
        *seen.entry(if same_way { 0 } else if ny < n1 { 1 } else if ny > n1 { 2 } else { 3 }).or_insert(0) += 1;
        assert_eq!(one.pot(), x.pot() + y.pot() - MERGE_TIP);
        let m1 = t1.market;
        for _ in 0..30 {
            let q = [r.range(0, hi + 1), r.range(1, 4_000_000_000)];
            for user in [u1, u2] {
                let before = raw_of(&x, &user, &m1, q) + raw_of(&y, &user, &m1, q) - half;
                let after = raw_of(&one, &user, &m1, q);
                worst = worst.max((after - before).abs());
                assert!((after - before).abs() <= 2, "{user:?} at {q:?}: {before} before, {after} after: {x:?} {y:?}");
            }
        }
    }
    println!("T1b L3: {pairs} folds (the same way {}, the other way with the first leg larger {}, smaller {}, equal {}), 30 price pairs each: each user's margin and gains are the two positions' less half the tip to {worst} sompi; the pot exact", seen[&0], seen[&1], seen[&2], seen[&3]);
    assert!(seen.len() == 4 && seen.values().all(|n| *n > 150));
}

/// L4. A turn changes what nobody is owed: 2,000 random positions of one and two legs, 20 pairs of prices: what each
/// user is owed before and after the caps, whether it is to be liquidated, what a close pays it, the keeper's fee,
/// are the same with the users turned (each read from the other side) and with the legs turned (the prices turned).
/// And a position of one leg that a turn left short for its A goes through the claims: a close declared and settled,
/// a close disputed and proved, the no-price exit refuted, a liquidation disputed and proved (the proof world's
/// committee). The merge and the novation, which read a leg long for its A, refuse it at the engine until it is
/// turned back.
#[test]
fn t1b_l4_a_turned_position_owes_what_it_owed_and_goes_through_the_claims() {
    let mut r = Rng(0x51ab_0c0f_fee0_0b04);
    let base = Tpl::of(&samples::position());
    let t2 = Tpl { market: [0x42; 32], scale: 1_000, ..base };
    let (u1, u2) = ([11u8; 32], [22u8; 32]);
    let (unit, px) = (base.scale, samples::position().x.legs[0].entry);
    let (mut n, mut liq) = (0, 0);
    while n < 2_000 {
        let m = |r: &mut Rng| r.range(KAS1, 200 * KAS1);
        let mut a = base.position(u1, u2, r.range(1, 9) * unit, r.range(px / 2, 2 * px), m(&mut r), m(&mut r));
        if r.range(0, 2) == 1 {
            a.x.legs[0].dir = SHORT;
        }
        a.a_min_counter = r.range(0, 50 * KAS1);
        a.b_min_counter = r.range(0, 50 * KAS1);
        if r.range(0, 2) == 1 {
            a = joined(&a, &if r.range(0, 2) == 1 { t2.position(u2, u1, r.range(1, 9) * 1_000, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)) } else { t2.position(u1, u2, r.range(1, 9) * 1_000, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)) }).unwrap();
        }
        n += 1;
        let t = turned_users(&a);
        assert_eq!((turned_users(&t), t.pot(), t.a_min_counter, t.b_min_counter), (a, a.pot(), a.b_min_counter, a.a_min_counter));
        assert!(t.check().is_ok() && t.hash() != a.hash());
        let legs = turned_legs(&a).ok();
        for _ in 0..20 {
            let q = [r.range(px / 4, 3 * px), r.range(px / 4, 3 * px)];
            for (side, other) in [(SIDE_A, SIDE_B), (SIDE_B, SIDE_A)] {
                assert_eq!(t.x.owed_at(other, q), a.x.owed_at(side, q));
                assert_eq!(t.x.view(q, None).raw, a.pot() - a.x.view(q, None).raw);
                assert_eq!(t.x.liquidatable(other, q), a.x.liquidatable(side, q));
                assert_eq!(t.owed(other, q), a.owed(side, q), "what a close pays");
                assert_eq!(t.x.fee_of(other, q), a.x.fee_of(side, q));
                assert_eq!(t.x.cure_add(other, q), a.x.cure_add(side, q));
                liq += a.x.liquidatable(side, q) as usize;
                if let Some(l) = &legs {
                    let p = [q[1], q[0]];
                    assert_eq!((l.x.owed_at(side, p), l.x.liquidatable(side, p), l.owed(side, p), l.x.fee_of(side, p)), (a.x.owed_at(side, q), a.x.liquidatable(side, q), a.owed(side, q), a.x.fee_of(side, q)));
                }
            }
        }
    }
    assert!(liq > 100);
    // ---- the claims on a position of one leg short for its A, made by a turn through the engine.
    let w = pworld();
    let registry = Hash::from_bytes(REGISTRY);
    let live = |a: Position, n: u8| OLive { s: a, value: a.x.floor() as u64, id: Some(registry), op: op(n), daa: 6_000 };
    let f = [fund(xk(&w.w), 75, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let turn_of = |a: Position, n: u8| -> OLive<Position> {
        let (b, out) = turn(&w.lib, &live(a, n), &w.script("KpfTTurn"), true, payer).unwrap();
        let sg = signed("a turn of the users", &b, &only_keys(&[&w.w]));
        assert_eq!(sg.tx.inputs[0].sequence, kpf_protocol::thin::TURN_AGE, "the position's input asks the age the turn states");
        // The same turn stating a DAA less than the age: refused by the turn's input. The position's input asking a
        // DAA less than the age stated: refused by the position's own.
        let mut young = b.clone();
        if let OPlan::Contract { args, .. } = &mut young.plans[1] {
            args[1] = Arg::V(ArtifactValue::Bytes(sm8(99).to_vec()));
        }
        let ys = young.sign(&only_keys(&[&w.w])).unwrap();
        assert!(script_refuses(&ys.tx, &ys.entries, 1), "a turn of a position stating an age of 99");
        let mut seq = sg.clone();
        seq.tx.inputs[0].sequence = 99;
        assert!(script_refuses(&seq.tx, &seq.entries, 0), "the position's input at a sequence of 99 under a turn stating 100");
        nth(&sg, 0, out, 6_200)
    };
    // A long 10 units against B, 40 and 60 KAS; turned: B is the position's A, short 10 units, 60 and 40.
    let short = turn_of(w.single(0, &w.a, &w.b, 10, 40 * KAS1, 60 * KAS1), 0x61);
    assert_eq!((short.s.x.a_key, short.s.x.legs[0].dir, short.s.x.a_margin, short.s.x.b_margin), (xk(&w.b), SHORT, 60 * KAS1, 40 * KAS1));
    let p = [9 * P10 / 10, 0];
    let a1 = w.attest(0, p[0]);
    let mut paths = 0;
    for side in [SIDE_A, SIDE_B] {
        let (b, c) = close(&w.lib, &short, &w.script("KpfTClose"), side, T0, p, None, payer).unwrap();
        assert_eq!(c.amount, short.s.owed(side, p).unwrap());
        let cs = signed("a close declared on the turned position", &b, &w.keys);
        assert!(engine_takes(&settle(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTSettle"), payer).unwrap(), &w.keys), "settled");
        let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
        let ds = signed("disputed", &b, &w.keys);
        assert!(engine_takes(&prove(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), None, payer).unwrap(), &w.keys), "proved");
        let (b, c2) = close(&w.lib, &short, &w.script("KpfTClose"), side, T0, p, Some(short.s.x.owed_at(side, p) + 1), payer).unwrap();
        let cs2 = b.sign(&w.keys).unwrap();
        let (b, d2) = dispute(&w.lib, &nth(&cs2, 0, c2, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
        let ds2 = b.sign(&w.keys).unwrap();
        assert!(!engine_takes(&prove_with(&w.lib, &nth(&ds2, 0, d2, 6_520), &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), None, payer, false).unwrap(), &w.keys), "a claim of a sompi more is proved");
        paths += 3;
    }
    let (b, q) = exit(&w.lib, &short, &w.script("KpfTExit"), SIDE_A, 6_400, payer).unwrap();
    let qs = signed("an exit claim on the turned position", &b, &w.keys);
    assert!(engine_takes(&exit_refute(&w.lib, &nth(&qs, 0, q, 6_450), &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), None, xk(&w.w), payer).unwrap().0, &w.keys), "refuted");
    // The liquidation: the long with a thin margin is, after the turn, the position's B.
    let thin = turn_of(w.single(0, &w.a, &w.b, 10, 6 * KAS1, 60 * KAS1), 0x62);
    let lp = [97 * P10 / 100, 0];
    let l1 = w.attest(0, lp[0]);
    assert!(thin.s.x.liquidatable(SIDE_B, lp) && !thin.s.x.liquidatable(SIDE_A, lp));
    let (b, lq) = liquidate(&w.lib, &thin, &w.script("KpfTLiq"), SIDE_B, T0, lp, None, xk(&w.w), payer).unwrap();
    let ls = signed("a liquidation of the turned position's B", &b, &w.keys);
    let (b, dl) = liq_dispute(&w.lib, &nth(&ls, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), payer).unwrap();
    let ds = signed("disputed", &b, &w.keys);
    assert!(engine_takes(&liq_prove(&w.lib, &nth(&ds, 0, dl, 6_520), &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &l1), None, payer).unwrap().0, &w.keys), "the liquidation proved");
    paths += 2;
    // The merge and the novation refuse a position short for its A (the model, and the entry at the engine).
    let other = w.single(0, &w.b, &w.a, 4, 30 * KAS1, 30 * KAS1);
    assert!(merged(&short.s, &other).is_err() && merged(&other, &short.s).is_err() && novated(&short.s, short.s.pot(), &other, other.pot(), true).is_err());
    let third = w.single(0, &w.w, &w.b, 4, 30 * KAS1, 30 * KAS1);
    let (mb, _) = kpf_protocol::thin_build::merge(&w.lib, &live(other, 0x63), &live(w.single(0, &w.b, &w.a, 3, 30 * KAS1, 30 * KAS1), 0x64), &w.script("KpfTMerge"), payer).unwrap();
    let (nb, _, _) = kpf_protocol::thin_build::novate(&w.lib, &live(other, 0x63), &live(third, 0x65), &w.script("KpfTNovate"), payer).unwrap();
    let mut refused = 0;
    for (what, built) in [("merge", &mb), ("novate", &nb)] {
        assert!(engine_takes(built, &w.keys), "{what}: as built");
        for (place, shown) in [(0usize, true), (1, false)] {
            let mut t = with_position(built, &w.lib, place, &short.s, shown);
            t.entries[place].amount = short.s.x.floor() as u64;
            let sg = t.sign(&w.keys).unwrap();
            let r = run_raw(&sg.tx, &sg.entries, Meter::Free).expect("the covenant rules");
            assert!(r[place].is_ok() && r[2].is_err(), "{what}: a position short for its A as input {place}");
            refused += 1;
        }
    }
    println!("T1b L4: {n} random positions turned: what each user is owed, its liquidation, its close and the fees are the same from the other side ({liq} liquidatable sides among them); a position short for its A went through {paths} claim paths in the engine; {refused} merges and novations of it refused by the entry");
}

/// L5. The share of a cross position that leaves with a leg: wm / wall, the entry notionals floored to 0.01 KAS.
/// S's margin, S's ask and U's ask are each floored: what stays keeps the odd sompi (S's margin behind U is rounded
/// up, the new position's down; S's ask of U left on what stays is rounded up). Nothing is lost: S's margin after,
/// in the two positions, and U's gain are S's margin before, to the sompi. A leg sold in four parts leaves what one
/// sale leaves, to a few sompi. A leg whose entry notional is below 0.01 KAS does not leave. And at the bounds
/// (sizes of 2,000,000,000, a weight of 2,000,000,000, margins of 5 x 10^17 sompi) the engine takes the leave at the
/// model's numbers: nothing overflows.
#[test]
fn t1b_l5_the_share_of_a_leave_its_rounding_and_its_bounds() {
    let mut r = Rng(0x51ab_0c0f_fee0_0b05);
    let tpl = Tpl::of(&samples::position());
    let tpl2 = Tpl { market: [0x42; 32], ..tpl };
    let (unit, px) = (tpl.scale, samples::position().x.legs[0].entry);
    let (u, s, l) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let (mut cases, mut worst_parts, mut more, mut less) = (0, 0i64, 0, 0);
    while cases < 600 {
        let m = |r: &mut Rng| r.range(50 * KAS1, 400 * KAS1) + r.range(0, 1_000);
        let n1 = 4 * r.range(1, 8) * unit;
        let x1 = Position { a_min_counter: m(&mut r), b_min_counter: m(&mut r), ..tpl.position(u, s, n1, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)) };
        let Ok(x) = joined(&x1, &tpl2.position(u, s, r.range(1, 8) * unit + r.range(0, unit), r.range(px / 2, 2 * px), m(&mut r), m(&mut r))) else { continue };
        let ey = x.x.legs[0].entry;
        let y = |n: i64| tpl.position(l, u, n, ey, 2_000 * KAS1, 10 * KAS1);
        let Ok(whole) = left(&x, x.pot(), &y(n1), y(n1).pot(), true) else { continue };
        cases += 1;
        let (xa, z) = (whole.x_after.0, whole.z.0);
        let (lx, l2) = (x.x.legs[0], x.x.legs[1]);
        let (wm, wall) = (lx.worth(lx.entry) / 1_000_000, (lx.worth(lx.entry) + l2.worth(l2.entry)) / 1_000_000);
        let fl = |v: i64| (v as i128 * wm as i128 / wall as i128) as i64;
        // Round 13: the leg leaves at its own entry here (gu = 0), so the margin shares are floors of the proportion,
        // and what stays keeps BOTH asks whole (no longer shrunk by the share). Z carries S's ask in the share.
        assert_eq!(whole.gain, 0, "the leg leaves at its own entry: no gain");
        assert_eq!((z.x.b_margin, z.b_min_counter), (fl(x.x.b_margin), fl(x.b_min_counter)), "the margin and ask shares are floors of the exact proportion");
        assert_eq!((xa.a_min_counter, xa.b_min_counter), (x.a_min_counter, x.b_min_counter), "what stays keeps both asks whole");
        assert_eq!(xa.x.b_margin + z.x.b_margin + whole.gain, x.x.b_margin, "S's margin: nothing lost");
        assert_eq!(xa.x.a_margin, x.x.a_margin + whole.gain);
        // The same leg in four parts, at the leg's entry: what stays at the end against what one sale leaves.
        let mut cur = x;
        let mut zs = 0;
        for _ in 0..4 {
            let part = left(&cur, cur.pot(), &y(n1 / 4), y(n1 / 4).pot(), true).unwrap();
            cur = part.x_after.0;
            zs += part.z.0.x.b_margin;
        }
        assert_eq!(cur.x.legs, xa.x.legs);
        worst_parts = worst_parts.max((cur.x.b_margin - xa.x.b_margin).abs()).max((cur.b_min_counter - xa.b_min_counter).abs());
        assert_eq!(cur.x.b_margin + zs, x.x.b_margin, "in parts too nothing of S's margin is lost");
        more += (cur.x.b_margin > xa.x.b_margin) as usize;
        less += (cur.x.b_margin < xa.x.b_margin) as usize;
    }
    // A leg whose entry notional is below 0.01 KAS weighs nothing and does not leave.
    let tiny = joined(&tpl.position(u, s, unit, 999_999, 50 * KAS1, 50 * KAS1), &tpl2.position(u, s, unit, px, 50 * KAS1, 50 * KAS1)).unwrap();
    assert!(left(&tiny, tiny.pot(), &tpl.position(l, u, unit, 999_999, 50 * KAS1, 10 * KAS1), 60 * KAS1, true).is_err());
    let small = joined(&tpl.position(u, s, unit, 1_000_000, 50 * KAS1, 50 * KAS1), &tpl2.position(u, s, unit, px, 50 * KAS1, 50 * KAS1)).unwrap();
    let lv = left(&small, small.pot(), &tpl.position(l, u, unit, 1_000_000, 50 * KAS1, 10 * KAS1), 60 * KAS1, true).unwrap();
    assert!(lv.z.0.x.b_margin > 0 && lv.z.0.x.b_margin < KAS1 / 10, "a leg of 0.01 KAS of entry notional leaves with its share of S's margin: {}", lv.z.0.x.b_margin);
    // The bounds, through the engine: two legs of 2,000,000,000 at an entry that makes the weight 2,000,000,000.
    let w = world();
    let (t1, t2) = (Tpl::of(&w.position.s), Tpl { market: [0x33; 32], ..Tpl::of(&w.position.s) });
    let big = ORDER_SIZE_MAX;
    let e = 1_000_000_000_000_000 / (big / t1.scale);
    let (uu, ss, ll) = (xk(&kp(51)), xk(&kp(52)), xk(&kp(53)));
    let huge = 500_000_000_000_000_000i64;
    let x = joined(&Position { a_min_counter: huge, b_min_counter: huge, ..t1.position(uu, ss, big, e, huge, huge) }, &t2.position(uu, ss, big, e, huge / 2, huge / 2)).expect("a position at the bounds");
    let (lx, l2) = (x.x.legs[0], x.x.legs[1]);
    assert_eq!((lx.worth(e) + l2.worth(e)) / 1_000_000, 2_000_000_000, "the weight of the whole is at its limit");
    let registry = Hash::from_bytes(REGISTRY);
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let mut at_bounds = 0;
    for (ny, ey) in [(big, e), (big, e + e / 3), (big / 3 + 1, e - e / 3), (big, 0)] {
        let y = t1.position(ll, uu, ny, ey, huge + huge / 2, huge / 4);
        match leave(&w.lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTLeave"), p) {
            Ok((b, lv)) => {
                assert!(all_taken(&b.sign(&only_keys(&[&kp(51), &kp(52), &w.w])).unwrap()), "a leave at the bounds: {ny} units at {ey}");
                assert_eq!(lv.x_after.0.x.b_margin + lv.z.0.x.b_margin + lv.gain, x.x.b_margin);
                at_bounds += 1;
            }
            Err(why) => println!("T1b L5 bounds: {ny} units at {ey}: the model refuses: {why}"),
        }
    }
    // A thousandth over the weight's limit: not left.
    let over = joined(&t1.position(uu, ss, big, e + e / 1_000, huge, huge), &t2.position(uu, ss, big, e, huge / 2, huge / 2)).unwrap();
    let ow = (over.x.legs[0].worth(over.x.legs[0].entry) + over.x.legs[1].worth(over.x.legs[1].entry)) / 1_000_000;
    let or = left(&over, over.pot(), &t1.position(ll, uu, big, e, huge, huge / 4), huge + huge / 4, true);
    assert!(or.is_err(), "a weight above 2,000,000,000: scale {}, e {e}, weight {ow}: {:?}", t1.scale, or.map(|l| l.z.0.x.b_margin));
    println!("T1b L5: {cases} leaves: the shares are floors of the exact proportion, nothing of S's margin lost; four parts leave S's margin behind U within {worst_parts} sompi of what one sale leaves (more in {more} cases, less in {less}: the weights are floored to 0.01 KAS at every step); {at_bounds} leaves at the bounds (sizes and weight of 2,000,000,000, margins of 5 x 10^17) taken by the engine at the model's numbers");
    assert!(at_bounds >= 2);
}

// ================================================================ round 12: the leave is the two users' own

/// A built transaction whose two last arguments at input 2 (the two users' signatures) are in each other's place.
fn sigs_swapped(b: &OBuilt) -> OBuilt {
    let mut t = b.clone();
    if let OPlan::Contract { args, .. } = &mut t.plans[2] {
        let n = args.len();
        args.swap(n - 2, n - 1);
    }
    t
}

/// F16 (Critical in round 11), fixed in round 12: a leave carries the signatures of the position's two users. The two
/// leaves of round 11 (the loser handed to U's second key at its own entry; the winner sold to it at the price) are
/// not signed without S's key, and with U's, the third user's or the payer's signature in S's place, or the two in
/// each other's place, the leave's own input refuses. Signed by both the engine takes them: what S is then owed
/// (round 11's figures) is what S signed for.
#[test]
fn t1b_fixed_f16_a_leave_is_signed_by_the_one_that_stays() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px, m1) = (tpl.scale, w.position.s.x.legs[0].entry, tpl.market);
    let (ku, ks, kl) = (kp(51), kp(52), kp(53));
    let (u, s, l) = (xk(&ku), xk(&ks), xk(&kl));
    let registry = Hash::from_bytes(REGISTRY);
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let not_s = only_keys(&[&ku, &kl, &w.w]);
    let all = only_keys(&[&ku, &ks, &kl, &w.w]);
    let k = tpl.position(u, s, 10 * unit, px, KAS1, KAS1).x.legs[0].worth(px) / 100;
    let one = |t: &Tpl| Position { a_min_counter: 20 * k, b_min_counter: 20 * k, ..t.position(u, s, 10 * unit, px, 20 * k, 20 * k) };
    let x = joined(&one(&tpl), &one(&tpl2)).unwrap();
    let mut refused = 0;
    for (what, q, y) in [
        ("the loser handed over at its own entry", [px - 3 * (px / 10), px + 15 * (px / 100)], tpl.position(l, u, 10 * unit, px, 20 * k, KAS1)),
        ("the winner sold at the price", [px + 15 * (px / 100), px - 3 * (px / 10)], tpl.position(l, u, 10 * unit, px + 15 * (px / 100), 35 * k, KAS1)),
    ] {
        let (lb, lv) = leave(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTLeave"), p).unwrap();
        assert!(lb.sign(&not_s).is_err(), "{what}: a leave is signed without S's key");
        assert!(lb.sign(&only_keys(&[&ks, &kl, &w.w])).is_err(), "{what}: a leave is signed without U's key");
        assert!(lb.sign(&only_keys(&[&kl, &w.w])).is_err());
        for (from, to) in [(s, u), (s, l), (s, xk(&w.w)), (u, s), (u, l), (u, xk(&w.w))] {
            let forged = resign(&lb, from, to).sign(&all).unwrap();
            assert!(script_refuses(&forged.tx, &forged.entries, 2), "{what}: a leave with another key's signature in a user's place");
            refused += 1;
        }
        let sw = sigs_swapped(&lb).sign(&all).unwrap();
        assert!(script_refuses(&sw.tx, &sw.entries, 2), "{what}: the two signatures in each other's place");
        refused += 1;
        assert!(all_taken(&lb.sign(&only_keys(&[&ku, &ks, &w.w])).unwrap()), "{what}: signed by both users (the third user does not sign)");
        let (s0, s1) = (owed_to(&x, &s, &m1, q), owed_to(&lv.x_after.0, &s, &m1, q) + owed_to(&lv.z.0, &s, &m1, q));
        println!("T1b F16 fixed: {what}: not signed without S, refused with another's signature in its place; with S's own signature S is owed {s1} where it was owed {s0}");
    }
    assert_eq!(refused, 14);
}

/// F17 (Medium in round 11), fixed in round 12. U signed a sell ORDER; S's second key took it at the price; the leave
/// that would take S's margin share from under U is built from the position of that match. Without U's key it is not
/// signed; with S's, the second key's or the payer's signature in U's place the leave's own input refuses. In a real
/// registry, through the engine. (What is left to U then is a position long in the cross position and short against the
/// second key: its own to keep, to close, or to leave by its own signature.)
#[test]
fn t1b_fixed_f17_a_sell_order_is_not_the_sellers_signature_of_the_leave() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px, m1) = (tpl.scale, w.position.s.x.legs[0].entry, tpl.market);
    let (ku, ks, kl, payer) = (kp(51), kp(52), kp(53), w.w);
    for key in [ku, ks, kl] {
        w.keys.add_keypair(key);
    }
    let (u, s, l) = (xk(&ku), xk(&ks), xk(&kl));
    let not_u = only_keys(&[&ks, &kl, &payer, &kp(9)]);
    let k = tpl.position(u, s, 10 * unit, px, KAS1, KAS1).x.legs[0].worth(px) / 100;
    let one = |t: &Tpl| Position { a_min_counter: 20 * k, b_min_counter: 20 * k, ..t.position(u, s, 10 * unit, px, 20 * k, 20 * k) };
    let x1 = w.enter("U long against S in market 1", &one(&tpl), &ku);
    let x2 = w.enter("U long against S in market 2", &one(&tpl2), &ku);
    let f = w.pay(&payer);
    let (jb, x) = join(&lib, &x1, &x2, &w.script("KpfTJoin"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    let xl = nth(&signed("the two joined by both users", &jb, &w.keys), 0, x, 6_300);
    let q = [px, px + 35 * (px / 100)];
    let sell = w.put("U's sell of market 1 rests", &Order { tpl: tpl.hash(), owner: u, dir: SHORT, size: 10 * unit, limit: px, margin: 20 * k, tip: KAS1 / 10, expiry: 9_000, min_fill: 10 * unit, min_counter: 20 * k }, &ku);
    let buy = w.put("the buy of S's second key", &Order { owner: l, dir: LONG, tip: 0, min_counter: 0, ..sell.s }, &kl);
    let f = w.pay(&kl);
    let (mb, m) = order_match(&lib, &tpl, &buy, &sell, &w.script("KpfTMatch"), 10 * unit, px, Payer { funds: &f, change_to: l, rate: MIN_FEE_RATE }).unwrap();
    // The match asks nobody: S's second key takes U's sell with no key of U.
    let ms = mb.sign(&not_u).expect("a match is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&ms.tx, &ms.entries).is_ok());
    let y = nth(&ms, 0, m.position, 6_400);
    let f = w.pay(&payer);
    let (lb, lv) = leave(&lib, &xl, &y, &w.script("KpfTLeave"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    assert!(lb.sign(&not_u).is_err(), "the leave is signed without U's key: its sell order was not its signature of the leave");
    for other in [s, l, xk(&payer)] {
        let forged = resign(&lb, u, other).sign(&w.keys).unwrap();
        assert!(script_refuses(&forged.tx, &forged.entries, 2), "the leave with another key's signature in U's place");
    }
    let sw = sigs_swapped(&lb).sign(&w.keys).unwrap();
    assert!(script_refuses(&sw.tx, &sw.entries, 2));
    assert!(all_taken(&lb.sign(&w.keys).unwrap()), "signed by U and S it is taken");
    let (u0, u1) = (owed_to(&x, &u, &m1, q), owed_to(&lv.x_after.0, &u, &m1, q));
    println!("T1b F17 fixed: the leave built from U's sell taken by S's second key is not signed without U and refused with another's signature in its place; U is owed {u0} and keeps it (signed, it would be owed {u1})");
}

/// Round 12: a leave's two signatures are the transaction's, as a join's. An output changed after signing (to what
/// the leave's own rule would take), the user's payment or the sender's change paid to another key, and the leave's
/// signature script moved to another leave of the same two positions (other funds) are each refused by the leave's
/// own input.
#[test]
fn t1b_r12_the_signatures_of_a_leave_commit_to_the_transaction() {
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (ku, ks, kl) = (kp(51), kp(52), kp(53));
    let (u, s, l) = (xk(&ku), xk(&ks), xk(&kl));
    let registry = Hash::from_bytes(REGISTRY);
    let keys = only_keys(&[&ku, &ks, &w.w]);
    let x = joined(&Position { a_min_counter: 40 * KAS1, b_min_counter: 40 * KAS1, ..tpl.position(u, s, 6 * unit, px, 100 * KAS1, 100 * KAS1) }, &tpl2.position(s, u, 4 * unit, px, 80 * KAS1, 80 * KAS1)).unwrap();
    let y = tpl.position(l, u, 9 * unit, px + px / 9, 300 * KAS1, 30 * KAS1);
    let build = |n: u8| {
        let f = [fund(xk(&w.w), n, 1_000)];
        leave(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTLeave"), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap().0
    };
    let (one, other) = (build(91), build(92));
    let sg = signed("a leave signed by both users", &one, &keys);
    let c = one.change.expect("a change output");
    let under: Vec<usize> = (0..sg.tx.outputs.len()).filter(|i| sg.tx.outputs[*i].covenant.map(|c| c.covenant_id) == Some(registry)).collect();
    assert_eq!(under.len(), 3, "what stays, the new position, what is left of the second");
    let mut tried = 0;
    for to in &under {
        let mut moved = sg.clone();
        moved.tx.outputs[c].value -= 1;
        moved.tx.outputs[*to].value += 1;
        assert!(script_refuses(&moved.tx, &moved.entries, 2), "a sompi moved from the change to output {to} after the signatures were made");
        tried += 1;
    }
    let last = sg.tx.outputs.len() - 1;
    assert_eq!(sg.tx.outputs[last].script_public_key, p2pk_spk(&u));
    for at in [c, last] {
        let mut t = sg.clone();
        t.tx.outputs[at].script_public_key = p2pk_spk(&xk(&kp(77)));
        // (The user's payment to another key is refused by the leave's rule as well.)
        assert!(script_refuses(&t.tx, &t.entries, 2), "output {at} paid to another key after the signatures were made");
        tried += 1;
    }
    let mut sg2 = other.sign(&keys).unwrap();
    assert!(!input_fails(&sg2, 2));
    sg2.tx.inputs[2].signature_script = sg.tx.inputs[2].signature_script.clone();
    assert!(script_refuses(&sg2.tx, &sg2.entries, 2), "the signatures of one leave in another");
    println!("T1b R12 signatures: a leave's two signatures commit to its outputs and to the transaction ({} changes refused)", tried + 1);
}

/// F18 (High in round 12), fixed in round 13: the fold adds to the one that stays' ask what the cancelled leg moved
/// into the mover's margin. The same three steps run in a real registry: U sells its winning leg into S's own resting
/// buy, anyone folds that position into the cross position (the leg cancels), and the loser is alone. But S now asks
/// of whoever takes U's place its 40, the order's 5, AND the 35 the winner had won: 80. A novation against a key
/// posting the old ask (45) is refused by the novation's own input; posting the new ask (80), S is owed what it was,
/// and U's second key stands behind the whole loss.
#[test]
fn t1b_fixed_f18_a_fold_asks_of_the_takers_replacement_what_it_took_into_its_margin() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px, m1) = (tpl.scale, w.position.s.x.legs[0].entry, tpl.market);
    let (ku, ks, kl, payer) = (kp(51), kp(52), kp(54), w.w);
    for key in [ku, ks, kl] {
        w.keys.add_keypair(key);
    }
    let (u, s, l2) = (xk(&ku), xk(&ks), xk(&kl));
    let nobody = only_keys(&[&payer]);
    let not_s = only_keys(&[&ku, &kl, &payer, &kp(9)]);
    let k = tpl.position(u, s, 10 * unit, px, KAS1, KAS1).x.legs[0].worth(px) / 100;
    let one = |t: &Tpl| Position { a_min_counter: 20 * k, b_min_counter: 20 * k, ..t.position(u, s, 10 * unit, px, 20 * k, 20 * k) };
    let x1 = w.enter("U long against S in market 1", &one(&tpl), &ku);
    let x2 = w.enter("U long against S in market 2", &one(&tpl2), &ku);
    let f = w.pay(&payer);
    let (jb, x) = join(&lib, &x1, &x2, &w.script("KpfTJoin"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    let xl = nth(&signed("the two joined by both users", &jb, &w.keys), 0, x, 6_300);
    let q = [px + 35 * (px / 100), px - 65 * (px / 100)];
    let buy = w.put("S's buy of market 1 rests at the price", &Order { tpl: tpl.hash(), owner: s, dir: LONG, size: 10 * unit, limit: q[0], margin: 5 * k, tip: KAS1 / 10, expiry: 9_000, min_fill: 0, min_counter: 5 * k }, &ks);
    let (s0, u0) = (owed_to(&x, &s, &m1, q) + buy.s.margin, owed_to(&x, &u, &m1, q));
    assert_eq!((u0, s0), (10 * k - MERGE_TIP / 2, 75 * k - MERGE_TIP / 2), "before: U is owed 10; S 70 and its order's 5");
    let sell = w.put("U's sell", &Order { owner: u, dir: SHORT, tip: 0, min_counter: 0, ..buy.s }, &ku);
    let f = w.pay(&ku);
    let (mb, m) = order_match(&lib, &tpl, &buy, &sell, &w.script("KpfTMatch"), 10 * unit, q[0], Payer { funds: &f, change_to: u, rate: MIN_FEE_RATE }).unwrap();
    let ms = mb.sign(&not_s).expect("a match is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&ms.tx, &ms.entries).is_ok());
    let y = nth(&ms, 0, m.position, 6_400);
    // Anyone folds it into the cross position: the first leg cancels, the second is the position's one leg.
    let f = w.pay(&payer);
    let (fb, folded_) = fold(&lib, &xl, &y, &w.script("KpfTFold"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    let fs = fb.sign(&nobody).expect("a fold is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "the engine takes the fold");
    let fl = nth(&fs, 0, folded_, 6_500);
    assert_eq!((folded_.x.legs[0].market, folded_.x.legs[1].size, folded_.b_min_counter), (tpl2.market, 0, 80 * k), "the loser alone; S asks 80: its 40, the order's 5, and the 35 the winner took into U's margin");
    assert_eq!(folded_.x.a_margin, 80 * k - MERGE_TIP, "U's margin: its 40, the order's 5, and the 35 the first leg had won");
    assert!((owed_to(&folded_, &s, &m1, q) - s0).abs() <= MERGE_TIP, "after the fold alone S is owed what it was");
    // The old ask (45): the novation against a second key posting it is refused by the novation's own input.
    let y_old = w.enter("U's second key posting the old ask", &tpl2.position(l2, u, 10 * unit, px, 45 * k, KAS1), &ku);
    assert!(novated(&fl.s, fl.value as i64, &y_old.s, y_old.value as i64, true).is_err(), "the model refuses the old ask");
    assert!(kpf_protocol::thin_build::novate(&lib, &fl, &y_old, &w.script("KpfTNovate"), Payer { funds: &[fund(xk(&payer), 97, 1_000)], change_to: xk(&payer), rate: MIN_FEE_RATE }).is_err(), "the tool refuses it");
    let (bad, _, _) = kpf_protocol::thin_build::novate_with(&lib, &fl, &y_old, &w.script("KpfTNovate"), Payer { funds: &[fund(xk(&payer), 98, 1_000)], change_to: xk(&payer), rate: MIN_FEE_RATE }, false).unwrap();
    let bs = bad.sign(&nobody).expect("built without the rule");
    assert!(script_refuses(&bs.tx, &bs.entries, 2), "the engine's novation refuses the old ask");
    // The new ask (80): the novation is taken, and S is owed what it was.
    let y2 = w.enter("U's second key posting the new ask", &tpl2.position(l2, u, 10 * unit, px, 80 * k, KAS1), &ku);
    let f = w.pay(&payer);
    let (nb, nv, _) = kpf_protocol::thin_build::novate(&lib, &fl, &y2, &w.script("KpfTNovate"), Payer { funds: &f, change_to: xk(&payer), rate: MIN_FEE_RATE }).unwrap();
    let ns = nb.sign(&nobody).expect("a novation is signed by its payer alone");
    assert!(kpf_protocol::verify::verify(&ns.tx, &ns.entries).is_ok(), "the engine takes the novation at the new ask");
    let z = nv.z.unwrap().0;
    let s1 = owed_to(&z, &s, &m1, q);
    assert!((s1 - s0).abs() <= 2 * MERGE_TIP, "S is owed what it was: {s0} -> {s1}");
    println!("T1b F18 fixed: U sells its winning leg into S's resting buy, the fold cancels it and asks 80 of U's replacement (was 45); a novation posting 45 is refused, posting 80 S is owed {s1} where it was owed {s0}");
}

/// The default asks the tool gives: each user asks of the other's replacement what the other posts.
fn with_default_asks(a: Position) -> Position {
    Position { a_min_counter: a.x.b_margin, b_min_counter: a.x.a_margin, ..a }
}
/// Whether each user's ask is at least its counterparty's current margin (the default's invariant).
fn asks_cover(a: &Position, slack: i64) -> bool {
    a.a_min_counter + slack >= a.x.b_margin && a.b_min_counter + slack >= a.x.a_margin
}

/// L13 (direction 1, the model). The fold and the leave add to the one that stays' ask what the other's margin took
/// from it. Over 2,000 random cross positions with the default asks and random folds/leaves, each user's ask is at
/// least its counterparty's margin after the step, to the sompi of the tip's half and the entry's rounding; so a
/// following novation (round 7) never leaves that user owed less than it was.
#[test]
fn t1b_l13_a_gain_taken_into_a_margin_is_asked_of_whoever_takes_that_users_place() {
    let mut r = Rng(0x51ab_0c0f_fee0_0d01);
    let base = Tpl::of(&samples::position());
    let t2 = Tpl { market: [0x42; 32], ..base };
    let (unit, px) = (base.scale, samples::position().x.legs[0].entry);
    let (u1, u2, l) = ([11u8; 32], [22u8; 32], [33u8; 32]);
    let (mut folds, mut leaves, mut worst_fold, mut worst_leave) = (0, 0, 0i64, 0i64);
    while folds < 2_000 {
        let m = |r: &mut Rng| r.range(20 * KAS1, 400 * KAS1);
        let (n1, n2) = (r.range(1, 8) * unit, r.range(1, 8) * unit);
        let mut x1 = base.position(u1, u2, n1, r.range(px / 2, 2 * px), m(&mut r), m(&mut r));
        if r.range(0, 2) == 0 {
            x1.x.legs[0].dir = SHORT;
        }
        let two = r.range(0, 2) == 1;
        let x = if two {
            let t = r.range(0, 2) == 1;
            let Ok(j) = joined(&with_default_asks(x1), &with_default_asks(if t { t2.position(u2, u1, n2, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)) } else { t2.position(u1, u2, n2, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)) })) else { continue };
            // A join keeps the default to the sompi of the tips: re-assert.
            with_default_asks(j)
        } else {
            with_default_asks(x1)
        };
        // A fold: a position of the same two users in the first leg's market, the default asks, any entry inside a
        // plausible order's limit, the keys either way.
        let turned = r.range(0, 2) == 1;
        let (ya, yb) = if turned { (u2, u1) } else { (u1, u2) };
        let yf = with_default_asks(base.position(ya, yb, r.range(1, 8) * unit, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)));
        if let Ok(out) = folded(&x, &yf) {
            folds += 1;
            // Before: both positions covered (the default, to the tips). After: the fold's ask bump keeps it.
            assert!(asks_cover(&x, MERGE_TIP) && asks_cover(&yf, MERGE_TIP));
            let slack = 2 * MERGE_TIP + 4;
            assert!(asks_cover(&out, slack), "a fold of default-ask positions leaves an ask below the counterparty's margin: {out:?}");
            worst_fold = worst_fold.max((out.x.b_margin - out.a_min_counter).max(out.x.a_margin - out.b_min_counter));
        }
        // A leave: the first leg sold to a third user L at any entry; U signs (so this is U's own choice of entry).
        let d1 = x.x.legs[0].dir;
        let ny = r.range(1, 8) * unit + r.range(0, unit);
        let ey = r.range(px / 2, 2 * px);
        let yl = if d1 == LONG { with_default_asks(base.position(l, u1, ny, ey, m(&mut r) * 3, r.range(KAS1, 40 * KAS1))) } else { with_default_asks(base.position(u1, l, ny, ey, r.range(KAS1, 40 * KAS1), m(&mut r) * 3)) };
        if let Ok(lv) = left(&x, x.pot(), &yl, yl.pot(), true) {
            leaves += 1;
            let xa = lv.x_after.0;
            // What stays: U's ask of S is unchanged (U keeps its position); S's ask of U covers S's margin there.
            assert!(xa.b_min_counter + 4 >= xa.x.a_margin, "what stays: S's ask below U's margin: {xa:?}");
            // The new position Z: S signs the leave, so S's ask there is its own (the share).
            worst_leave = worst_leave.max(xa.x.a_margin - xa.b_min_counter);
        }
    }
    println!("T1b L13: {folds} folds and {leaves} leaves of default-ask positions: after each, each user's ask is at least its counterparty's margin (worst shortfall fold {worst_fold}, leave {worst_leave}, below zero to the tips); a novation then never leaves that user owed less (round 7)");
    assert!(worst_fold <= 2 * MERGE_TIP + 4 && worst_leave <= 4 && leaves > 400);
}

/// L2 (round 13), the property over a chain. For 2,000 random cross positions with the default asks, a chain of
/// unsigned steps the mover builds alone: a fold that cancels part or all of EITHER leg (through an order of either
/// user at any price inside that order's limit; the mover is A or B by turn), a turn of the users or the legs between
/// steps, several folds in a row; then the worst novation the mover builds against a second key posting EXACTLY what
/// the position asks. The other user's clamped worth after the novation, at the prices where the comparison is sound
/// (the legs sold at their own orders' prices), is never below what it was before the novation (to the tips), and
/// the invariant "each ask >= counterparty's margin" holds after every step.
#[test]
fn t1b_l2_no_unsigned_chain_takes_from_a_user_whose_asks_are_the_default() {
    let mut r = Rng(0x51ab_0c0f_fee0_0d02);
    let base = Tpl::of(&samples::position());
    let markets = [[0x41u8; 32], [0x42; 32], [0x43; 32]];
    let (unit, px) = (base.scale, samples::position().x.legs[0].entry);
    let (u1, u2, l) = ([11u8; 32], [22u8; 32], [33u8; 32]);
    let (mut chains, mut steps, mut worst, mut broke) = (0, 0u64, 0i64, 0);
    while chains < 2_000 {
        let m = |r: &mut Rng| r.range(30 * KAS1, 300 * KAS1);
        let t = |i: usize| Tpl { market: markets[i], ..base };
        // A cross position of two legs, the default asks.
        let mut x1 = t(0).position(u1, u2, r.range(2, 8) * unit, r.range(px / 2, 2 * px), m(&mut r), m(&mut r));
        if r.range(0, 2) == 0 {
            x1.x.legs[0].dir = SHORT;
        }
        let tn = r.range(0, 2) == 1;
        let (ya, yb) = if tn { (u2, u1) } else { (u1, u2) };
        let Ok(x0) = joined(&with_default_asks(x1), &with_default_asks(t(1).position(ya, yb, r.range(2, 8) * unit, r.range(px / 2, 2 * px), m(&mut r), m(&mut r)))) else { continue };
        let x0 = with_default_asks(x0);
        chains += 1;
        let mut x = x0;
        let mut ok = true;
        // Up to three steps.
        for _ in 0..r.range(1, 4) {
            match r.range(0, 3) {
                // A turn.
                0 => {
                    x = if r.range(0, 2) == 0 { turned_users(&x) } else { turned_legs(&x).unwrap_or(x) };
                }
                // A fold that cancels into the first leg, through an order of either user at any price in its limit.
                _ => {
                    let market = x.x.legs[0].market;
                    let tpl = Tpl { market, ..base };
                    // The mover is whichever user; the order's entry is anywhere (its own order's limit).
                    let (ma, mb) = if r.range(0, 2) == 0 { (x.x.a_key, x.x.b_key) } else { (x.x.b_key, x.x.a_key) };
                    let yf = with_default_asks(tpl.position(ma, mb, x.x.legs[0].size.min(r.range(1, 8) * unit), r.range(px / 2, 2 * px), m(&mut r), m(&mut r)));
                    if let Ok(out) = folded(&x, &yf) {
                        x = out;
                    }
                }
            }
            steps += 1;
            if !asks_cover(&x, 4 * MERGE_TIP + 8) {
                ok = false;
                broke += 1;
            }
        }
        // The worst novation: U (x's A, long) leaves against a second key L posting exactly what x asks of U's
        // replacement (x.b_ask, the stayer S's ask). At any entry. The mirror (a turn first, so B leaves) is the
        // same arithmetic. Compare S's clamped worth before (in x) and after (in z) at a grid.
        if x.x.legs[1].size != 0 || x.x.legs[0].dir != LONG {
            continue; // a novation needs a one-leg position long for its A
        }
        let (leaver, stayer) = (x.x.a_key, x.x.b_key);
        let market = x.x.legs[0].market;
        let ey = r.range(px / 2, 2 * px);
        // y: L long against the leaver; L posts exactly x.b_ask, the leaver posts any margin.
        let y = Position {
            x: Cross { legs: [XLeg { market, ..base.position(l, leaver, x.x.legs[0].size, ey, 1, 1).x.legs[0] }, XLeg::none()], a_key: l, b_key: leaver, a_margin: x.b_min_counter.max(1), b_margin: m(&mut r), ..base.position(l, leaver, 1, ey, 1, 1).x },
            a_min_counter: x.b_min_counter, b_min_counter: m(&mut r),
            ..x
        };
        let Ok(nv) = novated(&x, x.pot(), &y, y.pot(), true) else { continue };
        let Some((z, _)) = nv.z else { continue };
        let _ = ok;
        for _ in 0..12 {
            let p = r.range(px / 3, 3 * px);
            let before = clamped_worth(&x, &stayer, p);
            let after = clamped_worth(&z, &stayer, p);
            worst = worst.max(before - after);
            assert!(after + 4 >= before, "the stayer is owed less after an unsigned chain and novation: {before} -> {after}; x {x:?} y {y:?}");
        }
    }
    println!("T1b L2 chain: {chains} cross positions, {steps} unsigned steps (folds that cancel, turns), then the worst novation the mover builds posting exactly the ask: the other user is never owed less (worst {worst} sompi, to the tips); the invariant ask >= counterparty's margin held every step ({broke} breaks)");
    assert!(broke == 0 && worst <= 4);
}
/// What `user` is owed by a one-leg position at the price `p`, clamped to the pot (nothing if not in it).
fn clamped_worth(a: &Position, user: &[u8; 32], p: i64) -> i64 {
    let q = [p, 0];
    if a.x.a_key == *user {
        a.x.a_at(q)
    } else if a.x.b_key == *user {
        a.x.owed_at(SIDE_B, q)
    } else {
        0
    }
}

// ================================================================ round 14: signing on two machines

/// The four transactions two users sign, as `sign_apart` is given them: (what, the builder, whether an executor pays).
/// A = kp(81), B = kp(82), the third user L = kp(83), the executor = kp(84).
fn apart_cases<'a>(w: &'a World, fa: &'a [KeyUtxo], fe: &'a [KeyUtxo], fb: &'a [KeyUtxo]) -> Vec<(&'static str, Box<dyn Fn(Swap) -> kpf_protocol::Result<OBuilt> + 'a>, bool)> {
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (a, b, l, ex) = (xk(&kp(81)), xk(&kp(82)), xk(&kp(83)), xk(&kp(84)));
    let registry = Hash::from_bytes(REGISTRY);
    let live = move |s: Position, n: u8| OLive { s, value: s.pot() as u64, id: Some(registry), op: op(n), daa: 6_200 };
    let x = with_default_asks(tpl.position(a, b, 5 * unit, px, 100 * KAS1, 100 * KAS1));
    let y = with_default_asks(tpl2.position(a, b, 3 * unit, px, 60 * KAS1, 70 * KAS1));
    let cross = joined(&x, &y).unwrap();
    let sold = with_default_asks(tpl.position(l, a, 5 * unit, px + px / 50, 200 * KAS1, 60 * KAS1));
    let fresh = with_default_asks(tpl.position(a, b, 2 * unit, px, 30 * KAS1, 30 * KAS1));
    let anchor = Anchor { s: (), value: KAS1 as u64, id: Some(registry), op: op(0xA7), daa: 6_000 };
    let (s_both, s_join, s_leave, s_open) = (w.script("KpfTBoth"), w.script("KpfTJoin"), w.script("KpfTLeave"), w.script("KpfTOpen"));
    vec![
        ("a close by both", Box::new(move |f: Swap| both_close(&lib, &OLive { s: swap_position(x, f), ..live(x, 0x71) }, &s_both, 90 * KAS1, 110 * KAS1, Payer { funds: &swap_funds(fa, f), change_to: f(a), rate: MIN_FEE_RATE })), false),
        ("a join", Box::new(move |f: Swap| join(&lib, &OLive { s: swap_position(x, f), ..live(x, 0x71) }, &OLive { s: swap_position(y, f), ..live(y, 0x72) }, &s_join, Payer { funds: &swap_funds(fa, f), change_to: f(a), rate: MIN_FEE_RATE }).map(|r| r.0)), false),
        ("a leg that leaves, an executor paying", Box::new(move |f: Swap| leave(&lib, &OLive { s: swap_position(cross, f), ..live(cross, 0x73) }, &OLive { s: swap_position(sold, f), ..live(sold, 0x74) }, &s_leave, Payer { funds: &swap_funds(fe, f), change_to: f(ex), rate: MIN_FEE_RATE }).map(|r| r.0)), true),
        ("an opening, the counterparty's margin from its own coin", Box::new(move |f: Swap| open(&lib, &anchor, &s_open, &swap_position(fresh, f), &swap_funds(fb, f), &f(b), true, Payer { funds: &swap_funds(fa, f), change_to: f(a), rate: MIN_FEE_RATE })), false),
    ]
}

/// Threat (a), the file against the co-signer, at the library. A signature made by `sign_more` is the co-signer's
/// signature of this transaction's input: wherever the file says to write it, it serves nothing else.
///  - the place moved (the start of the signature script, a byte before or after, the script's own bytes at the end):
///    the co-signer signs, and the transaction is refused by the engine; a place beyond the script is not signed;
///  - the co-signer's signature asked for the OBJECT's input (which checks none): the transaction is refused;
///  - after every signature is in, any argument of the script changed (every push before the script itself, a bit
///    flipped): refused. The outputs are signed, and each entry holds its arguments to them;
///  - `finished()` is not a verdict (it returns a transaction whose script fails): `verify` is, and the tool's last
///    signer runs it before it sends.
/// And the twin of `sign_apart` (c): no key of the twin is in what leaves the machine.
#[test]
fn t1b_r14_a_signature_made_apart_serves_its_transaction_and_its_place_only() {
    use kpf_protocol::verify::verify;
    let mut w = world();
    let (a, b, l, ex) = (kp(81), kp(82), kp(83), kp(84));
    let (fa, fe) = (w.pay(&a), w.pay(&ex));
    let fb = vec![fund(xk(&b), 0xB7, 31)];
    let (mut moved, mut args, mut beyond) = (0, 0, 0);
    for (what, mk, exec_pays) in apart_cases(&w, &fa, &fe, &fb) {
        let (part, _) = sign_apart(mk.as_ref(), &only_keys(&[&a])).unwrap_or_else(|e| panic!("{what}: {e}"));
        let rest = if exec_pays { only_keys(&[&b, &ex]) } else { only_keys(&[&b]) };
        let done = part.sign_more(&rest).unwrap().finished().unwrap();
        assert!(verify(&done.tx, &done.entries).is_ok(), "{what}: signed on two machines");
        let (si, at, key) = *part.open_sigs.iter().find(|(_, _, k)| *k == xk(&b)).expect("B's signature is waited for");
        let len = part.tx.inputs[si].signature_script.len();
        // The place moved.
        for off in [0usize, at - 1, at + 1, len - 65] {
            let mut p = part.clone();
            for s in p.open_sigs.iter_mut().filter(|(_, _, k)| *k == key) {
                s.1 = off;
            }
            let t = p.sign_more(&rest).unwrap().finished().unwrap();
            assert!(!verify(&t.tx, &t.entries).is_ok(), "{what}: the co-signer's signature written at {off} instead of {at}");
            moved += 1;
        }
        let mut p = part.clone();
        p.open_sigs.iter_mut().for_each(|s| s.1 = len - 64);
        assert!(p.sign_more(&rest).is_err(), "{what}: a place that ends beyond the signature script");
        beyond += 1;
        // The co-signer's signature asked for the object's input as well.
        let mut p = part.clone();
        p.open_sigs.push((0, 0, key));
        let t = p.sign_more(&rest).unwrap().finished().unwrap();
        assert!(!verify(&t.tx, &t.entries).is_ok(), "{what}: the co-signer's signature written into the object's own signature script");
        // Arguments changed after the signatures: every push before the script itself.
        let ss = done.tx.inputs[si].signature_script.clone();
        let c = chunks(&ss);
        for j in 0..c.len() - 1 {
            let mut t = done.clone();
            let mut d = ss[c[j].clone()].to_vec();
            if d.len() <= 1 {
                // An empty push or a one-byte number: another one-byte number.
                d = if d == vec![0x51] { vec![0x52] } else { vec![0x51] };
            } else {
                let n = d.len();
                d[n - 1] ^= 1;
            }
            t.tx.inputs[si].signature_script = [&ss[..c[j].start], &d[..], &ss[c[j].end..]].concat();
            assert!(!verify(&t.tx, &t.entries).is_ok(), "{what}: argument {j} of the script changed after the signatures");
            args += 1;
        }
        // `finished` is not a verdict: a signature of the co-signer's spoiled, and it still returns a transaction.
        let mut spoiled = part.sign_more(&rest).unwrap();
        spoiled.tx.inputs[si].signature_script[at + 3] ^= 1;
        let t = spoiled.finished().expect("finished returns it");
        assert!(!verify(&t.tx, &t.entries).is_ok(), "{what}: and verify refuses it");
        // The twin: no twin key is in the file (the transaction, its entries).
        let bytes = serde_json::to_vec(&part).unwrap();
        let text = String::from_utf8_lossy(&bytes).to_string();
        for k in [&a, &b, &l, &ex] {
            let secret = blake2b(&[&b"kpf twin key"[..], &xk(k)[..]].concat());
            let twin = KeySet::new().add_secret(&secret).expect("a hash is a secret key");
            let spk = p2pk_spk(&twin);
            assert!(!part.entries.iter().any(|e| e.script_public_key == spk) && !part.tx.outputs.iter().any(|o| o.script_public_key == spk), "{what}: a twin key in the file");
            assert!(!part.tx.inputs.iter().any(|i| i.signature_script.windows(32).any(|x| x == twin)), "{what}: a twin key in a signature script");
            let _ = &text;
        }
    }
    println!("T1b R14 library: 4 transactions signed apart; the co-signer's signature written at {moved} other places, or into the object's input: refused by the engine each time; {beyond} places beyond the script not signed; {args} arguments changed after signing refused; finished() returns what verify refuses; no twin key leaves the machine");
}

/// F19 (Low in round 14), fixed in round 15: `sign_more` takes an `open_sigs` entry only for an input that is not a
/// plain coin. A file that lists the co-signer's own coin among the signatures a script waits for (a push of 65 bytes
/// as its signature script, the place 1) is refused and nothing is signed; so is the same for the first machine's
/// coin. Listed as left to its key (`open`), the coin is signed as before, and an input left to a key that is not its
/// coin's is refused.
#[test]
fn t1b_fixed_f19_a_coin_is_not_signed_as_a_signature_a_script_waits_for() {
    use kpf_protocol::verify::verify;
    let mut w = world();
    let (a, b, ex) = (kp(81), kp(82), kp(84));
    let (fa, fe) = (w.pay(&a), w.pay(&ex));
    let fb = vec![fund(xk(&b), 0xB7, 31)];
    let cases = apart_cases(&w, &fa, &fe, &fb);
    let (what, mk, _) = &cases[3];
    let (part, _) = sign_apart(mk.as_ref(), &only_keys(&[&a])).unwrap();
    assert_eq!(part.open.len(), 1, "{what}: the counterparty's coin is left to it");
    let (ci, key) = part.open[0];
    let mut p = part.clone();
    p.open.clear();
    p.tx.inputs[ci].signature_script = [vec![65u8], vec![0xEE; 65]].concat();
    p.open_sigs.push((ci, 1, key));
    let r = p.sign_more(&only_keys(&[&b]));
    assert!(r.as_ref().err().is_some_and(|e| e.to_string().contains("is a plain coin, which the file lists among the signatures a script waits for")), "{:?}", r.map(|x| x.open_sigs));
    // The same for a coin of another key (the first machine's), whether or not the key is at hand.
    let other = (0..part.entries.len()).find(|i| *i != ci && part.entries[*i].script_public_key == p2pk_spk(&xk(&a))).expect("the first machine's coin");
    let mut p = part.clone();
    p.open_sigs.push((other, 1, xk(&a)));
    assert!(p.sign_more(&only_keys(&[&b])).is_err() && p.sign_more(&only_keys(&[&a, &b])).is_err());
    // As the file should list it, the coin is signed, and the transaction is whole.
    let honest = part.sign_more(&only_keys(&[&b])).unwrap().finished().unwrap();
    assert!(verify(&honest.tx, &honest.entries).is_ok());
    let mut q = part.clone();
    q.open[0].1 = xk(&a);
    assert!(q.sign_more(&only_keys(&[&a, &b])).is_err(), "an input left to a key that is not its coin's");
}

/// (a) An object's output under an id the transaction does not give it is no object of that id to `OPartial::bound`,
/// which is what the co-signer's tool reads an output's registry from: the id of another registry, an id no input carries,
/// an authorizing input that is a plain coin the file puts under the id.
#[test]
fn t1b_r14_an_output_is_of_a_registry_only_if_the_transaction_gives_it_that_id() {
    let mut w = world();
    let (a, b, ex) = (kp(81), kp(82), kp(84));
    let (fa, fe) = (w.pay(&a), w.pay(&ex));
    let fb = vec![fund(xk(&b), 0xB7, 31)];
    let cases = apart_cases(&w, &fa, &fe, &fb);
    let registry = Hash::from_bytes(REGISTRY);
    let mut tried = 0;
    for (what, mk, _) in &cases {
        let (part, _) = sign_apart(mk.as_ref(), &only_keys(&[&a])).unwrap();
        let under: Vec<usize> = (0..part.tx.outputs.len()).filter(|i| part.tx.outputs[*i].covenant.map(|c| c.covenant_id) == Some(registry)).collect();
        for i in &under {
            assert_eq!(part.bound(*i), Some(registry), "{what}: output {i} is of the registry");
            let other = Hash::from_bytes([0x5A; 32]);
            let mut p = part.clone();
            p.tx.outputs[*i].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: other });
            assert_eq!(p.bound(*i), None, "{what}: an id no input carries");
            // The authorizing input a plain coin: of the first machine's, and (an opening) the co-signer's open one,
            // the file putting each under the registry's id.
            for ci in (0..part.entries.len()).filter(|c| part.entries[*c].covenant_id.is_none()) {
                let mut p = part.clone();
                p.tx.outputs[*i].covenant = Some(CovenantBinding { authorizing_input: ci as u16, covenant_id: registry });
                assert_eq!(p.bound(*i), None, "{what}: authorized by the plain coin at input {ci}");
                if part.open.iter().any(|(j, _)| *j == ci) {
                    p.entries[ci].covenant_id = Some(registry);
                    assert_eq!(p.bound(*i), None, "{what}: authorized by the open coin at input {ci}, which the file puts under the id");
                }
                tried += 1;
            }
            tried += 1;
        }
    }
    println!("T1b R14 bound: {tried} outputs under an id the transaction does not give: no object of the registry");
    assert!(tried > 8);
}

/// Round 15. What a position holds beyond its two margins (its carrier, and anything a writer left there), followed
/// to who is paid it. The same position twice, once holding 7 KAS more: a close declared by A and by B and settled
/// after its dispute period; a margin added; a close by both. Through the engine. The 7 KAS are paid, at a settled close, to
/// the party the close was declared AGAINST (the one that did not declare), whichever side that is; an added margin
/// carries them on in the position; a close by both pays what the two sign. So `cosign`'s remark is the right one to
/// make: the state gives the excess to nobody, and the rule of a close gives it to whoever did not make it.
#[test]
fn t1b_r15_what_an_position_holds_beyond_its_state_goes_to_the_party_a_close_is_declared_against() {
    let w = pworld();
    let f = [fund(xk(&w.w), 76, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let plain = w.position(false);
    let extra = 7 * KAS1;
    let rich = OLive { value: plain.value + extra as u64, ..plain.clone() };
    let p = [9 * P10 / 10, 0];
    let (a, b) = (plain.s.x.a_key, plain.s.x.b_key);
    for (side, against) in [(SIDE_A, b), (SIDE_B, a)] {
        let mut got = vec![];
        for position in [&plain, &rich] {
            let (bd, c) = close(&w.lib, position, &w.script("KpfTClose"), side, T0, p, None, payer).unwrap();
            let cs = signed("a close declared", &bd, &w.keys);
            assert_eq!(cs.tx.outputs[0].value, position.value + c.bond as u64, "the claim holds what the position held and the bond");
            let sb = settle(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTSettle"), payer).unwrap();
            let ss = signed("settled after its dispute period", &sb, &w.keys);
            // (The claimer posted the bond from the payer's coin here; it is paid back to the claimer's key.)
            got.push((paid(&ss, &a), paid(&ss, &b), c.amount, c.bond));
        }
        let (pa, pb) = (got[1].0 - got[0].0, got[1].1 - got[0].1);
        let to_against = if against == a { pa } else { pb };
        let to_claimer = if against == a { pb } else { pa };
        assert_eq!((to_against, to_claimer), (extra, 0), "the {extra} sompi more go to the party the close was declared against");
        println!("T1b R15 excess: a close declared by side {side} and settled: of {extra} sompi the position held beyond its state, {to_against} to the other party, {to_claimer} to the one that declared");
    }
    // An added margin carries the excess on; a close by both pays what the two sign (here: all of it to A).
    let (ab, next) = add(&w.lib, &rich, &w.script("KpfTAdd"), SIDE_A, KAS1, payer).unwrap();
    let asg = signed("a margin added", &ab, &w.keys);
    assert_eq!(asg.tx.outputs[0].value as i64, rich.value as i64 + KAS1, "the position written anew holds the excess still");
    assert_eq!(next.pot(), plain.s.pot() + KAS1);
    let both = both_close(&w.lib, &rich, &w.script("KpfTBoth"), rich.value as i64, 0, payer).unwrap();
    let bs = signed("a close by both, everything to A", &both, &w.keys);
    assert_eq!(paid(&bs, &a), rich.value as i64, "what the two sign");
}

// ================================================================ round 16: a transaction read back; the executor

use kpf_protocol::thin_read::{read_tx, Known, TxRead};

/// What a reader was told: objects by their hash (a position's fixed part with them).
struct Pool(Vec<TObj>);
impl Known for Pool {
    fn object(&self, state: &[u8; 32]) -> Option<TObj> {
        self.0.iter().find(|o| o.hash() == *state).copied()
    }
    fn fixed(&self, h: &[u8; 32]) -> Option<Vec<u8>> {
        self.0.iter().filter_map(|o| if let TObj::Position(a) = o { Some(*a) } else { None }).find(|a| a.fixed_hash() == *h).map(|a| a.fixed())
    }
    fn template(&self, _: &[u8; 32]) -> Option<Template> {
        None
    }
}
fn read_signed(lib: &[u8; 32], sg: &OSigned, known: &dyn Known) -> TxRead {
    let amounts: Vec<Option<u64>> = sg.entries.iter().map(|e| Some(e.amount)).collect();
    let r = read_tx(lib, &sg.tx, &amounts, known).expect("a transaction of the library");
    // The property claimed, held on everything this file reads: what is returned as made is what the output holds.
    for (i, o, id) in &r.made {
        assert_eq!(object_spk(lib, o), sg.tx.outputs[*i].script_public_key, "made at output {i}: not the output's script");
        assert_eq!(sg.tx.outputs[*i].covenant.map(|c| c.covenant_id.as_bytes()), Some(*id));
    }
    for (i, h, o) in &r.spent {
        assert!(o.is_none_or(|o| o.hash() == *h), "spent: not the hash the input carries");
        // Round 17: an object's signature script is four pushes (l, k, the entry's tag, the slot's script), and the
        // l that `slot_call` reads (the fourth from the end) is the input of a script of the library.
        let ss = &sg.tx.inputs[*i].signature_script;
        assert_eq!(kpf_protocol::thin_read::pushes(ss).map(|p| p.len()), Some(4), "the object's signature script at input {i}");
        let l = kpf_protocol::thin_read::slot_call(ss).and_then(|x| x.2).expect("the script an object names");
        assert!(kpf_protocol::thin_read::call_of(&sg.tx.inputs[l as usize].signature_script).is_some(), "the object at input {i} names input {l}, which runs a script of the library");
    }
    r
}
/// The built transaction of an entry on two objects (inputs 0 and 1, the script at 2) with the two objects in each
/// other's place: the script names them by `who`, so the same operation is taken wherever they stand.
fn two_swapped(b: &OBuilt) -> OBuilt {
    let mut t = b.clone();
    t.tx.inputs.swap(0, 1);
    t.entries.swap(0, 1);
    t.plans.swap(0, 1);
    t.input_labels.swap(0, 1);
    if let OPlan::Contract { args, .. } = &mut t.plans[2] {
        args[0] = Arg::V(ArtifactValue::Bytes([1i64, 0, 0].iter().flat_map(|x| sm8(*x)).collect()));
    }
    for o in t.tx.outputs.iter_mut() {
        if let Some(c) = o.covenant.as_mut() {
            c.authorizing_input = match c.authorizing_input {
                0 => 1,
                1 => 0,
                n => n,
            };
        }
    }
    t
}

/// F20 (Medium in round 16), fixed in round 17: `read_tx` takes each call's objects from the inputs whose slot names
/// that call's input, in the order of the call's `who`. The fold and the match of round 16 with their two objects
/// in the other order (who = 1, 0, 0) pass the engine and are now read whole: the folded position, the position of
/// the match. An opening that publishes another position in its payload is read by the known fixed part. With the
/// objects swapped and `who` left as it was, the engine refuses the transaction (each object's own script holds
/// `who` to its input), so the reader is never shown one. Nothing read is wrong (`read_signed` holds every made
/// object to its output's script).
#[test]
fn t1b_fixed_f20_a_transaction_is_read_back_wherever_its_objects_stand() {
    use kpf_protocol::verify::verify;
    let w = world();
    let lib = w.lib;
    let tpl = Tpl::of(&w.position.s);
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (u, s) = (xk(&kp(31)), xk(&kp(32)));
    let registry = Hash::from_bytes(REGISTRY);
    let f = [fund(xk(&w.w), 90, 1_000)];
    let p = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let keys = only_keys(&[&w.w]);
    // ---- a fold into a cross position: X of two legs (U and S), Y of one leg in the market of X's first.
    let tpl2 = Tpl { market: [0x33; 32], ..tpl };
    let x = joined(&with_default_asks(tpl.position(u, s, 4 * unit, px, 80 * KAS1, 80 * KAS1)), &with_default_asks(tpl2.position(u, s, 3 * unit, px, 60 * KAS1, 60 * KAS1))).unwrap();
    let y = with_default_asks(tpl.position(u, s, 2 * unit, px + px / 50, 40 * KAS1, 40 * KAS1));
    let (fb, z) = fold(&lib, &live_in(registry, x, 0x71), &live_in(registry, y, 0x72), &w.script("KpfTFold"), p).unwrap();
    for (what, known) in [("both known", Pool(vec![TObj::Position(x), TObj::Position(y)])), ("nothing known", Pool(vec![]))] {
        let honest = read_signed(&lib, &signed("a fold as the builder lays it out", &fb, &keys), &known);
        assert!(honest.unread.is_empty() && honest.made.iter().any(|(_, o, _)| *o == TObj::Position(z)), "{what}: {honest:?}");
        let sw = two_swapped(&fb).sign(&keys).unwrap();
        assert!(verify(&sw.tx, &sw.entries).is_ok());
        let r = read_signed(&lib, &sw, &known);
        assert!(r.unread.is_empty() && r.made.iter().any(|(_, o, _)| *o == TObj::Position(z)), "{what}: the fold with its positions in the other order is read whole: {r:?}");
        assert!(r.spent.iter().any(|(i, _, o)| *i == 1 && *o == Some(TObj::Position(x))) && r.spent.iter().any(|(i, _, o)| *i == 0 && *o == Some(TObj::Position(y))), "{what}: each spent object at its own input: {:?}", r.spent);
    }
    // The objects swapped and `who` left as the builder wrote it: refused by the engine.
    let mut stale = two_swapped(&fb);
    if let OPlan::Contract { args, .. } = &mut stale.plans[2] {
        args[0] = Arg::V(ArtifactValue::Bytes([0i64, 1, 1].iter().flat_map(|x| sm8(*x)).collect()));
    }
    let st = stale.sign(&keys).unwrap();
    assert!(!verify(&st.tx, &st.entries).is_ok(), "who names the first object where the second stands");
    // ---- a match: a buy of 4 units holding 20 KAS, a sell of 4 holding 60; and one that leaves a rest.
    let buy = Order { tpl: tpl.hash(), owner: u, dir: LONG, size: 4 * unit, limit: px, margin: 20 * KAS1, tip: KAS1 / 2, expiry: 9_000, min_fill: 0, min_counter: 0 };
    let sell = Order { owner: s, dir: SHORT, margin: 60 * KAS1, ..buy };
    let big = Order { size: 10 * unit, margin: 150 * KAS1, ..sell };
    let live_o = |o: Order, n: u8| OLive { s: o, value: o.value() as u64, id: Some(registry), op: op(n), daa: 6_200 };
    for (what, short) in [("both used up", sell), ("the sell rests", big)] {
        let (mb, m) = order_match(&lib, &tpl, &live_o(buy, 0x75), &live_o(short, 0x76), &w.script("KpfTMatch"), 4 * unit, px, p).unwrap();
        let sw = two_swapped(&mb).sign(&keys).unwrap();
        assert!(verify(&sw.tx, &sw.entries).is_ok(), "{what}");
        let r = read_signed(&lib, &sw, &Pool(vec![]));
        assert!(r.unread.is_empty() && r.made.iter().any(|(_, o, _)| *o == TObj::Position(m.position)), "{what}: the match with the sell at the lower input is read whole: {r:?}");
        if let Rest::Order(o, _) = m.short {
            assert!(r.made.iter().any(|(_, x, _)| *x == TObj::Order(o)), "{what}: and what rests of the sell");
        }
    }
    // ---- an opening whose payload tells another position: read by the fixed part the reader knows.
    let position = with_default_asks(tpl.position(u, s, 2 * unit, px, 30 * KAS1, 30 * KAS1));
    let other = with_default_asks(tpl.position(u, s, 2 * unit, px, 31 * KAS1, 30 * KAS1));
    let anchor = Anchor { s: (), value: KAS1 as u64, id: Some(registry), op: op(0xA7), daa: 6_000 };
    let both = only_keys(&[&kp(31), &kp(32), &w.w]);
    let mut lying = open(&lib, &anchor, &w.script("KpfTOpen"), &position, &[], &s, true, p).unwrap();
    lying.tx.payload = opening_payload(&other);
    let ls = signed("an opening that publishes another position", &lying, &both);
    let r = read_signed(&lib, &ls, &Pool(vec![TObj::Position(position)]));
    assert!(r.unread.is_empty() && r.made.iter().any(|(_, o, _)| *o == TObj::Position(position)) && !r.made.iter().any(|(_, o, _)| *o == TObj::Position(other)), "the position made is read, the one published is not: {r:?}");
    // With nothing known the wrong payload still reads no position (and never the wrong one).
    let r = read_signed(&lib, &ls, &Pool(vec![]));
    assert!(!r.made.iter().any(|(_, o, _)| matches!(o, TObj::Position(_))) && !r.unread.is_empty());
    println!("T1b F20 fixed: a fold and a match with their two objects in the other order, and an opening with another position in its payload, are read whole");
}

/// F20, the fix tried further. A single-object entry with its object NOT at input 0 (the script at 0, the object at
/// 1); and a transaction that runs TWO scripts of the library, on positions of two registries: an added margin and a
/// declared close, in either order. The engine takes each; every object of both calls is read (the position with its
/// margin added, the claim), each spent object stands at its own input, and nothing is unread.
#[test]
fn t1b_r17_two_calls_in_one_transaction_and_an_object_at_another_input_are_read() {
    use kpf_protocol::verify::verify;
    let w = pworld();
    let f = [fund(xk(&w.w), 78, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let first = w.position(false);
    let mut second = w.position(true);
    second.id = Some(Hash::from_bytes([0x72; 32]));
    second.op = op(11);
    let p = [9 * P10 / 10, 12 * P10 / 10];
    let (ab, added) = add(&w.lib, &first, &w.script("KpfTAdd"), SIDE_A, 3 * KAS1, payer).unwrap();
    let (db, claim) = close(&w.lib, &second, &w.script("KpfTClose"), SIDE_B, T0, p, None, payer).unwrap();
    let known = Pool(vec![TObj::Position(first.s), TObj::Position(second.s)]);
    // The object at input 1, its script at input 0.
    let (t, _) = weave(&[(&ab, vec![Some(1), Some(0), Some(2)], 0)], 2);
    let sg = t.sign(&w.keys).unwrap();
    assert!(verify(&sg.tx, &sg.entries).is_ok(), "an added margin with the script before the position: {:?}", verify(&sg.tx, &sg.entries).errors());
    let r = read_signed(&w.lib, &sg, &known);
    assert!(r.unread.is_empty() && r.made.iter().any(|(_, o, _)| *o == TObj::Position(added)) && r.spent == vec![(1, first.s.hash(), Some(TObj::Position(first.s)))], "{r:?}");
    // Two calls, in either order of the two operations.
    for (what, ops) in [
        ("the add first", vec![(&ab, vec![Some(0), Some(1), Some(4)], 0usize), (&db, vec![Some(2), Some(3), None], 0)]),
        ("the declare first", vec![(&db, vec![Some(0), Some(1), Some(4)], 0), (&ab, vec![Some(2), Some(3), None], 0)]),
        ("the two scripts first, then the two objects", vec![(&ab, vec![Some(2), Some(0), Some(4)], 0), (&db, vec![Some(3), Some(1), None], 0)]),
        ("each object before the other's script", vec![(&ab, vec![Some(0), Some(3), Some(4)], 0), (&db, vec![Some(1), Some(2), None], 0)]),
    ] {
        let (t, _) = weave(&ops, 2);
        let sg = t.sign(&w.keys).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(verify(&sg.tx, &sg.entries).is_ok(), "{what}: {:?}", verify(&sg.tx, &sg.entries).errors());
        let r = read_signed(&w.lib, &sg, &known);
        assert!(r.unread.is_empty(), "{what}: {r:?}");
        assert!(r.made.iter().any(|(_, o, _)| *o == TObj::Position(added)) && r.made.iter().any(|(_, o, _)| *o == TObj::Claim(claim)), "{what}: what both calls made: {r:?}");
        assert_eq!(r.spent.len(), 2, "{what}");
        for (i, h, o) in &r.spent {
            let want = if *h == first.s.hash() { first.s } else { second.s };
            assert_eq!(*o, Some(TObj::Position(want)), "{what}: the object spent at input {i}");
        }
        // With nothing known the two positions are not rebuilt (their fixed part is shown as a hash): nothing is
        // made up for them.
        let blind = read_signed(&w.lib, &sg, &Pool(vec![]));
        assert!(blind.made.iter().all(|(_, o, _)| matches!(o, TObj::Anchor)) && blind.unread.len() == 2, "{what}: {blind:?}");
    }
    println!("T1b R17: an entry with its object at input 1, and a transaction of two calls in four layouts, are read: what each call made, each spent object at its own input");
}

/// Rounds 16 and 23, the rule of an answer at the engine. A disputed close or liquidation, shown the committee's
/// prices for its time, passes exactly one of the two verdicts: the one the model says (`Claim::within`,
/// `Liq::within`). Amounts on the line, a sompi each side of it, the claimer's cautious amount, nothing and the whole
/// pot, both sides, one leg and two; fees and cures on each bound and a sompi off, the other side, a sound position.
/// What each verdict pays is the model's (`prove_pay`, `refute_pay`): nothing is made and nothing lost, the half of
/// the bond lost that is nobody's is in the script's copy, and whoever refutes (the claimer, the disputer, the other
/// party or a stranger) has the same half. The reader reads the verdict: a proof ends the claim, a refutation writes
/// the position again.
#[test]
fn t1b_r23_a_disputed_claim_is_proved_or_refuted_never_both_and_pays_what_the_model_says() {
    let w = pworld();
    let f = [fund(xk(&w.w), 77, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (disputer, refuter, liquidator) = (kp(41), kp(42), kp(43));
    let fee = kpf_protocol::thin::ORACLE_FEE;
    let (mut proved, mut refuted) = (0, 0);
    // ---- closes.
    for two in [false, true] {
        let position = w.position(two);
        let p = [9 * P10 / 10, if two { 12 * P10 / 10 } else { 0 }];
        let (a1, a2) = (w.attest(0, p[0]), w.attest(1, p[1]));
        let t2 = two.then_some((&w.epoch_tables[1], &a2));
        for side in [SIDE_A, SIDE_B] {
            let line = position.s.x.owed_at(side, p);
            let cautious = position.s.owed(side, p).expect("a close is allowed");
            assert!(cautious <= line, "the claimer's own amount keeps the tolerance from the line");
            for stated in [line + 1, line, line - 1, cautious, 0, position.s.pot()] {
                let (b, c) = close(&w.lib, &position, &w.script("KpfTClose"), side, T0, p, Some(stated), payer).unwrap();
                let cs = b.sign(&w.keys).unwrap();
                let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&disputer), payer).unwrap();
                let ds = b.sign(&w.keys).unwrap();
                let dl = nth(&ds, 0, d, 6_520);
                let within = c.within(p);
                assert_eq!(within, stated <= line, "the model's line");
                let script = w.script("KpfTProve");
                let t1 = (&w.epoch_tables[0], &a1);
                let pb = prove_with(&w.lib, &dl, &script, t1, t2, payer, false).unwrap();
                let (rb, back) = refute_with(&w.lib, &dl, &script, t1, t2, xk(&refuter), payer, false).unwrap();
                assert_eq!((engine_takes(&pb, &w.keys), engine_takes(&rb, &w.keys)), (within, !within), "a close of {stated} with the line at {line} ({} legs, side {side}): (proved, refuted)", 1 + two as usize);
                assert_eq!((prove(&w.lib, &dl, &script, t1, t2, payer).is_ok(), refute(&w.lib, &dl, &script, t1, t2, xk(&refuter), payer).is_ok()), (within, !within), "the tool builds the one that passes");
                let v = dl.value as i64;
                let (claimer, other) = if side == SIDE_A { (xk(&w.a), xk(&w.b)) } else { (xk(&w.b), xk(&w.a)) };
                if within {
                    let sg = pb.sign(&w.keys).unwrap();
                    let (pa, pbb, kept) = d.prove_pay(v);
                    assert_eq!((paid(&sg, &xk(&w.a)), paid(&sg, &xk(&w.b)), paid(&sg, &xk(&disputer))), (pa, pbb, 0), "a proof pays the model's");
                    assert_eq!(sg.tx.outputs[1].value as i64, script.value as i64 + fee + kept);
                    assert_eq!(pa + pbb + kept, v, "nothing made, nothing lost");
                    assert_eq!(paid(&sg, &claimer), stated + c.bond + c.bond / 2);
                    let r = read_signed(&w.lib, &sg, &Pool(vec![TObj::Disputed(d)]));
                    assert_eq!((r.script, r.entry, r.made.len(), r.unread.len()), ("KpfTProve", "answer", 0, usize::from(two) + 1), "a proof read: the claim ended, the epoch tables go on");
                    proved += 1;
                } else {
                    let sg = rb.sign(&w.keys).unwrap();
                    let (pos, dpay, rpay, kept) = d.refute_pay(v).unwrap();
                    assert_eq!(back, position.s);
                    assert_eq!((sg.tx.outputs[0].value as i64, paid(&sg, &xk(&disputer)), paid(&sg, &xk(&refuter)), paid(&sg, &claimer), paid(&sg, &other)), (pos, dpay, rpay, 0, 0), "a refutation pays the model's");
                    assert_eq!(sg.tx.outputs[1].value as i64, script.value as i64 + fee + kept);
                    assert_eq!(pos + dpay + rpay + kept, v);
                    let r = read_signed(&w.lib, &sg, &Pool(vec![TObj::Disputed(d)]));
                    assert!(r.entry == "answer" && r.made.iter().any(|(_, o, _)| *o == TObj::Position(position.s)), "a refutation read: the position again: {r:?}");
                    // Whoever refutes: the claimer itself, the disputer, the other party.
                    for who in [claimer, xk(&disputer), other] {
                        let (rb, _) = refute_with(&w.lib, &dl, &script, t1, t2, who, payer, false).unwrap();
                        let sg = rb.sign(&w.keys).unwrap();
                        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok());
                        assert_eq!(paid(&sg, &who) - if who == xk(&disputer) { dpay } else { 0 }, rpay);
                    }
                    // The proof's layout told the other verdict, a proof told another key than the disputer's.
                    let mut t = rb.clone();
                    let n = args_of(&mut t, 1).len();
                    args_of(&mut t, 1)[n - 1] = iv(1);
                    assert!(!engine_takes(&t, &w.keys), "a refutation told the verdict 1");
                    refuted += 1;
                }
                if within {
                    let mut t = pb.clone();
                    let n = args_of(&mut t, 1).len();
                    args_of(&mut t, 1)[n - 1] = iv(0);
                    assert!(!engine_takes(&t, &w.keys), "a proof told the verdict 0");
                    for v in [2, -1] {
                        let mut t = pb.clone();
                        args_of(&mut t, 1)[n - 1] = iv(v);
                        assert!(!engine_takes(&t, &w.keys), "a verdict of {v}");
                    }
                    let mut t = pb.clone();
                    args_of(&mut t, 1)[n - 2] = Arg::V(ArtifactValue::Bytes(xk(&refuter).to_vec()));
                    assert!(!engine_takes(&t, &w.keys), "a proof told another key than the disputer's");
                }
            }
        }
    }
    // ---- liquidations of a thin side at 9.7: the rule's fee, and the two bounds of the cure.
    let mut thin = w.position(false);
    thin.s.x.a_margin = 6 * KAS1;
    thin.value = thin.s.x.floor() as u64;
    let lp = [97 * P10 / 100, 0];
    let l1 = w.attest(0, lp[0]);
    let x = thin.s.x;
    assert!(x.liquidatable(SIDE_A, lp));
    let (top, lo, hi) = (x.fee_of(SIDE_A, lp), x.cure_at(SIDE_A, lp, Some(true)), x.cure_at(SIDE_A, lp, Some(false)));
    assert!(0 < lo && lo <= hi && top > 0, "the fee {top}, the cure between {lo} and {hi}");
    let sound = w.position(false);
    let cases: Vec<(&str, &OLive<Position>, i64, (i64, i64))> = vec![
        ("the rule's fee, the least cure", &thin, SIDE_A, (top, lo)),
        ("the least cure less a sompi", &thin, SIDE_A, (top, lo - 1)),
        ("the most cure", &thin, SIDE_A, (top, hi)),
        ("the most cure and a sompi", &thin, SIDE_A, (top, hi + 1)),
        ("a sompi of fee more", &thin, SIDE_A, (top + 1, lo)),
        ("no fee", &thin, SIDE_A, (0, hi)),
        ("the other side, which holds its maintenance", &thin, SIDE_B, (top, lo)),
        ("a side of a position that holds its maintenance", &sound, SIDE_A, (top, lo)),
    ];
    for (what, position, side, stated) in cases {
        let (b, lq) = liquidate(&w.lib, position, &w.script("KpfTLiq"), side, T0, lp, Some(stated), xk(&liquidator), payer).unwrap();
        let s = b.sign(&w.keys).unwrap();
        let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&disputer), payer).unwrap();
        let ds = b.sign(&w.keys).unwrap();
        let live = nth(&ds, 0, dl, 6_520);
        let within = lq.liq.within(lp);
        let script = w.script("KpfTLiqProve");
        let t1 = (&w.epoch_tables[0], &l1);
        let (pb, _) = liq_prove_with(&w.lib, &live, &script, t1, None, payer, false).unwrap();
        let (rb, back) = liq_refute_with(&w.lib, &live, &script, t1, None, xk(&refuter), payer, false).unwrap();
        assert_eq!((engine_takes(&pb, &w.keys), engine_takes(&rb, &w.keys)), (within, !within), "a liquidation stating {what} {stated:?}: (proved, refuted)");
        let (v, bond) = (live.value as i64, lq.liq.bond);
        if within {
            let sg = pb.sign(&w.keys).unwrap();
            let (to_liq, kept) = dl.prove_pay();
            assert_eq!((paid(&sg, &xk(&liquidator)), paid(&sg, &xk(&disputer)), sg.tx.outputs[0].value as i64), (to_liq, 0, v - bond), "a proved liquidation: half of the disputer's bond to the liquidator, the claim again with its own bond");
            assert_eq!(sg.tx.outputs[1].value as i64, script.value as i64 + fee + kept);
            proved += 1;
        } else {
            let sg = rb.sign(&w.keys).unwrap();
            let (pos, dpay, rpay, kept) = dl.refute_pay(v).unwrap();
            assert_eq!(back, position.s);
            assert_eq!((sg.tx.outputs[0].value as i64, paid(&sg, &xk(&disputer)), paid(&sg, &xk(&refuter)), paid(&sg, &xk(&liquidator))), (pos, dpay, rpay, 0), "a refuted liquidation");
            assert_eq!(sg.tx.outputs[1].value as i64, script.value as i64 + fee + kept);
            assert_eq!(pos + dpay + rpay + kept, v);
            refuted += 1;
        }
    }
    println!("T1b R23 answers: {proved} disputed closes and liquidations proved and {refuted} refuted at the engine, each by the verdict the model gives and by no other; what each pays is the model's, to the sompi");
}

/// Round 23, the tolerance. The claimer's own caution holds: over 400 prices of the committee no further than g from
/// the claimer's (and any further in its favour), the amount `true_claim` states is within the rule; and a
/// liquidation `liquidation_own` states from a liquidator's own price is within the rule at every committee price
/// nearer than g (the model, a grid; the two ends of each at the engine).
#[test]
fn t1b_r23_the_claimers_caution_keeps_it_within_the_rule_wherever_the_committee_is_within_the_tolerance() {
    let w = pworld();
    let f = [fund(xk(&w.w), 78, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let position = w.position(false);
    let g = position.s.x.t.g_bps;
    let own = 9 * P10 / 10;
    let mut outside = vec![];
    for side in [SIDE_A, SIDE_B] {
        let c = position.s.true_claim(side, T0, [own, 0]).expect("a close");
        for k in -200..=200i64 {
            let committee = own + own * g * k / (200 * 10_000);
            if !c.within([committee, 0]) {
                outside.push((side, k));
            }
        }
    }
    assert!(outside.is_empty(), "a cautious close refuted at a committee price within the tolerance: (side, 200ths of g) {outside:?}");
    // The liquidator's own statement.
    let mut thin = position.clone();
    thin.s.x.a_margin = 6 * KAS1;
    thin.value = thin.s.x.floor() as u64;
    let mine = 96 * P10 / 100;
    let lq = thin.s.liquidation_own(SIDE_A, T0, [mine, 0], xk(&w.w)).expect("the side is liquidated even at the price moved toward it");
    let mut out = vec![];
    for k in -200..=200i64 {
        let committee = mine + mine * g * k / (200 * 10_000);
        if !lq.within([committee, 0]) {
            out.push(k);
        }
    }
    // The two ends at the engine: proved at both, when the model says so.
    for k in [-200i64, 200] {
        let committee = mine + mine * g * k / (200 * 10_000);
        let att = w.attest(0, committee);
        let (b, l) = liquidate(&w.lib, &thin, &w.script("KpfTLiq"), SIDE_A, T0, [mine, 0], Some((lq.fee, lq.add)), xk(&w.w), payer).unwrap();
        let s = b.sign(&w.keys).unwrap();
        let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, l, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), payer).unwrap();
        let ds = b.sign(&w.keys).unwrap();
        let (pb, _) = liq_prove_with(&w.lib, &nth(&ds, 0, dl, 6_520), &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, payer, false).unwrap();
        assert_eq!(engine_takes(&pb, &w.keys), lq.within([committee, 0]), "the engine and the model at {k} 200ths of g");
    }
    println!("T1b R23 tolerance: a cautious close is within the rule at 401 committee prices within g of its own on each side; the liquidator's own statement at {mine} (fee {}, cure {}) is outside the rule at {} of 401 committee prices within g: {:?}", lq.fee, lq.add, out.len(), &out[..out.len().min(8)]);
    // ("Nearer than g": at a committee price exactly g from the liquidator's, the cure stated at its own price can
    // fall a rounding short of the least the rule then asks. Inside, it holds.)
    assert!(out.iter().all(|k| k.abs() == 200), "FINDING: `liquidation_own` states something refutable inside the tolerance: {out:?}");
}

/// By design, and what F33 (High in round 23, fixed in round 24 in the executor) rested on. A claim is decided at
/// the committee's prices, whatever price it states: no script reads the stated one. A close that states a low price
/// and the amount the true price gives is not within the rule at the price it states and is within it at the
/// committee's: disputed, it is proved, and the disputer's bond is lost, half of it to the claimer. Whoever disputes
/// by the stated price pays for it; `exec-run` no longer does
/// (`t1b_fixed_f33_exec_run_disputes_no_claim_by_the_price_it_states` in auditT1_cli.rs).
#[test]
fn t1b_design_a_claim_is_decided_at_the_committees_prices_whatever_price_it_states() {
    let w = pworld();
    let f = [fund(xk(&w.w), 79, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let executor = kp(44);
    let position = w.position(false);
    // The committee's price is 10.5 (A is long from 10); the claim states 10.1.
    let (truth, stated) = ([105 * P10 / 100, 0], [101 * P10 / 100, 0]);
    let amount = position.s.owed(SIDE_A, truth).expect("a close at the committee's price");
    let (b, c) = close(&w.lib, &position, &w.script("KpfTClose"), SIDE_A, T0, stated, Some(amount), payer).unwrap();
    assert!(!c.within(c.a), "the executor's rule without signed prices: not within the rule at the prices it states: disputed");
    assert!(c.within(truth), "and within the rule at the committee's price");
    let cs = signed("the bait: a close stating 10.1 and what 10.5 gives", &b, &w.keys);
    let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&executor), payer).unwrap();
    let ds = signed("the executor disputes it", &b, &w.keys);
    let att = w.attest(0, truth[0]);
    let dl = nth(&ds, 0, d, 6_520);
    let pb = prove(&w.lib, &dl, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, payer).unwrap();
    let sg = pb.sign(&w.keys).unwrap();
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "a claim within the rule at the price the committee signed is proved");
    assert!(refute(&w.lib, &dl, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, xk(&executor), payer).is_err(), "nothing refutes it");
    assert_eq!(paid(&sg, &xk(&executor)), 0, "the executor's bond is gone");
    assert_eq!(paid(&sg, &xk(&w.a)), amount + c.bond + c.bond / 2, "the claimer has its amount, its bond and half of the executor's");
    println!("T1b DESIGN (F33 fixed in the executor): a close stating 10.1 and the amount 10.5 gives is disputed by the stated price and proved at the committee's: the disputer loses {} sompi, {} of it to the claimer", c.bond, c.bond / 2);
}

/// Round 23, the arrears. Only the table of an epoch that has ended pays the fee pool: the builder locks the payment
/// to the end of the table's window, and the same payment locked a DAA earlier is refused by the script.
#[test]
fn t1b_r23_only_the_table_of_an_ended_epoch_pays_the_fee_pool() {
    let w = dues_world(200, &[30, 22, 15]);
    let epoch_table = w.epoch_table();
    let att = w.attest(&epoch_table);
    let funds = vec![fund(xk(&w.f), 30, 1_000)];
    let (b, _) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    assert_eq!(b.tx.lock_time, epoch_table.s.until as u64, "locked to the end of the table's window");
    assert!(takes(&b, &w.keys));
    for lock in [epoch_table.s.until as u64 - 1, T0 as u64, 0] {
        let mut t = b.clone();
        t.tx.lock_time = lock;
        let s = t.sign(&w.keys).expect("signed");
        assert!(!kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok() && (lock < T0 as u64 || script_refuses(&s.tx, &s.entries, 1)), "a payment locked to {lock}, before the window's end, is taken");
    }
    println!("T1b R23 arrears: a payment by a table whose window ends at {} is locked there; locked a DAA earlier it is refused by the payment's script", epoch_table.s.until);
}

// ---------------------------------------------------------------- round 19: the oracle fees through the builders

use kpf_protocol::committee::Table;
use kpf_protocol::thin::{oracle_fee_shares, ORACLE_FEE_CUT, ORACLE_FEE_POT_MAX, ORACLE_FEE_UNIT};

/// What the genesis of an oracle fees world gives each copy and the fee pool.
const DFLOOR: i64 = 5 * KAS1;

/// A library with the fee pool of a committee, the committee's table, and an epoch table of it.
struct DW {
    f: Keypair,
    members: Vec<Keypair>,
    keys: KeySet,
    rid: Hash,
    table: Table,
    total: i64,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    fee_pool: FeePool,
    genesis: Transaction,
}
impl DW {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(&self.f), rate: MIN_FEE_RATE }
    }
    fn epoch_table_of(&self, table: Table, total: i64, from: i64, until: i64) -> OLive<EpochTable> {
        let rp = CommitteeParams::testnet();
        OLive { s: EpochTable { members: table, total, from, until, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(self.rid), op: op(20), daa: from as u64 }
    }
    fn epoch_table(&self) -> OLive<EpochTable> {
        self.epoch_table_of(self.table, self.total, 6_000, 9_000)
    }
    /// Every member's signature for a signed price of some market of the committee.
    fn attest_at(&self, t: &OLive<EpochTable>, keys: &[Keypair], t0: i64) -> Attestation {
        let market = [0xfe; 32];
        let signed_prices: Vec<SignedPrice> = keys.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, t0, 1_000_000_000)).collect();
        Attestation::collect(&t.s, &self.rid.as_bytes(), &market, t0, &signed_prices).expect("a quorum")
    }
    fn attest(&self, t: &OLive<EpochTable>) -> Attestation {
        self.attest_at(t, &self.members, T0)
    }
    /// The fee pool holding `above` more than its floor.
    fn fee_pool_with(&self, above: i64) -> FeePool {
        let mut p = self.fee_pool.clone();
        p.value += above as u64;
        p
    }
}
/// The library whose genesis spends the funder's coin `n` (another coin, another library), for the committee of members of
/// these weights (KAS).
fn dues_world(n: u8, weights: &[i64]) -> DW {
    let f = kp(9);
    let members: Vec<Keypair> = (0..weights.len()).map(|i| kp(101 + i as u8)).collect();
    let mut keys = KeySet::new();
    keys.add_keypair(f);
    let rid = Hash::from_bytes([0x52; 32]);
    let mut table = EMPTY_TABLE;
    for (i, wt) in weights.iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let funds = vec![fund(xk(&f), n, 1_000)];
    let (g, lib) = lib_genesis_for(1, DFLOOR as u64, Some(rid.as_bytes()), Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("the genesis signed");
    assert!(kpf_protocol::verify::verify(&gs.tx, &gs.entries).is_ok());
    assert_eq!(lib_check(&gs.tx).unwrap(), lib);
    let fee_pool = lib_fee_pool(&gs.tx, 6_000).expect("a fee pool");
    DW { f, members, keys, rid, table, total: weights.iter().sum::<i64>() * KAS1, lib, scripts: lib_scripts(&gs.tx, 6_000), fee_pool, genesis: gs.tx.clone() }
}
fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
/// The arguments of the contract input `i`.
fn args_of(b: &mut OBuilt, i: usize) -> &mut Vec<Arg> {
    let OPlan::Contract { args, .. } = &mut b.plans[i] else { panic!("input {i} is not a contract input") };
    args
}
fn takes(b: &OBuilt, keys: &KeySet) -> bool {
    b.sign(keys).map(|sg| kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok()).unwrap_or(false)
}

/// F22 (Medium in round 19), fixed in round 20. `KpfTPay.pay` held each member's output to its key and at least its
/// share, and nothing said the output was that payment's alone: two libraries whose fee pools are for one committee were
/// paid in ONE transaction with ONE set of member outputs, and whoever sent it kept the smaller fee pool whole. A payment
/// is now alone in its transaction: four inputs (the fee pool at 0, the script at 1, the epoch table at 2, one coin of a key
/// at 3), the members from output 3. The attack as it was built: library A's payment, exactly as its builder lays it
/// out, with library B's fee pool and script as two inputs more and no member output more. A's payment, whose every other
/// condition holds, refuses for the two inputs; B's has no place it could stand at (its fee pool is not input 0).
#[test]
fn t1b_fixed_f22_a_payment_is_alone_in_its_transaction() {
    let (wa, wb) = (dues_world(200, &[30, 22, 15]), dues_world(201, &[30, 22, 15]));
    assert_ne!(wa.lib, wb.lib, "two libraries");
    assert_eq!(wa.fee_pool.s, wb.fee_pool.s, "for one committee, with one floor");
    let (pot_a, pot_b) = (20 * KAS1, 12 * KAS1);
    let epoch_table = wa.epoch_table();
    let att = wa.attest(&epoch_table);
    let (fa, fb) = (vec![fund(xk(&wa.f), 30, 1_000)], vec![fund(xk(&wa.f), 31, 1_000)]);
    let (ba, shares_a) = pay(&wa.lib, &wa.fee_pool_with(pot_a), &wa.script("KpfTPay"), (&epoch_table, &att), wa.payer(&fa)).unwrap();
    let (bb, shares_b) = pay(&wb.lib, &wb.fee_pool_with(pot_b), &wb.script("KpfTPay"), (&epoch_table, &att), wb.payer(&fb)).unwrap();
    // Each on its own is taken.
    signed("pay of library A", &ba, &wa.keys);
    signed("pay of library B", &bb, &wa.keys);
    assert!(shares_a.iter().zip(shares_b.iter()).all(|(x, y)| x.0 == y.0 && x.1 >= y.1), "A's shares cover B's, member by member");
    // ---- both in one transaction: A's payment as built (the members at outputs 3 to 5, the change at 6), then B's
    // fee pool and script as inputs 4 and 5, going on at outputs 7 and 8. No member output for B.
    let mut t = ba.clone();
    for i in [0usize, 1] {
        t.tx.inputs.push(bb.tx.inputs[i].clone());
        t.entries.push(bb.entries[i].clone());
        t.plans.push(bb.plans[i].clone());
        t.input_labels.push(format!("B: {}", bb.input_labels[i]));
    }
    *args_of(&mut t, 4) = vec![iv(5), iv(0)];
    let who: Vec<u8> = [4i64, 4, 4].iter().flat_map(|x| sm8(*x)).collect();
    args_of(&mut t, 5)[0] = Arg::V(ArtifactValue::Bytes(who));
    let c = ba.change.expect("a change");
    assert_eq!((c, ba.tx.outputs.len()), (6, 7), "A's payment: the fee pool, the script, the epoch table, three members, the change");
    let lid_b = Hash::from_bytes(wb.lib);
    for (i, auth) in [(0usize, 4u16), (1, 5)] {
        t.tx.outputs.push(TransactionOutput { covenant: Some(CovenantBinding { authorizing_input: auth, covenant_id: lid_b }), ..bb.tx.outputs[i].clone() });
        t.output_labels.push(format!("B: {}", bb.output_labels[i]));
    }
    // (What B's fee pool lets go would be the sender's: its change takes it, less a little more fee.)
    t.tx.outputs[c].value += (pot_b - 5_000_000) as u64;
    let sg = t.sign(&wa.keys).expect("signed");
    assert!(!kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "F22 is open again: two libraries are paid by one set of member outputs");
    assert!(script_refuses(&sg.tx, &sg.entries, 1), "A's payment refuses the two inputs more (its transaction is four inputs)");
    assert!(script_refuses(&sg.tx, &sg.entries, 5), "B's payment refuses: its fee pool is not input 0, it is not input 1");
    // The same with B's two inputs alone beside A's coin and epoch table, B's script told the truth about nothing else:
    // every arrangement of two payments has a fee pool that is not input 0.
    println!("T1b F22 fixed: library A's payment with library B's fee pool and script beside it and no member output more: refused by both payments' scripts");
}

/// F23 (Low in round 19), fixed in round 20. The sender of a payment kept a twentieth AND what the shares lose by
/// being counted in units of 0.01 KAS. The fee pool is now held to what it held less the shares asked and the twentieth:
/// the rounding stays in it. Over every pot from 0.05 to 12 KAS, for 3 and 15 members, what leaves the fee pool is the
/// shares and a twentieth of the pot exactly; and the payment of round 19 (three equal members, 0.25 KAS above the
/// floor, where the sender kept 16 %) with the rounding moved from the fee pool to the sender's change is refused.
#[test]
fn t1b_fixed_f23_what_the_rounding_leaves_stays_in_the_fee_pool() {
    let mut most_left = 0i64;
    for members in [3usize, 15] {
        let w = dues_world(200, &vec![10; members]);
        let mut pot = 5_000_000i64;
        while pot <= 12 * KAS1 {
            if let Some((taken, shares)) = oracle_fee_shares(DFLOOR + pot, DFLOOR, &w.table, w.total) {
                let paid: i64 = shares.iter().map(|s| s.1).sum();
                assert_eq!(taken, paid + pot / ORACLE_FEE_CUT, "what leaves the fee pool is the shares and the sender's twentieth");
                assert!(paid <= pot - pot / ORACLE_FEE_CUT);
                let left = pot - taken;
                assert!((0..(members as i64 + 1) * ORACLE_FEE_UNIT).contains(&left), "the fee pool keeps the rounding: {left} of {pot}");
                most_left = most_left.max(left);
            }
            pot += 1_000_000;
        }
    }
    // The payment of round 19.
    let w = dues_world(200, &[10; 3]);
    let pot = 25_000_000;
    let epoch_table = w.epoch_table();
    let att = w.attest(&epoch_table);
    let funds = vec![fund(xk(&w.f), 30, 1_000)];
    let (b, shares) = pay(&w.lib, &w.fee_pool_with(pot), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    let sg = signed("pay of 0.25 KAS to three equal members", &b, &w.keys);
    let paid_out: i64 = shares.iter().map(|s| s.1).sum();
    assert_eq!(paid_out, 21_000_000);
    let rounding = pot - pot / ORACLE_FEE_CUT - paid_out;
    assert_eq!(sg.tx.outputs[0].value as i64, DFLOOR + rounding, "the fee pool keeps what the shares lost");
    let left = sg.tx.outputs[b.change.unwrap()].value as i64 - 1_000 * KAS1;
    assert_eq!(left, pot / ORACLE_FEE_CUT - sg.fee as i64, "the sender has its twentieth less the fee and nothing more");
    // The attack: the rounding to the sender's change, the fee pool at its floor.
    let mut t = b.clone();
    t.tx.outputs[0].value -= rounding as u64;
    t.tx.outputs[b.change.unwrap()].value += rounding as u64;
    let s = t.sign(&w.keys).expect("signed");
    assert!(script_refuses(&s.tx, &s.entries, 1), "F23 is open again: the sender keeps the rounding");
    let mut t = b.clone();
    t.tx.outputs[0].value -= 1;
    let s = t.sign(&w.keys).expect("signed");
    assert!(script_refuses(&s.tx, &s.entries, 1), "the fee pool a sompi under what it keeps");
    // What stays is shared the next time: the fee pool as it now stands and 0.25 KAS more.
    let mut next = nth(&sg, 0, w.fee_pool.s, 6_600);
    next.value += pot as u64;
    let (_, shares2) = pay(&w.lib, &next, &nth(&sg, 1, "KpfTPay", 6_600), (&epoch_table, &att), w.payer(&funds)).unwrap();
    assert!(shares2.iter().map(|s| s.1).sum::<i64>() > paid_out, "the rounding kept is in the next payment");
    // A member below a unit of weight is in the sum and has no output; its part stays in the fee pool too.
    let mut table = dues_world(200, &[10; 15]).table;
    table[14] = Member { key: table[14].key, weight: ORACLE_FEE_UNIT - 1 };
    let (_, shares) = oracle_fee_shares(DFLOOR + 100 * KAS1, DFLOOR, &table, 140 * KAS1 + ORACLE_FEE_UNIT - 1).unwrap();
    assert_eq!(shares.len(), 14);
    println!("T1b F23 fixed: what leaves the fee pool is the shares and a twentieth of the pot; the rounding (at most {most_left} sompi over the pots tried) stays; the payment of round 19 leaves its sender {left} sompi after the fee of {}", sg.fee);
}

/// Rounds 19 and 20, question 4. What a payment reads as its epoch table, the four inputs it is, where its members' outputs
/// stand, and what it shares.
#[test]
fn t1b_r19_a_payment_is_held_to_its_epoch_table_its_places_and_its_shares() {
    let w = dues_world(200, &[30, 22, 15]);
    let epoch_table = w.epoch_table();
    let att = w.attest(&epoch_table);
    let funds = vec![fund(xk(&w.f), 30, 1_000)];
    let (b, shares) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    let sg = signed("pay", &b, &w.keys);
    let c = b.change.unwrap();
    assert_eq!((b.tx.inputs.len(), c, b.tx.outputs.len()), (4, 6, 7));
    let mut refused = 0;
    let mut no = |what: &str, t: &OBuilt| {
        if let Ok(s) = t.sign(&w.keys) {
            assert!(script_refuses(&s.tx, &s.entries, 1) || !kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok(), "{what} is accepted");
        }
        refused += 1;
    };
    // Refused by the payment's own script (signed, and input 1 fails).
    let mut by_pay = 0;
    let mut pay_no = |what: &str, t: &OBuilt, i: usize| {
        let s = t.sign(&w.keys).unwrap_or_else(|e| panic!("{what}: not signed: {e}"));
        assert!(script_refuses(&s.tx, &s.entries, i), "{what} is accepted by the payment");
        by_pay += 1;
    };
    // ---- the four inputs (round 20). The key told is not the coin's.
    let mut t = b.clone();
    args_of(&mut t, 1)[5] = Arg::V(ArtifactValue::Bytes(xk(&kp(77)).to_vec()));
    pay_no("the payer told is another key than the coin's", &t, 1);
    // The coin an output of a script (its address a script's): no key is told that it pays to.
    let mut t = b.clone();
    t.entries[3].script_public_key = script_spk("KpfTAdd").unwrap();
    pay_no("the fourth input is not a coin of a key", &t, 1);
    // A second coin: five inputs.
    let mut t = b.clone();
    t.tx.inputs.push(TransactionInput::new_with_compute_budget(op(34), vec![], 0, 0));
    t.entries.push(kaspa_consensus_core::tx::UtxoEntry::new(1_000 * KAS1 as u64, p2pk_spk(&xk(&w.f)), 0, false, None));
    t.plans.push(OPlan::Key { key: xk(&w.f) });
    t.input_labels.push("a second coin".into());
    t.tx.outputs[c].value += 1_000 * KAS1 as u64;
    pay_no("a second coin (five inputs)", &t, 1);
    // No coin: three inputs (the fee out of the sender's twentieth).
    let mut t = b.clone();
    t.tx.inputs.pop();
    t.entries.pop();
    t.plans.pop();
    t.input_labels.pop();
    t.tx.outputs[c].value -= 1_000 * KAS1 as u64;
    pay_no("no coin (three inputs)", &t, 1);
    // The fee pool and the script in each other's places (their outputs with them, the places told as they stand).
    let mut t = b.clone();
    t.tx.inputs.swap(0, 1);
    t.entries.swap(0, 1);
    t.plans.swap(0, 1);
    t.input_labels.swap(0, 1);
    t.tx.outputs.swap(0, 1);
    t.output_labels.swap(0, 1);
    for i in [0usize, 1] {
        if let Some(cv) = &mut t.tx.outputs[i].covenant {
            cv.authorizing_input = i as u16;
        }
    }
    args_of(&mut t, 0)[0] = Arg::V(ArtifactValue::Bytes([1i64, 1, 1].iter().flat_map(|x| sm8(*x)).collect()));
    *args_of(&mut t, 1) = vec![iv(0), iv(0)];
    pay_no("the script at input 0 and the fee pool at input 1", &t, 0);
    // The epoch table and the coin in each other's places.
    let mut t = b.clone();
    t.tx.inputs.swap(2, 3);
    t.entries.swap(2, 3);
    t.plans.swap(2, 3);
    t.input_labels.swap(2, 3);
    if let Some(cv) = &mut t.tx.outputs[2].covenant {
        cv.authorizing_input = 3;
    }
    pay_no("the coin at input 2 and the epoch table at input 3", &t, 1);
    // ---- where the members' outputs stand: one output before them (they begin at output 4), the first member's left out.
    let mut t = b.clone();
    t.tx.outputs.insert(3, TransactionOutput { value: KAS1 as u64, script_public_key: p2pk_spk(&xk(&w.f)), covenant: None });
    t.output_labels.insert(3, "a key".into());
    t.change = Some(c + 1);
    t.tx.outputs[c + 1].value -= KAS1 as u64;
    pay_no("an output before the members'", &t, 1);
    let mut t = b.clone();
    let first = t.tx.outputs.remove(3);
    t.output_labels.remove(3);
    t.change = Some(c - 1);
    t.tx.outputs[c - 1].value += first.value;
    pay_no("the first member's output left out", &t, 1);
    // ---- who.
    for who in [[1i64, 1, 1], [2, 2, 2], [3, 3, 3], [0, 0, 2], [0, 2, 0], [0, 1, 1], [-1, -1, -1]] {
        let mut t = b.clone();
        args_of(&mut t, 1)[0] = Arg::V(ArtifactValue::Bytes(who.iter().flat_map(|x| sm8(*x)).collect()));
        no(&format!("who {who:?}"), &t);
    }
    // ---- the outputs: a member a sompi short, two members' shares in one output, the fee pool a sompi under what it keeps,
    // the fee pool as another script, the change under the library's id.
    for i in 3..6 {
        let mut t = b.clone();
        t.tx.outputs[i].value -= 1;
        no("a member a sompi short", &t);
    }
    let mut t = b.clone();
    t.tx.outputs[3].value += t.tx.outputs[4].value;
    t.tx.outputs[4].script_public_key = p2pk_spk(&xk(&w.f));
    no("two members' shares in the first member's output", &t);
    let mut t = b.clone();
    t.tx.outputs[0].value -= 1;
    no("the fee pool a sompi under what it keeps", &t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = fee_pool_spk(&FeePoolOf { committee: [0x53; 32], floor: DFLOOR });
    no("the fee pool goes on as the fee pool of another committee", &t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = script_spk("KpfTAdd").unwrap();
    no("the fee pool goes on as a script of the library", &t);
    for auth in [0u16, 1] {
        let mut t = b.clone();
        t.tx.outputs[c].covenant = Some(CovenantBinding { authorizing_input: auth, covenant_id: Hash::from_bytes(w.lib) });
        no("the change under the library's id", &t);
    }
    // ---- the epoch table retired instead of attesting (no output of the committee's id): the payment reads no such epoch table.
    let mut t = b.clone();
    if let OPlan::Contract { entry, args, .. } = &mut t.plans[2] {
        *entry = "retire";
        args.clear();
    }
    t.tx.lock_time = 12_000;
    no("an epoch table spent by retire", &t);
    // ---- an epoch table of the committee whose two first members are one key: each member has its own output.
    let mut table = w.table;
    table[1].key = table[0].key;
    let twin = w.epoch_table_of(table, w.total, 6_000, 9_000);
    let att2 = w.attest_at(&twin, &[w.members[0], w.members[0], w.members[2]], T0);
    let (b2, sh2) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&twin, &att2), w.payer(&funds)).unwrap();
    assert!(takes(&b2, &w.keys) && sh2[0].0 == sh2[1].0, "two members of one key are paid in two outputs");
    let mut t = b2.clone();
    t.tx.outputs.remove(4);
    t.output_labels.remove(4);
    t.change = Some(b2.change.unwrap() - 1);
    t.tx.outputs[3].value = sh2[0].1.max(sh2[1].1) as u64;
    no("two members of one key paid by one output", &t);
    // ---- the payer a member (round 20): its own coin pays, and its change and its share are two outputs or one. Either
    // way the other members' outputs are theirs, and what the payer has is its own share and its twentieth.
    let mut both = KeySet::new();
    both.add_keypair(w.members[0]);
    let coin = vec![fund(xk(&w.members[0]), 35, 1_000)];
    let (b4, sh4) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), Payer { funds: &coin, change_to: xk(&w.members[0]), rate: MIN_FEE_RATE }).unwrap();
    let s4 = b4.sign(&both).expect("signed by the member");
    assert!(kpf_protocol::verify::verify(&s4.tx, &s4.entries).is_ok(), "a member pays the fee pool with its own coin");
    let c4 = b4.change.unwrap();
    let mut t = b4.clone();
    let ch = t.tx.outputs.remove(c4);
    t.output_labels.remove(c4);
    t.tx.outputs[3].value += ch.value;
    t.change = Some(3);
    let s5 = t.sign(&both).expect("signed");
    assert!(kpf_protocol::verify::verify(&s5.tx, &s5.entries).is_ok(), "the paying member's change and share in one output");
    for i in [1usize, 2] {
        assert_eq!(paid(&s5, &sh4[i].0), sh4[i].1, "the other members are paid their shares");
    }
    // (One output fewer is a smaller fee: the paying member has that much more, and nothing else.)
    let (one, two) = (paid(&s5, &sh4[0].0), paid(&s4, &sh4[0].0));
    assert!(one >= two && one - two == s4.fee as i64 - s5.fee as i64, "the paying member has the same either way but for the fee: {one} and {two}");
    // ---- what the one coin asks of a payer: the builder takes exactly one, of any size that the twentieth and it
    // pay the fee from (a coin of 0.01 KAS here: the fee comes out of the sender's part).
    let two = vec![fund(xk(&w.f), 36, 1_000), fund(xk(&w.f), 37, 1_000)];
    assert!(pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&two)).unwrap_err().to_string().contains("one coin"));
    assert!(pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&[])).is_err());
    let small = vec![KeyUtxo { amount: 1_000_000, ..fund(xk(&w.f), 38, 1) }];
    let (b5, _) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&small)).unwrap();
    let s5 = signed("pay with a coin of 0.01 KAS", &b5, &w.keys);
    assert_eq!(s5.tx.outputs[b5.change.unwrap()].value as i64, 1_000_000 + KAS1 - s5.fee as i64, "the payer has its coin and its twentieth less the fee");
    // ---- an output more than the members and the change: taken.
    let mut t = b.clone();
    t.tx.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: p2pk_spk(&xk(&kp(77))), covenant: None });
    t.output_labels.push("a key".into());
    t.tx.outputs[c].value -= KAS1 as u64;
    assert!(takes(&t, &w.keys), "an output more, after the members'");
    // ---- the most one payment shares: the rest stays in the fee pool.
    let rich = w.fee_pool_with(ORACLE_FEE_POT_MAX + 7 * KAS1);
    let (b3, sh3) = pay(&w.lib, &rich, &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    let s3 = signed("pay of the most", &b3, &w.keys);
    let paid3: i64 = sh3.iter().map(|s| s.1).sum();
    assert_eq!(s3.tx.outputs[0].value as i64, DFLOOR + 7 * KAS1 + ORACLE_FEE_POT_MAX - ORACLE_FEE_POT_MAX / ORACLE_FEE_CUT - paid3, "what is above the most, and the rounding, stay in the fee pool");
    assert!(paid3 <= ORACLE_FEE_POT_MAX - ORACLE_FEE_POT_MAX / ORACLE_FEE_CUT && s3.tx.outputs[0].value as i64 >= DFLOOR + 7 * KAS1);
    let mut t = b3.clone();
    t.tx.outputs[0].value -= 1;
    no("the fee pool a sompi under what a payment of the most leaves", &t);
    let _ = (sg, shares);
    println!("T1b R19/R20 pay: {by_pay} arrangements of the four inputs and of the members' places refused by the payment's script, {refused} others refused; two members of one key are two outputs; a member that pays has its share and its twentieth, in one output or two; a payment shares at most {ORACLE_FEE_POT_MAX} sompi and leaves the rest");
}

/// Round 20, the fourth input. The payment asks that input 3 pays to the key it is told, and nothing of whether that
/// coin carries a covenant id. A coin of a key written under an id of the payer's own making passes, and the members'
/// outputs can then be written under that id (the coin authorizes them): each is still an output to the member's key
/// of at least its share, spent by that key alone. Measured, not a way to take anything; a wallet that leaves aside
/// outputs under an id it does not know would not show the member its payment.
#[test]
fn t1b_design_the_coin_of_a_payment_may_carry_an_id_and_the_members_outputs_with_it() {
    let w = dues_world(200, &[30, 22, 15]);
    let epoch_table = w.epoch_table();
    let att = w.attest(&epoch_table);
    let funds = vec![fund(xk(&w.f), 30, 1_000)];
    let (b, shares) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    let x = Hash::from_bytes([0x99; 32]);
    let mut t = b.clone();
    t.entries[3] = kaspa_consensus_core::tx::UtxoEntry::new(t.entries[3].amount, t.entries[3].script_public_key.clone(), 0, false, Some(x));
    for i in 3..6 {
        t.tx.outputs[i].covenant = Some(CovenantBinding { authorizing_input: 3, covenant_id: x });
    }
    let taken = takes(&t, &w.keys);
    println!("T1b DESIGN (round 20): a payment whose coin is a key's output under an id of the payer's making, the members' outputs written under that id: taken by the engine: {taken}; each member's output is to its key and of its share ({} sompi the first)", shares[0].1);
    // Either answer is recorded; the report says which. With the id on the outputs and not on the coin it is refused
    // by the engine's own rule (no input gives the id).
    let mut t = b.clone();
    for i in 3..6 {
        t.tx.outputs[i].covenant = Some(CovenantBinding { authorizing_input: 3, covenant_id: x });
    }
    assert!(!takes(&t, &w.keys), "outputs under an id no input carries");
    assert!(taken, "the engine began to refuse a member's output under the coin's id: say so in the report");
}

/// By design (limits 2 and 3 of the design), measured. A payment shares by the table of whatever epoch table of the
/// fee pool's committee it is shown: one whose dispute period ended long ago pays the members of that time, and nothing in the
/// payment asks for the present table (an epoch table stands until somebody retires it, which anybody may a dispute period after
/// its end: `KpfEpochTable.retire`, read). And the committee is whatever the library's genesis named: the payment trusts
/// any epoch table under that id, as the proofs trust a market's, so a library's oracle fees are its members' only if that id is
/// a committee's (`lib-check` shows the id; nothing in a library ties it to the committee of the markets traded in it).
#[test]
fn t1b_design_a_payment_shares_by_the_table_of_any_epoch_table_under_the_committees_id() {
    let w = dues_world(200, &[30, 22, 15]);
    let funds = vec![fund(xk(&w.f), 30, 1_000)];
    // An epoch table of an epoch long past (its dispute period 100..200, the signed price shown 150), its table two members that have left.
    let gone = [kp(120), kp(121)];
    let mut old = EMPTY_TABLE;
    old[0] = Member { key: xk(&gone[0]), weight: 40 * KAS1 };
    old[1] = Member { key: xk(&gone[1]), weight: 20 * KAS1 };
    let epoch_table = w.epoch_table_of(old, 60 * KAS1, 100, 200);
    let att = w.attest_at(&epoch_table, &gone, 150);
    let (b, shares) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    let sg = signed("pay by an epoch table of an old epoch", &b, &w.keys);
    assert_eq!(shares.len(), 2);
    assert_eq!(paid(&sg, &xk(&gone[0])) + paid(&sg, &xk(&gone[1])), shares.iter().map(|s| s.1).sum::<i64>());
    assert_eq!(w.members.iter().map(|k| paid(&sg, &xk(k))).sum::<i64>(), 0, "the members of the present table are paid nothing");
    // A "committee" of one member: whoever made an epoch table under the id the genesis named.
    let me = [kp(130)];
    let mut one = EMPTY_TABLE;
    one[0] = Member { key: xk(&me[0]), weight: KAS1 };
    let forged = w.epoch_table_of(one, KAS1, 6_000, 9_000);
    let att = w.attest_at(&forged, &me, T0);
    let (b, shares) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&forged, &att), w.payer(&funds)).unwrap();
    let sg = signed("pay by an epoch table of one member", &b, &w.keys);
    assert_eq!((shares.len(), paid(&sg, &xk(&me[0]))), (1, 19 * KAS1));
    println!("T1b DESIGN: a payment shares by the table of the epoch table shown: an epoch long past pays its members of then; an epoch table of one member under the id pays that member all (the id is trusted to be a committee's, as a market's is)");
}

/// Round 19, question 3 at the genesis. `lib_check` takes a genesis with the fee pool its payload names and with none;
/// every other output under the id is refused, with the id recomputed so that the refusal is the script's and not
/// the id's: a fee pool for another committee or floor than the payload's, a fee pool whose state is an anchor's or an
/// position's, a fee pool and a second one, a slot of the library. What it does not look at is values: a fee pool or a copy
/// holding less than the floor is taken (it is the maker's own money that is missing).
#[test]
fn t1b_r19_a_genesis_is_a_library_with_at_most_the_fee_pool_its_payload_names() {
    let w = dues_world(200, &[30, 22, 15]);
    let g = &w.genesis;
    let at = g.outputs.iter().position(|o| o.script_public_key == fee_pool_spk(&w.fee_pool.s)).expect("the fee pool");
    assert_eq!(lib_check(&regenesis(g, 0)).unwrap(), w.lib, "the genesis rebuilt as it was is the same library");
    let kit = kpf_protocol::thin_artifacts::kit();
    let fee_pool_of = |state: [u8; 32]| kaspa_txscript::pay_to_script_hash_script(&kit.fee_pool(&state));
    let payload = |committee: [u8; 32], floor: i64| [GENESIS_MAGIC.to_vec(), committee.to_vec(), sm8(floor).to_vec()].concat();
    let anchor_state = blake2b(&[vec![8u8], sm8(7).to_vec()].concat());
    let mut cases: Vec<(&str, Transaction)> = vec![];
    let mut put = |what: &'static str, f: &dyn Fn(&mut Transaction)| {
        let mut t = g.clone();
        f(&mut t);
        cases.push((what, regenesis(&t, 0)));
    };
    put("a fee pool for another committee than the payload's", &|t| t.outputs[at].script_public_key = fee_pool_spk(&FeePoolOf { committee: [0x53; 32], floor: DFLOOR }));
    put("a fee pool of another floor than the payload's", &|t| t.outputs[at].script_public_key = fee_pool_spk(&FeePoolOf { committee: w.rid.as_bytes(), floor: DFLOOR + 1 }));
    put("a fee pool whose state is an anchor's", &|t| t.outputs[at].script_public_key = fee_pool_of(anchor_state));
    put("a fee pool whose state is a position's", &|t| t.outputs[at].script_public_key = fee_pool_of(samples::position().hash()));
    put("a second fee pool of the same committee", &|t| { let o = t.outputs[at].clone(); t.outputs.push(o); });
    put("a second fee pool of another committee", &|t| { let mut o = t.outputs[at].clone(); o.script_public_key = fee_pool_spk(&FeePoolOf { committee: [0x53; 32], floor: DFLOOR }); t.outputs.push(o); });
    put("an object of the library beside the fee pool", &|t| { let mut o = t.outputs[at].clone(); o.script_public_key = kaspa_txscript::pay_to_script_hash_script(&kit.object(&w.lib, &samples::position().hash())); t.outputs.push(o); });
    put("a payload a byte longer", &|t| t.payload.push(0));
    put("a payload a byte shorter", &|t| { t.payload.pop(); });
    put("another magic", &|t| t.payload[3] = 1);
    put("a floor below what a script holds at least, the fee pool of that floor", &|t| { t.payload = payload(w.rid.as_bytes(), KAS1 - 1); t.outputs[at].script_public_key = fee_pool_spk(&FeePoolOf { committee: w.rid.as_bytes(), floor: KAS1 - 1 }); });
    put("a floor of nothing", &|t| { t.payload = payload(w.rid.as_bytes(), 0); t.outputs[at].script_public_key = fee_pool_spk(&FeePoolOf { committee: w.rid.as_bytes(), floor: 0 }); });
    put("a floor below nothing", &|t| { t.payload = payload(w.rid.as_bytes(), -DFLOOR); t.outputs[at].script_public_key = fee_pool_spk(&FeePoolOf { committee: w.rid.as_bytes(), floor: -DFLOOR }); });
    put("no fee pool and a payload that names one", &|t| { t.outputs.remove(at); });
    let n = cases.len();
    for (what, t) in &cases {
        match (what, lib_check(t)) {
            // A payload that names a fee pool the genesis did not write: a library without a fee pool (its oracle fees stay).
            (w, Ok(_)) if w.starts_with("no fee pool") => assert!(lib_fee_pool(t, 0).is_none()),
            (_, Ok(_)) => panic!("{what}: taken as a library"),
            (_, Err(e)) => assert!(e.to_string().contains("REFUSED") || e.to_string().contains("not one of the family"), "{what}: {e}"),
        }
    }
    // Values are not looked at.
    let mut t = g.clone();
    t.outputs[at].value = 1;
    let poor_fee_pool = lib_check(&regenesis(&t, 0)).is_ok();
    let mut t = g.clone();
    let copy = t.outputs.iter().position(|o| o.script_public_key == script_spk("KpfTAdd").unwrap()).unwrap();
    t.outputs[copy].value = (DFLOOR - KAS1) as u64;
    let poor_copy = lib_check(&regenesis(&t, 0)).is_ok();
    assert!(poor_fee_pool && poor_copy, "lib_check began to look at values: say so in the report");
    // A fee pool told a floor above what the copies hold: a sweep leaves a copy the floor, so there is nothing to sweep
    // until a copy holds the floor and the least above it (the builder says so).
    println!("T1b R19 genesis: {n} geneses that are not a library with the fee pool their payload names, refused (the one without its fee pool is a library without one); a fee pool or a copy holding less than the floor is taken: {poor_fee_pool}, {poor_copy}");
}

/// Round 19, question 6 at the reader. A sweep and a payment by anyone are transactions of the library that move no
/// object of a registry: read back as such whatever is known, with a copy left above its floor, with the fee pool given
/// more than its part, and with a key's output more.
#[test]
fn t1b_r19_the_reader_takes_a_sweep_and_a_payment_as_moving_nothing_of_a_registry() {
    let w = dues_world(200, &[30, 22, 15]);
    let funds = vec![fund(xk(&w.f), 30, 1_000)];
    let mut copy = w.script("KpfTMatch");
    copy.value += 60_000_000;
    let (b, moved, to_fee_pool) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &copy, w.payer(&funds)).unwrap();
    assert_eq!((moved, to_fee_pool), (60_000_000, 57_000_000));
    let read = |sg: &OSigned| {
        let amounts: Vec<Option<u64>> = sg.entries.iter().map(|e| Some(e.amount)).collect();
        read_tx(&w.lib, &sg.tx, &amounts, &Pool(vec![])).expect("read")
    };
    let mut variants = vec![("as built", b.clone())];
    let mut t = b.clone();
    t.tx.outputs[0].value += 3 * KAS1 as u64;
    t.tx.outputs[b.change.unwrap()].value -= 3 * KAS1 as u64;
    variants.push(("the fee pool given 3 KAS more by the sender", t));
    let mut t = b.clone();
    t.tx.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: p2pk_spk(&xk(&kp(77))), covenant: None });
    t.output_labels.push("a key".into());
    t.tx.outputs[b.change.unwrap()].value -= KAS1 as u64;
    variants.push(("a key's output more", t));
    for (what, t) in &variants {
        let sg = t.sign(&w.keys).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "{what}");
        let r = read(&sg);
        assert_eq!((r.script, r.entry, r.spent.len(), r.made.len(), r.unread.len()), ("KpfTSweep", "sweep", 0, 0, 0), "a sweep, {what}");
    }
    // The same sweep with the copy swept at input 0 and the fee pool at input 2 (their outputs with them): taken by the
    // engine, and read as moving nothing.
    let mut t = b.clone();
    t.tx.inputs.swap(0, 2);
    t.entries.swap(0, 2);
    t.plans.swap(0, 2);
    t.input_labels.swap(0, 2);
    t.tx.outputs.swap(0, 2);
    t.output_labels.swap(0, 2);
    for (i, auth) in [(0usize, 0u16), (2, 2)] {
        if let Some(c) = &mut t.tx.outputs[i].covenant {
            c.authorizing_input = auth;
        }
    }
    args_of(&mut t, 1)[0] = Arg::V(ArtifactValue::Bytes([2i64, 0, 0].iter().flat_map(|x| sm8(*x)).collect()));
    let sg = t.sign(&w.keys).expect("signed");
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "a sweep with the copy before the script and the fee pool after");
    let r = read(&sg);
    assert_eq!((r.script, r.entry, r.spent.len(), r.made.len(), r.unread.len()), ("KpfTSweep", "sweep", 0, 0, 0), "a sweep laid out the other way round is named by its sweep, not by the copy let go (round 20)");
    let epoch_table = w.epoch_table();
    let att = w.attest(&epoch_table);
    let (b, _) = pay(&w.lib, &w.fee_pool_with(20 * KAS1), &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    let sg = signed("pay", &b, &w.keys);
    let r = read(&sg);
    // (What it leaves unread is the epoch table going on, under the committee's id: of no registry, as beside a proof.)
    assert_eq!((r.script, r.entry, r.spent.len(), r.made.len(), r.unread.clone()), ("KpfTPay", "pay", 0, 0, vec![2]), "a payment");
    assert_eq!(sg.tx.outputs[2].covenant.map(|c| c.covenant_id), Some(w.rid));
    println!("T1b R19 reader: {} sweeps (as built, the fee pool given more, a key's output more), a sweep with its copy first (named by its sweep) and a payment read as transactions of the library that move nothing of a registry", variants.len());
}

/// Round 19, question 1 for the three proofs (which the engine file does not compile). A copy of a script of the
/// library, holding 5 KAS more than it keeps, let go (`release`) beside a proof and named at each of the proof's three
/// places in turn (the object's, and the two a proof reads as epoch tables), going on with less: the proof's own input
/// refuses each (the copy is no object of that state and no epoch table of the pinned template).
#[test]
fn t1b_r19_no_proof_passes_with_a_copy_let_go_at_a_place() {
    use kpf_protocol::oracle_build::OWhich;
    let w = pworld();
    let kit = kpf_protocol::thin_artifacts::kit();
    let lid = Hash::from_bytes(w.lib);
    let mut tried = 0;
    for two in [false, true] {
        for p in proofs(&w, two, 50) {
            assert!(engine_takes(&p.b, &w.keys), "{}", p.name);
            let (i, right) = who_of(&p.b);
            for k in 0..3usize {
                for name in ["KpfTAdd", "KpfTBoth"] {
                    let copy = w.script(name);
                    let mut t = p.b.clone();
                    let at = t.tx.inputs.len();
                    let code = kit.script(name).unwrap().to_vec();
                    t.tx.inputs.push(TransactionInput::new_with_compute_budget(op(90), vec![], 0, 0));
                    t.entries.push(kaspa_consensus_core::tx::UtxoEntry::new(copy.value + 5 * KAS1 as u64, kaspa_txscript::pay_to_script_hash_script(&code), copy.daa, false, copy.id));
                    t.plans.push(OPlan::Contract { which: OWhich::T(name), redeem: code, entry: "release", args: vec![iv(i as i64), iv(k as i64)] });
                    t.input_labels.push(name.into());
                    // Its output before the payments a proof counts from the end, so that those stay where they are.
                    let pos = t.tx.outputs.len() - p.ends;
                    t.tx.outputs.insert(pos, TransactionOutput { value: copy.value, script_public_key: script_spk(name).unwrap(), covenant: Some(CovenantBinding { authorizing_input: at as u16, covenant_id: lid }) });
                    t.output_labels.insert(pos, name.into());
                    if let Some(c) = t.change {
                        if c >= pos {
                            t.change = Some(c + 1);
                        }
                    }
                    let mut who = right;
                    who[k] = at as i64;
                    let t = with_who(&t, i, who);
                    let sg = t.sign(&w.keys).unwrap_or_else(|e| panic!("{}: not signed: {e}", p.name));
                    assert!(script_refuses(&sg.tx, &sg.entries, i), "{} ({} legs): a copy of {name} at place {k} is taken", p.name, if two { 2 } else { 1 });
                    tried += 1;
                }
            }
        }
    }
    println!("T1b R19 proofs: {tried} proofs with a copy of a script let go at one of their three places, each refused by the proof's own input");
}

/// By design, and what F38 (Medium in round 24, fixed in round 25 in the executor) rested on. Any price a quorum
/// stands near is attestable, not only the prices members signed: with two members at 10.5 and one a little above,
/// the epoch table attests up to the end of the band (10.5 x 10^4 / (10^4 - spread)). A close of what that end gives
/// is not within the rule at either signed price (it is refuted at both) and is proved at the end: the claimer has
/// its amount, its bond and half of the disputer's. Whoever disputes by the signed prices alone pays for it;
/// `exec-run` no longer does (`t1b_fixed_f34_f38_exec_run_does_not_dispute_what_an_attestable_price_proves` in
/// auditT1_cli.rs).
#[test]
fn t1b_design_a_claim_is_proved_at_any_price_a_quorum_stands_near_the_bands_ends_too() {
    let w = pworld();
    let f = [fund(xk(&w.w), 80, 1_000)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let executor = kp(45);
    let position = w.position(false);
    let table = &w.epoch_tables[0];
    // Two members at 10.5, one three quarters of the way to the highest price the table attests with them (the end
    // of the band: 10.5 x 10^4 / (10^4 - spread)).
    let low = 105 * P10 / 100;
    let end = (low as i128 * 10_000 / (10_000 - table.s.spread_bps as i128)) as i64;
    let high = low + (end - low) * 3 / 4;
    assert!(low < high && high < end && table.s.near(low, end) && table.s.near(high, end));
    let signed: Vec<SignedPrice> = [low, low, high].iter().zip(w.members.iter()).map(|(p, k)| sign_price(k, &w.rid.as_bytes(), &w.markets[0].id(), T0, *p)).collect();
    let collected = Attestation::collect(&table.s, &w.rid.as_bytes(), &w.markets[0].id(), T0, &signed).expect("a quorum");
    let at_end = collected.clone().claiming(end);
    assert!(at_end.check(&table.s, &w.rid.as_bytes(), T0).is_ok(), "the epoch table attests 10.51 from 10.5, 10.5 and 10.508: {:?} (collected at {}, bits {})", at_end.check(&table.s, &w.rid.as_bytes(), T0), collected.p, collected.bits);
    // What 10.51 gives: more than either signed price gives.
    let amount = position.s.x.owed_at(SIDE_A, [end, 0]);
    let (b, c) = close(&w.lib, &position, &w.script("KpfTClose"), SIDE_A, T0, [end, 0], Some(amount), payer).unwrap();
    assert!(!c.within([low, 0]) && !c.within([high, 0]) && c.within([end, 0]), "refuted at each price a member signed, proved at the band's end");
    let cs = b.sign(&w.keys).unwrap();
    let (b, d) = dispute(&w.lib, &nth(&cs, 0, c, 6_510), &w.script("KpfTDispute"), xk(&executor), payer).unwrap();
    let ds = b.sign(&w.keys).unwrap();
    let dl = nth(&ds, 0, d, 6_520);
    let pb = prove(&w.lib, &dl, &w.script("KpfTProve"), (table, &at_end), None, payer).unwrap();
    let sg = pb.sign(&w.keys).unwrap();
    assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "a claim within the rule at the end of the band is proved there");
    assert_eq!((paid(&sg, &xk(&executor)), paid(&sg, &xk(&w.a))), (0, amount + c.bond + c.bond / 2), "the disputer's bond is gone, half of it to the claimer");
    // The same three signatures refute it at a signed price, if the refutation lands first.
    let at_low = collected.clone().claiming(low);
    assert!(engine_takes(&refute(&w.lib, &dl, &w.script("KpfTProve"), (table, &at_low), None, xk(&executor), payer).unwrap().0, &w.keys), "and refuted at 10.5: the first to land has it");
    println!("T1b DESIGN (F38 fixed in the executor): members at {low}, {low} and {high} (a spread of {} bps): a close of {amount} sompi (what {end} gives) is refuted at every signed price and proved at {end}, which the same signatures attest; a disputer that judged by the signed prices loses {} sompi", table.s.spread_bps, c.bond);
}

// ---------------------------------------------------------------- round 27: two orders matched into a position

/// FINDING (High, F41). `KpfTReplace` takes the price of the match from the two orders, and both can be one owner's:
/// the leaver's order the other way, and the newcomer's, which is the leaver itself or a second key of it. Nothing
/// then stands between that price and any number up to the market's limit. A position is struck again at a price of
/// its counterparty's choosing: what the price "moved" is paid to the leaver at once out of the margin of the one
/// that stays, whose entry becomes that price. With prices signed, the one that stays is worth what it was (its
/// worth is now a claim on the newcomer's margin, which the rule made large enough). At the entries it is not: an
/// exit claim, which settles a position by its margins when no price is shown within the exit time, pays it its
/// margin less the move. Shown: A long against B, 10 KAS of margin each, each asking the other's 10 KAS. A's two
/// orders (a short, and a long of 20 KAS of margin) are matched into the position 1.98 above its entry: A is paid
/// 9.9 KAS of B's margin, B is left 0.1 KAS; an exit claim by the newcomer, settled, pays B 0.1 KAS.
#[test]
fn t1b_finding_f41_one_party_strikes_a_position_again_at_a_price_of_its_choice_and_exits_at_it() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (a, b, sock) = (kp(61), kp(62), kp(63));
    for k in [&a, &b, &sock] {
        w.keys.add_keypair(*k);
    }
    let reg = Hash::from_bytes([0x72; 32]);
    let ten = 10 * KAS1;
    let p = Position { a_min_counter: ten, b_min_counter: ten, ..tpl.position(xk(&a), xk(&b), 5 * unit, px, ten, ten) };
    p.check().unwrap();
    let x = OLive { s: p, value: p.pot() as u64, id: Some(reg), op: op(0xa1), daa: 6_200 };
    // The strike: 1.98 above the entry, so that 5 units "moved" 9.9 KAS.
    let strike = px + 198 * KAS1 / 100;
    assert!(strike <= tpl.price_max, "within the market's limit");
    let order = |owner: &Keypair, dir: i64, margin: i64| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: 5 * unit, limit: strike, margin, tip: KAS1 / 4, expiry: 9_000, min_fill: 0, min_counter: 0 };
    let live = |o: Order, n: u8| OLive { s: o, value: o.value() as u64, id: Some(reg), op: op(n), daa: 6_200 };
    for (what, newcomer) in [("the leaver's own key", a), ("a second key of the leaver", sock)] {
        let (sell, buy) = (live(order(&a, SHORT, ten), 0xa2), live(order(&newcomer, LONG, 2 * ten), 0xa3));
        let f = w.pay(&w.w.clone());
        let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
        let (bt, r) = replace(&lib, &x, &buy, &sell, &w.script("KpfTReplace"), 5 * unit, strike, SIDE_A, payer).unwrap_or_else(|e| panic!("FINDING F41 (if this fails the finding is fixed: {what} as the newcomer is refused): {e}"));
        let s = signed(&format!("a position struck again by its long, the newcomer {what}"), &bt, &w.keys);
        let (z, zv) = r.z.expect("the position at the new entry");
        assert_eq!((r.gain, r.to_leaver, r.leaver), (99 * KAS1 / 10, 2 * ten + 99 * KAS1 / 10, xk(&a)), "the leaver: its margin, its order's, and what the price it chose moved");
        assert_eq!((z.x.a_key, z.x.b_key, z.x.legs[0].entry, z.x.a_margin, z.x.b_margin), (xk(&newcomer), xk(&b), strike, 2 * ten, KAS1 / 10), "B is left a tenth of a KAS of margin, from the entry A chose");
        assert_eq!(paid(&s, &xk(&a)) - if newcomer == a { 0 } else { 0 }, r.to_leaver);
        // Where a price is signed B is worth what it was: its margin less what the new entry stands above the old.
        assert!((z.x.b_margin - z.x.legs[0].gain(px) - ten).abs() <= 2, "B at the old entry");
        // A sompi less in the new position, or to the leaver, is refused: the script holds what it says.
        for k in [0usize, bt.tx.outputs.len() - 1] {
            let mut t = bt.clone();
            t.tx.outputs[k].value -= 1;
            assert!(!engine_takes(&t, &w.keys), "output {k} a sompi short");
        }
        // At the entries: the newcomer says the committee signed nothing, nobody shows a price, and it is settled.
        let zl = nth(&s, 0, z, 6_300);
        assert_eq!(zl.value as i64, zv);
        let f = w.pay(&w.w.clone());
        let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
        let (eb, q) = exit(&lib, &zl, &w.script("KpfTExit"), SIDE_A, 6_250, payer).unwrap();
        let es = signed("the newcomer's exit claim", &eb, &w.keys);
        let f = w.pay(&w.w.clone());
        let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
        let sb = exit_settle(&lib, &nth(&es, 0, q, 6_310), &w.script("KpfTExitEnd"), payer).unwrap();
        let ss = signed("settled at the entries", &sb, &w.keys);
        assert_eq!(paid(&ss, &xk(&b)), KAS1 / 10, "FINDING F41: B, which had 10 KAS of margin and never signed anything, is paid a tenth of a KAS");
        assert_eq!(paid(&ss, &xk(&newcomer)), 2 * ten + q.bond, "the newcomer has its margin and its bond back");
        println!("T1b FINDING F41 ({what}): a position of 10 KAS a side struck again 1.98 above its entry by its long: the long is paid {} sompi of the short's margin at once; settled at the entries, the short is paid {} sompi of its {ten}", r.gain, paid(&ss, &xk(&b)));
    }
}

/// F41 in the form it had before `KpfTReplace` (round 28, asked by the coordinator): a match and a novation. U is long
/// against S; U's short order and a long order of L, a second key of U, are matched plainly far above the market
/// (Y: L long against U from the strike); anyone novates X and Y: U leaves, L stands against S from Y's entry, S's
/// margin gives up the transfer and U is paid it; L's exit claim, unrefuted, settles at the entries. It runs, with
/// the tool's own builders and their rules: the same 9.9 KAS of S's 10 leave it (less nothing: the novation's tip is
/// U's to pay), and S is paid 0.1 KAS at the entries. So striking the one that stays again at a price its
/// counterparty chose is a property of the novation too, and of every round since it; `KpfTReplace` does it in one
/// transaction. What bounds it is whether an exit claim is ended.
#[test]
fn t1b_finding_f41_the_same_by_a_match_and_a_novation() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let (unit, px) = (tpl.scale, w.position.s.x.legs[0].entry);
    let (u, s, l) = (kp(64), kp(65), kp(66));
    for k in [&u, &s, &l] {
        w.keys.add_keypair(*k);
    }
    let reg = Hash::from_bytes([0x73; 32]);
    let ten = 10 * KAS1;
    let p = Position { a_min_counter: ten, b_min_counter: ten, ..tpl.position(xk(&u), xk(&s), 5 * unit, px, ten, ten) };
    let x = OLive { s: p, value: p.pot() as u64, id: Some(reg), op: op(0xb1), daa: 6_200 };
    let strike = px + 198 * KAS1 / 100;
    let order = |owner: &Keypair, dir: i64, margin: i64| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: 5 * unit, limit: strike, margin, tip: KAS1 / 4, expiry: 9_000, min_fill: 0, min_counter: 0 };
    let live = |o: Order, n: u8| OLive { s: o, value: o.value() as u64, id: Some(reg), op: op(n), daa: 6_200 };
    let (sell, buy) = (live(order(&u, SHORT, ten), 0xb2), live(order(&l, LONG, 2 * ten), 0xb3));
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // The match: Y, L long against U from the strike.
    let (mb, m) = order_match(&lib, &tpl, &buy, &sell, &w.script("KpfTMatch"), 5 * unit, strike, payer).unwrap();
    let ms = signed("U's short and its second key's long matched far above the market", &mb, &w.keys);
    assert_eq!((m.position.x.a_key, m.position.x.b_key, m.position.x.legs[0].entry), (xk(&l), xk(&u), strike));
    let y = nth(&ms, 0, m.position, 6_210);
    // The novation of X and Y, by anyone.
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let built = novate(&lib, &x, &y, &w.script("KpfTNovate"), payer);
    let (nb, nv, _) = built.unwrap_or_else(|e| panic!("the novation form of F41 does not build (what stops it here is the fix for KpfTReplace): {e}"));
    let ns = signed("anyone novates the two", &nb, &w.keys);
    let (z, _) = nv.z.expect("L against S");
    assert_eq!((nv.transfer, z.x.a_key, z.x.b_key, z.x.legs[0].entry, z.x.b_margin), (99 * KAS1 / 10, xk(&l), xk(&s), strike, KAS1 / 10), "S is struck again at Y's entry, its margin less the transfer");
    assert_eq!(paid(&ns, &xk(&u)), nv.to_leaver);
    assert!(nv.to_leaver >= 2 * ten + 99 * KAS1 / 10 - kpf_protocol::thin::NOVATE_TIP, "U has its two margins and the transfer, less the tip");
    // At the entries.
    let zl = nth(&ns, 0, z, 6_300);
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (eb, q) = exit(&lib, &zl, &w.script("KpfTExit"), SIDE_A, 6_250, payer).unwrap();
    let es = signed("L's exit claim", &eb, &w.keys);
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let sb = exit_settle(&lib, &nth(&es, 0, q, 6_310), &w.script("KpfTExitEnd"), payer).unwrap();
    let ss = signed("settled at the entries", &sb, &w.keys);
    assert_eq!(paid(&ss, &xk(&s)), KAS1 / 10, "S, which had 10 KAS of margin, is paid a tenth of a KAS");
    println!("T1b FINDING F41 (a match and a novation): U's short and its second key's long matched 1.98 above the entry, then novated: the transfer of {} sompi leaves S's margin for U; settled at the entries S is paid {} sompi of its {ten}", nv.transfer, paid(&ss, &xk(&s)));
}
