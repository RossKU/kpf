# KPF review C15 (`cosign --market` and its clock)

## Scope and method

Target: `crates/kpf-cli/src/ccmds.rs` (`market_ids`, `markets_of`, the market lines of `inspect_committee`),
`crates/kpf-cli/src/ocmds.rs` (`cosign_cmd` with `--market` and `--daa`). The question: with `--market` given for
every object and the printout read, is anything left that loses the co-signer money without a refusal or a warning.

Files of this round: `crates/kpf-cli/tests/auditC15_cli.rs` (1 test, `c15_exploit_`, through the binary).

Runs: `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test auditC15_cli`: `ok. 1 passed`; `--test auditC14_cli`:
`ok. 2 passed` (with the coordinator's edit: both orders name their market, the one in the writer's is refused
under `--market`).

## Findings

**One Medium is new; one Low; three notes.** The fixes of review C14 hold.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C15-F1 | Medium | An account carries terms of its own beside its legs' markets, and `cosign` prints two of them (mmr, g) and compares none. An account in the known market, with `lateDaa` of a day (the script's maximum), a grace of 0 and a liquidation fee of 100 %, read with `--market` and `--daa`: not refused, not warned, and line for line the printout of the honest account. Any tick of the last day then serves the writer's claim, a liquidation is final at once, and its fee is all the loser is owed | `auditC15_cli::c15_exploit_an_account_in_the_known_market_is_signed_with_terms_of_the_writers_that_are_not_printed` |
| C15-F2 | Low | `--market` takes a market file without the roster's genesis facts as the co-signer's reference, without a word. `c-order` refuses such a file unless told (`--unchecked`): a roster that did not begin empty can hold forged tickets | the same test (its last lines) |
| C15-N1 | Note | `market_ids` takes the market's id from the file's block (the `id` field must agree) and the roster from the block: a file cannot state one market and carry another | by reading (`cmarket_from`) |
| C15-N2 | Note | A leg of size 0 is not read, and need not be: the account's script reads no ticket for it and renews no lease by it | by reading (`KpfCrossC.propose`, `keepAlive`) |
| C15-N3 | Note | The date check refuses a date at or before the present; one DAA later passes. An order that expires in the next block is refunded by anyone for the refund fee (at most a twentieth of its escrow); an account or a pair at the end of its lease settles at its entries, which the printout states | by reading |

## C15-F1 (Medium): the account's own terms

A pair and an order have their market's terms and nothing else, and the market's id commits to them: `--market`
settles them. An account has `XTerms` of its own (the term, mmr, g, the fee and its floor, the carrier, `idleDaa`,
`graceDaa`, `lateDaa`, `quietDaa`), which `x-open` and `x-swap` take from the markets (`terms_of`) and which a file
may carry as its writer likes inside the script's bounds. The printout has "maintenance 5 % of the notional,
tolerance 0.5 %" and the lease.

The test: the honest `x-open --partial` account (50 KAS a side) with `lateDaa` 864,000 where the market has 100,
`graceDaa` 0 where it has 100, `liqFeeBps` 10,000 where it has 50; `Cross::check` accepts it.
`cosign --market <the known market> --daa 33600`: signed, and its lines are the honest file's but for the object's
address, its id and the txid.

What the three terms do in the co-signer's account (by reading `KpfCrossC` and `KpfPendingX`):

- `lateDaa`: a claim is accepted with a tick up to `lateDaa` older than the account's UTXO, and is valid if it
  lands within `lateDaa` of the tick. With a day, the writer picks among every tick of the last day whose ticket
  still stands the one that is best for its close, or the one at which the co-signer's side was below its
  maintenance. The family bounded this to seconds on purpose (review C1, C1-F3).
- `graceDaa` 0: a liquidation is final in the block after it lands; `cure` has no time.
- `liqFeeBps` 10,000: the fee is the whole notional, capped by what the loser is owed: all of it goes to the
  proposer, who is the writer.

**Fix.** Print the account's terms whole, and with `--market` compare them with what `terms_of` gives for the
markets of its legs: refuse an account whose terms are not those (or are less careful: a longer `lateDaa`, a
shorter grace, a higher fee, a lower mmr or g). It breaks nothing: `x-open` and `x-swap` write exactly `terms_of`.

## C15-F2 (Low): a reference without genesis facts

The file given as `--market` is what the co-signer says it has checked. A file with no `roster` facts is one nobody
could check (`c-check` and `c-order` say so). `cosign` should refuse it as `c-order` does, or say "this market file
carries no genesis facts of its roster: it could not have been checked". A file the writer hands over together with
the transaction is a reference of the writer's whatever it carries: the runbook should say where the file comes from.

## The questions

- **A market file that is not what it seems.** C15-N1: the block is what counts. C15-F2 for the missing facts.
- **A leg of size 0 with a hostile roster.** Not read, and not used by the script (C15-N2).
- **Kinds `markets_of` returns nothing for.** A ticket, a charge and an exit are refused when a funder writes them
  (review C13). A roster a funder writes is printed with its words (the exit time, the charge line with its judge
  block) and warned of when none of its keys is the funder's; a roster that names the funder's key as a seat is a
  join into a roster of the writer's parameters, and those are printed. A stake: the judge block's bounds are
  checked; what a stake's printout says of its block I did not look at again this round.
- **The clock.** `--daa` is the reader's own word or the node's; either is the reader's. C15-N3 for the edge.

## Is the reading good enough to stand?

With C15-F1 closed, I find nothing left that loses the co-signer money without a refusal, a warning or a printed
term, given `--market` for every object and a reader that reads. But this is the fifth round in which one more
unprinted or uncompared property of an object was a way through (the covenant id, the labels, the keys, the
parameters' bounds and the kind, the market, the account's terms), and each was found by asking of one more field
"is it shown, and is it compared with anything". Two things would end that pattern before rebuild-and-compare does:

1. `cosign` prints every field of every object's state (a plain dump under the summary), so that no field is
   unprinted by omission;
2. where `--market` is given, everything an object could take from its market is compared with it, not only the
   two ids.

Until then the honest answer is: good enough with C15-F1 fixed, for a reader that gives `--market` and `--daa` and
reads; not something to rest more value on than a testnet's.

## Not checked

- The stake's printout (its judge block) and the roster's as a funder's seat, end to end.
- The claim with a day-old tick in the engine (the rule is review C1's, `t0 + lateDaa >= born`, read again).
- The flow against a node.

## Judgement

One Medium is new: an account's own terms are neither printed nor compared, and `--market` says nothing of them.
The rest of the fix does what it says: a market is named whole under every object priced in one, a market that is
not the reader's is refused, and a date already past is refused.
