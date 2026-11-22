# KPF review C1 (the committee family, first round)

## Scope and method

Target: the committee family as it stands in the working tree on 2026-11-21 (untracked, never reviewed):
`contracts/KpfRoster.sil`, `KpfTicket.sil`, `KpfExit.sil`, `KpfPairC.sil`, `KpfPendingC.sil`, `KpfOpenC.sil`, their library
models (`crates/kpf-protocol/src/committee.rs`, `cmarket.rs`) and the kit (`crates/kpf-tests/tests/committeekit`), read
against `docs/DESIGN-COMMITTEE.md` (cited as "the note", `N§n`) and the twelve reviews of the oracle-market family the
contracts were derived from. `ccmds.rs` was read in two places only (the order's defaults, `committee-check`). Every
template is compiled from source by the kit; no pinned artifact was used. The tree moved during the review: the six
contracts did not change, the note gained N§5a (stage 2 as designed; read at the end, it changes one sentence of
C1-F1), and builders, pinned artifacts, commands and `LIVE-3.md` appeared that this round did not read.

New files only: `crates/kpf-tests/tests/auditC1_tests.rs` (21 tests: 4 `c1_exploit_`, 11 `c1_bounded_`, 6 `c1_holds_`),
`crates/kpf-tests/tests/auditC1_walk_tests.rs` (two seeded random walks, both `c1_holds_`) and this report. Nothing in
`contracts/`, `crates/kpf-protocol/`, `crates/kpf-cli/`, `vendor/`, the kits or the existing tests was changed. Runs:

- `CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test auditC1_tests --test auditC1_walk_tests`:
  `test result: ok. 21 passed; 0 failed` (1.09 s) and `test result: ok. 2 passed; 0 failed` (5.91 s, seeds 1 to 6).
- The same walks with `C1_SEEDS=7..60 C1_STEPS=300`: `ok. 2 passed` (106.09 s). Roster walk: 4,144 joins (2,471 of them
  outbidding a seat), 996 top-ups, 822 leaves, 2,875 rolls, 2,187 withdrawals, 2,517 retirements, 929 attestations
  accepted; 15,346 hostile or ill-timed variants refused, each as the model predicts. Market walk: 1,285 amends, 1,515
  margin top-ups, 765 assigns, 955 renewals, 307 lapse settles, 1,647 closes and 527 liquidations proposed (452 with a
  stale tick), 175 cures, 1,218 closes settled, 46 forfeits, 689 restores; 6,958 variants refused as predicted. No
  violation.
- `--test roster_tests --test committee_tests --test committee_paths_tests --test c_experiments_tests`: 13, 8, 8 and 4
  passed (the family's own tests, unchanged).

Numbers use the kit's fixture unless stated: a roster of 100 KAS in 16 seats (30, 22, 15, 12, 8, 2, 2 and nine of 1), a
quorum of 67 %, epochs of 1,000 DAA, exits of 500, seats of at least 10 KAS; a market with a lease of 8,000 DAA, a lapse
1,000 after it, 50 DAA of grace, `lateDaa` 100, 20 DAA of quiet, mmr 5 %, g 2 %, a liquidation fee of 0.5 %, a carrier
of 0.2 KAS; the worked pair of 1,000 units at 10 KAS, the long's bound at 9 (1,000 KAS of margin), the short's at 12
(2,000 KAS). Fees are the relay floor at 100 sompi a gram (`kpf_protocol::tx::MIN_FEE_RATE`) with each input's compute
budget committed as the tool's builder commits it; they are estimates, not node measurements.

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C1-F1 | High | Half a quorum, not a quorum, dictates the price: the claimant chooses which signed seats an attestation includes, so seats holding `quorumBps / 2` of the weight (33.5 %) add just enough honest signatures (which are public) to reach the quorum and stay the majority of the included weight; their price is then the weighted median. On the fixture 34 KAS of 100 attest 12.30 and 9.28 at a feed of 10.00: a false close pays the long the whole range (3,000 KAS, the short's 2,000 KAS gone), a false liquidation forfeits the long's 1,000 KAS. Five minimum stakes bought into the fixture roster (50 KAS, returned whole after the exit time) are enough. The note says a price needs 67 % and that a third can only stop a market | `c1_exploit_half_a_quorum_dictates_the_price_with_the_honest_seats_own_signatures`, `c1_exploit_six_of_sixteen_equal_seats_attest_any_price`, `c1_bounded_five_minimum_stakes_bought_into_the_fixture_roster_dictate_the_next_epoch`; the bound: `c1_holds_a_third_of_the_weight_cannot_move_the_median_outside_the_honest_prices` |
| C1-F2 | High | `KpfOpenC.take` needs no pair and reads no clock, and unlike `fill` does not ask the counter side to be healthy at the price. Anyone enters a resting order into a pair it writes alone: (a) against a counterparty with no margin (the tool's default `min-counter-rate` is 0), which the true price liquidates at once: the taker proposes it and is paid the fee out of the order's margin (60 KAS of a 1,010.2 KAS order: the 10 KAS tip and a 50 KAS fee, for no capital); (b) with a lease already over: the taker settles at the entry when the price moves for the order and keeps or liquidates the position when it moves against it (tip and carrier, 10.2 KAS, in any case); (c) with a lease beyond every clock: no lapse settle can ever be built for that pair | `c1_exploit_a_take_without_a_pair_against_no_margin_pays_the_taker_a_liquidation_fee_from_the_orders_margin`, `c1_exploit_a_take_without_a_pair_writes_a_lease_that_is_over_or_never_ends` |
| C1-F3 | Medium | A liquidation proposed with a tick more than `lateDaa` old is void, costs its proposer nothing on a whole pair, takes the pair off the market for `graceDaa`, clears both listings, and can be proposed again on the restored UTXO in the next block (no quiet time, no bound of the tick against the pair). A loser due for liquidation chains 171 of them (8,550 DAA) up to the lapse date and is settled at the entry: it keeps 1,000 KAS instead of 0, the short gets 2,000.2 instead of 2,954.2 KAS. One stale tick of a dip freezes and unlists a healthy pair for as long as the tick's ticket is not retired (nobody can retire it for an epoch after its window) | `c1_bounded_a_loser_chains_void_liquidations_of_itself_up_to_the_lapse`, `c1_bounded_one_stale_tick_freezes_and_unlists_a_healthy_pair_for_as_long_as_its_ticket_lives` |
| C1-F4 | Medium | A price above the market's `priceMax` cannot be used: `propose` refuses it and no signature attests a lower one. A short that is bankrupt above `priceMax` is never liquidated and the long cannot close; the pair is renewed for ever or lapses at its entry (the long gets 1,000 KAS for a side worth 3,000) | `c1_bounded_a_feed_above_priceMax_cannot_be_claimed` |
| C1-F5 | Medium | The roster and each ticket are single UTXOs anyone may respend: a top-up of one sompi (0.0085 KAS of fee) invalidates a pending roll, leave or join; an attestation by anyone with public signatures (0.011 to 0.019 KAS) invalidates every claim built on that ticket. Held for every block that is about 7,300 KAS a day for the roster (no roll, so no price from the next epoch on) and 224 KAS for the four tickets of one 5-minute epoch on the testnet defaults. The note says stopping a market takes a third of the weight | `c1_bounded_anyone_respends_the_roster_and_every_ticket_for_a_fee`; the race in a mempool by reading |
| C1-F6 | Low | Review 12's F3 is inherited: every party entry (`assign`, the close, `transfer`, `amend`) needs the pair UTXO to be `quietDaa` old and any party entry restarts that age, so the counterparty's amend that changes nothing races every exit at the boundary (1,156 KAS a day at 20 DAA of quiet). `assign` is not "at once" against a counterparty that objects | `c1_bounded_the_counterparty_restarts_the_quiet_time_of_an_assign_and_of_a_close` |
| C1-F7 | Low | Review 10's F2 is inherited in a milder form: a pending part cannot be renewed, so a cure (or a void claim's restore) that lands after `expiry + idleDaa` is settled at the entry in the next block: the liquidated long gets its cure and its whole margin back, the short loses the liquidation (953.6 KAS). Only a winner that did not renew in the three quarters of a term before is exposed (N§4: "or nobody renewed") | `c1_bounded_a_cure_landing_after_the_lapse_date_is_settled_at_the_entry` |
| C1-N1 | Note | The bounds of `checkMarket` and `checkParams` admit markets that cannot work, and `committee-check` prints the parameters without a warning: `lateDaa` 0 (a transaction with lock time t0 lands after t0, so no claim is ever valid); a term below 4 DAA (one tick renews a pair in every block) or no idle time; a quorum of 5,001 (a quarter of the weight dictates, by F1); `lateDaa` above the roster's `exitDaa` (a stake is released while its ticks still settle claims); a keeper fee floor above the carrier (liquidating in minimum parts pays ten times the fee rate); an unbounded carrier; epochs of 1 DAA; tickets of 1,000 KAS | `c1_bounded_the_market_bounds_admit_markets_that_cannot_work`, `c1_bounded_a_keeper_fee_floor_above_the_carrier_pays_for_liquidating_in_the_smallest_parts`; the tool by reading (`ccmds.rs` 500-515) |
| C1-N2 | Note | A roll is not "one transaction an epoch": the roller funds `tickets x ticketValue` (0.8 KAS an epoch, 230 KAS a day on the testnet defaults) and anyone who retires a ticket two epochs later keeps its value. `committee-check` proves the lineage, not who sits in it: an empty roster is filled by its first comer for `16 x minSeat` | `c1_holds_stake_leaves_the_roster_only_as_the_exit_of_its_own_seat` (the roller pays), by reading |
| C1-N3 | Note | N§4 has no row for an order. `KpfOpenC`'s remark "so the order gets at least its margin back" is false (F2); the residual of `fill` (an order older than a term and the idle time is filled into a lapsed pair: its tip and one carrier) is real and unstated | `c1_bounded_an_order_older_than_a_term_and_the_idle_time_is_filled_into_a_lapsed_pair` |

No Critical finding. F1 is not rated Critical because it needs a party holding half a quorum of the weight; it is not
"a quorum that lies" either, and N§7 admits neither the threshold nor the damage.

### C1-F1 (High). Half a quorum dictates the price

**Affected:** `KpfTicket.attest` (`w >= quorum`, `2 * lo >= w`, `2 * hi >= w` over the seats in `bits`, which the
claimant chooses), N§0 ("a price needs the signatures of at least 67 % of the bonded weight"), N§1 ("may move the median
within the honest seats' spread"), N§7 ("A quorum that lies takes every pair", "A third of the weight can stop every
market ... a stopped market is safe").

**The rule that is missing.** The median is taken over the included seats, not over the roster. Let the colluding
weight be `a`, the quorum `q`. The claimant includes all of `a` and honest signatures of weight `h` with
`q - a <= h <= a`: the set reaches the quorum and the colluders hold at least half of it, so their price `X` satisfies
both median conditions (below the truth: `lo = a`, `hi = a + h`; above it, mirrored). Such an `h` exists as soon as
`a >= q / 2` and some set of honest seats has a weight in `[q - a, a]`; a colluder tunes its own weight by a top-up.
The honest signatures are the ones every honest seat publishes for every tick (N§7: "Seats publish ticks off chain").
So the family is safe against less than 33.5 % of the weight and no more; the same third that stops a market takes it.
At the lowest quorum `checkParams` accepts (5,001) the threshold is 25 %.

**The attack as transactions (`c1_exploit_half_a_quorum_...`).** Seats 1 and 3 (22 + 12 = 34 KAS of 100) collude; the
honest seats signed 10.00 for the tick 6,500.

- A false close. Inputs: the pair (`propose` kind 1, long, `p` = 12.30, the long's signature), the ticket (`attest`:
  bits = seats 0, 1, 3, 5, 7; prices 10.00 for 0, 5, 7 and 12.30 for 1, 3; `p` = 12.30), funding. `lo` = 67, `hi` = 34,
  `w` = 67: accepted. `finalize` in the next block (a close has no grace and no cure): the long 3,000 KAS, the short
  0.2 KAS. The short's side was worth 2,000 KAS at the feed.
- A false liquidation of the long at 9.28 by the short (the proposer): after `graceDaa` the short holds both sides of
  the forfeited part (2,953.8 KAS) and the fee (46.4 KAS); the long's side was worth 1,000 KAS and it was 7 % above its
  trigger.
- In the engine on 16 equal seats: 6 lying seats and 5 honest signatures attest 1 sompi, 5 KAS, 50 KAS and
  999,999.99999999 KAS; 5 lying seats cannot.

**What it costs (`c1_bounded_five_minimum_stakes_...`).** Five of the fixture's 1-KAS seats are outbid with the minimum
stake: 50 KAS of 145 (34.5 %). After the next roll the newcomer's five seats and 48 KAS of honest signatures (seats 0,
2, 5, 12) attest a price of one sompi. Nothing is slashed in stage 1; the newcomer leaves and withdraws its 50 KAS at
`from + E + U`. In general `a >= 0.504 h` for the honest weight `h` that remains: half the honest stake, lent for two
hours and five minutes on the testnet defaults, against every pair of every market of the roster. Stage 2 as designed
(N§5a) can charge each lying seat for the price it signed (the double-signing rule does not apply: each seat signs one
price), so the liars are slashable once it is built; what F1 changes then is the ratio N§7 rests on: the stake behind
a false price is half a quorum's, not a quorum's, and it must be charged within `exitDaa`.

**The smallest fix.** `attest` also requires every included price to lie within a tolerance of `p` (a seventh
fixed-width argument `tolBps`, read by `KpfPairC.ticked` and required to be at most `gBps`; or a field of the ticket). A
false price then needs a quorum of weight signed near it, which honest seats never give: the threshold becomes 67 %
as the note says, and a third still only stops. What it could break: the layout `ticked` reads (one more push, every
offset after it, the pins), a few hundred script units per seat, and liveness when honest seats disagree by more than
the tolerance (the claimant must then find 67 % that agree; today it needs 67 % that signed anything).

### C1-F2 (High). An order is taken into a pair nobody handed over

**Affected:** `KpfOpenC.take` (checks its own side's health at `x`, `e` in its favour, `bound` against
`minCounterRate`; `xe` only `0 <= xe < 5 x 10^11`; `pm` unchecked; no input is required to be a pair), against
`KpfOpenC.fill` (both sides healthy at `x`, `own DAA + termDaa <= xe <= lock time + termDaa`); `ccmds.rs` 540
(`min-counter-rate` defaults to 0); `KpfOpenC`'s DESIGN remark on `take`; N§3 ("the new pair's lease is `xe`, at least a
term after the order was accepted ... and at most a term from now": true of `fill` only).

**(a) No margin on the other side (`c1_exploit_..._pays_the_taker_a_liquidation_fee_...`).** The order: long, 1,000
units, limit 10 KAS, 1 KAS of margin a unit, tip 0.01 KAS a unit, `minCounterRate` 0; escrow 1,011.2 KAS.

1. Inputs: the order (`take(partOut 1, n 1,000, x 10, e 10, bound 10, counterKey = taker, pm 0, xe 14,200)`), the
   taker's key. Outputs: the order's refund 1 KAS, the part (fresh id; long = the order, `bl` 9, `bs` 10, value 1,000.2
   KAS). The order paid 1,010.2 KAS; the taker funded nothing and keeps the 10 KAS tip.
2. The short has no margin, so the feed itself (10.00, a true tick) liquidates it: `propose` kind 2 by the taker.
3. `finalize` after the grace: fee 50 KAS to the taker, the forfeited part (950.2 KAS, both keys the order's owner).

The owner signed nothing after placing the order, holds no position and has lost 60 KAS; the taker risked nothing.
The mirror on a short order is asserted. It works when the order's own margin is wide enough for the range rule to
admit a counter bound at the price (`2 mmr / (1 + mmr)`, 9.5 % of the price at mmr 5 %: 10x or less); the loss is the fee rate of
the market (up to the whole maintenance slice, 476 KAS here, at `liqFeeBps` 10,000) and the tip. An order with
`minCounterRate` above the maintenance band is refused to the taker (asserted).

**(b) A lease that is over.** The same take with `bound` 12 (the taker posts 1,990 KAS) and `xe` 0. The part is due for
`lapseSettle` from its first block. Feed up: the taker settles at the entry (the order gets its 1,000 KAS margin for a
side worth 2,000 at 11; the taker keeps the tip and the order's carrier, 10.2 KAS). Feed down to 9.28: the taker
liquidates the order's side with a true tick (accepted from the same UTXO). The order's owner can end it by settling
or renewing first, if it watches; a resting order's owner is by definition not there. `fill` refuses the same lease.

**(c) A lease that never ends.** `xe` = 499,999,999,999 is accepted; `lapseSettle` requires `expiry + idleDaa` to be a
lock time and is refused for ever; `keepAlive` cannot move it. With `xe` 500,000 DAA lower the lapse is due in 1,585
years. If the committee stops, this pair has no exit that needs nobody, which is the one thing the lease exists for.

**The smallest fix.** In `take`: the counter side healthy at `x` (the line `fill` has for it), and the lease bounded
as far as an order can prove: `OpTxInputDaaScore(self) >= 0`, `xe >= own DAA + termDaa / 2`, `tx.daa >= xe - termDaa`.
What it could break: a listed side whose stayer is below maintenance at `x` cannot be taken (it is due for
liquidation); a pair whose lease has less than half a term left past the order's acceptance must be kept alive first
(always possible then: it is inside the renewal window). The residual is `fill`'s (C1-N3), shifted by half a term.

### C1-F3 (Medium). Void liquidations are free and chain

**Affected:** `KpfPairC.propose` kind 2 (no `quiet()`, `t0` bounded only by the ticket's window; `refund` 0 for a whole
pair; every return state unlisted), `KpfPendingC.finalize` (`graceDaa` before the restore of a void claim too),
`KpfTicket.retire` (nobody before `until + E`), N§3 ("a void claim ... frozen ... for at most graceDaa" in
`KpfPendingC`'s remark: per claim, not in all).

**The loser (`c1_bounded_a_loser_chains_...`).** The feed falls to 9.20 at 6,300 and stays; no keeper lands a claim
within `lateDaa`. From 6,450 the long proposes a liquidation of itself with a tick 101 DAA old (the ticket that covers
it cannot have been retired: asserted in every round), lets it rest `graceDaa`, finalizes (restore, the whole value)
and proposes the next on the restored UTXO. 171 rounds, no value spent, to 15,000; then `lapseSettle`: the long 1,000
KAS, the short 2,000.2 KAS. A liquidation at 9.20 would have paid the long nothing and the short 2,954.2 KAS. While
the part is pending nobody can liquidate it, close it or renew its lease. What stops the chain is a keeper winning
one of the races for the pending UTXO at a restore (its valid claim is accepted from the same UTXO: asserted); the
loser publishes its finalize and its next claim together, so only the finalize is contested. Not measured on a node.

**The bystander (`c1_bounded_one_stale_tick_...`).** A dip to 9.28 at the tick 6,300 that nobody claimed in time.
Anyone holding that signed tick proposes void liquidations of the healthy pair from 6,450: four rounds asserted; after
each restore both listings are gone and the parties' own entries (amend, assign) are refused for 20 DAA, the next
void claim is not. It ends when someone retires the ticket (from 8,000; if nobody does, never).

**The smallest fix.** (1) `finalize` restores a void liquidation without the grace (only a valid one needs the time to
cure): a void claim then holds a pair for a block. (2) `propose` refuses a tick more than `lateDaa` older than the pair
UTXO it spends (`OpTxInputDaaScore(self) >= 0`, `t0 + lateDaa >= own DAA`): a claim on such a tick is void whenever
it lands, and one stale tick then freezes a pair once, not again. (3) The restore state keeps the listings (the pending
part already carries them). What they could break: (2) forbids chaining a claim on a pair UTXO that is not accepted
yet; (3) a listing survives a claim that was void. The race at each restore stays (two spends of one UTXO).

### C1-F4 (Medium). A price above priceMax has no claim

**Affected:** `KpfPairC.propose` (`p <= priceMax`), `KpfTicket.attest` (a median of signed prices only), `checkMarket`
(`priceMax` is the creator's; the testnet default is the cap, 1,000,000 KAS a unit). In the oracle family the party
that stated the price could state `priceMax`; here nobody states a price.

**The scenario.** `priceMax` 20 KAS, the worked pair, the feed at 25. The ticket attests 25; the liquidation of the
short (bankrupt from 12) and the long's close are refused; the same signatures do not attest 20. `keepAlive` works (it
reads no price), so the long either renews for ever or lets the pair lapse at 10: 1,000 KAS for a side worth 3,000.
Prices are the underlying in KAS, so a fall of KAS moves every feed up together.

**The smallest fix.** `propose` takes the attested price and claims `min(p, priceMax)`. What it could break: nothing
found; a short's cure above `priceMax` is already absent.

### C1-F5 (Medium). The shared UTXOs are anyone's to respend

**Affected:** `KpfRoster.topUp` (`amount > 0`, no signature, no age), `join`; `KpfTicket.attest` (anyone, no cost to
the ticket); N§2 ("the number of tickets is the number of claims that can use one epoch in parallel"), N§7.

**Measured.** Twelve top-ups of one sompi in a row are accepted (12 sompi of stake given to seat 7); the roll built on
the first UTXO is valid as a script and spends nothing that exists. Twelve attestations of one tick with the same
signatures are accepted on one ticket. Relay floors: top-up 0.0085 KAS, roll 0.0154, attest 0.0110 (3 signatures) to
0.0187 KAS (11). A respend in every block of 10 a second: 7,323 KAS a day for the roster; 0.75 KAS a second for four
tickets (224 KAS for one testnet epoch, after which the roster's jam is enough). A loser due for liquidation buys time
at that rate; a market stopped for `termDaa + idleDaa` settles every pair at its entry.

**By reading.** Whether an honest roll or claim can still get in depends on the mempool (first seen, chains on
unaccepted outputs, replacement); the spender of its own chain publishes each link with its successor.

**The smallest fix.** None removes the race. The roster: an age (CSV) on `join`, `topUp` and `leave` and none on
`roll`, so a roll always fits between two respends (it lets a spammer delay a leave by one transaction per age instead
of one per block). The tickets: the fee per attestation of stage 2, kept in the ticket and owed to the seats, prices
the respend; more tickets per epoch raise the cost linearly.

### C1-F6, C1-F7 (Low)

F6: three rounds asserted: at the pair's age 20 the long's assign, its close and the short's empty amend are all
accepted on one outpoint; after the amend the long's entries are refused one DAA early. An amend's relay floor is
0.0268 KAS: 43,200 a day. Liquidations and renewals need no quiet time, so only a party's own exits are held. Fix as
review 12 found: none without reopening the respend against claims; state it in N§3 and N§4.

F7: lease 14,000, lapse from 15,000; a valid liquidation at the tick 14,950; the long's cure at 15,005; the cured pair
is settled at the entry in the next block (the long is paid 1,360.32 KAS: its cure of 360.32 and its margin; the
short 2,000.2). The winner's renewal of the cured pair is accepted too (a race), and it could have renewed from 8,000.
F3's chain reaches the same end without a cure. Fix: tooling (renew at the opening of the window; never propose a
liquidation in the last `graceDaa` before the lapse date without renewing first).

### C1-N1 to C1-N3 (Notes)

N1: each market in the list passes `CMarket::check` (asserted); on the term-3 market one tick renews a pair four times
in a row and anyone settles it at 6,503 (asserted). The lock-time rule is rusty-kaspa's `check_tx_is_finalized`
(`lock_time < DAA score`, by reading). The fee floor: a market with `keeperFeeMin` 5 KAS pays 0.4737 KAS for a
one-unit part (the whole maintenance slice) against 0.0464 KAS a unit for the whole pair; 1,000 such parts pay 473.68
KAS for 200 KAS of carriers, which the winner receives: the winner bears 273.68 KAS instead of 46.4. The testnet
defaults (floor 0.1 KAS, carrier 0.2 KAS) do not pay for it. A wallet should refuse `keeperFeeMin > carrier`,
`lateDaa` of 0 or above the roster's `exitDaa`, `termDaa / 4 < quietDaa`, and show the quorum as the half that
matters until F1 is fixed.

N2: the roster's value is unchanged by a roll and each ticket must hold `ticketValue` (asserted); `retire` pays the
spender. Either the retire returns the value to the roster, or the note states the subsidy. On lineage: an attacker
who creates the roster honestly still chooses every parameter inside `checkParams` (checked only at `roll`), the first
epoch, and can hold all 16 seats for `16 x minSeat` (160 KAS on the testnet defaults) until outbid seat by seat at
1.05 times its lightest stake; `committee-check` says "OK: the roster id is that of one empty roster" for it. The
check should print the present table, the total stake and the weight F1 needs.

N3: an order accepted at 0 and filled alone at 9,100 gets the lease 8,000 and is settled in the next block; the filler
nets the tip and the order's carrier (10.2 KAS).

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| 1 | A third of the weight (33 KAS of 100) moving the median outside the honest prices: every honest subset, every sub-coalition, three claimed prices (184,320 cases on numbers, one in the engine) | holds | `c1_holds_a_third_of_the_weight_...` |
| 1 | A claimant's freedom with honest seats: 16 prices 0.02 % apart | bounded: the 6th to the 11th of 16 on equal seats (9.994 to 10.004 around 10.000; g is 0.05); on the fixture table the heavy seats' prices | `c1_bounded_a_claimant_moves_the_price_within_the_honest_spread` |
| 1 | A seat signing two prices (the claimant picks one per attestation); one key in two seats (both weights); signatures for another tick, market, roster or seat; a 17th bit; ticks outside the window or ahead of the lock time; seats of zero or negative weight, a total below the signed weight or of zero (forged states); 16 seats at the seat limit (no overflow) | holds | `c1_holds_a_ticket_counts_signed_seats_of_positive_weight_once_within_its_total` |
| 2 | What satisfies `ticked`: another roster's ticket, no id, another market, a look-alike template under the roster's id, the retire entry, odd `u` | refused (the family's tests, re-read); the push layout fixes every byte before the redeem script's header, and the header is forced by the P2SH hash and the script's length | `committee_paths_tests`; by reading |
| 2 | One output paying two inputs: a pair's mirrored short payout and another pair's long payout to one key; an exit's payout and a pair's payout to one key | refused: own-index payouts lie below the input count, mirrored ones at or above it | `c1_holds_two_inputs_never_share_a_payout_output` |
| 3 | Two objects of the roster id in one transaction (roll with attest, attest with withdraw, retire with withdraw, two attests); a forged ticket written beside an attest, a retire, a withdraw, a top-up, either join, a leave, a roll (as a fourth ticket, as its third, as the exit, outputs swapped) | refused: every entry of the three contracts requires one input of the id and fixes the count of its outputs | `c1_holds_no_two_objects_of_the_roster_id_...` |
| 3 | Stake leaving the roster other than as its own seat's exit; weight without KAS; a roll paying its tickets from the stake; rolls of several epochs, late rolls, a roll ahead of the lock time; the release of a seat leaving a late roster (at once, and its key is in no later ticket) | holds | `c1_holds_stake_leaves_the_roster_...`, the roster walk (stake conserved to the sompi over 60 seeds; every exit released at least `exitDaa` after the last ticket holding its key) |
| 3 | The eviction rule and the model: 2,471 outbiddings, 6,668 refused joins (a sompi short, a lighter or empty seat elsewhere, seat 16, a negative amount) | the engine agrees with `Roster::join` | the roster walk |
| 4 | A roster not begun empty | out of the covenants' reach by design; `committee-check` recomputes the id over one genesis output (index, value, the empty roster's script) | by reading (`ccmds.rs` 81-110) |
| 5 | Value of a passive party at the feed across amend, addMargin, assign (whole and part), keepAlive, propose (whole and part, fresh and stale ticks), cure, restore: unchanged but for a few sompi of split rounding; a valid close pays the non-closer at least its value at the attested price; a forfeit takes from the winner at most the fee | holds over 60 seeds; every output of every accepted transaction is pinned (one sompi less refused) | the market walk |
| 5 | A stayer harmed by an assign (key, bound, entry, lease, listing) | holds | `committee_paths_tests::a_side_is_assigned_in_one_transaction`, the market walk (765 assigns) |
| 5 | The lease moved back, or forward beyond a tick; renewals in every block | holds on markets with `termDaa >= 4` (a quarter term per renewal); C1-N1 below that | `committee_paths_tests`, `c1_bounded_the_market_bounds_...` |
| 5 | A loser escaping by respending (amend, assign to a key of its own at an invented price, addMargin) | once per `quietDaa`, as in the oracle family; the void claim is the new way (C1-F3) | by reading, C1-F6 |
| 6 | A cure racing a finalize; a cure of a void claim (accepted: the loser's signature and money); a claim at exactly `t0 + lateDaa` | as designed | `c1_bounded_a_cure_landing_...`, `c1_bounded_one_stale_tick_...`, `committee_tests` |
| R6 | A pending part or an assigned part whose fresh id a second output of the genesis also carries | refused (the scan) | `c1_holds_two_pairs_never_claim_into_one_pending_part_and_no_claim_shares_its_id` |
| R7 | Two pairs of one state claiming into one pending output | refused (the maker) | the same |
| R9 | An expiry in the past: `fill` | bounded by the order's own age (C1-N3); `take`: C1-F2. A renewal moves no value, so nothing is drained by renewing | `c1_bounded_an_order_older_than_a_term_...` |
| R10, R11 | Parts below `minSize` or the part floor | refused in the walk (every part of 999,999 base units) and by `checkPart`'s floor | the market walk; by reading |
| R12 | A false close | needs half a quorum (C1-F1); nothing else prices a close | C1-F1 tests |

### Lead 7: the economics, in the fixture's numbers

- **A colluding 67 % today** takes the whole value of every pair of its markets (3,000.2 KAS on the worked pair, in
  one block by a false close) and risks nothing: stage 1 slashes nothing and the stake is released `E + U` after the
  seat leaves. The note admits this for a quorum; C1-F1 makes it true of 33.5 %, bought on the fixture for 50 KAS that
  come back.
- **A 34 % minority** stops every market by not signing (as the note says), and takes every pair by signing (C1-F1).
- **A single claimant with honest seats** attests any price between about the 34th and the 66th weight percentile of
  the signed prices and any tick of the last `lateDaa`: on 16 equal seats spread over 0.3 %, 0.1 %; inside g.
- **Anyone, with no stake**: a take of a resting order (C1-F2), void claims (C1-F3), the respend of the shared UTXOs
  (C1-F5).

## Not checked

- Mempool behaviour on a node: the races of C1-F3, C1-F5 and C1-F6 (first seen, chains on unaccepted outputs,
  replacement by fee, whether a transaction is relayed before its sequence lock is met); the fees are relay-floor
  estimates.
- `kpf-cli` (`ccmds.rs`) beyond the order's defaults and `committee-check`; the seats' signing and publishing tools
  (which markets a seat signs for, at which ticks); what appeared during the review: `committee_build.rs`,
  `committee_artifacts.rs`, the pinned artifacts under `contracts/artifacts/committee`, `committee_cli.rs`, `LIVE-3.md`.
- Stage 2 (N§5a: forfeit, charge, slash, pools, the fee per claim): designed, not built, not reviewed.
- `transfer`, `mutualClose`, `coopClose` and an order's `take` with a real pair beyond the family's own tests: they are
  `KpfPairO`'s, read for differences only. The walk drives no orders and no listings' takers.
- The payout branches below `PLAIN_DUST` (by reading: at most 0.02 KAS per lineage end goes to the other side, and
  `checkPart` keeps each side's value at the entry at 0.2 KAS in every piece); the 64-bit edge of the pair's
  arithmetic (unchanged from `KpfPairO`, review 9).
- The exact script-unit budget of a propose with 11 signatures beside several pairs.

## Judgement

The covenant plumbing of the new family holds: one roster id never authors a forged ticket, stake leaves only as its
seat's exit, a pair takes a price only from a ticket of its roster, fresh ids and makers survive the earlier attacks,
and two random walks found no step in which a passive party loses value outside the note's list.
What does not hold is the rule the family exists for: a price is the median of whichever signatures the claimant
includes, so half a quorum decides it, and the note's two thresholds (67 % to lie, a third to stop) are one threshold
of 33.5 %. The second gap is older: `take` still trusts a pair it never sees, and without an underwriter's slot in the
way that is open to anyone against any resting order.
Both have small fixes (a spread bound in `attest`; health and lease bounds in `take`); the void-claim chain, the price
cap and the shared UTXOs need a rule each before a live run means anything. Round C2 should start from those five.

## Status after the fixes (review C2)

The fixes of 2026-11-21 changed all six contracts; `auditC1_tests.rs` and `auditC1_walk_tests.rs` were brought to them
(an `c1_exploit_` test became `c1_fixed_` and asserts the refusal at the step that used to succeed; the numbers above
are those of the family as first built). Details and what the fixes left or opened are in `REVIEW-C2.md`.

| # | Status | The rule now | Test |
|---|---|---|---|
| C1-F1 | closed | every included seat signed within `spreadBps` of the price claimed; a false price needs a quorum signed near it | `c1_fixed_half_a_quorum_no_longer_dictates_the_price`, `c1_fixed_six_lying_seats_of_fifteen_attest_nothing`, `c1_fixed_five_minimum_stakes_bought_into_the_fixture_roster_dictate_nothing`; the claimant's freedom is now the spread (`c1_bounded_a_claimant_moves_the_price_by_the_spread_around_what_a_quorum_signed`, and C2-F1) |
| C1-F2 | closed | `take` requires both sides healthy at the price and a lease between half a term after the order and a term from the lock time | `c1_fixed_a_take_needs_the_counter_side_healthy_at_the_price`, `c1_fixed_a_take_writes_no_lease_that_is_over_or_never_ends`; the residual for an order older than half a term and the idle time: `c1_bounded_an_old_order_is_filled_or_taken_into_a_lapsed_pair` |
| C1-F3 | closed (a bounded residual) | a tick more than `lateDaa` older than the pair UTXO is refused, the UTXO must be accepted, a void claim is restored at once with its listings | `c1_fixed_one_stale_tick_holds_a_pair_once_for_a_block_and_keeps_its_listings`, `c1_bounded_a_loser_holds_its_pair_in_void_claims_one_block_in_two_while_it_wins_every_race` |
| C1-F4 | closed | a price above `priceMax` counts as `priceMax` | `c1_fixed_a_feed_above_priceMax_is_claimed_at_priceMax` |
| C1-F5 | the roster: closed; the tickets: open by decision (N§7) | `join`, `topUp`, `leave` need the roster 20 DAA old, a top-up is at least 1 KAS, `roll` needs no age | `c1_fixed_the_roster_rests_after_every_change_and_a_roll_does_not`, `c1_bounded_anyone_respends_a_ticket_for_a_fee` |
| C1-F6 | open by decision (N§4) | unchanged | `c1_bounded_the_counterparty_restarts_the_quiet_time_of_an_assign_and_of_a_close` |
| C1-F7 | open by decision (N§9) | unchanged | `c1_bounded_a_cure_landing_after_the_lapse_date_is_settled_at_the_entry` |
| C1-N1 | partly closed | `termDaa >= 1,200`, `lateDaa > 0`, `epochDaa >= 100`, tickets of 1 to 100 KAS, a spread of at most 10 % | `c1_bounded_the_bounds_still_admit_markets_and_rosters_a_wallet_must_refuse` |
| C1-N2 | stated in the note (N§2) | unchanged; a ticket is 1 KAS now | `c2_bounded_a_roll_costs_its_roller_the_tickets_and_whoever_retires_them_keeps_them` |
| C1-N3 | stated in the note (N§4) | | `c2_bounded_an_orders_expiry_stops_neither_a_fill_nor_a_take` (one sentence of it does not hold) |
