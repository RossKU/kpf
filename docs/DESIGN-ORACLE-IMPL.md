# KPF oracle-only markets: first slice, the underwriting capital (implementation note)

Sections 1-7 are the first slice (the underwriting slot `KpfBond`); sections 8-14 the second (the stake judge
`KpfStake`).

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