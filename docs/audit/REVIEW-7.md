# KPF review 7

## Scope and method

Target: the oracle-market family after the review-6 fix (commit `4fc9acb`): `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil`, the library (`oracle.rs`, `oracle_build.rs`, `bond.rs`, `judge.rs`)
and the oracle commands (`crates/kpf-cli/src/ocmds.rs`). Read against `DESIGN-ORACLE-IMPL.md` 1-24 (24 in full),
`DESIGN-ORACLE-2.md`, `DESIGN-ORACLE.md`, `DESIGN.md` 3.1, 3.4, 4.5a-4.5g, and the failure patterns of reviews 1-6. The
review concentrated on what the fresh-id change replaced: before review 6 every output a pair entry wrote was found
under the entry's own covenant id (`OpCovOutputCount(selfId)`); since then the part an entrant gets and every pending
part are found by index and script only, checked by `fresh`.

Method: every finding is an engine run of the real compiled scripts with the covenant context (`common::run`), or a run
of the library builder the command calls, offline. New files only: `crates/kpf-tests/tests/audit7_tests.rs`,
`crates/kpf-protocol/tests/audit7_tooling.rs`, this report. Nothing in `contracts/`, existing docs, tests or sources was
changed.

## Findings

| # | Severity | Title | Tests |
|---|---|---|---|
| R7-F1 | Critical | Two pair inputs accept one and the same fresh output: whoever builds the transaction keeps the second pair's value (a bystander robs a stayer by a double hand-over; an underwriter deletes half a pair inside one proposal) | `audit7_tests::exploit_two_pairs_merge_into_one_part_by_transfer`, `exploit_partial_hand_overs_of_two_pairs_share_one_part`, `exploit_underwriter_merges_two_halves_into_one_claim_and_keeps_the_other` |
| R7-F2 | Medium | `kpf deposit` commits a staker into any dispute file without the trace to an opening the documents rely on | `audit7_tooling::exploit_deposit_commits_a_stake_into_a_dispute_no_opening_made` |

No High and no Low finding. Notes follow.

### R7-F1 (Critical). One fresh output satisfies two pair inputs

**Affected:** `KpfPairO.transfer` (via `handOver` and `partAndRest`), `closeHost` (same `partAndRest`), `propose`;
reachable through `KpfOpenO.take` too (the pair's `transfer` beside the order). DESIGN-ORACLE-IMPL 24.1 (the
induction), 21 (the bounds of a dishonest underwriter, "a party that did not act loses value only through such a
claim ..."), 22 (public wording).

**The hole.** `fresh(out)` proves that output `out` has a new covenant id that no input and no other output carries. It
does not prove that the output belongs to the input running the check. Each entry then checks only that output `out`
has the script it computes and at least its own value. Two pair inputs whose entries compute the same script therefore
both accept one output, and nothing accounts for the second input's value: the builder of the transaction (who chooses
every unconstrained output) keeps it. Before review 6 this could not happen, because each pair's part, remainder and
pending part were counted under that pair's own covenant id, which no other pair holds. The part's script does not
depend on anything that distinguishes the two inputs: a hand-over replaces the leaving side's key, bound, listing and
tag, and a pending part copies the pair's state without its id.

**Route 1, a bystander or a party (no claim, no underwriter).** The victim's resting short order is filled twice at the
same price, in one expiry step, by two long orders (the attacker's, or anyone's): two pairs that differ only in their
ids, the long's tag and possibly the long's key and bound. Once both longs are listed (here the attacker lists its own),
one transaction transfers both longs, each whole, to a new key with the same `nb` and the part at the same index. Both
pair inputs pass; the part is one pair of 1,000 units; pair 2's 3,000.2 KAS has no output of its own.
Who loses: the stayer (victim short) had 4,000 KAS of margin in two shorts and has 2,000 KAS in one; it signed
nothing and there was no price and nothing to contest. The attacker gains 2,000.2 KAS. A bystander who is not the long
gains the same by buying two listed longs at their asks (the longs are paid in full). Partial hand-overs work as well:
each pair keeps its remainder under its own id and only the entered part is shared, so the pairs need not have equal
sizes (`exploit_partial_hand_overs_of_two_pairs_share_one_part`: 400 units of each, 800 KAS of short margin lost). The
eight-output scan caps one transaction at about six to seven merged pairs, not at one.

**Route 2, the underwriter alone.** The underwriter manufactures two identical pairs without any party: it proposes a
void liquidation on half the pair (t0 more than `lateDaa` before landing); after W anyone finalizes it and the half is
restored under the claim's id with exactly the remainder's state (unlisted, tags 0). It then proposes one liquidation
on both halves in one transaction, vouched once, with one pending output: both pair inputs pass, and the other half's
1,500.2 KAS goes to the underwriter's own output. Who loses: the long 500 KAS and the short 1,000 KAS of margin in a
half against which no claim exists at all (no window, no cure, no contest). The underwriter paid the void claim's bond
of 300 KAS (to the long) and takes 1,500.2 KAS: +1,200.2 KAS per round, repeatable on the surviving half, independent of
the cover it counts and of rho.

**Smallest fix.** Tie every fresh output to the one input that creates it. Simplest: require the fresh output at the
creating input's own index (`c == this.activeInputIndex`, as `payTo(self, ...)` already does; move a hand-over's payout
to `M - 1 - self`), so two inputs can never name one output. Alternatives: write the creating input's covenant id into
the new state (a hand-over's free tag field; a 32-byte field of the pending state), or, if the engine exposes it,
require the fresh output's covenant binding to name this input as its authorizing input. Add the merge of two
identical inputs to the seeded conservation run (it currently checks lineages, not that each output is claimed once).

### R7-F2 (Medium). `kpf deposit` does not do the wallet check the design leaves to it

**Affected:** `ocmds.rs` `deposit_cmd` (and `stake_of_path`, which never consults `--node`); `oracle_build::deposit`;
DESIGN-ORACLE-IMPL 13 ("Wallets must check the opening transaction"), 14 ("wallets must trace a dispute to its opening
before committing"), 24.2 deviation (a) ("a staker who joins its dispute is protected only by its wallet tracing the
claim's id to a propose").

**The exploit.** A forger writes a dispute by a genesis under its own id with a tally that no receipt backs (the
documented `exploit_a_dispute_written_by_a_genesis_traps_the_stakers_who_join_it`) and hands the object file to a
staker ("this claim is false, help refute it"). `kpf deposit --dispute FILE --stake MINE` reads the dispute from the
file alone, checks only that the side trails and that the stakes are aged, and builds and signs the commitment; the
signed transaction verifies in the engine and no warning is printed anywhere. Who loses: the staker, every stake it
commits (130 KAS in the test), stuck in a receipt of a pool that can never pay it. The contract-side limit is
documented as bounded "only to stakers who do not trace"; the shipped wallet never traces, so the bound is every honest
user of the command.

**Smallest fix.** Refuse (or require `--i-traced-it`) unless the dispute is traced: with `--node`, look up the
transaction that created the dispute UTXO and require it to be a `KpfStake.open` spending a `KpfPendingO` whose own
creating transaction is a `KpfPairO.propose`; offline, print that the dispute is untraced and that a forged dispute
keeps the stake. Read every stake, dispute and pool file through `load_obj` so that `--node` refreshes it.

## Notes (informational)

1. **The induction of 24.1 is stated about ids, and R7-F1 is about outputs.** "Every UTXO of a part's or a claim's id
   was written by a family template a party verified" remains true after R7-F1 (the merged part is a genuine template
   under a genuine fresh id). What the change lost is the older property "each input's continuation is its own", which
   value conservation rests on. 24.4's row "cover counted twice or not released when the id changes: holds" is also
   affected in the harmless direction: both merged pairs stay counted by the slot until their expiry for one part.
2. **Renewal after a due lapse settle** (point 5): the underwriter's counted renewal is accepted at the very DAA the
   lapse settle becomes due and makes it not due (`holds_only_the_underwriters_counted_renewal_cancels_a_due_lapse_settle`;
   the slot advanced 10 steps first). This is deviation 24.2 (c) as documented; a party's amend does not move the clock.
   It gives a colluding underwriter an open-ended option over a lapsed pair (renew and vouch when the price favours its
   party, let it lapse-settle at entry otherwise), which 21's "letting the cover lapse at a chosen moment" already
   covers in amount.
3. `cmds::refresh` keeps the file's covenant id when the node reports none. No loss found (a transaction built on it
   fails at the node), but the object is then shown with an id it does not have.
4. Orders are not reached by R7-F1: the pair an order's `fill` or `take` writes carries that order's own id as a tag,
   so two orders of one side never compute one script (`holds_two_orders_never_share_one_new_pair`), and the escrow is
   counted under the order's id. The scan's eight-output cap holds (`holds_the_fresh_scan_refuses_a_ninth_output`).

## Unproven

- **`closeHost` merge.** Two identical hosts and one donor writing one P3: `closeHost` uses the same `partAndRest` as
  `transfer` and P3's tag `tl3` is a free argument, so the route should be open; not built (the donor's own P3 carries
  its id as tag, so two donors cannot share).
- **A merge inside an order's `take`.** Two identical pairs transferring into the one part a resting order takes: both
  pairs and the order pass by the same reasoning, but the transaction needs the underwriter's bind signature; not
  built.
- **An order's `fill` sharing its pair output with a pair's hand-over part** (the long order's tag set as the
  hand-over's `et`, the counter tag 0, the existing pair's entry and expiry): the order would pay its escrow spend into
  a pair that is already funded and the builder would keep it. Requires a cover expiry at least a term after the
  order's own DAA and the underwriter's bind; not built.
- Whether the sighash of the vouch can be made to cover two proposals of different underwriters in one transaction to
  any effect: not explored beyond reading (each `propose` checks its own `uwKey` input).

## What was checked and found sound

- `fresh` itself: id not zero, no input of the id, outputs at most 8, every other output scanned; a genesis cannot
  reproduce an id (it hashes an unspent outpoint and the group's outputs). The authoring input can be anyone's; that
  gives no later handle on the id, since only an input of the id authorizes its outputs.
- Every entry that keeps an id (remainders, renewals, amends, add margin, restores, cures, forfeits, contest, decide,
  the judge's deliveries) still counts its id's outputs exactly; the restored part lives under its claim's id beside
  that claim's pool and receipts, and no entry of either template admits a second input of the id, so neither can be
  spent with the other.
- One dispute per claim id: `contest` needs `PENDING`, `KpfStake.open` needs exactly three outputs of the id, which no
  pair entry allows; a re-proposed restored part gets a new id, so old receipts (other id, other anchor) never count or
  settle in the new dispute; a loser cannot restart a decided dispute.
- Cover: slots bind by input or output index and count by expiry bucket; a part under a new id renews, is claimed and
  lapse-settles; nothing found that releases cover early or strands a slot.
- Lapse settle is clocked from the recorded expiry; no party entry moves it (only the counted renewal, documented).
- The judge bounds are one text in all five templates and the library; within them no combination was found that
  makes a contest impossible beyond what `oracle-check` already warns about.
- Payout indices (`self`, `M - 1 - self` with `j >= inputs`) of pairs and pending parts cannot collide between inputs.

## Are the documented limits stated truthfully?

Not since review 6. DESIGN-ORACLE-IMPL 21 bounds a dishonest underwriter by the cover it counts (a false uncontested
claim takes the loser's margin, at most `ρK` in total; a contested one takes nothing from the parties) and states that
"a party that did not act loses value only through such a claim, an upheld one, the agreed premium at a renewal, or
the lapse settle at its entry price"; section 22 tells users that "a false price contested within the window is
reversed: your position comes back as it was". R7-F1 makes each of these false: a bystander takes a stayer's whole
margin in a pair by two hand-overs in one transaction, with no price and no claim, and an underwriter deletes half a
pair (both margins) inside a proposal against which nothing can be contested, at a cost of one void claim's bond, without
limit by `ρ`. The documented exploits of the earlier slices (genesis pairs and disputes, the lapse settle at entry, the
back-dated renewal, the Sybil underwriter, the take without a pair) are stated accurately as far as this review checked,
except that the protection the genesis-dispute limit relies on, the wallet's trace, is absent from the shipped
command (R7-F2).

## Status after fixes

Fixed in the working tree (not committed); the decisions, the role table, the conservation statement, the book-family
table and the attacks on the change are in DESIGN-ORACLE-IMPL 25.

| # | Status | How | Tests |
|---|---|---|---|
| R7-F1 | **fixed** | every fresh output of the family names its makers: one field per creating role (pair: long, short, parent, host, donor; pending part: parent), each the creating input's outpoint commitment (blake2b of txid and index); each creating input requires its own field to hold its own commitment and every role its shape cannot share to be zero; no entry copies a maker | `audit7_tests::holds_two_pairs_never_merge_into_one_part_by_transfer`, `holds_partial_hand_overs_of_two_pairs_never_share_one_part`, `holds_underwriter_cannot_merge_two_halves_into_one_claim` (the three exploits, now refused by the input the output does not name) |
| R7-F2 | **fixed** | `oracle_build::trace_claim` / `trace_dispute`; `contest` and `deposit` take an explicit `Trust`; `kpf deposit` (`--opening`, `--claim-tx`) and `kpf contest` (`--claim-tx`) require the trace with `--node` and refuse without it unless `--accept-untraced`, printing what was not checked; dispute and stakes read through the node | `audit7_tooling::holds_deposit_refuses_a_dispute_its_trace_does_not_reach`, `oracle_builders` (a real trace), `oracle_cli`, `ocmds::tests::the_trace_gate_of_contest_and_deposit` |

The unproven routes, built:

| Route | Verdict | Test |
|---|---|---|
| `closeHost` merge (two identical hosts, one donor, one P3) | **holds** | `holds_two_hosts_never_share_one_p3` (and two donors: `holds_two_donors_never_share_one_p3`) |
| a merge inside an order's `take` | **holds** | `holds_two_pairs_never_merge_into_the_part_an_order_takes` |
| an order's `fill` sharing its pair with a hand-over part | **holds** (before the fix: not built; the makers make the two entries demand different fields) | `holds_an_order_fill_never_shares_its_pair_with_a_hand_over_part` |
| one vouch over proposals of two underwriters | **holds**: each propose requires its own underwriter's input; two vouches in one transaction are harmless | `holds_one_vouch_never_covers_a_proposal_of_another_underwriter` |

The documented limits the review found untrue (DESIGN-ORACLE-IMPL 21, 22) are true of the fixed code and say when they
were not. The book-market family was checked once more for this class (12 new `holds_` tests in `audit7_book_tests`):
no exploit, no contract changed. The pinned oracle artifacts and 41 of the 46 golden vectors were regenerated; the book
market's artifacts are byte for byte as before.
