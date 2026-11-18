# KPF review 1

## Scope and method

Scope: `contracts/KpfPair.sil` and `contracts/KpfOpen.sil` as they are in the tree, checked against `docs/DESIGN.md`
and `docs/THREAT_MODEL.md`, with the KOB v3 evidence orders (`vendor/kob/contracts/orders/KobAsk.sil`, `KobBid.sil`)
and the slot rules of `vendor/kob/docs/spec/matcher.md` used as the price-evidence side. Method: read every entry
by hand and list each value it reads and each value it writes. For every write, ask who chooses it and which guard
later reads it. Each candidate was built as an engine transaction with the existing builders
(`crates/kpf-tests/tests/kpf/`). Proofs are in `crates/kpf-tests/tests/audit_tag_tests.rs`. Run them with
`cargo test -p kpf-tests --test audit_tag_tests`. Every input of every transaction runs its real script there,
including KOB orders and token inputs.

## Findings

| # | Severity | Title |
|---|---|---|
| F1 | Critical | A tag equal to the pair's own id freezes the counterparty; with a self-chosen entry price, idle settle takes its whole margin |
| F2 | High | A tag equal to the token's covenant id makes a pair immune to every evidence path |
| F3 | Medium | The entry price used by idle settle can be set by one party alone |
| F4 | Low | KpfOpen does not check the evidence parameters that the pair it creates needs |
| F5 | Informational | Keeper fee floor can be charged once per minimum-size piece |

### F1 (Critical). Self-tag freeze, then idle-settle theft

Affected: `KpfPair.transfer` → `handOver` (`tl = et` / `ts = et`), `closeHost` (`tl3`), `onEvidence` path 7 (`et`),
`closeDonor` (`ts3`, when P3 sits under an id whose holder is known); the guard `tagsNotLive`; `idleSettle`.

The tag of a direct taker is an argument that nothing checks. `tagsNotLive` requires `OpCovInputCount(tag) == 0`. A
tag equal to the pair's own covenant id is therefore never satisfiable, because the pair is always its own input.
Every entry except `idleSettle` and `coopClose` fails from then on.

Transaction sequence (the long attacks the short):
1. `amend` (long's signature): `longAsk = bs`.
2. `transfer(LONG, n = s, x = bs, nb = bl, ek = own key, et = selfId)` with the long as the direct taker. It pays
   itself `q(s, bs − bl)` and re-funds the part with the same amount, so the net cost is the network fee. The part
   now has `entryPrice = bs` and `tagL = selfId`.
3. The short's `amend` (which would reset the idle age), `addMargin`, P5 close, transfer and P4 are all refused.
4. After `idleDaa`, `idleSettle` pays the long `q(s, c(bs) − bl)`, the whole range. The short keeps only the carrier.

Who loses: the short loses its whole margin (2,000 KAS in the DESIGN 5 example), whatever the market price. The
mirror case exists too: a short lists `shortBid`, takes it itself at `x = 0`, and the long gets nothing. Variant:
any third-party taker of an ordinary listing can freeze the stayer. The stayer then has no exit and cannot be
rescued for 90 days, and is settled at the taker's price; the new side cannot be liquidated either. The design's
G3 promise and the K2 residual ("loses its unrealised profit") both fail.

Tests: `exploit_self_tag_freeze_then_idle_theft`, `exploit_short_self_tag_freeze_then_idle_theft`,
`exploit_third_party_freeze`. Controls: `holds_ordinary_transfer_tag_does_not_freeze`,
`holds_frozen_pair_coop_close_still_works` (shows the only release needs the attacker's signature).

Smallest fix: in `tagsNotLive`, ignore a tag equal to `selfId` (the single-input guard already counts it). Also
refuse `et`/`tl3`/`ts3` equal to `selfId` when they are written. Separately, fix F3.

### F2 (High). Token-id tag blocks all evidence

Affected: the same write sites as F1, and `KpfOpen.take`'s `counterTag` on a part that is not a continuation;
`evEntry` → `tagsNotLive`.

A direct taker writes `et = tokenCovId`. Every evidence transaction contains a token input of that id: an ask's
custody, or the tokens sold to a bid. As a result, P5, P6 and P7 are refused for the life of the pair, while amend
and the listing paths still work.

Who loses: the bystander loses its unilateral exit and the ability to liquidate the new side. The design says
liquidation must not need the loser's cooperation, and here it never happens. The entrant posts no less margin
(no-downgrade holds), so the pair stays fully funded and no principal leaves. What the bystander loses is the exit
and liquidation.

Test: `exploit_token_id_tag_blocks_evidence`, with a control showing the same pair with tag 0 is liquidated at the
same ask. Fix: skip `tokenCovId` in the guard as well, or have the guard count only claimer inputs (KpfOpen
templates and pairs), not covenant ids in general.

### F3 (Medium). Entry price writable by one party

Affected: `transfer`, `closeHost`, `closeDonor`, P7 (all write `entry = x`); `idleSettle`.

Without any tag trick, a party takes its own listing at any `x` it allows (`x ≥ longAsk`, `x ≤ shortBid`) and
thereby sets `E`. `idleSettle` then pays at that `E`. The counterparty must spend the pair at least once every
`idleDaa`. If it is absent, it loses its whole margin even when the market has not moved. That is worse than
THREAT_MODEL K2 states. Test: `exploit_entry_price_self_transfer`. Fix: keep `E` unchanged on a transfer in which
the entrant key equals the leaver key, or, more robustly, settle P8 at the last price both owners signed (write `E`
only in T1, `coopClose` and the amends), or at `(bl + bs)/2` when `E` was last written by one side.

### F4 (Low). KpfOpen does not validate the evidence-path parameters

`fill`/`take` check scale, priceMax, carrier, minSize and idleDaa, but not `kBps ∈ (0, 10⁵]`, `0 ≤ mmrBps < 5000`,
`liqFeeBps ≤ 10⁴`, `keeperFeeMin ≥ 0`, `0 ≤ minRestDaa < LOCK_TIME_THRESHOLD`, or the token template facts. A market
block outside those ranges creates pairs whose P5/P6/P7 are dead. Both makers must carry the same block, so this
is consented misconfiguration, not theft. No test needed: the refusals are already shown by
`p6_out_of_range_parameters_are_refused` and `k_out_of_bounds_is_refused`. Fix: copy the pair's evidence checks
into `fill`/`take`.

### F5 (Informational). Fee floor per piece

P6 charges `max(keeperFeeMin, rate fee)` per transaction, and pieces of `minSize` are allowed. When
`keeperFeeMin` exceeds the loser's residual per `minSize`, a keeper who liquidates in minimum pieces collects the
floor `s/minSize` times, and the winner pays whatever the loser's residual lacks. This does not bite at the test
parameters (rate fee ≈ 0.047 KAS per token > floor 0.01). It is a parameter-choice note: bound `keeperFeeMin`
relative to `minSize · mmr · price`.

## Unproven

- KpfOpen.take onto a part bound to an attacker-held twin id with `counterTag` = that id: this would freeze an
  honest order's new position as in F1. The analysis is the same as F1, but I did not build the twin-authorised
  output.
- closeDonor's P3 under an attacker-held inherited id with `ts3 = pid` (mirror of F1 via P4). Not built, for the
  same reason.
- Mixed batches with KOB router intents that pay at `j` and `j + 1` (matcher.md): a pair at input `j + 1` whose
  long payout aliases such an output when the long owner is also the intent's owner. KobIfd exits are covenant
  outputs and cannot alias. The router contract was not reviewed.

## Checked and found sound

- Evidence block: blocks naming one `ev` with different `q` always contain each other's members, so any outside
  pair fails. Byte equality on `q` is stricter than int equality. Non-pair members can only raise the sum, and a
  huge `n_j` overflows the attacker's own transaction.
- Evidence identity: template hash plus P2SH; token id, scale, slope 0, interval ≥ 0, extension commitment; the
  ask's custody is the only token input the ask's id may own (KobAsk `noStrays` and its exact-amount rule), so the
  custody DAA cannot be taken from a stale UTXO. A bid's or ask's tip only tightens the bound in the victim's favour.
- Sig-script layout: arguments are pushed in order before the dispatch tag (`encode_entry_sig_script`), so the
  first pushes are `nb`, `qb`. Other pair entries start with a minimally encoded int or a 65-byte signature, which
  `blockN` refuses.
- Output slots: positional `i` and reverse `M − 1 − i ≥` input count are injective across pairs and KpfOpen. A
  KobBid refund at `M − 1 − ev` refuses any covenant input at that index.
- Single-input guard on every entry; the live-tag guard against stale-tag double claims (already tested); P3/P4
  no-downgrade; carriers and the split rule; payout dust rules; signatures are 65 bytes with SIGHASH_ALL.
- State offsets: recomputed by hand (size 209, bl 218, bs 227, lists 310..364, end 430; KpfOpen sizeLeft 251).
  These match the compiled layout tests.
- Compilation of `this.ageDaa` and `tx.daa`: a runtime range check before CSV/CLTV (`[0, 2³²)` and
  `[0, LOCK_TIME_THRESHOLD)`), so a state value outside the range fails instead of hitting a disabled flag.
- Arithmetic within the DESIGN 5 bounds: trigger products ≤ 1.5·10¹⁸, fee rate ≤ 10¹⁸, size bound in `checkPair`
  mirrored by KpfOpen.

## Not covered

GhostDAG races (X7), script-budget limits (X4), the KCC-20 program itself, and KobPair/router interplay.
