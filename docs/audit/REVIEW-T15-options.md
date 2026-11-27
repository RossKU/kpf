# Review T15: the option leg as it is built (17.8i, commit 5e4f519), at the engine

Tests: `crates/kpf-protocol/tests/auditT15_options.rs`, 15 tests, all pass (332 s, two threads). A test named
`t15_yN_...` shows the finding YN as it stands: it passes while the finding is open and fails, saying "YN is
closed", when it is fixed. `t15_held_...` is of what was attacked and held. Below, "engine" was run through the
script engine; "model" through `covered.rs` and `crow.rs`; "read" was read only. Nothing but the test file and
this report was written; no script, no source, no other test.

## Verdict

**`KpfTCRow` is `crow.rs` for the option leg, to the sompi, and the ends are the rule as it is written; the rule
itself has two holes that lose the shard's or the owner's coins, both where an option leg meets a token leg, and
a third where an option leaves alone.**

The scripts against the model: 18,397 rows of accounts of one to six legs (futures, tokens, options of the four
ways, with strikes, days, premiums and entries at every place where the rule turns) in kinds 7, 8, 9, 10, 12, 13,
14 and 15; 34,944 rows with one of the 24 numbers a sompi off; 4,984 bits of the lines set and cleared: the
engine takes the model's verdict and no other on every one. Two corners part, on legs that no account of the
form has (Y4, Y5). 67 ends with premiums, each computed here by hand from the rule, a sompi either side: as the
rule says. The step's end written a second time in the test file from review T14's list is the model's end on
every one of 11,282 right steps and on every refusal of the walks.

The rule's money, with no token leg: 12,000 accounts walked on the model through 11,169 takes of the most the
rule gives (by the lines that ask least), leavings, splits, and 6,550 steps, the time going on so that legs come
within T of their days and each price moving by no more than the lower rate: **no loss unpaid, no account stuck,
none due after its user's own statement.** A premium is short on 7 steps, each an option named alone beside a
leg that gained (Y3).

The holes:

- **Y1 (high): beside a token leg a take reaches the premium, and a step then asks nobody for it.** What the
  tokens count stands for the premiums in a take; a step pays a premium from the coins only and what they do not
  pay is not owed. A user takes out the premium the owner paid it for a written call, has the call stepped out by
  a statement of its own (the token's price stated the tolerance low: nothing moved), and keeps the premium, its
  tokens and no option.
- **Y2 (high): a due account that takes no step.** A held option that has lost, beside a futures leg (or a
  written option) that has not, and a token leg: the option named takes every leg (E13), every leg is refused
  beside a token leg, and the other leg is not named while a leg that stays has lost (O2). A hundredth of a
  futures leg and a hundredth of a token keep an option bought in the money from ever being stepped. This is
  review T13's W1 again, by the option leg's rule.
- **Y3 (medium): an option named alone pays of its premium only what the coins hold, while the legs that stay
  keep what they gained.** Whoever states the step chooses; the choice moves 2 KAS of a premium of 9 in the
  test's numbers.

Y1 and Y2 should be closed before the seventh step builds an option's entry on this: both are a line or two of
`KpfTCEnd` and the same in the four smaller ends. The rest is low.

## Findings

### Y1 (high; the rule and the scripts; engine). Beside a token leg a take reaches the premium, and a step then asks nobody for it

`t15_y1_beside_a_token_leg_a_take_reaches_the_premium_and_a_step_then_asks_nobody_for_it`;
`t15_y1_y2_on_the_model_with_a_token_leg_in_every_account`.

A take (and a split's account that stays, a mark, tokens leaving, an entry) tests `m - amount + tk >= RA' + pa +
ls`: what the token legs count stands for R **and for the premiums**. A step pays a premium "as far as the coins
go" and what they do not pay is not owed (17.3); an account that holds tokens is not taken whole, and owes what
its coins do not pay of a **loss** only (16.18). The two together:

- **A written option** (17.3: the owner's premium lies in the account, "so no take reaches them while the leg
  stands"). A call written 20 % out of the money for 2 KAS, a quarter of a token (counted 2 KAS and a sompi), the
  coins a note's least and the owner's 2 KAS. The user takes the 2 KAS: the end is answered right at the engine.
  Then a step is stated with the token's price the tolerance low: due by a sompi's worth, nothing has moved. The
  written call is named: it gained nothing, the coins hold nothing, it leaves alone. **The shard is paid nothing
  and owed nothing; the user has the owner's 2 KAS, its tokens and no option.** "Whoever ends an option early by
  its own default does not keep its price" does not hold beside a token leg, and the default is the user's to
  bring about: the rate keeps a distance of twice the tolerance between a take and a step for what the legs can
  lose, and none for what tokens count.
- **A held option**: the same with a call held for 4.8 KAS beside 0.6 of a token: the 4.8 KAS leave, the call is
  stepped out, the premium is paid by nobody. The option was held for nothing while it stood.

Without the token leg the same take of one sompi is refused. In the walk with a token leg in every account (6,000
accounts, whoever watches taking the step that pays it most): a premium is short on 331 steps of 2,291, 881.6 KAS
in all (the most 11.1), 869.1 of it beside coins, tokens or a gain that the user keeps.

**Recommend:** both of (a) tokens stand for no premium: a take, a split, a mark, tokens leaving and an entry ask
that the coins alone hold the premiums (`m - amount >= pa`, or `ps`) beside the test they have, which keeps
17.3's sentence true as written; and (b) where an account holds tokens, what the coins do not pay of a premium in
a step is owed, with what they do not pay of a loss (a paid loss can still leave the coins under the premium).

### Y2 (high; the rule and the scripts; engine). A held option that lost, beside a leg that has not, and a token leg: no step

`t15_y2_a_held_option_that_lost_beside_a_leg_that_has_not_and_a_token_leg_takes_no_step`;
`t15_y1_y2_on_the_model_with_a_token_leg_in_every_account`.

Three rules meet:

- a held option is named alone only where every leg that stays is a held option (E13); beside a futures leg or
  a written option it takes every leg;
- every leg is refused beside a token leg (an account that holds tokens is not taken whole);
- a leg that gained, or stands where it entered, is named only where no leg that stays has lost (O2).

So an account of a held option that has lost (one bought in the money: it loses what it was worth exercised at its
entry), a futures leg or a written option that has not lost, and a token leg takes **no step**, whatever it lacks.
Its tokens are not sold either (a sale is of an account that owes, and it owes nothing).

In numbers: a put held on 100 units struck at 11, bought at 10 (worth 100 KAS exercised) for 1 KAS; a hundredth of
a long from 10; a hundredth of a token; coins of 57 KAS (it stands at 10: asked 55 and the premium). At 10.5 the
legs have lost 49.995 and the account is due; at 11.5 they have lost 99.985, **43 KAS more than the coins hold**.
At neither price is a step of any leg right, by the model (`c_state`, and the prices that show it due), and the
end of each as stated by hand, with a fee of nothing and of a hundredth, is answered wrong at the engine. The same
account without its hundredth of a token: the put named takes every leg at 10.5 and the shard is paid the 49.995
and the premium.

The futures leg and the token leg are dust and the user's to add: an option bought in the money is then held
with what a move of the rate asks and never stepped; the shard carries what it loses beyond the coins until its
day, up to all it was worth exercised at its entry. In the walk with a token leg in every account: stuck at 142
visits, the legs having lost 373 KAS beyond the coins when they were.

**Recommend:** as W1 was closed. Where the account holds tokens, a held option named leaves alone whatever
stays (E13 decides between the leg and every leg only where no token is held), the coins paying what it lost and
of its premium what they can, the account owing the rest (with Y1, of the premium as well).

### Y3 (medium; the rule and the scripts; engine). An option named alone pays of its premium what the coins hold, beside legs that gained

`t15_y3_an_option_named_alone_pays_of_its_premium_what_the_coins_hold_beside_a_leg_that_gained`; found by the walk
(`t15_held_on_the_model_with_options_the_shard_is_unpaid_nothing_at_the_markets_prices`: 7 steps of 3,341 where
whoever watches takes any right step, 7.48 KAS, the most 2.83; none where it takes the step that pays it most).

O3 lets a leg that gained leave alone only where the coins hold the premiums **that stay**. Of the premium of the
leg that itself leaves, the coins pay what they can and the rest is not owed, whatever the legs that stay have
gained; where every leg leaves, that gain is set against the premium first. Whoever states the step chooses the
leg, and may be the user.

A call written far out of the money for 9 KAS (the owner's coins) and a short of another market that gained 5; the
coins hold 7 (a loss of another leg was paid from them: the premium was margin for it, which 17.3 says it never
is). Due. The written call named: it leaves alone, the coins pay 7 of its 9, and the account goes on with the
short; its user then leaves with the short and is paid the 4.05 it gained beyond its band. The short named
instead: every leg (O3), 4.05 set against the premium and 4.95 paid by the coins: the shard has its 9. **By the
choice of the leg the shard has 2 KAS less of the premium, and the user 3.29 more** (the 2, and the fee of every
leg, 1.29, which nobody is paid). The same holds of a held option named beside held options whose premiums are
nothing.

It is not the user's to bring about at will: straight after a loss is paid from the coins the account is not due
(the rate holds the lower rate and twice the tolerance); it takes a move against it. Hence medium.

**Recommend:** an option leaves alone only where the coins pay all of its premium that is not set against what it
gained (as a leg that lost leaves alone only where the coins pay what it lost); else every leg. One comparison in
`KpfTCEnd`, no number more in a row.

### Y4 (low; a leg not of the form; engine). An option leg's row where a worth the rule does not read does not fit

`t15_y4_an_option_legs_row_where_a_worth_the_rule_does_not_read_does_not_fit`. `KpfTCRow` counts a leg's worth at
the two ends and at its entry as a futures leg's before it reads what the leg is; `crow_after` counts for an
option only what its rule reads. (a) A call held on its line of nothing, 900,000 units at 100,000 KAS (9 x 10^18
sompi at the price, more above): right by the model, neither verdict at the engine (review T13's W9, for an
option). (b) A leg wrong before anything (a strike of nothing) whose worth at its entry does not fit: wrong by the
model, neither verdict at the engine. Neither leg passes `leg_size_ok`; nothing holds an option leg to it at the
engine until the seventh step's entry. With legs of the form the walk has no row by neither verdict (0 of
18,397).

**Recommend:** said for the seventh step: the entry holds an option leg to the futures leg's bound, and its
premium to a bound of its own (read: an account of two options whose premiums by their units are 6 x 10^18 each
passes `c_check` and has no row that fits, so no statement and no step). Or the model takes the script's part,
as with W5.

### Y5 (low; a token leg; not of this stage; engine). A token leg named whose discount is out of range

`t15_y5_a_token_leg_named_whose_discount_is_out_of_range_is_right_by_the_model_and_wrong_at_the_engine`.
`crow_after` reads a token leg's discount only where the leg stays; `KpfTCRow` reads it before it knows that the
leg leaves. A token leg with a discount of 10,001 or of -1, named by kinds 14 and 15: the model's row is right,
the engine answers it wrong and not right. No account of the form has such a leg (`c_check`, `spot_added`).
Found by the walk's first run (4 of 1,756 rows). **Recommend:** the model takes the script's part.

### Y6 (low; the model's `c_state`, no script). How the model states a step, a split, and a token leg named

`t15_y6_how_the_model_states_a_step_a_split_and_a_token_leg_named`; counted in the walk.

- (a) `c_step_prices` reads an option leg as a short futures leg and tries the two ends of the lower rate only.
  What the legs gain to a step's price is a line with corners at the strikes: the price a step is due by may be
  the other end, or a strike between. Two held calls, one in the money: due with coins under 8 KAS; `c_state`
  states it only under 2.42. **In the walk `c_state` does not state 1,920 of 5,783 right steps** (1,798 of
  5,499). The scripts take any price within the lower rate: whoever watches with `c_state` misses a third of
  what is due. `thin_draw.rs` asserts "not due" by `c_state` failing; its cases are ones where that is true.
- (b) `c_head_least` takes the lines that ask least of all the legs as they stay (a take's test). Of a split and
  a leaving the least is of what stays and of what moves: a put held against a long, the long split off: by the
  lines `c_state` takes, the put that stays is asked 5.5 KAS that its other line does not ask. The user's own
  loss of use, no coin of the shard's. (17.8i's "as it is built" says "the lines that ask its user least".)
- (c) `c_state` states a leaving, a split and a step that name a token leg, with a trace in which the token leg
  stays; the model's end takes it and returns the account without its token leg, while the chain it writes
  still has it. The row is wrong (`c_wrong_point` names it; `KpfTCRow` answers it wrong). Found because the
  walk's first writing used it: its accounts lost their token legs by steps that asked nothing.

**Recommend:** (a) the step's price by the least of the group's gain over the two ends, the row's price and every
strike between (the test file's `least_prices`); (b) the lines by the statement's own test; (c) `c_state` refuses
a token leg named by kinds 8, 9, 12, 13.

## Answers to what was asked

**1. `KpfTCRow` against `crow.rs`.** Held, byte for byte, on legs of the form (the table below). Looked at
hardest and held: `strike == lo` and `strike == up` (not between: exact) and a sompi inside (the line); `day - t0
== T` (near) and a block either side; `day == t0`, before it (wrong); a premium of nothing, of a sompi, of
100,000 KAS a unit; an entry at the strike, a sompi either side, and where the entry's ends meet the strike; the
band's ends at the strike; an option as the leg an entry says (wrong whatever the row sums: 192 rows); an option
named by kinds 8, 13, 14, 15 (wrong: 1,442 rows of option legs wrong in that walk); a held option that leaves in
kinds 9 and 12 (NS by 256); the lines' bit of each place set and cleared, high bits set, T of the head at
nothing, below nothing and large. Parted: Y4 (overflow, legs not of the form), Y5 (a token leg's discount).
Products `cs * (...)`, `sg * (...)`: read; the factors are 1 and -1 and the engine has no least number.

**2. The ends with premiums.** Held, a sompi either side, 67 ends (the table). Found while computing them by
hand, no fault: where what the legs gain to the step's price is nothing or more, a due account's coins are under
its premiums; so a futures leg that gained, named beside held options, then always takes every leg by O3 (but
beside a token leg), and so does a held option that gained nothing, named beside held options (what the coins
pay of its premium leaves them under the premium that stays). An option that gained beyond its band leaves
alone (shown: with coins of 7.01 KAS alone, a sompi under every leg). The derivation of the
named leg's premium from PA - PS, GL and NS: right in every case tried (a written option that gained, that lost
less than its premium, that lost more; two options of which one is named; a futures leg whose worth is nothing).

**3. The rule's money.** (a) The lines: held. A take by the line "exercised wherever" for a held put against a
long leaves the account at exactly what a move of the rate loses; when the price moves and the strike leaves the
ends the account stands by what the long gained (walk: no unpaid loss, none due after its own statement; the
line lies below what the option is worth at every price, not only between the ends). (b) Near the day: as
written (N68), and worth saying to a user: **a hedged account is due at once, with no price moved, the block its
option comes within T of its day**, unless it holds what its unhedged legs are asked. The long named leaves
alone for the fee of its worth or the coins beyond the premium; the put named takes every leg (E13) for the fee
of every leg; the shard is unpaid nothing either way. In the walk 95 steps of 3,341 came by the time alone, for
100 KAS of fees. (c) A written option's premium: a take reaches it beside a token leg (Y1); a split moves it
with the option, a sompi either side (held); a step the user states leaves part of it unpaid beside a leg that
gained (Y3). (d) O1 for options: held: a call held and a call written at one strike, entered the tolerance off
either way, show 1 KAS from prices alone; the band is their entries' parts, 0.4975 and 0.5025; nothing is asked.
(Read: an entry's part is counted from the entry as stated, and the committee's price is the tolerance of
*itself* away: the part is short by the tolerance squared of the worth, 0.0025 % at 0.5 %. The same is true of a
futures leg's.)

**4. The steps by what is asked.** The cap of a leg's fee by what the legs that stay lost together: shown as
review T14 said it: a long that lost 1 is named beside a long that gained 80 and a short of another market that
lost 80, coins 6: it leaves for the fee of its worth, 4.995; the first market then falls by the lower rate and
every leg leaves: the shard is unpaid 48.595, and 43.6 had no fee been taken: one fee, and no more (the fee is
0.5 % of the worth of the leg named and is paid only from coins beyond its loss). The guard of a leg that gained
(each alone) is one of Y2's three rules. Read, not run: in a step the fee of every leg is of R with every held
option between the ends on its line of nothing, so a held option in the money adds all it is worth exercised to
the fee's base (a put worth 3 KAS: 0.27 KAS of fee); W3's kind, small, and capped by the coins.

**5. "The option leg as it is built" against the code.** It does what it says, with Y6 (b) for "the lines that
ask its user least". The bytes of its table are the artifacts' (7,508; 5,459; 2,003; 2,643; 2,507; 3,767: read
from `contracts/artifacts`). Review T13's three rewritten tests (W1, W2, W3) pass and assert what they claim.
Two remarks on that file, neither a fault of the rule: its `number()` names 21 numbers (the three new ones are
bent here); its walk's `due_at` counts the band of the gross worth, the rule before review T14's O1 (it calls
more accounts due than the rule does, so its "stuck 0" stands).

## Attacked and held

| What | Count | Result |
|---|---|---|
| Rows of kinds 7, 9, 12 (engine) on accounts of one to six legs: futures, tokens, options of the four ways; strikes at the ends, a sompi either side, at the band's ends, at the market's limit; days far, at T, a block either side, at t0 and before; premiums of nothing to 100,000 KAS a unit; q at the edges of the lower rate, a sompi beyond, at the strikes | 2,400 statements, 8,478 rows (4,500 right rows of option legs, 1,595 of a held option between the ends, 1,210 within T of the day; 960 wrong) | the engine takes the model's verdict and no other on every row |
| Rows of kinds 8, 13, 14, 15 (engine): an option named by a leaving, a mark, as tokens | 1,600 statements, 5,569 rows (1,855 wrong, 1,442 of option legs) | the same |
| Rows of an entry, kind 10 (engine), beside options; an option or a token as the leg the entry says; the leg not at its entry | 900 entries, 3,572 rows; 357 last rows wrong (192 of an option) | the same |
| Each of a row's 24 numbers a sompi more and less (engine) | 29,664 rows (kinds 7, 8, 9, 12, 13, 14) and 5,280 (kind 10) | wrong and not right; but 91 right rows of another price (a leg too small for a sompi, an option far from its strike), answered right |
| The head's bit of a leg's place set and cleared (engine): the row as it was; the row written again | 4,984 bits (2,095 change the row) | as the model; in a step the row is the same row whatever the bit |
| The ends with premiums a sompi either side (engine; each need computed here by hand): a take from a covered call, beside a held put on each of its lines; a futures leg that gained, that lost, leaving from beside a written call; a split (a held call with its premium, a written call with the owner's, a long from beside a held put); a mark; tokens leaving; an entry beside a held put; a step due below the premiums; the fee of a futures leg capped after the premium that stays; every leg in the order loss, premium, fee; E13 (beside a long, a written call, a held call); O3; the fee of an option named alone and its cap; PO of every option leg; a written put that lost less than its premium; what is owed beside a premium; a futures leg whose worth is nothing | 67 ends | as the rule written out |
| The short way to a disputed point used by the walks against the long way (engine) | 1 | the same object |
| The step's end written again from review T14's list (`step_ruled`), against the model's end (model) | 11,282 right steps and every refusal of 12,000 accounts; 2,291 more beside tokens | no difference |
| Walks with no token leg (model): takes of the most by the lines that ask least, leavings, splits, steps; the time going on; moves of the lower rate; whoever watches taking any right step, or the one that pays most | 12,000 accounts (9,945 with options), 11,169 takes, 2,543 leavings, 1,678 splits, 4,654 steps of a leg and 1,896 of every leg, 6,250 lives ended at a leg's day | a loss unpaid: 0; stuck: 0; due after its user's own statement: 0; a premium short: 7 steps (Y3) |
| The day approaching; a pair of options entered the tolerance off; the cap of a leg's fee (model; the pair at the engine) | 3 cases | as written; said above |

## Not reached, and what was only read

- **Prices stated the tolerance off** (review T13's W4) with options: no walk. Every statement of the walks is
  at the markets' prices; Y1's step alone uses the tolerance.
- **What is not built**: an option's entry with its premium, the day's transaction, the day's price, the shard's
  dated rows (the seventh step). Accounts with option legs are made in the tests as objects. O9 (an ended leg's
  account takes no statement) stands as review T14 left it: the walks end a life when a leg reaches its day
  (6,250 of 12,000).
- **A call** (read, not run): `KpfTCStep` requires a leg's way to be 1 or 2 (`call_step_at` returns none): **an
  account that holds an option leg takes no step under a call of its shard.** Not live while no option enters;
  said so that it is decided with the seventh step (17.3: "under a call no premium moves; what the option gained
  or lost exercised is settled as a futures leg's").
- **A hand-over and a join** (`KpfTPass`, `KpfTJoinC`) of accounts with option legs: not attacked. (Read: a join
  appends the other account's legs, so a held option's place, and with it its bit of the lines, changes; no
  statement stands across a join.)
- **A sale of tokens (15)** beside options: its rows are walked; its end is not at the engine (the model's is
  the rule before this commit but for the row's length).
- **The end of a mark and of tokens leaving** beside options: one edge each.
- `KpfTCEnter`, `KpfTCSplit`, `KpfTCMark`, `KpfTCTok`: read as the diff from 9f6e4d9 (the row's length, and the
  premiums beside R); `KpfTCRow` and `KpfTCEnd`'s kind 9 read whole against `crow.rs` and `c_end_as`.
- The weights of the answers in a block (9,189 bytes for a held call's row, as the text says): not measured.
