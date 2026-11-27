# Review T17: the dated rows at the engine (17.8j, steps A1 to A3)

Of the built code of the commits `e6ed042`, `d1a0abb`, `5e6ef54`, `f411374`
(`git diff 327e619 f411374 -- contracts crates/kpf-protocol/src`). The reviewer's tests are
`crates/kpf-protocol/tests/auditT17_dated_rows.rs`: 14 tests, all passing (4 of findings, 10 of what held). A
test named `t17_vN_...` shows the finding VN as it stands (it passes while the finding is open and says so in
its assertion); `t17_held_...` is of what was attacked and held.

## Verdict

**The tree's two scripts hold; the trace does not, by a script that was not meant to answer it.**

`KpfTRowIn` and `KpfTRowOut` took exactly what the rule takes on 5,800 transactions on books at random, wrote the
model's shard each time, and left books that hold. `KpfTShardRowD`, the two ends and `KpfTChange` took the
model's verdict and not the other at every point tried. What a dated row is asked is exactly the most it loses
in the move.

But a dated row's point (101 and above) is, to `KpfTShardPrice`, "a point beyond the list": it answers it RIGHT
with nothing shown (V1, high). A wrong dated row is so answered right at its own point, and the dated rows of a
draw or a call are held by nothing but who lands first. **Step B should not be built on the trace before V1 is
closed**: it is a line in one script, and a sentence saying which script owns which points.

The other three findings are of the model and of a rule's direction, not of coins at the engine: the model
still refuses what `KpfTCallPay` now takes (V3, medium); a call counts some dated rows at the price moved
against the shard, by a sliver (V2, low); the model's call object lists a head the trace does not (V4, low).

## Findings

### V1 (high): a wrong dated row is answered RIGHT at its own point by `KpfTShardPrice`

`KpfTShardPrice.answer` takes every point from 5 that is beyond the list's last price as "of nothing: right, with
nothing cited" (`if (e <= ne) { ... }`; nothing is asked where `e > ne`). Before A3 no point above 8 meant
anything. Now the points 101 and above are the dated rows', and each of them is such a point: `at = point = 100
+ k`, `e = 96 + k > ne`, `ok = 1`, the verdict 1 is taken, with no leaf, no path and no citation shown. The
builder `shard_price_t` builds it without being forced (its own judgement is "beyond the list: right").

So for one point of one statement both verdicts are taken at the engine: `KpfTShardRowD` answers the wrong row
wrong (and right by nobody), and `KpfTShardPrice` answers the same point right. The statement stands again, the
disputer has lost its bond (a quarter the answerer's, a quarter the claimer's, half the copy's). It can be
disputed again until its last time to dispute, and answered "right" again the same way each time.

What it is worth: a draw whose dated rows say that nothing is asked of them is right at its end by its own sums
(`KpfTShardEnd` reads the last row); in the test's shard the owner draws 78 KAS where the dated rows allow 66.5.
A call's trace the same way (a shard that is not due called). The same holds of a trace that says NO dated row
over a tree that has leaves (the point 101), and of a call's row of an ended day, which `KpfTShardRowD` does not
answer at all. An honest disputer wins only by landing its own "wrong" answer before the claimer's "right".

Test: `t17_v1_a_wrong_dated_row_is_answered_right_at_its_own_point_by_the_prices_script` (24 wrong dated points
answered right, a draw's and a call's: a sompi less asked, nothing asked of any row, a row worth a KAS more, a
row left out, no dated row said; and the end of the draw that asks nothing right by its sums).

Recommendation: `KpfTShardPrice` answers no point from 101 (`require(at <= 100)`; a point from the list's end to
100 stays "of nothing"). Then a point beyond the dated rows' last (100 + nd + 2 and above) must still be
answered right by somebody, or a right statement is ended by naming it (a point of a draw or a call that nobody
answers ends it at the claimer's cost: `KpfTPoint.lapse`, read, not run here): `KpfTShardRowD` should answer `at
> nd + 1` right with nothing shown. Say in 17.8j, 6 which script owns which points, and add to the A3 test that
no other script takes the other verdict of a dated point.

### V2 (low): a call counts some dated rows at the price moved AGAINST the shard

"In a call a dated row is counted at the price moved the tolerance in the shard's favour." The code (model and
script alike, as "A3 as it is built" says in its parenthesis) moves it up where the clearing side holds calls,
wrote puts, or has puts and no net unit, and down otherwise. Where the row's worth to the clearing side does
not rise with the price, the move up changes nothing but the gross's worth, which it raises:

| Row | Counted | Against the shard (of the rows tried; strikes mostly near the price) |
|---|---|---|
| wrote calls; holds puts; calls of no net unit | down | 0 of 19,953 |
| puts of no net unit | up | 6,617 of 6,617 |
| holds calls | up | 4,755 of 6,649 (every strike not below the move's lower end) |
| wrote puts | up | 2,007 of 6,781 (out of the money beyond the move) |

The amount is the rate on the gross of twice the tolerance of the row's gross worth (1.75 % of 1 % with the
tests' terms). At the engine: a shard of 175 KAS whose one row is a thousand units at the price 10 holds its
lower requirement at 10 (175 KAS) and more than it at 9.95 (174.125); it is counted at 10.05 (175.875) and the
call is right at every point and at its end, for puts of no net unit, for calls held far out of the money, and
for puts written far out of the money.

Test: `t17_v2_a_call_counts_some_dated_rows_at_the_price_moved_against_the_shard`.

Recommendation: for the last phase, with the rates. Either count a row with no net unit down (as a futures row
of no net unit is: `counted_at` moves up only where the net is above nothing), which closes the one case that is
always against the shard; or say in 17.8j, 6 that "in the shard's favour" is by the sign of the row's worth, not
of what it is asked. The same is true of a futures row whose gross is more than some fifty times its net: as it
was before A.

### V3 (medium): the model refuses the claims of a called shard whose heads count leaves; the script pays them

A1 changed `KpfTCallPay` to read the sum of the gross and no row (17.8j, 2; review T16, Z3). `Shard::claim_paid_at`
(reserve.rs:1284) and `Shard::call_off` (reserve.rs:1324) still ask `self.rows == [ShardRow::EMPTY; 4]`. So the
builders `claim_pay_c`, `claim_pay` and `call_off` refuse to build what the script takes: with the tool as it
stands, the claims of a called shard that has a row listed are not paid, and its call does not end, until every
leaf has been taken out (anyone can, a leaf a transaction of 5.8 kB).

At the engine the script does what the paper says: the claim is paid whole and the call ended with the head
counting two leaves and the root as it was; another root or an emptied head in the output is refused; and with a
unit in a dated row (the sum of the gross not nothing) nothing is paid and the call does not end.

Test: `t17_v3_the_model_refuses_the_claims_of_a_called_shard_whose_heads_count_leaves`.

Recommendation: both model functions by `gross_sum() == 0` alone, with a test of a called shard that has heads.

### V4 (low): the model's call lists a head of no unit; the trace, and so the script's call object, does not

`KpfTCallEnd` writes the call's object from the list the statement says, and the model's trace lists no market
whose futures row has no unit (17.8j, 6). `Shard::called_at` and `Called::list` (by which `Shard::call_outs`,
`shard_call`, `call_step`, `call_off` name the call) list every row's market. For a shard with a head of no unit
the model's call is another object than the one on the chain: a tool that keeps the model's cannot open it. With
it: `shard_draw`, `shard_call`, `shard_end`, `call_end` and `shard_row` (by a `Draw`) state and answer the
four-row trace whatever the tree, which is wrong at the point 101 for a shard that has leaves (by the model).
Nothing of this is at the engine: the builders by a trace (`..._t`, `shard_row_d`) are right.

Test: `t17_v4_the_models_call_lists_a_head_of_no_unit_that_the_trace_does_not`.

Recommendation: `Called` made from the trace's list (or `Called::list` leaving out a head of no unit where no
dated row of its market has a unit); the builders by a `Draw` take the books, or refuse a shard whose root is
not the opened one, before step E.

## Attacked and held

Each at the engine unless said; the engine and the rule (written in the test from the documents, of the books
and not of the script's hashes) agree on every transaction.

| What | Test | Count |
|---|---|---|
| The hand-made transactions are the builders' (the same transaction id) | `t17_held_0` | 2 |
| `KpfTRowIn.put` on books at random (11 markets of every kind of number, three scales and limits, trees from empty to full, units and written days by hand, called shards): honest, and with one thing changed. The leaf before at another place (64 and more, below nothing), another leaf before (a key left out, out of order), another key said named, the key twice, the row at a taken place by its own path, a path of other books, the day's leaf put in where one stands, not put in where none stands (any day's leaf shown), a leaf made "not written", a mode that is none, the day's leaf's place taken, another scale, limit, row of the four (an empty place while the market has its row), market (of nothing, a number of nothing below zero), strike (0, below 0, above the limit), day, kind (0, 3, 4), another signer, a called shard; a bad key, scale or limit shown honestly | `t17_held_1` | 2,800: 431 taken (each the model's shard, the books holding after), 2,369 refused |
| Its output with one thing changed (the count one more or less, the root of before, the sum of the gross, what it holds) | `t17_held_1` | 5 a taken listing, all refused |
| `KpfTRowOut.out` the same way, every leaf in every mode: a row with a unit, a net, something entered; a day's leaf while a row of its day stands; the head's leaf; another leaf as the one that names it (the head, a row of the day, a leaf made to name it); by anyone with the day not written, with another day's leaf shown written, with its leaf said written that is not; not called; another signer or none; another row of the four; places of 64 and more | `t17_held_2` | 3,000: 338 taken (95 dated rows by anyone), 2,662 refused |
| Its output: the count not lowered or lowered twice, the empty row while the head counts or has a unit, the head kept where it counts none, the root of before | `t17_held_2` | all refused |
| The trace, a draw's and a call's, on 14 shards of 1 to 63 leaves: every row's point (1 to 4, 101 to one beyond the last) | `t17_held_3` | 826 points; 13 with no verdict (a call's row of an ended day) taken neither way |
| The prices' points of a statement with dated rows, by a citation (`KpfTShardPrice`; not tested by its author) | `t17_held_3` | 59 right, 35 wrong, the other verdict refused |
| The ends with dated rows (`KpfTShardEnd`; `KpfTCallEnd`, not tested by its author) | `t17_held_3` | 12 right (7 calls), 12 wrong |
| Traces said otherwise: each of the six numbers of a dated row by one, a bit of its hash, a row left out (first, middle, last), said twice, two swapped, a key of another tree, the fourth row's sums | `t17_held_3` | 322 wrong at a point by the model, "wrong" taken there and "right" not; 10 that move the place of a price in a row that reads none: right everywhere, with the true sums |
| The statement's length in `KpfTChange` (the byte, the count, a byte or a row more, no row, 64 and 65 rows, the list's count written otherwise, five prices, the two forms) | `t17_held_4` | 21 forms |
| `dated_adds` against the most a row loses at any price of the move tried, a draw's and a call's (in the model) | `t17_held_5` | 120,000; 90,084 with the upper end cut at the limit, 37,654 with the strike at an end exactly |
| Rows of the largest sizes (units to 2^63, a limit of 2^60, prices either side of the largest a call multiplies) | `t17_held_6` | 82 with the model's verdict, 98 with none taken neither way |
| A futures leg enters a head of no unit (`KpfTEnterS`); the last legs leave a head that counts leaves, and one that counts none (`KpfTLeft`, `KpfTLeftAll`, `KpfTLeftC`, `KpfTLeftAllC`); the outputs with the count, the head, the empty row or the root otherwise | `t17_held_7` | 9 taken, 20 other outputs refused |
| A shard with a root that is not its books': no row in or out, no dated point answered either way | `t17_held_8` | 14 |
| The root carried by `KpfTRest`, `KpfTLeftVoid`, `KpfTMarkNoteC`; `KpfTCallPay` (in V3's test) | `t17_held_9` | 3 taken, the opened root refused |

On the questions asked: no list of leaves was cut, no leaf orphaned, no count made to disagree with the tree;
every transaction taken wrote the model's shard, so two transactions each right alone reach only what the model
reaches. No trace said otherwise was right at every point (but by the place of a price nothing reads). No
statement has a length both forms take. A dated row is never read as a futures row or as an entry of the list:
the call's object is the hash of the list by the count as written, which `KpfTChange` held to the length.

## Read and not run

- A day's leaf that says neither (a net of 2, a gross): "not written" to the model's `day_said`, not to
  `KpfTRowIn` (which wants three numbers of nothing). No script writes one; for step D.
- `trace_dated_judged` says "wrong" where the place is wrong and a sum is beyond 64 bits; the script computes
  the sum and fails. A trace with such sums is wrong at an earlier point.
- The numbers of a dated row have two forms of nothing in eight bytes, as every number of a trace
  (review T5, R1): the scripts read both, `Traced::read` reads none of such a statement.
- A written day's leaf whose rows are gone is taken out by anyone, and the day can be listed again "not
  written": step D should not rely on a written leaf standing once its rows have left.
- A call over a row of an ended day that has a unit has no answer at that row (said in A3: step D). Once V1 is
  closed such a shard is not called until `KpfTShardRowE` is built.

## Not reached

- Sequences at the engine that mix listings with entries and leavings (each was run alone; together only by
  the model's own test of 6,000 steps, which I read and did not rerun).
- `KpfTCStep` under a call whose shard has heads of no unit, and the call's settlement (`KpfTChangeSettle`)
  with dated rows: the end's outputs were answered, the statement was not applied.
- `KpfTDrawNote` and `KpfTDrawEntry` over a statement with dated rows.
- The whole workspace was not run (by the rules of this review): only this file.

## What was run

`CARGO_BUILD_JOBS=2 cargo test -p kpf-protocol --test auditT17_dated_rows --jobs 2`: 14 passed, none failed
(about 17 seconds). No file but this report and the test file was written. One slip against the review's rule:
seven places of the test file were once patched through a shell script instead of the edit tool; the changed
places were looked at afterwards, the file compiles and its tests pass, and every later change was by the edit
tool.
