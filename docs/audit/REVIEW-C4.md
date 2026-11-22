# KPF review C4 (the fixes of review C3, and the first part of stage 2: two prices for one tick)

## Scope and method

Target: `KpfRoster.forfeit` and `KpfExit.forfeit` (new), `KpfOpenC.refund` (changed), their models
(`committee::{burn_of, equivocation}`, `Roster::forfeit`, `Exit::forfeit`, `COpen::refund_fee`), the builders
`committee_build::{forfeit_seat, forfeit_exit}`, the commands `seat-forfeit`, `exit-forfeit` (`ccmds.rs`: `tick_raw`,
`proof`) and the three commands changed for review C3 (`c-order`, `c-cure`, `roster-roll`), read against
`docs/DESIGN-COMMITTEE.md` 5a (first paragraph, "built") and `docs/TESTNET.md` section 10.

Files of this round (nothing else was changed; `REVIEW-C3.md` got a status section):

- `crates/kpf-tests/tests/auditC3_tests.rs` and `crates/kpf-cli/tests/auditC3_cli.rs`, brought to the fixes: one and
  three `c3_bounded_` tests became `c3_fixed_` (the refusal asserted where the gap was). No fix of review C3 failed.
- `crates/kpf-tests/tests/auditC4_tests.rs`: 4 tests (3 `c4_holds_`, 1 `c4_bounded_`).
- `crates/kpf-protocol/tests/auditC4_builders.rs`: 1 test (`c4_holds_`): the forfeits and a ticket of fifteen signers
  through the builders and `verify::verify`.
- `crates/kpf-cli/tests/auditC4_cli.rs`: 1 test (`c4_holds_`): the two commands through the binary.

Runs:

- `CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test auditC1_tests --test auditC1_walk_tests --test auditC2_tests --test auditC3_tests --test auditC4_tests`:
  `ok. 22 passed`, `ok. 2 passed`, `ok. 12 passed`, `ok. 3 passed`, `ok. 4 passed`.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-protocol --jobs 2 --test auditC2_builders --test auditC3_builders --test auditC4_builders`: three times `ok. 1 passed`.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test auditC3_cli --test auditC4_cli --test committee_cli`: `ok. 6 passed`, `ok. 1 passed`, `ok. 2 passed`.
- The family's own `roster_tests`, `committee_tests`, `committee_paths_tests`, `c_experiments_tests`: 16, 8, 10, 4 passed.
- By hand with the built binary: `c-market --late-daa 700 --grace-daa 0` (the two new warnings are printed).

## Findings

**Nothing above Low; nothing at Low either.** Four notes.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C4-N1 | Note | The tenth is whoever's transaction lands, so it is also the offender's: a seat that sees its two signatures published slashes itself and keeps 2.2 of its 22 KAS (the penalty is nine tenths, and the reporter is paid nothing). The note says a miner can take the share; the offender is the party with the most reason to | `c4_holds_a_slash_moves_the_slashed_stake_and_nothing_else` (the transaction needs no key) |
| C4-N2 | Note | What protects an honest seat from its own software is one file name: `tick-sign` refuses a second price over the file of the first, and signs it into any other `--out` (review C3, C3-N2, unchanged). With a forfeit behind it the cost of that is the whole stake for one sompi of difference: two signer instances, a restart, or a re-signing with a fresher feed are all proofs. A journal per key (roster, market, tick) kept by the signer would be the guard the forfeit now deserves | `auditC3_cli::c3_bounded_the_signers_guard_against_a_second_price_is_one_file_name`, `auditC4_cli` (the two files are a proof the commands accept) |
| C4-N3 | Note | `docs/TESTNET.md` 10.4 as written cannot be typed by hand on the market 10.3 creates: three `tick-sign` calls, a `tick-check` and a `c-close` must land within the default `lateDaa` of 100 DAA (10 s) of the tick, or the close is refused by the tool (with `--node`) or void. Live run 3 used a market with `lateDaa` 600. The section should say so, or build its market with `--late-daa 600` (which now prints a warning that is true) | by reading (`TESTNET.md` 10.3, 10.4; `ccmds.rs` `claim`); `LIVE-3.md` |
| C4-N4 | Note | Still open from review C3 by its own wording: offline (`--daa`) `c-close` and `c-liquidate` compare the tick's age with nothing, and with a node they leave no margin for the landing | `auditC3_cli::c3_fixed_c_cure_refuses_a_void_claim` (its bounded half) |

## Review C3's findings after the fixes

| C3 finding | Now | Test |
|---|---|---|
| C3-F1 (Low) | closed: the refunder's fee is `min(keeperFeeMin, escrow / 20)`; on a market with a floor of 2,000 KAS the refunder of a 1,001.2 KAS order keeps 50.06 KAS, and one sompi to the owner is refused | `c3_fixed_the_refunders_fee_is_at_most_a_twentieth_of_the_escrow` |
| C3-F2 (Low) | closed: `c-order` refuses a market file without its roster's facts (or `--unchecked`) and an expiry not after the present | `c3_fixed_c_order_refuses_a_market_file_nobody_could_check_and_an_expiry_already_past` |
| C3-F3 (Low) | closed for the cure (a void claim is refused, `c-finalize` named); the offline age check is C4-N4 | `c3_fixed_c_cure_refuses_a_void_claim` |
| C3-N1 | closed: `--steps` into an epoch that has not begun is refused, into one that is over is warned | `c3_fixed_roster_roll_with_steps_is_compared_with_the_clock` |
| C3-N2 | open (C4-N2) | |
| C3-N3 | unchanged (a builder's message; nothing to fix) | `auditC3_builders` |
| C3-N4 | closed for the warnings (run by hand); the watcher's price test is as it was | |

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| a | Slashing a seat that did not sign two prices: signatures for another roster, another market, two ticks, by another key, one price twice | refused | `c4_holds_only_two_prices_of_one_key_for_one_tick_of_one_market_and_roster_slash_a_seat`, `auditC4_cli`, `roster_tests` |
| a | The digest's layout: the same signed bytes cut elsewhere (the tick as 7 bytes and each price as 9; the market as 31 and the tick as 9), and what that cut would make of an honest seat (one price of 0.0003 KAS signed for the ticks 327,680 and 393,216, read as one two-byte tick and two fourteen-byte prices) | refused: the script enforces every argument's width, so a message has one reading (tag 10, roster 32, market 32, tick 8, price 8) | the same test (the signature script rewritten by hand; the control with unchanged widths passes) |
| a | The price's encodings: zero and negative zero, both signed by the seat's key | refused: the numbers are compared, not the bytes; zero and one sompi are two prices | the same test |
| a | A tick digest equal to something else a seat's key signs | by reading: a transaction signature is over the keyed transaction hash, a tick's over blake2b of 90 bytes that begin with the tag; the roster id is in the digest, so nothing signed for another roster, or before this one existed, counts. A key used for seats of two rosters is two keys to the forfeit | by reading |
| a | Stealing with a slash: the burn a sompi short, not at the mirror index, to a key; the roster a sompi short; another seat emptied; a second output of the id; two rosters slashed in one transaction with one burn output (refused; with two it is accepted); a roster and an exit of one id slashed together | refused: the roster keeps all but the seat's weight, nine tenths of it reach the burn script (the hash of `OP_RETURN`, checked), a tenth is the sender's | `c4_holds_a_slash_moves_the_slashed_stake_and_nothing_else`, `roster_tests` |
| a | A burn below the dust floor | needs a stake below 0.0223 KAS; a seat is at least 1 KAS (`minSeat`), an exit is a seat's stake | by reading (`burn_of`) |
| a | A slashed seat's harm | bounded: it stays in its epoch's tickets with its old weight and no stake, and a quorum may still need it: at most one epoch (3,000 DAA, 5 minutes, on the testnet defaults); the next roll writes the table without it (78 of 100 KAS, a quorum of 52.26) | `c4_bounded_a_slashed_seat_stays_in_its_epochs_tickets` |
| a | Escaping: leaving before the proof lands (the roster refuses the empty seat, the exit is slashed by the same proof); slashing the exit after its release and before the withdrawal (accepted: the two are spends of one UTXO); an outbid seat's exit; a proof against another key | holds: a proof has the exit time after the end of the last epoch whose tickets hold the key (72,000 DAA, 2 hours, on the testnet defaults; the walks of review C1 assert that release for every exit) | `c4_holds_a_stake_on_its_way_out_is_slashed_until_its_withdrawal_lands`, `auditC4_cli` |
| a | A self-slash as an exit | a tenth at once against all of it after the exit time: never better than leaving (C4-N1 for what it is better than) | `c4_holds_a_slash_moves_...` |
| b | Signature operations as a node counts them: the roster 2 data signatures and 1 transaction signature (forfeit, leave), the exit 2 (forfeit, withdraw), the ticket 15 (a renewal in which all fifteen seats signed; a retire) | every transaction passes `verify::verify` (at most 15 an input) | `c4_holds_the_forfeits_and_a_ticket_of_fifteen_signers_pass_the_nodes_count_of_signature_operations`; the pair's and the order's by `auditC2_builders`, `auditC3_builders` |
| c | The three fixes of review C3 | complete but for C4-N4 | the `c3_fixed_` tests |
| d | `TESTNET.md` section 10 against the usage text and the commands: every flag exists (`--genesis`, `--market`, `--key`, `--tick`), `kpf daa`'s output feeds the `sed` as written, the forfeit commands take the two tick files | one line misleads (C4-N3) | by reading; `committee_cli`, `auditC3_cli`, `auditC4_cli` run the same commands offline |
| e | The builders of the forfeits: the roster at output 0, the change (with the tenth), the burn last (the mirror index of input 0); the sender's change is its funding plus the tenth less the fee to the sompi | holds | `auditC4_builders` |

## Not checked

- A node: the race of a withdrawal against a proof at the release, of the offender's own slash against a reporter's.
- The walks do not drive forfeits (the roster walk's model has no equivocating seat).
- The rest of N§5a (charge, contest, slash by the judge, pools, the fee per claim): designed, not built.
- `TESTNET.md` section 10 was not run against a node; "usable from DAA X" (the rest of the roster) is as the text says
  only by reading.

## Judgement

This round found nothing above Low, and nothing at Low: the stopping condition holds for a second round. The forfeit
is small and closed: a proof is two signatures of one key over two messages that differ only in the price's eight
bytes, the widths are enforced so no honest signature has a second reading, the numbers are compared so zero's two
encodings are one price, the slashed stake goes to the burn script but for the sender's tenth, and a stake that left
is slashed where it went until it is withdrawn. What it adds to an operator's risk is real and stated: one sompi of
difference between two signatures of one tick costs the whole stake, and the tool's guard against that is a file name.

## Status after the changes (review C5)

| Note | Now |
|---|---|
| C4-N1 | stated in N§5a (the offender can take the tenth). With the judged slash it has a second consequence: a charged seat that forfeits itself also strands its accuser's bond (review C5, C5-F2) |
| C4-N2 | closed: `tick-sign` keeps a journal beside the key file and refuses a second price for a tick into any file; `--second-price` overrides (`auditC3_cli::c3_fixed_the_signer_refuses_a_second_price_for_a_tick_into_any_file`) |
| C4-N3 | closed: `TESTNET.md` 10.3 makes its market with `--late-daa 600` |
| C4-N4 | closed by the coordinator's change (`c-close` and `c-liquidate` compare the tick's age with `--daa`); `auditC3_cli` passes with it |
