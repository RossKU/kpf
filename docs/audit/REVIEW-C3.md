# KPF review C3 (the committee family after the fixes of review C2)

## Scope and method

Target: the committee family as fixed after review C2: the six contracts (the changed parts read again:
`KpfPairC.ticked`, `KpfOpenC.refund`, `KpfRoster.sumOf` and `cont`; then every entry once more for one question, "which
output or value here is not pinned by a require"), the models, and the tooling, read in full by this reviewer this
time: `crates/kpf-protocol/src/committee_build.rs` (every builder) and `crates/kpf-cli/src/ccmds.rs` (every command),
against `docs/DESIGN-COMMITTEE.md` as corrected ("the note", `N§n`).

Files of this round (nothing else was changed; `REVIEW-C2.md` got a status section):

- `crates/kpf-tests/tests/auditC2_tests.rs`, brought to the fixes: 12 tests (3 `c2_fixed_`, 8 `c2_bounded_`, 1
  `c2_holds_`); `crates/kpf-protocol/tests/auditC2_builders.rs`: 1 `c2_fixed_`. No fix of review C2 failed. The C1 files
  needed no change.
- `crates/kpf-tests/tests/auditC3_tests.rs`: 3 tests (1 `c3_bounded_`, 2 `c3_holds_`).
- `crates/kpf-protocol/tests/auditC3_builders.rs`: 1 test (`c3_bounded_`): the smallest objects through the builders
  and `verify::verify`.
- `crates/kpf-cli/tests/auditC3_cli.rs`: 6 tests through the binary (4 `c3_bounded_`, 2 `c3_holds_`).

Runs:

- `CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test auditC1_tests --test auditC1_walk_tests --test auditC2_tests --test auditC3_tests`:
  `ok. 22 passed`, `ok. 2 passed` (5.01 s), `ok. 12 passed`, `ok. 3 passed`.
- The walks with `C1_SEEDS=7..40 C1_STEPS=300` on the fixed templates: `ok. 2 passed` (44.89 s), no violation.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-protocol --jobs 2 --test auditC2_builders --test auditC3_builders`: `ok. 1 passed`, `ok. 1 passed`.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test auditC3_cli --test committee_cli`: `ok. 6 passed`, `ok. 2 passed`.
- The family's own `roster_tests`, `committee_tests`, `committee_paths_tests`, `c_experiments_tests`: 14, 8, 10, 4 passed.

Numbers use the kit's fixture (a spread of 0.5 %, g 2 %, a keeper fee floor of 0.01 KAS) or, where said, the testnet
defaults (a spread of 0.1 %, g 0.5 %, a floor of 0.1 KAS, a carrier of 0.2 KAS, a ticket of 1 KAS).

## Findings

**Nothing above Low.** No Critical, High or Medium finding in the contracts, the builders or the commands.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C3-F1 | Low | The refunder's fee is the market's `keeperFeeMin`, which the bounds admit up to 10,000 KAS and `refund` does not compare with the escrow: on a market with a floor above an order's escrow, whoever refunds an expired order keeps all of it (a floor of 2,000 KAS: the owner of a 1,001.2 KAS order is paid one sompi). On the testnet defaults the fee is 0.1 KAS. `c-market` warns of a floor above the carrier, in words about liquidations only | `c3_bounded_the_refunders_fee_is_the_markets_keeper_fee_floor_up_to_the_whole_escrow` |
| C3-F2 | Low | `c-order` escrows into any market file: one with its roster's genesis facts cut out, which `c-check` refuses and the entering commands warn about, is taken without a word; and an `--expiry` already past is escrowed like any other. Whoever places an order is the one party the tool does not ask what it checked | `c3_bounded_c_order_escrows_into_a_market_file_that_c_check_refuses` |
| C3-F3 | Low | `c-cure` builds the cure of a void claim without saying that it is void: the loser adds margin (0.115 KAS a unit on the test's pair: its bound moves from 9 to 8.88535) and its pair comes back unlisted, where `c-finalize` (anyone, at once) restores it as it was. And offline (`--daa`) `c-close` and `c-liquidate` compare the tick with nothing: a claim on a tick 2,000 DAA old is built, lands void, and on a part costs its funder the carrier; with `--node` it is refused only once it is more than `lateDaa` old at build time, with no room for the landing | `c3_bounded_c_cure_cures_a_void_claim_and_c_liquidate_offline_checks_no_age` |
| C3-N1 | Note | `roster-roll --steps M` reads no clock: at 33,100 it builds (and verifies locally) the roll to [39,000, 42,000), which no block takes for ten minutes, and at 90,000 with `--steps 1` the tickets of [36,000, 39,000), which serve nothing, for 4 KAS of the funder's. Without `--steps` it rolls to the present | `c3_bounded_roster_roll_with_steps_reads_no_clock` |
| C3-N2 | Note | `tick-sign`'s guard against a second price is the file the first one was written to: the same seat's second price for the same tick into another `--out` is signed; and without `--daa` or `--node` a tick of any DAA is signed | `c3_bounded_the_signers_guard_against_a_second_price_is_one_file_name` |
| C3-N3 | Note | An order escrowed to the sompi (`c-order --escrow`) leaves a refund of nothing, which the builder writes as the least viable output (0.025 KAS, 400,000 grams of a block's 500,000): one such order is filled (479,923 grams), two cannot be opened against each other (the builder says why). With the default escrow (1 KAS above a full fill) the smallest pair, split, exit and their settlements are all mined (at most 161,213 grams, a roll) | `c3_bounded_the_smallest_pair_order_part_and_exit_are_mined_and_two_bare_refunds_together_are_not` |
| C3-N4 | Note | `c-watch` still tests liquidation at the one price typed (not the spread and g around it), and its last-quarter alarm is raised for every pair every term, whether or not its owner wants the position kept; `market_warnings` has no line for a long `lateDaa` (a day's look-back is inside the bounds) or a `graceDaa` of 0 | by reading (`ccmds.rs`, `pair_state`, `watch_cmd`, `market_warnings`) |

### C3-F1 to C3-F3 (Low)

F1. `KpfOpenC.refund` requires `outputs[self] >= inputs[self] - keeperFeeMin` to the owner's key. `checkMarket` bounds
`keeperFeeMin` by 10^12 sompi and `refund` does not call it; the order's owner chose the market, so this is a market
whose parameter means more than its name says. Smallest fix: the fee is `min(keeperFeeMin, a fixed share of the
escrow)` (one line), or the wallet's warning says "and whoever refunds an expired order keeps this much of it". The
other questions asked of the fee hold: one sompi more for the refunder is refused; a refund before the expiry is
refused; two orders of one owner are each paid at their own output index; at the expiry a refund and a fill are two
spends of one UTXO, so the expiry binds when the refund lands (N§4 now says so) and the refunder is paid to try.

F2. `order_cmd` reads `cmarket_from(..)` and drops the genesis facts; it calls neither `spread_or_refuse` nor a check
that the facts exist. Fix: refuse a market file without them (or `--unchecked`, as `roster-join`), and run the same
refusals and warnings as `c-check`; with `--daa` or `--node`, refuse an expiry that is not in the future.

F3. Fix: `c-cure` reads the part's DAA as `c-finalize` does and refuses a void claim ("`c-finalize` restores it for a
fee"); the claim commands apply the staleness test to `--daa` too and leave a margin (a tick older than
`lateDaa` less a few blocks is refused, as a claim cannot land in the block it is built in).

## Review C2's findings after the fixes

| C2 finding | Now | Test |
|---|---|---|
| C2-F1 (High) | closed: the pair reads the ticket's spread and requires four times it to fit in `g`, for a claim and for a renewal; the three claims that took a healthy side are refused by the pair (the ticket still attests the prices); the builders and `c-market`, `c-check` refuse the pair of roster and market | `c2_fixed_a_roster_whose_spread_exceeds_a_quarter_of_a_markets_tolerance_prices_none_of_its_pairs`, `c2_fixed_the_tools_builders_refuse_a_claim_and_a_renewal_when_the_spread_exceeds_a_quarter_of_g`, `committee_cli` |
| C2-F2 (Medium) | closed as far as a covenant can: the refund pays its sender, the watcher raises an expired order, the note says what an expiry is. An expired order is still filled if the fill lands first | `c2_bounded_an_orders_expiry_stops_neither_a_fill_nor_a_take`, `c3_bounded_the_refunders_fee_...`, `c3_holds_the_watcher_raises_an_expired_order_and_a_refund_pays_the_owner_all_but_the_fee` |
| C2-F3 (Medium) | closed: a spent pair or part is an action (exit 3), so are the last quarter of a lease and a liquidation of `--me`'s side that can still be cured. Discovery of the object that replaced a spent one is still by hand (C3-N4) | by reading (`watch_cmd`); `c3_bounded_c_cure_...` (a void part raises "it can be finalized now") |
| C2-F4 (Medium) | closed: `tick-sign` signs up to the ticket's limit and notes a price above `priceMax` | `committee_cli` (the family's test) |
| C2-F5 (Medium) | closed in the contract (a roster whose value does not cover its table accepts no join, top-up, leave or roll; negative and oversized weights are refused by every entry) and in the roster commands; the entering commands compare the market; `c-order` does not (C3-F2) | `c2_fixed_a_roster_that_did_not_begin_empty_accepts_no_stake_and_pays_none_out`, `c3_holds_the_backing_check_stops_no_honest_sequence_of_the_roster` |
| C2-F6 (Low) | closed: the collector finds a price between two groups of signed prices, up to the widest pair one price serves | `c2_fixed_the_library_finds_a_price_between_two_groups_of_signed_prices` |
| C2-N1, C2-N2 | mostly closed (`market_warnings`; the signer's guards); what is left is C3-N2 and C3-N4 | `committee_cli`, `auditC3_cli` |

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| a | The margin of 4. A claimant has the spread `s`; honest seats that one band serves differ by at most `2s`, so a claim lies at most `3s` from the farthest honest price it includes; liars below a quorum cannot move it further (an accepted set holds an honest seat within `s` of the price). At the least margin (`g = 4s`) a closer keeps a quarter of `g` (150.75 of 200 KAS on the fixture) and a long at its maintenance is liquidated at 9.85 instead of 9.80; `s` is left for the feed's move within `lateDaa`. On the testnet defaults (`s` 0.1 %, `g` 0.5 %): 0.1 % for the claimant, up to 0.3 % with the seats' widest disagreement, 0.2 % left for 10 s of feed | holds; the constant is sufficient, and the note's "inside g" is now a rule | `c2_bounded_the_claimants_choice_of_price_uses_a_quarter_of_the_tolerance_at_most`, `c1_holds_less_than_a_quorum_cannot_attest_beyond_the_spread_of_an_honest_price`, `c2_bounded_a_third_that_a_quorum_needs_pins_the_price_at_the_edge_of_the_spread` |
| a | A market on a roster too wide for it: no claim and no renewal. Its pairs are not locked: assign, transfer, mutual close, added margin, cooperative close work, and the lapse ends each pair at its entry; nobody is liquidated and nothing is taken | holds: the intended dead state | `c3_holds_on_a_roster_too_wide_for_a_market_a_pair_keeps_every_exit_that_needs_no_price` |
| a | The refund: more than the fee; before the expiry; one output for two orders; a race with a fill | holds but for the fee's size (C3-F1) | `c3_bounded_the_refunders_fee_...` |
| a | The roster's backing: fifteen joins, twenty evictions at the least bid (a sompi less refused), rolls and top-ups between them, every seat leaving down to the empty roster; a roster holding 50 KAS more than its table; negative and oversized weights | holds: no honest sequence is stopped, a surplus stays in the roster | `c3_holds_the_backing_check_stops_no_honest_sequence_of_the_roster`, `c2_fixed_a_roster_that_did_not_begin_empty_...`, the roster walk |
| b | The builders, read in full: output indices (a first payout at its input's index, a second last, fillers when inputs outnumber outputs), lock times (the tick, the epoch, the release, the lapse date, the lease less a term), sequences (the rest, the quiet time, the grace of a valid liquidation only), who is paid. The funder keeps what the contracts leave unpinned: tips, the carrier of an order emptied into a whole pair, a retired ticket, the refund fee; all stated in the note | no difference from the contracts found | by reading; `committee_cli`, `auditC3_cli`, the builders tests |
| b | The commands asked about: `c-take` against a short lease (refused, names the renewal); ticks of two DAAs (the first file's tick; the rest not counted); a close signed by the other key; a liquidation that does not trigger; `c-cure` by the wrong key; `c-finalize` of a void claim (says VOID, restores at once); `exit-withdraw` early (the release is the lock time); `c-watch` on an expired order (exit 3); `c-refund` (11.1 of 11.2 KAS to the owner) | holds | `c3_holds_the_claim_commands_refuse_what_would_be_refused_or_lost`, `c3_holds_the_watcher_raises_an_expired_order_...` |
| c | The storage mass of the smallest objects through `verify::verify`: a pair of 0.5 KAS (89,923 grams with the order's refund), a close of the smallest part and its settlement, an assign and a lapse of the smallest pair, joins, a roll (161,213), a leave and a withdraw of 10 KAS | all mined; C3-N3 for a bare escrow | `c3_bounded_the_smallest_pair_order_part_and_exit_are_mined_...` |
| d | The last pass, each entry for an output or value no require pins. Unpinned by design and stated: a cancel and a cooperative close (their signers'), a retired ticket's value, the refund's fee, a pair's or an exit's value above its floor (the builder's own money), one sompi of rounding in a mutual close. Nothing else | holds | by reading; the walks (every output of every accepted transaction one sompi short is refused) |

## Not checked

- A node: mempool behaviour in the races named in reviews C1 and C2, relay of transactions with a lock time or a
  sequence not yet met, fees above the relay floor. The second live run in `LIVE-3.md` was not replayed.
- Stage 2 (N§5a): designed, not built.
- `docs/TESTNET.md` section 10 (the runbook) was not read line by line against the commands.
- Discovery of the object that replaces a spent pair or part: the watcher now says that something happened; finding
  it is by hand.

## Judgement

This round found nothing above Low: the stopping condition of the loop is met. The fixes of review C2 hold at the
steps where the attacks worked, the earlier attacks stay refused, and the walks still find no step in which a passive
party loses value outside the note's list. What is left are three places where a tool could say more (the refund's
fee on a market with a large floor, `c-order`'s silence about its market file, a cure of a void claim) and notes on
commands that read no clock. The limits kept by decision stand as measured in review C2: the quiet time a counterparty
restarts, a cure after the lapse date, tickets respent for fees, the roller's float, and the trust the family rests
on: a quorum of the roster's weight, unslashed until stage 2.

## Status after the fixes (review C4)

| # | Status | The rule now | Test |
|---|---|---|---|
| C3-F1 | closed | the refunder's fee is `min(keeperFeeMin, escrow / 20)` | `c3_fixed_the_refunders_fee_is_at_most_a_twentieth_of_the_escrow` |
| C3-F2 | closed | `c-order` refuses a market file without its roster's facts (or `--unchecked`), a spread too wide for g, and an expiry not after the present | `c3_fixed_c_order_refuses_a_market_file_nobody_could_check_and_an_expiry_already_past` |
| C3-F3 | closed for the cure; the offline age check of a claim's tick stays (C4-N4) | `c-cure` refuses a void claim | `c3_fixed_c_cure_refuses_a_void_claim` |
| C3-N1 | closed | `roster-roll --steps` is compared with the clock when one is given | `c3_fixed_roster_roll_with_steps_is_compared_with_the_clock` |
| C3-N2 | open (C4-N2: it now guards a stake) | | `c3_bounded_the_signers_guard_against_a_second_price_is_one_file_name` |
| C3-N4 | closed for the two warnings | `market_warnings`: `lateDaa` above 600, `graceDaa` 0 | run by hand |
