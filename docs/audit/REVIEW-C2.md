# KPF review C2 (the committee family after the fixes of review C1)

## Scope and method

Target: the committee family as fixed on 2026-11-21 after review C1 and live run 3: `contracts/KpfRoster.sil`,
`KpfTicket.sil`, `KpfExit.sil`, `KpfPairC.sil`, `KpfPendingC.sil`, `KpfOpenC.sil` (all six changed; read again in full
or by their changed parts against the C1 reading), the models (`committee.rs`, `cmarket.rs`), the kit, and this round
also the tooling: `crates/kpf-protocol/src/committee_build.rs` and `crates/kpf-cli/src/ccmds.rs`, read against
`docs/DESIGN-COMMITTEE.md` as updated for the fixes ("the note", `N§n`). The tooling was read in full by a second
reader; what this report states about it was re-read here (`c-watch`, `tick-sign`, the claim builders), run with the
built binary, or is marked "by reading (second reader)".

Files of this round (nothing else was changed; `REVIEW-C1.md` got a status section):

- `crates/kpf-tests/tests/auditC1_tests.rs`, rewritten for the fixed family: 22 tests (8 `c1_fixed_`, 8 `c1_bounded_`,
  6 `c1_holds_`). Every `c1_exploit_` of review C1 is now a `c1_fixed_` that asserts the refusal at the step that used
  to succeed; none of the fixes failed.
- `crates/kpf-tests/tests/auditC1_walk_tests.rs`, rewritten: the roster walk with the rest and the spread rule; the
  market walk on a market whose `priceMax` the feed crosses, with orders taken with no pair and by a listed side,
  transfers, mutual and cooperative closes, stale ticks against the pair UTXO, void claims restored at once.
- `crates/kpf-tests/tests/auditC2_tests.rs`: 12 tests (1 `c2_exploit_`, 10 `c2_bounded_`, 1 `c2_holds_`).
- `crates/kpf-protocol/tests/auditC2_builders.rs`: 1 test (`c2_exploit_`), the tool's builders on the pinned artifacts
  through `verify::verify`.

Runs:

- `CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test auditC1_tests --test auditC1_walk_tests --test auditC2_tests`:
  `test result: ok. 22 passed`, `ok. 2 passed` (5.84 s, seeds 1 to 6), `ok. 12 passed`.
- The walks with `C1_SEEDS=7..60 C1_STEPS=300`: `ok. 2 passed` (71.40 s). Market walk, accepted: 129 takes with no
  pair and 71 of a listed side, 561 assigns, 102 transfers, 80 mutual closes, 203 cooperative closes, 818 amends, 892
  margin top-ups, 750 renewals, 371 lapse settles, 920 closes and 683 liquidations proposed (746 of them on a tick 150
  DAA old), 97 cures, 618 closes settled, 172 forfeits, 658 restores (40 inside the grace); 7,392 variants refused as
  predicted (943 takes and 762 claims with a stale tick among them). Roster walk: 3,758 joins (2,203
  outbidding), 777 top-ups, 762 leaves, 2,353 rolls (192 inside the rest), 2,102 withdrawals, 711 attestations
  accepted; 16,896 refused as the model predicts (1,278 of them table entries inside the rest). No violation.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-protocol --jobs 2 --test auditC2_builders`: `ok. 1 passed`.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test committee_cli`: `ok. 2 passed` (the tool's own lifecycle).
- The family's own `roster_tests`, `committee_tests`, `committee_paths_tests`, `c_experiments_tests`: 13, 8, 8, 4 passed.
- The built `kpf` binary by hand, offline: `roster-create --spread-bps 1000`, `roster-check`, `c-market --g-bps 5`,
  `c-check`, `tick-sign` (a price above `priceMax`; two prices for one tick; a tick at DAA 4 x 10^11), `c-watch`.

Numbers use the kit's fixture unless stated: 100 KAS of weight in 15 seats (30, 22, 15, 12, 8, 2, 2, seven of 1, 2), a
quorum of 67 % within a spread of 0.5 %; the market with a lease of 8,000 DAA, a lapse 1,000 after it, 50 DAA of grace,
`lateDaa` 100, 20 DAA of quiet, mmr 5 %, g 2 %, a fee of 0.5 %, a carrier of 0.2 KAS; the worked pair of 1,000 units at
10 KAS, bounds 9 and 12. Fees are the relay floor at 100 sompi a gram, estimated as in review C1.

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C2-F1 | High | Whoever builds a claim chooses the price: any `p` within the roster's `spreadBps` of what a quorum signed. The spread is the roster's parameter (up to 10 %), the tolerance `g` the market's (from one basis point), and neither the contracts nor `c-check` relate them. On a roster with a spread above a market's `g`, honest signatures of the feed liquidate a healthy side and close above the feed: spread 10 %, g 2 %, every seat signing 10.00: 9.0909 is attested and the long (7.7 % above its trigger) forfeits 1,000 KAS; 11.1111 is attested and the long closes for 1,888.89 KAS instead of 1,000. `c-check` prints two OKs for such a market | `c2_exploit_on_a_roster_whose_spread_exceeds_a_markets_tolerance_honest_signatures_take_a_healthy_side`; with the tool's builders and full verification `c2_exploit_the_tools_builders_take_a_healthy_side_with_honest_ticks_when_the_spread_exceeds_g`; the tool by running it |
| C2-F2 | Medium | An order's expiry binds nothing on chain: `fill` and `take` read `activeFrom` only, and an expired order stays fillable until someone spends it by `refund`, which nobody is paid or prompted to do. A long order that expired at 12,000 is filled at its limit at 15,100 with the feed at 9.20 and liquidated 50 DAA later: its whole margin, 1,000 KAS; or taken into a lease already due (its tip and carrier, 10.2 KAS). N§4 says "an order's `expiry` is its owner's bound on that" | `c2_bounded_an_orders_expiry_stops_neither_a_fill_nor_a_take`; the tools by reading (second reader: no builder or command compares an order's expiry with the present) |
| C2-F3 | Medium | `c-watch` does not see what happens to a party's pair. A pair that anyone respent (a renewal, a claim, the other party's amend) prints "NOT AT THE NODE" and the command exits 0; no command finds the pending part or the renewed pair, so a claim against a party is invisible to it during its time to cure, and its own entries need a file it no longer has. No renewal is asked for before the lapse (the first ACT is "LAPSED ... `c-lapse-settle`"), which is the defence the note gives against C1-F7. "Neither side can be liquidated at this price" tests the one price typed, not the spread around it nor the last `lateDaa` | by reading (`ccmds.rs` 818-845, 900-988), `c-watch` run on stale files |
| C2-F4 | Medium | The fix of C1-F4 cannot be reached with the shipped signer: `tick-sign` refuses a price above `priceMax` ("the price is outside [0, priceMax]"), so above it no seat signs, nothing is attested, and the short is not liquidated and the long does not close, as before the fix | `tick-sign` run with 1,000,001 KAS on the default market; `ccmds.rs` 345; the contract side: `c1_fixed_a_feed_above_priceMax_is_claimed_at_priceMax` |
| C2-F5 | Medium | The lineage check is where the note puts it for a market's wallet and nowhere for the others. `roster-join` and `roster-topup` take the roster's object file and no genesis facts: who joins a roster that did not begin empty funds a forged seat's exit (a seat of 100 KAS written over 1 KAS of value leaves with the joiner's 100 KAS; `join` and `leave` never compare the value with the table). `c-order` drops the genesis facts of its market file, and `c-open-taker`, `c-take`, `c-transfer` and `c-assign` take object files without a market to compare | `c2_bounded_who_joins_a_roster_that_did_not_begin_empty_funds_a_forged_seats_exit`; the commands by reading (second reader; `ccmds.rs` 223, 531) |
| C2-F6 | Low | `Attestation::collect`, which every claim command uses, tries only the signed prices as `p`: seats that signed 10.00 (30 KAS) and 10.08 (37 KAS) are attested by the ticket at 10.04 and by the library not at all. Honest claimants get "below the quorum" for ticks a custom builder claims; and prices more than about twice the spread apart attest nothing (0.2 % on the testnet defaults) | `c2_bounded_honest_prices_wider_than_the_spread_attest_nothing_and_the_library_misses_a_price_between_two` |
| C2-N1 | Note | Parameters inside the bounds that a wallet must still refuse, each printed by `c-check` under "OK": `quietDaa >= termDaa / 4` (one renewal a quarter term keeps every party entry out, and the lease never ends); `mmrBps` at or below `gBps` (a short whose bound is near `priceMax` is never liquidated above it; the long's close leaves it 260 KAS of a range it has lost); a spread of 0 (seats must sign the same sompi); `lateDaa` above the roster's `exitDaa`; a keeper fee floor above the carrier (review C1) | `c2_bounded_a_quiet_time_of_a_quarter_term_or_more_lets_anyone_hold_a_pair_by_renewing_it`, `c2_bounded_at_priceMax_a_short_is_liquidated_only_if_the_maintenance_exceeds_the_tolerance`, `c1_bounded_the_bounds_still_admit_markets_and_rosters_a_wallet_must_refuse` |
| C2-N2 | Note | `tick-sign` signs whatever it is given: a second price for the same tick (the file of the first is overwritten, no record, no warning beyond a printed sentence), a tick of the far future, any market file without its roster's facts. `c-close` and `c-liquidate` refuse a stale tick only with `--node` and with no margin for the landing delay; a void claim on a part costs its funder the carrier; `c-cure` cures a void claim (margin added and the listings cleared where `c-finalize` restores for free) | `tick-sign` run; the rest by reading (second reader) |
| C2-N3 | Note | Statements of the note and of the sources the contracts do not keep: listed below | by reading, and the tests named there |

No Critical finding. C2-F1 is High and not Critical because it needs a market whose `g` does not exceed its roster's
spread, which the note warns against ("should be several times") and the testnet defaults avoid (0.1 % and 0.5 %).

### C2-F1 (High). The spread is not bound to the tolerance

**Affected:** `KpfTicket.attest` (the claimant's `p`), `KpfPairC.ticked` (reads the market, `t0` and `p` from the
ticket's signature script, not the spread in its state), `checkMarket` (`gBps > 0`), `KpfRoster.checkParams`
(`spreadBps <= 1000`), `c-market`, `c-check`, `roster-check`; N§1 ("A market's `g` should be several times its
roster's spread"), N§4 ("both inside `g` by the market's choice").

**The rule that is missing.** With every seat signing `P`, any `p` in `[P / (1 + s), P / (1 - s)]` is attested. The
liquidation trigger and the close price carry `g` to absorb that; they do so only if `g > s`, and the pair never sees
`s`. A market is written by whoever writes its first order; the roster's parameters are whatever its creator chose.

**The attack (`c2_exploit_on_a_roster_...`).** A roster with `spreadBps` 1,000 rolls (its tickets carry the spread);
the fixture market, the worked pair, seats 0 to 2 signing 10.00 for the tick 6,500.

- A keeper: inputs the pair (`propose` kind 2, long, `a` = 9.0909091), the ticket (`attest` with the three honest
  signatures and `p` = 9.0909091: accepted, one sompi lower refused), funding. The trigger holds
  (`ceil(9.0909 x 1.02) x 0.95 = 8.81 < 9`). After the grace: the short holds the forfeited part (2,954.75 KAS), the
  keeper the fee (45.45 KAS); the long's side was worth 1,000 KAS.
- The long: `propose` kind 1 with `a` = 11.1111111; paid at `floor(11.1111 x 0.98)` = 10.8889: 1,888.89 KAS for a
  side worth 1,000. The short's mirror: 2,727.27 KAS instead of 2,000.

The builders test does the same on the testnet market (g 0.5 %) with `committee_build::propose` and `finalize`, signed
and passed through `verify::verify`: the collector's own attestation (the feed) is refused as "does not liquidate the
long", the same ticks with `Attestation::of(.., 909_090_910, ..)` build, verify and forfeit a long with a full 10 % of
margin; the close pays 205.56 KAS for a side worth 100.

**Where the edge is.** On the fixture (spread 0.5 %, g 2 %) the lowest attested price liquidates nobody and a closer
claiming the band's upper edge is still paid 150.75 KAS below the feed (three quarters of g). With `g` equal to the
spread a closer is paid the feed less one sompi a unit (the note's "the closer pays g" is 0.00001 KAS instead of 50),
and a long at its maintenance is forfeited on a dip of 0.001 % (`c2_bounded_the_claimants_choice_of_price_uses_up_the_tolerance`).
The seats' own differences and the feed's move within `lateDaa` come on top, so `g` must exceed the spread by both.

**The smallest fix.** `ticked` reads the ticket's `spreadBps` from its redeem script (the last eight state bytes,
`TICKET_PRE + 640`) and requires `k x spreadBps <= gBps` with `k` 2 or 3; a market on a roster with a wider spread then
has no claim, and is visibly dead. And at once, in the tool: `c-market` and `c-check` refuse the pair of them. What it
could break: the pins; nothing honest (such a market should not exist).

### C2-F2 (Medium). An order's expiry is not a bound

**Affected:** `KpfOpenC.fill`, `take` (no reading of `expiryDaa`; nothing can bound the present from above),
`refund` (anyone, no payment for it), N§4, `c-order` (`--expiry` required, compared with nothing), `c-watch` ("an
order ... resting (expires at DAA X)", no ACT), every builder that spends an order.

**The scenario (`c2_bounded_an_orders_expiry_...`).** A long order, 1,000 units, limit 10, tip 0.01, accepted at
10,000, expired at 12,000, never refunded. At 15,100: (a) taken with no pair into the lease 14,000 (half a term after
its acceptance, already due): settled at the entry in the next block, 10.2 KAS to the taker; (b) with the feed at
9.20: filled at 10.00 (both sides are healthy at the fill price; the feed is not read) with a full lease and
liquidated with a true tick at 15,150: the taker holds both sides. The refund was valid from 12,000.

**Fix.** A covenant cannot refuse a late fill. The watcher raises an ACT for an order at its expiry and `c-refund`
is part of every keeper's loop (it costs a fee; a share of the escrow paid to the refunder would make it someone's
business, at the price of a new rule). N§4 should say that the expiry binds only when a refund lands.

### C2-F3, C2-F4, C2-F5 (Medium, tooling)

F3: `watch_cmd` refreshes each file's own outpoint; on failure it prints and continues without counting (exit 0). A
pending part counts only once it can be finalized, which for a valid liquidation is when the cure time is over. For a
pair the two ACTs are "liquidatable at `--price`" and "lapsed"; between the lease's end and the lapse it prints
"lease ends in -N DAA". C1-F7 stays "as a stated limit ... which a winner avoids by renewing" (N§9): the tool that
would tell it to renew does not. Fix: discovery by the pair's and the part's addresses as review 12 proposed
(R12-F1); an ACT from the opening of the renewal window; the liquidation test with the spread and `g` around the price.

F4: either `tick-sign` signs any price up to the ticket's limit (9 x 10^14) and the pair's clamp does its work, or it
signs `min(price, priceMax)` and says so; today a seat that follows the tool leaves a market above `priceMax` without
ticks, which also stops renewals.

F5 (`c2_bounded_who_joins_...`): a roster state with seat 0 at 100 KAS over a UTXO of 1 KAS (it cannot roll); the
newcomer joins seat 1 with 100 KAS (the roster holds 101); seat 0 leaves: exit 100 KAS, roster 1 KAS; the newcomer's
seat is backed by nothing. Fix: `roster-join` and `roster-topup` take the genesis facts (`--roster ROSTER.json`) and
refuse otherwise; in the contract, `join` and `leave` could require the value to cover the table as `roll` does (a
loop the roster already has), which makes a forged table useless from the first entry. For markets: `c-order` keeps
the genesis facts and every entering command takes `--market` and compares the object's market block with it.

### C2-F6 (Low) and the notes

F6: `collect` should search `p` over the intervals the signed prices allow (the candidates are each signed price
divided by `1 + s` and `1 - s`), and let the claimant choose among the prices that reach the quorum; today it takes the
lowest signed price of the heaviest group.

N3, what the note and the sources say and the contracts do not keep:

- N§4, "An order's `expiry` is its owner's bound on that": C2-F2.
- N§1, "A market's `g` should be several times its roster's spread": nothing checks it (C2-F1).
- N§4, "a close by the other party ... the closer pays `g`": it pays `g` less the spread when it claims the band's
  edge (150.75 of 200 KAS on the fixture, 40 of 50 on the testnet defaults, nothing when the two are equal).
- N§1 and N§9, "honest seats whose prices differ by more than the spread attest nothing": they are attested up to
  `(1 + s) / (1 - s)` apart, about twice the spread, at a price between them; it is the library that attests nothing
  there (C2-F6).
- N§4, "a void claim on its pair: one block off the market": and the quiet time of the parties' own entries starts
  again (20 DAA), and a void claim on a part leaves the pair in two lineages for good.
- N§8: "sixteen signatures". `KpfPairC.sil` (header, `ticked`): "the price their median", "the median";
  `KpfPendingC.sil` (header): every return state "unlisted", while the restore keeps the listings.

## Review C1's findings after the fixes

| C1 finding | Now | Test |
|---|---|---|
| C1-F1 (High) | closed: 34 KAS of 100, six lying seats of fifteen, and five minimum stakes bought in, attest nothing false; less than a quorum cannot move a price beyond the spread of an honest one | `c1_fixed_half_a_quorum_no_longer_dictates_the_price`, `c1_fixed_six_lying_seats_of_fifteen_attest_nothing`, `c1_fixed_five_minimum_stakes_bought_into_the_fixture_roster_dictate_nothing`, `c1_holds_less_than_a_quorum_cannot_attest_beyond_the_spread_of_an_honest_price` |
| C1-F2 (High) | closed: a counter bound at the price, a lease of 0 and a lease beyond every clock are refused by `take` | `c1_fixed_a_take_needs_the_counter_side_healthy_at_the_price`, `c1_fixed_a_take_writes_no_lease_that_is_over_or_never_ends` |
| C1-F3 (Medium) | closed as found: a stale tick holds a pair once, for one block, and keeps its listings. What is left is a race a loser must win at every restore (below) | `c1_fixed_one_stale_tick_holds_a_pair_once_for_a_block_and_keeps_its_listings`, `c1_bounded_a_loser_holds_its_pair_in_void_claims_one_block_in_two_while_it_wins_every_race` |
| C1-F4 (Medium) | closed in the contract (25 KAS attested on a market capped at 20: the short forfeits, the long closes for the whole range); open in the tool (C2-F4) | `c1_fixed_a_feed_above_priceMax_is_claimed_at_priceMax` |
| C1-F5 (Medium) | the roster: closed (a top-up of one sompi, and any table entry within 20 DAA of the last, refused; a roll lands in the block after a respend). The tickets: open by decision | `c1_fixed_the_roster_rests_after_every_change_and_a_roll_does_not`, `c1_bounded_anyone_respends_a_ticket_for_a_fee` |
| C1-F6, C1-F7 (Low) | open by decision | `c1_bounded_the_counterparty_restarts_the_quiet_time_of_an_assign_and_of_a_close`, `c1_bounded_a_cure_landing_after_the_lapse_date_is_settled_at_the_entry` |
| C1-N1 to C1-N3 | N1 partly closed (the term, `lateDaa`, the epoch, the ticket's value); N2 and N3 stated in the note | `c1_bounded_the_bounds_still_admit_markets_and_rosters_a_wallet_must_refuse` |

### The limits kept by decision, and whether they may stay

- **C1-F6, the quiet time restarted by the counterparty.** Yes. It is inherent in an age rule on a UTXO both parties
  may spend; 0.0268 KAS a time, 1,159 KAS a day to hold one pair's exits, and liquidations and renewals are not held.
- **C1-F7, a cure or restore after the lapse date.** Only with a tool that asks for the renewal: today none does
  (C2-F3). The cost when it happens is the liquidation: 953.6 KAS on the worked pair.
- **The tickets respent.** For a testnet, yes, as N§7 now states it: 0.0178 KAS an attestation of 11 signatures, 214
  KAS to hold the four tickets of a 5-minute epoch in every block, with no stake. It is cheaper than the third of the
  weight the note asks for a stop, and a loser buys time with it; the fee per attestation of stage 2 should come
  before any value that makes 214 KAS worth paying.
- **Who funds a roll.** For a testnet, yes: 4 KAS an epoch, 1,152 KAS a day, taken by whoever retires the tickets two
  epochs later (a stranger does: `c2_bounded_a_roll_costs_its_roller_...`). A roller that does not win its tickets back
  pays for the committee's liveness alone; a ticket that names the key it returns to (32 bytes of state) ends that.
- **The void claim's residual (C1-F3).** Yes. A claim needs a tick at most `lateDaa` older than an accepted pair
  UTXO, so the loser's chain is one void claim per restore, on the one tick that is `lateDaa` older than it, against
  a keeper's valid claim on the same UTXO in every round (asserted for 60 rounds): 0.0423 KAS a round, 18,267 KAS a
  day, and 4,275 races won in a row where review C1 needed 171 uncontested ones.
- **`take`'s half term.** An order older than half a term and the idle time (4.5 days on the testnet defaults) is
  taken into a lapsed pair for its tip and carrier (`c1_bounded_an_old_order_is_filled_or_taken_into_a_lapsed_pair`);
  stated in N§4 and acceptable once C2-F2 is.

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| a | The band rule: who chooses `p`; the edges to the sompi; a third that a quorum needs (it pins the price at the edge of the spread, 0.5 %, no further); `p` = 0 (only from signed zeros); a negative or a huge signed price; a spread above 100 % in a forged state; 9 x 10^14 with the widest spread (no overflow) | holds, bounded by the spread | `c1_holds_less_than_a_quorum_...`, `c1_bounded_a_claimant_moves_the_price_by_the_spread_...`, `c2_bounded_a_third_that_a_quorum_needs_pins_the_price_at_the_edge_of_the_spread`, `c1_holds_a_ticket_counts_signed_seats_of_positive_weight_once_within_its_total` |
| a | Two quorums attesting two prices for one tick | at most two spreads apart while each seat signs one price; a cured part is reopened by the same tick only if the spread exceeds g (C2-F1's relation) | by reading; `c2_bounded_the_claimants_choice_of_price_uses_up_the_tolerance` |
| a | The bound of a tick against the pair UTXO: every valid claim passes it (all landings on numbers); an old pair UTXO; a claim chained on an unaccepted UTXO (refused, both kinds; a renewal is not held); a party's respend does not take a recent tick from a liquidator | holds; an honest liquidator waits one block for an accepted UTXO, as the loser does | `c2_holds_the_bound_of_a_tick_refuses_no_claim_that_could_be_valid_and_none_on_an_unaccepted_pair` |
| a | The clamp: a liquidation and a close at exactly `priceMax`, one sompi above, at 9 x 10^14; the pending part must store the clamped price; no cure above `priceMax`; a long's liquidation or a short's close at a clamped price | holds on markets with mmr above g (C2-N1 otherwise) | `c1_fixed_a_feed_above_priceMax_...`, `c2_bounded_at_priceMax_...`, the market walk (feed up to 14 on a cap of 13) |
| a | Void claims without the grace; the listings kept by a restore and cleared by nothing else | holds | `c1_fixed_one_stale_tick_...`, the market walk (658 restores, the listings compared) |
| a | `take`'s lease rule: a listed side with less than half a term left (refused to a young order, taken after a renewal and the quiet time, taken at once by a key or an older order); the edges of the lease | holds; not stranded while the committee signs | `c2_bounded_a_listed_side_with_less_than_half_a_term_left_is_renewed_before_a_young_order_takes_it`, `c1_fixed_a_take_writes_no_lease_...` |
| a | The roster's rest: a roll in the block after a respend; a late roster's stale rolls (one epoch each, no rest); a leave or a join raced by a top-up of 1 KAS at each end of the rest | holds; holding a leave back costs 364 KAS of fees and 43,200 KAS of stake (given to a seat) a day | `c1_fixed_the_roster_rests_...`, the roster walk |
| b | Transfer, mutual close, cooperative close, an order's take of a listed side, orders with no pair, in the walk | the engine agrees with the prediction; every output pinned; a passive party keeps its value at the feed. A mutual close leaves at most one sompi of rounding to the builder | the market walk |
| b | Payouts below the dust floor: a part's close whose other share is 0.0196 KAS goes to the closer (the claim costs 0.0424 KAS of fees); a lapse whose long holds 0.01 KAS at the entry pays it nothing (no order, hand-over or cure writes such a state); an assign's payout of 0.00001 KAS stays in the part | bounded: never worth its transaction | `c2_bounded_a_payout_below_the_dust_floor_goes_to_the_other_side_and_is_worth_less_than_its_transaction` |
| c | The builders: indices, values, lock times and the sequences of the rest, the quiet time and the grace; the models against the contracts | no difference found but C2-F6 | by reading (second reader); `committee_cli`, `auditC2_builders` |
| C1 | Every `c1_holds_` of review C1 (the roster's id, the stake's conservation, twins and shared ids, shared payouts) | holds on the fixed templates | `auditC1_tests` |

Economics after the fixes: a false price needs a quorum signed within the spread of it. On the fixture that is 177 KAS
of returnable stake against the 87 KAS of the five heaviest seats (ten light seats outbid, by arithmetic), and
nothing is slashed in stage 1: the note's first limit, at its true price. A third of the weight stops the market and
moves an attested price by the spread; a claimant alone moves it by the spread too.

## Not checked

- Mempool and node behaviour: every race named here (a restore against a keeper, the rest's end, the quiet time's
  end), whether a node relays a claim whose pair UTXO is not accepted, the fees beyond the relay floor's estimate.
- `ccmds.rs` and `committee_build.rs` line by line by this reviewer: read in full by a second reader, spot-read and
  run here; `committee_cli.rs` only run. The second run of `LIVE-3.md`.
- Stage 2 (N§5a): designed, not built.
- The storage mass of the smallest outputs under a covenant id (an order's remainder or a part at the 0.2 KAS floors,
  against the 1 KAS live run 3 found for a ticket): not measured.
- The 64-bit edges of the pair's arithmetic and `KpfPairO`'s entries beyond the walk.

## Judgement

The five fixes hold: no attack of review C1 survives at the step where it worked, the rewritten walks find no step in
which a passive party loses value outside the note's list, and what the fixes left is measured and small.
The spread rule moved the open question from who signs to who chooses: the claimant picks the price inside a band
whose width is the roster's and whose cost the market's `g` must cover, and nothing ties the two together; that is
one comparison in `ticked` and one refusal in `c-check`.
The rest of this round is the tooling, which had not been read before: an order's expiry that binds nothing, a watcher
that sees neither a claim nor a missing renewal, a signer that cannot sign above `priceMax`, commands that stake and
enter without the lineage. None of them is a covenant rule; all of them lose a user's money with the contracts
working as written. The round did not converge (one High, four Medium); C3 should find the comparison in place and
the watcher able to discover.

## Status after the fixes (review C3)

`auditC2_tests.rs` and `auditC2_builders.rs` were brought to the fixed family (a `c2_exploit_` became `c2_fixed_` and
asserts the refusal at the step that used to succeed; the numbers above are those of the family before these fixes).
Details in `REVIEW-C3.md`.

| # | Status | The rule now | Test |
|---|---|---|---|
| C2-F1 | closed | `KpfPairC.ticked` requires `4 x spreadBps <= gBps` of the ticket it is given, for a claim and a renewal; the builders, `c-market` and `c-check` refuse the pair of roster and market | `c2_fixed_a_roster_whose_spread_exceeds_a_quarter_of_a_markets_tolerance_prices_none_of_its_pairs`, `c2_fixed_the_tools_builders_refuse_a_claim_and_a_renewal_when_the_spread_exceeds_a_quarter_of_g`; the residual: `c2_bounded_the_claimants_choice_of_price_uses_a_quarter_of_the_tolerance_at_most` |
| C2-F2 | closed as far as a covenant can | `refund` pays its sender `keeperFeeMin`; `c-watch` raises an expired order | `c2_bounded_an_orders_expiry_stops_neither_a_fill_nor_a_take` (unchanged: a fill that lands first still fills), `c3_bounded_the_refunders_fee_is_the_markets_keeper_fee_floor_up_to_the_whole_escrow` |
| C2-F3 | closed | a spent pair or part, the last quarter of a lease and a curable liquidation of one's own side are actions | by reading; `auditC3_cli` |
| C2-F4 | closed | `tick-sign` signs up to the ticket's limit | `committee_cli` |
| C2-F5 | closed (but `c-order`: C3-F2) | every roster entry writes a table its value covers; the roster and entering commands take the checked facts | `c2_fixed_a_roster_that_did_not_begin_empty_accepts_no_stake_and_pays_none_out` |
| C2-F6 | closed | the collector tries the ends of each signed price's band | `c2_fixed_the_library_finds_a_price_between_two_groups_of_signed_prices` |
| C2-N1, C2-N2 | mostly closed | `market_warnings`; the signer's guards | `committee_cli`, `auditC3_cli` (C3-N2, C3-N4) |
