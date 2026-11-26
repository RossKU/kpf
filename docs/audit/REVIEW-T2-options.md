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

# Second round

27 November 2026. The second writing of section 17 (17.1 to 17.9), on paper. Nothing was run. Read for this
round, beside the section: `KpfTCStep` (the step, lines 84 to 273), `KpfTCallEnd` (the head and the answer to
line 209), `KpfTEnterS` (the stating, lines 100 to 200), the tolerance's lines of `KpfTStepPrice` and
`KpfTShardPrice`, `call_step_at` in `covered.rs`, and again `KpfTCEnd`'s ends. Findings N65 to N77. Numbers
with the terms of the first round (5 %, 0.5 %).

## Verdict of the second round

**Step 1 (the model, no script) can start.** The second writing took the five decisions whole and nothing of
N43 to N46 is left in it: I worked the four cases of a premium again and found no state in which a premium is
counted twice, and no way to one early (N70). What I found this round is of three kinds:

- **What netting brings with it, which the first round's rule did not say** (mine as much as the author's): a
  leg taken out of a group leaves the rest asked nothing (N65); a hedge ends by the clock (N68); my rule for a
  held option has a cliff (N69).
- **The day as the author wrote it**: the gain of an ended leg paid to the key is S2 again (N66), and "in no
  statement until it has left" makes an account nobody can step and a shard nobody can call for as long as a
  day's price is not written (N67).
- **17.9, the built form.** By reading it holds (N76). A band under a call does not close it: it leaves a short
  0.005 % of its worth and does nothing about the move between the entry and the call, which with no bound on
  a leg's size is the larger part.

Five rules to put into the model before its first test, each a line: N65's "stands after", N66 and N67's "an
ended leg is passed at its day's price and leaves when the account stands without it", N68's "no credit within
a time of its day", N69's two lines, and N76's time in the leg. None needs a script decided.

## Findings

### N65 (High). A leg taken out of a group leaves the rest asked nothing

With R in the place of the rate on W, two legs that set one another off are asked nothing. Every way one of
them leaves alone must then ask the account again. Two do not.

*A leg that lost leaves by its user's statement.* Its end asks only that the coins pay what it lost (`KpfTCEnd`,
lines 247 to 249). Today that is safe: without the leg the account is asked less. With R it is asked more.

*Scenario.* A long and a short of 10,000 units of one market, both from 100, on 10,100 KAS: asked nothing. The
price goes to 101. The user states that the short leaves: it lost 10,000, the coins pay. What stays: a long
worth 1,010,000 on 100 KAS of coins and 10,000 of gain: a standing of 10,100, where a step is due below 45,450
and an entry is asked 55,550. Up 1 %: 10,100 gained. Down 1 %: nothing left; beyond that the shard is unpaid.
An entry of that long alone would have cost five and a half times the coins.

*A step of one leg.* "A step names legs as today": the leg named lost, and is the account's largest or the
account stands after. An account of that pair and a third leg that lost is due for a step; whoever steps names
the short of the pair (it lost, it is the largest). The pair is broken, the account is asked 45,450 more than
before, and is then stepped whole for the fee of everything. The stepper chose the step that made it worse.

*Recommendation.* One rule for every statement that takes a leg out and leaves others (a leaving of one leg
that gained or lost, a split's two accounts, a step of one leg): **the account that stays is tested as it
stays.** For the user's own statements: at the level of an entry (the coins and what the legs that stay gained
at least R of those and their premiums). For a step: it is right only where what the account lacks is less
after it than before it; else every leg leaves. Whether a step names a leg or a whole group is then decided by
that test and needs no rule of its own. The model's random accounts should try every leg out of every account.

### N66 (High). An ended leg's gain paid to the key is S2 through the day

17.5: "What it gained exercised is asked of the shard by a note": a note pays the user's key. The transaction
has no trace and tests nothing of the legs that stay.

*Scenario.* A long of 10 units from 100 and a put held, strike 100, premium 3, on 85 KAS. The price falls to
80: the long lost 200, the put gained 200; the account stands (nothing net, and the premium). The day comes at
80. The put leaves: 200, less its band, to the user's key; 30 of premium from the coins. What stays: a long
that lost 200 on 55 KAS. The shard has paid 200 out and is owed 200 by an account of 55. This is S2 (review
44) with a day in place of a statement, and at an owner's own keys it is of any size.

*Recommendation.* An ended leg that lost always leaves (the coins pay; what they do not pay the account owes:
16.18's number). **An ended leg that gained leaves by the day's transaction only where the account passes the
test of a leaving without it** (the coins at R, the premiums and what each leg that stays lost alone). Else it
stays, at its day's price, as a gain that counts for the account's standing and is no coin, and leaves when
the test passes or with every leg. That needs N67's change: an account must be able to go on with such a leg.

### N67 (High). An account with an ended leg is in no statement: nobody steps it, and nobody calls its shard

"A row that names or passes a leg whose day is at or before the statement's time is wrong. The account does
nothing else until its ended legs have left." A step is a statement whose trace passes every leg. So between
a leg's day and the day's transaction no step is right; the transaction needs the note; the note needs the
committee's signature of that time.

*Scenario.* A long of 10,000 units from 100 on 55,550 KAS, and one call held of one unit, premium 0.01, whose
day is now. After the day the price falls 4 %: the account lost 40,000 and a step is due. No step can be
stated until someone has the committee's price of the day and has written it. A further 2 % in that time and
the shard is unpaid 4,450. Where the committee does not sign that time, it is for the whole of the fallback's
wait. It costs the user 0.01 KAS, and it is with N66 the reason the rule cannot stand: a leg that must leave
first, and cannot leave safely, stops the account.

The same on the shard: a call's trace says an ended row "at the day's price, and that point is answered by the
day's note". Until the note exists that point is answered by nobody and ends the call at the caller's cost. An
owner's shard cannot be called from each day until that day's price is written.

*Recommendation.* **A leg whose day has passed is passed by a row at its day's price**, and adds what it gained
or lost there and nothing to the two sums. That price is answered by the day's note, or, where none is written
yet, by a citation of the committee's price at the day (the signature the note would be written from). Nothing
is frozen. The day's transaction is then the cheap way out (N66), not a gate. For a shard's row of an ended
day whose price nobody has written: the leaf holds what that row's legs were entered at (8 bytes more a leaf,
their sum still the one number), and such a row is counted at that in a call's trace until the price is
written. It is what the fallback pays, so the two agree.

### N68 (High). A hedge ends by the clock

Netting is right up to an option's day and wrong after it, and no transaction stands between.

*Scenario.* A long of 10,000 units at 100 and 10,000 puts held at strike 150, whose day is tomorrow. The two
sums are nothing: the account is asked the premium (say 1,000) and holds a million of futures. At the day the
put is a number that no longer moves. The long is asked 55,550 from that second, the account has 1,000, and
every tick down from there is the shard's. A take stated a second before the day and applied after it leaves
the same account by a statement that was right.

*Recommendation.* **Within a time T of its day an option is counted in the two sums only where it adds to
what is asked** (what it would lose at each end, never what it would gain). T is a term: twice the longest a
statement lives and the time between two visits. The account is then asked the long whole while both legs
still stand, and a step that takes the group takes it at its net. The same T as "no entry within a statement's
life of the day" should be: one number.

For a shard's requirement the same jump comes at each day (a row of an ended day sets nothing off). Count it
the same way, or say that an owner tops its shard up before a day.

### N69 (Medium; High for a user near a strike). The rule for a held option has a cliff, and it is mine

"Where its strike is inside, the worse of the two ends in both sums." It is sound. It is also a line that gives
a held option no credit at all from the moment its strike is within the rate of the price.

*Scenario.* A short of 10 units and a call held at strike 96, ten units: above 96 the account is flat. At a
price of 102 the strike is outside the range (L is 96.39): asked nothing. At 101.5 (L is 95.92) it is inside:
the call is counted as losing all it is worth at both ends, 55; with the short's 55.8 the account is asked
110.8, eleven per cent of what it holds, and at the lower rate is due for a step. Nothing it can lose changed.

*Recommendation.* A held option is never worth less than either of two lines: nothing, and the price less its
strike (for a put: the strike less the price). **Where its strike is inside, it is counted along one of the
two, at both ends; the row says which** (a bit), and is right with either. Whoever states picks the one that
asks less. Above: along the second line the call is the short's exact opposite at both ends, and the account is
asked nothing at 101.5 as at 102. At a strike outside the range the two rules are the same, so nothing jumps.
The reasoning of 17.4 stands as written (a line below a held option, the rest bends downward, the least is at
an end). The model's test "at every strike between" is the right one for it.

### N70 (Medium). The premium in each test, the four cases, and a thing of mine to take back

*Each inequality.* m is the coins less a note's least; P the premiums of the options that stay, held and
written alike.

| Test | As written | A held option's premium | A written option's premium |
|---|---|---|---|
| an entry | m + G >= R + P | the user's own coins stand for it, beside R | the owner's coins are in m and in P: they cancel. The user's own coins meet R |
| a step is due | m + G + TK - owed < R (lower rate) + P | due while the coins still hold it: it is paid whole but in a gap | cancels: the step is due exactly where the user's own coins fall short. Not earlier |
| a take; a leaving of a leg that gained | m - amount >= R + P + LS | cannot leave | cannot leave |

No premium is counted twice: in every test it is on the side of what is asked, so the coins that stand for it
stand for nothing else. Asking a written option's premium the same way is right and neutral. The one way the
coins of a premium pay another leg's loss by the rule is the leaving of a leg that lost (no test after): N65
closes it. In a gap beyond the rate they can pay a loss in a step; that is the gap any margin has.

*Checked with a holder in the money:* a call at strike 100 entered at 110, premium 2, ten units, 80.5 KAS: a
step is due from about 108.8, where the coins pay the 12 lost and the 20 of premium whole.

*The four cases* (a call at strike 110, ten units, premium 2: 20 KAS; a long of ten units from 100 beside it):

| | What moves | Who gains by it | Can it bring it about |
|---|---|---|---|
| The user holds; its step | the option leaves at what it is worth exercised; the 20 to the shard, after the losses | the owner: the premium now and not at the day, and an option it wrote is gone | only by a price at the tolerance's edge, as any step today |
| The user wrote; its step | the shard has the larger of 20 and what the option lost | the owner: its 20 back for an option that has lost its price. The price falls: the long loses, the call is worth nothing, the step is due: the two come together | see below |
| The user holds; a call | nothing of the premium; what it gained exercised is a claim | the owner, by what the option was worth beyond its exercise | yes, at will (N48: said in 17.3) |
| The user wrote; a call | nothing; the 20 stay; what it lost exercised the coins pay | the user; the owner loses the premium it paid and the option | the user cannot |

*An owner that writes to itself or buys from itself:* its own coins on both sides; nothing in the books. What
its legs gain exercised is a claim as a futures leg's: that is 17.9 and not the premium.

*The second case, with numbers.* Own coins 55, the 20 beside them. At 98.8 the long has lost 12: 63 against
44.5 and 20: a step is due. Whoever steps names the call (it is worth nothing, it lost nothing): the shard is
paid 20. The account: 43 against 44.5: still due. The step took the user's premium and changed nothing of what
the account lacks. By N65's rule (a step is right where the account lacks less after) that step is wrong, and
the long is what a step names. Take N65 and this is closed.

*To take back.* The first round said a premium a user was paid might stand as margin for its other legs ("I
think it is safe with the escrow"). It is not. One user, two accounts: A wrote an option and holds 1,000 of
premium and a long; B holds the same short. The price falls: A's loss is paid to the shard out of the premium,
B is paid the same by the shard, A is stepped with nothing left. The premium has left through B. 17.3's line
("not margin for its other legs in the first form") should lose "in the first form".

### N71 (Medium). A held option in a step; "every leg" before the day

- A held option cannot lose more than what it is worth exercised above its entry and its premium, and the
  coins stand for the premium. Section 4b had: a holder that keeps nothing else is liquidated by nobody. Under
  17.4 a step of every leg ends it at what it is worth exercised, and its holder loses what it was worth
  beyond: the first row of N70's table. *Recommendation:* a step of every leg takes the legs that are not held
  options; a held option goes on with the account where the coins left hold its premium (as an account goes on
  with its token legs: 16.18), and leaves only where they do not.
- A user's leaving of every leg (kind 8, -1) ends the account. With an option in it that is an exercise before
  the day, which 17.5 forbids. Say that it is wrong while the account has an option, as it is while it has
  tokens.

### N72 (Medium). The fallback, and who gains by a committee that says nothing

"Where no price of a day was written by a time the terms say after it, an ended leg leaves at its entry, and
its premium returns to whoever paid it." Silence then pays whoever is out of the money: the writer of an option
that ended in the money owes nothing (a call written at 100, the day at 130, ten units: 300 not owed), and the
holder of one that ended worth nothing has its premium back. One party to every option under a shard is the
owner, and the committee is the one the owner named in its shard's id. A committee can also sign a false
price; but that is a signature anyone holds against it, and silence is nothing anyone holds.

*Recommendation.* Keep the fallback: the other way is coins locked for good. Make its time long (days, not
dispute periods), since with N67's change nothing waits on it but the leg itself. Say it to a user beside
14.7's lines: a committee that does not sign a day's price returns every option of that day to its entry. And
if the committee's own design has a cost for a member that does not answer, this is where it is wanted.

### N73 (Medium). The shard's side of a day

- *"A draw is not stated over a row of an ended day that has a unit."* A user cannot keep the owner from
  drawing: the owner sends the day's transaction for each account itself and has the keeper's fee back. It
  costs two transactions an account that held an option of that day (the transaction, and its note applied,
  one after another on the shard's one output): a thousand accounts, two thousand transactions before the next
  draw. With the day's price written a draw's trace can say such a row exactly. *Recommendation:* a draw is
  stated over a row of an ended day at its written price, and not over one whose price is not written.
- *An account the day's transaction cannot take:* one under a statement stated before the day (it waits for a
  statement's life at most), and one that owes (say that the transaction takes it).
- *A call:* N67.
- *The rate on the gross of an option row:* as today it is of the units' worth as futures, 1.75 % of the
  underlying on each side of a book set off, where the options' own price may be 1 %. It was sized for a side
  that leaves; before its day an option leaves only in a step. A number to set in the model.

### N74 (Medium). The leaf and the trace: what a script still needs said

- *The head.* A leaf at the first place with the least key and no row, never taken out and not counted. The
  tree is then never empty, the leaf before a new row always exists, and the same two paths serve.
- *The two paths of a new row* are of one tree that changes between them: the leaf before is shown against the
  root as it is; the empty leaf against the root computed with the leaf before rewritten.
- *A trace's first row* is tied to the head: its answer is shown the head's leaf. The last names none. The
  statement says how many rows its trace has, so that the end is shown the last one.
- *The order of keys.* A script compares numbers of 8 bytes, not strings of 32. Any order serves that is whole
  and the same in every script: the market's 32 bytes as four numbers compared in turn, then the day, what it
  is, the strike. The market first is what puts an underlying's rows next to one another.
- *The key is more than the market.* A row today is named by its market, committee, scale and price limit
  (`KpfTCStep` line 122 holds a leg to all 84 bytes): its units are worth nothing certain without the scale.
  The scale and the limit stay in the key, after the market. "One underlying" for the one-price rule and for
  the groups is the market alone.
- *The call's object* needs no tree of prices of its own: it holds the trace's root, and a step shows the
  trace's row of each leg's key by a path. That the prices are one an underlying is already the trace's.
- *What each leaf holds beside its row:* what its legs were entered at (N67).
- *Rows of one underlying with different days:* live ones are counted together in the two sums (they move with
  one price); an ended one adds its worth at its day's price to the equity and nothing to the sums; one within
  T of its day, N68.
- *When claims begin:* 17.7 gives the time; say with it "and the count of rows is nothing", as 14.6 does.

### N75 (Medium). The row of an account: more than three sums, and four things said

- *Three sums are not all.* Beside D-down, D-up and R the ends need: the premiums of the legs that stay (they
  can be added into R as each option's row is passed); what the legs that leave pay as premium (a held one its
  premium; a written one, in a step, what the larger of its premium and its loss is above its loss); and for a
  split, what the leg that leaves is asked alone. Count them in the model; a row is nearer 160 bytes than 144.
- *The band stays of W,* the worth of what leaves, each leg whole. R is no base for it: a pair that sets off
  has an R of nothing and two edges.
- *The fee of a step,* "what is asked for the legs named": for a group that sets off that is nothing, and
  nobody steps it. Keep the worth of the legs named, as today.
- *An entry that says its place.* It costs 8 bytes and nothing else, if it is done as a leaving is done in
  reverse: the trace's rows are in the new order, the first chain passes the new leg by, the second takes it
  in its place, and the end holds the first to the account's root and writes the second. The marked account
  keeps the leg at the end of its chain (so that `KpfTEnterOff` is as it is), and the order is written when the
  entry is applied.
- *A middle leg that leaves* puts the legs on each side of it next to one another: if they are of one
  underlying they are one group from then on, which asks less and is sound. A hedge is never parted silently,
  given N65: every trace is of the legs that stay. The transactions that have no trace: the call's step (all
  leave), an entry undone (the account is what it was before), and the day's transaction (an ended leg adds
  nothing to the sums, so the rest is asked the same with it or without: the change came with the clock: N68).
- *A join* is sound: two accounts that each stood are asked no more as one. *A mark* keeps its test (the other
  legs' losses each alone), so a mark of the leg that gained in a pair needs the coins of the one that lost.
  *Token legs and an account that owes:* nothing changes.

### N76 (High; of the built form; by reading, not run). 17.9 holds, and a band under a call is not enough

*What the scripts say.*

- A step under a call asks what the legs gained, whole: `KpfTCStep` line 178, `int asked = gain`, where the
  gain is the worth at the call's price less the worth at the entry (lines 132 to 134). Nothing is taken off
  but what the account owes (lines 219 to 224). `call_step_at` in `covered.rs` (lines 984 to 988) is the same.
- The call's test moves each price the tolerance in the shard's favour (`KpfTCallEnd`, lines 175 to 178). The
  call's object holds the prices as they were stated (lines 206 to 207), and the steps use those.
- A stated price is right from the committee's less the tolerance rounded down to the committee's and the
  tolerance rounded up (`KpfTShardPrice` 173 to 176, `KpfTStepPrice` 224 to 229): the two bounds are rounded
  outward. The tolerance is the terms', one number for a shard and its accounts.
- An entry's price is the leg's entry (`KpfTCRow` 196 to 201) and is held to the committee's only if someone
  disputes it. `KpfTEnterS` holds the leg to its numbers' bounds and to nothing of what the shard holds.
- Anyone states a call, for a bond of the shard's rate, 0.2 KAS at the least, and has the bond and a keeper's
  least fee back.

So the first round's numbers stand by the rule: a long of 10,000 units entered at 102.485, a call at 103.515,
10,300 KAS claimed at one share, an honest 350 paid 33.

*Is the band enough, exactly?* With the committee at c both times, and the band as kind 8's end has it (twice
the tolerance of the worth at the price of the leaving, rounded up):

- A long: gained 2gc a unit, and up to two steps of the price by the rounding; the band is 2g of c(1 + g). It
  is left 2 steps less 2g²c: nothing where a price has 40,000 steps or more (at 0.5 %), and something below.
- **A short: gained 2gc; the band is 2g of c(1 - g), the lower price. It is left 2g²c: 0.005 % of its worth,
  51 KAS on that leg.** Small, but it is not nothing, and it is so at every leaving today as well.
- The exact band, either way: the tolerance over one less the tolerance, of the leg's worth at its entry and
  of its worth at the price, each rounded up; and the two bounds of a stated price rounded inward.

*And the band is the smaller part.* It takes back the two edges. It does nothing about what the price moves
between the entry and the call. The leg's size has no bound, the books take it at once, the shard is far below
its lower level from that transaction, and until someone calls the owner holds a bet that pays one way: the
price up 0.1 %, the leg claims 1,030 KAS beyond its band and the honest 350 are paid 254; up 1 %, 33 again;
down, the account pays its own shard and the owner has it back when the call ends.

*What a band under a call costs.* Every leg that gained, of every honest user, 1 % of its worth (a long of
1,000 KAS up 2 %: 9.8 of its 20), paid at a call it did not choose; and what a called shard keeps is its
owner's when the call ends. An owner whose shard is whole and whose users are up has a reason to be called
that it does not have today.

*Recommendation, one.* **A leg says the time it entered, and under a call a leg that entered less than T
before the call's time asks nothing where it gained and pays what it lost.** T is 14.4's bound (a lateness, two
dispute periods, the time between two visits), a term of the shard. No band under a call, as today.

- The time is held where the entry is stated (`KpfTEnterS` has the statement's time). A split, a join, a
  hand-over and a mark carry the leg's bytes and its time with them. Under a shard the leg's 32 bytes of a
  committee can hold it (the committee is the shard's, shown apart, as it is for a token leg's vault), beside
  the strike, the day and the premium: the leg stays 111 bytes.
- The round trip: the leg that took the shard below its level is younger than T at any call that whoever
  watches states in time. It gained 10,300: it asks nothing. The honest 350 are paid whole. Had the price
  fallen, it pays. Worth nothing or less, exactly, with no arithmetic of edges.
- A leg older than T entered while the shard, with it, was not called for T: it was within what the
  requirement carries, and its two edges are what the lower level's "the tolerance in the shard's favour" and
  the upper level's "the tolerance more" already count.
- What it costs an honest user: one that entered within T of a call has no gain of those minutes. Nothing
  else. It is to be said beside 14.7's lines.
- It rests on what everything rests on: that whoever watches calls a shard within T of the transaction that
  took it below its level.

*The same round trip with no call.* One path takes no band: **the leaving of a leg that lost** (`KpfTCEnd`:
what it lost at the price stated, whole). A long in one account and a short in another, each entered at its
edge, the price then moved by more than the band: the one that gained is paid what the committee's prices
give, less 2g² of its worth; the one that lost pays that less 2g. The pair has 2g less 2g² of the notional,
0.995 %, a round. Against an honest owner it does not exist: the owner signs every entry and its tool signs no
edge (14.7); what is left is the leaving's edge, the tolerance once, which the band of the one that gained
pays for. With the owner's own keys on both accounts it is the owner taking 1 % of any size from its shard by
notes that are paid as they come: no draw's rule, and held only by the call that the entries make due. The
rule above closes that at a call; where nobody calls for T it is open, as a draw nobody disputes is.

The other paths: a mark leaves the band in the leg; a step of a leg that gained takes it (`KpfTCEnd` line 314,
for kinds 8 and 9 alike); a leaving of every leg takes it of W. 17.5's transaction at a day's written price
takes "only the entry's part": right, the written price has no edge; write the entry's part as what the option
is worth exercised at its entry moved by the tolerance over one less the tolerance, not by the tolerance.

### N77 (Low). Where the second writing is against itself or what is built

- 17.4, "a step names legs as today", against netting: N65.
- 17.5, the gain "asked of the shard by a note", against S2: N66. "In no statement", against a step: N67.
- 17.4, "a row grows by three sums": N75.
- 17.9, "makes the round trip worth nothing": not for a short, and not the move: N76.
- 17.3, "in the first form": N70.
- 17.8's step 2 says every test of today passes on the tree; step 5 changes what a futures account is asked
  (the last row of 17.4's table), so the tests of amounts change there. Say it at step 5.
- `KpfTEnterS` holds a leg's 32 bytes of a committee into the shard's id (line 174, review 51's F80). An option
  leg that keeps its strike, day and premium there is shown the committee apart. A line in 17.2.
- 17.6 keeps "the rate on the gross as today" and 17.7 one price an underlying; with N74's key the gross of
  two rows of one market and different scales is of two rows: as today.

## What must still be decided before step 1's first test

1. N65: the account that stays is tested as it stays; a step is right where the account lacks less after.
2. N66 and N67: an ended leg is passed at its day's price; it leaves by the transaction where it lost, and
   where it gained only if the account stands without it; nothing is frozen.
3. N68: no credit within T of its day (one T, with "no entry").
4. N69: the two lines for a held option.
5. N76: the time in the leg; a leg younger than T under a call. And the two bounds rounded inward.

To be found in the model, not decided before it: the sums a row needs (N75); the rate on the gross of an
option row (N73); the fallback's time (N72); whether a held option goes on through a step of every leg (N71).

# Third round

27 November 2026. The model in numbers (`crates/kpf-protocol/tests/research_options.rs`, commit 74f5cc1) and the
third writing of section 17. This round ran the model: the one command allowed
(`cargo test -p kpf-protocol --test research_options`), seven times, never beside another. Nothing else was run.
The model file was added to and nothing in it was changed: the author's eight tests pass as they were, beside
eight of mine (sixteen, 14 seconds). Findings N78 to N85.

## Verdict of the third round

**The model is faithful to the paper where it claims to be, with five things said below, and its result stands
against a stronger owner and stronger users** (an owner that pays its shard up after its own entries so that a
run goes on to the days, states every price at the tolerance's edge and sets its book off; users that know the
next price and take all a take gives; the levels and the steps counted at exactly what can be lost): nobody
unpaid on 1,000 paths a set, eight sets, at ticks of 0.5 %, 2 %, 4 % and 4.5 %.

What the model did not show and the added plays do:

- **N78 (high).** "A leg younger than T asks nothing where it gained and pays what it lost", said of a leg: a
  hedge that is young at a call pays the leg that lost whole and is asked nothing for the other. It must be of
  an account's young legs together. This is in `call` as the author wrote it and would be in `KpfTCStep` at
  step 2.
- **N80 (high for a user near a strike).** "The stater takes the line that asks less": whoever states a step is
  not the account's user. By the lines that ask most, 60 % of accounts with a held option between the ends are
  due for a step while holding half as much again as they can lose.
- **N83.** The owner's own plays end a run of `run` within two to four ticks, before any option's day. What
  17.4 concludes from it (the hedge that ends by the clock "is a user's play") is not so: with an owner that
  pays its shard up, users are unpaid on 131 of 1,000 paths at ticks of 4 % without the day's rule.
- **N79, N81, N82.** What the claim does not count (a young leg's gain), and where its edge is: at the lower
  rate less the tolerance for an account, and at twice the rate on the gross for a book one side of which
  leaves between two visits, which is 2.5 % at a shard's lower level and not 4.5 %.

**Step 2 can start now, beside the model's next round**, once N78 is decided: it is one line of what step 2
writes.

## 1. Is the model faithful?

| | In the model | Against the paper |
|---|---|---|
| `asked_as`, the two lines, `near` | the two sums at the ends of a move of the rate (rounded up); a held option between the ends along the line a bit says; an option within T of its day counted only where it loses | faithful to 17.4. The reasoning holds with `near` as well (the lesser of a line and nothing still bends downward). 17.4's last paragraph still says "at its worse end in both sums": stale (N85) |
| `Party::asked`, `due` | the least over every choice of lines, and the premiums whole | faithful for a user's own statements. For a step the paper lets the stater choose, and the model takes the least: **the model is right and the paper is not** (N80) |
| `Shard::requirement` | every leg mirrored and pooled, every held option of the clearing side along its line of nothing; the same `near`; a rate on the gross of every unit, option or not | on the safe side of the paper, which counts rows of net units and lets the owner pick its lines at a draw: the model's owner draws less than the paper's may. Run again with the levels at exactly what can be lost (`exact`): the result stands |
| the rate on the gross of option rows | 2.25 % and 1.25 % of units by the price | as 17.6 says. Whether it is the right number: N84 |
| `leave` | every leg at one price; in a step the premiums of what is held and the larger of premium and loss of what was written; the band of the futures legs | faithful. Only whole accounts leave; no fee |
| `days` | a day falls between two visits and is of the price before the move; what the leg gained less its premium is paid into the account | the same standing as 17.5's "it stays as a gain that is no coin": a take is then held to the take's rule. Fair as a model of N66 |
| `take`, `draw` | all the rule gives | faithful. Only the owner's accounts take (in `run_x` users do too) |
| `call` | leg by leg: young and gained, nothing; losers pay first; one share | faithful to 17.9 as written, which is the flaw (N78). The run ends at the first call: nothing after a call is in the model (F114, late coins, claims) |
| `run` | the owner knows the next price; three plays and the fourth bit; it pays its shard up where a user enters, and never after an entry of its own | N83 |

## 2. Where the adversary was too weak, and what was added

`run_x` is `run` with switches, each off by default (the list is in the file above it):

| Switch | What it adds | Result |
|---|---|---|
| `top_own` | the owner pays its shard up after its own entries, to the upper or the lower level | runs of 44 to 55 ticks at 0.5 % where `run` has 2 to 4; nobody unpaid |
| `edge`, `edge_users` | every price a key states is at the tolerance's edge, rounded inward: entries of the owner's legs, takes, leavings, draws, a step by the account's own other key, the call | nobody unpaid to 4 %; N82 at 4.5 % |
| `set_off` | the owner enters against its users' net, draws on the book set off, and is out before the tick against it (review 43's N3) | nobody unpaid |
| `informed`, `users_take` | half the users know the next price; users take every tick | nobody unpaid; the owner loses to them, as it should |
| `farm`, `junior` | the owner pays nothing in for a user that will gain; a young gain paid after the other claims | N79 |
| `pair` | a user's long and short; a tick later the leg that lost leaves alone: as built, or under 17.4 | as built: users' accounts unpaid on 593, 463 and 126 of 1,000 paths at 0.5 %, 2 % and 4 %. Under 17.4: none (with N78) |
| `young_net` | an account's young legs counted together | N78 |
| `exact`, `witness` | the shard's levels and a step's test at exactly what a move of the rate loses | nobody unpaid: the claim does not rest on the lines' slack |
| `gross_futures` | no rate on the gross of option rows | N84 |

Of what the author listed as not in the model: a leaving of one leg changes the result (the `pair` rows above,
which is N65 in the simulation and the rule holding); the tolerance moves the claim's edge by itself and no
further (N82); an ended leg that stays, a split, a hand-over and a mark cannot change it by reasoning (each is
held to a test of what stays, and none moves a coin out past a take's rule); the tree is not about amounts; the
fallback is about a committee and has no numbers here. A step of some legs is not added: under "lacks less
after" it leaves an account that is stepped again at the same visit until it is not due, which is what a step
of every leg leaves.

## Findings

### N78 (High). The young rule, said of a leg, takes a hedge apart

17.9: "a leg that entered less than T before the call's time asks nothing where it gained, and pays what it
lost". `call` does that leg by leg, and so would a script.

*Scenario (in the file: `review_t2_a_hedge_that_is_young_at_a_call_pays_the_leg_that_lost`).* A user enters a
long and a short of 1,000 units at 100: asked nothing, as 17.4 says. A tick later the price is 101 and the
shard is called. The short lost 1,000 KAS: it pays. The long gained 1,000: it asks nothing. The account owes
1,000 and has nothing. Or: a long of 1,000 units and a put bought against it at the money, premium 2 a unit:
asked the premium, 2,000. The price falls 4 %, a call. The long lost 4,000 and pays; the put gained 4,000 and
asks nothing. The user who bought protection pays 4,000, of which it has 2,000.

In the simulation (a user's pair, one leg leaving under 17.4's rule): users' accounts unpaid on 15 of 1,000
paths at ticks of 0.5 %; with the young legs counted together, none at any tick to 4.5 %.

*Recommendation.* **Under a call the legs of one account that are younger than T are counted together: where
together they gained they ask nothing, where together they lost they pay.** They are not set against the
account's older legs (else a young leg of any size, at the edge, forgives what an old leg lost). In
`KpfTCStep` that is two sums where there is one. It does not give the attack of 17.9 anything: a young leg of
any size still asks nothing, alone or beside another.

### N79 (Medium). What the claim does not count: the gain of a leg that is young at a call

"No user unpaid" is of what the rule gives, and the rule gives a young leg nothing. In the model that is not
small, because its owner draws to the upper level at every tick and a call comes often (at ticks of 2 %, on
50 to 70 % of paths within sixty ticks; at 4 %, on 99 %).

| Ticks of | Young gains not paid, a path | The owner pays nothing in for a user that will gain | The same, a young gain paid after the other claims |
|---|---|---|---|
| 0.5 % | 1,276 | 5,790 | 1,646 |
| 2 % | 3,681 | 20,371 | 7,880 |
| 4 % | 35,343 | 36,385 | 16,184 |

(Model's units: a user's leg is up to 4,000,000 of them. Half the users know the next price.)

An owner can bring it about: it signs a user in and pays nothing in (no gate), the shard is called at the next
visit, the leg is young. In the model it does not pay the owner to: what it took a path falls from 271,365 to
below nothing, because the call ends its shard. It is the user's cost, not the owner's gain.

*Recommendation.* Keep "nothing" in the first form (with N78), and have the main test print the number beside
its claim. "Paid after the other claims, of what is left" gives users back more than half in the model and
gives the attack of 17.9 nothing; it is the same second class of claim as 17.3's deterrent, and can come with
it. With T at minutes and not at two ticks of 4 %, what is given up is a few minutes' move.

### N80 (High for a user near a strike). A step's stater picks the line that asks most

17.4: a held option between the ends "is counted along one of its two lines, as the row says ... either is
safe; the stater takes the one that asks less". A take's stater does. A step's stater is anyone, and a step is
right where the account has less than what the trace asks.

*In numbers (`review_t2_a_step_stated_with_the_lines_that_ask_most`).* Of 25,383 random accounts with a held
option between the ends, each holding half as much again as its least lines ask (so well above what a move of
the lower rate can lose), 15,152 are due for a step by the lines that ask most. That is N69's cliff again, by
whoever is paid to step.

*Recommendation.* The two sums and the lines are for what must hold at every price of the move: a user's
entry, take and leaving. **A step is due by one price that shows it**: the statement says a price within the
lower rate of the row's; the rows sum what the legs gain from the row's price to that one (an option within T
of its day only where it loses); the step is due where the coins, what all gained and that sum are below the
premiums. One sum where there are two, exact, and nothing to choose against the account: any price whoever
steps may name is a price the account could be at. The same for a call's test of a shard. In the model
(`witness`, `exact`): nobody unpaid, on every set.

### N81 (Medium; of the claim, and of the form that is built). A book one side of which leaves between two visits

A shard whose book is set off is asked the rate on the gross and nothing else. One side leaves; until someone
comes the other stands against what the shard held: twice the rate on the gross, at the level the shard stood
at. Under a call no band is taken.

*In numbers (`review_t2_a_book_that_one_side_leaves_is_held_by_twice_the_rate_on_the_gross`).* A long and a
short of 1,000 units at 100; the short leaves; the long gains a tick; a call.

| The shard stood at | It held | A tick of | The long is short |
|---|---|---|---|
| its upper level | 4,500 | 4.5 % | nothing |
| its upper level | 4,500 | 4.6 % | 100 |
| its lower level | 2,500 | 2.5 % | nothing |
| its lower level | 2,500 | 3 % | 500 of 3,000 |
| its lower level | 2,500 | 4 % | 1,500 of 4,000 |

The main test passes at 4 % because its owner draws to the upper level at every tick and nobody leaves a large
side; a book set off does not drift, so it stays there. With a user's large pair, one leg leaving, and every
price at the edge: users unpaid on 2 of 1,000 paths at 4 %. Review 43 set the rate on the gross so that twice
1.75 % and the band, 1 %, were the one bound of 4.5 %; at a call there is no band, and a shard may stand at
1.25 %.

*Recommendation.* Say the claim with its two bounds as they are: an account, and a book that is one way, to
the lower rate less the tolerance; a book one side of which leaves, to twice the rate on the gross at the
lower level less the tolerance. Which number the lower rate on the gross should be is tuning, for later; the
test above is the one to tune against.

### N82 (Low). The edge of the claim for an account: the lower rate less the tolerance

An account first fails above the lower rate: in most sets the first failure is at ticks of 5 % to 6 %, and
the author's own test shows the bound is tight (of 800,000 moves of exactly the
lower rate, 210,259 leave an account exactly its premiums). With a call stated at the edge, an account that
lost pays the tolerance more than the tick took: at ticks of 4.5 % one account in 1,000 paths did not pay (a
user's hedge by the clock, every price at the edge); at 4 %, none in any set.

So the edge is where the paper puts it, less the tolerance once more: 4 % with these terms, which is the
model's largest tick. Section 12's wording ("less than the maintenance rate less the tolerance") is that
4.5 %; with prices stated at the edge in a call or a step it is 4 %.

Near a strike and near a day I found no earlier edge: users that know the next price and enter at the money,
and hedges that end three ticks after they enter, are in the sets that pass.

### N83 (Medium). `run`'s owner ends its own run, and what 17.4 concluded from it

Each of the owner's plays enters an account of its own and pays nothing into the shard. The requirement jumps,
the shard is called at the next visit, the leg is young, the run ends. With the first play on, a path is 2
ticks; with the second and third, 3 to 4. No option's day is reached (the earliest is three ticks after an
entry). Of the 15,000 paths of the main test, 6,000 test the first call and nothing else.

17.4 says of the hedge that ends by the clock: "With the owner's own account the shard's own requirement jumps
at the day and the shard is called: its users are paid. The play is a user's against the clearing party". In
the model the shard is called at the entry, not at the day. With an owner that pays its shard up to its upper
level after the entry (`review_t2_the_owners_own_hedge_by_the_clock_needs_the_days_rule`):

| Ticks of | Without the day's rule: users unpaid | its own accounts unpaid | With it |
|---|---|---|---|
| 2 % | on 20 of 1,000 paths | 954 | none |
| 4 % | on 131 of 1,000 | 726 | none |

*Recommendation.* The rule is needed against the owner as well: rewrite 17.4's parenthesis. The shard's
requirement near a day stays as the model has it (an option of the clearing side within T of its day gives no
credit): that is what makes the call come before the day and not after it. And the main test should run with
the owner paying its shard up (`run_x`'s first switch), so that its paths reach the days.

### N84 (Low). The rate on the gross of option rows

Asked in N73. With no rate on the gross of option rows (`gross_futures`): nobody unpaid at ticks to 4 % on four
sets; at 4.5 %, users unpaid on 1 of 1,000 paths in the strongest set, where with the rate there is none. What
it changes far more is how soon a shard is called: at ticks of 2 %, a path of 10 ticks where it was 36, and
young gains not paid four times as much. So it is cover at the edge and distance from a call, and the number
2.25 % of the underlying is more than the model needs. Keep it as it is in the first form; it is tuning.

### N85 (Low). The third writing and the model: small things

- 17.4's last paragraph ("so it is counted at its worse end in both sums") is the second writing's rule.
- T is said twice and differently: 17.4, "twice the longest a statement lives and the time between two
  visits"; 17.9, "a lateness, two dispute periods, the time between two visits". The model has one. Say
  whether it is one term. If it can be computed from the lateness and the dispute period that the terms have,
  step 2 needs no new term (the terms are 90 bytes in every script that shows them).
- 17.4 and 17.8 say that N71 (a held option through a step of every leg) and the sums of a row are "found in
  the model": neither is in this model. The second belongs to `covered.rs`.
- The day's rule steps few accounts that the price did not: in a run of the plain set, 5,352 of 29,154 users'
  accounts were stepped at ticks of 0.5 % with the rule and 5,096 of 28,686 without (seen in a run that is not
  kept as a test).
- The model's head still says "second writing"; 17.8 says eight tests.

## 3. The claim itself

`short == 0` and `unpaid == 0 && unpaid_own == 0` are the right two halves, and the second is stronger than
section 12's (it includes every premium in a step). For options three things are beside it and should be said
with it: what a young leg gives up (N79, a number the test can print); what an option was worth beyond its
exercise at a call (N48; no number in a model without a price of options); and the two bounds (N81, N82). Its
edge is where the paper says for an account, less the tolerance; for a book one side of which leaves it is
lower than the paper says.

## 4. The third writing against N65 to N77

Taken as written: N65, N66, N67, N68, N70, N72, N73, N74, N76. N69 taken, with N80 and the stale paragraph.
N71 and N75 are left to a model that does not have them yet. Nothing in 17.4 or 17.5 is shown unnecessary: the
day's rule is needed against users and owner (N83); the account that stays is needed (the `pair` rows); a leg's
time is needed (the author's 443 of 1,000). What is shown wrong: the young rule of a leg (N78), the stater's
choice in a step (N80), and the parenthesis of 17.4 (N83).

## 5. Before step 2, and whether it can start

Step 2 is the leg as it will stay, futures only, with F116. It needs of the model only the young rule and T.

- **Decide N78 first** (an account's young legs together): it is what `KpfTCStep` will sum. And that a young
  gain is nothing, not a second claim (N79): that keeps the note as it is.
- Say T once (N85), computed from the terms' own periods if it can be.
- Where the leg's four numbers lie is "decided in the model", and this model has no bytes: decide it at step
  2's start.

**With those, step 2 can start now, beside the model's next round.** It touches futures only, and nothing
found this round changes what a futures leg says.

What the model should show before it is carried into `covered.rs` and `reserve.rs`: `call` with N78; the main
test's claim run with `run_x`'s switches (the owner paying its shard up, users that take, prices at the edge);
a step by a price that shows it (N80); one leg of a group of options leaving, as the `pair` does for futures
(a long stepped away from a written call; a held leg of a spread ending at its day); N71; and N81's test as
the stated bound.

## Tests added (`research_options.rs`, below the author's)

| Test | Shows |
|---|---|
| `review_t2_a_stronger_owner_and_stronger_users_leave_nobody_unpaid` | eight sets, 1,000 paths each at four ticks to 4.5 %: nobody unpaid |
| `review_t2_one_leg_of_a_pair_leaves_as_the_end_is_built` | N65 in the simulation: 593, 463, 126 of 1,000 paths |
| `review_t2_a_hedge_that_is_young_at_a_call_pays_the_leg_that_lost` | N78: the two accounts, and 15 of 1,000 paths |
| `review_t2_what_the_young_give_up` | N79's table |
| `review_t2_the_owners_own_hedge_by_the_clock_needs_the_days_rule` | N83's table |
| `review_t2_the_edge_of_the_claim_and_the_gross_of_option_rows` | N82, N84 |
| `review_t2_a_book_that_one_side_leaves_is_held_by_twice_the_rate_on_the_gross` | N81's table |
| `review_t2_a_step_stated_with_the_lines_that_ask_most` | N80: 15,152 of 25,383 |
