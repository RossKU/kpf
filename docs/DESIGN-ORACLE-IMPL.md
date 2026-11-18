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
| pair `amend`, `addMargin`, `transfer`, `closeHost`, `closeDonor`, `mutualClose` | as KpfPair | as KpfPair; the pair UTXO `quietDaa` old (CSV); the range within `coverRate`; no entrant and no key of P3 is `uwKey`; `closeDonor` requires P3 under its own underwriter with `x3 <= X`, `premium3 <= premium` and P3's range within both cover rates |
| pair `coopClose` | both owners | any split |
| pair `lapseSettle` | anyone | `tx.daa >= X + tail + idleDaa` (CLTV, review 6; before: `X + tail` and the UTXO `idleDaa` old by CSV): settle at the entry price, as KpfPair's idle settle |
| pair `renew(u, k)` | the slot's bind (the underwriter signs) | the bind counts `q(s, coverRate)` at `X + term` for this input; the premium leaves the margins (the long's bound up by `premium - floor(premium / 2)`, the short's down by `floor(premium / 2)`); what may leave is `q(s, range) - q(s, new range)`; the range stays wider than both bands; possible only from `X - step` (the slot's window) |
| pair `propose(n, kind, side, p, t0, j, ...)` | the underwriter's P2PK input `j`, SIGHASH_ALL (and the closer's signature for a close) | `tx.daa >= t0` (CLTV); a liquidation triggers with g inside: long `ceil(p(1 + g)) (1 - m) < bl`, short `bs < floor(p(1 - g)) (1 + m)`; the keys differ; the range rule; the fee, the cure bound (no cure beyond `coverRate`) and the forfeit bound as KpfPair; the part moves into `KpfPendingO` with a bond `B = max(bondMin, 0.02 KAS, q(n, coverRate · σ))` |
| pending `contest(u, dOut)` | anyone with aged judge stake | the stand-in's contest: the opener is a canonical free `KpfStake` of this judge block spent by `open(self, self, dOut)`; three outputs of the id; records `P` and the opener's key |
| pending `decide(d)` | anyone, in the dispute's `resolve` | the stand-in's decide: upheld if YES leads, refuted if NO leads; upheld whatever the verdict if the contest was accepted at `P + W` or later |
| pending `cure(s)` | the loser, while pending | as `KpfPending.cure`, the refund to `uwKey` |
| pending `finalize()` | anyone: pending at age `W`, decided at once | refuted: restore, `floor(B · β)` to the disputer at `M - 1 - i`, the rest to the defender at `i`. Otherwise valid (`P + W <= X <= P + term + step` and `P - t0 <= lateDaa`): settle (a close at the price moved by g against the closer, its refund back; a liquidation forfeit and keep, `B` + fee to `uwKey`); void: restore, `B` to the defender |

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
close-versus-close with another pair of the same underwriter, (5) once the cover has lapsed (`X + tail`) and `idleDaa` more
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

Script bytes (prefix + state + suffix): `KpfOpenO` 5,845 (1 + 465 + 5,379), `KpfPairO` 13,337 (1 + 624 + 12,712; the note
estimated about 10 kB), `KpfPendingO` 4,302 (1 + 780 + 3,521; about 5 kB), `KpfBond` 4,453 (1 + 236 + 4,216), `KpfStake`
9,688 (1 + 270 + 9,417). The stack limit is 244: the stand-ins' bind proof (a local per argument) exceeds it with the
oracle market's 41 state fields, so the oracle order and pair carry an equivalent lean text (the same push checks, three
reads).

Transactions (each input committing the smallest covering compute budget; relay fee at 100 sompi per gram; the fixture
pair: 1,000 units, cover 6,000 KAS, a 10 KAS opening stake, a 0.5 KAS dispute base; `oracle_metrics_tests`):

| Transaction | Inputs | Script units per input | Compute mass | Storage mass | Bytes | Fee (KAS) |
|---|---|---|---|---|---|---|
| open (2 orders, slot bind, funder) | 4 | 268,178 / 268,325 / 169,945 / 100,001 | 52,850 | 529 | 42,850 | 0.0857 |
| propose a liquidation (pair, vouch, funder) | 3 | 583,177 / 100,001 / 100,001 | 26,016 | 0 | 17,486 | 0.0350 |
| propose a close | 3 | 399,986 / 100,001 / 100,001 | 24,116 | 0 | 17,486 | 0.0350 |
| finalize: forfeit, close, void, upheld, refuted | 2 | 9,386 to 9,601 / 100,001 | 6,814 to 7,271 | <= 109 | 4,734 to 4,821 | 0.0095 |
| cure | 2 | 110,188 / 100,001 | 8,025 | 28 | 4,835 | 0.0097 |
| contest (pending, opener's stake, funder) | 3 | 176,046 / 302,767 / 100,001 | 21,922 | 80,390 | 14,752 | 0.0295 |
| deposit, 1 stake / 7 stakes | 3 / 9 | dispute 299,751 / 1,136,235, each stake 229,904 | 27,260 / 110,660 | 80,079 / 82,231 | 20,060 / 79,640 | 0.0401 / 0.1593 |
| resolve and decide | 3 | 128,825 / 168,226 / 100,001 | 19,472 | 79,972 | 14,572 | 0.0291 |
| settle 1 winning + 1 losing receipt | 4 | 350,457 / 99,335 / 99,335 / 100,001 | 37,092 | 3,483 | 29,692 | 0.0594 |
| close the pool | 2 | 19,873 / 100,001 | 11,908 | 20,913 | 10,078 | 0.0202 |
| renew 1 / 4 / 7 pairs (one bind) | 3 / 6 / 9 | each pair 278,371 to 278,405, slot 170,049 to 170,363 | 25,487 / 75,161 / 124,835 | <= 524 | 18,627 / 59,091 / 99,555 | 0.0373 / 0.1182 / 0.1991 |
| lapse settle / mutual close | 2 | 27,638 / 28,266 | 16,049 / 16,059 | 4 | 13,769 / 13,779 | 0.0275 / 0.0276 |
| amend / transfer to a signing taker / cooperative close | 2 | 304,745 / 236,924 / 227,767 | 18,556 / 18,277 / 18,181 | <= 24 | 13,826 / 13,887 / 13,901 | 0.0277 / 0.0278 / 0.0278 |
| take by an order with the slot's bind | 4 | 236,924 / 268,118 / 169,945 / 100,001 | 46,820 | 30 | 37,490 | 0.0750 |
| unbond the slot | 2 | 167,792 / 100,001 | 8,273 | 12 | 4,943 | 0.0099 |

The storage masses near 80,000 come from the 0.5 KAS dispute base and the 10 KAS receipts (KIP-9); a larger base lowers
them. **The largest multi-pair shapes:** (before review 6; since then the fresh-id scan limits one proposal transaction to 7 whole pairs, section 24) proposals vouched by one input are limited by no scan: 4 / 8 / 12 pairs use
95,826 / 188,906 / 281,986 compute mass and 68,786 / 137,186 / 205,586 bytes (0.41 KAS for 12); the byte limit stops at
14 (17,150 bytes a pair). A renewal of 7 pairs (the bind's 7 claims) uses 124,835 compute mass and 99,555 bytes. An open
carries one pair (the twin scan admits 8 outputs; two order rests, the pair, the slot, the premium and change are six).

**One pair-month** (one term `Λ` = 28 days on testnet): open 0.0857 + one renewal 0.0373 + one claim (propose 0.0350 +
finalize 0.0095) = **0.1674 KAS of network fees**, plus the premium the orders agreed. At 10 % of the cover a year the
premium is 46.03 KAS a term for the fixture pair (0.767 % of its 6,000 KAS cover); the note's cost, `y/(Lρ)` a year per
side, is in the premium, not in the fees. A contested claim adds about 0.12 KAS (contest, a defence, decide, settle,
close).

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
   is valid only if `P + W <= X`.
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
    also requires P3's expiry no later, its premium no higher and its range within the donor's own cover rate.
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
the code since section 25, whose statement the run now checks.)

## 22. Public wording (oracle-only markets, as built; O2§6 corrected)

**True.** "No admin keys, no operator, no upgrade path." "Anyone creates markets; nobody approves them; read the feed
definition and the parameters before you trade." "A covenant holds your margin, but whoever sets the price can move it to
your counterparty: price control is custody control." "Each pair has one underwriter, chosen when the pair opens; only it
can price your pair; it can underwrite at most `ρ` times the KAS it has bonded." "A false price contested within the
window is reversed: your position comes back as it was, and the claim's bond is paid to you and to whoever contested."
"Disputes are won by the side committing more aged bonded KAS, by a factor `μ`; if colluders control that, false prices
stand." "A false price nobody contests within the window stands: you can lose your whole margin in that pair." "If your
underwriter stops, you can still leave without any price (transfer, mutual or cooperative close), and once its cover has
lapsed anyone can settle your pair at its entry price, after a fixed time nobody can postpone." (Review 6: the contest wording holds again only since the fixes of section 24; before them it was false for a part entered into an id another UTXO held. Review 7: it was false again between the fixes of reviews 6 and 7, when a bystander could take a stayer's margin by handing two pairs over into one part, and an underwriter could delete half a pair inside a proposal nobody could contest; it holds since section 25.) "The winner bears gaps beyond the loser's margin." "Prices are in
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
