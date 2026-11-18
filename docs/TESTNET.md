# First live exercise on testnet-10

Runbook for the repository owner. KPF is a **testnet-only research prototype**: the command line refuses mainnet
addresses, mainnet network ids and any node that does not report `testnet-10`. Nothing here has been run against a
network yet: the tooling was only exercised offline (engine tests, and the CLI against an in-memory UTXO set).

What this exercise covers: every evidence-free path of `KpfOpen` and `KpfPair` (create order, open, list, transfer,
add margin, mutual close, cooperative close, idle settle, plus cancel, refund, open against a signing taker and
close-versus-close). Price evidence (unilateral close, liquidation, in two steps: propose, then challenge, cure or
finalize) is a separate exercise: section 7; guaranteed fast settlement (guarantee, dispute, release) is section 8.

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
by the size of the redeem scripts (a pair's script is now 12,370 bytes, with the two-step evidence of DESIGN 4.5d and the
fast-settlement parameters of 4.5f,
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
   refuses `W < 2 * minRestDaa` (DESIGN 4.5e), `bondMin` above 10,000 KAS and `L + minRestDaa > 77,760,000`. Guaranteed
   fast settlement (DESIGN 4.5f, section 8) adds `lockBps` 11,000 (`λ` = 1.1), `disputeDaa` 650 (`D`, 65 seconds) and
   `graceDaa` 100 (`G`, 10 seconds); `kpf market --check` refuses `λ` outside 1..3, `D < 2 * minRestDaa` and `G > W`.
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

Every transaction contains the 12,370-byte redeem script of each pair it spends, or the 4,338-byte script of each
pending part, plus KOB's order script and the token program script (2.9 KB) with the custody or the seller's tokens
when it reads a quote. The storage mass (KIP-9) comes from the small token outputs; the block limit is 500,000 and a
token output of 0.2 KAS uses 200,000, which is why the builders keep their own outputs generous.

The command line (`crates/kpf-cli/tests/lifecycle.rs`, the evidence lifecycle):

| step | fee (KAS) | compute mass | storage mass |
|---|---|---|---|
| propose-close | 0.04391 | 31,623 | 400,059 (two token outputs of the bid fill) |
| finalize (close) | 0.00954 | 6,850 | |
| propose-liquidate | 0.04358 | 31,192 | 404,022 |
| challenge | 0.02010 | 14,950 | 400,027 |
| cure | 0.00974 | 8,061 | |
| finalize (forfeit) | 0.00961 | 6,895 | |
| coop-close of the forfeited part | 0.02542 | 15,270 | |

The engine metrics (`kpf_metrics_tests`; parts of 400 tokens; limits compute 500,000, storage 500,000, 250,000 bytes):

| transaction | inputs | script units | compute mass | storage mass | bytes | fee (sompi) |
|---|---|---|---|---|---|---|
| propose close long at a bid | 4 | 710,305 | 30,860 | 203,474 | 21,760 | 4,352,000 |
| propose close short at an ask | 4 | 651,623 | 29,894 | 200,056 | 21,664 | 4,332,800 |
| propose liquidate long at an ask | 4 | 789,015 | 31,129 | 200,056 | 21,599 | 4,319,800 |
| propose liquidate short at a bid | 4 | 847,763 | 32,095 | 203,205 | 21,695 | 4,339,000 |
| challenge (ask counter) | 4 | 217,249 | 13,221 | 200,035 | 9,761 | 1,952,200 |
| challenge (bid counter) | 4 | 275,930 | 14,187 | 203,255 | 9,857 | 1,971,400 |
| finalize close | 2 | 109,550 | 6,850 | 31 | 4,770 | 954,000 |
| finalize forfeit | 2 | 109,497 | 6,895 | 215 | 4,805 | 961,000 |
| cure | 2 | 210,248 | 8,061 | 306 | 4,871 | 974,200 |
| 8 proposals on one ask (8 x 500 tokens) | 11 | 5,202,501 | 194,924 | 200,408 | 136,014 | 27,202,800 |
| 8 challenges on one bid | 11 | 475,168 | 49,726 | 203,890 | 41,406 | 8,281,200 |

The largest input is a proposing pair (462,277 to 629,786 script units; every input commits the exact budget). A block
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

## 8. Guaranteed fast settlement: guarantee, dispute, release

Optional (DESIGN 4.5f); the slow path of section 7 stays the default. A pending claim (a proposal of section 7) can be
settled **at once**, exactly as `kpf finalize` would settle it after the window, by a **guarantor** that locks KAS for
it: `kpf guarantee` spends the pending part and creates a `KpfLock` output holding at least `λ` (1.1 on the testnet
market) times the **cover** (the value the settlement moves away from the party it could wrong: the part's range for a
liquidation, the closer's payoff for a close) plus the proposer's **offer**. For `D` (`disputeDaa`, 650 DAA, 65 seconds)
after the guarantee anyone may **dispute** the lock with exactly the counter order a challenge would have needed
(resting `minRestDaa` from the claim's DAA `P`, done resting within the window); the wronged party is then paid the whole
cover from the lock (whoever disputes, on whichever valid order), the disputer keeps `(λ - 1)` times the cover, and the rest (the offer) is burned.
After `D` anyone **releases** the lock to its owner. A liquidation can be guaranteed only once its pending part is
`graceDaa` (100 DAA, 10 seconds) old, so the loser can still cure. Nothing of this has been run on a network.

What each party sees:
- **Proposer.** `--offer KAS` on `propose-close` / `propose-liquidate` (at most the bond less 0.02 KAS) is what it gives
  up from its own payout if a guarantor fast-settles its claim; with no offer a guarantor earns nothing on chain. A
  guarantee pays the proposer its finalize payout less the offer, at once.
- **Guarantor.** Locks `λ × cover + offer` for `D`; gets it back (the offer included) with `kpf release` if nobody
  disputes; loses all of it if the book refutes the claim. It should guarantee only where a mispriced order at the
  refutation bound would be arbitraged within `minRestDaa`: on a book without arbitrage the deserving loser can place
  that order itself and take the lock (DESIGN 4.5f, residual risk 1).
- **Wronged party** (the loser of a liquidation, the other party of a close). Paid in cash, the whole cover (at least
  its side's value at any price), from the lock; it does not get its position back. It must dispute (or a watcher must) within `D`; `kpf watch` shows, for a pending claim, from which DAA a guarantor can settle it (after that there is no cure), before the slow-path deadline.
- **Winner.** Holds the forfeited part at once and may close or sell it while the lock is disputable; a later dispute
  never touches the part.

### 8.1 The exercise

On the market of 7.1 (with the parameters of 7.1, item 2) and the pair of section 7.3, `$G` = the guarantor's key flags
(`--funder guarantor.key.json`, its funding or `--node`).

| # | Command | After it, check |
|---|---|---|
| 30 | a stale liquidation proposed with an offer: `kpf propose-liquidate --pair P.json --evidence ask.json --offer 1 $F $S --out-dir out/30` | the pending part as in 7.3 step 23, with `offer` 1 KAS in its state (`kpf inspect`) |
| 31 | within 100 DAA: `kpf guarantee --pending out/30/*-pending.json $G --node $NODE --submit` | refused: `a liquidation can be guaranteed only once its pending part is 100 DAA old (the loser's cure grace)` |
| 32 | after it: `kpf guarantee --pending out/30/*-pending.json --pair P.json $G $S --out-dir out/32` | the proposer receives 5.47 - 1 = 4.47 KAS at once; the short holds the forfeited part; a lock of 33 + 1 = 34 KAS (the 10-token pair's range 30 KAS times 1.1, plus the offer); `kpf inspect out/32/*-lock.json` prints the dispute bound (`>= 9.66315790` KAS) and the DAA from which it can be released |
| 33 | `kpf watch --pair P.json --scan out/32 --order bid.json --node $NODE` | `FAST-SETTLED ... against the long`, the last DAA a new counter order can be placed (and the DAA by which it must have rested) and the release DAA with the time left, and for each given order `DISPUTES IT` with the exact `kpf dispute` command and the restitution, or why not |
| 34a | refute it (a bid of 9.80 placed after the claim, rested): `kpf dispute --lock out/32/*-lock.json --evidence bid.json --seller seller.json $F $S --out-dir out/34` | the long is paid the cover, 30 KAS, from the lock (its side at 9.80 was worth `q(10, 9.80 - 9)` = 8 KAS); the funder keeps 3 KAS; the offer, 1 KAS, is burned (a P2SH of `OP_RETURN`) |
| 34b | or nobody disputes: after 650 DAA `kpf release --lock out/32/*-lock.json $G $S --out-dir out/34` | the guarantor receives the 34 KAS lock; before 650 DAA the command says `can be released only from DAA N` |
| 35 | a close guaranteed at once: `kpf propose-close --pair P.json --side short --n 4 --evidence ask.json --key $B $F $S --out-dir out/35`, then `kpf guarantee --pending out/35/*-pending.json $G $S` | the short is paid at once what 7.3 step 21 pays after the window; the lock holds 1.1 times the short's payoff |

`kpf quote --pair P.json` states, besides section 7's figures, the lock a guarantee of a liquidation or of a close at
the entry needs, the reward of a disputer, and that the guarantor earns only the proposer's offer. `kpf watch` finds
locks, like pending parts, only from object files (`--lock FILE` or `--scan DIR`): a lock's address depends on its
whole state.

### 8.2 Measured cost (offline, relay floor 100 sompi per gram)

The command line (`crates/kpf-cli/tests/lifecycle.rs`, `fast_settlement_lifecycle_with_a_kob_book`):

| step | fee (KAS) | compute mass | storage mass |
|---|---|---|---|
| guarantee (liquidation) | 0.01397 | 9,947 | 94 |
| dispute (bid counter) | 0.01619 | 12,786 | 200,025 (the token output of the bid fill) |
| guarantee (close) | 0.01390 | 9,902 | 22 |
| release | 0.00588 | 4,659 | 0 |

The engine metrics (`kpf_metrics_tests`, parts of 400 tokens): guarantee 152,390 to 152,559 script units, 6,952 to 6,987
bytes; dispute 213,906 (ask counter) to 272,656 (bid counter) script units, about 8,000 bytes; release 105,372 script
units, 2,939 bytes; 8 disputes on one bid 449,376 script units, compute mass 38,518, 27,318 bytes. The `KpfLock` script is
2,559 bytes, the `KpfPending` script 4,338. Besides the fees, the guarantor's lock is held for at least `D`.

### 8.3 What is not built

- **Several locks for one claim** (the note's lock block): one guarantor per claim.
- **A data-signature guarantee** (a guarantor signing a claim digest instead of funding the lock in the transaction):
  not needed (experiment Z1, DESIGN 4.5f).
- **Discovery of locks** from the node: object files only (as for pending parts).
- **Stake, a registry, unbonding, markets without an order book**: not in this stage.

## 9. An oracle-only market with a slow underlying

An oracle-only market (`docs/DESIGN-ORACLE-IMPL.md`, sections 15-23) has no order book behind it: the price of every
claim on a pair is vouched by that pair's underwriter, and a false price stands unless someone contests it within the
window with aged judge stake. This exercise runs one market on an underlying that changes slowly (published once an
hour), so that a person at a terminal can play every role: the underwriter (bonds a slot, vouches prices, renews), two
parties (long and short), a staker (contests), and the executor (pays fees). It was exercised only offline
(`crates/kpf-cli/tests/oracle_cli.rs`); nothing here has been run against a network.

### 9.1 What must exist before

Everything of sections 1 and 2, and: keys `uw` (underwriter), `alice` (long), `bob` (short), `sam` (staker), `exec`
(executor, the funder of fees); about 700 KAS for the underwriter (a 600 KAS slot at the small test size below, its
vouching UTXO, a defence stake), 200 KAS each for alice and bob, 120 KAS for sam, 10 KAS for exec. The judge counts a
stake only if it was bonded `M_age` (7 days at the defaults) before the claim it judges: **create the stakes first and
wait**, or create the market with a short age (`--stake-age-daa`, `--round-daa`) for a quick drill. Since review 6 the contracts bound
the judge (a round at most 2,592,000 DAA, stake age at most 25,920,000, unbonding at most 77,760,000, the least stake at
most 10,000 KAS); `oracle-market` refuses a market beyond them, and `oracle-check` states every judge parameter and warns
when a contest within the window is not realistically possible. `oracle-watch --node` reads every object's DAA from the
node; without `--node` a DAA read from a file is marked unchecked.

### 9.2 The market

Write the feed definition as a plain ASCII text file, one `key: value` line per item: what is priced, its unit, the
sources and how they combine, the time a price refers to, the sessions (when the underlying is closed), and the
disagreement rule the judge applies. The market's id is `blake3` of its parameters and of this file's bytes exactly: one
byte changed is another market.

```sh
cat > feed.txt <<'EOF'
kpf feed definition v1
quantity: example slow underlying, published once per hour
unit: KAS per whole unit
sources: median of three named public sources
time: the price at the DAA score t0, mapped to the accepting block's timestamp
sessions: always open
disagreement: a claim is true if within 0.25 % of the median
EOF
kpf oracle-market --feed feed.txt --out market.json            # testnet defaults; prints the id and the worst cases
# a quick drill: a 1-hour window, 10-minute judge rounds, stake counted after 20 minutes
kpf oracle-market --feed feed.txt --out drill.json --window-daa 36000 --round-daa 6000 --stake-age-daa 12000 --stake-unbond-daa 6000
kpf oracle-check --market market.json --feed feed.txt --size 10 --x 10 --long-rate 1 --short-rate 2 --cover-rate 6 --premium 0.02
```

`oracle-check` is what every party runs before trading: it refuses a market whose feed text does not hash to the
market's feed hash or whose parameters fail the contracts' bounds, warns on characters that render misleadingly and on
missing lines, and states the parameters and the worst cases in words (an undisputed false claim takes up to a side's
whole margin; a refuted one costs its proposer the claim's bond). Parameters that pass the bounds can still be hostile
(a window of seconds, no quiet time, a tiny bond): the warnings name them; nothing else does.

### 9.3 The underwriter bonds a slot, the staker bonds a stake

```sh
kpf bond  --market market.json --value 600 --funder $UW  --node $NODE --out-dir out/bond   # cover up to 600 KAS at rho 1
kpf stake --market market.json --value 100 --funder $SAM --node $NODE --out-dir out/stake
kpf stake --market market.json --value 130 --funder $UW  --node $NODE --out-dir out/uwstake  # the underwriter's defence
```

### 9.4 Two orders and the open

```sh
kpf oracle-order --market market.json --side long  --size 10 --limit 10.5 --rate 1 --min-counter-rate 1.5 \
    --expiry $EXPIRY --prem-max 0.1 --funder $A --node $NODE --out-dir out/lo
kpf oracle-order --market market.json --side short --size 10 --limit 9.5  --rate 2 --min-counter-rate 0.5 \
    --expiry $EXPIRY --prem-max 0.1 --funder $B --node $NODE --out-dir out/so
kpf oracle-open --long out/lo/*-oracle-order.json --short out/so/*-oracle-order.json --slot out/bond/*-underwriting-slot.json \
    --n 10 --x 10 --cover-rate 6 --premium 0.02 --key $UW --funder $EX --node $NODE --out-dir out/open
```

Each order names the slots it accepts (`--underwriter SLOT_ID`, any by default) and the highest premium it pays; the open
requires the slot to count the pair's cover (`q(n, coverRate)`) at its expiry, refuses an underwriter that is a party,
and refuses a second output of the new pair's id. The expiry defaults to the first ring boundary one term after the
orders. The underwriter receives the first term's premium, half from each order's escrow.

### 9.5 A claim, a contest, the verdict

The underwriter vouches by spending one of its own plain UTXOs in the claim (write it to a file with `kpf utxos --key $UW
--node $NODE --out vouch.json` and keep one entry):

```sh
kpf vouch --pair out/open/*-oracle-pair.json --kind liquidate --side long --price 9.2 --vouch vouch.json \
    --key $UW --funder $EX --node $NODE --out-dir out/claim
kpf oracle-watch --me $A --scan out/claim --node $NODE       # A CLAIM AGAINST YOU ... contest until DAA ...
kpf contest --pending out/claim/*-oracle-pending-part.json --stake out/stake/*-judge-stake.json --key $SAM \
    --funder $EX --node $NODE --out-dir out/contest
kpf deposit --dispute <the dispute file of out/contest> --side yes --stake out/uwstake/*-judge-stake.json --key $UW \
    --funder $EX --node $NODE --out-dir out/defence       # only if the claim is true: the underwriter defends it
kpf resolve --dispute <the latest dispute file> --pending out/contest/*-oracle-pending-part.json --funder $EX --node $NODE \
    --out-dir out/verdict                                  # once the round's window has closed
kpf oracle-finalize --pending out/verdict/*-oracle-pending-part.json --funder $EX --node $NODE --out-dir out/final
kpf settle-pool --pool <the pool file of out/verdict> --receipt <each receipt file> --funder $EX --node $NODE
kpf settle-pool --pool <the settled pool file> --funder $EX --node $NODE    # close: the base back, the rest burned
```

What to check: a contest landing at `P + W` or later is ignored (the claim settles); a refuted claim restores the part
exactly and pays its bond to the long and to sam; an upheld one finalizes at once; an uncontested one finalizes after
`W`. A claim that lands more than `lateDaa` after its `t0`, or less than `W` before the expiry, is void: finalize
restores the part and pays the bond to the defender. The loser of a pending liquidation can cure instead
(`kpf oracle-cure --pending ... --key $A`).

### 9.6 Renewal, lapse, leaving

```sh
kpf oracle-watch --me $A --scan out --node $NODE            # your underwriter's cover runs until DAA X; renewal from X - step
kpf renew --pair <pair file> --slot <slot file> --key $UW --funder $EX --node $NODE     # from one step before X
kpf lapse-settle --pair <pair file> --funder $EX --node $NODE   # from X + tail + idleDaa by the calendar: at the entry
kpf unbond --slot <slot file> --key $UW --funder $UW --node $NODE   # once every cover's liability has ended
kpf withdraw-bond --slot <unbonding slot file> --key $UW --node $NODE
```

If the underwriter goes silent, the parties leave without any price: `amend` and `transfer` to a taker who signs, a
mutual close, a cooperative close, or the lapse settle once `idleDaa` has passed after the cover lapsed (by the calendar: no party can postpone it).
A part handed over to a taker starts a new covenant id (review 6): use the new object file the builder writes. Book-market commands that touch a pair
(`amend`, `transfer`, `mutual-close`, `coop-close`) do not read oracle pairs; the library builders do
(`kpf_protocol::oracle_build`), and their commands are the next step of the tooling.

### 9.7 Measured cost (offline, 100 sompi per gram)

Open 0.0900 KAS, a claim 0.0367 + finalize 0.0095, a renewal 0.0390 (seven pairs in one bind 0.2115), a contest 0.0294,
a defence 0.0400, the resolution 0.0291, settling and closing the pool 0.0793: a pair-month without a dispute is about
0.18 KAS of network fees, plus the premium the orders agreed. Details: `docs/DESIGN-ORACLE-IMPL.md` sections 19 and 24.6
(after review 6).

### 9.8 Not possible yet

- **Discovery**: pending parts, disputes and receipts are found only from object files (as pending parts and locks).
- **A request to a silent underwriter, rebind, the lock path, a retrospective dispute**: not in this slice.
- **The price-free pair commands for oracle pairs** on the command line (amend, transfer, mutual and cooperative close):
  built in the library and tested in the engine, not yet commands.
