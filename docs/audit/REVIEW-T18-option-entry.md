# Review T18: an option enters and leaves a shard's books (17.8j, B1 to B3), at the engine

Reviewer's file: `crates/kpf-protocol/tests/auditT18_option_entry.rs`, 15 tests, all passing (5 of findings as
they stand, 10 of what was attacked and held). Read against commits `eec08b6`, `04aec33`, `8331217`, `6aa44d0`. A
test named `t18_uN_...` shows the finding UN as it stands (it passes while the finding is open and says so in its
assertion); `t18_held_...` is of what was attacked and held.

## Verdict

**The three scripts that were built do what the model says, to the sompi, on everything tried.** `KpfTEnterSD`
against `entered_d` on 600 random rows, shards and legs (296 taken, 304 refused by the gate, 592 other shards after
refused) and with 124 single changes; the row's gate a sompi either side for each way; `KpfTLeftD` and
`KpfTLeftAllD` against `left_d` on 400 random notes of one to six legs (774 other shards after refused) and with 34
single changes; `KpfTEnterOff` for each way, a sompi either side; the account's side of an entry (52 points, 16
entries a sompi short); a whole life of a series that leaves the shard exactly as it was opened. No script and the
model part in what B built, but for one line of `c_marked` (U5, low).

**What is wrong is around them, and two of the three things are High.**

- **U1 (High)**: `KpfTLeftVoid`, which B did not touch, calls every option leg "no leg of these books". Any note
  with an option leg, the notes B3 is built to apply, is spent by anyone with the shard unchanged: what it asked for
  its user is paid to nobody, and its legs stay in the books for good. The paper says the opposite. The model
  agrees with the script: it is the rule that is wrong.
- **U2 (High)**: `KpfTEnterOff` lets the transaction name the output that returns the premium. Two undone entries
  in one transaction are met by one output: run at the engine, the owner has 20 KAS of 40.
- **U3 (High as the library stands; closes with C and D)**: B1 is in the library and the ways out are not. From an
  option's day on, its account states nothing and no draw or call of its shard has a trace; under a call an account
  with an option leg is stepped by nothing and no claim of the shard is ever paid. One entry of one unit, by the
  owner with an account of its own, takes its shard out of reach of a call from the next block.
- U4 (Low, said in the paper as L4 open, here in numbers) and U5 (Low).

B should not stand in a library that anything of value uses before U1 and U2 are closed and steps C and D are in.

## Findings

### U1 (High): `KpfTLeftVoid` spends every note that has an option leg, whatever the books say

`contracts/KpfTLeftVoid.sil`, `void`: `if (dir != DIR_LONG && dir != DIR_SHORT) { gone = 1; }`. The line was right
while no option leg was in any books. Since B1 an option leg IS in the books, in its dated row, and since B3 its
note is what takes it out. `KpfTLeftVoid` still calls it gone: anyone spends the note, and the shard goes on as it
is (but for coins the note held for it).

Run at the engine, each after the rule's own application of the same note was run and paid:

- A note that a held call left, asking 3 KAS for its user (tag 6): spent by `KpfTLeftVoid`, the shard's state and
  coins as they were, nothing to the user, no note of the rest. The user's claim is gone and the shard's owner is
  the one who gains by it.
- The leg stays in its dated row for good (no other note of it exists): the row is taken out by nothing, the shard
  is asked for it at every draw and call, and under a call no claim is paid while a row has a unit.
- The note that an option did not enter (tag 8, `KpfTEnterOff`'s): voided the same way; a leg that never entered
  stays in the row.
- A note of a long and a call (an account taken whole), asking 7 KAS: voided by naming the option leg; the futures
  leg stays in its row too.
- **Named by its FUTURES leg the same note is void as well.** The script counts "the units of the note's legs of
  the named leg's row" by market, scale and limit alone, so the call's ten units are counted against the futures
  row's ten: "the row has fewer units than the note's legs of it".

No malice is needed: the transaction that voids a note of an option leg is 4,091 bytes and the one that applies it
5,042 (a note of two legs: 14,678), and whoever clears a note is paid the same for either.

The model says what the script says on both branches (`Shard::gone_at`: `book_of` fails for an option leg; the
units are counted by market, scale and limit). "B3, the second part, as it is built" says "A note of an option leg
that is in no row is spent by nothing (`KpfTLeftVoid` reads futures rows)": the script spends every such note, in a
row or not.

Recommendation: both of these, together.
1. In `KpfTLeftVoid` (and `gone_at`): the named leg is a futures leg (`dir` 1 or 2), and the units counted against
   its row are those of the note's FUTURES legs of its market.
2. `KpfTLeftVoidD` (the list of step B has it; it was not built): the named leg is an option leg, shown the leaf of
   its key by its path with fewer units than the note's legs of that key, or shown that the key is in no leaf (the
   leaf that names the key after it). Without it, closing 1 alone leaves a note of an option leg that is in no
   row spent by nothing, and such a note ends every draw (review 51, F81); a wrong statement that nobody disputed
   writes any note (review 53, F86), so "no script writes such a note" does not hold it.

Test: `t18_u1_a_note_with_an_option_leg_is_spent_by_anyone_and_its_legs_stay_in_the_books`.

### U2 (High): `KpfTEnterOff` lets the transaction choose the output that returns the premium; one output serves two

`contracts/KpfTEnterOff.sil`, `off(..., int po)`: `tx.outputs[po]` pays the account's second key at least `back`.
Every other script of the family that pays a key pays it at a place computed from its object's input
(`tx.outputs.length - 1 - 4 * j`), so that two scripts in one transaction cannot be met by one output. `po` is free.

Run at the engine: two entries of a written call, 20 KAS of premium each, into two accounts under two shards of one
owner (two covenant ids), both ended. Undone alone, each returns 20 KAS to the owner. Undone in ONE transaction (a
library of two copies a script: `lib_genesis(2, ..)`; `COPIES_MAX` is 8), both scripts name output 6: the owner has
20 KAS of 40 and the builder's change is 1,019.8 KAS for a funding of 1,000. Nobody signs `KpfTEnterOff`: whoever
undoes the two takes a premium, and the user who wrote the options and had both entries end (a price nobody
answers; 14.3) is the one placed to do it.

Read, not run: the same output can be shared with any other script that pays the owner's key at a place of its own
in the same transaction (the settlement of a draw of another shard of the same owner by `KpfTChangeSettle`; a note
paid to the owner's key as a user), which needs one copy only. With a library of one copy and an owner of one shard
nothing reaches it (an account and its shard have one id, and `KpfTEnterOff` asks that id's inputs to be one).

Recommendation: no `po`; the premium at a place computed from `j` as the twins have it, with
`m >= tx.inputs.length` (a place no other branch of a script that can stand in the same transaction for the same
`j` uses). The builder `enter_off` then uses `d.last(...)`.

Test: `t18_u2_two_entries_undone_in_one_transaction_return_one_premium`.

### U3 (High as the library stands): an option that entered has no way out past its day or under a call

The paper says steps C (a call's step over option legs, the twins for a called shard) and D (the day) are not
built. Asked: does anything reach them. `KpfTEnterSD` does: it is in the library, and a leg it books is from then on
where only C and D can take it.

- **From its day on** (the model, `c_state` and `traced_with`, at the day, a block after, and long after): an
  account with an option leg states nothing: no take, no leaving of its futures leg, no step. Its coins and its
  other legs stand with it, and it is liquidated by nothing. No draw of its shard is traced ("a draw is wrong while
  a row of an ended day has a unit"), and no call: a call's row of an ended day has no answer (`KpfTShardRowD`:
  `require(kind == 5)`; review T17 ran the 13 points with no verdict), so a call disputed at that row lapses.
- **One transaction of the owner's is enough.** `KpfTEnterSD` takes an option whose day is the block after the
  statement's time (run at the engine: day `T0 + 1` taken, `T0` and `T0 - 1` refused; the paper has it so: "the
  script that states takes any day after the time: the row is what is answered wrong"), and the books have the leg
  from the statement. The owner, with an account of its own, enters one unit: from the next block no call of its
  shard is traced, whether the entry is later applied, or ended and its note voided (U1) or left unapplied. A shard
  that cannot be called is a shard whose users cannot be made whole from it.
- **Under a call** stated before the day (the model; the scripts read): `KpfTCStep` asks every leg's way to be 1 or
  2 (a token apart), so an account with an option leg is stepped by nothing; a note of an option leg is applied to
  a called shard by nothing (`KpfTLeftD` and `KpfTLeftAllD` refuse a called shard and `KpfTLeftC` and
  `KpfTLeftAllC` an option leg: all four run at the engine); `KpfTCallPay` asks the sum of the gross to be nothing,
  and the sum has the option's units: no claim of that shard, of any user, is paid.

Recommendation: C and D before B is in a library that holds coins; or, until then, `KpfTEnterSD` out of the
library's list (the scripts and tests stay). Whatever the order: an option's entry should ask its day to be more
than T after the statement's time in the script that states it (one comparison; the account's row then repeats
it), so that the books never have a leg of a day that is past before its entry is settled; and a call's row of an
ended day needs its answer (`KpfTShardRowE`) before any row can have a unit on its day.

Test: `t18_u3_an_option_that_entered_stops_its_account_and_its_shard_at_its_day_and_under_a_call` (the model), and
the day `T0 + 1` and the four scripts of a called shard in the two `..._with_one_thing_changed_is_refused` tests
(the engine).

### U4 (Low; the paper's L4, in numbers): the gate holds each row to all the shard holds

What the gate bounds: the leg's own row, with the leg, at the leg's entry, is asked no more than what the shard
holds less what it owes (or no more than before). To the sompi, at the engine, for each way. For 100 units at 10:
a call the user holds at 9.9, or a put at 10.1, asks the shard 77.5 KAS; a call the user holds at 13, 22.5 (the
rate on the gross alone, 2.25 %); a call the user writes at 9.9, 32.5; at 10.4, 22.5.

What it does not bound: the rows together, the futures rows, or the row at any later price. A shard of 100 KAS
takes, a row at a time, eight rows of 444 calls held at 13.0 to 13.7: 799 KAS asked together, each entry within the
gate (`entered_d`). An owner with an account of its own brings its shard under its lower level this way, at will
(review T3's L4, through an option).

Recommendation (the last phase, with L4): the gate against what the shard holds less what its rows are asked
already needs the other rows' sums, which an entry does not read; a number in the shard's state of what its rows
were asked at their last entries is the cheap form.

Test: `t18_u4_the_gate_holds_each_row_to_all_the_shard_holds`; the numbers are printed by
`t18_held_the_rows_gate_a_sompi_either_side_for_each_way`.

### U5 (Low): `c_marked` takes a written option that adds another amount than its premium

`Account::c_marked` is documented "`None` where the script refuses". Of a written option adding 0, or its premium
a sompi more or less, it gives a marked account; `KpfTEnterSD` refuses (run). Only `c_enter` has the rule, so a
caller that builds from `c_marked` (as `c_enter_state_d_as` allows) is told yes by the model and no by the engine.

Recommendation: the premium's line in `c_marked`.

Test: `t18_u5_the_models_marked_account_of_a_written_option_that_adds_another_amount`.

### Said, not findings

- A premium of less than a note's least stays in the account where the entry is undone (19,999,999 sompi stayed,
  20,000,000 returned: run). The paper says it.
- An account that enters with less than a note's least of its own: undone, the premium returns as far as the coins
  go above a note's least (19.9 of 20 KAS returned to an account that had 0.1), and the keeper's fee is nothing, so
  only the owner has a reason to undo it. Such an entry's end is wrong (it is the user's bond).
- The entry's price: the last row holds it to the leg's own entry and its point to the committee's within the
  tolerance, as a futures leg's (run: 0.4 % either side right, 2 % wrong). What an entry stated the tolerance off
  moves is what was entered (for 100 units in the money, 0.5 % of 10 is 5 KAS between the two signers), as a
  futures entry does: not run in numbers.
- `KpfTEnterS` is not in the diff of the four commits (`git diff --stat 2dc31a6 6aa44d0`): byte for byte what it
  was. It takes no option leg (run: 9 changes).

## Attacked and held

| What | Test | Count |
|---|---|---|
| `KpfTEnterSD` against `entered_d` on random rows (both signs of net, units already entered, the four ways, strikes in, at and out of the money, sizes that are no whole unit, premiums of nothing and large, days 200 thousand to 3 million blocks out, shards that hold nothing, a sompi either side of the gate, and much; shards that owe): the gate written from the paper is the model's verdict and the engine's; the engine's shard is the model's | `t18_held_an_options_entry_is_the_models_on_random_rows_and_the_gate_is_to_the_sompi` | 600 entries: 296 taken (70, 66, 76, 84 by way; 24 by a shard holding less than the row is asked, the row asked no more; 76 held to the sompi), 304 refused |
| The shard after with one thing changed (the root as before, the sum or the shard's number a unit either way, the units the other way, a unit more, the gross or what was entered a unit off, the time a block either way, a sompi more held or owed, a futures row changed) | the same | 592 refused |
| The gate a sompi either side, each way, in and out of the money and at the strike, with and without coins owed, and a shard of nothing | `t18_held_the_rows_gate_a_sompi_either_side_for_each_way` | 9 legs, 45 entries |
| One thing of a valid entry changed, everything else made to agree: a way of 0, 1, 2, -4 for a written call and 7, 8, 9 for a written put; a futures leg whole; the leg's time a block either side of the statement's; a scale, a limit, a strike or a day that is not the row's; a premium below nothing and above the limit (at the limit and of nothing: taken); a written option's premium another than what is added; either party signing twice; the input that shows the time (six); the leaf of another strike with its own path, the leaf at another place, the places -1, 64 and 64 more, a bit of the path, a unit in the leaf that is not there; a called shard; a shard's time a block too near (and at the edge: taken); a day at and before the statement's time (a block after: taken, see U3); `KpfTEnterS` with an option leg | `t18_held_an_options_entry_with_one_thing_changed_is_refused` | 124 |
| The largest size at the market's limit and a row at its limit of units: the engine's verdict is the model's (`c_marked` and `entered_d`) | `t18_held_the_largest_sizes_are_the_models` | 12, none parted |
| `KpfTEnterOff`: a futures leg and each way; what is paid, to whom, the model's; a sompi less in the account, in the note, to the owner; the premium to the user; the output named 0, 1, 2, the change, beyond, below nothing, 1,000; a sompi of the premium kept in the account; a premium a sompi below a note's least and at it; a premium larger than the coins less a note's least | `t18_held_what_an_undone_entry_pays_and_to_whom` | 10 entries undone (6 that return a premium), 80 changes refused |
| `KpfTLeftD` and `KpfTLeftAllD` against `left_d` on random books (three series, two futures rows, legs of other accounts in them; notes of one to six legs in any order; asked above and below nothing; a shard holding nothing, little and much; notes that hold less than they say): the model's shard, and the books exactly without the note's legs | `t18_held_notes_of_option_legs_are_the_models_on_random_books` | 400 notes (63 of one leg, 337 of several, 184 with two option legs of one row; 109 paid, 55 with a note of the rest, 165 that held coins); 774 other shards after refused |
| A note with one thing changed: an option leg's note by `KpfTLeft`, `KpfTLeftAll`, `KpfTLeftC`, `KpfTLeftAllC`; a futures leg's by `KpfTLeftD`; more units than the row has; the second leg of a row shown the row as it was; two legs each shown the other's slot; slots cut short (five); a leaf at another place; the futures leg's row misnamed; a called shard by `KpfTLeftD` and `KpfTLeftAllD` | `t18_held_a_note_of_option_legs_with_one_thing_changed_is_refused` | 34 |
| The account's side: an option of each way entering an empty account, beside a long of its market, a short of another, a leg of its market after one apart, another option, a long and an option; at the least coins the model lets in (found to the sompi): no wrong point by the model; the end and every row right and not wrong at the engine; applied; a sompi short: the end wrong and not right | `t18_held_the_accounts_side_of_an_options_entry` | 24 entries, 52 points, 16 a sompi short |
| The entering leg's price against the committee's (four prices); the last row of an option exactly T after the statement's time and a block after it wrong, a block more than T right, by the model and at the engine | the same | 4 and 3 |
| A whole life at the engine: `KpfTRowIn`; one user holds and one writes the same series (sizes of no whole unit, in the money), a lateness apart (a block less: refused); both leave by their notes; the row has no unit and nothing entered; `KpfTRowOut` of the row and of its day's leaf: the shard as it was opened, with the 3 KAS it was paid less the 2 it paid | `t18_held_a_series_whole_life_leaves_the_shard_as_it_was_opened` | 7 transactions |
| The sizes of "as it is built": an entry stated 7,475 bytes here (7,400 said, of another account); undone with its premium 2,340 (as said); a note of one option leg 5,042 (5,046); of a futures leg and an option leg 14,678 (14,682) | `t18_held_the_models_words_and_the_sizes_printed` | as said |

What an account alone enters with (10 units, premium 0.5 a unit, the price 10; printed): a held call or put, 5.2
KAS (its premium and a note's least); a written call at 10.4, 1.7 KAS of its own beside the 5 the entry adds; a
written put at 9.7, 2.7.

## Not reached

- **The account's statements after a real entry, at the engine**: a take, a step and a split of the account with
  its option leg were asked of the model only (a take is stated before the day; a hand-over, a join and a split of
  the option leg are stated); `KpfTPass`, `KpfTJoinC`, `KpfTCSplit`, `KpfTCEnd` were not run on such an account.
  The life test's two notes are made by hand, not written by a step's settlement.
- **A draw and a call at the engine over rows that a real entry filled**: `traced_with` before and after the day
  (the model); `KpfTShardEnd`, `KpfTShardRowD` and `KpfTChange` were not run by this review on such a shard (review
  T17 ran them on rows written by hand, and the state is of the same form).
- **`KpfTCStep`, `KpfTCallPay`, `KpfTCallEnter`, `KpfTDrawEntry`, `KpfTDrawNote` with an option leg**: read only
  (the two lines U3 rests on are quoted there). The lapse of a call disputed at a row of an ended day was not run.
- **U2's other forms** (with `KpfTChangeSettle`, or a note paid to the owner's key): read only.
- **Token legs and six legs**: no entry of an option beside a token leg, or as a sixth and a seventh leg, was run.
- **`KpfTStepPrice` on the price points of the other rows of an entry** (only the entering leg's own point).
- **`KpfTCRow`'s other changed lines** beyond the last row's time and day (a strike or premium out of range in the
  last row: held by `KpfTEnterSD` at the statement, run there).
- An entry at a price the tolerance off, in numbers at the engine; the sizes of the genesis.

## What was run and what was read

Run, all at the script engine through `verify::verify` unless said: every test of the file (15 passed, 0 failed, in
one run of `cargo test -p kpf-protocol --test auditT18_option_entry`). U1 and U2 are engine results. U3 is the
model's (`c_state`, `traced_with`, `call_step`, `left_d`, `left_class`) with the entry of day `T0 + 1` and the four
note scripts of a called shard at the engine; `KpfTCStep` and `KpfTCallPay` were read. U4 is the model's
(`entered_d`), the gate's sompi at the engine. U5 is the model's, the script's refusal at the engine. No other test
file was run, nothing outside the two files of this review was changed, and no git write was made.
