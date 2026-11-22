# KPF review C16 (`cosign`: the account's terms, and the verdict on the reading)

## Scope and method

Target: `crates/kpf-cli/src/ccmds.rs` (`checked_market`, `account_of_markets`, `terms_of`, the "terms:" line of
`cross_state`), `crates/kpf-cli/src/ocmds.rs` (`cosign_cmd`). Two questions: is any field of an object's state still
neither printed nor compared when `--market` is given, and does `account_of_markets` refuse anything honest or
accept anything hostile. Then the verdict the coordinator asked for.

Files of this round: `crates/kpf-cli/tests/auditC16_cli.rs` (2 tests through the binary: 1 `c16_holds_`, 1
`c16_bounded_`).

Runs: `CARGO_BUILD_JOBS=2 cargo test -p kpf-cli --jobs 2 --test auditC16_cli`: `ok. 2 passed`; `--test auditC15_cli`:
`ok. 1 passed` (with the coordinator's edit: the account with the writer's terms is refused under `--market`, a
market file without genesis facts is not taken).

## Findings

**Nothing above Low is new. One Low, two notes.** The fixes of review C15 hold.

| # | Severity | Statement | Proven by |
|---|---|---|---|
| C16-F1 | Low | A free judge stake is printed as its owner alone. The judge block it is bonded under (whose charges it judges, how old it must be, how long it takes to leave) is not shown and is compared with nothing; `--market` does not reach it. A stake of the co-signer's key under another block's id and the longest unbonding the script accepts (90 days where the roster's judge has 7) is read, not refused, not warned, and line for line the honest stake's printout. The money comes back; it comes back in 90 days and judged nothing meanwhile | `auditC16_cli::c16_bounded_a_free_stake_is_printed_without_its_judge_block` |
| C16-N1 | Note | An order's `activeFrom` is not printed. It gates `fill` and `take` only (the owner's `cancel` and the refund do not wait for it), so a late one makes an order nobody can fill yet, which its owner takes back: no loss | by reading (`KpfOpenC`, `COpen`) |
| C16-N2 | Note | A roster a funder writes is printed as its parameters and its charge line (the judge block's id by 8 bytes, without mu, the burn and the unbonding). Its first epoch, its table and its holds are not printed; the funder puts in the carrier and is warned when no key is its own | `auditC16_cli` (read in the run); by reading |

## (a) What is printed, per kind

| Kind | Printed | Compared under `--market` | Still unprinted |
|---|---|---|---|
| account | both keys, the co-signer's part, each leg (direction, size, entry, market id and roster id whole), the margins, mmr, g, every other term ("terms:" line), the lease and the lapse date | each leg's market and roster, its scale and price limit, all ten terms against `terms_of`; the lease against the clock | a second leg of size 0 (unused by the script) |
| order | side, size, limit, both rates, the least fill, the tip, the expiry, the owner, the market id and roster id whole | the market and roster (its terms are the market's: the id commits to them); the expiry against the clock | `activeFrom` (C16-N1) |
| pair | the range, the size, the entry, both keys, both listings, the lease, the co-signer's part, the market id and roster id whole | the market and roster; the lease against the clock | the makers (written by the pair's own script, never by a funder) |
| free stake | the owner | nothing | the whole judge block (C16-F1) |
| roster | the parameters in words, the charge line | nothing | the first epoch, the table, the holds (C16-N2) |

A pair, a pending part, a claim, an exit, a ticket and a charge are refused when a funder writes them (review C13).

## (b) `account_of_markets`

`c16_holds_an_account_of_two_markets_is_accepted_in_either_order_and_refused_without_both_files`: two pairs of two
markets of one roster (the second with g 0.8 % and a grace of 300 DAA) swapped into an account. With both market
files: accepted, the terms line showing the more careful of each ("a claim lands within 100 DAA", "cured within 300
DAA"). The pairs given in the other order: accepted (`terms_of` is a minimum or a maximum of each term, the legs are
looked up one by one). With one file: refused at the pair of the market that was not given. A one-leg account is
review C15's test. Hostile: a leg in a market not given, or with another scale or price limit than its market's, or
any term that is not `terms_of`, is refused; both legs in one market is an account `x-open` writes and is accepted
with that market's terms; a leg's direction, size and entry are the deal and are printed.

## The verdict

With `--market` for every object, a clock, and the printout read: I find no way left to make `cosign` sign
something that **loses** the co-signer money without a refusal, a warning or a printed term. What I find is one way
to have its money **held**: a free stake under a judge block it is not shown (C16-F1), for at most the 90 days the
script allows. Closing it is the same two lines as before (print the block; let `--roster FILE` say which block is
expected).

That is the state after six rounds on one command, and the manner of getting there is the argument for what the
design note now says: a reading of somebody else's transaction is a list, and a list is complete only until the
next field. It stands for a test network. The form that does not depend on a list is the one planned: the co-signer
rebuilds the transaction from the declared command with its own tool and its own files, and signs only what is
byte for byte the same.

## Not checked

- The flow against a node.
- A roster that names the funder's key as a seat (a join into the writer's parameters: they are printed; not run).
- `x-swap --partial` with hostile pairs (the writer's pair signatures would have to be made; a pair's market is now
  printed and compared).

## Judgement

Nothing above Low is new in this round. The reading does what it now claims, one stake's judge block excepted, and
the claim itself is modest enough: it refuses what it can know is wrong, warns of what leaves, and prints the terms.
