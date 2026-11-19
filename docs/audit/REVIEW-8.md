# KPF review 8

## Scope and method

Target: the oracle-market family at commit `a6ed5bb` (working tree): `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil`, the library model the tests build from (`oracle.rs`, `bond.rs`,
`judge.rs`), read against `DESIGN-ORACLE-IMPL.md` 16-25, `DESIGN-ORACLE-2.md`, and reviews 6 and 7. Every entry point
was read for what it constrains and what it leaves to the builder; each lead was built as a transaction and run in the
script engine with the covenant context (`common::run`, the real compiled templates).

New files only: `crates/kpf-tests/tests/audit8_tests.rs` (10 tests: 6 `r8_exploit_`, 4 `r8_holds_`) and this report.
Nothing in `contracts/`, artifacts, `vendor/`, protocol or CLI code was changed. Run:
`CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2 --test audit8_tests`: `test result: ok. 10 passed; 0 failed`.

Numbers use the fixture market of `oraclekit`: 1,000 units, entry 10 KAS, long bound 9 (margin 1,000 KAS), short bound
12 (margin 2,000 KAS), cover rate 6 KAS per unit, expiry 1,200, tail 200, idle 1,000, W 200, quiet 20, bond 10 % of the
cover (600 KAS), carrier 0.2 KAS.

## Findings

| # | Severity | Statement | Proven by |
|---|---|---|---|
| R8-F1 | Critical | `closeDonor` lets its builder write P3's binding (`x3`, `cr3`, `pr3`) with one-sided bounds only and needs no host: a listed short, or anyone who buys its listing, moves the passive long into a P3 whose cover already "lapsed" (settled at once at the entry price), that can never be settled (frozen), or that no slot can renew and no claim can afford | `r8_exploit_short_alone_forces_its_long_into_a_lapsed_p3_and_settles_at_entry`, `r8_exploit_a_short_past_its_trigger_escapes_its_liquidation`, `r8_exploit_the_longs_own_close_on_the_lapsed_p3_pays_its_bond_to_the_short`, `r8_exploit_close_donor_freezes_the_long_with_an_expiry_far_in_the_past`, `r8_exploit_close_donor_inflates_the_cover_rate_so_no_claim_or_renewal_is_affordable` |
| R8-F2 | Low | `KpfStake.close(j)` names its burn output by argument: two pools closed in one transaction share one burn output and the closer keeps the other pool's burn | `r8_exploit_two_pools_closed_together_share_one_burn_output` |

No High and no Medium finding.

### R8-F1 (Critical). `closeDonor` writes P3's binding; the long's cover is the builder's choice

**Affected:** `KpfPairO.closeDonor`; through it `lapseSettle`, `renew`, `propose`/`KpfPendingO.finalize` on P3.
DESIGN-ORACLE-IMPL 16 (row "closeDonor requires P3 under its own underwriter with `x3 <= X`, `premium3 <= premium` and
P3's range within both cover rates"), 20.13, 21 ("a party that did not act loses value only through such a claim, an
upheld one, the agreed premium at a renewal, or the lapse settle at its entry price"; "letting the cover lapse at a
chosen moment: exploit, bounded ... summed over the colluding underwriter's pairs"), 22 ("once its cover has lapsed
anyone can settle your pair at its entry price, after a fixed time nobody can postpone").

**The rule that is missing.** `closeDonor` takes P3's cover rate `cr3`, expiry `x3` and premium `pr3` as arguments and
bounds them only so that the moving long is "never covered less, for longer or at a higher premium": `x3 <= expiry`,
`pr3 <= premium`, `bs3 - bl <= cr3` (no upper bound on `cr3`, no lower bound on `x3`). The protection the design relied
on, the host copying its own binding into P3, does not exist when there is no host: since review 6 `closeDonor` accepts
any host (or none) as long as P3 is fresh, and a donor alone in a transaction is accepted. A shorter cover is not a
harmless direction: the expiry is the clock of the lapse settle (`X + tail + idle`), the validity of every claim
(`P + W <= X`) and the renewal (`X + term` must be a ring boundary the slot accepts). The donor's long signs nothing.

**The attack transaction (theft, `r8_exploit_short_alone_...`).** The true price is 11.50; the long's side is worth
2,500 KAS, the short's 500 KAS.

1. The short amends its listing: `shortBid = 11.50` (its own signature; any listing will do).
2. `quietDaa` later, one transaction, signed only by the short's funding key:
   - in 0: the donor pair, `closeDonor(n = 1000 units, x = 11.50, e3 = 10, c = 1, bs3 = 12, sk3 = the short's own key,
     0, 0, 0, cr3 = 6, x3 = -750, pr3 = premium, hm = 0)`;
   - in 1: the short's P2PK funding (it authors P3's genesis);
   - out 0: the short's payout `owed(n, bs - x) + keep` = 500.2 KAS;
   - out 1: P3 under a fresh id (the donor's long, bound 9; the short's key, bound 12; entry 10; expiry -750), value
     3,000.2 KAS (the short adds 500.2 KAS);
   - out 2: change.
3. P3 lands at DAA 450; `lapseSettle` is due at `x3 + tail + idle = 450` (the donor pair's own was due at 2,400, its
   cover live to 1,200). Anyone settles at the entry price: long 1,000 KAS, short 2,000.2 KAS.

Numbers: the short pays 500.2 KAS into P3 and takes 500.2 + 2,000.2 KAS out: it leaves with 2,000.2 KAS instead of the
500.2 KAS its side was worth. The long, which did not act and had a covered pair, gets 1,000 KAS instead of 2,500 KAS:
**1,500 KAS taken**, no underwriter, no claim, nothing to contest. An outsider who buys a listed short gains the same
(it pays the short its asked price and settles the long at the entry). The loss is the long's whole gain over the
entry, up to the whole pair: in `r8_exploit_a_short_past_its_trigger_escapes_its_liquidation` the price is 13.50, a
liquidation of the short triggers and is valid on the donor pair (control: it forfeits the short's side), the short
runs the same transaction with `x = 12`, the underwriter's true liquidation on P3 is void and its 600 KAS bond goes to
the short, and the lapse settle pays the long 1,000 KAS of the 3,000 KAS it was owed: the short keeps **2,000 KAS of
margin it had lost**. If the long tries to close at the true price itself, its close on P3 is void and its own 600 KAS
bond goes to the short (`r8_exploit_the_longs_own_close_...`).

Two more shapes of the same free field:
- **Freeze** (`r8_exploit_close_donor_freezes_...`): `x3 = -4 * 10^18`. `x3 + tail + idle` is negative, which
  `lapseSettle` refuses at every DAA; `x3 + term` is no ring boundary a slot accepts, so no renewal; every claim is void.
  P3 has no exit that needs nobody: the long leaves only by the short's consent (mutual or cooperative close at the
  short's price) or by a taker for a pair nobody can settle. Cost to the short: fees.
- **Unaffordable cover** (`r8_exploit_close_donor_inflates_the_cover_rate_...`): `x3 = expiry`, `cr3 = priceMax`. The
  range stays within the old cover rate, so `closeDonor` accepts; every claim's bond becomes `q(n, priceMax * sigma)` =
  100,000,000 KAS (600 KAS on the donor pair) and no slot can count a renewal of `q(n, priceMax)`: P3 is never
  liquidated and lapse-settles at the entry after its expiry. (On a market with `sigmaBps = 0` the bond stays at its
  floor; the renewal is refused on any market.)

Why reviews 6 and 7 did not see it: review 6 (R6-F1) made the donor accept any host so that P3 is fresh, which made the
host optional; the binding checks were written for a look-alike host (DESIGN-ORACLE-IMPL 20.13) and bound only "longer,
higher, wider". Review 7 added the makers, which constrain who created P3, not what it says.

**Smallest fix.** In `closeDonor`, write P3's binding from the donor's own state, as `transfer` and `closeHost` do:
`bnd = rs.slice(BIND_AT, LISTS_AT)` and drop the arguments `cr3`, `x3`, `pr3` (keep `bs3 - bl <= coverRate`). The host
writes its own binding into P3 too, so close-versus-close is then possible only between pairs with byte-identical
bindings (same slot, cover rate, expiry and premium). What that could break: an honest close-versus-close between two
pairs of one underwriter in different expiry buckets, or with different cover rates or premiums, is no longer possible
(today it moves the donor's long to the host's, possibly shorter, cover, which is itself the harm above in a milder
form); the other exits of DESIGN-ORACLE-IMPL 17 remain. A weaker alternative, `x3 == expiry`, `cr3 <= coverRate`,
`pr3 <= premium`, closes the three shapes above but keeps a builder-chosen lower premium and cover rate that only the
underwriter bears; it needs the same equality on the host side to keep the pair of entries consistent. Either way the
documents (16, 20.13, 21, 22) should state that a part's binding is never rewritten by a party entry, and the seeded
conservation run should add `closeDonor` without a host and check that no party entry moves a part's lapse date or
cover rate.

### R8-F2 (Low). Two pools share one burn output

**Affected:** `KpfStake.close`.

**The rule that is missing.** `close(j)` checks that output `j` is the burn script with value at least this pool's
burn `b = value - base`; `j` is an argument, not this input's positional slot. Every other payout of the family is at
`self` or `M - 1 - self` (review 7's table); this one is not.

**The attack transaction.** Anyone, two fully settled pools A and B (each base 0.5 KAS, burn 1 KAS: 10 % of a 10 KAS
losing stake): in 0 A `close(1)`, in 1 B `close(1)`, in 2 the closer's funding; out 0 the closer's change (both bases
and 1 KAS), out 1 one burn output of 1 KAS. Both pools pass. Numbers: 2 KAS should be burned, 1 KAS is; the closer keeps
1 KAS. With k pools in one transaction it keeps all but the largest burn.

**Who loses.** Nobody's balance: the burn is value the losers forfeited and the design destroys so that a loser's stake
does not flow to whoever closes. The deterrent `phi` is lost: a coalition that disputes against itself (or a closer
that batches pools) recovers the burns it should have paid.

**Smallest fix.** Require the burn at a positional slot (`j == M - 1 - self`, `j >= tx.inputs.length`). What it could
break: batch closes must place
one burn output per pool at the mirrored index (the builder already places one output per pool).

## Notes

1. **A take without a pair can land in a past expiry.** `KpfOpenO.take` has no lower bound on `xe` (fill has
   `xe >= own DAA + term`); the slot's bind accepts a past `x` while its ring has not advanced past it. With a colluding
   underwriter's bind, an order is taken into a part that lapse-settles at once at its `e` (long: `e >= x`, short:
   `e <= x`): the order gets at least its own margin back and loses its tip. Bounded by the order's terms (as
   DESIGN-ORACLE-IMPL 24.4's "take with no pair"); not tested.
2. **The donor's cover rate has no upper bound even with a host.** With a real host P3 carries the host's cover rate,
   which may be larger than the donor's; R8-F1's fix (byte-identical bindings) also closes this.

## Checked and holds

| Attack | Verdict | Test |
|---|---|---|
| `closeDonor` with `x3` later than the expiry, `pr3` above the premium, `e3` below the entry, the underwriter as P3's short | refused | `r8_holds_close_donor_refuses_a_later_expiry_or_premium` |
| `closeDonor` with the donor's own binding (the fix's shape): the long keeps its lapse date and a valid close | holds | `r8_holds_close_donor_with_the_donors_binding_keeps_the_long_covered` |
| the mirror on the host side: `closeHost` without a donor writing an earlier expiry for the passive short | refused (the host copies its binding) | `r8_holds_close_host_cannot_shorten_the_shorts_cover` |
| a refuted part's finalize and its pool's settle (with a receipt) in one transaction | refused by both (each counts the claim id's inputs) | `r8_holds_a_decided_part_and_its_pool_are_never_spent_together` |
| `KpfStake.open` riding inside a `deposit` of the same id (the dispute as the "claim" input, three outputs) | holds by reading, not tested: `open` writes a receipt on NO at round 0 with the dispute UTXO's DAA as anchor and a fresh dispute state with `yes = 0`; `deposit` needs the trailing side, the dispute's round and anchor, and its own updated tally at the same outputs; no state satisfies both | none |
| a commit (SINGLE\|ACP) absorbed by another entry of the claim's id | holds by reading: every entry of the id counts its outputs exactly (resolve 2, decide 2, contest 3, settle 1, close 0, restore 1, the restored pair 0 or 1 with fixed scripts); `redeem` needs a pool that runs `settle` or `close` | none |
| payout index collisions between two family inputs (pair, pending, order, stake) | holds by reading: all P2PK payouts at `self` or `M - 1 - self >= inputs`, P2SH continuations at `self` or by id; only `KpfStake.close` breaks the rule (R8-F2) | review 7's tests; R8-F2 |
| renewal beside a claim, two renewals in one bind, a bind ref read by two claimants | holds by reading: refs strictly increase, each claimant reads its own index, one bind has one `x` | none |
| arithmetic at the 64-bit edge inside the bounds (`p * (BPS + g)`, `coverRate * sigma`, `need`, `share`, the capacity) | holds by reading: at most about `2 * 10^18` | none |
| open and propose in one transaction | impossible: a pair created in a transaction is not an input of it | none |

## Not checked

- The CLI (`ocmds.rs`) beyond noting that `closeHost`/`closeDonor` have no builder or command (TESTNET.md), so no tool
  leads a user into R8-F1; whether `oracle-watch` shows a P3's expiry and lapse date was not checked.
- The shapes of `docs/LIVE-1.md` were not replayed; only the engine fixtures were used.
- `KpfBond.split`/`unbond` racing a bind, and stake unbond racing a deposit, beyond reading (a double spend of one
  UTXO; one of the two transactions fails).
- Confusion with the book-market family: not revisited.
- The seeded conservation run and the rest of the workspace's tests were not re-run (only `audit8_tests`).
- Note 1 (a take into a past expiry with a colluding underwriter) was not built.
