//! The proof of a disputed claim of the thin family with the committee's signatures (`thin_build::prove`): a committee's
//! epoch table, three members that sign a signed price, a position in the market that committee prices. Every transaction is built,
//! signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{Cross, XLeg, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::Position;
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
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    tight(what, b, keys);
    println!("{what}: {} bytes, fee {} sompi", sg.mass.size, sg.fee);
    sg
}

/// Every output a script fixes is fixed from below: the same transaction with a sompi less at any output but the
/// change and the fillers is refused by the engine (review T1, the lead it did not reach: what each entry pays).
fn tight(what: &str, b: &OBuilt, keys: &KeySet) {
    // Not an operation of the family (a genesis, an opening), or the one whose outputs are the two parties' to choose
    // (`KpfTBoth`: their signatures cover every output, and this check signs again with both keys).
    if !b.plans.iter().any(|p| p.name().starts_with("KpfSlot")) || b.plans.iter().any(|p| p.name().starts_with("KpfTBoth")) {
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

struct World {
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

impl World {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    /// The attestation of `price` for the time T0 of market `i` by the three members.
    fn attest(&self, i: usize, price: i64) -> Attestation {
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &self.markets[i].id(), T0, price)).collect();
        Attestation::collect(&self.epoch_tables[i].s, &self.rid.as_bytes(), &self.markets[i].id(), T0, &signed_prices).expect("a quorum")
    }
    /// A position of A against B: long 10 units of market 0 at 10, and with `two` short 5 units of market 1 at 10.
    fn position(&self, two: bool) -> OLive<Position> {
        let (m, n) = (&self.markets[0], &self.markets[1]);
        let leg = |m: &CMarket, size: i64, dir: i64| XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: size * m.p.scale, dir, entry: P10 };
        let p = &m.p;
        let t = XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa };
        let x = Cross { t, legs: [leg(m, 10, LONG), if two { leg(n, 5, SHORT) } else { XLeg::none() }], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 40 * KAS1, b_margin: 60 * KAS1, expiry: 6_000 + p.term };
        let position = Position { x, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 };
        position.check().unwrap();
        OLive { s: position, value: x.floor() as u64, id: Some(Hash::from_bytes([0x71; 32])), op: op(10), daa: 6_100 }
    }
}

fn world() -> World {
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
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = signed("library genesis", &g, &keys);
    World { a, b, w, members, keys, rid, markets, epoch_tables: [epoch_table(20), epoch_table(21)], lib, scripts: lib_scripts(&gs.tx, 6_000) }
}

/// A's true claim at `prices`, declared and disputed by the second executor: the disputed claim, live.
fn disputed(w: &World, position: &OLive<Position>, side: i64, prices: [i64; 2], amount: Option<i64>) -> OLive<kpf_protocol::thin::Disputed> {
    let who = if side == SIDE_A { &w.a } else { &w.b };
    let f = [fund(xk(who), 30)];
    let (b, claim) = close(&w.lib, position, &w.script("KpfTClose"), side, T0, prices, amount, Payer { funds: &f, change_to: xk(who), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("close", &b, &w.keys);
    let f = [fund(xk(&w.w), 31)];
    let (b, d) = dispute(&w.lib, &nth(&s, 0, claim, 6_510), &w.script("KpfTDispute"), xk(&w.w), Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE }).unwrap();
    let ds = signed("dispute", &b, &w.keys);
    nth(&ds, 0, d, 6_520)
}

#[test]
fn a_disputed_true_claim_is_proved_with_the_committees_signatures() {
    let w = world();
    let position = w.position(false);
    // The committee signed 11 for the time; A declares what the rule gives there, and is disputed.
    let p = [11 * P10 / 10, 0];
    let dis = disputed(&w, &position, SIDE_A, p, None);
    let c = dis.s.claim;
    let att = w.attest(0, p[0]);
    let f = [fund(xk(&w.a), 32)];
    let payer = Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE };
    let b = prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, payer).unwrap();
    let s = signed("prove (one leg, one epoch table)", &b, &w.keys);
    // The claimer has its amount, its bond and half of the disputer's; the other half stays in the script's copy
    // (the committee's, which answered); the other party has the rest; the disputer has lost its bond.
    let kept = c.bond - c.bond / 2;
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), ((c.amount + 2 * c.bond - kept) as u64, &p2pk_spk(&xk(&w.a))));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTProve").value + (kpf_protocol::thin::ORACLE_FEE + kept) as u64, "half of the disputer's bond, in the copy");
    let last = s.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), (dis.value - (c.amount + 2 * c.bond) as u64, &p2pk_spk(&xk(&w.b))));
    assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&xk(&w.w))), "nothing to the disputer");
    // The epoch table goes on at its own index, under the committee's id.
    assert_eq!((s.tx.outputs[2].value, s.tx.outputs[2].covenant.map(|c| c.covenant_id)), (w.epoch_tables[0].value, Some(w.rid)));
    // The claim states its claimer's own price; the committee's need not be the same. A is long and counted the
    // tolerance (0.5 %) against itself: a committee price 0.3 % lower still gives it at least what it states.
    let close = w.attest(0, p[0] - 3 * p[0] / 1000);
    let b2 = prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &close), None, payer).unwrap();
    signed("prove (the committee a little below the claim)", &b2, &w.keys);
    // A committee price better for the claimer proves it all the more.
    let better = w.attest(0, p[0] + p[0] / 50);
    let b3 = prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &better), None, payer).unwrap();
    signed("prove (the committee above the claim)", &b3, &w.keys);
    // A committee price 0.9 % lower gives A less than it states: not proved, by the tool and by the script.
    let other = w.attest(0, p[0] - 9 * p[0] / 1000);
    assert!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &other), None, payer).unwrap_err().to_string().contains("it is refuted"));
    let forced = prove_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &other), None, payer, false).unwrap();
    let fs = forced.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "a claim of more than the committee's price gives is not proved");
    let wrong_market = w.attest(1, p[0]);
    assert!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[1], &wrong_market), None, payer).is_err());
    assert!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), Some((&w.epoch_tables[1], &wrong_market)), payer).is_err(), "an epoch table too many");
    // The payouts are the script's: the same transaction with a sompi less to the other party is refused by the engine.
    let mut forged = b.clone();
    let n = forged.tx.outputs.len();
    forged.tx.outputs[n - 1].value -= 1;
    let fs = forged.sign(&w.keys).unwrap();
    assert!(!kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok(), "a sompi less to the other party");
}

/// A claim of more than the committee's prices give is refuted by anyone who shows the committee's signatures: the
/// position again, the disputer its bond, the refuter half of the claimer's, the other half in the script's copy.
/// Proof and refutation are each other's complement: the same signatures pass exactly one of the two.
#[test]
fn a_claim_of_more_than_the_committees_price_gives_is_refuted_by_anyone() {
    let w = world();
    let position = w.position(false);
    // A (long) states 11 and what the rule gives there; the committee signs 10.6 for that time.
    let p = [11 * P10 / 10, 0];
    let dis = disputed(&w, &position, SIDE_A, p, None);
    let c = dis.s.claim;
    let low = w.attest(0, 106 * P10 / 100);
    let f = [fund(xk(&w.w), 32)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let refuter = xk(&w.b);
    let (b, back) = refute(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &low), None, refuter, payer).unwrap();
    let s = signed("refute", &b, &w.keys);
    assert_eq!(back, position.s);
    assert_eq!((s.tx.outputs[0].value, s.tx.outputs[0].covenant.map(|c| c.covenant_id)), (position.value, position.id), "the position again, whole");
    let (half, n) = (c.bond / 2, s.tx.outputs.len());
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTProve").value + (kpf_protocol::thin::ORACLE_FEE + c.bond - half) as u64, "half of the claimer's bond, in the copy");
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (c.bond as u64, &p2pk_spk(&xk(&w.w))), "the disputer's bond back");
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (half as u64, &p2pk_spk(&refuter)), "half of the claimer's bond to who refuted");
    assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&xk(&w.a))), "nothing to the claimer");
    // The complement, at prices on both sides of the line and at the line: each attestation passes exactly one of
    // the two entries on the engine, and the tool refuses to build the other.
    let passes = |b: kpf_protocol::oracle_build::OBuilt| {
        let fs = b.sign(&w.keys).unwrap();
        kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok()
    };
    let exact = position.s.x.owed_at(SIDE_A, p);
    for price in [p[0], p[0] - p[0] / 1000, p[0] - 4 * p[0] / 1000, p[0] - 6 * p[0] / 1000, p[0] - p[0] / 50, p[0] + p[0] / 50, 106 * P10 / 100, P10 / 2] {
        let att = w.attest(0, price);
        let within = c.within([price, 0]);
        let proved = passes(prove_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, payer, false).unwrap());
        let refuted = passes(refute_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, refuter, payer, false).unwrap().0);
        assert_eq!((proved, refuted), (within, !within), "at the committee's price {price}");
        assert_eq!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, payer).is_ok(), within);
        assert_eq!(refute(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, refuter, payer).is_ok(), !within);
    }
    // An amount of the claimer's own: exactly what the committee's price gives is proved, a sompi more refuted.
    let at = w.attest(0, p[0]);
    for (amount, within) in [(exact, true), (exact + 1, false), (0, true), (c.amount - 1, true)] {
        let dis = disputed(&w, &position, SIDE_A, p, Some(amount));
        assert_eq!(dis.s.claim.within(p), within);
        let proved = passes(prove_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &at), None, payer, false).unwrap());
        let refuted = passes(refute_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &at), None, refuter, payer, false).unwrap().0);
        assert_eq!((proved, refuted), (within, !within), "an amount of {amount}");
    }
}

#[test]
fn a_false_claim_is_not_proved_and_two_legs_take_two_epoch_tables() {
    let w = world();
    // A false amount at signed prices: the tool refuses to build its proof.
    let position = w.position(false);
    let p = [11 * P10 / 10, 0];
    let truth = position.s.owed(SIDE_A, p).unwrap();
    let dis = disputed(&w, &position, SIDE_A, p, Some(truth + KAS1));
    let att = w.attest(0, p[0]);
    let f = [fund(xk(&w.a), 32)];
    let payer = Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE };
    assert!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, payer).is_err());
    // Two legs: an epoch table of each leg's committee, each attesting its market's price for the same time. B is the claimer.
    let position = w.position(true);
    let p = [9 * P10 / 10, 12 * P10 / 10];
    let dis = disputed(&w, &position, SIDE_B, p, None);
    let c = dis.s.claim;
    let (a1, a2) = (w.attest(0, p[0]), w.attest(1, p[1]));
    assert!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), None, payer).is_err(), "an epoch table short");
    assert!(prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[1], &a2), Some((&w.epoch_tables[0], &a1)), payer).is_err(), "the epoch tables in each other's place");
    let b = prove(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), payer).unwrap();
    let s = signed("prove (two legs, two epoch tables)", &b, &w.keys);
    let last = s.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), ((c.amount + 2 * c.bond - (c.bond - c.bond / 2)) as u64, &p2pk_spk(&xk(&w.b))), "the claimer B at the last output");
    assert_eq!(s.tx.outputs[0].script_public_key, p2pk_spk(&xk(&w.a)));
}

#[test]
fn a_disputed_liquidation_is_proved_and_waits_a_window_again() {
    use kpf_protocol::thin::{LiqObj, LiqState};
    let w = world();
    // A position with a thin margin: A long 10 at 10 with 6 KAS. The committee signs 9.7, where the rule liquidates A.
    let mut position = w.position(false);
    position.s.x.a_margin = 6 * KAS1;
    position.s.check().unwrap();
    position.value = position.s.x.floor() as u64;
    let p = [97 * P10 / 100, 0];
    let f = [fund(xk(&w.w), 40)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (b, lq) = liquidate(&w.lib, &position, &w.script("KpfTLiq"), SIDE_A, T0, p, None, xk(&w.w), payer).unwrap();
    let s = signed("liquidate", &b, &w.keys);
    // The side disputes it (the claim is true: it will lose its bond).
    let f2 = [fund(xk(&w.a), 41)];
    let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), Payer { funds: &f2, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let ds = signed("dispute a liquidation", &b, &w.keys);
    let dlive = nth(&ds, 0, dl, 6_520);
    // The liquidator proves it with the committee's signatures: the claim is a proved one, holding one bond less, and
    // the liquidator has the disputer's bond.
    let att = w.attest(0, p[0]);
    let f3 = [fund(xk(&w.w), 42)];
    let payer3 = Payer { funds: &f3, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (b, proved) = liq_prove(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, payer3).unwrap();
    let ps = signed("prove a liquidation", &b, &w.keys);
    assert_eq!(proved, LiqObj { liq: lq.liq, state: LiqState::Proved });
    assert_eq!(ps.tx.outputs[0].value, dlive.value - lq.liq.bond as u64);
    let last = ps.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), ((lq.liq.bond / 2) as u64, &p2pk_spk(&xk(&w.w))), "half of the disputer's bond");
    assert_eq!(ps.tx.outputs[1].value, w.script("KpfTLiqProve").value + (kpf_protocol::thin::ORACLE_FEE + lq.liq.bond - lq.liq.bond / 2) as u64, "the other half, in the copy");
    let plive = nth(&ps, 0, proved, 6_530);
    // Proved, it is not settled at once: a dispute period again, in which the side still cures.
    let f4 = [fund(xk(&w.b), 43)];
    let b = liq_settle(&w.lib, &plive, &w.script("KpfTLiqSettle"), Payer { funds: &f4, change_to: xk(&w.b), rate: MIN_FEE_RATE }).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, 50);
    let ss = signed("settle a proved liquidation", &b, &w.keys);
    assert_eq!((ss.tx.outputs[0].value, &ss.tx.outputs[0].script_public_key), ((lq.liq.fee + lq.liq.bond) as u64, &p2pk_spk(&xk(&w.w))));
    let f5 = [fund(xk(&w.a), 44)];
    let (b, cured) = cure(&w.lib, &plive, &w.script("KpfTCure"), Payer { funds: &f5, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    signed("cure a proved liquidation", &b, &w.keys);
    assert_eq!(cured.x.a_margin, 6 * KAS1 + lq.liq.add);
    // A liquidation that is not the rule's is not proved: a fee a sompi above the rule's.
    let (b, bad) = liquidate(&w.lib, &position, &w.script("KpfTLiq"), SIDE_A, T0, p, Some((lq.liq.fee + 1, lq.liq.add)), xk(&w.w), payer).unwrap();
    let bs = signed("liquidate (a fee a sompi above the rule)", &b, &w.keys);
    let (b, bd) = liq_dispute(&w.lib, &nth(&bs, 0, bad, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), Payer { funds: &f2, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let bds = signed("dispute it", &b, &w.keys);
    assert!(liq_prove(&w.lib, &nth(&bds, 0, bd, 6_520), &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, payer3).is_err());
    // It is refuted with the same signatures: the position again, the disputer (A) its bond, the refuter half of
    // the liquidator's, the other half in the script's copy.
    let bdl = nth(&bds, 0, bd, 6_520);
    let refuter = xk(&w.b);
    let (b, back) = liq_refute(&w.lib, &bdl, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, refuter, payer3).unwrap();
    let rs = signed("refute a liquidation", &b, &w.keys);
    assert_eq!(back, position.s);
    assert_eq!((rs.tx.outputs[0].value, rs.tx.outputs[0].covenant.map(|c| c.covenant_id)), (position.value, position.id), "the position again, whole");
    let (bond, n) = (bad.liq.bond, rs.tx.outputs.len());
    assert_eq!(rs.tx.outputs[1].value, w.script("KpfTLiqProve").value + (kpf_protocol::thin::ORACLE_FEE + bond - bond / 2) as u64);
    assert_eq!((rs.tx.outputs[n - 2].value, &rs.tx.outputs[n - 2].script_public_key), (bond as u64, &p2pk_spk(&xk(&w.a))), "the disputer's bond back");
    assert_eq!((rs.tx.outputs[n - 1].value, &rs.tx.outputs[n - 1].script_public_key), ((bond / 2) as u64, &p2pk_spk(&refuter)), "half of the liquidator's bond to who refuted");
    // The true one is not refuted at the price it states.
    assert!(liq_refute(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, refuter, payer3).is_err());
    // The committee's price need not be the claim's. Where the side is not liquidated at the committee's price
    // (9.9: A has its maintenance there) the liquidation is not proved, and refuted.
    let other = w.attest(0, 99 * P10 / 100);
    assert!(liq_prove(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &other), None, payer3).is_err());
    let (b, _) = liq_refute(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &other), None, refuter, payer3).unwrap();
    signed("refute a liquidation the committee's price does not bear", &b, &w.keys);
    let _ = SIDE_B;
}

/// Proof and refutation of a liquidation are each other's complement: at every price the committee may sign, and for
/// every fee and cure a liquidator may state, the signatures pass exactly one of the two entries, as the model says.
#[test]
fn a_disputed_liquidation_is_proved_or_refuted_and_never_both() {
    use kpf_protocol::thin::{LiqObj, LiqState};
    let w = world();
    let mut position = w.position(false);
    position.s.x.a_margin = 6 * KAS1;
    position.s.check().unwrap();
    position.value = position.s.x.floor() as u64;
    let x = position.s.x;
    let own = [97 * P10 / 100, 0];
    let payer_f = [fund(xk(&w.w), 40)];
    let payer = Payer { funds: &payer_f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let f2 = [fund(xk(&w.a), 41)];
    let passes = |b: kpf_protocol::oracle_build::OBuilt| {
        let fs = b.sign(&w.keys).unwrap();
        kpf_protocol::verify::verify(&fs.tx, &fs.entries).is_ok()
    };
    // What a liquidator states from a price of its own: within the rule at that price, and wherever the committee's
    // price is no further than the tolerance from it.
    let mine = position.s.liquidation_own(SIDE_A, T0, own, xk(&w.w)).expect("liquidated even at 9.7 moved toward A");
    let rule = position.s.liquidation(SIDE_A, T0, own, xk(&w.w)).unwrap();
    assert!(mine.within(own) && rule.within(own));
    assert!(mine.fee <= rule.fee && mine.add <= rule.add);
    let g = x.t.g_bps;
    // (To the last basis point but one: the cure a liquidator states is the rule's at its own price, and the span
    // of cures the rule takes at the committee's is exactly the tolerance wide.)
    for bps in [-(g - 1), -g / 2, 0, g / 2, g - 1] {
        assert!(mine.within([own[0] + own[0] * bps / 10_000, 0]), "the liquidator's own claim, the committee {bps} basis points away");
    }
    assert!(position.s.liquidation_own(SIDE_A, T0, [985 * P10 / 1000, 0], xk(&w.w)).is_none() || !x.liquidatable(SIDE_A, [99 * P10 / 100, 0]));
    assert!(position.s.liquidation_own(SIDE_B, T0, own, xk(&w.w)).is_none());
    let hi = x.cure_at(SIDE_A, own, Some(false));
    let lo = x.cure_at(SIDE_A, own, Some(true));
    assert!(lo < hi && rule.add == hi);
    let mut seen = [0usize; 2];
    for (fee, add) in [(mine.fee, mine.add), (rule.fee, rule.add), (0, rule.add), (rule.fee + 1, rule.add), (rule.fee, hi + 1), (rule.fee, lo), (rule.fee, lo - 1), (rule.fee, 1)] {
        let (b, lq) = liquidate(&w.lib, &position, &w.script("KpfTLiq"), SIDE_A, T0, own, Some((fee, add)), xk(&w.w), payer).unwrap();
        let s = signed("liquidate", &b, &w.keys);
        let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), Payer { funds: &f2, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
        let ds = signed("dispute a liquidation", &b, &w.keys);
        let dlive = nth(&ds, 0, dl, 6_520);
        assert_eq!(dl, LiqObj { liq: lq.liq, state: LiqState::Disputed(xk(&w.a)) });
        for price in [own[0], own[0] - own[0] * g / 10_000, own[0] + own[0] * g / 10_000, 96 * P10 / 100, 98 * P10 / 100, 99 * P10 / 100, P10, P10 / 2] {
            let att = w.attest(0, price);
            let within = lq.liq.within([price, 0]);
            let proved = passes(liq_prove_with(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, payer, false).unwrap().0);
            let refuted = passes(liq_refute_with(&w.lib, &dlive, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &att), None, xk(&w.b), payer, false).unwrap().0);
            assert_eq!((proved, refuted), (within, !within), "fee {fee}, cure {add}, the committee's price {price}");
            seen[within as usize] += 1;
        }
    }
    println!("liquidations at the committee's prices: {} within the rule (proved), {} not (refuted)", seen[1], seen[0]);
    assert!(seen[0] >= 8 && seen[1] >= 8);
}

/// Signs a transaction the tool would not build and says whether the engine accepts it.
fn engine_takes(b: &OBuilt, keys: &KeySet) -> bool {
    let sg = b.sign(keys).expect("signed");
    kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok()
}

#[test]
fn the_scripts_themselves_refuse_the_proof_of_what_the_rule_does_not_give() {
    let w = world();
    let f = [fund(xk(&w.w), 50)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // A close: any amount up to what the side is owed at the committee's price is proved (what the rule gives with
    // the tolerance counted against the claimer, and less); a sompi more is not, and is refuted.
    let position = w.position(false);
    let p = [11 * P10 / 10, 0];
    let att = w.attest(0, p[0]);
    let truth = position.s.owed(SIDE_A, p).unwrap();
    let exact = position.s.x.owed_at(SIDE_A, p);
    assert!(truth < exact);
    for (amount, ok) in [(truth, true), (truth - 1, true), (truth + 1, true), (exact, true), (exact + 1, false), (0, true)] {
        let dis = disputed(&w, &position, SIDE_A, p, Some(amount));
        let b = prove_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, payer, false).unwrap();
        assert_eq!(engine_takes(&b, &w.keys), ok, "a close stating {amount} where the committee's price gives {exact}");
        let (b, _) = refute_with(&w.lib, &dis, &w.script("KpfTProve"), (&w.epoch_tables[0], &att), None, xk(&w.b), payer, false).unwrap();
        assert_eq!(engine_takes(&b, &w.keys), !ok, "the refutation of a close stating {amount}");
    }
    // A liquidation: a position with a thin margin, the committee's price 9.7.
    let mut thin = w.position(false);
    thin.s.x.a_margin = 6 * KAS1;
    thin.value = thin.s.x.floor() as u64;
    let lp = [97 * P10 / 100, 0];
    let latt = w.attest(0, lp[0]);
    let rule = thin.s.liquidation(SIDE_A, T0, lp, xk(&w.w)).unwrap();
    let go = |stated: Option<(i64, i64)>, position: &OLive<Position>, side: i64, prices: [i64; 2], att: &Attestation| -> bool {
        let (b, lq) = liquidate(&w.lib, position, &w.script("KpfTLiq"), side, T0, prices, stated, xk(&w.w), payer).unwrap();
        let s = b.sign(&w.keys).unwrap();
        let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), payer).unwrap();
        let ds = b.sign(&w.keys).unwrap();
        let (b, _) = liq_prove_with(&w.lib, &nth(&ds, 0, dl, 6_520), &w.script("KpfTLiqProve"), (&w.epoch_tables[0], att), None, payer, false).unwrap();
        engine_takes(&b, &w.keys)
    };
    assert!(go(Some((rule.fee, rule.add)), &thin, SIDE_A, lp, &latt), "the rule's liquidation is proved");
    assert!(!go(Some((rule.fee + 1, rule.add)), &thin, SIDE_A, lp, &latt), "a fee a sompi above the rule's");
    assert!(go(Some((rule.fee - 1, rule.add)), &thin, SIDE_A, lp, &latt), "a fee below the rule's is the liquidator's own to give up");
    assert!(!go(Some((rule.fee, rule.add + 1)), &thin, SIDE_A, lp, &latt), "a cure a sompi above the rule's");
    // The least cure: what puts the side at the maintenance at the price moved toward it. A sompi less would leave
    // it liquidated again at once.
    let least = thin.s.x.cure_at(SIDE_A, lp, Some(true));
    assert!(least < rule.add);
    assert!(go(Some((rule.fee, least)), &thin, SIDE_A, lp, &latt), "the least cure");
    assert!(!go(Some((rule.fee, least - 1)), &thin, SIDE_A, lp, &latt), "a cure a sompi below the least");
    // The other side is not the one below its maintenance; and at the entry price nobody is.
    assert!(!go(Some((rule.fee, rule.add)), &thin, SIDE_B, lp, &latt), "the side that has its maintenance");
    let even = w.attest(0, P10);
    assert!(!go(Some((rule.fee, rule.add)), &thin, SIDE_A, [P10, 0], &even), "a price that liquidates nobody");
    // A well-margined position is liquidated by no price near its entry.
    assert!(!go(Some((KAS1, KAS1)), &position, SIDE_A, lp, &latt), "a position that has its maintenance at 9.7");
}

#[test]
fn a_no_price_claim_is_settled_at_the_entries_or_ended_by_a_signed_price() {
    use kpf_protocol::thin::TObj;
    let w = world();
    // A position whose "no price" claims wait 5,000 DAA. A has lost at the last prices and says the committee stopped.
    let mut position = w.position(true);
    position.s.x.expiry = 5_000;
    position.s.check().unwrap();
    let since = 6_400;
    let f = [fund(xk(&w.a), 60)];
    let (b, q) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, since, Payer { funds: &f, change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let s = signed("exit claim", &b, &w.keys);
    let live = nth(&s, 0, q, 6_450);
    assert_eq!(live.value, position.value + q.bond as u64);
    assert_eq!(TObj::decode(&TObj::ExitClaim(q).encode()).unwrap(), TObj::ExitClaim(q));
    // Only a party makes it: the same claim without A's key is not signed.
    let mut others = KeySet::new();
    others.add_keypair(w.w);
    assert!(b.sign(&others).is_err());
    let f = [fund(xk(&w.w), 61)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    // Nobody shows a price: after the exit time each party has its margin, the claimer its bond with it.
    let b = exit_settle(&w.lib, &live, &w.script("KpfTExitEnd"), payer).unwrap();
    assert_eq!(b.tx.inputs[0].sequence, 5_000, "the claim proves the exit time's age");
    let ss = signed("settle at the entries", &b, &w.keys);
    assert_eq!((ss.tx.outputs[0].value, &ss.tx.outputs[0].script_public_key), ((position.s.x.a_margin + q.bond) as u64, &p2pk_spk(&xk(&w.a))));
    let last = ss.tx.outputs.last().unwrap();
    assert_eq!((last.value, &last.script_public_key), (position.value - position.s.x.a_margin as u64, &p2pk_spk(&xk(&w.b))));
    let mut young = b.clone();
    young.tx.inputs[0].sequence = 4_999;
    assert!(!engine_takes(&young, &w.keys), "before the exit time has passed");
    // The committee is alive: the second executor shows a signed price of each market signed at the claim's `since` or after
    // (T0 = 6,500), and the claim is ended. The position again; the bond shared by it and the other party.
    let (a1, a2) = (w.attest(0, P10), w.attest(1, 12 * P10 / 10));
    let (b, back) = exit_refute(&w.lib, &live, &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), xk(&w.w), payer).unwrap();
    let rs = signed("refute with two epoch tables", &b, &w.keys);
    assert_eq!(back, position.s);
    assert_eq!((rs.tx.outputs[0].value, rs.tx.outputs[0].covenant.map(|c| c.covenant_id)), (position.value, position.id));
    let n = rs.tx.outputs.len();
    assert_eq!((rs.tx.outputs[n - 2].value, &rs.tx.outputs[n - 2].script_public_key), ((q.bond / 2) as u64, &p2pk_spk(&xk(&w.b))));
    assert_eq!((rs.tx.outputs[n - 1].value, &rs.tx.outputs[n - 1].script_public_key), ((q.bond - q.bond / 2) as u64, &p2pk_spk(&xk(&w.w))));
    // One market's price is not enough for a position of two, and a signed price older than `since` answers nothing: refused by
    // the tool, and by the script when the tool is told not to look.
    assert!(exit_refute(&w.lib, &live, &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), None, xk(&w.w), payer).is_err());
    let (b, q2) = exit(&w.lib, &position, &w.script("KpfTExit"), SIDE_A, T0 + 1, Payer { funds: &[fund(xk(&w.a), 62)], change_to: xk(&w.a), rate: MIN_FEE_RATE }).unwrap();
    let s2 = b.sign(&w.keys).unwrap();
    let later = nth(&s2, 0, q2, 6_600);
    assert!(exit_refute(&w.lib, &later, &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), xk(&w.w), payer).is_err());
    let (b, _) = exit_refute_with(&w.lib, &later, &w.script("KpfTExitRefute"), (&w.epoch_tables[0], &a1), Some((&w.epoch_tables[1], &a2)), xk(&w.w), payer, false).unwrap();
    assert!(!engine_takes(&b, &w.keys), "a signed price signed before the claim's `since`");
    // A claim that names a time to come says nothing: it is ended by anyone, not settled.
    let early = nth(&s2, 0, q2, T0 as u64);
    assert!(exit_settle(&w.lib, &early, &w.script("KpfTExitEnd"), payer).is_err());
    assert!(exit_early(&w.lib, &later, &w.script("KpfTExit"), xk(&w.w), payer).is_err());
    let (b, back) = exit_early(&w.lib, &early, &w.script("KpfTExit"), xk(&w.w), payer).unwrap();
    signed("end a claim of a time to come", &b, &w.keys);
    assert_eq!(back, position.s);
}

/// A small fixed-seed generator (the test is the same every run).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A value in [lo, hi).
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

/// Review T1, the lead it did not reach: the two proofs against the model, on positions, prices and sides drawn at
/// random. For each case the script's answer to "is this what the rule gives" is the model's, for the close (the
/// amount) and for the liquidation (the trigger, the fee and the cure together), and a sompi beside the rule is
/// refused. `KpfTLiqProve` computes the position's worth in another algebraic form than the model; this is what holds
/// the two together.
#[test]
fn the_proofs_agree_with_the_model_on_random_positions() {
    let w = world();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let f = [fund(xk(&w.w), 70)];
    let payer = Payer { funds: &f, change_to: xk(&w.w), rate: MIN_FEE_RATE };
    let (mut closes, mut no_close, mut liqs, mut no_liq, mut cases) = (0, 0, 0, 0, 0);
    while cases < 160 {
        let two = r.range(0, 3) > 0;
        let mut position = w.position(two);
        for i in 0..if two { 2 } else { 1 } {
            let l = &mut position.s.x.legs[i];
            l.size = r.range(1, 40) * l.scale + r.range(0, l.scale);
            l.dir = if r.range(0, 2) == 1 { LONG } else { SHORT };
            l.entry = r.range(5 * P10 / 10, 25 * P10 / 10);
        }
        // Margins from a tenth of a KAS to some eighty: thin positions are where the liquidation's rule decides.
        let top = if r.range(0, 2) == 1 { 12 * KAS1 } else { 80 * KAS1 };
        position.s.x.a_margin = r.range(KAS1 / 10, top);
        let top = if r.range(0, 2) == 1 { 12 * KAS1 } else { 80 * KAS1 };
        position.s.x.b_margin = r.range(KAS1 / 10, top);
        if position.s.check().is_err() {
            continue;
        }
        cases += 1;
        position.value = position.s.x.floor() as u64;
        let near = |r: &mut Rng, e: i64| e / 100 * r.range(60, 141) + r.range(0, 1_000);
        let p = [near(&mut r, position.s.x.legs[0].entry), if two { near(&mut r, position.s.x.legs[1].entry) } else { 0 }];
        let side = if r.range(0, 2) == 1 { SIDE_A } else { SIDE_B };
        let a1 = w.attest(0, p[0]);
        let a2 = two.then(|| w.attest(1, p[1]));
        let t2 = a2.as_ref().map(|a| (&w.epoch_tables[1], a));
        let say = format!("case {cases}: {:?} side {side} at {p:?}", position.s.x);
        // The close: what the model gives is proved; where the model allows no close (it would escape a liquidation)
        // nothing is, whatever is stated.
        let close = |amount: i64| -> bool {
            let who = if side == SIDE_A { &w.a } else { &w.b };
            let (b, c) = close(&w.lib, &position, &w.script("KpfTClose"), side, T0, p, Some(amount), payer).unwrap();
            let _ = who;
            let s = b.sign(&w.keys).unwrap();
            let (b, d) = dispute(&w.lib, &nth(&s, 0, c, 6_510), &w.script("KpfTDispute"), xk(&w.w), payer).unwrap();
            let ds = b.sign(&w.keys).unwrap();
            let dl = nth(&ds, 0, d, 6_520);
            let b = prove_with(&w.lib, &dl, &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), t2, payer, false).unwrap();
            let proved = engine_takes(&b, &w.keys);
            // Proof and refutation are each other's complement, and the model says which.
            let (b, _) = refute_with(&w.lib, &dl, &w.script("KpfTProve"), (&w.epoch_tables[0], &a1), t2, xk(&w.b), payer, false).unwrap();
            assert_eq!(engine_takes(&b, &w.keys), !proved, "{say}: a close of {amount} is proved and refuted, or neither");
            assert_eq!(d.claim.within(p), proved, "{say}: the model and the script differ on a close of {amount}");
            proved
        };
        match position.s.owed(side, p) {
            Some(v) => {
                closes += 1;
                assert!(close(v), "{say}: the rule's close of {v} is not proved");
                let exact = position.s.x.owed_at(side, p);
                assert!(v <= exact && close(exact), "{say}: what the side is owed at the committee's prices, {exact}, is not proved");
                if exact < position.s.pot() {
                    assert!(!close(exact + 1), "{say}: a sompi above what the committee's prices give is proved");
                }
                if v > 0 {
                    assert!(close(v - 1), "{say}: a sompi below the rule's close is not proved");
                }
            }
            None => {
                no_close += 1;
                assert!(!close(position.s.x.owed_at(side, p)), "{say}: a close the rule does not allow is proved");
                assert!(!close(0), "{say}: a close of nothing the rule does not allow is proved");
            }
        }
        // The liquidation: the model's is proved with its fee and its cure; where the model liquidates nobody, the fee
        // and the cure its formulas would give are not.
        let liq = |fee: i64, add: i64| -> bool {
            let (b, lq) = liquidate(&w.lib, &position, &w.script("KpfTLiq"), side, T0, p, Some((fee, add)), xk(&w.w), payer).unwrap();
            let s = b.sign(&w.keys).unwrap();
            let (b, dl) = liq_dispute(&w.lib, &nth(&s, 0, lq, 6_510), &w.script("KpfTLiqDispute"), xk(&w.a), payer).unwrap();
            let ds = b.sign(&w.keys).unwrap();
            let live = nth(&ds, 0, dl, 6_520);
            let (b, _) = liq_prove_with(&w.lib, &live, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &a1), t2, payer, false).unwrap();
            let proved = engine_takes(&b, &w.keys);
            let (b, _) = liq_refute_with(&w.lib, &live, &w.script("KpfTLiqProve"), (&w.epoch_tables[0], &a1), t2, xk(&w.b), payer, false).unwrap();
            assert_eq!(engine_takes(&b, &w.keys), !proved, "{say}: a liquidation (fee {fee}, cure {add}) is proved and refuted, or neither");
            assert_eq!(dl.liq.within(p), proved, "{say}: the model and the script differ on a liquidation (fee {fee}, cure {add})");
            proved
        };
        match position.s.liquidation(side, T0, p, xk(&w.w)) {
            Some(l) => {
                liqs += 1;
                assert!(liq(l.fee, l.add), "{say}: the rule's liquidation (fee {}, cure {}) is not proved", l.fee, l.add);
                assert!(!liq(l.fee, l.add + 1), "{say}: a cure a sompi above the rule's is proved");
                if l.fee > 0 {
                    assert!(liq(l.fee - 1, l.add), "{say}: a fee a sompi below the rule's is not proved");
                }
                let least = position.s.x.cure_at(side, p, Some(true));
                assert!(least >= 1 && least <= l.add, "{say}: the least cure {least} and the rule's {}", l.add);
                assert!(liq(l.fee, least), "{say}: the least cure is not proved");
                if least > 1 {
                    assert!(!liq(l.fee, least - 1), "{say}: a cure a sompi below the least is proved");
                }
                if l.fee < position.s.pot() {
                    assert!(!liq(l.fee + 1, l.add), "{say}: a fee a sompi above the rule's is proved");
                }
            }
            None => {
                no_liq += 1;
                let (fee, add) = (position.s.x.fee_of(side, p).clamp(0, position.s.pot()), position.s.x.cure_add(side, p).max(1));
                assert!(!liq(fee, add), "{say}: a liquidation the rule does not give is proved");
            }
        }
    }
    println!("random positions: {closes} closes proved, {no_close} refused; {liqs} liquidations proved, {no_liq} refused");
    assert!(closes >= 40 && no_close >= 5 && liqs >= 15 && no_liq >= 40, "the cases do not reach every branch: {closes} {no_close} {liqs} {no_liq}");
}
