# KPF review C9 (the cross-margin account: KpfCrossC, KpfPendingX, and two tickets side by side)

## Scope and method

Target: `contracts/KpfCrossC.sil` (`propose`, `addMargin`, `keepAlive`, `lapseSettle`, `coopClose`, the arithmetic
`moved`, `gain`, `maint`, `clampPot`), `contracts/KpfPendingX.sil` (`cure`, `finalize`), the change to
`contracts/KpfTicket.sil` (`attest`: as many outputs of the roster's id as inputs), the model
`crates/kpf-protocol/src/cross.rs`, and through the binary `x-swap`, `x-show`, `x-liquidate`, `x-cure`,
`x-finalize`, `x-close`; `docs/DESIGN-COMMITTEE.md` 3b (N§3b) is the specification.

Files of this round:

- `crates/kpf-tests/tests/auditC9_tests.rs`: 5 tests on the engine (3 `c9_holds_`, 2 `c9_bounded_`); the helpers are
  those of `cross_tests.rs`.
- `crates/kpf-cli/tests/auditC9_cli.rs`: 1 test (`c9_bounded_`) through the binary.
- No `auditC9_builders.rs`: the model was driven against the engine in `auditC9_tests`, and the builders through
  the binary.
- Removed: my `crates/kpf-cli/tests/auditC8_cli.rs`. It was never run (review C8), its command was renamed
  (`c-margin-run`), and what it was to show is now the family's own test of the runner.

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`):

- `kpf-tests`: `auditC9_tests` `ok. 5 passed`; `auditC1_tests` `ok. 22 passed` (with the coordinator's edit of the
  two-tickets test).
- `kpf-cli`: `auditC9_cli` `ok. 1 passed`.

## Findings

**Nothing above Low is new. Three Low, five notes.** The engine and the model agree on every claim I could throw at
them, no claim pays more than its UTXO holds, and the ticket's change mints nothing.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C9-F1 | Low | The maintenance follows the notional at the present prices and the pot is fixed: an account that lost nothing is liquidated when its markets have risen. With two legs that offset, both sides fall below it at one tick; the first claim takes the whole account, and the other party has `graceDaa` to add what cures it | `c9_bounded_both_sides_of_an_account_are_liquidated_at_one_tick_and_the_first_claim_takes_it_all` |
| C9-F2 | Low | The fee is capped by the maintenance, not by what the liquidated side is still owed: where that is less (316 of the liquidations sampled, a side owed nothing among them) the rest of the fee comes out of the winner's. A side with nothing left has itself liquidated by a key of its own and takes the fee; the winner avoids it only by claiming first | `c9_holds_the_engine_and_the_model_agree_on_accounts_at_their_edges` (the count); by reading |
| C9-F3 | Low (by reading) | A leg names its market's id and its roster's id side by side and nothing ties the two: an account can name a known market under a roster of its maker's, and is then priced by that roster. `x-show` prints eight bytes of the market's id and no roster. The same for `gBps` under four spreads of the leg's roster: such an account takes no ticket, so no claim, and ends by both signing or at the lapse (the entries) | by reading; `auditC9_tests` (the last lines of the first test: g 199 against a spread of 50) |
| C9-N1 | Note | `add > 0` does not follow from the trigger (128 of 3,000 sampled claims met the trigger with no cure to pay). The `require` is not a formality: it is what keeps a side that is owed the whole of a pot smaller than the maintenance from being liquidated while its own worth is above it | the same test |
| C9-N2 | Note | A side that is liquidated may also close at the same tick where the maintenance at the prices moved against it rounds to 0 sompi (prices of a sompi or two; 5 of the sample). Nothing of worth is at stake there | the same test |
| C9-N3 | Note | Masses: `x-swap` 27,557 bytes (55,114 grams of transient mass: over half of a standard transaction's 100,000; compute 36,627); `x-liquidate` and `x-close` 16,924 bytes, compute 27,494 and 27,894, storage 80,724 (the two tickets of 1 KAS and the claim); `x-cure` 4,557; `x-finalize` 3,858 and 8,518 of storage. None near the block's 500,000. A swap of two pairs has room for no third | `auditC9_cli::c9_bounded_the_masses_of_the_swap_the_claims_and_their_settlements` |
| C9-N4 | Note | What a UTXO holds beyond its pot and carrier goes to the party that did not claim (a close), to the winner (a liquidation), to B (the lapse), and stays in the account through a cure and a restore. A UTXO that holds less pays what it holds, the closer first | `c9_bounded_what_an_account_holds_beyond_its_pot_goes_to_the_party_that_did_not_claim` |
| C9-N5 | Note | `x-swap` is one process holding the owner's key and the counterparty's funding. Two parties need the transaction passed between them, and the owner's check of what it signs is then the builder run again on its side: nothing else shows it the account's bytes | by reading; `auditC9_cli` (what the command prints) |

## C9-F1 (Low): an account that lost nothing is liquidated, and both sides can be at once

`M = ceil(N mmr / 10^4)` with `N` the notional at the tick's prices; `V_A` and `V_B` sum to the pot. An account
needs `V >= M` on each side, so a pot under `2 M` has a side below it whatever happened, and `M` grows with prices.

The test: A long 1,000 units of one market and short 1,000 of another at 10, margins of 1,500 KAS each (15 % of
20,000), `g` 2 %. A is owed 1,500 at every pair of equal prices. At 10, 20 and 24 neither side is liquidated. At 30
the notional is 60,000 and its maintenance 3,000, the pot: both sides are liquidated at that tick. Either claim is
accepted; settled after the grace it pays 2,700.2 KAS to the other party and 300 KAS to the proposer; the liquidated
side's cure is 2,700 KAS within 50 DAA. Neither may close there. On the testnet defaults (`g` 0.5 %, the grace 10
s) an owner with 6 % of the gross notional as margin and offsetting legs is liquidated after both markets rise by a
third (0.06 N0 + g N < 0.05 N).

This is the rule as N§3b states it (the maintenance on the notional; "the loser of a liquidation forfeits the whole
account less the fee"), and the engine keeps it to the sompi. What N§3b does not say is that the account it was
built for, two positions that offset, is the one whose owner loses nothing and is liquidated all the same, and that
the counterparty, an executor that is online, is the party placed to claim first. Either say it there with these
numbers, or count legs of one market (and of markets the two parties agree to net) net in `N`.

## C9-F2 (Low): the fee is paid by whoever has the money

`amount = max(keeperFeeMin, floor(N liqFeeBps / 10^4))`, at most the maintenance at the prices moved toward the
side's gain; `KpfPendingX.finalize` pays it first and the winner the rest. A side owed 0 at the tick has less than
any fee: the winner pays. So a party that has lost everything sends the liquidation of its own side from another
key and is paid half a percent of the notional (the default) from what the other party won. The winner's answer is
to be the proposer itself. A cap of `min(maintenance, what the side is owed at the prices moved toward its gain)`,
with `keeperFeeMin` as the floor a keeper needs, would bound what the winner can be made to pay.

## C9-F3 (Low, by reading): nothing ties a leg's roster to its market

`ticked(u, t0, market, roster, a, priceMax)` wants a ticket of `roster` attesting `market`. A market's id is a hash
of its block, and a roster signs ticks for any 32 bytes: a ticket of roster R' attests "market M" as readily as M's
own roster does. In a pair the market block carries its roster; in an account the two ids stand side by side.
"Two parties, by consent" covers it only if the owner reads the roster id it signs. `x-show` and `inspect` print
`market 08534f3ad4b51fc7` and no roster, and compare neither with a market file. What a wallet must check of an
account it is handed, and the tool should check for it: each leg's roster is the roster of the market file whose id
the leg names; `gBps` is at least four times that roster's spread (below it the account refuses every ticket: the
engine does, the model's `pending_of` does not know); the UTXO holds the pot and the carrier; its own key is `aKey`.

## The questions put to this round

- **Can a claim pay more than the UTXO holds, or less than the rule?** No. 250 accounts at the edges (sizes of one
  unit and of 250,000 whole units, entries at 0 and at the price limit, a limit under the prices, margins from 0 to
  `INT_MAX / 8`, mmr 0, `g` from four spreads to 9,999, a fee of nothing and of all, one leg, one market twice, one
  direction twice), three ticks each, both claims for both sides: 3,000 claims, the engine accepted exactly the
  1,287 the model writes, each settled as the model says, the payouts summing to the UTXO's value; each liquidation
  was cured with the model's addition (a sompi less refused), and no cured side is liquidated at the same prices.
- **Is `add > 0` always true when the trigger is?** No (C9-N1), and it should not be.
- **Fair in both directions?** The rule is symmetric and the model's two sides mirror each other. A side liquidated
  at a tick is below the maintenance at the attested prices themselves in all but 21 of the sampled liquidations,
  and in those the pot is smaller than the maintenance (the side is owed all of it). A close and a liquidation of
  one side at one tick: only at C9-N2's edge. A's close and B's liquidation at one tick: both proposable; the
  liquidation pays A more, and whichever lands first is what happens.
- **The pending claim.** Two claims settled in one transaction pay each its own outputs; the payouts swapped, one B
  output for both, and a mirror index among the inputs' own are refused. Cure and finalize beyond that: the
  family's `cross_tests` (read, not repeated).
- **The ticket's change.** Sound. Each attest pins its own output (the same script, the roster's id, its index),
  and the count is equal: as many outputs of the id as attesting tickets, each one's own. Beside a retire, beside
  the roster, with a made-up output to make the count, or with another ticket in its own place: refused. Every
  entry of `KpfRoster` (`join`, `topUp`, `leave`, `forfeit`, `charge`, `slash`, `clear`, `roll`) and of `KpfExit`
  (`withdraw`, `forfeit`, `charge`, `slash`, `clear`) requires to be the only input of the id (by reading), and
  `retire` too. The coordinator's edit of `c1_holds_no_two_objects_of_the_roster_id_...` does not weaken it: the two
  attests are accepted, a third output, a merge and a continuation at another index are refused.
- **`x-swap`.** One transaction, the owner's signature over all of it: it is not left without its pairs and without
  an account. The default a-margin is the worth of the sides given up at the agreed prices (20 and 15 KAS: 35), the
  legs enter at those prices, so the owner is owed after the swap what it had before. The counterparty's bounds are
  no nearer than the owner's were (8.55 for 9, 12.6 for 12: healthy at the leaver's own bound), so X and Y hold
  wider ranges than before and lose nothing. The agreed prices are the two parties' word, not a ticket's.
- **A leg attested for another tick**: "REFUSED: leg 1 is attested for the tick 33600 and leg 2 for 33590: an
  account is priced for one tick".

## Not checked

- `x-swap` built by a counterparty that means harm (C9-N5: there is no two-party flow to attack yet); `x-open`.
- Signature operations per input script and the scripts' sizes (the transactions pass the builder's `verify`).
- The same ticket named for both legs (`u2 != u1`), tickets of two rosters, a second ticket beside a one-leg
  account, the staleness rule: by reading only. `addMargin`, `keepAlive`, `lapseSettle` beyond the family's tests.
- A pending claim whose `account` bytes lie: by reading, `x-finalize`, `x-cure` and `inspect` refuse one that is
  not `consistent`.
- Sums beyond the model's `MARGIN_MAX` (the contract refuses them in `checkCross`).

## Judgement

The account's contracts do what N§3b says, to the sompi, at every edge I found, and the ticket's change is the
smallest that lets two markets be priced for one tick. Nothing above Low is new. The three Low are about what the
rule means for the party it was built for: an owner with two positions that offset is liquidated by a rise of both,
the fee of a liquidation can be the winner's money, and an account names its rosters where no tool shows them.
