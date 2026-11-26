# Review T2: options under a shard, and the books in a tree (section 17, on paper)

27 November 2026. A review on paper of `docs/ARCHITECTURE-CLEARING.md` section 17 (17.1 to 17.8), which is not
built. Nothing was run: no cargo, no test, no node. Findings continue the numbering of `REVIEW-T1b.md` (N42 was
its last).

**What was read.** Section 17 twice; sections 4b (the option leg), 12, 13, 14.1 to 14.9, 16.1 to 16.5, 16.8 to
16.12, 16.17 to 16.25; `GOAL.md`. Scripts read whole: `KpfTCRow`, `KpfTShardEnd`, `KpfTCallPay`, `KpfTChange`.
Read in part: `KpfTCEnd` (the three ends, to line 336), and the heads of `KpfTEnterS`, `KpfTStepPrice`,
`KpfTCStep`, `KpfTLeft`. Not read: `KpfTCallEnd`, `KpfTCEnter`, `KpfTPass`, `KpfTJoinC`, and the Rust model
beyond a search (it has no option in `covered.rs`).

**What was reasoned with numbers:** N43 to N48, N53 to N56, the sizes of N58, the arithmetic of N60. **Read and
reasoned without numbers:** N49 to N52, N57, N59, N61 to N64. N56's last paragraph is about the built form, was
read and not run, and is to be handed to the reviewer at the engine.

Terms used in every example: maintenance 5 %, tolerance g 0.5 %. So: the test of an entry and of a take 5.5 %
(`up`), a step due below 4.5 % (`lo`), the band 1 %, a shard's upper level 5.5 % of the net and 2.25 % of the
gross, its lower level 4.5 % and 1.25 %.

## Verdict

Section 17 is not sound enough to build from as it is written. The two halves differ.

**17.5 and 17.6 (the tree, a draw and a call as traces, F114) can be built** once two things are decided: how a
row has one place in the tree, and that a trace says one price an underlying (N55, N56). Neither is hard.

**17.3 and 17.4 (the premium and the option leg) have four holes that lose users' coins**, each by the rule as
written and with the owner's own keys only:

- an option's entry is a number two signers choose, and the shard's equity moves by it at once: it is a draw
  that the draw's rule never sees (N43);
- the books count a premium a holder has not paid as the shard's equity, the owner draws on it, and the forfeit
  under a call then says it is not owed (N44);
- "what it lost is not paid" forgives a real loss, because an entry that holds the premium no longer says what
  the option was worth when it entered: an owner's own holder of options deep in the money is long with a put
  nobody sold it (N45);
- a holder is asked 5.5 % of what its option is worth exercised, where that worth moves one for one with the
  price: the premium leaves the account after one move up, and a fall of 0.3 % nobody stepped leaves the shard
  unpaid (N46).

And 17.4 does not hold the goal's first row: it nets nothing (N47). A covered call is asked twice the net risk,
a call spread three and a half times.

All five are closed by one change of form, which is the first thing to decide: **the premium is a number of its
own in the leg and coins in the user's account from the entry to the leg's end, whoever pays it; the shard's
books never know a premium; an option's entry is its market's price, as every leg's** (the recommendation under
N-h). With it kind 10's rule stands as built, the forfeits become payments of coins that are already there, and
no gate is needed.

Decide before any script: where the premium lives (N-h); what a holder is asked and the rule that nets (N-j);
that a settlement price is written once and an ended leg leaves by a transaction (N-i); that a row's place is
its key and a trace has one price an underlying (N-k); and whether the first form is European (N53).

## Findings

### N43 (High). An option's entry is a draw that no rule sees

Today the last row of an entry holds the leg's entry to the price stated (`KpfTCRow`, lines 196 to 201: the
leg is the one said, and its entry is the row's price), and that price is answered by the committee's. So an entry moves the
shard's equity by no more than the tolerance of the leg's worth. 17.3 enters an option "at the price at which it
is worth exercised what it is worth now and P more": the row's rule must go for an option, and nothing is put in
its place. What holds an option's entry is the two signatures, and one signer is the owner.

Section 14.3 cut the gate at an entry because "what holds a shard to its requirement is the draw and the call".
That holds while an entry cannot move the equity. Here it moves it by P at once.

*Scenario (a user wrote; the shard is diluted).* A shard holds 1,000 KAS. Y, honest, is long 100 units of M
from 100; the requirement is 775. M rises 3 %: Y has gained 300. The owner's own account X writes one call,
strike 100, entered at 100,100 (a premium of 100,000; X needs 5.5 KAS of coins). The books take the leg: the
equity is 99,300 below nothing, and the owner states the call. Under the call X's leg leaves having gained
100,000, and by 14.6 that is a claim. Y: 300 x 1,000 / 100,300 = 3 KAS. X: 997. The owner has its coins back
and Y has lost 297.

*Scenario (no call at all).* The same leg on a market whose last day is tomorrow, or already past (17.2 refuses
neither). It "leaves by anyone's statement" having gained 100,000; its note asks that of the shard, and outside
a call a note is paid in full as far as the shard holds. The owner has taken everything the shard holds, by an
entry and a leaving, with no draw stated. Whoever watches has two dispute periods to call the shard, and a call
gives the first scenario.

17.3 does not say what a written option's premium is under a call. If it is a claim: the above. If it is not:
N48.

*Recommendation.* A premium must not be a number in the books. See N-h: the premium the clearing side pays is
coins the owner puts into the user's account in the transaction that states the entry, out of its own pocket
and not the shard's; the entry is the market's price and kind 10's row stands as built. If "the premium in the
entry" is kept instead, an entry in which the shard pays a premium is a draw of that premium and must pass the
draw's rule at disputed prices, and a written option's premium under a call must rank after every other claim:
both are more script than the escrow, and the second needs two classes of claim.

### N44 (High). The books count a premium nobody has paid, and the forfeit then says it is not owed

17.3 says the writer has the premium "when the option has ended or was exercised, and not before", and holds a
user who wrote to that. The shard as writer is held to nothing: "what the legs were entered at stays one number
over all rows", so a leg a user holds, entered P above the price, is P of equity for the shard from its entry.
A draw is of equity above the requirement (`KpfTShardEnd`, line 181). And under a call "an option a user holds
leaves and what it lost is not paid".

*Scenario.* The shard of N43: 1,000 KAS, Y long 100 units from 100, requirement 775; the owner may draw 225.
The owner's account U holds one call, strike 100, entered at 800 (a premium of 700 for an option worth nothing;
U holds 700.2 KAS, as the entry's end asks). The books: equity 1,700, requirement 783. The owner draws 917. The
shard holds 83. M rises 3 %: Y has gained 300; the equity is 483, below the lower level (598); the call is
right. U's option leaves and its 700 are "not paid". Y is paid 83 of 300. The owner drew 917 where the rule
gave 225, and has U's 700 back.

With no price move the same steps take 692 of the 775 the requirement asked, and no statement of them was
wrong.

*Recommendation.* The same rule for the shard as for a user: a premium is nothing to the books until it is paid.
With the escrow of N-h the books never hold one. If "the premium in the entry" is kept: a second number in the
shard, the premiums of the options it wrote that still stand, taken off the equity in the draw's test and the
call's; and it must then be kept by every note, which the one number of today avoids.

### N45 (High). "What it lost is not paid" forgives a real loss

An entry that is "the price at which it is worth exercised what it is worth now and P more" is one number for
two things: what the option was worth exercised when it entered, and the premium. No script can take them apart
again. So the forfeit under a call cannot be of the premium: it is of all the leg lost.

*Scenario.* A shard holds 1,600 KAS. Y, honest, is short 100 units of M futures from 100. The owner's account U
holds 100 calls on M, strike 1, entered at 100 (premium nothing): a long of 100 units in all but name. U holds
3,500 KAS. M falls 30 %. Y has gained 3,000; U has lost 3,000 and is no step's (3,500 - 3,000 = 500, above 4.5 %
of the 6,900 the calls are worth exercised). The book is set off: the equity is 1,600 against 1,085 asked, and
no call is due. The owner makes one due (N48: an entry of its own, which no gate holds) and it is stated. Y
claims 3,000. U's calls leave and "what it lost is not paid": U keeps its 3,500. Y is paid 1,600 of 3,000. Had
M risen 30 %, U would have been paid 3,000 out of Y's loss. With a futures leg in U's place the call's step
pays the 3,000 into the shard first and Y is paid whole.

The same in the other forfeit: "in a step of a user who wrote an option ... what it gained is not asked". A user
who wrote a call in the money (strike 100 at 110, premium 2: entered at 112), the price at 105, stepped for
another leg: its 7 a unit are not asked, of which 5 are what the option honestly fell by. The shard keeps more
than the premium.

*Recommendation.* The premium is a number of its own in the leg (N-h), and both forfeits are of that number and
nothing else. What the option gained or lost exercised, from the market's price at its entry, is settled under a
call and in a step as a futures leg's is.

### N46 (High). A holder is asked 5.5 % of what its option is worth exercised: the premium leaves the account

17.4: the maintenance's base of an option a user holds is "its worth exercised at the price (what it can still
lose)", and the rate is asked of it. But it can lose all of that worth, not 5.5 % of it: in the money an
option moves one for one with the price. Section 4b had this right for an account no shard covers: the holder
"keeps whole" what the option is worth exercised.

*Scenario.* A call held alone: strike 100, the price 100, premium 5, ten units; entered at 105. The entry asks
50 KAS (and a note's least): right. The price goes to 105: the leg has lost nothing from its entry, W is 50, LS
is nothing. A take's test (`KpfTCEnd`, line 220) is m - amount >= 5.5 % of 50: **the user takes 47.25 out**.
The price returns to 100. A step is due from 104.9, and the coins pay what the leg lost down to 104.73: a fall
of 0.26 %. Below that the shard is unpaid; at 100 it has had 2.75 KAS of a premium of 50. The claim of section
12 is a move of 4.5 % between two visits; here it is 0.26 %.

For the user this is an option for 3 KAS: above 105 it gains as if it had paid 50.

The same in a straddle held (a call for 5 and a put for 4 at 100: 90 asked at the entry, 43 after the price
went up 5.5 %, and 90 lost if it comes back).

*Recommendation.* Two parts. The premium a holder has not paid is asked whole for as long as the leg stands,
whatever the price (it is the escrow of N-h). And what the option is worth exercised above its entry is asked
at the smaller of that worth and the rate on its underlying's worth: what it loses by a move of the rate. The
second is what N47's sums give without a rule of its own.

### N47 (High against the goal). Nothing is netted: the user's account, and the shard's books

"The writer is asked the maintenance of the underlying" and "options and futures are not set against one
another" (4b, kept by 17.4 and 17.5). Worked for the five accounts asked, ten units, the price 100:

| Account | Asked at the entry (17.4) | The most a move of 5.5 % loses | The most it ever loses |
|---|---|---|---|
| A call held alone (strike 100, premium 5) | 50 | 50 | 50 |
| Long futures and a call written (strike 110, premium 2) | 110, and the 20 come at the end | 55 | as the futures alone |
| Short futures and a put written (strike 90, premium 2) | 110 | 55 | as the futures alone |
| A straddle held (call 5, put 4, strike 100) | 90 | 90 | 90 |
| A spread: a call held (100, premium 5), a call written (110, premium 2) | 105 | 30 | 30 |

A holder alone is asked what it can lose (until the price moves: N46). Every account with a written option is
asked the whole underlying again: twice the risk for a covered call, three and a half times for a spread. And at
120 the spread, which can lose nothing above 110, is asked 5.5 % of 200 and of 1,200.

To be fair to the design: the built form does not net the requirement for futures either (W is the sum of the
legs' worths; a long and a short of one market are asked both). Nobody holds those two. A covered call, a
spread and a put bought against a long are what options are held as, so here it decides whether the goal's row
holds.

The shard's books have the same shape (17.5: the requirement of an option row is of that row alone): a clearing
party that wrote a strike to one user and holds the next strike from another is asked the underlying of both.
The goal's "hard part", a clearing party that faces many users on little capital, is not had for options.

*Recommendation: the least rule that nets.* Not a pairing of legs: two sums.

- A statement says the rate it computes with (8 bytes; the end, which has the terms, holds it to them; a row's
  answer then needs no terms).
- For a leg at the price a of its row, L = a less the rate and U = a and the rate: what the leg gains from a
  to L, and from a to U. A futures leg: its worth times the rate, either way. An option the user wrote: exactly
  what it is worth exercised at each end, the other way. An option the user holds: the same where its strike is
  outside L to U; where its strike is inside, the worse of the two ends, counted in both sums.
- Two running sums, D-down and D-up, over the legs that stay of one group; a third, R, of the groups closed:
  for each, the larger of nothing, what D-down lost and what D-up lost.
- A group is consecutive legs of one underlying at one price. A leg whose underlying or price differs from the
  row before's closes the group. (The row's answer is shown the leg before as well, 143 bytes, or a row holds
  its group's underlying, 32 bytes.) Legs that are not next to one another are not netted, which is safe: the
  sum of two worst cases is never less than the worst of the sum. An entry may say the place its leg takes in
  the chain; the second chain of the trace builds it.
- R takes W's place in the test of an entry (m + G >= R) and of a step (due below R counted at the lower rate).
  A take and the leaving of a leg that gained keep their second part as built: m >= R + LS, each loss alone
  (S2 stands: netting is of what is held, never of what leaves).

Why it is sound: what all the legs of a group gain at a price is a line with corners at the strikes. The legs
that are futures or written options bend it downward only, so their sum is least at one of the two ends. An
option held bends it upward, and the corner could be the lowest point between the ends: so where its strike is
inside, it is counted at its worse end in both sums, which is a line below it. The rule is exact for futures,
for anything written, and for options held outside the range; it gives no credit to an option held near the
money, and never asks less than can be lost.

The same five accounts, with the premium in escrow (N-h):

| Account | Asked at the entry |
|---|---|
| A call held alone | 50 (the premium); at 105.5 still 50 (N46 closed) |
| Long futures and a call written at 110 | 55, and the 20 the owner paid lie in the account |
| Short futures and a put written at 90 | 55, likewise |
| A straddle held | 90 |
| The spread | 50 of its own (the premium it pays), beside the 20 it was paid; 30 if a premium received may stand for a premium owed (N-h, last point) |

The shard's books take the same two sums over the rows of one underlying, in the order of N55: a book set off
across strikes is then asked the rate on its gross and its net worst case, as a futures book is.

Cost: 24 bytes more a row (three sums), or 56 with the group's underlying; a line or two per kind in
`KpfTCRow`.

### N48 (Medium). A call ends every option at what it is worth exercised, and the owner makes a call due at will

"What the option is worth exercised is settled either way" is right as arithmetic and says who loses the rest:
whoever holds the option loses what it was worth beyond that.

- *The owner's shard wrote.* A user bought calls for 5 a unit; the price nears the strike and such a call now
  costs 15. A call on the shard ends them at nothing exercised; the user "keeps the premium" (5 it never paid)
  and has lost an option worth 15. The owner has given up 5 it did not have and is rid of 15 of liability. A
  called shard returns what it holds, less the claims, to its owner, and the claims are of what was worth
  something exercised.
- *The owner's shard holds.* 17.3 does not say what a user who wrote is owed under a call. If nothing: the
  owner buys options, exercises the ones that come good, and has itself called before the others end, owing no
  premium. If the premium is a claim: N43.
- *Can the party that gains provoke it?* Yes, and for nothing. A call is right where the equity is below the
  lower level. No gate is computed at an entry (14.3). The owner enters a leg of its own large enough that the
  requirement with it is above the equity, and states the call.
- A user that lets itself be stepped loses its premium to the shard: it gains nothing by it. That direction is
  fine. (With N45's correction: the premium, and not what the leg gained exercised.)

*Recommendation.* With the escrow of N-h the second case is closed (the owner paid the premium into the
writer's account at the entry; under a call it stays there). The first is not closed by any rule on paper that
I can find, since no committee signs what an option costs: say it plainly to a user beside 14.7's lines (an
option under a shard is good while the shard is not called; a call ends it at what it is worth exercised, and
the premium stays with the user). A deterrent that costs little if wanted: what is left of a called shard after
its claims goes first to the users whose options the call ended, up to the premium of each again, and only then
to the owner.

### N49 (High for a shard with an ended row). The settlement price is a promise of the committee's, read by no script

"From the last day on it signs one price for every time." What a script checks: `KpfTStepPrice` holds the
citation to the statement's own time (line 213: the citation's time is `t0`). A statement of an ended leg
stated at t0 cites the committee's price of t0, and whoever states picks t0 within a lateness. If the committee
signs for a dated name what it signs for the underlying (a signer that does not know the name is dated, or one
that gains by it), the leg never ends: it moves for ever.

And every price of an ended row must be answered for as long as the row has a unit: in a leaving, in a draw's
trace and in a call's. A call's prices are the caller's to show (14.6); a draw's are the owner's. A committee
that no longer signs for a day long past (dissolved, or its members changed) leaves a shard that has one ended
row able to state no draw that survives a dispute, and **callable by nobody**: a griefer names that row's price
and the call ends at the caller's cost. An owner that wants a shard no one can call keeps one ended leg of its
own in it.

*Recommendation.* The price of an ended leg is the committee's price at the last day, whatever t0 is: the
citation is of the smaller of t0 and the day. Better, and it is what N50 needs: **the settlement price is
written once**. Anyone shows the committee's citation of the day and writes a note under the library's id (the
committee, the market, the day, the price); every script that needs an ended market's price is shown that note
and no citation. There is then no tolerance at an end, no dispute of it, and no committee needed after. Where a
committee signed two prices for one day the first note shown decides: a committee that does so is broken with or
without a note. Where no note is ever written (no committee left on the day): decide what a leg does then; the
least is that after a time said in the terms it leaves at its entry, nothing paid either way, as an entry that
ended.

### N50 (Medium). An ended leg leaves "as a leg leaves by its user's statement": what that brings with it

- *A leg that gained beside legs that lost does not leave.* A leaving of one leg that gained asks the coins to
  stand at what the legs that stay lost, each alone (S2; `KpfTCEnd`, line 246). An account of an ended leg that
  gained 3,000 and a live one that lost 3,000, on 500 of coins, fails it, whoever states. That is right (the
  other way is S2 again) and it is not "nothing is owed past its end": say that such a leg stands, at its
  settlement price, until the others leave or the account is stepped.
- *Nobody is paid to state it.* Whoever is owed states it: the user for a leg that gained, the owner for one
  that lost or for a premium. A leg of dust is stated by nobody and its row keeps a unit. That is the owner's
  matter outside a call, and under a call every account is stepped for a fee, so it holds; say it.
- *It is a statement a leg.* A dispute period, a bond, two transactions and a note for every ended leg, one
  after the other on one account; a user's own statement writes the account's time (twice the longest a
  statement lives). An account with three legs of one day is settled in three rounds.
- *The band.* "What it gained is asked of the shard less the band": N54.

*Recommendation.* With the written price of N49 an ended leg needs no statement: one transaction by anyone,
shown the note, in which every leg of the account whose market ended leaves at that price exactly, with no
band, for the terms' least fee of a keeper out of the account's coins (as `KpfTEnterOff` pays). A leg that
gained is paid into the account's coins where the account would not pass a take without it, and to the user's
key otherwise; what it lost the coins pay, and the account owes the rest (16.18's number serves).

### N51 (Medium). An entry on a market that has ended, or ends within a statement's life

Nothing in 17.2 holds an entry to the day. A futures leg entered after the day gains nothing for ever and adds
to the row's gross. An option entered there, with "the premium in the entry", is N43 with no wait: the leg is
"ended" as it enters and its premium is asked of the shard by the next statement.

*Recommendation.* An entry is wrong where its time, a lateness and two dispute periods reach the leg's day: the
script that states it is shown the day (it is shown the underlying and the day already) and holds it.

### N52 (Medium, against the yardstick). Dated futures, and the day in the market's name

- *What does a committee sign for a dated market before its day?* 17.2 says "as any market". If it is the
  underlying's price (nothing else is on paper), a dated futures leg is the perpetual leg with a stop: it gives
  a user nothing the perpetual does not, and costs a roll at each day (a leaving, an entry of two signatures,
  two dispute periods, the band on what was gained, and both legs asked whole in between).
- *Two names for one price.* A citation is of a market's name. A row of the perpetual and a row of an option
  of the same underlying, in one trace, are two citations of 4.4 kB, and two prices within the tolerance of one
  another (N55). And a script that nets them (N47) must be shown that the two names are of one underlying.
- *The committee must know every dated name* to sign for it, and keep signing its settlement price (N49).

*Recommendation.* Futures legs stay on markets with no day. An option leg's market is its underlying's own, and
its day stands in the leg beside its strike (under a shard the leg's 32 bytes of a committee are free: 16.12
uses them for a vault; the strike, the day and the premium fit there, and the leg stays 111 bytes). A price of
the leg is then the committee's price of the underlying at the smaller of t0 and the day: no new name, nothing
new for a committee to know. A row of the books is named by its underlying, its day, its kind and its strike.
Step 1 of 17.7 (dated futures alone) is then not built; what tests the ended leg is the option itself, or a
futures leg with a day in the same place if a test without options is wanted.

### N53 (Medium). The exercise of a written option by the owner's statement

What 17.4 relies on: `KpfTChange` lets any key state any kind, and the kind's end holds the claimer to a key
(the user's, at bytes 1 to 33 of the moving part, for kinds 7 and 8). The owner is the account's second key
(bytes 34 to 66). So it can be built: a new kind, its end, and the kind in `KpfTChange`'s bond floor,
`KpfTPoint`, `KpfTStepPrice`, `KpfTCRow` and `KpfTCStep`. What it does:

- *The owner picks the price.* Every price is stated within the tolerance, and until now whoever picked it paid
  for the freedom: a user's leaving pays the band, a step is due only below the level counted the tolerance
  less. An exercise as written has neither. A call a user wrote, strike 100, 100 units, the committee at 100:
  the owner states 100.5 and is paid 50 KAS for an option worth nothing. At every exercise the owner has up to
  0.5 % of the underlying; a premium is 1 to 3 % of it.
- *And the time,* within a lateness: a look back of some seconds on every option it holds.
- *It contradicts a decision.* Review 42 (H1): "nobody but side A states on a covered account otherwise, so a
  stranger holds no user's account". The owner is the user's counterparty, and a statement of its own holds the
  account for a dispute period; a wrong one costs it half a bond (0.1 KAS at the least bond).
- *Unsaid:* what happens where the writer's coins do not pay what the option lost (a step; the account owes?);
  whether an exercise is held to the account's time; whether one out of the money is right (it is the owner
  giving the option up, and should be).
- *When it is against the writer's interest:* in the option's own worth, never (the holder gives up what the
  option was worth beyond its exercise). It is against it by the three things above and by the band (N54).

*Recommendation.* Cut it from the first form: an option leaves at or after its day (N50), by a step, under a
call, or with its account by a hand-over. That is what the exchanges the yardstick names do (options there are
settled at their day). A holder's own early leaving can stay (it is a leaving of one leg and harms nobody but
the holder). If the owner's exercise is kept: the price counted the tolerance in the writer's favour, as a
step's level is.

### N54 (Medium; High for whether anyone writes). The band on an option

16.3: the band is "the tolerance twice of what the leg is worth". 17.2 and 17.4 take it off what an option
gained when it leaves. Which worth is not said, and with either the number is wrong:

- *Of the underlying's worth* (what a writer's base is): a user wrote a call for 2 a unit on 10 units at 100;
  it ends out of the money. It asks 20 less 1 % of 1,000: **10**. Half of every premium of 2 % goes to the
  band; a premium of 1 % is never paid.
- *Of what it is worth exercised:* nothing where it ends out of the money, which is right there, and too little
  near the money (a price stated the tolerance off moves an option at the money by the whole of that).

What the band is for: a price stated the tolerance off where a leg entered and where it leaves. For an option
that is, exactly, what its worth exercised changes by when the price is moved the tolerance: at the entry (from
the market's price it entered at; nothing for an option that entered further from its strike than the
tolerance), and at the leaving (nothing where the leaving is at a written price: N49).

*Recommendation.* The band of an option leg is those two amounts, computed in the row from the tolerance the
statement says (N47's 8 bytes carry the rate; the tolerance beside it). At a settlement by a written price only
the entry's part is left, and for an option entered out of the money that is nothing.

### N55 (High for the call). One price an underlying; a row has one place

Two things the tree must give and 17.5 does not say.

*(a) A trace may say two prices for one underlying.* A row of the books is of one market, one kind, one strike:
the futures of X and each strike of X are rows of their own, each with a price of its own in a draw's and a
call's trace, each within the tolerance. Under a call there is no band (14.6: "every leg leaves at the call's
one price for its row"), and that was safe with one row a market.

*Scenario.* The owner's account holds a long of 10,000 units of X and has written 10,000 calls on X at strike
1: flat. The owner calls its shard and states X's futures row at 100.5 and the call row at 99.5 (the committee
at 100). The account has gained 5,000 on each leg: a claim of 10,000 KAS for a position that was flat, at one
share with every honest claim. A shard that holds 1,000 against an honest claim of 300 pays it 29.

*(b) Two rows of one name.* Today `KpfTEnterS` reads all four rows and refuses a second row for a market. With
a path it reads one. If a tree can hold two rows of one name: a call's trace states two prices for them (the
above again, with futures alone), and a trace that must name every key once can never be stated, so an owner
who makes a second row has a shard nobody calls.

*Recommendation.* The leaves are a list in order of the row's key, written plain and not hashed (the
underlying, the day, the kind and the strike): a leaf holds its row and the key of the leaf after it. A new row
is shown the leaf before it (two paths); a row of no unit is taken out by anyone with the leaf before it (a
script of its own, so that a note's applying stays one path). That one structure gives: no second row of a key;
"every row once, in the tree's order" answered at one row (a trace's row is the leaf the row before named as
next; the first is the head, the last names none); and the rows of one underlying next to one another, so that
a trace's row is wrong where its underlying is the row before's and its price is not (and the day has not
passed for either). The call's object then holds one price an underlying and one for each ended day, and the
call's step shows that. N47's sums for the shard run over the same order.

### N56 (Medium). The rest of the tree: an empty leaf, a row that empties, sixty-four rows, sums in the nodes

- *An empty leaf* is one value (32 zero bytes, or the hash of a row of nothing under a tag); a leaf and an
  inner node are hashed under different tags, so that a row of 64 chosen bytes is no inner node.
- *A row that empties.* With days and strikes rows die at every day: they must leave the tree or sixty-four are
  used up in a season. See N55 (taken out by anyone). Until taken out a row of no unit is in every trace, at no
  harm.
- *A call's end* asks today that every row is nothing (`KpfTCallPay`, line 92: the four rows compared whole).
  With a tree: a count of rows that have a unit, kept in the shard's moving part by the scripts that change a
  row; the call's claims begin at a count of nothing.
- *Sixty-four rows.* A row a strike, a kind and a day: one underlying with three days and ten strikes is sixty
  rows. It is enough for a clearing party of a few series and not for what an exchange lists. Make the depth a
  number of the shard's terms (it is in the shard's id); six for the first form.
- *Sums in the inner nodes.* Considered as asked. Across underlyings nothing adds: each row's worth is at its
  own price. Within one underlying it does: with a day's strikes in order and each node holding its units and
  its units times their strikes, what all calls of that day are worth at a price is one path, not a row a
  strike; and the units written and held add as they are. A draw's trace would be a row an underlying and a
  day. It changes every script that writes a leaf (the path's nodes are recomputed with their sums) and it does
  not give the netted requirement of N47, which is not a sum of the rows'. **Not for the first form**; the
  ordered list of N55 is what it would be built on, so nothing is lost by waiting.

*Seen on the way, about the built form; read, not run; for the reviewer at the engine.* The reasoning of N43
and N55 (a) applies to a futures leg of any size. An owner's own account enters a long of 10,000 units at the
tolerance below the committee's price of 103 (about 56,400 KAS of coins, which it has back), the owner calls at the
tolerance above, and the leg has gained 10,300 KAS: a claim at one share. A shard of 1,000 KAS pays an honest
claim of 350 with 33. No gate at an entry, no band under a call, no bound on a leg's size against what the
shard holds: I did not find what stops it. If it holds, the least fix is a band under a call for a leg that
entered within some time of the call, or the gate.

### N57 (Medium). The more rows, the more often a price nobody shows

A draw is ended, at its owner's cost, by one price of its trace that nobody answers in a dispute period; a call
at its caller's. A committee signs when asked. With four rows that is four prices; with sixty-four, of which
many are far strikes of one underlying, it is still one price an underlying if N55 is taken (and one written
price for each ended day, which needs no committee). Without N55 it is sixty-four citations a griefer may ask
for, one after the other, each at the cost of a bond it gets a part of back when one is late.

*Recommendation.* N55 and N49 together: the prices of a draw or a call are one an underlying that is live, by
the committee, and one a day that has passed, by its note.

### N58 (Medium). What a draw and a call cost as traces, and how long they can be held up

With the trace written whole in the statement, as an account's is (17.6 as written; a row of 120 bytes, a path
192, a statement 3.2 kB today):

| Rows | A draw stated | A row answered (the trace, the leaf, its path) | A price answered (with a citation of 4.4 kB) |
|---|---|---|---|
| 4 (today) | 3.2 kB | 3.9 kB | 8 kB |
| 8 | 4.2 kB | 5.2 kB | 9.5 kB |
| 32 | 7.0 kB | 8.0 kB | 12.5 kB |
| 64 | 10.9 kB | 11.9 kB | 16 kB |

(Estimates from the sizes given; nothing was built.) The applying does not grow. The same for a call, which a
stranger states: at sixty-four rows a caller pays for 11 kB and must know every row, which it has from the
chain.

**The trace need not be written.** Whoever watches a shard knows its rows, and the sums of a draw's trace
follow from the rows and the prices alone. So a draw may say its amount, a price a row (8 bytes) and the root
of its trace; a disputed row is shown with the row before, each by a path. A draw of sixty-four rows is then
3.8 kB stated, and a row answered about 5 kB whatever the count. With one price an underlying (N55) the prices
are fewer still. I would build this form at once: the tree's script for a path is written anyway.

*How many disputes.* One point is open at a time, and none lands after the statement's time, its lateness and a
dispute period (14.1). So a draw lives at most its lateness and two dispute periods whatever its rows; each
round costs a griefer a bond (a KAS at the least for a draw), a quarter of it to the owner. A griefer does not
stall a draw past that, and stalls every draw only by paying a bond a life: as today. What grows with the rows
is N57, not this.

*A step under a call* shows a price by a path for each leg: 200 bytes a leg, 1.2 kB at six legs, less with one
price an underlying. A note of six legs shows six rows and paths: 1.8 kB more than today. Both fit.

### N59 (Medium). What 17.4 does not say of the step, the split and the rest

- *Which leg a step names, and its fee.* A step is right where the leg named "is its largest" or the account
  stands after, and its fee is of the leg's worth. For an option: largest by which worth; and an option held
  far from its strike is worth nothing, so nobody is paid to step it (harmless alone; say it).
- *Section 4b says a written option is not ended by a liquidation* ("what it wrote stays until its holder
  exercises"); 17.3 says it leaves in a step. 17.3 is the one that can work under a shard (an account must be
  able to end). Say that 4b's rule is of an account no shard covers.
- *"The seven sums of a row (section 16.12's six ...)"* lists six. Say whether a row grows; with N47 it does.
- *Kind 10 refuses every option today* (`KpfTCRow`, lines 183 to 188: a direction that is not 1, 2 or 7 is
  wrong; `KpfTEnterS`: "the leg is a futures leg ... of the shard's committee"). Both change; with the leg of
  N52 the committee's place holds the strike, the day and the premium, and the committee is the shard's.
- *The limits of an entry that holds a premium* (if it is kept): a call's entry above the leg's price limit, and
  a put's at or below nothing where what it is worth and its premium reach its strike, cannot be written.
- *A token leg and options in one account:* six legs in all; a step of one leg on an account that holds tokens
  makes it owe (16.18). An option's leaving under that rule is not said; it should be the futures leg's.

### N60 (Medium). F114: what a called shard has paid

The rule of 17.6 is right, and it is the only one of those considered in 16.22 that pays the same whatever the
order. Written out:

- The shard, called, has: H, what it holds; C, what it owes by claims, **which does not fall when a claim is
  paid**; D, what it has paid to claims. T = H + D does not change when a claim is paid, and grows by every
  coin that reaches the shard.
- A claim is a note of the key, its amount A, and **a, what it has been paid**. Nothing else.
- Where T >= C the claim is paid A - a and spent. Else, with `KpfTCallPay`'s unit u chosen from C as today (so
  u no longer changes between two claims): o = C in units rounded up, h = T in units rounded down, the part
  e = (A / o) x h + ((A mod o) x h) / o, and the claim is paid **the smaller of e - a and H, and nothing where
  e is not above a**. H falls and D and a rise by what is paid; the note is written again while a is below A.
- The products stay within 64 bits as today: h is below o, and o is at most 3,000,000,000.
- The sum of the parts is never above T (each is rounded down), so no claim finds less than its part while C
  stands still.
- *C grows after claims began* (a mark's note, or a note of the rest, applied late): the claims paid before
  were paid a part of a smaller C and keep it; the late one finds less. And a larger C may cross a step of u,
  which makes e smaller than a for a claim already paid: hence "nothing where e is not above a". To keep it
  rare: no claim is paid before the call's time, a lateness and two dispute periods (no statement of before
  the call is still to be applied), and whoever holds a mark's note applies it by then.
- *The call's end* is where the count of rows is nothing and D = C; both are then set to nothing. A shard that
  never holds enough stays called for good, its claims standing: that is right (it is what makes late coins the
  claims'), and it is to be said. An owner that tops the shard up ends it.
- A part of less than a note's least is not paid and a is not moved: the claim waits until its part has grown
  by that much, or is whole.

### N61 (Low). An option cannot be sold back

On an account no shard covers an option is bought back by two signatures (`KpfTLegOut`, 4b), which takes no
covered account (14.2). Under a shard a user that bought a call and wants its price back has: its exercise (it
loses what the option was worth beyond that), or a split and a hand-over of the new account to whoever pays for
it (16.2; a dispute period and the join's wait). Writing the same option against it leaves two legs standing to
the day, and the premium received lies in the account until then.

*Recommendation.* Say it as it is in 17.4: "sold" is a split and a hand-over. A leg that cancels another of the
same series (the two leave together, the premiums settled) is a kind of its own for later.

### N62 (Low). Whether the user can be kept from adding coins

Not checked. If an account under a statement takes no coins (`KpfTAddC` waits by the account's time), then any
statement on a user's account by another key (a step today; an exercise in 17.4) keeps the user from topping up
for a dispute period at the cost of half a bond. With forfeits that pay the shard where a user is stepped, the
owner has a reason. To be looked at when the exercise is decided (N53).

### N63 (Medium). The order of 17.7

See N-m. Steps 2 and 3 as written build the leaf and the trace before the option's rules have said what a row
is named by and what a trace sums; every one of the seventeen scripts 17.7 lists would be written twice.

### N64 (Low). A dated market's name and the rows' names

The tags in use are 13 (a market's options) and 14 (its futures) for a row, 19 for a dated market. With N52
there is no name of tag 19, and a row's key is plain. If 17.2 is kept as written, say that a row of a dated
market's options is named by the dated name, and that `KpfTShardPrice` is shown the underlying and the day.

## Recommendations on the author's questions

### N-h (17.3): the premium

**Decide: the premium is a number of its own and coins in the user's account. It is not in the entry and not
in the books.**

1. An option leg says its strike, its day and its premium a unit (N52: in the place of the committee). Its
   entry is the price of its market where it entered, held and answered as every leg's (kind 10 as built).
2. *A user holds:* its own coins stand for the premium, whole, for as long as the leg stands (its row adds the
   premium to what is asked whatever the price: N46). It is paid to the shard when the leg leaves by its own
   leaving or at its day; as far as the coins go, after what the legs lost, in a step.
3. *A user wrote:* the owner puts the premium into the account in the transaction that states the entry, from
   coins of its own (the transaction already takes "the coins added" from any input; the shard is not paid
   from). They are held as the holder's are: no take reaches them while the leg stands. At the day they are the
   user's. In a step of the user the larger of the premium and what the option lost is paid to the shard.
   `KpfTEnterOff` pays the premium back to the owner's key where the entry ended (today the coins added stay:
   a user would end its own entry for a bond and keep them).
4. *Under a call* no premium moves: a holder keeps its own, a writer keeps what the owner paid. What the option
   gained or lost exercised, from its entry, is settled as a futures leg's.
5. *The books* take an option at what it is worth exercised at its entry, by its units: one number over all
   rows stays exact, and no premium is ever equity.

What this answers of the question as asked:

- *An entry that is not the market's price: what holds it.* Nothing does, and nothing can short of the gate
  (N43). So do not have one.
- *The band against a premium.* N54: exact, of the option's own worth; nothing at a written settlement price
  for an option entered out of the money.
- *A holder that cannot pay the premium at the end.* It holds it from the entry; other legs' losses can take
  the coins only through a step that was due earlier. The shard never counted it, so nobody else is short.
- *Is "no gain for the writer while the leg stands" enough?* Yes for the writer: I found no way to the premium
  before the end through a take (a take counts no gain), a mark (none names an option), a split (each account
  stands alone), a hand-over (the coins go with the account) or a sale (tokens only). The way out that 17.3
  leaves open is the day itself, which the two signers choose: an option that ends tomorrow, or yesterday (N43,
  N51).
- *What it costs a writer.* A covered call under 17.4: 110 KAS and the premium of 20 at the end, where an
  exchange asks about 55 and credits the 20 at once. With N47 and the escrow: 55, and the 20 lie in the
  account.
- *The forfeits.* Fair in a step (the user gains nothing by its own default), once they are of the premium
  alone (N45). Under a call the user keeps the premium, which is fair, and loses the option, which the owner
  can bring about (N48): say it.

Can be decided while building: whether a premium a user was paid may stand as margin for its other legs (it
cannot leave the account, and whatever the account loses is paid to the same shard; with it the spread of N47
is asked 30). I think it is safe with the escrow and it is not safe without.

### N-i (17.2): the end

**Decide: the day is in the leg, the price of an ended leg is the committee's at its day, and that price is
written once (N49, N52).** An ended leg leaves by one transaction of anyone's, shown the written price, for a
keeper's fee, with no band and no dispute period (N50). No entry within a statement's life of the day (N51).
Futures have no day. What a leg is worth between its day and its leaving is its worth at the written price, in
every trace; where no price was written by a time the terms say, the leg leaves at its entry.

### N-j (17.4): the leg in an account

**Decide: three sums more a row and the rule of N47.** The holder's base is the smaller of what the option is
worth exercised and the rate of its underlying, which the two sums give; the premium is asked whole beside it.
A holder under a step: the option leaves at the step's price, what it gained exercised is asked of the shard
and set against what the account owes (as F112), the premium is paid as far as the coins go. A call written
against a long: netted where the two are next to one another in the chain. Six legs: enough for the first
form; what matters more is that an entry may place its leg, so that a hedge can stand beside what it hedges.

### N-k (17.5): the tree

**Decide: a tree whose leaves are a list in order of a plain key (N55), a count of rows that have a unit, the
depth in the terms (N56).** Not a longer list: at eight rows a list written whole is already 816 bytes in
every transaction that shows the shard. A draw that leaves a row out or names one twice is wrong at the row
after the gap (its leaf is not the one the row before named). An empty leaf is one value. An entry where the
row is new shows the leaf before it and writes two leaves. Sixty-four rows are enough for a first form and not
for an exchange's list of options.

### N-l (17.6): the traces and F114

**Decide: the trace by its root, with the prices written (N58).** A draw of sixty-four rows is 3.8 kB stated
and about 5 kB a row answered. A draw is held up for its lateness and two dispute periods at most, at a bond a
round, as today. The call's object holds one price an underlying and one for each ended day; a step shows each
by a path. F114 as N60 writes it: the claim's note remembers what it was paid; C does not fall; the call ends
at D = C.

### N-m: the order

**Neither as asked: the option leg comes first, in the model and not in scripts.** What a leaf is named by, what
a trace's row sums and what the call's object holds all come from 17.3 and 17.4, and a model finds them in days
where scripts find them in rounds of seventeen scripts.

1. **The first commit: the Rust model, no script.** An option leg in `covered.rs` with the premium in escrow
   and the three sums of N47; option rows in `reserve.rs`; and the owners of N43 to N48 as tests in numbers, as
   `research_solvency.rs` did for section 12 (an absurd premium; a draw on a premium; a holder deep in the
   money under a provoked call; a premium taken and a default chosen; the take after a move up). Review of the
   model.
2. The tree, futures only, with the leaf as it will stay (the plain key with a day, a kind and a strike of
   nothing; the next key; the count): every test of today passing. This is 17.7's step 2 and it is the right
   first large piece of script.
3. A draw and a call as traces by a root, one price an underlying; what a called shard has paid (N60).
4. The written price of a day, and the transaction by which an ended leg leaves (in place of 17.7's step 1).
5. The option leg on the account's side: `KpfTCRow`, the ends, the statement's rate.
6. Option rows; the entry with its premium; the step, the call's step, `KpfTEnterOff`.

**Safe to leave out of the first form without giving up the goal's first row:** the owner's exercise of a
written option and any exercise before the day by anyone but the holder (N53); dated futures (N52); sums in the
tree's inner nodes (N56); a depth above six; a leg that cancels another of its series (N61); the deterrent of
N48; a premium received as margin (N-h). **Not safe to leave out:** the netting of N47 (it is the goal's row),
the escrow, the written price of a day, the ordered leaves.

## What section 17 contradicts or relies on that is not built

| Section 17 | What is built or decided | |
|---|---|---|
| an option entered at a price that is not its market's (17.3) | kind 10's last row is the leg at the price stated, and that price is answered by the committee's (`KpfTCRow` 196 to 201; 14.3) | contradiction; N43 |
| an option leg in a covered account (17.4) | `KpfTCRow` takes directions 1, 2 and 7; `KpfTEnterS` takes a futures leg of the shard's committee | to be changed; N59 |
| the owner's key states on a user's account (17.4) | `KpfTChange` lets any key state; no end holds a claimer to the second key; review 42 decided that nobody but the user states but a step | can be built; contradicts H1; N53 |
| "it signs one price for every time" (17.2) | a citation is of the statement's own time (`KpfTStepPrice` 213); a committee signs when asked | not held by any script; N49 |
| an ended leg leaves "as a leg leaves by its user's statement" (17.2) | a leaving of one leg that gained asks the coins to stand at what the others lost (S2) | such a leg does not leave; N50 |
| a written option leaves in a step (17.3) | 4b: what a side wrote stays until its holder exercises | say which; N59 |
| "the seven sums of a row" (17.4) | a row has six sums (16.12) | N59 |
| a call ends where no row has a unit (14.6, kept) | `KpfTCallPay` compares the four rows whole | a count; N56 |
| no second row of a market (14.3) | `KpfTEnterS` reads every row | needs the ordered leaves; N55 |
| the fixed part holds the root of the rows (17.5) | the fixed part is the hash of the terms and of the hash of the rows | fits as it is: the root takes that hash's place |

## Decide before any script

1. Where the premium lives: in escrow in the user's account and out of the books (N-h), or in the entry with a
   gate and a second class of claim (N43, N44, N45).
2. What a holder is asked, and the rule that nets (N46, N47, N-j): it sets the row of a trace.
3. The day in the leg and the price of a day written once; an ended leg leaves by a transaction (N49, N50, N52).
4. The leaf: a plain key, in order, with the key after it; one price an underlying in every trace (N55).
5. European in the first form: no exercise by the owner's statement (N53).
6. What a written option's premium is under a call, if 1 is not taken (N43, N48).

## Can be decided while building

- The band of an option, exactly (N54), once the statement says its rate and tolerance.
- The trace by a root or written whole (N58); the depth of the tree (N56).
- What a leg does where no price of its day was ever written (N49).
- The time before which no claim is paid, and the least part paid (N60).
- Whether an entry may place its leg in the chain (N47), and whether a premium received is margin (N-h).
- What is said to a user of a call that ends its options (N48), and the deterrent.
- How an option is sold before its day (N61).
- N56's last paragraph, at the engine, by the other reviewer: it is of the built form.
