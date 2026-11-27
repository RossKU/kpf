# Review T6: what a called shard has paid (17.8c, piece (a)), at the script engine

27 November 2026. Under review: the uncommitted change on `563ef01` (the third of the four commits of 17.8a, its
first piece): `KpfTCallPay` (`claim`, `off`, rewritten), `KpfTLeft` (`apply`, `rest`), `KpfTLeftAll`,
`KpfTMarkNote`, `KpfTDrawNote` (kind 7), and the 36 bytes for 41 in `KpfTEnterS`, `KpfTLeftVoid`, `KpfTShardPrice`,
`KpfTShardRow`; `Claims`, `Claim`, `claim_paid_at`, `claims_time`, `call_off`, `books_head` in `reserve.rs`; the
shard's three numbers in `aobj.rs`; `claim_pay_c`, `claim_pay_as_c` in `thin_build.rs`.

One round. Written by it: this file; `crates/kpf-protocol/tests/auditT6_claims.rs` (new, 14 tests); and the earlier
reviewers' tests brought along (`auditT1_builders.rs`, `auditT2_vault.rs`, `auditT3_legtime.rs`, `auditT5_trace.rs`,
`kpf-tests/tests/auditT1_tests.rs`; `kpf-cli/tests/auditT1_cli.rs` needed nothing). No source, script, artifact,
`Cargo.toml` or other doc was touched; nothing under `crates/kpf-cli/kpf-out`; no commit; no node.

Every state of the second class, and every state of B1 and B2, is a shard and a claim **made by hand**: no script
writes a second-class amount in this piece. Said again at each item.

## Verdict

**The piece does what 17.8c says of it, and F114 is closed at the engine. No High or Medium finding; five Low,
two notes for piece (b).**

- **F114 is closed.** In its own numbers (100 KAS held against claims of 120 and 80; 60 KAS an account pays later
  reach the shard by `KpfTOwed.apply`): the claims are paid 60 and 40 and go on, the call does not end, the 60 are
  paid to the claims (36 and 24, their parts of the 160 ever held), and the call still does not end; `off` is
  refused at each stage. The three older tests that ended a call with claims short (review 54's whole way, review
  T3's and T5's) now show the shard staying called until its owner pays it up.
- **A claim is paid no more than its part, and the second class never before the first:** none found. The model is
  the script's text on 360,000 random shards and claims; at the engine, 1,200 random shards and claims: 428 of the
  model's outcomes taken and 7,087 lies refused (a sompi more or less to either class, a sompi moved between them,
  the claim spent while not whole, written again with other numbers or another key, the shard written with other
  numbers), and 7,720 outcomes written for 772 claims the model refuses, none taken. Over 6,000 sequences of
  payments and arriving coins (119,057 steps in the model, 900 payments and 843 lies at the engine): nothing paid
  beyond what is held; D1 within C1, D2 within C2 and within what was ever held beyond C1; what a claim has been
  paid is its two parts of all the shard ever held, to the sompi, whatever the order.
- **A call that should end, ends; one that should not, does not:** all 6,000 calls end once the shard has ever
  held exactly C1 and C2, every claim whole by one payment in any order (200 of the ends at the engine); 207 ends
  of a call not paid up and 31 other ways to end one are refused.
- **Two exceptions, both bounded:** the last part of a claim, where it is below a note's least, is whoever pays
  it's (K1: up to 0.2 KAS a claim, which an owner can arrange for every claim); and a note that was never counted
  is paid as a counted one, the call ending with the real claims short by its amount (K2: F96's class, with new
  consequences).

## Findings

**K1 (Low; RUN at the engine: `t6_k1_the_last_part_of_a_claim_below_a_notes_least_is_whoever_pays_its`).** *The last
part of a claim, below a note's least, has no output.* T33 (b) decided "whoever pays's" for a claim too small to be
an output. As built it is also the last part of **any** claim that was paid a part before: `whole == 1` with
`pay < NOTE_MIN` asks no output.
*Scenario, run.* Ten claims of 1 KAS (C1 = 10); the shard holds 8.1. Each claim is paid 0.81 KAS and goes on as
(1, 0.81). The owner pays in the 1.9 KAS that are missing and, in transactions that spend its own shard's output
one after another (nobody comes between), pays each claim "whole": 0.19 KAS, no output to the user. The owner's
transaction has 39,000,000 sompi of the user's before its fees (the 0.19 KAS and the claim's own least of 0.2 KAS,
which was the user's account's: F93). The call ends; each user has 0.81 of its 1 KAS, and the owner has its 1.9
back. In general an owner pays up to a level where every claim's remainder is below 0.2 KAS and keeps the
remainders: up to 0.2 KAS a claim (19 % of a claim of 1 KAS, 0.2 % of one of 100). With no play at all, a shard a
hair short pays parts, and what comes later of each claim is whoever sends the transaction first.
*Direction.* Either (a) where a payment makes a claim whole, ask one output to the key of the payment **and the
claim's own least** (the claim is spent: its 0.2 KAS is in the transaction, so the output is always one a block
takes; whoever pays then has nothing of it, and a user pays its own claim); or (b) pay no part that leaves a
remainder above nothing and below the least (pay a least less), so that the last payment is always an output.
(b) leaves a claim below the least as T33 has it.

**K2 (Low for this change; the class is F96's; RUN at the engine: `t6_b2_vi_...`).** *A note never counted is paid
as a counted one; the call ends; the real claims are short by its amount, for good.* "No part is more than C less
D" makes the call end; it does not tell a stray from a claim.
*Scenario, run.* Claims of 30 and 15 KAS (C1 = 45), a note of 20 KAS that a wrong statement nobody disputed wrote;
45 held. The stray shown first is paid 20. The claim of 30 is paid 25 and goes on as (30, 25). The claim of 15 is
paid by no outcome. D1 is C1: **`off` is taken**. After it the shard owes nothing: the claim of 15 is a note of
more than is owed, and `KpfTLeft.rest` spends it for nothing, by anyone (run); the claim (30, 25) is opened by no
script of an uncalled shard (run: `KpfTLeft.rest` shown 30 or 5; `KpfTDrawNote` in B5) and stands with its least
in it. Not ended, coins that reach the shard later pay neither (run). The real claims shown first are whole and
the stray has nothing (run). With half held, the stray first has its part of 10, the claim of 30 the 12.5 left
where its part is 15 (model). A stray of as much as C1, shown first, takes all the first class is owed.
One thing more: the claim (30, 25) that outlives the call is paid its 5 KAS **in a later call of the same shard**,
of what that call's claims are owed (run: a second call with one claim of 40 pays it 5, and the claim of 40 then
finds 35).
*Before the change* (read, the script of `563ef01`; not run): the stray paid 20, what is owed fell to 25, the claim
of 30 was then of more than is owed and paid by nothing for good, the claim of 15 was paid whole, and 10 KAS stayed
in a shard called for ever. The users lost the same 20 KAS; who loses them and whether the call ends changed.
*Direction.* None within this piece: a counted claim is told from a stray only by something the shard keeps of
each (a set: the tree of commit 4 could hold it). Until then it rests, as before, on every wrong statement being
disputed; say in 17.8c that the bound makes the call end and does not protect the claims.

**K3 (Low; the tool; RUN in the model: `t6_k3_...`).** *What is kept of a shard and of a claim.* (1) A shard kept
before this change (the same bytes without the 27 of the three numbers) is **not read back**: `shard_of` reads it
as a shard of no numbers (its comment says so), and `AObj::decode` then refuses it because `encode` writes the 27
bytes now. The object on the chain has the same hash as before; its file is refused by `acmds.rs` (`AObj::decode`
is how every clearing object is read), and so is a statement kept on such a shard. (2) A claim partly paid has no
form: `AObj::Rest` is a key and one amount, and no amount makes the hash of (A1, a1, 0, 0). (3) No command pays a
claim, steps an account under a call or ends a call (`kpf-cli/src` has none of `claim_pay`, `call_step`,
`call_off`, `shard_call`: read; as before this change).
*Direction.* Let `decode` take the shorter form as a shard of no numbers (compare with `encode` after taking the
27 bytes off), or re-write the files; a form for a claim (the four numbers) beside `Rest`.

**K4 (Low; words; RUN: `t6_k4_...`, and B1).** *"At most 7 parts in ten million less than its exact share"*
(`KpfTCallPay`'s header, the comment in `thin_draw.rs`) is the bound where nearly all that is owed is held. A part
is `floor(A h / o)`: below the exact share by up to a unit in h and a unit in o, about u/W + u/C of itself. With
3,000,000,001 sompi owed and a claim of all of it: half held, 1,499 sompi (1.0 part in a million) less; a
thousandth held, 1,000 sompi (333 parts in a million) less; on 360,000 random shards with at least half held the
largest was 9.4 in ten million (first class) and 8.6 (second). In sompi it is always small (under two units for a
class and a sompi a claim), never above the exact share, and what it leaves stays held for the claims. Say it in
sompi.

**K5 (Low; said to a user; RUN in the bring-along: `t3_f116_...`, and B2).** *A part below a note's least waits,
and where nothing more comes it waits for good.* A claim of A is first paid once all the shard ever held is at
least 0.2 KAS x C1 / A. In F116's old form (19.9 KAS held, 3,015 owed) the honest claim of 15 KAS has a part of
0.099 KAS: it is paid nothing and not spent (it was paid the 0.099 and spent), while the claim of 3,000 is paid its
19.8 at once. The 0.099 stay in the shard, which nobody but that claim can have (every part is of one sum; no draw
while called; `off` refused), so nothing is taken: but a claim of 1 KAS on a shard that owes 10,000 is paid
nothing until the shard has held 2,000. In B2, 50,695 of 84,469 claims shown waited. This is T13/T33 as decided;
it is said because a user sees it as "the large claims were paid and mine was not".

**Notes for piece (b)** (not findings of this piece; RUN: `t6_for_piece_b_...`):

- **N1. T29 in the first class alone, at the engine.** A shard that ever held 150; a claim of 100 is paid whole;
  a mark's note of 100 is then applied to the called shard by `KpfTMarkNote` (it reads no row: it is applied to
  books of no row; what is owed becomes 200, the numbers carried): its claim is paid 50, all that is held, where
  its part is 75. To the sompi what happened before this change. New: it goes on as (100, 50), the shard stays
  called, and 50 KAS that come later are its (before: spent with 50, the call ended). And the other way (model):
  X and Y of 100 each, 100 held; X is paid 50; a note of 200 is counted; it is paid the 50 held (its own part of
  100 over 400); Y finds nothing where its part was 50. "Second once anything was paid" is what closes it.
- **N2. A first part below the least waits behind the claim's second part.** First class covered (0.1 KAS owed
  and held, 1,000 sompi more); a claim of 0.1 KAS of the first class and 100 KAS of the second is paid by no
  outcome (0.1 KAS and 1,000 sompi: below the least together, not whole); the same 0.1 KAS as a first-class claim
  alone is paid whole. Nothing is lost (the second class cannot have it). In B2, 3,538 waits were of this kind.

## Holds, no finding

- **A shard that is not called is as before.** Its hash is the hash it had (three numbers of nothing hash to what
  was carried). A note the shard cannot pay is written (rest, 0, 0, 0) and raises what is owed; `rest` pays it, the
  note goes on as (what is left, 0, 0, 0) and what is owed **falls**; written the called way ((30, 10, 0, 0), D1
  raised) it is refused (B6).
- **`off` reads no time.** With nothing owed and no row it is taken in the block the call is applied in (run, every
  input landed long before the time of the claims). That is right: nothing is owed, and a mark's note of before the
  call that is applied later meets an uncalled shard, which pays it as far as it holds and whose draws it ends.
- **Before the time the owner gains nothing:** it adds (taken, the numbers carried), draws nothing (a draw's end
  on a called shard that has paid is answered wrong and not right: run), pays no note of the rest (`rest` refused,
  three ways of writing the shard), and ends the call only where D is C.
- **The periods are the shard's own.** No script that writes a moving part writes its bytes 93 to 120 (read: a
  search of every script for a moving part rebuilt found 15, none of which writes those bytes; the scripts were
  not each read whole); a shard of other periods pays from its own time (run); shown another moving part the
  script does not open the shard (run).
- **Read, not run:** a statement on a called shard (a draw, a second call) holds its output for up to a dispute
  period: no note is applied and no claim paid meanwhile. It costs half a bond a round and anyone answers it; it
  was so before. An undisputed second call would write a later call's time and so a later time of the claims:
  the premise that every wrong statement is disputed, as before.

## A. The earlier reviewers' tests

`auditT3_legtime.rs`, `auditT2_vault.rs`, `auditT5_trace.rs` and `auditT1_builders.rs` did not compile (16 literal
shards without `claims`: added, no meaning). Then 24 tests failed: 14 in `auditT1_builders.rs`, 7 in
`auditT3_legtime.rs`, 2 in `auditT5_trace.rs`, 1 in `kpf-tests/tests/auditT1_tests.rs`. **Each is an expected
consequence of the change; none is a regression; no test was weakened.** Now: 196, 9, 17, 12, 55 and 48 pass.

| Test | What changed | |
|---|---|---|
| T1b `r53`, `r55`, `r57`, `r58`, `r59`, `r60`, `r61`, `r62` `_sizes_...` (8) | the scripts' sizes (below) | expected |
| T1b `r54_the_sizes_of_the_calls_scripts_and_what_a_block_takes...` | sizes; the claims' coin lands at the time of the claims; a claim paid a part added | expected |
| T1b `r46_a_note_of_the_rest_is_paid_once...` | a called shard that pays a claim of 30 goes on owing 30 with 30 paid said beside (it owed 0) | expected |
| T1b `r54_a_claims_share_is_the_models_at_the_edges...` | the model is `claim_paid_at`: a part below the least waits (116 cases at the engine: 348 payments written for them, refused); a claim not whole is written again; each taken payment is refused a block before the time (125); the random sets: each claim its part of one sum whatever the order (stronger than "the share of what is left does not fall"), what is owed standing. The thresholds (100 taken, 100 refused) as they were | expected |
| T1b `r54_the_whole_way_of_a_call...` | **asserted the old behaviour as the end of the story:** the two claims were paid 32.07 and 12.83 of 70 owed and the call ended. Now: paid the same first part and the second its own part of the same sum (not all that was left), 70 owed still, **the call does not end** (model and script), the owner pays in 25.1, both whole, the call ends. What rounding leaves: below 2,000 sompi where it was below 2 (both parts are of one sum counted in thousands) | expected; F114's other half, closed |
| T1b `fixed_f89_...` | the claim's coin lands at the time; with the coin of 6,500 the builder writes nothing | expected |
| T1b `fixed_f94_...` | **F94 is closed more strongly:** 50 and 20 KAS on 44.9 held are paid 3,207,142,857 and 1,282,857,142 in either order, to the sompi (it was within a unit; the second had a sompi more as the last); what is owed stands. The uncounted claim of 71: refused also with every other number as the script would compute it (it was refused with the outputs of another claim: several reasons); the claim of 70 beside it is taken | expected; the lie tightened |
| T3 `b5_the_sizes...` | sizes; an entry stated 5,056 (5,040) | expected |
| T3 `r2_every_script_that_shows_the_rows_holds_the_41_bytes...` | 36 bytes; the other bytes shown are now the three numbers a sompi more each, a bit, each push's length, the 41 bytes of before, a byte fewer, a byte more; one case more (`KpfTCallPay.claim` on numbers that are not nothing, a claim of both classes): 11 transactions | expected. (Its name keeps "41".) |
| T3 `r2_a_script_that_carries_the_32_bytes_unopened...` | the line every script that hashes rows has (`p33(blake2b(gb.slice(9, 36)))`; `ng` in `KpfTCallPay`) | expected |
| T3 `r2_the_sum_at_its_ends_and_what_holds_it_to_the_rows` | **asserted, as a thing to know, that a call is ended on books of no row that say a sum of 5.** `off` reads the sum now: refused; and a claim on such books is paid by nothing (added). `KpfTLeft.apply` still compares nothing (a note of 10 units on books that say 3 leaves 7 below nothing: taken, as before) | expected; 17.8a, 1's "to know" closed for `off` and `claim` |
| T3 `f116_...` | the coin's time; in the old-leg form the honest claim's part (0.099 KAS) waits (K5) | expected |
| T3 `finding_l4_...` | the coin's time in the control. **The finding's assertion is unchanged and passes: L4 is open** (pieces (b), (c)) | finding stands |
| T3 `r2_the_sum_of_the_gross_is_kept_through...`, T5 `b4_the_whole_way_of_a_call_whose_list...` | each paid a claim 99.9 of 120 and ended the call. Now: paid its part, 120 owed still, the call does not end, paid up, whole, ended | expected |
| T5 `the_sizes...` | sizes | expected |
| kpf-tests `t1b_a3_every_entry...` | the source line of `KpfTShardRow.answer` | expected |

**Places.** The argument beside the rows kept its place in every entry (5; 7 in `KpfTEnterS`), so no index of an
earlier test moved; `KpfTCallPay.claim` grew at its end (amount still 8; paid 9, amount2 10, paid2 11, f 12);
`KpfTLeft.rest` (7, 8), `KpfTCallPay.off` (7, 8) and `KpfTDrawNote.end` (16 to 19) are as they were. Each place a
test of mine relies on is asserted against the bytes it should hold. The hazard this time was not the place but the
**time**: every earlier test that expects a claim's transaction to be refused would now be refused for a coin that
landed too early. Each was looked at: the refused transactions are made from one that is taken with a coin that
landed at the time of the claims.

**Earlier findings whose tests asserted the old behaviour:** F114 (no test asserted it; shown closed three ways,
above); the call ending with claims short (three tests: now the opposite, asserted); "a call ended on books that
say a sum of 5" (now refused, asserted); F94 (closed, exact); F96 (unchanged: a note of more than is owed ends no
draw and is spent for nothing; K2 is what that does after a call); L4 (open, unchanged).

## B. What was run

All in `auditT6_claims.rs`, at the engine unless said.

| Item | Test | What |
|---|---|---|
| B1 | `t6_b1_the_model_is_...` | model only: 360,000 random shards and claims (11 ways of drawing what was ever held: nothing held, a sompi below, at and beyond C1, a sompi below and at C1 and C2, a sompi below a whole number of units, parts, more; sizes around 3,000,000,000 x 1000^k to k = 3, a note's least, a sompi): `claim_paid_at` is my own writing-out of the script in every case (121,411 paid, 151,823 waiting, 26,766 of nothing, 60,000 out of bounds refused) |
| B1 | `t6_b1_at_the_engine_...` | 1,200 shards and claims by hand: 428 outcomes taken (229 whole, 199 written again; 68 pay both classes, 53 the second alone), each refused a block before the time; 7,087 lies refused; 506 waiting and 266 out of bounds or of nothing: 7,720 outcomes, none taken |
| B2 (i) to (v) | `t6_b2_over_sequences_...` | 6,000 shards of 2 to 6 claims of both classes, all counted: every step in the model; at the engine 900 payments, 843 lies, 1,253 waiting claims (three outcomes each), 207 early ends refused, 200 ends taken |
| B2 (vi) | `t6_b2_vi_...` | K2, every step at the engine but the half-held variant and "real claims first" (model, one transaction) |
| B3 | `t6_b3_...` | `f` naming each of four inputs (taken at the time, refused a block before though the others are later, refused for another input's landing); 7 values beyond or below; a shard of other periods; a moving part not its own; the owner adding, `KpfTLeftVoid` on a shard that has paid, `rest` on a called shard, `off` long before the time, a draw's end |
| B4 | `t6_b4_...` | 31 refused: D1 or D2 a sompi off either way and three more, a row with a unit, a sum of 1, 5, -1 and the largest on empty rows (by hand), ten ways of writing the shard, another time, body, call, a claim in the call's place; the shard after equals the shard of before the call; an entry and a draw on it |
| B5 | `t6_b5_...` | 27 refused across `KpfTLeft.rest`, `KpfTCallPay.claim`, `KpfTDrawNote.end`: the note of before, a claim of nothing, partly paid, whole, of the second class, of both, a note of a leg, the call |
| B6 | `t6_b6_...` | the life of a note of the rest on an uncalled shard, then the same note under a call |
| F114 | `t6_f114_...` | review 65's numbers, `KpfTOwed.apply` |
| K1, K3, K4, notes | `t6_k1_...`, `t6_k3_...`, `t6_k4_...`, `t6_for_piece_b_...` | above |
| sizes | `t6_the_sizes_...` | below |

## Not reached

- **No state of the second class is a script's.** B1, B2, the sizes and N2 use shards and claims made by hand; so
  is every shard of B1 and B2 (a real call, its steps and notes are run in the tests brought along and in
  `thin_draw.rs`, with first-class claims only).
- B2 at the engine is one step in forty; the rest is the model, which B1 holds to the script.
- Amounts of 2^62 sompi and more (the harness's limit); B1's shards go to 3 x 10^18.
- `KpfTLeft.apply` and `KpfTLeftAll.apply` on a called shard **that has already paid**: I think no such
  transaction exists (a claim is paid only on books of no row, no leg enters a called shard, and these scripts
  need the leg's row), so the three numbers they carry there are always nothing. Not run; the mark's note, the
  note of what an account paid, the owner's addition and `KpfTLeftVoid` on a shard that has paid are run.
- The time against real statements: that a mark stated a block before the call's time and disputed is applied a
  dispute period before the claims open is read (17.8c, T33 (f)), not run. A token leg's sale as the source of
  late coins (the same note as `KpfTOwed`'s) not run.
- K2 before the change: read, not run (no checkout).
- Nothing on a node; no CLI path exists to run (K3).
- Pieces (b) and (c): the note of tag 9, the class where a note is applied, "second once anything was paid", the
  band, the gate.

## Sizes

| Script | `563ef01` | Now |
|---|---|---|
| `KpfTCallPay` | 1,691 | 2,518 |
| `KpfTLeft` | 2,374 | 2,506 |
| `KpfTLeftAll` | 4,226 | 4,310 |
| `KpfTLeftVoid` | 2,017 | 2,030 |
| `KpfTMarkNote` | 1,154 | 1,190 |
| `KpfTDrawNote` | 1,700 | 1,742 |
| `KpfTEnterS` | 2,463 | 2,484 |
| `KpfTShardPrice` | 2,700 | 2,707 |
| `KpfTShardRow` | 2,098 | 2,105 |

(The four that only show the 36 bytes are longer, not shorter: they hash the 27 bytes where they carried a hash.)

| Transaction | Before | Now | 17.8c's estimate |
|---|---|---|---|
| a claim paid whole, with its output | 3,624 | 4,450 | about 4,100 |
| a claim paid whole below a note's least (no output) | | 4,342 to 4,345 | |
| a claim paid a part and written again | (none) | 4,485 or 4,486; of both classes 4,497; already partly paid 4,491 | |
| the call's end | 3,518 (derived: the script 827 longer, 5 bytes fewer shown; not measured at `563ef01`) | 4,340 | about 60 more |
| a note of one leg applied, nothing asked | 4,314 | 4,441 | about 4,550 |
| the same, paid whole; a note of the rest written; called, a claim written | | 4,550; 4,585; 4,533 | |
| a note of six legs applied to a called shard, a claim written | | 6,939 | |
| an entry stated | 5,040 | 5,056 | (the gate is piece (c)) |
| a mark's note applied to a called shard; a note of what an account paid | | 2,805; 2,531 | |

A claim's payment is 826 bytes heavier, 350 more than estimated: it is the script whole (both formulas, the time),
in every payment and in the call's end.

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, once, at the end: **65 targets, 703 tests
passed, none failed, none ignored** (689 after review T5, and the 14 of `auditT6_claims.rs`).

# Second round: K1

27 November 2026, the same day. Changed since the first round, in the working tree (the same base): in
`KpfTCallPay.claim` the payment that makes a claim whole pays its key, in **one** output, the part **and what the
claim's note held** (`tx.inputs[n].value`), whatever the part's size, and a payment that does not make it whole is
a note's least or more (so every payment has an output to the key); the builder `claim_pay_as_c` to match; and
`AObj::encode` writes a shard's three numbers only where they are not nothing. Written by this round: this
section; in `auditT6_claims.rs`, K1's and K3's tests turned into the proofs that they are closed, `output_lies`,
and B1's look at the outputs; and the tests brought along (below). No source, script, artifact or other doc
touched.

## Verdict

**K1 is closed at the engine. K3's first part (a shard kept before the change) is closed. Nothing new was
found.** One thing of words (K6).

## K1, as run (`t6_k1_closed_the_last_part_of_a_claim_and_what_its_note_held_are_paid_to_its_key`)

The first round's scenario, every one of its 22 transactions at the engine. Ten claims of 1 KAS (C1 = 10), 8.1
held.

- **Each claim is paid its part, 0.81 KAS:** the output to the key is the part alone; the claim goes on as
  (1, 0.81) **holding the 0.2 KAS it held**. Refused for each of the ten: the key's output a sompi short; to
  whoever pays; not the last output; the claim again holding a sompi less; **what the note held paid out with the
  part** (to the key, the claim again holding a sompi): `tx.outputs[c1].value >= tx.inputs[n].value` holds, so a
  part's payment cannot take the note's value out.
- The owner pays in the 1.9 KAS that are missing (`KpfTAddC`) and pays each claim whole itself. **Each payment is
  0.19 KAS, below a note's least, and has an output of 0.39 KAS to the user** (the part and the note's 0.2).
  Refused for each of the ten: that output a sompi short of 0.19 + 0.2; **no output to the user** (the last output
  to the owner; and the user's output there but not the last); the part alone (0.19); the note's least alone
  (0.2).
- The call ends. **The users have 1,200,000,000 sompi: 10 of 10 KAS, and their ten notes' 0.2 KAS.** The owner is
  out 201,599,500 sompi (the 1.9 KAS and its fees); whoever paid the ten parts is out 10,980,000 (its fees). 129
  transactions with the outputs written otherwise refused in this test.
- **A claim whole by ONE payment** pays its amount and what its note held in one output (a sompi less refused, and
  the other four lies): 12 KAS, the output 12.2 KAS; the same with a note that held 0.5 KAS, 12.5; a claim of a
  note's least, 0.4 KAS; of a sompi below it, 0.39999999; **of a sompi, 0.20000001 KAS** (it had no output in the
  first round); a claim that says it was paid all it asks (by hand), its note's 0.2 to its key.
- **On random states** (B1 at the engine, the same 1,200 shards and claims): for each of the 428 payments taken,
  the last output is the key's and is exactly the part, with what the claim's note held where it is whole;
  whoever pays is out its fee and the 200,000 sompi of the script's copy **to the sompi**; 2,067 transactions with
  the outputs written otherwise refused. The first round's 7,087 lies and 7,720 outcomes for claims the model
  refuses are refused as before (those whose output is too small for the builder to write are written by hand:
  below).

**What whoever pays a claim has of it now: nothing.** It is out the transaction's fee (891,000 sompi at the least
rate for a claim paid whole, 898,000 for a part) and what the script's copy keeps (200,000): about 0.011 KAS. It
had the claim's least (0.2 KAS) and any last part below it. **No other path** leaves something of the user's with
whoever builds the transaction: only the payment leaves the shard (its output holds no less than before less the
payment, and its state says held less the payment); what the note held goes to the key (whole) or to the claim
again; the script's copy keeps its 200,000 from the payer; at the call's end the call's coins return to the
shard. Run on every taken payment of B1 as the equality above.

**Is there a claim nobody would bother to pay?** Not for its user: a payment the script takes is a part of a
note's least or more, or the last part with the note's 0.2 KAS, for about 0.011 KAS of fees; a claim of a sompi
pays its user 0.2 KAS. A third party has no reason any more (said: F93's "the note's least is whoever pays the
claim's" no longer holds; what is said to a user is "show your own claim"). A claim whose user is gone is paid by
the owner, which needs D1 at C1 to have its shard back, at 0.011 KAS a claim; where the owner gave the shard up
too, that claim stands and nobody else loses by it.

**Read, beside it.** Every output a payment asks is one a block takes: a part is a note's least or more; a whole
payment is at least what the claim's note held, and every script that writes a note of the rest asks that it
holds a note's least (`KpfTLeft` apply and rest, `KpfTLeftAll`, `KpfTMarkNote`: `>= NOTE_MIN`; the claim again:
`>= tx.inputs[n].value`). The builder weighs a transaction and writes none whose output is too small (beside a
claim written again, below about 0.034 KAS: the storage mass of a block); the harness's engine does not weigh.
So the lies that pay a few sompi are written by hand here (as the builder writes a payment of a note's least,
the two outputs then set), and a claim of a sompi whose note holds 1,000 sompi (by hand; no script writes one) is
"paid" in the harness with an output the builder tops up to 0.025 KAS at the payer's cost.

## K3 (`t6_k3_closed_...`)

- **Closed:** a shard kept before the change (kind, terms, rows, moving part) reads back as the shard of no
  numbers, and a shard of no numbers is kept in exactly those bytes (called with nothing paid yet as well); a
  statement kept on one reads back.
- A shard with numbers (five sets, each number alone and together) is kept 27 bytes longer and reads back; a
  statement kept on one reads back.
- One form: the bytes of a shard of no numbers with 27 bytes of nothing after them (what the first round's tree
  wrote for every shard) are refused. None exists outside that tree's tests.
- **Still so:** a claim partly paid has no form in `AObj`, and no command pays a claim, steps an account under a
  call or ends one.

## K6 (Low; words; READ)

`KpfTCallPay.sil`'s header (its first sixteen lines) says the rule of before: a share of "what the shard holds
over what it owes, both as they stand (they fall together)", "the claim is spent whole", `off` "where nothing is
owed by claims", the two entries with the arguments of before, and K4's seven parts in ten million. The entries'
own comments are right.

## The tests brought along

All by K1's rule or the script's size; none a regression, none weakened.

| Test | What changed |
|---|---|
| T1b `r54_the_sizes_of_the_calls_scripts...`, `r55_the_sizes...`; T3 `b5_the_sizes...`; T6 `the_sizes...` | `KpfTCallPay` 2,523 (2,518) |
| T1b `r54_the_whole_way_of_a_call...` | the claim paid whole has its last part and its note's 0.2 KAS in one output |
| T3 `f116_...` (the young form), T3 `finding_l4_...` (the control) | a claim of 15 KAS paid whole: 15.2 KAS in one output. L4's assertion unchanged: open |
| T1b `r54_a_claims_share_is_the_models...` | the outcomes of a part below the least were skipped where the builder writes none (it weighs the output): written by hand so that the script is still asked: 348 refused, as in the first round |
| T6 B1, B2, `b2_vi`, `for_piece_b` | `pay_tx` the same way; the stray paid whole has its note's 0.2 KAS too; B1 looks at the outputs |

## Sizes

`KpfTCallPay` 2,523 bytes (2,518 in the first round; 1,691 at `563ef01`).

| Transaction | First round | Now |
|---|---|---|
| a claim paid whole | 4,450 | 4,455 |
| a claim below a note's least paid whole | 4,342 to 4,345 (no output) | 4,451 to 4,454 (with its output) |
| a claim paid a part and written again | 4,485 or 4,486 | 4,490 or 4,491; of both classes 4,502 |
| the call's end | 4,340 | 4,345 |

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, once, after the last change to a test: **65
targets, 703 tests passed, none failed, none ignored.**
