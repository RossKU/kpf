# Review T9: stage 1 of the tree, a shard's row named by its key (at the engine)

Reviewed: the uncommitted working tree over `d46bdc7` (docs/ARCHITECTURE-CLEARING.md 17.8d, "Stage 1: keys for
names", "Stage 1 as it is built", and the table of review T8). The reviewer's tests:
`crates/kpf-protocol/tests/auditT9_keys.rs` (10 tests, all run at the script engine, all pass). The earlier
reviews' files `auditT1_builders.rs`, `auditT3_legtime.rs`, `auditT5_trace.rs`, `auditT6_claims.rs` were brought
along (the nineteen, below). Nothing in `src/`, `contracts/`, the artifacts or other tests was touched.

## Verdict

**Commit as it is, with the paper's paragraph corrected in three places (Q2, Q4, Q5 below: words, no code).**
Stage 1 does at the engine what its paragraph says: a leg's row is the row of its futures key and no other, for
an entry, a leaving (one leg, several) and a void; a price is answered for the committee the shard's id is
opened by and for no other; a call's step finds the leg's market plain. The model and the engine agree on every
random case run (60 random books of rows named by keys here; the earlier randomized tests, re-seeded, pass).

One finding should be DECIDED before stage 2 is built on this (Q1, medium): stage 1 puts the plain market into
a list ordered by NUMBERS while `KpfTEnterS` compares keys as BYTES, and the paper's own decision that excludes
the gap ("an id of which a number of 8 bytes is nothing written below zero is no market's") is built nowhere in
stage 1. It needs the shard's owner's signature and gives the owner nothing that P1's open item (a row of a
market the committee never signs) does not already give, which is why it does not hold the commit.

## Findings

| # | Severity | What | Test (all run, all pass as written) |
|---|---|---|---|
| Q1 | Medium | **Two ids that are one market by their numbers enter two rows, and no draw or call over them is right.** The list of prices now holds the plain market and is ordered by its 32 bytes as four numbers (`Priced::numbers`, `KpfTShardPrice`); a script reads `00 00 00 00 00 00 00 80` as nothing. `KpfTEnterS` compares the 60 bytes of the key and holds no market to the paper's decision. Ids `M` and `M'` that differ in one chunk written `00..00` and `00..80`: both enter (engine AND model take the second: `Shard::entered`), two rows; the trace has two entries of which neither is below the other; the second is answered wrong with nothing cited and right by nobody (cited or not), in either order; the model names the same point (6). Every statement over such books has a wrong point: no draw, and no CALL (the same script answers a call's prices). Needs the owner's signature at the entry (its own account is enough). Same power as P1's open item, by another door; the old form (hashed names) had no such pair that anyone could write. Fix (not made): `KpfTEnterS` refuses a market of which a chunk is `00..80` (four comparisons), and `Shard::entered`/`RowHead::of` the same; or the list is ordered by bytes | `t9_k1b_two_markets_that_are_one_by_their_numbers_enter_two_rows_and_no_trace_over_them_is_right` |
| Q2 | Low (paper and model) | **"An entry no row is of is priced as any other" is built, and the model's judge still names such an entry wrong.** A draw that is the rule's but for one entry more (in order, of a market the committee signs, of no row), priced right: the end, the four rows and the three prices are all answered right and none wrong: the statement stands (before: unanswerable, it cost its writer a bond). Priced at twice the committee's: answered wrong with the citation. `Shard::trace_wrong_point` gives an entry of no row the price 0 and names its point as wrong whatever it says: a watcher that disputes by it where the entry is priced right is answered right and LOSES its bond. The CLI's `draw_wrong_point` builds its own trace of the rows and is not affected. What an entry more costs the others: 56 bytes in a call's object, shown at every step. Unchanged: an entry of a market the committee signs NO price of is answered by nobody and costs whoever wrote it (`t5_b3...` passes as it was; its heading and `thin_draw.rs:3674` "nobody cites its price" now say more than the engine does). Fix (not made): the model's judge takes the committee's price of an entry of no row from its caller, or says "not judged"; one sentence in 17.8d | `t9_k3_an_entry_no_row_is_of_is_priced_as_any_other_and_the_model_still_names_it_wrong` |
| Q3 | Low (not reachable in stage 1; a rule for stage 2 and the day) | **A row emptied keeps its nine bytes of "what was entered"; with a number there it is not the empty row.** No script writes them other than nothing today (`KpfTShardOpen`, `KpfTEnterS` write nothing; `KpfTLeft`, `KpfTLeftAll` keep `row[96..105]`). A row made by hand with 7 there: an entry writes nothing over it (kept: refused); the last leg leaving writes a row of no market, no unit and 7 entered (engine and model), which no leg can enter (`KpfTEnterS` requires the 105 bytes of nothing) and with which `KpfTCallPay.claim` and `.off` (which require four rows of nothing, by bytes: read in the script, not run) would never be taken. When a dated row's legs write the number, whoever empties a head must empty it too | `t9_k1_what_was_entered_is_written_by_nothing_and_a_row_made_with_it_is_stuck_when_emptied` |
| Q4 | Low (paper) | **Sizes the paragraph does not say.** `KpfTShardOpen` is 752 bytes (730): not in the list. A claim paid and a call's end are 56 bytes MORE each (4,511 / 4,510 / 4,546 / 4,558 / 4,401; `KpfTCallPay` +44, rows +12). A note of one leg applied is 10 bytes more (4,971 / 5,006 / 4,954 / 4,862). An entry stated is 5,550 (5,554). A step is 120 bytes less (7,235 with a list of four; 7,123 / 7,175 / 7,771 with a list of two) | `t3_b5...`, `t6_the_sizes...`, `t1b_fixed_f93...` |
| Q5 | Low (paper) | **The futures row of a market can be put in AFTER a row of that market with a day.** `KpfTEnterS` refuses a second row of one KEY, not of one market: beside a row of M with a day, a kind or a strike (made by hand) the leg of M enters an empty row (engine and model agree). The tree's rule (the futures row first) is stage 2's; stage 1 does not hold it and no script of stage 1 writes a dated row, so it is unreachable. Say in the paragraph that "one row a key" is what is held | `t9_k1_a_leg_enters_the_futures_row_of_its_market_and_no_other` |
| Q6 | Low (usability) | **Whoever answers a price must know the seed of the shard's opening.** No object says it (it is the hash of the opening's outpoint); `kpf` now asks `--seed` for a price's answer with a citation. As `KpfTStepPrice` already. A disputer's bot must follow a shard from its opening, or the seed must be published with the shard | read in `crates/kpf-cli/src/acmds.rs`; not run |
| Q7 | Note | A test artifact found while bringing `t3_b5` along: before stage 1 the old head held the 32 bytes where a covered leg says its TIME, so `Shard::booked` made old and young legs of one market two rows; that test's books had four rows and its call four prices (two markets twice), an object no entry at the engine could make. Stage 1 removes the possibility (a head has no such bytes). Of its 232 bytes "saved" a step, 112 were this | `t3_b5...` (asserts two rows, two prices now) |

## What was attacked and held (auditT9_keys.rs)

1. **Keys.**
   - `t9_k1_a_leg_enters_the_futures_row_of_its_market_and_no_other`: the row after an entry is exactly
     `push(M) i9(0) i9(0) i9(0) i9(scale) i9(limit) i9(net) i9(gross) i9(0)`, 105 bytes; any empty row takes it
     (engine = model). A row of M with a day, a kind, a strike, a day below zero: the leg enters it in none of
     three ways (keeping its key, writing the futures key over it, writing a fresh row over it). Two rows of one
     key: refused at rows 1 to 3 while row 0 has the key (control: a second leg into row 0 taken). A leg of
     another limit, and of another scale, than the row of its key: enters neither that row (written either way)
     nor an empty one (control: alone, taken). The market of nothing: enters no row of an empty shard nor of one
     with a row (the account's side does state such an entry; the shard's side refuses).
   - `t9_k2_a_leg_leaves_the_futures_row_of_its_market_and_no_other`: `KpfTLeft` (one leg) and `KpfTLeftAll` (two
     legs): applied to the futures rows (control); with the leg's row made a row with a day, a kind or a strike:
     applied at no row named (0 to 3; the shard after written two ways), and `KpfTLeftVoid` spends the note by
     that leg and not by its other leg. A leg of the market of nothing leaves no row (an empty row has its key:
     the scale and limit differ; with a scale and limit of nothing too: refused), and its note is voided.
   - Nothing below zero in the key's three numbers: every script that writes a key writes
     `leg[0..33] + three pushes of eight zero bytes` or copies `row[0..78]`, and `KpfTEnterS` holds `leg[0] == 0x20`:
     no script writes such a key. Not run by hand (the model's `ShardRow` cannot say the bytes).
   - Nothing below zero in the market's chunks: Q1.
   - The nine bytes of what was entered: Q3.
2. **`KpfTShardPrice` by the seed** (`t9_k3_a_price_is_answered_for_the_committee_the_shards_id_is_opened_by`):
   control (both prices of a draw right; arguments 20, 21, 22 are the seed, the committee, the kind). Refused:
   five other seeds (nothing, one bit, the committee, the id, the owner's key); five other committees; the two
   the other way round; kind 11 on a draw; the citation of the other market of the same committee at this point
   (both ways; the builder too). A shard of another seed and the same committee: answered with its seed (the
   control of the hand-written answer), not with this one's. A shard whose id is opened for ANOTHER committee:
   this committee's citation answers no price of it, whichever committee and seed are shown (four pairs; the
   builder too). A shard whose id is of chosen bytes: no price answered (four pairs); its rows are answered as
   any other's; the dispute of its price lapses and the disputer has its bond and half of the stater's: **whoever
   states on such a shard pays** (no script writes one: `KpfTShardOpen` writes the id; `t1b_r46...` answers a
   price of a shard opened AT THE ENGINE with the seed of its anchor, which holds the two scripts to one hash).
   An entry of no row: Q2. An entry of a market nobody signs: still costs its writer (`t5_b3...`, unchanged).
3. **One entry a market**: `t5_b6..._are_one_entry_and_one_price` (rewritten: a futures row and a row of calls
   of one market: two prices have no trace; one price is one entry both rows name, the points 0 to 6 right at
   the engine; the old form by hand has its second entry wrong with nothing cited); `t3_b4...` (the call's list
   of two rows of one market is the first row's entry alone; the step at the second price refused);
   `t9_k3b_two_rows_of_one_market_with_two_limits_have_no_right_trace` (the model: no trace; the engine: said
   with one entry, the row of the other limit is wrong; said with two entries, the second entry is wrong with
   nothing cited: no statement over such books is right. Made by hand; the tree's rule gives a dated row its
   futures row's limit).
4. **`KpfTCStep` with the plain market** (`t9_k4_a_calls_step_finds_the_legs_market_plain_in_the_list`): for a
   market of the world and for `ff..ff`: found at its place (control; `ff..ff` stands first: the order of
   numbers). Refused: the old form's name (the hash of tag 14 and the market), the options' name (tag 13), the
   market with another top bit, another limit, a day, a day of nothing written below zero. The entry a place
   later in a longer list: taken named there, refused named where it was. A list by hand with the market twice
   (no right trace has one; `KpfTCStep` reads no order): taken at either place, as before stage 1.
   `t9_k3_the_lists_order_is_of_numbers_where_a_markets_top_bits_are_set`: rows of `01..01` and `ff..ff`
   (the committee signs both): the model lists `ff..ff` first and both prices are answered right; in the order
   of the bytes the second is answered wrong.
5. **Model = engine on random shards.** New: `t9_k5_an_entry_a_leaving_and_a_void_on_random_books_the_model_and_the_engine`
   (60 books; rows of six markets, some with a day, some of another limit; legs of those markets or of nothing:
   39 entries taken and 21 refused, 12 notes of one leg applied and 48 not (each tried by hand at all four
   rows, two ways), 48 voided, notes of two legs applied and voided by the right leg only: no disagreement).
   Earlier randomized tests that cover stage 1 now, all passing: `t5_b1_the_trace_of_a_draw_and_of_a_call_on_random_shards...`
   (a draw's and a call's end, rows AND prices, with lies: the prices are reached again now that the random
   shard has an opened id), `t1b_r45_the_end_of_a_draw_is_the_models_on_random_books`,
   `t1b_r54_the_end_of_a_call_is_the_models_at_the_edges_and_on_random_shards`,
   `t1b_r54_a_step_under_a_call_is_the_models_on_random_accounts`, `t7_b5_the_gate_on_random_rows...`,
   `t6_b1_at_the_engine_the_models_outcome_is_taken_and_no_other`.
6. **Behaviour the paper does not say**: Q2 (the model's judge), Q3, Q4, Q5; and `Called::list` gives a market
   of two rows its FIRST row's price silently where `Shard::traced` gives no trace (said in the code's comment,
   not in the paper; no script writes two rows of a market).

## The author's mechanical edits in the earlier reviews' files

Checked every changed line of `auditT1_builders.rs`, `auditT3_legtime.rs`, `auditT5_trace.rs`, `auditT6_claims.rs`,
`auditT7_second.rs` (and read `auditT2_vault.rs`, `thin_draw.rs`, `thin_option.rs`). The committee each
`c_enter_state_as` call shows is the one the old code took from the row after (the leg given's, or the world's
market 0's: the same value). Three places had lost their meaning and were restored:

- `t1b_r46...` showed `SEED52` for a shard opened at the engine through an anchor (it failed for that; it now
  shows `shard_seed(&w.anchors[0])` and asserts the shard is of that seed and not of `SEED52`).
- `t1b_fixed_f80_a_leg_enters_a_shard_of_its_own_committee_and_no_other` (it PASSED, with three lines marked "to
  be derived again"): parts (2) and (3) were of a row's committee, which is gone. Its doc now says so: F80 is
  held by part (1) alone (the committee shown opens the id, or the entry is refused), which is whole. (2) is now:
  the row after written with a day, a kind, a strike or a number entered is refused, shown either committee
  (four cases; and the 420 bytes hold the committee nowhere). (3): a row of the leg's key with another scale is
  not the leg's, and the leg enters no empty row beside it.
- `t5_b3_an_entry_no_row_is_of...` (it PASSED): its doc said `KpfTShardPrice` requires a row of the entry's
  name. It does not any more; what the test runs is an entry of a market the committee signs no price of. Said
  in its doc; the other case is Q2.

Not mine to edit, to be reworded by the author: `thin_draw.rs:3674` ("An entry no row of the shard is of: nobody
cites its price") holds only because `[0x7f; 32]` is a market nobody signs.

## The nineteen

All nineteen pass now.

| Test | What it was | What it holds now |
|---|---|---|
| `t1b_r53`, `r54`, `r55`, `r57`, `r58`, `r59`, `r60`, `r62`, `r64` `..._the_sizes_of_...` (9, auditT1) | the sizes of the scripts | the new sizes, as measured: KpfTCStep 5,319, KpfTEnterS 2,966, KpfTLeft 2,914, KpfTLeftAll 5,059, KpfTLeftVoid 2,093, KpfTCallPay 2,567, KpfTShardPrice 2,317. Nothing else in them changed (r54's "what a block takes" passes as it was) |
| `t1b_fixed_f93_whoever_steps_adds_nothing...` (auditT1) | what a step leaves whoever made it, by the script's size: 8,318,600 and -1,671,000 | 8,342,600 and -1,647,000: `KpfTCStep` is 120 bytes less, 24,000 sompi at the least rate. The bounds above it are as they were |
| `t1b_fixed_f69_a_draw_whose_price_nobody_answers...` (auditT1) | a row of a market the committee signs no price of: its price is answered by nobody, the dispute lapses at the claimer's cost | the same, on a shard made by hand that has the id its opening gives it (`.seeded(&SEED52, committee)`): the price of the row the committee DOES price is cited (the control, `tried == 2`), the made row's is not. Before the fix the test failed because NO price of the shard could be cited (an id of chosen bytes) |
| `t1b_r46_a_disputer_of_an_honest_draws_row...` (auditT1) | the owner answers the price of an honest draw right, with one citation | the same; the seed shown is the seed of the shard's real opening (above) |
| `t3_b5_the_sizes_of_the_scripts_that_changed_and_of_an_entry_and_a_step` (auditT3) | sizes of scripts, of an entry stated (5,554) and of three steps (7,355, 7,407, 8,003) | scripts as above, and `KpfTShardOpen` 752 (730). An entry stated 5,550. Steps 7,123, 7,175, 7,771: 120 bytes are the script's; 112 are the test's (Q7). Two rows, two prices: asserted |
| `t3_b4_a_row_is_found_without_its_committee_and_what_holds_one_row_a_market` (auditT3) | (1) a row whose head says another committee: the note is applied all the same; (2) a call whose body has a market in two rows at two prices: whoever steps chooses 30 or 100 KAS | (1) a row says no committee: the 420 bytes hold it nowhere (asserted); the leg's row is the row of its key, and a row of its market with a day is not its row (model here; engine in auditT9 K2). (2) ONE entry a market: the call's object of two heads of one market is the call of the first row's price alone (hashes asserted equal, whatever the second row's limit); the step at the second price is refused named at either place; the right step naming a place the list has not is refused. What review T3 said the change gave up is closed for a call whose body is the model's |
| `t3r2_every_script_that_shows_the_rows_holds_the_41_bytes...` (auditT3) | every script that is shown the rows holds the books' 41 bytes | the same; the hand-made shard of the price's answer has the id its opening gives it |
| `t5_the_sizes_of_the_scripts_and_of_the_transactions_of_a_trace` (auditT5) | sizes | the new sizes; the world's shard seeded |
| `t5_b1_the_trace_of_a_draw_and_of_a_call_on_random_shards...` (auditT5) | model = engine on random books, every point | the same. "The point 5 of the rule's trace" failed because the random shard's id was of chosen bytes (`[0x5d; 32]`), which no seed opens: **not a disagreement of model and engine**. The random shard is seeded; all cases pass |
| `t5_b6_a_futures_row_and_an_options_row_of_one_market_are_two_entries_and_two_prices` (auditT5) | two entries of two names, two prices, one citation | RENAMED `t5_b6_a_futures_row_and_an_options_row_of_one_market_are_one_entry_and_one_price`: two prices have no trace in the model; one price is one entry both rows name, the points 0 to 6 answered right and not wrong at the engine; the old form by hand (two entries of one market) has its second entry answered wrong with nothing cited, and not right; the model names a wrong point of it |
| `t6_the_sizes_of_the_transactions_of_a_claim_and_of_a_calls_end` (auditT6) | sizes of scripts and of transactions | scripts as above; a claim paid 4,511, 4,510, 4,546, 4,558 and a call's end 4,401 (each 56 MORE); a note of one leg applied 4,971, 5,006, 4,954, 4,862 (each 10 more) |

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`, run once at the end, after the last edit of
any test file: **723 passed, 0 failed, 0 ignored** (67 `test result` lines summed). It was 694 passed and 19
failed: the nineteen pass, and ten are new (`auditT9_keys`). By file: `auditT1_builders` 196, `auditT3_legtime`
17, `auditT5_trace` 12, `auditT6_claims` 14, `auditT7_second` 8, `auditT9_keys` 10. (The harness moved the run to
the background after three minutes; no other cargo command ran beside it.)

The paragraph "Stage 1 as it is built" still says "694 tests pass and 19 fail ... theirs to bring along": to be
rewritten by the author with these totals (the reviewer does not edit that document).

## Not reached

- A citation signed by a SECOND committee's table against a shard of the first (the world of these tests has
  one table). The reverse was run (a shard opened for another committee, this committee's citation), and the
  script's check is one equality of 66 bytes of the citation with `push(cmt) + push(market)`.
- Q1 on a CALL stated at the engine (run on a draw; the script and the entry's check are the same for kind 11).
- A key whose day, kind or strike is nothing written below zero (bytes the model cannot say; no script writes
  them).
- The CLI (`kpf ... --seed`): read, not run. No testnet, no network.
- `KpfTShardRow`, `KpfTCallPay`, `KpfTShardOpen` beyond their offsets: covered by the earlier reviews' tests,
  which pass on the 105-byte row (`t5_b1`, `t5_b2`, `t6_*`, `t3r2_*`); no new attack written for them.
