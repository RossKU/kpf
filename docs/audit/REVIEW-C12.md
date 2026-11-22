# KPF review C12 (the fixes of review C11 in `cosign`)

## Scope and method

Target: `crates/kpf-cli/src/ocmds.rs` `cosign_cmd` as rewritten, `crates/kpf-protocol/src/oracle_build.rs`
`OPartial::check_signed` and `OPartial::bound`. The threat is as in review C11: the party that writes the file
against the party that reads the printout and signs.

Files of this round: `crates/kpf-cli/tests/auditC12_cli.rs` (1 test, `c12_bounded_`, three files a writer could have
made, through the binary).

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`):
`kpf-cli` `auditC12_cli` `ok. 1 passed`, `auditC11_cli` `ok. 1 passed` (with the coordinator's edit: the bare account
refused, the redirected change listed "to ANOTHER ADDRESS" and warned); `kpf-tests` `auditC11_tests` `ok. 2 passed`.

## Findings

**One Medium is new; one Low; three notes.** Both fixes of review C11 hold as far as they go.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C12-F1 | Medium | An object is counted "into the objects printed above" whoever its keys are. An account between two keys of the writer's, funded by the co-signer, is printed as an object with its covenant id, 51.2 KAS "go into the objects printed above, 0 KAS to other addresses", and there is no warning. The two keys are printed as addresses; nothing says that neither is the co-signer's. In `x-open --partial` the writer signs nothing and pays nothing, and both keys close the account between them | `auditC12_cli::c12_bounded_what_cosign_says_of_objects_that_are_not_the_cosigners_and_of_ids_the_file_gives` (first case) |
| C12-F2 | Low | The rule "an object carries a covenant id" is not true of every object: a free judge stake carries none by design and is spent by its owner. `judge-stake --partial` writes one, and `cosign` refuses it with "no entry of that script spends such an output, and the 100 KAS in it would be locked", which is false of a stake | the same test (second case) |
| C12-N1 | Note | `bound` takes the authorizing input's covenant id from the file's entry, also when that input is the co-signer's own coin. An account written as the continuation of an id the file gives the co-signer's coin is printed as an object under that id and signed. A node refuses the transaction (the id is neither the input's nor a genesis id), so nothing moves; with `--node` the command could read its own inputs' entries from the node | the same test (third case); by reading for the node |
| C12-N2 | Note | A second output under the genesis id of a printed object (not named in `created`) is listed as "a SCRIPT THIS TOOL DOES NOT KNOW" and warned, but not as sharing the object's id. No entry of `KpfCrossC` reads what else carries its id, so it harms nothing I see | by reading (`bound`, `KpfCrossC`) |
| C12-N3 | Note | `check_signed` is not the last word: it meters without the committed budgets and before the open inputs are signed, so a writer's input whose budget is too small, or whose script reads an open input's signature script, passes it and fails the verification after signing. Nothing is sent and the co-signer's signature stays on its machine | by reading |

## C12-F1 (Medium): an object that names none of the co-signer's keys

The first listing sorts every output into "to you", "an object" or elsewhere, and only the last is warned. An object
is the tool's own template under an id the transaction gives: that makes it spendable, not the co-signer's. The test:
`x-open --a <one key of the writer> --b <another> --a-margin 1 --b-margin 50 --funder <the co-signer's address>
--partial`, read by the co-signer:

```
  out 1: 51.2 KAS in an object (covenant id 14daa2f2894f):
owner (a) kaspatest:qzdmq4p6...   counterparty (b) kaspatest:qzgyg2mf...
you put in 1000 KAS: 948.797917 KAS come back to you, 51.2 KAS go into the objects printed above, 0 KAS to other addresses or unknown scripts
```

No warning. The co-signer's address stands six lines up, at "in 0"; a reader that compares it with the two in the
object sees that it is in neither. The command can compare for it. Fix: for each object printed, say which of its
keys are the co-signer's ("counterparty (b): YOU"), and count an object that names none of them with "other
addresses", under the WARNING. It breaks nothing: an executor that funds an object for third parties on purpose
reads the warning and signs.

## C12-F2 (Low): an object that has no covenant id by design

`judge_stake` writes a free stake with no binding, and `created` names it. `cosign` finds an object's script without
an id and refuses with the account's reason. A staker whose stake a second party funds cannot use the two-machine
flow, and is told its stake would be locked. The rule needs the object's kind: which templates live under an id
(an account, a pair, a pending part, an order, a roster's objects, a dispute and its receipts) and which must not
carry one (a free or unbonding stake).

## The questions

- **An output printed as an object or "to you" that the co-signer does not control.** "To you" is the plain output
  of a key that is at hand and among the open inputs' keys: right, and conservative (an output to another key of
  the co-signer's is "ANOTHER ADDRESS" and warned). An object: C12-F1.
- **A binding authorized by the co-signer's input or by the writer's.** Either is a genesis whose id `bound`
  computes from the transaction itself; whose input authors it changes nothing for an account.
- **A continuation of an id the file's entry lies about.** C12-N1: accepted by the tool, refused by a node.
- **Two outputs under one genesis id, one in `created`.** C12-N2.
- **A hostile state under the right script.** The printout shows the keys as addresses, the legs with their market
  and roster ids whole, the margins, the lease and the lapse date. It does not say whose the keys are (C12-F1) and
  compares the lease with no clock (there is none without `--daa` or a node). A careful reader has what it needs.
- **A redeem script in `created` for an output that is "to you".** The plain output of the co-signer's key is
  decided first; the entry in `created` is not read.
- **`bound` against consensus.** Where they differ `bound` is the wider (C12-N1), which costs a transaction no node
  takes. I found nothing `bound` refuses that a builder of the tool writes, but for C12-F2's object, which has no
  binding to check.
- **`check_signed` passing what fails once signed.** C12-N3: possible, and caught by the verification that follows.

## Not checked

- The flow against a node (C12-N1's refusal is by reading the binding rule, not by a submission).
- `x-swap --partial` with hostile keys in the account (the writer's pairs would have to be signed again).

## Judgement

The two fixes do what they say: an account's script without an id is refused before anything is signed, and no
line of the printout is the writer's label any more. One thing above Low is left, of the same kind as C11-F2 and
closed the same way: the printout tells its reader that money goes into an object and not whose the object is.
