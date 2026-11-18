# KPF oracle-only markets: first slice, the underwriting capital (implementation note)

Status: implemented and tested offline; not pinned, not on testnet. Implements part of `DESIGN-ORACLE-2.md` (cited
`O2§n`; `O§n` is `DESIGN-ORACLE.md`, `D§n` is `DESIGN.md`). This slice is the underwriting slot and its accounting only:
no price claim, no judge, no oracle-market pair. Resolutions of the note's open points given for this slice: `ρ` starts
at 1 and is a parameter of one template, refused above 5; the long delay belongs to the judge's stake, not to slot
unbonding; KAS margin (form a) only; the "one underwriter per side" alternative (O2§8) is not built.

## 1. What exists

| Path | What |
|---|---|
| `contracts/KpfBond.sil` | the slot (O2§1.3 `KpfUw`), statuses slot (2) and unbonding (3): `bind`, `advance`, `split`, `unbond`, `withdraw` |
| `contracts/stubs/KpfPairStub.sil` | a stand-in pair: `renew` (counted by its slot's bind), `rely` (what a price claim will require of the cover), `close` |
| `contracts/stubs/KpfOpenStub.sil` | a stand-in opening order: `fill` (T1′: the new pair is bound by the slot, no twin) |
| `contracts/probes/Z1Spk.sil`, `Z11Twin.sil` | experiments Z1 and Z11 |
| `crates/kpf-protocol/src/bond.rs` | the pure model: parameters and bounds, the ring, the acceptance rule of every entry of the three contracts (every `require`, and a failure on any 64-bit overflow), plus the note's merge and reserve |
| `crates/kpf-protocol/tests/bond_model.rs` | Z3 (property fuzz) and Z8 (capacity over a month) on the model |
| `crates/kpf-tests/tests/bondkit/` | fixtures and transaction builders |
| `crates/kpf-tests/tests/kpf_bond_tests.rs` | every path positive and negative, boundaries (21 tests) |
| `crates/kpf-tests/tests/bond_attack_tests.rs` | attacks (12) and the model-against-contract replay (1) |
| `crates/kpf-tests/tests/kpf_bond_metrics_tests.rs`, `z_oracle_experiments_tests.rs` | Z4, Z1, Z11 |

Run: `CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2` (457 tests: the 414 before this slice and 43 new), or
`-p kpf-tests --test kpf_bond_tests` etc.; `--nocapture` prints the measured tables. The contracts are compiled from
source by the tests; they are not in `contracts/artifacts/` yet (pinning changes the manifest, and belongs with the
first transaction builder and command line, next slice). The four existing contracts and the book-market path are
untouched.

## 2. The slot

**State** (236 bytes from redeem byte 1): `ownerKey`, `status` (the bond header, byte for byte `KpfLock`'s), the market
block (`market` id 32 and the bond parameters `rhoBps, stepDaa, termDaa, tailDaa, rateDaa, unbondDaa`, 87 bytes that a
pair and an order carry identically), the ring position `j0`, and the ring: 12 buckets of 8 bytes in one push. Bucket
`i` counts the cover whose pair expiry is `X = (j0 + i + 1) · step`, until its liability ends at `X + tail`. Amounts are
sompi.

**Invariant**, required by every entry that adds cover or divides a slot: every bucket `≥ 0` and
`Σ ring ≤ ⌊min(K, 10¹⁸) · ρ⌋`, `K` the slot's own value.

| Entry | Who | Rule |
|---|---|---|
| `bind(t, x, d, r0, c0 … r6, c6, s)` | owner, SIGHASH_ALL, age `≥ a` (CSV) | `tx.daa ≥ t` (CLTV, `t` a DAA lock time); advance `d` steps (as `advance`); `x` a ring boundary, `x ≤ t + Λ + step`, `x + tail` below the DAA lock-time range, inside the ring; the bucket of `x` (not negative) `+= Σ cᵢ`; claims: used ones first with strictly increasing refs (`i` = input `i`, `65,536 + j` = output `j`), `0 ≤ cᵢ ≤ K`, unused ones `(−1, 0)`; continuation value `≥` input (a top-up raises `K`); invariant |
| `advance(d)` | anyone | `d ≥ 1`, `tx.daa ≥ (j0 + d) · step + tail` (the liability of every dropped bucket has ended); value kept |
| `split(a0 … a11, s)` | owner, age `≥ a` | two outputs of the id: child A ring `a`, child B the rest, every bucket of both `≥ 0`, each child within the invariant on its own value, values together `≥` the slot's |
| `unbond(d, s)` | owner, age `≥ a` | after advancing `d` steps every bucket is 0; status unbonding |
| `withdraw(s)` | owner | unbonding, age `≥ U`, no output of the id; an id-less bond in any status |

Every entry runs `checkBond` (one text in the three contracts): `1 ≤ ρ ≤ 5`, `step > 0`, `Λ` a whole number of steps
below `2³²`, `0 ≤ tail < 2³²`, `Λ + tail + 2 · step ≤ 12 · step`, `0 ≤ a, U < 2³²`. The guards of the reviews: the
single-input-of-the-id guard and exactly one continuation on `bind`, `advance`, `unbond` (two on `split`); no state
field is copied without its bound (`checkBond`) or its sign (`ringSum`, the bucket before an add); every clock is a lower
bound proven by CLTV or a UTXO age (CSV), and the pair's `rely` reads its own UTXO's DAA score; every sompi is either in
the continuation(s) or, at `withdraw`, wherever the owner's SIGHASH_ALL signature sends it.

**How a pair relies on a bind.** The slot never reads a pair (no pin, as O2§1.1 wants). A claimant (an opening order for
the pair it creates, or a renewing pair) reads the slot input's signature script: `boundBy` checks the slot's template
(P2SH, template hash), its status and market block, that the input is spent by `bind` (every argument is a plain 8-byte
push at its fixed offset, the 65-byte signature, the bind dispatch tag, the exact length), and that the claim at index
`k` names the claimant itself (its input index, or `65,536` + its output index) with cover `≥` its own and expiry `x`
equal to its own. `bind` refuses two claims with one ref, so no two claimants count one claim; one bind serves up to 7
claimants (O2§1.3's "one renewal transaction may renew up to 7 pairs").

**Exit.** From every state the template writes, the owner alone gets its KAS out: `advance` needs only the calendar,
`unbond` and `withdraw` only the owner. A state only a genesis can write (an unknown status, a ring position whose
liability ends beyond the DAA range) has no exit; it is its author's own KAS.

## 3. Experiments

| # | Question | Measured | Verdict |
|---|---|---|---|
| Z1 | can a contract prove that input `j` is an ordinary key's input | `tx.inputs[j].scriptPubKey == P2PK(key)` is accepted for that key and refused for another key, a P2SH input and its own input; the raw read returns the 2 version bytes first. A SIGHASH_NONE or ANYONECANPAY signature of that input is valid for consensus, so "the key signed every outpoint and output" needs the reader to check the signature script: one push of 65 bytes ending in 0x01. Cost 577 script units (590 with the type check), no signature operation in the reader | **holds**, with the type-byte check the note does not mention |
| Z11 | can the creating inputs refuse a second output of a new pair id | `OpCovOutputCount(newId)` is 0 in the creating transaction with or without a twin (genesis outputs do not enter the covenant context, X1): "`== 1`" refuses every genesis, "`≤ 1`" admits every twin. A scan `OpOutputCovenantId(j) ≠ newId` for every `j ≠ pairOut` (at most 8 outputs) refuses one or two twins and accepts the honest genesis: 1,921 script units | **the note's mechanism is refuted; the scan holds** and is built into `KpfOpenStub.fill` |
| Z4 (part) | mass of a bind and of a renewal of 1, 4, 7 pairs | table below; every path within the limits by a wide margin | holds for the stand-ins; real pairs estimated below |
| Z3 | ring fuzz: no reachable state counts more than `ρK` or creates sompi | 144 seeded runs of 3,000 steps (hostile underwriter, hostile pair owner, both): 83,420 accepted and 253,186 refused steps, invariants checked after each. It **found a hole** (below), now fixed. Mutations (no capacity check; a claimant accepting any ref) are caught within one run | holds after the fix |
| Z8 | effective capacity over a month against `ρ_eff = ρΛ/(Λ + T + Λ/8)` | table below | holds, with a sharper reading |

**Z4.** Script units per input (bond input, then the funder's P2PK 100,001), mass with each input committing the smallest
covering compute budget, relay fee at 100 sompi per gram. `KpfBond` 4,453 bytes (1 + 236 + 4,216; the note estimated
about 3 kB for `KpfUw`), `KpfPairStub` 1,616, `KpfOpenStub` 1,790. Largest stack 163 of 244.

| Transaction | Inputs | Script units (slot / others) | Compute mass | Storage mass | Bytes |
|---|---|---|---|---|---|
| bind, slot alone, 1 claim / 7 claims | 2 | 169,880 / 170,181 | 8,417 / 8,517 | 611 | 5,087 |
| open (stand-in order + bind) | 3 | 169,928; order 73,810 | 12,848 | 13,890 | 8,448 |
| renewal of 1 pair | 3 | 169,943; pair 66,502 | 11,154 | 13,751 | 6,854 |
| renewal of 4 pairs | 6 | 170,096; 66,502–66,525 each | 19,465 | 41,903 | 12,155 |
| renewal of 7 pairs | 9 | 170,248; 66,502–66,541 each | 27,676 | 60,111 | 17,456 |
| advance / split / unbond (11 steps) / withdraw | 2 | 65,007 / 218,474 / 168,191 / 109,857 | 7,207 / 9,329 / 8,273 / 7,619 | ≤ 12,611 | ≤ 5,129 |

The storage masses come from the stand-ins' small test values (KIP-9). *(est.)* With real pairs (`KpfPair` is 12,370
bytes and its add-margin path 275,246 units, D§4.5g) a pair input of a renewal costs about 12.9 kB and 340,000 units
(its own continuation plus the 66,000 units of `boundBy`); 7 pairs and the slot then come to about 2.7 M script units and
96 kB, a compute mass of about 130,000 by the ratio `kpf_metrics_tests` measures (8 proposals: 5.04 M units, 125,934
bytes, 183,244 mass): within 500,000.

**Z8** (`z8_effective_capacity_over_a_month_matches_the_formula`). Testnet parameters: step 3.5 days, `Λ` 28 days (8
steps), `T` = `W` 6 h + `T_retro` 3 days; 480 pairs of 2,000 KAS opened evenly over one term, renewing forever; measured
hourly over the 30 days after a month. "Effective" = cover in force divided by the smallest `ρK` that serves every
renewal (checked: `K` = that peak serves all, one sompi less refuses a bind).

| Renewal | Average in force / counted | Formula with the mean lead | Effective multiple `ρ_eff/ρ` | Formula with the largest lead |
|---|---|---|---|---|
| at `X − step` (the note's case) | 0.811 | 0.806 | **0.800** | 0.806 (the note's) |
| uniform in the last step | 0.854 | 0.849 (`Λ/(Λ + T + step/2)`) | 0.803 | 0.806 |
| just before `X` | 0.902 | 0.896 (`Λ/(Λ + T)`) | 0.889 | 0.896 |

The note's formula is the effective multiple whenever renewals may come as early as one step before expiry, whatever
their spread: a slot must hold the peak, and the peak is set by the earliest renewal. Later renewal buys capacity only
if every renewal is late.

## 4. Where the note's arithmetic is wrong or ambiguous (each a `// DESIGN:` comment)

1. **The 8-bucket ring cannot hold a renewal** (`bond.rs` `RING`, `KpfBond.checkBond`). Advanced to the present, the
   last of 8 buckets of `Λ/8` expires at most 7 steps ahead, while a renewal in the last step needs `X + Λ` and must keep
   the old bucket until `X + T_tail`. Fixed: 12 buckets and the market bound `Λ + T_tail + 2·step ≤ 12·step`.
2. **The bind window** (`BondParams::horizon`, `KpfBond.bind`). "`X` within the next `Λ`; `X ≥ lockTime + 7Λ/8`" permits a
   renewal only from `t ≥ X`, after the cover expired, contradicting "renewal in the last step". Fixed:
   `x ≤ t + Λ + step`, no lower bound on the slot (the term is the pair's business).
3. **Units.** KAS units with rounding replaced by sompi with `K` clamped at `10¹⁸` (`ρ ≤ 5` keeps `ρK < 2⁶³`).
4. **"Honest-or-higher counters"** (O2§5.2) is false: a genesis can write negative buckets. Before the fix a bind into a
   bucket of `−c` with claims `c` counted 0 while the claimants read `c` (Z3 found it). Fixed: the bucket must be `≥ 0`
   before an add; `split` checks both children.
5. **The invariant after a slash** (`Bond::reserve`). "`ΣO ≤ ρK` on every reachable state" does not survive a
   reservation; what holds is "cover relied on `≤ ρ(K + reserved)`", and binds stop until the cover expires.
6. **Z11** (`KpfOpenStub.fill`): the counter cannot see a twin; an output scan does.
7. **Look-alike pairs** (`KpfPairStub.rely`): bounded on chain, see §5.
8. `withdraw`: `U` may be 0 (the long delay is the judge's stake's); an id-less bond withdraws in any status.

## 5. Attacks (`bond_attack_tests`, and the path tests named)

| Attack | Verdict |
|---|---|
| underwrite more than `ρK` by any sequence (binds, splits, top-ups, advances, twins, forged genesis states, parallel and joint binds, claims swapped or shared) | **holds** (Z3, the replay, `bind_over_underwriting_by_one_sompi`, `holds_a_negative_bucket_written_by_a_genesis_cannot_hide_cover`, `holds_twin_slots_of_one_id_each_count_within_their_own_value`, `two_binds_*`) |
| release cover while the pair relies on it | **holds**: `advance` and `unbond` need `tx.daa ≥ X + tail`; boundary and one DAA either side |
| cover never released, a bond stuck | **holds** for every state the template writes (`holds_a_slot_always_has_an_exit_that_needs_nobody`; an id-less bond; advance griefing at most once per step) |
| a pair claims cover it was never given: renewal without a bind, another pair's claim, a non-bind spend of the slot, a fake bond template, a P2PK twin of the slot, a slot child of another owner, another market | **holds** |
| a twin pair at genesis | **holds** (Z11 scan) |
| **a pair written by a genesis that names a real slot** (O2§5.2) | **exploit, bounded**: uncounted cover the slot's owner could vouch beyond `ρ`, for at most `Λ + step` after the pair's genesis (`rely` requires `X ≤ P + Λ + step`, which every template-written pair satisfies); any later renewal is counted. Entrants' wallets must still check the pair's genesis (`exploit_a_genesis_pair_relies_on_a_slot_it_never_bound_for_at_most_one_period`) |
| value out of a bond other than by its owner's withdraw | **holds** (`holds_value_leaves_a_bond_only_by_its_owners_withdraw`) |
| model and contract disagree | **holds**: 3,000 replayed steps (99 opens, 15 renewals, 137 advances, 31 splits, 65 unbonds, 48 withdraws, forged slots and pairs), every verdict equal |

## 6. Folding `KpfLock` in (not done)

The note (O2§7) wants one template `KpfBond` with statuses lock, slot, reserved, free stake, unbonding. Shaped for it:
the header is `KpfLock`'s (`ownerKey`, `status` at the same offsets), status values are disjoint (1 lock, 2 slot, 3
unbonding), every entry checks its status. The refactor: give the template one state layout, header + a status-tagged
payload (the larger of `KpfLock`'s 426-byte claim block and this slot's 194 bytes); move `dispute` and `release` in,
each requiring status 1; point `KpfPending.guarantee`'s `LOCK_TPL` and its lock-state builder at the merged template;
carry `KpfLock`'s KOB evidence constants. Cost *(est.)*: about 6.5 kB per input on every path (2.3 kB of lock code added to
every bind and renewal) against two templates sharing the bounds and the library; decide on Z4 of the next slice. It
changes `KpfPending` and the pinned artifacts, so it is a slice of its own.

## 7. What the next slices need from this one

- **Oracle-market pair and orders** (`KpfPairO`, `KpfOpen` T1′): the market block gains the bond parameters byte for
  byte (`BondMarket::bytes`) and `KpfOpen.checkMarket` calls the `checkBond` text; `boundBy` and the twin scan move into
  the real orders and pair; `rely`'s bound `X ≤ P + Λ + step` and "`P ≤ X`" go into every claim; `uwId`, `uwKey`,
  `cover`, `X` become pair fields; renewal moves bounds inward to pay the premium.
- **Price claims by the underwriter**: Z1's reader with the SIGHASH_ALL check on the underwriter's P2PK input.
- **Stake judge, reserve, release, pay, records**: the model's `reserve` and Z3's slashed invariant; the `KpfLock` fold.
- **Closed-market rules, dollar-token margin**: not started.
- Pin `KpfBond` and the real pair in `contracts/artifacts/`, builders and command line.
