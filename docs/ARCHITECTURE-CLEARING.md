# Architecture: a clearing network of two-party accounts

Written 25 November 2026, from `GOAL.md` and the research of `RESEARCH-UNIFIED-MARGIN.md` (R1 to R4). It is the
whole form in one place: what the objects are, which small scripts move them, what holds, and how far each part is
proven. It is a design to build against; the scripts of today (`DESIGN-COMMITTEE.md` 8a) are its first third.

Each statement carries how far it is known:

| Mark | Means |
|---|---|
| **E** | run at the engine (a script, in the repository) |
| **M** | held by a model with property tests |
| **D** | designed and reasoned, not yet modelled |
| **T** | needs testnet to be known |

## 1. What a user gets

The owner's measure: margin behaves as one and moves as one, as a user sees it.

1. One account, one margin, for everything it holds. Margin is asked for the net of its legs.
2. A gain is usable without closing: it is margin for another leg, or it is taken out.
3. It enters and leaves at once, whoever is on the other side.
4. If the party on the other side fails, it is paid from that party's reserve.
5. It signs when it places an order or takes something out, and never to let someone else in or out.

There is no operator and no object that every user touches. Whoever stands in the middle is a clearing party, and
anyone can be one.

## 2. The parts

```text
            committee (staked members)                       miners (later)
                   | attest, once a market and time                |
                   v                                               v
              price note  <------ cited, never spent ------  block payload
                   ^
                   | citation (a proof against the chain block's commitment)
   +---------------+----------------------------------------------------+
   |                                                                    |
 user U ---- account (U, C): legs, one margin a side ---- clearing party C ---- reserve of C
 user V ---- account (V, C) ----------------------------------- C               (shards: KAS held,
 user W ---- account (W, D) ---- clearing party D ---- account (C, D) ---- C     net by market)
```

| Object | Between | Holds | State |
|---|---|---|---|
| Account | a user and a clearing party (or two clearing parties, or two users) | both margins | the legs (market, size, direction, entry), the two margins, what each asked of the other, the reserve shard it is covered by |
| Order | its owner | the owner's margin and a tip | direction, size, limit, least fill, what it asks of its counterparty |
| Price note | nobody | the least an output holds | market, time, price; written under an id only a checked attest writes under |
| Reserve shard | a clearing party | its reserve | by market: its net and gross position over the accounts the shard covers |
| Committee, epoch table, fee pool | as today | | |

An account is today's position with more legs and a reserve named. Two users may still hold one directly; nothing
of today is removed.

## 3. Small scripts that rely on one another

The engine holds 244 stack items and a script is carried whole by every transaction that uses it. So nothing is one
large script. An operation is a few small ones spent in one transaction; one **serves** a result and the others
**read** it (R1, R3; the core's own proposal for this is `RESEARCH-UNIFIED-MARGIN.md` 11: a result is proposed, then
verified, and the record lies where all can read it).

| Service | Serves | Proof it rests on | Size | Known |
|---|---|---|---|---|
| Note writer (`KpfTNote`, 726 B; the note, `KpfNote`, 122 B) | a note: committee, market, time, price, spread | the committee's epoch table spent by its attest beside it | 5.8 kB with three members, once a market and time; what the note holds returns to its writer | E |
| Citation (`KpfTCite`, 3,391 B) | "this note was accepted by a chain block within the finality depth" | the block's sequencing commitment, recomputed | alone 5.0 kB and 21,766 script units; the note is not spent, and may be taken back at once | E; the opcode is live on testnet-10 and the proof's form agrees with a real block (T, done); the script itself not yet spent there |
| Valuation, by size (2, 4, 8, 16 legs) | A's gain over the legs and the maintenance, at given prices | arithmetic over the legs it is shown, held to their hash | 0.7, 1.3, 2.4, 4.6 kB; stack 23 for any number of legs | E |

Rules of serving, for every script of the library (the first is a hole to close in what exists):

1. A reader holds the service to a script and an id it knows, **and to the entry that ran**: a script with a second
   entry must not be readable as having served by it.
2. A service verifies everything it is shown; a reader checks that what it reads is about its own object (the legs'
   hash, the market, the time).
3. A record is read in one place a script, so that the place can move from the service's signature script to the
   payload when the convention exists; one record then serves many readers in a transaction.

## 4. The operations

Each row is one small script of the library, with the services it is spent beside. "Signs" is who must sign.

| Operation | What it does | Beside | Signs | Known |
|---|---|---|---|---|
| Place, cancel, expire | an order enters the registry, or returns | | the owner, to place | E (today) |
| Match | two orders become an account | | nobody | E (today) |
| Replace | two orders matched into an account: one party leaves, the other order's owner takes its place | | nobody | E (today's `KpfTReplace`) |
| Merge, fold, join, turn | accounts of the same two parties rearranged; legs joined into one account | | as today | E (today) |
| **Mark** | the entries move to the cited prices; the gain since becomes margin; nobody's worth changes | citation, valuation | nobody | M (R2); script D |
| **Take** and **collect** | with a mark, a party states what it takes out above its floor; collected once the mark is seen to have landed within the note's time | citation, valuation | the taker | M (R2); script D |
| **Move** | a take collected into another account of the same party as an addition, in one transaction | | the party | D |
| Close, dispute, answer | as today; the answer reads the valuation and the citation where it now carries both | citation, valuation | as today | E (today), smaller: D |
| **Liquidate in steps** | a side below its maintenance at a cited price hands one leg, or a part, to a liquidator at a discount, until it is above | citation, valuation | the liquidator | D |
| Exit | as today: out at the entries when the committee is gone | | | E (today) |
| **Cover, uncover** | an account is put under a reserve shard, or taken from it: the shard's net and gross change. A cover asks what a mark asks: both sides at their maintenance at a cited price | citation, valuation | the clearing party, to cover | D; why the citation: M (R4) |
| **Sweep to reserve** | at a mark, what the clearing side has above its floor goes to its shard | citation, valuation | nobody | D; why it must exist: M (R4) |
| **Claim from reserve** | at a settlement, the part of a user's worth the account's pot cannot pay comes from the shard | | nobody | D; arithmetic M (R4) |
| **Call the reserve** | a shard that holds less than its requirement at cited prices is in default: its accounts may be closed against it | citation | nobody | D |
| Fund, draw | the clearing party adds to a shard, or draws down to its requirement at cited prices | citation, to draw | the clearing party | D |
| Sweep fees, pay members | as today | | | E (today) |

### 4a. The default path: a change is stated, and a citation only answers (25 November 2026)

The table above put a citation beside every mark, take, liquidation and cover. That is not the default any more,
for two rules the owner set before: the committee signs when asked (a dispute), not for every operation, and a
transaction of a kilobyte or two is the measure. A citation is 3.4 kB of script and 0.5 to 1.3 kB of proof on every
use, and it asks the committee for a signature each time. So:

| | Stated (the default) | Cited at once (whoever is in a hurry) |
|---|---|---|
| What is sent | the change: what is stated and what the position becomes, shown whole, with a bond; nothing is computed | the statement, then its answer with the committee's price in a citation |
| Final after | the position's dispute period | the answer's transaction |
| Size | 2.1 kB stated, 2.2 kB applied | 2.1 kB and 10.1 kB (one leg), 14.7 kB (two) |
| The committee signs | only if it is disputed | once a market and time, for everyone |
| Who must watch | an executor, as for a close today (`exec-run` does: it disputes, applies, ends) | nobody |

Both are the same scripts (E, `crates/kpf-protocol/tests/thin_change.rs`): `KpfTChange` (938 B) states,
`KpfTChangeSettle` (1,080 B) applies, `KpfTChangeDispute` (1,277 B) disputes or lets a dispute lapse,
`KpfTChangeLate` (680 B) ends a statement that did not land near its time, and the kind's answer decides by the
committee's prices, disputed or not: `KpfTTake` (3,898 B) for a take, beside `KpfTCite`. The generic four are shown
the position as two hashes and its moving part: they do not know how many legs it has.

**A statement shows what it states.** The script takes what is stated and what the position becomes as they are,
holds the first to its kind's length, and hashes both itself. Whoever watches reads a statement on the chain and
judges it by a price of its own; given hashes alone (as the script first was, and as live run 17 ran it), a watcher
could only dispute blind, which costs it its bond or makes the committee sign for every statement. The answers are
shown the bytes that were stated and read their numbers there, so that whatever was stated can be opened.

A take (kind 1) is the mark and the take of the table in one: the entries move to the prices stated, the gain since
is the taker's margin, and it takes out what it then has above its floor. It is within the rule when the claimer is
the side's key, each price stated is within the position's tolerance of the committee's (the entries are what an
exit settles at: they are not moved from the truth), no margin falls below zero, and the side keeps its maintenance
at the committee's prices and what the other party asked of it. On 31 random takes the script and the model gave
the same verdict, and the other verdict was refused.

What the default costs, and is not yet measured: a take, a liquidation and a call wait the dispute period, so the
cover of section 6.1 is the move over that period and not between two marks; and an unanswered dispute returns both
bonds (the limit accepted for a close) for every kind now.

### 4b. An account of any number of legs, answered a point at a time (25 November 2026)

An account is a position whose fixed part is the hash of its ten terms and of the root of a chain of legs
(`g_0` = 32 zero bytes, `g_i = blake2b(g_(i-1) + leg_i)`), in place of two legs. Every script that is shown a
position's fixed part as its hash serves it as it is: `KpfTOpen`, `KpfTAdd`, `KpfTBoth`, and the four of 4a.

Nobody computes over all the legs on the chain. A take on an account (kind 2) is stated with a **trace**, a row of
112 bytes a leg: the price stated; over the legs up to that leg, A's gain, the worth each side is asked the
maintenance of, and what each side keeps whole beside it (the last two differ from one side to the other only by
options: below); the chain of the legs up to it, and the same chain with each entry at its price. A dispute names **one point** of it, and the answer is of that point:

| Point | Says | Script | Shown |
|---|---|---|---|
| 2i - 1 | row i follows from row i - 1 and the leg | `KpfTStepRow` (2,560 B) | one leg |
| 2i | the price of row i is within the tolerance of the committee's | `KpfTStepPrice` (1,545 B) | one leg, one citation |
| 0 | the last row is the account's, and the statement keeps its kind's rule | `KpfTStepEnd` (2,078 B), `KpfTStepLiq` (2,561 B) | the terms and the last row |

A point answered right leaves the statement standing (its dispute period begins again) and costs the disputer its
bond; one answered wrong restores the account and costs the claimer its bond. A statement is the rule's exactly
when no point of it is wrong: on 40 random accounts of 1 to 16 legs, every statement with one thing moved had a
wrong point, the script of that point took the model's verdict, and refused the other (E,
`tests/thin_account.rs`).

| Legs | Stated | Applied | Row answered | Price answered | End answered |
|---|---|---|---|---|---|
| 2 | 2,950 B | 2,567 B | 4,087 B | 7,951 B | 3,697 B |
| 5 | 3,286 B | 2,567 B | 4,423 B | 8,287 B | 4,033 B |
| 16 | 4,518 B | 2,567 B | 5,657 B | 9,521 B | 5,265 B |

(Measured 25 November 2026, with the row of 112 bytes and the scripts as they are after rounds 30 to 32.)
A settlement does not grow with the legs; a statement and an answer grow by the 112 bytes of a row (the trace is
shown whole, when it is stated so that whoever watches has it, and to the answer; a tree in its place would end the
second). The same position of two legs costs 14.7 kB to answer whole
(4a) and at most 8.0 kB by a point.

**A step of a liquidation** (kind 3) is stated the same way by anyone: a side stands below its maintenance at the
prices stated (counted at the maintenance rate less the tolerance, so that it is below at any price within it),
and one leg leaves the account for the liquidation fee of that leg's worth. The legs that stay stand at the prices
stated. Its rows and prices are answered by the scripts of a take, its end by `KpfTStepLiq`; whoever liquidates
states the next step while the side is still below (E: a side of three legs is brought above its maintenance by one
step, and no second is stated).

**What nobody could answer could lock an object for nothing**, since a lapse returns both bonds. Five such
statements were found while this was built (and a sixth with the next change), and each now has an answer or cannot
be stated (E):

| Statement | Now |
|---|---|
| kind 2 or 3 on a position of two legs | refused when stated: the statement shows the account's terms and root |
| kind 1 on an account | ended by `KpfTKind` (813 B), which shows the account: the claimer's bond is lost |
| kind 2 or 3 disputed whole | refused: it is disputed by a point |
| a point beyond the trace | answered right by `KpfTStepRow`, without a leg: the disputer's bond is lost |
| a trace of no row, or a take that leaves a leg out | its end is answered wrong |
| what is stated in a form its kind's answer could not open | refused when stated: its length is its kind's, and the answers read the bytes as they were shown |

**Review round 30** (`docs/audit/REVIEW-T1b.md`) found five holes in these scripts, three of them high, and each
is closed in the scripts (the review's verification is round 31):

| Finding | What it was | Now |
|---|---|---|
| F51 | an answer counted from what the object holds now; a disputer that wrote it with a sompi more made a true take "not within" and took half the claimer's bond | the statement's hash carries what the object held when it was stated, and every answer counts from that |
| F52 | rows whose chains are of no leg could be answered by nobody; the end was right by its own last row, so the statement could only lapse, or be applied | a row is opened by a chain and a leg that hash to its own chain, and is wrong where that chain is not the row before's: the row after the last chain of no leg is opened by the account's own chain and is wrong. And a wrong answer of any point ends a point-disputed statement, so a dispute of a point nobody can answer keeps nothing |
| F54 | the settlement took any first flag but 0 for a payment; the reader knew 0 and 1 only, so a statement of flag 2 was applied and seen by nobody | the flags are held when the statement is made, and by the settlement |
| F53 | a point answered right put the statement back to be held to its time again; answered late, a true statement could only be ended as late | a statement that stands again is not held to its time again (it was, when it was disputed) |
| F55 | a library of one copy of each script could not answer a take on two legs, which takes two citing copies | such a library is written with a second citing copy, and is no library without |

**An option is a leg** (25 November 2026; E, `tests/thin_option.rs`, eight tests; reviewed in rounds 33 to 35, below). The direction's
place of a leg says what it is: 1, A is long; a number from 3 on whose last three bits are 3 to 6, an option on the
leg's market whose strike stands above those bits (3 a call A holds, 4 a call A wrote, 5 a put A holds, 6 a put A
wrote); anything else, A is short. An option's entry is the price it was last counted at. Its row adds:

| To | What | So that |
|---|---|---|
| A's gain | what the option is worth if exercised at the price stated, less what it was worth at its entry (the other way where A wrote it) | a take moves an option's gain between the margins and its entry to the price, once, as a futures leg's. Nothing is taken twice (3,000 random accounts), and a loss on one leg is paid by a gain on another in the same sum |
| the worth of whoever wrote it | its underlying's worth at the price stated | the writer is asked the maintenance of the underlying: an option moves by no more than its underlying |
| what its holder keeps whole | what it is worth if exercised at the price stated | the holder is asked no maintenance for it, and does not take out what it was counted at before it exercises: it cannot lose by the option more than that. A holder that keeps nothing else in the account is below no maintenance at any price, and nobody liquidates it |

A futures leg adds its worth to both sides' worth and nothing to what they keep, so an account of futures legs is
counted as it was. A side's floor is the maintenance of its worth and what it keeps whole; a side that stands at
it is not below zero after every price moved by less than the rate (4,000 random accounts with options of every
kind).

| What is an option's own | Rule | Why |
|---|---|---|
| Who makes it leave | its holder's side alone: by the holder's statement (kind 4: the option was counted at what it is worth exercised, so its leaving is the exercise; from then the holder takes that out), or by a step of the liquidation of the holder (kind 3). The row of an option that leaves by the other side is wrong | whoever wrote an option does not end it for what it is worth exercised: far from its strike that is nothing, and the option is worth something to its holder. A writer below its maintenance loses its futures legs and the options it holds to a liquidation; what it wrote stays until its holder exercises |
| What a shard counts | a row of its own. A shard's row is named by the hash of a kind and a market: `blake2b(push8(13) + push(market))` for a market's options, `push8(14)` for its futures legs (section 6a). An option counts with its size whichever way it stands | options are not set against one another, nor against futures legs: a call written and a futures leg bought do not cancel where the price falls. And no leg of one kind is counted in the row of the other, whatever its market is called (review 34, F59) |

Where a leg leaves, what a side keeps whole is counted less what of it the pot could not pay (what A is owed
beyond the pot; what A's margin lacks for B): a side is not below its maintenance for what it was not paid (review
34, F57). And a leg that enters a covered account is held to the bound a position's leg has (its worth at its
market's limit is computed without overflow: review 33, F56).

**Reviews 33 to 35** (`docs/audit/REVIEW-T1b.md`; all at the engine) of the options, the row of 112 bytes, the
handing over of a side and the move of a leg:

| Finding | What it was | Now |
|---|---|---|
| F56 (medium) | a leg whose worth is beyond 64 bits entered a covered account: nobody could state on it or answer, and the margins stood locked | `KpfTLegAdd` holds a leg to the position's bound (and its scale to the limit); verified closed |
| F59 (medium) | a futures leg whose market was called as the options' row is, was counted in that row and set against the options: the owner drew more than the rule gave | a row is named by the hash of a kind and a market, for futures legs too; `KpfTShardPrice` is shown the market and cites it; verified closed |
| F57 (medium) | a holder that kept nothing but its option was liquidated, for a fee, once the option was worth more than the pot | what a side keeps whole is counted less what the pot could not pay it; verified closed |
| F58 (low) | seventeen of the largest legs sum beyond 64 bits: the last row is answered by neither verdict, at the committee's own prices (my claim that a wrong point stays answerable did not hold) | the tool states on no such account and adds or moves no leg into one. Open in the scripts, which have no count of legs: two parties who sign such legs past the tool (9.8 x 10^10 KAS of them) have an account nobody states on truly |

What held under attack: `KpfTStepRow` against the model on 1,755 answers (every kind and every direction's place
that is no kind, each of the six numbers off by one, leaving by each side in each kind); `KpfTStepEnd` and
`KpfTStepLiq` on 1,262; a holder alone liquidated at none of 19,680 random prices and legs; `KpfTHand` (9
refusals) and `KpfTLegMove` (23). By the rule and not findings: a hand-over to the same key (a margin added), the
payment of a hand-over (held by the two signatures of the whole), margin that leaves in a move of a leg (every
party of both accounts signs). Not reached: the commands, anything at a node, notes applied in random order over
four rows, several disputes of one draw in one window.

What this costs: 24 bytes a leg in every statement and answer (a row of 112 bytes for 88), and capital for whoever
writes: a written option is asked the margin of a futures leg of its size, far from its strike too. Two forms were
tried before this one and dropped (both in numbers or at the engine, the same day):

| Form | Why it was dropped |
|---|---|
| The strike where the entry stands, and what the option is worth in a number of its own that a take does not move | a futures loss beyond A's margin was forgiven while the option's worth still stood against the other party (`tests/research_option_spot.rs`, the first test: A up 25 on legs that made it 10) |
| The option counted in the gain (as now), with one worth for both sides | whoever held an option was asked the maintenance of its underlying, and lost the option to a liquidation where it kept less, though it owed nothing |

Not built: a last day (the price of a market that stands still from its last day gives one, with no script; a
written option that is worth nothing then leaves only with its holder); a price for buying an option back (it is
margin moved by both signatures). And a holder that stands below its own maintenance by its other legs does not
exercise by its own statement while the other party stands at its own (the rule of any leg that leaves): a step of
its liquidation exercises it.

Not built for an account: a close and an exit's refutation (an exit claim on an account can be ended by nobody
today, so an account's parties must not rely on the exit while a committee lives), the match and the replacement
into one, and an answer at once (it would be every point).

## 5. How a user's day goes

- **In.** It places an order. A clearing party's order, or another user's, is matched with it; if the account
  exists the match adds to it (merge) or replaces a party (replace). One transaction, one account.
- **More.** Another market is another leg of the same account (join). Margin is asked for the whole: a gain on one
  leg is worth as much as margin in the valuation at any time, without any operation.
- **Take a gain.** It marks its account at a cited price and states a take; one round later it collects, into its
  key or into another account (move).
- **Out.** It places the order the other way and is paid when it is matched. Where no order answers, a clearing
  party does (`take` of `exec-run` today). The claim at its own price stays as the door nobody can close.
- **The clearing party fails.** The user's watchtower closes the account at a cited price. What the pot lacks comes
  from the shard; if the shard is short, the user's own watchtower first sweeps into it the accounts in which the
  clearing party gained.

## 6. The clearing party's capital

It is the counterparty of its users, and it posts margin like anyone. What makes it cheap is two things, and only
the second needs anything new.

1. **Re-marks make the cover small.** Marked often, an account can cost its counterparty only the move since the
   last mark. A user asks that much of its clearing party, not a position's margin. (M: a mark changes no worth.)
2. **The reserve is on the net.** The clearing party's accounts are covered by a shard that holds a rate on its net
   position and a much smaller one on the gross. Its gains in some accounts pay its losses in others, because
   anyone may sweep them to the shard. (M, numbers only: with 0.5 % posted in each account and 5 % of the net
   reserved, a balanced book of 200 accounts needs 514 KAS where two-party cover needs 2,583, and every user is
   paid in full for moves up to 5 % when gains are swept first. A book all one way needs as much as two parties.)

What the reserve is as objects (M, `tests/research_reserve_objects.rs`: a shard with a state, accounts that name
their shard, every operation one transaction on the objects it spends):

- **Nothing touches the shard and an account at once.** A shard is one output: every account that had to spend it
  would wait for the others. A closing account leaves the shard notes (what it releases of the net, what the
  clearing side had left, what its user is still owed), and anyone applies them one after another. Over 200 random
  orders of 60 transactions nothing was made or lost.
- **A covered account pays the clearing party nothing directly.** What its side has left goes to the shard, and it
  draws from the shard what is above the requirement. Paid directly, its gains in accounts of its own left while
  its losses were noted on a shard that never saw them (a dress at 5 % of margin left users 36 KAS short; routed
  to the shard, none).
- **A draw is a statement**, refuted by any note of the shard that was not applied when it was stated. Drawn at
  once on a net that was stale (ten accounts had left, their notes not applied), 30 KAS left the shard and the users
  were 36 KAS short after a move of 5 %; stated, nothing was drawn.
- **A book is covered along a path.** The shard holds its requirement after every cover, so ten longs and ten
  shorts covered the longs first need 5 % of 1,000 on the way and covered in turn 5 % of 100.
- With gains closed first every user was paid in full for a move within the rate (200 books); in the order of the
  accounts 21 of them left somebody short. A move of 12 % on a book all one way paid 126 of 176 KAS.

What holds the reserve to its word (D):

- **The shard carries the net.** Covering an account changes the shard's net and gross by the account's legs, in
  units, by market. The cover is refused unless the shard then holds its requirement at the account's own prices.
- **Anyone calls it.** With cited prices, anyone shows that a shard holds less than its requirement. Then it is in
  default: its accounts are closed against it at those prices, and it covers no new one.
- **A user's side is always collectable.** A user below its maintenance at a cited price is liquidated by anyone,
  and its account leaves the net. So a clearing party cannot dress its net with accounts of its own that hold no
  margin: they are liquidated at the first mark and the net shows. (M: dressed, a book of 1,000 units asked 100 KAS
  of reserve where 550 were due and left its users 349 KAS short after a move of 5 %; marked before the move, the
  dress was liquidated, the shard called, and every user paid.) A cover is therefore made beside a citation, and
  the maintenance rate of a user side is at least the reserve's rate on the net.
- **What a user can lose** is what a move beyond the rate takes between two marks, less what the shard still holds
  when it claims. It is bounded by its own watchtower's speed, and it is the tail a clearing house's members share.

A clearing party lays off what it took by a replacement (to a user that comes the other way) or by an account with
another clearing party, itself under the other's reserve. Nothing else joins clearing parties: there is no object
between them but accounts.

### 6a. The reserve as scripts: the design (25 November 2026, before any script)

Written whole before anything is compiled, and walked through for each party that reads it. Nothing of this runs
yet (D).

**The objects.**

| Object | State | Who makes and changes it |
|---|---|---|
| Shard (its output holds 1 KAS beside what it holds) | an account's form with 171 bytes of moving part: the owner's key (A), no second key, what it holds (A's margin), its dispute period, bond rate and lateness, and at the end its own id `sid`; the fixed part is the hash of its terms (the rate on the net, the rate on the gross, the tolerance) and of four market rows (market, committee, scale, price limit, net units, gross units) | `KpfTShardOpen` from an anchor (`sid` is the hash of the outpoint it spends: no two shards have one); `KpfTAddC` (the owner funds); `KpfTNoteApply`; a stated draw |
| Covered account | an account (4b) with 171 bytes of moving part: the same 138, then the `sid` of the shard that covers it. Side B is the clearing party. Every leg of it is covered | `KpfTOpenC` (both sign, no leg yet); `KpfTLegAdd` (both sign, one leg appended: the root of a chain takes one hash); statements; `KpfTAcctEnd` |
| Note | to a shard: its `sid`, a kind, a market's four values, units for the net and the gross, a key, an amount | only the scripts below, each beside the change of an account that it is of; spent by `KpfTNoteApply` |

Why the moving part is longer: `KpfTOpen` and `KpfTBoth` take a position's two parts and both parties' signatures and
ask nothing else, and they are typed for 138 bytes. An account of that form could be born with covered legs, or
ended, without a note; the shard's net would be whatever two keys of one clearing party wished. An object of 171
bytes is opened and ended by the scripts of this section alone, each of which writes the note of what it did.

**The notes, and what makes each.**

| Note | Made by | Applied (`KpfTNoteApply`, anyone, one a transaction) |
|---|---|---|
| Join: a leg entered | `KpfTLegAdd` | the row of the leg's kind and market (named by the hash of the two; an option counts there with its size whichever way it stands): net and gross change by the leg's units (the row is taken if it is empty). Only if the note's key, the account's B, is the shard's owner: nobody adds to another's net |
| Release: a leg left | the settlement of a step of a liquidation or of a leg that leaves (kinds 3 and 4) | the reverse, under the same condition |
| Deposit: what the clearing side had | a take stated for side B of a covered account (by anyone: it pays the shard, not the claimer); `KpfTAcctEnd` | the shard holds the amount more |
| Owed: what the pot could not pay the user | the settlement of kind 3 or 4, where the entries standing at the prices stated leave the user more than the pot | the user's key is paid what the shard holds of it, and the note goes on with the rest |

A covered account pays its side B nothing by any path: a take for B is a deposit, the end of an account with no leg
is a deposit, and what the clearing party has is drawn from the shard.

**The draw (kind 5).** The owner states an amount and a price a row. Within the rule: the claimer is the owner, and
what the shard then holds is at least the rate on the net of the worth of each row's net and the rate on the gross
of the worth of its gross, each counted with the tolerance more (the prices are only near the committee's). Points:
0, the end (`KpfTShardEnd`: four rows are computed in the script, there is no trace to follow); 1 to 4, a row's
price against the committee's in a citation (`KpfTShardPrice`, which also answers a point beyond the fourth right;
a row is named by the hash of its kind and its market, so the answer shows the market whose name hashes to the
row's, and the citation is of that market: the options of a market are priced as its futures legs are).
And one more way to end it: `KpfTDrawNote`, anyone shows a note to this shard that landed before the statement and
is not applied (the note goes on as it is): the shard again, the claimer's bond lost. A note counts for this only
where it is of an account whose side B is the owner, and a deposit does not (it only adds). So an owner knows of
every such note before it lands: a leg enters by its own signature, and a leg's leaving and what is owed are
written by a statement that waited its dispute period. An owner settles what is ready, applies the notes, and
draws.

**No call.** The model had one; here it would only set a flag. A shard's rows and what it holds are public, a draw
is the one transaction that takes value out, and a user's leg leaves by its own statement at any time (kind 4). A
shard that holds less than its requirement is seen by every tool and covers nobody who looks. So the requirement
binds the clearing party where it can act (the draw), and nowhere else is a script asked to compute it.

**What changes in the scripts of 4a and 4b** (all to be reviewed again):

| Script | Change |
|---|---|
| `KpfTChange` | the moving part is 138 or 171 bytes; kind 1 on 138 only; kinds 4 and 5; kind 5 states 40 bytes |
| `KpfTChangeSettle` | an output may be a note (flag 2: an object under the same id), counted |
| `KpfTPoint` | kinds 2 to 5 |
| `KpfTStepRow` | what is stated begins with the leaving leg's market and units too, held to the leg at its row |
| `KpfTStepPrice` | the moving part is 138 or 171 bytes |
| `KpfTStepEnd` | on a covered account a take for side B is by anyone and its amount is a deposit |
| `KpfTStepLiq` | kinds 3 and 4 (a leg leaves by its party's own statement: no fee, and the party stands at its maintenance or the other below its own); on a covered account the release, and the owed where the pot is short |

New: `KpfTShardOpen`, `KpfTOpenC`, `KpfTLegAdd`, `KpfTAddC`, `KpfTNoteApply`, `KpfTAcctEnd`, `KpfTShardEnd`,
`KpfTShardPrice`, `KpfTDrawNote`.

**Read by each party.**

| Who | Reads | Where |
|---|---|---|
| A user before it relies on a cover | the shard's rows and what it holds; that its own leg's join was applied | the shard's last transaction; `KpfTNoteApply`'s arguments |
| A user's watchtower | every statement on its account (shown whole); notes to its shard not yet applied | the chain, as `exec-run` reads a take today |
| An executor | notes to apply (deposits before what is owed); draws to dispute by a price of its own or by an older note | the registry's objects |
| The clearing party | its shard above the requirement at its own prices, to draw | the same |
| Whoever answers | one row or one leg, and one citation | as in 4b |

**A statement nobody disputed reaches the shard.** A settlement writes the notes its statement stated. A false
statement on one covered account that nobody disputes within the dispute period can therefore state a note of what
is owed, and be paid by the shard: by every user's cover, not by that account's pot alone. A shard is as safe as
the watching of every account it covers. Its owner is a party of each of them and loses first, so it watches; where
it is gone, the users' watchtowers and the executors do, as they watch statements today.

**What this does not do.** One shard is four markets; a book of more is several shards and nets within each only.
A join is not held to the shard's requirement (the user's tool looks before it enters). Whoever applies a note
chooses the order: deposits before what is owed is every claimant's interest, and not a rule. A leg is not moved
between accounts, and a covered account is not matched into: those are GOAL's "any counterparty" for accounts, and
still to come.

Order of building: the scripts of the account's side and the notes (no draw: a shard that can be funded and pay,
and nothing else); then the draw. No testnet until both are reviewed.

**The first part is built** (E, `crates/kpf-protocol/tests/thin_reserve.rs`, 25 November 2026; reviewed in round 32): six
scripts, and the seven of 4a and 4b changed as the table above says.

| Script | Bytes | A transaction of it |
|---|---|---|
| `KpfTShardOpen` | 620 | a shard opened: 1,850 |
| `KpfTOpenC` | 602 | a covered account opened: 1,892 |
| `KpfTLegAdd` | 1,059 | a leg added, with its note: 2,497 (a futures leg or an option) |
| `KpfTAddC` | 535 | a margin added, a shard funded: 1,620 |
| `KpfTNoteApply` | 1,251 | a note applied: 3,130; what is owed paid: 3,233 |
| `KpfTAcctEnd` | 678 | an account with no leg ended: 1,781 |

What ran: a user goes long ten units against a clearing party under its shard, the price rises 30 %, the user's leg
leaves by its own statement (2,767 bytes stated, 2,730 applied): the pot pays 2 KAS of its gain, the settlement
writes the note that the leg left and a note of 28 KAS owed, and the shard pays them to the user's key. The
clearing side's gain in an account is swept to the shard by anyone's statement. A statement that forges a note (40
KAS owed; no release; a release of more units, or to another shard, or of another market) has a wrong point, which
the script of that point answers wrong.

Two things the engine showed that the design had not:

- A shard that pays out everything it holds would leave an output of nothing, which no block takes: a shard's
  output holds 1 KAS beside what the shard holds, and no note pays that out.
- A note is an object of 0.2 KAS and of its own transaction to apply (3.1 kB): a leg that enters and leaves costs
  its shard two of them. What a leg costs a clearing party is these, not the reserve alone.

**The second part, the draw, is built** (E, `crates/kpf-protocol/tests/thin_draw.rs`, 25 November 2026; not
reviewed): three scripts, and `KpfTChange` and `KpfTPoint` take kind 5.

| Script | Bytes | A transaction of it |
|---|---|---|
| `KpfTChange` (kind 5) | 1,317 | a draw stated: 2,681; applied by `KpfTChangeSettle`: 2,567 |
| `KpfTShardEnd` | 2,604 | the end answered: 4,384 |
| `KpfTShardPrice` | 1,655 | a row's price answered with a citation: 8,056 to 8,108; a row of no market: 3,416 |
| `KpfTDrawNote` | 1,134 | a draw ended by a note: 2,912 |

What ran: a shard of 50 KAS, 60 units short over 140 of legs in one market and 10 long over 30 in another, at a
price of 10: it is asked 43.7175 KAS, its owner draws 6.2825 and not a sompi more. A draw of more, by another key,
with rows of its own making in the shard that goes on, or with no price for a market: the end is answered wrong.
A price of 8 where the committee says 10 (the requirement looks smaller): the row's price is answered wrong with
one citation. A leg's note or a note of what is owed that is not applied ends a draw; a stranger's, a deposit, a
note to another shard, or one as young as the statement does not.

**Three holes found while this was built, in what was there before** (all closed, each with a test):

| Hole | What it gave | Closed by |
|---|---|---|
| A note of what is owed was paid by the shard it named, whoever wrote it. An account anybody opens names any shard. | Two strangers open an account that names a shard and one is owed by it: the shard is drained. Critical. | The note names who owes it (the account's side B, where a leg's market would stand); `KpfTNoteApply` pays only where that is the shard's owner. |
| A trace could begin with a row of its claimer's making whose chain is the seed of every chain (32 zero bytes): its end was right, every row of the account followed from the one before, and the made row could be opened by nobody (review 30's F52 again, by another door). | A gain of the claimer's choosing in a statement nobody could answer: a dispute lapsed and returned both bonds. High. | A row that carries the seed is answered wrong with no leg shown (`KpfTStepRow`). |
| A take, a liquidation step or a leg's leaving could be stated on a shard (an object of an account's form). | An end computed from rows of the claimer's own, none of which anybody could open. High. | `KpfTChange` states only a draw on an object of 171 bytes with no second key, and a draw on nothing else. |

**Reviewed** (round 32, `docs/audit/REVIEW-T1b.md`, 25 November 2026): no new finding; the three closures hold
between model and script. What the review did not reach: the order of notes over four rows in full, a random
comparison of the reserve's arithmetic between model, builder and script as the takes had, two rows of one market
in a shard, edges of `KpfTAcctEnd` and `KpfTAddC`, and anything at a node.

Its one open question, answered after it (E, `thin_draw.rs`): the engine's integers are 64 bits and checked, so a
shard whose rows at the stated prices are worth more than that is computed by no script. `KpfTShardEnd` then
accepts neither verdict, a dispute of the end lapses, and the draw ends with both bonds returned: nothing is drawn
that was disputed, and nothing is over-drawn. Rows are that large only by legs the shard's owner signed for. The
model follows the script step by step (`Shard::draw_end_judged`) and the tools state no draw there.

Found when the commands ran the whole story (6b): a row of a shard kept its market after every leg of it had
left, so a shard was full for ever once four markets had been in it. `KpfTNoteApply` (1,284 bytes) now empties a
row that carries no unit again.

Not built: a shard of more than four markets at one time.

### 6b. The commands of an account and of the reserve (25 November 2026)

`crates/kpf-cli/src/acmds.rs`; what they keep is `crates/kpf-protocol/src/aobj.rs` (an account, a shard, a note, a
statement with what it was stated on and the bytes it stated). Every command builds one transaction with one
builder, verifies it, and writes the files of what it made.

| Command | Who | Does |
|---|---|---|
| `shard-open`, `shard-fund` | the clearing party | opens a shard through an anchor; adds to what it holds |
| `shard-draw` | the shard's owner | states a draw (default: all above the requirement at the prices given) |
| `shard-note` | anyone | applies a note to its shard |
| `acct-open`, `acct-leg` | the user and the clearing party | open a covered account; add a leg (and write its note) |
| `acct-add` | a side | adds margin |
| `acct-take` | a side (or anyone, for side B) | states a take |
| `acct-leave` | a side | states that a leg leaves |
| `acct-liq` | anyone | states a step of a liquidation |
| `acct-end` | a side | ends an account with no leg |
| `stated-settle` | anyone | applies a statement after its dispute period, and writes the files of its notes |
| `stated-dispute` | anyone | disputes a point: the one named, or the first wrong at the prices given |
| `stated-answer` | anyone | answers the point disputed or another that is wrong; a price with a price note's citation |
| `stated-lapse` | anyone | ends a dispute nobody answered |
| `draw-note` | anyone | ends a draw by a note that is not applied |
| `acct-show` | anyone | prints an object; of a statement, the first point wrong at the prices given; of a shard, what can be drawn |

Ran offline as one story (`crates/kpf-cli/tests/clearing_cli.rs`): a shard of 50 KAS; a covered account of 20 and
2; a long leg of 10 units at 10 and its note applied; at 13 the leg leaves by statement, is disputed at its end,
answered right, and applied; the shard pays the 28 KAS the pot could not; the account is ended; the owner draws
the 22 KAS the shard has left; and a draw stated over a note that was not applied is ended by it.

Not built in the tools:

- What two parties sign (`acct-open`, `acct-leg`) takes both keys on one machine: no partly signed file, and
  `cosign` does not read these objects.
- `exec-run` watches neither accounts nor shards: nobody disputes, answers, applies a note or settles unless a
  person runs the command.
- An account that no shard covers has no opening (no script opens one).
- A price point was answered in the engine only: `stated-answer --note` needs a node for the citation.
- Nothing of 6a ran on testnet.

## 7. What must hold

| | Invariant | Known |
|---|---|---|
| I1 | An operation that nobody signed changes nobody's worth at any price (mark, replace, merge, sweep) | M for mark; E for replace and merge today, with F41 open (the one that stays is struck at the orders' price, not a cited one) |
| I2 | What leaves an account is signed for by who takes it, and is above its floor at a price cited for a time the operation is seen to have landed near | M (the floor); D (the two steps) |
| I3 | A claim is proved or refuted by the committee's prices and never both | E (today) |
| I4 | A shard never covers an account that would leave it under its requirement, and holds at least what it has been shown to owe | D |
| I5 | The sum paid out of a shard and of an account is what they held | M (numbers); D (objects) |
| I6 | Every script of the library goes on unchanged, and a reader knows the entry that served | E for the first; D for the second |

F41 in this form: a replacement or a novation still strikes the one that stays at the price of the two orders. The
mark gives the remedy the review asked for without changing them: a watchtower marks its account to a cited price
after it was struck, and a settlement at the entries then pays what each was worth at the last cited price.

## 8. What is not known, in the order it decides things

1. **Whether the citation's opcode is active: on testnet-10 it is** (run 25 November 2026: a script using it was
   mined, and a real transaction's citation recomputes its block's commitment; `RESEARCH-UNIFIED-MARGIN.md` 12).
   Mainnet is not checked. Were it not there, a price would be read as today (an epoch table spent, 8 to 11 kB, one
   use a block a table): the form stands, marks are dear, and the cover of section 6.1 is larger.
2. **The reserve's requirement in KAS** (M in numbers, D in objects): the net is in units and its worth moves. Cover
   and draw are checked at cited prices; between them only a call holds the shard to it. In numbers a call open to
   anyone, a cover that asks what a mark asks, and a user's maintenance no less than the reserve's rate are enough
   against a clearing party that opens accounts with itself. With objects (a shard's state, the cover's script, the
   order of transactions) it is not modelled yet.
3. **Who is paid first when a shard is short** (M: the order decides). Sweeping first is in every claimant's
   interest; whether a shortfall beyond that should be shared instead needs a period and a registration, which this
   form avoids.
4. **Liquidation in steps** (D): what discount, and that a liquidator of one leg cannot leave the rest worse.
5. **Fees a user lane costs** (T) if notes get a lane of their own.
6. **The payload convention** for served results (a proposal of the core's, without a number).
7. Spot tokens as margin and options: each is a leg with its own valuation; the form above does not change for
   them (section 10 says how, and what of it is known).

## 9. Order of building

1. The citation on testnet-10, alone (T, item 1). Then the note writer and the citation as library scripts.
   **Done** (E; `tests/thin_notes.rs`): the opcode is live, the proof's form agrees with a real block, and the
   library has the three scripts. The citing script itself is not yet spent on testnet.
2. Mark, take and collect, with the valuation by size; the answers of today rewritten on the same two services.
   **Done in the form of 4a** (E; `tests/thin_change.rs`): a take is stated and applied, or answered by cited
   prices. Today's close and liquidation still read the epoch table.
3. More legs in an account (the valuation exists; the account's state by size).
   **Done in the form of 4b** (E): no size classes; a take is answered a point at a time.
4. The reserve: shard, cover, sweep, claim, call. In the model with objects first, with the review's attacks.
   **Done as scripts** (E, section 6a): nine scripts, reviewed once (round 32). There is no call: the requirement
   binds the clearing party at the draw.
5. Liquidation in steps.
   **Done for an account** (E, 4b): one leg a step.
6. Tools, then a live run of 1 to 4 together.
   **Done for a position** (`docs/LIVE-4.md`, fifteenth and sixteenth parts): the commands, an executor that watches
   statements, and two runs on testnet-10. **For an account and the reserve the commands are done** (section 6b),
   offline only: no executor watches an account or a shard, and nothing of them ran at a node.

## 10. The rest of the goal: can it be expressed (25 November 2026)

`GOAL.md` names eight services. Five are scripts (sections 4a, 4b, 6a). This section asks of the other three, and of
the two items of its order of work that were not begun (spot, options), whether the forms above carry them. Each
row says what the form is and what the answer rests on. Three rows are built (the handing over of a side, the move
of a leg, options); each was reviewed (rounds 33 to 35, section 4b).

| Service of the goal | Form here | Rests on |
|---|---|---|
| Any counterparty: a side is handed over without the party that stays | The key of side A in an account's moving part becomes another's, signed by the two (who leaves, who enters). The legs are not touched, so no leg is shown and no price is read. The party that stays is held by what it asked when the account was opened: the margin of whoever enters is at least its ask (the two asks are in the moving part today). Side B of a covered account is its shard's owner and is not handed over. | **E** (`KpfTHand`, 593 bytes; a hand-over is a transaction of 1,827 bytes; `tests/thin_reserve.rs`; reviewed in round 34: nine refusals at the engine): who leaves and who enters sign, side B does not; less margin than there was, or than side B asked, is refused. What the place is worth is paid between the two in the same transaction, held by their signatures of the whole. The same rule runs for a position (`KpfTReplace`, E and T). |
| A clearing party takes the user's side at once | The clearing party is side B of every covered account and signs with a program that is always there. | D; what it signs is the next row. |
| A leg moves between accounts in one transaction | Two accounts of one clearing party are spent together: the leg leaves the one (the legs after it are shown, and the chain is hashed again) and is appended to the other, with its entry as it was; margins move by what the three sign (who leaves, who enters, the clearing party). The shard's rows do not change where both accounts are under one shard, so no note is written; under two shards, the note of leaving and the note of entering. | **E** for two accounts under one shard (`KpfTLegMove`, 1,449 bytes; a move is a transaction of 3,529 to 3,752 bytes for a leg with none to two legs after it; `tests/thin_reserve.rs`; reviewed in round 34: 23 refusals at the engine): any of three legs moves, stands last in the other account with its entry, no note is written, and without any one of the three signatures it is no transaction. Up to six legs stand after the leg that moves. D under two shards (the two notes). |
| ... without the clearing party's signature | Not as an immediate move. A leg that leaves takes its gain or loss with it: a user who moves its winning leg out leaves an account below its maintenance, and one who moves a losing leg into an empty account of a friend leaves the loss with the clearing party. Whether either is so is a question of prices. So without the clearing party it is the stated path: the leg leaves by statement (kind 4, E) and enters by the other account's own opening. | D: this is a limit, not a gap. A whole side is handed over without the clearing party (first row); one leg of several is not. |
| Spot as margin: a token held in the account counts at a discount | Three parts. (1) A leg of a new kind in the account: a token's market and an amount; its row in a trace adds the amount's worth at the stated price, less the discount, to what side A is counted to have, and gives side B nothing. (2) A vault: an object that is a covenant of its own (one id an account), which owns the tokens (a KCC-20 is owned by a key, by a script's hash or by a covenant id; the vault's id is the owner). (3) Tokens enter by a script that raises the leg's amount and holds the token's output to the vault; they leave by a ticket: a statement's settlement writes an object that names the vault, a key and an amount (as it writes a note to a shard today), and the vault's script pays the ticket and holds the rest to itself. A liquidation step on the leg writes the ticket to the liquidator, who pays the discounted worth into the account. | **M** for the counting (`tests/research_option_spot.rs`): what side A's tokens are worth less the discount, taken from what side A keeps whole (the row has that number since options: section 4b), which counts for side A alone (side B's standing does not read it), and which a liquidator's price covers where the token fell by less than the discount less the liquidator's fee (3,000 random cases). D for the vault and the ticket. T, by another project, for what the form needs of the chain: a script that owns KCC-20 tokens under a covenant id of its own, reads the token's state and holds the token's outputs to its rule is how the order book in `vendor/kob` keeps an ask's custody, and its orders ran on testnet-10 (`vendor/kob/docs/reports/TN10-TEST-2026-10-14.md`; `vendor/kob/docs/spec/matcher.md`, custody). Not known: the size of a vault's transaction (the token's own script runs beside ours), and a price note for a token's market (a committee signs it as it signs any). |
| A spot purchase and a futures sale in one transaction | The entering of tokens (above) and `KpfTLegAdd` on the same account in one transaction: two scripts of the library, one object. | D. Today each script of an account takes the object alone: either one script does both, or the two are made to run on one object (a position's scripts do: an object names the input of its script). |
| Options | A leg whose direction's place says its kind and its strike, and whose entry is the price it was last counted at (section 4b). Its row adds to A's gain what the option is worth if exercised at the price stated less what it was worth at its entry; a take moves that as it moves a futures leg's gain. The premium is paid when the leg enters, as margin moves today. It leaves by its holder's side alone, for what it is worth exercised. | **E** (`KpfTStepRow`, 2,560 bytes; `KpfTStepEnd`, 2,078; `KpfTStepLiq`, 2,561; `KpfTLegAdd`, 1,059; `KpfTShardPrice`, 1,655; `tests/thin_option.rs`, eight tests; reviewed in rounds 33 to 35, section 4b): a take on an account of a futures leg, a call held and a put written is stated and applied, and nothing is taken twice; the rows of options are answered right, and wrong where they are miscounted (and on ten random accounts); an option ended by whoever wrote it, or by a liquidation of whoever wrote it, has a wrong row; an option enters a covered account, is counted in a row of its own in the shard, and leaves with the note of that row and what is owed; the price of a row of options is answered with its underlying's citation; a holder that keeps nothing but its option is below no maintenance, takes out nothing of what the option was counted at before it exercises, and a sompi of it is a wrong end. In numbers: nothing is taken twice (3,000 random accounts), and the maintenance of the underlying covers a written option (4,000). Not built: a last day, a price for buying an option back (section 4b). |

What this says, in one line each:

- **Every service of the goal has a form in what exists.** None needs a new kind of object but the vault and its
  ticket, and both follow the shard and its note.
- **One condition, which is not a wall**: one leg of several moves at once where the clearing party signs, and
  its program is always there, so for a user this is at once. It is only where that program does not answer that
  the move is not immediate, and the user is not held then either: the whole side is handed over with no
  signature of the clearing party, or the leg leaves by statement after the dispute period. Somebody has to look
  at prices when one leg of several leaves (else the winning leg is taken out and the losing one left behind);
  that somebody is the clearing party's program, or the chain by a statement.
- **The clearing side passing to another clearing party without the user's signature** (D): side B's key and the
  shard's id in the moving part become another's, signed by the two clearing parties. The user is held by what it
  asked of whoever takes side B's place (its ask, in the moving part): the newcomer posts that much in the account
  itself. A user who takes a clearing party's shard as its cover asks little of that party and much of any other.
  Every leg is shown, and for each a note that it left the one shard and a note that it entered the other: two
  notes a leg, so this is a transaction of an account of few legs, or of one leg at a time.
- **What only building answers**: the size of a vault's transaction, with the token's script beside it. The row
  is built for options and has the place of a token's worth (what side A keeps whole, below zero): a row is 112
  bytes, and an option's row is answered in a transaction of 4,089 to 4,425 bytes on accounts of one to five legs.

Order, if these are built (the handing over of a side, the move of a leg and the option leg are): spot's vault
and ticket (the vault needs the token's scripts in the test bed; `KpfTStepRow` learns the token's leg, and the row
is as it is).

