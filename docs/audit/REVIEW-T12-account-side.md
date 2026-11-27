# Review T12: the account's side, how it is built (17.8h)

27 November 2026. A review of a design on paper. Nothing was run (no cargo, no test, no network): every number
below is worked by hand from the scripts and the model as they read.

**Read whole:** 17.8h, 17.2, 17.3, 17.4, 17.8g D; `KpfTCRow.sil`; `KpfTCEnd.sil` (header, 128 to 367).
**Read in part:** `KpfTCEnter` (header, 118 to 184), `KpfTCSplit` (146 to 186), `KpfTCMark` (150 to 190),
`KpfTCTok` (176 to 212), `KpfTEnterS` (150 to 200, 279), `KpfTStepPrice` (150 to 200), `KpfTChange` (180 to 205),
16.18 (its table and "step 4"), `research_options.rs` (1 to 150, 251 to 312, 1360 to 1640).
**By grep only (inferred):** `KpfTCStep`, `KpfTCallEnter`, `KpfTCallPay`, `KpfTEnterOff`, `KpfTDrawNote`,
`covered.rs` (its outline). Sections 12 and 13 (the shard's requirement) were not read.

## Verdict

**Change first, on paper and in the model; then build stage 1. The row stands.** The twenty numbers, the
open and closed group, "the leg before" shown in the answer, the head held by the end, and the one commit for
futures and tokens are sound. Five things are not ready:

1. **E1 (high):** "lacks less after than before" lets a step name a leg that **gained**, and the gain goes out to
   the user's key while a loss stays unpaid. The model does not show it, because the model pays the gain **into**
   the account. Keep today's guard in stages 1 and 2.
2. **E2 (high):** the entry's place is not a matter of the rows alone. `KpfTEnterS` writes the marked account's
   chain itself (the leg appended) and `KpfTEnterOff` opens it to undo. Build stage 1 with the leg last; the
   place is a step of its own.
3. **E3 (medium):** the step's "what is owed then" makes an account with no token owe. Say whether that is meant.
   For stage 1: no.
4. **E4 (medium):** the model has one underlying, no fee, no debt in `lacks`. What stage 1 builds (groups that
   close, the fee, what is owed) is not in it. Model first.
5. **E5 (medium):** stage 2 needs one number the row does not have (what the options that leave pay of their
   premiums, in a step of every leg). Add it now or the row changes twice.

## Findings

### E1 (high). A step names a leg that gained: the gain leaves, the loss is not paid

Today (`KpfTCEnd` 268 to 270): a leg that gained, named while a leg that stays lost, is wrong. 17.8h drops this
for "lacks less". For a leg of a market of its own, what is asked falls by the whole rate on its worth, so the
test passes whenever GL + fee < RA' - RS' + what is set off.

- Rates: upper 5.5 %, lower 4.5 %, tolerance 0.5 %, a step's fee 0.5 % (assumed). Long A worth 100,000, gained
  3,000. Short B worth 100,000, lost 8,000. m = 5,000, owes 0, no token.
- Due: G = -5,000, M = -9,000 (each at its worse q), m + X = -9,000 < 0.
- The stater (the user's second key) names A. Before: 11,000 - 0 = 11,000. After: 5,500 - (4,500 - 8,000) =
  9,000. Less: "right". The note asks the shard 3,000 less the band 1,000 = 2,000, **for the user's key**
  (`KpfTCEnd` 342, read by `KpfTDrawNote` 99: a key and an amount). The fee 500 goes to the stater.
- Then B: lost 8,000, coins 4,500. The shard is short 3,500 on B and paid 2,000 on A. Every leg at once: G =
  -5,000, the coins pay it whole. The user took 2,500 out of an account worth nothing.

The model's `stepped_owing` (line 1499) has `coins = a.coins + asks - set_off`: the gain stays as coins. The
built form pays it out. 17.8h's "after" is written as built (it does not count the gain), so the formula is
consistent, and the rule is what fails: "lacks" measures the distance to an entry's level, not what the shard
can lose.

**Recommendation.** In a step, a named leg with GL >= 0 is **wrong** where LS != 0, and where LS = 0 it is as
today (alone with tokens, else every leg). "Lacks less" is asked of a leg that lost only. Revisit with 17.8g C
(a gain credited to the account): then the model's form is the built one.

### E2 (high). The entry's place: who writes the marked account's chain

17.8h says only what the rows do. But:

- `KpfTEnterS` 279 writes the marked account with `blake2b(root + lg)`: the leg at the end. With a place k < n it
  cannot compute the chain without the legs after k.
- `KpfTEnterOff` 125 opens the marked root as `blake2b(root + leg)` to get back the account without the leg.
- If the transaction took the new root as stated, a false root would stand in the marked account until the
  entry ends. The call's step reads an account's legs by their chain (inferred from `KpfTCStep` 97 to 148 and
  16.18): an account whose root opens to nothing cannot be stepped by a call.

So the place needs one of: (a) `KpfTEnterS` and `KpfTEnterOff` are shown the chain before the place and the legs
after it (up to five legs, 555 bytes, a loop in both), and compute both roots; or (b) the marked account keeps
the old root, and every reader of a marked account learns it.

The rows themselves are sound: with g_i = H(g_{i-1} + leg_i) on every row, h unchanged at row k only, row k's leg
the leg said at its entry, 1 <= k <= n held by the end, and h_n the old root, the new list is the old one with
one leg put in. What is missing is where the old and the new root come from.

**Recommendation.** Stage 1 keeps "the leg that enters is the last" (a hedge entered right after what it hedges
is netted, which is the common case). Reserve the place in an entry's head now (8 bytes, held to n by
`KpfTCEnter`), so that `said` changes once. The place is a commit of its own, by (a). The row does not depend on
it.

### E3 (medium). An account with no token that owes

"After: ... less what is owed then" reads as: a leg whose loss the coins do not pay leaves alone and the account
owes the rest, tokens or not. Today that is only with tokens (`KpfTCEnd` 266); without, every leg leaves.

- Long A worth 100,000 gained 20,000; short B worth 100,000 lost 14,500; m = 1,000. Due: G = 5,500, M = -9,000,
  so X = -3,500 and m + X < 0. A step names B: before 11,000 - 6,500 = 4,500, after nothing: "right". The coins
  pay 1,000 and the account owes 13,500.
- What stays: X = 20,000 - 4,500 - 1,000 = 14,500, and -13,500 + 14,500 is not below nothing: not due.
- The account owes, so it takes a step and nothing else (`KpfTCEnd` 196), and no step is due. The user's way on
  is the payment of 16.18 from outside coins. The shard holds a claim of 13,500 that only an unrealized gain
  backs. Today: every leg leaves, G = +5,500, all paid.

What is owed does not break "lacks less": for a leg that lost, equity after is equity before less the fee,
whether the coins pay or the account owes (m - pay - fee + tk - (owes - GL - pay) + G - GL). So the test reduces
to RA' - RS' > fee. That is right, and it is why the rule must not be the only guard.

**Recommendation.** Stage 1: where the coins do not pay what the leg lost and no token is held, every leg leaves,
as today. Stage 2 needs the owing form for N71 (the held option goes on): decide it there, with the model
carrying what is owed inside `lacks`.

### E4 (medium). The model does not cover what stage 1 builds

- `Acct` is "legs of one underlying": no group closes in the model. RA, RS over several markets, and E6's gap,
  are untested.
- `stepped` has no fee. Q5(d)'s fee cases are not in it.
- `lacks` has no debt and no token: 17.8h's has both. The model's "after" is smaller than the paper's when
  something is unpaid, so the model accepts steps the paper refuses.
- The gain of a leg that leaves is paid in (E1).
- LS is in no test of the model but `stands` of the day's test.

**Recommendation.** Before any script: accounts of two and three markets in the model (the row's sums as the
script will compute them, against the sum of `asked_as` over groups); `lacks` as 17.8h writes it; a fee; the gain
paid out. Then E1's scenario as a test that fails without the guard.

### E5 (medium). A number is missing for stage 2

17.3: in a step a written option pays "the larger of the premium and what it lost". The model:
`owed += (premium - lost).max(0)` a leg (line 1491). With one leg leaving the end has it (PA - PS and GL). In a
step of **every** leg it is a sum of a maximum a leg, which no sum of the row gives.

**Recommendation.** A twenty-first number, PO: what the option legs that leave pay of their premiums (held: the
premium; written: what the premium is above what it lost). 232 bytes. Or RL is said to carry it in a step (a
split is never a step): fewer bytes, one more "by kind" rule. I would add PO.

### E6 (medium, accepted). The sums of what stays across a gap

A group closes "at a row whose leg's market is not the leg before's", for both triples. Checked against the sum
of `asked_as` over groups:

- All legs: exact.
- What stays, the leaving leg in the middle of a group (A, A leaves, A): no close, exact.
- The leaving leg first or last of a group, or a whole group between two markets (A, B leaves, C): exact.
- **A whole group between two groups of one market (A long, B leaves, A short):** the account that stays has
  A long and A short next to one another, netted. RS has them apart: it asks more, never less. A token leg of
  another market between two legs does the same.

Asks more in that statement only; the next statement nets them. Exact would need the last leg that **stays**
shown (the second chain of the row before, opened), not the leg before. **Decision: accept, and say it in 17.8h.**

### E7 (medium, usability). A hedge is left one leg at a time, or with every leg

Long and short A, 100,000 each from one price; coins 1,000 after a take; the price rises 10 %: the short lost
10,000, each is worth 110,000, G = 0.

- A take: m at LS: no.
- The short leaves (lost): the coins pay 1,000 of 10,000: wrong.
- The long leaves (gained): m at RS' + LS = 6,050 + 10,000: wrong.
- A mark of the long: m at RA' + LS = 10,000: wrong.
- A step: never due (X is about nothing). Right: the pair loses nothing.
- **Every leg leaves:** right, and coins added first (`KpfTAddC`, by 16.19: not read) open the others.

So the account is safe and frozen, and the exits are "every leg" or coins from outside. With a second market in
the account, "every leg" closes that too. This is 17.4's "netting is of what is held, never of what leaves".

**Decision asked (before scripts, because of the row):** a leaving of a group (several legs named, netted as
they leave) needs a third chain in the row (32 bytes) and a note of several legs on the shard's side. I would
not build it now and not reserve it: the exits exist, and a row grown later by one chain is a mechanical change.
Say the limit in 17.8h.

### E8 (medium, for the last phase). The fee and the band are of the gross worth

- Long 100,000 and short 90,000 of one market. Asked 550. Equity 400, a move of the lower rate is 450: due.
  Naming either leg alone leaves a naked leg: lacks more. Every leg leaves: the fee is 0.5 % of 190,000 = 950,
  capped at the coins. The user loses all of it for a net exposure of 10,000.
- The stater chooses the leg it names. Naming a hedge's leg (alone: worse) turns the step to every leg, for the
  fee of W, where another leg alone would have cured the account. E1's guard narrows it (a leg that gained is
  wrong, not "every leg"); a losing leg of a hedge still does it.
- The band of every leg leaving is twice the tolerance of each leg's worth, though a netted pair at one price
  has no error of price in its net.
- Leg after leg for fees: the fees sum to no more than the fee of W. No farming beyond today's.
- 17.8h drops "the coins pay the fee and LS" (review 47: no fee before a loss). Keep it as a cap:
  fee <= max(0, m - pay - LS).

**Recommendation.** Record; rates and bases are the last phase. Build the cap now.

### E9 (low). The head: say the tolerance, not the lower rate

The row needs twice the tolerance for BA and BL, and the head has only the rate and the lower rate. 2g = rate -
lower holds only where the maintenance is at least the tolerance (`KpfTCEnd` 253: the lower rate is cut at
nothing). **Head: the rate, the tolerance, T.** The row derives lower = max(rate - 2g, 0).

The end holds, each of the five ends (`KpfTCEnd`, `KpfTCEnter`, `KpfTCSplit`, `KpfTCMark`, `KpfTCTok`):

- rate = terms[10..18] + terms[19..27];
- tolerance = terms[19..27];
- T = 2 * (moving[94..102] + moving[103..111]) (as `KpfTCStep` 108).

**The order of disputes is safe.** A row right under a false head leaves the end wrong, and any answer may show
any point wrong (`at == point || verdict == 0`). It is the class of "the last chain is not the account's" today.

### E10 (low). The rounding: the bound

A leg's loss at an end is a difference of two worths rounded down: at least floor(size * d / scale), d the rate
on a rounded up. Today: the rate rounded up on W. So:

- **less than today by at most 1 sompi a leg** (n legs: n sompi; six at most);
- more than today by at most one step of the price in the leg's worth (size / scale) and a sompi.

Accept. The note's least (0.2 KAS) is outside every test.

### E11 (low). The leg before, in the answer

- Sound. H(g_{i-2} + legB) = g_{i-1} has one opening. It must be a `require`, not "wrong": an answerer that
  shows a false leg would otherwise make an honest row wrong.
- Row 1: no leg before, no opening. Row 2: the chain two rows before is 32 zero bytes.
- The row before carries the seed, or does not continue row i-2: row i cannot be answered; the row before is
  answered wrong instead (any point may be).
- Better than a market in the row: 111 bytes in a dispute against 32 a row always.
- Two rows of one market **not** next to one another with two prices: harmless. They are not netted, each price
  is held to the committee's by its own point, and it is what every pair of rows is today.

### E12 (low). Scripts the order does not name

`KpfTChange` 192 to 199 (`at0 = 48`, `rl = 120`) and `KpfTEnterS` 160 to 161 (239, 127, 120) hold the shape of
what is said. `KpfTStepPrice` 164 to 169 (`rl`, the first chain's offset). `covered.rs` `Row`, `c_row_after`,
`c_row_marked`, `c_row_judged`, `CSaid`, `CEnterSaid`. `KpfTChangeSettle` and `KpfTDrawEntry` were not checked.

**The call:** `KpfTCStep` reads legs by their chain and a leg's time (bytes 34 to 42), no row. `KpfTCallEnter`
and `KpfTCallPay` name only the shard's rows. Nothing of the call reads an account's row (by grep).

### E13 (stage 2). The lines in a step: flat is safe for the shard, and strips a hedge

Long 10 at 9,300 and a put held at 9,500 (worth 2,000 now), ends 8,788 and 9,812.

- By the line "exercised" the put is a short: asked nothing. By the flat line: 7,120.
- Flat, both sides, the put named: after is 5,120 + fee, before 7,120: "lacks less". By the lines that ask least
  the account lacked nothing but its premium and now lacks 5,120: more.
- Naming the long is right under flat as well. The stater chooses, and a hostile one takes the hedge first.

N71 itself holds under flat (a put at 90, the price 93: the long leaves, the put stays on its premium, lacks
nothing).

**Decision.** Flat in a step, with one rule more: **a held option is named alone only where every leg that
stays is a held option**; else wrong. The model must show before stage 2: (i) steps right by flat and wrong by
`asked_group` (expected: some); (ii) none with the rule; (iii) N71 under flat; (iv) the shard's net against
every leg at once, within the bands, with the gain paid out.

### E14 (low). Notes for the scripts

- G - GL is "what the legs that stay gained" in kinds 8, 9, 12 only. In a mark G is the marked leg's rest; in
  14 and 15 GL is a token leg's worth.
- A mark keeps the marked leg out of the sums of what stays: its end must read RA, as the table says.
- U is up to twice the market's limit: check the range of six worths at U.
- Stage 2: an account due by X with "before" at nothing cannot leave one leg; with a token it cannot leave
  every leg either.

## Decisions on "To be said by the review"

| Point | Decision |
|---|---|
| Each of the twenty needed, one missing | All needed across kinds. Missing: PO (E5). q, M and the triple of all legs are dead outside a step and a mark: last phase |
| The leg before, against the market in the row | The leg before (E11) |
| The lines in a step | Flat, with the rule of E13; model first |
| "Lacks less" with what is owed, played by a stater | Played by naming a leg that gained (E1). What is owed does not break it (E3). The fee: no farming (E8) |
| A leg that lost, held to an entry's level | Right. The user adds coins or leaves every leg (E7) |
| A split's leg alone at RL | Right, and RL is for nothing else |
| The rounding; one leg never asked less | Less by 1 sompi a leg at most (E10). Accept |
| The place of an entry and the two chains | The rows are sound; the transaction and the undoing are not written (E2). Stage 1: last |
| What `KpfTStepPrice` answers for q | Nothing. Right: q is held to a by the row, a by the citation |
| What of the call reads an account's row | Nothing (E12, by grep) |

## Check list to code from (stage 1)

**The head.** A statement of kinds 7 to 9, 12 to 15: amount [0,8), the leg named [8,16), its chain [16,48),
the rate [48,56), the tolerance [56,64), T [64,72), the lines [72,80); rows from 80. An entry: coins added
[0,8), the shard's row [8,16), the leg [16,127), the rate [127,135), the tolerance [135,143), T [143,151),
the lines [151,159), the place [159,167) (stage 1: n); rows from 167.

**The row, 224 bytes** (today's seven keep their place):

| Bytes | | Bytes | |
|---|---|---|---|
| 0 | a | 88 | DdA |
| 8 | G | 96 | DuA |
| 16 | W | 104 | RA |
| 24 | WS | 112 | DdS |
| 32 | LS | 120 | DuS |
| 40 | GL | 128 | RS |
| 48 | TK | 136 | RL |
| 56 | q | 144 | PA (stage 1: 0) |
| 64 | M | 152 | PS (stage 1: 0) |
| 72 | BA | 160 to 192 | the first chain |
| 80 | BL | 192 to 224 | the second chain |

(With PO: at 160, the chains at 168 and 200, 232 bytes.)

**A row, before its leg.** lower = max(rate - 2 * tolerance, 0). a > 0, a <= the leg's limit.
d = rate_up(a, rate); L = max(a - d, 0); U = a + d. Kind 9: |q - a| <= rate_up(a, lower), q >= 0. Any other
kind: q = a. Else wrong.

**The group.** Row 1: the sums before are nothing. Row i > 1, the leg before shown by
`require(H(g_{i-2} + legB) == g_{i-1})`:

- another market: RA += max(0, -DdA, -DuA), DdA = DuA = 0; the same for the triple of what stays;
- the same market: a and q are the row before's, or wrong.

**The leg's numbers.** s = +1 long, -1 short; w(p) = quoteAt(size, scale, p).
d = s(w(a) - w(entry)); dl = s(w(L) - w(a)); du = s(w(U) - w(a)); mq = s(w(q) - w(a));
b = rate_up(w(a), 2 * tolerance).

| The leg | G | W | WS | LS | GL | TK | M | BA | BL | A sums | S sums | RL | second chain |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| futures, stays | d (a mark: 0) | w | w | max(0,-d) | 0 | 0 | mq | b | 0 | dl, du | dl, du | 0 | with the leg |
| futures, leaves (8, 9, 12; -1) | d | w | 0 | 0 | d | 0 | mq | b | b | dl, du | 0 | max(0,-dl,-du) | as it is |
| futures, marked (13) | s(w(a)-w(ne)) | w | 0 | 0 | s(w(ne)-w(e)) | 0 | 0 | b | b | dl, du | 0 | 0 | with the leg at ne |
| futures, enters (10, at the place) | 0 (entry = a) | w | w | 0 | 0 | 0 | 0 | b | 0 | dl, du | dl, du | 0 | as it is |
| token, stays | 0 | 0 | 0 | 0 | 0 | as today | 0 | 0 | 0 | 0 | 0 | 0 | with the leg |
| token, leaves (14, 15) | 0 | 0 | 0 | 0 | its worth | 0 | 0 | 0 | 0 | 0 | 0 | 0 | as it is |

A token leg closes a group by its market as any leg does. Every other check of today's row stays (the leg
named by its chain, the kinds a token leg takes, the entry's leg said).

**The ends.** R' = R + max(0, -Dd, -Du) of the last row. m = coins - the note's least. E = m + tk - owes.
Each end first holds the head (E9) and, an entry, 1 <= place <= n.

| | One line |
|---|---|
| 10 | m + G + tk >= RA' + PA |
| 7 | m - amount >= 0 and m - amount + tk >= RA' + PA + LS |
| 8, one leg, GL >= 0 | m + tk >= RS' + PS + LS |
| 8, one leg, GL < 0 | m + GL >= 0 and m + G + tk >= RS' + PS |
| 8, every leg | as today |
| 12 | coins - amount - least >= 0; that + tk >= RS' + PS + LS; amount - least >= RL + (PA - PS) + max(0, -GL) |
| 13 | GL > 0; G >= BL; m + tk >= RA' + PA + LS |
| 14 | m + tk >= RS' + PS + LS |
| 15 | as today (no test of the rate) |
| 9, due | S = G + M; X = S if S < 0, else max(S - BA, 0); due where E + X < PA |
| 9, the leg named, GL >= 0 | wrong where LS != 0; else as today (alone with tokens; else every leg) |
| 9, the leg named, GL < 0 | fee = rate_down(W - WS, liq), at most max(0, m + GL - LS); before = max(0, RA' + PA - E - G); after = max(0, RS' + PS - (E + G - fee)); alone where after < before and (m + GL >= 0 or tk != 0, owing as 16.18); else every leg (wrong while tk != 0) |
| 9, every leg | as today; the band BA where it is the rate on W today |

Bands: BL where today's is of W - WS; BA where it is of W.

## Decide before any script

1. E1: the guard on a leg that gained (yes).
2. E3: no debt without tokens in stage 1 (yes).
3. E2: stage 1 with the leg last, the place reserved in the head (yes).
4. E5: PO in the row now (yes, 232 bytes), or RL by kind.
5. E9: the head says the tolerance.
6. E7: a leaving of a group: not now, not reserved; say the limit.
7. E4: the model's additions, run before `KpfTCRow` is touched.

## While building

E6 (say it in 17.8h), E8's cap of the fee, E10, E11 (the `require`), E12 (the list of scripts), E14.
Stage 2: E13 and E5's use.
