//! The oracle fees of the thin family (`KpfTSweep`, `KpfTPay`, `KpfFeePool`): every operation leaves a fixed amount in the
//! copy of the script it used, anyone moves what gathered there to the library's fee pool, and anyone pays the fee pool to
//! the members of the committee it is for, by weight. Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::committee::*;
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::thin::{oracle_fee_shares, swept, ORACLE_FEE, ORACLE_FEE_CUT, SWEEP_MIN};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
/// What the genesis gives each copy and the fee pool, and so what each keeps.
// (11 KAS: at 10 the genesis of a library of 63 scripts in two copies is above a block's storage mass.)
const FLOOR: i64 = 11 * KAS1;

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
fn accepted(b: &OBuilt, keys: &KeySet) -> bool {
    let sg = b.sign(keys).expect("signed");
    kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok()
}
/// Built, signed, accepted by the engine, and with a sompi less at any output but the change refused.
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    for k in 0..b.tx.outputs.len() {
        if Some(k) == b.change {
            continue;
        }
        let mut t = b.clone();
        t.tx.outputs[k].value -= 1;
        assert!(!accepted(&t, keys), "{what}: a sompi less at output {k} ({}) is accepted", b.output_labels[k]);
    }
    println!("{what}: {} bytes, fee {} sompi", sg.mass.size, sg.fee);
    sg
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

struct World {
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    table: Table,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    fee_pool: FeePool,
}

impl World {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(&self.f), rate: MIN_FEE_RATE }
    }
    fn epoch_table(&self, rid: Hash) -> OLive<EpochTable> {
        let rp = CommitteeParams::testnet();
        OLive { s: EpochTable { members: self.table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 }
    }
    /// The three members' signatures for a signed price of some market of the committee `rid`.
    fn attest(&self, t: &OLive<EpochTable>, rid: Hash) -> Attestation {
        let market = [0xfe; 32];
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &rid.as_bytes(), &market, T0, 1_000_000_000)).collect();
        Attestation::collect(&t.s, &rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum")
    }
}

/// A library of two copies of each script with the fee pool of a committee of three members (30, 22 and 15 KAS).
fn world() -> World {
    let f = kp(9);
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    keys.add_keypair(f);
    let rid = Hash::from_bytes([0x52; 32]);
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let funds = vec![fund(xk(&f), 200), fund(xk(&f), 202)];
    let (g, lib) = lib_genesis_for(2, FLOOR as u64, Some(rid.as_bytes()), Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis with a fee pool", &g, &keys);
    assert_eq!(lib_check(&gs.tx).unwrap(), lib);
    assert_eq!(genesis_fee_pool(&gs.tx), Some(FeePoolOf { committee: rid.as_bytes(), floor: FLOOR }));
    let scripts = lib_scripts(&gs.tx, 6_000);
    assert_eq!(scripts.len(), 2 * kpf_protocol::thin_artifacts::SCRIPTS.len());
    let fee_pool = lib_fee_pool(&gs.tx, 6_000).expect("a fee pool");
    assert_eq!((fee_pool.value, fee_pool.s, fee_pool.id.map(|h| h.as_bytes())), (FLOOR as u64, FeePoolOf { committee: rid.as_bytes(), floor: FLOOR }, Some(lib)));
    World { f, members, keys, rid, table, lib, scripts, fee_pool }
}

#[test]
fn a_library_is_one_whose_genesis_wrote_its_scripts_and_at_most_the_fee_pool_its_payload_names() {
    let w = world();
    let funds = vec![fund(xk(&w.f), 200)];
    let (g, _) = lib_genesis_for(1, FLOOR as u64, Some(w.rid.as_bytes()), w.payer(&funds)).unwrap();
    assert!(lib_check(&g.tx).is_ok());
    // Without a fee pool: a library still, whose oracle fees nobody collects.
    let (bare, _) = lib_genesis(1, FLOOR as u64, w.payer(&funds)).unwrap();
    assert!(lib_check(&bare.tx).is_ok() && lib_fee_pool(&bare.tx, 0).is_none());
    // A payload that names another committee than the fee pool is for: the output is no fee pool of the committee named, so it
    // is an output under the id that is not one of the family's.
    let mut t = g.clone();
    t.tx.payload = [GENESIS_MAGIC.to_vec(), vec![0x53; 32], kpf_protocol::state::sm8(FLOOR).to_vec()].concat();
    assert!(lib_check(&t.tx).unwrap_err().to_string().contains("REFUSED"));
    // Or another floor than the fee pool's state holds.
    let mut t = g.clone();
    t.tx.payload = [GENESIS_MAGIC.to_vec(), w.rid.as_bytes().to_vec(), kpf_protocol::state::sm8(FLOOR - 1).to_vec()].concat();
    assert!(lib_check(&t.tx).unwrap_err().to_string().contains("REFUSED"));
    // No payload: the same.
    let mut t = g.clone();
    t.tx.payload = vec![];
    assert!(lib_check(&t.tx).unwrap_err().to_string().contains("REFUSED"));
    // Two fee pools.
    let mut t = g.clone();
    assert_eq!(t.tx.outputs[0].script_public_key, p2pk_spk(&xk(&w.f)), "the change is the first output");
    let fee_pool_out = t.tx.outputs.last().cloned().unwrap();
    assert_eq!(fee_pool_out.script_public_key, fee_pool_spk(&w.fee_pool.s), "the fee pool is the last");
    t.tx.outputs.push(fee_pool_out);
    assert!(lib_check(&t.tx).unwrap_err().to_string().contains("REFUSED"));
    // The fee pool's address is the same in every library and names its committee and its floor.
    assert_ne!(fee_pool_spk(&w.fee_pool.s), fee_pool_spk(&FeePoolOf { committee: [0x53; 32], floor: FLOOR }));
    assert_ne!(fee_pool_spk(&w.fee_pool.s), fee_pool_spk(&FeePoolOf { committee: w.rid.as_bytes(), floor: FLOOR + 1 }));
}

/// A copy as it stands after `uses` operations left their oracle fees in it.
fn used(c: &LibScript, uses: i64) -> LibScript {
    let mut c = c.clone();
    c.value += (uses * ORACLE_FEE) as u64;
    c
}

#[test]
fn what_gathered_in_a_copy_is_swept_to_the_fee_pool_less_the_senders_part() {
    let w = world();
    let funds = vec![fund(xk(&w.f), 30)];
    // 250 operations left 0.5 KAS in a copy of KpfTAdd.
    let copy = used(&w.script("KpfTAdd"), 250);
    let (b, moved, to_fee_pool) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &copy, w.payer(&funds)).unwrap();
    assert_eq!((moved, to_fee_pool), (50_000_000, 47_500_000));
    assert_eq!(swept(copy.value as i64, FLOOR), Some((moved, to_fee_pool)));
    let s = signed("sweep", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (w.fee_pool.value + to_fee_pool as u64, &fee_pool_spk(&w.fee_pool.s)));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTSweep").value, "the script that collects leaves no oracle fees in its own copy");
    assert_eq!((s.tx.outputs[2].value, &s.tx.outputs[2].script_public_key), (FLOOR as u64, &script_spk("KpfTAdd").unwrap()));
    for o in &s.tx.outputs[..3] {
        assert_eq!(o.covenant.map(|c| c.covenant_id.as_bytes()), Some(w.lib));
    }
    // The sender is left its part less the fee: more than it had.
    let change = s.tx.outputs[3].value as i64;
    assert_eq!(change, 1_000 * KAS1 + moved / ORACLE_FEE_CUT - s.fee as i64);
    assert!(change > 1_000 * KAS1, "a sweep of 0.5 KAS pays its sender: fee {}", s.fee);
    // A transaction of the library, read as one: nothing of a registry in it.
    let amounts: Vec<Option<u64>> = s.entries.iter().map(|e| Some(e.amount)).collect();
    let r = kpf_protocol::thin_read::read_tx(&w.lib, &s.tx, &amounts, &kpf_protocol::thin_read::Nothing).expect("read");
    assert_eq!((r.script, r.entry, r.spent.len(), r.made.len(), r.unread.len()), ("KpfTSweep", "sweep", 0, 0, 0));

    // The fee pool as it now stands takes the next copy.
    let fee_pool = nth(&s, 0, w.fee_pool.s, 6_010);
    let sweeper = nth(&s, 1, "KpfTSweep", 6_010);
    let copy2 = used(&w.script("KpfTMatch"), 100);
    let (b2, moved2, to2) = sweep(&w.lib, &fee_pool, &sweeper, &copy2, w.payer(&funds)).unwrap();
    assert_eq!((moved2, to2), (SWEEP_MIN, SWEEP_MIN - SWEEP_MIN / ORACLE_FEE_CUT));
    let s2 = signed("sweep of the least", &b2, &w.keys);
    assert_eq!(s2.tx.outputs[0].value, (FLOOR + to_fee_pool + to2) as u64);

    // Less than the least above the floor: the builder refuses, and so does the script (the copy's value and the
    // fee pool's part set a sompi's oracle fees lower by hand).
    let short = used(&w.script("KpfTMatch"), 99);
    assert!(sweep(&w.lib, &fee_pool, &sweeper, &short, w.payer(&funds)).unwrap_err().to_string().contains("nothing to sweep yet"));
    // A copy goes on at exactly its floor: a sweep that leaves it more (to sweep the rest in another block, for
    // the sender's part again) is refused (review T1b, F24).
    let mut t = b2.clone();
    t.tx.outputs[2].value += 1;
    assert!(!accepted(&t, &w.keys), "a copy left above its floor");
    let mut t = b2.clone();
    t.entries[2].amount -= 1;
    t.tx.outputs[0].value -= 1;
    assert!(!accepted(&t, &w.keys), "a sweep of less than the least is refused by the script");
    // The whole of what moved kept by the sender: refused (the fee pool's output is fixed from below; `signed` tried a
    // sompi). And a copy left below its floor, the fee pool given the difference: refused.
    let mut t = b2.clone();
    t.tx.outputs[2].value -= KAS1 as u64 / 2;
    t.tx.outputs[0].value += KAS1 as u64 / 2;
    assert!(!accepted(&t, &w.keys), "a copy keeps its floor");
    // The copies of the two scripts that collect are not swept: they take no oracle fees and have no way out.
    assert!(sweep(&w.lib, &fee_pool, &sweeper, &used(&w.script("KpfTPay"), 500), w.payer(&funds)).unwrap_err().to_string().contains("takes no oracle fees"));
    // A copy of another library, a fee pool of another library.
    let mut other = copy2.clone();
    other.id = Some(Hash::from_bytes([0x77; 32]));
    assert!(sweep(&w.lib, &fee_pool, &sweeper, &other, w.payer(&funds)).is_err());
    let mut alien = fee_pool.clone();
    alien.id = Some(Hash::from_bytes([0x77; 32]));
    assert!(sweep(&w.lib, &alien, &sweeper, &copy2, w.payer(&funds)).is_err());
}

#[test]
fn a_copy_lets_go_only_beside_the_sweep_that_names_it() {
    let w = world();
    let funds = vec![fund(xk(&w.f), 30)];
    let copy = used(&w.script("KpfTAdd"), 250);
    let (b, _, _) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &copy, w.payer(&funds)).unwrap();
    assert!(accepted(&b, &w.keys));
    // The copy's `release` names the input of the script that answers for it and the place it is named at. Naming
    // the fee pool, itself, the funder's coin, or another place: refused.
    use kpf_protocol::oracle_build::OPlan;
    use kpf_protocol::tx::Arg;
    use silverscript_abi::ArtifactValue;
    for (l, k) in [(0i64, 1i64), (2, 1), (3, 1), (1, 0), (1, 3), (-1, 1), (9, 1)] {
        let mut t = b.clone();
        let OPlan::Contract { args, .. } = &mut t.plans[2] else { panic!("the copy is a contract input") };
        *args = vec![Arg::V(ArtifactValue::Int(l)), Arg::V(ArtifactValue::Int(k))];
        let ok = t.sign(&w.keys).map(|sg| kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok()).unwrap_or(false);
        assert!(!ok, "release({l}, {k}) is accepted");
    }
    // Named at the third place as at the second: accepted (the sweep's arguments name the copy at both).
    let mut t = b.clone();
    let OPlan::Contract { args, .. } = &mut t.plans[2] else { panic!() };
    *args = vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(2))];
    assert!(accepted(&t, &w.keys));
    // The sweep's places: the copy named where the fee pool is, or two different inputs at the second and third.
    for who in [[2i64, 2, 2], [0, 0, 0], [0, 2, 3], [0, 3, 2], [0, 1, 1]] {
        let mut t = b.clone();
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!() };
        args[0] = Arg::V(ArtifactValue::Bytes(who.iter().flat_map(|x| kpf_protocol::state::sm8(*x)).collect()));
        let ok = t.sign(&w.keys).map(|sg| kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok()).unwrap_or(false);
        assert!(!ok, "sweep with the places {who:?} is accepted");
    }
    // The committee the fee pool is for is not the one the sweep is told: refused. Nor the floor: a sweep told a lower
    // one would leave the copy less than the library's genesis gave it.
    let mut t = b.clone();
    let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!() };
    args[3] = Arg::V(ArtifactValue::Bytes(vec![0x53; 32]));
    assert!(!accepted(&t, &w.keys));
    for floor in [FLOOR - 1, FLOOR + 1, 0, -FLOOR] {
        let mut t = b.clone();
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!() };
        args[4] = Arg::V(ArtifactValue::Int(floor));
        assert!(!accepted(&t, &w.keys), "a sweep told the floor {floor}");
    }
}

#[test]
fn the_fee_pool_is_paid_to_every_member_by_its_weight() {
    let w = world();
    let funds = vec![fund(xk(&w.f), 30)];
    // The fee pool after sweeps: 20 KAS above its floor.
    let mut fee_pool = w.fee_pool.clone();
    fee_pool.value += 20 * KAS1 as u64;
    let epoch_table = w.epoch_table(w.rid);
    let att = w.attest(&epoch_table, w.rid);
    let (b, shares) = pay(&w.lib, &fee_pool, &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    // 19 KAS shared (a twentieth is the sender's) by 30 : 22 : 15, in units of 0.01 KAS, rounded down.
    assert_eq!(shares, vec![(xk(&w.members[0]), 850_000_000), (xk(&w.members[1]), 623_000_000), (xk(&w.members[2]), 425_000_000)]);
    // What leaves the fee pool: the shares and the sender's twentieth. The 0.02 KAS the rounding left stay in it.
    assert_eq!(oracle_fee_shares(fee_pool.value as i64, FLOOR, &w.table, 67 * KAS1).unwrap(), (1_898_000_000 + KAS1, shares.clone()));
    let s = signed("pay", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), ((FLOOR + 2_000_000) as u64, &fee_pool_spk(&w.fee_pool.s)));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTPay").value);
    assert_eq!(s.tx.inputs.len(), 4, "the fee pool, the script, the epoch table and one coin");
    assert_eq!((s.tx.outputs[2].value, s.tx.outputs[2].covenant.map(|c| c.covenant_id)), (epoch_table.value, Some(w.rid)), "the epoch table goes on");
    for (i, (key, share)) in shares.iter().enumerate() {
        assert_eq!((s.tx.outputs[3 + i].value, &s.tx.outputs[3 + i].script_public_key), (*share as u64, &p2pk_spk(key)));
    }
    let paid: i64 = shares.iter().map(|x| x.1).sum();
    assert_eq!(s.tx.outputs[6].value as i64, 1_000 * KAS1 + KAS1 - s.fee as i64, "the sender has a twentieth of the pot, less the fee");
    assert!(paid <= 19 * KAS1 && paid > 19 * KAS1 - 3 * 1_000_000);

    // The table paid is that of an epoch that has ended: the transaction is locked to the epoch table's `until`, and the
    // same transaction said to be valid a block before it is refused by the script (a member taken when the pool is
    // full is paid nothing of it: it is in no ended epoch's table).
    assert_eq!(b.tx.lock_time, 9_000, "the epoch table's until");
    let mut t = b.clone();
    t.tx.lock_time = 8_999;
    assert!(!accepted(&t, &w.keys), "paid by the table of an epoch still running");
    let mut t = b.clone();
    t.tx.lock_time = 6_500;
    assert!(!accepted(&t, &w.keys), "paid at the signed price's own time");
    // The members of another order or another key: refused (each output is held to its member's key and share).
    let mut t = b.clone();
    t.tx.outputs.swap(3, 4);
    assert!(!accepted(&t, &w.keys), "two members' outputs changed places");
    let mut t = b.clone();
    t.tx.outputs[5].script_public_key = p2pk_spk(&xk(&w.f));
    assert!(!accepted(&t, &w.keys), "a member's share paid to the sender");
    // An epoch table of another committee: the builder refuses, and the script (the committee named is the fee pool's).
    let other = Hash::from_bytes([0x53; 32]);
    let alien = w.epoch_table(other);
    let alien_att = w.attest(&alien, other);
    assert!(pay(&w.lib, &fee_pool, &w.script("KpfTPay"), (&alien, &alien_att), w.payer(&funds)).unwrap_err().to_string().contains("not of the committee"));
    // A fee pool for that other committee, in this library, would be paid to it: what makes the fee pool the library's one is
    // its genesis (`lib_check`), not the script. The script holds the fee pool to the committee the epoch table is of.
    use kpf_protocol::oracle_build::OPlan;
    use kpf_protocol::tx::Arg;
    use silverscript_abi::ArtifactValue;
    let mut t = b.clone();
    let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!() };
    args[3] = Arg::V(ArtifactValue::Bytes(other.as_bytes().to_vec()));
    assert!(!accepted(&t, &w.keys), "the committee told is not the fee pool's");
    // The coin is of the key the script is told: another key's coin in its place, or a second coin, is refused
    // (review T1b, F22: the transaction is the fee pool, the script, the epoch table and one coin, so that no other
    // script's output answers a member's share).
    let mut t = b.clone();
    let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!() };
    let n = args.len();
    args[n - 1] = Arg::V(ArtifactValue::Bytes(xk(&w.members[0]).to_vec()));
    assert!(!accepted(&t, &w.keys), "the coin is not of the key told");
    let two = vec![fund(xk(&w.f), 30), fund(xk(&w.f), 31)];
    assert!(pay(&w.lib, &fee_pool, &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&two)).unwrap_err().to_string().contains("one coin"));
    let mut t = b.clone();
    t.tx.inputs.push(kaspa_consensus_core::tx::TransactionInput::new_with_compute_budget(op(31), vec![], 0, 0));
    t.entries.push(kaspa_consensus_core::tx::UtxoEntry::new(1_000 * KAS1 as u64, p2pk_spk(&xk(&w.f)), 0, false, None));
    t.plans.push(b.plans[3].clone());
    t.input_labels.push("funding".into());
    assert!(!accepted(&t, &w.keys), "a fifth input");
    // The fee pool told a lower floor than its state holds (more to share): refused.
    let mut t = b.clone();
    let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!() };
    args[4] = Arg::V(ArtifactValue::Int(FLOOR - KAS1));
    assert!(!accepted(&t, &w.keys));

    // Too little for the smallest member's share to be an output: nobody is paid yet.
    let mut small = w.fee_pool.clone();
    small.value += 5_000_000;
    assert!(pay(&w.lib, &small, &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap_err().to_string().contains("cannot be mined"));
    // Nothing above the floor.
    assert!(pay(&w.lib, &w.fee_pool, &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap_err().to_string().contains("nothing to share"));
}

#[test]
fn fifteen_members_are_paid_in_one_transaction_and_an_empty_member_is_passed_over() {
    let mut w = world();
    let funds = vec![fund(xk(&w.f), 30)];
    // Fourteen members of 10 to 23 KAS; the fifth is empty.
    let keys: Vec<Keypair> = (0..15).map(|i| kp(110 + i as u8)).collect();
    let mut total = 0;
    for i in 0..15 {
        let weight = if i == 4 { 0 } else { (10 + i as i64) * KAS1 };
        total += weight;
        w.table[i] = Member { key: xk(&keys[i]), weight };
    }
    let rp = CommitteeParams::testnet();
    let epoch_table = OLive { s: EpochTable { members: w.table, total, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(w.rid), op: op(20), daa: 6_000 };
    let market = [0xfe; 32];
    let signed_prices: Vec<SignedPrice> = keys.iter().enumerate().filter(|(i, _)| *i != 4).map(|(_, k)| sign_price(k, &w.rid.as_bytes(), &market, T0, 1_000_000_000)).collect();
    let att = Attestation::collect(&epoch_table.s, &w.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
    let mut fee_pool = w.fee_pool.clone();
    fee_pool.value += 400 * KAS1 as u64;
    let (b, shares) = pay(&w.lib, &fee_pool, &w.script("KpfTPay"), (&epoch_table, &att), w.payer(&funds)).unwrap();
    assert_eq!(shares.len(), 14);
    assert!(shares.iter().all(|(k, _)| *k != xk(&keys[4])));
    let s = signed("pay to fourteen members", &b, &w.keys);
    assert_eq!(s.tx.outputs.len(), 3 + 14 + 1);
    let paid: i64 = shares.iter().map(|x| x.1).sum();
    assert!(paid <= 380 * KAS1 && paid > 380 * KAS1 - 14 * 1_000_000, "{paid}");
    // Each by its weight, to 0.01 KAS.
    for (i, (_, share)) in shares.iter().enumerate() {
        let weight = w.table[if i < 4 { i } else { i + 1 }].weight;
        let exact = 380.0 * KAS1 as f64 * weight as f64 / total as f64;
        assert!((exact - *share as f64).abs() <= 1_000_000.0, "member {i}: {share} against {exact}");
    }
}

/// Two sweeps in one transaction, each of its own copy, into the one fee pool: each asks the fee pool to have grown by
/// what its copy gave, and one growth would answer both, so whoever sends would keep the smaller copy's oracle fees whole.
/// A sweep is therefore alone in its transaction with the fee pool and its copy (three inputs under the library's id).
#[test]
fn two_sweeps_do_not_share_the_fee_pools_growth() {
    use kaspa_consensus_core::tx::{CovenantBinding, TransactionInput, TransactionOutput, UtxoEntry};
    use kaspa_txscript::pay_to_script_hash_script;
    use kpf_protocol::oracle_build::{OPlan, OWhich};
    use kpf_protocol::tx::Arg;
    use silverscript_abi::ArtifactValue;
    let w = world();
    let funds = vec![fund(xk(&w.f), 30)];
    let first = used(&w.script("KpfTAdd"), 250);
    let (b, _, _) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &first, w.payer(&funds)).unwrap();
    assert!(accepted(&b, &w.keys));
    // The second copy of KpfTSweep at input 4, naming the fee pool (input 0) and a copy of KpfTMatch at input 5 that
    // holds as much; both go on at their own indices, and the fee pool's output is what the first sweep alone asks.
    let second = w.scripts.iter().filter(|s| s.s == "KpfTSweep").nth(1).cloned().expect("two copies");
    let other = used(&w.script("KpfTMatch"), 250);
    let lid = Hash::from_bytes(w.lib);
    let kit = kpf_protocol::thin_artifacts::kit();
    let mut t = b.clone();
    let who: Vec<u8> = [0i64, 5, 5].iter().flat_map(|x| kpf_protocol::state::sm8(*x)).collect();
    let v = |x: ArtifactValue| Arg::V(x);
    let sweep_args = vec![v(ArtifactValue::Bytes(who)), v(ArtifactValue::Bytes(kpf_protocol::state::sm8(0).to_vec())), v(ArtifactValue::Int(w.fee_pool.s.script().len() as i64)), v(ArtifactValue::Bytes(w.rid.as_bytes().to_vec())), v(ArtifactValue::Int(FLOOR))];
    for (live, name, entry, args) in [(&second, "KpfTSweep", "sweep", sweep_args), (&other, "KpfTMatch", "release", vec![v(ArtifactValue::Int(4)), v(ArtifactValue::Int(1))])] {
        let code = kit.script(name).unwrap().to_vec();
        t.tx.inputs.push(TransactionInput::new_with_compute_budget(live.op, vec![], 0, 0));
        t.entries.push(UtxoEntry::new(live.value, pay_to_script_hash_script(&code), live.daa, false, live.id));
        t.plans.push(OPlan::Contract { which: OWhich::T(name), redeem: code, entry, args });
        t.input_labels.push(name.into());
    }
    for (i, (name, value)) in [("KpfTSweep", second.value), ("KpfTMatch", FLOOR as u64)].into_iter().enumerate() {
        t.tx.outputs.push(TransactionOutput { value, script_public_key: script_spk(name).unwrap(), covenant: Some(CovenantBinding { authorizing_input: 4 + i as u16, covenant_id: lid }) });
        t.output_labels.push(name.into());
    }
    assert_eq!((t.tx.inputs.len(), t.tx.outputs.len()), (6, 6));
    assert!(!accepted(&t, &w.keys), "two sweeps were answered by one growth of the fee pool: the sender kept a copy's oracle fees whole");
    // Nor does a sweep ride beside another operation's script, or a payment beside a sweep: the fee pool and what is
    // named are all that is under the library's id in the transaction.
    let mut t = b.clone();
    let code = kit.script("KpfTSweep").unwrap().to_vec();
    t.tx.inputs.push(TransactionInput::new_with_compute_budget(second.op, vec![], 0, 0));
    t.entries.push(UtxoEntry::new(second.value, pay_to_script_hash_script(&code), second.daa, false, second.id));
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|x| kpf_protocol::state::sm8(*x)).collect();
    t.plans.push(OPlan::Contract { which: OWhich::T("KpfTSweep"), redeem: code, entry: "sweep", args: vec![v(ArtifactValue::Bytes(who)), v(ArtifactValue::Bytes(kpf_protocol::state::sm8(0).to_vec())), v(ArtifactValue::Int(w.fee_pool.s.script().len() as i64)), v(ArtifactValue::Bytes(w.rid.as_bytes().to_vec())), v(ArtifactValue::Int(FLOOR))] });
    t.input_labels.push("KpfTSweep".into());
    t.tx.outputs.push(TransactionOutput { value: second.value, script_public_key: script_spk("KpfTSweep").unwrap(), covenant: Some(CovenantBinding { authorizing_input: 4, covenant_id: lid }) });
    t.output_labels.push("KpfTSweep".into());
    assert!(!accepted(&t, &w.keys), "a second sweep script naming the same fee pool and copy");
}
