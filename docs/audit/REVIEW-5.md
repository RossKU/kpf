# KPF review 5

## Scope and method

Target: commit `538e42a` (guaranteed fast settlement): `contracts/KpfLock.sil`, `KpfPending.guarantee`, the
`propose` and `checkMarket` changes in `KpfPair.sil` and `KpfOpen.sil`, the library (`lock.rs`, `evidence.rs`,
`pending.rs`, `state.rs`) and the commands `guarantee`, `dispute`, `release`, `watch`, `quote`. Read against DESIGN 3.1,
4.2-4.6, 4.5a-4.5f, THREAT_MODEL section L and reviews 1-4 with their fixes.

Method: every claim below is an engine run of the real scripts (KPF, KOB ask and bid, the KCC-20 token program) or a
run of the `kpf` binary against the in-memory chain, which re-runs every input in the engine. New files only:
`crates/kpf-tests/tests/audit5_tests.rs`, `crates/kpf-cli/tests/audit5_cli.rs`, this report. The full workspace passes
(`CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2`); every `exploit_` test asserts that the attack is accepted.

## Findings

| # | Severity | Title | Test |
|---|---|---|---|
| R5-F1 | High | The first disputer chooses the restitution quote; a party of the claim pays the victim the bound's value for free | `audit5_tests::exploit_the_first_disputer_prices_the_restitution_at_its_own_boundary_order` |
| R5-F2 | High | `kpf guarantee` accepts a look-alike pending part with an out-of-range market and tells the guarantor no quote disputes it | `audit5_tests::exploit_a_look_alike_part_with_an_out_of_range_market_takes_the_guarantors_lock`, `audit5_cli::exploit_guarantee_tells_the_guarantor_that_no_quote_disputes_a_look_alike_part` |
| R5-F3 | Medium | `kpf watch` gives a pending liquidation's loser the slow-path window and a cure that a guarantee can remove after `G` | `audit5_cli::exploit_watch_promises_the_window_and_the_cure_while_a_guarantee_can_settle_now` |

No Critical and no Low finding. Two informational notes follow the findings.

### R5-F1 (High). The first disputer chooses the restitution quote

**Affected:** `KpfLock.dispute` (the computation of `H`), DESIGN 4.5f "Restitution", THREAT_MODEL L1 and L5.

**What happens.** `H` is priced at the quote of the counter order *that the dispute uses*. Any order of the right kind
that contradicts the claim by `g` and rested `R` from `P` qualifies, and the first dispute to land spends the lock. A
party of the claim (the closer, the winner, the guarantor or a friend) disputes first with **its own** order at the
refutation bound and fills it itself. An ask above the market or a bid below it is not filled by arbitrage, so it rests
on any book: no thin book is needed. The victim is paid what its side is worth at the bound, not at the market. The
rest of what it is owed is burned.

**Who loses what** (DESIGN 5 pair, testnet ratios):
- *Close.* The long closes at a bid of 10.5 and guarantees its own claim. The market then falls to 9.00, and a third
  party's ask at 9.00 rests from `P`. A dispute on that ask would pay the short `H` = 1,500 KAS, so the short would end
  with its whole side, 3,000.2 KAS. The long disputes first with its own ask at 10.29: `H` = 210 KAS. The short ends
  with 1,710.2 KAS, **1,290 KAS less**, and those 1,290 KAS are burned. The long's net is 1,590 paid − 1,650 locked +
  150 reward = 90 KAS, which is its own bond, exactly what finalize at the true price would have paid it. The attack
  costs the long nothing.
- *Liquidation.* A liquidation of the long at 9.40 is guaranteed by the winner (the short). The market recovers to 12,
  and a bid at 12 rests from `P`: `H` on it is 3,000 KAS. The short disputes first with its own bid at 9.66315790:
  `H` = 663.16 KAS, and the long **loses 2,336.84 KAS**. The short's net is about 0: it gains the long's side and
  loses the lock less the reward.

L2's argument ("a lie is never a free option") holds: the coalition never profits. But it bounds only the coalition's
gain. It does not bound the victim's loss: whenever the claimant's true payoff is near zero, the claimant can burn up to
`cover − H(bound)` of the victim's money at no cost. Honest watchers do not cause this. A watcher filling an ask
prefers the cheapest ask, and one selling into a bid prefers the highest bid, which is the victim's best quote. The
harm needs a disputer that fills its own order. On the slow path the same move restores the part in full.

**Smallest fix.** Remove the disputer's choice of price: pay the victim the whole `cover` on any successful dispute.
`λ ≥ 1` covers it, and the burn shrinks to `lock − cover − R`. A narrower alternative is to price `H` at the bound
itself for every dispute and state it in DESIGN 4.5f: the victim then gets a known floor, and nothing depends on who
disputes first. Either way, change the claim "the victim ends whole at the counter quote" in DESIGN 4.5f and THREAT_MODEL
L1 and L5.

### R5-F2 (High). A look-alike pending part the tooling calls undisputable

**Affected:** `check_pending` / `check_guarantee` (`crates/kpf-protocol/src/evidence.rs`), `kpf guarantee`
(`evcmds.rs`), `KpfPending.guarantee`. THREAT_MODEL G4 says a look-alike pending part has "no effect on other parties'
funds". The fast path breaks that: the guarantor's funds are now at stake.

**What happens.** An attacker creates a genesis UTXO of the pinned `KpfPending` template. It computes the restore, cure
and forfeit hashes from the part's own state and funds the part with `q(n, bs − bl) + refund`. That is everything
`check_pending` checks. The market block has `mmrBps = −20,000` and `gBps = 20,000`. `Market::validate` and
`checkMarket` refuse these values, but nothing on the guarantee path calls them: `check_lock` does, `check_pending`
does not. On chain, `KpfPending.guarantee` re-checks only `λ` and `D`. The part claims that the short is liquidatable
at 11.50.

For this claim, `contradicts(c)` is `c·(10⁴ − 20,000) ≤ bs·(10⁴ − 20,000)`, i.e. `c ≥ bs`. That condition is reversed,
so the library's binary search finds no bound. `kpf guarantee` prints "no quote in 0..=priceMax disputes this claim",
omits its RISK line, offers the largest offer as bait, and builds and verifies the guarantee. The chain accepts it.

The attacker then disputes at once with its own ask at 13 KAS (above `bs`), filled by itself. `KpfLock.dispute`
accepts it, `H` is 0, the attacker keeps `R`, and the rest is burned.

**Who loses what.** The guarantor loses its whole lock, 3,300 KAS: 300 KAS to the attacker and 3,000 KAS burned. The
attacker gets its own pending value back through the settlement, because both sides and the proposer key are its own.
Its net is `R` less the offer and fees. With `λ = 3` it would take two thirds of the lock. A real market's part with
the same claim is not disputable by that ask (control in the test).

**Smallest fix.** In `check_pending`, call `w.m.validate()` and refuse a part whose market fails it. Also give
`kpf guarantee` the `--market` check that `create-order` has, or require `--pair`. On chain, have `KpfPending.guarantee`
re-check `0 ≤ mmrBps < 5,000` and `0 < gBps < 10⁴`, as it already does for `λ` and `D`, so that no look-alike can
create a lock whose dispute rule is inverted.

### R5-F3 (Medium). `watch` gives the loser W and a cure, but the claim can be fast-settled after G

**Affected:** `watch_cmd` (pending parts), `finalize_words`, and the JSON fields `finalizeInDaa` and `cureBound`.

**What happens.** Take a stale liquidation proposal on a testnet market (`W` = 600, `G` = 100). At `P + G`, `kpf watch`
prints "finalize from DAA P + W: 500 DAA, 50 s from now", offers `kpf cure` and `kpf challenge`, and says nothing about
guaranteed fast settlement. In the same block, a guarantor settles the claim with `kpf guarantee`. The pending UTXO
that the offered cure would spend is gone: there is no cure, and a refutation now pays cash at the counter quote
(R5-F1). DESIGN 4.5f's accepted limit (`exploit_a_band_loser_offline_for_the_grace_forfeits_its_residual`) is a loser
that is *offline* for `G`. Here a loser that is *online* and follows the tool also loses its residual: up to
`q(n, T(1 + g) − bl)`, 663 KAS on the DESIGN 5 pair. The same holds for a close's other party, which `watch` does not
tell that the close can be settled at `P`. `propose-close` and `propose-liquidate` (`print_proposal`) also describe only
the slow path.

**Smallest fix.** For every pending part, print the first DAA at which it can be guaranteed (`guarantee_from`) when
`P ≤ visibleUntil`: "a guarantor may settle this claim from DAA X (Y from now); after that there is no cure, and a
refutation pays cash from the lock". Put the same field in the JSON output, and state in `print_proposal` that the
proposal can be fast-settled.

### Informational (not counted)

- **`lock_words` reads as a deadline to act.** "disputable with counter-evidence resting by DAA X (Y from now)" counts
  `Y` to the rest *end*. An order placed now finishes resting only at now + `R`, so a victim without an order has
  `Y − R`. The per-order verdicts (`check_dispute` with the order's `rest_end`) are right; only the headline can mislead.
- **The guarantor key is not checked on chain.** `KpfPending.guarantee` takes any 32 bytes as `gk`. A lock whose owner
  key is not a valid x-only key can never be released, and only its own funder loses. The library checks the key.

## Unproven

- **Griefing by the burn on a close with a dust victim.** When the other party of a close is owed less than 0.02 KAS,
  the cover is the whole part less the refund, and R5-F1 then scales with it. The test covers the ordinary close only.
- **Mempool scores.** A dispute that spends a counter order not yet accepted reads its score as −1. Its exposure
  becomes `P`, and the mempool may relay a dispute that consensus, with the order's real score, refuses. Proven only
  for the lower bound (`holds_the_window_ignores_funding_scores_and_an_unaccepted_counter_order_counts_from_p`).
  Relay behaviour is not modelled.
- **Front-running of disputes.** A dispute's reward goes to the executor's change, so a relay observer can copy a
  pending dispute and take `R`. This is the same generic copying as review 4's relay copy. Combined with R5-F1, the
  copier can also re-point the dispute at its own boundary order. Not built.
- **`D` and `G` on a real market at their edges** (`D = 2R`, `G = 0`). These follow from the market's consent and were
  not tested beyond the existing boundary tests.

## Checked and found sound

- **Lock binding.** The lock state carries the market block byte for byte, the claim id (`selfId`), the six claim
  integers (one slice of the pending script), `P`, the cover and the victim. Every field is fixed by
  `KpfPending.guarantee` from its own script. The lock output is located by `j`, but its script must equal the P2SH of
  that exact state, so it cannot coincide with any settlement output (P2PK, forfeit P2SH), with another claim's lock, or
  with a later copy.
- **Value and conservation.** Close: `me + other = v`, and the offer is cut from `me` only. Liquidation: forfeit
  `≥ v − B − fee`, proposer `B + fee − offer`, offer `≤ B − 0.02 KAS`. Lock `≥ ⌈λ·cover⌉ + offer ≥ H + R`. `H ≤ cover`,
  and the dust raise of `H` comes out of the burn (lock floor 0.1 KAS). The reward is exactly what no output holds.
  Payouts are positional (`i`; reverse slot `M − 1 − i ≥` input count; KOB pays at the evidence index), so no output
  serves two obligations.
- **Victim key.** The other party of a close (`side` long → short, and the reverse); the loser of a liquidation. It is
  fixed at the guarantee, so splits, closes and sales of the forfeited part cannot redirect it (existing L9 tests). The
  forfeited part is one-owner and cannot be liquidated again.
- **Windows and clocks.** `F` and `P` are UTXO scores, and both must be `≥ 0`. `restEnd + R ≤ F + D` does not depend on
  the lock time or on the funding inputs' scores (new holds test). An unaccepted counter order counts from `P`, never
  earlier. The release CSV is `D < 2³²`. The liquidation guarantee CSV is `G ≤ W < 2³²` on real markets.
- **Orderings.** Challenge, cure, finalize and guarantee spend one pending UTXO, and dispute and release spend one
  lock. After a guarantee, neither the lock nor the forfeited pair has a cure, challenge, finalize or second-guarantee
  entry (new holds test).
- **Arithmetic.** `c·(10⁴ ± mmr/g)` ≤ 10¹⁸. The split products of `need` and `R` stay within 63 bits up to the KAS
  supply at `λ = 3`. The burn and `H` cannot go negative.
- **Burn.** P2SH(`OP_RETURN`) is unspendable (Z7), and the burn slot must be at or after the input count.
- **`quote` and `guarantee` summaries** match what the builders produce for genuine parts (lock = `lock_min`, payouts =
  `guarantee_close` / `guarantee_forfeit`). `dispute` and `watch` refuse locks whose market fails `validate`.

## Not covered

`KpfOpen` beyond the `checkMarket` diff; the compiled `KpfLock` artifact against its source beyond the pinned-artifact
and layout tests; the X6 random walk's new guarantee branch; relay and mempool policy; the `release` command beyond a
read of its code.
