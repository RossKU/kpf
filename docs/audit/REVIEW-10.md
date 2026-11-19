# KPF review 10

## Scope and method

Target: the oracle-market family at commit `c1b0b1d` (the fix of review 9): `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil` and their pinned artifacts, the builders and codecs they are used
through (`oracle.rs`, `oracle_build.rs`, `bond.rs`, `judge.rs`) and `ocmds.rs` where a command leads a user into a loss;
read against `DESIGN-ORACLE-IMPL.md` 26 and 27, `DESIGN-ORACLE-2.md`, reviews 6 to 9 and the two live runs. Every
entry point was read for what it constrains and what it leaves to the builder; every lead was built as a transaction
and run in the script engine with the covenant context (`common::run`, the templates compiled from source), and the
live runs were rebuilt with the tool's own builders on the pinned artifacts and checked with `verify::verify`.

New file only: `crates/kpf-tests/tests/audit10_tests.rs` (16 tests: 5 `r10_exploit_`, 9 `r10_holds_`, 2 `r10_bounded_`;
the harness pieces and the seeded walk of `audit9_tests.rs` are copied into it, the walk with one new hostile step) and
this report. Nothing in `contracts/`, artifacts, `vendor/`, protocol or CLI code was changed. Run:
`CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test audit10_tests`:
`test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 24.21s`.

Numbers use the fixture market of `oraclekit` (term 800, step 100, tail 200, idle 1,000, W 200, quiet 20, mmr 5 %,
g 2 %, keeper fee 0.5 %, carrier 0.2 KAS, scale 10^6, minimum size 1 unit; the fixture pair 1,000 units at 10, bounds
9 and 12, cover rate 6, expiry 1,200, opened at 300) unless the testnet defaults are named (`OMarket::testnet`: scale
10^8, minimum size 0.01 unit, carrier 0.2 KAS, term 28 days, step 3.5 days, tail 3.25 days, idle 7 days, W 6 hours).

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| R10-F1 | Critical | Every take of a long order costs it one carrier and its tip, and a part whose margin is below the 0.02 KAS dust floor is lost whole at the lapse settle; 27.4's residual of a take (or fill) "into the past" is per part, not per order. With the command's defaults (`--min-fill` = the market's minimum size, escrow = a full fill + 1 KAS) and `--any-slot`, an outsider with a genesis slot takes a resting long order in minimum parts with no pair and takes its whole escrow | `r10_exploit_small_takes_without_a_pair_drain_a_resting_long_orders_whole_escrow`, `r10_exploit_each_take_without_a_pair_costs_a_long_order_another_carrier`, `r10_exploit_the_seeded_walk_with_small_takes_finds_losses_beyond_the_documented_bound` |
| R10-F2 | Medium | A valid liquidation that is still pending at the expiry blocks the renewal (a pending part cannot be renewed); if the loser's cure lands after the expiry and before the finalize, the cured pair was written after its expiry and since review 9 is never renewed, so it lapses and settles at its entry: the loser gets its whole margin back, the winner loses the liquidation (954 KAS on the fixture pair). Review 9's table (27.3 row 9) marks the cure safe for exactly this reason | `r10_exploit_a_cure_after_the_expiry_turns_a_valid_liquidation_into_a_lapse_at_the_entry` |
| R10-N1 | Note | On markets with `tail + idleDaa <= term` (the testnet defaults) an underwriter that renews only after each lapse settle has become due resumes the cover every term, and its continuation stays renewable: at every due date it chooses between letting a side out at the entry and keeping both in, at the agreed premium (no premium beyond the on-time schedule). Documented in 27.4; measured here over five terms | `r10_bounded_with_tail_plus_idle_below_the_term_a_late_underwriter_resumes_the_cover_every_term` |
| R10-N2 | Note | `oracle-watch` tells a party after the expiry that "the underwriter may still renew" without checking whether the pair UTXO was accepted after the expiry (`OPair::unrenewable_why`), and nothing tells the winner of a pending liquidation to finalize before the expiry (R10-F2) | by reading (`ocmds.rs` 1166-1175) |

No High finding.

### R10-F1 (Critical). A long order taken in small parts loses a carrier per part, and a sub-dust part whole

**Affected:** `KpfOpenO.take` (and `fill`) with `escrow(n, rate, left)` charging `quoteOf(n, rate) + carrier` per
part; `KpfOpenO.amount` (`n >= minFill || n == sizeLeft`, the owner's `minFill` the only floor); `KpfPairO.lapseSettle`
(a long payout below `PLAIN_DUST` goes to the short); `kpf oracle-order` (`min_fill` defaults to `m.p.min_size`,
`ocmds.rs` 570; escrow defaults to `fill_spend + 1 KAS`, 577); `order_risk_lines` and DESIGN-ORACLE-IMPL 27.4 ("A take
into the past costs the order its tip and the part's carrier", "Bound per order: `q(n, tip + premMax / 2) + carrier`"),
26.2 and review 8 note 1 ("the order gets at least its margin back at the lapse settle").

**The rule that is missing.** Nothing relates the number of parts an order is taken in to the carrier each part
costs it, nor a part's margin to the dust floor of the settle that returns it. Per part, a long order pays
`q(n, myRate + tip) + carrier`; the part holds `q(n, bs - bl) + carrier`; the tip goes to the builder; at the lapse
settle the long is owed `q(n, e - bl)` (at least its margin, `e >= x`) and the short the rest, carrier included. So:

- above the dust floor each part returns the margin and keeps one carrier and the tip from the order (the short, the
  attacker, gets the carrier at the lapse settle; its change gets the tip at the take);
- below it (`q(n, e - bl) < 0.02 KAS`, i.e. `n < 0.02 KAS / myRate`: 0.02 unit at a rate of 1 KAS) `lapseSettle` pays
  the long 0 and the short the whole part, so the order loses its margin as well.

A take without a pair binds only the order, so its counterparty and the slot are the attacker's; with `uwAccept = 0`
the slot may be a genesis (any state, ring position 0, its expiries `step .. 12 step` usable whatever the present), so
each part's expiry lies in the past and its lapse settle is due at once. A short order is not exposed this way (the
carrier comes back with its side; it loses only its tip per part).

**The attack transaction (`r10_exploit_small_takes_...`, testnet defaults).** The victim is the order live run 2
wrote for its long, as `kpf oracle-order --any-slot` writes it today: 2 units, limit 10.50, rate 1 KAS, no tip,
`--prem-max` 0, `min-fill` 0.01 unit (the default), escrow 3.2 KAS (the default). The attacker holds a slot written by
a genesis (owner U, ring position 0) and a second key A. Each round, at DAA 20,000,000, two transactions:

1. take, signed by U (the bind) and A (its funding):
   - in 0: the order, `take(partOut = 1, n = 1,000,000 (0.01 unit), x = 10, e = 10, bound = 10.50, counterKey = A,
     pm = 0, 0, 0, 0, u = 1, k = 0, xe = 3,024,000, coverRate = 1.50, premium = 0, ...)`;
   - in 1: the slot, `bind(t = 20,000,000, x = 3,024,000, d = 0, (OUT_REF + 1, 0.015 KAS), ...)`;
   - in 2: A's P2PK funding (it authors the part's genesis);
   - out 0: the order's continuation (0.21 KAS less, `sizeLeft` 0.01 unit less); out 1: the part (long = the victim,
     bound 9; short = A, bound 10.50; entry 10; binding (the genesis slot, 1.50, 3,024,000, 0)), 0.215 KAS, A adding
     0.005 KAS; out 2: the slot; out 3: A's change;
2. lapse settle (due: 3,024,000 + 2,808,000 + 6,048,000 = 11,880,000 <= now), built by A: in 0 the part, in 1 A's
   funding; out 0 A's own output (the contract owes the long nothing: `q(0.01, 10 - 9)` = 0.01 KAS < 0.02 KAS), out 2
   the short's payout, 0.215 KAS to A.

Numbers: 15 rounds; the order pays 0.21 KAS a round and gets nothing back: **3.15 KAS of its 3.2 KAS escrow taken**
(the rest no round can use); A puts in 0.075 KAS and takes 3.225 KAS. 27.4's bound for the same order: 0.2 KAS. Each
transaction's storage mass stays below 52,000 grams, so both are minable. Fees, from the metrics of 27.7 (open 0.0936
KAS with two orders, lapse settle about 0.03 KAS): roughly 0.09 KAS a round against 0.21 KAS taken, so the attack
pays for itself (a miner pays no fee at all). The slot's owner rate limit (`rateDaa`, 60 s) is no obstacle: every
part can use another genesis slot, and `take` does not read its input's DAA score, so the takes can be chained on
unaccepted parents in one block, before any cancel of the owner lands.

Above the dust floor (`r10_exploit_each_take_..., fixture`): ten 1-unit parts of the fixture order with the default
`min-fill` and a tip of 0.01 KAS each lapse-settle with the margin back and cost the order 0.21 KAS each, 2.1 KAS for
ten units, seven times 27.4's bound for those units (their tip and one carrier, 0.3 KAS). In general a long order
taken in parts of `n` loses `q(n, tip) + carrier` per part (plus the margin when `q(n, myRate) < 0.02 KAS`) until its
escrow is gone; at the dust-floor size that is all of it whatever the order's size (a 2,000-unit order at rate 1
needs about 9,100 parts of 0.019 unit). The walk (`r10_exploit_the_seeded_walk_...`, 8 seeds x 200 steps with every
other entry point on the same clock) finds 34 shredded orders, 16.4 KAS lost beyond the documented bound, one extra
carrier per extra part in every case.

**By reading, the same through `fill`.** A fill of the order alone (the counterparty funded by P2PK, as review 9
noted) charges the long `q(n, myRate + tip + half) + carrier` and puts the carrier into the pair, which pays it to the
short at the lapse; only the first fill of an aged order can lie in the past (the continuation's DAA restarts the
`xe >= own DAA + term` bound), so later parts lapse a term + tail + idle later, at the same cost per part plus the
premium half. Not run in the engine.

**Smallest fix.** Tooling, no template change: `oracle-order` defaults `--min-fill` to the whole size (an order filled
or taken all at once pays one carrier), refuses a `--min-fill` whose part margin `q(minFill, rate)` is below the dust
floor, and states the cost per part (`q(minFill, tip) + carrier`, and the number of parts the size allows); 27.4 and
`order_risk_lines` give the bound per part. Contract, for the next template change: in `amount()` require
`quoteOf(n, myRate) >= PLAIN_DUST` (no part's margin can fall to the dust rule) and charge the carrier of a part to the
side that writes the lineage (in a take with a pair the old pair's carrier already moves with `keep` when `n == size`;
only a split needs a new one). What it could break: orders that want many small fills pay the carrier on each or must
name a larger minimum; the contract change moves the pair template hash and every pinned vector.

### R10-F2 (Medium). A cure after the expiry turns a valid liquidation into a lapse at the entry

**Affected:** `KpfPendingO.cure` (no date, any time while pending), `KpfPairO.renew` (`OpTxInputDaaScore(self) <=
expiry`, review 9), `lapseSettle`; DESIGN-ORACLE-IMPL 27.3 row 9 ("safe (since row 3 a cure accepted after X is never
renewed)") and 27.2 ("a cure ... written after X is never renewed: it lapses and settles at its entry").

**The rule that is missing.** A claim is valid when `P + W <= X`, so a valid claim can be pending until `X` itself,
and while it is pending the pair is a `KpfPendingO`, which no renewal reads. The cured state is written by the
loser's cure transaction; since review 9 a pair state accepted after its expiry can never be renewed. Before review 9
the underwriter renewed such a cured pair late and the cover went on; now the cure after `X` ends the cover, and the
lapse settle pays at the entry, whatever the price.

**The attack (`r10_exploit_a_cure_after_...`).** The fixture pair; the price falls to 9.20; the underwriter proposes
the (true) liquidation of the long at `P = 1,000` (`P + W = 1,200 = X`: valid). From 1,200 anyone can finalize it
(forfeit: the long's part to the short, 2,954.2 KAS). The long's `cure` (in 0 the pending part, signed by the long; in
1 its funding; out 0 the underwriter's refund, out 1 the cured pair, bound 8.5652) lands first, at 1,201: a block of
ordering, a miner delaying the finalize, or nobody having finalized during `[P + W, X]`. The underwriter's renewal of
the cured pair at 1,201 is refused by the pair (its slot's bind is satisfied); at 2,400 anyone settles at the entry
10: the long gets 1,434.8 KAS for the 434.8 KAS it paid into the cure (its whole entry margin of 1,000 KAS back,
against an equity of 200 KAS at 9.20); the short gets 2,000.2 KAS instead of 2,954.2: **954 KAS** to the loser. The
same cure landing at 1,199 is renewed (control).

**Who can stop it.** The winner, the underwriter (paid `B + fee` at the finalize) or anyone, by a finalize that lands
before `X`; and an underwriter that renews in its window before it proposes moves `X` a term away from the claim
(`r10_holds_a_prompt_finalize_or_a_renewal_before_the_claim_keeps_the_liquidation`). It needs the claim pending at `X`:
`P + W` close to `X` (on the fixture, where `W` = 200 exceeds the step, every claim with `P + W` in `(X - step, X]` is
pending through the whole start of the renewal window) or a finalize nobody sent; on the testnet defaults (`W` 6 hours,
step 3.5 days) a prompt finalize lands days before `X`, hence Medium.

**Smallest fix.** No rule can bound the cure from above (nothing proves "before X"), so the claim must end earlier:
in `KpfPendingO.finalize` a claim is valid only if `P + challengeDaa + stepDaa <= expiry` (today `P + challengeDaa <=
expiry`). A valid claim is then finalizable at least one step before `X`, before the renewal window opens, and a
cure after `X` needs the finalize withheld for a whole step (3.5 days on the testnet defaults) instead of one block.
What it could break: no claim is valid in the last `W + step` before the expiry of an unrenewed pair; an on-time
underwriter renews from `X - step` and then claims against `X + term`, so only a late underwriter loses that stretch
(the pending template and every pinned hash change). Tooling now: `oracle-watch` and `kpf finalize` should tell the
winner and the underwriter to finalize a claim pending near the expiry before it, and the runbook should renew before
proposing inside the window.

### R10-N1 (Note). On the testnet defaults a late underwriter resumes the cover every term

`r10_bounded_with_tail_plus_idle_...` (fixture with `idleDaa` 300, so `tail + idle` = 500 < term 800): for five terms
the underwriter stays silent past `X`, waits until the lapse settle is due (accepted at due + 50) and renews at the
same DAA instead; each continuation is accepted before its new expiry and is renewable again, and a liquidation
proposed after the last renewal is valid and forfeits. Five premiums (100 KAS) over the five terms, the on-time number:
no extra premium; what the underwriter holds is the choice at each due date. A party ends it by any entry after an
expiry (the amend is refused renewal afterwards). This is 27.4's second item; the residual stands as documented.

### R10-N2 (Note). The wallet's view of the expiry

`oracle-watch` (`ocmds.rs` 1166-1175) prints "the underwriter may still renew" for every pair past its expiry and
before the lapse; for a pair whose UTXO was accepted after the expiry this is false (`renew` refuses it, and `kpf renew`
already refuses it locally with `OPair::unrenewable_why`). It also has no line for a pending claim within `W` of the
expiry (R10-F2).

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| 1 | The entry as the shares' anchor: which entries write it | only `fill` (both orders' limits) and `take` without a pair (the counterparty, within the order's terms) write it; every price-free entry, the remainder, P3 (`e3 == entryPrice`), the part of a hand-over, the restore, cure and forfeit hashes (computed in `propose` from `entryPrice`) and the renewal copy it; the walk's hostile `entry += 1` variants are all refused | by reading; the walk (3,005 + 741 hostile variants refused) |
| 1 | An odd cover rate (6 KAS + 1 sompi): rounding of the halves | the short's share ends at 13, the long's at 6.99999999; each side grows exactly to its end, in either order, never one sompi further | `r10_holds_an_odd_cover_rate_gives_the_extra_sompi_to_the_long_and_no_side_more` |
| 1 | A cure to the end of the loser's share, then a hand-over keeping the leaver's bound (`nb == bl`), then both sides growing | no room of the other side's share is taken | `r10_holds_a_cure_at_the_share_then_a_hand_over_keeping_the_bound_takes_no_room` |
| 1 | The clamp at 0: a take of a short order with no pair at `e = -10^18` | the shares are measured from 0: the order cannot add margin, the counterparty may take the headroom; the lapse settle pays the whole part to the order (the mirror of review 9's documented residual) | `r10_bounded_a_take_of_a_short_order_with_a_negative_entry_clamps_the_shares_at_zero` |
| 1 | A take without a pair choosing the entry for a long order; a side opened beyond its share | documented residuals (27.4), unchanged | `audit9_tests` (not re-run) |
| 2 | A party respending the pair every `quietDaa` inside `[X - step, X]` to keep the renewal from landing | each new UTXO is accepted before `X` and is renewed; only an entry after `X` fixes the date | `r10_holds_a_party_respending_in_the_window_cannot_stop_an_on_time_renewal` |
| 2 | An outsider forcing a state written after `X` (amend, unlisted transfer, a claim vouched by another key, an early lapse settle) | refused; a covered pair loses its renewal only by a party's entry, a taker of a listed side, its own underwriter's claim (R10-F2) or the underwriter's silence | `r10_holds_no_outsider_can_write_a_covered_pair_after_its_expiry` |
| 2 | Chained renewals in one block (every renewal at one lock time, the continuation's DAA = the block's) | at most two, and never ahead of the on-time schedule, for every date from `X - step` to five terms after `X` (step 25 DAA) | `r10_holds_a_late_renewal_burst_never_runs_ahead_of_the_on_time_schedule` |
| 2 | Renewal on an unaccepted parent | refused by `born >= 0` (the mempool's unknown DAA score); in a block the parent's DAA is exact and the sweep above covers it | by reading |
| 2 | Markets with `tail + idle <= term`: racing the due settle term after term | R10-N1 (documented, bounded: no extra premium; a party ends it by an entry after an expiry) | `r10_bounded_with_tail_plus_idle_...` |
| 3 | `r9_bounded_a_take_into_the_past_...` and the fill residual, repeated and below the dust floor | **R10-F1** | the three F1 tests |
| 3 | `r9_bounded_a_late_renewal_...`: re-derived over a sweep | holds | `r10_holds_a_late_renewal_burst_...` |
| 3 | `r9_bounded_a_renewal_after_the_lapse_settle_is_due_...` | holds as stated for `tail + idle > term`; otherwise R10-N1 | as above |
| 3 | A short order taken the same way; an order whose minimum fill is its size | a short order loses only its tip per part; a part of a whole-fill order is refused | `r10_holds_a_short_order_or_an_order_filled_whole_loses_only_tip_and_one_carrier` |
| 4 | 27.3's 17 rows marked safe | see the table below; row 9 is R10-F2, the rest hold | below |
| 5 | The seeded walk on 20 new seeds (7 to 26) x 400 steps: 286 opens, 1,147 amends, 201 top-ups, 180 transfers, 15 takes with a pair, 94 `closeDonor` and 113 `closeHost` alone, 80 close-versus-close, 1,249 renewals accepted (767 refused as touched after their expiry, 20 catch-ups), 702 claims (194 on half a pair; 66 contests won by YES with a losing receipt, 68 refuted, 15 late), 343 void claims restored, 180 lapse settles; 3,956 refusals as ruled, 3,005 hostile variants refused | no violation: every binding, floor, range, passive side's payoff and lapse payout, cure share and calendar bound kept | `r10_holds_the_seeded_walk_on_new_seeds_and_longer_runs` |
| 5 | The walk with the hostile steps (R9-F1's take into the past, R10-F1's small takes) interleaved | R10-F1 found on every shredded order; no other violation | `r10_exploit_the_seeded_walk_with_small_takes_...` |
| 6 | Live runs 1 and 2 replayed with the tool's builders on the current templates: 42 transactions (stakes, slot, orders, open; two partial liquidations, one forfeited, one contested and refuted with the bond 0.8 / 0.2 KAS; pool settle and close; a claim on the restored part, defended and upheld, its pool settled with a loser to 48 KAS and closed burning 2 KAS; two lapse settles; slot unbond and withdrawal; stake unbond and withdrawal; a second open, a claim on one unit and its cure to 8.6963; the renewal run 2 could not mine (underwriter-funded); amends, a mutual close at 10.5, a transfer at 10.2 with bound 8.6, a cooperative close, the slot's exit) | every one builds, signs and passes `verify::verify` (engine, fee floor, storage mass, block limits); the pinned artifacts equal the source templates. No ordinary path of the runs became impossible | `r10_holds_the_live_runs_replay_on_the_current_templates` |
| 6 | The builders' top-up of a contract-fixed payout (27.5) | a lapse settle owing the long 0.021 KAS writes 0.025 KAS; the funder pays the fee and 0.004 KAS, the short gets what the contract owes, the transaction is minable. The premium fold (`pay_underwriter`) and the vouch remainder fold happen only when the change goes to the underwriter's own key; otherwise a too-small output is refused, never redirected | `r10_holds_a_topped_up_payout_costs_the_funder_at_most_half_a_hundredth_kas`; by reading |

**Time table 27.3, the rows marked safe.** What the attacker controls and the worst case.

| Row | Entry, reads | Attacker controls | Worst case | Verdict |
|---|---|---|---|---|
| 1 | party entries, CSV `quietDaa` | when it respends its own pair | it invalidates an in-flight claim or renewal once per `quietDaa`; `quietDaa = 0` is a market both orders accepted | holds (`r10_holds_a_party_respending_...`) |
| 4 | `propose`, CLTV `t0` | the underwriter's `t0 <= now` | a claim pricing up to `lateDaa` back | holds (accepted: the underwriter's vouch) |
| 5 | `propose`, nothing against `X` | the proposal's timing | a claim pending at `X` blocks the renewal | **R10-F2** with row 9 |
| 6 | `contest`, own DAA = `P` | nothing (exact) | - | holds |
| 7 | `decide`, own DAA against `P + W` | when the contest lands | a late contest delays a valid claim by one round (its verdict ignored); no cure meanwhile | holds |
| 8 | `finalize`, CSV `W` and `P` | when the finalize lands | none for the forfeit (one owner); the race of row 9 | holds |
| 9 | `cure`, nothing | the loser chooses when its cure lands | a cure after `X` makes the pair unrenewable: lapse at the entry | **R10-F2** |
| 10 | `fill`, CLTV `activeFrom` | - | - | holds |
| 13 | `refund`, `min(expiryDaa, own DAA + 90 days)` | a take restarts the order's own DAA | delays only the anyone-refund; the owner cancels at any time | holds |
| 14 | `bind`, `x <= t + term + step` | any `x` up to that, including the past | binds into the past (R9-F1's root; R10-F1 uses it) | holds as an upper bound |
| 15 | `bind`, `split`, `unbond`, CSV `rateDaa` | an outsider's `advance` restarts the slot's age once per step | the owner's next bind waits `rateDaa` | holds |
| 16 | `advance`, `unbond`, CLTV bucket end | anyone advances only ended buckets | - | holds |
| 17 | slot `withdraw`, CSV `unbondDaa` | - | - | holds |
| 18 | stake `open`, own DAA + age <= anchor | both exact | - | holds |
| 19 | `deposit`, the same; round start exact | - | - | holds |
| 20 | `resolve`, CLTV `start + roundDaa` | a deposit and the resolve race (the miner picks) | the trailing side's last deposit counted or not | holds (accepted) |
| 21 | stake `withdraw`, CSV `unbondDaa` | - | - | holds |

## Not checked

- `fill` as the route of R10-F1 (by reading only: the first fill of an aged order into the past, later parts lapsing a
  term + tail + idle later); takes chained on unaccepted parents on a real node, and the real fees of a take (estimated
  from 27.7's metrics).
- The walk still has no partial hand-over, no partial close-versus-close, no claim by a close's other side racing a
  cure, and no two disputes sharing stakers (a stake becomes a receipt of one dispute, so by reading none can).
- The race of R10-F2 and of R10-N1 on a real node (mempool order, replacement); the harness runs both conflicting
  transactions at chosen DAA scores.
- The rest of the workspace's tests (only `audit10_tests` was run); the book-market family; `KpfStake` deposits racing
  a stake's unbond beyond reading.
- `ocmds.rs` beyond `oracle-order`, `oracle-watch`'s expiry lines and the builders the replay uses.

## Judgement

The family is converging on its covenant rules (four reviews of hostile variants, a walk over every entry point and a
replay of both live runs find nothing new in the bindings, makers, ids or shares), but not on its economics: both
findings here sit in residuals a fix declared bounded (27.4) or safe (27.3 row 9) without re-deriving them per part,
per repetition and per state the new rule can produce.
The structural change that would stop the pattern: state every residual as an invariant over a whole lineage or
order (what a party can lose without its signature, as a function of its own terms), test it as such in the walk,
and make a fix change one rule only after the walk checks that invariant for every entry that can produce the state
the new rule refuses.

## Fix status

Fixed in the working tree after `c20e406` (DESIGN-ORACLE-IMPL 28). The method first: the invariants of 28.1 (what an
order, a party, the winner of a liquidation, the underwriter and a staker can lose or be denied without signing again,
as whole-order and whole-lineage quantities) and a walk that checks them after every step with hostile steps on
(`invariant_walk_tests`, 20 seeds x 400 steps). Run on the templates of `c20e406` it found R10-F1 on every long-order
route (280 orders, 147.33 KAS beyond the bound; the `fill` route included) and R10-F2 on every late cure (108), and, new,
one sompi of rounding per split or hand-over (a Note, now an allowance of I-party). On the fixed templates it records no
violation (6,456 hostile variants refused, 64,381 checks).

- **R10-F1: fixed in `KpfOpenO`.** A part takes the carrier from the order only when it empties the order (`escrow`),
  and every part, and what it leaves, holds at least `PART_FLOOR` = 0.2 KAS of the order's margin (`amount`). An order
  loses at most `q(N, tip) + q(N, its premium half) + one carrier + its share of the renewals' premiums (+ a ninth of
  it for the payout floor)` however many parts it is taken in. `r10_holds_small_takes_...` (live run 2's order: 0.01 and
  0.19 unit refused, ten 0.2-unit takes cost it 0.2 KAS, before 3.15 KAS), `r10_holds_each_take_...` (the tip only),
  `r10_holds_the_seeded_walk_with_small_takes_...`.
- **R10-F2: fixed in `KpfPendingO`** as proposed: a claim is valid only if `P + W + step <= X`. A valid claim is
  finalizable a step before the expiry; what remains is the finalize withheld by everyone for that whole step
  (`r10_bounded_a_cure_after_the_expiry_needs_the_finalize_withheld_for_a_whole_step`; 27.3 row 9 now reads "bounded").
  The cost: no valid claim in the last `W + step` of an unrenewed term (an on-time underwriter renews first; a
  liquidatable pair waits at most `W`).
- **R10-N1: left documented** (28.3): no extra premium, and a party ends it by an entry after an expiry; a cover resumed
  this late offers a valid claim only from its next on-time renewal.
- **R10-N2: fixed in the tool.** `oracle-watch` reads `OPair::unrenewable_why` and tells the winner to finalize a pending
  valid liquidation before the expiry; `oracle-order` defaults `--min-fill` to the whole size, refuses a part margin
  below 0.2 KAS and prints the least part and the whole-order bound; `kpf vouch` refuses a claim that would be void
  unless `--void-ok`; `--any-slot` is now a switch (it took the next argument as its value).
- Not fixed, found while stating I-exit: no unilateral exit while the underwriter keeps renewing on time and refuses to
  vouch closes (28.6, item 1).

Script bytes: `KpfOpenO` 6,516 (was 6,204), `KpfPendingO` 4,399 (was 4,395), the others unchanged; artifacts and 41 of the
46 oracle golden vectors re-pinned. The live-run replay (42 transactions) passes unchanged.
