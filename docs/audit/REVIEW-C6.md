# KPF review C6 (the judged slash after the fixes of review C5)

## Scope and method

Target: what changed for review C5 in `contracts/KpfCharge.sil` (`atStake`, `decide` writing the dispute's NO total
into `clock`), `contracts/KpfRoster.sil` and `contracts/KpfExit.sil` (`takeOf`, `rewardOf`, `keptOf`, `burnAt`, the
partial `slash`, the held `forfeit`, `contWith`), the models (`take_of`, `reward_of`, `kept_of`, `Charge::takes`,
`Roster::slash`, `Exit::slash`, `Exit::forfeit_rest`), the builders (`charge_settle`, `forfeit_seat`, `forfeit_exit`,
`charge_contest`), the commands (`charge-answer`, `c-watch --me`, `charge_warnings`, the words of `charge`,
`charge-settle`, `seat-forfeit`), read against `docs/DESIGN-COMMITTEE.md` 5a ("What the design answers") and 9.

Files of this round (nothing else was changed; `REVIEW-C5.md` got a status section):

- `crates/kpf-protocol/tests/auditC5_builders.rs`, `crates/kpf-tests/tests/auditC5_tests.rs`,
  `crates/kpf-cli/tests/auditC5_cli.rs`, brought to the fixes: the four `c5_exploit_` tests became `c5_fixed_` (the
  old numbers in their comments), the bounded ones assert the new bounds. No fix of review C5 failed outright; what
  is left of C5-F2 is C6-F1.
- `crates/kpf-protocol/tests/auditC6_builders.rs`: 7 tests (1 `c6_exploit_`, 3 `c6_bounded_`, 3 `c6_holds_`).
- `crates/kpf-cli/tests/auditC6_cli.rs`: 2 tests (1 `c6_holds_`, 1 `c6_bounded_`).

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`):

- `kpf-protocol`: `auditC6_builders` `ok. 7 passed`; `auditC5_builders` `ok. 7 passed`; `auditC2_builders`,
  `auditC3_builders`, `auditC4_builders` `ok. 1 passed` each.
- `kpf-tests`: `auditC5_tests` `ok. 2 passed`; `auditC1_tests` 22, `auditC1_walk_tests` 2, `auditC2_tests` 12,
  `auditC3_tests` 3, `auditC4_tests` 4 passed.
- `kpf-cli`: `auditC6_cli` `ok. 2 passed`; `auditC5_cli` `ok. 3 passed`; `auditC3_cli` 6, `auditC4_cli` 1 passed.
- `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- check`: `pinned artifacts match a fresh compile`.

## Findings

**One Medium, three Low, two notes.** The High of review C5 is closed as a High: what is left of it is the limit the
note now states, and I agree with its numbers. The Medium is what is left of C5-F2.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C6-F1 | Medium | A slash that takes a stake whole still zeroes its holds: every other charge on it keeps its bond for ever. With the cap this is what a good verdict does (NO at least the stake less 1 KAS), so of two honest authors on one seat the second loses its bond; and a seat that holds aged judge stake of its own weight does it to its accuser at will (it charges itself for the same tick, contests, slashes itself first) and keeps the tenth | `c6_exploit_a_seat_slashed_whole_by_its_own_charge_leaves_its_accuser_with_a_locked_bond` |
| C6-F2 | Low | The proof against a held stake is the same two signatures every time: replayed by anyone, each forfeit burns nine tenths of what the last left, down to 1 KAS. "A held stake keeps a tenth" is true of one forfeit; after two, a seat of 22 KAS holds 1 KAS and the author's tenth of 2 KAS is 1 (of a seat of 5,000 KAS: 1 KAS instead of 500) | `c6_bounded_a_held_forfeit_replays_down_to_one_kas_and_the_author_is_paid_what_is_left` |
| C6-F3 | Low | The NO total is its opener's one UTXO unless YES answers: while NO leads nothing joins it. A seat that lied contests its own charge first with `minStake` (returned whole: NO is the true verdict) and loses `minStake` a charge instead of its stake; honest stakes cannot be pooled on NO either. The author must charge again, `w / minStake` times | `c6_bounded_whoever_contests_first_sets_what_the_verdict_takes` |
| C6-F4 | Low | The defence's tools: `charge-answer` builds an answer that does not take the lead (20 KAS on YES against 20 on NO; the bar is 25) without a warning, and that stake is then on the losing side; `c-watch --me` raises a held seat and a charge against one's key but not a held exit of one's key | `auditC6_cli::c6_holds_charge_answer_takes_only_the_dispute_of_a_charge_the_stake_it_is_shown_holds` |
| C6-N1 | Note | `seat-forfeit` on a held seat says "its 22 KAS are forfeited": 19.8 are burned, 2.2 stay, and the sender, who was paid a tenth before the fix, is paid nothing and not told. It builds the replay too ("its 2.2 KAS are forfeited") | `auditC6_cli::c6_bounded_what_the_commands_say_of_a_held_forfeit_and_of_a_roster_that_cannot_slash` |
| C6-N2 | Note | C5-F5 is closed where a roster is described (`roster-create`, `roster-show`). `roster-join --genesis` and `c-market` still print nothing of a roster whose `chargeDaa` is 1; `c-market` points to `roster-show` | the same test |

## Review C5's findings after the fixes

| C5 finding | Now | Test |
|---|---|---|
| C5-F1 (High) | closed as found: a verdict takes `min(stake, NO)` (all of it if less than 1 KAS would be left) and pays a tenth of `min(atStake, NO)`. Three contests of 20 KAS take 20, 20 and 15 of seats of 30, 22 and 15 KAS and pay 2, 2 and 1.5; before, they emptied all three. The watcher raises a held seat and a charge against `--me`'s key. What is left is measured below ("C5-F1 in numbers") | `c5_fixed_an_unanswered_contest_takes_no_more_than_its_no_side_had_at_risk`, `auditC5_cli::c5_fixed_a_charged_seat_is_told_to_answer_and_an_unanswered_contest_takes_what_it_staked`, `c6_bounded_a_seat_is_emptied_by_silence_only_with_its_weight_in_aged_stake` |
| C5-F2 (Medium) | closed for the forfeit: a held seat shown two prices keeps a tenth under its holds, the sender is paid nothing, and the charge is settled against what is left (the author: 2 KAS and its bond). Open by another road: C6-F1; weaker than said: C6-F2 | `c5_fixed_a_charged_seat_shown_two_prices_keeps_a_tenth_under_its_holds_and_the_charge_is_paid`, `auditC5_cli::c5_fixed_a_refuted_seat_that_forfeits_itself_keeps_a_tenth_and_the_charge_is_settled` |
| C5-F3 (Low) | stated; unchanged (the numbers of `c5_bounded_a_charge_waits_behind_nine_later_ones_until_one_lapses`; the charges that outlive a partial slash are now cleared, the test does it) | |
| C5-F4 (Low) | closed: one rule (beyond the market's `g` of the feed at the tick's DAA score), printed by `charge`, `charge-contest` and `charge-answer` | `auditC5_cli::c5_bounded_the_charge_commands_at_their_edges`, `auditC6_cli` |
| C5-F5 (Low) | closed at `roster-create` and `roster-show`; C6-N2 | `auditC6_cli` |
| C5-N1 | stated in `KpfCharge`'s header; the lengths are now 318 before a roster of 17,374 bytes and an exit of 4,596, and the dressed ticket and exit still fail only their own clean stack | `auditC5_tests` |
| C5-N2 | closed by the coordinator's change; `check` passes | the run above |
| C5-N3 | closed: a contest's storage mass is 89,854 grams, a verdict's 87,765 | `c5_bounded_a_held_stake_keeps_its_hold_wherever_it_goes_and_the_heaviest_shapes` |
| C5-N4 | closed for both (`charge` notes a stake that already had a hold) | `auditC5_cli` |

## C5-F1 in numbers, after the fix

`c6_bounded_a_seat_is_emptied_by_silence_only_with_its_weight_in_aged_stake`. Seats of 30, 22 and 15 KAS that signed
true ticks are emptied by three unanswered contests of 30, 22 and 20 KAS (the judge's least stake for the last): 6.7
KAS to the author, 60.3 KAS burned, 72 KAS of aged judge stake at risk and returned whole (`pool_settle`), and the
empty roster retaken for 30 KAS as before. A sompi over `w - 1 KAS` on NO takes all of `w`; `w - 1 KAS` leaves exactly
1 KAS (`take_of`). A roster whose seats are left with 1 or 2 KAS is no cheaper to retake than an empty one: `roll`
needs `minTotal`, a join needs `minSeat`, and three joins of 10 KAS are 71 % of a full table of 1 KAS seats.

On the testnet defaults a seat of 5,000 KAS is emptied by one stake of 5,000 KAS aged 7 days; the author gains 500
KAS; answered by 6,250 KAS on YES it loses 5,010. The bet pays when more than 90.9 % of contests go unanswered (91.7 %
for a seat of 100 KAS); in review C5 it was 18 %. The coordinator's claim holds with two precisions:

- "Each stake usable once per `ageDaa`": yes. A winner's receipt is paid as a new free UTXO (`KpfStake.settle`), and
  `open` and `deposit` read the UTXO's own DAA score.
- NO cannot be inflated. It is the opener's UTXO plus what joins while NO trails; every sompi of it is the author's
  own aged stake and is lost to an answer of `mu` times it. Answering one's own contest on YES and retaking the lead
  only adds stake to both sides and burns a tenth of the YES.

What stays is what N§0.3 and N§5a now state: an honest seat must be answered for, within a round, by stake aged
before the charge, 1.25 times the contest; whoever holds the larger aged stake under a judge block wins every dispute
of it. On a block where the author's stake is the only aged one (any new roster's, for `ageDaa`), silence is certain
and the bet is not a bet. I do not count this as a finding: it is the assumption, said.

`clock` is read as a NO total only in a refuted charge, which only `decide` writes; a contested charge's P is refused
by `slash` (`c5_holds_a_charge_leaves_only_with_its_own_stake`), a verdict written with any other total than the
dispute's is refused (`c5_holds_the_judge_serves_a_charge_as_it_served_a_claim`: one sompi more or less, twice, the
old P), `take_of` refuses a total of zero or less, and a lapsed or upheld charge's total is read by nothing.

## C6-F1 (Medium): a stake taken whole still leaves its other charges behind

**Affected.** `KpfRoster.slash` (`s == w`: `withHold(i, zero32())`), `KpfExit.slash` (`s == amount`: no output of the
id), `KpfCharge.settle`; N§5a "Left behind", which states it and not who can cause it.

**The missing rule.** The fix of C5-F2 keeps the holds through a forfeit and through a partial slash. A slash that
takes everything drops them, and after the cap "everything" is what a contest of the stake's own size takes.

**The attack** (`c6_exploit_a_seat_slashed_whole_...`). Seat 1 (22 KAS) signed 20 for a feed of 10.

1. The accuser charges it (5 KAS, at 6,600); a judge's stake of 20 KAS contests; nobody answers; refuted.
2. The seat charges itself for the same tick (5 KAS, `payKey` its own, at 6,601) and contests with its own aged
   stake of 22 KAS. Nobody answers a NO on a false tick; refuted. The stake comes back whole.
3. The seat's slash lands first: all 22 KAS, 2.2 KAS and its bond to itself, 19.8 burned, the seat empty, its holds
   zero.
4. The accuser's slash (good a block before: 20 KAS taken, 2 KAS and the bond): refused against the roster as it is,
   with its witness and with none; the library and `charge-settle` refuse likewise. Its 5 KAS stay in the charge.

Had the seat's contest been 20 KAS, 2 KAS and the accuser's hold would be left and the accuser paid (asserted on the
model). Without any intent: two authors charge one seat, the first verdict is of the seat's size, the second author's
bond is locked. `charge` notes a stake that already has a hold; it does not say this.

**Smallest fix, and what it could break.** `takeOf` leaves the 1 KAS floor while the stake has other holds (`rest`
not zero), and takes nothing from a stake already at the floor; the last charge takes the floor. Every charge then
has a stake to settle against and its bond comes back. It could break: a stake at the floor under many charges pays
their authors nothing but the bond, and the last 1 KAS waits for the last charge; an exit needs the same in
`contWith`.

## C6-F2 (Low): one proof, replayed, leaves 1 KAS

`forfeit` of a held stake keeps `max(w / 10, 1 KAS)` and asks for `w` more than that; the two signatures are as good
the second time. 22 KAS, then 2.2, then 1, then refused (the builder and the engine). The sender is paid nothing, so
only spite replays, and the seat loses nothing a slash of its size would not take. Who loses is the charge's author:
its tenth is `min(atStake, NO) / 10` and no more than what is taken, 1 KAS here instead of 2, and nothing is burned.
A charge made after a forfeit records what was left (`atStake` 1 KAS). Either the note says "down to 1 KAS, by
anyone", or the held forfeit is made to run once (it has no state to know; a floor of `atStake / 10` would need the
charge as an input).

## C6-F3 (Low): whoever contests first sets what the verdict takes

`KpfStake.deposit` joins the trailing side only, so the NO total of an unanswered contest is one UTXO. Seat 0 (30
KAS) lied and is charged; its own stake of 20 KAS contests before the author's 40 KAS; the author's deposit on NO is
refused (the builder; the engine, on the side); the verdict is the true one and takes 20 KAS, 2 to the author; the
seat keeps 10 KAS and its own judge stake. The author cannot chain the contest to its charge (the contested state
holds the DAA score the charge was accepted at), so the first block after a charge is a race. On the testnet
defaults a seat of 5,000 KAS loses 100 KAS a charge: fifty charges (their bonds come back, with 10 KAS each) against
fifty aged stakes of the seat's own. Nothing forbids charging one tick again, so the stake is taken in the end, a
round at a time or at once. A rule that only a stake signed for by `payKey` contests during the first part of
`chargeDaa` would give the author the choice of size; it could break nothing I see but the seat's own early answer,
which is not an answer (an opener is on NO).

## C6-F4 (Low): the answer's tools

- `charge-answer --side yes` with 20 KAS against 20 KAS on NO prints "after the answer: NO leads, YES 20 KAS, NO 20
  KAS" and builds. The bar is 25. Unless more follows, that stake is the loser's: a tenth burned, the rest the
  contester's. The command should say what is missing to take the lead, and refuse or warn.
- `c-watch --me` on a held exit of that key prints "HELD for a charge" and no ACT. A seat that left with a hold is as
  much at risk as one that stayed.

`charge-answer` itself holds: the dispute of another charge, a pending charge, no stake shown, a roster that does not
hold the charge, the charge shown against another seat's holds, and the roster after the seat left are refused; the
exit the seat became is accepted; a second answer on the leading side is refused ("only the trailing side
deposits"); `--me` as an address raises the charge.

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| a | A sompi of the burn to the author; a sompi more out of the roster, on a whole and on a partial slash | refused | `c5_fixed_an_unanswered_contest_...` |
| a | A charge born with another `atStake` than the seat's weight (less, more) | refused | `c5_holds_only_a_charge_its_stake_wrote_is_among_its_holds` |
| a | The verdict written with another NO total; NO zero or negative; a contested charge's P read as a total | refused | `c5_holds_the_judge_serves_...`, `c6_bounded_a_seat_is_emptied_...`, `c5_holds_a_charge_leaves_...` |
| a | One stake, several contests in a row | by reading: a paid receipt is a new UTXO; its age restarts | |
| b | A seat left with 1 KAS: a slash of 29 KAS that empties a seat of 30 is refused; the seat leaves (an exit of 1 KAS: 40,978 grams of storage mass), the exit is withdrawn, the seat is topped up, a roll counts it | not stuck | `c6_holds_a_seat_left_with_one_kas_leaves_and_is_withdrawn` |
| b | A seat of 1 KAS in a full table outbid | by reading: it is the least, and a bid is at least `minSeat` | |
| b | A second and a ninth charge after a partial slash: `rest` kept, cleared afterwards | holds | `c5_bounded_a_charge_waits_behind_nine_later_ones_until_one_lapses` |
| b | An exit slashed in part: its continuation a sompi short, without its other hold, with the hold it was rid of, with a sompi more or less of amount, released, with another key; the author a sompi more | refused | `c6_holds_an_exit_slashed_in_part_continues_with_what_is_left_and_its_other_holds` |
| b | The second slash of that exit (10 KAS, 2 to the author, 8 burned, nothing of the id left); the withdraw of what a slash left; a held exit shown two prices (3 of 30 KAS kept under its hold, 27 burned) | as the note says | the same test |
| b | The reward above what is taken; a burn below the dust floor | the reward is at most what is taken; then nothing is burned and at most 0.02 KAS is nobody's | `c6_bounded_a_held_forfeit_replays_...`, by reading |
| c | A held forfeit: a sompi less burned; the holds rewritten; a held seat of exactly 1 KAS | refused | `c5_fixed_a_charged_seat_shown_two_prices_...`, `c6_bounded_a_held_forfeit_replays_...` |
| c | A held forfeit in the seat's favour | never: it burns nine tenths now, and a slash takes at most as much | by reading |
| e | `fold` and zero ids (a witness whose later id stands in the last place: accepted, the same holds); the stake naming a key input as its charge | accepted; refused | `c6_holds_what_review_c5_left_to_reading` |
| e | A pending charge contested in the transaction that clears it | refused by the roster: it wants the bond burned at the output the charge wants its continuation at | the same test |
| e | A roster and an exit of one id in one transaction (a clear beside a withdraw; a seat and an exit charged at once, one output two stakes' charge) | refused by both | the same test |
| f | The new lines, each asked which output or value no `require` pins: the held forfeit (the roster keeps its value less what is burned; the burn at the mirror index), the partial slash (the continuation's value, table and holds; `payKey`'s output; the burn), `contWith` (value at least the old less what left, and at least the amount) | nothing found unpinned | by reading, and the edits of the tests above |

## Not checked

- A node: every race named here (the seat's slash against its accuser's, the first contest after a charge).
- A walk over charges was not made.
- `KpfStake` beyond what a charge asks of it; a dispute of several lead changes was computed, not run.
- The outbid of a seat of 1 KAS, and `auditC5_tests`' table of lengths for the new script sizes, are by reading.

## Judgement

This round found one Medium: the loop does not stop here, by one condition in `takeOf`. The cap does what it was
made for. A minimum stake no longer burns a seat; burning one by silence now takes aged stake of the seat's size and
pays a bet only above nine silences in ten; the seat is told. The held forfeit closes the road C5-F2 was found on.
What is left of C5-F2 is the same sentence in another place: a stake that is taken whole forgets its other charges,
and the cap made "whole" the ordinary case of a good verdict. C6-F2 and C6-F3 are the two things the new rules made
possible and the note does not say: a proof replays, and the first contest sets the size.

## Status after the fixes (review C7)

| Finding | Now |
|---|---|
| C6-F1 (Medium) | closed: a slash leaves 1 KAS while the stake has another hold (`c6_fixed_a_seat_slashed_by_its_own_charge_keeps_one_kas_for_its_accusers_charge`; every order of two and three charges on a seat and on an exit: `auditC7_builders`) |
| C6-F2, C6-F3 (Low) | stated in N§5a ("Who is paid is whoever settles first") |
| C6-F4 (Low) | closed: `charge-answer` warns of an answer that does not take the lead; `c-watch --me` raises a held exit of one's key |
| C6-N1, C6-N2 | closed |
