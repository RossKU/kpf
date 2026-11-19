# Live run 2: cure, the price-free exits, the stake exit

Date 2026-11-19. Network testnet-10, the same node and the same market as live run 1 (`docs/LIVE-1.md`; market id
`48d0838c…1710b`, shortened windows). Code at commit `cf4065a` (the tooling fixes from run 1; contracts as in run 1).
Purpose: check the tooling fixes on the network and exercise what run 1 left out.

## Result

16 KPF transactions, all accepted once their time locks had matured; the contracts needed no change. The fix of run
1's worst defect holds on the network. One path failed: a renewal of a small pair cannot be mined (defect 1 below).
Four new tooling defects.

## Steps

| # | Step | Transaction | Fee (KAS) | Bytes |
|---|---|---|---|---|
| 1 | unbond the 48 KAS judge stake left by run 1 (`stake-unbond`) | `92c184ab…9e76` | 0.0202 | 10,099 |
| 2 | withdraw it after the delay (`stake-withdraw`): 47.980148 KAS | `6abe26a0…beb4` | 0.0199 | 9,926 |
| 3 | underwriter bonds a 40 KAS slot | `0035a362…4484` | 0.0043 | 593 |
| 4 | long order (2 units, limit 10.5, margin rate 1) | `82051b9c…2464` | 0.0043 | 593 |
| 5 | short order (2 units, limit 9.5, margin rate 2) | `12f2f38e…4195` | 0.0054 | 713 |
| 6 | open at 10: pair 6.2 KAS, range [9, 12] | `f0a9f8e9…f8ab` | 0.0917 | 45,856 |
| 7 | claim: liquidate one unit of the long at 9.2; the 49.07 KAS vouch input pays the 1 KAS bond, 48.07 KAS back to the underwriter | `9c086081…1d8b` | 0.0379 | 18,963 |
| 8 | **cure**: the long adds 0.3037 KAS, its bound moves 9 → 8.6963; bond and carrier 1.2 KAS back to the underwriter | `3dc20bd2…2849` | 0.0101 | 5,048 |
| 9 | **amend**: the long lists the remainder at 10 | `99e1f9ee…16c0` | 0.0302 | 15,102 |
| 10 | **amend**: the short lists a bid of 11 | `cba3e1cf…7479` | 0.0302 | 15,102 |
| 11 | **mutual close** at 10.5: long 1.5, short 1.7 | `4e5ec6dd…bbdc` | 0.0301 | 15,055 |
| 12 | **amend**: the long lists the cured part at 10.2 | `00c6300e…8a54` | 0.0302 | 15,102 |
| 13 | **transfer**: a third key takes the long side at 10.2 with bound 8.6; the leaver gets 1.5037 | `55d897ff…19fb` | 0.0303 | 15,163 |
| 14 | **cooperative close** (third key and short sign): long 1.6, short 1.97 | `a5ae24b2…8214` | 0.0300 | 15,005 |
| 15 | unbond the slot after the cover's liability ended | `ecc84397…be5a` | 0.0099 | 4,943 |
| 16 | withdraw the slot: 39.990546 KAS | `07f9b14a…c9b4` | 0.0095 | 4,727 |

## What the live network showed

- **The vouch fix holds** (run 1's defect 1): the underwriter's vouch input paid only the bond; the rest returned to it.
- **Cure, amend, transfer to a signing taker, mutual close, cooperative close, stake unbond and withdraw** behave on the
  network as in the engine. None had been sent before.
- **Refreshing object files from the node** (run 1's defect 3) works: no manual `status --update` was needed.
- **Dry-run folder** (run 1's defect 7) works: a refused submission leaves its files under `dry-run/`.
- **Relative locks are enforced as the contracts state:** an open needs the slot to be 600 DAA old; a stake withdrawal
  needs the unbonding delay; both were refused by the node until then ("one of the transaction sequence locks
  conditions was not met") and accepted afterwards, unchanged.

## Defects found

1. **A renewal of a small pair cannot be mined.** `kpf renew` on the one-unit pair built a transaction with storage
   mass 510,287 (limit 500,000): the premium, 0.02 KAS, is written as an output of its own, and an output that small
   costs about 500,000 grams by itself. The cover of a small pair therefore lapses. The contract leaves the premium's
   destination to the underwriter's signed bind, so the builder can fold it into a larger output. Not renewed in this
   run; the pair was closed cooperatively instead.
2. **The carrier of a split claim was paid by the funder** (0.2 KAS) while a cure refunds bond and carrier to the
   underwriter. Fixed after the run (commit `7d9024e`: a liquidation's vouch input funds both).
3. **No warning before a time lock has matured:** the open was submitted three times too early and the stake withdrawal
   once; the tool printed the node's refusal only. Fixed after the run (`7d9024e`: refused locally with "usable from
   DAA X").
4. **`oracle-transfer` with a bound that weakens the staying side** built a transaction that fails verification with a
   bare "VerifyError" and a fee of 0.217 KAS. Fixed after the run (`7d9024e`: the rule and the allowed range are named).
5. Operational: a background script stopped by the host kept running as an orphan, so a restart produced two copies
   (the second cooperative close was answered "already in the mempool"; no harm). Waiting scripts should take a lock
   file.

## Not exercised

Renewal (defect 1); an order taking over a listed side; close-versus-close (no command; review 8 found its critical
flaw there); a chained submission on an unaccepted parent; two transactions spending one UTXO; anything at `ρ > 1`;
the book-market family.

## Final balances

long 63.835346, short 41.422464, third key 1.6, underwriter 89.296917, staker 20.197951, executor 80.623558: in all
296.98 KAS of the 300 received (2 KAS burned in run 1; about 1.02 KAS of fees over both runs). Nothing is left in a
covenant.
