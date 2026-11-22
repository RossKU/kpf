# KPF review C11 (signing on two machines; the close of review C10 in the engine)

## Scope and method

Target: `crates/kpf-protocol/src/oracle_build.rs` (`KEY_INPUT_UNITS`, `OPartial`, `OBuilt::sign_partial`,
`OPartial::cosign`), `crates/kpf-cli/src/ocmds.rs` (`cosign_cmd`), `docs/TESTNET.md` 10.5c; and
`contracts/KpfCrossC.sil` `propose` kind 1 as built after review C10. The threat is the one the coordinator set: the
party that writes a partly signed file against the party that reads `cosign`'s printout and signs.

Files of this round:

- `crates/kpf-tests/tests/auditC11_tests.rs`: 2 tests on the engine (1 `c11_exploit_`, 1 `c11_holds_`).
- `crates/kpf-cli/tests/auditC11_cli.rs`: 1 test through the binary (`c11_exploit_`). The file a writer could have
  made is the tool's own file with its transaction edited and its storage mass and id set again; in the flow used
  (`x-open --partial`) the writer signs nothing, so nothing has to be signed again.

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p <crate> --jobs 2 --test <name>`):

- `kpf-tests`: `auditC11_tests` `ok. 2 passed`; `auditC10_tests` `ok. 4 passed`; `auditC9_tests` `ok. 5 passed`
  (both with the coordinator's edits for the close as built).
- `kpf-cli`: `auditC11_cli` `ok. 1 passed`.

## Findings

**One High and one Medium, both in `cosign`'s reading of the transaction; one Low; three notes.** The close of
review C10 holds in the engine.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C11-F1 | High | `cosign` names an output an object when its script is that object's and reads nothing of the output's covenant binding. An account whose output carries no covenant id is printed word for word as the honest one, verified and signed; no entry of `KpfCrossC` spends such a UTXO (both parties signing, the lapse, a claim, a top-up: all refused). In `x-open --partial` every input is the co-signer's: the writer locks the co-signer's 51.2 KAS for ever at no cost | `auditC11_cli::c11_exploit_cosign_prints_and_signs_an_account_whose_output_carries_no_covenant_id`, `auditC11_tests::c11_exploit_an_account_script_without_a_covenant_id_is_spent_by_nothing` |
| C11-F2 | Medium | After signing, `cosign` lists the transaction with the labels the file carries: the co-signer's change, sent to another key, is "to another party or a script this tool does not know" in the first listing (no warning) and "out 0: change 948.797917 KAS" in the second. The one sum the co-signer is given lumps "the objects above, the other outputs and the fee", and the heading is the writer's title | the same command-line test (its second half) |
| C11-F3 | Low | A partly signed transaction is an offer with no end: the co-signer lands it whenever it likes, at the prices agreed when it was written. The runbook says the writer takes it back by respending an input of its own; a writer with no input in it (`x-open`) has nothing to take back, and one whose inputs are its pairs must move a pair | by reading (`docs/TESTNET.md` 10.5c) |
| C11-N1 | Note | The amounts of the inputs "signed by the other party" are the file's. The co-signer's signature binds its own inputs' amounts and every output, so its own account is right; the fee it is shown is not bound | by reading (`cosign_cmd`, the sighash) |
| C11-N2 | Note | The file's network is compared with the command's; the transaction names none. `cosign --submit` prints and sends in one run: the reading is only a reading if the command is first run without `--submit` | by reading |
| C11-N3 | Note | A `created` entry that does not decode makes `cosign` fail after its printout; a writer's signature that fails is found at the verification, after the co-signer signed (nothing leaves the machine) | by reading |

## C11-F1 (High): an object is named by its script; its covenant id is not read

**Affected.** `ocmds::cosign_cmd` (the object's line), `OPartial::cosign`; any genesis a co-signer funds: the account
of `x-open` and of `x-swap`.

**The missing check.** `Ok(obj) if obj.spk() == o.script_public_key` decides that output i is "an object". The third
field of `created`, the covenant id, is not compared with anything, and the output's `covenant` is not looked at.
No contract writes an account's first output (N§3b: "no contract checks how the account began"), so a transaction
whose account output is a bare P2SH of the account's script is valid.

**What such a UTXO is.** `auditC11_tests`: with the id, `coopClose` and `lapseSettle` are accepted; without it
`coopClose` (both signatures), `lapseSettle`, a close and `addMargin` are refused, also when the account is the only
input without an id. Every entry begins with `OpCovInputCount(OpInputCovenantId(self)) == 1`.

**The attack.** The writer runs `x-open --partial` for an account of 1 KAS (its side) and 50 KAS (the co-signer's),
funded by the co-signer's UTXO, takes the covenant binding off output 1 and sets the storage mass and the id again.
It signs nothing: every input is the co-signer's. `cosign --tx bare.json --key exec`: the lines from "in an object:"
to the sum are identical to those of the honest file (`cross account`, both parties, the leg with its market and
roster, "margin: a 1 KAS, b 50 KAS"), no warning; the transaction passes the command's verification and is signed
(the test runs it without `--submit`; with it, it would be sent). 51.2 KAS of the
co-signer's are in a UTXO nothing spends. In `x-swap --partial` the writer signs its pairs over the same edited
transaction and loses its own positions with it: spite there, free here.

**Smallest fix, and what it could break.** In `cosign_cmd`, an output is an object only if it carries a covenant
binding whose id is the one `created` names, and that id is either the genesis id of this transaction for that
output (`covenant_id(authorizing input's outpoint, the outputs bound to it)`, the authorizing input one the tool
can name) or the id of an input of the object's own family. Print the id. Anything else is printed as "a script of
the account template WITHOUT a covenant id: nothing can spend it" and refused without an override. It breaks
nothing honest: every builder binds what it creates. The same check belongs wherever a wallet is handed an object
file (`x-show`, `inspect`): an account file whose UTXO has no id is not an account.

## C11-F2 (Medium): the second listing is the writer's labels

`finish_signed` prints the signed transaction with `output_labels` and `input_labels` from the file, under
"== cosign". The doc comment of `cosign_cmd` says what is printed is "never by the labels the file carries". In the
test the co-signer's change output is given to another key (the writer signs nothing, so nothing else changes):

```
== cosign: a refund to you: nothing leaves your wallet
  out 0: 948.797917 KAS to another party or a script this tool does not know
you put in 1000 KAS and get 0 KAS back in plain outputs: 1000 KAS go into the objects above, to the other outputs and to the fee (0.002083 KAS)
== cosign
  out 0: change 948.797917 KAS
  out 1: account 51.2 KAS
```

Everything needed is in the first lines, and a co-signer that reads them is safe. A co-signer that reads the last
listing, the one every other command of the tool has taught it to read, sees its change. Fix: the second listing
names outputs as the first does (or is not printed); the sum is three sums (into objects that name your key, to
other parties, the fee); an output that is neither the co-signer's nor an object naming its key is a WARNING with
its address; the title is printed as "the writer calls this: ...".

## C11-F3 (Low): an offer with no end

The writer's signatures are SIGHASH_ALL over this transaction and good for nothing else; nobody but the co-signer
completes it. But the co-signer completes it when it likes: the account's entries are the prices of the day the
file was written, and a co-signer that waits for the price to move lands an account whose other side is already
under water (and, with the liquidation of review C10, takes it if the writer is not quicker). The runbook's answer,
respending an input, is right and should be something the tool does (`--withdraw-offer`: respend the first input to
its own key), and `x-swap`'s account could carry a lease that starts at the file's DAA so that a late landing is at
least a short lease.

## The rest of the questions

- **An input that is not the co-signer's plain output.** `cosign` signs input i only if the file's entry for it is
  the plain output of the key, and the signature commits to that script and amount: on a UTXO that is anything
  else, or holds another amount, it is not a signature. By reading.
- **Inputs the co-signer is not told about.** Every open input is listed with "YOU SIGN THIS" and summed in "you
  put in": a writer that adds more of the co-signer's coins shows in the sum. An input of the co-signer's key that
  is not in `open` stays unsigned and the transaction is nothing.
- **A different transaction than the one verified.** No: the transaction signed is the one returned and verified.
- **Fee and mass after the signatures.** `sign_partial` sizes the open inputs with a 66-byte placeholder and gives
  them the budget of a key input; the honest account of the test is 319 bytes with a fee of 2,083 sompi for a
  compute mass of 2,049, and the family's test has two pair inputs beside the open one. Not driven further.
- **A co-signer that changes the file.** Where the writer signed, its signatures fail (the family's test); where it
  did not, all the money is the co-signer's own.
- **`sign_partial` leaving what only looks final.** It refuses when nothing is left open and when a contract's
  script needs a key that is not at hand; only plain key inputs are left. By reading.

## The close of review C10, in the engine

`c11_holds_the_close_of_review_c10_in_the_engine`: with both sides below the maintenance (two legs that offset, both
markets at 30) neither is liquidated and either closes, paid 300 KAS (its side at the prices moved against it) with
2,700.2 KAS to the other; a sompi more to the closer is refused. A side owed nothing with the pot under the
maintenance: the party owed everything closes and takes the 400 KAS (the carrier goes to the other). With the pot
over the maintenance the bankrupt side's close is refused and its liquidation accepted. `auditC9_tests` (250
accounts, 3,000 claims, the engine against the model) and `auditC10_tests` pass with the rule as built. C10-F2 is as
it was, and stated.

## Not checked

- The partial flow against a node; several open inputs of several keys; a file whose non-open entries lie (C11-N1)
  driven to a fee under the relay floor.
- `x-swap --partial` with the account's binding taken off (it needs the writer's pair signatures made again; the
  account's genesis is as unconstrained there as in `x-open`).
- Other genesis objects a co-signer might fund (an order, a judge stake): the same reading of `cosign` applies; not
  run.

## Judgement

Something above Low is new, and it is in the reading, not in the contracts: `cosign` verifies that the transaction
is valid and tells its user what the scripts are, but an account is a script under a covenant id, and it does not
look at the id. One condition closes C11-F1; C11-F2 is a listing printed from the wrong source. The account's close
is confirmed in the engine as it was judged on the model.
