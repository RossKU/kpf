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
