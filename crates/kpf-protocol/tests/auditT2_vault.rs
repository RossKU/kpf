//! Review T2 (docs/audit/REVIEW-T2-vault.md): the vault of a token leg, at the script engine, with the reference
//! KCC-20's own script running. The reviewer's file: its world and helpers are its own (after `thin_draw.rs`).

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::covered::{
    chain_of_one, CSaid, Ticket, Vault, KIND_C_SPLIT, KIND_C_TOK_OUT, KIND_C_TOK_SOLD,
};
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OPlan, OSigned};
use kpf_protocol::reserve::{Called, RowHead, Shard, ShardRow, CALL_HOLD, NOTE_MIN, SHARD_CARRIER};
use kaspa_txscript::pay_to_script_hash_script;
use kpf_protocol::state::LONG;
use kpf_protocol::thin::{Account, Pointed, Stated};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{KeySet, KeyUtxo, MIN_FEE_RATE};
use kpf_protocol::legacy::kob::TokenState;
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::cell::RefCell;

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
    KeyUtxo { outpoint: op(n), amount: 1_000 * KAS1 as u64, daa: T0 as u64, key }
}
fn accepted(b: &OBuilt, keys: &KeySet) -> bool {
    match b.sign(keys) {
        Ok(sg) => kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(),
        Err(_) => false,
    }
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

struct World {
    c: Keypair,
    w: Keypair,
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: Vec<CMarket>,
    table: OLive<EpochTable>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    chain: RefCell<Vec<(Hash, Hash)>>,
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
    fn head(&self, i: usize) -> RowHead {
        let m = &self.markets[i];
        RowHead { key: kpf_protocol::books::Key::futures(m.id()), scale: m.p.scale, price_max: m.p.price_max }
    }
    fn shard(&self) -> OLive<Shard> {
        let scale = self.markets[0].p.scale;
        let rows = [ShardRow::of(self.head(0), -60 * scale, 140 * scale), ShardRow::of(self.head(1), 10 * scale, 30 * scale), ShardRow::EMPTY, ShardRow::EMPTY];
        let s = Shard { t: self.terms(), rows, owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: -500 * KAS1, called: 0, owed: 0, claims: Default::default() };
        OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(12), daa: 6_100 }
    }
    #[allow(dead_code)]
    fn cited(&self, i: usize, price: i64) -> Cited {
        let market = self.markets[i].id();
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, T0, price)).collect();
        let att = Attestation::collect(&self.table.s, &self.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
        let funds = vec![fund(xk(&self.f), 90)];
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
    fn pointed(&self, st: &OLive<Stated>, point: i64) -> OLive<Pointed> {
        let f = [fund(xk(&self.w), 31)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(&self.w), point, self.payer(&self.w, &f)).unwrap();
        let s = b.sign(&self.keys).expect("pointed");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        nth(&s, 0, q, 6_520)
    }
}

fn world() -> World {
    let (c, w, f) = (kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&c, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { c, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), chain: RefCell::new(vec![]) }
}

fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
}

fn bare(w: &World) -> OLive<Shard> {
    let x = w.shard();
    OLive { s: Shard { rows: [ShardRow::EMPTY; 4], basis: 0, ..x.s }, ..x }
}

const SEED: [u8; 32] = [0x5e; 32];

fn seeded(w: &World) -> OLive<Shard> {
    let x = bare(w);
    let sid = kpf_protocol::reserve::shard_sid(&SEED, &w.markets[0].committee_id, &x.s.terms(), &x.s.moving()[93..120]);
    OLive { s: Shard { sid, ..x.s }, ..x }
}

fn cov(w: &World, shard: &OLive<Shard>, legs: &[XLeg], coins: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: 0, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(shard.s.sid) };
    OLive { s, value: coins as u64, id: shard.id, op: op(80), daa: 6_100 }
}

fn acct_stated(w: &World, acct: &OLive<Account>, st: &Stated, said: &[u8]) -> OLive<Stated> {
    let who = if st.claimer == xk(&w.c) { &w.c } else if st.claimer == xk(&w.f) { &w.f } else { &w.w };
    let f = [fund(xk(who), 34)];
    let b = account_state(&w.lib, acct, &w.script("KpfTChange"), st, said, w.payer(who, &f)).unwrap();
    let sg = b.sign(&w.keys).expect("stated");
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "the statement is stated: {:?}", v.errors());
    nth(&sg, 0, *st, 6_510)
}

fn end_answered(w: &World, what: &str, acct: &OLive<Account>, live: &OLive<Stated>, s: &CSaid, want: bool) {
    let f = [fund(xk(&w.w), 35)];
    let q = w.pointed(live, 0);
    let (b, right) = c_end(&w.lib, &q, &acct.s, s, &w.script(match live.s.kind { 12 => "KpfTCSplit", 13 => "KpfTCMark", 14 | 15 => "KpfTCTok", _ => "KpfTCEnd" }), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert_eq!(right, want, "{what}");
    signed(&format!("{what}: the end answered {}", if right { "right" } else { "wrong" }), &b, &w.keys);
    let (b, _) = c_end(&w.lib, &q, &acct.s, s, &w.script(match live.s.kind { 12 => "KpfTCSplit", 13 => "KpfTCMark", 14 | 15 => "KpfTCTok", _ => "KpfTCEnd" }), xk(&w.f), w.payer(&w.w, &f), Some(!want)).unwrap();
    assert!(!accepted(&b, &w.keys), "{what}: the other verdict is taken");
}

#[allow(clippy::too_many_arguments)]
fn c_applied(w: &World, what: &str, acct: &OLive<Account>, kind: i64, amount: i64, skip: i64, a: &[i64], claimer: &Keypair) -> (OLive<Stated>, CSaid, kpf_protocol::covered::COutcome, OSigned) {
    let f = [fund(xk(claimer), 33)];
    let (b, st, s, o) = c_state(&w.lib, acct, &w.script("KpfTChange"), kind, amount, skip, a, T0, xk(claimer), w.payer(claimer, &f)).unwrap();
    let sg = signed(&format!("{what}: stated"), &b, &w.keys);
    assert_eq!(acct.s.c_wrong_point(kind, &s, &st.claimer, &st.outs, acct.value as i64, T0, a), None, "{what}");
    let live = nth(&sg, 0, st, 6_510);
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(claimer, &f)).unwrap();
    let sg2 = signed(&format!("{what}: applied"), &b, &w.keys);
    (live, s, o, sg2)
}

fn out_of<'a>(sg: &'a OSigned, spk: &kaspa_consensus_core::tx::ScriptPublicKey) -> (usize, u64) {
    let k = sg.tx.outputs.iter().position(|o| &o.script_public_key == spk).expect("the output");
    (k, sg.tx.outputs[k].value)
}

const TOKEN_ID: [u8; 32] = [0x7c; 32];

fn tokens_of(key: [u8; 32], amount: i64, n: u8) -> OLive<TokenState> {
    OLive { s: TokenState::p2pk(amount, key, Hash::from_bytes([0; 32])), value: KAS1 as u64, id: Some(Hash::from_bytes(TOKEN_ID)), op: op(n), daa: T0 as u64 }
}

fn run(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    println!("{what}: {} bytes, fee {} sompi", sg.mass.size, sg.fee);
    sg
}

/// The deposit of `amount` of the user's tokens into `acct` (one long), at a 20% discount; returns the signed
/// transaction, the account after (held as an OLive at `daa`), the vault live, the custody live, and the leg.
#[allow(clippy::type_complexity)]
fn deposit(
    w: &World,
    acct: &OLive<Account>,
    amount: i64,
    held_tokens: i64,
    n: u8,
    daa: u64,
) -> (OSigned, OLive<Account>, OLive<Vault>, OLive<TokenState>, XLeg) {
    let (scale, pmax) = (w.markets[0].p.scale, w.markets[0].p.price_max);
    // (The vault's id is derived from the funder's first coin's outpoint: distinct coins per deposit, distinct
    // vaults. We key it off `n`.)
    let fw = [fund(xk(&w.w), n.wrapping_add(100))];
    let add = w.script("KpfTAddSpot");
    let mine = tokens_of(xk(&w.w), held_tokens, n);
    let (b, next, vault, leg, custody) = spot_add(&w.lib, acct, &add, &mine, amount, 2_000, scale, pmax, w.payer(&w.w, &fw)).unwrap();
    let sg = run("a deposit", &b, &w.keys);
    let held = OLive { s: next, value: acct.value, id: acct.id, op: op(n.wrapping_add(40)), daa };
    let vault_live = nth(&sg, 2, vault, daa);
    let custody_live = nth(&sg, 3, custody, daa);
    (sg, held, vault_live, custody_live, leg)
}

// =============================================================================================================
// A worked example, after thin_draw's `tokens_enter_an_account_by_a_vault_born_for_them_and_leave_by_a_ticket`:
// a deposit, a kind-14 statement that writes a ticket, and the vault's payment of it. Proves the world works.
// =============================================================================================================

#[test]
fn t2_a_deposit_and_a_ticket_paid_by_its_vault() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[long], 30 * KAS1);
    let scale = w.markets[0].p.scale;
    let fw = [fund(xk(&w.w), 44)];

    let (sg, held, vault_live, custody_live, leg) = deposit(&w, &acct, 100 * scale, 150 * scale, 50, 6_700);
    assert_eq!(held.s.legs, vec![long, leg]);
    let vid = sg.tx.outputs[2].covenant.unwrap().covenant_id;

    let a = [P10, P10];
    let (_, _, o, sg2) = c_applied(&w, "tokens leave", &held, KIND_C_TOK_OUT, 0, 2, &a, &w.w);
    let ticket = o.ticket.unwrap();
    assert_eq!((ticket.key, ticket.chain), (xk(&w.w), chain_of_one(&leg)));
    let (k, _) = out_of(&sg2, &hash_spk(&w.lib, &ticket.hash()));
    let ticket_live = nth(&sg2, k, ticket, 6_800);
    let pay = w.script("KpfTVaultPay");
    let (b, paid) = vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &leg, &pay, w.payer(&w.w, &fw)).unwrap();
    let sg3 = run("a ticket paid by its vault", &b, &w.keys);
    assert_eq!((paid.amount, paid.owner), (100 * scale, xk(&w.w)));
    assert!(sg3.tx.outputs.iter().all(|x| x.covenant.is_none_or(|c| c.covenant_id != vid)), "nothing goes on under the vault's id");
    let _ = Vault { ..vault_live.s };
}

/// Pay a ticket of `claimer`'s with `vault`/`custody`/`leg`, by the funder `who`. Returns the signed transaction.
fn pay_ticket(
    w: &World,
    what: &str,
    vault: &OLive<Vault>,
    custody: &OLive<TokenState>,
    leg: &XLeg,
    chain: [u8; 32],
    claimer: [u8; 32],
    who: &Keypair,
) -> OSigned {
    // (The ticket as it stands under the account's registry id, paid straight: we fabricate it as a live object
    // under the vault's `rid`, exactly as KpfTCTok's settlement would have written it.)
    let ticket = Ticket { chain, key: claimer };
    let ticket_live = OLive { s: ticket, value: NOTE_MIN as u64, id: Some(Hash::from_bytes(vault.s.rid)), op: op(70), daa: 6_800 };
    let fw = [fund(xk(who), 60)];
    let (b, _) = vault_pay(&w.lib, vault, &ticket_live, custody, leg, &w.script("KpfTVaultPay"), w.payer(who, &fw)).unwrap();
    run(what, &b, &w.keys)
}

// =============================================================================================================
// A2. The bearer rule with a real vault at the engine, the vault's payment at each end.
// =============================================================================================================

/// A2 (i): after a deposit the account is split so the FUTURES leg leaves and the token leg stays; then tokens
/// leave (kind 14) from the account that stays, and the vault pays. A split that NAMES the token leg is refused
/// (its row is wrong).
#[test]
fn t2_a2_split_then_tokens_leave_the_staying_account() {
    use kpf_protocol::covered::c_row_judged_of;
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[long], 40 * KAS1);
    let scale = w.markets[0].p.scale;
    let fw = [fund(xk(&w.w), 44)];

    let (sg, held, vault_live, custody_live, tleg) = deposit(&w, &acct, 100 * scale, 150 * scale, 50, 6_700);
    assert_eq!(held.s.legs, vec![long, tleg]);
    let vid = sg.tx.outputs[2].covenant.unwrap().covenant_id;
    let a = [P10, P10];

    // A split naming the TOKEN leg (skip = 2): its row is wrong where 12 names it (N28). NOTE (finding V-split):
    // the END (point 0) of such a split is NOT refused by the model -- c_end returns a valid outcome that moves
    // the token leg into a new account. The refusal is enforced only at the ROW point (KpfTCRow). We confirm the
    // row is wrong and run the row dispute at the engine, where it is answered false and the statement dies.
    let s_tok = CSaid { amount: 10 * KAS1, skip: 2, lc: chain_of_one(&tleg), rows: held.s.c_trace(&a, 2).unwrap() };
    assert_eq!(c_row_judged_of(KIND_C_SPLIT, &s_tok.rows[0].g, &tleg, &s_tok.rows[0], &s_tok.rows[1], 2, &s_tok), Some(false), "the token leg's row under a split is wrong");
    // (For the record: the end itself accepts; the row is the guard.)
    assert!(matches!(held.s.c_end(KIND_C_SPLIT, &s_tok, &xk(&w.w), held.value as i64, T0), Some(Some(_))), "the end of a token-leg split is accepted; the row is the guard");
    {
        let judged = held.s.c_end(KIND_C_SPLIT, &s_tok, &xk(&w.w), held.value as i64, T0).unwrap().unwrap();
        let st = Stated { position: held.s.hash(), window: held.s.dispute_daa, late: held.s.t.late_daa, kind: KIND_C_SPLIT, t0: T0, arg: s_tok.arg(), outs: judged.outs, bond: held.s.c_bond(), claimer: xk(&w.w), held: held.value as i64 };
        let live = acct_stated(&w, &held, &st, &s_tok.said());
        // (Point numbering: row i is point 2i-1, its price 2i; the token leg is the 2nd row, point 3.)
        let q = w.pointed(&live, 3);
        let (b, right) = c_row(&w.lib, &q, &s_tok, 3, &s_tok.rows[0].g, &tleg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(!right, "the token leg's row under a split is answered wrong at the engine");
        signed("a split that names the token leg: its row refused", &b, &w.keys);
    }

    // A split naming the FUTURES leg (skip = 1): the long leaves, the token leg stays. (The valid coin amount for
    // the leaving account is found from the model, so the arithmetic is the model's, not ours.)
    let amount = (1..=40).map(|k| k * KAS1).find(|&amt| held.s.c_state(KIND_C_SPLIT, amt, 1, &a, T0, held.value as i64, xk(&w.w)).is_ok()).expect("a split of the futures leg off an account holding a token leg is possible");
    let (live, said, o, sg2) = c_applied(&w, "the futures leg leaves, the token leg stays", &held, KIND_C_SPLIT, amount, 1, &a, &w.w);
    let (stays, gone) = (o.after.clone().unwrap(), o.split.clone().unwrap());
    assert_eq!((stays.legs.clone(), gone.legs.clone()), (vec![tleg], vec![long]), "the token leg stays, the long leaves");
    end_answered(&w, "the split off the token account", &held, &live, &said, true);
    // (The split writes a future "quiet" time: the user must wait before stating again. We advance past it; the
    // leg -- hence its chain and its binding to the vault -- is unchanged by the split, which is the point.)
    assert!(stays.b_min_counter > T0, "the split makes the user wait");
    let stays_live = OLive { s: Account { b_min_counter: 0, ..stays.clone() }, ..nth(&sg2, 0, stays.clone(), 6_900) };

    // Tokens leave the staying account (which now holds the token leg alone, skip = 1): a ticket is the user's.
    let (_, _, o2, sg3) = c_applied(&w, "tokens leave the account that stayed", &stays_live, KIND_C_TOK_OUT, 0, 1, &[P10], &w.w);
    let ticket = o2.ticket.unwrap();
    assert_eq!((ticket.key, ticket.chain), (xk(&w.w), chain_of_one(&tleg)));
    let (k, _) = out_of(&sg3, &hash_spk(&w.lib, &ticket.hash()));
    let ticket_live = nth(&sg3, k, ticket, 7_000);
    let custody_live = OLive { daa: 7_000, ..custody_live };
    let vault_live = OLive { daa: 7_000, ..vault_live };
    let (b, paid) = vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &tleg, &w.script("KpfTVaultPay"), w.payer(&w.w, &fw)).unwrap();
    let sg4 = run("the vault pays after a split", &b, &w.keys);
    assert_eq!((paid.amount, paid.owner), (100 * scale, xk(&w.w)), "the tokens go to the user whole");
    assert!(sg4.tx.outputs.iter().all(|x| x.covenant.is_none_or(|c| c.covenant_id != vid)), "the vault ends");
}

/// A2 (iii): a handed-over account (KpfTPass): the taker states the leaving, the ticket is the taker's, and the
/// vault pays the taker. The old user can get nothing.
#[test]
fn t2_a2_handover_then_the_taker_takes_the_tokens() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[long], 30 * KAS1);
    let scale = w.markets[0].p.scale;

    let (sg, held, vault_live, custody_live, tleg) = deposit(&w, &acct, 100 * scale, 150 * scale, 50, 6_700);
    let vid = sg.tx.outputs[2].covenant.unwrap().covenant_id;
    let a = [P10, P10];

    // The account (with its token leg) is handed from w.w to w.f.
    let fpass = [KeyUtxo { daa: 8_000, ..fund(xk(&w.f), 61) }];
    let (b, mine) = account_pass(&w.lib, &held, &w.script("KpfTPass"), xk(&w.f), 0, w.payer(&w.f, &fpass)).unwrap();
    let sgp = signed("the account with its token leg handed over", &b, &w.keys);
    assert_eq!((mine.a_key, mine.legs.clone()), (xk(&w.f), vec![long, tleg]));
    let mine_live = nth(&sgp, 0, mine.clone(), 8_100);

    // The old user can get nothing: a kind-14 claim by w.w is refused (claimer must be the account's key).
    assert!(mine_live.s.c_state(KIND_C_TOK_OUT, 0, 2, &a, 8_200, mine_live.value as i64, xk(&w.w)).is_err(), "the old user takes nothing out");

    // The taker states the leaving; the ticket is the taker's; the vault pays the taker.
    let (_, _, o, sg2) = c_applied(&w, "the taker takes the tokens out", &OLive { daa: 8_100, ..mine_live.clone() }, KIND_C_TOK_OUT, 0, 2, &a, &w.f);
    let ticket = o.ticket.unwrap();
    assert_eq!(ticket.key, xk(&w.f), "the ticket is the taker's");
    let (k, _) = out_of(&sg2, &hash_spk(&w.lib, &ticket.hash()));
    let ticket_live = nth(&sg2, k, ticket, 8_300);
    let vault_live = OLive { daa: 8_300, ..vault_live };
    let custody_live = OLive { daa: 8_300, ..custody_live };
    let ff = [fund(xk(&w.f), 62)];
    let (b, paid) = vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &tleg, &w.script("KpfTVaultPay"), w.payer(&w.f, &ff)).unwrap();
    let sg3 = run("the vault pays the taker", &b, &w.keys);
    assert_eq!((paid.amount, paid.owner), (100 * scale, xk(&w.f)), "the tokens go to the taker, not the old user");
    assert!(sg3.tx.outputs.iter().all(|x| x.covenant.is_none_or(|c| c.covenant_id != vid)), "the vault ends");
}

/// A2 (iv): an account that owes its shard is sold from (kind 15): the ticket is the claimer's and the vault pays
/// the claimer; the note of what was paid applies to the shard. And, via KpfTOwed.pay, the debt is paid and then
/// the tokens leave to the user.
#[test]
fn t2_a2_an_owing_account_is_sold_from_or_paid_for() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    // The deposit account holds only a note's least of coins (so a step would have made it owe).
    let acct = cov(&w, &shard, &[], NOTE_MIN);
    let scale = w.markets[0].p.scale;
    let _ = long;

    let (sg, held, vault_live, custody_live, tleg) = deposit(&w, &acct, 100 * scale, 150 * scale, 50, 6_700);
    let vid = sg.tx.outputs[2].covenant.unwrap().covenant_id;
    assert_eq!(held.s.legs, vec![tleg]);
    let a = [P10];

    // Make it owe 30 KAS (what a step on its futures would have left). A sale (kind 15) by anyone (w.f).
    let owes = 30 * KAS1;
    let ower = OLive { s: Account { b_margin: owes, ..held.s.clone() }, ..held.clone() };
    assert!(ower.s.c_check().is_ok() && ower.s.owes_nothing().is_err());
    // An owing account takes nothing out by its user and is handed to nobody.
    assert!(ower.s.c_state(KIND_C_TOK_OUT, 0, 1, &a, T0, ower.value as i64, xk(&w.w)).is_err(), "an owing account takes no tokens out");

    let (live, said, o, sg2) = c_applied(&w, "a token leg of an owing account sold", &ower, KIND_C_TOK_SOLD, 0, 1, &a, &w.f);
    let (after, ticket, owed) = (o.after.clone().unwrap(), o.ticket.unwrap(), o.owed.unwrap());
    assert_eq!(ticket.key, xk(&w.f), "the ticket is the claimer's");
    assert_eq!((after.legs.len(), after.b_margin), (0, 0), "the account owes nothing after and holds no leg");
    end_answered(&w, "the sale", &ower, &live, &said, true);

    // The vault pays the claimer (w.f), not the user.
    let (k, _) = out_of(&sg2, &hash_spk(&w.lib, &ticket.hash()));
    let ticket_live = nth(&sg2, k, ticket, 6_900);
    let custody_live = OLive { daa: 6_900, ..custody_live };
    let vault_live = OLive { daa: 6_900, ..vault_live };
    let ff = [fund(xk(&w.f), 62)];
    let (b, paid) = vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &tleg, &w.script("KpfTVaultPay"), w.payer(&w.f, &ff)).unwrap();
    let sg3 = run("the vault pays the sale's claimer", &b, &w.keys);
    assert_eq!(paid.owner, xk(&w.f), "the tokens go to the claimer");
    assert!(sg3.tx.outputs.iter().all(|x| x.covenant.is_none_or(|c| c.covenant_id != vid)), "the vault ends");

    // The note of what was paid applies to the shard.
    let (ko, _) = out_of(&sg2, &hash_spk(&w.lib, &owed.hash()));
    let note = nth(&sg2, ko, owed, 6_900);
    let fw = [fund(xk(&w.w), 44)];
    let (b, next) = owed_apply(&w.lib, &shard, &note, &w.script("KpfTOwed"), w.payer(&w.w, &fw)).unwrap();
    signed("the note of what was paid, applied to the shard", &b, &w.keys);
    assert_eq!(next.held, shard.s.held + owes);

    // The other path: KpfTOwed.pay clears the debt, and then the tokens leave to the user.
    let ff2 = [fund(xk(&w.f), 63)];
    let (b, paid_up, _n) = owed_pay(&w.lib, &ower, &w.script("KpfTOwed"), owes, w.payer(&w.f, &ff2)).unwrap();
    signed("the account's debt paid", &b, &w.keys);
    assert!(paid_up.owes_nothing().is_ok() && paid_up.c_state(KIND_C_TOK_OUT, 0, 1, &a, T0, ower.value as i64, xk(&w.w)).is_ok(), "then its tokens leave by its user's statement");
}

/// A2 (ii) and A4(a): two deposits make two vaults, under two ids, with two leg-chains. Each ticket pays its own
/// vault and is refused by the other (a ticket of leg A's chain cannot pay vault B).
#[test]
fn t2_a2_two_vaults_each_ticket_pays_its_own() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let scale = w.markets[0].p.scale;

    // Two separate covered accounts, two separate deposits -> two vaults.
    let acctA = cov(&w, &shard, &[long], 30 * KAS1);
    let acctB = OLive { op: op(81), ..cov(&w, &shard, &[long], 30 * KAS1) };
    let (sgA, _heldA, vaultA, custodyA, legA) = deposit(&w, &acctA, 100 * scale, 150 * scale, 50, 6_700);
    let (sgB, _heldB, vaultB, custodyB, legB) = deposit(&w, &acctB, 100 * scale, 150 * scale, 52, 6_700);
    let (vidA, vidB) = (sgA.tx.outputs[2].covenant.unwrap().covenant_id, sgB.tx.outputs[2].covenant.unwrap().covenant_id);
    assert_ne!(vidA, vidB, "two vaults, two ids");
    assert_ne!(chain_of_one(&legA), chain_of_one(&legB), "two leg-chains");

    // Each ticket (of its own leg's chain, under its own account's rid) pays its own vault.
    pay_ticket(&w, "vault A pays ticket A", &vaultA, &custodyA, &legA, chain_of_one(&legA), xk(&w.w), &w.w);
    pay_ticket(&w, "vault B pays ticket B", &vaultB, &custodyB, &legB, chain_of_one(&legB), xk(&w.w), &w.w);

    // A ticket of leg A's chain cannot pay vault B: the leg shown must name vault B's id and the ticket's chain
    // must hash to it; the builder refuses, and so would the engine (the leg's committee != vidB).
    let fw = [fund(xk(&w.w), 60)];
    let ticketA = Ticket { chain: chain_of_one(&legA), key: xk(&w.w) };
    let ticketA_under_B = OLive { s: ticketA, value: NOTE_MIN as u64, id: Some(Hash::from_bytes(vaultB.s.rid)), op: op(71), daa: 6_800 };
    assert!(
        vault_pay(&w.lib, &vaultB, &ticketA_under_B, &custodyB, &legA, &w.script("KpfTVaultPay"), w.payer(&w.w, &fw)).is_err(),
        "a ticket of leg A cannot pay vault B"
    );
    // Nor with leg B shown but A's chain: the ticket's chain would not hash from leg B.
    let ticketA_chain_under_B = OLive { s: Ticket { chain: chain_of_one(&legA), key: xk(&w.w) }, value: NOTE_MIN as u64, id: Some(Hash::from_bytes(vaultB.s.rid)), op: op(72), daa: 6_800 };
    assert!(
        vault_pay(&w.lib, &vaultB, &ticketA_chain_under_B, &custodyB, &legB, &w.script("KpfTVaultPay"), w.payer(&w.w, &fw)).is_err(),
        "leg B shown but A's chain in the ticket: refused"
    );
}

fn called_objs(shard: &OLive<Shard>, prices: [i64; 4], daa: u64) -> (OLive<Shard>, OLive<Called>) {
    let fee = shard.s.t.keeper_fee_min.min(shard.s.held);
    let (next, call) = shard.s.called_at(&prices, T0, fee);
    (OLive { s: next, value: shard.value - fee as u64 - CALL_HOLD as u64, id: shard.id, op: op(90), daa }, OLive { s: call, value: CALL_HOLD as u64, id: shard.id, op: op(91), daa })
}

/// A2 (v): under a call's step (KpfTCStep) an account that holds a token leg goes on with its token leg, owing the
/// rest. It then owes, so it is sold from and the vault pays the claimer.
#[test]
fn t2_a2_under_a_call_the_account_goes_on_with_its_token_leg_then_is_sold_from() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&long).unwrap() }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let scale = w.markets[0].p.scale;
    // An account of the long and 5 KAS; the token leg is added.
    let acct = cov(&w, &shard, &[long], 5 * KAS1);
    let (sg, held, vault_live, custody_live, tleg) = deposit(&w, &acct, 100 * scale, 150 * scale, 50, 6_700);
    let vid = sg.tx.outputs[2].covenant.unwrap().covenant_id;
    assert_eq!(held.s.legs, vec![long, tleg]);

    // The call's price is 8: the long of 10 from 10 has lost 20 KAS; the coins cannot pay; the account goes on
    // with the token leg, owing the rest.
    let (called, call) = called_objs(&shard, [P10 * 8 / 10, 0, 0, 0], 6_600);
    let ff = [fund(xk(&w.f), 36)];
    let (b, st) = call_step(&w.lib, &held, &held.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
    let sgs = signed("the call's step on an account that holds a token leg", &b, &w.keys);
    let after = st.after.clone().unwrap();
    assert_eq!(after.legs.clone(), vec![tleg], "the account goes on with only the token leg");
    assert_eq!(st.note.legs, vec![long], "the note names the futures leg alone");
    assert!(after.b_margin > 0, "it owes the rest");
    let _ = called;

    // It owes, so it is sold from (kind 15); the ticket is the claimer's and the vault pays the claimer. (The step
    // writes a future quiet time; we advance past it -- the token leg and its vault binding are unchanged.)
    let owing = OLive { s: Account { b_min_counter: 0, ..after.clone() }, ..nth(&sgs, 0, after.clone(), 6_700) };
    let a = [P10];
    let (live, said, o, sg2) = c_applied(&w, "a token leg of the owing account sold after a call's step", &owing, KIND_C_TOK_SOLD, 0, 1, &a, &w.f);
    let ticket = o.ticket.unwrap();
    assert_eq!(ticket.key, xk(&w.f), "the ticket is the claimer's");
    end_answered(&w, "the sale after a call's step", &owing, &live, &said, true);
    let (k, _) = out_of(&sg2, &hash_spk(&w.lib, &ticket.hash()));
    let ticket_live = nth(&sg2, k, ticket, 6_900);
    let custody_live = OLive { daa: 6_900, ..custody_live };
    let vault_live = OLive { daa: 6_900, ..vault_live };
    let ff2 = [fund(xk(&w.f), 62)];
    let (b, paid) = vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &tleg, &w.script("KpfTVaultPay"), w.payer(&w.f, &ff2)).unwrap();
    let sg3 = run("the vault pays the sale's claimer, after a call's step", &b, &w.keys);
    assert_eq!(paid.owner, xk(&w.f));
    assert!(sg3.tx.outputs.iter().all(|x| x.covenant.is_none_or(|c| c.covenant_id != vid)), "the vault ends");
}

// =============================================================================================================
// A3. The time of a deposit: the account's "at will" time W (its `expiry`).
// =============================================================================================================

#[test]
fn t2_a3_the_time_of_a_deposit() {
    let w = world();
    let base = seeded(&w);
    let shard = base.clone();
    let scale = w.markets[0].p.scale;
    let pmax = w.markets[0].p.price_max;
    let add = w.script("KpfTAddSpot");
    let mine = tokens_of(xk(&w.w), 150 * scale, 50);
    let late = w.terms().late_daa;

    // (b) The W a normal deposit writes: max(account.expiry, account.daa + lateness).
    let acct = cov(&w, &shard, &[], 10 * KAS1); // account.daa = 6100, expiry 0.
    let f_ok = [fund(xk(&w.w), 44)]; // the funder's coin landed at T0 = 6500 >= 6200.
    let (b, next, _v, _l, _c) = spot_add(&w.lib, &acct, &add, &mine, 100 * scale, 2_000, scale, pmax, w.payer(&w.w, &f_ok)).unwrap();
    assert_eq!(next.expiry, (acct.daa as i64 + late).max(0), "W = account.daa + lateness");
    let sg = run("a deposit at will", &b, &w.keys);
    // (NOTE: the deposit holds f to the account's CURRENT at-will time, not the new W; a fresh account's is 0, so
    // the bound bites only when the account already carries a future time -- the cases below.)

    // (a) A deposit by a fabricated account with W in the future, funded by a coin that landed before W, is refused
    // by the builder (no input lands at or after W).
    let future = OLive { s: Account { expiry: 9_000, ..acct.s.clone() }, daa: 6_100, ..acct.clone() };
    let early = [KeyUtxo { daa: 8_000, ..fund(xk(&w.w), 45) }];
    assert!(spot_add(&w.lib, &future, &add, &mine, 100 * scale, 2_000, scale, pmax, w.payer(&w.w, &early)).is_err(), "no input lands at or after W: refused");
    // Funded by a coin that landed at W: accepted, and the W it writes is the account's own future time.
    let at_w = [KeyUtxo { daa: 9_000, ..fund(xk(&w.w), 46) }];
    let (b, next2, _, _, _) = spot_add(&w.lib, &future, &add, &mine, 100 * scale, 2_000, scale, pmax, w.payer(&w.w, &at_w)).unwrap();
    assert_eq!(next2.expiry, 9_000, "W is the later of the account's own time and account.daa + lateness");
    let sgf = run("a deposit whose account's W is in the future, funded at W", &b, &w.keys);
    // The engine refuses the same with the funder's coin a block before W (input 3).
    let mut t = b.clone();
    t.entries[3].block_daa_score = 8_999;
    assert!(!accepted(&t, &w.keys), "f a block before a future W is refused at the engine");

    // (c) Two deposits in a row: the second must wait. After the first deposit the account's W is 6200; a second
    // deposit on the account-after, with both the account and the funder landing before that W, is refused.
    let after1 = OLive { s: next.clone(), value: acct.value, id: acct.id, op: op(84), daa: 6_150 };
    let early2 = [KeyUtxo { daa: 6_199, ..fund(xk(&w.w), 47) }];
    assert!(spot_add(&w.lib, &OLive { daa: 6_199, ..after1.clone() }, &add, &tokens_of(xk(&w.w), 50 * scale, 51), 25 * scale, 2_000, scale, pmax, w.payer(&w.w, &early2)).is_err(), "the second deposit must wait for the first's W");
    let late_enough = [KeyUtxo { daa: next.expiry as u64, ..fund(xk(&w.w), 48) }];
    let (b, _n, _, _, _) = spot_add(&w.lib, &after1, &add, &tokens_of(xk(&w.w), 50 * scale, 52), 25 * scale, 2_000, scale, pmax, w.payer(&w.w, &late_enough)).unwrap();
    run("a second deposit, a coin landing at the first's W", &b, &w.keys);
    let _ = sg;
    let _ = sgf;
}

// =============================================================================================================
// A4. What round 66 read and did not run: a selection at the engine and at the builder.
// =============================================================================================================

/// A4(b): a look-alike token program of the same lengths but another body -- the template-hash (and P2SH) check
/// refuses; and integer edges and the ill-formed account are refused.
#[test]
fn t2_a4_a_lookalike_token_program_and_edges() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[long], 30 * KAS1);
    let scale = w.markets[0].p.scale;
    let pmax = w.markets[0].p.price_max;
    let add = w.script("KpfTAddSpot");
    let mine = tokens_of(xk(&w.w), 150 * scale, 50);
    let fw = [fund(xk(&w.w), 44)];

    // A good deposit, then mutate one body byte of the token input's redeem (input 2), keeping its length, and fix
    // its P2SH scriptPubKey to match: the token program hashes to another template, so our check refuses.
    let (b, _n, _v, _l, _c) = spot_add(&w.lib, &acct, &add, &mine, 100 * scale, 2_000, scale, pmax, w.payer(&w.w, &fw)).unwrap();
    run("a good deposit", &b, &w.keys);
    let mut t = b.clone();
    let mutated: Vec<u8> = if let OPlan::Contract { redeem, .. } = &mut t.plans[2] {
        redeem[0] ^= 0xff; // a byte of the program's prefix (its code, not its state)
        redeem.clone()
    } else {
        panic!("input 2 is the token");
    };
    t.entries[2].script_public_key = pay_to_script_hash_script(&mutated);
    assert!(!accepted(&t, &w.keys), "a look-alike token program of the same lengths is refused (the template hash)");
    // Mutate only the redeem (not the spk): the P2SH check refuses first.
    let mut t2 = b.clone();
    if let OPlan::Contract { redeem, .. } = &mut t2.plans[2] {
        redeem[0] ^= 0xff;
    }
    assert!(!accepted(&t2, &w.keys), "a token input whose redeem does not match its P2SH is refused");

    // Integer edges: an amount of 2^63-1 overflows the worth; a discount under the floor or over the whole; an
    // amount of 0. (Model/builder refusals.)
    assert!(spot_add(&w.lib, &acct, &add, &mine, i64::MAX, 2_000, scale, pmax, w.payer(&w.w, &fw)).is_err(), "an amount of 2^63-1 is refused (worth overflow / more than held)");
    let t = w.terms();
    assert!(spot_add(&w.lib, &acct, &add, &mine, 100 * scale, t.liq_fee_bps + 2 * t.g_bps - 1, scale, pmax, w.payer(&w.w, &fw)).is_err(), "a discount under the floor is refused");
    assert!(spot_add(&w.lib, &acct, &add, &mine, 100 * scale, 10_001, scale, pmax, w.payer(&w.w, &fw)).is_err(), "a discount over the whole is refused");
    assert!(spot_add(&w.lib, &acct, &add, &mine, 0, 2_000, scale, pmax, w.payer(&w.w, &fw)).is_err(), "an amount of 0 is refused");

    // The ill-formed/forbidden account: one that owes, one of six legs. (Builder refusals.)
    assert!(spot_add(&w.lib, &OLive { s: Account { b_margin: KAS1, ..acct.s.clone() }, ..acct.clone() }, &add, &mine, 100 * scale, 2_000, scale, pmax, w.payer(&w.w, &fw)).is_err(), "an account that owes takes no deposit");
    let six = cov(&w, &shard, &[long, long, long, long, long, long], 30 * KAS1);
    assert!(spot_add(&w.lib, &six, &add, &mine, 100 * scale, 2_000, scale, pmax, w.payer(&w.w, &fw)).is_err(), "an account of six legs takes no deposit");
}

/// A4(a): the vault's payment held to the ticket and the amounts. A vault of an amount different from the leg or
/// the custody; the tokens' output to another key or fewer; the custody owned by the account's id. (Builder and
/// engine refusals, beyond round 66's.)
#[test]
fn t2_a4_the_payment_held_to_the_ticket_and_the_amounts() {
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[long], 30 * KAS1);
    let scale = w.markets[0].p.scale;
    let fw = [fund(xk(&w.w), 44)];

    let (sg, held, vault_live, custody_live, leg) = deposit(&w, &acct, 100 * scale, 150 * scale, 50, 6_700);
    let a = [P10, P10];
    let (_, _, o, sg2) = c_applied(&w, "tokens leave", &held, KIND_C_TOK_OUT, 0, 2, &a, &w.w);
    let ticket = o.ticket.unwrap();
    let (k, _) = out_of(&sg2, &hash_spk(&w.lib, &ticket.hash()));
    let ticket_live = nth(&sg2, k, ticket, 6_800);
    let pay = w.script("KpfTVaultPay");

    // A vault whose amount differs from the leg's and the custody's: the builder refuses (and the state hash of
    // such a vault would not match the custody the engine is shown).
    let wrong_vault = OLive { s: Vault { amount: 99 * scale, ..vault_live.s }, ..vault_live.clone() };
    assert!(vault_pay(&w.lib, &wrong_vault, &ticket_live, &custody_live, &leg, &pay, w.payer(&w.w, &fw)).is_err(), "a vault of an amount the leg does not name is refused");
    // A leg of an amount other than the vault's.
    let wrong_leg = XLeg { size: 99 * scale, ..leg };
    assert!(vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &wrong_leg, &pay, w.payer(&w.w, &fw)).is_err(), "a leg of a different amount is refused");
    // The custody of an amount other than the vault's.
    let wrong_custody = OLive { s: TokenState::covid(99 * scale, Hash::from_bytes(vault_live.s.rid), custody_live.s.ext), ..custody_live.clone() };
    assert!(vault_pay(&w.lib, &vault_live, &ticket_live, &wrong_custody, &leg, &pay, w.payer(&w.w, &fw)).is_err(), "custody of a different amount is refused");

    // The honest payment, then the tokens' output (index 0) written to another key or fewer: refused at the engine.
    let (b, _paid) = vault_pay(&w.lib, &vault_live, &ticket_live, &custody_live, &leg, &pay, w.payer(&w.w, &fw)).unwrap();
    run("the honest payment", &b, &w.keys);
    for (what, spk) in [
        ("to another key", TokenState::p2pk(100 * scale, xk(&w.f), custody_live.s.ext).spk()),
        ("fewer tokens", TokenState::p2pk(99 * scale, xk(&w.w), custody_live.s.ext).spk()),
        ("owned by the vault's id again", TokenState::covid(100 * scale, Hash::from_bytes(vault_live.s.rid), custody_live.s.ext).spk()),
    ] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = spk;
        assert!(!accepted(&t, &w.keys), "the tokens paid {what}: refused");
    }
    let _ = sg;
}
