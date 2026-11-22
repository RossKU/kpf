# KPF review C10 (the fixes of review C9 on the cross-margin account, and the close that is planned)

## Scope and method

Target: `contracts/KpfCrossC.sil` `propose` kind 2 as changed for review C9 (a side is liquidated only while the
other party is at its maintenance at the same prices; the fee is at most what the liquidated side is owed at the
attested prices), the model (`Cross::liquidatable`, `Cross::fee_of`), and, on paper and on the model, the close the
coordinator plans (`close by A: vaG >= mG || pot - vaF < mF`; `close by B: pot - vaF >= mF || vaG < mG`), which
is not in the tree.

Files of this round: `crates/kpf-tests/tests/auditC10_tests.rs` (4 tests: 1 `c10_holds_`, 3 `c10_bounded_`).

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`):

- `kpf-tests`: `auditC10_tests` `ok. 4 passed`; `auditC9_tests` `ok. 5 passed` (with the coordinator's edits: no
  tick with both sides liquidated, no fee above what the loser is owed).
- `kpf-cli`: `auditC9_cli` `ok. 1 passed` (the liquidation is 17,114 bytes now, compute 27,684; the swap as it was).

## Findings

**Nothing above Low is new. Both fixes hold.** Two Low, one of them the hole the coordinator named, and two notes.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C10-F1 | Low | (The hole named in the addendum, as the tree is.) A side owed nothing is not liquidated while the pot is under the maintenance, and the party owed all of it does not close. Left alone the account lapses and pays the bankrupt side its margin. The winner's way out today: add what the pot lacks (it comes back) and liquidate, itself, since the fee is 0 | `c10_bounded_a_side_owed_nothing_is_not_liquidated_while_the_pot_is_under_the_maintenance` |
| C10-F2 | Low | What is left of C9-F1. Under two maintenances nobody is liquidated; the party that first adds its shortfall is at its maintenance, the other is not, and is liquidated at once (a liquidation asks no quiet time; the other's close does, and the addition restarted it). 1,500 KAS that lost nothing are forfeited unless 2,700 are added within the grace. The planned close does not change this | `c10_bounded_under_two_maintenances_the_first_to_add_its_shortfall_liquidates_the_other` |
| C10-N1 | Note | With a fee capped by what the loser is owed, a liquidation of a side owed nothing pays no proposer (101 of the 123 liquidations sampled at the edges): only the winner has a reason to send it | `c10_holds_a_liquidation_needs_a_healthy_winner_and_its_fee_is_the_losers_money` |
| C10-N2 | Note | The planned close is not "refused exactly where the closer is liquidatable": it is also refused in the band of the tolerance (the closer at its maintenance at the prices toward its gain, below it at the prices against it, the other party healthy), as a close always was. And a side closes and is liquidated at one tick only where the maintenance against it rounds to 0 sompi (C9-N2) | `c10_bounded_the_planned_close_against_the_liquidation_on_the_model` |

## The fixes of review C9

| C9 finding | Now | Test |
|---|---|---|
| C9-F1 (Low) | closed as found: over 250 accounts at the edges and 750 ticks the engine accepts exactly the liquidations the model writes, in each the other party is at its maintenance at the trigger's prices, and never two at one tick. What it leaves: C10-F1, C10-F2 | `c10_holds_a_liquidation_needs_a_healthy_winner_and_its_fee_is_the_losers_money`; the coordinator's `c9_fixed_a_pot_smaller_than_two_maintenances_liquidates_nobody` |
| C9-F2 (Low) | closed: the fee is the model's `min(fee, owed)`, never above what the liquidated side is owed at the attested prices; the winner is paid at least what it is owed there and the carrier; a side owed nothing pays no proposer | the same test |
| C9-F3 (Low) | not run this round (the commands' new words); by the coordinator's account | |

## The questions

**Does "the other side has its maintenance" open anything?**

- *An account that is never liquidated though a side is far under water.* Yes, while the pot is under the
  maintenance of the notional at the trigger's prices (C10-F1): one leg long 1,000 units from 10, 200 KAS of margin
  each, the price at 9. A is owed 0, B 400; the maintenance at the prices moved toward A is 459. The liquidation of
  A and B's close are both refused. The lapse pays A 200 KAS.
- *Can a party arrange it to its gain?* Not by its own margin. A side is liquidated when it is below the maintenance
  and the other is not; the other's worth is the pot less its own, so a party makes itself unliquidatable only by
  the pot being under two maintenances, and it cannot lower the other's margin. What it can do is not add margin
  while the notional grows: then the hole is there without anybody arranging it, and the loser's best move is to
  wait for the lapse.
- *Can one party hold the other in it?* Not a party with the money to add: with 59 KAS more in the pot A is
  liquidated (58 is one short), B takes the whole account, and its addition with it. Anyone renews the lease, so
  the loser cannot force the lapse either. A winner without the money is held until it has it.
- *A side that is bankrupt keeps the other from being paid?* Only in that sense: until the winner tops the pot up.

**The fee cap: can the fee still come out of the winner's money?** No. `amount <= clampPot(raw at the attested
prices)` for the liquidated side; the pending claim pays it first and the winner `v - amount`, which is at least the
pot less what the loser is owed, and the carrier. Where the loser is owed nothing the proposer is paid nothing. A
UTXO that holds less than its pot pays what it holds, the fee first: that is review C9's C9-N4, unchanged.

**Did the two changes break anything else?** Nothing I found: the engine and the model agree on every liquidation of
the sample, the cure and the close are as they were (`auditC9_tests` passes with both).

## The planned close, judged on paper and on the model

`close by A: vaG >= mG || pot - vaF < mF`, and the mirror for B.

- **Is the complement exact?** No, and it need not be (C10-N2). A's close is refused where `vaG < mG` and
  `pot - vaF >= mF`. A's liquidation is `vaF < mF`, `pot - vaF >= mF`, `rawG < mG`. The refusal is wider by the band
  `vaF >= mF` and `vaG < mG` with the other party healthy: the tolerance, where a side neither closes nor is
  liquidated and the other party may close. It is narrower nowhere but where `mG` is 0 sompi.
- **Can a side choose between being liquidated and closing at one tick?** No. Its liquidation needs the other party
  at its maintenance at the trigger's prices, the second disjunct of its close needs the opposite at the same
  prices, and the first disjunct (`vaG >= mG`) excludes `rawG < mG` unless `mG` is 0. On the model, over 2,000
  accounts and 6,000 ticks: 36 side-ticks where both hold, each with a maintenance of 0 sompi against the side.
- **No account is stuck.** A's refusal needs `pot - vaF >= mF` and B's needs `pot - vaF < mF`: one of the two always
  closes. On the model: 1,582 of 6,000 ticks at which no claim of either kind is proposable today, 0 with the plan.
- **What it opens.**
  1. A loser below the maintenance leaves with its side whenever the winner is below its own too, where the
     winner, once topped up, would have taken all of it. The winner loses that windfall, never what it is owed: it
     is paid the rest, which is its side at the attested prices and the tolerance on top.
  2. Closing there is not free: the closer is paid its side at the prices moved by g against it on every leg. In
     the test's account (two legs of 30,000 KAS, g 2 %) A's close pays it 300 of its 1,500 KAS; with g 0.5 %, 1,200.
     When both are below the maintenance each would rather the other closed.
  3. It does not close C10-F2. A close asks the quiet time and a liquidation does not; the party that adds its
     shortfall respends the account (the other's quiet time starts again), is healthy, and liquidates at once. The
     plan gives the passive party an exit only if it takes it first. If that is to be closed too: a liquidation of
     a side within `quietDaa` of an `addMargin` by the other party could be refused, which would break nothing but
     the speed of a keeper after a top-up.
  4. Nothing for a stale tick beyond what every close has within `lateDaa`.

## Not checked

- The planned rule in the engine (it is not built); the commands' new words (C9-F3); `x-swap` and `x-open` with the
  terms of two markets (`terms_of`); the live run of `docs/LIVE-3.md`.

## Judgement

Both fixes hold and nothing above Low is new. The first fix trades a race for a hole, and the coordinator saw the
hole before I did: a pot under the maintenance leaves a bankrupt side unliquidated and the winner unable to close.
The planned close fills it soundly: one party can always leave, no side both closes and is liquidated at a tick,
and the winner is never paid less than it is owed. What neither the fix nor the plan removes is that the party
which adds margin first liquidates the one which does not, however little either has lost.
