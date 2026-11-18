# First live exercise on testnet-10

Runbook for the repository owner. KPF is a **testnet-only research prototype**: the command line refuses mainnet
addresses, mainnet network ids and any node that does not report `testnet-10`. Nothing here has been run against a
network yet: the tooling was only exercised offline (engine tests, and the CLI against an in-memory UTXO set).

What this exercise covers: every evidence-free path of `KpfOpen` and `KpfPair` (create order, open, list, transfer,
add margin, mutual close, cooperative close, idle settle, plus cancel, refund, open against a signing taker and
close-versus-close). Price evidence (unilateral close, liquidation) is a separate exercise: section 7.

## 1. What must exist before

1. **A testnet-10 node you control or may use**, synced, started with the UTXO index and the JSON wRPC listener, for
   example `kaspad --testnet --netsuffix=10 --utxoindex --rpclisten-json=127.0.0.1:18210` (check the flag names with
   `kaspad --help` of your version; the JSON wRPC port of a testnet node is 18210). `kpf` talks plain `ws://` only (no TLS is
   linked) and has no default node: pass `--node ws://127.0.0.1:18210` every time. For a remote node use an SSH tunnel
   you control. Do not use a node you are not allowed to use, and do not point `kpf` at a public resolver.
2. **A funded key.** `kpf keygen` makes it; fund its `kaspatest:` address with testnet-10 coins from a faucet or by mining.
   Amount: the scenario below locks about 65 KAS in orders at its peak and pays about 0.3 KAS in network fees in total
   (measured offline, table in section 4). **Have 100 KAS on the funder address.** Almost all of it comes back: the
   orders and pairs pay out to the other test keys you create below (bob, carol), which stay in your hands.
3. The build: `CARGO_BUILD_JOBS=2 cargo build -p kpf-cli --jobs 2` gives `target/debug/kpf`. All tests must pass first
   (`CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2`).

## 2. Setup

```sh
export NODE=ws://127.0.0.1:18210
alias kpf=target/debug/kpf
kpf keygen --name alice            # funder, long.   Key files go to ~/.kpf/keys/ (mode 600), never into a repository.
kpf keygen --name bob              # short
kpf keygen --name carol            # second short (the entrant of the transfer)
A=~/.kpf/keys/alice.json; B=~/.kpf/keys/bob.json; C=~/.kpf/keys/carol.json
kpf address --key $A               # fund this kaspatest: address with 100 KAS
kpf daa --node $NODE               # must say: network testnet-10 ... virtual DAA score N
kpf utxos --node $NODE --key $A    # the funding arrives
```

A market block pins the token identity and the parameters. The evidence-free paths never look at the token, so this
exercise uses the placeholder token identity of the example market. A pair on it can never use price evidence. The idle
timeout is set to 3,000 DAA (about 5 minutes) instead of 90 days so that idle settle can be tried live:

```sh
kpf market --idle-daa 3000 --carrier 0.2 > market.json
EXPIRY=$(( $(kpf daa --node $NODE | sed 's/.*score //') + 500000 ))     # orders refundable after about 14 hours
```

Every command below is run twice: first without `--submit` (a dry run: builds, signs, verifies every input in the
script engine, prints who pays and receives what, writes the files, sends nothing), then with `--submit --node $NODE`.
Signing is deterministic, so the second run is the same transaction. Both runs add `--out-dir out/<step>`; the new objects
(`*-out<i>-order.json`, `*-out<i>-pair.json`) are written there and are the inputs of the next steps. The funder's
UTXOs are fetched from the node (`--node`), the largest first; the objects are refreshed from the node as well.

Helper to wait until a new object is on chain (a block of the network has to include the transaction first):

```sh
live() { until kpf status --node $NODE --object "$1" --update | grep -q ': LIVE'; do sleep 2; done; }
```

## 3. The lifecycle

Common flags: `S="--submit --node $NODE"` (leave `$S` empty for the dry run), `F="--funder $A"`. Prices and rates are KAS
per whole token, sizes are whole tokens.

| # | Command | After it, check |
|---|---|---|
| 1 | `kpf create-order --market market.json --side long --size 20 --limit 10.5 --rate 1 --min-counter-rate 1.5 --min-fill 1 --tip 0.01 --expiry $EXPIRY --owner $A $F $S --out-dir out/1` | `live out/1/*-out0-order.json`: LIVE, 20.6 KAS (escrow = 20 x 1.01 + 2 x 0.2 carrier) |
| 2 | same with `--side short --size 10 --limit 9.5 --rate 2 --min-counter-rate 0.5 --owner $B --out-dir out/2` | LIVE, 20.5 KAS |
| 3 | same with `--side short --size 10 --limit 9 --rate 2.2 --owner $C --out-dir out/3` | LIVE, 22.5 KAS |
| 4 | `kpf open --long out/1/*-out0-order.json --short out/2/*-out0-order.json --n 10 --x 10 $F $S --out-dir out/4` | a pair: `kpf inspect out/4/*-pair.json` says size 10, range 9 .. 12 KAS, floor 30.2 KAS; the long order continues with 10 tokens; the short order is filled out (its remainder is paid to bob) |
| 5 | `kpf amend --pair out/4/*-pair.json --side long --price 10 --key $A $F $S --out-dir out/5` | the pair is spent (NOT LIVE) and replaced: long listing 10 |
| 6 | `kpf amend --pair out/5/*-pair.json --side short --price 10.5 --key $B $F $S --out-dir out/6` | short listing 10.5 |
| 7 | wait until the DAA the command prints for a hand-over (the pair's clock start + 4 x minRestDaa, about 4 minutes after step 4 with `minRestDaa` 600; the transaction's lock time is that DAA and `kpf` will not submit it earlier), then `kpf transfer --pair out/6/*-pair.json --side short --n 4 --x 10.2 --entrant-order out/3/*-out0-order.json $F $S --out-dir out/7` | two pairs: a part of 4 tokens (short = carol) and a remainder of 6 (short = bob); bob is paid `q(4, 12 - 10.2)` = 7.2 KAS; carol's order continues with 6 tokens |
| 8 | `kpf add-margin --pair out/7/*-out3-pair.json --side long --delta 0.1 --key $A $F $S --out-dir out/8` | `bl` 9 -> 8.9, value +0.6 KAS |
| 9 | `kpf mutual-close --pair out/8/*-pair.json --n 3 --x 10.2 $F $S --out-dir out/9` | both listings cross: alice is paid `q(3, 10.2 - 8.9)` = 3.9 KAS, bob `q(3, 12 - 10.2)` = 5.4 KAS; a remainder of 3 tokens |
| 10 | `kpf coop-close --pair out/9/*-out1-pair.json --long-pay <half of its value>s --fee-from short --key $A --key $B $S --out-dir out/10` | the pair is spent, nothing replaces it; both keys signed one transaction |
| 11 | wait 3,000 DAA after step 7 (about 5 minutes), then `kpf idle-settle --pair out/7/*-out2-pair.json $F $S --out-dir out/11` | the 4-token part is settled at its entry price 10: alice (long) receives `q(4, 10 - 9)` = 4 KAS, carol the rest |
| 12 | `kpf cancel --order out/7/*-out1-order.json --key $C $S --out-dir out/12` (carol's leftover order, 6 tokens) and `kpf cancel --order out/4/*-out0-order.json --key $A $S --out-dir out/13` (alice's leftover long order, 10 tokens) | each escrow returns to its owner, less the network fee |
| 13 | optional, refund: create a short-lived order (step 1 with `--expiry $(( $(kpf daa --node $NODE | sed 's/.*score //') + 3000 ))`), wait 3,000 DAA, then `kpf refund --order <it> $F $S` | the whole escrow goes to the order's owner; the funder pays the fee; before the expiry the chain rejects it |

`<half of its value>`: read `amount` from the remainder's object file (sompi) and pass `<amount/2>s`. After every
submitted step run `kpf utxos --node $NODE --key $A` and `kpf status --node $NODE --object <old and new files>` and compare with the
"check" column and with the dry-run summary. A step is correct when the node returns the txid the dry run printed and
every object it should create is LIVE with the printed amount.

Open against a signing taker (instead of step 4): `kpf open-taker --order out/1/*-out0-order.json --n 10 --x 10 --rate 1.5 --funder $B $S`: the funder's key becomes
the short owner of the new pair and its own UTXOs pay the short margin. Close-versus-close needs two pairs with
`host.bs >= donor.bs` and `donor.bl <= host.bl`: see `crates/kpf-cli/tests/lifecycle.rs`
(`cancel_refund_open_taker_and_close_versus_close`) for a working pair of pairs and the exact flags.

## 4. Measured cost (offline, rate 100 sompi per gram, the relay floor)

| step | fee (KAS) | compute mass | storage mass |
|---|---|---|---|
| create order | 0.0021 | 2,083 | about 1,900 |
| open | 0.0567 | 33,302 | 53,362 |
| amend | 0.0241 | 16,486 | 671 |
| transfer | 0.0524 | 32,918 | 7,779 |
| add margin | 0.0241 | 16,384 | 1,385 |
| mutual close | 0.0242 | 15,945 | 7,883 |
| cooperative close | 0.0239 | 14,879 | 10 |
| idle settle | 0.0240 | 14,279 | 2,736 |

Script units are small (the largest input uses about 280,000, every input commits the exact budget). The mass is dominated
by the size of the redeem scripts (a pair's script is 12.1 KB since review 3 and is revealed on every spend; the table was measured at 11.6 KB). If blocks are contended the
node may not relay at the floor: raise `--fee-rate` or add `--priority` (which also pays for the storage mass).

## 5. If something fails

- *"verification failed"* on a dry run: nothing was written or sent. The message names the input and the engine error.
- *"UTXO ... is not at the node"*: the earlier transaction is not in a block yet (use `live`), or it was spent.
- A node error on submit (`node rejected the request: ...`): copy it into the report; do not edit the tooling or the
  contracts to get around it. In particular note any rejection about transaction standardness, mass or script size.
- Funding errors: `Insufficient` means the funder's UTXOs do not cover the outputs and the fee; the change output must
  be at least 0.02 KAS.

## 6. What is not possible on a live network yet

- **Indexing other people's orders.** A pair or an order is a P2SH output: its state is revealed only when it is spent,
  and no indexer for KPF exists. `kpf` can follow an object only through the object file the builder wrote
  (outpoint, covenant id and redeem script); it cannot discover orders or pairs of other people. A counterparty has to
  send you its object file. There is no order book and no matching.
- **An executor.** Nobody watches the chain to fill orders, open pairs between orders, or collect tips. Every transaction
  of this exercise is built and funded by hand by the owner.
- **Multi-party signing across machines.** `coop-close` needs both owners' key files on the machine that builds it.
  There is no partial-signature exchange yet.
- **Mainnet**: refused on purpose.

## 7. Price evidence: unilateral close, liquidation, liquidation by transfer

The three paths of `KpfPair.onEvidence` (P5, P6, P7) need a **real KOB order**: a resting `KobAsk` or `KobBid` of the
market's token is filled in the same transaction, and the pair reads its quote (DESIGN 3.3). `kpf close`, `kpf liquidate`
and `kpf liquidate-transfer` build that whole transaction: the pairs, KOB's order, its counterparty and the token
program's inputs and outputs, all verified input by input in the script engine before anything is written. They have been
exercised only offline (engine tests and the CLI against an in-memory chain with a KOB book); nothing has been run on a
network. This section is the exercise that would change that.

### 7.1 What it needs

1. **A KCC-20 token on testnet-10 of KOB's reference program** `kcc20-ref-3x3` (template hash
   `173ca6a796c2c05f171c31b9a73aaca161a3226a9e8f57d2f3d833ff18dbe41b`, prefix 1 byte, suffix 2,802 bytes, 112-byte
   state; the program `kob token create --program 3x3` issues). The tool spends only this program: for any other
   (`KCC20Ref_4x5`, `_8x8`, `KCC20PublicMint` / kUSDT, KRON) `kpf evidence` says so and nothing is built. You need its
   covenant id and its extension commitment.
2. **A market file for that token.** Start from `kpf market --idle-daa 3000 --carrier 0.2 > market.json` and replace the
   token identity: `tokenCovId`, `extensionCommitment` (hex, 32 bytes), `tokenTplHash` =
   `173ca6a796c2c05f171c31b9a73aaca161a3226a9e8f57d2f3d833ff18dbe41b`, `tplPrefixLen` = 1, `tplSuffixLen` = 2802, and
   `scale` = 10^decimals of the token (and `minSize` in base units). `kpf market --check market.json` validates it.
   `minRestDaa` (default 600, about one minute) is how long an evidence order must have rested.
3. **Resting KOB orders of that token whose templates are byte-identical to the ones pinned in `KpfPair`.** The pair
   authenticates its evidence by the template hash, the prefix length and the suffix length of the order's redeem script:

   | order | template hash | prefix | suffix | state |
   |---|---|---|---|---|
   | `KobAsk` | `070bb3b2425cc800c02e2e9465241a8b94abdce93e712fd3baf19474b63b887c` | 1 | 1,407 | 276 B |
   | `KobBid` | `dfcd78f15cdadccb7db6f8094cb42421a409642a8b28041518a6bd2ec54bd5ef` | 1 | 1,343 | 285 B |

   These are the hashes of KOB at commit `ed16148` (`vendor/kob/contracts/orders/KobCondAsk.ctor.json` pins the same
   values). The orders must be plain: GTC (or IOC/FOK whose kill falls after the rest window), a fixed quote (no decay or
   rise), tip and `interval` as KOB allows, `expiryDaa` at least the rest window after the order was created, and an ask
   must be backed by its custody UTXO. A bid must be funded for at least `k` times the parts it serves.
4. **What the executor (the funder) pays or holds.** The evidence is an actual fill:
   - *ask evidence* (liquidate a long; a short closes): the executor **buys** `max(k x sum(n), the ask's minimum fill)`
     base units at the ask's price with its own KAS and keeps the tokens. At the default `k` = 1 it pays about
     `n x ask price` for a part `n`; with 10 tokens at 9.4 KAS that is 94 KAS (more than the pair is worth). It receives the
     tokens as a holder UTXO (a file `*-token.json` is written).
   - *bid evidence* (liquidate a short; a long closes): the executor **sells** that many tokens into the bid and receives the
     KAS, so it needs a token UTXO of its own (P2PK owner, the market's extension commitment) with at least that amount:
     pass it with `--seller TOKEN.json`. Tokens the ask evidence bought can serve here. Filling a resting ask against the
     bid instead (no tokens needed) would add three inputs and is not built.
   - Both pay the network fee (section 7.4) and lock about 0.2 KAS per token output the transaction creates.

### 7.2 Checking an order before anything is built

An order file (`ORDER.json`) is the order's outpoint, amount, DAA, covenant id and **redeem script** (the KOB order's
script is not on chain until it is spent: take it from KOB's indexer or from the order's creation record, prefix + state +
suffix of the template above) and, for an ask, its custody UTXO. `kpf kob-file` writes it and, with `--node`, reads the
UTXO facts from the node:

```sh
kpf kob-file --redeem <custody redeem hex> --outpoint <txid>:<i> --node $NODE --out custody.json
kpf kob-file --redeem <ask redeem hex>     --outpoint <txid>:<i> --node $NODE --custody custody.json --out ask.json
kpf kob-file --redeem <bid redeem hex>     --outpoint <txid>:<i> --node $NODE --out bid.json
kpf kob-file --redeem <your token redeem hex> --outpoint <txid>:<i> --node $NODE --out seller.json     # tokens to sell
kpf evidence --order ask.json --pair out/4/*-pair.json --node $NODE        # or --daa N instead of --node
```

`kpf evidence` decodes the order, checks it against the pair's market the way `KpfPair` does (token covenant id, scale,
extension commitment, token program, not decaying, interval, custody, the rest window of DESIGN 4.5b: soft expiry, IOC/FOK
kill and the 90-day idle limit) **on the pair's evidence clock** (DESIGN 4.5c: the rest counts only from
`max(exposed, S)`, `S` the pair's clock start; the command prints the clock start, `since`, and the DAA from which a hand-over
is allowed) and prints, in words, why an order cannot serve: `CANNOT SERVE` (never, as it is), `NOT YET` (it must rest
until DAA N) or `DEAD` (it is past its soft expiry, its IOC/FOK kill or 90 days idle at the DAA the transaction would
land: conforming matchers no longer fill it and no book shows it). `kpf evidence`, `close`, `liquidate` and
`liquidate-transfer` refuse a dead order; `--allow-dead-evidence` accepts one, for sweeping tests only. Order and object
files must carry `daa` (a file without it is refused, not read as 0: refresh it with `--node` or `kpf status --update`). It also lists what the quote lets this pair do (the long closes at a bid, the short
at an ask, who is liquidatable at that quote and why not). It exits non-zero unless the order is usable (without a DAA only the time-free conditions are judged).

**If the live KOB deployment uses different templates** the check says so: *the script (N bytes) is not an instance of the
KobAsk / KobBid template pinned in KpfPair ... its template hash is H against the pinned P*. Then no KPF pair of this
build can use that order as evidence: the hashes are constants of `KpfPair` (a constructor argument of the compiled
contract), and `KpfOpen` pins the `KpfPair` template, so using other KOB templates means recompiling both contracts and a
new market, which is a contract change this repository does not make here. Do not patch the tooling around it: record `H`,
the order's size and the KOB commit it came from and report them. Pairs that already exist keep only the evidence-free
exits of section 3.

### 7.3 The exercise

Use the pair of section 3, step 4 (size 10 tokens at 10 KAS, range 9 .. 12 KAS) on the market of 7.1, with `F` and `S` as
there and `--out-dir out/<step>`; dry run first, then `$S`. Time matters: `minRestDaa` DAA after the evidence order was
created (and, for an ask, after its custody), **and after the pair's own clock start** (its UTXO's DAA for a pair that
just changed hands), its quote counts; an order that rested before the pair was opened or transferred counts only from
then, and only if it is still live; before that `kpf evidence` says `NOT YET`, and a built
transaction carries a lock time ahead of the chain, which `kpf` will not submit (`--daa` or `--node` tells it the DAA).

| # | Command | After it, check |
|---|---|---|
| 20 | short closes at a resting ask (10.5 KAS): `kpf close --pair P.json --side short --n 4 --evidence ask.json --key $B $F $S` | the short is paid `q(4, 12 - 10.5)` = 6 KAS at the pair's last output; the long receives the rest of the part (`q(4, 3)` - 6 = 6 KAS); the pair continues with 6 tokens; the ask continues (or ends if it was exactly filled); the executor holds 4 tokens bought at 10.5 KAS |
| 21 | long closes at a resting bid (10 KAS): `kpf close --pair P.json --side long --n 4 --evidence bid.json --seller seller.json --key $A $F $S` | the long is paid `q(4, 10 - 9)` = 4 KAS; the executor's tokens went to the bid's maker and the executor received `4 x 10` KAS |
| 22 | the book falls to an ask of 9.4 KAS (the long's trigger is 9.47 KAS: `kpf quote --pair P.json` prints it): `kpf liquidate --pair P.json --evidence ask.json $F $S` | the long receives nothing; the short receives the part's value less the fee (`kpf quote` shows it); up to 8 pairs of distinct covenant ids go in one transaction: repeat `--pair` |
| 23 | the same, sold to a resting long order: `kpf liquidate-transfer --pair P.json --entrant-order out/.../order.json --evidence ask.json --n 4 $F $S` (`--x` defaults to the best price the entrant allows while it stays healthy at the ask: 9.93 KAS for a rate of 1 KAS; an entrant too thin for the quote is refused) | the loser's 4 tokens belong to the entrant's owner at its price, with a bound the same ask does not liquidate; the short keeps its position (on a fresh evidence clock) and is paid the entrant's payment less the fee |

After a transaction the evidence order that continued (`*-ask.json` with its custody, `*-bid.json`) and the executor's
tokens (`*-token.json`) are written beside the new pairs. The continuation is a new UTXO: it must rest again before it is
evidence. The pieces of one split (a part and its remainder) share a covenant id and a pair must be the only input of
its id, so liquidate them in separate transactions. A part that changed hands starts a fresh evidence clock: it cannot be
liquidated before an order has rested `minRestDaa` on it, and it cannot be transferred again (nor re-paired) before
`S + 4 * minRestDaa`.

### 7.4 Measured cost (offline, relay floor 100 sompi per gram)

Since review 3 the pair's script is 12.1 KB (11.6 KB before); the figures below were measured before that change and
are about 0.6 KB and 1 to 3 % of compute mass larger now per pair (the engine metrics: 8 liquidations on one evidence,
104,934 bytes and compute mass 130,644; `kpf_metrics_tests`).

Every transaction contains the 11.6 KB redeem script of each pair, KOB's order script and the token program script
(2.9 KB) with the custody or the seller's tokens. The storage mass (KIP-9) comes from the small token outputs; the block
limit is 500,000 and a token output of 0.2 KAS uses 200,000, which is why the builders keep their own outputs generous.

| shape | size (bytes) | compute mass | storage mass | fee (KAS) |
|---|---|---|---|---|
| P5, long on a bid (whole pair) | 17,433 | 23,883 | 400,022 | 0.0349 |
| P5, long on a bid (400 of 1,000) | 17,520 | 25,340 | 400,064 | 0.0350 |
| P5, short on an ask (ask larger than the fill) | 17,424 | 23,674 | 404,012 | 0.0348 |
| P5, short on an ask filled out | 17,144 | 22,154 | 200,012 | 0.0343 |
| P6, long on an ask | 17,359 | 22,609 | 414,004 | 0.0347 |
| P6, short on a bid | 17,316 | 22,406 | 400,009 | 0.0346 |
| P6, block of 2 pairs | 29,184 | 35,854 | 424,006 | 0.0584 |
| P6, block of 4 pairs | 52,834 | 62,344 | 444,012 | 0.1057 |
| P6, block of 8 pairs | 100,134 | 115,324 | 484,024 | 0.2003 |
| P7, whole pair | 31,384 | 39,714 | 404,073 | 0.0628 |
| P7, part (remainder kept) | 31,471 | 41,171 | 404,131 | 0.0629 |

The largest input in these transactions is the pair (43,000 to 313,000 script units in the shapes above; every input commits the exact
budget). The block of 8 uses 97 % of the storage mass a block holds: it needs every token output at 0.2 KAS or more.

### 7.5 What is not built, and why

- **Other token programs.** A holder's witness and the slot limits differ per program (KRON, `KCC20PublicMint` with its
  compiler context); only KOB's reference 3 / 3 program is spent.
- **Seller tokens that are not P2PK-owned** (P2PKH, P2SH or covenant-owned holders): the leader witness built is the P2PK
  one (`0x00` + signature).
- **A resting opposite order as the counterparty** of a bid evidence (the executor's own tokens are used instead).
- **An IOC ask filled in part**: its unsold tokens must return to the maker at the custody's own index; the builder takes
  such an ask entirely (an ask that ends with its fill).
- **Evidence from conditional, if-done, pair, decaying or rising orders**: `KpfPair` itself refuses them (plain, fixed
  quote only).
- **Several pieces of one split in one block**: they share a covenant id and a pair must be the only input of its id.
- **Mixed blocks from the command line**: the library accepts any mix that one evidence serves (a short closing and a long
  being liquidated at one ask); `kpf` exposes one path per command.
