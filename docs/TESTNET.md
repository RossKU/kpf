# First live exercise on testnet-10

Runbook for the repository owner. KPF is a **testnet-only research prototype**: the command line refuses mainnet
addresses, mainnet network ids and any node that does not report `testnet-10`. Nothing here has been run against a
network yet: the tooling was only exercised offline (engine tests, and the CLI against an in-memory UTXO set).

What this exercise covers: every evidence-free path of `KpfOpen` and `KpfPair` (create order, open, list, transfer,
add margin, mutual close, cooperative close, idle settle, plus cancel, refund, open against a signing taker and
close-versus-close). It does not cover price evidence (see the last section).

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
| 7 | `kpf transfer --pair out/6/*-pair.json --side short --n 4 --x 10.2 --entrant-order out/3/*-out0-order.json $F $S --out-dir out/7` | two pairs: a part of 4 tokens (short = carol) and a remainder of 6 (short = bob); bob is paid `q(4, 12 - 10.2)` = 7.2 KAS; carol's order continues with 6 tokens |
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
by the size of the redeem scripts (a pair's script is 11.6 KB and is revealed on every spend). If blocks are contended the
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
- **The evidence paths**: P5 unilateral close, P6 liquidation and P7 liquidation by transfer (`KpfPair.onEvidence`) are
  not built by the tooling. Their transaction embeds the fill of a live KOB `KobAsk` / `KobBid` of the market's token
  and that token's custody, with KOB's and the token program's own signature scripts, outputs and per-program limits.
  `kob-protocol` builds these only inside its crate-private draft, which cannot take foreign inputs, and a KPF evidence
  transaction needs such an order to exist, which requires KOB's indexer. The engine tests
  (`crates/kpf-tests/tests/kpf/evidence.rs`) exercise all three paths against KOB's real contracts. The market of this exercise
  carries a placeholder token, so evidence could not be used on it anyway. Without evidence a pair can end by transfer,
  close-versus-close, mutual or cooperative close, or idle settle.
- **Multi-party signing across machines.** `coop-close` needs both owners' key files on the machine that builds it.
  There is no partial-signature exchange yet.
- **Mainnet**: refused on purpose.
