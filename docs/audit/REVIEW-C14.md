# KPF review C14 (the fixes of review C13 in `cosign`, and whether the rest can wait)

## Scope and method

Target: `crates/kpf-cli/src/ocmds.rs` (`object_sound`, `part_of`, `cosign_cmd`). The question put: is there, after
these fixes, a way left to make `cosign` sign something that loses the co-signer money without a refusal or a
warning it would have to ignore, or can the rest wait for "the co-signer rebuilds the transaction itself".

Files of this round: `crates/kpf-cli/tests/auditC14_cli.rs` (2 tests through the binary: 1 `c14_holds_`, 1
`c14_exploit_`).

Runs: `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test auditC14_cli`: `ok. 2 passed`.

## Findings

**One Medium is new, and it is the reason not to wait for everything; two notes.** The fixes of review C13 hold,
and no honest flow is refused.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C14-F1 | Medium | What `cosign` prints of an order and of a pair has no market in it: not its id, not the roster that prices it. An order for the co-signer's key in a market of the writer's own roster (inside every bound, so it is "sound") is printed line for line as the same order in the market the co-signer knows; only the object's address and covenant id differ. The writer fills it and attests what it likes: what the co-signer put into the order is the writer's | `auditC14_cli::c14_exploit_an_order_is_printed_without_its_market` |
| C14-N1 | Note | `cosign` has no clock (`--daa` is not among its arguments): a lease or an order's expiry already past is printed as any date. With `--node` it could say so | by reading (`cosign_cmd`) |
| C14-N2 | Note | An honest `c-assign --partial` is signed under a WARNING (the leaver's payout is "to another address"), and an honest `roster-create --partial` under another (a roster names no key of its funder). Both are right, and both teach the reader that a warning can be ignored | `auditC14_cli::c14_holds_the_honest_partial_flows_are_read_and_signed` |

## C14-F1 (Medium): an order and a pair are printed without their market

The design note leaves the terms to the co-signer's reading, and for an account that works: each leg is printed with
its market's id and its roster's id whole. For the two kinds whose every rule comes from a market, there is nothing
to read:

```
  out 1: 11.2 KAS in an object (covenant id 095c80e9d016):
committee order   kaspatest:pq2a9xn5...
long 10 units, limit 100 KAS, margin rate 1 KAS, min counter rate 0, min fill 10, tip 0, expires at DAA 9000000; owner 2a49...
       yours in it: kaspatest:qq4yn37u...
```

```
committee pair   kaspatest:ppgr0yry...
pair [8.55 .. 12] KAS of 10 units at 10 KAS; the long is liquidated below 9 KAS, the short above 11.42857143 KAS; lease until DAA 6081400
long   4be73f05...   listing ask 0 (min part 0, tip 0)
short  a12a6a03...   listing bid 0 (min part 0, tip 0)
```

The test: the writer makes a roster of its own (`roster-create`) and a market on it (`c-market`), then writes
`c-order --partial` for the co-signer's key and funding, once in the market the co-signer knows and once in its own.
The two printouts are equal but for the object's address, its covenant id and the txid; neither market's id and
neither roster's is in either. No refusal, no warning, "yours in it". `object_sound` checks the market's parameters
against their bounds, and a market on another roster is inside them all. The order's escrow (and, filled, the side's
margin) is priced by whoever holds that roster's quorum. For a pair the same holds in `x-swap --partial` and
`c-assign --partial`: the account's legs name their rosters, the pairs the co-signer takes over name nothing.

**Fix.** Print, for an order and a pair, what an account's leg prints: the market's id and the roster's id whole,
and the market's terms (g, mmr, lateDaa, the fees). Better, let the co-signer say what it expects: `cosign --market
FILE` (repeatable) compares every market an object names with a market file the co-signer holds and refuses one
that is none of them. It breaks nothing: a co-signer without a file reads the ids.

## The question: can the rest wait?

Everything else I tried is a term, and is printed:

| What the writer chooses | What `cosign` shows | Enough for a careful reader |
|---|---|---|
| the roster of an account's leg (its own: prices at will) | "market <id>   priced by the roster <id>", whole | yes, if the reader holds the ids it expects; `--market FILE` would do the comparing |
| an expiry already past (the lapse pays the entries at once) | "lease until DAA X; lapse settle from DAA Y" | yes, with a clock of its own (C14-N1); the lapse pays what the margin line says |
| a pair or an order in a market of the writer's | nothing | **no: C14-F1** |
| an order at a price far from the market's (filled by the writer, the side liquidated at once) | "limit 100 KAS", "margin rate", "min counter rate 0" | yes: the loss is the escrow the reader sees it puts in |
| an account's margins and who holds them | "margin: a .. b ..", "your part of it at its entries: X of Y", the part of 0 warned | yes |
| the agreed prices of a swap (the owner's sides counted for more than they are worth) | the legs "from 11 KAS", the co-signer's parts of the pairs and of the account | yes for a reader that knows the market's price; `cosign` has none to compare |
| a roster with hostile parameters | the roster's words and its charge line; the funder puts in the carrier only | yes |

So: C14-F1 should not wait, it is two printed lines. The rest can wait for "rebuild and compare", which answers
all of them at once, the terms included, provided the declared command is what the co-signer reads and the
comparison is of the whole transaction (outputs, bindings, sequences, lock time, the other party's inputs by
outpoint).

Nothing honest is refused: `x-swap --partial` (three objects, a part in each: 14.5 of 34.7, 26 of 36.2, 20 of 55.2
KAS), `c-assign --partial` (a part of 10 of 30.2 KAS; the leaver's 14 KAS warned of), `c-order --partial`,
`judge-stake --partial` ("no covenant id: its owner's key spends it"), `roster-create --partial` (warned: none of
its keys is the funder's) are all read and signed.

## Not checked

- The refusals of review C13 again beyond the coordinator's `c13_fixed_` test (read; `auditC13_cli` was not rerun by
  me this round).
- A pair in a hostile market through `x-swap --partial` end to end (the writer's pair signatures would have to be
  made for pairs of its own market; the printout of a pair is shown above).
- The flow against a node.

## Judgement

One thing above Low is new: the two kinds a market rules are printed without it. With that printed (or compared),
I see no way left to have `cosign` sign a loss without a refusal, a warning, or a term in plain sight, and the
remaining weight is on the reader, which is where the design note puts it and what rebuilding the transaction on
the co-signer's side will take off it.
