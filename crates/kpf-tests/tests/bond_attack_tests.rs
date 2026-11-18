//! Attacks on `KpfBond` and its stand-ins (docs/DESIGN-ORACLE-IMPL.md, "Attacks"), in the manner of the reviews:
//! each is recorded as `holds_` (the attack fails) or `exploit_` (it works, within the stated bound). The last test
//! replays seeded hostile sequences generated from the model against the contracts, step by step: the model and the
//! script engine must agree on every step, and the accepted states must keep the invariants of Z3.

mod bondkit;
mod common;

use bondkit::*;
use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionId, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::*;
use secp256k1::Keypair;

const SLOT_ID: Hash = Hash::from_bytes([0x51; 32]);
const K: u64 = 1_000_000_000;
const LOCK: u64 = 1_000;

fn uw() -> Keypair {
    keypair(31)
}

fn slot_of(b: Bond, value: u64) -> LSlot {
    LSlot { op: outpoint(100), value, daa: 0, id: Some(SLOT_ID), b }
}

fn slot() -> LSlot {
    slot_of(Bond::new(xkey(&uw()), market(), 0), K)
}

fn pair_of(c: i64, x: i64, daa: u64, n: u8) -> LPair {
    LPair {
        op: outpoint(110 + n),
        value: 200_000_000,
        daa,
        id: Hash::from_bytes([0x60 + n; 32]),
        p: StubPair { m: market(), uw_id: SLOT_ID.as_bytes(), uw_key: xkey(&uw()), owner: xkey(&keypair(33)), cover: c, expiry: x },
    }
}

fn src(rel: &str) -> String {
    contract_source(rel)
}

fn block<'a>(s: &'a str, begin: &str, end: &str) -> &'a str {
    let a = s.find(begin).unwrap_or_else(|| panic!("{begin}"));
    let b = s.find(end).unwrap_or_else(|| panic!("{end}"));
    &s[a..b]
}

fn constant(s: &str, name: &str) -> Option<String> {
    let key = format!("int constant {name} = ");
    s.lines().find_map(|l| l.trim().strip_prefix(&key).map(|v| v.trim_end_matches(';').to_string()))
}

// ---------------------------------------------------------------- one text, one tag

/// The bond bounds are one text in `KpfBond` and both stand-ins, with the same constants; the constants they share
/// with the book market's check (`KpfOpen.checkMarket`) have the same values; and the bind proof is one text in both
/// stand-ins. So no contract can relax or tighten a bound alone (the rule of review 5, F2).
#[test]
fn holds_the_bond_bounds_and_the_bind_proof_are_one_text() {
    let (bond, pair, open, kopen) = (src("KpfBond.sil"), src("stubs/KpfPairStub.sil"), src("stubs/KpfOpenStub.sil"), src("KpfOpen.sil"));
    let b = block(&bond, "// BEGIN bond bounds", "// END bond bounds");
    assert_eq!(b, block(&pair, "// BEGIN bond bounds", "// END bond bounds"));
    assert_eq!(b, block(&open, "// BEGIN bond bounds", "// END bond bounds"));
    for n in ["BPS", "RHO_MAX_BPS", "RING", "CSV_LIMIT"] {
        assert!(b.contains(n), "{n} is used by the bounds");
        let v = constant(&bond, n).unwrap();
        assert_eq!(constant(&pair, n).as_deref(), Some(v.as_str()), "{n} in KpfPairStub");
        assert_eq!(constant(&open, n).as_deref(), Some(v.as_str()), "{n} in KpfOpenStub");
    }
    for n in ["BPS", "CSV_LIMIT", "LOCK_TIME_THRESHOLD"] {
        assert_eq!(constant(&bond, n), constant(&kopen, n), "{n}: KpfBond and KpfOpen");
    }
    for n in ["STATUS_SLOT", "OUT_REF"] {
        let v = constant(&bond, n).unwrap();
        assert_eq!(constant(&pair, n).as_deref(), Some(v.as_str()));
        assert_eq!(constant(&open, n).as_deref(), Some(v.as_str()));
    }
    // The library's bounds are the same numbers.
    assert_eq!(constant(&bond, "RHO_MAX_BPS").unwrap(), RHO_MAX_BPS.to_string());
    assert_eq!(constant(&bond, "RING").unwrap(), RING.to_string());
    assert_eq!(constant(&bond, "VALUE_CLAMP").unwrap(), VALUE_CLAMP.to_string());
    assert_eq!(constant(&bond, "REF_LIMIT").unwrap(), REF_LIMIT.to_string());
    assert_eq!(constant(&pair, "BOND_STATE_LEN").unwrap(), BOND_STATE_LEN.to_string());
    let p = block(&pair, "// BEGIN bind proof", "// END bind proof");
    assert_eq!(p, block(&open, "// BEGIN bind proof", "// END bind proof"));
    // Every entry of the three contracts runs the bounds (withdraw only for a slot with an id).
    assert_eq!(bond.matches("checkBond();").count(), 5, "bind, advance, split, unbond, withdraw");
    assert!(pair.matches("checkBond();").count() >= 2 && open.contains("checkBond();"));
}

/// The stand-ins identify the bind entry by its dispatch tag, a build constant. The compiler derives a tag from the
/// entry's signature alone (the first 4 bytes of the blake3 of `bind(byte[8],...,sig)`): a probe with the same
/// signature and nothing else in common gets the same tag, so the constant follows `KpfBond.bind`'s signature and
/// nothing else; a change of that signature changes the tag and the stand-ins refuse every bind until rebuilt.
#[test]
fn holds_the_bind_tag_is_the_bond_artifacts_dispatch_tag() {
    let probe = format!(
        "pragma silverscript ^0.1.0;\ncontract TagProbe(int init_t) {{\n    int t = init_t;\n    entry bind({}sig s) {{\n        require(t == 1);\n    }}\n}}\n",
        (0..17).map(|i| format!("byte[8] a{i}, ")).collect::<String>()
    );
    let art = silverscript_lang::compiler::compile_to_sil_abi_artifact_with_options(&probe, &[int(1)], silverscript_lang::compiler::CompileOptions::default()).expect("probe");
    let tag = *art.contracts.values().next().unwrap().entries["bind"].dispatch_tag.as_bytes();
    assert_eq!(tag, kit().tag);
    let other = *kit().bond.contracts.values().next().unwrap().entries["advance"].dispatch_tag.as_bytes();
    assert_ne!(other, kit().tag, "another entry has another tag");
}

// ---------------------------------------------------------------- over-underwriting

/// Found by the model fuzz (Z3) and fixed: a slot written by a genesis with a negative bucket. Before the fix, a bind
/// into that bucket of claims c added up to the bucket's deficit, leaving it 0: the claimants read c, the slot counted
/// nothing, and the invariant (checked on the result only) held. Now the target bucket must be >= 0 before the add.
#[test]
fn holds_a_negative_bucket_written_by_a_genesis_cannot_hide_cover() {
    let mut b = Bond::new(xkey(&uw()), market(), 8);
    b.ring[10] = -(K as i64); // the bucket of expiry 1,900
    let s = slot_of(b, K);
    let a = BindArgs::new(1_000, 1_900, 0, &[Claim::input(0, K as i64)]);
    let m = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64);
    assert!(m.is_err(), "the model refuses");
    let mut fixed = s.b.clone();
    fixed.ring[10] = 0;
    let t = bind_alone(&s, &a, &fixed, K, LOCK, 10, &uw());
    assert!(!agree("the hiding bind", &t, &m));
    // Another bucket negative: refused too (ringSum checks every bucket).
    let mut b2 = Bond::new(xkey(&uw()), market(), 8);
    b2.ring[3] = -1;
    let s2 = slot_of(b2, K);
    let m2 = s2.b.bind(&s2.ctx(LOCK, 10), &a, K as i64);
    assert!(!agree("another negative bucket", &bind_alone(&s2, &a, &s2.b, K, LOCK, 10, &uw()), &m2));
}

/// rho above 5 is refused by the bond (and by the stand-ins, the same text); rho exactly 5 is accepted.
#[test]
fn holds_no_market_runs_above_rho_five() {
    for (rho, want) in [(50_000i64, true), (50_001, false), (9_999, false), (10_000, true)] {
        let s = slot_of(Bond::new(xkey(&uw()), market_with(BondParams { rho_bps: rho, ..params() }), 0), K);
        let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 1)]);
        let m = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64);
        assert_eq!(agree("rho", &bind_alone(&s, &a, &m.clone().unwrap_or_else(|_| s.b.clone()), K, LOCK, 10, &uw()), &m), want, "rho_bps {rho}");
    }
}

/// A fake bond: KpfBond's source with the capacity check removed compiles to another template. The stand-in pair
/// refuses its bind (template hash), so a look-alike bond cannot count cover for a pair, however it is written.
#[test]
fn holds_a_look_alike_bond_template_cannot_bind_a_pair() {
    let fake_src = src("KpfBond.sil").replace("require(ringSum(r2b) <= capacity(v));", "require(ringSum(r2b) >= 0);");
    assert_ne!(fake_src, src("KpfBond.sil"));
    let s0 = slot();
    let fake = silverscript_lang::compiler::compile_to_sil_abi_artifact_with_options(&fake_src, &s0.b.ctor(), silverscript_lang::compiler::CompileOptions::default()).expect("fake");
    let ft = kpf_protocol::artifacts::Template::of("Fake", &fake);
    assert_ne!(ft.hash, kit().bond_t.hash);
    // The fake already counts 100 KAS on a 10 KAS slot and binds 10 KAS more for the pair; the pair is renewed by it.
    let pr = pair_of(K as i64, 1_900, LOCK, 0);
    let mut b = Bond::new(xkey(&uw()), market(), 8);
    b.ring[10] = 10_000_000_000;
    let a = BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, K as i64)]);
    let mut b2 = b.clone();
    b2.j0 = 16;
    b2.ring = [0; RING];
    b2.ring[2] = 10_000_000_000; // the old bucket of 1,900, moved by 8 steps
    b2.ring[10] = K as i64;
    let fake_in = |st: &Bond| {
        let art = kpf_protocol::artifacts::splice(&fake, &st.state_bytes());
        In { outpoint: outpoint(100), utxo: UtxoEntry::new(K, p2sh(&art), 0, false, Some(SLOT_ID)), spend: Spend::Signed { art, entry: "bind", args: bind_args(&a, &uw()) } }
    };
    let ins = [pair_in(pr.op, pr.value, Some(pr.id), pr.daa, &pr.p, "renew", vec![iv(1), iv(0)]), fake_in(&b), funder(201)];
    let cont = kpf_protocol::artifacts::splice(&fake, &b2.state_bytes());
    let outs = vec![pair_out(pr.value, &StubPair { expiry: 2_700, ..pr.p.clone() }, bind(0, pr.id)), out(K, p2sh(&cont), bind(1, SLOT_ID)), change(900_000_000)];
    let (tx, e) = build_ext(&ins, outs, 1_800, &[0, 100, 0]);
    assert_eq!(passes(&tx, &e), vec![false, true, true], "the fake bind passes, the pair refuses it");
}

/// A P2PK output carrying the slot's covenant id (a genesis twin authored with the slot) cannot stand in for the slot:
/// the pair reads the slot input's redeem script and template.
#[test]
fn holds_a_p2pk_twin_of_the_slot_cannot_renew_a_pair() {
    let pr = pair_of(5, 1_900, LOCK, 0);
    let ins = [pair_in(pr.op, pr.value, Some(pr.id), pr.daa, &pr.p, "renew", vec![iv(1), iv(0)]), p2pk_in(outpoint(100), K, Some(SLOT_ID), &uw()), funder(201)];
    let outs = vec![pair_out(pr.value, &StubPair { expiry: 2_700, ..pr.p.clone() }, bind(0, pr.id)), out(K, p2pk_spk(&uw()), bind(1, SLOT_ID)), change(900_000_000)];
    let (tx, e) = build_ext(&ins, outs, 1_800, &[0, 0, 0]);
    assert_eq!(passes(&tx, &e), vec![false, true, true]);
}

/// Twins of the bond made by a genesis (two KpfBond outputs of one id, any states) are each real KAS with their own
/// ring: each binds within its own value, so the id as a whole counts at most rho x its total KAS. Shown: a genesis
/// of two slots of 10 KAS, one binds 10 KAS, the other binds 10 KAS, neither 1 sompi more.
#[test]
fn holds_twin_slots_of_one_id_each_count_within_their_own_value() {
    let b = Bond::new(xkey(&uw()), market(), 0);
    let s1 = LSlot { op: outpoint(100), ..slot_of(b.clone(), K) };
    let s2 = LSlot { op: outpoint(101), ..slot_of(b, K) };
    for s in [&s1, &s2] {
        for (c, want) in [(K as i64, true), (K as i64 + 1, false)] {
            let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, c)]);
            let m = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64);
            assert_eq!(agree("twin", &bind_alone(s, &a, &m.clone().unwrap_or_else(|_| s.b.clone()), K, LOCK, 10, &uw()), &m), want);
        }
    }
}

// ---------------------------------------------------------------- cover released too early, or never

/// The pair relies on its cover until X (rely); the slot keeps counting it until X + tail (advance, unbond); no entry
/// releases it earlier, whoever spends the slot.
#[test]
fn holds_cover_is_not_released_while_the_pair_still_relies_on_it() {
    let s0 = slot();
    let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 5)]);
    let b = s0.b.bind(&s0.ctx(LOCK, 10), &a, K as i64).unwrap();
    let s = LSlot { op: outpoint(101), daa: LOCK, b, ..s0 };
    let p = pair_of(5, 1_900, 1_900, 0);
    assert!(agree("relied on at X", &rely_tx(&p), &p.p.rely(1_900, true)));
    for lock in [1_900u64, 2_000, 2_099] {
        let d = max_adv(&s.b, lock);
        let m = s.b.advance(&s.ctx(lock, 0), 11, K as i64);
        assert!(m.is_err() && s.b.advance(&s.ctx(lock, 0), d.max(1), K as i64).map(|b| b.counted_at(1_900) == 5).unwrap_or(true));
        assert!(!agree("dropping the cover early", &advance_tx(&s, 11, &s.b, K, lock), &m));
        let u = s.b.unbond(&s.ctx(lock, 100), d, K as i64);
        assert!(!agree("unbonding early", &unbond_tx(&s, d, &s.b, lock, 100, &uw()), &u));
    }
}

fn max_adv(b: &Bond, lock: u64) -> i64 {
    ((lock as i64 - b.m.p.tail).div_euclid(b.m.p.step) - b.j0).clamp(0, 64)
}

/// A slot is never stuck: from any state its own entries write, the owner alone gets its KAS out (advance needs only
/// the calendar; unbond and withdraw only the owner). End to end: bind, wait, advance, unbond, withdraw.
#[test]
fn holds_a_slot_always_has_an_exit_that_needs_nobody() {
    let s0 = slot();
    let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, K as i64)]);
    let b1 = s0.b.bind(&s0.ctx(LOCK, 10), &a, K as i64).unwrap();
    let t1 = bind_alone(&s0, &a, &b1, K, LOCK, 10, &uw());
    assert!(ok(&t1.0, &t1.1));
    let s1 = LSlot { op: TransactionOutpoint { transaction_id: t1.0.id(), index: 0 }, daa: LOCK, b: b1, ..s0 };
    let b2 = s1.b.advance(&s1.ctx(2_100, 0), 11, K as i64).unwrap();
    let t2 = advance_tx(&s1, 11, &b2, K, 2_100);
    assert!(ok(&t2.0, &t2.1), "anyone advances past the liability end");
    let s2 = LSlot { op: TransactionOutpoint { transaction_id: t2.0.id(), index: 0 }, daa: 2_100, b: b2, ..s1 };
    let b3 = s2.b.unbond(&s2.ctx(2_110, 10), 0, K as i64).unwrap();
    let t3 = unbond_tx(&s2, 0, &b3, 2_110, 10, &uw());
    assert!(ok(&t3.0, &t3.1));
    let s3 = LSlot { op: TransactionOutpoint { transaction_id: t3.0.id(), index: 0 }, daa: 2_110, b: b3, ..s2 };
    let t4 = withdraw_tx(&s3, 50, &uw(), false);
    assert!(ok(&t4.0, &t4.1), "the owner's KAS is out");
    // A state only a genesis can write (an unknown status) has no entry: it is its author's own loss, not a path.
    let mut dead = slot();
    dead.b.status = 7;
    let m = dead.b.unbond(&dead.ctx(LOCK, 10), 0, K as i64);
    assert!(!agree("unknown status", &unbond_tx(&dead, 0, &dead.b, LOCK, 10, &uw()), &m));
}

// ---------------------------------------------------------------- value out of a bond

/// Value leaves a slot only by its owner's withdraw of an unbonding slot (or an id-less one): every other entry keeps
/// at least the slot's value in its continuation, and every owner entry needs the owner's SIGHASH_ALL signature.
#[test]
fn holds_value_leaves_a_bond_only_by_its_owners_withdraw() {
    let thief = keypair(40);
    let s = slot();
    let a = BindArgs::new(1_000, 1_900, 8, &[]);
    let b1 = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64).unwrap();
    let t = bind_alone(&s, &a, &b1, K - 1, LOCK, 10, &uw());
    assert!(!ok(&t.0, &t.1), "bind keeps the value");
    let t = bind_alone(&s, &a, &b1, K, LOCK, 10, &thief);
    assert!(!ok(&t.0, &t.1), "bind needs the owner");
    let b2 = s.b.advance(&s.ctx(LOCK, 0), 8, K as i64).unwrap();
    let t = advance_tx(&s, 8, &b2, K - 1, LOCK);
    assert!(!ok(&t.0, &t.1), "advance keeps the value");
    let mut c = s.ctx(LOCK, 10);
    c.cov_out = 2;
    let (ba, bb) = s.b.split(&c, &[0; RING], 500_000_000, 500_000_000).unwrap();
    let t = split_tx(&s, &[0; RING], &ba, 500_000_000, &bb, 499_999_999, LOCK, 10, &uw());
    assert!(!ok(&t.0, &t.1), "split keeps the value");
    let t = split_tx(&s, &[0; RING], &ba, 500_000_000, &bb, 500_000_000, LOCK, 10, &thief);
    assert!(!ok(&t.0, &t.1), "split needs the owner");
    let u = s.b.unbond(&s.ctx(LOCK, 10), 0, K as i64).unwrap();
    let t = unbond_tx(&s, 0, &u, LOCK, 10, &thief);
    assert!(!ok(&t.0, &t.1), "unbond needs the owner");
    let mut ub = slot();
    ub.b.status = STATUS_UNBONDING;
    let t = withdraw_tx(&ub, 50, &thief, false);
    assert!(!ok(&t.0, &t.1), "withdraw needs the owner");
    let t = withdraw_tx(&slot(), 1_000, &uw(), false);
    assert!(!ok(&t.0, &t.1), "an active slot cannot be withdrawn, even by its owner");
}

// ---------------------------------------------------------------- pairs that claim cover they were never given

/// A pair renews only with a bind of its own slot that names it and covers it; without one, or with another pair's
/// claim, it is refused (the path tests cover each variation; here: no slot at all, and a slot input of another kind).
#[test]
fn holds_a_pair_cannot_renew_without_its_slots_bind() {
    let pr = pair_of(5, 1_900, LOCK, 0);
    let ins = [pair_in(pr.op, pr.value, Some(pr.id), pr.daa, &pr.p, "renew", vec![iv(1), iv(0)]), funder(201)];
    let outs = vec![pair_out(pr.value, &StubPair { expiry: 2_700, ..pr.p.clone() }, bind(0, pr.id)), change(900_000_000)];
    let (tx, e) = build_ext(&ins, outs, 1_800, &[0, 0]);
    assert!(!passes(&tx, &e)[0], "no slot input");
}

/// The residual exposure of the note (2 §5.2), bounded here: a pair written by a genesis that names a real slot
/// (uwId, uwKey) was never counted. The stand-in's `rely` (what the next slice's price claim requires) accepts it only
/// while its expiry lies within one period plus one step of the pair UTXO's own DAA score, which no pair a template
/// writes ever exceeds; so a look-alike pair relies on uncounted cover for at most `term + step` after its genesis, and
/// any later renewal is counted. If the slot's owner is the attacker it can vouch such pairs beyond rho for that long;
/// an entrant's wallet must check the pair's genesis (an indexer fact), as the note says.
#[test]
fn exploit_a_genesis_pair_relies_on_a_slot_it_never_bound_for_at_most_one_period() {
    // Created at DAA 5,000 with an expiry 900 later: relied on, uncounted.
    let p = pair_of(1_000_000_000_000, 5_900, 5_000, 0);
    assert!(agree("uncounted pair, within one period", &rely_tx(&p), &p.p.rely(5_000, true)));
    // An expiry further out is refused at once.
    let far = pair_of(1_000_000_000_000, 6_000, 5_000, 1);
    assert!(!agree("uncounted pair, beyond one period", &rely_tx(&far), &far.p.rely(5_000, true)));
    // After its expiry it is refused; to continue it needs a renewal, which the slot counts.
    let late = LPair { daa: 5_901, ..p.clone() };
    assert!(!agree("after its expiry", &rely_tx(&late), &late.p.rely(5_901, true)));
    // An honest pair after a renewal at the earliest moment sits exactly on the bound.
    let honest = pair_of(5, 2_700, 1_800, 2);
    assert!(agree("honest renewal at the earliest moment", &rely_tx(&honest), &honest.p.rely(1_800, true)));
}

// ---------------------------------------------------------------- the model and the contract agree

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            lo
        } else {
            lo + self.below((hi - lo + 1) as u64) as i64
        }
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn near(&mut self, v: i64) -> i64 {
        match self.below(12) {
            0 => v - 1,
            1 => v + 1,
            2 => v - 100,
            3 => v + 100,
            _ => v,
        }
    }
}

struct EW {
    now: u64,
    slots: Vec<LSlot>,
    pairs: Vec<(LPair, bool)>,
    n: u32,
    steps: usize,
    accepted: [usize; 8],
    /// Slots written by a genesis (any state) and not yet rewritten by bind or split.
    forged: std::collections::HashSet<TransactionOutpoint>,
}

impl EW {
    fn fresh_op(&mut self) -> TransactionOutpoint {
        self.n += 1;
        let mut b = [0u8; 32];
        b[..4].copy_from_slice(&self.n.to_le_bytes());
        b[31] = 0xee;
        TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: 0 }
    }

    fn check(&self) {
        let p = params();
        for s in &self.slots {
            if s.b.status == STATUS_SLOT && !self.forged.contains(&s.op) {
                let sum = ring_sum(&s.b.ring).expect("no negative bucket in a slot an entry wrote");
                assert!(sum <= capacity(s.value as i64, p.rho_bps), "slot over capacity");
            }
        }
        // Per expiry still liable, the credited pairs' cover is at most what the slots count.
        let mut xs: Vec<i64> = self.pairs.iter().filter(|(q, c)| *c && q.p.expiry + p.tail > self.now as i64).map(|(q, _)| q.p.expiry).collect();
        xs.sort();
        xs.dedup();
        for x in xs {
            let want: i64 = self.pairs.iter().filter(|(q, c)| *c && q.p.expiry == x).map(|(q, _)| q.p.cover).sum();
            let have: i64 = self.slots.iter().map(|s| s.b.counted_at(x).max(0)).sum();
            assert!(want <= have, "expiry {x}: relied {want} > counted {have}");
        }
    }
}

fn utxo(tx: &Transaction, i: usize) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: tx.id(), index: i as u32 }
}

/// One hostile or honest step, built as a transaction, judged by the model and by the engine (they must agree).
fn ew_step(w: &mut EW, rng: &mut Rng) {
    let p = params();
    let signer = if rng.chance(92) { uw() } else { keypair(40) };
    let signed = signer == uw();
    w.steps += 1;
    w.now += rng.range(0, 12) as u64;
    let now = w.now;
    match rng.below(11) {
        // Open a pair (stand-in order + bind), with perturbed arguments.
        0 | 1 => {
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            let t = rng.near(now as i64).clamp(0, now as i64 + 1);
            let back = rng.range(0, 2);
            let x = rng.near(((t + p.term + p.step) / p.step - back) * p.step);
            let d0 = max_adv(&s.b, now);
            let d = rng.near(d0).max(-1);
            let free = capacity(s.value as i64, p.rho_bps) - ring_sum(&s.b.ring).unwrap_or(0);
            let base = match rng.below(3) {
                0 => free,
                1 => free / 3,
                _ => rng.range(1, 1_000),
            };
            let c = rng.near(base).max(1);
            let claim_c = rng.near(c);
            let o = StubOpen { m: market(), uw_accept: if rng.chance(5) { [0x5a; 32] } else { [0; 32] }, owner: xkey(&keypair(33)), cover: c };
            let a = BindArgs::new(t, x, d, &[Claim::output(1, claim_c)]);
            let mut ctx = s.ctx(now, now - s.daa);
            ctx.signed = signed;
            let bm = s.b.bind(&ctx, &a, s.value as i64);
            let view = SlotView { bond: &s.b, id: s.id_bytes(), args: &a, canonical_bind: true };
            let twins = usize::from(rng.chance(5));
            let fm = o.fill(true, 1, Some(&view), 0, x, 3 + twins, twins);
            let pr = fm.clone().unwrap_or_else(|_| o.pair_for(s.id_bytes(), xkey(&uw()), x));
            let both: Result<(), Refusal> = bm.clone().and(fm.clone()).map(|_| ());
            // Build with a unique funder outpoint so every pair gets its own genesis id.
            let fop = w.fresh_op();
            let ins = [
                open_in(w.fresh_op(), 200_000_000, Some(Hash::from_bytes([0x53; 32])), 0, &o, 1, 1, 0, x),
                bond_in(s.op, s.value, s.id, s.daa, &s.b, "bind", bind_args(&a, &signer)),
                In { outpoint: fop, utxo: UtxoEntry::new(1_000_000_000, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) },
            ];
            let b2 = bm.clone().unwrap_or_else(|_| s.b.clone());
            let mut outs: Vec<TransactionOutput> = vec![bond_out(s.value, &b2, s.id.and_then(|id| bind(1, id))), pair_out(200_000_000, &pr, None), change(800_000_000)];
            if twins == 1 {
                outs.push(pair_out(1_000_000, &pr, None));
            }
            let group: Vec<usize> = if twins == 1 { vec![1, 3] } else { vec![1] };
            let g = genesis_id(fop, &outs, &group);
            for gi in &group {
                outs[*gi].covenant = bind(2, g);
            }
            let tx = build_ext(&ins, outs, now, &[0, now - s.daa, 0]);
            if agree("open", &tx, &both) {
                w.slots[i] = LSlot { op: utxo(&tx.0, 0), daa: now, b: b2, ..s };
                w.pairs.push((LPair { op: utxo(&tx.0, 1), value: 200_000_000, daa: now, id: g, p: pr }, true));
                w.accepted[0] += 1;
            }
        }
        // Renew up to three pairs of one expiry through one slot.
        2 | 3 => {
            if w.pairs.is_empty() {
                return;
            }
            let due: Vec<usize> = (0..w.pairs.len()).filter(|&q| { let x = w.pairs[q].0.p.expiry; x - 100 <= now as i64 && now as i64 <= x + 100 }).collect();
            let q0 = if !due.is_empty() && rng.chance(85) { due[rng.below(due.len() as u64) as usize] } else { rng.below(w.pairs.len() as u64) as usize };
            let x0 = w.pairs[q0].0.p.expiry;
            let uw_id = w.pairs[q0].0.p.uw_id;
            let mut idx: Vec<usize> = (0..w.pairs.len()).filter(|&q| (w.pairs[q].0.p.expiry == x0 && w.pairs[q].0.p.uw_id == uw_id) || rng.chance(2)).collect();
            idx.truncate(rng.range(1, 3) as usize);
            let own: Vec<usize> = (0..w.slots.len()).filter(|&k| w.slots[k].id_bytes() == uw_id && w.slots[k].b.status == STATUS_SLOT).collect();
            let i = if !own.is_empty() && rng.chance(85) { own[rng.below(own.len() as u64) as usize] } else { rng.below(w.slots.len() as u64) as usize };
            let s = w.slots[i].clone();
            let x = rng.near(x0 + p.term);
            let t = now as i64;
            let d = rng.near(max_adv(&s.b, now));
            let mut claims: Vec<Claim> = idx.iter().enumerate().map(|(k, &q)| Claim::input(k, rng.near(w.pairs[q].0.p.cover).max(0))).collect();
            if claims.len() > 1 && rng.chance(10) {
                claims.swap(0, 1);
            }
            let a = BindArgs::new(t, x, d, &claims);
            let mut ctx = s.ctx(now, now - s.daa);
            ctx.signed = signed;
            let bm = s.b.bind(&ctx, &a, s.value as i64);
            let view = SlotView { bond: &s.b, id: s.id_bytes(), args: &a, canonical_bind: true };
            let ks: Vec<i64> = (0..idx.len() as i64).collect();
            let pms: Vec<Result<StubPair, Refusal>> = idx.iter().enumerate().map(|(k, &q)| w.pairs[q].0.p.renew(true, k, Some(&view), k as i64)).collect();
            let mut both: Result<(), Refusal> = bm.clone().map(|_| ());
            for r in &pms {
                both = both.and(r.clone().map(|_| ()));
            }
            let lps: Vec<LPair> = idx.iter().map(|&q| w.pairs[q].0.clone()).collect();
            let p2s: Vec<StubPair> = pms.iter().zip(&lps).map(|(r, lp)| r.clone().unwrap_or_else(|_| lp.p.clone())).collect();
            let b2 = bm.clone().unwrap_or_else(|_| s.b.clone());
            let tx = renew_tx(&lps, &ks, &p2s, &s, &a, &b2, now, now - s.daa, &signer);
            if agree("renew", &tx, &both) {
                let u = lps.len();
                w.slots[i] = LSlot { op: utxo(&tx.0, u), daa: now, b: b2, ..s };
                for (k, &q) in idx.iter().enumerate() {
                    w.pairs[q] = (LPair { op: utxo(&tx.0, k), daa: now, p: p2s[k].clone(), ..lps[k].clone() }, true);
                }
                w.accepted[1] += 1;
            }
        }
        // Advance.
        4 => {
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            let d = rng.near(max_adv(&s.b, now).max(1));
            let m = s.b.advance(&s.ctx(now, 0), d, s.value as i64);
            let b2 = m.clone().unwrap_or_else(|_| s.b.clone());
            let tx = advance_tx(&s, d, &b2, s.value, now);
            if agree("advance", &tx, &m) {
                if w.forged.contains(&s.op) {
                    w.forged.insert(utxo(&tx.0, 0));
                }
                w.slots[i] = LSlot { op: utxo(&tx.0, 0), daa: now, b: b2, ..s };
                w.accepted[2] += 1;
            }
        }
        // Split.
        5 => {
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            if w.slots.len() > 12 {
                return;
            }
            let mut a = [0i64; RING];
            for (k, b) in s.b.ring.iter().enumerate() {
                a[k] = match rng.below(4) {
                    0 => *b,
                    1 => b / 2,
                    2 => rng.near(*b),
                    _ => 0,
                };
            }
            let va = rng.range(0, s.value as i64) as u64;
            let vb = (rng.near((s.value - va) as i64)).max(0) as u64;
            let mut ctx = s.ctx(now, now - s.daa);
            ctx.cov_out = 2;
            ctx.signed = signed;
            let m = s.b.split(&ctx, &a, va as i64, vb as i64);
            let (ba, bb) = m.clone().unwrap_or_else(|_| (s.b.clone(), s.b.clone()));
            let tx = split_tx(&s, &a, &ba, va, &bb, vb, now, now - s.daa, &signer);
            if agree("split", &tx, &m) {
                w.slots[i] = LSlot { op: utxo(&tx.0, 0), value: va, daa: now, b: ba, ..s.clone() };
                w.slots.push(LSlot { op: utxo(&tx.0, 1), value: vb, daa: now, b: bb, ..s });
                w.accepted[3] += 1;
            }
        }
        // Unbond or withdraw.
        6 => {
            let i = rng.below(w.slots.len() as u64) as usize;
            let s = w.slots[i].clone();
            if s.b.status == STATUS_UNBONDING {
                if w.slots.len() == 1 {
                    return;
                }
                let seq = now - s.daa;
                let c = Ctx { lock: 0, seq: seq as i64, v_in: s.value as i64, cov_in: 1, cov_out: 0, signed, has_id: true };
                let tx = withdraw_tx(&s, seq, &signer, false);
                if agree("withdraw", &tx, &s.b.withdraw(&c)) {
                    w.slots.remove(i);
                    w.accepted[4] += 1;
                }
            } else {
                let d = rng.near(max_adv(&s.b, now));
                let mut ctx = s.ctx(now, now - s.daa);
                ctx.signed = signed;
                let m = s.b.unbond(&ctx, d, s.value as i64);
                let b2 = m.clone().unwrap_or_else(|_| s.b.clone());
                let tx = unbond_tx(&s, d, &b2, now, now - s.daa, &signer);
                if agree("unbond", &tx, &m) {
                    w.slots[i] = LSlot { op: utxo(&tx.0, 0), daa: now, b: b2, ..s };
                    w.accepted[5] += 1;
                }
            }
        }
        // Rely.
        7 => {
            if w.pairs.is_empty() {
                return;
            }
            let q = rng.below(w.pairs.len() as u64) as usize;
            let lp = w.pairs[q].0.clone();
            let tx = rely_tx(&lp);
            if agree("rely", &tx, &lp.p.rely(lp.daa as i64, true)) {
                w.pairs[q].0 = LPair { op: utxo(&tx.0, 0), daa: now, ..lp };
                w.accepted[6] += 1;
            }
        }
        // A genesis slot of a new id with any state (negative, huge or misplaced buckets): no entry checks a genesis.
        9 => {
            if w.slots.len() > 6 {
                return;
            }
            let mut b = Bond::new(xkey(&uw()), market(), rng.range(0, (now / 100) as i64 + 2));
            for k in 0..RING {
                if rng.chance(25) {
                    b.ring[k] = rng.range(-1_000_000_000, 2_000_000_000);
                }
            }
            let op = w.fresh_op();
            let mut id = [0u8; 32];
            id[..4].copy_from_slice(&w.n.to_le_bytes());
            id[31] = 0x5f;
            w.forged.insert(op);
            w.slots.push(LSlot { op, value: rng.range(1, 2_000_000_000) as u64, daa: now, id: Some(Hash::from_bytes(id)), b });
            w.accepted[7] += 1;
        }
        // A genesis pair naming a real slot it was never bound by (uncounted: not in the invariant, its rely bounded).
        10 => {
            let s = &w.slots[rng.below(w.slots.len() as u64) as usize];
            let x = (now as i64 + rng.range(-300, 1_500)) / 100 * 100;
            let pr = StubPair { m: market(), uw_id: s.id_bytes(), uw_key: s.b.owner, owner: xkey(&keypair(33)), cover: rng.range(1, 5_000_000_000), expiry: x };
            let op = w.fresh_op();
            let mut id = [0u8; 32];
            id[..4].copy_from_slice(&w.n.to_le_bytes());
            id[31] = 0x6f;
            w.pairs.push((LPair { op, value: 1_000_000, daa: now, id: Hash::from_bytes(id), p: pr }, false));
            w.accepted[7] += 1;
        }
        // Time passes.
        _ => {
            w.now += rng.range(1, 150) as u64;
            w.accepted[7] += 1;
        }
    }
    w.check();
}

/// The hostile sequences of the model replayed against the contracts: honest and perturbed arguments (one off, one
/// step off, a wrong key, a twin, claims swapped, values short), judged by the model and by the engine on every step.
#[test]
fn holds_the_model_and_the_contracts_agree_on_every_step() {
    let mut total = [0usize; 8];
    let mut steps = 0;
    for seed in 0..6u64 {
        let mut rng = Rng(0xb0d5_0000 + seed);
        let mut w = EW { now: LOCK, slots: vec![slot()], pairs: vec![], n: 0, steps: 0, accepted: [0; 8], forged: Default::default() };
        for _ in 0..500 {
            ew_step(&mut w, &mut rng);
        }
        steps += w.steps;
        for k in 0..8 {
            total[k] += w.accepted[k];
        }
    }
    println!("model = contract on {steps} steps; accepted: open {}, renew {}, advance {}, split {}, withdraw {}, unbond {}, rely {}, time or genesis {}", total[0], total[1], total[2], total[3], total[4], total[5], total[6], total[7]);
    for (k, name) in ["open", "renew", "advance", "split", "unbond"].iter().enumerate() {
        let n = [total[0], total[1], total[2], total[3], total[5]][k];
        assert!(n >= 3, "{name} accepted only {n} times");
    }
}
