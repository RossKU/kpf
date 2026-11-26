//! The thin family through the tool's builders on the pinned scripts (`kpf_protocol::thin_build`): a library's
//! genesis, a position's opening, and the life of a claim, with a user (A), its counterparty (the executor B) and a
//! second executor that watches (W). Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kpf_protocol::cross::{SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::thin::{samples, Position};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;

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
    tight(what, b, keys);
    read_back(what, &sg);
    println!("{what}: {} bytes, fee {} sompi", sg.mass.size, sg.fee);
    sg
}

/// What a machine that follows the chain has read so far (one per test: a test is a thread).
#[derive(Default)]
struct Seen {
    objs: Vec<kpf_protocol::thin::TObj>,
    tpls: Vec<kpf_protocol::thin::Template>,
    /// The entries read whole, and those whose input was a position nobody published (shown by its hash only).
    whole: std::collections::BTreeSet<String>,
    blind: std::collections::BTreeSet<String>,
}

impl kpf_protocol::thin_read::Known for Seen {
    fn object(&self, state: &[u8; 32]) -> Option<kpf_protocol::thin::TObj> {
        self.objs.iter().find(|o| o.hash() == *state).copied()
    }
    fn fixed(&self, h: &[u8; 32]) -> Option<Vec<u8>> {
        use kpf_protocol::thin::TObj as O;
        self.objs.iter().find_map(|o| {
            let a = match o {
                O::Position(a) => *a,
                O::Claim(c) => c.position,
                O::Disputed(d) => d.claim.position,
                O::Liq(l) => l.liq.position,
                O::ExitClaim(q) => q.position,
                _ => return None,
            };
            (a.fixed_hash() == *h).then(|| a.fixed())
        })
    }
    fn template(&self, h: &[u8; 32]) -> Option<kpf_protocol::thin::Template> {
        self.tpls.iter().find(|t| t.hash() == *h).copied()
    }
}

thread_local! {
    static SEEN: std::cell::RefCell<Seen> = std::cell::RefCell::new(Seen::default());
}

/// A transaction of the family is read back from its bytes alone (`thin_read`), by a reader that knows only what it
/// read of earlier transactions: every object it spent is rebuilt to the hash its input carries, and every object it
/// made to the script of its output. Where a script is shown a position's fixed part by its hash and nobody published
/// that position, the reader says so (nothing read) and is wrong about nothing.
fn read_back(what: &str, sg: &OSigned) {
    use kpf_protocol::thin_read::{call_of, read_tx};
    let Some(c) = sg.tx.inputs.iter().find_map(|i| call_of(&i.signature_script)) else { return };
    let again = script_spk(c.script).unwrap();
    let lib = sg.tx.outputs.iter().find(|o| o.script_public_key == again).and_then(|o| o.covenant).map(|v| v.covenant_id.as_bytes()).unwrap_or_else(|| panic!("{what}: the script is not written again"));
    let amounts: Vec<Option<u64>> = sg.entries.iter().map(|e| Some(e.amount)).collect();
    let name = format!("{}.{}", c.script, c.entry);
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        let r = read_tx(&lib, &sg.tx, &amounts, &*seen).unwrap_or_else(|| panic!("{what}: not read as a transaction of the library"));
        assert_eq!((r.script, r.entry), (c.script, c.entry));
        assert!(!r.spent.is_empty(), "{what}: it spends an object");
        let shown_whole = c.bytes("st").is_some() || c.bytes("tplh").is_some() || c.bytes("tpl").is_some();
        if r.spent.iter().all(|(_, _, o)| o.is_some()) {
            // An opening that published nothing is the one transaction whose object the arguments do not give.
            if !(name == "KpfTOpen.open" && sg.tx.payload.is_empty() && !r.unread.is_empty()) {
                assert!(r.unread.is_empty(), "{what} ({name}): outputs {:?} are objects the reader did not rebuild", r.unread);
                seen.whole.insert(name);
            }
        } else {
            assert!(!shown_whole, "{what} ({name}): the arguments show the objects whole, and one was not rebuilt: {:?}", r.spent.iter().map(|(i, _, o)| (*i, o.is_some())).collect::<Vec<_>>());
            assert!(r.made.iter().all(|(_, o, _)| matches!(o, kpf_protocol::thin::TObj::Anchor)), "{what} ({name}): made without what was spent");
            seen.blind.insert(name);
        }
        for (_, _, o) in &r.spent {
            seen.objs.extend(o);
        }
        for (_, o, _) in &r.made {
            seen.objs.push(*o);
        }
        seen.tpls.extend(r.template);
    });
}

/// Every output a script fixes is fixed from below: the same transaction with a sompi less at any output but the
/// change and the fillers is refused by the engine (review T1, the lead it did not reach: what each entry pays).
fn tight(what: &str, b: &OBuilt, keys: &KeySet) {
    // Not an operation of the family (a genesis, an opening), or the one whose outputs are the two parties' to choose
    // (`KpfTBoth`: their signatures cover every output, and this check signs again with both keys).
    if !b.plans.iter().any(|p| p.name().starts_with("KpfSlot")) || b.plans.iter().any(|p| p.name().starts_with("KpfTBoth")) {
        return;
    }
    // An opening, likewise: both parties sign every output; the script asks the position's two margins (the test of the
    // registry holds it to them) and the builder adds the terms' carrier.
    if b.plans.iter().any(|p| matches!(p, kpf_protocol::oracle_build::OPlan::Contract { entry: "open", .. })) {
        return;
    }
    for k in 0..b.tx.outputs.len() {
        if Some(k) == b.change || b.output_labels[k] == "filler" {
            continue;
        }

        let mut t = b.clone();
        t.tx.outputs[k].value -= 1;
        let sg = t.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(!kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "{what}: a sompi less at output {k} ({}) is accepted", b.output_labels[k]);
    }
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

/// The id the hand-made orders and positions of these tests carry: one registry's.
const REGISTRY: [u8; 32] = [0xB0; 32];

struct World {
    /// An anchor of the library's registry: the way in.
    anchor: Anchor,
    a: Keypair,
    b: Keypair,
    w: Keypair,
    keys: KeySet,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    position: OLive<Position>,
    n: u8,
}

impl World {
    /// A fresh funding UTXO of 1,000 KAS of `k`.
    fn pay(&mut self, k: &Keypair) -> Vec<KeyUtxo> {
        self.n += 1;
        vec![fund(xk(k), self.n, 1_000)]
    }
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
}

/// A library of two copies, and a position of 2,000 + 3,000 KAS between A and B whose claims wait 50 DAA and bond a
/// tenth of the pot, opened with its state published.
fn world() -> World {
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let funds = vec![fund(xk(&f), 200, 3_000)];
    // (63 scripts in two copies at 10 KAS weigh 504,002 grams of storage mass, above a block's 500,000: 11 KAS a copy.)
    let (g, lib) = lib_genesis(2, lib_copy_least(2), Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis (11 scripts, 2 copies)", &g, &keys);
    let scripts = lib_scripts(&gs.tx, 6_000);
    assert_eq!(scripts.len(), 2 * kpf_protocol::thin_artifacts::SCRIPTS.len());
    assert!(scripts.iter().all(|s| s.id.map(|h| h.as_bytes()) == Some(lib) && s.value == lib_copy_least(2)));
    let mut x = samples::position().x;
    x.a_key = xk(&a);
    x.b_key = xk(&b);
    x.a_margin = 2_000 * KAS1;
    x.b_margin = 3_000 * KAS1;
    let position = Position { x, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 };
    let (af, bf) = (vec![fund(xk(&a), 201, 2_500)], vec![fund(xk(&b), 202, 3_100)]);
    // The registry: two anchors under one id. The position enters through one, signed by its two parties.
    let bfunds = vec![fund(xk(&f), 203, 1_000)];
    let (bg, registry) = registry_genesis(&lib, 2, KAS1 as u64, Payer { funds: &bfunds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let bs = signed("registry genesis (2 anchors)", &bg, &keys);
    assert_eq!(registry_check(&bs.tx, &lib).unwrap(), registry);
    let anchors = registry_anchors(&bs.tx, &lib, 6_000);
    assert_eq!(anchors.len(), 2);
    let place = scripts.iter().find(|s| s.s == "KpfTOpen").cloned().unwrap();
    let o = open(&lib, &anchors[0], &place, &position, &bf, &xk(&b), true, Payer { funds: &af, change_to: xk(&a), rate: MIN_FEE_RATE }).unwrap();
    let os = signed("opening (state published)", &o, &keys);
    assert_eq!(os.tx.payload, opening_payload(&position));
    assert_eq!(os.tx.outputs[0].value, position.x.floor() as u64);
    assert_eq!((os.tx.outputs[3].value, &os.tx.outputs[3].script_public_key), ((100 * KAS1) as u64, &p2pk_spk(&xk(&b))), "the counterparty's change");
    // The position is under the registry's id, and the anchor went on beside it.
    assert_eq!(os.tx.outputs[0].covenant.map(|c| c.covenant_id.as_bytes()), Some(registry));
    assert_eq!((os.tx.outputs[2].value, os.tx.outputs[2].covenant.map(|c| c.covenant_id.as_bytes())), (anchors[0].value, Some(registry)));
    let anchor = nth(&os, 2, (), 6_100);
    let live = nth(&os, 0, position, 6_100);
    World { anchor, a, b, w, keys, lib, scripts, position: live, n: 0 }
}

#[test]
fn a_true_claim_is_declared_and_settled_after_margin_was_added() {
    let mut w = world();
    let lib = w.lib;
    // A adds 50 KAS.
    let f = w.pay(&w.a.clone());
    let (b, next) = add(&lib, &w.position, &w.script("KpfTAdd"), SIDE_A, 50 * KAS1, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("add margin", &b, &w.keys);
    assert_eq!(next.x.a_margin, 2_050 * KAS1);
    let position = nth(&s, 0, next, 6_200);
    assert_eq!(position.value, w.position.value + (50 * KAS1) as u64);
    // The script went on at output 1, as it was.
    assert_eq!((s.tx.outputs[1].value, s.tx.outputs[1].script_public_key.clone()), (w.script("KpfTAdd").value + kpf_protocol::thin::ORACLE_FEE as u64, script_spk("KpfTAdd").unwrap()), "the copy goes on, the oracle fees in it");
    // A declares what the rule gives at the entries and posts the bond.
    let p = [next.x.legs[0].entry, next.x.legs[1].entry];
    let f = w.pay(&w.a.clone());
    let (b, claim) = close(&lib, &position, &w.script("KpfTClose"), SIDE_A, T0, p, None, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("close", &b, &w.keys);
    assert!(claim.is_true());
    let cl = nth(&s, 0, claim, 6_510);
    assert_eq!(cl.value, position.value + claim.bond as u64);
    // Anybody settles it after the dispute period: here the second executor pays the fee.
    let f = w.pay(&w.w.clone());
    let b = settle(&lib, &cl, &w.script("KpfTSettle"), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, 50, "the claim proves the dispute period's age");
    let s = signed("settle", &b, &w.keys);
    let (pa, pb) = claim.settle_pay(cl.value as i64);
    assert_eq!(pa, claim.amount + claim.bond);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (pa as u64, &p2pk_spk(&xk(&w.a))));
    let last = s.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), (pb as u64, &p2pk_spk(&xk(&w.b))));
    assert!(s.tx.outputs.iter().all(|o| o.covenant.map(|c| c.covenant_id) != cl.id), "the position's id ends");
    // The other side declares the same way.
    let f = w.pay(&w.b.clone());
    let (b, cb) = close(&lib, &position, &w.script("KpfTClose"), SIDE_B, T0, p, None, Payer { funds: &f, change_to: xk(&w.b), rate: MIN_FEE_RATE }).unwrap();
    signed("declare by the counterparty", &b, &w.keys);
    // Each side states its amount at prices moved against itself by the tolerance g, so the two statements leave a gap.
    assert!(cb.amount + claim.amount <= next.pot() && cb.amount + claim.amount > next.pot() - 10 * KAS1);
}

#[test]
fn a_false_claim_is_disputed_by_the_second_executor_and_lapses() {
    let mut w = world();
    let lib = w.lib;
    let a = w.position.s;
    let p = [a.x.legs[0].entry, a.x.legs[1].entry];
    let truth = a.owed(SIDE_A, p).unwrap();
    // A states 100 KAS more than the rule gives. The declaration computes nothing and is accepted.
    let f = w.pay(&w.a.clone());
    let (b, claim) = close(&lib, &w.position, &w.script("KpfTClose"), SIDE_A, T0, p, Some(truth + 100 * KAS1), Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("declare (false)", &b, &w.keys);
    assert!(!claim.is_true());
    let cl = nth(&s, 0, claim, 6_510);
    // W reads the opening's payload, finds the amount wrong, disputes with its own bond.
    let f = w.pay(&w.w.clone());
    let (b, dis) = dispute(&lib, &cl, &w.script("KpfTDispute"), xk(&w.w), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("dispute", &b, &w.keys);
    let dl = nth(&s, 0, dis, 6_520);
    assert_eq!(dl.value, cl.value + claim.bond as u64);
    // The script it used is the copy that went on at output 1.
    let copy = nth(&s, 1, "KpfTDispute", 6_520);
    // Nobody shows the committee's price, for the claim or against it. After the dispute period nothing is decided:
    // the position is whole again, A (the claimer) has its bond back and W (the disputer) its own. (A false claim is
    // not left to lapse where the committee answers: whoever shows its price has half of the claimer's bond.)
    let f = w.pay(&w.w.clone());
    let (b, back) = lapse(&lib, &dl, &copy, Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("lapse", &b, &w.keys);
    assert_eq!(back, a);
    assert_eq!(s.tx.outputs[0].value, w.position.value);
    assert_eq!(s.tx.outputs[0].covenant.map(|c| c.covenant_id), w.position.id, "the same position, under its id");
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (claim.bond as u64, &p2pk_spk(&xk(&w.a))));
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (claim.bond as u64, &p2pk_spk(&xk(&w.w))));
    // The position goes on: its owner can add margin to what the lapse wrote.
    let position = nth(&s, 0, back, 6_600);
    let f = w.pay(&w.a.clone());
    let (b, _) = add(&lib, &position, &w.script("KpfTAdd"), SIDE_A, KAS1, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    signed("add margin after the lapse", &b, &w.keys);
}

#[test]
fn a_late_claim_is_ended_and_the_two_parties_close_together() {
    let mut w = world();
    let lib = w.lib;
    let a = w.position.s;
    let p = [a.x.legs[0].entry, a.x.legs[1].entry];
    let f = w.pay(&w.a.clone());
    let (b, claim) = close(&lib, &w.position, &w.script("KpfTClose"), SIDE_A, T0, p, None, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("declare (lands late)", &b, &w.keys);
    let landed = (T0 + a.x.t.late_daa + 1) as u64;
    let cl = nth(&s, 0, claim, landed);
    // A late claim is neither settled nor disputed by the tool, and the script of the late end refuses one in time.
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    assert!(settle(&lib, &cl, &w.script("KpfTSettle"), payer).is_err());
    assert!(dispute(&lib, &cl, &w.script("KpfTDispute"), xk(&w.w), payer).is_err());
    assert!(late(&lib, &nth(&s, 0, claim, landed - 1), &w.script("KpfTLate"), xk(&w.w), payer).is_err());
    let (b, back) = late(&lib, &cl, &w.script("KpfTLate"), xk(&w.w), payer).unwrap();
    let s2 = signed("late", &b, &w.keys);
    assert_eq!(back, a);
    assert_eq!(s2.tx.outputs[0].value, w.position.value);
    // The claimer does not have its bond back (review T1b, F3): the party it claimed against has half, who ended the
    // claim the rest.
    let n = s2.tx.outputs.len();
    assert_eq!((s2.tx.outputs[n - 2].value, &s2.tx.outputs[n - 2].script_public_key), ((claim.bond / 2) as u64, &p2pk_spk(&xk(&w.b))));
    assert_eq!((s2.tx.outputs[n - 1].value, &s2.tx.outputs[n - 1].script_public_key), ((claim.bond - claim.bond / 2) as u64, &p2pk_spk(&xk(&w.w))));
    assert!(s2.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&xk(&w.a))), "nothing to the claimer");
    // The same transaction with a claim that landed in time is refused by the script (the builder is told a lie).
    let mut early = b.clone();
    early.entries[0].block_daa_score = landed - 1;
    let es = early.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&es.tx, &es.entries).is_ok(), "the script of the late end reads when the claim landed");
    // A claim that landed before its own time (review T1): it states a price nobody can have checked. It is neither
    // settled nor disputed, by the tool or by the scripts, and is ended as a late one.
    let before = nth(&s, 0, claim, (T0 - 1) as u64);
    assert!(settle(&lib, &before, &w.script("KpfTSettle"), payer).is_err());
    assert!(dispute(&lib, &before, &w.script("KpfTDispute"), xk(&w.w), payer).is_err());
    let (b, _) = late(&lib, &before, &w.script("KpfTLate"), xk(&w.w), payer).unwrap();
    signed("a claim of a signed price to come, ended", &b, &w.keys);
    let in_time = nth(&s, 0, claim, T0 as u64);
    for (builder, name) in [(settle(&lib, &in_time, &w.script("KpfTSettle"), payer).unwrap(), "settle"), (dispute(&lib, &in_time, &w.script("KpfTDispute"), xk(&w.w), payer).unwrap().0, "dispute")] {
        let mut t = builder.clone();
        t.entries[0].block_daa_score = (T0 - 1) as u64;
        let ts = t.sign(&w.keys).unwrap();
        assert!(!kpf_protocol::verify::verify(&ts.tx, &ts.entries).is_ok(), "the script of the {name} takes a claim that landed before its time");
    }
    // Both: the two sign a payout of the whole position.
    let f = w.pay(&w.a.clone());
    let v = w.position.value as i64;
    let b = both_close(&lib, &w.position, &w.script("KpfTBoth"), 1_900 * KAS1, v - 1_900 * KAS1, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    signed("close by both", &b, &w.keys);
    let mut one = KeySet::new();
    one.add_keypair(w.a);
    assert!(b.sign(&one).is_err(), "one party alone cannot sign it");
    assert!(both_close(&lib, &w.position, &w.script("KpfTBoth"), v, 1, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).is_err());
}

#[test]
fn the_builders_refuse_what_the_scripts_would() {
    let mut w = world();
    let lib = w.lib;
    let a = w.position.s;
    let p = [a.x.legs[0].entry, a.x.legs[1].entry];
    let f = w.pay(&w.a.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE };
    // Another script than the operation's, a script of another library, an object without an id.
    assert!(add(&lib, &w.position, &w.script("KpfTBoth"), SIDE_A, KAS1, payer).is_err());
    let mut alien = w.script("KpfTAdd");
    alien.id = Some(kaspa_consensus_core::Hash::from_bytes([7; 32]));
    assert!(add(&lib, &w.position, &alien, SIDE_A, KAS1, payer).is_err());
    let mut bare = w.position.clone();
    bare.id = None;
    assert!(add(&lib, &bare, &w.script("KpfTAdd"), SIDE_A, KAS1, payer).is_err());
    assert!(add(&lib, &w.position, &w.script("KpfTAdd"), SIDE_A, 0, payer).is_err());
    assert!(close(&lib, &w.position, &w.script("KpfTClose"), 3, T0, p, None, payer).is_err());
    assert!(close(&lib, &w.position, &w.script("KpfTClose"), SIDE_A, T0, p, Some(a.pot() + 1), payer).is_err());
    assert!(lib_genesis(0, 10 * KAS1 as u64, payer).is_err() && lib_genesis(9, 10 * KAS1 as u64, payer).is_err() && lib_genesis(1, 1, payer).is_err());
    // A transaction changed after it was built is refused by the scripts: margin stated but not paid in.
    let (b, _) = add(&lib, &w.position, &w.script("KpfTAdd"), SIDE_A, 50 * KAS1, payer).unwrap();
    let mut short = b.clone();
    short.tx.outputs[0].value -= 1;
    let ss = short.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&ss.tx, &ss.entries).is_ok());
    // And the position opened in another library is another address.
    assert_ne!(object_spk(&lib, &kpf_protocol::thin::TObj::Position(a)), object_spk(&[7; 32], &kpf_protocol::thin::TObj::Position(a)));
    let _ = (SIDE_B, &w.b);
}

// ---------------------------------------------------------------- the liquidation

use kpf_protocol::thin::{LiqObj, LiqState, TObj};

/// A price of the first leg at which the rule liquidates A (the first found going away from its entry by half a
/// percent, down then up), and the liquidation there by `liquidator`.
fn liquidating(a: &Position, liquidator: [u8; 32]) -> ([i64; 2], kpf_protocol::thin::Liq) {
    let e = [a.x.legs[0].entry, a.x.legs[1].entry];
    for k in 1..200 {
        for sign in [-1, 1] {
            let p = [e[0] + sign * (e[0] / 200) * k, e[1]];
            if let Some(l) = a.liquidation(SIDE_A, T0, p, liquidator) {
                return (p, l);
            }
        }
    }
    panic!("no price liquidates the position");
}

/// The world's position with A's margin a tenth of the notional at the entries, so that a price liquidates it.
fn thin_margin(w: &mut World) {
    let mut a = w.position.s;
    let n = a.x.legs[0].worth(a.x.legs[0].entry) + a.x.legs[1].worth(a.x.legs[1].entry);
    a.x.a_margin = n / 10;
    a.check().unwrap();
    w.position = OLive { s: a, value: a.x.floor() as u64, ..w.position.clone() };
}

#[test]
fn a_liquidation_is_declared_and_cured_or_settled() {
    let mut w = world();
    thin_margin(&mut w);
    let lib = w.lib;
    let a = w.position.s;
    let (p, rule) = liquidating(&a, xk(&w.w));
    assert!(rule.is_true() && rule.fee > 0 && rule.add > 0);
    // The second executor declares the liquidation the rule gives, with the bond.
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (b, lq) = liquidate(&lib, &w.position, &w.script("KpfTLiq"), SIDE_A, T0, p, None, xk(&w.w), payer).unwrap();
    let s = signed("liquidate", &b, &w.keys);
    assert_eq!(lq, LiqObj { liq: rule, state: LiqState::Claimed });
    let live = nth(&s, 0, lq, 6_510);
    assert_eq!(live.value, w.position.value + rule.bond as u64);
    // A side the rule does not liquidate is refused by the tool (the script would take it: a dispute tests it).
    assert!(liquidate(&lib, &w.position, &w.script("KpfTLiq"), SIDE_B, T0, p, None, xk(&w.w), payer).is_err());
    assert!(liquidate(&lib, &w.position, &w.script("KpfTLiq"), SIDE_A, T0, [a.x.legs[0].entry, a.x.legs[1].entry], None, xk(&w.w), payer).is_err());
    // The side cures: the position again with what the claim asked added, the liquidator's bond back.
    let f2 = w.pay(&w.a.clone());
    let (b, cured) = cure(&lib, &live, &w.script("KpfTCure"), Payer { funds: &f2, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let cs = signed("cure", &b, &w.keys);
    assert_eq!(cured.x.a_margin, a.x.a_margin + rule.add);
    assert_eq!(cs.tx.outputs[0].value, w.position.value + rule.add as u64);
    let last = cs.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), (rule.bond as u64, &p2pk_spk(&xk(&w.w))));
    assert!(!cured.x.liquidatable(SIDE_A, p), "cured, the side has its maintenance at those prices");
    // Nobody but the side cures: without its key the transaction is not signed.
    let mut others = KeySet::new();
    others.add_keypair(w.w);
    others.add_keypair(w.b);
    let f3 = w.pay(&w.w.clone());
    let (b, _) = cure(&lib, &live, &w.script("KpfTCure"), Payer { funds: &f3, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    assert!(b.sign(&others).is_err());
    // Not cured: after the dispute period anyone settles it. The liquidator its fee and bond, the other party the rest.
    let f4 = w.pay(&w.b.clone());
    let b = liq_settle(&lib, &live, &w.script("KpfTLiqSettle"), Payer { funds: &f4, change_to: xk(&w.b), rate: MIN_FEE_RATE }).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, a.dispute_daa as u64);
    let ss = signed("settle a liquidation", &b, &w.keys);
    assert_eq!((ss.tx.outputs[0].value, &ss.tx.outputs[0].script_public_key), ((rule.fee + rule.bond) as u64, &p2pk_spk(&xk(&w.w))));
    let last = ss.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), (w.position.value - rule.fee as u64, &p2pk_spk(&xk(&w.b))), "the side liquidated forfeits to the other party");
    // The dispute period is the script's to ask: the same settlement of a younger claim is refused by the engine.
    let mut young = b.clone();
    young.tx.inputs[0].sequence = a.dispute_daa as u64 - 1;
    let ys = young.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&ys.tx, &ys.entries).is_ok());
    // A liquidation that landed late is ended, not settled.
    let late = nth(&s, 0, lq, (T0 + a.x.t.late_daa + 1) as u64);
    assert!(liq_settle(&lib, &late, &w.script("KpfTLiqSettle"), Payer { funds: &f4, change_to: xk(&w.b), rate: MIN_FEE_RATE }).is_err());
    assert!(liq_late(&lib, &live, &w.script("KpfTLiq"), xk(&w.b), Payer { funds: &f4, change_to: xk(&w.b), rate: MIN_FEE_RATE }).is_err());
    let (b, back) = liq_late(&lib, &late, &w.script("KpfTLiq"), xk(&w.b), Payer { funds: &f4, change_to: xk(&w.b), rate: MIN_FEE_RATE }).unwrap();
    let ls = signed("a late liquidation", &b, &w.keys);
    assert_eq!(back, a);
    // Half of its bond to who ended it; the liquidator has none of it back.
    let n = ls.tx.outputs.len();
    assert_eq!((ls.tx.outputs[n - 1].value, &ls.tx.outputs[n - 1].script_public_key), ((rule.bond - rule.bond / 2) as u64, &p2pk_spk(&xk(&w.b))));
    assert_eq!(ls.tx.outputs[1].value, w.script("KpfTLiq").value + (rule.bond / 2) as u64 + kpf_protocol::thin::ORACLE_FEE as u64, "the other half stays in the script's copy, with the oracle fees");
    assert!(ls.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&xk(&w.a))), "nothing to the side named");
    // A cure asks the claim to have stood half a dispute period (review T1b, F2): the same cure of a younger claim is refused.
    let (b, _) = cure(&lib, &live, &w.script("KpfTCure"), Payer { funds: &f2, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, a.dispute_daa as u64 / 2);
    let mut young = b.clone();
    young.tx.inputs[0].sequence = a.dispute_daa as u64 / 2 - 1;
    let ys = young.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&ys.tx, &ys.entries).is_ok(), "a cure before the claim stood half a dispute period");
}

#[test]
fn a_false_liquidation_is_disputed_and_lapses_or_is_cured_under_dispute() {
    let mut w = world();
    let lib = w.lib;
    let a = w.position.s;
    let e = [a.x.legs[0].entry, a.x.legs[1].entry];
    assert!(a.liquidation(SIDE_A, T0, e, xk(&w.w)).is_none(), "nobody is liquidated at the entries");
    // The second executor states a liquidation at the entries, which the rule does not give. It is accepted.
    let f = w.pay(&w.w.clone());
    let (b, lq) = liquidate(&lib, &w.position, &w.script("KpfTLiq"), SIDE_A, T0, e, Some((KAS1, 10 * KAS1)), xk(&w.w), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("liquidate (false)", &b, &w.keys);
    assert!(!lq.liq.is_true());
    let live = nth(&s, 0, lq, 6_510);
    // The counterparty disputes it (it wins by the position going on).
    let f = w.pay(&w.b.clone());
    let (b, dl) = liq_dispute(&lib, &live, &w.script("KpfTLiqDispute"), xk(&w.b), Payer { funds: &f, change_to: xk(&w.b), rate: MIN_FEE_RATE }).unwrap();
    let ds = signed("dispute a liquidation", &b, &w.keys);
    assert_eq!(dl.state, LiqState::Disputed(xk(&w.b)));
    let dlive = nth(&ds, 0, dl, 6_520);
    let copy = nth(&ds, 1, "KpfTLiqDispute", 6_520);
    assert_eq!(dlive.value, live.value + lq.liq.bond as u64);
    // A disputed liquidation is not disputed twice, not settled, not ended as late.
    let f = w.pay(&w.b.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.b), rate: MIN_FEE_RATE };
    assert!(liq_dispute(&lib, &dlive, &copy, xk(&w.a), payer).is_err());
    assert!(liq_settle(&lib, &dlive, &w.script("KpfTLiqSettle"), payer).is_err());
    assert!(liq_late(&lib, &dlive, &w.script("KpfTLiq"), xk(&w.b), payer).is_err());
    // Nobody shows the committee's prices, for it or against it: after the dispute period nothing is decided. The
    // position is whole, the disputer has its bond back and the liquidator its own.
    let (b, back) = liq_lapse(&lib, &dlive, &copy, payer).unwrap();
    let ls = signed("lapse of a liquidation", &b, &w.keys);
    assert_eq!(back, a);
    assert_eq!(ls.tx.outputs[0].value, w.position.value);
    let n = ls.tx.outputs.len();
    let bond = lq.liq.bond;
    assert_eq!((ls.tx.outputs[n - 1].value, &ls.tx.outputs[n - 1].script_public_key), (bond as u64, &p2pk_spk(&xk(&w.b))));
    assert_eq!((ls.tx.outputs[n - 2].value, &ls.tx.outputs[n - 2].script_public_key), (bond as u64, &p2pk_spk(&lq.liq.liquidator)));
    // Nothing of the bonds stays in the script's copy (a false liquidation is not left to lapse where the committee
    // answers: whoever shows its prices has half of the liquidator's bond, `liq_refute`).
    assert_eq!(ls.tx.outputs[1].value, copy.value + kpf_protocol::thin::ORACLE_FEE as u64);
    // A disputed liquidation is not cured (review T1b, F2): its liquidator proves it, or it lapses as above. A side
    // that liquidates itself falsely and is disputed cannot take the claim back.
    let f = w.pay(&w.a.clone());
    assert!(cure(&lib, &dlive, &w.script("KpfTCure"), Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).is_err());
    // A proved liquidation (the proof itself is not built: its state is) is cured or settled as a claim is.
    let proved = LiqObj { liq: lq.liq, state: LiqState::Proved };
    let pl = kpf_protocol::oracle_build::OLive { s: proved, value: live.value, id: live.id, op: live.op, daa: 6_530 };
    assert_ne!(TObj::Liq(proved).hash(), TObj::Liq(lq).hash());
    assert!(liq_dispute(&lib, &pl, &copy, xk(&w.a), payer).is_err(), "not disputed twice");
    assert!(liq_settle(&lib, &pl, &w.script("KpfTLiqSettle"), payer).is_ok());
    // Every state of a liquidation is written and read back.
    for o in [TObj::Liq(lq), TObj::Liq(dl), TObj::Liq(proved)] {
        assert_eq!(TObj::decode(&o.encode()).unwrap(), o, "{}", o.kind());
    }
}

#[test]
fn a_library_is_read_from_its_genesis_and_one_output_more_is_refused() {
    use kaspa_consensus_core::tx::{CovenantBinding, TransactionOutput};
    let f = kp(9);
    let mut keys = KeySet::new();
    keys.add_keypair(f);
    let funds = vec![fund(xk(&f), 200, 1_000)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).unwrap();
    // The genesis the tool builds is a library, and its id is the one the check computes.
    assert_eq!(lib_check(&gs.tx).unwrap(), lib);
    assert_eq!(lib_scripts(&gs.tx, 0).len(), kpf_protocol::thin_artifacts::SCRIPTS.len() + 1, "one copy of each, and a second of the citing script");
    let binding = gs.tx.outputs[1].covenant;
    // One output more under the id, of any other script (review T1b, F4): refused, whatever the id.
    let mut more = gs.tx.clone();
    more.outputs.push(TransactionOutput { value: KAS1 as u64, script_public_key: p2pk_spk(&xk(&f)), covenant: binding });
    assert!(lib_check(&more).unwrap_err().to_string().contains("not one of the family's scripts"));
    // The family's scripts and one more copy of one of them, written after the fact under the same id: the id is not
    // these outputs' (a genesis commits to what it wrote).
    let mut copy = gs.tx.clone();
    let again = copy.outputs[1].clone();
    copy.outputs.push(again);
    assert!(lib_check(&copy).unwrap_err().to_string().contains("not the covenant id of these outputs"));
    // A library short of a script, a transaction of two ids, and one of none.
    let mut short = gs.tx.clone();
    short.outputs.truncate(short.outputs.len() - 1);
    assert!(lib_check(&short).is_err());
    let mut two = gs.tx.clone();
    two.outputs[2].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: kaspa_consensus_core::Hash::from_bytes([9; 32]) });
    assert!(lib_check(&two).is_err());
    let mut none = gs.tx.clone();
    for o in none.outputs.iter_mut() {
        o.covenant = None;
    }
    assert!(lib_check(&none).is_err());
    // The scripts are named by what they pay to, not by their place: with two outputs exchanged each is still itself.
    let mut swapped = gs.tx.clone();
    swapped.outputs.swap(1, 2);
    let names: Vec<&str> = lib_scripts(&swapped, 0).iter().map(|s| s.s).collect();
    assert_eq!((names[0], names[1]), (kpf_protocol::thin_artifacts::SCRIPTS[1], kpf_protocol::thin_artifacts::SCRIPTS[0]));
}

// ---------------------------------------------------------------- orders between users

use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::TObj as TObjT;
use kpf_protocol::thin::{Order, Rest, Template};

/// A sell of 10 units against buys of 3, 2, 4 and 2: four positions (3, 2, 4, 1), one a buyer each with the seller,
/// and 1 unit of the last buy left as an order. Every transaction through the engine, and every output held from below.
#[test]
fn orders_of_two_users_match_one_to_one_and_what_is_left_rests() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let buyers = [kp(21), kp(22), kp(23), kp(24)];
    for k in &buyers {
        w.keys.add_keypair(*k);
    }
    let seller = w.b;
    // Orders: each posts 20 KAS of margin a unit and a tip of 0.05 KAS a unit, rests, and ends at DAA 9,000.
    let order = |owner: &Keypair, dir: i64, units: i64, limit: i64| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: units * unit, limit, margin: units * 20 * KAS1, tip: units * KAS1 / 20, expiry: 9_000, min_fill: 0, min_counter: units * 20 * KAS1 };
    let place = |w: &mut World, o: &Order, who: &Keypair| -> OLive<Order> {
        let f = w.pay(who);
        let b = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), o, Payer { funds: &f, change_to: xk(who), rate: MIN_FEE_RATE }).unwrap();
        let s = signed("place an order", &b, &w.keys);
        assert_eq!(s.tx.outputs[0].value, o.value() as u64);
        nth(&s, 0, *o, 6_200)
    };
    let mut sell = place(&mut w, &order(&seller, SHORT, 10, px), &seller);
    let mut script = w.script("KpfTMatch");
    let mut positions = vec![];
    let mut left = None;
    for (i, units) in [3i64, 2, 4, 2].into_iter().enumerate() {
        let buy = place(&mut w, &order(&buyers[i], LONG, units, px + px / 100), &buyers[i]);
        // The matcher (the second executor) fills what both have, at the seller's price.
        let n = (units * unit).min(sell.s.size);
        let f = w.pay(&w.w.clone());
        let (b, m) = order_match(&lib, &tpl, &buy, &sell, &script, n, px, Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
        let s = signed(&format!("match {} of a buy of {units} with a sell of {}", n / unit, sell.s.size / unit), &b, &w.keys);
        // The position: the buyer its A, long n at the price; the seller its B; each its share of margin.
        assert_eq!((m.position.x.a_key, m.position.x.b_key), (xk(&buyers[i]), xk(&seller)));
        assert_eq!((m.position.x.legs[0].size, m.position.x.legs[0].entry, m.position.x.a_margin, m.position.x.b_margin), (n, px, n / unit * 20 * KAS1, n / unit * 20 * KAS1));
        assert_eq!(s.tx.outputs[0].covenant.map(|c| c.covenant_id), buy.id, "the position is under the long order's id");
        positions.push(nth(&s, 0, m.position, 6_300));
        assert_eq!(m.tips, 2 * (n / unit) * KAS1 / 20);
        // What rests of the sell is found by its id; the script went on at its own index.
        script = nth(&s, 2, "KpfTMatch", 6_300);
        match m.short {
            Rest::Order(o, v) => {
                // (All is under one id, the registry's: what rests of the sell is the output after the position.)
                let at = 1;
                assert_eq!(s.tx.outputs[at].value, v as u64);
                sell = nth(&s, at, o, 6_300);
            }
            r => assert_eq!((r, i), (Rest::Nothing, 3), "the sell is used up by the last buy"),
        }
        match m.long {
            Rest::Order(o, v) => {
                assert_eq!((i, o.size, v), (3, unit, 20 * KAS1 + KAS1 / 20), "one unit of the last buy rests");
                left = Some(nth(&s, 1, o, 6_300));
            }
            r => assert_eq!(r, Rest::Nothing),
        }
    }
    assert_eq!(positions.iter().map(|a| a.s.x.legs[0].size / unit).collect::<Vec<_>>(), vec![3, 2, 4, 1]);
    // A position of a match is a position: its owner adds margin to it.
    let f = w.pay(&buyers[0]);
    let (b, _) = add(&lib, &positions[0], &w.script("KpfTAdd"), SIDE_A, KAS1, Payer { funds: &f, change_to: xk(&buyers[0]), rate: MIN_FEE_RATE }).unwrap();
    signed("add margin to a position of a match", &b, &w.keys);
    // What rests of the last buy is an order under the same id as its position: its owner takes it back, and nobody
    // else does before its expiry.
    let left = left.expect("the rest of the last buy");
    assert_eq!(left.id, positions[3].id);
    let f = w.pay(&buyers[3]);
    let payer = Payer { funds: &f, change_to: xk(&buyers[3]), rate: MIN_FEE_RATE };
    let b = order_cancel(&lib, &left, &w.script("KpfTCancel"), payer).unwrap();
    let s = signed("cancel what rests", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (left.value, &p2pk_spk(&xk(&buyers[3]))));
    let mut others = KeySet::new();
    others.add_keypair(w.w);
    others.add_keypair(buyers[3]);
    let mut not_owner = b.clone();
    not_owner.plans[1] = match &b.plans[1] {
        kpf_protocol::oracle_build::OPlan::Contract { which, redeem, entry, args } => {
            let mut a = args.clone();
            let n = a.len() - 1;
            a[n] = kpf_protocol::tx::Arg::Sig(xk(&w.w));
            kpf_protocol::oracle_build::OPlan::Contract { which: *which, redeem: redeem.clone(), entry, args: a }
        }
        p => p.clone(),
    };
    let ns = not_owner.sign(&others).unwrap();
    assert!(!kpf_protocol::verify::verify(&ns.tx, &ns.entries).is_ok(), "a cancel signed by another key");
    let b = order_expire(&lib, &left, &w.script("KpfTCancel"), payer).unwrap();
    assert_eq!(b.tx.lock_time, 9_000, "the lock time proves the expiry");
    signed("expire what rests", &b, &w.keys);
    let mut early = b.clone();
    early.tx.lock_time = 8_999;
    let es = early.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&es.tx, &es.entries).is_ok(), "an expiry before its DAA");
}

#[test]
fn a_match_holds_both_limits_one_template_and_an_order_that_does_not_rest_is_returned() {
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (buyer, seller) = (kp(21), kp(22));
    w.keys.add_keypair(buyer);
    w.keys.add_keypair(seller);
    let order = |owner: &Keypair, dir: i64, units: i64, limit: i64, _rests: bool| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: units * unit, limit, margin: units * 20 * KAS1, tip: units * KAS1 / 20, expiry: 9_000, min_fill: 0, min_counter: units * 20 * KAS1 };
    let live = |o: Order, n: u8| OLive { s: o, value: o.value() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let script = w.script("KpfTMatch");
    // A buy of 5 and a sell of 2: 2 are matched, 3 of the buy rest, the sell is used up. The tips' shares are written
    // nowhere: they are in the change of whoever sent the match.
    let (buy, sell) = (live(order(&buyer, LONG, 5, px, true), 0x61), live(order(&seller, SHORT, 2, px, true), 0x62));
    let (b, m) = order_match(&lib, &tpl, &buy, &sell, &script, 2 * unit, px, payer).unwrap();
    let s = signed("match, 3 of the buy rest", &b, &w.keys);
    let Rest::Order(left, held) = m.long else { panic!("3 of the buy rest") };
    assert_eq!((left.size, left.margin, left.tip, held, m.short, m.tips), (3 * unit, 3 * 20 * KAS1, 3 * KAS1 / 20, 3 * 20 * KAS1 + 3 * KAS1 / 20, Rest::Nothing, 4 * KAS1 / 20));
    assert_eq!(s.tx.outputs.len(), 4, "the position, what rests, the script, the change");
    assert_eq!(s.tx.outputs.iter().filter(|o| o.covenant.map(|c| c.covenant_id) == buy.id).count(), 2, "the position and what rests of the buy");
    assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&xk(&buyer)) && o.script_public_key != p2pk_spk(&xk(&seller))), "nothing is paid back to an order's owner in a match");
    // An order to be taken whole or not at all says so by its least fill.
    let whole = live(Order { min_fill: 5 * unit, ..order(&buyer, LONG, 5, px, true) }, 0x61);
    assert!(order_match(&lib, &tpl, &whole, &sell, &script, 2 * unit, px, payer).unwrap_err().to_string().contains("at least"));
    // The price: within both limits and the market's, for the tool and for the script.
    let (buy, sell) = (live(order(&buyer, LONG, 5, px, true), 0x61), live(order(&seller, SHORT, 5, px - px / 50, true), 0x62));
    assert!(order_match(&lib, &tpl, &buy, &sell, &script, unit, px + 1, payer).is_err(), "above the long's limit");
    assert!(order_match(&lib, &tpl, &buy, &sell, &script, unit, px - px / 50 - 1, payer).is_err(), "below the short's limit");
    let (b, _) = order_match(&lib, &tpl, &buy, &sell, &script, unit, px, payer).unwrap();
    signed("match inside both limits", &b, &w.keys);
    // The script reads the price from its own arguments: the same match with a price a sompi above the long's limit
    // in the arguments is another position than the one written, and is refused.
    let mut high = b.clone();
    if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut high.plans[2] {
        let at = args.len() - 1;
        args[at] = kpf_protocol::tx::Arg::V(silverscript_abi::ArtifactValue::Int(px + 1));
    }
    let hs = high.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&hs.tx, &hs.entries).is_ok());
    // More than an order has, nothing, two longs, one owner on both sides, another template.
    assert!(order_match(&lib, &tpl, &buy, &sell, &script, 6 * unit, px, payer).is_err());
    assert!(order_match(&lib, &tpl, &buy, &sell, &script, 0, px, payer).is_err());
    assert!(order_match(&lib, &tpl, &buy, &live(order(&seller, LONG, 5, px, true), 0x62), &script, unit, px, payer).is_err());
    assert!(order_match(&lib, &tpl, &buy, &live(order(&buyer, SHORT, 5, px, true), 0x62), &script, unit, px, payer).is_err());
    let other = Template { dispute_daa: tpl.dispute_daa + 1, ..tpl };
    assert!(order_match(&lib, &other, &buy, &sell, &script, unit, px, payer).is_err());
    let mut alien = order(&seller, SHORT, 5, px, true);
    alien.tpl = other.hash();
    assert!(order_match(&lib, &tpl, &buy, &live(alien, 0x62), &script, unit, px, payer).is_err(), "orders of two templates do not mean one position");
    // A rest too small to be an order: a buy with a margin of 0.3 KAS a unit, matched for all but one of its units.
    let thin_order = Order { margin: 5 * 3 * KAS1 / 10, tip: 0, min_counter: 0, ..order(&buyer, LONG, 5, px, true) };
    let asks_nothing = live(Order { min_counter: 0, ..sell.s }, 0x62);
    assert!(order_match(&lib, &tpl, &live(thin_order, 0x61), &asks_nothing, &script, 5 * unit - 1, px, payer).unwrap_err().to_string().contains("too small to rest"));
    // What an order asks of the other side (review T1b, F6): a sell with 20 KAS of margin a unit that asks as much is
    // not matched against a buy that posts a sompi, by the tool or by the script; one that asks nothing is.
    let sompi = Order { margin: 5, tip: 5 * KAS1, min_counter: 0, ..order(&buyer, LONG, 5, px, true) };
    assert!(order_match(&lib, &tpl, &live(sompi, 0x61), &sell, &script, 5 * unit, px, payer).unwrap_err().to_string().contains("less margin than the other order asks"));
    let (b, m) = order_match(&lib, &tpl, &live(sompi, 0x61), &asks_nothing, &script, 5 * unit, px, payer).unwrap();
    assert_eq!(m.position.x.a_margin, 5, "an order that asks nothing is matched against anything");
    let mut forged = b.clone();
    forged.entries[1] = kaspa_consensus_core::tx::UtxoEntry::new(sell.s.value() as u64, object_spk(&lib, &TObjT::Order(sell.s)), 6_200, false, Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)));
    if let kpf_protocol::oracle_build::OPlan::Contract { redeem, .. } = &mut forged.plans[1] {
        *redeem = TObjT::Order(sell.s).script(&lib);
    }
    if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut forged.plans[2] {
        args[19] = kpf_protocol::tx::Arg::V(silverscript_abi::ArtifactValue::Int(sell.s.min_counter));
    }
    let fs = forged.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "the script matches an order against less margin than it asks");
    // The least fill: a sell of 10 that takes at least 4 a match is not matched for 3, is for 4, and what is left
    // (6, then 2) can be taken whole though it is below the least.
    let big = Order { min_fill: 4 * unit, ..order(&seller, SHORT, 10, px, true) };
    let small = |units: i64| live(order(&buyer, LONG, units, px, true), 0x61);
    assert!(order_match(&lib, &tpl, &small(3), &live(big, 0x62), &script, 3 * unit, px, payer).unwrap_err().to_string().contains("at least"));
    let (b, m) = order_match(&lib, &tpl, &small(4), &live(big, 0x62), &script, 4 * unit, px, payer).unwrap();
    signed("match at the least fill", &b, &w.keys);
    let Rest::Order(six, _) = m.short else { panic!("six rest") };
    assert_eq!((six.size, six.min_fill), (6 * unit, 4 * unit));
    let two = Order { size: 2 * unit, margin: 2 * 20 * KAS1, tip: 2 * KAS1 / 20, min_counter: 2 * 20 * KAS1, ..big };
    let (b, _) = order_match(&lib, &tpl, &small(2), &live(two, 0x62), &script, 2 * unit, px, payer).unwrap();
    signed("match all that is left, below the least fill", &b, &w.keys);
    // The script holds it too: the match of 4 with 3 in the size's place is another position and another rest.
    let (b3, _) = order_match(&lib, &tpl, &small(3), &live(Order { min_fill: 0, ..big }, 0x62), &script, 3 * unit, px, payer).unwrap();
    let mut forged = b3.clone();
    forged.entries[1] = kaspa_consensus_core::tx::UtxoEntry::new(big.value() as u64, object_spk(&lib, &TObjT::Order(big)), 6_200, false, Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)));
    if let kpf_protocol::oracle_build::OPlan::Contract { redeem, .. } = &mut forged.plans[1] {
        *redeem = TObjT::Order(big).script(&lib);
    }
    if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut forged.plans[2] {
        // The short order's least fill: four arguments of the head, eight of the long order, then the short's seventh.
        args[18] = kpf_protocol::tx::Arg::V(silverscript_abi::ArtifactValue::Int(4 * unit));
    }
    let fs = forged.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "the script takes a match below an order's least fill");
    // Every kind of order is written and read back.
    assert_eq!(TObjT::decode(&TObjT::Order(buy.s).encode()).unwrap(), TObjT::Order(buy.s));
    assert_eq!(tpl.bytes().len(), kpf_protocol::thin::TEMPLATE_LEN);
}

// ---------------------------------------------------------------- two positions into one

/// A buy of 5 filled by two sells of the same seller, 3 at one price and 2 at another: two positions of the same two
/// users, which anyone merges into one. Each user is owed at every price what the two positions owed it, less its half
/// of the merger's share.
#[test]
fn two_positions_of_the_same_two_users_are_merged_by_anyone() {
    use kpf_protocol::thin::{merged, MERGE_TIP};
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (buyer, seller) = (kp(21), kp(22));
    w.keys.add_keypair(buyer);
    w.keys.add_keypair(seller);
    let order = |owner: &Keypair, dir: i64, units: i64, limit: i64| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: units * unit, limit, margin: units * 20 * KAS1, tip: units * KAS1 / 20, expiry: 9_000, min_fill: 0, min_counter: units * 20 * KAS1 };
    let live = |o: Order, n: u8| OLive { s: o, value: o.value() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // Two fills of one buy, at two prices that do not divide evenly.
    let (p1, p2) = (px, px + px / 300 + 7);
    let buy = live(order(&buyer, LONG, 5, p2), 0x61);
    let (b, m1) = order_match(&lib, &tpl, &buy, &live(order(&seller, SHORT, 3, p1), 0x62), &w.script("KpfTMatch"), 3 * unit, p1, payer).unwrap();
    let s1 = signed("first fill", &b, &w.keys);
    let Rest::Order(rest, _) = m1.long else { panic!("two units rest") };
    let (position1, rest_live, script) = (nth(&s1, 0, m1.position, 6_300), nth(&s1, 1, rest, 6_300), nth(&s1, 2, "KpfTMatch", 6_300));
    let (b, m2) = order_match(&lib, &tpl, &rest_live, &live(order(&seller, SHORT, 2, p2), 0x63), &script, 2 * unit, p2, payer).unwrap();
    let s2 = signed("second fill", &b, &w.keys);
    let position2 = nth(&s2, 0, m2.position, 6_310);
    assert_eq!(position1.id, position2.id, "the two positions are under the buy order's id");
    // The merge, by the second executor.
    let (b, one) = merge(&lib, &position1, &position2, &w.script("KpfTMerge"), payer).unwrap();
    let s = signed("merge two positions", &b, &w.keys);
    assert_eq!(one, merged(&position1.s, &position2.s).unwrap());
    assert_eq!((one.x.legs[0].size, one.x.a_key, one.x.b_key), (5 * unit, xk(&buyer), xk(&seller)));
    assert!(one.x.legs[0].entry > p1 && one.x.legs[0].entry < p2, "the entry of the whole lies between the two");
    assert_eq!(one.pot(), position1.s.pot() + position2.s.pot() - MERGE_TIP, "the two pots, less the merger's share");
    assert_eq!(s.tx.outputs[0].value, position1.value + position2.value - MERGE_TIP as u64);
    assert_eq!(s.tx.outputs.iter().filter(|o| o.covenant.map(|c| c.covenant_id) == position1.id).count(), 1, "one position goes on");
    // At every price, what A is owed before any clamp: the two positions' sum, less its half of the share, to a few sompi.
    for k in 0..400 {
        let p = px / 2 + k * (px / 300) + k;
        let sep = position1.s.x.a_margin + position1.s.x.legs[0].gain(p) + position2.s.x.a_margin + position2.s.x.legs[0].gain(p);
        let got = one.x.a_margin + one.x.legs[0].gain(p);
        assert!((got - (sep - MERGE_TIP / 2)).abs() <= 2, "at {p}: {got} against {sep}");
    }
    // The one position is a position: its owner adds margin; and it merges again with a third of the same two.
    let merged_live = nth(&s, 0, one, 6_400);
    let f2 = w.pay(&buyer);
    let (b, _) = add(&lib, &merged_live, &w.script("KpfTAdd"), SIDE_A, KAS1, Payer { funds: &f2, change_to: xk(&buyer), rate: MIN_FEE_RATE }).unwrap();
    signed("add margin to the merged position", &b, &w.keys);
    let third = OLive { s: tpl.position(xk(&buyer), xk(&seller), unit, p1, 20 * KAS1, 20 * KAS1), value: (40 * KAS1) as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([0x64; 32]), index: 0 }, daa: 6_400 };
    let (b, six) = merge(&lib, &merged_live, &third, &w.script("KpfTMerge"), payer).unwrap();
    signed("merge with a third position of the two", &b, &w.keys);
    assert_eq!(six.x.legs[0].size, 6 * unit);
    // Not merged: another counterparty, the sides the other way round, another dispute period, a position with itself.
    let other_party = OLive { s: tpl.position(xk(&buyer), xk(&w.w), unit, p1, 20 * KAS1, 20 * KAS1), ..third.clone() };
    assert!(merge(&lib, &merged_live, &other_party, &w.script("KpfTMerge"), payer).is_err());
    let turned = OLive { s: tpl.position(xk(&seller), xk(&buyer), unit, p1, 20 * KAS1, 20 * KAS1), ..third.clone() };
    assert!(merge(&lib, &merged_live, &turned, &w.script("KpfTMerge"), payer).is_err());
    let other_terms = OLive { s: Template { dispute_daa: tpl.dispute_daa + 1, ..tpl }.position(xk(&buyer), xk(&seller), unit, p1, 20 * KAS1, 20 * KAS1), ..third.clone() };
    assert!(merge(&lib, &merged_live, &other_terms, &w.script("KpfTMerge"), payer).is_err());
    assert!(merge(&lib, &merged_live, &merged_live, &w.script("KpfTMerge"), payer).is_err());
    // The script holds the parties too: the merge with the second position's counterparty changed is refused (its
    // hash is another), and so is one that keeps a sompi more than the share (the tightness check of `signed`).
    let (good, _) = merge(&lib, &merged_live, &third, &w.script("KpfTMerge"), payer).unwrap();
    let mut forged = good.clone();
    forged.entries[1] = kaspa_consensus_core::tx::UtxoEntry::new(third.value, object_spk(&lib, &TObjT::Position(other_party.s)), 6_400, false, third.id);
    if let kpf_protocol::oracle_build::OPlan::Contract { redeem, .. } = &mut forged.plans[1] {
        *redeem = TObjT::Position(other_party.s).script(&lib);
    }
    let fs = forged.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "the script merges positions of two counterparties");
}

// ---------------------------------------------------------------- a user leaves two positions

/// U is long against S and, later, short against L: U leaves, and L faces S. U signs; neither S nor L does, and no
/// price is shown. At every price S and L are owed what they were; U is paid its margins and what the
/// price moved between its two entries.
#[test]
fn a_user_leaves_two_positions_and_the_two_it_faced_face_each_other() {
    use kpf_protocol::thin::NOVATE_TIP;
    use kpf_protocol::thin_build::{novate, novate_with};
    use kpf_protocol::verify::verify;
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s1, l2) = (kp(31), kp(32), kp(33));
    // The user that leaves signs; the two that stay never do.
    w.keys.add_keypair(u);
    // 20 KAS of margin a unit from each party; each asks `ask` a unit of whoever faces it.
    let position = |a: &Keypair, b: &Keypair, units: i64, entry: i64, min_counter: i64| Position { a_min_counter: units * min_counter, b_min_counter: units * min_counter, ..tpl.position(xk(a), xk(b), units * unit, entry, units * 20 * KAS1, units * 20 * KAS1) };
    let live = |a: Position, n: u8| OLive { s: a, value: a.pot() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let script = w.script("KpfTNovate");
    let (p1, p2) = (px, px + px / 100 + 3);

    // 1. U long 5 against S from p1, short 3 against L from p2 (the price rose): 3 units become L against S.
    let (x, y) = (live(position(&u, &s1, 5, p1, 8 * KAS1), 0x71), live(position(&l2, &u, 3, p2, 8 * KAS1), 0x72));
    let (b, nv, made) = novate(&lib, &x, &y, &script, payer).unwrap();
    let s = signed("a user leaves 3 of its 5", &b, &w.keys);
    let ((z, zv), (rx, rxv)) = (nv.z.unwrap(), nv.rest_x.unwrap());
    assert!(nv.rest_y.is_none() && nv.transfer > 0 && nv.n == 3 * unit);
    assert_eq!((z.x.a_key, z.x.b_key, z.x.legs[0].size, z.x.legs[0].entry), (xk(&l2), xk(&s1), 3 * unit, p2));
    assert_eq!((z.x.a_margin, z.x.b_margin), (60 * KAS1, 60 * KAS1 - nv.transfer));
    assert_eq!((z.a_min_counter, z.b_min_counter), (24 * KAS1, 24 * KAS1), "each that stays keeps what it asked");
    assert_eq!((rx.x.legs[0].size, rx.x.a_margin, rx.x.b_margin, rx.a_min_counter), (2 * unit, 40 * KAS1, 40 * KAS1, 16 * KAS1));
    assert_eq!(nv.to_leaver, 120 * KAS1 + nv.transfer - NOVATE_TIP, "the user: its two margins and what the price moved");
    assert_eq!(zv + rxv + nv.to_leaver + NOVATE_TIP, (x.value + y.value) as i64, "nothing else leaves the two positions");
    for (i, a) in &made {
        let v = if *a == z { zv } else { rxv };
        assert_eq!(s.tx.outputs[*i].value as i64, v);
    }
    assert_eq!(s.tx.outputs.last().unwrap().value as i64, nv.to_leaver);
    for k in 0..400 {
        let p = px / 2 + k * (px / 300) + k;
        let s_before = x.s.x.b_margin - x.s.x.legs[0].gain(p);
        let s_after = z.x.b_margin - z.x.legs[0].gain(p) + rx.x.b_margin - rx.x.legs[0].gain(p);
        assert!((s_before - s_after).abs() <= 2, "S at {p}: {s_before} against {s_after}");
        assert_eq!(y.s.x.a_margin + y.s.x.legs[0].gain(p), z.x.a_margin + z.x.legs[0].gain(p), "L at {p}");
    }

    // 2. What is left of X (2), against a larger sale (4, at a lower price: S takes): the new position under X's id,
    // 2 of the sale go on under Y's.
    let x2 = nth(&s, made.iter().find(|(_, a)| *a == rx).unwrap().0, rx, 6_300);
    let y2 = live(position(&l2, &u, 4, p1 - px / 200, 8 * KAS1), 0x73);
    let (b, nv2, made2) = novate(&lib, &x2, &y2, &script, payer).unwrap();
    let s2 = signed("the rest of it, against a larger sale", &b, &w.keys);
    let ((z2, _), (ry, _)) = (nv2.z.unwrap(), nv2.rest_y.unwrap());
    assert!(nv2.transfer < 0 && nv2.rest_x.is_none());
    assert_eq!(z2.x.b_margin, 40 * KAS1 - nv2.transfer, "the one that stays short takes what the price moved its way");
    assert_eq!((ry.x.legs[0].size, ry.x.a_key, ry.x.b_key), (2 * unit, xk(&l2), xk(&u)));
    let at = |a: &Position| made2.iter().find(|(_, m)| m == a).unwrap().0;
    assert_eq!(s2.tx.outputs[at(&z2)].covenant.map(|c| c.covenant_id), x2.id, "the new position under the first's id");
    assert_eq!(s2.tx.outputs[at(&ry)].covenant.map(|c| c.covenant_id), y2.id);

    // 3. The two that stay are one user (two opposite positions of the same two): nothing is written, both are paid;
    // and what is left of the larger goes on.
    let (x3, y3) = (live(position(&u, &s1, 3, p1, 0), 0x74), live(position(&s1, &u, 3, p2, 0), 0x75));
    let (b, nv3, made3) = novate(&lib, &x3, &y3, &script, payer).unwrap();
    signed("two opposite positions of the same two users close each other", &b, &w.keys);
    assert!(nv3.z.is_none() && made3.is_empty());
    assert_eq!(nv3.to_stayer, 120 * KAS1 - nv3.transfer);
    assert_eq!(nv3.to_leaver + nv3.to_stayer + NOVATE_TIP, (x3.value + y3.value) as i64);
    let y4 = live(position(&s1, &u, 5, p2, 0), 0x76);
    let (b, nv4, made4) = novate(&lib, &x3, &y4, &script, payer).unwrap();
    signed("... and what is left of the larger goes on", &b, &w.keys);
    assert!(nv4.z.is_none() && nv4.rest_y.is_some() && made4.len() == 1);

    // 4. What protects those who stay. L posts what S asked but not what S gives up as well: refused by the tool, and by
    // the script when built without the tool's rules.
    let refused = |x: &OLive<Position>, y: &OLive<Position>, word: &str, w: &World| {
        assert!(novate(&lib, x, y, &script, payer).unwrap_err().to_string().contains(word), "{word}");
        let (b, _, _) = novate_with(&lib, x, y, &script, payer, false).unwrap();
        let fs = b.sign(&w.keys).unwrap();
        assert!(!verify(&fs.tx, &fs.entries).is_ok(), "the script accepts it: {word}");
    };
    let mut thin_l = y.s;
    thin_l.x.a_margin = 3 * 8 * KAS1;
    refused(&x, &live(thin_l, 0x77), "less than the one that stays asked", &w);
    // S is left less than L asked of its counterparty.
    let mut thin_s = x.s;
    thin_s.x.b_margin = 5 * 8 * KAS1;
    refused(&live(thin_s, 0x78), &y, "less than the one that comes in asked", &w);
    // An entry far from the market, agreed between U and L: S's margin moves to U only as far as L's stands behind it.
    let far = live(position(&l2, &u, 3, p2 + 15 * (px / 10), 8 * KAS1), 0x79);
    refused(&x, &far, "less than the one that stays asked", &w);
    // Not the same user in the middle; another dispute period.
    assert!(novate(&lib, &x, &live(position(&l2, &kp(34), 3, p2, 8 * KAS1), 0x7a), &script, payer).is_err());
    let other = Template { dispute_daa: tpl.dispute_daa + 1, ..tpl }.position(xk(&l2), xk(&u), 3 * unit, p2, 60 * KAS1, 60 * KAS1);
    assert!(novate(&lib, &x, &live(other, 0x7b), &script, payer).is_err());
}

/// Review T1b, F10 and F11. An order or a position is written by whoever funds it, in anybody's name and at any entry.
/// What a library's scripts put together is of one covenant id, its registry, which an object enters only through an anchor
/// with its owner's signature (a position: both parties'). An object written beside the registry is of another id, and is
/// put together with nothing; so a merge and a novation are nobody's signature.
#[test]
fn what_is_put_together_is_of_one_registry_and_enters_it_signed() {
    use kpf_protocol::oracle_build::OPlan;
    use kpf_protocol::thin::merged;
    use kpf_protocol::tx::Arg;
    use kpf_protocol::verify::verify;
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (a, b, thief) = (kp(41), kp(42), kp(43));
    let other: [u8; 32] = [0xF0; 32];
    let live = |s: Position, n: u8, id: [u8; 32]| OLive { s, value: s.pot() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(id)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // The second object as it is on chain when it was written beside the registry: under another id.
    let beside = |bld: &OBuilt, s: Position, w: &World, what: &str| {
        let mut f = bld.clone();
        f.entries[1] = kaspa_consensus_core::tx::UtxoEntry::new(s.pot() as u64, object_spk(&lib, &TObjT::Position(s)), 6_200, false, Some(kaspa_consensus_core::Hash::from_bytes(other)));
        let fs = f.sign(&w.keys).unwrap();
        assert!(!verify(&fs.tx, &fs.entries).is_ok(), "{what}");
    };

    // The merge. A and B have 5 units from the market's price, 200 KAS each. A alone writes a second position of the
    // two: 10 units from a quarter of the market's price, a KAS each. Merged, B's margin would pay A's false gain.
    let real = tpl.position(xk(&a), xk(&b), 5 * unit, px, 200 * KAS1, 200 * KAS1);
    let fake = tpl.position(xk(&a), xk(&b), 10 * unit, px / 4, KAS1, KAS1);
    let one = merged(&real, &fake).unwrap();
    let b_after = (one.x.b_margin - one.x.legs[0].gain(px)).max(0);
    let gain = fake.x.legs[0].gain(px);
    assert!(gain > 10 * KAS1 && real.x.b_margin - b_after >= gain.min(real.x.b_margin) - 2 * KAS1, "at the market's price B would be owed {b_after} of its {}: A's false gain is {gain}", real.x.b_margin);
    assert!(merge(&lib, &live(real, 0x81, REGISTRY), &live(fake, 0x82, other), &w.script("KpfTMerge"), payer).unwrap_err().to_string().contains("not of one registry"));
    let (bld, _) = merge(&lib, &live(real, 0x81, REGISTRY), &live(fake, 0x82, REGISTRY), &w.script("KpfTMerge"), payer).unwrap();
    signed("a merge of two positions of one registry: nobody's signature", &bld, &w.keys);
    beside(&bld, fake, &w, "the script merges a position of another id into one of the registry");

    // The novation. U is long 3 against S from the market's price. S alone writes a position in which U sold 3 to S's
    // second key at half the market's price, to take U out at that price.
    let (u, s1) = (b, a);
    let x = tpl.position(xk(&u), xk(&s1), 3 * unit, px, 60 * KAS1, 60 * KAS1);
    let y = tpl.position(xk(&thief), xk(&u), 3 * unit, px / 2, 60 * KAS1, KAS1);
    assert!(novate(&lib, &live(x, 0x83, REGISTRY), &live(y, 0x84, other), &w.script("KpfTNovate"), payer).unwrap_err().to_string().contains("not of one registry"));
    let (bld, nv, _) = novate(&lib, &live(x, 0x83, REGISTRY), &live(y, 0x84, REGISTRY), &w.script("KpfTNovate"), payer).unwrap();
    assert!(nv.to_leaver < x.x.a_margin - 10 * KAS1, "U would be paid {} of its {}", nv.to_leaver, x.x.a_margin);
    beside(&bld, y, &w, "the script takes a user out against a position of another id");

    // The way in. An order enters through an anchor with its owner's signature, and nobody else's in its place.
    let alone = |k: &Keypair, w: &World| {
        let mut ks = KeySet::new();
        ks.add_keypair(w.w);
        ks.add_keypair(*k);
        ks
    };
    let forged = |bld: &OBuilt, at: usize, by: &Keypair, w: &World, what: &str| {
        let mut f = bld.clone();
        if let OPlan::Contract { args, .. } = &mut f.plans[1] {
            assert!(matches!(args[at], Arg::Sig(_)), "{what}: the argument at {at} is a signature");
            args[at] = Arg::Sig(xk(by));
        }
        let fs = f.sign(&alone(by, w)).unwrap();
        assert!(!verify(&fs.tx, &fs.entries).is_ok(), "{what}");
    };
    let o = Order { tpl: tpl.hash(), owner: xk(&a), dir: LONG, size: 3 * unit, limit: px, margin: 60 * KAS1, tip: KAS1, expiry: 9_000, min_fill: 0, min_counter: 60 * KAS1 };
    let placed = order_place(&lib, &w.anchor, &w.script("KpfTPlace"), &o, payer).unwrap();
    assert!(placed.sign(&alone(&thief, &w)).is_err(), "an order in A's name is not placed without A");
    forged(&placed, 13, &thief, &w, "the script takes another's signature for the owner's");
    let ps = signed("an order enters the registry", &placed, &alone(&a, &w));
    let registry = w.anchor.id;
    assert_eq!((ps.tx.outputs[0].covenant.map(|c| c.covenant_id), ps.tx.outputs[0].value), (registry, o.value() as u64));
    assert_eq!((ps.tx.outputs[2].covenant.map(|c| c.covenant_id), ps.tx.outputs[2].value, &ps.tx.outputs[2].script_public_key), (registry, w.anchor.value, &object_spk(&lib, &TObjT::Anchor)), "the anchor goes on");
    // One object enters at a time: a second written under the registry's id in the same transaction is refused (here an
    // position in B's name, paid for from the sender's change).
    let mut two = placed.clone();
    let mut extra = two.tx.outputs[0].clone();
    extra.script_public_key = object_spk(&lib, &TObjT::Position(fake));
    extra.value = fake.pot() as u64;
    let change = two.tx.outputs.len() - 1;
    two.tx.outputs[change].value -= extra.value;
    two.tx.outputs.push(extra);
    let ts = two.sign(&alone(&a, &w)).unwrap();
    assert!(!verify(&ts.tx, &ts.entries).is_ok(), "an anchor lets a second object in beside the one signed for");
    // A position enters with both parties' signatures.
    let opened = open(&lib, &w.anchor, &w.script("KpfTOpen"), &real, &[], &xk(&b), false, payer).unwrap();
    assert!(opened.sign(&alone(&a, &w)).is_err(), "a position of A and B is not opened without B");
    forged(&opened, 6, &a, &w, "the script takes A's signature for B's");
    forged(&opened, 5, &b, &w, "the script takes B's signature for A's");
    let mut both = alone(&a, &w);
    both.add_keypair(b);
    let os = signed("a position enters the registry", &opened, &both);
    assert_eq!(os.tx.outputs[0].covenant.map(|c| c.covenant_id), registry);
    // It holds at least its two margins.
    for (v, ok) in [(real.pot() as u64, true), (real.pot() as u64 - 1, false)] {
        let mut t = opened.clone();
        t.tx.outputs[0].value = v;
        let ts = t.sign(&both).unwrap();
        assert_eq!(verify(&ts.tx, &ts.entries).is_ok(), ok, "a position opened holding {v}");
    }
}

// ---------------------------------------------------------------- two markets into one position

/// The same two users have a position in each of two markets: they join them into one position of two legs, one pot
/// of margin for both. Each leg keeps its size and entry, so at every pair of prices each party is owed what the two
/// positions owed it; when the two were the other way round in the second, its leg is short for the first's A.
#[test]
fn two_positions_of_two_markets_become_one_cross_position() {
    use kpf_protocol::thin::{joined, merged, MERGE_TIP};
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (a, b) = (kp(51), kp(52));
    // Both users sign a join (review T1b, F15).
    w.keys.add_keypair(a);
    w.keys.add_keypair(b);
    // A second market of the same committee.
    let tpl2 = Template { market: [0x33; 32], ..tpl };
    let live = |s: Position, n: u8| OLive { s, value: s.pot() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let script = w.script("KpfTJoin");
    let (p1, p2) = (px, px + px / 7);
    let x = Position { a_min_counter: 50 * KAS1, b_min_counter: 40 * KAS1, ..tpl.position(xk(&a), xk(&b), 5 * unit, p1, 100 * KAS1, 100 * KAS1) };
    let half = MERGE_TIP / 2;
    // What A is owed before the caps: its margin and its legs' gains.
    let raw = |position: &Position, q1: i64, q2: i64| position.x.a_margin + position.x.legs[0].gain(q1) + if position.x.legs[1].size > 0 { position.x.legs[1].gain(q2) } else { 0 };

    // 1. The keys in the same places: A is long in both markets.
    let y = Position { a_min_counter: 30 * KAS1, b_min_counter: 20 * KAS1, ..tpl2.position(xk(&a), xk(&b), 3 * unit, p2, 60 * KAS1, 70 * KAS1) };
    let (bld, one) = join(&lib, &live(x, 0x91), &live(y, 0x92), &script, payer).unwrap();
    let s = signed("two markets into one position", &bld, &w.keys);
    assert_eq!(one, joined(&x, &y).unwrap());
    assert_eq!((one.x.legs[0], one.x.legs[1]), (x.x.legs[0], y.x.legs[0]));
    assert_eq!((one.x.a_margin, one.x.b_margin, one.a_min_counter, one.b_min_counter), (160 * KAS1 - half, 170 * KAS1 - half, 80 * KAS1, 60 * KAS1));
    assert_eq!(s.tx.outputs[0].value as i64, x.pot() + y.pot() - MERGE_TIP);
    for k in 0..30 {
        for m in 0..30 {
            let (q1, q2) = (px / 2 + k * (px / 20) + k, px / 2 + m * (px / 17) + m);
            assert_eq!(raw(&one, q1, q2), raw(&x, q1, 0) + y.x.a_margin + y.x.legs[0].gain(q2) - half, "A at {q1}, {q2}");
        }
    }
    // The cross position is a position: its owner adds margin to it.
    let joined_live = nth(&s, 0, one, 6_300);
    let fa = w.pay(&a);
    let (bld, _) = add(&lib, &joined_live, &w.script("KpfTAdd"), SIDE_A, KAS1, Payer { funds: &fa, change_to: xk(&a), rate: MIN_FEE_RATE }).unwrap();
    signed("add margin to the cross position", &bld, &w.keys);

    // 2. The keys the other way round in the second (B long against A there): its leg is short for A.
    let t = Position { a_min_counter: 30 * KAS1, b_min_counter: 20 * KAS1, ..tpl2.position(xk(&b), xk(&a), 3 * unit, p2, 60 * KAS1, 70 * KAS1) };
    let (bld, two) = join(&lib, &live(x, 0x91), &live(t, 0x93), &script, payer).unwrap();
    signed("... the second the other way round", &bld, &w.keys);
    assert_eq!((two.x.legs[1].dir, two.x.legs[1].size, two.x.legs[1].entry), (SHORT, 3 * unit, p2));
    assert_eq!((two.x.a_margin, two.x.b_margin, two.a_min_counter, two.b_min_counter), (170 * KAS1 - half, 160 * KAS1 - half, 70 * KAS1, 70 * KAS1));
    for k in 0..30 {
        for m in 0..30 {
            let (q1, q2) = (px / 2 + k * (px / 20) + k, px / 2 + m * (px / 17) + m);
            // In `t` A is the B: it is owed its margin there less what `t`'s A gained.
            assert_eq!(raw(&two, q1, q2), raw(&x, q1, 0) + t.x.b_margin - t.x.legs[0].gain(q2) - half, "A at {q1}, {q2}");
        }
    }

    // 3. Not joined: one market (that is a merge), other terms, another user, a position of two legs already, another
    // id; and a cross position is neither merged nor joined again.
    assert!(join(&lib, &live(x, 0x91), &live(tpl.position(xk(&a), xk(&b), unit, p1, 20 * KAS1, 20 * KAS1), 0x94), &script, payer).unwrap_err().to_string().contains("one market"));
    assert!(join(&lib, &live(x, 0x91), &live(Template { dispute_daa: tpl.dispute_daa + 1, ..tpl2 }.position(xk(&a), xk(&b), unit, p2, 20 * KAS1, 20 * KAS1), 0x95), &script, payer).is_err());
    assert!(join(&lib, &live(x, 0x91), &live(tpl2.position(xk(&a), xk(&kp(53)), unit, p2, 20 * KAS1, 20 * KAS1), 0x96), &script, payer).is_err());
    assert!(join(&lib, &live(one, 0x97), &live(y, 0x92), &script, payer).unwrap_err().to_string().contains("two legs already"));
    assert!(merged(&one, &x).is_err());
    let mut elsewhere = live(y, 0x92);
    elsewhere.id = Some(kaspa_consensus_core::Hash::from_bytes([0xF0; 32]));
    assert!(join(&lib, &live(x, 0x91), &elsewhere, &script, payer).unwrap_err().to_string().contains("not of one registry"));
    // The script holds the second to what it is: joined as if its keys were the other way round, it is refused.
    let (good, _) = join(&lib, &live(x, 0x91), &live(y, 0x92), &script, payer).unwrap();
    let mut forged = good.clone();
    if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut forged.plans[2] {
        args[6] = kpf_protocol::tx::Arg::V(silverscript_abi::ArtifactValue::Int(1));
    }
    let fs = forged.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "the script joins a position with its sides turned");
    // Nobody joins two users' positions for them: not a third key, not one of the two alone, and the script takes
    // no signature in another's place.
    let mut alone = KeySet::new();
    alone.add_keypair(w.w);
    alone.add_keypair(a);
    assert!(good.sign(&alone).is_err(), "a join is not signed by one of the two");
    for (at, by) in [(11usize, b), (12, a)] {
        let mut f = good.clone();
        if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut f.plans[2] {
            assert!(matches!(args[at], kpf_protocol::tx::Arg::Sig(_)));
            args[at] = kpf_protocol::tx::Arg::Sig(xk(&by));
        }
        let fs = f.sign(&w.keys).unwrap();
        assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "the script takes one user's signature for the other's");
    }
}

// ---------------------------------------------------------------- one side of a position of two legs

/// A position of two legs is not locked: a position of the same two users is folded into its first leg (added, or set
/// against it) and the position is written the other way round, by anyone; the first leg is sold to a third user with
/// the position's two users' signatures (review T1b, F16). No price is shown. The user that sells a leg is paid nothing
/// of the position: its margin and what it made on the leg stay behind the leg it keeps.
#[test]
fn one_side_of_a_cross_position_is_added_to_set_against_and_sold() {
    use kpf_protocol::thin::{folded, joined, left, turned_legs, turned_users, MERGE_TIP, NOVATE_TIP};
    use kpf_protocol::verify::verify;
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let tpl2 = Template { market: [0x33; 32], ..tpl };
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s1, l2) = (kp(61), kp(62), kp(63));
    let live = |s: Position, n: u8| OLive { s, value: s.pot() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // The turns and the folds below are nobody's signature (only the sender's key is among the signers); a leg that
    // leaves is signed by the position's two users, whose keys are added there.
    let mut sender = KeySet::new();
    for k in [w.a, w.b, w.w, kp(9)] {
        sender.add_keypair(k);
    }
    let asking = |a: Position, qa: i64, qb: i64| Position { a_min_counter: qa, b_min_counter: qb, ..a };
    // What the position's A gains at a price of each market (in the legs' order).
    let gains = |a: &Position, q: [i64; 2]| a.x.legs.iter().zip(q).filter(|(l, _)| l.size > 0).map(|(l, p)| l.gain(p)).sum::<i64>();
    let grid = |px: i64| (0..12).flat_map(move |k| (0..12).map(move |m| [px / 2 + k * (px / 9) + k, px / 2 + m * (px / 7) + m]));
    let half = MERGE_TIP / 2;
    let (p1, p2) = (px, px + px / 7);

    // The cross position: U long 5 of the first market from p1, and short 3 of the second from p2, against S.
    let x1 = asking(tpl.position(xk(&u), xk(&s1), 5 * unit, p1, 100 * KAS1, 100 * KAS1), 40 * KAS1, 40 * KAS1);
    let y1 = asking(tpl2.position(xk(&s1), xk(&u), 3 * unit, p2, 60 * KAS1, 60 * KAS1), 24 * KAS1, 24 * KAS1);
    let cross = joined(&x1, &y1).unwrap();
    assert_eq!((cross.x.legs[0].dir, cross.x.legs[1].dir, cross.x.legs[1].size), (LONG, SHORT, 3 * unit));
    let cl = live(cross, 0xA1);

    // 1. The same position written the other way round.
    let (b, tu) = turn(&lib, &cl, &w.script("KpfTTurn"), true, payer).unwrap();
    signed("a position with its users in each other's places", &b, &w.keys);
    assert_eq!((tu, turned_users(&tu)), (turned_users(&cross), cross));
    let (b, tl) = turn(&lib, &cl, &w.script("KpfTTurn"), false, payer).unwrap();
    signed("a position with its legs in each other's places", &b, &w.keys);
    assert_eq!((tl.x.legs, turned_legs(&tl).unwrap()), ([cross.x.legs[1], cross.x.legs[0]], cross));
    for q in grid(px) {
        assert_eq!(cross.x.a_margin + gains(&cross, q), tu.x.b_margin - gains(&tu, q), "the users turned, at {q:?}");
        assert_eq!(gains(&cross, q), gains(&tl, [q[1], q[0]]), "the legs turned, at {q:?}");
    }
    assert!(turn(&lib, &live(x1, 0xA0), &w.script("KpfTTurn"), false, payer).is_err(), "a position of one leg has no legs to turn");
    // It takes a position of some age: the same turn stating no age is refused by the script.
    let mut young = b.clone();
    if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut young.plans[1] {
        args[1] = kpf_protocol::tx::Arg::V(silverscript_abi::ArtifactValue::Bytes(0i64.to_le_bytes().to_vec()));
    }
    let ys = young.sign(&w.keys).unwrap();
    assert!(!verify(&ys.tx, &ys.entries).is_ok(), "a turn of a position of no age");

    // 2. Folded into the first leg. The same way: U long 2 more, from another entry.
    let more = asking(tpl.position(xk(&u), xk(&s1), 2 * unit, p1 + px / 50 + 3, 40 * KAS1, 40 * KAS1), 16 * KAS1, 16 * KAS1);
    let (b, f1) = fold(&lib, &cl, &live(more, 0xA2), &w.script("KpfTFold"), payer).unwrap();
    let sg = signed("a fill added to the first leg of a cross position", &b, &w.keys);
    assert_eq!(f1, folded(&cross, &more).unwrap());
    assert_eq!((f1.x.legs[0].size, f1.x.legs[0].dir, f1.x.legs[1], f1.b_min_counter), (7 * unit, LONG, cross.x.legs[1], cross.b_min_counter + 16 * KAS1));
    assert!((0..=unit).contains(&(f1.a_min_counter - cross.a_min_counter - 16 * KAS1)), "the asks together (and the sompi the entry's rounding moved)");
    assert_eq!((f1.pot(), sg.tx.outputs[0].value as i64), (cross.pot() + more.pot() - MERGE_TIP, cross.pot() + more.pot() - MERGE_TIP));
    for q in grid(px) {
        let apart = cross.x.a_margin + gains(&cross, q) + more.x.a_margin + more.x.legs[0].gain(q[0]) - half;
        assert!((f1.x.a_margin + gains(&f1, q) - apart).abs() <= 2, "added, at {q:?}");
    }
    // The other way: S long 2 against U (U sold 2). The leg is 3; what U made on the 2 is in its margin.
    let against = tpl.position(xk(&s1), xk(&u), 2 * unit, p1 + px / 20, 40 * KAS1, 40 * KAS1);
    let (b, f2) = fold(&lib, &cl, &live(against, 0xA3), &w.script("KpfTFold"), payer).unwrap();
    signed("a fill set against the first leg of a cross position", &b, &w.keys);
    assert_eq!((f2.x.legs[0].size, f2.x.legs[0].dir, f2.x.legs[0].entry), (3 * unit, LONG, p1));
    assert!(f2.x.a_margin > cross.x.a_margin + against.x.b_margin - half, "U sold above its entry: its margin has the gain");
    // What U's margin took from S's is added to what S asks of whoever takes U's place (review T1b, F18).
    let took = f2.x.a_margin - (cross.x.a_margin + against.x.b_margin - half);
    assert_eq!((f2.b_min_counter, f2.a_min_counter), (cross.b_min_counter + against.a_min_counter + took, cross.a_min_counter + against.b_min_counter));
    // More than the leg: S long 8 against U. U is short 3 from the new entry. And exactly the leg: it is gone, and the
    // position is of one leg (the second market's).
    let over = tpl.position(xk(&s1), xk(&u), 8 * unit, p1 + px / 20, 160 * KAS1, 160 * KAS1);
    let (b, f3) = fold(&lib, &cl, &live(over, 0xA4), &w.script("KpfTFold"), payer).unwrap();
    signed("a fill larger than the leg turns it", &b, &w.keys);
    assert_eq!((f3.x.legs[0].size, f3.x.legs[0].dir, f3.x.legs[0].entry), (3 * unit, SHORT, p1 + px / 20));
    let whole = tpl.position(xk(&s1), xk(&u), 5 * unit, p1 + px / 20, 100 * KAS1, 100 * KAS1);
    let (b, f4) = fold(&lib, &cl, &live(whole, 0xA5), &w.script("KpfTFold"), payer).unwrap();
    signed("a fill that cancels the leg leaves a position of one leg", &b, &w.keys);
    assert_eq!((f4.x.legs[0], f4.x.legs[1].size), (cross.x.legs[1], 0));
    for (other, name) in [(&against, "against"), (&over, "over"), (&whole, "whole")] {
        let after = folded(&cross, other).unwrap();
        for q in grid(px) {
            // In `other` U is the B: it is owed its margin there less what the other's A gained.
            let apart = cross.x.a_margin + gains(&cross, q) + other.x.b_margin - other.x.legs[0].gain(q[0]) - half;
            let q_after = if name == "whole" { [q[1], 0] } else { q };
            assert!((after.x.a_margin + gains(&after, q_after) - apart).abs() <= 2, "{name}, at {q:?}");
        }
    }
    // Not folded: another market in the first place, a third user, another id, two positions of one leg that cancel whole.
    assert!(fold(&lib, &cl, &live(tpl2.position(xk(&u), xk(&s1), unit, p2, 20 * KAS1, 20 * KAS1), 0xA6), &w.script("KpfTFold"), payer).is_err());
    assert!(fold(&lib, &cl, &live(tpl.position(xk(&u), xk(&l2), unit, p1, 20 * KAS1, 20 * KAS1), 0xA6), &w.script("KpfTFold"), payer).is_err());
    let mut elsewhere = live(more, 0xA2);
    elsewhere.id = Some(kaspa_consensus_core::Hash::from_bytes([0xF0; 32]));
    assert!(fold(&lib, &cl, &elsewhere, &w.script("KpfTFold"), payer).unwrap_err().to_string().contains("not of one registry"));
    assert!(folded(&x1, &whole).unwrap_err().to_string().contains("cancel whole"));

    // 3. The first leg sold to a third user. U sells its 5 to L above its entry: L long 5 against U. U and S sign; L
    // does not.
    w.keys.add_keypair(u);
    w.keys.add_keypair(s1);
    let pe = p1 + px / 50;
    let y = asking(tpl.position(xk(&l2), xk(&u), 5 * unit, pe, 100 * KAS1, 100 * KAS1), 10 * KAS1, 30 * KAS1);
    let (b, lv) = leave(&lib, &cl, &live(y, 0xB1), &w.script("KpfTLeave"), payer).unwrap();
    let sg = signed("the first leg of a cross position is sold to a third user", &b, &w.keys);
    assert_eq!(lv, left(&cross, cross.pot(), &y, y.pot(), true).unwrap());
    // Not the sender alone; and no signature in another's place (the other user's, or the third user's).
    assert!(b.sign(&sender).is_err(), "a leg does not leave a position its users did not sign for");
    for (at, by) in [(12usize, s1), (13, u), (12, l2), (13, l2)] {
        let mut f = b.clone();
        if let kpf_protocol::oracle_build::OPlan::Contract { args, .. } = &mut f.plans[2] {
            assert!(matches!(args[at], kpf_protocol::tx::Arg::Sig(_)));
            args[at] = kpf_protocol::tx::Arg::Sig(xk(&by));
        }
        let mut ks = KeySet::new();
        for k in [w.w, kp(9), u, s1, l2] {
            ks.add_keypair(k);
        }
        let fs = f.sign(&ks).unwrap();
        assert!(!verify(&fs.tx, &fs.entries).is_ok(), "the script takes another's signature for a user's");
    }
    let ((xa, xav), (z, zv)) = (lv.x_after, lv.z);
    assert!(lv.gain > 0 && lv.n == 5 * unit && lv.rest_y.is_none());
    assert_eq!((xa.x.legs[0], xa.x.legs[1].size, xa.x.a_key, xa.x.b_key), (cross.x.legs[1], 0, xk(&u), xk(&s1)), "the position is of its second leg alone");
    assert_eq!(xa.x.a_margin, cross.x.a_margin + lv.gain, "the seller's margin stays, with what it made");
    assert_eq!((xa.b_min_counter, xa.a_min_counter), (cross.b_min_counter + lv.gain, cross.a_min_counter), "what stays keeps both asks, and S's has what U's margin took");
    assert_eq!((z.x.a_key, z.x.b_key, z.x.legs[0].size, z.x.legs[0].dir, z.x.legs[0].entry, z.x.a_margin, z.a_min_counter), (xk(&l2), xk(&s1), 5 * unit, LONG, pe, 100 * KAS1, 10 * KAS1));
    assert_eq!(xa.x.b_margin + z.x.b_margin, cross.x.b_margin - lv.gain, "S's margin is in the two positions, less what the price moved against it");
    assert_eq!(lv.to_leaver, 100 * KAS1 - NOVATE_TIP, "the seller is paid its margin in the second position and nothing of the first");
    assert_eq!(xav + zv + lv.to_leaver + NOVATE_TIP, cross.pot() + y.pot());
    assert_eq!((sg.tx.outputs[0].value as i64, sg.tx.outputs[1].value as i64, sg.tx.outputs.last().unwrap().value as i64), (xav, zv, lv.to_leaver));
    for q in grid(px) {
        let s_before = cross.x.b_margin - gains(&cross, q);
        let s_after = xa.x.b_margin - gains(&xa, [q[1], 0]) + z.x.b_margin - z.x.legs[0].gain(q[0]);
        assert!((s_before - s_after).abs() <= 2, "S at {q:?}");
        assert_eq!(y.x.a_margin + y.x.legs[0].gain(q[0]), z.x.a_margin + z.x.legs[0].gain(q[0]), "L at {q:?}");
        let u_before = cross.x.a_margin + gains(&cross, q) + y.x.b_margin - y.x.legs[0].gain(q[0]);
        let u_after = xa.x.a_margin + gains(&xa, [q[1], 0]) + lv.to_leaver + NOVATE_TIP;
        assert!((u_before - u_after).abs() <= 2, "U at {q:?}");
    }
    // A part of the leg: the position keeps two legs. A sale larger than the leg: what is left of it goes on.
    let part = asking(tpl.position(xk(&l2), xk(&u), 2 * unit, pe, 40 * KAS1, 40 * KAS1), 4 * KAS1, 12 * KAS1);
    let (b, lp) = leave(&lib, &cl, &live(part, 0xB2), &w.script("KpfTLeave"), payer).unwrap();
    signed("a part of the first leg is sold", &b, &w.keys);
    assert_eq!((lp.x_after.0.x.legs[0].size, lp.x_after.0.x.legs[1], lp.z.0.x.legs[0].size), (3 * unit, cross.x.legs[1], 2 * unit));
    let big = asking(tpl.position(xk(&l2), xk(&u), 8 * unit, pe, 160 * KAS1, 160 * KAS1), 16 * KAS1, 48 * KAS1);
    let (b, lb) = leave(&lib, &cl, &live(big, 0xB3), &w.script("KpfTLeave"), payer).unwrap();
    signed("a sale larger than the leg", &b, &w.keys);
    let (ry, _) = lb.rest_y.unwrap();
    assert_eq!((lb.n, ry.x.legs[0].size, ry.x.a_key, ry.x.b_key, ry.x.a_margin, lb.to_leaver), (5 * unit, 3 * unit, xk(&l2), xk(&u), 60 * KAS1, 100 * KAS1 - NOVATE_TIP));
    // The leg in which U is short: the legs turned, U buys 3 of the second market from L, and S is the new position's A.
    let tlive = live(tl, 0xB4);
    let cover = asking(tpl2.position(xk(&u), xk(&l2), 3 * unit, p2 - px / 40, 60 * KAS1, 60 * KAS1), 18 * KAS1, 6 * KAS1);
    let (b, ls) = leave(&lib, &tlive, &live(cover, 0xB5), &w.script("KpfTLeave"), payer).unwrap();
    signed("the leg in which the user is short is bought back from a third user", &b, &w.keys);
    assert!(ls.gain > 0, "U sold at p2 and bought below it");
    assert_eq!((ls.z.0.x.a_key, ls.z.0.x.b_key, ls.z.0.x.legs[0].dir, ls.z.0.x.b_margin, ls.x_after.0.x.legs[0]), (xk(&s1), xk(&l2), LONG, 60 * KAS1, cross.x.legs[0]));
    // The position's B sells: the users are turned first.
    let b_sells = asking(tpl2.position(xk(&l2), xk(&s1), 3 * unit, p2, 60 * KAS1, 60 * KAS1), 6 * KAS1, 18 * KAS1);
    assert!(leave(&lib, &tlive, &live(b_sells, 0xB6), &w.script("KpfTLeave"), payer).is_err());
    let s_first = turned_users(&tl);
    assert!(left(&s_first, s_first.pot(), &b_sells, b_sells.pot(), true).is_ok(), "S, long 3 of the second market, sells them once it is the position's A");
    // What protects those who stay: L posts less than S asked for the share and U's gain; S brings less than L asked.
    let refused = |y: Position, word: &str, w: &World| {
        assert!(leave(&lib, &cl, &live(y, 0xB7), &w.script("KpfTLeave"), payer).unwrap_err().to_string().contains(word), "{word}");
        let (b, _) = leave_with(&lib, &cl, &live(y, 0xB7), &w.script("KpfTLeave"), payer, false).unwrap();
        let fs = b.sign(&w.keys).unwrap();
        assert!(!verify(&fs.tx, &fs.entries).is_ok(), "the script accepts it: {word}");
    };
    let mut thin_l = y;
    thin_l.x.a_margin = KAS1;
    refused(thin_l, "less than the one that stays asked", &w);
    refused(Position { a_min_counter: 1_000 * KAS1, ..y }, "less than the one that comes in asked", &w);
    // The third user is the position's other user: that is a fold. A position of one leg: that is a novation.
    assert!(leave(&lib, &cl, &live(against, 0xB8), &w.script("KpfTLeave"), payer).unwrap_err().to_string().contains("pos-fold"));
    assert!(leave(&lib, &live(x1, 0xA0), &live(y, 0xB1), &w.script("KpfTLeave"), payer).unwrap_err().to_string().contains("one leg"));
}

/// Review T1b, F18. A user of a cross position sells its winning leg into the other's own resting order; anyone folds
/// that in (the leg cancels, the gain is in the user's margin, the loser is the position's one leg); the user is then
/// taken out against a second key of its own. What that key posts is what the other asks: since the fold adds what the
/// margin took, it is the user's whole margin, and the other is owed what it was.
#[test]
fn a_gain_taken_into_a_margin_is_asked_of_whoever_takes_that_users_place() {
    use kpf_protocol::thin::{folded, joined, novated};
    let w = world();
    let tpl = Template::of(&w.position.s);
    let tpl2 = Template { market: [0x33; 32], ..tpl };
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, s1, k2) = (kp(71), kp(72), kp(73));
    let asking = |a: Position, qa: i64, qb: i64| Position { a_min_counter: qa, b_min_counter: qb, ..a };
    // A leg is 10 units; U and S post the same a leg, each asking what the other posts; they joined. U is long both.
    let m = 10 * unit;
    let post = tpl.position(xk(&u), xk(&s1), m, px, 0, 0).x.legs[0].worth(px) / 5;
    let x = asking(tpl.position(xk(&u), xk(&s1), m, px, post, post), post, post);
    let y = asking(tpl2.position(xk(&u), xk(&s1), m, px, post, post), post, post);
    let cross = joined(&x, &y).unwrap();
    // The first market has risen 35 %, the second fallen 65 %. S has a buy of the first resting at the price, with a
    // small margin, asking as much: U sells its ten units into it.
    let (q1, q2) = (px + px * 35 / 100, px - px * 65 / 100);
    let small = post / 4;
    let sold = asking(tpl.position(xk(&s1), xk(&u), m, q1, small, small), small, small);
    let one = folded(&cross, &sold).unwrap();
    assert_eq!((one.x.legs[0], one.x.legs[1].size), (cross.x.legs[1], 0), "the loser is the position's one leg");
    let gain = cross.x.legs[0].gain(q1);
    assert!(gain > 0 && one.x.a_margin >= cross.x.a_margin + small + gain - 1_000_000);
    assert!(one.b_min_counter >= one.x.a_margin, "S asks of whoever takes U's place all that stands in U's margin: {} of {}", one.b_min_counter, one.x.a_margin);
    // What S is owed at the two prices, the cap counted.
    let owed_s = |a: &Position, p: i64| (a.x.b_margin - a.x.legs[0].gain(p)).clamp(0, a.pot());
    let before = owed_s(&one, q2);
    // U's second key opens against U at the loser's own entry (U long in the position: the second key is long against
    // U), posting what S asks and no more; anyone takes U out.
    let thin_key = asking(tpl2.position(xk(&k2), xk(&u), m, px, one.b_min_counter, 2_000_000), 0, 0);
    let nv = novated(&one, one.pot(), &thin_key, thin_key.pot(), true).unwrap();
    let (z, _) = nv.z.unwrap();
    assert_eq!(owed_s(&z, q2), before, "S is owed what it was");
    // With less than that it is not taken out.
    let thinner = asking(tpl2.position(xk(&k2), xk(&u), m, px, one.b_min_counter - gain, 2_000_000), 0, 0);
    assert!(novated(&one, one.pot(), &thinner, thinner.pot(), true).unwrap_err().to_string().contains("less than the one that stays asked"));
}

/// What two users sign (a close by both, a join, a leg that leaves, an opening) is signed on two machines: the first
/// signs with its key, the second adds its own, and the transaction is, to the byte, the one a machine with both keys
/// makes. A script that waits for a signature is not run by the first; whoever signs last runs every script.
#[test]
fn what_two_users_sign_is_signed_on_two_machines() {
    use kpf_protocol::thin::Template;
    use kpf_protocol::verify::verify;
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let tpl2 = Template { market: [0x33; 32], ..tpl };
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (a, b, l, ex) = (kp(81), kp(82), kp(83), kp(84));
    let only = |ks: &[Keypair]| {
        let mut s = KeySet::new();
        for k in ks {
            s.add_keypair(*k);
        }
        s
    };
    let live = |s: Position, n: u8| OLive { s, value: s.pot() as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }, daa: 6_200 };
    let asking = |x: Position| Position { a_min_counter: x.x.b_margin, b_min_counter: x.x.a_margin, ..x };
    let x = asking(tpl.position(xk(&a), xk(&b), 5 * unit, px, 100 * KAS1, 100 * KAS1));
    let y = asking(tpl2.position(xk(&a), xk(&b), 3 * unit, px, 60 * KAS1, 70 * KAS1));
    let cross = kpf_protocol::thin::joined(&x, &y).unwrap();
    // A sold the first leg's 5 units to L: L long against A.
    let sold = asking(tpl.position(xk(&l), xk(&a), 5 * unit, px + px / 50, 200 * KAS1, 60 * KAS1));
    let (fa, fe) = (w.pay(&a), w.pay(&ex));
    let (s_both, s_join, s_leave, s_open) = (w.script("KpfTBoth"), w.script("KpfTJoin"), w.script("KpfTLeave"), w.script("KpfTOpen"));
    let anchor = Anchor { s: (), value: KAS1 as u64, id: Some(kaspa_consensus_core::Hash::from_bytes(REGISTRY)), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([0xA7; 32]), index: 0 }, daa: 6_000 };
    let fresh = asking(tpl.position(xk(&a), xk(&b), 2 * unit, px, 30 * KAS1, 30 * KAS1));
    let fb = vec![fund(xk(&b), 0xB7, 31)];
    type Mk<'a> = Box<dyn Fn(Swap) -> kpf_protocol::Result<OBuilt> + 'a>;
    // Each: its builder, and the key that pays (A, but for the third, which an executor pays for and neither user).
    let cases: Vec<(&str, Mk, Keypair)> = vec![
        ("a close by both", Box::new(|f: Swap| both_close(&lib, &OLive { s: swap_position(x, f), ..live(x, 0x71) }, &s_both, 90 * KAS1, 110 * KAS1, Payer { funds: &swap_funds(&fa, f), change_to: f(xk(&a)), rate: MIN_FEE_RATE })), a),
        ("a join", Box::new(|f: Swap| join(&lib, &OLive { s: swap_position(x, f), ..live(x, 0x71) }, &OLive { s: swap_position(y, f), ..live(y, 0x72) }, &s_join, Payer { funds: &swap_funds(&fa, f), change_to: f(xk(&a)), rate: MIN_FEE_RATE }).map(|r| r.0)), a),
        ("a leg that leaves, paid for by an executor", Box::new(|f: Swap| leave(&lib, &OLive { s: swap_position(cross, f), ..live(cross, 0x73) }, &OLive { s: swap_position(sold, f), ..live(sold, 0x74) }, &s_leave, Payer { funds: &swap_funds(&fe, f), change_to: f(xk(&ex)), rate: MIN_FEE_RATE }).map(|r| r.0)), ex),
        ("an opening, the counterparty's margin from its own coin", Box::new(|f: Swap| open(&lib, &anchor, &s_open, &swap_position(fresh, f), &swap_funds(&fb, f), &f(xk(&b)), true, Payer { funds: &swap_funds(&fa, f), change_to: f(xk(&a)), rate: MIN_FEE_RATE })), a),
    ];
    for (what, mk, payer) in &cases {
        let whole = mk(&|k| k).unwrap().sign(&only(&[a, b, ex])).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(verify(&whole.tx, &whole.entries).is_ok(), "{what}");
        // Nobody alone signs it whole.
        assert!(mk(&|k| k).unwrap().sign(&only(&[a, ex])).is_err() && mk(&|k| k).unwrap().sign(&only(&[b, ex])).is_err(), "{what}");
        // The first machine: A's key (and the executor's nowhere yet, where it pays).
        let (part, size) = sign_apart(mk.as_ref(), &only(&[a])).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(size, whole.mass.size, "{what}: the size");
        assert!(part.open_sigs.iter().any(|(_, _, k)| *k == xk(&b)) && !part.open_sigs.iter().any(|(_, _, k)| *k == xk(&a)), "{what}: B's signature is what the script waits for");
        part.check_signed().unwrap();
        assert!(part.finished().is_err(), "{what}: not whole yet");
        // It is a file: what the second machine reads is what the first wrote.
        let part: kpf_protocol::oracle_build::OPartial = serde_json::from_str(&serde_json::to_string(&part).unwrap()).unwrap();
        // A key that is not asked changes nothing.
        let same = part.sign_more(&only(&[l])).unwrap();
        assert_eq!((same.open.len(), same.open_sigs.len()), (part.open.len(), part.open_sigs.len()));
        // The second machine: B's key; the third, where an executor pays: its own.
        let mut p2 = part.sign_more(&only(&[b])).unwrap();
        if *payer == ex {
            assert!(!p2.open.is_empty() && p2.finished().is_err(), "{what}: the payer's inputs are still open");
            p2 = p2.sign_more(&only(&[ex])).unwrap();
        }
        let done = p2.finished().unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(verify(&done.tx, &done.entries).is_ok(), "{what}: {:?}", verify(&done.tx, &done.entries).errors());
        assert_eq!(done.tx, whole.tx, "{what}: the transaction of one machine, to the byte");
        // B's signature of another transaction (the same with one sompi less in an output) does not serve.
        let mut other = part.clone();
        let last = other.tx.outputs.len() - 1;
        other.tx.outputs[last].value -= 1;
        let sig_of_other = other.sign_more(&only(&[b, ex])).unwrap();
        let mut forged = part.sign_more(&only(&[ex])).unwrap();
        for (i, at, k) in &part.open_sigs {
            if *k == xk(&b) {
                let s = sig_of_other.tx.inputs[*i].signature_script[*at..*at + 65].to_vec();
                forged.tx.inputs[*i].signature_script[*at..*at + 65].copy_from_slice(&s);
            }
        }
        forged.open_sigs.clear();
        forged.open.retain(|(_, k)| *k != xk(&b));
        if forged.open.is_empty() {
            let f = forged.finished().unwrap();
            assert!(!verify(&f.tx, &f.entries).is_ok(), "{what}: a signature of another transaction passed");
        }
    }
}

/// Every entry of the library's scripts that these tests run is read back whole at least once (`read_back`, called
/// on every transaction they sign): this test runs them again in its own thread and counts.
#[test]
fn every_entry_these_tests_run_is_read_back_from_its_transaction() {
    a_true_claim_is_declared_and_settled_after_margin_was_added();
    a_false_claim_is_disputed_by_the_second_executor_and_lapses();
    a_late_claim_is_ended_and_the_two_parties_close_together();
    a_liquidation_is_declared_and_cured_or_settled();
    a_false_liquidation_is_disputed_and_lapses_or_is_cured_under_dispute();
    orders_of_two_users_match_one_to_one_and_what_is_left_rests();
    a_match_holds_both_limits_one_template_and_an_order_that_does_not_rest_is_returned();
    two_positions_of_the_same_two_users_are_merged_by_anyone();
    a_user_leaves_two_positions_and_the_two_it_faced_face_each_other();
    what_is_put_together_is_of_one_registry_and_enters_it_signed();
    two_positions_of_two_markets_become_one_cross_position();
    one_side_of_a_cross_position_is_added_to_set_against_and_sold();
    let (whole, blind) = SEEN.with(|s| (s.borrow().whole.clone(), s.borrow().blind.clone()));
    println!("read whole: {whole:?}");
    println!("not read (a position nobody published): {blind:?}");
    let want = [
        "KpfTAdd.add", "KpfTBoth.both", "KpfTCancel.cancel", "KpfTCancel.expire", "KpfTCure.cure", "KpfTClose.close", "KpfTDispute.dispute", "KpfTDispute.lapse", "KpfTFold.fold", "KpfTJoin.join",
        "KpfTLate.late", "KpfTLeave.leave", "KpfTLiq.liquidate", "KpfTLiqDispute.dispute", "KpfTLiqDispute.lapse", "KpfTLiqSettle.settle", "KpfTMatch.match", "KpfTMerge.merge", "KpfTNovate.novate",
        "KpfTOpen.open", "KpfTPlace.place", "KpfTSettle.settle", "KpfTTurn.turn",
    ];
    let missing: Vec<&str> = want.iter().copied().filter(|n| !whole.contains(*n)).collect();
    assert!(missing.is_empty(), "never read whole: {missing:?}");
}


/// `KpfTReplace`: a user leaves a position by an order of its own, and the owner of the order it is matched with takes
/// its place, in one transaction. What a match and a novation did in two, without the position between the leaver and
/// the newcomer.
#[test]
fn two_orders_matched_into_a_position_put_the_newcomer_in_the_leavers_place() {
    use kpf_protocol::thin::{replaced, Rest};
    use kpf_protocol::thin_build::{replace, replace_with};
    use kpf_protocol::verify::verify;
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, st, nw) = (kp(41), kp(42), kp(43));
    w.keys.add_keypair(u);
    let reg = kaspa_consensus_core::Hash::from_bytes(REGISTRY);
    // 20 KAS of margin a unit from each party; each asks `ask` a unit of whoever faces it.
    let position = |a: &Keypair, b: &Keypair, units: i64, entry: i64, ask: i64| Position { a_min_counter: units * ask, b_min_counter: units * ask, ..tpl.position(xk(a), xk(b), units * unit, entry, units * 20 * KAS1, units * 20 * KAS1) };
    let order = |owner: &Keypair, dir: i64, units: i64, limit: i64, margin: i64, ask: i64| Order { tpl: tpl.hash(), owner: xk(owner), dir, size: units * unit, limit, margin: units * margin, tip: units * KAS1 / 20, expiry: 9_000, min_fill: 0, min_counter: units * ask };
    let op = |n: u8| TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 };
    let lp = |a: Position, n: u8| OLive { s: a, value: a.pot() as u64, id: Some(reg), op: op(n), daa: 6_200 };
    let lo = |o: Order, n: u8| OLive { s: o, value: o.value() as u64, id: Some(reg), op: op(n), daa: 6_200 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let script = w.script("KpfTReplace");
    let (p1, p2) = (px, px + px / 100 + 3);
    let tip = |units: i64| 2 * units * KAS1 / 20;

    // 1. U is long 5 against S from p1. It sells 3 at p2 (the price rose) to a newcomer: the newcomer is long 3
    //    against S from p2, S's margin gives up what the price moved, 2 units of the position go on, U is paid.
    let x = lp(position(&u, &st, 5, p1, 8 * KAS1), 0x81);
    let (sell, buy) = (lo(order(&u, SHORT, 3, p2, 20 * KAS1, 0), 0x82), lo(order(&nw, LONG, 3, p2, 20 * KAS1, 8 * KAS1), 0x83));
    let (b, r) = replace(&lib, &x, &buy, &sell, &script, 3 * unit, p2, SIDE_A, payer).unwrap();
    let s = signed("a long leaves 3 of its 5 by an order", &b, &w.keys);
    println!("replace (a rest of the position, a new position): {} bytes", s.tx.inputs.iter().map(|i| i.signature_script.len()).sum::<usize>());
    let ((z, zv), (rx, rxv)) = (r.z.unwrap(), r.rest.unwrap());
    assert!(r.gain > 0 && r.long == Rest::Nothing && r.short == Rest::Nothing);
    assert_eq!((z.x.a_key, z.x.b_key, z.x.legs[0].size, z.x.legs[0].entry), (xk(&nw), xk(&st), 3 * unit, p2));
    assert_eq!((z.x.a_margin, z.x.b_margin), (60 * KAS1, 60 * KAS1 - r.gain));
    assert_eq!((z.a_min_counter, z.b_min_counter), (24 * KAS1, 24 * KAS1), "the newcomer's ask and what the one that stays asked");
    assert_eq!((rx.x.legs[0].size, rx.x.legs[0].entry, rx.x.a_margin, rx.x.b_margin, rx.a_min_counter), (2 * unit, p1, 40 * KAS1, 40 * KAS1, 16 * KAS1));
    assert_eq!((r.leaver, r.to_leaver, r.to_stayer), (xk(&u), 120 * KAS1 + r.gain, 0), "the leaver: its margin in the position, its order's, and what the price moved");
    assert_eq!(zv + rxv + r.to_leaver + r.tips, (x.value + buy.value + sell.value) as i64, "nothing else leaves the three");
    assert_eq!(r.tips, tip(3));
    assert_eq!((s.tx.outputs[0].value as i64, s.tx.outputs[1].value as i64, s.tx.outputs.last().unwrap().value as i64), (rxv, zv, r.to_leaver));
    assert_eq!(s.tx.outputs.last().unwrap().script_public_key, p2pk_spk(&xk(&u)));
    // The one that stays is worth at every price what it was; the newcomer what a match at p2 would have given it.
    for k in 0..400 {
        let p = px / 2 + k * (px / 300) + k;
        let before = x.s.x.b_margin - x.s.x.legs[0].gain(p);
        let after = z.x.b_margin - z.x.legs[0].gain(p) + rx.x.b_margin - rx.x.legs[0].gain(p);
        assert!((before - after).abs() <= 2, "the one that stays at {p}: {before} against {after}");
        assert_eq!(z.x.a_margin + z.x.legs[0].gain(p), 60 * KAS1 + tpl.position(xk(&nw), xk(&st), 3 * unit, p2, 0, 0).x.legs[0].gain(p), "the newcomer at {p}");
    }

    // 2. A short leaves: S is long 4 against U from p2; U buys 4 back at p1 (the price fell: U gains) from a newcomer
    //    that sells. The whole position is replaced; orders larger than the match rest.
    let x = lp(position(&st, &u, 4, p2, 8 * KAS1), 0x84);
    let (buy, sell) = (lo(order(&u, LONG, 6, p1, 20 * KAS1, 0), 0x85), lo(order(&nw, SHORT, 7, p1, 20 * KAS1, 8 * KAS1), 0x86));
    let (b, r) = replace(&lib, &x, &buy, &sell, &script, 4 * unit, p1, SIDE_B, payer).unwrap();
    let s = signed("a short leaves by an order, and both orders rest", &b, &w.keys);
    let (z, zv) = r.z.unwrap();
    assert!(r.rest.is_none() && r.gain > 0);
    assert_eq!((z.x.a_key, z.x.b_key, z.x.legs[0].size, z.x.legs[0].entry), (xk(&st), xk(&nw), 4 * unit, p1));
    assert_eq!((z.x.a_margin, z.x.b_margin), (80 * KAS1 - r.gain, 80 * KAS1));
    assert_eq!((r.leaver, r.to_leaver), (xk(&u), 160 * KAS1 + r.gain));
    let (Rest::Order(bl, blv), Rest::Order(sl, slv)) = (r.long, r.short) else { panic!("both orders rest") };
    assert_eq!((bl.size, bl.margin, sl.size, sl.margin, sl.min_counter), (2 * unit, 40 * KAS1, 3 * unit, 60 * KAS1, 24 * KAS1));
    assert_eq!(zv + r.to_leaver + r.tips + blv + slv, (x.value + buy.value + sell.value) as i64);
    assert_eq!((s.tx.outputs[0].value as i64, s.tx.outputs[1].value as i64, s.tx.outputs[2].value as i64), (zv, blv, slv));
    for k in 0..400 {
        let p = px / 2 + k * (px / 300) + k;
        assert!((x.s.x.a_margin + x.s.x.legs[0].gain(p) - (z.x.a_margin + z.x.legs[0].gain(p))).abs() <= 2, "the long that stays at {p}");
    }

    // 3. Four objects are left under the registry's id: a rest of the position, the new one, and both orders' rests.
    let x = lp(position(&u, &st, 5, p1, 8 * KAS1), 0x87);
    let (sell, buy) = (lo(order(&u, SHORT, 4, p1, 20 * KAS1, 0), 0x88), lo(order(&nw, LONG, 6, p2, 20 * KAS1, 8 * KAS1), 0x89));
    let (b, r) = replace(&lib, &x, &buy, &sell, &script, 2 * unit, p1 + 1, SIDE_A, payer).unwrap();
    let s = signed("a rest of everything", &b, &w.keys);
    assert!(r.rest.is_some() && r.z.is_some() && matches!(r.long, Rest::Order(..)) && matches!(r.short, Rest::Order(..)));
    assert_eq!(s.tx.outputs.iter().filter(|o| o.covenant.map(|c| c.covenant_id) == Some(reg)).count(), 4);

    // 4. The newcomer is the one that stays: U (long 3 against S) sells 3, and the buyer is S. Nothing is written of
    //    them: the two are paid.
    let x = lp(position(&u, &st, 3, p1, 8 * KAS1), 0x8a);
    let (sell, buy) = (lo(order(&u, SHORT, 3, p2, 20 * KAS1, 0), 0x8b), lo(order(&st, LONG, 3, p2, 20 * KAS1, 8 * KAS1), 0x8c));
    let (b, r) = replace(&lib, &x, &buy, &sell, &script, 3 * unit, p2, SIDE_A, payer).unwrap();
    let s = signed("the one that stays buys the leaver out", &b, &w.keys);
    assert!(r.z.is_none() && r.rest.is_none());
    assert_eq!((r.stayer, r.to_stayer, r.to_leaver), (xk(&st), 120 * KAS1 - r.gain, 120 * KAS1 + r.gain));
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 2].value as i64, &s.tx.outputs[n - 2].script_public_key), (r.to_stayer, &p2pk_spk(&xk(&st))));
    assert_eq!((s.tx.outputs[n - 1].value as i64, &s.tx.outputs[n - 1].script_public_key), (r.to_leaver, &p2pk_spk(&xk(&u))));
    assert!(s.tx.outputs.iter().all(|o| o.covenant.map(|c| c.covenant_id) != Some(reg)), "nothing under the registry's id");

    // What the tool refuses, and the script with it.
    let x = lp(position(&u, &st, 5, p1, 8 * KAS1), 0x8d);
    let sell = lo(order(&u, SHORT, 3, p1, 20 * KAS1, 0), 0x8e);
    let refused = |b: OBuilt| !verify(&b.sign(&w.keys).unwrap().tx, &b.sign(&w.keys).unwrap().entries).is_ok();
    // The newcomer posts less than the one that stays asked of its counterparty (8 a unit), or asks more of it than
    // it is left with.
    let thin = lo(order(&nw, LONG, 3, p2, 7 * KAS1, 0), 0x8f);
    assert!(replace(&lib, &x, &thin, &sell, &script, 3 * unit, p1, SIDE_A, payer).unwrap_err().to_string().contains("less than the one that stays asked"));
    assert!(refused(replace_with(&lib, &x, &thin, &sell, &script, 3 * unit, p1, SIDE_A, payer, false).unwrap().0));
    let greedy = lo(order(&nw, LONG, 3, p2, 20 * KAS1, 21 * KAS1), 0x90);
    assert!(replace(&lib, &x, &greedy, &sell, &script, 3 * unit, p1, SIDE_A, payer).unwrap_err().to_string().contains("less than the one that comes in asks"));
    assert!(refused(replace_with(&lib, &x, &greedy, &sell, &script, 3 * unit, p1, SIDE_A, payer, false).unwrap().0));
    // The price rose by what the one that stays posts: its margin does not cover the move, and nobody takes the place.
    let far = p1 + 21 * unit.min(p1) * 0 + 21 * (KAS1 / unit.max(1)).max(1) * unit;
    let _ = far;
    let buy = lo(order(&nw, LONG, 3, p2, 20 * KAS1, 8 * KAS1), 0x91);
    // The order the other way is not the leaver's: side b's order would be the long one, and the long one is the newcomer's.
    assert!(replace(&lib, &x, &buy, &sell, &script, 3 * unit, p1, SIDE_B, payer).unwrap_err().to_string().contains("no order the other way"));
    // Somebody else's short order does not take U out of its position.
    let others = lo(order(&st, SHORT, 3, p1, 20 * KAS1, 0), 0x92);
    assert!(replace(&lib, &x, &buy, &others, &script, 3 * unit, p1, SIDE_A, payer).is_err());
    // More than the position has; outside a limit; an order of another template; a position of another registry.
    let (big_sell, big_buy) = (lo(order(&u, SHORT, 6, p1, 20 * KAS1, 0), 0x93), lo(order(&nw, LONG, 6, p2, 20 * KAS1, 8 * KAS1), 0x94));
    assert!(replace(&lib, &x, &big_buy, &big_sell, &script, 6 * unit, p1, SIDE_A, payer).unwrap_err().to_string().contains("within what both orders and the position have"));
    assert!(replace(&lib, &x, &buy, &sell, &script, 3 * unit, p1 - 1, SIDE_A, payer).is_err(), "below the short's limit");
    assert!(replace(&lib, &x, &buy, &sell, &script, 3 * unit, p2 + 1, SIDE_A, payer).is_err(), "above the long's limit");
    let alien = Order { tpl: [9; 32], ..buy.s };
    assert!(replace(&lib, &x, &lo(alien, 0x95), &sell, &script, 3 * unit, p1, SIDE_A, payer).is_err());
    let elsewhere = OLive { id: Some(kaspa_consensus_core::Hash::from_bytes([7; 32])), ..x.clone() };
    assert!(replace(&lib, &elsewhere, &buy, &sell, &script, 3 * unit, p1, SIDE_A, payer).is_err());
    // The model alone: a leaver that lost more than its two margins hold is not paid a negative amount.
    let deep = position(&u, &st, 3, p2 * 3, 0);
    let cheap = order(&u, SHORT, 3, 1, KAS1 / 100, 0);
    assert!(replaced(&deep, deep.pot(), &order(&nw, LONG, 3, p1, 20 * KAS1, 0), order(&nw, LONG, 3, p1, 20 * KAS1, 0).value(), &cheap, cheap.value(), 3 * unit, p1, SIDE_A, true).is_err());
}

/// The script of a replacement and its model agree on random positions and orders: what the model gives is taken by
/// the engine, and what only the two conditions of the ones that stay and come in refuse is refused by it.
#[test]
fn a_replacement_agrees_with_its_model_on_random_cases() {
    use kpf_protocol::thin::replaced;
    use kpf_protocol::thin_build::replace_with;
    use kpf_protocol::verify::verify;
    let mut w = world();
    let lib = w.lib;
    let tpl = Template::of(&w.position.s);
    let unit = tpl.scale;
    let px = w.position.s.x.legs[0].entry;
    let (u, st, nw) = (kp(51), kp(52), kp(53));
    w.keys.add_keypair(u);
    let reg = kaspa_consensus_core::Hash::from_bytes(REGISTRY);
    let op = |n: u8| TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 };
    let f = w.pay(&w.w.clone());
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let script = w.script("KpfTReplace");
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = |lo: i64, hi: i64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        lo + (seed % (hi - lo) as u64) as i64
    };
    let (mut taken, mut by_rule, mut by_model) = (0, 0, 0);
    for case in 0..240 {
        let side = if rnd(0, 2) == 0 { SIDE_A } else { SIDE_B };
        let np = rnd(1, 9) * unit + rnd(0, 2) * rnd(0, unit);
        let entry = rnd(px / 2, px * 2);
        let price = entry / 100 * rnd(70, 131) + rnd(0, 1_000);
        let (am, bm) = (rnd(KAS1, 60 * KAS1), rnd(KAS1, 60 * KAS1));
        let (a, b) = if side == SIDE_A { (&u, &st) } else { (&st, &u) };
        let position = Position { a_min_counter: rnd(0, 2) * rnd(0, am), b_min_counter: rnd(0, 2) * rnd(0, bm), ..tpl.position(xk(a), xk(b), np, entry, am, bm) };
        if position.check().is_err() {
            continue;
        }
        let who = if rnd(0, 8) == 0 { &st } else { &nw };
        let mut order = |owner: &Keypair, dir: i64| {
            let size = rnd(1, 12) * unit;
            Order { tpl: tpl.hash(), owner: xk(owner), dir, size, limit: if dir == LONG { price + rnd(0, 50) } else { price - rnd(0, 50) }, margin: rnd(KAS1, 80 * KAS1), tip: rnd(0, KAS1), expiry: 9_000, min_fill: 0, min_counter: rnd(0, 2) * rnd(0, 30 * KAS1) }
        };
        let (long, short) = if side == SIDE_A { (order(who, LONG), order(&u, SHORT)) } else { (order(&u, LONG), order(who, SHORT)) };
        if long.check().is_err() || short.check().is_err() {
            continue;
        }
        let n = np.min(long.size).min(short.size) - rnd(0, 2) * rnd(0, unit / 2);
        let x = OLive { s: position, value: position.pot() as u64 + rnd(0, 2) as u64 * 20_000_000, id: Some(reg), op: op(0xa1), daa: 6_200 };
        let (lv, sv) = (OLive { s: long, value: long.value() as u64, id: Some(reg), op: op(0xa2), daa: 6_200 }, OLive { s: short, value: short.value() as u64, id: Some(reg), op: op(0xa3), daa: 6_200 });
        let say = format!("case {case}: side {side}, {n} of {np} from {entry} at {price}, margins {am} {bm}");
        let strict = replaced(&position, x.value as i64, &long, long.value(), &short, short.value(), n, price, side, true);
        let loose = replace_with(&lib, &x, &lv, &sv, &script, n, price, side, payer, false);
        let Ok((b, _)) = loose else {
            by_model += 1;
            assert!(strict.is_err(), "{say}: the builder refuses what the model gives");
            continue;
        };
        let sg = b.sign(&w.keys).unwrap();
        let ok = verify(&sg.tx, &sg.entries).is_ok();
        assert_eq!(ok, strict.is_ok(), "{say}: the engine and the model differ ({:?})", strict.as_ref().err());
        if ok {
            taken += 1;
        } else {
            by_rule += 1;
        }
    }
    println!("random replacements: {taken} taken, {by_rule} refused by the two conditions, {by_model} refused by the model before a transaction");
    assert!(taken >= 30 && by_rule >= 10 && by_model >= 10, "the cases do not reach every branch: {taken} {by_rule} {by_model}");
}
