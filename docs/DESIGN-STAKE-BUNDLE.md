# KPF stake judge at scale: 1,000 stakers in one dispute (design note)

Status: design only. Nothing here is built or measured unless a measurement is cited. Every size marked "estimate"
must be replaced by the experiment of section 8 that measures it before any decision rests on it.

Requirement (founder): about 1,000 stakers in one dispute should cost about 20 transactions in total, not 1,000+;
anyone who bonds KAS may take part; no operator and no admin key. Accepted assumption: honest majority of the
participating aged stake (`f* = p_h/(p_h + μ)`, DESIGN-ORACLE-2 1.7, measured in DESIGN-ORACLE-IMPL 10, Z10).

## 0. Summary

- Today one dispute with 1,000 stakers costs **289 transactions and about 22.7 MB** (143 deposits, 143 settles, open,
  resolve, close), plus **3,000 more** for bond, unbond and withdraw (one each per staker). 97 % of every deposit and
  settle input is the 9.65 kB `KpfStake` script that each stake carries.
- **Option D (slim stake, wide batches)** keeps every property of today's judge (own UTXO, own vote, age from the
  consensus DAA score) and moves the heavy code out of the stake. Estimate: **19 to 41 transactions** per dispute for
  1,000 stakers, depending on the compiled size of the slim stake (1 to 3 kB). It meets the target only at the low end.
- **Option B (Merkle vote bundle)**: one bundle UTXO commits to its members' leaves; each member signs its own vote as a
  data signature (`checkMsgSig`), aggregators carry 110 to 200 votes per transaction. Estimate: **10 to 15
  transactions** per dispute for 1,000 members, **14 to 21** counting the deferred leaf updates. It meets the target.
  It gives up: own-UTXO custody (one shared UTXO and one shared script per bundle), self-sufficient data (the bundle's
  leaves must be reconstructed from chain history), and contention-free exits.
- **Option A (delegation pool)**: 6 transactions per dispute for one pool; members cannot vote, only exit. The pool's
  key-holder can destroy members' stake at will and keep part of it (section 6). Not recommended.
- **Recommendation:** build D first (it is a refactor of reviewed code and a prerequisite of B); measure; add B as a
  second kind of stake in the same judge only if D's measured count is not acceptable and the founder accepts B's
  give-ups. Section 5.

## 1. Baseline: today's shapes and sizes

Measured (DESIGN-ORACLE-IMPL 11, 25.8; LIVE-1 table):

| Shape | Bytes | Compute mass | Source |
|---|---|---|---|
| bond a stake (plain send to the template) | 319 | | LIVE-1 step 2 |
| open (claim, opener's stake, funder) | 14,809 | 22,079 | LIVE-1 step 16, IMPL 25.8 |
| deposit, 1 stake | 19,988 | 27,188 | LIVE-1 step 17 |
| deposit, 7 stakes (BATCH) | 79,640 | 110,660 | IMPL 11 (script 9,688 B then) |
| resolve and deliver | 14,629 | 19,629 | LIVE-1 step 18 |
| settle, 7 winning receipts | 78,805 | 100,525 | IMPL 11 |
| close | 9,922 | 11,908 | LIVE-1 step 21, IMPL 11 |
| unbond / withdraw | 10,135 / 10,134 | 14,065 / 13,054 | IMPL 11 |

Derived: one more stake in a deposit adds about 9,930 B and 13,900 compute mass; one more receipt in a settle adds
9,812 B and 12,482 compute mass (IMPL 11). `KpfStake` is 9,652 B (IMPL 25.8), so the template is about 97 % of each
stake input. Fee at the tool's rate is about 0.002 KAS per kB (LIVE-1: 45,856 B paid 0.0917 KAS).

Today, N = 1,000 stakers, one dispute:

| Step | Transactions | Bytes |
|---|---|---|
| open | 1 | 15 kB |
| deposits (⌈999/7⌉) | 143 | 11.4 MB |
| resolve | 1 | 15 kB |
| settles (⌈1000/7⌉) | 143 | 11.3 MB |
| close | 1 | 10 kB |
| **per dispute** | **289** | **22.7 MB, about 45 KAS** |
| bond, unbond, withdraw (once per staker, not per dispute) | 3,000 | 20.6 MB |

Serialisation today: deposits pass through the one dispute UTXO (IMPL 11 "Contention"); nothing else is shared.

## 2. Platform facts this note uses (with where they were read)

1. **Transaction size.** Transient mass is 4 per byte (`consensus/core/src/constants.rs:31`, rusty-kaspa `01b532e`,
   the v2.1.0 tag in `Cargo.lock`) against a block limit of 1,000,000 (`consensus/core/src/config/params.rs:626`):
   at most 250,000 bytes per transaction, and such a transaction fills a whole block's transient budget. Compute and
   storage mass 500,000 each (same line).
2. **Script units.** 100 script units per gram (`consensus/core/src/mass/units.rs:6`). A signature check costs 100,000
   units (`runtime_resource_meter.rs:41-42`; DESIGN.md Z1). Every pushed byte costs 1 unit
   (`runtime_resource_meter.rs:47-49`). Hashing costs blake2b 2 units per byte, blake3 1, sha256 1
   (`crypto/txscript/src/opcodes/mod.rs:62-68`). So a Merkle level (concatenate two 32-byte hashes, blake3) costs
   about 64 + 64 + 32 = 160 units, about 1.6 grams; a signature costs 1,000 grams. Bytes and signatures dominate.
3. **Element and script size.** `MAX_SCRIPT_ELEMENT_SIZE` and `MAX_SCRIPTS_SIZE` are 1,000,000 in v2.1.0
   (`crypto/txscript/src/lib.rs`, checkout `01b532e`; the older `a41a333` has 520 / 10,000 before Toccata). Today's
   contracts already handle a 9.6 kB redeem script as one byte string (`KpfStake.ownRs`). A batch witness of 100 kB as
   one push is therefore within the limit (experiment 5 must confirm it end to end). Stack 244 items
   (`MAX_STACK_SIZE`); so a batch must be passed as one blob and sliced, not as hundreds of arguments.
4. **No loops.** SilverScript `for` is unrolled at compile time (IMPL 11: "7 (the unrolled loop)"). A batch of K items
   costs K copies of the loop body in the script that runs it.
5. **Primitives that exist.** Seen used in this repository: `blake3` (`KpfStake.sil:280, 451`), `blake2b` and
   `OpOutpointTxId` / `OpOutpointIndex` (`KpfPairO.sil:296`, the maker commitment), byte slicing and `+`
   concatenation (everywhere; `+` compiles to `OpCat`, `builtin.rs:153`), `checkSig`, `checkMsgSig`
   (`OpCheckSigFromStack`, probe `contracts/probes/Z1MsgSig.sil`; measured "100,000 script units each, as `checkSig`;
   not in the static sig-op bound", DESIGN.md row Z1). Present in the compiler but never used here: `sha256`,
   `blake2bWithKey`, `blake3WithKey`, `templateHash`, `OpAuthOutputCount` / `OpAuthOutputIdx`, `OpTxPayloadSubstr`,
   `OpChainblockSeqCommit`, and the zero-knowledge verifiers `g16.verify`, `r0.g16.verify`, `r0.succinct.verify`
   (`vendor/kob/vendor/silverscript/silverscript-lang/src/compiler/compile/expression/builtin.rs:19-77, 284-335`).
   There is no Merkle opcode; a Merkle proof is a sequence of concatenations and blake3 calls written out in script.
6. **Covenant ids.** A genesis id is a hash of the authorizing input's outpoint and, for each output of the group, its
   index, value and script public key (`consensus/core/src/hashing/covenant_id.rs`). So whoever knows an id's genesis
   outpoint and outputs can check offline what the first UTXO of the id was, without the transaction body. Any input
   carrying an id authorizes continuation outputs of it; a genesis output is not entered into the counters of its
   creating transaction (`crypto/txscript/src/covenants.rs:101-163`; reviews 6 and 7).
7. **Time.** Only lower bounds (`tx.daa` by lock time, a UTXO's age by sequence). An input's DAA score
   (`OpTxInputDaaScore`) is the exact acceptance score of the UTXO being spent: an exact upper bound on when that UTXO
   was created, read only when it is spent. This is the one fact that makes stake age provable, and every option
   below must take each age from such a score.

## 3. Options

Notation: N stakers or members in one dispute; K items per batch transaction; S the compiled size of a stake template
in bytes; all byte counts per item include the item's input or witness, its output if any, and its share of the
unrolled loop code.

### Option C. Aggregation at deposit time only, today's template

Raise `BATCH` in today's single template. Each stake input still carries the whole template, and the template grows
with `BATCH` because both unrolled loops (deposit, settle) are in it.

- Size: today 9,417 B of code holds two loops of 7; I estimate 300 to 600 B per loop iteration (not measured). With
  450 B: template ≈ 3.1 kB + 900 B × BATCH. A deposit of K stakes ≈ (K + 1) × (template + 0.4 kB). Under 250 kB:
  K = 14 (15 × 16.1 kB ≈ 242 kB). By compute mass alone about 35 would fit (500,000 / 13,900).
- Signatures: one per stake input; the 15-per-input limit does not bind.
- N = 1,000: ⌈999/14⌉ + ⌈1000/14⌉ + 3 = 72 + 72 + 3 = **147 per dispute**, about 32 MB (each stake input grew to
  about 16 kB).
- Who moves whose money, age, unbonding, forfeiture, payout: unchanged from today (each stake is its owner's; the
  receipt is the stake; age is the stake UTXO's own DAA score).
- Serialises: the dispute UTXO, as today.

Verdict: halves the count, does not approach 20. The carried template is the problem, not the signatures.

### Option D. Slim stake, wide batches (no new trust)

Split `KpfStake` into two templates:

- **`KpfStake` (slim)**: statuses free (4), unbonding (5), receipt (6). Entries `commit`, `unbond`, `withdraw`,
  `redeem`, and `open` (or `open` moves; see 7.1). No loop.
- **`KpfJudge`**: statuses dispute (7) and resolved (8). Entries `deposit`, `resolve`, `settle`, `close`, with the
  loops sized for wide batches. A stake input never carries this code; each batch transaction carries it once (as the
  dispute or pool input), or once per helper template if the loops are moved out of it (refinement below).

Per stake in a deposit: the slim input (S + 271 B state + about 150 B signature, arguments and outpoint) + its receipt
output (about 80 B) + one loop body in the judge (L ≈ 450 B, estimate). Per receipt in a settle: the same without the
signature.

| S (slim stake code) | K, loops in the judge (K ≤ 244 kB / (S + 0.5 kB + 2L)) | K, loops in helpers (S + 0.5 kB + L) | Transactions for N = 1,000 (2⌈N/K⌉ + 3) |
|---|---|---|---|
| 1.0 kB | 101 | 125 | 23 / 19 |
| 2.0 kB | 71 | 82 | 33 / 29 |
| 3.0 kB | 55 | 61 | 41 / 37 |

Compute mass does not bind first: per stake about 100,000 (signature) + 9 × S units (pushing and hashing its own
script twice, the judge reconstructing it twice) ≈ 118,000 units at S = 2 kB = 1,180 grams, plus about 3,000 grams of
bytes; 500,000 / 4,200 ≈ 119 > K.

"Loops in helpers": the judge's `deposit` and `settle` loops live in two stateless helper templates whose script
hash the judge pins; the batch transaction spends one helper UTXO beside the dispute or pool, so a deposit carries the
deposit loop and not also the settle loop. This is the same pattern as option B's verifier (B.3) and needs the same
checks; it is optional for D.

- **Bond.** A staker's plain P2PK input pays its own stake output. With `SIGHASH_SINGLE|ANYONECANPAY` (input i commits
  only to output i, Z13 for the stake entries; a plain P2PK accepts any sighash type) anyone can collect bonds into one
  transaction: about 200 B per bonder, 1,000 bonds in 1 transaction by bytes; about 1,100 grams each by compute
  (signature), so 2 to 3 transactions. Without a collector, one transaction per bonder, as today.
- **Commit and deposit.** As today: the owner signs `commit` `SINGLE|ANYONECANPAY`; an aggregator puts K commitments
  into one deposit.
- **Resolve, settle, close.** As today, K receipts per settle.
- **Unbond and withdraw.** Accept `SINGLE|ANYONECANPAY` with the continuation (unbond) or the payout (withdraw) at the
  input's own index, so these batch too: about K per transaction (S + 0.4 kB each).
- **Who can move whose money.** Unchanged: only the owner's signature moves a free or unbonding stake; a receipt moves
  only beside its resolved pool. No lead, no shared UTXO besides the dispute.
- **Lead disappears / votes against a member.** No lead exists.
- **Forfeiture and payout.** Unchanged: per receipt.
- **Age and unbonding.** Unchanged: the stake UTXO's own DAA score against the anchor; CSV on the unbonding UTXO.
- **End to end, N = 1,000, one dispute** (S = 2 kB, loops in the judge): bond 3 + deposits 15 + open, resolve, close 3
  + settles 15 + unbond 15 + withdraw 15 = 66 transactions. The per-dispute part is 33 transactions and about
  2 × 1,000 × 3.4 kB ≈ 6.8 MB (≈ 14 KAS of fees); over the table's range, 3.9 to 8.8 MB.
- **Serialises:** the dispute UTXO only (as today). Deposits chain in the mempool except the first after a lead change
  (IMPL 11).

### Option A. Delegation pool (key-holder or rule votes for members)

One UTXO per pool (`KpfPool`) holds the members' KAS. A **lead key** signs one data signature per dispute: "the pool
commits its counted value to side s of claim c, round r, anchor a". There is no rule a script could follow instead of a
key: no script can know which price is true, and "follow the majority of the members' signed votes" is option B.

- **State.** Judge block; lead key; a Merkle root of member leaves (owner key, shares); total shares S_p; accounted
  value V; a short list of age cohorts (since, shares); counters of open receipts.
- **Age.** The pool UTXO's own DAA score restarts at every join or exit, so it cannot be the age. Cohorts: a join
  adds its shares to a new cohort whose `since` is the exact DAA score of the joining stake UTXO (it joins as a free
  stake, read by `OpTxInputDaaScore`). Two cohorts may merge into one with the later `since`. An exit removes shares
  from the oldest cohort first. Then for every anchor A the recorded shares with `since + M_age ≤ A` are at most the
  true ones (proof: a join adds only at thresholds at or after its true time; a merge only delays; removing x from the
  oldest cohort lowers the recorded cumulative by min(x, R(t)) ≥ the true decrease or keeps it below the true value).
  So the counted value is conservative. This is a derivation on paper, not a test.
- **Deposit.** The pool input beside the dispute; the receipt holds the counted value c = aged shares × V / S_p; the
  pool keeps the rest.
- **Forfeiture and payout.** By shares: a lost receipt lowers V by c for every share, aged or not; a won receipt's
  return raises V. **Members who are not yet aged share the loss of a vote in which they did not count.** O(1) per
  dispute, no per-member work.
- **Unbonding.** An exit request freezes the member's shares from future commitments; payout of shares × V / S_p
  after the receipts open at the request resolve, then `U` by the calendar. Without the freeze the lead can keep the
  pool in disputes forever and no member can leave. The exact bookkeeping of "receipts open at the request" is not
  derived here.
- **Who moves whose money.** The covenant moves value only by join, exit and the lead's vote. The lead cannot
  withdraw, but it can put the whole counted value on the side that will lose (section 6: theft by vote).
- **Lead disappears.** No votes; members' stake idle; exits work (they need no lead).
- **Lead votes against a member's wish.** The member cannot stop it; it can only have left before.
- **Count, N = 1,000 in one pool:** open 1 + deposit 1 + resolve 1 + settle 1 + collect the return 1 + close 1 =
  **6 per dispute**. In P pools under one covenant id (B.2 explains why one id): 2P + 4 (one pool per transaction).
- **Serialises:** each pool's UTXO (joins, exits, votes).

### Option B. Merkle vote bundle (each member votes; one UTXO commits to the leaves)

#### B.1 Objects

- **Bundle** (`KpfBundle`), one UTXO per bundle, all bundles of a market under **one covenant id F** that the market
  body records (B.2). Value: the members' free KAS plus a surplus. State (about 450 B, estimate): the judge block
  (87 B); `root`, the root of a fixed-depth Merkle tree (depth 12, 4,096 leaves; blake3); `seq`, a counter raised by
  every spend; `stamps`, a ring of up to 16 pairs (seq, DAA) (B.4); `recRoot`, the root of a second tree of dispute
  records; `surplus`.
- **Leaf** (about 120 B): `ownerKey`, `voteKey` (the owner's own key unless the owner sets another), `amt`, `seq`
  (the bundle sequence number of the transaction that last made this amount: join or win), `status` (free, locked,
  exiting), and when locked the claim id, the side and the round it was voted in.
- **Record** (about 90 B, one per (claim id, side) the bundle has voted in): total amount and units committed, the
  outcome when known (won, lost), the pool's `lnet` and winning weight `wu`, leaves still locked to it.
- **Bundle receipt**: a `KpfStake` receipt (status 6) under the claim's id with a kind flag "bundle", owner field F,
  side, `amt` = ΣA, a new `units` field = Σ ⌊a_i / 10 KAS⌋, round, anchor; value ΣA + a reserve of 1 KAS (B.6).
- **Return** (new `KpfStake` status 10): written by the pool's `settle` at the bundle receipt's own index under the
  claim id; names F, the claim, the side, won or lost, `lnet`, `wu`.
- **Verifier helpers** (`KpfBatchVote`, `KpfBatchJoin`, `KpfBatchApply`, `KpfBatchExit`): stateless templates with the
  unrolled loops; the bundle pins their script public keys (B.3).

#### B.2 Why one covenant id F per market, and how F is trusted

A bundle's leaves carry ages that the bundle's own UTXO does not show. A bundle written by a genesis could declare
leaves with any age and any amount backed by KAS just sent: capital raised after the claim would count, which the
judge's age rule exists to forbid (IMPL 12.1). Unlike a forged dispute (IMPL 13: it traps only the stakers who join
it), a forged bundle attacks the honest side of someone else's dispute, so no victim's wallet check helps. The judge
must therefore accept a bundle only if its lineage is clean, on chain.

The induction of IMPL 24.1, applied to a pinned id: F is created once, before the market, by a genesis whose only
output is an empty bundle; F goes into the market body (so into the market id, which every claim and stake of the
market carries). Every entry of `KpfBundle` requires `OpCovInputCount(F) == 1`, counts `OpCovOutputCount(F)` exactly
(1, or 2 for `spawn`) and fixes each F output's script; no entry writes a leaf age from a witness. Then every UTXO of F
is a bundle written by the bundle template (only an input of F authorizes F outputs; a genesis cannot reproduce F, it
hashes a spent outpoint). The genesis itself is checked once offline from F's preimage (fact 6: the genesis outpoint
and the empty bundle's value and script), which `oracle-check` must do; it needs no pruned transaction body.

Consequence: one bundle per transaction (two inputs of F would let one authorize outputs the other relies on, the
R7-F1 class). Several bundles per market exist only to spread contention (`spawn`).

#### B.3 Transaction shapes

Each shape spends the bundle (F), sometimes the dispute or pool (claim id), one verifier helper (stateless P2SH, a
small carrier) and a funder. The batch is one blob argument of the verifier, sliced at fixed offsets; the counts and
declared sums are fixed-position arguments before the blob so that other inputs can read them (as the claim reads
`open`'s arguments today, IMPL 9 `contest`).

1. **Join.** Two ways.
   - Interactive: members' plain P2PK inputs, each signing `SIGHASH_ALL` over the final transaction that a collector
     builds (one round of signatures; a non-signer is dropped and the batch rebuilt). Per member: input about 150 B,
     leaf and path in the witness about 120 + 384 B, verifier code about 400 to 1,000 B (estimate): 1.0 to 1.6 kB,
     K ≈ 150 to 230.
   - Non-interactive: from a free slim stake (a "ticket"), whose new entry `join` checks a data signature of its owner
     over (join, F, voteKey, its own outpoint) and that the bundle input runs `join` over a range covering it. Per
     member about S + 0.5 kB more (the ticket's script): K ≈ 50 to 80.
   - The new leaf gets `seq` = the bundle's current `seq` (its exact DAA becomes known at the next spend, B.4).
2. **Vote** (deposit). Inputs: the dispute, the bundle, the vote verifier, a funder. Witness per vote: the leaf, its
   path, the side, a data signature by the leaf's `voteKey` over m = blake2b(vote ‖ F ‖ ownerKey ‖ leaf seq ‖ claim id
   ‖ side ‖ round ‖ anchor). The verifier checks each leaf against the running root, that it is free, `amt ≥
   minStake`, its age (B.4), the signature; locks the leaf; sums ΣA and ΣU; and inserts or updates the record of
   (claim, side). Outputs: the dispute's continuation (tally + ΣA, ΣU), the bundle receipt at the bundle input's own
   index (claim id, authorized by the dispute input), the bundle's continuation (F, value less ΣA + reserve).
   - Per vote: witness about 120 + 384 + 64 + 8 ≈ 580 B; verifier code about 400 to 1,000 B (estimate); compute about
     1,000 grams (signature) + 30 grams of hashing + the bytes. By bytes K ≈ (250 − 15) / (0.98 to 1.58) ≈ 150 to 240,
     by compute ≈ 190 to 235. With the judge's loops inline in the dispute input (option D without helpers, about 70 kB
     at K = 71), K ≈ 110 to 170. Take **K_v ≈ 110 to 200**.
   - Stake inputs of option D may be in the same deposit (the judge loop handles both).
3. **Resolve.** Unchanged.
4. **Settle.** A bundle receipt is settled like any receipt; a winner's value plus share, or a loser's reserve, goes
   into a return at the receipt's own index under the claim id (counted: `OpCovOutputCount(id) == 1 + returns`). A
   vote transaction makes one receipt, so a dispute has at most ⌈N/K_v⌉ bundle receipts: one settle.
5. **Collect.** The bundle with up to K returns: each return checks that the bundle runs `collect` over a range
   covering it; the bundle checks each return's template, id = the record's claim id, F, and records the outcome,
   `lnet`, `wu`; value in. One transaction.
6. **Close.** Unchanged.
7. **Apply** (anyone, no signature). For leaves locked to a decided record: a winner's `amt` += ⌊u × lnet / wu⌋
   (today's `share` arithmetic per leaf) and its `seq` := the current `seq` (the age restarts, as a winning stake's
   does today, IMPL 9 settle); a loser's leaf is cleared. The record's count goes down. Per leaf about 0.5 kB witness
   + 0.4 to 0.8 kB code: K_a ≈ 190 to 260. Apply can also be folded into the same leaf's next vote or exit, so it need
   not be a separate transaction.
8. **Exit.** Two steps. Request: the owner's data signature over (exit, F, ownerKey, leaf seq, amt, payout script
   hash); the leaf (free) becomes exiting at the current `seq`. Payout: when `tx.daa ≥ stamp + U` (lock time) for a
   stamp at or after the request's `seq` (B.4), the leaf is cleared and a P2PK output of the owner's choice gets
   `amt`, at a positional index the verifier fixes (outputs `M − 1 − j` beyond the inputs, as the pair family does).
   Per member about 1 kB (request) and 0.6 kB (payout): K ≈ 150 and 250.
9. **Spawn.** The bundle writes a second F output: an empty bundle. Spreads contention; no value moves.

#### B.4 Stake age through the bundle

Leaf ages come only from the bundle's own input DAA scores. The transaction with bundle sequence number e is accepted
at some DAA score T_e, unknown inside it. The next spend reads `OpTxInputDaaScore(bundle)`, which is exactly T_e
(the bundle input it spends was created by transaction e), and appends the stamp (e, T_e) to `stamps`. Leaves joined
or won in transaction e carry `seq` = e.

A vote at anchor A counts a leaf with `seq` = e if some stamp (e', d') in the ring has e' ≥ e and d' + M_age ≤ A.
Sound if T_e ≤ T_e' whenever e ≤ e': each bundle transaction spends the previous one's output, and a child is accepted
in the same chain block as its parent or a later one, whose DAA score is not lower. This monotonicity is a consensus
property I have not read in the code (experiment 12). The ring keeps the newest stamp always and thins older ones to a
spacing of at least M_age / 8 (estimate of a useful rule), so a leaf counts at most about M_age / 8 later than its true
age, whatever the spend rate.

The exit's unbonding uses the same: payout needs `tx.daa ≥ d'' + U` for a kept stamp (e'', d'') with e'' ≥ the
request's seq, so d'' ≥ the true request time and the wait is at least U.

Comparison with today: a stake counts if its UTXO was accepted M_age before the anchor; a leaf counts if the bundle
transaction that last made its amount was accepted M_age before the anchor (or a little later, conservatively). A
win restarts the age in both. The bundle UTXO's own age is never used.

#### B.5 Who can move whose money; what a member does and when

| Action | Who | Signature | Effect |
|---|---|---|---|
| join | the member | `SIGHASH_ALL` (P2PK) or a data signature (ticket) | its KAS into a new leaf |
| vote | anyone who holds the leaf's signed vote | `voteKey` data signature, per dispute and round | the leaf's whole `amt` into a bundle receipt on that side |
| set vote key | the member | `ownerKey` data signature | changes who signs its votes |
| exit request, payout | the member signs the request; anyone submits both | `ownerKey` data signature | after U, `amt` to the member's chosen script |
| apply, collect, spawn | anyone | none | deterministic bookkeeping |

A member must: join once (at least M_age before the claims it wants to count in); sign one vote message per dispute
and round it takes part in (no transaction of its own, no fee); sign one exit request. No lead exists. If nobody
aggregates its vote it can submit a batch of one itself. A member that never signs never risks its leaf.

#### B.6 Forfeiture and payout per member

The bundle receipt is a receipt like any other: the judge counts ΣA and weight ΣU (today's units, summed per leaf so
that bundling gives no rounding weight). A losing receipt is forfeited to the pool except its 1 KAS reserve, which
returns to the bundle's surplus in the return (the reserve exists so that a loss produces a UTXO the bundle can
collect; without it nothing tells the bundle the dispute ended, and anyone may close the pool first). The reserve is
paid from the bundle's surplus, or by the vote's builder when the surplus is short, and is not counted. A winning
receipt's return holds its value plus ⌊ΣU × lnet / wu⌋. Each winning leaf is later paid ⌊u_i × lnet / wu⌋; since
Σ⌊u_i x⌋ ≤ ⌊Σu_i x⌋, the bundle never owes more than it received; the remainder stays in the surplus. Each losing leaf
loses its whole `amt`, exactly as a losing stake today.

#### B.7 Count and bytes, N = 1,000 members, one dispute

| Step | Transactions | Bytes (estimate) |
|---|---|---|
| open | 1 | 15 kB |
| votes ⌈1000 / K_v⌉, K_v 110 to 200 | 5 to 10 | 1.0 to 1.6 MB |
| resolve | 1 | 15 kB |
| settle (≤ 10 bundle receipts) | 1 | 20 to 40 kB |
| collect | 1 | 20 to 40 kB |
| close | 1 | 10 kB |
| **immediately** | **10 to 15** | **about 1.1 to 1.7 MB, 2 to 4 KAS** |
| apply ⌈1000 / K_a⌉ (deferrable; can ride on the next vote or exit) | 4 to 6 | 0.9 to 1.3 MB |
| **with apply** | **14 to 21** | **2.0 to 3.0 MB** |

Once per member, not per dispute: join 5 to 7 (interactive) or 13 to 20 (tickets); exit 7 (requests) + 4 (payouts).
End to end with one dispute: about 31 to 45.

**Serialises:** the bundle UTXO (every join, vote, collect, apply, exit of its members) and the dispute UTXO (every
deposit, as today). A vote spends both. Members spread over `spawn`ed bundles serialise per bundle.

### Option E. Others considered

- **Zero-knowledge batch.** `g16.verify` and `r0.succinct.verify` exist in the compiler (`builtin.rs:40-44, 284-335`).
  A proof that "these 1,000 signed votes are valid leaves of root R, here is the new root and ΣA" would make a vote one
  transaction. Not used anywhere in KPF; the verifier's cost under v2.1.0 is unknown to me; it needs a prover, a
  circuit, and for Groth16 a trusted setup; the data problem of B remains. Not pursued now.
- **An explicit roster instead of a Merkle tree.** The bundle's redeem script holds every leaf (about 120 B each;
  2,000 members ≈ 240 kB, the transaction limit). Every spend carries and rehashes the whole roster (2 units per byte
  for blake2b), so a join or exit costs about 120 kB at 1,000 members. No better than B on bytes; same data problem
  (a P2SH UTXO stores only the hash of its script). Rejected.
- **A small co-owned stake** (m owners, all or a threshold sign by data signature): one side per UTXO, so members
  cannot vote differently. A small version of A. Rejected.
- **Votes without moving stake.** Not possible: the judge must read the stake (to check age and to forfeit it later),
  and reading means spending (no read-only inputs). Every scheme touches each stake at least twice per dispute (vote,
  settle) unless stake is pooled, which is what A and B do.

## 4. Comparison

| | Today | C | D | A | B |
|---|---|---|---|---|---|
| per dispute, N = 1,000 | 289 | 147 | 19 to 41 | 6 (one pool) | 10 to 15 (14 to 21 with apply) |
| bytes per dispute | 22.7 MB | ~32 MB | 3.9 to 8.8 MB | about 0.15 MB | 1.1 to 3.0 MB |
| member votes itself | yes | yes | yes | **no** | yes (or a vote key it chooses) |
| member's KAS in its own UTXO | yes | yes | yes | no | **no** |
| age from | the stake UTXO | same | same | cohorts of joining stakes (conservative) | bundle stamps (conservative) |
| data a member needs | its own UTXO | same | same | the pool's leaves | **the bundle's whole tree** |
| shared UTXO besides the dispute | none | none | none | the pool | the bundle |
| new trust | none | none | none | the lead | F's genesis checked once; no key |
| new code | none | constant | split template | pool + cohorts | bundle + 4 verifiers + records + stamps |

## 5. Recommendation

**Build D now; build B on top only if D's measured count is not acceptable and the give-ups below are accepted.**

Reasons:
1. D removes the dominant cost (the 9.65 kB template in every stake input) and changes no trust assumption, no age
   rule, no custody, and no data dependency. Its attack surface is the template split (section 6.2), which is the
   review-6/7 class in a known place.
2. D is a prerequisite of B anyway: B's receipts, returns and tickets are slim `KpfStake` instances, and B's vote
   transaction needs the judge's code out of the way.
3. B is the only option found that reaches about 20 with each member deciding its own vote. Its estimate rests on
   two unmeasured sizes (verifier code per vote, slim stake) and on two platform facts that need experiments
   (one-megabyte push elements, `checkMsgSig` outside the sig-op limit).

Count against the target (1,000 stakers ≈ 20 transactions per dispute):
- D: 19 to 41. Meets the target only if the slim stake is about 1 kB and the judge's loops are split into helpers.
  Best honest central estimate: about 30.
- B: 10 to 15 at once, 14 to 21 with the deferred leaf updates. Meets the target.
- A: 6, by giving up the members' votes.

What B gives up, exactly:
1. **Own-UTXO custody.** Members' KAS sit in one UTXO per bundle under one script. A defect in `KpfBundle` or a
   verifier exposes every member of every bundle of the market at once. No key can move the money, but a script bug
   can.
2. **Self-sufficient data.** To vote or exit, a member needs its leaf's current Merkle path, which changes with every
   other member's action. The path is computable from the bundle's transaction history (every witness carries the
   leaves it changes and the stamps are on chain), so one honest holder of the history suffices and a served tree
   can be checked against the root; but if nobody kept the history beyond the node's pruning horizon, the bundle is
   frozen with all its KAS. This is a data dependency, not an operator with powers. Today a staker needs only its own
   UTXO's state.
3. **Contention-free exits and votes.** Every member's operation passes through the bundle UTXO; a griefer can delay
   them (section 6, open).
4. **A one-time check per market.** Wallets must verify F's genesis (empty bundle) before trusting the market's
   bundles; a market created before this change has no F and no bundles.

Members who do not accept these keep individual slim stakes; both kinds count in the same dispute.

## 6. Attacks

### 6.1 On option B (the bundle)

| Attack | Verdict | Reason |
|---|---|---|
| theft of members' stake by a lead | **holds (no lead)** | no key can move a leaf except its owner (exit to the owner's chosen script) and its vote key (into a receipt of a side); join, apply, collect, spawn are deterministic |
| theft by an aggregator | **holds** | it can only include or omit signed operations; it cannot change a leaf, a side, a payout script or an amount (each is signed or computed) |
| vote against a member | **holds with `voteKey` = `ownerKey`; broken by design with a delegated vote key** | a delegate holding many leaves' vote keys can put them on the losing side while its own stake is on the winning side and collect part of their value, as the lead of A (6.3); members who delegate trust the delegate for their stake |
| a signed vote landed late | **exploit, bounded, as today** | no upper bound on time: whoever holds the signed vote can land it after the window, before the resolution, and the leaf is forfeited (IMPL 13 "a relayed commitment landed late"); void after a lead change (round in the message) and in any other claim (claim id in the message) |
| vote replayed in another dispute or round | **holds** | the message names claim id, side, round, anchor, the leaf's owner and seq; the leaf is locked by its first use |
| same stake in two disputes, or on both sides | **holds** | a leaf is locked to one (claim, side) until applied; a second vote fails the "free" check against the running root; one bundle input per transaction, so no concurrent state |
| the same leaf twice in one batch | **holds** | updates are sequential on one root; the second proof is against the root that already has the leaf locked |
| stake-age forgery through the bundle | **holds if B.2 and B.4 hold** | ages come only from `OpTxInputDaaScore(bundle)` stamps; no entry reads an age from a witness; a forged bundle needs id F, which only bundle inputs can author; F's genesis is checked offline from its preimage. Depends on stamp monotonicity (experiment 12) |
| forged bundle by a genesis under another id | **holds** | the judge accepts a bundle input only with id = the F recorded in the dispute (copied from the claim's market body at `open`) |
| P2PK UTXO carrying id F (review 6) | **holds** | only an input of F authorizes F outputs, and every bundle entry fixes every F output's script and counts them; a genesis cannot reproduce F |
| two inputs satisfied by one output (review 7) | **holds by construction, to be tested** | one F input per transaction; the bundle receipt and returns sit at their own input's index; exit payouts at `M − 1 − j` beyond the inputs; the bundle continuation is the only F output |
| new id invisible to the counters (review 6/7) | **holds** | the bundle never creates an id: receipts and returns are continuations of the claim id (authorized by the dispute or pool input), the bundle's outputs continuations of F; the only genesis is F's own, checked offline |
| forged return credits winnings | **holds if returns live under the claim id** | a return without an id could be written by anyone with any `lnet`, `wu`; under the claim id only the pool's `settle` writes one (induction of IMPL 24.1). This is why `settle` must change its output count (7.1) |
| a return spent outside `collect` | **holds, to be tested** | the return requires the bundle input to run `collect` over a range covering it (dispatch tag and arguments at fixed offsets, as the claim checks `open`); the pool's entries count their inputs exactly, so a return beside them fails |
| ticket joined without its owner's consent | **holds** | the ticket's `join` needs its owner's data signature over its own outpoint (single use) |
| exit replayed after the owner rejoins | **holds** | the request names the leaf's seq and amount; a new leaf has a new seq |
| blocking a member's exit | **open** | nobody can refuse a signed request (anyone may submit it), but a griefer can keep the bundle UTXO busy with valid operations, and with replacement of its own unconfirmed spends could keep invalidating an honest chained spend. Whether the v2.1.0 mempool allows that, and at what fee, is experiment 11. Cost to the griefer: fees, plus `minStake` per join it uses as filler (recoverable after U) |
| blocking the bundle's deposit within a round window | **open, more serious than the exit case** | the same griefing against a bundle during a round delays its members' votes until the window closes, and the honest side can lose the dispute. Today a griefer of the dispute UTXO must use aged stake on the trailing side (IMPL 11); a griefer of a bundle needs no aged stake. Mitigations: several bundles (`spawn`), members free to vote through individual stakes, votes not bound to the bundle's outpoint (they stay valid across its spends). No mitigation removes it |
| filling the record space | **holds** | records are leaves of `recRoot`, not a fixed array; a griefer who votes its own leaves into many disputes only adds records |
| filling the tree (4,096 leaves) | **bounded** | costs 4,096 × `minStake` bonded (409,600 KAS at the 100 KAS default), recoverable; `spawn` makes another bundle |
| thinning the stamp ring by spending often | **holds with a spacing rule** | the ring keeps the newest stamp and thins by DAA spacing, not by count, so frequent spends do not delay any leaf's counting by more than the spacing (experiment 13) |
| forged dispute (genesis claim) used to trap votes | **bounded, as today** | the vote names the claim id; a member who signs for an untraced claim can lose its leaf (IMPL 13, R7-F2); the wallet must trace before signing |
| capture threshold (concentration of voting power) | **unchanged with own vote keys; reduced to the delegates with delegation** | see 6.3 |
| overflow | **holds on paper** | ΣA over at most 4,096 leaves is at most the supply, about 2.9 × 10^18 sompi < 2^63; per-leaf payout uses `share`'s split product; to be fuzzed |
| dust and mass | **holds** | a leaf holds at least `minStake` ≥ 10 KAS (`checkJudge`); payouts are ≥ 10 KAS, about 1,000 grams of storage mass each by IMPL 11's 10^12 / value, so 250 payouts ≈ 250,000 < 500,000; a return holds at least the 1 KAS reserve, 10,000 grams, at most about 10 per dispute; a 250 kB vote transaction fills one block's transient budget, about 0.5 KAS of fee |
| data withheld or lost | **open (give-up 2)** | anyone can rebuild the tree from the bundle's chain of transactions while they are available; after the pruning horizon only from whoever kept them. A bundle nobody can reconstruct is frozen. The horizon on TN10 is unknown to me |
| a script defect | **open (give-up 1)** | one template guards every member's KAS |

### 6.2 On option D (the template split)

| Attack | Verdict | Reason |
|---|---|---|
| a stake that is not a stake counted by the judge | **holds, to be tested** | the judge recognises a stake by its template hash (stored in the dispute at `open`, from the opener's own redeem script, which the claim already checks against its pinned stake template) and reconstructs its P2SH from prefix, state and suffix, as `stateOf` does today |
| a receipt redeemed beside something other than its pool | **holds by the induction, to be tested** | after the split the receipt cannot reconstruct the judge's script cheaply; it requires another input of its id, and every template under a claim id counts its inputs exactly (review 7, "What was checked and found sound"), so only `settle` admits receipts |
| a pin cycle (each template needs the other's hash) | **avoided** | the stake pins the judge's template hash (needed only at `open`, where the judge's prefix and suffix are passed as witness and hashed); the judge pins nothing about the stake (it reads the stake template hash from the dispute state) |
| `SINGLE|ANYONECANPAY` on unbond and withdraw | **holds as for commit (Z13)** | the signature fixes the input and the output at its index (script, value, binding); everything else is free, which is what batching needs |
| wide batches and the stack | **to be measured** | the loops keep a constant number of live items (today 97 of 244 at 7, IMPL 11); a larger unroll should not raise it, experiment 2 |

### 6.3 Capture threshold in numbers

The judge decides for the side that holds at least μ times the other's aged stake at the end of a round (IMPL 9).
With honest participation p_h of the honest stake, an attacker needs a fraction f* = p_h / (p_h + μ) of all aged
stake (DESIGN-ORACLE-2 1.7; Z10 measured it to 10^-4). At μ = 1.25: f* = 44.4 % at p_h = 1, 28.6 % at 0.5, 16.7 % at
0.25.

If keys that decide for others (A's leads, B's delegated vote keys) decide a fraction X of the aged stake and the
attacker controls those keys, the attacker needs own stake f > f* − X; at X ≥ f* it needs none.

| Distribution | Who must be corrupted to capture (μ = 1.25, delegates always vote: p_h = 1, f* = 44.4 %) |
|---|---|
| individual stakers, p_h = 0.25 (no delegation) | capital of 16.7 % of all aged stake |
| 10 equal leads holding all stake | 5 keys, no capital |
| 3 leads holding 60 / 25 / 15 % | 1 key |
| B, every member its own vote key | unchanged from individual stakers (16.7 % to 44.4 % capital, by participation) |

Delegation may raise p_h (passive members are counted), which raises f*; it also turns the question from "how much
capital" into "how many keys". B with own vote keys does not concentrate voting power. It concentrates custody
(6.1, script defect) and the timing point (6.1, deposit griefing).

A's lead (or B's delegate) can also take value: it commits the pool's counted value P to side X and its own aged
stake of at least ⌈μP⌉ to side Y. If nobody else answers, Y wins after the round and the lead's own receipt is paid
its stake plus P − φP: at φ = 10 %, 90 % of the members' committed value, at the cost of holding 1.25 P of aged stake
for a round. If X is the false side, honest stakers join Y and share the 0.9 P with the lead; the members lose P in
every case. **Broken for A by design; bounded by the members' committed value.**

## 7. What changes in the existing contracts (not implemented)

### 7.1 `KpfStake.sil` and the new `KpfJudge.sil` (option D, needed by B)

1. Split: `KpfStake` keeps statuses 4, 5, 6 and entries `commit`, `unbond`, `withdraw`, `redeem`; `KpfJudge` takes
   statuses 7, 8 and entries `deposit`, `resolve`, `settle`, `close`. `open` stays in the stake and receives the
   judge's prefix and suffix as witness, checks `templateHash(prefix, suffix)` against a pinned `JUDGE_TPL`, and builds
   the dispute's script from them (one large transaction per dispute). Alternative: let the claim's `contest` build
   the dispute; to be decided with `KpfPendingO`.
2. `checkJudge` stays one text in every template (the existing test that enforces it is extended).
3. `BATCH = 7` becomes `K_D` (deposit) and `K_S` (settle), set from experiment 2 (expected 55 to 125); optionally in
   two helper templates whose script public keys the judge pins.
4. Dispute state gains `stakeTpl` (32 bytes, written by `open` from the opener's own redeem script) and `bundleId` F
   (32 bytes, copied from the claim's market body). `stateOf` takes the stake template from `stakeTpl` instead of the
   judge's own redeem script.
5. `unbond` and `withdraw` accept `SIGHASH_SINGLE|ANYONECANPAY` (0x84) with the continuation or payout at the input's
   own index; `withdraw`'s payout script becomes the one at its own index.
6. `redeem` (receipt) requires another input of its id; it no longer reconstructs the pool with its own template.

### 7.2 For option B

7. Receipt state gains `units` (8 bytes) and a kind flag; for a bundle receipt the owner field is F.
8. `KpfJudge.deposit`: an optional bundle input b (id = the dispute's `bundleId`, at most one); reads (side, ΣA, ΣU,
   count) at fixed offsets of b's signature script; adds them to the tally; requires the bundle receipt at index b
   (value ≥ ΣA + reserve, the receipt state with `units` = ΣU); `OpCovOutputCount(id) == 1 + k + 1`.
9. `KpfJudge.settle`: for a bundle receipt, a return at its own index under the claim id (winner: value ≥ r + share;
   loser: value ≥ reserve, the pool keeps r − reserve); `OpCovOutputCount(id) == 1 + returns`; `wLeft` −= ΣU,
   `lLeft` −= ΣA as for stakes.
10. `KpfStake` gains status 10 (return) with entry `collect(b)`: requires input b of id F (from its state) running
    `collect` over a range covering it; and status 4 gains `join(b, sig)`: the owner's data signature over (join, F,
    voteKey, own outpoint), b running `join` over its range.
11. New: `KpfBundle` (entries `join`, `vote`, `collect`, `apply`, `exitRequest`, `payout`, `setVoteKey`, `spawn`;
    state as B.1; every entry `OpCovInputCount(F) == 1`, exact F output count, the stamp update), and the stateless
    verifiers `KpfBatchJoin`, `KpfBatchVote`, `KpfBatchApply`, `KpfBatchExit`.
12. Outside the judge: the market body gains F (`KpfPairO`, `KpfOpenO`, `KpfPendingO` hash it into the market id);
    `KpfPendingO.contest` and `decide` pin the new stake and judge templates; `judge.rs` models all of it;
    `oracle-check` verifies F's genesis from its preimage.

## 8. Experiments (script engine first; node only where stated)

Each must pass, with the stated expected result, before the design is believed.

1. **Slim stake size.** Compile `KpfStake` with only `commit`, `unbond`, `withdraw`, `redeem`, `open`. Expected: 1.0
   to 3.0 kB of code; `commit` about 105,000 to 120,000 units. Decides D's row in the table of option D.
2. **Wide deposit and settle.** `KpfJudge` with `K_D`, `K_S` ∈ {16, 32, 64, 96, 128}, loops inline and in helpers:
   bytes, compute mass, largest stack, per input. Expected: linear in K, about S + 0.5 kB + L per stake; the largest K
   under 250,000 B is 55 to 125; stack stays below 244; compute does not bind first.
3. **Batched bonding, unbonding and withdrawal.** 200 P2PK inputs signing `SINGLE|ANYONECANPAY` into 200 free stakes in
   one transaction; 64 unbonds and 64 withdrawals likewise. Expected: accepted; about 200 B per bond.
4. **Judge recognises the slim stake from `stakeTpl`**; a stake of another template with a valid-looking state is
   refused; a receipt beside every non-pool input of its id is refused. Expected: all refused except `settle`.
5. **Large push.** A P2SH spend whose signature script pushes a 100 kB blob that the script slices with
   `OpTxInputScriptSigSubstr` and `.slice`. Expected: accepted by the engine and by a TN10 node; charged 1 unit per
   pushed byte (fact 2).
6. **Merkle code.** Depth-12 path check plus update (old leaf to new leaf, same path) in SilverScript, unrolled for
   1, 8 and 64 leaves. Expected: about 30 to 40 B of code and about 320 units per level; about 400 to 1,000 B of code
   per leaf in total. Decides K_v, K_a and the join and exit batch sizes.
7. **Data signatures at scale.** 128 and 200 `checkMsgSig` in one input. Expected: 100,000 units each, no sig-op
   limit (Z1 measured up to 8: "not in the static sig-op bound"). If they are counted against the 15 per input, B's
   vote batch falls to 15 per verifier input and B needs about 10 verifier inputs per transaction (bytes about the
   same, design changes).
8. **Vote transaction.** Dispute + bundle + vote verifier + funder with K_v ∈ {32, 64, 128, 192}. Expected: largest
   K_v under 250,000 B and 500,000 compute mass is 110 to 200.
9. **F's genesis checked offline.** Recompute F from (outpoint, index, value, script of the empty bundle) as
   `covenant_id.rs` does. Expected: equal. Optional: whether `blake2bWithKey` reproduces the `CovenantID` hasher in
   script.
10. **Induction tests for F.** A genesis bundle under another id refused by the judge; two F inputs in one transaction
    refused by every bundle entry; a second F output with another script refused; a P2PK holding F impossible to
    create. Expected: all refused.
11. **Mempool contention.** On TN10: a chain of 5 bundle spends in one block interval; then a replacement of the first
    by a different spend of the same bundle. Expected: unknown. Decides the two "open" griefing rows of 6.1.
12. **Stamp monotonicity.** Read the consensus rule (or test on TN10) that a transaction spending another's output is
    never accepted at a lower DAA score. Expected: holds; if not, B.4 is unsound and must use per-leaf stamps.
13. **Stamp ring.** Model test: random spend rates, thinning by DAA spacing M_age / 8; check that no leaf ever counts
    before its true age and that the delay is at most the spacing. Expected: holds.
14. **Attack tests of 6.1 as engine runs**: forged age by witness; vote for the leading side; vote replayed in another
    claim and another round; the same leaf twice in a batch; ticket without the owner's signature; exit replayed after
    a rejoin; a return without id; a return spent beside `join`; the reserve and the units arithmetic (Σ per-leaf
    payouts ≤ the return; overflow at the bounds of `checkJudge`). Expected: every one refused or bounded as stated.
15. **Conservation run.** Extend `oracle_conservation_tests` with bundles: every accepted step conserves value per
    account (IMPL 25.3) with the bundle as one account, and a hostile builder's merges of bundle outputs are refused.
    Expected: no accepted merge.

## 9. Open questions

1. The compiled size of the slim stake and of the Merkle loop body (experiments 1 and 6). Every count in this note
   scales with them; the D and B estimates may be off by a factor of about 2 in either direction.
2. Whether a replacement of an unconfirmed spend is accepted by the v2.1.0 mempool in a way that lets a griefer keep
   a shared UTXO busy, and at what cost (experiment 11).
3. The pruning horizon of the TN10 node and of mainnet nodes, which bounds how long a bundle's history can be fetched
   from an ordinary node. I did not find it in the code I read.
4. Whether chained spends of one UTXO always get non-decreasing DAA scores (experiment 12).
5. Whether `checkMsgSig` stays outside the sig-op bound in every path (experiment 7; Z1 measured 8).
6. Whether `open` should move into the claim's `contest` (one less large witness, but `KpfPendingO` grows).
7. Whether the judge should accept more than one bundle per deposit (it would need several F-like ids or a proof that
   two bundles of one id cannot alias; not derived).
8. Whether `OpAuthOutputCount` / `OpAuthOutputIdx` (per-input authorized outputs, `covenants.rs:63-76`) could replace
   positional slots in the bundle's payouts; unused in KPF so far.
9. A's exit bookkeeping ("receipts open at the request") and the fairness of young members sharing losses in a pool:
   not derived, since A is not recommended.
10. The cost of the zero-knowledge verifiers under v2.1.0, should B's data and size limits make option E worth a look.
