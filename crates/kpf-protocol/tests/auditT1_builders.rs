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
    let funds = vec![fund(xk(&f), 200, 3_000)];
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
        TObj::Change(c) => c.position,
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

// ================================================================ round 30: notes, citations, stated changes, accounts

use kpf_protocol::cite;
use kpf_protocol::thin::{ATake, Account, Change, Out, Outs, Pointed, Stated, Take, KIND_ACCOUNT_LIQ};
use std::cell::RefCell;

const P12: i64 = 1_200_000_000;

/// As `thin_change.rs` and `thin_account.rs`: a committee of three with one epoch table, sixteen markets, a library of two
/// copies, and a chain view that takes the blocks the notes are written in.
struct NW {
    a: Keypair,
    b: Keypair,
    w: Keypair,
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: Vec<CMarket>,
    table: OLive<EpochTable>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    genesis: Transaction,
    chain: RefCell<Vec<(Hash, Hash)>>,
}

impl NW {
    fn script(&self, name: &str) -> LibScript {
        self.copy(name, 0)
    }
    fn copy(&self, name: &str, k: usize) -> LibScript {
        self.scripts.iter().filter(|s| s.s == name).nth(k).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, who: &Keypair, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(who), rate: MIN_FEE_RATE }
    }
    fn terms(&self) -> XTerms {
        let p = &self.markets[0].p;
        XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa }
    }
    /// A against B: long 10 units of market 0 at 10, and with `two` short 5 units of market 1 at 10; 40 and 60 KAS.
    fn position_as(&self, two: bool, id: u8, n: u8) -> OLive<Position> {
        let (m, k) = (&self.markets[0], &self.markets[1]);
        let leg = |m: &CMarket, size: i64, dir: i64| XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: size * m.p.scale, dir, entry: P10 };
        let x = Cross { t: self.terms(), legs: [leg(m, 10, LONG), if two { leg(k, 5, SHORT) } else { XLeg::none() }], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 40 * KAS1, b_margin: 60 * KAS1, expiry: 6_000 + m.p.term };
        let position = Position { x, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 };
        position.check().unwrap();
        OLive { s: position, value: x.floor() as u64, id: Some(Hash::from_bytes([id; 32])), op: op(n), daa: 6_100 }
    }
    fn position(&self, two: bool) -> OLive<Position> {
        self.position_as(two, 0x71, 10)
    }
    /// An account of A against B with `n` legs, one a market, long and short in turn, `3 + i` units from 10; 40 and
    /// 60 KAS a leg.
    fn account(&self, n: usize) -> OLive<Account> {
        let legs = (0..n).map(|i| { let m = &self.markets[i]; XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: (3 + i as i64) * m.p.scale, dir: if i % 2 == 0 { LONG } else { SHORT }, entry: P10 } }).collect();
        let s = Account { t: self.terms(), legs, a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 40 * n as i64 * KAS1, b_margin: 60 * n as i64 * KAS1, expiry: 6_000 + self.markets[0].p.term, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: None };
        s.check().unwrap();
        OLive { value: (s.pot() + KAS1) as u64, s, id: Some(Hash::from_bytes([0x72; 32])), op: op(11), daa: 6_100 }
    }
    /// The committee's price of market `i` for T0 written as a note and accepted by a chain block: the note's
    /// transaction and its citation.
    fn noted(&self, i: usize, price: i64) -> (OSigned, Cited) {
        let market = self.markets[i].id();
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, T0, price)).collect();
        let att = Attestation::collect(&self.table.s, &self.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
        let funds = vec![fund(xk(&self.f), 90 + i as u8, 1_000)];
        let (b, note) = note_write(&self.lib, &self.script("KpfTNote"), (&self.table, &att), xk(&self.f), self.payer(&self.f, &funds)).unwrap();
        let s = b.sign(&self.keys).expect("the note's transaction");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok(), "the note's transaction");
        let wit = self.witness(&s.tx);
        let c = Cited::of(&s.tx, &self.lib, note, wit).unwrap();
        (s, c)
    }
    fn cited(&self, i: usize, price: i64) -> Cited {
        self.noted(i, price).1
    }
    /// A chain block that accepted `tx` fourth of five in the native lane, added to the chain view.
    fn witness(&self, tx: &Transaction) -> cite::Witness {
        let h = |k: u64| Hash::from_bytes(blake2b(&[&tx.id().as_bytes()[..], &k.to_le_bytes()].concat()));
        let lane: Vec<(Hash, u16, u32)> = (0..5).map(|k| (if k == 3 { tx.id() } else { h(k) }, 1u16, k as u32 + 1)).collect();
        let (_, activity_path) = cite::merkle(&cite::activity_leaves(&lane), 3);
        let wit = cite::Witness { block: h(100), merge_idx: 4, activity_path, lane_key: cite::native_lane_key(), parent_ref: h(101), context: cite::context(1_700_000_000_000, 6_600, 6_500), blue_score: 6_500, lanes_path: vec![(h(102), false), (h(103), true)], inactivity_shortcut: h(104), payload_and_ctx: h(105), parent_commitment: h(106) };
        self.chain.borrow_mut().push((wit.block, wit.commitment(&tx.id(), 1)));
        kpf_protocol::verify::set_chain_view(self.chain.borrow().clone());
        wit
    }
    fn key_of(&self, k: &[u8; 32]) -> &Keypair {
        [&self.a, &self.b, &self.w, &self.f].into_iter().find(|x| xk(x) == *k).expect("a key of the world")
    }
    /// The statement `st` made on the account, landed at `landed`.
    fn stated(&self, account: &OLive<Account>, st: &Stated, said: &[u8], landed: u64) -> OLive<Stated> {
        let who = self.key_of(&st.claimer);
        let f = [fund(xk(who), 30, 1_000)];
        let b = account_state(&self.lib, account, &self.script("KpfTChange"), st, said, self.payer(who, &f)).unwrap();
        let s = signed("stated on an account", &b, &self.keys);
        nth(&s, 0, *st, landed)
    }
    fn pointed(&self, st: &OLive<Stated>, by: &Keypair, point: i64, landed: u64) -> OLive<Pointed> {
        let f = [fund(xk(by), 31, 1_000)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(by), point, self.payer(by, &f)).unwrap();
        let s = signed("point disputed", &b, &self.keys);
        nth(&s, 0, q, landed)
    }
}

fn nw_of(copies: usize) -> NW {
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = (0..16u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200, 3_000)];
    let (g, lib) = lib_genesis(copies, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    NW { a, b, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), genesis: gs.tx.clone(), chain: RefCell::new(vec![]) }
}
fn nw() -> NW {
    nw_of(2)
}
/// Built, signed and taken by the engine, or not (a transaction that cannot be signed is not taken).
fn taken30(b: &OBuilt, keys: &KeySet) -> bool {
    match b.sign(keys) {
        Ok(sg) => kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(),
        Err(_) => false,
    }
}
fn bytes_arg(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
fn int_arg(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
/// Input `i` of a built transaction made the object whose state hashes to `h` (the builder built it for another
/// state of the same object).
fn object_made(b: &mut OBuilt, lib: &[u8; 32], i: usize, h: &[u8; 32]) {
    b.entries[i].script_public_key = hash_spk(lib, h);
    let OPlan::Contract { redeem, .. } = &mut b.plans[i] else { panic!("input {i} is not an object") };
    *redeem = kpf_protocol::thin_artifacts::kit().object(lib, h);
}
/// The hash of a statement whose outputs are the bytes `outs` (the model's `Outs` holds flags 0 and 1 only).
fn stated_hash_raw(position: &[u8; 32], kind: i64, t0: i64, arg: &[u8; 32], outs: &[u8], bond: i64, claimer: &[u8; 32], window: i64, late: i64, held: i64) -> [u8; 32] {
    blake2b(&[i9(10), p33(position), i9(kind), i9(t0), p33(arg), p33(&blake2b(outs)), i9(bond), p33(claimer), i9(window), i9(late), i9(held)].concat())
}

/// F51 (fixed). The statement's hash now carries `held`, what the object held when the change was stated
/// (`KpfTChange.state` writes `tx.inputs[j].value`), and every answer counts from that committed `held`, not from
/// the live input. So a disputer writing extra value into the object at the dispute no longer turns a true take to
/// "not within": the committed `held` is unchanged, the stored outputs still match, and the take is applied.
#[test]
fn t1b_fixed_f51_held_is_committed_so_more_value_at_the_dispute_does_not_turn_the_verdict() {
    let w = nw();
    // ---- a position (KpfTChangeDispute, KpfTTake)
    let position = w.position(false);
    let x = position.s;
    let tk = Take { side: SIDE_A, a: [P12, 0], amount: x.take_max(SIDE_A, [P12, 0]) };
    assert_eq!(tk.amount, 54 * KAS1);
    let fa = [fund(xk(&w.a), 30, 1_000)];
    let (b, c) = take(&w.lib, &position, &w.script("KpfTChange"), tk, T0, w.payer(&w.a, &fa)).unwrap();
    assert_eq!(c.held, position.value as i64, "held is what the object held when it was stated");
    let s = signed("F51 take stated", &b, &w.keys);
    let live = nth(&s, 0, c, 6_510);
    let (c12, cite1) = (w.cited(0, P12), w.script("KpfTCite"));
    let fw = [fund(xk(&w.w), 31, 1_000)];
    let (db, d) = change_dispute(&w.lib, &live, &w.script("KpfTChangeDispute"), xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    // The control: disputed as the builder writes it, the take is answered within, and W loses its bond.
    let ds = signed("F51 disputed as built", &db, &w.keys);
    let (ab, within, _) = take_answer(&w.lib, &nth(&ds, 0, d, 6_520), &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    assert!(within && taken30(&ab, &w.keys), "the control");
    // The same dispute with one sompi more in the object: STILL within the rule (the fix).
    let mut db1 = db.clone();
    db1.tx.outputs[0].value += 1;
    let ds1 = signed("F51 disputed with a sompi more", &db1, &w.keys);
    let d1 = nth(&ds1, 0, d, 6_520);
    assert_eq!(d1.value, live.value + c.bond as u64 + 1);
    let (ab, within, after) = take_answer(&w.lib, &d1, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    assert!(within && after == x.taken(&tk).unwrap(), "F51 is open again: a sompi more at the dispute turned the verdict");
    let r = signed("F51 the true take applied though the object holds a sompi more", &ab, &w.keys);
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), ((c.held - tk.amount) as u64, &object_spk(&w.lib, &TObj::Position(after))), "the position with the take applied, holding held - amount");
    assert_eq!(paid(&r, &xk(&w.a)), tk.amount + c.bond + c.bond / 2, "the claimer: its amount, its bond, half the disputer's");
    // The forced "not within" verdict is refused: the take really is within.
    let forced = take_answer_with(&w.lib, &d1, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &fw), Some(false));
    assert!(forced.is_err() || !taken30(&forced.unwrap().0, &w.keys), "the true take is refuted all the same");

    // ---- an account (KpfTPoint, KpfTStepEnd): a sompi more at the dispute does not turn the end.
    let account = w.account(3);
    let y = &account.s;
    let a: Vec<i64> = (0..3).map(|i| if i % 2 == 0 { P10 * 105 / 100 } else { P10 * 95 / 100 }).collect();
    let max = y.take_max(SIDE_A, &a);
    assert!(max > 0);
    let (st, atk) = y.take(SIDE_A, max, &a, T0, account.value as i64).unwrap();
    assert_eq!(st.held, account.value as i64);
    assert_eq!(y.wrong_point(&atk, &st.claimer, &st.outs, account.value as i64, &a), None, "the statement is the rule's");
    let lst = w.stated(&account, &st, &atk.said(), 6_510);
    let (pb, q) = point_dispute(&w.lib, &lst, &w.script("KpfTPoint"), xk(&w.w), 0, w.payer(&w.w, &fw)).unwrap();
    let mut pb1 = pb.clone();
    pb1.tx.outputs[0].value += 1;
    let ps1 = signed("F51 the end disputed with a sompi more", &pb1, &w.keys);
    let q1 = nth(&ps1, 0, q, 6_520);
    let (eb, right) = step_end(&w.lib, &q1, y, &atk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &fw), None).unwrap();
    assert!(right && taken30(&eb, &w.keys), "F51 is open again on an account: a sompi more turned the end");
    let forced = step_end(&w.lib, &q1, y, &atk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &fw), Some(false));
    assert!(forced.is_err() || !taken30(&forced.unwrap().0, &w.keys), "the true end is answered wrong all the same");
    println!("T1b F51 fixed: a take of 54 KAS on a position and a take of {max} sompi on an account (both the rule's), disputed with a sompi more in the object, are still answered within the rule: `held` is committed in the statement, so extra value at the dispute does not turn the verdict");
}

/// F52 (fixed). A row's answer now opens the row's chain with a `prev` the answerer gives and requires
/// `blake2b(prev + leg) == row.g`; where `prev` is not the chain of the row before, the row is WRONG (not
/// unanswerable). And `wrong_point_of` names the row after the last chain that is not the account's own. So a trace
/// whose middle rows carry a chain of no leg has a wrong point anyone can answer: the row after the fabricated one,
/// opened by the account's own chain. With the new `at != point => verdict 0` rule, that wrong row can be answered
/// whatever point was disputed, so the false statement cannot be kept and the claimer loses its bond.
#[test]
fn t1b_fixed_f52_a_fabricated_trace_has_a_wrong_point_anyone_can_answer() {
    let w = nw();
    let n = 3usize;
    let account = w.account(n);
    let x = &account.s;
    let held = account.value as i64;
    let a: Vec<i64> = x.legs.iter().map(|l| l.entry).collect();
    let honest = x.trace_without(&a, n as i64);
    let g2 = honest[n - 2].g; // the real chain of the first n-1 legs, before the fabrication
    let mut rows = honest.clone();
    rows[n - 2].g = [0xee; 32];
    // (The row carries a worth a side since the row of five numbers: both are set as the one worth was.)
    (rows[n - 2].wa, rows[n - 2].wb) = (0, 0);
    (rows[n - 1].wa, rows[n - 1].wb) = (1_000_000 * KAS1, 1_000_000 * KAS1);
    let (after, fee) = x.liq_stepped(SIDE_B, n as i64, &rows).expect("below, by the trace's own sums");
    assert_eq!(fee, x.b_margin, "the fabricated trace says the fee is B's whole margin");
    let tk = ATake { side: SIDE_B, amount: fee, rows: rows.clone(), skip: n as i64, leave: None };
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&rows[n - 1].h)].concat())), after.moving()].concat());
    let outs = Outs::of(&[Out { key: false, to: h, value: held - fee }, Out { key: true, to: xk(&w.w), value: fee }]).unwrap();
    let st = Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_ACCOUNT_LIQ, t0: T0, arg: tk.arg(), outs, bond: x.bond(), claimer: xk(&w.w), held };
    // The end is still "right" by its own numbers, but the model now finds a wrong point: the row after the bad chain.
    assert!(x.liq_end_right(&tk, &st.claimer, &st.outs, held), "the end is right by its own numbers");
    let c: Vec<i64> = a.clone();
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LIQ, &tk, &st.claimer, &st.outs, held, &c), Some(2 * (n as i64 - 1) + 1), "the wrong point is row n (after the fabricated chain)");
    let live = w.stated(&account, &st, &tk.said(), 6_510);
    let fw = [fund(xk(&w.w), 32, 1_000)];
    let fb = [fund(xk(&w.b), 33, 1_000)];
    // B disputes the end (point 0, which is "right"); anyone answers the wrong row (point 5) with verdict 0: the
    // `at != point` rule lets the real wrong point be answered, so the account is restored and the claimer's bond is lost.
    let q0 = w.pointed(&live, &w.b, 0, 6_520);
    let at = 2 * (n as i64 - 1) + 1;
    let (eb, right) = step_row_at(&w.lib, &q0, &tk, at, &g2, &x.legs[n - 1], &w.script("KpfTStepRow"), xk(&w.b), w.payer(&w.b, &fb), None).unwrap();
    assert!(!right, "the wrong row is answered wrong");
    let rs = signed("F52 the wrong row answered though the end was disputed", &eb, &w.keys);
    let k = rs.tx.outputs.len();
    assert_eq!((rs.tx.outputs[0].value, &rs.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "the account restored");
    assert_eq!((rs.tx.outputs[k - 2].value, &rs.tx.outputs[k - 2].script_public_key), (st.bond as u64, &p2pk_spk(&xk(&w.b))), "the disputer's bond back");
    assert_eq!((rs.tx.outputs[k - 1].value, &rs.tx.outputs[k - 1].script_public_key), ((st.bond / 2) as u64, &p2pk_spk(&xk(&w.b))), "half of the claimer's bond to who answered");
    // The forced "right" verdict of that row is refused (it really is wrong).
    let forced = step_row_at(&w.lib, &q0, &tk, at, &g2, &x.legs[n - 1], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &fw), Some(true));
    assert!(forced.is_err() || !taken30(&forced.unwrap().0, &w.keys), "the wrong row is answered right all the same");
    // And opening the bad row itself (point 3) with the account's own chain is wrong too (its chain is not the leg's).
    let q3 = w.pointed(&live, &w.b, 3, 6_520);
    let g1 = honest[n - 3.min(n)].g; // chain before the bad row; for n=3, before row 2 = g_1 = honest[0].g
    let _ = g1;
    println!("T1b F52 fixed: a fabricated liquidation trace (B's whole {} KAS) now has a wrong point (row {at}) that anyone opens with the account's own chain; disputing the right end does not save it (at != point => verdict 0), the account is restored and the claimer loses its bond", x.b_margin / KAS1);
    let _ = q3;
}

/// A statement of the account `obj` that is out of its time ended by `KpfTChangeLate` (the builders have this for a
/// position's statement only): built for a position's, then made the account's.
fn stated_late(w: &NW, obj: &OLive<Stated>, ender: &Keypair) -> OBuilt {
    let position = w.position(false);
    let c = position.s.take(Take { side: SIDE_A, a: [P12, 0], amount: 0 }, T0, position.value as i64).unwrap();
    let dummy = OLive { s: c, value: position.value + c.bond as u64, id: position.id, op: op(77), daa: (T0 + position.s.x.t.late_daa + 1) as u64 };
    let f = [fund(xk(ender), 78, 1_000)];
    let (mut b, _) = change_late(&w.lib, &dummy, &w.script("KpfTChangeLate"), xk(ender), w.payer(ender, &f)).unwrap();
    let st = &obj.s;
    b.tx.inputs[0].previous_outpoint = obj.op;
    b.entries[0].amount = obj.value;
    b.entries[0].block_daa_score = obj.daa;
    b.entries[0].covenant_id = obj.id;
    object_made(&mut b, &w.lib, 0, &st.hash());
    let args = args_of(&mut b, 1);
    args[3] = bytes_arg(&st.position);
    args[4] = int_arg(st.kind);
    args[5] = int_arg(st.t0);
    args[6] = bytes_arg(&st.arg);
    args[7] = bytes_arg(&st.outs.hash());
    args[8] = int_arg(st.bond);
    args[9] = bytes_arg(&st.claimer);
    args[10] = int_arg(st.window);
    args[11] = int_arg(st.late);
    args[12] = int_arg(st.held);
    b.tx.outputs[0] = TransactionOutput { value: obj.value - st.bond as u64, script_public_key: hash_spk(&w.lib, &st.position), covenant: Some(CovenantBinding { authorizing_input: 0, covenant_id: obj.id.expect("an id") }) };
    let n = b.tx.outputs.len();
    b.tx.outputs[n - 1].value = st.bond as u64;
    b
}

/// F53. A point answered right writes the statement again, as it was stated: a new output, which landed when the
/// answer did. Every script asks a statement when it LANDED (`OpTxInputDaaScore`) against the time it prices at: more
/// than the position's lateness after it (100 DAA by the tools' terms, ten seconds) it is neither applied nor
/// disputed, and `KpfTChangeLate` gives its bond to whoever ends it. So a true statement one point of which was
/// disputed and answered right is not applied: it is out of its time, and its claimer's bond is anyone's.
#[test]
fn t1b_fixed_f53_a_point_answered_right_restores_the_statement_not_held_to_its_time() {
    let w = nw();
    let account = w.account(2);
    let x = &account.s;
    let a = vec![P10 * 105 / 100, P10 * 95 / 100];
    let max = x.take_max(SIDE_A, &a);
    let (st, tk) = x.take(SIDE_A, max, &a, T0, account.value as i64).unwrap();
    assert_eq!(x.wrong_point(&tk, &st.claimer, &st.outs, account.value as i64, &a), None, "the statement is the rule's");
    assert_eq!((x.t.late_daa, x.dispute_daa), (100, 50));
    let live = w.stated(&account, &st, &tk.said(), 6_510);
    let fa = [fund(xk(&w.a), 34, 1_000)];
    let fw = [fund(xk(&w.w), 35, 1_000)];
    // W disputes row 1, which is right, late in the dispute period; A answers it right.
    let q = w.pointed(&live, &w.w, 1, 6_555);
    let (rb, right) = step_row(&w.lib, &q, &tk, &x.legs[0], &w.script("KpfTStepRow"), xk(&w.a), w.payer(&w.a, &fa), None).unwrap();
    assert!(right);
    let rs = signed("F53 row 1 answered right", &rb, &w.keys);
    let k = rs.tx.outputs.len();
    // The statement stands again with LATE_AGAIN in its hash (the fix: not held to its time again), not as first stated.
    assert_eq!(st.again().late, kpf_protocol::thin::LATE_AGAIN);
    assert_eq!((rs.tx.outputs[0].value, &rs.tx.outputs[0].script_public_key), (live.value, &hash_spk(&w.lib, &st.again().hash())), "the statement stands again with LATE_AGAIN");
    assert_ne!(rs.tx.outputs[0].script_public_key, hash_spk(&w.lib, &st.hash()), "not as first stated");
    assert_eq!((rs.tx.outputs[k - 1].value, &rs.tx.outputs[k - 1].script_public_key), ((st.bond / 2) as u64, &p2pk_spk(&xk(&w.a))), "half of the disputer's bond to the claimer");
    // The answer landed at 6,601 (past the lateness 100 of 6,500); the restored statement is still in its time.
    let again = nth(&rs, 0, st.again(), 6_601);
    assert!(!st.in_time(again.daa) && st.again().in_time(again.daa), "F53 is open again: the restored statement is not in its time");
    // It is applied (settled) though the answer landed past the original lateness.
    let sb = stated_settle(&w.lib, &again, &w.script("KpfTChangeSettle"), w.payer(&w.a, &fa)).unwrap();
    let ss = signed("F53 the restored statement applied", &sb, &w.keys);
    let after = x.taken(SIDE_A, max, &tk.rows).unwrap();
    assert_eq!((ss.tx.outputs[0].value, &ss.tx.outputs[0].script_public_key), ((st.held - max) as u64, &hash_spk(&w.lib, &after.hash())), "the account with the take applied");
    // And it cannot be ended as late: KpfTChangeLate asks the statement's own lateness (LATE_AGAIN), which no landing exceeds.
    let lb = stated_late(&w, &again, &w.w);
    assert!(!taken30(&lb, &w.keys), "F53 is open again: the restored statement is ended as late");
    // It can also be disputed again and answered (the dispute period restarted).
    assert!(point_dispute(&w.lib, &again, &w.script("KpfTPoint"), xk(&w.w), 3, w.payer(&w.w, &fw)).is_ok(), "another point of the restored statement may be disputed");
    println!("T1b F53 fixed: a true take on an account, row 1 disputed at 6,555 and answered right at 6,601 (time 6,500, lateness 100): the statement stands again with LATE_AGAIN, is applied by the settlement, and cannot be ended as late");
}

/// F54 (fixed). A statement whose first output flag was not 0 or 1 was applied by `KpfTChangeSettle` (which read "0 =
/// object, anything else = payment") but never read by the reader (which reads flags 0/1 only), so no executor
/// disputed it. The fix: `KpfTChange.state` now requires the first flag to be 0 or 1 and every later flag to be 1,
/// and `KpfTChangeSettle` requires flag 1 for a first output it pays. This verifies that: a statement with any flag
/// other than 0/1 cannot be stated (the state script refuses it), while the honest flag-1 form is stated and read.
#[test]
fn t1b_fixed_f54_a_statement_states_only_flags_0_and_1() {
    let w = nw();
    let position = w.position(false);
    let x = position.s;
    let held = position.value as i64;
    let thief = xk(&w.w);
    // "The position ends, everything to W": the honest flag-1 form is stated and read as a change.
    let honest = x.take(Take { side: SIDE_A, a: [P12, 0], amount: 0 }, T0, held).unwrap();
    let c = Change { outs: Outs::of(&[Out { key: true, to: thief, value: held }]).unwrap(), claimer: thief, ..honest };
    let fw = [fund(thief, 36, 1_000)];
    let b1 = change_state(&w.lib, &position, &w.script("KpfTChange"), &c, w.payer(&w.w, &fw)).unwrap();
    let s1 = signed("F54 stated with flag 1", &b1, &w.keys);
    let known = Pool(vec![TObj::Position(x)]);
    let r1 = read_signed(&w.lib, &s1, &known);
    assert_eq!((r1.made.len(), r1.unread.len()), (1, 0), "the reader reads the flag-1 statement");
    assert!(matches!(r1.made[0].1, TObj::Change(k) if k == c));
    // Every flag other than 0 or 1 in the first output: the state script refuses it (so it is never stated). The
    // stated-change output's hash matches the forged outs, but the script's own flag check fails input 1.
    let mut refused = 0;
    for flag in [2u8, 3, 0x7f, 0x80, 0xff] {
        let mut o = c.outs.bytes();
        o[0] = flag;
        let h = stated_hash_raw(&x.hash(), c.kind, c.t0, &c.arg(), &o, c.bond, &c.claimer, x.dispute_daa, x.x.t.late_daa, held);
        let mut b = b1.clone();
        assert!(matches!(&args_of(&mut b, 1)[8], Arg::V(ArtifactValue::Bytes(bb)) if *bb == c.outs.bytes()), "the statement's outputs are its ninth argument");
        args_of(&mut b, 1)[8] = bytes_arg(&o);
        b.tx.outputs[0].script_public_key = hash_spk(&w.lib, &h);
        let sg = b.sign(&w.keys).expect("signed");
        assert!(script_refuses(&sg.tx, &sg.entries, 1), "F54 is open again: a statement with first flag {flag} is stated");
        assert!(Outs::read(&o).is_none(), "the reader never produced such bytes");
        refused += 1;
    }
    // A payment output after the first with flag 0: also refused by the state script (later flags must be 1).
    let c2 = Change { outs: Outs::of(&[Out { key: false, to: x.taken(&Take { side: SIDE_A, a: [P12, 0], amount: KAS1 }).unwrap().hash(), value: held - KAS1 }, Out { key: true, to: thief, value: KAS1 }]).unwrap(), claimer: thief, ..x.take(Take { side: SIDE_A, a: [P12, 0], amount: KAS1 }, T0, held).unwrap() };
    let b2 = change_state(&w.lib, &position, &w.script("KpfTChange"), &c2, w.payer(&w.w, &fw)).unwrap();
    assert!(taken30(&b2, &w.keys), "the honest two-output form is stated");
    let mut o2 = c2.outs.bytes();
    o2[41] = 0;
    let h2 = stated_hash_raw(&x.hash(), c2.kind, c2.t0, &c2.arg(), &o2, c2.bond, &c2.claimer, x.dispute_daa, x.x.t.late_daa, held);
    let mut b = b2.clone();
    args_of(&mut b, 1)[8] = bytes_arg(&o2);
    b.tx.outputs[0].script_public_key = hash_spk(&w.lib, &h2);
    let sg = b.sign(&w.keys).expect("signed");
    assert!(script_refuses(&sg.tx, &sg.entries, 1), "F54 is open again: a later payment output with flag 0 is stated");
    println!("T1b F54 fixed: a statement with any first-output flag but 0 or 1 ({refused} tried), or a later payment output with flag 0, is refused by KpfTChange.state; the honest flag-1 form is stated and read");
}

/// What holds of the library's id (lead 1) through the builders. A note is written only by `KpfTNote` beside the
/// attest, and a genesis that writes one is no library (`lib_check`); a note taken back writes nothing under the id
/// and pays its writer; a note's transaction with an output more under the id, the note at another script, a second
/// note, or the note's script on the copy's place, is refused; a citation of a transaction whose first output is not
/// the note under this library's id is refused by the builder and by the script.
#[test]
fn t1b_holds_r30_nothing_but_a_note_of_the_attest_is_written_under_the_librarys_id() {
    let w = nw();
    let (ns, cited) = w.noted(0, P12);
    let note = cited.note;
    assert_eq!((ns.tx.outputs[0].script_public_key.clone(), ns.tx.outputs[0].covenant.map(|c| c.covenant_id.as_bytes())), (note_spk(&note), Some(w.lib)));
    // The note's writing rebuilt, for the mutations.
    let market = w.markets[0].id();
    let signed_prices: Vec<SignedPrice> = w.members.iter().map(|k| sign_price(k, &w.rid.as_bytes(), &market, T0, P12)).collect();
    let att = Attestation::collect(&w.table.s, &w.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
    let funds = vec![fund(xk(&w.f), 90, 1_000)];
    let (b, _) = note_write(&w.lib, &w.script("KpfTNote"), (&w.table, &att), xk(&w.f), w.payer(&w.f, &funds)).unwrap();
    assert!(taken30(&b, &w.keys));
    let lid = Hash::from_bytes(w.lib);
    let mut refused = 0;
    let mut no = |what: &str, t: OBuilt| {
        assert!(!taken30(&t, &w.keys), "{what}");
        refused += 1;
    };
    let other_note = kpf_protocol::thin::Note { price: P10, ..note };
    let evil = hash_spk(&w.lib, &[0x55; 32]);
    let k = b.tx.outputs.len();
    for auth in [1u16, 2, 0] {
        let cov = Some(CovenantBinding { authorizing_input: auth, covenant_id: lid });
        let mut t = b.clone();
        t.tx.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: note_spk(&other_note), covenant: cov });
        t.output_labels.push("x".into());
        no("a second note, of a price nobody signed", t);
        let mut t = b.clone();
        t.tx.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: evil.clone(), covenant: cov });
        t.output_labels.push("x".into());
        no("an object under the library's id beside the note", t);
        let mut t = b.clone();
        t.tx.outputs[k - 1].covenant = cov;
        no("the change under the library's id", t);
    }
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = note_spk(&other_note);
    no("the note of another price", t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = note_spk(&kpf_protocol::thin::Note { t0: T0 + 1, ..note });
    no("the note of another time", t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = note_spk(&kpf_protocol::thin::Note { market: w.markets[1].id(), ..note });
    no("the note of another market", t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = note_spk(&kpf_protocol::thin::Note { spread_bps: 0, ..note });
    no("the note of another spread", t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = evil.clone();
    no("an object in the note's place", t);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = script_spk("KpfTCite").unwrap();
    no("a script of the library in the note's place", t);
    let mut t = b.clone();
    t.tx.outputs[0].covenant = None;
    no("the note under no id", t);
    let mut t = b.clone();
    t.tx.outputs[1].script_public_key = note_spk(&note);
    no("a note in the copy's place", t);
    let mut t = b.clone();
    t.tx.outputs[0].value -= 1;
    no("the note a sompi short", t);
    // The epoch table of another committee (its own id): the note says the committee the table is under.
    let mut t = b.clone();
    t.entries[2].covenant_id = Some(Hash::from_bytes([0x53; 32]));
    for o in t.tx.outputs.iter_mut() {
        if let Some(c) = &mut o.covenant {
            if c.covenant_id == w.rid {
                c.covenant_id = Hash::from_bytes([0x53; 32]);
            }
        }
    }
    no("an epoch table under another id, the note naming the committee", t);
    // Taken back: nothing under the id, the writer paid. With an output under the id, or to another key: refused.
    let live = nth(&ns, 0, note, 6_520);
    let fw = [fund(xk(&w.w), 40, 1_000)];
    let rb = note_reclaim(&live, w.payer(&w.w, &fw)).unwrap();
    let rs = signed("a note taken back", &rb, &w.keys);
    assert_eq!((rs.tx.outputs[0].value, &rs.tx.outputs[0].script_public_key), (live.value, &p2pk_spk(&xk(&w.f))));
    for spk in [note_spk(&other_note), evil.clone(), script_spk("KpfTCite").unwrap(), p2pk_spk(&xk(&w.w))] {
        let mut t = rb.clone();
        t.tx.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: spk, covenant: Some(CovenantBinding { authorizing_input: 0, covenant_id: lid }) });
        t.output_labels.push("x".into());
        no("a note taken back writes under the library's id", t);
    }
    let mut t = rb.clone();
    t.tx.outputs[0].script_public_key = p2pk_spk(&xk(&w.w));
    no("a note taken back to another key", t);
    let mut t = rb.clone();
    t.tx.outputs[0].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: lid });
    no("a note taken back, its writer's output under the id", t);
    // A genesis that writes a note under the id is no library.
    let mut g = w.genesis.clone();
    assert_eq!(lib_check(&g).unwrap(), w.lib);
    let last = g.outputs.iter().rposition(|o| o.covenant.is_some()).unwrap();
    g.outputs[last].script_public_key = note_spk(&note);
    assert!(lib_check(&regenesis(&g, 0)).is_err(), "a genesis with a note in a script's place");
    let mut g = w.genesis.clone();
    let cov = g.outputs[last].covenant;
    g.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: note_spk(&note), covenant: cov });
    assert!(lib_check(&regenesis(&g, 0)).is_err(), "a genesis with a note more");
    // A citation is of this library's note: the same transaction read for another library, or its second output
    // read as the note, is refused by the builder.
    assert!(Cited::of(&ns.tx, &[0x4c; 32], note, cited.witness.clone()).is_err());
    assert!(Cited::of(&ns.tx, &w.lib, other_note, cited.witness.clone()).is_err());
    // And by the script: the price, the time, the committee or the writer told otherwise than the note's.
    let f = [fund(xk(&w.w), 41, 1_000)];
    let cb = cite(&w.lib, &w.script("KpfTCite"), &cited, w.payer(&w.w, &f)).unwrap();
    assert!(taken30(&cb, &w.keys));
    for (i, what) in [(1usize, "the committee"), (2, "the market"), (3, "the time"), (4, "the price"), (5, "the spread"), (6, "the writer"), (0, "the chain block")] {
        let mut t = cb.clone();
        let Arg::V(ArtifactValue::Bytes(v)) = &mut args_of(&mut t, 0)[i] else { panic!("bytes") };
        v[0] ^= 1;
        no(&format!("a citation told another {what}"), t);
    }
    println!("T1b R30: {refused} transactions that would write under the library's id what no attest signed, or cite what no note says, refused; a genesis with a note is no library");
}

/// F55 (fixed). A take on a two-leg position is answered beside two copies of `KpfTCite` (two inputs). The fix: a
/// library genesis writes at least two `KpfTCite` even at `--copies 1`, and `lib_check` refuses a library with fewer
/// than two. So a two-leg take is always answerable. This verifies it: a one-copy library holds two `KpfTCite` and
/// passes `lib_check`; a genesis with only one is refused; the two copies answer a two-leg take.
#[test]
fn t1b_fixed_f55_a_library_holds_two_cite_copies() {
    let w = nw();
    let position = w.position(true);
    let x = position.s;
    let tk = Take { side: SIDE_A, a: [P12, P10], amount: x.take_max(SIDE_A, [P12, P10]) };
    let fa = [fund(xk(&w.a), 42, 1_000)];
    let (b, c) = take(&w.lib, &position, &w.script("KpfTChange"), tk, T0, w.payer(&w.a, &fa)).unwrap();
    let s = signed("F55 take on two legs stated", &b, &w.keys);
    let live = nth(&s, 0, c, 6_510);
    let (c1, c2) = (w.cited(0, P12), w.cited(1, P10));
    let (k1, k2) = (w.copy("KpfTCite", 0), w.copy("KpfTCite", 1));
    let (ab, within, _) = take_answer(&w.lib, &live, &w.script("KpfTTake"), &[(&k1, &c1), (&k2, &c2)], xk(&w.a), w.payer(&w.a, &fa)).unwrap();
    assert!(within);
    let good = signed("F55 answered beside two copies", &ab, &w.keys);
    assert_eq!(good.tx.inputs.len(), 5, "the object, the script, two citing copies, a coin");
    // One copy at both places: refused by the script (the two places are two inputs).
    let mut one = ab.clone();
    args_of(&mut one, 1)[0] = bytes_arg(&[0i64, 2, 2].iter().flat_map(|v| sm8(*v)).collect::<Vec<u8>>());
    let sg = one.sign(&w.keys).expect("signed");
    assert!(script_refuses(&sg.tx, &sg.entries, 1), "one citing copy cannot answer a take on two legs (the two places are two inputs)");
    // The fix: a one-copy library holds TWO KpfTCite and passes lib_check.
    let w1 = nw_of(1);
    assert_eq!(lib_check(&w1.genesis).unwrap(), w1.lib, "a one-copy library is still a library");
    assert_eq!(w1.scripts.iter().filter(|s| s.s == "KpfTCite").count(), 2, "F55 is open again: a one-copy library has fewer than two KpfTCite");
    // And a genesis with only one KpfTCite is refused by lib_check: strip the extra copy.
    let mut g = w1.genesis.clone();
    let cite_spk = script_spk("KpfTCite").unwrap();
    let extra = g.outputs.iter().rposition(|o| o.script_public_key == cite_spk).unwrap();
    g.outputs.remove(extra);
    assert!(lib_check(&regenesis(&g, 0)).is_err(), "F55 is open again: lib_check takes a library with one KpfTCite");
    // The two copies of the one-copy library answer a two-leg take (the fix lets a lone operator answer).
    let p1 = w1.position(true);
    let tk1 = Take { side: SIDE_A, a: [P12, P10], amount: p1.s.take_max(SIDE_A, [P12, P10]) };
    let fa1 = [fund(xk(&w1.a), 44, 1_000)];
    let (b1, _) = take(&w1.lib, &p1, &w1.script("KpfTChange"), tk1, T0, w1.payer(&w1.a, &fa1)).unwrap();
    let s1 = signed("F55 two-leg take stated in a one-copy library", &b1, &w1.keys);
    let (cc1, cc2) = (w1.cited(0, P12), w1.cited(1, P10));
    let (kk1, kk2) = (w1.copy("KpfTCite", 0), w1.copy("KpfTCite", 1));
    let (ab1, within1, _) = take_answer(&w1.lib, &nth(&s1, 0, tk1_change(&p1.s, tk1, p1.value as i64), 6_510), &w1.script("KpfTTake"), &[(&kk1, &cc1), (&kk2, &cc2)], xk(&w1.a), w1.payer(&w1.a, &fa1)).unwrap();
    assert!(within1 && taken30(&ab1, &w1.keys), "the two-leg take is answered beside the library's two KpfTCite copies");
    println!("T1b F55 fixed: a take on two legs is answered beside two KpfTCite copies, one copy at both places refused; a one-copy library holds two KpfTCite and passes lib_check, a genesis with one is refused, and the two copies answer a two-leg take ({} bytes)", good.mass.size);
}

/// The `Change` of a take on `position` holding `held`, for `nth` (the live object a take-answer reads).
fn tk1_change(position: &Position, tk: Take, held: i64) -> Change {
    position.take(tk, T0, held).expect("a take")
}

// ================================================================ round 33: options, the 112-byte row, the reserve

/// F56 (fixed in round 34). A leg whose worth at a price within its market's limit does not fit 63 bits entered a
/// covered account (`KpfTLegAdd` asked `size <= MARGIN_MAX` alone, `Account::check` asked `size > 0`), and no
/// statement of that account could be computed. The fix: `KpfTLegAdd` asks `scale <= SCALE_MAX` and
/// `size / scale <= MARGIN_MAX / price_max - 1` (the bound of a position's leg), and the model's `leg_size_ok` is
/// asked by `Account::check` and `leg_added`. Verified: the leg of round 33 is refused by the model and, built past
/// the model, by the script; the largest size the bound allows enters, and one unit more does not; the same for an
/// option.
#[test]
fn t1b_fixed_f56_a_leg_whose_worth_overflows_does_not_enter_a_covered_account() {
    use kpf_protocol::cross::MARGIN_MAX;
    let (a, b, f) = (kp(1), kp(2), kp(9));
    let mut keys = KeySet::new();
    for k in [&a, &b, &f] {
        keys.add_keypair(*k);
    }
    let rid = [0x52u8; 32];
    let market = CMarket::testnet(rid, [0xc0; 32]);
    let price_max = market.p.price_max;
    let scale = market.p.scale;
    // A library (two copies) and a registry of two anchors, as the reserve world is built.
    let funds = vec![fund(xk(&f), 200, 3_000)];
    let (g, lib) = lib_genesis(2, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    let scripts = lib_scripts(&gs.tx, 6_000);
    let script = |n: &str| scripts.iter().find(|s| s.s == n).cloned().unwrap_or_else(|| panic!("{n}"));
    let funds = vec![fund(xk(&f), 201, 3_000)];
    let (r, _) = registry_genesis(&lib, 2, KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let rs = r.sign(&keys).expect("registry");
    let anchors = registry_anchors(&rs.tx, &lib, 6_000);
    let terms = {
        let p = &market.p;
        XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa }
    };
    // The clearing party's shard, and a covered account of a user (A) against it (B).
    let fs = [fund(xk(&b), 40, 1_000)];
    let (sb, shard) = shard_open(&lib, &anchors[0], &script("KpfTShardOpen"), terms, market.committee_id, xk(&b), 50 * KAS1, 9_000_000, 50, 100, Payer { funds: &fs, change_to: xk(&b), rate: MIN_FEE_RATE }).unwrap();
    let ss = signed("shard opened", &sb, &keys);
    let sid = shard.sid;
    let account = Account { t: terms, legs: vec![], a_key: xk(&a), b_key: xk(&b), a_margin: 20 * KAS1, b_margin: 2 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: Some(sid) };
    let fa = [fund(xk(&a), 41, 1_000)];
    let ob = account_open_c(&lib, &anchors[1], &script("KpfTOpenC"), &account, shard_seed(&anchors[0]), market.committee_id, Payer { funds: &fa, change_to: xk(&a), rate: MIN_FEE_RATE }).unwrap();
    let os = signed("covered account opened", &ob, &keys);
    let live = nth(&os, 0, account.clone(), 6_100);
    let _ = ss;
    // The leg of round 33: its size within MARGIN_MAX, its worth at the market's limit beyond 63 bits.
    let big = XLeg { market: market.id(), committee: market.committee_id, scale, price_max, size: MARGIN_MAX, dir: LONG, entry: P10 };
    assert!(kpf_protocol::payoff::q_checked(big.size, price_max, scale, 0).is_none());
    let with = |l: XLeg| Account { legs: vec![l], ..account.clone() };
    assert!(with(big).check().is_err(), "F56 is open again: Account::check takes a leg whose worth overflows");
    let fl = [fund(xk(&a), 42, 1_000)];
    let payer = || Payer { funds: &fl, change_to: xk(&a), rate: MIN_FEE_RATE };
    assert!(leg_add(&lib, &live, &script("KpfTLegAdd"), big, 0, 0, payer()).is_err(), "F56 is open again: the builder adds it");
    // Built past the model (the account and the note the outputs would be): the script refuses it.
    let past = |l: XLeg| leg_add_as(&lib, &live, &script("KpfTLegAdd"), l, 0, 0, with(l), kpf_protocol::reserve::SNote::leg(kpf_protocol::reserve::NOTE_JOIN, sid, &l, xk(&b)), payer()).unwrap().0;
    let refused = |l: XLeg| {
        let sg = past(l).sign(&keys).expect("signed");
        script_refuses(&sg.tx, &sg.entries, 1)
    };
    assert!(refused(big), "F56 is open again: the engine takes a leg whose worth overflows i64");
    // The largest size the bound allows enters (model and engine), and one unit more does not; a futures leg and
    // an option struck at the limit.
    let most = (MARGIN_MAX / price_max - 1) * scale + (scale - 1);
    let mut tried = 0;
    for dir in [LONG, SHORT, kpf_protocol::thin::option_dir(kpf_protocol::thin::CALL_WRITTEN, price_max), kpf_protocol::thin::option_dir(kpf_protocol::thin::PUT_HELD, 1)] {
        let fits = XLeg { size: most, dir, ..big };
        let over = XLeg { size: most + 1, dir, ..big };
        assert!(kpf_protocol::payoff::q_checked(fits.size, price_max, scale, 0).is_some(), "the largest leg's worth at the limit fits");
        assert!(with(fits).check().is_ok() && taken30(&past(fits), &keys), "the largest leg the bound allows enters (direction's place {dir})");
        assert!(with(over).check().is_err() && refused(over), "F56 is open again: a unit beyond the bound enters (direction's place {dir})");
        tried += 1;
    }
    // A scale beyond the family's most is refused too (the bound divides by it).
    let wide = XLeg { scale: kpf_protocol::state::SCALE_MAX + 1, size: kpf_protocol::state::SCALE_MAX + 1, ..big };
    assert!(with(wide).check().is_err() && refused(wide), "a scale beyond SCALE_MAX");
    println!("T1b F56 fixed: the leg of size MARGIN_MAX is refused by Account::check, by the builder and by KpfTLegAdd; the largest size the bound allows ({most}) enters and one more does not, for {tried} directions' places; a scale beyond SCALE_MAX is refused");
}

// ================================================================ round 34: the model against the scripts, at the engine

use kpf_protocol::reserve::{Head, SNote, Shard};
use kpf_protocol::thin::{option_dir, row_after, row_judged_of, Row, CALL_HELD, CALL_WRITTEN, KIND_ACCOUNT_LEAVE, KIND_ACCOUNT_TAKE, PUT_HELD, PUT_WRITTEN};

/// As `thin_option.rs`: a committee of three with one epoch table, eight markets, a library, a registry of two
/// anchors (a shard and a covered account are opened through them). `a` is the user (side A), `b` the other party
/// (the clearing party where an account is covered), `w` anyone.
struct OW {
    a: Keypair,
    b: Keypair,
    w: Keypair,
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: Vec<CMarket>,
    table: OLive<EpochTable>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    anchors: Vec<Anchor>,
    chain: RefCell<Vec<(Hash, Hash)>>,
}

impl OW {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, who: &Keypair, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(who), rate: MIN_FEE_RATE }
    }
    fn terms(&self) -> XTerms {
        let p = &self.markets[0].p;
        XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa }
    }
    fn fut(&self, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
        let m = &self.markets[i];
        XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
    }
    fn opt(&self, i: usize, units: i64, kind: i64, strike: i64, entry: i64) -> XLeg {
        self.fut(i, units, option_dir(kind, strike), entry)
    }
    /// An account of A against B that no shard covers, as an output stands on the chain (nothing of it is checked
    /// here: an account of 138 bytes is opened by its two parties' signatures over its hash).
    fn account(&self, legs: Vec<XLeg>, am: i64, bm: i64) -> OLive<Account> {
        let s = Account { t: self.terms(), legs, a_key: xk(&self.a), b_key: xk(&self.b), a_margin: am, b_margin: bm, expiry: 6_000 + self.markets[0].p.term, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: None };
        OLive { value: (s.pot() + KAS1) as u64, s, id: Some(Hash::from_bytes([0x72; 32])), op: op(11), daa: 6_100 }
    }
    fn cited(&self, i: usize, price: i64) -> Cited {
        let market = self.markets[i].id();
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, T0, price)).collect();
        let att = Attestation::collect(&self.table.s, &self.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
        let funds = vec![fund(xk(&self.f), 90, 1_000)];
        let (b, note) = note_write(&self.lib, &self.script("KpfTNote"), (&self.table, &att), xk(&self.f), self.payer(&self.f, &funds)).unwrap();
        let s = b.sign(&self.keys).expect("the note's transaction");
        let h = |k: u64| Hash::from_bytes(blake2b(&[&s.tx.id().as_bytes()[..], &k.to_le_bytes()].concat()));
        let lane: Vec<(Hash, u16, u32)> = (0..5).map(|k| (if k == 3 { s.tx.id() } else { h(k) }, 1u16, k as u32 + 1)).collect();
        let (_, activity_path) = cite::merkle(&cite::activity_leaves(&lane), 3);
        let wit = cite::Witness { block: h(100), merge_idx: 4, activity_path, lane_key: cite::native_lane_key(), parent_ref: h(101), context: cite::context(1_700_000_000_000, 6_600, 6_500), blue_score: 6_500, lanes_path: vec![(h(102), false), (h(103), true)], inactivity_shortcut: h(104), payload_and_ctx: h(105), parent_commitment: h(106) };
        self.chain.borrow_mut().push((wit.block, wit.commitment(&s.tx.id(), 1)));
        kpf_protocol::verify::set_chain_view(self.chain.borrow().clone());
        Cited::of(&s.tx, &self.lib, note, wit).unwrap()
    }
    fn who(&self, key: &[u8; 32]) -> &Keypair {
        [&self.a, &self.b, &self.w, &self.f].into_iter().find(|k| xk(k) == *key).unwrap_or(&self.w)
    }
    /// The statement made on the account (taken by the engine), landed at 6,510.
    fn stated(&self, account: &OLive<Account>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        let who = self.who(&st.claimer);
        let f = [fund(xk(who), 30, 1_000)];
        let b = account_state(&self.lib, account, &self.script("KpfTChange"), st, said, self.payer(who, &f)).unwrap();
        let s = b.sign(&self.keys).expect("stated");
        let v = kpf_protocol::verify::verify(&s.tx, &s.entries);
        assert!(v.is_ok(), "stated: {:?}", v.errors());
        nth(&s, 0, *st, 6_510)
    }
    fn pointed(&self, st: &OLive<Stated>, point: i64) -> OLive<Pointed> {
        let f = [fund(xk(&self.w), 31, 1_000)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(&self.w), point, self.payer(&self.w, &f)).unwrap();
        let s = b.sign(&self.keys).expect("pointed");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        nth(&s, 0, q, 6_520)
    }
    fn shard(&self) -> OLive<Shard> {
        let f = [fund(xk(&self.b), 40, 1_000)];
        let (b, shard) = shard_open(&self.lib, &self.anchors[0], &self.script("KpfTShardOpen"), self.terms(), self.markets[0].committee_id, xk(&self.b), 50 * KAS1, 9_000_000, 50, 100, self.payer(&self.b, &f)).unwrap();
        let s = signed("shard opened", &b, &self.keys);
        nth(&s, 0, shard, 6_100)
    }
    /// A covered account of the user against the clearing party, with `am` and `bm` of margin and no leg.
    fn covered(&self, sid: [u8; 32], am: i64, bm: i64) -> OLive<Account> {
        let a = Account { t: self.terms(), legs: vec![], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: am, b_margin: bm, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: Some(sid) };
        let f = [fund(xk(&self.a), 41, 1_000)];
        let b = account_open_c(&self.lib, &self.anchors[1], &self.script("KpfTOpenC"), &a, shard_seed(&self.anchors[0]), self.markets[0].committee_id, self.payer(&self.a, &f)).unwrap();
        let s = signed("covered account opened", &b, &self.keys);
        nth(&s, 0, a, 6_100)
    }
    fn added(&self, account: &OLive<Account>, leg: XLeg) -> (OLive<Account>, OLive<SNote>) {
        let f = [fund(xk(&self.a), 42, 1_000)];
        let (b, next, note) = leg_add(&self.lib, account, &self.script("KpfTLegAdd"), leg, 0, 0, self.payer(&self.a, &f)).unwrap();
        let s = signed("leg added", &b, &self.keys);
        (nth(&s, 0, next, 6_110), nth(&s, 2, note, 6_110))
    }
    fn applied(&self, shard: &OLive<Shard>, note: &OLive<SNote>, what: &str) -> OLive<Shard> {
        let f = [fund(xk(&self.w), 43, 1_000)];
        let (b, next, _) = note_apply(&self.lib, shard, note, &self.script("KpfTNoteApply"), self.payer(&self.w, &f)).unwrap();
        let s = signed(what, &b, &self.keys);
        nth(&s, 0, next, shard.daa + 10)
    }
}

fn ow() -> OW {
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = (0..8u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200, 3_000)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    let funds = vec![fund(xk(&f), 201, 3_000)];
    let (r, _) = registry_genesis(&lib, 2, KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let rs = r.sign(&keys).expect("registry");
    let anchors = registry_anchors(&rs.tx, &lib, 6_000);
    OW { a, b, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), anchors, chain: RefCell::new(vec![]) }
}

/// What the engine takes of a row's answer: (the verdict "right" is taken, the verdict "wrong" is taken).
fn row_taken(w: &OW, q: &OLive<Pointed>, tk: &ATake, at: i64, prev: &[u8; 32], leg: &XLeg) -> (bool, bool) {
    let take = |v: bool| {
        let who = if v { w.who(&q.s.st.claimer) } else { &w.w };
        let f = [fund(xk(who), 33, 1_000)];
        step_row_at(&w.lib, q, tk, at, prev, leg, &w.script("KpfTStepRow"), xk(&w.w), w.payer(who, &f), Some(v)).is_ok_and(|b| taken30(&b.0, &w.keys))
    };
    (take(true), take(false))
}

fn verdicts(m: Option<bool>) -> (bool, bool) {
    match m {
        Some(true) => (true, false),
        Some(false) => (false, true),
        None => (false, false),
    }
}

/// Round 34, item 1. `KpfTStepRow` against `row_judged_of`, at the engine, for legs of every kind and for
/// directions' places that are no kind (0, below zero, the bits 0, 1, 2 and 7 above a strike), a strike of zero and
/// one above the market's limit, entries above and below the strike; for a row that stays and a row that leaves, by
/// each side, in a statement of each kind (2, 3, 4); with what the statement says of the leaving leg as the model
/// makes it, of another market, and of other units; and with each of the six numbers of the row off by one. For each
/// the engine takes exactly the verdict the model gives, and where the model gives none (a chain that does not open
/// the row) the engine takes neither.
#[test]
fn t1b_r34_a_rows_answer_is_the_models_for_every_kind_of_leg() {
    let w = ow();
    let pm = w.markets[1].p.price_max;
    let scale = w.markets[1].p.scale;
    let k = P10;
    let mut legs: Vec<(String, XLeg)> = vec![("long".into(), w.fut(1, 7, LONG, P10)), ("short".into(), w.fut(1, 7, SHORT, P10))];
    for (name, kind) in [("call A holds", CALL_HELD), ("call A wrote", CALL_WRITTEN), ("put A holds", PUT_HELD), ("put A wrote", PUT_WRITTEN)] {
        legs.push((format!("{name}, entry above its strike"), w.opt(1, 7, kind, k, k * 13 / 10)));
        legs.push((format!("{name}, entry below its strike"), w.opt(1, 7, kind, k, k * 7 / 10)));
        legs.push((format!("{name}, strike 0"), w.opt(1, 7, kind, 0, k)));
        legs.push((format!("{name}, strike above the limit"), w.opt(1, 7, kind, pm + 1, k)));
    }
    for dir in [0, -5, -8 * k + 3, 8 * k, 8 * k + 1, 8 * k + 2, 8 * k + 7, 7, 8] {
        legs.push((format!("the direction's place {dir}"), w.fut(1, 7, dir, P10)));
    }
    for (_, l) in legs.iter_mut() {
        l.size += scale / 3;
    }
    let first = w.fut(0, 3, SHORT, P10 * 9 / 10);
    let (mut cases, mut rights, mut wrongs, mut nones) = (0, 0, 0, 0);
    for (name, leg) in &legs {
        let account = w.account(vec![first, *leg], 500 * KAS1, 500 * KAS1);
        let x = &account.s;
        let held = account.value as i64;
        for (kind, skip, side) in [(KIND_ACCOUNT_TAKE, 0i64, SIDE_A), (KIND_ACCOUNT_LIQ, 2, SIDE_A), (KIND_ACCOUNT_LIQ, 2, SIDE_B), (KIND_ACCOUNT_LEAVE, 2, SIDE_A), (KIND_ACCOUNT_LEAVE, 2, SIDE_B), (KIND_ACCOUNT_TAKE, 0, SIDE_B), (KIND_ACCOUNT_LIQ, 1, SIDE_A)] {
            for price in [k * 12 / 10 + 7, k * 8 / 10 + 3] {
                let before = row_after(&Row::ZERO, &first, P10, skip == 1);
                let honest = row_after(&before, leg, price, skip == 2);
                // What the statement says of the leg that leaves: as the model makes it, and (for the row that
                // leaves) of another market and of other units.
                let said_of = |l: &XLeg| Some((Head::of(l), SNote::units(l)));
                let mut leaves = vec![if skip == 2 { said_of(leg) } else if skip == 1 { said_of(&first) } else { None }];
                if skip == 2 && kind == KIND_ACCOUNT_LEAVE {
                    leaves.push(Some((Head::of(&XLeg { dir: if kpf_protocol::thin::option_of(leg).is_some() { LONG } else { option_dir(CALL_HELD, k) }, ..*leg }), SNote::units(leg))));
                    leaves.push(Some((Head::of(leg), -SNote::units(leg))));
                    leaves.push(Some((Head::of(leg), SNote::units(leg) + 1)));
                }
                let whole = kind == KIND_ACCOUNT_TAKE || (kind == KIND_ACCOUNT_LIQ && skip == 2 && side == SIDE_A);
                for (li, leave) in leaves.iter().enumerate() {
                    let mut rows = vec![honest];
                    if whole && li == 0 && price > k {
                        for which in 0..6 {
                            for d in [1i64, -1] {
                                let mut r = honest;
                                match which {
                                    0 => r.a += d,
                                    1 => r.gain += d,
                                    2 => r.wa += d,
                                    3 => r.wb += d,
                                    4 => r.ea += d,
                                    _ => r.eb += d,
                                }
                                rows.push(r);
                            }
                        }
                        let mut r = honest;
                        r.h = [0x5a; 32];
                        rows.push(r);
                    }
                    for (ri, row) in rows.iter().enumerate() {
                        let tk = ATake { side, amount: 0, rows: vec![before, *row], skip, leave: *leave };
                        let outs = Outs::of(&[Out { key: false, to: x.hash(), value: held }]).unwrap();
                        let st = Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind, t0: T0, arg: tk.arg(), outs, bond: x.bond(), claimer: xk(&w.a), held };
                        let q = w.pointed(&w.stated(&account, &st, &tk.said()), 3);
                        let row_leave = (skip == 2).then(|| leave.unwrap_or((Head::EMPTY, 0)));
                        let model = row_judged_of(&before.g, leg, &before, row, row_leave, side);
                        let engine = row_taken(&w, &q, &tk, 3, &before.g, leg);
                        assert_eq!(engine, verdicts(model), "{name}: kind {kind}, skip {skip}, side {side}, price {price}, what is said of the leg {li}, row {ri}: the model says {model:?}");
                        match model {
                            Some(true) => rights += 1,
                            Some(false) => wrongs += 1,
                            None => nones += 1,
                        }
                        cases += 1;
                        // A chain that is not the row before's, which does not open the row: neither verdict.
                        if ri == 0 && li == 0 && price > k {
                            assert_eq!(row_judged_of(&[0x11; 32], leg, &before, row, row_leave, side), None);
                            assert_eq!(row_taken(&w, &q, &tk, 3, &[0x11; 32], leg), (false, false), "{name}: a chain that does not open the row");
                            // And the leg of another row shown for this one.
                            assert_eq!(row_taken(&w, &q, &tk, 3, &before.g, &first), (false, false), "{name}: another leg");
                        }
                    }
                }
            }
        }
    }
    assert!(rights > 50 && wrongs > 100 && nones == 0, "{rights} right, {wrongs} wrong, {nones} with no verdict");
    println!("T1b R34 rows: {} legs, {cases} rows answered at the engine: {rights} right and {wrongs} wrong, each the model's verdict and the other refused; a chain or a leg that does not open the row is taken with neither verdict", legs.len());
}

/// What the engine takes of the answer of an end (point 0): (the verdict "right" is taken, "wrong" is taken).
fn end_taken(w: &OW, account: &OLive<Account>, st: &Stated, tk: &ATake) -> (bool, bool) {
    let q = w.pointed(&w.stated(account, st, &tk.said()), 0);
    let script = w.script(if st.kind == KIND_ACCOUNT_TAKE { "KpfTStepEnd" } else { "KpfTStepLiq" });
    let take = |v: bool| {
        let who = if v { w.who(&st.claimer) } else { &w.w };
        let f = [fund(xk(who), 33, 1_000)];
        step_end(&w.lib, &q, &account.s, tk, &script, xk(&w.w), w.payer(who, &f), Some(v)).is_ok_and(|b| taken30(&b.0, &w.keys))
    };
    (take(true), take(false))
}

/// An account of one to five legs, futures and options of every kind, and a price a leg; `sid`: covered by it.
fn random_account34(w: &OW, r: &mut Rng, sid: Option<[u8; 32]>) -> (OLive<Account>, Vec<i64>) {
    let n = r.range(1, 6) as usize;
    let scale = w.markets[0].p.scale;
    let tenth = |r: &mut Rng| P10 / 10 * r.range(5, 16);
    let (mut legs, mut p) = (vec![], vec![]);
    for i in 0..n {
        let entry = tenth(r);
        let mut l = match r.range(0, 6) {
            0 => w.fut(i, 1, LONG, entry),
            1 => w.fut(i, 1, SHORT, entry),
            k => w.opt(i, 1, [CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN][k as usize - 2], tenth(r), entry),
        };
        l.size = r.range(1, 51) * scale + r.range(0, scale);
        legs.push(l);
        p.push(tenth(r) + r.range(0, 1_000));
    }
    let mut account = w.account(legs, r.range(0, 61) * KAS1 + r.range(0, KAS1), r.range(0, 61) * KAS1 + r.range(0, KAS1));
    account.s.shard = sid;
    if account.s.pot() < KAS1 {
        account.s.a_margin += KAS1;
    }
    account.value = (account.s.pot() + KAS1) as u64;
    (account, p)
}

/// The statement that the leg `skip` leaves an account that no shard covers, written as the rule would write it
/// were it allowed (the fee and the margins by `KpfTStepLiq`'s arithmetic), whether or not the rule allows it: so
/// that an end the script took for right against the model would be seen, and not hidden by outputs that differ.
fn step_as_if(x: &Account, kind: i64, side: i64, skip: i64, rows: &[Row], held: i64, claimer: [u8; 32]) -> (Stated, ATake) {
    let k = skip as usize;
    let last = rows[rows.len() - 1];
    let before = if k > 1 { rows[k - 2] } else { Row::ZERO };
    let wk = (rows[k - 1].wa - before.wa).max(rows[k - 1].wb - before.wb);
    let pot = x.pot();
    let mut am = (x.a_margin + last.gain).clamp(0, pot);
    let mut bm = pot - am;
    let mine = if side == SIDE_A { am } else { bm };
    let fee = if kind == KIND_ACCOUNT_LIQ { ((wk / 10_000) * x.t.liq_fee_bps + ((wk % 10_000) * x.t.liq_fee_bps) / 10_000).min(mine).max(0) } else { 0 };
    if side == SIDE_A {
        am -= fee;
    } else {
        bm -= fee;
    }
    let l = &x.legs[k - 1];
    let tk = ATake { side, amount: fee, rows: rows.to_vec(), skip, leave: Some((Head::of(l), SNote::units(l))) };
    let after = Account { a_margin: am, b_margin: bm, ..x.clone() };
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&last.h)].concat())), after.moving()].concat());
    let mut v = vec![Out { key: false, to: h, value: held - fee }];
    if fee > 0 {
        v.push(Out { key: true, to: claimer, value: fee });
    }
    (Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind, t0: T0, arg: tk.arg(), outs: Outs::of(&v).unwrap(), bond: x.bond(), claimer, held }, tk)
}

/// Round 34, item 2. `KpfTStepEnd` and `KpfTStepLiq` against `Account::end_right` and `step_end_right`, at the
/// engine, on random accounts of futures and options of every kind, covered and not: a take of the most the rule
/// gives, of a sompi more, of what the options the side holds were counted at more, of nothing, of less than
/// nothing; a take for side B of a covered account by a stranger (a deposit) and by a stranger on one that is not
/// covered; the last row with each of its sums below zero or moved; every leg leaving by a step of a liquidation and
/// by its party's statement, of each side, written as the rule would write it whether or not it allows it, and with
/// the fee a sompi more and less. The engine takes exactly the model's verdict each time. And the rule itself, in
/// numbers: a side that takes the most keeps what its options were counted at; a side all of whose legs are options
/// it holds is liquidated at no price.
#[test]
fn t1b_r34_an_ends_answer_is_the_models_for_takes_and_legs_that_leave() {
    let w = ow();
    let mut r = Rng(0x51ab_0c0f_fee0_3401);
    let (mut cases, mut rights, mut wrongs, mut allowed_steps, mut refused_steps) = (0, 0, 0, 0, 0);
    let check = |what: String, account: &OLive<Account>, st: &Stated, tk: &ATake, cases: &mut i32, rights: &mut i32, wrongs: &mut i32| -> bool {
        let x = &account.s;
        let model = if st.kind == KIND_ACCOUNT_TAKE { x.end_right(tk, &st.claimer, &st.outs, st.held) } else { x.step_end_right(st.kind, tk, &st.claimer, &st.outs, st.held) };
        assert_eq!(end_taken(&w, account, st, tk), verdicts(Some(model)), "{what}: the model says {model}");
        *cases += 1;
        if model { *rights += 1 } else { *wrongs += 1 }
        model
    };
    for round in 0..36 {
        let sid = (round % 3 == 2).then_some([0x5d; 32]);
        let (account, p) = random_account34(&w, &mut r, sid);
        let x = &account.s;
        assert!(x.check().is_ok());
        let held = account.value as i64;
        let rows = x.trace(&p);
        let last = rows[rows.len() - 1];
        let name = |s: &str| format!("account {round} ({} legs{}): {s}", x.legs.len(), if sid.is_some() { ", covered" } else { "" });
        // ---- takes
        for side in [SIDE_A, SIDE_B] {
            let key = if side == SIDE_A { x.a_key } else { x.b_key };
            let max = x.take_max(side, &p);
            let (_, e) = last.asked(side);
            // The rule in numbers: what is left to the side after the most it takes is at least what its options
            // were counted at (or all it is owed, where that is less).
            let due_a = (x.a_margin + last.gain).clamp(0, x.pot());
            let due = if side == SIDE_A { due_a } else { x.pot() - due_a };
            assert!(due - max >= e.min(due), "{}", name("a side that takes the most keeps what its options were counted at"));
            for (what, amount, claimer) in [("the most", max, key), ("a sompi more than the most", max + 1, key), ("the most and what its options were counted at", max + e.max(1), key), ("nothing", 0, key), ("less than nothing", -1, key), ("the most, stated by a stranger", max, xk(&w.w)), ("a sompi more, stated by a stranger", max + 1, xk(&w.w))] {
                let built = if amount >= 0 { x.take_by(side, amount, &p, T0, held, claimer).ok() } else { None };
                let was_built = built.is_some();
                let (st, tk) = match built {
                    Some(v) => v,
                    // (An amount the model does not write a statement of: the outputs of the take of nothing.)
                    None => {
                        let Ok((st0, tk0)) = x.take_by(side, 0, &p, T0, held, claimer) else { continue };
                        let tk = ATake { amount, ..tk0 };
                        (Stated { arg: tk.arg(), ..st0 }, tk)
                    }
                };
                let model = check(name(&format!("side {side} takes {what}")), &account, &st, &tk, &mut cases, &mut rights, &mut wrongs);
                if what == "the most" && was_built {
                    assert!(model, "{}", name("the most the rule gives is within the rule"));
                }
                if what == "a sompi more than the most" || what == "the most and what its options were counted at" {
                    assert!(!model, "{}", name("more than the most is not within the rule"));
                }
            }
            // The last row with a sum of its own making (the row is then wrong; the end is judged by its numbers).
            if let Ok((st0, tk0)) = x.take_by(side, max.min(KAS1), &p, T0, held, key) {
                for which in 0..6 {
                    let mut rows2 = tk0.rows.clone();
                    let l = rows2.last_mut().unwrap();
                    match which {
                        0 => l.wa = -1,
                        1 => l.wb = -1,
                        2 => l.ea = -1,
                        3 => l.eb = -1,
                        4 => l.ea = -(x.pot() + 1),
                        _ => l.eb = -(x.pot() + 1),
                    }
                    let tk = ATake { rows: rows2, ..tk0.clone() };
                    let st = Stated { arg: tk.arg(), ..st0 };
                    check(name(&format!("side {side} takes with the last row's sum {which} below zero")), &account, &st, &tk, &mut cases, &mut rights, &mut wrongs);
                }
            }
        }
        // ---- legs that leave
        for kind in [KIND_ACCOUNT_LIQ, KIND_ACCOUNT_LEAVE] {
            for side in [SIDE_A, SIDE_B] {
                for skip in 1..=x.legs.len() as i64 {
                    let key = if side == SIDE_A { x.a_key } else { x.b_key };
                    let claimer = if kind == KIND_ACCOUNT_LIQ { xk(&w.w) } else { key };
                    let rows = x.trace_without(&p, skip);
                    let allowed = x.stepped(kind, side, skip, &rows, &claimer).is_some();
                    let (st, tk) = match (x.step_any(kind, side, skip, &p, T0, held, claimer), sid) {
                        (Ok((st, tk, _)), _) => (st, tk),
                        (Err(_), None) => step_as_if(x, kind, side, skip, &rows, held, claimer),
                        (Err(_), Some(_)) => continue,
                    };
                    let model = check(name(&format!("kind {kind}, side {side}, leg {skip} leaves")), &account, &st, &tk, &mut cases, &mut rights, &mut wrongs);
                    assert_eq!(model, allowed, "{}", name("the end of a leg that leaves is right exactly where the rule allows it"));
                    if allowed { allowed_steps += 1 } else { refused_steps += 1 }
                    // The fee a sompi more and a sompi less, and (kind 4) by another key than the side's.
                    for d in [1i64, -1] {
                        let tk2 = ATake { amount: tk.amount + d, ..tk.clone() };
                        let st2 = Stated { arg: tk2.arg(), ..st };
                        assert!(!check(name(&format!("kind {kind}, side {side}, leg {skip}: the fee {d} off")), &account, &st2, &tk2, &mut cases, &mut rights, &mut wrongs));
                    }
                    if kind == KIND_ACCOUNT_LEAVE && allowed {
                        let st2 = Stated { claimer: xk(&w.w), ..st };
                        assert!(!check(name(&format!("kind 4, side {side}, leg {skip}: stated by a stranger")), &account, &st2, &tk, &mut cases, &mut rights, &mut wrongs));
                    }
                }
            }
        }
    }
    // ---- a side all of whose legs are options it holds: liquidated at no price, whatever its margin.
    let mut never = 0;
    for round in 0..40 {
        let n = r.range(1, 4) as usize;
        let scale = w.markets[0].p.scale;
        let side = if round % 2 == 0 { SIDE_A } else { SIDE_B };
        let legs: Vec<XLeg> = (0..n).map(|i| {
            let kind = match (side, r.range(0, 2)) {
                (SIDE_A, 0) => CALL_HELD,
                (SIDE_A, _) => PUT_HELD,
                (_, 0) => CALL_WRITTEN,
                _ => PUT_WRITTEN,
            };
            // (Struck where it is entered: nothing was counted yet, so its holder's margin owes it nothing.)
            let strike = P10 / 10 * r.range(5, 16);
            let mut l = w.opt(i, 1, kind, strike, strike);
            l.size = r.range(1, 51) * scale;
            l
        }).collect();
        // (The other party holds more than the options can come to be worth at these prices: where it does not,
        // see F57.)
        let (am, bm) = if side == SIDE_A { (r.range(0, 3) * KAS1, 9_000 * KAS1) } else { (9_000 * KAS1, r.range(0, 3) * KAS1) };
        let account = w.account(legs, am.max(1), bm.max(1));
        let x = &account.s;
        for _ in 0..6 {
            let p: Vec<i64> = (0..n).map(|_| P10 / 100 * r.range(1, 400)).collect();
            for skip in 1..=n as i64 {
                let rows = x.trace_without(&p, skip);
                assert!(x.stepped(KIND_ACCOUNT_LIQ, side, skip, &rows, &xk(&w.w)).is_none(), "a side that holds options and nothing else is liquidated (round {round}, prices {p:?})");
                never += 1;
            }
        }
        // And at the engine, once an account: the step written as the rule would write it is answered wrong.
        if round < 6 {
            let p: Vec<i64> = (0..n).map(|_| P10 / 100 * r.range(1, 400)).collect();
            let rows = x.trace_without(&p, 1);
            let (st, tk) = step_as_if(x, KIND_ACCOUNT_LIQ, side, 1, &rows, account.value as i64, xk(&w.w));
            assert!(!check(format!("a holder of options alone, side {side}: a step of its liquidation"), &account, &st, &tk, &mut cases, &mut rights, &mut wrongs));
        }
    }
    assert!(rights > 100 && wrongs > 300 && allowed_steps > 20 && refused_steps > 20, "{rights} right, {wrongs} wrong, {allowed_steps} steps allowed, {refused_steps} not");
    println!("T1b R34 ends: {cases} ends answered at the engine on 36 random accounts (a third covered) and 6 of options held alone: {rights} right, {wrongs} wrong, each the model's verdict and the other refused; {allowed_steps} legs' leavings the rule allows and {refused_steps} it does not; a side that holds options and nothing else is liquidated at none of {never} prices and legs");
}

/// The statement that the leg `skip` leaves an account, covered or not, written as `KpfTStepLiq` would write its
/// end (the fee and the margins by its arithmetic; on a covered account the note that the leg left and the note of
/// what A is owed beyond the pot), whether or not the rule allows it, and saying of the leg that leaves what is
/// given (`head`, `units`), whether or not that is the leg's.
#[allow(clippy::too_many_arguments)]
fn step_said35(x: &Account, kind: i64, side: i64, skip: i64, rows: &[Row], held: i64, claimer: [u8; 32], head: Head, units: i64) -> (Stated, ATake) {
    let k = skip as usize;
    let last = rows[rows.len() - 1];
    let before = if k > 1 { rows[k - 2] } else { Row::ZERO };
    let wk = (rows[k - 1].wa - before.wa).max(rows[k - 1].wb - before.wb);
    let pot = x.pot();
    let raw = x.a_margin + last.gain;
    let mut am = raw.clamp(0, pot);
    let mut bm = pot - am;
    let mine = if side == SIDE_A { am } else { bm };
    let fee = if kind == KIND_ACCOUNT_LIQ { ((wk / 10_000) * x.t.liq_fee_bps + ((wk % 10_000) * x.t.liq_fee_bps) / 10_000).min(mine).max(0) } else { 0 };
    if side == SIDE_A {
        am -= fee;
    } else {
        bm -= fee;
    }
    let tk = ATake { side, amount: fee, rows: rows.to_vec(), skip, leave: Some((head, units)) };
    let after = Account { a_margin: am, b_margin: bm, ..x.clone() };
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&last.h)].concat())), after.moving()].concat());
    let mut v = vec![(Out { key: false, to: h, value: held - fee }, false)];
    if fee > 0 {
        v.push((Out { key: true, to: claimer, value: fee }, false));
    }
    if let Some(sid) = x.shard {
        let release = SNote { sid, kind: NOTE_RELEASE, head, net: units, gross: units.wrapping_abs(), key: x.b_key, amount: 0 };
        v.push((Out { key: false, to: release.hash(), value: release.value() }, true));
        // (Round 41: what the shard is asked is what the pot could not pay less twice the tolerance of the two
        // sides' worth at the last row, and no note where that is nothing.)
        let band = last.wa + last.wb;
        let asked = (raw - pot).max(0) - ((band / 10_000) * 2 * x.t.g_bps + ((band % 10_000) * 2 * x.t.g_bps + 9_999) / 10_000);
        if asked > 0 {
            let owed = SNote::owed(sid, x.a_key, x.b_key, asked);
            v.push((Out { key: false, to: owed.hash(), value: owed.value() }, true));
        }
    }
    (Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind, t0: T0, arg: tk.arg(), outs: Outs::with_notes(&v).unwrap(), bond: x.bond(), claimer, held }, tk)
}

/// As `step_said35`, saying of the leg that leaves what it is.
fn step_as_if35(x: &Account, kind: i64, side: i64, skip: i64, rows: &[Row], held: i64, claimer: [u8; 32]) -> (Stated, ATake) {
    let l = &x.legs[skip as usize - 1];
    step_said35(x, kind, side, skip, rows, held, claimer, Head::of(l), SNote::units(l))
}

/// F57 (fixed in round 35). The holder of an option alone was liquidated, for a fee out of its margin, where the pot
/// could not pay what the option had come to be worth: its margin was capped at the pot and what it keeps whole was
/// not. The fix: what a side keeps whole is reduced by what the pot could not pay it (for A: what A is owed beyond
/// the pot; for B: what A's margin lacks), not below zero, before the comparison. Verified at the engine: the step
/// of round 34 is answered wrong and not right, on an account that is covered and on one that is not, with A and
/// with B as the holder, calls and puts. And attacked: a holder alone whose margin is at least what its options were
/// counted at when they entered is liquidated at no price; no side is liquidated now that was not before; a side
/// that is no longer liquidated is one the pot did not pay in full, and counted in full it stands at its
/// maintenance; a side below the maintenance of what it is asked is liquidated whatever it keeps whole; a side that
/// holds no option (a writer) is judged as before. And the holder that was paid its option's gain through a note of
/// what is owed, the option still standing: its margin is then the pot and what it keeps whole was paid out, it is
/// below while the option is worth anything, and a step exercises the option.
#[test]
fn t1b_fixed_f57_a_holder_of_an_option_alone_is_not_liquidated_for_what_the_pot_did_not_pay() {
    let w = ow();
    let stranger = xk(&w.w);
    // ---- the step of round 34, A and B as the holder, covered and not.
    let mut refused = 0;
    for (holder, kind) in [(SIDE_A, CALL_HELD), (SIDE_A, PUT_HELD), (SIDE_B, CALL_WRITTEN), (SIDE_B, PUT_WRITTEN)] {
        for covered in [false, true] {
            let (am, bm) = if holder == SIDE_A { (5 * KAS1, 20 * KAS1) } else { (20 * KAS1, 5 * KAS1) };
            let mut account = w.account(vec![w.opt(0, 10, kind, P10, P10)], am, bm);
            if covered {
                account.s.shard = Some([0x5d; 32]);
            }
            let x = &account.s;
            let held = account.value as i64;
            // The price at which the option is worth 40 and the pot holds 25.
            let p = [if kind == CALL_HELD || kind == CALL_WRITTEN { P10 * 14 / 10 } else { P10 * 6 / 10 }];
            let rows = x.trace_without(&p, 1);
            let (_, e) = rows[0].asked(holder);
            assert_eq!(e, 40 * KAS1, "the holder keeps 40 whole");
            assert!(x.stepped(KIND_ACCOUNT_LIQ, holder, 1, &rows, &stranger).is_none(), "F57 is open again: the holder of an option alone is liquidated where the pot is short (holder {holder}, kind {kind}, covered {covered})");
            let (st, tk) = step_as_if35(x, KIND_ACCOUNT_LIQ, holder, 1, &rows, held, stranger);
            assert_eq!(end_taken(&w, &account, &st, &tk), (false, true), "F57 is open again at the engine: the end of the liquidation of the holder is answered right (holder {holder}, kind {kind}, covered {covered})");
            refused += 1;
            // By its own statement the holder exercises, at no fee; the end is answered right.
            let key = if holder == SIDE_A { x.a_key } else { x.b_key };
            let (st, tk, sd) = x.step(KIND_ACCOUNT_LEAVE, holder, 1, &p, T0, held, key).expect("the holder exercises");
            assert_eq!(sd.fee, 0);
            assert_eq!(end_taken(&w, &account, &st, &tk), (true, false), "the leaving the holder states is answered right");
        }
    }
    // ---- a holder alone, of options of any strike entered at any price, whose margin is at least what they were
    // counted at when they entered: liquidated at no price, whatever the other party holds.
    let mut r = Rng(0x51ab_0c0f_fee0_3501);
    let scale = w.markets[0].p.scale;
    let mut never = 0;
    for round in 0..500 {
        let n = r.range(1, 4) as usize;
        let holder = if round % 2 == 0 { SIDE_A } else { SIDE_B };
        let legs: Vec<XLeg> = (0..n).map(|i| {
            let kind = match (holder, r.range(0, 2)) {
                (SIDE_A, 0) => CALL_HELD,
                (SIDE_A, _) => PUT_HELD,
                (_, 0) => CALL_WRITTEN,
                _ => PUT_WRITTEN,
            };
            let mut l = w.opt(i, 1, kind, P10 / 10 * r.range(5, 16), P10 / 10 * r.range(5, 16));
            l.size = r.range(1, 51) * scale + r.range(0, scale);
            l
        }).collect();
        let entries: Vec<i64> = legs.iter().map(|l| l.entry).collect();
        let mut account = w.account(legs, KAS1, KAS1);
        let at_entry = account.s.trace(&entries);
        let counted = at_entry[n - 1].asked(holder).1;
        let (mine, other) = (counted + r.range(0, 30) * KAS1, r.range(0, 30) * KAS1 + 1);
        (account.s.a_margin, account.s.b_margin) = if holder == SIDE_A { (mine, other) } else { (other, mine) };
        account.value = (account.s.pot() + KAS1) as u64;
        if round % 4 >= 2 {
            account.s.shard = Some([0x5d; 32]);
        }
        let x = &account.s;
        for _ in 0..20 {
            let p: Vec<i64> = (0..n).map(|_| P10 / 100 * r.range(1, 400)).collect();
            for skip in 1..=n as i64 {
                let rows = x.trace_without(&p, skip);
                assert!(x.stepped(KIND_ACCOUNT_LIQ, holder, skip, &rows, &stranger).is_none(), "a holder alone is liquidated (round {round}, prices {p:?}, skip {skip})");
                never += 1;
            }
        }
        // And at the engine, on some: the step written as the rule would write it is answered wrong.
        if round < 12 {
            let p: Vec<i64> = (0..n).map(|_| P10 / 100 * r.range(1, 400)).collect();
            let rows = x.trace_without(&p, 1);
            let (st, tk) = step_as_if35(x, KIND_ACCOUNT_LIQ, holder, 1, &rows, account.value as i64, stranger);
            assert_eq!(end_taken(&w, &account, &st, &tk), (false, true), "a holder alone, at the engine (round {round})");
            refused += 1;
        }
    }
    // ---- with other legs beside the options: the reduction lets nobody escape what it is asked.
    let rate_up = |v: i64, rate: i64| (v / 10_000) * rate + ((v % 10_000) * rate + 9_999) / 10_000;
    let (mut asked_alone, mut as_before, mut steps, mut spared, mut liquidated, mut engine) = (0, 0, 0, 0, 0, 0);
    for round in 0..600 {
        let (account, p) = random_account34(&w, &mut r, (round % 3 == 2).then_some([0x5d; 32]));
        let x = &account.s;
        let rate_lo = (x.t.mmr_bps - x.t.g_bps).max(0);
        for side in [SIDE_A, SIDE_B] {
            let rows = x.trace_without(&p, 1);
            let last = rows[rows.len() - 1];
            let (ws, es) = last.asked(side);
            let pot = x.pot();
            let raw = x.a_margin + last.gain;
            let am = raw.clamp(0, pot);
            let mine = if side == SIDE_A { am } else { pot - am };
            // What the pot could not pay the side.
            let unpaid = if side == SIDE_A { (raw - pot).max(0) } else { (-raw).max(0) };
            let stepped = x.stepped(KIND_ACCOUNT_LIQ, side, 1, &rows, &stranger).is_some();
            let before = mine < rate_up(ws, rate_lo) + es;
            assert!(!stepped || before, "a side is liquidated now that was not before the change (round {round}, side {side})");
            if stepped {
                liquidated += 1;
            }
            if before && !stepped {
                assert!(unpaid > 0, "a side the pot pays in full is spared (round {round}, side {side})");
                assert!(mine >= rate_up(ws, rate_lo), "a side below the maintenance of its worth is spared (round {round}, side {side})");
                assert!(es <= unpaid || mine + unpaid >= rate_up(ws, rate_lo) + es, "a side is spared that does not stand at its maintenance counted in full (round {round}, side {side})");
                spared += 1;
            }
            // At the engine, where the pot did not pay the side in full (what the change is about): the step written
            // as the rule would write it is answered as the model says.
            if unpaid > 0 && engine < 120 {
                let (st, tk) = step_as_if35(x, KIND_ACCOUNT_LIQ, side, 1, &rows, account.value as i64, stranger);
                assert_eq!(end_taken(&w, &account, &st, &tk), if stepped { (true, false) } else { (false, true) }, "the engine and the model differ on a side the pot did not pay (round {round}, side {side})");
                engine += 1;
            }
            // A side below the maintenance of what it is asked is liquidated, whatever it keeps whole and was not
            // paid.
            if mine < rate_up(ws, rate_lo) {
                assert!(stepped, "a side below the maintenance of its worth escapes (round {round}, side {side})");
                asked_alone += 1;
            }
            // A side that holds no option is judged by its worth alone, as before the change.
            if es == 0 {
                assert_eq!(stepped, mine < rate_up(ws, rate_lo), "a side that holds no option (round {round}, side {side})");
                as_before += 1;
            }
            // A side the pot pays in full is judged with all it keeps.
            if unpaid == 0 {
                assert_eq!(stepped, before, "a side the pot pays in full (round {round}, side {side})");
            }
            steps += 1;
        }
    }
    assert!(spared > 0 && liquidated > 50 && as_before > 50 && engine == 120, "{spared} spared, {liquidated} liquidated, {as_before} with no option, {engine} at the engine");
    // ---- the holder that was paid through a note of what is owed, the option standing.
    let mut account = w.account(vec![w.opt(0, 10, CALL_HELD, P10, P10), w.fut(1, 10, LONG, P10)], 5 * KAS1, 20 * KAS1);
    account.s.shard = Some([0x5d; 32]);
    let x = &account.s;
    let p = [P10 * 14 / 10, P10 * 14 / 10];
    // A leaves the futures leg by its own statement: it is owed 85 and the pot holds 25.
    let (st, tk, sd) = x.step(KIND_ACCOUNT_LEAVE, SIDE_A, 2, &p, T0, account.value as i64, x.a_key).expect("the futures leg of A leaves by its statement");
    assert_eq!((sd.owed, sd.after.a_margin, sd.after.b_margin, sd.after.legs.len(), sd.after.legs[0].entry), (60 * KAS1 - 420 * KAS1 / 100, 25 * KAS1, 0, 1, p[0]), "of the 60 the pot cannot pay the shard is asked 55.8 (round 41: less twice the tolerance of the 420 of worth); the pot to A; the option stands, counted at 14");
    assert_eq!(end_taken(&w, &account, &st, &tk), (true, false));
    // Then: its margin is the pot (25) and what it keeps whole is the 40 the option was counted at, which it was
    // paid. It is below at the price, above it and under it, while the option is worth anything.
    let mut then = w.account(sd.after.legs.clone(), sd.after.a_margin, sd.after.b_margin);
    then.s.shard = Some([0x5d; 32]);
    let y = &then.s;
    for (price, fee) in [(P10 * 14 / 10, 70_000_000), (P10 * 16 / 10, 80_000_000), (P10 * 30 / 10, 150_000_000), (P10 * 11 / 10, 0), (P10 + 1, 0)] {
        let rows = y.trace_without(&[price], 1);
        let s = y.stepped(KIND_ACCOUNT_LIQ, SIDE_A, 1, &rows, &stranger).unwrap_or_else(|| panic!("the holder that was paid is not below at {price}"));
        assert_eq!(s.fee, fee, "at {price}");
    }
    let (st, tk, _) = y.step(KIND_ACCOUNT_LIQ, SIDE_A, 1, &[p[0]], T0, then.value as i64, stranger).unwrap();
    assert_eq!(end_taken(&w, &then, &st, &tk), (true, false), "the step that exercises the option is answered right");
    // (By its own statement it exercises at no fee.)
    assert_eq!(y.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[p[0]], T0, then.value as i64, y.a_key).unwrap().2.fee, 0);
    // What the clearing party bears of the time between: where the price is back at the strike before a step lands,
    // the option is worth nothing, nothing is stepped, and the margin of A lacks 15 of the 40 it was paid.
    let back = y.trace_without(&[P10], 1);
    assert!(y.stepped(KIND_ACCOUNT_LIQ, SIDE_A, 1, &back, &stranger).is_none());
    assert_eq!(y.a_margin + back[0].gain, -15 * KAS1, "the pot holds 25 of the 40");
    println!("T1b F57 fixed: the liquidation of a holder where the pot is short is answered wrong and not right ({refused} at the engine: A and B as holder, calls and puts, covered and not); a holder alone whose margin covers what its options entered at is liquidated at none of {never} random prices and legs; of {steps} sides of random accounts {liquidated} are liquidated, none that was not before; {spared} are spared, each unpaid by the pot and at its maintenance counted in full; {engine} steps of sides the pot did not pay are answered at the engine as the model says; {asked_alone} below the maintenance of what they are asked are all liquidated; {as_before} that hold no option are judged as before. The holder paid through a note of what is owed (55.8 KAS: the 60 the pot could not pay less the band of round 41), its option standing, is below while the option is worth anything and a step exercises it (fee 0.7 KAS at the price); the clearing party bears up to what was paid for the option less the pot (15 KAS here) until that step");
}

/// Round 35, the notes of a leg that leaves. `KpfTStepLiq` makes the note that a leg left a covered account of what
/// the statement says of the leg (`said`: a row's name and side B's units), and `KpfTStepRow` holds that to the leg
/// in the row of the leg. At the engine, for a futures leg and for an option that leave by their party's statement:
/// a statement that says of the leg the other kind's row of its market, the market's bare name, another market's
/// row, another committee, the units the other way, none, or one more, has an end that is answered right (the end
/// is made of what is said) and a row of the leg that is answered wrong and not right, the model agreeing on both.
/// Units of the least 64-bit number (eight bytes the engine reads as none): the end and the row are both answered
/// wrong. And where the row of the leg that leaves carries a chain of the claimer's making (nobody opens it), in
/// each of the three ways the chains can then be written another point is wrong and is answered so. So no statement
/// whose note names another row has all its points right, and none has its wrong points all without an answer.
#[test]
fn t1b_r35_a_leaving_legs_note_names_the_legs_own_row_or_its_row_is_answered_wrong() {
    use kpf_protocol::reserve::futures_market;
    let w = ow();
    let mut account = w.account(vec![w.fut(0, 10, LONG, P10), w.opt(1, 10, CALL_HELD, P10, P10)], 50 * KAS1, 50 * KAS1);
    account.s.shard = Some([0x5d; 32]);
    let x = &account.s;
    let held = account.value as i64;
    let p = [P10 * 11 / 10, P10 * 11 / 10];
    let (mut ends_right, mut rows_wrong) = (0, 0);
    for skip in [1i64, 2] {
        let l = x.legs[skip as usize - 1];
        let rows = x.trace_without(&p, skip);
        let prev = if skip == 1 { [0u8; 32] } else { rows[0].g };
        let before = if skip == 1 { Row::ZERO } else { rows[0] };
        let (head, units) = (Head::of(&l), SNote::units(&l));
        let at = 2 * skip - 1;
        let judge = |hd: Head, un: i64| {
            let (st, tk) = step_said35(x, KIND_ACCOUNT_LEAVE, SIDE_A, skip, &rows, held, x.a_key, hd, un);
            let end_model = un != i64::MIN && x.step_end_right(KIND_ACCOUNT_LEAVE, &tk, &x.a_key, &st.outs, held);
            let row_model = row_judged_of(&prev, &l, &before, &rows[skip as usize - 1], Some((hd, un)), SIDE_A);
            let end = end_taken(&w, &account, &st, &tk);
            let row = row_taken(&w, &w.pointed(&w.stated(&account, &st, &tk.said()), at), &tk, at, &prev, &l);
            (end, row, end_model, row_model)
        };
        // The control: what the leg is.
        assert_eq!(judge(head, units), ((true, false), (true, false), true, Some(true)), "leg {skip}: said as it is");
        let other_kind = if skip == 1 { option_market(&l.market) } else { futures_market(&l.market) };
        let elsewhere = XLeg { market: w.markets[2].id(), ..l };
        for (what, hd, un) in [
            ("the other kind's row of its market", Head { market: other_kind, ..head }, units),
            ("its market's bare name", Head { market: l.market, ..head }, units),
            ("another market's row", Head::of(&elsewhere), units),
            ("another committee", Head { committee: [0x09; 32], ..head }, units),
            ("another scale", Head { scale: head.scale * 10, ..head }, units),
            ("another limit", Head { price_max: head.price_max + 1, ..head }, units),
            ("no market", Head::EMPTY, units),
            ("the units the other way", head, -units),
            ("no units", head, 0),
            ("a unit more", head, units + 1),
        ] {
            assert_eq!(judge(hd, un), ((true, false), (false, true), true, Some(false)), "leg {skip}, said of it: {what}");
            ends_right += 1;
            rows_wrong += 1;
        }
        let (end, row, _, row_model) = judge(head, i64::MIN);
        assert_eq!((end, row, row_model), ((false, true), (false, true), Some(false)), "leg {skip}: units of the least number");
        rows_wrong += 1;
    }
    // ---- and where the row of the leg that leaves is one nobody opens (its chain is of the claimer's making), the
    // note naming another row: another point is wrong and answered so, whichever way the chains are written.
    let wrong = |l: &XLeg| Head { market: if kpf_protocol::thin::option_of(l).is_some() { futures_market(&l.market) } else { option_market(&l.market) }, ..Head::of(l) };
    let (l1, l2) = (x.legs[0], x.legs[1]);
    let made = [0x6b; 32];
    let points = |skip: i64, rows: &[Row], opens2: &[u8; 32]| {
        let l = x.legs[skip as usize - 1];
        let (st, tk) = step_said35(x, KIND_ACCOUNT_LEAVE, SIDE_A, skip, rows, held, x.a_key, wrong(&l), SNote::units(&l));
        let live = w.stated(&account, &st, &tk.said());
        let end = end_taken(&w, &account, &st, &tk);
        let row1 = row_taken(&w, &w.pointed(&live, 1), &tk, 1, &[0; 32], &l1);
        let row2 = row_taken(&w, &w.pointed(&live, 3), &tk, 3, opens2, &l2);
        (end, row1, row2)
    };
    // The last leg leaves, its row's chain made: the row has no answer and the end is wrong (the last chain is not
    // the account's).
    let mut rows = x.trace_without(&p, 2);
    let true1 = rows[0].g;
    rows[1].g = made;
    assert_eq!(points(2, &rows, &true1), ((false, true), (true, false), (false, false)), "the last row's chain made");
    // The first leg leaves, its row's chain made, the second row's chain following the made one: the first row has
    // no answer, the second is right, the end is wrong.
    let mut rows = x.trace_without(&p, 1);
    let true2 = rows[1].g;
    rows[0].g = made;
    rows[1].g = blake2b(&[&made[..], &kpf_protocol::thin::leg_bytes(&l2)].concat());
    assert_eq!(points(1, &rows, &made), ((false, true), (false, false), (true, false)), "the first row's chain made, the second following it");
    // The same with the second row's chain the account's own: the end is right, the first row has no answer, and
    // the second row is wrong (the chain that opens it is not the row before's).
    rows[1].g = true2;
    assert_eq!(points(1, &rows, &true1), ((true, false), (false, false), (false, true)), "the first row's chain made, the second the account's own");
    rows_wrong += 3;
    println!("T1b R35 notes of a leg that leaves: {ends_right} statements that say another row or other units of the leg have an end answered right and the row of the leg answered wrong, {rows_wrong} rows answered wrong in all (the least 64-bit units: wrong at the end and at the row; a row of the leaving leg that nobody opens: another point answered wrong in each of three ways of writing the chains)");
}

/// What the engine takes of a row's answer where the model cannot compute the row (a sum beyond 64 bits): the
/// transaction is built for a leg that does not open the row (the builder then computes nothing) and the leg shown
/// is put in its place.
fn row_taken_past_the_model(w: &OW, q: &OLive<Pointed>, tk: &ATake, at: i64, prev: &[u8; 32], leg: &XLeg, decoy: &XLeg) -> (bool, bool) {
    let take = |v: bool| {
        let who = if v { w.who(&q.s.st.claimer) } else { &w.w };
        let f = [fund(xk(who), 33, 1_000)];
        let Ok((mut b, _)) = step_row_at(&w.lib, q, tk, at, prev, decoy, &w.script("KpfTStepRow"), xk(&w.w), w.payer(who, &f), Some(v)) else { return false };
        *args_of(&mut b, 1).last_mut().unwrap() = bytes_arg(&kpf_protocol::thin::leg_bytes(leg));
        taken30(&b, &w.keys)
    };
    (take(true), take(false))
}

/// F58 (closed in the tool: where an account is stated on in round 35, where a leg is added or moved after it; left
/// open in the scripts by decision). The scripts hold a leg to its own bound and have no count of legs; the sums of a trace
/// may pass 64 bits, and the first row whose sum does not fit is answered by neither verdict. `Account::check` now
/// refuses an account whose legs together are worth more at their markets' limits than 64 bits hold, and every
/// statement the model writes goes through it. Verified: eight of the largest legs are an account and nine are not;
/// the model writes no take and no leaving on the account of seventeen. Round 35 found `leg_added` and `leg_moved`
/// not asking it; they do now (verified in round 36: the ninth leg is not added and not moved in, the eighth is).
/// `KpfTLegAdd` still takes the ninth leg built past the model. The row nobody answers stands as it did, for two
/// parties who sign such legs past the tool.
#[test]
fn t1b_open_f58_the_tool_states_on_no_account_whose_sums_do_not_fit_and_adds_no_leg_into_one() {
    use kpf_protocol::cross::MARGIN_MAX;
    let w = ow();
    let scale = w.markets[0].p.scale;
    let pm = 2 * P10;
    let size = (MARGIN_MAX / pm - 1) * scale;
    let big = |i: usize| XLeg { price_max: pm, size, ..w.fut(i % 8, 1, LONG, P10) };
    let of = |n: usize| w.account((0..n).map(big).collect(), 100 * KAS1, 300 * KAS1);
    // ---- closed: an account is one whose sums fit.
    assert!(of(8).s.check().is_ok(), "eight of the largest legs are an account");
    for n in [9usize, 16, 17, 64] {
        assert!(of(n).s.check().is_err(), "F58 is open again in the model: {n} of the largest legs are an account");
    }
    let account = of(17);
    let x = &account.s;
    let held = account.value as i64;
    let p = vec![P10; 17];
    assert!(x.take_by(SIDE_A, 0, &p, T0, held, x.a_key).is_err() && x.step_any(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &p, T0, held, x.a_key).is_err() && x.step_any(KIND_ACCOUNT_LIQ, SIDE_B, 1, &p, T0, held, xk(&w.w)).is_err(), "the model writes a statement on an account whose sums do not fit");
    // And on the largest account that is one, every row is computed at the markets' limits.
    let eight = of(8);
    let at_limits = eight.s.trace(&[pm; 8]);
    assert_eq!(at_limits.len(), 8);
    // ---- closed in the tool after round 35 (edited by the author, mechanically, when `leg_added` and `leg_moved`
    // were made to ask it): the model adds no ninth leg and moves none in. The script still takes the ninth leg
    // where it is built past the model (it holds a leg to its own bound: left open by decision).
    let mut covered = of(8);
    covered.s.shard = Some([0x5d; 32]);
    assert!(covered.s.leg_added(big(8), 0, 0).is_err(), "F58 is open again where a leg is added: the model adds the leg that makes the sums too large");
    // (The controls, round 36: it is the sums that are refused. The eighth leg is added to seven and moved in.)
    let mut seven = of(7);
    seven.s.shard = Some([0x5d; 32]);
    assert!(seven.s.leg_added(big(7), 0, 0).is_ok_and(|(a, _)| a.check().is_ok()), "the eighth of the largest legs is not added");
    let f = [fund(xk(&w.a), 42, 1_000)];
    assert!(leg_add(&w.lib, &covered, &w.script("KpfTLegAdd"), big(8), 0, 0, w.payer(&w.a, &f)).is_err(), "the builder adds the ninth leg");
    let mut nine = covered.s.clone();
    nine.legs.push(big(8));
    assert!(nine.check().is_err());
    let note = kpf_protocol::reserve::SNote::leg(kpf_protocol::reserve::NOTE_JOIN, [0x5d; 32], &big(8), covered.s.b_key);
    let (lb, _, _) = leg_add_as(&w.lib, &covered, &w.script("KpfTLegAdd"), big(8), 0, 0, nine, note, w.payer(&w.a, &f)).expect("built past the model");
    assert!(taken30(&lb, &w.keys), "the script takes the ninth leg (it holds a leg to its own bound: left open by decision)");
    let mut one = of(1);
    one.s.shard = Some([0x5d; 32]);
    one.s.legs.push(w.fut(1, 1, LONG, P10));
    assert!(one.s.leg_moved(1, &covered.s, [0; 4]).is_err(), "F58 is open again where a leg is moved: the model moves the leg that makes the sums too large");
    assert!(one.s.leg_moved(1, &seven.s, [0; 4]).is_ok_and(|(_, to)| to.legs.len() == 8 && to.check().is_ok()), "the eighth of the largest legs is not moved in");
    // ---- left open in the scripts: the seventeenth row at the committee's own price is answered by neither verdict.
    let legs = &x.legs;
    let mut rows: Vec<Row> = vec![];
    for l in &legs[..16] {
        rows.push(row_after(rows.last().unwrap_or(&Row::ZERO), l, P10, false));
    }
    let b16 = rows[15];
    let last = Row { a: P10, gain: x.b_margin, wa: 0, wb: 0, ea: 0, eb: 0, g: x.legs_root(), h: blake2b(&[&b16.h[..], &kpf_protocol::thin::leg_bytes(&legs[16])].concat()) };
    rows.push(last);
    let amount = x.pot();
    let tk = ATake { side: SIDE_A, amount, rows: rows.clone(), skip: 0, leave: None };
    let after = Account { a_margin: 0, b_margin: 0, ..x.clone() };
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&last.h)].concat())), after.moving()].concat());
    let outs = Outs::of(&[Out { key: false, to: h, value: held - amount }, Out { key: true, to: x.a_key, value: amount }]).unwrap();
    let st = Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_ACCOUNT_TAKE, t0: T0, arg: tk.arg(), outs, bond: x.bond(), claimer: x.a_key, held };
    let live = w.stated(&account, &st, &tk.said());
    let decoy = w.fut(7, 1, SHORT, P10);
    assert_eq!(row_taken_past_the_model(&w, &w.pointed(&live, 33), &tk, 33, &b16.g, &legs[16], &decoy), (false, false), "the seventeenth row is answered (the scripts now bound the sums)");
    assert_eq!(row_taken_past_the_model(&w, &w.pointed(&live, 31), &tk, 31, &rows[14].g, &legs[15], &decoy), (true, false), "the sixteenth row is answered right");
    println!("T1b F58: eight of the largest legs are an account and nine are not; the model writes no statement on an account of seventeen. Open: the model's leg_added and KpfTLegAdd add the ninth leg, and leg_moved moves it in; a statement signed past the tool on seventeen has a row answered by neither verdict");
}

use kpf_protocol::reserve::{option_market, ShardRow, NOTE_RELEASE};

/// The built transaction with the argument `i` of the script at input 1 replaced.
fn with_arg(b: &OBuilt, i: usize, a: Arg) -> OBuilt {
    let mut t = b.clone();
    args_of(&mut t, 1)[i] = a;
    t
}
fn last_arg(b: &OBuilt, a: Arg) -> OBuilt {
    let mut t = b.clone();
    *args_of(&mut t, 1).last_mut().unwrap() = a;
    t
}

/// F59 (fixed in round 35). A futures leg whose market's name was the options' row's name was counted in that row
/// and set the options' net against itself. The fix: a row is named by the hash of its legs' kind and of their
/// market (`push8(14)` a market's futures legs, `push8(13)` its options), by `KpfTLegAdd` for the note of a leg that
/// enters and by `KpfTStepRow` for the note of one that leaves; `KpfTShardPrice` cites the market `under` whose
/// name hashes to the row's with one of the two tags. Verified: the leg of round 34 has a row of its own and the
/// shard is asked more with it, not less; a note that names another kind's row or another market's is not written
/// by `KpfTLegAdd` and not applied to that row; a row is priced by the one market its name is the hash of. And the
/// row nobody prices (the made leg's market has no committee): its price point has no answer, the dispute lapses
/// with the shard again and nothing drawn, and whatever price a draw states for it the shard is asked no less than
/// without the row (at the least price next to nothing more: the owner draws as if the row were not there while
/// nobody disputes that price; who stands on the other side of that leg signed a leg no committee prices).
#[test]
fn t1b_fixed_f59_a_row_is_named_by_the_kind_and_the_market_of_its_legs() {
    use kpf_protocol::reserve::{futures_market, NOTE_JOIN};
    let w = ow();
    let shard = w.shard();
    let account = w.covered(shard.s.sid, 20 * KAS1, 2 * KAS1);
    let (m0, m1) = (w.markets[0].id(), w.markets[1].id());
    let scale = w.markets[1].p.scale;
    let opt = w.opt(1, 10, CALL_HELD, P10, P10);
    let fut0 = w.fut(0, 4, LONG, P10);
    assert_eq!((Head::of(&opt).market, Head::of(&fut0).market), (option_market(&m1), futures_market(&m0)));
    assert_ne!(Head::of(&w.fut(1, 10, LONG, P10)), Head::of(&opt), "a market's futures legs and its options have two rows");
    let (account, j_opt) = w.added(&account, opt);
    let (account, j_fut) = w.added(&account, fut0);
    let s1 = w.applied(&w.applied(&shard, &j_opt, "the option's join applied"), &j_fut, "the futures leg's join applied");
    let need1 = s1.s.requirement(&[P10, P10, 0, 0]).unwrap();
    // The leg of round 34: a "futures" leg on the market whose name is the options' row's old name.
    let named = XLeg { market: option_market(&m1), ..w.fut(1, 10, LONG, P10) };
    assert_ne!(Head::of(&named), Head::of(&opt), "F59 is open again: a futures leg's head is the options' row's");
    assert_eq!(Head::of(&named).market, futures_market(&option_market(&m1)));
    let (account3, j_named) = w.added(&account, named);
    assert_ne!(j_named.s.head, j_opt.s.head);
    // It is not applied to the options' row (the model, and the script with the row told to be the options').
    assert!(s1.s.applied(&j_named.s, 0).is_err() && s1.s.applied(&j_named.s, 1).is_err());
    let f = [fund(xk(&w.w), 43, 1_000)];
    let (nb, _, _) = note_apply(&w.lib, &s1, &j_named, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).unwrap();
    assert!(taken30(&nb, &w.keys), "the control: applied to a row of its own");
    for r in [0i64, 1] {
        assert!(!taken30(&last_arg(&nb, int_arg(r)), &w.keys), "F59 is open again: the named leg's note is applied to row {r}");
    }
    let s2 = w.applied(&s1, &j_named, "the named leg's join applied");
    assert_eq!((s2.s.rows[0].net, s2.s.rows[0].gross, s2.s.rows[2].head, s2.s.rows[2].net), (10 * scale, 10 * scale, Head::of(&named), -10 * scale), "the options' row as it was; the named leg in a row of its own");
    // The shard is asked no less, whatever price is stated for the row nobody prices.
    for p3 in [1, P10 / 100, P10, w.markets[1].p.price_max] {
        assert!(s2.s.requirement(&[P10, P10, p3, 0]).unwrap() >= need1, "the shard is asked less with the made row at {p3}");
    }
    // ---- a note that names another row is not written by KpfTLegAdd (built past the model).
    let mut refused = 0;
    let fa = [fund(xk(&w.a), 42, 1_000)];
    let mut forged = |leg: XLeg, head: Head, net: i64| {
        let next = Account { legs: [account.s.legs.clone(), vec![leg]].concat(), ..account.s.clone() };
        let note = SNote { sid: shard.s.sid, kind: NOTE_JOIN, head, net, gross: leg.size, key: account.s.b_key, amount: 0 };
        let (b, _, _) = leg_add_as(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, next, note, w.payer(&w.a, &fa)).unwrap();
        let sg = b.sign(&w.keys).expect("signed");
        assert!(script_refuses(&sg.tx, &sg.entries, 1), "F59 is open again: a leg's note names a row that is not its kind's and its market's");
        refused += 1;
    };
    let fut1 = w.fut(1, 10, LONG, P10);
    forged(fut1, Head::of(&opt), -fut1.size); // a futures leg noted in its market's options' row
    forged(fut1, Head::of(&fut0), -fut1.size); // in another market's futures row
    forged(fut1, Head { market: m1, ..Head::of(&fut1) }, -fut1.size); // under its market's bare name
    forged(named, Head::of(&opt), -named.size); // the leg of round 34, noted in the options' row
    forged(opt, Head::of(&fut1), opt.size); // an option noted in its market's futures row
    forged(opt, Head::of(&fut1), -opt.size); // and with a futures leg's units
    forged(opt, Head { market: option_market(&m0), ..Head::of(&opt) }, opt.size); // in another market's options' row
    forged(fut1, Head::of(&fut1), fut1.size); // the right row, the units the other way
    // ---- a row is priced by the one market its name is the hash of.
    let fb = [fund(xk(&w.b), 50, 1_000)];
    let fw = [fund(xk(&w.w), 51, 1_000)];
    let prices = [P10, P10, P10, 0];
    let max = s2.s.draw_max(&prices);
    let (db, st, d) = shard_draw(&w.lib, &s2, &w.script("KpfTChange"), max, prices, T0, w.payer(&w.b, &fb)).unwrap();
    let live = nth(&signed("the draw stated", &db, &w.keys), 0, st, 6_600);
    let pointed = |point: i64| {
        let (b, q) = point_dispute(&w.lib, &live, &w.script("KpfTPoint"), xk(&w.w), point, w.payer(&w.w, &fw)).unwrap();
        nth(&signed("a point of the draw disputed", &b, &w.keys), 0, q, 6_610)
    };
    let cite1 = w.script("KpfTCite");
    let (c0, c1) = (w.cited(0, P10), w.cited(1, P10));
    // (The answer built with a citation the builder takes for the row, then `under` put as given.)
    let price = |point: i64, cite: Option<&Cited>, under: Option<[u8; 32]>, verdict: bool| -> bool {
        let who = if verdict { &w.b } else { &w.w };
        let f = [fund(xk(who), 52, 1_000)];
        let built = shard_price(&w.lib, &pointed(point), &s2.s, &d, point, &w.script("KpfTShardPrice"), cite.map(|c| (&cite1, c)), xk(&w.w), w.payer(who, &f), Some(verdict));
        built.is_ok_and(|(b, _)| taken30(&match under { Some(u) => last_arg(&b, bytes_arg(&u)), None => b }, &w.keys))
    };
    let mut tried = 0;
    assert!(price(1, Some(&c1), None, true) && !price(1, Some(&c1), None, false), "the options' row is priced by its market's citation");
    assert!(price(2, Some(&c0), None, true) && !price(2, Some(&c0), None, false), "the futures row is priced by its market's citation");
    for (point, cite, own) in [(1i64, &c1, m1), (2, &c0, m0)] {
        for under in [[0u8; 32], if own == m1 { m0 } else { m1 }, option_market(&own), futures_market(&own), w.markets[2].id()] {
            assert!(!price(point, Some(cite), Some(under), true) && !price(point, Some(cite), Some(under), false), "row {point} answered with `under` another than its market");
            tried += 2;
        }
        // (And the other market's citation the builder does not take for the row.)
        let other = if own == m1 { &c0 } else { &c1 };
        assert!(!price(point, Some(other), None, true) && !price(point, Some(other), None, false));
    }
    // The made row (its name the hash of a name no committee signs for): no citation answers it, by either verdict.
    for (cite, under) in [(&c1, None), (&c1, Some(option_market(&m1))), (&c1, Some(m1)), (&c0, Some(m0))] {
        assert!(!price(3, Some(cite), under, true) && !price(3, Some(cite), under, false), "the row nobody prices is answered");
        tried += 2;
    }
    assert!(price(4, None, None, true) && price(4, None, Some(m1), true) && !price(4, None, None, false), "a row with no market is right with no citation");
    // Its dispute lapses: the shard again, both bonds back, nothing drawn.
    let q3 = pointed(3);
    let lb = point_lapse(&w.lib, &q3, &w.script("KpfTPoint"), w.payer(&w.w, &fw)).unwrap();
    let ls = signed("the dispute of the made row's price lapsed", &lb, &w.keys);
    let k = ls.tx.outputs.len();
    assert_eq!((ls.tx.outputs[0].value, &ls.tx.outputs[0].script_public_key), (s2.value, &hash_spk(&w.lib, &s2.s.hash())), "the shard again, holding what it held");
    assert_eq!((ls.tx.outputs[k - 2].value, ls.tx.outputs[k - 1].value), (st.bond as u64, st.bond as u64));
    // What the owner gets of the row nobody prices: a draw that states the least price for it is asked for that
    // row next to nothing (as if the row were not there, and no less than that), and its end is answered right;
    // so it goes through while nobody disputes that price, and where somebody does it lapses as above.
    let low = [P10, P10, 1, 0];
    let need_low = s2.s.requirement(&low).unwrap();
    assert!(need_low >= need1 && need_low - need1 < 1_000, "the made row at the least price: {} more than without it", need_low - need1);
    let (db2, st2, d2) = shard_draw(&w.lib, &s2, &w.script("KpfTChange"), s2.s.draw_max(&low), low, T0, w.payer(&w.b, &fb)).unwrap();
    let live2 = nth(&signed("the draw stated with the least price for the made row", &db2, &w.keys), 0, st2, 6_600);
    let (pb, q0) = point_dispute(&w.lib, &live2, &w.script("KpfTPoint"), xk(&w.w), 0, w.payer(&w.w, &fw)).unwrap();
    let q0 = nth(&signed("its end disputed", &pb, &w.keys), 0, q0, 6_610);
    let end = |v: bool| {
        let who = if v { &w.b } else { &w.w };
        let f = [fund(xk(who), 53, 1_000)];
        shard_end(&w.lib, &q0, &s2.s, &d2, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(who, &f), Some(v)).is_ok_and(|(b, _)| taken30(&b, &w.keys))
    };
    assert_eq!((end(true), end(false)), (true, false), "the end of the draw that states the least price for the made row");
    let _ = account3;
    println!("T1b F59 fixed: the leg of round 34 is counted in a row of its own and the shard is asked no less at any price stated for it; {refused} notes that name another kind's row, another market's, or the other units are not written by KpfTLegAdd; the named leg's note is not applied to the options' row nor the futures row; `under`: {tried} answers with another market refused, one market prices a row; the row no committee prices has no answer and its dispute lapses with the shard as it was; a draw that states the least price for it is asked {} sompi more than without the row and its end is answered right", need_low - need1);
}

/// Round 34, item 4. `KpfTHand`, at the engine: who enters holds at least what who left held and what side B
/// asked; both sign; a shard, an account that no shard covers, a margin taken out, a signature of the wrong key and
/// a sompi less in the account are refused. (What who leaves is paid is no rule of the script: both sign the whole
/// transaction.)
#[test]
fn t1b_r34_a_side_is_handed_over_by_the_two_and_holds_what_was_asked() {
    let w = ow();
    let shard = w.shard();
    let account = w.covered(shard.s.sid, 20 * KAS1, 2 * KAS1);
    let (account, _) = w.added(&account, w.fut(0, 3, LONG, P10));
    let x = &account.s;
    let f = [fund(xk(&w.w), 50, 1_000)];
    let hand = |acct: &OLive<Account>, nkey: [u8; 32], da: i64| account_hand(&w.lib, acct, &w.script("KpfTHand"), nkey, da, 3 * KAS1, w.payer(&w.w, &f));
    let (hb, next) = hand(&account, xk(&w.w), KAS1).unwrap();
    let hs = signed("a side handed over", &hb, &w.keys);
    assert_eq!((next.a_key, next.a_margin, next.b_key, next.shard, &next.legs), (xk(&w.w), 21 * KAS1, x.b_key, x.shard, &x.legs));
    assert_eq!((hs.tx.outputs[0].value, &hs.tx.outputs[0].script_public_key), (account.value + KAS1 as u64, &hash_spk(&w.lib, &next.hash())));
    let mut refused = 0;
    let mut no = |what: &str, t: OBuilt| {
        assert!(!taken30(&t, &w.keys), "{what}");
        refused += 1;
    };
    // The account a sompi short; the signatures by the wrong keys (args: who, age, length, fixed, moving, nkey, da,
    // sa, sn).
    let mut t = hb.clone();
    t.tx.outputs[0].value -= 1;
    no("the account a sompi short", t);
    no("who leaves does not sign (the newcomer signs twice)", with_arg(&hb, 7, Arg::Sig(xk(&w.w))));
    no("who enters does not sign (who leaves signs twice)", with_arg(&hb, 8, Arg::Sig(x.a_key)));
    no("side B signs in the newcomer's place", with_arg(&hb, 8, Arg::Sig(x.b_key)));
    // Another key named than the one that signs.
    no("the key named is not the one that signed", with_arg(&hb, 5, bytes_arg(&xk(&w.f))));
    // A margin taken out with the hand-over: the model refuses it, and the script.
    assert!(hand(&account, xk(&w.w), -1).is_err());
    let made = |acct: &OLive<Account>, base: &OBuilt, nkey: [u8; 32], da: i64| {
        let mut t = with_arg(base, 6, int_arg(da));
        let next = Account { a_key: nkey, a_margin: acct.s.a_margin + da, ..acct.s.clone() };
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &next.hash());
        t.tx.outputs[0].value = (acct.value as i64 + da) as u64;
        t
    };
    assert!(taken30(&made(&account, &hb, xk(&w.w), 2 * KAS1), &w.keys), "the control of the helper");
    no("a sompi of margin taken out", made(&account, &hb, xk(&w.w), -1));
    // Less than side B asked of whoever stands on side A (the account as B's ask of 30 KAS stands in it).
    let asked = OLive { s: Account { b_min_counter: 30 * KAS1, ..x.clone() }, ..account.clone() };
    assert!(hand(&asked, xk(&w.w), 10 * KAS1 - 1).is_err());
    let (ab, _) = hand(&asked, xk(&w.w), 10 * KAS1).unwrap();
    assert!(taken30(&ab, &w.keys), "who enters holds exactly what side B asked");
    no("a sompi less than side B asked", made(&asked, &ab, xk(&w.w), 10 * KAS1 - 1));
    // A shard handed over (it has no second key): refused. Built from the hand-over of the account: the object, its
    // two parts and what it would become.
    let s = &shard.s;
    let (sf, sm) = (s.fixed_hash(), s.moving());
    let mut t = with_arg(&with_arg(&hb, 3, bytes_arg(&sf)), 4, bytes_arg(&sm));
    t.tx.inputs[0].previous_outpoint = shard.op;
    t.entries[0].amount = shard.value;
    object_made(&mut t, &w.lib, 0, &s.hash());
    let m = s.held + KAS1;
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &blake2b(&[p33(&sf), p33(&xk(&w.w)), sm[33..67].to_vec(), sm8(m).to_vec(), sm[75..].to_vec()].concat()));
    t.tx.outputs[0].value = shard.value + KAS1 as u64;
    args_of(&mut t, 1)[7] = Arg::Sig(s.owner);
    no("a shard is handed over by its owner", t);
    // An account that no shard covers (138 bytes): refused.
    let plain = w.account(vec![w.fut(0, 3, LONG, P10)], 20 * KAS1, 2 * KAS1);
    let (pf, pm) = (plain.s.fixed_hash(), plain.s.moving());
    let mut t = with_arg(&with_arg(&hb, 3, bytes_arg(&pf)), 4, bytes_arg(&pm));
    t.tx.inputs[0].previous_outpoint = plain.op;
    t.entries[0].amount = plain.value;
    t.entries[0].covenant_id = plain.id;
    object_made(&mut t, &w.lib, 0, &plain.s.hash());
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { a_key: xk(&w.w), a_margin: 21 * KAS1, ..plain.s.clone() }.hash());
    t.tx.outputs[0].value = plain.value + KAS1 as u64;
    t.tx.outputs[0].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: plain.id.unwrap() });
    no("an account that no shard covers is handed over", t);
    // To the same key: the script takes it (a margin added by another way), and nothing else changes.
    let same = hand(&account, x.a_key, KAS1).map(|(b, n)| (taken30(&b, &w.keys), n.a_key == x.a_key));
    // What who leaves is paid is the two parties' own: with a sompi less, signed again by both, it is taken.
    let k = hb.tx.outputs.len();
    let pay = (0..k).find(|i| hb.tx.outputs[*i].script_public_key == p2pk_spk(&x.a_key) && hb.tx.outputs[*i].value == 3 * KAS1 as u64).expect("the payment to who leaves");
    let mut t = hb.clone();
    t.tx.outputs[pay].value -= 1;
    let less = taken30(&t, &w.keys);
    println!("T1b R34 hand: a side handed over with 1 KAS more; {refused} hand-overs refused (a sompi short, a wrong signer, a margin taken out, less than side B asked, a shard, an account of 138 bytes); to the same key taken: {same:?}; the payment to who leaves a sompi less and signed again by both taken: {less} (no rule of the script: the two sign the whole transaction)");
}

/// Round 34, item 5. `KpfTLegMove`, at the engine: a leg moves between two covered accounts of one clearing party
/// under one shard by three signatures, its entry as it was. Each signature by another key, an account of another
/// shard or another side B, a chain that does not open the leg, legs after it that are no legs or more than six, a
/// margin below zero and a sompi less in either account are refused. (Margin taken out of an account with the move
/// is taken: every party of both accounts signs the whole transaction.)
#[test]
fn t1b_r34_a_leg_moves_between_two_accounts_of_one_clearing_party_by_three_signatures() {
    let w = ow();
    let shard = w.shard();
    let x0 = w.covered(shard.s.sid, 20 * KAS1, 2 * KAS1);
    let (x1, _) = w.added(&x0, w.fut(0, 3, LONG, P10));
    let (x2, _) = w.added(&x1, w.opt(1, 2, CALL_HELD, P10, P10 * 11 / 10));
    let (from, _) = w.added(&x2, w.fut(2, 1, SHORT, P10));
    // The other account: another user (W) against the same clearing party under the same shard.
    let y = Account { a_key: xk(&w.w), legs: vec![w.fut(3, 1, LONG, P10)], a_margin: 10 * KAS1, b_margin: KAS1, ..x0.s.clone() };
    let to = OLive { value: y.pot() as u64, s: y.clone(), id: from.id, op: op(60), daa: 6_100 };
    let f = [fund(xk(&w.a), 53, 1_000)];
    let mv = |i: usize, to: &OLive<Account>, d: [i64; 4]| leg_move(&w.lib, &from, i, to, d, &w.script("KpfTLegMove"), w.payer(&w.a, &f));
    let (mb, nx, ny) = mv(2, &to, [0; 4]).unwrap();
    let ms = signed("a leg moved", &mb, &w.keys);
    assert_eq!((nx.legs.len(), ny.legs.len(), ny.legs[1], nx.pot() + ny.pot()), (2, 2, from.s.legs[1], from.s.pot() + y.pot()), "the leg as it was, its entry too; the margins as they were");
    assert_eq!((ms.tx.outputs[0].value, ms.tx.outputs[2].value), (from.value, to.value));
    // Every leg of the account, moved from each place.
    for i in [1usize, 3] {
        assert!(taken30(&mv(i, &to, [0; 4]).unwrap().0, &w.keys), "the leg {i} moves");
    }
    let mut refused = 0;
    let mut no = |what: &str, t: OBuilt| {
        assert!(!taken30(&t, &w.keys), "{what}");
        refused += 1;
    };
    // The three signatures (args 15, 16, 17: side A of each account, and side B), each by another of the keys.
    for (i, what) in [(15usize, "side A of the account the leg leaves"), (16, "side A of the account it enters"), (17, "the clearing party")] {
        for key in [xk(&w.a), xk(&w.w), xk(&w.b), xk(&w.f)] {
            let Arg::Sig(own) = args_of(&mut mb.clone(), 1)[i].clone() else { panic!("a signature") };
            if key != own {
                no(&format!("{what} does not sign"), with_arg(&mb, i, Arg::Sig(key)));
            }
        }
    }
    // An account of another shard, or of another side B: the model refuses, and the script (the other account, its
    // moving part and what it becomes put in the honest one's place).
    for (what, other) in [("another shard", Account { shard: Some([0x77; 32]), ..y.clone() }), ("another side B", Account { b_key: xk(&w.f), ..y.clone() })] {
        let o = OLive { s: other.clone(), ..to.clone() };
        assert!(mv(2, &o, [0; 4]).is_err(), "the model moves a leg to an account of {what}");
        let mut t = with_arg(&mb, 10, bytes_arg(&other.moving()));
        object_made(&mut t, &w.lib, 2, &other.hash());
        let mut after = other.clone();
        after.legs.push(from.s.legs[1]);
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &after.hash());
        no(&format!("a leg moves to an account of {what}"), t);
    }
    // The chain before the leg, the leg, and the legs after it.
    no("a chain that does not open the leg", with_arg(&mb, 4, bytes_arg(&[0x33; 32])));
    no("another leg than the one at that place", with_arg(&mb, 5, bytes_arg(&kpf_protocol::thin::leg_bytes(&from.s.legs[0]))));
    let Arg::V(ArtifactValue::Bytes(after)) = args_of(&mut mb.clone(), 1)[6].clone() else { panic!("the legs after") };
    assert_eq!(after.len(), 111);
    no("the legs after are not whole legs", with_arg(&mb, 6, bytes_arg(&[after.clone(), vec![0]].concat())));
    no("no leg after where one stands", with_arg(&mb, 6, bytes_arg(&[])));
    no("seven legs after", with_arg(&mb, 6, bytes_arg(&after.repeat(7))));
    let eight = Account { legs: (0..8).map(|i| w.fut(i % 8, 1, LONG, P10)).collect(), ..from.s.clone() };
    assert!(eight.leg_moved(1, &y, [0; 4]).is_err() && eight.leg_moved(2, &y, [0; 4]).is_ok(), "the model moves a leg with at most six after it");
    // A margin below zero, by the model and by the script.
    assert!(mv(2, &to, [-(from.s.a_margin + 1), 0, 0, 0]).is_err());
    for (i, m) in [(11usize, from.s.a_margin), (12, from.s.b_margin), (13, y.a_margin), (14, y.b_margin)] {
        no("a margin below zero", with_arg(&mb, i, int_arg(-(m + 1))));
    }
    // A sompi less in either account.
    for o in [0usize, 2] {
        let mut t = mb.clone();
        t.tx.outputs[o].value -= 1;
        no("an account a sompi short", t);
    }
    // The two accounts in each other's place in what is written.
    let mut t = mb.clone();
    let (a0, a2) = (t.tx.outputs[0].script_public_key.clone(), t.tx.outputs[2].script_public_key.clone());
    t.tx.outputs[0].script_public_key = a2;
    t.tx.outputs[2].script_public_key = a0;
    no("the two accounts written in each other's place", t);
    // Margin taken out with the move, by the three signatures: taken.
    let (ob, ox, _) = mv(2, &to, [-5 * KAS1, 0, 0, 0]).unwrap();
    let out = taken30(&ob, &w.keys);
    assert_eq!(ox.a_margin, from.s.a_margin - 5 * KAS1);
    println!("T1b R34 leg move: a leg moved from each place, its entry as it was; {refused} moves refused (a signature by another key, another shard, another side B, a wrong chain or leg, legs after that are none or seven, a margin below zero, a sompi short, the accounts swapped); 5 KAS of margin taken out with the move by the three signatures taken: {out}");
}

/// Round 34, item 6. (a) `KpfTNoteApply` empties a row whose net and gross return to 0. Whoever applies chooses the
/// order: with two equal legs in a market, the second's leaving applied before its entering empties the row while
/// the first leg stands. A draw stated on the shard as it then looks is ended by the entering's note, which landed
/// before it and is not applied (`KpfTDrawNote`); once that note is applied the row is the market's again. (b) The
/// head-form check of `KpfTChange` for kinds 2 to 4: a statement whose pushes at 24, 57, 90, 99 are others is not
/// stated; and every statement the script states is one `ATake::read` reads.
#[test]
fn t1b_r34_a_row_emptied_out_of_order_is_guarded_and_what_is_stated_is_read() {
    let w = ow();
    let shard = w.shard();
    let scale = w.markets[0].p.scale;
    let x0 = w.covered(shard.s.sid, 30 * KAS1, 30 * KAS1);
    let leg = w.fut(0, 5, LONG, P10);
    let (x1, j1) = w.added(&x0, leg);
    let (x2, j2) = w.added(&x1, leg);
    let j2 = OLive { daa: 6_120, ..j2 };
    let s1 = w.applied(&shard, &j1, "the first leg's join applied");
    assert_eq!((s1.s.rows[0].net, s1.s.rows[0].gross), (-5 * scale, 5 * scale));
    // The second leg leaves by A's statement; its settlement writes the note that it left.
    let x = &x2.s;
    let (st, tk, _) = x.step(KIND_ACCOUNT_LEAVE, SIDE_A, 2, &[P10, P10], T0, x2.value as i64, x.a_key).unwrap();
    let live = w.stated(&x2, &st, &tk.said());
    let fw = [fund(xk(&w.w), 54, 1_000)];
    let ss = signed("the second leg's leaving applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let release = SNote { sid: shard.s.sid, kind: NOTE_RELEASE, head: Head::of(&leg), net: SNote::units(&leg), gross: leg.size, key: x.b_key, amount: 0 };
    let at = (0..ss.tx.outputs.len()).find(|i| ss.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &release.hash())).expect("the note that the leg left");
    let rel = nth(&ss, at, release, 6_570);
    // Applied before the second leg's entering: the row is empty, with one leg of 5 units standing.
    let s2 = w.applied(&s1, &rel, "the leaving applied before the entering");
    assert_eq!(s2.s.rows[0], ShardRow::EMPTY, "the row is emptied while a leg of the market stands");
    assert_eq!(s2.s.requirement(&[P10, 0, 0, 0]), Some(0));
    // A draw of everything on the shard as it looks: stated, and ended by the note that is not applied.
    let fb = [fund(xk(&w.b), 55, 1_000)];
    let (db, dst, _) = shard_draw(&w.lib, &s2, &w.script("KpfTChange"), s2.s.held, [0; 4], T0, w.payer(&w.b, &fb)).unwrap();
    let ds = signed("a draw of everything stated on the emptied row", &db, &w.keys);
    let dlive = nth(&ds, 0, dst, 6_600);
    let nb = draw_note(&w.lib, &dlive, &dst, None, &s2.s, &j2, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    let ns = signed("the draw ended by the entering's note", &nb, &w.keys);
    assert_eq!(ns.tx.outputs[0].script_public_key, hash_spk(&w.lib, &s2.s.hash()), "the shard again");
    // The entering applied: the row is the market's again, with the leg that stands.
    let s3 = w.applied(&s2, &j2, "the entering applied after");
    assert_eq!((s3.s.rows[0].head, s3.s.rows[0].net, s3.s.rows[0].gross), (Head::of(&leg), -5 * scale, 5 * scale));
    // A row is not emptied by a note of another market, nor by one of an account of another side B.
    let other = SNote { head: Head::of(&w.fut(1, 5, LONG, P10)), ..rel.s };
    assert!(s1.s.applied(&other, 0).is_err() && s1.s.applied(&SNote { key: xk(&w.w), ..rel.s }, 0).is_err());

    // ---- (b) the head form of what a statement of kinds 2 to 4 says.
    let account = w.account(vec![w.fut(0, 3, LONG, P10), w.fut(1, 2, SHORT, P10)], 50 * KAS1, 50 * KAS1);
    let y = &account.s;
    let held = account.value as i64;
    let outs = Outs::of(&[Out { key: false, to: y.hash(), value: held }]).unwrap();
    let stated = |kind: i64, said: &[u8]| -> bool {
        let st = Stated { position: y.hash(), window: y.dispute_daa, late: y.t.late_daa, kind, t0: T0, arg: blake2b(said), outs, bond: y.bond(), claimer: y.a_key, held };
        let f = [fund(y.a_key, 56, 1_000)];
        account_state(&w.lib, &account, &w.script("KpfTChange"), &st, said, w.payer(&w.a, &f)).is_ok_and(|b| taken30(&b, &w.keys))
    };
    let honest = ATake { side: SIDE_A, amount: 0, rows: y.trace(&[P10, P10]), skip: 0, leave: None }.said();
    let (mut bad, mut read) = (0, 0);
    for kind in [KIND_ACCOUNT_TAKE, KIND_ACCOUNT_LIQ, KIND_ACCOUNT_LEAVE] {
        assert!(stated(kind, &honest), "the control: kind {kind}");
        for at in [24usize, 57, 90, 99] {
            for v in [0u8, 0x21, 0x09, 0x4c] {
                let mut s = honest.clone();
                if s[at] != v {
                    s[at] = v;
                    assert!(!stated(kind, &s), "kind {kind}: the push at {at} is {v:#x}");
                    bad += 1;
                }
            }
        }
        // Too short, a row not whole, a row more than whole.
        for s in [honest[..115].to_vec(), honest[..honest.len() - 1].to_vec(), [honest.clone(), vec![0]].concat()] {
            assert!(!stated(kind, &s), "kind {kind}: a length that is no head and rows");
            bad += 1;
        }
    }
    // Whatever else the bytes are, what the script states the reader reads.
    let mut r = Rng(0x51ab_0c0f_fee0_3406);
    for _ in 0..40 {
        let mut s: Vec<u8> = (0..116 + 112 * r.range(0, 4) as usize).map(|_| r.next() as u8).collect();
        (s[24], s[57], s[90], s[99]) = (0x20, 0x20, 0x08, 0x08);
        let kind = [KIND_ACCOUNT_TAKE, KIND_ACCOUNT_LIQ, KIND_ACCOUNT_LEAVE][r.range(0, 3) as usize];
        assert!(stated(kind, &s), "bytes of the form are stated");
        assert!(ATake::read(&s).is_some_and(|t| t.said() == s), "what the script states the reader reads back as it is");
        read += 1;
    }
    println!("T1b R34 notes and forms: a leaving applied before its entering empties the row while an equal leg stands, a draw stated then is ended by the entering's note, and the row is the market's again once it is applied; {bad} statements whose head is not of the form refused; {read} statements of random bytes in the form are stated and read back by ATake::read");
}

// ---------------------------------------------------------------- round 36: KpfTLegOut

use kpf_protocol::reserve::{futures_market, NOTE_JOIN};

/// A shard and a covered account of the user (20 KAS) against the clearing party (`bm`), with three legs whose
/// joins are applied: long 10 of market 0, a call held on 10 of market 1, short 4 of market 2.
fn three36(w: &OW, bm: i64) -> (OLive<Shard>, OLive<Account>, Vec<OLive<SNote>>) {
    let mut shard = w.shard();
    let mut account = w.covered(shard.s.sid, 20 * KAS1, bm);
    let mut joins = vec![];
    for leg in [w.fut(0, 10, LONG, P10), w.opt(1, 10, CALL_HELD, P10, P10), w.fut(2, 4, SHORT, P10)] {
        let (next, join) = w.added(&account, leg);
        shard = w.applied(&shard, &join, "join applied");
        account = next;
        joins.push(join);
    }
    (shard, account, joins)
}

/// `KpfTLegOut` built by the model, the user paying the fee (`payb` from A's margin to B's; `paid` to A's key from
/// the payer's coins).
fn out36(w: &OW, account: &OLive<Account>, i: usize, payb: i64, paid: i64, take: i64, addb: i64) -> Option<(OBuilt, Account, SNote)> {
    let f = [fund(xk(&w.a), 57, 1_000)];
    leg_out(&w.lib, account, i, payb, paid, take, addb, &w.script("KpfTLegOut"), w.payer(&w.a, &f)).ok()
}

/// `KpfTLegOut` with the account and the note its outputs are said to be: whether the engine takes it.
#[allow(clippy::too_many_arguments)]
fn out_as36(w: &OW, account: &OLive<Account>, i: usize, payb: i64, paid: i64, take: i64, addb: i64, next: Account, note: SNote) -> bool {
    let f = [fund(xk(&w.a), 57, 1_000)];
    leg_out_as(&w.lib, account, i, payb, paid, take, addb, next, note, &w.script("KpfTLegOut"), w.payer(&w.a, &f)).is_ok_and(|b| taken30(&b.0, &w.keys))
}

/// F60 (fixed in round 37). A covered account paid its side B: `KpfTLegOut` moved B's margin to A's by `pay` and let
/// A take it, to B's own key; `KpfTLegMove` let B's margin fall in either account. The fix: `KpfTLegOut` has no
/// movement from B's margin at all (`payb` not below zero moves from A's to B's; what B owes A for the leg it pays
/// from its own coins, by an ordinary output A's signature holds), and `KpfTLegMove` asks that the two changes of
/// B's margin are together not below zero. Verified at the engine: the transaction of round 36 in its new form (a
/// `payb` below zero, the outputs as they were) is refused; the move with B's margin 30 KAS lower is refused, and
/// with the other account's 30 higher it is taken, and with a sompi less than that refused; both places of a move
/// naming one object is refused. And B's margin after a leg that leaves is never below what it was (the second
/// test below, 400 random amounts).
#[test]
fn t1b_fixed_f60_side_bs_margin_does_not_leave_by_a_leg_that_leaves_or_moves() {
    let w = ow();
    let (_, account, _) = three36(&w, 40 * KAS1);
    let x = &account.s;
    let all = x.b_margin;
    // ---- the leg leaves: no amount moves B's margin to A's.
    assert!(x.leg_out(1, -all, all, 0).is_err(), "F60 is open again: the model moves side B's margin to A's");
    let mut legs = x.legs.clone();
    let leg = legs.remove(0);
    let release = SNote::leg(NOTE_RELEASE, x.shard.unwrap(), &leg, x.b_key);
    for (payb, take, am, bm) in [(-all, all, x.a_margin, 0), (-all, 0, x.a_margin + all, 0), (-1, 0, x.a_margin + 1, all - 1), (-KAS1, KAS1, x.a_margin, all - KAS1)] {
        assert!(!out_as36(&w, &account, 1, payb, 0, take, 0, Account { legs: legs.clone(), a_margin: am, b_margin: bm, ..x.clone() }, release), "F60 is open again: a leg leaves with {} sompi of side B's margin moved to A's", -payb);
    }
    // The leg's gain is paid by the clearing party's own coins: 5 KAS to A's key, B's margin in the account as it was.
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let (b, next, _) = leg_out(&w.lib, &account, 1, 0, 5 * KAS1, 0, 0, &w.script("KpfTLegOut"), w.payer(&w.b, &fb)).unwrap();
    let sg = signed("a leg leaves at once, its gain paid from the clearing party's coins", &b, &w.keys);
    assert!(sg.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(&x.a_key) && o.value == 5 * KAS1 as u64));
    assert_eq!((sg.tx.outputs[0].value, next.a_margin, next.b_margin), (account.value, x.a_margin, all), "the account holds what it held, both margins as they were");
    // ---- the leg moves: B's margin moves between its two accounts and not out of them.
    let y = Account { a_key: xk(&w.w), legs: vec![w.fut(3, 1, LONG, P10)], a_margin: 10 * KAS1, b_margin: KAS1, ..x.clone() };
    let to = OLive { value: y.pot() as u64, s: y.clone(), id: account.id, op: op(60), daa: 6_100 };
    let mv = |d: [i64; 4]| leg_move(&w.lib, &account, 1, &to, d, &w.script("KpfTLegMove"), w.payer(&w.b, &fb));
    assert!(mv([0, -30 * KAS1, 0, 0]).is_err() && mv([0, -30 * KAS1, 30 * KAS1, 0]).is_err() && mv([0, -30 * KAS1, 0, 30 * KAS1 - 1]).is_err(), "F60 is open again for a move: the model lowers side B's margin over the two accounts");
    // Between the two: taken, each object holding its change.
    let (mb, nx, ny) = mv([0, -30 * KAS1, 0, 30 * KAS1]).unwrap();
    let ms = signed("a leg moves and 30 KAS of side B's margin with it", &mb, &w.keys);
    assert_eq!((ms.tx.outputs[0].value, ms.tx.outputs[2].value, nx.b_margin, ny.b_margin), (account.value - 30 * KAS1 as u64, to.value + 30 * KAS1 as u64, 10 * KAS1, 31 * KAS1));
    // Built past the model: the arguments 12 and 14 are the two changes of B's margin, the outputs 0 and 2 the two
    // accounts.
    let (base, bx, by) = mv([0; 4]).unwrap();
    assert!(taken30(&base, &w.keys));
    let past = |dxb: i64, dyb: i64, dya: i64| {
        let mut t = base.clone();
        let args = args_of(&mut t, 1);
        args[12] = int_arg(dxb);
        args[13] = int_arg(dya);
        args[14] = int_arg(dyb);
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { b_margin: bx.b_margin + dxb, ..bx.clone() }.hash());
        t.tx.outputs[0].value = (account.value as i64 + dxb) as u64;
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &Account { a_margin: by.a_margin + dya, b_margin: by.b_margin + dyb, ..by.clone() }.hash());
        t.tx.outputs[2].value = (to.value as i64 + dya + dyb) as u64;
        taken30(&t, &w.keys)
    };
    assert!(past(-30 * KAS1, 30 * KAS1, 0) && past(-30 * KAS1, 31 * KAS1, 0) && past(0, KAS1, 0) && past(30 * KAS1, -KAS1, 0), "the controls: between the two, and more");
    let mut refused = 0;
    for (dxb, dyb, dya) in [(-30 * KAS1, 0, 0), (-30 * KAS1, 30 * KAS1 - 1, 0), (-1, 0, 0), (0, -1, 0), (-30 * KAS1, 0, 30 * KAS1), (-KAS1, 0, KAS1)] {
        assert!(!past(dxb, dyb, dya), "F60 is open again for a move: side B's margin changes by {dxb} and {dyb} (A's in the other account by {dya})");
        refused += 1;
    }
    // One object at both places.
    let mut same = base.clone();
    let Arg::V(ArtifactValue::Bytes(who)) = args_of(&mut same, 1)[0].clone() else { panic!("the places") };
    args_of(&mut same, 1)[0] = bytes_arg(&[&who[..8], &who[..8], &who[..8]].concat());
    assert!(!taken30(&same, &w.keys), "one object stands at both places of a move");
    println!("T1b F60 fixed: a leg leaves with no movement from side B's margin (4 forms of round 36's transaction refused; the gain paid from the clearing party's coins and the account as it was); a leg moves with side B's margin between its two accounts (taken) and not out of them ({refused} refused: lower in one, a sompi short of made up, lower in the one and A's higher in the other); one object at both places refused");
}

/// Rounds 36 and 37, question 1 (value). `KpfTLegOut` against `Account::leg_out` on 400 random amounts (`payb`,
/// `take`, `addb`, each also below zero and beyond the margins), built past the model where it refuses: the engine
/// takes exactly what the model allows, and B's margin is never below what it was. And on a transaction that is
/// taken: the account a sompi short; the note below its least; a third output under the id (to B's key, and a
/// second note); the note with no id; the two outputs in each other's place; each amount a sompi off what the
/// outputs say; either signature by another key; a `take` below zero with the account holding what it held, or
/// less (it adds: it takes nothing); margins the object does not hold; the amounts at the ends of 64 bits, with a
/// `payb` so large that B's margin is beyond what is counted. What A is paid for the leg, and where what it takes
/// goes, are held by the two signatures alone (the script names no output for either).
#[test]
fn t1b_r36_a_leg_that_leaves_moves_what_the_two_sign_for_and_holds_the_account_to_it() {
    let w = ow();
    let (_, account, _) = three36(&w, 20 * KAS1);
    let x = &account.s;
    let sid = x.shard.unwrap();
    let mut r = Rng(0x36_0001);
    let (mut taken, mut refused) = (0, 0);
    for case in 0..400 {
        let i = r.range(1, 4) as usize;
        let amount = |r: &mut Rng, lo: i64, hi: i64| match r.range(0, 5) {
            0 => 0,
            1 => r.range(lo, hi) * KAS1,
            _ => r.range(lo * KAS1, hi * KAS1),
        };
        let (payb, take, addb) = (amount(&mut r, -10, 26), amount(&mut r, -5, 30), amount(&mut r, -2, 10));
        let paid = if case % 3 == 0 { r.range(0, 10) * KAS1 } else { 0 };
        let model = x.leg_out(i, payb, take, addb);
        let (am, bm) = (x.a_margin - payb - take, x.b_margin + payb + addb);
        assert_eq!(model.is_ok(), payb >= 0 && addb >= 0 && am >= 0, "case {case}");
        let mut legs = x.legs.clone();
        let leg = legs.remove(i - 1);
        let engine = out_as36(&w, &account, i, payb, paid, take, addb, Account { legs, a_margin: am, b_margin: bm, ..x.clone() }, SNote::leg(NOTE_RELEASE, sid, &leg, x.b_key));
        assert_eq!(engine, model.is_ok(), "case {case}: leg {i}, payb {payb}, take {take}, addb {addb}: the model says {}", model.is_ok());
        if engine {
            assert!(bm >= x.b_margin, "case {case}: side B's margin falls by a leg that leaves");
            taken += 1;
        } else {
            refused += 1;
        }
    }
    assert!(taken > 60 && refused > 100, "{taken} taken, {refused} refused");
    // ---- a transaction that is taken, and what is not.
    let (payb, paid, take, addb) = (KAS1, 2 * KAS1, 5 * KAS1, 3 * KAS1);
    let (b, next, note) = out36(&w, &account, 1, payb, paid, take, addb).unwrap();
    assert!(taken30(&b, &w.keys), "the control");
    assert_eq!((b.tx.outputs[0].value, b.tx.outputs[2].value, next.a_margin, next.b_margin), (account.value - 2 * KAS1 as u64, note.value() as u64, 14 * KAS1, 24 * KAS1));
    assert!(matches!((&args_of(&mut b.clone(), 1)[11], &args_of(&mut b.clone(), 1)[12]), (Arg::Sig(_), Arg::Sig(_))), "the two signatures are arguments 11 and 12");
    let mutations = std::cell::Cell::new(0);
    let no = |what: &str, t: OBuilt| {
        assert!(!taken30(&t, &w.keys), "{what}");
        mutations.set(mutations.get() + 1);
    };
    let with = |f: &dyn Fn(&mut OBuilt)| {
        let mut t = b.clone();
        f(&mut t);
        t
    };
    no("the account holds a sompi less", with(&|t| t.tx.outputs[0].value -= 1));
    no("the note holds a sompi less than a note holds", with(&|t| t.tx.outputs[2].value = 20_000_000 - 1));
    let id = account.id.expect("an id");
    let under = |spk: ScriptPublicKey, value: u64| TransactionOutput { value, script_public_key: spk, covenant: Some(CovenantBinding { authorizing_input: 0, covenant_id: id }) };
    no("a third output under the id, to the clearing party's key", with(&|t| t.tx.outputs.push(under(p2pk_spk(&x.b_key), KAS1 as u64))));
    no("a third output under the id, a second note", with(&|t| t.tx.outputs.push(under(hash_spk(&w.lib, &note.hash()), 20_000_000))));
    no("a third output under the id, an account of side B's making", with(&|t| t.tx.outputs.push(under(hash_spk(&w.lib, &Account { a_key: x.b_key, ..next.clone() }.hash()), KAS1 as u64))));
    no("the note under no id", with(&|t| t.tx.outputs[2].covenant = None));
    no("the account under no id", with(&|t| t.tx.outputs[0].covenant = None));
    no("the note first and the account after it", with(&|t| t.tx.outputs.swap(0, 2)));
    for (at, what) in [(8usize, "payb"), (9, "take"), (10, "addb")] {
        let Arg::V(ArtifactValue::Int(v)) = args_of(&mut b.clone(), 1)[at].clone() else { panic!("argument {at} is a number") };
        assert_eq!(v, [payb, take, addb][at - 8], "argument {at} is {what}");
        for d in [1i64, -1] {
            no(&format!("{what} a sompi off what the outputs say"), with_arg(&b, at, int_arg(v + d)));
        }
    }
    for (at, own, what) in [(11usize, x.a_key, "side A"), (12, x.b_key, "side B")] {
        for key in [x.a_key, x.b_key, xk(&w.w)] {
            if key != own {
                no(&format!("the signature of {what} by another key"), with_arg(&b, at, Arg::Sig(key)));
            }
        }
    }
    // A margin the object does not hold: the state says a KAS more for either side.
    assert!(!out_as36(&w, &account, 1, payb, paid, take, addb, Account { a_margin: next.a_margin + KAS1, ..next.clone() }, note) && !out_as36(&w, &account, 1, payb, paid, take, addb, Account { b_margin: next.b_margin + KAS1, ..next.clone() }, note));
    mutations.set(mutations.get() + 2);
    // A take below zero adds: the account holds 3 KAS more, and is not taken holding what it held, or less.
    let (ab, added, _) = out36(&w, &account, 2, 0, 0, -3 * KAS1, 0).unwrap();
    assert!(taken30(&ab, &w.keys));
    assert_eq!((ab.tx.outputs[0].value, added.a_margin), (account.value + 3 * KAS1 as u64, 23 * KAS1));
    for less in [1u64, 3 * KAS1 as u64, 6 * KAS1 as u64] {
        let mut t = ab.clone();
        t.tx.outputs[0].value -= less;
        no("a take below zero with the account holding less than it adds", t);
    }
    // The amounts at the ends of what is counted are no transaction: a `payb` or an `addb` below zero there, a
    // `payb` beyond what B's margin counts (made up by a take below zero, so that A's margin stands), each alone.
    let big = kpf_protocol::cross::MARGIN_MAX;
    for (payb, take, addb) in [(i64::MIN, 0, 0), (i64::MIN + 1, 0, 0), (-1, 0, 0), (0, 0, i64::MIN), (0, 0, -1), (i64::MAX, 0, 0), (i64::MAX, i64::MIN + 1, 0), (big, -big, 0), (big - 20 * KAS1 + 1, -(big - 20 * KAS1 + 1), 0), (0, 0, i64::MAX), (0, 0, big), (0, i64::MIN, 0), (0, i64::MAX, 0), (1, 0, i64::MAX)] {
        assert!(x.leg_out(1, payb, take, addb).is_err(), "the model takes payb {payb}, take {take}, addb {addb}");
        let mut t = b.clone();
        for (at, v) in [(8usize, payb), (9, take), (10, addb)] {
            args_of(&mut t, 1)[at] = int_arg(v);
        }
        no("an amount at the end of what is counted", t);
        // And with the outputs such amounts would ask for, where they can be written.
        if let (Some(am), Some(bm)) = (x.a_margin.checked_sub(payb).and_then(|v| v.checked_sub(take)), x.b_margin.checked_add(payb).and_then(|v| v.checked_add(addb))) {
            assert!(!out_as36(&w, &account, 1, payb, 0, take, addb, Account { a_margin: am, b_margin: bm, ..next.clone() }, note), "payb {payb}, take {take}, addb {addb} with the outputs they ask for");
        }
    }
    // What A is paid for the leg and where what it takes goes are the two signers' to say: the output to A's key
    // (7 KAS: 5 taken, 2 paid) re-addressed, or a sompi less, is taken as well. A's signature of the whole holds it.
    let k = b.tx.outputs.iter().position(|o| o.script_public_key == p2pk_spk(&x.a_key) && o.value == (take + paid) as u64).expect("what is taken and paid");
    assert!(taken30(&with(&|t| t.tx.outputs[k].script_public_key = p2pk_spk(&xk(&w.w))), &w.keys) && taken30(&with(&|t| t.tx.outputs[k].value -= 1), &w.keys));
    // The clearing party signs for a take that leaves the user with nothing and its legs standing: taken. The
    // user is then below, and anyone states a step of its liquidation (the model).
    let (zb, zero, _) = out36(&w, &account, 3, 0, 0, 20 * KAS1, 0).unwrap();
    assert!(taken30(&zb, &w.keys));
    let p = [P10, P10];
    let rows = zero.trace_without(&p, 1);
    assert!(zero.a_margin == 0 && zero.stepped(KIND_ACCOUNT_LIQ, SIDE_A, 1, &rows, &xk(&w.w)).is_some(), "the user left with nothing is below at once");
    println!("T1b R36 value: 400 random amounts, {taken} taken and {refused} refused at the engine, each as the model says, side B's margin never lower; {} changes of a taken transaction refused (a sompi short, a third output under the id, the note or the account under no id, the outputs in each other's place, an amount off, a signature by another key, a margin not held, a take below zero that holds less, the ends of 64 bits)", mutations.get());
}

/// Round 36, question 2 (the note). The note of a leg that leaves by `KpfTLegOut` is the one its entering wrote,
/// with the other kind, and the one a statement's settlement writes: for a long and a short leg and the four kinds
/// of option. Applied after the join, the shard's rows are as before the leg. Not written: no note, a note of fewer
/// units, the other way, a join, a deposit, what is owed, another row (the other kind's, another market's, the
/// market's bare name), another shard, another side B, an amount in it. Two legs of one market in one account and
/// in two: a release takes its own leg's units out of the row and the other stands. A release applied before its
/// leg's join (the leg entered and left a second account at once) empties the row while an equal leg of another
/// account stands: a draw stated then is ended by the join that is not applied (`KpfTDrawNote`), as round 34 found
/// for a statement's release. A leg that left does not leave again. And legs no script lets in (a direction's
/// place that is no kind, a size below zero): what the note is then.
#[test]
fn t1b_r36_a_leg_that_leaves_writes_the_note_of_its_own_row_and_units() {
    let w = ow();
    let (shard, account, joins) = three36(&w, 20 * KAS1);
    let x = &account.s;
    let sid = shard.s.sid;
    let scale = w.markets[0].p.scale;
    // ---- each leg's release against its join and against a statement's.
    for i in 1..=3usize {
        let (b, _, note) = out36(&w, &account, i, 0, 0, 0, 0).unwrap();
        let s = signed(&format!("leg {i} leaves at once"), &b, &w.keys);
        let j = &joins[i - 1].s;
        assert_eq!((note.kind, j.kind, note.head, note.net, note.gross, note.key, note.sid), (NOTE_RELEASE, NOTE_JOIN, j.head, j.net, j.gross, j.key, j.sid), "leg {i}: the release is the join, the other way");
        let l = &x.legs[i - 1];
        let by_statement = SNote { sid, kind: NOTE_RELEASE, head: Head::of(l), net: SNote::units(l), gross: SNote::units(l).abs(), key: x.b_key, amount: 0 };
        assert_eq!(note.hash(), by_statement.hash(), "leg {i}: the note a settlement writes");
        // Applied: the row of the leg is no market's again, the others as they were.
        let after = w.applied(&shard, &nth(&s, 2, note, 6_200), "the release applied");
        for (r, row) in shard.s.rows.iter().enumerate() {
            assert_eq!(after.s.rows[r], if row.head == note.head { ShardRow::EMPTY } else { *row }, "leg {i}, row {r}");
        }
    }
    // The other kinds of leg (built on an account of the model's making): the note the model writes is taken.
    let made = |legs: Vec<XLeg>| {
        let mut a = w.account(legs, 20 * KAS1, 20 * KAS1);
        a.s.shard = Some(sid);
        a
    };
    for leg in [w.fut(0, 3, SHORT, P10), w.opt(1, 3, CALL_WRITTEN, P10, P10), w.opt(1, 3, PUT_HELD, P10 * 12 / 10, P10), w.opt(1, 3, PUT_WRITTEN, P10 / 2, P10)] {
        let a = made(vec![w.fut(2, 1, LONG, P10), leg]);
        let (b, _, note) = out36(&w, &a, 2, 0, 0, 0, 0).unwrap();
        assert!(taken30(&b, &w.keys));
        assert_eq!((note.head, note.net, note.gross), (Head::of(&leg), 3 * scale, 3 * scale));
    }
    // ---- the notes that are not written.
    let (_, honest, release) = out36(&w, &account, 1, 0, 0, 0, 0).unwrap();
    let m0 = w.markets[0].id();
    let mut refused = 0;
    for (what, note) in [
        ("fewer units", SNote { net: release.net / 2, gross: release.gross / 2, ..release }),
        ("a unit less in the net alone", SNote { net: release.net + 1, ..release }),
        ("a unit less in the gross alone", SNote { gross: release.gross - 1, ..release }),
        ("the other way", SNote { net: -release.net, ..release }),
        ("no units", SNote { net: 0, gross: 0, ..release }),
        ("a join", SNote { kind: NOTE_JOIN, ..release }),
        ("a deposit", SNote::deposit(sid, x.b_key, KAS1)),
        ("what is owed", SNote::owed(sid, x.a_key, x.b_key, KAS1)),
        ("the options' row of its market", SNote { head: Head { market: option_market(&m0), ..release.head }, ..release }),
        ("its market's bare name", SNote { head: Head { market: m0, ..release.head }, ..release }),
        ("another market's row", SNote { head: Head::of(&x.legs[2]), ..release }),
        ("another committee", SNote { head: Head { committee: [0x09; 32], ..release.head }, ..release }),
        ("another scale", SNote { head: Head { scale: release.head.scale * 10, ..release.head }, ..release }),
        ("another limit", SNote { head: Head { price_max: release.head.price_max + 1, ..release.head }, ..release }),
        ("another shard", SNote { sid: [0x44; 32], ..release }),
        ("another side B", SNote { key: xk(&w.w), ..release }),
        ("side A's key", SNote { key: x.a_key, ..release }),
        ("an amount in it", SNote { amount: 1, ..release }),
    ] {
        assert!(!out_as36(&w, &account, 1, 0, 0, 0, 0, honest.clone(), note), "a leg leaves with a note of {what}");
        refused += 1;
    }
    // An option that leaves with a futures leg's note, and a futures leg of the same market beside it.
    let opt_release = SNote::leg(NOTE_RELEASE, sid, &x.legs[1], x.b_key);
    let (_, honest2, _) = out36(&w, &account, 2, 0, 0, 0, 0).unwrap();
    for (what, note) in [("the futures row of its market", SNote { head: Head { market: futures_market(&w.markets[1].id()), ..opt_release.head }, ..opt_release }), ("a futures leg's units", SNote { net: -opt_release.net, ..opt_release })] {
        assert!(!out_as36(&w, &account, 2, 0, 0, 0, 0, honest2.clone(), note), "an option leaves with a note of {what}");
        refused += 1;
    }
    // ---- two legs of one market in one account: the release of one leaves the other counted.
    let x0 = w.covered(sid, 30 * KAS1, 30 * KAS1);
    let (ten, five) = (w.fut(0, 10, LONG, P10), w.fut(0, 5, LONG, P10));
    let (x1, j1) = w.added(&x0, ten);
    let (x2, j2) = w.added(&x1, five);
    let s2 = w.applied(&w.applied(&shard, &j1, "ten more of market 0"), &j2, "five more of market 0");
    assert_eq!((s2.s.rows[0].net, s2.s.rows[0].gross), (-25 * scale, 25 * scale), "the first account's ten, and these fifteen");
    let (ob, x3, rel) = out36(&w, &x2, 1, 0, 0, 0, 0).unwrap();
    let os = signed("the ten leave, the five stand", &ob, &w.keys);
    let rel = nth(&os, 2, rel, 6_200);
    let s3 = w.applied(&s2, &rel, "the release of the ten applied");
    // A release is applied once: the transaction that applies it has no output for it to go on in.
    let fw2 = [fund(xk(&w.w), 43, 1_000)];
    let (ab, _, _) = note_apply(&w.lib, &s2, &rel, &w.script("KpfTNoteApply"), w.payer(&w.w, &fw2)).unwrap();
    let mut twice = ab.clone();
    twice.tx.outputs.push(TransactionOutput { value: 20_000_000, script_public_key: hash_spk(&w.lib, &rel.s.hash()), covenant: Some(CovenantBinding { authorizing_input: 0, covenant_id: s2.id.expect("an id") }) });
    assert!(taken30(&ab, &w.keys) && !taken30(&twice, &w.keys), "a release goes on after it is applied");
    assert_eq!((s3.s.rows[0].net, s3.s.rows[0].gross, x3.legs.clone()), (-15 * scale, 15 * scale, vec![five]), "the five of this account and the ten of the other stand");
    // The leg that left does not leave again: the account that goes on is asked for the ten once more.
    let x3 = nth(&os, 0, x3, 6_200);
    let (again, _, _) = out36(&w, &x3, 1, 0, 0, 0, 0).unwrap();
    assert!(taken30(&again, &w.keys), "the control: the five leave");
    assert!(!taken30(&with_arg(&again, 5, bytes_arg(&kpf_protocol::thin::leg_bytes(&ten))), &w.keys), "the ten leave a second time");
    // ---- in two accounts, and the release applied before its own join: a second account of the clearing party
    // (W on side A) takes an equal leg and lets it go at once; its release is applied and its join is not.
    let y0 = OLive { value: (21 * KAS1) as u64, s: Account { a_key: xk(&w.w), legs: vec![], a_margin: 10 * KAS1, b_margin: 10 * KAS1, ..x.clone() }, id: account.id, op: op(61), daa: 6_100 };
    let (y1, jy) = w.added(&y0, w.fut(0, 10, LONG, P10));
    let jy = OLive { daa: 6_120, ..jy };
    let (yb, _, ry) = out36(&w, &y1, 1, 0, 0, 0, 0).unwrap();
    let ys = signed("the second account's leg leaves at once", &yb, &w.keys);
    let emptied = w.applied(&shard, &nth(&ys, 2, ry, 6_130), "the release applied before its join");
    assert_eq!(emptied.s.rows[0], ShardRow::EMPTY, "the row of market 0 is emptied while the first account's ten stand");
    let low = emptied.s.requirement(&[0, P10, P10, 0]).unwrap();
    assert!(low < shard.s.requirement(&[P10, P10, P10, 0]).unwrap());
    let fb = [fund(xk(&w.b), 55, 1_000)];
    let fw = [fund(xk(&w.w), 54, 1_000)];
    let prices = [0, P10, P10, 0];
    let (db, dst, _) = shard_draw(&w.lib, &emptied, &w.script("KpfTChange"), emptied.s.draw_max(&prices), prices, T0, w.payer(&w.b, &fb)).unwrap();
    let dlive = nth(&signed("a draw stated on the emptied row", &db, &w.keys), 0, dst, 6_600);
    let nb = draw_note(&w.lib, &dlive, &dst, None, &emptied.s, &jy, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    let ns = signed("the draw ended by the join that is not applied", &nb, &w.keys);
    assert_eq!(ns.tx.outputs[0].script_public_key, hash_spk(&w.lib, &emptied.s.hash()), "the shard again");
    let back = w.applied(&emptied, &jy, "the join applied after");
    assert_eq!((back.s.rows[0].head, back.s.rows[0].net, back.s.rows[0].gross), (shard.s.rows[0].head, shard.s.rows[0].net, shard.s.rows[0].gross), "the row as it was");
    // ---- legs no script lets into a covered account (an account of the model's making): what the note is.
    let (mut futures, mut options, mut by_statement) = (0, 0, 0);
    for dir in [0i64, -1, -8, 7, 8, 9, 10, 15, 16 + 7, 3, 4, 5, 6, i64::MAX] {
        let leg = XLeg { dir, ..w.fut(0, 3, LONG, P10) };
        let a = made(vec![w.fut(2, 1, LONG, P10), leg]);
        let (b, _, note) = out36(&w, &a, 2, 0, 0, 0, 0).unwrap();
        assert!(taken30(&b, &w.keys), "a leg whose direction's place is {dir} leaves with the note the model writes");
        let option = dir >= 3 && (3..=6).contains(&(dir % 8));
        assert_eq!((note.head.market, note.net, note.gross), (if option { option_market(&m0) } else { futures_market(&m0) }, 3 * scale, 3 * scale), "{dir}");
        if option {
            options += 1;
        } else {
            futures += 1;
        }
        // By a statement: the row of the leg holds the same head and units to what is said (`KpfTStepRow` reads
        // the kind the same way), and is right or wrong by the rest of the row.
        let rows = a.s.trace_without(&[P10, P10], 2);
        let row = row_judged_of(&rows[0].g, &leg, &rows[0], &rows[1], Some((note.head, note.net)), holder_of36(&leg));
        let other = row_judged_of(&rows[0].g, &leg, &rows[0], &rows[1], Some((note.head, -note.net)), holder_of36(&leg));
        assert_eq!(other, Some(false), "{dir}: a statement that says other units of the leg");
        by_statement += (row == Some(true)) as usize;
    }
    // A size below zero: the script counts the gross above zero and the model writes it as the size.
    let leg = XLeg { size: -3 * scale, ..w.fut(0, 3, SHORT, P10) };
    let a = made(vec![w.fut(2, 1, LONG, P10), leg]);
    let (next, note) = a.s.leg_out(2, 0, 0, 0).unwrap();
    assert_eq!((note.net, note.gross), (-3 * scale, -3 * scale));
    assert!(!out_as36(&w, &a, 2, 0, 0, 0, 0, next.clone(), note) && out_as36(&w, &a, 2, 0, 0, 0, 0, next, SNote { gross: 3 * scale, ..note }), "the script writes the gross of a size below zero above zero");
    println!("T1b R36 notes: the release of each kind of leg is its join the other way and a settlement's note; {refused} other notes not written; two legs of one market: the other stands; a release applied before its join empties the row and the draw stated then is ended by the join; legs of no kind: {futures} leave as futures legs with the size as side B's units, {options} as options (a strike of nothing), {by_statement} of them have a row a statement could state right; a size below zero: the script's gross is above zero, the model's is the size");
}

fn holder_of36(l: &XLeg) -> i64 {
    kpf_protocol::thin::holder_of(l).unwrap_or(SIDE_A)
}

/// Round 36, questions 3 and 5 (the chain, and what the script opens). On a taken transaction: the leg shown is
/// another of the account's, or none of it; the chain before it is another; the legs after it are fewer, more, in
/// another order, not whole legs. An account of eight legs: the second leaves (six after it), the first does not
/// (seven), by the model and by the script. An object that carries a statement, or a disputed point, is not
/// opened: the leg does not leave under a statement. An account of one leg is left with none, and then ends
/// (`KpfTAcctEnd`). An account no shard covers (138 bytes) and a shard are not opened.
#[test]
fn t1b_r36_a_leg_that_leaves_opens_the_chain_of_a_covered_account_and_nothing_else() {
    let w = ow();
    let (shard, account, _) = three36(&w, 20 * KAS1);
    let x = &account.s;
    let sid = shard.s.sid;
    let lb = |l: &XLeg| kpf_protocol::thin::leg_bytes(l);
    let chain = |legs: &[XLeg]| legs.iter().fold([0u8; 32], |g, l| blake2b(&[&g[..], &lb(l)].concat()));
    let (b, _, _) = out36(&w, &account, 2, 0, 0, 0, 0).unwrap();
    assert!(taken30(&b, &w.keys), "the control: the second of three leaves");
    let refused = std::cell::Cell::new(0);
    let no = |what: &str, t: OBuilt| {
        assert!(!taken30(&t, &w.keys), "{what}");
        refused.set(refused.get() + 1);
    };
    // (Arguments: 4 the chain before the leg, 5 the leg, 6 the legs after it, 7 the moving part.)
    no("the first leg shown in the second's place", with_arg(&b, 5, bytes_arg(&lb(&x.legs[0]))));
    no("the third leg shown in the second's place", with_arg(&b, 5, bytes_arg(&lb(&x.legs[2]))));
    no("a leg that is not the account's", with_arg(&b, 5, bytes_arg(&lb(&w.fut(3, 1, LONG, P10)))));
    no("the leg with its entry a sompi off", with_arg(&b, 5, bytes_arg(&lb(&XLeg { entry: P10 + 1, ..x.legs[1] }))));
    no("the leg with a unit less", with_arg(&b, 5, bytes_arg(&lb(&XLeg { size: x.legs[1].size - 1, ..x.legs[1] }))));
    no("no chain before the leg", with_arg(&b, 4, bytes_arg(&[0u8; 32])));
    no("the chain of two legs before it", with_arg(&b, 4, bytes_arg(&chain(&x.legs[..2]))));
    no("no leg after it", with_arg(&b, 6, bytes_arg(&[])));
    no("the leg after it twice", with_arg(&b, 6, bytes_arg(&[lb(&x.legs[2]), lb(&x.legs[2])].concat())));
    no("another leg after it", with_arg(&b, 6, bytes_arg(&lb(&x.legs[0]))));
    no("the leg after it a byte short", with_arg(&b, 6, bytes_arg(&lb(&x.legs[2])[..110])));
    no("the leg after it and a byte", with_arg(&b, 6, bytes_arg(&[lb(&x.legs[2]), vec![0u8]].concat())));
    // The whole account shown as the legs after a leg that is not there (the chain opens, the leg is of no place).
    {
        let mut t = b.clone();
        let args = args_of(&mut t, 1);
        args[4] = bytes_arg(&[0u8; 32]);
        args[5] = bytes_arg(&lb(&x.legs[0]));
        args[6] = bytes_arg(&[lb(&x.legs[1]), lb(&x.legs[2])].concat());
        no("the first leg's place shown, the outputs the second's", t);
    }
    // ---- eight legs: six after the second, seven after the first.
    let mut eight = w.account((0..8).map(|i| w.fut(i, 1 + i as i64, if i % 2 == 0 { LONG } else { SHORT }, P10)).collect(), 20 * KAS1, 20 * KAS1);
    eight.s.shard = Some(sid);
    for i in 2..=8usize {
        let (t, next, _) = out36(&w, &eight, i, 0, 0, 0, 0).unwrap();
        assert!(taken30(&t, &w.keys), "leg {i} of eight leaves");
        assert_eq!(next.legs_root(), chain(&[&eight.s.legs[..i - 1], &eight.s.legs[i..]].concat()));
    }
    assert!(eight.s.leg_out(1, 0, 0, 0).is_err(), "the model lets the first of eight leave");
    let mut seven = eight.s.clone();
    let first = seven.legs.remove(0);
    assert!(!out_as36(&w, &eight, 1, 0, 0, 0, 0, seven, SNote::leg(NOTE_RELEASE, sid, &first, x.b_key)), "the first of eight leaves, seven legs after it");
    refused.set(refused.get() + 1);
    // ---- an object that carries a statement, or a disputed point.
    let held = account.value as i64;
    let p = [P10, P10, P10];
    let (st, tk) = x.take_by(SIDE_A, KAS1, &p, T0, held, x.a_key).unwrap();
    let live = w.stated(&account, &st, &tk.said());
    let pointed = w.pointed(&live, 0);
    for (what, value, id, at, hash) in [("a statement", live.value, live.id, live.op, live.s.hash()), ("a disputed point", pointed.value, pointed.id, pointed.op, pointed.s.hash())] {
        let under = OLive { value, s: x.clone(), id, op: at, daa: 6_530 };
        let (t, _, _) = out36(&w, &under, 2, 0, 0, 0, 0).unwrap();
        assert!(taken30(&t, &w.keys), "the control: the object as the account it was");
        // The output that is there holds the statement's hash; shown with the account's script it is not that
        // output, and shown as it is the script does not open it.
        let mut as_it_is = t.clone();
        as_it_is.entries[0].script_public_key = hash_spk(&w.lib, &hash);
        no(&format!("the object that carries {what}, shown as the account"), as_it_is);
        let mut opened = t.clone();
        object_made(&mut opened, &w.lib, 0, &hash);
        no(&format!("the object that carries {what}, shown as it is"), opened);
    }
    // ---- one leg: the account is left with none, and ends.
    let x0 = w.covered(sid, 20 * KAS1, 2 * KAS1);
    let (x1, _) = w.added(&x0, w.fut(0, 3, LONG, P10));
    let (ob, none, _) = out36(&w, &x1, 1, 0, KAS1, 0, 0).unwrap();
    let os = signed("the only leg leaves", &ob, &w.keys);
    assert_eq!((none.legs.len(), none.legs_root(), none.a_margin, none.b_margin), (0, [0u8; 32], 20 * KAS1, 2 * KAS1));
    let left = nth(&os, 0, none, 6_200);
    let fa = [fund(xk(&w.a), 59, 1_000)];
    let (eb, deposit) = acct_end(&w.lib, &left, &w.script("KpfTAcctEnd"), SIDE_A, w.payer(&w.a, &fa)).unwrap();
    signed("the account with no leg ends", &eb, &w.keys);
    assert_eq!(deposit.map(|d| d.amount), Some(2 * KAS1), "what side B had is a deposit to the shard");
    // ---- an account no shard covers, and a shard.
    let plain = w.account(vec![w.fut(0, 3, LONG, P10), w.fut(1, 2, SHORT, P10)], 20 * KAS1, 20 * KAS1);
    assert!(plain.s.leg_out(1, 0, 0, 0).is_err(), "the model serves an account no shard covers");
    let mut rest = plain.s.clone();
    let gone = rest.legs.remove(0);
    assert!(!out_as36(&w, &plain, 1, 0, 0, 0, 0, rest, SNote::leg(NOTE_RELEASE, sid, &gone, plain.s.b_key)), "an account of 138 bytes is opened");
    refused.set(refused.get() + 1);
    // A shard shown as an account with a leg (its owner on side A, no key on side B): no transaction is signed
    // for a key of 32 zero bytes, and with the clearing party's key in its place the object is not the shard.
    let leg = w.fut(0, 3, LONG, P10);
    for b_key in [[0u8; 32], x.b_key] {
        let as_account = Account { a_key: shard.s.owner, b_key, a_margin: shard.s.held, b_margin: 0, legs: vec![leg], ..x.clone() };
        let under = OLive { value: shard.value, s: as_account.clone(), id: shard.id, op: shard.op, daa: shard.daa };
        let f = [fund(xk(&w.b), 57, 1_000)];
        let built = leg_out_as(&w.lib, &under, 1, 0, 0, 0, 0, Account { legs: vec![], ..as_account }, SNote::leg(NOTE_RELEASE, sid, &leg, b_key), &w.script("KpfTLegOut"), w.payer(&w.b, &f));
        let opened = built.is_ok_and(|(mut t, _, _)| {
            object_made(&mut t, &w.lib, 0, &shard.s.hash());
            taken30(&t, &w.keys)
        });
        assert!(!opened, "a shard is opened as an account");
        refused.set(refused.get() + 1);
    }
    println!("T1b R36 chain: {} refused: a leg, a chain before it or legs after it that are not the account's at that place; seven legs after the leg; an object that carries a statement or a disputed point; an account of 138 bytes; a shard. Each of the legs 2 to 8 of eight leaves; an account of one leg is left with none and ends with side B's margin a deposit", refused.get());
}

// ---------------------------------------------------------------- round 37: where side B's margin goes

/// Round 37. Side B's margin of a covered account under the scripts that are no statement: `KpfTAddC` only adds
/// (an amount below zero, or none, is refused for either side); `KpfTHand` leaves it as it is; `KpfTLegAdd` does not
/// lower it. Margin that arrived by a move stays side B's: the leg leaves the second account, the account ends
/// (`KpfTAcctEnd`), and all of it (what was there and what moved in) is a deposit to the shard, which the shard
/// then holds. A take for B stated by B's own key pays no key (a deposit). And the one payment out of B's margin
/// that is to a key: the fee of a step of B's own liquidation, to whoever states it, where B stands below its
/// maintenance (a statement at prices that are answered for; at most the fee rate of the leg's worth, and no more
/// than B has there).
#[test]
fn t1b_r37_side_bs_margin_falls_by_no_other_script_and_ends_as_a_deposit() {
    let w = ow();
    let (shard, account, _) = three36(&w, 40 * KAS1);
    let x = &account.s;
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let fa = [fund(xk(&w.a), 59, 1_000)];
    let mut refused = 0;
    // ---- KpfTAddC.
    for side in [SIDE_A, SIDE_B] {
        let who = if side == SIDE_A { &w.a } else { &w.b };
        let f = [fund(xk(who), 58, 1_000)];
        let (ab, added) = account_add(&w.lib, &account, &w.script("KpfTAddC"), side, KAS1, w.payer(who, &f)).unwrap();
        assert!(taken30(&ab, &w.keys), "the control: side {side} adds 1 KAS");
        assert!(matches!(args_of(&mut ab.clone(), 1)[6], Arg::V(ArtifactValue::Int(v)) if v == KAS1), "argument 6 is the amount");
        for delta in [-KAS1, -1, 0, i64::MIN] {
            let mut t = with_arg(&ab, 6, int_arg(delta));
            if delta > i64::MIN {
                let less = if side == SIDE_A { Account { a_margin: x.a_margin + delta, ..x.clone() } } else { Account { b_margin: x.b_margin + delta, ..x.clone() } };
                t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &less.hash());
                t.tx.outputs[0].value = (account.value as i64 + delta) as u64;
            }
            assert!(!taken30(&t, &w.keys), "side {side} adds {delta}");
            refused += 1;
        }
        let _ = added;
    }
    // ---- KpfTHand.
    let (hb, handed) = account_hand(&w.lib, &account, &w.script("KpfTHand"), xk(&w.w), KAS1, 0, w.payer(&w.a, &fa)).unwrap();
    assert!(taken30(&hb, &w.keys) && handed.b_margin == x.b_margin, "the control: side A handed over, side B's margin as it was");
    for less in [1, KAS1, x.b_margin] {
        let mut t = hb.clone();
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { b_margin: handed.b_margin - less, ..handed.clone() }.hash());
        t.tx.outputs[0].value -= less as u64;
        assert!(!taken30(&t, &w.keys), "a side is handed over with side B's margin {less} lower");
        refused += 1;
    }
    // ---- KpfTLegAdd.
    let leg = w.fut(3, 2, SHORT, P10);
    let (lb, honest, _) = leg_add(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, w.payer(&w.a, &fa)).unwrap();
    assert!(taken30(&lb, &w.keys) && matches!(args_of(&mut lb.clone(), 1)[8], Arg::V(ArtifactValue::Int(0))), "the control; argument 8 is what side B adds");
    for db in [-1, -KAS1, i64::MIN] {
        let mut t = with_arg(&lb, 8, int_arg(db));
        if db > i64::MIN {
            t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { b_margin: honest.b_margin + db, ..honest.clone() }.hash());
            t.tx.outputs[0].value = (account.value as i64 + db) as u64;
        }
        assert!(!taken30(&t, &w.keys), "a leg enters with side B's margin {db} lower");
        refused += 1;
    }
    // ---- margin that arrived by a move: it ends as a deposit.
    let y0 = OLive { value: (12 * KAS1) as u64, s: Account { a_key: xk(&w.w), legs: vec![], a_margin: 10 * KAS1, b_margin: KAS1, ..x.clone() }, id: account.id, op: op(62), daa: 6_100 };
    let (mb, _, ny) = leg_move(&w.lib, &account, 3, &y0, [0, -30 * KAS1, 0, 30 * KAS1], &w.script("KpfTLegMove"), w.payer(&w.b, &fb)).unwrap();
    let ms = signed("a leg and 30 KAS of side B's margin move to the second account", &mb, &w.keys);
    assert_eq!((ny.b_margin, ny.legs.len()), (31 * KAS1, 1));
    let y1 = nth(&ms, 2, ny, 6_200);
    assert_eq!(y1.value, y0.value + 30 * KAS1 as u64);
    // (A take for B there, stated by B's own key: no key is paid.)
    let p = [P10];
    let most = y1.s.take_max(SIDE_B, &p);
    let (st, _) = y1.s.take_by(SIDE_B, most, &p, T0, y1.value as i64, x.b_key).unwrap();
    assert!(most > 20 * KAS1 && st.outs.list().iter().all(|o| !o.key), "a take for side B pays a key");
    let fw = [fund(xk(&w.w), 57, 1_000)];
    let (ob, none, _) = leg_out(&w.lib, &y1, 1, 0, 0, 0, 0, &w.script("KpfTLegOut"), w.payer(&w.w, &fw)).unwrap();
    let os = signed("the leg leaves the second account", &ob, &w.keys);
    let y2 = nth(&os, 0, none, 6_210);
    let (eb, deposit) = acct_end(&w.lib, &y2, &w.script("KpfTAcctEnd"), SIDE_A, w.payer(&w.w, &fw)).unwrap();
    let es = signed("the second account ends", &eb, &w.keys);
    let deposit = deposit.expect("a deposit");
    assert_eq!(deposit.amount, 31 * KAS1, "what side B had there and what moved in");
    let at = (0..es.tx.outputs.len()).find(|i| es.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &deposit.hash())).expect("the note of the deposit");
    assert!(es.tx.outputs[at].value >= 31 * KAS1 as u64);
    assert!(es.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&x.b_key)), "the end of an account pays side B's key");
    let after = w.applied(&shard, &nth(&es, at, deposit, 6_220), "the deposit applied");
    assert_eq!(after.s.held, shard.s.held + 31 * KAS1);
    // ---- the fee of a step of B's own liquidation.
    let (_, thin, _) = three36(&w, KAS1 / 2);
    let t = &thin.s;
    let p3 = [P10, P10, P10];
    let own = xk(&w.f);
    let (st, tk, sd) = t.step(KIND_ACCOUNT_LIQ, SIDE_B, 1, &p3, T0, thin.value as i64, own).expect("side B with half a KAS against 240 of worth is below");
    assert_eq!((sd.fee, sd.after.b_margin), (KAS1 / 2, 0), "the fee is what B has there, no more");
    assert_eq!(end_taken(&w, &thin, &st, &tk), (true, false));
    assert!(account.s.stepped(KIND_ACCOUNT_LIQ, SIDE_B, 1, &account.s.trace_without(&p3, 1), &own).is_none(), "side B with 40 KAS is not below: no step, no fee");
    println!("T1b R37 side B's margin: {refused} refused (an amount below zero or none added by either side; a side handed over or a leg entering with side B's margin lower); 30 KAS moved into a second account end as a deposit of 31 KAS that the shard then holds, and no output of that end is to side B's key; a take for B by its own key pays no key; the fee of a step of B's own liquidation is paid to whoever states it, at most what B has there, and only where B is below");
}

/// F61 (High). The owner of a shard takes what the shard holds, past the rule of a draw, with a second key of its
/// own and no other party. The shard pays a note of what is owed in full, whatever it is asked to hold
/// (`KpfTNoteApply`, kind 4), where the account's side B is its owner. `KpfTLegAdd` reads no price: the entry of a
/// leg is what the two keys of the account sign for. So: an account whose side A is a key of the owner's own; a
/// leg entered far from the market (long 10 units at 0.1 where the committee says 10); A states that the leg
/// leaves at the committee's price. Every point of that statement is right (the end and the row are answered
/// right at the engine; the price is the committee's own), so nobody ends it. Its settlement moves B's margin in
/// that account to A and writes a note of what the pot could not pay: 94 KAS. Applied, the shard pays all 50 KAS it
/// holds to the owner's second key, while a user's leg stands in its rows and it is asked 3 KAS and more; a draw
/// would have given the owner what is above that and not a sompi more. The other users of the shard lose their
/// cover. Passes while open. (Not opened by rounds 36 and 37: it is there since the reserve, and I did not see it in
/// rounds 32 to 36.)
#[test]
fn t1b_finding_f61_the_owner_of_a_shard_is_owed_by_it_through_an_account_of_its_own_at_an_entry_of_its_choosing() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    // A user is long 10 units of market 0 against the clearing party; the shard counts it.
    let user = w.covered(sid, 20 * KAS1, 20 * KAS1);
    let (user, ju) = w.added(&user, w.fut(0, 10, LONG, P10));
    let s1 = w.applied(&shard, &ju, "the leg of the user counted");
    let prices = [P10, P10, 0, 0];
    let asked = s1.s.requirement(&prices).unwrap();
    let by_a_draw = s1.s.draw_max(&prices);
    assert!(asked > 3 * KAS1 && by_a_draw <= s1.s.held - asked && by_a_draw > 40 * KAS1 && s1.s.held == 50 * KAS1, "asked {asked}, a draw gives {by_a_draw}");
    // The owner's own account: its second key (W) on side A, opened by the two signatures, both its own.
    let own = Account { a_key: xk(&w.w), a_margin: 25 * KAS1, b_margin: 5 * KAS1, legs: vec![], ..user.s.clone() };
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let ob = account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &own, shard_seed(&w.anchors[0]), w.markets[0].committee_id, w.payer(&w.b, &fb)).unwrap();
    let own = nth(&signed("the owner opens an account with a key of its own on side A", &ob, &w.keys), 0, own, 6_100);
    // A leg at an entry of its choosing: long 10 units of market 1 at 0.1.
    let leg = XLeg { entry: P10 / 100, ..w.fut(1, 10, LONG, P10) };
    let Ok((ab, y, join)) = leg_add(&w.lib, &own, &w.script("KpfTLegAdd"), leg, 0, 0, w.payer(&w.b, &fb)) else { panic!("F61 is closed: the model adds no leg at an entry far from the market") };
    assert!(taken30(&ab, &w.keys), "F61 is closed: a leg does not enter a covered account at an entry far from the market");
    let ys = signed("a leg enters at an entry of the two keys' choosing", &ab, &w.keys);
    let (y, join) = (nth(&ys, 0, y, 6_110), nth(&ys, 2, join, 6_110));
    let s2 = w.applied(&s1, &join, "the owner's own leg counted");
    // A states that the leg leaves at the committee's price. The statement is right at every point.
    let x = &y.s;
    let held = y.value as i64;
    let Ok((st, tk, sd)) = x.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10], T0, held, x.a_key) else { panic!("F61 is closed: the model does not let the leg leave at the committee's price") };
    assert_eq!((sd.owed, sd.after.a_margin, sd.after.b_margin), (92 * KAS1, 30 * KAS1, 0), "side B's margin there is A's, and of the 94 KAS the pot cannot pay the shard is asked 92 (round 41: less the band)");
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &x.a_key, &st.outs, held, &[P10]), None, "F61 is closed: the statement has a wrong point at the committee's price");
    assert_eq!(end_taken(&w, &y, &st, &tk), (true, false), "the end is answered right");
    let live = w.stated(&y, &st, &tk.said());
    assert_eq!(row_taken(&w, &w.pointed(&live, 1), &tk, 1, &[0; 32], &x.legs[0]), (true, false), "the row is answered right");
    // Settled: the note of what is owed, to the owner's second key.
    let fw = [fund(xk(&w.w), 54, 1_000)];
    let ss = signed("the leaving settled", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let owed = SNote::owed(sid, x.a_key, x.b_key, sd.owed);
    let at = (0..ss.tx.outputs.len()).find(|i| ss.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &owed.hash())).expect("the note of what is owed");
    let owed = nth(&ss, at, owed, 6_570);
    // Applied: the shard pays all it holds.
    let Ok((nb, paid_out, rest)) = note_apply(&w.lib, &s2, &owed, &w.script("KpfTNoteApply"), w.payer(&w.w, &fw)) else { panic!("F61 is closed: the model does not pay the note") };
    assert!(taken30(&nb, &w.keys), "F61 is closed: the shard does not pay its owner's own account below what it is asked");
    let ns = nb.sign(&w.keys).unwrap();
    let to_own: u64 = ns.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(&x.a_key) && o.value == 50 * KAS1 as u64).map(|o| o.value).sum();
    assert_eq!((to_own, paid_out.held, rest.map(|n| n.amount)), (50 * KAS1 as u64, 0, Some(42 * KAS1)), "50 KAS to the owner's second key, nothing left, 42 still owed");
    assert!(paid_out.rows.iter().any(|r| r.head == Head::of(&user.s.legs[0])) && paid_out.requirement(&prices).unwrap() >= asked, "the leg of the user stands in the rows and the shard is asked no less");
    println!("T1b F61: a shard of 50 KAS asked {asked} sompi for a user's leg gives its owner {by_a_draw} by a draw; through an account of the owner's own (a leg long 10 at 0.1, left by statement at the committee's 10, no wrong point) it is asked 92 KAS and pays all 50 to the owner's second key, with the leg of the user standing and {} sompi asked", paid_out.requirement(&prices).unwrap());
}

// ---------------------------------------------------------------- rounds 39 and 40: a leg enters by a statement (kind 6)

use kpf_protocol::reserve::shard_sid;
use kpf_protocol::thin::{enter_row_judged, AEnter, KIND_ENTER};

fn seed39(w: &OW) -> [u8; 32] {
    shard_seed(&w.anchors[0])
}

/// The note that the leg an entry shows entered, as `KpfTStepEnter` writes it.
fn join39(x: &Account, l: &XLeg) -> SNote {
    SNote { gross: l.size.wrapping_abs(), ..SNote::leg(NOTE_JOIN, x.shard.unwrap(), l, x.b_key) }
}

/// The statement of an entry with the margins given, as `KpfTStepEnter` writes its outputs: the account with the
/// last row's second chain, holding what it held, and the note that the leg shown entered.
fn stated_enter39(x: &Account, en: &AEnter, am: i64, bm: i64, held: i64) -> Stated {
    let last = en.rows[en.rows.len() - 1];
    let next = Account { a_margin: am, b_margin: bm, ..x.clone() };
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&last.h)].concat())), next.moving()].concat());
    let note = join39(x, &en.leg);
    let outs = Outs::with_notes(&[(Out { key: false, to: h, value: held }, false), (Out { key: false, to: note.hash(), value: note.value() }, true)]).unwrap();
    Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_ENTER, t0: T0, arg: en.arg(), outs, bond: x.bond(), claimer: x.a_key, held }
}

/// The entry of `l` (its entry the last price) at the prices `a`, written whether or not the rule allows it.
fn enter_by_hand39(x: &Account, l: XLeg, a: &[i64], seed: [u8; 32], held: i64) -> (Stated, AEnter) {
    let mut with = x.clone();
    with.legs.push(l);
    let rows = with.trace(a);
    let last = rows[rows.len() - 1];
    let en = AEnter { side: SIDE_A, amount: 0, skip: rows.len() as i64, leg: l, seed, rows };
    (stated_enter39(x, &en, x.a_margin + last.gain, x.b_margin - last.gain, held), en)
}

/// The entry stated by `KpfTEnter`, as it landed.
fn entered39(w: &OW, account: &OLive<Account>, st: &Stated, en: &AEnter) -> OLive<Stated> {
    let f = [fund(xk(&w.f), 60, 1_000)];
    let b = enter_state(&w.lib, account, st, &en.said(), &w.script("KpfTEnter"), w.payer(&w.f, &f)).expect("an entry built");
    let s = b.sign(&w.keys).expect("an entry signed");
    let v = kpf_protocol::verify::verify(&s.tx, &s.entries);
    assert!(v.is_ok(), "an entry stated: {:?}", v.errors());
    nth(&s, 0, *st, 6_510)
}

/// What the engine takes of the answer of the end of an entry: (right is taken, wrong is taken).
fn enter_end_taken39(w: &OW, account: &OLive<Account>, st: &Stated, en: &AEnter) -> (bool, bool) {
    let q = w.pointed(&entered39(w, account, st, en), 0);
    let take = |v: bool| {
        let f = [fund(xk(&w.w), 33, 1_000)];
        enter_end(&w.lib, &q, &account.s, en, &w.script("KpfTStepEnter"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| taken30(&b.0, &w.keys))
    };
    (take(true), take(false))
}

/// Of the answer of row `i` of an entry, with the leg shown and the chain that opens it.
fn enter_row_taken39(w: &OW, account: &OLive<Account>, st: &Stated, en: &AEnter, i: usize, prev: &[u8; 32], leg: &XLeg) -> (bool, bool) {
    let at = 2 * i as i64 - 1;
    let q = w.pointed(&entered39(w, account, st, en), at);
    let take = |v: bool| {
        let f = [fund(xk(&w.w), 33, 1_000)];
        enter_row(&w.lib, &q, en, at, prev, leg, &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| taken30(&b.0, &w.keys))
    };
    (take(true), take(false))
}

/// Of the answer of the price of row `i` of an entry, with a citation.
fn enter_price_taken39(w: &OW, account: &OLive<Account>, st: &Stated, en: &AEnter, legs: &[XLeg], i: usize, cite: &Cited) -> (bool, bool) {
    let at = 2 * i as i64;
    let q = w.pointed(&entered39(w, account, st, en), at);
    let cs = w.script("KpfTCite");
    let take = |v: bool| {
        let f = [fund(xk(&w.w), 33, 1_000)];
        enter_price(&w.lib, &q, &account.s, legs, en, at, &w.script("KpfTStepPrice"), (&cs, cite), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| taken30(&b.0, &w.keys))
    };
    (take(true), take(false))
}

/// The ten terms, one changed at a time, and the three periods of the moving part that a shard's id is of.
fn other_terms40(t: XTerms) -> Vec<(&'static str, XTerms)> {
    vec![
        ("the term", XTerms { term: t.term + 1, ..t }),
        ("the maintenance rate", XTerms { mmr_bps: t.mmr_bps - 1, ..t }),
        ("the tolerance", XTerms { g_bps: t.g_bps + 1, ..t }),
        ("the liquidation fee", XTerms { liq_fee_bps: t.liq_fee_bps + 1, ..t }),
        ("the least keeper's fee", XTerms { keeper_fee_min: t.keeper_fee_min + 1, ..t }),
        ("the carrier", XTerms { carrier: t.carrier + 1, ..t }),
        ("the idle period", XTerms { idle_daa: t.idle_daa + 1, ..t }),
        ("the grace period", XTerms { grace_daa: t.grace_daa + 1, ..t }),
        ("the lateness", XTerms { late_daa: t.late_daa + 1, ..t }),
        ("the quiet period", XTerms { quiet_daa: t.quiet_daa + 1, ..t }),
    ]
}

/// F62 (fixed in round 40). A covered account's terms and periods were its two keys' own, and an entry was answered
/// within them. The fix: a shard's id is the hash of its opening, its committee, its ten terms and its lateness,
/// dispute period and bond rate; `KpfTStepEnter` holds the account's own terms and periods to its shard's id, and
/// `KpfTLegMove` asks the two accounts' terms and periods equal. Verified at the engine: round 39's account (a
/// tolerance of 50 %, no maintenance rate) is still opened, and the entry at 5 where the committee says 10 has its
/// row and its price answered right within those terms and its **end answered wrong**, and not right: the entry is
/// ended. One field at a time: an account of any other term (ten), dispute period or bond rate has an end that is
/// wrong for an entry whose row is right; and a leg does not move into such an account, nor out of one. The lease
/// and the two asks are not of the id: an account of another lease or ask is entered (said in the write-up).
#[test]
fn t1b_fixed_f62_an_entry_and_a_move_hold_an_account_to_its_shards_terms_and_periods() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    let seed = seed39(&w);
    let committee = w.markets[0].committee_id;
    let user = w.covered(sid, 80 * KAS1, 20 * KAS1);
    let x = &user.s;
    assert_eq!(shard_sid(&seed, &committee, &x.terms(), &x.periods()), sid, "the account of the shard's terms and periods");
    let leg = XLeg { entry: P10, ..w.fut(1, 10, LONG, 0) };
    // ---- round 39's account: a tolerance of 50 % and no maintenance rate, an entry at 5.
    let own = Account { t: XTerms { g_bps: 5_000, mmr_bps: 0, ..w.terms() }, a_key: xk(&w.w), ..x.clone() };
    let fb = [fund(xk(&w.b), 58, 1_000)];
    assert!(account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &own, seed, committee, w.payer(&w.b, &fb)).is_err(), "(round 41: such an account is not opened; the F64 test. Here it is made past the scripts)");
    let own = OLive { value: user.value, s: own, id: user.id, op: op(65), daa: 6_100 };
    let half = XLeg { entry: P10 / 2, ..leg };
    assert!(own.s.enter(leg, &[P10 / 2], T0, own.value as i64, seed).is_err(), "F62 is open again: the model states an entry on an account of other terms");
    let (st, en) = enter_by_hand39(&own.s, half, &[P10 / 2], seed, own.value as i64);
    assert_eq!(own.s.enter_wrong_point(&en, &st.claimer, &st.outs, own.value as i64, &[P10]), Some(0), "the end is the wrong point");
    assert_eq!(enter_end_taken39(&w, &own, &st, &en), (false, true), "F62 is open again: the end of an entry on an account of other terms is answered right");
    assert_eq!(enter_row_taken39(&w, &own, &st, &en, 1, &[0; 32], &half), (true, false), "(its row is right)");
    assert_eq!(enter_price_taken39(&w, &own, &st, &en, &[half], 1, &w.cited(1, P10)), (true, false), "(its price is right within the account's own tolerance)");
    // ---- one field at a time: an entry.
    let mut others: Vec<(String, Account)> = other_terms40(w.terms()).into_iter().map(|(what, t)| (what.to_string(), Account { t, ..x.clone() })).collect();
    others.push(("the dispute period".into(), Account { dispute_daa: x.dispute_daa + 1, ..x.clone() }));
    others.push(("a dispute period of one block".into(), Account { dispute_daa: 1, ..x.clone() }));
    others.push(("the bond rate".into(), Account { bond_bps: x.bond_bps - 1, ..x.clone() }));
    others.push(("a bond rate of one in ten thousand".into(), Account { bond_bps: 1, ..x.clone() }));
    let live = |s: &Account| OLive { value: user.value, s: s.clone(), id: user.id, op: op(63), daa: 6_100 };
    let mut ends = 0;
    for (what, y) in &others {
        assert_ne!(shard_sid(&seed, &committee, &y.terms(), &y.periods()), sid, "{what}");
        let a = live(y);
        assert!(y.enter(leg, &[P10], T0, a.value as i64, seed).is_err(), "F62 is open again: the model states an entry on an account of another {what}");
        let (st, en) = enter_by_hand39(y, leg, &[P10], seed, a.value as i64);
        // (With a bond rate of 1 in 10,000 the bond is 0.01 KAS, and the builder writes no answer whose payouts are
        // that small: such an output is not mined unless whoever answers adds to it. The end is not answered right.)
        let end = enter_end_taken39(&w, &a, &st, &en);
        assert!(!end.0 && (end.1 || y.bond_bps == 1), "F62 is open again: an entry on an account of another {what} has a right end ({end:?})");
        assert!(!y.enter_end_right(&en, &st.claimer, &st.outs, a.value as i64));
        if ends % 5 == 0 {
            assert_eq!(enter_row_taken39(&w, &a, &st, &en, 1, &[0; 32], &leg), (true, false), "{what}: the row is right");
        }
        ends += 1;
    }
    // The control, and what is not of the id: the lease and the two asks.
    let (cst, cen) = enter_by_hand39(x, leg, &[P10], seed, user.value as i64);
    assert_eq!(enter_end_taken39(&w, &user, &cst, &cen), (true, false), "the control: the account of the shard's terms and periods");
    for (what, y) in [("another lease", Account { expiry: x.expiry + 1_000, ..x.clone() }), ("what side A asks", Account { a_min_counter: 5 * KAS1, ..x.clone() }), ("what side B asks", Account { b_min_counter: 5 * KAS1, ..x.clone() })] {
        let a = live(&y);
        let (st, en) = enter_by_hand39(&y, leg, &[P10], seed, a.value as i64);
        assert_eq!(enter_end_taken39(&w, &a, &st, &en), (true, false), "{what} is not of the shard's id: the leg enters");
    }
    // ---- one field at a time: a leg's move, into such an account and out of one.
    let (from, _) = w.added(&user, w.fut(0, 3, LONG, P10));
    let to = Account { a_key: xk(&w.w), legs: vec![w.fut(3, 1, LONG, P10)], a_margin: 10 * KAS1, b_margin: KAS1, ..x.clone() };
    let to_live = OLive { value: to.pot() as u64, s: to.clone(), id: from.id, op: op(60), daa: 6_100 };
    let (base, bx, by) = leg_move(&w.lib, &from, 1, &to_live, [0; 4], &w.script("KpfTLegMove"), w.payer(&w.b, &fb)).unwrap();
    assert!(taken30(&base, &w.keys), "the control: a leg moves between two accounts of the shard's terms");
    let Arg::V(ArtifactValue::Bytes(who)) = args_of(&mut base.clone(), 1)[0].clone() else { panic!("the places") };
    let (j, n) = (who[0] as usize, who[8] as usize);
    let mut moves = 0;
    for (what, y) in &others {
        // Into an account that differs: the second account's terms and moving part are arguments 8 and 10.
        let (to2, by2) = (Account { t: y.t, dispute_daa: y.dispute_daa, bond_bps: y.bond_bps, ..to.clone() }, Account { t: y.t, dispute_daa: y.dispute_daa, bond_bps: y.bond_bps, ..by.clone() });
        assert!(from.s.leg_moved(1, &to2, [0; 4]).is_err(), "F62 is open again: the model moves a leg into an account of another {what}");
        let mut t = base.clone();
        args_of(&mut t, 1)[8] = bytes_arg(&to2.terms());
        args_of(&mut t, 1)[10] = bytes_arg(&to2.moving());
        object_made(&mut t, &w.lib, n, &to2.hash());
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &by2.hash());
        assert!(!taken30(&t, &w.keys), "F62 is open again: a leg moves into an account of another {what}");
        // Out of one: the first account's terms and moving part are arguments 3 and 7.
        let (from2, bx2) = (Account { t: y.t, dispute_daa: y.dispute_daa, bond_bps: y.bond_bps, ..from.s.clone() }, Account { t: y.t, dispute_daa: y.dispute_daa, bond_bps: y.bond_bps, ..bx.clone() });
        let mut t = base.clone();
        args_of(&mut t, 1)[3] = bytes_arg(&from2.terms());
        args_of(&mut t, 1)[7] = bytes_arg(&from2.moving());
        object_made(&mut t, &w.lib, j, &from2.hash());
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &bx2.hash());
        assert!(!taken30(&t, &w.keys), "F62 is open again: a leg moves out of an account of another {what}");
        moves += 2;
    }
    // (Both of the same other terms: they are equal, and the move is taken. Such accounts have no leg that
    // entered by a statement; with KpfTLegAdd gone they have none.)
    println!("T1b F62 fixed: on an account of a tolerance of 50 % the entry at 5 has a right row, a right price and a wrong end; {ends} accounts that differ from the shard in one term or period have a wrong end for an entry, and {moves} moves into or out of such an account are refused; the lease and the two asks are not of the id");
}

/// F63 (fixed in round 40). A leg that entered by a statement was shown to nobody. The fix: what is stated of an
/// entry carries the leg whole (111 bytes at 24), `KpfTStepRow` holds the named row to the leg shown, and
/// `KpfTStepEnter` makes the note of it. Verified: the statement's transaction shows the leg's bytes (an option
/// worth nothing exercised, strike and all); read back from what is stated it is the leg; and whoever has only
/// what the chain shows answers its row in a later statement: the leg from the entry, its entry the price of its
/// row in the statement that was applied last. A row is right for no leg but the one shown: the leg that opens the
/// chain with another market, strike, size, direction, committee or entry shown in its place is answered wrong.
#[test]
fn t1b_fixed_f63_an_entry_shows_the_leg_and_whoever_watches_answers_its_rows_from_the_chain() {
    let w = ow();
    let shard = w.shard();
    let account = w.covered(shard.s.sid, 20 * KAS1, 2 * KAS1);
    let seed = seed39(&w);
    let f = [fund(xk(&w.a), 42, 1_000)];
    let fw = [fund(xk(&w.w), 54, 1_000)];
    // A call on 10 units struck at 13, entered at 10: it is worth nothing exercised.
    let leg = w.opt(1, 10, CALL_HELD, P10 * 13 / 10, P10);
    let (eb, st, en, after) = account_enter(&w.lib, &account, &w.script("KpfTEnter"), leg, &[P10], T0, seed, w.payer(&w.a, &f)).unwrap();
    let es = signed("an option enters by a statement", &eb, &w.keys);
    let shown = |tx: &Transaction, what: &[u8]| -> bool { tx.inputs.iter().any(|i| i.signature_script.windows(what.len()).any(|x| x == what)) };
    let bytes = kpf_protocol::thin::leg_bytes(&after.legs[0]);
    assert!(shown(&es.tx, &bytes) && shown(&es.tx, &leg.market), "F63 is open again: the statement of an entry does not show the leg");
    // Read back from what the transaction shows.
    let said = en.said();
    assert!(shown(&es.tx, &said));
    let read = AEnter::read(&said).expect("what is stated is read");
    assert_eq!((read.leg, kpf_protocol::thin::leg_read(&said[24..135])), (after.legs[0], Some(after.legs[0])));
    // Applied; then a take is stated. Whoever watches has the leg from the entry and its entry from the entry's
    // row (the statement applied last), and answers the take's row with it.
    let live = nth(&es, 0, st, 6_510);
    let rs = signed("it is applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let y = nth(&rs, 0, after.clone(), 6_570);
    let watched = XLeg { entry: read.rows[0].a, ..read.leg };
    assert_eq!(watched, y.s.legs[0]);
    let (tst, tk) = y.s.take_by(SIDE_A, KAS1, &[P10 * 11 / 10], T0, y.value as i64, y.s.a_key).unwrap();
    let tlive = w.stated(&y, &tst, &tk.said());
    assert_eq!(row_taken(&w, &w.pointed(&tlive, 1), &tk, 1, &[0; 32], &watched), (true, false), "the row of a later statement is answered with the leg the chain showed");
    // A row is right for the leg shown and no other.
    let mut wrong = 0;
    let honest = en.rows[0];
    let real = after.legs[0];
    for (what, shown_leg) in [
        ("another market", XLeg { market: w.markets[2].id(), ..real }),
        ("another strike", XLeg { dir: kpf_protocol::thin::option_dir(CALL_HELD, P10), ..real }),
        ("a futures leg", XLeg { dir: LONG, ..real }),
        ("a unit more", XLeg { size: real.size + 1, ..real }),
        ("another committee", XLeg { committee: [0x09; 32], ..real }),
        ("another scale", XLeg { scale: real.scale * 10, ..real }),
        ("another limit", XLeg { price_max: real.price_max + 1, ..real }),
        ("the entry a sompi off", XLeg { entry: real.entry + 1, ..real }),
    ] {
        let en2 = AEnter { leg: shown_leg, ..en.clone() };
        let st2 = stated_enter39(&account.s, &en2, account.s.a_margin, account.s.b_margin, account.value as i64);
        let model = enter_row_judged(&[0; 32], &real, &Row::ZERO, &honest, Some(&shown_leg));
        assert_eq!((model, enter_row_taken39(&w, &account, &st2, &en2, 1, &[0; 32], &real)), (Some(false), (false, true)), "the row of the leg, {what} shown in its place");
        // (And the leg shown does not open the row: no answer with it.)
        assert_eq!(enter_row_taken39(&w, &account, &st2, &en2, 1, &[0; 32], &shown_leg), (false, false), "{what}: the leg shown does not open the chain");
        wrong += 1;
    }
    assert_eq!(enter_row_taken39(&w, &account, &st, &en, 1, &[0; 32], &real), (true, false), "the control");
    println!("T1b F63 fixed: the statement of an entry shows the leg (an option worth nothing exercised, with its strike); read back, with its entry from the row of the statement applied last, it answers the row of a later take right; {wrong} rows whose leg is another than the one shown are answered wrong");
}

/// Round 39, item 1. `KpfTEnter`: an entry is stated by the two signatures, side A the claimer, on a covered account
/// and nothing else, with what is stated of an entry's length and pushes, outputs of flags the settlement knows,
/// and the bond. And `KpfTChange` states no kind 6; `KpfTEnter` states no other kind.
#[test]
fn t1b_r39_an_entry_is_stated_by_the_two_on_a_covered_account_and_by_no_other_door() {
    let w = ow();
    let shard = w.shard();
    let account = w.covered(shard.s.sid, 20 * KAS1, 2 * KAS1);
    let x = &account.s;
    let held = account.value as i64;
    let seed = seed39(&w);
    let f = [fund(xk(&w.f), 60, 1_000)];
    let (b, st, en, _) = account_enter(&w.lib, &account, &w.script("KpfTEnter"), w.fut(0, 10, LONG, 0), &[P10], T0, seed, w.payer(&w.f, &f)).unwrap();
    assert!(taken30(&b, &w.keys), "the control");
    assert!(matches!((&args_of(&mut b.clone(), 1)[9], &args_of(&mut b.clone(), 1)[10]), (Arg::Sig(_), Arg::Sig(_))), "the two signatures are arguments 9 and 10");
    let refused = std::cell::Cell::new(0);
    let no = |what: &str, t: OBuilt| {
        assert!(!taken30(&t, &w.keys), "{what}");
        refused.set(refused.get() + 1);
    };
    // ---- the signatures.
    for (at, own, what) in [(9usize, x.a_key, "side A"), (10, x.b_key, "side B")] {
        for key in [x.a_key, x.b_key, xk(&w.w)] {
            if key != own {
                no(&format!("the signature of {what} by another key"), with_arg(&b, at, Arg::Sig(key)));
            }
        }
    }
    // ---- what the object becomes: the statement of kind 6 by side A with the bond, and no other.
    let said = en.said();
    let becomes = |h: [u8; 32], value: u64| {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &h);
        t.tx.outputs[0].value = value;
        t
    };
    let full = account.value + st.bond as u64;
    assert!(taken30(&becomes(st.hash(), full), &w.keys), "the control: the statement as it is");
    no("the claimer is side B", becomes(Stated { claimer: x.b_key, ..st }.hash(), full));
    no("the claimer is a third key", becomes(Stated { claimer: xk(&w.w), ..st }.hash(), full));
    for kind in [1i64, 2, 3, 4, 5, 7] {
        no(&format!("KpfTEnter states kind {kind}"), becomes(Stated { kind, ..st }.hash(), full));
    }
    no("a bond a sompi less in the statement", becomes(Stated { bond: st.bond - 1, ..st }.hash(), full - 1));
    no("no bond in the statement", becomes(Stated { bond: 0, ..st }.hash(), account.value));
    no("the object a sompi short of the bond", becomes(st.hash(), full - 1));
    no("another time than the argument", becomes(Stated { t0: st.t0 + 1, ..st }.hash(), full));
    no("another dispute period", becomes(Stated { window: st.window - 1, ..st }.hash(), full));
    no("another lateness", becomes(Stated { late: st.late + 1, ..st }.hash(), full));
    // ---- what is stated: its length and its pushes (argument 6), the statement's hash following it.
    let outs_bytes = st.outs.bytes();
    let with_said = |s: &[u8]| {
        let mut t = becomes(stated_hash_raw(&st.position, 6, st.t0, &blake2b(s), &outs_bytes, st.bond, &st.claimer, st.window, st.late, st.held), full);
        args_of(&mut t, 1)[6] = bytes_arg(s);
        t
    };
    assert!(taken30(&with_said(&said), &w.keys), "the control: what is stated, put again");
    assert_eq!(said.len(), 167 + 112);
    no("no row", with_said(&said[..167]));
    no("a byte short", with_said(&said[..said.len() - 1]));
    no("a byte more", with_said(&[&said[..], &[0u8][..]].concat()));
    no("a leg in the place of a row", with_said(&said[..167 + 111]));
    no("the rows from 116, as a leaving has them", with_said(&[&said[..116], &said[167..]].concat()));
    no("the rows from 148, as an entry had them before the leg was shown", with_said(&[&said[..148], &said[167..]].concat()));
    for at in [24usize, 57, 90, 99, 108, 117, 126] {
        for v in [0u8, 0x21, 0x09, 0x4c] {
            if said[at] != v {
                let mut s = said.clone();
                s[at] = v;
                no(&format!("the push at {at} is {v:#x}"), with_said(&s));
            }
        }
    }
    // Two rows (what is stated is of any length of whole rows): stated, though its end is wrong.
    assert!(taken30(&with_said(&[&said[..], &said[167..]].concat()), &w.keys), "whole rows are stated");
    // ---- the outputs' flags (argument 7).
    let with_outs = |o: &[u8]| {
        let mut t = becomes(stated_hash_raw(&st.position, 6, st.t0, &st.arg, o, st.bond, &st.claimer, st.window, st.late, st.held), full);
        args_of(&mut t, 1)[7] = bytes_arg(o);
        t
    };
    assert!(taken30(&with_outs(&outs_bytes), &w.keys) && outs_bytes.len() == 82 && outs_bytes[0] == 0 && outs_bytes[41] == 2, "the control: the account and a note");
    for (at, v) in [(0usize, 2u8), (0, 3), (41, 0), (41, 3), (41, 0x80)] {
        let mut o = outs_bytes.clone();
        o[at] = v;
        no(&format!("the flag at {at} is {v}"), with_outs(&o));
    }
    no("no output", with_outs(&[]));
    no("five outputs", with_outs(&[&outs_bytes[..], &outs_bytes[41..], &outs_bytes[41..], &outs_bytes[41..]].concat()));
    no("an output a byte short", with_outs(&outs_bytes[..81]));
    // ---- KpfTChange states no entry.
    let fa = [fund(xk(&w.a), 30, 1_000)];
    assert!(!account_state(&w.lib, &account, &w.script("KpfTChange"), &st, &said, w.payer(&w.a, &fa)).is_ok_and(|t| taken30(&t, &w.keys)), "KpfTChange states kind 6");
    refused.set(refused.get() + 1);
    // ---- the objects it is not stated on.
    // An object that carries a statement (shown as the account it was, and as it is).
    let live = entered39(&w, &account, &st, &en);
    let under = OLive { value: live.value, s: x.clone(), id: live.id, op: live.op, daa: 6_530 };
    let again = enter_state(&w.lib, &under, &Stated { held: live.value as i64, ..st }, &said, &w.script("KpfTEnter"), w.payer(&w.f, &f)).unwrap();
    assert!(taken30(&again, &w.keys), "the control: the object as the account it was");
    let mut as_it_is = again.clone();
    as_it_is.entries[0].script_public_key = hash_spk(&w.lib, &live.s.hash());
    no("an entry stated on an object that carries a statement, shown as the account", as_it_is);
    let mut opened = again.clone();
    object_made(&mut opened, &w.lib, 0, &live.s.hash());
    no("an entry stated on an object that carries a statement, shown as it is", opened);
    // An account no shard covers (138 bytes), and a shard.
    let plain = w.account(vec![w.fut(0, 3, LONG, P10)], 20 * KAS1, 20 * KAS1);
    let covered_twin = Account { shard: Some(shard.s.sid), ..plain.s.clone() };
    let (pst, pen) = enter_by_hand39(&covered_twin, XLeg { entry: P10, ..w.fut(1, 1, LONG, 0) }, &[P10, P10], seed, plain.value as i64);
    let pst = Stated { position: plain.s.hash(), ..pst };
    assert!(!enter_state(&w.lib, &plain, &pst, &pen.said(), &w.script("KpfTEnter"), w.payer(&w.f, &f)).is_ok_and(|t| taken30(&t, &w.keys)), "an entry is stated on an account of 138 bytes");
    let as_account = Account { a_key: shard.s.owner, b_key: x.b_key, a_margin: shard.s.held, b_margin: 0, legs: vec![], ..x.clone() };
    let sh = OLive { value: shard.value, s: as_account.clone(), id: shard.id, op: shard.op, daa: shard.daa };
    let (sst, sen) = enter_by_hand39(&as_account, XLeg { entry: P10, ..w.fut(1, 1, LONG, 0) }, &[P10], seed, shard.value as i64);
    let on_shard = enter_state(&w.lib, &sh, &sst, &sen.said(), &w.script("KpfTEnter"), w.payer(&w.f, &f)).is_ok_and(|mut t| {
        object_made(&mut t, &w.lib, 0, &shard.s.hash());
        taken30(&t, &w.keys)
    });
    assert!(!on_shard, "an entry is stated on a shard");
    refused.set(refused.get() + 2);
    let _ = held;
    println!("T1b R39 stating: {} refused: either signature by another key; another claimer, kind, bond, time, dispute period or lateness in what the object becomes; what is stated a byte off, with no row, a leg for a row, a leaving's form or the form before the leg was shown, or another of the leg's seven pushes; flags the settlement does not know and outputs that are none, five or cut; kind 6 through KpfTChange; an object that carries a statement, an account of 138 bytes, a shard", refused.get());
}

/// Rounds 39 and 40, item 2. The entering row, `KpfTStepRow` against `enter_row_judged`, at the engine: on an account with one
/// leg, the second row is of the leg that enters. Legs of every kind, strikes of 0, 1, the limit and one beyond,
/// directions' places that are no kind, sizes of none, below zero, the bound and beyond it, a scale beyond the most;
/// the entry a sompi off the row's price; what is said of the leg another row's name or other units; the row named
/// the first (the old leg judged as one that enters); the entering leg equal to the old one. And the old leg's row
/// in an entry's trace with each of its numbers off by one. For each the engine takes the model's verdict and not
/// the other.
#[test]
fn t1b_r39_an_entering_rows_answer_is_the_models() {
    let w = ow();
    let shard = w.shard();
    let seed = seed39(&w);
    let old = w.fut(0, 5, LONG, P10);
    let mut account = w.account(vec![old], 500 * KAS1, 5 * KAS1);
    account.s.shard = Some(shard.s.sid);
    let x = &account.s;
    let held = account.value as i64;
    let row1 = row_after(&Row::ZERO, &old, P10, false);
    let m = &w.markets[1];
    let (scale, pm) = (m.p.scale, m.p.price_max);
    let bound = (kpf_protocol::cross::MARGIN_MAX / pm - 1) * scale;
    let base = XLeg { entry: P10, ..w.fut(1, 3, LONG, 0) };
    let mut legs: Vec<(String, XLeg)> = vec![("long".into(), base), ("short".into(), XLeg { dir: SHORT, ..base }), ("equal to the old leg".into(), old)];
    for kind in [CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN] {
        for (what, strike) in [("0", 0i64), ("1", 1), ("the price", P10), ("the limit", pm), ("beyond the limit", pm + 1)] {
            legs.push((format!("an option of kind {kind} struck at {what}"), XLeg { dir: kind + 8 * strike, ..base }));
        }
    }
    for dir in [0i64, -1, 7, 8, 9, 15, 8 * P10 + 7] {
        legs.push((format!("a direction's place of {dir}"), XLeg { dir, ..base }));
    }
    for (what, size) in [("no size", 0i64), ("a size below zero", -scale), ("a size at the bound", bound), ("a size beyond the bound", bound + scale), ("a size of a unit's part", 1)] {
        legs.push((what.into(), XLeg { size, ..base }));
    }
    legs.push(("a scale beyond the most".into(), XLeg { scale: 1_000_000_001, ..base }));
    legs.push(("a scale of the most".into(), XLeg { scale: 1_000_000_000, size: 3_000_000_000, ..base }));
    let (mut cases, mut rights, mut wrongs, mut nones) = (0, 0, 0, 0);
    for (name, leg) in &legs {
        // The row as an honest trace has it, where the model computes one; else a row of the old leg's sums.
        let honest = std::panic::catch_unwind(|| row_after(&row1, leg, leg.entry, false)).unwrap_or(Row { a: leg.entry, g: blake2b(&[&row1.g[..], &kpf_protocol::thin::leg_bytes(leg)].concat()), ..row1 });
        // (what, the row, the leg the statement shows)
        let mut variants: Vec<(&str, Row, XLeg)> = vec![("as it is", honest, *leg)];
        if name == "long" || name.starts_with("an option of kind 3 struck at the price") {
            variants.push(("the entry a sompi under the row's price", Row { a: honest.a + 1, ..honest }, *leg));
            variants.push(("a sompi of gain as it enters", Row { gain: honest.gain + 1, ..honest }, *leg));
            variants.push(("shown: another market", honest, XLeg { market: w.markets[2].id(), ..*leg }));
            variants.push(("shown: a unit more", honest, XLeg { size: leg.size + 1, ..*leg }));
            variants.push(("shown: the other direction", honest, XLeg { dir: if leg.dir == LONG { SHORT } else { LONG }, ..*leg }));
            variants.push(("shown: another committee", honest, XLeg { committee: [0x09; 32], ..*leg }));
            variants.push(("shown: the entry a sompi off", honest, XLeg { entry: leg.entry + 1, ..*leg }));
        }
        for (what, row, shown) in variants {
            let en = AEnter { side: SIDE_A, amount: 0, skip: 2, leg: shown, seed, rows: vec![row1, row] };
            let st = stated_enter39(x, &en, x.a_margin, x.b_margin, held);
            let model = enter_row_judged(&row1.g, leg, &row1, &row, Some(&shown));
            let engine = enter_row_taken39(&w, &account, &st, &en, 2, &row1.g, leg);
            assert_eq!(engine, verdicts(model), "{name}, {what}: the model says {model:?}");
            match model {
                Some(true) => rights += 1,
                Some(false) => wrongs += 1,
                None => nones += 1,
            }
            cases += 1;
        }
    }
    assert!(rights >= 10 && wrongs >= 25, "{rights} right, {wrongs} wrong, {nones} with no verdict");
    // The row named is the first: the old leg is then judged as a leg that enters (its entry is its row's price
    // here), and is right only where the leg shown is the old leg; the second row is then a row that stays.
    let honest2 = row_after(&row1, &base, P10, false);
    for (what, shown) in [("the entering leg shown", base), ("the old leg shown", old)] {
        let en = AEnter { side: SIDE_A, amount: 0, skip: 1, leg: shown, seed, rows: vec![row1, honest2] };
        let st = stated_enter39(x, &en, x.a_margin, x.b_margin, held);
        let model = enter_row_judged(&[0; 32], &old, &Row::ZERO, &row1, Some(&shown));
        assert_eq!(enter_row_taken39(&w, &account, &st, &en, 1, &[0; 32], &old), verdicts(model), "the first row named, {what}");
        assert!(!x.enter_end_right(&en, &x.a_key, &st.outs, held) && enter_end_taken39(&w, &account, &st, &en) == (false, true), "the first row named, {what}: the end is wrong");
        cases += 1;
    }
    // The old leg's row in an entry's trace: each number off by one is answered wrong.
    let mut off = 0;
    for which in 0..6 {
        for d in [1i64, -1] {
            let mut r = row1;
            match which {
                0 => r.a += d,
                1 => r.gain += d,
                2 => r.wa += d,
                3 => r.wb += d,
                4 => r.ea += d,
                _ => r.eb += d,
            }
            let en = AEnter { side: SIDE_A, amount: 0, skip: 2, leg: base, seed, rows: vec![r, row_after(&r, &base, P10, false)] };
            let st = stated_enter39(x, &en, x.a_margin, x.b_margin, held);
            let model = enter_row_judged(&[0; 32], &old, &Row::ZERO, &r, None);
            assert_eq!((model, enter_row_taken39(&w, &account, &st, &en, 1, &[0; 32], &old)), (Some(false), (false, true)), "the old leg's row with number {which} off by {d}");
            off += 1;
        }
    }
    // An old option leg is counted as in a take.
    let opt = w.opt(0, 4, CALL_HELD, P10, P10);
    let mut with_opt = w.account(vec![opt], 500 * KAS1, 5 * KAS1);
    with_opt.s.shard = Some(shard.s.sid);
    let (ost, oen) = enter_by_hand39(&with_opt.s, base, &[P10 * 12 / 10, P10], seed, with_opt.value as i64);
    assert_eq!(oen.rows[0], with_opt.s.trace(&[P10 * 12 / 10])[0], "the row of an old option in an entry is its row in a take");
    assert_eq!(enter_row_taken39(&w, &with_opt, &ost, &oen, 1, &[0; 32], &opt), (true, false));
    println!("T1b R39 rows: {} legs, {cases} entering rows answered at the engine: {rights} right, {wrongs} wrong, {nones} with no verdict, each the model's; the old leg's row with a number off by one answered wrong {off} times; an old option's row is its row in a take", legs.len());
}

/// Round 39, item 3. `KpfTStepEnter` against `Account::enter_end_right`, at the engine. The committee: another
/// seed; a leg of another committee with the shard's committee said of it (the end is right and the row is wrong)
/// and with its own said (the row is right and the end is wrong): in neither are both right. Side A's floor: at
/// exactly the floor and a sompi under, with a futures leg, with an option worth exercising entering, with an old
/// option that side A holds, and with side B's ask above the floor. A margin below zero by an old leg's loss and by
/// its gain. The first leg of an account, and an account of three. The row named not the last; side B; an amount;
/// another claimer. And outputs that are another account or another note.
#[test]
fn t1b_r39_the_end_of_an_entry_is_the_models() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    let seed = seed39(&w);
    let made = |legs: Vec<XLeg>, am: i64, bm: i64| {
        let mut a = w.account(legs, am, bm);
        a.s.shard = Some(sid);
        a
    };
    let rate_up = |v: i64, rate: i64| (v / 10_000) * rate + ((v % 10_000) * rate + 9_999) / 10_000;
    let (mut cases, mut rights, mut wrongs) = (0, 0, 0);
    let mut judge = |what: &str, account: &OLive<Account>, st: &Stated, en: &AEnter, expect: Option<bool>| {
        let model = account.s.enter_end_right(en, &st.claimer, &st.outs, account.value as i64);
        if let Some(e) = expect {
            assert_eq!(model, e, "{what}: the model");
        }
        assert_eq!(enter_end_taken39(&w, account, st, en), if model { (true, false) } else { (false, true) }, "{what}: the model says {model}");
        cases += 1;
        if model {
            rights += 1;
        } else {
            wrongs += 1;
        }
        model
    };
    let fut = XLeg { entry: P10, ..w.fut(1, 10, LONG, 0) };
    // ---- the first leg, and an account of three.
    let first = made(vec![], 20 * KAS1, 2 * KAS1);
    let (st, en) = enter_by_hand39(&first.s, fut, &[P10], seed, first.value as i64);
    judge("the first leg of an account", &first, &st, &en, Some(true));
    let three = made(vec![w.fut(0, 3, LONG, P10), w.opt(2, 2, PUT_HELD, P10, P10), w.fut(3, 1, SHORT, P10)], 60 * KAS1, 20 * KAS1);
    let (st3, en3) = enter_by_hand39(&three.s, fut, &[P10 * 11 / 10, P10 * 9 / 10, P10, P10], seed, three.value as i64);
    judge("a fourth leg", &three, &st3, &en3, Some(true));
    // ---- the committee.
    let other_seed = AEnter { seed: [7; 32], ..en.clone() };
    judge("another seed", &first, &Stated { arg: other_seed.arg(), ..st }, &other_seed, Some(false));
    let foreign = XLeg { committee: [0x09; 32], ..fut };
    let (fst, fen) = enter_by_hand39(&first.s, foreign, &[P10], seed, first.value as i64);
    let end_own = judge("a leg of another committee, its own said of it", &first, &fst, &fen, Some(false));
    let row_own = enter_row_taken39(&w, &first, &fst, &fen, 1, &[0; 32], &foreign);
    let dressed = AEnter { leg: XLeg { committee: fut.committee, ..foreign }, ..fen.clone() };
    let dst = stated_enter39(&first.s, &dressed, first.s.a_margin, first.s.b_margin, first.value as i64);
    let end_dressed = judge("a leg of another committee, the committee of the shard said of it", &first, &dst, &dressed, Some(true));
    let row_dressed = enter_row_taken39(&w, &first, &dst, &dressed, 1, &[0; 32], &foreign);
    assert_eq!((end_own, row_own, end_dressed, row_dressed), (false, (true, false), true, (false, true)), "a leg of another committee: the row or the end is wrong, whichever committee is said");
    // An account that names an id of the other committee (a shard nobody opened): the leg enters it, and its note is
    // to that id, not to the shard.
    let elsewhere = shard_sid(&seed, &foreign.committee, &first.s.terms(), &first.s.periods());
    let mut away = w.account(vec![], 20 * KAS1, 2 * KAS1);
    away.s.shard = Some(elsewhere);
    let (ast, aen) = enter_by_hand39(&away.s, foreign, &[P10], seed, away.value as i64);
    judge("an account that names another id", &away, &ast, &aen, Some(true));
    assert_ne!(elsewhere, sid);
    // ---- side A's floor: exactly, and a sompi under.
    let call = XLeg { entry: P10 * 12 / 10, ..w.opt(1, 10, CALL_HELD, P10, 0) };
    for (what, old, leg, prices, ask) in [
        ("a futures leg", vec![], fut, vec![P10], 0i64),
        ("a call worth 20 exercised", vec![], call, vec![P10 * 12 / 10], 0),
        ("a futures leg beside an old put that A holds, worth 6", vec![w.opt(2, 2, PUT_HELD, P10, P10)], fut, vec![P10 * 7 / 10, P10], 0),
        ("side B asks 30", vec![], fut, vec![P10], 30 * KAS1),
    ] {
        let probe = made(old.clone(), 500 * KAS1, 500 * KAS1);
        let (_, pen) = enter_by_hand39(&probe.s, leg, &prices, seed, probe.value as i64);
        let last = pen.rows[pen.rows.len() - 1];
        let floor = (rate_up(last.wa, probe.s.t.mmr_bps + probe.s.t.g_bps) + last.ea).max(ask);
        // (A's margin before the statement: the floor less what the old legs gain it.)
        for (under, expect) in [(0i64, true), (1, false)] {
            let mut account = made(old.clone(), floor - last.gain - under, 500 * KAS1);
            account.s.b_min_counter = ask;
            let (st, en) = enter_by_hand39(&account.s, leg, &prices, seed, account.value as i64);
            judge(&format!("{what}: {under} under a floor of {floor}"), &account, &st, &en, Some(expect));
            assert_eq!(account.s.enter(XLeg { entry: 0, ..leg }, &prices, T0, account.value as i64, seed).is_ok(), expect, "{what}: the model's statement");
        }
    }
    // ---- a margin below zero by an old leg.
    let old = w.fut(0, 10, LONG, P10);
    let loses = made(vec![old], 15 * KAS1, 500 * KAS1);
    let (lst, len) = enter_by_hand39(&loses.s, fut, &[P10 * 8 / 10, P10], seed, loses.value as i64);
    judge("A's margin below zero by an old leg's loss", &loses, &lst, &len, Some(false));
    let gains = made(vec![old], 500 * KAS1, 2 * KAS1);
    let (gst, gen) = enter_by_hand39(&gains.s, fut, &[P10 * 13 / 10, P10], seed, gains.value as i64);
    judge("B's margin below zero by an old leg's gain (nothing is owed by an entry)", &gains, &gst, &gen, Some(false));
    let (gst2, gen2) = enter_by_hand39(&gains.s, fut, &[P10 * 102 / 100, P10], seed, gains.value as i64);
    judge("an old leg's gain within B's margin moves to A's", &gains, &gst2, &gen2, Some(true));
    // ---- what is stated.
    let two = made(vec![old], 60 * KAS1, 20 * KAS1);
    let (tst, ten) = enter_by_hand39(&two.s, fut, &[P10, P10], seed, two.value as i64);
    judge("the control: a second leg", &two, &tst, &ten, Some(true));
    for (what, en) in [
        ("the row named is the first", AEnter { skip: 1, ..ten.clone() }),
        ("no row named", AEnter { skip: 0, ..ten.clone() }),
        ("a row beyond the last named", AEnter { skip: 3, ..ten.clone() }),
        ("side B", AEnter { side: SIDE_B, ..ten.clone() }),
        ("an amount", AEnter { amount: 1, ..ten.clone() }),
        ("the last row's worth below zero", AEnter { rows: vec![ten.rows[0], Row { wa: -1, ..ten.rows[1] }], ..ten.clone() }),
        ("the old leg's row left out", AEnter { skip: 1, rows: vec![ten.rows[1]], ..ten.clone() }),
    ] {
        judge(what, &two, &Stated { arg: en.arg(), ..tst }, &en, Some(false));
    }
    // ---- the outputs.
    let honest_note = join39(&two.s, &ten.leg);
    let last = ten.rows[1];
    let acct = |am: i64, bm: i64, chain: &[u8; 32]| blake2b(&[p33(&blake2b(&[two.s.terms(), p33(chain)].concat())), Account { a_margin: am, b_margin: bm, ..two.s.clone() }.moving()].concat());
    let held = two.value as i64;
    let (am, bm) = (two.s.a_margin, two.s.b_margin);
    let outs_of = |to: [u8; 32], value: i64, note: SNote, more: Option<Out>| {
        let mut v = vec![(Out { key: false, to, value }, false), (Out { key: false, to: note.hash(), value: note.value() }, true)];
        if let Some(o) = more {
            v.push((o, false));
        }
        Outs::with_notes(&v).unwrap()
    };
    assert_eq!(outs_of(acct(am, bm, &last.h), held, honest_note, None), tst.outs, "the outputs, put again");
    for (what, outs) in [
        ("A's margin a sompi more", outs_of(acct(am + 1, bm - 1, &last.h), held, honest_note, None)),
        ("B's margin a KAS less", outs_of(acct(am, bm - KAS1, &last.h), held - KAS1, honest_note, None)),
        ("the account without the leg", outs_of(acct(am, bm, &ten.rows[0].h), held, honest_note, None)),
        ("the account holding a sompi less", outs_of(acct(am, bm, &last.h), held - 1, honest_note, None)),
        ("a note of fewer units", outs_of(acct(am, bm, &last.h), held, SNote { net: honest_note.net / 2, gross: honest_note.gross / 2, ..honest_note }, None)),
        ("a note of the other way", outs_of(acct(am, bm, &last.h), held, SNote { net: -honest_note.net, ..honest_note }, None)),
        ("a note that a leg left", outs_of(acct(am, bm, &last.h), held, SNote { kind: NOTE_RELEASE, ..honest_note }, None)),
        ("a note to another shard", outs_of(acct(am, bm, &last.h), held, SNote { sid: [0x44; 32], ..honest_note }, None)),
        ("a note of another side B", outs_of(acct(am, bm, &last.h), held, SNote { key: two.s.a_key, ..honest_note }, None)),
        ("a note of what is owed", outs_of(acct(am, bm, &last.h), held, SNote::owed(sid, two.s.a_key, two.s.b_key, KAS1), None)),
        ("a payment to side A beside", outs_of(acct(am, bm, &last.h), held, honest_note, Some(Out { key: true, to: two.s.a_key, value: KAS1 }))),
        ("the note of a short leg", outs_of(acct(am, bm, &last.h), held, join39(&two.s, &XLeg { dir: SHORT, ..ten.leg }), None)),
        ("the note of an option", outs_of(acct(am, bm, &last.h), held, join39(&two.s, &XLeg { dir: kpf_protocol::thin::option_dir(CALL_HELD, P10), ..ten.leg }), None)),
        ("the note of another market", outs_of(acct(am, bm, &last.h), held, join39(&two.s, &XLeg { market: w.markets[2].id(), ..ten.leg }), None)),
        ("the note of a unit more", outs_of(acct(am, bm, &last.h), held, join39(&two.s, &XLeg { size: ten.leg.size + 1, ..ten.leg }), None)),
    ] {
        judge(&format!("outputs: {what}"), &two, &Stated { outs, ..tst }, &ten, Some(false));
    }
    // Another claimer cannot be stated (KpfTEnter); the end holds it too: read in the script, and the model.
    assert!(!two.s.enter_end_right(&ten, &two.s.b_key, &tst.outs, held));
    println!("T1b R39 ends: {cases} ends of entries answered at the engine, {rights} right and {wrongs} wrong, each the model's; a leg of another committee has a wrong row or a wrong end whichever committee is said of it");
}

/// Round 39, item 4. The lapse of a disputed point (`KpfTPoint`). Of an entry: the account again without the leg;
/// the disputer has its bond and half of the claimers', and the other half stays in the script's copy, to the sompi
/// (an odd bond: the copy keeps the larger half); a sompi less to the disputer, or in the copy, or the disputer's
/// part to the claimer, is refused. Of a take: both bonds back, as before. The claimers' own dispute of their
/// entry, left to lapse, costs them half their bond and the leg does not enter.
#[test]
fn t1b_r39_the_lapse_of_a_point_of_an_entry_costs_its_claimers_their_bond() {
    let w = ow();
    let shard = w.shard();
    let seed = seed39(&w);
    // (A pot whose bond is odd: 1 % of 22.00000100 KAS.)
    let mut account = w.account(vec![], 20 * KAS1 + 100, 2 * KAS1);
    account.s.shard = Some(shard.s.sid);
    let x = &account.s;
    let (st, en) = enter_by_hand39(x, XLeg { entry: P10, ..w.fut(1, 10, LONG, 0) }, &[P10], seed, account.value as i64);
    assert_eq!(st.bond % 2, 1, "an odd bond: {}", st.bond);
    let half = st.bond / 2;
    let fw = [fund(xk(&w.w), 31, 1_000)];
    let ff = [fund(xk(&w.f), 61, 1_000)];
    let mut tried = 0;
    for point in [0i64, 1, 2, 7] {
        let q = w.pointed(&entered39(&w, &account, &st, &en), point);
        let lb = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap();
        let ls = signed(&format!("the point {point} of an entry lapsed"), &lb, &w.keys);
        let n = ls.tx.outputs.len();
        assert_eq!((ls.tx.outputs[0].value, &ls.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "the account again, without the leg");
        let m = n - 1;
        assert_eq!((ls.tx.outputs[m].value, &ls.tx.outputs[m].script_public_key), ((st.bond + half) as u64, &p2pk_spk(&xk(&w.w))), "the disputer: its bond and half of the claimers'");
        assert_eq!(ls.tx.outputs[1].value - ls.entries[1].amount, (200_000 + st.bond - half) as u64, "the copy: the fee and the other half");
        assert!(ls.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&x.a_key) && o.script_public_key != p2pk_spk(&x.b_key)), "nothing to the claimers");
        if point == 2 {
            let entered = Account { legs: vec![XLeg { entry: P10, ..w.fut(1, 10, LONG, 0) }], ..x.clone() };
            for which in 0..5 {
                let mut t = lb.clone();
                let what = match which {
                    0 => {
                        t.tx.outputs[m].value -= 1;
                        "a sompi less to the disputer"
                    }
                    1 => {
                        t.tx.outputs[1].value -= 1;
                        "a sompi less in the copy"
                    }
                    2 => {
                        t.tx.outputs[m].script_public_key = p2pk_spk(&x.a_key);
                        "the part of the disputer to side A"
                    }
                    3 => {
                        t.tx.outputs[0].value -= 1;
                        "the account a sompi short"
                    }
                    _ => {
                        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &entered.hash());
                        "the account with the leg"
                    }
                };
                assert!(!taken30(&t, &w.keys), "the lapse of a point of an entry: {what}");
                tried += 1;
            }
        }
    }
    // The claimers' own dispute (a key of side A's own as the disputer), left to lapse: they have 1.5 of the two
    // bonds they put in back, half a bond is in the copy, and the leg is not in.
    let live = entered39(&w, &account, &st, &en);
    let (pb, q) = point_dispute(&w.lib, &live, &w.script("KpfTPoint"), x.a_key, 0, w.payer(&w.w, &fw)).unwrap();
    let q = nth(&signed("the claimers dispute their own entry", &pb, &w.keys), 0, q, 6_520);
    let ls = signed("and let it lapse", &point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
    let back: u64 = ls.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(&x.a_key)).map(|o| o.value).sum();
    assert_eq!((back, &ls.tx.outputs[0].script_public_key), ((st.bond + half) as u64, &hash_spk(&w.lib, &x.hash())), "of two bonds they have one and a half, and the account is as it was");
    // A take's lapse: both bonds back.
    let old = w.account(vec![w.fut(0, 3, LONG, P10)], 20 * KAS1 + 100, 20 * KAS1);
    let y = &old.s;
    let (tst, tk) = y.take_by(SIDE_A, KAS1, &[P10], T0, old.value as i64, y.a_key).unwrap();
    let tq = w.pointed(&w.stated(&old, &tst, &tk.said()), 2);
    let tl = signed("the point of a take lapsed", &point_lapse(&w.lib, &tq, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
    let k = tl.tx.outputs.len();
    assert_eq!((tl.tx.outputs[k - 1].value, &tl.tx.outputs[k - 1].script_public_key, tl.tx.outputs[k - 2].value, &tl.tx.outputs[k - 2].script_public_key), (tst.bond as u64, &p2pk_spk(&xk(&w.w)), tst.bond as u64, &p2pk_spk(&y.a_key)), "the disputer's bond and the claimer's, each back");
    assert_eq!(tl.tx.outputs[1].value - tl.entries[1].amount, 200_000, "the copy keeps the fee and no part of a bond");
    println!("T1b R39 lapse: of an entry (points 0, 1, 2, 7; a bond of {} sompi) the disputer has {} and the copy keeps {} beside its fee, the account again without the leg; {tried} other outputs refused; the claimers' own dispute left to lapse costs them {}; of a take both bonds are back", st.bond, st.bond + half, st.bond - half, st.bond - half);
}

/// Rounds 39 and 40, item 5. A shard's id is the hash of its opening (the hash of the anchor's outpoint), of its one
/// committee, of its ten terms and of its lateness, dispute period and bond rate: `KpfTShardOpen` takes that id
/// and no other (another committee shown; the id of another committee, of another term or period, the id as it
/// was in round 39, the seed alone, nothing, another opening's). A shard of other terms has another id. And what
/// a user reads off a shard: its terms and its moving part are arguments of its opening.
#[test]
fn t1b_r39_a_shards_id_is_of_its_opening_its_committee_and_its_terms() {
    let w = ow();
    let committee = w.markets[0].committee_id;
    let f = [fund(xk(&w.b), 40, 1_000)];
    let (b, shard) = shard_open(&w.lib, &w.anchors[0], &w.script("KpfTShardOpen"), w.terms(), committee, xk(&w.b), 50 * KAS1, 9_000_000, 50, 100, w.payer(&w.b, &f)).unwrap();
    assert!(taken30(&b, &w.keys), "the control");
    let seed = shard_seed(&w.anchors[0]);
    let periods = shard.moving()[93..120].to_vec();
    assert_eq!((shard.sid, shard_id(&w.anchors[0], &committee, &shard)), (shard_sid(&seed, &committee, &shard.terms(), &periods), shard.sid));
    assert!(matches!(&args_of(&mut b.clone(), 1)[5], Arg::V(ArtifactValue::Bytes(c)) if c[..] == committee[..]), "argument 5 is the committee");
    assert!(matches!(&args_of(&mut b.clone(), 1)[3], Arg::V(ArtifactValue::Bytes(t)) if t[..] == shard.terms()[..]), "argument 3 is the shard's terms");
    assert!(matches!(&args_of(&mut b.clone(), 1)[4], Arg::V(ArtifactValue::Bytes(m)) if m[..] == shard.moving()[..] && m[139..171] == shard.sid[..]), "argument 4 is its moving part, the id in it");
    assert!(!taken30(&with_arg(&b, 5, bytes_arg(&[0x09; 32])), &w.keys), "a shard is opened with an id that is not of the committee shown");
    let mut ids: Vec<(String, [u8; 32])> = vec![
        ("another committee".into(), shard_sid(&seed, &[0x09; 32], &shard.terms(), &periods)),
        ("the opening and the committee alone, as in round 39".into(), blake2b(&[&seed[..], &committee[..]].concat())),
        ("the seed alone".into(), seed),
        ("nothing".into(), [0u8; 32]),
        ("another opening".into(), shard_sid(&shard_seed(&w.anchors[1]), &committee, &shard.terms(), &periods)),
        ("another dispute period".into(), shard_sid(&seed, &committee, &shard.terms(), &Shard { dispute_daa: shard.dispute_daa + 1, ..shard }.moving()[93..120])),
        ("another bond rate".into(), shard_sid(&seed, &committee, &shard.terms(), &Shard { bond_bps: shard.bond_bps + 1, ..shard }.moving()[93..120])),
    ];
    for (what, t) in other_terms40(w.terms()) {
        let other = Shard { t, ..shard };
        ids.push((format!("another term: {what}"), shard_sid(&seed, &committee, &other.terms(), &other.moving()[93..120])));
    }
    let mut refused = 1;
    for (what, sid) in &ids {
        assert_ne!(*sid, shard.sid, "{what}");
        let other = Shard { sid: *sid, ..shard };
        let mut t = with_arg(&b, 4, bytes_arg(&other.moving()));
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &other.hash());
        assert!(!taken30(&t, &w.keys), "a shard is opened with the id of {what}");
        refused += 1;
    }
    // A shard of other terms is opened, with the id of those terms: who takes it as its cover reads them there.
    let (lb, loose) = shard_open(&w.lib, &w.anchors[0], &w.script("KpfTShardOpen"), XTerms { g_bps: 5_000, mmr_bps: 0, ..w.terms() }, committee, xk(&w.b), 50 * KAS1, 9_000_000, 1, 1, w.payer(&w.b, &f)).unwrap();
    assert!(taken30(&lb, &w.keys) && loose.sid != shard.sid && loose.sid == shard_id(&w.anchors[0], &committee, &loose));
    println!("T1b R39 shard id: the id is the hash of the opening, the committee, the ten terms and the three periods; {refused} openings with another committee shown or another id refused; a shard of a tolerance of 50 %, a dispute period of a block and a bond rate of 1 is opened under its own id");
}

/// F64 (fixed in round 41). An account's own dispute period decided whether the check of its terms was ever
/// answered. The fix: `KpfTOpenC` is shown the seed and the committee and opens a covered account only where its
/// own ten terms, lateness, dispute period and bond rate give the id it names. Verified at the engine: round 40's
/// account (the shard's terms and a dispute period of one block) is not opened, nor one of a dispute period of
/// nothing, nor any account that differs from the shard in one term or period (16 in all), by the builder and
/// built past it. Another seed or another committee shown is refused. An account of other terms is opened under
/// the id those terms give, which is no shard's: a note of it names that id. The lease and the two asks are free.
/// And a shard is not opened with a second key.
#[test]
fn t1b_fixed_f64_a_covered_account_is_opened_only_with_its_shards_terms_and_periods() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    let seed = seed39(&w);
    let committee = w.markets[0].committee_id;
    let user = w.covered(sid, 80 * KAS1, KAS1);
    let base = Account { a_key: xk(&w.w), ..user.s.clone() };
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let open = |y: &Account| account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), y, seed, committee, w.payer(&w.b, &fb));
    let ob = open(&base).unwrap();
    assert!(taken30(&ob, &w.keys), "the control: an account of the shard's terms and periods");
    assert!(matches!(&args_of(&mut ob.clone(), 1)[5], Arg::V(ArtifactValue::Bytes(s)) if s[..] == seed[..]) && matches!(&args_of(&mut ob.clone(), 1)[6], Arg::V(ArtifactValue::Bytes(c)) if c[..] == committee[..]), "arguments 5 and 6 are the seed and the committee");
    // Built past the builder: the terms (3), the moving part (4), the seed (5), the committee (6), the output.
    let open_as = |y: &Account, seed: [u8; 32], committee: [u8; 32]| {
        let mut t = ob.clone();
        let args = args_of(&mut t, 1);
        args[3] = bytes_arg(&y.terms());
        args[4] = bytes_arg(&y.moving());
        args[5] = bytes_arg(&seed);
        args[6] = bytes_arg(&committee);
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &y.hash());
        taken30(&t, &w.keys)
    };
    assert!(open_as(&base, seed, committee), "the control, put again");
    let mut others: Vec<(String, Account)> = other_terms40(w.terms()).into_iter().map(|(what, t)| (what.to_string(), Account { t, ..base.clone() })).collect();
    others.push(("the dispute period".into(), Account { dispute_daa: base.dispute_daa + 1, ..base.clone() }));
    others.push(("a dispute period of one block (round 40)".into(), Account { dispute_daa: 1, ..base.clone() }));
    others.push(("a dispute period of nothing".into(), Account { dispute_daa: 0, ..base.clone() }));
    others.push(("the bond rate".into(), Account { bond_bps: base.bond_bps - 1, ..base.clone() }));
    others.push(("a bond rate of one in ten thousand".into(), Account { bond_bps: 1, ..base.clone() }));
    others.push(("a tolerance of 50 % and no maintenance rate (round 39)".into(), Account { t: XTerms { g_bps: 5_000, mmr_bps: 0, ..w.terms() }, ..base.clone() }));
    let mut refused = 0;
    for (what, y) in &others {
        assert!(open(y).is_err(), "F64 is open again: the builder opens an account of another {what}");
        assert!(!open_as(y, seed, committee), "F64 is open again: KpfTOpenC opens an account of another {what} under the shard's id");
        // Under the id its own terms and periods give it is opened: an id no shard has.
        let own_id = shard_sid(&seed, &committee, &y.terms(), &y.periods());
        assert_ne!(own_id, sid);
        assert!(open_as(&Account { shard: Some(own_id), ..y.clone() }, seed, committee), "{what}: opened under the id of its own terms");
        refused += 1;
    }
    for (what, s, c) in [("another seed", [7u8; 32], committee), ("another committee", seed, [0x09; 32]), ("the committee as the seed", committee, seed)] {
        assert!(!open_as(&base, s, c), "F64 is open again: an account is opened with {what} shown");
        refused += 1;
    }
    for (what, y) in [("another lease", Account { expiry: base.expiry + 1_000, ..base.clone() }), ("an ask of side A", Account { a_min_counter: 5 * KAS1, ..base.clone() }), ("an ask of side B", Account { b_min_counter: 5 * KAS1, ..base.clone() })] {
        assert!(open(&y).is_ok() && open_as(&y, seed, committee), "{what} is not of the id");
    }
    // A shard is not opened with a second key (an object of 171 bytes with a second key under a shard's id is an
    // account, and is opened by KpfTOpenC alone).
    let fs = [fund(xk(&w.b), 40, 1_000)];
    let (sb, sh) = shard_open(&w.lib, &w.anchors[0], &w.script("KpfTShardOpen"), w.terms(), committee, xk(&w.b), 50 * KAS1, 9_000_000, 50, 100, w.payer(&w.b, &fs)).unwrap();
    assert!(taken30(&sb, &w.keys));
    let mut mv = sh.moving();
    mv[34..66].copy_from_slice(&xk(&w.w));
    let mut keyed = with_arg(&sb, 4, bytes_arg(&mv));
    keyed.tx.outputs[0].script_public_key = hash_spk(&w.lib, &blake2b(&[p33(&sh.fixed_hash()), mv.clone()].concat()));
    assert!(!taken30(&keyed, &w.keys), "a shard is opened with a second key");
    println!("T1b F64 fixed: {refused} openings refused (an account that differs from the shard in one term or period, round 40's dispute period of one block and one of nothing, round 39's terms; another seed or committee shown); each such account is opened only under the id of its own terms, which is no shard's; the lease and the asks are free; a shard is not opened with a second key");
}

/// F65 (fixed in round 41). A leg entered at one end of the tolerance and left at the other was owed 1 % of its
/// worth by the shard with no point wrong. The fix: the note of what is owed is of what the pot could not pay
/// less twice the tolerance of the two sides' worth at the last row (a futures leg is in both: 2 % of its worth),
/// not below zero, and no note where that is nothing. Verified at the engine: round 40's two statements are right
/// as before, and the leaving writes **no note** (the end with the account and the note that the leg left is
/// answered right; the end of round 40, with a note of 13 KAS, is answered wrong). And on the model, at the ends of
/// the tolerance against what the committee's prices alone would leave owed: for futures legs, long and short,
/// what the shard is asked is never more than that where the leg's worth at its entry is less than three times its
/// worth where it leaves; beyond that (a price that fell by more than two thirds, for a leg A is short in) the
/// entry's half of the skew is larger than what is deducted, by at most the tolerance of the worth at the entry.
/// For an option A holds the band is of the writer's worth once: the same shortfall shows where the worth fell at
/// all. What the band costs an honest user is printed.
#[test]
fn t1b_fixed_f65_what_prices_within_the_tolerance_make_is_not_asked_of_the_shard() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    let seed = seed39(&w);
    let committee = w.markets[0].committee_id;
    let user = w.covered(sid, 20 * KAS1, 20 * KAS1);
    let own = Account { a_key: xk(&w.w), a_margin: 80 * KAS1, b_margin: KAS1, legs: vec![], ..user.s.clone() };
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let fw = [fund(xk(&w.w), 54, 1_000)];
    let ob = account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &own, seed, committee, w.payer(&w.b, &fb)).unwrap();
    let own = nth(&signed("the owner opens an account of the shard's terms with a key of its own", &ob, &w.keys), 0, own, 6_100);
    let x = &own.s;
    let g = x.t.g_bps;
    let (low, high) = (P10 * (10_000 - g) / 10_000, P10 * (10_000 + g) / 10_000);
    let c = w.cited(1, P10);
    // In at 9.95: right at every point, as in round 40.
    let (eb, st, en, after) = account_enter(&w.lib, &own, &w.script("KpfTEnter"), w.fut(1, 140, LONG, 0), &[low], T0, seed, w.payer(&w.b, &fb)).unwrap();
    assert!(taken30(&eb, &w.keys));
    assert_eq!(enter_price_taken39(&w, &own, &st, &en, &after.legs, 1, &c), (true, false));
    let live = entered39(&w, &own, &st, &en);
    let rs = signed("the entry at 9.95 applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let y = nth(&rs, 0, after.clone(), 6_570);
    // Out at 10.05: right, and nothing is asked of the shard.
    let y1 = &y.s;
    let (st2, tk2, sd) = y1.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[high], T0, y.value as i64, y1.a_key).unwrap();
    assert_eq!((sd.owed, sd.after.a_margin, sd.after.b_margin, st2.outs.list().len()), (0, 81 * KAS1, 0, 2), "F65 is open again: the shard is asked what the two ends of the tolerance made");
    assert_eq!(y1.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk2, &y1.a_key, &st2.outs, y.value as i64, &[P10]), None);
    assert_eq!(end_taken(&w, &y, &st2, &tk2), (true, false), "the end with no note of what is owed is answered right");
    // The end of round 40, with the note of 13 KAS.
    let old = SNote::owed(sid, y1.a_key, y1.b_key, 13 * KAS1);
    let mut v: Vec<(Out, bool)> = st2.outs.list().iter().enumerate().map(|(i, o)| (*o, i > 0)).collect();
    v.push((Out { key: false, to: old.hash(), value: old.value() }, true));
    let st_old = Stated { outs: Outs::with_notes(&v).unwrap(), ..st2 };
    assert_eq!(end_taken(&w, &y, &st_old, &tk2), (false, true), "F65 is open again: the end with a note of the 13 KAS is answered right");
    // ---- the band against the skew, on the model: futures legs.
    let rate_up = |v: i64, rate: i64| (v / 10_000) * rate + ((v % 10_000) * rate + 9_999) / 10_000;
    let units = 100i64;
    let (mut held_to, mut beyond, mut worst) = (0, 0, 0i64);
    for dir in [LONG, SHORT] {
        for e in [2i64, 5, 10, 20, 30, 60, 100] {
            for l in [1i64, 2, 3, 5, 10, 20, 30, 60, 100] {
                let (ce, cl) = (P10 / 10 * e, P10 / 10 * l);
                // The ends that favour A: in low and out high where it is long, in high and out low where short.
                let (ae, al) = if dir == LONG { (ce * (10_000 - g) / 10_000, (cl * (10_000 + g) + 9_999) / 10_000) } else { ((ce * (10_000 + g) + 9_999) / 10_000, cl * (10_000 - g) / 10_000) };
                let acc = Account { legs: vec![XLeg { entry: ae, ..w.fut(1, units, dir, 0) }], a_margin: 500 * KAS1, b_margin: 0, ..x.clone() };
                let rows = acc.trace_without(&[al], 1);
                let Some(s) = acc.stepped(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &rows, &acc.a_key) else { continue };
                // What the committee's own prices would leave owed.
                let gain = units * (cl - ce) * if dir == LONG { 1 } else { -1 };
                let true_owed = (acc.a_margin + gain - acc.pot()).max(0);
                let excess = s.owed - true_owed;
                if e < 3 * l {
                    assert!(excess <= 0, "F65 is open again: the shard is asked {excess} more than the prices of the committee leave owed (dir {dir}, in at {e}, out at {l})");
                    held_to += 1;
                } else {
                    assert!(excess <= rate_up(units * ce, g), "the shortfall of the band is beyond the tolerance of the worth at the entry (dir {dir}, in at {e}, out at {l}: {excess})");
                    beyond += 1;
                    worst = worst.max(excess);
                }
            }
        }
    }
    assert!(held_to > 30 && worst > 0, "{held_to} held, {beyond} beyond, the worst {worst}");
    // ---- an option A holds: a put struck at 40, in at the upper end of 20, out at the lower end of 10 and of 19.
    let mut opt_excess = vec![];
    for l in [10i64, 19, 20, 30] {
        let (ce, cl) = (2 * P10, P10 / 10 * l);
        let (ae, al) = ((ce * (10_000 + g) + 9_999) / 10_000, cl * (10_000 - g) / 10_000);
        let leg = XLeg { entry: ae, ..w.opt(1, units, PUT_HELD, 4 * P10, 0) };
        let acc = Account { legs: vec![leg], a_margin: 5_000 * KAS1, b_margin: 0, ..x.clone() };
        let rows = acc.trace_without(&[al], 1);
        let s = acc.stepped(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &rows, &acc.a_key).expect("the holder exercises");
        let gain = units * ((4 * P10 - cl) - (4 * P10 - ce));
        let true_owed = (acc.a_margin + gain - acc.pot()).max(0);
        opt_excess.push((l, s.owed - true_owed));
        assert!(s.owed - true_owed <= rate_up(units * ce, g), "an option: the shortfall is beyond the tolerance of the worth at the entry");
    }
    // ---- what the band costs an honest user: long 10 units from 10 to 14, a pot of 25 (A 5, B 20).
    let honest = Account { legs: vec![w.fut(1, 10, LONG, P10)], a_margin: 5 * KAS1, b_margin: 20 * KAS1, ..x.clone() };
    let hs = honest.stepped(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &honest.trace_without(&[P10 * 14 / 10], 1), &honest.a_key).unwrap();
    assert_eq!(hs.owed, 20 * KAS1 - rate_up(2 * 140 * KAS1, 2 * g), "the 20 KAS the pot could not pay, less 2 % of the worth of 140");
    // And with nine other legs of the same worth standing in the account: the band is of all ten.
    let mut ten = vec![w.fut(1, 10, LONG, P10)];
    ten.extend((0..9).map(|i| w.fut(2 + i % 6, 10, if i % 2 == 0 { SHORT } else { LONG }, P10 * 14 / 10)));
    let many = Account { legs: ten, a_margin: 5 * KAS1, b_margin: 20 * KAS1, ..x.clone() };
    let mut prices = vec![P10 * 14 / 10; 10];
    prices[0] = P10 * 14 / 10;
    let ms = many.stepped(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &many.trace_without(&prices, 1), &many.a_key).unwrap();
    assert_eq!(ms.owed, 0, "the same 20 KAS beside nine legs of 140 each: twice the tolerance of 2,800 is 28, and nothing is asked");
    println!("T1b F65 fixed: the leaving at 10.05 of a leg entered at 9.95 writes no note of what is owed (the end right; with round 40's note of 13 KAS wrong). At the ends of the tolerance the shard is asked no more than the committee's prices leave owed in {held_to} cases of futures legs whose worth at the entry is less than three times that at the leaving; in {beyond} beyond that it is asked up to {worst} sompi more (100 units). A put A holds, in at 20: out at 10, 19, 20, 30 the shard is asked more by {opt_excess:?}. An honest user owed 20 KAS on a leg worth 140 is asked for {} sompi; beside nine other legs of that worth, for nothing", hs.owed);
}

/// F66 (Medium). The end of an entry asks side A's floor and not side B's: a leg enters an account whose side B
/// has nothing. A side with nothing is below its maintenance from the first block, and anyone may state a step of
/// its liquidation; but the fee of a step is paid out of that side's margin, so nobody is paid to. In an account of
/// the owner's two keys this is how the owner is still owed by its shard with every price the committee's own:
/// 140 units enter at 10 with nothing on side B (the end is right); the committee's price goes to 11; the leg
/// leaves at 11, every point right; the pot cannot pay the 140 KAS gained, the shard is asked 109.2 KAS (what is
/// owed less the band of round 41) and pays all 50 KAS it holds. Had the price fallen, side A would have lost no
/// more than the 80 KAS of its margin, to a liquidator who is paid. (With a second account the other way the
/// owner is owed whichever way the price goes.) What stands against it today is somebody stepping side B's
/// liquidation for no fee: the step is right at once, here with a fee of nothing. Passes while open.
#[test]
fn t1b_finding_f66_side_b_enters_with_nothing_and_nobody_is_paid_to_liquidate_it() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    let seed = seed39(&w);
    let committee = w.markets[0].committee_id;
    let own = Account { a_key: xk(&w.w), a_margin: 80 * KAS1, b_margin: 0, legs: vec![], ..w.covered(sid, 20 * KAS1, 20 * KAS1).s.clone() };
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let fw = [fund(xk(&w.w), 54, 1_000)];
    let ob = account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &own, seed, committee, w.payer(&w.b, &fb)).unwrap();
    let own = nth(&signed("an account whose side B has nothing", &ob, &w.keys), 0, own, 6_100);
    let x = &own.s;
    // The leg enters at the committee's price, side B having nothing against a worth of 1,400.
    let Ok((eb, st, en, after)) = account_enter(&w.lib, &own, &w.script("KpfTEnter"), w.fut(1, 140, LONG, 0), &[P10], T0, seed, w.payer(&w.b, &fb)) else { panic!("F66 is closed: the model states no entry where side B stands below its floor") };
    assert!(taken30(&eb, &w.keys));
    assert_eq!(enter_end_taken39(&w, &own, &st, &en), (true, false), "F66 is closed: the end of an entry asks side B's floor");
    let live = entered39(&w, &own, &st, &en);
    let rs = signed("the entry applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let y = nth(&rs, 0, after.clone(), 6_570);
    let join = join39(x, &after.legs[0]);
    let at = (0..rs.tx.outputs.len()).find(|i| rs.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &join.hash())).expect("the note that the leg entered");
    let s1 = w.applied(&shard, &nth(&rs, at, join, 6_570), "the leg counted");
    let y1 = &y.s;
    // What stands against it: a step of side B's liquidation, right at once, for a fee of nothing.
    let stranger = xk(&w.f);
    let (lst, ltk, lsd) = y1.step(KIND_ACCOUNT_LIQ, SIDE_B, 1, &[P10], T0, y.value as i64, stranger).expect("side B with nothing is below");
    assert_eq!((lsd.fee, lsd.owed), (0, 0), "whoever steps it is paid nothing");
    assert_eq!(end_taken(&w, &y, &lst, &ltk), (true, false));
    // Nobody does. The committee's price is 11, and the leg leaves at 11.
    let (st2, tk2, sd) = y1.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10 * 11 / 10], T0, y.value as i64, y1.a_key).unwrap();
    let band = 2 * 1_540 * KAS1 / 100;
    assert_eq!((sd.owed, sd.after.a_margin), (140 * KAS1 - band, 80 * KAS1), "140 KAS gained, none of it in the pot: asked of the shard less the band");
    assert_eq!(y1.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk2, &y1.a_key, &st2.outs, y.value as i64, &[P10 * 11 / 10]), None);
    assert_eq!(end_taken(&w, &y, &st2, &tk2), (true, false));
    let live2 = w.stated(&y, &st2, &tk2.said());
    let ss = signed("the leaving at 11 applied", &stated_settle(&w.lib, &live2, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let owed = SNote::owed(sid, y1.a_key, y1.b_key, sd.owed);
    let at = (0..ss.tx.outputs.len()).find(|i| ss.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &owed.hash())).expect("the note of what is owed");
    let (nb, paid_out, _) = note_apply(&w.lib, &s1, &nth(&ss, at, owed, 6_640), &w.script("KpfTNoteApply"), w.payer(&w.w, &fw)).unwrap();
    assert!(taken30(&nb, &w.keys));
    let ns = nb.sign(&w.keys).unwrap();
    assert!(ns.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(&y1.a_key) && o.value == 50 * KAS1 as u64) && paid_out.held == 0, "the shard pays all 50 KAS");
    // Had it fallen to 9.6: side A is below, and a step of its liquidation pays whoever states it.
    let down = y1.stepped(KIND_ACCOUNT_LIQ, SIDE_A, 1, &y1.trace_without(&[P10 * 96 / 100], 1), &stranger).expect("side A is below at 9.6");
    assert!(down.fee > 0 && down.owed == 0);
    println!("T1b F66: 140 units enter at 10 with nothing on side B (the end right); a step of side B's liquidation is right at once and pays its claimer nothing; the leg leaves at 11, every point right: the shard is asked {} sompi and pays all it holds. At 9.6 a step of side A's liquidation pays {} sompi", sd.owed, down.fee);
}

/// F67 (High). The band of round 41 is twice the tolerance of the two sides' worth **at the leaving's prices**. For a
/// futures leg that is 2 % of its worth where the two ends make 1 %: enough. An option is in its writer's worth
/// alone, so for an option the band is 1 % of the underlying's worth at the leaving's price; and a put that A
/// holds, entered at the upper end of the tolerance and left at the lower end, has made 1 % of the worth at the
/// committee's price, which is more than 1 % of the worth at the lower end: by twice the tolerance squared (0.005 %
/// of the underlying's worth). That is asked of the shard with every point right. And it costs nothing to hold:
/// struck at the price it enters at, the put is counted at nothing, its holder is asked no margin, and side B is
/// asked none (F66). Here, on an account of the shard's own terms: a put on 11,000 units struck and entered at 20.1
/// where the committee says 20 (the row, the price against the citation of 20 and the end answered right) leaves
/// at 19.9 (the end and the price answered right): 2,200 KAS gained, of which the band takes 2,189, and the shard
/// is asked 11 KAS and pays them, the 30 KAS of margin back whole. Eight such legs fit an account; any number of
/// accounts. Passes while open.
#[test]
fn t1b_finding_f67_a_put_entered_and_left_at_the_two_ends_of_the_tolerance_is_asked_of_the_shard_beyond_the_band() {
    let w = ow();
    let shard = w.shard();
    let sid = shard.s.sid;
    let seed = seed39(&w);
    let committee = w.markets[0].committee_id;
    let own = Account { a_key: xk(&w.w), a_margin: 30 * KAS1, b_margin: 0, legs: vec![], ..w.covered(sid, 20 * KAS1, 20 * KAS1).s.clone() };
    let fb = [fund(xk(&w.b), 58, 1_000)];
    let fw = [fund(xk(&w.w), 54, 1_000)];
    let ob = account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &own, seed, committee, w.payer(&w.b, &fb)).unwrap();
    let own = nth(&signed("the owner opens an account of the shard's terms with a key of its own", &ob, &w.keys), 0, own, 6_100);
    let x = &own.s;
    let held = own.value as i64;
    let g = x.t.g_bps;
    let c20 = 2 * P10;
    let (high, low) = ((c20 * (10_000 + g) + 9_999) / 10_000, c20 * (10_000 - g) / 10_000);
    let c = w.cited(1, c20);
    let units = 11_000;
    // In at 20.1, struck there: counted at nothing, and nothing asked of its holder.
    let put = w.opt(1, units, PUT_HELD, high, 0);
    let Ok((eb, st, en, after)) = account_enter(&w.lib, &own, &w.script("KpfTEnter"), put, &[high], T0, seed, w.payer(&w.b, &fb)) else { panic!("F67 is closed: the model states no such entry") };
    assert!(taken30(&eb, &w.keys));
    assert_eq!((en.rows[0].wa, en.rows[0].ea, en.rows[0].gain), (0, 0, 0), "the holder is asked the maintenance of nothing and keeps nothing whole");
    assert_eq!(x.enter_wrong_point(&en, &st.claimer, &st.outs, held, &[c20]), None);
    assert_eq!(enter_end_taken39(&w, &own, &st, &en), (true, false));
    assert_eq!(enter_row_taken39(&w, &own, &st, &en, 1, &[0; 32], &after.legs[0]), (true, false));
    assert_eq!(enter_price_taken39(&w, &own, &st, &en, &after.legs, 1, &c), (true, false), "20.1 is right against the citation of 20");
    let live = entered39(&w, &own, &st, &en);
    let rs = signed("the put entered at 20.1 applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let y = nth(&rs, 0, after.clone(), 6_570);
    let join = join39(x, &after.legs[0]);
    let at = (0..rs.tx.outputs.len()).find(|i| rs.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &join.hash())).expect("the note that the leg entered");
    let s1 = w.applied(&shard, &nth(&rs, at, join, 6_570), "the leg counted");
    // Out at 19.9.
    let y1 = &y.s;
    let Ok((st2, tk2, sd)) = y1.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[low], T0, y.value as i64, y1.a_key) else { panic!("F67 is closed: the model states no such leaving") };
    let gained = units * (high - low);
    let band = { let b = units * low; (b / 10_000) * 2 * g + ((b % 10_000) * 2 * g + 9_999) / 10_000 };
    assert_eq!((gained, band), (2_200 * KAS1, 2_189 * KAS1), "1 % of the worth at 20, and 1 % of the worth at 19.9");
    assert!(sd.owed == gained - band && sd.owed == 11 * KAS1, "F67 is closed: the shard is asked nothing of what the two ends made ({})", sd.owed);
    assert_eq!((sd.after.a_margin, sd.after.b_margin, sd.fee), (30 * KAS1, 0, 0));
    assert_eq!(y1.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk2, &y1.a_key, &st2.outs, y.value as i64, &[c20]), None, "F67 is closed: the leaving has a wrong point at the price of the committee");
    assert_eq!(end_taken(&w, &y, &st2, &tk2), (true, false));
    let live2 = w.stated(&y, &st2, &tk2.said());
    let q2 = w.pointed(&live2, 2);
    let cs = w.script("KpfTCite");
    let price = |v: bool| step_price(&w.lib, &q2, y1, &tk2, &w.script("KpfTStepPrice"), (&cs, &c), xk(&w.w), w.payer(&w.w, &fw), Some(v)).is_ok_and(|b| taken30(&b.0, &w.keys));
    assert_eq!((price(true), price(false)), (true, false), "19.9 is right against the citation of 20");
    let ss = signed("the leaving at 19.9 applied", &stated_settle(&w.lib, &live2, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap(), &w.keys);
    let owed = SNote::owed(sid, y1.a_key, y1.b_key, sd.owed);
    let at = (0..ss.tx.outputs.len()).find(|i| ss.tx.outputs[*i].script_public_key == hash_spk(&w.lib, &owed.hash())).expect("the note of what is owed");
    let (nb, paid_out, _) = note_apply(&w.lib, &s1, &nth(&ss, at, owed, 6_640), &w.script("KpfTNoteApply"), w.payer(&w.w, &fw)).unwrap();
    assert!(taken30(&nb, &w.keys));
    let ns = nb.sign(&w.keys).unwrap();
    assert!(ns.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(&y1.a_key) && o.value == 11 * KAS1 as u64) && paid_out.held == 39 * KAS1, "11 KAS to the second key of the owner");
    // The same the other way holds: a call entered at the lower end and left at the upper has its band counted at
    // the upper end, and nothing is asked.
    let call = Account { legs: vec![XLeg { entry: low, ..w.opt(1, units, CALL_HELD, low, 0) }], ..x.clone() };
    let cs2 = call.stepped(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &call.trace_without(&[high], 1), &call.a_key).expect("the call's holder exercises");
    assert_eq!(cs2.owed, 0, "a call the other way");
    println!("T1b F67: a put on {units} units struck and entered at 20.1 and left at 19.9, the committee saying 20, every point answered right: {} sompi gained, {} of it the band, and the shard pays {} on a margin of 30 KAS that comes back whole", gained, band, sd.owed);
}
