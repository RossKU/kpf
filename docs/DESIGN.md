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
| resting **ask** at `a` | anyone could buy at `a`: price ≤ `a` (upper bound) | short's unilateral close; liquidation of a long; counter-evidence against a long's close or a short's liquidation (§4.5d); (later) long-side mark |
| resting **bid** at `b` | anyone could sell at `b`: price ≥ `b` (lower bound) | long's unilateral close; liquidation of a short; counter-evidence against a short's close or a long's liquidation (§4.5d); (later) short-side mark |
| both in one transaction | `b ≤ price ≤ a` | funding / mark-to-market (D4 ii), margin withdrawal of either side |

Rule: a price-dependent split is computed at the bound least favourable to the party that acts (or benefits), i.e.
most favourable to the party that did not consent. A long closing on a bid `b` gets at most its fair value. A
liquidation does not split at the quote at all (§4.5b): the quote only proves the trigger, and the loser forfeits its
residual, because the loser can itself print a free quote on the wrong side of the market (review 2, F1).
Since §4.5d no evidence settles in the transaction that fills it: the claim (close or liquidation) is **proposed** with
a bond, and for a window `W` anyone may refute it with fresh evidence of the opposite kind that contradicts it by a
margin `g`; only an unrefuted claim settles, at **finalize**.

Corrections. (1) **Proportional to capital exposed, not to cost.** Both sides of the evidence may be one party
(`matcher.md:761-765`). A fake print costs the real opposite-side depth swept between the true and the fake quote
(matchers cross a mispriced resting order in the next tick, `matcher.md:274-287`) plus `k·n` exposed for `R` to whoever
takes it; with no arbitrageurs the second is free. Testnet-10 has no external market for the token, so resistance can
only be simulated (X9). (2) **Per transaction, not per position.** Evidence is "read, never consumed"
(`matcher.md:742-743`): `fill ≥ k × size` per position lets one print act on unboundedly many pairs. KPF requires
`n_ev ≥ ⌈k·Σ n_i⌉` over all pairs acting on that evidence (evidence block, §4.4). Evidence must be an accepted UTXO
(`matcher.md:1047-1054`) and must have been fillable by conforming matchers for the whole rest window: not past its
`expiryDaa`, its IOC/FOK kill or 90 days idle before `exposed + R` (§4.5b, review 2 F2), with the rest counted only on
the pair's own evidence clock: `exposed' = max(exposed, S)` (§4.5c, review 3 F1), and it must stay fillable for the
market's evidence life `L` after its rest (§4.5d). Counter-evidence is read the same way with the pending part's own
DAA score `P` as the clock (§4.5d).

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
i.e. token inventory or a natural crossing flow in a KOB batch, plus a bond locked for the window `W`; it settles only
at finalize, and a fresh opposite quote beyond the closer's by `g` refutes it (§4.5d).
(ii) Funding needs a two-sided price in the same transaction as **every** pair, every period (no shared index exists,
*given*), and a perp price from KPF fills, which two keys of one party print for free. In range form it is a shift of
`BL` and `BS` by `F/s`: easy arithmetic, unobtainable inputs. Recommendation: no funding in M1 or M2; revisit only if X9
shows the pin failing.

### 3.6 D5 liquidation

M1: permissionless, evidence per §3.3, partial allowed. Long liquidatable at ask `a` iff `a·(10⁴ − mmrBps) < BL·10⁴`;
short at bid `b` iff `BS·10⁴ < b·(10⁴ + mmrBps)`. Pro-rata splits keep the margin ratio, so a partial liquidation does
not cure the pair; it lets thin evidence liquidate in pieces of `n ≤ n_ev/k`.

One M1 variant since §4.5d: **forfeit and keep, after a window.** A keeper proposes the liquidation (P6, §4.5 T6)
with a bond; the part waits `W` DAA as a `KpfPending`, during which anyone may refute it with fresh counter-evidence
(the part returns to the pair, the bond to the challenger) and the loser may cure it by moving its bound to the cure
bound. Unrefuted, it finalizes: the loser receives nothing (the quote decides only whether the trigger holds), the
proposer receives its bond and the keeper fee, and the winner keeps both sides of the part, i.e. the pending part's
value less the bond and the fee (the part's own value less the fee), with no price exposure. The winner then closes it with its own two signatures (P9) or sells
the forfeited side by an ordinary listing and transfer (P1 + P3) after the hand-over limit.
Superseded by §4.5d: the two variants of §4.5 T6/T7 (close both: the winner paid out in the evidence transaction;
**liquidation by transfer**, P7: the loser's part sold to a resting `KpfOpen` in the evidence transaction). P7 is
removed; its outcome is now forfeit and keep followed by an ordinary P3.

Pool-empty (D5 later): in a bilateral pair the shortfall is already assigned: the winner of that pair absorbs it,
because its payout is capped at `BL` (`[L] docs/plan.md:909`). A pool can only pay the winner the gap first; when the
pool is empty the M1 cap applies. **No ADL of third parties is needed, and none should be built:** the legacy ADL was
exploitable because nothing proved the bad debt existed (`[L] docs/PERP_DESIGN.md:163-170`), and no covenant can verify
whom ADL selects (`[L] security/adl-insurance-threat-model.md:34-39`). Pool requirements: spent only next to the
bankrupt pair it pays for (the proof, `:506-514`); claim ≤ proven gap; taken-over sides are ordinary sides with a
covenant owner; several pool UTXOs so liquidations do not serialise.

### 3.7 D6 cross-margin: feasible only as a different representation

One covenant id can carry several templates: continuation compares ids only, never scripts (experiment X12,
`crates/kpf-tests`). Three holes remain:
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
| `KpfPair` | one pair (position): both margins, both owners, both listings; `propose` moves a part into `KpfPending` |
| `KpfPending` | a part of a pair against which a claim on price evidence is pending (§4.5d): challenge, cure, finalize; a second template under the pair's covenant id (one id, two templates, X12); pinned in `KpfPair` as `PENDING_TPL` |
| `KpfOpen` | resting open order of one side (KAS escrow); takes an opposite `KpfOpen` (open) or a same-side listing (transfer, P3). No liquidation by transfer (removed, §4.5d) |
| `KpfLock` | a guarantor's lock of a guaranteed fast settlement (§4.5f): created by `KpfPending.guarantee`, disputed with counter-evidence (the wronged party paid, the rest burned) or released to its owner after `D`; pinned in `KpfPending` as `LOCK_TPL`; no covenant id |
| KOB `KobAsk` / `KobBid` (existing, v3) | evidence and counter-evidence only; template hash, prefix, suffix pinned as constants, as KobCondAsk does (`[K] contracts/orders/KobCondAsk.sil:27-30`, `:191-246`) |

No other UTXO type. No registry, pool, or oracle in M1. `KpfPending` never needs the pair's template: `propose`
precomputes the pair states it may return to (§4.5d), so there is no template-hash cycle; `KpfLock` needs neither
(§4.5f): the pin graph is `KpfOpen → KpfPair → KpfPending → KpfLock`.

### 4.2 State layouts

`KpfPair` (mutable fields marked `*`; mutated by window replacement, `[K] contracts/orders/KobCondAsk.sil:156-170`).

| Field | Bytes | Meaning |
|---|---|---|
| tokenCovId, tokenTplHash, extensionCommitment | 32 ×3 | market token identity (evidence check) |
| tplPrefixLen, tplSuffixLen, scale | 8 ×3 | token template cut, base units per whole token |
| mmrBps, kBps, minRestDaa, liqFeeBps, keeperFeeMin | 8 ×5 | maintenance, evidence multiple `k`, rest `R`, liquidation fee, fee floor |
| minSize, carrier, idleDaa, priceMax | 8 ×4 | smallest part, `C`, idle timeout, upper price bound |
| bondBps, bondMin, kcBps, gBps, challengeDaa, minEvidenceLife | 8 ×6 | two-step evidence (§4.5d): bond rate and floor, counter-evidence multiple `k_c`, contradiction margin `g`, window `W`, evidence life `L` |
| lockBps, disputeDaa, graceDaa | 8 ×3 | guaranteed fast settlement (§4.5f): lock ratio `λ`, dispute window `D` after a fast settlement, cure grace `G` of a fast liquidation |
| size* | 8 | `s`, base units |
| BL*, BS* | 8 ×2 | bankruptcy prices, sompi per whole token, `0 ≤ BL < BS ≤ priceMax` |
| entry* | 8 | `E`, last price both parties consented to (§4.5a) |
| longKey*, shortKey* | 32 ×2 | owners (x-only pubkeys) |
| longAsk*, longMin*, longTip* | 8 ×3 | long listing: sell ≥ `longAsk`, parts ≥ `longMin`, tip per whole token (`longAsk = 0`: unlisted) |
| shortBid*, shortMin*, shortTip* | 8 ×3 | short listing: buy back ≤ `shortBid` (`0`: unlisted) |
| tagL*, tagS* | 32 ×2 | covenant id of the input that handed the long / short over in the transaction that created this UTXO (0 if none; no entry copies it, §4.4) |
| since* | 8 | evidence clock start `S` (0: the DAA score of this UTXO); kept by every output that changes no hands, 0 on a part that changes hands (§4.5c) |

519 B of state, each field with its push byte (the market block 288 B, state `[0..288)`, redeem bytes `[1..289)`;
the position 231 B, unchanged; `KpfOpen` 402 B; `KpfPending` 666 B; `KpfLock` 468 B; measured, `kpf_layout_tests`).
Before §4.5f: 492 B (market block 261 B), `KpfOpen` 375 B, `KpfPending` 621 B; before §4.5d: 438 B (market block 207 B)
and `KpfOpen` 321 B. Redeem offsets of the pair: `size` 290, `BL` 299, `BS` 308, entry payload 317, keys 326 and 359,
listings `[391, 445)`, tags 446 and 479, `since` push 511, state end 520. The market block is identical in every pair,
open order, pending part and lock of a market; a reader compares it byte for byte.

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

Escrow = value ≥ `q(sizeLeft, myRate + tip) + C + carrier`. The market block starts the state (`MARKET_LEN` 261);
`sizeLeft` is at redeem offset 305.

`KpfPending` (written by `KpfPair.propose`; nothing is mutable).

| Field | Bytes | Meaning |
|---|---|---|
| market block | 261 | the pair's, byte for byte |
| kind, side | 8 ×2 | `kind` 1 close (P5), 2 liquidation (P6); `side` the closer (kind 1) or the loser (kind 2) |
| n, bl, bs | 8 ×3 | size of the part and its bounds |
| quote | 8 | the proposal's evidence quote `p` |
| cureBound | 8 | kind 2: the loser's bound after a cure (0: no cure) |
| fee | 8 | kind 2: keeper fee, to the proposer at finalize |
| bond, refund | 8 ×2 | `B`; `refund` = `B`, plus the carrier `C` the proposer funded when `n < s` |
| visibleUntil, offer | 8 ×2 | §4.5f: the last DAA at which the proposal's evidence order was fillable by conforming matchers (a guarantee needs `P ≤` it); what the proposer gives up from its payout to a guarantor (`0 ≤ offer ≤ B − 0.02 KAS`) |
| entry, longKey, shortKey, listings | 8, 32 ×2, 8 ×6 | the pair's, copied as one slice of the pair's script |
| since | 8 | the pair's clock `S` at the proposal |
| proposerKey | 32 | receives `refund` (cure) or `B + fee` (finalize of a liquidation); the closer's key for kind 1 |
| restoreHash, cureHash, forfeitHash | 32 ×3 | P2SH script hashes of the pair states the part may become (§4.5d; `cureHash` 0: no cure) |

666 B (621 B before §4.5f; the note estimated about 420). `KpfPending` reads only the market block, the claim, the keys and the hashes;
the entry, the listings and `since` make the state self-describing: a tool rebuilds the three pair states from the
pending UTXO alone and checks them against the hashes.

### 4.3 Spend paths of `KpfPair`

Every path: `OpCovInputCount(selfId) == 1` (`[K] contracts/orders/KobCondAsk.sil:310`, `[K] docs/spec/matcher.md:241`);
continuations located by `OpCovOutputIdx(selfId, ·)`, at most 2 (remainder + one new part); every continuation's state
equals this state outside the windows the path may change; `value ≥ q(s, BS − BL) + C` checked on every pair output.

| # | Path | Who | Evidence | Effect |
|---|---|---|---|---|
| P1 | amend | owner of the side (sig) | none | set own listing window; resets age |
| P2 | add margin | owner (sig) | none | `BL −= δ` or `BS += δ`, value grows ≥ `⌈q(s, δ)⌉` |
| P3 | fill listing: transfer | anyone, `tx.daa ≥ S + 4R` | none | leaver paid; entrant's key, tag and new bound written; no-downgrade; entrant healthy at `X` |
| P4 | fill listings: close-versus-close or mutual | anyone (close-versus-close: `tx.daa ≥ S + 4R` on each pair) | none | §3.4 (c); same-pair crossing listings close both at `X` |
| P5 | unilateral close: `propose` kind 1 | owner (sig) | adverse bound | part `n` into `KpfPending` with bond `B`; settles at finalize at `c(b)` (long) / `c(a)` (short) unless refuted (§4.5d) |
| P6 | liquidate: `propose` kind 2 | anyone (proposer key `pk`) | trigger only (ask for a long, bid for a short), range wider than both bands | part `n` into `KpfPending` with bond `B`; at finalize forfeit and keep: the winner holds both sides, the proposer gets `B` + fee (§4.5d) |
| P7 | removed (§4.5d) | | | was liquidation by transfer; now P6 then an ordinary P1 + P3 by the winner |
| P8 | idle settle | anyone, `age ≥ idleDaa` (CSV, `KobCondAsk.sil:553`) | none | settle all at `E` |
| P9 | cooperative close | both owners (2 sigs) | none | any split (`[L] docs/PERP_DESIGN.md:94-97`) |

Not in M1: margin withdrawal (needs the adverse bound: bid for a long, ask for a short), funding, maturity.

`KpfPending` paths (§4.5d), each with `OpCovInputCount(selfId) == 1`:

| # | Path | Who | Evidence | Effect |
|---|---|---|---|---|
| W1 | challenge | anyone | counter-evidence of the opposite kind, rested `R` on the clock `P`, contradicting by `g` | one output of the id, P2SH(`restoreHash`), value `≥ v − B`; the bond is the challenge executor's |
| W2 | cure (kind 2 only) | the loser's owner (sig) | none | one output P2SH(`cureHash`) with the added margin; proposer repaid `≥ refund` |
| W3 | finalize | anyone, `age ≥ challengeDaa` (CSV) | none | kind 1: payouts at the clamped quote, no output of the id; kind 2: P2SH(`forfeitHash`) `≥ v − B − fee`, proposer `≥ B + fee` |
| W4 | guarantee (§4.5f) | anyone; a liquidation at `age ≥ graceDaa` (CSV) | none | W3's settlement now, the proposer paid less its offer; a `KpfLock` output naming this claim `≥ max(⌈λ·cover⌉ + offer, 0.1 KAS)`; `P ≤ visibleUntil` |

`KpfLock` paths (§4.5f): L1 dispute (anyone, counter-evidence as W1 on the claim's clock `P`, done resting by
`F + D − R`: the wronged party `≥ H` at output `i`, the burn at `M − 1 − i`, the reward to the executor) and L2 release
(anyone, `age ≥ D`: the owner `≥` the lock at output `i`).

`KpfOpen` paths: `fill` (anyone; `OpCovInputCount(selfId) == 1`; the pair output it funds must carry
`tagL`/`tagS == selfId`), `cancel` (owner sig),
`refund` (anyone at `min(expiryDaa, age 90 days)`, as `[K] contracts/orders/KobCondAsk.sil:343-349`).

### 4.4 Cross-input rules (the class the legacy audit found)

- **No fixed indices.** Every index is an argument; every read of another input is authenticated by template hash and
  P2SH (`[K] contracts/orders/KobCondAsk.sil:133-154`) or is a plain value read whose worst case only gives.
- **Party outputs are injective.** A pair at input `i` pays its first leaving party at output `i` and a second at
  output `M − 1 − i`, which must be ≥ the input count (so it is never another input's positional output); KOB uses the
  same reverse slots (`[K] docs/spec/matcher.md:334-340`). Requirements are `≥`; any excess is the executor's.
- **Hand-over outputs are claimed once.** An input that puts its party into a pair output it does not continue (a
  `KpfOpen`, or the pair whose stayer moves into another pair's continuation) requires that output's `tagL`/`tagS` to
  equal its own covenant id. Covenant ids are **not** unique per input: the parts of a same-id split, and the outputs
  of one multi-output genesis group, share an id (experiments X2, X5). Every claimer therefore also requires
  `OpCovInputCount(selfId) == 1`; with that guard two inputs of one transaction cannot both claim one output.
- **Tags live for one transaction.** A tag that outlived its creating transaction let the continuation of an older
  pair pose as a fresh hand-over to a claimer that is still alive (slice 1b). The first fix, a guard that refused
  any input carrying a copied tag's id, was itself exploitable: a taker could write the pair's own id or the token's
  id as a tag and freeze the pair (review 1, F1/F2). Rule now: **no entry copies a tag.** Every pair output an entry
  creates carries tag 0 on each side, except the side handed over in this very transaction, which carries the tag
  the hand-over writes. There is no live-tag guard.
- **Carriers on a split.** The departing part takes exactly `q(n, BS − BL)` out of the pair; the remainder keeps the
  old carrier, any excess and the rounding slack. The carrier of a new part is funded by the taker (the `KpfOpen`'s
  per-fill carrier, or the executor's own input), never out of a non-consenting party's margin.
- **Evidence block.** All pairs that act on evidence input `ev` sit at inputs `[q, ev)`; each carries `(n_i, q)` as the
  first pushes of its signature script (8-byte `n_i`, as KOB evidence carries `n`, `KobCondAsk.sil:220-221`). Each such
  pair reads the first push of every input in `[q, ev)` (≤ 8, unrolled as the stray scan, `KobCondAsk.sil:172-189`),
  requires each to name the same `q` and `n_j ≥ 0`, and requires `n_ev ≥ ⌈k·Σ n_j⌉`. A pair outside the block that names
  `ev` must name a block containing an honest member with a different `q`, so it fails. Non-pair inputs in the block can
  only raise the sum. No template check is needed. Since §4.5d the block members are proposing pairs and challenged
  pending parts; one block may mix both on one print, each member requiring the threshold over the whole block with
  its own multiple (`kBps` for a proposal, `kcBps` for a challenge).
- **Fees.** Network fee and extra carriers come from the executor's own inputs (`[K] docs/spec/matcher.md:1062-1070`).

### 4.5 Transaction shapes

Indices are illustrative; covenants accept any order satisfying §4.4. `F` = executor funding (P2PK), `Ch` = change.

**T1 Open (KpfOpen long × KpfOpen short), `n` units at `X`.**
in: `0` KpfOpen L, `1` KpfOpen S, `2` F. out: `0` L continuation-or-refund, `1` S continuation-or-refund, `2` new
`KpfPair (n, BL = X − rL, BS = X + rS, E = X, tagL = id(L), tagS = id(S))`, `3` Ch.
The pair's covenant id comes from a genesis authored by `F` (a plain-key input can author it; the id is a hash of that
input's outpoint and its output group, X1). Each order requires `OpOutputCovenantId(j) ≠ 0` for the pair output `j`,
given as an argument (a new id cannot be looked up by id in its creating transaction), because an id-less pair output
is unspendable forever (X1). Each also requires `OpCovInputCount(id(j)) == 0`, so the pair starts a fresh lineage.
Each order checks: own `n` (minFill rule), `X` within its limit, its own key and rate in its side, the counterparty's
rate ≥ `minCounterRate`, pair value ≥ `q(n, BS − BL) + C`, its tag, its escrow out ≥ left.

**T2 Transfer (listing of the long side × KpfOpen long), `n ≤ s` at `X ≥ longAsk`.**
in: `0` KpfPair P, `1` KpfOpen L′, `2` F. out: `0` leaver payout ≥ `q(n, X − BL) − q(n, longTip)` to `longKey`, `1`
L′ continuation-or-refund, `c1` P remainder `(s − n)` (if any, tags 0), `c2` P part `(n, BL′ = X − rL′, BS, E kept,
longKey = L′, tagL = id(L′), shortKey, tagS = 0)`, `3` Ch.
P checks: listing (`X ≥ longAsk`, `n ≥ longMin` or `n = s`), leaver payout, `BL′ ≤ BL` (no-downgrade), both
continuations, value of each ≥ its range. Since review 3 (§4.5c): `tx.daa ≥ S + 4R` (lock time at least that and the
entrant order's `activeFrom`), the entrant healthy at `X`, the part `since = 0`, the remainder `since = S`. L′ checks its own guarantees as in T1, and accepts the kept `E` only if `E ≥ X` (a short order: `E ≤ X`), so its
own idle payout is at least its margin (`KpfOpen.take`).
A direct taker (P2PK signer) may replace L′ (any `tagL` it writes; 0 by convention).

**T3 Close-versus-close (long listing of P1 × short listing of P2), `n` at `X`, `longAsk1 ≤ X ≤ shortBid2`.**
in: `0` P1, `1` P2, `2` F. out: `0` L1 payout ≥ `q(n, X − BL1)` less tip, `1` S2 payout ≥ `q(n, BS2 − X)` less tip,
`c` P3 = P1 continuation `(n, BL2, BS1, E3, longKey = key(L2), tagL = id(P2), shortKey = key(S1), tagS = 0)`,
remainders of P1 and P2 (if any, tags 0), `3` Ch.
P1 checks: L1 payout; P3 found by its own id with `BS = BS1`, `shortKey = S1`, `BL ≤ BL1`, `E3 ≤ E(P1)`, value. P2 checks:
S2 payout; P3 found by `tagL == id(P2)` with `BL = BL2`, `longKey = L2`, `BS ≥ BS2`, `E3 ≥ E(P2)`, value. Neither reads
the other's state. Since review 3 each pair also requires `tx.daa ≥ S + 4R` on its own clock, P3 carries
`since = 0` and each remainder its own pair's `S`.
**T3′ Mutual close** (both listings of one pair cross): in `0` P; out `0` long ≥ `q(n, X − BL)`, `M−1` short ≥
`q(n, BS − X)`.

**T4 Proposal of a unilateral close by the long, `n` at bid `b`** (§4.5d; the one-step settlement is superseded).
in: `0` P (`propose` kind 1, sig of `longKey`, first pushes `n, q = 0`), `1` evidence `KobBid` (fill `n_ev`), `2…` the
bid's counterparty (the executor's token custody per KOB), `F`. out: the pending part `KpfPending (kind 1, side long, n,
BL, BS, quote b, B, refund, proposerKey = longKey, restoreHash)` at the ordinal `k` among the outputs of P's id, value
`≥ v + B` (`n = s`) or `≥ q(n, BS − BL) + C + B` (`n < s`, the executor funds `C`), the P remainder `(s − n, since = S)`,
KOB outputs at their own indices, Ch. P checks: signature, evidence identity and rest on the clock `S` (`touch` logic),
fillability through `restEnd + L`, evidence block, `lockTime ≥ exposed' + R` via CLTV, the pending template
(`PENDING_TPL`), its state and value. Nothing is paid out. Short closes mirrored with an ask `a` and its custody `tk`.
The settlement is T13.

**T5 Add margin / T9 cooperative close / T10 amend.** One pair input with owner signature(s) (`δ` for T5), `F`.
T5: continuation with `BL − δ` or `BS + δ`, value ≥ old + `⌈q(s, δ)⌉`. T10 changes only the signer's listing window.

**T6 Proposal of the liquidation of a long, `n` at ask `a`** (§4.5d; the one-step close-both payout is superseded).
in: block `[q, ev)` of proposing pairs (`propose` kind 2, anyone, proposer key `pk`), `ev` evidence `KobAsk`, `tk` its
custody, buyer side, `F`. out per pair: the pending part `KpfPending (kind 2, side long, n, BL, BS, quote a, cureBound,
fee, B, refund, proposerKey = pk, restoreHash, cureHash, forfeitHash)` with value as in T4, the remainder continuation
(tags 0, `E` kept, `since = S`; it keeps the carrier, excess and rounding slack). `fee = min(max(keeperFeeMin,
q(n, ⌊a·liqFeeBps/10⁴⌋)), q(n, T − BL))`, `T` the trigger price of §4.5b, which lies inside `(BL, BS)` because a pair
is liquidatable only when `BL·(10⁴ + mmrBps) < BS·(10⁴ − mmrBps)` (§4.5c): it is fixed here and paid to the proposer
at finalize. Check: `a·(10⁴ − mmrBps) < BL·10⁴`, the forfeit bound `fb` (§4.5d). Short mirrored with a bid. The
settlement is T13 (forfeit and keep).

**T7 Liquidate by transfer.** Removed (§4.5d). The winner of a finalized T6 owns both sides of the part and sells the
forfeited side by T10 (amend) + T2 at its own price after `S′ + 4R`, `S′` the forfeited part's clock start.

**T11 Challenge, `m` pending parts on one counter print.**
in: block `[q, ev)` of `KpfPending` parts (`challenge`, anyone, first pushes `n, q`), `ev` counter-evidence of the
opposite kind (a `KobBid` refutes a long's liquidation or a short's close, a `KobAsk` a short's liquidation or a long's
close), `tk` its custody, the executor's side of the fill, `F`. out per part: one output of its id, P2SH(`restoreHash`),
value `≥ v − B`; KOB outputs; Ch. No output holds the bond: it is the executor's (the challenger). Checks: the
counter-evidence on the clock `P` (§4.5d), `n_ev ≥ ⌈k_c·Σn⌉`, the contradiction by `g`.

**T12 Cure (kind 2).** in: `0` W (`cure`, sig of the loser's key), `F` (the curer's KAS). out: one output of W's id,
P2SH(`cureHash`), value `≥ max(q(n, cured range) + C, v − refund + ⌈q(n, moved distance)⌉)`; `proposerKey ≥ refund` at
output `i`; Ch.

**T13 Finalize.** in: `0` W (`finalize`, anyone, CSV `age ≥ challengeDaa`), `F`. kind 1 (close, long closer at `b`):
out `i` long `≥ q(n, c(b) − BL) + refund`, `M−1−i` (≥ input count) short `≥ v −` that; no output of the id; a payout of
the other party below dust goes to the closer; a short closer mirrored. kind 2: out one output of W's id,
P2SH(`forfeitHash`), value `≥ v − B − fee`; `proposerKey ≥ B + fee` at output `i`; Ch.

**T8 Idle timeout.** in: `0` P (age ≥ `idleDaa`), `F`. out: `0` long ≥ `q(s, c(E) − BL)`, `M−1` short ≥ the rest, Ch.

### 4.5a Rules changed by review 1 (these override the shapes above)

- **The entry price `E` changes only with the consent of both parties of the pair** (review 1, F3: a party taking
  its own listing could set `E` alone, and idle settle at that `E` took an absent counterparty's whole margin).
  T1 sets `E = X` (both orders, or the order and a signing taker, agree). **Transfer (T2) keeps `E`** in the part
  and in the remainder (so did liquidation by transfer, T7, removed by §4.5d; every pending, restored, cured and
  forfeited state keeps `E` too); the entrant sees `E` and accepts it by entering.
  **Close-versus-close (T3)** writes `E3` into the re-paired output with `E(P2) ≤ E3 ≤ E(P1)`: not below the
  staying long's old entry, not above the staying short's; if `E(P2) > E(P1)` the two pairs cannot be re-paired.
  Mutual close, unilateral close and liquidation leave `E` of any remainder unchanged. Consequence: at idle settle
  an absent party gets its payoff at a price it consented to, or better.
- **The winner never pays the liquidation fee** (F5). `fee = min(loser's residual, max(keeperFeeMin,
  q(n, ⌊quote·liqFeeBps/10⁴⌋)))`. A bankrupt piece pays no fee; the winner has its own reason to liquidate it.
- **`KpfOpen` checks the evidence parameters** the pair it creates will need (`kBps`, `minRestDaa`, `mmrBps`,
  `liqFeeBps`, `keeperFeeMin`) with the same bounds as `KpfPair` (F4).

### 4.5b Rules changed by review 2 (these override 3.3, 3.6, T6, T7 and 4.5a's fee rule)

- **Liquidation forfeits the loser's residual** (review 2, F1: the loser could rest its own order on the wrong side
  of the market for free, liquidate itself at that quote and keep the maintenance buffer the winner was owed). The
  evidence quote now decides only **whether** a pair is liquidatable, never what the loser receives. On P6 the
  loser gets 0 and the winner gets the whole value of the part less the fee (since §4.5d: at finalize, as the
  forfeited part, both sides its own). *(Superseded by §4.5d: on P7 the entrant paid
  `q(n, X − BL)` to the winner, less the fee; P7 is removed.)* The maintenance buffer is thus the winner's
  compensation for the gap risk it carries (its profit is capped at the loser's margin), and the loser's way out before the trigger is its own unilateral
  close, a transfer, or more margin (and, since §4.5d, a cure or a challenge while the claim is pending).
- **Fee.** `fee = min(max(keeperFeeMin, q(n, ⌊quote·liqFeeBps/10⁴⌋)), q(n, T − BL))` for a long, where
  `T = ⌊BL·10⁴/(10⁴ − mmrBps)⌋` is the trigger price (computed from the pair's own state, not from the quote), and
  `q(n, BS − T)` with `T = ⌈BS·10⁴/(10⁴ + mmrBps)⌉` for a short. The winner therefore always receives at least its
  payoff at the trigger price.
- **Evidence must have been fillable by conforming matchers for the whole rest window** (F2: an order past its soft
  expiry is hidden from every book but still fills, so it "rested" unseen). With `exposed` as in KOB's touch rule:
  `expiryDaa ≥ exposed + minRestDaa`; the order is GTC, or its IOC/FOK kill time `max(UTXO DAA, activeFrom) + 600`
  is `≥ exposed + minRestDaa`; and `exposed + minRestDaa ≤ UTXO DAA + 77,760,000` (90 days idle). Since §4.5d each
  bound is `restEnd + minEvidenceLife` (`restEnd = exposed' + minRestDaa`) instead of `restEnd`.
- Accepted as documented behaviour: *(moot since §4.5d, P7 removed: when a whole pair was liquidated by transfer the
  loser's carrier stayed in the new part, F3)*; a listing's minimum part does not bind a part made smaller by a split, by the "or `n = s`" rule (F4).
- Not chosen: valuing the loser with two-sided evidence (an ask and a bid within a maximum spread in the same
  transaction). It keeps a residual for the loser but needs both sides of a thin book for every liquidation.

### 4.5c Rules changed by review 3 (these override the evidence timing of 3.3 and 4.5b where they differ)

**The limit behind the finding.** A covenant can prove a lower bound on the current time (lock time, a UTXO's DAA
score, a UTXO's age) and never an upper bound. "This order is still unexpired now" therefore cannot be checked, and
an order that conforming matchers no longer fill (soft-expired, hidden from every book) can still be filled by its
owner at any later time (review 3, F1). What a covenant can do is tie the evidence to the pair's own clock.

- **Evidence clock.** `KpfPair` gains a state field `since` (8 bytes). The clock start is `S = since` when it is
  non-zero, otherwise the DAA score of the pair's own UTXO. Evidence counts for a pair only from `S`: with
  `exposed' = max(exposed, S)` (`exposed` as in KOB's touch rule), the order must have been fillable by conforming
  matchers through `exposed' + minRestDaa` (the three conditions of 4.5b, with `exposed'` in place of `exposed`), and
  the lock time must prove `tx.daa ≥ exposed' + minRestDaa`. Evidence that rested before the pair's clock started, or
  that died before it had rested `R` on the pair's clock, is refused.
- **Who sets the clock.** A pair output created by a hand-over for the part that changes hands — the new pair of T1,
  the transferred part of T2, the re-paired output of T3 (and the part of T7, removed by §4.5d) — carries
  `since = 0`, so its clock starts at its own UTXO. Every other pair output (the continuation of amend and add
  margin, the remainder of every split, including a proposal's) carries `since = S`: an owner cannot restart the
  clock by touching the pair. §4.5d adds: a restored part `since = S`, a cured part `since = S`, a forfeited part
  `since = 0`. Adding margin needs no restart: it
  moves the trigger, and evidence must satisfy the trigger of the state it is used against.
- **Hand-overs are rate-limited.** Transfer (T2) and close-versus-close (T3) require `tx.daa ≥ S + 4·minRestDaa`, so
  a side cannot dodge liquidation by handing itself over faster than evidence can rest. *(The exemption of
  liquidation by transfer, T7, is moot: P7 is removed by §4.5d.)*
- **Consequence, stated plainly: barrier semantics.** *(Superseded by §4.5d: hold semantics with a challenge
  window.)* A side was liquidatable if the market touched its trigger at any time since the clock started while its
  bound was unchanged; the quote could be used later. Since §4.5d a claim on such a quote settles only if no fresh
  quote contradicting it by `g` rests on the pending part's clock `P` before a finalize lands; a touched side may
  also cure while the claim is pending.
- **Residual risk, not removed.** Within the clock, a party can still use an order that rested honestly at a past
  price and has since died: the closer of a unilateral close gets a look-back to the best quote since `S`, bounded
  only by economics: the dead order stays fillable by anyone at its price, so it is a free gift to whoever sweeps
  it, and it must cover `k` times the size. Executors of KPF must sweep dead mispriced orders; the tooling refuses
  soft-expired evidence by default and warns. A shared heartbeat UTXO per market would bound staleness but
  serialises every evidence transaction; not built. Since §4.5d the evidence must stay fillable for `L` after its
  rest and a stale claim is refutable, so the look-back shrinks to quotes within `g` of the market and to markets
  with no opposite book during `W` (§4.5d residual risks).
- **A side must be healthy when it enters** (F3, F4). At T1 and T2 (and at T7, removed by §4.5d) the entering side
  must not be liquidatable at its own entry price `X`: long `X·(10⁴ − mmrBps) ≥ BL′·10⁴`, short
  `BS′·10⁴ ≥ X·(10⁴ + mmrBps)`. `KpfPair` enforces it for every entrant, `KpfOpen` for the pair it creates. R3-F3's
  check now binds the P3 by which the winner of a finalized liquidation sells the forfeited side (health at entry at
  the entrant's own `X`).
- **The range must be wider than both maintenance bands** (F2). A pair is created, and a side is liquidatable, only
  when `BL·(10⁴ + mmrBps) < BS·(10⁴ − mmrBps)`, i.e. the long's trigger lies below the short's. Then no price
  liquidates both sides and the side that forfeits is never the one owed the pair. The clamp of `T` to `[BL, BS]`
  is dropped: with this condition `T` is inside the range.
- **Tooling** (T2–T5): token amounts are parsed and printed exactly for any `scale` (no decimal shortcut unless
  `scale` is a power of ten); funding files keep the covenant flag; object files without a DAA are refused, not read
  as 0; `keygen` reports what it actually did about file permissions.

### 4.5d Rules changed by the two-step evidence mechanism (these override 3.3, 3.6, 4.3 P5-P7, 4.5 T4, T6, T7 and 4.5b-4.5c where they differ)

Source: the design note `DESIGN-TWO-STEP.md` (cited `note §n`), with the resolutions below. Code:
`contracts/KpfPair.sil` (`propose`), `contracts/KpfPending.sil`, `contracts/KpfOpen.sil` (`checkMarket`), library
`crates/kpf-protocol/src/pending.rs`, `evidence.rs`.

**What it replaces.** No evidence path settles in the evidence transaction. P5 (unilateral close) and P6
(liquidation) become a **proposal**: `KpfPair.propose` (it replaces `onEvidence`) checks what the one-step entry
checked, settles nothing, and moves the part into a `KpfPending` UTXO with a bond. A second transaction ends it:
**challenge**, **cure** or **finalize**. P7 (liquidation by transfer) is removed entirely. The barrier semantics of
4.5c become **hold semantics with a challenge window**: a claim settles only if no fresh quote contradicting it rests
on the pending part's own clock before a finalize lands.

**Resolutions** (of the note §7).

| # | Resolution | Adopted |
|---|---|---|
| 1 | contradiction margin `g` (`gBps`) with the inequalities of note §3 (close rows on clamped prices, deviation 3) | yes |
| 2 | counter-evidence counts from the pending UTXO's DAA score `P`: `exposed' = max(exposed, P)`, rest `R`, fillable through `restEnd + L`; existing third-party orders qualify once rested `R` on `P` | yes |
| 3 | P7 removed; a finalized liquidation is **forfeit and keep**: the winner owns both sides of the part; one liquidation path (no close-both at finalize); `KpfOpen.take` kept (P3 uses it), nothing removed from `KpfOpen` | yes |
| 4 | the proposal precomputes the pair states the part may become (restore, cure, forfeit) as P2SH script hashes | yes |
| 5 | `W` (`challengeDaa`) is not a deadline: challenge and cure are accepted until a finalize lands | yes |
| 6 | the keeper fee goes to the proposer (with its bond) at finalize | yes |
| 7 | a challenged close is re-settled at the counter quote | **no**: a challenged close is cancelled (restored, bond to the challenger) |

The pending part is its own template `KpfPending` under the pair's covenant id (one id, two templates, X12). The
per-pair clock, the range condition, health at entry, tags and entry-price rules of 4.5a-4.5c stay.

**Propose** (`KpfPair.propose(nb, qb, kind, s, side, ev, tk, fb, pk, k, pPre, pSuf)`, paths 5 and 6).
- kind 1 (P5 close): signature of the owner of `side`; the proposer key is the closer's (`pk` ignored). kind 2 (P6
  liquidation): anyone; `pk` is the proposer's payout key.
- Checks exactly what the one-step entry checked: evidence identity, the evidence clock `exposed' = max(exposed, S)`,
  the rest `R` by CLTV, fillability through `restEnd + L` (expiry, IOC/FOK kill, 90-day idle), the evidence block
  `n_ev ≥ ⌈k·Σn⌉`, for a liquidation the trigger, the range rule `BL·(10⁴ + m) < BS·(10⁴ − m)` and the fee
  `min(max(keeperFeeMin, q(n, ⌊p·liqFeeBps/10⁴⌋)), maintenance slice)`. Of the two-step bounds it re-checks only
  `0 ≤ challengeDaa < 2³²` (the CSV range), so not even a look-alike pair can create a pending part with a dead
  finalize.
- Bond `B = max(bondMin, 0.02 KAS, q(n, ⌊(BS − BL)·bondBps/10⁴⌋))`. Pending value `≥ v + B` when `n = s` (it
  inherits the pair's carrier and excess; `refund = B`); `≥ q(n, BS − BL) + C + B` when `n < s` (the proposer funds
  the part's carrier `C`; `refund = B + C`). The remainder exactly as on every split (`since = S`, the old carrier,
  the excess, the rounding slack; value `≥ max(floor, v − q(n, BS − BL))`).
- `k` is the ordinal of the pending part among the two outputs of the id. The pending template is given as witness
  (`pPre`, `pSuf`); its template hash must be the build constant `PENDING_TPL`.
- Cure bound: long loser `⌊p·(10⁴ − m)/10⁴⌋`, short loser `⌈p·(10⁴ + m)/10⁴⌉`, the nearest bound at which the
  proposed quote no longer triggers; no cure (`cureHash` 0, `cureBound` 0) when the short's cure bound exceeds
  `priceMax`. *(Superseded by §4.5e: the bound clears every quote within `g` of `p`.)*
- *(§4.5e)* A liquidation is refused when `longKey == shortKey`.
- Forfeit bound `fb` (argument, checked unique): the nearest inward bound with `q(n, narrowed range) + fee ≤
  q(n, BS − BL)` (it fits, and one tick wider does not, unless it is the loser's own bound).

**Challenge** (`KpfPending.challenge(nb, qb, ev, tk)`). Anyone. Counter-evidence of the opposite kind, read exactly
like evidence with `P` (this UTXO's DAA score; refused if negative) as the clock and the multiple `kcBps`; it must
contradict the claim (`m = mmrBps`, `g = gBps`, `c(p) = min(max(p, BL), BS)`):

| Claim (evidence) | Counter-evidence | Contradicts iff |
|---|---|---|
| close long at bid `b` | ask `a` | `c(a)·10⁴ ≤ c(b)·(10⁴ − g)` |
| close short at ask `a` | bid `b` | `c(b)·10⁴ ≥ c(a)·(10⁴ + g)` |
| liquidate long (at an ask) | bid `b` | `b·(10⁴ − m) ≥ BL·(10⁴ + g)` |
| liquidate short (at a bid) | ask `a` | `a·(10⁴ + m) ≤ BS·(10⁴ − g)` |

Equality contradicts. Liquidation claims are contradicted relative to the trigger (the forfeiture does not depend on
the quote), close claims relative to the quote (the payout does). One output of the id: P2SH(`restoreHash`), value
`≥ v − B`. The bond is what no output must hold: the challenge executor's.

**Cure** (`KpfPending.cure(s)`). kind 2 only, the loser's signature; refused when `cureHash` is zero. One output
P2SH(`cureHash`) with value `≥ max(q(n, cured range) + C, v − refund + ⌈q(n, moved distance)⌉)`; the proposer is
paid `≥ refund` at output `i`. No cure fee (a fee would pay stale proposers). The cure moves the bound exactly to
`cureBound` (since §4.5e past the claim by `g`); more margin is an ordinary P2 afterwards.

**Finalize** (`KpfPending.finalize()`). Anyone, CSV age `≥ challengeDaa`. No key, no evidence.
- kind 1: no output of the id; the closer `≥ q(n, c(p) − BL)` (long) or `q(n, BS − c(p))` (short) plus `refund`; the
  other party `≥ v −` that; the long at output `i`, the short at `M − 1 − i` (≥ input count); a payout of the other
  party below dust goes to the closer.
- kind 2: one output P2SH(`forfeitHash`) with value `≥ v − B − fee`; the proposer `≥ B + fee` at output `i`.

**Market parameters** (in the market block, not constants; bounds enforced by `KpfOpen.checkMarket`, so every pair an
order creates has live branches).

| Parameter | Meaning | Bound | Testnet default |
|---|---|---|---|
| `bondBps` | bond rate, of the part's range `q(n, BS − BL)` | `0 ≤ bondBps ≤ 10⁴` | 300 |
| `bondMin` | bond floor, sompi | `0 ≤ bondMin ≤ 10¹²` (10,000 KAS; `2.9·10¹⁸` before §4.5e) | 500,000,000 (5 KAS) |
| `kcBps` | counter-evidence multiple `k_c` | `0 < kcBps ≤ 10⁵` | `= kBps` (10,000) |
| `gBps` | contradiction margin `g` | `0 < gBps < 10⁴` | 200 |
| `challengeDaa` | window `W`, earliest finalize (CSV) | `2·minRestDaa ≤ W < 2³²` (§4.5e; `minRestDaa ≤ W` before) | 600 DAA, 60 s (with `R` = 50 DAA, 5 s) |
| `minEvidenceLife` | evidence life `L` after the rest, for evidence and counter-evidence | `L ≥ 0`, `L + minRestDaa ≤ 77,760,000` | 864,000 DAA (1 day) |

The pair keeps its other evidence checks (`kBps`, `mmrBps`, `liqFeeBps`, `keeperFeeMin`, `minRestDaa`) as before.

**Pending state.** §4.2 (`KpfPending`, 621 B): the market block, the claim (kind, side, `n`, `BL`, `BS`, quote,
`cureBound`, fee, bond, refund), the pair's entry, keys and listings, `since` (= `S` at the proposal), `proposerKey`,
the three hashes. Immutable.

**Precomputed return states** (each a P2SH hash of the pair's own script, tags 0 on both sides, entry kept; so no
party can be written out of them and no `KpfOpen` can claim them).

| State | Written for | Content | `since` |
|---|---|---|---|
| restore | kind 1 and 2 | the part `n` with keys, bounds and entry unchanged, both sides unlisted (§4.5e) | `S` |
| cure | kind 2 (0 if no cure) | the loser's bound moved to `cureBound`, both sides unlisted (§4.5e) | `S` |
| forfeit | kind 2 | both keys the winner's, both sides unlisted, the loser's bound moved inward by the fee to `fb` | 0 |

**Bond, refund and fee in each branch.**

| Branch | Proposer | Challenger (executor) | Parties |
|---|---|---|---|
| challenge (any kind) | loses `B` (and `C` when `n < s`: the restored part keeps it) | `B` | the part returns as restore with value `≥ v − B` |
| cure (kind 2) | `refund` (`B`, + `C` when `n < s`); no fee | — | the loser funds the moved margin (and the cured part's `C` when needed); the part keeps `v − refund` plus the margin |
| finalize kind 1 | is the closer: payoff at `c(p)` + `refund` | — | the other party `v −` that (carrier, excess, rounding) |
| finalize kind 2 | `B + fee` (the `C` it funded stays in the part, deviation 5) | — | the winner holds the forfeited part, `v − B − fee`, both sides; the loser 0 |

**Clock rules.** Restore `since = S`, the pair's effective clock start at the proposal written explicitly (a refuted
claim neither restarts nor moves the clock, so a side that refutes its own claim gains no rest time); cure
`since = S` (adding margin does not restart the clock, 4.5c); forfeit `since = 0`; the remainder of a proposal
`since = S`. A new owner of a side enters only by a hand-over or a forfeit, each `since = 0`, so a restore never
carries evidence older than the owner's entry. Counter-evidence counts from `P`, the pending UTXO's DAA score;
an order resting since before `P` counts from `P`. `minEvidenceLife` applies to evidence and counter-evidence: each
must stay fillable by conforming matchers through `restEnd + L`. The hand-over rate limit `S + 4R` is unchanged: after
a forfeit the winner can sell the forfeited side from `S′ + 4R` (`S′` the forfeited part's DAA); after a cure the old
`S` still counts, and after a restore as well (a propose-and-restore cycle does not bypass it).

**Actions refused on a pending part.** `KpfPending` has only challenge, cure and finalize. A second proposal, amend,
add margin (other than the cure), idle settle, transfer (P3), close-versus-close (P4) and cooperative close are not
paths of the pending part; the loser cannot sell its side to escape forfeiture and the recorded payouts cannot go
stale. The single-input guard forbids spending the pending part with the remainder of its own split. The remainder is
an ordinary pair.

**Rules that became unnecessary (removed).** P7 and everything only it used: the P7 path of `onEvidence`, the P7
entrant's health check at the evidence quote (review 3 deviation 1), the P7 exemption from the hand-over rate limit
(moot), the payee/`hp` generality of `handOver` (now P3 only), the one-step settlement `closeAt` in `KpfPair`, the
library's `p7_default_x`, `entrant_health_bound`, `close_on_evidence`, `liquidate`, `liquidate_transfer`, and the CLI
commands `close`, `liquidate`, `liquidate-transfer`. R3-F3's check now binds the later P3 by which the winner sells a
forfeited side. Kept unchanged: the evidence clock, the hand-over rate limit, the range condition, health at entry
(T1, P3), tags (every restore, cure and forfeit state has tags 0), the entry price (kept by every pending state),
no-downgrade, the evidence block (a block may mix proposals and challenges on one print).

**Deviations** (`// DESIGN:` comments added by this change).
1. `KpfPair.propose`: the bond has a floor of 0.02 KAS (the note's formula has no dust floor; a smaller bond could not
   be paid as a plain output).
2. `KpfPair.propose`: `gBps > 0` required (the note allows `g = 0`; with `g = 0` a counter quote equal to a close's
   own quote would refute a truthful close). Enforced by `KpfOpen.checkMarket`.
3. `KpfPending`: the close rows compare clamped prices (the note compares raw quotes; a counter quote beyond the same
   bound as the claim changes no payout and must not refute an exact close).
4. `KpfPending.cure`: the cured part keeps `v − refund` plus the added margin, not only the range floor
   `q(n, BS − BL*) + C` as the note writes (else the curer takes the part's carrier and excess, which are not its own).
5. `KpfPending.finalize` (kind 2, `n < s`): the proposer is paid `B + fee` and the carrier `C` it funded stays in the
   forfeited part (the note pays `refund = B + C` and leaves the part without a carrier, which no party but the
   proposer funded).
6. `KpfPending.challenge`: a negative `P` (a mempool spend of an unaccepted proposal reads −1) is refused, as
   `clockStart` does.
7. `KpfOpen.checkMarket`: `W ≥ R` (since §4.5e `W ≥ 2R`) and `L + R ≤ 77,760,000` required (the note only advises them): with `W < R` a
   stale claim could be finalized before any counter-evidence can have rested; with `L + R` above 90 days no evidence
   ever qualifies. Consequence: a market with `R > 77,760,000 − L` is refused (the old edge market
   `R = 124,999,999,999`, where no evidence could ever qualify, is now refused).
8. `KpfPair.propose` relies on `KpfOpen.checkMarket` for the two-step bounds and re-checks only the CSV range of `W`,
   so `KpfPair` does not grow.
9. The pending state is self-describing (entry, listings, `S` copied; not read by the contract): 621 B instead of the
   note's estimated ~420.

**Residual risks** (the attack results, `two_step_attack_tests`; holds are listed in `THREAT_MODEL.md` A, B, F, G, K).
1. **False challenge by the loser on a thin book** (`exploit_false_challenge_by_the_loser_on_a_thin_book`; note §5b
   "fails"). On a book with nobody to sell into its bid, the deserving loser rests its own bid at the refutation
   bound, self-fills it and refutes an honest liquidation, collecting the keeper's bond. On a liquid book the bid is a
   gift of `(bound − market)·k_c·n`. An accepted economic limit; the keeper may re-propose at once (the restore keeps
   `S`) and the loser must pay the fill again each round.
2. **A self-refuted claim no longer restarts the clock** (`holds_self_refuted_claim_does_not_delay_the_liquidation`).
   A truly liquidatable long that proposes its own close at a fake bid and refutes it itself gets its bond back, but
   the restored part keeps `S`: the keeper's ask that had rested on `S` is accepted in the very next proposal. What
   remains is the race of B1: while the fake close is pending (about `R`) no liquidation can be proposed, and the
   keeper's own ask refutes the fake close from `P + R`, taking the loser's bond.
3. **Freeze.** A proposal on a 4.5c-valid quote freezes the part (no close, transfer, listing, margin except cure)
   for `R` against an online victim, up to `W` against an offline one, at the cost of two fees, a self-fill and the
   bond. Since the restore keeps `S`, a refuted quote that is still fillable can be proposed again at once; each round
   costs the proposer its bond, paid to the challenger, and leaves the victim's part exactly as it was
   (`holds_a_stale_proposer_pays_a_bond_every_round`).
4. **Liveness.** Every liquidation is delayed by `W`, and the winner carries the gap risk meanwhile. A stale claim
   against an offline victim stands unless a funded watcher refutes it within `W`; on a one-sided or empty opposite
   book stale closes stand and stale liquidations are stopped only by a cure; a token pause blocks every challenge
   (the counter fill) but not finalize (not tested, Y10). `kpf watch` finds pending parts only from object files: a node
   lists UTXOs by address only, and a pending part's address depends on its whole state; discovery on a live network
   needs an indexer.

**Worked example** (§5 pair: 1,000 tokens, `BL` 9, `BS` 12, `C` 0.2, `mmr` 5 %, `liqFeeBps` 50, `k = 1`, testnet
defaults). Bond `B = q(10⁹, ⌊3·10⁸·300/10⁴⌋)` = 90 KAS; pending value 3,000.2 + 90 = 3,090.2 KAS.
- Liquidation of the long at an ask of 9.4: fee 47 KAS; cure bound 8.7514 KAS since §4.5e (the cure adds 248.6 KAS;
  8.93 and 70 KAS before review 4); forfeit bound
  `fb` = 9.047 KAS (`1,000·(12 − 9.047) + 47 = 3,000`); refuted by a bid `≥ 9.66315790` KAS
  (`b·9,500 ≥ 9·10,200·10⁸`). Finalize: the proposer receives 90 + 47 = 137 KAS; the short holds the forfeited part
  `[9.047, 12]` with 3,000.2 + 90 − 137 = 2,953.2 KAS (both sides).
- A short's liquidation at a bid of 11.5 is refuted by an ask `≤ 11.20`; its cure bound is 12.3165 (12.075 before
  §4.5e).
- A long's close at a bid of 10.5 is refuted by an ask `≤ 10.29`; a short's close at an ask of 11 by a bid `≥ 11.22`.

**Size and compute** (measured, `kpf_metrics_tests`; offline; the figures as of §4.5e: §4.5f grew the market block by 27 bytes and the pending state by 18, and gives the current figures).

Script bytes (prefix + state + suffix), measured after the §4.5e fixes: `KpfPair` 12,201 (1 + 492 + 11,708; before
§4.5e 12,155 = 1 + 492 + 11,662; before §4.5d 12,115 = 1 + 438 + 11,676); `KpfOpen` 3,090 (1 + 375 + 2,714, unchanged
in size); `KpfPending` 3,203 (1 + 621 + 2,581, unchanged).

Script units per input (after §4.5e): P1 amend 287,626; P2 add margin 275,246; P3 transfer (part) 303,921; `KpfOpen`
take 152,022; P4 closeHost 291,465, closeDonor 279,410, mutualClose 150,401; propose close long at a bid 446,156, close
short at an ask 476,273, liquidate long at an ask 609,705, liquidate short at a bid 579,654; challenge of a close long
55,750, of a close short 25,632, of a liquidation of a long 25,513, of a short 55,629; finalize close 7,235 / 7,368,
finalize forfeit 7,083; cure 107,977; P8 25,239; P9 225,495; `KpfOpen` fill 151,359 / 151,505, cancel 106,593, refund
6,522. Largest stack 216 of 244 (propose).

Whole transactions (partial parts of 400 tokens; relay fee 100 sompi per gram; limits: compute 500,000, storage
500,000, 250,000 bytes):

| Transaction | Inputs | Script units | Compute mass | Storage mass | Bytes | Fee (sompi) |
|---|---|---|---|---|---|---|
| propose close long at a bid | 4 | 694,184 | 29,400 | 203,474 | 20,500 | 4,100,000 |
| propose close short at an ask | 4 | 635,502 | 28,434 | 200,056 | 20,404 | 4,080,800 |
| propose liquidate long at an ask | 4 | 768,934 | 29,669 | 200,056 | 20,339 | 4,067,800 |
| propose liquidate short at a bid | 4 | 827,682 | 30,635 | 203,205 | 20,435 | 4,087,000 |
| challenge (ask counter) | 4 | 214,979 | 12,086 | 200,035 | 8,626 | 1,725,200 |
| challenge (bid counter) | 4 | 273,660 | 13,052 | 203,255 | 8,722 | 1,744,400 |
| finalize close | 2 | 107,236 | 5,715 | 31 | 3,635 | 727,000 |
| finalize forfeit | 2 | 107,084 | 5,760 | 215 | 3,670 | 734,000 |
| cure | 2 | 207,978 | 6,826 | 306 | 3,736 | 747,200 |
| 8 proposals on one ask (8 × 500 tokens) | 11 | 5,041,853 | 183,244 | 200,408 | 125,934 | 25,186,800 |
| 8 challenges on one bid | 11 | 457,008 | 40,646 | 203,890 | 32,326 | 6,465,200 |

The largest proposal block that fits is 8 (the scan's maximum); the note estimated it might not fit. The storage
mass of about 200,000 comes from the 0.2 KAS token output of the KOB fill (KIP-9), as before. Before this change 8
one-step liquidations used 1,651,150 script units, compute mass 130,644, 104,934 bytes. A liquidation now costs two
transactions and locks `B` for `W` (90 KAS per 1,000-token pair of the example: 300 bps of the part's range, at least
5 KAS).

**Tests.** `kpf_two_step_tests` (35: every path positive and negative), `two_step_attack_tests` (16: fifteen holds, one
exploit; the clock rule of the restore: self-refute, stale re-proposal, hand-over limit, no clock moved, no old
evidence against a new owner), X6 (`kpf_x6_conservation_tests`: proposes, challenges, cures, finalizes and coop-closes forfeited parts; no
sompi created, a party that did not act loses value only through a finalized unrefuted proposal, every bond ends with
exactly one of proposer or challenger, the clock rules), and the converted tests (`kpf_evidence_tests`, `audit2_tests`,
`audit3_tests`, `audit_tag_tests`, `kpf_metrics_tests`, `kpf_layout_tests`, the library's tests, the CLI lifecycle).
Boundaries that depended on `restEnd` move by `L`; the `R = 124,999,999,999` edge market of `audit_tag_tests` moved to
`R = 76,896,000`.

### 4.5e Rules changed by review 4 (these refine 4.5d)

- **A part whose two sides have one owner cannot be liquidated** (R4-F1: after forfeit and keep the winner holds
  both sides and the old quote still triggered, so anyone could propose again and again and take a keeper fee from
  the winner each round). `propose` refuses a liquidation when `longKey == shortKey`. A side the winner later sells
  enters under the healthy-at-entry rule, so the sold part is not liquidatable at birth either.
- **A cure must clear the claim by the contradiction margin** (R4-F3): the cured bound puts the trigger at least `g`
  beyond the proposed quote, so a quote one tick away cannot reopen the claim against the margin just added.
- **The window leaves slack for the defender** (R4-F5): a market requires `challengeDaa ≥ 2·minRestDaa`.
- **A restored or cured part comes back unlisted**: both listing windows are zero in the precomputed return
  states, so a listing the owner could not withdraw while the part was pending is not live again without its
  signature.
- **`bondMin` is bounded** like the other market parameters, so no market can make every proposal unfundable.
- **Tooling** (R4-F2, R4-F4): `watch` and `evidence --pending` say a counter order refutes a claim only when it can
  also fill the required coverage; `cure`, `challenge` and `finalize` refuse a pending part that is not the one the
  pair's lineage and value imply, and the one-line summary states the total the funder pays.
- Accepted, documented: anyone who relays a proposal or a challenge can copy it with their own payout key and their
  own funds; these actions are permissionless by design.

**How the rules above are implemented** (`contracts/KpfPair.sil` `propose`, `contracts/KpfOpen.sil` `checkMarket`,
`crates/kpf-protocol` `pending.rs`, `evidence.rs`, `kob.rs`, `state.rs`; tests `audit4_tests`, `audit4_tooling`,
`audit4_cli`, X6; `docs/audit/REVIEW-4.md`, "Status after fixes").
- One owner: `require(longKey != shortKey)` in `propose` kind 2, on the pair's two keys as they are when the claim is
  proposed (a pending part's keys never change). A close (kind 1) by the single owner, a cooperative close with its key
  twice, an idle settle and a sale of a side stay open. Two different keys are two owners, even if one party holds both.
- Cure margin, with `m = mmrBps`, `g = gBps`: a long loser at an ask `p` has the reach `pg = ⌈p·(10⁴ − g)/10⁴⌉` (the
  lowest ask with `a·10⁴ ≥ p·(10⁴ − g)`) and the cure bound `cb = ⌊pg·(10⁴ − m)/10⁴⌋`; the cured part is liquidatable
  at an ask `a` iff `a·(10⁴ − m) < cb·10⁴`, which no `a ≥ pg` satisfies and `pg − 2` always does (`pg − 1` does unless
  `pg·(10⁴ − m) mod 10⁴ ≥ 10⁴ − m`; mirrored for a short). A short loser at a bid `p`: `pg = ⌊p·(10⁴ + g)/10⁴⌋`,
  `cb = ⌈pg·(10⁴ + m)/10⁴⌉`, liquidatable at a bid `b` iff `cb·10⁴ < b·(10⁴ + m)`: never for `b ≤ pg`. No cure when
  the short's `cb > priceMax`. Example: 9.40 → reach 9.212, cure bound 8.7514; 11.50 → reach 11.73, cure bound 12.3165.
- Unlisted returns: the restore and cure hashes are computed with both listing windows zero; the pending state still
  records the listings the pair had (self-describing, not read by `KpfPending`).
- Window: `challengeDaa ≥ 2·minRestDaa` in `KpfOpen.checkMarket` and `Market::validate`; `KpfPair.propose` still
  re-checks only the CSV range (deviation 8 of §4.5d).
- `bondMin ≤ 10¹²` sompi (10,000 KAS) in `KpfOpen.checkMarket` and `Market::validate`. *Deviation:* §4.5e names no
  number; 10¹² is 2,000 times the testnet floor and far below what one keeper can hold (the proportional part of the
  bond is already bounded by the part's own range).
- Tooling: `watch` and `evidence --pending` require the order's fill capacity (capped by its `maxFill`) to reach
  `⌈kcBps·n/10⁴⌉` and report the coverage with `kcBps`; `cure`, `challenge`, `finalize` (library and command line) and
  `watch` refuse a pending part whose hashes do not match its state or whose value is below `q(n, BS − BL) + refund`,
  and with `--pair` one whose covenant id or keys are not that pair's; `cure` states the funder's total.

### 4.5f Guaranteed fast settlement (book-backed markets; adds to 4.5d-4.5e, changes nothing there)

Source: the design note `DESIGN-ORACLE.md` (cited `O§n`), its book-backed part (3a) and its "build first" recommendation
(O§6), shaped as the first status of the shared bond template of `DESIGN-ORACLE-2.md` §7, with the resolutions and
deviations below. Code: `contracts/KpfLock.sil`, `contracts/KpfPending.sil` (`guarantee`), `contracts/KpfPair.sil`
(`propose` writes `visibleUntil` and the offer), `contracts/KpfOpen.sil` (`checkMarket`), library
`crates/kpf-protocol/src/lock.rs`, `evidence.rs` (`guarantee`, `dispute`, `release`), command line `kpf guarantee`,
`dispute`, `release`, `watch`, `quote`.

**What it is.** A pending claim (4.5d: a close or a liquidation, proposed with a bond) may be settled at once, without
waiting for the window `W`, by anyone who locks KAS for it: `KpfPending.guarantee` settles exactly as `finalize`
would, provided the same transaction creates a `KpfLock` output that names the claim and holds at least `λ` times the
value the settlement moves away from the party it could wrong (the *cover*), plus the proposer's *offer*. For a window
`D` after the settlement the lock is disputable with exactly the counter-evidence that would have refuted the claim by
a challenge (same clock `P`, rest `R`, life `L`, coverage `k_c` over the evidence block, margin `g`, opposite kind). A
successful dispute pays the wronged party its restitution from the lock, the disputer keeps a stated reward, the rest
is burned. Otherwise anyone releases the lock to its owner after `D`. The judge is the order book, never a vote; one
guarantor is enough; there is no bond registry, no unbonding delay, no quorum.

**What it leaves alone.** The slow path (propose, challenge, cure, finalize, every rule of 4.5d and 4.5e) is unchanged
and remains the default: nothing settles early unless a guarantor pays for it. A guarantee spends the same pending
UTXO as challenge, cure and finalize, so exactly one of them lands: a challenged or cured claim no longer exists to be
guaranteed, and a guaranteed one no longer exists to be challenged (its refutation is now a dispute of the lock). A
refuted fast settlement is **not undone**: the winner keeps what the settlement gave it, and the wronged party is paid
in cash (the whole cover since 4.5g; before, its side's value at the refuting quote), it does not get its position back.

**The two experiments of the note** (engine tests `z_experiments_tests`).

| # | Question | Measured | Decision |
|---|---|---|---|
| Z1 | cost of `checkMsgSig` (`OpCheckSigFromStack`) in script units and sig-ops, 1, 2, 4, 8 in one input | 101,720 / 201,951 / 402,409 / 803,321 script units, i.e. exactly one sig-op charge (100,000 units) per data signature plus about 115 units of opcodes, as `checkSig` (101,673 for one, 803,056 for eight). The static sig-op bound of an input (the 15-per-P2SH-input rule) counts `OpCheckSig` only: the probe's 15 data-signature checks count 0 | not needed: a guarantor consents by spending its own KAS into the lock output with an ordinary SIGHASH_ALL transaction signature; the claimant reads the lock *output*. A data signature spends nothing, so one stake could back many claims (O§1.3, attack 13) |
| Z2 | is a UTXO's DAA score exact enough for the late-submission rule | `OpTxInputDaaScore(i)` returns the UTXO entry's DAA score exactly (any value), -1 for a parent the mempool has not seen accepted; CSV and CLTV check only the committed sequence and lock time (the node enforces them against the accepting block's DAA score), so they are lower bounds and nothing a script reads bounds *now* | exact enough: `P` (the pending part's score, the accepting chain block's DAA, the same on every node and never below the proposal's lock time) is fixed once accepted; a later acceptance only raises `P` and makes `P <= visibleUntil` stricter. What no script can bound is the time of the guarantee itself, so the lock's liability is measured on UTXO scores (`P`, `F`), never on the lock time |
| Z7 | an unspendable burn output | P2SH of the redeem script `OP_RETURN` (0x6a): a standard P2SH script public key (script class `ScriptHash`), and every spend of it fails | the burn of `KpfLock.dispute` |

**Cover, lock, offer.** With `c(x) = min(max(x, bl), bs)`:
- a **close** (kind 1): the cover is what the closer is paid beyond its refund: `q(n, c(p) - bl)` for a long closer,
  `q(n, bs - c(p))` for a short one (or the whole part less the refund when the other party's payout falls below dust
  and goes to the closer). The other party could have kept at most that much: in the worst case the true price is the
  closer's bankruptcy bound, where the closer is owed nothing. Whole and partial parts use the same formula (a part's
  refund includes the carrier its proposer funded, which the closer gets back either way);
- a **liquidation** (kind 2, forfeit and keep): the cover is the part's range `q(n, bs - bl)`, the most the loser's side
  can be worth at any price, all of which the forfeiture moves to the winner; whole or partial alike;
- the **lock** holds at least `max(ceil(cover * lockBps / 10^4) + offer, 0.1 KAS)` (split product, exact for any cover
  up to the KAS supply at `λ <= 3`);
- the **offer** (`0 <= offer <= bond - 0.02 KAS`, a `propose` argument written into the pending state) is what the
  proposer gives up from its own payout if its claim is guaranteed: the guarantee pays the proposer (the closer, or
  the keeper of a liquidation) its finalize payout less the offer, and the offer rides in the lock, so it is the
  guarantor's fee if the lock is released and is burned with it if the claim is refuted. Finalize ignores it. The
  offer is the only fee: no party who did not set it pays anything (the other party of a close and the winner of a
  liquidation are settled exactly as at finalize).

**Restitution, reward, burn** (`KpfLock.dispute`). **Superseded by 4.5g:** every successful dispute pays the wronged party `H` = the whole cover, whatever the counter quote; the table and the "ends with exactly" sentence below are the rule before review 5 (the library keeps it as `Lock::taken_at`, what the claim took at a price, which the cover never falls below), and the burn is now the lock less the cover and `R` (the offer and the rounding of `λ`).

| Claim | Counter quote | Wronged party | Restitution `H` |
|---|---|---|---|
| close long at `b0` | ask `a` | the short | `cover - q(n, c(a) - bl)` |
| close short at `a0` | bid `b` | the long | `cover - q(n, bs - c(b))` |
| liquidate long | bid `b` | the long | `q(n, c(b) - bl)` |
| liquidate short | ask `a` | the short | `q(n, bs - c(a))` |

`0 <= H <= cover <= lock`. The wronged party is paid `H` at the lock's own output index (a restitution below 0.02 KAS
is paid as 0.02 KAS) and ends with exactly what its side was worth at the counter quote, to the sompi: for a
liquidation the loser's payout at `c`; for a close what the settlement paid it plus `H` equals the pending part's value
less the closer's refund and the closer's payoff at `c`, i.e. its payout had the close been at `c`. The **reward** `R =
floor(cover * (lockBps - 10^4) / 10^4)` is what no output must hold, the dispute's executor's (as the bond of a
challenge is). The **burn** output, P2SH(`OP_RETURN`) at the reverse slot `M - 1 - i`, holds the lock less `H` less `R`
(not created below 0.02 KAS). Why a disputer cannot profit from a dispute it could have avoided: whoever could have
avoided it is the coalition that made the false claim (proposer, winner, guarantor). Disputing itself first, it keeps
the loser's side (worth `H` at the counter quote) and the reward and loses the lock, at least `cover + R`: net
`H + R - lock <= H - cover <= 0`, so a lie is never a free option (O§1.7); a guarantor alone recovers `R < lock - H`.
The burn, not a fee, carries the rest: a fee could be taken by a miner who is the liar. A victim that disputes its own
lock is paid `H + R`; it is rewarded for doing a watcher's work, and cannot manufacture the refutation, which needs a
real counter order filled for `k_c * n` (but see the thin-book exposure below).

**The window, release, clocks.** `F` is the lock's own DAA score. A dispute needs counter-evidence that finished its
rest on the clock `P` by `F + disputeDaa - minRestDaa` (`restEnd + R <= F + D`); the lock is released by anyone from
age `D` (CSV), its whole value to its owner at output `i`, the fee from the releaser's input. So the guarantor's
liability is the fixed window `[P, F + D - R]` whenever the release lands, and a disputer whose evidence rested in time
has `R` to land before a release can. `P <= visibleUntil` (written by `propose` as the evidence order's earliest of
`expiryDaa`, IOC/FOK kill and 90-day idle limit) keeps dead orders (review 3 F1) off the path that has no challenge
window; a liquidation needs the pending part to be `graceDaa` old (the cure grace).

**Market parameters** (in the market block, bounds in `KpfOpen.checkMarket` and `Market::validate`; `KpfPending.guarantee`
re-checks the first two ranges, and since 4.5g the whole market block with `KpfOpen.checkMarket`'s text, so not even a look-alike part creates a lock whose reward overflows or whose release
is dead).

| Parameter | Meaning | Bound | Testnet default and why |
|---|---|---|---|
| `lockBps` (`λ`) | lock ratio | `10^4 <= λ <= 3 * 10^4` | 11,000 (1.1): O§6's smallest first version. A refuted claim costs the coalition `λ·cover`; the attack pays only if `p_r < H/(λ·cover) <= 1/λ` (91 %), against 97 % on the slow path with the 3 % bond (O§3.1), for a liquidation |
| `disputeDaa` (`D`) | window after the settlement; the release age | `2R <= D < 2^32` | 650 = `W + R`: evidence must have rested by `F + D - R >= P + W`, so a victim has at least the slow path's window from `P` (the note's `D = W` with this deadline would have given it `W - R`) |
| `graceDaa` (`G`) | age before a liquidation can be guaranteed | `0 <= G <= W` | 100 = `2R` (10 s): an online loser can still cure (the trade-off below) |

The engine fixtures use `R` 600, `W` 6,000, `D` 6,600, `G` 1,200.

**State.** The market block is 288 bytes (261 + the three parameters above); `KpfPair` 519 B, `KpfOpen` 402 B; `KpfPending`
666 B (+ `visibleUntil`, `offer` after `refund`). `KpfLock` (468 B, from redeem byte 1): a bond header shared with the
later statuses of the oracle-only stage (`ownerKey` 32, `status` 8: 1 = lock, the only status this stage writes and
which its entries do not read), the pending part's market block (288, byte for byte), the claim id (the pending part's
covenant id, 32), `kind, side, n, bl, bs, quote` (one 54-byte slice of the pending state), the clock `P`, the cover, the
victim's key (32). `KpfPending.guarantee` builds it from its own script; the lock never needs the pending template, so
the pin graph stays acyclic: `KpfOpen → KpfPair → KpfPending → KpfLock` (the lock pins the KOB evidence templates only).

**Paths.**

| # | Path | Who | Evidence | Effect |
|---|---|---|---|---|
| W4 | `KpfPending.guarantee(j, gk, lPre, lSuf)` | anyone; a liquidation at age `≥ G` | none | finalize's settlement now, the proposer paid less its offer; output `j` = P2SH(`KpfLock` of owner `gk`, this claim) `≥ max(⌈λ·cover⌉ + offer, 0.1 KAS)`; requires `P ≥ 0`, `P ≤ visibleUntil`, the market block within `KpfOpen.checkMarket`'s bounds (4.5g) |
| L1 | `KpfLock.dispute(nb, qb, ev, tk)` | anyone | counter-evidence as W1, on the clock `P`, rest done by `F + D - R` | the victim `≥ H` (= the cover, 4.5g) at output `i`, the burn `≥ lock - H - R` at `M - 1 - i`; the reward is the executor's |
| L2 | `KpfLock.release()` | anyone, age `≥ D` (CSV) | none | the owner `≥` the lock at output `i` |

**Transaction shapes.** **T14 Guarantee.** in: `0` W (`guarantee`; sequence `G` for a liquidation), `F` (the guarantor's
KAS, P2PK, SIGHASH_ALL: its consent). out, a close: `0` the long, the lock, change, the short at `M - 1`; a liquidation:
`0` the proposer, the forfeited part (W's id), the lock, change. **T15 Dispute** (a block member like W1; up to 8 locks
and pending parts on one print): in `[q, ev)` the locks (first pushes `n, q`), `ev` the counter order, `tk`, `F`; out per
lock `i` the victim, KOB's outputs, change, the burns at the reverse slots. **T16 Release.** in `0` the lock (sequence
`D`), `F`; out `0` the owner, change. The lock has no covenant id: it never continues itself and every payout is
positional, so two locks in one transaction never share a payout or a burn.

**Every ordering on one claim** (`every_ordering_of_challenge_cure_guarantee_finalize_dispute_release`).

| First | Then | Result |
|---|---|---|
| challenge, cure or finalize | guarantee | impossible: the pending UTXO is spent; the restored or cured part is a `KpfPair` (no guarantee entry) |
| guarantee | challenge, cure, finalize | impossible: the pending UTXO is spent; the refutation is a dispute of the lock |
| any of the four at age `W` | the others | a race on one UTXO: exactly one lands (GhostDAG decides, as for challenge vs finalize, N§5g) |
| dispute | release | impossible: the lock is spent; the wronged party was paid once |
| release | dispute | impossible: the lock is spent; at age `D` both are valid and one lands |

**Resolutions and deviations** (`// DESIGN:` comments of this change).
1. `KpfPending.guarantee`: **a cure grace** `G`. The note gives the loser of a fast-settled liquidation no cure (O§0.1,
   attack 17). A loser within `g` above its trigger cannot refute a stale claim (no bid in its band contradicts by `g`)
   and on the slow path would cure within `W`; without a grace a guarantor could take its residual at once. With `G` it
   can cure within `G` (2R on testnet), at the cost of delaying every fast liquidation by `G`. `G = 0` is allowed (a
   market that wants no cure right on the fast path), `G = W` makes the fast path no faster than finalize.
2. `KpfLock.dispute`: **a bounded window**. The note keeps a lock disputable until a release lands (O§1.5, "D is not a
   deadline"). Here the counter-evidence must have finished resting by `F + D - R`: the guarantor's exposure is the
   fixed window, a lock left unreleased is not exposed to every later price, and a disputer whose evidence rested in
   time has `R` to land.
3. **The guarantor's fee is the proposer's offer**, set at the proposal and paid out of the proposer's own payout into
   the lock. The note leaves the fee open; a fee taken from any other party would take from a non-consenting party.
4. **Cover of a close** = the closer's payoff (beyond its refund), not the part's range as the note's `V` (O§1.1): the
   value a close moves away from the other party is at most that. For a liquidation the cover is the range, as in the
   note.
5. **`λ`, `D` and `G` are market parameters** in the market block (the note makes `λ`, `β` deployment constants), and
   the reward is `(λ - 1) x cover`, not a separate `βBps` (O§6's `β` = 10 % of `V` at `λ` = 1.1 is the same number).
6. **Dust**: the lock is at least 0.1 KAS; a restitution below 0.02 KAS is paid as 0.02 KAS (out of the burn); a burn below
   0.02 KAS is not created (it stays with the executor, at most 0.02 KAS). The note does not say.
7. **One lock per claim** (`m = 1`, the note's smallest first version); the multi-lock block is not built.
8. The lock carries no covenant id and its entries do not read its status (one template, one status in this stage).

**Size and compute** (measured, `kpf_metrics_tests`; parts of 400 tokens; limits: compute 500,000, storage 500,000,
250,000 bytes; relay fee 100 sompi per gram).

Script bytes (prefix + state + suffix; after 4.5g): `KpfPair` 12,370 (1 + 519 + 11,850), `KpfOpen` 3,255 (1 + 402 + 2,852),
`KpfPending` 4,612 (1 + 666 + 3,945), `KpfLock` 2,299 (1 + 468 + 1,830; the note estimated about 3.2 kB). Before 4.5g: `KpfOpen` 3,231, `KpfPending` 4,338, `KpfLock` 2,559.

Script units per new input: guarantee 52,040 to 52,209; dispute 23,737 to 23,855 with an ask counter, 53,853 to 53,973
with a bid counter; release 4,851. Largest stack 98 of 244.

| Transaction | Inputs | Script units | Compute mass | Storage mass | Bytes | Fee (sompi) |
|---|---|---|---|---|---|---|
| guarantee of a close | 2 | 152,110 | 9,916 | 46 | 6,966 | 1,393,200 |
| guarantee of a liquidation | 2 | 152,041 | 9,961 | 222 | 7,001 | 1,400,200 |
| dispute (ask counter) | 4 | 213,202 | 11,137 | 200,018 | 7,687 | 1,537,400 |
| dispute (bid counter) | 4 | 271,774 | 12,103 | 203,692 | 7,783 | 1,556,600 |
| release | 2 | 104,852 | 4,399 | 0 | 2,679 | 535,800 |
| 8 disputes on one bid | 11 | 442,777 | 33,054 | 203,619 | 24,814 | 4,962,800 |
| (for comparison) finalize forfeit | 2 | 110,045 | 7,269 | 215 | 5,079 | 1,015,800 |
| (for comparison) propose liquidate long at an ask | 4 | 791,481 | 31,503 | 200,056 | 21,873 | 4,374,600 |

All fit with a wide margin. The proposal grew by the two new pending fields (propose liquidate long 768,934 → 789,015
script units, 20,339 → 21,599 bytes; 4.5g's market check in `KpfPending` adds 274 bytes: 791,481 units, 21,873 bytes); 8 proposals on one print still fit (197,116 compute mass, 138,206 bytes).

**Worked example** (the §5 pair, testnet defaults). A long's whole liquidation at an ask of 9.40, guaranteed: the
cover is the range, 3,000 KAS; the lock 3,300 KAS (plus any offer); the proposer receives 90 + 47 = 137 KAS at once
(less its offer), the short holds the forfeited part (2,953.2 KAS). If a bid of 10 KAS that rested `R` from `P`
refutes it within the window, the long is paid the cover, 3,000 KAS, from the lock (4.5g; `q(1,000, 10 - 9)` = 1,000 KAS before), the disputer keeps 300 KAS,
the offer burns; at a bid of 9.6632 (the bound) the long gets the same 3,000 KAS (663.16 KAS before). A long's close at a bid
of 10.5: the cover is the long's payoff 1,500 KAS, the lock 1,650 KAS; the long receives 1,590 KAS and the short
1,500.2 KAS at once; refuted by an ask of 10.29 (or any lower one), the short is paid the cover 1,500 KAS (it then has
3,000.2 KAS, its whole side; 210 KAS and 1,290 KAS burned before 4.5g), the disputer 150 KAS, the offer burns.

**What a guarantor carries** (O§3.1, unchanged in substance): it loses the whole lock whenever the market itself
moves past the refutation bound within `[P, F + D - R]` (rebound risk, priced into its offer), and on a thin book
below; its throughput is its capital over `λ`, per window.

**Residual risks** (the attack results, `guarantee_attack_tests`; holds are listed in `THREAT_MODEL.md` L).
1. **Thin books** (`exploit_a_deserving_loser_on_a_thin_book_takes_the_guarantors_lock`; the mirror of 4.5d residual
   risk 1, O§4 attack 6). A true liquidation (market 9.30) guaranteed; on a book with nobody to sell into a bid, the
   deserving loser rests its own bid at the refutation bound 9.66315790 for `k_c·n` = 1,000 tokens, fills it with its
   own tokens and disputes: it takes `H` = the cover 3,000 KAS as the victim (4.5g; 663.16 KAS before) and `R` = 300 KAS as the executor, nothing (before: 2,336.84 KAS)
   burn, and the guarantor loses its whole 3,300 KAS lock, where the same move on the slow path wins only the 90 KAS
   bond. Exposure per guaranteed claim on such a book: `λ x cover`. On a book where arbitrage fills (and so re-creates)
   a mispriced bid within `R` the bid never rests and the dispute is impossible. Guarantors must not guarantee where
   that arbitrage is absent; the tool says so.
2. **The cure right** (`exploit_a_band_loser_offline_for_the_grace_forfeits_its_residual`). A loser in the band
   `[T, T(1 + g))` offline for `G` (instead of `W` on the slow path) loses its residual to a guaranteed stale claim:
   up to `q(n, T(1 + g) - bl)`, 663 KAS on the example (550 KAS at a true bid of 9.55). The slow path loses the same
   after `W`; the fast path shortens the loser's time from `W` to `G`. Raising `G` buys back the cure time at the price
   of the fast path's speed.
3. **Closes with a small cover.** For a close whose cover is below `B/(λ - 1)` (ten bonds, 30 % of the range at the
   defaults) the fast path deters a coalition less than the slow path in relative terms (`λ·cover < cover + B`); the
   absolute gain is at most the cover.
4. **Cash, not position.** A wronged party is paid the whole cover (4.5g), at least its side's value at any price; it loses the position's
   option (on the slow path a refuted claim restores the part).
5. **Liveness and discovery.** A false claim fast-settled against an offline victim stands unless a funded watcher
   disputes within `D`; `kpf watch` finds locks only from object files (a lock's address depends on its whole state), so
   discovery on a live network needs an indexer, as for pending parts.

**Tests.** `kpf_guarantee_tests` (18: truthful claims of every kind and side, whole and partial; false claims disputed
and the victim whole to the sompi; one tick short of the margin; clock, life and coverage; the window and the release
boundary; a lock one sompi short or of another claim; the exact settlement; the late-submission rule; the cure grace;
the offer; one lock for two claims; guarantor identities; every ordering; the forfeited part acted on while
disputable; boundary values up to a part worth the KAS supply at `λ` = 3), `guarantee_attack_tests` (10: eight holds,
two exploits), `z_experiments_tests` (Z1, Z2, Z7), X6 (`x6_guarantee_random_walk`: proposals with offers, challenges,
cures, finalizes, guarantees, disputes in and out of the window, releases; no sompi created, a non-acting party loses
only through an undisputed claim, every lock ends in exactly one of release or payout plus burn and is accounted for
to the sompi, a wronged party ends with at least its side's value at every grid price whoever disputes, 4.5g), the layout, metrics, library
(`a_guaranteed_liquidation_is_disputed_on_the_chain`, `a_guaranteed_close_is_released_after_the_window`, the builders'
refusals, golden vectors) and command-line (`fast_settlement_lifecycle_with_a_kob_book`) tests.

**Public wording (book-backed markets, with guaranteed fast settlement; from O§5).**
True: "no admin keys, no operator, no upgrade path"; "no custody: funds move only by covenant rules"; "no trusted
price feed: prices are fills on the KOB order book, and every claim can be refuted on chain by counter-evidence";
"optional fast settlement: a settlement is immediate when a guarantor locks KAS at least `λ` times the value it moves;
if the book refutes it within the window, the wronged party is paid from that lock". False: "decentralized oracle"
(there is no oracle here); "manipulation-proof" (evidence is economic, TM T5); "insured" or "guaranteed against loss"
(only refuted claims are paid, only within `D`, and in cash, the cover); "faster liquidations are as safe on
any book" (not on books without arbitrage, residual risk 1).

### 4.5g Rules changed by review 5 (these refine 4.5f)

- **A successful dispute pays the wronged party the whole cover**, not a payoff priced at the disputing order's
  quote (R5-F1: whoever disputed first chose the counter-order, so a party to the claim could dispute its own
  claim with an order at the refutation limit and leave the wronged party a fraction, the rest burned). The cover
  is the upper bound of what the claim could have taken from that party, so it is never short-changed whoever
  disputes and with whichever valid order. The disputer's reward is unchanged; what remains of the lock is burned.
  Known cost: on a thin book the deserving loser who self-fills a counter-order now receives the cover rather than
  a quote-priced amount; the guarantor's loss (its whole lock) is the same as before, the attacker's gain is larger.
  Guarantors must not guarantee on a book that cannot refute a false counter-order; the tooling says so.
- **`guarantee` checks the whole market block** with the bounds `KpfOpen` and `KpfPair` enforce (R5-F2: a
  look-alike pending part with impossible parameters was accepted, then disputed at once). The tooling validates
  the market of any pending part before it calls a claim undisputable, and refuses otherwise.
- **`watch` shows the fast path** (R5-F3): for a pending liquidation it states that a guarantor can settle it once
  the cure grace has passed, shows that deadline before the slow-path one, and puts the cure command first.

Oracle-only markets, first slice (the underwriting slot `KpfBond` and its accounting) and second slice (the stake judge `KpfStake`): `DESIGN-ORACLE-IMPL.md`.

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
| bond rate (§4.5d) | `(BS − BL)·bondBps` | `≤ 10¹⁸` for `bondBps ≤ 10⁴` |
| contradiction (§4.5d) | `BL·(10⁴ + g)`, `a·(10⁴ + m)`, `c(b)·(10⁴ − g)` | `< 2·10¹⁸` for prices `≤ priceMax`, `g < 10⁴` |
| cure bound (§4.5d, §4.5e) | `p·(10⁴ ± g)`, then `pg·(10⁴ + mmrBps)` with `pg < 2·priceMax` | `≤ 3·10¹⁸`; a short cure bound above `priceMax` means no cure |
| lock and reward (§4.5f) | `⌊cover/10⁴⌋·lockBps`, `⌊cover/10⁴⌋·(lockBps − 10⁴)` | `≤ 8.7·10¹⁸` for a cover up to the KAS supply (`cover ≤` the pending value is required) and `lockBps ≤ 3·10⁴`; an offer on top of a cover near the supply at `λ` = 3 overflows and the guarantee fails (the slow path stays) |

No product `size × price` is ever formed. Every entry point checks `0 ≤ BL < BS ≤ priceMax`, `0 < n ≤ s`.

**Worked example.** Token `scale = 10⁶` (assumed 6 decimals), `C = 0.2 KAS`. Long and short open 1,000 tokens
(`n = 10⁹`) at `X = 10 KAS = 10⁹` sompi/token. Long rate 1 KAS (10 %): `BL = 9·10⁸`. Short rate 2 KAS: `BS = 1.2·10⁹`.
- Pair value `q(10⁹, 3·10⁸) = 1000·3·10⁸ = 3·10¹¹` sompi = 3,000 KAS (+ C): long margin 1,000, short 2,000. Largest
  intermediate `1000·3·10⁸ = 3·10¹¹`.
- `mmrBps = 500`. Long liquidatable at ask `a` iff `a·9500 < 9·10¹²`, i.e. `a ≤ 947,368,420` (9.4737 KAS): equity at
  9.47 = 470 KAS < 5 % of 9,470. Short liquidatable at bid `b` iff `1.2·10¹³ < b·10500`, i.e. `b ≥ 1,142,857,143`.
  Liquidating the whole long (§4.5b): the long gets 0, the short 3,000.2 KAS less the fee (since §4.5d at finalize,
  held as the forfeited part), at least
  `q(s, BS − T) + C` with `T = 947,368,421` (2,526.32 KAS + C); at `a = 9.4 KAS` the fee is 47 KAS. The range is
  wider than both bands (`9·10500 = 94,500 < 12·9500 = 114,000`, §4.5c), and a T1 at 10 is healthy for both sides
  (`10·9500 ≥ 9·10⁴`, `12·10⁴ ≥ 10·10500`).
- `kBps = 10⁴` (k = 1): either liquidation needs a resting order of ≥ 1,000 tokens at the trigger, rested `R`.
- Two-step (§4.5d, testnet defaults `bondBps` 300, `bondMin` 5 KAS, `gBps` 200, `W` 600 DAA): every claim on the
  whole pair carries `B = q(10⁹, ⌊3·10⁸·300/10⁴⌋)` = 90 KAS; the pending part holds 3,090.2 KAS. A liquidation of the
  long at `a = 9.4` is refuted by a bid `≥ 9.66315790` (`b·9,500 ≥ 9·10,200·10⁸`, rested `R` (50 DAA) from `P`), cured by
  moving `BL` to 8.7514 (248.6 KAS; §4.5e: no ask `≥ 9.212`, within `g` of 9.4, reopens it), or finalized after 600 DAA:
  the proposer receives 90 + 47 = 137 KAS and the short
  holds the forfeited part `[9.047, 12]` (`1,000·(12 − 9.047) + 47 = 3,000`) with 2,953.2 KAS, both sides. A short's
  liquidation at a bid of 11.5 is refuted by an ask `≤ 11.20` (cure bound 12.3165); a long's close at a bid of 10.5 by
  an ask `≤ 10.29`; a short's close at an ask of 11 by a bid `≥ 11.22`.
- Guaranteed fast settlement (§4.5f, `λ` 1.1, `D` 650, `G` 100): the long's liquidation at 9.4, guaranteed once the
  pending part is 100 DAA old, settles at once (the proposer 137 KAS less its offer, the short the forfeited part); the
  guarantor locks 3,300 KAS (`λ` × the range) for 650 DAA. A bid of 10 that rested from `P` and finished resting by
  `F + 600` disputes it: the long is paid the cover, 3,000 KAS, from the lock (§4.5g), the disputer keeps 300 KAS, nothing burns.
- Close-versus-close with `P2 = (BL2 = 8.5, BS2 = 11.5)` KAS at `X = 10.2`: L1 gets `1000·1.2` = 1,200 KAS, S2 gets
  `1000·1.3` = 1,300 KAS, `P3 = (8.5, 12.0)` holds 3,500 KAS. 3,000 + 3,000 = 1,200 + 1,300 + 3,500. No-downgrade:
  `8.5 ≤ 9.0` (S1's counterparty deeper), `12.0 ≥ 11.5` (L2's counterparty deeper).

## 6. Open questions, most threatening first

| # | Question | Why it threatens the design |
|---|---|---|
| Q1 | Can liquidations find evidence of `k·n` in time? | Evidence must rest `R` and be filled for `k` times the size; a fast move on a thin book outruns it and the winner absorbs the gap. Bound: OI liquidatable per `R` ≤ depth/`k`. Since §4.5d every liquidation settles `W` later still, and the evidence must stay fillable for `L` after its rest. Solvency rests on this. |
| Q2 | What does manipulation really cost on a thin book? | Cost = opposite-side depth swept + exposure risk; with no arbitrageurs exposure is free. Sets `k`, `R`, per-pair size caps. |
| Q3 | Is strict no-downgrade too strict for OI to shrink? | If stayers are rarely better margined than leavers, close-versus-close almost never fills and exits fall back on transfers or evidence. A downgrade allowance against two-sided evidence is the only safe relaxation. |
| Q4 | Script budget of the worst shapes (T6 with 8 pairs, T3 with remainders) | Evidence read + block scan + two continuations + tag checks per input; KOB pair inputs already reach large budgets (`[K] docs/spec/matcher.md:1172-1180`). Measured for the two-step shapes (§4.5d): 8 proposals on one print fit (183,660 compute mass, 125,550 bytes). |
| Q5 | Covenant-id behaviour: genesis authored by a P2PK funder, `OpCovOutputCount == 2` continuations, outputs without an id | An id-less pair output would lock funds forever. |
| Q6 | Who runs KPF executors and keepers? | No operator; KOB executors would need KPF legs in their batches (arming precedent `matcher.md:770-812`). If nobody does, liquidations do not happen. |
| Q7 | Token issuer actions (pause, migration) | Freeze liquidations market-wide (§3.2). |
| Q8 | KOB template obsolescence | KPF pins KOB v3 evidence templates and has no upgrade path; if makers move to a new KOB template, evidence dries up. |
| Q9 | D6, D7 feasibility | §3.7, §3.8. |
| Q10 | Should a resting order tolerate an entry price `E` on the wrong side of its fill price? | `KpfOpen.take` accepts a listing only when `E` is not worse than the fill price for the entrant (long `E ≥ x`, short `E ≤ x`), so a maker is never idle-settled below its entry. Once the market has moved away from `E`, resting orders can take only one side's listings; the other side needs a signing taker, close-versus-close or evidence. A per-order tolerance (maximum idle loss per token) would lift this. |

## 7. Milestones

| M | Content | Change from D8 |
|---|---|---|
| M1 | `KpfPair` + `KpfOpen` + `KpfPending`, T1-T13 (T7 removed), testnet-10, one market | two-step evidence (§4.5d): propose, challenge / cure, finalize; liquidation by transfer (T7) replaced by forfeit and keep + an ordinary P3 |
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

Two-step experiments of the note (`DESIGN-TWO-STEP.md` §6), status after §4.5d:

| # | Experiment | Status |
|---|---|---|
| Y1 | Id continuation `KpfPair` → `KpfPending` → `KpfPair` with a remainder of the same id; co-spend refused | covered: `kpf_two_step_tests` (`propose_challenge_cure_finalize_end_to_end`, `pending_part_and_its_remainder_cannot_be_co_spent`), `holds_pending_twin_and_remainder_are_never_co_spent` |
| Y2 | Script units and mass of `propose` (1 and 8 pairs) | covered: `kpf_metrics_tests` (8 proposals fit, §4.5d table) |
| Y3 | Challenge with an order resting since before `P`; an order dead at `P + R` refused | covered for the clock: `counter_evidence_counts_from_the_proposal_clock`, `counter_evidence_dead_or_short_lived_on_the_proposal_clock_is_refused`; a zero-inventory crossing of two third-party orders is not tested |
| Y4 | CSV boundary: finalize at `W − 1` refused, at `W` accepted; challenge after `W` accepted while unfinalized | covered: `finalize_one_daa_early_is_refused`, `challenge_after_the_window_but_before_finalize_is_accepted` |
| Y5 | Watcher latency on a test-engine book | open |
| Y6 | False-challenge drill, seeded and empty books, `g` and `k_c` sweep | open (the empty-book case is recorded as `exploit_false_challenge_by_the_loser_on_a_thin_book`) |
| Y7 | Evidence supply under `minEvidenceLife` on recorded KOB order flow | open |
| Y8 | Conservation fuzz over propose / challenge / cure / finalize | covered: X6 (`kpf_x6_conservation_tests`), `bond_conservation_in_every_branch` |
| Y9 | Cure at `cureBound`: the same quote no longer triggers; a lower fresh quote does | covered: `cure_exactly_at_and_one_tick_short_of_the_bound`, `cured_pair_is_safe_at_the_claimed_quote_only` |
| Y10 | Token pause during a pending part | open: token pause not tested |

Experiments of the guaranteed-fast-settlement note (`DESIGN-ORACLE.md` §6), status after §4.5f:

| # | Experiment | Status |
|---|---|---|
| Z1 | `checkMsgSig` cost and sig-op count, 1 to 8 in one input | covered: `z1_check_msg_sig_is_priced_like_check_sig_but_not_counted_as_a_sig_op` (100,000 script units each, as `checkSig`; not in the static sig-op bound); not used by this stage |
| Z2 | a lock's / a pending part's DAA score as a contract reads it; lateness detected | covered: `z2_*` (exact entry score, -1 unaccepted, CSV and CLTV lower bounds only), `a_part_accepted_after_its_evidence_died_cannot_be_guaranteed` |
| Z3 | SIGHASH flags a covenant can require | not needed: the guarantor's consent is its own funding input; the contracts accept SIGHASH_ALL only where they read a signature |
| Z4 | mass of a guarantee (1 lock) | covered: `kpf_metrics_tests` (§4.5f table); the multi-lock block is not built |
| Z5 | a dispute with counter-evidence, 8 locks, burn relayed | covered: `metrics_largest_dispute_block` (8 disputes fit), the burn is a standard P2SH script (Z7) |
| Z6 | rebound statistics on recorded KOB flow | open |
| Z7 | an unspendable burn output | covered: `z7_the_burn_output_is_standard_p2sh_and_unspendable` |
| Z8 | GhostDAG races (T14 vs W1, release vs dispute) | engine: every ordering shown to spend one UTXO (`every_ordering_of_challenge_cure_guarantee_finalize_dispute_release`); not run on a network |
| Z9 | `KpfStake` | not in this stage |
| Z10 | watcher latency for lock disputes | open |
