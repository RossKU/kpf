# KPF review C8 (cross margin: the trim, and the unattended commands)

## Scope and method

Target: `contracts/KpfPairC.sil` `propose` kind 3 (trim), `contracts/KpfPendingC.sil` (`settle` for a trim, `cure`
for a liquidation only, the grace), the model (`cmarket::{trim_state, trim_take, trim_ok, trim_bound_of,
trim_bound_for, trim_pending}`, `CSettlement::Trim`), the builder `committee_build::trim`, and by reading the
commands `c-trim`, `c-account` (`position_of`, `liq_line`), `c-cross-run` (`cross_round`, `newest_attested`),
`seat-run` and `roster-roll --wait` in `crates/kpf-cli/src/ccmds.rs`; `docs/DESIGN-COMMITTEE.md` 3a (N§3a).

Files of this round (nothing else was changed):

- `crates/kpf-tests/tests/auditC8_tests.rs`: 5 tests on the engine (3 `c8_holds_`, 2 `c8_bounded_`).
- `crates/kpf-protocol/tests/auditC8_builders.rs`: 3 tests on the model (1 `c8_holds_`, 2 `c8_bounded_`).
- `crates/kpf-cli/tests/auditC8_cli.rs`: 1 test, **written and not run** (marked `#[ignore]`): for the whole round
  `target/debug/kpf.exe` was held by a live `roster-roll --wait` loop against a testnet node, cargo could not
  rebuild it, and I did not touch that process. The file type-checks (`cargo check -p kpf-cli --test auditC8_cli`).

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`):

- `kpf-tests`: `auditC8_tests` `ok. 5 passed`; `auditC1_tests` 22, `auditC1_walk_tests` 2, `auditC2_tests` 12,
  `auditC3_tests` 3, `auditC4_tests` 4, `auditC5_tests` 2 passed.
- `kpf-protocol`: `auditC8_builders` `ok. 3 passed`; `auditC2_builders` .. `auditC4_builders` 1 each,
  `auditC5_builders` 7, `auditC6_builders` 7, `auditC7_builders` 3 passed.
- `CARGO_BUILD_JOBS=2 cargo test -p kpf-artifacts --jobs 2`: `ok. 2 passed` (the templates and pinned constants).
- `kpf-cli`: nothing could be run this round (see above), mine or earlier.

## Findings

**No covenant rule of the trim was broken, and nothing in the contracts is above Low. One Medium in the tools, by
reading and on the model, not by a run: C8-F1.** Three Low, four notes.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C8-F1 | Medium (by reading) | `c-cross-run` acts on every pair file in its directory that names its key and is at the node, and pays margin into it from the wallet and then from the key's other positions (trim, add). It checks neither where the pair comes from nor whether margin added to it stays the key's. A pair anyone writes by a genesis (the key as the long, an entry below the long's bound, a lease already run out, in a market whose ticks the runner has) is "a position near its liquidation"; what the runner adds is its maker's at the lapse | `cross_round` and `position_of` by reading; `auditC8_builders::c8_bounded_a_side_trimmed_past_its_entry_gets_nothing_at_the_lapse_and_neither_does_what_it_adds` (the model's lapse) |
| C8-F2 | Low | The runner trims a position past its entry (the family's own test: from 9 to 10.0062425 for an entry of 10) and renews no lease. At a lapse a side trimmed past its entry is paid nothing, and neither is what it added since. N§3a says a trimmed side keeps its lease alive or closes; the process that trims for it does neither | the same model test; `committee_cli::the_positions_of_a_key_are_kept_linked_by_a_runner` (the trim past the entry); `auditC8_cli` (not run) |
| C8-F3 | Low | A trim to just inside the entry leaves that side a sliver of worth at the entry, and `checkPart` wants PART_FLOOR of it in every piece: nobody splits the pair any more. The other party closed half before; after, it closes whole or not at all; a liquidation is whole or not at all. Trimmed to the entry or past it, splits are allowed again | `auditC8_tests::c8_bounded_a_trim_to_just_inside_the_entry_ends_every_split_of_the_pair` |
| C8-F4 | Low | `seat-run` counts ten DAA a second between two readings of the node (one a minute by default) and signs the tick `lag` (20 DAA) behind its estimate. If the DAA score runs behind that by more than 20 DAA, the tick it signs is of the future: what `tick-sign` refuses, and a signed price for a moment that has not come is a price a charge can call false | by reading (`seat_run_cmd`) |
| C8-N1 | Note | A pending trim delays a due liquidation by one transaction: anyone settles it at once (a trim has no grace) and the liquidation follows on the trimmed pair with no quiet time. A side liquidated at the present tick trims only with a tick of a better moment within lateDaa, and that takes less than its close at the same tick would | `c8_bounded_a_trim_delays_a_due_liquidation_by_one_transaction_and_pays_for_it` |
| C8-N2 | Note | `liq_line` is the model's last liquidating price to within 1 sompi for a long and 2 for a short, not "to a sompi" (0 of 10,000 sampled bounds exact) | `auditC8_builders::c8_bounded_the_liquidation_line_the_commands_print_is_the_models_within_two_sompi` |
| C8-N3 | Note | The loop the source suggests for `roster-roll --wait` has no pause: when the newest roster file is of a spent UTXO (a join, a charge or another roller spent the roster) every pass fails at once and the node is asked as fast as the shell loops | by reading |
| C8-N4 | Note | "No rescue by others": the reasoning holds. A way that would be sound, and why it does not fit this family, is below | by reading |

## C8-F1 (Medium, by reading): the runner pays margin into any pair file that names its key

**Affected.** `ccmds::cross_round` (`mine`, the `c-add-margin` and `c-trim` sub-commands), `position_of`.

**The missing check.** A pair counts as the key's position if exactly one of its keys is the key and (with a node)
its UTXO exists. A pair state is whatever its script says, and anyone writes a pair script by a genesis: the UTXO
exists, the market is a real one (the state carries the market's parameters; the runner has its ticks), the key is
the long. Nothing in the round asks whether the key ever signed into this pair, or what a lapse pays.

**The attack, as transactions.** The market is one the victim trades; its price is 11.3.

1. The adversary writes, by a genesis it funds, a pair of the market: the victim's key as the long, its own as the
   short, `bl` at 10.57 (the long's liquidation line at 11.07, 2 % below 11.3), `bs` at 13, the entry 5, a lease that ran out more
   than `idleDaa` ago. It gets the pair's file into the runner's `--dir`.
2. The runner's next round: "the long of ... is 2 % from its liquidation: it needs X KAS", `c-add-margin` from the
   wallet; if the wallet does not hold X, `c-trim` of the victim's positions with margin to spare, `c-finalize`,
   then `c-add-margin`. The need grows with the made-up pair's size, so the wallet and every position's free margin
   go into it (the round warns only of what it could not cover).
3. The adversary sends `lapseSettle`: the pair ends at its entry clamped to the range, which is the long's bound.
   The long is paid nothing; all the pair holds, the victim's margin with it, is the short's.

Step 3 on the model: a long whose bound is above the entry gets 0 at the lapse, and margin it adds while the bound
stays above the entry is the short's to the sompi (`c8_bounded_a_side_trimmed_past_its_entry_...`). Steps 1 and 2 are
by reading: I could not run the runner (see Scope). The condition is a file in the runner's directory. The toolchain
has no discovery but files, a position's file goes stale whenever the other party acts on the pair, and so files
from elsewhere are what a runner is fed with: I count the condition realistic, and it is the one the coordinator
named.

**Smallest fix, and what it could break.** In `cross_round`, before any margin moves into a pair: (a) the lapse
would pay the side more for the margin added (its bound stays on its side of the entry after the move), and the
lease is alive with time to spare; (b) the pair is one the key entered: its file was written by this directory's own
commands, or is listed in a file the owner keeps (`--positions`), not merely found. (a) alone stops this attack and
C8-F2's loss; it could break nothing but the rescue of a position already trimmed past its entry, which then needs
its lease renewed first (the runner can do that: `c-keep-alive`).

## C8-F2 (Low): the runner trims past the entry and renews nothing

Before trims a lapse cost a winner its gain: each side was paid its worth at the entry. A side trimmed past the
entry has had that out already (the model: 1.2 KAS taken against 1 KAS owed at the entry) and is paid 0; whatever it
still had in the pair, at least its maintenance and g of the notional, is the other party's, and anyone sends the
lapse once the lease and `idleDaa` have passed (8 days by default). N§3a says so. `c-cross-run` chooses the bound by
the amount it needs (`--take`), not by the entry, and has no `c-keep-alive`. An owner who leaves it running, as it is
meant to be left, finds out at the lapse. Fix: the runner renews the lease of every pair it holds a side of once a
quarter of the term is left (a renewal must add a quarter: `keepAlive`), and `c-account` and `c-watch` say of a side
"trimmed past its entry: a lapse pays it nothing; lease to DAA X". What `c-account` and `c-watch` say today I could
not run.

## C8-F3 (Low): a trim to just inside the entry ends every split

`checkPart` gives "each side with value at the entry" at least PART_FLOOR (0.2 KAS) in each piece of a split, at the
entry clamped to the range. After a long's trim to one sompi inside the entry (10 less a sompi, at an attested
11.5), half the pair gives the long 500 sompi at the entry: every split is refused, the short's close of half and a
keeper's liquidation of half among them; the whole pair still closes, is liquidated and lapses. At the entry itself
or past it the side has no worth there and the floor does not apply. The other party loses the choice of size, not
an exit. A trim could be refused when it leaves its side less than PART_FLOOR per `minSize` at the entry and more
than nothing; it would break only such trims.

## C8-F4 (Low): `seat-run` can sign a tick of the future

`est = base + elapsed / 100 ms`, `t0 = grid * floor((est - lag) / grid)`, the node read once every `--sync-secs`
(60, at least 10). The estimate is ahead of the DAA score whenever blocks come slower than ten a second: 3.3 % slow
over a minute, or a stall of two seconds, is more than the 20 DAA of `lag`. The tick is then signed for a DAA that
has not come, with the price of now. A ticket refuses it until its time; the judge's rule is the feed at the tick's
DAA score, so a move beyond g before that time makes it a false tick, signed, and chargeable. After the next reading
the estimate falls back and no tick is signed twice (`t0 > last`, and the journal). Fix without more queries than a
seat can afford: sign only ticks at or before the last DAA score READ plus what a tolerance allows (`--lag` at least
the drift a sync interval can hold), or read the node before each tick (one query per `grid`: six a minute at 100).

## Notes

**C8-N4: no rescue by others.** The reasoning is right: a covenant sees the script and state of the input beside it,
and a state is written by anyone, so "a pending liquidation of a position of the same key" proves nothing; C8-F1 is
the same fact met from the tool's side. What a covenant can not be shown falsely is a covenant id. A rescue would be
sound if the key's consent named lineages and not keys: a data signature of the side's key over (a tag, the source
pair's id, the destination's id, the side), checked by the source pair, which then requires the input beside it to
carry that id, to be a pending liquidation of that side by the pinned template, and its output to be the cure with
the freed margin added. A made-up pair has another id, and the key signs only ids of pairs it entered. It does not
fit this family because ids do not stay: every claim and every hand-over writes its part under a fresh id, so a
consent goes stale at the first claim on either pair, which is exactly when it is needed. It needs one lineage that
holds the positions: the account covenant of section 8.

**What I agree with in N§3a.** "It never had more: the trimmer could end the pair at the same tick with a close" is
true to the sompi on the fixture: over 62 trims at their limit (both sides, attested prices from 8.5 to 13) and 71
true prices each, the other party is left at least the lesser of what it had and what the close at the same tick
would have paid it, and the trim takes less than the close pays its side. Inside the new range the other party's
worth is unchanged.

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| trim | A bound a sompi nearer than the side is healthy at, at every one of 62 limits (both sides) | refused | `c8_holds_a_trim_leaves_the_other_party_no_worse_than_a_close_at_the_same_tick` |
| trim | A stale tick (within lateDaa), the true price anywhere from 7 to 14 when the claim lands | the other party keeps at least min(what it had, what a close at that tick pays it); the most a stale tick cost it over the grid: 2,052.6 KAS of a pair of 3,000, where the close costs more | the same test |
| trim | A price above the short's bound and above priceMax; a trim while the other side is bankrupt | within the same bound | the same test (attested prices to 13) |
| trim | A side that is liquidated at the tick trims | refused to any bound; with an older tick see C8-N1 | `c8_bounded_a_trim_delays_a_due_liquidation_...` |
| trim | One key on both sides: trims either side (paid to that key), another key's signature refused, not liquidated | holds | `c8_holds_a_pair_of_one_key_trims_and_is_not_liquidated` |
| pending | Anything but `finalize` spends a pending trim; a void trim paid; the trimmed pair written with another state | refused | the family's `trim_tests` (read; not repeated) |
| pending | A void trim as a shelter | restored by anyone at once | `c8_bounded_a_trim_delays_...` |
| after | Listings written before the trim: the long's taken at its ask (paid its worth from the NEW bound; an entrant at the trimmed bound that is not healthy at the ask is refused; below the ask refused); the short's taken at a price below the trimmed range (paid its bid's worth from its own bound by the entrant, 1,600 KAS where the pair holds 1,500: what the pair holds instead is refused) | a listing means after the trim what it meant before | `c8_holds_the_trimmed_pair_serves_its_listings_and_every_exit` |
| after | The other side's margin, a mutual close inside the new range, the lapse (the trimmed long 0; a lapse paying it refused) | holds | the same test |
| after | A renewal of a trimmed pair | by reading: `keepAlive` reads no bound (it wants the new lease a quarter of the term past the old) | |
| limits | The templates and pinned constants | `cargo test -p kpf-artifacts`: 2 passed | |
| tools | The free margin (`trim_bound_of`, `trim_take`): the nearest bound may close at the price, a sompi nearer is refused, the range rule kept; `trim_bound_for` frees at least what is asked and stays inside the limit | holds over both sides and 400 prices | `auditC8_builders::c8_holds_the_free_margin_is_the_trim_the_model_accepts_and_no_more` |
| tools | Ticks of another market or roster, or with a file name that says another DAA than was signed, fed to the runner | by reading: `tick_raw` filters by roster and market, and `Attestation::collect` checks the signatures for the DAA in the name; such files price nothing | |
| tools | Two trims for one need | by reading: a pending trim of the key stops further trims that round (`trim_pending`); a trim whose file the round has not seen yet is not counted, so a second round before the first trim's file is in the directory trims again (bounded by the free margin; the margin stays the key's) | |

## Not checked

- Every command through the binary: `c-trim`, `c-account`, `c-cross-run`, `seat-run`, `roster-roll --wait` were
  read, not run (the binary was held by a live run). `auditC8_cli.rs` is to be run with `--ignored` and its prints
  turned into assertions.
- The masses of the trim and its finalize, the pair's and the pending part's sizes and signature operations after
  the change: not measured by me (the family's command-line tests build them through `verify`).
- The walk (`auditC1_walk_tests`) was not extended to trims.
- C8-F1 end to end (a planted pair file and a runner round), and what a round costs in node queries.
- `seat-run` against a clock that stalls; a price source that fails or returns an old price (by reading: a failure
  is "NOT SIGNED" and the next tick is tried; an old price is signed as it is).

## Judgement

The trim itself is sound: it takes only what its own side has above maintenance at an attested price, never more
than a close at the same tick, its pending part pays only a valid claim and only through `finalize`, and the trimmed
pair is a pair. Nothing above Low is new in the contracts. What is new above Low is in the process that was built
on it: `c-cross-run` moves an owner's money on the word of a file, and a pair that is nobody's position can be
written to look like one that needs saving. That, and a runner that trims past the entry without keeping the lease,
are both closed by one question asked before margin moves: what does a lapse pay this side afterwards.
