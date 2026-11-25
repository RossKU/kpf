//! The thin family's pinned artifacts and the model of its objects (`kpf_protocol::thin`).

use kpf_protocol::cross::{SIDE_A, SIDE_B};
use kpf_protocol::thin::{samples, Position, TObj, FIXED_LEN, MOVING_LEN};
use kpf_protocol::thin_artifacts as pinned;
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn contracts() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts")
}

#[test]
fn the_manifest_and_the_checksums_agree_with_the_sources() {
    let m = pinned::manifest();
    assert_eq!(m.format, "kpf-artifacts/thin-2");
    assert_eq!(pinned::artifact_texts().len(), m.contracts.len());
    for (name, text) in pinned::artifact_texts() {
        let c = &m.contracts[*name];
        let src = std::fs::read_to_string(contracts().join(format!("{name}.sil"))).unwrap().replace("\r\n", "\n");
        assert_eq!(c.source_sha256, sha(src.as_bytes()), "contracts/{name}.sil changed: regenerate the artifacts");
        assert_eq!(c.artifact_sha256, sha(text.replace("\r\n", "\n").as_bytes()), "{name}");
    }
    let sums = std::fs::read_to_string(contracts().join("artifacts/SHA256SUMS")).unwrap();
    let mut seen = 0;
    for line in sums.lines() {
        let (h, path) = line.split_once("  ").unwrap();
        let text = std::fs::read_to_string(contracts().join(path)).unwrap().replace("\r\n", "\n");
        assert_eq!(h, sha(text.as_bytes()), "{path}");
        seen += 1;
    }
    assert_eq!(seen, 2 * (kpf_protocol::thin_artifacts::SCRIPTS.len() + 3) + 1, "a source and an artifact of every script, of the object, of the price note and of the fee pool, and the manifest");
    let k = pinned::kit();
    assert_eq!(k.object_len(), m.contracts[pinned::SLOT].script_len);
    for n in pinned::SCRIPTS {
        assert_eq!(k.script(n).unwrap().len(), m.contracts[n].script_len, "{n}");
    }
}

#[test]
fn an_objects_script_is_the_slot_with_its_two_values() {
    let k = pinned::kit();
    let (lib, state) = ([0xa1; 32], [0xb2; 32]);
    let s = k.object(&lib, &state);
    assert_eq!(k.read(&s), Some((lib, state)));
    assert_eq!(&s[2..34], &state, "the scripts of the library read the state's hash at byte 2");
    // Another library or another state is another script, of the same length; anything else is not an object.
    assert_ne!(k.object(&[0xa2; 32], &state), s);
    assert_ne!(k.object(&lib, &[0xb3; 32]), s);
    let mut t = s.clone();
    t[0] ^= 1;
    assert_eq!(k.read(&t), None);
    assert_eq!(k.read(&s[1..]), None);
    assert_eq!(k.read(k.script("KpfTAdd").unwrap()), None);
}

#[test]
fn the_states_hash_in_two_parts_and_every_field_is_in_one() {
    let a = samples::position();
    a.check().unwrap();
    assert_eq!((a.fixed().len(), a.moving().len()), (FIXED_LEN, MOVING_LEN));
    let h = a.hash();
    // Every field of the position moves the hash.
    let mut vs: Vec<Position> = vec![];
    let mut x = a;
    x.x.t.mmr_bps += 1;
    vs.push(x);
    x = a;
    x.x.legs[1].entry += 1;
    vs.push(x);
    x = a;
    x.x.a_key[0] ^= 1;
    vs.push(x);
    x = a;
    x.x.b_margin += 1;
    vs.push(x);
    x = a;
    x.x.expiry += 1;
    vs.push(x);
    x = a;
    x.x.t.late_daa += 1;
    vs.push(x);
    x = a;
    x.dispute_daa += 1;
    vs.push(x);
    x = a;
    x.bond_bps += 1;
    vs.push(x);
    for v in &vs {
        assert_ne!(v.hash(), h);
    }
    // Margin added by a side is that side's and nothing else.
    let b = a.added(SIDE_A, 5).unwrap();
    assert_eq!((b.x.a_margin, b.x.b_margin, b.fixed_hash()), (a.x.a_margin + 5, a.x.b_margin, a.fixed_hash()));
    assert_eq!(a.added(SIDE_B, 7).unwrap().x.b_margin, a.x.b_margin + 7);
    assert!(a.added(SIDE_A, 0).is_none() && a.added(3, 1).is_none() && a.added(SIDE_A, i64::MAX).is_none());
    // The dispute period and the bond rate are bounded.
    assert!(Position { dispute_daa: 0, ..a }.check().is_err() && Position { bond_bps: 10_001, ..a }.check().is_err() && Position { bond_bps: 0, ..a }.check().is_err());
}

#[test]
fn a_claim_pays_what_it_states_and_a_dispute_moves_the_bonds() {
    let a = samples::position();
    let pot = a.pot();
    assert_eq!(a.bond(), pot / 10, "a tenth of the pot at 1000 basis points");
    assert_eq!(Position { bond_bps: 1, ..a }.bond(), (pot as i128 / 10_000) as i64);
    let p = [a.x.legs[0].entry, a.x.legs[1].entry];
    let t = a.true_claim(SIDE_A, 6_500, p).expect("a close at the entries");
    assert!(t.is_true());
    let lie = a.claim(SIDE_A, 6_500, p, t.amount + 1).unwrap();
    assert!(!lie.is_true());
    assert_ne!(t.hash(), lie.hash());
    assert!(a.claim(SIDE_A, 6_500, p, pot + 1).is_none() && a.claim(3, 6_500, p, 0).is_none() && a.claim(SIDE_A, -1, p, 0).is_none());
    // In time or late.
    assert!(t.in_time(6_500 + a.x.t.late_daa as u64) && !t.in_time(6_501 + a.x.t.late_daa as u64));
    // An undisputed claim: the claimer its amount and its bond, the other the rest.
    let v = pot + t.bond;
    assert_eq!(t.settle_pay(v), (t.amount + t.bond, pot - t.amount));
    let tb = a.true_claim(SIDE_B, 6_500, p).unwrap();
    assert_eq!(tb.settle_pay(v), (pot - tb.amount, tb.amount + tb.bond));
    // A disputed claim. Proved: the claimer has its bond and half of the disputer's, the other half stays in the
    // script's copy. Refuted: the disputer its bond, whoever refuted half of the claimer's, the other half in the
    // copy. Neither within the dispute period: each its bond back. The position is whole in the last two.
    let d = t.disputed([9; 32]);
    assert_ne!(d.hash(), t.hash());
    assert_ne!(t.disputed([8; 32]).hash(), d.hash());
    let v2 = v + t.bond;
    let kept = t.bond - t.bond / 2;
    assert_eq!(d.prove_pay(v2), (t.amount + 2 * t.bond - kept, pot - t.amount, kept));
    assert_eq!(d.refute_pay(v2), Some((pot, t.bond, t.bond / 2, kept)));
    assert_eq!(d.lapse_pay(v2), Some((pot, t.bond, t.bond)));
    assert_eq!((d.other_key(), tb.disputed([9; 32]).other_key()), (a.x.b_key, a.x.a_key));
    assert_eq!((d.claimer_key(), tb.disputed([9; 32]).claimer_key()), (a.x.a_key, a.x.b_key));
    // A claim states its claimer's own prices and an amount. It is within the rule where the committee's prices give
    // its side at least that amount: at its own prices (the tolerance is counted against it there), at prices better
    // for it, and not at prices worse for it by more than the tolerance.
    let g = a.x.t.g_bps;
    assert!(t.is_true() && t.within(t.a));
    // (Which way is better for A is each leg's direction.)
    let sign: Vec<i64> = (0..2)
        .map(|i| {
            let mv = |bps: i64| {
                let mut q = t.a;
                q[i] += q[i] * bps / 10_000;
                a.x.owed_at(SIDE_A, q)
            };
            if mv(100) >= mv(-100) { 1 } else { -1 }
        })
        .collect();
    let toward = |bps: i64| [t.a[0] + sign[0] * t.a[0] * bps / 10_000, t.a[1] + sign[1] * t.a[1] * bps / 10_000];
    assert!(t.within(toward(-(g - 1))) && !t.within(toward(-(g + 2))), "a committee price worse for the claimer by more than g gives it less than it states");
    assert!(t.within(toward(g)) && t.within(toward(10 * g)), "a committee price better for the claimer proves the claim all the more");
    // A sompi more than the committee's prices give is refuted; any amount below is the claimer's own caution.
    let exact = a.x.owed_at(SIDE_A, t.a);
    assert!(a.claim(SIDE_A, t.t0, t.a, exact).unwrap().within(t.a));
    assert!(!a.claim(SIDE_A, t.t0, t.a, exact + 1).unwrap().within(t.a));
    assert!(a.claim(SIDE_A, t.t0, t.a, 0).unwrap().within(t.a));
    assert_eq!(d.lapse_pay(2 * t.bond - 1), None);
    // The three kinds of object are three scripts of one library.
    let lib = [0x4c; 32];
    let scripts = [TObj::Position(a).script(&lib), TObj::Claim(t).script(&lib), TObj::Disputed(d).script(&lib)];
    assert!(scripts[0] != scripts[1] && scripts[1] != scripts[2] && scripts[0] != scripts[2]);
    assert_eq!(pinned::kit().read(&scripts[1]), Some((lib, t.hash())));
}

#[test]
fn a_state_is_written_and_read_back_and_nothing_else_is_read() {
    let a = samples::position();
    let p = [a.x.legs[0].entry, a.x.legs[1].entry];
    let t = a.true_claim(SIDE_B, 6_500, p).unwrap();
    let d = a.claim(SIDE_A, 6_500, p, 1).unwrap().disputed([9; 32]);
    for o in [TObj::Position(a), TObj::Claim(t), TObj::Disputed(d)] {
        let b = o.encode();
        assert_eq!(TObj::decode(&b).unwrap(), o, "{}", o.kind());
        assert_eq!(TObj::decode(&b).unwrap().hash(), o.hash());
        // A byte more, a byte less, another kind byte.
        assert!(TObj::decode(&b[..b.len() - 1]).is_err() && TObj::decode(&[b.clone(), vec![0]].concat()).is_err());
        let mut k = b.clone();
        k[0] = 9;
        assert!(TObj::decode(&k).is_err());
    }
    assert!(TObj::decode(&[]).is_err());
    // A moving part whose lateDaa is not the terms', and a position the bounds refuse, are not read.
    let mut other = a;
    other.x.t.late_daa += 1;
    assert!(Position::from_parts(&a.fixed(), &other.moving()).is_err());
    assert!(Position::from_parts(&a.fixed(), &Position { dispute_daa: 0, ..a }.moving()).is_err());
    assert_eq!(Position::from_parts(&a.fixed(), &a.moving()).unwrap(), a);
    // A claim whose bond is not the position's is not read.
    let mut bytes = TObj::Claim(t).encode();
    let n = bytes.len();
    bytes[n - 8] ^= 1;
    assert!(TObj::decode(&bytes).is_err());
    // An opening's payload is read back by anybody.
    let pay = kpf_protocol::thin_build::opening_payload(&a);
    assert_eq!(kpf_protocol::thin_build::read_opening(&pay), Some(a));
    assert_eq!(kpf_protocol::thin_build::read_opening(&pay[1..]), None);
    assert_eq!(kpf_protocol::thin_build::read_opening(b"hello"), None);
}
