# KPF two-step evidence (design note, draft 1)

Status: implemented as `DESIGN.md` 4.5d, with the resolutions of section 7 adopted for 1-6 and not adopted for 7 (a
challenged close is cancelled, not re-settled at the counter quote), and with the deviations listed in DESIGN 4.5d.
The rest of this note is kept as the record of the proposal. Cited as `D§n` (DESIGN), `TM` (THREAT_MODEL), `R3`
(review 3). Section 4.5c is taken as the current rules. Numbers marked *(estimate)* are not measured.

## 1. Problem and idea

A covenant proves only lower bounds on time (lock time, a UTXO's DAA score, a UTXO's age). "This evidence order is
still fillable now" cannot be checked, so an order that rested honestly and then soft-expired can be used later
(R3-F1). 4.5c bounds the evidence from below by the pair's clock `S` and accepts barrier semantics and a look-back.

Idea: no evidence path settles at once. A claim is **proposed** with a bond, the part becomes **pending**, and for
a window anyone may **challenge** it with counter-evidence whose rest starts after the proposal. Freshness of the
counter-evidence needs only a lower bound on time (the pending UTXO's own DAA score), which covenants can prove. After
the window anyone **finalizes**; the owner of a threatened side may **cure**.

## 2. Mechanism

### 2.1 `KpfPending`, a second template under the pair's covenant id

`KpfPair` gets one new evidence entry, `propose` (paths 5 and 6; 7 is replaced, §2.5), and loses the settlement code
of P5/P6/P7, which moves to `KpfPending`. The pending part continues the pair's covenant id into the other template
(one id, two templates: X12). `KpfPair` holds `PENDING_TPL` as a constant; `KpfPending` never needs the pair's
template, because `propose` precomputes every pair output the pending part can return to (below). So there is no
template-hash cycle and `KpfOpen` is unchanged.

State of `KpfPending` (written by `propose`, immutable; about 420 B *(estimate)*):

| Field | Bytes | Meaning |
|---|---|---|
| evidence market | 32 ×3, 8 ×3 | token covenant id, token template hash, extension commitment, template cut, `scale` |
| kcBps, minRestDaa, gBps, challengeDaa | 8 ×4 | counter-evidence multiple `k_c`, rest `R`, contradiction margin `g`, window `W` |
| mmrBps, kind, side | 8 ×3 | `kind` 1 close (P5), 2 liquidation (P6/P7); `side` = the closer (kind 1) or the loser (kind 2) |
| n, BL, BS, quote | 8 ×4 | size of the part, its bounds, the proposed quote `p` (bid for a long close / short loser) |
| fee, bond, refund | 8 ×3 | keeper fee (kind 2), bond `B`, `refund` = `B` + the carrier the proposer funded |
| longKey, shortKey, proposerKey | 32 ×3 | payout keys; `proposerKey` receives `refund` and `fee` |
| restoreHash, cureHash, forfeitHash, cureBound | 32 ×3, 8 | P2SH hashes of the three `KpfPair` outputs this part may become; the cured bound |

### 2.2 Propose (`KpfPair.propose`) — replaces the execution of P5, P6, P7

Who: P5, the owner of the closing side (signature, as today); P6, anyone. Checks: everything `onEvidence` checks
today, unchanged — evidence identity, `exposed' = max(exposed, S)`, rest and fillability through `exposed' + R`, the
evidence block `n_ev ≥ ⌈k·Σn⌉`, the trigger, the range rule of 4.5c, the fee and its cap — plus
`minEvidenceLife` (§4). Outputs (found by `OpCovOutputIdx(selfId, ·)`, at most two of the id):
- the **pending part**: P2SH of `KpfPending` with the state above, value `≥ q(n, BS − BL) + B` (+ `C` when `n < s`);
  with `n = s` it inherits the pair's carrier and any excess, with `n < s` the proposer funds a carrier `C`, and
  `refund = B + C`;
- the **remainder** `(s − n)` exactly as on every split today (keeps `since = S`, the old carrier and the slack).
`propose` computes `restoreHash` = the part as a `KpfPair` with every field unchanged, tags 0 and `since = 0`;
`cureHash` = the same with the loser's bound moved to `cureBound` and `since = S` (kind 2 only); `forfeitHash` = the
part with the loser's key replaced by the winner's, both sides unlisted, tags 0, `since = 0` (kind 2 only).
`cureBound`: long loser `BL* = ⌊p·(10⁴ − mmrBps)/10⁴⌋`, short loser `BS* = ⌈p·(10⁴ + mmrBps)/10⁴⌉`, the nearest bound
at which the proposed quote no longer triggers. The bond is `B = max(bondMin, q(n, ⌊(BS − BL)·bondBps/10⁴⌋))`.

### 2.3 Challenge (`KpfPending.challenge`)

Who: anyone. Checks: `OpCovInputCount(selfId) == 1`; counter-evidence at input `ev` read with the 4.5b/4.5c reader,
with the pending UTXO's own DAA score `P` as the clock: `exposed_c' = max(exposed_c, P)`, `tx.daa ≥ exposed_c' + R`
by CLTV, fillable by conforming matchers through `exposed_c' + R`; the evidence block counts this part's `n` with
multiple `k_c`; the counter-evidence is of the opposite kind and contradicts the claim (§3). Outputs: exactly one
output of the id, with `restoreHash` and value `≥ V − B` (`V` this UTXO's value). The bond is what no output is
required to hold, so it is the challenge executor's (KOB precedent: the executor keeps the excess). With `n < s` the
restored part keeps the proposer's carrier: a failed proposer loses `B + C`.

Existing third-party orders qualify: an order resting since before `P` counts from `P`, provided it was still fillable
at `P + R`. Counter-evidence does not need `minEvidenceLife`; its freshness comes from `P`.

### 2.4 Cure (`KpfPending.cure`, kind 2 only)

Who: the loser's owner (signature). Output: exactly one output of the id with `cureHash`, value
`≥ q(n, BS − BL*) + C` (long loser; mirrored), plus `proposerKey` paid `≥ refund` at the positional slot. The curer
funds the added margin and the cured part's carrier. No cure fee: a fee would pay stale proposers (§5a). The cure
moves the bound exactly to `cureBound`; more margin is an ordinary P2 afterwards. `since = S` is kept (4.5c: adding
margin does not restart the clock).

### 2.5 Finalize (`KpfPending.finalize`)

Who: anyone, `age ≥ challengeDaa` (CSV on the pending input, a relative lower bound). No evidence. Outputs, with `i`
this input and `j = M − 1 − i ≥` input count (D§4.4):
- **kind 1 (P5 close at `p`)**: the closer at its slot `≥ q(n, c(p) − BL) + refund` (long; short `q(n, BS − c(p))`),
  the other party at the other slot `≥ V − ` that; no output of the id. The non-actor takes the rounding.
- **kind 2, close both (P6)**: the winner `≥ V − refund − fee`, `proposerKey ≥ refund + fee`; the loser 0.
- **kind 2, forfeit and keep (P7′, replaces P7)**: one output of the id with `forfeitHash`, value `≥ V − refund − fee`
  where the forfeited bound is first moved inward by the fee (`BL′ = BL + ⌈fee·scale/n⌉`, so the part's range still
  covers its value; the forfeited side is the winner's own, so no third party is downgraded); `proposerKey ≥ refund +
  fee`. The winner now holds both sides of the part (no price exposure) and sells the loser's old side by an ordinary
  listing and P3, keeping its own position: P7's outcome in two ordinary steps, with no entrant during the window.
Payouts below dust follow 4.6 (they stay with the other party); the fee is computed at propose with today's formula
and cap, and goes to the proposer, not to the finalizer, because the proposer carried the bond risk.

### 2.6 Rules of 4.5b and 4.5c

| Rule | Status |
|---|---|
| fillable through `exposed + R` (4.5b) | kept for proposals; generalised by `minEvidenceLife` (§4); applied with `P` to counter-evidence |
| evidence clock `since` (4.5c) | **kept**: bounds the proposal's evidence; a successful challenge restores with `since = 0`, which kills every older quote against that part (an attacker's whole library) |
| hand-over rate limit `S + 4R` | **kept**: two-step does not change how fast a side can re-clock itself by transfer; P7 exemption moot (P7′ uses P3) |
| barrier semantics | **replaced** by hold semantics: a liquidation holds only if no contradicting quote rests in `[P, finalize]` |
| look-back of P5 | **reduced** to quotes within `g` of the market and to markets with no opposite book (§5h, §5i) |
| forfeiture, fee cap, range rule, health at entry | kept unchanged |
| P7 entrant chosen in the evidence transaction | replaced by P7′; R3-F3's check moves to the later P3 (health at entry at `X`) |

## 3. What contradicts what

`m = mmrBps`, `g = gBps`, all products below 2·10¹⁸ for prices `≤ priceMax`. Liquidation claims are contradicted
relative to the **trigger**, not to the quote, because the forfeiture does not depend on the quote; close claims
relative to the **quote**, because the payout does.

| Claim (evidence) | Counter-evidence (fresh, opposite kind) | Contradicts iff | Tie |
|---|---|---|---|
| close long at bid `b` | ask `a` | `a·10⁴ ≤ b·(10⁴ − g)` | equality contradicts |
| close short at ask `a` | bid `b` | `b·10⁴ ≥ a·(10⁴ + g)` | equality contradicts |
| liquidate long at ask `a` (P6 or P7′) | bid `b` | `b·(10⁴ − m) ≥ BL·(10⁴ + g)` | equality contradicts |
| liquidate short at bid `b` (P6 or P7′) | ask `a` | `a·(10⁴ + m) ≤ BS·(10⁴ − g)` | equality contradicts |

With `g = 0` the liquidation rows say "the side is not liquidatable at the counter quote" (the strict trigger of D§3.6
fails), and the close rows "the counter quote is strictly better for the non-actor". Maintenance band: a counter bid
with `BL·10⁴ ≤ b·(10⁴ − m) < BL·(10⁴ + g)` — the long is healthy at it, but by less than `g` — does not contradict;
such a victim must cure (§2.4). A counter bid in `(a, T)` never contradicts: the long is still liquidatable there. P7 and P6 are one claim
(kind 2); the choice between close-both and forfeit-and-keep is made at finalize, so no `KpfOpen` is touched while
pending, and an entrant order rests, cancels or refunds exactly as today.

## 4. Parameters

| Parameter | Bound and reason | Testnet value |
|---|---|---|
| `challengeDaa` `W` | `≥ R + detection + inclusion + margin` for a watcher (§5a); `≤` the time in which the maintenance band absorbs a move, since every liquidation is delayed by `W` and the winner carries the gap (B3) | 6,000 DAA (10 min) with `R = 600` |
| `bondBps`, `bondMin` | proportional to the part's **range** `q(n, BS − BL)`, a state quantity that bounds the theft a stale claim can make (a quote is attacker-chosen). Lower bound: pays a watcher's cost (fees + spread × `k_c·n`). Upper bound: honest keeper's EV `fee − Pr(price recovers by g in W)·B > 0`. Deterrence of false challenges: `g·T·k_c·n > B` (§5b) | 300 bps of range, `bondMin` 5 KAS |
| `kcBps` (`k_c`) | `≥ k`: a counter print must cost as much to fake as the claim. Larger raises the defender's capital (§5h) | `= kBps` (1) |
| `gBps` (`g`) | trades the loser's free false challenge near the trigger (needs `g` large) against an honest victim within `g` of the trigger (must cure) and the P5 look-back (`≤ g·b·n`) | 200 (2 %) |
| `minEvidenceLife` `L` | proposal evidence must stay fillable (GTC, or `expiryDaa`, kill, 90-day limit all `≥ restEnd + L`). Use within `L` of `restEnd` is then a live quote; a library quote must survive visible for `R + L`. `L ≥ 600` excludes IOC/FOK; `L ≤ 77,760,000 − R`; larger `L` shrinks evidence supply | 864,000 DAA (1 day) |

**Worked example** (D§5 pair: 1,000 tokens, `BL 9`, `BS 12`, `C 0.2`, `mmr 5 %`, `liqFeeBps 50`, `k = 1`, `T =
947,368,421`). `B = q(10⁹, ⌊3·10⁸·300/10⁴⌋) = q(10⁹, 9·10⁶) = 90 KAS`. Liquidation contradiction:
`b·9500 ≥ 9·10⁸·10200` ⇔ `b ≥ 966,315,790` (9.6632 KAS).
- *Stale attack.* Market 10. The short holds a dead ask at 9.45 that rested after `S`; under 4.5c it liquidates the
  long at once (1,000 KAS). Two-step: it proposes; pending value 3,000.2 + 90 KAS. A watcher crosses a third-party
  ask at 10.02 with a third-party bid at 9.98 that was fillable at `P + R` (spread cost 40 KAS + fees), challenges at
  `P + 600` and nets about 50 KAS; the part returns with `since = 0`, the attacker loses 90 KAS and its library.
  The victim alone could instead cure: `BL* = ⌊945,000,000·9500/10⁴⌋ = 897,750,000`, margin `q(10⁹, 2,250,000)` =
  22.5 KAS, which stays its own.
- *Honest liquidation.* Market 9.3, live ask 9.3. Fee `q(10⁹, 4,650,000)` = 46.5 KAS (cap 473.68). A false challenge
  needs a bid ≥ 9.6632 filled for 1,000 tokens; on an arbitraged book the long buys 1,000 tokens 0.3632 above the
  market (−363 KAS) to collect 90: it loses 273 KAS per cycle and the keeper 90, so it stops. On a book with nobody to
  sell into the bid it costs nothing (§5b).
- *Finalize after 6,000 DAA.* The short gets `3,000.2 + 90 − 90 − 46.5` = 2,953.7 KAS, the proposer 90 + 46.5.

## 5. Attacks

**(a) Stale or dead proposal.** Online victim: challenges with any order fillable at `P + R` (its own, self-filled,
or two crossing third-party orders), or cures; it wins the bond. **Holds.** Offline victim: only watchers defend,
paid by `B`. `W` must exceed the watcher's `R` + latency with margin, and `B` must exceed its cost; if no watcher
acts within `W`, the stale claim finalizes exactly as under 4.5c. **Holds only if** a funded watcher is live during
`W` (a liveness assumption that replaces R3's keeper-refund assumption). Victim within `g` above the trigger: cannot
challenge, must cure (owner online with KAS). **Holds only if** the owner or its wallet is online within `W`.

**(b) False challenge by the deserving loser.** Its counter-order sits beyond the trigger by `g` and must be filled
for `k_c·n`. On an arbitraged book whoever sells into it gains `(T(1+g) − p)` per token; the loser's cost per cycle
`≥ g·T·k_c·n` exceeds its gain `B` when `g·k_c·T > bondBps·(BS − BL)/10⁴` (example: 189 > 90 KAS). It does not keep
its residual: the keeper re-proposes with its still-live evidence after `R` (since 0 delays it by `R`). A keeper with
token inventory takes the counter-order instead and profits; filling it before the challenge removes it. **Holds
only if** the bid side has `k_c·n` of arbitrage at `T(1+g)` within `R`. On a book with no token holders (testnet-10)
the loser cancels every proposal for free and collects every bond: honest keepers stop and the winner carries the
loss. **Fails** there — the mirror of A1, moved from the victim to the winner. Without `g` it fails near the trigger
on every book (cost → 0 as `p → T`).

**(c) Self-challenge and bond passing.** A proposer that challenges itself pays fees and a counter fill to cancel its
own claim and recover its own bond; two colluders passing the bond move value only between themselves. The sole
external effect is a freeze, (d). **Holds.**

**(d) Proposal spam (freeze).** A proposal needs evidence that liquidates (or closes) under today's rules, so only a
part that 4.5c would settle at once can be frozen. Cost per freeze: two fees and a self-fill; the bond is lost to
whoever challenges first at `P + R` (an online victim races for it; pre-placed counter-orders count from `P` for
everyone). The restored part has `since = 0`, so the same library cannot be reused. Harm: neither side of the part
can close, transfer, list or add margin (except cure) for `R` (online victim) to `W` (offline). **Holds**, with a
residual: a cheap freeze of up to `W` per stale quote against an offline victim's part.

**(e) Actions on a pending part.** `KpfPending` has only challenge, cure and finalize. Refused, and why: *add margin by
the winner, amend, list, transfer (P3), close-versus-close (P4)* — the loser could sell its side to escape
forfeiture, and the winner's recorded payout would go stale; *a second proposal* — one claim per UTXO, and a P5
proposal by a liquidatable loser would otherwise freeze out its liquidation (it still can race a proposal before
one lands, as B1 today); *idle settle* — finalize is the timeout. Allowed: cure (only to `cureBound`). The remainder
`(s − n)` is an ordinary pair and is unaffected. **Holds.**

**(f) Forfeiture.** A winner proposing what it expects to be challenged loses `B` and gains a freeze of at most `R`
against an online loser; it could liquidate outright under 4.5c with the same evidence, so the loser is never worse
off than today. Never finalizing: the payout is fixed at propose, any delay only gives the loser time to challenge,
and anyone (the loser or the non-actor of a P5 included) may finalize to end the freeze. **Holds.**

**(g) Races.** A challenge cannot be bounded above in time: it stays valid until a finalize lands, so `W` is the
earliest finalize, not a deadline. At age `W` challenge, cure and finalize are all valid and one is accepted;
defenders must act before `W − latency`, finalizers right at `W`. Two challenges: one wins, the other transaction is
invalid as a whole (its counter fill does not happen). Cure versus challenge: both cancel; the curer loses nothing it
does not keep. **Holds**, the boundary being a GhostDAG coin flip by design.

**(h) Defender capital.** A challenge must fill `k_c·n`. With a live book the defender needs no inventory: crossing
an existing ask with an existing bid in one transaction (the bid fillable at `P + R`) costs the spread × `k_c·n`
(40 KAS above); self-filling its own order needs `k_c·n` tokens and the matching KAS escrow, returned. A thinly
capitalised liquidation victim can always cure with KAS (cost small exactly when the stale quote is near the
trigger). A thinly capitalised P5 non-actor (the short against a stale bid) has no cure; **holds only if** an
opposite book or a watcher with inventory exists, else the look-back of 4.5c remains.

**(i) Thin or empty books.** No evidence: no proposal (K1, unchanged). Evidence but an empty opposite side: no
counter-evidence; stale liquidations are stopped only by cure, stale closes not at all. A token pause after a
proposal blocks every challenge (the token program refuses the counter fill) but not finalize. **Fails** for stale
P5 claims and for offline victims on a one-sided book; no worse than 4.5c.

**(j) Covenant ids and tags.** The pending part shares the id with the remainder, like any split today; the
`OpCovInputCount(selfId) == 1` guard on all three pending entries forbids co-spending them. Pending and restored
outputs carry no tags, so no `KpfOpen` can claim them, and no entry copies a tag (D§4.4 holds). The returned
`KpfPair` is fixed by a hash `propose` computed, so no party can be written out of it. A look-alike `KpfPending` from
a genesis can only pay out its own value (C3). **Holds**, provided X12's two-template continuation behaves.

**(k) Disappeared proposer; stuck funds.** Finalize needs no key and no evidence, so every pending part ends: anyone
finalizes after `W`. A lost `proposerKey` loses only its bond and fee. Branches: challenge (restored pair), cure
(cured pair), finalize (payouts or forfeited pair); each leaves an ordinary pair or plain outputs. **Holds.**

**(l) Cost.** Per liquidation: two transactions instead of one (propose ≈ today's evidence transaction plus one
P2SH output; finalize without evidence, about 5 kB of `KpfPending` script *(estimate)*), so roughly 1.4× the bytes
and fee, plus `B` locked for `W` and `C` funded by the proposer on a partial. Script: `propose` hashes three extra
11.6 kB pair scripts (restore, cure, forfeit; one for P5). For the 8-pair block that is about 280 kB hashed more; it
may not fit (today 1.60 M script units, 125,460 / 500,000 compute mass) and the block may have to drop to 4. A
challenge transaction is an evidence transaction over pending parts and uses the same block rule. `KpfPair` gains
`propose` and the hashes but loses the P5/P6/P7 settlement and P7's hand-over, so it need not grow *(estimate)*.

## 6. Verdict

The two-step design removes the stale-evidence problem **for parts whose opposite book is live during `W` and whose
defence (owner, wallet or paid watcher) is online within `W`**: stale proposals then cost the proposer its bond and
its whole library against that part, barrier semantics become hold semantics, and the P5 look-back shrinks to `g`.
It does not remove it on a one-sided or empty book (stale closes stand; stale liquidations stand unless cured), nor
during a token pause, nor for an offline victim without watchers. It costs a second transaction, a liquidation delay
of `W` (gap risk for the winner), a freeze of up to `W` that anyone holding a 4.5c-valid quote can impose, a new
template with its own evidence reader, and three extra pair-script hashes per proposal. Its new exposure is the
false challenge: on a book without arbitrageurs the deserving loser cancels every liquidation for free, so the design
moves the thin-book risk (TM T5) from the victim of a fake liquidation to the winner of a real one. On testnet-10 both
fail; the design is no worse than 4.5c on any book only if `g > 0` and the bond is small against `g·T·k_c·n`.

Experiments needed:

| # | Experiment | Pass criterion |
|---|---|---|
| Y1 | Id continuation `KpfPair` → `KpfPending` → `KpfPair` with a remainder of the same id; co-spend of pending and remainder | each path spendable alone; co-spend refused |
| Y2 | Script units and mass of `propose` (1 and 8 pairs) with three precomputed hashes | fits 500,000 compute mass; largest block size measured |
| Y3 | Challenge with zero-inventory crossing of two third-party orders, one resting since before `P` | accepted; an order dead at `P + R` refused |
| Y4 | CSV boundary: finalize at `W − 1` refused, at `W` accepted; challenge after `W` accepted while unfinalized | as stated |
| Y5 | Watcher latency on a test-engine book with proposals at random times | challenge lands before `W` in ≥ 99 % |
| Y6 | False-challenge drill (X9 extension): seeded and empty books, `g` and `k_c` sweep | loser's cost per cycle > `B` on the seeded book |
| Y7 | Evidence supply under `minEvidenceLife` on recorded KOB order flow | share of liquidations still provable |
| Y8 | Conservation fuzz over propose / challenge / cure / finalize with bond, carrier, fee and dust | no sompi created; proposer gets exactly `refund (+ fee)` |
| Y9 | Cure at `cureBound`: the same quote no longer triggers; a lower fresh quote does | as stated |
| Y10 | Token pause during a pending part (X13 extension) | challenge refused, finalize and cure accepted |

## 7. Recommended changes to the decision

1. **Contradiction margin `g`.** Without it the loser defeats any liquidation near its trigger at almost no cost on
   any book. Victims within `g` cure instead.
2. **Count counter-evidence from `P`, not "started after `P`".** `exposed_c' = max(exposed_c, P)` admits existing
   third-party orders still fillable at `P + R`, which is what lets a defender without inventory defend.
3. **Replace P7 by forfeit-and-keep (P7′).** No entrant is held while pending, `KpfOpen` is untouched, and
   `KpfPending` needs no pair template (no hash cycle). Liquidation by transfer becomes finalize + an ordinary P3.
4. **Precompute the returnable pair outputs** (restore, cure, forfeit) in `propose`; restore with `since = 0` so a
   refuted library is dead against that part. Cure only to a fixed bound, with no cure fee.
5. **State that the window is not a deadline.** Challenges remain valid until finalize; the proposer finalizes at `W`.
6. **Pay the keeper fee to the proposer**, not to the finalizer, or finalizers snipe the fee of the bond-holder.
7. Consider, for P5 only, a challenge that **settles at the counter quote** instead of cancelling (the closer gets
   `q(n, c(a) − BL)` and the challenger the bond): an honest closer then loses at most the price move, not `B`, and a
   stale bid still earns nothing. It needs `closeAt` in the challenge entry; worth it if Y6 shows honest closers
   losing bonds to market moves within `W`.
8. If Y2 shows the 8-pair propose does not fit, prefer a smaller block over dropping the cure hash.
