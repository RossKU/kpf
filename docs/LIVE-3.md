# Live run 3: a committee market on testnet-10

Date 2026-11-21. Network testnet-10, node `ws://<tn10-node>:18210` (JSON wRPC; the node the KOB soak uses), the
committee family as first built (before review C1's fixes). One funder key paid for everything; three seat keys, a
long and a short key signed what only they can sign. Every transaction below was built, verified locally, and
submitted by `kpf` with `--node --submit`; a command made at most eight small queries and none polled.

Roster: epochs of 1,200 DAA (2 minutes), exits of 600 DAA, seats of at least 10 KAS, a quorum of 67 %, 4 tickets an
epoch of 1 KAS each. Market: lease 2,400 DAA, lapse 600 after it, a claim valid within 600 DAA of its tick (60 s; the
default is 100), 100 DAA to cure. Roster id `ca1290fe…f65c`, market id `dcd3daa1…75be`.

## Transactions

| # | Step | Transaction | Fee (KAS) | Bytes |
|---|---|---|---|---|
| 1 | `roster-create`: an empty roster of 1 KAS | `df24ef42…2ebe` | 0.002083 | 353 |
| 2 | `roster-join` seat 0, 30 KAS | `eabce775…5454` | 0.00873 | 4,365 |
| 3 | `roster-join` seat 1, 22 KAS | `ca7744d3…b517` | 0.00873 | 4,365 |
| 4 | `roster-join` seat 2, 15 KAS | `0ad9a3aa…0a1d` | 0.008728 | 4,364 |
| 5 | `roster-roll` to the epoch [628,527,600, 628,528,800): 4 tickets | `63fbcce9…893b` | 0.015596 | 7,798 |
| 6 | `c-order` long, 20 units, limit 10, rate 1 | `5f940847…ba83` | | |
| 7 | `c-order` short, 20 units, limit 10, rate 2 | `96c1d878…a684` | | |
| 8 | `c-open` 10 units at 10: pair A, 30.2 KAS, lease until 628,530,603 | `06d78066…c0be` | 0.067742 | 33,871 |
| 9 | `c-open` the orders' other 10 units: pair B, lease until 628,530,893 | `23cfb28c…` | | |
| 10 | `c-close` pair A's long at 10.5 (three seats signed the tick 628,528,681) | **refused by the node** | | 19,790 |

`roster-check` recomputed the roster id from its genesis facts against the live object before step 2 (one query).

After the refusal the positions were unwound with the same templates, which exercised the paths that need no price:

| # | Step | Transaction |
|---|---|---|
| 11, 12 | `c-coop-close` pairs A and B (both keys sign): the long 10.1 KAS, the short 20.073432 KAS each | `4d7e5215…e82d`, `1390d7cb…df68` |
| 13 to 15 | `roster-leave` seats 0, 1, 2: three exits of 30, 22 and 15 KAS released at 628,529,400 | `98ea2949…44c5`, `90e281e4…8244`, `bc12cbd5…65ee` |
| 16 to 18 | `exit-withdraw`: each stake to its seat's key | `b3540c71…bd45`, `7df74507…1cf2`, `0dd8c9bf…beb3` |
| 19 | `ticket-retire` of the four tickets | **refused by the node** (defect 1) |

## Defects found

1. **A ticket of 16 seats is not standard.** The node refused step 10: "transaction input #1 has 16 signature
   operations which is more than the allowed max amount of 15". The node counts a data signature
   (`OpCheckSigFromStack`) in the static count of an input's signature operations; the rusty-kaspa v2.1.0 function
   the local verification uses (`get_sig_op_count_upper_bound`) does not, which is what experiment Z1 measured ("not
   in the static sig-op bound") and what `kpf`'s own check passed. The loop over the seats is unrolled, so the ticket
   script holds one signature check a seat. The count is of the whole script, whatever entry runs: `ticket-retire`,
   which checks no signature, was refused for the same 16 (step 19), so the four tickets of this run (4 KAS) can
   never be spent. Fix: 15 seats, and the local verification counts data signatures as the node does.
2. **Four tickets of 0.2 KAS could never be mined** (found offline by the command-line test before the run): an
   output that carries a covenant id costs about 4 x 10^12 / value grams of storage mass (KIP-9), so four tickets of
   0.2 KAS are 800,588 grams against a block limit of 500,000. The default is 1 KAS a ticket (160,508 grams in step
   5); the roster's lower bound on `ticketValue` must rise with it.
3. **An epoch of two minutes is too short to work by hand**: the tickets of step 5 served ticks for two minutes; a
   claim needs a ticket of the epoch that holds its tick, so every pause costs a roll. The default of 5 minutes is
   kept; a signer or keeper that rolls on time makes the length a matter of how fast a seat can be replaced.

## Second run, after the fixes

The same day, with the templates as fixed after review C1 and the defects above (15 seats, tickets of 1 KAS, the
spread rule, the rest of the roster, the bound of a tick against the pair, a void claim restored at once, `take`'s
health and lease rules). Roster: epochs of 3,000 DAA (the default), exits of 600, a quorum of 67 % within 0.1 % of the
price. Market as before. Roster id `857f856e…48f5`, market id `67d583d3…26e1`. All 30 transactions were accepted.

| # | Step | Transaction |
|---|---|---|
| 1 | `roster-create` | `a9f18639…5183` |
| 2 to 4 | `roster-join` seats 0, 1, 2 (30, 22, 15 KAS); each waited the roster's rest of 20 DAA | `ef4ed17f…381c`, `ff533e07…088c`, `8a626a48…42b6` |
| 5, 6 | `c-order` long and short, 30 units each in parts of 10 | `953a22eb…0038`, `8505307f…52b4` |
| 7 to 9 | `c-open` pairs A, B, C: 10 units at 10, range [9, 12], 30.2 KAS each | `c141517c…a28a`, `6988ebcf…5686`, `a3195dd0…d49d` |
| 10 | `roster-roll` to the epoch [628,551,000, 628,554,000): 4 tickets of 1 KAS | `96e61b6c…6eb7` |
| 11 | `c-close` A's long: three seats signed 10.5 for the tick 628,551,170; paid at 10.4475 | `d9573b36…436b` |
| 12 | `c-finalize`: valid, landed 50 DAA after its tick; the long 14.475 KAS, the short 15.725 KAS | `a17c96da…4cb0` |
| 13, 14 | `c-liquidate` B's and C's long with one set of ticks (9.28 at 628,551,261), each on its own ticket | `d38e331b…1ae8`, `0b597581…e782` |
| 15 | `c-cure` B: the long's bound moves to 8.77192; the cured pair holds 32.4808 KAS | `86a7f7f5…7416` |
| 16 | `c-finalize` C after the grace: the short keeps the part (29.736 KAS), the proposer's fee 0.464 KAS | `8f37207f…f779` |
| 17 | `c-keep-alive` B with a tick at 628,551,676: the lease from 628,552,529 to 628,554,076 | `7ff7602a…d767` |
| 18 | `c-assign` B's long to the funder at 10: the leaver is paid 12.2808 KAS in the same transaction | `08b14fde…935a` |
| 19 | `c-coop-close` the forfeited C (both sides the short's: its key alone signs) | `2395b42c…f272` |
| 20 to 22 | `roster-leave` seats 0, 1, 2: exits released at 628,554,600 | `4c7ccc9a…5c77`, `236f37c7…b9b0`, `4d83cb42…baf1` |
| 23 | `c-lapse-settle` B, 600 DAA after its lease: the long 12.2808 KAS, the short 20.2 KAS (the entry) | `64580f58…2751` |
| 24 to 26 | `exit-withdraw` the three stakes to their seats' keys | `4dd0db64…509c`, `60c106af…657c`, `d05ba4d5…caa8` |
| 27 to 30 | `ticket-retire` the four tickets, an epoch after their window | `a1b08027…506d`, `4dd515c9…e82f`, `5a52e938…1b7a`, `27bdb9de…899a` |

What it shows that the offline tests cannot: a ticket of 15 seats is standard to a node (attested four times, retired
four times); a claim built, signed and submitted by the tool lands 5 to 6 seconds after the DAA its ticks name (three
`tick-sign` calls, one `c-close`: eight queries); two pairs are liquidated in the same seconds on two tickets of one
epoch; `c-watch` reads a live pair with one query.

Still open after this run:

- **Ticks travel by file.** The seats' signed prices were written by `tick-sign` and handed to the claim as files on
  one machine. How a claimant gets them from seats elsewhere is not built.
- **Somebody must roll.** Between an epoch's end and the next `roster-roll` no tick of the present can be attested;
  in this run that was the tester.
- **A lease of four minutes** made the run fit an afternoon; with it a pair must be renewed or closed within minutes.
  The defaults are 7 days and 1 day.

## Third run, after reviews C2 and C3 and with the two-price slash

The templates of this run are those of the working tree after the fixes of reviews C2 and C3 and with
`KpfRoster.forfeit` / `KpfExit.forfeit` (DESIGN-COMMITTEE.md 5a). Roster `a00e205b…405e` (epochs of 1,200 DAA, exit
time 600 DAA), market `8fa16c12…ba62` (term 2,400, lateDaa 600, grace 100). 21 transactions, all accepted.

| # | What | Transaction |
|---|---|---|
| 1 | `roster-create` | `5230782a…65c6` |
| 2 to 4 | `roster-join --genesis` seats 0, 1, 2 with 30, 22, 15 KAS | `13417bf3…e2ad`, `a135322b…9612`, `95f8175a…76c3` |
| 5 to 7 | `c-order` a long and a short of 10 units, a long of 5 units that expires at 628,585,946 | `65c9bd22…55d1`, `0e379537…40b1`, `a9785022…d1c3` |
| 8 | `c-open --market`: a pair of 10 units at 10 KAS | `196c97c0…056b` |
| 9 | `roster-roll`: the epoch [628,585,200, 628,586,400) with 4 tickets | `28decfef…7a36` |
| 10 | `c-close` the long at 10.5 attested for 628,585,265 (paid at 10.4475) | `85676ae2…fc14` |
| 11 | `c-finalize`: valid, landed 40 DAA after its tick; the long 14.475 KAS, the short 15.725 KAS | `8b8c944c…94d7` |
| 12 | `seat-forfeit`: seat 1 signed 10.5 and 11 for the tick 628,585,265; the roster 68 to 46 KAS, 19.8 KAS burned | `11463e9c…1ad0` |
| 13, 14 | `roster-leave` seats 0 and 2: exits released at 628,587,000 | `5e3ef740…599c`, `166ab2ee…dcac` |
| 15 | `exit-forfeit`: seat 2's key signed 10 and 9 for one tick; its exit of 15 KAS is gone, 13.5 KAS burned | `e1863ee6…2ab4` |
| 16 | `c-refund` the expired order: its owner is paid 6.1 KAS, the sender keeps the fee | `cf2b21c3…a788` |
| 17 | `exit-withdraw` seat 0's 30 KAS at its release (refused by the tool 94 s before: "usable from DAA 628,587,001") | `a78efff8…2d37` |
| 18 to 21 | `ticket-retire` the four tickets | `8a946841…fd45`, `d9c6c0d0…65b3`, `54c017ac…9388`, `94f398da…6a10` |

What it adds: a roster script with two data signatures and a transaction signature is standard to a node; a burn
output (P2SH of OP_RETURN) of 19.8 and of 13.5 KAS is accepted; `tick-sign` refused to write the second price over the
seat's own tick file and wrote it only under another name; the refund of an expired order by a third party lands.
Not run here: a liquidation, a cure, a keep-alive, an assignment (they ran on the templates before the C2 fixes, in
the second run; the entries are unchanged but for the ticket's spread check, which this run's close passed).

## Fourth run: the judged slash

The templates of the working tree with `KpfCharge` and the roster's and the exit's `charge`, `slash`, `clear`
(DESIGN-COMMITTEE.md 5a), before review C5. Roster `7c1846fe…e44f` with a judge for a trial (stakes of 10 KAS aged
300 DAA, rounds of 300 DAA), a bond of 2 KAS a charge and 1,200 DAA to contest; market `c4e82475…bbb8`. The roster's
script is 15,334 bytes; every transaction was accepted.

| # | What | Transaction |
|---|---|---|
| 1 | `roster-create` with the judge block and the charge parameters | `4823a486…1942` |
| 2 | `judge-stake` of 10 KAS | `645418eb…c753` |
| 3 to 5 | `roster-join` seats 0, 1, 2 with 12, 11, 10 KAS | `38b2925a…3757`, `c16ffcc1…2fbd`, `f41e3791…ed48` |
| 6 | `charge`: seat 1 signed 20 for the tick 628,612,001; the charge `cf8e565f…27bc` with its bond of 2 KAS; the roster 34 KAS as before | `c0b8eab7…2281` |
| 7 | `charge-contest`: the judge's stake, 10 KAS on NO; the dispute | `a87343b6…dfd5` |
| 8 | `charge-decide` after the round of 300 DAA: NO leads, the tick is refuted | `5ad941b4…5999` |
| 9 | `charge-settle`: the seat's 11 KAS are gone: 3.1 KAS (the bond and a tenth) to the charge's author, 9.9 KAS burned; the roster 23 KAS | `da950075…f2f3` |
| 10 | `settle-pool`: the opener's 10 KAS back as free stake | `30e4e03d…47a5` |
| 11 | `roster-leave` seat 0: an exit of 12 KAS (already released: the roster was never rolled) | `4954e6ce…0d8b` |
| 12 | `charge` on the exit for a tick its key signed; the exit holds the charge | `d9d0c91f…9673` |
| 13 | `charge-settle` 1,200 DAA later, nobody having contested: the hold ends, the bond of 2 KAS is burned | `ffd739f7…fd92` |
| 14 | `exit-withdraw` of the cleared exit | `32cd761d…7ea3` |
| 15, 16 | `roster-leave` seat 2 and its `exit-withdraw` | `92ab3615…65d5`, `f3fc9c6c…a9ec` |
| 17 | `settle-pool` without receipts: the pool closes | `adad0d2d…7d97` |
| 18 | `stake-unbond` of the judge's stake | `9731b616…df33` |
| 19 | `stake-withdraw` 300 DAA later | `97ebef85…7bac` |

Refused by the tool on the way, as they should be: `charge-settle` of the pending charge ("can still be contested
until DAA 628,614,147") and `exit-withdraw` of the held exit ("the stake is held for a charge"), though the exit's
release had passed.

What it shows: a node takes the roster's script at twice its former size (a leave carries 19 kB of signature
script); the judge of the oracle family rules on a claim of another family through the same interface; a charge's
life from the signed tick to the burn is six commands. Not run: an answered dispute (YES against NO), a stake held
for two charges, a late contest.

## Fifth run: the judged slash after reviews C5 and C6

The templates of the working tree after the fixes of reviews C5 and C6 (a verdict takes no more than its NO side had
at risk; a held stake shown two prices keeps a tenth; a stake with other holds keeps 1 KAS), the roster's script at
17 kB. Roster `e1b9e3f7…bf5c` (a trial judge: stakes of 10 KAS aged 300 DAA, rounds of 300 DAA; bond 2 KAS, 1,200 DAA
to contest), seats of 12, 11 and 10 KAS. 33 transactions, all accepted.

| # | What | Transaction |
|---|---|---|
| 1 | `roster-create` | `2e034561…c354` |
| 2 to 4 | `judge-stake` of 10, 10 and (seat 0's own) 13 KAS | `587184cf…f534`, `2057be37…7d22`, `68c393a5…c0d2` |
| 5 to 7 | `roster-join` seats 0, 1, 2 | `35a16b52…ec16`, `4ff93769…0a9a`, `155b15ae…a541` |
| 8 to 10 | `charge` seat 1 (a false 20), seat 0 (a true 10), seat 2; `c-watch --me` on the roster: "ACT: your seat is charged" | `3cb596cf…38de`, `199896af…e682`, `7372cbe8…3963` |
| 11, 12 | `charge-contest` on the first two, 10 KAS on NO each | `d6c92015…fd4e`, `5e228c3f…2c13` |
| 13 | `charge-answer`: seat 0 answers its own contest with 13 KAS on YES: "YES leads, YES 13 KAS, NO 10 KAS" | `495410f6…cef5` |
| 14 | `seat-forfeit` of seat 2 while it is held: 9 of its 10 KAS burned, 1 KAS kept under its hold, the sender unpaid | `417fa7bb…f6b1` |
| 15, 16 | `charge-decide` and `charge-settle` of the first: refuted; 10 of seat 1's 11 KAS taken (what NO had at risk): 1 KAS and the bond to the author, 9 KAS burned, the seat keeps 1 KAS | `d9f3a5e8…02f9`, `c7ca9aac…4555` |
| 17, 18 | `charge-decide` and `charge-settle` of the second: upheld; the hold ends, the bond of 2 KAS to seat 0's key | `53a6d286…c4db`, `2ae9793e…7896` |
| 19 | `charge-settle` of the third, 1,200 DAA after it landed and never contested: the hold ends, the bond burned | `b140af34…fefd` |
| 20, 21 | `settle-pool`: the unanswered opener's 10 KAS back; the answered dispute pays seat 0's stake 22 KAS (its 13 and the contest's 10 less the burn of a tenth) | `9d7b74e9…3ff3`, `cdd281a4…f6a9` |
| 22 to 27 | `roster-leave` and `exit-withdraw` of the three seats (12, 1 and 1 KAS: an exit of 1 KAS is mined and withdrawn) | `036d8411…8a69`, `8dac64cb…a4ca`, `96841a7e…127c`, `d94a1851…d8b0`, `fd893af7…18d3`, `1e9e242c…9a62` |
| 28 to 33 | the two pools closed, the two judge stakes unbonded and withdrawn | `912a1c23…bc65`, `19cb61bc…1db8`, `9077835e…40b6`, `6b98fb68…b928`, `1201405e…4c2a`, `dad77880…658b` |

What it adds to the fourth run: an answered dispute (a seat charged for a true price defends itself and wins the
contest stake), three charges on one roster at once, a capped slash that leaves the seat 1 KAS, a held forfeit, seats
and exits of 1 KAS leaving. Not run: a stake held for two charges, a late contest, a dispute with more than one lead
change.

## Sixth run: margin taken out of a position (trim)

A roster with epochs of 36,000 DAA and seats of 12 KAS each, a market with `lateDaa` 600, two pairs [9 .. 12] of 10
units: one key long in the first and short in the second. 16 transactions, all accepted.

| # | What | Transaction |
|---|---|---|
| 1 to 4 | `roster-create`, `roster-join` seats 0, 1, 2 | `623413ce…d61c`, `43eb4c1c…`, `08bbe904…`, `af3c878b…` |
| 5 to 10 | four `c-order`, two `c-open` | `699abf20…`, `4752f1c5…`, `503c6dff…`, `e2ef991c…`, `ead0f99e…41cb`, `108a3e33…8132` |
| 11 | `roster-roll` (after `--wait` had woken 16 DAA early and been refused: defect below) | `9093c24d…8327` |
| 12 | `c-trim` of the long at the attested 11.3: its bound from 9 to 10.2, 12 KAS freed | `9c8f6d28…c843` |
| 13 | `c-finalize`: "valid (landed 92 DAA after its tick): 12 KAS to the long, whose bound is now 10.2 KAS" | `4d339edd…8aef` |
| 14 | `c-add-margin` of the short of the other pair by 0.9862425: its liquidation line from 11.49 to 12.43 | `97a60b18…4ba3` |
| 15, 16 | `c-coop-close` of both pairs | `a2d9b836…9866`, `2fe68503…2758` |

`c-account` before: the short 1.64 % from its liquidation, 9.862425 KAS needed, the long with 16.81 KAS to spare;
after: no position within 3 % of its liquidation.

Defect found: `roster-roll --wait` slept `(end - now) / 10 + 1` seconds once and rolled; the score had advanced by
9.95 a second and the roll was refused 16 DAA before the epoch's end. It now reads the score again after every sleep.

## Seventh run: two positions in one cross-margin account

After the ticket's change (several tickets of a roster attest in one transaction) a new roster; two markets priced by
it, a pair [9 .. 12] of 10 units in each: one key (the owner) long in market 1 and short in market 2, the funder as
the executor. 19 transactions, all accepted.

| # | What | Transaction |
|---|---|---|
| 1 to 4 | `roster-create` (`--from` so that the first epoch ends in five minutes), `roster-join` seats 0, 1, 2 | `47cc8bad…`, `50662078…`, `499a67bb…`, `c0cafb4d…` |
| 5 to 10 | four `c-order` in two markets, two `c-open` | `7bbf1f69…`, `fe9dd882…`, `c0b1f4fc…`, `9d22c690…`, `6150d3ce…1929`, `b15b247b…493f` |
| 11 | `roster-roll` | `e94ff8c1…7b69` |
| 12 | `x-swap` at the agreed 11 and 10.5: the executor holds the pairs [8.55 .. 12] and [9 .. 12.6]; the owner one account, owed 35 KAS, the executor's margin 20 KAS | `d13d5349…0f62` |
| 13 | `x-liquidate` of the owner at the attested (11, 13), two tickets and six signatures for one tick: the fee 1.2 KAS, the cure 3.205 KAS | `10b44ed3…c988` |
| 14 | `x-cure`: "3.205 KAS added; the margins are now a 38.205 KAS, b 20 KAS" | `3897140f…abb5` |
| 15 | `x-add-margin` of 2 KAS by the owner | `6e8a3580…47fa` |
| 16 | `x-close` by the owner at the attested (13.5, 13): paid 38.88 KAS | `12e572b2…0543` |
| 17 | `x-finalize`: "valid (landed 128 DAA after its tick): the owner 38.88 KAS, the counterparty 21.525 KAS" | `095ae8ca…e678` |
| 18, 19 | `c-coop-close` of the two pairs the executor held | `ec1cd0d6…3b48`, `65d66f65…7b57` |

`x-show` between 12 and 13: at (11, 13) "a is owed 10 KAS (leg 1 0 KAS, leg 2 -25 KAS) ... the OWNER (a) can be
LIQUIDATED"; at (13.5, 13) "a is owed 35 KAS (leg 1 25 KAS, leg 2 -25 KAS) ... neither party can be liquidated": the
gain of one leg paying for the loss of the other.

The contracts of that run are those after review C9. After review C10's close rule, on the same roster, two accounts
written by `x-open` (6 transactions, all accepted):

| # | What | Transaction |
|---|---|---|
| 20 to 22 | an account of 35 and 20 KAS; `x-liquidate` of the owner at (11, 13); `x-finalize` after the grace: "valid: 54 KAS to the winner ..., the proposer's fee 1.2 KAS" (the forfeit) | `d4a6e3a8…cb6d`, `66db61a3…279c`, `f41f3967…c60c` |
| 23 to 25 | an account of 6 and 6 KAS against a maintenance of 10.75 KAS: `x-liquidate` refused for want of a healthy other side; `x-close` by the owner at (11, 10.5), paid 4.925 KAS; `x-finalize`: "the owner 4.925 KAS, the counterparty 7.275 KAS" | `1cbd0756…8c00`, `76b8cd61…b68c`, `fd7d40d5…3c4a` |

Signing on two machines, on the same network: `x-open --partial` built without any key for the funder's four plain
inputs, `cosign --submit` with that key (`344e17e6…be8a`, 713 bytes, each open input metered at the 100,001 units
assumed for it), then `x-coop-close` of the account by both keys (`3b986a53…07e9`).

Not run: a void claim, `x-keep-alive`, `x-lapse-settle`,
`x-coop-close`, an account with one leg, legs priced by two rosters. The seats' stakes of this run and the sixth are
still in their rosters.
