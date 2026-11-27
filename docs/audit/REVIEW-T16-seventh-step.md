# Review T16: the seventh step, whole (17.8j), on paper

On paper. Nothing was run (no cargo), nothing but this file was written. The design was read at commit `327e619`;
commit `cd0e227` landed during the review and added the paragraph "Step 0, as shown" to 17.8j (the twelve points
did not change): it is taken into Z10.

**Read:** `docs/ARCHITECTURE-CLEARING.md` 17.2 to 17.8e, 17.8g, 17.8j, 17.9 (to review T3's head);
`contracts/KpfTShardRow.sil`, `KpfTShardEnd.sil` (whole); `KpfTEnterS.sil` (the entry's body), `KpfTLeft.sil`
and `KpfTLeftVoid.sil` (the bodies), `KpfTCallPay.sil` (both entries), `KpfTCStep.sil` (lines 96 to 280),
`KpfTCEnd.sil` (196 to 495), `KpfTCRow.sil` (296 to 415), `KpfTChange.sil` (126 to 150), `KpfTPass.sil`,
`KpfTJoinC.sil`, `KpfTEnterOff.sil` (head and the entry's lines); `reserve.rs` (rows, `book_of`, `entered`,
`left_class`, `need_of`, the draw's and the call's ends, `counted_at`, `traced`, `trace_row_judged`, the two trace
ends), `covered.rs` (an entry, `Marked::undone`, what an account owes), `crow.rs` (`ends`),
`tests/research_options.rs` (the shard: `requirement`, `days`, `pays_into`, `call`, `netted`, `level`);
`REVIEW-T14` (O9), `REVIEW-T15` (the verdict, Y1 to Y3). **By grep only:** every line of `contracts/` that reads
bytes 76 to 84 of an account, that compares a row with the empty row, or that reads a row's ninth field.
**Inferred, not read:** `KpfTShardPrice`, `KpfTCallEnd`, `KpfTCEnter`, `KpfTCSplit`, `KpfTLeftAll` beyond the
lines grep gave, `KpfTDay`, `KpfTPoint`, `books.rs` beyond `Key::well_made`, the parked `KpfTRows.sil` (taken
as 17.8d and review T10 describe it), reviews T10 and T11 beyond their tables in the paper.

## Verdict

**Not buildable as written; buildable after five decisions, none of which changes what the paper is for.** The
account's side (8, 9, 12) and the tree's own mechanics stand. What does not stand is where the paper joins
things that were built apart: the four futures rows and the tree (2, 6), the day and the tree's places (3, 4,
10), the account's credit and the shard's books (10). The three decisions that matter most:

1. **A credit must be a number of the shard as well (Z1).** As written, an ended option in the money leaves
   the row, the shard's coins do not move, and what the shard owes for it is in no number a draw or a call
   reads: the owner draws it. The model does not show the paper's rule (its `days` moves coins).
2. **The day's leaf must have its place before the day (Z2), and an ended row with no leaf must not be
   counted at its entry while a price can still be written (Z4).** As written a full tree takes no leaf, price
   or "none"; the owner alone frees a place, at the fallback's time, and writes "none". And in the blocks after
   every day a call counts every ended row at nothing gained and nothing lost: a whole shard is called, a
   broken one is not.
3. **How a trace goes over four rows and then a list has to be said again (Z5), and the scripts that write a
   futures row have to keep its ninth field and its head (Z3, Z6).** The end reads no row and cannot hold the
   count the paper gives it; nothing opens the head's leaf; a futures row of no unit "reads no price" and is
   then in no order; `KpfTCallPay` compares the four rows with four empty rows.

The other findings are sentences to add or correct. The order of building holds once five scripts are moved
into step A and three things are decided before it (Z14).

## Findings

| | Severity | Point | What |
|---|---|---|---|
| Z1 | High | 10, 6 | a credit is in no number of the shard: its equity rises by what it owes, and the owner draws it |
| Z2 | High | 3, 4, 10, 11 | a day's leaf needs an empty place: a full tree takes neither a price nor "none"; the owner chooses "none"; anyone puts in leaves of days no row is of |
| Z3 | High | 2, 11 | `KpfTCallPay` holds the four rows to four empty rows: a count holds every claim, and a row of a day to come is the owner's alone to take out |
| Z4 | High | 6 | from a day to its leaf a call counts an ended row at its entry: a shard that holds an option in the money is called while whole; one that wrote it is not called while broken |
| Z5 | High (buildable only with more said) | 6 | the trace: the end reads no row; nothing opens the head's leaf; a futures row of no unit has no place in the order; review T8's P1 is open again; 88 bytes lack what the order needs |
| Z6 | Medium (High if missed) | 2 | `KpfTEnterS` writes the ninth field as nothing; `KpfTLeft` and its three twins write the empty head and keep the field; nothing writes the empty row when the count falls to nothing |
| Z7 | Medium | 10 | a number below nothing against the scripts: only a step subtracts what is owed; ten scripts refuse or fail on it; `KpfTCStep` fails, so a credit under a call is stepped by nobody |
| Z8 | Medium | 10, 11 | under a call a credit becomes a claim a statement's life after the row is empty: of the second class by T29's rule; 10 and 11 say two things of the note |
| Z9 | Medium | 8 | the premium's return: the one place is `KpfTEnterOff`, and the marked account says neither the amount nor a key; "add" is one number for the user's coins and the owner's |
| Z10 | Medium | 6 | a call by a second price: point 6 and "Step 0, as shown" say two rates; with the first a call is the tolerance of the net easier; with the second it is not "to the rounding" |
| Z11 | Medium | 7 | "a row alone, where the clearing side holds, is asked the rate on the gross only" contradicts 6 |
| Z12 | Medium | 10 (O9) | what an ended leg lost is counted nowhere from its day to its end; "not taken whole" is a shelter while no leaf stands; the day's transaction waits for a gap between statements |
| Z13 | Medium | 4 | a script cannot hold "a day more than T after now": a time said is held not ahead of the chain only |
| Z14 | Medium | order | five scripts missing from step A, two from B, eleven from D; three decisions that must precede A |
| Z15 | Low | 9 | a leg a note needs a note's least a leg; the call's step's note is one by N78, so the script of several legs with paths exists anyway; tag 8 |
| Z16 | Low | 6 | "by a sompi" is the rounding of a price's end, a part in the price by the rate |
| Z17 | Low | 10, 11 | unsaid: `KpfTDayIn` and `KpfTRowOut` on a called shard; `KpfTDayLeg.end` on an account that owes; a join of an account with an ended leg |

### Z1 (high). A credit is in no number of the shard

**The paper** (10): above nothing, "the number of what the account owes goes down by it... a credit, no coin";
"the note to the shard says the leg and what was paid; applied, the row loses the leg". Point 6 says nothing of
a credit in what a shard is asked.

**What happens.** A shard's equity is what it holds, what its rows are worth, less what was entered
(`reserve.rs` 996; `KpfTShardEnd` 161: `has + sn - basis`). Before the day the row carries what the option
gained against the shard. When the note of an ended leg that gained is applied, the row and the basis lose the
leg, `held` does not move, and nothing is added to what the shard owes: the debt has left every number. It
comes back only when the user's take writes its note.

**In numbers.** A shard holds 200. One user holds 10 calls at 100, entered at 100, premium 5 a unit; the day's
price is 130. Before the day's transaction the equity is 200 - 300 = -100. Anyone (the owner) writes the leaf,
ends the leg and applies its note: the account is credited 250, the row is empty, the equity is 200 and the
shard is asked nothing. The owner states a draw of 200: right at every point. The user's take of 250 writes a
note that asks a shard of nothing: a note of the rest of 250.

**The model does not show the rule of the paper**: `research_options.rs` `days` calls `pays_into`, which takes
the coins out of what the shard holds and puts them into the account. Step 0's "a credit in an account through
its day" is that, not "no coin".

**And** a credit's note "of no leg" can be held to nothing by the script that applies it (a mark's note is
held to its leg's two entries): without a number at the shard, a take nobody disputed asks any amount.

**Instead:** "The note of an ended leg says what was credited. Applied, the shard's number K (what it owes by
credits; in the books' hash beside the claims' numbers) rises by it. A draw's end and a call's end count the
equity less K. A note that uses a credit (a take of it; a step's or a leaving's note where it was set against
what legs lost or asked for the user) says how much, and K falls by that, never below nothing. No claim is
paid and no call ends while K is not nothing." (Or the model's form: the note asks, and the shard pays into
the account. Then 17.5's test is back, and Z8 with it.)

### Z2 (high). A day's leaf needs an empty place

**The paper** (10): `KpfTDayIn.price` and `.none` write a leaf where "no leaf of that key stands (the leaf
before)". (4): a row of no unit is taken out "by the owner at any time, by anyone once its day has passed".
The tree has sixty-three places, and only the owner lists.

**What happens.**
- **A full tree takes no leaf.** Sixty-three rows stand (17.6: three days by ten strikes by two is sixty). A
  day comes. No place is empty; the rows of that day have units and leave only by the day's transaction,
  which reads the shard's note, which is written with the leaf. Neither the price nor "none" is ever written:
  the legs of that day never end, a draw is wrong for good (6), and under a call every claim is held for good
  (11). With no malice: a busy shard.
- **The owner chooses "none".** Rows of days to come with no unit are the owner's alone to take out. The owner
  keeps the tree full through the day and the life of the committee's tables (17.8g, A: "within... an epoch or
  two, or never"), then at the fallback's time takes one row out and writes "none" in the next transaction (it
  spends the output of the first: nobody comes between). Users hold 10 calls at 100 and the day's price was
  130: 300 not owed, and the premiums were never the owner's cost. It needs no silence of the committee.
- **Anyone writes leaves of days no row is of.** `.none` asks an input that landed after "the day and the
  fallback's time" and no leaf of the key: any day long past, of any market with a row among the four. Each
  takes a place and raises the count (Z3). Anyone takes them out again; it is a race on one output, paid in
  fees. A user that wrote calls now in the money gains by keeping the day's price out.

**Instead:** "`KpfTRowIn` puts the day's leaf in with the first row of a market and day, saying 'not written';
it is shown that leaf for every later row of the day and refuses where it is written. `KpfTDayIn` rewrites a
leaf that stands (one path; the count does not move). The leaf is taken out with the last row of its day." A
day then has its place from the listing on, no leaf exists for a day no row is of, and no row is listed for a
written day (Z13). Sixty rows and three leaves are sixty-three.

### Z3 (high). `KpfTCallPay` holds the four rows to four empty rows

**Built:** `KpfTCallPay.claim` line 94 and `.off` line 217: `require(rw == none + none + none + none)`, beside
`gbook`'s sum of nothing. (The parked tree had the sum alone: 17.8d, "U8".) **The paper** (2): "a futures row
with a count is not the empty row". Step A lists "every script that shows the rows (33 bytes)" and says
nothing of this line.

**What happens** if only 33 bytes are added: while one dated row or one day's leaf of a market stands, no claim
is paid and no call ends. No row is put into a called shard, but a row listed before the call, of a day a year
on and of no unit, is taken out by the owner alone (4). The owner of a called shard holds every claim by doing
nothing.

**Instead:** "`KpfTCallPay` reads the sum of the gross (and K: Z1) and not the rows. A row of no unit is taken
out of a called shard by anyone, whatever its day."

### Z4 (high). From a day to its leaf, a call counts an ended row at its entry

**The paper** (6): an ended row in a call's trace "is shown the day's leaf, or the leaf before where none
stands, and then counts the row at what its legs were entered at". (17.8g, B had: "where none is written yet
the row is priced as a row of a day to come (the price now)".) A shard under a statement is another object:
no leaf is written between a call's statement and its end.

**What happens.** A leaf is written, at the soonest, some blocks after the day (the committee signs for the
day's time when asked; `KpfTDay.write`; `KpfTDayIn`). A call is stated with the day's own time in the first
block after it.
- **A whole shard is called.** The shard holds 150. Users wrote 10 calls at 100 (the clearing side holds
  them; entered at 100) and are long 10 futures from 100. At 130, a block before the day: 150 + 300 - 300 =
  150, asked about 75 at the lower level. A block after, no leaf: the calls count as nothing, the equity is
  -150, and the call is right at every point. Every account is stepped, every option of every other day ends
  at what it is worth exercised (N48), the caller has its fee. Who gains: a user that wrote options with time
  left; the owner's other key (L4's play with no leg of its own).
- **A broken shard is not called.** The mirror: users hold the calls. What the shard owes them, 300, leaves the
  equity at the day; no call is right until the leaf. With Z2's full tree it never is, and then "none" makes
  it true.

**Instead:** "Where no leaf stands and the fallback's time has not passed, an ended row is counted in a call
as a row within T of its day, at the market's price in the list. From the fallback's time on it is counted at
what its legs were entered at." The answer needs the terms' time and no object.

### Z5 (high: buildable only with more said). The trace over four rows and a list

**The paper** (6): the order is the list's; a futures row carries the hash of the next key; "the end holds
that the last row has no key left, and that the count of futures rows traced is the count of rows with a
market (their places in the list rise, so none is traced twice)"; 88 bytes a row.

1. **The end reads no row.** `KpfTShardEnd.answer` is shown `byte[32] books` and the statement; it reads the
   last row's sums out of what is said. It has no count of rows with a market to compare with, and the 88
   bytes carry no count of futures rows traced. A trace of two markets of four is right at every point.
2. **Nothing opens the head's leaf.** "The head's at the start" is a number the stater writes. A futures
   row's answer is shown four rows and the root; a dated row's answer compares its key with the hash carried.
   A trace whose first carried hash is the key of the thirtieth dated row leaves twenty-nine rows out and is
   right at every row.
3. **A futures row of no unit.** "A row whose gross is nothing adds nothing and reads no price." If it reads
   no place of the list, "their places rise" does not hold it: it is traced twice in the place of a row with
   units (the count agrees). If it must name a place, the list has an entry for a market no leg is in, anyone
   disputes that price, and where the committee does not sign that market the call lapses at its stater's
   cost: **review T8's P1 again**, by a row the owner lists alone (`KpfTRowIn` makes the futures row).
4. **What a market's first dated row is checked against** is not said. It has to be: its key's market is the
   list entry's at the place it names, and that place is the open market's (the row before's).
5. One price a market needs no rising places: the list has no market twice (held at the prices' points), and
   every row with a unit holds its entry's market to its own.

**Instead** (one way that closes the five; 96 bytes a row, 8 more):
- "A trace's row says: the place; the place of its market's price, or none; **which of the four futures rows
  are traced (four bits)**; the sums; the hash of the key the tree names next."
- "A futures row's answer is shown the four rows: its bit was clear in the row before and is set now; every
  row of a market below its own is traced and none above; it closes the market before and opens its own. It
  names a place of the list where a row of its market has a unit (the entry's market is its own), and may
  name none where it has no unit."
- "A dated row with a unit names the open place, and the entry's market is its key's. One of no unit names
  nothing and carries the sums."
- "The first row's answer opens the head's leaf by its path and holds the hash carried to what it names."
- "**The end is shown the four rows** (420 bytes, in an answer that is rare): the bits of the last row are
  the rows with a market; with no row, no row has a market; the last row carries the key of nothing."
- "A place of 64 + 63 is the head's and is no row's" (P3, for the dated rows).

### Z6 (medium; high if missed). The ninth field and the head, in the scripts as they are

Every line that writes or tests the field or the empty row today:

| Where | What it does | With a count |
|---|---|---|
| `KpfTEnterS` 207 | an empty place is 105 bytes of nothing, whole | right as it is |
| `KpfTEnterS` 254 | writes the row with `z9` in the ninth field | **every futures entry sets the count to nothing** |
| `KpfTLeft` 155 to 156, `KpfTLeftC` 174 to 175, `KpfTLeftAll` 162 to 163, `KpfTLeftAllC` 186 to 187 | net and gross nothing: the head is written empty, `row.slice(96, 105)` kept | **the last futures leg leaves a row of no market with a count**: no market enters it, the market's own next leg takes another place, the dated rows have no head |
| `KpfTLeftVoid` (the loop over four rows) | finds a row by key, scale and limit | right as it is |
| `KpfTCallPay` 94, 217 | four empty rows | Z3 |
| `KpfTShardRow` 162 | a row of no market by its 33 bytes | right once the head is kept |
| `reserve.rs` `entered` 619 (`ShardRow::of`: entered nothing), `left_class` 714 to 716 (key, scale, limit cleared; `entered` kept), `row_for` 340, `ShardRow::EMPTY` | the model does the same two things | the same two changes |
| nothing | | **the count falls to nothing on a row of no unit: nothing writes the empty row**, and the place is that market's for good |

**Instead:** "`KpfTEnterS` keeps the ninth field. The four scripts that take a futures leg out write the empty
head only where the count is nothing. `KpfTRowOut` writes the empty row where the count falls to nothing and
the row has no unit. `KpfTRowIn`, where it gives an empty place to a market, holds what `KpfTEnterS` holds of a
market (no number of its id nothing written below zero: T9's Q1)." Four markets a shard, counting the markets
that have only dated rows: said in the paper, right.

### Z7 (medium). A number below nothing, in the scripts as they are

**The paper** (10): "a credit counts for the account wherever what it owes counts against it (every end
subtracts that number: below nothing it adds)".

**Built:** only a step subtracts it. Every line that reads bytes 76 to 84 of an account:

| Script, line | Today | A number below nothing |
|---|---|---|
| `KpfTChange` 133 to 139 | not nothing: only a step (9) and a sale (15) are stated; the bond is of the coins | **an account with a credit states no take: the credit is never taken out** |
| `KpfTCEnd` 203 to 206 | below nothing: wrong; not nothing: wrong but in a step | every end of a take or a leaving wrong |
| `KpfTCEnd` 263 to 265 (a take), 300 to 307 (a leaving) | do not read it | "the take's test counts the credit" is new lines, not a sign |
| `KpfTCEnd` 323, 360 to 362, 391 to 395 | `m + tk - owes`, `m - owes`: a step's tests | adds: right as it is |
| `KpfTCEnd` 436, 457 to 459 | what is owed set against what legs ask, no more than what is asked | below nothing: `asked - owes` adds the credit to the note in the first, and in the second `setoff` is below nothing: to be written, not inherited |
| `KpfTCEnd` 482; `KpfTCStep` 266 | writes `owes + more` | right once `more` is |
| `KpfTCStep` 246 | `require(owes >= 0)` | **the call's step fails: an account with a credit is stepped by nobody under a call, its legs hold the rows, the rows hold every claim** |
| `KpfTCEnter` 155, `KpfTCMark` 166, `KpfTCSplit` 161, `KpfTCTok` 168, 203, 213 | not nothing: wrong | an entry, a mark, a split, tokens out: all wrong beside a credit |
| `KpfTEnterS` 138, `KpfTAddC` 99, `KpfTAddSpot` 94, `KpfTPass` 80, `KpfTJoinC` 92 | `require` nothing | no entry, no coins added, no tokens added, no hand-over, no join |
| `KpfTOwed` 78 | `require(owes > 0)` | right |

**Unsaid:** a split (which account has the credit); a join (the two numbers added: then one of them owing is
set off by the other's credit, which today is refused by "an account that owes is joined to nothing"); a
hand-over (the credit goes with the account: the buyer signs for it).

**Instead:** "A credit counts in a step, a take and an entry, and in nothing else. Beside a credit an account
states what an account that owes nothing states. A hand-over carries it; a join adds two credits and refuses
where either owes; a split leaves it in the account that stays. The call's step sets it into what is asked,
first class (Z8)." And step D lists the scripts above.

A take of a credit against "nothing touches the shard and an account at once": **held.** The take is the
account's statement and writes a note; the shard pays when the note is applied, as a mark's note. What it
needs is Z1's K, so that the note is held to something.

### Z8 (medium). A credit under a called shard

**The paper** (11): "`KpfTDayLeg.end`, whose note is a claim of the first class where it asks". (10): above
nothing the note asks nothing; the credit is taken by a take, a statement. 17.8c: "once anything was paid,
every claim that is counted is of the second class".

**What happens.** The owner holds a first-class claim by an account of its own. A user's account has one
leg, a call that ended 300 in the money, its day before the call's time. The owner ends the user's leg
(anyone's), applies the note (the row is empty: the sum of the gross is nothing), and pays its own claim, in
three transactions of one block once the claims' time has passed. The user's take is a statement: it lives a
lateness and a dispute period before its note exists. Counted then: second class, behind everything. With
other legs the call's step would carry the credit in its note, if `KpfTCStep` took a credit (Z7); with no leg
`KpfTCStep` asks a leg at the least (line 99).

**Instead:** with Z1's K: "no claim is paid while K is not nothing. Under a called shard a credit leaves the
account by one transaction of anyone's, beside the call as a call's step is: the note asks it for the user's
key, of the first class. The call's step sets a credit into what its note asks." And 11's "where it asks"
becomes "its note says what was credited".

### Z9 (medium). The premium's return

**The open question of 8, answered from the scripts.** An entry ends unapplied in three ways (answered wrong
at a point; a lapse; late), and in each the object is the marked account again. One script takes a marked
account: **`KpfTEnterOff.off`** (line 126 holds the mark, `i9(1)`). That is the one place. A call does not
end an entry (`KpfTCStep` 184 takes kinds 7 to 9 and 12 to 15, not 10): it settles or ends as above.

**What the place lacks.** `KpfTEnterS` 273 adds `add` to the coins and writes the mark as `i9(1)` (279). After
the statement is gone no object says how much was added or a key. `KpfTEnterOff` writes "the coins added
staying".

**And "add" is one number.** A user adds 100 of its own with an entry of a written call whose premium is 20:
120. The entry ends late. Returned to "the key the entry says": the owner's key has the user's 100, or the
user's key has the owner's 20 (and whoever holds the signed transaction sends it late, for nothing).

**Instead:** "An entry of an option the user writes adds its premium exactly, and nothing else (the user adds
coins apart, by `KpfTAddC`, before). The mark of its account says the amount and the key where it says 1
today. `KpfTEnterOff` pays that key the amount, in an output (above a note's least or not at all: then it
stays, and is said), before the keeper's fee." Whose coins pay a premium is the signers': right as written;
the account's test holds the amount whoever paid.

### Z10 (medium). A call by a second price

**The paper** (6): "a second price within the lower rate of it". **"Step 0, as shown"** (since `cd0e227`):
"within that rate of the first (the account's lower rate less the tolerance), and a futures shard is called
where it is today, to the rounding".

**In numbers** (one futures row, the clearing side long, worth 1,000 at the stated price; maintenance 5 %,
tolerance 0.5 %; the gross apart). Today (`call_end_judged`: counted at the price a tolerance up, the lower
rate 4.5 %): called where the equity at the stated price is below 4.5 % of 1,005 less 5: **40.2**. Short:
**39.8**.
- Point 6 as written (4.5 %): below **45**. A call is 0.5 % of the net easier, and the room between a draw at
  the edge and a call is gone: an owner that draws all the rule gives with its price stated a tolerance its
  way leaves the true equity at 5 % of the net, and a caller that states a tolerance the other way calls
  anything below 5 %.
- "Step 0" (4 %): below **40**, both ways. Not today's: 0.2 either way a thousand (the maintenance rate by the
  tolerance of the net), and the gross is counted at the stated price where today it is counted a tolerance
  off. Tests of a call's edge move by more than a sompi.

**Who gains by the choice.** The caller chooses; the test is exact at the price chosen, so nothing is gained
beyond finding the worst price, which is what a call is for. A held row whose strike is between the two
prices is counted at what it is worth there, not along a line: a call is never due where a draw's level
holds. A false call is not easier but by the rate (above) and by Z4. A true call is no harder.

**Instead:** one sentence in 6, the rate named: "a second price within the maintenance rate less twice the
tolerance of the first". And in step 0: "a futures shard is called within the maintenance rate by the
tolerance of its net of where it is today".

### Z11 (medium). The gate of a row alone

**The paper** (7): "A row alone, where the clearing side holds, is asked the rate on the gross only." **By
6** a held row is asked what it loses at an end where its strike is outside the ends, and what it is worth
exercised now where its strike is between them.

**In numbers** (the price 103, the rate 5.5 %, the rate on the gross 2.25 %). An account writes 1,000 calls
at 80: the clearing side holds them, deep in the money. At the lower end, 97.3, the row loses 5,665; by 6 it
is asked 5,665 and 2,317. By 7's sentence: 2,317. An owner's own account enters that leg into a shard that
holds 2,317, and the shard is 5,665 below its level from that transaction: L4's play at a third of the gate.
(Young at a call, the leg asks nothing: the second class bounds it. The gate was built so that it never
enters.)

**Instead:** "A row alone is asked its two sums as 6 computes them, the larger lost, and the rate on its
gross."

### Z12 (medium). O9 as decided

**The paper** (10): in a step an ended leg adds its premium and nothing else; the account is not taken whole;
"what an ended leg lost waits for the price or the fallback".

- **The ended leg lost, the others lose more.** Coins 400. A call written, 10 at 100, premium 2: at its day
  the price is 130, it lost 300. A long of 10 from 100. A block before the day the account is counted with
  the 300. From the day on it is counted without: the due test is 400 and what the long gained against 20.
  The long falls 15 %: 250 against 20, not due. In truth 400 - 300 - 150 = -50: the shard is unpaid 50 and no
  step was ever right. **The loss is not unpaid by "not taken whole"; it is unpaid by not being counted.** It
  lasts from the day to the leaf, the end and nothing else; with the committee answering, blocks; silent, to
  the fallback's time; under Z2, for good.
- **The ended leg gained.** Its gain is not counted either: the account is stepped leg by leg sooner than it
  need be. Each leg leaves alone, and the credit, when it comes, is set against what is then owed. Nobody is
  frozen: right as decided.
- **No price is ever written.** "None": the leg ends at its entry. A written option's loss is forgiven
  (N72); the account is then taken whole as any other.
- **A shelter.** A leg of one unit, far from the money, its day past: the account is "not taken whole" until
  somebody has written the committee's note, the leaf, ended the leg and waited for the account to be free.
  Leg by leg a due account of five legs is five statements' lives. A join carries the ended leg into the
  user's main account, with no test (Z17). It costs a premium of nearly nothing. It ends when anyone ends the
  leg: the keeper's fee has to pay for three transactions, of which one is 5.7 kB.
- **`KpfTDayLeg.end` asks an account not under a statement.** Between a user's statement and the account's
  time the gap is the time 14 leaves a step: held. But a step is anyone's to state: the user's other key
  states steps that are not due, back to back; each stands until it is disputed and costs a bond when it is.
  Nobody is paid to dispute them but whoever wants the leg ended.

**Instead:** "In a step an ended leg with no leaf yet is passed by a row that adds its premium and, where its
market's price in the statement has it out of the money for its user, what it loses there (never what it
gains): until the fallback's time; after it, the premium alone." (It counts by the price now, as Z4's rule
does for the shard; the price now is stated and answered already for the market's other legs, and for a leg
alone in its market it is a price more.) Or keep the rule and say the hole: "from its day to its end an ended
leg's loss is counted nowhere". And: "`KpfTDayLeg.end` takes an account under a step's statement of a time at
or after the leg's day, and ends the statement, as a call's step ends one."

### Z13 (medium). "A day more than T after the time the transaction says"

A time said is held not ahead of the chain (an input that landed at or after it); nothing holds it not
behind. A statement's time is held by its lateness, by whoever disputes; `KpfTRowIn` is no statement. So the
owner lists a row of a day that has passed, by saying an old time.

**What it gives:** nothing alone: no leg enters it, since the account's entry is a statement and a late one
is ended. With a late entry nobody ended: an option of a day whose price is known (hold 10 calls at 100 of a
day written at 130: a credit of 300 less the premium). That is "a statement nobody disputed", as everywhere;
but here the row is what the watcher would have seen missing.

**Instead:** Z2's sentence (a row is refused where its day's leaf is written), and: "a day's leaf is written
'not written' by `KpfTRowIn` only; `KpfTDayIn` asks an input that landed at or after the day": the two
together hold on the chain what a time said cannot.

### Z14 (medium). The order of building

Each step is testable alone with books and accounts made as objects (as reviews T9 and T15 did), with these
corrections:

| Step | Missing from its scripts | Why |
|---|---|---|
| A | `KpfTChange` | it holds a trace's length (184 to 408 by 56 today; 17.8d: its two counts) |
| A | `KpfTCStep` | it reads an entry of the call's object as 56 bytes (line 127); with 64, no account is stepped under a call after A |
| A | `KpfTCallPay` | Z3 |
| A | `KpfTEnterS`, `KpfTLeft`, `KpfTLeftC`, `KpfTLeftAll`, `KpfTLeftAllC` | Z6: more than 33 bytes |
| A | "futures amounts move by the rounding" | not "every test passing": the tests of amounts and of a call's edge are changed in the commit (Z10, Z16) |
| B | `KpfTEnterOff`, `KpfTEnterS` (the mark) | Z9 |
| C | nothing; but `KpfTCStep` comes back in D | a credit (Z7, Z8) |
| D | `KpfTChange`, `KpfTCStep`, `KpfTCEnter`, `KpfTCMark`, `KpfTCSplit`, `KpfTCTok`, `KpfTEnterS`, `KpfTAddC`, `KpfTAddSpot`, `KpfTPass`, `KpfTJoinC` | Z7 |
| D | the scripts that apply a note with a credit, a draw's and a call's end | Z1 |

**Decided before A, because A writes them once:** what the books' hash is of (K: Z1; five things, not four);
the trace's row and what the end is shown (Z5), with the field `KpfTShardRowE` will need (Z4: the terms'
time, no field); whether `KpfTRowIn` puts the day's leaf in (Z2). **Decided before B:** the note's shape for
an option and for a credit (T14 said so; Z1, Z15). Step 0 is not done by its own list: a call by a second
price, and a credit as the paper has it (no coin), are not in the model.

### Z15 to Z17 (low)

- **Z15** (9, the open point). A leg a note: an account holds one note's least (`KpfTCEnd` 444: `NOTE_MIN +
  pay`); six notes are six, and "whoever steps adds nothing" (F93). And the call's step's note is one by
  rule (N78: the young legs together; tag 9), so a script of several legs with paths for a called shard
  exists whatever is chosen. So: one script and its twin; or, lighter, "a step of an account not under a call
  names one leg where the account holds an option" and only the called twin takes several. Unsaid: the note
  that an option did not enter (tag 8) is applied by `KpfTLeftD`.
- **Z16** (6). `crow::ends` rounds the rate on the **price** up. A futures row asked by its ends differs from
  today by a part in the price by the rate: a sompi a unit where prices are of nine digits, a tenth where a
  price is 10. Say: "by the rounding of the price's end".
- **Z17.** `KpfTDayIn` takes a called shard (else 11's "until the day's price is written" is never);
  `KpfTDayLeg.end` takes an account that owes (D6) and one with a credit; a join cannot refuse an ended leg
  (a script shows that a day has passed, never that it has not): said, with Z12.

## The questions of the brief, answered

**(11) One ended leg holding every claim: how long, and who.** Until the leaf, the end and the note's
applying. With the committee answering: three transactions. Silent: the fallback's time, "two epochs at the
least" (the number is not in the paper; it is a term to say to a user). With Z2: for ever. With the account
kept under steps (Z12): a statement's life a round more. On purpose: any user, by one unit of any option
whose day is before the call (the cost of its premium); the owner by an account of its own; the owner alone
by Z2 and Z3.

**What an owner with accounts of its own keys can take.**

| By | What | Verdict |
|---|---|---|
| listing rows | fills the tree: Z2. A row of a market the committee does not sign: Z5, 3 | open |
| entering both sides of a series | the row is netted to the gross; the owner draws; its writing account must then be **due** to leave (no option leaves by its user's statement): narrower than S1 for futures. Then a call, and the users' options end at what they are worth exercised | N48, said in the paper; not new |
| holding in one account and writing in another through a day | one leaf a shard: D1 closed. The credit of the one is paid by the shard whether the other paid or not: bounded by what a written option is asked (a move of the rate), as two futures legs | held, with Z1 |
| the day's price late | Z4 (a call), Z12 (an account) | open |
| the day's price never | N72 as said; and Z2 without the committee | open |
| "none" | every premium is its user's: the owner gains only what it owed in the money | as said |
| a draw just before a day | the rows are within T: no sum above nothing; counted at the price stated; the move to the day's price is a move like any other | held |
| a call just after | Z4 | open |

**The shard's requirement by rows against what the legs can lose.** Two users' legs in one row, netted: the
clearing side loses nothing at any price or at the day; what is left is that one of them does not pay, which
the rate on the gross is for, as for futures (S1's rate is open there as here). A row the clearing side holds:
along the line of nothing between the ends, exact outside: never less than is lost (step 0 shows it on
200,000 books; not re-run here). Near the day: no sum above nothing, the row's and not the leg's: right, and
the requirement of a hedged book jumps at T before every day by the clock (N83, said). An ended row: Z4.
Against `research_options.rs`: its `level` with `rows` is point 6; its `requirement` (leg by leg, `asked_flat`)
asks no less. The gate: Z11.

## What can be left out of the first form

The goal's row is "futures, spot and options netted in one account under a shard". The netting it names is the
account's, and is built (17.8h, 17.8i).

| Left out | What it saves | What it costs |
|---|---|---|
| **netting the rows of a market at the shard** (the two sums over rows, the open market, a call by a second price): each row asked alone, summed | most of point 6: a trace's row is today's and a hash; the call stays as built (`counted_at` has its meaning a row); the gate (7) is the requirement itself; Z5 shrinks, Z10 goes | the owner's coins: a book of calls written against futures held is asked both. Nothing of a user's |
| `KpfTDayCopy` | a script | one leg of a shard and day ends a block |
| a note of several legs with a day, on a shard not called (Z15) | a script of some 10 kB | an account with options is stepped a leg a statement |
| counting an ended leg in a step (Z12's first form) | lines of `KpfTCRow` | the hole, said |

**Not to be left out:** the tree (a series is a row); the day's leaf and "none" (else a freeze); the credit
or 17.5's test (else D7's dead rows); the twins for a called shard.

**What the paper leaves out that the goal's row needs:** the shard's side of a credit (Z1). And, for the yardstick
of use rather than for the row: a user cannot close an option before its day but by handing the account over
or by writing the same series beside it, which holds both premiums and two of six legs to the day ("a leg that
cancels another of its series" is left out in 17.8).

## Attacked and held

| Tried | The rule that refuses it |
|---|---|
| a dated row traced twice, or left out between two others | each leaf names the next key; the hash carried is the row's key (with Z5, 2 for the start) |
| two prices for one market | the list has no market twice; a row's entry is of its market |
| a dated row traced under another market's futures row | with a unit its entry's market is its key's and its place the open one (Z5, 4); with none it adds nothing anywhere |
| a place of 64 + p read as p | P3, kept |
| a second row of a key; a futures leg of another scale into a market that has dated rows | `KpfTEnterS` 206 to 210: the row is the head's or an empty place where no row has the name; the head is there while the count is (with Z6) |
| two ids that are one market by their numbers (T9's Q1) through `KpfTRowIn` | held if `KpfTRowIn` holds what `KpfTEnterS` 215 holds (Z6) |
| a row emptied with something entered (T9's Q3) | 4: "no unit and nothing entered"; what a leg adds and takes is computed from its own bytes both times |
| a row taken out under a leg that is entering | the leg is in the row from the transaction that states its entry |
| a take of a credit touching the shard and the account at once | a note, as a mark's (Z7) |
| a premium paid in used as margin | `KpfTCEnd` 265: the coins hold the premiums (T15, Y1) |
| an option leaving by its user's statement, or marked | `KpfTCRow` 325 to 327 |
| an entry within T of the day by a late statement | the lateness of a statement (and Z13) |
| a hand-over, a join, a split reading a leg's way as 1 or 2 | `KpfTPass` and `KpfTJoinC` read no leg's bytes but to hash them; a split's rows are `KpfTCRow`'s (12 holds; `KpfTCSplit` by grep only) |
| a join that asks less than the two apart by more than netting | what a group is asked is the larger lost of two sums: joined, never more than apart; legs2 after legs1, so only the last group of one and the first of the other meet |
| the second price of a call chosen to make a false call | exact at the price chosen (Z10, but for the rate) |
| a draw over an ended row | wrong while it has a unit |
| the owner paid twice by "none" | D4 kept: a written option's premium stays its user's |

## A check list to build from

The twelve points as lines to tick, corrections in (the finding in brackets).

1. **The books' hash**: of the sum of the gross of every row; the hash of the four futures rows; the root of
   the dated rows; the hash of the claims' numbers **and K** [Z1]. Opened with a constant root. Every script
   that shows the rows is shown the root.
2. **The head.** The ninth field of a futures row counts its market's dated rows (with Z2: its rows, the day's
   leaves among them or not, said once).
   - [ ] `KpfTEnterS` keeps the field [Z6].
   - [ ] `KpfTLeft`, `KpfTLeftC`, `KpfTLeftAll`, `KpfTLeftAllC`: the empty head only where the count is nothing [Z6].
   - [ ] `KpfTRowIn`: the market's row among the four, or an empty place given to it (a market that is one:
     Q1), the row's scale and limit; the count rises.
   - [ ] `KpfTRowOut`: the count falls; at nothing with no unit, the empty row [Z6].
   - [ ] `KpfTCallPay`: the sum of the gross and K, not four empty rows [Z3].
   - [ ] The model: `entered`, `left_class`, `row_for`.
3. **A dated row** is the parked leaf. "Of" 3 is the day's leaf, strike nothing, the last key of its day; it
   holds "not written", a price, or "none" [Z2]. `Key::well_made` takes it.
4. **`KpfTRowIn`** (the owner): not called; the key well made; the leaf before and the key it names; an empty
   place; **the day's leaf of the row's market and day shown: put in "not written" where none stands, refused
   where it is written** [Z2, Z13]. **`KpfTRowOut`**: no unit, nothing entered; the owner, or anyone once the
   day has passed, **or anyone where the shard is called** [Z3]; the day's leaf with the last row of its day.
5. **An option leg to the books**: as written.
6. **The requirement and the trace.**
   - [ ] A trace's row, 96 bytes: the place (0 to 3; 64 to 126), the place of its market's price or none, the
     four bits, the two sums, what the closed markets are asked, the gross's worth, the clearing side's worth,
     the next key's hash [Z5].
   - [ ] A futures row's answer: the four rows shown; its bit; the order of the markets; closes and opens; a
     place of the list where its market has a unit [Z5].
   - [ ] A dated row's answer: the leaf by its path; the key is the hash carried; with a unit, the open place
     and its market's entry; of a day to come, the two sums as written (within T no more than nothing); of an
     ended day in a draw, wrong where it has a unit.
   - [ ] The first row's answer opens the head's leaf [Z5].
   - [ ] The ends are shown the four rows: the bits, the key of nothing [Z5]. Less K [Z1].
   - [ ] A call: a second price within the maintenance rate less twice the tolerance [Z10]; 64 bytes an entry;
     `KpfTCStep` reads 64 [Z14].
   - [ ] `KpfTShardRowE`: the leaf's price; no leaf and before the fallback's time, as a row within T at the
     list's price; from then on, at its entry [Z4].
   - [ ] `KpfTChange`: a trace's length by its two counts [Z14].
7. **The gate**: the row alone asked its two sums as 6 computes them and the rate on its gross [Z11].
8. **An entry.** `KpfTCRow` as written. `KpfTEnterSD` as written. A written option's entry adds its premium
   exactly; the mark says the amount and the key; `KpfTEnterOff` pays it [Z9].
9. **A leaving before the day.** `KpfTLeftD`, its twin, `KpfTLeftVoidD`; tag 8 by `KpfTLeftD`; one script of
   several legs for a called shard at the least [Z15]. A note says how much of a credit it used [Z1].
10. **The day.**
    - [ ] `KpfTDayIn.price`, `.none`: rewrite the leaf that stands "not written" [Z2]; an input that landed at
      or after the day; a called shard as well [Z17]; the shard's note, tag 23.
    - [ ] `KpfTDayLeg.end`: an account under no statement, or under a step's of a time at or after the day
      [Z12]; owing or not [Z17]; the note says what was paid **and what was credited** [Z1].
    - [ ] A credit: in a step, a take, an entry; the scripts of Z7's table.
    - [ ] O9: an ended leg in a step, as written or with what it loses at the price now [Z12]; said either way.
11. **Under a call.** `KpfTCStep`: an option of a day after the call's time at the call's price; refuses a
    leg of a day at or before; **takes a credit into what it asks** [Z7, Z8]. A credit of an account of no
    leg: one transaction of anyone's [Z8]. No claim while a row has a unit **or K is not nothing**.
12. **Without a price**: as written; a hand-over carries a credit, a join adds two and refuses an owing one,
    a split leaves it [Z7].

## Not reached

`KpfTShardPrice`, `KpfTCallEnd`, `KpfTCEnter`, `KpfTCSplit`, `KpfTDay`, `KpfTPoint` and the parked
`KpfTRows.sil` were not read line by line: what is said of them rests on the paper and on grep. The sizes (a
trace's row answered with a path and four rows; the stack of `KpfTEnterSD`) were not reckoned. The length of an
epoch and the fallback's time in blocks were not found. Step 0's two new tests were read, not run. The second
class beside option legs (a young option under a call: 17.9 with a premium) and the band of an option at a
call were not walked. Token legs beside an ended leg (T15's Y1 and Y2 with "an ended leg" in the place of "a
token") were reasoned from `KpfTCEnd` 343 to 398 only: the `tk != 0` branch is the one O9 has to share, and
whether every line of it is right for an account with no token to sell was not walked.
