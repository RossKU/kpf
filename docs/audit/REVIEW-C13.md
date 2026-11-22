# KPF review C13 (the fixes of review C12 in `cosign`)

## Scope and method

Target: `crates/kpf-cli/src/ocmds.rs` (`object_parties`, `cosign_cmd`), `crates/kpf-cli/src/ccmds.rs` (`party_keys`),
`crates/kpf-protocol/src/oracle_build.rs` (`OPartial::bound`). The threat is as in reviews C11 and C12: the party
that writes the file against the party that reads the printout and signs.

Files of this round: `crates/kpf-cli/tests/auditC13_cli.rs` (1 test, `c13_exploit_`, four files a writer could have
made, through the binary; in each the writer signs nothing and every input is the co-signer's).

Runs (one cargo command at a time, `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test <name>`):
`auditC13_cli` `ok. 1 passed`; `auditC12_cli` `ok. 1 passed` (with the coordinator's edit: the foreign account marked
and warned, the stake accepted, the continuation from the co-signer's coin refused).

## Findings

**One High and one Medium are new; two notes.** The three fixes of review C12 hold as they are stated.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C13-F1 | High | `cosign` reads an object's script, id and keys, and not whether the object's own script accepts its parameters. An account with a term of 1 DAA (`checkCross` refuses it in every entry but the one both parties sign) is printed as the honest account and signed: the co-signer's 50 KAS leave only with the writer's signature. A free judge stake of the co-signer's key under a judge block `checkJudge` refuses (mu 0) is printed "no covenant id: its owner's key spends it" and signed: no entry spends it | `auditC13_cli::c13_exploit_cosign_counts_as_the_cosigners_objects_that_give_its_key_nothing_or_that_nothing_spends` (cases iv and iii); the two contracts by reading |
| C13-F2 | Medium | "yours in it" says a key is named, and the sum then counts the object as one "that names a key of yours", with no warning, whatever the key's part is. An account in which the co-signer is the party with a margin of 0 and the writer the party with 50 KAS, all 50.2 funded by the co-signer; a charge written by a genesis with the co-signer as its `payKey` (in no stake's holds: nothing ever settles it) | the same test (cases i and ii) |
| C13-N1 | Note | A ticket names no key and is counted with the objects that name none: right. A ticket (and a pending part, a claim, an exit, a charge) is never written by a transaction a co-signer funds unless the contract that writes it is an input | by reading (`party_keys`) |
| C13-N2 | Note | The stake rule: a stake in another status than free or unbonding must be bound and is refused without an id; a free stake of another key is printed with "NONE OF THIS OBJECT'S KEYS IS YOURS" | by reading (`object_parties`); the coordinator's test |

## C13-F1 (High): an object its own script refuses

Every entry of `KpfCrossC` but `coopClose` begins with `checkCross` (the term, mmr, g, the times, the legs, the
margins in their ranges); every entry of `KpfStake` begins with `checkJudge`. An object whose parameters fail them
is a UTXO those entries never spend. `cosign` decodes the state and prints it; it does not run the model's `check`.

- **The account.** The honest `x-open --partial` file, the account's term set to 1 DAA (`Cross::check` fails), the
  genesis id computed again. Printed line for line as the honest account (the term is not among the lines; the
  lease is printed from the expiry), "yours in it", "51.2 KAS go into objects that name a key of yours", no
  warning, signed. `addMargin`, `keepAlive`, `propose` and `lapseSettle` are refused for ever; `coopClose` needs
  the writer, who names its price for the co-signer's 50 KAS.
- **The stake.** A free judge stake owned by the co-signer's key with `muBps` 0 (`JudgeParams::check` fails), in
  place of the account's output. Printed "in an object (no covenant id: its owner's key spends it): judge stake of
  <the co-signer>", "yours in it", no warning, signed. `open`, `commit`, `unbond` and `withdraw` all run
  `checkJudge`: nothing spends it. The printout of a stake is its owner alone; the judge block is not shown.

The same reading applies to every template whose entries begin with a check of their own parameters (`checkPair`
and `checkMarket`, `checkParams` of a roster, `checkCharge`).

**Smallest fix.** An object is its script, its id and parameters its script accepts: `cosign` runs the model's
check of the decoded state (`Cross::check`, `JudgeParams::check`, the market's and the pair's, `RosterParams::check`,
`ChargeParams::check` and `judge_ok`) and refuses an object that fails it, saying which rule. It breaks nothing a
builder of the tool writes: every builder runs the same check before it builds.

## C13-F2 (Medium): a key that is named and owed nothing

- **The account with a margin of 0.** `x-open --a <the writer> --b <the co-signer> --a-margin 50 --b-margin 0`,
  funded by the co-signer. The state line reads "margin: a 50 KAS, b 0 KAS (the pot 50 KAS)", then "yours in it:
  <the co-signer>", then "50.2 KAS go into objects that name a key of yours, 0 KAS into objects that name none",
  no warning. The lapse pays A 50 KAS; A closes for its side at any tick. A reader must find that it is (b) and
  that (b) has 0. The tool knows both.
- **The made-up charge.** A `KpfCharge` by a genesis, `payKey` the co-signer's, 51.2 KAS in it. Printed "a charge
  that 2222... signed 10 KAS ...: pending; if the tick is refuted the bond and a tenth of the stake go to <the
  co-signer>", "yours in it", counted as the co-signer's. No stake holds it (review C5: a charge is settled only
  beside the stake whose `charge` wrote it): the bond stays in it for ever.

**Smallest fix.** Two things. (1) Say the part and what it holds, not only that a key is named: "you are the
counterparty (b): margin 0 KAS of the pot 50 KAS; you fund 50.2 KAS", and warn when the co-signer puts more into an
object than its part of it holds at the entries (in `x-open` the funder pays both margins by design: then the
warning is the point). (2) Kinds only a contract writes (a charge, a pending part, an account claim, an exit, a
ticket, a dispute, a receipt, a pool): when one is a genesis of the co-signed transaction and the contract that
writes it is not among the inputs, refuse it, or print "not written by the contract that writes such objects: it
is in nobody's books". A co-signed genesis is an account, an order, a free stake, an empty roster, a slot.

## The questions

- **A key named in a part that gives nothing.** C13-F2 for the party with no margin and for `payKey`. The proposer
  of a claim and a seat of weight 0: by reading, a claim is a kind only a contract writes (the second fix), and
  `party_keys` lists the taken seats only.
- **Kinds for which naming a key is the wrong question.** A ticket: counted as naming none and warned, which is
  right (C13-N1).
- **The stake rule.** As stated (C13-N2); what it misses is the judge block (C13-F1).
- **What the three changes broke.** Nothing I found: the foreign account is warned, the free stake of the
  co-signer is accepted, the continuation from the co-signer's own coin is refused (`auditC12_cli`).

## Not checked

- The flow against a node; `x-swap --partial` with a hostile account (the writer's pair signatures would have to be
  made again; the account's state is as free there).
- The engine for the two objects of C13-F1 (the contracts' first lines by reading: `checkCross` in `addMargin`,
  `lapseSettle`, `keepAlive`, `propose`; `checkJudge` in every entry of `KpfStake`).
- The other templates under a hostile state (a pair, an order, a roster): the same reading, not run.

## Judgement

Something above Low is new again, and it is the same lesson a third time: `cosign` tells its reader what an object
is by one more property each round (its script, its covenant id, its keys), and each property left out is a way to
have it sign for a UTXO it cannot spend. The list is finite. An object is the co-signer's when its script is a
pinned template, it carries the id this transaction gives it (or none, for a stake), its parameters pass the
template's own checks, it is a kind a co-signed genesis may write, and the co-signer's part of it holds what the
co-signer puts in. The first two are checked; C13-F1 is the third, C13-F2 the last two.
