# KPF committee markets: prices signed by a staked roster (design note and build log)

Status: stage 1 is built: the contracts and their engine tests (`roster_tests`, `committee_tests`,
`committee_paths_tests`, `c_experiments_tests`), the pinned artifacts (`contracts/legacy/artifacts/committee/`), the builders
(`committee_build.rs`), the commands (`roster-*`, `tick-*`, `c-*`; `committee_cli`), seven review rounds with their fixes
(`audit/REVIEW-C1.md` to `REVIEW-C7.md`, section 9; the fourth found nothing above a note for the price path, the
seventh for the slashes) and a live run on testnet-10
(`LIVE-3.md`). Of stage 2 (section 5a) both slashes are built: a seat that signs two prices for one tick, by the two signatures,
and a seat that signs a false price, by the verdict of a judge's stake. Pools and fees are designed, not built. This note is the specification the contracts of the committee family are written and
reviewed against; a section says "built" only when its tests exist. It replaces the price source of the oracle-market
family (`DESIGN-ORACLE-2.md`, `DESIGN-ORACLE-IMPL.md`, cited `I§n`); the pair as a price range, the orders and the
two-step claim are kept. `D§n` is `DESIGN.md`.

## 0. Why, and what changes

Review 12 found no broken covenant rule in the oracle-market family and a broken economy around it: a price there is
one party's statement that stands unless somebody contests it within a window, so a false price is a winning bet below
a contest probability of 80 to 90 %, and the shipped tools make that probability zero. More covenant rules do not fix
that. The founder's decisions of 2026-11-20:

1. A price is valid only when a committee has signed it: anyone may bond KAS and sign, a price needs the signatures of
   at least 67 % of the bonded weight, and without them nothing happens (fail closed). No party states a price, so no
   contest window protects one, and the underwriter (slot, cover, premium) is gone.
2. A user's speed comes from an executor that takes the user's side over in one transaction and settles later; the
   committee path may take seconds.
3. Lies are judged afterwards by a separate stake (the judge of `I§9`), slowly; a seat that signs two prices for one
   tick is slashed on chain without a judge. A majority of the judge can still decide falsely; that is accepted.
4. Small holders join by pooling into a seat (stage 2). The claim's shape, "a claim and a proof that the price source
   said so, or nothing", is the one a miner-voted price or one aggregated proof would also have: only the verifier of
   the proof (the ticket, section 2) changes then.

| | oracle-market family | committee family |
|---|---|---|
| who states a price | the pair's underwriter (liquidation), a party (close) | nobody; 67 % of the roster's weight sign ticks |
| what makes it final | no contest within W, or the judge | the signatures; the claim lands within `lateDaa` of the tick |
| a false price | a bond, lost when contested | the signers' stake, slashed after a verdict (stage 2) |
| bound of the damage | `rho` x the underwriter's bond | none by construction (section 7) |
| silence | a party closes on its own word | no claim; exits without a price stay; lapse at the entry |
| objects | KpfOpenO, KpfPairO, KpfPendingO, KpfBond, KpfStake | KpfOpenC, KpfPairC, KpfPendingC, KpfRoster, KpfTicket (KpfStake in stage 2) |

## 1. Ticks

A **tick** is (roster id, market id, t0, price): a seat's statement that the feed of the market (its feed definition,
hashed into the market id as before) was `price` at DAA score `t0`. A seat signs
`blake2b("kpf-tick-1" || roster id || market id || t0 || price)` (the two numbers as 8-byte script numbers) as a data
signature (`checkMsgSig`, 64 bytes). The digest names the roster, so a signature is of no use to another roster, and
not the pair, so a seat cannot choose whom it serves: a signed tick is good for every pair of the market.

Each seat signs **its own price**. A price `p` is **attested** for a tick when seats holding at least `quorumBps` of
the roster's total weight each signed, for that tick, a price within `spreadBps` of `p`
(`|price - p| x 10^4 <= p x spreadBps`). No seat needs to know another's price, there is no leader and no round: seats
publish signed ticks, anyone collects them. Whoever builds a claim chooses `p` and which signed seats to include, so
it may move `p` by the spread around the prices a quorum signed; the tolerance `g` of every claim (kept from the
oracle family) is what that freedom, and the choice of a tick within `lateDaa`, may cost a party. A market's `g`
is at least four times its roster's spread: a pair reads the spread of the ticket it is given and refuses a wider one
(review C2, C2-F1), so a claimant's choice of price takes at most a quarter of `g`. A closer that claims the edge of
the band pays `g` less the spread, not `g`.

The first rule was a weighted median of the included prices. Review C1 (C1-F1) showed that it made half a quorum
enough: the claimant chooses the included seats, so seats holding a third of the weight added just enough honest
signatures (public, of the true price) to reach the quorum and stay the majority of what was included. With the
spread rule a false price needs a quorum of the weight to have signed near it, and honest seats whose prices lie further
apart than about twice the spread (so that no price is within the spread of a quorum of them) attest nothing (fail
closed).

Experiment C1 (`contracts/probes/C1Attest.sil`, `c_experiments_tests`): 16 seats in one input, a 3.8 kB script;
11 signed seats cost 1,143,522 script units, 16 cost 1,658,993 (100,000 a signature, as experiment Z1), the stack
stays at 29. The cost allows sixteen seats and more; a node does not: it admits at most 15 signature operations in
an input's script and counts every data signature of the unrolled loop, whatever entry runs (live run 3; the local
engine's static count did not, which is what Z1 measured). A ticket has **15 seats**, a template constant. More
signers need a seat to be a pool (stage 2) or one aggregated proof in place of the signatures (section 8).

## 2. The roster and its tickets

**KpfRoster** is one covenant lineage (its covenant id is the roster id) holding every seat's stake:

- parameters: `epochDaa` (E), `exitDaa` (U), `minSeat`, `quorumBps`, `spreadBps`, `minTotal`, `tickets` (how many
  copies an epoch gets), `ticketValue`;
- `from`: the start of the current epoch; epochs are `[from + kE, from + (k + 1)E)`, fixed by arithmetic, never by
  "now";
- 15 seats of (key, weight); a seat is empty iff its weight is 0;
- its value is at least the sum of the weights, checked whenever tickets are written.

| Entry | Who | Rule |
|---|---|---|
| `join(i, key, amount)` | anyone | `amount >= minSeat`. Seat i empty: it becomes (key, amount) and the roster's value grows by `amount`. Seat i taken: no seat is empty, no seat is lighter, and `amount >= weight + weight / 20`; the holder's stake leaves as a `KpfExit` released at `from + E + U` |
| `topUp(i, amount)` | anyone | a taken seat's weight grows by `amount` (at least 1 KAS), as the value |
| `leave(i, s)` | the seat's key | the seat is empty; its stake leaves as a `KpfExit` released at `from + E + U` |
| `forfeit(i, market, t0, p1, s1, p2, s2)` | anyone | seat i's key signed two different prices for one tick of one market: the seat is empty, a tenth of its stake is the sender's, the rest is burned. No rest is needed |
| `roll(m)` | anyone | `m >= 1`, `tx.daa >= from + mE`: the epoch moves to `from + mE`; `tickets` tickets of that epoch are written with the seat table as it stands, each holding `ticketValue`; the sum of the weights is at least `minTotal` and at most the roster's value |

`join`, `topUp` and `leave` need the roster UTXO to be 20 DAA old; `roll` does not, so after every change there are
20 blocks in which only a roll can spend the roster and nobody keeps an epoch from starting by respending it on every
block (C1-F5). A change of the seat table takes effect with the next `roll`: the tickets of an epoch are a copy of the table at its
start. A seat that left or was outbid during an epoch can sign until that epoch ends, which is why its release is
`from + E + U`: its stake stays under the roster's id for U after its last valid tick (U must exceed the time a verdict takes,
stage 2). **KpfExit** is that stake on its way out, a UTXO of its own under the roster's id: `withdraw()`, anyone, once
`tx.daa >= release`, pays the amount to its key; `forfeit(...)`, anyone, before that, slashes it by the same proof as
a seat. An exit is not an entry of a queue inside the roster: a queue of fixed
length could be kept full by whoever wanted to stop seats from leaving or from being outbid. There is no vote and no election: the table is the 15 largest bids, and a seat changes hands only by a
larger stake.

**KpfTicket** is a copy of one epoch's table under the roster's id: the seats, the total, the window `[from, until)`,
the quorum and the spread. Covenants have no read-only inputs, so a claim spends a ticket and the ticket re-creates itself; the
number of tickets is the number of claims that can use one epoch in parallel without waiting for each other.

| Entry | Who | Rule |
|---|---|---|
| `attest(market, t0, p, bits, prices, sigs)` | anyone | `from <= t0 < until`, `tx.daa >= t0`; each seat in `bits` signed its price for the tick, within the spread of `p`; their weight is at least the quorum of the total; the ticket continues unchanged at its own output index |
| `retire()` | anyone | `tx.daa >= until + (until - from)`: no output of the id; the ticket's value is the spender's |

Every argument of `attest` is a fixed-width push, so a claimant reads (market, t0, p) from the ticket input's signature
script at fixed offsets and the dispatch tag before the redeem script, exactly as the oracle family read
`KpfBond.bind` (`I§2`). The claimant checks only that the input carries the roster id its market names and is an
instance of the ticket template spent by `attest` with its own market id; the signatures are verified once, by the
ticket, however many pairs the transaction holds.

**Why a ticket can be trusted.** Every UTXO of the roster id was written by the roster (itself, its tickets, its exits) or
by a ticket continuing itself, provided the lineage began as one empty roster. A covenant id is a hash of its genesis (the authorizing
outpoint and the index, value and script of each genesis output), so that is checked offline: the market file names
the roster's genesis outpoint and value, and the wallet recomputes the id from them and the empty roster's script
(`committee-check`). A roster whose id does not recompute is refused.

**If nobody rolls**, no ticket covers the present and no price can be attested: every market of the roster is closed
until someone rolls. That is the fail-closed rule. A roll is one transaction an epoch, and its author funds the
tickets (`tickets x ticketValue`, 4 KAS on the defaults), which whoever retires them an epoch after their window
takes back. A ticket holds at least 1 KAS: an output under a covenant id costs about 4 x 10^12 / value grams of
storage mass, and four tickets of 0.2 KAS were a roll no block could hold (live run 3).

## 3. Markets, orders and pairs

A market block is: the market id (blake3 of the rest), the roster id, the feed hash, and `termDaa, scale, priceMax,
mmrBps, gBps, liqFeeBps, keeperFeeMin, minSize, carrier, idleDaa, graceDaa, lateDaa, quietDaa` (a term of at least
1,200 DAA, `lateDaa` above 0). A pair is the oracle
family's pair without the binding to an underwriter: its state is the market, the position (size, the range
`[bl, bs]`, the entry, the two keys), the lease `expiry`, the two listings and the makers.

| Entry | Who | Rule (what differs from `I§16`) |
|---|---|---|
| order `fill`, `take` | anyone | no slot and no premium; both sides healthy at the price. The lease `xe` is at most a term from now (`tx.daa >= xe - termDaa`) and at least a term (`fill`) or half a term (`take`) after the order was accepted. A side listed in a pair with less lease than that is renewed before an order can take it |
| pair `amend`, `addMargin`, `transfer`, `mutualClose`, `coopClose` | as before | no cover rate and no shares of it: a range is bounded by `priceMax` only. No close-versus-close in this family |
| pair `assign(s, side, n, x, nb, key, c)` | the owner of `side` signs | new: the side changes hands at `x` without a listing, in one transaction, under the rules of a hand-over. The executor's instant exit |
| pair `keepAlive(u, t0)` | anyone | the ticket at input u attests a tick of this market at `t0`: the lease becomes `t0 + termDaa`, at least a quarter of a term later than it was (so nobody respends a pair on every tick) |
| pair `lapseSettle()` | anyone | `tx.daa >= expiry + idleDaa`: both sides at the entry price |
| pair `propose(...)` | close: the closer signs; liquidation: anyone | the price at `t0` is the one the ticket at input u attests, counted as `priceMax` when it is above it; the tick is at most `lateDaa` older than the pair UTXO (an older one could only be void). Close: the closer is not below its maintenance at the price it is paid (as `I§29.3`). Liquidation: the trigger with `g` inside (as `I§16`); the fee goes to the proposer's key |
| pending `cure(s)` | the loser, while pending | as before; the refund to the proposer |
| pending `finalize()` | anyone; a valid liquidation once `graceDaa` old | P is the part's own DAA score. `P - t0 <= lateDaa`: settle (a close: the closer at the price moved by `g` against it; a liquidation: forfeit and keep, the fee to the proposer). Otherwise the claim is void: the part is restored at once with its whole value and its listings (the carrier a proposer funded for a part stays in it) |

There is no bond, no contest and no verdict in a claim: the signatures are the price. What a claim still needs two
transactions for is time. A data signature has no age a script can read, and a transaction proves only that the
present is at least its lock time, so a tick signed long ago could be used when it suits (`D§3.8`). The one exact time
a covenant reads is the DAA score of a UTXO it spends, so the claim becomes a UTXO first and `finalize` reads when it
landed.

**The lease.** Without a price source a pair whose parties cannot agree would be locked for ever. A pair therefore
carries an expiry: after it and `idleDaa` more, anyone settles the pair at its entry price. Any attested tick of the
market proves the committee was alive at `t0` and moves the expiry to `t0 + termDaa`; anyone may do it, for any pair.
While the committee signs, a party that wants its position kept renews the lease once a term; when the committee is
gone for good, every pair unwinds at the price both parties agreed to.

**The executor.** A user leaves at once by `assign`: it signs its side over to an executor, which pays the user in the
same transaction and holds the position. The executor closes with the committee's price when it likes; a missing
quorum is the executor's problem, not the user's. The executor needs no permission and no contract of its own.

## 3a. Taking margin out of a position, and moving it

A side's value is `q(s, c(p) - its bound)`: the margin it posted and what the price has earned it are one number, and
until now nothing took it out but leaving the pair. A trim makes that number movable: a position with margin to
spare gives it to a position near its liquidation, without closing either. This is not cross margin: each position
is still liquidated on its own (section 3b is the account in which two positions are one).

**Trim** (`KpfPairC.propose` kind 3, `c-trim`). The owner of a side signs; a ticket attests `p` for the tick `t0`; the
side's bound moves inward to `fb` and `q(s, bs - bl) - q(s, the new range)` is paid to its owner. The rules:

| Rule | Why |
|---|---|
| the side is not below its maintenance at `p` moved by g against it, measured from `fb` (the rule of a close; the bound a cure at `p` gives) | a trim never leaves a side that a claim within g of `p` liquidates |
| the trimmed range is still wider than both maintenance bands (`bl (1 + m) < bs (1 - m)`) | a narrower range is one in which neither side can be liquidated nor close |
| the whole pair, at least 0.02 KAS freed, after `quietDaa` | no split to carry, no dust output, no respending a pair on every block |
| a claim: a pending part, settled by `finalize` only if it landed within `lateDaa` of `t0`, restored otherwise | a tick signed when the price was better would free margin the side no longer has, and that margin is the other party's |
| `cure` is for a liquidation only | a trim's cure bound is the bound it asks for: cured, a void trim would be paid |

The trimmed pair keeps the entry, the lease and both listings.

**What the other party gives up.** Its payoff at every price inside the new range is unchanged. It loses the range
beyond `fb`: if the price gets there it is paid in full and the position ends (the liquidation of a side at its
maintenance, which pays the winner the loser's slice). It never had more: the trimmer could end the pair at the same
tick with a close, which takes the whole position from the other party. A trim is the lesser of the two and needs no
tolerance of its own, since the side stays `g` and the maintenance away from the attested price.

**The lapse.** A pair nobody keeps alive settles at its entry clamped to the range. A side trimmed above the entry
has taken out more than the entry would have paid it and is owed nothing more at a lapse: a trimmed side keeps its
lease alive (`c-keep-alive`) or closes.

**The limit of a trim.** `c-account` shows it as the free margin: the nearest bound is `floor(floor(p (1 - g)) (1 -
m))` for a long, and no nearer than the range rule allows, so a side whose counterparty has little margin left frees
less than its own health would allow.

**With an executor** the same move needs no trim and nothing new on chain: the user lists its side (`c-amend`), an
executor takes it at once and pays the side's whole value (`c-transfer`), and the user opens the same size again at
a nearer bound against the executor's order (`c-open-taker`). The other party's range does not change at all; the
executor is paid by the price it takes the side at. The trim is the route when no executor answers.

**Moved, not shared.** A pair is liquidated on its own, at its own bounds: there is no account whose total decides.
What links the positions of a key is something that moves margin before a liquidation finalizes, and that is a
process with the key: `c-margin-run`. Each round it prices the key's positions at the newest attested tick; a position
within `alertBps` of its liquidation line gets the margin that puts the line `targetBps` away, from the wallet if it
holds it and otherwise from the positions with margin to spare (trim, finalize, add margin: three transactions, the
first a claim that must land within `lateDaa`); a pending liquidation of a position is cured within its grace; a
pending trim is finalized; a lease in its second half is renewed. While it runs margin follows the positions that
need it; when it stops, each stands alone. It manages the files it was started with and those its own transactions
wrote, and no other: a pair is a script and a state, anyone writes one that names a key, and margin put into such a
pair is its maker's (review C8, C8-F1).

A trim to a bound near the pair's entry price also ends the splitting of that pair: a split must leave each side
0.2 KAS of value at the entry in each piece (`checkPart`), and a side whose bound is at the entry has none there. The
other party then closes or hands over the whole pair or nothing (C8-F3).

DESIGN: no rescue by others. A rule that let anyone move a key's spare margin into its position under liquidation
(on the key's signed consent, with the key away) was designed and dropped. A covenant knows a position of its own
lineage and nothing of another's: a "position of the same key" presented beside it is a script and a state, and a
state can be written by anyone. A helper would show a pending liquidation it made up, of a pair it made up, and the
margin would go to the helper's side of it. The margin a trim frees therefore goes to the owner's key and nowhere
else. Positions linked without a process need one lineage that holds them all (an account covenant): section 8.

Tests: `crates/kpf-tests/tests/trim_tests.rs` (the engine: every rule, one sompi each way),
`crates/kpf-cli/tests/committee_cli.rs` (`c-account`, `c-trim`, `c-margin-run`).

## 3b. Cross margin: two positions in one account

Cross margin is two positions that are liquidated as one: a gain of one pays for a loss of the other as it happens,
and nothing is moved or claimed between them. Two pairs cannot be that. A pair's other side is a party of its own,
and what one pair owes its other side cannot depend on a second pair it never agreed to. So the positions change
form: the owner (A) asks, an executor (B) agrees, the executor takes A's two positions over as they are, and A holds
one UTXO against the executor instead, with both positions in it and one margin: `KpfCrossC`.

```
before                              after
A  long   market 1  vs  X           B  long   market 1  vs  X      (the pair, as it was)
A  short  market 2  vs  Y           B  short  market 2  vs  Y      (the pair, as it was)
                                    A  vs  B: one account, long market 1 and short market 2, one margin
```

X and Y keep the pairs they had, with the executor in A's place (a hand-over, section 3): nothing of theirs depends
on the account. The executor is flat (long against X, short against A) and carries what A's account can lose beyond
its margin, as it carries every position it fronts.

**The account.** Terms the two parties agree (the term of the lease, `mmrBps`, `gBps`, the fee, the times), two legs
and the margins. A leg is a position of A in one market: the market id and its roster, the scale and the price limit,
a size, a direction and the entry price it is counted from; a second leg of size 0 is no leg. With
`q_i(x) = floor(size_i x / scale_i)` and prices `(x_1, x_2)`:

```
gain_i = q_i(x_i) - q_i(entry_i)   (long)      q_i(entry_i) - q_i(x_i)   (short)
A is owed   V_A = clamp(aMargin + gain_1 + gain_2, 0, pot)        pot = aMargin + bMargin
B is owed   pot - V_A
```

The UTXO holds at least the pot and a carrier. `N = q_1 + q_2` is the notional and `M(N) = ceil(N mmr / 10^4)` the
maintenance. A price moved by g rises to `ceil(p (10^4 + g) / 10^4)` (at most the leg's price limit) or falls to
`floor(p (10^4 - g) / 10^4)`; "toward A" moves each leg the way A gains, "against A" the other way.

| Entry | Who | Rule |
|---|---|---|
| `addMargin(side, delta)` | the side's owner, after `quietDaa` | its margin grows by delta, paid in |
| `propose` kind 1, close | the side's owner, after `quietDaa` | it is paid its side at the prices moved by g against it; refused only where it would escape a due liquidation (the closer is below the maintenance there and the other party has its own); the other party is paid the rest |
| `propose` kind 2, liquidation | anyone | the side is below the maintenance at the prices moved by g toward its gain and the other party is not; the two keys differ; the fee `max(keeperFeeMin, floor(N liqFeeBps / 10^4))`, at most what the loser is still owed at the attested prices, to the proposer and the rest of the account to the other party |
| `KpfPendingX.cure` | the loser, before finalize | its margin grows by what puts its side at the maintenance at the prices moved by g against it: no claim within g of these prices reopens a cured account |
| `KpfPendingX.finalize` | anyone | a claim that landed within `lateDaa` of its tick settles (a liquidation after `graceDaa`); otherwise the account is restored whole |
| `keepAlive(leg, ...)` | anyone | a tick of either leg's market moves the lease to `t0 + termDaa`, by a quarter of a term at least |
| `lapseSettle` | anyone, `idleDaa` after the lease | A is paid `aMargin`, B the rest: the entries |
| `coopClose` | both | any outputs |

**Both prices for one tick.** A claim presents a ticket for every leg, attesting its market for the same `t0`, in
the same transaction. A leg priced now and a leg priced a minute ago would be an account that never existed. For
that `KpfTicket.attest` no longer requires to be the only ticket of its roster in a transaction: several attest side
by side, one a market, each continuing at its own output index, as many outputs of the roster's id as inputs. A
market without ticks closes the account to claims (fail closed) and the lease unwinds it if the committee is gone.

**What the claim fixes.** `propose` computes every amount where the legs are known and writes them into the pending
claim (`KpfPendingX`): the closer's payout or the fee, the cure's addition, the hashes of the restored and the cured
account, and the account's state as it was (so that whoever finalizes or cures can write the account again; the
hashes are what bind). The pending claim knows nothing of legs or prices: it reads when it landed.

**Two parties, by consent.** No contract checks the terms, the legs or how the account began: both parties sign the
transaction that writes it and each reads the state it signs. Nobody else ever holds a side of it, so nobody else is
owed a rule. The loser of a liquidation forfeits the whole account less the fee, as the loser of a pair forfeits its
side; the cure is its way out.

**The maintenance is on the gross notional.** It grows with the prices while the pot does not: an account whose legs
offset loses nothing when both markets rise, and still needs `mmr` of a larger notional. A side below it is
liquidated only while the other side has it (review C9, C9-F1): when the pot is smaller than two maintenances neither
side is liquidated, and either leaves by a close, paid its side at the prices moved by g against it (a close is
refused only where it would escape a due liquidation). So a bankrupt party never holds the other until the lapse,
which would pay it its margin at the entries. What stays (review C10, C10-F2): in that state the party that first
adds what its own side lacks has the maintenance while the other does not, and may liquidate it at once; the other
closes before that, or cures within the grace. An owner keeps its
margin above `mmr` of the gross notional as it would for two pairs; what the account spares it is the margin that
one leg's loss would have needed while the other leg held the gain.

**What a wallet checks.** Nothing on chain ties a leg's market id to its roster id or the terms to the markets'
(C9-F3): `x-show` prints both ids whole, to be compared with the market files one checked (`c-check`); `x-swap` and
`x-open` take the legs from the pairs or the market files and the more careful of the two markets' parameters. An
account file handed over by somebody else is read before it is trusted, like any other object.

**What it does not do.** Two legs, no more. No hand-over of a side of the account (both sign, or it is closed).
No split. Opening it and taking the two pairs over in one transaction is a builder's matter, not a contract's: the
owner builds the swap and signs its pairs, the executor reads it, signs its funding and sends it (`--partial`,
`cosign`: runbook 10.5c), so the two keys never meet.

Tests: `crates/kpf-tests/tests/cross_tests.rs` (the engine: a gain of one leg paying for a loss of the other, the
liquidation on the total, each amount to the sompi).

## 4. What a party can lose without signing

| Cause | Bound |
|---|---|
| a true liquidation of its side | its maintenance margin at the attested price, as before |
| a close by the other party | nothing at the attested price; the closer pays `g` |
| the tick and the price a claimant chose | the feed's move within `lateDaa`, and the roster's spread around what a quorum signed; both inside `g` by the market's choice of `g` and `lateDaa` |
| a void claim on its pair | one block off the market, and the quiet time of the parties' own entries starts again; a void claim on a part leaves the pair in two lineages. A claim on a tick older than the pair UTXO by more than `lateDaa` is refused, so each needs a tick of the present |
| the other party's entries | each restarts the quiet time: a party that respends the pair at every boundary delays an `assign` or a close for a fee every `quietDaa` (C1-F6) |
| a lapse at the entry | its value at the feed less its value at the entry; only after the committee attested nothing usable for `idleDaa` past the lease, or nobody renewed |
| a false price signed by a quorum | its whole side (section 7) |

An order's owner, without signing: a `fill` or `take` enters it only into a pair healthy on both sides at the price,
with a lease of at least a term (half a term for a `take`) from the order's own acceptance; an order that rested
longer than that less the idle time can be entered into a pair about to lapse and gets its margin back at the entry,
less its tip and one carrier. An order's `expiry` does not stop a fill: nothing proves to a script that the present is
before a time. It binds when a refund lands, which anyone may send from the expiry on and is paid `keeperFeeMin` of
the escrow for (review C2, C2-F2); until then the order is filled at its limit whatever the feed is, as a resting
limit order is.

## 5. Stages

1. **The price path** (this stage): KpfRoster and KpfTicket (join, top up, leave, withdraw, roll; attest, retire);
   KpfOpenC, KpfPairC, KpfPendingC; the library, the commands, a live run. Stake is locked for the exit time but not
   yet slashed.
2. **Accountability and pools**: a seat that signs two prices for one tick is slashed by anyone showing both
   signatures; a signed price is accused, judged by KpfStake and the seat slashed on a verdict; a seat becomes a pool
   (an operator's key and bond, depositors' shares, the operator's bond slashed first); a fee per attested claim paid
   into the seats.
3. **Review rounds** until one finds nothing above Low, then the items of section 8.

## 5a. Stage 2

**A seat that signs two prices for one tick (built)** is slashed by anyone who shows both signatures, without a
judge: `KpfRoster.forfeit(i, market, t0, p1, s1, p2, s2)` (and the same entry of `KpfExit` for a stake on its way
out) checks two data signatures of the seat's key over the two tick digests and `p1 != p2`, empties the seat, and
requires all but a tenth of its stake at the burn script (the output at the input's mirror index); the tenth is the
sender's. The sender is whoever lands the transaction: the proof is public once shown, so a miner can take the share, and
so can the offender, which then loses nine tenths and not all (review C4, C4-N1); the seat is slashed either way. The
tenth pays for the transaction and for looking; it is not what deters. A slashed seat stays in the tickets of its epoch (a copy cannot be recalled) and can
sign until the epoch ends, with nothing left to lose: at most one epoch. Commands: `seat-forfeit`, `exit-forfeit`;
`tick-sign` keeps what a key signed in a journal beside the key file and refuses a second price for a tick it holds
(review C4, C4-N2). A seat's software must keep that journal: a restart that signs the same tick again at another
price is the same proof.

**A seat that signs a false price (built)** is charged, judged by a stake apart from the seats' and slashed on its
verdict. A quorum of seats cannot rule on itself, so the judge is the stake judge of the oracle family (`KpfStake`,
unchanged) under a judge block the roster names: stakers who bond under that block judge this roster's ticks, on any
market it prices. The claim the judge rules on is "the tick is true": the price the key signed lies within the
market's `g` of the feed at that DAA score, by the market's feed definition (a closed session has no true price).
`g` is at least four times the roster's spread, so honest seats that differ by their sources are not false (review
C5, C5-F4: this is the one statement; the commands print it).

| Step | Entry | Rule |
|---|---|---|
| charge | `KpfRoster.charge(market, t0, p, s, i, payKey, k, ...)`, `KpfExit.charge(...)` | anyone, with `chargeBond`: the stake's key signed that tick (`checkMsgSig`). A `KpfCharge` (the tick, the key, the roster's id, the judge block, `payKey`, the stake as it is now: `atStake`) is written at output k under a fresh covenant id, and the stake's holds take the id. The seat goes on signing |
| contest | `KpfCharge.contest(u, d)` | the claim interface of the judge: a free stake of the roster's judge block, accepted `ageDaa` before the charge, opens the dispute with its value on NO |
| verdict | `KpfCharge.decide(d)` | with `KpfStake.resolve`: refuted if NO leads, upheld if YES leads, lapsed whatever the verdict if the contest landed `chargeDaa` or more after the charge |
| slash | `KpfRoster.slash(c, i, a, later)` / `KpfExit.slash` with `KpfCharge.settle` | the charge at input c is among the stake's holds and is refuted: the stake loses `min(stake, NO)`, where NO is the total the judge's NO side had at risk when the round closed (if less than 1 KAS would be left: all of it when this is the stake's last hold, all but 1 KAS while it has others); `min(atStake, NO) / 10` of that and the charge's bond to `payKey`, the rest burned. A stake that keeps something keeps its other holds |
| clear | `KpfRoster.clear(c, i, a, later)` / `KpfExit.clear` with `KpfCharge.settle` | upheld: the hold ends and the bond is the charged key's. Lapsed, or pending for `chargeDaa`: the hold ends and the bond is burned |

What the design answers:

- **A charge does not suspend the seat.** Anyone could otherwise silence a roster for the bonds. It holds the stake:
  a held seat signs, tops up, is outbid and leaves as before, the exit carries its holds, and an exit with a hold is
  not withdrawn. So the exit time `U` is the time a false tick can be *charged* in, and a charged stake waits for its
  verdict however long the dispute runs.
- **Holds are one hash a stake**, zero for none and `blake2b(holds || id)` for each charge added, so a stake can be
  charged for any number of ticks at once. A charge is shown to be among them, and taken out, with the value before
  it and the ids added after it (at most eight). With one slot a seat, an accomplice's charge that nobody contests
  would shield a seat from the charge that matters. With the chain, a real charge is blocked only while more than
  eight unsettled charges were made after it, each of which costs its author the bond (burned when it lapses) or the
  burned share of a contest stake.
- **The roster knows its own charges by their ids.** A charge's covenant id is in a stake's holds only if the
  roster's (or the exit's) `charge` wrote it, from its pinned template, under an id no input carried and no other
  output carries; every later object of that id is written by `KpfCharge` or by the judge. A `KpfCharge` anyone
  writes by a genesis, in whatever status, is in no stake's holds.
- **The stake and the charge leave together.** A charge settled alone would leave a hold nothing can end. The
  roster and the exit pin the charge's template, so the charge cannot pin theirs; it names them as a claim names the
  judge's entries: an input of the roster's covenant id (only the roster's templates write under it) whose signature
  script has the length of a `slash` or `clear` of the roster's or the exit's script, their dispatch tag at its end,
  and this input's index as its first argument. Lengths and tags do not depend on a hash, so the three templates are
  compiled to a fixed point (the generator does it; the manifest records the lengths and tags).
- **A lapsed charge's bond is burned**, not paid to the charged key, and a late contest lapses instead of upholding:
  a seat could otherwise hold its own stake for nothing.
- **A contest nobody answers is a verdict** (review C5, C5-F1). The judge counts stake, and a NO that no YES
  answers within a round wins and comes back whole. With a slash of the whole seat, one judge stake of `minStake`
  emptied a seat of any size that had signed a true price, paid its author a tenth, and seat by seat left the roster
  to whoever then joined for `minTotal`. So a verdict takes no more than its NO side had at risk, and pays a tenth
  of no more than that. Emptying a roster by silence now needs as much aged judge stake as its seats hold, each
  stake usable once per `ageDaa` (a stake returns with its age restarted), and all of it is lost to the first
  answer; a contest of `minStake` gains its author a tenth of `minStake` at most, a winning bet only if more than
  ten contests in eleven go unanswered. What stays: an honest seat that is charged must answer, or have someone
  answer for it, within a round, with judge stake aged before the charge: at least `mu` times the contest (125 KAS
  against the least contest on the defaults), which wins the contest stake less the burn. A seat is online to sign
  every tick; `c-watch --me` raises a held seat and a charge against one's key, and `charge-answer` is the answer.
  A roster does well to name a judge block that already has stake: on a new block the first stakes are whoever
  came first. And whoever holds most of a judge block's aged stake wins every dispute: that is the judge's limit
  (section 0), not this one.
- **A held stake shown two prices keeps a tenth under its holds** (review C5, C5-F2). A charged seat forfeited
  itself, took the sender's tenth and left its charges nothing to settle against. Now nine tenths are burned, nobody
  is paid, and the tenth (at least 1 KAS) waits for the charges: a refuted one takes it, and its author's share is
  measured by the stake as it was charged (`atStake`), so the author is paid what it would have been.
- **What a charge costs.** Its author: the bond for the time of the dispute, and a judge's stake on NO (its own or
  a staker's) as large as what it wants taken, which is lost if the tick is upheld. The seat: nothing unless the
  tick is refuted, and then up to all of it. Fifteen seats that signed one false price are fifteen charges and fifteen disputes over the same
  question; one dispute a tick whose verdict slashes every signer is the saving not made here.
- **A stake with other holds keeps 1 KAS for them** (review C6, C6-F1). A slash that took a stake whole left its
  other charges nothing to settle against, and a seat arranged that by charging and contesting itself with a stake
  of its own size. Now the last charge takes the last KAS, and every charge is settled: a refuted one's bond returns to its author,
  an upheld one's goes to the charged key, a lapsed one's is burned (review C7 drove every order of two and three
  charges on a seat and on an exit: none is left locked). A refuted charge that meets a stake at the floor with
  another hold takes nothing and pays no tenth: it returns the bond and ends the hold (C7-N2).
- **Who is paid is whoever settles first.** A seat with aged judge stake of its own size can charge itself, contest
  itself and take the tenth (as an offender can take the sender's tenth of a two-price proof), leaving a later
  accuser its bond and the last KAS. A held stake shown two prices keeps a tenth of *that* forfeit: the same proof
  shown again leaves a tenth of the tenth, down to 1 KAS (C6-F2), and the accuser's share with it. And whoever
  contests first sets what the verdict takes (C6-F3): nothing joins a leading side, so a seat that contests its own
  charge with `minStake` loses `minStake` to that charge, and its accuser charges again (the same signed tick
  serves) for each `minStake` more. In each case the stake is burned all the same, in the third while its accuser keeps charging (C7-N3); what moves
  is the reward.
- **Left behind.** Nothing is locked any more. A hold costs its author the same whatever it holds (C5-F3): the bond a
  `chargeDaa`, or the burned share of a lost contest. Whoever answers a contest must check by hand that the dispute's claim is a charge
  of the roster (`charge-settle` refuses a charge that is not among the stake's holds): `deposit` cannot trace it.

What it added to the templates: the holds (480 bytes), the judge block (87) and two parameters in the roster's state;
a hold, the judge block and the two parameters in an exit's; three entries each; `KpfCharge` (three entries). The
roster's script is 17,374 bytes (7,690 before), the exit's 4,596. Commands: `charge`, `judge-stake`,
`charge-contest`, `charge-answer`, `charge-decide`, `charge-settle`, `hold-witness`; the judge's own (`deposit`,
`settle-pool`, `stake-unbond`, `stake-withdraw`) are the oracle family's.

The rest of this section is designed, not built.

**A seat as a pool.** A seat gets an operator key (which signs ticks), the operator's own bond and a number of shares;
a depositor adds KAS to a seat and holds a share note (a UTXO of its own under the roster's id); a slash takes the
operator's bond first and the depositors' stake after it; a note leaves like a seat, through an exit. The reporter's
share of a slash stays below the operator's bond, so an operator cannot slash its own pool through an accomplice and
come out ahead.

**A fee per attested claim**, paid into the seats that signed (their weight grows, so every share of a pool does).

## 6. Parameters (testnet defaults)

| | value | at 10 DAA/s |
|---|---|---|
| seats | 15 | |
| `quorumBps`, `spreadBps` | 6,700, 10 | a price needs 67 % of the weight within 0.1 % of it |
| `epochDaa` | 3,000 | 5 minutes |
| `exitDaa` | 72,000 | 2 hours |
| `tickets`, `ticketValue` | 4, 1 KAS | |
| `termDaa` | 6,048,000 | 7 days |
| `idleDaa` | 864,000 | 1 day |
| `graceDaa` | 100 | 10 s |
| `lateDaa` | 100 | 10 s |
| `quietDaa` | 20 | 2 s |
| `gBps`, `mmrBps` | 50, 500 | |

## 7. Assumptions and known limits

- **A quorum that lies takes every pair of its markets.** Nothing caps open interest against the roster's stake:
  covenants have no global total. Whether lying pays is the ratio of the stake a quorum loses (stage 2) to the value
  it can move in the time before it is slashed and replaced. A cap per epoch carried by the tickets is listed in
  section 8.
- **Respending is cheap.** Each ticket is one UTXO, and anyone with public signed ticks may spend it by `attest`:
  keeping all four tickets of an epoch busy in every block costs fees only (review C1: about 224 KAS for a 5-minute
  epoch at the relay floor), and a claim built on a ticket that was just respent must be rebuilt. More tickets and
  a higher fee are the answers; no stake is needed to try.
- **A third of the weight can stop every market** by not signing; a stopped market is safe (exits without a price,
  then the lapse) but nobody is liquidated meanwhile, so a winner's gain beyond the loser's margin is lost, as in every
  gap.
- **The judge is a vote of another stake** and decides falsely if its majority does; only a clear lie (beyond `g`) is
  judged at all.
- **Seats publish ticks off chain.** How signed ticks reach claimants (files, a relay) is tooling, not a rule.

## 8. Later

Open-interest cap per epoch in the tickets; restitution of a false tick's victims from the slashed stake; one
aggregated proof instead of fifteen signatures (the compiler has `g16.verify` and `r0.succinct.verify`; not measured);
a miner-voted price as another ticket template; dollar-token margin.

A seat for a moment. A roster is made for one market by whoever wants one, with epochs as short as a minute, so a
signer joins for one market and one hour; what does not shrink is the time a stake stays after its seat is left (the
window in which a false signature is charged). A seat that wants its KAS at once hands the stake over as a position
is handed over: an executor pays it now and holds the stake, with its liability, until it is released; a pool's
share that changes hands does the same for small holders. Neither is designed.

An account covenant: the positions of one owner under one lineage, so that a liquidation is decided on their total
and margin moves between them without the owner's key at hand (section 3a moves margin with a process that holds the
key; "no rescue by others" there says why a helper cannot). It changes what a counterparty holds: its claim is on an
account and no longer on a range, and every entry of the pair is to be thought through again.

### 8a. Size: what a transaction carries (measured, not built)

A fee is paid by the byte (two grams a byte at the relay floor: 0.002 KAS a kilobyte), and a P2SH spend carries its
whole script. Measured over every transaction the command-line tests build (`KPF_COST_LOG=FILE cargo test -p
kpf-cli --test committee_cli`):

| Transaction | Bytes | What it carries |
|---|---|---|
| `c-open` | 34,813 | two orders, each given the pair's 13 kB script as an argument to name the pair it writes |
| `x-swap` | 27,557 | two pairs' scripts |
| `roster-join` | 22,324 | the roster's 17 kB script, the exit's template |
| `c-close`, `c-liquidate`, `c-trim` | 19,654 | the pair (13.3 kB), a ticket (3.1 kB), its signatures (1.1 kB), the pending template |
| `x-close`, `x-liquidate` | 17,133 | the account (7 kB), two tickets |
| `c-add-margin` / `x-add-margin` | 13,824 / 7,532 | one number changes |
| `c-finalize` / `x-finalize` | 2,415 / 1,778 | |
| a plain payment | 353 | |

The owner's measure: 1 kB is what an operation should be, 2 kB is large, 3 kB and more is far too much. Every
operation of this family is 4 to 35 times that, and nearly all of it is code the operation does not run.

Probe T1 (`contracts/probes/T1Slot.sil`, `T1Add.sil`; `crates/kpf-tests/tests/thin_probe_tests.rs`): a position whose
script is the hash of its state and the hashes of the scripts allowed to move it (202 bytes), and the script of one
operation (an added margin: 369 bytes) presented as another input, a UTXO of its own that goes on unchanged and that
anyone may keep copies of at its fixed address. The position checks that the logic beside it is one of its own and
names it; the logic reads the position's script from the position's signature script, checks the state it is shown
against the hash, and requires the same script with the new state's hash. The whole transaction is 1,228 bytes: 6
and 11 times smaller than the two forms above, and the wiring binds in the engine.

Probe T2 (`T2Slot.sil`, `T2Close.sil`; the same test file): a close of a two-leg account at attested prices, in
that form. One script for every object (236 bytes: a position, a pending claim, a price note); the prices read from
two price notes (objects under the roster's id whose state is market, tick, price) that go on unchanged; the
account's 405 bytes of state shown whole; the arithmetic of `KpfCrossC.propose` kind 1 to the operator; the pending
claim the same position with another state's hash. The logic is 2,248 bytes and the whole claim 4,303, against
17,133: four times smaller, and still far above the measure. Of the 4,303: the arithmetic 2,248, the state 405, the
position and two notes 735 with 450 of inputs and outputs, the rest the transaction's frame. A one-leg account, a
script for the healthy close alone (half the arithmetic: the prices moved one way), and a state shown in parts would
bring a claim to about 2.5 kB, not to 1.

To bring a claim itself to 1 kB the arithmetic must leave the common path: the claimer states the amount, with a
bond, and the script that computes it (T2Close, with the notes) runs only if somebody disputes the claim within a
window, at the loser's cost. That is a watcher again, as the judge is one for a false tick: a decision of the
owner's, not of a measurement.

The owner chose it (21 November 2026): claims are declared; the script that computes is the dispute's, and no
claim that computes on the common path is offered beside it (its two merits, no window and no watcher, are covered
by the executor's instant exit and by the grace a liquidation has anyway).

Probes T3 and T4 (`T3Slot.sil`, `T3Claim.sil`, `T4Slot.sil`, `T4Declare.sil`, `T4Settle.sil`; the same test file):
the common path of a declared close. The claimer signs "at these prices of this tick I am owed this" and posts a
bond of a rate of the pot; the account becomes a claim holding its value and the bond; once the claim is as old as
the dispute window, and if it landed within `lateDaa` of its tick, anyone settles it as declared. Nothing is
computed. An account's state is hashed in two parts, so that these scripts are shown its 120 moving bytes (keys,
margins, lease, the three times) and only the hash of the terms and legs. A relative age is the spent input's own to
prove: the object proves the age its logic's signature script asks for, and the logic asks for the window.

| Probe | The object's script | Declaration | Settlement |
|---|---|---|---|
| T3: the object names four scripts by hash; one script holds both entries | 371 | 2,386 | 2,343 |
| T4: every script is a UTXO under one library id, which is all the object names; one script an entry | 204 | 1,707 | 1,593 |

(Each with a funding input and a change output, about 180 bytes; `KpfCrossC`'s claim and finalize are 17,133 and
1,778.) A library is the family's scripts as UTXOs under one covenant id, written at its genesis, each going on
unchanged and the transaction writing no other output of the id: "an input that carries the library's id" is then "a
script of the family", one comparison whatever the number of operations, and a later version is another library that
a position moves to by both signatures.

Probe T5 (`T5Dispute.sil`, `T5Prove.sil`): the rest of a declared claim's life. A dispute is a bond as large as the
claimer's and the disputer's key written into the claim, allowed only for a claim that landed within `lateDaa` of its
tick; the claimer answers with the proof (the account whole, a price note a leg, the arithmetic of a close: 4,295
bytes, the only transaction that computes) and is paid its amount and both bonds; a dispute nobody answers within the
window restores the account, gives the disputer its bond back and half of the claimer's, and the other half to the
party the claim was made against (2,304 bytes; the dispute itself 2,200). In the
engine: a true claim takes the disputer's bond; a claim 100 KAS above the rule is proved by nobody, whatever notes
are shown, and loses its own; a true claim whose claimer does not answer loses it too; and a false claim nobody
disputes is paid as declared, which is what the window is for.

Why the side that is losing does not dispute for the sake of it: the amount is that of the claim's tick and no
delay changes it; the dispute costs a bond that a proved claim takes; what it buys is the proof window, and the bond
is set above what the price moves in that time. A liquidation's cure keeps its own deadline, which a dispute does
not move. No stake of the committee is touched: the proof is the signatures the seats already published.

A limit of the engine met here (`MAX_STACK_SIZE`, 244 items on the two stacks together, a consensus rule and not
the compiler's): a function's parameters stay on the stack after the call, and the proof was over the limit until its
eighteen small field readers were written out in place. One script an operation keeps each of them under it.

A rule the probes were missing, found on reading them again and shown by a test before it was closed
(`t6_an_object_named_at_a_place_the_script_does_not_read`): an object accepts a script that names it at any of the
three places, so a script must account for every place it names. The declaration read the first place only; a second
account spent beside it, naming itself at the second place, was accepted and its 500 KAS went to the change. Every
script now has the places it does not read name the object it does (25 bytes), and the sizes above are with it.

Argent (asked 21 November 2026): the family stays hand-written SilverScript. Argent compiles to SilverScript and meets
the same limits; KOB's measurement of a port (`vendor/kob/docs/argent/argent-feedback.md`, items 6, 9, 10; argentc of
30 September 2026) is +1,021 bytes on a 1,684-byte order, because `become` rebuilds and compares the whole state
where the hand-written order splices the bytes that change, and every entry of an actor goes into one redeem script,
which is the opposite of one script an operation. What Argent is for is composition: when positions are settled in
KCC-20 or another app builds on them, the hand-written contracts are published as an Argent app, as KOB's are.

The family's own scripts began from these probes (`contracts/`, 21 November 2026): `KpfSlot` (the object),
`KpfTDeclare`, `KpfTSettle`, `KpfTDispute`, `KpfTProve` (the probes' bytes, which the test asserts), and three
operations more, each a script of its own under the library's id:

(The sizes in this table are from before the dues: every script, and so every transaction, is 83 bytes longer
since 22 November 2026.)

| Script | What | Script | Transaction |
|---|---|---|---|
| `KpfTAdd` | the owner of a side signs and adds to its margin | 475 | 1,509 |
| `KpfTBoth` | the two parties sign and the script asks nothing else: close, new terms, or the move to a later library | 320 | 1,378 |
| `KpfTLate` | a claim that landed after `lateDaa` is ended by anyone: the account restored, the bond back | 601 | 1,640 |

Since the same day the family is the main one of the repository (`contracts/*.sil`, pinned under
`contracts/artifacts/`; the three earlier families are under `legacy/`), with a model of its objects
(`kpf_protocol::thin`: the two parts of an account's state, the three hashes, the bond, what a settlement, a proof
and a lapse pay) whose hashes and scripts a test holds equal to the ones the engine tests run, builders
(`kpf_protocol::thin_build`: a library's genesis, an opening that publishes the account's state in the
transaction's payload, add, declare, settle, late, dispute, lapse, a close by both) and commands (`lib-create`,
`acct-*`; `acct-show` tells of a claim whether its amount is the rule's, which is the watcher's check). Through the
builders, signed and metered, an operation is 1.4 to 2.3 kB and pays 0.004 to 0.006 KAS (a library's genesis of 14
scripts 0.008). An object's file holds its state, since the chain holds its hash; a command refuses a file whose
state is not the one its script carries. Not built: the proof (it waits for the price notes), liquidation, the lease,
the committee's fee, orders between users, reading an opening from the chain (the liquidation, the proofs and the
exit from a committee that stopped were built later the same day: below). No review yet. Run on testnet the same day (`docs/LIVE-4.md`,
eight transactions): a library, an opening with its payload, a false claim disputed by a second executor and
lapsed, a true claim settled.

Who can dispute: an object carries a hash, and the fixed part of an account (its terms and legs) is never shown on
the chain before a proof. A claim shows the moving part and what was stated, so only somebody who holds the fixed
part can tell a false amount from a true one: the counterparty, which has it and loses by a false claim, and whoever
the parties give it to. The window is therefore the time the counterparty has to look, and publishing the fixed part
at the opening (312 bytes, once) is what would let anybody watch.

The liquidation (`KpfTLiq`, `KpfTLiqSettle`, `KpfTCure`, `KpfTLiqDispute`; 21 November 2026) is declared as a close
is, by anyone: "at the prices of this tick the side S is below its maintenance; my fee is F and a cure adds C", with
the bond. The rule is `KpfCrossC`'s (the side below its maintenance even at the prices moved toward it and the other
party not; the keeper's fee, at most what the side is still owed; the cure what puts it at the maintenance at the
prices moved against it; the side forfeits to the other party). A claim is one of three states: claimed
(`liqHash`), disputed (`blake2b(push(claim) + push(disputer))`), proved (`blake2b(int 1 + push(claim))`).

| Step | Who | What | Transaction |
|---|---|---|---|
| liquidate | anyone, with the bond | the account becomes a claim | 2,306 |
| cure | the side named, signed; a claim nobody disputed half a window old, or a proved one; never a disputed one (T1b, F2) | the account again with the margin the claim asks added; the liquidator's bond back | 2,111 |
| settle | anyone, the claim a window old (claimed and in time, or proved) | the liquidator its fee and bond, the other party the rest | 1,723 |
| dispute | anyone, with a bond | the liquidator must prove it | 2,349 |
| lapse | anyone, the disputed claim a window old | the account again; the disputer its bond and half of the liquidator's, the side named the other half | 2,453 |
| late | anyone, a claim that landed after `lateDaa` or before its own tick | the account again; the bond shared by who ended it and the side named (T1b, F3) | 2,364 |

Two choices in it. A proved liquidation is not settled by its proof: it waits a window again, so that a liquidator
that disputes its own claim and proves it at once does not take the cure's time from the side (the price of this is
that a dispute lengthens the time to settlement by up to two windows, which costs the disputer its whole bond when
the claim is proved). And the cure is signed by the side: unsigned, a liquidator caught with a false claim would end
it by a cure of a sompi and have its bond back. The proof itself (`KpfTLiqProve`) is not built: it comes with the
price notes. The two scripts of two entries (`KpfTLiq`, `KpfTLiqDispute`) are above 2 kB for it and would each be
about 0.3 kB less as two scripts.

The proofs (`KpfTProve`, `KpfTLiqProve`; 21 November 2026) read no price note. A proof spends, in its own
transaction, a ticket of each leg's roster by the ticket's `attest` for the tick and the price the claim states, as
`KpfCrossC` did: the script holds the ticket to its pinned template and reads the entry's arguments from the ticket's
signature script, and the ticket's own script checks the seats' signatures. So nothing is minted, the roster and its
tickets are the committee family's as they are, and a library's scripts name no roster (the proofs are compiled
with the ticket's template, which is one for every roster). The price of it is the size of a proof, paid only when a
claim is disputed:

| Proof | Transaction | Fee (KAS) |
|---|---|---|
| a close, one leg (one ticket, three seats' signatures) | 8,375 | 0.0168 |
| a close, two legs (two tickets) | 12,816 | 0.0256 |
| a liquidation, one leg | 8,680 | 0.0174 |

A proved close is paid at once (its claimer the amount and both bonds); a proved liquidation becomes a proved claim
holding one bond less, the liquidator having the disputer's. Held in the engine with the committee's real signatures
(`crates/kpf-protocol/tests/thin_proof.rs`): the rule's amount is proved and a sompi more or less is not; the rule's
liquidation is proved and a fee or a cure a sompi off, the other side, a price that liquidates nobody and an account
that has its maintenance are not. The tool takes a claim's tick and prices from the seats' signed files when they
are given (`acct-declare --ticket1 --tick1 ...`), and warns of a claim typed in without them: a claimer that cannot
show the signatures within the window loses its bond, true claim or not.

What replaces the lease (`KpfTQuiet`, `KpfTQuietEnd`, `KpfTQuietRefute`; built the same day): no renewal. An
account pays nothing and writes nothing while nothing happens to it. For a committee that stopped signing there is an
exit by declaration, as everything else here: a party signs "no price of one of the account's markets since the DAA
`since`" and posts the bond; the state is `blake2b(int 2 + push(account) + side, since, bond)`.

| Step | Who | What | Transaction |
|---|---|---|---|
| quiet | a party, signed, with the bond | the account becomes the claim | 2,170 |
| refute | anyone | a ticket of each leg's roster spent by its attest for a tick at or after `since` (the ticks may differ a leg): the account again; half of the bond to who showed them, half to the other party | 11,701 (two legs) |
| settle | anyone, the claim as old as the account's quiet time | each party its margin, the claimer its bond with it: the settlement at the entries | 1,510 |
| early | anyone, a claim that landed before its own `since` | it says nothing of the past: ended as a refuted one | 2,246 |

The quiet time is a term of the account (the `expiry` field of the state, which this family reads as a duration; a
day by default: `acct-open --quiet-daa`). A quiet market is then no transaction at all, and no income to the
committee from it. A party that would rather leave at its entries than at the market pays the bond to try, and loses
it to the first executor that shows the committee's signatures; while the claim stands the account takes no other
claim, which is what the bond's size has to cover. In the engine with the committee's real signatures: the
settlement and its window, the refutation with two tickets, one market's price refused for an account of two, a tick
older than `since` refused by the tool and by the script, a claim of a time to come ended.

Review T1 (21 November 2026) of this family was not completed: `docs/audit/REVIEW-T1.md` is a reading of nine of
the sixteen scripts with no test, and it did not report on what each entry pays. It is no clearance. Three of its
observations were acted on:

- **A tick to come.** A claim could name a tick that had not happened, whose price no watcher can check. A claim
  that landed before its own tick is now neither settled nor disputed (`require(landed >= t0)` in `KpfTSettle`,
  `KpfTDispute`, `KpfTLiqSettle`, `KpfTLiqDispute`) and is ended as a late one (`KpfTLate`, `KpfTLiq.late`).
- **A bond of nothing.** `dispute` asks for a bond above zero, so a claim whose bond rounds to nothing could not be
  disputed and would be paid as stated. `KpfTDeclare`, `KpfTLiq` and `KpfTQuiet` now refuse to make one, and the
  model refuses the account.
- **The ticket's life.** A proof spends the ticket of the claim's tick, and a ticket can be retired one epoch after
  its epoch ends. A true claim disputed late in its window could then not be proved. The rule is
  `lateDaa + 2 x window <= the roster's epoch`; the tool refuses to state a claim at a ticket's tick whose proof could
  come after the ticket's life (`acct-declare`, `acct-liquidate` with `--ticket1`). No script can hold it: an
  account does not know its roster's epoch. With the testnet roster's epoch of 3,000 DAA and `lateDaa` 600, a window
  is at most 1,200 DAA.

Left as they are: a claim that states less than the rule gives is not provable (the proof asks for the exact
amount; the tool states the exact amount); a late claim is ended with its bond back, so a party can hold its account
by late claims for the fees alone, each one ended by whoever looks.

What the review did not reach was then checked here, by tests that stay in the suite
(`crates/kpf-protocol/tests/thin_builders.rs`, `thin_proof.rs`): every transaction the builders make is run again
with a sompi less at each output but the change, and refused each time (the close by both aside, whose outputs are
the two signers' to choose); and on 160 accounts, prices and sides drawn at random the two proofs answer as the model
does (117 closes proved and 43 refused; 41 liquidations proved and 119 refused; a sompi beside the rule refused every
time), which holds `KpfTLiqProve`'s own form of the arithmetic to the model's.

Review T1b (`docs/audit/REVIEW-T1b.md`; 26 tests, 2,313 changed transactions in the engine) was a second, narrower
one by a fresh reader: the trust in a library's id, one kind of object shown as another, the windows, the state
codec. Everything it tested of those held, and it found four things, each with a test. All four are fixed:

| | Finding | Fix |
|---|---|---|
| F1, High | **One output paid two dues of one key.** An entry paid at `len - 1 - j` and, where it pays two parties, at the place before; the second place of the object at input j was the first of the object at j + 1. Two operations in one transaction, the same key owed at both: one output satisfied both scripts and whoever built it kept the smaller due (a lapse beside a settlement: 250 KAS; a cure beside a lapse: a disputer's own 500 KAS bond) | Every place counted from the end is `len - 1 - 2j` and the one before it: two an object, shared by no two |
| F2, High | **A side held its account for nothing by liquidating itself.** `liquidate` is unsigned and states its own cure; `cure` returned every bond in any state. A side declared its own liquidation with a cure of a sompi and cured it before the window ended, round after round: the account was a claim, which takes no other operation, for 147 of 180 DAA at 0.011 KAS a round | A claim nobody disputed is cured once it is half a window old; a disputed one is not cured (its liquidator proves it, and the side cures the proved claim, or it lapses and the liquidator loses its bond) |
| F3, Medium | **A claim of a tick long past, or to come, cost its maker the fees.** It is neither settled nor disputed, so its bond was never at stake, and `late` returned it whole | `KpfTLate` and `KpfTLiq.late` share the bond as an early no-price claim's: half to who ends it, half to the party it was made against |
| F4, Medium | **A library is what its genesis wrote, and nothing read a genesis.** One output more under the id, of a script that leaves true, spent an object of each kind to a key with no signature | The tool reads the genesis (`thin_build::lib_check`, `lib-check`): every output under the id pays to a pinned script, every script is there, and the id is the covenant id of exactly those outputs. `acct-open` takes the genesis transaction's file and refuses a bare id unless told `--lib-unchecked` |

What F2's fix costs an honest side: it cures between half a window and the window (after which anyone settles),
and it cannot cure while a dispute it did not make is unanswered. What F3's costs an honest claimer: a claim that
takes longer than `lateDaa` to land loses its bond, so the tool's claims are stated at a recent tick. The scripts
cannot hold F4: an object accepts whatever carries its library's id, and a script cannot see how the id began.
Rounds 2 and 3 of the same report tested the fixes (39 tests in the end; every invariant of round 1 holds again on
the changed scripts). F1, F3 and F4 were closed at once. F2 was not: the fix had left, and eased, another route.

- **F2b, High (round 2).** A side that liquidated itself under a second key and disputed itself under a third had
  both bonds back at the lapse: the disputer's half and the side-named's half were both its own, and a disputed
  claim could no longer be cured or disputed by anybody else, so the account was held a whole window a round for the
  fees. Fixed: the half of a false liquidator's bond that went to the side named goes to nobody. It stays in the
  script's own copy (`KpfTLiqDispute.lapse` and `KpfTLiq.late` ask the script's output for that much more), and the
  transaction is an output smaller. The declared close keeps its split: there the half goes to the party the claim
  was made against, who is never the claimer.
- **What holding an account costs after round 3** (W the window, b the bond), as the reviewer measured it: by a
  false self-liquidation that a watcher disputes, b; by a self-liquidation and self-dispute, b/2 a window; by a false
  close disputed by the claimer's own key, by a claim of a tick long past, by a no-price claim ended by the claimer's
  own key, b/2 to the other party; by a false self-liquidation cured after half a window where nobody looks, the
  fees, as any claim nobody looks at.
- **F5, Low (round 3), left as it is.** A side that is truly below its maintenance can be its own liquidator, its own
  disputer and its own prover: the claim is true, so nobody can call it false, and it stands three windows instead
  of one before the side must cure, for 0.03 KAS of fees. The fix in the manner of F2b (half of a disputer's bond
  left in the script at the proof) would halve what an honest liquidator has from a frivolous dispute.
- **Not reached by the review:** anything that needs a mempool (who wins an account again after a lapse, a cure or a
  late end; a party that adds a sompi to its own margin over and over to keep the account's UTXO moving), three
  operations in one transaction, the edge amounts of the three proofs, the command line of the proof and of the
  refutation, and how what is left in a script's copy is ever collected (it is not: nothing spends a script's
  copy but an operation that writes it again).

No script ties what an object holds to its state (the slot holds a hash). Only an opening, or both parties
together, can write an object that holds less than its margins; the tool refuses to read one
(`TObj::least_value`).

Orders between users (`KpfTMatch`, `KpfTCancel`; 21 November 2026). An account is two parties', and the family
pairs users with each other, one to one: an executor matches and watches, and holds no position (it stands in the
middle only where it chooses to: an instant exit it fronts, two positions it merges into one account). An order is an
object like any other: a user's margin and a tip for whoever matches it, under a covenant id of its own. Its state:
the hash of its *template* (the account it would become, all but the two keys, the size, the entry and the two
margins: the terms, the leg's market, roster, scale and price limit, an empty second leg, and the quiet time,
`lateDaa`, the window and the bond rate; 321 bytes shown to the match), the owner's key, the side, what is left of
its size, its limit price, the margin and the tip for what is left, its expiry, whether what a match leaves rests,
the least a match takes of it, and the least margin it asks of the other side for what is left (`ask`).

| Step | Who | What | Transaction |
|---|---|---|---|
| place | the owner | the order, holding its margin and its tip | 353 |
| match | anyone (the matcher) | a long order and a short one of one template, a size both have left and neither's least fill forbids, a price not above the long's limit and not below the short's: an account under the long order's id, the long's owner its A, holding `n / size` of each order's margin; what is left of each order rests under its own id (or is returned, if the order says so); the tips' shares to the matcher | 3,705 |
| cancel | the owner, signed | the order back | |
| expire | anyone, from the order's expiry (the lock time proves the DAA) | the order back to its owner | |

A sell of 10 against buys of 3, 2, 4 and 2 is four matches and four accounts (3, 2, 4, 1), and 1 of the last buy
rests (the test `orders_of_two_users_match_one_to_one_and_what_is_left_rests`, every transaction through the
engine). What follows from one to one: the seller holds four accounts, with four counterparties, each settled and
liquidated on its own, and one order can become a hundred. No script grows with it: a match is one transaction of
one size, an account one object of 204 bytes; what grows is their number, and the fees of closing each. So an order
states the least a match takes of it (`least`: a match takes that much or all that is left; 10,000 with a least of
2,000 is five accounts at most), and a rest too small to be an order (below 0.2 KAS) is not left: the matcher fills
it whole or the order returns its rest. An account of a match shares its covenant id with what rests of the long
order; every operation asks for one input and one output of an object's id, so the two are never spent together.
Run on testnet: `docs/LIVE-4.md`, fourth part (a match about 4,500 bytes and 0.012 KAS; a merge 3,016 bytes and
0.0083 KAS).

Review T1b, round 4 (the orders; `docs/audit/REVIEW-T1b.md`) and what was changed:

- **F6, High, fixed.** An order stated its owner's margin and nothing of the other side's: a sell with 60 KAS of
  margin was matched against a buy with a sompi, an account in which one party can lose nothing. An order now states
  the least margin it asks of the other side (`ask`, the last integer of its hash; the tool's default is its owner's
  own margin). A match takes each order's share of its ask as of its margin and refuses a side that posts less; what
  rests carries the rest of the ask. Round 5: closed at the script, the model and the tool, and it holds across
  partial fills (the rounding never lets a side under the ask).
- **F7, Low, the tool.** A script cannot show that a DAA has *not* come, so an order past its expiry is still matched
  by whoever is faster than `expire`. `order-match` refuses it when it knows the time (`--past-expiry` to insist).
- **F8, Low, the tool.** An order without a least fill is cut up, or ended, a unit at a time. `order-place` gives one
  by default: the whole of an order that does not rest, a tenth of one that rests.

The book (`KpfTPlace`; 21 November 2026, the owner's: "a covenant id inherits its origin"). An order or an account is
a UTXO anyone can write, in anybody's name and at any entry, and a script sees a state, not who agreed to it. Two
operations put two objects together, and both were open to an object one party wrote alone (F10, F11 below). So every
order and account of a library is under **one covenant id, its book**. An output under an id is written only by a
transaction that spends an input of that id; the book begins as anchors (objects whose state is nothing but their
kind), which one script alone spends, and writes again beside what enters:

| Entry | Who signs | Under the book's id, in order |
|---|---|---|
| `place` | the order's owner | the order (at least its margin and tip), the anchor |
| `open` | the account's two parties | the account (at least its two margins), the anchor |

Whatever else is under the id was written by a script from what was there: an account by a match of two orders their
owners signed, inside both limits and what each has left; a claim from an account; and so on. Every entry of every
script fixes how many outputs it writes under the id and what each is (`KpfTBoth` did not, and now writes none). An
object written beside the book is of another id, and the scripts that take two objects take two of one id: there is
nothing to tell a false account by, because one cannot be in the book. KCC-20 holds a token the same way. A book's
genesis is read and held to anchors (`book-create`, `lib-check --book`, `--book` on `acct-open` and `order-place`).
What it costs: an order's placement spends an anchor and a copy of the script (the anchors are the number of
placements a block takes), and one object operation a transaction (each asks for exactly its own inputs of the id).
What it changes for a user: its accounts with one counterparty are one pot whenever anyone merges them (below).

Review T1b, round 8, on the book: every one of the 26 entries of the 20 scripts fixes the inputs and outputs of the
object's id (about a thousand changed transactions at the engine: an output more under the id, by any input, is
refused everywhere); objects of two ids are not put together; an anchor is spent by `KpfTPlace` alone. Two findings,
both Medium. **F14, fixed:** `--book` was optional on `acct-open` and `order-place`; a book whose genesis wrote an
account beside its anchors is the false merge again, so the book's genesis is now checked as the library's is
(`--book-unchecked` to insist). **F13, the design's consequence, accepted by the owner (21 November 2026):** an
order's margin is not the most it can lose. A buy resting at a stale limit with 2 KAS of margin, sold into after the
market fell a tenth, has lost 10; apart, its owner loses the 2; once anyone merges that account with another of the
same two users (30 KAS each), the owner is owed 22 of its 30. The same through a novation against an opposite
account of the owner's with anyone. In one market, a user's accounts are pooled by default (the word cross margin is kept for an account of two
markets); `order-place` says so. Run on testnet: `docs/LIVE-4.md`, fifth part.

Two accounts into one (`KpfTMerge`; 21 November 2026). Orders pair users a fill at a time, so two users come to hold
several accounts with each other. Anyone merges two accounts of the same two parties and of one book (the same
template, the keys in the same places, both long for their A, one leg each): the sizes, each party's margins and
what each asks of the other's together, and for the entry the largest price at which the whole is worth no more than
the two legs were at theirs, the few sompi by which that lowers A's cost moved from A's margin to B's. Nobody signs.
Whoever sends it keeps `MERGE_TIP` of what the two held (0.01 KAS, a constant of the library, half from each party's
margin; it stays in the sender's change). Run on testnet in its first form, before the book (`docs/LIVE-4.md`,
fourth part: 3,016 bytes, 0.0083 KAS).

- **F11, Critical, fixed (found after round 5, which had passed the merge; in the form run on testnet).** "At every
  price each party is owed what the two accounts owed it" was checked (4,000 random pairs, a chain of 200 merges, no
  drift) and is true of the worth *before the cap*. Apart, a false account's gain is capped by its own pot, its
  maker's money; merged, it is paid from the other party's real margin. Five real units with 200 KAS each, ten false
  ones from a quarter of the market's price with a KAS each: merged, the reviewer's numbers are 74 of the other
  party's 200 KAS. First fixed by both parties' signatures (round 7), then by the book, which needs none.
- **F9, Low (round 5), closed.** A party cannot make an account of the two alone inside the book.
- What a merge changes: the pot is larger (a party's worth is capped later; the bond is a share of a larger pot), and
  the whole is liquidated on the whole. Since anyone merges, two users' accounts in one market are pooled by default
  ("cross margin" is kept for an account of two markets). A declared claim or liquidation is not an account and is not merged.

What an account carries of its orders (21 November 2026). The moving part is 138 bytes: after the bond rate, what A
asked of its counterparty's margin and what B asked of A's (`KpfTMatch` writes each order's share of its `ask`; an
account the two opened directly asks what the other posts unless `acct-open --a-ask --b-ask`: F12, Medium, round 7,
an account that asked nothing had its counterparty replaced by one without margin). A merge adds them. They are what
a party that never signs again is held to when its counterparty changes.

A user leaves; the two it faced face each other (`KpfTNovate`; 21 November 2026). U is long n against S in the account
X (from the entry ex) and short against L in the account Y (L long from ey): it bought at ex and sold at ey. For the
units both accounts have (all of the smaller), L is now long against S from ey; S's margin gives up
`tr = quote(n, ey) - quote(n, ex)` (or takes it, when negative), so that S and L are owed at every price what they
were; U is paid its two margins' shares and tr, less `NOVATE_TIP` (0.015 KAS, the sender's). Under the book's id, in
order: what is left of X, the new account, what is left of Y. If S and L are one user (two opposite accounts of the
same two), nothing is written: both are paid. No price is shown and nobody signs: a user that wants out places an
order as it would to open, and an executor matches it and takes it out. 3,947 bytes in the engine.

- **F10, Critical (round 6), fixed.** S wrote a Y in which U "sold" far below the market, to S's second key, and
  took U out at that price; the reviewer's command-line test moved 150 of U's 200 KAS. The payment went to U's own
  key: what was false was the amount, that is the entry of Y. First fixed by U's signature (round 7), then by the
  book: U is in Y by its own order, inside its limit.
- **S and L** are held whole by this: the worth of each is the same line before and after, and two rules for the
  cap: L's margin is at least what S asked, and tr more when tr is S's loss (U and L can agree any entry between
  themselves, so tr can be made as large as S's margin: this makes L's own margin stand behind every sompi of it);
  and S's margin after tr is at least what L asked. Who asked less than its counterparty posted can lose the
  difference through the cap (the reviewer's 1,500 random novations: nothing lost by who asked as much).
- With the tool's default (an order asks as much margin as its owner posts) a novation passes when the price has not
  moved or has moved against U; after a move in U's favour the one that stays is short of what the one that comes
  in asked, by tr. Orders that ask less than their own margin are taken over in both directions.

Two markets into one account (`KpfTJoin`; 22 November 2026): cross margin. The same two users have an account in each
of two markets, of one book and the same terms, one leg each: joined, they are one account of two legs (the form the
claims and their proofs have read since section 3b), one pot of margin for both markets, closed and liquidated on the
whole. The first's leg stays; the second's becomes the second leg, as it was when the two keys are in the same
places, short for the first's A when they are the other way round. Each leg keeps its size and its entry, so at
every pair of prices each party is owed, before the caps, what the two accounts owed it, to the sompi (the
reviewer's 2,000 random pairs at 40 pairs of prices). Margins and asks of each user are put together; whoever sends
it keeps `MERGE_TIP`. 2,598 bytes in the engine. **Both users sign.**

- **F15, High (round 9), fixed by those signatures.** The first join was anyone's. An account of two legs is read by
  no merge, novation or join, and nothing splits it: a third key, for the 0.005 KAS the tip left it, ended both
  users' way out by a sale, in both markets, for good; what was left was a close of the whole at the committee's
  prices (the tolerance on both notionals to the other party, a bond on the whole pot) or the other party's
  signature. It also carried an order's loss beyond its margin into another market's account (F13 across markets).
- Sizes (the owner, 22 November 2026: "we made it thin and it is heavy again"). One object: 1.5 to 2.4 kB. A
  placement 1.7 kB (it was 2.2 with both entries of the way in in one script). A merge or a join 2.7 to 2.8 kB. A
  novation 3.9 kB. A match 3.7 kB, after two things were taken out of it (it was 4.1): the output for the tips (their
  shares are written nowhere, and whoever sends the match has them in its change; the key they were paid to was an
  argument nobody signed, so a copied match took them before as well) and the order that does not rest (what a match
  leaves always rests; an order to be taken whole says so by its least fill, as `order-place --no-rest` now writes
  it). Not done, each for a few per cent: showing the state without the template and the empty second leg (the
  three proofs would be rewritten), a smaller slot (every script), one entry a script for the five that have two.

One side of an account of two legs (`KpfTTurn`, `KpfTFold`, `KpfTLeave`; 22 November 2026; the owner: "it feels very
wrong that one side of an account of two cannot be done").

(The sizes in this table are from before the dues: every script, and so every transaction, is 83 bytes longer
since 22 November 2026.)

| Script | What | Who | Bytes |
|---|---|---|---|
| `KpfTTurn` | The same account written the other way round: its two users in each other's places (each leg's direction turns), or its two legs. Nothing is owed that was not (2,000 random accounts: the same to the sompi, through the claims too). It is for the two below, which read the account's A and its first leg | anyone, once the account is 100 DAA old (whoever means to use the account is overtaken by a turn once at most) | 1,860 |
| `KpfTFold` | An account of the same two users (one leg, the market of the first leg) folded into that leg: added when it is the same way; set against it when it is the other, what cancels realized between the two margins and what is left keeping its own entry; when the leg is gone the second leg is the account's one leg. Margins and asks together. Before the caps each user is owed the two accounts' sum to a sompi (3,000 random folds) | anyone (it only pools, as a merge does: F13) | 3,300 |
| `KpfTLeave` | The first leg sold to a third user L, who faces the account's other user S from the entry of the account in which U sold. U is paid its margin in that account and NOTHING of the cross account: its margin there, with what it made on the leg, stays behind the leg it keeps. S's margin and the asks go with the leg in the share of its notional at its entry in the two legs' | **the account's two users sign**; L does not | 4,330 |

- **F16, Critical (round 11), fixed by the two signatures.** The first leave was anyone's, and "U is paid nothing of
  the account" did not hold it. The two legs' gains and losses are set against each other in one pot; a leg that
  leaves takes its own out. A user handed the losing leg, at its own stale entry, to a second key with as much
  margin as the share asked, and kept the winning one: what the other was owed on the loser was capped there (10 of
  55 KAS in the reviewer's example, up to 43 over a grid, no signature of the other). **F17, Medium:** the same from
  the other side (the other user's second key takes the seller's resting order, and the other's margin leaves with
  the leg from under what it owes on the leg that stays). Asking the one that comes in for the WHOLE ask does not
  hold it either: a gain of 60 on the leg that left against an ask of 40, and a loss of 40 on the leg that stayed,
  was 20 owed and is 0. What is lost is the netting, and only a price says how much: no rule without a price holds
  both users whole, so both sign, as for the join.
- **F18, High (round 12), fixed.** F16 again by two steps that were each nobody's signature: a user sold its winning
  leg into the other's own resting order; anyone folded that account in (the leg cancels, the gain is in the user's
  margin, the loser is the account's one leg: nobody is owed less yet); the user was then taken out by a novation
  against a second key that posted what the other ASKED, which the fold had left as it was (20 of 75 KAS in the
  reviewer's example). What one margin takes from the other inside an account is now added to what the payer asks
  of the taker's replacement, in the fold and in what stays after a leave (whose asks are also no longer shrunk).
  The property, which round 13 could not break (2,000 cross accounts, 3,989 unsigned steps from both sides, then
  the worst novation: never owed less, to the sompi): **an account whose users each ask at least what the other
  posts (the tool's default) stays so through every unsigned match, merge, fold and turn, and a novation takes
  nothing from such a user.** A user that asks less can be left short by the gap it chose (F12's shape).
- The line this drew (22 November 2026): an operation that only ADDS to a pot needs no signature; one that SPLITS a
  pot needs a price or its users' signatures (F15, F16); and whatever moves between two margins inside a pot is
  added to an ask, because the asks are all an unsigned novation reads (F18). A worth compared before the caps
  proves none of this: the four findings above all passed that comparison.
- What a user of a cross account does alone, then: adds to a leg and cancels against it with the same counterparty
  (an order of each, matched: anyone folds it), and closes the whole at the committee's prices. To sell one leg to a
  third user it needs the other user's signature.

Separate machines (22 November 2026). Every live run to the sixth part had its keys and its files in one folder. Two
things did not exist for users whose keys are on their own machines, and are tool, not script (no script changed).

- **What moves is found at the node.** A script's copy and an anchor are spent and written again by every
  transaction that uses them, so the file of "the copy as it is now" is on the machine that used it last. But every
  copy of a script is at one address (its script is its own), and every anchor of a library at another; what tells a
  true one is its covenant id (the library's; the book's), which nothing but the genesis and the scripts themselves
  write. So `--script` and `--anchor` may be left out: one query of that address, the UTXOs under the id, one of
  them taken by the clock. `lib-status` lists them all in one query. Two machines that take the same copy at the same
  moment: the node refuses the second, which asks again. A library of N copies of a script serves N transactions of
  that kind a block.
- An order's or an account's file is not found: the chain holds a hash, and the state is in the file. It goes from
  whoever has it to whoever needs it (the matcher has both orders' files and hands the account's to both users),
  and whoever receives one reads it at the node against the library and the book it checked (`acct-show --lib
  --book --node`): its script is the one its state makes, its UTXO is there, and its covenant id, as the node says
  it, is the book's. (Reading a transaction's arguments from the chain into the files of what it made, with no file
  passed, is not built.)
- **What two users sign is signed where each key is** (`--partial FILE`, then `cosign`; an opening, a close by both,
  a join, a leg that leaves). The signatures are arguments of a script, and each covers the transaction's inputs'
  outpoints and all its outputs. Three things follow.
  1. *The budget.* An input commits to what its script costs to run, and that is signed; a script that waits for a
     signature cannot be run. The first machine measures it on a twin: the same transaction with a key of its own
     making in the place of every key (the same inputs, sizes and path), which it signs whole. The transaction two
     machines make is, to the byte, the one a machine with both keys makes (tested for the four).
  2. *What the second signer reads.* A signature does not cover what the file SAYS an input is, nor a script's
     arguments. So the tool takes nothing from the file but states, and holds each to a script: every input and
     output is a plain coin, a script of the library under the library's id, or an object whose state is in the
     file and whose script that state makes, under the one book of the objects spent; anything else is refused.
     Every input is then looked up at the node (one query): the outpoint holds that script, that value, that
     covenant id. What is printed is therefore what the outpoints hold and what the outputs are, which is all a
     signature is for. Where an anchor is among the inputs (something enters a book), the book is checked as the
     writer's tool checks it.
  3. *No last day.* Whoever holds the file with the last signature sends it when it likes, for as long as every
     input is unspent: a transaction is valid from a day, never until one. A split agreed at one price can be sent
     after the price moved. Whoever signed takes its signature back by spending an input: a coin of its own among
     them, or the account itself (`acct-add-margin` writes it anew). It is a race and no recall: whichever lands
     first stands. The tool says so when it writes the file.
  Review T1b, round 14 (nothing above Low): a signature made apart serves its transaction and its place only
  (moved, the transaction is refused; every argument changed after signing is refused; an output is of a book only
  if the transaction gives it that id). F19, Low, fixed: a file could list the co-signer's own coin among the
  signatures a script waits for, and the coin was then worded as already signed; such a file is refused. The lookup
  takes each input at its own address and no entry without a script.

Who watches (the owner, 21 November 2026): not one executor but ten or more, which compete for a false claim's bond
as they compete for orders. So the fixed part is published at the opening, any executor can test a claim, and the
first dispute to land has it. Two things follow. A claim's counterparty being away no longer leaves it unwatched. And
a dispute cannot be assumed to be honest: a claimer can dispute its own false claim with a second key and let it
lapse, which holds the account for two windows (no liquidation reaches a claim) and, if both bonds went to the
disputer, would cost it nothing. A lapse therefore splits the claimer's bond: half to the disputer, half to the
party the claim was made against (`KpfTDispute.lapse`), so that holding the account costs half a bond paid to the
party held. The half is the script's constant, not a term of the account.

What pays the seats (the owner, 21 November 2026): about what pays a miner, or less. A fee a claim would be near
nothing in the declared form (a signed price reaches the chain only in a dispute), and a rate on open interest is
far above that level. The proposal: every operation leaves a fixed amount, at most the transaction's own fee, in the
copy of the script it spends (`goesOn` asks for the input's value plus that amount, a constant of the library), and
the committee collects what has gathered there, by stakes. No output and no byte more: an output of 0.005 KAS could
not be written at all (its storage mass). A quiet market with open accounts still pays, by the renewals of their
leases. Not built: the collection is the thin roster's. What follows from the level: a seat's income does not repay
its stake, so the seats are held by those who need the prices, the executors. The committee's three clocks are not
one: it signs prices every few seconds off the chain, its roster rolls every epoch (3,000 DAA on testnet), and it is
paid when accounts are operated on.

Where the prices are to come from in the end (the owner, 21 November 2026): from the miners, in the blocks
themselves, and not from a roster of seats. The committee is what serves until then. The declared form leaves
little to move: an account, a claim, a dispute, a bond, a liquidation and its cure handle a tick and its prices as
numbers, and only the three scripts that answer a dispute read where a price came from (`KpfTProve`,
`KpfTLiqProve`, `KpfTQuietRefute`: the function `ticked`). A library with those three written for another source, and
accounts moved to it by both signatures, is the whole of the change. (This note went on: "so nothing new is built
on the roster". That was its author's inference, not the owner's word, and the owner corrected it on 22 November
2026: the committee is to be made whole. Its reasons: the move to another source goes better from a committee that
works, a conflict between the two may show only in use, a staked committee may show things a proof-of-work source
does not, and the two may run side by side in the end. The dues, the guard and the measure of what stands under a
roster, below, are of that day.) What a miners' source needs is not established. The
node has an opcode (`OpChainblockSeqCommit`) that, given the hash of a block on the selected chain and within some
depth, gives the block's commitment to what it accepted; a price a miner wrote in its block could in principle be
shown to a script through it, and several blocks' prices combined in the proof, which the declared form runs only in
a dispute.

Read in the node's source the same day (rusty-kaspa at the revision this repository builds against,
`consensus/seq-commit`, `crypto/txscript`, `crypto/hashes`; nothing of it run yet):

- **What the commitment covers.** `SeqCommit(B) = H(SeqCommit(selected parent of B), state_root)`, and
  `state_root = H(activity_root, H(context_hash, payload_root))`. `context_hash` is of the mergeset's timestamp, DAA
  score and blue score. `payload_root` is the Merkle root of one leaf for each block B merges:
  `H_leaf(block hash, the block's blue work, H(the block's miner payload))`. So a chain block commits to what every
  miner it merged wrote in its coinbase, with that block's work, and to the DAA score it was merged at.
- **Where a miner writes.** The coinbase's miner data is a pay-to script and `extra_data`, bytes of the miner's own.
- **Whether a script can follow it.** The hashes are BLAKE3 keyed by a domain string, and scripts have
  `OpBlake3WithKey` (the compiler knows it and `OpChainblockSeqCommit`). A script given the parent's commitment, the
  activity root, the context's three numbers and the leaves can compute `SeqCommit(B)` and compare it with what the
  opcode pushes.
- **How far back.** The opcode answers for a block of the selected chain within the finality depth of the block that
  holds the transaction (about twelve hours), which is far longer than a dispute's two windows.

So a proof "the miners merged at the tick wrote these prices" looks computable in a script as the node is, with no
change of consensus: the proof shows every leaf of the chain block's payload tree (so that none is left out) and
takes the middle of the prices by work. What is not established: that testnet-10 runs the opcode (to be tried with a
small script), the exact bytes of each hash's key and of the leaf's preimage as a script must write them, the size of
such a proof for a mergeset of ten blocks, and whether one chain block's miners are enough or several blocks must be
chained (more than a script may hold; the zero-knowledge opcode is the node's own answer to that). And none of it
makes a miner write a price: that is the miners' software and their reason to run it.

Contention: a script of the library is one UTXO, and two transactions that spend it cannot both be accepted, in a
DAG either; the one that loses is built again on the script's new output, within a second at ten blocks a second.
The library's genesis takes the number of copies of each script, and the tool takes another copy on a conflict. Open in this form: the library's
genesis and how many copies of a script it holds (a script is one UTXO, so two transactions of one operation in the
same moment contend for it), the liquidation and its cure, the lease, the price notes' minting by the ticket.

What follows from it, for a second form of this family:

- Positions carry no logic; one script an operation (or a few small groups of operations). A script that creates
  another object then needs that object's 200 bytes, not its code: the 35 kB of an open go with it.
- One position form. The account of 3b with one leg is a pair; orders and the hand-over move to it.
- A position names its allowed scripts, among them "both parties sign": that is the way to a later version of any
  script, which the pinned templates of today do not have (a change of one line of `KpfTicket` made every roster,
  ticket and pair of the old form an object of an older binary).
- The price proof is the part a thin position does not shrink: a ticket's script and fifteen signature slots are
  4.2 kB a market. It is taken out of the claim: an attestation is a transaction of its own that writes a small price
  note (market, tick, price) which the claims of that tick read and write back, so no transaction is large and what
  a price costs is paid once a tick. Seats signing one root over all their markets' prices a tick, and a ticket that
  carries only the signatures presented, shrink the attestation itself. Claims that present no signatures unless
  challenged are a further saving that adds a watcher, and can be one more script later.

The chain read back, and executors left running (22 November 2026). After the runs from separate machines two
things were still done by hand: the file of an object went from the machine that made it to the machine that used
it, and an executor matched, merged and took out what it was told to. Tool, not script (no script changed).

- **A transaction is read back** (`thin_read`). The chain holds an object's hash, but the script that ran was shown
  the object: every entry's arguments carry the moving part of the account and what the operation adds, and the
  input's own script carries the hash. So the objects a transaction spent are rebuilt from its arguments and held to
  the hash in the input, and those it made are computed by the builders' own arithmetic and held to the script of
  the output. Whatever is not rebuilt is reported as not read; nothing is taken from a transaction on trust. What
  the arguments do not carry: an account's fixed part where the script is shown its hash only (terms and legs: known
  from the account's opening, which publishes it unless `--private`, or from any earlier file), and an order's
  template at its placement (its hash; a match shows it whole, and the tool is given a market's with `--market1`).
  Every entry the builders' tests run is read back from its bytes there (23 entries).
- **Which transactions.** Every transaction of the family spends a copy of a script of the library and writes it
  again, and an entry to the book does the same with an anchor. Two things follow. While every copy and every anchor
  stands at the outpoint it stood at, nothing happened in the book: one list of UTXOs says so. And when one moved,
  the outpoint it stands at names the transaction that wrote it, and the outpoint that one spent names the one
  before: the book's transactions since the last reading are walked back from the present to an outpoint that
  stood (each step held to being a copy under the library's id or an anchor under the book's, since a transaction
  may carry an input that only looks like one).
- **What the node is asked for.** A node gives a transaction by its block, not by its id. So the tool asks which
  chain block accepted which id since the last reading (`getVirtualChainFromBlock`, ids only: about 70 bytes a
  transaction of the chain), then for the blocks that chain block merged until the transaction is found (a block:
  10 to 30 kB). The first form of this read every transaction of the chain whole
  (`getVirtualChainFromBlockV2`, as KOB's indexer does without pause next to a node of its own) and cost thirty to a
  hundred times as much (docs/LIVE-4.md, tenth and eleventh parts: 4 MB against 0.2 MB a reading, 25 to 85 MB
  against 0.5 MB for a user back after some minutes). A reading further back than half an hour is asked for by name
  (`--catch-up`), or skipped (`--from-now`).
- **`book-sync`**: a folder brought to the present. A user runs it to learn what became of its order or its account
  (a match, a merge, a novation, a claim made on it), with nobody sending a file; `--only KEY` writes the files of
  that key's objects only.
- **`exec-run`**: the executor's loop. A round asks the node three small things (the present, one list of UTXOs),
  reads as above if the book moved, and sends, a coin of the sender a transaction: orders that cross, matched for
  all the smaller has at the limit of the one whose output is older; a user taken out of two accounts that cancel;
  two accounts of the same two users merged or folded; a claim that landed late ended; a claim it disputed ended
  after the window. Each of the first four only if it leaves the sender richer after the fee (`--min-gain`).
  Asked by name: a claim or a liquidation whose amount is not the rule's at its own prices disputed (its claimer
  cannot prove it), a true claim settled, an expired order returned. A transaction the node refuses (another
  executor was first) leaves its objects alone for three rounds; a node that fails is asked again later and the
  loop goes on.
- **What an executor earns** (measured): a match, the two tips less 0.0072 KAS; a merge 0.0045 KAS; a novation
  0.0073 KAS; a late claim ended, half its bond; a false claim disputed and lapsed, half its bond. An order with no
  tip is matched by nobody that looks for a gain, and the tool says so when the order is placed.
- **Limits.** One transaction a coin and a copy a round (no transaction is built on one not yet in a block). The
  price of a match is the limit of the order whose output is older; a partial fill writes the rest anew, so "older"
  is the age of what is left, not of the first placement, and a user makes its own order the younger by a small fill
  with a second key. That is no more than the script gives whoever sends a match (any price between the two limits):
  the rule binds this tool, and a limit is what an order is sure of. A claim at a false price whose amount is right for that
  price is not disputed (that takes the committee's ticks: not built into the loop). A window shorter than a round
  and a block cannot be watched (a round is 10 s at least). An account opened with `--private` and never shown
  whole is not read; its two users have its file. The walk reads what moved through copies that still stand; a
  folder that begins (`--from-now`) knows what entered the book before only from files.
  Review T1b, round 16 (nothing above Medium). F20, Medium, fixed: the reader took a script's objects in the order of
  the inputs, and the scripts take them by the places their arguments name, so a match or a fold sent with its two
  objects the other way round (by anyone, at the cost of its fee) was read with nothing made, and every folder lost
  the files of what it made. The reader now takes them as the script does, reads every script a transaction runs,
  and at an opening tries the fixed part it knows beside the payload. F21, Low, fixed: orders that cross without a
  tip were built and left alone every round; a match whose tips cannot reach the gain asked is not built, and what
  was left alone is not built again. Rounds 17 and 18 (both fixes hold; the workspace 1,027 tests): what is left
  alone is remembered by its script and its objects (a novation that does not pay hides no fold of the same two
  accounts), and what a used-up order holds beyond its margin and tip counts toward a match's gain, as the match pays
  it to its sender.

The dues (22 November 2026). What pays the seats, at the level the owner set (about what pays a miner, or less), and
set by this tool's author where the owner left it open ("set the committee well"): the amount, who collects, when.
Scripts changed: every script of the library, and three new ones. Review T1b, round 19: one Medium and two Low, all
three fixed (below) and verified in round 20; nothing found against a copy or the purse being spent under no rule.
On testnet the same day, in a new library: an operation's dues, a sweep by the executor's loop and a payment to the
three seats' keys (docs/LIVE-4.md, twelfth part).

| What | Set to | Why |
|---|---|---|
| The dues of an operation | 0.002 KAS, a constant of every script (`DUES`) | an operation pays its miner 0.004 to 0.008 KAS: less than half of that |
| Where they gather | in the copy of the script the operation used | no output and no input more: an output of 0.002 KAS cannot be written (its storage mass) |
| Who moves them | anyone (`dues-sweep`, and `exec-run` by itself), a copy at a time, once it holds 0.2 KAS above what a copy keeps (100 operations) | a sweep takes the copy for a block: it must be worth its fee, and cost something to repeat |
| Whose they are | the seats of one roster, named at the library's genesis, each by its weight (`dues-pay`, anyone) | the roster's table is on the chain (a ticket); weight is what a seat has at stake |
| What the sender keeps | a twentieth of what it moves, of a sweep and of a payment | its fee and a reason: a sweep of 0.2 KAS leaves about 0.003 KAS, of 0.5 KAS about 0.02 KAS |
| How often | no interval: when it pays its sender | a script reads no clock |
| A quiet market | pays nothing | no operation, no dues (the quiet time took the place of the leases) |

- **`goesOn`** (every script but the two that collect): the copy's output holds what its input held and `DUES` more.
  The two entries that leave half a bond in the copy (`KpfTLiq.late`, `KpfTLiqDispute.lapse`) leave it with the dues;
  that half was nobody's, and is now the committee's.
- **`release(l, k)`** (the same 25 scripts): the one way value leaves a copy. It asks what `KpfSlot.go` asks of an
  object, and no more: an input under the library's id whose signature script begins with the 24 bytes of the
  places names this input at place k. That input is an entry of a script of the library, and every entry holds each
  place it names to the hash of an object's state (a copy's bytes there are the script's own code: no state hashes
  to them) or to the same input as a place it reads; only `KpfTSweep.sweep` passes with a copy at a place, and it
  answers for the copy: the same script, the library's id, at least the floor. A script is 83 bytes longer for the
  dues and this entry (`KpfTAdd`: 475 to 558), and so is every transaction (an added margin: 1,509 to 1,610 bytes;
  a match: about 3,650 to 3,756).
- **`KpfPurse`**, where the dues gather: one object under the library's own id, written at its genesis (nothing is
  written under the id after it). Its state is `blake2b(push(8) + push(roster id) + push(floor))`: whose the dues
  are, and what a copy and the purse go on holding, which is what the genesis gave each (a genesis of many copies
  needs more than 1 KAS a copy for its own storage mass, so the floor is the library's and not a constant). It is
  `KpfSlot` with the id it carries in the place of the library's: an object of a library cannot be written at the
  library's genesis, since its script would hold the id that genesis makes. The genesis' payload names the roster and
  the floor (`KPF\x02`), and `lib_check` accepts one purse, the one the payload names, beside the scripts. A purse
  moves no object: an object is moved by an input whose signature script begins with the places, and a purse's
  begins with a number. The scripts' bytes stay the same in every library.
- **`KpfTSweep.sweep(who, ageb, slotLen, rid, floor)`**: who names the purse and a copy. The copy goes on at exactly
  its floor (F24, Low: left above it, a copy was swept in pieces, each for the sender's part and a block of the
  copy's); what it held above (0.2 KAS at least) reaches the purse less a twentieth. The purse, the sweep's own copy
  and the copy swept are all the transaction holds under the library's id (found before any review, by a test that
  was accepted: two sweeps in one transaction each asked the purse for a growth that one growth answered, and the
  sender kept the smaller copy's dues whole; a payment is alone with the purse likewise). 1,872 bytes for a copy of
  `KpfTAdd`, fee 0.0043 KAS; 3,448 bytes for one of `KpfTMatch`, fee 0.0069 KAS.
- **`KpfTPay.pay(who, ageb, slotLen, rid, floor, payer)`**: the transaction is the purse, the script, a ticket and
  one coin of the key `payer`, in that order, and nothing else (F22, Medium: each seat's output is held to a key
  and an amount, so an output another script of the transaction asked for the same key answered both; two
  libraries with one roster were paid by one set of outputs and the sender kept the smaller purse: 13 KAS of 32 in
  the review's test). Input 2 is a ticket of the purse's roster spent by its
  `attest` (read as the proofs read one), for any tick of its window a quorum signed. What the purse holds above
  its floor, less a twentieth, is shared by weight in units of 0.01 KAS: each seat with a weight has an output to
  its key, in the table's order from output 3, and what the rounding leaves stays in the purse (F23, Low: it was
  the sender's, up to 0.01 KAS a seat beyond its twentieth). Every seat in one transaction or none, so the purse is paid when the smallest
  share can be an output, and no seat is paid ahead of a smaller one. 7,439 bytes for three seats (fee 0.0149 KAS),
  8,011 for fourteen (0.0305 KAS). With the three proofs it is the fourth script that knows where prices come from:
  a library for another source replaces four.
- **What an executor earns** is less by the dues of the script it used: a match the two tips less 0.0095 KAS (fee
  0.0075 and dues), a merge 0.0024 KAS, a novation about 0.005 KAS.
- **Limits.** (1) A seat is paid for its weight whether it signs or not: paying those whose signatures the payment
  shows would let whoever sends it leave the others out. (2) The table is that of whatever ticket under the roster's id is shown: an old epoch's ticket
  stands until somebody retires it, and pays the seats of then. (3) The roster is the genesis': a roster that turned bad is paid by this
  library until its accounts are elsewhere (a library is one transaction; an account moves by both signatures). (4)
  A library made without `--roster` has no purse, and its dues stay in the copies for good. (5) A payment of more
  than 300,000 KAS leaves the rest for the next. (6) A sweep takes a copy and the purse for a block, and a copy is swept
  once for all it holds: to do it again somebody has to have left 0.2 KAS more in it (the dues of 100 operations, or
  a gift, which is the committee's). (7) Every executor reads the chain when anybody sweeps or pays (a copy moved):
  one transaction fetched, nothing of a book in it. (8) The purse's roster is the choice of whoever made the
  library: nothing ties it to the rosters of the markets traded there, and `lib-check` prints it.
- **What the dues do not do: make a committee honest.** At this level a seat's income is nothing beside what a lie
  moves, so two things are held by other means, and one is not held at all. *A quorum that signs a false price*
  (67 % of the weight) takes every account of its markets: answered by the stake (two prices for one tick forfeit
  it at once; a false price is charged, judged by another stake, slashed), and by nothing that caps open interest
  against that stake (section 7). *A third that stops signing when it is losing* stops every claim: the winner has
  the last signed tick for `lateDaa` (10 s by default on testnet) and after that only the way out at the entries,
  so a party that is not watching loses its gain to a counterparty that holds a third of the roster. Silence is not
  punished. Nobody is removed by name: a seat leaves by being slashed or outbid, and users leave a roster by not
  opening accounts under it. Built the same day, in the tool and not in scripts: the next two notes.

The guard (22 November 2026): `exec-run --do guard --ticks DIR --ticket FILE`, for the accounts the loop's key is a
party of. No script changed.

- **What it answers.** Seats holding a third of a roster stop every claim by not signing. They do it when they are
  losing: the winner has the last signed tick for `lateDaa` and after that only the way out at the entries. The
  guard is the winner's machine doing in that time what the winner would.
- **What it does.** Each round it reads the folder the seats' signed prices arrive in (the files a seat's runner
  writes, named by their tick) and takes the newest tick, not after the present and not older than `lateDaa`, for
  which a quorum of the roster signed a price of every market of the account (a ticket's seat table says what a
  quorum is). Of the prices seats signed for that tick that a quorum stands within the spread of, it takes the one
  that gives its side most: whoever claims chooses among them. When that tick is
  `--silence` DAA old (a third of `lateDaa`, unless given) and the account is worth more to the key at that price
  than its margin (by more than `--guard-gain`), it declares the claim the rule gives at that tick, with a fifth of
  `lateDaa` left for it to land. Past that it says the price can no longer be claimed. A claim of the key that is
  disputed is proved at exactly the price it states, with every signed price of its tick the machine has seen and
  a live ticket of the tick's epoch, found at the node. A claim of the key is settled after its window.
- **A tick of a time to come is not counted.** Seats that sign one and then stop would otherwise look as if they
  were still signing (found by its author before the review).
- **Review T1b, round 21** (three High, one Medium, one Low, all in the tool and all fixed; each cost the user the
  gain the guard is for, or its claim's bond). F25: the guard looked at the sixty newest ticks by name, so sixty
  ticks no quorum signed hid the last one a quorum did; it now reads every tick of the time a claim may land in.
  F26: it proved only if the price it would collect now was the claim's, and a third seat's price arriving within
  the spread moved that price; it now attests exactly the claim's price from the seats within the spread of it.
  F27: a seat's second price for a tick took the place of its first (by its file's name, or by the order of
  reading); every signed price read is now kept under a name of its own (`DIR/prices-kept`), each seat's price
  nearest the price wanted is used, and a seat with two prices for a tick is named (the two are proof against it).
  F28: the tickets were read once, so the guard was blind from the roster's next roll; they are read every round,
  and the tool says when the ticks it has are of a time no ticket given covers. F29, of the measure below: a second
  match of the same round passed the limit the first had filled; what a round matched is counted.
- **Limits.** The guard closes the account: it is a stop at the last signed price, not a hold. It cannot tell seats
  that stopped from ticks that stopped reaching this machine, and does the same in both cases (the claim is at a
  price a quorum signed either way). It takes the newest tick a quorum signed, not the best of those still claimable. The folder is filled by a
  transport that is not this tool's: files that are no signed price cost time, and nothing else. It needs a
  coin that holds the claim's bond. A round is 10 s at least: with `lateDaa` of 100 DAA (10 s) there is no time to
  guard in; the testnet market's 600 DAA leaves about 40 s. A proof needs a ticket of the tick's epoch to be at the
  node: tickets kept busy by somebody (section 7) delay it.

What stands under a roster (22 November 2026). With a ticket given (`--ticket`, `--tickets`), a round of `exec-run`
says, for each roster, how many accounts of its markets the folder holds live and the sum of their margins, beside
the roster's stake (the sum of the seats' weights in the newest ticket given); it says so again when it changes.
`--max-exposure R` leaves unmatched the orders whose two margins would put that sum above R times the stake. It is
this executor's own limit, as a limit on what it holds itself would be: no script counts what stands under a
roster, and another executor matches what this one leaves. The sum is of what this folder has read, so it is a
floor. What the measure is for: a quorum that lies takes the margins under it and loses its stake at most, so a
roster with more under it than at stake is one that lying pays.

The committee answers when asked (22 November 2026; the names are those of the renaming of that day: a position,
a committee and its members, an epoch table, a signed price). The owner's frame: nobody is sent prices. A user, an
executor and a member each look where they like and judge for themselves; the committee signs only where two of them
disagree, and a dispute ends one of three ways: proved, refuted, or unanswered. The details below are the
implementation's.

- **A claim states its claimer's own prices**, and no script reads them. A close states an amount; a liquidation a
  fee and a cure. Undisputed, a claim is settled as stated, as before.
- **A dispute is decided at the prices the committee signs for the claim's time**, shown by anyone with the members'
  signatures (an epoch table spent by its attest, as before). One entry of one script answers both ways
  (`KpfTProve.answer`, `KpfTLiqProve.answer`): it computes whether the claim is within the rule at those prices, and
  the verdict it is told must be the one it computes.

  | | within the rule at the committee's prices | verdict 1 (proved) | verdict 0 (refuted) |
  |---|---|---|---|
  | close | a close is allowed there, and the amount is at most what the side is owed at those prices as they are | the claimer has its amount, its bond and half of the disputer's | the position again; the disputer has its bond, whoever answers half of the claimer's |
  | liquidation | the side is liquidated there; the fee is at most the rule's; the cure is between what puts the side at the maintenance at the prices moved by g toward it and at the prices moved against it | the claim is a proved one (a dispute period again, to cure); the liquidator has half of the disputer's bond | the position again; the disputer has its bond, whoever answers half of the liquidator's |

  The other half of the bond lost stays in the script's copy, with the oracle fees: the committee's, which answered.
- **Proof and refutation are each other's complement.** With the committee's signatures every disputed claim is one
  or the other and never both. A first version had a band between them (proved within g of the committee's price,
  refuted beyond 2 g, neither in between) and a refutation that looked at the price only. Both were holes, found
  before any review: a claim of a false amount at a true price could be neither proved nor refuted, so it cost
  nothing where disputed and paid where not; and a price stated in the band held a position for a dispute period at
  no cost, again and again.
- **The tolerance g is the claimer's own caution**, not a rule of the proof. `true_claim` counts a claimer's price g
  worse for it, so its claim stands wherever the committee's price is no worse for it than that; a claimer sure of its
  price may state up to what the price gives, and one that states more loses its bond. A liquidator's tool
  (`liquidation_own`, the default of `pos-liquidate` without the committee's signatures at hand) liquidates only a
  side that is below its maintenance even at its price moved by g toward the side, with the least fee over the span
  and the cure at its own price: within the rule wherever the committee's price is nearer than g. The lower bound of
  a cure is what keeps a side from liquidating itself with a cure of a sompi and holding its position by it.
- **Unanswered.** Where nobody shows the committee's signatures within the dispute period, nothing is decided: the
  position again, and each has its bond back (`KpfTDispute.lapse`, `KpfTLiqDispute.lapse`). The lapse no longer
  moves half of the claimer's bond (the paragraph "Who watches" above, and review T1b F2b): that half priced a
  claimer that disputed its own claim to hold a position, when a proof was the claimer's alone to bring. Now anyone
  answers, and whoever is held has the answer in hand: a false claim is refuted for half of its bond; a true one is
  proved by the party it holds, which settles it. What is left is a committee that signs nothing: then a claim and a
  dispute by two keys of one owner hold a position for a dispute period at the cost of the fees, and the way out is
  the exit at the entries (`pos-exit`). A stated limit.
- **Which price is the committee's.** The members' prices differ within the spread (at most a quarter of g, held by
  the scripts), and any price a quorum signed near is attestable. Whoever answers picks the one that decides its way
  (`exec-run` does: `Watchtower::answer`). A claim nearer than the spread to the line can so be proved by one and
  refuted by another, and the first to land has it. A claimer that keeps g of caution is a quarter of it at most from
  that.
- **Measured** (the engine, one leg unless said): the answer of a close 9,023 bytes proved and 9,110 refuted (13,464
  with two legs); of a liquidation 10,810 and 10,862. Two entries, one a verdict, each carried the arithmetic and
  made the liquidation's answer 15,221 bytes. `KpfTProve.answer` stands at the engine's limit of 244 stack items: the
  next local variable does not compile.
- **Checked:** every amount around the line, and 160 random positions with one and two legs, pass exactly one of the
  two verdicts on the engine, the one the model says (`thin_proof.rs`).
- **Not built yet:** the member's tool that watches for disputes and publishes its signed price for the claim's time
  as a transaction's payload; a watcher reading those payloads (today the signed prices are files in a folder,
  `--signed-prices`).
- **What an executor disputes** (after review T1b rounds 23 and 24, F33 to F38). The prices a claim states decide
  nothing, so an executor does not judge by them: a claim that stated a low price and a true amount took the bond of
  one that did. Every rule below came from a way to make the executor post a bond against a claim that is then
  proved.
  - With the members' signed prices of the claim's time at hand, it disputes a claim that an attestable price
    refutes and none proves. Attestable is any price a quorum stands near, the ends of that band among them, not
    only the prices members signed (F34, F38).
  - Without them it judges by a price of its own (`exec-run --price-file`, `--price-cmd`, `--price`), kept with the
    time of every round in the folder (`own-prices`). The price is of one market, the one `--market1` names, and no
    claim of another is judged by it (F35). A claim is judged by the readings on both sides of its own time, each
    within three rounds of it, never by the price of now (F36): a claim of a time the executor did not watch is not
    judged. It is disputed where it is within the rule at no price from the least of those readings less a margin to
    the most of them plus it (`--dispute-margin-bps`, twice the tolerance unless said, never less than the
    tolerance), tried in steps of a quarter of the tolerance whatever the margin (F37). One leg only. A price of 0,
    or a price file nobody rewrote for three rounds, is no reading. A reading names its market, and one of another
    market is not read (F40). A source may say three numbers, the price and the least and the most since it was
    last asked, and then the range judged is from the least to the most; a source that says one number leaves a
    price that moved and came back between two readings to the margin alone (F39: at the default margin, a move of
    twice the tolerance and back inside one round costs the executor its bond. A stated limit of such a source).
  - With neither it disputes nothing.
  - A disputed claim it refutes wherever an attestable price refutes it: that costs it nothing if a proof lands
    first.
- **Accepted by the owner** (22 November 2026; review T1b round 23, F32): an unanswered dispute returns both bonds,
  so a claim and a dispute of one owner hold a position for the fees wherever nobody has the committee's signatures
  for the claim's time. Before, a lapse cost the claimer half its bond. The other course (a part of the claimer's
  bond to the party held when a third key disputed) was not taken: while a committee is silent it lets a
  counterparty take from an honest claimer by disputing with a second key. What makes the limit rare is the
  member's tool that answers a dispute by itself, which is the next thing built; until it exists the limit is the
  common case, not the rare one.

A match into a position (22 November 2026; the owner: "the reassignment and the close are transactions of their
own: they can be done inside the match, can they not"). They can, by one more script, `KpfTReplace`: a novation
takes as an input the position a match wrote, so the two could not be one transaction of the scripts there were.

- **What it does.** A position P (A long against B, one leg) and two orders of its template, a long one and a short
  one, matched for n units at a price inside both limits, as `KpfTMatch` matches them. One of the two orders is a
  party's own and the other way: A's short order, or B's long one. That party leaves and the other order's owner
  takes its place. The leaver is paid its share of margin in P, its order's share, and what the price moved its
  way since P's entry. The one that stays keeps its side from the match's price, its margin less that move: at
  every price it is worth what it was. The newcomer has the leaver's side from the match's price with its order's
  margin, as a match would give it. What is left of P and of each order goes on.
- **Closing by an order.** A user that wants out places the order the other way and is out when it is matched:
  paid at once, with no claim, no dispute period and no committee. The claim (`pos-close`) stays for the user no
  order answers.
- **Who is protected how.** Nobody signs. The leaver's order is its word (a price it agreed to leave at; it was
  placed with its signature), the newcomer's order its own. The one that stays is protected as in a novation: the
  newcomer's margin is at least what it asked of its counterparty when its position was made, and more by what
  the price moved against it; and its own margin after the move is at least what the newcomer's order asks.
- **Nothing between.** The position of the leaver and the newcomer that a match would have written does not exist.
  The sender has the two orders' tips; the share of a novation's sender (`NOVATE_TIP`) is not taken.
- **If the newcomer is the one that stays** (a party buys the other out by crossing orders), nothing is written
  of those units: the two are paid.
- **Measured** (the engine): 5,383 to 5,505 bytes, where a match (3,843) and a novation (4,005) were 7,848 in two
  transactions. The script is 3,263 bytes. Its helpers are written out in the entry: a call keeps its arguments on
  the stack to the entry's end, and with them the entry passed the engine's 244 items.
- **Checked:** the four shapes (a long leaves, a short leaves, a rest of everything, the stayer buys the leaver
  out) at the engine, with the worth of the one that stays compared at 400 prices; 240 random cases, in which the
  engine takes what the model gives (156) and refuses what only the two conditions refuse (61).
- **In the tools:** `order-match --into POSITION --leaver a|b`; `exec-run --do match` looks, before it matches two
  orders, for a position one of their owners is the other side of, and matches into it.
- **Where no order answers** (`exec-run --do take`, off unless asked, with `--take-max KAS` and the executor's own
  price of one market). The uncertainty left to a user that wants in or out is an order nobody matches. An executor
  may answer it with an order of its own, the other way at the order's limit, when that limit is better for it than
  its own price of now by a spread (`--take-spread-bps`, 1 % unless said); the next round matches the two as any two
  orders. What it took from one user it passes on to the next that comes the other way: its order then is matched
  into the position it holds (`KpfTReplace`), it leaves, and the user takes its place. Where an order lets it out
  of what it holds it asks no spread. What bounds it: the most of its own margin it has in positions and orders
  (`--take-max`); one order of its own a direction; one a round; only orders that carry a tip. No script knows of
  any of this: it is the executor's choice and its money, and ten executors that compete answer the same order or
  leave it (the owner's limit on what an executor holds, of the JELLY kind, is a default of the tool here, not a
  rule of the chain).
- **After review T1b round 27** (F41 to F46).
  - *Open, the owner's to decide (F41).* The one that stays is struck again at the match's price, and that price is
    the two orders' alone: a leaver and a newcomer that are one owner choose it. While prices are signed the one that
    stays is worth what it was. At a settlement at the entries (an exit claim nobody ends) it is paid its margin as
    struck again, and the leaver has taken the difference. The same is so of a match and a novation, since the
    novation was designed: the replacement is those two in one. The other form (the one that stays untouched, the
    newcomer paying the leaver the move out of its own margin) moves the exposure to every newcomer, honest ones
    too: at a settlement at the entries it is paid less by the move whoever it traded with. What bounds either is
    that an exit claim is ended while a committee answers (next point).
  - *Exit claims are answered (F46).* With a committee that signs when asked, nothing asked it of an exit claim, and
    a losing party was out at its entry. A member (`--do answer`) now signs for an exit claim of its market as for a
    dispute, for the first multiple of 300 DAA at or after the claim landed (so that members that never talk sign
    for one time); `exec-run` ends the claim with a quorum's prices of any time at or after its `since` (`--do
    dispute`, or `--do watch` for a position of its own key), for half of the claimer's bond.
  - *A member signs only what its readings agree on (F45):* a reading each side of the time, within a round of it,
    and all they say of the time between no further apart than the committee's spread.
  - *`take` (F42, F43, F44, and what was read):* the cap counts the executor's margin and bonds in positions under a
    claim too; an order lets it out only of a position of the order's own template that is not claimed; the spread is
    asked against the least and the most its source saw since the last reading; an order that asks of its
    counterparty more than it posts is left.
- **After review T1b round 28** (F47, F48, F49; F42 to F46 verified). The reviewer ran F41 by a plain match and a
  novation, with the numbers of the replacement: it is a property of striking the one that stays again, older than
  `KpfTReplace`, and what bounds it is that an exit claim is ended.
  - *`take` answers only orders of the templates the executor was started with (F49).* The folder learns the
    template of every match the chain shows; an order of a template with a dispute period or an exit time of one
    DAA was answered like any other, and a position of such terms pays a claim before any round can dispute it.
  - *An exit claim is asked every multiple of 300 DAA since it landed (F47),* the newest ten, for as long as it
    stands: a member signs each its readings allow, so that a time comes that a quorum signed. One time only was
    asked, and the claimer chooses where it falls.
  - *An exit claim that landed before its own `since` is ended by a price at or after `since` (F48),* as the script
    allows.
- **Not done:** more than one leg; a match of many orders at once; `take` has placed no order at the engine or on
  testnet (offline it finds no anchor); the fixes of round 28 are not verified yet.

## 9. Changes after review C1 and live run 3

| Finding | Change |
|---|---|
| C1-F1 (High): half a quorum dictated the price | the spread rule (section 1); `spreadBps` in the roster and its tickets |
| C1-F2 (High): `take` without a pair, against no margin, with any lease | `take` requires both sides healthy at the price and bounds the lease (section 3) |
| C1-F3 (Medium): void liquidations chained on one stale tick | a tick older than the pair UTXO by more than `lateDaa` is refused; a void claim is restored at once and keeps the listings |
| C1-F4 (Medium): a price above `priceMax` could not be claimed | counted as `priceMax` |
| C1-F5 (Medium): the roster respent on every block | `join`, `topUp`, `leave` need the roster to be 20 DAA old, `roll` does not; a top-up is at least 1 KAS. The tickets' half stays (section 7) |
| C1-F6, C1-F7 (Low) | kept as stated limits (section 4; a cure or restore landing after the lapse date is settled at the entry, which a winner avoids by renewing) |
| C1-N1 | `termDaa >= 1,200`, `lateDaa > 0`, `epochDaa >= 100`, a ticket of 1 to 100 KAS, a spread of at most 10 % |
| live run 3, defect 1 | 15 seats; the tool's verification counts data signatures as a node does |
| live run 3, defect 2 | a ticket holds at least 1 KAS |
| C2-F1 (High): a claimant chose the price within a spread no market bounded | a pair refuses a ticket unless `4 x spreadBps <= gBps`; `c-market` and `c-check` refuse such a pair of roster and market |
| C2-F2 (Medium): an order's expiry bound nothing | `refund` pays its sender `keeperFeeMin`; `c-watch` raises an expired order; the note says what an expiry is |
| C2-F3 (Medium): the watcher did not see what happened to a pair | `c-watch` raises a spent pair or part, the last quarter of a lease, a liquidation of one's own side that can still be cured, an expired order. It still cannot follow a spent object to its successor (a node lists UTXOs by address) |
| C2-F4 (Medium): the signer refused a price above `priceMax` | `tick-sign` signs any price up to the ticket's limit |
| C2-F5 (Medium): stake put into a roster that did not begin empty | every roster entry checks that its value covers the table it writes; `roster-join` and `roster-topup` take the genesis facts; entering commands compare the object's market with `--market` |
| C2-F6 (Low): the collector tried only signed prices | it tries the ends of every band too and claims the middle of the best |
| C2-N1, C2-N2 | `c-market` and `c-check` warn of parameters that make a poor market; `tick-sign` refuses a second price for a tick and a tick of the future |
| C5-F1 (High): a contest nobody answered slashed a whole seat that had signed a true price, for a judge stake of `minStake` returned whole | a verdict takes at most the NO total of its dispute and pays a tenth of at most that; `c-watch --me` raises a held seat and a charge against one's key; `charge-answer` |
| C5-F2 (Medium): a charged seat forfeited itself, kept the tenth and locked its charges' bonds | a held stake shown two prices keeps a tenth under its holds, nobody is paid; the charge's author is paid by the stake as it was charged |
| C5-F3 to F5, notes | one statement of a false tick (beyond the market's `g`); the charge parameters are warned of wherever a roster is described; the generator asserts that its loop settled; a dispute's base is at least 0.5 KAS; the watcher's words for a refuted charge whose stake is gone |
| C6-F1 (Medium): a slash that took a stake whole locked the bonds of its other charges | a stake with other holds keeps 1 KAS for them; the last charge takes it |
| C6-F2 to F4, notes (Low) | stated above (who is paid); `charge-answer` warns of an answer that does not take the lead; `c-watch --me` raises a held exit; `seat-forfeit` and `exit-forfeit` say what a held forfeit does; the charge warnings at `roster-join` and `c-market` |
| C7 (nothing above a note) | the wording of the three notes above |
| C8 (trim, section 3a; one Medium in the runner) | C8-F1: `c-margin-run` manages the files it was started with and those it wrote. C8-F2: it renews leases and adds no margin to a pair about to lapse. C8-F3: stated in 3a. C8-F4: `seat-run` counts 9.5 DAA a second between readings. C8-N2, N3: the wording, a pause in the roller's loop |
| C9 (the cross-margin account, section 3b; nothing above Low) | C9-F1: a side is liquidated only while the other has the maintenance. C9-F2: the fee is at most what the loser is still owed. C9-F3: `x-show` prints the legs' market and roster ids whole; the terms are the more careful of the two markets' |
| C10 (the fixes of C9; nothing above Low) | C10-F1: a close is refused only where it would escape a due liquidation, so the party owed everything by a bankrupt one closes when the pot is under the maintenance. C10-F2 stays and is stated in 3b: under two maintenances the party that restores its own margin first may liquidate the other, which then has the grace to cure |
| C11 (the close rule in the engine: holds; signing on two machines: one High, one Medium, in `cosign`) | C11-F1: `cosign` named an output an object by its script alone, so an account written without its covenant id was printed as the honest one and signed, its value locked; an object is now its script and a covenant id the transaction gives it, and anything else with an object's script is refused. C11-F2: every listing is the tool's own words, the sums are apart, money leaving to another address is a warning. C11-N3: the writer's signatures are checked before anything is printed or signed. C11-F3 is stated in the runbook (an offer has no end) |
| C12 (the fixes of C11; one Medium) | C12-F1: `cosign` says which of an object's keys are the co-signer's; an object that names none is counted apart and warned of. C12-F2: a free or unbonding judge stake is an object without a covenant id. C12-N1: the co-signer's own coin continues no lineage, whatever the file says of it |
| C13 (the fixes of C12; one High, one Medium) | C13-F1: `cosign` refuses an object its own script refuses (the model's check of its parameters, as every builder runs it) and a kind only a contract writes when a plain input writes it. C13-F2: it says what the co-signer's part of an account or a pair holds at the entries; a part that holds nothing is counted with the objects of other keys and warned of. What `cosign` cannot judge is the terms: prices, margins, the roster a leg names are the co-signer's to read |
| C14 (the fixes of C13; one Medium) | C14-F1: an order and a pair are printed with their market's id and the roster that prices it, as an account's legs are; `cosign --market FILE` refuses an object priced in any other market than those given. C14-N1: with `--daa` or a node, an order past its expiry or an object past its lease is refused |
| C15 (the fixes of C14; one Medium) | C15-F1: an account's terms are printed whole; with `--market`, an account is accepted only with the terms and the legs its markets give (what `x-open` and `x-swap` write). C15-F2: a market file given as a reference carries its roster's genesis facts. Five rounds in a row found one more thing `cosign` did not read: a reading of somebody else's transaction is a list, and a list has an end one does not see. It stands for a test network; the next form rebuilds the transaction from the declared command with the co-signer's own tool and signs an identical one |
| C16 (the fixes of C15; nothing above Low) | C16-F1: a free judge stake is printed with its judge block (its id whole and every parameter), to be compared with the block the roster names |
| C4 (nothing above a note) | C4-N2: the signer's journal. C4-N4: `c-close` and `c-liquidate` compare the tick's age with `--daa` as with a node and warn when little time is left to land. C4-N1, C4-N3: said in the note and the runbook |
| C3 (nothing above Low) | C3-F1: the refunder's fee is at most a twentieth of the escrow. C3-F2: `c-order` refuses a market file without its roster's genesis facts and an expiry already past. C3-F3: `c-cure` refuses a void claim (`c-finalize` restores it for nothing). C3-N1: `roster-roll --steps` is compared with the clock when one is given. C3-N4: two more warnings |
