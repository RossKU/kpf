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
written option that is worth nothing then leaves only with its holder, or by the two signatures: `KpfTLegOut`,
section 11, which is also how an option is bought back, for the price the two sign). And a holder that stands below its own maintenance by its other legs does not
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

**What this section's cover is, and is not, as of review 41 (26 November 2026).** Against a clearing party that
keeps to it, the shard pays what an account's pot could not, beyond the band of the tolerance. Against a clearing
party that opens accounts with keys of its own to take what its shard holds, **it does not hold yet**: five
reviews (37 to 41) each found a way, and two are open (F66, F67). They are of one kind, and closing them one at a
time has not converged: section 11, at its end, says why and what the choice is. Until that is settled, a user
relies on a clearing party's shard as it relies on the clearing party.

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
written by a statement that waited its dispute period, or by the owner's own signature (`KpfTLegOut`, section 11).
An owner settles what is ready, applies the notes, and draws.

What whoever watches a shard does not have since `KpfTLegOut` (review 36, to know): time. A leg's leaving no
longer waits a dispute period, so an owner can add and remove an equal leg in an account with a key of its own, in
two transactions and with no bond, apply the release alone, and state a draw over rows that count a user's leg no
more. The draw is still ended by the join that is not applied (`KpfTDrawNote`; engine), but whoever watches has
the draw's dispute period to find it, among all the notes to the shard. That is the work of an executor's loop
for shards (section 11, item 3), which is not built.

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
| `shard-note` | anyone | applies a note that legs left, whole, or pays a note of the rest |
| `acct-open` | the user and the clearing party | open a covered account of the user's coins and no leg |
| `acct-add` | the user | adds to its coins |
| `acct-take`, `acct-leave`, `acct-liq` | a side; anyone | state a take, a leaving, a step, on an account no shard covers (a covered account is refused: its own kinds have no command yet) |
| `acct-end` | a side | ends an account with no leg: its coins to its user |
| `stated-settle` | anyone | applies a statement after its dispute period, and writes the files of its notes |
| `stated-dispute` | anyone | disputes a point: the one named, or the first wrong at the prices given |
| `stated-answer` | anyone | answers the point disputed or another that is wrong; a price with a price note's citation |
| `stated-lapse` | anyone | ends a dispute nobody answered |
| `draw-note` | anyone | ends a draw by a note (that legs left, or of the rest) that is not applied |
| `acct-show` | anyone | prints an object; of a statement, the first point wrong at the prices given; of a shard, what can be drawn |

(26 November 2026: the table is of what remains after the old form of a covered account went, 14.10 (d);
`acct-leg` and `acct-out` are gone, and the story below is the old one, kept as what was run. What runs now: a
shard of 50 KAS; a covered account of 20 KAS that takes nothing of a second party's; 3 KAS added by its user and
none by the clearing party; a take of the old kind refused; the account ended for its 23 KAS; the shard drawn
whole, applied, the amount and the bond back.)

Ran offline as one story (`crates/kpf-cli/tests/clearing_cli.rs`): a shard of 50 KAS; a covered account of 20 and
2; a long leg of 10 units at 10 and its note applied; at 13 the leg leaves by statement, is disputed at its end,
answered right, and applied; the shard pays the 28 KAS the pot could not; the account is ended; the owner draws
the 22 KAS the shard has left; and a draw stated over a note that was not applied is ended by it. And, between
the first leg's note and its leaving: a second leg enters and leaves at once by the two signatures (`acct-out`),
and the shard counts as before once its two notes are applied.

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

## 11. One form: the account (25 November 2026)

Two families stand in this repository. A position (two legs at most; `DESIGN-COMMITTEE.md`) has orders, their
match, the replacement of who leaves by who enters, an executor's loop (`exec-run`) and runs on testnet-10. An
account (sections 4b, 6a, 10) has the shard, the liquidation in steps, options, the handing over of a side and the
move of a leg, and has none of the first list. What the goal asks is in the second; what a user trades with is in
the first. The decision of 25 November 2026: **the account is the one form**. What a position has is built for
the account; a position is then an account of one or two legs, and its scripts go where the three families before
it went.

What an executor does for a user that finds nobody on the other side is not a rule in either family and does not
become one. In the position family it is `exec-run --do take`: off unless asked, with a limit on the executor's
own margin that has no default (`--take-max`), at a spread from its own price, by an order of its own that the
next round matches as any two orders; no script knows of it. In an account the other side is a clearing party
from the first leg, so there is nobody to find; what was missing is the way out at once.

| Way out of a covered account | Who is needed | When | |
|---|---|---|---|
| The leg leaves by the user's statement (kind 4) | nobody | after the dispute period | E, reviewed |
| The side is handed over (`KpfTHand`) | who enters | at once | E, reviewed |
| The leg leaves by the two signatures (`KpfTLegOut`) | the clearing party's program | at once | E, reviewed (round 36: F60, closed in the scripts) |

**`KpfTLegOut`** (1,294 bytes; a leg that leaves is a transaction of 2,728 to 2,955 bytes with none to two legs
after it; `tests/thin_option.rs`, the last test; `acct-out`). What `KpfTLegAdd` does, the other way: the chain is
hashed again without the leg (up to six legs after it), the other legs and their entries stay, and the note that
the leg left is written for the shard as a statement's settlement writes it (the row of the leg's kind and
market, side B's units, the size). No price is read: what the leg is worth is what the two sign for (the tool
computes it from a price, or takes the amount: the price of an option bought back).

| Who owes for the leg | How it is paid |
|---|---|
| The user | from its margin to the clearing party's, inside the account |
| The clearing party | from its own coins, by an output of the transaction the user signs. Its margin in the account does not fall here, by any amount |

The user may take margin out in the same transaction, and the clearing party may add. Refused at the engine:
either signature missing; the clearing party's margin falling, to the user's margin or out; a margin below zero;
the account that goes on keeping the leg or losing another; a note of other units, of the other kind's row, of
another shard or clearing party, or of a leg that entered.

**Review 37** verified F60 closed in both scripts (400 random amounts, none taken with side B's margin lower; 37
mutations refused, 14 of them at the ends of 64 bits; the same object at both places of a move refused), and found
F61, which is older than this script (the box at the head of section 6a, and the end of this section).

**Review 36** found the first form wrong (F60, medium): it let the two move any amount from the clearing party's
margin to the user's and the user take it out, so that with a user's signature, which costs the user nothing, a
clearing party's margin left an account by another way than its shard; and `KpfTLegMove` let it leave directly by
the three signatures. What a clearing party has in its accounts is what anyone deposits to its shard by a take for
side B, for the users the shard owes: it leaves by the shard, or to the user by a statement at prices that are
answered for. Now `KpfTLegOut` moves nothing out of side B's margin, and `KpfTLegMove` (1,459 bytes) asks that the
two changes of side B's margin are together not below zero (it moves between the two accounts, and not out). The
rest held at the engine: 400 random amounts taken or refused as the model says, 30 changes of a taken transaction
refused, the note of every kind of leg equal to its join and to a settlement's, 20 other notes refused, 13 wrong
chains, an object that carries a statement not opened.

The clearing party's program signs or does not, as it chooses, with what limits it chooses: that is its tool's,
as `--take-max` is. A user it does not answer is not held: the first two rows of the table need no signature of
it.

Order of what follows, each on paper before it is a script:

1. A request the user signs and the clearing party's program completes: today both keys sign on one machine
   (`acct-open`, `acct-leg`, `acct-out`), which no two parties do.
2. An order into an account: what a user rests for any clearing party to fill (the position's order is a margin
   and a tip that wait for the other side; an account's names the account or the terms of one to open).
3. `exec-run` for accounts and shards: notes applied, statements settled, disputed and answered, the requests of 1
   signed within the program's limits.
4. The position's scripts to where the earlier families are, once 1 to 3 ran on testnet-10.

**F61 and what closes it** (on paper, second form, 26 November 2026; not built, not reviewed). Three things, each
as designed, make it: an entry nobody checks, a gain the pot cannot pay written as owed, and an owed note paid
before anything the shard is asked to hold. Paying an owed note only above the requirement would refuse users
what they are owed, so the note's rule stays. The entry is what is wrong: `KpfTLegAdd` is the one change of an
account that sets a price and is neither stated nor answered for. Everything else of this design is stated, and a
citation only answers (section 4a).

**A leg enters a covered account by a statement** (kind 6), and `KpfTLegAdd` is withdrawn.

| | |
|---|---|
| Who states | the account's two parties, by two signatures on the statement (a statement is one party's today) |
| What is stated | a trace of the account's legs and of the leg that enters, last, at the prices of the statement's time; and of that leg its row's name in the shard and side B's units (the 92 bytes a leaving leg has today) |
| The entering leg's row (`KpfTStepRow`) | the leg's entry is its row's price, so it gains nothing as it enters; it is a leg (size within the bound, a kind, a strike within the limit); what the statement says of it is its own |
| Its price (`KpfTStepPrice`) | within the tolerance of the committee's at the statement's time, as any row's |
| The end (a new script) | the row before the last carries the account's chain; side A stands at its maintenance with the leg (its worth, and what the options it holds are counted at); no margin below zero. Outputs: the account with the leg and every entry at its price, and the note that the leg entered |
| When | stated at once; applied after the dispute period, as any statement. Until then the account carries the statement |

What each part closes:

| | |
|---|---|
| An entry of one's own choosing | the entry is a price of the statement's time that anyone disputes with a bond; a wrong one is answered with one citation and costs the claimers their bond |
| Two accounts of the owner's own keys with no margin, one long and one short at true entries (owed by the shard whichever way the price moves, the side that lost having nothing) | the end asks side A's maintenance at the entry. With it this is what any user's default is, and somebody is paid to liquidate it |
| An option struck far in the money, entered for nothing (the same taking by a strike of one's choosing) | an option enters counted at what it is worth exercised at its entry: its holder is paid none of that, and keeps it whole as margin. What a holder pays its writer for the option is beside the account |
| The clearing party's margin handed to a user who signs (F60, by an entry in place of a payment) | a gain comes from two prices that were answered for, and no other way |

What this costs a user: an entry is final a dispute period after it is stated, and the account carries the
statement until then (no other change of it: the way out at once is at once after that). It is the cost every
priced change has here. What it does not cost: the price is the one both signed, from the moment it is stated; no
citation at an entry; margin asked on the account as a whole, not a leg at a time. A leg enters at the committee's
price and what the clearing party charges is paid beside it: one price (`GOAL.md`, the third line).

Left as it is, and said: whoever states chooses the statement's time within the lateness the terms allow, so two
parties who agree can enter at the better end of that window (as a take already chooses its time); and a shard
whose rows are large and set against one another is asked little, while a default of one side at a gap in the
price takes from it what a margin did not hold. That is the rate on the gross, a number of the terms, and what it
should be is not computed (section 8).

**Review 38 attacked this design on paper** and found that it does not close F61 as written (H1, high), and one
thing older than it (M1, medium). The four takings of the table above it could not break, on a market that is
priced by a committee the shard's users chose.

| Found | What it is | Decided |
|---|---|---|
| H1: a leg on a market nobody prices, or that the owner's own committee prices | A leg names its own committee, and nothing binds it to one the shard's users know. A price of a committee of the owner's keys is answered right at any value. A price nobody signs is answered by nobody: a dispute of it lapses, both bonds come back, and what nobody disputed settles. So the owner enters and leaves at prices of its choosing all the same; a watcher is paid nothing for stopping it and must never miss a window | Two rules. (1) **A shard is of one committee**: its id is the hash of what its anchor gives and of a committee's id, which whoever opens it names; a leg enters an account under it only if the leg's committee is that one (the end of an entry is shown the two and holds the entering leg to it). A user who takes a shard as its cover reads its committee as it reads its rows. (2) **The price of a leg that enters is the claimers' to show**: a dispute of it that nobody answers ends the entry and pays the disputer the claimers' bond, where a dispute of any other statement that lapses returns both. An entry on a market nobody prices then costs its claimers a bond each time, and whoever watches is paid. No citation where nobody disputes |
| M1: an account that carries a statement takes no other change | One party states a change (a take of nothing is enough; for side B anyone states it) and the account is closed to the other for a dispute period, and again: the other's way out at once and its own statement wait as long as the first has coins for bonds that come back. An account of n legs built by entries takes n dispute periods | Open. It is as old as section 4a and is not of this design; an entry by statement makes it met more often. To design: whose turn it is after a settlement, or a shorter period for what moves no value |

Also decided for the building: the statement of an entry carries two signatures (`KpfTChange` takes one today) and
the bond is the claimer's, as any statement's; every row of an entry's trace is a point as in a take, so an old
leg marked at a false price in it is answered wrong; the first leg of an account enters after a row that is
nothing; an entry adds no value (margins are added before it, by `KpfTAddC`).

Left as it is, and said: a committee the users chose that stops signing. Nothing priced is answered then, here as
everywhere in this design; with rule (2) nobody enters under it, and what stands is left by the exit of a market
with no price.

**Built, first stage** (26 November 2026; E, `tests/thin_enter.rs`, six tests; `KpfTLegAdd` is still in the
library, so F61 is still open):

| Script | Bytes | |
|---|---|---|
| `KpfTShardOpen` | 650 | the id is of the opening, the committee, and the shard's ten terms, lateness, dispute period and bond rate |
| `KpfTEnter` (new) | 1,326 | the two parties state an entry: three numbers, the leg whole (111 bytes), the seed of the shard's id, the trace |
| `KpfTStepRow` | 3,472 | the row the statement names is, in an entry, the leg it shows: a leg, with the row's price for its entry, and it stays |
| `KpfTStepPrice` | 1,564 | takes an entry's rows |
| `KpfTStepEnter` (new) | 1,955 | the end of an entry |
| `KpfTPoint` | 1,487 | a point of an entry that nobody answers costs the claimers their bond |
| `KpfTLegMove` | 1,481 | the two accounts have the same terms and periods |

An entry is stated in a transaction of 2,908 bytes and applied in one of 2,602 (one leg). `KpfTStepRow` is now
the largest script of an answer: what it asks of an entering leg belongs in a script of its own.

**Review 39** (at the engine) found that the first build did not close F61, and one more thing:

| Finding | What it was | Now |
|---|---|---|
| F62 (high) | The tolerance, the maintenance rate, the dispute period, the lateness and the bond rate an entry is judged by were the account's own, which its two keys choose when they open it (`KpfTOpenC` asks nothing of them). An account of the owner's keys with a tolerance of 50 % and no maintenance entered at 5 and left at 15 where the committee said 10, each answered right, and the shard paid all it held | A shard's id is of its committee and of the terms and periods the accounts under it have, which are its own; the end of an entry holds the account to them, and a leg moves only between accounts of the same. Who takes a shard as its cover reads them in the shard |
| F63 (medium) | The leg that entered was shown to nobody: the statement carried its row's name (a hash) and units. An option's strike was in nothing shown, so nobody but the two parties could answer that leg's row, then or later | The statement shows the leg whole, 111 bytes for 92, and its row is held to that leg |

What held: 52 statements refused by `KpfTEnter`; 49 entering rows and 36 ends each as the model says; the lapse of
an entry's point to the sompi; a leg of another committee never has a right row and a right end at once. To know,
from rule (2): where the committee does not answer within the dispute period, an honest entry that is disputed
costs its claimers half a bond.

**Review 40** (at the engine) verified F63 closed and found F62 closed only where an entry's end is answered,
and two more ways for an owner with only keys of its own to be owed by its shard:

| Finding | What it was | Now |
|---|---|---|
| F64 (high) | The terms and periods were held in the answer of an entry's end, and an answer is given only where somebody disputes within the dispute period, which was the account's own: an account of a dispute period of one block settled an entry nobody had the time to dispute, then a leaving at twice the price | `KpfTOpenC` is shown the seed and the committee of the shard's id and opens an account under it only with the shard's ten terms, lateness, dispute period and bond rate. The lease and what each party asks of the other stay the account's own |
| F65 (high) | On an account wholly of its shard's terms: a leg entered at 9.95 and left at 10.05 where the committee said 10, every point answered right. The prices of a statement are only near the committee's; twice the tolerance of a leg's worth is a gain from prices alone, and what the pot could not pay of it was owed by the shard. Four rounds emptied a shard | What the shard is asked to pay is what is owed at any price within the tolerance: `KpfTStepLiq` writes the note of what the pot could not pay less twice the tolerance of the legs' worth. A futures leg is in both sides' worth and is counted twice in it (exact would take a number more in a row) |

**What F65 makes of the cover, to say plainly.** The reserve pays what an account's pot could not, beyond a band:
with a tolerance of 0.5 %, 2 % of the worth of the account's futures legs (1 % were it counted once), and 1 % of
the worth of options. Up to that band a user is covered by what the clearing party keeps in the account itself,
which the user asks of it when the account is opened (its ask). The band is the price of prices that are stated
and answered for within a tolerance, and it is as narrow as the tolerance: a committee whose price a statement can
follow within 0.1 % gives a band a fifth as wide. It is not asked where the pot paid: a user whose clearing party
holds its ask in the account is paid in full.

Left as it is, and said: the same band is what a clearing party and a user who signs with it can move from the
clearing party's margin to the user's, a round at a time, by statements that are each answered right (what F60
closed for a payment stands, within the band, for a price).

**Review 41** (at the engine) verified F64 closed and F65 closed for futures legs, and found two more:

| Finding | What it is | |
|---|---|---|
| F67 (high) | The band is counted at the leaving's prices and from the sides' worth, in which an option stands once (its writer's). A put side A holds, struck at its entry so that it is counted at nothing and asks no margin, entered at the upper end of the tolerance and left at the lower, gains 1 % of its underlying at the committee's price; the band is 1 % of it at the lower end. The shard pays the difference, twice the tolerance squared: 11 KAS on 11,000 units, with a margin of 30 that comes back, eight such legs an account | open |
| F66 (medium) | The end of an entry asks side A's maintenance and nothing of side B. A step of side B's liquidation takes its fee from side B's margin, so where that is nothing nobody is paid to step, and the leg stands while the price moves: 140 units entered at 10 with nothing on side B left at 11, and the shard was asked 109 KAS and paid its 50. With a second account the other way the owner is owed whichever way the price goes | open |

Also from it: where a leg's worth fell by more than two thirds between its entry and its leaving, the band at the
leaving's worth is less than the entry's half of what prices could make (low: 48.51 KAS on a gain of 9,900). And
what the band costs an honest user grows with its account: owed 20 KAS on a leg worth 140, it is asked 17.2 of the
shard; with nine other legs of that worth standing beside it, nothing.

**Why this is not closed a finding at a time, and the choice** (not decided). F61, F62, F64, F65, F66 and F67
are one thing seen six times: the shard pays what an account says it is owed, the account's other side can be the
shard's owner, and the one rule that holds the owner, the requirement, is asked only when it draws. Nothing is
asked of a shard when it takes a leg, and nothing as the price moves. So an owner takes, through an account of
its own keys, a position its shard cannot carry; where the position wins, the shard pays its owner; where it
loses, the owner's own margin goes into its own shard. Each finding was a way to win surely or cheaply (an entry
of its choosing, terms of its choosing, the tolerance, an option that asks no margin, a side nobody liquidates).
Close them all and what is left is the same at fair odds: an owner with coins outside its shard turns the shard's
into its own with a chance as large as its coins are to the shard's, and the users' cover is what it lost.

A clearing house holds a member to its requirement all the time: a member below it is called, and its positions
are closed by others where it does not answer. That is what is missing here, and it is not one script:

| | What it is | What it costs |
|---|---|---|
| A. The cover is of a clearing party one trusts | Leave the reserve as it is, with what is closed. Say in every place what it is: capital pooled behind many accounts, which pays where a clearing party that keeps to the rules cannot; no protection from one that turns on its users. A user chooses its clearing party as it chooses a broker | nothing more to build. The goal's line "no party that must be trusted to stay" is not met for the cover |
| B. No shared cover | A user that does not trust takes an account no shard covers: the clearing party's margin stands in the account itself, as between any two parties (section 4b; it exists) | the capital the reserve was to save: this is the two-party position again |
| C. The requirement held all the time | Three things. A leg enters only against what the shard holds over what it is asked with the leg. The shard's rows carry what its legs were entered at, so that what it has lost at given prices is a statement on the shard as a draw is. A shard below what it is asked is stepped by anyone, for a fee the shard pays: its users' legs leave at the prices stated, paid first | a design of its own, on paper first: the entry and every mark of an account then touch the shard or leave it a note to apply before it is counted, and the order in which a shard's users are paid when it fails has to be said. Weeks, and reviews of its own |

What is built stands under any of the three: the entry by statement (one price, a leg shown, terms and periods
of the shard), the band, the committee in the shard's id.

**Chosen, 26 November 2026: C.** Section 12 is its design, on paper.

## 12. A shard held to its requirement all the time (26 November 2026; on paper and in numbers, nothing built)

What section 6a promises and does not keep: a user of a shard is paid what it is owed whatever the shard's owner
does. Reviews 37 to 41 are why it does not (section 11, at its end). This section is the form that would, what
was checked of it in numbers, and what is not known. It replaces nothing yet.

**The claim to be built towards.** Whatever the owner of a shard does, with any number of accounts of its own
keys, every user of the shard is paid in full what it is owed beyond the band, as long as the price moves by less
than the maintenance rate less the tolerance in the time it takes whoever watches to act. A move beyond that is
the gap any margin has.

**Three things, and why each.**

| | What | Why |
|---|---|---|
| Books | The shard knows what its side has gained or lost at given prices: beside each row's net and gross units, one number for all rows, what its legs were entered at (the sum of side B's units times their entries). Its equity at prices is what it holds, and what its net units are worth there, less that number | today a shard knows what it holds and how many units it has, and not that a leg entered at 10 stands at 14 against it. What a shard owes is then seen only when an account says so |
| Gate | A leg enters only where the shard's equity is at least its requirement with the leg | else an owner takes, through an account of its own, a position the shard cannot carry |
| Call | A shard whose equity is below its requirement is stepped by anyone: the legs of the accounts under it leave at the prices stated, each user paid what it is owed, for a fee the shard pays. No draw meanwhile | the requirement was asked at a draw alone. A clearing house calls a member below its requirement, and closes its positions where it does not answer |

With the three, what an owner's own account wins is what the shard owed a counterparty while it was still held to
its requirement for everything else; the requirement, not the owner's good will, is what stands behind the users.
A step of side B's liquidation by an account's own numbers (which F66 found nobody is paid for) goes: side B of a
covered account stands by its shard, and is stepped when its shard is called.

**In numbers** (`tests/research_solvency.rs`; one market of futures legs, the books exact, whoever watches acting
once a tick, the owner knowing the next tick, entering accounts of its own as large as the gate lets in, leaving
the ones that would lose, and drawing all it may; 80 ticks a path):

| Rules | Ticks of | Users unpaid | The owner took, a path | Whoever stepped was paid |
|---|---|---|---|---|
| books, gate, call | 0.5 % | on 0 of 1,000 paths | 37 KAS of 500 | 464 KAS |
| books, gate, call | 2 % | 0 of 1,000 | 183 KAS | 323 KAS |
| books, gate, call | 4.5 % (the rate less the tolerance) | 0 of 1,000 | 369 KAS | 134 KAS |
| books, gate, call | 15 % | 134 of 1,000 | | |
| as built today (the requirement at a draw alone) | 2 % | 995 of 1,000 | | |

The owner still takes from its own shard, by winning as a counterparty and by its draws, and pays for the steps
it causes: that is its own capital, and no user's. The last row is reviews 37 to 41 with no trick of entry or
terms.

**What is not known** (each decides whether this can be scripts; in the order it decides things):

1. **Books that are whole.** The model's books know every leg the moment it enters. A script's books are told by
   notes, which somebody applies later: a leg whose note of entry is not applied is not in the books, and its
   account is owed all the same. Either what is owed is paid only for legs the books had (a note of what is owed
   then names what it is of, and the shard has four rows, not a list of legs), or the entry touches the shard
   (which the goal says an account does only at a default). Not solved.
2. **What tells the books of a mark.** A take moves entries and moves A's gain into its margin from side B's in
   the account: the shard's number of what its legs were entered at changes with it, by one amount a statement.
   Every settled statement of a covered account then leaves a note for the shard (a leaving and a deposit do
   today; a take does not).
3. **Options.** What an option has gained is not units times a price: no sum over legs gives it. The books above
   are of futures legs. An option under a shard is either not there in the first form, or counted at the whole of
   its underlying with what it gained seen only at a mark.
4. **How an account knows its shard is called.** A statement on an account does not read the shard. The call
   would leave an object that a step of side B shows, as a draw is ended by a note today.
5. **Who is paid first when a shard fails at a gap.** Today whoever applies its note first. Closed in an order
   somebody chooses, the owner's own accounts can be first (the model takes that order).
6. **What it costs.** The statements of a call are a trace over the shard's rows and prices that are answered
   for, with a dispute period: the time whoever watches needs is that period, and the move the requirement must
   hold is the move over it. And the account that carries a statement takes no other change meanwhile (M1).
7. **The band and the gate together**: the gate is asked at prices within the tolerance, as everything is.

**Review 42 attacked this section on paper and in numbers** (`tests/auditT1_solvency.rs`, the reviewer's copy
and extension of the model; `docs/audit/REVIEW-T1b.md`). In the model's own terms the claim held (3,000 paths,
the owner acting again after the price moved and before whoever watches; several accounts both ways). It breaks
on what the model took for granted:

| Found | What it is | Decided (on paper) |
|---|---|---|
| H1 (high): the books count a loss the shard does not have | The bound assumes that whoever watches can step an account that lost. An account that carries a statement takes no other change, and a take of nothing is right: its owner states one, settles it and states the next in the same breath, for as long as it likes. The owner holds a pair of its own accounts so; the price moves; the winner leaves and is paid, the loser stands unstepped with its loss counted as the shard's equity, and the owner draws against it. Users unpaid on 257 of 500 paths at ticks of 2 %. (This is M1, which stood as medium: here it is what the claim stands on.) | A step is stated over a statement that is none: the standing statement ends, its bond back to its claimer, and the step stands in its place. A step is right only where the leg that leaves is the account's largest, or the side stands at its maintenance after it (a step that lets a leg of one sompi leave is no step). Nobody but side A states on a covered account otherwise (the next row), so a stranger holds no user's account |
| H2 (high): books that are whole | With notes as they are: a leg whose entry was never applied wins and is paid; and a leaving rides on another leg's entry, since a row is sums (a leg booked at 20; another of the same market, entered at 10 and never booked, leaves at 20 and is paid 10 a unit) | The rule of the notes, in four parts. The shard pays nothing for a leg whose entry it has not booked. Booking is the gate, asked at the booking's prices and counting what the leg gained since it entered. What is paid for a leg is counted from the entry booked. And a booked leg is unbooked by its own leaving alone: the booked entry goes on as an object of its own, and the note of a leaving is applied only with it beside. An account never reads the shard or waits on it; a leg's cover begins when it is booked, and a user is told so |
| H3 (medium): the call at a hair | A shard drawn to its requirement is called rightly after any move, the fees are paid first and take what it holds, and the caller may be the owner | Two levels, as an account has: a draw leaves the equity at the requirement counted the tolerance more, a call is right only below it counted the tolerance less, at any price within the tolerance. The caller posts a bond. Fees after the users, and capped |
| H4 (medium): coins and equity | Equity counts what accounts that lost owe, which stands in their margins until a statement brings it in; a draw of equity above the requirement pays coins against it | A draw is the smaller of the coins above what is owed and the equity above the requirement |

And with them, what an account under a shard is changes (the reviewer's smallest first form, taken):

- **Side B has no margin in a covered account.** The account holds its user's margin; the other side is the
  shard's books. What went with side B's margin goes: its floor and its step by the account's own numbers, the
  deposit of a take for side B, what `KpfTLegOut` and `KpfTLegMove` asked of it, and the band's twin (what a
  clearing party and a user could move between them).
- **A leg's entry is the entry booked, and does not move with a mark.** What a leg gained counts for its side's
  standing in the account as it does today (a gain of one leg is margin for another); taken out, it is paid by
  the shard from the entry booked, by a note. A mark then tells the books nothing.
- **Futures legs only under a shard in the first form.** An option stands in an account no shard covers.
- **At a call nothing is paid until every row is empty**; then every note the same share, fees after. Outside a
  call a note is paid in full as it comes.

What this costs: a leg is covered from its booking, a statement on the shard that takes many notes at once and
has a dispute period; a late top-up closes every user of the shard at prices within the tolerance, the band taken;
and against two-party margins (5.5 % of the worth, an account) a book that is set off is asked 0.5 % and a
one-way book 6 %.

Not decided: how many notes one statement on the shard takes, and what a booked object costs in bytes (one a
leg, where a row was four numbers a market).

**The second model** (`tests/research_solvency_notes.rs`, five tests; 26 November 2026): no margin of side B in
an account, legs booked or not, notes paid when somebody applies them and a statement on the shard waiting for
them, the two levels, a call that pays every note the same share, and whoever watches coming every so many
ticks. Three owners, each knowing the next tick: one that enters accounts of its own the way the price goes and
leaves the ones that would lose; one that holds a pair of its own accounts under statements, lets the one that
won leave and the one that lost stand (H1); one whose own accounts' entries are never booked (H2).

| Rules | Owner | Ticks of | Visits | Users unpaid |
|---|---|---|---|---|
| all | each of the three | 0.5 %, 2 %, 4.5 % | every tick | on 0 of 1,000 paths, nine times |
| a step does not reach an account that carries a statement | the pair | 2 % | every tick | 340 of 1,000 |
| every leg is paid, booked or not, and the requirement is asked at a draw alone (as built today) | the unbooked | 2 % | every tick | 996 of 1,000 |
| all | the first | 1 % | every 2nd, every 4th tick | 0 of 1,000 |
| all | the first | 4.5 % | every 4th tick | 72 of 1,000 |

**What the second model found that was not looked for: a book that is set off is held by the rate on the
gross.** Nothing gates a leaving, and none should (a user leaves when it likes). So one side of a set-off book
leaves, and until the next visit the other side stands against a shard that holds what a set-off book is asked:
the rate on the gross, and nothing on a net of nothing. Two users, 1,000 units each way at 10, the owner having
drawn all it may: the shard holds 100 KAS. The short leaves. A rise before the visit of 1 %, 1.5 %, 2 %: the long
is paid in full (the rate on the gross of the two sides and the band of the tolerance are 2 % of its worth). Of
4.5 %: it is short 245 KAS of 345. So the claim's bound is two bounds:

| Book | The move between two visits that is held |
|---|---|
| one way | the maintenance rate less the tolerance (4.5 % with the terms as they are) |
| set off | twice the rate on the gross, and twice the tolerance (2 %) |

The rate on the gross was the liquidation fee's rate, taken because the terms had the number. What it is here is
the capital a set-off book keeps for the time between one side's leaving and the call: for the one bound to be
the other, it is half the maintenance rate less three times the tolerance (1.75 % for 0.5 %), and a set-off
book is then asked 1.75 % of its legs' worth where two parties' margins are 5.5 % (three times less, not eleven).
It is a number of the shard's terms, read by its users, like the others.

Order: review of the second model (round 43: section 13, at its head); the scripts' design on paper (section
13); then scripts.

## 13. The smallest form of section 12, as scripts (26 November 2026; on paper, after review 43)

**Review 43** attacked the second model (`tests/auditT1_solvency.rs`, module `second`) and the draft of this
section:

| Found | What it is | Decided |
|---|---|---|
| N1 (high): a leg not booked loses what it loses and is paid nothing of what it gains | An entry settled with the clearing party's signature, and its booking was another statement, on the shard, that might never come (no room; and the gate, counting what the leg gained since it entered, takes losers and not winners). For the owner's own account that is right; for a user it is a bet it can only lose | **There is no leg that is not booked.** An entry is applied by a transaction that spends the shard with the account: the gate is asked there, at the prices the entry stated for the shard's rows, and the rows and the number take the leg in the same transaction. Where the gate does not hold, the entry is ended, the account as it was. No note of an entry, no booked leg as an object, no leg in an account that the books do not have |
| N3 (high for the claim as worded): an owner makes any book set off, then leaves | It enters an account of its own against its users' net (the gate welcomes it: the requirement falls to the rate on the gross), draws what a one-way book was asked, and leaves its account, at no loss, before a tick that goes its users' way. Unpaid on 38 of 1,000 paths at ticks of 3 %, 92 at 4.5 %, with the rate on the gross at 0.5 % | One bound, and the rate on the gross is what gives it: half the maintenance rate less three times the tolerance (1.75 % with the terms as they are), a number computed from the terms and not the liquidation fee's rate. With it that owner leaves nobody unpaid at 4.5 % (the reviewer's run). The claim is worded with the one bound |
| N2 (medium): two levels are one for a set-off book | The levels differed by the tolerance of the net; with a net of nothing they are one, and a shard drawn to its level is called at a hair, the capped fee being all it held | Two levels for the gross as well (the upper counts it the tolerance more); the fees of a call are a share of what is left when the users are paid |

Also taken from it: a call is stated over anything the shard carries but another call; a draw of nothing is no
statement; the note of every leaving, gain or loss, ends a statement on the shard that it landed before and is
not applied in; whoever watches is not asked to wait a dispute period a leg at a call (the steps of a call are
stated together, one statement an account).

**One thing of the goal's wording is given up, and why.** `GOAL.md` says of the bond that many accounts refer
to it and touch it only at its default. Here an entry touches its shard once, in the transaction that applies
it. The reason is N1: where taking a leg and booking it are two acts, there is a time in which the leg is in the
account and not in the books, and no rule for that time is both safe for the shard's users and fair to the leg's
(paid for neither way, the user cannot hedge with it; paid for both ways, it is F61 again). What the goal's line
is for is kept: no trade waits on the clearing party after it signed (anyone applies an entry), a shard is one
clearing party's and not everybody's, and nothing but an entry touches it short of a default (a leaving, a take
and a step leave notes).

### The form

Correct before it is cheap. Futures legs only; no gain taken out without leaving; no leg moved between
accounts. An account that no shard covers keeps everything it has (two parties' margins, entries that move with
a statement, options).

**A covered account**: its user's margin and its legs. Nothing of the clearing side. A leg's entry is the price
it entered at and stays so. What its legs gained or lost counts for the user's standing (the trace) and moves no
coin until a leg leaves.

**A shard**: what it holds; four rows of net and gross units; what its legs were entered at (the sum of side
B's units times their entries: one number, where a position has an ask); whether it is called. Equity at prices:
what it holds, and its net units' worth, less that number. Requirement: the maintenance rate of the net's worth
and the rate on the gross of the gross's; the upper level counts both the tolerance more, the lower the
tolerance less.

| Statement on a covered account | Rule | Applied |
|---|---|---|
| A leg enters (kind 6; `KpfTEnter` exists) | as built, and it states a price for each of the shard's rows (answered as a draw's are). The user stands at its maintenance with the leg | **with the shard**: its equity at the prices stated, with the leg, is at least the upper level; the rows and the number take the leg. Else the entry ends |
| The user takes margin out (kind 2) | of its own coins, standing at its floor after | as today |
| A leg leaves by the user's statement (kind 4) | the user stands at its maintenance, or the leg lost. The leg is settled alone, from its entry to the price stated | as today; leaves the note that it left (the leg whole, the user's key, what the shard is asked: its gain beyond the band) and, where it lost, the deposit of what it lost, out of the user's margin |
| A step of the user's liquidation (kind 3, side A) | the user is below its maintenance; the leg is its largest, or it stands at its maintenance after. Stated over a statement that is none | the same, the fee out of the user's margin |
| The steps of a call (kind 3, side B) | stated with the shard's call shown; every leg of the account leaves in one statement | the same; the fee is a note the shard pays after its users |

| Statement on a shard | Rule | Writes |
|---|---|---|
| A draw (kind 5; exists) | leaves the equity at the upper level, and is of coins; not while called; not of nothing | as today |
| A call (new) | the equity is below the lower level at any price within the tolerance; anyone, with a bond; stated over a draw | the shard called, and the call: an object that the steps of a call show |
| The call ends (new) | no row has a unit, or the equity is at the upper level again | the shard not called |

| Note to a shard | Does |
|---|---|
| A leg left | takes the leg out of the rows and of the number (by its units and its entry, which the note has from the leg); pays what it asks to the user's key: outside a call in full as far as the shard holds, in a call its share once no row has a unit |
| A deposit | adds to what it holds |
| The fee of a step under a call | paid after the users, of what is left |

A statement on the shard, and the applying of an entry, is ended by a note to the shard that landed before it
and is not applied.

**What goes** of what is built: side B's margin in a covered account and all that guarded it; the notes of an
entry and of what is owed; entries that move with a statement, in a covered account; options and the move of a
leg under a shard; `KpfTLegAdd`, and with it F61 as it was found.

**What a user does to be covered**: reads the shard (its committee, terms, periods, what it holds against what
it is asked); opens an account under it; states an entry with the clearing party and waits its dispute period;
has it applied (anyone does), and is covered from that transaction, or is not in. To leave: states, waits,
applies, and has its note applied.

**Not in the first form, and wanted:** a gain taken out without leaving; options; a leg moved between accounts;
an entry that does not wait a dispute period (a citation in place of the wait).

**Sizes are not known.** The applying of an entry carries the shard's rows and terms beside the account's.

### Review 44, and what it changes of the form above

Review 44 attacked this section on paper before any script. What round 43 asked is in (no leg is unbooked; the
rate on the gross holds against both reviewers' owners). Two things would have made the scripts wrong:

| Found | What it is | Decided |
|---|---|---|
| S2 (high): a leg settled alone, in an account whose standing counts its other legs | An account of the owner's keys holds 10,000 units each way, at its floor: its standing is its coins, the books show nothing, no step and no call is ever due. The price drifts up 34 %. The long leaves alone and is paid all the shard holds; the short pays its coins, which are less than it lost; a user owed 3,324 KAS is paid nothing | Two tests where there was one. **Standing** is of the net, as today (a gain of one leg is margin for another): it decides a step. **What leaves an account as coins** (a take; what a leg that leaves is paid) leaves the coins at the maintenance and at what the legs that stay have lost: a gain of a leg that stays is no coin. And a leaving is of one leg or of all: of all, the shard is asked the account's net, once. A step (of the user's liquidation, or under a call) lets every leg leave |
| S1 (high): an entry stated at one time and its gate asked at another | Applied a dispute period after it was stated, at the prices of then against the rows of now, by whoever decides that the gate holds: the owner, whose room it is. It applies the entries that lost for the user and lets the ones that gained end; and while the shard carries a draw nothing is applied | **An entry is stated with the shard.** The transaction that states it spends the shard: the gate is asked there, at the prices stated and the rows as they stand, and the rows and the number take the leg at once. The statement then stands on the account as any does, and the shard is not held. Where the entry is answered wrong or its disputed point lapses, the script that ends it leaves the shard a note that the leg did not enter (a leaving at its entry, asking nothing). Nobody has a time to choose in |
| S3 (medium): prices by a row's place | a row emptied and taken by another market between stating and applying | gone with S1: the rows are those of the transaction that states |
| S4 (medium): a call that cannot empty | one account nobody steps keeps a row from emptying; with fees after the users nobody is paid to step; the steps' prices were not said | A call states a price a row, answered for as a draw's. A step under a call is no statement: one transaction in which the account's legs leave at the call's prices, over anything the account carries. A flat part of its fee is paid before the users. A call has a time: after it, what the legs nobody stepped would be owed at the call's prices is set aside at the same share, and the others are paid |
| S5 (medium): the arithmetic | a row's units and the number are sums with no count, and a sum beyond 64 bits could be neither applied nor ended; rounding; a row no committee prices | a row is bounded before a leg is added, and beyond the bound the gate does not hold; rounding against the owner at the upper level and for the shard at the lower; a leg is counted by one quote where it enters and where it leaves; a price of a call that nobody answers leaves the call standing |

The reviewer would build the entry otherwise: one transaction with the two signatures, the shard and the
committee's citations, which asks the gate at the price and time of the entry and holds no account. It asks the
committee at every entry and carries a citation a row (some 5 kB each). Stating with the shard gives what that
gives but the wait (an entry is final a dispute period after it is stated), with a citation only where somebody
disputes: it is the form taken. The other is kept in mind for a user who pays for no wait.

What this makes of a covered account: one margin (its user's coins), legs whose entries stay, and three numbers a
statement computes over them (the net gain, the worth, what the legs that stay have lost). Its scripts are not a
position's with a longer moving part any more: they are written for it.

**Order of the scripts** (the reviewer's, taken):

1. The shard's arithmetic alone: rows with the number, the equity, the two levels, the bounds, the rounding.
   Review: script against model on random books.
2. The notes of a leaving and a deposit, and the guard of a note not applied. Review: the books are the model's
   under any order of notes.
3. The covered account with one margin, the two tests, a leaving of one leg or of all, the step. Review:
   accounts of several legs.
4. The entry stated with the shard.
5. The call, the steps under it, its end.
6. What goes.

Before 3: the model with accounts of more than one leg.

### Built so far (26 November 2026; at the engine only)

**Stage 1, the shard's arithmetic** (`KpfTShardEnd`, `KpfTShardOpen`; `reserve.rs`; `tests/thin_draw.rs`). A
shard's moving part carries, where a position has its two asks, what its legs were entered at and whether it is
called. The end of a draw counts the equity (what it holds, and what its net units are worth, less that number)
against the upper level (the maintenance rate of the net's worth and the rate on the gross of the gross's, each
the tolerance more); a draw is of coins the shard holds and is not made while it is called. Run: a shard that
lost draws no sompi of the 100 KAS it holds; the most is right and a sompi more wrong, at two prices; no more than
its coins; nothing while called; and on 60 random shards the script takes the model's verdict.

**Review 45** of it (at the engine; 360 random draws, each as the arithmetic written out says):

| Finding | What it is | Now |
|---|---|---|
| F68 (high while stage 1 stands without what writes the number) | nothing writes what the legs were entered at until an entry is stated with the shard: a shard with legs of the old entry has that number at nothing, and its equity counts the whole worth of its long rows as gain. A shard of 50 KAS with a user short 10 units was drawn empty | open until stage 4 and the old entry's removal; the box at the head of 6a stands |
| F69 (medium) | a row nobody prices is equity at whatever price its owner states: the price's dispute lapsed with both bonds back | a point of a draw that nobody answers costs the claimer its bond, as a point of an entry does (`KpfTPoint`) |
| F70 (low) | a shard was opened with the bytes the engine reads as nothing in place of nothing; whoever read the numbers and wrote them again had another hash | `KpfTShardOpen` holds the bytes |
| F71 (low) | the model said wrong where the script computes an equity that does not fit and gives no verdict | the model gives none there |

To do with the call (stage 5): a short row's worth is rounded down, which is for the owner by less than a sompi
a row at the upper level.

**Stage 2, the notes of a leaving** (`KpfTLeft`, 4,503 bytes; `KpfTDrawNote`; `tests/thin_draw.rs`). One note for
legs that left, one leg or all an account has (the chain of the legs, the user's key, what the shard is asked):
applied, each leg is taken out of its row and out of what the legs were entered at, and what is asked is paid as
far as the shard holds; what it does not hold goes on as a note of the rest. Nothing is paid by it while the shard
is called. A draw is ended by either note where it landed before the draw and is not applied. Run: six legs of
three users in three markets booked by hand, three notes applied in each of their six orders, the books as they
began and the shard holding what it paid less; more asked than is held; the rest paid; a called shard; five
things that are not applied. The script is large for what it does (a loop of six legs): to be made smaller.

**Review 46** of stage 2 and of the fixes of review 45 (at the engine): no finding. F69, F70 and F71 are closed
(a draw over a row nobody prices is ended by the dispute of that price, at its claimer's cost; the three openings
with bytes that read as nothing are refused; the model and the script part on none of 360 random draws). On 240
random notes over shards of one to eight booked legs the engine takes exactly what the script's arithmetic gives,
and the model agrees on all; 41 changed transactions of one note and of a note of the rest are refused. The three
shapes of a note (a leg's or a deposit's, legs that left, the rest) differ in their third push and their length:
none is read as another. One thing is asked of stage 3: **a note that legs left is written only for legs that are
in the shard's rows** (a note of legs in no row would end every draw and could never be applied). It holds by
there being no leg in a covered account that the books do not have (stage 4), and until then by nothing writing
such an account but a test.

**The third model** (`tests/research_solvency_accounts.rs`; numbers only): accounts of up to six legs in two
markets, with the two tests, a leaving of one leg or of all, and the step. Six owners (one that only draws; the
two of the second model; a straddle, which is review 44's case; a spread across the two markets by an owner who
knows the next tick; an account that enters a second leg on what its first gained), ticks of 0.5 %, 2 % and
4.5 %, 1,000 paths each: no user is unpaid, and **no act on an account (an entry, a take, a leaving) makes what
the shard is good for fall**, to the sompi (what it is good for: its coins, less what it owes and what its
accounts gained, and what the accounts that lost can pay, each no more than its coins). With one test the
straddle's owner takes 47 KAS a path out of a shard of 500 by acts alone, and in review 44's case a user is short
39 KAS. What the second test costs users: a leg of theirs that gained was refused leaving alone up to 437 times in
1,000 paths (they then leave with all their legs).

**Stage 3, the covered account's own statements** (`KpfTCRow`, 2,060 bytes; `KpfTCEnd`, 2,700; `covered.rs`;
kinds 7 to 9 in `KpfTChange`, `KpfTPoint` and `KpfTStepPrice`; the last five tests of `tests/thin_draw.rs`).

| Kind | Who | Right when (m: the user's coins; at the prices stated) | Makes |
|---|---|---|---|
| 7, a take | the user | the coins that stay are at the maintenance and the tolerance of what all legs are worth, and at what the legs that lost have lost, and at a note's least | the account, the amount less; the amount to the user |
| 8, one leg leaves; it gained | the user | the coins are at the same of the legs that stay | the account without it; the note: its gain beyond the band (twice the tolerance of its worth) |
| 8, one leg leaves; it lost | the user | the coins pay what it lost, a note's least staying | the account without it, the loss less; the note, which holds the loss |
| 8, every leg leaves | the user | always | what the account held to the user, less what the net lost (as far as the coins go, a note's least staying); the note: the net beyond the band, or what was paid |
| 9, a step naming a leg | anyone | the coins and the net are below the maintenance less the tolerance of what all are worth. The leg lost and the coins pay its loss and its fee: it leaves. Else every leg leaves (it lost and they do not pay; or no leg lost). Naming a leg that gained while another lost is wrong | as a leaving, and the fee to whoever stated it |

What is stated is two numbers (the amount: a take's, or a step's fee; the leg that leaves, -1 for all), the chain
of the leg that leaves alone, and a trace of a row a leg (112 bytes, as a position's, with other sums: the net
gain, what all are worth, what the legs that stay are worth and have lost, what the legs that leave gained). A
dispute names a row (`KpfTCRow`), a price (`KpfTStepPrice`, as it is) or the end (`KpfTCEnd`); a point nobody
answers costs the claimer its bond.

**Changed against the form above, and why.** There is no deposit beside the note of a leaving: **the note that
legs left asks below zero where the account pays, and holds those coins** (`KpfTLeft` adds them to what the shard
holds). With two notes there was an order in which the coins were in and the leg still in the books, its loss
counted twice; with one there is none. And a note's carrier is not the user's: whoever applies the statement puts
it in, and whoever applies the note has it.

Run at the engine: a take to the sompi (a sompi more, and another key, are answered wrong); the five leavings
above, each note applied to a shard and the books compared; the lie the second test is for (a leg that gained
leaves alone from coins that do not hold the other) answered wrong; the step in its three outcomes; five rows that
are not the rule's; a price within and beyond the tolerance; a lapse; and 57 random statements on accounts of one
to four legs, each answered right, and wrong with a sompi more. Sizes: a statement on an account of two legs is a
transaction of 2,952 bytes, its applying 2,602; the applying of a note 6,545 (`KpfTLeft` is 4,532: too large).

Not yet: nothing enters a covered account in this form but a test (stage 4), so F68 and the requirement of
review 46 stand; a statement is not yet stated over another (review 42's M1: a step over a statement that is
none); kinds 2 to 4 still serve a covered account of the old form (they go at stage 6).

**Review 47** of stage 3 (at the engine) and of the paper below. The two scripts take their model's verdict on
180 statements and 180 rows, sums at the 64-bit ends among them, and every note a statement of the rule's writes
on an account of up to six legs is applied. Over 21,829 statements on 10,000 accounts no take and no leaving
makes what the shard is good for fall. Found, and what was done:

| Finding | What it is | Now |
|---|---|---|
| F72 (high) | a note of more than six legs was applied by nothing: the legs stayed in the books for good, and the note ended every draw | **a note is applied a leg at a time**, the last of its chain first; with its last leg what it asks is settled. Of any number of legs. `KpfTLeft` is 2,278 bytes (it was 4,532) |
| F73 (medium) | a step that took one leg took its fee before what the legs that stayed had lost | one leg only where the coins pay what it lost, its fee, and what the legs that stay have lost; else every leg, the loss first |
| F75 (medium) | a note's least stays with the user, and no test counted it | every test counts the user's coins less a note's least |
| F74 (medium) | one sompi of side B's makes every statement of kinds 7 to 9 wrong | with what a covered account refuses (section 14) |
| F68, a second path | a leg of the old entry that leaves the new way takes out of the number an entry that was never in it | the old entry goes with the new one's coming (section 14) |

The paper of stage 4 below stands corrected by section 14 (P1: a mark where a position has an ask is read by
nothing, and every script took the marked account; P2: nothing bounded the time a leg is in the books and has not
entered; P3: a point beyond the trace is answered right by the row's script; P5: the wait before a leg that did
not enter is taken out is needed by nothing).

### Stage 4 on paper: an entry stated with the shard (26 November 2026; nothing built; superseded by section 14)

**The transaction that states an entry** (one script, `KpfTEnterS`) spends the account and its shard, with the
user's and the clearing party's signatures. What is stated: the coins the user adds; the leg, whole; the row of
the shard it takes; a price for each of the shard's four rows; and the account's trace with the leg as its last
row. The script computes, there:

- the leg is a futures leg within its bounds, its entry the price of its own row and of the shard's row it takes;
- the shard's row is its market's (or no market's, and no other row is its market's); the row takes the leg's
  units and the number takes its units times its entry, each within a bound;
- the gate: the shard is not called, and its equity at the four prices stated, with the leg, is at least the
  upper level.

It writes the shard with the leg in its books, and the statement, on the account **marked**: the account with
the leg at the end of its chain, the coins added, and a mark in its moving part (where a position has an ask)
that says the last leg has not entered. Nothing that exists is shown a marked account and takes it: it carries
no take, no leaving, no step.

| The statement then | By | The account | The shard's books |
|---|---|---|---|
| is applied after its dispute period | `KpfTChangeSettle`, as it is | the account with the leg, unmarked (what the statement's outputs say) | as they are |
| is answered wrong, lapses, or is late | the scripts that end a statement, as they are | the marked account (what the statement was stated on) | still have the leg |
| ... and is undone | `KpfTEnterOff`, by anyone | the account without the leg, unmarked, the coins added staying | a note that the leg did not enter: applied (no sooner than a dispute period of the shard after it is written), it takes the leg out at its entry and pays nothing |

So no script that ends a statement is changed, and none needs the shard.

**What is disputed** of an entry, a point at a time: the rows of the account's trace and their prices (as a
covered account's are); its end (the user's coins with what is added, and what its legs gained or lost, are at
the maintenance and the tolerance of what all legs with the new one are worth; the outputs are the account with
the leg, unmarked); and the four prices stated for the shard's rows (as a draw's are: a row no committee prices
is its claimers' to show). The gate itself is not disputed: it was computed.

**A leg that is in the books and may yet not have entered** is the price of asking the gate when the entry is
stated. The clearing party signs every entry, so it chooses what is pending. What it could do with one of its
own:

| It could | Held by |
|---|---|
| draw against what a pending leg lost, then have the entry ended: the loss was never owed | **a draw is ended by an entry that is not settled**: by showing, under the shard, a statement of an entry that stands, a marked account, or a note that a leg did not enter (whenever it landed; it cannot be applied before a dispute period of the shard has passed, so that it outlives any draw stated before it). Whoever draws does so when no entry of its shard is pending |
| let users in on what a pending leg lost (the gate counts it), and end the entry after | not held by a script: the shard is then below its upper level, and a call is due below its lower. A user reads the shard and what is pending on it before it signs |
| hold an account of its own under an entry it disputes again and again, so that it is not stepped | not held yet (review 42's M1: a step is stated over a statement); with stage 5 |
| gain by ending the entry of a leg that gained | nothing: a leg that did not enter is owed nothing, and its account lost nothing |

**Scripts of stage 4**: `KpfTEnterS`; the end of an entry (`KpfTCEnter`), its rows by `KpfTCRow` and their
prices by `KpfTStepPrice` (kind 6 of the new form is another kind, 10, so that nothing of the old entry reads
it); the shard's four prices by a script made of `KpfTShardPrice`; `KpfTEnterOff`; the note that a leg did not
enter and its applying; the three ways an entry not settled ends a draw. Then the opening of a covered account
with no leg, and `KpfTLegAdd` and the old entry go (stage 6), which closes F68 and gives review 46 what it asked.

## 14. The rest of the form, whole, on paper (26 November 2026; third writing, after reviews 48 and 49; nothing of it built)

What is built and reviewed: the shard's arithmetic, the note that legs left, a covered account's take, leaving
and step (section 13, stages 1 to 3; committed). What follows is everything else, as one design, to be reviewed
as one before any of it is written: its parts lean on one another.

**Review 48** verified the fixes of review 47 at the engine (F72, F73, F75 closed; the whole workspace passes:
1,268 tests) and attacked the first writing of this section. Found at the engine, F76: a note applied a leg at a
time shows, between two legs, an equity that is neither the one before nor the one after (a long and a short of
1,000 units from 10, the price at 15: 2,000 KAS before and after, 7,000 between), and a gate or a call would
read it. Found on paper, fourteen things; the five that were high, and what this writing does with them:

| Found | What it is | Here |
|---|---|---|
| Q1 | "one right answer makes it final": a claimer disputes a right point of its own at once, answers it, and no watcher can any more name a price and leave the showing to the claimer. A price no committee answers goes through (F69 undone) | 14.1: no finality by an answer; a bound by a last time to dispute |
| Q4 | an entry both signed is an option for whoever planted a point: the user cannot check the shard's four prices; the owner disputes one when the leg gained | 14.3: no price of the shard's is said at an entry (review 49's cut); a price nobody answers ends an entry at nobody's cost; each tool signs only well within the tolerance, on either side. What is left is said there |
| Q7 | a step stated over a statement ended it, right or not: a wrong step cancelled a right leaving for a bond | 14.2: no step over a statement; an account says a time before which its user states nothing again |
| Q8 | a call's price that nobody answers left the call standing, and every account was stepped at it | 14.6: a call's prices are the caller's to show |
| Q9 | an account of more than six legs under a call was stepped by nobody, and only its user ended the call | 14.2: six legs an account, counted |

**Review 49** read the second writing: nine of its thirteen findings closed as written, the others in substance,
and fourteen new things (R1 to R14), four of them not to be gone past (R3, R5: who pays where an entry lapses or
lands late; R6: `KpfTOpenC` asks nothing of the places the count and the time are kept in, run at the engine;
R7: the time written left nothing free). It can be finished, the review says, and is not yet for strangers. It
would cut one thing more, and this writing does: **nobody disputes the shard's prices at an entry, and no gate
is computed there.** With the two signers choosing the prices, a gate computed on them held nothing; disputed, it
cost a script, a second bond, three ways out and a lapse of its own, and bounded a leg that should not have been
let in to a statement's life, which a call does as well. What holds a shard to its requirement is the draw (the
upper level) and the call (the lower), each at prices that are disputed. What a user has at its entry is what it
reads of the shard before it signs.

**A form considered and measured, and not taken.** A statement that says only its prices, anybody correcting a
price with the committee's, and the rule computed once, when it is applied (no trace, no point of a row or of
an end): it would end the planted point and let the band be small. A script that computes a covered account's
three kinds over legs shown whole is 3,539 bytes at two legs, 4,739 at four, 5,939 at six (600 bytes a leg);
an operation would be some 10 kB where it is 5.5 kB today. Not taken. Nor an entry made final when it is
stated, by the committee's prices shown with it: a citation is 4.4 kB a price, and the gate reads up to four
rows; it waits for a committee (or miners) that attests many prices at once.

### 14.1 The life of a statement

Today a point answered right lets the statement stand again, and it can be disputed again without end. For
every statement answered a point at a time (kinds 2 to 11):

1. **A point is disputed only by a dispute that lands by the statement's time, its lateness and one dispute
   period** (`t0 + late + window`). A script cannot hold a transaction to "no later than": the answer and the
   lapse read when the dispute landed, and one that landed later is dismissed by anyone, at its disputer's cost.
2. **A right answer is taken at once, from anyone.** Of the disputer's bond a quarter is the claimer's, a
   quarter whoever answered's, and half stays in the script's copy (a claimer that answers its own other key's
   dispute loses half a bond a round: review 49, R1). Whoever answered may name the next point in the same
   transaction, with a bond of its own.
3. A statement answered right stands again, as today, and keeps its lateness (it is written beside the mark
   that it stands again: the last time to dispute is counted from it). It is applied once it stood a dispute
   period, or at once after the last time to dispute.
4. A point nobody answers within a dispute period lapses as today: for kinds 5 and up at the claimer's cost
   (its prices are the claimer's to show).

So a statement lives no longer than its lateness and two dispute periods from its time. A claimer that
disputes points of its own to keep others out pays for every round, and whoever answers it may put the point
it wants looked at in the same transaction. Outside the scripts: a price the committee signed for one is had
by anyone at once (a citation only the claimer holds would keep a watcher from answering).

### 14.2 A covered account

- It is opened with no leg, its user's coins, nothing of a second party's (side B's margin is refused at the
  opening and after: F74), no leg counted and no time (the opening holds the bytes of both places: review 49,
  R6).
- **Six legs at most.** The moving part counts them, where a position has its first ask: an entry counts one
  more and refuses a seventh; a leg that leaves alone, one less.
- An account of no leg takes a take (stated with no row) and an entry. The leaving of its only leg leaves it
  with no leg; the leaving of every leg (-1) ends it and pays its user; `KpfTAcctEnd` ends an account of no leg.
- **The account's time.** A take, a leaving and an entry write, in the account they leave (where a position has
  its second ask), a time before which its user states nothing again: its own time and **twice** the longest
  a statement lives (the lateness and two dispute periods, twice: review 49, R7). A statement of the user's
  stated before the time the account says has a wrong end. A step is held to no such time: between two
  statements of its user's an account is free for a step for at least as long as it was held (review 42's M1). A statement that is ended writes no time: stating again at once costs a bond each
  time.
- **A note is applied whole** (F76): `KpfTLeft` for a note of one leg (the common one: 2.3 kB), a second script
  for two to six. No state of the books between two legs.
- From the transaction in which the new entry exists, **nothing of the old form takes a covered account**:
  kinds 2 to 4 and their answers, `KpfTEnter` and `KpfTStepEnter`, `KpfTLegAdd`, `KpfTLegOut`, `KpfTLegMove`;
  `KpfTNoteApply` for the notes of a leg and of what is owed (a deposit stays). An account no shard covers keeps
  all of it. This closes F68.

### 14.3 An entry (kind 10)

**Stated** by `KpfTEnterS`, in a transaction that spends the account (under no statement, at or after its
time) and its shard (not called), signed by the shard's owner, who is the account's second key, and by the
user. **The owner signs first; the user signs last and lands it**: a statement that lands late is its user's
doing, and costs the user its bond as any late statement does (review 49, R5). What is said: the coins the user
adds; the shard's row the leg takes; the leg, whole; and the account's trace with the leg as its last row.

Computed there, not disputed after: the leg is a futures leg within its bounds, **of the shard's committee**
(the shard's id is the hash of its opening, of its committee, and of the terms and periods of its accounts: the
opening is shown; review 51, F80); the row is its market's (or no market's, and no other row is its market's);
the row and the number take the leg, within bounds; the account has fewer than six legs. No price of the shard's
is said and no gate is computed (the head of this section). **It writes the shard's time** (14.5): the later of
what the shard says and the entry's time, its lateness and two dispute periods.

Written: the shard, with the leg in its books; and the statement, on the account **marked**: the leg at the end
of its chain, the coins added, one leg more counted, its moving part a push longer (180 bytes: every script that
takes an account asks for 171 or 138 and refuses it; review 48 ran that).

**Disputed**, a point at a time, as a covered account's statements are: a row of the trace (`KpfTCRow`; the last
row is of the leg said, at its entry), a row's price (`KpfTStepPrice`; the last row's is the leg's own), the end
(`KpfTCEnter`: the user's coins with what is added, less a note's least, and what all legs gained or lost, are
at the maintenance and the tolerance of what all with the new leg are worth; the outputs are the account with
the leg, unmarked, with its time). A point answered wrong ends the entry at the user's cost: the trace is the
user's, and the owner's tool reads it before it signs. **A price nobody answers ends the entry at nobody's
cost** (both bonds back): an entry is two parties' trade, and with a committee that answers, whoever wants the
entry to stand answers and has its part of the disputer's bond.

**Applied** by `KpfTChangeSettle` as it is. **Ended** by the scripts that end a statement as they are: the
marked account again. **Undone** then by `KpfTEnterOff`, by anyone, for the terms' least fee of a keeper out of
the account's coins: the account without the leg, unmarked, one leg fewer, the coins added staying; and the note
that the leg did not enter (a note that a leg left, asking nothing, of a kind of its own): applied, it takes the
leg out at its entry.

Each signer's tool signs only where every price of the trace, the leg's own first, is well within the tolerance
of what it sees, **on either side** (a price off in one's own favour is a point the other can have answered
wrong when it suits). What is left of review 48's Q4: while a committee does not answer, either signer ends any
entry by disputing a price, at no cost, when the leg went against it. Left for later, and what it would buy: a
price of the leg's own that is answered wrong corrected to the committee's (the account written with the leg at
that entry, a note of the difference to the shard's number) in place of ending the entry; with it the band
need not be twice the tolerance.

### 14.4 A leg that is in the books and has not entered

An entry is in the shard's books from its statement and in the account from its settlement. Between the two the
books may be wrong by that leg, if the entry ends. It ends where it was stated wrong and answered so, where it
landed late, or where a price of it is answered by nobody.

| With such a leg | Held by |
|---|---|
| the owner draws on a requirement the leg sets off, or on what the leg lost, and has the entry ended | **no draw is stated before an entry is settled or ended**: the entry wrote the shard's time. And **a draw is ended by an entry that ended and is not undone**: by showing, under the shard, a marked account or a note that a leg did not enter, whenever it landed. (A statement that stands is not shown: written again it would land again, late, and be ended at its user's cost: review 51, F82.) |
| a call is hidden: the leg sets its users' gains off in the books for as long as it is there (in numbers: a user long 1,000 units, the price up 2 % a tick for five ticks; the leg kept five ticks: no call, the user short 155 KAS; kept one tick, or the price up 0.9 % a tick: nothing) | 14.1: the entry is settled or ended within its lateness and two dispute periods; it is then undone, and its note applied, by whoever watches (each is paid: the fee of 14.3, the note's carrier). **The bound of the claim is on what a price moves in a lateness, two dispute periods and the time between two visits** |
| a call is stated on a loss the leg shows and that is not owed (a pending leg that gained), and stands when the leg is undone | not held: the call was right on the books. Its accounts are closed at the call's prices and paid (review 49, R10) |
| its account is not stepped | the same bound |
| either signer gains by having the entry end when the leg went against it | a wrong point is the user's cost; a price nobody answers is nobody's (14.3) |

If the bound does not come out as a number a market's terms can carry, the fallback is on paper (review 48):
**a pending leg counts against the shard only** (in the row's gross and in two numbers a row of pending units,
not in the net or the number; a note that it entered moves it in). It costs a note more an entry and a wider
row.

### 14.5 A shard is not held under statements

Entries and notes spend and write the shard in one transaction. A draw holds it: so a draw writes, in the shard
it leaves (where a position has its expiry, which no script reads today), **a time before which no draw is
stated again** (an entry writes that place too: 14.3) (its own time and twice the longest a statement lives: review 49, R7), and the end of a draw
stated before the time the shard says is wrong. Between two draws the shard is free for at least as long as it
was held: notes are applied, a call is stated. A draw
that is ended writes no time and costs its claimer a bond: an owner whose other key disputes a price of its own
draw that nobody answers, and lets it lapse, keeps its shard under draws at half a bond a dispute period (review
50). A call holds the shard at a bond (14.6).

### 14.6 The call

**A call is a statement on the shard** (kind 11, by anyone; `KpfTChange` as it is): a price for each row, and
that at those prices, each moved the tolerance in the shard's favour, the equity is below the lower level. Its
bond is the shard's, and no more than a hundred times the terms' least fee of a keeper. Disputed: a row's price
(**the caller's to show**: a price nobody answers ends the call at the caller's cost) or the end. Applied, it
writes the shard **called** (its time) and **the call**, an object of its own under the shard's id (the
shard's id, the call's time, the rows' names, the prices), and pays the caller its bond and the terms' least
fee of a keeper out of what the shard holds.

**While a shard is called** nothing enters and nothing is drawn. A note that asks something is not paid: applied
(whole), what it asks is added to **what the shard owes by claims** (a number, where a position has side B's
margin) and it goes on as a claim (the key, the amount); a note of the rest likewise. A note that pays the
shard is applied as always.

**A step under a call** is no statement: one transaction, by anyone, that is shown the call (spent and written
again as it is) and spends an account as it stands, or under a statement of kinds 7 to 9 that landed at or
after the call's time, standing or disputed (it is ended, its bonds back). A statement that landed before the
call's time stands: it is answered and applied as any, and the account is stepped after. Every leg leaves at
the call's price for its row. In this order: what the account lost, to the shard, out of its coins as far as
they go, a note's least staying; then the terms' least fee of a keeper to whoever stepped, out of what is left
and, where that is short, out of the note's least (every account with a leg holds one; a fee is no more than
it: no claim of a fee on the shard, which accounts of dust would take ahead of every user: review 49, R11); the
rest to the user. An account marked is first undone (`KpfTEnterOff`); a statement of kind 10 that landed at or
after the call's time is ended by showing the call, and one that landed before it stands, as any statement
(R9); no entry is stated on a called shard, so no account is marked again.

**Claims are paid at one share, once no row has a unit**: a claim of `a` is paid `a` times what the shard holds
over what it owes by claims, both as they stand, rounded down (the two fall together). Every account has six
legs at most and is stepped by anyone, so the rows empty without any user's leave.

**A call ends** where no row has a unit and nothing is owed by claims: the shard is not called, and the call is
spent. (An owner that adds coins to a called shard raises the share; it does not end the call.)

**Fixed where it was written as scripts** (26 November 2026; decided by reasoning back from the form above; each
held against the engine in 14.10 (c)):

- *What a call says* is a draw's 40 bytes with nothing where the amount is, so that a row's price of either is
  answered by one script (`KpfTShardPrice`, told the kind).
- *Its bond* is the shard's, no more than a hundred of the terms' least fee of a keeper, **and no less than a
  note's least**: the shard's bond is a rate of what it holds, and a shard that holds nothing would be called by
  nobody.
- *The fee is paid whole or not at all*: where the shard holds less than the fee the caller has its bond back
  and no fee (a part of a fee is no payable amount, and a statement that cannot be applied would hold the shard).
- *The call's coins* are half of what the shard's output holds beside what the shard holds (0.5 KAS of its
  carrier), and return when the call ends: nothing of what users are owed is in them.
- *No note that is not applied ends a call*, as one ends a draw: an owner writes notes at will (a leaving of an
  account of its own), and a call could then never be applied. The owner keeps its shard's notes applied; a
  shard called because a note that paid it was not applied is called.
- *"Of the call's time or after"* (review 54, F90): a statement is ended by a step where the time it says is at
  or after the call's; or where it stands as it was stated and its output landed at or after it (the DAA score
  of the output a script spends). One that stands again, or is disputed, is an output no older than the answer
  or the dispute, and is held to the time it says: a statement of before the call stands whoever disputes it
  afterwards.
- *No band under a call*: every leg leaves at the call's one price for its row, for every account alike, and
  what the accounts gained is paid at one share.
- *A claim is the note of the rest*, and no kind of its own (review 54, F94): whatever a shard does not pay of
  what a note asks is counted in what it owes when the note of the rest is written, called or not, and uncounted
  as it is paid. A called shard pays none in full and each at one share, in whatever order they are shown.
- *The share* of a claim of `a` is `a` times what the shard holds over what it owes, rounded down, and no more
  than `a`. A script's numbers are of 64 bits, so above 3,000,000,000 sompi owed both are counted in a unit of a
  thousand, a million, ... sompi (what is owed rounded up, what is held down): a claim is paid at most 7 parts in
  ten million less than its exact share, never more, and what is left of that is the shard's when the call ends.
- *What a block takes decides the payments of a step.* An output weighs as the inverse of what it holds, and
  one under a covenant four times a plain one: the call and a note, each at a note's least, were four fifths of a
  block, and no step fitted. So the call holds 0.5 KAS; a step has **no output of the fee** (what the
  transaction leaves is whoever steps's, who funds it); **a payment of less than a note's least is not asked**
  (it is whoever steps's: an account that cannot be stepped would keep the rows from emptying, and every claim
  from being paid); and the bond of a statement the user itself stated is paid with the user's coins, in one
  output. A user whose account is left with less than 0.2 KAS after what it lost and the fee is paid nothing.
- *The note's least of a step is the account's own* (review 54, F93): the note's least that stays in an account
  while it has a leg becomes the least of the note the step writes, so that whoever steps adds nothing and has
  the fee; whoever applies the note, or pays what it asks, has that least. A user stepped under a call is the
  poorer by the fee and by a note's least (0.3 KAS with the terms in use), and has the least back where it
  applies its own note and is paid its own share.
- *A step ends a statement at nobody's cost but for bonds too small to pay* (review 54, F92): a bond of less
  than a note's least is whoever steps's, as every such payment is.
- *An account's output that holds less than the account says* pays what it holds (review 54, F89): its legs
  leave the books all the same.

### 14.7 What rests on whoever watches, and what pays for it

| Whoever watches | Is paid |
|---|---|
| disputes a wrong statement within its period; answers a right point | half the bond lost |
| applies an entry that stands | nothing: its user and its owner each want it applied |
| undoes an entry that ended; applies a note | a keeper's fee from the account; the note's carrier |
| ends a draw stated with an entry pending or a note not applied | half the bond lost |
| states a call that is due; steps every account of a called shard | a keeper's fee each |

Not paid for, and nobody's to enforce: that a user reads a shard and what is pending on it before it signs
(nothing is computed of the shard at an entry). A statement's prices are of a time up to the lateness before
it landed, which whoever states picks, and an entry is landed by its user: that is worth what a price moves in
a lateness, and the terms keep it short.

Rules of a tool, held by no script: whoever watches answers a price of an entry wrong with the committee's
price where it has one, and does not wait for the lapse (a lapse costs nobody: two who are one try a false entry
for nothing). The owner, who signs an entry first, spends in what it signs an output of its own, and spends it
elsewhere where the entry has not landed: its signature is else a leg the user can put into the books at any
time within the lateness.

Said plainly to a user, before any coin of a stranger's: the band (14.9); that a call closes every account of
its shard at prices of the last lateness, and that a call can be right on the books by a leg that then does not
enter; that a committee that does not answer lets either party of an entry end it.

### 14.8 What goes, the order, and what is cut from the first form

Goes, once nothing reaches it: side B's margin in a covered account and what guarded it; the notes of a leg
and of what is owed and their applying; `KpfTLegAdd`, `KpfTEnter`, `KpfTStepEnter`; kinds 2 to 4, options and
the move of a leg under a shard; their commands.

Order of writing, each reviewed at the engine: (a) 14.1 (it rewrites the end of every answer once) and 14.2
with the two times, the opening and the second script of a note; (b) 14.3, the first row of 14.4, and 14.5;
(c) 14.6; (d) what goes; then the commands, and what an Executor does for accounts and shards.

Cut from the first form: accounts of more than six legs; a step stated over a statement; claims paid before a
called shard's rows are empty; a gate at an entry and the dispute of a shard's prices there; a price that is
corrected in place of ending (14.3); an entry final when it is stated.

### 14.9 Not known, and for the owner of the design to decide

- **Sizes.** The transaction that states an entry carries the account's and the shard's parts and two
  signatures: some 6 kB by what the parts weigh. The script of a note of two to six legs is 4.5 kB.
- **The bound as a number** (review 49; arithmetic on a rate, the model's tick being a visit). With the
  tolerance (0.5 %) and the maintenance (5 %) in use the claim needs a price to move less than 4.5 % in a
  lateness, two dispute periods and a visit. In standard moves of a coin at 100 % a year, and at 300 % (a bad
  day): a dispute period of 5 s (the tests'): 50 and 17; of 20 s: 34 and 11; of 60 s: 22 and 7; of 300 s: 10
  and 3.4. Prices jump: six to eight standard moves of a bad day are wanted, which is **a dispute period of 45
  to 90 seconds with a lateness of 10**. At five minutes it fails about one bad day in ten, unless the
  maintenance is doubled.
- **The band** (review 48, Q14: no hole; a decision). Every leg that leaves at a gain is paid the gain less
  twice the tolerance of what it is worth: a long of 1,000 KAS on 55 KAS of coins, up 2 %, has 9.8 of its 20;
  up 1 %, nothing; down 1 %, it pays 10. It is what keeps a shard whole against prices stated the tolerance off
  where a leg entered and where it leaves. It is a term of the shard (the tolerance), and a smaller tolerance
  makes it smaller and makes an honest price wrong more often; the correction of 14.3, left for later, is what
  would let it be small without that.

### 14.10 Built of it (26 November 2026; at the engine only)

**(a) 14.1 and 14.2.** The life of a statement: the end of every answer pays a quarter of the disputer's bond to
the claimer and a quarter to whoever answered, who is anyone; a statement that stands again keeps its lateness
beside its mark; `KpfTChangeSettle` applies at once a statement written at or after the last time to dispute;
`KpfTPoint.lapse` takes only a dispute that landed by that time, and `KpfTPoint.dismiss` dismisses one that
landed after (`KpfTPoint` is 2,499 bytes; it was 1,487). The answerer does not name the next point in the same
transaction: a claimer that answered its own key's dispute could then keep everybody out with no gap; as it is,
the statement stands undisputed between two rounds and anyone's dispute may land.

A covered account: `KpfTOpenC` holds the count of legs and the time to nothing; `KpfTCEnd` (3,145 bytes) holds a
take and a leaving to the account's time and writes the next one (its own time and twice the lateness and two
dispute periods), counts a leg less where one leaves alone, takes a take of an account of no leg, and leaves an
account of no leg where its only leg leaves alone. A note is applied whole: `KpfTLeft` (one leg; 2,069 bytes)
and `KpfTLeftAll` (two to six; 3,767). Not yet: side B's margin refused, and the other refusals of 14.2 (with
the entry, in (b)).

**Review 50** of (a), at the engine: it does what is said above; the scripts take their model's verdict on 180
ends, 180 rows and 240 notes; the whole workspace passes (1,275 tests). Found: F77 (medium), a right answer's two
quarters fit in no transaction where the bond is under 0.2 KAS (two outputs of less than 0.05 KAS): **a quarter
each only where a quarter is 0.05 KAS or more; else half to the claimer in one output, as before**. F79 (medium),
a lateness of the mark or more made a statement applied at once with no dispute: **no statement is stated on an
object with such a lateness**. F78 (low), `KpfTHand` reads the account's time as a least margin (with what a
covered account refuses). And a dispute is not made of a statement written at or after the last time to dispute.

**(b) 14.3, the first row of 14.4, and 14.5** (the last four tests of `tests/thin_draw.rs`).

| Script | Bytes | What |
|---|---|---|
| `KpfTEnterS` | 2,130 | an entry is stated: the account and its shard are spent; the shard is written with the leg in its books, and the statement on the account marked |
| `KpfTCEnter` | 1,664 | the end of an entry |
| `KpfTCRow`, `KpfTStepPrice`, `KpfTPoint` | 2,239; 1,698; 2,582 | take kind 10 (the last row of an entry is of the leg said, at its entry; a marked account's moving part; a price of an entry that nobody answers costs nobody) |
| `KpfTEnterOff` | 770 | an entry that ended is undone: the account without the leg, and the note that the leg did not enter |
| `KpfTLeft` | 2,104 | takes that note (kind 8, asking nothing) |
| `KpfTDrawEntry` | 1,370 | a draw is ended by an entry that is not settled: its statement, standing or disputed; a marked account; the note |
| `KpfTShardEnd` | 3,162 | a draw is stated at or after the time its shard says, and writes the next |

Sizes of the transactions: an entry stated, 4,590 bytes; applied, 2,514; undone, 2,168; its note applied, 4,002.

Run: an entry into an account of no leg, stated, applied, and each kind of point answered right and wrong; a
price nobody answers (the marked account, both bonds back); the entry undone and its note applied (the books as
they were); what the stating refuses (a seventh leg, before the account's time, a count below nothing, a called
shard, a row of another market, a second row for a market, a leg that is none); a user that does not stand
(answered wrong); a second leg entering on what the first gained; a draw ended by each of the four forms of an
entry that is not settled, disputed or not; a draw's time written, and a draw before it answered wrong.

What a covered account refuses of the old form (14.2) came with the removal of that form, before the call:
(d) below.

**The library is at what one transaction writes.** Its genesis writes every copy of every script in one
transaction, and a block takes 500,000 grams of storage mass: an output of 10 KAS weighs 4,000, so 125 of them.
With 63 scripts, two copies at 10 KAS (126 outputs) weigh 504,002 and are refused, and one copy at 5 KAS as
well; the tests that made such libraries now give a copy 11 and 6 KAS. Twice as many coins a copy, or half as
many copies, is the price of every script added; the old form's scripts going will take some off.

**Review 51** of (b), at the engine: the end and the rows of an entry take their model's verdict on 200 random
statements; thirteen things the stating refuses; F77 and F79 closed. Found, and done:

| Finding | What it is | Now |
|---|---|---|
| F80 (high) | a leg of another committee entered a shard's books: a row nobody but its maker prices (a call disputed at that row for ever; a draw counted at a price its owner signs) | `KpfTEnterS` is shown the shard's opening and holds the hash of it, of the leg's committee, and of the account's terms and periods to the shard's id |
| F82 (high) | an entry shown to end a draw was written again, landed again, and was late: whoever showed it ended it at its user's cost, and an owner did so on purpose for half a draw's bond | a statement is not shown. **An entry writes the shard's time**: no draw is stated before it is settled or ended. `KpfTDrawEntry` (1,138 bytes) takes a marked account or the note, which have no time |
| F81 (medium) | a marked account or a note of a leg that is in no row (a statement nobody disputed can write one) was cleared by nothing and ended every draw | `KpfTLeftVoid` (1,612 bytes): a note of legs one of which is no leg of the books is spent by anyone, the shard as it is. (Written into the two scripts that apply a note it added 1.4 kB to the one of several legs.) |
| F83 (low) | the undo of an entry on a small account fits no transaction (three small outputs) | open: a builder's matter; the script asks "at least" |

`KpfTEnterS` is 2,282 bytes, an entry stated 4,775. The command that opens a shard says no time (it wrote 864,000
where the draw's time now stands).

**Review 52**, at the engine: F80, F82 and F81 are closed. `KpfTLeftVoid` on 170 random books spends a note
exactly where a leg of it is no leg of the books, and **no note that is applied is spent** (76 such notes,
whichever leg is named). The whole workspace passes (1,291 tests). Found: F84 (low), what a spent note held goes
to whoever spends it (a note that holds what an account paid is of the books when a right statement writes it;
it is spent only after a wrong note nobody disputed took its row's units): open, to be closed by the shard
taking what such a note holds. Said of the shard's time: the owner waits a lateness and two dispute periods
after every entry before it draws, whatever became of the entry; the time an entry writes stays; and the script
holds an entry's time to no clock, so whatever signs for the owner checks it (a time far ahead would keep the
shard from drawing till then).

**(d) What went** (before (c), the call: nothing of the call rests on the old form, and every script that went
is a script the call need not be held against). The library has 57 scripts (it had 64): `KpfTLegAdd`,
`KpfTEnter`, `KpfTStepEnter`, `KpfTLegOut`, `KpfTLegMove`, `KpfTHand` and `KpfTNoteApply` are out, with their
builders, their model and their tests. What a covered account was by them, it is no more:

| Script | Now |
|---|---|
| `KpfTChange` | kinds 2 to 4 are stated on an account whose moving part is 138 bytes: no shard covers it |
| `KpfTPoint` | kind 6 is no kind |
| `KpfTOpenC` | the place of a second party's margin is the bytes of nothing |
| `KpfTAddC` | side A alone adds |
| `KpfTAcctEnd` | ends an account of no leg and nothing of a second party's; no deposit is written |
| `KpfTDrawNote` | takes a note that legs left and a note of the rest, and no other |
| `KpfTLeftVoid` | the shard holds what the note it spends held for it (review 52, F84) |

In the model: the notes of a leg entered or left, of a deposit and of what is owed are gone (`SNote`), with the
entry of kind 6; a take, a leaving or a step of kinds 2 to 4 on a covered account is no statement the model
makes or takes for right. What is kept of an object (`aobj.rs`): 22 is a note that legs left (its legs whole),
25 a note of the rest; a statement of kinds 7 to 9 on a covered account is kept and read back, and its outcome
is the account and the note its end makes. The commands: `acct-leg` and `acct-out` are gone; `acct-open` takes
no `--b-margin`; `acct-add` is the user's; `shard-note` applies a note whole by the script of its number of
legs, or pays a note of the rest; `acct-take`, `acct-leave` and `acct-liq` refuse a covered account. **The
commands of the new form (an entry, a take, a leaving, a step, the undoing of an entry) are not written**: they
come after the call, with what an Executor does.

Gone with it: an option under a shard (a row of a shard is of futures legs: `Shard::book_of`), and a leg that
leaves at once by two signatures. Both are of an account no shard covers still.

**Review 53** of (d), at the engine: F74, F78 and F68 are closed by the removal (fourteen transactions that would
give a covered account something of a second party's are refused; no script that stays reads a covered account's
count and time as coins; a leg is in the rows and in the basis by one transaction and out of both by one), and
F84 is fixed. The removal opened nothing that was found: kinds 2 to 4 are taken on 138 bytes and refused on 171,
kind 6 is stated and disputed nowhere, the scripts of a draw and of a note refuse every object of the old notes'
form, and the scripts of a covered account refuse an account of 138 bytes. The whole workspace passes (1,251
tests; it was 1,291 with the old form's). Sizes: `KpfTChange` 1,417 bytes, `KpfTPoint` 2,588, `KpfTOpenC` 672,
`KpfTAddC` 516, `KpfTAcctEnd` 682, `KpfTDrawNote` 1,159, `KpfTLeftVoid` 1,719.

| Finding | What it is | Now |
|---|---|---|
| F86 (medium) | a note that says it holds what an account paid is taken at its word: the three scripts that spend a note write the shard holding that much more and never read what the note holds. A right statement writes a note that holds it; a wrong one nobody disputed writes one that holds a note's least and says anything, and whoever clears it pays the difference into the shard, or (a number too large) nothing clears it, and it ends every draw. F81 again, for a note that asks below zero | open: the shard is to take what the note holds above its least, and no more than it says |
| F85 (low) | what went is still written in five scripts that stay, behind conditions nothing reaches (`KpfTStepEnd`, `KpfTStepLiq`, `KpfTStepRow`, `KpfTStepPrice`, `KpfTAcctEnd`) | open: to be taken out, so that what is handed on is the form and nothing else |
| F83 (low) | the undo of an entry on a small account fits no transaction | open, at the builder |

Said, and no finding: `KpfTAcctEnd` takes either party's signature alone, so the clearing party ends a user's
account of no leg at will; the coins go to the user.

**(c) 14.6, the call** (26 November 2026; at the engine). The library has 61 scripts. New:

| Script | What it does |
|---|---|
| `KpfTCallEnd` | the answer of the end of a call (kind 11): the equity, each price moved the tolerance in the shard's favour, is below the lower level; the outputs are the shard called, the call, the fee |
| `KpfTCStep` | a step under a call: every leg of an account leaves at the call's price; the account as it stands, or under a statement of kinds 7 to 9 (standing or disputed) that landed at or after the call's time |
| `KpfTCallPay` | `claim`: a claim is paid its share once no row has a unit; `off`: the call ends where nothing is owed |
| `KpfTCallEnter` | an entry (kind 10) that landed at or after the call's time is ended by showing the call |

Changed: `KpfTChange` states kind 11 on a shard, with the call's bond; `KpfTPoint` disputes it (a point nobody
answers costs the caller); `KpfTShardPrice` answers a row's price of a draw or of a call; `KpfTLeft` and
`KpfTLeftAll`, on a called shard, pay nothing and write a claim (and count it), and a note of the rest shown to
a called shard goes on as a claim. A shard has one number more (what it owes by claims, where a position has
side B's margin), nothing while it is not called.

Sizes of the transactions: a call stated, 2,913 bytes; applied, 2,736; its end answered, 5,402; a row's price
answered (one citation), 8,226. A step, 5,234 to 5,350. A note applied to a called shard (a claim written),
4,229. A claim paid, 3,454. The call ended, 3,347. An entry ended by the call, 2,628. (A step is large: it is
what happens to an account when its shard fails, and not what a user does.)

Run (`tests/thin_draw.rs`): a shard at 11 against 10 called, applied, and the shard at 10.5 not; twelve calls
that are not the rule's answered wrong (a called shard, a sompi more than the fee, no fee, another shard in the
outputs, prices other than those stated, a market with no price); a shard that holds less than the fee called
for no fee at a bond of a note's least; a call's price answered wrong and right by the committee's, and a point
nobody answers at the caller's cost; 80 random shards and prices where the script takes the model's verdict and
no other (31 right, 49 wrong). Under a call, whole: three accounts stepped (a leg that gained; one that lost
more than its account had; one under a take stated after the call, standing and disputed; a statement that
landed before the call's time not ended); their notes applied (two claims, and what the third held to the
shard); no claim paid while a row has a unit, and no call ended while a claim is owed; the two claims paid at
one share (24.7 KAS against 45 owed), the last having all that was left; the call ended and the shard not
called. On 60 random accounts and calls the step is the model's, and a note that asks a sompi more or less, a
user paid a sompi less, a note holding a sompi less, are refused. An entry that landed after the call ended by
it, disputed or not; one that landed before, not.

**Review 53's F86 is closed** before it: the three scripts that spend a note give the shard what the note's
output holds above a note's least, and no more than the note says; seven notes that say more or less than they
hold, of a leg in the books and of none, are cleared at nobody's cost (`KpfTLeft`, `KpfTLeftAll`,
`KpfTLeftVoid`). F85 (what went, still written behind conditions nothing reaches) is left for the pass that
follows the call.

Not built of 14.6: the commands; what an Executor does (to call, to step, to apply, to pay). Not decided by a
script and resting on whoever watches (14.7): that a call that is wrong is disputed. **A call nobody disputed
is the rule for every account of the shard**: its prices are what every leg leaves at.

**Review 54** of (c), at the engine. F86 is closed (60 notes that say more or less than they hold, each cleared
at no cost to whoever clears it). The call's arithmetic is its model's wherever it was pressed: the end of a
call on 344 calls (the edges: a price at its limit moved up, a price of a sompi moved down to nothing, a gross
with no net, the largest numbers, a maintenance below the tolerance), a step on 140 random accounts in the three
forms, a claim's share on 765 cases at the edges of the units (no claim above its exact share; over 300 random
sets of claims in random orders, never more paid than held). No way was found to be paid twice, or to lose what
a called shard owes or that it is called. The whole workspace passes (1,277 tests). Sizes: `KpfTCallEnd` 3,622
bytes, `KpfTCStep` 3,173, `KpfTCallPay` 1,562, `KpfTCallEnter` 1,013; `KpfTChange` 1,508, `KpfTShardPrice`
1,772, `KpfTLeft` 2,239, `KpfTLeftAll` 3,846, `KpfTLeftVoid` 1,747. Every transaction of a call at the smallest
accounts and claims is taken by a block (the heaviest, a claim of a sompi topped up to an amount that is paid:
414,346 grams of 500,000).

What it found is of whether a call reaches its end, not of its sums:

| Finding | What it is |
|---|---|
| F87 (medium) | a note whose chain nobody opens (seven legs; 32 bytes that are no chain) ends every draw and is cleared by nothing: `KpfTDrawNote` reads the chain and opens none |
| F88 (medium) | an owner keeps its shard from being called, and from every note, by statements of its own: a draw that is not the rule's, disputed and answered wrong by its second key, gives the shard back to the sompi for half a bond, and the bond is a rate of what the shard holds (0.01 KAS on a shard of 1 KAS) |
| F89 (medium) | an account whose output holds less than it must pay is stepped by nothing, its leg stays in the row, and no claim of that shard is ever paid |
| F95 (medium) | under a call a user leaves at a later price by a statement of its own, where nobody steps over it within its period; and nobody is paid to (F93) |
| F90 (low) | a statement that landed before the call's time and is disputed after it is stepped over: the script reads the time of the object it spends |
| F91 (low) | two sums of the model overflow where the scripts do not (a note that says the largest number; a keeper's fee beyond anything), and a shard of such a fee is called by nobody |
| F92 (low) | a bond under a note's least goes to whoever steps, where the paper said a step costs nobody a bond |
| F93 (low) | a step leaves whoever makes it 0.11 KAS the poorer where the user is paid: it adds the note's least, which whoever applies the note has |
| F94 (low) | a claim paid before a note of the rest is shown is paid at a share that does not count the note |
| F83 (low, open since review 51) | the undo of an entry on a small account fits no transaction: it now stands on the way to every claim of a called shard |

**After review 54** (26 November 2026; at the engine, my tests; the reviewer's are round 55):

| Finding | Closed by |
|---|---|
| F87 | what ends a draw is opened: `KpfTDrawNote` is shown the legs of a note that legs left (one to six) and holds their chain to the note; `KpfTDrawEntry` is shown the leg of a note that a leg did not enter, and of a marked account its terms, the chain before its last leg and that leg. An object nobody opens ends no draw |
| F88 | in part: a statement on a shard (a draw, a call) has a bond of a note's least at the least, whatever the shard holds. A round of a draw that is not the rule's, answered wrong by the owner's other key, costs half of that and three fees (0.13 KAS) and holds the shard for the blocks of three transactions: an hour of it costs some thousands of KAS, on a shard of any size. Not closed: that a caller's statement lands between two rounds is a race in the mempool, which no script decides |
| F89 | a step pays no more than the account's output holds, and its note says so |
| F90 | the rule of time above |
| F91 | the model's sums saturate as the scripts refuse; the cap of a call's bond is taken only where it is a number, so that terms of a fee beyond anything leave a shard that is called |
| F92 | the paper says it (above) |
| F93 | the note's least of a step is the account's own: whoever steps has the fee less what the transaction costs (0.09 KAS where the account has it) and adds nothing |
| F94 | one note of the rest, counted when it is written |
| F95 | not by a script: a leaving stated after the call is ended by a step within its period, and a step is now paid. That somebody steps rests on whoever watches (14.7) |
| F83 | the undo of an entry has no output of the fee (the account holds it less; what the transaction leaves is whoever undoes it's): the account and the note alone fit a block at the smallest account an entry leaves |

F85 stays: the pass that takes out what nothing reaches follows the commands.

**Review 55**, at the engine: F87, F89, F90, F91, F93, F94 and F83 are closed (what ends a draw is opened, and
fifteen objects nobody opens end none; an account that says 1,000 KAS is stepped whatever its output holds, from
a sompi up; the rule of time on 24 cases of the two scripts; whoever steps adds nothing and has 0.0873 KAS where
the account has the fee; two notes of the rest paid 32.07 and 12.83 KAS in either order; the undo of an entry
taken from a note's least upward, 403,764 grams of a block's 500,000). A step on 180 random accounts in the three
forms, with outputs that hold the coins, more, half, or a sompi: model, arithmetic written out and engine agree.
The whole workspace passes (1,282 tests). Sizes: `KpfTDrawNote` 1,393 bytes, `KpfTDrawEntry` 1,181, `KpfTChange`
1,543, `KpfTCStep` 3,239, `KpfTCallEnter` 1,051, `KpfTLeft` 2,197, `KpfTLeftAll` 3,825, `KpfTEnterOff` 719.

| Finding | What it is |
|---|---|
| F98 (medium) | a sompi added in every block keeps an account or a shard from every other transaction: `KpfTAddC` asks no age, and each addition costs a fee and no bond (0.0067 KAS, a twentieth of F88's round). Under a call one such user keeps a row from emptying and every claim unpaid |
| F96 (medium) | a note of the rest that was never counted ends every draw and is paid by nothing: F94's closing turned a theft into a lock |
| F97 (low) | what the rule of time leaves of F95: a statement of a block before the call's time, landed a lateness after and disputed at once by its user's other key, is not stepped, and once answered stands again |
| F99 (low) | the undo of an entry costs whoever makes it the note's least, which it has back only by applying the note too |
| F88 | closed in part as said: a round costs 0.129 KAS on a shard of 1 or 10 KAS, 5.03 on 1,000, 500 on 100,000; and F98 is cheaper |
| F95 | open by decision |

**Review 55 on section 15** (on paper): nothing of a third party's moves by a hand-over alone, and no way was
found round an account's time, the second test or a statement's bond. What it found is in 15.5.

**After review 55** (26 November 2026; at the engine, my tests; the reviewer's are round 56):

| Finding | Closed by |
|---|---|
| F98 | what a party does at will and for no bond asks that the object's output has stood a lateness: an addition (`KpfTAddC`, on an account and on a shard), and so a hand-over and a join (15). Between two such there is a lateness in which anybody else's transaction lands |
| F88, the rest of it | a draw is stated on a shard that has stood a lateness (`KpfTChange`, kind 5): between two statements of its owner's a call lands. A call asks no age |
| F96 | a note of the rest ends a draw only where the shard owes that much (`KpfTDrawNote`); and one of more than its shard owes, which was never counted, is spent by `KpfTLeft.rest` for nothing, called or not. It is paid by nothing and locks nothing |
| F85 | what went with the old form is out of the five scripts that stay: the old entry's branches (kind 6) of `KpfTStepRow` and `KpfTStepPrice`; the covered account's of `KpfTStepEnd` and `KpfTStepLiq`, which take an account of 138 bytes alone; the deposit note of `KpfTAcctEnd` |
| F97, F99 | left (low): a statement of a block before the call that its user's other key disputes at once; the undo of an entry, whose maker has the note's least back by applying the note |

## 15. In and out with no wait: an account changes hands (26 November 2026; reviewed on paper, then built at the engine: 15.5, 15.6)

Section 14 is the form that is safe whoever the clearing party is. What it costs a user is time: a leg enters by
a statement, and the account takes no other change until the statement is applied (a lateness and a dispute
period: 45 to 90 seconds by 14.9); a leg leaves by a statement, and its user is paid after the same. An exchange
answers in a second. This section is how the same form answers in a second, without a price read by any script
and without anything new that a third party must trust.

**Why a statement waits, and when nothing needs to.** A statement waits because its price is said and not read:
the wait is the time anyone has to show the committee's. A price matters to a third party only where the
shard's books change: a leg enters them (its entry is what the shard is held to) or leaves them (what it gained
is asked of the shard). **Where the books do not change no price is needed, and nothing waits.** Two things
leave the books as they are:

1. **The user's side of an account is handed to another key.** The legs stay, at their entries; the coins stay.
   What the two agree to pay each other for it is theirs alone, in plain coins, in the same transaction.
2. **Two accounts of one user under one shard become one.** The legs of both, the coins of both.

With the two, a statement is what a clearing party (or anyone who makes a market) does ahead of time, and a user
never waits for one:

| A user wants | What happens, in one transaction |
|---|---|
| in, when somebody is leaving | the leaver's account is handed to it; it pays the leaver what the two agreed |
| in, with nobody leaving | an account the clearing party entered before (its stock: accounts of one leg, long and short, of round sizes) is handed to it |
| one account for all it holds | the account it was handed is joined to its own |
| out, when somebody is entering | its account is handed to whoever enters |
| out, with nobody entering | the clearing party takes the account for what it quotes (and leaves the books by a statement, in its own time) |
| less, or the other way | its account is handed over and another is handed to it, in the one transaction: both or neither |

This is the goal's second row ("a side is handed over without the party that stays; a clearing party takes the
user's side at once") in the form of section 14, and what was asked for on 22 November: a close and an open
joined in one transaction, a statement of a price the way out when nobody is there.

### 15.1 A hand-over (`KpfTPass`)

Spends a covered account that carries no statement and no mark, by the signatures of its user and of whoever
takes it, once its output has stood a lateness; writes it again with the taker's key where the user's was.
Nothing else of it changes: its legs, their entries, its coins, its count, its time, its shard. The shard is not
shown.

- *Who is paid* is not the script's: the taker's coin and the leaver's payment are in the same transaction,
  which both sign or neither.
- *The taker signs* (review 55, N11): what it is handed can lose no more than the coins in it, but an account
  handed to a key nobody has would keep its legs in the books for good.
- *It waits a lateness after the last thing done at will* (N1; 15.10): a hand-over is made at will and for no
  bond, and one made in every block would keep the account from a step and from a call's step. The account says
  the time before which nothing is done on it at will; "in the same second" is "a lateness after the last
  addition or hand-over".
- *Who signs last has an option on the other* (N5): a hand-over says no time, and a script cannot hold "no later
  than". The taker signs first and the account's side lands it; the tool of whoever signed first moves its coin
  where the other has not landed it within a number of blocks.
- *Nothing of a third party's moves.* The shard is owed by the account what it was owed, out of the same coins;
  what the legs gain is asked of the shard by a later statement as before, for the key that then holds the
  account.
- *The time before which the user states nothing* stays as it is: a hand-over is no way round it.

What whoever takes an account checks, and no script does: that the account is the registry's (its object's id);
what its legs gained at the price it believes; and **what its shard is good for** (N2): not what it holds
against its requirement, but what it would pay if every account under it left now, with every statement, note
and entry that is pending. (A shard whose stock is a long and a short of the same size reads net nothing and an
equity above its level, while the short has lost what the long is owed: the account that lost pays no more than
its coins.) The tool computes it over every account of the shard. No script can, and a taker that does not look
is a user of that shard like any.

### 15.2 Two accounts joined (`KpfTJoinC`)

Spends two covered accounts of one user's key under one shard, neither carrying a statement or a mark and the
same in everything but their legs, coins, count and time (N8), by that key's signature; writes one: the legs of
the first then the legs of the second (six at most), the coins of both, the later of each of their two times.
**The shard is shown, and written again as it is: it is not called** (N3). Every object has stood a lateness (a
join is in no hurry: 15.10).

- *Why the shard is shown.* Under a call a gain is paid at a share and a loss in full. An account that lost,
  bought after the call and joined to one that gained, would set the one off against the other at par, out of
  what the other users are owed (a gain of 100 and a loss of 80, others owed 200, 100 held: apart the user nets
  -20 and the others have 120; joined it nets +9.1 and they have 90.9). A shard that carries a statement, a draw
  or a call, is another object: nothing is joined from the moment a call is stated. What it costs: a join waits
  its turn on the shard, and on a shard that changes in every block it waits for a quiet lateness.
- *The shard is not the worse for it but by the band* (N4). A gain of one's legs stands against a loss of the
  other's, and what is left of a loss is paid out of the coins of both. But a gain within its band, which was
  paid nothing apart, is set off against a loss: the shard receives less by at most twice the tolerance of what
  the legs whose gain is set off are worth. It is what one account that entered the same legs already has, and
  it is meant.
- *The other way (one account made two) is not here.* Legs and coins divided as their user likes leave the legs
  that lost in an account of no coins: that needs a price (a statement), or it is a hand-over of the whole.

### 15.3 What it asks of a clearing party, and what is not solved

- **Stock.** The accounts it hands out were entered by statements of its own, with its own coins in them, and
  are counted in its shard's gross (1.75 % of them, 14.9) though a long and a short of its stock are no risk to
  anybody: a stock of S each way keeps 15.5 % of S idle, 4.5 of it the gross's (review 55). (An owner's own
  account counted at no gross is not the way down: it is the owner as its own counterparty again, and a
  hand-over would have to move the books. N6.) And every entry of its stock writes the shard's time: it draws
  only after it pauses.
- **Sizes.** An account is handed over whole. A user that wants 37 units takes accounts of 20, 10, 5 and 2 and
  joins them: four legs of its six. *Legs of one market and one way made one leg* (the sum of the sizes, and an
  entry that leaves what the legs were entered at as it is) would take the count back to one. It is possible
  without the shard (N7): the sizes summed, the entry rounded against the user, the books' number off by under a
  sompi a unit, and written as nothing where the last unit of a row leaves. Not built: with the commands.
- **A user that is handed an account and wants out in the same second** hands it on: nothing waits. A user
  whose account carries a statement of its own (it chose the slow way out) waits for it.
- **Orders.** An offer of an account (a price for it, signed by its user) and an offer to take one are what
  Executors match. In the first form the two parties sign one transaction on one machine, or one signs a part
  and the other the rest, as an entry is signed today. An order as an object comes with the commands.
- **What a taker is not protected from by a script:** an account of a shard that is about to be called. A call
  steps every account at the call's price and pays what was gained at one share. The tool shows the shard's
  equity against its level; a taker that does not look is a user of that shard like any.

### 15.4 What this leaves of section 14

Everything. A statement is how legs come into being and cease (the clearing party's work, and a user's way out
when nobody takes its account), how a user takes coins out, how an account that lost is stepped, how a shard is
called. Section 15 is how accounts that exist find their users.

### 15.5 Review 55, on paper, and what was decided

| | What it found | Decided |
|---|---|---|
| N1 (high) | a hand-over or a join lands the account again at will: no bond, no time. A user below its maintenance hands its account between two of its keys in every block, and a step never finds it | each asks that the output has stood a lateness, as an addition does (F98) |
| N2 (high) | what a taker must check is what the shard is good for, over every account and everything pending | the paper says it (15.1); the tool computes it; no script can |
| N3 (medium) | a join under a call sets a loss off at par against a claim paid at a share | a join is shown the shard, which is not called (15.2) |
| N4 (medium) | "never worse" is wrong by the band | the sentence is corrected; the difference is meant |
| N5 (medium) | whoever signs first gives the other an option | the taker signs first; the first signer's tool moves its coin after a number of blocks |
| N6 (medium) | an owner's own account counted at no gross is the owner as its own counterparty | struck |
| N7 (low) | legs of one market made one is possible without the shard | with the commands |
| N8 (low) | a join must hold the two accounts equal in everything but coins, count and time | the script does |
| N9 (low) | where a wait comes back: a take; an account's time, inherited; a statement pending; a leaving when nobody quotes; a called shard; the stock running out; the owner's draw | said here: these are the waits that stay |
| N10 (low) | every hand-over spends a copy of its script, and a copy is used once a block: the copies cap the trades of a second | more copies in the library; not solved otherwise |
| N11 (low) | a hand-over to a key nobody has, or without leave | the taker signs |

Still missing for an exchange's feel (the reviewer's list, and true): a size that is not a sum of the stock's;
an order that waits; coins out with no statement; a price a user can hold the owner to; what a shard is good
for, seen in the time of a click; more trades a second than there are copies.

### 15.6 Built (26 November 2026; at the engine)

`KpfTPass` and `KpfTJoinC`; the library has 63 scripts. A hand-over is 1,714 bytes (the account, the script, the
taker's coin; no shard, no price); a join of two accounts, 3,913, of four legs and two, 4,358. Run
(`tests/thin_draw.rs`): an account handed over for 3 KAS paid in the same transaction; five ways of writing the
account otherwise refused, and the account holding a sompi less; the same transaction without the taker's key
not made; the taker states on the account and the leaver no more. Two accounts joined (the legs in a row, the
coins of both, the later time, the shard as it is); four ways of writing the one account otherwise refused, and
the shard written otherwise; no join under a called shard, of another user's account, of another shard's, or of
more than six legs.

### 15.7 Review 56, at the engine

F98, F96 and the rest of F88 are closed for what was found, and F85 is out (`KpfTStepRow` 2,693 bytes from 3,589,
`KpfTStepLiq` 2,487, `KpfTStepEnd` 1,954, `KpfTStepPrice` 1,687, `KpfTAcctEnd` 505). Section 15 as built holds
what the paper says: a hand-over refuses 18 other transactions and a join 31, and no way through either was
found. A hand-over under a called shard is taken, and nothing is wrong with it: what changes hands is a claim,
and the call's step pays the key that holds the account. N3 on what is built: a join under a called shard is
refused; one made before the call is what the paper says is meant (the others' 200 KAS are paid 119.93 apart and
90.82 joined before). `KpfTPass` is 537 bytes and `KpfTJoinC` 1,550; a hand-over weighs 203,764 grams of a
block's 500,000 at the smallest account, the largest join 103,557. The whole workspace passes (1,290 tests).

| Finding | What it is |
|---|---|
| F101 (medium) | a shard spent in every lateness is neither added to nor drawn, and a third party can spend it: the age F98 asks is of the shard's output, and a note, an entry or a join makes it young. Two keys that are not the owner's open two accounts under the shard's id and join them, alone; an owner that has the coins cannot add them while its users leave, which is when a call comes |
| F100 (medium) | an account and a shard are still landed again at will where no age is asked: a false step by the user's other key, disputed and answered wrong (0.129 KAS a round on an account of 20 KAS; 0.042 on one of 3, held a whole dispute period); the owner's entry on an account of both its keys (0.012 KAS); a note the owner made ahead (nothing) |
| F102 (low) | a wrong answer on a bond under 0.13 KAS is written by no builder: half of it is an output a block does not take |

### 15.8 After review 56 (26 November 2026; at the engine, my tests; the reviewer's are round 57)

**Where no age can be asked the bond is the price.** An age is asked of what a party does at will on an object
that nobody else spends without it: an addition to an account, a hand-over, a join, an entry. It is not asked
of a shard's addition or draw: anybody's note or join makes a shard's output young, and an owner must be able to
add while its users leave. What is left is priced:

| Finding | Closed by |
|---|---|
| F101 | a shard is added to at no age, a note's least or more at a time (`KpfTAddC`; an account's addition keeps the age). A draw is stated at no age, and its bond is a KAS at the least: a round of a draw that is not the rule's costs its owner half a KAS and three fees, on a shard of any size. A join asks that the accounts' second key is the shard's owner: two keys that are not the owner's no longer spend a shard by joining accounts that say its id |
| F100 | a covered account's statements (kinds 7 to 9) and an entry have a bond of a note's least at the least: a false step by the user's other key, disputed and answered wrong, costs 0.1 KAS and three fees a round on an account of any size. An entry asks that the account and the shard have stood a lateness: an owner's entries of its own no longer follow one another in every block (and a shard takes one entry a lateness: 15.3's stock is built at that pace). Not closed: a note the owner made ahead and applies when it likes, which is as many notes as it had legs leave |
| F102 | with the floors: half of a bond of a note's least is an amount a transaction pays. Left for an account no shard covers |

**The tests of the forms before the thin one are out of the default run** (the KOB evidence form, the oracle
form, the committee form's cross accounts: 78 files of 127). Each crate lists its tests and those are behind the
feature `legacy` (`cargo test -p kpf-tests --features legacy`); no file moved, and a test file that is added is
listed in its crate's `Cargo.toml` or it is not run. The default run is 596 tests in 60 targets (it was 1,290 in
138). The committee's own tests stay: every price of this form is the committee's.

### 15.9 Review 57, at the engine

F101, F100 and F102 are closed for what they named: a shard is added to at no age after anybody's note or join,
0.2 KAS at a time; a draw's bond is a KAS on shards of 1 to 100 KAS, and the owner's round of a draw that is not
the rule's costs 0.529 KAS (5.03 on a shard of 1,000); a join of accounts whose second key is not the shard's
owner is refused; the floors of kinds 7 to 9 and of an entry at the edge, and a false step's round at 0.1287 KAS
on accounts of 3 and 20 KAS; nothing honest breaks by a floor (a leaving on an account of 0.25 KAS is stated and
applied). The tests behind `legacy`: every file listed once, nothing of the thin form or of the committee behind
the feature, and nothing behind it the only cover of what the thin form uses. The default workspace: 600 tests in
60 targets, 210 seconds; with the features, 1,294 in 138, 851 seconds; none failed. Sizes: `KpfTAddC` 563 bytes,
`KpfTChange` 1,571, `KpfTJoinC` 1,565, `KpfTEnterS` 2,309.

What the age of an output does not hold (and 15.8 said it did):

| Finding | What it is |
|---|---|
| F103 (medium) | a shard that is spent in every lateness takes no entry and no join: both ask the shard's output age, and every note applied, addition, draw, call, entry and join makes it young. With two notes a lateness three entries in ten are taken; a third party's call that is not due, timed at the lateness, costs it 0.13 KAS a lateness of no entry |
| F104 (medium) | anyone keeps an account young for half a bond, and its user adds nothing to it: an addition asks the account's age, a false step by two keys that are not the account's lands it again at no age (0.1287 KAS a round). A key that wants a step's fee keeps the user from adding margin until the price is below its maintenance |

### 15.10 After review 57: a time written in the object (26 November 2026; at the engine, my tests; the reviewer's are round 58)

The age of an output is the wrong clock for what a party does at will: anybody makes an output young (a note
applied, a statement that is not the rule's and is answered wrong), and the party that must act in time then
cannot (15.8's premise, "an object that nobody else spends without it", held for none of its cases). The clock
is a time written in the object. A script has no "now", but an input that landed at or after a time shows that
it is that late; so:

- **A covered account says, where a position has its lease, the time before which nothing is done on it at will**
  (nothing at its opening). An addition and a hand-over show an input that landed at or after that time (a coin
  of whoever funds the transaction, or the account's own output), and write the time the account's output landed
  and a lateness. One who does such a thing in a row has two at most, then a lateness between two. Whoever lands
  the account again by a statement of its own moves no time: the user adds to its account as soon as the
  account is there (F104). What it asks of a wallet: a coin that landed after the account's last such change.
- **A shard takes an entry whose time is a lateness after the last**: what the entry writes as the shard's time
  (its own time, a lateness and two dispute periods) is a lateness or more after the time the shard says. No age
  is asked (F103): notes applied, additions and calls are nothing to an entry. An owner that states entries of
  its own in every block would have to say times ahead of the chain, and an entry's time is not ahead of the
  chain (15.12). After a draw is applied entries wait until the draw's time is a lateness behind.
- **A join keeps the age** of its three outputs: it spends the shard, is in no hurry, and a shard that changes in
  every lateness takes none (the rest of F103, left: a user holds its legs in two accounts meanwhile). The two
  accounts need not say the same time; the one has the later.
- A covered account is opened with no such time (`KpfTOpenC`), and the command says none.

### 15.11 Review 58, at the engine

F104 is closed: an addition and a hand-over show an input that landed at the account's time (a block older
refused by both scripts; an output that is in no block refused; somebody else's coin taken, since what is read is
when it landed, which nobody chooses), write the time (a block less or more refused), and after two in a row
wait; a third party's false step answered wrong costs it 0.1287 KAS and moves no time, and the user adds at once.
Nobody pushes an account's time more than a lateness ahead of the chain. F103 is closed for an entry: it is stated
at no age on a shard that just came out of a note, an addition or a call, a lateness after the last by its time
(0.99 entries a lateness whatever the notes); a join's wait is left, as 15.10 says. The default workspace: 601
tests, 169 seconds; with the features 1,295, 894 seconds; none failed. Sizes: `KpfTAddC` 618 bytes, `KpfTPass`
602, `KpfTJoinC` 1,625, `KpfTEnterS` 2,293, `KpfTOpenC` 684.

| Finding | What it is |
|---|---|
| F105 (medium) | a statement out of its time is ended by its own claimer's key, and the bond comes back: `KpfTChangeLate` gives the whole bond to the key the ender names, and nothing holds a statement's time to the chain. A step of next year on an account, a call or a draw of a time ahead, each stated and ended by one party, costs two fees (0.0144 KAS) and no bond: the floors of 15.8 are gone round. The owner's entries of times ahead (15.10) are this case: three in three blocks, and the shard refuses every entry for a lateness a block; one entry of the largest time closes a shard to entries and draws for good |
| F106 (low) | a taker is handed an account it cannot add to, or hand on, for up to a lateness: a hand-over writes when the account's output landed and a lateness. Whoever landed it last chooses that; a stock account whose entry was just applied does it with nobody meaning to. The taker's tool checks the time before it signs |

### 15.12 After review 58 (26 November 2026; at the engine, my tests; the reviewer's are round 59)

- **F105: a statement out of its time is ended for half its bond.** `KpfTChangeLate` pays whoever ends it half,
  and half stays in the script's copy (it paid the whole bond to the key the ender named, so a claimer that stated
  a time ahead and ended it by a key of its own had the object out of everybody's hands for two fees, whatever
  the floors of 15.8). A statement of a time that is not the chain's costs its claimer half its bond at the least:
  0.1 KAS on a covered account or a call, half a KAS on a draw.
- **F105: an entry's time is not ahead of the chain.** `KpfTEnterS` is shown an input that landed at or after the
  entry's time (a coin of whoever funds it, the shard or the account). So the time an entry writes on the shard
  is no further ahead than a lateness and two dispute periods, an owner's entries of its own are a lateness
  apart by the chain's clock, and no entry closes a shard for good.
- **F106** is said in 15.1's list of what a taker checks: the time the account will say (when its output landed,
  and a lateness) against the chain's, before it signs.

## 16. The goal's rows in this form: what holds, and the rest on paper (26 November 2026; 16.2 to 16.5 on paper, nothing of them built)

Section 10 asked of each service of `GOAL.md` whether the forms of that day carried it. The form has changed since
(sections 12 to 15): a covered account holds its user's coins and futures legs, and the other side is a shard's
books. This section asks again, row by row, of the form as it is, and writes what is missing.

### 16.1 Row by row

| Service of the goal | In this form | On paper | Scripts, at the engine |
|---|---|---|---|
| One account, one risk number | a covered account of up to six legs, futures and tokens: one pot of coins, standing counted on the net, tokens counted as coins at a discount (14.2, 16.12) | yes | yes, for futures and token legs. Options are not in a covered account: section 17, on paper |
| Any counterparty: a side is handed over without the party that stays | a hand-over (15.1): the user's side to another key, the shard not shown | yes | yes (`KpfTPass`) |
| A clearing party takes the user's side at once | the owner (or anyone who makes a market) is the taker of a hand-over: at once, or within a lateness where the account's time is ahead (15.10) | yes | yes |
| A leg moves between accounts | one account is made two by a statement (16.2), and the new one is joined to another (15.2): at the price of a dispute period and the join's wait | yes | yes (`KpfTCSplit`, `KpfTJoinC`: 16.7) |
| One price | the committee's signed price, cited (4a) | yes | yes |
| Gains are usable: margin for another position | a gain of one leg counts for the standing of the others (14.2) | yes | yes |
| Gains are usable: taken out without closing | a mark (16.3): the leg is from now entered at a higher price and the shard is asked what it gained up to there; the band a leaving takes stays in the leg | yes | yes (`KpfTCMark`, `KpfTMarkNote`: 16.9) |
| Spot as margin | a token leg: the tokens are owned by a vault born for them under an id of its own, count as coins at their worth less a discount, leave by a ticket, and are sold where the account owes (16.12 to 16.24) | yes | yes, at the engine with the reference token's own script (`KpfTAddSpot`, `KpfTVaultPay`, `KpfTCTok`, `KpfTOwed`). No command yet; reviewed at the engine through a split, a hand-over, a sale and a call's step (16.26) |
| A spot purchase and a futures sale in one transaction | **two transactions**: the tokens bought are margin from the block they are bought in (a deposit beside the order book's fill: the places do not collide), and the futures leg is an entry stated on that transaction's output, in the next block (16.12 (g)) | said as it is | not run (the deposit alone is) |
| Default is handled in steps | a step takes one leg that lost, or all (14.2, kind 9) | yes | yes |
| One bond behind many accounts | the shard, held to its requirement, called by anyone (12 to 14) | yes | yes |
| ... touched only at its default | not so: an entry spends it (14.3), every note of a leaving or a step is applied to it, and a join shows it. What does not touch it: a hand-over, a take, a split, and the statement of a leaving or a step itself | said as it is | yes |
| Options | on an account no shard covers (4b). Under a shard: section 17, on paper | 17 | on an account no shard covers only |

One thing is missing: options under a shard (a leg that moves, a gain taken out and spot were the others: 16.7,
16.9, 16.23). It asks the shard's books to hold any number of rows: section 17.

### 16.2 One account made two: a leg leaves for an account of its own (kind 12)

15.2 left this out: legs and coins divided as their user likes leave the legs that lost in an account of no
coins, and whether that is so is a question of prices. A question of prices is a statement.

**The user states that leg i leaves its account for an account of its own, with `c` of the coins**, at prices
stated one a leg (a trace, as kinds 7 to 9; the rows are a leaving's of one leg, and `KpfTCRow` answers them as
they are). The end is right where:

- the account that stays passes the test of a take with what stays: its coins less `c`, less a note's least, are
  at least the maintenance and the tolerance of what the legs that stay are worth, and what those lost, each
  counted alone;
- the new account stands by its own leg, counted alone: `c`, less a note's least, is at least the maintenance and
  the tolerance of what the leg is worth, and what it lost.

Applied, it writes two accounts under the same id: the one that stays (the legs without leg i, the coins less
`c`, a leg fewer counted, its time), and the new one (leg i at its entry, `c`, one leg counted, the same keys,
terms, periods and shard, the same time). No note, and the shard is not shown: its books are as they were, since
no leg left them.

- *Nothing of a third party's moves by it.* Each of the two accounts stands by itself at the prices stated, each
  leg counted as badly as it can be; the shard is owed by the two what it was owed by the one, out of the same
  coins.
- *A leg moves between accounts* is then this and a join (15.2): the new account is joined to the other. *A part
  of an account is sold* is this and a hand-over (15.1).
- It waits as a take waits (the account's time, a dispute period). A user that is in a hurry hands the whole
  account over.
- *What whoever watches a split answers:* the end does not open the chain the statement says for the new
  account; the row of the leg that leaves does (`KpfTCRow`, as for a leaving). A split that names another chain
  has a right end and a wrong row (review 59).
- New: the end of kind 12 (`KpfTCSplit`, of `KpfTCEnd`'s leaving of one leg: the same sums, other outputs), and
  kind 12 in `KpfTChange` and `KpfTPoint`. The settlement writes the second account as it writes a note (an
  object of a hash stated, under the same id).

### 16.3 A gain taken out without closing: a mark (kind 13)

**The user states that leg i is from now entered at a price it says**, between the leg's entry and the price
stated for it. What the leg gained up to that new entry is asked of the shard. What the leg has beyond it,
unrealized at the price stated, is at the least the tolerance twice of what the leg is worth there: the band a
leaving takes. No unit moves: the leg stays, in the same market, with the same size, and no row of the books
changes.

- *The band is taken once.* A leaving asks what the leg gained less the band. A mark asks what the leg gained up
  to its new entry and leaves the band in the leg; the leg then leaves from its new entry, and the leaving takes
  the band. At one price, and at any later price at or above it, the two together ask exactly what the leaving
  alone asks: there a mark is an advance on the leaving. Where the price comes back by the band or more, the leg
  leaves from its new entry as a leg that lost: it pays back what it lost, whole, and no band is taken. The user
  then has up to the band more than the leaving alone gives at that price, and never more than the leg gained
  from its first entry at the last price stated: it is the band that is not earned, not coins that were not
  gained (review 60, F107). What bounds how often is the time a mark writes: no statement of the user's for
  twice the longest a statement lives.
- *Why not without a band* (what review 59's N12 asked, and what was decided there and is taken back): a price is
  stated within the tolerance of the committee's. A mark at the price itself, stated at the tolerance above a
  price that did not move, would be paid the tolerance of the leg's worth; the leg then leaves at the same stated
  price having gained nothing, and the shard has paid for nothing. The band at the leaving does not cover it:
  the entry moved. With the band left in the leg, the new entry is below the committee's price less the tolerance
  whatever is stated, and what is paid was gained.
- *Who states it:* the user, of a leg that gained more than the band. Nobody marks a leg for another, and no leg
  that lost is marked (decided after N15, built, and taken back). A mark of a leg that lost is a leaving and an
  entry at the lower price: where the price comes back the user is out by as much as the band, each time, since a
  gain from the new entry pays the band again; and the one who gains by it is the shard's owner, who would do it
  at every dip. Leaving the band with the user instead has the shard forgive two thirds of a fall marked in steps.
  The sum a leaving pays is not a straight line (a loss whole, a gain less the band), so no entry written by
  another is fair to both.
- *What N15 found stays so:* a shard pays a mark from the coins it holds, and a book that is set off holds its
  owner's coins while the legs that lost stay where they are. Where it holds less than is asked the rest is a note
  of the rest: counted in what the shard owes, paid as the shard comes to hold, ending every draw until then, and
  a claim under a call. A user that will not wait hands the account to a clearing party and takes one of the same
  legs back at today's price (15): that asks somebody's leave, and the mark does not.
- *The time:* it waits by the account's time and writes it, as a take.

The end is right where the statement is the user's, at or after the account's time; the leg gained something up
to its new entry; what it has beyond is the tolerance twice of its worth or more; and the account passes the
test of a take: its coins, less a note's least, are at least the maintenance and the tolerance of what all its
legs are worth, and what the other legs lost, each counted alone. Applied, it writes the account with the leg at
its new entry (the coins as they were), and **a note that a leg was marked**: the leg as it was, its new entry,
the user's key, what is asked. (The note's least is put in by whoever applies the statement, as a leaving's is;
the end's test keeps as much of the account's coins out of every sum.)

- *The note cannot be a lie that costs the shard its equity.* The script that applies it computes, of the leg and
  its two entries, what the leg gained between them, and takes no note that asks anything else. What the legs
  were entered at moves by exactly what is asked: at any price the shard's equity is what it was. A note that asks
  otherwise (a statement nobody disputed wrote it) is applied by nothing; it holds a note's least and ends no
  draw.
- *It is applied whatever the rows hold by then* (N13): no row is read. The leg may have left first: the two
  notes end the books the same in either order. Until it is applied the books say the old entry and have not
  paid: the equity is the same, the coins are not. So a note of a mark that landed before a draw was stated and
  is not applied ends the draw, as a note that legs left does (16.11).
- *The rows.* A mark's trace has a covered account's five sums, read so: what all the legs are worth; what the
  others are worth (so that the marked leg's worth is had apart); what the others lost, each counted alone;
  what the marked leg gained up to its new entry; and, where the others have what all gained, what the marked leg
  keeps beyond it, to which no other leg adds. `KpfTCRow` answers them (it has the leg and its direction); the
  end holds the sums to the rule (it has the tolerance).
- *No entry's rule is asked* (a lateness after the last, the two signatures): no unit enters the books.
- *Under a call* a mark stated at or after the call's time is ended by a step, as kinds 7 to 9 and 12 are. A
  note of a mark applied to a called shard is owed whole, as a note that a leg left is.
- New: the row of a mark in `KpfTCRow`; the end of kind 13 (`KpfTCMark`); the applying of its note
  (`KpfTMarkNote`); kind 13 in `KpfTChange`, `KpfTPoint`, `KpfTStepPrice` and `KpfTCStep`.

### 16.4 Options under a shard

On an account no shard covers an option is a leg (4b): its row adds what it is worth exercised, less what it was
worth at its entry. Under a shard the question is the books: a shard knows what its side gained by one number
(what its legs were entered at) because a futures leg gains its units times the price. An option does not: what
it is worth exercised bends at its strike.

**The form proposed:** a row of a shard for options is of one market, one kind and one strike (a call or a put;
the units its side holds, long or short). What such a row is worth to the shard at a price is its units times
what an option of that strike is worth exercised there, which a script computes as it computes a futures row's
worth; what its legs were entered at is one number as before (each option's entry is what it was counted at,
and a mark moves it). The requirement counts an option row at the whole of its underlying (the maintenance and
the tolerance of the units' worth at the price), as section 12 said.

- *What it costs:* a shard has four rows, and a strike is a row. A clearing party that writes options of many
  strikes has a shard a few strikes each, or the row count grows (a shard's rows are 408 bytes of every
  transaction that shows them).
- *What is not known:* whether an account that holds an option and a futures leg of the same market is asked the
  margin of both whole (it is, by the rule above, and that is more than the risk); and the price of an option's
  entry (the premium), which no committee signs: it is what the two agree, and the band does not bound it.
- Not built, and not the next thing: the rows above ask `KpfTShardEnd`, `KpfTCallEnd`, `KpfTEnterS`, the notes'
  scripts and `KpfTCStep` to know an option's worth.

### 16.5 Spot as margin

Section 10's design stands, and nothing of it is built: a leg of a kind of its own (a token's market and an
amount, counted at the price stated less a discount, for the user's standing alone); a vault, an object under
an id of its own that owns the tokens; tokens enter by a script that raises the leg's amount and holds the
token's output to the vault, and leave by a statement (their worth is a price).

- *Under a shard* a token leg is nothing to the books: it is the user's margin, not a position against the
  clearing party. It changes the account's two tests and no row.
- *What it needs:* a token. The reference KCC-20 is in `vendor/kob/contracts/kcc20` (2,915 bytes; its owner is a
  key, a script's hash or a covenant id: the last is an input under that id in the transaction), with the order
  book that keeps custody of such tokens by a script. And a price of the token signed by a committee. Whether a
  vault's transaction fits a block beside the token's script is what only building answers.
- *What review 59 adds* (N22): a step must be able to sell tokens out of the vault; a call's step takes no account
  that has a leg outside the call's four rows, so a token leg must be passed by it; the vault must say whose
  tokens it holds (every account is under the one registry's id, so "an input under that id" is not one
  account); and the discount must be more than the tolerance.
- *A spot purchase and a futures sale in one transaction* is then two scripts on one account in one
  transaction, which the account's scripts do not do today (each takes the object alone).

### 16.6 Order

The rows of the goal first, the tools after (`GOAL.md` is what is to be shown to hold; a command and an Executor
make it usable, and are written for everything at once):

1. 16.2 (a split), then 16.3 (a mark): each is a statement of the kinds that exist, with one end of its own, and
   closes a row.
2. 16.5 (spot), designed whole against the reference KCC-20 (16.12) and attacked on paper before a script is
   written. 16.4 (options under a shard) is decided for now as 16.8 says.
3. The commands, and what an Executor does, for sections 12 to 16.

### 16.7 Built: 16.2, one account made two (26 November 2026; at the engine)

`KpfTCSplit`; the library has 64 scripts. Kind 12 is stated by `KpfTChange` (with the floor of a covered
account's bond), disputed by `KpfTPoint`, its rows answered by `KpfTCRow` and its prices by `KpfTStepPrice` as a
leaving's are, and a split stated at or after a call's time is ended by a step. A split is stated in 3,181
bytes and applied in 2,643; its end is answered in 3,887.

Run (`tests/thin_draw.rs`): an account of a long and a short with 40 KAS; the short leaves with 10 for an account
of its own, and 30 stay; no note; both accounts say the time a statement of the user's writes. The end at its
four edges (the least the new account stands by and a sompi less; the most that leaves the other standing and a
sompi more), by another key, with no leg named, with nothing; an account of one leg is not made two; a leg that
lost carries what it lost. Then **a leg moved** (the new account joined to another of its user's: 3,988 bytes)
and **a part sold** (the new account handed over: 1,780 bytes). With it the row "a leg moves between accounts"
holds in this form, by a statement and no signature of the clearing party.

### 16.8 Review 59

At the engine: F105 is closed (a statement out of its time, ended by its own key, costs 0.1145 KAS for a step,
0.5144 for a call or a draw; ended by another, the whole bond; an entry of a time ahead of every input is not
stated, and the entry of the last time is gone). **The split holds**: on 72 random accounts of two to six legs
the engine takes the model's verdict at the end and no other; ten ways of writing the two accounts otherwise are
wrong; neither account is a step's at the price stated, at the tolerance or a tenth beyond; no coin leaves past
a take's test; splitting before a call gives what two accounts held from the start give, and no more. No new
finding at the engine. The default workspace: 607 tests, 217 seconds; with the features 1,301, 1,004 seconds; none
failed. `KpfTCSplit` is 2,251 bytes, `KpfTChangeLate` 725, `KpfTEnterS` 2,323.

Said and left: an owner's entries of times behind the chain, on a shard that says no time, land in a row; each
that is out of its time costs it half its bond and the fees (0.12 KAS at the least), and the shard's time does not
run ahead. An entry sent late by the party that lands it costs the other its bond's half (14.3's known point).

On paper (sections 16.3 to 16.5), and what was decided:

| | What it found | Decided |
|---|---|---|
| N12 | the band at a mark is taken for nothing: the stated price becomes the entry, and the end holds the tolerance | first: no band. **Taken back** while building: a mark at the tolerance above a price that did not move, and a leaving at the same stated price, had the shard pay the tolerance for nothing. The band stays in the leg, and is taken once (16.3) |
| N13 | a mark's note must be applied whatever its row holds by then, and spent for nothing by no script | so (16.3) |
| N14 | the books' number moves by the leg's worth at the new entry less at the old, as an entry counts each | so |
| N15 | a mark is the winner's alone: a book set off pays its winners from the owner's coins while its losers sit | first: anyone marks a leg that lost. Built, and **taken back**: it costs the user the band at every dip that comes back, to the gain of whoever owns the shard. It is said as it is: the note of the rest waits (16.3) |
| N16, N17 | the books between statement and note, under a draw and a call: as written; a mark is no way round the entry's rule, a step or a take | the user's mark waits by the account's time and writes it |
| N18 | build it | built (16.9) |
| N19 to N21 | options under a shard: a row a strike is as sound as a futures row and no better; the expiry is not said, nor what a row is worth after it; a holder is asked several times its risk; four rows make a shard a series | **not under a shard in this form.** Options are legs of an account no shard covers (4b: built, reviewed), where futures and options are netted in one account. Under a shard: after the expiry is designed, and only if the goal asks it there |
| N22 | spot: what a step, a call's step, the vault and the discount need; three rows of 16.1 were wrong | 16.5 and the table say it |

### 16.9 Built: 16.3, a gain taken out of a leg that stays (26 November 2026; at the engine, my tests)

`KpfTCMark` and `KpfTMarkNote`; the library has 66 scripts. Kind 13 is stated by `KpfTChange` (with the floor of
a covered account's bond), disputed by `KpfTPoint`, its rows answered by `KpfTCRow`, its prices by
`KpfTStepPrice`, and a mark stated at or after a call's time is ended by a step. A mark is stated in 3,193 bytes
and applied in 2,643; its end is answered in 3,856; its note is applied in 2,785 (2,820 where a note of the rest
is written).

Run (`tests/thin_draw.rs`): a long of 10 units from 10, at 12, in an account of 40 KAS beside a short. Its new
entry is the nearest to 12 that leaves the band of 120 KAS in the leg; the shard is asked what the leg gained up
to there and pays it; its rows are as they were and its equity is the same at three sets of prices. The leg then
leaves at 12: **the mark and the leaving together ask what the leaving alone asks, to the sompi**, and the two
notes end the books the same in either order. A short that gained, the same. A shard that holds less pays what it
holds and owes the rest; a called one pays nothing; a shard whose rows have nothing of the leg takes the note. A
note that asks a sompi more or less than the leg gained between its two entries is applied by no transaction.

The ends that are not the rule's, each answered wrong at the engine and right refused: an entry a step nearer the
price than the band allows; at the price; beyond it; the leg's own entry; below it (a loss is no mark); none; by
another key; no leg or every leg; a leg that gained less than the band; before the account's time; a sompi less
than stands, alone and beside a leg that lost. The rows that are not: a leaving's; the marked leg said to keep
more; another leg adding to what it keeps; the marked leg passed by in the chain.

Not tested by me, and asked of review 60: random accounts against the model; a mark under a call; a leg of a
fraction of a unit (the nearest entry is searched a price's step at a time, 64 steps: a leg too small has none).

### 16.10 Review 60

At the engine, the mark as built. On 80 random accounts of one to six legs (long and short, whole units and
fractions; new entries at the nearest the rule allows, a step each side, the leg's own entry, the price itself;
coins at the take's test and a sompi each side) the engine takes the model's verdict at the end and no other, and
the marked leg's row is answered as the model says; 13 ways of writing the account or the note otherwise are
wrong. On 200 random legs a mark and the leaving at one price ask what the leaving alone asks, to the sompi, and
so do three marks at rising prices and the leaving at the top; a second mark at one price, a leg that lost and
another key are refused. A take and a split after a mark are what they were before it; an account at the take's
test marks and is then no step's at the price stated or at the tolerance against it. Under a call the total a
user is asked or has back is the same marked or not. The note: applied to a shard that holds enough, less,
nothing, that is called, and whose rows no longer have the leg, the rows are unchanged and what it holds less
what it owes less what its legs were entered at is as before; a note that asks otherwise is refused. Taking back
"no band" (N12) and "anyone marks a leg that lost" (N15) is right for the form as built. The default workspace:
612 tests, 251 seconds; with the features 1,306, 1,035 seconds; none failed. `KpfTCMark` is 2,220 bytes,
`KpfTMarkNote` 1,154, `KpfTCRow` 2,607 (it was 2,245).

| Finding | What it is | Decided |
|---|---|---|
| F107 (low) | a mark, a fall of the band, the leaving: no band is taken (a long of 10 from 10, marked at 12 to 11.88 and paid 18.8 KAS, leaves at 11.5 and pays 3.8 back: 15 KAS, where the leaving alone at 11.5 asks 13.85) | said as it is in 16.3. The form that closes it (the leg keeps its entry and remembers what was advanced: eight bytes in every leg, a line in every script that reads one) is not built now: the band as a fee is tuning, which waits |
| F108 (low) | a draw stands in front of a mark's note: the equity is the same but the coins are not, and `KpfTDrawNote` opens no note of kind 13. A shard of 30 KAS, a mark that will ask 18.8, a draw of 25: the note is paid 5 and the user holds a note of the rest | `KpfTDrawNote` takes a mark's note as it takes a note that legs left, where the note is one the shard would apply (16.11) |
| said | a note of a leg that is in no account (a statement nobody disputed wrote it) takes what the shard holds while its equity stands, so that no call is due and honest claims wait behind it: the worst a wrong leaving's note does in coins, without the call | as every statement: it is disputed in its time |
| said | a set-off book whose winners all mark runs out of coins after a move of a few per cent while its equity has not moved; winners then hold notes of the rest and are paid as losers leave. The books do not know the winners' part, so the upper level cannot ask coins for it | a line in what a user reads of a shard: what it holds in coins against what it owes |

### 16.11 After review 60 (27 November 2026; at the engine, my tests; the reviewer's are round 61)

- **F108: a note of a mark ends a draw.** `KpfTDrawNote` takes a note of kind 13 that landed before the draw
  was stated, shown as the leg as it was and its new entry, where it is one the shard would apply: the script
  computes what the leg gained between its two entries and holds what the note asks to it. A note that asks
  otherwise (which `KpfTMarkNote` applies never) ends no draw: it would end every draw for good. Run: a draw
  ended by a mark's note; a note that landed after the draw; one that asks a sompi more or less; one of a leg
  that lost between its entries; one to another shard: each of these four ends none at the engine. The command
  that ends a draw by a note takes a mark's.
- A draw stated before the mark's note lands stands, as it stands before a leaving's note that lands after it:
  the note is then paid what the shard holds and the rest is owed (16.3, what N15 found).
- **F107** is said in 16.3.

### 16.12 Spot as margin under a shard: the whole of it on paper (27 November 2026; nothing built)

This replaces 16.5's sketch, and is as review 61 left it (16.13). It is written against the reference KCC-20
(`vendor/kob/contracts/kcc20/KCC20Ref.sil`) and against how the order book in `vendor/kob` keeps custody of such
tokens (`contracts/orders/KobAsk.sil`, `KobBid.sil`; its vectors: an ask placed 4,002 bytes, filled 5,534,
cancelled 5,136).

**(a) Custody: a vault under an id of its own.** A token UTXO moves where its owner allows: a key's signature; an
input whose script is P2SH(owner); or any input under the covenant id `owner` (scheme 4), which is how an order
of the order book holds what it sells. With scheme 4 the token says nothing of where the tokens then go: **in
every transaction that has an input under the owner's id, a script of ours pins the token's outputs.** That is
the whole of custody.

- Owner = the registry's id is no custody: every object of the library is under it.
- Owner = P2SH of the account's object: the object's script changes with every state.
- So a **vault**: an object under a covenant id V of its own, born by a genesis output. The tokens are owned by V
  (scheme 4, borrowing off). They move only in a transaction that spends the vault, and the vault is a slot: it
  is spent only where a script of the library names it, and that script answers for where the tokens go.
- The vault's script is the slot every object has (`KpfSlot`): it asks that the input that names it is under
  the library's id, and nothing of the id it is under itself. No new script holds a vault.
- The vault's state begins with a tag no other object's state begins with (so that no script that knows nothing
  of vaults opens one: the static test of the places every script names is then the proof of custody). It says:
  **the id its account's objects are under** (a ticket is taken from under that id and no other: N23); the token
  (its covenant id T, the hash of its program's template, the two lengths that place the state in the program,
  its extension commitment); and the amount it holds.
- The token's program (2,915 bytes) is no constant of ours: a script that must know a token output takes the
  program from a token input of the same transaction (whose redeem script is in its signature script), holds it
  to the template's hash, and rebuilds the output's script with the state it wants (as `KobAsk` does; `KobBid`
  where it holds no token yet).
- **One output under V when it is born, and none when it is paid** (N24): a second output under V would be a key
  to the tokens.

**(b) The account's side: a token leg.** A leg of direction 7 (1 to 6 are taken by futures and options): push(the
market of the token's price) push(V) scale, price limit, the amount, 7, the discount in basis points. (V stands
where a futures leg has its committee: under a shard the committee is the shard's, and a script that needs it
opens the shard's id, the hash of a seed, the committee, the terms and the periods.)

- **The market's name is the hash of the token** (its id, its template's hash, its extension commitment, under a
  tag for spot): the deposit holds the leg's market to the vault's token (N25). So no token is priced as another:
  a committee that signs a price for that name signs it for that token and that program.
- The leg is the bearer of the vault. A leg is never written twice (a split moves it, a join appends it, a
  hand-over keeps it), and V is new when the leg is made: one vault, one leg, at all times.
- **What it counts:** its worth at the price stated, less the discount, rounded down, and a sompi: as coins, for
  the user's standing and for what leaves as coins. Nothing for the maintenance (no rate of its worth is asked:
  the discount is that) and nothing for the shard's books: no row, no entry's rule, no note to the shard.
  (The sompi: the end of a step turns to "every leg" by its sums, with rows that named one leg; that an account
  holds a token leg is then known by TK not being nothing, whatever the token is worth.)
- **A row of a covered account has a sixth sum, TK:** what the token legs that stay count. Such a row is 120
  bytes (it was 112): the price, six sums, two chains. The rows of an account no shard covers stay 112.

**(c) What each statement asks, with TK.** (m: the coins less a note's least.)

| Kind | With token legs |
|---|---|
| 7, a take | m - amount + TK >= the maintenance and tolerance of W, and LS |
| 8, a futures leg leaves | a leg that gained: m + TK >= ... of WS, and LS. A leg that lost: the coins pay what it lost (coins alone: a token pays nothing; a user that holds only tokens adds coins first) |
| 8, every leg | not while the account holds a token leg (its row is wrong under "every leg"): tokens leave first |
| 9, a step on a futures leg | due where m + what all gained + TK is below the lower level. As today, but never "every leg" while TK is something |
| 12, a split; 13, a mark | the leg named is a futures leg (a token leg's row is wrong where 8, 9, 12 or 13 names it: N28); the tests take TK as a take's does |
| **14, tokens leave** (new) | the user's, at or after the account's time: the leg named is a token leg (a futures leg's row is wrong where 14 or 15 names it); the account passes the test of a take without it. Writes the account without the leg, and a **ticket** to the user |
| **15, tokens sold in a step** (new) | anyone's, where a step is due and no step on a futures leg is: no leg has lost, or m less what the legs lost (each alone) is below the liquidation fee of W. The leg named is a token leg; the claimer says what it pays, at least the leg's worth at the price stated less the liquidation fee's rate; the account's coins grow by it; the ticket is the claimer's. The claimer's bond holds what it pays, so that anybody applies the statement |

- *Futures legs first* (N29): a step takes a leg that lost for a fee of that leg's worth. Tokens are sold only
  where the coins do not pay such a step, and then a whole token leg is sold for a fee of its worth.
- A **ticket** is a note under the account's id: blake2b(i9(12) + i9(14) + push(the chain of the leg alone) +
  push(a key)). It names the leg, and the leg names V and the amount. Only the end of kinds 14 and 15 writes it.
- The discount a leg says is at least the liquidation fee's rate and twice the tolerance: so the sale of 15 never
  leaves the account standing worse than the leg counted, whatever price within the tolerance is stated.
- The price of a token leg's row is answered as any (the committee's price for the leg's market, cited), by a
  script that opens the shard's id for the committee.

**(d) Tokens enter: `KpfTAddSpot`.** No statement (more margin is safe at once), and two signatures: the user's
and the shard's owner's (who signs for the amount and the discount, and by it says its shard takes this token).
One transaction: the account, a copy of the script, a token input; outputs: the account with the leg appended
(its chain is the hash of the old chain and the leg: no leg is opened), the copy, **the vault, born here** (the
script reads the covenant id of that output for V; no input is under V, and exactly one output is: it is a
genesis, so V is new), the tokens owned by V with the vault's amount, the user's token change. The leg's market
is the hash of the vault's token; the vault says the id the account is under. It waits by the account's own time
and writes it, as an addition of coins. Six legs an account in all; a second deposit is a second leg.

**(e) Tokens leave: `KpfTVaultPay`.** Anyone, with a ticket: the vault, a copy of the script, the ticket (**an
input under the id the vault says**), the vault's tokens (the token's leader, which the vault's presence
authorizes). The leg is shown: it hashes to the ticket's chain, its V is the vault's id, its amount the
vault's. Output: the tokens to the ticket's key, whole. No output is under V: the vault ends. The ticket is
spent. A token output of another's sent to V (a stray) leaves with whoever pays the ticket.

**(f) Under a call: the rule of the coins** (N26). An account whose coins pay what its futures legs lost at the
call's prices is stepped as today, and goes on with its token legs and what coins it has left. The call's step
refuses an account that holds a token leg and whose coins do not pay. For such an account kind 15 stands under
a call (where 7 to 9 and 12 to 14 are ended by a step): its end is shown the call. So no user takes tokens out
from under what it owes. **Built before any token enters an account.**

**(g) A spot purchase.** A fill of an ask of the order book gives the buyer a token output that the order does
not pin, and the two projects' places for inputs and outputs do not collide (the order wants its maker's payment
at the output of its own input's index; ours wants a copy at the copy's index). So one transaction fills the ask
and runs `KpfTAddSpot`: the tokens bought are margin from the block they are bought in. The futures sale is an
entry stated on that transaction's output, in the next block. An entry that also binds a vault (one transaction
for both, about 11 kB) is not in the first form (N33).

**(h) Sizes, before building.** A token input carries the token's program: 2.9 to 3.2 kB each, whatever we do.
A deposit: about 6 kB. Tokens leaving: a statement (3.2 kB), its applying (2.7 kB), the vault paid (about 5.5
kB). A row 8 bytes longer: every statement of a covered account, 8 bytes a leg. The vault and the tokens' output
each hold a carrier of some KAS (three outputs of 1 KAS in one transaction are more than it may weigh: 14.6); how
much is computed when the deposit is built.

**(i) The order of building.** (0) The reference token compiled and run once in our script engine. (1) The row of
120 bytes for covered accounts in the model and in every script that reads one (`KpfTCRow`, `KpfTCEnd`,
`KpfTCSplit`, `KpfTCMark`, `KpfTStepPrice` for kinds 7 and above, the entry's rows), with TK nothing: every test
as it was. (2) The token leg in `KpfTCRow` and TK in the tests of the ends. (3) Kinds 14 and 15 and their end.
(4) The call's step and kind 15 under a call. (5) The vault: `KpfTAddSpot`, `KpfTVaultPay`. Each a commit and a
review.

### 16.13 Review 61

At the engine: F108 is closed for a note that is there when the draw is stated (a draw stated at 6,600 is ended
by a mark's note that landed at 6,050, standing and disputed; a note that landed at 6,599 is taken, at 6,600 or
after refused; a sompi off, another entry, another shard: refused by the engine). A draw stated while a mark or a
leaving waits its dispute period stands, for a leaving as for a mark (round 60 had it otherwise, and takes it
back): closing it would give every user a way to end every draw for a bond that comes back. `KpfTDrawNote` is
1,700 bytes (it was 1,406). The default workspace: 615 tests, 200 seconds; none failed.

On paper, section 16.12:

| | What it found | Decided |
|---|---|---|
| N23 (high) | the ticket is held to no id: anybody makes an object of its own, under an id of its own, whose state is a ticket's hash for a victim's leg and its own key, and is paid the vault | the vault says the id its account's objects are under; `KpfTVaultPay` takes the ticket from under that id |
| N24 (high) | a second output under V at the deposit is a key to the tokens while the account counts them | exactly one output under V at its birth; none when it is paid |
| N25 (high) | nothing but the owner's eye holds the token to the market that prices it | both: the market's name is the hash of the token and its program; the owner signs for the amount and the discount |
| N26 (high) | under a call tokens walk: an account with no coins and tokens counted at 900, whose legs lost 800, is asked nothing by the call's step and takes its tokens out | the rule of the coins, (f); built before any token enters an account |
| N27 (medium) | the vault's state needs a tag no other object's begins with | so; the static test of the places covers the two new scripts |
| N28 (medium) | a token leg's row answered wrong where 8, 9, 12 or 13 names it, a futures leg's where 14 or 15 does; direction 3 is a held call | so; direction 7 |
| N29 (medium) | kind 15 sold a whole token leg wherever a step was due, for about twice a futures step's rate | futures legs first: 15 only where no step on a futures leg is due to be paid; the claimer says what it pays; anybody applies it |
| N30 (low) | "coins alone pay a loss" strands a user with only tokens | a line in (c) |
| N31 (low) | drop the sompi: the token row being wrong under "every leg" holds it | kept: a step's end turns to "every leg" by its sums with rows that named one leg, and must know a token leg is held whatever it is worth |
| N32 (low) | six sums is the least; the 112-byte row is shared with accounts no shard covers | the rows of covered accounts grow, the others stay |
| N33 (low) | a fill and a deposit share a transaction; an entry that binds a vault is 1 kB more script and an 11 kB transaction | the deposit with the fill, the entry on its output in the next block |
| N34 (low) | 6 kB and 5.5 kB are about right; the carriers of a vault and of the tokens are to be computed | when the deposit is built |
| N35 (low) | a split nobody disputed leaves a token leg in both accounts | as every statement; whoever watches such accounts answers every leg's row |

Whether to build it: yes, in this form (no smaller form drops a hard part: custody, a sale and the call's rule
are needed whoever sells).

### 16.14 Built: 16.12, steps 1 to 3 (27 November 2026; at the engine, my tests; nothing of the vault yet)

No token enters an account yet: the deposit and the vault are step 5, after the call's rule (step 4). What is
built is the account's side, on accounts made by hand in the tests. (Step 0, the reference token in our engine:
the order book in `vendor/kob` runs it on the same engine and the same compiler; it is wired into our tests with
the vault.)

**Step 1, the row.** A row of a covered account is 120 bytes: the price, six sums, two chains (the sixth sum at
48, the chains at 56 and 88). `KpfTCRow`, `KpfTCEnd`, `KpfTCSplit`, `KpfTCMark`, `KpfTCEnter`, `KpfTEnterS` and
`KpfTChange` read that length; `KpfTStepPrice` reads 120 for kinds 7 and above and 112 below; the three scripts
of accounts no shard covers are as they were. With the sixth sum held to what the row before had, every test of
the workspace but those that say a script's size passed.

**Step 2, the token leg.** `KpfTCRow`: a leg of direction 7 that stays adds nothing to the first five sums and,
to the sixth, its worth at the row's price less its discount (rounded down) and a sompi; a discount beyond the
whole is wrong; named by a statement of kinds 8, 9, 12 or 13, or entering by kind 10, its row is wrong. The ends
take TK as coins: a take (and no more than the coins: with TK in the sum that had followed from the test, and is
now said), a futures leg that gained leaving, the standing of a step, a split's account that stays (and no more
than its coins), a mark, an entry. "Every leg" is not while TK is something.

**Step 3, tokens leave or are sold.** `KpfTCTok`; the library has 67 scripts. Kinds 14 and 15 are stated by
`KpfTChange`, disputed by `KpfTPoint`, their rows answered by `KpfTCRow` (the leg named is a token leg, one; its
worth whole is the fifth sum; it counts in the sixth only where it is sold), a 14 stated at or after a call's time
is ended by a step. The end is 16.12's table. Two things are as built and not as 16.12 said them:

- *What the claimer of a 15 pays is in the statement from the start:* `KpfTChange` asks the statement's output
  to hold the account's coins, the bond and what is said where a take has its amount. A statement's settlement
  gives its claimer its whole bond back, so a bond cannot hold the payment; this way anybody applies a 15 for a
  note's least. A 15 that ends (answered wrong, or out of its time) leaves what it put in on the account.
- *The price of a token leg's row:* `KpfTStepPrice` has one argument more, the seed of the shard's id. For a leg
  of direction 7 it holds the account's shard to the hash of the seed, the committee the citation is of, the
  account's terms and periods, and the citation's market to the leg's.

Run (`tests/thin_draw.rs`, 38 pass): a long of 1,000 units and 100 tokens that count 800 KAS: the user takes its
coins out, to the sompi the test binds at; no more than the coins; without the tokens, nothing. The token leg's
row right, and wrong where it counts nothing, its worth whole, among the legs' worth, or where a futures leg adds
to the sixth sum; a leaving that names it. A step not due with the tokens and due without; "every leg" refused
while tokens are held. A split and a mark carried by the tokens. Tokens leave (stated 3,267 bytes, applied 2,644,
its end answered in 4,367) where the account stands without them and not a sompi under; the ticket is the
user's. The token row's price answered right and wrong by the committee's citation with the seed (8,298 bytes),
and not with another seed. Tokens sold in a step: the claimer pays their worth less the fee's rate, the statement
holds it, the account's coins grow by it, the ticket is the claimer's; a sompi less paid, no step due, or coins
that pay a step on the futures leg: wrong; and that step is then stated.

Not built: the call's step and a 15 under a call (step 4); the deposit and the vault's payment (step 5); the
commands. Until step 5 no account on a chain can hold a token leg.

### 16.15 Review 62

At the engine, steps 1 to 3. On 36 random accounts with none to three token legs (discounts of nothing, of more
than the whole, below nothing) and a statement of each of kinds 7, 8, 9, 12, 13, 14 and 15, 122 rows were
disputed and the engine took the model's verdict and no other. On 129 statements the rule allows, 68 on accounts
that hold a token leg, the right end is taken and the wrong is not; a take at its edge with what the tokens count
allows the most and not a sompi more, and never more than the coins less a note's least. A token leg leaves and
its ticket is the user's; a second ticket of that leg is not written. Of 30 random accounts below the lower level
none was one on which neither a step nor a sale is the rule's. The sompi a token leg counts (N31) holds what it
is there for: "every leg" is never the rule's while a token leg is held, at a discount of the whole too (by the
model). The default workspace: 621 tests, 191 seconds; with the features 1,315, 990 seconds; none failed.
`KpfTCTok` is 2,713 bytes, `KpfTCEnd` 3,342 (it was 3,154), `KpfTCRow` 2,574, `KpfTStepPrice` 1,825.

| Finding | What it is | Decided |
|---|---|---|
| F109 (medium) | the end of kinds 14 and 15 writes a ticket for a futures leg, and the model states it: the end is right, only the leg's row is wrong. A user sheds a futures leg that lost, with no note to the shard, unless that one row is disputed | the end is shown the leg the statement names, holds it to the chain said, and asks that it is a token leg; the model writes no such statement (16.16) |
| F110 (medium) | a sale that is not applied gives what its claimer put in to the account: a claimer a block late or a price's step off loses the whole payment, and a falling token finds no claimer | nothing is put in at the stating: the claimer pays as it applies. A sale nobody applies in its life is ended by anyone for half its bond (16.16) |
| said | `KpfTCStep` asks every leg to be long or short: an account that holds a token leg is stepped under no call. No script writes a token leg into an account yet | step 4, before step 5 |
| said | an account whose coins are a note's least (a take emptied it onto its tokens) has statements whose answers are too heavy for a block (F102's kind) | with the storage mass of step 5 |

Not reached by the review: the price of a token row at the engine (a wrong seed, another committee, another
market); an entry on an account that holds a token leg; the edges of a split, a mark and a step with what the
tokens count; a sale disputed at a row or a price; directions 15 and 23.

### 16.16 After review 62 (27 November 2026; at the engine, my tests; the reviewer's are round 63)

- **F109: the end of kinds 14 and 15 is shown the leg the statement names.** `KpfTCTok.answer` has one argument
  more, the leg: it hashes to the chain the statement says (with no such leg shown nobody answers the end, and
  the point lapses against the claimer), and where it is not a token leg the end is wrong. The model writes no
  trace of these kinds that names another leg. (A statement that says a token leg's chain and passes a futures
  leg by in its rows has a right end and a wrong row, as a leaving that names one leg and passes another by.)
- **F110: nothing is put in at the stating of a sale.** `KpfTChange` is as it was. The claimer pays as it applies
  its statement (a settlement's outputs hold at least what the statement says, and whoever applies it funds
  them; the ticket is the claimer's, so nobody else does). **A sale nobody applies is ended by anyone**
  (`KpfTCTok.lapse`): where the statement's output is as old as a lateness and two dispute periods, the account
  again, half of the bond to whoever ends it and half in the script's copy. So a claimer that is a block late or
  a price's step off loses a bond, as with any statement, and one that states a sale and leaves it has held the
  account for that long for a bond.

Run: a futures leg named by a 14: no trace, the end answered wrong and right refused, its row answered wrong.
A sale stated (the statement holds the account and the bond), applied by its claimer who pays; the same sale not
applied, ended at its life (half the bond to whoever ends it) and not a block before.

### 16.17 Step 4 on paper: an account that owes (27 November 2026; replaces 16.12 (f); nothing built)

16.12 (f), "the rule of the coins", had the call's step refuse an account that holds tokens and whose coins do
not pay, until somebody bought the tokens. Building it showed what it rests on: a buyer. A token nobody buys (it
fell to nothing; its market has no committee left) would keep the account's legs in the shard's rows for good,
and a call ends only when no row has a unit: one such account would hold every claim of a called shard. The same
outside a call: "not every leg while tokens are held" leaves the clearing party with an account it cannot close.

**The rule instead: the legs leave, and the account owes.**

- A covered account's moving part has a number that is nothing today (where a position has side B's margin).
  It is **what the account owes its shard**. Every script that takes a covered account asks that it is nothing
  (they do today), but the three below: an account that owes does nothing else until it has paid.
- **A step of every leg** (kind 9, stated of every leg) on an account that holds token legs: the futures legs
  leave, the coins pay what they can (a note's least stays), and the account goes on with its token legs and
  owes the rest. Its note names all the account's legs, as today; the scripts that apply it pass a token leg by.
  (A step that names one leg and turns to every leg by its sums is still not the rule's while tokens are held:
  its rows do not say the chain of the token legs alone. It is stated of every leg.)
- **The call's step** on an account that holds token legs: the same. The futures legs are counted at the call's
  prices, the coins pay what they can, the account goes on with its tokens and owes the rest; where the coins
  pay everything it owes nothing and has what is left. The step is never refused: a call reaches its end
  whatever a token is worth.
- **An account that owes** has two ways on. Its user pays (a script that takes coins of the user's and writes
  the payment to the shard). Or anyone sells a token leg (kind 15): of an account that owes, a sale is the rule's
  with no other test; what the claimer pays goes to the shard first, as far as the account owes, and the rest to
  the account's coins. When it owes nothing it is an account as any, and its tokens leave by its user's
  statement.
- **What is paid to the shard** is a note that holds the coins and says its shard, which a script of one line
  applies (the shard holds that much more). Under a call it is what the claims are paid from, late or not.
- The sale before any debt (16.12 (c), kind 15: where a step is due and the coins pay no step on a futures leg)
  stays: it keeps a user's futures legs where its tokens can pay. The debt is for what is left when nobody did.

**What changes in what is built.** `KpfTCRow`: of a step of every leg, a token leg stays and adds one to the
third sum (which is nothing there: so the end knows how many legs the account goes on with). `KpfTCEnd`: a step
of every leg with tokens writes the account that owes and no payment to the user. `KpfTLeftAll`, `KpfTLeftVoid`:
a token leg in a note's legs is passed by, and is no leg "that is not in the books". `KpfTCStep`: token legs
passed by in the sum; the account that goes on. `KpfTCTok`: the sale of an account that owes. New: the payment's
note and the script that applies it; the user's payment.

**Asked of the review, on paper:** whether the number is safe where every other script asks nothing of it (a
script that forgot to); what an account that owes and whose tokens are worth less than it owes ends as; whether
the user's own payment and a sale can race or cross; what the three covenant outputs of the call's step weigh
(the call, the note, the account that goes on) and what the account must hold; a split, a hand-over or a join
that makes an account owe without its user (none should: they ask that it owes nothing); and whether the sale
before any debt and the debt together leave a state with no way on.

### 16.18 Review 63, and step 4 as it will be built

At the engine: F109 is closed (a statement of kind 14 on a futures leg is written by no tool, and written by
hand has a wrong end) and F110 is (a sale's statement holds the account and the bond; ended, it leaves nothing
in the account). Whoever applies a sale of 50 KAS puts in 50.2076, and the ticket is the claimer's. A sale lives
200 blocks: ended then by another key the claimer has lost its bond of 0.2 KAS; a block short is refused. An
honest claimer applies from an age of 50. A claimer that disputes itself restarts the life once, no more (no
dispute lands after the last time to dispute). The default workspace: 621 tests, none failed (my run, after the
reviewer's one count was brought along). `KpfTCTok` is 3,394 bytes.

| | What it found | Decided |
|---|---|---|
| F111 (low) | a sale never applied holds an account 200 blocks for the least bond: sixty times cheaper a block than a wrong step | a sale's bond is no less than a hundredth of what it pays, and its life is a dispute period and a lateness |
| N36 (high) | four scripts ask nothing of the number 16.17 uses (`KpfTAddC`, `KpfTPass`, `KpfTCStep`, `KpfTCallEnter`), and `KpfTChange` adds it to the coins for the bond | each asks that it is nothing, but where the rule below gives an account that owes a way on. First, alone |
| N37 (high) | a note that names a token leg is applied by nothing and spent for nothing by anybody | no note names a token leg: see below |
| N38 (medium) | three outputs under the id in the call's step weigh close to what a block takes where the account that goes on holds a note's least; the step's fee is taken from the coins before the shard is paid | measured when built; the loss is paid before the fee |
| N39 (medium) | a user that owes a little is sold whole in the next block | the step that makes an account owe writes its time; a sale is stated at or after it |
| N40 (medium) | coins that reach a called shard after its claims were paid at one share | decided with `KpfTCallPay` read, when the payment's note is built |
| N41 (low) | an account that owes more than its tokens are worth owes for good | with the vault |
| N42 (low) | with the debt no account is left with no way on; the sale before any debt is one rule too many | so: a sale is of an account that owes, and of no other |

**Step 4, as it will be built** (16.17, with these, and one thing more: a step of one leg makes the account owe,
so that no step of every leg is needed and no note names a token leg):

- **What an account owes** is the number at bytes 76 to 84 of its moving part. Every script that takes a covered
  account asks that it is nothing, but: a step (kind 9), a sale (kind 15), the call's step, and the payment.
- **A step of one futures leg on an account that holds tokens** does not turn to every leg where the coins do not
  pay: the leg leaves, the coins pay what they can of what it lost (and then of the fee), and the account owes
  the rest. Default stays in steps, a leg at a time. The standing a step is due by counts what the account owes
  against it. A step of every leg is not stated while tokens are held, as built.
- **The call's step** on an account that holds tokens: the futures legs leave by a note that names them alone
  (the step reads the legs: it has their chain), the coins pay what they can, and the account goes on with its
  token legs, owing the rest.
- A step that makes an account owe more writes the account's time: the time of what it reads, a lateness and a
  dispute period.
- **A sale (kind 15)** is of an account that owes, stated at or after the account's time: the claimer pays at
  least the leg's worth less the liquidation fee's rate; the shard is paid first, as far as the account owes, by
  a note that holds the coins; the rest is the account's coins. (The sale where a step was due and the coins did
  not pay a step, as built in step 3, goes.)
- **The payment**: anyone pays what an account owes, in part or whole, by a script that writes the same note.
- An account that owes nothing is an account as any: its tokens leave by its user's statement.

### 16.19 Built: step 4, an account that owes (27 November 2026; at the engine, my tests; the reviewer's are round 64)

As 16.18 says it. `KpfTOwed` is new; the library has 68 scripts. No token enters an account yet (step 5).

- **The number.** Bytes 76 to 84 of a covered account's moving part are what it owes its shard. `KpfTAddC` (an
  account's branch) and `KpfTPass` ask that it is nothing (N36); `KpfTChange` states, of an account that owes, a
  step and a sale and nothing else, and counts no debt in the bond; the ends of a take, a leaving, a split, a
  mark, an entry and tokens leaving ask that it is nothing, as they did. (An entry's stating asked already; a
  join asks it of both.)
- **A step of one leg** (`KpfTCEnd`, kind 9) on an account that holds tokens and whose coins do not pay the leg:
  no turn to every leg. The leg leaves; the coins, a note's least apart, pay what they can of what it lost, then
  of the fee; the account owes the rest, and says the time of the statement, a lateness and a dispute period. A
  leg that gained, named where no leg lost, leaves alone too. The standing a step is due by counts what is owed
  against the account. A step of every leg is not the rule's while tokens are held, as before.
- **The call's step** (`KpfTCStep`): a token leg is passed by in the sum; the note names the futures legs alone;
  the account goes on, a third output under the id, with its token legs, what is left of its coins (no payment
  to the user's key), and owing what its futures legs lost beyond what the coins paid; where that is something it
  says the time its output landed, a lateness and a dispute period. The output holds a note's least at the
  least, which whoever steps puts in where the coins are less. An account of token legs alone is not stepped.
  It ends a sale that stands, as the other kinds.
- **A sale** (`KpfTCTok`, kind 15): of an account that owes, at or after its time; the claimer pays at least the
  leg's worth less the liquidation fee's rate; of that the shard has what is owed, by a note that holds it, and
  the account's coins the rest; the account owes that much less. Its bond is no less than a hundredth of what it
  pays (`KpfTChange`), and one nobody applies is ended at an age of a lateness and a dispute period (F111). The
  token leg named counts for nothing in the sixth sum, sold or leaving.
- **The payment** (`KpfTOwed.pay`): anyone, of an account that owes: all of it, or a KAS or more; the account
  owes that much less and the same note holds the coins. `KpfTOwed.apply`: the shard holds what the note's
  output holds above a note's least, and no more than it says; called or not.

Run (`tests/thin_draw.rs`, 40 pass): an account of a long, 10 tokens and 10 KAS, the long down 500: the step
takes the long, the coins pay 9.8, the account owes 490.2 and says its time; a second leg stepped adds what it
lost; an account that owes takes nothing out, is handed to nobody. A sale of 10 tokens of an account that owes
30: the claimer pays 99.5, 30 to the shard by its note, the rest the account's; a sompi less, an account that
owes nothing, before its time: wrong; more owed than paid: all of it the shard's. The note applied (to a called
shard too; one that holds less gives what it holds). The payment, whole and of a KAS; less than a KAS, more
than is owed, nothing: refused at the engine. A sale nobody applied ended at its life and not a block before.
Under a call: an account of a long, 10 tokens and 5 KAS at a call's price of 8: the note of the long alone, 4.8
paid, the account on with its tokens owing 15.2; written otherwise in five ways, refused; one whose coins pay
owes nothing and keeps what is left; one that owed already owes both.

Left, and said (N40): a claim is paid its share of what the shard holds when it is shown, and is then spent
whole; the call ends when nothing is owed by claims, and what the shard holds then is its owner's again. So coins
that come late (what accounts that owe pay) are shared among the claims not yet shown, and after the last, the
owner's. A holder that waits loses nothing by others going first (what is held and what is owed fall together:
the share of those who wait is as it was), and has its part of what comes late; one that goes first has given
that up. Whoever holds a claim sees the accounts that owe its shard, and waits or not. An account that owes more than its tokens are worth owes for good (N41). What the three
outputs of the call's step weigh is asked of round 64 (N38).

### 16.20 Review 64

At the engine, step 4. Of 31 steps of a futures leg the model allows on random accounts that hold token legs (a
quarter of them owing already) none ends the account and 22 leave it owing more; 24 were stated, disputed at the
end and answered: right taken, wrong not. Of 14 accounts below the lower level none had no way on. An account
that owes is refused an addition by the engine, a hand-over and a join by the builder, and the stating of kinds
7, 8, 12, 13 and 14 by the engine; a step is stated. What it owes is paid by a key that is not its own, with no
signature: all, a KAS, two; less than a KAS that is not all, more than is owed, nothing: refused. A sale of an
account that owes 8 KAS for 50: 8 to the shard in its note, 42 to the coins, the ticket the claimer's, the bond
(a hundredth: 0.5 KAS) back; not applied, ended after 150 blocks. The note applied, to a called shard too. The
call's step: three outputs under the id, nothing to the user's key, and the account it leaves is sold from at
its time and paid for. The default workspace: 625 tests, 297 seconds; with the features 1,319, 963 seconds; none
failed. `KpfTOwed` is 1,154 bytes, `KpfTCStep` 4,001 (it was 3,257), `KpfTCEnd` 3,726, `KpfTCTok` 3,492.

| Finding | What it is | Decided |
|---|---|---|
| F112 (medium; by the rule, not run) | a gain is paid out of the shard to a user whose account owes the shard: a leg that gained, stepped off an account that owes 20, is paid 15 to the user's key | what a leg that gained asks is set against what the account owes, in a step and in the call's step (16.21) |
| F113 (medium; measured) | the call's step on an account that goes on with no coins weighs 483,526 of the 500,000 a block takes (291,533 where the note holds what was paid); with a bond paid out beside, more | an account that goes on keeps a KAS of its coins back and owes that much more; its output holds a KAS at the least (16.21) |
| said | a stepper that names a small leg that lost first takes its fee out of coins a larger loss would have had whole: the shard is paid less by that fee at the most | left: bounded by a fee |
| said | where the coins are gone a step has no fee: the shard's owner is who has a reason to step | so |
| N40 | a claim paid at a share is closed, and the call's end gives what is left to the owner | said as it is in 16.19: whoever waits shares what comes late |

### 16.21 After review 64 (27 November 2026; at the engine, my tests; the reviewer's are round 65)

- **F112: what a leg that gained asks is set against what its account owes.** In a step (`KpfTCEnd`, one leg and
  every leg) and in the call's step (`KpfTCStep`): of what the legs ask of the shard, as much as the account owes
  is not asked, and the account owes that much less. Nothing is paid to a user's key while its account owes.
- **F113: an account that goes on from the call's step keeps a KAS.** `KpfTCStep`, on an account that holds token
  legs: a KAS of its coins is no coin to pay with (it owes that much more) and no fee is taken out of it; its
  output holds a KAS at the least, which whoever steps puts in where the account had less (an account of a
  note's least alone). The three outputs under the id are then the call (half a KAS), the note and a KAS.

Run: a long that gained 15 KAS stepped off an account that owes 200: its note asks nothing, the account owes
that much less (the band apart), nothing to the user's key; an account that owes 2 is no step's. Under a call
at 12 an account that owed 7 and whose leg gained 20: the note asks 13, the account owes nothing; one that owed
50 owes 30 and its note asks nothing. An account of a long, 10 tokens and 5 KAS at a call's price of 8: 3.8
paid, the account on with a KAS and its tokens, owing 16.2; one of a note's least alone: nothing paid, a KAS put
in, 20 owed.

### 16.22 Review 65

At the engine: F112 is closed (an account that owes 60 and whose long gained 15: the note asks nothing and the
account owes 46.15; under a call at 11.5 an account that owes 20 keeps on owing 5, one of the long alone ends
with its note asking nothing, one that owed 6 asks 9). F113 is closed for the account: it holds a KAS in every
case, and a step that pays the shard something weighs 133,533 (it was 291,533). The default workspace: 627
tests, 203 seconds; none failed. `KpfTCEnd` is 3,832 bytes, `KpfTCStep` 4,113.

| Finding | What it is | Decided |
|---|---|---|
| F113, what is left (low) | a call's step that pays the shard nothing writes its note with a note's least alone: 323,526 of the 500,000 a block takes (it was 483,526); with a statement's bonds paid out beside, not measured | left: one such step a block. Whoever steps may hold the note to more (an output holds at least what the script says) |
| F114 (medium) | what an account pays after the last claim of a called shard was shown goes to the owner, not to the claims: a shard of 100 KAS against claims of 200, an account that owes 60: every claim paid a half and spent; the 60 that come later are the owner's by the call's end. Before accounts that owe, nothing came late to a called shard | **open.** A claim paid at a share and not spent, the rest staying owed, pays whoever is first more than whoever is second (what is held and what is owed no longer fall together), so it is no fix. The fix is a share of all that was ever held: a claim is paid up to its part of what the shard has paid and holds, which needs one number more in a called shard (what it has paid). It is built with the call as section 17 rewrites it; until then a claim's holder that sees accounts owing its shard waits (16.19), and the last to wait gains nothing by it |

Not reached by the review, of what round 64 left: the call's step on random accounts and a called shard brought
whole to its end with accounts that hold tokens; the fees a step may say; an owing account at the ends of the
other kinds (by reasoning there is no way to one: a statement stands on the account as it was, and an account
comes to owe only by an end or a call's step, each of which writes a new account); sales on random accounts; a
payment racing a sale.

### 16.23 Built: step 5, the vault (27 November 2026; at the engine, my tests; the reviewer's are round 66)

`KpfTAddSpot` and `KpfTVaultPay`; the library has 70 scripts. With them tokens enter an account and leave it: the
goal's row "a token held in the account counts at a discount" runs whole at the script engine, beside the
reference KCC-20's own script (`vendor/kob`: the token's program, its state and its transfer were already in this
crate from the order book's evidence; an input's plan takes the token's program as it takes a script of the
library).

- **Tokens enter** (`KpfTAddSpot.add`), as 16.12 (d) says, with one thing learned at the engine: **a covenant's
  counts do not see an output that is born.** "No input under the vault's id and one output" cannot be asked by
  counting. The script recomputes the id as the consensus derives it for a genesis of that one output (the hash,
  keyed "CovenantID", of an input's outpoint, the count one, the output's index, value and script), and holds the
  output's id to it: so the id is new (an outpoint is spent once), it is the id of nothing else, and a second
  output under it would have made another id (N24). The script is told the input whose outpoint it is; the
  funder's coin.
- What it holds the transaction to: the account owes nothing, has a second key and fewer than six legs; the leg
  is written as a leg is, of direction 7, with an amount, a scale and a price limit its worth is counted at; its
  discount is the liquidation fee's rate and the tolerance twice at the least; its market is the hash of the token
  (N25); it names the vault's id; the vault's state says the id the account is under (N23), the token and the
  amount, under a tag of its own (N27); a token input shows the program, held to the template's hash; the tokens'
  output is that program with the vault's amount, owned by the vault's id, borrowing off, under the token's id;
  the account's output has the leg at the end of its chain and a leg more counted; an input landed at or after
  the account's time, which is written; the user and the shard's owner sign.
- **A ticket is paid** (`KpfTVaultPay.pay`), as 16.12 (e): the vault's state; the leg shown hashes to the ticket's
  chain, is a token leg, names the vault's id and has its amount; the ticket is an input under the id the vault
  says; one input under the vault's id and no output; one input under the account's id (the ticket) and no
  output; the token input is the program, owned by the vault's id (scheme 4), borrowing off, of the vault's
  amount and extension commitment; the tokens' output is that program with the amount, owned by the ticket's key.

Run (`tests/thin_draw.rs`, 42 pass): of a user's 150 tokens 100 enter an account of one long at a discount of 20
% (6,446 bytes): the vault under an id of its own, the tokens owned by it, 50 back to the user; the tokens'
output written to another owner or of another amount, or the account without the leg: refused. They count: the
user takes its coins out, which the long alone would not allow. They leave by the user's statement (3,342
bytes stated, 2,643 applied), which writes the ticket; the vault pays it (5,403 bytes): the tokens owned by the
user's key again, nothing under the vault's id; to another key or fewer: refused; a ticket of the same leg to
another key (a sale's claimer) is paid to that key; a ticket of another leg, or under another id than the vault
says, pays nothing (the tool; by hand at the engine is round 66's).

Not built: the commands; a deposit beside a fill of the order book's ask (16.12 (g): the places are free, not
run); the entry that binds a vault; tokens of another program than the reference's (the template's hash and the
two lengths are the vault's to say, but the tool makes the reference's only).

### 16.24 Review 66

At the engine, with the reference token's script running: one deposit (6,446 bytes; storage mass 124,873; the
vault and the tokens' output a KAS each), one payment (5,403 bytes; 43,216; built and signed by a key that is
neither the user's nor the owner's), and 34 transactions rewritten by hand, all refused: a second output under
the vault's id, the vault holding a sompi more than hashed, the input the id is of said to be each other input,
one beyond the last or below nothing, the vault's output said to be the tokens', the account's, the copy's or
none; the tokens' output under no id, with borrowing on, of another extension, owned by the account's id or by
the user's key, a unit fewer or more; the leg of direction 1, of a discount under the floor or over the whole,
of a unit more, of another market, of another vault's id, of scale nothing; without the owner's signature or the
user's; the payment's tokens to another key, a unit fewer, owned by the vault's id again, under no id; a ticket
under an attacker's own id (N23), of another key, of another leg. N23 and N24 are closed at the engine. The
preimage of the vault's id matches the node's field for field. Whoever pays a ticket has 1.187 KAS more after
(the vault's KAS, the tokens' carrier, the ticket's least, less the fee): a ticket is paid. `KpfTAddSpot` is
1,466 bytes, `KpfTVaultPay` 955.

| Finding | What it is | Decided |
|---|---|---|
| F115 (medium) | the library's genesis is over what a block takes: with 70 scripts two copies at 11 KAS a copy weigh 509,043. The worlds of three of my test files made that library, and 32 tests failed (I had run my own file and a compile after step 5, not the workspace) | the least a copy holds follows the count of scripts (`lib_copy_least`: an output under an id weighs 4 x 10^12 over its value; 7 KAS for one copy of each of 70 scripts, 13 for two). The workspace again: 629 tests, none failed. Every script added raises it: at 100 scripts two copies are 18 KAS each |

**Not yet reviewed, and said so by the review:** that no script of the library but `KpfTVaultPay` can be made to
name a vault and pass (N27: every entry listed by the places it names and the state it holds each to); the
bearer rule with a real vault (a split, a join, a hand-over, a step, a sale and a call's step, with the vault's
payment at each end); the time of a deposit; and, read and not run: no ticket, the same ticket twice, a ticket
and a vault of different amounts, a continuation under an existing vault's id as the output that is born, a
look-alike token program, the token's delegates, legs of other lengths, an account that owes or has six legs.
Round 67.

Stuck for good, by reasoning: the tokens of a leg that left by a wrong statement nobody disputed (no ticket was
written: its user's loss, as with any wrong statement); the tokens of an account that owes more than they are
worth (N41).

### 16.25 Review 67

N27, the proof of custody, as a test that reads the sources of all 70 scripts: every line that holds an input's
slot to a state is sorted by how that state's preimage begins (an account, a shard or a position: a push of 32
bytes; a statement: the number 10; a disputed point: its own tag; a note: 12 and then a push or a kind that is
not 16; a ticket: 12 and 14; a vault: 12 and 16). Only `KpfTVaultPay` holds an input to a vault's form; nothing
is unsorted; no script reads a slot and holds its state on no line; and the test fails where any of the three
stops being so. At the engine a real vault put in the account's place of an addition, a hand-over and a payment
of what is owed is refused, each beside its honest transaction. It sorts lines, not entries and places: with the
older test that every entry names its places it covers most of N27, and is not the list by entry that was asked.
The default workspace: 630 tests, 170 seconds; none failed (F115's fix holds).

Not done by round 67, whose reviewer had reached what one session carries: the bearer rule with a real vault
(a split, a join, a hand-over, a step, a sale, a call's step); the time of a deposit; what round 66 read and did
not run; and section 17. They go to two reviewers that start from the review file: one at the engine (the
vault), one on paper (section 17).

### 16.26 Review T2 of the vault

A reviewer that started afresh (`docs/audit/REVIEW-T2-vault.md`; its tests `auditT2_vault.rs`, nine). **No way
to anybody's tokens in what it ran or read.** At the engine, with a real vault born by a deposit and the token's
own script running:

| The account, after its deposit | Who the vault pays | |
|---|---|---|
| is split (a futures leg leaves, the token leg stays), and its tokens leave | the user | the vault ends; nothing goes on under its id |
| is handed over, and the taker states the leaving | the taker | the old user's statement is refused |
| is stepped until it owes, and sold from | the sale's claimer | the note of what was paid is applied to the shard; paid for instead, the tokens leave to the user |
| is stepped under a call (it goes on with its token leg, owing), and sold from | the claimer | the note names the futures leg alone |
| two deposits, two vaults | each ticket its own vault | a ticket of one leg at the other's vault is refused |

Refused as well: a deposit funded before the account's time, and the second of two in a row until its time; a
token program of the same lengths with one byte of another body (the template's hash), and a script that is not
the output's; an amount of nothing, of 2^63 - 1, a discount under the floor or over the whole; an account that
owes, and one of six legs; a vault, a leg and tokens of different amounts; the payment's tokens to another key,
fewer, or owned by the vault's id again.

| Finding | What it is | Decided |
|---|---|---|
| V1 (low) | a split that names a token leg is wrong at its row (`KpfTCRow`) and not at its end: undisputed it moves the leg to a new account of the same user under the same id, the vault bound as before | said as it is: a wrong statement nobody disputes, and nobody's tokens move by it |
| V2 (low) | a deposit holds its funding coin to the time the account has, not the one it writes: an account that never waited takes a deposit at once | as meant: more margin is safe at once (16.12 (d)) |
| V3 (low) | the vault's and the tokens' carriers, about 1.2 KAS, are whoever's pays the ticket | as meant: it is why a ticket is paid |
| V4 (low; read) | a second token input a payer adds beside the tokens is the payer's own | nothing to decide |

**Read and not run:** a true join of two accounts that each have a token leg; a note that is no ticket at the
ticket's place; the output that is born said to be under an existing vault's id; the token's borrowing on what a
vault owns; the marked account and a shard in the account's place. The default workspace: 639 tests, none failed.

## 17. Options under a shard, and the books in a tree (27 November 2026; third writing, on paper; a model in numbers, no script)

The goal's first row is that everything a user holds (spot, futures, options) is netted in one account, behind one
bond, and that margin is asked for the net risk only. Futures and spot are there (sections 13 to 16). Options are
legs of an account no shard covers (4b). This section is the form that puts them under a shard. The user's
decision of 27 November: the form that holds the goal, not a smaller one.

### 17.1 The first writing, and what its review decided

The first writing (commit 86c346e) put the premium in the option's entry, dated a market by its name, asked a
writer the maintenance of the whole underlying, and kept the shard's rows in a tree by a hashed name. Its review
on paper (`docs/audit/REVIEW-T2-options.md`, N43 to N64) found it **not sound to build from**: four ways to lose
users' coins by the rule as written, with the owner's own keys only, and nothing netted.

| Finding | What it was | Decided |
|---|---|---|
| N43 (high) | an option's entry was a free number, and moved the shard's equity at once: an owner's own account writes one call entered far from the price, and under a call claims a hundred times what the shard holds | **an option's entry is its market's price, as every leg's**; the premium is a number of its own (17.3) |
| N44 (high) | the books counted a premium nobody had paid as equity: the owner draws on it, has itself called, and the forfeit says it is not owed | **the books never know a premium** (17.3) |
| N45 (high) | "what it lost is not paid" forgave a real loss, since one entry could not tell the premium from what the option was worth | the premium apart; what an option gained or lost exercised is settled as a futures leg's, always |
| N46 (high) | a holder was asked the rate on what its option was worth: at a price above its entry it took the premium out | **the premium is asked whole while the leg stands**, whatever the price |
| N47 (high against the goal) | nothing was netted: a covered call was asked twice its risk, a spread three and a half times | **two sums a group of legs of one underlying** (17.4), for futures as well |
| N48 (medium) | a call ends every option at what it is worth exercised, and an owner makes a call due at will | said as it is (17.3); a deterrent later |
| N49 (high for a shard with an ended row) | "the committee signs one price for ever" was read by no script | **the price of a day is written once**, as a note (17.5) |
| N50, N51 (medium) | an ended leg left by a statement, which a leg that gained beside losers cannot; nothing refused an entry on a market that had ended | an ended leg leaves by one transaction of anyone's; no entry within a statement's life of the day |
| N52 (medium) | a dated future is a perpetual with a stop | **futures have no day; the day is in the option leg** |
| N53 (medium) | the owner's exercise by statement: it picks the price's edge and the time | **no exercise before the day by anybody** (17.5) |
| N54 (medium) | the band of an option, of the underlying's worth, took half of a premium of 2 % | the band of an option is what its worth exercised changes by where the price is moved the tolerance (17.5) |
| N55 (high for the call) | rows of one underlying had a price each, and a path cannot show that a market has no second row | **the leaves are a list in order of a plain key; one price an underlying in every trace** (17.6) |
| N56 to N58 | an empty leaf; rows that die each day; sizes | 17.6, 17.7 |
| N59 | what 17.4 did not say of a step | 17.4 |
| N60 | F114 written out | 17.7 |
| N61 | an option cannot be sold back | said as it is: a split and a hand-over; a leg that cancels another, later |
| N63 | the order would have had seventeen scripts written twice | the model first (17.8) |

One thing the review saw of the form that is built (N56, its last paragraph) is 17.9.

**The second round** read the second writing (N65 to N77): nothing of N43 to N46 is left, no premium is counted
twice and none is had early; **sound enough to start the model**, with five rules to go into it first.

| Finding | What it was | Decided |
|---|---|---|
| N65 (high) | with what is asked netted, a leg taken out of a group leaves the rest asked nothing: a long and a short of 10,000 units on 10,100 KAS, the short leaves by its user's statement (the end asks only that the coins pay), and a long worth a million stands on a fifth of what an entry asks. A step that names one leg of a pair does the same | **the account that stays is tested as it stays** (17.4) |
| N66 (high) | an ended leg's gain paid to the user's key with nothing asked of what stays: S2 through the day | an ended leg that gained leaves only where the account stands without it (17.5) |
| N67 (high) | "in no statement until its ended legs have left": no step until someone has the day's price written; a shard uncallable from each day until its note exists | **nothing is frozen**: an ended leg is passed at its day's price (17.5) |
| N68 (high) | a hedge ends by the clock: a long and deep puts whose day is tomorrow are asked the premium, and at the day the long is asked whole and has nothing | within a time T of its day an option gives no credit (17.4) |
| N69 (medium) | the rule for a held option had a cliff at its strike (found by the model the same hour: a put bought against a long was given nothing) | a held option is counted along one of its two lines (17.4) |
| N70 (medium) | a step that names a worthless written call takes its premium and leaves the account lacking as much; "a premium received may be margin" is not safe (one user, two accounts) | closed by N65's rule; a premium received is never margin |
| N71, N72, N73 (medium) | a held option in a step of every leg; the fallback pays whoever is out of the money for a committee's silence; a draw over an ended row | 17.4, 17.5 |
| N74, N75 (medium) | what a script still needs said of the leaf and the trace; a row needs more than three sums | 17.6; the row's sums are found in the model |
| N76 (high; of the form that is built) | 17.9 holds by reading, and a band under a call is not enough | **a leg says the time it entered** (17.9) |

**The third round** read the model and added to it (`review_t2_*`, eight tests): faithful where it claims to
be, and its claim stands against a stronger owner and stronger users (the owner pays its shard up after its own
entries, so that runs reach the days; every price stated at the tolerance's edge; half the users know the next
price and all take every tick; levels counted at exactly what can be lost): nobody unpaid on 1,000 paths a set,
eight sets, ticks of 0.5 % to 4.5 %.

| Finding | What it was | Decided |
|---|---|---|
| N78 (high) | the rule of a young leg, said of a leg, takes a hedge apart: a long and a short of 1,000 units entered together and called a tick later at 101: the short pays 1,000, the long asks nothing | **the young legs of an account are counted together** (17.9) |
| N79 (medium) | the claim does not count what a young leg gives up at a call | it is nothing, and no second claim; said beside the claim (17.9) |
| N80 (high near a strike) | "the stater takes the line that asks less", and a step's stater is anyone: by the lines that ask most, 15,152 of 25,383 accounts with a held option between the ends were due while holding half as much again as their least lines ask | **a step is due by one price that shows it** (17.4) |
| N81 (medium; of the form that is built as well) | a book set off, one side of which leaves between two visits, is held by twice the rate on the gross: at its lower level to a move of 2.5 %, not 4.5 % | a number to set with the rates, later; the test is the one to set it against |
| N83 (medium) | "the hedge by the clock is a user's play" was an artefact of short runs: with an owner that pays its shard up, without the day's rule users are unpaid on 131 paths of 1,000 at ticks of 4 % | the rule is needed against the owner too; the shard's requirement counts no credit near a day |
| N84 (low) | without the rate on the gross of option rows nobody is unpaid to 4 %; it is the distance from a call | kept |
| N65, confirmed | one leg of a pair leaving as the end is built today: users' accounts unpaid on 593 paths of 1,000 | under 17.4's rule: none |

**The edge of the claim**, found: an account, the lower rate less the tolerance (4 % with these terms; the first
failure in most sets at 5 % to 6 %); no earlier edge near a strike or near a day.

### 17.2 What an option leg says

An option leg is a leg of its underlying's market, priced by that market's committee as a futures leg is. Beside
what a futures leg says it says: what it is (a call or a put; held or written by the user), its **strike**, its
**day** (a time: the option is of that time's price and of no other), and its **premium a unit**. Its entry is
the market's price where it entered. **Every leg of a covered account, futures as well, says the time it
entered** (17.9): the time of the statement by which it entered; a split, a join, a hand-over and a mark carry
it with the leg. The four numbers are 32 bytes, and lie where a leg of an account no shard covers has its committee: a shard
has one committee, in its id, and it is shown apart wherever a price is answered (17.9, built for futures legs).

- What it is worth exercised at a price: for a call the price less the strike, or nothing; for a put the strike
  less the price, or nothing; by its units.
- What it gained: for the holder its worth exercised at the price less the same at its entry; for the writer the
  reverse. A premium is in neither.
- A futures leg has no day.

### 17.3 The premium is coins in the user's account, from the entry to the leg's end

**Whoever pays a premium puts it into the user's account when the option enters, and it lies there until the
option has left.** The premium of a leg is its premium a unit by its units.

- **The user holds the option.** Its own coins stand for the premium: the leg's row adds the premium to what the
  account is asked, whatever the price. When the leg leaves (at its day, or in a step) the premium is paid to the
  shard with what the leg lost, or set against what it gained.
- **The user wrote it.** The shard's owner puts the premium into the account in the transaction that states the
  entry, from coins of its own (not from the shard). They are held as a holder's are: the row adds the premium to
  what is asked, so no take reaches them while the leg stands. At the day they are the user's. Where the entry
  ends unapplied they return to the owner's key.
- **In a step of the user** (its own default): an option it holds leaves, and the premium is paid as far as the
  coins go, after what the legs lost. An option it wrote leaves, and the shard is paid the larger of the premium
  and what the option lost: whoever ends an option early by its own default does not keep its price. (A step
  that names a written option worth nothing only to have its premium leaves the account lacking as much as
  before, and is wrong by 17.4's rule of a step.)
- **Under a call** (the clearing side's default) no premium moves. A holder keeps its own; a writer keeps what
  the owner paid. What the option gained or lost exercised is settled as a futures leg's.
- **The books** take an option at what it is worth exercised at its entry, by its units. No premium is ever
  equity of a shard: a holder's premium reaches the shard as coins, when the leg leaves.
- **A premium a user was paid is never margin for its other legs** (a spread is asked the premium it pays,
  beside the one it was paid). With one user's two accounts it would be: the premium of one pays that one's
  loss to the shard, and the other is paid the same by the shard.

**Said as it is (N48).** An option under a shard is good while the shard is not called. A call ends it at what
it is worth exercised; what it was worth beyond that is lost to its holder, who keeps the premium. An owner can
bring a call about (no gate is computed at an entry: 14.3). It then gives up the premium and every other account
of its shard, and pays the call's price. A deterrent, later: what is left of a called shard after its claims
goes first to the users whose options the call ended, up to the premium of each, and only then to the owner.

### 17.4 What an account is asked: two sums a group

Today an account is asked the rate on what all its legs are worth (W): a long and a short of one market are
asked both. With options that is twice the risk or more for what options are held as. **What an account is
asked is what it can lose by a move of the rate, legs of one underlying counted together.**

- A statement says the rate it computes with (the end holds it to the terms). For a row at the price a:
  L is a less the rate, U is a and the rate.
- Each leg that stays adds to two sums what it gains from a to L (D-down) and from a to U (D-up):
  a futures leg its worth by the rate, one way and the other; an option the user wrote, exactly what it is worth
  exercised at each end, against it; an option the user holds the same where its strike is outside L to U.
- **A held option whose strike is between L and U is counted along one of its two lines, as the row says**: as
  worth nothing exercised wherever the price goes, or as exercised wherever it goes. Both lie below what it is
  worth, so either is safe; in a user's own statement (an entry, a take, a leaving, a split) its stater takes
  the one that asks less. (Counted at its worse end only, a put
  bought against a long was given nothing, and what an account was asked jumped as the price crossed a strike:
  N69.)
- **Within a time T of its day an option is counted in the two sums only where it adds to what is asked** (N68):
  what it would lose at each end, never what it would gain. T is one number, of what the terms have: twice a lateness and a dispute period (14.4's
  bound, a lateness for the time between two visits). It is 17.9's T, and no option enters within T of its day. A hedge so stops counting while
  both legs still stand, and a step that takes the group takes it at its net. (In the model
  without it: a user's such account leaves the shard unpaid on 955 paths of 1,000; an owner's own, where the
  owner keeps its shard paid up, leaves users unpaid on 131 at ticks of 4 %. The shard's requirement counts no
  credit near a day either.)
- **A group** is consecutive legs of one underlying. A leg whose underlying differs from the row before's closes
  the group: R grows by the larger of nothing, what D-down lost and what D-up lost. Rows of one underlying say
  one price, or the row is wrong.
- R takes the place of the rate on W: in the test of an entry (the coins and what all gained at least R and the
  premiums), a step is the rule below. A take and the leaving of
  a leg that gained keep their second part as built: the coins at least R, the premiums and what each leg lost
  counted alone. Netting is of what is held, never of what leaves.
- **The account that stays is tested as it stays** (N65). Today a leg that lost leaves where the coins pay it:
  without it the account was asked less. Netted, it may be asked more. So every statement that takes a leg out
  and leaves others (a leaving of one leg, that gained or that lost; a split's two accounts) holds what stays to
  the level of an entry: the coins and what the legs that stay gained at least R of those and their premiums.
  **A step of some legs is right only where the account lacks less after it than before it**; else every leg
  leaves. Whether a step names a leg or a whole group is decided by that and by no rule of its own.
- **A step is due by one price that shows it** (N80). Anyone states a step, and would choose for a held option
  the line that asks most. So a step's statement names a price within the lower rate of the row's; the rows sum
  what the legs gain from the row's price to it; and the step is due where the coins, what all gained and that
  sum are below the premiums. Exact, one sum, and nothing to choose against the account. A call's test of a
  shard is the same, over its rows.
- The band stays of what the legs that leave are worth; a step's fee keeps its base of today (the worth of the
  legs named: what is asked is nothing for a hedged group).
- An entry says the place its leg takes among the legs, so that a hedge stands next to what it hedges. Legs of
  one underlying that are not next to one another are not netted, which asks more and never less.

Why it never asks less than can be lost: what the legs of a group gain at a price is a line with corners at
the strikes. Futures and written options bend it downward only, so their sum is least at one of the two ends. A
held option bends it upward and the lowest point could lie between: so it is counted along a line below it.

| Account (ten units, the price 100, the rate 5.5 %) | Asked today's way | Asked so | The most a move of the rate loses |
|---|---|---|---|
| A call held alone (strike 100, premium 5) | 50 | 50 (the premium), at any price | 50 |
| Long futures and a call written at 110 (premium 2) | 110 | 55, and the 20 the owner paid lie in the account | 55 |
| Short futures and a put written at 90 (premium 2) | 110 | 55, likewise | 55 |
| A straddle held (call 5, put 4, strike 100) | 90 | 90 | 90 |
| A call held at 100 (premium 5), a call written at 110 (premium 2) | 105 | 50, beside the 20 it was paid | 30 |
| A long and a short of one market | 110 | nothing but what each lost alone | nothing |

A row of a covered account's trace grows by the two sums, R, and the premiums that stay and that the legs
leaving pay: nearer 160 bytes than 120; found in the model. Six legs an account, as today. The table and the
rule are run in numbers (`research_options.rs`): on 400,000 random groups what is asked is never less than what
a move of the rate loses, and exactly that where no held option has its strike between the ends.

**A step** (N59) names legs under the rule above. An option named by a step leaves at the step's price: what it
gained or lost exercised from its entry, and its premium as 17.3 says. A user's leaving of every leg is wrong
while it holds or wrote an option: that would be an exercise before the day. Whether an option whose premium
the coins hold goes on through a step of every other leg is found in the model (N71).
A mark (16.3) names no option. A split and a hand-over move an option with its premium: that is how one is sold
(N61).

### 17.5 The day

**An option is of its day's price, and leaves on no other.** Nobody exercises before the day: not the holder, not
the writer, not the owner (N53). Before its day an option leaves only in a step, under a call, or with its
account.

- **The price of a day is written once.** Anyone writes the note of a market's price at a day, from the
  committee's signed price of that time (the citation, as a disputed price is answered). The note stays; every
  script that needs the day's price is shown it.
- **An ended leg leaves by one transaction of anyone's**, shown the note: no statement, no dispute period (the
  price is written; the script computes the one leg). **One that lost always leaves so**: the coins pay what it
  lost and its premium, and what they do not pay the account owes (16.18). **One that gained leaves so only
  where the account stands without it** (the test of a leaving: the coins at R of what stays, the premiums and
  what each leg that stays lost alone); else it stays, at its day's price, as a gain that counts for the
  account's standing and is no coin, and leaves when the test passes or with every leg (N66: S2, through the
  day). Whoever sends the transaction has a keeper's fee from the account.
- **The band** of an option is what its worth exercised changes by where a price is moved the tolerance: at its
  entry (nothing for an option that entered further from its strike than the tolerance), and at a leaving at a
  stated price (a step). At the day's written price only the entry's part is taken.
- **No entry** of an option within a statement's life of its day, or after it.
- **Nothing is frozen** (N67). Between its day and its leaving a leg is passed by a row at its day's price: it
  adds what it gained or lost there, and nothing to the two sums. That price is answered by the day's note, or,
  where none is written yet, by a citation of the committee's price of the day (what the note would be written
  from). A step, a take and a call's step go on over it.
- A shard's row of an ended day is counted at its day's price in a draw's and a call's trace, answered the same
  way. Where nobody has written the price, a call's trace counts the row at what its legs were entered at (a
  leaf holds that: 8 bytes), which is what the fallback pays.
- **The fallback.** Where no price of a day was written by a time the terms say after it, an ended leg leaves
  at its entry, and its premium returns to whoever paid it. It pays whoever is out of the money for the
  committee's silence (a call written at 100 whose day was at 130: 30 a unit not owed), and the owner names the
  committee: the time is long, and it is said to a user beside 14.7's lines (N72).

### 17.6 The books in a tree

**A shard's rows are the leaves of a tree, in the order of a plain key.** Today a shard's four rows are written
whole (408 bytes) in every transaction that shows it. Instead the shard holds the root; a script that reads or
changes a row is shown the row and its path (six hashes: 192 bytes).

- **The key** of a row is written plain, not hashed, 78 bytes: the underlying's market, the scale, the price's
  limit, the day, what it is (futures; calls; puts) and the strike. No committee: a shard has one, in its id
  (17.9). Futures: a day and a strike of nothing. Keys are compared as numbers of 8 bytes (the market's 32
  bytes as four), so that the rows of one underlying are next to one another, its futures first. **The key
  of nothing is below every key by rule, not by its numbers**: eight bytes of a market's id with their top bit
  set read as a number below nothing, and such a key would be below the head's (found by the model:
  `books.rs`, not yet the form of any script).
- **A leaf** holds its row (the key, the clearing side's net units, the gross, what its legs were entered at)
  and **the hash of the key of the leaf after it**: 147 bytes hashed. The key after is shown plain only where a
  row is put in or taken out. A leaf and an inner node are hashed under different tags; an empty leaf is one
  value. **A head leaf**, of the key of nothing (below every key: a scale of nothing is no row's), is in every
  tree from its opening: a tree is never empty and every row has a leaf before it. A row's place among the
  sixty-four is whichever is empty: the order is the keys', not the places'.
- **The books' hash** takes the place of the hash of the four rows in a shard's state: the hash of the sum of
  all rows' gross units, of the root, and of the hash of the claims' numbers (17.7, 17.10). No byte of the
  shard's moving part changes, and a script that touches no row carries 32 bytes as today.
- **A new row** is shown the leaf before it (two paths, the second checked against the root as the first
  rewrote it) and written between: so no key has two rows. A row of no unit is taken out by anyone, shown the
  leaf before it, by a script of its own (a note's applying stays one path). Rows of ended days leave the tree
  so.
- **The sum of the rows' gross units** is in the books' hash (not a count of rows: a script that missed one
  crossing of nothing would leave a count wrong for ever; review T4). A call ends where it is nothing.
- The depth is a number of the shard's terms: six for the first form (sixty-four rows: an underlying of three
  days and ten strikes is sixty).
- What a row is worth to the shard at a price is its net units by what a unit is worth there. What the legs
  were entered at stays one number over all rows (an option at what it was worth exercised at its entry).
- **The requirement** is the two sums of 17.4 over the rows of one underlying, which the order puts next to one
  another, and the rate on the gross as today.
- It takes away the limit of four markets a shard.

Sums in the inner nodes (what all calls of a day are worth at a price by one path) were considered: not for the
first form. The ordered list is what they would be built on.

| What a script does | Shown | Bytes (today: the four rows, 408, always) |
|---|---|---|
| reads or changes a row that is there (an entry on a market the shard has; a note applied) | the leaf's 147, its place, six hashes (192) | about 350 |
| puts a row in (an entry on a new key) | the leaf before and its path, the key after it plain (78), the empty place's path against the root as rewritten | about 630 |
| takes a row of no unit out (anyone; a script of its own) | the row and its path, the leaf before and its path | about 700, a transaction of its own |
| touches no row (a top-up, a draw's end, a claim paid) | the count, what was paid, the root | 50 |

A shard of up to four futures markets so pays about 60 bytes less where a row changes and about 220 more where
one is new. **Open, for the review on paper:** `KpfTShardPrice` reads a row's committee for its citation today:
with none in the key it opens the shard's id by the seed (an argument more). A call's object holds four heads
and four prices today (368 bytes): with a tree it holds the root of the call's trace, and `KpfTCStep` is shown
a row of that trace by a path for each underlying of the account's legs (about 280 bytes an underlying, where
it reads 8 bytes a leg today). Who pays for taking dead rows out, at sixty-four rows.

### 17.7 A draw and a call are traces; what a called shard has paid

With any number of rows no script reads them all. **A draw is stated as an account's statements are**: a trace
of a row a row of the books, in the tree's order, with running sums, and the end held to the rule. The
statement holds the trace whole, as an account's does, and a list of its prices in order (a market, its limit,
a day, the price): a trace held by its root would be disputed blind (17.10, T9).

- A trace's row is the leaf the row before named as next: the first is the leaf the head names, the last names
  none, and the trace says its count. A row left out or named twice is wrong at the row after it.
- **One price an underlying**: a row whose underlying is the row before's and whose price is not, is wrong
  (unless its day has passed: 17.5).
- A dispute names the end, a row (answered with its leaf's path) or a price. A draw is held up for its lateness
  and two dispute periods at most, at a bond a round, as today.
- The call's object holds the list of the call's prices in order (one a market, and one for each ended day): a
  call's step reads a leg's price from it at the place its stepper names, and the list has one price for it
  (17.10, T10).

Sizes, estimated and not measured (N58): a draw stated about 3.8 kB at any count of rows; a row answered about
5 kB; today's draw is 3.2 kB.

**Two classes of claim (review T3, L1 and L4).** A claim is of the first class, or of the second: what the young
legs of an account gained (at a call's step, where today it is nothing; at a leaving or a mark stated after the
call's time), less the band of those legs. The second class is paid after every claim of the first is paid
whole, of what is left, at one share among themselves. An owner that brings a call about to be rid of what its
newest users gained is not rid of it; and a leg of its own, young, takes nothing from a claim of the first
class, and from the second only what the price moved beyond the band while the call was due.

**What a called shard has paid (F114, N60).** The shard, called, has: H, what it holds; C, what it owes by
claims, which does not fall when a claim is paid; D, what it has paid to claims. T is H and D: it does not
change when a claim is paid and grows by every coin that reaches the shard.

- A claim is a note of its key, its amount A, and a, what it has been paid.
- Where T is at least C the claim is paid A less a, and spent. Else its part e is A by T over C (in the unit
  `KpfTCallPay` has, chosen from C; C rounded up, T down), and it is paid the smaller of e less a and H, and
  nothing where e is not above a. H falls and D and a rise by what is paid; the note is written again.
- A part of less than a note's least is not paid.
- No claim is paid before the call's time, a lateness and two dispute periods: no statement of before the call
  is still to be applied.
- The call ends where the count of rows is nothing and D is C. A shard that never holds enough stays called,
  its claims standing: that is what makes late coins the claims' and not the owner's.

### 17.8 The order

1. **The model, no script** (`research_options.rs`, eight tests). The option leg with the premium beside it;
   the two sums with the two lines and the time before a day; the account that stays tested as it stays; and
   **a shard over time**: users with the accounts options are held as, an owner with accounts of its own keys
   who knows the next price (a leg of any size the way it goes; a hedge that ends by the clock, its own or a
   user's; options of any premium; a take and a draw of all the rules give, the draw again after the price
   moved and before anybody steps an account), a day that falls between two visits, and whoever watches once a
   tick. On 15,000 paths (five sets of plays, ticks of 0.5 %, 2 % and 4 %) **no user is left unpaid and no
   account leaves the shard unpaid**. Without a leg's time (as the form that is built) users are unpaid on 443
   paths of 1,000; without the day's rule the shard is unpaid by a user on 955. The review's third round added eight
   tests to it (17.1). **Shown since (28 November 2026): a step by a price that shows it**
   (`a_step_is_due_by_one_price_that_shows_it`: on 150,000 accounts a step is due by some price exactly where
   the account, after some move of the lower rate, has less than its premiums; an account no price shows due
   has its premiums after any such move; a step due by a price is due by the two sums with the lines that ask
   least as well, which step 580 accounts more than can fail to pay; and of the accounts that the lines that
   ask most would step on half as much again as they can lose, 8,244, none is due by any price). **And a step
   of some legs** (`a_step_of_some_legs_is_right_only_where_the_account_lacks_less`): a long stepped away from
   a written call is right (the account lacks 1,500 where it lacked 6,620), and with the call's step after it
   the shard has what a step of both at once gives it; the call named alone, worth nothing, for its premium
   leaves the account lacking as much as before, and is wrong (17.3's line; at random, 30,029 such steps, all
   wrong by the rule); **N71: a held option whose premium the coins hold goes on through a step of every other
   leg** (the account then lacks nothing and is due at no price), and where the coins do not pay the others'
   loss it stays on nothing and is due next. On 58,819 accounts due by a price, of every step of some of their
   legs 503,427 are right and 269,239 wrong. Found: legs that leave apart leave the shard what they leave it
   together only because what the coins do not pay is owed by the account and what a later leg gained is set
   against it (16.18: without that, less in 74,463 of the right steps); and then never less by more than the
   bands (together, the futures legs' bands are taken from what all gained; apart, a leg's band is at most
   what that leg gained: 53,391 steps). **And a held leg of a group ending at its day**
   (`a_held_leg_that_gained_at_its_day_leaves_only_where_the_account_stands_without_it`). 17.5's test is read
   as a futures leg's that gained and leaves (16): what the ended option gained, less its premium, is asked
   of the shard and paid to its user, out of the account; so it leaves only where the account stands on its
   coins without it, and else its gain stays in the account, at its day's price, counted and no coin. A call
   held at 100 for 5 whose day comes at 120, beside a call written at 110 that stands on: its 150 stay (the
   account would be asked 186 and has 70), the account has 120 and after a move of the lower rate 66, its
   premium and more; paid out at the day, the written call would stand on 70 having lost 100. At random
   (40,000 accounts, the price then moving a lower rate a visit, the user taking all a take gives): by the rule
   the gain stayed on 3,836 paths and left at once on 4,685; paid out at the day whatever stays, the shard is
   unpaid on 474 paths, twenty-eight times as much. **Found (M1): with the rule the shard is still unpaid on 34
   paths, never by more than the band.** An account may have its premiums in what its futures legs gained, and
   a step pays what they gained less their band: the test of a step counts no band. For the account's side
   (17.8, 6): a step is due where the coins, what the legs gained less the band of the futures legs, and the
   sum to the price named are below the premiums; or a step takes no band (its price is its stater's, within
   the tolerance, and the band is there for that). Decided by review T11 (D8) and shown: the test counts what
   a step would leave the account (17.8g, D): the shard is then unpaid on no path, 43 visits step an account
   the plain test does not, and an account that took all a take gives is due at no price after a move of the
   lower rate. Still to say before it is carried into
   `covered.rs` and `reserve.rs`: the sums a row needs (with the account's side); N81's bound is 17.8f's, left
   for the last phase.
2. **The leg as it will stay**, futures only: the time it entered, and a day, a strike and a premium of
   nothing; under a call a leg younger than T asks nothing (17.9, F116); the two bounds of a stated price
   rounded inward.
3. The tree, futures only, with the leaf as it will stay: every test of today passing.
4. A draw and a call as traces by a root, one price an underlying; what a called shard has paid (F114).
5. The note of a day's price, and the transaction by which an ended leg leaves.
6. The account's side: the two sums in `KpfTCRow` (for futures as well: what a futures account is asked changes
   here, and the tests of amounts with it), the option leg, the ends, the statement's rate.
7. Option rows; the entry with its premium; the step, the call's step, the entry that ends.

**Left out of the first form**, and the goal's first row holds without them: exercise before the day; dated
futures; sums in the tree's inner nodes; a depth above six; a leg that cancels another of its series; the
deterrent of 17.3.

### 17.8a The order of the books' steps, after review T4

Steps 3 and 4 above are not built in that order: a draw's end, a call's end, a price of a shard, a call's step
and a claim's payment read four rows today, and with a tree and no trace they have nothing to read. Traces do
not need the tree; the tree needs traces. Four commits, each with every test of today meaning what it meant:

1. **The books' hash, on today's four rows**: the sum of the gross, the hash of the rows, the hash of the
   claims' numbers. No rule changes; about 40 bytes more where rows are shown. **Built** (`s286_books_sil.py`,
   run after every other generator; `Shard::books_hash`): the nine scripts that show the rows take 41 bytes
   beside them, and the three that rewrite a row rewrite the sum. Review T3's second round: nothing found; 88
   transactions with another sum or another hash of the claims refused in the ten entries; the sum followed
   through a shard's life (entries on a new row and on one that is there, a void note, a call, a note of two
   legs of one row, a claim, the call's end); a shard of the old form, an account and a note refused in the
   shard's place; no script that carries the 32 bytes compares them with rows. An entry stated is 5,040 bytes
   (4,902), a note of one leg applied 4,314 (4,118), a claim paid 3,624 (3,453); the default workspace 674
   tests, none failed. **To know:** no script compares the sum it is shown with the rows it is shown: it holds
   by the hash from the opening on, since the three scripts that rewrite a row rewrite the sum in the same
   branch. The commit that reads the sum where `KpfTCallPay.off` reads the four rows today (3) keeps both until
   the tree. And: tests that change an argument by its place in the list were refusing for the wrong reason
   after an argument was added (five of the review's own, found and corrected): a change that adds an argument
   brings the places along.
2. **A draw and a call as traces, on four rows**, with today's judgement of an end kept as what the trace is
   compared with; the call's object a list of prices. **Built and reviewed** (17.8b; `s294_trace_sil.py`, run after
   `s286`; `Traced`, `Priced`, `TraceRow` and `Shard::traced` in `reserve.rs`): a draw and a call say a trace;
   `KpfTShardEnd` and `KpfTCallEnd` read no row; the new `KpfTShardRow` answers a row (the library has 71
   scripts); `KpfTShardPrice` answers a price of the list (the points 5 and above) and its order; the call's
   object is the list, and `KpfTCStep` reads a leg's price from it. In the model, on 80,000 random shards and
   prices, the trace judged a row at a time is the verdict of the end over the rows (14,594 right, 61,026
   wrong, 4,380 of no verdict), and each of 291,880 numbers of a right trace moved is wrong at its own row. At
   the engine (`a_draws_trace_is_answered_a_row_at_a_time` and the tests of a draw and a call brought along):
   every row answered right and no other verdict taken; a sum, a place, a price's place moved, each answered
   wrong at its row and the shard again; the counts as written, at the end; a list out of its order, a market
   twice, a price of a day, wrong at the second with nothing cited; an entry no row is of cited by nobody; a
   statement of another length refused where it is stated. **One thing is not as 17.8b has it:** a market in
   the list is the name its row has (the hash of its kind and its market, as a row's head holds it), not the
   plain market: a row's answer and a step compare bytes they have today, the price's answer keeps its
   citation's market shown, and whoever states a draw gives a price a row as before. The plain market comes
   with the tree's keys (4), where the rows' names change as well.
   **Review T5** (`docs/audit/REVIEW-T5-trace.md`; its tests `auditT5_trace.rs`; the earlier reviews' tests
   brought along; the default workspace 689 tests, none failed). Every point of a right statement is answered:
   the end, the four rows (one with no market by its sums), each price with its citation, and every point
   beyond the list up to the largest number with nothing cited. No wrong statement was found that is answered
   right everywhere: on 14 random shards, a draw and a call each, 347 points of the rule's traces answered
   right and not wrong, and 747 traces with one thing changed each answered wrong at the model's point (127 at
   the end, 551 at a row, 69 at a price), and again under a dispute of another point. A draw lives no longer
   for having nine points: the last time to dispute does not move.

   | Finding | What it is | Decided |
   |---|---|---|
   | R1 (high) | a list's day was read as a number by the row's and the price's answers and as bytes by the call's step, and a number of nothing has two forms in eight bytes (F70 again): a call with one byte so written was right at every point, was applied, and no account of that market could be stepped under it: the shard called for good | **fixed, and closed at the engine** for a call and for a draw: both answers compare the eight bytes with eight of nothing, as the step does (the row of that market and the price of that entry are answered wrong and not right, the shard again); what is said is read back only where it is written back the same. No other number of nothing is read by two readers that could disagree: the day was the only one that also goes into an object compared as bytes |
   | R2 (low) | under a false call nobody disputed, an account is stepped only if the list has its market's entry (the end wrote the four heads itself before) | said: a call's list is looked at, as its prices are |
   | R3 (low) | the model's judgement of a row differs from the script's where a sum said is at the end of 64 bits (nothing is gained by it) | the model follows the script's order |
   | R4 (low) | the call's object says no scale: a call whose row says another scale is the same object | as meant (17.8b): a leg has its row's scale |
   | R5 (low) | measured: a price answered is 9,454 to 9,566 bytes (8,226), a call's step of one leg 6,912 to 7,024 (6,890): heavier than estimated | said |
   | R7 (low; open) | by R1's fix the model reads no statement that writes any number of nothing its other way, and the scripts still do: a wrong draw so written is answered wrong at the engine, and a watcher's tool can neither name its wrong point nor answer it | the model is to keep the bytes as said beside the numbers (judge by the numbers, answer with the bytes); with the next change of these builders |

   Measured: a draw stated 3,382 bytes at two markets and 3,494 at four (about 3,200); a call stated 3,423 and
   3,535 (2,913); a draw's end answered 3,407 and 3,519 (about 5,050); a call's 3,512 and 3,624 (5,402); a row
   answered 4,176 and 4,288. `KpfTShardEnd` is 1,747 bytes (3,234), `KpfTCallEnd` 1,853 (3,695),
   `KpfTShardPrice` 2,700 (1,810), `KpfTCStep` 5,056 (4,829), `KpfTShardRow` 2,098. And found on the way: one of
   the earlier reviews' tests had passed for the wrong reason since a point's number changed its meaning, and
   another named places as rows where they are the list's.
3. **The claims**: no claim paid while a row has a unit, what a called shard has paid (F114), two classes (L1,
   L4). In three pieces (17.8c). **(a) What a called shard has paid: built and reviewed**
   (`s310_claims_sil.py`, run after `s294`; `Claims`, `Claim` and `Shard::claim_paid_at` in `reserve.rs`): a
   shard's state has the claims' three numbers, shown plain beside the rows (36 bytes where 41 were); the note
   of the rest says four numbers everywhere (three of nothing while its shard is not called); `KpfTCallPay.claim`
   pays a claim its part of all the shard ever held for its claims, the first class before the second, from
   the call's time, a lateness and three dispute periods on (an input that landed then shows it), and writes
   the claim again while it is not whole; `KpfTCallPay.off` ends the call where what was paid is what is owed.
   At the engine: a shard of 24.7 KAS that owes 45 pays its two claims their parts (one share, whatever the
   order), stays called, and takes 30 KAS that reach it late: the claims are then paid whole and the call ends
   with what they left: **F114 is closed in this form.** A part below a note's least waits; a claim that is
   covered is paid whole whatever its size; no claim is paid before the time; a sompi more, a claim spent while
   it is not whole, what is owed falling, the call ended with the numbers left: each refused. The second class's
   part is run on shards made by hand (no script writes a claim of it before (b)).
   **Review T6** (`docs/audit/REVIEW-T6-claims.md`; its tests `auditT6_claims.rs`, fourteen; the earlier
   reviews' tests brought along: 24 failed by the change, each as it was meant to; the default workspace 703
   tests, none failed). **No finding above low; F114 is closed at the engine**, in review 65's own numbers (100
   KAS held against claims of 120 and 80, then 60 KAS an account pays later: 60 and 40, then 36 and 24, and no
   end of the call before). No claim paid more than its part and no second class before the first: the model
   is the script written out line by line on 360,000 random shards and claims; at the engine 1,200 shards made
   by hand, 7,087 outcomes written otherwise refused, and none of 7,720 taken for claims the model refuses;
   over 6,000 sequences of two to six claims of both classes with coins arriving between payments, nothing is
   paid beyond what is held, the second class never has what the first is still owed, what a claim has been
   paid is its two parts to the sompi whatever the order, and every call ends once what the shard ever held is
   what it owes.

   | Finding | What it is | Decided |
   |---|---|---|
   | K1 (low) | the last part of a claim, where it was below a note's least, asked no output and was whoever paid it's: for any claim paid a part before, up to 0.2 KAS a claim (ten claims of 1 KAS, 8.1 held: an owner that pays in the rest pays each "whole" with no output, and has 1.9 of the users' 10 and their notes' least) | **fixed, and closed at the engine**: the payment that makes a claim whole pays its key the part and what the claim's note held, in one output, whatever its size (the ten users have their 10 KAS and their notes' least; the owner is out what it paid in; 129 payments with another output refused). Whoever pays a claim gains nothing and is out its fee, about 0.011 KAS: a claim is paid by its user, or by the owner, who needs every claim whole to have its shard back |
   | K2 (low here; F96's kind) | a note that was never counted is paid as a counted one, up to what its class is still owed, and the call ends with the real claims short by it (claims of 30 and 15, a stray of 20 shown first, 45 held: the stray has 20, the 30 has 25 and goes on, the 15 is paid by no outcome, the call ends) | said: the bound makes the call end; it does not protect the claims. A set of the claims counted comes with the tree. Only a wrong statement nobody disputed writes such a note |
   | K3 (low) | a shard kept before this form did not read back; a claim partly paid has no kept form; no command pays a claim or ends a call | the first fixed (a shard of no claims' numbers is kept as before); the others with the commands of this form |
   | K4 (low) | "at most 7 parts in ten million less than its exact share" holds near full cover only: at half held a part in a million, at a thousandth 333; under two units in sompi, never above | the sentence of 14.6 is of the unit, not of a ratio |
   | K5 (low) | a part below a note's least waits, for good if nothing more comes: a claim of A is first paid once what the shard ever held is 0.2 KAS by what it owes over A (in the sequences 50,695 of 84,469 claims shown waited) | said to a user; nothing is taken, the part stays in the shard |

   For (b): a claim counted after another was paid whole (a mark's note applied late, at the engine) has 50 of
   150 where its part is 75: 17.8c's "second once anything was paid". Measured: `KpfTCallPay` 2,523 bytes
   (1,691); a claim paid whole 4,455 (3,624; estimated 4,100), a part and written again 4,491, the call's end
   4,345; a note of one leg applied 4,441 to 4,585 (4,314); an entry stated 5,056 (5,040).

   **(b) The second class, and (c) the row's gate: built and reviewed (S1, S2 and S6 open)** (`s322_second_sil.py` and
   `s320_gate_sil.py`, run after `s310`; `Shard::left_class`, `marked_class`, `young`, `CallStep::young` and the
   gate in `Shard::entered`). `KpfTCStep` writes a note of tag 9 that says, beside what is asked, what the young
   legs gained together less their band; `KpfTLeft`, `KpfTLeftAll` and `KpfTMarkNote` (which opens the books: 68
   bytes) count what a note asks of a called shard in its class, and write the claim with both amounts;
   `KpfTLeftVoid` takes a note of tag 9; `KpfTEnterS` holds the row's gate. At the engine:
   - **L4's play** (a shard of 20 KAS, a user's long of 5 units that gained 15 a hundred blocks after it
     entered, an owner's own long of 1,000 units entered at the call's time): the user's step says 14.35 KAS of
     the young's (15 less the band of 0.65); its note is counted of the second class; the owner does not have
     its shard back while that claim stands; the claim is paid whole from the time of the claims, and the call
     ends with what is left. With the gate the owner's leg does not enter that shard at all: it asks 1,007.5
     KAS held, and enters where the shard is paid up.
   - **L1**: the same leg leaving by its user's own statement after the call: its note is of the second class,
     whole; counted of the first, refused. A leg that entered T before the call: of the first.
   - **T29**: an old leg's note counted after a claim was paid: of the second class.
   - A step's note is held to its legs; shown as an account's own, refused; the young's amount a sompi more,
     without its band, as what is asked, or left out: each refused.
   - **The gate**, to the sompi: a leg of 100 units at 10 in a row of its own asks 77.5 KAS held (7.75 %); a
     sompi less, refused; a shard that owes holds that much more; a short that sets a row off enters a shard
     of 1 KAS (155 asked before, 122.5 after), one that turns the row and asks more does not; another row's
     requirement is not counted.
   **Review T7** (`docs/audit/REVIEW-T7-second.md`; its tests `auditT7_second.rs`, eight; the earlier reviews'
   24 failing tests brought along, none weakened; the default workspace 713 tests, none failed). **L4 and L1,
   as they were found, are closed at the engine**; N1 and T29 (a claim counted late) closed; nothing young
   dilutes the first class (108 notes made by hand of every combination applied as the rule says, 574 other
   outcomes and kinds refused); nothing is claimed twice; the gate is the model's to the sompi on 120 random
   rows. F116 is closed for a young leg: an owner's young leg is a claim of the second class, paid only what is
   held beyond the honest claims, whoever is shown first. With the gate and the second class both, L4's user
   (the shard paid up for the owner's long) is whole at moves of 0, 1, 2 and 5 % and at 1 % down, has 18.97 of
   19.85 at 8 % and 16.21 of 21.14 at 10 %; the paper's 3.79, 2.12 and 1.16 of 15 do not hold with the gate,
   since what the owner paid in is in what the shard ever held.

   | Finding | What it is | Decided |
   |---|---|---|
   | S1 (medium; open) | **a leaving raises its row's requirement with no gate, and the leg that stays is as old as its owner likes.** In L4's shard: the owner's long of 1,000 units enters with 1,007.47 KAS held; its short of 1,000 from a second account enters by the gate's second arm; hedged, it draws 272; the short leaves by its own statement; the row is one-sided on 734 KAS (5.6 % where the gate asks 7.75 %) and a call is due at once. With no move nobody loses. With 10 % up before the call: the long old, a first-class claim of 1,436, the old user has 11.01 of 21.86 and the young one nothing; the long young, the old user whole and the young one 11.81 of 21.14. It costs the owner time, and needs a move beyond what the shard holds while a call is due | open. Two directions: the upper rate on the gross at half the upper rate on the net (a row set off is then held as its larger side alone would be: a number, with the rates); or a leg is young where its row's net grew by a leaving within T. For the owner of the design; with the rates |
   | S2 (medium; open) | the gate reads what is held, not the equity, and one row: on a shard that holds 20 and has an equity of 5, a leg of 2 units more enters and a call is due at once (the user then loses its band and no more); a shard of 775 KAS takes 1,000 units in each of four rows and is asked 3,100 | said in 17.8c already (it bounds a leg; it is not the rule of 12); a number over the rows is the tree's question |
   | S6 (medium; rests on 17.9) | the gate's price is the one the entry states: a long of 10,000 units "from a sompi" enters a shard of 1 KAS, and a call is due at once; if nobody disputes the entry's price, its step says 128,700 KAS of the young's | as every statement: a wrong price is answered wrong and the entry undone. Whoever watches disputes an entry whose price is not the market's; the second class bounds what it takes to the young |
   | S3 (low) | a young leg nobody stepped leaves at its own later price: a second-class claim of 48.5 where its step said 18.8 (F95, inside the second class) | with F95 |
   | S4 (medium; a hole in the tests, not the scripts) | about a dozen places in the earlier reviews' tests applied a step's output as a note of tag 6, which no script writes (the builder made the input): two tests passed on the changed scripts so, one of them still asserting L4's hole | re-derived by the review: a step's note is held to its hash and applied with its tag, and the same transaction with the other tag refused. One line of `thin_draw.rs` has the same weakness (refused for its outcome, not its hash) |
   | S5 (low) | the gate's arithmetic refuses an entry where its row's gross is worth more than 64 bits at the leg's entry, even a leg that sets the row off | said |

   Measured: an entry stated 5,554 bytes (5,056; `KpfTEnterS` 2,982); a note of one leg applied 4,852 to 4,944
   (4,441 to 4,533; `KpfTLeft` 2,916, `KpfTLeftAll` 5,093); a call's step of one leg 7,355 to 7,407 (6,972 to
   7,024; `KpfTCStep` 5,439); a mark's note applied about 3,100. **The two common paths are about 10 % and 9 %
   heavier than before these two pieces, where 17.8c estimated 5 %.** Not reached: `KpfTLeftVoid` with a note
   of tag 9 at the engine; the entry undone after S6's; S1's draw at the engine and S1 in four rows; terms other
   than the tests'.

4. **The tree**: it then touches only the scripts that read one row, and a row's answer.

### 17.8b A draw and a call as traces, on four rows (17.8a, 2; on paper, reviewed)

Today a draw says 40 bytes (an amount and a price a row), and one script, `KpfTShardEnd`, computes its end over
the four rows: five points (the end, and a price a row). A call the same (`KpfTCallEnd`), and its object holds
the four rows' heads and prices (368 bytes). This is what they become, with the rows still four and written
whole: the rule is the same, and `draw_end_judged` and `call_end_judged` stay in the model as what a trace's
verdict is compared with, on random shards.

**What is said** (the bytes of a draw's and a call's statement):

| | Bytes | |
|---|---|---|
| the amount | 8 | a draw's amount; nothing for a call (kind 11: its fee is the terms') |
| E, and **the list of prices** | 8 + 56 a price | a market, plain (32), its price's limit (8), a day (8; nothing: the price now, and nothing else until the day is built) and the price (8); in the order of the keys (`books.rs`), no market and day twice; four at the most |
| N, and **the rows** | 8 + 40 a row | the row's place in the books (8: its number among the four now, a leaf's place with the tree); the place of its price in the list (8); and the three sums up to it: what the net units are worth, what the gross units are worth, and what the clearing side's net units are worth signed |

184 bytes and 56 a price: 240 at one market, 408 at four, where it is 40 today. N is four: a row with no market
is a row of the trace, and its sums are the row before's.

**The points:**

| Point | What is answered | By |
|---|---|---|
| 0 | the end: the last row's sums against the rule (the upper level for a draw, the lower for a call), the amount, the outputs; and for a call that its object is the list | `KpfTShardEnd`, `KpfTCallEnd`: no row is read, the rule's arithmetic as today |
| 1 to N | a row: the row of the books at that place (of the 408 bytes, until the tree), the price the list has at the place the trace's row names (the hash of its market the row's name, its limit the row's, its day nothing, the price within its limit: judged before any worth is computed; a row with no market adds nothing), the sums the row before's and this row's worths (a call's price moved the tolerance, by the row's net: the answer is told the kind) | a script of its own, `KpfTShardRow` |
| N + 1 to N + E | a price of the list: within the tolerance of the committee's at the statement's time (the shard's id opened by the seed: 17.9), and strictly above the entry before it in the order (where the order is wrong, no citation is needed) | `KpfTShardPrice` |

Five to nine points at four rows, five today. **Every point from nothing up is answered**, and a trace that is
not well made is answered wrong and not by a script that fails: a point nobody can answer lapses at the
stater's cost (`KpfTPoint.lapse`), and an owner's other key would end every call for nothing. So a point beyond
the trace is answered right with nothing cited, and a place beyond the list or a count that is not the length's
is wrong. The rates are counted once, at the end, on the totals, as `need_of` does: a trace of the three sums
gives today's verdict bit for bit (where a worth or a sum does not fit 64 bits, the row's script or the end's
fails, and the dispute lapses at the stater's cost). N is the shard's rows (four) by the end's reading of the statement's
length; with the tree (commit 4) it is the list from the head, and a point more holds that the last leaf names
none.

**The call's object** is the list (E by 56 bytes, hashed flat as its body is today): 224 bytes at four markets,
368 today. `KpfTCStep` reads a leg's price at the place its stepper names: the entry's market and limit are the
leg's, its day nothing. No entry is twice in a list whose prices' points were open to dispute, so whoever steps
has nothing to choose.

**What changes:** `KpfTChange` (what a statement of kind 5 or 11 may say: a length of 184 to 408 bytes by steps of
56, where it is 40: about 50 bytes more in every statement of every account, since the script is one),
`KpfTShardEnd`, `KpfTCallEnd` (they read no row; the call's end hashes the list out of what is said),
`KpfTShardPrice`, `KpfTCStep` (the entry a leg is shown: its market the leg's, its limit the leg's, its day
nothing; a price of nothing stays a price), the new `KpfTShardRow`; `Draw`, `Called` and their builders. What
does not: the rule, the bonds, the times, `KpfTPoint`, `KpfTCallPay.off` and `KpfTCallEnter` (they carry the
hash of the call's body), `KpfTDrawNote` and `KpfTDrawEntry`, every script of an account.

Sizes, estimated by the review: a draw stated 3,450 to 3,650 bytes (about 3,200 today) and a call stated 3,150
to 3,350 (2,913); the ends answered about 3,500 (5,050 and 5,402); a row answered 4,300 to 4,500 (new); a
price answered 8,390 to 8,560 (8,226); a step of one leg 6,520 to 6,750 (6,890). A draw lives no longer than
its lateness and two dispute periods whatever the number of points, and a stall costs a bond a period, as
today.

**Said beside 14.7's lines (T23, of today's form as well):** a row of a market the committee does not sign makes
a shard that cannot be called (that price has no citation). Whoever watches disputes any entry whose price
nobody can cite; a user reads the markets of a shard before it enters.

The checks of each script, as lists to build from, are in `docs/audit/REVIEW-T4-tree.md` ("Second round").

**The row's gate (for commit 3, with numbers in the model first).** A gate of the whole books at every entry
is a trace on every entry (5.2 to 5.4 kB now, about 3 kB more at sixty rows): not that. But `KpfTEnterS` reads
the leg's own row, and can hold, with nothing more shown and about 200 bytes of script, that **what the shard
holds less what it owes is at least the upper requirement of that row with the leg, at the leg's own entry**.
The 1,000 units of L4's play ask 1,012 KAS of a shard that holds 19.9: refused; 14.7 units enter. With it the
review has L4's user whole at moves of 1 % and 2 % with one row, and with all four rows at their caps and
every market moving the owner's way 12.3 of 15.3 at 1 % and 9.1 of 16.0 at 2 % (2.12 and 1.16 with no gate).
It counts what is held and not the equity, and a row at a time: it bounds a leg, it is not the rule of 12.

### 17.8c The claims of a called shard, and the row's gate (17.8a, 3; on paper, reviewed)

Today a called shard owes one number (what its notes asked and it did not pay), and a claim is paid its amount
by what the shard holds over what it owes, once no row has a unit, and is spent. Three things are wrong with
it: coins that reach the shard after the last claim are the owner's (F114); a leg entered and called is a
claim (F116: closed at the call's step by asking nothing of the young, which an owner uses to be rid of what
its newest users gained: L4), and the same leg leaving by its user's own statement is a claim again (L1).

**The numbers.** A called shard has: H, what it holds; C1 and C2, what it owes by claims of the first and of
the second class, neither of which falls when a claim is paid; D1 and D2, what it has paid to each. C1 stands
where what it owes stands today (its moving part): while the shard is not called it is as today, the sum of
the notes of the rest that stand (it rises by what was asked and not paid, and falls as a note is paid), and at
the call nothing is written but the time. C2, D1 and D2 are the claims' numbers, in the books' hash, shown
plain (27 bytes with their pushes) where 17.8a, 1 carries their hash. W is H, D1 and D2: what the shard ever held for its claims; a
payment does not change it, and every coin that reaches the shard raises it.

**A claim** is a note of its key and four numbers: A1 and a1, its amount of the first class and what it has
been paid of it; A2 and a2, of the second. One note, since a block takes one beside the shard.

**When it is paid.** No claim is paid while a row has a unit (as today: every note that takes a leg out is
counted first), nor before the call's time, a lateness and **three** dispute periods (one more than a
statement lives: a mark stated a block before the call's time and disputed is applied by then). Then, in
whatever order:

- First class: its part is A1 where W is at least C1; else A1 by W over C1 (in the unit `KpfTCallPay` has,
  chosen from C1; C1 rounded up, W down). It is paid the smaller of its part less a1 and H.
- Second class: S is W less C1, and nothing is paid where S is not above nothing. Its part is A2 where S is at
  least C2; else A2 by S over C2. It is paid the smaller of its part less a2 and H less what the first class is
  still owed (C1 less D1).
- A claim that is covered is paid whole whatever its size; a part below a note's least waits (the least is of
  the two parts together, in one output). A note says amounts no less than what it was paid, or is paid
  nothing (a note is not taken at its word: F86); and no part is more than what its class is still owed (C
  less D: a note that was never counted would else take what was paid past what is owed, and the call would
  never end).
- **Once anything was paid, every claim that is counted is of the second class** (review T4, T29). A claim
  counted late is else not behind: with 150 held, a user's first claim of 100, the owner's second of 50 and
  the owner's mark's note of 100 not yet applied, the 50 are paid, the note is then applied and paid 75, and
  the user finds 25, where at one share it had 75. Only a mark's note can be counted that late; whoever holds
  one applies it before the time.
- The call ends where no row has a unit, D1 is C1 and D2 is C2: the numbers are written as nothing. A shard
  that never holds enough stays called, its claims standing, and what reaches it late is theirs.

**Where a claim has its class: where its note is applied to a called shard, by its legs' times against the
call's** (a leg is young where it entered after the call's time less T: 17.9), for every kind of note:

| Where the claim is born | Class |
|---|---|
| a note of the rest written before the call | first: it is in what is owed already |
| the call's step (`KpfTCStep`) | a note of a kind of its own (tag 9: the legs' chain, the key, what is asked, what the young gained), written by that script alone, so that the note every account's leaving writes does not change: what the old legs gained, with what the young lost together, first; what the young gained together, less their band, second. Where it is applied it is held to its legs: the young's amount nothing where no leg is young, the other not above nothing where every leg is |
| a note of one leg (a leaving, a step of a liquidation), a mark's note | by that leg: young at the call, second |
| a note of two to six legs | second, whole, where any leg is young; else first |

- **The band** of the second class is the tolerance twice of the sum of the young legs' larger worths (each at
  its entry or at the call's price), rounded up once, in the call's step: without it a user's young gain is
  diluted for certain by an owner's leg entered at one edge and called at the other (17.10). No second band
  where a note is applied: a leaving took its own, and a mark left its band in the leg.
- **Nothing of the second class is set off**: not against what the account's old legs lost, not against what
  the account owes. (Set off, an owner's old loss would be paid with a young leg's gain, and the first class
  would lose it.)
- Not "stated after the call's time": no script that counts a claim knows when a statement was stated, and an
  owner that says a call's time a lateness back would push a user of old legs down.

**The row's gate.** `KpfTEnterS` reads the row a leg enters, and holds that **what the shard holds, less what
it owes, is at least the upper requirement of that row with the leg, at the leg's own entry, or that the
row's requirement with the leg is no more than without it** (a leg that sets the row off is not refused: a
shard that holds 100 with users long 10,000 units takes the short that brings it from 775 asked to 450). The
requirement is `need_of` for the one row: the maintenance rate and the tolerance of the worth of its net units,
and the rate on the gross and the tolerance of the worth of its gross units. Nothing more is shown, about 200
bytes of script. A leg of 100,000 KAS in a row of its own needs 7,750 held; an owner's 1,000 units of L4's play
need about 990 KAS paid in, which then pay the users. It bounds a leg by what its shard holds; it is not the rule of 12 (it counts
what is held and not the equity, and a row at a time). With it and the second class, L4's user is whole at
moves of 1 % and 2 % in one row (the review's numbers; in the model before it is built).

**What changes:** `KpfTCStep` (the note of tag 9; the band), `KpfTLeft` (`apply`: the class; `rest`: the note
of four numbers), `KpfTLeftAll`, `KpfTLeftVoid` (it takes a note of tag 9), `KpfTMarkNote` (it opens the
books: under a call it writes what is owed of the second class), `KpfTDrawNote` (a note of the rest by its new
hash), `KpfTCallPay` (the two parts; the end), `KpfTEnterS` (the gate), `KpfTShardOpen` and the scripts that
show the claims' numbers. What does not: an account's statements; the note an account's leaving writes; a shard
that is not called.

**In three pieces, each reviewed at the engine:** (a) what a called shard has paid: the claim of four numbers
with nothing of the second class, the first formula, the time, the end (F114's shard is its test); (b) the
second class: the note of tag 9, the band, the class at the applying, "second once anything was paid", the
second formula (L1, T29's play, L4 in the model); (c) the gate, in `KpfTEnterS` alone (the tests of F116 and L4
enter legs it refuses: their shards are paid up, or the legs are within it).

**Said to a user:** a shard stays called until both classes are whole: an owner that brought a call about
pays its users whole to have its shard back, or gives it up. Apply your notes and show your claim when the
time comes: within the second class a claim shown early keeps what it was paid. A step of a liquidation, which
is anyone's, that names an old leg that gained beside a young one writes a note of the second class whole.

Sizes, estimated by the review: an entry stated about 5,300 bytes (5,040), a note of one leg applied about
4,550 (4,314), a claim paid about 4,100 (3,624), a call's step about 350 more: everything a little heavier,
the two common paths by about 5 %.

The checks of each script, as lists to build from, are in `docs/audit/REVIEW-T4-tree.md` ("Third round").

### 17.8d The tree, in two stages (17.8a, 4; on paper, for review)

The model of the tree is `books.rs` (no script reads it yet): the key (60 bytes: the market, the day, what the
row is of, the strike), the row (the key, its market's scale and price's limit, the net, the gross, what its
legs were entered at where it has a day, the hash of the key after: 147 bytes hashed under a leaf's tag), the
head leaf, a row put in and taken out, the root and the books' hash in the form 17.8a, 1 built. What the model
decided beyond 17.6 and 17.10:

- **One scale and one limit a market are held by one leaf shown, the one before**: a market's futures row is
  its first row and is put in before any other row of the market (the leaf before it is another market's); a
  row with a day is put in after a row of its own market, and has that row's scale and limit; the futures row
  is taken out only where the row after it is another market's, or none. No third leaf is ever shown.
- An id of which a number of 8 bytes is nothing written below zero is no market's (two such ids are one key by
  their numbers).

Every script, test and tool that says `rows` changes with the tree. Two stages, each reviewed at the engine,
so that the change of what a row is CALLED is not made in the same commit as the change of where a row IS:

**Stage 1: keys for names, still four rows written whole.** Today a row's head is the hash of its kind and its
market, the committee, the scale, the limit (84 bytes), and the list of a draw's prices holds that hash (17.8a,
2, the one thing not as 17.8b had it). In this stage a row is the tree's row without its last field: the key
plain (the market, a day and a strike of nothing, futures; an options row of today's one test: calls, with a
day and a strike), the scale, the limit, the net, the gross, what was entered. No committee: `KpfTShardPrice`
opens the shard's id by the seed, as `KpfTStepPrice` does. The list of prices holds the plain market, in the
keys' order; `KpfTCStep` compares the leg's market with the entry's as bytes (a hash less a leg); a price's
answer needs no market shown beside the citation. The model's `Head` and `ShardRow` become `books::Key` and a
row of `books.rs`'s fields; `Shard` still holds four of them. Every test of today passes with its rows said so.

**Stage 1 as it is built, and reviewed at the engine (review T9).** A row is 105 bytes, the tree's leaf without its tag
and the key after: the key (60: the market, a day, a kind and a strike, three of nothing in a futures row), the
scale and the limit (18), the net, the gross and what its legs were entered at (27; the last nothing, in a
futures row). Four rows are 420 bytes. `KpfTEnterS`, `KpfTLeft`, `KpfTLeftAll` and `KpfTLeftVoid` name a leg's
row by the leg's market and three numbers of nothing (no hash); a market of nothing enters no row (the rows of
no market have its key). What is held is one row a KEY: beside a row of a market with a day (made by hand:
no script writes one yet) the market's futures leg enters an empty row. `KpfTShardPrice` reads no row: it is shown the seed of the shard's opening and a
committee, which give the shard's id with its terms and periods, and the citation is of that committee and of
the entry's market, plain (an entry no row is of is priced as any other). `KpfTCStep` holds the list's entry
to the leg's market as bytes. The model: `ShardRow` (a key, a scale, a limit, the net, the gross, what was
entered), `RowHead` (what a leg says of its row), `Shard::row` (the futures row of a market), `of_rows`,
`seeded`, `is_of`; one entry of a list a market, whatever rows are its (two rows of one market given two
prices have no trace). The builders of an entry and of a price's answer are given the committee and the seed.
Measured: `KpfTShardPrice` 2,317 bytes (2,707), `KpfTCStep` 5,319 (5,439), `KpfTEnterS` 2,966 (2,982),
`KpfTLeft` 2,914 (2,916), `KpfTLeftAll` 5,059 (5,093), `KpfTLeftVoid` 2,093 (2,088), `KpfTCallPay` 2,567
(2,523: two rows of no market written out, longer), `KpfTShardOpen` 752 (730). Of the transactions: an entry
stated 5,550 bytes (5,554); a note of one leg applied 4,862 to 5,006 (10 more: the rows are 12 bytes longer); a
call's step 120 less; a claim paid and a call's end 56 more each. The patch of the scripts is
`run/gen/s333_keys_sil.py`, run after `s320_gate_sil.py`.

**Review T9** (`docs/audit/REVIEW-T9-keys.md`; its tests `auditT9_keys.rs`, ten; the earlier reviews' nineteen
failing tests brought along; the default workspace 723 tests, none failed). Held under attack: a leg enters,
leaves and is voided by the row of its futures key and no other, for the four scripts; no two rows of one key;
a leg of another scale or limit enters neither its market's row nor an empty one; the market of nothing enters
and leaves nothing. A price is answered only with the seed and the committee that open the shard's id (five
wrong seeds, five wrong committees, the two the other way round, another market's citation: refused); a shard
whose id no seed opens has no price answered, at the cost of whoever stated. A futures row and an options row
of one market are one entry both rows name; two prices or two limits for one market have no right statement.
A call's step finds its leg's market plain and refuses the old hashed names. Sixty random books: the model is
the engine for an entry, a leaving of one and of two legs, and a void.

| Finding | What it is | Decided |
|---|---|---|
| Q1 (medium) | **two ids that are one market by their numbers enter two rows, and no draw or call over them is right.** The list is ordered by a market's 32 bytes as four numbers, and a script reads eight bytes of nothing with the top bit set as nothing; `KpfTEnterS` compares keys as bytes. Of two ids that differ so in one number, neither entry is below the other: the second is answered wrong, by anybody, in either order. It takes the owner's signature at the entry (the owner's power of P1's rest, by another door); the hashed names had no such pair | closed in stage 2, where a row is put in by `KpfTRowIn` alone: it refuses a market of which a number is nothing written below zero (the model's `Key::well_made`; review T8, P2), and no entry makes a row. Until then: open |
| Q2 (low) | an entry no row is of is priced as any other, as built; the model's `trace_wrong_point` still names its point wrong whatever it says (a watcher that disputes by it loses its bond; the tool computes its own) | with stage 2, where a trace's functions are written again |
| Q3 (low; not reached in stage 1) | a row emptied keeps its nine bytes of what was entered: with a number there it is not the empty row, takes no leg, and `KpfTCallPay` never sees four empty rows | a rule for stage 2 and the day: a row is taken out only with nothing entered (the model's `Books::out` has it), and whatever writes "entered" takes it back as the legs leave |
| Q6 (low; usability) | whoever answers a price must know the seed of the shard's opening: no object says it, and the tool asks for it | the seed is public (the hash of the anchor's outpoint the shard was opened with: the first transaction of the shard's chain gives it); the tool should find it there. Not built |

**Stage 2: the tree.** A shard's state holds the root where it holds the hash of the four rows.

| Script | Is shown | Holds |
|---|---|---|
| `KpfTShardOpen` | nothing more | the root of a tree of the head leaf alone, a constant |
| a row put in (new, `KpfTRowIn`; the owner signs) | the leaf before and its path, the key it names plain, the new row's key, scale and limit, an empty place and its path against the root as the leaf before rewrote it | the leaf before's key below the new key; the key it names above it, or nothing; the rule of a market's first row; the new row of no unit |
| a row taken out (new, `KpfTRowOut`; anyone for a row whose day has passed, the owner for a futures row) | the row and its path, the leaf before and its path, the key the row names plain | no unit, nothing entered; the futures row's rule; the leaf before names what the row named; the place empty |
| `KpfTEnterS`, `KpfTLeft`, `KpfTLeftVoid` | the one row and its path (138 + 8 + 192 bytes: the row without its tag, its place, six hashes; 408 are shown today) | the row opens the root; the root with the row rewritten |
| `KpfTLeftAll` | a row and its path for each leg of the note, in the note's order: each path against the root as the leg before rewrote it (two legs of one row: the second is shown the row as the first left it) | the same, leg by leg |
| `KpfTShardRow` (a row of a trace) | the row and its path, and the row before it in the trace's order and its path | the row before names this row's key (the first: the head names it); the trace's row as today |
| a point more of a trace (the last) | the last row and its path | it names the key of nothing: no row is left out at the end |
| `KpfTShardPrice` | no row (the shard's id by the seed) | as stage 1 |
| `KpfTMarkNote` | no row, as today: the books' three numbers and the root | as today |
| `KpfTCallPay` | no row: the books' three numbers and the root | the sum of the gross is nothing (the four rows compared whole today, beside it) |
| `KpfTChange` | | a trace's length by its two counts: 24 bytes, 56 a price, 40 a row; sixty-three rows and a number of prices at the most |

- A row's place among the sixty-four is whoever puts it in's to choose; the trace's row says the place (the
  first of its five numbers, which says the row's number among four today).
- No entry puts a row in: an entry on a market the shard has no row of is refused, and the owner puts the row
  in first (a transaction of its own: about 700 bytes of rows and paths, 4 to 4.5 kB with its script; the common
  entry is not made heavier for it).
- The model: `Shard` holds `books::Books` in a form that copies (sixty-four places); `rows` as tests say it
  today is given by a function (the rows in the keys' order), and a shard of given rows is made by one.

**To be said by the review:** whether stage 1 is worth a commit of its own, or the two are one; the checks of
`KpfTRowIn` and `KpfTRowOut` exactly, with the order's comparison (the head by rule; seven numbers) and what
"the place is empty in the root as the leaf before rewrote it" needs where the leaf before and the empty place
share nodes of their paths; `KpfTLeftAll` with paths (bytes, and whether one note of six legs of six rows fits
a transaction: 6 x 365 bytes of rows and paths, and the script unrolled six times); `KpfTShardRow` with two
paths; what an owner that never puts a row in, or fills the tree, does to its users (nothing: they do not
enter); who takes a dated row out and when (its day has passed AND it has no unit); sizes of the common paths
against today's 5,554 (an entry) and 4,852 (a note of one leg), which review T4 estimated 6 to 15 % heavier
before the last two pieces made them 10 % heavier already; and S1 (the leaving that raises a row's requirement):
does anything in the tree make either of its two directions cheaper or dearer.

**Review T8** (`docs/audit/REVIEW-T8-tree-plan.md`; on paper, nothing run; every size reasoned from the scripts
as they are, none compiled). **The tree's rules as the model has them are sound; no rule of a row put in or
taken out changes.** One leaf shown is enough for one scale and one limit a market: the order compares the
market first, so a market's rows are next to one another; its futures key is the least of its keys, so the leaf
before a new futures row is never of its market; a dated row needs a row of its market before it, which has the
market's scale and limit; the futures row leaves only where the row after is another market's. A leaf before
that another transaction rewrote opens no root; only the owner fills a tree, and nothing of a user's is held by
a full one. Decided, before any script:

| Finding | What it is | Decided |
|---|---|---|
| P1 (medium) | with the tree a row stands at no unit until it is taken out, and a trace asks a price of every row with a market: an owner puts in a row of a market its committee never signs, and no call's trace can be answered | **a row whose gross is nothing adds nothing to a trace and reads no price** (what a row of no market is today). The rest stands as today and is open: a leg of one unit in such a market does the same, and a row is the owner's to put in. With the day and the option rows: what a call does with a price nobody can cite |
| P2 | a key shown as bytes with a day written as nothing below zero is the futures row by its numbers and not by its bytes | `KpfTRowIn` takes the market as 32 bytes and the rest as numbers and writes the key itself; "nothing" on a shown row is compared as bytes; `KpfTChange` holds the two counts as written |
| P3 (medium) | a place of 64 + p opens what p opens: a trace would have two right forms | every place is held within 0 to 63 before its path; a trace's row with a place beyond is answered wrong with nothing opened |
| P4 | a leaf is 147 bytes hashed (9 + 60 + 45 + 33), not 165 | corrected above; a row with its path is about 345 bytes |
| P5 (medium) | the arms of a dated row cannot be tested before the day is built | in stage 2 `KpfTRowIn` takes a futures key only and `KpfTRowOut` is the owner's only; the model keeps both arms, for the day |
| P6 (medium) | `KpfTLeftAll` is the one path that is much heavier: the script about 7.2 to 7.8 kB (5,093), a note of six legs of six rows about 12 kB (7.6), a note of two legs about 10 kB (7.2) | the note stays whole, a path a leg, no limit of legs; it fits (the rows and paths go as one push). A test of the worst case with its bytes; a twin of three legs from the same source if the script measures above 7 kB |
| P7 (medium) | the model's `Shard` copies, and with sixty-four places it is 8.7 kB: frames of many of them can overflow a test's stack | the type's change alone first, and the tests run; the stack set larger if it must be; an `Arc` inside and no copying is the fallback. Not fewer places in the model than the scripts take |
| P8 | stage 1 is worth a commit of its own if the tests' change is spent there once | stage 1 brings the tests the form they keep: a shard of given rows by a function, a row read by its market and not by its number, the points of a trace by functions |
| P9 | the last point | folded into the row's answer: the last row of a trace names the key of nothing; a trace of no row shows the head alone. Rows are points 1 to 63, prices from 64 |
| P10 | `408` is two things in the scripts (the four rows; the longest trace in `KpfTChange`) | the trace's length by its counts: 24 + 56 a price + 40 a row, each count 63 at the most (6,072 bytes the longest) |
| P12 to P14 | the market of nothing; the opened tree's root; a rule made local; a called shard | a key of the market of nothing is no row's; a test holds `KpfTShardOpen`'s constant to the model's opened root; `KpfTRowIn` compares the named key's market with the new futures row's; no row is put into a called shard |

Stage 1's own five points for its review: the list's order is the plain markets'; the committee by the seed (a
shard made by hand with an id of chosen bytes fails a price's answer); "an entry no row is of is cited by
nobody" is "by anybody"; a futures row and an options row of one market are one entry of the list; which uses
of the tags 13 and 14 are a shard's row's name and which an account's own.

Sizes, reasoned: an entry stated about 5.65 to 5.85 kB (5,554 today: 2 to 5 % more); a note of one leg applied
about 5.0 to 5.2 kB (4,852: 3 to 7 % more); a price answered about 450 bytes lighter; a row of a trace answered
about 4.9 to 5.3 kB (4,290). No cheaper form is worth two forms: a depth of five saves 80 bytes a path.

S1 and the leaf: a higher rate on the gross is neither cheaper nor dearer with the tree. "Young by a leaving"
would need a time in the leaf and in the list's entry (a call's step reads no row), and is a rule a row where
dated rows are judged an underlying. **The leaf has no field for it**: if S1 is closed, it is by the rate
(not decided: the owner of the design's).

**Stage 2, how it is built.** P7's trial was made (not kept): with a shard's row padded so that the model's
`Shard` is 9 kB, as it will be with sixty-four places, the default workspace passes whole (723 tests; no stack
is short, in the tests or the tool's), a little slower (the tests' own time 227 s where it was 158 s). So the
model's `Shard` copies and has every place. To keep the tests' change small, without the model saying anything
the scripts do not:

- **The model's rows are the places.** `Shard.rows` is the sixty-three places a row can stand at, a row of no
  market where none stands; the head leaf is at the last place (63), in the model and in the constant
  `KpfTShardOpen` writes. The key after each row is not kept: it is the next key of the rows that stand, which
  is what the scripts hold a tree to. A test's row `r` is the place `r`.
- **The tests' worlds have their markets in the keys' order**, so that the rows a test puts at the places 0, 1,
  2 are the first, second and third rows of a trace.
- **The tests' shards are made with their rows put in** (the model's `of_rows`); a test that enters a leg into a
  shard of no row puts the row in first, by the model or by `KpfTRowIn`.
- **A point is said by a function** (`trace_row_point`, `trace_price_point`): a price's points are from 64.

In this order, the default workspace passing after each: (1) the tests say rows by `of_rows` and points by the
functions, the worlds' markets in order: nothing changes in what is built; (2) the tree: the scripts that show
rows, `KpfTRowIn` and `KpfTRowOut`, the model, the builders, the tool, in one commit, reviewed at the engine;
(3) `KpfTLeftAll`'s worst case measured, and its twin of three legs if it must be (P6).

**Stage 2 as it was built, and reviewed at the engine (review T10): kept, and not the form of the futures rows (17.8e).** A shard's state holds the root of its books' tree
where it held the hash of four rows. A leaf is the hash, under a tag (20), of a row's 138 bytes: its key, its
scale and limit, the net, the gross, what was entered, and the hash of the key of the row after it; a node is
the hash of its two halves under another tag (21); the head's leaf stands at the last place (63) and a row at
one of 0 to 62; the books of no row have a root that is a constant (`books::OPENED_ROOT`).

- **One script, `KpfTRows`, of two entries** (where the plan had two scripts), signed by the shard's owner:
  `put` (the futures row of a market, of no unit, at an empty place: shown the leaf before the new key and the
  key that leaf names, plain; not into a called shard; not of a market of nothing, nor of one of whose id a
  number is nothing written below zero: review T9's Q1 is closed here) and `out` (a row of no unit and nothing
  entered; a futures row only where the row after is another market's). No entry makes a row, and no leaving
  takes one out: a row of no unit stands.
- `KpfTEnterS` and `KpfTLeft` are shown the one row (138 bytes) and the six hashes beside its place's way
  (192), the place being the row the statement or the note names; `KpfTLeftAll` 330 bytes a leg, each against
  the books as the leg before left them. One function, `both`, gives the root with the row as it stands and
  the root with the row rewritten; the shard is opened by the first and written with the second.
- `KpfTLeftVoid` is shown the row of the named leg's key (the leg is gone where it is of another scale or limit
  or has fewer units), or the leaf before that key and the key it names: no row of the key.
- `KpfTShardRow` is shown the leaf at the place the trace's row says and the leaf at the place of the row
  before it (the head's, before the first): the one before names this one's key, and the trace's last names
  none, so that a trace right at every row is the rows that stand, each once, in the keys' order. A row of no
  unit adds nothing and reads no price (P1); a point beyond the trace is of nothing. Rows are the points 1 to
  63, prices the points from 64.
- `KpfTChange` holds a trace's two counts (63 at the most, written as numbers are) and its length to them, so
  that the answers read the counts and no end judges them: a trace whose counts are not its own is no statement
  (it was a statement answered wrong at its end).
- `KpfTShardPrice`, `KpfTCallPay` and `KpfTLeft.rest` are shown the root (32 bytes) and read no row;
  `KpfTCallPay` asks that the sum of the gross is nothing, not that four rows are empty.
- The model: `Shard.rows` are the sixty-three places; `standing`, `books`, `root`, `shown`, `row_in`,
  `row_out`; a trace has as many rows as stand. A shard is kept with its rows and their places. The tool:
  `shard-row-in`, `shard-row-out` (prices are still said for the first four places: a shard whose rows stand
  beyond them is not drawn from or called by the tool).

**What it weighs: more than was estimated.** The way to the root is twelve hashes written out in each script
that rewrites a row (six levels, the root before and the root after), about 650 bytes, where review T8 reckoned
some 150. Measured: `KpfTEnterS` 3,573 bytes (2,966), `KpfTLeft` 3,732 (2,914), `KpfTLeftVoid` 3,618 (2,093),
`KpfTLeftAll` 10,042 (5,059), `KpfTChange` 1,851 (1,760), `KpfTCallPay` 2,401 (2,567); a note of one leg
applied is a transaction of about 5,735 bytes (4,862: 18 % more), an entry stated about 6,160 (5,550: 11 %
more; not yet measured as one number). A row shown with its path is 330 bytes where four rows were 420. A
tree is the lighter form from about nine rows on (four rows shown whole were 420 bytes and no hash); the rows
of options are dozens. **`KpfTLeftAll` at 10 kB is P6's case**: a note of two legs pays it. Its twin of three
legs, or another form of a note of several legs, is the next piece (not built). And a script's stack has 244
places, every name of a function set into an entry being one: `KpfTEnterS` has none to spare (the path's
function has four names for it).

**Review T10** (`docs/audit/REVIEW-T10-tree.md`; its tests `auditT10_tree.rs`, twenty, kept with the tree; of
the tree's own mechanics, the earlier reviews' tests not brought along: 17.8e). No finding above medium; the
tree holds at the engine. The path: on sixteen random books of 0 to 63 rows a row opened the root at its place
and 1,926 other forms were refused (a place of 64 and more, a leaf a byte longer or shorter, a node shown as a
leaf, the empty leaf where a row stands). A row put in: 110 forms refused, a check at a time (the path of the
empty place taken from the books before the leaf before was rewritten is refused at every place). A row taken
out: 43 forms refused. The order: 570 random steps of rows put in and taken out, honest and not, up to a full
tree, the engine taking what the rule takes and the list from the head whole after each; sixty-three rows
stand, and one more is refused at every place. A trace's rows: 200 stated traces (145 of them lies) and 2,416
answers as the model says; 56 forms of counts and lengths are no statement. A shard with a statement on it is
another object: no row is put in or taken out between a statement and its answer.

| Finding | What it is | For the options' tree |
|---|---|---|
| U1 (low to medium) | a note of a leg whose key is a standing row's by its numbers and not by its bytes is spent by `KpfTLeftVoid` with no leaf: the row of the same numbers is neither below nor above the key (before the tree the lookup was by bytes, and the note was spent) | the second arm takes a leaf whose key is neither below nor above the leg's and is not its bytes |
| U4 (low today) | `KpfTRows.put` holds "a market's futures row is its first" by the key the leaf before names alone: after a row of the market made by hand with a day below nothing it takes the futures row | a dated key's day, kind and strike are held above nothing and written as numbers are, in the script |
| U5 (low to medium; the model) | `trace_wrong_point` names a rightly priced entry wrong where its market's row stands at no unit and no price is given for that place | the model judges an entry it has no price for by its order alone (done for the four rows: below) |
| U3, U6 (low; the model) | `with_row_for` puts a row in with none of the checks; `Books::moved` writes nothing entered where the scripts keep it; a row with something entered and no unit is taken out by nothing; `standing` sorts twin keys by no order | with the day, which writes "entered" |
| U7 (low to medium; the tool) | a shard kept in the form of before the tree is not read back | a shard is kept in one form a stage; an older one is read by its own reader |
| U8 (by design) | `KpfTCallPay` reads the sum of the gross alone: every script that writes a row keeps the sum | said |

Measured: `KpfTRows` 6,086 bytes (review T8 reckoned 2.2 to 2.7 kB), a row put in 7,897 and taken out 7,992;
`KpfTShardRow` 3,985; an entry stated 6,068 (5,550); a note of one leg applied 5,591 (4,862), of two legs
12,357, of six 14,154; a trace's row answered 6,311 (three rows) to 8,767 (sixty-three); a price answered
8,663. Kept, beside the repository's tree (`run/parked/tree-all-rows/`: the patch against commit `c0ef7bd`,
`KpfTRows.sil`, the review's tests; the patches of the scripts `s351`, `s352`, `s353`, `s355`, `s365`).

### 17.8e The common paths made lighter (decided 28 November 2026; on paper)

An operation's transaction carries the whole script of the operation: about half of every transaction is
script, and the scripts grew with every hole that was closed. What only a called shard needs, and what only
books of many rows need, stands in the scripts every entry and every leaving pays for. The yardstick is the
common path: an entry stated and a note of one leg applied. In bytes of script, each number the measured
difference of the piece when it was built (none measured with the piece taken out):

| | `KpfTEnterS` | `KpfTLeft` | costs |
|---|---|---|---|
| with the tree for every row (17.8d, stage 2) | 3,573 | 3,732 | |
| a market's futures row one of four written whole; the tree for dated rows only | less 607 | less 818 | four futures markets a shard (a row more written whole is 105 bytes in every transaction that shows the rows: eight would be 420 more) |
| what a called shard alone needs (the class of a claim) in scripts of its own (built: below) | | less 275 | three scripts more in the library; none in what holds |
| the row's gate judged where an entry is disputed, not where it is stated | less about 400 | | see below: not decided |
| | about 2,470 to 2,970 | about 2,500 | |

**An operation a script (built).** A script is shown whole by a transaction that uses one entry of it, and
`KpfTLeft` carried two operations: the applying of a note of one leg, and the paying of a note of the rest.
The second is a script of its own, `KpfTRest` (1,053 bytes), and `KpfTLeft` is 1,949 (2,914). **A note of one
leg applied is a transaction of 3,897 bytes (4,862): a fifth less**, with nothing of what is held changed (the
same lines in two scripts; the default workspace 723 tests, none failed). It was not in the table above: it is
more than any line of it. The other scripts that carry more than one operation are on no common path
(`KpfTCTok` 3,492: an answer and a lapse; `KpfTPoint` 2,588: a dispute, a lapse, a dismissal; `KpfTCallPay`
2,567: a claim and the call's end; `KpfTLiq`, the disputes, `KpfTExit`, `KpfTCancel`, `KpfTOwed`): each is
taken apart when its path is looked at.

**1. The tree is for the dated rows.** Stage 2 put every row in the tree, and only futures rows exist: every
entry and leaving paid twelve hashes for books of a few rows. A tree is the lighter form from about nine rows
on; a shard's futures markets are few, and the rows of options are many. So: the futures rows stay as stage 1
has them (four, written whole, 420 bytes, made by an entry and emptied by the last leaving), and the books'
hash holds beside their hash the root of a tree of the dated rows, which only a leg with a day reads. **Stage
2 as it was built is not committed as the form of the futures rows.** What review T10 holds of the tree's own
mechanics (the path, a row put in and taken out, the order kept, the leaf before as the proof that no row is
of a key, a trace's rows as a list from the head, the counts held where a trace is stated) is the dated tree's,
as built; it is kept (`run/parked/tree-all-rows/`, with the model, the scripts, `KpfTRows` and the review's
tests) and comes back with the option rows, where the paper has to say anew: where a dated row's
scale and limit come from (its market's futures row is not in the tree: shown among the four, which must then
not be emptied while a dated row of the market stands), and how a trace goes over four rows and then a list.
Lost with it for now, and closed otherwise: review T9's Q1 (two ids that are one market by their numbers) is
closed in `KpfTEnterS`, which makes a row: a market of which a number is nothing written below zero makes none
(built, with the model's `entered`: 49 bytes of `KpfTEnterS`, now 3,015, an entry stated 5,599; and review
T9's Q2: the model judges an entry no row is of by its order alone). The default workspace: 723 tests, none
failed.

**2. A called shard's scripts (built).** A note applied to a shard that is not called pays or leaves a note of the
rest; applied to a called shard it is counted in one of two classes, with the young legs' window, a step's
note of another tag, and what was paid already. The second is rare and is 410 bytes of `KpfTLeft`, 783 of
`KpfTLeftAll` and 229 of `KpfTMarkNote`, in every application. So: `KpfTLeft`, `KpfTLeftAll` and `KpfTMarkNote`
take a shard that is not called (and a note of an account's own: no step's note exists without a call), and
their twins `KpfTLeftC`, `KpfTLeftAllC`, `KpfTMarkNoteC` take a called one; each holds its shard to its state
(called, or not), so that no note is applied by the wrong one. Nothing of what is held changes. Measured:
`KpfTLeft` 1,674 bytes (1,949 after the paying of a note of the rest left it; 2,914 before), `KpfTLeftAll`
4,369 (5,059), `KpfTMarkNote` 1,188 (1,419; and it is shown the books' hash, 32 bytes, where it was shown 68);
the twins 1,953, 5,063 and 1,423. **A note of one leg applied to a shard that is not called is a transaction of
3,622 bytes: it was 4,862 before this section, a quarter less.** To a called shard, 3,993. The builders name a
note's script by its shard's state (`left_script_on`, `mark_script_on`), and the tests of every review apply
their notes so; given the twin, a builder builds with it, and the engine refuses it both ways, for each of the
three pairs (`a_note_is_applied_by_the_script_of_its_shards_state_and_not_by_its_twin`). The default workspace:
724 tests, none failed. No review of its own: the lines are the same lines, in two scripts.

**What a script more costs.** A library's genesis writes a copy of every script, and an output under a
covenant weighs four million million over what it holds, in a block of 500,000: with 75 scripts a copy holds 7
KAS for the genesis to be one transaction (6 did for 72). An operation a script, and a twin a state, is paid
for there, once, by whoever makes a library; it is nothing in a transaction that uses one.

**3. The row's gate, where an entry is disputed: not decided.** The gate (17.8c) is about 500 bytes of every
entry. Judged at the entry's end instead, the entry would say the four numbers it read (what the shard holds
and owes, the row's net and gross before it), `KpfTEnterS` would hold them to the shard, and the end's answer
would compute the requirement. But the gate is there so that an owner's leg that its shard cannot carry is
never in the books (review T3, L4: a call is due at once, and is stated at once); judged only where somebody
disputes, the leg is in the books for a dispute period, and a call stated in that time reads it. That is a
game in time between a call and the dispute of an entry, and three reviews went into closing it. It is taken
up only with a model of that game, and after the two above.

**What is not a matter of bytes.** Below about 2,500 bytes of script and as much of arguments and objects, an
operation of this form is not made lighter by taking things out: a script would have to be named and not
shown, which the chain does not give. And before bytes: an entry and the applying of a note spend the shard,
one output. The users of a shard act one after another, and two that act at once are one of them refused and
stated again. For a clearing party of many users that is the limit that is felt first; it is not designed
here, and is the next question of the form (rows, or accounts' notes, that are objects of their own, so that
what two users do does not meet in one output).

**Left for a last phase of refactoring and of cost** (the owner of the design, 28 November 2026: the form is
finished first; what makes a transaction lighter, and the rates, are done together at the end). Each is written
here when it is found, with its number, and is not built:

| What | What it would give | Why not now |
|---|---|---|
| the row's gate judged where an entry is disputed (3, above) | about 400 bytes of an entry | a game in time with a call: a model first |
| a note of two or three legs by a script of three (`KpfTLeftAll` unrolls six: 4,369 bytes for any note of two to six) | about 1,600 bytes of a note of two or three legs (reckoned) | a script more; a twin for a called shard |
| an operation a script, for the scripts that still carry more than one (`KpfTCTok`, `KpfTPoint`, `KpfTCallPay`, `KpfTOwed`, `KpfTLiq`, the disputes, `KpfTExit`, `KpfTCancel`) | what the other operations weigh, in each | none is on a common path |
| `KpfTEnterS`'s stack (244 places, none to spare) | room for a line more in an entry | nothing needs it yet |
| the most legs an account has (six) | more legs an account | every script that goes over the legs is unrolled to the most: about 540 bytes a leg more (reckoned), in every transaction of several legs |
| the rows a shard writes whole (four) | more futures markets a shard | 105 bytes a row in every entry and every note applied |
| **S1's rate** (17.8f): the upper rate on the gross at 3.13 % | a leaving never leaves a shard below its lower level | it is a rate, with the rates; until then S1 is open as review T7 found it |
| the band where legs leave apart (17.8, 1: a leg's band is at most what that leg gained) | one rule for a band, apart or together | a number, with the rates |
| what a copy holds at a library's genesis (7 KAS a copy with 76 scripts; 8 from the 77th: the genesis is one transaction, and an output under an id weighs 4 x 10^12 over its value against a block's 500,000) | a library made for less (78 outputs of 7 KAS today: 546) | a genesis in several transactions, or fewer scripts: with the scripts' final count |

### 17.8f S1 in numbers: the rate on the gross (a model; no rate changed)

Review T7's S1: a leaving raises its row's requirement, and nothing asks the shard to hold it. An owner's long
and short of one row are asked little while they set each other off; the owner draws down to that; one of them
leaves, and the shard is below its lower level at once: a call is due at a time of the owner's choosing. The
direction taken was the rate, not a rule for the leg that stays (17.8e; the owner of the design, 28 November
2026). The model (`s1_what_a_shard_that_held_its_upper_requirement_holds_after_legs_leave`, in `thin_draw.rs`:
20,000 random books of one to four rows that hold exactly their upper requirement, after legs leave at the
same prices; the maintenance rate 5 %, the tolerance 0.5 %):

| the upper rate on the gross | below its lower level after legs left | by, at the most | a row set off is asked, of one side | a one-sided row |
|---|---|---|---|---|
| today: the lower rate on the gross and a tolerance, 2.25 % | in 14.5 % of the books | 1.74 % of the side that stays | 4.50 % | 7.75 % |
| half the upper rate on the net, 2.75 % (what review T7 and I said first) | in 3.3 % | 0.74 % | 5.50 % | 8.25 % |
| **half the two lower rates together, 3.13 %** | **in none** | | 6.26 % | 8.63 % |

**What I said first does not close it.** At 2.75 % a row set off holds what its larger side is asked at the
upper rate on the net alone, 5.5 %; but a one-sided row's lower level is the two lower rates together, 6.25 %:
the shard is still below it once a side leaves. What closes it is that a row set off whole holds what one of
its sides alone is asked at the lower level: the upper rate on the gross is half of the lower rate on the net
and the lower rate on the gross together, (4.5 % + 1.75 %) / 2 = 3.125 %. A requirement is a sum over legs, so
a shard that held its upper requirement is then at or above its lower level after any legs leave, in any
order (the model's 20,000). It is a number in two scripts (`KpfTEnterS`'s gate, `KpfTShardEnd`) and the model:
no byte. What it costs is what the clearing party holds before it draws or takes a leg: a book set off whole,
6.26 % of one side where it is 4.50 % (39 % more); a one-sided book, 8.63 % where it is 7.75 % (11 % more).
The level a call is due at does not move. (A form that asks the more of today's upper requirement and of the
larger sides' lower levels leaves a one-sided book at 7.75 %, for some 70 bytes in each of the two scripts;
`KpfTEnterS` has no place of its stack to spare for it.)

**Not decided:** the owner of the design agreed to "the rate" on my first number (2.75 %, a fifth more for a
book set off), which does not close S1. The choice is 3.13 % (closed; 39 % and 11 % more), 2.75 % (narrowed:
a call still due at once, with at the most 0.74 % less than the lower level), or nothing until the rates are
set. Whichever: the numbers of many tests move with it, and it is reviewed at the engine.

### 17.8g The day's price, and an ended leg (17.8, 5; second writing, after review T11; on paper)

17.5 says what holds; this says the objects and the transactions, and what of it is built now. The first
writing (kept below, with the review's table) let every transaction take the day's note it liked, and a
committee's attest takes its price, within the spread, from whoever builds the transaction: two true notes of
one day differ. So: **a shard has one price a market and a day, written once into its books, and every leg of
the shard ends by it.**

**A. The committee's note of a day** (built first, alone).

- **The object** (`KpfDay`): under the library's id, in the form the fee pool has (its `go` asks the id of its
  own input and that a script of the library names it), its state `blake2b(push8(22) + push(committee) +
  push(market) + push8(day) + push8(price) + push8(spread))`. No owner; it holds `DAY_MIN` (a note's least).
- **`KpfTDay.write(u, n, rid, mkt, t0)`**, anyone: the twin of `KpfTNote.note`. Beside the committee's epoch
  table spent by its attest for the market and the time `t0` (every line `KpfTNote` has of that: the table of
  the time, the quorum, the time not ahead of the chain); the price is the attest's and above nothing, the
  spread the table's; output `n` is the object of that state, under the library's id, holding `DAY_MIN` or
  more; its copy goes on with the fee; one input and two outputs under the library's id. No citation: a price
  note's signatures are public and are used again while a table of the day's window lives.
- **`KpfTDay.copy(who, ageb, x, n)`**, anyone: the note at the place `who` names, of the state `x`, goes on
  as it is (its script, the id, its value or more), and output `n` is a second note of the same script
  holding `DAY_MIN` or more. Two inputs and three outputs under the library's id.
- **A reader** names the note at a place of `who`; holds its input to the library's id and to the script of
  the state it is shown (the committee, opened from the shard's id by the seed; the market and the day of
  what it prices, equal; the price; the spread); asks four times the spread within its terms' tolerance and a
  price above nothing, and uses the lower of the price and its row's limit; and writes the note again as it
  is, its value or more (never a least: what a note holds above a copy's floor can be swept).
- A day's price is attested within the life of the committee's tables of that time (an epoch or two), or
  never: whoever holds a leg of the day, or will have its keeper's fees, writes the note in that time.

**B. The shard's price of a day** (built with the dated rows of the tree: 17.8's seventh step).

- **A leaf of the day** in the tree of dated rows: its key the market and the day, its kind "the day's
  price"; it holds the price, or "none". **Put in once**: by anyone, in a transaction that spends the shard
  beside a committee's note of that market and day (a reader, as above: the spread is held here, once, against
  the shard's tolerance), where no leaf of that key stands (the leaf before, as a row is put in).
- **"None"** is put in the same way, with no note, shown an input that landed at or after the day and the
  fallback's time (two epochs of the committee at the least), where no leaf of that key stands. Whichever is
  written first is the day's for that shard: the fallback cannot be taken where a price was written, nor a
  price after it.
- **The shard's note of the day** (state `blake2b(push8(23) + push(sid) + push(market) + push8(day) +
  push8(price or nothing) + push8(1 where none))`) is written by that transaction and copied by anyone
  (`KpfTDay.copy` takes either note). **It is all the end of a leg reads**: of its account's shard, by
  equality of the market and the day, and nothing of a spread.
- A draw's and a call's trace count a dated row of an ended day at the leaf's price, answered by the leaf;
  where none is written yet the row is priced as a row of a day to come (the price now, stated and answered).

**C. An ended leg leaves by one transaction of anyone's, always** (`KpfTDayLeg.end`; built with the account's
side, 17.8's sixth step, and B).

- The account, not under a statement (a statement holds the account's hash; an ended leg leaves in the time
  14 leaves a step); the shard's note of the leg's market and day, which goes on; the leg named.
- **What it gained** is what it is worth exercised at the day's price less the same at its entry (the lower
  of the price and the leg's limit), and for a held option less its premium, and less the entry's part of
  its band (17.5).
  - **Below nothing**: the coins pay it, as far as they go, into a note to the shard that says the leg; what
    they do not pay is added to what the account owes (16.18).
  - **Above nothing**: it is **credited to the account**: the number of 16.18 goes down by it, below nothing
    where the account owed less (the shard then owes the account). It is no coin. It counts for the account's
    standing in every test, is turned into the account's coins by a payment of the shard into it, and is
    taken out by a take under the take's test (the coins at what stays is asked, the premiums, what each leg
    lost alone): the model's rule of 17.8, 1, with nothing left in the account that is no leg and no number.
    A step sets it against what the legs lost; under a call it is what the shard owes: a claim of the first
    class (an ended leg is never young: none enters within T of its day).
  - A written option's premium is its user's coins from this transaction on (17.3: "at the day").
- **"None"**: the leg ends at its entry. A held option's premium is its user's coins again; **a written
  option's premium stays its user's** (it does not return to the owner: silence of the committee the owner
  named pays the owner nothing).
- **The note to the shard** says the leg whole (its row's key and what its legs were entered at come from
  it; a leg says whether it is a call or a put and held or written in the place a futures leg says its way),
  and what is asked of the shard or paid to it; the dated row loses the leg when it is applied.
- **The keeper's fee**: after what the leg lost and its premium, from the coins; never owed; none where the
  input shown landed before the day and a lateness (its user ends its own leg free). Six legs, six fees at
  the most.
- A leg whose day came before a call's time ends by the day's rule, not the call's.

**D. M1**: a step's test counts what a step would leave the account. What the legs gained to the price named,
where it is above nothing, is counted less every band the step takes (the futures legs', and an option's at a
stated price), and no less than nothing; where it is below nothing, as it is. A step is due where the coins
and that are below the premiums. (Not "less the bands" outright: an account whose legs set each other off
gained nothing and gives up no band, and would be due for bands it never pays.) Shown in the model
(`due_stepped`, in `a_held_leg_that_gained_at_its_day_leaves_only_where_the_account_stands_without_it`): the
thirty-four paths that left the shard unpaid leave it paid; an account that took all a take gives is due at no
price after a move of the lower rate.

**What is built when.** A now: the note and `KpfTDay.write`. B with the dated rows; C with the account's side.
What 17.5 said otherwise is replaced by this: the price "written once" is the shard's leaf; a leg that gained
leaves by the one transaction, credited; an ended leg is passed by no row (it has left); a citation answers no
day.

**A as it is built** (the engine; `tests/thin_day.rs`, four tests; not yet read by a reviewer, and the tool has
no command for it yet: both with the first script that reads a day's note).

- **The note is no contract of its own**: it is the fee pool's script (`KpfFeePool`, 137 bytes) with the day's
  state where the fee pool's is. `KpfTDay` holds its output to that script by what stands before and after the
  state (as `KpfTNote` holds a price note's).
- **`KpfTDay.write(u, n, rid, mkt, t0)`** is `KpfTNote.note` line for line, but for: the tag (22), no owner, the
  least (`DAY_MIN`, 0.2 KAS, where a price note holds 1), and **a price above nothing** (the committee's attest
  takes a price of nothing, and the price note's script writes it; shown). 761 bytes; its transaction 5,771
  (a price note's 5,769). The library has 76 scripts.
- **Shown at the engine:** a note of another price, market, day, committee or spread than the attest's, a
  second note, a note under no id, a price note in its place, a note before its day: refused. The signed prices
  of a price note write a day's note (the table as it went on). **Two true notes of one day** (the members
  signed one price; a second note is written a spread above it): review T11's D1, as the engine has it. A time
  no table covers has no attest.
- **A sweep and a day's note** (D5): `KpfTSweep` takes what a note holds above a copy's floor (it passes for a
  copy), and the note stays, of its state, at the floor; a note at its least is below the floor and nothing of
  it moves; a day's note named where the sweep reads the fee pool is refused. So a reader asks the value the
  note had, never a least.
- **`copy` is not built here.** It was for the notes many transactions read at once, and those are the
  shard's notes of B (every ended leg of a shard reads one); a committee's note is read once a shard, and
  anyone writes a second from the same signed prices. It comes with B, for both notes, as a script of its own
  (an operation a script), and takes the state opened (the committee or the shard, the market, the day, the
  price, the fifth number) and not its hash: a fee pool is of the same script, and a copy by the hash alone
  would write a second fee pool.

#### The first writing (replaced)

17.5 says what holds; this says the objects and the transactions, and what of it is built now.

**A. The day's note** (built first, alone: nothing else needs to exist for it).

- **What it is.** An object under the library's id, in the form every object has (a slot of the library and
  the hash of a state): `blake2b(push8(16) + push(committee) + push(market) + push8(day) + push8(price) +
  push8(spread))`. A day is a time as a statement's time is. It holds a note's least, from whoever writes it,
  and has no owner: nobody takes it back.
- **How it is written** (`KpfTDay.write`, anyone): beside a citation (a copy of `KpfTCite` spent by `cite`) of
  that committee, that market and the time that is the day, whose price and spread it copies (the six pushes
  at the head of the citing input, read as `KpfTShardPrice` reads them). So a day's note says what the
  committee signed for that time and what a chain block took, and nothing else can be written into one.
- **How it is read.** A transaction that needs the day's price spends the note as an input and writes it
  again as it is (its state, its value or more): its script holds the note to the hash of the committee, the
  market, the day and the price it is shown. A note read is one output, so one transaction of a block reads a
  given note: **`KpfTDay.copy` (anyone) writes a second note of the same state** beside the first, and whoever
  ends many legs of a day makes the copies it needs. (A citation is read by any number of transactions at
  once, but only while its block is within the finality depth, and it is 5.6 kB a transaction: the day's note
  is what stays, and is 32 bytes of state.)
- **When.** The committee signs when it is asked (its prices are not a feed: decided 22 November 2026): whoever
  holds a leg of the day, or anyone who will have the keeper's fees of the legs it ends, asks for the price of
  the day, writes the price's note and the citation's transaction while they hold, and the day's note from
  them. Nothing can be written before the day, since no price of a time is signed before it.
- **Two notes of one day that differ** would be a committee that signed two prices for one time: the fault
  14 has for any price (two citations of one time that differ), and no more here. A reader takes the note it
  is shown; a note is a public object, and the two are the proof.
- **What a reader asks of the spread** is what an answer of a price asks (four times the spread within the
  tolerance of the reader's terms). A day whose only notes have a wider spread is a day with no price for
  that reader: the fallback.

**B. An ended leg** (built with the account's side, 17.8's sixth step: it needs a leg that has a day).

- **A leg is ended** where its day is at or before the time of the transaction's inputs, as times are held
  elsewhere (an input that landed at or after the day shows it).
- **It stays a leg until it leaves.** Nothing new is kept for "a gain that stays": the leg is in the account
  as it was, and every row that passes it counts it at its day's price (17.5: it adds what it gained or lost
  there and nothing to the two sums), the price answered by the day's note.
- **One transaction of anyone's, no statement** (`KpfTDayLeg.end`: the account, not under a statement; the
  day's note, which goes on; the leg named), for the two cases that need no other price:
  - *it lost* (what it is worth exercised at the day's price less the same at its entry, and for a held
    option its premium: together below nothing): the coins pay it, as far as they go, into a note to the
    shard that says the leg (as a leg that left says it, the amount below nothing: 16); what they do not pay
    the account owes (16.18). Always, whatever stays.
  - *it gained and is the account's only leg*: the note to the shard asks what it gained less its premium,
    for the user's key.
  The sender has a keeper's fee from the account's coins, the least the terms say.
- **It gained and other legs stay: by the user's own statement of a leaving** (the kind that is built, with
  its rows and its dispute period), the leg's row at its day's price. The test of what stays needs the prices
  of the legs that stay, and a price is stated and answered, not computed: this is the one case that is not
  one transaction, and it is the user's to make (the gain is its own; until then it counts for the account's
  standing and is no coin: the model's rule, 17.8, 1). A step and a call's step take it with the others.
- **The shard's books** have the leg out when its note is applied (an option's row, in the tree: 17.8e).
- **The fallback** (17.5): where no note of the day can be shown by the day and a time the terms say, the
  same transaction ends the leg at its entry, shown an input that landed after that time: nothing is asked
  either way; a held option's premium is its user's coins again, a written option's returns to the owner's
  key.

**C. M1** (17.8, 1: a step pays what the legs gained less the band of the futures legs, and its test counts no
band). Proposed: **the test counts it**: a step is due where the coins and what the legs gained, less the band
the futures legs would give up, and the sum to the price named, are below the premiums. The band stays what it
is (it is there for the tolerance of a stated price, and a step's price is stated); an account is then stepped
a band sooner, and pays whole.

**To be said by the review:** whether a day's note that anyone copies and nobody takes back is the right form
against one written into each shard (the books would hold a day's price in its row: a leaf of 8 bytes more,
written once by a transaction that spends the shard); what `KpfTDay.write` must hold of the citation beside
the three pushes (the committee's table of the time; the block's depth); whether "a leg is ended" by an input
that landed at or after the day can be played by whoever chooses the inputs; the two cases of one
transaction, and whether a leg that lost may leave while a statement stands on its account; the keeper's fee
and who can drain an account by ending its legs one at a time; the fallback's time and who gains by a
committee's silence at a day (17.5 has N72: the written side); M1's two forms; and what of B can be tested
before option rows exist.

**Review T11** (`docs/audit/REVIEW-T11-day.md`; on paper, nothing run). **Change first: 17.8g is not built as
it is written above.** It is rewritten before any script (not yet done):

| Finding | What it is | Direction |
|---|---|---|
| D1 (high) | **two notes of one day differ with no fault of the committee, and whoever sends picks.** The committee's attest takes its price from whoever builds the transaction (each member's signed price within the spread of it): at a spread of 0.25 % any price from 99.75 to 100.25 is a true note of one time. A user that holds a call in one account and wrote it in another of one shard ends each by the note it likes: twice the spread of the notional a day, from the shard. 17.5 took no band at the day because the price was "written once" | one price a shard, a market and a day, **in the shard's books** (a leaf put in once by a transaction that spends the shard beside a day's note; the spread held once), and a note of the shard's day that the ends of legs read |
| D2 (high) | the fallback is not exclusive of a note: no script shows that no note is; after the fallback's time a leg is ended either way, by whoever gains | the same leaf says a price or "none", and the first written holds |
| D3 (high; A cannot be built as written) | `KpfTDay.write` beside a citation cannot write an object: a citation's transaction has as many outputs under the library's id as inputs, and only `KpfTNote.note` writes one more | the day's note is written as a price's note is, beside the committee's attest (a twin of `KpfTNote.note`): no citation, nothing of a block's depth |
| D4 (medium) | the fallback pays the owner twice for a committee's silence (it does not pay holders in the money, and has the written options' premiums back); and a day's price is attested within one or two epochs or never (an epoch can be passed over) | at the fallback a written option's premium stays its user's; the fallback's time at least two epochs; a keeper's duty |
| D5 (medium) | `KpfTSweep` takes a day's note down to a copy's floor | a least of its own; a reader asks the value it had, never a least |
| D6 (medium) | a leg that lost, ended first, leaves the account owing, and an owing account does nothing (16.19): its user cannot state the leaving of its ended gain | gains are set against what is owed; an owing account may leave an ended leg that gained |
| D7 (medium) | "gained and others stay: by the user's statement" leaves dead legs and rows: an option written that ended worth nothing (the commonest end) needs a statement, and a dozen accounts of dust hold every row of a tree | **every ended leg leaves by the one transaction, and what it gained less its premium is credited to the account** (16.18's number below nothing: counted, no coin, taken by a take under the same test); the row is free at once |
| D8 (medium) | M1 | the test counts every band the step takes (the options' as well); the model first |
| D9 to D11 | the keeper's fee's order and time; what a library's move strands; what 17.8g drops of 17.5 (the entry's part of the band at the day; a call's trace where no price was written; a leg whose day came before a call) | said in the rewriting |

### 17.8h The account's side, how it is built (17.8, 6; on paper, for review)

17.4 says the rule, and the model has it in numbers (`research_options.rs`: `asked_as`, `lacks`, `due_stepped`).
This says the row, the head of a statement, the test of each end, and the order. Nothing of it is built.

**The row as it will stay: 224 bytes** (232 after review T12: PO, below) (today 120: a price, six sums, two chains). Twenty numbers and the two
chains; futures and options, so that the row is changed once.

| Number | What it is |
|---|---|
| a | the price stated for the leg's market (as today) |
| q | a step's price for it (17.4: a step is due by one price); in every other statement the same as a |
| G, W, WS, LS, GL, TK | as today: what all gained; what they are worth; the worth of those that stay; what those lost, each alone; what those that leave gained; what the token legs count |
| M | what the legs gain from a to q (an option within T of its day: only what it loses) |
| BA, BL | the bands of all the legs, and of those that leave (a futures leg: twice the tolerance of its worth, as today's is of the sum; an option: 17.5) |
| DdA, DuA, RA | of all the legs: the two sums of the group that is open, and R of the groups that are closed |
| DdS, DuS, RS | the same of the legs that stay |
| RL | of the legs that leave, each alone: the larger of nothing and what it loses at either end |
| PA, PS | the premiums of all the option legs, and of those that stay |

- **L and U** of a row: a less, and a plus, the rate on a rounded up (L not below nothing). **q** is within
  the lower rate of a, or the row is wrong.
- **A futures leg** adds to the two sums its worth at L and at U less its worth at a, one way or the other.
  **A token leg** adds nothing (it counts as coins: TK). An option: 17.4, with the stage after this.
- **A group closes** at a row whose leg's market is not the leg before's: R grows by the larger of nothing,
  what the sum down lost and what the sum up lost, and the two sums begin again. The end closes the last group
  (R' below). So that a row knows the leg before, **its answer is shown that leg** as well (its chain is the
  row before's, opened from the chain two rows before): 111 bytes of an answer, none of a statement. Two rows
  next to one another, of one market, that say two prices (a, or q): the second is wrong.

**The head of a statement** says, after what it says today (an amount, the leg that leaves, its chain): **the
rate** (the upper: the maintenance and the tolerance), **the lower rate**, **T** (twice a lateness and a
dispute period: 17.4), and **the lines** (a bit a leg: for a held option whose strike is between the ends, which
of its two lines): 32 bytes. The rows compute with what the head says; **the end holds the three numbers to the
account's terms and times**, or the end is wrong. (A row is answered without the account's terms opened, as
today.)

**An entry says the place its leg takes** (17.4: a hedge stands next to what it hedges, or it is not netted).
Today the leg that enters is the last. So: the head of an entry says the place; the row at that place is of the
leg said, at its entry; the second chain passes it by, and the last row's second chain is the account's legs
as they were (the first, as today, the legs as they will be).

**The ends** (m: the coins less a note's least; tk and owes as today; R' is R with the last group closed):

| Statement | Today | So |
|---|---|---|
| an entry (10) | m + G + tk at the rate on W | m + G + tk at RA' + PA |
| a take (7) | m - amount + tk at the rate on W, and LS | at RA' + PA + LS |
| a leg that gained leaves (8) | m + tk at the rate on WS, and LS | at RS' + PS + LS |
| a leg that lost leaves (8) | the coins pay it | and what stays is at the level of an entry (N65): m + GL + tk + (G - GL) at RS' + PS |
| every leg leaves (8) | the account ends | the same (with options: wrong while the account holds or wrote one) |
| a split (12) | what stays at the rate on WS and LS; what moves at the rate on its worth and what it lost | at RS' + PS + LS; at RL, its premium and what it lost |
| a mark (13), tokens leaving or sold (14, 15) | the rate on W where they test it | RA' + PA in its place (RS' + PS where a token leg leaves) |
| a step (9) is due | m + G + tk - owes below the lower rate on W | **m + tk - owes + X below PA**, where X is G + M less BA and no less than nothing where G + M is above nothing, and G + M as it is where it is below (17.8g, D: what a step would leave the account) |
| a step, the leg named | it lost, and the coins pay it, the fee and LS: it leaves alone; else every leg | **it leaves alone where the account lacks less after it than before** (17.4): before, RA' + PA less (m + tk - owes + G), or nothing; after, RS' + PS less (what the coins are once the leg, its premium and the fee are paid, tk, less what is owed then, and G - GL), or nothing; else every leg leaves (not while a token leg stays, as today) |

What a note asks of the shard, the fee's base, what is owed (16.18), the times: as they are built. The bands
are read from BA and BL where they are computed from W today.

**What it costs.** A statement grows by 104 bytes a leg and 32 for its head: a take of an account of one leg
by 136 bytes, a statement of six legs by 656. A row's answer by the leg before (111) and the script's lines.
For the last phase (17.8e): the numbers a kind does not read (q, M and the sums of all the legs are a step's;
RL a split's) are 48 bytes a row in a statement of any other kind.

**The order.**

1. **Futures and tokens, whole** (one commit: a take by R with a step by the rate on W would step a hedged
   account the moment its take is paid). The row and the head as above with the options' numbers at nothing;
   `KpfTCRow`; the ends (`KpfTCEnd`, `KpfTCEnter`, `KpfTCSplit`, `KpfTCMark`, `KpfTCTok`); what reads a row by its
   place (`KpfTStepPrice`); `covered.rs`, the builders, the tool. **What a futures account is asked changes**: a
   long and a short of one market next to one another are asked nothing but what each lost alone; an account of
   one leg, or of legs of several markets, what it is asked today (to the rounding). The tests of amounts
   change with it, and say which.
2. **The option leg**: what it is worth (17.2) where a futures leg has its way (a call or a put, held or
   written); its premium in PA and PS; a held option between the ends along the line the head says; within T
   of its day only what it loses; its band; a mark names none; a leaving of every leg is wrong beside one; what
   a step pays of a premium (17.3). An account with an option leg is made in the tests as an object is (the
   entry with its premium is 17.8's seventh step).
3. With 17.8g's C: what an account owes may be below nothing (a credit).

**To be said by the review:** whether each of the twenty numbers is needed and whether one is missing (the two
chains and "the leg before" against carrying the market in the row); the lines in a step, where the stater is
anyone: proposed, **a step computes both "lacks" with every held option between the ends on its line of
nothing**, the head's bits read in the user's own statements only (to be run in the model first: `lacks` by
`asked_flat`); whether "lacks less after than before" with what is owed (16.18) in it can be played by a stater
that names a leg (the fee is of the leg's worth; a leg named again and again); the leg that lost and leaves by
its user's statement, held to the level of an entry (a user that cannot leave a losing leg: is that right, and
what does it do then); a split's leg alone at RL; the rounding of L, U and of a leg's worth at them against
today's rate on W, and that no account of one leg is asked less than today; the place of an entry and the two
chains; what `KpfTStepPrice` answers for q (nothing: q is the stater's, held to a by the row); and what of the
call's scripts reads an account's row.

**Review T12** (`docs/audit/REVIEW-T12-account-side.md`; on paper, nothing run; it holds the row's and the
head's places and each end as a line, to build from). **The row stands** (the twenty numbers, the group open and
closed, the leg before shown in an answer, the head held by the end, futures and tokens in one commit). Changed
before any script:

| Finding | What it is | Decided |
|---|---|---|
| E1 (high) | by "lacks less" alone a step names a leg that **gained**: its gain is paid out to the user's key (a note asks it of the shard) while a leg that stays has lost more than the coins pay. A long that gained 3,000 beside a short that lost 8,000, coins 5,000: named, the long "lacks less" (11,000 to 9,000); 2,000 leave for the user, the fee is paid, and the short's loss is then short by 3,500. The model paid the gain into the coins and did not see it | **a leg that gained is named by a step only where no leg that stays has lost** (as built). "Lacks less" is asked of a leg that lost. Again with 17.8g's C (a gain credited) |
| E2 (high) | an entry's place is not the rows' alone: `KpfTEnterS` writes the marked account's chain itself with the leg at its end, and `KpfTEnterOff` opens it to undo; a place before the end needs the legs after it shown there, or the old chain kept | **stage 1 keeps the leg that enters last**; the head of an entry has the place's 8 bytes, held to the count; the place is a commit of its own. (Until then a hedge nets where it is entered next after what it hedges) |
| E3 (medium) | "what is owed then" lets a step leave an account with no token owing: named, a short that lost 14,500 beside a long that gained 20,000, coins 1,000: the account owes 13,500, is not due, and takes nothing but a step | stage 1: **where the coins do not pay the leg named and no token is held, every leg leaves** (as built). What is owed beside options: with stage 2 |
| E4 (medium) | the model is of one underlying, has no fee, nothing owed and no tokens, and pays a gain in | **the model first**: groups of several markets, the fee, what is owed, a gain paid out; E1's account as a test |
| E5 (medium) | in a step of every leg a written option pays the larger of its premium and what it lost, a leg at a time: no sum of the row gives it | a number more now, **PO** (what the written options that leave pay, each the larger of the two): the row is **232 bytes** |
| E6 (medium, taken) | where a leg leaves from between two of one market, what stays is counted as one group across the gap only if they are then next to one another in the rows: else more is asked, never less | said here |
| E7 (medium, of use) | **a hedged pair after a move is safe and cannot be undone a leg at a time**: a take, either leg's leaving and a mark fail (what stays alone would be under); its exits are every leg leaving, which ends the legs of its other markets too, or coins added | not built and not kept room for (a leaving of a group needs a third chain and notes of the shard's side). **Said to the owner of the design** |
| E8 (medium) | the fee and the band are of the gross worth: a long of 100,000 and a short of 90,000 barely due leave whole, for a fee of 950 on a net of 10,000 | the fee no more than what the coins hold after the loss and what the legs that stay lost (review 47's rule, kept); its base with the rates, in the last phase |
| E9 (low) | the row needs twice the tolerance for a band, and the upper rate less the lower is not that where the maintenance is below the tolerance | **the head says the rate, the tolerance and T**; each end holds them: the maintenance and the tolerance of the terms, the tolerance, twice the account's lateness and dispute period |
| E10 to E12, E14 | an account is asked less than today by at most a sompi a leg; the opening of the leg before is asked, not judged (a false leg shown would make a right row wrong); `KpfTChange`, `KpfTEnterS` and `KpfTStepPrice` hold the shape of what is said; a mark reads the sums of all | while building |
| E13 (stage 2) | a step with every held option on its line of nothing is safe for the shard and takes a hedge apart (a put held against a long, named alone, "lacks less" so and 5,120 more by the lines that ask least) | a held option is named alone only where every leg that stays is a held option; the model first |

**The model of stage 1** (E4; `tests/research_account_rows.rs`, four tests, no script): futures legs of three
markets, the row's sums as the review's list has them, the ends' tests, a step with its fee, a gain paid out to
the user's key.

- **What is asked.** On 200,000 accounts of one to six legs, R is never below what the legs lose where each
  market moves by the rate (the legs of a market counted together wherever they stand); exactly that on the
  121,633 whose markets stand together, more on 47,797 of the others. Against today's rate on the legs' worth:
  an account whose legs are each of another market is asked that, less a sompi a leg at the most; no account is
  asked more than that and a step of the price on each leg's units (**the ends are the rate on the price, rounded
  up to a step of the price**: at a price of 100 a rate of 5.5 % is asked as 6 %; a market's price has five
  digits or more, or its users are asked more). What stays when a leg leaves is asked what it would be asked
  alone, and more on 7,813 leavings from between two groups of one market (E6).
- **A hedge** (a long and a short of one market, next to one another): asked nothing where today's rate asks
  1,100,000 of 20,000,000; after a move of 4 % a take asks what the short lost alone; it is due at no price;
  with no coins neither leg leaves by its user's statement (E7). The same two with another market's leg between
  them are asked as two.
- **E1 and E3, in their numbers**: by "lacks less" alone the long that gained 3,000 is named, 2,000 leave for
  the user's key and the short's loss of 8,000 meets coins of 5,000; as built the statement is wrong, the short
  named takes every leg, and the shard is paid. The short that lost 14,000 beside a long that gained 20,000,
  coins 1,000: as built every leg leaves and the shard pays what they gained together less the band; nothing is
  owed.
- **In time.** 30,000 accounts entered at the level of a take and something more, the prices moving the lower
  rate at the most between visits, whoever watches naming a leg at random: 35,689 steps (31,910 of a leg
  alone), 9,799 accounts ended, **the shard unpaid nothing**. By "lacks less" alone (any leg named, what the
  coins do not pay owed): unpaid on 162 accounts. An account at the level of a take is due at no price.

**Stage 1 as it is built** (the engine; not yet read by a reviewer). A covered account's statement says its
head and rows of 232 bytes; `KpfTCRow` computes them (`crow.rs` is its model); the ends test by R; a step is due
by one price a group. Every test of the workspace passes (740), the reviewers' randomized walks of the model
against the engine among them, rewritten where the rule changed (below).

- **The scripts** (`s404_account_rows_sil.py`, after the regeneration's last):

  | Script | Bytes before | Now | What changed |
  |---|---|---|---|
  | `KpfTCRow` | 2,547 | 4,193 | the leg before shown and held to the row before's chain; the two ends; a step's price; the group closed; fourteen numbers more, compared as bytes |
  | `KpfTCEnd` | 3,832 | 4,384 | the head held; R twice; a step due by what it would leave; a leg that lost named, by what the account lacks |
  | `KpfTCEnter` | 1,721 | 1,994 | the head and the place held; R |
  | `KpfTCSplit` | 2,388 | 2,614 | the head; R of what stays; what the leg that moves can lose alone |
  | `KpfTCMark` | 2,298 | 2,546 | the head; R |
  | `KpfTCTok` | 3,492 | 3,757 | the head; R of what stays |
  | `KpfTStepPrice`, `KpfTChange`, `KpfTEnterS` | | +16, +1, +2 | where the rows begin, and their length |

  No script met the stack's limit. The scripts that grew are answers of a dispute: none is spent where a
  statement is stated or applied.
- **What a transaction weighs** (an account of two legs): a take stated 3,631 bytes (3,374 before: 32 for the
  head and 112 a row); applied 2,608, as before; its end answered 6,293 (5,485); a row answered 6,096. An entry
  stated 5,754 (5,599).
- **The model** (`covered.rs`): `CSaid` and `CEnterSaid` say the head (and an entry the place); the traces are
  `crow::ctrace`; `c_step_prices` finds the prices that show a step due if any do (a group's sum is a line in its
  price: the end of the lower rate at which it gains least). A row is judged with the leg before, held to the
  row before's chain opened from the chain two rows before as the rows say it (`None` where it does not open:
  the script fails there, and takes neither verdict).
- **Two legs of one market next to one another are stated at one price**: the model writes no trace of two, and
  a statement that says two has its second row wrong. So a leg that enters next to a leg of its market is
  stated with that leg at its own entry.
- **A token leg** stays in the model's trace where a statement that is not of its kinds names it (its row is
  then wrong and the end sees what it counts), as before.
- **The tests that changed, and why.** F73 (review 47): three legs that lost 45 each on coins of 138 left in one
  step; they now leave in three, a leg at a time, the shard paid the 135 whole, the stepper the 2.8 the coins
  have over at the first, no fee after it, the account going on with a note's least: what F73 found (a fee
  before a loss) is closed the same. A step whose coins pay the leg named and not its fee is no longer of every
  leg: the leg leaves alone, for no fee. The walk of "what a shard is good for" (review 47) states one price a
  market: measured at the prices of two groups of one market that a leaving joins, an account seemed to fall;
  at one price a market no statement of 21,513 makes it fall. The written-out rules of the split's, the mark's
  and the take's walks ask R where they asked the rate on a worth. Two counts of statements a walk needs are
  lower (fewer traces are written of random prices).
- **New at the engine** (`thin_draw.rs`,
  `legs_of_one_market_next_to_one_another_are_asked_what_they_can_lose_together`): a hedge at 11 keeps the 10
  its short lost and a note's least of 100 (12.1 less than by the rate on its worth), its two rows answered
  right; the second row shown another leg before, or none, is taken by neither verdict; two prices in one group:
  the second row wrong; a head of another T, or of a rate a point less with the rows that makes: the end wrong;
  the two legs with another market's between them asked as two; a leg that lost refused a sompi under the level
  of an entry of what stays, though the coins pay it; a hedge with a KAS not due, a long alone due by the price
  a lower rate below, which its row says; a step's price beyond the lower rate, or one that does not show it
  due: wrong.
- **Not built here**: the place of an entry (the leg that enters is the last, and its head says so: E2); the
  option leg (stage 2); a leaving of a group (E7).

**Review T13** (`docs/audit/REVIEW-T13-rows.md`; the engine; `tests/auditT13_rows.rs`, fifteen tests). **The
scripts are the model's and the rule's to the sompi; the rule had one hole, of this stage.** Attacked and held:
each of a row's 21 numbers a sompi off in every kind (1,806 rows, and 134 of an entry: wrong, and not right); a
row after a bent row before, the group closing at the ends of 64 bits (2,856 rows: the engine takes the model's
verdict on every one); the leg before (22 answers: it cannot be played, no right statement has a row nobody can
answer, no wrong one escapes); the ends a sompi either side and the head's three numbers (43); on the model,
12,000 accounts through 13,224 takes of the most and 9,535 steps at the markets' prices: the shard unpaid
nothing, no account stuck, none due after its own statement.

| Finding | What it is | What is done |
|---|---|---|
| W1 (high) | **a due account that holds a token leg and a partly hedged group took no step**: a leg that does not make the account lack less sent the step to every leg, and every leg is refused beside a token leg. A long of 10 and a short of 9 of one market with a tenth of a token: due at 9, short 0.7 KAS at 8, and nobody could end it. Any dust of tokens did it | **closed**: an account that holds tokens is not taken whole, so the leg named that lost leaves alone, the account owing what the coins do not pay (as before this stage: 16.18); "lacks less" decides between the leg and every leg only where no token is held |
| W2 (low) | the fee was counted in what the account lacks after: a sompi more of coins turned a step of one leg (fee 2.189) into a step of every leg (fee 4.589) | **closed**: what the account lacks after is counted before the fee |
| W3 (medium) | **whoever steps chooses the leg, and is paid most for the worst choice**: the fee of a leg alone is capped by what the legs that stay lost each alone, a hedge's losing leg whole, so the step that cures pays nothing; a leg named that the coins do not pay (a hedge's loser) takes every leg, for the coins as fee, and what the legs gained together falls under a band of the gross worth. In its numbers: a user loses 4.7 KAS by the stater's choice. In the walks, where both were right: 205.9 KAS of fees a leg alone against 891.2 for every leg. The choice was there before; netting widens it | **closed, below** (a step by what is asked): the model first, then the script. E8 with it, no longer left for the last phase |
| W4 (low; the model; not of this stage) | with every price stated the tolerance off against the shard and moves of the whole lower rate, the shard is unpaid on 45 of 4,000 accounts, 0.41 KAS the most; at 3.5 % none. "An account at the level of a take is due at no price" holds at one price | said here: the upper rate holds the lower and twice the tolerance, to the rounding |
| W5, W6, W9 (low) | the model and the script part where no coin moves: a step's price at the least number (the script subtracts before it compares); row 1 shown a leg before (the script reads none); a token leg worth 9 x 10^18 (the script counted its worth at the upper end before it saw a token leg) | **closed**: the model takes the script's part in the first two; the script counts a token leg's worth at its row's price alone |
| W7 (low) | the bands of a row (of all, of those that leave) are computed and held by the row and read by no end: the ends count a band from the worth, as before; the scripts' head comments were of the rule before | the comments are of the rule; the bands' use with W3 |
| W8 (low) | review 47's walk of "good for" took the most by the rate on the worth, below what the rule gives on 1,335 accounts of 5,000 | **closed**: it takes the most by R |

**After review T13** (`s419_t13_fixes.py`; every test passes, 755; the reviewer's tests of W1, W2, W5, W6 and W9
assert the findings closed). `KpfTCEnd` is 4,374 bytes, `KpfTCRow` 4,240. As a step now is: the leg named lost;
**an account that holds tokens is not taken whole**, so the leg leaves alone and the account owes what the coins
do not pay; with no token held it leaves alone where the account lacks less without it (what it lacks after,
counted before the fee) and the coins pay it, and else every leg leaves. W1's account, at the engine: the long
named leaves alone, the coins paying what they hold above a note's least and the account owing 9.5; then the
short (it gained, beside no leg that lost) is named, what it gained set against what is owed; then the tokens
of an account that owes are sold.

**Found while closing them (X1; not of this stage; closed, below).** An account that holds a note's least and no more
(what a step that made it owe leaves) can have a statement stated and applied, but **the answer of a disputed
point of it is a transaction no block takes**: the statement's bond is then the least bond (0.2 KAS), the script
pays a quarter of a bond lost in an output of its own where a quarter is 0.05 KAS or more, and beside the
account's own output of 0.2 KAS (which weighs 200,000 of a block's 500,000 by itself) an output of 0.05 KAS
is too small by a thousandth (0.05094867 at the least here). So on such an account whoever disputes a step
wins by nobody being able to answer, and the stater loses its bond. The threshold of review 50 (F77) was set
for accounts whose own output is larger. **Closed** (`s437_x1_quarter.py`): a quarter of a bond lost is paid in an
output of its own only where it is 0.1 KAS or more (the fifteen answering scripts, a byte more each;
`ANSWER_PAY_MIN`); below that the claimer has half in one output and whoever answered is not paid, as F77 has it
for the smaller bonds. Two outputs of 0.1 KAS fit beside an output of 0.2. Shown: the end of a step on W1's
account once it holds a note's least is answered right at the engine, and not wrong. (So the answer of a point
of a statement whose bond is under 0.4 KAS pays its sender nothing: the least bond is 0.2.)

**A step by what is asked** (review T13, W3; `s432_step_net.py`; the model first:
`research_account_rows.rs`, `a_step_by_what_is_asked_costs_a_user_no_more_than_its_risk_whoever_chooses_the_leg`).
Whoever steps chooses the leg; so no choice may pay it more than the risk it takes away. Three changes, all in
`KpfTCEnd` (4,471 bytes) and its model, no number more in a row:

- **The fee and the band of every leg leaving are of what is asked**: R of all the legs (the net, the last
  group closed) by the fee's rate over the rate, rounded down; and by twice the tolerance over the rate, rounded
  up. For an account of one leg both are what they were (R is then the rate on its worth). The band is so
  what a price stated the tolerance off can move of what the legs gained together, and no more: the legs of a
  group say one price. A user's own leaving of every leg has the same band; the test of a step that is due
  counts it.
- **What the legs that stay lost is what they lost together**, where a step reads it: in the cap of the fee of
  a leg that leaves alone (the coins it leaves pay what the legs that stay lost at the prices stated), and
  where a leg that gained is named (only where the legs that stay have not lost together: E1 holds; a hedge's
  losing leg no longer counts whole against its other leg).
- **A leg that gained, named, leaves alone where the account lacks less after it** (what it gained, less its
  band, leaves for its user), and else every leg leaves; an account that holds tokens is not taken whole, as
  before.

In W3's numbers: the step that cures now pays the fee of its leg (0.048 KAS) and leaves the account as it was
but for that leg; the step that names the hedge's short takes every leg for 0.563 of fee and 1.126 of band (of
6.193 asked), the user paid its coins and the 2.6 its legs gained less those: by the stater's choice it has 1.7
KAS less, where it had 4.7 less and nothing left. With a tenth of a KAS less of coins, the long that has to
leave is named and leaves alone (as built first it could not be named, and every leg left). **In the model**,
30,000 accounts after a move of the lower rate, whoever steps taking the step that pays it most, or least: the
shard is unpaid nothing by either rule and either stater. What the choice moves of what users have: as the
legs fall, 445 million in all by the gross worth and 317 by what is asked (the most on one account 1.32
million and 0.83); **of accounts that hold a hedge beside their legs, 2,090 million and 458**, the fees of the
stater that takes most 3,004 million and 1,500. The choice is not gone (a leg the coins do not pay still takes
every leg, with no token held): what it pays is now of the risk taken away.

**After review T14** (O1, O2, below 17.8i; `s440_t14_o1_o2.py`). Two of the three changes above were wrong as
built, and are now: **the band of every leg leaving is the entries' part, the tolerance of what the legs are
worth (gross: each leg entered at a price of its own statement), and the leaving's part, R by the tolerance
over the rate (net: the legs of a group leave at one price)**; one leg pays what it paid, a hedge half of the
gross band. And **a leg that gained is named only where no leg that stays has lost, each alone** (as before
the change: together, a gain of one market stood for a loss of another). The fee of every leg on R stands, and
the cap of a leg's fee by what the legs that stay lost together (the shard can be unpaid by it, by no more
than one fee, where the prices then part). In W3's numbers now: the cure pays 0.048; every leg, 0.563 of fee,
and the 2.6 the legs gained are under a band of 12.126 again: the stater's choice costs the user 3.2 KAS where
it cost 4.7. In the model, of accounts that hold a hedge: 2,090 million as first built, 512 now; the fees of
the stater that takes most 3,004 million and 1,500; the shard unpaid nothing.

**What is not read** (W7): a row's band of all the legs (BA) is read by no end (the band of every leg is of R;
of a leg alone, of its worth, counted by the end as before). It stays for the option leg, whose band is not a
rate on a worth (17.5); if stage 2 does not read it, it leaves the row in the last phase.

### 17.8i The option leg in an account, how it is built (17.8h's stage 2; on paper, for review)

17.2 to 17.5 say what an option leg is and what holds; 17.8h has the row and the ends for futures and tokens,
with the options' numbers at nothing. This says what an option leg adds to a row, what each end does with it,
and what changes of 17.8h now that a step is by what is asked (review T13, W3). Nothing of it is built. An
account with an option leg is made in the tests as an object is: the entry with its premium, the shard's dated
rows, and the day (17.8g, B and C) are 17.8's seventh step.

**The leg.** Where a futures leg says its way: 3 a call the user holds, 4 a call it wrote, 5 a put it holds, 6
a put it wrote (as a leg of an account no shard covers has them; the strike is not there). Its 32 bytes: the
time it entered, its day, its strike, its premium a unit. Its entry: the market's price where it entered. A
strike within the market's limit; a day after the time it entered; a premium not below nothing.

**What it is worth exercised**, v(p), by its units: a call held, the larger of nothing and the worth at p less
the worth at the strike; a put held, the other way; one the user wrote, that below nothing.

**What it adds to a row** (L and U, q, the head: 17.8h):

| Number | A futures leg | An option leg |
|---|---|---|
| G; GL where it leaves | its worth at a less at its entry, by its way | v(a) less v(entry) |
| W; WS where it stays | its worth at a | its underlying's worth at a (the base of the fee of a leg named, as a futures leg's) |
| LS where it stays | what it lost | the same |
| M | what it gains from a to q | v(q) less v(a); **within T of its day, no more than nothing** |
| the two sums; RL alone | what it gains from a to L and to U | v(L) and v(U) less v(a). **A held option whose strike is between L and U: along the line its bit of the head says** (worth nothing wherever the price goes: v(a) lost at both ends; or exercised wherever: what it is exercised at each end, below nothing as well, less v(a)). **Within T of its day: each no more than nothing.** In a step: the line of nothing, whatever the head says (below) |
| the band where it leaves alone (BL) | twice the tolerance of its worth | what it loses by a move of twice the tolerance, the worse way (17.5) |
| PA; PS where it stays | nothing | its premium by its units |
| PO where it leaves | nothing | held: its premium. Written: its premium less what it lost, or nothing (17.3: the shard is paid the larger of the premium and what it lost) |
| NS where it stays | one | **nothing for a held option**, one for a written |

A row of a leg whose day is at or before the statement's time is **wrong**: an ended leg leaves by the day's
transaction (17.8g, C) before anything is stated on its account. T and the statement's time: the head's, and
the statement's own.

**Three numbers change in the row of 17.8h, and one is new: 256 bytes.**

- **The band of every leg leaving is what the legs can lose by a move of twice the tolerance**: the two sums and
  the closed groups again, at that rate (three numbers, where the row has the band of all the legs, which no
  end reads). For futures legs it is what is built (R by twice the tolerance over the rate), to the rounding.
  For options R by a ratio is not it: a call held a hundredth out of the money is asked nothing but its
  premium, and a price stated a hundredth high shows it a gain.
- **NS**: how many of the legs that stay are not held options (a step's rule, below).

**The ends** (R', m, tk, owes as in 17.8h):

| Statement | With an option leg |
|---|---|
| an entry (10), a take (7), a mark (13), tokens leaving (14) | the premiums beside R: PA where all stay, PS of what stays |
| a leaving (8) | **an option is named by none**: its row is wrong where it leaves, alone or with every leg (nobody exercises before the day: 17.5). Of a futures leg beside options: the premiums of what stays beside R |
| a mark (13) | names no option: its row is wrong |
| a split (12) | moves an option with its premium (how one is sold: 17.4): what moves at RL, its premium and what it lost; what stays at RS', PS and LS |
| a step (9) is due | the coins and what a step would leave of what the legs gained **below the premiums** (PA) |
| a step, a futures leg or a written option named | as 17.8h, with every held option between the ends on its line of nothing; where it leaves alone the coins pay what it lost and its PO, and then the fee |
| a step, a held option named | **alone only where every leg that stays is a held option** (NS of the last row is nothing: review T12, E13); else every leg |
| a step, every leg | the coins pay what the legs lost together, then PO as far as they go, then the fee; what they gained together is asked less the band above. What the coins do not pay of a premium is not owed (a premium is the user's own coins, or the owner's: 17.3) |
| the fee of every leg | what is asked, **R and the premiums**, by the fee's rate over the rate (an account of held options is asked its premiums alone) |

**To be shown in the model first** (`research_options.rs`; the options' model has one underlying, no fee and
its own step): a step with every held option on its line of nothing and E13's rule, against the rule that
takes the lines that ask least (how many steps part, and that the shard is unpaid by none; N71 as it was); the
band at twice the tolerance (a held option near its strike stated the tolerance high gains nothing by it); a
step by what is asked with premiums (the walk of 17.8, 1, with W3's fee).

**To be said by the review:** whether an ended leg's row being wrong can freeze an account against its user or
its stepper (who sends the day's transaction, and what if no price of the day is written yet); the day and T
against the statement's time where a statement stands for a dispute period across the day; whether "an option
is named by no leaving" leaves a user with a futures leg and an option no way to close the option but a split
or a step; a written option that gained (it ended out of the money before its day?) named in a step; the
premium of a written option in a split (the owner's coins move to another account of the same user: 17.3's
"never margin for its other legs"); whether PO as one sum is enough where a step of every leg pays losses
first and premiums after, and where a leg leaves alone; the band of a held option that leaves alone against
17.5's (the entry's part); whether the fee on R and the premiums pays whoever steps an account of held options
too little or too much; what E13's rule does to a put held against a long (the long named first); and the
cost: 256 bytes a row (24 more than is built), and what `KpfTCRow` weighs with two kinds of leg more.

**Review T14** (`docs/audit/REVIEW-T14-option-leg.md`; on paper, nothing run; it holds the leg's bytes, the row
at 256 bytes with its places, each number's rule for an option leg, each end as a line, and thirteen tests of
the model to write before any script).

*Of the step by what is asked, as it was built (17.8h):* **change**; both closed.

| Finding | What it is | What is done |
|---|---|---|
| O1 (high) | the band of every leg on R covers the price the legs leave at and not the prices they entered at (review 43's F65). The committee at 10; a long of 1,000 entered at 9.95 and a short next to it at 10.05, each statement right; every leg leaves: gained 100, R nothing, band nothing, **100 paid from prices alone**; a mirrored account takes the risk between the two entries away | **closed**: the band is the entries' part, the tolerance of the gross worth, and the leaving's, R by the tolerance over the rate. Shown at the engine (the hedge is paid nothing; a sompi more gained is asked) |
| O2 (medium; new with that change) | "what the legs that stay lost together" nets across markets: a long of X that gained 8,000 and a short of Y that lost 8,000 stay; a long of Z that gained 3,000 is named, 2,000 leave for the user's key; X falls 3 %: the shard unpaid 3,000. A user states the step itself and is paid what a take refuses | **closed**: the guard is "no leg that stays has lost", each alone. Exact would be "a group at a time" (two numbers a row); the whole class closes where a gain in a step is credited and not paid (17.8g, C) |
| O7 (low) | accounts nobody has a reason to step (a hedge under water: no fee; a hedged account that owes) | a least fee in the terms: the last phase |

*Of the option leg (17.8i):* **change first, then build.** The row's shape is right and 256 bytes hold it.

| Finding | What it is | Decided |
|---|---|---|
| O3 (high) | a leg that gained leaves alone beside a held option whose premium stands on that gain: a long from 100 with coins of 61 and a call held for a premium of 100; at 105 the long is named, 39.5 leave for the user's key, and the call's premium is then 44.25 short and not owed | a leg that gained leaves alone only where the coins hold the premiums that stay (m less what is owed, less the fee, at PS); else every leg |
| O4 (medium) | the fee of a leg alone was capped before the premiums that stay | the cap is after them |
| O5 (medium) | where every leg leaves, PO was paid from the coins alone | it is set against what the legs gained first (as the model has it); PO as one number is enough |
| O6 (medium) | the band was not 17.5's: no entry's part for an option (a call struck and entered at 100, the committee at 100.5, named at 99: band nothing, 5 hidden), and twice the tolerance at the leaving where 17.5 has once | one rule with O1: each leg's entry part, and what the legs lose by a move of the tolerance (the two sums and the closed groups at that rate) |
| O8 (low) | the fee of an option named alone was of its underlying's worth | of what it can lose alone and its premium, by the fee's rate over the rate; an option adds nothing to the legs' worth |
| O9 (medium; the seventh step's) | "an ended leg's row is wrong" freezes a due account against whoever would step it while no price of the day is written | built as written here; decided before the seventh step (a step passes such a leg by a row that adds its premium alone) |
| O10, O11 (low) | a token leg adds nothing to NS; "between the ends" is strict; the lines are of nothing in a step whatever the head says | in the text of the second writing |

The second writing of 17.8i follows the review's list; the model's tests come before any script.

**The second writing of 17.8i** (what is built from: the review's list, `REVIEW-T14-option-leg.md`, "Check list to
code from", with these words for it). The first writing above stands but for:

- **The row: 256 bytes**, the built row's twenty-one numbers in their places and three more: the two sums of
  the open group **at the tolerance** (what the legs gain to the price less and plus the tolerance), and NS.
  **The band of all the legs** (the row's ninth number, read by no end until now) is each leg's **entry's
  part** and, of the groups that are closed, what they lose by a move of the tolerance. The band of every leg
  leaving is that and the open group's. For futures legs it is what is built since review T14 (the tolerance
  of the worth, and R by the tolerance over the rate), to the rounding.
- **An entry's part**: of a futures leg, the tolerance of its worth; of an option, what it is worth exercised
  changes by where its entry is moved the tolerance (nothing for one entered further from its strike). **The
  band of a leg that leaves alone**: its entry's part and what it loses by a move of the tolerance (a futures
  leg: twice the tolerance of its worth, as built).
- **An option adds nothing to the legs' worth.** The fee of an option named alone is of what it can lose
  alone and its premium, by the fee's rate over the rate.
- **A step, in the order loss, premium, fee.** PO is set against what the legs that leave gained (less their
  band) before the coins pay it. The fee of a leg alone is capped after the premiums that stay. A leg that
  gained leaves alone only where no leg that stays has lost, each alone, **and the coins hold the premiums
  that stay** (O3); else every leg.
- **The note to the shard** is as it is: what is asked, below nothing by what the coins pay, of loss and of
  premium together (where a leg gained more than its premium nothing is paid, and where less nothing is
  asked: one number says both). What the shard's books do with a note that names an option leg is the
  seventh step's.
- A token leg adds nothing to NS. A strike is between the ends where it is above the lower and below the
  upper. In a step every held option between the ends is on its line of nothing, by the row.
- An ended leg's row is wrong, as written; what a step does beside one while no price of its day is written
  is decided before the seventh step (O9).

**The order.** (1) The model: the review's tests 1 to 8 and 12 (the row's sums of the four ways against a
direct count; a step's payments against `stepped_owing`; the band at the tolerance; O3's account; the walk
with premiums and the fee). (2) `crow.rs` and `KpfTCRow` at 256 bytes, futures and tokens as they are (every
test of today passing, the band of every leg read from the row). (3) The option leg in the row, and the ends
with premiums; accounts with option legs made in the tests as objects. (4) The engine's review.

**The option leg as it is built** (the engine; every test passes, 759; not yet read by a reviewer). In the order
of the second writing:

- **The row at 256 bytes** (`s446_row_256.py`), futures and tokens as they were: the band of all the legs is each
  leg's entry's part and what the closed groups lose by a move of the tolerance; the open group's two sums at
  the tolerance; NS. The ends read the band of every leg, and of a leg alone, from the row.
- **The option leg in the row** (`crow.rs` `option_of`, `crow_after`; `KpfTCRow`, `s454_row_option_sil.py`): the
  four ways, the two sums exact or along a line, within T of the day no more than nothing, the entry's part,
  the band's ends, the premiums. A statement's row is of its time and of the leg's place (the head's bit).
  Wrong before anything: a strike out of range, a premium below nothing, an ended leg, a leaving or a mark
  that names it, the leg an entry says, a head whose lines are below nothing.
- **The ends** (`covered.rs` `c_end_as`; `KpfTCEnd` and the four others, `s457_ends_premium_sil.py`): the premiums
  beside what is asked; a step due below the premiums; the leg named in the order loss, premium, fee; the fee
  of an option named alone; E13 and O3 as decided.
- **Two things the list did not have, found while building.** (1) An end is not shown the leg named, and a
  step's rule reads whether it is a held option: **NS counts, by 256, the held options that leave**, beside the
  legs that stay that are not held options. (2) **PO is of every option leg**, not of those that leave: a step
  that names one leg may take every leg (E13, O3, or where the account does not lack less), and its rows were
  written with one leg leaving; the premium of the others was then paid by nobody. Of the leg named alone the
  end has what it pays from the row (its premium is PA less PS; a holder's whole, a writer's less what it
  lost). Found at the engine by O3's account: the shard had 3.95 of its 10.
- **How a statement is stated** (`c_state`): at its time; with the lines of its held options that ask its user
  least (`c_head_least`, every choice tried: sixty-four at the most); a step's fee is the rule's
  (`c_end_as`, not strict, then strict).

| Script | Bytes before the option leg | Now |
|---|---|---|
| `KpfTCRow` | 4,873 (232 bytes a row: 4,241) | 7,508 |
| `KpfTCEnd` | 4,488 | 5,459 |
| `KpfTCEnter`, `KpfTCSplit`, `KpfTCMark`, `KpfTCTok` | 1,993, 2,613, 2,497, 3,756 | 2,003, 2,643, 2,507, 3,767 |

A held call's row answered: 9,189 bytes; an end answered beside it: 7,043; a take stated beside a held call
3,423 (a leg). For the last phase: the answer of a row in two scripts, by the kind of its leg (a futures leg's
answer carries the option leg's 2,600 bytes).

**Shown** (`crow_model.rs`, the row's model: 40,000 accounts of futures and options of the four ways, legs within
T of their day among them): what is asked is no less than what the legs lose at any price a move of the rate
reaches (the ends, every strike between them and a sompi either side), and exactly that on the 13,541 with no
held option between the ends and none near its day; the band of every leg is no less than what prices stated
the tolerance off, at the entries and at the leaving, could show the legs to have gained. **At the engine**
(`thin_draw.rs`, `an_option_leg_is_asked_its_premium_and_what_it_can_lose_with_its_group`): a call held alone is
asked its premium at any price; each of the seven numbers an option leg writes, a sompi off, is answered wrong
and not right; no leaving and no mark names an option; an ended leg's account takes no statement; a covered
call is asked what the long loses below and the premium it was paid (7.5 where the rate on the worth asked 13);
a long leaves in a step from beside a held call whose premium the coins hold, and the call goes on (N71); the
held call named beside the long takes every leg, in the order loss, premium, fee; O3's account leaves whole
and the shard has its 10; a put held against a long is asked nothing but its premium by its user's line, and
the long whole within T of the day.

**Not done**: the review's tests 6 and 7 of the model (the walk of 17.8, 1 with premiums and a fee; the
payments against `stepped_owing`, leg by leg): the options' model has one underlying and its own step, and the
engine's review is asked for the walks instead. The tool states no account with an option leg (none enters).

**Review T15** (the engine; `docs/audit/REVIEW-T15-options.md`, `tests/auditT15_options.rs`, fifteen tests). The
scripts are the model to the sompi for every leg of the form (17,619 rows of every kind, each of the 24 numbers
a sompi off, the lines set and cleared, the ends a sompi either side); with no token leg the rule left no loss
unpaid and no account stuck on 12,000 accounts. **Both high findings are where an option leg meets a token leg**,
which no test of mine had tried.

| Finding | What | Closed by (`s463_t15_fixes.py`) |
|---|---|---|
| Y1 (high) | A take tested the coins and what the tokens count against what is asked and the premiums: tokens stood for premiums. A step pays a premium from coins alone and the rest was not owed. A written call's 2 KAS of the owner's premium beside a quarter of a token: taken out, the call then stepped out with the token stated the tolerance low; the shard paid nothing and owed nothing | **Tokens stand for no premium.** Every end that counts what the token legs count (a take, a leg leaving, a split, a mark, tokens leaving, an entry) holds the premiums to the coins alone as well. **Beside tokens a premium the coins do not pay is owed** (16.18), as a loss is |
| Y2 (high) | A held option named beside a leg that is not one takes every leg (E13); an account that holds tokens is not taken whole (W1); the other leg is not named while a leg that stays has lost (O2). A put held in the money that lost, a hundredth of a long, a hundredth of a token: no step was right, the legs 99.985 down on coins of 57. W1 again | **Beside tokens a held option named leaves alone**, owing what the coins do not pay of its loss and its premium. E13 stands where no token is held |
| Y3 (medium) | O3 guards the premiums that stay. An option leaving alone paid of its own premium what the coins held, the rest not owed, while the legs that stay kept what they gained: whoever states the step chose 2 KAS of the shard's | **An option leaves alone only where the coins pay all of its premium that is not set against what it gained**; else every leg (where what all gained beyond the band is set against the premiums first). Beside tokens it leaves alone and the rest is owed |
| Y4 (low) | On legs that fail the size's rule the script counts worths the option's rule never reads, and gives no verdict where the model gives one. Nothing bounded a premium | A premium a unit is within the market's limit (`c_check`, the row's model and `KpfTCRow`: wrong before anything). The rest stands: no account of the form has such a leg, and the test shows it as it is |
| Y5 (low; a token leg) | A token leg named by kinds 14 and 15 with a discount out of range: right by the model, wrong at the engine | The model holds a token leg to its discount where it is named as where it stays |
| Y6 (low; the model's `c_state`, no script) | A step's prices read an option as a short futures leg and tried the two ends only (1,920 of 5,783 right steps not stated); the lines were chosen by a take's test whatever the kind; a leaving, a split and a step that name a token leg were stated, with a wrong row | `c_step_prices_at`: of the ends, the price and every strike between with a sompi either side, the one at which the group gains least by the row's own sum. `c_head_for`: the lines that ask least of what the statement's own end tests. No statement names a leg of the other sort |

| Script | Before | Now |
|---|---|---|
| `KpfTCEnd` | 5,459 | 5,871 |
| `KpfTCRow` | 7,508 | 7,520 |
| `KpfTCEnter`, `KpfTCSplit`, `KpfTCMark`, `KpfTCTok` | 2,003, 2,643, 2,507, 3,767 | 2,069, 2,716, 2,573, 3,847 |

The review's seven tests of the findings hold the closed rule now (Y4's as it stands), and its own writing of
the step's rule was changed by the three lines of Y2 and Y3. Its walks after the fixes: with no token leg, 12,000 accounts (9,939 with options), 6,524 steps of a leg or of every leg: stuck 0, a loss unpaid 0, a premium short 0, due after its user's own statement 0, right steps the model does not state 0 of 11,223. With a
token leg in every account: 6,000 accounts, 1,944 steps of a leg: stuck 0 (before: 142 visits), a premium short 0 (before: 331 steps, 881.6 KAS), a loss unpaid 0. The whole workspace: 774 tests, none failed.

**What the review held and a user should be told** (N68 as written): an account that stands by a hedge is **due at
once, with no price moved, the block its option comes within T of its day**; the shard is unpaid nothing; in the
walk 95 such steps cost their users 100 KAS of fees. The tool must say the day a hedge stops counting.

**For the seventh step, from the review**: `KpfTCStep` (a call's step) asks a leg's way to be 1 or 2, so an account
with an option leg takes no step under a call: not live while no option enters, and decided with the entry. Not
reached: prices stated the tolerance off with options (W4), a hand-over or a join of an account with option
legs, the end of a sale (15) at the engine, block weights, and O9, which stands as review T14 left it.

### 17.8j The seventh step, whole: the dated rows, an option's entry, its leaving, its day, a call (on paper, for review; nothing of it built)

**Where it starts.** An account knows an option leg whole (17.8i, reviews T14 and T15): its row, the ends, a
step. Nothing lets one in: `KpfTCRow` says the leg an entry says is wrong where it is an option; the shard's
books take futures legs only (`Shard::book_of`); `KpfTCStep` takes the ways 1 and 2. The shard's books are four
futures rows written whole; the tree, its two scripts and the answer of a trace's row by paths are built,
reviewed (T10) and parked (`run/parked/tree-all-rows/`). The committee's note of a day is built (`KpfTDay`).
What 17.6, 17.7 and 17.8g say stands; this section says what they left open (17.8e: where a dated row's scale
and limit come from, how a trace goes over four rows and then a list; 17.8g: B and C as transactions; reviews
T14 and T15: O9, a call's step), and the order it is built in. Each numbered point is decided here unless it
says "open".

**1. The books' hash** is of four things: the sum of the gross of every row, the hash of the four futures rows,
**the root of the tree of dated rows**, the hash of the claims' numbers. A shard is opened with the root of a
tree of the head leaf alone (a constant). Every script that shows the four rows is shown the root beside them,
33 bytes; only a script that touches a dated row opens it.

**2. A market's futures row is its head.** One scale and one limit a market were held, in the tree of every
row, by the market's futures row being its first leaf. With the futures rows out of the tree: **the ninth
field of a futures row (what its legs were entered at: nothing, in a futures row, today) is the count of the
market's dated rows.** A dated row is put in only where its market has a row among the four (an empty place
takes the market, with no unit, where it has none), takes that row's scale and limit, and the count rises by
one; taken out, the count falls. A futures row with a count is not the empty row: no other market takes its
place (review T9's Q3 is this rule already). The tree then has no futures key and no rule of a market's first
row: the head leaf, and keys in order. Cost: dated rows of four markets a shard at the most, as futures.

**3. A dated row** is the parked leaf: the key (the market, the day, what it is of, the strike), the scale and
the limit, the net (the clearing side's units: above nothing where users wrote more than they hold), the gross,
what its legs were entered at, the hash of the key after. "Of": 1 calls, 2 puts, **3 the price of the day**
(17.8g, B), with a strike of nothing: it is the last key of its market and day, so the leaf before it shows
whether a row of that day still stands. A day's leaf holds the price where a row holds what was entered, and 1
for "none" where a row holds its net; its gross is nothing.

**4. A row put in and taken out** (`KpfTRowIn`, `KpfTRowOut`; the parked scripts, with the four rows shown).
- In: the owner signs; the shard not called; the key well made (a strike within the limit, a day more than T
  after the time the transaction says, which is not ahead of the chain; "of" 1 or 2); the leaf before below it
  and naming a key above it or nothing; an empty place; the market's futures row as point 2 says.
- Out: a row of no unit and nothing entered; by the owner at any time, by anyone once its day has passed. A
  day's leaf: by anyone, where the leaf before it is not of its market and day.
- No entry puts a row in (as stage 2 had it): the owner lists a series before anyone enters it.

**5. What an option leg is to the books.** Its row's key is its market, its day, calls or puts, its strike. The
clearing side's units: the size where the user wrote it, the size below nothing where the user holds it. The
gross: the size. What it was entered at: what it is worth exercised at its entry by the clearing side's units,
added to the row's number and to the shard's one number over all rows (17.3: the books take an option at what
it is worth exercised at its entry). No premium is in the books.

**6. What a shard is asked, and the trace of a draw and of a call.** As an account (17.4, 17.8h): what its side
can lose by a move of the rate, the rows of one market counted together, and the rate on the gross of every row
at the market's price.
- **The order** is the list's (the markets, as today): for each market its futures row, then its dated rows as
  the tree has them. A trace's row says the place (0 to 3 a futures row; 64 and the leaf's place a dated row),
  the place of its price in the list, the two sums of the open market, what the closed markets are asked, the
  worth of the gross, the clearing side's worth signed, and **the hash of the key the tree names next** (the
  head's at the start; carried by a futures row; a dated row is of that key and writes what its leaf names).
  88 bytes a row; sixty-seven rows at the most. The end holds that the last row has no key left, and that the
  count of futures rows traced is the count of rows with a market (their places in the list rise, so none is
  traced twice).
- **A futures row** adds what its net units gain from the price to each end of the rate (the account's `ends`).
  A market with no dated row is so asked the rate on its net, to the rounding: the amounts of today's tests
  move by a sompi, as the account's did at 17.8h's stage 1.
- **A dated row of a day to come** adds what its net is worth exercised to the clearing side's worth; and to
  the two sums: where the clearing side wrote (net below nothing), exactly what it gains at each end; where it
  holds (net above nothing), the same where the strike is outside the ends, and along the line of nothing
  between them (it loses what it is worth exercised now, at both ends: no line is chosen for a shard). **Within
  T of its day** each of its two sums is no more than nothing (17.4, N68).
- **A row whose gross is nothing** adds nothing and reads no price (review T8, P1), a day's leaf among them.
- **A draw is wrong while a dated row of an ended day has a unit.** (The owner ends the legs, or waits for the
  fallback: 10.) No day's price is read by a draw.
- **A call** is right by one price a market that shows it (17.4, N80: "a call's test of a shard is the same,
  over its rows"): an entry of the list says, beside the price, a second price within the lower rate of it; the
  rows sum what the clearing side gains from the first to the second (a dated row within T of its day: no more
  than nothing); the call is right where the shard's equity at the first prices and that sum are below the
  lower rate on the gross. This takes the place of the price moved the tolerance in the shard's favour a row
  (`counted_at`), which has no meaning for rows counted together. An entry of the list is 64 bytes.
- **A dated row of an ended day, in a call's trace**, is counted at its day's price: its answer is shown the
  day's leaf, or the leaf before where none stands, and then counts the row at what its legs were entered at
  (17.5). A script of its own (`KpfTShardRowE`): the common answer does not carry a third path.

**7. The row's gate** (17.8c) for a dated row is the same sentence: what the shard holds, less what it owes, is
at least what this row alone is asked with the leg, at the leg's entry, at the upper rate; or the row is asked
no more with the leg than without it. A row alone, where the clearing side holds, is asked the rate on the
gross only.

**8. An option enters** (kind 10, as a futures leg; `KpfTCEnter` is built for it).
- `KpfTCRow`: the leg an entry says may be an option where: its time is the statement's; its day is more than T
  after it; its strike and premium are within the limit. Its row is an option leg's that stays.
- `KpfTEnterSD`, the twin of `KpfTEnterS` for a leg with a day: the dated row and its path where the twin reads
  one of four; the row is there (4); the leg's scale and limit are the row's; the gate (7); the root rewritten.
- **The premium of an option the user writes** is put into the account in the transaction that states the
  entry (the entry's "add", which the account's gate then holds: the coins hold every premium, T15's Y1). An
  entry carries the two signatures; whose coins they are is the signers'. **Where the entry ends unapplied the
  added coins return to the key the entry says** (the owner's, for a written option; today what is added stays
  in the account). Open: read in the scripts where an entry ends unapplied (a wrong point, a lapse, the
  shard's time) and say the one place the key is paid.

**9. An option leaves before its day** in a step, or with its account under a call; never by its user's
statement (17.4). The note to the shard says the leg whole (as today: its chain) and what is asked or paid; a
premium paid is coins in the note, and is no row's number.
- `KpfTLeftD` (a note of one leg with a day; a shard not called), its twin for a called shard, and
  `KpfTLeftVoidD`: the dated row by its path; the row and the shard's number lose what the leg was entered at.
- A note of several legs of which some have a day (`KpfTLeftAllD` and its twin): a futures leg by its place
  among the four, a dated leg by its row and path against the root as the legs before left it.
- Open: whether `KpfTLeftAllD` is one script or the note of an account that holds option legs is written a leg
  a note (what `KpfTCEnd` writes changes then: a step of every leg writes up to six notes).

**10. The day** (17.8g, B and C, as transactions).
- **`KpfTDayIn.price`**, anyone: the shard, beside the committee's note of the market and the day (read as
  17.8g, A says: four times its spread within the tolerance); no leaf of that key stands (the leaf before); the
  leaf is written with the lower of the price and the market's limit; the count of the market's futures row
  rises. **`KpfTDayIn.none`**: the same with no note, shown an input that landed at or after the day and the
  fallback's time. Either writes **the shard's note of the day** (tag 23), which `KpfTDayCopy` copies.
- **`KpfTDayLeg.end`**, anyone: the account, not under a statement; the shard's note of the leg's market and
  day; the leg named, its day at or before the note's. What it gained is what it is worth exercised at the
  day's price less the same at its entry, less the entry's part of its band, and for a held option less its
  premium. Below nothing: the coins pay it into a note to the shard, and what they do not pay is added to what
  the account owes. Above nothing: **the number of what the account owes goes down by it, below nothing where
  it owed less**: a credit, no coin. A written option's premium is its user's from here. "None": the leg ends
  at its entry and every premium is its user's. The note to the shard says the leg and what was paid; applied,
  the row loses the leg. A keeper's fee after the loss and the premium, from the coins, never owed.
- **A credit** counts for the account wherever what it owes counts against it (every end subtracts that
  number: below nothing it adds). **It is taken out by a take that says how much of it**: the take's test
  counts the credit less that; the account writes a note to the shard that asks it for the user's key (the
  form a mark's note has, of no leg), and the number rises. A leaving of every leg asks what is left of it in
  its note. Nothing touches the shard and an account at once (14).
- **O9, decided: in a step an ended leg is passed by a row that adds its premium and nothing else, and is named
  by no step** (it leaves by `KpfTDayLeg.end`). **An account that holds an ended leg is not taken whole**, as
  one that holds tokens: the leg named leaves alone, owing what the coins do not pay. In every other kind an
  ended leg's row is wrong, as built: its user ends it first. So a due account is stepped while no price of a
  day is written; what an ended leg lost waits for the price or the fallback.

**11. Under a call** (17.3, 17.9).
- `KpfTCStep` takes an option leg of a day after the call's time at the call's price for its market: what it
  gained or lost exercised from its entry, as a futures leg; no premium moves; a leg younger than T asks
  nothing (17.9).
- An account with a leg whose day is at or before the call's time is ended first (`KpfTDayLeg.end`, whose note
  is a claim of the first class where it asks): the call's step refuses it. **Said as it is:** no claim is paid
  while a row has a unit, so one such leg holds every claim until the day's price is written or the fallback's
  time has passed.
- A row of gross nothing is counted in no call.

**12. What moves an option without a price** (a hand-over, a join, a split by the two signatures): the leg's
bytes go as a futures leg's; the premium is in the coins. To be read in `KpfTPass`, `KpfTJoinC` and `KpfTLegOut`
that none of them reads a leg's way as 1 or 2 only, and that a join keeps legs of one market next to one
another or the account is asked more (17.4).

**The order it is built in**, each a commit with every test passing, a review at the engine after B, D and E:

| | What | Scripts |
|---|---|---|
| 0 | **The model first** (no script): a shard's books of futures and dated rows, the requirement by rows against what the legs under them can lose (the rows never ask less), a draw and a call by one price a market, ended rows, a credit in an account through its day; with the walk of 17.8, 1 by rows | none |
| A | The books' hash with a root; the tree back for dated rows; a row put in and out; a trace's row for a dated row; the two sums in a draw and a call (futures amounts move by the rounding) | `KpfTRowIn`, `KpfTRowOut`, `KpfTShardRow` (and a twin for a dated row), `KpfTShardEnd`, `KpfTCallEnd`, `KpfTShardPrice`, every script that shows the rows (33 bytes) |
| B | An option enters and leaves in a step | `KpfTCRow`, `KpfTEnterSD`, `KpfTLeftD` and twins, `KpfTLeftAllD` |
| C | A call with option legs | `KpfTCStep`, the twins for a called shard |
| D | The day: its leaf, the shard's note, an ended leg, a credit, O9 | `KpfTDayIn`, `KpfTDayCopy`, `KpfTDayLeg`, `KpfTShardRowE`, `KpfTCRow`, `KpfTCEnd` and the other ends (what is owed below nothing) |
| E | The tool: a series listed, an option entered, stepped, ended; then testnet | none |

About fourteen scripts more: the library's genesis is then of ninety copies (17.8e's last line: with the
scripts' final count).

**For the review on paper**: each numbered point against the scripts and models as they are (does anything
built contradict it; what does it not say that a script will need); the trace's order and the count that no
row is left out or traced twice (6); the futures row as a market's head against a row emptied by the last leg
(2) and against review T9's Q1 and Q3; a call by one price a market against review T3's L4 and 17.9 (what an
owner gains by the second price being its to choose; a caller's); the day's leaf as the last key of its day
(3, 4, 10) against a row put in for an ended day; a credit (10) against every end that reads what an account
owes, and against a called shard; O9 as decided against the freeze it was found as, and whether "not taken
whole" leaves a loss unpaid; what an owner or a user gains by the day's price being written late, or never;
the premium's return (8); whether any of it can be left out of the first form with the goal's row still held.

**Step 0, as shown** (`research_options.rs`, twenty-one tests; `s469_model_rows.py`; no script).
`a_shards_rows_ask_no_less_than_its_side_can_lose_and_no_more_than_its_legs`: on 200,000 random books of one to
eight legs (five strikes, three days, so that legs meet in one series), at the upper and the lower rate, what
the rows ask is **never less than what the clearing side loses at any price the move reaches** (the ends, every
strike between and a price either side), exactly that on 221,886 of 400,000, and never more than the legs asked
one by one; netting a series changes nothing of what is lost at any price. On 16,521 the rows ask less than leg
by leg, 5,073 of them less than the legs counted one by one near their day: legs of one series set each other
off exactly at every price and at their day, so **a row near its day is netted as any other**, and the rule of
N68 is the row's, not the leg's. `with_the_shards_levels_by_rows_nobody_is_left_unpaid`: review T2's walks (an
owner that pays its shard up, states at the edge and sets its book off; users that know the next price and
take; a hedge by the clock, a user's and the owner's own) with the levels by rows, 28,000 paths: no user and no
account unpaid. **What this does not show:** in those walks two legs seldom meet in one series (what the owner
took differs from the walk leg by leg in one set of twenty-eight), so they say that nothing breaks, and the
books at random are what holds the rule. Not in the model yet, and before step A: a call by a second price a
market (6) against today's call of a futures shard. Reckoned: today a price is moved the tolerance in the
shard's favour and the lower rate is the maintenance rate less the tolerance, which for one market is the
shard's equity at the price below the maintenance rate less twice the tolerance of the net; so the second
price is within **that** rate of the first (the account's lower rate less the tolerance), and a futures shard
is called where it is today, to the rounding. An ended row in a call, and a credit through a day, are 17.8, 1's
(`a_held_leg_that_gained_at_its_day_leaves_only_where_the_account_stands_without_it`) and are not shown again by
rows.

### 17.9 Of the form that is built: a leg entered, and a call (F116, high: closed at the call's step; L1 and L4 open)

14.6 has no band under a call: every leg leaves at the call's one price for its row, and what the accounts
gained is paid at one share. An entry's price is held to the committee's only within the tolerance, a call's
the same, and **nothing bounds a leg's size against what the shard holds** (no gate at an entry: 14.3). So an
owner's own account enters a long of 10,000 units at the tolerance below a price of 103 (56,400 KAS of coins,
which it has back); the shard is far below its lower level from that transaction; the owner calls at the
tolerance above: the leg has gained 10,300 KAS and is a claim at one share. A shard that holds 1,000 KAS and
would have paid an honest claim of 350 whole pays it 33. Read in `KpfTCStep` (what is asked is what was gained,
whole), `KpfTCallEnd` (the call holds the prices as stated), `KpfTShardPrice` and `KpfTStepPrice` (the bounds of
a stated price rounded outward); in numbers in `research_options.rs` (an honest claim of 401 paid 37.8).

**A band under a call is not enough**, though it was the first answer here. It takes back the two edges (and
not exactly: a short is left the tolerance's square twice, at every leaving today as well). It does nothing
about what the price moves between the entry and the call: until someone calls, the owner holds a bet that pays
one way (up a tenth of a percent, the leg claims 1,030 KAS beyond its band). And it costs every honest account
that gained 1 % of its worth at a call it did not choose, which the owner has when the call ends.

**Decided: a leg says the time it entered, and under a call a leg that entered less than T before the call's
time asks nothing where it gained, and pays what it lost.** T is 17.4's: twice a lateness and a dispute period, computed from the terms. No band under a call, as today.

- **The young legs of an account are counted together** (N78): nothing where together they gained, paid where
  together they lost, and what the older legs gained beside. Leg by leg a hedge entered together would pay for
  the leg that lost and ask nothing for the one that gained.
- **What a young leg gained is nothing, and no second claim** (N79). In the model, with users that know the
  next price: 1,276, 3,681 and 35,343 a path at ticks of 0.5 %, 2 % and 4 %, against shards of five million.
  An owner does not gain by bringing it about (it loses its shard).

- The leg that took the shard below its level is younger than T at any call that whoever watches states in
  time: it asks nothing, and the honest claims are paid whole. Had the price fallen, it pays.
- A leg older than T entered while the shard, with it, was not called for T: it was within what the requirement
  carries, and its two edges are what the levels' "the tolerance in the shard's favour" count.
- What it costs an honest user: one that entered within T of a call has no gain of those minutes. Said beside
  14.7's lines.
- It rests on what everything rests on: that whoever watches calls a shard within T of the transaction that
  took it below its level.
- The two bounds of a stated price are rounded inward.

The same round trip with no call: one path takes no band, the leaving of a leg that lost. A long in one account
and a short in another, each entered at its edge, have the tolerance twice of the notional a round once the
price moved more than the band. Against an honest owner it does not exist (the owner signs every entry; its
tool signs no edge). With the owner's own keys it is the owner taking from its shard by notes, held by the call
that the entries make due and by the rule above at that call; where nobody calls for T it is open, as a draw
nobody disputes is.

**Built** (17.8, step 2; under review):

- **The leg.** A covered account's leg is 111 bytes as before. Its second push, where a leg of an account no
  shard covers has its committee, is four numbers of 8 bytes: the time it entered, and a day, a strike and a
  premium a unit, nothing for a futures leg (a token leg names its vault there, as before). `leg_more`,
  `leg_time`, `leg_entered` in `covered.rs`.
- **The committee is shown apart.** A shard has one committee, in its id (the hash of a seed, the committee, the
  terms and the periods). `KpfTEnterS` is shown it (one argument more), holds it into the id, writes it in the
  head of the row, and holds the leg's second push to the statement's time and three numbers of nothing.
  `KpfTStepPrice` opens the id by the seed for every leg of a covered account (kinds 7 and above), as it did
  for a token leg: whoever answers a price of a covered account shows the seed of its shard (`c_price_seeded`;
  `c_enter_price` takes it).
- **A leg's row** in a shard and in a call is found by its market's name, its scale and its price's limit
  (`KpfTCStep`, `KpfTLeft`, `KpfTLeftAll`, `KpfTLeftVoid`; `Head::same_row`).
- **The call's step** (`KpfTCStep`, `call_step_at`) sums the legs that entered after the call's time less T
  apart, and adds that sum only where it is below nothing.
- At the engine (`under_a_call_the_young_legs_of_an_account_ask_nothing`): a long that entered T before the
  call asks its 30 KAS and one that entered a block later asks nothing, as does one that says a time after the
  call's; a young short pays; a long and a short entered together, young, nothing either way; an old gain and a
  young loss set off; an old loss paid beside a young gain of nothing; a note that asks a young leg's gain and
  a step that lets a young loss off are refused; an entry writes the statement's time whatever the leg given
  said, and one whose leg says an earlier time, a committee, a day or a premium is refused.
- A leg that entered before this form says, where its time is, the first bytes of a committee's id: young or
  old as those bytes fall. There is none on a chain that matters.

**Review T3** (`docs/audit/REVIEW-T3-legtime.md`; its tests `auditT3_legtime.rs`, twelve; the tests of review T1
brought along: 27 of them failed by the change, every one as it was meant to, none a regression; the default
workspace 668 tests, none failed). **F116 as found is closed at the engine, at the step**: a shard of 19.9 KAS,
an honest claim of 15 KAS, an owner's long that gained 3,000 KAS: the honest claim is paid 15 where the owner's
leg entered a block before the call's time, and 0.099 where it entered T before. Held under attack: the time
is written by `KpfTEnterS` alone and is the statement's; a join, a hand-over, a mark and a split carry it, and
written otherwise each is refused or answered wrong; no leg makes its account one that cannot be stepped (the
largest time, the largest below nothing, bytes of 0xff); an account has its shard's periods; F80 stays closed
(another committee shown; the shard's shown and another's head written; a row of another committee made by
hand); the model and the engine agree on 180 random accounts of young and old legs. `KpfTCStep` is 4,829 bytes
(4,113), a step of one leg 6,890 (6,174), of six 7,538; an entry stated 4,902 (4,825).

| Finding | What it is | Decided |
|---|---|---|
| L1 (medium) | the rule is in the call's step alone. A young leg whose account nobody steps leaves by its user's own statement after the call (F95's way, open by decision), and its note is a claim at one share: entered at 6,201, a call of time 6,500 at 12; its step asks nothing; its leaving stated at 6,601 at 15 asks 48.5 KAS of the called shard. A mark's note is counted the same way (read) | **the rule goes where a claim is counted**: with the claims of 17.7 (step 4): a claim is of two classes |
| L4 (medium; N79 at the engine) | "an owner does not gain by bringing it about" does not hold: a shard of 20 KAS, a user long 5 units, 100 blocks old, that gained 15 KAS; the owner signs in a long of 1,000 units of its own and the call is due at once; both legs are young, both steps ask nothing, the call ends, the shard is open again and owes nothing: the user has its coins and nothing of the 15. Had its leg been older than T it had been paid whole | **what the young legs of an account gained is a claim of the second class**: paid after the others, of what is left, less the band (17.7). Not nothing |
| L2 (low) | a leg's time is up to a lateness before its landing: old 200 blocks after it landed, not 300 | said as it is: T counts from the time said |
| L3 (low) | a call's time is within a lateness before its landing, at the caller's choice: an honest user is sure of its gain a lateness and T after its entry (400 blocks with these terms) | said as it is, beside 14.7's lines |

Rests also on: the committee answers a call's disputed price within a dispute period (a call that lapses twice
ages a leg past T); every false statement on an account is disputed (an entry whose account says a time of
nothing is taken where it is stated, answered wrong at its end, and stands where nobody disputes). **Not built
yet:** the two bounds of a stated price rounded inward.

### 17.10 Review T4: the tree, the traces and the claims, on paper

`docs/audit/REVIEW-T4-tree.md` (T1 to T18; read, and reasoned with numbers; nothing run). **The tree is sound in
kind and can be built, not as step 3 was written.** Where 17.6 to 17.8 say otherwise, this holds.

| Finding | What it is | Decided |
|---|---|---|
| T1 (high) | the head leaf is not below every key by its numbers (a script's numbers have a sign: half of all market ids begin below nothing); found by the model the same hour | the head is below every key by rule (`books.rs`) |
| T9 (high) | a trace held by its root is disputed blind: whoever watches sees that the root is wrong and not which of sixty points; and nobody but the caller can build a path for a call's step | **the trace whole in what is said**, as an account's, with a list of prices in order. About 440 bytes at four rows, 3.5 kB at sixty-three; a row answered about 5 kB at four rows |
| T12 (high) | 17.7 had dropped "no claim is paid while a row has a unit": with a share of all ever held, whoever is shown first is paid whole (a shard of 100, three claims of 100: the owner steps and claims first and takes 100) | **kept**, beside the time before which none is paid |
| T16 (high) | the tree is not separable from the traces in the order written | 17.8a |
| T2 (medium) | "an underlying" was the market, the scale and the limit: one market could have two futures rows and two prices | **an underlying is its market**: one scale and one limit a market, held against the two neighbours where a row is put in |
| T3 (medium) | a leaf's "what its legs were entered at" against the one number over all rows: a mark's note reads no row | the leaf's number for rows with a day only (no mark names an option) |
| T4 (medium) | a count of rows is wrong for ever if one script misses a crossing (two legs of one row in `KpfTLeftAll`) | **the sum of the rows' gross**: the same 8 bytes, the same test for nothing |
| T6 (medium) | sixty-three rows beside the head; three days of ten strikes of calls and puts and the futures are sixty-one, and eighty-one across a day's roll | depth six, a constant of the scripts; a clearing party of many series has more than one shard |
| T7 (medium) | two classes of claim need three numbers more, and the moving part has no room | **the books' hash is of the sum of the gross, the root, and the hash of the claims' numbers**: a script of rows carries the last unopened |
| T8 (medium) | the table of 17.6 counted arguments and not the script, which is in every transaction: an entry on a row that is there about 5.2 to 5.5 kB (4,902 today), a note of one leg applied 4.35 to 4.6 kB (4,002) | said: **the common path is 6 to 15 % heavier with the tree, not lighter**. Measured where it is built |
| T10 (medium) | a call's object as a root costs every step six paths (a step of one leg 8.4 to 9.2 kB, 6,890 today), and "a row an underlying" lets a stepper give a futures leg an ended day's price | **the call's object is the list of prices**: 224 bytes at four markets (368 today), no path, nothing to choose |
| T11 (medium) | an ended row counted at its entry where nobody signed its day's price: disputed, that point lapses at the caller's cost | a price said as none is right unless a note or a citation is shown; with the day (step 5) |
| T13 (medium) | "a part below a note's least is not paid" and "the call ends where what was paid is what is owed": a dust claim keeps a shard called for ever, and blocks the second class | a claim that is covered is paid whole whatever its size; the second class is paid of what is held beyond what the first is still owed |
| T14 (medium) | where a claim has its class: "stated after the call's time" is computed by no applying script, and an owner backdates a call to push a user of an old leg down | **the class is decided where the note is applied, by its legs' times against the call's**; the call's step writes two amounts in one note; a note of several legs any of which is young is of the second class whole; nothing of the second class is set against an old loss or what the account owes |
| T15 (medium) | **L4 is narrowed and not closed** (below) | said as it is; the gate, again, once the traces are there |
| T5 (low) | who puts a row in and takes one out | a script of its own, signed by the owner, puts one in; a row with a day is taken out by anyone once the day has passed, one without by the owner; the owner pays for dead rows |

**L4, as it stands with a second class** (the review's numbers: a shard of 19.9 KAS, a user's young gain of 15,
an owner's own young long of 1,000 units; the band is each young leg's larger worth, at its entry or at the
call, rounded up):

| The price before the call's time | The user is paid |
|---|---|
| does not move (honest prices, or both edges) | 14.35 or 14.67: whole |
| up 1 %, honest prices | 14.99: whole |
| up 1 %, the owner's entry at its edge | 3.79 |
| up 1 %, both edges | 2.12 |
| up 2 %, both edges | 1.16 |
| up 1 %, both edges, 10,000 units | 0.23 |
| down 1 % | 13.71: whole; the owner loses nothing |

An owner so has a bet that pays one way, of any size, on what a price moves while its own leg is young, and
what it wins is what its newest users gained in the same time, no more: nothing of a leg older than T and a
lateness, and nothing a user put in. Without the band the user has 2.1 for certain. **The review found no
closure short of a gate at an entry** (no leg enters where the shard, with it, is below its level). The gate
was cut by review 49 for what it cost before there were traces (a script, a second bond, a lapse of its own).
With a draw and a call as traces (17.8a, 2) a gate is the same trace at the entry's prices: what it costs is
then measured, and whether every entry carries it is decided there. Until then it is said to a user: what a
leg gained is its user's for sure once the leg is T and a lateness old.

Holds, with no finding: a leaf's place need not be in the leaf; a stale leaf-before does nothing; no row leaves
under a pending draw or call; a user makes no row; nothing pins the fixed part across a call; a draw is held up
no longer than today; a call dated ahead is not applied.
