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

# Third round: 17.8c

27 November 2026, the same day. Under review: `docs/ARCHITECTURE-CLEARING.md` 17.8c (the claims of a called
shard, and the row's gate), with 17.8a and 17.10. Findings T29 to T38. Nothing run; this section is the only
thing written.

**Read whole for it, as they stand in the working tree (commits 1 and 2):** `KpfTCStep`, `KpfTCallPay` (claim,
off), `KpfTDrawNote`, `KpfTOwed`, `KpfTShardOpen.open`, `KpfTLeft` (apply, rest), `KpfTLeftAll.apply`,
`KpfTLeftVoid.void`, `KpfTMarkNote.apply`, the row's arithmetic of `KpfTEnterS`; in `reserve.rs` `Left`, `Rest`,
`MarkNote`, `Owed`, `left_holding`, `marked`, `rest_paid`, `claim_paid`, `call_off`, `paid_in`, `books_hash`,
`claims_hash`, `need_of`; in `covered.rs` `call_step_at` to its end; 16.3 (a mark's band). **Not opened:**
`KpfTCEnd`, `KpfTCMark`, `KpfTJoinC`, `KpfTPass`, the tests. What a leaving's end takes as its band is from 14.9
and 16.3, not from the script. **Sizes are estimates** (a statement about 20 bytes, one inside the loop over six
legs six times that).

## Verdict

**Sound to build once four things are changed; the arithmetic of both classes holds as written.** With every
claim counted before the first payment, no order of payments pays more than there is and no first-class claim
is paid less because a second-class one was paid first (shown below, with the unit's edges). To change first:

1. **"A claim counted late is behind what was paid, at its holder's cost" is not what the formulas do** (T29).
   A first-class claim counted after the second class was paid takes from a first-class claim that was on time
   and not yet shown. One rule closes it: **once anything was paid, every claim counted is of the second
   class.**
2. **The call's step writes a note of a kind of its own** (T30). Two amounts in the note that legs left would
   change the hash every account's leaving writes, which 17.8c says does not change.
3. **`KpfTDrawNote`, `KpfTLeft.rest` and `KpfTLeftVoid` change too** (T31): the first two hold a note of the
   rest by its hash.
4. **The gate needs a second arm** (T35): as written it refuses a leg that lowers its row's requirement to a
   shard that holds too little, which is the leg such a shard needs.

The gate: **yes**, as the third piece of three, with the inequality of T35.

## The numbers, and every script that moves them

| Number | Where | While not called | While called |
|---|---|---|---|
| H, what it holds | moving part, 67 to 75 | as today | falls by what a claim is paid; rises by every note that pays in and every addition |
| C1 | moving part, 76 to 84, where what it owes is | **as today: rises by what a note asked and was not paid, falls as a note of the rest is paid** | rises by every first-class amount counted; never falls |
| D1, C2, D2 | the claims' numbers, `blake2b(i9(D1) + i9(C2) + i9(D2))` in the books' hash | nothing, all three | D1 and D2 rise by what is paid; C2 by every second-class amount counted |

**C1 that does not fall is the called shard's rule only, and that is compatible with today.** Not called, a
note of the rest is written again with what is left and what is owed falls with it (`KpfTLeft.rest`,
`rest_paid`): keep that, so what is owed is always the sum of the notes outstanding, each with nothing paid. At
the call's applying nothing is written but the call's time: C1 is what was owed, D1 nothing, and each note of
before the call is a first-class claim of its amount with a1 nothing. No script needs to know which notes are
of before.

**Show the three numbers, not their hash** (T32): with their pushes they are 27 bytes and the hash is 32. The
41 bytes beside the rows become 36: `i9(gross) + i9(D1) + i9(C2) + i9(D2)`, and each script hashes the last 27.

| Script | Shown of the claims | Writes |
|---|---|---|
| `KpfTShardOpen` | none | three of nothing, as built |
| `KpfTEnterS` | the 27 bytes, carried | none (it refuses a called shard; the gate reads what is owed) |
| `KpfTLeft.apply`, `KpfTLeftAll.apply` | the 27 bytes | not called: C1, as today. Called: C1 by the first-class amount, C2 by the second |
| `KpfTLeft.rest` | carried | C1 falls by what is paid (not called only) |
| `KpfTLeftVoid` | carried | none |
| `KpfTMarkNote.apply` | **the books opened**: the sum of the gross (8), the hash of the rows (32), the three numbers: 64 bytes where it carries 32 | as `KpfTLeft.apply` |
| `KpfTCallPay.claim` | the 27 bytes | D1, D2 |
| `KpfTCallPay.off` | the 27 bytes | C1 and the three as nothing |
| `KpfTOwed.apply`, `KpfTAddC`, the ends and answers of a draw and a call | carried in the books' hash or beside the rows | none |

## The notes

**The claim** (the note of the rest, tag 7), for every shard, called or not:

    blake2b(i9(12) + p33(sid) + i9(7) + p33(key) + i9(A1) + i9(a1) + i9(A2) + i9(a2))

Not called it is always (A1, 0, 0, 0). Written by `KpfTLeft.apply`, `KpfTLeftAll.apply`, `KpfTMarkNote.apply`
(what was asked and not paid; under a call, what was counted), by `KpfTLeft.rest` (what is left, not called) and
by `KpfTCallPay.claim` (a1 and a2 raised).

**The note that legs left** (tags 6 and 8) stays as it is: one amount. Every leaving, split, sale and undone
entry writes it by a hash its statement's end holds; none of those scripts changes.

**The note of a call's step, tag 9, new**, written by `KpfTCStep` alone:

    blake2b(i9(12) + p33(sid) + i9(9) + p33(chain of the futures legs) + p33(key) + i9(asked) + i9(young))

`asked` as today (what the old legs gained with what the young lost together, less the set-off against what the
account owes; or, below nothing, what the account paid, which the note holds); `young` the second-class amount,
nothing or more.

**The class, where a note is applied to a called shard.** With `called` the moving part's 130 to 138 (not
nothing), `late` its 94 to 102, `dispute` its 103 to 111:

    young(leg)  is  int(leg[34..42]) > called - 2 * (late + dispute)

the comparison `KpfTCStep` makes. And `paid` is `D1 + D2 > 0`.

| Note | A1 counted | A2 counted |
|---|---|---|
| tag 6, one leg; a mark's note (its one leg, as it was) | `asked` where the leg is not young and not `paid` | `asked` otherwise |
| tag 6, two to six legs | `asked` where no leg is young and not `paid` | `asked` otherwise |
| tag 9 | `asked` where above nothing and not `paid` | `young`, and `asked` where above nothing and `paid` |
| tag 8 | nothing | nothing |

A tag 9 note is held to its legs as well, since a note is not taken at its word: `young >= 0`; `young == 0`
where no leg is young; `asked <= 0` where every leg is young. That is what the legs can say without prices; a
mixed account's split is the step's.

## The checks, script by script

**`KpfTCStep.step`.** As built, and:

1. In the loop, for a young leg: `ym = ym + (the larger of wp and we)`.
2. After it: `band = (ym / 10000) * 2g + ((ym % 10000) * 2g + 9999) / 10000`, g the terms' tolerance (bytes
   19 to 27). `young = ygain - band`, nothing where that is below nothing. (`ygain` below nothing is added to
   `gain` as built.) One rate on the sum, not one a leg: T34.
3. `asked`, `pay`, `setoff` and what the account owes exactly as built, from `gain`. **Nothing of `young` is set
   off, and nothing of it is paid by or to the account.**
4. The note is tag 9 with `asked` and `young`; its value `NOTE_MIN + pay` as built.

**`KpfTLeft.apply` (one leg), `KpfTLeftAll.apply` (two to six).** Shown as built, the 36 bytes for the 41, the
tag, and `young` (nothing but for tag 9).

1. The shard, the rows' arithmetic, the sum of the gross: as built. The note's hash by its tag: 6 and 8 as
   built; 9 with both amounts. Tag 9: `called != 0` (a step's note is of a called shard), and the three checks
   against the legs above.
2. `asked < 0`: what the note holds is taken as built.
3. Not called: `asked > 0` is paid as far as the shard holds, the rest owed: as built, the note written
   (rest, 0, 0, 0).
4. Called: nothing is paid. A1 and A2 by the table. `C1 = C1 + A1`, `C2 = C2 + A2`.
5. Outputs under the id: the shard; and the claim (key, A1, 0, A2, 0), holding a note's least, where
   `A1 + A2 > 0`. (Today's branches are "below nothing" or "asked": a tag 9 note may pay in and ask of the
   second class in one.)

**`KpfTMarkNote.apply`.** As built, the books opened (64 bytes). Not called: as built, the note (rest, 0, 0, 0).
Called: A1 or A2 by its leg and `paid`; the claim (key, A1, 0, A2, 0).

**`KpfTLeft.rest`.** Shown the note's four numbers.

1. `A1 > C1`, or not called and any of `A1 <= 0`, `a1 != 0`, `A2 != 0`, `a2 != 0`: never counted. Spent,
   nothing paid, the shard as it is (as today for a note of more than is owed).
2. Else: not called (`require`); `pay` the smaller of A1 and what is held, above nothing; `C1 = C1 - pay`; the
   note written again as (A1 - pay, 0, 0, 0), or spent where nothing is left.

**`KpfTLeftVoid.void`.** Tags 6, 8 and 9 (the hash of 9 with both amounts); the rest as built.

**`KpfTDrawNote.end`, kind 7.** The note's hash with four numbers; `A1 > 0`, `a1 == 0`, `A2 == 0`, `a2 == 0`,
`A1 <= C1`. (Today a note of the rest of amount nothing would pass `owed >= amount` and could be spent by
nothing: only a wrong statement nobody disputed writes one, and `A1 > 0` closes it here.) Kinds 6 and 13 as
built. Tag 9 ends no draw: no step's note with legs in the books outlives its call.

**`KpfTCallPay.claim`.** Shown the terms, the rows, the 36 bytes, the moving part, the key, A1, a1, A2, a2, and
the place f of an input.

1. The shard, called. The four rows empty (until the tree) **and** the sum of the gross nothing.
2. The time: `OpTxInputDaaScore(f) >= called + late + 3 * dispute` (an input that landed then shows it, as
   `KpfTEnterS` shows an entry's time; a period more than 17.8c has: T33).
3. The note's hash. `0 <= a1 <= A1 <= C1`, `0 <= a2 <= A2 <= C2`, `A1 + A2 > 0`, `H >= 0`.
4. `W = H + D1 + D2`.
5. First: `e1 = A1` where `W >= C1`; else u from C1 by today's four thresholds, `h = W / u`,
   `o = (C1 - 1) / u + 1`, `e1 = (A1 / o) * h + ((A1 % o) * h) / o`. `p1 = e1 - a1`, nothing where below
   nothing, no more than H, and no more than `C1 - D1`.
6. Second: `S = W - C1`. `S <= 0`: `e2 = 0`. `S >= C2`: `e2 = A2`. Else u from C2 by the same thresholds,
   `h = S / u`, `o = (C2 - 1) / u + 1`, `e2 = (A2 / o) * h + ((A2 % o) * h) / o`. `p2 = e2 - a2`, nothing where
   below nothing, no more than `S - D2` (which is H less what the first class is still owed, whatever p1
   is), and no more than `C2 - D2`.
7. `whole` is `a1 + p1 == A1 && a2 + p2 == A2`. `p1 + p2 >= NOTE_MIN`, or `whole` (`require`: a part below the
   least waits; a payment of nothing that spends nothing is no transaction).
8. Where `p1 + p2 >= NOTE_MIN`: an output of that to the key. Below it (the claim is whole thereby): no output
   is asked; it is whoever pays's, as a step's small payments are.
9. The shard: H less `p1 + p2`, the three numbers with `D1 + p1` and `D2 + p2`, C1 as it is; its output's
   value no less than before less `p1 + p2`.
10. `whole`: the note spent, one output under the id. Else the note again with `a1 + p1`, `a2 + p2`, its
    output holding what it held.

**`KpfTCallPay.off`.** As built, with: the sum of the gross nothing and the four rows empty; `D1 == C1` and
`D2 == C2`; written: C1 nothing, the three numbers nothing, not called.

**`KpfTEnterS.state`, the gate** (T35). After the row's new net `rn` and gross `rg` are computed, with `rn0`,
`rg0` the row's before, `sc` and `e` the leg's scale and entry, `mmr` and `g` the terms' (bytes 10 to 18, 19 to
27), `worth` the script's three-term product, `|x|` the size of x:

    rN = mmr + g
    rG = (mmr - 3 * g) / 2, nothing where below nothing, and g
    need(n, s) = (worth(|n|) / 10000) * rN + ((worth(|n|) % 10000) * rN + 9999) / 10000
               + (worth(s)   / 10000) * rG + ((worth(s)   % 10000) * rG + 9999) / 10000     at the price e
    require( held - owed >= need(rn, rg)  ||  need(rn, rg) <= need(rn0, rg0) )

`need` is `need_of(wn, wg, terms, true)` for one row. `held` and `owed` are the shard's 67 to 75 and 76 to 84.

## The payment: what holds, and where it breaks

Take every claim counted before the first payment, so C1 and C2 stand, and the sum of the A1 is C1, of the A2
is C2 (a note that was never counted is another matter: it says no more than C and is paid a part as a real one
is, today as well).

- **No part is above its exact share.** `h <= W / u` and `o >= C1 / u`, so `e1 <= A1 * W / C1`; the same for
  the second with S and C2. So the parts of the first class sum to no more than the smaller of W and C1, and of
  the second to no more than the smaller of S and C2.
- **The products fit.** `(A1 / o) * h <= (C1 / o) * (W / u) <= W`. `(A1 % o) * h < o * h`, and o is no more
  than 3,000,000,000 by the thresholds while h is below o: under 9 x 10^18. The same with S and C2. That needs
  `A1 <= C1` and `A2 <= C2`, which check 3 holds.
- **W does not fall while a shard is called** (no draw, no note paid where it is applied; a payment moves H to
  D), so a part only grows.
- **What is kept:** `D2 <= S` after every payment (`p2 <= S - D2`, and S moves only up). Then what the first
  class may still take, the smaller of W and C1 less D1, is no more than `W - D2 - D1`, which is H. **So a
  first-class claim finds its part whatever was paid to the second class before it**, and the bound "no more
  than H" in check 5 only ever cuts by rounding.
- At the edges. C1 = 3,000,000,001 (u a thousand, o = 3,000,001) and W one sompi less: a single claim of all
  of it has a part of 2,999,999,001, 999 sompi below its exact share and never above; S is below nothing and
  the second class is paid nothing. W = C1: every first part is whole. W = C1 + 1, so S is 1 sompi: a second
  part is nothing where C2 is 3,000,000,000 or more (h is nothing), and at most 1 sompi otherwise, which is
  below a note's least and waits unless it makes its claim whole. What rounding leaves stays held, and is the
  claims' when more comes.

**Where it breaks: T29.**

## Findings

**T29 (High as written).** *A claim counted after payments began is not behind.* The only claims that can be
counted then are of notes that take no leg out: a mark's. (Every other waits for no row to have a unit by
being what empties the rows.) A shard, called, with W 150: X, a user's first-class claim of 100; Y, the owner's
own second-class claim of 50; and Z, a mark's note of an old leg of the owner's, of 100, stated before the call
and left unapplied. Payments open. Y is shown: S is 50, Y is paid 50. Z is applied: C1 is 200. Z is shown:
its part is 75, paid. X is shown: its part is 75 and the shard holds 25. The owner has 125 of the 150; at one
share X had 75. X was on time; what it did not do is show its claim, or apply the owner's note, in the block
payments opened.
*Decision.* **A claim counted where `D1 + D2 > 0` is of the second class, whatever its legs** (the table
above). Then C1 stands from the first payment and everything under "what holds" is true without a condition. Z
then shares S with the second class, behind every first-class claim: that is "behind", and at its holder's
cost. C2 can still grow late, and within the second class a claim shown early keeps what it was paid: said to a
user as "apply the notes of a called shard and show your claim when its time comes". 17.10's T12 has this as
"it stands behind what was paid"; correct it there as well.

**T30 (Medium).** *Two amounts in the note that legs left reach every account's script.* That note's hash is
computed by each statement that takes a leg out and held by its end. Tag 9 keeps them all as they are and
gives the applying scripts one more form. A user's own leaving cannot then carry a first-class amount for young
legs: it writes tag 6, one amount, and the class is its legs'. A wrong statement nobody disputed can write a
tag 9 note as it can write any note; the three checks against the legs bound what it says where the legs are
all young or all old.

**T31 (Medium).** *What 17.8c's list of what changes leaves out.* `KpfTDrawNote` (kind 7: the hash of a note of
the rest and `owed >= amount`); `KpfTLeft.rest` (the same hash; what "never counted" is with four numbers);
`KpfTLeftVoid` (tag 9). Built without them, a draw is ended by no note of the rest and a note of the rest of an
uncalled shard is paid by nothing.

**T32 (Low).** *The claims' numbers unopened cost more than opened*: 32 bytes against 27. One form, shown
plain, in every script that shows the rows. `KpfTMarkNote` cannot stay at 32 bytes carried: under a call it
writes C2.

**T33 (Medium).** *What 17.8c does not say of a payment.* (a) The two parts are one output: the least is of
their sum. (b) "Covered, paid whole whatever its size": below the least no output can be asked of a block, so
it is whoever pays's; without that line a claim of 0.001 KAS is paid by nobody and `D1 == C1` never comes. (c)
A transaction that pays nothing and spends nothing is refused, or a claim is written again a block for ever.
(d) The time is shown by an input's landing; the shard's own output is as young as its last note. (e) A claim
says `A <= C` for the arithmetic to fit, not only `a <= A`. (f) **A dispute period more before the first
payment.** "A lateness and two dispute periods" is when the last statement of before the call is applied, not
when its note is: a mark stated a block before the call's time and disputed by the owner's other key is applied
in the block payments open, and with T29's rule a payment made in that block puts its note in the second
class. Three periods give its holder one to apply it. (g) **No class is paid more than it is owed**: a part is
also no more than C less D of its class. A note that was never counted and says no more than C is paid a part
as today; without the bound it takes D past C, and the call never ends.

**T34 (Low).** *The band.* (a) One rate on the sum of the young legs' larger worths, rounded up once: the same
band to a sompi a leg, and about 130 bytes less in a loop that is unrolled six times. (b) No second band where
a note is applied: a leaving took its own at its own price, and a mark left its band in the leg (16.3); the
applying script has no price to take one at. What L1's path keeps is what every leaving keeps today: a short's
tolerance squared twice (0.65 KAS a 13,000 KAS of size). It goes where the leaving's band is corrected, not
here. (c) A young pair entered together is taken the band of both legs from their net: a hedge that gained 10
on 2,000 KAS of legs is of nothing in the second class. That is the two edges of each leg; say it.

**T35 (Medium).** *The gate.*

- **It refuses the leg that helps.** A shard holds 100 KAS and its row is users long 10,000 KAS: asked 775.
  A user's short of 10,000 KAS would bring the row to a net of nothing and 450 asked. The gate as written
  refuses it (100 is below 450). Hence the second arm: a leg that does not raise its row's requirement enters.
  It lets an owner's own leg in for nothing only against the row's net and up to 1.4 times it (a leg lowers
  what is asked by 5.5 % of the units it sets off and raises it by 2.25 % of its own size): that is a hedge of
  what the users hold, not L4's leg.
- **Both rates are counted the tolerance more** (5.5 % and 2.25 % with the tests' terms: 7.75 % of a leg alone).
  17.8c's bracket has the tolerance on the net only.
- **What it refuses that is honest:** a shard at its level by what its legs gained on paper, whose owner drew
  the coins: it holds less than the row asks and takes no leg that raises the row until it is paid up. And a
  shard that owes: it holds nothing and takes nothing that raises a row. Both are what a gate is for. It never
  asks more than the upper level a draw already holds the whole shard to, and for one row only.
- **The price.** The leg's own entry, which its statement's last row holds to the committee's within the
  tolerance: an edge moves the bound by 0.5 % of itself. A wrong entry far below passes the gate and is wrong at
  its own price: disputed, the entry ends and the leg is undone at its entry, a claim of nothing. Meanwhile it
  is in the books (14.4) and a call may be due by it: an owner can still bring a call about so, for an entry's
  bond; what it cannot do is dilute by it.
- **The other rows** cannot be counted without their prices. Four rows at their caps is what the review's
  second table shows: 12.3 of 15.3 at 1 %, 9.1 of 16.0 at 2 %, every market moving the owner's way. With the
  tree it is a row of sixty-three: the gate is then a bound on a leg and no more, and a number kept over the
  rows is a question for commit 4.
- **An honest busy shard.** A leg of 100,000 KAS into a row of its own: 7,750 KAS held. Into a row of users
  long and short a million each: 52,750 (45,000 without it). Against the row's net: nothing more.
- **L4.** The 1,000 units ask 1,012 KAS of a shard that holds 19.9: refused; 14.7 units enter. To enter them
  the owner pays its shard up by about 990 KAS, and those pay the second class: the user is whole unless the
  price moves about 7.7 % beyond the band while the legs are young.
- An entry is about 5,300 bytes with both arms (5,040 now): 5 %.

*Decision.* Build it, both arms, at the leg's entry, held less owed. Its tests need shards that hold what their
rows ask: the plays of F116 and L4 as they are written enter legs the gate refuses, and are kept by paying the
shard up or by entering under the cap.

**T36 (Low).** *"Second, whole, where any leg is young" against a user.* A young leg enters an account by its
user's signature. But a step of a liquidation is anyone's and names the legs: on an account that is due, a step
that names an old leg that gained beside a young one makes one note of both, of the second class whole if a
call's time then falls within T of the young leg's entry. Narrow (the account is due; a leg that gained is
named only where the legs that stay lost nothing), and the user chose the young leg. Said, not built against.

**T37 (Low; to say in 17.8c).** *A shard is called until both classes are whole.* An owner that diluted its
users with a young leg of its own has that leg's claim standing too: to have its shard back it pays in until
every second-class claim is whole, the users' with its own. Or it leaves the shard called for good with
nothing in it. So what L4 leaves is a user unpaid by an owner that gives the shard up, not an owner that is rid
of the claim and goes on. And how the first class is protected from the owner's old legs: it is not, as today;
an old leg was within what the shard was held to for T, and with the gate it entered against coins the shard
held.

**T38 (Medium).** *Sizes, estimated.*

| Transaction | Now (commits 1, 2) | With commit 3 | |
|---|---|---|---|
| an entry stated | 5,040 | about 5,300 | the gate |
| a note of one leg applied | 4,314 | about 4,550 | the class, the claim of four numbers; 5 bytes less shown |
| a note of two to six legs applied | not measured | about 400 more | a test of a time in each of six turns |
| a mark's note applied | not measured | about 200 more | the books opened (32 more shown), the class |
| a call's step, one leg | 6,520 to 6,750 (estimated) | about 350 more | the larger worth in each of six turns, the band, an amount more |
| a claim paid | 3,624 | about 4,100 | the second formula, four numbers, the time |
| a call ended | not measured | about 60 more | two comparisons, three numbers written |

Every one is heavier. The common paths are the first two: 5 % each.

## On each "to be said by the review" line

| 17.8c asks | Said |
|---|---|
| the arithmetic with the unit and its rounding, and that no order pays more than there is | checks 4 to 6 of `KpfTCallPay.claim`; the unit of the second class from C2, its part of S; holds for every order once C1 stands, which T29's rule makes so |
| a claim counted late | not behind as written (T29); second class once anything was paid; said in 17.8c's "when it is paid" and in 17.10's T12 |
| "second, whole, where any leg is young" against a user | its own signature for an entry; a liquidation's step is the narrow way (T36) |
| the gate's arithmetic, what it refuses that is honest, which price | T35: both arms, both rates the tolerance more, at the leg's entry |
| how the first class is protected from the owner's old legs | it is not, as today (T37) |
| the sizes | T38 |

## The order within commit 3

Three pieces, each reviewable at the engine alone:

1. **What a called shard has paid**: the claim of four numbers with A2 and a2 nothing everywhere; D1; the first
   formula; no payment while a row has a unit, and the time; whole below the least; the call's end; `KpfTLeft`
   (apply, rest), `KpfTLeftAll`, `KpfTMarkNote`, `KpfTCallPay`, `KpfTDrawNote`. Its test is F114's shard (100
   held, 200 owed, 60 that come late).
2. **The second class**: `KpfTCStep`'s tag 9 and band; the class where a note is applied; "second once anything
   was paid"; the second formula; `KpfTLeftVoid`. Its tests are L1, T29's three claims, and L4's table in the
   model.
3. **The gate**: `KpfTEnterS` alone, and the shards of the tests paid up. L4's table again, with it.

The first changes every hash of a note of the rest once; the second adds a tag and touches no hash the first
wrote; the third touches one script.

## Decide before the scripts of commit 3

1. Second class once anything was paid (T29).
2. Tag 9 for a step's note; tags 6 and 8 as they are (T30).
3. The three numbers shown plain, 36 bytes beside the rows (T32).
4. A payment's least, its one output, its time by an input (T33).
5. The gate with both arms (T35), and the three pieces in that order.

## Can be decided while building

- The band on the sum or a leg at a time (T34).
- Whether `KpfTLeft.rest` keeps showing the rows it does not read (it could carry the books' hash and the 27
  bytes).
- What is said to a user of T36 and T37, with 14.7.
