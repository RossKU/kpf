# KPF review 6

## Scope and method

Target: the oracle-market family as it stands in the working tree: `contracts/KpfOpenO.sil`, `KpfPairO.sil`,
`KpfPendingO.sil`, `KpfBond.sil`, `KpfStake.sil`, the library (`oracle.rs`, `oracle_build.rs`, `oracle_artifacts.rs`,
`bond.rs`, `judge.rs`) and the oracle commands (`crates/kpf-cli/src/ocmds.rs`). Read against `DESIGN-ORACLE-IMPL.md`
sections 1-23 (with `DESIGN-ORACLE-2.md` and `DESIGN-ORACLE.md` as background), `DESIGN.md` 3.1, 3.4, 4.5a-4.5g, and
the failure patterns of reviews 1-5. The covenant rules were read in the pinned rusty-kaspa v2.1.0
(`crypto/txscript/src/covenants.rs`, `CovenantsContext::from_tx`): **any input that carries a covenant id authorizes
continuation outputs of that id, whatever its script**, and a P2PK UTXO can carry a covenant id.

Method: every claim below is an engine run of the real compiled scripts with the covenant context
(`common::run`), or a run of the `kpf` binary offline. New files only: `crates/kpf-tests/tests/audit6_tests.rs`,
`crates/kpf-cli/tests/audit6_cli.rs`, this report. Both new test binaries pass
(`CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --test audit6_tests --jobs 2`, `-p kpf-cli --test audit6_cli`); every
`exploit_` test asserts that the attack is accepted. Nothing in `contracts/`, existing docs, tests or sources was
changed.

## Findings

| # | Severity | Title | Tests |
|---|---|---|---|
| R6-F1 | Critical | A part whose covenant id someone else also holds: the holder forges the judge's dispute and pool under that id (contested false claims upheld, true claims refuted, every receipt stolen) | `audit6_tests::exploit_take_into_a_held_id_lets_a_forged_dispute_uphold_a_contested_false_claim`, `exploit_a_held_id_refutes_a_true_claim_and_takes_the_underwriters_bond`, `exploit_a_held_id_lets_a_forged_pool_take_every_honest_receipt`, `exploit_close_donor_into_a_held_id_moves_the_long_beyond_the_judge`, `exploit_an_order_taking_a_genesis_pair_with_a_twin_lands_under_a_held_id`; control `holds_a_pair_opened_by_fill_has_no_handle_for_a_forgery` |
| R6-F2 | High | Receipts of two disputes of one pair lineage with the same anchor settle in each other's pools | `audit6_tests::exploit_receipts_of_two_disputes_with_one_anchor_settle_in_the_wrong_pool` |
| R6-F3 | Medium | The lapse settle, the one exit "that needs nobody", is blocked forever by a party that amends once per `idleDaa` | `audit6_tests::exploit_a_party_blocks_the_lapse_settle_forever_by_amending` |
| R6-F4 | Medium | `oracle-check` raises no warning on a market whose judge cannot be reached (1,000,000 KAS stakes, 13.6-year stake age and rounds) | `audit6_cli::exploit_oracle_check_raises_no_warning_on_an_unreachable_judge` |

No Low finding. Notes follow the findings.

### R6-F1 (Critical). A part under an id someone else holds: the judge can be forged

**Affected:** `KpfOpenO.take`; `KpfPairO.closeDonor`; `KpfPendingO.decide`; `KpfStake.resolve`, `settle`, `redeem`,
`close`; DESIGN-ORACLE-IMPL 16 ("Contest and verdict"), 20.8, 21 ("look-alike pending parts and disputes: holds"),
22 (public wording).

**The unstated assumption.** The judge identifies everything by the contested part's covenant id: the dispute and its
receipts take the part's id; `decide` accepts any `KpfStake` dispute of that id that is spent by `resolve(self)`;
`resolve` accepts any claim input of the id whose script hash it recorded; `redeem` accepts any resolved pool of the id
with the receipt's anchor. That is safe only while every UTXO of the id is a template instance whose entries count the
id's outputs exactly. For a pair born by `fill` this holds (the twin scan and `OpCovInputCount(pairId) == 0` at the
genesis, and every entry of the three templates and of `KpfStake` counts the id's outputs; checked entry by entry).
It does not hold for a part created in an id that was not born that way, and three entries put a party's position
into exactly such an id:

1. **`KpfOpenO.take`** checks only `OpOutputCovenantId(partOut) != 0`. The part's id can be authorized by any input of
   that id, e.g. the attacker's own P2PK UTXO that carries the id (made by an ordinary genesis,
   `make_handle`), which also authorizes its own continuation: the attacker keeps a **handle** on the part's id.
2. **`KpfPairO.closeDonor`** checks P3's script, its binding bounds and its value, never the host: the "host" can be
   the attacker's P2PK UTXO of an id it holds. The donor's long, which signed nothing, is moved into that id.
3. **A genesis look-alike pair with a P2PK twin** (no scan runs at a genesis no order takes part in). An order taking
   its listing through the pair's own `transfer` with the fresh bind (the protection 20.8 relies on) lands under the
   twin's id. So requiring a real pair input in `take` closes route 1 but not this one.

**What the handle does.** With one P2PK signature the holder writes any `KpfStake` state under the part's id at any
time, after seeing the contest:
- a dispute led by YES, round started at DAA 0, naming the honestly contested part's script: `resolve` + `decide`
  accept it at once and the part is **UPHELD** although it was contested in time; it finalizes into the forfeit;
- the same led by NO: a true claim is **REFUTED**, the part restored and the claim's bond paid to the defender and the
  disputer, both the attacker;
- a resolved pool with the honest dispute's anchor, the honest side as loser and a base equal to all it will hold:
  `settle` forfeits every honest receipt of any dispute on the part into it (anyone may settle; `redeem` checks id,
  status and anchor only), before or after that dispute's own resolution, and `close` hands the whole value to its
  closer with no burn;
- the honest dispute can then never resolve (its claim was decided by the forgery) unless the holder recreates a UTXO
  of the contested script under the id.

**Who loses what** (fixture pair; tests above):
- route 1, a resting long order whose owner is offline (`uwAccept` zero, the attacker's own slot as underwriter, its
  counterparty key a different key): a false liquidation at 9.20 contested in time by an honest staker is upheld; the
  order's owner loses its whole margin, **1,000 KAS**; the forfeited part (2,954.2 KAS) is the attacker's alone;
- the honest underwriter (route 1 with a trusted slot, or any route): every true liquidation of the attacker's side is
  refuted; **600 KAS** of the underwriter's bond per claim to the attacker (480 as defender, 120 as disputer), and the
  long never receives its due liquidation;
- every bystander who contests or defends any claim on such a part: its whole committed stake (**10 KAS** in the test,
  without limit in general) to the attacker;
- route 2, the donor's passive long, **1,000 KAS**, with a colluding underwriter (the stake theft needs no collusion);
- route 3, the taking order's owner, **1,000 KAS**.

This contradicts DESIGN-ORACLE-IMPL 20.8 ("an entrant through an order is protected on chain"), 21 ("no output can
carry an existing part's id without that part as an input"; "the same claim contested in time, judge honest: the
parties lose nothing") and the public wording "A false price contested within the window is reversed". The per-pair
loss is still bounded by the pair's margin, but the contest no longer protects it and stakers' and underwriters' funds
are exposed beyond any documented limit.

**Smallest fix.** (a) `take`: require exactly one input of the part's id and that it be a `KpfPairO` instance
(`tplState` on `PAIR_TPL`) spent by `transfer`; (b) `closeDonor`: the same for the input carrying P3's id, spent by
`closeHost`. (a) and (b) close routes 1 and 2 but not 3. The root fix is to make the dispute unforgeable by a holder of
the claim's id: let `contest` create the dispute under a fresh genesis id checked with the twin scan of `fill`, record
that id in the contested state, let the dispute record `(claim id, P, disputer key)` read from the claim's state
instead of the contested script's hash (this breaks the cycle of IMPL 12.5), let `decide` require
`OpInputCovenantId(d)` to be the recorded id, and let receipts carry the dispute's id. Until then the documentation must
say that an order (or a donor's long) entering a pair whose genesis it cannot inspect has no contest protection.

### R6-F2 (High). Receipts of two disputes with one anchor settle in each other's pools

**Affected:** `KpfStake.settle`, `redeem`; IMPL 12.5 ("receipts carry the anchor so that an earlier dispute's receipts
of the same lineage never settle against a later one").

**What happens.** The anchor is the contested part's own DAA score. Two parts of one lineage (a part and the remainder
after any split: a partial transfer, take, propose, or a restored part) share the id; two claims proposed on them in two
independent transactions that the same chain block accepts get the same DAA score, so their disputes have the same id
and the same anchor. `settle` checks the receipt's id, status, market and anchor, never which dispute it joined. The
test builds exactly this: dispute A (a false claim, refuted) and dispute B (a true claim, upheld), both anchored at 700.

**Who loses what.** The attacker defends claim A with 20 KAS on YES and loses A. It settles that losing receipt in pool
B (where YES won) as a winner: paid its **20 KAS back plus all of B's winners' share, 9 KAS**. B's honest YES winner can
no longer be paid in B (its weight is used up, refused) and is forfeited into pool A as a loser instead: it loses
**29 KAS** (its 20 KAS stake and its 9 KAS share). In general any loser of one dispute steals a winner's place in the
other, and any receipt can be forfeited into the wrong pool by anyone.

**Reachability.** Every UTXO created by the transactions a chain block accepts gets that block's DAA score
(`block_daa_score`); two independent proposals in one block, or in blocks merged by one chain block, do. A keeper
liquidating both parts of a split pair at the same moment does it naturally; an underwriter can arrange it.

**Smallest fix.** Write the dispute's identity into each receipt (e.g. the dispute's `spkHash` instead of the zero
field at `open`, `deposit` and `commit`) and require it equal in `settle` and `redeem`; or the dispute id of R6-F1's fix.

### R6-F3 (Medium). The lapse settle can be blocked forever

**Affected:** `KpfPairO.lapseSettle` (`this.ageDaa >= idleDaa`), `amend`; IMPL 17 (5), 20.5, 22 ("once its cover has
lapsed anyone can settle your pair at its entry price").

**What happens.** `lapseSettle` needs the pair UTXO `idleDaa` old (CSV). `amend` needs only `quietDaa` and has no
condition on the cover, so a party re-creates the UTXO once per `idleDaa` and its age restarts each time. With a silent
underwriter there is then no claim, no lapse settle and no exit for the other party without the blocker's consent
(cooperative or mutual close at the blocker's price, or a taker for a pair nobody can settle).

**Who loses what.** The party the entry-price settlement favours (the side losing at the market) has its whole margin
held hostage indefinitely; the blocker pays one amend fee (0.028 KAS) per `idleDaa` (7 days on testnet). The test amends
a lapsed pair three times with sequence `quietDaa` only and shows each fresh UTXO refusing the lapse settle below
`idleDaa`.

**Smallest fix.** Measure the idle time by the calendar, not the UTXO: `tx.daa >= expiry + tailDaa + idleDaa` (CLTV),
or refuse `amend` and `addMargin` once `tx.daa >= expiry + tailDaa`.

### R6-F4 (Medium). `oracle-check` is silent on an unreachable judge

**Affected:** `market_warnings` (`oracle.rs`), `words` and `check_cmd` (`ocmds.rs`).

**What happens.** The judge bounds admit `minStake` up to 1,000,000 KAS, `M_age`, `U` and `D_r` up to `2^32 - 1` DAA
(13.6 years at 10 DAA/s). `market_warnings` flags only a small stake and a short age. A market with the largest values
passes the bounds; `oracle-market` and `oracle-check` print no warning, the words never state `minStake`, and they still
say "A false price stands unless someone contests it within 6.00 h". Stakes are per market (the judge block carries the
market id), so on such a market no stake can count for 13.6 years after it is created, a contest costs 1,000,000 KAS of
aged stake, and any contest freezes the part for 13.6 years.

**Who loses what.** An honest user who opens or rests an order there on the strength of the check: up to its whole
margin to the first false claim, with no possible contest.

**Smallest fix.** Print `minStake`; warn above a stake that a typical defender can post (for example 1,000 KAS), above
an age of some weeks and above rounds of some days; and say that a new market's judge has no aged stake for `M_age`.

## Notes (informational)

- **A new market has no judge for `M_age`.** Stake counts only in its own market and only if accepted `M_age` before
  the claim; every claim in a market younger than `M_age` (7 days at the defaults) is uncontestable. True by design,
  not stated in IMPL 22 or by `oracle-check`.
- **A late contest freezes a claim for free.** A contest landing after `P + W` but before the finalize is ignored for
  the verdict, yet the part waits for the dispute's round, and the opener gets its stake back when nobody answers: one
  round of delay (12 h at the defaults) per claim at the cost of fees.
- **`oracle-watch` reads the DAA from the object file** and never from the node even with `--node`; a claim file whose
  `daa` was edited can show a valid claim as void or its window as closed (not tested, see below).

## Unproven

- R6-F2 relies on my reading of consensus that every UTXO created by the transactions a chain block accepts carries
  that block's DAA score; the engine test assigns the equal scores. Not run on a node.
- That an honest underwriter's bind service would sign a `take` whose inputs include a P2PK input of the part's id
  (route 1 with `uwAccept` set to that underwriter): plausible for an automated binder, not tested.
- `oracle-watch` showing a valid claim as void from an edited `daa`: read in the code, no test written.
- Overflow inside the bounds: the payout and premium arithmetic were read, not fuzzed beyond the existing replays.
- Compiled script versus source: relied on the existing `holds_the_engine_templates_are_the_pinned_artifacts`; not
  re-derived independently.

## What was checked and found sound

- The vouch: an input of `uwKey` with exactly one 65-byte push ending in SIGHASH_ALL; it commits to every outpoint
  (so this pair's state), every output (the claim, the bond) and the lock time (`t0` by CLTV); replay after a split,
  transfer, restore or cure fails (new outpoint). `quietDaa` after every party entry; `propose` and `renew` need none.
- Binding at `fill`: the bind proof's offsets match `KpfBond.bind`'s 17 argument pushes, the dispatch tag and the exact
  length; claims by ref are unique per bind; the twin scan covers every output (at most 8) and the pair id has no input.
- Every entry of `KpfPairO`, `KpfPendingO` and `KpfStake` that continues an id counts that id's outputs exactly and
  fixes each one's script (enumerated: amend, addMargin, transfer, closeHost, closeDonor, mutualClose, lapseSettle,
  coopClose, renew, propose; contest, decide, cure, finalize; open, deposit, resolve, settle, close, redeem); so a
  fill-born lineage has no handle (`holds_a_pair_opened_by_fill_has_no_handle_for_a_forgery`).
- The range stays within `coverRate` in every price-free entry; the covers of split parts round down and sum to at
  most the counted cover; renewal only by `uwId`/`uwKey`, exactly the agreed premium, the range kept wider than both
  bands.
- The contest window after the fact (`c >= clock + W` ignored), `P + W <= X <= P + term + step`, `P - t0 <= lateDaa`:
  a timely contest and a finalize cannot both be valid; a claim on a lapsed pair is void
  (`holds_no_claim_is_valid_on_a_lapsed_pair`).
- The bond: posted with the claim, paid to the proposer (valid), the defender (void) or the defender and the disputer
  (refuted), never from the slot; restore, cure and forfeit states fixed at the proposal.
- Cross-family: the oracle and book templates pin each other's hashes only within their family; `OObj::decode` accepts
  only the pinned oracle templates, the book commands only the book's; no object of one family satisfies a template
  check of the other.

Not reached: `KpfBond.split`/`unbond` beyond the existing Z3 replay; the multi-dispute judge strategies beyond Z10; the
mass limits of the largest shapes; `cure` edge cases at `priceMax`; the conservation replay was not re-run.

## Are the documented limits stated truthfully?

For pairs born by `fill` and never moved into another id, the accepted limits are stated accurately and the numbers
reproduce: an undisputed false claim takes at most the loser's margin, bounded by `rho x K` per underwriter; a silent
underwriter turns a due liquidation into an entry-price settlement; back-dated renewals and the Sybil underwriter are
as described. The documentation is not truthful about two things. First, it claims on-chain protection for every
entrant through an order and says no output can carry an existing part's id without that part as an input: both are
false for a part whose id another UTXO holds (R6-F1), which `take`, `closeDonor` and genesis look-alikes produce, and
there the judge, the documented answer to every false claim, can be bypassed and the stakers robbed. Second, "once its
cover has lapsed anyone can settle your pair" holds only while the counterparty does not refresh the UTXO (R6-F3), and
the receipts' anchor does not separate disputes of one lineage as IMPL 12.5 says (R6-F2). The public wording "A false
price contested within the window is reversed" must be withdrawn or restricted until R6-F1 is fixed.
