# KPF review 12

## Scope and method

Target: the oracle-market family at commit `1313454` (the fix of review 11): `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil` and their pinned artifacts, the builders and codecs (`oracle.rs`,
`oracle_build.rs`, `bond.rs`, `judge.rs`) and `ocmds.rs` where a command leads a user into a loss (`oracle-close`, the
watcher mode of `oracle-watch`, `contest`, `deposit`, `renew`), read against `DESIGN-ORACLE-IMPL.md` 26 to 29,
`DESIGN-ORACLE-2.md`, reviews 6 to 11 and the two live runs. This round attacks the party's own close (R11-F1's fix), the
two-moment rule and the removal of the void for lateness (R11-F2, R11-F3), the 2-minute window, and the economic walk.

New files only: `crates/kpf-tests/tests/audit12_tests.rs` (13 tests: 1 `r12_exploit_`, 7 `r12_bounded_`, 5 `r12_holds_`,
one of them the honest life with the tool's builders) and `crates/kpf-tests/tests/audit12_walk_tests.rs` (random pairs
of the testnet market driven through the shapes the economic walk never produces, one test), and this report. Nothing
in `contracts/`, artifacts, `vendor/`, protocol or CLI code was changed. Runs:

- `CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test audit12_tests --test audit12_walk_tests`:
  `test result: ok. 13 passed; 0 failed` and `test result: ok. 1 passed; 0 failed` (2.43 s); the random-pair test also
  with `R12_SEEDS=5..40` (937 pairs, `ok`, 12.74 s).
- `audit10_tests` (16, the live-run replay included), `audit11_tests` (9, the honest life included), `audit6_tests` to
  `audit9_tests`, `oracle_attack_tests`, `oracle_tests`, `oracle_conservation_tests`: all pass on the current templates.
- The economic walk on new seeds: `ECON_SEEDS=21..36 ECON_SHORT_SEEDS=109..116 ... --test econ_walk_tests`: `ok. 2
  passed` (143.28 s), 63,476 and 30,659 invariant checks, no violation.

Numbers use the gold-like pair of review 11 on the testnet defaults (`OMarket::testnet`): 0.1 oz at 80,000 KAS an
ounce (8,000 KAS notional), 10x on each side (bounds 72,000 and 88,000: 800 KAS of margin each), cover rate 20,000 KAS an
ounce (2,000 KAS of cover; bond 10 % of it, 200 KAS; 160 to the defender and 40 to the disputer when refuted), W 1,200
DAA (2 minutes), quiet 600 DAA (60 s), lateDaa 100 (10 s), mmr 5 %, g 0.5 %, judge rounds 432,000 DAA (12 hours), stake
age 6,048,000 DAA (7 days), 100 KAS stakes, mu 1.25, burn 10 %, at 10 DAA/s. Volatility: 15 % a year (gold in a stable
unit) and 80 % a year (gold priced in KAS).

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| R12-F1 | High | A party alone takes its counterparty's whole side with a false close: on the gold pair 800 KAS for a 200 KAS bond, a winning bet unless it is contested with probability above 80 % (83.3 % at any leverage when the cover equals the range, at most 90.9 %; 34.4 % to 89.9 % over 1,039 random pairs). The tools cannot contest it: the pending part's address commits to the claimed price and `t0`, the node API the tool uses looks UTXOs up by address, and `oracle-watch` reads claims only from object files, so the victim's watcher shows its pair as "NOT AT THE NODE" and never `CONTEST NOW` | `r12_exploit_a_party_takes_its_counterpartys_side_by_a_false_close_the_tools_cannot_see`, `r12_bounded_random_pairs_...`; the watcher by reading (`ocmds.rs` 1246-1251, 1386; `cmds.rs` 98-110) |
| R12-F2 | Medium | A true close contested by the counterparty with one aged stake of its own and not defended is refuted: the closer's whole bond (200 KAS) goes to the contester (160 as the defender, 40 as the disputer), whose stake returns with the pool, and the closer stays in the pair. The defence (a YES deposit of 1.25 times the opener's stake, aged 7 days before the claim) works in the contract, but the closer's tools neither find the dispute nor prompt it; 29.1's E-exit ("the counterparty ... loses its contest stake") is true only if someone defends | `r12_bounded_a_false_contest_of_a_true_close_that_nobody_defends_takes_the_closers_bond`, the honest life (the defended case) |
| R12-F3 | Medium | The quiet time of a party's own close is the pair UTXO's age, and the counterparty restarts it with any entry of its own: at every minute's boundary the close and the counterparty's amend become valid at the same DAA on the same outpoint. With a miner choosing the conflicting transaction (inside the threat model) the counterparty holds the party indefinitely for 47.52 KAS a day; without one it is a fee auction each minute. E-exit's "landing as soon as the pair UTXO is quietDaa old" assumes nobody else touches the pair | `r12_bounded_the_counterparty_restarts_the_quiet_time_of_a_close_every_minute`, the random-pair test (1,039 races) |
| R12-F4 | Medium | A loser about to be liquidated loses nothing by a false close: it races the underwriter's due liquidation on the same UTXO, and if it lands, uncontested it takes the whole range (a gain of 1,300 KAS over its 300 KAS at the feed), contested it buys a judge round and the contest's latency (432,001 DAA, 12 h, measured from the proposal to the restore; up to W more if the watcher contests late in the window) for the bond. On the gold pair at 75,000 with no cure the break-even contest probability is 99.2 % (80 % a year) or 87.3 % (15 % a year); contested for sure it is still positive from the trigger (+76.72 KAS at 80 % a year, +5.83 at 15 %) | `r12_bounded_a_false_close_racing_a_due_liquidation_buys_a_judge_round_for_its_bond` |
| R12-F5 | Medium | A false close pending across the expiry makes the part unrenewable and unliquidatable: if the pair has not been renewed by `X - round` (12 h before the expiry), the loser's false close is refuted only after `X`, the restored part is written after its expiry (no renewal, review 9; no liquidation, review 11's claimability), and the loser is never liquidated: on the gold pair it keeps 337.70 KAS instead of forfeiting its 300 KAS, +137.70 KAS even when contested for sure. The tools let an underwriter renew at any time in `[X - step, X]` without a warning | `r12_bounded_a_false_close_pending_across_the_expiry_leaves_the_part_unliquidatable` |
| R12-F6 | Medium | A contest uses up the opener's aged stake for the stake age: the stake returns from the pool as a new UTXO whose age restarts, so each stake UTXO contests one claim per 7.5 days, while a false closer can post a new false close every 12.02 h (one round and the quiet time). A victim (or a market's watchers together) with k aged stake UTXOs stops k false closes per 7.5 days; the attacker's net on the gold pair is 800 - 200 k KAS, positive for k <= 3 | `r12_bounded_a_contest_uses_up_an_aged_stake_for_the_stake_age` |
| R12-F7 | Low | A late contest (at `P + W`, racing the finalize) delays a true close by a round for its fees: the verdict is ignored and the close settles, the contester's NO receipt wins its own pool | `r12_bounded_a_late_contest_delays_a_true_close_for_its_fees` |
| R12-F8 | Low | A void liquidation is free: landed one DAA after the last valid landing (held by the underwriter, or delayed), it freezes the part for W, clears both parties' listings and returns the whole bond; once per part per term | `r12_bounded_a_void_liquidation_freezes_and_unlists_the_pair_for_free_once_a_term` |
| R12-N1 | Note | The watcher's verdict uses the feed now for P (and for t0 unless `--feed-at-t0`), not the feed at P; it checks no session; its `CONTEST NOW` hint omits `--claim-tx`, without which `contest --node` refuses | by reading (`ocmds.rs` 1237-1288, 837-847) |
| R12-N2 | Note | The rule's wording: "judged at t0 and at P ... each mapped to that block's timestamp" (template, `two_moment_rule`) can be read as mapping t0 to the accepting block's timestamp too (the old fixture text says exactly that); nothing says what a claim is when the feed has no value at t0 (before the series, a gap) | by reading (`oracle.rs` 289, `ocmds.rs` 666-671, `oraclekit` `FEED`) |
| R12-N3 | Note | No command revokes a close that has not landed; a close delayed (or rebroadcast by anyone holding it) until the feed has moved more than g is refuted and costs the closer its bond. `oracle-close` lets a funder other than the closer post the bond; the refund goes to the closer | by reading (`ocmds.rs` 763-794, `oracle_build.rs` 859-883) |

No Critical finding.

### R12-F1 (High). A false close takes the counterparty's side, and the tools cannot see it

**Affected:** `KpfPairO.propose` kind 1 (the closer's signature, the bond and the closer's own maintenance only; any
price passes for the side it favours), `KpfPendingO.settle` (pays the closer at its own price moved by g), the market's
W of 1,200 DAA, `oracle-watch` and `cmds::refresh` (UTXOs looked up by the object file's address and outpoint),
29.1 ("a false claim ... nobody contests within W: outside the invariant"), 29.3 ("A false close costs its closer the
bond ... whenever a watcher contests it").

**The rule that is missing.** Before review 11 only the underwriter could post a price; now each party can, against the
one counterparty that matched its order, and the economics of a false claim were never stated against a contest
probability. The only stated defence is an automated watcher, and the shipped watcher cannot discover a claim it was
not given a file for.

**The attack (`r12_exploit_...`).** Inputs: the pair (`propose` kind 1, side long, `p = 88,442.21`, `t0` = now, the
long's signature), the long's own funding (the bond). Outputs: the pending part (fresh id), the long's change. The feed
is 80,000 at t0 and at P, so the claim is false at both moments, but it passes the long's maintenance rule (a high
price always does). Uncontested, `finalize` W later pays the long 1,600 KAS (the whole range) plus its bond and the short
0.20 KAS (the carrier). The short's mirror (a close at 1 sompi) takes the long's side the same way. Contested and
refuted, the long loses the bond (160 KAS to the short, 40 to the disputer).

| | take (the victim's value at the feed) | bond | break-even contest probability |
|---|---|---|---|
| gold pair, 10x, cover 1.25 x range | 800 KAS | 200 KAS | 80.0 % |
| any leverage (2x to 20x), cover = range | half the range | 10 % of the range | 83.3 % |
| any pair (the victim's value is the whole cover) | `q(n, coverRate)` | `sigma q(n, coverRate)` | `1 / (1 + sigma)` = 90.9 % |
| 1,039 random pairs (`audit12_walk_tests`, seeds 1 to 40) | 13,153,824 KAS in all | | 34.4 % to 89.9 % |

The closer's expected gain is `(1 - pi) take - pi bond`: +800 KAS at pi = 0, +300 at 50 %, 0 at 80 %, -190 at 99 %.

**What a victim's tools do.** The pending part's script commits to the claimed price and `t0` (one sompi or one DAA is
another address, asserted), its id is a genesis of the closer's transaction, and the node client implements only
`getBlockDagInfo`, `getUtxosByAddresses` and `submitTransaction`. `oracle-watch --node` refreshes each object file by its
address and outpoint: the victim's pair file becomes "NOT AT THE NODE ... nothing in this file is current", which is
neither `CONTEST NOW` nor exit code 3, and no command finds the pending part. TESTNET.md 7.3 states this for the book
market ("discovery on a live network needs an indexer ... which does not exist"); section 9 and 29.3 nevertheless present
`oracle-watch --feed-price` as the watcher every party needs. With the shipped tools the realistic contest probability
of a false close against a party that did not receive the closer's files is zero.

**The smallest fix.** Discovery first: the watcher subscribes to the party's pair addresses (wRPC `notifyUtxosChanged`
reports the removal), fetches the spending transaction (mempool by address, or the accepting block), decodes the
pending output and raises `CONTEST NOW`; `contest` then traces the claim from that body. Then the economics: a close is
never urgent, so a close's contest window can be hours (a market parameter `closeWindowDaa`, `finalize` of kind 1 needs
that age; every template and pin changes, an exit settles hours later); or a close bond of the counterparty's value
(break-even 50 %; an honest closer posts that much for the window). What they could break: the window change delays
every honest exit; the larger bond needs capital the closer gets back.

### R12-F2 (Medium). An undefended contest of a true close takes the closer's bond

**Affected:** `KpfStake.open` (the opener's stake on NO leads until YES deposits 1.25 times it), `KpfPendingO.decide`
and `restorePaying(1)` (the bond to the defender, which for a close is the counterparty, and the disputer, which can
be the counterparty too), `oracle-close` (says a false close costs the bond, nothing about defending a true one),
`oracle-watch` (a contested claim shows only "resolves when its dispute's round closes"), 29.1 E-exit.

**The attack.** The long closes at the feed (true at both moments). The short opens a dispute with one 100 KAS stake of
its own, aged 7 days. Nobody deposits YES. One round later NO wins: refuted, the part restored, 160 KAS to the short as
defender, 40 KAS to the short as disputer, and the short's receipt wins its pool (its stake back). Defended (the long's
YES deposit of 125 KAS aged 7 days), the same dispute is upheld, the close settles at its price and the short's stake is
forfeited: the contract is right, but the closer must hold an aged stake and must find the dispute, a new UTXO of the
contester's transaction (the same discovery gap as R12-F1). Each further attempt to exit costs the closer another bond
while the counterparty has aged stakes; the random-pair test moved 1,636,652 KAS of bonds this way over 1,039 pairs (the
model's settlement).

**Within the assumptions?** Literally yes ("a majority of participating judge stake can decide falsely"), but the tools
make the honest closer a non-participant, and 29.1 promises the opposite outcome. **Fix:** the closer's watcher watches
its own pending part and prompts `deposit --side yes` with its aged stake; E-exit and the `oracle-close` text state that
a party who closes must be able to defend (an aged stake), or a staking service must.

### R12-F3 (Medium). The counterparty restarts the quiet time of a close

**Affected:** `KpfPairO.propose` kind 1 (`quiet()`), every party entry of the counterparty (`amend` is enough: no value,
0.033 KAS), 29.1 E-exit, 29.3 ("a close is a party entry").

**The attack (`r12_bounded_the_counterparty_...`).** Five rounds: at the pair's age 600 the long's close and the short's
amend are both accepted by the scripts against the same outpoint; the amend lands; the long's close one DAA early on
the amended pair is refused; repeat. Consensus admits both at the same DAA, so the miner (or the fee) picks. Cost to the
counterparty: 1,440 amends a day, 47.52 KAS at the documented fee. The underwriter's liquidation needs no quiet time, so a
counterparty that holds the party until it is due for liquidation, or until a lapse at the entry, profits from the hold
(the lapse case: the 29.4 allowance "a lapse at the entry its own close could have avoided" assumes the close was
available). The walk's E-exit probe forges the pair's DAA to be quiet-old and nothing races it, so it cannot see this.

**The smallest fix.** Drop `quiet()` from the close: it is bond-backed and refused below maintenance, so the quiet time
protects nothing a false close's bond does not already price. What it could break: a loser's false close can then race
the underwriter's re-liquidation right after a restore (R12-F4 repeated per bond, at the bond's price).

### R12-F4 (Medium). A false close is a free option for a loser about to be liquidated

**Affected:** as R12-F1, and the race of two spends of one pair UTXO (29.3: "A close racing the underwriter's
liquidation is a race of two spends").

**The attack (`r12_bounded_a_false_close_racing_...`).** The feed is 75,000 (the long's trigger 75,789.47); the
underwriter's liquidation at 75,000 and the long's close at 76,170.33 (the lowest its maintenance rule accepts; any
higher price works) are both accepted from the same UTXO. The close lands; contested at once and refuted one round later
(160 KAS to the short, 40 to the disputer); the liquidation from the restored UTXO lands at once (no quiet), the long's
next close is refused for 60 s. Delay bought: 432,001 DAA (12 h). What the delay is worth to a loser without a cure (it
can close its own side once the feed is back above its trigger): E[n (f - bl) 1{f >= 75,789}] after 12 h:

| | from 75,000 | from the trigger |
|---|---|---|
| 15 % a year | 11.62 KAS (net of the bond -188.38) | 205.83 KAS (+5.83) |
| 80 % a year | 190.02 KAS (-9.98) | 276.72 KAS (+76.72) |

Uncontested, the same close (at the price paying the whole range) gains the loser 1,300 KAS over its 300 KAS at the feed,
so its break-even contest probability is 1,300 / (1,300 + 9.98) = 99.2 % at 80 % a year (87.3 % at 15 %). A cure is
offered on the gold pair at 75,000 (a 110.63 KAS top-up keeps the position), so there the delay buys nothing a cure
does not; where no cure is offered (a cure bound beyond the winner's reach, for a liquidation priced below about 74,055 on this
pair) the option is the loser's only one. Escalation adds rounds at a growing stake (each lead change 1.25 times the
other side, the attacker's YES total about 1.56 times larger per day of delay), and wins outright when the honest side
runs out of aged stake (R12-F6). **Fix:** R12-F1's; the race itself is inherent in two spends of one UTXO.

### R12-F5 (Medium). A false close across the expiry leaves the part unliquidatable

**Affected:** `KpfPairO.renew` (`born <= expiry`), `KpfPairO.propose` kind 2 (`born + W + step <= X`), the pending part
(no renewal reads it), `renew_cmd` (no timing advice).

**The attack (`r12_bounded_a_false_close_pending_...`).** X = 30,240,000; the pair not yet renewed; the long below
maintenance in the last step (a liquidation landing there is void: asserted). At `X - round + 100` the long's false
close lands; contested at once; refuted and restored at 30,240,101, after X. The restored part cannot be renewed nor
liquidated; the short exits by its own close at the feed (the long then holds 337.70 KAS: its 300 KAS at the feed, the
37.50 KAS of g the short's close pays and the 0.20 KAS carrier) or waits for the lapse settle at the entry. The long's net, contested for sure: +137.70
KAS over a liquidation. With the renewal done at `X - step` the liquidation from the renewed pair is valid (asserted).

**The smallest fix.** Tooling: `renew` and `oracle-watch` tell the underwriter to renew at the opening of the window and
warn in its last round. Contract (if wanted): a restore of a claim that landed before X stays renewable (the restored
state would have to carry the claim's P; a layout change).

### R12-F6 (Medium). Each contest uses an aged stake for 7.5 days

**Affected:** `KpfStake.open` and `deposit` (`own + ageDaa <= anchor`, the UTXO's own DAA score), `KpfStake.settle`
(winners return as fresh free stakes).

**The attack.** After a refuted false close the watcher's stake comes back from the pool as a new UTXO; the attacker's
next false close, 432,611 DAA (12.02 h) after the first, cannot be contested with it (refused), only with another stake
bonded 7 days earlier (accepted). One stake UTXO contests one claim per 6,480,000 DAA (7.5 days); the attacker can make
14 attempts in that period. Against k aged stakes the (k + 1)-th false close stands: on the gold pair 800 - 200 k KAS.
**Fix:** a winning receipt returns as a free stake that keeps its original bonding DAA in its state (the family writes
it, so it can be trusted like the anchor), or stakers keep many UTXOs (documentation: contest capacity is a count of
UTXOs, not an amount).

### R12-F7, R12-F8 (Low)

F7: at `P + W` the finalize and a contest are both valid; a late contest is accepted, the verdict ignored, the close
settled one round later (432,000 DAA), and the late contester's NO receipt wins its own pool (its 100 KAS back). The
market text says it for claims in general; it now applies to every party's exit. F8: a liquidation proposed from a UTXO
that can carry a valid one and landed one DAA after the last valid landing is void: W of freeze, both listings cleared
(every restore is unlisted), the whole bond back; the restored UTXO cannot carry another liquidation until the renewal.
Before review 11 the void paid the bond to the defender; now the freeze costs the proposer nothing. Fix if wanted: a
fixed fee per void to the defender (R11's alternative).

### R12-N1 to R12-N3 (Notes)

N1: `oracle-watch` judges with `--feed-price` (the feed now) at P and, unless `--feed-at-t0` is given, at t0; a watcher
that runs a minute after P may flag a claim true at P or miss one false at P; sessions are not checked; the hint
`kpf contest --pending F --stake S` is refused with `--node` without `--claim-tx`, a body the node API cannot fetch.
N2: "each mapped to that block's timestamp" names one block for two moments; the fixture's feed text ("the price at the
DAA score t0, mapped to the accepting block's timestamp") is the pre-review-11 rule; the accepting block's timestamp is
the miner's within the timestamp tolerance (a look-back of that length at P, far below g); a t0 before the series or in
a gap has no defined feed. N3: no revoke command for a close (spend the funding UTXO by hand); a close released late is
judged at its P; `oracle-close` does not check that the funder is the closer.

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| 1 | The close's maintenance rule at its edges: the lowest accepted long price and the highest accepted short price accepted, one sompi beyond refused, on the whole pair, half of it and a cured part; at the lowest accepted price the lowest price true at the feed does not trigger a liquidation (no true close and true liquidation coexist); a close signed by the other party | holds | `r12_holds_the_close_maintenance_rule_at_its_edges` |
| 1 | Repeated closes to grief | a true close ends the part; a false one costs the bond when contested (160 KAS to the victim); the grief that pays is the counterparty's contest (R12-F2) and its amends (R12-F3) | the tests above |
| 1 | A close after the expiry, with the underwriter gone right after the open; the slot's accounting | valid at any time, settles W later; the slot is not read (its cover stays counted until X + tail, the prepaid premium is not refunded) | the honest life |
| 1 | A pair with one owner on both sides (a forfeited part) | its owner is paid both sides whatever the price; a false price only risks 20 % of its bond to a disputer | by reading |
| 1 | The bond from or to the wrong party | the refund goes to the closer's key, the refuted bond to the other party and the opener; a funder other than the closer gives its bond away (N3) | by reading |
| 2 | A claim void on purpose | only a liquidation, only by landing after `X - W - step` (propose refuses one from a UTXO that cannot carry a valid one): R12-F8; lateness voids nothing | `r12_bounded_a_void_...`, `r11_holds_a_late_honest_claim_...` |
| 2 | Look-back under the two-moment rule (t0 far in the past, a claim parked until the feed returns) | a closer is paid at most the feed at both moments; a parked claim is true at P or refuted; the rule is the judge's, not the contract's (N2) | `r11_holds_a_close_is_paid_at_most_the_feed_when_it_lands`, the walk's look-back closes (52 in the new seeds, 0 below the feed) |
| 3 | A timely contest: mass, the trace, the stake age | the contest (0.0297 KAS), the deposit (0.0400) and the resolution build and verify; the stake age is R12-F6; the trace needs a body the tool cannot fetch (N1) | the honest life, `r12_bounded_a_contest_uses_up_...` |
| 4 | The walk on 24 new seeds (21 to 36 at the fixture, 109 to 116 with the short window) | no violation; its blind spots are listed below | `econ_walk_tests` with `ECON_SEEDS`, `ECON_SHORT_SEEDS` |
| 5 | R6: a verdict of another dispute's id delivered to a contested close | refused | `r12_holds_a_verdict_of_another_dispute_cannot_decide_a_close` |
| 5 | R7: two pairs of one state closed into one pending output | the second input is refused (the maker names the proposing input) | `r12_holds_two_pairs_never_close_into_one_pending_part` |
| 5 | R8: a binding written by the new paths | the close writes only the restore state, with the binding copied byte for byte | by reading (`pairHash`) |
| 5 | R9: renewal of a part restored after its expiry | refused | `r12_bounded_a_false_close_pending_...` |
| 5 | R10 / R11-F7: shredding by partial closes | a partial close is a split and keeps the part floor (0.19 and 0.5 unit of the 1-unit pair refused, 0.01 oz of the gold pair accepted) | `r12_holds_a_partial_close_keeps_the_part_floor` |
| 6 | The live-run replay; review 11's honest life | pass | `audit10_tests`, `audit11_tests` |
| 6 | The honest life with the new defaults: both parties of pair A exit by their own closes (half before the expiry, the rest after it) with the underwriter gone after the open; on pair B the long's true close contested falsely by the short, defended by the long's YES deposit, upheld, settled, the pool pays the long the short's stake less the burn | every step builds, signs and verifies; fees: open 0.0992-0.0993 (documented 0.0994), close 0.0406-0.0407 (0.041), finalize 0.0097, contest 0.0297, deposit 0.0400, resolve 0.0293, pool settle 0.0592, pool close 0.0199 KAS | `r12_holds_the_honest_life_with_own_closes_a_defended_close_and_a_vanished_underwriter` |

### Lead 4: what the economic walk does not see

- **E-exit is probed on a forged clock.** `exit_probe` sets the pair's DAA to `min(daa, now - quiet)` and nothing races
  the close; the lapse allowance builds the same forged probe. R12-F3 is invisible by construction.
- **False closes are only look-back prices** (within 2 g of the feed) and every false claim is contested at once
  (`c_daa = now + 1`) by one stake (`stake(70, 10 KAS, daa 0, outpoint 0xc0)`) reused for every contest, never consumed
  and always aged: contest probability 100 % but for the judge-failure modes. The walk cannot tell a winning bet
  (R12-F1, R12-F4) and cannot exhaust stake (R12-F6).
- **True closes are never contested**; the watcher contests only what its judge finds false and always wins: R12-F2
  and R12-F7 do not occur.
- **Claims never stay pending across other steps**: each claim is finalized (or decided) within its own step, so no
  close is pending across a renewal window or X (R12-F5), and no close races a liquidation (R12-F4).
- **P is the step's DAA**; the miner's choice of P, the timestamp mapping and sessions at the boundary are not driven.
- **The causes of E-party, in a trader's numbers (gold pair):** a renewal's premium, 7.67 KAS a term (0.96 % of a
  side's margin, 10 % of the cover a year): acceptable and agreed at open. A true liquidation: up to 400 KAS (the
  maintenance margin, 5 % of notional): the market's rule. The keeper fee beyond the loser's value: up to 37.5 KAS borne
  by the winner in a gap: acceptable. Rounding and the payout floor: sompi and 0.02 KAS. The lapse at the entry "its own
  close could have avoided": up to the whole move (2,000 KAS on the fixture in the new seeds), acceptable only if the
  close really is available, which R12-F3 and R12-F2 contest. The honest closer's spread: g, 40 KAS on the gold pair (5 %
  of its margin per exit, 0.5 % of notional): a high but documented exit cost; quoting `f / (1 - g)` pays the feed but is
  refuted by any tick down. "A false claim the judge upheld" (up to 1,358.74 KAS in the new seeds) is outside E-party by
  design and the walk never produces the uncontested case that R12-F1 shows is the cheap one.
- **The extension (`audit12_walk_tests`)**: 1,039 random pairs of the testnet market (sizes 0.01 to 1.01 unit, entries
  1,000 to 200,000 KAS, leverage 1.5x to 50x a side, cover 1 to 1.5 times the range, the feed where both sides may
  close): every false close paying the closer the range is accepted and settles (13,153,824 KAS taken in all, break-even
  34.4 % to 89.9 %, never above `1 / (1 + sigma)`); every exit at the quiet boundary conflicts with an amend at the same
  DAA and is refused one DAA early on the amended pair; every undefended contest of a true close moves the bond to the
  counterparty. No violation of the contract rules themselves.

## Not checked

- Mempool behaviour on a real node: the cost of keeping a contest out for 2 minutes (1,200 blocks of 500,000 grams just
  above the tool's default fee rate of 100 sompi a gram: about 606 KAS, an estimate, below the take of any pair larger
  than the gold pair; a watcher that bids a higher rate raises it in proportion), replacement by fee in the races of
  R12-F3 and R12-F4, and whether a node relays a close before its sequence lock is met.
- Discovery with other node methods (`notifyUtxosChanged`, mempool queries): proposed, not built.
- Escalation of a dispute by the attacker across X or until the honest side runs out of aged stake (arithmetic only).
- The judge's reading of the feed at a timestamp the miner chose, gaps, a t0 before the series (N2).
- A close by an entrant just handed the side (the same rule on the state; by reading), close-versus-close and transfers
  racing a close (two spends of one UTXO; by reading).
- `kpf-cli` and `kpf-protocol` tests, `invariant_walk_tests`, `audit11_walk_tests`; the book-market family.

## Judgement

The covenant rules of the fix hold: makers, fresh ids, the part floor, the maintenance rule of a close at its edges and
the claimability of liquidations survive every earlier attack re-run with the new paths.
What does not hold is the economics around the new unilateral exit: a party's false close is a winning bet against any
contest probability below 80 to 90 %, and the shipped tools make that probability zero (no discovery, no defence prompt,
a clock the counterparty can reset, stake that is used up per contest).
Convergence now depends on the watcher and the judge's capacity being real (discovery, defence, reusable aged stake) and
on a close window or bond sized to a stated contest probability, not on more covenant rules.
