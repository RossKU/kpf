# Review T10: stage 2 of the tree (a shard's books as a tree of its rows), at the script engine

Base: c0ef7bd plus the uncommitted working tree. Reviewer's tests: `crates/kpf-protocol/tests/auditT10_tree.rs`.

## Verdict

**The tree's mechanics hold at the engine; no High finding. Fix U1 and U4 (and the model's U5) before anything is
built on the tree; do not read this as "commit stage 2 as it is".**

The duties changed while the review ran: it was decided that the futures rows go back to four rows written whole
and that the tree will hold the dated rows of options only. So this review is of the tree itself (the path,
`KpfTRows.put`/`.out`, the keys' order through any sequence of puts and outs, `KpfTLeftVoid`'s leaf-before proof,
`KpfTShardRow`'s chain of rows, `KpfTChange`'s counts), duty (A) (the earlier reviews' 82 tests) was dropped part
way, and duty (C) (the workspace run) was not made. What was held, in one line each:

- The path (`both`, `up`): a row opens the root at its place and nothing else does (1,926 other forms refused).
- `put` and `out`: 110 and 43 forms refused one check at a time; 570 random steps honest and not, on sparse and on
  dense books (to a full tree): the engine takes exactly what the rule written from the documents takes, writes
  what the model writes, and the list from the head is every row once in the keys' order after every step.
- `KpfTLeftVoid`: every pair of a leaf and a key, on hand-made and on 90 random books: the rule, **but for U1**.
- `KpfTShardRow`: 200 stated traces (145 lies), 2,416 answers as the model judges; 106 dishonest showings refused.
- `KpfTChange`: 56 forms of other counts and lengths are no statement; the longest (6,072 bytes) is stated.
- No row moves under a statement: the shard a statement stands on is another object.

Findings: U1 (Low to Medium, two forms; the one to fix in the script), U4 (Low today, **the first thing to decide
for a tree of dated rows**), U5 (Low to Medium, the model), U2 (the weights), U3, U6, U7, U8 (Low or notes).
`auditT10_tree.rs`: 20 tests, 20 pass (the last run, of this target alone).

### What carries over to the options' tree, and what it must add

1. **The path, `put`/`out` and the list carry as they are** (no finding). A note for whoever writes the builder:
   the path of the second leaf a script opens is ALWAYS of the books as the first rewrite left them (the two ways
   share the root's halves at the least); there is no case where the path of before does.
2. **U4 becomes live**: `put` holds "a market's futures row is its first" by the key the leaf before names, which
   is right only while a market's other keys are above its futures key. A dated key must be held, in the script,
   to a day, a kind and a strike above nothing and written as numbers are (no `00..80`), or the order has keys the
   rule does not expect. `out` has no rule of its own for a dated row yet (the owner's; "by anyone once the day
   has passed" is the model's comment only).
3. **With the futures rows out of the tree, where a market's scale and limit live must be decided again**: the
   tree's rule is that a dated row has the scale and limit of the leaf before it, of its market, and that leaf
   chain starts at the market's futures row, which `put` writes with them. If no futures row stands in the tree, a
   market's first dated row has no leaf to take them from. (Not a finding: a decision the new plan opens.)
4. **U1 is of the leaf-before proof itself**, not of futures: a key that is a row's by its numbers and not by its
   bytes is proved absent by nothing. With dated keys the three numbers join the market's four. The fix in U1's
   second form covers all of them.
5. **`KpfTShardRow`'s chain and `KpfTChange`'s counts carry as they are.** The model's U5 goes with them. Known
   and unchanged (review T5's R7, met again while `auditT5_trace.rs` was looked at): the OTHER numbers of a trace
   (a row's place, its place of a price, its sums, a call's amount) written as nothing below zero are stated and
   answered as nothing, while `Traced::read` reads no trace of them; only the two counts are held canonical.
6. **U8**: a call's end and a claim read the sum of the gross alone; every script that writes a row of the tree
   must move the sum in the same step.
7. **The weights (U2)**: a way to the root is about 650 bytes of script each time it is written; `KpfTRows` has it
   twice an entry (6,086 bytes). For options the rows are dozens and the tree is the lighter form from about nine
   rows on (17.8d), but a script that rewrites two leaves pays twice.

## Findings

**U1 (Low to Medium; a regression of review 51's F81 in one corner). A note of a leg whose market is, by its numbers,
the market of a row that stands (one number of its id written `00..80` where the row's is `00..00`) is spent by
`KpfTLeftVoid` with NO leaf and NO key, and is applied by nothing.** `KpfTLeftVoid` now proves "no row of the leg's
key" by the leaf before the key and the key that leaf names, both compared with the leg's key as numbers and
strictly (`below`). The twin's key is neither the row's key by its bytes (so the first arm, "the row of the leg's
key", is not taken) nor strictly between any leaf and the key it names (the row of the same numbers is neither
below nor above it). Before the tree the script looked the key up by bytes in the four rows and found none: gone.
`KpfTLeft` does not apply it either (it compares bytes). Such a note stands for good, and a note that stands ends
every draw of the shard (`KpfTDrawNote`: the claimer's bond lost each time), until the real market's row has no unit
and its owner takes it out (then the twin is spent as any key the books do not have: run). Who can write such a
note: nobody by an entry any more (`KpfTEnterS` asks the row by bytes, and `KpfTRows.put` refuses the twin: T9's Q1
is closed at the entry); a statement on an account that nobody disputed can (F81's own premise: it is what
`KpfTLeftVoid` exists for). The model disagrees with the engine here: `Shard::gone_at` says the leg is gone and
`left_void` builds a transaction the engine refuses. Fix (not made): in `KpfTLeftVoid`, a leg whose market has a
number `00..80` (or is nothing) is gone whatever leaf is shown (four comparisons, the ones `KpfTRows.put` has), and
`Shard::gone_at` the same; or the second arm takes "not above" in place of "below" on the left where the bytes
differ. Test: `t10_5_what_a_void_takes_pair_by_pair` (run: every pair of a leaf of the books and a key said is tried
at the engine; none is taken; where the market has no row the twin is spent).

No finding in `KpfTRows` (run: `t10_1` 110 forms of `put` refused one check at a time, `t10_2` 43 forms of `out`,
`t10_3` 360 random steps honest and not, the engine taking exactly what the rule written from the documents takes,
writing what `Books::put`/`out` write, the list from the head every row once in the keys' order after every step),
nor in the path (`t10_4`: on 16 random books of 0 to 63 rows a row opened the root 45 times at its place and 1,926
other forms were refused: another place, 64 + p, below nothing, hashes beside of another length or place, a leaf of
137/139 bytes, a byte of each field, a level up, an inner node's halves as a leaf, the empty leaf where a row
stands, a row at an empty place).

Notes that are not findings:
- `put`: the path of the empty place taken of the books before the leaf before was rewritten is refused at EVERY
  place (the two ways always share a node whose hash moves: the root's halves at the least). There is no case
  "where they do not share".
- `out` on a called shard is taken (run, `t10_2`). It is right: a call is of the rows that have units (the call's
  list has no entry of a row of no unit, no step reads a row, `KpfTCallPay` asks the sum of the gross alone).
- A dated row (made by hand) is taken out by `out` with no rule of its own (the arm is the day's).

More that was run and held (no finding):
- **Entry and leaving** (`t10_6`, `t10_14`): a leg enters the row of its market at the row's place and at no other
  (another market's row, an empty place; shown another row's leaf, the row with another place's path, the row as
  it will be, the head's leaf: refused); it leaves, the row stands at no unit, the root is the one of before the
  entry, and the owner takes the row out; a leaving at another place, at a place of 64 more, or shown the row as it
  stood before: refused. A note of a leg of the market of nothing (scale 0, 1, or the world's) is applied at no
  place, the head's among them, and is spent shown the head. `KpfTCallPay.off` and `.claim` are taken on a called
  shard whose six rows stand at no unit, and refused while a row has a unit.
- **`KpfTLeftAll`** (`t10_7`): two legs of one row (the second shown the row as the first left it: taken; as it
  stood: refused), of rows beside one another, of rows in the two halves, three legs two of one row, six legs of
  six rows, six legs of two rows: applied; EVERY leg after the first shown its row or its path as the books stood
  is refused (a leg before always moved a hash on its way); what is shown a byte less or more, a leg's less or more,
  nothing; the places of two legs changed over: refused.
- **A full tree** (`t10_8`): sixty-three rows put in at the engine; a row more is refused at every place (by the
  model and at the engine, shown what an honest owner would show); a row taken out and another market's put at its
  place; legs enter at the places 62, 0 and 30; a draw whose trace has 63 rows is stated and its rows 1, 2, 31, 62,
  63 answered right (wrong refused). A full tree is the owner's own limit: only the owner puts rows in.
- **Traces** (`t10_9`): on ten random books of 0 to 8 rows at random places, a third of no unit, 200 traces stated
  (145 lies) and 2,416 answers: the engine's verdict is the model's (`trace_row_judged`) at every point, and each
  lie is wrong at the point named: a row left out (first, between, last), a row twice, two rows changed over, a
  place that is empty, the head's place, 64 + p, below nothing, far beyond, a row of no unit with a sum of its own,
  no row said while rows stand, a row said where none stands, a row with units whose place of a price is out of
  range. A row of no unit whose place of a price is out of range is right (read by nothing): as designed (P1).
- **`KpfTChange`** (`t10_10`): 56 forms (lengths; counts a price or a row more or less, 64, below nothing, far
  beyond; 64 with the length theirs; a count of nothing written `00..80`) are no statement, for a draw and for a
  call, the engine agreeing with `Traced::read` on each; the longest (63 and 63: 6,072 bytes) is stated.
- **Ends and prices** (`t10_11`): books of no row and books whose rows stand at no unit: a draw's end is right
  with sums of nothing, a call's wrong, the rows' points right; prices are the points 64 and 65 (the committee's:
  right; twice it: wrong), 66, 67, 126, 127, 1,000 and the largest number are of nothing (right, nothing cited);
  63, 5, 4, 1, 0 are answered by `KpfTShardPrice` neither way; 63 is a row's point (`KpfTShardRow`: beyond the
  trace, right).
- **P1's rest, statements** (`t10_12`): a shard on which a draw (or a call) is stated is ANOTHER OBJECT: its state is
  the statement's hash, and `KpfTRows.put`/`.out` hold the object they spend to the hash of a shard's state. Run:
  the same `put` and `out` that are taken on the shard are refused on the object the statement left (with the
  shard's script and with the statement's). So no row is put in or taken out between a statement and its answer;
  the answers (`KpfTShardRow`, `KpfTShardPrice`, the ends) open the books by the root in the statement's position
  and spend no shard: nothing the owner does after stating makes an honest statement unanswerable or a wrong one
  stand. A call's list has no entry of a row of no unit; `KpfTCStep` steps the leg of the row that has units; a
  row of no unit is taken out of the called shard and nothing reads it.

Run after the duties changed (the tree's mechanics alone):
- **Dense books** (`t10_16`): 210 steps of puts and outs, honest and not, from hand-made books of 62, 50 and 57
  rows of random markets (a third with units): 33 rows put in, 20 taken out, 157 refused, 25 puts into a full
  tree; the engine is the rule, the root written is the model's, the list and the sum of the gross hold after
  every step.
- **A void on random books** (`t10_17`): 90 random books of 0 to 8 rows, a leg of a row (its units, more, another
  scale), of no row, or a row's twin; all 934 pairs of a leaf and a key at the engine. A note is spent by some
  pair exactly where its leg is no leg of the books by the bytes of its key (59 spent, 13 legs of the books spent
  by none), but for U1 (18 cases: no leg of the books, spent by none). U1 is the ONLY gap of the proof found.
- **Whoever answers shows the leaves that stand** (`t10_18`): on a right trace of five rows, the answer "wrong" is
  refused with the true leaves and with 10 other showings a point, for both verdicts (the place said empty with its
  own path or an empty place's, another row and its path, the leaf before said empty or another, the head as the
  row, the two changed over, paths of nothing, the row naming another key, the row said of no unit): 106 showings
  refused; another row's point answered under the disputed one, another root: refused. On a lie (a row twice) the
  answer "right" is refused shown the leaf that does name the row's key (it stands at another place than the trace
  says).
- **A call the whole way over a row of no unit** (`t10_19`): stated (three rows, two prices), its rows and its
  end answered right, applied by `KpfTChangeSettle`: the call's object holds the trace's list (the model's
  `called_at` and the trace agree), `KpfTCStep` steps a leg of each row that has units by it, and a leg of the row
  of no unit has no price in it.

**U2 (Low: the weights; no script is wrong).** `KpfTRows` is 6,086 bytes and a row put in or taken out is a
transaction of 7.9 to 8.0 kB: review T8 reckoned 2.2 to 2.7 kB a script and 4 to 4.5 kB a transaction, and 17.8d's
paragraph "What it weighs" does not say `KpfTRows` at all. A note of two legs applied is 12.4 kB and of six 14.2 kB
(T8's P6 reckoned 10 and 12): the engine takes them (the worst case is ACCEPTED), and it is the heaviest common
transaction by far. A row of a trace answered is 6.3 kB at three rows (reckoned 4.9 to 5.3) and 8.8 kB at 63.
The paragraph should carry these numbers; the table is below.

**U3 (Low: the model and the tool).** `Shard::gone_at` and `left_void` say a twin's note is spent (U1): the model
is not the script there. And `Shard::with_row_for` puts a row in for a leg of ANY head (a scale of nothing, the
market of nothing, a twin id) without `row_in`'s checks: it is the tests' convenience and the earlier reviews'
files now lean on it (`.with_row_for(x).booked(x)`); `row_put` goes by `row_in`.

**U1, its second form** (`t10_15`, run). A note holds its legs as a chain of their bytes, and nothing holds the byte
before a leg's market to `0x20` in a note a statement wrote. A leg of a standing row's market written with another
byte there (`0x21`) has a key that is the row's by its numbers and not by its bytes: spent by no pair of a leaf and
a key (16 pairs tried), applied at no place; spent where its market has no row. So the fix is better said by the
rule than by the twin: **in the second arm, a leaf whose key is neither below nor above the leg's key and is not its
bytes proves the same** (the books have one row a key's numbers: no other row can have the leg's bytes). The
bring-along met U1 too: `t9_k1b` and `t9_k5` (7 of 120 random books) in `auditT9_keys.rs`, marked `// T10 U1`.

**U4 (Low; not reachable today; decide with the day).** `KpfTRows.put` takes a market's futures row AFTER a row of
that market whose day is below zero (made by hand): it holds "a market's futures row is its first" by the key the
leaf before names alone, reckoning the futures key the least of its market, and a day below zero is below it. The
model's `row_in` refuses. No script writes a dated row and `Key::well_made` asks a day above zero, so it needs a
rule only when `put` takes dated rows (then: a dated key's day, kind and strike are above nothing, in the script).
Test: `t9_k1_a_leg_enters_the_futures_row_of_its_market_and_no_other` in `auditT9_keys.rs` (run; marked
`// T10-BUG?:`).

**U5 (Low to Medium: the model, not a script; review T9's Q2 in a new form).** `Shard::trace_wrong_point` judges an
extra entry of the list (a market whose row stands at NO UNIT, so that an honest trace has no entry of it) at the
caller's price for that row's place. A watcher who gives no price there (an honest trace reads none for such a row)
is told the point is wrong although it is priced right, and the engine answers it right: a watcher that disputes by
the model loses its bond. Given the committee's price for that place the model and the engine agree. For an extra
entry of a market of no row the model now names nothing either way (priced at twice the committee's the engine
answers wrong with the citation: nothing reads that price, so nothing is lost by it). Fix (not made): the model
judges an entry no row with units is of by the entry alone (its order, its day), as the comment in
`trace_wrong_point` already says it does. Test: `t9_k3_an_entry_no_row_is_of_is_priced_as_any_other_and_the_model_still_names_it_wrong`
in `auditT9_keys.rs` (run; marked `// T10-BUG?:`).

**U6 (Low; notes on the model, seen while bringing the tests along; none reachable by a script today).**
`books::Books::moved` writes `entered = 0` for a futures row where `Shard::entered`/`left` and the scripts keep
what stands there (two models of one row). A row with something entered and no unit is taken out by nothing
(`out` refuses it; no script clears the number): harmless while nothing writes `entered`, a place held for good
once dated rows do unless every leaving takes it back. `Shard::standing()` sorts by a comparison that is no total
order for two keys equal by their numbers (hand-made books only).

**U7 (Low to Medium: the tool; review T6's K3 is open again).** A shard kept in the form of before the tree
(`[21, terms, four rows whole (420 bytes), moving]`) is not read back by `AObj::decode`: `aobj.rs` now reads a count
where the first row's push was, and returns an error. It is the very case K3 was closed for (what a tool kept before
a change of the form reads back): nothing reads or converts what was kept of a shard before stage 2. On a chain the
old shards are other objects anyway (their state is the hash of four rows, which no script of this stage opens), so
this is a matter for the tool and for testnet's kept files: say in 17.8d that stage 2 opens no shard of before it,
and either read the old kept form (as an error that says so) or convert it. Test:
`t6_k3_closed_a_shard_kept_before_the_three_numbers_reads_back_...` in `auditT6_claims.rs` (run; marked
`// T10-BUG?:`).

**U8 (a note, by design; say it in 17.8d).** `KpfTCallPay.claim` and `.off` read no row: only the sum of the gross
beside the root. An object made by hand whose row has a unit and whose sum says nothing is taken by both (before
the tree each row was held to an empty one). No script writes such an object: "no claim is paid and no call ends
while a row has a unit" now rests on EVERY script that writes a row keeping the sum (`KpfTEnterS`, `KpfTLeft`,
`KpfTLeftAll`: each moves the sum by the leg's size in the same line as the row; `KpfTRows` moves neither: read,
and run in `t10_1`/`t10_2`, "a unit in the sum" refused). Asserted as the engine behaves in
`t6_b1_at_the_engine_...` and `t6_b4_...` (`auditT6_claims.rs`). Also seen there: `Shard::left_class` and `gone_at`
find a leg's row by `row.head() == head` without `!no_market()` (an inconsistency with `row_for`; no way found to
reach it with a well-made leg).

## Sizes

Measured here (`t10_0`, `t10_6`, `t10_7`, `t10_8`, `t10_11`, `t10_13`; bytes of the signed transaction, of the script).

| Script | bytes | 17.8d's paragraph |
|---|---|---|
| `KpfTRows` (put, out) | 6,086 | not said (T8: 2.2 to 2.7 kB each of two) |
| `KpfTEnterS` | 3,573 | 3,573 |
| `KpfTLeft` | 3,732 | 3,732 |
| `KpfTLeftAll` | 10,042 | 10,042 |
| `KpfTLeftVoid` | 3,618 | 3,618 |
| `KpfTShardRow` | 3,985 | not said |
| `KpfTShardPrice` | 2,297 | not said |
| `KpfTChange` | 1,851 | 1,851 |
| `KpfTCallPay` | 2,401 | 2,401 |
| `KpfTShardEnd` / `KpfTCallEnd` | 1,777 / 1,874 | not said |
| `KpfTShardOpen` / `KpfTCStep` | 711 / 5,319 | not said |

| Transaction | bytes | the paragraph / the plan |
|---|---|---|
| an entry stated (`KpfTEnterS.state`) | 6,068 | "about 6,160, not yet measured as one number" |
| a note of one leg applied (`KpfTLeft`) | 5,591 | "about 5,735" |
| a note of two legs applied (`KpfTLeftAll`) | 12,357 | P6: "about 10 kB" |
| a note of three legs applied | 12,807 | |
| a note of six legs of six rows applied (the worst case; accepted) | 14,154 | P6: "about 12 kB" |
| a row put in (`KpfTRows.put`) | 7,897 | T8: 4 to 4.5 kB |
| a row taken out (`KpfTRows.out`) | 7,992 | T8: 4 to 4.5 kB |
| a row of a trace answered, a trace of 2 prices and 3 rows | 6,311 | T8: 4.9 to 5.3 kB at four rows (4,290 before) |
| a row of a trace answered, a trace of 3 prices and 63 rows | 8,767 to 8,769 | |
| a price answered (with its citation), 2 prices and 3 rows | 8,663 | "about 450 bytes lighter" (not compared here) |
| a draw stated on a full tree (a trace of 63 rows, 2,712 bytes said) | 5,889 | |

## The earlier reviews' tests brought along (duty (A): DROPPED part way; what was done before it was)

Kinds of change: (a) a number, a point, an argument's position, a size; (b) a setup of the old form (a leg entered
into an empty row, a shard of no row); (c) a property gone or moved by design (said in the test); (d) a
disagreement of the model and the engine, asserted as it is today and marked.

### `auditT9_keys.rs`: 10 of 10 pass (were 10 failing)

| Test | What it held | What it holds now | Kind |
|---|---|---|---|
| `t9_k1_a_leg_enters_the_futures_row_of_its_market_and_no_other` | a leg enters an empty row; a dated row of its market is not its row and it enters the empty row beside it (Q5); no two rows of one key; another scale or limit, the market of nothing: nowhere | into books of no row the leg enters at no place; the owner puts the row in at the engine (place 5) and the leg enters it and no empty place; Q5 is closed (`row_in` and `put` refuse the futures row beside a dated row of its market) but for a day below zero (U4); two rows of one key refused by entry and by `put`; the rest kept | b, c, d |
| `t9_k1_what_was_entered_is_written_by_nothing_and_a_row_made_with_it_is_stuck_when_emptied` | an entry writes nothing over 7 entered; emptied, the row of no market with 7 is entered by nobody and `KpfTCallPay` never sees four empty rows (Q3) | `put` writes nothing there; an entry KEEPS the 7 (inverted: the scripts keep `row[96..138]`); after the last leg the row stands with its head and 7 entered; stuck in a new way (`out` refuses it); no longer stuck the old way (a leg enters it again, `call_off` is taken) | c |
| `t9_k1b_two_markets_that_are_one_by_their_numbers_enter_two_rows_and_no_trace_over_them_is_right` | both ids enter, two rows, no trace over them is right (Q1) | Q1 closed: the twin gets no row (`row_in`; `put` refused on three shards at four places), its leg enters nowhere; on hand-made books of both rows the old result stands at the point 65 (was 6); U1 met (the name now says the opposite of the test; kept to be tracked) | c, a, d |
| `t9_k2_a_leg_leaves_the_futures_row_of_its_market_and_no_other` | notes applied to futures rows, which are emptied; a dated row is not the leg's; the market of nothing leaves no row and is spent | rows put in at the engine (9 and 2); after the legs they stand at no unit, the root that of before; the old writing (rows emptied) refused; the rest kept at the places 9, 2, 0, 8, 62, 63 | b, c |
| `t9_k3_a_price_is_answered_for_the_committee_the_shards_id_is_opened_by` | the seed and committee checks of `KpfTShardPrice` | every check kept, at the points 64/65 (were 5/6); argument 4 is the root | a |
| `t9_k3_an_entry_no_row_is_of_is_priced_as_any_other_and_the_model_still_names_it_wrong` | an extra entry is priced as any other; the model names it wrong whatever (Q2) | run for a market of no row and for a market whose row stands at no unit; the engine as before; the model: U5 | a, b, d |
| `t9_k3_the_lists_order_is_of_numbers_where_a_markets_top_bits_are_set` | the list in the numbers' order, not the bytes' | the same, rows put in at the engine; added: the trace's rows said in the bytes' order are wrong at row 1 | a, b |
| `t9_k3b_two_rows_of_one_market_with_two_limits_have_no_right_trace` | no right trace | the same, at 64/65 | a |
| `t9_k4_a_calls_step_finds_the_legs_market_plain_in_the_list` | `KpfTCStep` finds the leg's market plain | the same; prices by place; the list has entries of rows with units only | b |
| `t9_k5_an_entry_a_leaving_and_a_void_on_random_books_the_model_and_the_engine` | 60 random books of four rows: entry, leaving, void, the model against the engine | 120 random books of 0 to 6 rows at random places (edges among them), a third of no unit, dated rows, another limit, a twin id: the same three, plus: the row a leg left stands, a note named at another place refused, an entry naming another place refused; every counter held to a threshold; no disagreement but U1 (7 times) | b, c, d |

### `auditT7_second.rs`: 8 of 8 pass (4 were failing)

| Test | What it held | What it holds now | Kind |
|---|---|---|---|
| `t7_b2_the_class_where_a_note_is_applied_is_the_rules_and_no_other` | the note's kind is argument 11, the young's amount 12; said the other kind: refused | the same, the place by the script (`KpfTLeft` 12/13, `KpfTLeftAll` 11/12), and the argument there is held to be the tag | a |
| `t7_b1_i_l4_as_a_game_with_the_gate_and_the_second_class` | a step's note applied as tag 9 taken, as 6 refused; `rows == [EMPTY; 4]` after the last note, then every claim paid | the same; the rows are the shard's own each at no unit, the sum nothing, the rows still stand; the claims paid by `KpfTCallPay.claim` | a, c |
| `t7_b1_ii_a_pair_entered_and_one_side_leaving_leaves_a_row_the_gate_never_saw` | the same two helpers | the same two changes | a, c |
| `t7_b5_the_gate_on_random_rows_is_the_models_and_the_arithmetics` | the gate on random rows, one in four "a row that is new" (an empty row the entry made) | the new row is the market's row at no unit (at least 15 cases held); added: a shard of no row takes no leg (model, and three ways at the engine), and takes it once `row_in` | b, c |

### `auditT6_claims.rs`: 14 of 14 pass (7 were failing)

Its shards were all "of no row"; the author's edit gave them four rows at no unit. Both kinds are run now.

| Test | What it held | What it holds now | Kind |
|---|---|---|---|
| `t6_b1_the_model_is_the_scripts_arithmetic_on_random_shards_and_claims` | the model is the reviewer's writing-out of `KpfTCallPay.claim` (`rows == [EMPTY; 4]`) | the writing-out reads the sum of the gross, as the script; both kinds of shard; a shard with a unit refused by both | b, c |
| `t6_b1_at_the_engine_the_models_outcome_is_taken_and_no_other` | the same at the engine | both kinds; a claim on a shard with a unit refused; shown another root or sum refused; U8's hand-made object asserted | b, c |
| `t6_b2_over_sequences_of_payments_and_coins_...` | five properties over 6,000 sequences | the same, both kinds | b |
| `t6_b4_the_call_ends_where_what_was_paid_is_what_is_owed_...` | argument 4 is the rows; "four empty rows and a sum that is not nothing" refused | argument 4 is the root; the wrong sum refused on both kinds; `off` shown another root refused; U8's object asserted | a, c |
| `t6_b6_a_note_of_the_rest_on_a_shard_that_is_not_called_...` | `rows == [EMPTY; 4]` after the one leg left | gone by design: the row stands at no unit; the old writing (row emptied) is refused | c |
| `t6_the_sizes_of_the_transactions_of_a_claim_and_of_a_calls_end` | sizes | measured again (a claim paid or a call's end 3,845 to 4,002 bytes: 556 less; a note of one leg 5,591 to 5,735: 729 more) | a |
| `t6_k3_closed_a_shard_kept_before_the_three_numbers_reads_back_...` | a kept shard of the old form reads back | the same in today's kept form; the form of before the tree does not read back: U7 | a, d |

A passing test whose meaning the mechanical edit had changed was restored: `t6_b3_...` ("a note of a leg of a market
that has no row is spent") ran on a shard where that market's row stood at no unit; it runs three ways now (the row
at no unit, the market's row out, no row at all).

### `auditT5_trace.rs`: STOPPED part way (4 of 12 pass, 8 fail; the file compiles)

The duty was dropped while this file was being brought along. Changed in it: the shared helpers (`list_of`,
`canon`, `loose` read the two counts from the bytes; `answer_as` takes 1 to 63 for a row and the rest for a price;
new `num_at`, `counts`, `price_point`), and two tests rewritten and passing
(`t5_r1_closed_a_day_of_nothing_below_zero_...`, `t5_r1_the_other_numbers_of_nothing_have_one_way_of_being_read`).
`t5_b3_the_end_answered_right_again_and_again_...` passes through the helpers; `t5_r7_...` passed before. Left
failing, untouched: `t5_b1_...random_shards...`, `t5_b2_...sum_at_the_end_of_64_bits...`,
`t5_b3_an_entry_no_row_is_of_...`, `t5_b4_a_step_under_a_list_of_five_...`, `t5_b4_the_whole_way_...`,
`t5_b5_...length...`, `t5_b6_...one_entry_and_one_price`, `t5_the_sizes_...`.

### Which of the earlier reviews' files this review changed (for whoever returns them to four rows)

| File | Changed by this review? | State as left |
|---|---|---|
| `crates/kpf-protocol/tests/auditT9_keys.rs` | yes, whole | 10 of 10 pass in the tree's form |
| `crates/kpf-protocol/tests/auditT7_second.rs` | yes | 8 of 8 pass in the tree's form |
| `crates/kpf-protocol/tests/auditT6_claims.rs` | yes | 14 of 14 pass in the tree's form |
| `crates/kpf-protocol/tests/auditT5_trace.rs` | yes, in part | compiles; 4 pass, 8 fail |
| `crates/kpf-protocol/tests/auditT3_legtime.rs` | no (the author's mechanical edit only) | as it was: 10 fail |
| `crates/kpf-protocol/tests/auditT1_builders.rs` | no | as it was: 39 fail |
| `crates/kpf-tests/tests/auditT1_tests.rs` | no | as it was: 1 fails |
| `crates/kpf-cli/tests/auditT1_cli.rs` | no | not run |

These four changed files are of the tree's form for FUTURES rows: when the futures rows go back to four rows they
are to be returned to their form of before (commit c0ef7bd has it), not kept. What they showed that is of the tree
itself is in the findings above (U1 met in `t9_k1b` and `t9_k5`; U4, U5, U7, U8) and in `auditT10_tree.rs`.
The bring-along of T9, of T7 and T6, and of T5 in part was done by three helpers under this review's brief, one
after another (one cargo command at a time); their per-test tables are above as they reported them, and their two
disagreements (U4, U5) were not run again by me beyond reading the assertions they left (`// T10-BUG?:` in
`auditT9_keys.rs` at the lines of `t9_k1_a_leg_enters...` and `t9_k3_an_entry_no_row_is_of...`; one in
`auditT6_claims.rs`, U7).

## Not reached

- Duty (A): `auditT3_legtime.rs` (10), `auditT1_builders.rs` (39), `kpf-tests auditT1_tests.rs` (1), and 8 of
  `auditT5_trace.rs`: dropped by the change of duties, left failing as they were.
- Duty (C): the workspace run was not made (dropped). The last run is of this target alone:
  `CARGO_BUILD_JOBS=2 cargo test -p kpf-protocol --test auditT10_tree --jobs 2`: 20 passed, 0 failed.
- `KpfTShardRow` for a call (kind 11) on random books with units: run on one book (`t10_19`) and on books of no
  unit (`t10_11`); the random run (`t10_9`) is of draws. The two kinds differ in the price counted, not in the
  chain of rows.
- `KpfTEnterS` at the place 63 and with a leg of the market of nothing: the builder makes no such statement (the
  script's `r <= 62` and its division by the scale were read, not run); `KpfTLeft` was run there (`t10_14`).
- The tool (`shard-row-in`, `shard-row-out`, `acmds.rs`) and `kpf-cli` were not looked at.
- A price answered was not compared with its size of before ("about 450 bytes lighter" is the paragraph's).
- `KpfTCStep` with a list of 63 against 64 entries (its bound moved from 4 to 63: read, not run).
