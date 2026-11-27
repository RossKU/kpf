# Review T8: the tree in two stages, on paper (17.8d)

Read-only; nothing built, nothing run. Every size below is reasoned from the scripts as they are written, not
compiled: a range, to be measured where it is built.

**Read:** `ARCHITECTURE-CLEARING.md` 17.6, 17.8a, 17.8d, 17.10 (whole); `books.rs` (whole); `reserve.rs` (`Head`,
`ShardRow`, `Shard`, the books' hash, `Priced`, `TraceRow`, `Traced`, `traced`, `trace_row_judged`,
`trace_price_right`); `KpfTEnterS.sil` (whole); the `apply` of `KpfTLeftAll.sil`; the `answer` of
`KpfTShardRow.sil`; the order's comparison and the committee's lookup of `KpfTShardPrice.sil`; the length rule of
`KpfTChange.sil`; the two tests of an options row (`thin_option.rs`, `auditT5_trace.rs` b6); the engine's limits
(`rusty-kaspa` v2.1.0, `crypto/txscript/src/lib.rs`: script and element 1,000,000 bytes, 244 stack items).
**Inferred only:** every byte count of a script that is not built; that no statement stands on a shard while a row
is put in or taken out (from how a statement replaces the shard's state); `KpfTLeft`, `KpfTLeftVoid`,
`KpfTCallPay`, `KpfTCStep`, `KpfTShardOpen` (not read beyond a grep); how many test sites rely on `Shard: Copy`
(counted by grep: 313 `Shard {`, 221 `rows[`, 162 `ShardRow` in 17 files).

## Verdict

**Change first, then build in two stages.** The tree's rules are sound as `books.rs` has them: one leaf shown
does hold one scale and one limit a market, for every sequence of rows put in and taken out (the argument is
under P0). No rule of `put` or `out` needs to change. What must change on paper before a script is written:
a row of no unit must read no price (P1: else an owner makes its shard uncallable with one transaction of its
own); the script builds the key's bytes itself and holds the counts as bytes (P2); a place is held within 0 to 63
before any path (P3); the dated arm of both new scripts waits for the day (P5); the last point is folded into the
last row's own point (P9). `KpfTLeftAll` is the plan's one heavy regression (P6: about half as large again, in
every note of two legs or more); it fits a transaction with room. The row is 147 bytes hashed, not 165 (P4).

## P0. Why one leaf is enough (no finding)

Invariant J: for every market M with a row, its first row in the keys' order is its futures row F(M), and every
row of M has F(M)'s scale and limit.

- The order compares the market's four numbers first, so the rows of one market are next to one another; two
  different ids have the same four numbers only where one has a number written "minus zero", and `well_made`
  refuses those. F(M) = (M, 0, 0, 0) is below every other well-made key of M (an option's day is above nothing).
- A futures row put in: the leaf before is the largest key below F(M), so it is never of M (`same` is always
  false for a futures key: that arm of `put`'s test is vacuous, and harmless). By J no row of M stood without
  F(M), so the leaf after is not of M either. J holds with any scale.
- A row with a day put in: the leaf before must be of M, so F(M) stands and, by J, the leaf before has F(M)'s
  scale and limit, which the new row must have. J holds.
- A row with a day taken out: J holds. F(M) taken out only where the row it names is not of M: the rows of M
  being next to one another, no row of M is left. J holds. `moved` changes no key, scale or limit.

So two rows of one market never have different scales, and a row with a day never outlives its futures row.
The script's test for a futures row taken out (shown the key the row names, plain: its hash is the row's `next`,
its market's 32 bytes are not the row's) equals the model's lookup: the key that hashes to `next` is the next
row's, or the key of nothing, whose market (32 zero bytes) is no row's. A stale leaf-before does nothing: the
shard is one output, and a leaf that does not open the root in its state opens nothing. A full tree: only the
owner fills it (it alone signs a row in); then no new market enters until it takes a row of no unit out;
nothing of a user's is stuck, since no script that changes a row that is there needs an empty place.

## Findings

| # | Severity | What | Scenario | Recommendation |
|---|---|---|---|---|
| P1 | medium (decide before any script) | **A row of no unit still needs a price.** Today a row whose last unit left is emptied by the script that took it; with the tree it stands until taken out, and a futures row only by the owner. "The trace's row as today" asks an entry of the list for every row that has a market | The owner signs one `KpfTRowIn` for a market its committee never signs: no leg, no second signature, nothing a user's tool sees in an account. Any call must then say a price for that row; the owner disputes that price; nobody can cite it; "a price nobody answers ends the call at the caller's cost" (14.6). The shard is uncallable for about 4 kB once. (With a leg of one unit in such a market the same holds today, both keys the owner's: that part is not the tree's and is not closed here. Inferred from 14.6 and `KpfTShardRow`; not run.) Also: 63 dead rows would cost every caller 63 entries, 3.5 kB | **A row whose gross is nothing adds nothing and reads no entry**: the branch a row of no market has today (the three sums are the row before's; the place of its price is read by nothing). The list then has entries only for rows with units, and a dead row costs a caller 40 bytes |
| P2 | low to medium (decide before any script) | **A number of nothing has two forms (F70, R1 again), now in a key and in the two counts.** `Key` in the model holds numbers and cannot say "minus zero"; a script shown 60 bytes can | A key shown as 60 bytes with a day of `00..0080`, put in where M has no row: by its numbers it is M's futures row, by its bytes and its hash it is not. The true futures row can then never be put in (the same numbers: neither is below the other), no leg enters it (an entry compares bytes), rows with a day follow it, and the model and every tool cannot say the tree (R7's kind). Nothing is taken; the market is dead in that shard until the row is out. The counts `e`, `n` written as minus zero pass the length rule and are another statement of the same trace | `KpfTRowIn` takes the market as 32 bytes and the day, what it is of, the strike, the scale and the limit as numbers, and **builds the key's 60 bytes itself**. Every test of "nothing" on a shown row compares bytes (`== i9(0)`), as `called` is tested today. `KpfTChange` holds `said[8..16] == byte[8](e)` and the same for `n` |
| P3 | medium (decide before any script) | **A place outside 0 to 63.** A path's bit is `(place / 2^d) % 2`: place 64 + p opens what p opens, and a place below zero does what `%` does with it | A trace that says place 67 for the row at 3 is "right" at every point: two right statements of one trace (what R1 was). In `KpfTShardRow` a place out of range must give the verdict wrong with no path shown, or a wrong statement has a point nobody can answer | Every script: `require(place >= 0 && place <= 63)` before the loop. `KpfTShardRow`: a trace's place below 1 or above 63 is `ok = 0` with no leaf opened (place 0 is the head: no row of a trace) |
| P4 | low | **The row is 147 bytes hashed, not 165** (9 tag + 60 key + 45 five numbers + 33 next; 17.6 has 147; `books.rs`'s comment and 17.8d counted the scale and limit twice) | Sizes in 17.8d are 18 bytes a row too heavy: a row shown with its path is 138 (the script adds the tag) + a place + 192, about 345 with pushes; six are about 2,070 | Correct the comment and 17.8d |
| P5 | medium (decide before any script) | **The dated arm cannot be tested before the day is built.** "Its day has passed" needs a time shown (an input that landed at or after it, as F105) and a unit of the day; no entry writes a row with a day; how a row with a day is counted in a trace is step 5 | A `KpfTRowIn` that takes a day puts in rows no script can count or enter: reachable on chain, tested by nothing | **Stage 2: `KpfTRowIn` takes a futures key only; `KpfTRowOut` is the owner's only.** `Books::put`/`out` keep both arms in the model (tested there). The dated arms come with the day. The hand-made options row of the two tests is counted as today (at its market's entry of no day), and said so |
| P6 | medium | **`KpfTLeftAll` is the heavy path.** Six blocks unrolled, each with two leaves hashed and a path of six levels verified and rewritten | Script about 7.2 to 7.8 kB (5,093 today); a note of six legs of six rows applied about 11.5 to 12.5 kB (about 7.6 today); **a note of two legs pays the same script: about 10 kB where it is about 7.2 today (+35 to 40 %)**. It fits (100,000 bytes; script limit 1,000,000; the legs' rows and paths as ONE blob, 244 stack items) | Keep the note whole and one transaction (F76 stands; a note that could not be applied would hold its rows' units for ever: no row out, no claim paid, no call ended). Per-leg paths in the note's order, as planned: "a row shown once for several legs" saves arguments only (the six path blocks are unrolled either way) and adds a way to alias. Do not limit the legs. **A test at the engine of the worst case** (six legs, six rows, a called shard, tag 9, a rest) with its bytes asserted. If it measures above about 7 kB, generate a twin of three legs from the same source (script about today's size for notes of two and three legs) |
| P7 | medium (a spike before the migration) | **`Shard: Copy` with sixty-four places is about 8.7 kB** (a `Row` is 128 bytes in memory, `Option<Row>` 136; not 6.5), about 14 times today's. Debug builds give every temporary its own slot | A test function with a hundred `Shard { .., ..*s }` and `OLive<Shard>` values has a frame near 1 MB; test threads have 2 MB, the CLI's main thread on Windows 1 MB: a stack overflow aborts the whole test binary, named by no test. Speed: copying is nothing; **`root()` is 64 leaf and 63 node hashes for a shard of two rows, and `in_order()` hashes keys n squared times** | Keep `Copy` and the sixty-four places (no lie; no churn at 313 sites). Before anything else, one spike: the type alone, the whole suite in debug. If a binary overflows: `RUST_MIN_STACK` under `[env]` in `.cargo/config.toml` (test threads), and the CLI's work on a thread of a set stack. Make `root()` skip empty subtrees by seven constants, and `rows()` sort by key (chase `next` only in the test that checks the list). Fallback if the spike is ugly: `Arc` inside and no `Copy` (loud, mechanical, several hundred sites). **Not** a smaller array in the model: it refuses a ninth row the scripts take |
| P8 | medium | **Stage 1 is worth its commit only if the tests' churn is spent there once.** Its scripts are touched again in stage 2 either way | Without it: every `Shard { rows: [ShardRow { head, .. }, ..] }` is rewritten for keys in stage 1 and again for the tree in stage 2 | Stage 1 brings the tests' API of stage 2: a shard of given rows by a function, a row read **by its market** (`row_of`), points named by `row_point(i)` and `price_point(e)`. Stage 2 then changes no test's rows. Stage 1's review is short and of five things only (below); the full review at the engine is stage 2's |
| P9 | low | **The last point need not be a point.** | A point more means a script more, or an entry more, for one comparison | In `KpfTShardRow`: where `at == n` the row must name the key of nothing, or the verdict is wrong. Where `n == 0`, point 1 is shown the head alone: right where it names nothing. Rows are points 1 to 63, prices 64 to 126, any other point is right with nothing shown |
| P10 | low | **`408` is two things.** `KpfTChange` has `require(said.length <= 408)` (184 + 4 x 56), beside the rows' 408 in seven scripts; the generators patch scripts by text | A replacement of the rows' 408 changes the statement's longest length, silently | Stage 2's length rule is written anew (below); search the generators for the literal |
| P11 | low | Stage 1 changes meaning in places a test may pass through: see "Stage 1" below | | |
| P12 | low | `well_made` lets a key of the market of 32 zero bytes with a day be "well made" (it can never be put in: no futures row of that market is one). The head's place is 0 for ever (opened there, never taken out, no row put where a leaf is), and nothing says so | | `well_made` refuses the zero market; the script does (`market != z32`). A pinned test: `KpfTShardOpen`'s constant is `Books::opened().root()` |
| P13 | low | `KpfTRowIn`'s "the leaf after is not of the new futures row's market" holds by J only | A shard made by any later path that broke J once would take a futures row of another scale before its market's dated rows | One comparison of 32 bytes more in `KpfTRowIn` (the key the leaf before names is not of the new futures row's market): the rule is then local |
| P14 | low | The table of 17.8d does not list `KpfTMarkNote` (it opens the books: 68 bytes) nor say what a called shard does with rows | | `KpfTMarkNote` carries the root where it carries the rows' hash. `KpfTRowIn` refuses a called shard (one comparison); `KpfTRowOut` does not ask |

## The check lists

A row as it is shown, 138 bytes (the script adds `i9(20)` before hashing): `0x20` market [1..33], `0x08` day
[34..42], `0x08` of [43..51], `0x08` strike [52..60], `0x08` scale [61..69], `0x08` limit [70..78], `0x08` net
[79..87], `0x08` gross [88..96], `0x08` entered [97..105], `0x20` next [106..138]. `HEAD60` is `0x20`, 32 zero
bytes, three `i9(0)`; `NOTHING` is `blake2b(HEAD60)`, a constant. A shown row's pushes are not checked again: a
leaf that opens the root was written by a script.

`both(old, new, place, beside)`: ONE unrolled loop of six levels that carries two hashes over the same six
hashes beside and the same bits (`bit = p % 2; p = p / 2`), and returns the root with the old leaf and the root
with the new. It is the only form of a path in any script. The state is opened with the first root (no root
argument); the output is written with the second.

**Key below key**, `lt(a, b)` over 60 bytes: `a == HEAD60`: true (b is never the head where it is called);
`b == HEAD60`: false; else the seven numbers (market [1..9], [9..17], [17..25], [25..33], day, of, strike) in
order, as `KpfTShardPrice` compares five today (`below`, `same`). About 170 to 200 bytes each time it is written.

**`KpfTRowIn`** (the owner signs). Shown: `terms`, `moving`, `gbook` (36), the leaf before `B` (138), `pb`,
`Sb` (192), `knext` (60), `market` (32), `scale`, `limit` (numbers), `pe`, `Se` (192), the signature.
1. `0 <= pb <= 63`, `0 <= pe <= 63`.
2. `market != z32`; none of its four runs of 8 bytes is `00 00 00 00 00 00 00 80`. `K = 0x20 + market + i9(0) + i9(0) + i9(0)` (stage 2: a futures key).
3. `0 < scale <= SCALE_MAX`; `0 < limit <= MARGIN_MAX`.
4. `lt(B[0..60], K)`.
5. `blake2b(knext) == B[106..138]`; `knext == HEAD60` or `lt(K, knext)`; `knext[1..33] != market` (P13).
6. `B' = B[0..105] + 0x20 + blake2b(K)`; `(r0, r1) = both(leaf(B), leaf(B'), pb, Sb)`.
7. The shard's state opens with `r0` (books' hash of `gbook[0..9]`, `r0`, the hash of `gbook[9..36]`); no second key; not called.
8. `R = K + i9(scale) + i9(limit) + i9(0) + i9(0) + i9(0) + B[105..138]`; `(e0, r2) = both(EMPTY, leaf(R), pe, Se)`; `e0 == r1`.
9. The shard again with `r2`, the same `gbook` and moving part, holding no less; the owner's signature; the library's copy goes on.

"Empty in the root as the leaf before rewrote it": `Se` is the path of the empty place in the tree AFTER `B'`
is written (where the two ways meet, its hash is the node above `B'`, not above `B`). Whoever builds the
transaction knows every leaf and always has it: put `B'` in the model, then `beside(pe)`. `pe != pb` and
`pe != 0` follow (those leaves are not empty). (With the day: a dated key asks `B[1..33] == market` and takes
`B[60..78]` as its scale and limit, with no scale or limit shown.)

**`KpfTRowOut`** (stage 2: the owner signs). Shown: the row `R` (138), `pr`, `Sr`, the leaf before `B` (138),
`pb`, `Sb`, `knext` (60), the signature.
1. `0 <= pr <= 63`, `0 <= pb <= 63`; `R[1..33] != z32` (the head stays).
2. `R[78..105] == i9(0) + i9(0) + i9(0)` as bytes (no net, no gross, nothing entered).
3. Where `R[33..60]` is three `i9(0)` (a futures row): `blake2b(knext) == R[106..138]` and `knext[1..33] != R[1..33]`.
4. `(r0, r1) = both(leaf(R), EMPTY, pr, Sr)`; the state opens with `r0`.
5. `B[105..138] == 0x20 + blake2b(R[0..60])`; `B' = B[0..105] + R[105..138]`; `(b0, r2) = both(leaf(B), leaf(B'), pb, Sb)`; `b0 == r1` (`Sb` is the path after `R`'s place was emptied).
6. The shard again with `r2`, the same `gbook` (the sum of the gross does not move); the signature.

Both equal the model's `put` and `out`. Each: a script of about 2.2 to 2.7 kB, a transaction of about 4 to 4.5 kB
(17.8d's "about 700 bytes" is the arguments alone).

**`KpfTEnterS`, `KpfTLeft`, `KpfTLeftVoid`**: `0 <= place <= 63`; `row[0..33] == leg[0..33]`;
`row[33..60]` three `i9(0)`; `row[60..78] == leg[66..84]` (the head, of a scale of nothing, is so no leg's
row); the row rewritten (net, gross; entered as it was); `both(old, new, place, beside)`; the sum of the gross as
today. `KpfTEnterS` loses the branch of a new row (the empty row's comparison, the loop of four names);
`KpfTLeft` no longer empties a row. `KpfTLeftAll`: one blob of 338 bytes a leg (row, place as 8 bytes, path),
`shown.length * 111 == legs.length * 338`, each leg's `both` against the root the leg before left.

**`KpfTShardRow`**, a point `at` of 1 to 63 (`said`: amount, `e`, the list, `n`, 40 bytes a row). Shown
beside today's: the root (32), the row `A` or the word that its place is empty, its path; the leaf before `B`
or the same word, its path.
1. `at > n` and not (`n == 0` and `at == 1`): right, nothing opened.
2. `n == 0`, `at == 1`: the head at place 0 opens the root (required); right where its next is `NOTHING`.
3. Else `pc` = the trace's place of row `at`; `pb` = 0 where `at == 1`, else the trace's place of row `at - 1`. A place below 1 or above 63 (`pb`: below 0): wrong, nothing opened.
4. Required (not a verdict): the leaf shown at `pc` and the leaf shown at `pb` open the root, each a row's hash or `EMPTY`. Either empty: wrong.
5. `B[106..138] != blake2b(A[0..60])`: wrong. `at == n` and `A[106..138] != NOTHING`: wrong.
6. `A`'s gross is nothing (bytes): right where the three sums are the row before's; no entry read (P1).
7. Else as today with `A`'s market, scale, limit, net, gross: the entry at the trace's place is of `A[1..33]`, of its limit, of a day of nothing (bytes); the three sums.

A trace that is right at every point is the list from the head, each row once, to the row that names nothing:
no row left out, none twice, no count needed. "The row before names this row's key" cannot be checked from the
trace (it says places, the keys are in the leaves): the leaf before is shown, about 345 bytes and one path more.
The other form (32 bytes of key's hash in each row of the trace, one leaf a point) moves that cost from the
rare answer into every statement: not taken. A row answered: about 4.9 to 5.3 kB at four rows (about 4,290).

**`KpfTChange`**, kinds 5 and 11: `said.length >= 24`; `e` read at 8, `0 <= e <= 63`, its 8 bytes as
written; `said.length >= 24 + 56 e`; `n` read at `16 + 56 e`, `0 <= n <= 63`, its 8 bytes as written;
`said.length == 24 + 56 e + 40 n`. The longest is 6,072 bytes (63 and 63), not 2.9 kB; with P1 `e` is the rows
with units. Acceptable: one element, carried by each answer; a dispute is rare and the limit is far.

## Decisions on what 17.8d left to the review

- **Stage 1 a commit of its own: yes**, on P8's condition, with a short review of: (1) the list's order (plain
  markets): any test that names a price by its place; (2) the committee by the seed: every hand-made shard whose
  id is not the hash of a seed fails a price's answer (`thin_option.rs` has `sid: [0x5d; 32]`): loud; (3) "an
  entry no row is of is cited by nobody" becomes "by anybody": stricter, and the test turns; (4) a futures row
  and an options row of one market are ONE entry (`auditT5_trace.rs` b6 says so in its own comment and asserts
  two: it is rewritten); the options row's key is `(market, a day, calls, a strike)` made by hand, counted at
  the entry of no day as today; (5) which uses of the tags 13 and 14 are a shard's row's name
  (`KpfTShardPrice`, `Head::of`, `thin_build.rs` 3499) and which an account's own trace's (`KpfTCRow`,
  `KpfTStepRow`: not touched). Stage 1 does not hold one scale a market (four rows, as today): say so.
- **`KpfTRowIn`, `KpfTRowOut`, the comparison, the shared nodes**: the lists above.
- **`KpfTLeftAll`**: fits; whole; P6.
- **`KpfTShardRow` with two paths**: the list above; the last point folded in (P9).
- **An owner that never puts a row in, or fills the tree**: nothing to its users, as said; and P1.
- **Who takes a dated row out, and when**: with the day (P5). Then: anyone once the day has passed and it has
  no unit; the owner at any time it has no unit.
- **Sizes of the common paths** (reasoned): an entry stated about 5.65 to 5.85 kB (5,554: +2 to 5 %); a note
  of one leg applied about 5.0 to 5.2 kB (4,852: +3 to 7 %). The arguments are about 65 bytes lighter; the
  script is 200 to 370 heavier (one `both`, two leaves). A price answered is about 450 lighter. No cheaper form
  worth its cost: depth five saves about 80 bytes a path and loses the rows T6 already found short; a leaf
  without what a futures row never uses saves 36 bytes and makes two forms. What does save: `both` as one
  loop, no root argument, the place as a number. The script and the envelope are the weight, not the rows.
- **S1**: the tree makes the first direction (a higher upper rate on the gross) neither cheaper nor dearer: it
  is a number in the terms, read where the row is already in hand. The second (a leg is young where its row's
  net grew by a leaving within T) is cheaper to store with the tree (9 bytes in the one leaf the leaving script
  holds, where four rows would each carry it) but must be read by the call's step, which reads no row (T10):
  the list's entry would carry the time (8 bytes an entry, held by the row's answer), and it is a rule of one
  row where the requirement of rows with a day is of an underlying's rows together (17.4). It must also be
  decided before the leaf's form is fixed, or every leaf changes again. Recommended: the first direction, with
  the rates; no field kept for the second. (S2's "a number over the rows" is dearer with the tree: a gate over
  all rows is a trace at the entry.)

## Decide before any script

P1 (a row of no unit reads no price); P2 (bytes of nothing; the key built by the script); P3 (the place's
range; wrong with nothing opened); P5 (futures only, the owner only, in stage 2); P9 (points: rows 1 to 63,
prices from 64, the last folded in); the length rule and `e <= 63`; S1's direction (the leaf's form); P7's
spike (the model's form); P8 (the tests' API in stage 1).

## While building

P4 (147); P6 (measure `KpfTLeftAll`; the twin of three legs if above about 7 kB; the worst case at the engine);
P10 (the literal 408 in the generators); P12, P13, P14; `rows()` by sorting and `root()` over what is there;
`SCALE_MAX` at a row put in; the first entry on a market is two transactions (a row in, then the entry: about
4 kB once a market a shard); tests that assert a row is empty after its last leg left now see a row of no unit.
