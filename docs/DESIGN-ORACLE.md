# KPF bonded fast settlement and oracle markets (design note, draft 1)

Status: proposal, nothing implemented. Cited as `D§n` (`DESIGN.md`), `N§n` (`DESIGN-TWO-STEP.md`), `TM x`
(`THREAT_MODEL.md`). The covenant facts relied on are those the contracts already use: cross-input reads authenticated
by template hash and P2SH and hand-unrolled scans of at most 8 inputs (D§4.4), signed 64-bit arithmetic that fails on
overflow (D§5), lower bounds on time only (D§4.5c), at most 15 signature operations per P2SH input and an unused,
uncosted `checkMsgSig` (D§3.8, X10), no shared state (TM §5.3). One fact is used more than before: a UTXO's DAA score,
read by a later spend (`OpTxInputDaaScore`, `KpfPending.sil:304`, `:263`), is the exact score of the block that
accepted it. It gives no bound on *now*, but it does prove, after the fact, how late a transaction landed (§1.4).
Numbers marked *(estimate)* are not measured.

## 0. Findings

1. **Book-backed markets need no oracle.** Their judge already exists: counter-evidence from the book (D§4.5d). The
   bonded KAS that vouches for a fast settlement is a *guarantee*: it is the fund the wronged party is paid from when
   the book refutes the claim. Its safety never uses an honest majority; one guarantor with enough KAS is as good as
   many, and a Sybil is irrelevant. The fast path holds wherever the slow path holds, and it is harder to exploit (§3.1).
   What it costs: the guarantors carry the market's rebound risk over the window, and a liquidation victim loses its cure.
2. **Oracle-only markets** can only be judged by stake. With no global total, "majority" can only mean "the side that
   puts more aged bonded KAS into this dispute". With the lead factor `μ` of §2 an attacker wins once
   `f ≥ p_h/(p_h + μ)` (`p_h` = the share of honest stake that takes part). The per-window cap limits **speed, not
   size**: locks come back after the window, the oracle-priced margin has no cap (no global total), and a coalition that
   wins disputes takes all of it within a few windows (§3.2). The price-impact deterrent binds only when the attacker's
   locked KAS times the KAS price fall exceeds what it takes. It turns negative for an attacker who is short KAS.
3. **Changes to the decision** (§6): call 3a "guaranteed fast settlement" and build it first, without a bond
   registry or `U`; give 3b no unvouched path; count only *aged* stake in escalation and use a lead factor close to 1;
   burn the part of a slashed lock not owed to the victim; before 3b carries real value, replace the per-window cap by
   per-pair underwriting, which caps size (§7).

## 1. Mechanism

### 1.1 Objects and notation

`V` = value a claim can move = the part's range `q(n, BS − BL)` (it bounds both a close's mispayment and a forfeited
residual, D§3.1). `λ` = lock ratio (`λBps`, `10⁴ ≤ λBps ≤ 3·10⁴`). `D` = lock (dispute) window. `U` = unbonding delay.
`P` = the pending part's DAA score. `F` = the DAA score of the fast-settlement transaction (its locks' DAA score).

| Object | Market | Role |
|---|---|---|
| `KpfLock` (new, about 3.2 kB *(estimate)*, like `KpfPending`'s 3,203 B) | book-backed (3a) | a guarantee for one claim: KAS paid to the wronged party on refutation, released to its owner after `D` |
| `KpfPending` (+1 entry, +1 field) | 3a | `fastFinalize`: settles a pending part at once when locks covering `V` are created in the same transaction |
| `KpfStake` (new; one template, five statuses) | oracle-only (3b) | bonded stake: free, unbonding, locked (a vouch), dispute, receipt; it continues itself, so there is no template-hash cycle (as `KpfPair` continues itself) |
| `KpfPairO` (new) | 3b | `KpfPair` with the evidence reader of `propose` replaced by the lock check; settles in one transaction |

### 1.2 The bond (3b: `KpfStake`; 3a needs only `KpfLock`)

State: `ownerKey` 32, `status` 8; in status locked, the **claim block**: market hash (blake3 of the market block) 32,
claimant id (covenant id of the part) 32, `kind, side, n, bl, bs, p` 8 ×6, `t0` 8, `victimKey` 32, `cover, sumCover`
8 ×2. The dispute and receipt fields are in §2. Stake is counted in **units of 10 KAS** (`⌊value/10⁹⌋`): the total
supply is under `2.9·10⁹` units, so every pro-rata product `units × units` is below `8.4·10¹⁸` (D§5).

| Entry | From → to | Who | Rule |
|---|---|---|---|
| split / merge | free → free | owner | values conserved, each ≥ 1 unit; merge only of the same owner's slots |
| vouch | free → locked | owner (signature) | value ≥ `⌈cover·λBps/10⁴⌉` (split product, D§5); the claim block is fixed by the claimant, which checks it (§1.5) |
| release | locked → free | anyone, CSV age ≥ `D` | same owner, value ≥ own; never to a plain output, so vouched KAS waits `D + U` before it can leave |
| unbond / rebond | free ↔ unbonding | owner | the UTXO is re-created, so its age restarts |
| withdraw | unbonding → plain | owner, CSV age ≥ `U` | — |
| open / deposit / resolve / redeem | locked, free → dispute, receipt | anyone | §2 |

Slashing is not a separate entry: KAS is taken only from locks (by a dispute) and from losing receipts. A free or
unbonding slot is liable for nothing, because no on-chain proof can reference it. Status is checked on every spend,
and age is the UTXO's own age (CSV), never a stored time: a genesis can write any state (D§3.4, TM G4). A
look-alike slot is therefore real KAS that can only do what the template allows, and a forged "unbonding" status only
lets its author withdraw `U` after its own creation.

### 1.3 Carrying a voucher's approval into the settlement

**By spending (chosen).** Each voucher contributes one input. In 3a that is any KAS input, signed with SIGHASH_ALL,
which pays a `KpfLock` output. In 3b it is a free `KpfStake` slot whose `vouch` entry checks the owner's signature and
that its continuation is a locked slot. The approval is a transaction signature: one signature operation in the
voucher's own input, so the 15-per-input cap never binds. The claimant (the pending part or `KpfPairO`) reads the lock
*outputs*, not the inputs: an output's value is physical, whatever KAS funded it. Size per voucher: one input (about
150 B for a P2PK funding input; a `KpfStake` input carries its script, about 1.5 kB *(estimate)*) and one 43 B output
(P2SH 35 + value 8). The lock template's prefix and suffix are passed once as witness and authenticated by a build
constant `LOCK_TPL`, exactly as `propose` authenticates `PENDING_TPL` (`KpfPair.sil:1040`). Per claim there are at
most 8 locks (the scan).

**By data signature (rejected for item 2).** A `checkMsgSig` over a claim digest, verified in the claimant's input,
does not spend the bond. The same stake can then back any number of claims in parallel transactions, so "each
signature locks bond ≥ the value it moves" cannot be enforced. The most it gives is "a signer's whole stake is
slashable once for all its outstanding signatures". It is also limited to 14 signers per input (15 less the
claimant's own) and is uncosted (X10). It remains an option *inside* a voucher's own input: the slot verifies a message
signature instead of a transaction signature, which lets a coordinator build the transaction in one round. That
choice is worth making only if experiment Z1 prices `checkMsgSig` like `checkSig`.

**Contention.** A slot backs one claim at a time (it is spent). A voucher serving `c` claims at once holds `c` slots
of at least `λ·V_max` each. For example 50 slots of 3,300 KAS (`λ` = 1.1 on the D§5 pair) = 165,000 KAS. No UTXO is
shared between vouchers or between claims, so nothing serialises liquidations. Each voucher serialises only its own
slots.

### 1.4 What is signed; replay, reuse, delay

A SIGHASH_ALL signature covers every input outpoint (the pending part, whose script fixes `kind, side, n, bl, bs`, the
quote, the keys and `P`), every output (the payouts and each lock's claim block) and the lock time.
- **Replay**: the voucher's input is spent. **Reuse across parts**: the outpoint is signed. Parts of one split share a
  covenant id, but the lock's claim block names the id *and* the part's state, and two inputs of one id cannot meet
  (`OpCovInputCount == 1`). A message signature must name the slot outpoint, the part id, the part's state hash and `P`.
- **Delayed submission.** A signed settlement stays valid until one of its inputs is spent. The voucher revokes it by
  spending its own input. Honest voucher software revokes any signature not accepted within `Δ_v`. What lateness costs
  differs by market:
  - 3a: the dispute clock is `P`, the claim's own evidence time (§1.5). A late `F` only lengthens the guarantors'
    exposure `[P, F + D]`, so lateness harms them and not the victim.
  - 3b: the claim carries a signed lower bound `t0` (`P ≤ t0`, lock time ≥ `t0` by CLTV), and the locks' DAA is `F`,
    exact once accepted. **Rule: a claim is a statement about the market at `t0` if `F − t0 ≤ Δ_max`, otherwise
    about the market at `F`.** The dispute reads `F` from the lock it spends. A stale price submitted late is therefore
    judged at its landing time, which removes the look-back, even though no transaction can bound *now*.

### 1.5 Book-backed fast path (3a)

Unchanged: `propose` (T4/T6) creates the pending part with its evidence and bond `B`. New transaction **T14 fast
finalize**, any time before T13: in `0` the pending part `W` (`fastFinalize(o0, m, keys, covers, lockPre, lockSuf)`),
the guarantors' inputs, `F`; out the party payouts exactly as T13 (kind 1) or the forfeited part (kind 2), the
proposer `≥ B + fee` (kind 2), the lock block `[o0, o0 + m)`, change. `W` checks:
- `OpCovInputCount(selfId) == 1`; `P` = own DAA, `P ≥ 0`;
- **`P ≤ visibleUntil`**: the proposal's evidence was still fillable by conforming matchers when it was filled. This
  is one new pending field, written by `propose` as the order's earliest of `expiryDaa`, IOC/FOK kill and 90-day limit,
  which `propose` already reads (`KpfPending.sil:280-287`). It is checkable only now, because `P` is exact here; at
  propose it would need an upper bound on time. It shuts out dead orders (R3-F1) from the path that has no cure;
- the lock template's hash `== LOCK_TPL`. Each output `j` of the block is `P2SH(lockPre + common + key_j + cover_j +
  lockSuf)`, where `common` = this part's market block, id, claim, `P`, victim key and `Σcover`, rebuilt from own
  state. Its value is `≥ ⌈cover_j·λBps/10⁴⌉`. `Σcover ≥ V`. The block is disjoint from the party slots `i` and
  `M − 1 − i`;
- then T13's settlement without the CSV.

"Enough bond" is thus an **absolute amount fixed by the claim's own state** (`λ·V`), summed over at most 8 outputs of
this transaction. No total stake, voucher count or quorum fraction is involved, and none could be computed.

`KpfLock` entries:
- **dispute** (anyone): counter-evidence of the opposite kind, read and judged exactly as `KpfPending.challenge`
  (`KpfPending.sil:298-333`): clock `P`, rest `R`, life `L`, multiple `k_c`, margin `g`. All locks of the claim are
  spent together (`Σcover` present, one evidence block, `n` counted once). Lock `i` pays the wronged party its share
  `q(H, cover_i)` with divisor `sumCover` (in units), pays the disputer `β_i = ⌊cover_i·βBps/10⁴⌋`, and burns the
  rest to an unspendable output (Z7). Wronged party and `H`:

  | Claim | Counter quote | Wronged | `H` |
  |---|---|---|---|
  | liquidate long | bid `b` | long | `q(n, c(b) − bl)` |
  | liquidate short | ask `a` | short | `q(n, bs − c(a))` |
  | close long at `b0` | ask `a` | short | `q(n, c(b0) − c(a))` |
  | close short at `a0` | bid `b` | long | `q(n, c(b) − c(a0))` |

  `H ≤ V ≤ Σcover`, so the locks always cover it.
- **release** (anyone, CSV age ≥ `D`): the owner key `≥` the value. `D ≥ R` + watcher latency (testnet 600 DAA = `W`).
  As with W1, `D` is not a deadline: a dispute is valid until a release lands (N§5 g).

Why hold semantics (counter-evidence resting `R` after `P`) and not "the book at `F`": a dispute must fill its
counter-order, and any fill, even a minimum fill by the attacker, re-creates a KOB order with a new UTXO DAA score
(`KpfPending.sil:263-264`). Third-party orders that existed at `F` can therefore be erased for the cost of fees. A
defender's *own* fresh order with `minFill = k_c·n` cannot be erased cheaply.

### 1.6 Oracle-only fast path (3b)

`KpfPairO.settle(kind, side, n, p, t0, …)`: everything `propose` checks except the evidence (trigger, range rule, fee,
forfeit bound), then T13's settlement in the same transaction, provided the locks of this transaction cover `V` as in
§1.5. The locks are locked `KpfStake` slots whose claim block carries `p` and `t0`. There is no pending part, so
nothing freezes the pair while a dispute runs, and there is no cure. In compensation **the tolerance sits inside the
trigger**: a long is liquidatable only if `p·(10⁴ + g)/10⁴ · (10⁴ − m) < BL·10⁴` (two steps, each below `2·10¹⁸`), and
a close is paid at `p` moved by `g` against the closer. A quote that is wrong by less than `g` then never liquidates a
healthy pair (§4, slow bleed). **No unvouched path exists in 3b**: the slow path of 3a has no judge here, and a claim
backed by a small bond would escape the cap entirely (§3.2).

### 1.7 Who is paid what

| Branch | Wronged party | Guarantors / vouchers | Disputer | Proposer | Burn |
|---|---|---|---|---|---|
| 3a slow path | D§4.5d table, unchanged | — | — | — | — |
| 3a fast, undisputed | settled as T13 | lock back after `D` | — | `refund` (+ fee) at once | 0 |
| 3a fast, refuted | T13 settlement **plus `H`** | lose `Σ` locks | `Σβ_i` | keeps its payout | rest |
| 3b vouched, undisputed | T13 at `p` | locks → free stake after `D`, then `U` | — | as T13 | 0 |
| 3b, the claim side (YES) wins | — | stakes back + share of the NO pool less `φ` | loses its stake | — | `φ`·NO pool |
| 3b, the dispute side (NO) wins | `H(p′)` from the YES pool | lose locks and YES deposits | stake + share of the YES pool after `H`, less `φ` | — | `φ`·(YES pool − `H`) |

A refuted 3a claim is not undone: the winner keeps its part, and the wronged party is paid in cash from the locks.
Burning (`φ` in 3b, everything above `H + β` in 3a) is needed: without it a liar who sees a dispute coming disputes
itself first and loses only the restitution `H`, so lying becomes a free option.

### 1.8 Changes to existing contracts

`KpfPair`: `propose` writes `visibleUntil` (9 B, pending state 621 → 630 B), and the constant `PENDING_TPL` changes.
`KpfPending`: plus `fastFinalize`, plus the constants `LOCK_TPL`, `λBps`, `βBps`. These are deployment constants,
which keeps the market block (and `KpfOpen.checkMarket`) unchanged. `KpfOpen`: unchanged. New: `KpfLock` for 3a;
`KpfStake` and `KpfPairO` for 3b. The slow path, cure, challenge and every D§4.5d rule stay as they are.

## 2. The judge by stake (3b)

**Opening.** Within `D` of `F` (before a release lands), anyone spends **all** locks of a claim (`Σcover`, ≤ 8) plus
an opening deposit `d_1 ≥ ρ·Σa` (`ρ = 1`, any KAS) and states a counter-quote `p′` that contradicts `p` by `g` (the
D§4.5d inequalities with `p′` as the counter quote). The locks verify the shape of the genesis they enter: exactly one
dispute UTXO (status dispute) holding all stakes, with state = claim block, `F`, `p′`, totals `Y`, `N` (in units),
leader, and one receipt per depositor (status receipt: side, units, owner) under the dispute's covenant id. As in T1,
the spent covenants check the new id's outputs, so the author cannot mint extra receipts.

**Deposits.** Anyone adds stake to either side from free `KpfStake` slots **aged ≥ `M_age`** (CSV; at most 7 per
transaction). Each deposit re-creates the dispute UTXO, which restarts its age, and mints a receipt. The lead changes
only when the trailing side's total reaches `μ ×` the leader's total. Every deposit is at least `ε·(Y + N)`, which
bounds the number of age restarts to about `log_{1+ε}` of the stake involved.

**Resolution.** Anyone, once the dispute UTXO is `D_r` old (CSV: `D_r` without any deposit). The leader wins; a tie
(never reached under `μ ≥ 1`) keeps the claim. The payouts are §1.7. Winning receipts redeem one at a time against the
resolved dispute UTXO into free slots of their owners, so `U` applies to winnings. That serialises only this dispute.
Lower bounds only: `D` and `D_r` are earliest actions, not deadlines; a deposit that lands before the resolve is valid.

**What "majority" means.** Total stake is unknowable, so the judge measures only what each side *deploys* in this
dispute. The leader at the end is the side whose deployable capital is at least `μ` times the other's. With an
all-in attacker of aged stake `fB` and honest deployment `p_h(1 − f)B`, honest wins iff `p_h(1 − f) ≥ μf`, i.e.
`f < f* = p_h/(p_h + μ)`. `μ = 2` (plain doubling) hands the game to a third (`f* = 1/3` at `p_h = 1`). Recommended:
`μ = 1.25` (`f*` = 44 % at full participation, 29 % at half), with `ε = 0.1`.

**What stops a rich minority.** Only two things. (1) `M_age`: after the opening, stake must have been idle and bonded
for `M_age` (recommended `M_age ≥ U`), so capital raised for the occasion arrives too late. (2) Honest deployable
stake ≥ `μ ×` the minority's. Nothing else does. A minority richer than the *participating* honest stake wins.

**What honest vouchers must do and hold.** Keep an idle **aged reserve**: a slot's age restarts on every vouch, so
active vouching capital does not count. Across honest parties the reserve must exceed `μ` times the largest aged stake
an attacker could deploy *in parallel disputes*. The attacker chooses where and when to escalate, after honest stake
is committed elsewhere. Honest parties must also watch every claim within `D`, answer every lead change within
`D_r`, and judge `p` against `p′` at `τ` (`t0`, or `F` if late, §1.4), voting for the quote nearer the reference
price. Recommended windows: `D` = 6 h, `D_r` = 12 h, `U` = `M_age` = 7 days. A dispute ends after at most `D_r`
plus `D_r` per lead change; the wronged party is paid only then (the winner was paid at `F`).

**Nobody defends.** If the claim is false and nobody opens a dispute, it stands. The victim may open with plain KAS,
but needs `ρ·Σa ≈ λV`, i.e. its own margin again; otherwise it depends on watchers, whose prize is the YES pool less
`H` and `φ`. If the claim is true and the vouchers do not answer an opening, the disputer leads and wins: the vouchers
lose their locks and the "wronged" party (a deserving loser) is paid `H(p′)`. Vouchers are therefore liable for being
online for `D + D_r` after every vouch.

## 3. Numbers

### 3.1 Book-backed (3a)

`p_r` = probability that a false claim is refuted within the window (a live opposite book and a funded watcher). A
colluding winner and guarantors gain `H ≤ V` either way, because the winner keeps the part, and lose `λV` when refuted:
`EV = H − p_r·λV`. The attack pays iff `p_r < H/(λV) ≤ 1/λ`. Today's slow path, where a refuted claim gains nothing
and loses `B`: `EV = (1 − p_r)H − p_r·B`, which pays iff `p_r < H/(H + B)`.

| Path | `λ` / `B` | Attack pays when `p_r` below |
|---|---|---|
| slow (today) | `B` = 3 % of `V` | 97 % |
| fast | `λ` = 1.1 | 91 % |
| fast | `λ` = 1.5 | 67 % |
| fast | `λ` = 2 | 50 % |

The fast path is harder to exploit than the slow path whenever `λ > 1 + B/V`. It does not depend on `f`: an attacker
holding 100 % of all guarantee capital is still refuted by the book. Throughput is `A/λ` per `D` for an attacker
with capital `A` (no registry is needed). The total is unbounded over time only where the slow path is unbounded too,
i.e. on books with no refutation. Cost to honest guarantors: they lose `λV` whenever the market itself moves past the
refutation bound within `[P, F + D]`. The premium is about `Pr(move ≥ g′ in D)·λV`; on the D§5 pair (liquidation at
9.4, refuted at ≥ 9.66, +2.8 %) with `λ` = 1.1 and an assumed 1 % chance, it is 33 KAS on 3,000 (Z6). Refuted example:
counter bid 9.70, so `H` = 700 KAS to the long, `β` (10 %) = 300 KAS to the disputer, 2,300 KAS burned; the short
keeps its forfeited part.

### 3.2 Oracle-only (3b)

Per window `D`, a coalition can settle at most `fB/λ` of margin. That bounds speed only. Locks are released (or
returned by won disputes) and reused, while the oracle-priced margin `M_o` has no cap. Illustration: `B` = 10 M KAS
aged bonded, `λ` = 1.5, `μ` = 1.25, `D` = 6 h, `M_o` = 20 M KAS.

| `f` | attacker stake `fB` | per window `fB/λ` | wins vs `p_h` = 1 / 0.5 | windows to take `M_o` | price fall `δ` that would deter (`M_o/fB`) | largest `M_o` deterred at `δ` = 30 % |
|---|---|---|---|---|---|---|
| 10 % | 1.0 M | 0.67 M | no / no | 30 (7.5 days) | 20 (impossible) | 0.30 M |
| 34 % | 3.4 M | 2.27 M | no / **yes** | 8.8 (53 h) | 5.9 | 1.02 M |
| 51 % | 5.1 M | 3.40 M | **yes / yes** | 5.9 (35 h) | 3.9 | 1.53 M |
| 67 % | 6.7 M | 4.47 M | yes / yes | 4.5 (27 h) | 3.0 | 2.01 M |
| 100 % | 10 M | 6.67 M | yes / yes | 3 (18 h) | 2.0 | 3.00 M |

- **Losing coalition** (`f < f*`, disputes answered): every false claim costs its locks plus its escalation deposits,
  and the victim is paid `H`. Undisputed claims still pay. The watcher assumption of 3a applies in full.
- **Winning coalition**: it takes `M_o` at `fB/λ` per window. It also wins honest stake from every dispute it
  escalates, and needs an aged reserve of at least `μ ×` the honest deployment per dispute running in parallel. Honest
  parties can only exit, and an exit at a fair price needs the oracle (cooperative close, transfer and idle settle
  remain, TM K1).
- **Price impact.** Let `δ` be the fall of the KAS price caused by the attack. The attacker sells what it takes at
  once, while its bonded KAS (and other holdings `K`) stays exposed for `U`. Deterred iff `δ·(fB + K − Σ) > M_take`,
  where `Σ` is the size of a KAS short held through `U`. If the fall is proportional to the theft (`δ ≈ κ·M_take/S`,
  `S` the circulating KAS), deterrence needs `fB + K − Σ > S/κ`: the attacker would have to hold more than `1/κ` of all
  KAS. If, as the owner argues for a large venue, the fall is the venue's share `s_v` of KAS's value, deterrence needs
  `fB + K − Σ > M_take/s_v`, i.e. bonded stake several times the oracle-priced margin. No contract can enforce that,
  because no contract knows `M_o`.
- **Attacker short KAS.** With `Σ > fB + K` the fall *pays* the attacker `δ·(Σ − fB − K)` on top of the theft. Listed
  KAS perpetuals elsewhere, or borrowed KAS sold spot, supply `Σ`. A KAS/USD oracle market on this venue is worse: a
  short there is settled by the oracle the attacker controls, so it is simply part of `M_o`. Honest vouchers will hedge
  their `U` exposure for the same reason, so the deterrent is a cost anyone can remove at a hedging fee.

## 4. Attacks

Verdicts: **holds**, **fails**, or **holds only if** (the condition follows).

1. **False claim won by stake** (3b). **Fails** for `f ≥ f*`; **holds only if** honest aged deployment exceeds `μ ×` the
   attacker's in every dispute (§2). Not applicable to 3a (the book judges).
2. **Vouch, then unbond before the consequence.** Holds. A lock cannot unbond, only release after `D`. A lock that
   enters a dispute stays inside it until resolution. Released KAS returns as free stake and needs `U` more. A
   dispute that lands after `D` races the release (N§5 g), so watchers must act before `D` less latency.
3. **Bribery.** Bribing a guarantor in 3a buys only its lock: the book still refutes, and the bribe must exceed
   `p_r·λV`. To silence the watchers the briber must reach every possible watcher, and anyone with counter-evidence is
   one. **Holds only if** one unbribed watcher is live. In 3b, a bribe to *abstain* is cheap: abstaining costs the
   staker nothing but forgone winnings, and lowers `p_h` and with it `f*`. A credible p+ε commitment (a covenant that
   pays YES stakers if NO wins) is buildable. **Fails** against stakers who follow the bribe.
4. **Equivocation.** In 3a it does not matter (the book judges). In 3b two locks of one key on one market with
   `|t0 − t0′| ≤ ε` and quotes contradicting by `2g` are provable by spending both while both are locked (≤ `D`). Key
   splitting evades it. **Provable but weak**; not in a first version.
5. **Lazy vouching; the verifier's dilemma.** A lazy 3a guarantor carries the full refutation risk, so laziness is
   priced. A lazy 3b voucher signs what proposers bring and is slashed only if someone disputes. Watchers earn only when
   lies happen, so if lies are rare nobody watches, and then lies pay. **Holds only if** parties with their own stake in
   the outcome watch: the victim's wallet, and the winner as natural keeper. The design pays the victim `H` whoever
   disputes, so the victim's interest is direct.
6. **Griefing honest vouchers with frivolous disputes.** In 3a a dispute without valid counter-evidence is an invalid
   transaction and costs nothing. Exception: on a thin book the deserving loser self-fills a bid at the bound (B6) and
   takes `λV` instead of `B`. **Fails on thin books**, so guarantors must not vouch there, and the fast path is
   unavailable exactly where B6 already hurts. In 3b griefing costs `d_1 ≥ Σa`, paid to the vouchers. **Holds.**
7. **Disputes to freeze a counterparty.** Fast settlement has already paid the parties, and a dispute moves only locks
   and stakes. **Holds.** The 3a slow path keeps A9.
8. **Victim offline for `D`.** In 3a any watcher with counter-evidence disputes and the victim is paid at its key.
   **Holds only if** a watcher is live within `D`. In 3b likewise, with a stake (`d_1`). A victim offline beyond
   `D_r` cannot defend a dispute against its own position, but it has none left.
9. **A single whale.** In 3a it is a guarantor like any other: harmless. In 3b, if it holds `f ≥ f*` it *is* the
   oracle. "Decentralized" is then false.
10. **Sybil splitting.** Neutral. Only amounts count, never numbers of keys. It only evades the equivocation proof (4).
11. **Stale or delayed submission.** In 3a the clock is `P`, so lateness only extends the guarantors' own risk, and
    `P ≤ visibleUntil` excludes dead evidence. In 3b a claim landing more than `Δ_max` after `t0` is judged at `F`,
    which is exact (§1.4). **Holds**, given Z2. Honest vouchers revoke unused signatures within `Δ_v < Δ_max/2`.
12. **Vouched and challenged.** T14 and W1 spend the same pending part, so exactly one lands. If W1 lands first the part
    is restored and T14 is invalid as a whole (no lock is created). If T14 lands first, the challenger disputes the
    locks with the same counter-evidence on the same clock `P` and is paid `β`, while the victim is paid `H`. Vouching
    after a challenge is impossible, since the part is gone. Cure versus T14 is a race of the same kind. **Holds.**
13. **One bond for two claims in parallel.** Each lock is a distinct output funded in its own transaction, and a slot is
    one UTXO: two transactions spending it conflict. **Holds** by UTXO exclusivity. It would **fail** with data
    signatures that do not spend the bond (§1.3).
14. **Slow bleed below the dispute threshold** (3b). A quote biased by less than `g` is never disputable. With the
    tolerance inside the trigger (§1.6) a healthy pair is never liquidated by it. Closes lose up to `g·n` to the
    closer's own disadvantage: honest closers pay a spread of `g`. **Holds only if** the tolerance is inside the trigger;
    without it each pair within `g` of its trigger forfeits its whole maintenance slice, a theft far larger than `g·n`.
    3a has the same within-`g` residual today (D§4.5c, N§3), except that a T14 victim cannot cure.
15. **64-bit and scan caps.** `λBps ≤ 3·10⁴` keeps `⌈cover·λBps/10⁴⌉` in the split product. Pro-rata shares use
    10-KAS units. `Σ` over at most 8 locks never exceeds the supply. A claim has at most 8 locks, a deposit at most 7
    slots, and a dispute about 11 inputs (8 locks, counter-evidence, custody, funding). **Holds** (Z4, Z5 measure mass).
16. **A shared UTXO.** None: locks are per claim, slots per voucher, disputes per claim. A dispute's own UTXO
    serialises only its deposits and redemptions. **Holds.**
17. **Cure lost on the fast path** (3a, new). A victim within `g` above its trigger, facing an A1 print that survives
    `visibleUntil`, would cure on the slow path. On T14 it forfeits. **Fails, bounded**: the exposure is the
    maintenance slice of pairs within `g` of their trigger, at A1's cost (the depth between the market and the print).
    Allow T14 for liquidations only when the quote is at least `g` beyond the trigger, if Y6/X9 show A1 prints near the
    trigger to be cheap.

## 5. What can be said in public

**Book-backed markets, with guaranteed fast settlement.**
True: "no admin keys, no operator, no upgrade path"; "no custody: funds move only by covenant rules"; "no trusted
oracle: prices are fills on the KOB order book, and every claim can be refuted on chain by counter-evidence";
"optional fast settlement: a settlement is immediate when guarantors lock KAS at least `λ` times the value it moves;
if the book refutes it within the window, the wronged party is paid from that lock".
False: "decentralized oracle" (there is no oracle here); "manipulation-proof" (evidence is economic, TM T5); "insured"
or "guaranteed against loss" (only refuted claims are paid, and only within `D`); "faster liquidations are as safe on
any book" (not on books without arbitrage, attack 6).

**Oracle-only markets.**
True: "no admin keys"; "anyone may bond KAS and take part"; "prices are set by bonded participants, and disputes are
decided by the side that commits more aged bonded KAS"; "secure only while honest participants who take part commit
more than `μ` times any attacker"; "at most `fB/λ` of margin can be settled per `D` window by a fraction `f` of bonded
stake". False: "no trusted oracle" (the oracle *is* the trust assumption); "no custody" in effect (price control is
custody control, D§3.8); "your funds are protected up to the bond" (the cap is per window, the total is not capped);
"liars are always slashed" (only when disputed and outvoted); "secured by the value of KAS" (§3.2: negative against a
short).

## 6. Verdict, staging, experiments

**Verdict.** 3a holds: it holds wherever the slow path holds (a live opposite book with arbitrage within `R`, TM T5,
and a live watcher within `D`). It is strictly harder to exploit for `λ > 1 + B/V`, needs no honest-majority
assumption, and costs the guarantors a premium for rebound risk. Remaining exposure: thin books (attack 6), victims
who would have cured (17), and the watcher assumption. 3b holds only under all of: honest *participating* aged stake
`> μ ×` the attacker's in every dispute; a funded watcher per claim within `D`; the tolerance inside the trigger; no
bribed abstention. Even then its loss is not bounded by the bond: a coalition that crosses `f*` takes all
oracle-priced margin within `M_o·λ/(fB)` windows, and the price-impact deterrent cannot be enforced or relied on.

**Changes to the decision.** (1) 3a needs no bond registry, no `U` and no honest majority: guarantors fund locks from
any KAS. Present it as guaranteed fast settlement, not as an oracle. (2) 3b has no unvouched path. (3) Escalation
counts only aged stake (`M_age`) with `μ ≈ 1.25`, not doubling. (4) Burn the part of a slashed lock not owed to the
victim. (5) Replace the per-window cap by per-pair underwriting (§7) before 3b carries more than test value.

**Smallest first version (agreed lean, narrowed).** 3a, one guarantor lock per claim (`m = 1`, so no lock block),
`λ` = 1.1, `β` = 10 % of `V`, `D = W`; `visibleUntil`; one `KpfLock` template with dispute and release. No
`KpfStake`, no 3b. Then the multi-lock block (`m ≤ 8`). 3b only after Z6–Z9 and §7.

**Experiments** (before building; offline unless noted).

| # | Experiment | Pass criterion |
|---|---|---|
| Z1 | `checkMsgSig`: script units, sigop count, 1 and 14 in one input (X10) | priced like `checkSig`; otherwise keep transaction signatures |
| Z2 | `OpTxInputDaaScore` of a lock equals its accepting block's DAA; CLTV of `t0`; a late `F` detected | exact; `F − t0` reproducible from the chain |
| Z3 | SIGHASH flags a covenant can see and require (ALL; ALL with ANYONECANPAY for one-round assembly) | measured; default ALL |
| Z4 | T14 mass with 1 and 8 locks (8 lock-script hashes of about 3.2 kB) | within 500,000 compute mass |
| Z5 | lock dispute with counter-evidence and 8 locks; burn output relayed | fits; burn is standard or documented |
| Z6 | rebound statistics: `Pr(move ≥ g′ within D)` on recorded KOB flow | premium per `V` tabulated for `g`, `D` |
| Z7 | burn output: unspendable script accepted by mempool and consensus | accepted, else document fee burn (miner capture) |
| Z8 | GhostDAG race: revocation against a signed T14; T14 against W1; release against dispute at `D` | exactly one lands, no invalid state |
| Z9 | `KpfStake` dispute: opening shape, deposits, `μ` lead rule, redemption in 10-KAS units, conservation fuzz | no sompi created; receipts not mintable outside entries |
| Z10 | watcher latency for lock disputes (Y5) | dispute lands before `D` in ≥ 99 % |

## 7. A better mechanism for oracle-only markets: per-pair underwriting

The flaw of §3.2 is that the bond caps speed, while what needs a cap is the total margin the oracle can steal, and no
contract can know that total. UTXO exclusivity can bound it without a global counter. **Every oracle-market pair is
underwritten for its lifetime** by one locked stake slot of value `≥ λ′·q(s, BS − BL)`, chosen at open by both orders
(as `KpfOpen` checks the new pair today) and named in the pair's state. The slot names the pair. Every price-dependent
settlement of the pair spends its underwriting slot, which carries the claim and is disputable as in §2. The slot is
released only when the pair closes, or moves with a transfer. A slot underwrites one pair at a time, so **oracle-priced
margin ≤ underwriting stake / `λ′`**, structurally. The price-impact condition of §3.2 then becomes a parameter:
`λ′ ≥ 1/(f·δ)` (`λ′` ≥ 6.5 at `f` = 51 %, `δ` = 30 %). That is expensive, but it is a bound, and bonded KAS grows
with the market instead of lagging it. Underwriters charge the parties a premium. The stake judge, its `f*` and its
liveness duties are unchanged: underwriting bounds the loss; it does not make the oracle honest. If oracle-only
markets are pursued, this, rather than the per-window cap, is the form to design in detail.
