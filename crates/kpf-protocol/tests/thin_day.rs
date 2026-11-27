//! The committee's note of a day (`KpfTDay`; docs/ARCHITECTURE-CLEARING.md 17.8g, A): the committee's price for a
//! market and a time written beside its attest as an object that stays, in the fee pool's form, with no owner.
//! Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{CovenantBinding, TransactionId, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::committee::*;
use kpf_protocol::oracle_build::{OBuilt, OLive, OPlan, OSigned, OWhich};
use kpf_protocol::thin::{Day, Note, DAY_MIN, NOTE_VALUE, ORACLE_FEE};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{Arg, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use silverscript_abi::ArtifactValue;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const MARKET: [u8; 32] = [0xfe; 32];
const PRICE: i64 = 1_000_000_000;

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
    match b.sign(keys) {
        Ok(sg) => kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(),
        Err(_) => false,
    }
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
    println!("{what}: {} bytes, fee {} sompi, script units {:?}", sg.mass.size, sg.fee, sg.units);
    sg
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
    fn epoch_table(&self) -> OLive<EpochTable> {
        let rp = CommitteeParams::testnet();
        OLive { s: EpochTable { members: self.table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(self.rid), op: op(20), daa: 6_000 }
    }
    fn attest_at(&self, t: &OLive<EpochTable>, market: [u8; 32], t0: i64, price: i64) -> Option<Attestation> {
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, t0, price)).collect();
        Attestation::collect(&t.s, &self.rid.as_bytes(), &market, t0, &signed_prices).ok()
    }
    fn attest(&self, t: &OLive<EpochTable>, market: [u8; 32], price: i64) -> Attestation {
        self.attest_at(t, market, T0, price).expect("a quorum")
    }
    /// The day's note of the market's price written, and its transaction.
    fn written(&self) -> (OBuilt, OSigned, Day) {
        let funds = vec![fund(xk(&self.f), 30)];
        let table = self.epoch_table();
        let att = self.attest(&table, MARKET, PRICE);
        let (b, day) = day_write(&self.lib, &self.script("KpfTDay"), (&table, &att), self.payer(&funds)).unwrap();
        let s = signed("day's note", &b, &self.keys);
        (b, s, day)
    }
}

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
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis_for(1, lib_copy_least(1), Some(rid.as_bytes()), Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis", &g, &keys);
    let scripts = lib_scripts(&gs.tx, 6_000);
    assert!(lib_check(&gs.tx).is_ok());
    let fee_pool = lib_fee_pool(&gs.tx, 6_000).expect("a fee pool");
    World { f, members, keys, rid, table, lib, scripts, fee_pool }
}

fn lib_out(w: &World, value: u64, spk: kaspa_consensus_core::tx::ScriptPublicKey, by: u16) -> TransactionOutput {
    TransactionOutput { value, script_public_key: spk, covenant: Some(CovenantBinding { authorizing_input: by, covenant_id: Hash::from_bytes(w.lib) }) }
}

#[test]
fn a_days_price_is_written_beside_the_committees_attest_as_a_note_that_stays() {
    let w = world();
    let (b, s, day) = w.written();
    assert_eq!(day, Day { committee: w.rid.as_bytes(), market: MARKET, day: T0, price: PRICE, spread_bps: CommitteeParams::testnet().spread_bps });
    assert_eq!(s.tx.inputs.len(), 3, "a coin, the script and the epoch table");
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key, s.tx.outputs[0].covenant.map(|c| c.covenant_id.as_bytes())), (DAY_MIN as u64, &day_spk(&day), Some(w.lib)));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTDay").value + ORACLE_FEE as u64, "the script's copy keeps the oracle fees");
    assert_eq!(s.tx.outputs[2].covenant.map(|c| c.covenant_id), Some(w.rid), "the epoch table goes on");
    // The object is in the fee pool's form: the fee pool's script with the day's state where the fee pool's is, and
    // so another script than the library's fee pool. Its state is no price note's (the tag differs).
    let (pool, mine) = (w.fee_pool.s.script(), day_script(&day));
    assert_eq!(pool.len(), mine.len());
    let differ: Vec<usize> = (0..pool.len()).filter(|i| pool[*i] != mine[*i]).collect();
    assert!(!differ.is_empty() && differ.iter().all(|i| (2..34).contains(i)), "{differ:?}");
    assert_eq!(&mine[2..34], &day.hash()[..]);
    let as_note = Note { committee: day.committee, market: day.market, t0: day.day, price: day.price, spread_bps: day.spread_bps, owner: xk(&w.f) };
    assert_ne!(as_note.hash(), day.hash(), "a day's note and a price note of the same word have two states");

    // A note of another price, market, day, committee or spread than the attest's: refused.
    for (what, other) in [
        ("price", Day { price: PRICE + 1, ..day }),
        ("market", Day { market: [0xfd; 32], ..day }),
        ("day", Day { day: T0 + 1, ..day }),
        ("committee", Day { committee: [0x53; 32], ..day }),
        ("spread", Day { spread_bps: day.spread_bps + 1, ..day }),
    ] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = day_spk(&other);
        assert!(!accepted(&t, &w.keys), "a day's note of another {what} than was attested is written");
    }
    // A price note in its place (a note its writer takes back): refused.
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = note_spk(&as_note);
    assert!(!accepted(&t, &w.keys), "the day's script writes a price note");
    // A second note beside the first, the note under no id, the note where the script's copy is: refused.
    let mut t = b.clone();
    t.tx.outputs.push(lib_out(&w, DAY_MIN as u64, day_spk(&Day { price: 2 * PRICE, ..day }), 1));
    assert!(!accepted(&t, &w.keys), "a second note is written by one attest");
    let mut t = b.clone();
    t.tx.outputs.push(lib_out(&w, DAY_MIN as u64, day_spk(&day), 1));
    assert!(!accepted(&t, &w.keys), "the same note is written twice by one transaction");
    let mut t = b.clone();
    t.tx.outputs[0].covenant = None;
    assert!(!accepted(&t, &w.keys), "the note is not under the library's id");
    // Before the day nothing is written: the attest is of a time the chain has reached.
    let mut t = b.clone();
    t.tx.lock_time = (T0 - 1) as u64;
    assert!(!accepted(&t, &w.keys), "a day's note is written before its day");
    // More than the least may be put into it.
    let mut t = b.clone();
    t.tx.outputs[0].value += KAS1 as u64;
    t.tx.outputs[b.change.expect("change")].value -= KAS1 as u64;
    assert!(accepted(&t, &w.keys), "a day's note holds more than its least");

    // Without the epoch table spent by its attest nothing is written, and another script's copy writes none.
    let funds = vec![fund(xk(&w.f), 31)];
    let table = w.epoch_table();
    let att = w.attest(&table, MARKET, PRICE);
    let mut lying = att.clone();
    lying.p = PRICE * 2;
    assert!(day_write(&w.lib, &w.script("KpfTDay"), (&table, &lying), w.payer(&funds)).is_err(), "a price the members did not sign");
    assert!(day_write(&w.lib, &w.script("KpfTNote"), (&table, &att), w.payer(&funds)).is_err(), "another script's copy");
    // The price note's script writes no day's note.
    let (mut t, _) = note_write(&w.lib, &w.script("KpfTNote"), (&table, &att), xk(&w.f), w.payer(&funds)).unwrap();
    assert!(accepted(&t, &w.keys));
    t.tx.outputs[0].script_public_key = day_spk(&day);
    assert!(!accepted(&t, &w.keys), "the price note's script writes a day's note");
}

#[test]
fn the_signed_prices_of_a_price_note_write_a_days_note_while_a_table_of_the_day_lives() {
    let w = world();
    let table = w.epoch_table();
    let att = w.attest(&table, MARKET, PRICE);
    // The same attestation, twice: a price note, then (the table as it went on) a day's note.
    let f1 = vec![fund(xk(&w.f), 30)];
    let (b1, note) = note_write(&w.lib, &w.script("KpfTNote"), (&table, &att), xk(&w.f), w.payer(&f1)).unwrap();
    let s1 = signed("price note", &b1, &w.keys);
    let mut next = table.clone();
    next.op = TransactionOutpoint { transaction_id: s1.tx.id(), index: 2 };
    next.value = s1.tx.outputs[2].value;
    let f2 = vec![fund(xk(&w.f), 31)];
    let (b2, day) = day_write(&w.lib, &w.script("KpfTDay"), (&next, &att), w.payer(&f2)).unwrap();
    signed("day's note from the same signed prices", &b2, &w.keys);
    assert_eq!((note.price, note.t0, note.market), (day.price, day.day, day.market));
    assert_eq!((b1.tx.outputs[0].value, b2.tx.outputs[0].value), (NOTE_VALUE as u64, DAY_MIN as u64));
    // A time no table of the committee covers has no attest, and so no note (docs 17.8g: a day's price is attested
    // within the life of the tables of that time, or never).
    assert!(w.attest_at(&table, MARKET, 9_000, PRICE).is_none(), "the table's window ends before 9,000");
    assert!(w.attest_at(&table, MARKET, 8_999, PRICE).is_some());
}

#[test]
fn two_notes_of_one_day_may_differ_within_the_spread_and_a_price_of_nothing_is_no_days_price() {
    let w = world();
    let table = w.epoch_table();
    let funds = vec![fund(xk(&w.f), 30)];
    let att = w.attest(&table, MARKET, PRICE);
    let (b, day) = day_write(&w.lib, &w.script("KpfTDay"), (&table, &att), w.payer(&funds)).unwrap();
    signed("day's note", &b, &w.keys);
    // Review T11, D1: the attest takes its price from whoever builds it, each member's signed price within the
    // spread of it. The members signed PRICE: a price a spread above it is attested, and written as a second, true
    // note of the same day. (So a day's note is the committee's word and not the one price of a day.)
    let spread = CommitteeParams::testnet().spread_bps;
    let mut moved = att.clone();
    moved.p = PRICE + PRICE * spread / 10_000;
    let (b2, day2) = day_write(&w.lib, &w.script("KpfTDay"), (&table, &moved), w.payer(&funds)).expect("a price within the spread of what was signed");
    signed("a second note of the day, a spread above", &b2, &w.keys);
    assert_eq!((day2.day, day2.market, day2.committee), (day.day, day.market, day.committee));
    assert!(day2.price > day.price && day2.hash() != day.hash());
    let mut beyond = att.clone();
    // (Twice the spread: the attest measures the spread on the price named, so its edge is not the sompi after.)
    beyond.p = PRICE + 2 * PRICE * spread / 10_000;
    assert!(day_write(&w.lib, &w.script("KpfTDay"), (&table, &beyond), w.payer(&funds)).map(|(t, _)| !accepted(&t, &w.keys)).unwrap_or(true), "a price beyond the spread of what was signed");

    // A price of nothing is attested (the committee's attest asks no more than "not below nothing"), and the price
    // note's script writes it; the day's script does not. (The attest of nothing is put into the day's transaction
    // in the place of its own: both spend the same table at input 2.)
    let zero = w.attest(&table, MARKET, 0);
    let (z, zn) = note_write(&w.lib, &w.script("KpfTNote"), (&table, &zero), xk(&w.f), w.payer(&funds)).unwrap();
    signed("a price note of nothing", &z, &w.keys);
    assert_eq!(zn.price, 0);
    let swap = |from: &OBuilt, price: i64| -> OBuilt {
        let mut t = b.clone();
        t.plans[2] = from.plans[2].clone();
        t.tx.outputs[0].script_public_key = day_spk(&Day { price, ..day });
        t
    };
    assert!(!accepted(&swap(&z, 0), &w.keys), "a day's note of a price of nothing is written");
    // (The same exchange with an attest of a price above nothing is accepted: it is the price that is refused.)
    let other = w.attest(&table, MARKET, PRICE + 7);
    let (o, _) = note_write(&w.lib, &w.script("KpfTNote"), (&table, &other), xk(&w.f), w.payer(&funds)).unwrap();
    assert!(accepted(&swap(&o, PRICE + 7), &w.keys));
    assert!(!accepted(&swap(&o, PRICE), &w.keys), "the note of one price beside the attest of another");
}

#[test]
fn a_sweep_takes_what_a_days_note_holds_above_a_copys_floor_and_the_note_stays() {
    // Review T11, D5. `KpfTSweep` names a copy of a script at a place and asks that it goes on as it is holding the
    // floor; an object in the fee pool's form passes for a copy (its `go` asks only to be named). So what a day's
    // note holds above a copy's floor is the fee pool's by anyone's sweep; the note stays, of the same state. A
    // reader therefore asks of a day's note the value it had, never a least. A note holding its least is below the
    // floor and nothing of it moves.
    let w = world();
    let (_, s, day) = w.written();
    let floor = w.fee_pool.s.floor;
    assert!(DAY_MIN < floor, "a note's least is below what a copy keeps");
    let funds = vec![fund(xk(&w.f), 32)];
    let rich = (floor + KAS1 / 2) as u64;
    let swept = |value: u64| -> Option<OBuilt> {
        let mut copy = w.script("KpfTDay");
        copy.value = value;
        let (mut b, _, _) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &copy, w.payer(&funds)).ok()?;
        b.tx.inputs[2].previous_outpoint = TransactionOutpoint { transaction_id: s.tx.id(), index: 0 };
        b.entries[2] = UtxoEntry::new(value, day_spk(&day), 6_600, false, Some(Hash::from_bytes(w.lib)));
        b.plans[2] = OPlan::Contract { which: OWhich::T("KpfFeePool"), redeem: day_script(&day), entry: "go", args: vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(1))] };
        b.tx.outputs[2].script_public_key = day_spk(&day);
        Some(b)
    };
    assert!(swept(DAY_MIN as u64).is_none(), "nothing to sweep in a note at its least");
    let b = swept(rich).expect("a note that holds more than a copy's floor");
    let sg = b.sign(&w.keys).expect("signed");
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{:?}", v.errors());
    assert_eq!((sg.tx.outputs[2].value, &sg.tx.outputs[2].script_public_key), (floor as u64, &day_spk(&day)), "the note stays, at the floor");
    // The note cannot be made another's by the sweep, nor end.
    let mut t = b.clone();
    t.tx.outputs[2].script_public_key = day_spk(&Day { price: day.price + 1, ..day });
    assert!(!accepted(&t, &w.keys), "a sweep writes a day's note of another price");
    // A day's note is no fee pool: named where the sweep reads the fee pool, it is refused.
    let mut copy = w.script("KpfTDay");
    copy.value += (250 * ORACLE_FEE) as u64;
    let (good, _, _) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &copy, w.payer(&funds)).unwrap();
    signed("sweep of KpfTDay", &good, &w.keys);
    let mut t = good.clone();
    t.entries[0] = UtxoEntry::new(w.fee_pool.value, day_spk(&day), 6_600, false, Some(Hash::from_bytes(w.lib)));
    if let OPlan::Contract { redeem, .. } = &mut t.plans[0] {
        *redeem = day_script(&day);
    }
    t.tx.outputs[0].script_public_key = day_spk(&day);
    assert!(!accepted(&t, &w.keys), "a day's note is read as the fee pool");
}
