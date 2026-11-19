# KPF review 11

## Scope and method

Target: the oracle-market family at commit `b3675fb` (the fix of review 10): `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil` and their pinned artifacts, the builders and codecs they are used
through (`oracle.rs`, `oracle_build.rs`, `bond.rs`, `judge.rs`) and `ocmds.rs` where a command leads a user into a loss;
read against `DESIGN-ORACLE-IMPL.md` 26 to 28, `DESIGN-ORACLE-2.md`, reviews 6 to 10 and the two live runs. This round
attacks the safety net of review 10 as well as the contracts: the invariants of 28.1, and the walk that checks them
(`invariant_walk_tests.rs`).

New files only: `crates/kpf-tests/tests/audit11_tests.rs` (9 tests: 1 `r11_exploit_`, 7 `r11_bounded_`, 1 `r11_holds_`
for the honest life with the tool's builders) and `crates/kpf-tests/tests/audit11_walk_tests.rs` (a
copy of the walk with the steps of lead 4 added, one test), and this report. Nothing in `contracts/`, artifacts,
`vendor/`, protocol or CLI code was changed. Run:
`CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test audit11_tests --test audit11_walk_tests`:
`test result: ok. 9 passed; 0 failed` and `test result: ok. 1 passed; 0 failed; ... finished in 99.48s`; the walk also
with `R11_SEEDS=37..56` (`ok`, 127.56 s). `audit10_tests` (16, the live-run replay included) and
`invariant_walk_tests` pass unchanged on the current templates.

Numbers use a gold-like pair on the testnet defaults unless the fixture market of `oraclekit` is named: 0.1 oz at
80,000 KAS an ounce (8,000 KAS notional), 10x on each side (bounds 72,000 and 88,000: 800 KAS of margin each), cover
rate 20,000 KAS an ounce (2,000 KAS of cover; bond 10 % of it, 200 KAS), premium 10 % of the cover a year (153.42 KAS an
ounce a term). Testnet defaults (`OMarket::testnet`, 10 DAA/s): term 28 days, step 3.5 days, tail 3.25 days, idle 7
days, W 6 hours, lateDaa 6,000 (10 min), quiet 60 s, slot rate limit 60 s, mmr 5 %, g 0.5 %, judge rounds 12 hours,
stake age 7 days, 100 KAS stakes. Volatility: 15 % a year for gold in a stable unit, 80 % a year for gold priced in KAS
(this market's unit; KAS's own volatility dominates).

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| R11-F1 | High | An underwriter that renews on time and refuses to vouch a close holds both parties in the pair until the premiums have narrowed the range to the maintenance bands: 52 terms (3.99 years) on the gold pair, each side paying 398.90 KAS of its 800 KAS margin to the underwriter, then a lapse settle at the entry. No party entry stops a renewal, no "do not renew" state exists, and every exit needs a third party (a taker, the counterparty's listing or signature, or the underwriter's vouch). I-exit permits it | `r11_bounded_an_underwriter_renewing_on_time_holds_a_party_for_52_terms` |
| R11-F2 | Medium | The look-back: a closer and the pair's underwriter vouch the best price of the last `lateDaa` moved by g (unrefutable); the settlement moves it back by g. The non-closer loses the window's best move against a close at the present feed (95 % case on the testnet defaults: 55.80 KAS, 7.0 % of its margin) and the whole g of the documented close (40 KAS more). For a liquidation the underwriter prices the lowest moment of the window (an uncured loser forfeits its whole value at the present feed). I-party permits both | `r11_bounded_a_close_is_priced_at_the_best_moment_of_the_last_late_daa`, `r11_bounded_a_liquidation_may_price_the_lowest_moment_of_the_last_late_daa`, the walk's measurement |
| R11-F3 | Medium (at `lateDaa` 100; Note at 6,000) | At the proposed `lateDaa` = 100 (10 s), an honest, true claim landing 101 DAA after its t0 is void and its whole bond (200 KAS) goes to the loser it was right about, who also keeps its side. Anyone who can delay a claim by 10 s (a full mempool bought for 100 blocks, a miner that produces them) farms the bond and escapes the liquidation | `r11_bounded_a_late_honest_claim_at_late_daa_100_pays_its_bond_to_the_loser`, the walk (the fixture's lateDaa is 100: 67 late claims void, 20,114.76 KAS of bonds to defenders in 36 seeds) |
| R11-F4 | Medium | The market bounds accept `rateDaa >= stepDaa`. There, an outsider's `KpfBond.advance` (no signature, no age) once per step restarts the slot UTXO's age before the owner's rate limit has passed, so the owner never binds again: no renewal, open or take for any pair of the slot; every pair lapses at its expiry and settles at its entry. The wallet does not warn | `r11_exploit_on_a_market_with_rate_at_least_step_anyone_stops_every_renewal_by_advancing_the_slot` |
| R11-F5 | Medium | The safety net: I-party values every side at the entry price (the lapse value), so it is blind to every loss that depends on the price: F1, F2, a refused due liquidation, and the lapse at the entry (in the extended walk the side the feed favoured lost 97,913 KAS of value at the feed in 278 lapse settles, up to 2,600 KAS in one, all within I-party). The close allowance lets the non-closer lose its whole lapse value and checks it only against the vouched price itself; the I-exit check (lapse date at most a term and its tails ahead) holds by construction for a renewing underwriter | the extended walk (`r11_holds_the_extended_walk_on_new_seeds`, its printed measurements); by reading |
| R11-F6 | Low | Any SIGHASH_ALL spend of a P2PK UTXO of `uwKey` vouches every `propose` in its transaction, and premiums, repaid bonds and keeper fees all land on `uwKey` as such UTXOs. A transaction built by someone else that the underwriter signs to pay 1 KAS (a wallet signing request) can carry a false liquidation; uncontested within W it forfeits the long's side (1,000 KAS on the fixture pair) | `r11_bounded_any_sighash_all_spend_of_an_underwriter_utxo_vouches_the_claims_beside_it` |
| R11-F7 | Low | The part floor of review 10 lives in the order only. A listed side and a second key of its own split the passive long by partial hand-overs into lineages whose long margin lies below the payout floor; each lapse settle pays that long nothing. On a 1-unit pair at 1 KAS a unit, 10x, the long loses its whole 0.1 KAS margin in six lineages. I-party allows one floor per lineage end, and the number of lineages is the counterparty's choice. The attacker pays more in fees than it takes | `r11_bounded_hand_overs_split_a_passive_long_into_parts_below_the_payout_floor` |
| R11-N1 | Note | The carrier an order pays with the part that empties it goes to the transaction's builder, not to the pair, when that part is a take of a whole pair (the pair's own carrier moves with `keep`); 28.3 says it comes back to the short | `r11_bounded_the_carrier_of_an_emptying_take_of_a_whole_pair_is_the_executors` |
| R11-N2 | Note | `kpf vouch` takes `t0` from the node at build time and refuses a claim that would be void by the expiry window, but not one that will land more than `lateDaa` after `t0` (a dry run sent later, `--t0` given by hand); harmless at 6,000, a lost bond at 100 | by reading (`ocmds.rs` 675-690) |

No Critical finding.

### R11-F1 (High). A renewing underwriter that refuses to vouch holds a party for years

**Affected:** `KpfPairO.renew` (no party can refuse it; a renewal needs only `born <= expiry`, the slot's bind and the
bands), `KpfPairO.amend` (`minPart >= 0`, `ask >= 0`: no field a party writes alone can mean "do not renew"),
`KpfPairO.propose` kind 1 (a close needs `uwKey`'s vouch), and 28.1 I-exit ("While the underwriter keeps renewing on
time the date moves by one term per renewal ... the party's own exits then need a counterparty ... or the
underwriter's vouch").

**The rule that is missing.** A party has no exit of its own while the underwriter renews: transfer needs a taker,
mutual close the counterparty's listing, cooperative close both signatures, close-versus-close a listed counterparty of
another pair with a byte-identical binding, a close claim the underwriter's vouch, and the lapse settle waits for the
renewals to stop. They stop only when `nbl (1 + m) >= nbs (1 - m)`.

**The attack.** The underwriter (alone, or siding with the counterparty) renews in every window `[X - step, X]` and
never vouches a close. Inputs of each renewal: the pair (`renew`), the slot (`bind`, the underwriter's signature), a
funder. No party signs. Measured on the gold pair: renewals 1 to 52 accepted, the 53rd refused (the bands), each moving
the long's bound up and the short's down by 76.71 KAS an ounce; each side's lapse value falls by 398.90 KAS (of 800 KAS)
over 3.99 years; the lapse settle then pays at the entry, so a side the price has favoured meanwhile gets nothing of
that gain. An amend writing a negative minimum (a candidate opt-out field) is refused. At 2 % a year the hold is 260
terms (19.9 years); at a premium of 0 it never ends. With the counterparty in the coalition the premium the counterparty
pays returns to it, and the victim's half (398.90 KAS) is the coalition's gain on top of the victim's denied gain.

**Can a party force a lapse?** Not on the testnet defaults: a party entry inside the window yields a UTXO accepted
before `X`, which is renewed (`r10_holds_a_party_respending_...`); the slot's `advance` by an outsider costs the owner
60 s per step (R11-F4 is the exception). Settling at the entry is not an acceptable exit once the price has moved: it is
the loss itself.

**The smallest rule (not implemented).** A party-priced close the underwriter cannot block: `propose` kind 1 accepts the
closer's own signature as the vouch when the pair UTXO is at least `T_req` old (a request time, for example a day: the
underwriter had that long to vouch a close and did not), with the closer's bond and the same contest window; the price
is judged against the feed as any claim, moved by g against the closer. It reuses the pending part, the bond and the
judge unchanged. What it could break: a false party-priced close stands if nobody contests within W (the accepted
assumption now extends from the underwriter to a party), and the counterparty must watch its pairs. The alternative, a
"do not renew" flag a party writes (renew refuses it), restores an exit at `X + tail + idle` but at the entry price: a
loser could then escape every coming liquidation at each term end (I-winner), so it is not acceptable alone.

### R11-F2 (Medium). The look-back option of a vouched price

**Affected:** `KpfPairO.propose` (`tx.daa >= t0` by CLTV: a lower bound only), `KpfPendingO.finalize` (`P - t0 <=
lateDaa`, P exact), `KpfPendingO.settle` (a close at `p` moved by g against the closer), the feed definition's judging
rule (a price within g of the feed at t0 is true), 28.1 I-party ("a close by the other party at the vouched price moved
by g against the closer").

**The rule that is missing.** Nothing ties `t0` to a moment the coalition did not choose: any `t0` in `[P - lateDaa,
P]` is valid, and the judge compares only with the feed at `t0`. The close settles the long closer at
`floor(p (1 - g))`; with `p = best (1 + g)` (within g, so not refutable) it is paid `best (1 - g^2)`.

**The attack (`r11_bounded_a_close_...`).** Inputs: the pair (`propose` kind 1, side long, `t0 = P - 6,000`, `p =
80,962.80`, the long's signature), the underwriter's vouch input, the closer's funding (bond 200 KAS). The feed at `P`
is 80,000; the best price of the window 80,560 (the 95 % look-back over 10 minutes at 80 % a year). Finalized after W
(uncontested, or contested and upheld: the price is within g of the feed at t0): the long closer is paid 855.80 KAS, the
short 744.40 KAS. A close at the present feed pays the long 800, the documented close (at the feed moved by g against
the closer) 760. The short, which signed nothing, loses 55.80 KAS against the fair close (7.0 % of its margin) and 95.80
KAS against the documented one. `t0` one DAA earlier is void (the closer's bond to the short).

| | 10 min (`lateDaa` 6,000) | 10 s (`lateDaa` 100) |
|---|---|---|
| look-back, expected / 95 %, gold at 15 % a year | 0.052 % / 0.128 % of the price (0.52 % / 1.28 % of a side's margin at 10x) | 0.007 % / 0.017 % (0.07 % / 0.17 %) |
| look-back, expected / 95 %, gold priced in KAS at 80 % a year | 0.278 % / 0.684 % (2.78 % / 6.84 % of the margin) | 0.036 % / 0.088 % (0.36 % / 0.88 %) |
| the gold pair, 95 % case at 80 %: the short below the fair close / below the documented close | 55.80 / 95.80 KAS | 6.80 / 46.80 KAS |

The liquidation side (`r11_bounded_a_liquidation_...`): the long stands at 76,000 (healthy; its trigger with g about
75,412) and dipped to 75,000 inside the window. A claim at 75,000 with `t0` in the dip is valid; a claim at the present
price is refused. Uncured, the long forfeits 400 KAS, its whole value at the present feed. A cure is offered here (bound
70,893.75, a top-up of 110.63 KAS within 6 hours, no value lost); where none is offered (a cure bound beyond the loser's
share) the dip costs the whole residual. The extended walk proposed 107 look-back closes in seeds 21 to 36 (24
settled, 29 more settled in seeds 37 to 56; the non-closer up to 295.76 KAS below a close at the walk's price on the
fixture pair, whose price path is far more volatile than gold) and 2 look-back liquidations.

**Can it be bounded more tightly?** The contract cannot read the present: P (exact at finalize) against t0 is the only
lateness it can measure, and the lock time is the proposer's choice (only `lockTime >= t0`). What it can do is a smaller
`lateDaa` (a market parameter; R11-F3 is its price). The judge can do better without a contract change: **a claim is
true only if its price is within g of the feed at t0 and at P** (P is on chain: the pending part's DAA score). The
look-back then shrinks to nothing beyond g (the closer is paid at most `f(P) (1 - g^2)`), the latency of an honest claim
is covered by g, and a claim delayed by others is refuted only if the price moved by more than g meanwhile. To keep the
documented g of a close against a colluding underwriter the settlement would have to move the price by 2g against the
closer (an honest closer then pays 2g). What it could break: stakers must read two moments of the feed; a claim during a
move faster than g in its own latency is refuted.

### R11-F3 (Medium at `lateDaa` 100). A late honest claim pays its bond to the party it was right about

**Affected:** `KpfPendingO.finalize` (`p - t0 > lateDaa` voids; `restorePaying(0)` pays the whole bond to the
defender), `oracle_build::propose` (`lock = t0`), `kpf vouch` (`t0` = the node's DAA at build time).

**The attack.** The underwriter vouches a true liquidation at t0 (lock time t0); the loser delays its landing past
`t0 + lateDaa`. Measured (`r11_bounded_a_late_honest_claim_...`): with `lateDaa` 100 a landing at `t0 + 100` forfeits,
at `t0 + 101` the part is restored and the loser receives 200 KAS; with 6,000 both settle. The contract cannot tell an
honest delay from a chosen one. Who can delay 10 s: a miner that produces the blocks of those 10 s (beyond "a few
blocks" at 10 blocks a second, so outside the assumption for one miner, within it for a coalition), or anyone who keeps
100 blocks full at a higher fee rate than the claim's (an estimate, not tested: 100 blocks of 500,000 grams at a rate
just above the tool's, on the order of 100 KAS) while the bond (200 KAS) and the escaped liquidation (up to the loser's
800 KAS) are at stake. The underwriter's defence is a replacement: re-sign with a fresh `t0`, spending the same vouch
input and pair outpoint at a higher fee, which a sustained flood outbids. Propagation alone: P is the accepting chain
block's DAA, a few seconds after submission at best; a 10 s budget leaves little margin for a congested mempool.

**The smallest rule.** Do not void for lateness: judge a claim at t0 and at P (R11-F2), and keep a long `lateDaa` (or
none) only as the bound on how stale a vouch may be. A delay then gains nothing unless the price moves by more than g
during it. If a void for lateness is kept, a late void should return the bond to the proposer less a fixed fee to the
defender (for the freeze of W), and only a void by the expiry window should pay the whole bond; but that makes a freeze
by deliberately late claims cost only the fee, so the fee must be set per W of freeze. What moves with `lateDaa` 100:
`kpf vouch` must refuse to send a claim built more than a few seconds before (R11-N2) and should build, sign and submit
in one step; the feed definition's time resolution must be finer than 10 s (the fixture's "a slow series published once
per hour" makes the look-back the publication interval, not `lateDaa`); `quietDaa` (60 s) still exceeds it, so a party
entry still revokes an in-flight claim.

### R11-F4 (Medium). On a market with `rateDaa >= stepDaa` anyone stops every bind of a slot

**Affected:** `KpfBond.advance` (anyone, once the first d buckets have ended; it rewrites the slot UTXO, restarting its
age), `KpfBond.bind` (`this.ageDaa >= rateDaa`, CSV), `checkBond` (any `0 <= rateDaa < 2^32`), `market_warnings` (no
line on the rate).

**The attack (`r11_exploit_...`, the fixture with rate 150, step 100).** At each bucket end `(j0 + 1) step + tail` an
outsider sends `advance(1)` (slot input, its own funding; no key of the slot). The owner's bind needs the slot UTXO 150
old; a sequence of 149 is refused by the script, a sequence of 150 is accepted by the script but admitted by consensus
only from `slot DAA + 150`, after the outsider's next advance (100 later) has spent the UTXO. Every renewal window
`[X - step, X]` passes this way; the pair lapses and settles at its entry from 2,400. On such a market a losing party
escapes its loss at the first expiry and the winner loses its gain; the slot can also never open or take. With
`rateDaa == stepDaa` the two race and the miner picks. The testnet defaults (rate 600, step 3,024,000) leave the owner
all but 60 s of every step.

**The smallest fix.** `checkBond` requires `2 * rateDaa <= stepDaa` (one text in the five contracts: every template and
pin changes), or at least a wallet warning. What it could break: markets with long owner rate limits; none exist.

### R11-F5 (Medium). The invariants value sides at the entry; the walk cannot see price-dependent losses

**Affected:** 28.1 ("A side's claimable value in a pair is what the lapse settle would pay it"), `invariant_walk_tests.rs`
(`lapse_values`, `ledger`, the close allowance `allow_loss(winner, whole lapse value)`, `check_exit`).

**What is too weak.** (1) A side's value is its payoff at the entry, so a step that changes who gets the price move is
invisible: the lapse at the entry, a refused due liquidation, the look-back of R11-F2, the hold of R11-F1. In the
extended walk the side the walk's price favoured lost 44,729.49 KAS (seeds 21 to 36, 127 lapses) and 53,183.72 KAS (37
to 56, 151 lapses) of its value at that price, up to 2,600 KAS in one, all allowed. (2) A close lets the non-closer
lose its whole lapse value, and the only check compares its payout with the vouched price, the claim's own number. (3)
I-exit is checked as "the lapse date lies at most a term and its tails after the present", which every renewal keeps
by construction; the length of a hold is never measured. (4) The payout floor is allowed once per lineage end, and the
number of lineages is chosen by others (R11-F7). (5) I-uw checks only the slot's value and id, not that it counts the
cover of every live pair (the conservation run does; the walk does not). (6) I-staker checks one shape (a pool with one
loser).

**What the walk could not reach (before this review).** Partial hand-overs (every transfer was whole), mutual and
cooperative closes (never run), claims with `t0` before `P` or landing late (every claim had `t0 = now`), claims vouched
by another key, disputes with more than one deposit, deposits after a round's window, receipts settled in more than one
batch or with winners first, verdicts from another dispute, the slot's unbond or split while cover is counted, pairs on
the testnet defaults (only the hostile orders used them), claims left pending while other steps run (every claim was
finalized in its own step), close-versus-close of unequal sizes or parts.

**What this review added (`audit11_walk_tests.rs`).** All of the first group above, in 36 seeds x 400 steps (21 to 56):
no violation of the invariants of 28.1; the hostile variants refused (seeds 21 to 36): 575 claims vouched by another
key, 134 verdicts from a dispute of another id, 99 unbonds and 122 over-capacity splits of the slot under live cover, 55
partial transfers moving the stayer and 55 with the remainder one sompi short, 116 mutual closes underpaying the short,
64 cooperative closes signed by another key, 28 deposits on the leading side; 28 disputes with five stakers over three
rounds and a deposit after the window, each pool settled in random batches (25 winner batches refused until the losers
were forfeited) with the winners' shares and the burn exact. The measurements above are printed by the test.

**The smallest fix.** State I-party a second time at the feed's price (the walk's true price): in a step a party does
not sign, its value at the feed falls only by its share of a premium, a valid liquidation at the feed (within g), or a
close at the feed (within g); list the lapse at the entry, the look-back and the hold as the documented residuals they
are, with numbers, and make the walk measure each. It changes no contract.

### R11-F6 (Low). Any SIGHASH_ALL spend of an underwriter UTXO is a vouch

**Affected:** `KpfPairO.vouched` (any P2PK input of `uwKey` whose script is one 65-byte push ending in SIGHASH_ALL);
every payout to `uwKey` (premiums of opens and renewals, a liquidation's `B + fee`, a cure's refund) is a P2PK output of
that key; nothing in the vouch input says it vouches.

**The attack (`r11_bounded_any_sighash_all_...`).** A transaction: the fixture pair (`propose`, a liquidation of the
long at 5 KAS), an underwriter UTXO (2 KAS, paying 1 KAS to a merchant), the merchant's funding (the bond and the fee).
The underwriter signed a payment request; the scripts accept the claim, and uncontested within W it forfeits the long's
1,000 KAS to the short. The tool never builds such a transaction and has no co-signing flow, so this needs an
underwriter that signs transactions built by others with the key that owns its slot.

**The smallest fix.** A dedicated vouch: the vouch input's UTXO is a script of the family (a P2PK with a tag, or a
`checkSig` over a tagged message as DESIGN-ORACLE-2 1.5 mentions), and payouts to the underwriter go to a different key
or script. What it could break: one more output per claim; the tool's funding of liquidations from the vouch input.

### R11-F7 (Low). Hand-overs split a passive side below the payout floor

**Affected:** `KpfPairO.checkPart` (`n >= minSize`, the rest too; no margin floor), `transfer`, `closeHost`,
`closeDonor` (partial), `lapseSettle` (a long payout below 0.02 KAS goes to the short), 28.1 I-party ("the payout floor
D at the lapse settle (once per lineage end)").

**The attack.** The short lists its side with the market's minimum; a second key of its own takes 0.19 unit five times
(each a partial `transfer` keeping the short's bound, the taker funding the part's carrier; the leaver's payout below
the floor stays in the remainder), each after the quiet time. The passive long now has six lineages with long lapse
values 0.019 KAS (five) and 0.005 KAS; every lapse settle pays it nothing, and the attacker's short keys get its whole
0.1 KAS margin. Five transfers cost about 0.16 KAS in fees (0.0312 KAS each): griefing at a loss, possible only where a minimum-size part
holds less than 0.02 KAS of a side's margin (a 10x pair priced below about 20 KAS a unit with the testnet minimum of
0.01 unit; never on the gold pair, where 0.01 oz holds 80 KAS).

**The smallest fix.** The order's `PART_FLOOR` in `checkPart` for the stayer's margin in the part and in the remainder,
or lapse payouts below the floor paid to their own side by the funder's top-up (the tool already tops up to 0.025 KAS);
or I-party stated per party (one floor per lineage the party itself created).

### R11-N1, R11-N2 (Notes)

N1: a take of a whole pair that empties the order: the part needs `range + keep` (the pair's carrier is in `keep`), the
order pays `q(n, rate + tip) + carrier`, and the builder keeps the carrier (0.2 KAS measured). Bounded by I-order; the
wording of 28.3 ("the short gets it back at the pair's end") is wrong for this shape. N2: `vouch_cmd` checks
`last_valid_landing` against the node's present but not `t0 + lateDaa`.

## Checked and holds

| Lead | Attack | Verdict | Test |
|---|---|---|---|
| 1 | Each invariant of 28.1 on realistic terms | I-order: its tip, its premium half, one carrier and its premium share (on the gold order of the honest life: no tip, 7.67 KAS of premium half per 0.1 oz filled, one carrier paid by the short order with the fill that empties it); no excess in the walk's 556 attacked orders (seeds 21 to 36); I-party: R11-F2, R11-F5, R11-F7; I-winner: holds as stated (a valid claim is finalizable a step before X; the wait for a claim after the last `W + step` is at most W, 6 hours); I-exit: R11-F1; I-uw, I-staker: hold | the tests above; the walks |
| 1 | `t0` bounded by the contract more tightly than `P - t0 <= lateDaa` | not possible: the lock time is only a lower bound and the proposer's choice; P is exact; the judge can (R11-F2) | by reading |
| 2 | A party forcing a lapse against an on-time underwriter (respending in the window, an entry after X, the slot's advance at rate 600, an opt-out field) | refused or ineffective on the testnet defaults; only R11-F4's markets | `r10_holds_a_party_respending_...`, `r11_bounded_..._52_terms`, `r11_exploit_..._advancing_the_slot` |
| 3 | The carrier: two orders both emptied in one fill (each pays one, the builder keeps one, 28.4); a part exactly at the floor (accepted: `>=`); the executor's carrier for fills that empty neither order (0.2 KAS each, measured in the honest life: 0.4 KAS and 3 sompi of premium rounding for three fills) | as documented, within I-order; N1 | `r11_holds_the_honest_life_...`, `r11_bounded_the_carrier_...` |
| 3 | Pushing an order's part below the floor after creation | renewals only (I-order's ninth); hand-overs of a pair have no floor (R11-F7); a cure moves the loser's bound outward only | by reading, R11-F7 |
| 3 | The claim rule `P + W + step <= X`: a loser escaping by timing; a liquidatable pair that can never be claimed | a loser cannot time a claim; a pair of a live underwriter is claimable within W of becoming liquidatable (after the on-time renewal); one that is never claimable has a silent underwriter (it lapses at the entry, as documented). In the dead zone the underwriter can only post void claims, each costing it the bond | the walk's winner scenarios (353 checks); by reading |
| 4 | The walk on 36 new seeds with the steps listed under R11-F5 | no violation; 5,973 hostile variants refused in 21 to 36 alone | `r11_holds_the_extended_walk_on_new_seeds` |
| 5 | `KpfStake`: forged or replayed verdicts (the dispute must carry the claim's id and resolve onto that input in one transaction; a dispute of another id is refused), a receipt redeemed twice (spent once) or against another pool (the pool's id and anchor), a lead never resolvable (the contested part has no entry but `decide`), rounds extended for ever (each lead change needs mu times the leader: bounded by the supply, about 87 rounds of 12 hours; an attacker playing both sides pays the burn and only delays the verdict, whose restore or forfeit it cannot change), stake age forged (the age is the UTXO's own DAA score, restarted by every re-creation, winnings included), a committed stake grabbed by another id (every input of a claim's id counts the id's outputs exactly) | holds | the walk's disputes; by reading |
| 5 | `KpfBond`: cover counted twice or freed early (a renewal counts the new term while the old bucket stays until its tail: conservative; `split` divides every bucket and checks each child's capacity, so value freed by a child's unbond is value no cover needs), a slot unbonding under live cover (refused) | holds | the walk's hostile slot steps (99 unbonds, 122 splits refused) |
| 5 | A contested claim pushed across X by escalating rounds | the restore or forfeit is then written after X and lapses at the entry; only the underwriter gains a lapse that way, which it has by silence; a forfeited part belongs to the winner on both sides | by reading |
| 6 | The live-run replay on the current templates (42 transactions) | passes | `r10_holds_the_live_runs_replay_on_the_current_templates` |
| 6 | The honest life of the gold pair on the testnet defaults with the tool's builders (29 transactions: stake, slot, orders, three partial fills, both sides handed over, a cure, a forfeit, an on-time renewal, a false claim contested and refuted with the bond 160 + 40 KAS, pool settle and close, three lapse settles, the slot's and the stake's exits) | every step builds, signs and verifies; no fee is more than twice the documented one (open 0.0949, propose 0.0386, renewal 0.0407, contest 0.0297, resolve 0.0293, cure 0.0099, finalize 0.0097 KAS; amend 0.0311 and lapse settle 0.0309 KAS against 0.0277 and 0.0275 in the older table of section 19, which is stale) | `r11_holds_the_honest_life_of_a_10x_pair_with_the_testnet_defaults` |

## Not checked

- Mempool behaviour on a real node: the replacement of a delayed claim (R11-F3), the cost of keeping blocks full for
  10 s (an estimate), and the race of R11-F4 when `rateDaa == stepDaa`.
- A party-priced close or any other fix of R11-F1 (proposed only).
- Partial close-versus-close and close-versus-close of unequal sizes in the walk (the walk joins only whole pairs of one
  size and entry); claims left pending across other steps; a cure racing a close by the other side.
- Storage mass of every new shape beyond the honest life (the tool's own check ran on each of its transactions).
- `ocmds.rs` beyond `vouch_cmd`; the book-market family; the rest of the workspace's tests (only `audit10_tests`,
  `audit11_tests`, `invariant_walk_tests` and `audit11_walk_tests` were run).

## Judgement

The covenant rules hold under a wider walk than before (partial hand-overs, closes, multi-staker disputes, slot
attacks, other vouchers, look-back and late claims): nothing in ids, makers, bindings, shares or pools broke.
What does not hold is what the invariants were never asked: they value positions at the entry, so every loss that
depends on the price (the hold, the look-back, the lapse at the entry) passes them by construction.
Convergence now needs economic invariants at the feed's price and a unilateral exit, not more covenant rules.
