//! `KpfBond` (docs/DESIGN-ORACLE-IMPL.md) in the script engine: every path positive and negative, with the stand-in
//! pair and order of `contracts/legacy/stubs/`. Boundaries are tested at the boundary and one either side. Every verdict is
//! also the model's (`kpf_protocol::bond`): `agree` fails the test if the engine and the model differ.
//!
//! Fixture market (`bondkit::params`): step 100 DAA, term 800 (8 steps), tail 200, `a` = 10, `U` = 50, rho 1. A slot
//! created at DAA 0 with `j0` = 0; most transactions land at lock time 1,000, where a bind can advance 8 steps
//! (`(0 + 8) * 100 + 200 <= 1,000`) and the window admits expiries up to `1,000 + 900 = 1,900`.

mod bondkit;
mod common;

use bondkit::*;
use common::*;
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::*;
use secp256k1::Keypair;

const SLOT_ID: Hash = Hash::from_bytes([0x51; 32]);
const ORDER_ID: Hash = Hash::from_bytes([0x53; 32]);
const K: u64 = 1_000_000_000; // 10 KAS bonded
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

fn order(cover: i64) -> StubOpen {
    StubOpen { m: market(), uw_accept: [0; 32], owner: xkey(&keypair(33)), cover }
}

/// A slot that has bound `c` at expiry `x` (bind at LOCK with the full advance), as the next UTXO (DAA `daa`).
fn bound(c: i64, x: i64, daa: u64) -> LSlot {
    let s = slot();
    let a = BindArgs::new(LOCK as i64, x, 8, &[Claim::input(0, c)]);
    let b2 = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64).expect("bound");
    LSlot { op: outpoint(101), value: K, daa, id: Some(SLOT_ID), b: b2 }
}

fn pair(c: i64, x: i64, n: u8) -> LPair {
    LPair {
        op: outpoint(110 + n),
        value: 200_000_000,
        daa: LOCK,
        id: Hash::from_bytes([0x60 + n; 32]),
        p: StubPair { m: market(), uw_id: SLOT_ID.as_bytes(), uw_key: xkey(&uw()), owner: xkey(&keypair(33)), cover: c, expiry: x },
    }
}

/// The model's verdict of a bind by the slot alone (continuation value `v_out`).
fn bind_verdict(s: &LSlot, a: &BindArgs, v_out: u64, lock: u64, seq: u64, signed: bool) -> Result<Bond, Refusal> {
    let mut c = s.ctx(lock, seq);
    c.signed = signed;
    s.b.bind(&c, a, v_out as i64)
}

/// Builds and runs a bind by the slot alone; the continuation is the model's (or the slot itself when refused).
fn try_bind(what: &str, s: &LSlot, a: &BindArgs, v_out: u64, lock: u64, seq: u64, signer: &Keypair) -> bool {
    let m = bind_verdict(s, a, v_out, lock, seq, *signer == uw());
    let b2 = m.clone().unwrap_or_else(|_| s.b.clone());
    let tx = bind_alone(s, a, &b2, v_out, lock, seq, signer);
    agree(what, &tx, &m)
}

#[test]
fn layout_state_at_byte_one_and_lengths() {
    let k = kit();
    assert_eq!(k.bond_t.prefix.len(), 1, "KpfBond state starts at byte 1");
    assert_eq!(k.bond_t.state_len, BOND_STATE_LEN);
    assert_eq!(k.pair_t.state_len, 204);
    assert_eq!(k.open_t.state_len, 162);
    let b = slot().b;
    let rs = k.bond_t.redeem(&b.state_bytes());
    println!(
        "KpfBond {} bytes (1 + {} + {}), KpfPairStub {}, KpfOpenStub {}",
        rs.len(),
        k.bond_t.state_len,
        k.bond_t.suffix.len(),
        k.pair_t.prefix.len() + k.pair_t.state_len + k.pair_t.suffix.len(),
        k.open_t.prefix.len() + k.open_t.state_len + k.open_t.suffix.len()
    );
    assert_eq!(rs[34], 0x08, "status push at 34");
    assert_eq!(rs[130], 0x08, "j0 push at 130");
    assert_eq!(&rs[139..141], &[0x4c, 0x60], "the ring's push (OP_PUSHDATA1 96) at 139");
    assert_eq!(rs.len(), 1 + BOND_STATE_LEN + k.bond_t.suffix.len());
    assert_eq!(&rs[43..130], &market().bytes()[..], "market block at [43..130)");
    // The header is KpfLock's: ownerKey then status, at the same offsets.
    let lock_rs = kpf_protocol::artifacts::lock_template();
    assert_eq!(lock_rs.prefix.len(), 1);
}

// ---------------------------------------------------------------- bind

#[test]
fn bind_alone_owner_signature_and_rate_limit() {
    let s = slot();
    let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 5_000)]);
    assert!(try_bind("positive", &s, &a, K, LOCK, 10, &uw()));
    assert!(!try_bind("another key", &s, &a, K, LOCK, 10, &keypair(32)));
    assert!(!try_bind("age a - 1", &s, &a, K, LOCK, 9, &uw()));
    assert!(try_bind("age a", &s, &a, K, LOCK, 10, &uw()));
    // No claim at all: a top-up and advance.
    assert!(try_bind("no claim", &s, &BindArgs::new(1_000, 1_900, 8, &[]), K + 1, LOCK, 10, &uw()));
}

#[test]
fn bind_lock_time_bound_and_window_at_the_boundary() {
    let s = slot();
    let c = [Claim::input(0, 1)];
    assert!(try_bind("t = lock", &s, &BindArgs::new(1_000, 1_900, 8, &c), K, LOCK, 10, &uw()));
    assert!(!try_bind("t = lock + 1", &s, &BindArgs::new(1_001, 1_900, 8, &c), K, LOCK, 10, &uw()));
    assert!(!try_bind("t < 0", &s, &BindArgs::new(-1, 800, 0, &c), K, LOCK, 10, &uw()));
    // The window x <= t + term + step: 1,900 at t = 1,000, so 2,000 is beyond it; at t = 999 so is 1,900.
    assert!(!try_bind("x one step beyond", &s, &BindArgs::new(1_000, 2_000, 8, &c), K, LOCK, 10, &uw()));
    assert!(!try_bind("x at t + 901", &s, &BindArgs::new(999, 1_900, 7, &c), K, LOCK, 10, &uw()));
    assert!(try_bind("x at t + 900", &s, &BindArgs::new(999, 1_800, 7, &c), K, LOCK, 10, &uw()));
    assert!(!try_bind("x not a boundary", &s, &BindArgs::new(1_000, 1_850, 8, &c), K, LOCK, 10, &uw()));
    assert!(!try_bind("x below the ring", &s, &BindArgs::new(1_000, 800, 8, &c), K, LOCK, 10, &uw()));
    assert!(try_bind("x at the ring's first bucket", &s, &BindArgs::new(1_000, 900, 8, &c), K, LOCK, 10, &uw()));
    assert!(!try_bind("x negative", &s, &BindArgs::new(1_000, -100, 8, &c), K, LOCK, 10, &uw()));
}

#[test]
fn bind_advance_proof_at_the_boundary() {
    let s = slot();
    let c = [Claim::input(0, 1)];
    // Dropping 8 buckets needs lock >= (0 + 8) * 100 + 200 = 1,000.
    assert!(try_bind("d = 8 at 1,000", &s, &BindArgs::new(1_000, 1_900, 8, &c), K, 1_000, 10, &uw()));
    assert!(!try_bind("d = 8 at 999", &s, &BindArgs::new(999, 1_800, 8, &c), K, 999, 10, &uw()));
    assert!(!try_bind("d = 9 at 1,000", &s, &BindArgs::new(1_000, 1_900, 9, &c), K, 1_000, 10, &uw()));
    assert!(!try_bind("d < 0", &s, &BindArgs::new(1_000, 1_100, -1, &c), K, 1_000, 10, &uw()));
    // Without the advance the expiry 1,900 is outside the ring (it holds 100 .. 1,200).
    assert!(!try_bind("no advance", &s, &BindArgs::new(1_000, 1_900, 0, &c), K, 1_000, 10, &uw()));
    assert!(try_bind("no advance, x in the ring", &s, &BindArgs::new(1_000, 1_200, 0, &c), K, 1_000, 10, &uw()));
}

#[test]
fn bind_over_underwriting_by_one_sompi() {
    let s = slot();
    let k = K as i64;
    // rho 1: a single claim of the whole value fits, one sompi more does not.
    assert!(try_bind("cover = K", &s, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, k)]), K, LOCK, 10, &uw()));
    assert!(!try_bind("cover = K + 1", &s, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, k + 1)]), K, LOCK, 10, &uw()));
    // Spread over two expiries and two claims: the sum counts.
    let half = bound(k / 2, 1_900, LOCK);
    assert!(try_bind("the rest", &half, &BindArgs::new(1_000, 1_800, 0, &[Claim::input(0, k / 4), Claim::input(1, k / 4)]), K, LOCK, 10, &uw()));
    assert!(!try_bind("the rest + 1", &half, &BindArgs::new(1_000, 1_800, 0, &[Claim::input(0, k / 4), Claim::input(1, k / 4 + 1)]), K, LOCK, 10, &uw()));
    // A top-up in the same transaction raises the capacity by exactly what it adds.
    assert!(try_bind("top-up", &half, &BindArgs::new(1_000, 1_800, 0, &[Claim::input(0, k / 2 + 7)]), K + 7, LOCK, 10, &uw()));
    assert!(!try_bind("top-up - 1", &half, &BindArgs::new(1_000, 1_800, 0, &[Claim::input(0, k / 2 + 7)]), K + 6, LOCK, 10, &uw()));
    // rho 2: two claims of K each fit (each claim at most K), a third sompi does not.
    let m2 = market_with(BondParams { rho_bps: 20_000, ..params() });
    let s2 = slot_of(Bond::new(xkey(&uw()), m2, 0), K);
    assert!(try_bind("rho 2: 2K", &s2, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, k), Claim::input(1, k)]), K, LOCK, 10, &uw()));
    assert!(!try_bind("rho 2: 2K + 1", &s2, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, k), Claim::input(1, k), Claim::input(2, 1)]), K, LOCK, 10, &uw()));
    assert!(!try_bind("rho 2: one claim above K", &s2, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, k + 1)]), K, LOCK, 10, &uw()));
    // rho 1.5 rounds the capacity down: floor(K * 1.5).
    let m15 = market_with(BondParams { rho_bps: 15_000, ..params() });
    let s15 = slot_of(Bond::new(xkey(&uw()), m15, 0), 1_001);
    assert!(try_bind("rho 1.5 of 1,001: 1,501", &s15, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 1_000), Claim::input(1, 501)]), 1_001, LOCK, 10, &uw()));
    assert!(!try_bind("rho 1.5 of 1,001: 1,502", &s15, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 1_000), Claim::input(1, 502)]), 1_001, LOCK, 10, &uw()));
}

#[test]
fn bind_claim_rules() {
    let s = slot();
    let ok_args = |cl: &[Claim]| BindArgs::new(1_000, 1_900, 8, cl);
    assert!(try_bind("seven claims", &s, &ok_args(&[0, 1, 2, 3, 4, 5, OUT_REF + 9].map(|r| Claim { r, c: 10 })), K, LOCK, 10, &uw()));
    assert!(!try_bind("refs decreasing", &s, &ok_args(&[Claim::input(1, 1), Claim::input(0, 1)]), K, LOCK, 10, &uw()));
    assert!(!try_bind("refs equal", &s, &ok_args(&[Claim::input(1, 1), Claim::input(1, 1)]), K, LOCK, 10, &uw()));
    let mut gap = ok_args(&[Claim::input(0, 1)]);
    gap.claims[2] = Claim::input(5, 1);
    assert!(!try_bind("a used claim after an unused one", &s, &gap, K, LOCK, 10, &uw()));
    let mut junk = ok_args(&[]);
    junk.claims[3] = Claim { r: -1, c: 5 };
    assert!(!try_bind("an unused claim with cover", &s, &junk, K, LOCK, 10, &uw()));
    assert!(!try_bind("ref -2", &s, &ok_args(&[Claim { r: -2, c: 0 }]), K, LOCK, 10, &uw()));
    assert!(!try_bind("ref at REF_LIMIT", &s, &ok_args(&[Claim { r: REF_LIMIT, c: 1 }]), K, LOCK, 10, &uw()));
    assert!(try_bind("ref at REF_LIMIT - 1", &s, &ok_args(&[Claim { r: REF_LIMIT - 1, c: 1 }]), K, LOCK, 10, &uw()));
    assert!(!try_bind("negative cover", &s, &ok_args(&[Claim::input(0, -1), Claim::input(1, 2)]), K, LOCK, 10, &uw()));
    assert!(try_bind("zero cover", &s, &ok_args(&[Claim::input(0, 0)]), K, LOCK, 10, &uw()));
}

#[test]
fn bind_continuation_rules() {
    let s = slot();
    let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 5)]);
    let b2 = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64).unwrap();
    assert!(!ok(&bind_alone(&s, &a, &b2, K - 1, LOCK, 10, &uw()).0, &bind_alone(&s, &a, &b2, K - 1, LOCK, 10, &uw()).1), "value taken");
    let mut wrong = b2.clone();
    wrong.ring[10] -= 1;
    let t = bind_alone(&s, &a, &wrong, K, LOCK, 10, &uw());
    assert!(!ok(&t.0, &t.1), "a continuation counting one sompi less");
    let mut moved = b2.clone();
    moved.j0 += 1;
    let t = bind_alone(&s, &a, &moved, K, LOCK, 10, &uw());
    assert!(!ok(&t.0, &t.1), "a continuation at another ring position");
    let mut owner = b2.clone();
    owner.owner = xkey(&keypair(32));
    let t = bind_alone(&s, &a, &owner, K, LOCK, 10, &uw());
    assert!(!ok(&t.0, &t.1), "a continuation with another owner");
    // Two outputs of the id.
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "bind", bind_args(&a, &uw())), funder(200)];
    let outs = vec![bond_out(K, &b2, bind(0, SLOT_ID)), bond_out(1_000_000, &b2, bind(0, SLOT_ID)), change(800_000_000)];
    let t = build_ext(&ins, outs, LOCK, &[10, 0]);
    assert!(!ok(&t.0, &t.1), "a second output of the id");
    // An unbonding slot cannot bind; nor a slot with parameters out of bounds.
    let mut ub = s.clone();
    ub.b.status = STATUS_UNBONDING;
    assert!(!try_bind("unbonding", &ub, &a, K, LOCK, 10, &uw()));
    let bad = slot_of(Bond::new(xkey(&uw()), market_with(BondParams { rho_bps: 50_001, ..params() }), 0), K);
    assert!(!try_bind("rho above 5", &bad, &a, K, LOCK, 10, &uw()));
    let five = slot_of(Bond::new(xkey(&uw()), market_with(BondParams { rho_bps: 50_000, ..params() }), 0), K);
    assert!(try_bind("rho 5", &five, &BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, K as i64), Claim::input(1, K as i64), Claim::input(2, K as i64), Claim::input(3, K as i64), Claim::input(4, K as i64)]), K, LOCK, 10, &uw()));
    let tail = slot_of(Bond::new(xkey(&uw()), market_with(BondParams { tail: 201, ..params() }), 0), K);
    assert!(!try_bind("a ring that cannot hold a renewal", &tail, &a, K, LOCK, 10, &uw()));
}

// ---------------------------------------------------------------- advance (release on expiry)

#[test]
fn advance_releases_cover_only_after_its_liability_ends() {
    // Cover with expiry 1,900 sits in bucket 10 after the bind (j0 = 8); its liability ends at 1,900 + 200 = 2,100.
    let s = bound(K as i64, 1_900, LOCK);
    assert_eq!(s.b.counted_at(1_900), K as i64);
    let try_adv = |what: &str, d: i64, lock: u64| {
        let m = s.b.advance(&s.ctx(lock, 0), d, s.value as i64);
        let b2 = m.clone().unwrap_or_else(|_| s.b.clone());
        agree(what, &advance_tx(&s, d, &b2, s.value, lock), &m)
    };
    // Dropping 11 buckets drops expiry 1,900: needs lock >= (8 + 11) * 100 + 200 = 2,100.
    assert!(!try_adv("one DAA before the end", 11, 2_099));
    assert!(try_adv("at the end", 11, 2_100));
    assert!(try_adv("one DAA after", 11, 2_101));
    // Ten buckets can go a step earlier and keep the cover.
    assert!(try_adv("not the cover's bucket", 10, 2_000));
    let b10 = s.b.advance(&s.ctx(2_000, 0), 10, K as i64).unwrap();
    assert_eq!(b10.counted_at(1_900), K as i64, "still counted");
    assert!(!try_adv("d = 0", 0, 5_000));
    assert!(try_adv("d beyond the ring drops everything", 40, 5_000));
    let all = s.b.advance(&s.ctx(5_000, 0), 40, K as i64).unwrap();
    assert_eq!(all.ring, [0; RING]);
    // Anyone (no signature), but the value stays.
    let m = s.b.advance(&s.ctx(2_100, 0), 11, K as i64).unwrap();
    let t = advance_tx(&s, 11, &m, K - 1, 2_100);
    assert!(!ok(&t.0, &t.1), "an advance cannot take value");
}

#[test]
fn advance_cannot_be_repeated_before_the_next_bucket_ends() {
    // An advance to the present leaves nothing droppable until the next boundary: a griefer can keep restarting the
    // slot's age (which an owner entry needs, CSV a) at most once per step.
    let s = slot();
    let b1 = s.b.advance(&s.ctx(1_000, 0), 8, K as i64).unwrap();
    let s1 = LSlot { op: outpoint(102), daa: 1_000, b: b1.clone(), ..s };
    for lock in [1_000u64, 1_050, 1_099] {
        let m = s1.b.advance(&s1.ctx(lock, 0), 1, K as i64);
        assert!(!agree("too early", &advance_tx(&s1, 1, &b1, K, lock), &m));
    }
    let m = s1.b.advance(&s1.ctx(1_100, 0), 1, K as i64);
    assert!(agree("next end", &advance_tx(&s1, 1, &m.clone().unwrap(), K, 1_100), &m));
}

// ---------------------------------------------------------------- split

#[test]
fn split_divides_value_and_cover() {
    let s = bound(600_000_000, 1_900, LOCK);
    let try_split = |what: &str, a: [i64; RING], va: u64, vb: u64, signer: &Keypair, seq: u64| {
        let mut c = s.ctx(LOCK + 20, seq);
        c.cov_out = 2;
        c.signed = *signer == uw();
        let m = s.b.split(&c, &a, va as i64, vb as i64);
        let (ba, bb) = m.clone().unwrap_or_else(|_| (s.b.clone(), s.b.clone()));
        agree(what, &split_tx(&s, &a, &ba, va, &bb, vb, LOCK + 20, seq, signer), &m)
    };
    let mut a = [0i64; RING];
    a[10] = 400_000_000;
    assert!(try_split("positive", a, 400_000_000, 600_000_000, &uw(), 10));
    assert!(try_split("child B with the rest of the cover and less value", a, 800_000_000, 200_000_000, &uw(), 10));
    assert!(!try_split("child B over capacity by one", a, 800_000_001, 199_999_999, &uw(), 10));
    assert!(!try_split("child A over capacity by one", a, 399_999_999, 600_000_001, &uw(), 10));
    assert!(!try_split("values short by one", a, 400_000_000, 599_999_999, &uw(), 10));
    let mut neg = [0i64; RING];
    neg[10] = 600_000_001;
    assert!(!try_split("child B negative", neg, 700_000_000, 300_000_000, &uw(), 10));
    let mut neg_a = [0i64; RING];
    neg_a[3] = -1;
    assert!(!try_split("child A negative", neg_a, 500_000_000, 500_000_000, &uw(), 10));
    assert!(!try_split("another key", a, 400_000_000, 600_000_000, &keypair(32), 10));
    assert!(!try_split("rate limit", a, 400_000_000, 600_000_000, &uw(), 9));
    // A split into one output of the id is not a split.
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "split", split_args(&a, &uw())), funder(204)];
    let t = build_ext(&ins, vec![bond_out(K, &s.b, bind(0, SLOT_ID)), change(900_000_000)], LOCK, &[10, 0]);
    assert!(!ok(&t.0, &t.1));
}

#[test]
fn split_bonds_serve_several_pairs() {
    // One slot of 10 KAS splits into two children of 5 KAS; each binds its own pairs within its own capacity, and a
    // pair's renewal may be counted by either child (both are the pair's uwId, owned by uwKey).
    let s = slot();
    let mut c = s.ctx(LOCK, 10);
    c.cov_out = 2;
    let (ba, bb) = s.b.split(&c, &[0; RING], 500_000_000, 500_000_000).unwrap();
    assert!(agree("split", &split_tx(&s, &[0; RING], &ba, 500_000_000, &bb, 500_000_000, LOCK, 10, &uw()), &Ok::<_, Refusal>(())));
    let ca = LSlot { op: outpoint(120), value: 500_000_000, daa: LOCK, id: Some(SLOT_ID), b: ba };
    let cb = LSlot { op: outpoint(121), value: 500_000_000, daa: LOCK, id: Some(SLOT_ID), b: bb };
    // Child A opens a pair of 5 KAS (its whole capacity); 1 sompi more is refused by A but fits B.
    let lock = LOCK + 10;
    for (child, cover, want) in [(&ca, 500_000_000i64, true), (&ca, 500_000_001, false), (&cb, 500_000_000, true)] {
        let o = order(cover);
        let a = BindArgs::new(lock as i64, 1_900, 8, &[Claim::output(1, cover)]);
        let m = child.b.bind(&child.ctx(lock, 10), &a, child.value as i64);
        let p = o.pair_for(SLOT_ID.as_bytes(), xkey(&uw()), 1_900);
        let b2 = m.clone().unwrap_or_else(|_| child.b.clone());
        assert_eq!(agree("open by a child", &open_tx(&o, ORDER_ID, 0, child, &a, &b2, &p, lock, 10, 0, 0), &m), want);
    }
    // The pair opened by A renews through B: the renewal is counted by B at the new expiry.
    let pr = pair(500_000_000, 1_900, 0);
    let bfull = LSlot { op: outpoint(122), daa: lock, b: cb.b.bind(&cb.ctx(lock, 10), &BindArgs::new(lock as i64, 1_800, 8, &[Claim::input(0, 1)]), 500_000_000).unwrap(), ..cb.clone() };
    let t = 1_800u64;
    let a = BindArgs::new(t as i64, 2_700, 8, &[Claim::input(0, 500_000_000)]);
    let m = bfull.b.bind(&bfull.ctx(t, 100), &a, bfull.value as i64);
    assert!(m.is_err(), "child B is full at 1,800 (its 1-sompi bind)... ");
    let a = BindArgs::new(t as i64, 2_700, 8, &[Claim::input(0, 499_999_999)]);
    let m = bfull.b.bind(&bfull.ctx(t, 100), &a, bfull.value as i64);
    // The pair needs its whole cover: 499,999,999 is one short.
    let view = SlotView { bond: &bfull.b, id: SLOT_ID.as_bytes(), args: &a, canonical_bind: true };
    let pm = pr.p.renew(true, 0, Some(&view), 0);
    assert!(m.is_ok() && pm.is_err());
    let tx = renew_tx(&[pr.clone()], &[0], &[pr.p.clone()], &bfull, &a, &m.clone().unwrap(), t, 100, &uw());
    assert!(!agree("renewal one sompi short", &tx, &pm));
    // Through the empty child B it fits.
    let a = BindArgs::new(t as i64, 2_700, 16, &[Claim::input(0, 500_000_000)]);
    let m = cb.b.bind(&cb.ctx(t, 100), &a, cb.value as i64);
    let view = SlotView { bond: &cb.b, id: SLOT_ID.as_bytes(), args: &a, canonical_bind: true };
    let pm = pr.p.renew(true, 0, Some(&view), 0);
    let tx = renew_tx(&[pr.clone()], &[0], &[pm.clone().unwrap()], &cb, &a, &m.unwrap(), t, 100, &uw());
    assert!(agree("renewal through child B", &tx, &pm));
}

// ---------------------------------------------------------------- unbond and withdraw

#[test]
fn unbond_is_refused_while_cover_is_outstanding_and_accepted_after() {
    let s = bound(5, 1_900, LOCK);
    let try_unbond = |what: &str, d: i64, lock: u64| {
        let m = s.b.unbond(&s.ctx(lock, 10), d, s.value as i64);
        let b2 = m.clone().unwrap_or_else(|_| s.b.clone());
        agree(what, &unbond_tx(&s, d, &b2, lock, 10, &uw()), &m)
    };
    assert!(!try_unbond("outstanding, no advance", 0, 1_500));
    assert!(!try_unbond("outstanding, advanced as far as possible", 10, 2_000));
    assert!(!try_unbond("one DAA before the liability ends", 11, 2_099));
    assert!(try_unbond("at the end of the liability", 11, 2_100));
    assert!(try_unbond("after", 12, 2_200));
    // An empty slot unbonds at once.
    let e = slot();
    let m = e.b.unbond(&e.ctx(0, 10), 0, K as i64);
    assert!(agree("empty", &unbond_tx(&e, 0, &m.clone().unwrap(), 0, 10, &uw()), &m));
    assert_eq!(m.unwrap().status, STATUS_UNBONDING);
    let m = e.b.unbond(&e.ctx(0, 10), 0, K as i64);
    let mut c = e.ctx(0, 10);
    c.signed = false;
    assert!(!agree("another key", &unbond_tx(&e, 0, &m.unwrap(), 0, 10, &keypair(32)), &e.b.unbond(&c, 0, K as i64)));
}

#[test]
fn withdraw_after_the_unbonding_delay() {
    let mut ub = slot();
    ub.b.status = STATUS_UNBONDING;
    let try_w = |what: &str, s: &LSlot, seq: u64, signer: &Keypair, keep: bool| {
        let c = Ctx { lock: 0, seq: seq as i64, v_in: s.value as i64, cov_in: 1, cov_out: keep as usize, signed: *signer == uw(), has_id: s.id.is_some() };
        agree(what, &withdraw_tx(s, seq, signer, keep), &s.b.withdraw(&c))
    };
    assert!(!try_w("U - 1", &ub, 49, &uw(), false));
    assert!(try_w("U", &ub, 50, &uw(), false));
    assert!(!try_w("another key", &ub, 50, &keypair(32), false));
    assert!(!try_w("an output of the id remains", &ub, 50, &uw(), true));
    assert!(!try_w("an active slot", &slot(), 1_000, &uw(), false));
    // An id-less bond (funded without a covenant binding) is withdrawable in any status: nobody can name it.
    let idless = LSlot { id: None, ..bound(5, 1_900, LOCK) };
    assert!(try_w("id-less active slot", &idless, 0, &uw(), false));
    assert!(!try_w("id-less, another key", &idless, 0, &keypair(32), false));
}

// ---------------------------------------------------------------- the pair: renewal and rely

/// A renewal of one pair by `s` at lock `t`, with the claim given; returns the agreed verdict.
fn try_renew(what: &str, pr: &LPair, s: &LSlot, a: &BindArgs, k: i64, t: u64) -> bool {
    let bm = s.b.bind(&s.ctx(t, 100), a, s.value as i64);
    let view = SlotView { bond: &s.b, id: s.id_bytes(), args: a, canonical_bind: true };
    let pm = pr.p.renew(true, 0, Some(&view), k);
    let both: Result<(), Refusal> = bm.clone().and(pm.clone()).map(|_| ());
    let b2 = bm.unwrap_or_else(|_| s.b.clone());
    let p2 = pm.unwrap_or_else(|_| pr.p.clone());
    agree(what, &renew_tx(&[pr.clone()], &[k], &[p2], s, a, &b2, t, 100, &uw()), &both)
}

#[test]
fn renewal_at_the_boundary_and_its_refusals() {
    let s = bound(5, 1_900, LOCK);
    let pr = pair(5, 1_900, 0);
    // Renewal to 2,700 needs t >= 2,700 - 900 = 1,800 (the last step before expiry); the slot advances 8 more at
    // t = 1,800? (8 + 8) * 100 + 200 = 1,800: yes.
    assert!(try_renew("earliest", &pr, &s, &BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, 5)]), 0, 1_800));
    assert!(!try_renew("one DAA earlier", &pr, &s, &BindArgs::new(1_799, 2_700, 7, &[Claim::input(0, 5)]), 0, 1_799));
    assert!(try_renew("one DAA later", &pr, &s, &BindArgs::new(1_801, 2_700, 8, &[Claim::input(0, 5)]), 0, 1_801));
    assert!(try_renew("after the expiry", &pr, &s, &BindArgs::new(2_000, 2_700, 9, &[Claim::input(0, 5)]), 0, 2_000));
    let a = |c: i64| BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, c)]);
    assert!(!try_renew("cover one sompi short", &pr, &s, &a(4), 0, 1_800));
    assert!(try_renew("more cover than needed", &pr, &s, &a(6), 0, 1_800));
    assert!(!try_renew("a claim naming another input", &pr, &s, &BindArgs::new(1_800, 2_700, 8, &[Claim::input(1, 5)]), 0, 1_800));
    assert!(!try_renew("a claim naming an output", &pr, &s, &BindArgs::new(1_800, 2_700, 8, &[Claim::output(0, 5)]), 0, 1_800));
    assert!(!try_renew("another expiry", &pr, &s, &BindArgs::new(1_800, 2_600, 8, &[Claim::input(0, 5)]), 0, 1_800));
    assert!(!try_renew("the wrong claim index", &pr, &s, &BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, 5)]), 1, 1_800));
    assert!(!try_renew("claim index 7", &pr, &s, &BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, 5)]), 7, 1_800));
    // A slot of another id, another owner, another market.
    let other_id = LSlot { id: Some(Hash::from_bytes([0x5a; 32])), ..s.clone() };
    assert!(!try_renew("another slot id", &pr, &other_id, &a(5), 0, 1_800));
    let mut other_key = s.clone();
    other_key.b.owner = xkey(&keypair(32));
    let mut pk = pr.clone();
    pk.p.uw_key = xkey(&uw());
    let bm = other_key.b.bind(&other_key.ctx(1_800, 100), &a(5), K as i64);
    let tx = renew_tx(&[pk.clone()], &[0], &[StubPair { expiry: 2_700, ..pk.p.clone() }], &other_key, &a(5), &bm.unwrap_or_else(|_| other_key.b.clone()), 1_800, 100, &keypair(32));
    assert!(!ok(&tx.0, &tx.1), "a child of the id owned by another key (a genesis twin) cannot renew the pair");
    let mut other_m = s.clone();
    other_m.b.m.id = [0x4e; 32];
    assert!(!try_renew("a slot of another market", &pr, &other_m, &a(5), 0, 1_800));
}

#[test]
fn renewal_needs_the_bind_entry_not_another_spend_of_the_slot() {
    // The slot advanced (not bound) in the renewal transaction: the pair refuses, whatever the advance's pushes say.
    let s = bound(5, 1_900, LOCK);
    let pr = pair(5, 1_900, 0);
    let b2 = s.b.advance(&s.ctx(1_800, 0), 8, K as i64).unwrap();
    let ins = [pair_in(pr.op, pr.value, Some(pr.id), pr.daa, &pr.p, "renew", vec![iv(1), iv(0)]), bond_in(s.op, s.value, s.id, s.daa, &s.b, "advance", vec![b8(8)]), funder(201)];
    let outs = vec![pair_out(pr.value, &StubPair { expiry: 2_700, ..pr.p.clone() }, bind(0, pr.id)), bond_out(K, &b2, bind(1, SLOT_ID)), change(900_000_000)];
    let (tx, e) = build_ext(&ins, outs, 1_800, &[0, 0, 0]);
    assert_eq!(passes(&tx, &e), vec![false, true, true]);
}

#[test]
fn rely_requires_live_cover_and_a_near_expiry() {
    let base = pair(5, 1_900, 0);
    for (daa, want) in [(1_900u64, true), (1_901, false), (1_000, true), (999, false)] {
        let p = LPair { daa, ..base.clone() };
        let m = p.p.rely(daa as i64, true);
        assert_eq!(agree("rely", &rely_tx(&p), &m), want, "P = {daa}");
    }
}

// ---------------------------------------------------------------- open (T1' stand-in)

/// An open with cover `cover` at expiry `x`, claim `cl` and the given variations; returns the agreed verdict.
#[allow(clippy::too_many_arguments)]
fn try_open(what: &str, o: &StubOpen, cl: Claim, x: i64, k: i64, twins: usize, extra: usize, pair_x: i64) -> bool {
    let s = slot();
    let a = BindArgs::new(LOCK as i64, x, 8, &[cl]);
    let bm = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64);
    let view = SlotView { bond: &s.b, id: SLOT_ID.as_bytes(), args: &a, canonical_bind: true };
    let fm = o.fill(true, 1, Some(&view), k, pair_x, 3 + twins + extra, twins);
    let both: Result<(), Refusal> = bm.clone().and(fm.clone()).map(|_| ());
    let p = fm.unwrap_or_else(|_| o.pair_for(SLOT_ID.as_bytes(), xkey(&uw()), pair_x));
    agree(what, &open_tx(o, ORDER_ID, k, &s, &a, &bm.unwrap_or_else(|_| s.b.clone()), &p, LOCK, 10, twins, extra), &both)
}

#[test]
fn open_binds_the_pair_and_refuses_twins_and_look_alikes() {
    let o = order(300_000_000);
    let cl = Claim::output(1, 300_000_000);
    assert!(try_open("positive", &o, cl, 1_900, 0, 0, 0, 1_900));
    assert!(try_open("8 outputs", &o, cl, 1_900, 0, 0, 5, 1_900));
    assert!(!try_open("9 outputs", &o, cl, 1_900, 0, 0, 6, 1_900));
    assert!(!try_open("a twin of the pair (Z11)", &o, cl, 1_900, 0, 1, 0, 1_900));
    assert!(!try_open("two twins", &o, cl, 1_900, 0, 2, 0, 1_900));
    assert!(!try_open("a claim for another output", &o, Claim::output(2, 300_000_000), 1_900, 0, 0, 0, 1_900));
    assert!(!try_open("a claim for an input", &o, Claim::input(1, 300_000_000), 1_900, 0, 0, 0, 1_900));
    assert!(!try_open("a claim one sompi short", &o, Claim::output(1, 299_999_999), 1_900, 0, 0, 0, 1_900));
    assert!(!try_open("the pair names another expiry", &o, cl, 1_900, 0, 0, 0, 1_800));
    assert!(!try_open("the wrong claim index", &o, cl, 1_900, 1, 0, 0, 1_900));
    let mut only = o.clone();
    only.uw_accept = [0x5a; 32];
    assert!(!try_open("a slot the order does not accept", &only, cl, 1_900, 0, 0, 0, 1_900));
    only.uw_accept = SLOT_ID.as_bytes();
    assert!(try_open("the slot the order names", &only, cl, 1_900, 0, 0, 0, 1_900));
}

#[test]
fn open_refuses_a_pair_output_that_continues_a_lineage() {
    // The pair output carries an id an input already carries (a funder holding a stray of some lineage continues it):
    // not a genesis, refused (OpCovInputCount(pairId) == 0).
    let o = order(300_000_000);
    let s = slot();
    let a = BindArgs::new(LOCK as i64, 1_900, 8, &[Claim::output(1, 300_000_000)]);
    let b2 = s.b.bind(&s.ctx(LOCK, 10), &a, K as i64).unwrap();
    let p = o.pair_for(SLOT_ID.as_bytes(), xkey(&uw()), 1_900);
    let stray = Hash::from_bytes([0x77; 32]);
    let ins = [
        open_in(outpoint(150), 200_000_000, Some(ORDER_ID), 0, &o, 1, 1, 0, 1_900),
        bond_in(s.op, s.value, s.id, s.daa, &s.b, "bind", bind_args(&a, &uw())),
        p2pk_in(outpoint(202), 1_000_000_000, Some(stray), &keypair(60)),
    ];
    let outs = vec![bond_out(K, &b2, bind(1, SLOT_ID)), pair_out(200_000_000, &p, bind(2, stray)), change(800_000_000)];
    let (tx, e) = build_ext(&ins, outs, LOCK, &[0, 10, 0]);
    assert_eq!(passes(&tx, &e), vec![false, true, true]);
}

// ---------------------------------------------------------------- parallel and joint binds

#[test]
fn two_binds_of_one_slot_in_parallel_transactions_conflict() {
    let s = slot();
    let a1 = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 700_000_000)]);
    let a2 = BindArgs::new(1_000, 1_800, 8, &[Claim::input(0, 700_000_000)]);
    let b1 = s.b.bind(&s.ctx(LOCK, 10), &a1, K as i64).unwrap();
    let b2 = s.b.bind(&s.ctx(LOCK, 10), &a2, K as i64).unwrap();
    let t1 = bind_alone(&s, &a1, &b1, K, LOCK, 10, &uw());
    let t2 = bind_alone(&s, &a2, &b2, K, LOCK, 10, &uw());
    assert!(ok(&t1.0, &t1.1) && ok(&t2.0, &t2.1), "each is valid alone (together they would count 1.4 K)");
    let mut set = Utxos::default();
    set.add(s.op, t1.1[0].clone());
    set.add(outpoint(200), t1.1[1].clone());
    assert!(set.apply(&t1.0, LOCK).is_ok());
    assert!(set.apply(&t2.0, LOCK).is_err(), "the slot is spent: the second bind cannot land");
}

#[test]
fn two_binds_in_one_transaction() {
    // Two children of one id, each binding in one transaction: refused by the single-input guard.
    let s = slot();
    let mut c = s.ctx(LOCK, 10);
    c.cov_out = 2;
    let (ba, bb) = s.b.split(&c, &[0; RING], 500_000_000, 500_000_000).unwrap();
    let a = BindArgs::new(1_000, 1_900, 8, &[Claim::input(0, 500_000_000)]);
    let ca = ba.bind(&Ctx { v_in: 500_000_000, ..s.ctx(LOCK, 10) }, &a, 500_000_000).unwrap();
    let cb = bb.bind(&Ctx { v_in: 500_000_000, ..s.ctx(LOCK, 10) }, &a, 500_000_000).unwrap();
    let ins = [
        bond_in(outpoint(120), 500_000_000, Some(SLOT_ID), 0, &ba, "bind", bind_args(&a, &uw())),
        bond_in(outpoint(121), 500_000_000, Some(SLOT_ID), 0, &bb, "bind", bind_args(&a, &uw())),
        funder(200),
    ];
    let outs = vec![bond_out(500_000_000, &ca, bind(0, SLOT_ID)), bond_out(500_000_000, &cb, bind(1, SLOT_ID)), change(900_000_000)];
    let (tx, e) = build_ext(&ins, outs, LOCK, &[10, 10, 0]);
    assert_eq!(passes(&tx, &e), vec![false, false, true]);

    // One bind with two claims renews two pairs at once: the bucket grows by the sum.
    let s = bound(10, 1_900, LOCK);
    let (p0, p1) = (pair(3, 1_900, 0), pair(7, 1_900, 1));
    let a = BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, 3), Claim::input(1, 7)]);
    let b2 = s.b.bind(&s.ctx(1_800, 100), &a, K as i64).unwrap();
    assert_eq!(b2.counted_at(2_700), 10);
    let renewed = [StubPair { expiry: 2_700, ..p0.p.clone() }, StubPair { expiry: 2_700, ..p1.p.clone() }];
    let t = renew_tx(&[p0.clone(), p1.clone()], &[0, 1], &renewed, &s, &a, &b2, 1_800, 100, &uw());
    assert!(ok(&t.0, &t.1), "two pairs, two claims");
    // Two pairs naming the same claim: the second names its own input index, which is not the claim's ref.
    let a1 = BindArgs::new(1_800, 2_700, 8, &[Claim::input(0, 10)]);
    let b3 = s.b.bind(&s.ctx(1_800, 100), &a1, K as i64).unwrap();
    let t = renew_tx(&[p0.clone(), p1.clone()], &[0, 0], &renewed, &s, &a1, &b3, 1_800, 100, &uw());
    assert_eq!(passes(&t.0, &t.1), vec![true, false, true, true], "the second pair cannot use the first's claim");
    // Seven pairs at once (the scan's maximum).
    let ps: Vec<LPair> = (0..7).map(|n| pair(1, 1_900, n)).collect();
    let cl: Vec<Claim> = (0..7).map(|i| Claim::input(i, 1)).collect();
    let a7 = BindArgs::new(1_800, 2_700, 8, &cl);
    let b7 = s.b.bind(&s.ctx(1_800, 100), &a7, K as i64).unwrap();
    let r7: Vec<StubPair> = ps.iter().map(|p| StubPair { expiry: 2_700, ..p.p.clone() }).collect();
    let ks: Vec<i64> = (0..7).collect();
    let t = renew_tx(&ps, &ks, &r7, &s, &a7, &b7, 1_800, 100, &uw());
    assert!(ok(&t.0, &t.1), "seven renewals in one bind");
}

// ---------------------------------------------------------------- boundary values and overflow

#[test]
fn boundary_values_and_overflow() {
    // A slot above the clamp counts at most rho x 10^18 (here rho 5: 5 x 10^18, below 2^63).
    let m5 = market_with(BondParams { rho_bps: 50_000, ..params() });
    let v = 2_000_000_000_000_000_000u64;
    let big = slot_of(Bond::new(xkey(&uw()), m5, 0), v);
    let cap = capacity(v as i64, 50_000);
    assert_eq!(cap, 5 * VALUE_CLAMP);
    let q = cap / 5;
    let five: Vec<Claim> = (0..5).map(|i| Claim::input(i, q)).collect();
    assert!(try_bind("the clamped capacity", &big, &BindArgs::new(1_000, 1_900, 8, &five), v, LOCK, 10, &uw()));
    let mut six = five.clone();
    six.push(Claim::input(5, 1));
    assert!(!try_bind("one sompi above it", &big, &BindArgs::new(1_000, 1_900, 8, &six), v, LOCK, 10, &uw()));
    // Claims whose sum overflows 63 bits: refused (the script fails), never wrapped.
    let huge: Vec<Claim> = (0..7).map(|i| Claim::input(i, 1_900_000_000_000_000_000)).collect();
    assert!(!try_bind("overflowing claims", &big, &BindArgs::new(1_000, 1_900, 8, &huge), v, LOCK, 10, &uw()));
    // A slot with a ring position near the end of the integers: every advance overflows and is refused.
    let far = slot_of(Bond::new(xkey(&uw()), market(), i64::MAX / 100), K);
    let m = far.b.advance(&far.ctx(1_000, 0), 1, K as i64);
    assert!(!agree("advance overflows", &advance_tx(&far, 1, &far.b, K, 1_000), &m));
    // An expiry whose liability end reaches the DAA lock-time range is refused: no advance could release it.
    let thr = 500_000_000_000i64;
    let late = slot_of(Bond::new(xkey(&uw()), market(), (thr - 1_000) / 100 - 3), K);
    let t = (thr - 1_000) as u64;
    let x_ok = thr - 300; // x + 200 < threshold
    assert!(try_bind("liability end below the threshold", &late, &BindArgs::new(t as i64, x_ok, 0, &[Claim::input(0, 1)]), K, t, 10, &uw()));
    let x_bad = thr - 200; // x + 200 = threshold
    assert!(!try_bind("liability end at the threshold", &late, &BindArgs::new(t as i64, x_bad, 0, &[Claim::input(0, 1)]), K, t, 10, &uw()));
    // t at the threshold is not a DAA lock time.
    assert!(!try_bind("t at the threshold", &late, &BindArgs::new(thr, x_ok, 0, &[Claim::input(0, 1)]), K, t, 10, &uw()));
}
