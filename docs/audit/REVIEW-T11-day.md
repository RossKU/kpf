# Review T11: the day's price and an ended leg (17.8g), on paper

Base: 7adb550. Read-only; nothing run. Under review: `docs/ARCHITECTURE-CLEARING.md` 17.8g, against 17.5 and
17.8 (item 1 and step 5).

## Verdict

**Change first; then build A.** A as written cannot be built (D3: no output more is written under the library's
id beside `cite`), and its claim that two notes of one day are a committee's fault is false by the code (D1: the
price in a note is its writer's choice within the spread of what a quorum signed). With three changes A is sound
and is built alone: (1) `KpfTDay.write` stands beside the committee's **attest** (the twin of `KpfTNote.note`),
not beside a citation; (2) the note is an object in `KpfFeePool`'s form, holds a least nothing sweeps, and a
reader asks its value again and no absolute least (D5); (3) the text says a note's price is one of the prices
the spread allows. **B is not to be built as written.** A leg must not be ended by "the note it is shown":
a shard takes one price a day, in its books, written once (the alternative the section names), and the note
legs read is that row's copy (D1, D2). That one change also makes the fallback exclusive of a price, which
nothing in 17.8g does. Decide D4, D6, D7 and M1 before any script of B.

## Findings

**D1 (High). Two notes of one day differ without any fault, and whoever sends picks.**
`KpfEpochTable.attest` takes `p` from whoever builds the transaction: every included member's own signed price
within `spreadBps` of `p`, a quorum of weight (lines 27-30, 98; `KpfTNote` copies `p`). All members signing
100.00 at a spread of 25 bps, any `p` from 99.75 to 100.25 is attested; members' prices that differ widen it.
So for one day any number of true notes exist, and 17.8g lets each transaction take the one it likes.
Scenario: one user, two accounts of one shard: a call held at 90 in the first, the same call written in the
second (the books net nothing). At the day (price 100) it ends the first by a note at 100.25 (its own
statement, or the only leg) and the second by a note at 99.75: the shard pays 10.25 and is paid 9.75 a unit:
twice the spread of the notional, each day, less the premiums' spread. Without two accounts the same is taken
from each user by whoever sends first: a keeper (the owner) ends a written call that lost at the highest note.
17.5 takes no band at the day ("only the entry's part") because it held the price written once: it is not.
"First" cannot be held of free objects; "stood a time unanswered" needs a statement. *Recommendation:* one
price a shard, a market and a day, **in the shard's books** (a leaf of the day, put in by one transaction that
spends the shard beside a day's note, checks the spread against the shard's tolerance once, and refuses where
the leaf is there), and a **shard's day note** (`sid`, market, day, price) written by that transaction and
copied by anyone, which is all a leg's end reads. Both sides of the books then end at one price and the books
are exact; what the first writer chooses within the spread is the committee's tolerance, said to the user.
The other way (a band of the tolerance at the day) costs every holder in the money 4 spreads of its notional
and leaves D2 open: not that.

**D2 (High). The fallback is not exclusive of a note; after its time each leg is ended either way.**
No script shows that no note exists. After "the time the terms say" a leg is ended at its entry by anyone
"shown an input that landed after that time", whether or not a note of the day was written; and a note written
in time stays usable for ever. Scenario: a call held at 100, day at 130, beside a written leg that stands on:
its gain stays (17.5's test fails), "no coin". The owner waits out the time and ends the leg by the fallback:
30 a unit gone, the account under. Or the reverse: a holder out of the money waits and takes its premium back
by the fallback though the price was written. The user's statement of a leaving can also be held past the time
by disputes (a bond each). *Recommendation:* the leaf of D1 says the day's price **or "none"**; "none" is
written by anyone shown an input that landed at or after the day and the time, where the leaf is not there;
whichever is written first is the day's for that shard. The fallback's leg-end reads the shard's note of
"none" as the other reads a price.

**D3 (High as written: cannot be built). `write` beside a citation writes nothing under the library's id.**
`KpfTCite.cite` asks `OpCovOutputCount(lib) == OpCovInputCount(lib)` (line 93); so does every script's
`goesOn`. Only `KpfTNote.note` writes an output more (inputs 1, outputs 2, alone with the committee's table).
And the note cannot be "a slot of the library": `KpfSlot` holds the library's id as a constant, which no
script of the library can hold (`KpfFeePool`'s header says why); an object of its own id would be nobody's
word (anyone pays to such a script). *Recommendation:* `KpfTDay.write` is `KpfTNote.note`'s twin: beside the
epoch table spent by `attest` for the committee, the market and the day; it writes one object in
`KpfFeePool`'s form (`go` asks the id of its own input) under the library's id, state tag 16. No citation,
no 5.6 kB, nothing of a block's depth to hold; the signatures of a price note already written are public and
are used again while a table of the day's window lives. (Changing `cite`'s count instead would let a `cite`
spent alone write anything under the id: no.)

**D4 (Medium; the holder is not protected). Who gains by silence, and who can make it.**
(a) 17.5's fallback returns a holder's premium and ends its leg at its entry: a holder in the money loses its
gain. The owner names the committee and gains twice on a day where users' held options are in the money and
their written ones out of it: it does not pay the first and has back the premiums of the second. N72 says it
to the user and holds nothing. No price can be had without the committee, so no script pays the gain; what a
script can do is make silence cost its owner: **at the fallback a written option's premium stays the
user's** (it does not return to the owner's key), and 17.3's deterrent applies to a day with no price as to a
call. Decide that now; the deterrent may wait. "The last price a note of the market has" cannot be held
("last"); a later time's price gives the owner a free look. (b) Silence can be made without the committee:
`KpfCommittee.roll(m)` moves the window forward by `m` epochs, and a table attests only `from <= t0 < until`.
Where nobody rolled into the day's epoch before it ended, anyone rolls past it and that day has no price for
ever; with the fallback paying premiums back, every holder out of the money wants that. And anyone retires
the tables a window after `until`: the day's price is attested within one to two epochs of the day or never.
*Recommendation:* say both as a keeper's duty and in the client (roll into the day's epoch; write the day's
note before the tables go); the fallback's time is at least two epochs of the committee. (c) A table rolled
late holds the members as of the roll: a day's price may be signed by a later table's members. The same for
any price; nothing for `write` to check.

**D5 (Medium). `KpfTSweep` takes what a day's note holds above a copy's floor; a reader must not ask a least.**
`sweep` names any input under the library's id as its copy, asks that it goes on with its script holding
`floor` exactly and that at least 0.2 KAS moved (lines 75-87); an object whose `go` passes for any script of
the library passes for it. A note of 1 KAS (a price note's least) is swept to the floor by anyone, 5 % to the
sender; a reader that asked the least again would then refuse it for ever. *Recommendation:* `write` and
`copy` ask `DAY_MIN` = a note's least (0.2 KAS: below a floor and 0.2 more, or the floor's value is to be
checked: I did not read it); a reader asks `out.value >= in.value` and no number.

**D6 (Medium). A leg that lost, ended first, leaves the account owing, and an account that owes does nothing.**
16.19: every end but a step, a sale and a payment asks that nothing is owed. Scenario: three legs, two of one
day: a written call that lost 100 and a held call that gained 150, and a futures leg that stays; coins 40. A
keeper ends the first: the account owes 60. The user's statement of the leaving of its 150 is now refused
(it owes); the gain is no coin; the sale's clock of 16.18 runs on its tokens. 17.8g says "what they do not
pay the account owes" and nowhere that a gain is set against it, which is what the model's result rests on
(`stepped_owing`). *Recommendation:* say it in B: (1) every leg's end at a day sets what it gained against
what is owed first (the only-leg case asks the gain less the premium less what is owed); (2) an account that
owes may still state the leaving of an ended leg that gained; (3) a sale is due only where what is owed is
above the ended gains that stand. D7's form gives all three by itself.

**D7 (Medium). "It gained and other legs stay: by the user's statement" leaves dead legs and dead rows.**
(a) A written option that ended out of the money gained nothing and asks nothing of the shard, yet by B's two
cases it is not "lost" and not the only leg: the commonest ending (a covered call, expired) needs a statement
and a dispute period to free a premium that 17.3 says "is the user's at the day" (the section names no
transaction for that). (b) Nobody but the user ends such a leg: twelve accounts of dust legs hold the
sixty-four rows of a tree for ever. (c) Every row script must pass an ended leg at its day's price for as
long as it stays. *Recommendation, a third form:* **every ended leg leaves by the one transaction, and what
it gained (less its premium) is credited to the account, not paid to the key**: the number of 16.18 goes
below nothing (the shard owes the account). It counts for the account's standing exactly as the model's
"kept" does, is no coin, is turned into coins by anyone's payment from the shard into the account, and
leaves by a take under the take's test, which is the test of 17.5. The row is free at once, no later
statement needs the day's price, a step nets it (the model's `coins + kept`), and under a call it is what the
shard owes: a claim of the first class, where an "ended leg that stays" is a leg's gain of no class named.
Its cost: the scripts that ask "owes nothing" ask "owes nothing above nothing" (step 6 rewrites their sums).
If that is too much now: keep B's statement, add (a) as a third one-transaction case (a leg that asks
nothing always leaves) and D6's three lines.

**D8 (Medium). M1: the test counts the band.** As proposed, with two additions: it counts every band the step
would take (an option's band at a stated price as well, 17.5), and the model shows, before `covered.rs`, the
30 paths at nothing and that an account is not due right after a take and a move of the lower rate. "A step
takes no band" is wrong: a step would be the one leaving at a stated price that gives up no tolerance, and a
user that makes its own account due leaves at the price's better edge. A third form (the band taken only as
far as the account does not owe) leaves the shard unpaid by a price's error where the account has nothing:
no. The band's size is a later tuning; the form is this.

**D9 (Low). The keeper's fee.** A leg ends once, so six fees an account at the most: no drain. Say the order
(what the leg lost, its premium, then the fee; a fee is never owed) and that no fee is taken where the input
shown landed before the day and a lateness: the user ends its own leg free. Nothing is "rolled" at a day.

**D10 (Low).** A position that moves to a new library leaves its day's notes behind (D1's row moves with the
shard). Copies are 0.2 KAS each for ever where nobody takes them back; with D1 the shard's note can have an
owner that takes it back, since the row writes it again. A note of a time that is no day of anything is
harmless. A copy under another library's id is refused by the reader's first check. The note holds no price
limit: the reader takes the lower of the price and its leg's limit, as `KpfTShardPrice` does (lines 202-205).

**D11 (Medium, the text). What 17.8g changes or drops of 17.5.**
- "Written once": dropped (D1).
- 17.5: a leg that gained leaves by the one transaction where the account stands; 17.8g: by a statement. The
  reason given is right (the test needs other prices); 17.5 is to be rewritten, by D7.
- The entry's part of the band, taken at the day: absent from B's sums.
- "Where none is written yet, by a citation of the day": B answers by the note alone. By D1 a citation's
  price and the note's differ: before the shard's price is written an ended leg is passed at a stated price
  within the tolerance, after it at the row's.
- A call's trace counts a day's row "at what its legs were entered at where nobody has written the price":
  unprovable with free notes; with D1 the trace's entry of a day is answered by the leaf (and
  `KpfTShardPrice` line 175 asks a day of nothing today).
- A leg whose day came before a call's time: 17.3's "no premium moves under a call" or the day's rule? Say:
  the day's rule (the option ran its life).
- The fee "the least the terms say": which number of the terms.

## Decisions on "To be said by the review"

| Point | Decision |
|---|---|
| A free note against the shard's row | Both: the row holds the one price (or "none"); the note is how it is read without the shard. A committee-wide note (A) is the source a shard writes its row from |
| What `write` holds of the citation | Nothing: no citation. The attest holds the table of the time (`from <= t0 < until`), `tx.daa >= t0` and the quorum |
| "Ended" by an input's time | Cannot be played to end early (no input lands in the future); to delay is anyone's always. With a note: compare the leg's day and market with the note's and no input's time (a note is not before its day: `attest`, line 77). An input's time only for "none" and for a row that passes an ended leg in a statement (the statement's own time) |
| The two cases of one transaction | D7: one case, every ended leg. A leg does not leave while a statement stands on its account (the statement holds the account's hash); it leaves in the time 14 leaves for a step |
| The keeper's fee, a drain | D9 |
| The fallback's time; silence | D2, D4 |
| M1 | D8: the test counts the band |
| The note to the shard (B, e) | As a leg that left: the leg's 111 bytes whole (the row's key and the "entered" number come from its market, scale, limit, day, strike and entry; where the leg says call or put, held or written is not in the four numbers of X: say it), the user's key, what is asked (below nothing: the note holds the coins); with D7 the credit instead of the key |
| An ended gain under a call | With D7 a claim of the first class. Its leg is never "young": no option enters within T of its day |
| Before option legs exist | A whole (`write`, `copy`, a probe reader). Of B: the shard's side (the day's leaf put in beside a day's note, "none", the shard's note, a trace's entry of a day answered by the leaf), if the tree takes a dated key; the model of D7 and D8 |

## Check lists

**`KpfDay`** (the object; `KpfFeePool`'s script with its state): `go(l, k)`.

**`KpfTDay.write(u, n, rid, mkt, t0)`**, anyone; constants `EPOCH_TABLE_TPL/PRE/SUF`, `ATTEST_TAG`, `DAY_PRE`,
`DAY_SUF`.
1. `version == 1`.
2. `attested(u, t0, mkt, rid)` as `KpfTNote` has it, every line.
3. `price` = the attest's `p`; `price > 0`. `spread` = the table's last number; `spread >= 0`.
4. `h = blake2b(i9(16) + p33(rid) + p33(mkt) + i9(t0) + i9(price) + i9(spread))`.
5. `n >= 0`, `n != self`; `OpOutputCovenantId(n) == lib`; `outputs[n].scriptPubKey == P2SH(DAY_PRE + h + DAY_SUF)`;
   `outputs[n].value >= DAY_MIN`.
6. Its copy goes on with the fee; `OpCovInputCount(lib) == 1`; `OpCovOutputCount(lib) == 2`.

**`KpfTDay.copy(who, ageb, x, n)`**, anyone; `x` the 32 bytes of the note's state.
1. `version == 1`; `int(ageb) >= 0`; `d` = `who`'s first place, the other two equal it; `d != self`.
2. `OpInputCovenantId(d) == lib`; `inputs[d].scriptPubKey == P2SH(DAY_PRE + x + DAY_SUF)` (the whole script).
3. `outputs[d].scriptPubKey == inputs[d].scriptPubKey`; `OpOutputCovenantId(d) == lib`;
   `outputs[d].value >= inputs[d].value`.
4. `n != d`, `n != self`; `outputs[n].scriptPubKey == inputs[d].scriptPubKey`; `OpOutputCovenantId(n) == lib`;
   `outputs[n].value >= DAY_MIN`.
5. Its copy goes on with the fee; `OpCovInputCount(lib) == 2`; `OpCovOutputCount(lib) == 3`.

**A reader** of a day's note at input `d` (shown `price`, `spread`, and what opens the committee):
1. `d` is read from a place of `who` (the note's `go` asks that it is named); every place of `who` is read;
   `d != self`, `d` is not the object.
2. `OpInputCovenantId(d) == OpInputCovenantId(self)`.
3. `inputs[d].scriptPubKey == P2SH(DAY_PRE + h + DAY_SUF)`, `h` of the committee (opened from the shard's id by
   the seed, as `KpfTShardPrice` line 191), the market and the day **of what it prices** (equal, not "at or
   after"), `price`, `spread`.
4. `spread >= 0`; `spread * 4 <= g` of the terms; `price > 0`; the price used is the lower of it and the limit.
5. `outputs[d].scriptPubKey == inputs[d].scriptPubKey`; `OpOutputCovenantId(d) == lib`;
   `outputs[d].value >= inputs[d].value`.
6. Its own `goesOn` (the count holds with the note in and out).
With D1 only the shard's writing of its row is such a reader; a leg's end reads the shard's note (state tag
17: `sid`, market, day, price or "none") by 1, 2, 3 (its account's `sid`), 5, 6, and nothing of a spread.

## When

- **Before any script (A):** D3 (beside the attest; the object's form), D5 (`DAY_MIN`, the reader's value),
  the text of D1.
- **Before any script of B:** D1 and D2 (the row and the shard's note), D4 (a) (the written premium at the
  fallback), D6, D7 (the credit, or the statement kept), D8 with its model, D11's lines, where a leg says
  what it is.
- **While building:** the tags (16, 17: I did not check that they are free), D9, D10, the fallback's time
  against the committee's epoch, what the client says of D4 (b).

## Read, and inferred

Read: 17.2 to 17.6, 17.8 (item 1, the steps), 17.8g, the lines of 16.18 and 16.19 on what is owed;
`KpfTCite`, `KpfTNote`, `KpfNote`, `KpfTShardPrice`, `KpfTAddC`, `KpfSlot`, `KpfFeePool`, `KpfTSweep`,
`KpfEpochTable`, `KpfCommittee.roll` alone, `KpfTLeft`'s header; the structs of `reserve.rs` and `thin.rs`
`Note`; `research_options.rs` lines 1577 to 1690; the head of REVIEW-T10.
Inferred, not read: that anyone rolls (the entry takes no signature) and that members change between rolls;
the floor of a copy; that tags 16 and 17 are free; where a leg says its kind; that an account's terms open its
shard's id; `KpfTOwed`, `KpfTCEnd`, the tree's scripts and T10's U4; the size of the model's band. Nothing was
run: D1's and D6's scenarios are from the scripts' text and want a test each.
