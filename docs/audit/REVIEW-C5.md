# KPF review C5 (the judged slash: KpfCharge, the holds, the coupling to KpfStake)

## Scope and method

Target: `contracts/KpfCharge.sil` (new), `contracts/KpfExit.sil` (rewritten), `contracts/KpfRoster.sil` (`charge`,
`slash`, `clear`, `fresh`, `fold`, `held`, `tplState`, `checkCharge`, `cont` with the holds, `exit` with a hold),
`contracts/KpfStake.sil` as the judge of a charge (unchanged), the models (`committee::{hold_add, hold_fold,
hold_witness, hold_without, ChargeParams, Charge}`, `Roster::{charged, slash, clear}`, `Exit::{charged, slash,
clear}`), the builders (`charge`, `judge_stake`, `charge_contest`, `charge_decide`, `charge_settle`), the commands
(`charge`, `judge-stake`, `charge-contest`, `charge-decide`, `charge-settle`, `hold-witness`, `c-watch`, `deposit` on
a charge's dispute) and the generator's fixed point (`kpf-artifacts`, `generate_committee`), read against
`docs/DESIGN-COMMITTEE.md` 0, 5a and 7 (N) and `docs/TESTNET.md` 10.5.

Files of this round (nothing else was changed; `REVIEW-C4.md` got a status section):

- `crates/kpf-protocol/tests/auditC5_builders.rs`: 7 tests (2 `c5_exploit_`, 2 `c5_bounded_`, 3 `c5_holds_`). Every
  transaction is built by the library, signed and run through `verify::verify`; an attack is a built transaction whose
  plan, entries or outputs are edited before it is signed, and each refusal is asserted on the input whose rule is
  meant, beside a control that is accepted.
- `crates/kpf-tests/tests/auditC5_tests.rs`: 2 tests (`c5_holds_`), on the engine with hand-made signature scripts.
- `crates/kpf-cli/tests/auditC5_cli.rs`: 3 tests (2 `c5_exploit_`, 1 `c5_bounded_`), through the binary.

Runs:

- `CARGO_BUILD_JOBS=2 cargo test -p kpf-protocol --jobs 2 --test auditC5_builders`: `ok. 7 passed`.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test auditC5_tests`: `ok. 2 passed`.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test auditC5_cli`: `ok. 3 passed`.
- The earlier audit suites on the new templates, unchanged: `auditC1_tests` 22, `auditC1_walk_tests` 2,
  `auditC2_tests` 12, `auditC3_tests` 3, `auditC4_tests` 4; `auditC2_builders`, `auditC3_builders`, `auditC4_builders`
  1 each; `auditC3_cli` 6, `auditC4_cli` 1. All passed.
- `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- check`: `pinned artifacts match a fresh compile`.

The coordinator's edits to my earlier tests (the signer's journal in `auditC3_cli`, the void claim built without a
clock, `--second-price` in the `ticks` helpers, three data signatures in `auditC4_builders`): read and run; I agree
with all of them.

## Findings

**One High, one Medium, three Low, four notes.** No covenant rule of the new code was broken: a stake is slashed or
cleared only by a charge its own `charge` wrote, and a charge leaves only with that stake. Both findings above Low
are what those rules, kept exactly, let a participant do.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C5-F1 | High | A contest nobody answers is a verdict. One aged judge stake of `minStake`, returned whole, slashes a seat of any size for a TRUE tick when no stake accepted `ageDaa` before the charge answers within one round; the author keeps a tenth. An emptied roster is retaken for `minTotal` and prices every pair of its markets. No tool tells a charged seat that it must answer | `c5_exploit_an_unanswered_contest_slashes_honest_seats_and_the_roster_is_retaken_for_its_least_stake`, `auditC5_cli::c5_exploit_a_true_tick_is_charged_and_slashed_and_the_watcher_asks_the_seat_for_nothing` |
| C5-F2 | Medium | A charged seat ends every charge against it by forfeiting itself (two prices for a tick of its choosing): it keeps the tenth the author was to have, and the author's bond is locked in the charge for ever. Without malice the same happens to every author but the first when several charge one seat | `c5_exploit_a_refuted_seat_forfeits_itself_first_takes_the_tenth_and_leaves_the_charge_with_its_bond`, `auditC5_cli::c5_exploit_a_refuted_seat_forfeits_itself_and_the_charge_cannot_be_settled` |
| C5-F3 | Low | What a hold costs does not grow with what it holds: an exit of any size is kept from its owner for one bond per `chargeDaa` (testnet 240 KAS a day), or for 18.5 KAS a day by a contest at the last DAA that nobody answers | `c5_bounded_a_charge_waits_behind_nine_later_ones_until_one_lapses` (the numbers), by reading |
| C5-F4 | Low | Whole stakes are burned on a rule stated three ways: N§5a and `TESTNET.md` say a tick is true within the roster's `spreadBps` of the feed, N§7 says only a lie beyond `g` is judged, and `deposit` prints the oracle family's rule (within `g`, at `t0` and at the landing) to whoever answers a charge's dispute | `auditC5_cli::c5_bounded_the_charge_commands_at_their_edges`, by reading |
| C5-F5 | Low | Only `roster-create` looks at a roster's charge parameters. A roster whose charges wait one DAA for a contest (no seat of it can ever be slashed for a false price) is created, joined and given a market without a word; nothing shows a joiner the judge block its stake will answer to | `auditC5_cli::c5_bounded_the_charge_commands_at_their_edges` |
| C5-N1 | Note | `KpfCharge.settle` is tied to its stake by the engine's clean-stack rule and by arithmetic, not by a template check: a ticket or an exit dressed as a slash passes the charge's script. The engine accepts non-minimal pushes and non-minimal numbers | `c5_holds_a_ticket_or_an_exit_dressed_as_a_slash_passes_the_charge_and_fails_its_own_clean_stack` |
| C5-N2 | Note | The generator's two "did not settle" assertions cannot fail (each compares a value with what it was just set from). The loop settles in two rounds, so the pinned charge is right; a stale one would be written without a word if it ever did not | by reading; `c5_holds_a_charge_leaves_only_with_its_own_stake` (the constants in the pinned script) |
| C5-N3 | Note | A contest at the builder's least base (0.1 KAS) is 409,854 grams of storage mass, a verdict 407,765: one of each a block. The command's default base (0.5 KAS) is 82,000 | `c5_bounded_a_held_stake_keeps_its_hold_wherever_it_goes_and_the_heaviest_shapes` |
| C5-N4 | Note | `c-watch` on a refuted charge says "ACT: slash the stake" for ever when the stake is gone; `charge` says nothing when the seat already has a hold | `auditC5_cli` (both `c5_exploit_` tests, the edges test) |

## C5-F1 (High): a contest nobody answers slashes a seat for a true tick

**Affected.** `KpfCharge.decide` (refuted whenever NO leads), `KpfStake.open` (the opener's stake is on NO; a stake
counts only if accepted `ageDaa` before the claim), `KpfRoster.slash`, `KpfExit.slash`, N§5a "What a charge costs",
N§7, `c-watch`, `deposit`.

**The missing rule.** Nothing ties a verdict to more than its opener: a dispute holding one stake of `minStake` on NO
and nothing on YES resolves after one round, and that verdict moves a stake of any size. In the oracle family the
default of an unanswered contest was harmless (a claim was void). Here the opener's side is the destructive one.

**The attack** (`c5_exploit_an_unanswered_contest_...`, fixture: seats of 30, 22 and 15 KAS; bond 5 KAS; judge stakes
of 20 KAS aged 200 DAA, rounds of 100). Each seat signed 10 KAS for the tick 6,500; the feed was 10.

1. Three `charge` transactions (5 KAS each, at 6,600..6,602), `payKey` the author's.
2. Three `contest` with three stakes of 20 KAS accepted at 6,000 (at 6,650). Nobody answers.
3. Three `decide` at lock time 6,750: `CHARGE_REFUTED`.
4. Three `slash`: 3.0 + 2.2 + 1.5 = 6.7 KAS and the three bonds to the author, 60.3 KAS burned, the roster empty.
5. `pool_settle`: the opener's 20 KAS back, whole (it won; there is no loser).
6. Three `join` of 10 KAS and a `roll`: the ticket of [9,000, 12,000) is the author's alone for 30 KAS.

At risk meanwhile: 60 KAS of judge stake and 15 KAS of bonds, all returned. With the testnet defaults: one stake of
100 KAS aged 7 days and 10 KAS a seat, a round of 12 hours. If a contest IS answered (YES at 1.25 times the NO), its
author loses the stake (10 % burned, the rest to the answerers) and the bond (to the seat): 110 KAS against a tenth
of the seat, so the bet pays when nobody answers more often than 110 / (110 + w/10): 18 % for a seat of 5,000 KAS.

**Why this is not N§7's "the judge decides falsely if its majority does".**

- It needs no majority of the judge's stake, only that nobody answers one round. An answer cannot be raised for the
  occasion: a stake counts only if it was accepted `ageDaa` before the charge. On a judge block nobody else has bonded
  under (every new roster, for 7 days after the first honest stake) the answer probability is exactly zero, and the
  roster's creator can be that first staker.
- The tools make it zero elsewhere too: `c-watch` on the roster does not show that a seat is charged, on the charge
  it says "it waits for a contest" and "with the judge" and asks for nothing, and `deposit` can only answer with
  `--accept-untraced`. N§5a says the charge costs "the seat: nothing unless the tick is refuted".
- It pays (a tenth), and it chains: seats emptied for a judge's least stake are retaken for `minTotal`, and the same
  judge then upholds whatever the new quorum signs. N§7 prices a lying quorum by the stake it loses; this quorum
  loses none.

This is the economy review 12 found in the oracle family and N§0 gives as the reason for this one ("a false price is
a winning bet below a contest probability of 80 to 90 %, and the shipped tools make that probability zero"), with the
roles turned: a false charge is a winning bet below an answer probability.

**Smallest fix, and what it could break.** No single rule closes it; in rising cost:

1. Tooling and the note: the roster's watcher lists held seats; the charge's watcher tells the charged key "answer
   with YES by DAA X or the stake is gone"; `deposit` traces a charge to a roster's `charge` as it traces a claim to a
   pair's `propose`; N§5a and N§7 say that a seat must hold, or trust, judge stake aged under its roster's judge block.
   Breaks nothing; raises the answer probability from zero.
2. A floor on the verdict: the charge records the stake it holds (the seat's weight, the exit's amount) and `decide`
   writes refuted only if the dispute's NO total is at least a fixed share of it (lapsed otherwise). A false charge
   then puts at risk capital that grows with what it burns. It could break: a real liar larger than the judge's
   honest stake is not slashed, so a roster's seats should not outgrow its judge, which is worth stating anyway.
3. No reward from a slash decided without an answer (the tenth burned too): removes the profit, not the grief or
   the takeover.

## C5-F2 (Medium): a charged seat forfeits itself, and the charge keeps its author's bond for ever

**Affected.** `KpfRoster.forfeit`, `KpfExit.forfeit` (the holds end with the stake, the tenth is the sender's),
`KpfRoster.slash` (the seat's other holds end), `KpfCharge.settle` (only beside its stake), N§5a "Left behind".

**The missing rule.** A charge has no way out once its stake is gone, and a forfeit pays its sender while the stake
is under a charge. N§5a says "nothing is owed to it: the stake it wanted slashed is slashed"; it does not say that
the charged key decides when, and is paid for it.

**The attack** (`c5_exploit_a_refuted_seat_forfeits_itself_first_...`). Seat 1 (22 KAS) signed 20 for a feed of 10.

1. The author charges it (5 KAS), a stake contests, nobody answers, `decide`: refuted at 6,760. The author's slash is
   built and good.
2. The seat signs 10 and 11 for the tick 6,700 and sends `forfeit` with its own funding: 2.2 KAS to itself, 19.8
   burned, its holds zero.
3. The author's slash against the roster as it is now: refused (`held`). A clear: refused. The charge beside a key
   input: refused. The library and `charge-settle` refuse likewise ("no seat of the roster holds that key").

The seat lost 19.8 KAS instead of 22. The author is out 5 KAS for ever (10 on the testnet defaults) and gets no
tenth. The seat risks nothing by waiting for the verdict first: its stake is held either way. So against a seat that
acts in its interest a charge always costs its author the bond, and reporting a false price is a loss.

The same without malice: of several authors who charge one seat for one false tick, the first slash is paid and the
bonds of the others stay in their charges (`c5_bounded_a_charge_waits_behind_nine_later_ones_until_one_lapses` leaves
eight such charges, 40 KAS). `charge` does not warn that the seat already has a hold (C5-N4).

**Smallest fix, and what it could break.**

- One line that removes the gain: `forfeit` requires the whole stake at the burn script (no tenth to the sender)
  while the stake has a hold. It breaks the reward of an honest two-price reporter on a charged seat, and the bond
  is still stranded when the seat chooses spite.
- The fix of the cause, for a seat: `forfeit` and `slash` empty the weight and keep the other holds; `slash` and
  `clear` run on an empty seat (nothing to burn: a refuted charge's bond back to `payKey`); `join` refuses an empty
  seat whose hold is not zero. It could break: a seat index stays closed until its charges settle, at most
  `chargeDaa` plus a dispute after the forfeit (no charge can be added to an empty seat). An exit needs the same and
  has no place to keep it: a forfeited exit leaves no UTXO, so it would have to leave a carrier holding the holds.

## C5-F3 (Low): a hold costs the same whatever it holds

Any tick a key ever signed for the roster charges it, true or not, on a seat or on an exit past its release
(`KpfExit.charge` has no deadline). An uncontested charge holds the stake for `chargeDaa` and burns its bond: 5 KAS
per 300 DAA on the fixture, 10 KAS an hour, 240 KAS a day, on the testnet defaults. A charge contested at the last
DAA and not answered holds for `chargeDaa + roundDaa` (13 hours) for one bond, 18.5 KAS a day, with a judge stake of
100 KAS per charge that comes back (its age restarts, so about thirteen stakes rotate). If the contest is answered
its author loses the stake, so this variant is cheap only where F1 is open. The shield (nine charges after a real
one) costs 90 KAS an hour and only delays: in the test the ninth lapses 300 DAA after it landed, is cleared by
anyone, and the real charge is slashed. A charge
also respends the roster without a rest (sequence 0) and restarts the 20 DAA rest of `join`, `topUp` and `leave`: a
bond per 20 DAA, 5 KAS a second on the testnet defaults. All bounded and paid by the author; Low because the price
is per day, not per KAS held. `chargeBond` could instead be a share of the stake it holds.

## C5-F4 (Low): what a false tick is, stated three ways

N§5a and `TESTNET.md` 10.5: YES if the price "lies within the roster's spread of the feed at that DAA score" (testnet
`spreadBps` 10: 0.1 %). N§7: "only a clear lie (beyond `g`) is judged at all" (`g` 0.5 %). `charge-contest` prints the
first. `deposit`, the only command that answers, prints the oracle family's: "within the market's g of the feed both
at its t0 and at P (the DAA it landed at)"; a tick has no landing. Review C3's margin allows honest seats to stand up to
twice the spread apart: under N§5a's wording a seat 0.11 % from the feed loses everything on an honest
verdict. One rule should be written once (the note), and printed by both commands.

## C5-F5 (Low): nothing looks at a roster's charge parameters after it is made

`roster-create` refuses a judge block outside the judge's bounds. `roster-join --genesis`, `c-market` and the
market's warnings read none of it: a roster made with `--charge-daa 1` (a contest must land within one DAA of the
charge: every charge lapses, no seat is ever slashed for a false price) is created, joined and given a market with
no warning (`c5_bounded_the_charge_commands_at_their_edges`). The contract's `checkCharge` checks the judge block's
form only, by design. For a joiner the judge block is who can take its stake (F1); for a trader it is whether lying
costs the roster anything. Both commands should print the block and warn on a `chargeDaa` a contest cannot meet and
on a block that fails `JudgeParams::check`.

## Notes

**C5-N1.** `settle` reads four things of the input it names: the roster's id, a length, five bytes at 310 and an
8-byte first push. A retiring ticket and a withdrawing exit, given filler below their arguments, satisfy all of it:
the charge's script returns Ok, and the ticket's and the exit's fail with `CleanStack(2)`. Without filler nothing
reaches the shape, though the engine lets every push be written longer and an `int` be eight bytes (a `topUp` whose
seat index is `08 01 00 .. 00` is accepted):

| Input of the roster's id | Bytes before its script must be | Entries, shortest to longest | Verdict |
|---|---|---|---|
| the roster | 318, with its own tag at 310 | `topUp` 14..40, `forfeit` 199..241, `slash`/`clear` 318 and up, `charge` from 1,676, `roll` from 2,512, `join` and `leave` from about 3,980 | only `slash` and `clear`; with any longer tag or script push the five bytes at 310 are not a tag |
| an exit, at its own length | 318 | `withdraw` 8..14, `forfeit` 198..228, `slash`/`clear` 318, `charge` from 1,675 | only `slash` and `clear` |
| an exit, at the roster's length | 11,534 | none above 1,800 | none |
| a ticket, at the exit's length | 1,289 | `retire` 8..14, `attest` 1,153..1,180 | none: 109 bytes short, and its first push is 32 bytes |
| a ticket, at the roster's length | 12,505 | | none |

The table is by reading and holds while no entry of the three templates takes an argument of free length. A new
entry, or a new template under the roster's id, must be checked against it; an explicit template check in `settle`
is not possible (the cycle the note describes), so this table is the check.

**C5-N2.** In `generate_committee`, after the loop `cc` was just set from the last roster and exit and `rc.charge_tpl`
from the last charge, so both `assert_eq!` are true whatever happened. The lengths and tags do not depend on the
constants' values (two-byte numbers, four-byte tags), so the second round is the fixed point and `check` compares
every file with a fresh compile. Asserting `done` after the loop would make the guard real.

**C5-N3, C5-N4.** As in the table.

## Review C4's notes after the changes

| C4 note | Now |
|---|---|
| C4-N1 | stated in N§5a; its consequence for a charged seat is C5-F2 |
| C4-N2 | closed: `tick-sign` keeps a journal beside the key and refuses a second price into any file (`c3_fixed_the_signer_refuses_a_second_price_for_a_tick_into_any_file`) |
| C4-N3 | closed: the runbook's market is made with `--late-daa 600` |
| C4-N4 | closed by the coordinator's account (the offline age check); `c3_fixed_c_cure_refuses_a_void_claim` passes with its void claim built without a clock |

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| a | A charge at its birth: a sompi below the bond; born refuted or contested; naming another key, another roster, another price than the one signed, another judge, a shorter wait; `k` the roster's continuation; under the roster's own id; a twin under its id | refused by the roster (control: the same transaction with its id recomputed is accepted) | `c5_holds_only_a_charge_its_stake_wrote_is_among_its_holds` |
| a | A refuted `KpfCharge` under an id the roster did not write, with every witness that names the real id (after it, before it, as the holds) | refused: the input's own id is hashed into the fold, and the fold is a chain of blake2b over 64 bytes | the same test |
| a | The real charge against another seat; the holds themselves as `a`; the charge's id again in `later` | refused | the same test |
| a | Zero ids, a hold of zero, grinding | by reading: an empty hold has no witness (a preimage of zero), `fold` skips only zero ids and `fresh` refuses a zero id, a genesis id commits to the charge's value and script | |
| a | One output taken as their charge by two stakes | by reading: the charge's state names one roster id and one key; a roster and an exit of one id are never two inputs (`OpCovInputCount == 1`) | |
| a | A roster made with holds already set | by reading: `join` writes a zero hold for the seat it fills; the tools refuse a roster that did not begin empty | |
| b | `settle` beside a key input, naming itself; beside the roster's `topUp`, `charge`, `leave`, `forfeit`, `roll` (each a control that is accepted without the charge) | refused by the charge | `c5_holds_a_charge_leaves_only_with_its_own_stake` |
| b | A ticket's `retire` and an exit's `withdraw` dressed as a slash | the charge passes; the engine's clean-stack rule refuses (C5-N1) | `c5_holds_a_ticket_or_an_exit_dressed_as_a_slash_...` |
| b | A status the entry does not serve: refuted cleared as lapsed or upheld; contested slashed or cleared; pending slashed (seat and exit); upheld slashed; upheld cleared with its bond burned | refused (each built for a charge of the status the entry wants, the real charge put in its place) | `c5_holds_a_charge_leaves_only_with_its_own_stake` |
| b | A pending charge cleared a DAA before `chargeDaa`, on a seat and on an exit | refused; at `born + chargeDaa` accepted | the same test |
| b | Two charges of one seat in one transaction; the stake naming the second while the first settles | refused | the same test |
| b | Two stakes of one roster id in one transaction | by reading: every entry of the roster and the exit wants one input of the id | |
| c | A contest by a stake of another judge block, a DAA too young (exactly `ageDaa`: accepted), keeping a sompi, the dispute at another output, of a contested or decided charge | refused | `c5_holds_the_judge_serves_a_charge_as_it_served_a_claim` |
| c | The verdict a DAA early, keeping a sompi, written upheld when NO leads, rewriting `payKey`, from another charge's dispute, beside the dispute's `deposit` | refused | the same test |
| c | After the verdict: the pool pays and closes while the charge waits; the slash runs after the pool is gone; the pool or a receipt inside the slash | accepted, accepted, refused | the same test |
| c | A late contest; YES leading | lapsed; upheld | the coordinator's `charge_builders`, and `c5_holds_a_charge_leaves_only_with_its_own_stake` (upheld) |
| d | The shield: nine charges after a real one | bounded: no witness until one lapses, then the slash is accepted (C5-F3) | `c5_bounded_a_charge_waits_behind_nine_later_ones_until_one_lapses` |
| d | A seat charging itself and winning | the bond and a tenth back, nine tenths burned: the forfeit's arithmetic (C4-N1); with C5-F2 it does not need its own charge | by reading |
| e | A held seat rolled (in the ticket with its weight, holds kept), topped up (hold kept, the added stake under it), outbid (the exit carries the hold and 11 KAS, the new holder has none and is not slashed for the old one's tick), the exit slashed for all it holds, a held exit not withdrawn | as the note says | `c5_bounded_a_held_stake_keeps_its_hold_wherever_it_goes_and_the_heaviest_shapes` |
| e | The backing after a slash | the roster keeps its value less the seat's weight, and its table sums to that less the carrier | `c5_exploit_an_unanswered_contest_...` (the roster's value at the end), the coordinator's test |
| e | Mass through `verify::verify`: charge 23,204 compute / 35,008 transient; roll with six tickets 25,525 / 37,550 and 240,137 storage; join over a taken seat 24,706 / 39,612; leave of a held seat (19,340 bytes of signature script) 25,634 / 39,668; slash of a seat 22,783 / 36,046; of an exit 9,522 / 13,544 | all under a standard transaction's 100,000 but for storage (C5-N3) | the same test |
| e | Data signatures as a node counts them: 3 in the roster's script and 3 in the exit's | holds | `c5_holds_a_charge_leaves_only_with_its_own_stake`, `c5_bounded_a_held_stake_...` |
| f | The builders' payees: slash (bond and tenth to `payKey`, the burn last), clear (the charged key, or the burn script); `charge_settle` choosing by status | agree with the contracts in every status (the edited transactions above are the builder's own for the status claimed) | `c5_holds_a_charge_leaves_only_with_its_own_stake` |
| f | `charge-settle` on a pending charge without a clock, and before `chargeDaa`; `hold-witness` with an id not among them and with nine after it; a stake bonded from another roster file; a late contest (warned, then lapsed) | refused or warned as they should be | `c5_bounded_the_charge_commands_at_their_edges` |
| g | The pinned `KpfCharge` holds the pinned roster's and exit's lengths (15,334 and 4,118) and the tags of the `slash` and `clear` that ran; `kpf-artifacts check` | holds; `check` compares every file with a fresh compile | `c5_holds_a_charge_leaves_only_with_its_own_stake`, the run above |
| | The offsets the roster, the exit, the charge and the judge read in each other | are the models' bytes | `c5_holds_the_layouts_the_templates_read_in_each_other` |

## Not checked

- A node: the races (a forfeit against a slash, a clear against a contest at `born + chargeDaa`), mempool behaviour of
  the 19 kB signature scripts, a live dispute.
- `KpfStake` beyond what a charge asks of it (`open`, `deposit`, `resolve`, `settle`, `close` as the builders use
  them); a pool with losers on both sides of several rounds.
- The widths of `attest`'s arguments on the compiled ticket (C5-N1 rests on the ticket's first push, not on them).
- What a judge decides for a tick of a market the roster never priced, or a tick of the future: the contracts carry
  any 32 bytes and any 8.
- The walks do not drive charges.
- `TESTNET.md` 10.5 was not run against a node, and was being edited during this round.

## Judgement

This round found a High and a Medium: the loop does not stop here. The covenant work of the judged slash is sound
and I could not break it: the holds are a hash chain with no second reading, a charge's id is in a stake's holds only
if that stake's `charge` wrote it, a charge and its stake leave together or not at all, and the judge's interface
serves a charge as tightly as it served a claim. What is wrong is what the verdict is allowed to mean. The family
was built so that no price rests on somebody answering in time, and stage 2 puts every seat's stake, and through an
emptied roster every pair, on exactly that: a contest of one minimum stake that nobody answers within a round. And
the one honest actor the design needs, whoever reports a false price, is the one who loses money when the seat it
reports acts in its own interest. F1 needs a decision about what an unanswered contest may do; F2 needs a charge to
have a way out when its stake is gone.

## Status after the fixes (review C6)

| Finding | Now |
|---|---|
| C5-F1 (High) | closed as found: a verdict takes at most its dispute's NO total and pays a tenth of at most that; the watcher raises a held seat and a charge against `--me`'s key; `charge-answer`. What is left is the stated limit (aged stake of the seat's size, returned if nobody answers; the bet pays above 91 % of silence): `c5_fixed_an_unanswered_contest_takes_no_more_than_its_no_side_had_at_risk`, `auditC6_builders::c6_bounded_a_seat_is_emptied_by_silence_only_with_its_weight_in_aged_stake` |
| C5-F2 (Medium) | closed for the forfeit (a held stake keeps a tenth under its holds, nobody is paid, the charge is settled): `c5_fixed_a_charged_seat_shown_two_prices_keeps_a_tenth_under_its_holds_and_the_charge_is_paid`. Open where a slash takes a stake whole (review C6, C6-F1, Medium); the tenth kept is 1 KAS after a replay (C6-F2) |
| C5-F3 (Low) | stated, unchanged |
| C5-F4 (Low) | closed: one rule, printed by `charge`, `charge-contest`, `charge-answer` |
| C5-F5 (Low) | closed at `roster-create` and `roster-show`; `roster-join` and `c-market` say nothing (C6-N2) |
| C5-N1 | stated in `KpfCharge`'s header |
| C5-N2, C5-N3, C5-N4 | closed |
