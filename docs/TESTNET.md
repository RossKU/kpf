# First live exercise on testnet-10

Runbook for the repository owner. KPF is a **testnet-only research prototype**: the command line refuses mainnet
addresses, mainnet network ids and any node that does not report `testnet-10`. Nothing here has been run against a
network yet: the tooling was only exercised offline (engine tests, and the CLI against an in-memory UTXO set).

What this exercise covers: every evidence-free path of `KpfOpen` and `KpfPair` (create order, open, list, transfer,
add margin, mutual close, cooperative close, idle settle, plus cancel, refund, open against a signing taker and
close-versus-close). Price evidence (unilateral close, liquidation, in two steps: propose, then challenge, cure or
finalize) is a separate exercise: section 7.

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
| 7 | wait until the DAA the command prints for a hand-over (the pair's clock start + 4 x minRestDaa, 200 DAA, about 20 seconds after step 4 with `minRestDaa` 50; the transaction's lock time is that DAA and `kpf` will not submit it earlier), then `kpf transfer --pair out/6/*-pair.json --side short --n 4 --x 10.2 --entrant-order out/3/*-out0-order.json $F $S --out-dir out/7` | two pairs: a part of 4 tokens (short = carol) and a remainder of 6 (short = bob); bob is paid `q(4, 12 - 10.2)` = 7.2 KAS; carol's order continues with 6 tokens |
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
by the size of the redeem scripts (a pair's script is now 12,155 bytes, with the two-step evidence of DESIGN 4.5d,
and is revealed on every spend; the table was measured at 11.6 KB). If blocks are contended the
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
  send you its object file. There is no order book and no matching. The same holds for pending parts (DESIGN 4.5d):
  a claim against your pair is visible to `kpf watch` only from the proposal's object file (section 7.3).
- **An executor.** Nobody watches the chain to fill orders, open pairs between orders, or collect tips. Every transaction
  of this exercise is built and funded by hand by the owner.
- **Multi-party signing across machines.** `coop-close` needs both owners' key files on the machine that builds it.
  There is no partial-signature exchange yet.
- **Mainnet**: refused on purpose.

## 7. Price evidence: propose, challenge, cure, finalize

Price evidence works in two steps (DESIGN 4.5d). A unilateral close (P5) or a liquidation (P6) is first **proposed**
(`KpfPair.propose`): the part moves into a `KpfPending` UTXO with a bond and nothing is paid out. For a window
`W` (`challengeDaa`, 600 DAA, 60 seconds, on the testnet market) anyone may **challenge** it with fresh
counter-evidence, and the loser of a liquidation may **cure** it; after the window anyone may **finalize** it. Every
transaction that reads a quote needs a **real KOB order**: a resting `KobAsk` or `KobBid` of the market's token is filled
in the same transaction (DESIGN 3.3). `kpf propose-close`, `kpf propose-liquidate` and `kpf challenge` build that whole
transaction: the pairs or pending parts, KOB's order, its counterparty and the token program's inputs and outputs;
`kpf cure` and `kpf finalize` need no evidence. Every input is verified in the script engine before anything is
written. The one-step commands `close`, `liquidate` and `liquidate-transfer` are gone (liquidation by transfer, P7, is
removed). All of this has been exercised only offline (engine tests and the CLI against an in-memory chain with a KOB
book); nothing has been run on a network. This section is the exercise that would change that.

What the steps mean for each party:
- **Bond.** The proposer posts `B = max(bondMin, 0.02 KAS, q(n, (BS − BL) · bondBps / 10^4))`: 300 bps of the part's
  range, at least 5 KAS on the testnet market (for the 10-token pair of section 3, range 30 KAS, that is the 5 KAS
  floor; 90 KAS for a 1,000-token pair of the same range). For a part of a pair (`n < s`) the proposer also funds the
  part's carrier (0.2 KAS). **Refund** = the bond, plus that carrier when it was funded.
- **Challenge.** A counter order of the opposite kind (a bid against a long's liquidation or a short's close, an ask
  against a short's liquidation or a long's close) that has rested `minRestDaa` counted from the pending part's own DAA
  `P` (an older order counts from `P`), stays fillable for the evidence life after its rest, is filled for `k_c` times
  the parts, and lies beyond the claim by the margin `g` (2 %): for a liquidation relative to the loser's trigger, for
  a close relative to the closer's quote. The part returns to its pair, unlisted (DESIGN 4.5e: list again with `kpf amend`), with its evidence clock unchanged (`since` = `S`, the pair's clock start at the proposal), so a refuted claim gains nobody rest time; the bond goes to
  the challenge's funder. A refuted close is cancelled, not re-settled.
- **Cure** (liquidations only). The threatened owner signs, moves its bound exactly to the cure bound (the nearest
  bound at which no quote within the margin `g` of the proposed one triggers, DESIGN 4.5e) and pays the margin that takes; the proposer gets its refund,
  no fee. The cured pair keeps its evidence clock and comes back unlisted.
- **Finalize.** Anyone, once the pending part is `challengeDaa` old. A close: the closer receives its payoff at the
  quote plus its refund, the other party the rest. A liquidation: **forfeit and keep**: the winner now owns both
  sides of the part (the loser's bound moved inward by the fee; a part with one owner cannot be liquidated, DESIGN 4.5e), and the proposer receives the bond and the keeper
  fee. The winner then either closes the part with its own key twice (`kpf coop-close --pair FORFEITED.json
  --long-pay 0 --key $B --key $B $S` for a short winner) or sells the forfeited side by an ordinary listing and
  transfer (`kpf amend`, then `kpf transfer`, allowed from `S' + 4 * minRestDaa`, `S'` the forfeited part's DAA).
- The window is not a deadline: a challenge or a cure is accepted until a finalize lands.

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
   `minRestDaa` (default 50 DAA, 5 seconds) is how long an evidence order must have rested. The two-step parameters
   (DESIGN 4.5d) are in the same block: `bondBps` 300, `bondMin` 500,000,000 sompi (5 KAS), `kcBps` 10,000 (`k_c` = 1),
   `gBps` 200, `challengeDaa` 600 (the window `W`, 60 seconds), `minEvidenceLife` 864,000 (one day, `L`); the hand-over limit `S + 4 * minRestDaa` is then 200 DAA (20 seconds); `kpf market --check`
   refuses `W < 2 * minRestDaa` (DESIGN 4.5e), `bondMin` above 10,000 KAS and `L + minRestDaa > 77,760,000`.
3. **Resting KOB orders of that token whose templates are byte-identical to the ones pinned in `KpfPair`.** The pair
   authenticates its evidence by the template hash, the prefix length and the suffix length of the order's redeem script:

   | order | template hash | prefix | suffix | state |
   |---|---|---|---|---|
   | `KobAsk` | `070bb3b2425cc800c02e2e9465241a8b94abdce93e712fd3baf19474b63b887c` | 1 | 1,407 | 276 B |
   | `KobBid` | `dfcd78f15cdadccb7db6f8094cb42421a409642a8b28041518a6bd2ec54bd5ef` | 1 | 1,343 | 285 B |

   These are the hashes of KOB at commit `ed16148` (`vendor/kob/contracts/orders/KobCondAsk.ctor.json` pins the same
   values). The orders must be plain: GTC (an IOC/FOK order is killed 600 DAA after it starts, so with the testnet
   evidence life of one day it never serves), a fixed quote (no decay or rise), tip and `interval` as KOB allows,
   `expiryDaa` at least the rest window plus the evidence life after the order's exposure starts, and an ask must be
   backed by its custody UTXO. A bid must be funded for at least `k` (`k_c` for a challenge) times the parts it serves.
4. **What the executor (the funder) pays or holds.** The evidence is an actual fill:
   - *ask evidence* (liquidate a long; a short closes; challenge a short's liquidation or a long's close): the executor **buys** `max(k x sum(n), the ask's minimum fill)`
     base units at the ask's price with its own KAS and keeps the tokens. At the default `k` = 1 it pays about
     `n x ask price` for a part `n`; with 10 tokens at 9.4 KAS that is 94 KAS (more than the pair is worth). It receives the
     tokens as a holder UTXO (a file `*-token.json` is written).
   - *bid evidence* (liquidate a short; a long closes; challenge a long's liquidation or a short's close): the executor **sells** that many tokens into the bid and receives the
     KAS, so it needs a token UTXO of its own (P2PK owner, the market's extension commitment) with at least that amount:
     pass it with `--seller TOKEN.json`. Tokens the ask evidence bought can serve here. Filling a resting ask against the
     bid instead (no tokens needed) would add three inputs and is not built.
   - Both pay the network fee (section 7.4) and lock about 0.2 KAS per token output the transaction creates.
   - A proposal's funder also posts the bond (and, for a part, its carrier) for at least `W`; a liquidation's proposer
     (`--proposer`, default the funder) gets it back with the keeper fee at finalize, a closer gets it back with its
     payout. A challenge's funder receives the bonds of the parts it refutes. A cure's funder pays the margin.

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
kpf evidence --order bid.json --pending out/24/*-pending.json --node $NODE # does this order refute a pending claim
```

`kpf evidence` decodes the order, checks it against the pair's market the way `KpfPair` does (token covenant id, scale,
extension commitment, token program, not decaying, interval, custody, the rest window of DESIGN 4.5b: soft expiry, IOC/FOK
kill and the 90-day idle limit, each at or after the rest's end plus the evidence life `L`, DESIGN 4.5d) **on the pair's
evidence clock** (DESIGN 4.5c: the rest counts only from `max(exposed, S)`, `S` the pair's clock start; the command
prints the clock start, `since`, and the DAA from which a hand-over is allowed) and prints, in words, why an order
cannot serve: `CANNOT SERVE` (never, as it is), `NOT YET` (it must rest until DAA N) or `DEAD` (it is past its soft
expiry, its IOC/FOK kill or 90 days idle at the DAA the transaction would land, or within `L` of it: conforming
matchers no longer fill it and no book shows it). `kpf evidence`, `propose-close`, `propose-liquidate` and `challenge`
refuse a dead order; `--allow-dead-evidence` accepts one, for sweeping tests only. Order and object files must carry
`daa` (a file without it is refused, not read as 0: refresh it with `--node` or `kpf status --update`). It also lists
what the quote lets this pair do (the long closes at a bid, the short at an ask, who is liquidatable at that quote and
why not). It exits non-zero unless the order is usable (without a DAA only the time-free conditions are judged).

With `--pending W.json` instead of `--pair` it judges the order as **counter-evidence** against that pending claim:
the opposite kind, a quote beyond the claim by the margin `g` (it prints the refutation bound), rested `minRestDaa`
from the proposal's DAA `P` and fillable `L` after; it says `REFUTES the claim now` with the `kpf challenge` command,
or why not, and exits non-zero unless the order refutes it.

`kpf quote --pair P.json` prints, besides the payoffs and the triggers, the two-step figures of a claim on the part
(`--n T` for a part): the bond and the refund, the window, the bid that refutes the long's liquidation and the ask that
refutes the short's (at the extreme quotes that trigger), how a close at a quote is refuted, and the cure bounds.
`kpf inspect W.json` decodes a pending part: the claim, the bond, refund, fee and proposer, the DAA from which it can
be finalized, its refutation bound and, for a liquidation, its cure bound.

**If the live KOB deployment uses different templates** the check says so: *the script (N bytes) is not an instance of the
KobAsk / KobBid template pinned in KpfPair ... its template hash is H against the pinned P*. Then no KPF pair of this
build can use that order as evidence or counter-evidence: the hashes are constants of `KpfPair` and `KpfPending`
(constructor arguments of the compiled contracts), `KpfPair` pins the `KpfPending` template and `KpfOpen` pins the
`KpfPair` template, so using other KOB templates means recompiling the three contracts and a new market, which is a contract change this repository does not make here. Do not patch the tooling around it: record `H`,
the order's size and the KOB commit it came from and report them. Pairs that already exist keep only the evidence-free
exits of section 3.

### 7.3 The exercise

Use the pair of section 3, step 4 (size 10 tokens at 10 KAS, range 9 .. 12 KAS) on the market of 7.1, with `F` and `S` as
there and `--out-dir out/<step>`; dry run first, then `$S`. `P.json` is the pair's latest object file. Time matters: an
evidence order's quote counts `minRestDaa` DAA after the order was created (and, for an ask, after its custody), **and
after the pair's own clock start** (its UTXO's DAA for a pair that just changed hands or was forfeited; a restored or cured part keeps the clock of the pair it came from), and
only while it stays fillable for the evidence life `L` (one day) after that; a counter order counts `minRestDaa` after
the pending part's own DAA `P` (an older order counts from `P`). Before that `kpf evidence` says `NOT YET`, and a built
transaction carries a lock time ahead of the chain, which `kpf` will not submit (`--daa` or `--node` tells it the DAA).
A proposal writes the pending part as `*-out<i>-pending.json` beside the remainder pair; that file is the input of
every later step.

| # | Command | After it, check |
|---|---|---|
| 20 | short proposes to close at a resting ask (10.5 KAS): `kpf propose-close --pair P.json --side short --n 4 --evidence ask.json --key $B $F $S --out-dir out/20` | a pending part of 4 tokens holding `q(4, 3)` + 0.2 carrier + 5 KAS bond = 17.2 KAS (refund 5.2 KAS) and the remainder pair of 6 tokens; nothing is paid out; the ask continues (or ends if it was exactly filled); the executor holds 4 tokens bought at 10.5 KAS; `kpf inspect out/20/*-pending.json` prints the DAA from which it can be finalized and the refuting bid (`>= 10.71` KAS) |
| 21 | wait `challengeDaa` (600 DAA, 60 seconds), then `kpf finalize --pending out/20/*-pending.json $F $S --out-dir out/21` | the short is paid `q(4, 12 - 10.5)` = 6 KAS plus its refund 5.2 KAS; the long receives the rest, 6 KAS; earlier the command says `can be finalized only from DAA N` |
| 22 | long proposes to close at a resting bid (10 KAS): `kpf propose-close --pair P.json --side long --n 4 --evidence bid.json --seller seller.json --key $A $F $S --out-dir out/22`, then after the window `kpf finalize --pending out/22/*-pending.json $F $S` | at the proposal the executor's tokens go to the bid's maker and it receives `4 x 10` KAS; at finalize the long is paid `q(4, 10 - 9)` = 4 KAS plus its refund, the short the rest; a fresh ask `<= 9.8` KAS would have refuted it |
| 23 | the book falls to an ask of 9.4 KAS (the long's trigger is 9.47 KAS: `kpf quote --pair P.json` prints it, with the bond, the refuting bid and the cure bound): `kpf propose-liquidate --pair P.json --evidence ask.json $F $S --out-dir out/23` (`--proposer KEY` to be paid elsewhere; up to 8 pairs of distinct covenant ids: repeat `--pair`, with `--n T` per pair) | figures for a whole 10-token pair on 9 .. 12 (one opened again as in section 3, step 4): a pending part of 30.2 + 5 = 35.2 KAS, fee 0.47 KAS, cure bound 8.7514 KAS, refuted by a bid `>= 9.66315790` KAS |
| 24 | `kpf watch --pair P.json --scan out/23 --order bid.json --node $NODE` | `PENDING ... it threatens the long`, the DAA from which it can be finalized with the time left in DAA and seconds (10 DAA per second), and for each given order `REFUTES IT` with the exact `kpf challenge` command, or why it does not; also the `kpf cure` and `kpf finalize` commands |
| 25a | refute it (if the quote is stale): `kpf evidence --order bid.json --pending out/23/*-pending.json --node $NODE`, then `kpf challenge --pending out/23/*-pending.json --evidence bid.json --seller seller.json $F $S --out-dir out/25` | the part returns to its pair, unlisted (evidence clock unchanged: `since` = the pair's clock start `S`) with 30.2 KAS; the funder (the challenger) keeps the 5 KAS bond; several pending parts on one bid: repeat `--pending` |
| 25b | or the long cures it: `kpf cure --pending out/23/*-pending.json --key $A $F $S --out-dir out/25` | the pair returns with `bl` 8.7514 (2.486 KAS more margin, paid by the funder; the command states the total), unlisted, with its old clock; the proposer is paid its refund, 5 KAS; no ask at or above 9.212 KAS (within `g` of 9.4) liquidates it |
| 25c | or nobody acts: after the window `kpf finalize --pending out/23/*-pending.json $F $S --out-dir out/25` | forfeit and keep: a pair with both keys bob's, `bl` 9.047, holding 35.2 - 5 - 0.47 = 29.73 KAS; the proposer receives 5.47 KAS (bond and fee); the long nothing; the part, both sides bob's, can no longer be liquidated |
| 26 | after 25c, bob ends the forfeited part: `kpf coop-close --pair out/25/*-pair.json --long-pay 0 --key $B --key $B $S` (or sells the long side: `kpf amend --side long ...`, then `kpf transfer` from `S' + 4 * minRestDaa`, `S'` the forfeited part's DAA) | the whole value goes to bob, less the network fee |

After a transaction the evidence order that continued (`*-ask.json` with its custody, `*-bid.json`) and the executor's
tokens (`*-token.json`) are written beside the new objects. The continuation is a new UTXO: it must rest again before it
is evidence. The pieces of one split (a part and its remainder, and a pending part and its remainder) share a covenant
id and each must be the only input of its id, so act on them in separate transactions. While a part is pending its
pair paths are refused (a second proposal, amend, add margin, idle settle, transfer, close-versus-close); the
remainder is an ordinary pair. A part that changed hands or was forfeited starts a fresh
evidence clock: it cannot be liquidated before an order has rested `minRestDaa` on it, and it cannot be transferred
again (nor re-paired) before `S + 4 * minRestDaa`. A cured or restored part keeps its clock and comes back unlisted.

**`kpf watch` cannot discover pending parts from the node.** A node lists UTXOs by address only, not by covenant id,
and a pending part's address depends on its whole state, so `watch` finds pending parts only from object files
(`--pending FILE`, or `--scan DIR`, e.g. the proposer's out-dir) and refreshes them from the node. A party whose pair is
claimed by someone else learns of it only if it gets that file; discovery on a live network needs an indexer (KOB's,
or a KPF one), which does not exist (section 6).

### 7.4 Measured cost (offline, relay floor 100 sompi per gram)

Every transaction contains the 12,155-byte redeem script of each pair it spends, or the 3,203-byte script of each
pending part, plus KOB's order script and the token program script (2.9 KB) with the custody or the seller's tokens
when it reads a quote. The storage mass (KIP-9) comes from the small token outputs; the block limit is 500,000 and a
token output of 0.2 KAS uses 200,000, which is why the builders keep their own outputs generous.

The command line (`crates/kpf-cli/tests/lifecycle.rs`, the evidence lifecycle):

| step | fee (KAS) | compute mass | storage mass |
|---|---|---|---|
| propose-close | 0.04129 | 30,215 | 400,059 (two token outputs of the bid fill) |
| finalize (close) | 0.00727 | 5,715 | |
| propose-liquidate | 0.04097 | 29,884 | 404,022 |
| challenge | 0.01783 | 13,815 | 400,027 |
| cure | 0.00747 | 6,826 | |
| finalize (forfeit) | 0.00734 | 5,760 | |
| coop-close of the forfeited part | 0.02499 | 15,053 | |

The engine metrics (`kpf_metrics_tests`; parts of 400 tokens; limits compute 500,000, storage 500,000, 250,000 bytes):

| transaction | inputs | script units | compute mass | storage mass | bytes | fee (sompi) |
|---|---|---|---|---|---|---|
| propose close long at a bid | 4 | 705,225 | 29,452 | 203,474 | 20,452 | 4,090,400 |
| propose close short at an ask | 4 | 646,543 | 28,486 | 200,056 | 20,356 | 4,071,200 |
| propose liquidate long at an ask | 4 | 779,010 | 29,721 | 200,056 | 20,291 | 4,058,200 |
| propose liquidate short at a bid | 4 | 837,770 | 30,687 | 203,205 | 20,387 | 4,077,400 |
| challenge (ask counter) | 4 | 214,979 | 12,086 | 200,035 | 8,626 | 1,725,200 |
| challenge (bid counter) | 4 | 273,660 | 13,052 | 203,255 | 8,722 | 1,744,400 |
| finalize close | 2 | 107,236 | 5,715 | 31 | 3,635 | 727,000 |
| finalize forfeit | 2 | 107,084 | 5,760 | 215 | 3,670 | 734,000 |
| cure | 2 | 207,967 | 6,826 | 308 | 3,736 | 747,200 |
| 8 proposals on one ask (8 x 500 tokens) | 11 | 5,122,461 | 183,660 | 200,408 | 125,550 | 25,110,000 |
| 8 challenges on one bid | 11 | 457,008 | 40,646 | 203,890 | 32,326 | 6,465,200 |

The largest input is a proposing pair (457,197 to 619,781 script units; every input commits the exact budget). A block
of 8 proposals fits (the scan's maximum). Before the two-step change 8 one-step liquidations used 1,651,150 script
units, compute mass 130,644 and 104,934 bytes. Besides the fees, a proposal locks its bond for at least `W`: 5 KAS for
the pairs of this exercise, 300 bps of the part's range in general.

### 7.5 What is not built, and why

- **Other token programs.** A holder's witness and the slot limits differ per program (KRON, `KCC20PublicMint` with its
  compiler context); only KOB's reference 3 / 3 program is spent.
- **Seller tokens that are not P2PK-owned** (P2PKH, P2SH or covenant-owned holders): the leader witness built is the P2PK
  one (`0x00` + signature).
- **A resting opposite order as the counterparty** of a bid evidence or a bid counter-evidence (the executor's own
  tokens are used instead). A challenger without tokens therefore refutes a liquidation of a long only by holding
  tokens first, e.g. those an ask evidence bought.
- **An IOC ask filled in part**: its unsold tokens must return to the maker at the custody's own index; the builder takes
  such an ask entirely (an ask that ends with its fill). With the testnet evidence life no IOC/FOK order serves anyway.
- **Evidence from conditional, if-done, pair, decaying or rising orders**: `KpfPair` and `KpfPending` refuse them
  (plain, fixed quote only).
- **Several pieces of one split in one block**: they share a covenant id and each must be the only input of its id.
- **Mixed blocks from the command line**: the library accepts any mix that one evidence serves (a short's close
  proposal and a long's liquidation proposal at one ask, proposals and challenges on one print); `kpf` exposes one
  path per command.
- **Discovery of pending parts** from the node (see 7.3): `kpf watch` reads object files only.
