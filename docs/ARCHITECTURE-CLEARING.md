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
  its own in every block must say times ahead of the chain, which nothing settles: each costs it a bond and an
  account, and its shard takes no entry and gives no draw until the chain is there. After a draw is applied
  entries wait until the draw's time is a lateness behind.
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

