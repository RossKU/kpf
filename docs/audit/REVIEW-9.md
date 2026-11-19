# KPF review 9

## Scope and method

Target: the oracle-market family at commit `7d9024e` (the fix of review 8): `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil`, the library model the tests build from (`oracle.rs`, `bond.rs`,
`judge.rs`), and `ocmds.rs` where a command leads a user into a loss; read against `DESIGN-ORACLE-IMPL.md` 16-26,
`DESIGN-ORACLE-2.md` and reviews 6-8. Every entry point was read for what it constrains on inputs and outputs and what it
leaves to the builder; each lead was built as a transaction and run in the script engine with the covenant context
(`common::run`, the real compiled templates).

New file only: `crates/kpf-tests/tests/audit9_tests.rs` (16 tests: 7 `r9_exploit_`, 9 `r9_holds_`, the seeded walk
among them) and this report. Nothing in `contracts/`, artifacts, `vendor/`, protocol or CLI code was changed. Run:
`CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test audit9_tests`: `test result: ok. 16 passed; 0 failed`.

Numbers use the fixture market of `oraclekit`: term 800, step 100, tail 200, idle 1,000, W 200, quiet 20, mmr 5 %,
g 2 %, bond 10 % of the cover, carrier 0.2 KAS; the fixture pair is 1,000 units at 10 KAS, long bound 9, short bound
12, cover rate 6, expiry 1,200, premium 0.02 KAS per unit and term; the fixture order accepts any slot and a premium up
to 0.10 KAS per unit and term.

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| R9-F1 | Critical | `KpfPairO.renew` never relates the expiry to the present, and `KpfOpenO.take` (without a pair) and `fill` (of an order older than a term) write an expiry in the past: the underwriter then renews the pair back to back at one DAA, one premium per renewal, until the range is no longer wider than both bands. An outsider with its own slot drains a resting order's whole margin; an underwriter alone drains two aged honest orders | `r9_exploit_a_take_without_a_pair_and_back_to_back_renewals_drain_the_orders_margin`, `r9_exploit_an_underwriter_alone_fills_two_aged_orders_into_a_past_expiry_and_renews_them_dry`, `r9_exploit_the_seeded_walk_finds_the_renewal_burst_of_a_take_into_the_past` |
| R9-F2 | Medium | The same missing rule on an honest pair: a silent underwriter renews late, twice at one DAA (the first premium buys a term already over, in which no claim was valid), or after the lapse settle has become due, which is then no longer due | `r9_exploit_a_silent_underwriter_charges_the_lapsed_term_by_a_late_double_renewal`, `r9_exploit_a_renewal_after_the_lapse_settle_is_due_cancels_it`, the walk (`r9_holds_the_seeded_walk_...` records it on 4 of 6 seeds) |
| R9-F3 | Medium | The headroom between the range and the cover rate is one pool both sides draw on: either party (`addMargin`) or an entrant (`transfer`, `closeHost`, `closeDonor`) takes it whole, after which the other side can add no margin and no liquidation of it offers a cure | `r9_exploit_the_short_takes_the_shared_headroom_and_strips_the_longs_cure` |
| R9-N1 | Note | A valid partial liquidation pays the underwriter `B + fee`, not its refund `B + carrier`: the carrier its vouch funded stays in the forfeited part (0.2 KAS); DESIGN-ORACLE-IMPL 26.4 says a valid settlement pays the refund back | `r9_exploit_a_partial_liquidations_carrier_goes_to_the_winner_not_back_to_the_underwriter` |

No High finding.

### R9-F1 (Critical). A pair whose expiry lies in the past is renewed back to back

**Affected:** `KpfPairO.renew`; through it `KpfOpenO.take` without a pair and `KpfOpenO.fill`. DESIGN-ORACLE-IMPL 16 (the
renewal row: "possible only from `X - step` (the slot's window)"), `KpfPairO.renew`'s comment ("the slot's window
(x <= t + termDaa + stepDaa) allows a renewal only from one step before the expiry, so the premium is paid at most once
per term of calendar time"), 26.2 (`take` without a pair "binds only the order, within its terms"), review 8 note 1
("the order gets at least its margin back and loses its tip").

**The rule that is missing.** `renew` computes `x2 = expiry + termDaa` and needs the slot's bind at `x2`. The slot's
window is `x2 <= t + termDaa + stepDaa` with `tx.daa >= t`: a *lower* bound on the present. Nothing bounds the expiry
from below by the present, so a pair whose expiry `X` lies `k` terms in the past is renewed `k` times at one DAA, each
renewal moving `X` by one term and taking `premium` per unit out of the margins (the long's bound up by
`premium - floor(premium / 2)`, the short's down by the floor), until the range is no longer wider than both bands. The
slot's own ring does not stop it: a slot written by a genesis (any state is allowed) at ring position 0 holds the
expiries `100 .. 1,200` (`(j0 + i + 1) * step`) whatever the present, and the bind may advance it as far as the calendar
allows, one renewal at a time. A pair gets an expiry in the past in three ways:

- `KpfOpenO.take` with no pair in the transaction: `xe` is an argument with no lower bound; the slot proves only that
  it counts `q(n, coverRate)` at `xe`.
- `KpfOpenO.fill`: `xe >= OpTxInputDaaScore(self) + termDaa` bounds it by the order's own acceptance; an order resting
  longer than one term can be filled into an expiry in the past (each order checks only itself, and a fill needs no
  second order: by reading, the builder may fund the other side).
- an honest pair whose underwriter was silent past `X` (R9-F2, bounded by `tailDaa + idleDaa`).

**The attack transaction (theft, `r9_exploit_a_take_without_a_pair_...`).** The victim's long order rests: 2,000 units,
limit 10.50, margin rate 1 KAS, `premMax` 0.10 KAS, any slot (`uwAccept = 0`, the command's default), 20,000 KAS of
escrow. The attacker holds a slot U (30,000 KAS, a genesis at ring position 0) and a second key A.

1. At DAA 30,000, one transaction, signed by U's owner (the bind) and A's funding key:
   - in 0: the order, `take(partOut = 1, n = 2,000 units, x = 10, e = 10, bound = 12.20, counterKey = A, pm = 0, 0, 0, 0,
     u = 1, k = 0, xe = 1,200, coverRate = 3.20, premium = 0.10, ...)`;
   - in 1: the slot U, `bind(t = 30,000, x = 1,200, d = 0, (OUT_REF + 1, 6,400 KAS), ...)`;
   - in 2: A's P2PK funding (it authors the part's genesis);
   - out 0: the order's refund, escrow less 2,000.2 KAS (margin and carrier); out 1: the part (long = the victim, bound 9;
     short = A, bound 12.20; entry 10; binding (U, 3.20, 1,200, 0.10)), 6,400.2 KAS, A adding 4,400 KAS; out 2: the slot;
     out 3: A's change.
2. Still at DAA 30,000 (the harness runs them at one lock time; on the network one per `rateDaa` of the slot, or one per
   block from children of a split slot), 21 transactions each `renew(1, 0)` on the part beside U's `bind(t = 30,000,
   x = X + 800, d = 8, (0, 6,400 KAS))`, signed only by U's owner: `X` goes 1,200 -> 18,000, the long's bound 9 -> 10.05,
   the short's 12.20 -> 11.15, each renewal sending 200 KAS of the part to U. The 22nd is refused by the band rule.
3. Anyone lapse-settles (due: `X + 1,200 = 19,200 <= 30,000`): the entry 10 is clamped to the long's bound 10.05, the long
   gets 0, A gets 2,200.2 KAS.

Numbers: the victim paid 2,000.2 KAS and receives 0. The attacker paid 4,400 KAS and receives 21 x 200 + 2,200.2 =
6,400.2 KAS: **2,000.2 KAS taken**, the whole margin. Without the renewals the same take lapses at once and pays the
margin back (`r9_holds_a_take_without_a_pair_binds_only_within_the_orders_terms`, the bound of review 8 note 1). The drain
needs `ceil(margin / (premium / 2))` renewals, so an order with `premMax = 0` is not drained (the command's default
`--prem-max` is 0; an order that any underwriter will fill names one above 0).

**Without any colluding party (`r9_exploit_an_underwriter_alone_...`).** Two honest orders of the fixture (a long and a
short, both accepted at DAA 0, any slot) are filled at DAA 30,000 by an outsider's slot with `xe = 800` (`>= 0 + 800`)
and the orders' maximal premium (0.10). The slot renews the pair 19 times at DAA 30,000: at the entry the long's equity
falls from 1,000 to 50 KAS and the short's from 2,000 to 1,050 KAS; the underwriter takes 1,900 KAS (plus the 100 KAS of
the first term) and signs nothing but its own binds. No claim exists, so nothing can be contested.

**Who can stop it.** During the burst the lapse settle is due (its date is `X + tailDaa + idleDaa`, behind the present),
so the victim, or anyone, can settle the part at its entry between two renewals; the renewal and the settle spend one
UTXO and the miner chooses which lands. A resting order's owner is usually not watching; a miner, or an attacker that
splits its slot to renew in consecutive blocks, completes the burst before an honest settle. Each renewal that lands
first takes `premium / 2` per unit from each side.

**Smallest fix.** In `KpfPairO.renew`, `require(OpTxInputDaaScore(self) <= expiry)`: the renewed UTXO was accepted no
later than its own expiry (an exact DAA, not a bound on the present). A part written by a take or a fill into the past
is then never renewable (it lapses at once and pays the order its margin, review 8 note 1), and a burst stops after one
renewal (the renewed UTXO is accepted at the present, past the old expiry by up to a term). What it could break: a pair
whose party acted after its expiry (an amend, a hand-over, a P3 or a restored part written after `X`) can no longer be
renewed; its cover lapses and settles at the entry. That is also what lets a party fix its lapse date (R9-F2). Optionally,
`take` could require `xe >= OpTxInputDaaScore(self)` as `fill` does with the term; a lower bound of a full term would
refuse every take of a pair in the last term of its cover, so it should not be the term.

### R9-F2 (Medium). A silent underwriter charges a lapsed term and cancels a due lapse settle

**Affected:** `KpfPairO.renew`, `lapseSettle`; DESIGN-ORACLE-IMPL 17 (5) and 22 ("once its cover has lapsed anyone can
settle your pair at its entry price, after a fixed time nobody can postpone"), `KpfPairO.lapseSettle`'s comment ("which
no party can move ... only the underwriter's renewal moves it"), and the market text of `oracle-market` ("once ... has
passed after its cover's expiry ..., anyone settles it at its entry price").

**The rule that is missing.** The one of R9-F1, on an honest pair: the renewal's window has a start (`X - step`) but no
end. (a) An underwriter silent past `X` renews at `X + 700` or later: the first renewal buys the term `[X, X + 800]`, in
which no claim on the pair could be valid (`P + W <= X` fails), and the second follows at once
(`r9_exploit_a_silent_underwriter_...`: the fixture pair renewed at DAA 2,300 to 2,000 and again to 2,800; the long pays
two premium halves at one DAA). (b) After the lapse settle is due the underwriter renews anyway and the settle is no
longer due (`r9_exploit_a_renewal_after_the_lapse_settle_is_due_cancels_it`: at DAA 2,500 the settle due at 2,400 is
accepted, and so is a conflicting renewal, then a second; the settle moves to 4,000). The settle and the renewal spend
one UTXO and the miner picks: a party that wants out at the entry (its side under water at the true price) is kept in
by an underwriter that sides with its counterparty, for one premium per term.

**Numbers.** (a) 2 x 20 KAS of premium at one DAA on the fixture pair, the first for a term already over. (b) The exit
of section 17 (5) is delayed by one term per renewal, at the premium. The walk records the shape (a) on its own on 4 of
6 seeds (seed 1 step 167, seed 4 steps 16, 93 and 174, seed 2 step 183, seed 5 step 58: two renewals of one lineage 0
to 358 DAA apart), with no hostile step involved.

**Smallest fix.** The fix of R9-F1. It bounds (a) to one premium per lapse and gives the parties a defence against (b):
any party entry after `X` (an amend costs nothing) makes the pair unrenewable, after which its lapse date is fixed.
What it could break: as in R9-F1. The wording of DESIGN-ORACLE-IMPL 22 and of `oracle-market` should say that the
underwriter can renew until a party acts after the expiry.

### R9-F3 (Medium). The headroom is shared, so one side strips the other's defence

**Affected:** `KpfPairO.addMargin`, `transfer`, `closeHost`, `closeDonor` (each lets the range grow up to `coverRate`),
and `propose`'s cure rule (`u1 - u0 <= coverRate`). DESIGN-ORACLE-IMPL 26.5 (1), left open there.

**The rule that is missing.** The headroom `coverRate - (bs - bl)` is not divided between the sides. The short can take
it all with its own `addMargin` (refundable margin, its own signature); an entrant taking the short side can post
`nb = bl + coverRate` (`transfer`) or `bs3 = bl + coverRate` (`closeDonor`, with no host). After that, the long's
`addMargin` of one sompi per unit is refused, and a liquidation of the long carries no cure, because the cured range
`bs - cb` exceeds the cover rate.

**The attack (`r9_exploit_the_short_takes_the_shared_headroom_...`).** On the fixture pair (headroom 3 KAS): the short's
`addMargin(S, 3 KAS)` (in 0 the pair, in 1 its funding; out 0 the pair at 6,000.2 KAS, the range equal to the cover
rate). The price falls to 9.20 and the underwriter's (true) liquidation of the long is proposed: on the untouched pair
the claim offers a cure (to a bound of 8.56) and the long cures; after the top-up `cureHash = 0`, the long's cure is
refused, and the finalize forfeits the long's part to the short. The long loses its 200 KAS of equity at 9.20 (less the
keeper fee, which goes to the underwriter), which a cure, or a top-up before the trigger, would have kept.

**Who loses, and why Medium.** The forfeit happens only at a true trigger, so the counterparty takes nothing the rules
do not hand it at a liquidation; what it takes away is the other side's means to avoid the liquidation. Either side can
do it to the other (the long by lowering its own bound), the first to act wins the headroom, and a hand-over gives the
entrant the same power over the stayer.

**Smallest fix.** Give each side its own share of the headroom at open (for example, the long may not go below
`entry - coverRate * rl / (rl + rs)` and the short not above `entry + coverRate * rs / (rl + rs)`, the rates of the two
orders), enforced in `addMargin` and on every hand-over, and size the cure against the loser's own share. What it could
break: a side can no longer use headroom the other leaves unused; a new binding field (or a rule from fields already in
the state) changes the pair template and every pinned hash.

### R9-N1 (Note). A partial liquidation's carrier is not refunded

`KpfPendingO.settle` (kind 2) pays `uwKey` `bond + fee` and puts `v - bond - fee` into the forfeited part; for `n < s` the
pending part's `refund` is `bond + carrier` (the carrier the underwriter's vouch funded, DESIGN-ORACLE-IMPL 26.4), and
only `cure` pays the refund. The winner gets the 0.2 KAS. The forfeited part needs a carrier of its own, so the code is
consistent; 26.4's sentence ("which a cure or a valid settlement pays back to the underwriter") is not. Fix the text, or
pay `refund + fee` and require the forfeited part to hold its floor.

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| 1 | Two pairs whose bindings differ in one field (slot id, slot key, cover rate up/down, expiry up/down, premium up/down) both accepting one P3, with P3 carrying either binding | refused (the other pair refuses) | `r9_holds_close_versus_close_joins_only_byte_identical_bindings` |
| 1 | Two covered pairs joined into one P3: capacity freed, the slot unbonding while P3 is covered | holds: nothing but the calendar lowers the ring; the bucket of 1,200 (12,000 KAS) stays until 1,400, P3's renewal is counted again at 2,000 | `r9_holds_two_covered_pairs_joined_stay_counted_until_their_liability_ends` |
| 1, 4 | A split moving the cover to one child while the other unbonds; renewal by either child | holds: each child of the id renews, the binding child counts it and cannot unbond | `r9_holds_a_split_slot_keeps_every_cover_counted` |
| 2 | An entrant (or the counterparty) making the stayer liquidatable at once, or changing its payoff at a price or its lapse payout | holds: the trigger reads only the loser's own bound, the range rule only lets the range grow, and P3's entry moves only in the stayer's favour; the walk checks the passive side's payoff at four prices and its lapse payout after every accepted `closeDonor` and `closeHost` alone, and refuses every hostile variant moving the stayer's bound, key, listing, the entry or the binding (639 hostile variants refused over the seeds) | `r9_holds_the_seeded_walk_over_every_entry_point_but_records_renewal_bursts` |
| 2 | An entrant taking the headroom | **R9-F3** | `r9_exploit_the_short_takes_...` |
| 3 | `take` without a pair beyond the order's terms: entry below the price, price above the limit, premium above `premMax`, cover rate below the range, counter rate below `minCounterRate`, the slot's owner as counterparty, another slot than `uwAccept` | refused; without renewals a past expiry costs the order its carrier (and tip) only | `r9_holds_a_take_without_a_pair_binds_only_within_the_orders_terms` |
| 3 | The same with a past expiry and the colluding (or own) underwriter's renewals | **R9-F1** | `r9_exploit_a_take_without_a_pair_...` |
| 3 | Maximum cover rate | bounded by the attacker's capacity: the slot must count `q(n, coverRate)` (rho x K); the claim bond is at most `sigma` of that; the order's own close then needs that bond (a capital freeze, the order's exits of section 17 (1)-(3) and (5) stay) | by reading |
| 3 | A slot about to unbond | `unbond` needs every bucket 0 after the calendar advance, so a slot cannot unbond while it counts any part it bound | lead 1 tests |
| 4 | A stake `open` riding inside a `deposit` of the same id (the dispute as the open's claim) | refused: laid out for the deposit the open refuses (its receipt at its own index must be NO, round 0, its anchor the dispute's DAA), laid out for the open the deposit refuses | `r9_holds_a_stake_open_never_rides_inside_a_deposit_of_its_id` |
| 4 | A SINGLE\|ANYONECANPAY commit absorbed by another entry of the claim's id (the restored pair's `amend`, the pool's `settle`) | refused by the entry's count of the id's outputs; the commit itself is satisfied | `r9_holds_a_single_anyonecanpay_commit_is_not_absorbed_by_another_entry_of_the_id` |
| 4 | The pool's burn at `M - 1 - self` beside another input's mirrored payout (a lapse settle's short) | holds: each at its own slot; swapped, both refuse. Every first payout is at `self < inputs`, every second at `M - 1 - self >= inputs`, every argument-named output is a P2SH of the family | `r9_holds_the_pools_burn_and_a_pairs_mirrored_payout_never_share_an_output` |
| 4 | The 64-bit edge: the largest size `checkPair` admits (92,232 units) with `bs = priceMax`, a liquidation at `priceMax` (bond 4.6 x 10^17 sompi, pending part above 5 x 10^18), finalize, lapse settle | holds | `r9_holds_the_arithmetic_at_the_largest_size_and_price` |
| 4 | The judge's arithmetic (`need`, `share`, `burnOf`), the slot's capacity and ring sums | holds by reading: inputs bounded by the supply (2.9 x 10^18 sompi) and 2.9 x 10^9 units, the largest product about 8.4 x 10^18 (`share`'s `u * a`); the ring's seven-claim sum could overflow only for a slot above 1.3 x 10^18 sompi, and an overflow fails the entry (no wrap) | none |
| 4 | Confusion with book-market UTXOs | holds by reading: every cross-template read pins a template hash (`PAIR_TPL`, `PENDING_TPL`, `BOND_TPL`, `JUDGE_TPL`); the book templates count the outputs of their id in every entry, so no oracle object rides on a book UTXO's id, and `KpfStake.open` against an order's id needs the order's owner (`cancel`) | none |
| 5 | The seeded walk: 6 seeds x 200 steps over opens, amends, top-ups, whole transfers, `closeDonor` with no host, `closeHost` with no donor, close-versus-close, renewals in the window and late (a silent underwriter per lineage), closes, true liquidations cured or forfeited, void claims, false liquidations contested in time, refuted, pools settled and closed, and lapse settles; free arguments drawn wide and every accepted shape compared with the stated rule (349 refusals as ruled, 639 hostile variants refused); after every step every live pair keeps its lineage's binding (expiry moved by one term per renewal only), its floor and its range within the cover rate | holds except the late double renewal (R9-F2), recorded with seed and step; with the hostile take enabled (4 seeds x 120 steps) the burst of R9-F1 is found (seed 1 step 53, DAA 5,444: four renewals of the taken part at one DAA) | `r9_holds_the_seeded_walk_over_every_entry_point_but_records_renewal_bursts`, `r9_exploit_the_seeded_walk_finds_the_renewal_burst_of_a_take_into_the_past` |

## Not checked

- The walk uses whole sizes only (no partial hand-over, partial close-versus-close or partial claim), no `take` with a
  pair, no contest that YES wins, no late contest and no pool with losing receipts (those are in
  `oracle_conservation_tests`, which was not re-run). Its close-versus-close lands the two amends and the close at one
  DAA (the harness does not check the `quietDaa` age against the UTXO's DAA).
- The CLI beyond: `oracle-order` defaults to any slot (`uwAccept = 0`), which R9-F1 needs, and `--prem-max` to 0, which
  makes the drain zero; the market text claims a lapse settle nobody can postpone (R9-F2). Whether `oracle-watch` shows a
  part's expiry against the present was not checked. No command builds a take without a pair.
- The shapes of `docs/LIVE-1.md` were not replayed; the rest of the workspace's tests were not run (only `audit9_tests`).
- The race of R9-F1's burst against an honest lapse settle on a real node (mempool and block ordering) was not run; the
  harness runs every renewal at one lock time.
- `KpfStake` deposits racing a stake's unbond, beyond reading (one UTXO, one of the two transactions fails).
