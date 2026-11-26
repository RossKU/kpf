# Review T5: a draw and a call as traces, on four rows (17.8b), at the script engine

27 November 2026. Under review: the uncommitted change on `5281d25` (the second of the four commits of 17.8a):
`KpfTShardEnd`, `KpfTCallEnd`, `KpfTShardPrice`, `KpfTCStep`, `KpfTChange`, the new `KpfTShardRow`; `Traced` and
the trace's functions in `reserve.rs`; `call_step_at`; the builders `shard_end(_t)`, `call_end(_t)`,
`shard_row(_t)`, `shard_price(_t)`; `stated-answer` and `shard-draw` in `acmds.rs`.

One round. Written by it: this file; `crates/kpf-protocol/tests/auditT5_trace.rs` (new, ten tests); and the
earlier reviewers' tests brought along (`auditT1_builders.rs`, `auditT3_legtime.rs`,
`kpf-tests/tests/auditT1_tests.rs`, `kpf-cli/tests/auditT1_cli.rs`). No source, script, artifact or other doc
was touched. (`docs/audit/REVIEW-T4-tree.md` gained a "Third round: 17.8c" while this round ran: another
reviewer's, not this one's.)

## Verdict

**The trace is sound in its arithmetic and not yet in its bytes: one High finding.** At the engine the model and
the scripts agree, a point at a time: 347 points of 28 rule's statements (a draw and a call on 14 random shards)
answered right and not wrong; 747 lies each answered wrong at the point the model names and under a dispute of
another point; 360 draws and 344 calls on wild books (rows to the ends of 64 bits) each answered, at the end
and at every row, as the script's arithmetic written out says. **Every point of a right statement is
answerable**: 0, the four rows (a row with no market by its sums), each price with its citation, and every point
beyond the list up to the largest number with nothing cited; the only inputs found on which an answer FAILS are
those 17.8b names (books beyond 64 bits; an entry no row is of; a market nobody signs), each at the stater's
cost. **No wrong statement was found that is answered right at every point.** But **a call that is RIGHT at every
point can be written so that its object is one `KpfTCStep` refuses** (R1): the day of an entry written as nothing
below zero is nothing to the two answers, which read a number, and is not eight zero bytes to the step, which
compares bytes. Such a call is applied and nobody disputes it away; the accounts of that market are stepped by
nobody; the call does not end. It is one line in each of two scripts. The rest is Low.

## Findings

**R1 (High; RUN: `t5_r1_a_call_whose_list_writes_a_day_of_nothing_below_zero_...`).** *The day of a price is read
as a number by the answers and as bytes by the step.* `KpfTShardRow` (`int(en.slice(40, 48)) != 0`) and
`KpfTShardPrice` (the same test, and the day as the fifth number of the order) read the day as a number, and the
engine reads `00 00 00 00 00 00 00 80` as nothing (F70 of review 45, again). `KpfTCStep` requires
`cb.slice(56 * r + 40, 56 * r + 48) == byte[](0 as byte[8])`. `KpfTCallEnd` cuts the object's list out of what is
said, byte for byte.
*Scenario, run.* A shard of 20 KAS, short 10 units of market 0 and 3 of market 1 from 10; anyone states the call
at 13 with the trace the rule makes, the byte at 47 of market 0's entry made `0x80` (296 bytes; the same
numbers). Points 0 to 8: each answered right, none wrong (the end, four rows, both prices with the committee's
citations, two beyond). Applied by `KpfTChangeSettle`: the shard is called, the object holds those 112 bytes. An
account long 3 units of market 1 is stepped under it (the control: same object, same patching). An account long 10
units of market 0 is refused, and no place or argument makes it taken: the entry is the only one of its market.
*What it does.* The shard is called and takes no second call. Accounts of market 0 leave only by their own
statement (F95's way): a user that gained leaves and holds a claim; an account that lost (the owner's own, or
anyone who prefers not to pay) is stepped by nobody and stays. No claim is paid while a row has a unit, and the
call ends only on empty books: the winners are not paid and the shard stays called for as long as one such
account stands. It costs the caller a bond it has back. Whoever watches cannot remove it: every point is right,
and a dispute loses its bond. An honest tool never writes it (`sm8(0)`); and `Traced::read` reads it as the rule's
trace and writes it back as other bytes, so `draw_open` refuses to answer any point of it for whoever uses the
builders (the test puts the bytes in after).
*Direction.* The row's answer and the price's answer hold the day's eight bytes to eight zero bytes, as the step
does (and as F70 was closed): `en.slice(40, 48) != byte[](0 as byte[8])` is `ok = 0`. Then the row is answered
wrong and the call ends at its caller's cost. In the model, `Traced::read` gives `None` (or the trace keeps the
bytes) where what is said is not what `said()` writes back, so that the tool sees what the scripts will be
shown. Checked for the other fields: the limit and the price are above nothing (one way to write each in eight
bytes); the market is compared as bytes in the row's answer and in the step; the counts, the places and the sums
are read by answers only. The day is the one field that is nothing and goes into an object.

**R2 (Low; READ, its parts RUN).** *Under a false call nobody disputed, an account is stepped only if the liar
listed its market.* Before, `KpfTCallEnd` wrote the object's four heads from the rows it was shown: whatever the
prices said, every account of the books had its row in the object, and T25 keeps `p >= 0` in the step so that
"an account of a false call" is still stepped. Now the object is the list as said. A call whose list leaves a
row's market out (the row written as empty: stated, and answered wrong at that row: B1, "a row that has a market
written as one that has none", "no list"), or writes it with another limit, is applied where nobody disputes
it, and the accounts of that market are stepped by nobody (`t1b_r54_what_a_step_under_a_call_takes_and_refuses`:
"a call that has no row of a leg's market"). It needs a false call that stands, under which worse is done
(any price). *Direction.* Say it beside T25 in 17.8b; nothing to build unless the end is to hold that every row
is named, which T24 puts with the tree.

**R3 (Low; RUN: `t5_b2_the_end_reads_no_row_and_a_sum_at_the_end_of_64_bits_...`).** *The model of a row is not
the script where a sum said is at the end of 64 bits.* `KpfTShardRow` computes `psn + w` before it looks whether
the row is short, and compares all three sums whatever the first gave. `trace_row_judged` subtracts at once and
stops at the first sum that differs. A draw on a shard long 10 units of market 0 and short 60 of market 1: row
1's signed worth said as the largest number, the rows after following: the engine takes no answer of row 2, the
model says `Some(true)`. With row 1's worth of the gross the largest number and row 2's net a sompi off: the
engine takes none, the model says `Some(false)`. Nothing is gained (row 1 is answered wrong, whatever was
disputed, and `trace_wrong_point` names it); the tool builds an answer the engine refuses, as with F71.
On the rule's traces and on 360 + 344 wild books with sums the builder or the reviewer wrote the two agree at
every point. *Direction.* The model in the script's order: `prev.sn.checked_add(w)?` first, then the three
comparisons without `&&`.

**R4 (Low; RUN in `t3_b4_...`, its consequence READ).** *The call's object says no scale.* `KpfTCStep` held a
leg's 18 bytes of scale and limit to its row's head in the call; the list has the name, the limit, a day and a
price. A call whose row says another scale is the same object, and the step is taken where review T3's test had
it refused (the test now asserts that, and that another limit is refused). T20 called it harmless: one row a
name (`KpfTEnterS`, read again: a head that is not the leg's is taken only for an empty row and no row of the
name), and a note of a leg of no row is void (`KpfTLeftVoid`). That reasoning holds as read; what changed is that
an account with a leg of the market's name and another scale (made only by a statement nobody disputed) was
stepped by nobody and is now stepped at the list's price with its own scale. Also to know: the two ends left
the scripts that cannot open a shard of the old form (they carry the books' 32 bytes;
`t5_the_two_ends_hold_the_32_bytes_they_are_shown_to_the_statements_shard`).

**R5 (Low; measured).** *Two transactions are heavier than 17.8b says.* `KpfTShardPrice` is 2,697 bytes (1,810):
887 more where the paper counted about 270, because it is still shown the rows and finds the committee in them.
A price answered is 9,451 to 9,563 bytes (8,226; estimated 8,390 to 8,560). `KpfTCStep` is 5,056 (4,829): it
kept the hash of the leg's market (the deviation) and gained the day's and the length's tests; a step of one leg
is 6,912 to 7,024 bytes (6,890; estimated 6,520 to 6,750, lighter) and of six 7,508 to 7,620 (7,538). A call
stated is 3,423 to 3,535 (2,913; estimated 3,150 to 3,350). The ends, a row and a draw stated are within the
estimates. *Direction.* Write the measured numbers in 17.8a, 2; the price's answer loses about 600 bytes when the
shard's id is opened by its seed as the paper had it.

**R6 (Low; READ).** *What still says 40 bytes.* The headers of `KpfTShardEnd`, `KpfTCallEnd`, `KpfTShardPrice` and
the new `KpfTShardRow` ("40 bytes, an amount and a price a row", "computes it over the four rows", "point r, 1..4
the price of row r"); `KpfTShardRow`'s line of its entry's arguments (no `gbook`, no `kind`); `KpfTChange` line
183; `reserve.rs` 594, 758, 911; and the help of `stated-answer` (`acmds.rs` 593: "on a draw the point r of a row
with a market": a price is the point 4 + its place now, and `--at 1` to `--at 4` answer rows). And `stated-answer`
with `--at 0` on a call builds `shard_end_t`, which refuses a call: the command answers a call's rows and prices
and not its end (it was so before; no command states a call). The CLI was not run on a draw in this round.

## (A) The earlier reviewers' tests

19 uses of `Draw::said()`/`Draw::arg()` in `auditT1_builders.rs` and 2 in `auditT3_legtime.rs` did not compile.
They now say the trace on the statement's shard (`dsaid`, `csaid`, `trace_t5`: the rule's trace, or, where a
sum is beyond 64 bits and nobody states one, the sums as far as a number goes).

| Test | Verdict |
|---|---|
| `auditT1_builders`: helpers `draw_end_taken45`, `call_end_taken54`, `draw_end_at52` (used by about 20 tests: F71, R45 rounding, R51 time, F82, R54, F87 ...) | expected, updated: they answered the END, which computed the rule over the rows. They now answer the end and the four rows (`points_taken_t5`) and give one verdict: right where all five are answered right, wrong where one is answered wrong. Every test that used them passes unchanged |
| `t1b_r45_the_end_of_a_draw_is_the_models_on_random_books` | expected, updated and extended: the engine against the script's arithmetic written out (the reviewer's own, `row_as_the_script_t5`, `draw_end_of_trace_as_the_script_t5`) at each of five points of 360 draws, and against the model's functions (0 differ). Against the old end over the rows: 29 right, all five right; 180 wrong, of which 178 have a point answered wrong and 2 (books with a row beyond 64 bits, an amount below nothing) have none and a point nobody answers (the end computes its requirement of the sums said whatever it found: it was answered wrong, it now lapses; at the stater's cost either way); 151 of no verdict, never all right, 88 of them now answered wrong at a point that can be computed |
| `t1b_r54_the_end_of_a_call_is_the_models_at_the_edges_and_on_random_shards` | expected, updated and extended the same way ("a price above its limit" is wrong at its ROW, right at its end) |
| `t1b_fixed_f69_a_draw_whose_price_nobody_answers_costs_its_claimer_the_bond` | **was passing for the wrong reason**: it disputed the point 2 and asked `shard_price` for the point 2, which the builder now refuses for any citation (a point below 5), while the point 2 is the row's and is answered RIGHT. Updated: the price is the point 4 + its place; the engine is run on the transaction with the citation of another market (argument 17 made the point); the row's own point answered right. The lapse at the claimer's cost stands |
| `t1b_r46_a_disputer_of_an_honest_draws_row_loses_its_bond_not_the_owners` | expected, updated: the price of row 1 is the point 5 |
| `t1b_r54_what_a_step_under_a_call_takes_and_refuses` | expected, updated: the body is the list (the price at 48 of the leg's entry, where it indexed byte 336); **the places were written as rows 0 and 1 and are the list's**: they are now said about the control's own (`step.at`), with a place far beyond and five other lengths of the body added. Arguments 5, 7 and 8 of `KpfTCStep.step` are where they were (checked by their content) |
| `t1b_fixed_f93_whoever_steps_adds_nothing_...` | expected, updated: a step is 82 bytes more (7,024 from 6,942) and leaves its maker 16,400 sompi less |
| `t1b_fixed_f78_...`, `t1b_r53_the_sizes_...`, `t1b_r54_the_sizes_...`, `t1b_r55_`, `r56_`, `r57_`, `r58_`, `r59_`, `r60_`, `r61_`, `r62_`, `r64_` (the sizes) | expected, updated: 71 scripts; `KpfTChange` 1,760, `KpfTCStep` 5,056, `KpfTCallEnd` 1,853, `KpfTShardPrice` 2,697 |
| `auditT3_legtime`: `t3r2_every_script_that_shows_the_rows_holds_the_41_bytes_...` | expected, updated: the two ends show no row and left the list; `KpfTShardRow.answer` is in it twice (a draw, a call): nine entries of seven scripts, ten transactions, the 41 bytes argument 5 of each answer (checked by content). New beside it: `t5_the_two_ends_hold_the_32_bytes_...` (argument 4 the books' hash; four other values refused) |
| `t3r2_a_script_that_carries_the_32_bytes_unopened_cannot_tell_the_old_form` | expected, updated: the scripts that hash rows are seven: without the two ends, with `KpfTShardRow` |
| `t3_b4_a_row_is_found_without_its_committee_and_what_holds_one_row_a_market` | **finding R4**: "a row of another scale is no row of the leg: the step is refused" no longer holds; the test asserts what is so |
| `t3_b5_the_sizes_...` | expected, updated: `KpfTCStep` 5,056; steps 6,972, 7,024 and 7,620 (6,890, 6,942, 7,538); `KpfTShardEnd` 1,747, `KpfTShardPrice` 2,697, `KpfTCallEnd` 1,853 |
| `kpf-tests`: `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` | expected, updated: 71 scripts, 83 entries, 69 `release`. The lint holds of `KpfTShardRow.answer` what it holds of every answer (it goes on by `ends`, the fee and the kept half, the counts tied, `version == 1`, named by `who`, `release` the six lines, the fee read twice). Added: its `ends`, `slotOf`, `p33`, `i9` are `KpfTShardEnd`'s line for line; its entry ties the three places of `who`, rebuilds the position and the point's hash, and between the point's range and the verdict requires nothing but the length of what is said (so does each end) |
| `kpf-cli`: `t1b_r53_the_old_form_of_a_covered_account_is_gone_from_the_command_line` | expected, updated: it counts the script files of a library the command creates: 71, `KpfTShardRow` among them. Nothing of the command line broke |

Argument places checked for every test that names one in the entries that changed: `KpfTShardEnd.answer` and
`KpfTCallEnd.answer` (4 `books`, 5 `moving`, 7 `said`; no earlier test named a place in them); `KpfTShardRow` and
`KpfTShardPrice` (4 rows, 5 the 41 bytes, 8 `said`, 16 `point`, 17 `at`); `KpfTCStep.step` (unchanged: 4 legs, 5
places, 7 the time, 8 the body, 9 the form). The other by-place tests of the files are of scripts this change
does not touch (`KpfTDrawNote.end`, `KpfTCallEnter.end`, `KpfTAddC`, `KpfTJoinC`, `KpfTShardOpen`).

## (B) What was run

All in `auditT5_trace.rs` unless said; the world is `thin_draw.rs`'s with twelve markets.

- **B1** (`t5_b1_...`): 14 random shards (one to four markets in rows taken at random, 17 with an empty row
  before a row that has a market; nets of both signs, nothing and the whole gross; sizes that are not whole
  units; 16 lists not in the order of their bytes, 38 first numbers below nothing), a draw and a call on each.
  The rule's trace: every point 0 to 4 + E, then 5 + E, 6 + E, 99, 2^62 and the largest number: right taken,
  wrong refused (347 points); one price a statement cited 3 % away: wrong taken, right refused. Then 747 lies
  (each number of a row moved, places below nothing and at 2^62 and the largest number, the two counts, an
  entry's limit, day, price and market, the list reordered with the rows following, every row naming the next
  entry, an entry twice, no list, a row written as empty, a false price with the trace the rule makes of it):
  each stated, each wrong at the point the model names (127 at the end, 551 at a row, 69 at a price), right
  refused there, and the same wrong answer taken under a dispute of another point. 21 entries of no row: the
  builder gives no answer (the engine: B3). 14 changes left the trace the rule's (the place of the price of a
  row with no market; an amount a sompi more within the rule). Not in it: rows at the ends of 64 bits (those are
  in `t1b_r45` and `t1b_r54`, 360 draws and 344 calls, a point at a time), and two things changed at once.
- **B2**: sums at the end of 64 bits (R3; the end answered right on sums no row takes, row 1 wrong); a row with
  units written as empty, two rows naming one entry, an entry with another limit, an entry of another market, no
  list with rows that have markets, a price a sompi off with the old sums, places below nothing and huge: all in
  B1, each wrong at its row and never failing. The call's object against the list said: `KpfTCallEnd` is right
  only for the object of the list's bytes (the lies' outputs were written of their own lists; R1 shows it byte
  for byte). An entry no row is of goes into the object and is used by no step (B3, B4). Inputs on which an
  answer fails on a statement `KpfTChange` took, found: a row whose worth or sum is beyond 64 bits (as 17.8b);
  the end where the sums said do not fit its requirement or equity (then a row is wrong or cannot be computed);
  a price whose entry no row is of; a row after a sum said at the end of 64 bits (R3). None on a right statement
  of books within 64 bits.
- **B3** (`t5_b3_an_entry_no_row_is_of_...`, `t5_b3_the_end_answered_right_again_and_again_...`): an entry of no
  row in a call: points 0 to 5 right; its point refused four ways (cited and not, right and wrong); lapsed: the
  shard again and not called, the disputer its bond and half the caller's (3 KAS of a bond of 2), the caller
  nothing, the copy the other half. A point below nothing is disputed by nobody; 7 and the largest number are.
  A market nobody signs: run for a draw in `t1b_fixed_f69` (lapse at the claimer's cost); for a call only the
  entry of no row. The order of events: a draw with a right end and a wrong row 1; three rounds of the end
  disputed and answered right: in each, row 1 is answered wrong under the dispute of the end, and again once the
  draw stands again; the last time to dispute stays 6,650; standing again it is not applied with no age before
  then, and at 6,650 it is applied at once and no dispute is built. `KpfTPoint`, `KpfTChangeSettle` and
  `KpfTChangeDispute` are not changed: READ only, through the builders. The number of points does not matter: a
  dispute of a right point shields nothing because any wrong point answers it, and the bound is the time. The
  race for the object between a watcher's transaction and an accomplice's in the same blocks is not modelled.
- **B4** (`t5_b4_a_step_under_a_list_of_five_...`, `t5_b4_the_whole_way_...`, and the two updated step tests): a
  place beyond the list, far beyond, below nothing, of another market, another limit: refused; a body longer,
  shorter, not of whole entries, empty: refused; an object of five entries: refused; of four with the three
  after the leg's out of order and one twice: taken (the step does not look at the order). A price of nothing:
  the step is taken; a long of 10 units from 10 with 40 KAS lost 100 and pays 39.8, its user nothing. A shard of
  no market: no call is due, and one stated (184 bytes) is wrong at its end, its rows and points beyond right.
  The whole way with the list the other way than the rows (places 1, 1 and 0): stated, applied, two accounts
  stepped (naming the rows' places: refused), notes applied by `KpfTLeftAll` and `KpfTLeft`, the claim of 120 KAS
  paid what the shard holds (100 KAS less the caller's fee), the call ended. Two accounts, not three.
- **B5** (`t5_b5_...`): a draw and a call at 0, 40, 183, 184, 185, 239, 240, 241, 296, 352, 407, 408, 409, 464
  and 520 bytes: stated at 184, 240, 296, 352, 408 and at no other. A take and a leaving of a covered account
  stated as before; with a byte more, and as a draw on an account, refused. `KpfTChange` 1,760 bytes (1,728).
- **B6** (`t5_b6_...`): no script writes a row of options (`KpfTEnterS` writes the futures' name and holds an
  entry's leg to no day, strike or premium: READ). A shard made by hand with a futures row and an options row of
  one market: a draw states 0.4 % below and 0.4 % above the committee's; two entries; every point right, both
  prices with the same citation; one entry named by both rows is wrong at the second row. It was so before (a
  price a row): N55 is not new with this change, and not reachable by a script today.

## Not reached

- The command line on a draw or a call (`shard-draw`, `stated-answer --at 1..4`): read, not run. The existing
  CLI tests pass.
- A market nobody signs under a CALL (T23) with a row of its own; only the draw and the entry of no row.
- `-0` elsewhere than an entry's day (the amount of a call, the counts, places, sums): reasoned harmless, not run.
- Lies of two changes at once; an entry twice in a list of four; token legs under the new object beyond the
  earlier tests, which pass.
- The model's unit test was not extended; nothing was run on a node.

## Sizes

Scripts (bytes; before from the artifacts of `5281d25`):

| | Before | Now |
|---|---|---|
| `KpfTShardEnd` | 3,234 | 1,747 |
| `KpfTCallEnd` | 3,695 | 1,853 |
| `KpfTShardPrice` | 1,810 | 2,697 |
| `KpfTCStep` | 4,829 | 5,056 |
| `KpfTChange` | 1,728 | 1,760 |
| `KpfTShardRow` | none | 2,095 |

Transactions (`t5_the_sizes_...`; "before" as 17.8b and T26 give them, the steps as review T3 measured; the old
form was not run again):

| | Before | Two markets (296 said) | Four markets (408 said) |
|---|---|---|---|
| a draw stated | about 3,200 | 3,382 | 3,494 |
| a call stated | 2,913 | 3,423 | 3,535 |
| a draw's end answered | about 5,050 | 3,407 | 3,519 |
| a call's end answered | 5,402 (5,520 with commit 1) | 3,512 | 3,624 |
| a row answered | none | 4,176 | 4,288 |
| a price answered | 8,226 (8,300) | 9,451 | 9,563 |
| a call applied | 2,736 | 2,735 | 2,735 |
| a call's step, one leg | 6,890 | 6,912 | 7,024 |
| a call's step, six legs | 7,538 | 7,508 | 7,620 |

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, once, at the end: 64 result lines, **687
passed, 0 failed**. Of them `auditT1_builders` 196, `auditT3_legtime` 17 (one new), `auditT5_trace` 10 (new),
`kpf-tests auditT1_tests` 55, `kpf-cli auditT1_cli` 48, `thin_draw` 44. R1, R3 and R4 are asserted as they are
found: the tests pass while the findings are open, and say "R1 is closed" (or R3) where they will fail.

# Second round: R1

27 November 2026, the same day. Under review: the fix of R1 in the working tree (still on `5281d25`):
`KpfTShardRow.answer` and `KpfTShardPrice.answer` hold the day's eight bytes to eight bytes of nothing
(`en.slice(40, 48) != byte[](0 as byte[8])`: `ok = 0`), and `Traced::read` is `None` where what it reads is not
written back the same. Written by this round: this section; in `auditT5_trace.rs` the R1 test turned into the
proof that it is closed, and two tests more (twelve now); the sizes in the three audit files.

## Verdict

**R1 is closed at the engine**, for a call and for a draw. No other field of what is said has two readers that
can disagree. **One thing follows from the fix on the model's side (R7, Low):** the scripts still read every other
number of nothing in either form, and the model now reads no statement that has one, so a wrong statement that
writes one is judged by the scripts and read by no function of the tool.

## R1, closed (RUN: `t5_r1_closed_a_day_of_nothing_below_zero_is_wrong_at_its_row_and_at_its_price_...`)

The call of the first round (20 KAS, short 10 units of market 0 and 3 of market 1, at 13; byte 47 of market 0's
entry `0x80`) and a draw of a KAS on the same books holding 500, with the same byte. `Traced::read` is `None` of
both. The answers are written past the builders: the builder is given the trace of the same numbers (the
reviewer's `loose`), and the bytes said, the object and the statement that stands again are put in after.

| Point | A call | A draw |
|---|---|---|
| stated by `KpfTChange` | yes (the length alone) | yes |
| 0, the end | right taken, wrong refused | the same |
| 1, the row of that market | **wrong taken, right refused** | the same |
| 2, 3, 4 | right taken, wrong refused | the same |
| 5, the price of that entry, nothing cited | **wrong taken, right refused** | the same |
| 5, with its market's citation shown | no answer taken, right or wrong | the same |
| 6, the other price, cited | right taken, wrong refused | the same |
| 7, 8, beyond the list | right taken, wrong refused | the same |
| under a dispute of the end, row 1 answered wrong | taken: the shard again, as it was, not called | taken: the shard again, nothing drawn |
| under a dispute of the end, the price answered wrong, nothing cited | taken: the same | taken: the same |

No answer takes the row or the price as right. The statement does not stand where anyone looks: it is a wrong
statement like any other (not disputed for a period it is applied, as every wrong statement is; and then the
step refuses its object, as it did). The entry after it in the list is still in its order: the order reads the
day as a number, and the entry's own point is wrong by its bytes before any citation.

## The other numbers of nothing (RUN: `t5_r1_the_other_numbers_of_nothing_have_one_way_of_being_read`)

A number of eight bytes has two forms only where it is nothing. For each field of what is said: who reads it,
how, and whether two readers can disagree.

| Field | Read by | Two readers that can disagree | Run |
|---|---|---|---|
| an entry's day | the row's answer and the price's (bytes, since the fix; the price's order also as a number), `KpfTCStep` (bytes) | no longer | above |
| the place of a row's price (`at`), nothing for the row that names the first entry | the row's answer alone (a number); a step is told its places by whoever steps | no | written `00..80` in a call of the rule's: every point right, none wrong; applied; the object is the rule's call's; an account stepped as the builder writes it |
| a row's place in the books, nothing for row 1 | the row's answer alone (a number) | no | the same |
| the amount, nothing for a call | `KpfTCallEnd` alone (a number); the object is cut from byte 16 on | no (only the statement's hash differs) | the same |
| a sum that is nothing (the rows before any worth; a shard whose rows are worth nothing) | the row's answer, the next row's and the end, each as a number | no | the three sums of an empty first row, in a call: every point right; the last row's sums in a draw on a shard of no market: every point right |
| the count E of an empty list | the end alone (a number) | no | the same draw: every point right |
| the count N | the end; it is 4, never nothing | no | |
| an entry's limit, its price | above nothing in a right statement: one form. The limit: a number in both answers, bytes against the leg's in the step. The price: a number in all three | no | the limit with the sign bit: wrong at its row (not right). The price `00..80`: wrong at its row and at its own point with the citation |
| an entry's market | bytes in the row's answer and in the step; four numbers in the price's order | only between two names that differ in the sign of a number of nothing: not of two hashes (T23's twin) | B1: a bit of a name, and the sign of its first number, each wrong at the row |

So the day was the only field that is nothing AND goes into an object a script compares as bytes.

## R7 (Low; RUN: `t5_r7_a_wrong_statement_with_a_number_of_nothing_below_zero_is_judged_by_the_scripts_...`)

*The model reads none of what the scripts still read.* A draw of a KAS on the books of 500 KAS, the worth of the
net a sompi more from row 2 on, and the place in the books of row 1 written `00..80`. It is stated. At the engine
(the answer written past the builders): row 2 is answered wrong and not right; row 1 and the end right. `Traced::
read` and `Draw::read` are `None`; `stated-answer` says "the statement's bytes are not a draw's" (read); every
builder of an answer takes a `Traced`. Whoever watches with the tool cannot name the wrong point (`trace_wrong_
point` has nothing to read) nor answer it, and a dispute of a point chosen blind is answered right by the stater
where it chose badly. Read as the scripts read it, the model names row 2 (asserted with the reviewer's `loose`).
Before the fix it was half so: the model named the point, and the builders refused to answer (`draw_open`, the
hash). *Direction, either:* the model keeps the bytes as said beside the numbers (it judges by the numbers, and
answers with the bytes: only an entry's day must be eight zero bytes, and that is the row's verdict, not a
refusal to read); or, dearer, each answer holds every number it reads to its one form. The test passes while
the model reads nothing of such a statement, and says "R7 is closed" where it will fail.

## R3: the order of `KpfTShardRow` (as the script stands; for the model)

For the row `at`, with `prev` the three sums of the row before (nothing for the first) and `cur` this row's five
numbers. A result fails the script where it is beyond 63 bits and a sign, the least 64-bit number included.

1. `ok = (cur.place == at - 1)`. Nothing here or in 2 and 3 can fail.
2. The books' row has no market: `ok` only where `cur`'s three sums are `prev`'s (as numbers). The verdict.
3. Else `c = cur.at`; `c < 0` or `c >= E`: wrong. Only where `ok` is still 1 is the entry read: wrong where its
   name is not the row's (bytes), its limit not the row's (numbers), its day not eight zero bytes, `p <= 0`,
   `p > pm`, `sc <= 0`. A wrong place (1) so gives `Some(false)` with nothing computed.
4. Only where `ok` is still 1, in this order, every step computed whatever the next will find:
   `net = |net|`, `gross = |gross|`; for a call `q = p * (BPS - g) / BPS` ALWAYS, then, the row long,
   `q = (p * (BPS + g) + BPS - 1) / BPS` and no more than `pm`; `w = (net / sc) * q + (net % sc) * (q / sc) +
   ((net % sc) * (q % sc)) / sc`; `v` the same of the gross.
5. `nsn = psn + w` ALWAYS; then, the row short, `nsn = psn - w`. Then `pwn + w` and the comparison with `cur.wn`;
   then `pwg + v` and the comparison with `cur.wg`, computed whatever the first comparison gave; then `cur.sn`
   against `nsn`. Right where all three are equal.

So `trace_row_judged` is `None` exactly where, having reached 4, one of `|net|`, `|gross|`, the products and sums
of `q`, `w`, `v`, then `prev.sn + w` (whatever the sign), `prev.sn - w` (short), `prev.wn + w`, `prev.wg + v` does
not fit: all computed before any of the three comparisons decides. Today it differs in two places: it subtracts
without adding first, and `&&` skips `prev.wg + v` where the net already differs. The same order written out is
`row_as_the_script_t5` in `auditT1_builders.rs` (the engine agrees with it on 360 draws and 344 calls of wild
books, four rows each). When the model follows it, the two "R3 is closed" assertions of `t5_b2_...` fail, as meant.

## Sizes

`KpfTShardPrice` 2,700 (2,697), `KpfTShardRow` 2,098 (2,095): three bytes each. The other four as reviewed
(`KpfTShardEnd` 1,747, `KpfTCallEnd` 1,853, `KpfTCStep` 5,056, `KpfTChange` 1,760), and every other script whose
size an audit test holds. A row answered is 4,179 bytes at two markets and 4,291 at four (4,176, 4,288); a price
answered 9,454 and 9,566 (9,451, 9,563); the other transactions of the first round's table as they were.

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, once, at the end of this round: 64 result
lines, **689 passed, 0 failed** (687 and the two new tests). `auditT5_trace` 12, `auditT1_builders` 196,
`auditT3_legtime` 17, `kpf-tests auditT1_tests` 55, `kpf-cli auditT1_cli` 48, `thin_draw` 44. (The command was
started in the foreground; the harness moved it behind after three minutes on its own, and nothing else was run
until it ended.) R3, R4 and R7 are asserted as they are found; R1 is asserted closed.
