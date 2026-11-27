# Review T14: the option leg in an account (17.8i), and a step by what is asked (17.8h, commit 1f89ec0)

On paper. Nothing was run, built or edited but this file. Numbers in the scenarios are in one unit (read them
as sompi or KAS alike); the rates are the model's: the upper rate 5.5 %, the lower 4.5 %, the tolerance g
0.5 %, the liquidation fee 0.5 % (the last is assumed, where a scenario needs it).

**What was read.** `docs/ARCHITECTURE-CLEARING.md` 17.2 to 17.5, 17.8h whole, 17.8i whole; `crow.rs` whole;
`covered.rs`: `over`, `rate_up`, `c_end` (lines 544 to 744), `c_enter`, `c_enter_end`; `research_options.rs`
lines 1 to 147, 276 to 297, 1366 to 1502; `research_account_rows.rs` lines 455 to 604 (`step_net`, `walked`,
the test's written-out part). **Grepped only:** 17.8g (its headings and D7), F65 (line 804), the signature of
`KpfTCRow.answer`. **Not read:** `KpfTCEnd.sil`, the body of `KpfTCRow.sil`, `c_fees`, the two review files
(E1, E5, E13, W3 are taken as 17.8h's tables quote them), the random walk after line 604. **Inferred, not
checked:** that no charge at an entry takes back the entry's edge of the tolerance (O1 rests on it); the codes
of the legs' ways; what the scripts will weigh.

## Verdicts

**(2) A step by what is asked, as built: change.** The fee of every leg on R stands. The band of every leg on
R does not: it covers the edge of the price at which the legs leave and not the edges of the prices at which
they entered, and a hedged pair is paid review 43's F65 again, with a band of nothing (O1, high). "Lost
together" across markets lets a gain be paid out to the user's key beside a loss that another market's gain
covers and no coin does (O2, medium; new with this commit). Both are closed without a number more in the row:
the band as the entries' part (the tolerance once on the worth, gross) and the leaving's part (R by the
tolerance once over the rate, net); the guard of a leg that gained as review T12 decided it (no leg that stays
lost, each alone) until a gain is credited and not paid out (17.8g, C).

**(1) The option leg: change first, then build.** The row's shape is right and 256 bytes hold it. Four things
before any script: a leg that gained must not be paid out beside a premium the coins do not hold (O3, high);
PO must be set against what the legs gained before the coins are asked for it (O5, as the model has it and the
paper's line has not); the band is not 17.5's (O6: the entry's part is missing, and the leaving's is at twice
the tolerance where 17.5 has it once); the fee of an option named alone is of what it is asked, not of its
underlying's worth (O8). The freeze of 17.8i's "an ended leg's row is wrong" is real and is stage 7's to close
(O9); stage 2 builds the rule as written.

## Findings

| # | Sev. | Of | What |
|---|---|---|---|
| O1 | High | (2) | the net band misses the entries' edges: F65 on a hedge, band nothing |
| O2 | Medium | (2) | "lost together" nets across markets: a gain paid out, a fee paid, beside a loss no coin covers |
| O3 | High | (1) | a leg that gained leaves alone beside a held option whose premium the coins do not hold |
| O4 | Medium | (1) | the fee of a leg alone is paid before the premiums that stay |
| O5 | Medium | (1) | every leg: the paper pays PO from the coins only; it must be set against the gain first |
| O6 | Medium | (1) | the band of an option is not 17.5's: no entry's part |
| O7 | Low | (2) | accounts nobody has a reason to step; no least fee |
| O8 | Low | (1) | the fee of an option named alone on its underlying's worth |
| O9 | Medium | (1), stage 7 | an ended leg's row wrong: a due account nobody can step while no price of the day is written |
| O10 | Low | (1) | the band with the line of nothing: an account at the level of an entry due at once |
| O11 | Low | (1) | small things to fix in the text: token legs and NS, the lines in a step, "between", "within T" |

### O1 (high; built). The band of every leg leaving covers the leaving's price and not the entries'

`c_end`, both kinds: `asked = gain - over(ra, 2g, up)`. For the price the legs leave at, that is right: the
legs of a group say one price, so a price the tolerance off moves what they gained by the net, and R of the
net by 2g over the rate is that twice (R is computed with the rate rounded up on the price, so it is never
below 2g of the net worth but by a sompi a leg; legs of one market apart are two groups and are asked both;
legs entered at different prices change nothing, their entries are not in the sum that moves). But twice the
tolerance was never of the leaving alone: F65 (line 804) is "a leg entered at 9.95 and left at 10.05 where the
committee said 10". An entry is the user's statement (`c_enter`: the leg's entry is the price stated, held to
the committee's within the tolerance), and two legs enter by two statements, each off its own way.

- The committee says 10.00 throughout. The user enters a long of 1,000 units at 9.95 (right: within 0.5 %),
  and after its quiet time a short of 1,000 next to it at 10.05 (right; the long's row says 10.05 too). Every
  leg leaves by the user's statement (kind 8), at any price: gain 100, R nothing, band nothing, **asked 100**:
  1 % of the worth, from prices alone. Before 1f89ec0 the band was 1 % of 20,000 = 200 and nothing was asked.
- The move between the two entries is hedged in a second account that enters the other way at the same two
  times (short at 10.05, long at 9.95): both accounts end as hedges with nothing asked of them, and are paid
  200 together, with no risk. Coins needed: R of one leg (5.5 %) for two statements' lives.
- The same band is in the test of a step that is due and in a step of every leg.

**Recommend.** The band of every leg leaving = the entries' part, gross, `rate_up(W, g)`, and the leaving's
part, net, `over(RA', g, up, up)`. An account of one leg pays what it pays today (to the rounding); a hedge
half of the gross band (in W3's numbers about 11.6 where the gross was 23 and the net 1.1: the 2.6 its legs
gained are under it again; the fee on R, which is most of what W3 gave back, stays). A user's own leaving of
every leg has the same. Model first: test 1 below.

### O2 (medium; built; new with 1f89ec0). What the legs that stay lost "together" nets across markets

`nls = max(0, -(gain - gl))` is one sum over every market. Within a group it is exact (the group moves as its
net). Across groups a gain of one market that is no coin stands for a loss of another, and the prices move
apart.

- **A gain paid out (E1 again).** Stays: a long of X worth 100,000 that gained 8,000; a short of Y worth
  100,000 that lost 8,000. Named: a long of Z worth 100,000 that gained 3,000. Coins 100. Due (a move of the
  lower rate against each leg loses 13,500). nls = 0, so the leg is named (before this commit: LS = 8,000,
  refused); before 13,400, after 10,900: it leaves alone; **2,000 leave for the user's key**, the fee takes the
  100. X then falls 3 %: every leg leaves at -3,000 with no coin: **the shard unpaid 3,000**. Without the
  payout the same prices leave it unpaid nothing (Z's 3,000 against them). A take refuses this account by
  16,500 and more: the step paid out what a take would not, and the user may state the step itself.
- **A fee before a loss (F73).** The same two that stay; named, a long of Z that lost 1,000; coins 1,500. The
  cap is `m + gl - nls` = 500: fee 500, coins nothing. Before: the cap was by LS, fee nothing. X falls 0.5 %:
  unpaid 500. The loss here is bounded by one fee a step; the risk is new but small.
- What remains after either fix is the old risk, not this commit's: a leg that gained beside legs that have
  lost nothing is paid out of an account that is due (as T12 decided E1); `walked` steps an account to rest at
  one set of prices, so no test moves the prices between two steps of one account.

**Recommend.** The guard of a leg that gained: `ls == 0` again (each alone), no number more; the fee's cap may
stay "together" (bounded, said here). W3's third change is then given up for accounts whose hedge's leg
"lost"; they leave whole, at the fee and the band of the net, which is where W3's money was. Exact is "lost
together, a group at a time": two numbers more a row (the open group's sum of the legs that stay; the closed
groups' losses), for the last phase. The whole class (O2, O3, the user's own step) closes when a leg that
gained in a step is **credited to the account (17.8g, C) and not paid to the key**: decide that with stage 3.

### O3 (high; paper). A leg that gained, named, leaves alone beside a premium the coins do not hold

An entry counts what the legs gained (m + G + tk at RA' + PA), so a held option's premium may stand on a
futures leg's gain and not on coins. 17.8i's line "a futures leg named: as 17.8h" then pays that gain out.

- A long of 10 units entered at 100; coins 61. At 110 (gained 100) a call is entered, held, premium 100: 161
  at 60.5 + 100, right. The price falls to 105: the long has gained 50, R is 57.75, a move of the lower rate
  loses 47.25, the band is 10.5: X is nothing, 61 is below 100: due.
- The long is named: nothing that stays lost; before 46.75, after 39: it leaves alone, **39.5 to the user's
  key**, fee 5.25, coins 55.75. Then the call (it is every leg): PO 100, the coins pay 55.75, **44.25 of the
  premium is unpaid and not owed**.
- Every leg at once: X 39.5 set against PO, the coins pay 60.5: the shard has the 100. The order is the
  stater's, and the stater may be the user: it leaves with 44.75 where it would have 0.5.

**Recommend.** A leg that gained (a futures leg, or an option by O5's arithmetic) leaves alone only where,
beside 17.8h's test, **the coins hold the premiums of what stays**: `m - owes - what it pays of its own PO >=
PS`; else every leg. Closed for good by a credit (O2). Model first: test 3.

### O4 (medium; paper). The fee of a leg alone before the premiums that stay

17.8i has "losses, then PO, then the fee" for every leg, and for a leg alone "what it lost and its PO, and
then the fee". The premiums of the legs that stay are the shard's as well, at their end. In O3's account with
the long having lost 10 and not gained: coins 61, the fee of the long (5.25) leaves 45.75 against a premium of
100. **Recommend:** the cap of a leg's fee is `m + gl - (PO paid) - nls - PS`, or nothing. A due account of
held options then pays nearly no fee by any step (see B(h)); that is right, nobody needs to step it.

### O5 (medium; paper against its model). PO is set against what the legs gained, before the coins

17.8i: "the coins pay what the legs lost together, then PO as far as they go, then the fee; what they gained
together is asked less the band". Read as written, O3's account at once: the coins pay 61 of PO and 39.5 is
asked of the shard: the shard has 21.5 where 17.3 ("set against what it gained") and `stepped_owing` (the gain
into the coins, then the premiums from them) give it 60.5. The same where a held option that gained is named
alone. **PO as one number is enough** (with G, GL, PA, PS): the payments are in the check list. It must be
said which runs out first: a loss the coins do not pay is owed or takes every leg; a premium they do not pay
is forgiven; so the coins pay the loss first.

### O6 (medium; paper). The band of an option is not 17.5's

17.5: the band is what the worth exercised changes by where a price moves the tolerance, **at its entry** and
at a leaving at a stated price. 17.8i: what it loses by a move of twice the tolerance at the price it leaves
at. For a futures leg the two are one number; for an option they are not.

- A call held, struck at 100, entered at 100 (stated; the committee said 100.5: it was worth 0.5 a unit),
  10 units. At 99 it is named: G nothing where it lost 5; 17.8i's band is nothing (it is worth nothing at 99
  and at 98). 17.5's entry's part is 5.
- The other way 17.8i takes too much: the same call at 120 pays 24 where 17.5 has 5 and 6.

**Recommend (with O1: one rule for both kinds of leg).** A leg's band where it leaves alone: its entry's part
(a futures leg: g of its worth; an option: the larger of nothing and what its worth exercised changes by, the
worse way for the shard, where its entry moves g: a number of the leg alone) and what it loses by a move of g
from a, the worse way. Every leg: the entries' parts summed, and what the legs lose together by a move of g
(two sums a group at a less and plus g of a, closed as R is). **In the row: BA keeps its place** and holds the
entries' parts and the closed groups' part; two numbers more for the open group. That is the paper's three
numbers and its 256 bytes.

**A cheaper form that is not wrong: there is one, in a step only; it is the fallback, not the choice.** R by a
ratio alone is wrong (the paper's example). **R by the ratio, and the own loss by that move of each held option
whose strike is between L and U, is not wrong where the sums are a step's** (every such option on its line of
nothing): what is left of the group's line between L and U bends downward only, and such a line loses by a
part of a move no more than that part of what it loses by the whole; the held options between the ends are
counted apart, each at its worst. One number (in BA's place), no sum more: **240 bytes**. The band of every
leg is read beside an option by a step alone, so it serves. Two cautions. It is wrong with the user's lines: a
long of one unit at 100 with a put held at 96 loses 1 by a move of 1 %, R by the line that asks least is 4,
the band 0.73; so it must never be read in a user's statement. And it asks a hedge about the gross band again:
a long with a put held at 103 loses nothing by any move down, the exact sums give a band of nothing, this
form 1.27 on 100 (at the tolerance once). Decision: the two sums (256 bytes); this form if `KpfTCRow` meets a
limit. Each leg's own band summed (BA as built) is right as well and undoes W3 for every account with an
option.

### O7 (low; built). Accounts nobody has a reason to step

- A hedge under water (a long and a short of one market, a loss locked of 100, coins 50): due at every price;
  the fee is nothing. It was nothing before as well (the coins do not pay the loss): the old case. The loss
  does not grow (R is nothing); the user ends it by its own leaving of every leg.
- New: an account nearly hedged. A long of 100 and a short of 99, due: the fee is of the net (0.5 % of one
  unit) where it was the lesser of the coins left and 0.5 % of 199. What the shard can lose is of the net as
  well, so the size is right; what is missing is a floor.
- An account that owes, with a hedge and no token: the leg named sends it to every leg (after is not below
  before), fee nothing.

**Recommend:** a least fee of a step in the terms (what a statement and its application cost at the least),
taken after the loss and the premiums as every fee is; with the rates, in the last phase. Until then the owner
of the shard is whoever steps these.

### O8 (low; paper). The fee of an option named alone

17.8i gives an option its underlying's worth in W "as the base of the fee of a leg named". A call written at
104 with the price at 100, 10 units: asked 15 and its premium, fee 5 (0.5 % of 1,000) where a futures leg
asked 55 pays 5. W3's rule ("no choice pays more than the risk it takes away") says: **the fee of an option
alone = (what it is asked alone, RL, and its premium) by the fee's rate over the rate**, as the paper already
has it for every leg. An option then adds nothing to W and WS; the ends read BL and RL for an option, and W as
built for a futures leg.

### O9 (medium; stage 7's). An ended leg's row is wrong: who is frozen

It agrees with 17.8g's second writing (an ended leg always leaves by the one transaction, a gain credited) and
not with 17.5's first (N67, "nothing is frozen"). What it freezes: from the day until the day's transaction, no
statement on the account is right. Whoever wants to step it sends the note and the day's transaction first;
that works while the committee answers. **Where no price of the day is written, a due account is stepped by
nobody until the fallback's time** ("long"), and its other legs go on losing. Decide before stage 7's scripts:
in a step only, an ended leg whose day has no note yet is passed by a row that adds its premium and nothing
else (what the fallback will make of it). And say whether the day's transaction may be sent while a statement
stands (it changes the legs' chain: anyone could undo a step so; recommended: not while one stands). Stage 2
builds "wrong where the day is at or before the statement's time" and tests it with objects.

### O10 (low; paper). The band's line in a step, and the level of an entry

The band's sums are read by a step alone (a user's leaving of every leg is wrong beside an option), so a held
option between the band's ends is on its line of nothing there: safe, never less than the true. It asks more
of a hedge: a short of one unit at 100 with a call held at 99.5 is asked a band of 1.5 where the true is
nothing. Since X is not taken below nothing, an account at the level of a take is still due at no price. An
account at the level of an entry by what it gained (O3's kind) is due that much sooner: 17.8g's M1 ("a band
sooner") with the option's worth added. Accept, and count it in test 8.

### O11 (low; paper). To be said in the text

- NS: a token leg that stays adds nothing (it counts as coins); a futures leg and a written option one.
- "Between": the strike above L and below U, both strictly (the model's). "Within T": the day less the
  statement's time no more than T (say which side the equal falls; the day at or before the time is wrong).
- The lines: the row computes line nothing in a step whatever the head says; in every other kind the bit of a
  leg that is no held option between the ends is not read. `c_head_held` does not hold the lines: right.
- A sale of tokens (15) is anyone's and tests what stays: any line is safe (both lie below), so it reads the
  head's bits as a user's statement does.
- v(p) from two worths each rounded down is a sompi off either way: say which way each use rounds (R up).

## The questions, briefly

**A(a)** The leaving's edge: yes, in every case asked (two groups of one market are asked both; entries do not
matter; the rounding is to the shard's side but a sompi a leg). The entries' edges: no (O1). A user's own
leaving of every leg with every price the tolerance in its favour takes nothing of the leaving and 2g of the
gross of the entries. **A(b)** O2: new, not the old risk; F73 holds at the prices stated and fails by one fee
when they move apart; E1 fails by the whole gain paid out. **A(c)** Yes: the user states the step itself and is
paid a gain a take refuses (the take's test counts no gain of a leg that stays; the step's "lacks" counts
them). The price is the leg's band and that the account is due; the gain is below the R the leg frees. **A(d)**
O7.

**B(a)** The paper has and the model lacks: a band for an option, any fee, NS, W of an option, the line of
nothing in a step (`lacks` takes the least lines), several markets, what is owed with a fee. The model has and
the paper lacks: the gain before the coins (O5). `due_stepped` takes the band of the futures legs' gross
worth: neither W3's nor 17.8i's. PO is the right number. **B(b)** O6, O10. **B(c)** A put held against a long:
far from its day the account is due only where it cannot pay at some price, and near its day (N68) the long
named leaves alone where the coins pay it or every leg leaves at the net; the user keeps the put and loses the
fee: no harm beyond 17.4's own. A spread: the written call is the one that can be named alone, the held one
never beside it: right. A futures leg and a held option on a gain: O3. Stripping by order: naming the futures
leg of a hedge first is always "lacks less" (the held option gives no credit before or after); it takes no
value from the user but the fee and the realized loss, and the account was due by one price. **"Never named
alone" is right for the shard and simpler** (no NS, one branch less); it ends every option of an account of
held options for one that lost. Decision: keep E13's rule and NS; test 11 says what it is worth. **B(d)** O9.
The dispute period across the day is covered by T (it is twice a lateness and a dispute period): compare with
the statement's time alone (`KpfTCRow.answer` is given it). An option named by a step stated before its day
leaves at the step's price though it is applied after: allowed by 17.5 (a step). **B(e)** With a futures leg
and an option the user can take, mark the futures leg, and leave it where what stays stands at RS' + PS + LS;
it cannot end the account or the option before the day but by a split and a hand-over, or by entering the
opposite option (both premiums stay asked). With options only: a take and a split. Right by 17.5; say it
beside 14.7's lines. **B(f)** Stands: the premium moves with the leg and is asked in the account that holds
it; the moved account is tested with the option alone (RL, its premium, what it lost). What 17.3 warns of is
two accounts, not the split. **B(g)** An account of held options that entered out of the money is due exactly
where its coins are below its premiums (G and M are never below nothing); after a take never. A written
option that lost more than its premium is due while it could still pay the larger of the two: by design (the
premium is no margin). Near the day M counts losses only: a hedge is due sooner (N68). **B(h)** The rate is
consistent (9.1 % of what is asked, as a futures leg's). By the order of payment a due account of held options
has for a fee only what is above its premiums: nearly nothing. Right.

**C** Yes: `KpfTCRow` for the four ways (the two sums, the lines, near the day, M, PO, NS, the band), the ends
7, 8, 9, 12, 13, 14, 15 and an entry of a futures leg beside options are all tested with accounts made as
objects (`leg_more` gives the 32 bytes). Not testable so: that the premium is in the account (the owner's
coins at an entry; "unapplied, it returns"); no entry within T; the place of an entry (E2); the day's
transaction and O9; **what the shard does with a note that names an option leg or carries a premium** (a
step's note is of legs and of what is asked: the premium paid is coins beside it; the dated rows that take it
do not exist). So the note's shape for an option is decided now or `KpfTCEnd` is opened again in stage 7.

## Check list to code from

**The leg.** As a futures leg, with its way 3 a call held, 4 a call written, 5 a put held, 6 a put written
(check they do not meet the token leg's), and its 32 bytes `leg_more(time, day, strike, premium)`, 8 bytes
each. Right: 0 < strike <= the market's limit; day > time; premium >= 0; the worth at the strike and the
premium by the units fit. P = `worth(size, scale, premium)`. ex(p) = worth(p) - worth(strike) for a call, the
reverse for a put. v(p) = max(0, ex(p)) held, -max(0, ex(p)) written.

**The row: 256 bytes, 24 numbers of 8 and two chains.** Places 0 to 20 as built (`numbers()`):

| 0 a | 1 gain | 2 w | 3 ws | 4 ls | 5 gl | 6 tk | 7 q |
|---|---|---|---|---|---|---|---|
| 8 m | 9 ba | 10 bl | 11 dda | 12 dua | 13 ra | 14 dds | 15 dus |
| 16 rs | 17 rl | 18 pa | 19 ps | 20 po | **21 ddb** | **22 dub** | **23 ns** |

Bytes 8i to 8i + 8; **g at 192 to 224, h at 224 to 256**. ba (9): the entries' parts of all the legs and the
closed groups' leaving part. ddb, dub: the open group's two sums at a less and plus `rate_up(a, tolerance)`.

**Wrong, before anything** (an option leg): day <= t0; in kind 8 it leaves (named, or every leg); in kind 13
it is the leg marked; strike out of range. near = day - t0 <= T. between(lo, up) = held and lo < strike < up.

**What an option leg adds** (d = v(a) - v(entry); line 0 = (-v(a), -v(a)), line 1 = (ex(L) - v(a), ex(U) -
v(a)) signed for a call, the reverse for a put; E = its entry's part, O6):

| Number | Stays | Leaves (kind 9: named or every leg; kind 12: the leg moved) |
|---|---|---|
| gain | + d (not in kind 13) | + d |
| w, ws | nothing (O8) | nothing |
| ls | + max(0, -d) | |
| gl | | + d |
| m | + (v(q) - v(a)), near: no more than 0 | the same |
| dda, dua | + (dl, du): exact, or by its line where between(L, U); line 0 in kind 9, else the head's bit; near: each no more than 0 | the same |
| dds, dus | the same | |
| rl | | + lost(dl, du) of the leg alone |
| ba | + E | + E |
| ddb, dub | + exact at the band's ends, line 0 where between them | the same |
| bl | | + E + max(0, v(a) - v at either band's end) |
| pa | + P | + P |
| ps | + P | |
| po | | held: + P; written: + max(0, P - max(0, -d)) |
| ns | held: nothing; written: + 1 (a futures leg + 1, a token leg nothing) | |
| g, h | both | g alone |

A group closes as built, and with it ba += lost(ddb, dub), the two set to nothing. By kind: 7, 10, 13, 14, 15:
every option stays; 8: stays, or the row is wrong; 9: either; 12: either. A futures leg adds to ba
`rate_up(w, g)`, to ddb and dub what it gains to the band's ends, to bl `rate_up(w, 2g)` as built.

**The ends** (m the coins less a note's least; e = m + tk - owes; RA', RS' closed; band = ba + lost(ddb, dub);
PL = pa - ps; up = the rate):

- Entry (10): m + gain + tk >= RA' + pa.
- Take (7): m - amount >= 0 and m - amount + tk >= RA' + pa + ls.
- A futures leg leaves (8): gained: m + tk >= RS' + ps + ls; lost: m + gl >= 0 and m + gain + tk >= RS' + ps.
- Mark (13), tokens (14, 15): as built, with pa beside RA' and ps beside RS'.
- Split (12): stays: m - amount + tk >= RS' + ps + ls; moves: amount >= rl + PL + max(0, -gl) (and a note's
  least, as built).
- Step due: sx = gain + m(8); where sx >= 0, sx = max(0, sx - band); due where e + sx < pa.
- Step, a held option named and ns of the last row not nothing: every leg.
- Step, the leg named lost (gl < 0): pay = -gl; prem = min(po, max(0, m - pay)); before = max(0, RA' + pa - e
  - gain); after = max(0, RS' + ps - (e + gain - prem)); alone where a token is held (owing as built) or
  (after < before and m >= pay); fee = min(over(rl + PL, liq, up, down) for an option or `rate_down(wl, liq)`
  for a futures leg, max(0, m - pay - prem - nls - ps)).
- Step, the leg named gained (gl >= 0): right only where ls = 0 (O2); x = max(0, gl - bl); set = min(po, x);
  prem = min(po - set, max(0, m)); asked = x - set, less what is owed as built; alone where a token is held, or
  (after < before and m - owes - prem >= ps (O3)), with after = max(0, RS' + ps - (m - owes - prem + gain -
  gl)); the fee as above, no more than m - prem - ps.
- Step, every leg (never with a token held): room = max(0, m); gain < 0: pay = min(-gain, room), x = 0; else
  pay = 0, x = max(0, gain - band); set = min(po, x); prem = min(po - set, room - pay); asked = -pay, or x -
  set less what is owed; fee = min(over(RA' + pa, liq, up, down), room - pay - prem); the user is paid held -
  pay - prem - fee; the note holds its least, pay and prem. What of po is neither set nor paid is not owed.
- A user's leaving of every leg (8; futures only): asked = max(0, gain - band), band as above (O1).

## Model tests to write before any script

1. With every entry and every leaving stated the tolerance off for the user, in one account and across two, a
   hedge entered in two statements is paid no more than at the committee's prices by the band of the entries'
   and the leaving's parts, and is paid 2g of its worth by the band on R alone.
2. With the prices moving the lower rate between two steps of one account and the stater taking the step that
   pays it most, the shard is unpaid nothing where a leg that gained is named only beside legs that each lost
   nothing, and is unpaid by "lost together" across markets (how often, how much).
3. An account whose held option's premium stands on a futures leg's gain is paid to the shard the same
   whichever leg is named first, with the guard of O3, and less without it.
4. A step with every held option on its line of nothing and E13's rule, against the lines that ask least: how
   many steps part, the shard unpaid by none, N71 as it was (the paper's own).
5. A held option within the tolerance of its strike, entered or left at a price the tolerance off, gains
   nothing by it under the band; the ratio forms of O6 are shown wrong by the long with a put 4 % below.
6. The walk of 17.8, 1 with premiums, the fee on R and the premiums, and the order loss, premium, fee: the
   shard unpaid nothing, no fee before a premium (O4), whoever steps.
7. The payments of a leg alone and of every leg, computed from the row's sums (gain, gl, po, pa, ps), equal
   `stepped_owing` leg by leg on random accounts, the gain set against PO before the coins.
8. An account at the level of a take is due at no price, far from and near its day, with the band in X; an
   account at the level of an entry by what it gained is due how much sooner (M1, with O10).
9. A statement stated just outside T of a day and applied after it leaves the shard paid: T covers the life.
10. W3's test with options: no choice of the leg pays the stater more than the risk it takes away, the fee of
    an option alone by what it is asked (O8).
11. An account of held options only, one that lost: an option at a time (NS) against every leg ("never named
    alone"): what the user keeps, the shard unpaid by neither.
12. The row's sums for the four ways (the two sums with the lines and near the day, rl, po, ns, ba, ddb, dub)
    equal a direct count on 200,000 accounts of mixed legs of several markets: the model of `crow.rs` first.
13. A split that moves a written option: each account stands alone, the premium asked in the one that holds
    the leg, and the two together are asked no less than the one was.

## Decisions on 17.8i's "To be said by the review"

| Point | Decision |
|---|---|
| an ended leg's row wrong: a freeze | build as written; the freeze is real where no price of the day is written (O9): decide before stage 7 |
| the day and T across a dispute period | the statement's time alone; T covers the life; a step stated before the day stands |
| no leaving names an option | stands (17.5); the user's ways out are a split and a hand-over, the opposite option, the day; say it to users |
| a written option that gained, named | the branch of a leg that gained: PO is its premium whole, set against what it gained less its band; O3's guard |
| a written option's premium in a split | stands; tested alone at RL, its premium and what it lost |
| PO as one sum | enough; the order is loss, then PO (set against the gain first), then the fee (O5) |
| the band of a held option alone | not 17.5's: the entry's part and the leaving's at the tolerance once (O6) |
| the fee on R and the premiums | stands for every leg; a leg alone by what it is asked (O8); capped after the premiums that stay (O4) |
| E13 and a put against a long | stands; keep NS; test 11 for the simpler rule |
| 256 bytes | stands, with ba kept in its place and two numbers for the band's open group (240 by O6's fallback form); what `KpfTCRow` weighs was not estimated (its body was not read) |

## When

**Before any script:** O1's band (model, then `KpfTCEnd`: it is built and wrong now); O2's guard back to
`ls == 0`; O3's guard and O4's cap; O5's order; O6's band and the row's places above; O8's base; the note's
shape for an option leg and a premium paid (C); tests 1 to 8 and 12.

**While building:** O11; tests 9 to 11 and 13; the rounding of v.

**Later, said now:** O7's least fee (the last phase); O9 (before stage 7); a gain credited in a step (stage 3:
closes O2 and O3 at the root); "lost together" a group at a time (two numbers, the last phase).
