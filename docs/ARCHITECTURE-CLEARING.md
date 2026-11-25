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
| `shard-note` | anyone | applies a note to its shard |
| `acct-open`, `acct-leg` | the user and the clearing party | open a covered account; add a leg (and write its note) |
| `acct-add` | a side | adds margin |
| `acct-take` | a side (or anyone, for side B) | states a take |
| `acct-out` | the user and the clearing party | a leg leaves at once, for what the two sign (and writes its note): section 11 |
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

Order: this section attacked on paper by review; then 1, since without it the rest is of no use; then the model
with notes that arrive late and in any order.

