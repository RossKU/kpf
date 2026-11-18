# KPF oracle-only markets: per-pair underwriting with coverage multiple ρ (design note, draft 2)

Status: proposal, nothing implemented. Builds on `DESIGN-ORACLE.md` (cited `O§n`); `D§n` is `DESIGN.md`, `N§n`
`DESIGN-TWO-STEP.md`, `[K]` the read-only `vendor/kob`. Covenant facts relied on are those of the top of `O§`: no
global total, lower bounds on time only, scans of ≤ 8 inputs, signed 64-bit arithmetic, no read-only inputs, ≤ 15
signature operations per input, uncosted `checkMsgSig`. Two further facts: an accepted UTXO's DAA score is exact when a
later spend reads it (O§1.4), and `OpCovOutputCount(id)` exists (`KpfOpen.sil:295`). Numbers marked *(est.)* are not
measured; figures about other venues are marked *(approx.)*.

## 0. Findings

1. **The design holds as a bound per underwriter, not per market.** Every pair names one underwriting slot; the
   slot's outstanding cover `O` is counted on every bind and released only by **expiry** (a lower bound on time, which a
   covenant can prove), never by a proof that a pair ended (which a look-alike pair could forge). So `O ≤ ρ·K` holds
   per slot with no global total and without any pair action touching the slot except open and renewal.
2. **Only the pair's own underwriter can vouch a price for it**, by co-spending an input of its key. This is what
   turns `ρ` into a cap: a coalition can falsely settle only pairs it underwrites, i.e. at most `ρ ×` its own bond.
   Disputes can only **cancel** a claim (restore) or repay a victim, never settle a pair at the disputer's price.
3. **`ρ` does not change the share of open interest at risk; it changes the price of attacking.** If a fraction `f` of
   underwriting bond lies and the judge is captured, users lose `f` of all pair margin at any `ρ`. What `ρ` sets is the
   bonded KAS an attacker must *hold* (not lose) to do it: `f·ΣV/ρ`. At `ρ` = 20 and 10× leverage, a majority of
   the bond is worth 0.5 % of open interest.
4. **`ρ` is not needed for the cost target.** Because cover is sized to margin, not notional, the annual cost per side
   is `y/(L·ρ)`: at `ρ` = 1, 10× and bonders wanting 10 %, it is 1 % of notional a year. Recommendation: `ρ ≤ 2`
   until §7's experiments; raise it only in new markets, knowing it buys capital efficiency with attack cheapness.
5. **Decision 4's "worst-case loss = `ρ ×` bond under a colluding majority" is false as stated.** It holds for pairs the
   colluders underwrite. A captured judge additionally lets the colluders (a) block true claims on pairs where they are
   a party, (b) slash honest underwriters, and (c) settle stranded pairs. (a) is bounded by the colluders' own margin in
   those pairs, (b) costs bonders, not users, (c) by the stranded margin; none of it is bounded by `ρ`.
6. **New: the retrospective dispute.** A finalized claim leaves a record UTXO; for `T_retro` anyone may dispute it and a
   refuted claim's victim is repaid from the slot, first come. Offline victims are protected for days, not for `W`;
   under a lie on every pair at `ρ` = 10, at most 1/10 is repaid.
7. **Margin:** dollar-token margin (b) is the product, KAS margin (a) the prototype. (a) liquidates on KAS volatility
   (about 5× gold's: a fifth of the leverage); (b) depends on the issuer (a pause freezes every pair) and on the KCC-20
   slot limit (one pair per transaction).
8. **Closed markets:** no claim may refer to a closed session; a weekend rate `mmr_w` applies in the last hour before a
   long close; the winner bears gaps beyond the loser's margin. At 20×, `mmr` 2.5 %, a 5 % gap costs the winner up to
   half its margin; with `mmr_w` = 5 %, nothing.

## 1. Mechanism

### 1.1 Objects

| Object | Role | Size *(est.)* |
|---|---|---|
| market block (oracle form) | parameters of a market; its blake3 is the market id | about 330 B |
| `KpfUw` (new; statuses active, reserved, unbonding) | an underwriting slot: bonded KAS `K` with a ring of outstanding cover | about 3 kB |
| `KpfPairO` (new) | `KpfPair` with the KOB evidence reader replaced by the underwriter check; plus binding fields | about 10 kB |
| `KpfPendingO` (new) | `KpfPending` whose challenge is a stake dispute; statuses pending, contested | about 5 kB |
| `KpfRecord` (new) | 0.08 KAS record of a finalized claim, disputable for `T_retro` | about 1 kB |
| `KpfStake`, dispute, receipt | the stake judge of O§2, unchanged except §1.7 | O§1.1 |
| `KpfReq` (second version) | a liquidation request answering a silent underwriter (§1.6) | about 1.5 kB |

Template hashes are **build constants**, never market parameters (`PENDING_TPL` precedent, `KpfPair.sil:1040`): a
market can choose numbers, not code. Pin graph, acyclic: `KpfPairO → KpfPendingO, KpfUw`; `KpfPendingO → KpfUw,
dispute`; `KpfUw → dispute`. `KpfUw` never reads a pair, so it does not pin one.

### 1.2 The market block

| Field | Bytes | Meaning; bound enforced by `KpfOpen.checkMarket` |
|---|---|---|
| `feedHash` | 32 | blake3 of the human-readable feed definition (§1.9); not interpreted on chain |
| `unit` | 1 + 32 ×3 + 8 | 0 = KAS (form a); 1 = KCC-20 token: covenant id, template hash, extension commitment, `scale` (form b) |
| `priceMax`, `mmrBps`, `mmrWBps`, `gBps`, `liqFeeBps`, `keeperFeeMin`, `minSize` | 8 ×7 | as D§4.2; `mmrW ≥ mmr`; `0 < g < 10⁴` |
| `W`, `Δmax`, `Tretro` | 8 ×3 | challenge window (CSV), lateness limit (O§1.4), retrospective window; `W ≥ 3,600 s` |
| `ρBps`, `Λ`, `σBps`, `βBps`, `a` | 8 ×5 | coverage multiple `10⁴ ≤ ρBps ≤ 2·10⁵`; ring period; slash rate of cover; disputer share; slot rate limit |
| `μBps`, `Mage`, `Dr`, `εBps`, `φBps`, `d1Bps` | 8 ×6 | judge (O§2): lead factor `10⁴ < μBps ≤ 1.5·10⁴`, minimum stake age `≥ 7 days`, round time, step, burn, opening deposit |
| `Treq`, `G` | 8 ×2 | request answer time (§1.6), strand grace |

What the protocol enforces is that every parameter lies in its bound, so that every pair of the market has live
branches. Everything else (what the feed is, whether `ρ` is prudent) is displayed, not enforced (§5.1).

### 1.3 The underwriting slot `KpfUw`

**State.** `ownerKey` 32, `market` 32 (market id), `status` 8, ring `j0` 8 and `O[0..7]` 8 ×8, `T_tail` derived,
`reservedFor` 32 and `reserve` 8 (status reserved only), `unbondFrom` 8. Amounts are in **KAS units** (`⌊sompi/10⁸⌋`,
cover rounded up): `K ≤ 2.9·10¹⁰`, so `ρBps·K ≤ 5.8·10¹⁵` and the ring sum stays far inside 64 bits (O§1.2 used
10-KAS units for pro-rata products; here no product of two amounts is formed).

**The ring.** Bucket `i` holds the cover whose liability ends at `E_i = (j0 + i + 1)·Λ/8 + T_tail`, with
`T_tail = W + T_retro`. A pair's claims are valid only while accepted before its cover expiry `X` (§1.5), and its
record can be disputed until `X + T_tail`, so the bucket counts the cover exactly as long as a slash can reach it.
Invariant on every bind and owner split: **`Σ O ≤ ρBps·K/10⁴`**, `K` the slot's own value.

| Entry | Who | Rule |
|---|---|---|
| bind / renew `(c, X)` | owner (sig), CSV age ≥ `a` | `X` a ring boundary within the next `Λ`; `X ≥ lockTime + 7Λ/8`; `c ≤ K`; bucket of `X` `+= c`; invariant; also advances the ring |
| advance | anyone, CLTV ≥ `E_0` | drop bucket 0, rotate, `j0 += 1`; repeatable |
| split / merge | owner, age ≥ `a` | values and every bucket conserved; each child satisfies the invariant; merge only of one id |
| reserve | the opening of a dispute (§1.7) | the slot divides into itself (less `s`) and a **reserved** UTXO of value `s` naming the dispute id |
| release / pay | the resolution of that dispute | reserved → active child (claim upheld) or → payouts and burn (refuted) |
| unbond | owner | all buckets 0 (so CLTV ≥ the last `E_i`), status → unbonding |
| withdraw | owner, CSV age ≥ `U` in unbonding | to any output |

**Why expiry, not release.** A decrement needs proof that a counted pair ended. A covenant cannot tell a pair it
counted from a look-alike pair created by a genesis with the same template and the same slot id (D§3.4): the bonder
would "end" look-alikes to free capacity it never earned. An increment needs no such proof: the slot is spent and
the open orders check that its own continuation grew. So the slot counts up on bind and down only by the calendar.

**The cost of expiry.** A renewing pair sits in its old bucket until `X + T_tail` and in its new one: with eight
buckets and renewal in the last step, `ρ_eff = ρ·Λ/(Λ + T_tail + Λ/8)`. With `Λ` = 28 days, `W` = 6 h,
`T_retro` = 3 days: `ρ_eff ≈ 0.81ρ`. A longer `T_retro` protects offline victims longer and costs capacity linearly.

**Unbonding.** When all buckets are empty no claim, record or open dispute can reach the slot any more except
reservations, which are separate UTXOs. So slashability needs no long `U`: `U` on slots only needs to exceed `M_age` for
stake reuse. The long delay belongs to the judge's stake (`M_age`, O§2), where it stops capital bought for the
occasion. This replaces "a long unbonding delay keeps bond slashable" (decision 4) with "the ring keeps it slashable".

**Contention.** Every bind or renewal spends a slot; nothing else does except disputes. The rate limit `a` (60 s)
guarantees a window after every owner spend in which only non-owner entries (reserve, advance) can land, so a bonder
cannot keep its own slot permanently in flight to dodge disputes. A bonder that wants `r` binds per minute runs `r`
children; one renewal transaction may renew up to 7 pairs of one child (the scan).

### 1.4 Binding a pair (open, transfer, re-pair, renewal, rebind)

`KpfPairO` adds `uwId` 32 (the slot's covenant id), `uwKey` 32, `coverRate` 8 (cover per whole unit: `c = q(s,
coverRate) ≥ q(s, BS − BL)`), `X` 8, `πBps` 8 (premium a year, of `c`), `altSet` 32 (hash of up to four alternative
slot ids, or a code for "any slot of the market").

- **Open (T1′).** In: the two `KpfOpen` orders, one slot child (bind, owner sig), funding. Out: the order
  continuations, the pair, the slot continuation, the premium `⌈π·c·Λ/yr⌉` to `uwKey`, change. Each order checks, besides
  T1: the slot input's template is `UW_TPL` and its market id is this market; the slot's continuation bucket for `X`
  grew by `≥ c`; `uwKey` equals the slot's owner; `X ≥ lockTime + its own minimum term`; `π ≤ its maximum`; the
  pair's `altSet` is the one it names (or it accepts any); and **exactly one output carries the new pair id**
  (`OpCovOutputCount`, experiment Z11). The last check is new: the dispute machinery (§1.7) trusts the pair's lineage,
  so a hidden genesis output under the pair id must be excluded at birth.
- **Split** (partial close, partial liquidation): both parts keep `uwId`, `coverRate`, `X`; no slot is touched, since
  the sum of parts never exceeds the cover.
- **Add margin, transfer:** allowed without the slot while the new range stays within `coverRate`; otherwise the
  transaction must renew with the slot (bind of the difference). An entrant's `KpfOpen` states which underwriters it
  accepts (an id, a set hash, or any) and checks `uwId` and `X`.
- **Close-versus-close** only between pairs with the same `uwId` (re-pairing must not move a stayer to an
  underwriter it never accepted); P3 keeps P1's `coverRate`, which must contain `BS1 − BL2`, else no re-pair.
- **Renewal** by the incumbent: `X → X + Λ`, premium paid from the margins (each side half, its bound moved inward,
  `BL += ⌈premium/2·scale/s⌉` and mirrored). No party signs: the rate `π` was agreed at open. The incumbent can
  therefore extort only by refusing to renew.
- **Rebind** to another slot: both parties sign; or, once `CLTV ≥ X`, anyone to a slot of `altSet` at a premium
  `≤ π`. A party cannot unilaterally move its pair to a slot of its choosing before expiry: it could choose its own.
- **Lapse.** Claims accepted after `X` are void (§1.5). A pair not renewed by `X + G` is **stranded**: price-free exits
  stay (transfer, mutual close, cooperative close); idle settle at `E` is **removed** in oracle markets, because a
  blocker could run the clock out to it (§5.13); price claims need the lock path (§1.6).

### 1.5 The price claim

A claim is a two-step proposal as in D§4.5d: `KpfPairO.propose(kind, side, n, p, t0, j, …)` moves the part into a
`KpfPendingO` and settles nothing.

- **Who vouches.** Input `j` of the transaction must be a plain P2PK input of `uwKey` (`0x20‖key‖0xac`, read by
  `OpTxInputSpk`, experiment Z1). Its SIGHASH_ALL signature covers every outpoint (the pair, so its state), every
  output (the pending state with `p` and `t0`) and the lock time. One signature operation, in that input. Data
  signatures (`checkMsgSig` over `blake2b(market ‖ pair outpoint ‖ state hash ‖ kind, side, n, p, t0)`) are an
  alternative if Z1 prices them; with `ρ > 1` the reason O§1.3 gave for spending the bond no longer applies, because
  the slot is liable for the pair whether or not it is spent.
- **Replay and reuse.** The underwriter's input is spent; the pair outpoint is signed; two parts of one id cannot meet
  (`OpCovInputCount == 1`). A signature not used within `Δ_v` is revoked by spending its input.
- **Lateness.** `lockTime ≥ t0` by CLTV; `P` is the pending part's exact DAA (read at finalize). The claim is about the
  reference price at `τ = t0` if `P − t0 ≤ Δmax`, else at `τ = P` (O§1.4). **Finalize restores, instead of settling,
  when `P > X`**: a claim landing after its cover expired is void, so no claim outlives its counted liability.
- **Tolerance inside the trigger** (O§1.6): a long is liquidatable only if `p·(10⁴ + g)·(10⁴ − m) < BL·10⁸` (two
  steps, each `< 2·10¹⁸`); a close is paid at `p` moved by `g` against the closer. Then a price wrong by less than `g`
  never liquidates a healthy pair and never pays a closer more than fair. The cure of D§4.5d stays.
- **No proposer bond `B`**: the slot is liable. The keeper fee goes to the underwriter.
- **Finalize** (anyone, CSV `≥ W`, not contested, `P ≤ X`): T13's settlement, plus one `KpfRecord` output (0.08 KAS,
  funded by the finalizer and recovered by the underwriter after `T_retro`) holding the claim block, `P`, `uwId`, the
  wronged party's key and `c`.

### 1.6 Liveness: a silent underwriter (second version)

Without a fallback a silent underwriter stops every liquidation of its pairs until `X + G` (up to `Λ`). The fallback
must not let a captured judge settle honest pairs, so it **rebinds**, it never settles:
- anyone posts a `KpfReq` (bond `b_r`) naming the pair and a claim `(kind, side, n, p, t0)`;
- within `T_req` the underwriter answers by proposing the claim (vouched) or by spending the request with a signed
  counter-quote `p′` that contradicts `p` by `g`; the counter is a claim and is disputable (§1.7). `b_r` goes to it;
- after `T_req` without an answer, or if the counter is refuted, anyone may rebind the pair to a slot of `altSet`
  (whose owner then vouches), and `b_r` is refunded.

**Stranded pairs** (`CLTV ≥ X + G`) alone may use the lock path of O§1.6: any bonded staker vouches one claim with an
exclusive lock `≥ λ·V`, judged by stake. This is the only path on which a party other than a counted underwriter can
price a pair, and it is open only where no counted underwriter exists.

### 1.7 Dispute and judge

The judge is O§2 (aged stake, lead factor `μ`, rounds of `D_r`, minimum step `ε`, burn `φ`), with these changes.
- **Live dispute** (pending part, before finalize): anyone spends the pending part into status contested, deposits
  `d_1 ≥ max(d_min, d1Bps·c)`, states `p′`, and in the same transaction **reserves** `s = σ·c` (plus nothing else) from
  a child of `uwId` with free value. The contested part cannot finalize; it waits for the dispute UTXO (same id).
- **Retrospective dispute** (within `T_retro` of the record): spends the `KpfRecord`, deposits `d_1`, reserves
  `s = H_max + σ·c` (`H_max = c`) from up to 7 children of `uwId`, first come.
- **Resolution** (anyone, dispute UTXO `D_r` old): claim upheld (YES) → the contested part finalizes as proposed, the
  reservation returns to an active child, the NO pool goes to YES stakers less `φ`. Claim refuted (NO) → live: the
  part is **restored** (D§4.5d restore state); retro: the wronged party is paid `H(p′)` (O§1.5 table, with `p′` as the
  counter quote) from the reservation. In both, `β` of `σ·c` goes to the disputer and the rest of `σ·c` is burned;
  the YES pool goes to NO stakers less `φ`.
- **Nobody shows up.** No dispute: the claim stands after `W` and the record lapses after `T_retro`. A dispute nobody
  answers: the disputer leads and wins after `D_r`, so an honest underwriter must be online for `W + T_retro + D_r`
  after every claim (professional duty, priced in `π`).
- **Why burn.** A liar that expects a dispute disputes itself first; it then loses `(1 − β)·σ·c`, not nothing (O§1.7).

The judge's verdict condition is O§2's: honest participating aged stake `p_h(1 − f) ≥ μ·f`, `f* = p_h/(p_h + μ)`;
with `μ` = 1.25, `f*` = 44 % at full participation, 29 % at half.

### 1.8 Money flows

| Flow | From | To | When |
|---|---|---|---|
| premium `π·c` per year | both sides' margins (half each) | `uwKey` | at open and each renewal, prepaid for `Λ` |
| keeper fee | the loser's residual (D§4.5b) | the underwriter | at finalize |
| record carrier 0.08 KAS | finalizer | underwriter | after `T_retro` |
| slash `σ·c` | the slot | `β` to the disputer, rest burned | refuted claim |
| restitution `H` | the slot | the wronged party | refuted retro claim |
| dispute pools | depositors | winning side, less `φ` | resolution |

Judge stakers earn only from disputes (O§4.5); a premium share would need a hot pool UTXO. In practice the judge's
stake is the underwriters' own idle reserve.

### 1.9 Creating a market; what a user checks

Anyone creates a market by writing its block into a first `KpfOpen` order; nothing is registered. The feed definition
(off chain, content-addressed by `feedHash`) states the quantity ("spot gold, US dollars per troy ounce"), the sources
and how they combine (median of named public sources), the time of `τ` (DAA mapped to the accepting block's timestamp),
the session calendar and opening rule (§4), rounding, and what the judge does when sources disagree beyond `g`. A wallet
hash-checks the text (none → "unknown feed", hidden by default), shows the parameters and §2's worst case in words,
groups markets of one display name with their ids, and shows the underwriter (slot id, `K`, ring sum, age) and
`altSet`. It never shows approval, rank or "official".

## 2. ρ analysed

Notation: notional of open interest `N` (one side); leverage `L`, so each side posts `N/L` and the pair range is
`ΣV = 2N/L`; total underwriting bond `K = ΣV/ρ_eff` (here `ρ_eff ≈ ρ` for readability; multiply bond by 1.24 for the
ring of §1.3); bonders' required yield `y`. Cost per side, as a share of its notional per year, is `y·K/(2N) = y/(Lρ)`,
before network fees.

**Capital and cost.**

| `ρ` | bond / OI at 5× | at 10× | at 20× | cost/yr at 10×, `y` = 5 % / 10 % / 20 % | at 5×, `y` = 10 % | at 20×, `y` = 10 % |
|---|---|---|---|---|---|---|
| 1 | 40 % | 20 % | 10 % | 0.50 / 1.00 / 2.00 % | 2.00 % | 0.50 % |
| 2 | 20 % | 10 % | 5 % | 0.25 / 0.50 / 1.00 % | 1.00 % | 0.25 % |
| 5 | 8 % | 4 % | 2 % | 0.10 / 0.20 / 0.40 % | 0.40 % | 0.10 % |
| 10 | 4 % | 2 % | 1 % | 0.05 / 0.10 / 0.20 % | 0.20 % | 0.05 % |
| 20 | 2 % | 1 % | 0.5 % | 0.025 / 0.05 / 0.10 % | 0.10 % | 0.025 % |

Users' own margin (`2/L` of OI) is not system capital and is excluded. The 5 % target is met at `ρ` = 1 everywhere
except 5× with `y` = 20 % (4 %). But `y` is not free: bonders carry slash risk and the KAS price, and a 20 % demand at
`ρ` = 1 is plausible early.

**Loss under collusion.** Colluders hold `f` of the underwriting bond and users spread over underwriters in proportion
to bond, so colluders underwrite `f·ΣV`. "Captured" means `f ≥ f*` of participating aged stake (same population).

| `f` | judge captured at `p_h` = 1 / 0.5 | loss, captured: share of margin (of OI at 10×) | colluders' bond needed at `ρ` = 1 / 2 / 5 / 10 / 20 (OI at 10×) | judge honest, every victim silent past `T_retro` |
|---|---|---|---|---|
| 34 % | no / yes | 34 % (6.8 %) | 6.8 / 3.4 / 1.4 / 0.68 / 0.34 % | `(1 − 1/ρ)` of 34 % |
| 51 % | yes / yes | 51 % (10.2 %) | 10.2 / 5.1 / 2.0 / 1.0 / 0.51 % | — |
| 67 % | yes / yes | 67 % (13.4 %) | 13.4 / 6.7 / 2.7 / 1.3 / 0.67 % | — |
| 100 % | yes / yes | 100 % (20 %) | 20 / 10 / 4 / 2 / 1 % | — |

- **Speed.** All within one `W` plus finalize (hours), so per day = total before unbonding. Unbonding bounds nothing:
  under capture no slash happens and the colluders keep their bond (O§3.2's deterrent needs a KAS fall of `ρ`·100 %).
- **Honest judge.** A false claim disputed within `W` is restored: loss 0, slash `σ·c`. Disputed within `T_retro`
  after finalize: the victim is repaid while the slot lasts. Loss arises only from claims nobody disputes for
  `W + T_retro`, and then the slot repays first-come up to `K`: unrecovered `≤ (ρ − 1)·K_att` = `(1 − 1/ρ)` of the
  colluders' pairs. At `ρ` = 1 nothing is unrecovered if anyone disputes before `X + T_tail`.
- **One voucher's honest mistake** (one pair, `V` = 2,000 USD for a 10,000 USD pair at 10×, `σ` = 10 %): disputed
  live → the pair is restored, the voucher pays 200 (`β` 20 % = 40 to the disputer, 160 burned); disputed retro → the
  victim gets `H ≤ 2,000` (guaranteed because `c ≤ K` at bind) and the voucher pays `H + 200`; never disputed → the
  victim loses up to 2,000. The user's loss is independent of `ρ`; the voucher's is too.

**What bounds the total when `ρ > 1`.** Per slot, `Σ cover ≤ ρ·K` structurally (§1.3), and only a slot's owner can
vouch its pairs (§1.5). Hence false settlements `≤ ρ ×` the colluders' own bond, summed over their slots, with no global
total. Nothing bounds the colluders' bond except their wealth, and nothing bounds the three capture exposures of §0.5.
The bound is therefore on *who* can be robbed (users of colluding underwriters), not on *how much the system* loses.

**Comparison** (outside figures approximate, public dashboards of 2024–2026; not verified for this note).

| System | Backstop vs OI | Beyond the backstop | Price source and its trust |
|---|---|---|---|
| large centralized exchange | insurance fund ≈ 1–3 % *(approx.)* | auto-deleveraging of winners | the operator |
| dYdX v4 | insurance fund ≈ a few % *(approx.)* | deleveraging | validators' price, 2/3 of stake *(approx.)* |
| Hyperliquid | vault ≈ 5 % *(approx.)* | deleveraging | validators' stake-weighted price |
| GMX-type pool | pool ≈ 100–200 % *(approx.)*; it is the counterparty | the pool | an external oracle network plus keepers |
| UMA optimistic oracle | a bond per assertion | — | token-holder vote on dispute (honest majority of token) |
| KPF oracle, `ρ` = 1 / 10 at 10× | bond 20 % / 2 %, plus a bilateral counterparty | the winner (bilateral cap) | the pair's underwriter; aged-stake judge on dispute |

KPF needs no deleveraging (D§3.6); at `ρ ≥ 10` its system capital is in the exchanges' range. Its judge is of the
kind dYdX and UMA use (honest stake majority), but capturing it yields only the pairs the captor's slots underwrite.

## 3. Margin asset

| | (a) KAS margin, KAS quote | (b) dollar-token margin |
|---|---|---|
| what the pair holds | KAS (today's `KpfPair`) | a token UTXO owned by the pair's covenant id (KCC-20 scheme 0x04, exact amount, `[K] KobAsk.sil:177-191`) plus a KAS carrier |
| product | underlying priced in KAS | underlying in dollars |
| price claim | `p` = underlying/KAS at `τ`, two legs (for example XAU/USD ÷ KAS/USD), both in the feed definition | `p` = underlying in token units (dollars) at `τ` |
| payouts | KAS outputs | token outputs (the token program checks the transfer) plus carriers |
| volatility that liquidates | that of the ratio, ≈ KAS's (≈ 80 % a year *(approx.)*; ≈ 4 % a day) | the underlying's (gold ≈ 15 % a year, ≈ 1 % a day *(approx.)*) |
| leverage that liquidates as often as gold at 10× in (b) | about 2× | 10× |
| bond vs cover | same unit: `ρ` exact | KAS bond against dollar cover: needs a KAS/USD floor (below) |
| issuer powers | none | pause, blacklist, migration to a new covenant id, bridge failure |
| transaction limits | as today (evidence blocks unused) | 3 token inputs and 3 outputs per transaction (`KCC20PublicMint`, `[K] token-programs.md` §1): one pair per transaction; partial transfer (4 token outputs) and partial close-versus-close (5) impossible, whole ones fit; renewal one pair at a time |
| extra size *(est.)* | none | + 1.5 kB of custody and stray checks in `KpfPairO`; + 2 token inputs/outputs of 1–2 kB each per transaction |

**Cover in (b).** The slot is KAS (the 3-slot limit leaves no room for a token bond in T1). Its capacity is
`ρ·K·pK·(1 − h)`, `pK` a KAS price floor in dollars marked on the slot by a fill on the KOB kUSDT/KAS book (the
existing evidence reader), `h` a haircut, and binds may only choose `X ≤ markDaa + Λ`. A KAS fall of `x` within `Λ` raises
the effective multiple to `ρ(1 − h)/(1 − x)`: at `ρ` = 2, `h` = 30 %, a 50 % fall gives 2.8. The judge's security in
dollars also falls with KAS while the dollar margin does not (§5.10).

**Pause.** A paused token refuses every transfer: no proposal, finalize, transfer, close or renewal moves. The market
halts; covers lapse; the winner bears all moves during the pause (bilateral cap). After the pause the incumbent renews
(renewal after `X` is allowed to the incumbent), and claims for `τ` inside the pause are void (no settlement was
possible then). A blacklist of a pair's covenant id freezes it forever; a migration leaves pairs holding the old token.
A dollar token's issuer and bridge carry their own legal status; a market chooses its token knowingly.

**Recommendation.** Build (a) first: today's contract plus the underwriting layer, `ρ` exact. Never present it as
"gold" (it is gold in KAS, at a fifth of the leverage). (b) is the product; build it second, on a token without pause
or blacklist where one exists, and show the issuer's powers next to every (b) market.

## 4. Closed markets and gaps

**Rule.**
1. The feed definition carries the session calendar. A claim whose `τ` lies in a closed session is false by
   definition; the judge refutes it. (A covenant cannot read calendars: DAA is not wall time.)
2. The first valid `τ` after a reopening is the official opening print, or the first price `δ_open` (5 min) after it.
3. A claim flagged "pre-close" with `τ` in the last hour before a scheduled closure of 24 h or more evaluates the
   trigger with `mmr_w` instead of `mmr` (the covenant applies the flag; the judge refutes a flag outside the window).
4. The winner bears any gap beyond the loser's margin; the underwriter and the judge bear none of it.
5. In form (a) the KAS leg trades all week: the feed must say whether the underlying leg is frozen at its last close
   (then KAS moves still liquidate on weekends) or the market closes entirely.

**Exposure for a 5 % gap down against the long** (10,000 USD notional at the close; strict trigger; no move during `W`).

| Leverage, `mmr` | Long at the close | After the gap | Loser (long) loses | Winner's shortfall |
|---|---|---|---|---|
| 10×, 5 % | fresh, equity 10 % (1,000) | equity 500, still healthy (`0.95·0.95 > 0.90`) | the 500 move | 0 |
| 10×, 5 % | just above trigger, equity ≈ 5 % (≈ 500) | at `BL` | ≈ 500, all | 0 |
| 20×, 2.5 % | fresh, equity 5 % (500) | at `BL` | 500, all | 0 |
| 20×, 2.5 % | just above trigger, equity ≈ 2.5 % (≈ 250) | 2.6 % below `BL` | ≈ 250, all | ≈ 250 = half of its 500 margin |
| 20×, `mmr_w` = 5 % | pre-close claims leave only equity ≥ 5 % open | at worst `BL` | ≤ 500, all | 0 |

The winner is safe whenever the gap is at most the maintenance rate in force. So set `mmr_w` at or above the 99th
percentile closure gap of the underlying (experiment Z9) and let `mmr` serve the open market. "No claims while
closed" alone is not enough (it does not change the exposure), and "claims vouchers will sign" while closed would
make the underwriter the price source for the weekend, which is the oracle with no reference.

## 5. Attacks

Verdicts: **holds**, **fails**, **holds only if** (the condition follows).

1. **Misleading feed, look-alike market, hostile parameters.** Enforced: parameter bounds; templates are constants; a
   look-alike is a different id. Only displayed: what the feed means, whether it names a source the underwriter
   controls (then the price is simply the underwriter's), similar names, prudent `ρ`, `T_retro`, `mmr_w`. **Holds only
   if** wallets hash-check definitions, group look-alikes and show the worst case; **fails** for users who trust a
   name. An ambiguous definition turns disputes into votes on taste: flag definitions without a disagreement rule.
2. **Underwriting more than `ρ` through parallel transactions or split bonds.** Parallel binds spend one UTXO and
   conflict. Splits conserve every bucket and keep the invariant per child. Look-alike *slots* from a genesis are real
   KAS with honest-or-higher counters. Look-alike *pairs* naming a real slot are uncounted cover: the creator can sell
   their sides to entrants by transfer, and if the creator owns the slot it vouches them beyond `ρ`. **Holds only if**
   an entrant's wallet checks that the pair's genesis transaction spent the slot with a bind of `≥ c` (an indexer fact,
   like KOB's `genesis_verified`, `[K] matcher.md` §1.2a). Opens are safe (the orders check the bind on chain).
3. **A bonder vouches falsely on all its pairs at once at `ρ` = 10.** Live disputes restore every disputed pair; the
   slashes `σ·c` exhaust `K` after `1/(ρσ)` = 100 % of the pairs at `σ` = 10 % (deterrent spent, victims unharmed).
   Undisputed pairs finalize; retro disputes are repaid first-come until `K` is gone, so 90 % of their value is lost.
   **Holds only if** victims or watchers dispute within `W`; **fails** for the share beyond `K` of the silent ones.
4. **Unbonding before a dispute resolves.** Unbond needs an empty ring, i.e. `CLTV` past `X + T_tail` of every pair;
   claims after `X` are void; reservations are separate UTXOs. **Holds**, with the boundary race at `X + T_tail`
   (a retro dispute and an advance are both valid: watchers act before).
5. **Judge captured by `f` of aged stake.** **Fails** for `f ≥ f*` (§2). Bribery to abstain lowers `p_h` cheaply
   (O§4.3): **fails** against stakers who take it. A late flood: only stake aged `≥ M_age` counts: **holds only if**
   the flood was not prepared `M_age` ahead (patient capital is just `f`).
6. **Honest bonders too passive.** `f*` scales with participation: 17 % at `p_h` = 0.25. **Holds only if** honest
   underwriters defend their own claims (they are slashed if they do not) and keep an aged reserve above `μ ×` what an
   attacker can deploy in parallel disputes (O§2).
7. **Offline victim.** Watchers within `W`, then anyone within `T_retro` (the victim itself on return). **Holds only
   if** someone disputes within `W + T_retro`; repayment capped by the slot (3).
8. **Slow bleed by slightly wrong prices.** With the tolerance inside the trigger and closes moved by `g` against the
   closer, a bias `< g` liquidates no healthy pair and pays no closer more than fair. **Holds**; the price is that every
   honest closer pays a spread `g` (set `g` just above feed noise: 0.2–0.5 % for gold *(est.)*).
9. **Stale or delayed claims.** `τ = P` beyond `Δmax`; void after `X`; revocation by spending. **Holds**, given Z2.
10. **KAS collapse.** Form (a): bond and margin fall together; `ρ` unchanged. **Holds.** Form (b): beyond the haircut
    the slot under-covers, and the judge's stake in dollars shrinks while dollar margins do not, so capture becomes
    cheaper. **Holds only if** `h` exceeds the KAS fall within `Λ`; **fails** in a crash.
11. **Bonder is a party to its pair.** `uwKey ≠ longKey, shortKey` is enforced; a Sybil key evades it. Its false
    claims are ordinary false claims (3), with a sharper motive. **Holds only if** the counterparty or a watcher
    disputes; rebinding before `X` needs both parties, so a party cannot move a pair to its own slot.
12. **Griefing by frivolous disputes.** Costs `d_1`, paid to the vouchers when upheld; the contested part is frozen
    for `≥ D_r`, but its outcome (forfeit, or payout at `p`) is fixed, so only time value is lost. **Holds.**
13. **Blocking by a captured judge.** The colluders dispute true claims on pairs where they are the loser; the judge
    restores, and the honest underwriter is slashed. Idle settle at `E` would let them run out the clock, so it is
    removed (§1.4). **Fails** under capture, bounded by the colluders' own margin in those pairs and by honest bonds.
14. **Pair stranded** (underwriter slashed below its ring, unbonded, vanished). Slashed: binds stop, existing cover
    stays counted. Cannot unbond with cover. Vanished: request (§1.6) → rebind to `altSet`; after `X + G`, lock path.
    **Holds only if** `altSet` holds a live underwriter or a staker uses the lock path; otherwise only price-free exits.
15. **Scan, signature and 64-bit limits.** One sigop per claim (the underwriter's own input); ≤ 7 pairs per renewal,
    ≤ 7 children per retro reservation; amounts in KAS units (§1.3); premium `π·c·Λ` as a split product like `q`.
    **Holds** (Z4).
16. **A UTXO every liquidation must touch.** None: a claim spends the pair and the underwriter's own P2PK input; the
    slot is touched by binds, renewals and disputes. **Holds.** The underwriter serialises only its own inputs.
17. **Extortion by the incumbent** (refuses to renew unless paid more). Bounded by `altSet` and by the lapse rule.
    **Holds only if** `altSet` is non-empty and honest.
18. **Fake resolved dispute** to drain a reservation. The reservation names the dispute id (the pair id); a second
    UTXO of that id can only come from the pair's own templates or a hidden genesis output, which T1′ excludes. **Holds
    only if** Z11 shows the output count is enforceable at genesis.

## 6. What can be said in public (oracle-only markets)

**True.** "No admin keys, no operator, no upgrade path." "Anyone creates markets; nobody approves them; read the feed
definition and parameters before you trade." "A covenant holds your margin, but whoever sets the price can move it to
your counterparty: price control is custody control." "Each pair has one underwriter, bonded for at least `1/ρ` of
what it underwrites; only it can price your pair." "A false price disputed within the window is reversed and the
underwriter slashed; disputed within `T_retro` after settlement, you are repaid from its bond, first come, while it
lasts." "Disputes are won by the side committing more aged bonded KAS, by a factor `μ`; if colluders control that,
false prices stand and nothing is repaid." "The winner bears gaps beyond the loser's margin." (Form a) "Prices are in KAS."

**False.** "Trustless", "no trusted oracle", "decentralized oracle" as a security claim (the trust is an honest
majority of participating bonded KAS). "Insured", "fully covered", "protected up to the bond" (cover is `1/ρ`, and
nothing under capture). "The total loss is capped" (only per underwriter, §2). "Liars are always slashed" (only when
disputed and outvoted). "Gold price", "S&P", or any licensed index name, and "gold in dollars" for form (a). "Listed",
"verified", "official" for any market. Markets on individual stocks, bonds and rates may fall under securities or
derivatives rules in many places; this note takes no position on that.

## 7. Verdict, staging, experiments

**Verdict.** The mechanism holds under: an honest majority of *participating aged* stake by the factor `μ` in every
dispute; a dispute within `W + T_retro` of every false claim; wallets that hash-check feed definitions and pair
geneses; `mmr_w` above the closure gap. Then a false price is reversed or repaid and an underwriter's lie costs it
`σ·c` per pair. It costs `y/(Lρ)` a year per side plus about two extra transactions per pair-month (renewal, record);
at `ρ` = 1, 10×, `y` = 10 %: 1 %. Still exposed: everything under a captured judge (§0.5), silent victims beyond
`1/ρ` of their underwriter's bond, the stranded lock path, issuer powers in form (b), KAS collapse beyond `h`.

**Changes to the decisions.** (4) Say "worst-case loss through colluding underwriters is `ρ ×` their bond", not "under
a colluding majority"; start at `ρ` = 1 and cap new markets at `ρ ≤ 5` until the judge has a history, because the cost
target is met without it. (4) The long delay belongs to the judge's stake (`M_age`), not to slot unbonding (§1.3).
(6) Prototype on (a), product on (b). (8) Idle settle at `E` must go in oracle markets.

**Smallest first version.** Form (a); `ρ` = 1 as a parameter (one template for all `ρ`); one slot child per
underwriter; no `KpfReq` (second version), lock path, retro dispute or `mmr_w` yet; live disputes with the O§2 judge;
one market, a slow underlying, on testnet. Then: retro dispute and records; `altSet` rebind and `KpfReq`; `mmr_w`;
`ρ` = 2; form (b).

**Order relative to guaranteed fast settlement** (O§0, book markets). Build that first (it needs no judge and no
honest majority). Write its `KpfLock` as one status of a shared bond template `KpfBond` (statuses: lock, slot,
reserved, free stake, unbonding; dispute and receipts as companions), so the oracle slot of this note, the stake of the
judge and the book-market lock share units, unbonding, reserve and burn code. The extra statuses grow the template
to about 6 kB *(est.)*; if Z4 shows that too heavy, keep two templates sharing the library.

| # | Experiment | Pass criterion |
|---|---|---|
| Z1 | `OpTxInputSpk` reads another input's P2PK script; `checkMsgSig` cost (O Z1) | an underwriter's input is provable; or a data signature priced like `checkSig` |
| Z2 | `P` exact at finalize; `P > X` restores; `F − t0` reproducible (O Z2) | as stated |
| Z3 | ring arithmetic fuzz: bind, renew, split, merge, advance, reserve | `Σ O ≤ ρK` on every reachable state; no sompi created |
| Z4 | mass of T1′, renewal of 7 pairs, propose with the underwriter input, retro reservation of 7 children | within 500,000 compute mass |
| Z5 | contested pending → dispute → restore / finalize; reservation release and pay | each branch; no co-spend of two parts |
| Z6 | rate limit `a`: a non-owner reserve lands between owner spends under load | ≥ 99 % within one window |
| Z7 | form (b): T1′, propose, finalize, whole transfer within 3 token inputs/outputs; a paused token | fits; pause halts, nothing is lost |
| Z8 | ring capacity over a simulated month of opens and renewals | `ρ_eff` within 5 % of the formula |
| Z9 | closure-gap statistics (99th, 99.9th percentile) for gold, FX, oil, wheat, a stock, a rate future | `mmr_w` per underlying |
| Z10 | judge drill: 100 parallel disputes, half participation, a patient attacker | `f*` measured against O§2 |
| Z11 | `OpCovOutputCount(newId)` (or an output scan) at the genesis of a pair | a second output of the new id is refused |
| Z12 | DAA-to-timestamp mapping for `τ` (block time drift) | judge tolerance under 1 min |

## 8. A different mechanism worth comparing: one underwriter per side

Each party chooses its own underwriter and a claim needs **both**; if one is silent for `T_req` the other's claim
proceeds and the silent one may dispute. A party's loss then depends only on the underwriter *it* chose. Costs: two
premiums, two inputs per claim, and liveness that depends on the loser's own underwriter, which has every commercial
reason to delay its client's liquidation. Under a captured judge it adds nothing. Worth a drill after Z10, not a change.
