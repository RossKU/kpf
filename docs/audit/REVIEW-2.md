# KPF review 2

## Scope and method

Scope: `contracts/KpfPair.sil` and `contracts/KpfOpen.sil` at commit `003ddea` (after the review 1 fixes), checked
against `docs/DESIGN.md` 3.1, 3.3, 3.4, 4.3-4.6 and 4.5a, with `docs/THREAT_MODEL.md` and `docs/audit/REVIEW-1.md`
as the record of what was already considered. The KOB evidence orders (`vendor/kob/contracts/orders/KobAsk.sil`,
`KobBid.sil`) and the matcher rules in `vendor/kob/docs/spec/matcher.md` were read as the evidence side.

Method: every entry was read by hand, with attention to what the review 1 fixes changed (cleared tags, entry price
kept or bounded by `e3`, the loser-only fee, `KpfOpen.take` without `counterTag` and with the `e` rule). Each
candidate was built with the existing builders and run in the engine, every input executing its real script.
Proofs: `crates/kpf-tests/tests/audit2_tests.rs` (`cargo test -p kpf-tests --test audit2_tests`). The full suite
passes with the new file (177 tests).

## Findings

| # | Severity | Title |
|---|---|---|
| F1 | Critical | A liquidatable party can liquidate itself at a quote it prints for free, and takes value from the winner |
| F2 | High | Orders past their KOB soft expiry count as rested evidence |
| F3 | Informational | P7 with `n = s` keeps the loser's carrier and excess in the new part |
| F4 | Informational | A listing's minimum part does not hold on a smaller part created by a split |

### F1 (Critical). Self-liquidation at an inflated quote

Affected: `KpfPair.onEvidence`, paths 6 and 7 (`closeAt` with `clampP(p)`, and `handOver` with `x >= p` / `x <= p`).

P6 settles a liquidated long at the ask that triggers it, the bound that favours the liquidated party (DESIGN 3.3:
"a long liquidated at an ask `a` is valued at a price ≥ the market"). The rule assumes the liquidated party does not
act. Nothing enforces that: anyone can run P6, the loser included. An ask *above* the market costs nothing to rest,
because no rational buyer takes it, so the "exposed for R" requirement prices nothing here. The same holds for a
short and a bid below the market.

Transaction: the long of a pair whose market price is at or below its trigger rests its own ask at the top of the
trigger window (`a` just under `bl·10⁴/(10⁴ − mmrBps)`), waits R, buys it itself (it is maker, buyer and executor),
and runs P6 on its own pair. It receives `q(n, c(a) − bl)` minus the keeper fee, and it keeps the fee as executor.

Who loses: the winner, who did not act. In the DESIGN 5 pair with the market at 8.5 KAS (long bankrupt), the short
is owed the whole 3,000 KAS. After the self-liquidation it gets 2,526.32 KAS, and the long takes 473.68 KAS
(`exploit_self_liquidation_at_an_inflated_ask`). Mirror case: the short takes 571.43 KAS from the long
(`exploit_self_liquidation_at_a_deflated_bid`). The gain is bounded by the trigger: at most about
`bl·mmrBps/(10⁴ − mmrBps)` per token for a long (5.26 % of `bl` at 5 %), or about 2× `bl` at the allowed
`mmrBps = 4999` (`holds_self_liquidation_needs_the_trigger`). It also beats P5, which settles a closing long at
the adverse bid. Honest keepers do not limit it: a keeper earns more from a higher quote (the fee is proportional to
the quote), and the loser can keep its ask resting in advance.

Smallest fix: value the loser at its trigger bound rather than at the quote, so that a better quote earns it
nothing. For a long, settle at `c = clamp(⌊a·(10⁴ − mmrBps)/10⁴⌋)`, which is always `≤ bl`, so the loser's residual
is 0. The winner then receives the part less the keeper fee, and the fee is capped at the part's maintenance slice
instead of at the loser's residual. Mirror this for a short. P7's loser payout should use the same bound. If the
design wants the loser to keep a residual, it needs two-sided evidence (the adverse bid as well) to value it.

### F2 (High). Soft-expired orders count as rested evidence

Affected: `KpfPair.evQuote` (all of P5, P6, P7).

KOB's matcher spec (`matcher.md` 2.3) says conforming matchers must not fill an order past its soft expiry
(`expiryDaa`, the IOC/FOK kill at `max(UTXO DAA, activeFrom) + 600`, or 90 days idle), and the reference indexer
leaves such orders out of its books. The covenants still accept the fill. `evQuote` reads only the UTXO DAA, the
interval, `activeFrom` and the custody DAA, so an order that expired when it was placed still counts as having
rested R. The arbitrage that DESIGN 3.3 and THREAT_MODEL A1 rely on to make a mispriced print costly ("matchers
cross a mispriced resting order in the next tick") therefore never applies to it, on any book.

Transactions: `exploit_expired_ask_counts_as_rested_evidence` liquidates a healthy long (market 10 KAS) on the
counterparty's own ask at 9.4 KAS with `expiryDaa = 1`, and repeats it with an IOC ask past its kill.
`exploit_expired_bid_closes_the_long_at_the_whole_range` lets a long close unilaterally (P5) on its own expired bid
at `bs`. It takes the whole range, and the short keeps only the carrier.

Who loses: the counterparty of whoever prints the quote. The amount is up to its whole margin, limited only by what
non-conforming takers would do. The covenant cannot see those takers, so this is rated High, not Critical.

Smallest fix: in `evQuote`, also require that the order stayed fillable for conforming matchers during the whole
rest window. That means `expiryDaa ≥ exposed + minRestDaa`, `tif == GTC` or `kill ≥ exposed + minRestDaa` for IOC
and FOK (`kill = max(UTXO DAA, activeFrom) + 600`), and `exposed + minRestDaa ≤ UTXO DAA + 77,760,000`. All three
read the evidence's own state, as `activeFrom` and `interval` already do. Record the rule in DESIGN 3.3 and
THREAT_MODEL A5. KOB's `KobCondAsk.touch` has the same gap.

### F3 (Informational). P7 with `n = s` keeps the loser's carrier

In P6 with `n = s` the loser's residual includes the carrier and any excess. In P7 with `n = s`, `partAndRest` moves
them into the new part, where they end up with the winner or the entrant. The entrant's own funded carrier becomes
the executor's. This is documented ("When n = s the moved part inherits the old carrier and the excess"), but the
loser is paid differently on the two liquidation paths, by one carrier (0.2 KAS) plus excess. Suggest paying `keep`
to the loser in P7 when `n = s`, as `closeDonor` does.

### F4 (Informational). Listing minimum on a split part

`transfer`, `closeHost`, `closeDonor` and `mutualClose` accept `n >= min || n == size`. A split made through the
other side's listing (or a partial P5/P6/P7) copies a listing into a part smaller than its minimum. That part can
then be filled whole below the minimum the lister set. Value is lost only through the dust rule: with `n = s` a
payout below 0.02 KAS stays in the part and goes to the entrant. No test, since the amounts are sub-dust per piece.
Suggest documenting the minimum as a per-UTXO rule, or clearing a listing whose minimum exceeds the part's size.

## Unproven

- A token covenant id carrying two templates with different suffix lengths would let `KobAsk.noStrays` (which
  locates the owner with the ask's own suffix length) miss a stray. `evQuote`'s `tk` could then be a stale UTXO
  owned by the ask, which shortens the rest. Not built: KCC20Ref keeps its outputs on its own template, and the
  harness has no second template under one id.
- Dust from the winner's pieces flowing to the loser: in P6 with `n < s`, a winner payout below dust stays in the
  remainder, and the final `n = s` piece gives the excess to the loser. This needs per-piece payouts below
  0.02 KAS. The total is bounded by `s/minSize · 0.02 KAS`, and each piece costs its own evidence fill and
  transaction fee, so it is not economic. Not built.
- KOB router intents in the same transaction: the review 1 reasoning (pairs never pay at `me` or `me + 1`) was
  re-checked and still holds. Not built, for the same reason as in review 1 (the router artifacts are not in the
  harness).

## Checked and found sound

- `onEvidence` path selector: path 5 alone checks the signature and fixes the evidence kind by side; paths 6 and 7
  check the trigger and the fee bounds. The arguments one path does not use do not reach another path. Changing the
  path or `nb` in a signed P5 transaction cannot change its signed outputs into another valid split.
- The shared hand-over (`handOver`) in P3 and P7: no-downgrade, the leaver's listing cleared in the part, the
  stayer's copied, `E` kept, the stayer's tag 0.
- Close-versus-close with `e3`: the host checks every output of its id (count 1 or 2, exact scripts), so the donor's
  P3 is either the host's P3 or an output under another id. The donor half alone, with P3 from a fresh genesis, is
  exactly a transfer of the donor's short: the long's payoff at every price and its idle payout do not fall, and an
  `e3` below the donor's entry is refused (`holds_close_donor_half_alone_is_only_a_transfer`). The host half alone
  is the mirror. The host's short is protected by `bl3 ≤ bl` and `e3 ≤ E(host)`. The host's P3 with `lk3` = a
  KpfOpen is an ordinary take.
- Claims after the tag change: a fill never shares an output with a take or a donor (their tags on one side differ,
  with 0 against a non-zero id). A take never shares one with a donor. Claimer ids are never 0, because an id-less
  input fails `OpCovInputCount(0) == 1`.
- `KpfOpen.take` on a part from a genesis rather than a pair: it is fully funded on the order's terms, and the `e`
  rule can only favour the order. Two pair outputs of one genesis claimed by two `pairOut` values are a gift from the
  executor.
- Evidence block: blocks that end at one `ev` nest, and a member naming another `q` refuses. KOB orders, KpfOpen
  entries, P2PK and token inputs cannot be block members (their first pushes are not two 8-byte items). Pairs of
  another `kBps`, `mmrBps` or `minRestDaa` each check their own bound. Pairs of another token, scale or commitment
  fail the evidence identity.
- Evidence state offsets (KobAsk and KobBid) and the custody layout were recomputed by hand. The KPF redeem-script
  offsets (209, 218, 227, 310..364, 430; KpfOpen 251) and the zero-tag window (66 bytes) were recomputed too.
- Output slots: the pair's own index and `M − 1 − i ≥ inputs` against KobAsk (maker at `ev`, token output at an
  argument index), KobBid (delivery at `ev`, refund at `M − 1 − ev`, which refuses a covenant input at that index)
  and KpfOpen (refund at its own index). A collision only invalidates the transaction.
- Arithmetic: trigger products, fee rate, block threshold and continuation sums stay in range within the bounds of
  `checkPair` and `checkMarket`. The block threshold overflows only where no evidence fill could satisfy it anyway.
- Fees: the winner never pays in P6 or P7 (the fee is capped at the loser's residual or payout), and dust thresholds
  at a whole-pair end send the carrier to a party, except with a market carrier below dust.
- Entry price: `E` is written only by T1, P4 (bounded on both sides) and genesis, and every other continuation copies
  it. `addMargin` and the hand-overs never lower the other party's idle payout.

## Not covered

GhostDAG races (X7), script budgets of the new shapes (X4), the KCC-20 program itself, and the economic size of F1
and F2 on a real book (X9).

## Status after fixes

The fixes implement DESIGN 4.5b. Proofs: `crates/kpf-tests/tests/audit2_tests.rs` (every former `exploit_*` test is
now a `holds_*` test), `kpf_layout_tests.rs`, and the extended X6 run. Full suite: 189 tests pass.

| # | Status | Shown by |
|---|---|---|
| F1 | Fixed. P6: the loser gets 0, the winner the liquidated value less the fee (with `n = s` the whole UTXO value, carrier and excess included; with `n < s` the remainder keeps the carrier, excess and slack as on every split). P7: the entrant's payment less the fee goes to the winner at output `i`, the loser gets 0. The fee is capped at the maintenance slice `q(n, T − BL)` / `q(n, BS − T)` computed from the pair's state. | `holds_self_liquidation_at_an_inflated_ask`, `holds_self_liquidation_at_a_deflated_bid`, `holds_self_liquidating_loser_receives_zero_at_any_quote`, `holds_winner_receipt_depends_on_the_quote_only_through_the_fee`, `holds_p7_winner_at_least_its_trigger_payoff` |
| F2 | Fixed. `evQuote` requires `expiryDaa ≥ exposed + R`, for any `tif ≠ 0` (as KOB applies the kill) `max(UTXO DAA, activeFrom) + 600 ≥ exposed + R`, and `exposed + R ≤ UTXO DAA + 77,760,000`. Offsets of `tif` and `expiryDaa` (KobAsk 154, 172; KobBid 187, 205) are checked against the compiled KOB templates. | `holds_expired_ask_is_not_rested_evidence`, `holds_expired_bid_cannot_close_the_long_at_the_whole_range`, `holds_expiry_boundary`, `holds_ioc_and_fok_kill_boundary`, `holds_ninety_day_idle_boundary`, `kob_evidence_offsets_match_the_compiled_templates` |
| F3 | Accepted as documented (DESIGN 4.5b). | — |
| F4 | Accepted as documented (DESIGN 4.5b). | — |

Residual of F1. The loser that liquidates itself still collects the keeper fee as executor, as any keeper would.
At a free quote (the top of its window) that fee is about `liqFeeBps / mmrBps` of the maintenance slice (10 % in
the DESIGN 5 market: 47.37 KAS against 42.50 KAS for an honest keeper at 8.5 KAS, against 473.68 KAS before the fix).
A market with `liqFeeBps ≥ mmrBps` would let the loser keep the whole slice this way; no bound between the two is
enforced (not in 4.5b).

Fee and quote. The fee rule is proportional to the quote, so for a short loser a higher bid (against the loser)
gives a larger fee and a slightly smaller winner receipt; the receipt never falls below the winner's payoff at `T`.

Deviation (`// DESIGN:` in `KpfPair.sil`). `T` is clamped to `[BL, BS]` before the cap is formed. Without it a pair
whose range is narrower than its maintenance (for example `mmrBps = 4999`) has a cap above the part, and the
winner's receipt could be negative; with it the winner receives at least its payoff at the clamped `T` (0 when the
pair is liquidatable from birth). Also documented: in P7 the winner's cash is `max(0, payment − fee)`.

Winner-triggered liquidations (now the winner takes the loser's whole residual). Checked and refused: the wrong side
of the evidence (a bid for a long, an ask for a short, side values other than 1 and 2), a look-alike or foreign-custody
ask, a quote of another token, scale or extension commitment, a fill reused by a second pair or a pair outside its
block, an evidence index pointing at another pair, and the trigger boundary (exact and inexact `T`). Rounding of `T`
moves only the fee cap and always in the winner's favour. No exploit found (`holds_winner_cannot_*`,
`holds_trigger_boundary_and_rounding_of_t`).

Script size of `KpfPair`: 11,246 → 11,567 bytes. Script units per path (before → after): P1 272,437 → 277,252;
P2 261,012 → 265,506; P3 256,189 → 263,254; P4 host 244,727 → 251,468, donor 233,626 → 240,046, mutual 138,251 →
142,103; P5 long 245,682 → 249,422, short 275,932 → 279,672; P6 long 175,747 → 179,339, short 145,506 → 149,340;
P7 long 305,039 → 312,627, short 274,970 → 282,552; P8 23,329 → 23,971; P9 223,585 → 224,227. Eight liquidations on
one evidence: 1,598,895 script units, compute mass 125,460 / 500,000, 100,550 / 250,000 bytes. `KpfOpen` unchanged.
