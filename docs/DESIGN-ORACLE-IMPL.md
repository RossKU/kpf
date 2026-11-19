# KPF oracle-only markets: the underwriting capital, the stake judge, the integration (implementation note)

Sections 1-7 are the first slice (the underwriting slot `KpfBond`); sections 8-14 the second (the stake judge
`KpfStake`); sections 15-23 the third (the oracle-market order, pair and pending part wired to both: `KpfOpenO`,
`KpfPairO`, `KpfPendingO`, pinned, with builders and the command line); section 24 the changes after review 6 (fresh
covenant ids for every entered part and every claim, the calendar lapse clock, the judge's upper bounds), which
supersede the earlier sections where they differ.

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

# Second slice: the stake judge

Status: implemented and tested offline; not pinned, not on testnet. Implements the judge of O§2 with the changes of
O2§1.7, standalone: a disputed claim is a stand-in covenant; no price claim, no counter quote `p′`, no reservation of
an underwriter's stake, no wiring into `KpfPending`. Resolutions given for this slice: participation is permissionless
(any aged judge stake, no member list, key or token); the long delay applies to judge stake, both the age a stake needs
before it counts (`M_age`) and the delay before it leaves (`U`), both market parameters; windows are lower bounds on UTXO
DAA scores and ages; `μ` is a market parameter, default 1.25; no UTXO is shared between disputes; a dispute nobody
answers resolves for the side that staked.

## 8. What exists

| Path | What |
|---|---|
| `contracts/KpfStake.sil` | the judge, one template, statuses free stake (4), unbonding (5), receipt (6), dispute (7), resolved (8): `open`, `commit`, `unbond`, `withdraw`, `deposit`, `resolve`, `settle`, `close`, `redeem` |
| `contracts/stubs/KpfClaimStub.sil` | a stand-in claim: `contest` (hands itself to the judge), `decide` (receives the verdict), `close` |
| `crates/kpf-protocol/src/judge.rs` | the pure model: parameters and bounds, the state codec, whole transactions (`JTx`) and the acceptance rule of every input of both contracts (every `require`, a failure on any 64-bit overflow), the transaction shapes (`Arena`), a UTXO world |
| `crates/kpf-protocol/tests/judge_model.rs` | property games, adversarial strategies, Z10 (9 tests) |
| `crates/kpf-tests/tests/judgekit/` | the compiled templates, the real hash environment, `JTx` to engine transaction |
| `crates/kpf-tests/tests/kpf_judge_tests.rs` | every path positive and negative, boundaries, payouts to the sompi (10) |
| `crates/kpf-tests/tests/judge_attack_tests.rs` | attacks (12), the shared bounds text and offsets (2), the model-against-contract replay (1) |
| `crates/kpf-tests/tests/kpf_judge_metrics_tests.rs`, `z_judge_experiments_tests.rs` | the numbers; Z13 |

Run: `CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2` (493 tests: the 457 before this slice and 36 new), or
`-p kpf-protocol --test judge_model`, `-p kpf-tests --test kpf_judge_tests` etc.; `--nocapture` prints the tables. Every
engine test builds its transactions from the model's `JTx` and requires the model's and the engine's verdicts to agree
input by input (`judgekit::agree`), so every engine test is also a replay. `KpfBond`, `KpfPair`, `KpfOpen`,
`KpfPending`, `KpfLock` are unchanged; the test harness gained a signature of any sighash type.

## 9. The judge

**Why a companion template, not a status of `KpfBond`.** `KpfStake` is 9,688 bytes; `KpfBond`'s bind is close to the
stack limit and every bind and renewal input would carry the judge's code (about 9 kB more per input, three times its
size), while the judge needs none of the ring. The two share the bond header (`ownerKey` 32, `status` 8 at the same
offsets; status values disjoint: `KpfLock` 1, `KpfBond` 2-3, `KpfStake` 4-8), so the fold of O2§7 stays possible. One
template for stake, receipts and dispute continues itself, so it needs no template hash of its own: inputs of the same
template are recognised by their P2SH over this template's own prefix and suffix around their state.

**State** (270 bytes from redeem byte 1, one layout, unused fields 0): `ownerKey`, `status`, the judge block (market id
32, `μBps, M_age, U, D_r, φBps, minStake`: 87 bytes that a claim carries identically), `spkHash` (blake3 of the
contested claim's script public key), `side`, `amt`, `round`, `yes`, `no`, `yesU`, `noU`, `anchor`, `start`, `wLeft`,
`lLeft`, `base`. Amounts are sompi; weights are whole 10-KAS units (`⌊amt/10⁹⌋`).

**Bounds** (`checkJudge`, one text in both contracts, enforced by `holds_the_judge_bounds_are_one_text_in_the_stake_and_the_claim`):
`1.1 ≤ μ ≤ 1.5`; `0 < D_r < 2³²`; `D_r ≤ M_age < 2³²`; `D_r ≤ U < 2³²`; `0 ≤ φ ≤ 50 %`; `10 KAS ≤ minStake ≤ 10⁶ KAS`.
Testnet defaults (10 DAA/s): `μ` 1.25, `M_age` = `U` = 7 days, `D_r` 12 h, `φ` 10 %, 100 KAS.

| Entry | Status | Who | Rule |
|---|---|---|---|
| `open(c, cOut, dOut, s)` | free | owner, SIGHASH_ALL | input `c` is the claim (id `C ≠ 0`, the only input of `C`, accepted: its DAA score is the **anchor**); this stake `≥ minStake`, accepted at least `M_age` before the anchor; exactly three outputs of `C`: the claim's continuation `cOut`, this stake's receipt at its own index (side NO, round 0, the anchor, value `≥` the stake), the dispute `dOut` (leader NO, `no` = the stake, `start = -1`, `spkHash` = blake3 of output `cOut`'s script) |
| `commit(side, r, a, s)` | free | owner, SIGHASH_ALL or SINGLE\|ANYONECANPAY | the receipt at its own index: this owner, `side`, round `r`, anchor `a`, the stake's value, a covenant id |
| `unbond(s)` / `withdraw(s)` | free / unbonding | owner | unbonding continuation at its own index / CSV `≥ U`, SIGHASH_ALL decides the outputs |
| `deposit(side, first, k)` | dispute | anyone | the only input of its id; `side` is the trailing side; `1 ≤ k ≤ 7` stakes at `[first, first + k)`, each a free stake of this judge `≥ minStake` accepted `M_age` before the anchor, its receipt at its own index; exactly `1 + k` outputs of the id. If the side's total reaches `⌈μ × the leader's⌉` the lead changes, `round + 1`, `start = -1` (the continuation's own DAA score starts the round); otherwise the round's start is kept, read exactly from the UTXO that started it |
| `resolve(c)` | dispute | anyone | `tx.daa ≥ start + D_r` (CLTV); input `c` has the dispute's id and the recorded script hash; exactly two inputs and two outputs of the id; the resolved pool: winner = leader, `wLeft` = its weight, `lLeft` = the loser's total, `base` = the pool's own value |
| `settle(first, k)` | resolved | anyone | `1 ≤ k ≤ 7` receipts of this id and anchor; a winner becomes a free stake of its owner at its own index worth its value `+ ⌊u·Lnet/W_u⌋` (`Lnet = L − ⌊φL⌋`); a loser's value stays in the pool; `wLeft`, `lLeft` count down; the pool pays only what it holds |
| `close(j)` | resolved | anyone | every receipt settled; the pool's value above `base` to the burn output `j` (not created below 0.02 KAS); `base` is the executor's |
| `redeem(d)` | receipt | anyone | input `d` is its resolved pool (same id, same anchor) |
| claim `contest(u, dOut)` | pending | anyone | input `u` is a `KpfStake` (template hash, P2SH, **canonical state**) free stake of the claim's judge block spent by `open(self, self, dOut)` (dispatch tag and argument pushes read at fixed offsets); three outputs of its id; continuation contested |
| claim `decide(d)` | contested | anyone | input `d` is its id's dispute of its judge block spent by `resolve(self)`; continuation upheld (leader YES) or refuted (NO) |

**Who is paid what.** Winners: their receipts back plus the losers' stake less `φ`, pro rata by weight; the rounding
(less than one sompi per winning receipt) is burned with `φ`. Losers: nothing. Executors: the pool's base at close.
Every sompi of every branch is in a continuation, a receipt, a payout, the burn or, explicitly, the executor's.

**Exits.** A dispute resolves by anyone once its window closes (its contested claim decides by anyone in the same
transaction, and has no other exit); a pool settles every receipt and closes by anyone; a free or unbonding stake needs
its owner only. A state only a genesis can write (an id-less dispute or receipt, a forged tally) is its author's KAS
(`holds_no_dispute_or_stake_is_left_unspendable`, `exploit_a_dispute_written_by_a_genesis_traps_the_stakers_who_join_it`).

## 10. Experiments

| # | Question | Measured | Verdict |
|---|---|---|---|
| Z13 (new) | does a SIGHASH_SINGLE\|ANYONECANPAY signature (0x84) fix its own output's covenant binding | other outputs and the lock time are free; its own output's value, script and covenant binding (removed, or moved to another existing id) are committed | **holds**: a staker's commitment names its dispute through the binding, so stakers sign once and aggregators chain deposits without re-signing |
| Z10 | `f*` against O§2's `p_h/(p_h + μ)` | table below (model, `judge_model`) | **the formula holds for one dispute to 10⁻⁴**; a patient attacker across parallel disputes does better (below) |

**Z10, one dispute** (`z10_the_measured_threshold_matches_the_notes_formula`: 2·10⁶ units of aged stake, the attacker's
in one UTXO, honest stake in pieces of 1,000 units of which honest deploys only an answer that reaches the lead;
bisection on `f` to 10⁻⁴; "patient": the attacker opens with the minimum and goes all in on its answer):

| `μ` | `p_h` = 1: formula / measured | 0.5 | 0.25 |
|---|---|---|---|
| 1.1 | 0.4762 / 0.4762 | 0.3125 / 0.3126 | 0.1852 / 0.1852 |
| 1.25 | 0.4444 / 0.4445 | 0.2857 / 0.2858 | 0.1667 / 0.1667 |
| 1.5 | 0.4000 / 0.4000 | 0.2500 / 0.2501 | 0.1429 / 0.1429 |

Patient and all-in thresholds are equal: holding back never helps the attacker, since the responder always needs `μ`
times the other side's total and the opener can open with everything.

**Z10, the drill of O2§7** (`z10_drill_100_parallel_disputes_half_participation_patient_attacker`: 100 parallel
disputes, `p_h` = 0.5, `μ` = 1.25; the attacker opens every dispute, then escalates pass after pass, cheapest first;
honest answers every escalation while its free aged reserve lasts, since committed stake is a receipt and won stake comes
back fresh). The attacker wins one dispute from `f` = 0.29 (formula 0.2857, on a 1 % grid), 19 at 0.30, 72 at 0.35,
**all 100 from 0.39**, against `μ p_h/(1 + μ p_h)` = 0.385: in each dispute it ends `μ` times honest's last answer,
so it takes every dispute once its stake reaches `μ` times honest's whole reserve. A coordinated honest side that
abandons disputes it cannot hold to the end keeps most of them (the attacker spread evenly takes all only at 0.976); no
contract can coordinate that.

## 11. Numbers

Script bytes: `KpfStake` 9,688 (1 + 270 + 9,417; O§1.3 estimated about 1.5 kB per stake input, O2§7 about 6 kB for a
shared template), `KpfClaimStub` 1,687 (1 + 129 + 1,557). Stakes of 100 KAS, pool and claim carriers 1 KAS, each input
committing the smallest covering compute budget, relay fee at 100 sompi per gram; largest stack 97 of 244.

| Transaction | Inputs | Script units (first input / each stake or receipt / funder) | Compute mass | Storage mass | Bytes | Fee (KAS) |
|---|---|---|---|---|---|---|
| open (claim, opener's stake, funder) | 3 | 125,499 / 302,773 / 100,001 | 18,807 | 80,400 | 12,137 | 0.0243 |
| deposit, 1 stake | 3 | 299,769 / 229,904 / 100,001 | 27,260 | 40,400 | 20,060 | 0.0401 |
| deposit, 7 stakes | 9 | 1,136,277 / 229,904 / 100,001 | 110,660 | 42,800 | 79,640 | 0.1593 |
| resolve and deliver (dispute, claim, funder) | 3 | 128,808 / 119,128 / 100,001 | 16,357 | 80,000 | 11,957 | 0.0239 |
| settle, 1 losing / 7 losing receipts | 3 / 9 | 210,488 / 635,151; 99,335 each | 24,610 / 92,764 | ≤ 57 | 19,880 / 78,434 | 0.0398 / 0.1569 |
| settle, 1 winning / 7 winning receipts | 3 / 9 | 279,724 / 1,119,596; 99,335 each | 25,633 / 100,525 | ≤ 724 | 19,933 / 78,805 | 0.0399 / 0.1576 |
| close (with burn) | 2 | 19,878 / 100,001 | 11,908 | 125 | 10,078 | 0.0202 |
| unbond / withdraw | 2 | 227,933 / 120,040 | 14,065 / 13,054 | 0 | 10,135 / 10,134 | 0.0203 |

The storage masses of 40,000-80,000 come from the 1-KAS carriers (KIP-9 charges a small output about 10¹²/value); a
larger pool base lowers them. **Stakers paid per transaction**: 7 (the unrolled loop); a winning receipt adds 12,482
compute mass and 9,812 bytes, so by mass alone 24 would fit. **Contention**: one dispute's deposits pass through its one
UTXO, at most 7 stakes each: an answer by `n` stakers is `⌈n/7⌉` chained deposits (1,000 stakers: 143 deposits, about
22.8 KAS of relay fees, 0.023 KAS per staker). Commitments signed SINGLE\|ANYONECANPAY are not bound to the dispute's
outpoint, so a lost race costs an aggregator a rebuild, not a new round of signatures; only the first partial deposit
after a lead change waits for that change to be accepted (it reads the round's start from it), later ones chain in the
mempool. Nothing is shared between disputes. A griefer can occupy the slot only with aged stake on the trailing side
(the leader cannot deposit), at least `minStake` per deposit, at risk like any other.

## 12. Where the notes are wrong or ambiguous (each a `// DESIGN:` comment)

1. **Age measured at the deposit** (O§2 "aged ≥ `M_age` (CSV)"; `KpfStake.open`, `Spend::Open`). A dispute that runs
   longer than `M_age` then admits capital bonded after the claim. Here a stake counts only if its UTXO was accepted
   `M_age` before the claim's own UTXO (the anchor, exact): capital raised for the occasion never counts.
2. **The opening deposit is "any KAS"** (O§2). It would let fresh capital set the first bar. Here it is aged judge stake.
3. **Deposits to either side, every deposit restarts the window, minimum step `ε`** (O§2). The leader could raise the bar
   in the last DAA of a window, and either side could prolong a dispute at a cost of `ε` per restart. Here only the
   trailing side deposits; the window moves only when the lead changes; `ε` is dropped (`minStake` bounds the deposits).
4. **`μ` just above 1** (O2§1.2 `10⁴ < μBps`). The number of rounds is `log_μ` of the stake range, about 230,000 at
   1.0001; here `μ ≥ 1.1` (at most 222 lead changes from 10 KAS to the supply). O2's "`M_age ≥ 7 days`" is a DAA count
   that depends on the network; the bound is `M_age, U ≥ D_r`, the 7 days are the default.
5. **The dispute under a new id with a checked genesis** (O§2) cannot deliver its verdict safely: the dispute must name
   the contested claim's script and the claim the dispute's id, a cycle. O2§1.7's "same id" is used: dispute and receipts
   take the claim's id (no genesis, no twin scan); receipts carry the anchor so that an earlier dispute's receipts of the
   same lineage never settle against a later one.
6. **Resolution and delivery as separate steps** (O§2 is silent). Delivered later, the pool could close before the claim
   heard the verdict, or the verdict could go to another UTXO of the claim's id (a pair's parts share an id). Here the
   resolving transaction spends the contested claim, identified by the script hash recorded at the opening.
7. **Receipts as records beside a dispute UTXO holding every stake** (O§2). Small outputs cost about 10¹²/value of
   storage mass each. Here a receipt is the stake itself; losers are forfeited into the pool, winners paid from it.
8. **Units.** Weights in whole 10-KAS units as the note, but the lead rule and every payment in exact sompi (the note
   counts everything in units); a stake's remainder below 10 KAS carries no weight.
9. **SINGLE\|ANYONECANPAY commitments** (Z13), where every other owner entry requires SIGHASH_ALL.
10. **Burn dust**: a burn below 0.02 KAS stays with the executor (as `KpfLock`).
11. **The note's `f*`** is exact for one dispute; across parallel disputes against a patient attacker it is the threshold
    to win one, while greedy honest answers lose all at `μ p_h/(1 + μ p_h)` (§10).

## 13. Attacks (`judge_attack_tests`, `judge_model`, and the path tests named)

| Attack | Verdict |
|---|---|
| a dispute that never resolves, or is prolonged | **holds**: partial answers do not move the window, the leader cannot deposit, rounds are bounded by `μ ≥ 1.1` (`holds_a_dispute_always_resolves_and_cannot_be_prolonged`); every game of the model ends |
| a dispute resolving for the side with less stake | **exploit, by design** (`exploit_the_leader_wins_with_less_stake_when_the_answer_falls_short_of_mu`): a leader holding `x` wins against any answer below `⌈μx⌉`, even one above `x`; that is the note's rule, the price of bounded rounds |
| stake counted twice, in two disputes or two rounds | **holds**: a receipt carries one id, a receipt cannot commit, a UTXO is spent once (`holds_one_stake_counts_once`) |
| aged stake manufactured: a parent not yet accepted (-1), a lock time, winnings, a look-alike stake | **holds**: the anchor and the UTXO's own score only; winnings come back fresh; the claim refuses an opener whose state is not canonical (`holds_aged_stake_cannot_be_manufactured`) |
| a staker paid twice or not at all | **holds** for every dispute an opening made: exact to the sompi (`settlement_pays_uneven_stakes_to_the_sompi`, the property games), a look-alike receipt refused, a winner waits only for forfeits anyone can make (`holds_a_staker_is_paid_once_and_exactly`) |
| the outcome delivered to another claim, or twice | **holds**: script hash, id, the dispatch read by the claim (`holds_the_outcome_reaches_the_disputed_claim_once`, `a_claim_decides_only_from_its_disputes_resolution`) |
| a decided claim re-opened | **holds**: not pending (`holds_the_outcome_reaches_the_disputed_claim_once`, `a_decided_claim_cannot_be_reopened`) |
| a rich minority winning by timing | **holds**: a last-DAA answer gives the other side a full window, the bar is fixed for the round, capital younger than the claim never counts (`holds_a_rich_minority_cannot_win_by_timing`, `a_late_flood_...`); a minority richer than `1/μ` of the participating honest stake wins, as O§2 says (Z10) |
| griefing honest stakers | **holds, with cost**: an opening on a true claim with 100 KAS (testnet) forces 125 KAS of honest stake idle for 7.5 days (938 KAS-days: the round plus the age its fresh winnings need), and the attacker loses its 100 KAS, 90 of them to honest: 0.107 KAS per honest KAS-day (`holds_griefing_honest_stake_costs_the_attacker_its_stake`) |
| a relayed commitment landed late | **exploit, bounded** (`exploit_a_relayed_commitment_can_land_after_the_window_and_be_forfeited`): no upper bound on time exists, so whoever holds a commitment can land it as a partial answer after the window, before the resolution; its stake is then forfeited. Void after a lead change (round) and in any other dispute (binding, anchor); revocable only by spending the stake. Stakers sign only into an answer an aggregator completes |
| a commitment moved to another dispute | **holds** (Z13; `holds_a_commitment_cannot_be_moved_to_another_dispute`) |
| a dispute written by a genesis | **exploit, bounded** (`exploit_a_dispute_written_by_a_genesis_traps_the_stakers_who_join_it`): a forged tally with no losing receipts lets honest stakers win a pool that can never pay them; only stakers who join a dispute they did not trace to an opening (a claim of a verified lineage spent by `open`) lose. Wallets must check the opening transaction, as slice A's look-alike pairs |
| a dispute or stake left unspendable | **holds** for every state the template writes (`holds_no_dispute_or_stake_is_left_unspendable`) |
| model and contracts disagree | **holds**: 1,398 replayed steps of 120 random games with 3,255 hostile variants (782 accepted by both, 2,473 refused by both): 100 openings, 184 deposits, 100 resolutions, 194 settlements, 100 closes, 68 withdrawals |

**The model's games** (`judge_games_conserve_terminate_and_never_take_more_than_was_staked`): 300 seeded games, 5,073
receipts, 656 lead changes, uneven stakes with sub-unit remainders, both sides by one key, unaged and leader stake and
early resolutions (all refused), answers in the last DAA of a window, settlement in random order and batches. On every
game: winners' gain plus burn plus the executor's dust equals the losers' forfeit exactly; the rounding is below one
sompi per winning receipt; no receipt is paid less than it staked if it won, or anything if it lost; the claim gets the
leader's verdict; the game ends.

## 14. What the next slice needs from this one

- **Wiring into the pending part** (`KpfPendingO`): its contest is the stand-in's (`contest`: the opener read by
  template hash, canonical state and dispatch, three outputs of its id; the anchor is its `P`), plus O2§1.7's
  reservation of `σ·c` from the underwriter's slot (a `KpfBond` reserved status, the model's `Bond::reserve`) and the
  counter quote `p′`; its decide restores (refuted) or finalizes (upheld); the restitution and slash are paid from the
  reservation at the same resolution, not by the pool. `JUDGE_TPL`, `OPEN_TAG`, `RESOLVE_TAG` become its build constants.
- **The oracle-market pair and orders**: the judge block (87 bytes) joins the oracle market block, and `checkMarket`
  calls the `checkJudge` text; `KpfOpen` and the pair are unchanged until then.
- **The underwriter's price claims and records** (`KpfRecord` for `T_retro`): contested the same way, with their own `P`.
- **Closed-market rules**: a claim about a closed session is false by the feed definition; that is the judge's (the
  stakers') judgement, not a covenant rule; no judge change is needed.
- **Tooling**: wallets must trace a dispute to its opening before committing; aggregators chain deposits from
  SINGLE\|ANYONECANPAY commitments and complete an answer in one chain.
- **Not done here**: pinning `KpfStake` and the claim in `contracts/artifacts/`, builders and command line; the fold
  with `KpfBond` and `KpfLock`; the coordinated (triage) honest strategy of the drill; Z12.
# Third slice: the integration (oracle-market pair, order and pending part)

Status: implemented and tested offline; pinned (`contracts/artifacts/oracle/`), builders and command line; not on
testnet. Implements the smallest first version of `DESIGN-ORACLE-2.md` (O2§7) end to end: form (a) only (KAS margin, the
underlying priced in KAS); `ρ` a market parameter, 1 by default, refused above 5; one underwriter per pair, chosen at open
by both orders; live disputes during the window, decided by the stake judge of the second slice; no retrospective
dispute, no fast path, no request object (`KpfReq`), no lock path, no rebind; closed markets by the feed definition and
a lateness bound; markets permissionless (the contracts enforce parameter bounds, never a list of markets). Sections
15-25; section 24 records the changes after review 6 and section 25 those after review 7, which supersede the text of
15-24 where they differ.

## 15. What exists

| Path | What |
|---|---|
| `contracts/KpfOpenO.sil` | the oracle-market order: `fill` (T1′: two orders, the slot's bind, the twin scan), `take` (T2 with a fresh bind), `cancel`, `refund` |
| `contracts/KpfPairO.sil` | the oracle-market pair: `amend`, `addMargin`, `transfer`, `closeHost`, `closeDonor`, `mutualClose`, `coopClose`, `lapseSettle`, `renew`, `propose` |
| `contracts/KpfPendingO.sil` | the oracle-market pending part: `contest`, `decide`, `cure`, `finalize`; statuses pending, contested, upheld, refuted |
| `contracts/KpfBond.sil`, `KpfStake.sil` | unchanged from slices A and B1 (source, bytecode and tests untouched); pinned with the family |
| `contracts/artifacts/oracle/` | the five pinned artifacts, `manifest.json` (template hashes, cuts, the bind, open and resolve dispatch tags), `SHA256SUMS`; the book market's files in `contracts/artifacts/` are byte for byte as before |
| `crates/kpf-protocol/src/oracle.rs` | the oracle market block (codec, id, bounds, testnet defaults), the feed check, the states and codecs of the three templates, the math of every entry (trigger with g, close price, bond and its split, cure and forfeit bounds, validity, the settlement of every branch, renewal, lapse), the wallet's market warnings, the worst case |
| `crates/kpf-protocol/src/oracle_artifacts.rs`, `oracle_build.rs` | the pinned kit; builders of every shape: slot (create, advance, split, unbond, withdraw), stake (create, unbond, withdraw), order, open, propose, contest, deposit, resolve and decide, finalize, cure, renew (up to 7 pairs), lapse settle, cooperative close, amend, transfer, mutual close, settle and close a pool |
| `crates/kpf-cli/src/ocmds.rs` | `oracle-market`, `oracle-check`, `bond`, `unbond`, `withdraw-bond`, `stake`, `oracle-order`, `oracle-open`, `vouch`, `contest`, `deposit`, `resolve`, `oracle-cure`, `oracle-finalize`, `renew`, `lapse-settle`, `settle-pool`, `oracle-watch` |
| `crates/kpf-tests/tests/oraclekit/` | fixtures and engine builders of every shape (the templates compiled from source, checked equal to the pinned ones) |
| `crates/kpf-tests/tests/oracle_tests.rs` | the lifecycle and the negatives of every rule (24) |
| `crates/kpf-tests/tests/oracle_attack_tests.rs` | the attacks of section 21, the one-text checks, pinned equals compiled (24) |
| `crates/kpf-tests/tests/oracle_conservation_tests.rs` | the seeded conservation run across pairs, slots and stakes, and the model-against-engine replay (1) |
| `crates/kpf-tests/tests/oracle_metrics_tests.rs` | the numbers of section 19 (1) |
| `crates/kpf-protocol/tests/oracle_builders.rs` | the manifest and checksums, the codecs, every builder signed and verified, 46 golden vectors (3) |
| `crates/kpf-cli/tests/oracle_cli.rs` | the command-line lifecycle and the wallet check's refusals (1) |

Run: `CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2` (547 tests: the 493 before this slice and 54 new), or
`-p kpf-tests --test oracle_tests` etc.; `--nocapture` prints the tables and the worked numbers. Regenerate the pinned
artifacts with `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`.

## 16. The mechanism as built

**The market block** (state `[0..309)` of every order, pair and pending part; the byte table is in `oracle.rs`). The
market id is `blake3` of the body `[33..309)`: the bond parameters (`KpfBond`'s, so `[0..87)` is a slot's bond block byte
for byte), the judge parameters (`KpfStake`'s: the judge block is `[0..33) + [87..141)`), the feed hash, and the pair
parameters `scale, priceMax, mmrBps, gBps, liqFeeBps, keeperFeeMin, minSize, carrier, idleDaa, challengeDaa (W),
lateDaa, quietDaa, sigmaBps (σ), betaBps (β), bondMin`. `checkMarket` is one text in the three templates; its lines on
parameters both families carry are `KpfOpen.checkMarket`'s lines, and its bond and judge parts are `KpfBond.checkBond`
and `KpfStake.checkJudge` (`holds_the_market_bond_judge_bounds_and_the_bind_proof_are_one_text`). It requires the id to
be the body's hash and every parameter in the range that keeps every branch live: beyond the book bounds,
`0 < W < term`, `0 <= lateDaa, quietDaa < 2^32`, `0 <= σ, β <= 10^4`. A block one byte off is another market (another id,
bond block and judge block): orders, slots and stakes of two markets never meet.

**The binding** (pair state `[411..504)`): `uwId` (the slot's covenant id), `uwKey` (its owner), `coverRate` (cover per
whole unit, at least the range; the cover of a part of n is `q(n, coverRate)`), the expiry `X`, the `premium` (sompi per
whole unit per term). Every price-free entry keeps the range within `coverRate`, so no claim moves more than the counted
cover.

| Entry | Who | Rule |
|---|---|---|
| order `fill` (T1′) | anyone | the book order's checks; the slot input is a `KpfBond` of this market spent by `bind` counting `q(n, coverRate)` at `X` for `OUT_REF + pairOut` (the bind proof); the slot accepted (`uwAccept`, zero for any), its owner neither this order's owner nor the counterparty; `premium <= premMax`; `coverRate >= bs - bl`; `X >= this order's DAA score + term`; the pair id fresh and carried by no other output (the scan, at most 8 outputs); the escrow spend `q(n, rate + tip + half the premium) + carrier` |
| order `take` (T2) | anyone | as `KpfOpen.take`, but the part starts a fresh covenant id the order checks (review 6), and the slot binds the entered part afresh in the same transaction (claim `OUT_REF + partOut`, at the part's `X`) |
| pair `amend`, `addMargin`, `transfer`, `closeHost`, `closeDonor`, `mutualClose` | as KpfPair | as KpfPair; the pair UTXO `quietDaa` old (CSV); the range within `coverRate`; no entrant and no key of P3 is `uwKey`; every part and P3 carries the spent pair's binding byte for byte (review 8, section 26: `closeDonor` took P3's cover rate, expiry and premium as arguments before), so close-versus-close joins only pairs with identical bindings |
| pair `coopClose` | both owners | any split |
| pair `lapseSettle` | anyone | `tx.daa >= X + tail + idleDaa` (CLTV, review 6; before: `X + tail` and the UTXO `idleDaa` old by CSV): settle at the entry price, as KpfPair's idle settle |
| pair `renew(u, k)` | the slot's bind (the underwriter signs) | the bind counts `q(s, coverRate)` at `X + term` for this input; the premium leaves the margins (the long's bound up by `premium - floor(premium / 2)`, the short's down by `floor(premium / 2)`); what may leave is `q(s, range) - q(s, new range)`; the range stays wider than both bands; possible only from `X - step` (the slot's window, a lower bound) and, since review 9, only from a pair UTXO accepted no later than `X` (section 27) |
| pair `propose(n, kind, side, p, t0, j, ...)` | the underwriter's P2PK input `j`, SIGHASH_ALL (and the closer's signature for a close) | `tx.daa >= t0` (CLTV); a liquidation triggers with g inside: long `ceil(p(1 + g)) (1 - m) < bl`, short `bs < floor(p(1 - g)) (1 + m)`; the keys differ; the range rule; the fee, the cure bound (no cure beyond `coverRate`) and the forfeit bound as KpfPair; the part moves into `KpfPendingO` with a bond `B = max(bondMin, 0.02 KAS, q(n, coverRate · σ))` |
| pending `contest(u, dOut)` | anyone with aged judge stake | the stand-in's contest: the opener is a canonical free `KpfStake` of this judge block spent by `open(self, self, dOut)`; three outputs of the id; records `P` and the opener's key |
| pending `decide(d)` | anyone, in the dispute's `resolve` | the stand-in's decide: upheld if YES leads, refuted if NO leads; upheld whatever the verdict if the contest was accepted at `P + W` or later |
| pending `cure(s)` | the loser, while pending | as `KpfPending.cure`, the refund to `uwKey` |
| pending `finalize()` | anyone: pending at age `W`, decided at once | refuted: restore, `floor(B · β)` to the disputer at `M - 1 - i`, the rest to the defender at `i`. Otherwise valid (`P + W + step <= X <= P + term + step` since review 10, `P - t0 <= lateDaa`): settle (a close at the price moved by g against the closer, its refund back; a liquidation forfeit and keep, `B` + fee to `uwKey`); void: restore, `B` to the defender |

**The vouched claim.** The evidence of a proposal is the underwriter's own P2PK input whose signature script is exactly
one 65-byte push ending in SIGHASH_ALL (experiment Z1's reader with the type check). Consensus verifies the signature;
the pair checks what it signed: every outpoint (so this pair's state), every output (the pending state with `p`, `t0`
and the bond) and the lock time. It cannot be replayed (the input is spent), reused on another part or after the part
changed (another outpoint: the copied signature fails, `a_claim_reused_on_another_part_or_after_the_part_changed_is_refused`),
or delayed harmfully (a claim landing more than `lateDaa` after its `t0` is void; spending the input revokes an unused
claim).

**Contest and verdict.** The pending part's `contest` and `decide` are the stand-in claim's of the second slice with the
oracle judge block; the dispute and its receipts take the claim's id (since review 6 a fresh id per claim, section 24); the resolution spends the contested part and
delivers the verdict in the same transaction (the dispute's recorded script hash and id:
`a_verdict_delivered_to_the_wrong_part_is_refused`). The window is enforced after the fact: the contested part's own DAA
score is the contest's acceptance, exact when `decide` reads it; a contest accepted at `P + W` or later is ignored, so a
timely contest and a finalize can never both be valid (`contest_one_daa_after_the_window_does_not_stop_the_claim`).

**What a false claim costs, and how the wronged party is paid.** A claim's bond `B` is posted with the claim by its
proposer (the underwriter for a liquidation, the closer for a close) and sits in the pending part. Refuted: the part is
restored exactly (keys, bounds, entry, binding; unlisted, tags 0), the disputer gets `floor(B · β)`, the defender the rest
(120 + 480 KAS of a 600 KAS bond at β = 20 %). Void (landed late, or too close to the expiry): restored, `B` to the
defender, so a void claim is never a free freeze. Upheld or uncontested and valid: `B` back to its proposer.

**Renewal and lapse.** The underwriter renews in the last step before `X` (the slot's window `x <= t + term + step`) at
the agreed premium, half from each margin; it cannot take more (one sompi more is refused) and nobody else can renew. A
pair whose cover is not renewed has no valid claim landing after `X - W`; from `X + tail`, once `idleDaa` idle, anyone
settles it at its entry price.

**Closed markets.** The covenant cannot read a calendar: DAA is not wall time and time has only lower bounds. What it
enforces: `t0 <= tx.daa` (no claim prices the future) and `P - t0 <= lateDaa` (a claim lands within `lateDaa` of the
moment it prices, 10 minutes at the testnet defaults), so a claim priced before a closure cannot be landed hours into it.
What it cannot enforce: that `t0` lies in an open session. That is the feed definition's `sessions:` line: a claim whose
`t0` lies in a closed session is false by definition and the judge refutes it when contested. The wallet's check flags a
feed definition without a `sessions:` line. A weekend maintenance rate (`mmr_w`) is not built.

## 17. What a party can do when its underwriter goes silent

It is never stuck: (1) a transfer to a signing taker (no underwriter needed; an entrant through an order needs the slot's
bind, so the underwriter), (2) a mutual close (both list, anyone executes), (3) a cooperative close (both sign), (4)
close-versus-close with another pair of the same binding (the same slot, cover rate, expiry and premium; section 26), (5) once the cover has lapsed (`X + tail`) and `idleDaa` more
has passed by the calendar (review 6), a settlement by anyone at the entry price (`lapseSettle`, what replaces the idle settle). The cost: no
liquidation and no unilateral close while the underwriter is silent; a winner whose loser is past its trigger gets its
entry-price payoff at the lapse settle instead of the loser's margin
(`exploit_a_silent_underwriter_turns_a_due_liquidation_into_a_settlement_at_the_entry`). An underwriter that renews but
never vouches keeps the pair alive only while the premium leaves the range wider than both bands (97 terms on the fixture
pair); exits (1)-(4) stay open throughout. No rebind or request exists in this slice.

## 18. Experiments of this slice

| # | Question | Measured | Verdict |
|---|---|---|---|
| Z2′ | can the contest window be enforced although time has only lower bounds | the contested part's DAA score read at `decide` is the contest's exact acceptance; `C = P + W - 1` refutes, `C = P + W` is ignored | **holds**: the window is a deadline after the fact |
| Z4 | the masses of T1′, propose with the underwriter's input, contest, decide, a renewal of 7 pairs | section 19 | **holds**: every shape within the limits |
| Z5 | contested part, dispute, restore or settle; delivery to the right part only | lifecycle and attack tests | **holds** |
| Z11 | the twin scan at a pair's genesis, any template as the twin | `holds_a_twin_of_any_template_at_a_pairs_genesis_is_refused` | **holds** (8 outputs at most) |
| Z3′ | cover `<= ρK` on every reachable state of the integrated flow | the conservation run (section 21) | **holds** |

## 19. Numbers

Refreshed after review 11 (the numbers of this slice as first built are in the history of this file; each later section
states what its change moved). Script bytes (prefix + state + suffix): `KpfOpenO` 6,536 (1 + 465 + 6,070), `KpfPairO`
16,149 (1 + 723 + 15,425; the note estimated about 10 kB), `KpfPendingO` 4,427 (1 + 813 + 3,613; about 5 kB), `KpfBond`
4,493 (1 + 236 + 4,256), `KpfStake` 9,665 (1 + 270 + 9,394). The stack limit is 244: the stand-ins' bind proof (a local
per argument) exceeds it with the oracle market's 41 state fields, so the oracle order and pair carry an equivalent lean
text (the same push checks, three reads). The largest stack is the take's, 209; the pair's largest 181 (a liquidation's
propose).

Transactions (each input committing the smallest covering compute budget; relay fee at 100 sompi per gram; the fixture
pair: 1,000 units, cover 6,000 KAS, a 10 KAS opening stake, a 0.5 KAS dispute base; `oracle_metrics_tests`):

| Transaction | Inputs | Script units per input | Compute mass | Storage mass | Bytes | Fee (KAS) |
|---|---|---|---|---|---|---|
| open (2 orders, slot bind, funder) | 4 | 306,013 / 306,165 / 170,523 / 100,001 | 60,598 | 529 | 49,698 | 0.0994 |
| propose a liquidation (pair, vouch, funder) | 3 | 690,223 / 100,001 / 100,001 | 30,020 | 0 | 20,390 | 0.0408 |
| a party's own close (pair, the closer's funding) | 2 | 452,181 / 100,001 | 26,500 | 6 | 20,270 | 0.0405 |
| finalize: forfeit, close, void, upheld, refuted | 2 | 9,636 to 9,884 / 100,001 | 6,939 to 7,396 | <= 109 | 4,859 to 4,946 | 0.0097 to 0.0099 |
| cure | 2 | 110,438 / 100,001 | 8,150 | 28 | 4,960 | 0.0099 |
| contest (pending, opener's stake, funder) | 3 | 187,039 / 302,307 / 100,001 | 22,124 | 80,390 | 14,854 | 0.0297 |
| deposit, 1 stake / 7 stakes | 3 / 9 | dispute 299,061 / 1,133,613, each stake 229,605 | 27,214 / 110,476 | 80,079 / 82,231 | 20,014 / 79,456 | 0.0400 / 0.1589 |
| resolve and decide | 3 | 128,526 / 179,201 / 100,001 | 19,674 | 79,972 | 14,674 | 0.0293 |
| settle 1 winning + 1 losing receipt | 4 | 349,652 / 99,105 / 99,105 / 100,001 | 36,923 | 3,483 | 29,623 | 0.0592 |
| close the pool | 2 | 19,849 / 100,001 | 12,297 | 20,924 | 10,107 | 0.0202 |
| renew 1 / 4 / 7 pairs (one bind) | 3 / 6 / 9 | each pair 327,236 to 327,271, slot 170,627 to 170,941 | 28,839 / 88,449 / 148,059 | <= 524 | 21,479 / 70,379 / 119,279 | 0.0430 / 0.1408 / 0.2386 |
| lapse settle / mutual close | 2 | 33,255 / 33,890 | 18,961 / 18,971 | 4 | 16,581 / 16,591 | 0.0332 / 0.0332 |
| amend / transfer to a signing taker / cooperative close | 2 | 347,552 / 285,152 / 233,391 | 21,768 / 21,589 / 21,093 | <= 24 | 16,638 / 16,699 / 16,713 | 0.0333 / 0.0334 / 0.0334 |
| take by an order with the slot's bind | 4 | 285,298 / 306,926 / 170,523 / 100,001 | 54,109 | 30 | 43,779 | 0.0876 |
| unbond the slot | 2 | 168,370 / 100,001 | 8,313 | 12 | 4,983 | 0.0100 |

The storage masses near 80,000 come from the 0.5 KAS dispute base and the 10 KAS receipts (KIP-9); a larger base lowers
them. **The largest multi-pair shapes:** the fresh-id scan limits one proposal transaction to 7 whole pairs (section
24): 1 / 4 / 7 proposals under one vouch use 30,020 / 111,842 / 193,664 compute mass and 20,390 / 80,402 / 140,414 bytes
(0.0408 / 0.1608 / 0.2808 KAS). A renewal of 7 pairs (the bind's 7 claims) uses 148,059 compute mass and 119,279 bytes.
An open carries one pair (the twin scan admits 8 outputs; two order rests, the pair, the slot, the premium and change
are six).

**One pair-month** (one term `Λ` = 28 days on testnet): open 0.0994 + one renewal 0.0430 + one claim (propose 0.0408 +
finalize 0.0098) = **0.1929 KAS of network fees**, plus the premium the orders agreed. At 10 % of the cover a year the
premium is 46.03 KAS a term for the fixture pair (0.767 % of its 6,000 KAS cover); the note's cost, `y/(Lρ)` a year per
side, is in the premium, not in the fees. A contested claim adds about 0.12 KAS (contest, a defence, decide, settle,
close). The honest life of the gold pair on the testnet defaults (`r11_holds_the_honest_life_...`, 30 transactions with
the tool's builders) pays: open 0.0994, a liquidation's propose 0.0409, a party's own close 0.0406, finalize 0.0097 to
0.0099, cure 0.0099, renewal 0.0430, contest 0.0297, resolve 0.0293, amend 0.0333, transfer 0.0334, lapse settle 0.0332,
pool settle 0.0398 and close 0.0198 KAS.

## 20. Where the notes are wrong or ambiguous (each a `// DESIGN:` comment)

1. **The slash by reservation** (O2§1.7; `KpfPendingO`). A reservation of `σ·c` from the slot at the opening of a dispute
   needs the slot to authenticate the contested claim, which it cannot: a genesis can create a look-alike pending part
   naming any slot (pinning the pending template in `KpfBond` would not help: the look-alike passes the template check),
   so any reservation entry lets anyone slash honest slots. The slash is posted with the claim instead, by its
   proposer, as the book market's bond. The slot only caps the cover (`ρ`) and is never slashed in this slice; slashing
   a slot belongs with the retrospective dispute and needs that authentication first.
2. **The live dispute's window** (O2§1.7) has no deadline a covenant can check. Enforced after the fact: the contest's
   exact DAA score is read at `decide`.
3. **Late claims** (O2§1.5, "τ = P beyond Δmax"): void instead, so an honest underwriter is never judged at a price it
   did not sign; a void claim pays its bond to the defender.
4. **"A claim accepted after X is void"** (O2§1.5): a claim accepted at `X - 1` would be pending past the cover. A claim
   is valid only if `P + W <= X` (`P + W + step <= X` since review 10, section 28).
5. **Idle settle must go** (O2§1.4, §7 decision 8): kept only after the cover has lapsed (`lapseSettle`). While an
   underwriter renews, `X` moves on and the settle is never due; without it a pair whose counterparty is gone and whose
   underwriter is silent had no exit needing nobody. (Review 6, R6-F3: the idle time was the UTXO's age, which a party
   restarted by amending; it is now `idleDaa` after `X + tail` by the calendar, section 24.)
6. **The hand-over rate limit** has no evidence to protect here. What needs protecting is the underwriter's signed claim,
   which commits to the pair's outpoint: any respend voids it. Every party entry needs the pair `quietDaa` old.
7. **The term an order gets** cannot be proven (no upper bound on the present): `X >= the order's DAA score + term`; the
   first premium is due whatever the remaining term.
8. **Look-alike pairs** (O2§5.2, left to the entrant's wallet): an entrant through an order requires the slot to bind
   the entered part in the same transaction (a second count of the part's cover until its expiry); only a signing taker
   relies on its wallet. (Review 6, R6-F1: the bind counts the cover but could not stop a forged verdict under an id a
   foreign UTXO also held; since then every entered part starts a fresh id its entrant checks, section 24.)
9. **Margin and cure beyond the cover** (O2§1.4, "otherwise the transaction must renew"): the range never exceeds
   `coverRate`; a cure that would is not offered (the orders choose the headroom at open).
10. **Who posts the bond of a close:** the closer (it signs the claim; the underwriter co-signs the price). The note's "no
    proposer bond" would make a false close free for a colluding closer and underwriter.
11. **The tolerance g** "never liquidates a healthy pair" (O2§1.5, §5.8) holds only to second order and only when g
    exceeds the feed's own disagreement tolerance (section 21).
12. **The premium** (O2§1.4, `⌈π·c·Λ/yr⌉`): a premium per whole unit per term, agreed by both orders (`premMax`), half
    from each side at open (from the escrow) and at each renewal (bounds inward). A back-dated renewal is possible
    (section 21).
13. **Close-versus-close "only between pairs with the same uwId"** is not enough when the host is a look-alike: the donor
    also required P3's expiry no later, its premium no higher and its range within the donor's own cover rate. That was
    not enough either (review 8, R8-F1: no host is needed, and an earlier expiry is a lapse the builder chooses): since
    section 26 the donor writes its own binding into P3, as the host does, so the two must be identical.
14. **The bind proof's text** of the stand-ins exceeds the stack limit with the oracle market's state; an equivalent lean
    text is the one text of the oracle order and pair.

## 21. Attacks (`oracle_attack_tests`, `oracle_tests`)

Worked on the fixture pair (1,000 units at 10 KAS, long margin 1,000 KAS, short margin 2,000 KAS, cover 6,000 KAS, bond
600 KAS) with a slot of 6,000 KAS.

| Attack | Verdict |
|---|---|
| a false claim nobody contests within W | **exploit, bounded by ρ**: forfeit and keep takes the loser's side (1,000 KAS from the long; the underwriter's fee 46 KAS); a slot of K counts at most ρK of cover: **at ρ = 1 one such pair (1,000 KAS), at ρ = 5 five (5,000 KAS)**, a sixth open is refused. The underwriter's own 6,000 KAS bond is never at risk in this slice. (Between the fixes of reviews 6 and 7 the bound did not hold: an underwriter merged a restored half and its remainder into one claim and kept the other half, 1,500.2 KAS for a 300 KAS void bond, R7-F1. Since section 25 it holds again: `audit7_tests::holds_underwriter_cannot_merge_two_halves_into_one_claim`) |
| the same claim contested in time, judge honest | **holds**: the part restored exactly; the underwriter pays its 600 KAS bond (480 to the long, 120 to the disputer); at ρ = 5 the five cost it 3,000 KAS. The parties lose nothing, at any ρ |
| claims slightly wrong, below the judge's tolerance | **holds when g exceeds the feed's tolerance** (g 2 %, tolerance 1 %: a long healthy 0.06 % above its trigger survives a claim 1 % low; a close 1 % high is paid 9.898 for 10). **Exploit when g is at or below it**: a pair within `g^2` of its trigger can be liquidated, losing its residual (at most 473.68 KAS here). Only a wallet can compare g with the feed text |
| letting the cover lapse at a chosen moment; refusing a due liquidation | **exploit, bounded**: the pair settles at its entry after the lapse; at a true 8.50 the winner gets 2,000.2 KAS instead of 3,000.2, losing the loser's entry margin (1,000 KAS) per pair, at ρ = 1 and 5 alike, summed over the colluding underwriter's pairs (at most ρK of cover) |
| a back-dated renewal | **exploit, bounded**: a lapsed pair renewed later pays the premium for a term in which no claim was valid; at most one premium per term of calendar time (20 KAS a term here) |
| extortion for renewal | **holds on chain** (the premium is fixed at open, one sompi more is refused, no other slot can renew); the leverage is off chain: not renewing (the parties exit or lapse) or renewing without vouching (97 terms at most here) |
| a freeze by void or false claims | **holds, with cost**: each void claim pays the defender 600 KAS, each refuted one the same plus the dispute's delay |
| cover released while relied on; the underwriter unbonding with a claim pending | **holds**: the slot counts the cover until `X + tail`, a valid claim has `P + W <= X`, and the bond is in the part, so a refuted claim pays after the slot has left |
| a bind beyond ρK by twins, parallel transactions, split bonds | **holds** (the slot's capacity to the sompi; one UTXO; each child within its own value) |
| a frivolous contest of a true claim | **holds, with cost**: the opener loses its stake less the burn to the defence (9 of 10 KAS); the settlement waits for the rounds |
| a party respending its pair to void a signed claim | **holds**: `quietDaa` after every party entry; the claim on the new outpoint lands meanwhile |
| a party voiding a valid claim or taking the bond | **holds**: validity is `P`, `t0`, `X` only; a late contest is ignored |
| the underwriter contesting its own claim to own the only dispute | **holds**: honest stakers deposit into it on the trailing side and refute |
| a Sybil key of the underwriter as a party | **exploit**: the contracts compare keys only |
| a stuck pair, slot, stake or dispute | **holds**: every state has an exit needing nobody (or only its owner): finalize, decide with the resolution, lapse settle, advance and unbond, settle and close |
| a genesis pair naming a real slot | **exploit, bounded** (as slice A): uncounted cover for at most one period; no order enters it without the slot counting the part; a signing taker relies on its wallet |
| look-alike pending parts and disputes | **holds** for its bond (its own, reaching no slot). The claim "no output can carry an existing part's id without that part as an input" was false (review 6, R6-F1: any input of the id authorizes, a plain key's included); since review 6 every entered part and every claim starts a fresh id its party checks, section 24 |
| a twin of any template at a pair's genesis | **holds** (the scan) |
| two inputs accepting one fresh output (R7-F1): two pairs filled at one price against one stayer handed over into one part, whole or partly; a restored half and its remainder proposed into one pending part; two hosts or two donors into one P3; two pairs into the part an order takes; an order's fill sharing a pair's hand-over part; twins of one id | **holds since review 7** (section 25): every fresh output names its makers by outpoint, one field per role, and each creating entry zeroes every role its shape cannot share. Before the fix the first two were accepted and the builder kept the second input's value: a bystander took a stayer's 2,000 KAS of margin with no price and no claim (`audit7_tests`) |
| a hostile market within the bounds (W 1 DAA, no quiet time, a 0.02 KAS bond, ρ 5, the whole bond to the disputer, settlement right after a lapse) | **exploit, only a wallet can flag it**: the contracts open a pair on it and a false claim finalizes one DAA after it lands; `market_warnings` names each. A feed definition whose hash matches can still render misleadingly (bidirectional overrides, homoglyphs, invisible characters, carriage returns): `check_feed` flags them, and only a wallet can |

**The conservation run** (`seeded_conservation_and_replay_across_pairs_bonds_and_stakes`, as run after review 7): 10
seeds of 150 steps (111 opens and 89 refused for capacity, 131 claims, 28 contests in time and 1 late, 28 resolutions,
126 finalizes, 5 cures, 16 renewals and 57 refused for capacity, 7 lapse settles, 87 hand-overs), every step built from
the model and accepted by the engine, 478 hostile variants (one sompi less, one DAA early, another key, the old id kept)
refused, and 2,578 hostile merges of two family inputs into one output refused (section 25.3). After every step: the
slot counts at most ρK and counts every live pair's cover; no branch requires more than its inputs hold; every account
of family inputs is covered by the outputs that name it and its documented payouts (the statement of 25.3, 692 steps);
every bond ends with its proposer (54,600 KAS of 78,600), its defender (21,960) or a disputer (2,040), to the sompi;
every committed stake ends as a winner's payout (4,700 of 4,800 KAS) or burned (100), the rounding below one sompi a
receipt; a false claim contested in time never settles (17 restored); a false claim not contested in time settles
(27). A party that did not act loses value only through such a claim, an upheld one, the agreed premium at a renewal,
or the lapse settle at its entry price. (This sentence was false between the fixes of reviews 6 and 7, when two inputs
could accept one output, R7-F1; the run then checked lineages but not that each output is relied on once. It is true of
the code since section 25, whose statement the run now checks.) (Review 8, R8-F1: it was false again until section 26:
a short alone moved its passive long by `closeDonor` into a P3 whose cover the short had written as already lapsed,
and the lapse settle at the entry price took the long's gain; the run never built a `closeDonor` without a host.
Since section 26 no party entry writes a binding: `audit8_tests`.)

## 22. Public wording (oracle-only markets, as built; O2§6 corrected)

**True.** "No admin keys, no operator, no upgrade path." "Anyone creates markets; nobody approves them; read the feed
definition and the parameters before you trade." "A covenant holds your margin, but whoever sets the price can move it to
your counterparty: price control is custody control." "Each pair has one underwriter, chosen when the pair opens; only it
can price your pair; it can underwrite at most `ρ` times the KAS it has bonded." "A false price contested within the
window is reversed: your position comes back as it was, and the claim's bond is paid to you and to whoever contested."
"Disputes are won by the side committing more aged bonded KAS, by a factor `μ`; if colluders control that, false prices
stand." "A false price nobody contests within the window stands: you can lose your whole margin in that pair." "If your
underwriter stops, you can still leave without any price (transfer, mutual or cooperative close), and once its cover has
lapsed anyone can settle your pair at its entry price, after a fixed time nobody can postpone." (Review 9, R9-F2: false as worded; true since section 27 as "after a fixed time no party entry postpones; the underwriter can still renew late until a party acts after the expiry".) (Review 8, R8-F1: false
until section 26 for a long whose short sold its side by `closeDonor`, which chose the long's new expiry; true since:
no party entry rewrites your cover.) (Review 6: the contest wording holds again only since the fixes of section 24; before them it was false for a part entered into an id another UTXO held. Review 7: it was false again between the fixes of reviews 6 and 7, when a bystander could take a stayer's margin by handing two pairs over into one part, and an underwriter could delete half a pair inside a proposal nobody could contest; it holds since section 25.) "The winner bears gaps beyond the loser's margin." "Prices are in
KAS." "Before you commit stake to a dispute, your wallet traces it to its opening and its claim to a pair's proposal;
without a node it tells you what it could not check." (Review 7, R7-F2: since section 25.)

**False.** "Trustless", "no trusted oracle", "decentralized oracle" as a security claim. "Insured", "covered by the
underwriter's bond", "repaid from the bond": in this version the underwriter's bond only limits how much it underwrites
and is never paid to anyone. "The total loss is capped" (only per underwriter, `ρ` times its bond in cover). "Liars are
always slashed" (only when contested in time and outvoted). "Gold price" or any index name; "in dollars". "Listed",
"verified", "official" for any market. "A price can be disputed after the window" (no retrospective dispute yet).

## 23. What remains for later stages

- **The retrospective dispute and records** (`KpfRecord`, `T_retro`): repayment of an offline victim from the slot. It
  needs the slot to authenticate a claim (20.1) before any slot can be slashed.
- **The fast path** for oracle claims (a guarantor's lock) and the **request** (`KpfReq`) answering a silent underwriter.
- **Rebind** to another slot (both parties, or `altSet` after the expiry) and the **lock path** of stranded pairs.
- **A wider closed-market margin** (`mmr_w` for claims flagged pre-close) and a session-aware wallet.
- **Dollar-token margin** (form b): token custody in the pair, the 3-slot limit, a KAS/USD floor for the cover, pauses.
- **`ρ` above 1 in practice:** the contracts admit up to 5; section 21 shows what it buys an attacker; the note's
  recommendation (`ρ <= 2` until the judge has a history) stands.
- Not done here: discovery from a node (object files only, as for pending parts and locks); a live testnet exercise; the
  `KpfLock` fold (section 6).

## 24. Changes after review 6

Review 6 (`docs/audit/REVIEW-6.md`) found one Critical, one High and two Medium findings and three notes. Its root cause:
under rusty-kaspa v2.1.0 any input that carries a covenant id, a plain key's UTXO included, authorizes outputs of that id
with any script (`CovenantsContext::from_tx`). The family treated "an object under this part's id" as authentic (the
dispute, its pool and its receipts live under the claim's id), and three entries placed a party into a lineage whose id
another UTXO could also hold: an order's `take` (the part's id only had to be non-zero), `closeDonor` (the host could be a
plain-key UTXO) and a genesis look-alike pair created with a plain-key twin. The decisions below are built; every
deviation is a `// DESIGN:` comment in the contracts.

### 24.1 The induction

**Every UTXO of a pair's or a claim's covenant id was written by a template of this family that a party of that part
verified.** Base: a lineage starts only where a script of the family checks, in the creating transaction, that no input
carries the new id and that no other output does (`fresh`, one text in `KpfOpenO` and `KpfPairO`: the twin scan of slice
A over at most 8 outputs; a brand-new id is invisible to the covenant counters of its creating transaction, so the scan,
not `OpCovOutputCount`, proves "exactly one"). Step: only an input of an id authorizes outputs of it (a genesis cannot
reproduce an existing id: it is a hash of an unspent outpoint), every input of the id is a template instance by
hypothesis, and every entry of `KpfPairO`, `KpfPendingO` and `KpfStake` that spends a UTXO of an id counts that id's
outputs exactly and fixes each one's script (enumerated in review 6, "What was checked and found sound"). Tests:
`audit6_tests` (every route); the seeded run checks it on every accepted step (24.5). (Review 7: the induction is about
ids. Value conservation also needs that each input's continuation is its own, which the fresh scan alone did not give:
two inputs computing one script accepted one fresh output, R7-F1. Section 25 restores it with the makers.)

### 24.2 The decisions as built

| # | Decision | Built as | What it replaced |
|---|---|---|---|
| 1 | a party never enters a lineage it cannot prove clean | a fresh id, checked by `fresh`, for: the part an order takes (`KpfOpenO.take`, checked by the order itself, whatever the other inputs are), the part of a transfer to a signing taker (`KpfPairO.transfer`), P3 of close-versus-close (checked by both `closeHost` and `closeDonor`), and every pending part (`propose`, decision 3). The old id is not carried; remainders, renewals, amends, restores, cures and forfeits (nobody new enters) keep their id | `take` and `transfer` continued the pair's id; `closeDonor` accepted P3 under any non-zero id other than its own; `propose` wrote the pending part under the pair's id |
| 2 | nothing is authentic merely because of its id | inside the induction the claim's fresh id is sufficient again: `decide`, `resolve`, `settle` and `redeem` keep their id checks, which no outsider can now satisfy. No further on-chain binding was added (deviation a) | the id of a lineage any input could hold |
| 3 | a dispute, its pool and its receipts are keyed by something unique to the claim | a fresh covenant id per pending part (chosen over the pending part's outpoint: it survives equal DAA scores, needs no change of `KpfStake`'s state, and also closes R6-F1 for claims on any lineage) | the claim lived under the pair's id; receipts carried only the anchor (the claim's DAA score), shared by two claims of one split accepted in one block |
| 4 | the lapse clock is the recorded expiry, not the UTXO's age | `lapseSettle` needs `tx.daa >= expiry + tailDaa + idleDaa` (CLTV) and no age; `OPair::lapse_from()` | `tx.daa >= X + tail` and the UTXO `idleDaa` old (CSV), which an amend restarted |
| 5 | upper bounds on the judge's parameters, one text everywhere | `checkJudge` in `KpfStake`, `KpfOpenO`, `KpfPairO`, `KpfPendingO` and the stand-in claim, and `JudgeParams::check`: `roundDaa <= ROUND_MAX` (3 days at 10 DAA/s), `ageDaa <= AGE_MAX` (30 days), `unbondDaa <= UNBOND_MAX` (90 days), `minStake <= MIN_STAKE_MAX` (10,000 KAS, was 10^6 KAS). `oracle-market` and `oracle-check` state every judge parameter in words and warn above 1,000 KAS of least stake, 14 days of stake age, a 1-day round or 30 days of unbonding, and say "a contest within the window is not realistically possible" when the window is under an hour, the least stake above 1,000 KAS or the age above 14 days | bounds below `2^32` DAA and `10^6` KAS; warnings only for a small stake and a short age |
| 6 | the notes, in the tooling | `oracle-check` states that a new market has no judge for its first `M_age` and that a late contest delays a claim for a round; `oracle-watch` reads every object's DAA and value from the node when `--node` is given (an object not at the node is reported as such), marks a DAA read from a file as unchecked otherwise, flags a contest that landed after the window, and gives the lapse settle's DAA | the words were silent; `oracle-watch` trusted the file's `daa` even with `--node` |

`// DESIGN:` deviations. (a) Decision 2's "explicit binding wherever the induction does not reach" cannot be built: the
induction does not reach a claim or dispute written by a genesis instead of by a pair's `propose`, and there the forger
writes every field and every object of its id, so no field a contract could check helps. No party of a real pair is in
such a claim; a staker who joins its dispute is protected only by its wallet tracing the claim's id to a `propose` (as in
section 13; the `KpfStake` header). (Review 7, R7-F2: the shipped wallet did not trace; since section 25 `kpf deposit` and
`kpf contest` do, `oracle_build::trace_dispute` and `trace_claim`.) (b) The fresh-id scan admits at most 8 outputs, so one proposal transaction now
carries at most 7 whole-pair claims (before review 6 only the mass limited it: 12 measured, 14 by bytes); 8 in one
transaction are refused (`oracle_metrics_tests`). (c) A renewal by the underwriter still moves the expiry of a lapsed pair
(the back-dated renewal of section 21, paid from both margins): no party can postpone the lapse settle, only the
underwriter's counted renewal can.

Interfaces that changed: `KpfPairO.transfer`'s last argument and `closeHost`'s fourth are now the absolute output index
of the fresh part (they were the part's ordinal among the pair id's outputs); `propose`'s `k` is the pending part's output
index; `lapseSettle` needs no sequence; the builders (`oracle_build::propose`, `transfer`, `lapse_settle`) write the
pending part and the entrant's pair as a genesis authored by the pair input. The commands are unchanged.

### 24.3 The sibling book-market family (no contract changed)

The same three routes were run against `KpfOpen`, `KpfPair`, `KpfPending` and `KpfLock` (`audit6_book_tests`). Each route
is accepted (the victim's side does land under an id the attacker holds through a plain-key UTXO) and each is harmless:
**holds**. No book-family entry treats a UTXO as authentic because it carries the part's id: every pair and pending entry
(and `KpfOpen.fill`, `take`, `refund`) requires `OpCovInputCount(selfId) == 1`, so the handle can never be an input beside
a genuine object of the id; the only foreign inputs ever read (the KOB evidence order, the ask's token custody) are
authenticated by template hash and P2SH, the custody's id compared with the market's token id; `KpfPending` settles from
its own state with template-checked counter-evidence (there is no dispute object to forge); `KpfLock` reads no covenant
id. A `KpfPending` forged with the handle pays out only what its author funded. Tests:
`holds_take_into_a_held_id_gives_the_handle_no_power`, `holds_close_donor_into_a_held_id_gives_the_handle_no_power`,
`holds_an_order_taking_a_genesis_pair_with_a_twin_gives_the_twin_no_power`, and the control
`holds_a_held_id_cannot_join_any_pair_entry`. The two-step paths add no reader of a part's id (the lock is read by
template; its claim id is informational).

### 24.4 The change attacked

| Attack on the fresh ids | Verdict |
|---|---|
| a reference to the old id dangles: cover bound to a pair by its id | **holds**: a slot binds by input or output index and counts by expiry bucket, never by the pair's id; a part under a new id renews, is claimed and lapse-settles (`holds_a_part_under_a_new_id_renews_claims_and_lapses`) |
| a pending claim or a dispute in progress when its lineage is handed over | **holds**: the claim has its own id; handing over the remainder neither voids nor redirects it; the dispute resolves onto its own contested part and the restored part continues the claim's id (`holds_a_hand_over_of_the_remainder_leaves_a_claim_and_its_dispute_intact`) |
| a vouch bound to a part | **holds as before**: the vouch signs the pair's outpoint, so any respend voids an unlanded claim; every party entry still needs `quietDaa`, and a new part is a new UTXO with its own quiet time |
| a listing, the quiet time | **holds**: the stayer's listing is copied into the part (as KpfPair); the entrant's side is unlisted; the new part waits `quietDaa` before any party entry |
| a forced hand-over to void a claim, orphan cover or escape a dispute | **holds**: only a listed side can be taken (its owner consented); a pending or contested part has no hand-over entry; cover follows the expiry, not the id |
| cover counted twice or not released when the id changes | **holds**: a transfer to a signing taker re-binds nothing (the cover stays counted at its expiry); an order's take counts the part again (the documented second count of 20.8); the seeded run moves each pair's cover to its new id and checks the slot after every step |
| a party stuck because its lineage changed | **holds**: the restored part (under its claim's id) amends, transfers and is claimed again, each new claim under its own id |
| a take with no pair at all (the order's checks are all on the part) | **exploit, as before and bounded**: whoever funds the counterparty side can "take" a resting order into a fresh part at the order's own terms (price, rate, the slot's fresh bind) and collect the order's tip; it was possible before (route 1 needed no pair either), it is what an order offers, and the part's lineage is now clean |

### 24.5 Tests

`audit6_tests` (15): every `exploit_` test of the review is now a `holds_` test of the same transaction
(`holds_take_into_a_held_id_is_refused_by_the_order`, `holds_close_donor_into_a_held_id_is_refused`,
`holds_an_order_taking_a_genesis_pair_with_a_twin_lands_under_a_fresh_id`, `holds_a_true_claim_cannot_be_refuted_by_a_forgery`,
`holds_receipts_of_two_claims_with_one_anchor_never_settle_in_each_others_pool`,
`holds_a_party_cannot_block_the_lapse_settle_by_amending`), plus a handle inside the take, a second output of each new
id (take, transfer, propose, close-versus-close: host and donor), a plain-key host, every forgery of a dispute, pool and
receipt under a part's and a claim's id, markets at and beyond each judge bound (an open and a stake), and the attacks of
24.4. `audit6_cli` (3): the unreachable judge refused, the bounds stated and warned, the notes stated; `oracle_cli` checks
the unchecked DAA and the late contest in `oracle-watch`. The seeded run (`oracle_conservation_tests`) now hands pairs
over, checks on every accepted step that each output with an id continues a known id from an input of the family or
starts a fresh one carried by no input and no other output (308 fresh lineages over the 10 seeds), refuses a hand-over
keeping the old id, and checks that every receipt settles only in its own claim's pool (a receipt of another claim is
refused). Tests whose premise changed: `oracle_tests::a_lapsed_pair_settles_at_its_entry_price` (the calendar clock),
`oracle_tests::finalize_takes_the_branch_validity_dictates` (two claims no longer share an id: the swapped outputs are
rebound), `oracle_metrics_tests` (proposals of 1, 4 and 7 pairs; 8 refused),
`judge_attack_tests::holds_the_judge_bounds_are_one_text_in_the_stake_and_the_claim` (the new constants), the 44 changed
golden vectors of `oracle_builders`, and the pinned artifacts.

### 24.6 Numbers after review 6

Script bytes: `KpfOpenO` 6,027 (was 5,845), `KpfPairO` 14,222 (13,337), `KpfPendingO` 4,298 (4,302), `KpfBond` 4,453
(unchanged), `KpfStake` 9,652 (9,688). Largest stack 180 of 244 (the take). The pinned artifacts and the golden vectors
were regenerated (`CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`, `KPF_GOLDEN=print`); the book
market's artifacts are byte for byte as before.

| Transaction | Script units per input | Compute mass | Bytes | Fee (KAS), was |
|---|---|---|---|---|
| open (2 orders, slot bind, funder) | 278,836 / 278,983 / 169,945 / 100,001 | 55,184 | 44,984 | 0.0900, 0.0857 |
| propose a liquidation / a close | 617,107 / 416,216 (pair) | 27,197 / 25,197 | 18,367 | 0.0367, 0.0350 |
| take by an order with the slot's bind | 252,530 / 279,408 / 169,945 / 100,001 | 49,072 | 39,442 | 0.0789, 0.0750 |
| transfer to a signing taker | 252,384 / 100,001 | 19,362 | 14,772 | 0.0295, 0.0278 |
| amend / cooperative close | 318,020 / 229,537 | 19,541 / 19,066 | 14,711 / 14,786 | 0.0294 / 0.0296, 0.0277 / 0.0278 |
| lapse settle / mutual close | 29,401 / 30,036 | 16,934 / 17,044 | 14,654 / 14,664 | 0.0293 / 0.0293, 0.0275 / 0.0276 |
| renew 1 / 4 / 7 pairs | each pair 293,416 to 293,450 | 26,572 / 79,501 / 132,430 | 19,512 / 62,631 / 105,750 | 0.0390 / 0.1253 / 0.2115, 0.0373 / 0.1182 / 0.1991 |
| contest / deposit 1 / resolve and decide / settle 2 receipts | 175,622 / 298,671 / 128,357 / 349,197 (first input) | 21,882 / 27,188 / 19,432 / 36,884 | 14,712 / 19,988 / 14,532 / 29,584 | 0.0294 / 0.0400 / 0.0291 / 0.0592 |
| 7 proposals vouched by one input (the most since review 6) | 617,539 to 617,562 each | 173,903 | 126,253 | 0.2525 |

One pair-month (open, one renewal, a close proposed and finalized): **0.1752 KAS** of network fees (was 0.1674).

## 25. Changes after review 7

Review 7 (`docs/audit/REVIEW-7.md`) found one Critical and one Medium finding. R7-F1's root: review 6 gave every part a
party enters and every claim a fresh covenant id checked by an output scan (`fresh`). That proves the output new and
unique; it does not say which input made it. Before review 6 every part was counted under its creating pair's own id,
which no other pair holds; since then two inputs whose entries compute one script both accepted one output and the
builder kept the second input's value (the aliasing class "two inputs satisfied by one output" that the book family
closed with its tags, DESIGN 4.4). R7-F2: `kpf deposit` committed stake into any dispute file without the trace the
documents relied on. The decisions below are built; every deviation is a `// DESIGN:` comment.

### 25.1 Makers

**Every output a script of this family creates and accepts names its makers.** The state of every output kind has one
field per role an input can create it in, each the creating input's maker commitment `me` = blake2b(outpoint txid ||
index as 8 bytes) (`KpfPairO.me`, `KpfOpenO.me`, `oracle::maker_of`; the outpoint is unique per input even for twins
of one id, and is spent once). A creating input requires its own role's field to hold its own commitment, the
co-maker role of its transaction shape to hold the argument it is given, and every other role to hold zero.

| Output kind | Roles (fields; the rest zero) | Who writes and checks each |
|---|---|---|
| a pair opened by orders (`KpfOpenO.fill`) | `mkLong` the long order, `mkShort` the short order | each order its own; the other's is its `counterMk` argument (zero for a counterparty that is no order) |
| a part handed over to a signing taker (`KpfPairO.transfer`) | `mkParent` the pair | the pair; the entrant's side is the argument `et`, zero for a taker |
| a part an order takes (`transfer` + `KpfOpenO.take`) | `mkParent` the pair, `mkLong` or `mkShort` the order | the pair writes itself and `et`; the order itself and `pm` |
| P3 of close-versus-close (`closeHost`, `closeDonor`) | `mkHost` the host, `mkDonor` the donor | the host itself and `dm`; the donor itself and `hm` |
| a pending part (`KpfPairO.propose`) | `mkParent` (KpfPendingO state `[780..813)`) the pair | the pair |

Pair state `[558..723)` holds the five fields (long, short, parent, host, donor) where the two tags were (`tagL`, `tagS`
are gone: their only use was to carry the order or donor ids these fields now carry by outpoint). Interfaces that
changed: `KpfOpenO.fill`'s `counterTag` is `counterMk`; `take` has a new argument `pm` after `counterKey`;
`closeHost`'s `tl3` is `dm`; `closeDonor` has a new last argument `hm`; `oracle_build::contest` and `deposit` take a
`Trust`.

Two inputs in one role can never accept one output: one field holds one commitment, and no two inputs share an
outpoint. Across roles, two entries can share an output only if no field is their own for both and no field is one's
own and the other's zero; that leaves exactly the documented compositions, two orders opening a pair, a pair and the
order taking its listing, a host and its donor, and no three inputs ever
(`holds_two_inputs_of_any_two_roles_accept_one_output_only_in_the_documented_compositions`). In particular an order's
fill can never share a pair's hand-over part (the fill zeroes the parent, the pair writes itself), and a transfer
never shares P3.

**DESIGN:** "a role nobody fills in this transaction must hold zero" cannot be checked by an input that does not fill it.
A creating input forces zero on every role its shape cannot share; the one co-maker role of its shape (the counter
order of a fill, the order of a take, the donor or host) is an argument it cannot verify, which only that co-maker's
own check gives meaning. A nonzero co-maker field with no co-maker present is accepted and harmless: no input relies on
the output through it, so its value is the builder's own.

### 25.2 Makers live for one transaction

No entry copies a maker field: every continuation, remainder, renewal, amend, add-margin, and every state a pending part
becomes (restore, cure, forfeit, precomputed by `propose`) carries every maker zero; contest and decide write the
pending part's maker zero. A stale field cannot satisfy a later input: a maker field is read only by its own input,
in the transaction that spends that input, against that transaction's outputs (`tx.outputs`), and the outpoint it
commits to is spent there and nowhere else. An output written earlier with a field naming a live UTXO is no output of
the transaction that spends the UTXO; spent beside it, its continuation carries an input's id (not fresh) and zero
makers (`holds_a_stale_maker_field_never_satisfies_a_later_input`, `holds_contest_clears_the_pending_parts_maker`).

### 25.3 The conservation statement

In every transaction the engine accepts, partition the family inputs into accounts (the inputs of one covenant id, or
one id-less input such as a free stake). An output of the family **names** an account when (a) it carries the
account's id (a continuation, which only an input of the id authorizes), (b) it starts a fresh id and one of its maker
fields holds the commitment of an input of the account, or (c) it is the family output at an input's own index and one
of the two has no covenant id (a stake's receipt, a winner's free stake). Then: **(C1)** every fresh family output
names at least one input of the transaction, and no input is named in two fields or by two fresh outputs; **(C2)** the
value of each account's inputs is at most the value of the outputs naming it (an output of a documented composition
counted for each of its makers) plus its payouts at the positional slots `i` and `M - 1 - i` plus what the step declares
(the premium a renewal releases, the pool's base and burn at `close`). So each input's value is accounted for by the
outputs that name it and its documented payouts, and no builder keeps an input's value by merging. The seeded run
checks C1 and C2 on every accepted step (692 over the 10 seeds, 329 fresh outputs named by their makers) and, at every
step, lets a hostile builder merge two family inputs into one output (two proposals into one pending part, 149 times
between two real pairs of the run that share a state, otherwise with a look-alike twin; two hand-overs into one part;
two long orders into one pair; a claim and a look-alike into one settled part): 2,578 merges, every one refused.

### 25.4 The sibling book-market family (no contract changed)

The class was checked once more across `KpfPair`, `KpfOpen`, `KpfPending` and `KpfLock` (`audit7_book_tests`, 12
tests). No exploit: every output an entry checks is tied to the input that checks it.

| Entry | Output | What stops a second input | Test |
|---|---|---|---|
| `KpfPair.transfer` | the part, the remainder | own-id count, `OpCovInputCount(selfId) == 1` | `holds_two_pairs_never_hand_over_into_one_part`, `holds_two_partial_hand_overs_never_share_one_part` (new) |
| `transfer`, `closeHost`, `closeDonor`, `mutualClose`, `idleSettle` | payouts | positional slot (`self`, `M - 1 - self >= inputs`) | `holds_two_idle_settles_never_share_payouts` (new) |
| `KpfPair.closeHost` | P3 (under the host's id), the remainder | own-id count | `holds_two_hosts_never_share_one_p3` (new) |
| `KpfPair.closeDonor` | P3 | tag `tagL` = the donor's own id, with the guard | `kpf_close_tests::p4_one_p3_cannot_be_claimed_twice` |
| `KpfPair.amend`, `addMargin` | continuation | own-id count | `holds_two_pairs_never_amend_into_one_continuation` (new) |
| `KpfPair.propose` | pending part (under the pair's id), remainder | own-id count | `holds_two_pairs_never_propose_into_one_pending_part` (new, whole and partial) |
| `KpfPair.coopClose` | any | both owners sign SIGHASH_ALL | none needed |
| `KpfOpen.fill` | the new pair | tag = the order's own id, with the guard; the id on no input | `kpf_open_tests::a_pair_output_claimed_by_another_long_is_refused`, `two_long_orders_sharing_an_id_cannot_claim` |
| `KpfOpen.fill` beside a pair's hand-over | the part | the fill needs `OpCovInputCount(pairId) == 0`, the pair its own id | `holds_an_order_fill_never_accepts_a_pairs_hand_over_part` (new) |
| `KpfOpen.take` | the part | tag = the order's own id, with the guard | `kpf_transfer_tests::p3_two_orders_cannot_claim_one_part` |
| a host's P3 that is also a take part | P3 | one tag field: the donor or the order claims it, never both (and a host's P3 as a take part is a plain transfer) | `holds_a_host_p3_is_claimed_by_the_donor_or_by_a_take_never_both` (new) |
| `KpfOpen.fill`, `take` | the escrow continuation | own-id count | `holds_two_orders_never_share_one_escrow_continuation` (new) |
| `KpfOpen.fill`, `take`, `refund` | the owner's refund | positional slot | none needed |
| `KpfPending.challenge` | restore | own-id count | `holds_two_challenges_never_restore_into_one_pair` (new) |
| `KpfPending.cure` | cured pair, refund | own-id count, positional slot | `holds_two_cures_never_share_one_cured_pair` (new) |
| `KpfPending.finalize`, `guarantee` | forfeit, payouts | own-id count, positional slot | `holds_two_finalizes_never_share_one_forfeited_pair` (new) |
| `KpfPending.guarantee` | the lock | the claim's own id in the lock's state, with the guard | `kpf_guarantee_tests::one_lock_for_two_claims_is_refused_in_one_transaction_and_across_transactions` |
| `KpfLock.dispute`, `release` | victim, burn, owner | positional slot | `guarantee_attack_tests::holds_two_locks_never_share_a_payout_or_a_burn` |

### 25.5 R7-F2: the wallet's trace

`oracle_build::trace_claim(id, birth, pending_redeem)` proves a claim's origin from its birth transaction: the claim's id
is carried by exactly one output of it and is the genesis hash of that one output (so, by the induction of 24.1, every
later UTXO of the id was written by a template of the family); the output's script is a pinned `KpfPendingO` in status
pending; and its maker field names a pair input that ran the pinned `KpfPairO`'s `propose` and writes exactly this
pending state. `trace_dispute(dispute, opening, birth)` adds the opening: three outputs of the claim's id, a contest
input spending the claim's birth output and carrying its redeem script, and the dispute's recorded script hash among
them. A transaction's id binds its outpoints, outputs and covenant bindings but not its signature scripts, so the one
step read from the body as given is that the pair input ran `propose` (the pending template and the clean birth are
bound by the output's script and the genesis hash). `contest` and `deposit` take a `Trust`: `Traced` (of this claim)
or `Untraced`.

`kpf deposit` (`--opening`, `--claim-tx`) and `kpf contest` (`--claim-tx`): with `--node` the dispute and the stakes are
read through the node and the trace is required (a failing or missing trace refuses); without `--node` the command
refuses unless `--accept-untraced` is given, and then prints what it did not check (the UTXO's liveness, and the trace
when no transaction files are given). These are the commands where the documents say the wallet must trace (13, 14,
24.2 (a)); a signing taker's check of a pair's genesis (20.8) has no command in this tool.

**DESIGN:** the trace reads the transaction bodies from files (a `kpf-tx/1` file the tool wrote, a node's `rpc` object, or
the bare transaction JSON): the node API this tool uses looks UTXOs up by address and has no transaction lookup by id.
A body is bound to the UTXO by its id, so any source will do; the node confirms the UTXO itself.

### 25.6 The change attacked

| Attack on the makers | Verdict |
|---|---|
| sizes: the stack limit 244 and the scan caps | **holds**: the pair state grows by 99 bytes and three fields, the pending part's by 33 and one; largest stack 197 of 244, the take (25.8); the fresh scan's 8 outputs and the bind's 7 claims are unchanged; 7 whole-pair proposals in one transaction still fit |
| a maker commitment a builder can influence | **holds**: it hashes the input's own outpoint, which the input's UTXO fixes; a builder chooses only the co-maker arguments, which mean something only to the co-maker's own check |
| an input that should not be a maker accepted as one | **holds**: only the creating entries read maker fields, each only its own role of the output it creates; an input named in a field it does not check (a pending part, a stake, an amend) neither gains nor loses (the spec table and the merge tests) |
| a correct maker field on an output created in another transaction than the one spending its maker | **impossible**: the field is read only by its maker, against the outputs of the transaction that spends it, and an outpoint is spent once (25.2) |
| the fresh scan | **holds**: both are required; a part naming its maker but keeping the pair's id, and a fresh part naming no maker, are refused (`holds_the_maker_and_the_fresh_scan_are_both_required`) |
| the cover binding | **holds**: a bind still names the output or input index (`OUT_REF + out`), unchanged; take, open and renew run as before |
| the vouch | **holds**: SIGHASH_ALL covers the maker fields with every output; one vouch covers only its own key's pairs (`holds_one_vouch_never_covers_a_proposal_of_another_underwriter`); several proposals under one vouch each write their own pending part |
| the quiet time | **holds**: unchanged; every party entry still needs `quietDaa` |
| the judge's deliveries | **holds**: contest and decide clear the pending part's maker, the dispute records the contested part's script with it zero, resolve and decide run as before (the lifecycle and seeded tests) |
| an honest shape that can no longer be built | **no documented one**: the 46 builder shapes, the multi-pair proposals and renewals, a take without a pair (`pm` zero), close-versus-close with a plain donor (`dm` zero) and a transfer to a signing taker all build, sign and verify. Two undocumented compositions the tags admitted are gone: an order taking a host's P3 (`closeHost` in place of `transfer`) and a donor moving into a transfer's part (`transfer` in place of `closeHost`); each is a transfer or a close-versus-close in another shape, which remain |

### 25.7 Tests

`audit7_tests` (17): the review's three exploits are now `holds_` tests of the same transactions (each with the part
named by either input: the named one accepts, the other refuses), the four unproven routes built (`closeHost` merge,
a merge inside an order's take, a fill sharing a hand-over part, one vouch over two underwriters: all hold), two donors,
two long orders, twins of one pair id and of one order id, a stale maker, the contest's maker, the maker with the fresh
scan, the spec table of every pair of roles. `audit7_tooling` (`holds_deposit_refuses_a_dispute_its_trace_does_not_reach`:
the forged dispute of the review in every arrangement the forger controls), `oracle_builders` (a real claim and dispute
traced before the stake is committed), `oracle_cli` (contest and deposit refused without a node or flag; traced from the
tool's own transaction files; a wrong opening refused), `ocmds::tests::the_trace_gate_of_contest_and_deposit`,
`audit7_book_tests` (12, 25.4), and the seeded run (25.3). Tests whose premise changed: `oracle_tests::layout_state_at_byte_one_and_lengths`
(the new offsets), `oracle_tests::a_restored_part_comes_back_unlisted_and_bound` (makers instead of tags),
`oracle_attack_tests::holds_an_order_enters_a_pair_only_with_its_cover_counted` (the part's makers),
`audit6_tests` (the take's `pm`, the donor's `hm`, makers instead of tags), `oracle_metrics_tests` (the pending parts'
maker), the 41 changed golden vectors of `oracle_builders` (5 unchanged: slot, stake and order creation), and the
pinned artifacts.

### 25.8 Numbers after review 7

Script bytes: `KpfOpenO` 6,204 (was 6,027), `KpfPairO` 14,613 (14,222; state 723, was 624), `KpfPendingO` 4,395 (4,298;
state 813, was 780), `KpfBond` 4,453 and `KpfStake` 9,652 (unchanged). Largest stack 197 of 244 (the take; was 180),
the pair's largest 178 (propose). The pinned artifacts and the golden vectors were regenerated
(`CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`, `KPF_GOLDEN=print`); the book market's artifacts
are byte for byte as before. The whole workspace: 600 tests.

| Transaction | Script units per input | Compute mass | Bytes | Fee (KAS), was |
|---|---|---|---|---|
| open (2 orders, slot bind, funder) | 286,243 / 286,393 / 169,945 / 100,001 | 56,322 | 45,922 | 0.0918, 0.0900 |
| propose a liquidation / a close | 631,388 / 424,313 (pair) | 27,852 / 25,752 | 18,822 | 0.0376, 0.0367 |
| take by an order with the slot's bind | 259,071 / 287,156 / 169,945 / 100,001 | 50,065 | 40,335 | 0.0807, 0.0789 |
| transfer to a signing taker | 258,925 / 100,001 | 19,753 | 15,163 | 0.0303, 0.0295 |
| amend / cooperative close | 324,512 / 230,319 | 20,032 / 19,557 | 15,102 / 15,177 | 0.0302 / 0.0304, 0.0294 / 0.0296 |
| lapse settle / mutual close | 30,183 / 30,818 | 17,425 / 17,435 | 15,045 / 15,055 | 0.0301 / 0.0301, 0.0293 / 0.0293 |
| renew 1 / 4 / 7 pairs | each pair 300,690 to 300,724 | 27,063 / 81,465 / 135,867 | 19,903 / 64,195 / 108,487 | 0.0398 / 0.1284 / 0.2170 |
| contest / deposit 1 / resolve and decide / settle 2 receipts | 186,347 / 298,671 / 128,357 / 349,197 (first input) | 22,079 / 27,188 / 19,629 / 36,884 | 14,809 / 19,988 / 14,629 / 29,584 | 0.0296 / 0.0400 / 0.0293 / 0.0592 |
| 7 proposals vouched by one input | 631,820 to 631,846 each | 178,488 | 129,438 | 0.2589, 0.2525 |

One pair-month (open, one renewal, a close proposed and finalized): **0.1789 KAS** of network fees (was 0.1752).

## 26. Changes after review 8

Review 8 (`docs/audit/REVIEW-8.md`) found one Critical and one Low finding. R8-F1: `KpfPairO.closeDonor` took P3's cover
rate, expiry and premium as arguments with one-sided bounds (`x3 <= X`, `pr3 <= premium`, `cr3 >= range`), and since
review 6 a donor accepts any host or none. The donor's short alone (or anyone who bought its listing) moved the passive
long into a P3 whose cover had already lapsed, so anyone settled it at once at the entry price and the long lost its
gain over the entry; or into a P3 no lapse settle could reach (an expiry of `-4 * 10^18`), or one whose cover rate made
every claim and renewal unaffordable. R8-F2: `KpfStake.close(j)` named its burn output by argument, so two pools closed
together shared one burn and the closer kept the other. Both are fixed. This is the third review in a row whose
critical was opened or exposed by the previous fix, so the change is kept to the two rules below.

### 26.1 The rule

**A party entry never writes a binding.** Every pair state a price-free entry writes (a continuation, a remainder, the
part an entrant gets, P3) carries the spent pair's binding (`uwId`, `uwKey`, `coverRate`, `expiry`, `premium`, state
`[411..504)`) byte for byte; only the underwriter's renewal moves the expiry (by exactly one term, proven by its slot's
bind), and only an opening or a take writes a binding afresh, each proven by the slot's bind in the same transaction and
bounded by the order's own terms. In `closeDonor` the arguments `cr3`, `x3`, `pr3` are gone and P3's binding is
`rs.slice(BIND_AT, LISTS_AT)`, as `transfer` and `closeHost` already wrote theirs; `bs3 - bl <= coverRate` stays (the
range rule). The entry is now `closeDonor(n, x, e3, c, bs3, sk3, sa3, sm3, st3, hm)`.

**R8-F2: the burn at the mirrored slot.** `KpfStake.close(j)` requires `j == M - 1 - self` and `j >= inputs`, the rule
every second payout of the family follows (`mutualClose`, `lapseSettle`, a refuted claim's disputer, a short's close
payout, and `KpfLock`'s burn in the book family). Two inputs never name one such slot, and the slot is never an input's
own index, where another input's first payout sits. The argument is kept (the entry's interface and dispatch tag are
unchanged) and must equal it. The tool's `pool_close` already put the burn there (one input, the burn at output 1).

### 26.2 Every entry that writes a pair state, field by field

What each entry point that writes a `KpfPairO` state (or a `KpfPendingO` state that becomes one) takes for the binding
of the written state, and who is bound by an argument.

| Entry | Writes | Binding of the written state | Other fields from arguments, and who they bind |
|---|---|---|---|
| `KpfPairO.amend` | continuation | spent state (`contSpk`) | the signer's own listing |
| `KpfPairO.addMargin` | continuation | spent state | the signer's own bound (outward only, paid for) |
| `KpfPairO.transfer` (`handOver`) | the entrant's part, the remainder | spent state (`rs.slice(BIND_AT, LISTS_AT)`; `restSpk`) | the entrant's key, bound `nb` and maker: the entrant (a signing taker, or an order that checks them in `take`); the stayer's key, bound, entry and listing are copied |
| `KpfPairO.closeHost` | P3, the remainder | spent (host) state | the entering long's key, bound `bl3 <= bl`, entry `e3 <= E`, listing, `dm`: the donor's side, which the donor checks against its own state |
| `KpfPairO.closeDonor` | P3, the remainder | **arguments `cr3`, `x3`, `pr3` before review 8 (R8-F1, unsafe: the passive long is bound by them); spent (donor) state since** | the entering short's key, bound `bs3 >= bs`, entry `e3 >= E`, listing, `hm`: the host's side |
| `KpfPairO.mutualClose` | the remainder | spent state | none |
| `KpfPairO.renew` | continuation | spent state, the expiry moved to `X + term` (computed, proven by the slot's bind) | none (`u`, `k` are indices the bind proof ties to this input) |
| `KpfPairO.propose` | the pending part; its restore, cure and forfeit states (hashes); the remainder | spent state (`pairHash`; the pending state copies `[ENTRY_AT, LISTS_END)`) | `kind, side, p, t0` vouched by `uwKey`'s SIGHASH_ALL input (a close also signed by the closer); `fb` and `n` bounded as KpfPair |
| `KpfPairO.lapseSettle`, `coopClose` | no pair state (`coopClose`: any outputs, both owners sign) | none | none |
| `KpfPendingO.contest`, `decide` | the pending continuation | spent state | none |
| `KpfPendingO.cure`, `finalize` | the cure, restore or forfeit state | the hash `propose` fixed (the spent pair's binding) | none |
| `KpfOpenO.fill` | a new pair | arguments `coverRate`, `xe`, `premium`; `uwId`, `uwKey` from the slot's bind proof | binds the two orders, each within its own terms: `uwAccept`, `premium <= premMax`, `range <= coverRate <= priceMax`, `xe >= the order's DAA + term`, the slot counting `q(n, coverRate)` at `xe` |
| `KpfOpenO.take` | the part an order enters | arguments, proven by the slot's bind in the same transaction | with a pair (its `transfer` beside it): equal to the pair's binding, which the pair copies; without a pair: binds only the order, within its terms (review 8 note 1) |

Result: `closeDonor` was the only entry that took binding fields from arguments for a state a non-signing party is
moved into. `fill` and `take` take them from arguments by design, but the party bound by them is the order whose own
terms bound them and whose cover the slot counts in that transaction; no staying party of an existing pair is moved
into a binding it did not have. `take` without a pair accepts an expiry with no lower bound (review 8 note 1: the order
gets at least its margin back at the lapse settle and loses its tip; not fixed, bounded by the order's terms) and, like
`fill`, a cover rate up to `priceMax` (no claim on the pair is then affordable and no slot can count a renewal: the
pair lapses at its expiry and settles at the order's own price; the slot counts the inflated cover, so the underwriter
pays for it in capacity). Neither moves a party that did not consent through its terms.

**Output indices given as arguments (R8-F2's pattern), every entry of the five contracts.** `KpfPairO`: `transfer`,
`closeHost`, `closeDonor` (`c`) and `propose` (`k`) name a fresh output, which `fresh` and the maker field tie to the
input; `propose`'s `j` and `renew`'s `u` are inputs checked by script and (for `u`) by a bind ref equal to this input;
every payout is at `self` or `M - 1 - self`. `KpfOpenO`: `fill`'s `pairOut` and `take`'s `partOut` are fresh outputs with
makers, `u`, `k` a bind ref `OUT_REF + out`; the refund at `self`. `KpfPendingO`: `contest`'s `u` is a stake whose `open`
names this input, `dOut` an output of the claim's id (three counted); `decide`'s `d` an input of the id whose `resolve`
names this input; payouts at `self`, `M - 1 - self`, continuations by id. `KpfStake`: `open`'s `cOut`, `dOut` outputs of
the claim's id (three counted by both the stake and the claim); `deposit`, `settle` input ranges, receipts at their own
index, counted by id; `resolve`, `redeem` inputs of the id; `close`'s `j` **the one free output index (R8-F2), fixed**.
`KpfBond`: every output by its own id; `withdraw` wherever the owner's SIGHASH_ALL signature says.

### 26.3 What stops working

- **Close-versus-close joins only two pairs with byte-identical bindings**: one slot (`uwId`, `uwKey`), one cover
  rate, one expiry, one premium. Two pairs of one underwriter opened in different expiry buckets, at different cover
  rates or premiums, or one renewed and the other not, can no longer close against each other (before, the donor's long
  moved to the host's cover, which could be shorter: the harm of R8-F1 in a milder form). Every other exit of section 17
  remains. No command builds close-versus-close (TESTNET.md 9.8), so no command changes.
- **A pool's burn at any other index** is refused. The tool's own `pool_close` is unchanged (its transaction id changes
  only because the templates did).

### 26.4 Tooling (live run 1)

- `oracle_build::propose`: for a liquidation the underwriter funds `funded_by_proposer` = pending part + remainder - the
  pair's value, which is the bond, plus the carrier of the new part when `n < s` (the pending part's `refund`, which a
  cure pays back to the underwriter; review 9, R9-N1: a valid settlement does not, it leaves the carrier in the
  forfeited part, section 27.2). The vouch input funds it and the rest goes back to the
  underwriter; the funder pays only the fee. Live run 1 had the funder pay a 0.2 KAS carrier that the cure then
  refunded, with the bond, to the underwriter (1.2 KAS). A close is unchanged (the closer funds it).
- Relative locks: `kpf`'s oracle commands share one `finish`, which now computes the first DAA at which consensus accepts
  the transaction (`usable_from`: an input with a relative lock `r` on a UTXO at DAA `d` from `d + r`, a DAA lock time
  `t` from `t + 1`, as rusty-kaspa's `check_sequence_lock` and `check_tx_is_finalized`) and prints it; with `--node`
  a submission before it is refused locally ("usable from DAA X (now Y, about Z s)"), and a dry run says it is not yet
  usable. This covers every command with a lock: open (the slot's rate limit), the price-free pair entries
  (`quietDaa`), finalize (`W`), resolve, renew and lapse settle (lock times), the slot's unbond and withdraw, the
  stake's withdraw.
- `oracle_build::transfer_bounds`: `oracle-transfer` checks the listing, the no-downgrade rule, health at `x` and the
  cover rate before it builds, and a refusal names the rule and the allowed range of `--nb`.

### 26.5 What a reviewer should attack next

1. **Headroom an entrant takes from a stayer.** Every hand-over (transfer, closeHost, closeDonor) lets the entrant post
   any bound that keeps the range within `coverRate`. An entrant that takes all the headroom (`bs3 = bl + coverRate`)
   leaves the stayer no `addMargin` and no cure (a cure is offered only within `coverRate`). This is not a binding
   field and was not changed; whether it lets a listed side strip its counterparty's defence against a liquidation is
   open.
2. **The identical-binding join.** The host and the donor each write their own binding into one P3; P3's cover is then
   counted by the slot in both pairs' original counts until the expiry. Check that no shape lets a host and a donor
   with different bindings both accept (all of `[411..504)` is in both scripts' equality) and that the double count
   cannot be used to free capacity.
3. **The relative-lock gate is local only.** `usable_from` trusts the UTXO DAA scores the command loaded (read from the
   node with `--node`); without `--node` the line is information only. It does not change what consensus accepts.
4. Review 8 note 1 (a take without a pair into a past expiry) remains as stated in 26.2.

### 26.6 Tests and numbers

`audit8_tests` (11, all `r8_holds_`): each of the five former exploits builds the same transaction and the donor refuses
it (a past, a later, a far-past and a maximal expiry; a cover rate of `priceMax`, one above and one below the donor's,
and equal to the range; a premium one above and one below); the control with the donor's binding keeps the long's
lapse date, a valid close and a valid liquidation, and renews; `closeHost` refuses another expiry, cover rate or
premium; `transfer` refuses each binding field changed in the part, and a partial `closeDonor` a remainder with another
premium; two pools sharing one burn are refused, each burn at its own `M - 1 - i` is accepted and burns both.
`oracle_tests` (close-versus-close refuses a donor whose expiry is later, premium higher or cover rate lower than the
host's: the directions accepted before), `oracle_builders` (a partial liquidation's vouch funds bond + carrier and the
funder pays only the fee; exact fits; the refusal names the sum), `oracle_cli` (`oracle-transfer --nb` above `bl`
refused with the rule and the range; the offline "usable from" line), and
`ocmds::tests::relative_locks_are_compared_with_the_node_before_submitting`. Kits whose premise changed:
`oraclekit::close_vs_close_tx`, `audit6_tests`, `audit7_tests` (no binding arguments), `oraclekit::pool_close_tx` and
`judge::close_tx` (the burn at `M - 1 - 0`), and the golden vectors.

Script bytes: `KpfPairO` 14,509 (was 14,613: the three arguments and their checks are gone), `KpfStake` 9,665 (was
9,652: two checks), `KpfOpenO` 6,204 and `KpfPendingO` 4,395 (byte counts unchanged; their pinned template hashes
changed with the templates they pin), `KpfBond` 4,453 (unchanged). The stake's dispatch tags (`open`, `resolve`) and
the slot's `bind` tag are unchanged. Largest stack 197 of 244 (the take, unchanged); the pair's largest 178 (propose).
Every shape of `oracle_metrics_tests` still fits a block, 7 proposals under one vouch and 7 renewals under one bind
included; one pair-month costs 0.1781 KAS of network fees (was 0.1789). The pinned artifacts and 44 of the 46 golden
vectors were regenerated (`CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`, `KPF_GOLDEN=print`; slot
and stake creation unchanged); the book market's artifacts are byte for byte as before.

| Transaction | Script units (first input) | Compute mass | Bytes | Fee (KAS), was |
|---|---|---|---|---|
| open (2 orders, slot bind, funder) | 285,307 | 56,114 | 45,714 | 0.0914, 0.0918 |
| propose a liquidation / a close | 627,436 / 422,441 | 27,648 / 25,648 | 18,718 | 0.0374, 0.0376 |
| transfer to a signing taker | 257,157 | 19,649 | 15,059 | 0.0301, 0.0303 |
| close the pool (the burn at `M - 1 - 0`) | 19,849 | 12,297 | 10,107 | 0.0202 |
| 7 proposals vouched by one input | 627,868 to 627,894 each | 177,060 | 128,710 | 0.2574, 0.2589 |

## 27. Changes after review 9

Review 9 (`docs/audit/REVIEW-9.md`) found one Critical, two Medium findings and a note. R9-F1: `KpfPairO.renew` never
related the expiry to the present, so a pair whose expiry lay `k` terms in the past (a `take` without a pair or a `fill`
of an aged order written into the past, or a silent underwriter) was renewed `k` times at one DAA, one premium each,
until its margins were gone (2,000.2 KAS of an order's margin on the review's fixture). R9-F2: the same on an honest
pair: a late double renewal charging a term already over, and a renewal after the lapse settle was due postponing it
term after term. R9-F3: the headroom between the range and the cover rate was one pool both sides drew on, so one side
(or an entrant) took it and left the other no top-up and no cure. R9-N1: the documentation said a valid partial
liquidation refunds the underwriter's carrier; it does not. This is the fourth review in a row with a critical finding,
three of them opened or exposed by the previous fix, so each change below is kept to one rule and states what it could
break.

### 27.1 The underlying fact

A covenant proves only lower bounds on the present: a lock time `L` (CLTV) proves `now >= L`, a relative lock (CSV)
proves `now >= the UTXO's DAA + age`, and the DAA score of a UTXO it spends (`OpTxInputDaaScore`) is exact but dates
that UTXO's acceptance, not the present. Nothing proves "now is before X". The comment on `renew` ("the slot's window
allows a renewal only from one step before the expiry, so the premium is paid at most once per term of calendar time")
read the slot's window `x2 <= t + term + step`, `tx.daa >= t`, as if `t` were the present; it is only a lower bound, so
the window bounds the renewal from below (`now >= X - step`) and never from above. That is the root of R9-F1 and R9-F2.

### 27.2 The rules

**R9-F1, R9-F2: a renewal spends a pair UTXO accepted no later than its expiry.** `KpfPairO.renew` requires
`0 <= OpTxInputDaaScore(self) <= expiry`. The renewed continuation is accepted at the present, so:

- a part or pair written after its expiry (a `take` or `fill` into the past, a hand-over, close-versus-close, a restore,
  cure or forfeit, or a remainder written after `X`) is never renewed: it lapses and settles at its entry;
- after a renewal of `X` landing at `D'`, the next one needs `D' <= X + term`, and the slot's window needs
  `now >= X + term - step`; so two renewals at one DAA happen only when the first lands in `[X + term - step, X + term]`,
  and a third needs a term more of calendar time;
- any party entry accepted after `X` (an amend is enough, its fee the only cost) makes the pair unrenewable, and its
  lapse date `X + tail + idleDaa` is then fixed.

What it could break: an underwriter can no longer revive a pair a party touched after its expiry, nor any pair state
written after it; those pairs settle at their entry. An unaccepted parent (DAA score unknown in the mempool) is refused
(`>= 0`), as `fill` and `contest` already do. No other entry reads the new check.

**R9-F3: each side's share of the cover rate, measured from the entry.** The short's share is `floor(coverRate / 2)`
above the entry, the long's the rest below it: `shortReach() = max(bs, entry + floor(coverRate / 2))`,
`longReach() = min(bl, entry - (coverRate - floor(coverRate / 2)))`, the entry clamped to `[0, priceMax]` for this
(`shareAnchor()`: a take without a pair may write any entry in its order's favour, and an unclamped one near the 64-bit
edge would make every share check overflow and fail, a liquidation's included). A price-free entry may move a side's bound outward
only within the other side's reach: the long's new bound `nbl` needs `shortReach() - nbl <= coverRate`, the short's
`nbs - longReach() <= coverRate`, besides the range rule. Applied in `addMargin` (the signer's own bound), `handOver`
(`transfer`, and `take` with a pair through it: the entrant's bound, unless it keeps the leaver's), `closeHost` (P3's
entering long `bl3`, unless `bl3 == bl`) and `closeDonor` (P3's entering short `bs3`, unless `bs3 == bs`); and to the
cure `propose` offers: the cured bound is measured against the winner's reach, not its bound. Close-versus-close keeps
the entry: `e3 == entryPrice` in both `closeHost` and `closeDonor` (KpfPair: `e3 <= the host's E`, `e3 >= the donor's
E`), because the shares are measured from it and an entrant that moved it shifted the passive side's share. No
price-free entry now writes the entry or the binding; only `fill` and `take` without a pair (new lineages, bound by the
orders' terms) set them.

What it gives: whatever the other side or an entrant does, a side can add margin up to the end of its own share, and a
true liquidation of it offers a cure as long as the cure bound lies within its share (and `priceMax`); renewals move
both bounds inward and only enlarge both rooms. What it costs:

- a side's bound reaches at most half the cover rate from the entry: on the fixture pair (entry 10, cover rate 6) the
  long down to 7, the short up to 13; before, either could take the whole 3 KAS of headroom (the long to 6, the short to
  15);
- a cure reaches only the loser's share: on the untouched fixture pair a liquidation of the long at 6.98 (cure bound
  6.498) offered a cure before (`12 - 6.498 <= 6`) and offers none now (`13 - 6.498 > 6`); the short's likewise at
  12.40 (cure bound 13.2804, `13.2804 - 7 > 6`);
- a side opened beyond its half (asymmetric rates with little headroom, e.g. rates 1 and 5 under a cover rate of 8)
  keeps its bound but can never widen it or be cured beyond its half; the other side keeps the headroom;
- close-versus-close joins only two pairs with byte-identical bindings and one entry price; no command builds it.

**R9-N1 (documentation only).** The underwriter funds the carrier of the new part of every partial liquidation it
proposes (`refund = B + carrier`) and gets it back only on a cure (`cure` pays `refund`). A valid settlement pays it
`B + fee` and the carrier stays in the forfeited part (the winner's); a void claim restores the part with the carrier in
it and pays `B` to the defender; a refuted one restores it with the carrier and pays `B` to the defender and the
disputer. A close's closer gets its `refund` (with the carrier) on a valid settlement and loses `B` on a void or refuted
one, the carrier staying in the restored part. Corrected in `oracle_build::propose`'s and `funded_by_proposer`'s
comments, the `kpf vouch` text, TESTNET.md 9.5 and 26.4 (whose "which a cure or a valid settlement pays back" was wrong for the
valid settlement).

**Not changed: `fill` and `take` refusing a provably past expiry.** A lower bound by the order's own DAA score (or a
term after it, as `fill` has) cannot shrink what an aged order loses: the attacker picks the expiry just after that DAA.
Since the renewal rule, a past expiry costs an order only what it put in beyond its margin (27.4), so `KpfOpenO`'s
source is unchanged (its template hash changed only because it pins `KpfPairO`'s).

### 27.3 Lock times and DAA scores, every entry point

Every place an oracle-family entry reads time, what it proves, and whether something relied on it as the present or as
an upper bound.

| # | Entry | Reads | Proves | Verdict |
|---|---|---|---|---|
| 1 | `KpfPairO` party entries (`amend`, `addMargin`, `transfer`, `closeHost`, `closeDonor`, `mutualClose`) | CSV `quietDaa` | the UTXO is at least that old | safe: a rate limit, used only as "at least" |
| 2 | `KpfPairO.lapseSettle` | CLTV `X + tail + idleDaa` | the settle is due | the bound itself is safe; the claim "nobody can postpone it" relied on `X` not moving after the lapse, which row 3 allowed: **fixed** with row 3 (residual bounded, 27.4) |
| 3 | `KpfPairO.renew` | the slot's window (`x2 <= t + term + step`, CLTV `t`) | `now >= X - step` | **unsafe** (read as "now is before X"): **fixed**, `OpTxInputDaaScore(self) <= expiry` |
| 4 | `KpfPairO.propose` | CLTV `t0` | `t0 <= now`: no claim prices the future | safe |
| 5 | `KpfPairO.propose` | nothing against `X` | validity is judged at finalize from the exact `P` | safe |
| 6 | `KpfPendingO.contest` | `OpTxInputDaaScore(self)` = `P` | the claim's acceptance, exact | safe |
| 7 | `KpfPendingO.decide` | `OpTxInputDaaScore(self)` against `P + W` | the contest's acceptance, exact: a deadline after the fact | safe |
| 8 | `KpfPendingO.finalize` | CSV `W` (pending), `OpTxInputDaaScore(self)` | the window has passed; `P` exact for validity | safe |
| 9 | `KpfPendingO.cure` | nothing | a cure may land any time while pending | **unsafe** for a claim pending at `X` (R10-F2: the cured pair, accepted after `X`, is never renewed); **bounded** since review 10 (a valid claim is finalizable a step before `X`, 28.3) |
| 10 | `KpfOpenO.fill` | CLTV `activeFrom` | the order is active | safe |
| 11 | `KpfOpenO.fill` | `xe >= OpTxInputDaaScore(self) + term` | a term after the order's acceptance, not after the present | relied on as "the order gets a term of cover": **unsafe** for an order older than a term; with row 3 the pair is never renewed and the order loses only its tip, carrier and premium half (27.4) |
| 12 | `KpfOpenO.take` | nothing on `xe` (the slot's window bounds it from above only) | nothing about the present | **unsafe** together with row 3 (the drain of R9-F1): **fixed** by row 3; residual tip and carrier (27.4) |
| 13 | `KpfOpenO.refund` | CLTV `min(expiryDaa, DAA + 90 days)` | the order may be refunded | safe; `expiryDaa` does not stop a fill after it (nothing can), and the tools say "refundable after" |
| 14 | `KpfBond.bind` | CLTV `t`, `x <= t + term + step` | `x <= now + term + step`: no expiry runs ahead of the present by more than that | safe as an upper bound on `x`; its comment "so a pair renews in the last step before its expiry" holds as a lower bound only (source unchanged; row 3 is where it was relied on) |
| 15 | `KpfBond.bind`, `split`, `unbond` | CSV `rateDaa` | owner rate limit | safe |
| 16 | `KpfBond.advance`, `unbond` | CLTV `(j0 + d) * step + tail` | the dropped buckets' liability has ended | safe |
| 17 | `KpfBond.withdraw` | CSV `unbondDaa` | the unbonding delay has passed | safe |
| 18 | `KpfStake.open` | `own DAA + ageDaa <= anchor` (both exact) | the stake was bonded before the claim | safe |
| 19 | `KpfStake.deposit` | `dd + ageDaa <= anchor` (exact); `start` from its own DAA | the same; the round's start exact | safe |
| 20 | `KpfStake.resolve` | CLTV `start + roundDaa` | the round has passed; a deposit landing before the resolution still counts (its comment says so) | safe |
| 21 | `KpfStake.withdraw` | CSV `unbondDaa` | the unbonding delay has passed | safe |

Twenty-one places; four relied on a lower bound as the present or as an upper bound (rows 2, 3, 11, 12), all through the
renewal: row 3 is fixed, and rows 2, 11 and 12 now leave the bounded residuals of 27.4.

### 27.4 What remains, with numbers

The fixture: 1,000 units, premium 0.02 KAS a term (20 KAS a renewal, 10 from each side), term 800, step 100, tail 200,
idle 1,000.

- **A late renewal still back-charges.** An underwriter silent past `X` can renew once from the untouched pair (its UTXO
  older than `X`), buying the term `[X, X + term]` partly or wholly passed: one premium, 20 KAS. If that renewal lands
  in `[X + term - step, X + term]` = `[1,900, 2,000]` a second follows at once: 40 KAS at one DAA, the first for a term
  already over. The pair then stands exactly where an on-time underwriter (renewing at 1,150 and 1,950) leaves it: a
  lineage opened at `D` has paid at most `1 + (now + step - D) / term` premiums, the most an on-time underwriter
  charges by then (`r9_bounded_a_late_renewal_charges_no_more_than_the_on_time_schedule`; the walk checks the bound
  after every renewal). What the parties lose during the silence is the cover (no claim is valid after `X - W`), not
  extra premiums.
- **A due lapse settle can be overtaken once.** At 2,500 (the settle was due at 2,400) one renewal still lands (it races
  the settle; the miner picks) and moves the date to 3,200: postponed by one term, at one premium (20 KAS) for a cover
  already over; its continuation, accepted after its new expiry, is never renewed again
  (`r9_bounded_a_renewal_after_the_lapse_settle_is_due_postpones_it_once_by_one_term`). This holds when
  `tail + idleDaa > term` (the fixture: 1,200 > 800). **The testnet defaults do not meet it** (tail 3.25 days + idle
  7 days < term 28 days): there a renewal landing between the due settle and `X + term` resumes the cover (claims are
  valid again until `X + term - W`) and the pair stays renewable, so an underwriter siding with one party can race the
  due settle once per term, at the agreed premium each time. The parties' defence is any entry after the expiry (it
  fixes the date); an underwriter that renews on time cannot be stopped, by design.
- **A take into the past** (no pair) costs the order its tip and the part's carrier (per part, which review 10 showed
  is not a bound per order: R10-F1, the whole-order bound in 28.4): 20.2 KAS for 2,000 units at a tip
  of 0.01 KAS (`r9_bounded_a_take_into_the_past_costs_the_order_only_its_tip_and_the_carrier`); a take pays no premium.
  Before: 2,000.2 KAS, the whole margin.
- **A fill of aged orders into the past** costs each order its tip and its half of the first premium, and the long's
  order the pair's carrier (the short gets it at the lapse settle): 50.2 and 50 KAS at the fixture's maximal premium
  0.10 KAS, 100 KAS to the underwriter for a cover that had lapsed
  (`r9_bounded_a_fill_of_aged_orders_into_the_past_costs_each_its_tip_and_half_the_first_premium`). Bound per order:
  `q(n, tip + premMax / 2) + carrier`; an order filled within a term of its acceptance is not exposed. Before: 1,900 KAS
  more by 19 renewals.
- **A colluding outsider** adds nothing to these: the taken or filled part cannot be renewed by anyone.
- **R9-F3:** nothing an entrant or the other side of an existing pair does removes room from a side's share. One path
  remains where the counterparty sets the shares: a `take` without a pair writes a new lineage whose entry the
  counterparty chooses within the order's terms (a long order: any `e >= x`). An entry above `x` moves the order's share
  up by `e - x`: at `e` = 12.20 under a cover rate of 6 the long order (bound 9) lies beyond its share (floor 9.20) and
  can neither add margin nor be cured, while the short may grow to 15
  (`r9_bounded_a_take_without_a_pair_sets_the_shares_by_the_entry_it_writes`). The counterparty pays for it only at a
  lapse settle (the long is paid at the higher entry: there the whole range, 6,400 KAS for 2,000 units), which a
  renewing underwriter on its side defers; what the order can lose is the cure of a true liquidation, at most its
  margin. Not closed here: `KpfOpenO.take` cannot tell a take with a pair from one without; a later change could let an
  order bound the entry (`e <= x + offset`) or refuse takes without a pair.
- What a side otherwise loses is only what the share rule itself gives up (27.2).

### 27.5 Tooling

- `oracle-order` requires `--slot SLOT_ID` (or the old `--underwriter`) or `--any-slot`; with `--any-slot` it prints
  what any underwriter may then choose (cover rate, expiry, premium up to `--prem-max`), and for every order what a fill
  after more than a term can cost (tip, carrier, premium half: the most, in KAS).
- `renew` refuses locally a pair whose UTXO was accepted after its expiry (`OPair::unrenewable_why`: the DAA, the
  expiry, and the DAA from which anyone can settle it).
- `oracle-market` / `oracle-check` text: the late renewal and the fixed date after a party entry replace "nobody can
  postpone"; each side's half of the cover rate is stated. `oracle-transfer`'s bounds include the stayer's share
  (`transfer_bounds`, `OPair::growth_limit`).
- **Storage mass (live run 2).** A renewal of a 1-unit pair failed: "mass exceeds block limits: storage: 510287". KIP-9
  charges `10^12 / value` grams per output, so its 0.02 KAS premium output alone was the whole block limit (500,000).
  Every oracle builder now measures the draft's storage mass before signing and again after (`storage_refusal`) and
  refuses with "output N (LABEL, V KAS) is too small to be mined: at least X KAS". The premium of an open or a renewal
  (which no contract fixes to an output: the slot owner's signed bind decides where it goes) joins the funder's change
  when the funder is the underwriter, or one output with an underwriter UTXO (`--uw-utxo`; `open_with`, `renew_with`),
  and is otherwise refused with that advice when too small. A liquidation's vouch remainder below the viable size joins
  the change when the funder is the underwriter, and is refused otherwise. A payout a contract fixes to an output of its
  own is topped up by the funder to `VIABLE_OUTPUT_MIN` (0.025 KAS) when it is smaller (the contracts bound every
  payout from below only), and so is a pool's burn by the closer's base. The wallet text warns when a pair's margins,
  bond, bond shares or premium fall below that size (`payout_warnings`).

Payouts a contract fixes to an output of its own (none can be folded), each at least 0.02 KAS (`PLAIN_DUST`; smaller
ones go to the other party or stay in the pair): the leaver of `transfer`, `closeHost`, `closeDonor` (at `self`); the
long and the short of `mutualClose` and `lapseSettle` (`self`, `M - 1 - self`); a close's closer and other party; the
underwriter's `B + fee` of a valid liquidation and its `refund` at a cure (`self`); the defender's and the disputer's
shares of a void or refuted bond (`self`, `M - 1 - self`); an exhausted order's refund (`self`, any value, even 0);
the pool's burn (`M - 1 - self`, at least 0.02 KAS); a winning receipt's free stake (at least `minStake`). An output's
own storage mass is `10^12 / value`, so beside a few other outputs of a few KAS the least minable value is about
0.0202 KAS, and 0.02 KAS exactly never fits beside another output unless the inputs' term offsets it (several small
funding inputs do). No payout is bounded from above, so every honest path stays minable by the funder's top-up (at
most 0.005 KAS to the payee): a cost, not a freeze. Contract-level note (not a fix): the contracts' dust floor equals the
block's storage limit for one output; a later template change should raise `PLAIN_DUST` (for example to 0.05 KAS) so
that no contract-fixed payout needs a top-up. No contract was changed for it.

### 27.6 What a reviewer should attack next

1. **The entry as the shares' anchor.** Every rule of R9-F3 assumes no price-free path writes `entryPrice`, and a take
   without a pair lets the counterparty choose it (27.4): is that the only path, and is its bound right? Check every
   state a pair can become (the pending part's restore, cure and forfeit hashes, the remainder of a split, P3, the part
   of a take with a pair), the rounding of an odd cover rate (`floor` for the short, the rest for the long), and whether
   a cure bound at a share's end, then a hand-over keeping the leaver's bound (`nb == bl`), lets a side reach past its
   share.
2. **The renewal's exact date.** `born <= expiry` trusts the pair UTXO's DAA score: check chained renewals in one block
   and in the mempool (an unaccepted parent is refused), the double renewal of `[X + term - step, X + term]`, and markets
   with `tail + idleDaa <= term` (the testnet defaults), where a late renewal after the due settle resumes the cover: can
   an underwriter siding with one party, racing the settle each term, keep a party in longer or more cheaply than 27.4
   says?
3. **The tools' storage-mass handling.** The top-up of contract-fixed payouts changes what the builders pay (the payee
   gets up to 0.005 KAS more than the contract owes; check no builder now underfunds its change or double counts it), the
   premium folded into the underwriter's change (an executor-funded renewal of a small pair is now refused), and
   `storage_refusal`'s estimate of the least value (linear in `1 / value`, exact while the input term does not depend on
   the outputs).

### 27.7 Tests and numbers

`audit9_tests` (23: 16 `r9_holds_`, 7 `r9_bounded_`, no `r9_exploit_`): every former exploit builds the same
transactions (the take into the past and its renewals; the fill of aged orders and its renewals; the late double
renewal at 2,300; the renewal after the due settle; the short's top-up to the cover rate and the entrant's bound
`bl + coverRate`) and is refused, or its residual is measured and bounded. New: a party entry after the expiry fixes the
lapse date (control: an amend before it does not stop an on-time renewal); each side's share against `addMargin`,
`transfer` (both sides), `take` with a pair, `closeDonor` and `closeHost` without a partner (the entry moved by one sompi
refused); a side beyond its share; the long's and the short's cure within their shares; the carrier of a partial
liquidation per outcome. The seeded walk (6 seeds x 200 steps, and 4 x 120 with the hostile take) now also runs takes by
an order with a pair, claims on half a pair (the remainder continuing the lineage), contests won by a colluding YES stake
(the pool settling a losing receipt), late contests (the verdict ignored) and upheld void claims; it compares the pair's
verdict of every renewal with the new rule and every top-up, hand-over and close-versus-close with the share rule,
checks every cure against the loser's share and every lineage against the calendar bound, and records no violation (585
refusals as ruled, 589 hostile variants refused; 299 renewals, 129 refused as touched after their expiry).
`oracle_attack_tests::exploit_a_back_dated_renewal_charges_one_premium_for_a_lapsed_term` (the second renewal now
refused). `oracle_builders::a_one_unit_pair_with_the_testnet_defaults_fits_the_mass_limits_on_every_path` (open,
liquidation, cure, renewal, liquidation, forfeit and lapse settle of a 1-unit pair; the executor-funded open and renewal
refused with the output named; the `--uw-utxo` form accepted). `oracle_cli` orders pass `--any-slot`.

Script bytes: `KpfPairO` 15,042 (was 14,509: the renewal's date check, the clamped anchor and two reach functions, inlined
at each of the five share checks),
`KpfOpenO` 6,204, `KpfPendingO` 4,395, `KpfBond` 4,453, `KpfStake` 9,665 (byte counts unchanged; `KpfOpenO`'s template
hash changed because it pins `KpfPairO`'s). Dispatch tags unchanged. Largest stack 197 of 244 (the take, unchanged); the
pair's largest 180 (propose, was 178). Every shape of `oracle_metrics_tests` still fits a block, 7 proposals under one
vouch and 7 renewals under one bind included. Fees (100 sompi per gram): open 0.0936 (was 0.0914), propose 0.0385 (was
0.0374), transfer 0.0312 (was 0.0301), a renewal of one pair 0.0407, 7 proposals under one vouch 0.2649 (was 0.2574):
the pair's template travels in an open's arguments and a pair input's signature script. The pinned artifacts and 41 of
the 46 golden vectors were regenerated (`CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`,
`KPF_GOLDEN=print`); the book market's artifacts are byte for byte as before.

## 28. Changes after review 10

Review 10 (`docs/audit/REVIEW-10.md`) found one Critical, one Medium and two notes. R10-F1: every take (and fill) of a
long order took another carrier from it, which the pair pays to its short at the end, and a part whose margin lay below
the 0.02 KAS payout floor was lost whole at the lapse settle; an outsider with a slot of its own took a resting order in
minimum parts and kept almost its whole escrow. R10-F2: a valid liquidation could still be pending at the expiry, where
no renewal reads it; the loser's cure landing after the expiry wrote a pair no renewal accepts (review 9), so the
liquidation turned into a lapse at the entry. This family had a critical in five reviews in a row, the last two in
residuals a fix had declared bounded or safe without re-deriving them per part, per repetition and per state. So this
round first states what each role can lose without signing again, as whole-order and whole-lineage quantities, makes a
seeded walk check exactly that after every step with hostile steps on, runs it on the unfixed templates, and accepts a
rule only when the walk holds.

### 28.1 The invariants

Notation: `q(n, r)` is `KpfOpenO.quoteOf` (rounded down), `C` the market's carrier, `D` the payout floor (`PLAIN_DUST`,
0.02 KAS), `F` the part floor (`PART_FLOOR`, 0.2 KAS = 10 D), `W` the contest window, `X` a pair's expiry. A side's
**claimable value** in a pair is what the lapse settle would pay it before the payout floor: the long
`q(s, clamp(E, bl, bs) - bl)`, the short the rest of the UTXO's value; a party's claimable value is that, summed over
its sides of live pairs, plus its resting orders' escrow, plus what it received in plain outputs less what it paid in.
The judge's verdict is taken as the truth of a claim (a claim upheld by the judge is valid; contesting a false claim
within `W` is the defender's part).

- **I-order.** An order of owner `o`, side `σ`, rate `myRate`, tip `t`, `premMax`, filled or taken in any number of
  parts `n_1 .. n_k` of total `N`, by anyone, with any slot its terms accept, at any time: `escrow_0 - (refund + Σ
  payouts to o from those parts) <= q(N, t) + q(N, h) + C + P + floor(P / 9) + k`, where `h` is the order's half of
  `premMax` (the long `premMax - floor(premMax / 2)`, the short the floor; charged by fills only, whether or not the
  first term is still ahead: an order cannot prove the present), `C` is charged at most once (with the part that
  empties the order), `P` is the order's side's share of the premiums of the renewals of its parts (cover actually
  bound by the slot, at most the agreed premium per term), `floor(P / 9)` bounds what the payout floor takes from a
  part those renewals brought below it, and `k` sompi is rounding, `k <= q(N, myRate) / F + 1`. Nothing else depends on
  the number of parts.
- **I-party.** In a step a party does not sign, its claimable value falls only by: a valid liquidation of its side (its
  value in the part), a close by the other party at the vouched price moved by `g` against the closer (the party gets
  at least its payoff at that price), the premium of a renewal of its pair (its half of `q(s, premium)` plus one sompi;
  the whole premium when the entry lies outside the renewed range, where the other side's lapse value is already zero
  or the whole range), the payout floor `D` at the lapse settle (once per lineage end), and one sompi of rounding per
  split (the long's two parts are each rounded down) or hand-over (the entrant's range is rounded down). A hand-over
  keeps the stayer's bound and share; a refuted or void claim pays its bond to the defender. A lapse settles at the
  entry; no lineage pays premiums ahead of the on-time schedule (27.4).
- **I-winner.** A valid liquidation (`P + W + step <= X`, 28.3) is settled, cured by margin that leaves a pair the
  underwriter can still renew (accepted no later than `X`), or refuted. It is turned into a lapse at the entry only if
  nobody, the winner and the underwriter (paid `B` + fee by it) included, finalizes it during the whole step `[P + W,
  X]` and the loser's cure lands after `X`. A pair that becomes liquidatable is claimable at once, or, in the last
  `W + step` of an unrenewed term, after its on-time underwriter's renewal (from `X - step`): a wait of at most `W`.
- **I-exit.** From every reachable state anyone can lapse-settle a pair at `X + tail + idleDaa` (not before), a pending
  part is finalized at `P + W` (or decided when its dispute resolves), and an order is cancelled by its owner at any
  time or refunded by anyone at `min(expiryDaa, DAA + 90 days)`; no party can postpone the lapse date (a party entry
  after `X` fixes it). While the underwriter keeps renewing on time the date moves by one term per renewal, at the
  agreed premium; the party's own exits then need a counterparty (a taker, a mutual or cooperative close) or the
  underwriter's vouch (a close claim): see 28.6, item 1.
- **I-uw / I-staker.** A slot's value and id change only by its owner's signed entries (`bind` keeps the value; `split`,
  `unbond`, `withdraw`), an underwriter's bond is repaid as 26.4 states; a stake becomes a receipt only by its owner's
  `open` or `commit`, and a pool pays the winners from the losers and burns as ruled.

The walk (`crates/kpf-tests/tests/invariant_walk_tests.rs`) keeps the claimable-value ledger of every party (every
applied transaction is booked: plain outputs received, plain inputs spent, sides of live pairs valued as above) and
checks I-party around every action and every renewal, and I-uw after each. Its hostile steps are on by default: a
resting order (either side, any slot or a named slot, the fixture market or the testnet defaults, tip 0 / 0.01 / 0.05
KAS, `premMax` 0 / 0.02 / 0.10 KAS) taken without a pair or filled alone, in up to 9 parts of every size from the
market's minimum (0.01 unit on testnet) to the rest, by an outsider with a genesis slot of its own or by the colluding
owner of the slot it names, into the past or with a live cover the attacker renews up to twice at `premMax`, every part
lapse-settled at its date; R9-F1's take into the past and its renewal; and the winner's cases: the latest claim the rule
calls valid, then the loser's cure after `X` with no finalize, a cure before `X` (must renew), or the finalize; and a
pair that becomes liquidatable in the last `W + 2 steps`. Every order's whole loss is compared with I-order, every late
cure with I-winner; at the end every live pair must lapse-settle at its date and not one DAA before. 20 seeds x 400
steps. On the fixed templates it holds: 8,000 steps (1,611 renewals and 905 refused as touched after their expiry, 278
opens, 1,112 amends, 184 transfers, 13 takes with a pair, 99 `closeDonor` and 103 `closeHost` alone, 76 close-versus-
close, 647 claims of which 178 on half a pair, 193 lapse settles; 727 attacked orders, 209 takes into the past, 379
winner cases), 3,754 refusals as the rules say, 6,456 hostile variants of 13 kinds refused (among them 695 parts below
the part floor, 1,935 parts asking the order one sompi more, 316 named-slot orders through another slot), and 64,381 invariant
checks (I-party 54,841, I-uw 8,154, I-order 727, I-winner 401, I-staker 159, I-exit 99), no violation.

### 28.2 What the walk found on the unfixed templates

Run on the templates of `c20e406` (the same walk, the same seeds):

| Invariant | Violations | Where |
|---|---|---|
| I-order | 280 of 700 attacked orders (2,222 parts), 147.33 KAS beyond the bound in all | every long-order route: taken and filled, any slot and named slot (the colluding underwriter), into the past and with a live cover, the fixture market and the testnet defaults (9 to 29 orders per route); one carrier per part, and on the testnet defaults the margin of every part below 0.02 KAS. No short order (its carrier comes back with its side). **R10-F1, and its `fill` route, which review 10 had only by reading** |
| I-winner | 108 of 108 late cures | the latest valid claim (`P` = 1,000 on the fixture: `P + W = X`) was finalizable 0 DAA before `X`; the cure at `X + 1 .. X + 50` wrote an unrenewable pair. **R10-F2** |
| I-party | 4 | a claim on half a pair: the long lost one sompi of lapse value (each part's share is rounded down). With the fixed templates (other paths) the same once in `closeHost`: the passive short lost one sompi (the entrant's range is rounded down). **New, Note:** at most one sompi per split or hand-over, moved to the other side; now an allowance of I-party |
| I-exit, I-uw, I-staker | 0 | |

Besides: 5,847 hostile variants refused, 3,590 refusals as the rules say; the longest wait for a valid claim of an
on-time underwriter 99 DAA (claims valid until `X - W`, renewal from `X - step`, `W` > step on the fixture).

Found while stating the invariants or building the walk, outside the contracts:

- **`--any-slot` was not a switch** (`args.rs`): it took the next argument as its value, so `--any-slot --escrow 3000`
  dropped the escrow, which then defaulted to a full fill + 1 KAS (the CLI test wrote it in that order). Fixed: it is a
  switch.
- **The tool's open took a carrier from both orders** (`oracle_build::open`, both orders' `fill_spend`): the pair needs
  one, so the funder's change kept the other. Fixed with 28.3.
- **I-exit has no unilateral form against a renewing underwriter**: see 28.6, item 1 (not fixed).

### 28.3 The rules

**R10-F1: an order pays at most one carrier, and every part holds at least 0.2 KAS of its margin.** `KpfOpenO.escrow`
allows `q(n, rate)` and adds the carrier only when the part empties the order (`left == 0`); `KpfOpenO.amount` requires
`q(n, myRate) >= PART_FLOOR` and, when something is left, `q(left, myRate) >= PART_FLOOR` (`PART_FLOOR` = 0.2 KAS, ten
payout floors). The carrier of a part that leaves a continuation is funded by whoever writes the part: the counterparty
of a take or fill alone, the executor of a fill of two orders neither of which it empties (the tool's builder takes the
carrier from the short order when the fill empties it, since the pair pays it back to the short; else from the long when
it empties; else the funder pays), the pair's own carrier in a take of a whole pair (it moves with `keep`; the order that
this take empties still pays its own carrier, which then goes to the transaction's builder, not to the short: review 11,
R11-N1, bounded by I-order). The options
weighed against I-order:

- the carrier paid by whoever takes: taken in the form above, which keeps a whole fill exactly as before;
- a part floor in the contract: taken; with it no part starts below the payout floor, and a part the renewals bring
  below it has paid at least `F - D` = 9 D in premiums, so the payout floor's take is at most a ninth of those premiums;
- an order that must name its slot: not taken. With the two rules an outsider's slot gains it nothing from an order
  beyond the tip and one carrier, and the same attack by a colluding named underwriter is just as bounded (the walk
  drives both); naming the slot would remove orders that accept any slot (live run 2 wrote one) and buy nothing
  against collusion. An order that accepts any slot still accepts that any underwriter vouches the prices of the pairs
  it writes (a false claim stands unless contested within `W`): the order text says so;
- no `take` without a pair: not enforceable. An order cannot tell a pair from a look-alike genesis of the pair template
  that its author lists and transfers; requiring a pair input changes nothing.

**R10-F2: a claim is valid only if `P + W + step <= X`.** `KpfPendingO.finalize` voids a claim with `P + challengeDaa +
stepDaa > expiry` (before: `P + challengeDaa > expiry`). A valid claim is finalizable at least one step before `X`,
before the renewal window opens; the market bound `challengeDaa < termDaa` keeps a valid window in every term. The other
answer the review asked about, refusing a cure after the expiry or making it restore the winner's protection, has no
rule a covenant can check: nothing proves "now is before X", the cured state's hash is fixed at the proposal, and a
renewal can only read the cured UTXO's own DAA. Checked against I-winner and I-exit: a pair liquidatable in the last
`W + step` of a term is claimable after its on-time underwriter's renewal (which no party can stop, 27.2), so the loser
cannot escape by waiting; it escapes only from a late underwriter (in the last `W + step` instead of `W`), by the lapse
at the entry or a party entry after `X`, as 27.4 states for every late underwriter.

**R10-N1: left documented.** Under I-party a late renewal racing a due lapse settle charges no premium beyond the
on-time schedule (27.4), and under I-exit a party ends it by any entry after an expiry (its own signature, no one
else's); a market bound `tail + idleDaa > term` would only lengthen every lapse exit (beyond 28 days on the testnet
defaults). With 28.3's claim rule a cover resumed after its due settle offers a valid claim only from its next on-time
renewal when the late renewal lands after `X' - W - step` (on the fixture with idle 300, always), which
`r10_bounded_with_tail_plus_idle_...` now measures.

**R10-N2 and the tool.** `oracle-order` defaults `--min-fill` to the whole size, refuses an order whose margin, or whose
`--min-fill`'s margin, is below 0.2 KAS (and names the least part), and prints the least part, the number of parts and
the whole order's worst case (`order_risk_lines`); `oracle-watch` reads `OPair::unrenewable_why` for a pair past its
expiry, tells the winner and the underwriter of a pending valid liquidation to finalize it before the expiry, and gives
the last DAA at which a claim is valid; `kpf vouch` refuses a claim that would land later than `X - W - step` (its bond
would go to the defender) unless `--void-ok`, and says when the renewal opens; `--any-slot` and `--void-ok` are switches.

**Not done: the payout floor.** Raising `PLAIN_DUST` to 0.05 KAS (27.5) would touch every payout of the pair and the
pending part; none of 28.3's rules needs it, so it waits for a change that touches those lines.

### 28.4 What remains, with numbers

- **I-order, the whole-order bound.** Live run 2's long order (2 units, rate 1 KAS, no tip, `premMax` 0, any slot):
  at most one carrier, 0.2 KAS, however it is taken; before, fifteen 0.01-unit rounds took 3.15 KAS of its 3.2 KAS
  escrow (`r10_holds_small_takes_...`: ten 0.2-unit takes, 0.2 KAS). The fixture order (2,000 units, rate 1 KAS, tip
  0.01, `premMax` 0.10): its tip 20 KAS, a fill's first premium half 100 KAS (`q(2,000, 0.05)`), one carrier 0.2 KAS, and
  its half of each renewal's premium at most at that rate (100 KAS a term on 2,000 units) plus a ninth of it for the
  payout floor; at most 10,000 parts (0.2 KAS of margin each), so the rounding term is at most 10,001 sompi.
  Before: a carrier per part, 2.1 KAS for ten 1-unit parts (`r10_holds_each_take_...`: 0.1 KAS, the tip). The walk:
  727 attacked orders in 1,935 parts, 0 sompi beyond the bound.
- **A valid liquidation undone by a whole step of silence.** If nobody finalizes a valid claim during `[P + W, X]`, at
  least one step (3.5 days on the testnet defaults, 100 DAA on the fixture), the loser's cure after `X` still writes a
  pair no renewal accepts, which settles at the entry: on the fixture pair the loser regains 954 KAS
  (`r10_bounded_a_cure_after_the_expiry_needs_the_finalize_withheld_for_a_whole_step`). Before: no step at all (`P + W =
  X` was valid). The walk met it 104 times by construction and nothing else.
- **No valid claim in the last `W + step` of an unrenewed term.** An on-time underwriter renews from `X - step` and
  claims against the next expiry, so a pair liquidatable in `(X - W - step, X - step)` waits at most `W` (6 hours on the
  testnet defaults, 200 DAA on the fixture; the walk's longest wait 195 DAA). Before: `max(0, W - step)`, i.e. none on
  the testnet defaults, 100 DAA on the fixture.
- **Rounding.** At most one sompi of lapse value per split (long to short) and per hand-over (to or from the stayer).
- **Two carriers in one fill.** A fill that empties both orders lets each pay one carrier (the contract cannot tell which
  one the pair needs): a builder other than the tool's can keep one. Bounded by I-order (one carrier per order).
- Unchanged from 27.4: the late renewal's back-charge (never ahead of the on-time schedule), R10-N1, an order that
  accepts any slot accepting any underwriter's vouch, and the entry a take without a pair writes in the order's favour.

### 28.5 What it gives up

- **Small parts.** No order is filled or taken in a part, or left with a rest, holding less than 0.2 KAS of its margin:
  at a rate of 1 KAS per unit, 0.2 unit (20 of the testnet's minimum sizes); an order whose whole margin is below 0.2 KAS
  can never be filled (the tool refuses to write it). A market maker that wants many small fills names a larger
  `--min-fill`, and every partial fill now needs its carrier from the counterparty or the executor (0.2 KAS each; the
  short gets it back at the pair's end).
- **The executor's carrier.** A fill of two orders that empties neither costs its executor the pair's carrier (paid back
  to the short at the pair's end, not to the executor), which the tips must cover.
- **The last `W + step` before an unrenewed expiry.** A claim landing there is void (its bond goes to the defender), and
  each term has a gap of up to `W` before the renewal opens in which a liquidatable pair cannot yet be claimed.
- Script bytes: `KpfOpenO` 6,516 (was 6,204: the two floor checks and the conditional carrier), `KpfPendingO` 4,399 (was
  4,395), `KpfPairO` 15,042, `KpfBond` 4,453, `KpfStake` 9,665 (unchanged; `KpfPairO`'s and `KpfOpenO`'s template hashes
  changed with the templates they pin). The take's largest stack is 209 of 244 (was 197). Every shape of
  `oracle_metrics_tests` still fits a block; an open costs 0.0948 KAS (was 0.0936), a take by an order 0.0830 KAS, one
  pair-month 0.1836 KAS. The pinned artifacts and 41 of the 46 golden vectors were regenerated; the book market's
  artifacts are byte for byte as before.

### 28.6 What a reviewer should attack next

1. **I-exit against a renewing underwriter.** While an underwriter renews on time (no party entry can stop it, 27.2), a
   party's exits all need someone else: a taker, the counterparty's listing, both signatures, or the underwriter's
   vouch of a close; the renewals stop only when the premiums have narrowed the range to the maintenance bands (97
   terms on the fixture pair at 0.02 KAS; never at a premium of 0). An underwriter that renews and refuses to vouch
   closes keeps a party in. A party opt-out of renewal (a state the renewal refuses, written by either party) would
   restore a unilateral exit at `X + tail + idleDaa`, but it gives the loser of a coming liquidation the late
   underwriter's escape; weigh it against I-winner.
2. **The carrier's new payers.** Every partial take or fill now needs a carrier from the counterparty, the pair (a whole
   take) or the executor: check the builders' funding (the open's funder, a take's executor), the storage mass of a
   fill whose order pays no carrier, and whether a counterparty can make an order pay its carrier earlier than with its
   last part (a part that empties the order by a rest the order did not choose).
3. **The part floor and the premium.** The floor bounds what the payout floor takes after renewals by a ninth of their
   premium; check renewals of parts whose entry lies outside the range (the premium then falls on one side's lapse
   value, I-party's allowance), cures and hand-overs of parts at the floor, and fills whose premium half rounds to zero.


## 29. Changes after review 11

Review 11 (`docs/audit/REVIEW-11.md`) is the first of six reviews of this family without a critical finding. It found
one High, four Mediums, two Lows and two notes, and two of the Mediums are holes in the safety net of section 28 rather
than in the contracts: the invariants valued every side at its entry price, so every loss that depends on the price
passed them by construction (R11-F5), and the exit invariant held by construction for a renewing underwriter. R11-F1: an
underwriter that renews on time and vouches no close held both parties for 52 terms of the gold pair. R11-F2: a closer
and its underwriter (or an underwriter alone, for a liquidation) chose the best moment of the last `lateDaa`. R11-F3:
an honest, true claim landing late paid its bond to the party it was right about. R11-F4: a market with `rateDaa >=
stepDaa` let anyone stop every bind of a slot. R11-F6: any SIGHASH_ALL spend of an underwriter UTXO vouches the claims
beside it. R11-F7: hand-overs split a passive side below the payout floor. The method is review 10's with one change:
the walk values every side at a simulated feed price, and a rule is accepted only when that walk holds with hostile
steps on.

The founder's decision of this round is part of it: the testnet contest window `W` is 2 minutes (1,200 DAA, from 6
hours) and the reference-time bound `lateDaa` is 10 s (100 DAA, from 10 minutes).

### 29.1 The economic invariants

Notation as in 28.1, and: `f(τ)` the feed at DAA `τ` (the walk's simulated feed; for a market, what its feed definition
names), `g` the tolerance, `m` the maintenance rate, `P` the DAA of the block that accepted a claim, `t0` the DAA it
names. A side's **value at the feed** in a pair is the long's `q(s, clamp(f, bl, bs) - bl)` and the short's the rest of
the UTXO's value; a party's value at the feed is that, summed over its sides of live pairs, plus its plain outputs less
its plain inputs. **The judge's rule** (the feed definition's, 29.3): a claim at price `p` is true only if `f` lies in
`[p (1 - g), p (1 + g)]` both at `t0` and at `P`, and neither moment falls in a closed session.

- **E-party.** In an action that a party does not sign, its value at the present feed falls only by these causes, each
  with its bound (gold pair: 0.1 oz at 80,000, 10x, `g` 0.5 %, `m` 5 %, premium 15.34 KAS a term):

  | Cause | Bound at the feed | Gold pair |
  |---|---|---|
  | premium of a renewal | its half of `q(s, premium)` + 1 sompi; the whole premium + 2 when the feed lies outside the renewed range | 7.67 KAS a term |
  | a true liquidation of its side (below maintenance at the feed at `t0` and at `P`) | its value at the feed in the part, `< q(n, f m)` + the pair's excess over its range | at most 400 KAS (its maintenance margin at 80,000) |
  | the keeper fee beyond the loser's value (the winner, when the loser's residual at the feed is below the fee) | the fee, `<=` the maintenance slice | at most 37.5 KAS at 75,000 |
  | a close by the other party | 1 sompi of rounding (the closer is paid at most the feed at `P`); the payout floor `D` when the party's payout is below it | 0 |
  | rounding of a split or hand-over | 3 sompi | 3 sompi |
  | the payout floor at the lapse settle | `D` per lineage end; with the part floor (29.3) a passive side's lineages each hold at least `PART_FLOOR` when created | 0.02 KAS |
  | a lapse at the entry | the side the feed favours loses its value at the feed less its value at the entry; it can be forced on a party only by its underwriter's silence (no renewal by `X`), and the party had `tail + idleDaa` after `X` to close its own side at the feed | 10.25 days to act; then its distance from the entry (a 10 % move: 800 KAS) |
  | a false claim the judge upholds, or nobody contests within `W` | its value in the part: outside the invariant (the judge's honesty and a watcher are the assumptions, as in 28.1) | 800 KAS |

- **E-exit.** From every state each party of a live pair reaches a settlement of its side (or of any part leaving both
  pieces above the part floor) by its own action, without the underwriter and without the counterparty: its own close
  at the price it states for the present, landing as soon as the pair UTXO is `quietDaa` old (60 s) and settled `W`
  later (2 minutes), at the feed at `P` moved by between 0 and `2g` against it (an honest close quoting the feed pays
  `g`; as the short, also the pair's excess over its range, which stays with the long). If the counterparty contests a
  true close it adds at least one judge round (12 hours) and loses its contest stake. A party below its maintenance, or
  within `g` of it, at the feed first adds margin within its share, or its exit is the liquidation (the market's rule). During a closed
  session the close waits for the reopening.
- **E-claim.** An honest claim, true at the time it prices and still true when it lands, never costs its proposer its
  bond: a late claim is judged, not voided, and a liquidation that lands after its last valid DAA (delayed across the
  boundary) returns its bond to the underwriter. A claim false at `P` because the feed moved by more than `g` during its
  delay may be refuted; that is the proposer's risk, which it removes by revoking (spending its vouch or funding input)
  a claim that has not landed within `lateDaa`.
- **I-order, I-winner, I-uw, I-staker** as in 28.1; the lapse date part of I-exit (the date and not one DAA before) stays.

The walk (`crates/kpf-tests/tests/econ_walk_tests.rs`) is review 11's extended walk with a feed: one seeded price path
for the market, a move of up to 2 % a tick on a volatile seed and 0.2 % on a calm one (a third of the seeds), a gap of
up to 6 % now and then and at every reopening, closed sessions of 300 to 800 DAA in which the price is frozen and no
claim is true. Every action step is checked at the feed of that step (the feed does not move inside an action); every
loss of a party that did not act is booked against its cause and checked against the sum of the bounds the step allows.
The E-exit check is a real one: the walk builds, for each party of a pair (each step for a random pair, and every pair
at the end), its own close of its whole side at the feed with no underwriter input, runs it and its finalize `W` later
in the engine, and checks both payouts against the stated tolerance. E-claim is driven by honest claims landing at once,
within `lateDaa`, beyond it, and delayed across the last valid landing of a term. The walk detects which rules the
templates carry by probing a party's own close once, so the same file runs on the old templates (29.2).

### 29.2 What the walk found on the old templates

Run on the templates of `c8a3a29` (20 seeds x 400 steps, the same walk), 4,402 violations:

| Invariant | Violations | What |
|---|---|---|
| E-exit | 4,025 | no party could close its own side at the feed: every close needed the underwriter's vouch, refused (silent underwriter) or withheld (a renewing one). **R11-F1** |
| E-party | 127 | a lapse at the entry where the party had no close of its own: 41,460.76 KAS of value at the feed lost to the entry in all, up to 2,948.76 KAS in one (the walk's fixture pair). **R11-F1 with R11-F5's blind spot** |
| E-party | 5 | look-back closes: the non-closer below its value at the feed by up to 243.74 KAS. **R11-F2** |
| E-claim | 245 | an honest claim true at both moments void, its bond (up to 600 KAS) to the defender: claims landing more than `lateDaa` after `t0`, and claims delayed across the last valid landing (`P + W + step > X`), 100,606.29 KAS of bonds in all. **R11-F3**, and its second form: a delay across the expiry boundary punished an honest proposer exactly as lateness did |
| I-order, I-winner, I-uw, I-staker | 0 | |

Nothing outside R11-F1, F2 and F3 (F4, F6 and F7 have shapes the walk does not drive: a market with a long rate limit,
a foreign transaction signed by the underwriter, splits of a pair whose side is worth less than the floor; each has its
own test). The second form of R11-F3 (across the expiry boundary) is the one new element: the review measured lateness.

### 29.3 The rules

**R11-F1: a party's own close.** `KpfPairO.propose` kind 1 no longer reads a vouch: the closer signs, states the price,
posts the bond (and the new part's carrier for a partial close), and the pair UTXO must be `quietDaa` old (a close is a
party entry). The closer may not be below its maintenance at the price it is paid, its price moved by g against it (a
long: `c = floor(p (10^4 - g) / 10^4)`, `c (10^4 - m) >= bl 10^4`; a short: `c = ceil(p (10^4 + g) / 10^4)`, `bs 10^4
>= c (10^4 + m)`). `KpfPendingO.finalize` no longer applies the expiry rules to a close: a close is valid at
any time, so a party exits at the feed whatever the cover's state. The options weighed against E-exit:

- *The review's request with `T_req`* (a party-priced close allowed once the pair has carried the party's signed close
  request for `T_req` without a vouched close). It satisfies E-exit only after `T_req`, needs a request state that every
  renewal, hand-over, restore, cure and forfeit must carry or clear (each a new way to lose it or to outlast it), and
  buys nothing the judge does not already give: a false party-priced close stands only if nobody contests within `W`,
  whether or not the underwriter was asked first. The underwriter's vouch of a close was never a check of the price
  that the judge does not make; it was a veto.
- *A "do not renew" flag* restores an exit only at the entry, and lets a loser escape every coming liquidation.
- *Any party proposes a close at any time with its own bond and no underwriter signature*: taken. It removes the
  underwriter's veto (a degree of freedom) instead of adding a request state and a timer.

Attacked before implementing:
- *A false price when nobody watches*: it stands exactly as any uncontested claim (28.1's assumption, now extended from
  the underwriter to each party). A false close costs its closer the bond (10 % of the cover, 200 KAS on the gold pair)
  whenever a watcher contests it within `W`; the disputer gets 20 % of it.
- *Escaping a due liquidation*: refused by the maintenance rule, stated at the price the closer is paid. A long's close
  at `p` is paid at `c = floor(p (1 - g))` and needs `c (1 - m) >= bl`; it is true at `P` only if `f(P) >= p (1 - g) >=
  c`, so then `f(P) (1 - m) >= bl`: the long is healthy at the feed, where no liquidation is true (the lowest price true
  at `P`, `f / (1 + g)`, triggers only if `f (1 - m) < bl`). The short's mirror holds. So a true close and a true
  liquidation of the same side never coexist. A first draft of the rule read the maintenance at `p` itself; attacking it
  showed a band of width `g` below the maintenance band in which a loser could close at the top of the true range and
  keep its residual, which the close price closes. A healthy party within `g` of its band adds margin before it closes.
  A close racing the underwriter's liquidation is a race of two spends; a false one is refuted (the bond to the winner
  and the disputer), and the quiet time gives the underwriter 60 s after every restore before the next.
- *Griefing by repeated requests*: there are none; a true close ends the part, a false one costs the bond.
- *The slot's cover accounting*: a close moves value between the two sides of the part only; the slot is not read.
- *What the underwriter can do meanwhile*: renew (a pending part is not renewed; a partial close leaves the remainder
  renewable), and liquidate a part the close does not cover; nothing that stops the close.
- *Hand-overs, renewals, the share rule*: a close spends the pair like any claim; nothing is carried that a renewal
  could clear or outlast. *The part floor*: a partial close is a split (29.3, R11-F7). *The claim-validity window and a
  lapse*: a close is valid after `X`, so a party of a pair whose underwriter went silent closes at the feed before the
  lapse settle at the entry is due (`tail + idleDaa` = 10.25 days at the defaults).

**The underwriter's role afterwards**: it sells cover (renewals; the slot bounds the cover it sells by `rho x K`), and
it is the only source of liquidation prices (the vouch). It has no say in closes and no way to hold a party: a party
pays premiums only for the terms it stays.

**R11-F2 and R11-F3: the two-moment rule.** The feed definition, the judge and every staker hold a claim to the feed at
its `t0` and at `P`. On chain: `KpfPendingO.finalize` no longer voids a claim for lateness (`P - t0 > lateDaa`), and a
void claim (a liquidation landed after `X - W - step`, or with `X > P + term + step`) restores the part and returns the
bond to its proposer, the underwriter. So that a void claim cannot be repeated to freeze a pair for free,
`KpfPairO.propose` refuses a liquidation from a pair UTXO that cannot carry a valid one (`born + W + step <= X <= born +
term + step`, `born` the UTXO's own DAA score): the restored part of a void claim is such a UTXO, so each part can be
frozen once per term, for `W`. Checked against the invariants:

- *What remains of the look-back* (E-party): a long closer is paid at most `floor(p (1 - g))` with `f(P) >= p (1 - g)`:
  at most the feed at `P`, whatever `t0` it picks; the non-closer gets at least its value at the feed. A liquidation
  needs the loser below maintenance at the feed at both moments (the lowest price true at `P`, `f / (1 + g)`, triggers
  only if `f (1 - m) < bl`). On the gold pair: before, the short lost 55.80 KAS (10 minutes) or 6.80 KAS (10 s) against
  the feed; now 0 at any `lateDaa`. The honest closer quoting the feed pays `g` (40 KAS).
- *Delaying a claim* (E-claim): a delay changes the outcome only if the feed moves by more than `g` during it. At 80 % a
  year the feed moves 0.045 % (one sigma) in 10 s, 0.11 % in 10 minutes and 0.85 % in an hour: a miner or a party that
  keeps blocks full for an hour (on the order of 36,000 KAS by the review's estimate) gives a true liquidation about an
  even chance of being false at `P`, against a 200 KAS bond, and the proposer revokes a claim that has not landed within
  `lateDaa` by spending its vouch input. Delaying a liquidation across its last valid DAA only freezes the part for `W`
  and returns the bond.
- *`lateDaa` as an outer bound*: kept as a market parameter (the block's layout is unchanged) with the tools' meaning:
  `kpf vouch` and `kpf oracle-close` take `t0` from the node's DAA score, refuse a `--t0` older than `lateDaa` or in the
  future, and with `--node` sign and submit in one step (R11-N2). Default 100 DAA (10 s, the founder's wish). On chain it
  is only bounded (`0 <= lateDaa < 2^32`); a void for lateness would bring R11-F3 back for every delay above it.
- The feed definition template (`kpf oracle-market --feed-template`, and the sample market's feed text) states the rule
  in its `time:` and `disagreement:` lines; `vouch`, `oracle-close`, `contest` and `deposit` print it.

**R11-F4.** Every copy of the bond bounds (`KpfBond`, `KpfOpenO`, `KpfPairO`, `KpfPendingO`, the two stand-ins) and
`BondParams::check` require `2 rateDaa <= stepDaa`: the owner has at least half of every step after an outsider's
advance. The walk (both markets) found no other parameter combination that makes an honest path unreachable; the
combinations the bounds already exclude are 28.3's.

**R11-F6.** `OBuilt::vouch_guard` (run by every signing of the tool) refuses to sign a transaction that holds a proposal
and a plain input of that pair's underwriter key, unless it is a liquidation the tool built (`propose`: the vouch, and
the funding when the underwriter funds). A close needs no vouch, so a close beside such an input is refused (the honest
life checks it). The documentation tells underwriters to keep vouching funds on a key of their own. A contract-level
binding (the vouch input naming the claim: a tagged script or a data signature over the claim) is not worth its size:
one more output and a script per claim, and it protects only an underwriter that signs others' transactions with its
slot key, which no command asks it to do.

**R11-F7.** `KpfPairO.checkPart` (transfer, closeHost, closeDonor, mutualClose, propose): when a split leaves a
remainder, each side with value at the entry must hold at least `PART_FLOOR` (0.2 KAS) of it in the smaller piece (so in
both). The review's attack (a 1-unit pair at 1 KAS a unit, 10x: the long's 0.1 KAS) is refused at its first take; a
split of 0.01 oz of the gold pair (80 KAS a side in the piece) is accepted. A side with nothing at the entry (its bound
at or past the entry) has nothing to lose and is not checked.

**R11-N1.** Documentation: in a take of a whole pair that empties the order, the pair's carrier moves with `keep` and the
order's carrier goes to the transaction's builder (28.3 said it came back to the short; corrected there). Bounded by
I-order (one carrier per order). **R11-N2**: covered by `vouch`'s freshness rule above.

**The founder's window.** With `W` = 1,200 DAA: the claim-validity rule `P + W + step <= X` loses 2 minutes and a step
of each unrenewed term (was 6 hours and a step); a liquidatable pair of an on-time underwriter waits at most `W` for a
valid claim (2 minutes); a party's own close settles 2 minutes after it lands; the quiet time (60 s) stays below `W`, so
an underwriter's claim still lands within the window after a party entry; `KpfStake`'s rules are unchanged (a contest
must be accepted before `P + W`, then the judge's rounds of 12 hours). A contested claim therefore takes at least 12
hours and 2 minutes, and each change of the lead another round (the supply bounds them, about 87 rounds). The tool's
warning that a short window is "not realistically" contestable is replaced by the true statement: only an automated
watcher can contest within it, every party needs one, and `kpf oracle-watch --feed-price P` is that watcher's check
(`CONTEST NOW` with the deadline, `--json` fields `contestNow` and `contestUntil`, exit code 3). The judge's round, stake
age and unbonding stay at their defaults: no rule needs them changed.

### 29.4 What remains, at the feed price, with numbers on the gold pair

- **Lapse at the entry.** Only for a party that does not close its own side within `tail + idleDaa` (10.25 days) after
  its underwriter failed to renew by `X`; then the side the feed favours loses its move since the entry (10 % = 800
  KAS). The walk met it 88 times (fixture), up to 2,004.93 KAS, each time with the party's own close proved available.
- **A true liquidation** takes at most the loser's maintenance margin at the feed (400 KAS at 80,000); the keeper fee may
  exceed what the loser had left (a gap): the winner's loss is at most the fee (37.5 KAS at 75,000).
- **The closer's spread.** An honest close quoting the feed pays `g`: 40 KAS on the gold pair; quoting the top of the band
  true at both moments pays nothing; the non-closer never gets less than its value at the feed. Closing as the short also
  leaves the pair's excess over its range (its carrier, 0.2 KAS, and premium rounding) to the long.
- **False claims** stand if nobody contests within 2 minutes (the whole side, 800 KAS): every party needs an automated
  watcher (its own or a service). Since review 11 the counterparty, not only the underwriter, can post a false claim (a
  close); each costs its poster the bond (200 KAS) when contested.
- **A frozen part**: a liquidation delayed across its last valid landing freezes the part for `W` once per term, with
  the bond returned.
- **A claim delayed long enough to be false at `P`** costs its proposer the bond; it needs a move above `g` during the
  delay (about an hour of full blocks at 80 % a year), and the proposer revokes a claim not landed within 10 s.
- Unchanged from 28.4: the late renewal's back-charge (never ahead of the on-time schedule), the two carriers of a fill
  that empties both orders, the entry a take without a pair writes in the order's favour, an order that accepts any slot
  accepting any underwriter's liquidation prices.

The walk's measurements (20 seeds x 400 steps on the fixture, 8 x 400 with the window, quiet time and `lateDaa` in the
testnet proportions), each cause's largest loss against its bound:

| Cause (fixture walk / short-window walk) | Losses | Largest | Its bound |
|---|---|---|---|
| premium of a renewal | 3,938 / 1,709 | 20.00 KAS / 20.00 | 20.00 KAS (+2 sompi) |
| a true liquidation of its side | 5 / 1 | 210.22 KAS / 48.42 | 454.21 KAS / 174.12 |
| the keeper fee beyond the loser's value | 6 / 2 | 46.38 KAS / 21.32 | 46.38 KAS / 21.52 |
| rounding of a split or hand-over | 18 / 9 | 1 sompi | 3 sompi |
| a lapse at the entry its own close could have avoided | 88 / 28 | 2,004.93 KAS / 1,665.31 | the same (the party's distance from the entry; its own close proved available each time) |
| a false claim the judge upheld (outside E-party) | 29 / 3 | 1,756.25 KAS / 4,028.71 | the defender's value in the part |
| look-back closes: the non-closer below its value at the feed | 54 / 20 | 0 | 0 |
| an honest closer below its value at the feed (E-exit probe; the fixture's g is 2 %) | 3,616 / 1,551 | 323.27 KAS / 291.18 | g of the price |

Counts: 20 seeds x 400 steps, 77,092 invariant checks (E-party 61,911, I-uw 9,104, E-exit 4,718, I-order 723, I-winner 254,
I-staker 201, E-claim 181), 3,751 refusals as the rules say, 723 attacked orders in 1,893 parts (0 sompi beyond
I-order), hostile variants refused of every kind of review 11's walk plus a party's own close signed by the other party
and one below its maintenance (372); the short-window walk, 8 seeds x 400 steps, 32,259 checks. No violation in either.

### 29.5 What it gives up

- **The underwriter's veto over closes**, and with it the second signature on a close: a party alone can now post a
  false close against its counterparty, as the underwriter alone could post a false liquidation. Both stand only if
  nobody contests within `W`.
- **A contest by hand.** With `W` = 2 minutes a claim is contestable only by an automated watcher; a party without one
  (its own or a service) is exposed to any false claim.
- **Small splits.** No hand-over, close or claim splits a pair so that a side keeps less than 0.2 KAS of its value at the
  entry in a piece: on a pair at 1 KAS a unit, 10x, a split needs at least 2 units in each piece.
- **Long slot rate limits.** No market with `rateDaa` above half a step.
- **The void for lateness** as an on-chain hygiene rule: a claim signed long ago and released late is judged at the feed
  when it lands, not voided.
- Script bytes: `KpfPairO` 16,149 (was 15,042: the own close and its maintenance rule, the liquidation's claimability,
  the part floor inlined at five entries, the rate bound), `KpfPendingO` 4,427 (was 4,399), `KpfOpenO` 6,536 (was 6,516),
  `KpfBond` 4,493 (was 4,453), `KpfStake` 9,665 (unchanged). Largest stack: the take 209 (unchanged), the pair 181 (a
  liquidation's propose, was 180), a party's own close 167. Fees: amend, transfer, lapse settle 0.033 KAS (were 0.031), a
  claim 0.041 (was 0.0386), a renewal 0.043 (was 0.0407), an open 0.0994 (was 0.0949). The pinned artifacts and the golden vectors were regenerated; the book market's artifacts are byte
  for byte as before.

### 29.6 What a reviewer should attack next

1. **The judge's two moments.** The covenant enforces nothing of the rule: check that every staker's tool, the feed
   definition template and the dispute texts state it identically, how a feed that publishes less often than `lateDaa`
   reads "the feed at `P`", and the closed-session boundary (a claim whose `t0` is the last open moment and `P` the first
   closed one).
2. **A party's own close against a liquidation in flight.** The race of a false close with a true liquidation (both spend
   the pair): measure what a loser gains by buying `W` + one round of delay for a bond, at 2-minute windows, and whether
   the quiet time after a restore suffices; and a partial close that leaves the loser's remainder liquidatable.
3. **The 2-minute window.** Contest landing times under load (a contest accepted at `P + W` is ignored): what share of
   false claims an honest watcher can no longer stop when blocks are full for 2 minutes, which costs far less than the
   hour a delay attack on a claim needs.
