# KPF design (draft 1)

Kaspa Perpetual Futures: a perpetual on the KAS price of a KCC-20 token, settled in KAS, enforced by covenants on L1.
Testnet-only research prototype. No operator, no admin key, no upgrade path. Threats: `THREAT_MODEL.md`.

**References.** `[L]` = the legacy KOB repository (`RossKU/KOB-legacy`, last commit 2026-04-10; dead code). `[K]` = the
current KOB repository (`github.com/RossKU/kob` at commit `ed16148`).
`path:line` points into those trees. Statements marked *(inference)* are not stated by the source. Items marked *(given)*
are the lead designer's research summary, not re-verified here.

## 1. Goals and non-goals

Goals.
- G1 Isolated-margin perpetual whose every state change is an L1 transaction checked by covenants only.
- G2 Fills atomic per transaction and parallel across users: no UTXO that every trade touches.
- G3 Every exit path works without the counterparty's signature; at least one exit needs no price evidence.
- G4 Price evidence only from KOB's touch rule (M1); no oracle, no matcher statement.
- G5 Partial fills on every path (founder's standing principle).
- G6 64-bit arithmetic that cannot overflow for stated parameter bounds, with conservation of KAS by construction.

Non-goals (M1): cross-margin, funding, a pool, external prices, mainnet, a new token, any operator role, guaranteed
liquidation liveness on a thin book (a measured risk, §6).

## 2. The founder's legacy model

### 2.1 Three generations

| Generation | Where | Pairing | Price | Status |
|---|---|---|---|---|
| Phase-5 concept | `[L] docs/plan.md:864-976` | one `perp_position` with both owners and both collaterals (`:880-892`) | spot `TradeReceipt`, two-step close with a delay so arbitrage corrects a fake print (`:915-941`) | never built |
| v6.3 "one UTXO per pair" | `[L] docs/PERP_DESIGN.md` | one UTXO locks both margins; `value = total_margin`, long share = `split_num/split_den` (`:5-7`, `:87-88`) | "price UTXO" at `input[1]`, **no identity check** (`:39-42`, `:101-102`) | 9 paths, 122 tests (`:9`) |
| "atomic settlement via spot co-spend" | `[L] kob-core/src/contract/perp/position.rs` | **two UTXOs, one per side**, each `value` = that side's margin (`:13-20`) | ratio of two spot outputs `sk/bt` (`:176-180`) next to two pinned spot orders (`:155-174`) | bytecode + builder only |

### 2.2 v6.3: how PnL is conserved

One UTXO holds `total_margin`. On close: `pnl = (price − entry_num)·size/entry_den`, `lg = clamp(margin_long + pnl, 0,
avail)`, `sg = avail − lg` (`[L] docs/PERP_DESIGN.md:103-111`): one side is computed, the other gets the remainder, so
rounding never leaks (`:248`). Profit is capped by the opponent's collateral (`[L] docs/plan.md:909`, `:967`). The split
ratio is immutable, so adding margin raises both sides' margins pro rata (`[L] docs/PERP_DESIGN.md:132-134`). No
funding: maturity settlement plus rollover spread replaces it (`:225-236`).

### 2.3 What the v6.2 audit found (`[L] docs/PERP_DESIGN.md:53-62`)

| Sev | Finding |
|---|---|
| CRITICAL | Permissionless ADL (path 9) let anyone drain profitable positions: it checked `bad_debt_share ≤ |pnl|` but never that bad debt existed (`:163-170`). Disabled. |
| CRITICAL | Unilateral partial close let the closer take residual margin. Made 2-of-2. |
| CRITICAL | Tracker PnL ignored `entry_den` for rational prices. |
| HIGH | Paths 2-8 read their own value at hardcoded `input[0]` (`Op0`): a position at another index could be drained. Fixed with `OpTxInputIndex` (`:60`, `:90`). |
| MEDIUM | Maintenance ratio ≥ 50 % allowed both sides underwater; builder now rejects it (`:61`, `:251`). |

Unfixed by design: the price is any UTXO's value at `input[1]` (`:39-42`, trust table `:177-194`). Whoever builds the
transaction chooses the price; v6.3 is custodial in effect for every price-dependent path.

The two pre-implementation threat models add: margin externalised to a vault conflicts with immutable state
(`[L] docs/v7-perp-threat-model.md:500-508`), liquidation must never need the loser's cooperation (`:306-318`), and ADL
target selection cannot be verified by any covenant (`[L] security/adl-insurance-threat-model.md:34-39`, `:469-477`).

### 2.4 The latest model (two side UTXOs), read from the bytecode

Intended shape: `in0` long side, `in1` short side, `in2/in3` spot buy/sell orders, outputs `0/1` payouts
(`[L] kob-core/src/contract/perp/position.rs:21-30`). State: owner, direction, size, entry, fees, bounds and the two
pinned spot script hashes (`:32-40`). Findings (this review, not the founder's audit):

- **F1 No binding between the sides.** The counterparty is "whatever input sits at `1 − self_idx`" (`:220-226`, asserted
  by the test `:1343-1349`); its script, covenant id and state are never read. Nothing in the state names a pair.
- **F2 Unconditional cancel.** Path 1 returns the whole side to its owner on the owner's signature alone, at any time
  (`:111-138`). A losing side simply leaves. The engine relies on it: "cancel the position and re-open"
  (`[L] kob-engine/src/matcher/perp_executor.rs:1060-1067`).
- **F3 Liquidation drains any side.** Path 3 checks `pay < (mm + cm)·mn/md` with the unauthenticated counter margin `cm`
  (`:355-375`) and has no output check at all (`:389-406`). A large dummy UTXO at `1 − self` makes every side
  "underwater" after `grace_daa`, and the keeper keeps the whole margin *(inference from the bytecode)*.
- **F4 Price from free outputs.** `sk`, `bt` are amounts of outputs 2 and 3 (`:176-180`), not bound to what the spot
  inputs filled *(inference)*; the spot inputs are pinned by script hash at creation (`:155-174`), so once those two
  orders are gone paths 2-4 are dead. `delta/bt` is divided before `× size` (`:258-277`).
- **F5 No pair-level conservation.** Each side checks only `output[self] ≥ own pay` (`:414-435`). Path 2 needs each
  side's own signature (`:444-469`): a "unilateral" close cannot spend the other side unless it is liquidatable or
  matured (`:476-492`).
- **F6 Never integrated.** The executor still builds the v6 single UTXO and a price UTXO at `input[1]`
  (`[L] kob-engine/src/matcher/perp_executor.rs:198-225`, `:448-457`); the tracker uses the v6 split
  (`[L] kob-engine/src/matcher/perp_tracker.rs:67-76`).

Verdict: the two-UTXO model as written is unsound (F1-F3 each lose a party's funds); it is repairable only with an
unforgeable pair identity (§3.4).

### 2.5 What carries over

Bilateral pairs with profit capped by the opponent's margin; conservation by "compute one side, remainder to the other";
own-index reads, never `Op0`; maintenance < 50 %; minimum keeper fee; the no-funding argument (`[L] docs/plan.md:866-875`);
the delayed-settlement idea, which KOB now implements as rest time `R` (`[K] docs/spec/matcher.md:745-768`).
Dropped: the free price UTXO, cancel, counterparty-by-index, maturity, ADL.

## 3. KPF architecture

### 3.1 The pair is a price range

A pair of size `s` (token base units) is fully described by two **bankruptcy prices** `BL < BS` (sompi per whole
token, `scale` base units, `[K] docs/spec/matcher.md:180-185`). With `c(p) = min(max(p, BL), BS)`:

    long payout(p)  = q(s, c(p) − BL)        short payout(p) = q(s, BS − c(p))        q = quoteOf
    pair value      = q(s, BS − BL) + C      (C = covenant carrier)

This is the v6 payoff (`margin_long + pnl`, clamped) rewritten: long margin at entry `E` is `q(s, E − BL)`, short margin
`q(s, BS − E)`, and `E` cancels from every payout. Consequences used everywhere below:
- re-marking a pair to a new entry `E'` changes **nothing on chain** (`BL`, `BS` are invariant); D3(c)'s margin move
  `(E' − E)·s` is exactly that identity;
- splitting is exact in rate form (both parts keep `BL`, `BS`); only `q` rounds, ≤ 1 sompi per part;
- all triggers compare prices with prices (§5), never `size × price`.
`E` is still stored: it is the last consented price, used only by the idle timeout (§4.3 P8) and for display.

### 3.2 D1 scope: accepted, with two issuer risks

Market = (token covenant id, token template, extension commitment, `scale`), the identity KOB evidence is checked
against (`[K] contracts/orders/KobCondAsk.sil:219-246`). Candidate first market: kUSDT on testnet-10. It is a
federated-bridge token with a guardian that can pause it (`[K] contracts/third-party/kusdt/UPSTREAM.md:5`); a token
program that refuses blocks every evidence fill (`[K] docs/spec/matcher.md:268-272`), so liquidations stop while it is
paused. Every issuer code change gives a new covenant id (`UPSTREAM.md:17`): a KPF market pinned to the old id then has
no evidence and only the evidence-free exits (§4.3). Linear in KAS: long gains `s·Δp/scale` sompi.

### 3.3 D2 price evidence: accepted, corrected in two places

Evidence = the fill, in the same transaction, of a plain resting `KobAsk`/`KobBid` of the market's token and `scale`,
not decaying, `interval ≥ 0`, exposed ≥ `R` DAA (`[K] docs/spec/matcher.md:683-743`, code
`[K] contracts/orders/KobCondAsk.sil:219-299`). Its quote is the conservative print (`matcher.md:703-706`):

| Evidence filled | Proves | Used by |
|---|---|---|
| resting **ask** at `a` | anyone could buy at `a`: price ≤ `a` (upper bound) | short's unilateral close; liquidation of a long; (later) long-side mark |
| resting **bid** at `b` | anyone could sell at `b`: price ≥ `b` (lower bound) | long's unilateral close; liquidation of a short; (later) short-side mark |
| both in one transaction | `b ≤ price ≤ a` | funding / mark-to-market (D4 ii), margin withdrawal of either side |

Rule: a price-dependent split is computed at the bound least favourable to the party that acts (or benefits), i.e.
most favourable to the party that did not consent. A long liquidated at an ask `a` is valued at a price ≥ the market;
a long closing on a bid `b` gets at most its fair value.

Corrections. (1) **Proportional to capital exposed, not to cost.** Both sides of the evidence may be one party
(`matcher.md:761-765`). A fake print costs the real opposite-side depth swept between the true and the fake quote
(matchers cross a mispriced resting order in the next tick, `matcher.md:274-287`) plus `k·n` exposed for `R` to whoever
takes it; with no arbitrageurs the second is free. Testnet-10 has no external market for the token, so resistance can
only be simulated (X9). (2) **Per transaction, not per position.** Evidence is "read, never consumed"
(`matcher.md:742-743`): `fill ≥ k × size` per position lets one print act on unboundedly many pairs. KPF requires
`n_ev ≥ ⌈k·Σ n_i⌉` over all pairs acting on that evidence (evidence block, §4.4). Evidence must be an accepted UTXO
(`matcher.md:1047-1054`).

### 3.4 D3 position representation: one UTXO per pair, not two

Recommendation: **refute "start from two side UTXOs" for M1; use one `KpfPair` UTXO per pair** holding both margins,
with mutable state windows (`[K] contracts/orders/KobCondAsk.sil:156-170`) instead of v6's immutable split.

Why. The two-UTXO model needs an unforgeable pair identity. A covenant cannot check anything at its creation, and a
genesis may carry extra outputs bound to the new covenant id, including plain keys that can later author more UTXOs with
that id (`[K] docs/spec/matcher.md:135-141`, `[K] docs/security/AUDIT-2026-10.md:238`). A shared pair id with
"exactly two inputs of the id" therefore admits a contaminated pair `{L, L′, S, S′}` whose owner sells `L` and later
settles `(L′, S)`, leaving the buyer against the dust side `S′`. Every buyer would have to verify the genesis off chain.
In one UTXO the counterparty's margin is physically in the same output, so a look-alike or a twin with the same id is
just another self-funded pair ("a look-alike can only give", `[K] docs/spec/matcher.md:978-982`); no genesis check is
needed. One UTXO also halves inputs, state reads and sigops per action.

Required properties:
- (a) **Atomic and parallel.** Each pair is its own UTXO; transactions conflict only when they touch the same pair.
- (b) **Exit without consent or evidence: transfer.** The leaving side's owner lists it (signed amend of its listing
  window). Any taker of the same side fills it: the leaver receives `q(n, X − BL)` (long) for `n` at price `X`; the
  entrant posts its own margin, i.e. a new `BL′`. The bystander (short) keeps `BS`. Economically the buyer pays the
  seller its equity at `X`; on chain the pair pays the leaver and the entrant's margin replaces it.
- (c) **Close-versus-close.** Leaving long `L1` of `P1 = (BL1, BS1)` and leaving short `S2` of `P2 = (BL2, BS2)` trade
  at `X`: `L1` gets `q(n, X − BL1)`, `S2` gets `q(n, BS2 − X)`, and the stayers form `P3 = (BL2, BS1)`. Conservation
  holds exactly: `(BS1 − BL1) + (BS2 − BL2) = (X − BL1) + (BS2 − X) + (BS1 − BL2)`. Open interest falls by `n`.
- (d) **Partial.** A pair splits into `(n, BL, BS)` and `(s − n, BL, BS)`; a party's two parts sum to its old payoff
  at every price, up to `q` rounding.

**The re-mark algebra (D3 c) is correct, but the stated loss is wrong.** Equity is invariant at every price only if no
clamp moves. The bystander short's payoff after the swap is `q(s, BS − c′(p))` with `c′` clamped at the new `BL′`; it
equals the old payoff for all `p` iff `BL′ ≤ BL`. If `BL′ > BL` the short loses `q(s, max(p, BL′) − max(p, BL))`; when the
current price is already below `BL′`, that loss is immediate and includes profit already accrued, not only "the cap on
further profit". The covenant cannot know `p` without evidence, so no softer rule is safe. **KPF rule (no-downgrade):**
any side replacement keeps the bystander's counterparty bankruptcy price at least as far: `BL′ ≤ BL` for a staying
short, `BS′ ≥ BS` for a staying long. Close-versus-close needs both `BL2 ≤ BL1` and `BS1 ≥ BS2`. With it the
bystander loses nothing at any price, and `X` (the "extreme `E′`") moves value only between the two consenting
leavers, each bounded by its own listing limit. Cost: close-versus-close is feasible only when the stayers are at
least as well margined as the leavers; otherwise leavers transfer or close unilaterally. A relaxation that admits
downgrades against two-sided evidence is open question Q3.

### 3.5 D4 funding: (i) for M1, and (ii) not recommended at all

(i) No funding; either side closes unilaterally at its adverse bound (§3.3). Pin: a short opened at `X` above the spot
ask closes at once on that ask and keeps `(X − a)·n` (mirrored for longs), so `X` stays within the spot spread plus
evidence cost. The passive side earns nothing for holding but, when forced out, is settled at the bound adverse to the
closer. Costs: a counterparty can end your position at will (`THREAT_MODEL.md` D); a close needs a spot fill of `k·n`,
i.e. token inventory or a natural crossing flow in a KOB batch.
(ii) Funding needs a two-sided price in the same transaction as **every** pair, every period (no shared index exists,
*given*), and a perp price from KPF fills, which two keys of one party print for free. In range form it is a shift of
`BL` and `BS` by `F/s`: easy arithmetic, unobtainable inputs. Recommendation: no funding in M1 or M2; revisit only if X9
shows the pin failing.

### 3.6 D5 liquidation

M1: permissionless, evidence per §3.3, partial allowed. Long liquidatable at ask `a` iff `a·(10⁴ − mmrBps) < BL·10⁴`;
short at bid `b` iff `BS·10⁴ < b·(10⁴ + mmrBps)`. Pro-rata splits keep the margin ratio, so a partial liquidation does
not cure the pair; it lets thin evidence liquidate in pieces of `n ≤ n_ev/k`.

Two M1 variants (both specified, §4.5 T6/T7):
- **close both** at the evidence quote (the lead's M1);
- **liquidate by transfer** (refinement, no pool): the loser's part is sold to any resting `KpfOpen` of the same side at
  `X ≥ a` (long) / `X ≤ b` (short), with no-downgrade for the winner. The winner continues; the open book is the backstop.

Pool-empty (D5 later): in a bilateral pair the shortfall is already assigned: the winner of that pair absorbs it,
because its payout is capped at `BL` (`[L] docs/plan.md:909`). A pool can only pay the winner the gap first; when the
pool is empty the M1 cap applies. **No ADL of third parties is needed, and none should be built:** the legacy ADL was
exploitable because nothing proved the bad debt existed (`[L] docs/PERP_DESIGN.md:163-170`), and no covenant can verify
whom ADL selects (`[L] security/adl-insurance-threat-model.md:34-39`). Pool requirements: spent only next to the
bankrupt pair it pays for (the proof, `:506-514`); claim ≤ proven gap; taken-over sides are ordinary sides with a
covenant owner; several pool UTXOs so liquidations do not serialise.

### 3.7 D6 cross-margin: feasible only as a different representation

One covenant id carrying several templates is very likely possible: a token id covers all its genesis outputs
(`[K] docs/spec/matcher.md:135-136`) and the kUSDT genesis group holds two templates
(`[K] contracts/third-party/kusdt/UPSTREAM.md:5`) *(inference; X12)*. Three holes remain:
1. A pair has two owners; one UTXO cannot sit in both owners' lineages. Cross-margin forces two side UTXOs bound across
   two lineages (counterparty account id + pair tag), i.e. a new contract set, not an extension of M1.
2. `OpCovInputCount(account)` counts the inputs of this transaction only; withdrawal needs the account state to hold the
   number of open sides, so every side action touches the account UTXO (per-user serialisation).
3. Account health needs evidence for **every** market the account holds, in one transaction (up to 8 evidence fills
   with their token custodies, each token bounded by its program's slots, `[K] docs/spec/matcher.md:330-333`). Liquidation
   liveness becomes the conjunction of 8 books. Measure before committing (X12).

### 3.8 D7 external prices: the bond secures the bond, and price control is custody control

- Attestation UTXOs give freshness (a member replaces its UTXO; the live one is the latest) but each can be spent once:
  consumers serialise on the committee's UTXOs, reintroducing a hot UTXO on every liquidation and close. Data signatures
  (`checkMsgSig` → `OpCheckSigFromStack`, `[K] vendor/silverscript/silverscript-lang/src/compiler/compile/expression/builtin.rs:38`)
  are reusable but uncosted (*given*) and have **no upper bound on age**: CLTV proves only `tx.daa ≥ lockTime`, so a stale
  signed price can be submitted later. Fixed membership (all `n ≤ 8` members, ≤ 15 sigops per P2SH input,
  `[K] docs/spec/matcher.md:351`) or a quorum the consumer cannot cherry-pick.
- Slashing needs an on-chain contradiction: equivocation (two signed prices for one epoch) is provable; a merely wrong
  price is provable only against KOB evidence, which exists only where KPF would not need a committee.
- A band limits the speed of a lie, not its size: a colluding majority walks the price at the band rate and liquidates
  every position, so "controls price only, never custody" is false in effect. The bond secures at most itself; OI would
  have to be capped against it, which needs shared state.

## 4. Milestone M1 specification

Every covenant enforces only its own parties' guarantees (KOB option 2, `[K] contracts/README.md:88`); crossing
spread, tips and rounding slack go to the executor (`[K] contracts/orders/KobIfdBid.sil:350-353`).

### 4.1 Contracts

| Contract | Role |
|---|---|
| `KpfPair` | one pair (position): both margins, both owners, both listings |
| `KpfOpen` | resting open order of one side (KAS escrow); takes an opposite `KpfOpen` (open) or a same-side listing (transfer, liquidation by transfer) |
| KOB `KobAsk` / `KobBid` (existing, v3) | evidence only; template hash, prefix, suffix pinned as constants, as KobCondAsk does (`[K] contracts/orders/KobCondAsk.sil:27-30`, `:191-246`) |

No other UTXO type. No registry, pool, or oracle in M1.

### 4.2 State layouts

`KpfPair` (mutable fields marked `*`; mutated by window replacement, `[K] contracts/orders/KobCondAsk.sil:156-170`).

| Field | Bytes | Meaning |
|---|---|---|
| tokenCovId, tokenTplHash, extensionCommitment | 32 ×3 | market token identity (evidence check) |
| tplPrefixLen, tplSuffixLen, scale | 8 ×3 | token template cut, base units per whole token |
| mmrBps, kBps, minRestDaa, liqFeeBps, keeperFeeMin | 8 ×5 | maintenance, evidence multiple `k`, rest `R`, liquidation fee, fee floor |
| minSize, carrier, idleDaa, priceMax | 8 ×4 | smallest part, `C`, idle timeout, upper price bound |
| size* | 8 | `s`, base units |
| BL*, BS* | 8 ×2 | bankruptcy prices, sompi per whole token, `0 ≤ BL < BS ≤ priceMax` |
| entry* | 8 | `E`, last consented price |
| longKey*, shortKey* | 32 ×2 | owners (x-only pubkeys) |
| longAsk*, longMin*, longTip* | 8 ×3 | long listing: sell ≥ `longAsk`, parts ≥ `longMin`, tip per whole token (`longAsk = 0`: unlisted) |
| shortBid*, shortMin*, shortTip* | 8 ×3 | short listing: buy back ≤ `shortBid` (`0`: unlisted) |
| tagL*, tagS* | 32 ×2 | covenant id of the input that put the current long / short into this UTXO in its creating transaction (0 if none) |

About 390 B of state *(estimate)*. The market block is identical in every pair and open order of a market; a reader
compares it byte for byte.

`KpfOpen`.

| Field | Bytes | Meaning |
|---|---|---|
| market block | as above | must equal the pair's |
| side | 8 | 1 long, 2 short |
| ownerKey | 32 | maker |
| sizeLeft* | 8 | base units |
| limit | 8 | long: entry ≤ `limit`; short: entry ≥ `limit` |
| myRate | 8 | own margin, sompi per whole token: long `BL = X − myRate` (≥ 0), short `BS = X + myRate` |
| minCounterRate | 8 | counterparty margin accepted: long needs `BS − X ≥` it, short `X − BL ≥` it |
| minFill, tip, activeFrom, expiryDaa | 8 ×4 | KOB semantics (`[K] docs/spec/matcher.md:206-228`) |

Escrow = value ≥ `q(sizeLeft, myRate + tip) + C + carrier`.

### 4.3 Spend paths of `KpfPair`

Every path: `OpCovInputCount(selfId) == 1` (`[K] contracts/orders/KobCondAsk.sil:310`, `[K] docs/spec/matcher.md:241`);
continuations located by `OpCovOutputIdx(selfId, ·)`, at most 2 (remainder + one new part); every continuation's state
equals this state outside the windows the path may change; `value ≥ q(s, BS − BL) + C` checked on every pair output.

| # | Path | Who | Evidence | Effect |
|---|---|---|---|---|
| P1 | amend | owner of the side (sig) | none | set own listing window; resets age |
| P2 | add margin | owner (sig) | none | `BL −= δ` or `BS += δ`, value grows ≥ `⌈q(s, δ)⌉` |
| P3 | fill listing: transfer | anyone | none | leaver paid; entrant's key, tag and new bound written; no-downgrade |
| P4 | fill listings: close-versus-close or mutual | anyone | none | §3.4 (c); same-pair crossing listings close both at `X` |
| P5 | unilateral close | owner (sig) | adverse bound | settle `n` at `b` (long) / `a` (short) |
| P6 | liquidate (close both) | anyone | victim-favourable bound | settle `n` at the quote, fee to keeper |
| P7 | liquidate by transfer | anyone | as P6 | loser's `n` sold to a `KpfOpen` at `X ≥ a` / `X ≤ b` |
| P8 | idle settle | anyone, `age ≥ idleDaa` (CSV, `KobCondAsk.sil:553`) | none | settle all at `E` |
| P9 | cooperative close | both owners (2 sigs) | none | any split (`[L] docs/PERP_DESIGN.md:94-97`) |

Not in M1: margin withdrawal (needs the adverse bound: bid for a long, ask for a short), funding, maturity.

`KpfOpen` paths: `fill` (anyone; the pair output it funds must carry `tagL`/`tagS == selfId`), `cancel` (owner sig),
`refund` (anyone at `min(expiryDaa, age 90 days)`, as `[K] contracts/orders/KobCondAsk.sil:343-349`).

### 4.4 Cross-input rules (the class the legacy audit found)

- **No fixed indices.** Every index is an argument; every read of another input is authenticated by template hash and
  P2SH (`[K] contracts/orders/KobCondAsk.sil:133-154`) or is a plain value read whose worst case only gives.
- **Party outputs are injective.** A pair at input `i` pays its first leaving party at output `i` and a second at
  output `M − 1 − i`, which must be ≥ the input count (so it is never another input's positional output); KOB uses the
  same reverse slots (`[K] docs/spec/matcher.md:334-340`). Requirements are `≥`; any excess is the executor's.
- **Hand-over outputs are claimed once.** An input that puts its party into a pair output it does not continue (a
  `KpfOpen`, or the pair whose stayer moves into another pair's continuation) requires that output's `tagL`/`tagS` to
  equal its own covenant id. Covenant ids are unique per input, so two inputs cannot both claim one output.
- **Evidence block.** All pairs that act on evidence input `ev` sit at inputs `[q, ev)`; each carries `(n_i, q)` as the
  first pushes of its signature script (8-byte `n_i`, as KOB evidence carries `n`, `KobCondAsk.sil:220-221`). Each such
  pair reads the first push of every input in `[q, ev)` (≤ 8, unrolled as the stray scan, `KobCondAsk.sil:172-189`),
  requires each to name the same `q` and `n_j ≥ 0`, and requires `n_ev ≥ ⌈k·Σ n_j⌉`. A pair outside the block that names
  `ev` must name a block containing an honest member with a different `q`, so it fails. Non-pair inputs in the block can
  only raise the sum. No template check is needed.
- **Fees.** Network fee and extra carriers come from the executor's own inputs (`[K] docs/spec/matcher.md:1062-1070`).

### 4.5 Transaction shapes

Indices are illustrative; covenants accept any order satisfying §4.4. `F` = executor funding (P2PK), `Ch` = change.

**T1 Open (KpfOpen long × KpfOpen short), `n` units at `X`.**
in: `0` KpfOpen L, `1` KpfOpen S, `2` F. out: `0` L continuation-or-refund, `1` S continuation-or-refund, `2` new
`KpfPair (n, BL = X − rL, BS = X + rS, E = X, tagL = id(L), tagS = id(S))`, `3` Ch.
The pair's covenant id comes from a genesis authored by `F`; neither order needs it fresh (a twin only gives), but each
requires `OpOutputCovenantId(2) ≠ 0` so the pair can ever be spent *(experiment X1)*.
Each order checks: own `n` (minFill rule), `X` within its limit, its own key and rate in its side, the counterparty's
rate ≥ `minCounterRate`, pair value ≥ `q(n, BS − BL) + C`, its tag, its escrow out ≥ left.

**T2 Transfer (listing of the long side × KpfOpen long), `n ≤ s` at `X ≥ longAsk`.**
in: `0` KpfPair P, `1` KpfOpen L′, `2` F. out: `0` leaver payout ≥ `q(n, X − BL) − q(n, longTip)` to `longKey`, `1`
L′ continuation-or-refund, `c1` P remainder `(s − n)` (if any), `c2` P part `(n, BL′ = X − rL′, BS, E = X, longKey = L′,
tagL = id(L′), shortKey, tagS)`, `3` Ch.
P checks: listing (`X ≥ longAsk`, `n ≥ longMin` or `n = s`), leaver payout, `BL′ ≤ BL` (no-downgrade), both
continuations, value of each ≥ its range. L′ checks its own guarantees as in T1.
A direct taker (P2PK signer) may replace L′ (`tagL = 0`).

**T3 Close-versus-close (long listing of P1 × short listing of P2), `n` at `X`, `longAsk1 ≤ X ≤ shortBid2`.**
in: `0` P1, `1` P2, `2` F. out: `0` L1 payout ≥ `q(n, X − BL1)` less tip, `1` S2 payout ≥ `q(n, BS2 − X)` less tip,
`c` P3 = P1 continuation `(n, BL2, BS1, E = X, longKey = key(L2), tagL = id(P2), shortKey = key(S1))`, remainders of P1
and P2 (if any), `3` Ch.
P1 checks: L1 payout; P3 found by its own id with `BS = BS1`, `shortKey = S1`, `BL ≤ BL1`, value. P2 checks: S2 payout;
P3 found by `tagL == id(P2)` with `BL = BL2`, `longKey = L2`, `BS ≥ BS2`, value. Neither reads the other's state.
**T3′ Mutual close** (both listings of one pair cross): in `0` P; out `0` long ≥ `q(n, X − BL)`, `M−1` short ≥
`q(n, BS − X)`.

**T4 Unilateral close by the long, `n` at bid `b`.**
in: `0` P (sig of `longKey`, first pushes `n, q = 0`), `1` evidence `KobBid` (fill `n_ev`), `2…` the bid's counterparty
(an ask with its custody, or the closer's own token custody per KOB), `F`. out: `0` long ≥ `q(n, c(b) − BL)`, `M−1`
short ≥ `q(n, BS − c(b))` (the remainder; short gets the rounding), P remainder, KOB outputs at their own indices, Ch.
P checks: signature, evidence identity and rest (`touch` logic), evidence block, `lockTime ≥ exposed + R` via CLTV.
Short closes mirrored with an ask `a` and its custody `tk`.

**T5 Add margin / T9 cooperative close / T10 amend.** One pair input with owner signature(s) (`δ` for T5), `F`.
T5: continuation with `BL − δ` or `BS + δ`, value ≥ old + `⌈q(s, δ)⌉`. T10 changes only the signer's listing window.

**T6 Liquidate a long (close both), `n` at ask `a`.**
in: block `[q, ev)` of liquidated pairs, `ev` evidence `KobAsk`, `tk` its custody, buyer side, `F`. out per pair `i`:
`i` long residual ≥ `q(n, c(a) − BL) − fee`, `M−1−i` short ≥ `q(n, BS − c(a))`, keeper output (argument index) ≥ `fee`,
remainder continuation. `fee = max(keeperFeeMin, q(n, ⌊a·liqFeeBps/10⁴⌋))`, taken from the long's residual first and
from the short's payout only for the part the residual lacks (the winner wants the liquidation done).
Check: `a·(10⁴ − mmrBps) < BL·10⁴`. Short mirrored with a bid.

**T7 Liquidate by transfer.** As T6, but the long's `n` goes to a `KpfOpen` long at `X ≥ a` exactly as T2 (`BL′ ≤ BL`),
the long receives `q(n, X − BL) − fee`, the short is untouched.

**T8 Idle timeout.** in: `0` P (age ≥ `idleDaa`), `F`. out: `0` long ≥ `q(s, c(E) − BL)`, `M−1` short ≥ the rest, Ch.

### 4.6 Rounding

`q` rounds down; on every split the side that did not act takes the remainder (`[L] docs/PERP_DESIGN.md:111`). Part
sizes: `n ≥ minSize` and `s − n ∈ {0} ∪ [minSize, ∞)`, so a pair is cut into at most `⌈s/minSize⌉` parts (KOB's
minimum-fill bound, `[K] docs/spec/matcher.md:221-228`). Every covenant output ≥ 0.08 KAS, plain payout ≥ 0.02 KAS
(`matcher.md:229-239`); a payout below dust is not created and its value stays in the remainder.

## 5. Arithmetic bounds (signed 64-bit, overflow fails the script)

`q(n, r) = ⌊n/scale⌋·r + (n mod scale)·⌊r/scale⌋ + ⌊((n mod scale)·(r mod scale) + c)/scale⌋`
(`[K] contracts/orders/KobCondAsk.sil:125-131`). With `priceMax = 10¹⁴` (1,000,000 KAS per token) and `scale ≤ 10⁹`:

| Expression | Largest term | Bound |
|---|---|---|
| `q(n, BS − BL)` | `⌊n/scale⌋·r` | `⌊n/scale⌋ ≤ 9.2·10⁴` whole tokens at `r = 10¹⁴`; `9.2·10⁹` at `r = 10⁹` |
| `(n mod scale)·(r mod scale)` | `< scale²` | `scale ≤ 3.03·10⁹` |
| liquidation triggers | `a·10⁴`, `b·(10⁴ + mmrBps)` | `≤ 10¹⁸` for `a, b ≤ priceMax`, `mmrBps < 5000` |
| fee rate | `a·liqFeeBps` | `≤ 10¹⁸` |
| evidence threshold | `⌈k·Σn/10⁴⌉` split as `⌊Σn/10⁴⌋·kBps + ⌈(Σn mod 10⁴)·kBps/10⁴⌉` | `Σn ≤ 8·maxSize`, `kBps ≤ 10⁵` |
| pair value | `q(s, BS − BL) + C` | ≤ total KAS supply (≈ 2.9·10¹⁸ sompi) |

No product `size × price` is ever formed. Every entry point checks `0 ≤ BL < BS ≤ priceMax`, `0 < n ≤ s`.

**Worked example.** Token `scale = 10⁶` (assumed 6 decimals), `C = 0.2 KAS`. Long and short open 1,000 tokens
(`n = 10⁹`) at `X = 10 KAS = 10⁹` sompi/token. Long rate 1 KAS (10 %): `BL = 9·10⁸`. Short rate 2 KAS: `BS = 1.2·10⁹`.
- Pair value `q(10⁹, 3·10⁸) = 1000·3·10⁸ = 3·10¹¹` sompi = 3,000 KAS (+ C): long margin 1,000, short 2,000. Largest
  intermediate `1000·3·10⁸ = 3·10¹¹`.
- `mmrBps = 500`. Long liquidatable at ask `a` iff `a·9500 < 9·10¹²`, i.e. `a ≤ 947,368,420` (9.4737 KAS): equity at
  9.47 = 470 KAS < 5 % of 9,470. Short liquidatable at bid `b` iff `1.2·10¹³ < b·10500`, i.e. `b ≥ 1,142,857,143`.
- `kBps = 10⁴` (k = 1): either liquidation needs a resting order of ≥ 1,000 tokens at the trigger, rested `R`.
- Close-versus-close with `P2 = (BL2 = 8.5, BS2 = 11.5)` KAS at `X = 10.2`: L1 gets `1000·1.2` = 1,200 KAS, S2 gets
  `1000·1.3` = 1,300 KAS, `P3 = (8.5, 12.0)` holds 3,500 KAS. 3,000 + 3,000 = 1,200 + 1,300 + 3,500. No-downgrade:
  `8.5 ≤ 9.0` (S1's counterparty deeper), `12.0 ≥ 11.5` (L2's counterparty deeper).

## 6. Open questions, most threatening first

| # | Question | Why it threatens the design |
|---|---|---|
| Q1 | Can liquidations find evidence of `k·n` in time? | Evidence must rest `R` and be filled for `k` times the size; a fast move on a thin book outruns it and the winner absorbs the gap. Bound: OI liquidatable per `R` ≤ depth/`k`. Solvency rests on this. |
| Q2 | What does manipulation really cost on a thin book? | Cost = opposite-side depth swept + exposure risk; with no arbitrageurs exposure is free. Sets `k`, `R`, per-pair size caps. |
| Q3 | Is strict no-downgrade too strict for OI to shrink? | If stayers are rarely better margined than leavers, close-versus-close almost never fills and exits fall back on transfers or evidence. A downgrade allowance against two-sided evidence is the only safe relaxation. |
| Q4 | Script budget of the worst shapes (T6 with 8 pairs, T3 with remainders) | Evidence read + block scan + two continuations + tag checks per input; KOB pair inputs already reach large budgets (`[K] docs/spec/matcher.md:1172-1180`). |
| Q5 | Covenant-id behaviour: genesis authored by a P2PK funder, `OpCovOutputCount == 2` continuations, outputs without an id | An id-less pair output would lock funds forever. |
| Q6 | Who runs KPF executors and keepers? | No operator; KOB executors would need KPF legs in their batches (arming precedent `matcher.md:770-812`). If nobody does, liquidations do not happen. |
| Q7 | Token issuer actions (pause, migration) | Freeze liquidations market-wide (§3.2). |
| Q8 | KOB template obsolescence | KPF pins KOB v3 evidence templates and has no upgrade path; if makers move to a new KOB template, evidence dries up. |
| Q9 | D6, D7 feasibility | §3.7, §3.8. |

## 7. Milestones

| M | Content | Change from D8 |
|---|---|---|
| M1 | `KpfPair` + `KpfOpen`, T1-T10, testnet-10, one market | adds liquidation by transfer (T7) as the backstop |
| M2 | backstop pool (claims only with the bankrupt pair spent in the same transaction), pool-empty = bilateral cap | **no funding** (S4) |
| M3 | cross-margin | new two-UTXO side representation; gated by X12 |
| M4 | bonded committee for markets without a KOB book | gated by X10 and an OI cap design |
| M5 | testnet UI | a minimal UI may be needed earlier to run X9 with real users |

## 8. Experiments required before the design can be trusted (testnet-10 or the KOB test engine)

| # | Experiment | Pass criterion |
|---|---|---|
| X1 | Pair genesis authored by a P2PK input; spend a pair output with and without a covenant id | id-less output is refused by the opening orders; the pair continues |
| X2 | `OpCovOutputCount(selfId) == 2` split continuation; later spend each part alone | both parts spendable; co-spend of both refused |
| X3 | Evidence block: 1, 2, 8 pairs on one evidence; mutations (pair outside block, mismatched `q`, negative `n`) | all mutations refused |
| X4 | Script units, bytes and fee of T4, T6 (1 and 8 pairs), T3 with remainders, on the 3/3 and public-mint token programs | within 500,000 compute and 250,000 bytes; fee table |
| X5 | Aliasing mutations: two `KpfOpen` of one key on one pair output; two pairs on one hand-over output; reverse slot below input count | all refused |
| X6 | Conservation fuzz of `q` rounding over splits, transfers, close-versus-close | no sompi created; ≤ 1 sompi slack per part |
| X7 | GhostDAG races: liquidation vs add margin vs transfer vs amend on one pair; liquidation vs evidence cancel | one accepted, no invalid state |
| X8 | Idle timeout via CSV; amend resets it | as specified |
| X9 | Manipulation drill: attacker fakes a low ask on a thin and on a seeded book; measure cost vs gain for `k`, `R` | parameters where gain < cost under stated depth |
| X10 | `checkMsgSig` cost, sigop counting, 8-signature median in one input | measured |
| X11 | Boundary vectors for `q`, triggers, `priceMax`, `scale = 10⁹` | no overflow below bounds; refusal above |
| X12 | One covenant id carrying two templates by continuation; 8-market evidence transaction size | measured |
| X13 | Token pause (kUSDT guardian) during open pairs | evidence paths refused; T2/T3/T8/T9 still work |
