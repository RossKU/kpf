# KPF review C7 (the fix of C6-F1, and a last sweep of the judged slash)

## Scope and method

Target: `takeOf(w, st, rest)` in `contracts/KpfRoster.sil` and `contracts/KpfExit.sil` (a slash leaves 1 KAS while
the stake has another hold), the models (`take_of(w, no, others)`, `Charge::takes`, `Roster::slash`, `Exit::slash`),
the commands changed for review C6 (`charge-answer`, `c-watch --me`, `seat-forfeit`, `roster-join`, `c-market`), and
`docs/DESIGN-COMMITTEE.md` 5a read against the code.

Files of this round (nothing else was changed; `REVIEW-C6.md` got a status section):

- `crates/kpf-protocol/tests/auditC7_builders.rs`: 3 tests (`c7_holds_`).
- Brought to the fixes: `auditC6_builders.rs` (the `c6_exploit_` became `c6_fixed_`, the old numbers in its comment;
  `take_of`'s third argument), `auditC6_cli.rs` (the two gaps of C6-F4 and the two notes now assert the fix),
  `auditC5_builders.rs` and `auditC5_tests.rs` (the scripts' lengths: 17,417 and 4,639 bytes), `auditC5_cli.rs` (the
  words of `seat-forfeit` and of `roster-join`).

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`), all passed:

- `kpf-protocol`: `auditC7_builders` `ok. 3 passed`; `auditC6_builders` `ok. 7 passed`; `auditC5_builders`
  `ok. 7 passed`; `auditC2_builders`, `auditC3_builders`, `auditC4_builders` `ok. 1 passed` each.
- `kpf-tests`: `auditC5_tests` 2, `auditC1_tests` 22, `auditC1_walk_tests` 2, `auditC2_tests` 12, `auditC3_tests` 3,
  `auditC4_tests` 4.
- `kpf-cli`: `auditC6_cli` 2, `auditC5_cli` 3, `auditC3_cli` 6, `auditC4_cli` 1.
- `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- check`: `pinned artifacts match a fresh compile`.

## Findings

**Nothing above Low, and no new Low: the stopping condition of the loop holds.** Three notes. The Lows that stay are
the three the note states (C5-F3, C6-F2, C6-F3).

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C7-N1 | Note | N§5a says of the floor "every charge gets its bond back". What the code gives is that every charge is settled: a refuted one's bond goes back to its author, an upheld one's to the charged key, a lapsed one's is burned | `c7_holds_every_order_of_settling_the_charges_of_a_seat_settles_them_all` (`check_bond`) |
| C7-N2 | Note | A slash of a stake at the floor that has another hold takes nothing and pays no tenth: it returns the bond and ends the hold (96 of 1,252 settlements in the walk). It is worth its fee to the author only. A seat can keep its last KAS from an accuser this way with a standing charge of its own (a bond for 1 KAS: no gain) | the same test; `c7_holds_the_witness_cannot_say_whether_other_holds_exist` |
| C7-N3 | Note | "In each case the stake is burned all the same" (N§5a, "Who is paid") is true of a seat that contests its own charge with `minStake` only while its accuser goes on charging: each charge takes `minStake` | `auditC6_builders::c6_bounded_whoever_contests_first_sets_what_the_verdict_takes` |

**The question put to this round**: a seat with aged judge stake of its own size charges and contests itself and
takes the tenth before its accuser. I do not rate it above Low; it is a note, and now stated. Of 22 KAS, 21 are
taken by the seat's own verdict, 2.2 go back to it, 18.8 are burned, and the accuser's charge takes the last KAS and
its bond (`c6_fixed_a_seat_slashed_by_its_own_charge_keeps_one_kas_for_its_accusers_charge`). Nobody loses anything
it held without signing: the accuser is whole and 1 KAS up, where it would have had 2. The seat loses nine tenths
instead of all, which is review C4's C4-N1 by a costlier road (aged stake of its own size, a round's wait, and the
risk that its own contest is answered). What it thins is the reward for reporting, which was never what deters.

## Review C6's findings after the fixes

| C6 finding | Now | Test |
|---|---|---|
| C6-F1 (Medium) | closed: a slash leaves 1 KAS while the stake has another hold, so a stake is gone only with its last hold. The step that locked the bond (the seat's own slash emptying the seat) is refused; the seat's own verdict of 22 takes 21, and the accuser's charge takes the last KAS and gets its bond | `c6_fixed_a_seat_slashed_by_its_own_charge_keeps_one_kas_for_its_accusers_charge`, the two walks of `auditC7_builders` |
| C6-F2 (Low) | stated (N§5a, "Who is paid"); `seat-forfeit` says what a held forfeit burns and keeps, also for the replay | `c6_bounded_a_held_forfeit_replays_down_to_one_kas_and_the_author_is_paid_what_is_left`, `auditC6_cli` |
| C6-F3 (Low) | stated; C7-N3 | `c6_bounded_whoever_contests_first_sets_what_the_verdict_takes` |
| C6-F4 (Low) | closed: `charge-answer` warns when the answer does not take the lead ("your side has 20 KAS and needs 25 KAS ... add 5 KAS") and is silent when it does; `c-watch --me` raises a held exit of one's own key (given as an address) and not another's | `auditC6_cli::c6_holds_charge_answer_takes_only_the_dispute_of_a_charge_the_stake_it_is_shown_holds` |
| C6-N1 | closed: "19.8 KAS of its 22 KAS are burned, 2.2 KAS stay under its holds ... the sender of this proof is paid nothing" | `auditC6_cli::c6_fixed_the_commands_say_what_a_held_forfeit_does_and_warn_of_a_roster_that_cannot_slash`, `auditC5_cli` |
| C6-N2 | closed for `roster-join --genesis` and `c-market` (both print the `chargeDaa` warning); `c-check` and `c-order` by reading | the same tests |

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| a | Every order of settling two charges (each refuted with 20, 22 or 40 KAS on NO, upheld or lapsed) and three (22, 40, upheld, lapsed) on a seat of 22 KAS: 434 orders, 1,252 settlements, each built, signed and verified | all accepted; every charge settled and its bond paid where its end says; the roster's value is its start less what was taken; the seat is empty only after its last hold (218 orders), else it keeps at least 1 KAS under no hold (152 orders: exactly 1 KAS) | `c7_holds_every_order_of_settling_the_charges_of_a_seat_settles_them_all` |
| a | The same on an exit of 22 KAS | all accepted; the continuation holds its amount and carries the other holds; gone only with the last hold (218), else its key withdraws the rest (216) | `c7_holds_every_order_of_settling_the_charges_of_an_exit_settles_them_all` |
| a | A stake nobody can take and no key can withdraw | none: a seat left with 1 KAS by a slash whose last hold is then cleared leaves, and its exit is withdrawn | the seat walk's tail; `auditC6_builders::c6_holds_a_seat_left_with_one_kas_leaves_and_is_withdrawn` |
| a | The witness as a choice between "takes all" and "leaves 1 KAS": the only hold shown with something before it, after it, itself before it; either of two holds shown as the only one; the transaction of two holds that takes all 22 KAS | refused: `rest` is `fold(a, later)`, zero only for `a` zero and no later id, and then `held` wants the hold to be the charge's alone | `c7_holds_the_witness_cannot_say_whether_other_holds_exist`, `c6_fixed_...` |
| a | A slash that takes nothing: the bond a sompi short; a sompi out of the stake | refused; the transaction itself is accepted (0 taken, 0 paid, 0 burned, the hold ended) | the same test |
| b | The floor against the held forfeit (a held stake of 1 KAS is not forfeited further), against `minSeat`, the outbid rule and the roll's total (a seat of 1 KAS is topped up, rolled, the least of a full table) | as review C6 found; nothing new | `auditC6_builders` (three tests), by reading |
| b | A standing charge of one's own to keep more than the last KAS | no: the floor is kept only when less than 1 KAS would be left; a verdict below the stake takes its NO total whatever the holds | `take_of` in `auditC6_builders::c6_bounded_a_seat_is_emptied_...`, the walks |
| b | A hold that can never end (so that the last KAS and an exit are locked) | by reading: a pending charge lapses, a contested one is decided by anyone once its round closes, and every decided status has an entry; C5-F3 prices holding by paying | |
| c | The commands of the round | as the table above | `auditC6_cli`, `auditC5_cli` |
| d | The three templates, the changed lines: `takeOf` is the only one; both branches of `slash` pin the continuation's table, holds and value, `payKey`'s output and the burn as before | nothing unpinned | by reading; the C5 and C6 tests edit each output |
| d | N§5a against the code: "nothing is locked any more" | holds: a stake leaves its holds only when it has none (the last slash, an unheld forfeit, a withdraw), and carries them through leave, outbid, held forfeit and partial slash | the walks; `c5_bounded_a_held_stake_keeps_its_hold_wherever_it_goes_and_the_heaviest_shapes` |

## Not checked

- A node: no race was run (the seat's slash against its accuser's, the first contest after a charge).
- The walks write a charge's end into its state as a verdict would; they do not run the disputes. Four or more
  charges on one stake were not driven (three with every order were; the witness of up to eight later ids is review
  C5's test).
- `c-check` and `c-order` printing the charge warnings: by reading.
- The table of lengths of C5-N1 for the scripts as they are now (17,417 and 4,639 bytes): by reading; the dressed
  ticket and exit still fail only their own clean stack (`auditC5_tests`).

## Judgement

This round found nothing above Low and nothing at Low: the stopping condition holds. The fix of C6-F1 is one
condition and it is exact: a stake is taken whole only by its last hold, the witness cannot say otherwise, and in
every order of two and three charges on a seat and on an exit every charge is settled and nothing is left that
neither a charge nor a key can take. What the judged slash still asks of its users is said in the note, and I agree
with its wording but for C7-N1: an honest seat must be answered for within a round by stake aged before the charge;
the larger aged stake of a judge block wins its disputes; who is paid a tenth is whoever settles first; and holding
a stake costs a bond a `chargeDaa` whatever it holds.
