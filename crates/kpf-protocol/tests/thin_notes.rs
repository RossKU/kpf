//! Price notes and their citation (`KpfTNote`, `KpfNote`, `KpfTCite`; docs/ARCHITECTURE-CLEARING.md 3): the
//! committee's price for a market and a time is written once under the library's id beside its attest, and any
//! number of transactions hold, against the commitment of the chain block that accepted that writing, that it was
//! written. Every transaction is built, signed and run through `verify::verify`; the chain block is one made here
//! (what a node says of a real one is `probe-cite`'s to test).

use kaspa_consensus_core::tx::{CovenantBinding, TransactionId, TransactionOutpoint, TransactionOutput};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::committee::*;
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::thin::{Note, NOTE_VALUE, ORACLE_FEE};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
// (6 KAS: at 5 the genesis of a library of 63 scripts is above a block's storage mass.)
const FLOOR: i64 = 6 * KAS1;
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
    fn attest(&self, t: &OLive<EpochTable>, market: [u8; 32], price: i64) -> Attestation {
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, T0, price)).collect();
        Attestation::collect(&t.s, &self.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum")
    }
    /// The note of the market's price written, and its transaction.
    fn written(&self, owner: [u8; 32]) -> (OBuilt, OSigned, Note) {
        let funds = vec![fund(xk(&self.f), 30)];
        let table = self.epoch_table();
        let att = self.attest(&table, MARKET, PRICE);
        let (b, note) = note_write(&self.lib, &self.script("KpfTNote"), (&table, &att), owner, self.payer(&funds)).unwrap();
        let s = signed("note", &b, &self.keys);
        (b, s, note)
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
    let (g, lib) = lib_genesis_for(1, FLOOR as u64, Some(rid.as_bytes()), Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis", &g, &keys);
    let scripts = lib_scripts(&gs.tx, 6_000);
    assert_eq!(scripts.len(), kpf_protocol::thin_artifacts::SCRIPTS.len() + 1, "one copy of each, and a second of the citing script");
    assert_eq!(scripts.iter().filter(|s| s.s == "KpfTCite").count(), 2);
    assert!(lib_check(&gs.tx).is_ok());
    let fee_pool = lib_fee_pool(&gs.tx, 6_000).expect("a fee pool");
    World { f, members, keys, rid, table, lib, scripts, fee_pool }
}

/// A chain block that accepted the transaction `id` as the `at`-th of `n` of the native lane (after its coinbase),
/// with `lanes` steps above the lane's leaf: the witness, and the block's commitment.
fn block(id: Hash, at: usize, n: usize, lanes: usize, tag: u8) -> (cite::Witness, Hash) {
    let h = |k: usize| Hash::from_bytes(kpf_protocol::committee::blake2b(&[&[tag][..], &(k as u64).to_le_bytes()].concat()));
    let lane: Vec<(Hash, u16, u32)> = (0..n).map(|k| (if k == at { id } else { h(k) }, 1u16, k as u32 + 1)).collect();
    let (_, activity_path) = cite::merkle(&cite::activity_leaves(&lane), at);
    let w = cite::Witness {
        block: h(1_000),
        merge_idx: lane[at].2,
        activity_path,
        lane_key: cite::native_lane_key(),
        parent_ref: h(1_001),
        context: cite::context(1_700_000_000_000, 620_000_000, 610_000_000),
        blue_score: 610_000_000,
        lanes_path: (0..lanes).map(|k| (h(2_000 + k), k % 2 == 1)).collect(),
        inactivity_shortcut: h(1_002),
        payload_and_ctx: h(1_003),
        parent_commitment: h(1_004),
    };
    let c = w.commitment(&id, 1);
    (w, c)
}

fn lib_out(w: &World, value: u64, spk: kaspa_consensus_core::tx::ScriptPublicKey, by: u16) -> TransactionOutput {
    TransactionOutput { value, script_public_key: spk, covenant: Some(CovenantBinding { authorizing_input: by, covenant_id: Hash::from_bytes(w.lib) }) }
}

#[test]
fn the_committees_price_is_written_once_as_a_note_beside_its_attest() {
    let w = world();
    let owner = xk(&kp(44));
    let (b, s, note) = w.written(owner);
    assert_eq!(note, Note { committee: w.rid.as_bytes(), market: MARKET, t0: T0, price: PRICE, spread_bps: CommitteeParams::testnet().spread_bps, owner });
    assert_eq!(s.tx.inputs.len(), 3, "a coin, the script and the epoch table");
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key, s.tx.outputs[0].covenant.map(|c| c.covenant_id.as_bytes())), (NOTE_VALUE as u64, &note_spk(&note), Some(w.lib)));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTNote").value + ORACLE_FEE as u64, "the script's copy keeps the oracle fees");
    assert_eq!(s.tx.outputs[2].covenant.map(|c| c.covenant_id), Some(w.rid), "the epoch table goes on");

    // A note of another price, market, time, committee or spread than the attest's: refused.
    for (what, other) in [
        ("price", Note { price: PRICE + 1, ..note }),
        ("market", Note { market: [0xfd; 32], ..note }),
        ("time", Note { t0: T0 + 1, ..note }),
        ("committee", Note { committee: [0x53; 32], ..note }),
        ("spread", Note { spread_bps: note.spread_bps + 1, ..note }),
        ("writer", Note { owner: xk(&w.f), ..note }),
    ] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = note_spk(&other);
        assert!(!accepted(&t, &w.keys), "a note of another {what} than was attested and asked is written");
    }
    // A second note beside the first, or the note under no id: refused.
    let mut t = b.clone();
    t.tx.outputs.push(lib_out(&w, NOTE_VALUE as u64, note_spk(&Note { price: 2 * PRICE, ..note }), 1));
    assert!(!accepted(&t, &w.keys), "a second note is written by one attest");
    let mut t = b.clone();
    t.tx.outputs[0].covenant = None;
    assert!(!accepted(&t, &w.keys), "the note is not under the library's id");
    // Without the epoch table spent by its attest nothing is written: the script's `release` writes no note either.
    let funds = vec![fund(xk(&w.f), 31)];
    let table = w.epoch_table();
    let att = w.attest(&table, MARKET, PRICE);
    let mut lying = att.clone();
    lying.p = PRICE * 2;
    assert!(note_write(&w.lib, &w.script("KpfTNote"), (&table, &lying), owner, w.payer(&funds)).is_err(), "a price the members did not sign");
    assert!(note_write(&w.lib, &w.script("KpfTCite"), (&table, &att), owner, w.payer(&funds)).is_err(), "another script's copy");
}

#[test]
fn what_a_note_holds_returns_to_its_writer_and_its_spend_writes_nothing() {
    let w = world();
    let owner = xk(&kp(44));
    let (_, s, note) = w.written(owner);
    let live = OLive { s: note, value: s.tx.outputs[0].value, id: Some(Hash::from_bytes(w.lib)), op: TransactionOutpoint { transaction_id: s.tx.id(), index: 0 }, daa: 6_600 };
    let funds = vec![fund(xk(&w.f), 32)];
    let b = note_reclaim(&live, w.payer(&funds)).unwrap();
    let r = signed("note reclaimed", &b, &w.keys);
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (NOTE_VALUE as u64, &p2pk_spk(&owner)));
    // To another key: refused. And nothing is written under the library's id by a note's spend: not a note nobody
    // signed, not a script nobody pinned.
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = p2pk_spk(&xk(&w.f));
    assert!(!accepted(&t, &w.keys), "the note's value goes to another key");
    for spk in [note_spk(&Note { price: 2 * PRICE, ..note }), script_spk("KpfTCite").unwrap(), p2pk_spk(&owner)] {
        let mut t = b.clone();
        t.tx.outputs.push(lib_out(&w, KAS1 as u64, spk, 0));
        assert!(!accepted(&t, &w.keys), "a note's spend writes under the library's id");
    }
}

#[test]
fn a_note_is_cited_by_any_number_of_transactions_against_the_block_that_accepted_it() {
    let w = world();
    let owner = xk(&kp(44));
    let (_, s, note) = w.written(owner);
    // The chain block that accepted the note's transaction: the third of eleven of the lane, four steps above.
    let (wit, commitment) = block(s.tx.id(), 2, 11, 4, 1);
    kpf_protocol::verify::set_chain_view(vec![(wit.block, commitment)]);
    let c = Cited::of(&s.tx, &w.lib, note, wit.clone()).unwrap();
    assert_eq!(c.commitment(), commitment);
    let funds = vec![fund(xk(&w.f), 33)];
    let b = cite(&w.lib, &w.script("KpfTCite"), &c, w.payer(&funds)).unwrap();
    let sg = signed("citation alone", &b, &w.keys);
    assert_eq!(sg.tx.outputs[0].value, w.script("KpfTCite").value + ORACLE_FEE as u64);
    // What a reader finds at the head of the citing input's signature script: the block, then what the note says.
    let head = [&[0x20][..], &wit.block.as_bytes(), &note.served()].concat();
    assert_eq!(&sg.tx.inputs[0].signature_script[..head.len()], &head[..]);
    assert_eq!(head.len(), 33 + 33 + 33 + 9 + 9 + 9);
    // The note is not spent by it: a second transaction cites it beside the first (another coin, the copy as the
    // first left it).
    let mut again = w.script("KpfTCite");
    again.value = sg.tx.outputs[0].value;
    again.op = TransactionOutpoint { transaction_id: sg.tx.id(), index: 0 };
    let funds2 = vec![fund(xk(&w.f), 34)];
    signed("citation again", &cite(&w.lib, &again, &c, w.payer(&funds2)).unwrap(), &w.keys);

    // What the note does not say: refused by the script (the builder's own check is of the block alone).
    for (what, other) in [
        ("price", Note { price: PRICE + 1, ..note }),
        ("market", Note { market: [0xfd; 32], ..note }),
        ("time", Note { t0: T0 + 1, ..note }),
        ("committee", Note { committee: [0x53; 32], ..note }),
        ("spread", Note { spread_bps: note.spread_bps + 1, ..note }),
    ] {
        let lie = Cited { note: other, ..c.clone() };
        let r = cite(&w.lib, &w.script("KpfTCite"), &lie, w.payer(&funds));
        assert!(r.is_err() || !accepted(&r.unwrap(), &w.keys), "a citation says another {what} than the note's");
    }
    // A block the chain does not have, or a commitment that is not the block's: refused.
    let (other, _) = block(s.tx.id(), 2, 11, 4, 2);
    assert!(cite(&w.lib, &w.script("KpfTCite"), &Cited { witness: other.clone(), ..c.clone() }, w.payer(&funds)).is_err(), "a block that is not in the chain view");
    kpf_protocol::verify::set_chain_view(vec![(wit.block, commitment), (other.block, commitment)]);
    assert!(cite(&w.lib, &w.script("KpfTCite"), &Cited { witness: other, ..c.clone() }, w.payer(&funds)).is_err(), "the witness does not lead to the block's commitment");
    // Each part of the witness moved: the script does not reach the commitment (the builder is passed by).
    kpf_protocol::verify::set_chain_view(vec![(wit.block, commitment)]);
    let x = Hash::from_bytes([0x77; 32]);
    let mut moved: Vec<(&str, cite::Witness)> = vec![];
    let mut m = wit.clone();
    m.merge_idx += 1;
    moved.push(("place", m));
    let mut m = wit.clone();
    m.activity_path[1].0 = x;
    moved.push(("activity sibling", m));
    let mut m = wit.clone();
    m.activity_path[0].1 = !m.activity_path[0].1;
    moved.push(("activity side", m));
    let mut m = wit.clone();
    m.lanes_path[2].0 = x;
    moved.push(("lanes sibling", m));
    let mut m = wit.clone();
    m.blue_score += 1;
    moved.push(("blue score", m));
    for (name, f) in [("parent ref", 0), ("lane key", 1), ("context", 2), ("inactivity", 3), ("payload and context", 4), ("parent commitment", 5)] {
        let mut m = wit.clone();
        *[&mut m.parent_ref, &mut m.lane_key, &mut m.context, &mut m.inactivity_shortcut, &mut m.payload_and_ctx, &mut m.parent_commitment][f] = x;
        moved.push((name, m));
    }
    for (name, m) in moved {
        let mut t = b.clone();
        let lie = Cited { witness: m, ..c.clone() };
        // (The view is told the lie's commitment for its own block id is the true one, so that only the script judges.)
        let mut d = cite_unchecked(&w, &lie, &funds);
        std::mem::swap(&mut t, &mut d);
        assert!(!accepted(&t, &w.keys), "a citation with another {name} is accepted");
    }
}

/// The citation built with the builder's own check of the block passed by: the view is set to what the witness leads
/// to while it is built, and back to the truth before it is run.
fn cite_unchecked(w: &World, c: &Cited, funds: &[KeyUtxo]) -> OBuilt {
    let truth = kpf_protocol::verify::chain_commitment(&c.witness.block).expect("the block");
    kpf_protocol::verify::set_chain_view(vec![(c.witness.block, c.commitment())]);
    let b = cite(&w.lib, &w.script("KpfTCite"), c, w.payer(funds));
    kpf_protocol::verify::set_chain_view(vec![(c.witness.block, truth)]);
    b.expect("built against its own commitment")
}

#[test]
fn only_a_notes_writing_is_cited() {
    let w = world();
    let owner = xk(&kp(44));
    let (_, s, note) = w.written(owner);
    let funds = vec![fund(xk(&w.f), 33)];
    // The same script at the first output of a transaction, under no id or under another: anyone writes that. The
    // citation asks for the library's id there.
    for cov in [None, Some(Hash::from_bytes([0x99; 32]))] {
        let mut forged = s.tx.clone();
        forged.outputs[0].covenant = cov.map(|covenant_id| CovenantBinding { authorizing_input: 1, covenant_id });
        forged.finalize();
        let (wit, commitment) = block(forged.id(), 0, 3, 2, 3);
        kpf_protocol::verify::set_chain_view(vec![(wit.block, commitment)]);
        assert!(Cited::of(&forged, &w.lib, note, wit.clone()).is_err());
        let lie = Cited { note, id: forged.id(), rest: kaspa_consensus_core::hashing::tx::transaction_v1_rest_preimage(&forged), inputs: forged.inputs.len(), payload_digest: kaspa_consensus_core::hashing::tx::payload_digest(&forged.payload), witness: wit };
        let r = cite(&w.lib, &w.script("KpfTCite"), &lie, w.payer(&funds));
        assert!(r.is_err() || !accepted(&r.unwrap(), &w.keys), "a note under {cov:?} is cited");
    }
    // The note's script at the second output (the first is anyone's): refused. And an input count that moves where
    // the outputs are read: refused.
    let mut forged = s.tx.clone();
    forged.outputs.swap(0, 3);
    forged.finalize();
    let (wit, commitment) = block(forged.id(), 0, 3, 2, 4);
    kpf_protocol::verify::set_chain_view(vec![(wit.block, commitment)]);
    let lie = Cited { note, id: forged.id(), rest: kaspa_consensus_core::hashing::tx::transaction_v1_rest_preimage(&forged), inputs: forged.inputs.len(), payload_digest: kaspa_consensus_core::hashing::tx::payload_digest(&forged.payload), witness: wit };
    let r = cite(&w.lib, &w.script("KpfTCite"), &lie, w.payer(&funds));
    assert!(r.is_err() || !accepted(&r.unwrap(), &w.keys), "a note that is not the first output is cited");
    let (wit, commitment) = block(s.tx.id(), 0, 3, 2, 5);
    kpf_protocol::verify::set_chain_view(vec![(wit.block, commitment)]);
    let good = Cited::of(&s.tx, &w.lib, note, wit).unwrap();
    for n in [2usize, 4] {
        let r = cite(&w.lib, &w.script("KpfTCite"), &Cited { inputs: n, ..good.clone() }, w.payer(&funds));
        assert!(r.is_err() || !accepted(&r.unwrap(), &w.keys), "the transaction is read as one of {n} inputs");
    }
    // A citation writes nothing under the library's id but its own copy.
    let b = cite(&w.lib, &w.script("KpfTCite"), &good, w.payer(&funds)).unwrap();
    let mut t = b.clone();
    t.tx.outputs.push(lib_out(&w, NOTE_VALUE as u64, note_spk(&Note { price: 2 * PRICE, ..note }), 0));
    assert!(!accepted(&t, &w.keys), "a citation writes a note");
}

#[test]
fn the_new_scripts_copies_let_go_beside_the_sweep_as_every_copy_does() {
    let w = world();
    let funds = vec![fund(xk(&w.f), 30)];
    for name in ["KpfTNote", "KpfTCite"] {
        let mut copy = w.script(name);
        copy.value += (250 * ORACLE_FEE) as u64;
        let (b, moved, _) = sweep(&w.lib, &w.fee_pool, &w.script("KpfTSweep"), &copy, w.payer(&funds)).unwrap();
        assert_eq!(moved, 50_000_000);
        signed(&format!("sweep of {name}"), &b, &w.keys);
    }
}
