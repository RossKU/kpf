//! The reserve, first part (`KpfTShardOpen`, `KpfTOpenC`, `KpfTLegAdd`, `KpfTAddC`, `KpfTNoteApply`, `KpfTAcctEnd`,
//! and the notes a covered account's statements carry; docs/ARCHITECTURE-CLEARING.md 6a): a clearing party's shard,
//! accounts that name it, and the notes between them. No draw yet: a shard is funded, counts the legs it covers, and
//! pays what a user is owed. Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::cross::{XLeg, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::{Head, SNote, Shard, ShardRow, NOTE_DEPOSIT, NOTE_JOIN, NOTE_MIN, NOTE_OWED, NOTE_RELEASE, SHARD_CARRIER};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{ATake, Account, Out, Outs, Stated, KIND_ACCOUNT_LEAVE, KIND_ACCOUNT_LIQ};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;

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
/// Built, signed, accepted by the engine, with a sompi less at any output a script fixes refused, and nothing made
/// or lost but the fee.
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    for k in 0..b.tx.outputs.len() {
        if Some(k) == b.change || b.output_labels[k] == "filler" {
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
    u: Keypair,
    c: Keypair,
    w: Keypair,
    f: Keypair,
    keys: KeySet,
    markets: Vec<CMarket>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    anchors: Vec<Anchor>,
}

impl World {
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
    fn leg(&self, i: usize, units: i64, dir: i64) -> XLeg {
        let m = &self.markets[i];
        XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry: P10 }
    }
    /// The clearing party's shard, opened through the first anchor with 50 KAS.
    fn shard(&self) -> OLive<Shard> {
        let f = [fund(xk(&self.c), 40)];
        let (b, shard) = shard_open(&self.lib, &self.anchors[0], &self.script("KpfTShardOpen"), self.terms(), xk(&self.c), 50 * KAS1, 9_000_000, 50, 100, self.payer(&self.c, &f)).unwrap();
        let s = signed("shard opened", &b, &self.keys);
        nth(&s, 0, shard, 6_100)
    }
    /// A covered account of the user (A) against the clearing party (B), opened through the second anchor with 20
    /// and 2 KAS of margin and no leg.
    fn account(&self, sid: [u8; 32]) -> OLive<Account> {
        let a = Account { t: self.terms(), legs: vec![], a_key: xk(&self.u), b_key: xk(&self.c), a_margin: 20 * KAS1, b_margin: 2 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: Some(sid) };
        let f = [fund(xk(&self.u), 41)];
        let b = account_open_c(&self.lib, &self.anchors[1], &self.script("KpfTOpenC"), &a, self.payer(&self.u, &f)).unwrap();
        let s = signed("covered account opened", &b, &self.keys);
        nth(&s, 0, a, 6_100)
    }
    /// The leg added to the account: the account as it then is, and the note that the leg entered.
    fn added(&self, account: &OLive<Account>, leg: XLeg) -> (OLive<Account>, OLive<SNote>) {
        let f = [fund(xk(&self.u), 42)];
        let (b, next, note) = leg_add(&self.lib, account, &self.script("KpfTLegAdd"), leg, 0, 0, self.payer(&self.u, &f)).unwrap();
        let s = signed("leg added", &b, &self.keys);
        (nth(&s, 0, next, 6_110), nth(&s, 2, note, 6_110))
    }
    fn applied(&self, shard: &OLive<Shard>, note: &OLive<SNote>, what: &str) -> (OSigned, OLive<Shard>, Option<OLive<SNote>>) {
        let f = [fund(xk(&self.w), 43)];
        let (b, next, rest) = note_apply(&self.lib, shard, note, &self.script("KpfTNoteApply"), self.payer(&self.w, &f)).unwrap();
        let s = signed(what, &b, &self.keys);
        let rest = rest.map(|r| nth(&s, 2, r, shard.daa + 10));
        let live = nth(&s, 0, next, shard.daa + 10);
        (s, live, rest)
    }
    /// The statement made on the account and applied after the dispute period: the settlement.
    fn settled(&self, account: &OLive<Account>, st: &Stated, tk: &ATake, what: &str) -> OSigned {
        let who = if st.claimer == xk(&self.u) { &self.u } else if st.claimer == xk(&self.c) { &self.c } else { &self.w };
        let f = [fund(xk(who), 44)];
        let b = account_state(&self.lib, account, &self.script("KpfTChange"), st, &tk.said(), self.payer(who, &f)).unwrap();
        let s = signed(&format!("{what}: stated"), &b, &self.keys);
        let live = nth(&s, 0, *st, 6_510);
        let f = [fund(xk(&self.w), 45)];
        let b = stated_settle(&self.lib, &live, &self.script("KpfTChangeSettle"), self.payer(&self.w, &f)).unwrap();
        signed(&format!("{what}: applied"), &b, &self.keys)
    }
}

fn world() -> World {
    let (u, c, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let mut keys = KeySet::new();
    for k in [&u, &c, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = [0x52; 32];
    let markets = (0..5u8).map(|i| CMarket::testnet(rid, [0xc0 + i; 32])).collect();
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    let funds = vec![fund(xk(&f), 201)];
    let (r, _) = registry_genesis(&lib, 2, KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let rs = r.sign(&keys).expect("registry");
    let anchors = registry_anchors(&rs.tx, &lib, 6_000);
    assert_eq!(anchors.len(), 2);
    World { u, c, w, f, keys, markets, lib, scripts: lib_scripts(&gs.tx, 6_000), anchors }
}

#[test]
fn a_shard_is_opened_by_its_owner_with_an_id_of_its_own_and_funded() {
    let w = world();
    let shard = w.shard();
    assert_eq!(shard.s.sid, shard_id(&w.anchors[0]));
    assert_eq!((shard.value, shard.s.held, shard.s.rows), ((50 * KAS1 + SHARD_CARRIER) as u64, 50 * KAS1, [ShardRow::EMPTY; 4]), "what it holds, and 1 KAS beside");
    assert_eq!(shard.s.moving().len(), 171);
    // Another id than the anchor's outpoint gives, a second key, or something on B's side: refused.
    let f = [fund(xk(&w.c), 40)];
    let (b, s) = shard_open(&w.lib, &w.anchors[0], &w.script("KpfTShardOpen"), w.terms(), xk(&w.c), 50 * KAS1, 9_000_000, 50, 100, w.payer(&w.c, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    for (what, lie) in [("another id", Shard { sid: [0x44; 32], ..s }), ("rows of its own", Shard { rows: [ShardRow { head: Head::of(&w.leg(0, 1, LONG)), net: 5, gross: 5 }, ShardRow::EMPTY, ShardRow::EMPTY, ShardRow::EMPTY], ..s })] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &lie.hash());
        assert!(!accepted(&t, &w.keys), "a shard is opened with {what}");
    }
    // Not by another key than its owner's.
    let mut other = KeySet::new();
    other.add_keypair(w.w);
    other.add_keypair(w.f);
    assert!(b.sign(&other).is_err() || !accepted(&b, &other));
    // The owner funds it (KpfTAddC); nobody else does, and no second side has anything to add to.
    let f = [fund(xk(&w.c), 46)];
    let (b, more) = shard_fund(&w.lib, &shard, &w.script("KpfTAddC"), 30 * KAS1, w.payer(&w.c, &f)).unwrap();
    let s = signed("shard funded", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, more.held), ((80 * KAS1 + SHARD_CARRIER) as u64, 80 * KAS1));
    assert!(b.sign(&other).is_err() || !accepted(&b, &other));
}

#[test]
fn a_leg_enters_a_covered_account_with_its_note_and_the_shard_counts_it() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    assert_eq!(account.s.moving().len(), 171);
    // The user goes long 10 units of market 0: side B is short them.
    let (account, join) = w.added(&account, w.leg(0, 10, LONG));
    let scale = w.markets[0].p.scale;
    assert_eq!(join.s, SNote { sid: shard.s.sid, kind: NOTE_JOIN, head: Head::of(&w.leg(0, 10, LONG)), net: -10 * scale, gross: 10 * scale, key: xk(&w.c), amount: 0 });
    assert_eq!((account.s.legs.len(), join.value), (1, NOTE_MIN as u64));
    // Anyone applies it: the first row is the market's.
    let (_, shard, rest) = w.applied(&shard, &join, "join applied");
    assert!(rest.is_none());
    assert_eq!((shard.s.rows[0].net, shard.s.rows[0].gross, shard.s.rows[0].head), (-10 * scale, 10 * scale, Head::of(&w.leg(0, 1, LONG))));
    // A second leg, short 4 of the same market, and one of another market.
    let (account, join2) = w.added(&account, w.leg(0, 4, SHORT));
    let (_, shard, _) = w.applied(&shard, &join2, "second join applied");
    assert_eq!((shard.s.rows[0].net, shard.s.rows[0].gross), (-6 * scale, 14 * scale), "the net is 6 short, the gross 14");
    let (_, join3) = w.added(&account, w.leg(1, 3, LONG));
    let (_, shard, _) = w.applied(&shard, &join3, "join of another market applied");
    assert_eq!((shard.s.rows[1].net, shard.s.rows[1].gross), (-3 * scale, 3 * scale));

    // The note of an account whose side B is not the shard's owner is applied by nobody: nobody adds to another's
    // net. (Here the user names the clearing party's shard in an account with a third party.)
    let f = [fund(xk(&w.w), 47)];
    let lie = SNote { key: xk(&w.w), ..join.s };
    let forged = OLive { s: lie, ..join.clone() };
    assert!(note_apply(&w.lib, &shard, &forged, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).is_err());
    // And a note to another shard is not this shard's.
    let other = OLive { s: SNote { sid: [0x77; 32], ..join.s }, ..join.clone() };
    assert!(note_apply(&w.lib, &shard, &other, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).is_err());
    // The script refuses a note read as another than it is: its units, its kind, its amount, the row of another
    // market.
    let (b, ..) = note_apply(&w.lib, &shard, &join, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    for (what, lie) in [("more units", SNote { net: 2 * join.s.net, ..join.s }), ("a release", SNote { kind: NOTE_RELEASE, ..join.s }), ("a deposit", SNote { kind: NOTE_DEPOSIT, amount: KAS1, ..join.s })] {
        let as_if = OLive { s: lie, ..join.clone() };
        let r = note_apply(&w.lib, &shard, &as_if, &w.script("KpfTNoteApply"), w.payer(&w.w, &f));
        let mut t = r.expect("built for the note it is said to be").0;
        // (The object spent is the true note: the transaction is built for another, with the true note's script.)
        t.entries[2] = b.entries[2].clone();
        t.plans[2] = b.plans[2].clone();
        assert!(!accepted(&t, &w.keys), "a note that a leg entered is applied as {what}");
    }
}

#[test]
fn a_leg_leaves_with_the_note_of_its_leaving_and_what_the_pot_cannot_pay_is_owed_by_the_shard() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    let (account, join) = w.added(&account, w.leg(0, 10, LONG));
    let (_, shard, _) = w.applied(&shard, &join, "join applied");
    let x = &account.s;
    let held = account.value as i64;
    let scale = w.markets[0].p.scale;
    // The price is 13: the user gained 30 KAS, and the pot holds the clearing party's 2. The user leaves.
    let a = [P10 * 13 / 10];
    let f = [fund(xk(&w.u), 44)];
    let (b, st, tk, stepped) = account_leave(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, 1, &a, T0, w.payer(&w.u, &f)).unwrap();
    signed("leg leaves: stated", &b, &w.keys);
    assert_eq!((st.kind, stepped.fee, stepped.owed), (KIND_ACCOUNT_LEAVE, 0, 28 * KAS1), "30 gained, 2 in the pot: 28 owed");
    assert_eq!((stepped.after.legs.len(), stepped.after.a_margin, stepped.after.b_margin), (0, 22 * KAS1, 0));
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &st.claimer, &st.outs, held, &a), None);
    // Its outputs: the account with no leg, the note that the leg left, and the note of what is owed.
    let release = SNote { sid: shard.s.sid, kind: NOTE_RELEASE, head: Head::of(&x.legs[0]), net: -10 * scale, gross: 10 * scale, key: xk(&w.c), amount: 0 };
    let owed = SNote::owed(shard.s.sid, xk(&w.u), xk(&w.c), 28 * KAS1);
    assert_eq!(st.outs.list().iter().map(|o| o.to).collect::<Vec<_>>(), vec![stepped.after.hash(), release.hash(), owed.hash()]);
    assert!(!st.outs.is_note(0) && st.outs.is_note(1) && st.outs.is_note(2));
    let s = w.settled(&account, &st, &tk, "leg leaves");
    let n = s.tx.outputs.len();
    let lid = Some(account.id.unwrap());
    assert_eq!((s.tx.outputs[0].script_public_key.clone(), s.tx.outputs[0].value), (hash_spk(&w.lib, &stepped.after.hash()), account.value));
    assert_eq!((s.tx.outputs[n - 1].script_public_key.clone(), s.tx.outputs[n - 1].covenant.map(|c| c.covenant_id)), (hash_spk(&w.lib, &release.hash()), lid), "the release, under the account's id");
    assert_eq!((s.tx.outputs[n - 2].script_public_key.clone(), s.tx.outputs[n - 2].covenant.map(|c| c.covenant_id)), (hash_spk(&w.lib, &owed.hash()), lid), "what is owed");
    let (release, owed, ended) = (nth(&s, n - 1, release, 6_570), nth(&s, n - 2, owed, 6_570), nth(&s, 0, stepped.after.clone(), 6_570));

    // The release is applied: the shard's row is empty of units again.
    let (_, shard, _) = w.applied(&shard, &release, "release applied");
    assert_eq!(shard.s.rows[0], ShardRow::EMPTY, "a row with no unit is empty again, for the next market");
    // What is owed is paid by the shard: 28 of its 50 KAS, to the user's key.
    let (s, shard, rest) = w.applied(&shard, &owed, "owed paid");
    let n = s.tx.outputs.len();
    assert!(rest.is_none());
    assert_eq!((shard.s.held, shard.value), (22 * KAS1, (22 * KAS1 + SHARD_CARRIER) as u64));
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (28 * KAS1 as u64, &p2pk_spk(&xk(&w.u))));
    // The account, with no leg, is ended by the user: its 22 KAS to it; the clearing party's side holds nothing.
    let f = [fund(xk(&w.u), 48)];
    let (b, deposit) = acct_end(&w.lib, &ended, &w.script("KpfTAcctEnd"), SIDE_A, w.payer(&w.u, &f)).unwrap();
    assert!(deposit.is_none());
    let s = signed("account ended", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (22 * KAS1 as u64, &p2pk_spk(&xk(&w.u))));
    // In all the user has its 20, and the 30 it gained: 2 of the pot and 28 of the shard.

    // A shard that holds less than is owed pays what it holds, and the note goes on with the rest.
    let thin = OLive { s: Shard { held: 10 * KAS1, ..shard.s }, value: (10 * KAS1 + SHARD_CARRIER) as u64, ..shard.clone() };
    let again = OLive { s: SNote::owed(shard.s.sid, xk(&w.u), xk(&w.c), 28 * KAS1), ..owed.clone() };
    let (s, thin, rest) = w.applied(&thin, &again, "owed paid in part");
    let n = s.tx.outputs.len();
    let rest = rest.expect("the rest goes on");
    assert_eq!((thin.s.held, thin.value, rest.s.amount, s.tx.outputs[n - 1].value), (0, SHARD_CARRIER as u64, 18 * KAS1, 10 * KAS1 as u64), "everything it held is paid, and its output still stands");
    assert_eq!(s.tx.outputs[2].script_public_key, hash_spk(&w.lib, &SNote::owed(shard.s.sid, xk(&w.u), xk(&w.c), 18 * KAS1).hash()));
    // A shard that holds nothing pays nothing, and the note waits.
    let f = [fund(xk(&w.w), 49)];
    assert!(note_apply(&w.lib, &thin, &rest, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).is_err());
}

#[test]
fn a_covered_account_pays_its_clearing_side_nothing_but_by_the_shard() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    let (account, _) = w.added(&account, w.leg(0, 10, LONG));
    let x = &account.s;
    let held = account.value as i64;
    // The price is 9: the clearing side gained 10 KAS of the user's 20. Anyone states a take for side B: its
    // amount is a deposit to the shard, not a payment to the claimer or to B.
    let a = [P10 * 9 / 10];
    let max = x.take_max(SIDE_B, &a);
    assert!(max > 0);
    let (st, tk) = x.take_by(SIDE_B, max, &a, T0, held, xk(&w.w)).unwrap();
    let deposit = SNote::deposit(shard.s.sid, xk(&w.c), max);
    assert_eq!(st.outs.list()[1].to, deposit.hash());
    assert!(st.outs.is_note(1));
    assert_eq!(st.outs.list()[1].value, max + NOTE_MIN);
    assert_eq!(x.wrong_point(&tk, &st.claimer, &st.outs, held, &a), None);
    let s = w.settled(&account, &st, &tk, "the clearing side's gain swept");
    let n = s.tx.outputs.len();
    assert_eq!(s.tx.outputs[n - 1].script_public_key, hash_spk(&w.lib, &deposit.hash()));
    assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&xk(&w.c))), "nothing is paid to the clearing party's key");
    let deposit = nth(&s, n - 1, deposit, 6_570);
    let (_, more, _) = w.applied(&shard, &deposit, "deposit applied");
    assert_eq!((more.s.held, more.value), (50 * KAS1 + max, (50 * KAS1 + max + SHARD_CARRIER) as u64));
    // The same take paid to side B's key, or to the claimer: its end is wrong.
    let after = x.taken(SIDE_B, max, &tk.rows).unwrap();
    for (what, to) in [("B's key", xk(&w.c)), ("the claimer", xk(&w.w))] {
        let lie = Stated { outs: Outs::of(&[Out { key: false, to: after.hash(), value: held - max }, Out { key: true, to, value: max }]).unwrap(), ..st };
        assert!(!x.end_right(&tk, &lie.claimer, &lie.outs, held), "a take for side B of a covered account is paid to {what}");
    }
    // The user's take is its own, as on any account: stated by another key, its end is wrong.
    let up = [P10 * 101 / 100];
    let (mine, tk_a) = x.take(SIDE_A, KAS1 / 2, &up, T0, held).unwrap();
    assert!(x.end_right(&tk_a, &mine.claimer, &mine.outs, held));
    assert!(!x.end_right(&tk_a, &xk(&w.w), &mine.outs, held));

    // An account with no leg ends by either party: A's margin to A, B's as a deposit.
    let empty = w.account(shard.s.sid);
    let f = [fund(xk(&w.c), 48)];
    let (b, deposit) = acct_end(&w.lib, &empty, &w.script("KpfTAcctEnd"), SIDE_B, w.payer(&w.c, &f)).unwrap();
    let deposit = deposit.expect("B's margin");
    let s = signed("empty account ended by the clearing party", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (20 * KAS1 as u64, &p2pk_spk(&xk(&w.u))));
    assert_eq!((deposit.amount, deposit.kind), (2 * KAS1, NOTE_DEPOSIT));
    assert!(s.tx.outputs.iter().any(|o| o.script_public_key == hash_spk(&w.lib, &deposit.hash()) && o.value == (2 * KAS1 + NOTE_MIN) as u64));
    assert!(s.tx.outputs.iter().enumerate().all(|(k, o)| o.script_public_key != p2pk_spk(&xk(&w.c)) || Some(k) == b.change), "nothing but its own change to the clearing party's key");
    // An account with a leg is not ended that way; and not by a third key.
    assert!(acct_end(&w.lib, &account, &w.script("KpfTAcctEnd"), SIDE_A, w.payer(&w.c, &f)).is_err());
    let mut other = KeySet::new();
    other.add_keypair(w.w);
    other.add_keypair(w.f);
    assert!(b.sign(&other).is_err() || !accepted(&b, &other));
}

#[test]
fn a_statement_on_a_covered_account_that_forges_a_note_has_a_wrong_end() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    let (account, _) = w.added(&account, w.leg(0, 10, LONG));
    let (account, _) = w.added(&account, w.leg(1, 5, SHORT));
    let x = &account.s;
    let held = account.value as i64;
    let a = [P10, P10];
    let (st, tk, stepped) = x.step(KIND_ACCOUNT_LEAVE, SIDE_A, 2, &a, T0, held, xk(&w.u)).unwrap();
    assert_eq!(stepped.owed, 0);
    assert!(x.step_end_right(KIND_ACCOUNT_LEAVE, &tk, &st.claimer, &st.outs, held));
    let release = st.outs.list()[1];
    let obj = st.outs.list()[0];
    let note = |n: SNote| (Out { key: false, to: n.hash(), value: n.value() }, true);
    let lies: Vec<(&str, Outs)> = vec![
        // What a user is owed, of the claimer's own making: paid by the shard, were it applied.
        ("a note of 40 KAS owed", Outs::with_notes(&[(obj, false), (release, true), note(SNote::owed(shard.s.sid, xk(&w.u), xk(&w.c), 40 * KAS1))]).unwrap()),
        // No note that the leg left: the shard's net would keep it.
        ("no release", Outs::with_notes(&[(obj, false)]).unwrap()),
        // The release of more units than the leg has.
        ("a release of more units", Outs::with_notes(&[(obj, false), note(SNote { net: 50 * w.markets[1].p.scale, gross: 50 * w.markets[1].p.scale, ..SNote::leg(NOTE_RELEASE, shard.s.sid, &x.legs[1], xk(&w.c)) })]).unwrap()),
        // The release to another shard.
        ("a release to another shard", Outs::with_notes(&[(obj, false), note(SNote::leg(NOTE_RELEASE, [0x66; 32], &x.legs[1], xk(&w.c)))]).unwrap()),
    ];
    let f = [fund(xk(&w.w), 50)];
    for (what, outs) in lies {
        let lie = Stated { outs, ..st };
        assert!(!x.step_end_right(KIND_ACCOUNT_LEAVE, &tk, &lie.claimer, &lie.outs, held), "{what}");
        assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &lie.claimer, &lie.outs, held, &a), Some(0), "{what}");
        // Stated, disputed at its end, and answered wrong by the script: the account again.
        let fu = [fund(xk(&w.u), 44)];
        let b = account_state(&w.lib, &account, &w.script("KpfTChange"), &lie, &tk.said(), w.payer(&w.u, &fu)).unwrap();
        let s = signed(&format!("stated: {what}"), &b, &w.keys);
        let live = nth(&s, 0, lie, 6_510);
        let (b, q) = point_dispute(&w.lib, &live, &w.script("KpfTPoint"), xk(&w.w), 0, w.payer(&w.w, &f)).unwrap();
        let s = signed("disputed at its end", &b, &w.keys);
        let q = nth(&s, 0, q, 6_520);
        let (b, right) = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepLiq"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
        assert!(!right, "{what}");
        let s = signed(&format!("end answered wrong: {what}"), &b, &w.keys);
        assert_eq!(s.tx.outputs[0].script_public_key, hash_spk(&w.lib, &x.hash()), "{what}: the account again");
        let forced = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepLiq"), xk(&w.w), w.payer(&w.w, &f), Some(true));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: answered right");
    }
    // What the statement says of the leaving leg (the release is made of it) is held to the leg by its row: said
    // of another market, the end is right by what is said, and the leg's row is the point that is wrong.
    let other = ATake { leave: Some((Head::of(&x.legs[0]), SNote::units(&x.legs[1]))), ..tk.clone() };
    let rel = SNote { sid: shard.s.sid, kind: NOTE_RELEASE, head: Head::of(&x.legs[0]), net: SNote::units(&x.legs[1]), gross: x.legs[1].size, key: xk(&w.c), amount: 0 };
    let lie = Stated { arg: other.arg(), outs: Outs::with_notes(&[(obj, false), note(rel)]).unwrap(), ..st };
    assert!(x.step_end_right(KIND_ACCOUNT_LEAVE, &other, &lie.claimer, &lie.outs, held));
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &other, &lie.claimer, &lie.outs, held, &a), Some(3), "row 2, the leg that leaves");
    let fu = [fund(xk(&w.u), 44)];
    let b = account_state(&w.lib, &account, &w.script("KpfTChange"), &lie, &other.said(), w.payer(&w.u, &fu)).unwrap();
    let s = signed("stated: a release of another market", &b, &w.keys);
    let (b, q) = point_dispute(&w.lib, &nth(&s, 0, lie, 6_510), &w.script("KpfTPoint"), xk(&w.w), 3, w.payer(&w.w, &f)).unwrap();
    let s = signed("disputed at the leg's row", &b, &w.keys);
    let (b, right) = step_row(&w.lib, &nth(&s, 0, q, 6_520), &other, &x.legs[1], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    signed("the leg's row answered wrong", &b, &w.keys);
    let _ = (KIND_ACCOUNT_LIQ, NOTE_OWED, Hash::default());
}

#[test]
fn a_position_s_scripts_do_not_open_a_covered_account_and_a_take_of_a_position_is_not_stated_on_one() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    let (account, _) = w.added(&account, w.leg(0, 10, LONG));
    let x = &account.s;
    let held = account.value as i64;
    // Kind 1 (a take on a position of two legs) is stated on 138 bytes only: on a covered account it is refused.
    let (st, tk) = x.take(SIDE_A, 0, &[P10], T0, held).unwrap();
    let f = [fund(xk(&w.u), 44)];
    let odd = Stated { kind: 1, arg: kpf_protocol::committee::blake2b(&[7u8; 32]), ..st };
    let b = state_raw(&w.lib, &account, &x.fixed_hash(), &x.moving(), &odd, &[7u8; 32], vec![], &w.script("KpfTChange"), w.payer(&w.u, &f)).unwrap();
    assert!(!accepted(&b, &w.keys), "a take of a position is stated on a covered account");
    // (Its own kind is: the control.)
    let b = account_state(&w.lib, &account, &w.script("KpfTChange"), &st, &tk.said(), w.payer(&w.u, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    // A moving part of another length than a position's or a covered account's is no object's.
    let cut = &x.moving()[..170];
    let b = state_raw(&w.lib, &account, &x.fixed_hash(), cut, &st, &tk.said(), x.form(), &w.script("KpfTChange"), w.payer(&w.u, &f)).unwrap();
    assert!(!accepted(&b, &w.keys));
    // A party adds to its margin by the script of 171 bytes.
    let (b, more) = account_add(&w.lib, &account, &w.script("KpfTAddC"), SIDE_A, 5 * KAS1, w.payer(&w.u, &f)).unwrap();
    let s = signed("margin added to a covered account", &b, &w.keys);
    assert_eq!((more.a_margin, s.tx.outputs[0].value), (25 * KAS1, account.value + 5 * KAS1 as u64));
}

#[test]
fn what_an_account_of_strangers_owes_is_not_paid_by_the_shard_it_names() {
    use kpf_protocol::oracle_build::OPlan;
    use kpf_protocol::tx::Arg;
    use silverscript_abi::ArtifactValue;
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    // An account anybody opens names any shard. Two strangers open one that names this shard, and a statement on it
    // writes a note of 28 KAS owed to one of them: it names the other as who owes it, and the shard's owner is not
    // that.
    let own = OLive { s: SNote::owed(s.sid, xk(&w.u), xk(&w.c), 28 * KAS1), value: NOTE_MIN as u64, id: shard.id, op: op(15), daa: 6_570 };
    let strange = OLive { s: SNote::owed(s.sid, xk(&w.u), xk(&w.w), 28 * KAS1), ..own.clone() };
    let f = [fund(xk(&w.w), 49)];
    assert!(note_apply(&w.lib, &shard, &strange, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).is_err());
    assert!(s.applied(&strange.s, 0).is_err());
    // The script: the transaction that pays the owner's own note, with the strangers' note in its place.
    let (b, _, _) = note_apply(&w.lib, &shard, &own, &w.script("KpfTNoteApply"), w.payer(&w.w, &f)).unwrap();
    assert!(accepted(&b, &w.keys), "the control");
    let mut t = b.clone();
    let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
    assert!(matches!((&args[6], &args[7]), (Arg::V(ArtifactValue::Int(k)), Arg::V(ArtifactValue::Bytes(h))) if *k == NOTE_OWED && *h == own.s.head.bytes()), "the note's kind and head");
    args[7] = Arg::V(ArtifactValue::Bytes(strange.s.head.bytes()));
    let script = kpf_protocol::thin_artifacts::kit().object(&w.lib, &strange.s.hash());
    t.entries[2].script_public_key = kpf_protocol::tx::p2sh_spk(&script);
    if let OPlan::Contract { redeem, .. } = &mut t.plans[2] {
        *redeem = script;
    }
    assert!(!accepted(&t, &w.keys), "a shard pays what an account of strangers owes");
    // And a note that names nobody (the form of before).
    let nobody = SNote { head: Head::EMPTY, ..own.s };
    assert!(s.applied(&nobody, 0).is_err());
}

#[test]
fn a_statement_is_stated_only_in_a_form_whoever_watches_reads() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    let (account, _) = w.added(&account, w.leg(0, 10, LONG));
    let x = &account.s;
    let a = [P10 * 13 / 10];
    let f = [fund(xk(&w.u), 44)];
    let (b, st, tk, _) = account_leave(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, 1, &a, T0, w.payer(&w.u, &f)).unwrap();
    assert!(accepted(&b, &w.keys), "the control");
    let said = tk.said();
    assert_eq!(kpf_protocol::thin::ATake::read(&said), Some(tk));
    // The leg's market written with another push at any of its four places: the model reads no such bytes, and the
    // script takes no such statement.
    for at in [24usize, 57, 90, 99] {
        let mut odd = said.clone();
        odd[at] ^= 1;
        assert!(kpf_protocol::thin::ATake::read(&odd).is_none(), "byte {at}");
        let lie = Stated { arg: kpf_protocol::committee::blake2b(&odd), ..st };
        let b = state_raw(&w.lib, &account, &x.fixed_hash(), &x.moving(), &lie, &odd, x.form(), &w.script("KpfTChange"), w.payer(&w.u, &f)).unwrap();
        assert!(!accepted(&b, &w.keys), "a statement nobody's tool reads is stated (byte {at})");
    }
}

#[test]
fn a_side_is_handed_over_without_the_party_that_stays() {
    let w = world();
    let shard = w.shard();
    let account = w.account(shard.s.sid);
    let (account, _) = w.added(&account, w.leg(0, 10, LONG));
    let x = &account.s;
    // The user leaves; another key takes its place: the leg and its entry stay, the newcomer adds 3 KAS and pays who
    // leaves 25 for the place (the 20 of margin and what the leg gained, as the two see it). Side B signs nothing.
    let v = &w.w;
    let f = [fund(xk(v), 60)];
    let (b, next) = account_hand(&w.lib, &account, &w.script("KpfTHand"), xk(v), 3 * KAS1, 25 * KAS1, w.payer(v, &f)).unwrap();
    // (What who leaves is paid is held by its own signature of the whole transaction, not by the script: the check
    // of a sompi less at every output is the script's, and is made for the account's output alone.)
    let s = b.sign(&w.keys).expect("both keys");
    assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
    println!("side handed over: {} bytes, fee {} sompi", s.mass.size, s.fee);
    let mut less = b.clone();
    less.tx.outputs[0].value -= 1;
    assert!(!accepted(&less, &w.keys), "a sompi less in the account is accepted");
    assert_eq!((next.a_key, next.a_margin, &next.legs, next.b_key, next.b_margin), (xk(v), x.a_margin + 3 * KAS1, &x.legs, x.b_key, x.b_margin));
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value + 3 * KAS1 as u64, &hash_spk(&w.lib, &next.hash())));
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (25 * KAS1 as u64, &p2pk_spk(&xk(&w.u))));
    // Each of the two signs the whole: without either key it is not a transaction.
    for keep in [&w.u, v] {
        let mut one = KeySet::new();
        one.add_keypair(*keep);
        one.add_keypair(w.f);
        assert!(b.sign(&one).is_err() || !accepted(&b, &one), "one signature hands a side over");
    }
    // Less margin than there was, or less than side B asked of whoever takes the place: refused by the tool and by
    // the script.
    assert!(account_hand(&w.lib, &account, &w.script("KpfTHand"), xk(v), -1, 0, w.payer(v, &f)).is_err());
    let asked = OLive { s: Account { b_min_counter: x.a_margin + 5 * KAS1, ..x.clone() }, ..account.clone() };
    assert!(account_hand(&w.lib, &asked, &w.script("KpfTHand"), xk(v), 3 * KAS1, 0, w.payer(v, &f)).is_err());
    let (ok, _) = account_hand(&w.lib, &asked, &w.script("KpfTHand"), xk(v), 5 * KAS1, 0, w.payer(v, &f)).unwrap();
    assert!(accepted(&ok, &w.keys), "the ask is met");
    {
        use kpf_protocol::oracle_build::OPlan;
        use kpf_protocol::tx::iv;
        // (The transaction that meets the ask, said to add 3 KAS: the object as the model has it with 3.)
        let mut t = ok.clone();
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
        args[6] = iv(3 * KAS1);
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { a_key: xk(v), a_margin: x.a_margin + 3 * KAS1, ..asked.s.clone() }.hash());
        assert!(!accepted(&t, &w.keys), "less than side B asked is handed over");
    }
    // A shard is not handed over.
    let odd = account_hand(&w.lib, &OLive { s: Account { b_key: [0; 32], ..x.clone() }, ..account.clone() }, &w.script("KpfTHand"), xk(v), 0, 0, w.payer(v, &f));
    assert!(odd.is_err());
}

#[test]
fn a_leg_moves_between_two_accounts_of_one_clearing_party_or_nothing_does() {
    let w = world();
    let shard = w.shard();
    // The user's account has three legs; another user's account, of the same clearing party under the same shard,
    // has one.
    let mut account = w.account(shard.s.sid);
    for (i, dir) in [(0usize, LONG), (1, SHORT), (2, LONG)] {
        account = w.added(&account, w.leg(i, 10, dir)).0;
    }
    let x = &account.s;
    let other = Account { a_key: xk(&w.w), legs: vec![w.leg(3, 5, LONG)], a_margin: 30 * KAS1, b_margin: 0, ..x.clone() };
    let to = OLive { s: other.clone(), value: other.pot() as u64, id: account.id, op: op(77), daa: 6_200 };
    // Each leg in turn moves (the first has two legs after it, the last none): it leaves the one account and stands
    // last in the other with its entry as it was; here who enters adds 4 KAS and who leaves takes 3 out.
    let f = [fund(xk(&w.w), 61)];
    for i in 1..=3usize {
        let (b, nx, ny) = leg_move(&w.lib, &account, i, &to, [-3 * KAS1, 0, 4 * KAS1, 0], &w.script("KpfTLegMove"), w.payer(&w.w, &f)).unwrap();
        let s = signed(&format!("leg {i} of three moved"), &b, &w.keys);
        let mut rest = x.legs.clone();
        let leg = rest.remove(i - 1);
        assert_eq!((&nx.legs, nx.a_margin, ny.legs.len(), ny.legs[1], ny.a_margin), (&rest, x.a_margin - 3 * KAS1, 2, leg, 34 * KAS1));
        assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value - 3 * KAS1 as u64, &hash_spk(&w.lib, &nx.hash())));
        assert_eq!((s.tx.outputs[2].value, &s.tx.outputs[2].script_public_key), (to.value + 4 * KAS1 as u64, &hash_spk(&w.lib, &ny.hash())));
        // What the shard is asked is as it was: the same units of the same market against the same owner. No note.
        assert!(s.tx.outputs.iter().all(|o| o.script_public_key != hash_spk(&w.lib, &SNote::leg(NOTE_RELEASE, shard.s.sid, &leg, xk(&w.c)).hash())));
        // Without any one of the three keys it is not a transaction.
        for out in 0..3 {
            let mut some = KeySet::new();
            for (k, key) in [&w.u, &w.w, &w.c].iter().enumerate() {
                if k != out {
                    some.add_keypair(**key);
                }
            }
            assert!(b.sign(&some).is_err() || !accepted(&b, &some), "leg {i}: moved without one of the three signatures");
        }
    }
    // An account of another clearing party, or under another shard: refused by the tool; and by the script (the
    // transaction of a move, with the other account said to be under another shard).
    let stranger = OLive { s: Account { b_key: xk(&w.f), ..other.clone() }, ..to.clone() };
    assert!(leg_move(&w.lib, &account, 1, &stranger, [0; 4], &w.script("KpfTLegMove"), w.payer(&w.w, &f)).is_err());
    let elsewhere = Account { shard: Some([0x44; 32]), ..other.clone() };
    assert!(leg_move(&w.lib, &account, 1, &OLive { s: elsewhere.clone(), ..to.clone() }, [0; 4], &w.script("KpfTLegMove"), w.payer(&w.w, &f)).is_err());
    {
        use kpf_protocol::oracle_build::OPlan;
        use kpf_protocol::tx::Arg;
        use silverscript_abi::ArtifactValue;
        let (mut t, _, _) = leg_move(&w.lib, &account, 1, &to, [0; 4], &w.script("KpfTLegMove"), w.payer(&w.w, &f)).unwrap();
        assert!(accepted(&t, &w.keys), "the control");
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
        args[10] = Arg::V(ArtifactValue::Bytes(elsewhere.moving()));
        let script = kpf_protocol::thin_artifacts::kit().object(&w.lib, &elsewhere.hash());
        t.entries[2].script_public_key = kpf_protocol::tx::p2sh_spk(&script);
        if let OPlan::Contract { redeem, .. } = &mut t.plans[2] {
            *redeem = script;
        }
        let mut moved = elsewhere.clone();
        moved.legs.push(x.legs[0]);
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &moved.hash());
        assert!(!accepted(&t, &w.keys), "a leg moves under another shard with no note");
    }
    // A margin below zero: refused.
    assert!(leg_move(&w.lib, &account, 1, &to, [-(x.a_margin + 1), 0, 0, 0], &w.script("KpfTLegMove"), w.payer(&w.w, &f)).is_err());
}

