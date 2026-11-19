# Live run 1: an oracle-only market on testnet-10

Date 2026-11-19. Network testnet-10, node `ws://<tn10-node>:18210` (JSON wRPC; the node the KOB soak uses; one
connection at a time, no bulk queries). Code at commit `9ebfbcd` (after review 7). This is the first time any KPF
transaction was sent to a real network. Every transaction below was built by `kpf`, verified locally in the script
engine, and accepted by the node at the fee the tool computed.

## Result

The whole core flow of an oracle-only market ran on the live network on the first attempt: 21 KPF transactions, none
refused. The contracts needed no change. Three tooling defects and several gaps were found (below).

## Market

`kpf oracle-market --feed feed.txt --window-daa 1200 --round-daa 600 --stake-age-daa 1200 --stake-unbond-daa 600
--min-stake 10 --term-daa 36000 --step-daa 6000 --tail-daa 1200 --slot-unbond-daa 600 --idle-daa 3000 --quiet-daa 100`
(market id `48d0838c…1710b`; a made-up slow underlying; windows shortened so the run fits in an hour; the tool
warns, correctly, that a contest within such a window is not realistic). Pair: 2 units at 10 KAS, long rate 1, short
rate 2, range [9, 12], cover 12 KAS on a 40 KAS slot (ρ = 1).

## Steps

| # | Step | Transaction | Accepted DAA | Fee (KAS) | Bytes |
|---|---|---|---|---|---|
| 1 | underwriter bonds a 40 KAS slot | `bb02d642…3e47` | | 0.0021 | 353 |
| 2 | staker bonds 20 KAS of judge stake | `68401724…2274` | 627075050 | 0.0020 | 319 |
| 3 | underwriter bonds 30 KAS of judge stake | `7d2de925…9e42` | 627075160 | 0.0020 | 319 |
| 4 | long order | `8001e7bf…5534` | | 0.0021 | 353 |
| 5 | short order | `02301121…95f2` | | 0.0021 | 353 |
| 6 | open: two orders + slot → bound pair | `43605c6e…400b` | 627075875 | 0.0917 | 45,856 |
| 7 | claim 1: liquidate half the long at 9.2 (nobody contests) | `d247e0fc…7be9` | 627076463 | 0.0378 | 18,911 |
| 8 | claim 2: liquidate the other half at 9.2 | `efb69ecc…a426` | 627076736 | 0.0376 | 18,824 |
| 9 | contest of claim 2 with the staker's aged stake | `679cf07f…9285` | 627077111 | 0.0296 | 14,809 |
| 10 | resolve: nobody defended → **refuted** | `3f3f14bd…474c` | 627078078 | 0.0293 | 14,629 |
| 11 | finalize claim 1 after the window → the long half is forfeited | `a98e66ab…538a` | 627078186 | 0.0097 | 4,862 |
| 12 | finalize claim 2 → part restored, bond 0.8 KAS to the long, 0.2 KAS to the disputer | `5e1eb858…2627` | 627078426 | 0.0098 | 4,914 |
| 13 | settle the pool: the staker's 20 KAS back | `bb475af9…7252` | 627079111 | 0.0397 | 19,861 |
| 14 | close the pool | `ef39e9f5…c03d` | | 0.0197 | 9,869 |
| 15 | claim 3 on the restored part: liquidate the long at 9.2 | `b2d24e95…6fb9` | 627080679 | 0.0379 | 18,944 |
| 16 | contest of claim 3 (20 KAS on NO) | `b6ade356…53db` | 627080888 | 0.0296 | 14,809 |
| 17 | the underwriter defends (30 KAS on YES; YES leads) | `056a066f…2ef1` | 627081122 | 0.0400 | 19,988 |
| 18 | resolve after the round → **upheld** | `1c82b924…6bb7` | 627082003 | 0.0293 | 14,629 |
| 19 | finalize claim 3 → forfeit; bond and fee 1.1 KAS back to the underwriter | `8d7a1401…aa27` | | 0.0097 | 4,862 |
| 20 | settle the pool: loser forfeited, winner's stake 48 KAS | `8379c596…efba` | 627082435 | 0.0592 | 29,584 |
| 21 | close the pool: 2 KAS burned | `e105e983…f6f0` | | 0.0198 | 9,922 |

Funding (not KPF transactions): 300 testnet KAS from another test wallet (`610142a9…4129`), then four plain
sends from the funder to the role keys.

## What the live network showed

- **Engine and node agree** on every shape sent: the open with its fresh-id and twin scans and the bind; a proposal
  with a split; contest; deposit; resolve with delivery of the verdict; finalize in all three branches (uncontested
  forfeit, refuted restore, upheld forfeit); pool settlement with forfeiture and burn.
- **Relay:** no shape was refused; each was accepted at the tool's fee. The open has storage mass 268,930 (a 0.04 KAS
  premium output is the heavy term) against compute mass 55,236 and was accepted at 0.0917 KAS.
- **Acceptance** took under about 10 seconds in every case (the run waited 8–12 s between dependent steps).
- **DAA scores read by the contracts** behaved as the tests assume: the stake age (1,200 DAA before the claim), the
  contest window, the round and the claim window were all enforced at the on-chain scores recorded above.
- **Three outputs of one transaction** (pending part, receipt, dispute) carry the same DAA score, as review 6 assumed.

## Defects and gaps found

1. **The vouch input's value goes to the funder's change** (tooling, real loss in practice). `kpf vouch` spends one
   of the underwriter's plain UTXOs as the vouch and does not return its value to the underwriter: in step 8 the
   underwriter's 70 KAS UTXO ended in the executor's change. Here both keys are the tester's; an underwriter using
   a third-party executor would have paid it 70 KAS. The builder must pay the vouch input's value back to its owner
   (and the runbook must say to vouch with a small UTXO until it does). Whether the contract constrains this output
   needs checking.
2. **`kpf inspect` cannot decode oracle-family objects** (it tries only the book-market decoders).
3. **Object files written at build time have no DAA**, and every later command refuses them until
   `kpf status --object … --node … --update` is run. Commands given `--node` should refresh the DAA themselves.
4. **No command to unbond a judge stake.** A stake returned by `settle-pool` is a `KpfStake` UTXO; the underwriter's
   48 KAS stays there until an unbond command exists.
5. **No commands for the price-free exits of an oracle pair** (amend, transfer, mutual and cooperative close), as
   already documented; the two remaining pairs can leave only by lapse settle.
6. `settle-pool` must be given the loser's receipt together with the winner's ("the pool cannot pay these winners
   before the losers are forfeited"); the runbook did not say so.
7. A dry run and its live rerun write two sets of object files (the transaction id changes); the stale set must be
   deleted by hand.

## Not exercised

Cure; renewal; a chained submission on an unaccepted parent; two transactions spending one
UTXO; anything at `ρ > 1`; the book-market family (not sent in this run).

## Balances after step 21

alice (long) 63.57 KAS + 0.8; bob (short) 35.78; staker 20.20 (its 20 KAS stake was forfeited in the upheld dispute);
underwriter 1.1 + 1.1 plain, 40 KAS in the slot, 48 KAS in judge stake; executor about 82 KAS (it received the
underwriter's 70 KAS through defect 1 and paid every fee). Still in covenants: two one-unit pairs (3.1 KAS each, both
sides the short's key after the forfeits), the 40 KAS slot, 48 KAS of judge stake.

## Clean-up after the cover lapsed (same day, DAA 627120794 onward)

Run by `run/live-1/cleanup.sh` about an hour after the open, once the cover had expired (expiry DAA 627114000, lapse
settle allowed from 627118200). Four more transactions, all accepted on the first attempt; 25 in total.

| # | Step | Transaction | Fee (KAS) | Bytes |
|---|---|---|---|---|
| 22 | lapse settle of the first one-unit pair: long 0.9, short 2.2 | `9da26928…cdb8` | 0.0304 | 15,217 |
| 23 | lapse settle of the second one-unit pair: long 0.9, short 2.2 | `461f8670…32b2` | 0.0304 | 15,217 |
| 24 | unbond the 40 KAS slot | `6ec41a95…8ef6` | 0.0101 | 5,063 |
| 25 | withdraw the slot after the unbonding delay: 39.990546 KAS (it pays its own fee) | `5159930d…be88` | 0.0095 | 4,727 |

- Lapse settle, unbond and withdrawal behave on the network as in the engine; the unbonding delay (600 DAA here) was
  enforced on the slot's on-chain score.
- **Defect 1 happened a second time:** the vouch for claim 3 spent the underwriter's 1.1 KAS UTXO returned by claim 1,
  and its value again went to the executor's change. In this run the underwriter lost 71.1 KAS to the executor this way.

### Final balances

| Key | KAS |
|---|---|
| long (alice, also the funder of the role keys) | 63.569765 |
| short (bob) | 41.977917 |
| underwriter | 41.090546 plain + 48 in judge stake (no unbond command yet, defect 4) |
| staker | 20.197951 |
| executor | 82.534328 |

Sum 297.37 KAS of the 300 received: 2 KAS burned by the pool close of step 21, about 0.63 KAS of fees. Nothing else
is left in a covenant.
