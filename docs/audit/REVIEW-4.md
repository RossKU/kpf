# KPF review 4

## Scope and method

Scope: the two-step evidence mechanism of DESIGN 4.5d as built in commit `6a481d1`: `contracts/KpfPair.sil`
(`propose` and every entry that touches a pair while a part is pending), the new `contracts/KpfPending.sil`
(`challenge`, `cure`, `finalize`), the `checkMarket` changes of `contracts/KpfOpen.sil`, and the tooling that drives
them (`crates/kpf-protocol/src/pending.rs`, `evidence.rs`, `kob.rs`; `crates/kpf-cli/src/evcmds.rs`: `watch`,
`evidence --pending`, `challenge`, `cure`, `finalize`, `propose-*`). Checked against DESIGN 3.1, 3.4, 4.2-4.6 and
4.5a-4.5d, `DESIGN-TWO-STEP.md` and `THREAT_MODEL.md`. Reviews 1-3 and the recorded exploit
`exploit_false_challenge_by_the_loser_on_a_thin_book` are not repeated.

Method: each return state of a pending part (restore, cure, forfeit) was followed one transaction further than the
existing tests go, asking what every party can do to the pair it lands in; the tooling's verdicts were compared with
the contract's conditions one by one. Candidates were built with the existing builders and run in the engine, every
input executing its real script (pairs, pending parts, KobAsk/KobBid, KCC20Ref).

Proofs (all pass; in every `exploit_*` test the engine accepts):
- `crates/kpf-tests/tests/audit4_tests.rs` (`cargo test -p kpf-tests --test audit4_tests`), 7 tests;
- `crates/kpf-protocol/tests/audit4_tooling.rs` (`-p kpf-protocol --test audit4_tooling`), 1 test;
- `crates/kpf-cli/tests/audit4_cli.rs` (`-p kpf-cli --test audit4_cli`), 1 test.

The full workspace passes with the new files (`CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2`).

## Findings

| # | Severity | Title |
|---|---|---|
| R4-F1 | High | The forfeited part is liquidatable at birth: anyone drains the winner's forfeited part in keeper fees, round after round |
| R4-F2 | Medium | `watch` and `evidence --pending` report a counter order as refuting without checking that it covers `k_c · n` |
| R4-F3 | Low | A cure is reopened at once by any lower quote that rested on the old clock, and the added margin is then forfeited |
| R4-F4 | Low | `cure` builds and signs a cure of a look-alike pending part whose value is far below its claim; its summary line understates the payment |
| R4-F5 | Informational | `W = R` is admitted, which leaves a defender no slack between the earliest challenge and the earliest finalize |

No Critical finding.

### R4-F1 (High). The forfeited part is liquidatable at birth

Affected: `KpfPair.propose` (kind 2) on a pair written by `KpfPending.finalize` (kind 2), i.e. the `forfeitHash` state.

Exploit. Forfeit and keep writes a pair whose two keys are both the winner's, `since = 0`, and the loser's bound moved
inward by the fee (`fb > BL` for a long loser). The quote that liquidated the old long therefore still liquidates the
new pair's long (`p·(10⁴ − m) < BL < fb`), and the range condition `fb·(10⁴ + m) < BS·(10⁴ − m)` still holds. The new
clock `S′` restarts at the forfeited part, but a still-live ask that rested before `S′` counts from `S′`, so from
`S′ + R` anyone (here the former loser, with nothing left at stake) proposes the liquidation of the forfeited part's long
with its own payout key; anyone finalizes it after `W`. The result is a new forfeited part, again both sides the
winner's, with the fee taken out of it and paid to the proposer with its bond. Nothing moves between the "long" and the
"short" (both are the winner): the fee is a pure transfer from the winner to the proposer. It repeats every `R + W`
while the winner is offline, until the range condition fails. On the 1,000-token example with the ask at 9.40 it runs
39 rounds and takes 1,833 KAS of the 2,953.2 KAS forfeited part (62 %). The proposer's cost per round is two
transactions and a fill of `k·n` from an ask that can be its own (zero net), with the bond returned.

Who loses: the winner of an honest liquidation, for not acting within `R` of a finalize that anyone can trigger at any
time. THREAT_MODEL B4 states that the winner "carries no price exposure meanwhile" and DESIGN 4.5d expects it to hold
the part until it closes or sells it; selling waits `S′ + 4R`, so the honest path (list, then transfer) is exactly the
window in which it is drained. The winner can only stop each round by a cure (adding margin to its own pair) or by a
cooperative close.

Tests: `exploit_forfeited_part_is_liquidatable_at_birth` (three rounds, exact fee each),
`exploit_forfeited_part_is_drained_until_the_range_condition` (39 rounds to the range condition).

Smallest fix: in `propose`, kind 2, require `longKey != shortKey` (a pair whose two sides have one owner has nobody to be
owed a liquidation). A later sale of one side restores two keys, and with them the normal rules. In the tooling,
`finalize` (kind 2) should tell the winner to cooperative-close or cure promptly.

### R4-F2 (Medium). `watch` says "REFUTES IT" for an order that cannot cover the part

Affected: `kpf watch` (`evcmds.rs`, the `refuters` check), `kpf evidence --order … --pending …` (`counter_evidence`).

Exploit. Both commands call an order a refuter when it is of the opposite kind, contradicts the claim by `g`, and has no
problem and is not dead on the proposal's clock `P`. Neither compares the order's capacity with what
`KpfPending.challenge` requires, `n_ev ≥ ⌈kcBps·n/10⁴⌉` (the `covers` figure of `check_evidence_on` is computed with
`kBps` and is not tested at all). A stale liquidation of 1,000 tokens and a bid at 9.80 that can buy about 100 tokens:
`watch` prints "REFUTES IT" and the `kpf challenge` command line to run, and `evidence --pending` exits 0 with "REFUTES the
claim now"; the challenge builder refuses the same order ("the bid can buy … fewer than the … the evidence must
fill"), as does the engine (`holds_counter_fill_must_cover_kc_n`).

Who loses: a defender relying on `watch` during a 60-second window: it learns only at `challenge` time that it has no
defence, and the part is forfeited at finalize if no larger order or a cure is found in time.

Test: `exploit_watch_reports_a_refuting_order_that_cannot_cover_the_part`.

Smallest fix: in both commands, require `ev.capacity() >= fill_for(kc_bps, n)` (and, for a block, the sum) before
saying "refutes"; report `covers` with `kcBps` for counter-evidence.

### R4-F3 (Low). A cure is reopened by the next quote

Affected: `KpfPending.cure`, `KpfPair.propose` on the cured state.

Exploit. The cure moves the loser's bound exactly to the bound at which the proposed quote no longer triggers, and the
cured part keeps the clock `S`. An ask one sompi lower that rested on `S` is evidence against the cured part at once:
a proposal with lock time equal to the cure's own DAA is accepted. The loser cannot cure further in one step (more
margin is a separate add margin, which races the next proposal; the cured output can be spent by either while still in
the mempool), and a second claim finalized unanswered forfeits the margin just added. The proposer gets its bond back
at every cure (no cure fee), so a ladder of the proposer's own asks costs it only fees and self-fills.

Who loses: a loser that cures (instead of adding margin first) and then misses one window loses the cure margin
(70 KAS on the example) with the rest; otherwise each rung costs it a cure transaction. The loser was liquidatable at
the lower quote too, so this is griefing and a trap in the cure's semantics, not a false forfeiture.

Test: `exploit_cure_is_reopened_by_a_lower_rested_ask`; `holds_cure_bound_is_tight` shows the bound itself is exact.

Smallest fix: let `cure` take the new bound as an argument, any bound at or beyond `cureBound` (long: `≤`, short: `≥`,
within `[0, priceMax]`), with the added margin and the cured hash computed for it, so one transaction cures with
headroom. (This needs the cured hash to be computed in `KpfPending`, or a second precomputed "cure to …" state.)

### R4-F4 (Low). `cure` on a look-alike pending part

Affected: `kpf_protocol::evidence::cure`, `kpf cure` (`cure_cmd`).

Exploit. A look-alike `KpfPending` (the real template under a genesis covenant id, so its script runs the genuine
`cure`) carries the victim's own pair copied into a liquidation claim of its long, an attacker's proposer key and a
value of 1 KAS. Its hashes match its state, so `cure_pair()` is `Some` and the builder raises nothing: it does not
compare the part's value with the claim it describes (every genuine part holds at least `q(n, BS − BL) + refund`) nor its
covenant id with a pair of the user. The long's funder pays the attacker the `refund` and funds the whole range of the
cured pair (about 3,160 KAS in total) while the command's line says the cure "adds 70 KAS of margin". The cured pair is a
real `KpfPair` of the long and its short, so the short receives the margin of a range the long paid for in full. The
transaction report's "who pays and receives" table does show the funder's total, which is why this is Low. The same line
also omits the carrier `C` the curer funds for a genuine part with `n < s`.

Who loses: a long that runs `kpf cure` on a file handed to it as "your pair is being liquidated": the refund outright and
the short's side of the range.

Test: `exploit_cure_of_an_underfunded_look_alike_pending`.

Smallest fix: in `cure` (and `challenge`, `finalize`), refuse a part whose value is below `q(n, BS − BL) + refund`
(`+ C` already inside `refund` for `n < s`), and when the user's pair file is given, require the same covenant id; print
the funder's total in the summary line.

### R4-F5 (Informational). `W = R` leaves no slack

Affected: `KpfOpen.checkMarket` (`challengeDaa >= minRestDaa`).

With `W = R`, counter-evidence placed at `P` can first be used at `P + R` (CLTV), and finalize is first valid at age `R`
(CSV): the same DAA. A defender that only learns of the proposal at `P` has zero slack for detection and relay; the
bound documents `W ≥ R` as the safety condition (DESIGN 4.5d deviation 7, THREAT_MODEL B3 "keeps refutation possible"),
but at equality refutation is a race the defender cannot plan for. The testnet default (`R` 50, `W` 600) is fine.

Test: `exploit_window_equal_to_rest_leaves_no_slack`.

Smallest fix: require `W ≥ 2R` (or `W ≥ R + a stated detection margin`) in `checkMarket`.

## Unproven

- **A listing resurrected on restore and filled in the same block.** A pending part cannot be amended, so a listing the
  owner has withdrawn from the remainder is still live on the restored part; a transfer can spend the restored output
  while the challenge is still in the mempool (`since` is non-zero, so no UTXO DAA is read). An attacker holding a
  refutable claim could package propose, self-challenge and the transfer. Not built: a liquidation proposal needs a
  quote that really triggers on `S`, and then the attacker gains more by finalizing.
- **A market whose bond floor makes every proposal unfundable.** `checkMarket` admits `bondMin` up to 2.9·10¹⁸, so a
  market can be created on which no close or liquidation can ever be proposed (P5 and P6 dead, against the rule of
  4.5a that no order creates a pair with a dead path). Both makers see the market, so it is consented; not tested.
- **Front-running of keepers and challengers.** A proposal's payout key `pk` and a challenge's bond destination are not
  signed by anyone, and an ask fill needs no buyer signature, so a relay observer can copy a pending proposal or
  challenge with its own key. Generic transaction-copying; it moves the keeper's reward, not the parties' funds. Not
  built.
- **CLTV/CSV compilation.** The several lower bounds on the lock time and the CSV of `finalize` were checked through
  their behaviour in the engine (`holds_one_lock_time_covers_every_bound_of_the_block`, R4-F5), not by reading the
  compiler's opcode output.

## Checked and found sound

- Covenant id shared by the pair, its remainder and its pending part: the single-input guard holds on every pending
  and pair entry; two pending parts of one lineage, or a part and its remainder, are never co-spent.
- The precomputed return states: restore, cure and forfeit are hashed by the pair from its own script (keys, bounds,
  entry, listings, size, tags 0, clock), so the proposer cannot choose them; the tooling rebuilds and checks them.
- Bond and refund: funded from outside the pair in both the `n = s` and `n < s` shapes; on challenge it ends with the
  challenger, on cure and finalize with the proposer; never both, never neither (existing X6 plus this review's tests).
- Contradiction rows at equality and one tick either side for all four claims (`holds_contradiction_rows_at_the_tick`);
  counter-evidence coverage with `kcBps` (`holds_counter_fill_must_cover_kc_n`); counter-evidence counted from `P`; the
  proposal's own evidence continuation can never be counter-evidence (it is of the same kind as the claim's evidence).
- Cure: only the loser signs, kind 2 only, refused without a cure hash; the cured bound is exact and the cured value
  keeps the part's value plus the moved margin (`holds_cure_bound_is_tight`).
- Finalize: no key, no evidence, CSV age `W` on the pending UTXO; payouts for close and forfeit at the documented slots;
  the arithmetic stays inside 63 bits for the documented bounds (bond and value sums, `c·(10⁴ ± g)`, cure bounds).
- State layout: the 621-byte pending state matches `KpfPair.propose`'s byte assembly field for field (market block,
  ten claim integers, `[289..418)` of the pair, clock, proposer, three hashes).
- Note (wording, not counted): `print_proposal` and `challenge_cmd` (`evcmds.rs` lines 277 and 383) still say a refuted
  part "returns with a fresh clock"; since commit `6a481d1` the restore keeps `S`. A defender reading it may think the
  old evidence no longer counts after a challenge.

## Not covered

`KpfOpen` beyond the `checkMarket` diff; the `propose-close` / `propose-liquidate` summaries against their built
transactions; close-versus-close (P4) with a pair whose sibling is pending; the compiled artifacts against the sources
(relied on the pinned-artifact and layout tests); relay and mempool policy.
