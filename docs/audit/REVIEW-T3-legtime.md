# Review T3: a leg says the time it entered, and under a call the young legs ask nothing (F116), at the engine

27 November 2026. One review round of one change: the working tree against commit 73176f7 (uncommitted), as
`docs/ARCHITECTURE-CLEARING.md` 17.9 lists it under "Built". Read: 17.9, 17.2, the rows N76, N78, N79 of 17.1,
14.3, 14.6; the diff of the six scripts and of `covered.rs`, `reserve.rs`, `thin_build.rs`; whole: `KpfTCStep`,
`KpfTEnterS`, `KpfTCEnd`, `KpfTChange`; the entries of `KpfTChangeSettle`, `KpfTChangeLate`, `KpfTPoint`
(dispute, lapse), `KpfTCallEnd`, `KpfTCallEnter`, `KpfTCEnter`, `KpfTStepPrice`, `KpfTOpenC`, `KpfTJoinC`,
`KpfTPass`, `KpfTShardOpen`, `KpfTEnterOff`; lines of `KpfTCRow`, `KpfTCSplit`, `KpfTAddSpot`, `KpfTMarkNote`.
Run: `auditT1_builders.rs` (the previous reviewer's, brought along: part A) and `auditT3_legtime.rs` (new, 12
tests: part B), every transaction signed and run through `verify::verify`. Nothing in `src`, no contract, no
artifact and no other test or doc was changed. Findings are numbered L1 to L4.

Terms in every example (the tests' world): lateness L = 100, dispute period D = 50, so **T = 2 (L + D) = 300**;
the time an entry writes on its account, before which its user states nothing, is 2 (L + 2 D) = **400** after
the entry's time; tolerance 0.5 %, maintenance 5 %, the keeper's fee 0.1 KAS, a note's least 0.2 KAS.

## Verdict

**F116 as it was found is closed at the engine, at the step.** An owner's own leg that is young at the call asks
nothing by `KpfTCStep`, whatever it gained, and an honest claim beside it is paid whole: run whole (steps, notes,
claims) with a shard of 19.9 KAS, an honest claim of 15 KAS and an owner's long that gained 3,000 KAS: the claim
is paid 15 KAS where the owner's leg entered a block before the call's time, and 0.099 KAS where it entered T
before it. The time is written by `KpfTEnterS` alone and is the statement's; a join, a hand-over, a mark and a
split carry it (each run, and each refused or answered wrong where it is written otherwise); no leg makes its
account unsteppable; an account has its shard's periods; the committee shown apart leaves F80 closed (run in
three forms). The model and the script agree on 180 random accounts with young and old legs, a time below
nothing and the edges, through the engine.

**Open, new with the change or made worse by it:**

- **L1 (Medium).** The rule lives in `KpfTCStep` only. A young leg whose account is not stepped leaves by its
  user's own statement 400 blocks after it entered (F95's path, open by decision), and its note is a claim at
  one share: run, 48.5 KAS claimed by a leg whose step asks nothing. With the owner disputing the call at its
  last block, whoever watches has 250 blocks, not T, and none for a leg that is nearly T old.
- **L4 (Medium; the paper's N79, whose reason does not hold at the engine).** An owner makes a call due with a
  leg of its own at no cost, every honest leg younger than T asks nothing, and when the call ends the shard is
  open again and holds what those legs gained. In the play run, the user's 15 KAS were paid whole by the scripts
  before the change and are the owner's after it. For legs younger than T the change does not close F116: it
  turns a share taken into the whole kept.
- **L2, L3 (Low).** A leg's time is up to a lateness before its landing, and a call's time is up to a lateness
  before its landing at the caller's choice: "within T of the transaction" (17.9) is T less a lateness for a
  caller that says the time it lands at, and an honest user is sure of its gain only T and a lateness after it
  entered.

Not a finding, but what the closure now rests on beside "whoever watches calls within T" (each read, the third
run): the committee answers a disputed price of a call within a dispute period (a call that lapses twice ages
the owner's leg past T); the owner's own statements on its shard are answered, not waited out (a shard takes one
statement at a time); and every statement on an account that does not write its legs as they were is disputed
within a dispute period (run: an entry whose account-to-be says the time 0 is taken by `KpfTEnterS`, answered
wrong at its end, and applied where nobody disputes).

## Findings

### L1 (Medium). A young leg's gain is a claim where it leaves by a statement: the rule is in the step alone

`KpfTCEnd` (a leaving, a liquidation's step), `KpfTCMark` and `KpfTMarkNote` know no time of a leg. Under a call a
statement that landed at or after the call's time is ended by a step, and applied where nobody steps within its
dispute period (F95, "open by decision"). What holds a young leg is only that its user states nothing before
the account's time, 400 after the entry.

*Run* (`t3_finding_l1_...`). Long 10 units from 10, entered at 6,201. A call of the time 6,500 at 12: the leg is
young by one block, its step asks 0 (taken). At 6,601 (not at 6,600: refused by the builder) its user states its
leaving at 15; a step over it asks 0 (taken: the answer). Nobody steps: applied at 6,651, the note asks
4,850,000,000 sompi; applied to the called shard by `KpfTLeft` it is a claim of 48.5 KAS, owed by claims.

*The time whoever watches has.* A call is an object no sooner than D after its time (50) and, where its owner
disputes a price of it in the last block and the answer comes at once, about L + D (150; L + 2 D = 200 where
the answer takes its whole period). The leaving is applied at entry + 2 L + 5 D = entry + 450. For the leg that
made the call due (entry = the call's time): 450 - 200 = 250 blocks to step the owner's account, not T. For a
leg that entered T - 1 before the call: 151 - 200, no block at all: the leaving is applied before the call
exists, and its note waits for the called shard. (Read, not run: the dispute of the call and its timing.)

*Also by reading, not run.* A mark's note is counted as owed by a called shard (`KpfTMarkNote`, line 114 to
119): the same path at the same time (a mark is the user's, at the account's time). A step of a liquidation
(kind 9) is anyone's and is held to no time; it names a leg that gained only where the account is due and the
legs that stay lost nothing, so it needs an account that entered on another leg's gain and then fell: narrow,
not constructed.

*Direction.* Put the rule where a claim is counted and not only where a step writes one: `KpfTLeft` and
`KpfTLeftAll` are shown the note's legs and the shard's moving part (its call's time at 130, the periods at 94
and 103): applied to a called shard, a note one of whose legs is younger than the call's time less T asks
nothing where it asks. The same in `KpfTMarkNote` needs the leg shown. Or close F95 itself.

### L2 (Low). A leg is old T less a lateness after the transaction that put it in the books

The leg's time is the statement's `t0`. `KpfTEnterS` holds `t0` from above (an input that landed at or after
it); from below, where it is stated, only by the account's time and the shard's (an entry of the time 0 is
stated at 6,500 on a new account: run). What holds it is `KpfTChangeSettle` and `KpfTPoint`: landed within a
lateness of its time, or neither applied nor disputed (so it never stands again), only ended (run, each past the
builder, with controls). So `t0` >= landing - L.

*Run* (`t3_b1_an_entrys_time_...`, `t3_b3_a_calls_time_...`). An entry of the time 6,410 that landed at 6,510 is
applied, and the account's leg says 6,410. Under a call of the time 6,710, two hundred blocks after the entry's
landing, it asks its 30 KAS; under one of 6,709, nothing. 17.9's "younger than T at any call that whoever
watches states in time" and "within T of the transaction" are T - L = 200 for a caller that says its landing's
time. A caller that says the earliest time its landing allows (L3) has the whole T back: 6,610 said by a call
that landed at 6,710 leaves the leg young (run).

*Direction.* Say it in 17.9 and 14.7, and have the tool state a call at the earliest time the lateness allows
when a leg near T is in the books (its prices are then that time's); or count T a lateness longer.

### L3 (Low). The caller chooses the call's time within a lateness, and with it which legs are young

A call's time is the statement's `t0`: applied only where it landed at it or within the shard's lateness after
it (run: 6,410 landed at 6,510 applied, the call object says 6,410; landed at 6,511, or of a time a block ahead
of its landing: not applied, not disputed, ended). No caller makes a leg old by the time it says. But a leg that
entered at 6,150 asks its 30 KAS under a call that landed at 6,510 and says 6,500, and nothing under one that
landed in the same block and says 6,410 (run). An honest user is sure of a gain T + L = 400 after it entered,
not 300; the caller that gains by the earlier time is the owner (L4).

*Direction.* Say it beside 14.7's lines. (A step cannot read where the call landed: the call is written again
by every step.)

### L4 (Medium; N79 by decision). An owner brings a call about with its own leg and keeps what young legs gained

17.9: "What a young leg gained is nothing ... An owner does not gain by bringing it about (it loses its shard)."
The second half is the model's (a path ends at a call). No script takes a shard from its owner: when the call
ends the shard is not called, has no row, owes nothing and holds what is left (`call_off`).

*Run* (`t3_finding_l4_...`). A shard of 20 KAS; one user long 5 units from 10 for 100 blocks; the price 13 (the
user gained 15 KAS). Alone the shard is not due (`call_due` false) and the user's leaving is paid 14.35 KAS
whole (model). The owner signs in its own long of 1,000 units from 13: due at once (no gate at an entry). Called
at 13: the user's step asks 0 and pays it 19.7 KAS of its 20; the owner's step asks 0 and gives it its coins
back less 0.3 KAS; both notes applied, nothing owed; the call ends; the shard is open and holds 19.9 KAS. The
same play with the user's leg older than T, which is what the scripts did for every leg before the change: the
note asks 15 KAS and the claim is paid 15 KAS, whole.

So against a leg younger than T (and up to T + L: L3) the owner has, for the fees of an entry, a call and two
steps, the whole of what the leg gained, at a time of its choosing; what the leg lost it is paid as always.
Before the change this play cost the user nothing (the owner's leg gained nothing and diluted nothing).

*Direction.* The earlier review's own alternative for N79 (a young gain as a claim paid after the others, of
what is left) gives the user back what the shard has here; it needs the band taken off such a claim, or an
owner's own young leg stated at the two edges takes most of what is left by its 1 % of 13,000 KAS. As built,
"nothing" is what the owner keeps. For the owner of the design to decide; the number to print is this test's.

## (A) `auditT1_builders.rs`: the 27 tests that failed

All 27 fail by the change as intended; none is a regression. 196 of 196 pass now. No assertion was removed; one
bound was moved (F93, below) and is said. Two more tests that passed were changed so that they still mean what
they meant (last two rows).

| Test | Why it failed | What was done |
|---|---|---|
| `t1b_r53_the_sizes_of_the_seven_scripts_that_changed` | KpfTLeftVoid 1,747 -> 1,955 | number updated |
| `t1b_r54_the_sizes_of_the_calls_scripts_and_what_a_block_takes_...` | KpfTCStep 4,113 -> 4,829; KpfTLeft 2,212 -> 2,220; KpfTLeftAll 3,825 -> 3,873; KpfTLeftVoid | numbers updated; every transaction still taken |
| `t1b_r55_the_sizes_of_the_scripts_that_changed` | KpfTCStep, KpfTLeft, KpfTLeftAll | numbers updated |
| `t1b_r57_the_sizes_of_the_scripts_that_changed` | KpfTEnterS 2,323 -> 2,367 | number updated |
| `t1b_r58_the_sizes_of_the_scripts_that_changed` | KpfTEnterS | number updated |
| `t1b_r59_the_sizes_of_the_scripts_that_changed` | KpfTEnterS, KpfTCStep, KpfTStepPrice 1,825 -> 1,821 | numbers updated |
| `t1b_r60_the_sizes_of_the_scripts_that_changed` | KpfTCStep, KpfTStepPrice | numbers updated |
| `t1b_r62_the_sizes_of_the_scripts_that_changed` | KpfTCStep, KpfTEnterS, KpfTStepPrice | numbers updated |
| `t1b_r64_the_sizes_of_the_scripts_that_changed` | KpfTCStep | number updated |
| `t1b_fixed_f80_a_leg_enters_a_shard_of_its_own_committee_and_no_other` | an entry by hand: the leg did not say the statement's time | helper `enter_by_hand52` writes the leg with the time and the row's head with the committee; test rewritten to the new form. **F80 is closed at the engine**: another committee shown (4 values) refused; the shard's committee shown and a head of another written, refused (and with that other shown); a fabricated row of another committee: not entered, not doubled. Its old cases still pass |
| `t1b_r51_what_the_stating_of_an_entry_takes_and_refuses` | the same helper | passes with the helper; its controls are taken |
| `t1b_fixed_f82_an_entry_writes_the_shards_time_...` | the same helper | passes with the helper |
| `t1b_fixed_f74_a_covered_account_has_nothing_of_a_second_partys_...` | the same helper (its control) | passes with the helper |
| `t1b_fixed_f100_a_covered_accounts_statements_and_an_entry_have_a_floor` | the same helper | passes with the helper |
| `t1b_fixed_f103_an_entry_is_a_lateness_after_the_last_...` | the same helper | passes with the helper |
| `t1b_r53_no_script_takes_a_note_of_the_old_form_...` | the same helper (its control) | passes with the helper |
| `t1b_r55_a_shard_that_owes_and_is_not_called_takes_an_entry_...` | the same helper | passes with the helper |
| `t1b_r54_an_entry_under_a_call_is_ended_undone_...` | the helper; and a marked account made by hand without the time | the marked account shown is `timed_t3(..., T0)` |
| `t1b_r59_an_entry_shows_an_input_that_landed_at_its_time_...` | `KpfTEnterS` has an argument more: the time is argument 10, `f` 13 | indices 12 -> 13 |
| `t1b_r54_a_step_under_a_call_is_the_models_on_random_accounts` | the reviewer's own arithmetic of the step had no young legs and found rows by the committee | arithmetic rewritten to the script (own reading of the eight bytes); legs given times (a third of the accounts all old; else old, young, the two edges, the call's time, after it, nothing, below nothing). 180 steps: model = arithmetic = engine; in 48 the rule decided; a new assertion that no account has more by it |
| `t1b_r54_what_a_step_under_a_call_takes_and_refuses` | legs with a committee where the time is: young by accident | legs entered at 0 (old); five refusals added (the leg shown with another time, a committee, a day, a premium) |
| `t1b_r54_the_whole_way_of_a_call_and_what_the_order_of_claims_does` | the same | accounts' legs entered at 0 |
| `t1b_r56_a_join_before_a_call_sets_a_loss_off_against_a_gain_...` | the same | accounts' legs entered at 0 |
| `t1b_fixed_f112_f113_a_gain_is_set_against_what_is_owed_...` | the same | the long entered at 0 |
| `t1b_finding_f95_under_a_call_a_user_leaves_at_a_later_price_...` | the same | the leg entered at 0. F95 is as it was (open by decision); with a young leg it is L1 |
| `t1b_fixed_f93_whoever_steps_adds_nothing_and_has_the_fee_...` | the legs; and then its bound: whoever steps an account that has no fee for it was at most 1,500,000 sompi the poorer | legs entered at 0. **The bound moved to 1,700,000 and the two numbers are asserted**: the step is 716 bytes more, 143,200 sompi more at the least rate: 1,578,000 where it was 1,434,800 (derived: the old tree was not run). Whoever steps an account that pays the fee has 8,411,600. Said, not hidden: a cost of the change, no finding |
| `t1b_r52_a_note_is_spent_only_where_a_leg_of_it_is_no_leg_of_the_books_...` | the reviewer's own rule of `KpfTLeftVoid` compared 84 bytes of a leg with the row | rule rewritten to the script (name, scale, limit); the notes' legs given random times; "a leg of another committee" is now a leg of the books. 469 legs named: 93 notes spent, 77 kept and applied, the engine as the rule |
| (passed) `t1b_r59_what_a_split_writes_and_the_life_of_the_two_accounts` | its legs were young by accident under its call; it asserts none of the gains it prints | legs entered at 0, the books by the raw legs |
| (passed) `t1b_r60_what_a_user_has_by_marks_against_the_leaving_alone` | the same | legs entered at 0 |

Not changed, and under a call with legs young by accident: `f89`, `f90`, `f97`, `r55_f92`, `r56_a_hand_over`,
`r57_a_bond_above`, `r64_a_step_makes_an_account_owe` (losses, bonds, times and keys: the same young or old).
Only their calls of the step were looked at; they were not read whole and not rewritten. The other 160 tests
that passed before were not read.

## (B) What was run, item by item (`auditT3_legtime.rs`)

**B1, the time's source.** What binds bytes 34 to 66 of each leg after each operation:

| Operation | What binds it | Run or read |
|---|---|---|
| entry (`KpfTEnterS`) | the leg said = `t0` and 24 bytes of nothing; `t0` <= an input's landing; the marked account's hash has it | run (the developer's test; time 0 stated; the edges of a lateness applied and refused) |
| the account an entry becomes | `KpfTCEnter`, only where the end is disputed | run: a false one stated, answered wrong, and applied undisputed |
| opening (`KpfTOpenC`) | the root is of no leg, the count nothing | read |
| join (`KpfTJoinC`) | the two chains opened, the legs appended as bytes | run both ways; young leg written old refused |
| hand-over (`KpfTPass`) | the fixed part is carried whole | run; written with another time refused |
| mark (`KpfTCRow`, kind 13) | the leg's first 103 bytes and the new entry | run: right; three other times answered wrong |
| split (`KpfTCRow`, `KpfTCSplit`) | the chain of the leg alone, opened by the row | run: right; another time answered wrong |
| leaving of some legs (`KpfTCRow`) | the chain of those that stay, each leg whole | read |
| `KpfTEnterOff`, `KpfTCallEnter` | the chain before the leg; the marked account as it was | read |
| `KpfTAddSpot` | direction 7 and the vault's id (a token leg is passed by a step) | read |

How far back: a lateness (L2). How far ahead: not at all (the developer's test; not rerun). A mark and a split
are stated only 400 after the entry (asserted in the model; the builder refuses a block sooner).

**B2, the rule.** Run: the boundary (T before the call old, a block later young); the largest time (young), the
largest below nothing, eight bytes of 0xff, nothing with the sign's bit, a block below nothing (old): each
stepped; a day, a strike and a premium beside the time are read by no step. Every leg young and gained: the user
has its coins less 0.3 KAS, the note asks 0 and holds a note's least, applied (`KpfTLeftAll`) it empties the row
and makes no claim; the step is paid its fee as any. A young loss beyond the coins: the coins pay, 0.2 KAS
staying. Beside a token leg: the account goes on; a young gain asks nothing; a young loss the coins do not pay
is owed whole; an old loss beside a young gain is paid, an old gain beside a young loss set off. **A young gain
is not set against what the account owes** (owes 6 KAS before and after; an old gain pays it off): the rule's
letter, and safe for the shard; said so that it is known. T: the step reads the account's own periods (no
lateness and no dispute period: a leg of a block before the call is old; taken); T as large as fits is taken; a
block more and the engine refuses the step (the control passes); such an account stands under no shard:
`KpfTEnterS` refuses an account of another dispute period (0, 49, 51) or bond rate under the shard's id, with
the shard written as that account would have it (bytes 94 to 111 are inside what the id is of). Not run:
`KpfTOpenC` on such an account (read).

**B3, ways around.** (a) join, hand-over, mark, split: none changes the time (B1). (b) the call's time: within a
lateness before its landing, at the caller's choice (L3); a caller ages no leg; the owner's delay of a call is
its dispute at the last block (read, not run) and matters through L1; an owner's own statement on its shard
keeps other statements out until it is answered (read). (c) no script grows a leg: entry and `KpfTAddSpot`
append one, a mark rewrites the entry price alone, the others move or remove whole legs (read).

**B4, the committee shown apart.** `KpfTEnterS`: in `auditT1_builders.rs` (F80, above). `KpfTStepPrice` on a
covered account (kind 8): the right seed taken; nothing, another seed, the committee's id and the shard's id as
the seed refused; an account of a shard of another seed and the same committee answered with its own seed
(taken) and with this shard's (refused); an account of a shard of another committee: this committee's citation
refused under four seeds. (A citation by a second committee was not made: the hash that fails is the same.)
Rows: a fabricated shard whose row says another committee takes the note of a leg of that market (applied, not
spent as a note of no leg); a fabricated call with the market in two rows at 13 and 20: the step names either
(30 KAS or 100 KAS asked); a row of another scale is no row of the leg. Neither object is written by a script:
one row a market and one committee a shard rest on `KpfTEnterS` and `KpfTShardOpen` (four empty rows: read).

**B5, sizes.**

| Script | Before | After |
|---|---|---|
| KpfTCStep | 4,113 | 4,829 (+716) |
| KpfTEnterS | 2,323 | 2,367 (+44) |
| KpfTLeft | 2,212 | 2,220 (+8) |
| KpfTLeftAll | 3,825 | 3,873 (+48) |
| KpfTLeftVoid | 1,747 | 1,955 (+208) |
| KpfTStepPrice | 1,825 | 1,821 (-4) |

| Transaction | Before (derived, not run) | After (run) |
|---|---|---|
| an entry stated | 4,825 bytes | 4,902 (script +44, the committee +33); compute 9,772 |
| a step, one leg, a note's least | 6,174 | 6,890; compute 9,560, storage 283,526 |
| a step, one leg, the user paid | 6,226 | 6,942; fee 1,388,400 sompi |
| a step, six legs | 6,822 | 7,538; compute 10,868, storage 283,797 |
| a step, six legs over a disputed statement | 6,940 | 7,656; compute 11,706 |

The step's 716 bytes are the six unrolled legs' time and sum (about 119 a leg).

**Not reached, or not done.** No transaction of the old tree was run (every "before" of a transaction is
derived). The owner's dispute of a call and the lapse of a call were read, not run. `KpfTOpenC`, `KpfTAddSpot`,
`KpfTEnterOff`, `KpfTCallEnter` were read for B1, not run for it. A mark's note under a call and a liquidation's
step of a leg that gained (L1) were read, not run. No second committee's citation was built. 17.9's "the two
bounds of a stated price are rounded inward" is decided and not on the list of what is built: `KpfTStepPrice`
and `KpfTShardPrice` round the upper bound up and the lower down as before (read; not reviewed further).
`kpf-cli`, the node and the testnet were not touched.

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, once, after the last change to a test:
63 `test result` lines, **668 passed, 0 failed**, 0 ignored. `auditT1_builders`: 196 passed.
`auditT3_legtime`: 12 passed (two of them, `t3_finding_l1_...` and `t3_finding_l4_...`, pass while the finding
stands).

# Second round: the books' hash

27 November 2026. The working tree against a1cd15a (uncommitted): docs 17.8a, commit 1 of four, "the books'
hash, on today's four rows". Read: the diff of the nine scripts, of `reserve.rs` and `thin_build.rs`; the entries
of `KpfTAddC`, `KpfTOwed.apply`, `KpfTMarkNote.apply`, `KpfTChange`, `KpfTDrawNote`, `KpfTDrawEntry`,
`KpfTJoinC` for what they do with a shard's 32 bytes. Not read: `run/gen/s286_books_sil.py`, 17.8a itself.
Changed: `auditT1_builders.rs`, `auditT3_legtime.rs` (four tests more, `t3r2_...`), this file.

## Verdict

**No rule changed, by what was run; nothing found against the change.** Every entry that shows the rows holds
the 41 bytes to the shard's hash; the scripts' running sum is the model's through entries, notes, a call and its
end, and at the largest books; no shard of the old form and no other object is taken where the rows are shown;
no script compared the 32 bytes with a hash of rows. Three things to know, none a hole (R1 to R3 below).

## (A) The eleven tests

Sizes, in bytes (the first seven by the tests; `KpfTShardOpen` and `KpfTShardEnd` before, by the artifacts of
a1cd15a):

| Script | Before | After |
|---|---|---|
| KpfTShardOpen | 679 | 730 (+51) |
| KpfTEnterS | 2,367 | 2,463 (+96) |
| KpfTLeft | 2,220 | 2,374 (+154) |
| KpfTLeftAll | 3,873 | 4,226 (+353) |
| KpfTLeftVoid | 1,955 | 2,017 (+62) |
| KpfTCallPay | 1,562 | 1,691 (+129) |
| KpfTShardEnd | 3,162 | 3,234 (+72) |
| KpfTShardPrice | 1,772 | 1,810 (+38) |
| KpfTCallEnd | 3,622 | 3,695 (+73) |

| Transaction | Before (run in round one) | After (run) |
|---|---|---|
| an entry stated | 4,902 | 5,040 (the script +96, the 41 bytes and their push +42); compute 10,010 |
| a note of one leg applied (`KpfTLeft`), a claim written | 4,209 | 4,405 (+154, +42) |
| the same, no claim | 4,118 | 4,314 |
| a note of two legs applied (`KpfTLeftAll`) | 5,897 (derived) | 6,292 |
| a claim paid | 3,453 | 3,624 |

The eight sizes tests (seven in `auditT1_builders.rs`, `t3_b5_...`): numbers updated; `t3_b5_...` now holds all
nine. The three others, each **the argument list and nothing else**:

| Test | What moved |
|---|---|
| `t1b_fixed_f80_...` | `KpfTEnterS.state`: the seed 8 -> 9, the committee 9 -> 10. And its last case put the other shard's moving part at 7, now the 41 bytes: 7 -> 8 (it was refused for the width; it is refused by the id again, and the test now asserts what argument 8 is) |
| `t1b_r52_what_the_spending_of_a_note_takes_and_refuses` | `KpfTLeftVoid.void`: the leg named 7 -> 8, the key 8 -> 9, what is asked 9 -> 10, the kind 10 -> 11 |
| `t1b_r59_an_entry_shows_an_input_that_landed_at_its_time_...` | `KpfTEnterS.state`: `f` 13 -> 14 |

**R1 (my own tests; fixed).** Five more tests of `auditT1_builders.rs` PASSED and were wrong: they change an
argument by its index and assert a refusal, and the index now named another argument, so each refusal was of a
wrong width or type and not of what the test says. Brought along, with the refusals in `t1b_r52_...` and the
last case of `t1b_fixed_f80_...` that had the same fault:
`t1b_r46_a_note_that_legs_left_is_applied_once_...` (`KpfTLeftAll`: legs, rows, key, asked, each one more),
`t1b_r46_a_note_of_the_rest_is_paid_once_...` and `t1b_fixed_f96_...` (`KpfTLeft.rest`: moving 5 -> 6, key
6 -> 7, amount 7 -> 8), `t1b_fixed_f76_...` (`KpfTLeftAll`: 6, 7 -> 7, 8),
`t1b_r51_an_entry_that_ended_is_undone_once_...` (`KpfTLeft.apply`: asked 9 -> 10, kind 10 -> 11; it now
asserts what the two arguments are), `t1b_fixed_f94_...` (`KpfTCallPay.claim` and `KpfTLeft.rest`: amount
7 -> 8). 196 of 196 pass. The other uses of an index in that file from round 45 on were looked at one by one:
they are of scripts this change does not touch (`KpfTCStep`, `KpfTCallEnter`, `KpfTDrawNote`, `KpfTDrawEntry`,
`KpfTAddC`, `KpfTPass`, `KpfTJoinC`, `KpfTOpenC`, `KpfTAcctEnd`, `KpfTEnterOff`, `KpfTAddSpot`). A change that
adds an argument in the middle of a list will do this again: a count of failing tests does not show it.

## (B) The attack (`auditT3_legtime.rs`, `t3r2_...`, all at the engine)

**(1) Another `gbook`.** Ten entries, each in its builder's transaction (taken): `KpfTEnterS.state`,
`KpfTLeft.apply`, `KpfTLeftAll.apply`, `KpfTLeftVoid.void`, `KpfTLeft.rest`, `KpfTCallPay.claim`,
`KpfTCallPay.off`, `KpfTShardEnd.answer`, `KpfTShardPrice.answer`, `KpfTCallEnd.answer`. The argument after the
rows is asserted to be the shard's 41 bytes. Shown a sum one more, one less, the largest, below nothing,
nothing; a claims' hash with a bit changed, or of 32 bytes of nothing; another first byte; the 41 bytes of
another shard: **88 transactions, all refused**.

**(3) The old form, and what is no shard.** In each of the ten, the shard of the form of before (fixed part of
the terms and the rows' hash alone; for the three answers, the disputed point of the statement on such a shard),
an account, and a note, in the shard's place: **30 objects, all refused**, each also with six other 41 bytes.
The control (the shard's own hash put in its place the same way) is taken in each.

**(2) The sum kept.** One life (`t3r2_the_sum_of_the_gross_is_kept_...`): a shard of no row; a long of 10 units
into a new row (10), a short of 4 into the row that is there (14), a long of 3 into a second row (17); a note of
a leg of no row spent (17); called by a statement and applied (17: `KpfTChangeSettle` carries the hash); an
account of two legs of ONE row stepped and its note applied by `KpfTLeftAll` (3); an account of one leg, by
`KpfTLeft` (nothing, no row); the claim paid; the call ended. Every transaction taken; every shard returned has
the sum of its rows' gross in its 41 bytes; after each entry and each note the shard written with another sum
(one more, one less, as it was, nothing, twice the leg) is refused.
The ends (`t3r2_the_sum_at_its_ends_...`): four rows at 2^60 - 1 units, the last a unit short: the unit enters,
the sum is 4 (2^60 - 1), under 2^62; a unit more into any row is refused by the row's bound (the builder; not
run past it); a leg leaves at the top. So **the model's saturating sum never saturates on a shard a script
wrote, the script's addition never fails, and there is no reachable state where the two differ.** It cannot go
below nothing either: a note takes off no more than its row has (`rg >= 0`), and the row's gross is in the sum.

**R2 (to know; no hole today).** What holds the sum to the rows is the hash and two steps: an entry adds the
leg's size where it writes the row, a note takes it off. **No script compares the sum it is shown with the rows
it is shown.** Run on objects no script writes: a called shard of no row whose books say 5 has its call ended
and goes on saying 5; a shard whose row has 10 units and whose books say 3 has the note of the 10 applied, and
its books then say 7 below nothing. Unreachable (`KpfTShardOpen` writes a sum of nothing over four empty rows:
read; the three scripts that rewrite a row each rewrite the sum in the same branch: read). It matters for the
commit that reads the sum where `KpfTCallPay.off` reads the rows today: the induction is then the only thing
that says a call ends on empty books; say so in 17.8a, and keep the base (`KpfTShardOpen`) and the three steps
the only writers.
The first of the 41 bytes is hashed as it is shown and written as 0x08 by the three writers; `KpfTShardOpen`
writes 0x08: consistent (read; a first byte of 0x09 is refused in every entry: run).

**(4) The 32 bytes carried unopened.**

| Script | What it does with a shard's 32 bytes | |
|---|---|---|
| `KpfTAddC` | takes the fixed part whole, writes it again | run: R3 |
| `KpfTOwed.apply` | the 32 bytes under the terms, hashed with them, written again (its argument is still named `rowsHash`) | read |
| `KpfTMarkNote.apply` | the same (`rowsHash`); a mark moves no unit, so the books are as they were | read |
| `KpfTJoinC` | the shard's fixed part whole (`sfixed`), the shard written as it is | read |
| `KpfTChange` | the fixed part, and for kinds 2 and up the 122 bytes of the form (terms and 32 bytes) hashed to it; it reads the terms, not the 32 bytes | read |
| `KpfTDrawNote`, `KpfTDrawEntry` | the fixed part whole, in the position of the statement | read |
| `KpfTChangeSettle`, `KpfTChangeLate`, `KpfTPoint` | the position's hash, and the hashes of what it becomes | read |
| `KpfTCallEnter`, `KpfTCStep` | no shard: the account, and the call's body (the rows' names and prices, which the books' hash is not in) | read |

**No script outside the eight computes a hash of rows**, and inside them every `blake2b(rw)` stands in the new
form (a test scans the sources for both: `t3r2_a_script_that_carries_...`). Nothing broke silently.

**R3 (to know; no hole).** A script that carries the 32 bytes cannot tell the old form: `KpfTAddC` adds to an
object that hashes as a shard of the form of before, and writes it in that form (run). No script of this
library writes such a shard and none of the eight opens one: it could be added to and stated on, and no more.

Left as they are, and stale: the head comment `shard fixed = blake2b(terms(90) + push(blake2b(rows)))` in seven
contracts, and the argument name `rowsHash` in two.

**Not done.** The generator was not read or run. `KpfTShardOpen` was not run with another form (it computes the
hash itself: read; its opening is run by the tests of `auditT1_builders.rs`). `KpfTOwed.apply`,
`KpfTMarkNote.apply` and `KpfTJoinC` were not run on an old-form shard (read: the same carrying as `KpfTAddC`).
A draw's whole way was not rerun here (the developer's tests pass). The 160 tests of `auditT1_builders.rs`
before round 45 were not read for indices (no shard exists there).

## (C) The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, once, after the last change to a test:
63 `test result` lines, **674 passed, 0 failed**, 0 ignored (`auditT1_builders` 196, `auditT3_legtime` 16,
`thin_draw` 43).
