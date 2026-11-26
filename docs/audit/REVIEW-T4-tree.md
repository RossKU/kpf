# Review T4: the books in a tree, and a draw and a call as traces (17.6, 17.7), on paper

27 November 2026. A review of a design that is not built: `docs/ARCHITECTURE-CLEARING.md` 17.6 and 17.7, which
are steps 3 and 4 of 17.8. Findings are numbered T1 to T18. Nothing was run, built or changed; this file is the
only thing written.

**Read whole:** 17.4 to 17.9, 14.1 to 14.9, 16.22, 17.1 to 17.3; `crates/kpf-protocol/src/reserve.rs`;
`KpfTEnterS`, `KpfTCStep`, `KpfTCallPay`. **Read, the entry that matters:** `KpfTLeft` (apply, rest),
`KpfTLeftAll.apply`, `KpfTLeftVoid.void`, `KpfTShardEnd.answer`, `KpfTCallEnd.answer`, `KpfTShardPrice.answer`,
`KpfTMarkNote.apply`, `KpfTChange.state`, the first half of `KpfTChangeSettle.settle`, `int8` and `sm8`.
**Signatures only** (what each carries of the shard): `KpfTShardOpen`, `KpfTDrawNote`, `KpfTDrawEntry`,
`KpfTCallEnter`, `KpfTOwed`, `KpfTAddC`, `KpfTPoint` (and the three lines of its bound in time), `KpfTEnterOff`,
`KpfTOpenC`. **Not opened:** `KpfTCRow`,
`KpfTCEnd`, `KpfTCEnter`, `KpfTCMark`, `KpfTStepPrice`, `covered.rs`, `research_options.rs`, any test. Where a
point rests on one of those it says "by review T3" or "not checked".

**Reasoned with numbers:** T1, T6, T8 (sizes: estimates from the compiled lengths in `contracts/artifacts`, not
measured), T9, T12, T13, T15. **Read only:** the rest.

## Verdict

**The tree itself is sound in kind and can be built, but not as step 3 is written, and eight things have to be
decided before any script** (the first list at the end; two more before the traces and the claims). The ordered list of leaves does what N55 asked of it (no key twice, and a row that
is not there can be shown not to be there, which `KpfTLeftVoid` needs). What does not stand as written:

- **Step 3 is not separable from step 4 in that order** (T16). `KpfTShardEnd` and `KpfTCallEnd` read every row.
  With a tree and no trace they have nothing to read. Traces do not need the tree; the tree needs traces. So the
  statements' new form comes first, on today's four rows, and the tree after it.
- **The head leaf is not below every key** under the comparison 17.6 names (T1): half of all market ids are
  numbers below nothing.
- **A trace held by its root alone is disputed blind** (T9): `KpfTChange` says so itself, in its comment on
  why what is stated is shown and hashed there. This is the one finding of 17.7 that changes its shape.
- **17.7 no longer says that no claim is paid while a row has a unit** (T12). With "a share of all that was ever
  held" and that condition gone, whoever is shown first is paid whole.
- **L4 is narrowed by the second class, not closed** (T15): the owner has a bet that pays one way, of any size.

Nothing found lets anybody take coins from the tree as such. T9 and T12 would, built as written.

## What holds (reasoned, no finding)

- **The place of a leaf is in its path only, and that is enough.** No script writes a leaf but at a path it
  checked against the root in the shard's state that it spends. A leaf cannot be moved or written twice by
  anything but a script's own error; binding the place into the leaf would not prevent that either.
- **A stale leaf-before does nothing.** Its path opens against no current root. "No key twice" rests on the
  list's invariant (every leaf that is not empty is in the list, in order): a key that is there has exactly one
  leaf whose next is its hash, and the strict `key < key after` fails for it.
- **No row is taken out from under a pending draw or call.** A statement spends the shard and stands in its
  place (`KpfTChange.state`: `becomes`), and holds the shard's whole state in `position`. Every answer rebuilds
  `position` from the terms, the books and the moving part, so a row is answered against the root of the
  statement's time, kept there. After a call is applied its object is a copy of prices; a step reads the call,
  not the shard. Rows may leave a called shard freely.
- **A user cannot make a row.** `KpfTEnterS` takes the owner's signature (`s2` against the account's second key,
  which line 135 holds to the shard's owner). Sixty-three rows of its own keys harm the owner only: nothing a
  user holds needs a new row to leave.
- **Nothing pins the fixed part across a call.** Accounts, notes and the call name the shard's id. `KpfTAddC`,
  `KpfTOwed`, `KpfTMarkNote`, `KpfTDrawNote`, `KpfTDrawEntry` and `KpfTChange` (its `form` of 122 bytes) carry
  32 bytes for the rows today and carry the books' hash the same way. `KpfTLeft.rest` shows 408 bytes it does
  not read: with the tree it shows 32.
- **A draw is held up no longer than today.** The bound of 14.1 is a time (`t0 + late + window`, the three
  lines of `KpfTPoint` that hold it), not a number of points. More points give a griefer nothing but more bonds
  to lose.
- **A call's time ahead of the chain** would make a young leg old. It is closed: `KpfTChangeSettle` applies
  only where `landed >= t0` (and review T3 ran it).

## Findings

### 1. The tree (17.6)

**T1 (High; it stops the build).** *The head leaf is not below every key, and the order has ties.* Keys are
"compared as numbers of 8 bytes (the market's 32 bytes as four)". A script's number of 8 bytes is little-endian
with the top bit as its sign (`int8`). A market's id is a hash: the first chunk of half of all ids is below
nothing. The head's key is nothing. For such a market no leaf is below the key, so no leaf-before exists and
the row cannot be put in: a shard cannot list half of all markets. Second, `00..00` and `00..80` are both the
number nothing: two ids that differ so compare equal, and neither can be put in beside the other (harmless: an
id made so shuts out only its twin).
*Recommendation.* The head is below every key by rule, not by comparison: the leaf-before passes where its key
is the head's (all nothing; no row has a scale of nothing) or is below the new key. Keep the comparison as
numbers, chunk by chunk, the first that differs deciding: it is a total order on what the numbers are, the rows
of one market are next to one another whatever the id (the market leads), and there is no cheaper one (a script
has no order on bytes). "No row after" is 32 bytes of nothing in the leaf.

The checks of a row put in, to hold the script to (P the leaf before, at place j; k the new key; e the empty
place; R the root in the state):

1. P's hash with its path at j gives R. P's key is the head's, or below k.
2. P's next is nothing; or the key after is shown, hashes to P's next, and k is below it.
3. R1 is the root with P written again, its next the hash of k. An empty leaf with its path at e gives R1
   (so e is not j). R2 is the root with the new leaf at e: k, nothing of units, P's old next.
4. Where P's market or the key after's market is k's: its scale and its price's limit are k's (T2).
5. The owner signs (T5).

A row taken out: the row L (at i, no unit, not the head) gives R; R1 with an empty leaf at i; P at j gives R1
and P's next is the hash of L's key; R2 with P's next L's next. **The key after is not shown at a removal**: L's
next is copied as a hash (78 bytes less than the table says).

**T2 (Medium).** *What "an underlying" is, is not said, and the key lets one market have two rows of futures.*
The key holds the scale and the price's limit. Today `KpfTEnterS` refuses a second row of a market's name
whatever its scale (the loop over four rows). With uniqueness by key alone, a market M has a row of limit 20
and a row of limit 1,000. At a committee's price of 30 the first is capped at 20 (`KpfTShardPrice`: the price is
the smaller of the committee's and the limit), the second is not. If an underlying is the market, every draw
and call of that shard is wrong at one of the two rows. If it is the market with its scale and limit, one
market has two prices in one trace: an owner's own long in the row that follows the price and its own short in
the row that stops at 20 (1,000 units each, entered at 15, the price 30) ask 15,000 and pay 5,000, both legs
old. That is N55 again.
*Recommendation.* An underlying is the market's 32 bytes. One scale and one limit a market in a shard, held
where a row is put in by its two neighbours (check 4 above: rows of a market are next to one another, so the
neighbours are enough).

**T3 (Medium).** *A leaf "holds what its legs were entered at", and "what the legs were entered at stays one
number over all rows".* Both are in 17.6. `KpfTMarkNote` moves the one number and reads no row ("it is applied
whatever the rows hold by then"). A long of 10 units from 10 is marked at 12, then leaves at 12. The leaving's
note is applied first: the leaf, which never saw the mark, is left with 20 at no unit, and anyone takes the row
out. If a mark's note must write the leaf, it now has none: it is applied by nothing, its 20 KAS are never
paid, and a note that cannot be applied ends every draw (F81's kind). If it does not write the leaf, the
leaf's number is wrong from every mark on.
*Recommendation.* The leaf's number is kept **for rows that have a day only** (it is what 17.5's fallback
counts, and no mark names an option: 17.4); a futures row writes nothing there, and the one number in the
moving part stays the only one for futures. `KpfTMarkNote` then reads no row, as built. Say it in 17.6.

**T4 (Medium).** *The count is a number every script that moves a gross must move at the crossing, and one
that is wrong is wrong for ever.* The scripts: `KpfTEnterS` (up, also on a row of no unit that is still
there), `KpfTLeft.apply` (tags 6 and 8), `KpfTLeftAll.apply`; `KpfTShardOpen` writes nothing; the two scripts
of a row put in and taken out do not move it; later the leaving of an ended leg, if it writes the row itself.
`KpfTLeftAll` is the trap: a long of 5 and a short of 5 of one market leave in one note, the row's gross goes
10, 5, nothing; counted against the leaf as first shown, each leg sees "5 left" and the count does not fall,
or both see the last 5 and it falls twice. One too many: the call never ends and no claim of that shard is ever
paid. One too few: claims are paid with units standing (T12's harm).
*Recommendation.* **Hold the sum of every row's gross, not the count of rows.** The same 8 bytes, the same
test (nothing), and every script adds or takes the leg's size with no crossing to see: `KpfTLeftAll` cannot get
it wrong. (An entry that would take the sum past a script's numbers is refused.) Nothing else reads the count:
a trace says its own number of leaves, which is another number (rows of no unit are in the list). Either way,
the model holds "the number is what the leaves say" after every operation on random books.
Also: a row with a net and no gross (books a wrong note left so) blocks `KpfTCallPay` today, which asks for
four empty rows. By the gross alone it does not. Say that a row is taken out by its gross only.

**T5 (Low).** *Who puts a row in and who takes one out.* Putting a row in costs a second path and two
comparisons of nine numbers. Inside `KpfTEnterS` that code is in every entry's transaction, used or not (T8).
*Recommendation.* A script of its own (the owner signs; a row of no unit), so an entry always touches a row
that is there. Then a row put in ahead of its first entry must not be anyone's to take out: **a row with a day
is taken out by anyone once its day has passed; a row with no day by the owner's signature.** Who pays for dead
rows: the owner, who needs the places; anyone may, for a passed day, so a tool can.

**T6 (Medium).** *Sixty-three rows, and the example does not fit across a day.* The head takes a place. Three
days of ten strikes, calls and puts, and the futures row are 61 rows and the head: 62 of 64. When a day rolls,
20 new rows are wanted while the 20 of the day that ended still have units (an ended leg that gained stays
until its account stands without it: 17.5). 81 rows.
*Recommendation.* Depth seven for the first form (224 bytes a path, one level more), or say the example as two
days and a third rolling. And the depth as a constant of the scripts, not a number of the terms: the 90 bytes
of terms are the accounts' as well (`KpfTEnterS` hashes both with them), and a depth read from them means
every script walks to the largest depth.

### 2. The books' hash

**T7 (Medium).** *"The count, what was paid, the root" is not what the books' hash will stay.* 17.7's two
classes need what was paid to each and what is owed to the second: three numbers more. The moving part has no
place (171 bytes, all in use; the second key of nothing is how `KpfTChange` and `KpfTLeft` know a shard). If
step 3 fixes the books' hash at three fields, step 4 rewrites every script step 3 wrote. And `KpfTMarkNote`,
which "carries 32 bytes", must open the books on a called shard where its leg is young.
*Recommendation.* The books' hash is of the sum of gross, the root, and **a hash of the claims' numbers**
(what was paid first, what is owed second, what was paid second; nothing while the shard is not called). A
script that moves a row carries those 32 bytes unopened; only what counts or pays a claim opens them; a third
class later (17.3's deterrent) changes no row script. The call's end writes them as nothing.

### 3. Sizes

**T8 (Medium).** *The table counts what a script is shown and not the script, and the script is in the
transaction* (a step of one leg is 6,890 bytes of which `KpfTCStep` is 4,829). Estimated from today's compiled
lengths (a statement of an unrolled loop is about 20 bytes: `KpfTLeftAll` 3,873 against `KpfTLeft` 2,220), a
path checked and rewritten at depth six is **450 to 700 bytes of script**, one only checked 250 to 400. Not
measured: a probe of one path would say.

| Transaction | Today | With the tree as written | Why |
|---|---|---|---|
| an entry stated on a row that is there | 4,902 | about 5,200 to 5,500 | 30 to 50 bytes less shown (a leaf, its place and its path are 338, the books' other fields 18 to 40, against 408), one path more of script, the loop over four rows gone |
| the same, the row put in by `KpfTEnterS` itself | 4,902 | about 6,200 to 7,000 on **every** entry, 280 more where the key is new | a second path and two comparisons, carried whether used or not |
| a note of one leg applied | 4,002 (14.10) | about 4,350 to 4,600 | as the first row |
| a note of two to six legs applied | not measured | 2,700 to 4,200 more of script on every one; with six rows 1,600 more shown | six paths unrolled. The table has no row for `KpfTLeftAll`; every call's step of an account of two legs writes such a note |
| a note voided | not measured | a leaf, or the leaf before and the key after with the comparison | the table has no row for `KpfTLeftVoid`; "no row is its market's" is shown by the ordered list |
| a claim paid; the call ended | 3,454; 3,347 | about 3,300; 3,000 | 408 bytes less, the arithmetic of two classes more |
| a row taken out | none | about 3,000 to 3,500 | 600 shown (no key after), two paths of script |
| a call's step | 6,890 to 7,538 | T10 | |

So "about 60 bytes less where a row changes" is **about 300 to 600 more**: the common path is heavier than
today by 6 to 15 %, by the yardstick's own count. That is the price of more than four rows; it should be said.
With a row put in by a script of its own, an entry stays at the first row of the table. Not checked: whether
one push may be as long as a trace of sixty rows (an entry's `said` is one push of up to 847 bytes today).

### 4. The traces (17.7)

**T9 (High).** *A trace held by its root is not on the chain, and a wrong one cannot be disputed at its wrong
row.* `KpfTChange`: "What is stated and what the position becomes are shown as they are and hashed here:
whoever watches reads them on the chain (a statement that could not be checked could only be disputed blind)."
An owner states a draw of 500 KAS where the rule gives 100, by the root of a trace whose row 37 of 60 says a
worth of nothing. Whoever watches builds the right trace from the leaves and the prices written and sees
another root. It knows the draw is wrong and not where: the end, shown the claimer's last row, is answered
right; rows 1 to 36 are answered right. A point guessed wrong costs a bond; thirty guesses on average. With
options it is worse: a held option is counted "as the row says", so even a right trace has no one root, and
nobody but the caller can build a path for `KpfTCStep` at all.
*Recommendation.* **Whatever holds the trace is computed by the script that states it, from bytes in that
transaction.** The form that needs no new machinery is today's for an account: the rows whole in `said`.

- `said`: the amount; N, the number of leaves; E; **a list of E prices**, each a market, its limit, a day
  (nothing: the price now) and the price, 56 bytes, in the keys' order, no market and day twice; then N rows of
  about 48 bytes: the leaf's place, its price's place in the list, and the running sums (the three of today
  for futures; 17.4's later). At four rows and four markets 440 bytes; at sixty-three and eight, 3.5 kB: a
  draw or a call stated is about 0.4 kB more than today at four rows and 3.5 kB more at sixty-three (17.7
  has 3.8 kB at any count).
- Point 0, the end: the last row's sums against the rule, the outputs, and (a call) that the call's object is
  the list. Points 1 to N, a row: shown the leaf at the row's place and the leaf at the place of the row before
  (the head for row 1), each with its path against the root in `position`: the second's key hashes to the
  first's next; the price is the list's for the leaf's market (its day where that has passed, else the price
  now); the sums follow from the row before's. Point N + 1: the last leaf's next is nothing. Then E points, a
  price each: within the tolerance of the committee's (the shard's id opened by the seed), and above the entry
  before it in the order. 2 + N + E points; 5 today.
- A row answered, estimated from `KpfTShardEnd`'s length and what a call's answered end weighs beside its
  script: about 5 kB at four rows, 8 kB at sixty-three (two leaves and paths 680, two paths of script, `said`
  whole). If that is too much, `KpfTChange` hashes the rows in eight parts and an answer opens one or two:
  about 6 kB at any count. Decide that from a measurement. (17.7 has 5 kB for a row answered by a root.)

"One price an underlying" is then how a row finds its price, and no rule of its own.

**T10 (Medium).** *The call's object as a root costs every step, and "a row for each underlying" lets the
stepper choose.* 17.6 says a path "for each underlying of the account's legs", 17.7 "for each leg". By
underlying, a futures leg of M may be shown the trace's row of an option of M whose day has passed, and leaves
at that day's price: a long of 100 units, the price now 12, a day's price 15: 300 KAS more, at the choice of
whoever steps (its own account). So it is a row a leg, the whole key held. Then six paths are unrolled in
`KpfTCStep`: 1,500 to 2,400 bytes on **every** step; a step of one leg about 8.4 to 9.2 kB (6,890 today), of
six legs and six rows about 10.4 to 11.3 kB (7,538).
*Recommendation.* **The call's object is the list of T9**, hashed flat as the body is today, shown whole to a
step with a place a leg: 56 bytes a market (and a day that passed). Four markets: 224 bytes against today's
368, no path, no script more. A leg finds its entry by its market, its limit and its day. With no entry twice
(a price's point holds the order) whoever steps has no choice under a call that was looked at; under one nobody
looked at it has, as with any false statement. The root is lighter only above some thirty-eight entries: cap
the markets of a shard (a number, counted where a row is put in, by the neighbours).

**T11 (Medium; for step 5, but it shapes the list).** *"Where nobody has written the price a call's trace
counts the row at what its legs were entered at" (17.5) meets "a price nobody answers ends the call at the
caller's cost" (14.6).* A day's price the committee never signed: the caller says "at its entry"; the owner's
other key disputes that price; nobody can answer; the call ends at the caller's cost. Every call on that shard
ends so: the shard N67 did not want.
*Recommendation.* A price said as none is right unless someone shows the day's note or the committee's price
of the day, and its point is answered right by anyone with nothing shown. A lapse on it does not end the call.

### 5. Claims (17.7)

**T12 (High, as written).** *Paid before the books are empty, whoever is shown first is paid whole.* 17.7's
conditions for a payment are the time (the call's, a lateness, two dispute periods) and nothing else; today's
"once no row has a unit" is only in the call's end. A shard holds 100; the owner's account (an old long that
gained 100) and two users' (100 each). The owner steps its own account, applies its note: owed 100, T 100, paid
whole and spent. The users' accounts were under statements of before the call, which stand for a lateness and
two dispute periods and are stepped after: owed 300, T 100, held nothing: each part is 33.3 and each is paid
the smaller of that and nothing. At one share each had 33.3.
*Recommendation.* **No claim is paid while a row has a unit**, as today, and the time as well. Then every note
that takes a leg out is applied and counted before the first payment. What can still be counted late is a
mark's note of before the call that nobody applied: it stands behind what was paid, its own holder's loss; say
it.

**T13 (Medium).** *A claim below a note's least keeps a shard called for ever, and "after every first claim is
paid whole" hands the second class to whoever makes dust.* "A part of less than a note's least is not paid";
"the call ends where D is C". Read as it stands, the first covers a claim that is itself below the least. A
called shard holds 1,000 KAS, owes 400.001: the 0.001 is one account's old leg that gained a tick. Everything
else is paid; that claim is below 0.2 KAS for ever; D is never C; the owner's 600 KAS stay in a shard that is
called and gives no draw. Any user does it for nothing. And if the
second class waits for D1 to be C1, it waits for ever too, or for every dust claim an owner's old accounts
made to be shown by someone at a transaction each.
*Recommendation.* Where what was ever held covers what is owed, a claim is paid whole and spent whatever its
size (what is below a note's least is whoever pays it's, as a step's small payments are). The least holds only
for a part. And the second class is paid **from what is held beyond what the first is still owed**, not after
the first is shown paid. Written out, with H held, C1 and C2 owed, D1 and D2 paid, W = H + D1 + D2 (17.7 calls
it T, which is the time of 17.4 and 17.9: another letter):

- First class, a claim (A, a): its part e is A where W is at least C1, else A by W over C1 (the unit chosen
  from C1; C1 up, W down). Paid: the smaller of e less a and H.
- Second class: S is W less C1; nothing where S is not above nothing; e is A where S is at least C2, else A by
  S over C2. Paid: the smaller of e less a and H less (C1 less D1).
- The parts of a class sum to no more than there is (each is rounded down), so the second bound holds only by
  rounding. A claim says 0 <= a <= A or pays nothing: a note is not taken at its word (F86).
- The call ends where no row has a unit, D1 is C1 and D2 is C2; the three numbers and what is owed are written
  as nothing.

**T14 (Medium).** *Where a claim gets its class.* 17.7 says "at a leaving or a mark stated after the call's
time". No script that counts a claim knows when its statement was stated (a note says its legs, a key and an
amount), and the rule so said misses a step of a liquidation stated before the call that names a young leg
(L1's narrow path). What the applying scripts are shown is each leg whole, the call's time and the periods.
*Recommendation.* **A claim's class is decided where the note is applied to a called shard, by its legs' times
against the call's**, for every kind of note:

| Where the claim is born | Class |
|---|---|
| a note of the rest written before the call | first: it is in what is owed already |
| the call's step (`KpfTCStep`) | two amounts in the one note: what the old legs gained (with what the young lost together, as built) first; what the young gained together, less the band, second |
| a note of one leg (a leaving, a liquidation's step), a mark's note | by that leg: young at the call, second |
| a note of two to six legs | second, whole, where any leg is young; else first |

The last row is harsher than 17.7 and costs nobody who uses a tool. A leg young at the call entered after the
call's time less 2 (L + D); its account states nothing until 2 (L + 2 D) after the entry (by review T3; 14.2):
so such a note is of a statement at least 2 D after the call's time, when the call is on the chain wherever a
lateness is no more than two dispute periods (100 and 50 in the tests), and its user steps its own account
instead. Do **not** make it "stated after the call's time, whole": an owner that says the call's time a
lateness back and applies a user's leaving of old legs the moment it stood would push that user into the second
class.

- **Nothing of the second class is set off**: not against what the old legs lost (as built: "an old loss paid
  beside a young gain"), not against what the account owes (F112's set-off stays for the first amount). An
  owner's old long that lost 100, and a young short of 10,000 units beside it that gained 130 beyond its band:
  set off, the account pays nothing and the users' first claims lose the 100.
- One note of the rest carries both classes (the key, A1, a1, A2, a2): a block takes one note beside the shard.
- 17.9's bullets still say "nothing, and no second claim" and "an owner does not gain by bringing it about":
  superseded by its own table; correct them with step 4.

**T15 (Medium).** *L4 with the second class, in numbers: the owner has a bet that pays one way.* Review T3's
play: a shard of 20 KAS (19.9 after the caller's fee), a user long 5 units from 10, young, the price 13; the
owner's own long of 1,000 units. Tolerance 0.5 %, so the band is 1 % of what a leg is worth.

| Between the owner's entry and the call's time | The owner's leg, second class | The user's | The user is paid |
|---|---|---|---|
| no move, honest prices | nothing | 14.35 | 14.35, whole: what a leaving had paid |
| no move, both edges the owner's (12.935; 13.065) | 130 less 130.65: nothing | 14.67 | whole |
| up 1 %, honest prices both | 130 less 131.3: nothing | 14.99 | whole |
| up 1 %, the entry at its edge | 63.7 | 14.99 | 3.79 |
| up 1 %, both edges | 128.7 | 15.32 | 2.12 |
| up 2 %, both edges | 258.0 | 15.97 | 1.16 |
| up 1 %, both edges, 10,000 units | 1,287 | 15.32 | 0.23 |
| down 1 % | it pays the shard 65, and has them back when the call ends | 13.71 | whole |

The owner loses nothing when the price goes against its leg and keeps most of what it owed when it goes its
way; the size is its own to choose (5.5 % of it as coins, which it has back); it says the call's time within a
lateness, after seeing the prices of that lateness; and it keeps other callers out meanwhile by statements of
its own on its shard at half a bond a dispute period. Against "nothing" (today: the user loses 15 for certain)
the second class is much better. It is not "an owner is not rid of it".

- **The band is needed**: without it the two edges alone are a claim of 130 at no move, and the user is paid
  2.1 for certain.
- **Of which worth**: of every young leg, each alone (not the young legs' net), **the larger of its worth at
  its entry and at the call's price**, rounded up. Of the call's price alone a short keeps the tolerance's
  square twice (0.65 KAS a 13,000 KAS of size, at no move): 1.3 million KAS of short is a claim of 65 on the
  second class for certain.
- **L3 adds to it**: the owner's call a lateness back makes a leg of up to T and a lateness young (400 blocks
  in the tests). Said as it is; with the second class it costs such a user the bet above, not everything.
- I found no rule that closes it without a gate at an entry. An order by age within the second class would,
  and one share cannot say it. *Decision for the owner of the design:* take the second class as the first
  form's, print this table in 17.9, and have the model play it (an owner that enters, holds its shard and
  calls at the best price of a lateness) so that the number said to a user is a measured one.

### 6. The order

**T16 (High for the plan).** *"The tree, futures only: every test of today passing" has no ends.* With a tree
`KpfTShardEnd`, `KpfTCallEnd`, `KpfTShardPrice`, `KpfTCStep` and `KpfTCallPay` have no four rows. To keep them
until step 4 the tree would be held to four rows and each end shown the head and four leaves with five paths
(1.7 kB shown, some 2 kB of script, in two scripts that step 4 throws away). The table's own row "a draw's
end: 50 bytes" is true only once the draw is a trace.
*Recommendation.* Turn the two steps round, in four commits, each with today's tests meaning what they mean:

1. **The books' hash, on today's four rows**: the sum of gross, the hash of the rows where the root will be,
   and the claims' hash (nothing yet). `KpfTCallPay` asks the sum to be nothing and, for this commit only, the
   four rows empty as well: the two must agree in every test. About forty bytes more where the rows are
   shown, none where 32 bytes are carried, no rule changed. This is the smallest first commit, and it proves the number T4 is about where it can still be
   compared with the rows.
2. **The draw and the call as traces, on four rows** (T9's `said`; the call's object as the list; a script
   that answers a row from the 408 bytes). `draw_end_judged` and `call_end_judged` stay in the model as the
   reference: on random shards the trace's verdict is theirs.
3. **What a called shard has paid, and the two classes** (T12 to T15).
4. **The tree**: the leaf, a row put in and taken out, one path in `KpfTEnterS`, `KpfTLeft`, `KpfTLeftAll`,
   `KpfTLeftVoid`, and two paths in the row's answer. The ends, the prices, the step and the claims do not
   change. In the model first, with the list's invariant held on random books.

### 7. What contradicts what

**T17 (Low).** (a) T3: the leaf's number and the one number. (b) 17.6's table: a top-up carries 32 bytes, not
50; "the key after shown where a row is taken out" is not needed. (c) 17.7's T and 17.4's T. (d) 17.4 says no
option enters within T of its day, 17.5 within a statement's life of it: 300 and 200 blocks; one of them. (e)
17.9's bullets against its table (T14). (f) 17.6 says a call ends where the count is nothing; 17.7 adds that D
is C. (g) An entry's `said` says "the shard's row the leg takes": with a tree it says the place, or nothing;
keep the 8 bytes so that `KpfTCEnter`, `KpfTCRow` and `KpfTEnterOff` read `said` where they do (not checked in
those scripts). (h) 14.4's fallback (a pending leg in two numbers a row) is not in the leaf "as it will stay":
say that it is dropped, or keep 18 bytes for it now.

**T18 (Low).** *Sixty rows behind one output.* Every entry and every note of a shard spends its one output; a
signed entry dies when any note is applied before it lands, as today. Four markets become sixty series on the
same one output. No coin is at risk; it is what a user feels first. Say it, and measure it on testnet before
the options are listed.

## A decision for each open point

| Open point | Decision |
|---|---|
| `KpfTShardPrice` and the committee (17.6) | open the shard's id by the seed, as `KpfTStepPrice` does: 64 bytes more on a rare path. The point is of an entry of the price list |
| the call's object and `KpfTCStep` (17.6) | the list of prices, flat, not a root (T10) |
| who pays for dead rows (17.6) | the owner; anyone may for a passed day; the owner alone for a row with no day (T5) |
| is the place bound into the leaf | no; the path is enough |
| a stale leaf-before | nothing to do |
| a row taken out under a pending draw or call | cannot happen; a row is answered against the root in `position` |
| the order of keys | as numbers, the head below all by rule (T1); an underlying is the market alone, one scale and limit a market (T2) |
| the count | the sum of gross (T4) |
| sixty-four leaves | sixty-three rows; depth seven, or a smaller example; the depth a constant (T6) |
| the books' hash | the sum, the root, a hash of the claims' numbers (T7) |
| what a trace is in bytes | whole in `said`, with the list of prices (T9) |
| when a second-class claim is payable | from what is held beyond what the first is still owed (T13) |
| what if C1 grows late | it stands behind what was paid; only a mark's note can (T12) |
| L1 | the class at the applying, by the legs' times (T14) |
| L4 | the second class less the band, said as a bet the owner has, with T15's table |
| L3 | said as it is |
| "less the band": needed, of what | needed; each young leg's larger worth, rounded up (T15) |
| F114 | T13's arithmetic, T12's condition |
| step 3 before step 4 | no: T16's four commits |

## Decide before any script

1. The order of T16, and its first commit.
2. The head below every key by rule (T1).
3. An underlying is the market; one scale and limit a market (T2).
4. The leaf's "entered at" for rows with a day only (T3).
5. The sum of gross in place of the count (T4).
6. The books' hash with a hash of the claims' numbers (T7).
7. A row put in by a script of its own, and who takes one out (T5).
8. The depth: six, seven, and a constant (T6).
9. Before step 2 of T16: the trace on the chain, and the call's object as the list (T9, T10).
10. Before step 3 of T16: no payment while a row has a unit; the two formulas; the class at the applying; no
    set-off; the band's worth (T12 to T15).

## Can be decided while building

- The numbering of points, and whether the list's end is a point of its own or the last row's.
- Rows hashed whole or in eight parts (from a measured answer at sixty rows).
- The cap on a shard's markets.
- A price said as none (T11), with step 5.
- `KpfTLeftAll`: one script of six paths, or one for up to three rows and one for more; legs of one row sharing
  a path.
- The 8 bytes of the row in an entry's `said`; 14.4's fallback; the letters and the stale lines (T17).
- What is said to a user of one output a shard (T18).

# Second round: 17.8b

27 November 2026, the same day. Under review: `docs/ARCHITECTURE-CLEARING.md` 17.8b (a draw and a call as traces,
on four rows), with 17.8a and 17.10. Findings T19 to T28. Nothing run; this section is the only thing written.

**Read whole for it:** `KpfTPoint`, `KpfTCallEnter`, `KpfTChange.state` (again), `KpfTShardEnd` and `KpfTCallEnd`
(answer and `ends`), `KpfTShardPrice.answer`, `KpfTCStep`, `KpfTCallPay.off`, as they stand in the working tree
with commit 1 (the 41 bytes `gbook` beside the rows); in `reserve.rs` `Draw`, `worths`, `level`, `equity`,
`need_of`, `draw_end_judged`, `call_end_judged`, `called_at`, `Called`, `call_outs`; in `covered.rs`
`call_step_at`; `books.rs` `Key::numbers` and `below`. **Not opened:** `KpfTCite`, `KpfTChangeLate`,
`KpfTShardOpen`, the tests. **Sizes are estimates** from the compiled lengths now in `contracts/artifacts` and
the transactions 14.10 measured (a statement of a loop about 20 bytes); none is measured.

## Verdict

**Sound to build once five things in 17.8b are corrected; none of them changes its shape.** The three sums are
enough, and the trace with today's `need_of` at the end gives today's verdict bit for bit (below). To change
first:

1. A call is kind 11 and says **nothing** where a draw says its amount (T19). 17.8b has "kind 5 or 6" and "a
   call's fee".
2. A row of the trace is 32 bytes by the fields 17.8b lists and 40 in its sum: make it 40, the fifth number the
   row's place in the books (T19).
3. The list holds the **plain** market; a row's head holds the hashed name: the row's answer hashes (T20).
4. **Every point from nothing up has a script that answers it right where the statement is right, and a
   malformed trace is answered wrong, not by a script that fails** (T21). For kinds 5 and 11 a point nobody can
   answer costs the stater its bond.
5. `KpfTPoint`, `KpfTCallPay.off` and `KpfTCallEnter` do not change (T19); `KpfTChange` does, and it is in every
   statement of every account (T26).

## The bytes of what is said

All numbers 8 bytes as a script writes them; no push bytes; E entries, N rows; N is 4 until the tree.

| At | Bytes | |
|---|---|---|
| 0 | 8 | the amount (a draw: above nothing; a call: nothing) |
| 8 | 8 | E |
| 16 + 56 e | 56 | entry e (from 0): the market **plain** (32), its limit (8), a day (8; nothing), the price (8) |
| 16 + 56 E | 8 | N |
| 24 + 56 E + 40 i | 40 | row i (from 0): its place in the books (8; i until the tree), the place of its price in the list (8), then wn, wg, sn up to and with it (3 x 8) |

Length 184 + 56 E: 240 bytes at one market, 408 at four (40 today). Sufficient: yes, for the rule as it is.

**Empty rows are rows of the trace: N is 4, row i of the trace is row i of the 408 bytes.** An empty row's
sums are the row before's and its place of a price is read by nothing. Left out instead, the end would have to
read the four heads to hold N, and a row's answer to count the rows before it: two loops, for 40 bytes a row
saved. With N fixed and a row a row, no trace leaves a row with units out and the end reads no row.

**The three sums and today's verdict.** `worths` adds, row by row in the order of the rows, w to wn, w signed to
sn, and the gross's worth to wg; a trace's row does the same from the row before's. `need_of` rounds each rate
up on the **total**, so it is computed once, at the end, from the last row's wn and wg: a row at a time it would
round four times and differ. What is counted a row at a time is only a call's price moved the tolerance in the
shard's favour (up where the row's net is above nothing, capped at the limit; down otherwise), which depends on
the row's own net: in the row's answer, told the kind. So:

- `draw_end_judged` or `call_end_judged` is `Some(true)` exactly where every row's point and the end are right.
- It is `Some(false)` exactly where the end or a row is answered wrong. A price of nothing or beyond its limit,
  or a scale of nothing, is wrong **at that row**, judged before any worth is computed, as today's loop stops
  computing once it found one (so a later row beyond 64 bits does not keep a wrong draw from being answered
  wrong: the answer names the first wrong row, whatever point was disputed: `at == point || verdict == 0`).
- It is `None` exactly where some script fails whatever verdict is shown: **the row's** where the worth of that
  row, a sum up to it, or a call's `p * (10000 - g)` is beyond 64 bits; **the end's** where `need_of`, what is
  held less the amount, the equity, or the draw's next time is. T22 for what happens then.

The model's test to write: on random shards, prices and amounts, the builder's trace judged by the three new
functions against the old `draw_end_judged` / `call_end_judged` in those three classes; and each stated sum,
place and entry moved by one, answered wrong at its own point.

## The checks, script by script

Common to the three answers (as today): the statement is of kind 5 or 11; `position`, the statement's two
hashes (as stated; standing again) and the point's hash are rebuilt; `at == point || verdict == 0`;
`ok == verdict`; `ends`. And the layout, held by each (a statement written by a wrong statement nobody
disputed did not pass `KpfTChange`): `said.length >= 184`; E = (length - 184) / 56 with nothing left over and
E <= 4. The scripts read E from the length. The two written counts (E at 8, N at 16 + 56 E) are held by the
end alone.

**`KpfTChange.state`, kinds 5 and 11** (in place of `said.length == 40`): `said.length >= 184`,
`said.length <= 408`, `(said.length - 184) % 56 == 0`, the object a shard. Three statements; no more there
(T26).

**`KpfTShardEnd.answer` (a draw), `at == 0`.** Shown the terms, the books' hash (32 bytes: no row, no `gbook`),
the moving part, `said`.

1. The counts as written: the number at 8 is E, the number at 16 + 56 E is 4; else `ok = 0` (not a `require`).
2. wn, wg, sn: the last 24 bytes of `said`.
3. As today, on those: the claimer is the owner; the amount above nothing; `need` at the upper level from wn
   and wg; held less the amount not below nothing; not called; held less the amount, and sn, less the basis,
   at least `need`; `t0` at or after the shard's time; the outputs' hash (the shard with the amount less and
   the next time, the amount to the claimer).

**`KpfTCallEnd.answer`, `at == 0`.** The same shown.

1. The counts, as above. The amount is nothing; `t0` above nothing.
2. As today on the last row's sums: `need` at the lower level; not called; held and sn less the basis **below**
   `need`; the fee whole or none; `held - pay - CALL_HOLD >= CALL_HOLD`.
3. The call's object: `blake2b(i9(12) + sid + i9(11) + i9(t0) + p33(blake2b(said.slice(16, 16 + 56 * E))))`:
   the list's bytes and nothing else. The outputs' hash as today with it.

**`KpfTShardRow.answer`, new, `1 <= at <= 4`.** Shown the terms, the 408 bytes, `gbook`, the moving part,
`said`, the kind.

1. row = the 102 bytes at `102 * (at - 1)` of the books; cur = the trace's row `at - 1`; prev = the row before
   it, or three sums of nothing where `at == 1`. `ok = 1`.
2. cur's place in the books is `at - 1`; else `ok = 0`.
3. The row's first 33 bytes are a push of nothing (today's test of an empty row): `ok` only where cur's three
   sums are prev's. Nothing else is read.
4. Else, in this order:
   - c = cur's place of a price. `c < 0` or `c >= E`: `ok = 0` (and no slice is taken at c).
   - entry = the 56 bytes at `16 + 56 * c`. `blake2b(i9(14) + p33(entry market))` or
     `blake2b(i9(13) + p33(entry market))` is the row's name (bytes 1 to 33), as `KpfTShardPrice` takes both
     today; the entry's limit is the row's (bytes 76 to 84); the entry's day is nothing; else `ok = 0`.
   - p = the entry's price, sc = the row's scale (bytes 67 to 75), pm the limit: `p <= 0`, `p > pm` or
     `sc <= 0`: `ok = 0`.
   - Only where `ok` is still 1: q = p for a draw; for a call `p * (10000 - g) / 10000`, or, where the row's
     net is above nothing, `(p * (10000 + g) + 9999) / 10000` and no more than pm. w = the worth of the net's
     size at q, v = of the gross's size, by `KpfTShardEnd`'s three-term product. `ok` only where cur's wn is
     prev's and w, cur's wg prev's and v, cur's sn prev's less w where the net is below nothing, else and w.
     Compared as numbers.

**`KpfTShardPrice.answer`, `at >= 5`.** Shown the terms, the books' hash (32), the moving part, `said`, the
seed and the committee (in place of the rows and `under`).

1. e = `at - 4`. `e > E`: right, nothing cited (the citation's place names the object, as today).
2. entry = the 56 bytes at `16 + 56 * (e - 1)`. The order, where `e >= 2`, with before = the entry before it
   and m0 to m3 the market's bytes 0 to 8, 8 to 16, 16 to 24, 24 to 32 each read as a number (as
   `Key::numbers`): before is below entry where `b0 < m0`, or they are equal and `b1 < m1`, or those are and
   `b2 < m2`, or those are and `b3 < m3`, or all four are and before's day is below entry's. Not below:
   `ok = 0`, nothing cited. The entry's day is not nothing: `ok = 0`, nothing cited (until the day is built).
3. Else the committee: the shard's id (the moving part's last 32 bytes) is
   `blake2b(seed + committee + terms + moving.slice(93, 120))`, a `require`. The citation as today, of
   `p33(committee) + p33(entry market)` and `t0`. c the smaller of the cited price and the entry's limit; hi
   as today, no more than the limit; a = the entry's price: `a <= 0`, `a < c * (10000 - g) / 10000` or
   `a > hi`: `ok = 0`.

**`KpfTCStep.step`.** `body` is bytes: `body.length % 56 == 0`, `body.length <= 224`; the call's hash is of
`blake2b(body)` as today. For each leg that is not a token leg: r = its place; `r >= 0`,
`56 * r + 56 <= body.length`; the entry's first 32 bytes are the leg's bytes 1 to 33 (the plain market); its
next 8 the leg's bytes 76 to 84 (the limit); its next 8 nothing; p its last 8, `p >= 0` as today. Nothing else
changes. A token leg's place is read by nothing, as today.

**`KpfTCallPay.off`, `KpfTCallEnter.end`.** Each is shown the body's hash (32 bytes) and holds the call by it:
no change in either script. `Called` in the model: the shard's id, the time, the list.

## The points, and how long a draw is held

What the scripts do today, read in `KpfTPoint`, `KpfTChangeSettle` and `ends`:

- `KpfTPoint.dispute` holds a point to `point >= 0` and nothing more, for every kind; it is shown the hash of
  what is said. It takes a dispute only of a statement that, as it stands, landed before its time, its
  lateness and a dispute period. `lapse` takes one that landed by then; `dismiss` the others.
- A right answer writes the statement standing again. Nothing remembers a point: the same one is disputed
  again. Standing again, it is applied once it stood a dispute period, or at once where it was written at or
  after the last time to dispute.

So a draw lives no longer than its time, its lateness and two dispute periods, with five points or nine: the
bound is a time. Each answered point does begin a dispute period again, but only until that last time. With
the tests' terms (a lateness of 100, a period of 50): undisputed a draw is applied 50 blocks after it landed;
stalled to the end, 200 after its time. **A stall costs a bond a dispute period**: three bonds for the 150
blocks, each a KAS at the least for a draw, of which the owner has a quarter and whoever answers a quarter. A
griefer that disputes faster loses a bond a round and stalls no longer. What nine points mean for an owner
that wants to draw: its tool answers three kinds of point, a row's answer is a transaction of about 4.4 kB
that did not exist, and a round on a price asks the committee for a citation, one a round as today.

## Findings

**T19 (Medium).** *17.8b against what is built.* (a) A call is kind 11 (`KIND_CALL`; `KpfTChange` line 182);
kind 6 is no kind and `KpfTPoint` refuses it. (b) A call says nothing where a draw says its amount:
`KpfTCallEnd` answers a call that says anything there wrong, and takes the fee from the terms. Built as "a
call's fee", every call of today's tool is wrong at its end, or the end must compare a stated fee with the
terms' for nothing. (c) "The place (8) and the three sums" are 32 bytes; the sum of 408 counts 40. Take 40: the
first number the row's place in the books, which a row's answer holds to its own number today and which is the
leaf's place with the tree, so the form does not change at commit 4. (d) `KpfTPoint` holds no bound on a point
for any kind (`point >= 0`) and sees the hash of what is said, not what is said: it does not change. (e)
`KpfTCallPay.off` and `KpfTCallEnter` carry the body's hash: they do not change. (f) "Six to ten points": 5 and
E, so five to nine.

**T20 (Medium).** *Which market is in the list.* A row's head has `blake2b(i9(14) + push(market))`; a leg and a
citation have the market plain. 17.8b says "its market and limit the row's".
*Decision.* The plain market, in the list and so in the call's object. The row's answer hashes it once to meet
the head; the price's answer cites it as it is and needs no `under`; `KpfTCStep` compares bytes with the leg
and loses the hash it computes a leg today; and the list's order is the tree's order of keys (`books.rs` orders
by the plain market). The scale is not in the list: a worth needs the row's (the row's answer has the row) or
the leg's (the step has the leg). `KpfTCStep` so no longer holds a leg's scale to the call. That is harmless: a
leg in the books has its row's scale (`KpfTEnterS` refuses a second row of a name), and one that is not in the
books leaves by a note that no row takes (`KpfTLeftVoid`).

**T21 (High if missed; one line in 17.8b).** *A point nobody can answer right ends a right statement at its
stater's cost.* `KpfTPoint.lapse`: for kinds 5 and up but 10 the claimer's bond goes to the disputer, half of
it. A caller's right call of two markets has points 0 to 6. The owner's other key disputes point 7. If
`KpfTShardRow` takes 1 to 4 and `KpfTShardPrice` 5 to 4 + E and nothing takes 7, the call ends, the caller has
lost its bond, and the disputer has its own back and half the caller's: every call, for nothing. The same for a
row that names a place of a price beyond the list where the script slices there and fails, and for a row's
point of an empty row.
*Decision.* As the checks above: `KpfTShardPrice` answers every point above 4 + E right with nothing cited (as
it answers a point beyond the fourth today); a place beyond the list and counts that are not the length's are
`ok = 0`; an empty row's point is answered by its sums. The numbering of 17.8b stands (0; 1 to N; N + 1 to
N + E). The model's test: for a right statement every point from 0 to 12 has an answer the engine takes.

**T22 (Medium).** *Where a number does not fit.* Today the end computes everything, and where a sum is beyond
64 bits the end takes neither verdict. `reserve.rs` (above `draw_end_judged`) and section 6a say "the dispute
lapses and the draw ends with both bonds returned". That is not what `KpfTPoint.lapse` does since F69: the stater
loses its bond. With the trace it is the same, at another point: the row whose worth or sum does not fit, or
the end for the rate and the equity. A stater cannot write the right sums there (they are not numbers of 8
bytes), so whatever it wrote is wrong at a row nobody can answer either way: that row's dispute lapses at its
cost. For a draw that is right (its owner signed for rows that large). For a call it means such a shard is
called only while nobody disputes; it is so today.
*Decision.* Nothing to build; correct the two lines, and keep the order of the checks in a row's answer (the
price judged before the worth) so that a wrong price stays answerable beside a row that does not fit.

**T23 (Medium; it is so today, and 17.8b carries it).** *A row of a market the shard's committee does not sign
makes the shard one nobody can call.* A call must say a price for every row that has units. The price of a
market nobody signs has no citation; `KpfTShardPrice` requires one; the point is answered by nobody and lapses
at the caller's cost. An owner enters a leg of its own on such a market (two keys of its own sign), and its
other key disputes that price of every call: half of each caller's bond is its. For a draw the same rule is
right (F69: the owner cannot draw over such a row). For a call it is the wrong way round, and the other way
round is worse (a price nobody answers standing lets anyone call a sound shard at any price while a committee
is silent).
*Decision.* Not for a script now. Said beside 14.7's lines as a rule of whoever watches: an entry whose own
price nobody can cite is disputed within its period (it ends at nobody's cost and the leg is undone), and a
user reads a shard's rows for one before it signs. The twin of T1 falls under it: two markets whose numbers
are equal (`00..00` and `00..80` in a chunk) can both have rows until the tree, and no list in strict order
holds both; neither is a market a committee signs.

**T24 (Low).** *An entry no row names.* It costs only whoever wrote it: a price nobody can cite lapses at the
stater's cost, which is the right way round for a draw and for a call alike (the stater chose the entry).
Standing, it is 56 bytes more shown to every step: three entries at most, 168 bytes.
*Decision.* E no more than 4 (the length); the end does not hold that every entry is named (on four rows that
is a loop of sixteen comparisons in each end). With the tree the rows are in the list's order and it is one
comparison a row: hold it there.

**T25 (Low).** *Whoever steps has one entry to show.* A list in strict order has no market and day twice, and
bytes that are equal are numbers that are equal: for a leg there is at most one entry of its market with a day
of nothing. The order is held only where a price's point is disputed. Under a call nobody looked at, a stepper
chooses, as review T3 found of today's form. `KpfTChange` cannot hold the order (it would be in every
statement of every account). Keep `p >= 0` in the step as it is: `p > 0` would make an account of a false call
one nobody can step, and its row would never empty.

**T26 (Medium).** *What gets heavier.*

| Transaction | Today | After | |
|---|---|---|---|
| a draw stated | about 3,200 | 3,450 to 3,650 | 200 to 368 more said, `KpfTChange` 50 more |
| a call stated | 2,913 | 3,150 to 3,350 | the same |
| a draw or call applied | 2,736 | the same | the settlement is shown the outputs, not what was said |
| a call's end answered | 5,402 (5,520 with commit 1) | 3,450 to 3,650 | the loop over four rows leaves the script (about 1,900 bytes), the rows are not shown (417 less), `said` whole (200 to 368 more) |
| a draw's end answered | about 5,050 | 3,500 to 3,700 | the same, the loop about 1,400 |
| a row answered | none | 4,300 to 4,500 | new: a script of about 2,300, the rows and `said` shown |
| a price answered | 8,226 (8,300) | 8,390 to 8,560 | the order and the seed in the script (about 270 more), 185 to 17 less shown |
| a call's step, one leg | 6,890 | 6,520 to 6,750 | the body 56 to 224 bytes for 368; a hash less a leg |
| a call's step, six legs | 7,538 | 7,170 to 7,390 | |
| a claim paid, a call ended, an entry ended by a call | 3,454; 3,347; 2,628 | the same | |
| every statement of an account (a take, a leaving, a mark) | | about 50 more | `KpfTChange` is in each |

Heavier: a draw and a call stated (8 to 16 %), a price answered (1 to 3 %), every account's statement (under
2 %). Lighter: both ends (a third), a step (2 to 5 %). All of them are over 3 kB before and after; none is the
common path but the 50 bytes of `KpfTChange`, which is why its rule is three statements and the counts are the
end's to hold.

**T27 (Medium; a decision for the owner of the design).** *The gate.* Three forms, on L4's play (a shard of
19.9 KAS, a user's young gain of about 15, the owner's long of 1,000 units at 13; a row's requirement at the
upper level is 7.75 % of its worth with these terms).

| Form | What an entry carries | On every entry (4,902) | What it does to L4 |
|---|---|---|---|
| none | | | 17.10's table: the user is paid 2.12 at a move of 1 % with both edges |
| **the whole gate, as a trace**: the entry says the list of prices and the shard's rows with the leg, and the hash of the shard as the entry wrote it; disputed as a draw's rows and prices are; its end holds the lower level | 216 to 384 bytes of `said` and 32 of the hash; three answers more for kind 10 (three scripts in the library) | about 5,200 to 5,400 now; **40 bytes a row more with the tree: 3 kB at sixty rows** | The owner's leg does not enter by a true statement, and a false one is ended and the leg undone at its entry: no claim of the owner's dilutes. It does **not** keep the call from being due meanwhile (14.4: the leg is in the books from the statement, and a call stated then stands). Each price of the list is one more point by which either party ends an entry that went against it while a committee is silent |
| **the leg's own row**: `KpfTEnterS` holds that what the shard holds, less what it owes, is at least the upper-level requirement of the leg's row with the leg, at the leg's own entry | nothing shown; about 200 bytes of script | about 5,100 | The 1,000 units ask 1,012 KAS of a shard that holds 19.9: refused; 14.7 units enter. The owner pays its shard up to enter more, and what it paid in pays the users. With the user's row at its cap the user is whole at 1 % and at 2 %. With all four rows at their caps and all four markets moving the owner's way (its young legs about fifty times what the shard holds) the user is paid about 12.3 of 15.3 at 1 % and 9.1 of 16.0 at 2 % (2.12 and 1.16 with no gate) |

The whole gate cannot be on every entry once the books are a tree, and it does not stop the call. The row's own
gate is cheap and bounds the size, which is what makes L4 "of any size". It leaves a row's worth for each row
(four now, sixty-three later), and it counts what the shard holds, not its equity: a sound shard whose side has
gained on paper pays up before a large entry.
*Recommendation.* **Commit 2 carries no gate**: it is today's rule said another way, and its tests are today's
verdicts. Decide the row's own gate with commit 3, where the second class is built and L4's table is run in the
model with and without it. The whole gate only later, if at all, for a leg the row's gate refuses and an owner
will not pay up for.

**T28 (Low).** *Small things.* (a) The first three bullets of `KpfTShardEnd`'s and `KpfTCallEnd`'s headers
("computes it over the four rows", "point r, 1..4 the price") go with the loop. (b) A point's number is read by
the answer scripts from `said`'s length; a tool that disputes computes it the same way, and the builders should
give the three ranges names. (c) `Draw::read` and `Draw::said` are of 40 bytes; `call_wrong_point` and
`draw_wrong_point` return 0 or a row: they return the first wrong point of the new numbering, a row before a
price before the end, so that what is answered wrong is what a script can answer (T22).

## On each "to be said by the review" line

| 17.8b asks | Said |
|---|---|
| a row with no market: a row of the trace, or left out | a row of the trace; N is 4; its sums are the row before's |
| are three sums enough; where the tolerance is counted | enough. The rates and their tolerance at the end, on the totals, by `need_of` as it is; a call's price moved the tolerance, in the row's answer |
| what the end holds of E and N | the two written counts are the length's (E) and 4; no row is read; that every row is in the trace is the fixed N and a row's answer reading the row of its own number |
| a call's `said` and its object | the same bytes as a draw's with an amount of nothing; the object's body is the list's bytes, hashed by the end from `said` |
| `KpfTCStep` and a token leg | nothing: passed by, as today |
| sizes | T26 |

## Decide before the scripts of commit 2

1. Kind 11 and an amount of nothing for a call; a row of 40 bytes with its place in the books (T19).
2. The plain market in the list (T20).
3. Every point answered: the three ranges, a point beyond, a place beyond, wrong counts (T21).
4. `KpfTChange`'s rule as three statements on the length; the counts at the end (T26).
5. No gate in commit 2 (T27).

## Can be decided while building

- Whether `KpfTShardRow` takes option names (13) beside futures names (14): the checks above take both, as
  `KpfTShardPrice` does today.
- The names of the points in the builders; the two stale comments (T22, T28).
- What is said to a user and a watcher of a market nobody signs (T23), with 14.7.
