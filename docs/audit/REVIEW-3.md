# KPF review 3

## Scope and method

Scope: `contracts/KpfPair.sil` and `contracts/KpfOpen.sil` after the review 2 fixes (DESIGN 4.5b: forfeiting
liquidation, fee capped at the maintenance slice, evidence fillable for the whole rest window), checked against
`docs/DESIGN.md` 3.1, 3.3, 3.4, 3.6, 4.3-4.6, 4.5a, 4.5b and `docs/THREAT_MODEL.md`; and the testnet tooling
`crates/kpf-protocol` and `crates/kpf-cli`. KOB's `KobAsk.sil`, `KobBid.sil` and `docs/spec/matcher.md` (2.3, 4.3, 5)
were read as the evidence side. Reviews 1 and 2 were read first and are not repeated.

Method: every entry was re-read with one question: which written or unwritten assumption does the review 2 fix rest
on ("the liquidatable party is the losing party", "a quote that rested is still a live quote", "the entrant chose its
bound"). Each candidate was built with the existing builders and run in the engine, every input executing its real
script (pairs, KpfOpen, KobAsk/KobBid, KCC20Ref). Tooling candidates were driven through the library and the binary
offline.

Proofs:
- `crates/kpf-tests/tests/audit3_tests.rs` (`cargo test -p kpf-tests --test audit3_tests`), 14 tests;
- `crates/kpf-protocol/tests/audit3_tooling.rs` (`-p kpf-protocol --test audit3_tooling`), 1 test;
- `crates/kpf-cli/tests/audit3_cli.rs` (`-p kpf-cli --test audit3_cli`), 4 tests (one Windows-only).

The full workspace passes with the new files (`CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2`). In every
`exploit_*` test the engine accepts the transaction.

## Findings

| # | Severity | Title |
|---|---|---|
| R3-F1 | Critical | A quote that rested and then soft-expired stays valid evidence forever: a healthy pair is liquidated (or closed at the whole range) on a dead quote |
| R3-F2 | High | When the trigger lies outside `[BL, BS]`, the side that holds the whole pair at the true market is "liquidatable" and forfeits everything to the bankrupt side |
| R3-F3 | Medium | Liquidation by transfer enters a resting order at exactly the loser's bankruptcy price, so the same quote already liquidates it; the tool does this by default |
| R3-F4 | Low | An order filled at its limit can be born liquidatable at the market and then forfeits its whole margin |
| R3-T2 | Medium | Token amounts are parsed and printed wrongly for a market whose `scale` is not a power of ten |
| R3-T3 | Low | A funding file written by `kpf utxos --out` loses the covenant flag, so covenant UTXOs become funding |
| R3-T5 | Low | On Windows, `keygen` reports private permissions it may not have set |
| R3-T4 | Informational | Object and KOB order files without `daa` are read as DAA 0 |

R3-T1 (the tool's default P7 price) is the tooling half of R3-F3 and is listed there.

### R3-F1 (Critical). Dead quotes are evidence forever

Affected: `KpfPair.evQuote`, so P5, P6 and P7 (`onEvidence`).

The review 2 fix requires an evidence order to have been fillable by conforming matchers until the end of its rest
window: `expiryDaa ≥ restEnd`, the IOC/FOK kill `≥ restEnd`, and `restEnd ≤ UTXO DAA + 90 days`. Nothing requires it to
be live when it is used. The unstated assumption is that a quote that rested for `R` is still a live, visible quote at
the moment of the fill. Once an order passes its soft expiry, KOB's conforming matchers stop filling it and the
reference indexer drops it from every book (matcher.md 2.3). So the arbitrage that DESIGN 3.3 and THREAT_MODEL A1
count on ("matchers cross a mispriced resting order in the next tick") can never reach it again. The covenants still
accept the fill. CLTV gives only a lower bound on `tx.daa`, so no check in the transaction can show how stale the quote
is. Only a refund keeper removes such an order, and with `refundTip = 0` (unchecked by KPF) no keeper is paid to.
`check_evidence` in the tool even reports "past its soft expiry ... use it now or not at all" (`kob.rs:717-722`).

Exploit. The attacker, as the short, rests its own IOC ask at 9.45 KAS while the market is at or below 9.45. Nobody
takes it, so it costs nothing. At DAA 1,600 the kill passes and the ask disappears from every book. Much later the
market is at 10, and a long opens at 10 with `BL = 9` (trigger 9.4737). The long is healthy, and it never was
liquidatable in its life: its UTXO is from DAA 5,000,000. The short fills its own dead ask and runs P6. The same works
with a GTD order (`expiryDaa = restEnd`) and with a GTC order past 90 days idle. It also works for P5 in mirror: the
long rests a bid at `BS = 12` while the market is above 12, lets it die, and months later closes on it at 10, taking
the whole range. The attacker can chain the victim's open (T1 against its own order, or as a direct taker) and the
liquidation in one block, so the victim gets no time to refund the dead order. An attacker that parks orders at many
prices while the market passes through them builds a library of free quotes. Any later long whose trigger is above
one of them (any long with 10 % margin after a 5.6 % rise) can then be wiped out.

Who loses: the healthy party, its whole margin. In the test the long loses 1,000 KAS, and the short takes it plus the
keeper fee. In the P5 variant the short, which is owed 2,000 KAS at the market, keeps only the 0.2 KAS carrier.

Tests: `exploit_stale_ask_liquidates_a_pair_opened_after_it_died` (IOC, GTD, GTC past 90 days),
`exploit_stale_bid_closes_the_long_at_the_whole_range` (GTD and IOC bids). The control
`holds_live_quote_is_the_control_of_r3_f1` is the same ask with its soft expiry ahead: the covenant accepts both and
cannot tell them apart.

Smallest fix: KPF cannot bound the fill time from above, so evidence must be an order that stays visible, and that
keepers remove, for as long as the covenants can still use it. In `evQuote`: accept only `tif = 0` (no kill);
require `expiryDaa ≥ UTXO DAA + 77,760,000`, so no GTD order dies earlier than the 90-day limit; and require
`refundTip ≥` a new market field `minRefundTip`, so KOB's keepers refund the order at its 90-day limit
(matcher.md 5: "in the first block that allows it"). The residual is a keeper-liveness assumption: a dead GTC quote
stays usable until a keeper removes it. Record that residual in DESIGN 3.3 and THREAT_MODEL A5. Have the KPF wallet
refund any dead order that could act on the user's pairs (anyone may refund), and stop the tool telling keepers to
use dead orders.

### R3-F2 (High). The winner forfeits when the trigger lies outside the range

Affected: `KpfPair.onEvidence` paths 6 and 7, with the `T` clamp.

DESIGN 4.5b assumes, without saying so, that the liquidatable party is the losing party. The trigger compares the
quote with the party's own bound only: long `a·(10⁴ − mmr) < BL·10⁴`, short `BS·10⁴ < b·(10⁴ + mmr)`. When the
pair's range is narrower than its maintenance band, the long's trigger `BL/(1 − mmr)` lies above `BS` (or the
short's `BS/(1 + mmr)` lies below `BL`). The long is then "liquidatable" at asks above `BS`, where the short is
bankrupt and the long is owed the whole pair. Before 4.5b, such a liquidation valued the long at `clamp(a) = BS` and
paid it everything. Now it pays 0, and the bankrupt short receives the pair less the fee. The code clamps `T` for the
fee cap ("the winner receives at least its payoff at the clamped T"), but the "winner" there is the bankrupt side.
`holds_winner_receipt_depends_on_the_quote_only_through_the_fee` (review 2) already accepts these transactions at
`mmrBps = 4999` without noticing.

Exploit. `mmrBps = 3000`, inside the documented bound `< 5000`, and the DESIGN 5 pair (`BL 9`, `BS 12`). The true market
is 12.5. An honest third-party ask at 12.5 satisfies `12.5·0.7 < 9`. The bankrupt short runs P6 and takes 3,000.2 KAS
less a 62.5 KAS fee. The long, owed 3,000 KAS at that price, gets 0. The mirror case (`mmrBps = 4999`, honest bid 8.5,
the bankrupt long takes the pair) also works. So does the default market (`mmrBps = 500`) for a high-leverage pair:
`BL 9.6`, `BS 10.0`, honest bid 9.55, and the long, bankrupt, takes the short's 400 KAS. The precondition is
`(BS − BL)·10⁴ < mmr·BS` for the long side, or `≲ mmr·BL` for the short side: for the DESIGN 5 pair `mmr > 25 %` or
`> 33 %`; at 5 % maintenance, pairs whose two margins together are below about 5 % of the price. `KpfOpen` creates
such pairs without objection.

Who loses: the side that is ahead at the true market, its whole payoff (3,000 KAS in the first test).

Tests: `exploit_profitable_long_forfeits_inside_the_maintenance_band`,
`exploit_profitable_short_forfeits_inside_the_maintenance_band`, `exploit_profitable_short_forfeits_on_a_narrow_pair`.
The boundary `holds_trigger_inside_the_range_protects_the_winner` is the DESIGN 5 pair at 5 %, where the same quotes are
refused.

Smallest fix: in paths 6 and 7, also require `a < BS` for a long loser and `b > BL` for a short loser, so a quote that
shows the counterparty bankrupt never liquidates. Better, have `KpfOpen.fill`/`take` refuse a pair or part whose
trigger falls outside `[BL, BS]` (`(BS − BL)·10⁴ ≥ mmrBps·BS` and the short's mirror), since such a pair has no
healthy state at any price. Record in DESIGN 4.5b that the forfeit rule needs the trigger inside the range.

### R3-F3 (Medium). P7 enters an order that the same quote liquidates

Affected: `KpfPair.onEvidence` path 7 (`handOver` with `nbd`), `KpfOpen.take`; tooling `p7_default_x`
(`crates/kpf-protocol/src/evidence.rs:738`), `kpf liquidate-transfer`.

No-downgrade bounds the entrant's new bound at `nbd ≤ BL` (long), and the entrant order checks only its own terms. The
executor fills the order at `x_open = BL + myRate ≤ limit`, which gives it exactly the loser's `BL`. The same evidence
quote that proved the loser liquidatable therefore already proves the entrant liquidatable. The pair's own `x` is a
separate argument, so the executor sets it to the ask and keeps the difference (600 KAS in the test), as in T2. In the
next transaction a second resting ask liquidates the entrant, and under 4.5b the entrant forfeits everything. All of
the order's terms hold (`x ≤ limit`, counter rate, `E ≥ x`), but it enters at a price that the same transaction proved
is 6 % above the market. Its whole margin, not the price difference, goes to the winner. These are exactly the orders
a liquidation meets: resting long orders whose limits were set before the fall. The reference tool chooses this `x` by
default. `p7_default_x` takes `min(limit, BL + myRate, …)`, and the summary prints no warning (R3-T1).

Who loses: the maker of the entrant `KpfOpen`, its margin `q(n, myRate)` (1,000 KAS for 1,000 tokens) plus its tip.

Tests: `exploit_p7_enters_a_kpfopen_that_the_same_quote_liquidates` (the P7 transaction, then the second liquidation),
`holds_p7_entrant_bound_never_above_the_losers`, and tooling
`exploit_default_p7_price_enters_the_order_already_liquidatable` (long and short).

Smallest fix: P7 knows the quote, so the pair should require that the entrant is not liquidatable at it: long
`p·(10⁴ − mmrBps) ≥ nbd·10⁴`, short `nbd·10⁴ ≥ p·(10⁴ + mmrBps)`. This does not touch the winner's floor. The tool should
pick `x` under that bound, or refuse.

### R3-F4 (Low). An order filled at its limit is born liquidatable

Affected: `KpfOpen.fill` and `take` (T1, T2), and the tooling commands `create-order`, `open-taker --rate` and
`transfer --rate`, which do not warn.

The executor chooses `x` up to the long's limit. The fixture long order (limit 10.5, rate 1, a 10 % margin) filled at
10.5 gets `BL = 9.5` and a trigger of 10.0. At a market of 10 (an honest ask at 9.99) it is liquidatable at once and
forfeits 1,000 KAS to the short order. Before 4.5b it kept 490 KAS. A spot bid filled at a limit 5 % above the market
loses 510. The order's terms hold. What changed is that 4.5b turns limit slippage into a total loss. A covenant cannot
see the market, but `KpfOpen` does not even refuse a fill at which its own side is liquidatable at its own fill price
(`myRate·10⁴ < mmrBps·x`).

Test: `exploit_order_filled_at_its_limit_forfeits_its_margin_at_once`.

Smallest fix: `KpfOpen.fill`/`take` require `myRate·10⁴ ≥ mmrBps·x`. The wallet should warn when the limit sits more
than `myRate` less maintenance away from the market, and state in the order ticket that a liquidation forfeits the
whole margin.

### R3-T2 (Medium). Amounts for a non-decimal scale

Affected: `kpf_cli::args::parse_amount` and `fmt_units` (`args.rs:101`, `:131`), and every `--size`, `--n` and
`--min-fill` and every printed size.

The number of decimal places is `round(log10(scale))`, but `Market::validate` accepts any scale in `1..=10⁹`, and the
scale comes from the market, pair or order file that someone else gives the user. With scale 400,000, `--n 0.5`
becomes 500,000 base units (1.25 tokens), and the summary prints "1.1". With scale 5, "0.5" is a whole token.

Who loses: the user, who commits margin and exposure for 2.5 times the size it typed and is shown a third number.

Test: `exploit_token_amounts_for_a_non_decimal_scale`.

Fix: refuse a scale that is not a power of ten in `Market::validate` (the CLI path), or parse with exact rational
arithmetic.

### R3-T3 (Low). The saved funding file loses the covenant flag

Affected: `kpf utxos --out` (`cmds.rs:801`, which writes `covenantId`) and `parse_utxos` (`files.rs:195-201`, whose
simple shape ignores it and the script).

A covenant-bound UTXO that the node path filters out is admitted as funding when it is read back from the tool's own
file. The effect is a transaction that spends a covenant-bound UTXO as plain funding, which ends that UTXO's lineage, or
that fails, for example a stray of a pair's id in that pair's transaction.

Test: `exploit_saved_funding_file_readmits_covenant_utxos`. Fix: read `covenantId` (and the script, when present) in
the simple shape, and refuse an entry without the script unless the user confirms.

### R3-T5 (Low). Windows key permissions

Affected: `keys::write_private` (`keys.rs:74-90`).

The secret is written with inherited ACLs. `icacls` then runs on a best-effort basis and its result is ignored, but
`keygen` always prints "mode 600 / current user". With a `USERNAME` that `icacls` cannot resolve (a domain account
needs `DOMAIN\user`), the file keeps every inherited entry. Test (Windows):
`exploit_keygen_claims_private_permissions_it_did_not_set`. Fix: create the file with a restrictive security
descriptor, or check the `icacls` status and fail (or warn) on error.

### R3-T4 (Informational). Missing `daa` is read as 0

`ObjFile::from_json` (`files.rs:57`) defaults `daa` to 0. With `--daa N` and no node, the `evidence` command computes
the rest window from DAA 0 and reports a fresh order as rested, and the local engine check passes. The node, which
reads the real UTXO DAA, refuses the transaction, so no funds are at risk. Test:
`exploit_object_file_without_daa_is_read_as_daa_zero`. Fix: make `daa` required, or report it as unknown.

## Concurrency (reasoning, no node)

Every entry spends the pair UTXO itself, so any two of them conflict, and exactly one is accepted. The table lists
the orderings in which accepting the later, conflicting transaction instead of the earlier one costs an honest party
more than the race documented in THREAT_MODEL B1, K2 and D4.

| Intended (earlier) | Accepted instead (later) | Extra cost |
|---|---|---|
| P2 add margin by a loser near its trigger | P6 / P7 by any keeper | Documented race (B1). Since 4.5b the loser loses its whole residual, not the slice. Magnitude raised by the fix, not a new race. |
| P5 close by the loser at a bid | P6 / P7 | Same as above: `q(n, b − BL)` becomes 0. Documented. |
| Owner's refund of a dead evidence order (R3-F1 defence) | T1/T2 open, then P6 on a dead quote, chained in one block | The victim has no window after its own open. Not a race the victim can win once its pair exists (R3-F1). |
| P1 amend (raise or remove a listing) | P3 / P4 on the old listing | None. The old listing was consented. |
| P3 to entrant A | P3 to entrant B, or P4 | None. The leaver is paid at least its listing in every case, and the stayer is protected by no-downgrade. |
| P6 by the winner | P6 by a third-party keeper | Fee only. The winner keeps at least its payoff at `T` (but see R3-F2, where the "winner" is the bankrupt side). |
| P6 | P7 | None at the quote. The winner holds a position instead of cash (B4). |
| P6 / P7 | P8 idle settle (`age ≥ idleDaa`) | Documented (K2). |
| P9 cooperative close | P6 | Documented. The loser forfeits instead of the agreed split. |
| P6 using evidence `E` | A third party buys `E`, or a keeper refunds it past soft expiry | The liquidation fails. Harmless to funds, it delays the winner (Q1 gap risk). |
| `KpfOpen.cancel` | `fill` / `take` (T1, T2, P7) | Documented for fills at the order's terms. With R3-F3/F4 the race now decides between a cancel and a total margin loss, not limit slippage. |
| `KpfOpen.refund` | `take` in P7 | As above. |
| Two parts of a split pair | (none) | They are separate UTXOs. `OpCovInputCount == 1` forbids co-spending, and no conflict arises. |

No ordering was found in which a conflicting acceptance creates an invalid state. Every transaction is checked
against the UTXO it spends.

## Unproven

- Mainnet replay through a lying node. Kaspa signatures do not commit to a network. `check_network` trusts the node's
  report, a key file without `network` loads (`keys.rs:96`), and a funding file has no network field. A user with a
  reused key who builds `create-order` (funding inputs only) against an attacker's node hands the attacker a
  transaction valid on any network that accepts version-1 transactions. Not built: it needs mainnet consensus support
  and a network.
- The node's field name for a UTXO's covenant id. `parse_utxos` reads `covenantId`. If a Toccata node reports it under
  another name, covenant UTXOs pass the node-path filter as in R3-T3. Not verified (no network).
- The cost of R3-F1 on a real book depends on whether keepers refund dead orders whose `refundTip` is 0. This is not
  measurable offline.
- Builders never set the lock time to an order's `activeFrom` (`build.rs:157, 187, 297`). An order with
  `activeFrom > 0` cannot be opened or taken through the tool, and local verification fails. This is functional, not a
  loss. Not built.
- Sub-dust winner payouts kept in a remainder and later paid to the loser by P8 (review 2), and two token templates
  under one id (review 2): not retried.

## Checked and found sound

- Who can make a healthy party liquidatable. A party's trigger depends only on its own bound. Only its owner moves
  that bound, and only outward (`addMargin`). Every hand-over keeps the stayer's bound (no-downgrade). A remainder
  keeps both bounds. The P4 continuation carries each stayer's own bound: the host checks `BS`, the donor checks `BL`.
  A pair is one UTXO spent once per transaction, so no same-transaction transfer or close-versus-close can change
  "the loser" of a liquidation.
- Partial liquidation in minimum pieces with `keeperFeeMin` far above the slice: each piece's fee is capped at its
  slice, and the winner gets at least its trigger payoff (`holds_minimum_pieces_never_charge_the_winner_beyond_the_slice`).
  At `mmrBps = 0` only a bankrupt party is liquidatable and the fee is 0 (`holds_mmr_zero_…`). `mmrBps = 4999`: see
  R3-F2.
- Evidence block. One evidence serving a P5 close and a P6 liquidation must cover both
  (`holds_one_evidence_for_a_close_and_a_liquidation_counts_both`). A loser cannot close and be liquidated in one
  transaction (one UTXO). A partial-fill continuation and a fresh custody each restart the rest, so their own DAA
  counts (`holds_fresh_continuation_or_custody_restarts_the_rest`). An expiry or kill exactly at the end of the window
  is accepted as specified (R3-F1 concerns what happens after it). A block member that is a KpfOpen, KOB order or
  token input cannot pass `blockN`.
- The loser on the way out. In P6 the loser is owed 0, so with `n = s` the carrier and excess go to the winner. The
  winner's payout is never sub-dust with `n = s` (it is at least the carrier when the carrier is ≥ 0.02 KAS), and with
  `n < s` a sub-dust payout stays in the remainder. Output slots stay injective. A loser that runs P7 on itself at
  `x ≤ BL` with `nbd = BL` pays 0 and leaves the pair as it was: the winner's payoff does not change at any price.
- `KpfOpen` as the P7 entrant: limit, own rate, counter rate, `E ≥ x` (long) and the claim by tag are all checked
  exactly as in T2. Its payment is its own margin. The winner's cash comes from the old part's range, and the gap
  between the pair's `x` and the order's `x` is the executor's, as in T2. The order gets its terms (but see R3-F3).
- Arithmetic of the trigger, the clamped `T`, the fee and the block threshold within the bounds of `checkPair`.
- Tooling (verified by reading): signatures are SIGHASH_ALL over every output. Pair, order and KOB states are
  re-derived from the redeem script under the pinned templates. `check_evidence` applies the same rules as `evQuote`,
  stricter on the template. The trigger, slice and fee formulas match the contract. `parse_address` refuses `kaspa:`.
  `keygen` refuses paths inside a git tree and existing files. No secret appears in transaction or object files.

## Not covered

GhostDAG races on a node (X7), script budgets of new shapes, the KCC-20 program itself, the KOB router in mixed
batches, and the economic size of R3-F1 and R3-F3 on a real book (X9).

## Status after fixes

The fixes implement DESIGN 4.5c. Every `exploit_*` test of this review is now a `holds_*` test (refused, or valid
only within the residual 4.5c documents, as each test's comment says). Proofs: `audit3_tests` (25 engine tests),
`audit3_tooling` (2), `audit3_cli` (4, one Windows-only), the extended X6 walk, and the updated tests listed below. The
full workspace passes (`CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2`). The pinned artifacts and both golden
vector sets were regenerated.

| # | Status | Shown by |
|---|---|---|
| R3-F1 | Fixed. `KpfPair` state field `since`; `S = since` or the UTXO's DAA; `exposed' = max(exposed, S)` in the three fillability conditions and the lock-time proof; `since = 0` on the part that changes hands (T1, T2, T7, T3's re-paired output), `since = S` on every other pair output (P1, P2 and every remainder: P3, P4 host and donor, P4', P5, P6, P7); transfer and close-versus-close need `tx.daa ≥ S + 4R`, P7 does not. Residual as 4.5c: a quote that rested on the clock and died later still counts (look-back since `S`, barrier semantics); a touch that starts resting in the last `R` before `S + 4R` and dies within `R` of a self-transfer is dodged | `holds_stale_ask_cannot_liquidate_a_pair_opened_after_it_died`, `holds_stale_bid_cannot_close_the_long_at_the_whole_range`, `holds_evidence_resting_before_the_clock_counts_only_from_s`, `holds_evidence_that_died_before_it_rested_on_the_clock_is_refused`, `holds_the_clock_is_kept_by_every_output_that_changes_no_hands`, `holds_the_clock_restarts_on_each_hand_over`, `holds_self_transfer_before_s_plus_4r_is_refused_and_keepers_keep_the_window`, X6 |
| R3-F2 | Fixed. `KpfOpen.fill` requires `BL·(10⁴ + mmr) < BS·(10⁴ − mmr)`, P6 and P7 require it, the clamp of `T` is gone; add margin, transfer and re-pairing only move bounds outward, so they keep it (tested from the narrowest admissible pair) | `holds_profitable_side_never_forfeits_in_a_narrow_pair`, `holds_range_condition_at_its_boundary`, `holds_narrow_pairs_are_refused_at_creation`, `holds_add_margin_transfer_and_re_pairing_keep_the_range` |
| R3-F3 | Fixed. The P7 entrant must be healthy at the evidence quote (deviation 1 below); `p7_default_x` picks the highest healthy `x` or refuses | `holds_p7_entrant_is_healthy_at_the_quote`, `holds_entrant_unhealthy_by_one_tick_is_refused`, `holds_default_p7_price_*` |
| R3-F4 | Fixed as 4.5c: every entrant is healthy at its entry price (both sides at T1, the order's own side in `take`, the entrant in P3 and P7). Residual: a fill at the limit above the market is liquidated once a live quote below its trigger rests `R` on the pair's clock; `create-order` prints the forfeiture and the trigger at the limit | `holds_order_filled_at_its_limit_is_healthy_at_entry` |
| R3-T1 | Fixed with F3 | `holds_default_p7_price_keeps_the_entrant_healthy_at_the_quote` |
| R3-T2 | Fixed: exact parsing and printing for any scale; an inexact decimal is refused | `holds_token_amounts_are_exact_for_a_non_decimal_scale` |
| R3-T3 | Fixed: the simple funding shape keeps `covenantId` and the script; `kpf utxos --out` writes both | `holds_saved_funding_file_keeps_the_covenant_flag` |
| R3-T4 | Fixed: a missing `daa` is unknown and refused wherever it matters; new object files are written with `daa: null` | `holds_object_file_without_daa_is_refused` |
| R3-T5 | Fixed: `keygen` prints what it did, a WARNING when `icacls` fails | `holds_keygen_reports_the_permissions_it_set` |

Tooling. The evidence decoder and `evidence`, `close`, `liquidate`, `liquidate-transfer` apply the clock rule per pair,
show the pair's clock start, `since` and the DAA from which a hand-over is allowed, and refuse an order that is dead at
the DAA the transaction would land (`--allow-dead-evidence` for sweeping tests only). The builders set the lock time
each entry needs: the rest on every pair's clock, `S + 4R` for transfer and close-versus-close, and each order's
`activeFrom` (open, open-taker, transfer, P7). `inspect` shows the clock and warns on a narrow range.

`// DESIGN:` deviations (each marked in the source):
1. `KpfPair.onEvidence`: the P7 entrant's health is checked at the evidence quote, not only at `x`. A check at `x` alone
   lets the executor pick `x` above the trigger while the same quote proves the entrant liquidatable (F3 again); the
   check at the quote implies the one at `x`.
2. `KpfOpen.fill` checks the health of both sides of the pair it creates, so a pair opened against a signing taker is
   healthy on both sides too.
3. `KpfOpen.take` also requires the range condition of the part it enters (binds only a look-alike part).
4. `KpfOpen.checkMarket` requires `4·minRestDaa < 5·10¹¹`: the hand-over limit is a lock time, and without the bound a
   market could create pairs that can never be transferred (`Market::validate` mirrors it).
5. `KpfPair.clockStart` refuses a negative clock (a forged `since`, or a mempool spend of an unaccepted parent, whose
   DAA score the engine reads as -1), so a mempool and consensus never disagree about a written clock.

Attacks on the fix (all recorded as `holds_*`; none found beyond what 4.5c documents):
- the `since = 0` sentinel: a UTXO of DAA 0 writes `since = 0` and restarts its clock at the next output; only the
  genesis block has DAA 0, so this exists in the engine only. An unknown DAA (-1) is refused by every clock-dependent
  entry (`holds_since_sentinel_against_unknown_and_zero_daa`);
- a forged `since` in the future freezes evidence and hand-overs of the forger's own look-alike, which keeps amend, idle
  settle and cooperative close; one in the far past makes stale quotes count only against that look-alike, never for a
  part an order takes (`holds_forged_future_since_freezes_evidence_and_hand_overs`,
  `holds_forged_past_since_binds_only_the_look_alike`);
- one lock time for several lower bounds: each covenant checks its own, so the transaction needs their maximum
  (`holds_one_lock_time_must_cover_every_bound`);
- split pieces with different clocks re-paired, and close-versus-close of pairs with different clocks: each pair's limit
  binds, P3 restarts and cannot take over either clock, remainders keep their own
  (`holds_split_pieces_with_different_clocks_re_paired`, `holds_close_versus_close_with_two_different_clocks`);
- the rate limit against the idle timeout (independent; amend resets the age, never the clock), and P7 of a part handed
  over moments ago: it needs `R` on the part's clock and is not rate-limited, and the winner's part restarts too, so
  when both sides were touched since `S` whichever liquidation lands first decides, a race of the kind of B1
  (`holds_rate_limit_counts_from_the_clock_and_leaves_the_idle_timeout_alone`,
  `holds_liquidation_by_transfer_of_a_part_handed_over_moments_ago`).

Existing tests whose premise changed: `audit2_tests` (P7 entrant prices now healthy at the quote; pairs at 20 % and
49.99 % maintenance are narrow, so they are refused or rebuilt wider; the close-donor half waits for `S + 4R`),
`audit_tag_tests` (high-maintenance edge orders post wider rates; a market with `4·minRestDaa` at the threshold is
refused; a malformed `minRestDaa` now also makes the pair's transfer refuse), `kpf_open_tests`
(`t1_counter_rate_below_minimum_is_refused` uses rates above the maintenance margin), `kpf_transfer_tests` (the dust
entrant posts the maintenance margin; hand-built drafts carry the lock time), `kpf_close_tests` (lock time),
`kpf_layout_tests` (state 438 bytes, since window), `kpf_x6_conservation_tests` (healthy random orders and a DAA
model), the library's `builders`, `evidence` (default P7 price 9.93 / 10.075; a dead order is refused, not noted),
`golden` and `pinned`, and the CLI `lifecycle` (waits for lock times; the P7 entrant is no longer liquidatable by the
same ask).

Sizes and compute. `KpfPair` 11,567 → 12,115 bytes (state 429 → 438), `KpfOpen` 2,634 → 2,809. Script units per path
(after review 2 → now): P1 277,252 → 285,946; P2 265,506 → 273,652; P3 263,254 → 276,290; P4 host 251,468 → 263,921,
donor 240,046 → 251,952, mutual 142,103 → 149,153; P5 long 249,422 → 255,928, short 279,672 → 286,178; P6 long
179,339 → 185,870, short 149,340 → 155,871; P7 long 312,627 → 325,685, short 282,552 → 295,610; P8 23,971 → 25,067; P9
224,227 → 225,323; `KpfOpen` fill 146,447 / 146,593, take 147,110, cancel 106,031, refund 5,960; largest stack 190 of
244. Eight liquidations on one evidence: 1,651,150 script units, compute mass 130,644 / 500,000, storage mass
280,256 / 500,000 (1 KAS fillers in the loser slots), 104,934 / 250,000 bytes.
