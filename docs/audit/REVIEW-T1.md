# KPF review T1 (the thin family): incomplete

## Status

**This round was not finished, and nothing in it is proven by a test.** I read `contracts/KpfSlot.sil`,
`KpfTSettle.sil`, `KpfTBoth.sil`, `KpfTAdd.sil`, `KpfTDeclare.sil`, `KpfTDispute.sil`, `KpfTLate.sil`, `KpfTProve.sil`
and `KpfTCure.sil`, and the payout lines of the other scripts. I wrote no audit test file and ran no cargo command
for this round. Part of my work on lead 3 (money conservation and the payout index rules) was stopped by a safety
filter on my side before I could finish or report it; I do not report on that lead here. It should be treated as
**not reviewed**, and reviewed independently.

Everything below is by reading only. The ratings are tentative.

## Observations (by reading, unproven)

| # | Tentative severity | Statement |
|---|---|---|
| T1-O1 | Medium | A proof needs the tickets of the claim's tick, and the two clocks are unrelated: the claim, the dispute and the proof take up to `lateDaa` + two windows of the account, while a ticket may be retired by anyone an epoch after its own end and the window may be as long as a day. Where `lateDaa + 2 x window` exceeds the roster's epoch, a true claim that is disputed late can be left without a ticket to prove it, and lapses. Nothing in the model's `check` or (as far as I read) in the tool compares an account's window with its rosters' epoch |
| T1-O2 | Low | `KpfTDeclare.declare` bounds `t0` by `0 <= t0 < LOCK_TIME_THRESHOLD` only, and `KpfTProve.prove` does not compare `t0` with the claim's landing: a claim may name a tick that has not happened yet. A watcher cannot check such a claim when it lands. Requiring `tx.daa >= t0` in `declare` would make every claim a statement about the past |
| T1-O3 | Low | `KpfTDispute.dispute` requires `bond > 0`. A claim whose bond is 0 (a bond rate of 0 in a hand-made account, or a pot so small that the rate rounds to nothing) cannot be disputed and is paid as declared. The model's `check` refuses a rate of 0; whether every path that loads a foreign account file runs that check I did not verify |
| T1-O4 | Note | `KpfTProve.prove` requires `due == amount` exactly at the attested prices the claim names. A claimer that declares less than it is due, to be safe, has declared something it cannot prove |
| T1-O5 | Note | A claim that is late by its own `t0` is restored by anyone with the claimer's bond returned whole (`KpfTLate`), and `declare` asks no age of the account: a party can put its account into a late claim again and again for fees alone, and the other party restores it for a fee each time |
| T1-O6 | Note | `slotOf` checks that the named input is the P2SH of the script it is shown, not that the script is a `KpfSlot` of this library. For an object that names the script through `KpfSlot.go` this is the object's own redeem script, so I see no harm by reading; I did not test it |
| T1-O7 | Note | The preimages of the kinds have different lengths (account 153 bytes, claim 87, liquidation 129, proved liquidation 42, no-price claim 69) except the two disputed kinds (66 each), whose first hash is over preimages of different lengths. I found no confusion by reading, provided `byte[120]`, `byte[32]` and `byte[312]` arguments are held to their widths by the compiled scripts, which I did not test for these scripts |

## What I did not get to

- Lead 3 (conservation and the index rules of every entry, alone and with several objects in one transaction):
  not reported; see Status.
- Lead 1 beyond reading `KpfSlot` and the `who` checks of nine scripts: no engine test of two scripts in one
  transaction, of one object named by two scripts, or of a script input shaped for an object its entry does not
  handle; the library's genesis and what the tool checks of it.
- Leads 4 and 5 beyond the notes above: the bond's split in every lapse, early and refute path; churn on claims.
- Lead 6: `KpfTLiqProve` against the model (`Cross::liquidatable`, `fee_of`, `cure_add`), the one-leg account, a
  price above the limit, side B; no fuzz was written.
- Lead 7: the ticket check in the three proving scripts and the builders' `tickets_in` / `ticks_in`.
- Lead 8: `tcmds.rs` (`load`, `TObj::decode`, `acct-show`'s CHECK lines, `acct-dispute`'s warning, the opening's
  payload).
- Lead 9: the legacy changes (`ODraft`'s shared genesis and payload, `x_att`, `cross_from_args`).
- `KpfTLiq`, `KpfTLiqSettle`, `KpfTLiqDispute`, `KpfTLiqProve`, `KpfTQuiet`, `KpfTQuietEnd`, `KpfTQuietRefute`
  were not read beyond their payout lines.

## Judgement

None. The thin family has still had no independent review: this file records where one began and what it noticed
on the way, and should not be read as clearing anything.
