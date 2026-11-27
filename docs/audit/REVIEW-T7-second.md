# Review T7: the second class of claims and the row's gate, at the engine (17.8c, pieces (b) and (c))

One round, at the script engine, of the uncommitted change on 2e97ec6: `KpfTCStep` (the band, the note of tag 9),
`KpfTLeft` / `KpfTLeftAll` (the class at the applying), `KpfTLeftVoid` (tag 9), `KpfTMarkNote` (the books opened,
the class), `KpfTEnterS` (the gate), and their models. The reviewer's tests: `auditT7_second.rs` (new), and the
earlier reviewers' files brought along. Nothing of the sources, contracts or artifacts was changed.

**Be exact about what was run: each finding and each item of (B) says RUN at the engine, RUN in the model,
COMPUTED or READ; "Not reached" lists what was not done.**

## Verdict

L1 and L4 are **closed as found** and **narrowed, not closed, as plays**; F116 is closed for young legs and is
back in shape for an owner that waits T with its row set off (S1). The worst play found: S1, in the shard of L4:
the owner enters a long and a short of 1,000 units (the gate asks 1,007.47 KAS for the first and nothing for the
second), draws 272.44, takes the short out: a one-sided row of 13,065 KAS behind 734.39 KAS (5.6 % where the gate
asks 7.75 %), a call due at once, and the leg that stays as old as the owner likes. With the price 10 % up before
the call, the owner's old leg is a claim of the FIRST class of 1,436.5: an old user is paid 11.01 of 21.86 and a
young user 0 of 21.14; with no move everybody is whole. **The gate holds** for what it is (one entry, one row,
what is held, the price the entry states): to the sompi, engine = model = arithmetic on 120 random rows; it does
not bound a row that a leaving makes (S1), the four rows together or the equity (S2), or an entry stated at a
false price that nobody disputes (S6). **The first class is not diluted by anything young**: at the applying the
engine takes the rule's class and refuses every other outcome on 108 notes of every combination (574 lies
refused); the first class is diluted only by OLD legs (S1, and 17.9's one share). **Nothing is claimed twice**:
a note is one input and is spent where it is applied; the kind said the other way is refused by the hash; a
claim that says a sompi more than the note, in either class, is refused; no claim says anything paid.

In short:

- **L4 as found (the user has nothing of its 15) is closed at the engine**: the young user's step writes a note of
  tag 9 that says 14.35 KAS (15 less the band of 0.65), it is a claim of the second class, the call does not end
  over it, and it is paid whole. `t3_finding_l4_...` asserted the hole and PASSED after the change for the wrong
  reason (below); it is re-derived.
- **L1 as found is closed at the engine**: the leaving of a young leg stated after the call is a claim of the
  second class, whole, and counted in the first it is refused. What is left of it: finding S3.
- **F116 stays closed for a young leg** (now a claim of the second class of 2,870 KAS, behind the honest claim,
  paid 4.9 of what is left), and for an OLD leg it is as 17.9 says (one share): with the gate the play costs the
  owner the row's requirement paid in (778.875 KAS for 1,000 units at 10), and the honest claim of 15 beside an
  old leg that gained 3,000 is paid 3.87 where it was 0.099.
- **The gate holds to the sompi for one entry into one row** (engine = model), and it is a gate of one row, of
  what is held (not of the equity), at the leg's own entry: findings S1, S2, S5 say what that leaves.

## Findings

(Numbered as found. RUN at the engine / RUN in the model / READ is said for each.)

### S4 (Medium, a hole of the TESTS, not of the scripts): a step's note applied as an account's own passes every test that does not hold the output to the note

RUN at the engine. The builders make the input of a note from the hash the test gives them. Twelve places in the
earlier reviewers' files stepped an account (`call_step`), took output 2 of the step's transaction as "the note"
(`nth(&sg, 2, step.note.clone(), ..)`) and applied it with `left_apply` (tag 6). After the change the step's
output is the hash of tag 9: the object those tests applied is one no script wrote, and the engine took it,
because the outcome of a tag 6 note of the same legs and amount is the same where the young gained nothing.
`t3_finding_l4_...` and `t3_f116_...` both PASSED on the changed scripts this way, the first still asserting "the
shard is open, holds 19.9 KAS and owes nothing: the user has nothing of its 15". Fixed in the tests: the helper
`step_apply_t7` (in `auditT1_builders.rs` and `auditT3_legtime.rs`) holds output 2 to `CallStep::note_hash()`,
applies with tag 9 and `step.young`, and runs the same transaction with the kind said 6: refused (the hash is
another: confirmed at the engine wherever the helper runs). The author's own test has the same weakness in one
line: `thin_draw.rs`, "a step's note shown as an account's own" is built with `left_apply_as_c(.., NOTE_LEFT, ..)`,
which makes the INPUT a tag 6 object: it is refused for its outcome (the claim), not for its hash.
Direction: a test that applies what a transaction wrote should take the output's script from the transaction
(`nth` could check it against the state's hash).

### S5 (Low): the gate's arithmetic makes a row full by its worth at the entering leg's entry

RUN at the engine and in the model (`t3r2_the_sum_at_its_ends_...`). `KpfTEnterS` computes the worth of the row's
gross units, with the leg, at the leg's own entry. A row whose gross is worth more than 64 bits of sompi there
(the largest gross at 10 KAS a unit: 115 thousand million KAS) takes no leg at that entry, whatever the shard
holds and although the leg sets the row off; the same unit at a sompi a unit enters. Before the gate it entered
(the earlier test entered it). No shard is near it. Said because the bound of a row is now a function of a price
a user states. Direction: say it beside `MARGIN_MAX`, or compare the two requirements without the worth where
the leg lowers the net.

### S1 (Medium): a leg that leaves raises its row's requirement with no gate; the leg that stays is as old as its owner likes, and old it is of the FIRST class

RUN at the engine (`t7_b1_ii_a_pair_entered_and_one_side_leaving_leaves_a_row_the_gate_never_saw`; the draw in the
model). The shard of L4: 20 KAS, a user long 5 units from 10, the price 13.

1. The owner's long of 1,000 units at the lower edge (12.935) enters only where the shard holds 1,007.4748 KAS
   (7.75 % of the row with it), to the sompi: the first arm. It pays 987.47 in.
2. Its short of 1,000 units at the upper edge (13.065), from a second account of its own, enters a shard that
   holds 1 KAS: the second arm (the row is asked 592.98 where it was asked 1,017.59).
3. The row set off, no call is due at 13, at 15.6 or at any lower price. The owner draws 272.44 (`draw_max`: the
   upper requirement of the hedged books).
4. Its short leaves by its own statement at the lower edge (stated and applied; the note applied; it is paid 0.65:
   the leaving's band takes its edges back). **The row is one-sided again, 1,005 units, and the shard holds
   734.39 KAS: 5.6 % of the row's worth where the gate asked 7.75 %. A call is due at once**, at either edge, and
   the owner's long is young or old as the owner chose to wait (hedged, at no risk).

Then the call (every step, note and claim at the engine; the owner's claim shown first):

| Price between the short's leaving and the call | the owner's long | the user's leg | the owner's claim (1st, 2nd) paid | the user's claim (1st, 2nd) paid |
|---|---|---|---|---|
| none (call at the upper edge) | old | old | (130, 0): 130 | (15.325, 0): 15.325 whole |
| none | old | young | (130, 0): 130 | (0, 14.6717): whole |
| none | young | old or young | (0, 0) | whole |
| 10 % up | old | old | (1,436.5, 0): 723.28 | (21.8575, 0): **11.0053** |
| 10 % up | old | young | (1,436.5, 0): 734.29 | (0, 21.1389): **0** |
| 10 % up | young | old | (0, 1,292.785): 712.43 | (21.8575, 0): whole |
| 10 % up | young | young | (0, 1,292.785): 722.47 | (0, 21.1389): **11.8135** |

- With no move of the price nobody is short: an old leg of the owner's is a claim of the first class of its two
  edges (130 KAS, no band), paid out of the owner's own coins.
- **F116's shape is back for an owner that waits T hedged**: its leg "entered while the shard, with it, was not
  called for T" (17.9), but what the requirement carried in that time was a row set off (2.25 % of the gross), not
  the one-sided row the call finds. The young rule does not see it: the leg is old, of the first class, at one
  share with old users and AHEAD of every young user (the young user above has nothing of 21.14).
- What it takes: a move of the price beyond what the shard holds (5.6 % here less the edges) between the leaving
  and the call, with a call due all that time (17.9's "whoever watches calls within T"); or, with no such gap, a
  move while hedged that the losing account does not pay (COMPUTED, NOT RUN: the short on its entry's least
  coins, 718.6 KAS, the price 10 % up while hedged, the short then leaving for what it holds: the shard has
  1,453 against the long's old claim of 1,436.5 and an old user's 21.86: the old user is paid 99.6 %, a young
  user nothing; that rests on nobody liquidating the short between 0.5 % and 10 %).
- What it costs the owner: nothing but time. It has its coins back by its own claims and when the call ends; what
  it wins is what its users are short.

Direction: the gate cannot stand at a leaving (a user must be able to leave). (a) Say it as it is: the gate
bounds an ENTRY by 7.75 % of the row; a pair and a leaving bring the same row about at 4.5 % (the upper rate on
the gross, twice) and what the edges cost. (b) If the gate's number is meant to hold of a one-sided row however
it comes about, the upper rate on the gross is what carries it: at half the upper rate on the net (3.875 %) a
row set off holds what the gate asks of either side alone. (c) The class of a leg under a call could look at
the row: a leg is "young" also where its row's net grew by a LEAVING within T of the call (8 bytes a row: the
time its net last grew other than by an entry). Not built, not costed.

### S2 (Medium): the gate is of what is HELD and of one ROW: an owner brings a call about at will, at no cost

RUN (`t7_s2_the_gate_is_of_what_is_held_and_of_one_row`: the entries at the engine, what they make due in the
model). The gate compares the row's requirement with what the shard holds less what it owes by claims, not with
its equity, and reads one row.

- In the shard of L4 (20 KAS held, of which 15 are what the user gained: an equity of 5, a lower level of 4.06)
  the gate lets in whatever leg asks 20 KAS or less of its row, although the shard's equity is 5: an owner's
  long of 2 units more at 13 (the row asks 7.0525) or a leg of 250 KAS in another row (19.375) passes, at the
  engine, and the call is due at once (`call_due`). The call that follows was not played to its end here: what
  the user is paid under a call is B1's.
  So the gate does not keep an owner from bringing a call about; what it keeps it from is a leg whose claim is
  beyond what the shard holds.
- What the user is then paid is what B1 (i) below measures: its gain less its band (14.67 of 15.325 at the upper
  edge), which is also what its own leaving would have paid (14.35 of 15 at 13: `t3_finding_l4`). So an owner
  that brings a call about takes from a young user no more than a leaving's band, and ends every position of its
  shard doing so. Low in coins; said because 17.8c reads as if the gate stopped the play.
- Four rows, one gate each: the same coins pass the gate of each row. A shard that holds H takes a leg of H / 7.75 %
  in each of four markets: run: a shard of 775 KAS takes 1,000 units at 10 in each of its four rows (each entry
  at the engine), is then asked 3,100 KAS, and a call is due at once. With S1 in each row: 4.5 % a row (not run). An owner's edge gains (1 % a row, first class where old) stay below what it must hold with these terms
  (mmr 5 %, g 0.5 %); with a maintenance rate of 2 % and the same tolerance the upper rates are 2.5 % and 0.75 %,
  and four rows of old legs entered at one edge and called at the other claim 4 % of a row's worth against
  3.25 % held: **the first class diluted with no move of the price** (COMPUTED; NOT RUN: the tests' terms are
  5 % and 0.5 %).

Direction: the gate on the equity at the entry's prices is 17.8b's trace (the cost 17.10 names); short of that,
the sum over the four rows of `need` at the leg's entry is 3 more multiplications of rows `KpfTEnterS` already
holds (it shows all 408 bytes), and closes the "four times" part.

### S3 (Low): L1's rest: a young leg that nobody steps leaves at its own later price, within the second class

RUN at the engine (`t3_finding_l1_...`). The step of the leg says 18.8 KAS for the young (20 less its band); its
user's own leaving, stated 101 blocks after the call's time at a later price, is a claim of the second class of
48.5. It is behind every old claim (that was L1), and beside other young claims it has 2.6 times its step's
share. F95 (open by decision: "stepped within a dispute period"), now inside the second class. For an OLD leg
F95 is unchanged: the leaving at a later price is a claim of the first class (`t1b_finding_f95_...`: 48.5 where
the call's price gives 20).

## (A) The earlier reviewers' tests

State found: 681 passed, 24 failed. Kinds: (i) an expected consequence, updated; (ii) a finding.

| Test | What changed | Kind |
|---|---|---|
| `t1b_r53_...sizes`, `t1b_r54_the_sizes_of_the_calls_scripts...`, `t1b_r55_...sizes`, `t1b_r57_...sizes`, `t1b_r58_...sizes`, `t1b_r59_...sizes`, `t1b_r60_...sizes`, `t1b_r62_...sizes`, `t1b_r64_...sizes`, `t3_b5_the_sizes...`, `t5_the_sizes...`, `t6_the_sizes...` | the six scripts' sizes (table "Sizes"); in `t3_b5` an entry stated 5,554 from 5,056 and the steps 7,355 / 7,407 / 8,003 from 6,972 / 7,024 / 7,620 | (i) |
| `t1b_fixed_f85_nothing_of_the_old_form...` | the scan of the sources for notes' kinds meets `i9(9)` in `KpfTCStep`: 9 is a kind now | (i) |
| `t1b_fixed_f93_whoever_steps_adds_nothing...` | a step is 383 bytes longer: whoever steps an account that has no fee is 1,671,000 sompi the poorer (1,594,400) | (i) |
| `t1b_fixed_f94_a_note_of_the_rest_is_counted...` | the note applied to the called shard was of a leg as `fut` gives it (its "time" is bytes of a committee's id: young): now of a leg entered at 0, which is what the test counts (the first class) | (i) |
| `t1b_r46_a_note_that_legs_left_is_applied_as_the_model_says` | the script's arithmetic written out by the earlier reviewer (`left_as_the_script54`) now has the class (re-derived from `KpfTLeft.sil`): the claim of four numbers, of the second class where a leg is young or anything was paid; the model is `left_class`; added: under a call the claim counted in the OTHER class is refused, each case | (i), strengthened |
| `t1b_r46_a_note_that_legs_left_is_applied_once...` | on the called shard the note of old legs is the first-class claim the test meant; added: of the legs as `fut` gives them, a claim of the second class, and counted in the first it is refused | (i), strengthened |
| `t1b_r47_the_note_a_leaving_of_the_rules_writes...` | by `left_apply_c`; under a call what is owed rises in the class the legs' times say | (i) |
| `t1b_r54_a_step_under_a_call_is_the_models_on_random_accounts` | the step's note is of tag 9; added: the young's amount by arithmetic written out from `KpfTCStep.sil` (`young_as_the_script_t7`) equals the model's on all 180 accounts; the step's output is the hash with it; one sompi off either way, and a note of tag 6, are refused (every sixth case) | (i), strengthened |
| `t1b_r60_the_note_of_a_mark_is_applied_whatever_the_books_hold...` | under a call the mark's note of a young leg is a claim of the second class (and counted in the first, refused); of the leg entered long before, of the first, as it was | (i), strengthened |
| `t1b_r54_what_a_step_takes_and_refuses` (control's note), `t1b_r54_...two accounts stepped` (the notes of A and B), `t1b_fixed_f89_...` (the note of what the account held), `t1b_..._paths` (`run`) | PASSED for the wrong reason (S4): now applied as the step wrote them, by `step_apply_t7` | finding S4 |
| `t3_f116_the_owners_young_leg_asks_nothing...` | PASSED for the wrong reason (S4). Re-derived: tag 9; the owner's young leg now says 2,870 KAS for the young (3,000 less the band of 130): a claim of the second class, paid 4.9 KAS (what the shard has beyond the 15 of the first class) even where it is shown first; the honest claim 15 whole. Added, the gate: into the shard of 20 KAS the owner's 1,000 units do NOT enter (the play fails at its entry: engine); paid up to 778.875 KAS, to the sompi, they do; in that shard the old leg's play pays the honest claim 3.87 KAS (0.099 before) | finding S4; (i) |
| `t3_finding_l4_an_owner_brings_a_call_about...` | PASSED for the wrong reason (S4), asserting the hole. Re-derived: the young user's claim of the second class of 14.35 KAS; the call does not end over it (model and engine); paid whole; the shard then open with 5.55 KAS. Added, the gate: the owner's 1,000 units at 13 do not enter the shard of 20 KAS; paid up to 1,012.5375 KAS they do, and the shard is then NOT due for a call | finding S4; L4 as found: closed |
| `t3_finding_l1_a_young_legs_gain_is_a_claim...` | asserted the hole (a claim of 48.5 at one share). Now: a claim of the second class of 48.5, nothing of the first; counted in the first, refused; beside an old claim of 9 on 9.9 held the old is paid 9 and the leaving 0.9 (1.55 of 9 before) | L1 as found: closed; S3 |
| `t3_b2_the_young_rule_at_its_edges...` | two young legs that gained: the note says 57.4 KAS (60 less the band of 2.6) and applied is a claim of the second class ("makes no claim" was L4); a young gain beside 6 KAS owed is said whole (28.7) and the account owes its 6 still | (i) |
| `t3r2_the_sum_at_its_ends...` | the last unit entered at 10 KAS into a row of the largest gross is refused by the gate's arithmetic (S5); the test's unit enters at a sompi; "a unit more" is held to the bound of the gross by its message, not to the gate | (i); finding S5 |
| `t3_second round, the accounts stepped` (`...two legs of one row`, `one leg`) | PASSED for the wrong reason (S4): by `step_apply_t7` | finding S4 |
| every entry written by hand in `auditT1_builders.rs` (`enter_by_hand52`, 30 places) | a precondition added: the gate, written out from `KpfTEnterS.sil` (`gate_open_t7`), lets the leg in: no "refused" entry of those tests is refused by the gate alone. One place fails it: `t1b_r51_what_the_stating_of_an_entry_takes_and_refuses` (below) | see below |
| `t1b_r51_what_the_stating_of_an_entry_takes_and_refuses` | "a row whose gross would be beyond the bound": with its leg at 10 KAS the entry is now refused by the gate's arithmetic before the bound is reached (S5): passed for another reason than it meant. Re-derived: the leg at a sompi a unit (the gate's numbers fit, the shard holds what the row asks): refused by the bound; the same leg into a row a sompi of size lower: taken (the control) | (i); finding S5 |
| `t6_for_piece_b_a_claim_counted_late...` | asserted the hole T29 (N1). Now: after a claim was paid the mark's note is of the second class (100 owed of the first still, 100 of the second); the first-class form is refused by the model and at the engine; the late claim is paid the 50 beyond the first class and, 50 more reaching the shard, the rest. The second play at the engine now (it was by hand): X paid 50 of 100; a note of 200 counted then is of the second class; Y, on time and not yet shown, finds its 50 (it found nothing). Part 2 (N2) unchanged | T29 / N1: closed |
| `t6_the_sizes...` | a note of one leg applied: 4,961 / 4,996 / 4,944 / 4,852 from 4,550 / 4,585 / 4,533 / 4,441 | (i) |
| `t5_b4_...` (the steps' notes of the call by a statement) | PASSED for the wrong reason (S4): the step's output held to `note_hash()`, applied with tag 9 | finding S4 |
| `auditT2_vault.rs`, `kpf-tests/auditT1_tests.rs`, `kpf-cli/auditT1_cli.rs` | not changed: no test of them applies a note or enters a leg (read by grep); all pass | |

After: every test of the seven files passes (`auditT1_builders` 196, `auditT3_legtime` 17, `auditT5_trace` 12,
`auditT6_claims` 14). No test was weakened: where a test asserted a hole that is closed, it now asserts the
closure and fails with "... is open" if the hole comes back.

Tests that change an argument by its place: read, all of them (`with_arg`, `args_of`). The entries of this
change keep the places of what they had (`young` and `tag, young` are appended; `books` stands where `rowsHash`
stood): no such test moved to another argument. `t3r2`'s and `t6`'s tests of "the 36 bytes" and "the books' hash,
32 bytes" do not include `KpfTMarkNote`; its 68 bytes are attacked in `t7_b4`.

## The earlier findings

| Finding | Now |
|---|---|
| F116 (a leg entered and called is a claim at one share) | young: closed (second class, behind; less its band). Old: as designed, one share; with the gate the owner pays the row's requirement in first: the honest 15 beside 3,000 of an old leg is paid 3.87 (0.099) |
| L1 | closed as found (engine). Left: S3 |
| L4 | closed as found (engine): the user is paid 14.35, what its own leaving would have paid. What an owner can still do: B1 below |
| N1 of review T6, T29 of the paper review (a claim counted after payments began takes from the others) | closed at the engine: counted late it is of the second class, whatever its leg (`t6_for_piece_b`, `t7_b2`, `t7_b4`) |
| N2 of review T6 (a first part below a note's least waits behind the claim's second part) | open, unchanged, and now reachable: a step's note of an account whose old legs gained little and whose young legs gained is a claim of both classes (`t7_b2`: 2 of the combinations). Low |

## Sizes (bytes)

| Script | 2e97ec6 | now |
|---|---|---|
| `KpfTCStep` | 5,056 | 5,439 |
| `KpfTLeft` | 2,506 | 2,916 |
| `KpfTLeftAll` | 4,310 | 5,093 |
| `KpfTLeftVoid` | 2,030 | 2,088 |
| `KpfTMarkNote` | 1,190 | 1,419 |
| `KpfTEnterS` | 2,484 | 2,982 |

| Transaction | before | now |
|---|---|---|
| an entry stated | 5,056 | 5,554 |
| a call's step, one leg (a note's least / 40 KAS) | 6,972 / 7,024 | 7,355 / 7,407 |
| a call's step, six legs | 7,620 | 8,003 |
| a note of one leg applied to a called shard, a claim written | 4,533 | 4,943 |

| a note of one leg applied, not called: paid whole / the rest written / nothing asked | 4,550 / 4,585 / 4,441 | 4,961 / 4,996 / 4,852 |
| a mark's note applied (called; not called) | not measured before (the script is 229 bytes longer and is shown 36 bytes more) | 3,069; 3,086 to 3,121 |
| a claim paid whole; the call's end (unchanged scripts) | 4,455; 4,345 | 4,455; 4,345 |

17.8c estimated an entry stated at about 5,300 and a note of one leg applied at about 4,550: they are 5,554 and
4,852 to 4,996: the two common paths are 10 % and 9 % heavier than at 2e97ec6, not 5 %.

## (B) What was run

### B1 (i): L4 as a game, with the gate and the second class (RUN at the engine)

`t7_b1_i_l4_as_a_game_with_the_gate_and_the_second_class`. The shard of L4 paid up to what the gate asks for the
owner's long of 1,000 units at the lower edge of 13: 1,007.4748 KAS (the owner pays 987.47 in; a sompi less and
the entry is refused, at the engine). The user's leg young (it gained 15 at 13). The owner's long, or its long
and a short at the upper edge from a second account, young. The call is stated at the upper edge of the moved
price. Steps, notes and claims at the engine, the owner's claims shown first. The call is written as its
settlement writes it whether or not it is due ("due" below is the model's `call_due`).

| The owner's | Price | Due | W | C2 (owner's + user's) | The user gained | its claim (2nd) | paid |
|---|---|---|---|---|---|---|---|
| long | none | no | 1,007.37 | 0 + 14.6717 | 15.325 | 14.6717 | whole |
| long | 1 % up | yes | 1,007.37 | 128.69 + 15.3184 | 15.9782 | 15.3184 | whole |
| long | 2 % up | yes | 1,007.37 | 258.04 + 15.9651 | 16.6315 | 15.9651 | whole |
| long | 5 % up | yes | 1,007.37 | 646.07 + 17.9053 | 18.5912 | 17.9053 | whole |
| long | 8 % up | yes | 1,007.37 | 1,034.10 + 19.8454 | 20.5510 | 19.8454 | **18.9686** |
| long | 10 % up | yes | 1,007.37 | 1,292.79 + 21.1389 | 21.8575 | 21.1389 | **16.2070** |
| long | 1 % down | no | 1,008.02 | 0 + 14.0250 | 14.6717 | 14.0250 | whole |
| long and short | none to 10 % up, 1 % down | no | 1,007.37 to 1,807.17 | as above | as above | as above | whole, every row |

- **The paper reviewer's table (3.79, 2.12 and 1.16 of 15 at 1 % and 2 % with edges, without the gate) does not
  hold WITH the gate: the user is whole at 1 %, 2 % and 5 %**, because the owner's 987 KAS paid in are in W.
  The user has what it gained less its own band (0.65 to 0.72 KAS: what a leaving takes as well).
- The user is short from a move of about 7.7 % while the owner's leg is young (T, 300 blocks here), with a call
  due from 1 % on: at 8 % it has 95.6 % of its claim, at 10 % 76.7 %, the same share as the owner's own claim.
- With both sides the owner's losing account pays in what the winning one claims (as far as it has coins: 799.8
  here): the user is whole in every row, and no call is due at all.
- What the play costs the owner: what it paid in is in the shard while the call stands, and it has it back by its
  own claims and at the call's end. Time, and nothing else.
- **L4 is closed** for the play as found and for moves below the gate's rate; a young gain is diluted (i) by a
  move beyond 7.75 % less the edges while the owner's leg is young and nobody calls a due call, (ii) by S1.

### B1 (ii): the way round the gate (RUN at the engine; the draw in the model)

Finding S1, with its table.

### B2: the class at the applying (RUN at the engine)

`t7_b2_the_class_where_a_note_is_applied_is_the_rules_and_no_other`. Notes made by hand, every combination of:
the kind said (6; 9 with nothing, with 7 KAS, with a sompi below nothing for the young; 6 with 7 KAS) x one leg
(`KpfTLeft`) or three (`KpfTLeftAll`) x all old, all young, mixed (T before the call's time is old, a block
later young) x asked 50 KAS, 5 KAS, nothing, 3 KAS paid in x nothing or 1 KAS paid to a claim before x called
or not. The outcome is computed in the test from 17.8c's rule; `left_class` says the same on each, and refuses
exactly where the rule does. **108 notes applied as the rule says (claims of the first class 15, of the second
36, of both 2); 192 that the rule refuses refused (young > 0 with no young leg; asked > 0 with every leg young;
young < 0; tag 9 on a shard that is not called; tag 6 with something for the young); 574 other outcomes
refused**: the two amounts the other way round, a sompi more in either class, no claim, the claim written and
the shard's count left as it was, everything in the first class, a claim that says a sompi paid, a claim where
none is due, and the same transaction with the kind said the other (6 for 9 and 9 for 6: argument 11, with the
note's input as it was: the hash is another).
- Twice, in two classes: a note is one input, spent where it is applied; the claim written is one output of four
  numbers; nothing applies a claim again (READ: `KpfTCallPay` pays it, and holds each part to what its class is
  still owed). A total above what the note says: refused ("a sompi more").
- A step's notes at the engine, as the steps wrote them: `play` (B1), 22 calls, and the earlier files.
- Not run: `KpfTLeftVoid` on a note of tag 9 (read: it hashes both amounts; the author's builder `left_void_c`).

### B3: the band and the step's two amounts (RUN at the engine)

- `t1b_r54_a_step_under_a_call_is_the_models_on_random_accounts`, extended: 180 random accounts of 1 to 6
  futures legs, old and young mixed at random and at the edges, over a statement or not: the young's amount by
  arithmetic written out from `KpfTCStep.sil` (`young_as_the_script_t7`) is the model's on every one; the
  step's output is the hash with it; what the account, a claimer and a disputer are paid is what they were paid
  before the change (the earlier arithmetic, unchanged, still equals the engine: `young` is paid to nobody and
  set against nothing); a sompi off either way and a note of tag 6 refused (30 of the cases).
- `t7_b3_the_band_at_its_corners`: a young SHORT that gained 30 from 13 to 10 says 28.7: the band's base is its
  entry's worth (130), the larger, which is right for a short (at the call it is worth 100: a band of the call's
  worth would leave it the tolerance's square, as 17.9 says of today's leaving); a gain of 5 below its band of
  10.05 says nothing; two legs worth 13 sompi each: one rate on the sum (band 1 sompi, where a rate a leg is 2);
  a young gain beside an old loss: the loss paid whole (30) and 28.7 said for the young beside it.
- With token legs and with something owed: `t3_b2` (the young's 28.7 said whole beside 6 KAS owed, the account
  owing its 6 still). Not randomized with token legs or with what is owed.

### B4: a mark's note under a call (RUN at the engine)

`t7_b4_...`: old leg: first; young: second; old or young after a claim was paid: second; not called (the
control): 20 paid, 80 owed, as before. Each other outcome refused (the other class, a sompi more in either, no
claim, written and not counted). The 68 bytes: a bit changed in the sum of the gross, in the rows' hash, in each
of the three numbers: refused; the 32 bytes of before: refused. `t1b_r60_...` and `t6_for_piece_b` the same on
notes real statements wrote. A mark's note applied: 3,069 bytes called, 3,086 to 3,121 not called.

### B5: the gate (RUN at the engine)

- `t7_b5_the_gate_on_random_rows_is_the_models_and_the_arithmetics`: 120 random rows (new, or with legs: a net of
  either sign up to the gross), legs of either direction from a hundred-millionth of a unit to 500 units at a
  sompi to 50 KAS a unit, the shard holding nothing, a sompi less than asked, exactly, a sompi more, half, far
  more, owing nothing or 3 KAS: the engine, `Shard::entered` and arithmetic written out in wide numbers agree on
  each: 57 taken by what is held, 20 taken though less is held (the row is asked no more), 43 refused.
- Overflow at the ends: S5 (engine and model both refuse).
- The second arm cannot build a row up: it never raises the row's requirement, so after any run of entries the
  row asks no more than the largest amount the shard held, less what it owed, at one of them (READ: the two
  arms). What raises it with no gate is a leaving (S1) and the price.
- The entry's price: finding S6 below.

### S6 (Medium, resting on 17.9's own rest): the gate's price is the one the entry STATES

RUN at the engine (`t7_b5_an_entry_stated_at_a_sompi_passes_the_gate`). An owner's long of 10,000 units stated
"from a sompi a unit" enters a shard of 1 KAS (the row asks 777 sompi). The books have the leg from that
transaction: at 13 the shard's equity is 130 thousand KAS below nothing and a call is due at once (model).
- While the entry is not settled the account is marked, and `KpfTCStep` steps no marked account (run: the step
  with the marked object in the account's place, and with its 180 bytes shown, refused; the control taken): the
  leg makes no claim. Disputed, the price point is answered wrong and the entry undone (not run here: reviews T1
  and T3). So with one dispute in a dispute period this is no way round the gate; it IS a way for an owner to
  have its own shard called at will for a bond (S2 has cheaper ones).
- Not disputed for a dispute period (50 blocks here), the entry is applied and the leg stands at its sompi: under
  the call it is young and its step says **128,699.9999 KAS** for the young (run: the step of the settled
  account): every young user of the shard is paid a ten-thousandth. Before the gate the same undisputed entry
  did the same; the gate does not narrow it, because it reads the stated price.
Direction: say beside the gate that it is as strong as the dispute of entries; or hold the gate's worth to the
larger of the leg's entry and what the row's own legs were entered at (the row would need its basis: 8 bytes a
row), so that a leg cannot be cheaper to the gate than its row is.

### B6: what a shard that is not called does (RUN at the engine)

Controls inside B2 (36 of the 108 notes are applied to a shard that is not called: paid as far as it holds, the
rest owed, the claim (rest, 0, 0, 0), tag 9 refused), B4 (the mark's note, not called), B5 (57 + 20 entries
taken), and the earlier files' tests of notes, rests and marks on shards that are not called, which pass
unchanged in their numbers.

## Not reached

- B1 (ii) with the price moving WHILE hedged and the losing account short of coins: computed, not run.
- S2's plays to their end (the call after the small leg, and after the four rows; S1 in four rows): the entries
  are run at the engine and the call's being due in the model; the calls themselves are not played.
- `KpfTEnterOff` after a silly entry and its note on the called shard; a call STATED and settled on the books of
  B1 (the calls of `play` are written as their settlement writes them; the statement of a call was run by
  reviews T1, T3 and T5).
- `KpfTLeftVoid` with tag 9 at the engine.
- B3 randomized with token legs and with what is owed.
- Terms other than the tests' (mmr 5 %, tolerance 0.5 %, lateness 100, dispute period 50).
- The draw of S1 at the engine (the model's `draw` and `draw_max`).

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, run once at the end, after the last change to a test: **713 passed, 0 failed, 0 ignored** (the sum of the `test result` lines). Found: 681 passed, 24 failed. The 32 more: the 24 brought along, and the 8 of `auditT7_second.rs`.
