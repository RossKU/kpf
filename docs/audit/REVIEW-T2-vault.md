# Review T2 — the vault of a token leg (the bearer rule with a real vault, at the engine)

Scope: ONE round of ONE feature, the script-engine custody of KCC-20 tokens for a covered account — the vault
(`KpfTAddSpot`, `KpfTVaultPay`; `KpfSlot`; the reference `KCC20Ref` running beside ours). I took over what
rounds 66 and 67 left unrun (docs 16.24, 16.25): the bearer rule through a split, a hand-over, a step/sale and a
call's step with a real vault and the vault's payment at each end (A2); the time of a deposit (A3); and a
selection of what round 66 read and did not run (A4/A5). My file is `crates/kpf-protocol/tests/auditT2_vault.rs`
(9 tests, its world after `thin_draw.rs`). No src, contract, `Cargo.toml`, other test, other doc or `kpf-out`
file was touched; no commit; no network/node.

## Verdict

I found no path to anybody's tokens in what I ran or read. With a real vault born by a genuine deposit, the
token leg survives a split, a hand-over, a sale of an owing account and a call's step, and in every case the
vault pays exactly the ticket's key (the user, the taker, or the sale's claimer) and then ends — nothing goes
on under the vault's id, and the tokens go to that key whole. Cross-vault payment is refused (a ticket of leg
A cannot pay vault B). The deposit's at-will time, the discount floor/ceiling, the amount edges, a look-alike
token program (one body byte, P2SH fixed), and an ill-formed/owing/six-leg account are all refused. The
findings below are Low / informational: one is a defence-in-depth note (a token-leg split is refused at the
row point, not at the statement's end), one clarifies the deposit's time semantics, and two record known /
reasoned facts about the carrier KAS and token delegates. Custody holds. I did NOT run a true `KpfTJoinC` of
two token-bearing accounts, nor several A4/A5 cases listed under "Not reached"; those are READ only.

## Findings

### V1 (Low, informational; RUN) — a split that names the token leg is refused at the ROW point, not at the end

docs 16.12(c)/N28 say a token leg's row is wrong where kind 8/9/12/13 names it. At the model, the ROW is indeed
judged wrong: `c_row_judged_of(KIND_C_SPLIT, …, token_leg, …) == Some(false)`. But the statement's **end**
(`Account::c_end`/`c_split_end`, point 0) *accepts* a split that names the token leg: it returns a valid
outcome that moves the token leg into a new account of the same user (`after` keeps the futures long, `split`
holds the dir-7 token leg). The refusal therefore rests entirely on the row dispute (`KpfTCRow`), which I ran:
on a real deposit (account `[long, token]`), I stated the token-leg split with the end's own outputs, pointed
at the token leg's row (point 3), and `KpfTCRow` answered it **false** at the engine, killing the statement.

- Scenario (RUN): deposit 100 tokens into `[long(10u)]`; `c_row_judged_of(12, …, token_leg)` = `Some(false)`;
  `c_end(12, skip=2)` = `Some(Some(COutcome{ split: token-leg account, …}))`; engine row dispute at point 3 =
  verdict false, refused.
- Why this is Low: the token leg's `chain_of_one` and its binding to the vault (committee = vid, amount) are
  unchanged by such a move, so even an *undisputed* token-leg split would only relocate the leg within the same
  registry id, and the vault would still pay only a ticket of that exact leg — no tokens are lost or redirected.
  It is a defence-in-depth gap: custody does not depend on the end rejecting it, only on the row being disputed.
- Direction: consider having `c_split_end` (and the engine's `KpfTCSplit` end) reject outright when the named
  leg is `DIR_SPOT`, so the token-leg split is refused at the end as well as the row — belt and braces.

### V2 (Low, informational; RUN) — the deposit holds `f` to the account's CURRENT at-will time, not the W it writes

A2/A3's premise "a deposit whose input `f` landed before W is refused" is true only for the W the account
*already carries*, not the W the deposit *writes*. `KpfTAddSpot` requires some input to land at/after
`wt = mv[85..93]` (the account's existing `expiry`) and then writes `W = max(wt, account.daa + lateness)`. For
a fresh account (`expiry = 0`) any funder coin passes, which is correct — docs 16.12(d) says more margin is
safe at once. The written W governs the *next* at-will action, matching `Account::at_will` and `KpfTPass`.

- Scenario (RUN): fresh account (daa 6100, expiry 0) → W written = 6200 (`= daa + lateness`), and the funder
  coin's age is unconstrained. A fabricated account with `expiry = 9000` is refused when funded by a coin at
  8000 (builder) and, built honestly and funded at 9000, refused at the engine when input 3's daa is set to
  8999. Two deposits in a row: the second, on the account whose written W is 6200, is refused with the account
  and coin at 6199 (builder) and accepted with a coin at 6200 (engine).
- Direction: none needed; recording the exact semantics so the next reviewer does not mistake the bound.

### V3 (Low, known/accepted; READ, confirmed by round 66) — the vault's and custody's carrier KAS go to whoever pays the ticket

The vault output and the custody output each hold ~1 KAS (`VAULT_VALUE`). `KpfTVaultPay` ends the vault (no
output under its id) and pins only the *token* output to the ticket's key; the two carrier KAS, less fees and
what the paid output holds, fall to the payer (round 66 measured +1.187 KAS). This is the intended keeper
reward / service fee (~1.2 KAS) borne by whoever funded the deposit; it hurts nobody but the funder, and only
to that extent. I did not re-measure; I confirmed the shape (no output under vid; tokens to the key only).

### V4 (Low, informational; READ, not run) — the custody is authorized as the token's leader by the vault's presence; a stray delegate is the payer's own

In a payment, `token_in` spends the custody with `PATH_NORMAL` and an empty witness; `KCC20Ref.transfer`
(leader) authorizes it under `OWNER_COVENANT_ID` (scheme 4) because `OpCovInputCount(vid) > 0` — i.e. the vault
is spent. Our script pins the custody input's state (owner = vid, scheme `0x01040100`, amount, ext) and the one
token output to the ticket's key of the vault's amount. It does not bound the number of token inputs/outputs,
so a payer could add its *own* delegate tokens (under the same token id) and a second token output to itself;
those are the payer's own and return to the payer (KCC20 conserves the sum, and our output takes exactly the
vault's amount to the key). No third party loses. READ only — I did not build a stray-delegate payment.

## What was run, per item

| Item | Case | At engine? | Result |
|---|---|---|---|
| A2(i) | deposit → split off the futures leg (token leg stays) | engine | taken |
| A2(i) | a split that NAMES the token leg (row dispute, point 3) | engine | refused (V1) |
| A2(i) | tokens leave the staying account (kind 14) → vault pays the user; vault ends | engine | taken / vault ends |
| A2(ii)/A4(a) | two deposits → two vaults (distinct ids & chains); each ticket pays its own vault | engine | taken |
| A2(ii)/A4(a) | a ticket of leg A under vault B's rid; leg B shown with A's chain | builder | refused |
| A2(iii) | hand-over (`KpfTPass`) of a token-bearing account; old user's kind-14 claim | engine / model | taken / refused |
| A2(iii) | taker states kind 14 → vault pays the taker; vault ends | engine | taken / vault ends |
| A2(iv) | owing account sold from (kind 15) → vault pays the claimer; owed note applied to shard | engine | taken |
| A2(iv) | `KpfTOwed.pay` clears the debt → tokens then leave to the user | builder / model | taken |
| A2(v) | call's step (`KpfTCStep`): account goes on with its token leg, owing the rest; note names futures leg alone | engine | taken |
| A2(v) | the owing account then sold from → vault pays the claimer; vault ends | engine | taken |
| A3 | W written = `max(expiry, daa + lateness)`; fresh account's `f` unconstrained | builder | as model |
| A3 | future-W account funded before W | builder + engine | refused |
| A3 | two deposits in a row — the second must wait; funded at W | builder + engine | refused / taken |
| A4(b) | look-alike token program (one body byte, P2SH fixed): template-hash check | engine | refused |
| A4(b) | token redeem not matching its P2SH | engine | refused |
| A4(b) | amount `2^63-1`; amount 0; discount < floor; discount > 10000 | builder | refused |
| A4(c) | account that owes; account of six legs | builder | refused |
| A4(a) | vault / leg / custody of an amount the others do not name | builder | refused |
| A4(a) | tokens' output to another key, fewer, or owned by the vault's id again | engine | refused |

## Not reached (READ only, or not attempted) — stated plainly

- **A2(ii) a true `KpfTJoinC`** of two deposited (token-bearing) accounts, each ticket then paying its own vault
  and refused by the other. I ran the custody-relevant part (two vaults, cross-ticket refused) but did NOT run
  an actual join of two token legs into one account.
- **A4(a)**: a non-ticket note of the same registry id at the ticket's place (the builder is typed to `Ticket`;
  READ: the state-hash equality `blake2b(i9(12)+i9(14)+p33(chain)+p33(key)) == ns[2..34]` is the gate — a note
  of another kind hashes otherwise); the same ticket "twice" in one tx (`OpCovInputCount(rid)==1` forbids a
  second rid input); a stray second token output / delegate in a payment (V4). READ, not run.
- **A4(b)**: a continuation under an EXISTING vault's id as the born output (round 66 read: the recomputed
  `covenant_id` equality fails because the outpoint is new); `ti` naming the account, the script copy, or a
  plain coin (the `OpInputCovenantId(ti) == token` equality fails). READ.
- **A4(c)**: a leg whose bytes are not a canonical leg (the `leg` arg is a fixed `byte[111]`; the dir/scale/
  price/amount requires bound it); the "marked" 180-byte account and a shard in the account's place (the
  moving-part length and the second-key / owes-nothing checks in `spot_add` refuse). READ, not run.
- **A5**: the token's borrow path on a custody; index aliasing among `ti/vo/co/po/f/ga/j/n` (READ: `n != j`,
  `cov(ti)==token`, `cov(n)==rid`, and the `OpCovInputCount/Output(vid|rid)` counts pin the roles — I found no
  alias that skips a check); whether `KpfTVaultPay` can burn/redirect the vault's KAS beyond the payer's reward
  (V3). Analyzed by reading; not run.

## The workspace

`CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2`: **639 passed, 0 failed** (summing every
`test result` line). No failures, so there are no names to give. It is round 67's 630 plus my 9 new tests in
`auditT2_vault.rs`. (While iterating I ran only `--test auditT2_vault`, one cargo command at a time.)

## Findings ledger

| Finding | Severity | State |
|---|---|---|
| V1 (token-leg split refused at the row, not the end) | Low | open, defence-in-depth; row dispute runs at the engine |
| V2 (deposit's `f` bound to the pre-deposit W) | Low | informational; semantics recorded, benign |
| V3 (carrier KAS to the ticket's payer) | Low | known/accepted (round 66); service fee on the funder |
| V4 (stray token delegate is the payer's own) | Low | READ; no third-party loss |
| N23, N24, N25, N27 | High/Medium (prior) | as rounds 66/67 left them; re-confirmed consistent at the engine |
