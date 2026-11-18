# KPF threat model (draft 1)

Companion to `DESIGN.md` (cited as `D§n`). References `[L]` (legacy KOB code) and `[K]` (read-only KOB checkout) as
defined there. *(inference)* marks conclusions not stated by a source. Attack entries list: vector (V), precondition
(P), impact (I), mitigation in the design (M), residual risk (R), test idea (T).

## 1. Assets

| Asset | Where |
|---|---|
| Both parties' margins | `KpfPair` value, `q(s, BS − BL) + C` (D§3.1) |
| Open-order escrows | `KpfOpen` value |
| Evidence makers' tokens and KAS | KOB `KobAsk` custodies, `KobBid` escrows read as evidence |
| Keeper capital | the KAS or tokens a keeper trades to produce evidence |
| Pool deposits (M2), committee bonds (M4) | pool / bond UTXOs |
| Liveness | the ability to exit, liquidate and recover funds |

## 2. Actors

Long and short owners (a pair's two parties); a leaver, an entrant, a bystander (the party whose counterparty
changes); executors (anyone who builds transactions, untrusted, may trade; KOB's competing matchers,
`[K] docs/spec/matcher.md:281-287`); keepers (anyone who liquidates); evidence makers (anyone with a resting KOB order);
the token issuer (kUSDT has a federated mint and a guardian pause, `[K] contracts/third-party/kusdt/UPSTREAM.md:5`);
LPs (M2); committee members (M4); miners and GhostDAG ordering. Any actor may control many keys
(`[K] docs/spec/matcher.md:761-765`).

## 3. Trust assumptions

- T1 Consensus and the script engine behave as specified, including covenant-id binding and the limits in
  `[K] docs/spec/matcher.md:346-355`.
- T2 KOB v3 `KobAsk`/`KobBid` enforce their own fills, so a filled evidence order really traded at its quote
  (`[K] docs/spec/matcher.md:710-717`); the token program enforces token conservation.
- T3 Hashes (blake2b covenant ids, blake3 template hashes) are collision resistant.
- T4 Some executor eventually includes any valid transaction that pays its fee. Censorship by every executor is an
  accepted residual, as in KOB (`[K] docs/spec/matcher.md:836-837`).
- T5 **Economic, not cryptographic:** price evidence is honest only to the extent that arbitrageurs take mispriced
  resting orders within `R`. This assumption does not hold on testnet-10 (no external market for the token).
- Not trusted: executors, keepers, the indexer and UI (a wallet re-checks every covenant inequality before signing, as
  KOB requires of matchers, `[K] docs/spec/matcher.md:1119-1120`), any input's state that the reading covenant does not
  authenticate.

## 4. Attacks

### A. Price evidence

**A1 Self-dealt (wash) evidence.**
V: the attacker rests its own ask at a low `a` for `R`, fills it with its own bid next to a liquidation of the victim's
long; the short (attacker) receives `q(n, BS − a)` instead of the fair value. P: attacker is the counterparty or pays it.
I: theft of `q(n, p − a)` plus the liquidation fee from the victim. M: evidence must rest `R` exposed to every matcher
and be filled for `k·Σn` (D§3.3, D§4.4); honest matchers cross a low ask with every resting bid ≥ `a` in the next tick
(`[K] docs/spec/matcher.md:274-287`), so the attacker first sells into all real bids between `p` and `a`.
R: the covenant cannot tell self-dealing (`matcher.md:761-765`); cost is the depth swept plus exposure risk, zero on a
book with no bids and no arbitrageurs. T: X9 drill on an empty book (expect success: documents the residual) and on a
seeded book (measure cost vs gain).

**A2 Thin-book manipulation; open interest versus spot depth.**
V: push the spot quote through the liquidation prices of many pairs. P: OI large relative to the depth between the
price and the liquidation band. I: mass liquidation at prices that need not reflect the market; winners absorb any gap.
M: per-transaction threshold `k·Σn`; minimum rest `R` per market; partial liquidation. Bound *(inference)*: the size
liquidatable per `R` window from one resting order of `Q` is `Q/k`, and a faker must sweep the real opposite depth `D`
between `p` and the fake quote; manipulation of total size `N` costs at least the loss on `D` plus `k·N` exposed. KPF
cannot cap market-wide OI (no shared state); it can cap per-pair size (`minSize` .. a per-market `maxSize`).
R: on a thin book honest liquidations are equally limited (Q1 in D§6): solvency and manipulation resistance pull `k`
and `R` in opposite directions. T: simulate price paths against recorded TN10 depth; tabulate unliquidatable size.

**A3 One-sidedness of evidence.**
V: settle a party at the bound favourable to the actor (a long closing on an ask, a short liquidated on an ask).
P: a path that accepts either side's evidence. I: the actor chooses the price within the spread. M: each path accepts
exactly one kind (D§3.3 table): long close = bid, short close = ask, long liquidation = ask, short liquidation = bid;
withdrawal and funding (not in M1) need both. R: none if the table is enforced; the bound itself is loose by the spread.
T: mutation tests feeding the wrong evidence kind to each path.

**A4 One print acting on many pairs.**
V: one evidence fill of `n_ev` liquidates many pairs each `≤ n_ev/k`. P: per-position threshold (the D2 wording).
I: unbounded leverage of one fake print. M: evidence block rule (D§4.4): all acting pairs sit in `[q, ev)`, agree on
`q`, and `n_ev ≥ ⌈k·Σn⌉`; at most 8 per block. R: a second evidence order in a second transaction is a second cost.
T: X3 mutations (pair outside the block naming `ev`, mismatched `q`, negative `n`, non-pair input in the block).

**A5 Rest-time gaming.**
V: shorten the exposure with a negative `interval`, an `activeFrom` in the past, a fresh custody, or an unaccepted
continuation. P: evidence read without KOB's checks. I: a print that never rested. M: reuse KOB's `touch` logic
verbatim: `interval ≥ 0` (audit 2026-10-29 F1, `[K] docs/spec/matcher.md:729-734`), exposure = max of UTXO DAA + interval,
`activeFrom`, custody DAA, checked by CLTV (`[K] contracts/orders/KobCondAsk.sil:219-246`); only accepted UTXOs
(`matcher.md:1047-1054`). R: none known. T: port KOB's evidence mutation suite to every KPF evidence path.

**A6 Evidence identity.**
V: evidence of another token, another extension commitment, another `scale`, a decaying order, or a conditional /
if-done / pair fill. P: weak identity check. I: free fake prices (units of another commitment cost nothing to fake,
`[K] contracts/orders/KobCondAsk.sil:206-211`). M: same identity checks as KOB (`KobCondAsk.sil:219-246`), template
constants pinned. R: KPF inherits any defect of KOB's evidence rules. T: identity mutations.

### B. Liquidation

**B1 Liquidation races under GhostDAG.**
V: two keepers, or a keeper and the owner (add margin, transfer, amend), spend the same pair. P: concurrent submission.
I: one transaction wins; an owner's rescue may lose to a liquidation that was valid when built. M: none needed for
safety: each transaction is checked against the state it spends; KOB treats races as harmless
(`[K] docs/spec/matcher.md:1090`, `:1097`). R: an owner cannot guarantee a rescue lands first; add margin early.
T: X7.

**B2 Keeper incentive failure.**
V: nobody liquidates. P: fee below the transaction cost (an evidence transaction costs about 0.025 KAS,
`[K] docs/spec/matcher.md:1146`, `:1148`) or below the capital cost of producing `k·n` evidence; the loser's residual is
zero after a gap. I: bad debt grows; the winner absorbs it. M: `fee = min(loser's residual, max(keeperFeeMin,
q(n, a·liqFeeBps/10⁴)))` (D§4.5a): the winner never pays, and a bankrupt piece pays no fee; the winner is the natural
keeper; KOB executors can add
liquidations to batches that already fill qualifying evidence (arming precedent, `matcher.md:770-812`). R: no operator
runs keepers (Q6). T: X4 fee table vs `keeperFeeMin`; liveness drill with only the winner as keeper.

**B3 Evidence liveness and bad debt.**
V: the price gaps past bankruptcy faster than evidence of `k·n` can rest. P: thin book, large pair. I: winner's payout
capped at the loser's margin (`[L] docs/plan.md:909`); the gap is lost to the winner. M: partial liquidation; per-pair
`maxSize`; M2 pool pays the gap first. R: inherent to an evidence-only design. T: A2 simulation.

**B4 Liquidation by transfer at a bad price.**
V: keeper and a colluding `KpfOpen` buy the loser's part at a price below the market. P: T7. I: the loser is under-paid.
M: T7 requires `X ≥ a` for a long (`X ≤ b` for a short), so the loser is paid at least its value at an upper bound of
the price; the winner is protected by no-downgrade. R: the loser pays the spread between the market and `a`. T:
mutation `X < a`.

**B5 Liquidation of a healthy pair.**
V: liquidate with evidence that does not satisfy the trigger, or with a trigger computed on the wrong side. M: trigger
in price units only (`a·(10⁴ − mmrBps) < BL·10⁴`, D§3.6), checked by the pair itself. R: none. T: boundary vectors X11.

### C. Re-mark, re-pair and transfer

**C1 Counterparty downgrade.**
V: a leaver is replaced by an entrant with less margin (higher `BL′` for a long), or close-versus-close forms `P3` with
`BL2 > BL1`. P: any side replacement without a bound. I: the bystander's payoff is cut by `q(s, max(p, BL′) −
max(p, BL))` at every price, immediately when `p < BL′` (D§3.4): this includes accrued profit, not only future upside.
M: no-downgrade, checked by the bystander's own pair (`BL′ ≤ BL`, `BS′ ≥ BS`). R: none for the bystander; the price is
feasibility (Q3). T: mutation `BL′ = BL + 1`.

**C2 Extreme trade price `X` (the "extreme `E′`").**
V: executor or colluding leavers choose `X` far from the market. I (in range form): none for bystanders, because `P3 =
(BL2, BS1)` does not depend on `X`; `X` only splits value between the two leavers, each paid at least its listing limit.
M: listings carry limits; payouts are `≥` requirements. R: a leaver whose limit is loose is paid at its limit; the
executor keeps the crossing spread (KOB precedent, `[K] contracts/orders/KobIfdBid.sil:350-353`). T: X6 conservation
fuzz with random `X`.

**C3 Look-alike or twin pairs.**
V: a pair template instance with forged state (value below its range, `BL ≥ BS`) or a twin sharing a covenant id,
offered to a buyer or used in close-versus-close. P: a contaminated genesis or a stray key holding the id
(`[K] docs/spec/matcher.md:135-141`, `[K] docs/security/AUDIT-2026-10.md:238`). I (two-UTXO model): a buyer left against a
dust side (D§3.4). M: single-UTXO pairs: every check uses the input's physical value and its own state; `value ≥ q(s, BS
− BL) + C` and `0 ≤ BL < BS ≤ priceMax` on every spend; `OpCovInputCount(selfId) == 1`; a twin is another self-funded
pair. R: an indexer may list look-alikes; harmless to funds. T: X2, forged-state pairs in T2/T3.

**C4 Transfer of a side about to be liquidated.**
V: a seller lists a nearly bankrupt long; a buyer pays for it. I: the buyer's own choice; the bystander is protected by
no-downgrade. M: the buyer's `KpfOpen` checks its own rate, i.e. it posts its own margin; the leaver is paid only
`q(n, X − BL)`. R: none beyond user error. T: T2 at `X` just above `BL`.

### D. Griefing

**D1 Forced close.**
V: the counterparty closes unilaterally at any time (D4 i). I: loss of the position and of re-entry costs; no loss of
value, since the passive side is settled at the bound adverse to the closer. M: the closer needs evidence of `k·n`
(cost) and loses its own position. R: accepted by design (D§3.5). T: none (economic).

**D2 Forced splitting into dust.**
V: repeated partial transfers or liquidations cut a bystander's position into many pairs. I: UTXO count and future fees
for the bystander. M: `n ≥ minSize`, remainder 0 or ≥ `minSize` (D§4.6), so at most `⌈s/minSize⌉` parts; the splitter
funds every new carrier; listings carry `longMin`/`shortMin`. R: up to `s/minSize` UTXOs. T: X6 with minimum parts.

**D3 Contention on someone else's pair.**
V: spend the victim's pair repeatedly so its pending transactions fail. P: a path anyone can run without the owner.
M: only listing fills (consented), liquidation (needs a true trigger and evidence) and idle settle (needs `idleDaa` of
inactivity) are permissionless; add margin and amend need the owner's signature. R: none known. T: X7.

**D4 Listing sniping.**
V: an executor fills a listing at its limit rather than at a better crossing. I: the leaver gets its limit, the executor
keeps the spread. M: KOB-style competition; the limit is the leaver's own. R: accepted (as KOB). T: none.

### E. Arithmetic

**E1 64-bit overflow.**
V: parameters or arguments that make an intermediate exceed `2⁶³ − 1` (the script then fails: a liveness, not a theft,
risk, unless it fails only on one path, e.g. liquidation). M: no `size × price` product; `q` split multiplication
(`[K] contracts/orders/KobCondAsk.sil:125-131`); bounds D§5 checked at every entry (`priceMax`, `scale ≤ 10⁹`, `0 ≤ BL
< BS`). R: a pair whose own state is near the bounds may be unliquidatable; the bounds are checked when an honest
`KpfOpen` creates it, not when a look-alike is created. T: X11 boundary vectors per path.

**E2 Rounding and partial-fill dust.**
V: many tiny partial fills each rounding in the actor's favour (the legacy A-4 pattern, `[L] docs/v7-perp-threat-model.md:51-63`).
M: `q` rounds down for the actor, the non-acting side takes the remainder; `minSize` bounds the number of fills; dust
payouts are not created (D§4.6). R: ≤ 1 sompi per part. T: X6.

**E3 Garbage arguments.**
V: `n ≤ 0`, `n > s`, `δ < 0`, negative prices. M: explicit range checks on every argument. T: argument fuzz.

### F. Transaction-shape confusion (the legacy audit class)

**F1 Index assumptions.**
V: legacy patterns: own value at `Op0` (`[L] docs/PERP_DESIGN.md:60`), counterparty at `1 − self`
(`[L] kob-core/src/contract/perp/position.rs:220-226`), spot at fixed inputs 2/3 (`:155-180`). M: no fixed index; every
cross-input read authenticated (D§4.4); counterparty margin is inside the same UTXO. T: every shape with permuted inputs.

**F2 Output aliasing.**
V: one payout output satisfies the requirements of two inputs (two pairs pay one key; two `KpfOpen` of one key fund one
pair). I: one party's funds go to the executor. M: positional `i` and reverse `M − 1 − i ≥` input count (D§4.4,
`[K] docs/spec/matcher.md:334-340`); hand-over outputs claimed by `tagL`/`tagS` = the claimer's covenant id, and no entry copies a tag, so only an output
whose side is handed over in that very transaction can carry one (D§4.4). R: keeper
outputs may alias (only the keeper loses). T: X5.

**F3 Two inputs of one covenant id.**
V: two parts of a split pair in one transaction claim the same continuation. M: `OpCovInputCount(selfId) == 1`
(`[K] contracts/orders/KobCondAsk.sil:310`). T: co-spend both parts (must fail).

**F4 Evidence block bypass.** See A4.

**F5 Interaction with KOB batch layout.**
V: KOB puts bids with refunds first and their refunds last (`[K] docs/spec/matcher.md:337-339`); a KPF shape could
collide. M: KPF reverse slots require `M − 1 − i ≥` input count, KOB refunds sit in the same region but at other
indices; the evidence block may start at any `q` *(inference: not yet tested in a mixed batch)*. T: mixed KOB+KPF batch.

### G. Stray UTXOs and covenant-id lineage

**G1 Pair output without a covenant id.**
V: an open puts the pair in a plain P2SH output. I: the pair can never satisfy its own `OpCovOutputCount(selfId)` rules:
funds locked. M: each `KpfOpen` requires `OpOutputCovenantId(pairOut) ≠ 0` (D§4.5 T1). T: X1.

**G2 Stray UTXO carrying a pair's id.**
V: a contaminated genesis key authors extra outputs with the id (`[K] docs/security/AUDIT-2026-10.md:238`). I: none on
funds (C3); an extra input of the id in a transaction makes `OpCovInputCount == 1` fail, but nobody can add inputs to
someone else's transaction. R: indexers must key pairs by outpoint, not by id. T: X2 with a stray.

**G3 Lineage proofs that do not exist.**
V: a design that trusts "this UTXO was minted by that parent". Covenant ids do not reveal their genesis
(`[K] contracts/orders/KobIfdBid.sil:255-268`). M: KPF never relies on parentage; it relies on physical value and
authenticated state (C3). This rules out the lead's D6 lineage enumeration as a proof of completeness (D§3.7).

### H. Backstop pool (M2)

**H1 Pool empty.** I: shortfall falls to the bankrupt pair's winner (bilateral cap); no third party is touched.
M: by construction (D§3.6). R: winners on a thin market may be under-paid. T: liquidation with empty pool.
**H2 Fake claim.** V: pay out without bad debt (the legacy ADL / insurance flaw, `[L] docs/PERP_DESIGN.md:163-170`,
`[L] security/adl-insurance-threat-model.md:475`). M: the pool pays only next to the bankrupt pair in the same
transaction, at most the proven gap. T: claim with a solvent pair (must fail).
**H3 Pool drain by self-bankruptcy.** V: open both sides, let one go bankrupt on a manipulated print, claim from the pool
as the winner. P: A1 succeeds. I: pool loss up to the gap per pair. M: evidence costs (A1/A2); claim cap per pair
(`adl-insurance-threat-model.md:284-298`). R: real; the pool is the first thing an A1 attacker targets. T: X9 with pool.
**H4 Pool contention.** Every liquidation touching one pool UTXO serialises. M: several pool UTXOs. R: shard
accounting for LP shares is undesigned.

### I. Committee prices (M4)

**I1 Collusion.** A majority walks the price within the band each window and liquidates every position; the band
limits speed, not size (D§3.8). M: bond, band, OI cap. R: loss up to all margins in markets the committee prices.
**I2 First-come signing and cherry-picking.** With more attestations than the consumer must include, the consumer picks
the subset that moves the median; with attestation UTXOs, whoever spends them first denies others. M: fixed membership,
all members included or a quorum rule the consumer cannot select. R: liveness depends on all members.
**I3 Stale signatures.** CLTV gives only `tx.daa ≥ lockTime`; a signed price has no enforceable maximum age (D§3.8).
M: UTXO attestations (freshness by replacement) at the cost of contention. T: X10.
**I4 Bond economics.** A bond can secure at most its own value; extractable value is up to the OI the committee prices.
Slashing needs a provable contradiction (equivocation, or a print contradicted by KOB evidence), which a careful
colluder avoids. R: a committee is a trust assumption the size of OI minus the bond. T: none until X10.

### J. UTXO-set bloat and denial of service

**J1 Bloat.** Every pair and open order needs ≥ 0.08 KAS (`[K] docs/spec/matcher.md:229-239`) paid by its creator;
splits are bounded by `minSize` (D2). R: a funded attacker can still create many small orders; standard KIP-9 economics.
**J2 Indexer spam.** Look-alike pairs and orders with inconsistent state. M: the indexer applies the covenant
inequalities and lists only consistent UTXOs; funds are not at risk (C3).
**J3 Heavy transactions.** Worst shapes may approach the compute limit (Q4). M: block of ≤ 8 pairs; X4 measures.

### K. Timeouts and stuck funds

**K1 Evidence impossible.** Token paused, token migrated to a new id (`[K] contracts/third-party/kusdt/UPSTREAM.md:17`), KOB
makers moved off the pinned v3 templates, or an empty book. I: no unilateral close and no liquidation. M: evidence-free
exits remain: transfer (T2), close-versus-close and mutual close (T3), cooperative close (T9), idle settle (T8).
R: liquidations stop for the duration; bad debt accrues to winners. T: X13.
**K2 Idle settle as a free option.** V: a losing party waits out `idleDaa` hoping nobody touches the pair, then settles
at `E`. M: `idleDaa` long (90 days, as KOB, `[K] contracts/orders/KobCondAsk.sil:109-110`); the winner can close with
evidence or amend (which resets the age) at any time; `E` changes only with both parties' consent (T1 sets it, P3/P7
keep it, P4 writes `E(donor) ≤ E3 ≤ E(host)`, D§4.5a), so no party can move it alone (review 1, F3), and a `KpfOpen`
entrant accepts the kept `E` only on its own side of its fill price. R: a winner absent for `idleDaa` with no evidence
available loses its unrealised profit since `E`, never more. T: X8, X6 (idle payout of every party that did not act).
**K3 Lost key.** The counterparty of a lost-key owner can still close (evidence), transfer its own side, or wait for idle
settle; the lost key's own funds are lost as with any key. **K4 Open orders.** `KpfOpen` refundable by anyone after
`min(expiryDaa, 90 days idle)`. **K5 Censorship.** All executors refusing a pair's transactions: accepted residual (T4).

## 5. Residual risks no design change removes

1. **Evidence is economic, not cryptographic.** The touch rule makes a fake print cost the depth it sweeps and the
   exposure it risks; on a book without arbitrageurs both can be near zero. Testnet-10 cannot show otherwise.
2. **Solvency versus manipulation.** Larger `k` and `R` make prints costlier to fake and liquidations slower; on a thin
   book no parameter makes both safe. Winners absorb the gap.
3. **No market-wide limits.** Without shared state there is no global OI cap, no global price, no fair ADL; per-pair
   caps only.
4. **Issuer and template dependence.** A token pause or migration, or the end of KOB v3 resting orders, stops every
   evidence path for the market; no upgrade path exists to repoint it.
5. **Liveness depends on volunteers.** No operator; if nobody runs executors and keepers, listings do not fill and
   liquidations do not happen. Censorship by all executors is accepted.
6. **Forced close.** Under no funding, a counterparty may end a position at will at the spot bound.
7. **Committee markets (M4)** carry a trust assumption equal to the value the committee can move, less its bond.
