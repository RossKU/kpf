# Review T13: the account's side, stage 1, as built (commit 8dbae87)

Tests: `crates/kpf-protocol/tests/auditT13_rows.rs`, 15 tests, all pass. A test named `t13_wN_...` shows the
finding WN as it stands: it passes while the finding is open and fails, saying "WN is closed", when it is
fixed. `t13_held_...` is of what was attacked and held. Below, "engine" was run through the script engine;
"model" through `covered.rs` and `crow.rs`; "read" was read only.

## Verdict

**The scripts are the model, and the model is the rule, to the sompi; the rule has one hole that loses the
shard's coins, and it came in with this commit.** `KpfTCRow` computes a row as `crow.rs` does on every one of
4,800 rows at the engine (each of the 21 numbers a sompi off in every kind; every number of the row before bent;
the group closed at every pair of signs), two corners apart where the engine takes neither verdict and no coin
moves (W5, W9). The ends are the rule written out, a sompi either side, at 43 ends. The leg before cannot be
played: nothing but the leg before opens the row before's chain. At the markets' prices, 12,000 accounts walked
on the model through 13,000 takes of the most the rule gives, leavings, marks, entries into groups and 9,500
steps leave the shard unpaid nothing, and every due account takes a step.

The hole (W1, high): **a due account that holds a token leg and a partly hedged group takes no step from
anyone.** The step of every leg, which "lacks less" now sends such an account to, is refused beside a token
leg, and the way out that stood before the commit (the leg leaves alone, the account owes) is reached only
after "lacks less". The account goes under with the price. This is in review T12's check list as written; it
is the rule's, not a slip of the script. It should be closed before anything is built on stage 1.

Second (W3, medium): whoever steps chooses the leg, the step that cures a hedged account pays nothing and the
step that ends it pays the coins. The rest is low.

## Findings

### W1 (High; engine): a due account that holds a token leg and a partly hedged group takes no step

`KpfTCEnd`, kind 9, the leg named lost:

    if (af >= bf) { all = 1; } else if (m + gl < 0) { if (tk != 0) { owing = 1; } else { all = 1; } }
    ...
    if (all == 1) { if (tk != 0) { ok = 0; } }

Before this commit a leg that lost and that the coins did not pay left alone beside a token leg, the account
owing the rest (16.18): a due account always took a step. Now, where the account does not lack less after the
leg named, the step is of every leg, and every leg is refused beside a token leg. A long of 10 and a short of 9
of one market: without the long, the short alone is asked 5.5 % of 81 where the two were asked 5.5 % of 9. The
short gained and is not named while the long has lost (E1). A sale of tokens (15) is of an account that owes,
and this one owes nothing. No statement of anyone but the user is right. (T12's check list has it so: "else
every leg (wrong while tk != 0)".)

`t13_w1_a_due_account_with_a_token_leg_and_a_partly_hedged_group_takes_no_step`: long 10, short 9 (from 10), a
tenth of a token (counted 0.8 KAS), coins 0.5 KAS above a note's least. At 9 it is due by the end's own test.
Model: no step for any leg named, no sale. Engine: the end of a step that names the long, or the short, with a
fee of nothing or of the long's worth, is answered wrong and not right (4 ends). At 8 the account has lost
0.7 KAS more than its coins and tokens, and still takes no step. The same legs with 0.8 KAS of coins in the
tokens' place are stepped at once (every leg).

Any token leg makes `tk` at least a sompi. So a user who adds a dust of tokens to a partly hedged group (the
larger leg less than about twice the smaller) holds the net of the group at the shard's risk: it gains with the
price, and below nothing nobody can end it.

Recommendation: where a token leg is held and the leg named does not make the account lack less, do not refuse:
either the leg leaves alone and the account owes (as before the commit), or every futures leg leaves and the
account goes on with its tokens, owing what the coins did not pay (what the call's step does: `KpfTCStep`,
`call_step_at`). Then, in the model's walk, with token legs: a due account takes a step, whatever it holds
(the walks of this review and of 17.8h have no token leg, which is why neither saw it).

### W3 (Medium; engine): the step that cures pays its stater nothing where a hedged leg has lost, and the step that ends the account pays it the coins

The fee of a leg that leaves alone is no more than `m + gl - ls` (review 47: no fee before a loss), and `ls`
counts each leg that stays alone: the losing leg of a hedge counts whole, though its other leg gained as much.
So in an account with a hedge that has moved, a step of the leg that made it due pays nothing. The stater
chooses the leg. Naming the hedge's losing leg (the account lacks more without it) takes every leg: the fee of
the worth of all, as far as the coins go, and what the legs gained together is asked less the band on the gross
worth of all (E8), the hedge's two legs in it.

`t13_w3_the_step_that_cures_pays_nothing_and_the_step_that_ends_the_account_pays_the_coins`: a hedge of 100
units at 11 (from 10), a long that gained 3, a small long that lost 0.4, coins 2.1 KAS above a note's least; due.
Named, the small leg: it leaves, fee 0, the account stands with 1.7 KAS and is not due. Named, the hedge's
short: every leg leaves, fee 2.1 KAS (all the coins), and the 2.6 KAS the legs gained together are below the
band (23.1 on 2,312.6): the user is paid nothing. Both statements are right at the engine. The user loses its
1.7 KAS and what its legs had gained by the stater's choice, and the stater is paid only for that choice.
(With 2.0 KAS the account is still due after the small leg left, and then the only right step names the hedge's
short and takes every leg.)

In the walks at the markets' prices: at 486 visits of 6,000 accounts (395 of the second 6,000) both a step of
one leg and a step of every leg were right; the largest fee of a leg alone summed 205.9 KAS (153.0), of every
leg 891.2 KAS (752.5).

Recommendation: the fee's cap counted by group (what the groups that stay lost, not each leg alone); and a step
of every leg right only where no leg named alone would be, or its fee no more than the best single step's. In
the model first. This is review 47's rule met by netting, and E8 again; it was so in kind before the commit
(the stater chose then too), and a hedge makes the two ends further apart.

### W2 (Low; engine): the fee counts in "lacks after", so a sompi more of coins turns a step of one leg into a step of every leg

`af = rst - (m + tk - owes + gain - fee)`, and the fee is `min(the rate on the leg's worth, m + gl - ls)`.
Where the leg named is the larger leg of a group and what the account lacks falls by less than the fee, the fee
itself makes `af >= bf`: every leg leaves, for the fee of the worth of all (as far as the coins go).

`t13_w2_a_sompi_more_of_coins_turns_a_step_of_one_leg_into_a_step_of_every_leg`: long 100, short 48 from 10, at
9.95. Coins 7.18899999 KAS above a note's least: the long leaves alone, fee 2.18899999, the account goes on with
its short. Coins 7.189: every leg leaves, fee 4.589 (all that is left after the net loss of 2.6). Both ends are
answered right at the engine.

Recommendation: the fee no more than leaves `af < bf` (at most `bf - af0 - 1`, `af0` what the account lacks
after with no fee); or "lacks after" counted before the fee.

### W4 (Low; model; not shown to be of this commit): with every price stated the tolerance off, a move of the whole lower rate leaves a little unpaid

`t13_w4_on_the_model_with_prices_stated_the_tolerance_off`: the walk below with the user stating each group's
price the tolerance off as serves it (a take of the most that gives) and whoever steps stating each group's
price either way (two groups of one market that are apart: two prices). Moves of 4.5 % at the most: on 45 of
4,000 accounts every leg left with the shard paid less than the legs lost at the prices stated, 2.63 KAS in
all, 0.41 KAS the most (the second run, whoever watches taking the step that pays most: none). Moves of 3.5 %:
none. After 398 to 477 of the user's own statements a step was due at once at prices the tolerance the other
way. No due account without a step in any run.

This is the product of the tolerance and the rate (a short that takes the most at the lowest price stated
holds 5.5 % of a worth 0.5 % smaller than the worth the step counts 4.5 % and 1 % of): the rule before had it
the same way, and a hedge does not have it at all (what its legs gained together does not move with the price
stated: `t13_held_a_hedge_that_took_all_and_a_hedge_stated_the_tolerance_off`). Said because 17.8h says "an
account at the level of a take is due at no price": at one price, yes. For the last phase, with the rates.

### W5 (Low; engine): a step's price of -(2^63 - 1) is answered by neither verdict

`if (q < 0 || q - a > dq || a - q > dq) { ok = 0; }`: the script subtracts before it compares, and `q - a` does
not fit. The model says the row is wrong (`crow_after`: `q < 0` first). `t13_a_steps_price_and_a_head_at_their_edges`
(its last case). No coin is at risk: the statement falls when the point lapses, and nobody is paid for an answer.
Recommendation: the model returns "neither" there, or the script tests `q < 0` in an `if` of its own.

### W6 (Low; engine): the model says "neither" of row 1 shown a leg before; the script reads none

`c_row_judged_with`: `None if i <= 1 => {}, _ => return None`. `KpfTCRow` reads `legb` only where `i > 1`. Row 1
of a right statement, shown any leg before, is answered right at the engine
(`t13_held_the_leg_before_is_asked_where_the_row_stands_on_the_row_before`). Harmless: the builder passes none.
Recommendation: the model ignores the leg before for the first row.

### W7 (Low; read): BA and BL are computed and held by the row, and read by no end; comments of the rule before

17.8h: "The bands are read from BA and BL where they are computed from W today"; T12's check list: "X = max(S -
BA, 0)", "13: G >= BL", "Bands: BL where today's is of W - WS; BA where it is of W". As built, `KpfTCEnd`
computes the band from `w` and from `wl = w - ws` (`(w / BPS) * (g + g) + ...`), `KpfTCMark` from `w - ws`;
`covered.rs` the same (`rate_up(w, g2)`). No script reads the row's bytes 72 to 88. The two differ by the
rounding (each leg's band rounded up, against the sum's rounded up once: under a sompi a leg). Sixteen bytes a
row are carried and checked for nothing in stage 1. Either the ends read them (and the option's band of stage 2
has its place), or the doc says they wait for stage 2.

Also read: the head comment of `KpfTCEnd.sil` (lines 8 to 36) still says a row of 120 bytes and the rule
before ("the coins that stay are at `up` of W", "the coins pay what it lost, the fee of its worth, and what the
legs that stay have lost: it leaves"); `KpfTCRow.sil` line 22 says "a futures leg" and the six sums; a test of
`auditT1_builders` prints "with 48 bytes and rows of 112".

### W8 (Low; model): review 47's walk of "what a shard is good for" no longer takes the most a take gives

`t1b_r47_what_a_shard_is_good_for_falls_by_no_statement...` still takes, as "the most the rule gives",
`m - a note's least - (the rate on W + LS)`: the most of the rule before. `t13_w8_...`: on 5,000 accounts as
that walk makes them it is the most on 3,665 and below the most on 1,335 (by up to 56.8 KAS): on every account
whose legs are counted together, which is where the rule changed. It is refused on none (the prices of that
walk are round, so R is not above the rate on W).

The other changes of that file are what the rule's change needs: `one_price` joins only legs next to one
another (two groups apart keep two prices in the rows' and ends' walks); the two thresholds lowered (3 ends by
neither verdict where 5 were; 8 wrong rows of token statements where 15 were) follow from fewer traces of
random prices, and the walks of this review add 1,870 wrong rows and 301 by neither verdict. Measuring "good
for" at one price a market hides nothing real: at two prices for two groups of one market, the account before
is counted at prices no market has together, and the same legs after the leaving that joins them are counted
at one; at any one price what the legs gained is the same before and after. The walk of this review states two
prices for such groups (the tolerance each way) in every kind: no due account without a step, and nothing
unpaid at moves of 3.5 %.

Recommendation: the take of that walk at `m - a note's least - (RA' + LS)`. (This review's walk takes that
amount 13,224 times, a sompi more refused each time, and what stays pays what the legs lost together.)

### W9 (Low; engine): a token leg's row is answered by neither verdict where its worth at the upper end does not fit

`KpfTCRow` computes `quoteAt(size, scale, up)` before it finds the leg a token leg and counts nothing of it;
`crow_after` computes none for a token leg, and `c_check` holds a token leg's worth to 64 bits at its price
limit only. `t13_w9_...`: a token leg worth 9 * 10^18 sompi at its row's price: right by the model, neither
verdict at the engine (a tenth the size: right). 90 billion KAS of tokens: no account will hold it; said as a
place where the model gives a verdict and the script none. Recommendation: the script computes the ends' worth
inside the futures branch, or `c_check` holds a token leg's worth at twice its limit.

## Answers to what was asked

- **Item 1, the numbers.** Compared as bytes after being computed: G, W, WS, LS, GL, TK (48 bytes), M, BA, BL,
  DdA, DuA, RA, DdS, DuS, RS, RL (80 bytes). Carried as bytes from the row before: PA, PS, PO (the first row's
  before is nothing, so they are nothing in every right row). Read as numbers and held by a test, not
  compared: a (range; a group's price), q (a step: within the lower rate; else a). The two chains: the first
  opened by the leg shown, the second computed. No number of the 232 bytes is unchecked. A row before whose
  numbers are bent is read as it is said: the row after it is right when it follows from it (2,856 rows).
  The group closes by `max(0, -dd, -du)`: yes, at 14 pairs of signs, of all and of those that stay.
  What review 47's walk of rows does not cover: it tampers a, gain, w, ws, ls, gl only, in statements whose
  rows are all a leaving's (q = a), judged as kind 8 whatever kind is stated; no step's price of its own, no
  kind 12, 13, 14, no token leg, and none of the fourteen new numbers.
- **Item 2, the leg before.** No right statement with a row that cannot be answered right, and no wrong one
  that cannot be answered wrong or left to lapse, was found. The row's chain fixes the leg, the row before's
  chain and the chain two rows before fix the leg before; a row that does not stand on the row before is wrong
  with no leg before asked; a row whose chain two rows before is false takes neither verdict, and the row
  before it is then answerable wrong, also in a dispute of the later row (`at`).
- **Item 3, two groups of one market apart.** Yes: a step states each group its own worst price, the long 4.5 %
  below and the short 4.5 % above, which no move of one price is. A long 10, another market's 5, a short 10: due
  with coins under 16.925 KAS, where the same three as a hedge are due under 7.025. But the account is asked as
  two in every test: at the level of a take it holds 29.575, 12.65 above that edge, and at the level of an
  entry 14.575 with what the legs gained. No statement puts a leg between two (the leg that enters is last; a
  join appends; a leaving only brings legs together). So an account that stood at the level of a take or an
  entry is not made due by this at the same prices; the margin is the rate less the lower rate on each leg's
  worth.
- **Item 4.** (a) A hedge whose user took all: due at no price that may be stated; no take, mark, split or
  leaving of one leg; every leg leaves at nothing. (b) A leg that gained leaves a netted group only where the
  coins hold R of what stays and what each of those lost alone (23.525 KAS above a note's least, where a take
  leaves 17.475): the user
  does not walk out leg by leg. (c) The tolerance in the user's favour on a hedge: what is taken out in all is
  below what was put in (the band on the leg that gained is more than the tolerance gives on the leg that lost).

## Attacked and held

| What | How | Count | Result |
|---|---|---|---|
| Each of the 21 numbers of a row, a sompi more and a sompi less | kinds 7, 8 (one leg from inside a group; every leg), 9 (two statements, prices of its own), 12, 13, 14, and a take beside token legs (one between two legs of one market, one whose market is a futures leg's); every row | 1,806 rows, engine | wrong by the model, answered wrong and not right. The 43 rows as the model writes them are answered right |
| The same of an entry (kind 10), and its last row | each number of each of 3 rows; the leg said another; the last row not at its entry; the leg before at another price than the entry's | 134 rows, engine | 129 wrong, answered wrong; the 5 of the model's right |
| A row after a row before that is bent | each number of the row before a sompi off; the two sums of a group at 14 pairs of signs and sizes (both below, both above, one each way, nothing, the ends of 64 bits), of all and of those that stay; R near the end of 64 bits; the row the model computes from it | 2,856 rows, engine (2,491 right, 64 wrong, 301 neither) | the engine takes the model's verdict on every one |
| A step's price at its edges, a head at its edges | the lower rate either way and a sompi beyond; below nothing; the largest number; heads of no rate, of a tolerance twice above the rate, of the whole, beyond the whole, below nothing; a price of one sompi | 21 rows, engine | the rule written out, the model and the engine agree (W5 apart) |
| The leg before | rows 1, 2, 3 shown the leg before, another, one a sompi larger, none; a false chain two rows before; row 2 answered wrong in a dispute of row 3; a row more of a leg nobody holds; a row less | 22 answers, engine | as said under item 2 (W6 apart) |
| The ends, a sompi either side | 7 (a hedge and a leg; beside tokens), 8 (a leg that gained from a hedge; one that lost, by the level of an entry and by the coins), 12 (what stays; the new account; out of a hedge), 13, 14; 9 due and not (a leg alone; what gained less the band, of an account that owes; a hedge and a leg; two groups of one market apart), the leg named (alone; the coins a sompi short: every leg; E1's account) | 34 ends, engine | each as the rule written out in numbers says |
| The head | the rate, the tolerance, T, one more and one less, with the rows that head makes; the lines at three values | 9 ends, engine | the three numbers held; the lines held by nothing and read by nothing (as 17.8h says of stage 1) |
| An entry (10) at its edge | a short enters next to a long of its market; a sompi under; the leg before at another price; the place one off, nothing, below nothing; the two apart | model | as the rule says (the engine against the model is review 47's walk, which passes) |
| A hedge that took all; a hedge stated the tolerance off | 4 (a), (c) | model, 2 ends at the engine | held |
| The shard unpaid, a due account without a step, an account due after its user's own statement | 12,000 accounts of one to four futures legs of three markets, ten visits each, moves of the lower rate (half the time all of it); 13,224 takes of the most the rule gives, 10,086 leavings, 3,280 marks, 8,987 entries, 8,185 steps of a leg and 1,350 of every leg; whoever watches taking any right step, or the one that pays most | model | none, none, none |

## Not reached, and what was only read

- Bytes that are not the canonical writing of a number (a "minus zero") in a statement: the builders write
  none and take the statement as a `CSaid`, which reads them away. Read: only PA, PS, PO are compared as the
  bytes of the row before (a row before that says a minus zero there makes the right row carry it); every
  other number is read as a number and written anew. Not run.
- `KpfTStepPrice` (where rows begin, their length, the zero row of 336 bytes of which the chain's place is
  read), `KpfTChange`, `KpfTEnterS`: read; no price was cited in these tests. Kind 15's rows: read (the path of
  14 but for the kind).
- `c_step_prices` against every price a stater may choose: read. A group's sum is a line but for the rounding
  of each leg's worth, so a price between the two ends can show a step due a sompi before the ends do; the
  script takes it, the tool does not state it.
- The whole workspace's tests (740 are said): not run, by the rule of one test at a time. Run: this file (15
  pass) and `auditT1_builders` `t1b_r47` (5 pass). The bytes and weights of 17.8h's tables: not checked.
- W1's cure, W3's cap by group: not modelled. Token legs in the walks: none (W1 is of a directed test).
- The option's numbers and stage 2.
