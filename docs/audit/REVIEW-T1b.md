# KPF review T1b (the thin family): property tests

21 November 2026. Scope: `contracts/KpfSlot.sil`, `contracts/KpfT*.sil` (15 scripts, 19 entries),
`kpf_protocol::thin` (the codec) and, for three findings, `kpf_protocol::thin_build`. Everything below was run; an
invariant is said to hold only where a test of this review built the violating transaction and the engine refused it.

Tests (26, all passing; a test named `t1b_finding_...` asserts an acceptance and so documents a break):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 18 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 8 tests
```

Engine level (`crates/kpf-tests/tests/auditT1_tests.rs`): 19 scenes, one accepted transaction for every entry that
reads one object (`add`, `both`, `declare`, `settle`, `late`, `dispute`, `lapse`, `liquidate`, `liq-late`,
`liq-settle` claimed and proved, `cure` in its three states, `liq-dispute`, `liq-lapse`, `quiet`, `quiet-early`,
`quiet-end`), then 2,313 changed transactions each expected to be refused (or, for the controls, accepted).
Builder level (`crates/kpf-protocol/tests/auditT1_builders.rs`): signed and metered through `verify::verify`.

## Result by invariant

| Id | What was tried | Result | Test |
|---|---|---|---|
| A0 | Every scene is accepted; its hashes are the model's | holds | `t1b_a0_every_scene_is_accepted_and_the_model_agrees` |
| A1 | A second object (a twin of the first, or an unknown one) spent beside the first and named at place 1 or 2, every arrangement of `who`, for all 19 scenes (494 transactions) | holds | `t1b_a1_a_script_that_reads_one_object_moves_one_object` |
| A1 | Two different scripts of the library naming the same object (44 pairs of the same kind): both must hold; only `KpfTBoth` beside another passes, and adds its two signatures | holds | `t1b_a1_two_scripts_naming_one_object_must_both_hold` |
| A1 | `KpfTProve`, `KpfTLiqProve`, `KpfTQuietRefute`, one leg and two: every other `who` over the transaction's inputs (1,017 triples), through the builders with the committee's real signatures | holds | `t1b_a1_the_scripts_that_read_three_places_take_one_who` (builders) |
| A1 | A script shown another object's state, another account's state, another fixed part | holds | `t1b_c_every_entry_refuses_every_other_kind_of_object` |
| A2 | `who` at the script itself, at the funding input (with every length a script could ask of it), out of range, negative; the object's length a byte off (228) | holds | `t1b_a2_who_at_the_script_itself_a_funding_input_or_no_input` |
| A3 | Sources: all 19 entries of the 15 scripts end by the four conditions of going on, none returns early | holds | `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` |
| A3 | The script a sompi short, under no id, under another id, under the object's id, replaced by another script or an object, paid to a key, at another index; a second output under the library's id (a copy, another script, an object, a key) (247) | holds | `t1b_a3_a_library_script_goes_on_unchanged_at_its_index_and_nothing_else_takes_the_id` |
| A3 | A library whose genesis wrote one output more than the family's scripts | **FINDING F4** | `t1b_finding_a_library_with_one_output_more_takes_every_object`, `t1b_finding_the_tool_reads_a_library_by_position_not_by_script` (builders) |
| A4 | `go` with k outside 0..2, l its own index, the funding input, out of range, negative; a script under no id or another id; an object of another library (266) | holds | `t1b_a4_slot_go_bounds` |
| A5 | The object younger than the age stated; a negative age; an age of 7 or 9 bytes; a sequence with its top bit set; for the six window entries a stated age below the window or none (176) | holds | `t1b_a5_the_object_is_as_old_as_the_script_states_and_the_script_states_the_window` |
| B | `KpfTCure` with every wrong state number on a claimed, disputed and proved liquidation; `KpfTLiqSettle` with the wrong `proved`, and a disputed liquidation under either; a side that is neither | holds | `t1b_b_state_numbers_and_flags_are_the_objects` |
| B | Widths: `who` of 0, 16, 23, 25, 32 bytes; `moving`, `fixed`, the disputer's, liquidator's and ender's key a byte short, a byte long, empty, of 64 and 119 bytes, the object's hash following the bytes shown so that only the width stands in the way (370; for the state and the keys the script's own input fails) | holds | `t1b_b_every_byte_string_has_the_width_the_entry_declares` |
| B | Another cut of a preimage: a claim, a liquidation (two cuts), a disputed claim and a disputed liquidation read as an account by `fixed`/`moving` of other widths; a dispute whose "key" of 119 bytes would make the disputed claim an account of the disputer's keys | holds | `t1b_b_no_object_is_an_account_by_another_cut_of_its_preimage` |
| C | Each of the 19 scenes given the real object of each of the six other kinds on the same account, with and without that kind's value (285): account operations on every claim, claim operations on an account, one claim's on another's | holds | `t1b_c_every_entry_refuses_every_other_kind_of_object` |
| D | Every output a script fixes a sompi short; the object written under no id or twice; `late` on a claim in time; `quiet-early` on a claim of the past; settle and dispute of a claim that is late or before its tick; `quiet-end` on a claim of a time to come; cure, add, declare, quiet signed by another key; both by other signatures; a disputed object settled, a proved or disputed liquidation disputed again (144) | holds | `t1b_d_a_rewrite_pays_what_the_entry_asks_and_every_output_is_whole` |
| D | A claim or liquidation of a tick long past or still to come: made, never at risk, ended with the whole bond back | **FINDING F3** | `t1b_finding_a_claim_of_a_tick_long_past_holds_the_account_for_nothing`, `t1b_finding_a_liquidation_of_a_tick_long_past_costs_its_maker_the_fees` (builders) |
| D | A side liquidates itself falsely for a cure of one sompi and cures before the window ends, three rounds, and under a dispute | **FINDING F2** | `t1b_finding_a_side_holds_its_account_by_liquidating_itself_and_curing_for_a_sompi` (builders) |
| (payments) | Two operations in one transaction whose payment places coincide | **FINDING F1** | `t1b_finding_one_output_pays_two_dues_of_one_key`, `t1b_finding_a_cure_beside_a_lapse_takes_the_disputers_bond`, `t1b_finding_which_entries_share_an_output` |
| E | `decode(encode(x)) == x` and the same hash and bytes, 300 random accounts times 7 kinds | holds | `t1b_e_decode_of_encode_is_the_object_for_random_objects_of_every_kind` (builders) |
| E | 17,182 proper prefixes, 1,008 extensions, 21,420 other kind bytes | holds | `t1b_e_decode_refuses_truncated_extended_and_cross_kind_bytes` (builders) |
| E | Whatever `decode` reads of a changed byte string encodes back to the bytes read and has another hash (33,600 single-bit changes, and the top bit of every stated byte) | holds | `t1b_e_what_decode_reads_encodes_back_to_the_same_bytes` (builders) |
| E | A claim with a side that is neither, a negative tick, price or amount, more than the pot, another bond; an account whose `lateDaa`, window or bond rate is out of range | holds | `t1b_e_decode_refuses_what_the_scripts_would_not_have_written` (builders) |

What each entry that writes an object back under its id asks (D, from the scenes; pot 5,000 KAS, bond 500):

| Entry | Who | Object's value | Paid out |
|---|---|---|---|
| `KpfTAdd.add` | the side, signed | + what is added (above 0) | |
| `KpfTDeclare.declare`, `KpfTQuiet.quiet` | a party, signed | + bond | |
| `KpfTLiq.liquidate` | anyone | + bond | |
| `KpfTDispute.dispute`, `KpfTLiqDispute.dispute` | anyone, a claim in time | + bond | |
| `KpfTLate.late`, `KpfTLiq.late` | anyone, a claim not in time | - bond | the whole bond to who made the claim (F3) |
| `KpfTDispute.lapse`, `KpfTLiqDispute.lapse` | anyone, a window old | - 2 bonds | 1.5 bonds to the disputer, 0.5 to the other party / the side named |
| `KpfTCure.cure` | the side named, signed, any state | - every bond + `add` (1 sompi is enough) | every bond to who posted it (F2) |
| `KpfTQuiet.early` | anyone, a claim before its `since` | - bond | half to who ends it, half to the other party |
| `KpfTBoth.both` | both parties, signed | anything | anything |

## Findings

**F1 (High). One output pays two dues of one key.** A script pays "at the mirror of the object's index"
(`outputs[len - 1 - j]`) and, where it pays two parties, "the output before it" (`len - 2 - j`), each checked with
`>=` for itself. The second place of the object at input j is the first place of the object at input j + 1. When two
operations are put in one transaction with their objects at neighbouring inputs and the same key is owed at both
places, one output satisfies both scripts: the key is paid the larger of its two dues, and whoever builds the
transaction keeps the smaller. Entries with a second place: `KpfTDispute.lapse`, `KpfTLiqDispute.lapse` (half the bond
to the other party), `KpfTCure.cure` under dispute (the disputer's bond), `KpfTQuiet.early`, and by reading
`KpfTQuietRefute`. The test found 11 accepted pairs among the single-object entries. Two shown whole, with nothing
left as fee: a lapse beside a settlement, where B is owed 250 + 2,930.2 KAS and is paid 2,930.2 (neither entry is
signed: anyone builds it, and the half bond that was to make a self-dispute cost something is not paid); and a cure
beside a lapse, where the disputer of both is owed 500 + 750 KAS and is paid 750, the side that cures keeping the
disputer's own bond of 500. The second is the worse one: a party can make false claims on accounts it controls, wait
for a watcher to dispute them with one key, and take the watcher's bond. It needs the same key owed in two objects
that can be ended at the same time; a fresh key a dispute avoids it on the disputer's side, not on a party's. A fix
that costs no output: the second payment at `len - 1 - self` (the script's own input index; object and script
indices are disjoint), or the two places at `len - 1 - 2j` and `len - 2 - 2j`.

**F2 (High). A side holds its own account for nothing by liquidating itself.** `KpfTLiq.liquidate` is unsigned and
states the cure it asks (`add`, anything above zero); `KpfTCure.cure` is signed by the side named, works in any state
and gives every bond back. So a side declares its own liquidation with a second key, false, in time, `add` = 1 sompi,
and cures it before the window ends. Through the builders, three rounds: the account was a liquidation claim for 147
of 180 DAA, for the fees (about 0.011 KAS a round) and 3 sompi that stayed in the side's own margin; the 500 KAS bond
came back whole each time. While the claim stands no other entry reaches the account (C: `add`, `declare`,
`liquidate`, `quiet`, `both` all refuse a liquidation), so the other party can declare no close and nobody can
liquidate the side for real. A dispute does not help: under dispute the cure gives the watcher its bond back and
nothing more, and the liquidator's whole (shown in the same test). The only thing the side must not do is miss a
window. Not tested (no node): that the side wins the race for the cured account, which it should since it can send
the cure and the next liquidation together. The design's note on the cure covers a liquidator caught by somebody
else, not a liquidator that is the side. Directions: a floor on `add` tied to the pot or to the bond, or a part of
the liquidator's bond forfeited to the other party when a claim is cured.

**F3 (Medium). A claim of a tick long past, or still to come, costs its maker the fees.** `declare` and `liquidate`
read no time. A claim that did not land within `lateDaa` of its tick is neither settled nor disputed, so its bond is
never at stake, and `late` gives the bond back whole. Tested for both and for both directions in the engine, and for
a liquidation of tick 1 through the builders (0.0046 KAS to make, 0.0048 to end). For `liquidate` the maker is
anyone. Unlike F2 anybody can end it at once, so it is a race each round and not a hold; but it is free, and
`KpfTQuiet.early` already takes the bond of the same kind of claim (one that names a time to come) and shares it.
Direction: `late` shares the bond as `early` does, at least for a claim that landed before its tick and for
liquidations.

**F4 (Medium). A library is what its genesis wrote, and nothing checks a genesis.** `KpfSlot` accepts any input that
carries the library's id and begins its signature script with the two pushes. In the engine: one more output under
the id, of a script that drops two items and leaves true, spends an object of every one of the seven kinds to a key
with no signature and no window, and goes on. The scripts cannot prevent this (it is the design: the id is the
family), so it rests on whoever opens an account in a library checking its genesis. The tool does not:
`thin_build::lib_scripts` names a genesis's outputs by position without comparing scripts (tested), `open` takes a
bare id, and no command reads a library somebody else made (by a search of `crates/kpf-cli/src`, not by a test).
Direction: a check that every output of the genesis under the id is `script_spk` of a pinned script, run before
opening and before co-signing an opening.

No finding in A1, A2, A4, A5, B, C, E: the place rule added after the probes holds for every single-object entry and
for the three proofs, the widths are enforced by the scripts themselves, and no object of one kind is read as
another.

## Not reached

- A3, B, C and widths (`st` of 312 bytes) at the engine for `KpfTProve`, `KpfTLiqProve`, `KpfTQuietRefute`: only their
  `who` (A1) and their sources (A3) were tested here.
- F1 with `KpfTQuietRefute` or a proof as one of the two operations: by reading only.
- E, `Claim::is_true` against the engine: left to `the_proofs_agree_with_the_model_on_random_accounts`, as asked.
- Anything that needs a mempool or a node: the races in F2 and F3 are reasoning.
- The command line (`acct-*`, `lib-create`): not run. That no command checks a library is from reading.
- Only one account shape at the engine (two legs, margins 2,000 and 3,000 KAS, window 50, bond a tenth): amounts at
  the edges (a bond of one sompi, a claim above the object's value, `MARGIN_MAX`) were not tried.

# Round 2

21 November 2026, after the fixes of F1 to F4 (contracts, model, builders, tool). The two test files were adapted to
the changed entries (`KpfTCure.cure` without `dkey` and with an age, `KpfTLate.late` and `KpfTLiq.late` with `rkey`,
every place from the end at `len - 1 - 2j`), each finding was tested as closed and attacked again, and what round 1
did not reach was covered. Round 1's `t1b_finding_...` tests are now `t1b_fixed_...` and assert the refusal; the
scene `cure-disputed` is gone (18 scenes) and is a refusal in `t1b_b_state_numbers_and_flags_are_the_objects`.

Result: F1, F3 and F4 are closed. **F2 is open by another way** (F2b below, High): a side that liquidates itself and
disputes itself has both bonds back at the lapse. Nothing new otherwise.

Tests (33, all passing):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 17 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 13 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture            # 3 tests (new file)
```

## Round 1's invariants on the changed scripts

All hold again, on 18 scenes: A1 468 + 42 transactions, A2 216, A3 234, A4 252, A5 172 (seven entries now ask an
age: `KpfTCure` of a claimed liquidation asks half a window), B 68 + 345 + 27 (the new `rkey` of the two `late`
entries is among the widths), C 270, D 137, E unchanged, the three proofs' `who` 1,017 triples. Same test names as in
round 1.

## The four findings

| Id | What was tried | Result | Test |
|---|---|---|---|
| F1 | Every ordered pair of the 11 entries that pay a key (121), objects at inputs (0,1), (0,2), (1,2) in both orders, one key for both parties, the disputer, the liquidator and the ender, 5 to 12 outputs. 2,332 layouts with every payment at its own output: accepted. 224 with one output for two payments (a place from the end on a place from the front) and 814 with a place from the end among the input indices: refused. 1,378 layouts at round 1's places with one output for two payments: refused. 2,640 payments written a sompi short: each fails the input of the one script that asked it, never the other | closed | `t1b_fixed_no_output_pays_two_dues_of_one_key` |
| F1 | `KpfTProve`, `KpfTLiqProve`, `KpfTQuietRefute` (two legs, real signatures) each beside a lapse, a settlement and the end of a late claim of a second account of the same parties, in one transaction: the other object at input 0, the proof's at input 2. 9 pairs accepted at the places of 2j and refused at the places of j; 27 payments a sompi short, each failing the one script that asked it | closed | `t1b_fixed_no_output_pays_two_dues_with_a_proof_beside_another_operation` (builders) |
| F2 | The cure of round 1: at once, a DAA before half the window, with a smaller age stated: refused. A disputed liquidation under every state number and at every age: refused (engine), not built (tool, command line) | the cure is closed | `t1b_fixed_a_cure_waits_half_a_window_and_a_disputed_liquidation_is_not_cured` (builders), `t1b_b_state_numbers_and_flags_are_the_objects`, `t1b_a5_...` |
| F2 | The side liquidates itself with a second key and disputes that with a third, three rounds | **FINDING F2b** | `t1b_finding_a_side_holds_its_account_by_liquidating_and_disputing_itself` (builders), `t1b_finding_the_command_line_builds_a_free_hold_by_self_liquidation_and_self_dispute` (command line) |
| F3 | A liquidation and a declared claim of a tick long past and of one still to come: ended by a third key, half the bond to it, half to the side named / the party the claim was made against, nothing to the maker; round 1's transaction (the bond back to the maker) refused; the maker as ender cannot take both halves | closed | `t1b_fixed_a_claim_of_a_tick_long_past_loses_its_bond`, `t1b_fixed_a_claim_of_a_tick_long_past_loses_its_bond_through_the_builders` (builders), `t1b_fixed_the_command_line_shares_the_bond_of_a_claim_not_in_time` (command line) |
| F4 | `lib_check` on 27 transactions that are not a sound library: one output more under the id (a script anybody spends, a key's output, an object), with the id unchanged and with the id following; the same in the place of a script; under a second id; under the id by another authorizing input; a copy more without the id following; a script short (both ways); an authorizing input that is not there; no output under an id; a script's address under version 1; a later transaction that writes the scripts again under the id; an operation of the family. All refused. Taken: more copies and another order when the id follows (another library, as sound) | closed | `t1b_fixed_lib_check_refuses_what_is_not_a_library` (builders) |
| F4 | `lib-check` and `acct-open --lib FILE` on 12 changed genesis files (an output more, the change under the id, a key in a script's place, a copy more, a script short, two scripts swapped, other ids, other authorizing inputs, another value, nothing under an id), an operation's transaction, a file that is no transaction | closed | `t1b_fixed_the_command_line_opens_no_account_in_a_library_it_cannot_check` (command line) |
| F4 | The engine's side is unchanged and by design: an output more under a library's id takes every object | (design) | `t1b_design_an_object_trusts_every_input_of_its_librarys_id` |

F2 measured again (account of 2,000 + 3,000 KAS, window 50, bond 500 KAS):

| Way | Watcher | Account held | Cost to the side |
|---|---|---|---|
| liquidate itself, cure after half a window (round 1's) | none | 147 of 180 DAA in three rounds, 25 of each 50 open to a dispute | the fees (0.0103 KAS a round) and 1 sompi a round into its own margin |
| the same | disputes within half a window | 60 DAA (10 before the dispute, the window after) | 250 KAS (half the bond: to the watcher; the other half comes back to the side as "the side named") and the fees |
| liquidate itself, dispute itself at once (F2b) | any number | 150 of 180 DAA in three rounds | the fees of three transactions a round (0.0149 KAS) |

## Finding

**F2b (High). A side that liquidates itself and disputes itself has both bonds back.** `KpfTLiqDispute.lapse` pays
the disputer its bond and half of the liquidator's, and the other half to the side the liquidation named. When the
side named is the one that made the claim, with a second key as liquidator and a third as disputer sent in the same
moment, all three payees are that side: 750 + 250 KAS back of the 2 x 500 posted, nothing to the other party. A
claim already disputed is not disputed by a watcher, a false one is proved by nobody, and since round 2 a disputed
one is not cured, so nothing reaches the account for a full window each round; then the same again. Through the
builders, three rounds: held 150 of 180 DAA for 0.0446 KAS of fees. At the command line the same three commands
(`acct-liquidate`, `acct-dispute`, `acct-lapse`) are built and verified, and every other command on the claim is
refused. The fix of F2 made this way better than the old one: the side no longer has to come back before the window
ends. The rule for a declared close does not have the hole (its half goes to the party the claim was made against,
who is never the claimer), and that is the difference: for a liquidation "the side named" is the victim only when
somebody else made the claim. Giving the half to the other party instead would open the mirror (the other party
liquidates the side through a second key and disputes that). A half that neither party can be on the receiving end
of closes both: a quarter to each party, or the half left in the script's own copy (`goesOn` already lets its value
grow: the design's place for the committee's fee), which costs no output. Not tested (no node): that the side wins
the account again after each lapse; it can send the lapse and the next two transactions together.

## Covered from round 1's "not reached"

| What | Result | Test |
|---|---|---|
| The three proofs going on: the script a sompi short, under no id, another id, bound to the object's input, replaced by another script or one anybody spends, at another index; the change under the library's id (54 transactions, one leg and two) | holds | `t1b_r2_a3_the_proofs_go_on_unchanged` (builders) |
| The three proofs given the object of every other kind on the same account (40) | holds | `t1b_r2_c_the_proofs_refuse_every_other_kind_of_object` (builders) |
| The three proofs' widths: `st` of 313, 311, 320 and 0 bytes, `moving`, the disputer's and the liquidator's key likewise, with the object's hash following the bytes shown and on the real object; `who` and the age (168 refused, 12 controls accepted) | holds | `t1b_r2_b_the_proofs_hold_every_byte_string_to_its_width` (builders) |
| A bond of one sompi: every entry runs, every output whole; the half that is nothing is not asked | holds | `t1b_r2_edge_amounts` |
| A bond that would be nothing (9,999 sompi at one basis point; a rate of 0): no declare, liquidate or quiet | holds | `t1b_r2_edge_amounts` |
| Both margins at `MARGIN_MAX` with a claim of the whole pot: every entry runs but `add` and `cure`, which cannot add; an add to `MARGIN_MAX` exactly is taken, a sompi more is not | holds | `t1b_r2_edge_amounts` |
| An object holding less than its state says: a settlement pays its first party what there is (a sompi less refused) and the other nothing; the entries that write the account back refuse an object holding less than the bonds they give back, and take one holding exactly them | holds (see the note) | `t1b_r2_edge_amounts` |
| The command line: `lib-check`, `acct-open`, `acct-liquidate`, `acct-dispute`, `acct-lapse`, `acct-late`, `acct-declare`, `acct-settle`, `acct-cure`, `acct-add-margin` on the three subjects above | as the builders | `crates/kpf-cli/tests/auditT1_cli.rs` |

Notes, not findings:

- No script ties an object's value to its margins: that is the opening's. An account opened holding less than its
  pot settles in favour of whoever claims first, up to what is there. Whether the tool checks an object's value
  against its state when it reads one was not tested.
- What is left of F3: a liquidation of a tick long past that names its maker's own side and is ended by its maker
  gives the whole bond back (both halves are its own). It holds the account only until anybody else ends it, and
  whoever does takes half the bond, so it is not a hold (asserted at the end of
  `t1b_fixed_a_claim_of_a_tick_long_past_loses_its_bond`).
- `OBuilt::sign` panics ("attempt to subtract with overflow", `oracle_build.rs:588`) instead of returning an error
  when the outputs of a built transaction that a caller edited exceed its inputs. Only an edited `OBuilt` reaches it.

## Not reached in round 2

- F1 with a proof's object next to the other object (inputs 0 and 1), and two proofs in one transaction: the proofs
  were paired with the object at input 2 only.
- Edge amounts for the three proofs (a bond of one sompi, margins at `MARGIN_MAX`).
- The command line for `acct-prove`, `acct-refute`, `acct-quiet`, `acct-close-both`, `--lib-unchecked`, and how
  `acct-cure` behaves before half a window has passed (the tool knows no current DAA offline).
- That a genesis file `lib-check` accepts is the transaction on the chain (no node).
- Anything that needs a mempool: who wins the account after a lapse or an end as late.
- One account shape for the measurements; the three-operation case of F1.

# Round 3

21 November 2026, after the fix of F2b (`KpfTLiqDispute.lapse` and `KpfTLiq.late` leave half the liquidator's bond in
the script's own copy; nothing to the side named). The three test files were adapted (the scenes `liq-lapse` and
`liq-late` pay one key and write the script half a bond richer), F2b was tested as closed and every route I could
think of to hold an account was measured, and most of round 2's "not reached" was covered.

Result: **F2b is closed.** No route gives a single party under several keys a whole bond back from a false claim.
One new finding, Low (F5): a side that is truly below its maintenance buys two windows more to cure, for the fees, by
being its own liquidator and disputer. Every round-1 and round-2 invariant holds again on the changed scripts.

Tests (39, all passing):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 18 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 17 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture            # 4 tests
```

## F2b and what stayed as it was

| What was tried | Result | Test |
|---|---|---|
| `liq-lapse` and `liq-late` with one key as the side named, the liquidator, the disputer and the ender: the keys have 1.5 bonds of 2 (lapse) and half of one (late); round 2's transaction (the half to the side named), the half added to the last output, left in the account, in a second copy of the script, in the script's address under no id, the script a sompi short: all refused; the script richer than the half: accepted (14 transactions) | closed | `t1b_fixed_the_half_of_a_liquidators_bond_stays_in_the_script` |
| The same three rounds as round 2 through the builders: self-liquidation, self-dispute, lapse. 750 KAS back of 1,000 posted each round, 250 in the script, nothing to either party; round 2's transaction refused by the engine | closed | `t1b_fixed_a_side_that_liquidates_and_disputes_itself_loses_half_a_bond_a_round` (builders), `t1b_fixed_the_command_line_self_liquidation_and_self_dispute_costs_half_a_bond` (command line) |
| Round 2's note on F3 (a liquidation of a tick long past naming its maker's own side, ended by its maker): half back, half in the script; the whole refused | closed | `t1b_fixed_a_claim_of_a_tick_long_past_loses_its_bond` (and `..._through_the_builders`, the command line's) |
| Unchanged on purpose: of a declared close's lapse and late end and of a no-price claim's early end, half to the party the claim was made against, with the claimer's own key as disputer or ender; that half to the claimer's key, added to the claimer's output, a sompi short: refused (9 transactions) | holds | `t1b_fixed_the_half_of_a_liquidators_bond_stays_in_the_script` |
| F1 again with the script's richer output: 2,464 layouts accepted, 240 + 902 + 886 refused, 2,376 payments a sompi short each failing the one script that asked it | holds | `t1b_fixed_no_output_pays_two_dues_of_one_key` |
| The source check of A3 now takes an entry that asks more of its own output after going on, and nothing else after it | holds | `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` |

## What holding an account costs now

W is the window, b the bond. Measured on the account of 2,000 + 3,000 KAS (W = 50 DAA, b = 500 KAS); the no-price
routes and the last one on the proof world's account (pot 100 and 66 KAS, b = 10 and 6.6 KAS). Fees are for one round.

| Route | Account held a round | Cost to the side a round | Test |
|---|---|---|---|
| Liquidates itself (false), cures after half a window; nobody watches | W/2 to W - 1 (147 of 180 DAA in three rounds) | fees 0.0102 KAS, 1 sompi into its own margin | `t1b_fixed_a_cure_waits_half_a_window_...` |
| The same, a watcher disputes within W/2 | up to W/2 + W (60 DAA measured) | b = 500 KAS (250 to the watcher, 250 in the script) | the same |
| Liquidates itself and disputes itself at once | W (150 of 180 DAA) | b/2 = 250 KAS in the script, fees 0.0142 | `t1b_fixed_a_side_that_liquidates_and_disputes_itself_...` |
| Declares a false close, disputes it with a second key | W | b/2 = 250 KAS to the other party, fees 0.0144 | `t1b_r3_holding_by_a_declared_close_costs_half_a_bond_to_the_other_party` |
| Declares a close of a tick long past, ends it itself | until anybody ends it (a third key that does takes the claimer's half too) | b/2 = 250 KAS to the other party, fees 0.0094 | the same |
| Liquidates itself at a tick long past, ends it itself | until anybody ends it | b/2 = 250 KAS in the script, fees 0.0096 | the same |
| Declares "no price", refutes it with a second key and the committee's tick | until anybody refutes it | b/2 to the other party, fees 0.0198 | `t1b_r3_holding_by_a_no_price_claim_costs_half_a_bond_to_the_other_party` |
| Declares "no price" of a time to come, ends it with a second key | until anybody ends it | b/2 to the other party | the same |
| Declares "no price", nobody shows one for the account's quiet time | the quiet time | nothing: settled at the entries, which is what the claim is for | the same |
| **Truly below its maintenance**: liquidates itself at the rule's terms, disputes itself a DAA before the window ends, proves it a DAA before the proof window ends, cures a DAA before the third window ends | 3W - 3 (147 DAA) where an honest liquidator's claim gives W | fees 0.0322 KAS; both bonds back | `t1b_finding_a_side_truly_below_its_maintenance_takes_three_windows_to_cure_for_the_fees` |

The first row is free because nobody looked; a false claim nobody looks at is paid as declared anyway, and the
watcher's half bond is what pays for looking. Every other false route costs half a bond or a whole one.

Not measured, by reasoning only (it needs a mempool): a party adds a sompi to its own margin again and again
(`KpfTAdd`, signed by the side, 0.004 KAS each), which moves the account's UTXO each time. Whoever wants to claim on
the account must spend the newest one, and the side can send its next add first. Each add is one race the other can
win, and winning once ends it, so I do not count it as a hold; it is cheaper per DAA than half a bond on a large pot
if the side wins the races.

## Finding

**F5 (Low). A side truly below its maintenance takes three windows to cure instead of one, for the fees.** The
proof of a disputed liquidation gives the disputer's bond to the liquidator and the proved claim waits a window
again (so that a liquidator disputing its own claim cannot take the cure's time from the side). When the side is the
liquidator and the disputer, with a second and a third key, that bond goes from its third key to its second. Through
the builders with the committee's real signatures: the rule's liquidation of A by A's second key, disputed by its
third a DAA before the window ends, proved a DAA before the proof window ends, cured a DAA before the third ends:
147 DAA after it landed, 2 x 6.6 KAS posted and back, 0.0322 KAS of fees. Nobody can call the claim false, and while
it stands no honest liquidator's claim and no close by the other party reaches the account. If the side does not
cure, the settlement is the rule's (and the keeper's fee is its own). What it has for nothing is a decision two
windows later than an honest liquidator would leave it; disputing an honest liquidator's true claim for the same two
windows costs it a bond. It has to be first to liquidate itself. A direction in the manner of round 3: of the
disputer's bond at the proof of a liquidation, half to the liquidator and half left in the script, so that the two
windows cost half a bond whoever the liquidator is. That halves what an honest liquidator has from a frivolous
dispute; whether the time is worth that is the owner's to weigh.

## Round 2's two notes

- An object that holds less than its state says: the tool now refuses it on read (`acct-show` and `acct-declare` on an
  account file a sompi under its two margins: "REFUSED: the object holds 99.99999999 KAS and its state says at least
  100"; a sompi more is read; `t1b_r3_the_command_line_quiet_close_both_and_an_object_that_holds_too_little`). I do
  not think a script-level check is needed. Only an opening (or the two parties by `KpfTBoth`) can write an object
  that holds less than its pot, and every later entry keeps what it is given whole (D, and the edge amounts of round
  2: a bond is always added on top, so an object made by the scripts holds at least its bonds, the entries that give
  bonds back take an object holding exactly them, and a settlement pays the claimer or the liquidator first). So a
  disputer's or a liquidator's bond is never stuck in an underfunded object; who is short is the other party of the
  opening, which is where the check belongs: whoever co-signs an opening, and the tool on read.
- `OBuilt::sign` on an edited transaction whose outputs exceed its inputs: not tested again as such; my tests set the
  change to nothing before signing.

## Covered from round 2's "not reached"

| What | Result | Test |
|---|---|---|
| F1 with a proof's object next to the other operation's object: each of the three proofs with a lapse and with a settlement of a second account, objects at inputs (0,1) in both orders (12 transactions) | holds | `t1b_fixed_no_output_pays_two_dues_with_a_proof_next_to_another_object_and_with_two_proofs` (builders) |
| Two proofs in one transaction: a proved close and a refuted no-price claim of a second account that reads the tickets the first spends; objects at (0,1), (1,0) and (0,5). With the 12 above: 15 transactions accepted at the places of 2j and refused at the places of j, 43 payments a sompi short each failing the one script that asked it | holds | the same |
| The command line: `acct-quiet`, its settlement at the entries (the claimer 40 + 10 KAS), its early end by the claimer's second key (5 KAS to the other party), `acct-close-both` (one signature refused; 45 KAS to a) | as the builders | `t1b_r3_the_command_line_quiet_close_both_and_an_object_that_holds_too_little` (command line) |

## Not reached in round 3

- The command line for `acct-prove` and `acct-refute` (they need the seats' signed ticks; the repository's own
  `committee_cli.rs` runs them), `--lib-unchecked`, and `acct-cure` before half a window (offline the tool knows no
  current DAA: it sets the sequence, and the node is what refuses a young claim).
- F1 with three operations in one transaction; edge amounts for the three proofs.
- Anything that needs a mempool: who wins the account after a lapse, a cure or an end as late, and the route by
  repeated `KpfTAdd`.
- That a genesis file `lib-check` accepts is the transaction on the chain.
- How the half bonds that stay in a script's copy are ever collected (not built: the design's committee fee).

# Round 4

21 November 2026: orders between users (`KpfTMatch`, `KpfTCancel`; `thin::{Template, Order, match_orders}`,
`thin_build::order_*`, the `order-*` commands). The three test files were first made to build and pass again (a
library of 17 scripts, 22 entries; `TObj::Order`), then the two scripts were put through the same batteries as the
rest and through O1 to O8.

Result: **one finding above Low.** F6 (High): an order does not say what margin it asks of the other side, so an
order with a real margin is matched against one with a sompi of margin. Two Low findings: F7, an order past its
expiry is still matched; F8, without a least fill (the tool's default) an order is ended or cut up by a fill of one
unit of size. Everything else of O1 to O8 holds: the amounts, the consent of both orders, the two objects under one
id, the places, cancel and expire, the codec.

Tests (57, all passing):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 30 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 22 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture            # 5 tests
```

## The earlier batteries on the two new scripts

`order-cancel` and `order-expire` are two more scenes (20 now) and an order is an eighth kind of object, so every
battery of rounds 1 to 3 ran on them unchanged: A1 520 + 42 transactions, A2 240, A3 260 and the source check (22
entries of 17 scripts, the list held equal to `thin_artifacts::SCRIPTS`), A4 280, A5 188, B 68 + 355 + 27, C 340
(every older entry refuses an order; cancel and expire refuse every other kind), D 144, the edge amounts 200, and F1
with thirteen entries that pay a key instead of eleven (3,904 layouts accepted, 2,520 refused, 3,120 payments a sompi
short each failing the one script that asked it). All hold. `KpfTMatch` reads two objects and has its own harness.

## O1 to O8

| Id | What was tried | Result | Test |
|---|---|---|---|
| O1 | 8 cases (both rest; one filled whole; one or both return what is left; no tips; orders holding more than margin and tip; sizes and amounts that do not divide) at all 24 arrangements of the long order, the short order and the script among four inputs: 192 matches accepted at the model's account, tips and rests; what leaves each order is what it held; 816 outputs a sompi short, each failing the script | holds | `t1b_o1_a_match_pays_what_the_model_says_wherever_its_inputs_stand` |
| O1 | 60 random matches (sizes to 2,000,000,000, margins and tips at random, orders holding more) the model takes: accepted at the model's amounts; what the script takes and the model does not is only what the model's account check refuses | holds | `t1b_o1_random_matches_are_the_models` |
| O1, O6, O8 | The model: 600 orders filled by 2,056 random matches down to nothing. Each share is the floor of its proportion, what rests carries exactly what was not taken and is itself an order that is filled whole, the accounts' margins sum to the order's margin and the tips' shares to its tip, what an order held above them goes back to its owner | holds | `t1b_o1_fills_down_to_nothing_neither_make_nor_lose_a_sompi` (builders) |
| O1 | Through the builders, signed and metered: sells of odd sizes and amounts filled by buys of random sizes, 17 matches; the matcher is paid the model's tips, the owners what their orders do not keep | holds | `t1b_o1_a_sell_is_filled_by_random_buys_through_the_engine` (builders) |
| O2 | 214 matches outside what the orders say: a price a sompi beyond either limit or the market's, nothing, negative; a size beyond either order, nothing, negative; below either least fill (and the exception: all that is left); a size above 2,000,000,000; a share of margin that rounds to nothing; another template (three ways); the sides swapped (four ways); each value shown one off, each key another's; an object of every other kind in either place; every other `who` over the inputs; two orders under one id; the template at 322, 320, 642 and 0 bytes and the three keys at 33, 31 and 0, the orders' hashes following; the script's own copy changed | holds | `t1b_o2_a_match_is_within_what_both_orders_say` |
| O2 | The margin of the other side | **FINDING F6** | `t1b_finding_an_order_is_matched_against_a_side_with_a_sompi_of_margin`, `t1b_finding_the_tool_matches_an_order_against_a_side_with_a_sompi_of_margin` (builders) |
| O2 | Two orders of one owner: the script takes them (an account whose two keys are one; the model refuses). Any 321 bytes both orders name are a template to the script (the tool reads a template back and refuses what is not one) | by design, noted | `t1b_design_what_a_match_leaves_to_the_orders_owners` |
| O3 | What a match writes under the two ids, 56 wrong ways: the account under the short's id, under none, missing, twice, another beside it; what rests written before the account, missing, twice, under the other id, paid out instead; an output more under either id | holds | `t1b_o3_two_objects_under_one_id_are_never_written_wrong_or_spent_together` |
| O3 | Every one of the 20 scenes with its object under the long order's id: accepted alone (an account of a match is an account while the order rests); refused beside a match whose long order has the id, beside the cancel and beside the expiry of an order of the id (77 transactions; among them `KpfTBoth`, which asks nothing of the outputs: the order's script refuses) | holds | the same |
| O4 | One key owning both orders and matching: the three places at all 24 arrangements and 4 to 14 outputs: 116 layouts accepted, 51 refused for a place among the inputs; no number of outputs puts two of the three at one output | holds | `t1b_o4_the_three_places_of_a_match_never_meet` |
| O4 | A match beside each of the thirteen entries that pay a key (lapse, settle, cure, late, cancel, expire and the rest), every key the same, 7 arrangements of six inputs, 6 to 17 outputs: 412 layouts accepted, 204 refused (30 with one output for two payments, a place from the end on a place from the front); 735 outputs a sompi short each failing the one script that asked it | holds | `t1b_o4_a_match_beside_another_operation_shares_no_output` |
| O5 | Cancel signed by another key; expire a DAA before the expiry, with no lock time, with a lock time that is a time stamp, with the script's input final (the lock switched off), with an earlier expiry shown; the value to another key, elsewhere, split; the order written again; the template's hash and the owner's key at 31, 33 and 0 bytes; an expiry of 0 (returned by anyone at once) and one that is a time stamp (never; its owner cancels) (38) | holds | `t1b_o5_only_the_owner_cancels_and_nobody_expires_an_order_early` |
| O5 | A cancel beside a match of the same order: of the long, refused (the match writes the account under its id); of a short filled whole, taken only when somebody pays the order's whole value to its owner on top of the match | holds (nobody loses) | the same |
| O5 | An order past its expiry | **FINDING F7** | `t1b_finding_an_order_past_its_expiry_is_still_matched`, `t1b_finding_the_tool_matches_orders_past_their_expiry` (builders), the last lines of `t1b_o7_orders_at_the_command_line` |
| O6 | What would rest below 0.2 KAS: refused, taken when the order returns what is left; after each fill what rests is filled whole; an order holding a sompi more, filled whole: the sompi to its owner at its place and refused without it; an order holding less than its margin and tip: refused | holds | `t1b_o6_what_rests_is_an_order_that_can_be_filled` |
| O6 | A fill of one unit of size of an order without a least fill | **FINDING F8** | `t1b_finding_an_order_without_a_least_fill_is_ended_or_cut_up_by_a_fill_of_one` |
| O7 | `decode(encode)` with an order as the eighth kind (300 a kind), 18,070 prefixes, 1,152 extensions, 24,480 other kind bytes, 38,400 single-bit changes each read to its own bytes or refused | holds | the four `t1b_e_...` (builders) |
| O7 | A template: `from_bytes(bytes)` on 60 random ones, every prefix and four extensions refused, 18,000 single-bit changes each read to its own bytes (9,815) or refused; the template is the account's state without what a match adds | holds | `t1b_o7_a_template_is_its_bytes_and_nothing_else_is_one` (builders) |
| O7 | The command line: orders placed and shown; a match in part writes an account and two order files, each naming the output that holds its amount under its id, the account before the long's rest; the account takes margin while the order rests; a match whole; more than is left, a price beyond a limit, the orders swapped: refused; cancel without the owner's key refused, with it the whole value to the owner; expire the same; an order file with another order's state, or holding a sompi less than its margin and tip: refused on read | holds | `t1b_o7_orders_at_the_command_line` (command line) |
| O8 | Tips: the shares over any sequence of fills sum to the tip, each the floor of its proportion (the model test above); no tip without an account (a share of margin that is nothing is refused); an owner matching its own two orders has its tips back and an account with itself | holds | `t1b_o1_fills_...`, `t1b_o2_...`, `t1b_design_...` |

## Findings

**F6 (High). An order is matched against a side with no margin to speak of.** An order states the margin its owner
posts. It states nothing of the margin it asks of the other side, and `KpfTMatch` asks only that each share of
margin be above zero; the model and the tool ask no more. In the engine: B sells 4 units at 10 with 60 KAS of
margin; A buys the 4 units with one sompi of margin (and the 0.2 KAS an order must hold as a tip, which A takes back
as the matcher, with B's tip); the account is 1 sompi against 60 KAS. B can never be owed more than the pot, its own
60 KAS and a sompi; A is owed 3.12 KAS at a price 10 % up, for the sompi. Through the builders the same, signed and
metered. B's way out is to liquidate A at once: A is below its maintenance at the entry, and stops being so once the
price has gone its way (at 11 the rule liquidates nobody). The keeper's fee of that liquidation is at most what the
liquidated side is still owed, 1 sompi here, and it takes a bond of a tenth of the pot (6 KAS), so nobody but B has
a reason to do it; and during the liquidation's window A still chooses, by the cure, to keep the position if the
price went its way. An owner who is not watching the moment its order is matched holds a position that can only
lose. The matcher needs nothing but its own order: it is a free option on every order in the book. Directions: the
script is shown the template, which carries the maintenance rate, and computes shares already; it can ask each share
of margin to be at least the maintenance of the size matched at the price (or a multiple of it), or an order can
state the least margin a unit it takes from the other side, one more integer in its hash.

**F7 (Low). An order past its expiry is still matched.** The expiry is the DAA from which anyone returns the order
to its owner (`expire`, lock time at or after it: holds, O5). It is not a DAA after which the order is no longer
matched: a match reads no time, and a script can show that a DAA has come, not that it has not passed. Two orders
that ended at DAA 1 are matched at the long's limit in a transaction locked to DAA 9,000,000; the tool builds it
without a word (builders, and `order-match` on orders that ended at 100 and landed at 33,900). Nobody but the owner
has a reason to run `expire`: the tip goes to a match and nothing to an expiry. So a limit left in the book after
its time is picked off at its limit unless its owner takes it back itself. Directions: a part of the tip to whoever
runs `expire`, which makes the matchers the ones that return old orders; and the tool refusing or warning on a match
of an order past its expiry.

**F8 (Low). Without a least fill an order is ended or cut up by a fill of one.** `least` is the owner's guard and
the tool's default for it is 0. Then a match of one unit of size (a millionth of a market unit here) against the
matcher's own order of that size is accepted: an order that does not rest is ended by it (all but 405 sompi of its
40.5 KAS back to its owner, who places it again), and one that rests is cut into accounts of 401 sompi, each its
owner's to close. It costs the matcher a transaction's fee each time. Directions: the tool asking for `--least` (or taking the
whole size for `--no-rest`), not 0.

Notes, not findings: the matcher chooses the price anywhere between the two limits, so the whole of a crossing goes
where the matcher puts it (each owner has its limit, as it said). The script takes two orders of one owner and any
321 bytes that both orders name as their template; both harm only the owners that made them.

## Not reached in round 4

- Two matches in one transaction, and a match beside a proof (F1's question for those pairs): a match was paired
  with the thirteen single-object entries only.
- O6's order holding a sompi more through the builders (that the tool tops the 1-sompi output up, and who pays): the
  engine test writes the sompi as it is.
- O5 with the order's own input final and the script's not; `order-place --no-rest` and `--least` at the command
  line; `lib-check` on a 17-script genesis beyond the round-2 cases, which ran again unchanged.
- Anything that needs a mempool: who matches first, and how fast an owner can liquidate after F6's match.
- What F6 is worth over a window (it depends on the market's moves): only the two ends are measured, the sompi at
  risk and what a 10 % move pays.

# Round 5

21 November 2026, after the fixes of F6, F7 and F8 (orders gained an `ask`; the tool defaults the least fill and
refuses a match past a known expiry) and one new script, `KpfTMerge`. The three test files were adapted to the order's
new shape (an `ask` integer, the library now 18 scripts / 23 entries), F6/F7/F8 were tested as closed, and the merge
was put through M1 to M5.

Result: F6 is closed at the script, the model and the tool; F7 and F8 are closed at the tool (unchanged at the script,
by design, since a script cannot prove a DAA has not passed and cannot know an owner's intent). The merge shows no
critical: it is fair to a few sompi and writes only the one account. One finding, Low (F9): a merge is unsigned and
takes MERGE_TIP (half from each party), so a party can force merges on the account it shares with another, each
costing the other MERGE_TIP/2 and replacing its UTXO. Open from before: F5, and F7/F8's script-level behaviour, as
recorded.

Tests (67, all passing):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 36 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 25 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture            # 6 tests
```

## F6, F7, F8 on the changed orders

| Id | What was tried | Result | Test |
|---|---|---|---|
| F6 | A buy of a sompi of margin against a sell that asks 60 KAS: refused by the script and the model; a buy that posts the 60 KAS asked, accepted; a sompi short, refused; an order that asks nothing, matched against a sompi (the owner's choice) | closed | `t1b_fixed_a_match_posts_the_margin_each_order_asks`; `t1b_fixed_the_tool_refuses_a_match_below_the_ask` (builders); `t1b_o7_orders_at_the_command_line` (command line) |
| F6 | The ask across a sequence of partial fills down to nothing (400 orders, the short posting exactly the ask share each fill): the total posted is never below the total asked, so rounding does not let a side under the ask | closed | `t1b_fixed_the_ask_holds_across_partial_fills` |
| F7 | The script still matches two orders that ended at DAA 1, even in a transaction locked far past the expiry (a script cannot prove a DAA has not passed) | by design | `t1b_design_the_script_matches_an_order_past_its_expiry`; `t1b_design_the_builder_matches_orders_past_their_expiry` (builders) |
| F7 | `order-match` with `--daa` after the expiry: refused, unless `--past-expiry`; before the expiry, allowed; offline (no `--daa`), it matches | closed at the tool | `t1b_o7_orders_at_the_command_line` (command line) |
| F8 | The script still matches a one-unit fill of an order without a least fill (both sides posting what the asks require) | by design | `t1b_design_the_script_matches_a_fill_of_one_without_a_least_fill` |
| F8 | `order-place` defaults `--least` to a tenth of the size for a resting order and to the whole size for `--no-rest`, so a one-unit fill of a resting order and a part fill of a `--no-rest` order are refused | closed at the tool | `t1b_o7_orders_at_the_command_line` (command line) |

The order's earlier batteries (A1 to A5, B, C, D, E, O1 to O8 of round 4) ran again on the 18-script library with the
`ask` field, and hold: the match-amount tests build orders that ask nothing (round-4 semantics), the F6 tests set the
ask. A3 is 23 entries of 18 scripts, the list held equal to `thin_artifacts::SCRIPTS`.

## The merge (M1 to M5)

| Id | What was tried | Result | Test |
|---|---|---|---|
| M1 | A merge at all 24 arrangements of its three inputs, two ids and one: the merged pot is the two pots less MERGE_TIP; a sompi short at the account refused; a sompi more accepted | holds | `t1b_m1_a_merge_pays_the_two_pots_less_the_tip` |
| M1 | The sender keeps only MERGE_TIP: the account written 1, MERGE_TIP or MERGE_TIP+1 short is refused; the account written twice under its id refused | holds | `t1b_m1_the_sender_keeps_only_the_tip` |
| M2 | 4,000 random pairs (sizes, entries, margins, scales that do not divide; entries at 1 and at the price limit; sizes near the cap), six prices each: what each party is owed before the clamp differs from the sum of the two accounts by exactly MERGE_TIP/2, drift 0 sompi; a chain of 200 merges does not accumulate drift beyond the tips (0 sompi) | holds | `t1b_m2_a_merge_is_fair_to_a_few_sompi` (builders) |
| M3 | A claim, a disputed claim, a liquidation (claimed, disputed, proved), a no-price claim or an order in either account's place: refused; a second account of another window: refused (15 at the engine); the model refuses each too | holds | `t1b_m3_only_plain_accounts_are_merged`, `t1b_m4_the_model_refuses_what_is_not_a_fair_merge` (builders) |
| M3 | A party opening an account of the two alone and merging it in | **FINDING F9** | `t1b_finding_a_party_can_force_merges_that_cost_the_other_half_the_tip` (builders), `t1b_m_the_merge_at_the_command_line` (command line) |
| M4 | The model refuses another counterparty, the sides turned, another window, a second leg, a size of nothing, a size over the cap together, a blended entry over the price limit, margins that would leave a side at or below zero; entries at 0 and at the limit are allowed | holds | `t1b_m4_the_model_refuses_what_is_not_a_fair_merge` (builders) |
| M5 | The account under the second's id, under none, missing, written twice; an output under the second's id (it must end); a second output under the first's id; the script a sompi short, under no id, replaced; `who` at the script itself, a funding input, out of range | holds | `t1b_m5_the_merge_writes_only_the_account_under_id1_and_ends_id2` |
| M5 | A merge beside a lapse, a settlement, a cancel and a late end in one transaction: accepted, each payment a sompi short failing the one script that asked it (the merge's only output is the account, a from-the-front place, so it collides with no from-the-end payment) | holds | `t1b_m5_a_merge_beside_another_operation` |

## Finding

**F9 (Low). A merge is unsigned and takes MERGE_TIP from both parties, so a party can force merges on the shared
account.** `KpfTMerge` takes no signature and moves MERGE_TIP/2 out of each party's margin into the sender's change.
It needs two accounts of the same two parties in the same places; a party can make one alone, by opening an account
that names itself and the other and funding both margins (`acct-open` without `--b-funding`), of the same template as
the account they share. Merging it in is fair to a few sompi (M2), so it moves neither the entry nor the margins in
the merger's favour beyond rounding, and the injected B-margin is the merger's own money paid into B's side. But each
merge takes MERGE_TIP/2 from the other party (0.005 KAS here) and writes the shared account to a new UTXO, spending
out from under anything the other party was about to send on the old one. Through the builders and the command line: B
pays MERGE_TIP/2 and B signs nothing. It costs the griefer about the same (its own MERGE_TIP/2, the fee, and the
injected pot, of which a2 returns to it as the merged A-margin and b2 goes to B), and B gains the injected b2, so it
is bounded churn rather than theft; a party that sets b2 just above zero makes the other pay about MERGE_TIP/2 a
merge. Directions: the merge could ask the signature of the party whose margin falls (both, in effect), or leave the
tip in the script's own copy as the other halves now do (F2b, F3), so that forcing a merge is not free of a payout to
the sender; or the tool could warn when it merges an account a party funded both sides of.

Not findings: the merge enlarges the pot, so each party's worth is clamped later and the bond is a tenth of a larger
pot (by design: one object, one close, one liquidation); an account that could be liquidated alone may not be after a
merge with a healthy one, but only a plain account merges (a declared liquidation cannot, M3), so a merge cannot
rescue an account from a liquidation already on the chain; and the merge's entry arithmetic did not overflow or
misprice over the random pairs of M2 and M4.

## Not reached in round 5

- F9 over many rounds through the engine with the real race (needs a mempool): the per-merge cost is measured, the
  contention is reasoning.
- A model/script gap on the input entry: `merged` checks only the blended entry against the price limit, while the
  merge script also checks the input `e2 <= pm`. So `thin_build::merge` could in principle build, from an account
  whose entry is above the limit but whose blend is under, a transaction the engine still accepts (the engine's own
  `e2 <= pm` would reject only an input over the limit). Not exercised here (the builders go through `merged`, which
  rejects a blend over the limit); worth a model check that each input entry is at most the price limit.
- Two merges in one transaction; a merge beside a proof.
- Edge amounts for the merge (a pot one sompi above what the tip and a zero margin allow) at the engine; only the
  model's margin-above-zero guard was checked.

## After round 5 (by the author of the fixes, not reviewed)

F9 is fixed in `KpfTMerge` and in `thin::merged`: each party's margin in each of the two accounts is at least
`MERGE_MIN` (0.2 KAS), so an account one party makes alone to force a merge gives the other forty times the
`MERGE_TIP / 2` the merge takes from it. The model also holds both accounts to `Account::check` (the gap noted above:
an input entry above the price limit). The two tests of F9 were adapted by the author of the fix
(`t1b_fixed_a_forced_merge_gives_the_other_party_more_than_it_takes`, and the last lines of
`t1b_m_the_merge_at_the_command_line`); the churn itself (a merge rewrites the account's UTXO without either party's
signature) remains, at that price.


# Round 6

21 November 2026, after the F9 fix (a merged account's margins are at least MERGE_MIN), the account's moving part
growing to 138 bytes (two asks, `a_ask` and `b_ask`), and one new script, `KpfTNovate`. The three test files were
adapted (the 138-byte moving part, the match writing each order's ask share, the merge's two extra arguments, the
library now 19 scripts / 24 entries, `KpfTNovate` compiled with NOVATE_TIP and the version), and the novation was put
through N1 to N5.

Result: **one Critical finding (F10): a counterparty steals a user's whole margin by a fabricated novation**, because an
opening names the counterparty with no signature of it and `KpfTNovate` pays the user taken out its realized total with
no signature of that user and no floor on the transfer. F9 I consider closed at its price (the merge-churn remains,
recorded after round 5). The novation's other properties hold: it is fair to those who stay to a few sompi, R1/R2 bound
what a collusion of U and L can take from an honest S (and the mirror for L), it pays the model's amounts, and it
writes only the right objects under the right ids.

Tests (76, all passing):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 36 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 33 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture            # 8 tests
```

## Adaptation and F9

The 138-byte moving part, the asks, the merge's two extra arguments and the 19-script library were threaded through
every harness; all of rounds 1 to 5 pass again (engine 36, builders 33, CLI 8). F9: an account one party makes alone to
force a merge must now hold at least MERGE_MIN (0.2 KAS) a side, so the forced merge gives the other party forty times
the MERGE_TIP/2 it takes (`t1b_fixed_a_forced_merge_gives_the_other_party_more_than_it_takes`, adapted by the fix's
author; I checked it). I consider F9 closed: a forced merge is no longer a way to drain the other party, only to churn
its UTXO, which the "After round 5" note records.

## The novation (N1 to N5)

| Id | What was tried | Result | Test |
|---|---|---|---|
| N1 | U and L collude at an entry far from the market (tr large, S's loss): R1 forces L to post S's ask share plus tr, a sompi short refused; S keeps its worth at every price to rounding | holds | `t1b_n1_the_rules_bound_what_a_novation_moves_from_the_two_who_stay` |
| N2 | A novation pays the model's amounts, only NOVATE_TIP leaves the two accounts, every fixed output tight (X rests, Y rests, S == L no Z) | holds | `t1b_n2_a_novation_pays_the_model_and_nothing_more` |
| N2 | Y whose B is not U, another template, X and Y the same account, a second leg on Y: refused | holds | `t1b_n2_only_a_matched_pair_of_plain_accounts_is_novated` |
| N3 | A counterparty fabricates the mirror account and novates the user out | **FINDING F10 (Critical)** | `t1b_finding_a_counterparty_steals_a_users_margin_by_a_fabricated_novation`, `t1b_finding_novation_theft_stands_with_two_attacker_keys`, `t1b_finding_the_command_line_novates_a_user_out_of_its_margin` (command line) |
| N4 | L (the one coming in) is carried whole into Z: at every price it is owed what Y owed it, and Z holds at least L's ask behind the short | holds (L cannot be drained, its position is carried not realized) | `t1b_n4_the_one_coming_in_keeps_its_position` |
| N5 | Z under the wrong id, under none, a sompi short, its script replaced; U paid short or to another key: all refused; Z under the right id and U paid, accepted | holds | `t1b_n5_a_novation_writes_only_the_right_objects_under_the_right_ids` |
| N5 | Sizes at the cap (2,000,000,000), entries at 0 and at the price limit, the transfer at its extremes, u == 0: no overflow or panic, shares exact | holds | `t1b_n5_arithmetic_at_the_bounds` |

The novation is exercised at the engine through the builder path: `signed()` runs `kpf_protocol::verify::verify`, so
every accepted novation above (including the theft) is one the script engine takes, and every refusal is the engine's.

## Finding

**F10 (Critical). A counterparty steals a user's whole margin by a fabricated novation.** `KpfTNovate` takes U out of
two accounts and pays U `axs + bys + tr`, where `tr = quote(n, ey) - quote(n, ex)` is set by the entry `ey` of the
second account Y. It requires no signature of U and puts no floor on `tr` beyond `u >= 0`. And an opening needs no
signature of B: `thin_build::open` (and `acct-open`) take B's key, fund both margins from the opener, and write the
account as a genesis output, so anyone can create an account naming any victim as a party, at any entry, with any asks.
So U's counterparty S, in a real account X where U is long against S, opens a mirror Y in which U is the short at an
entry near zero (S = L, or a second key as L), then novates: `tr` is hugely negative, U is paid `axs + bys + tr`
(little or nothing of its margin axs), and the rest, `zb = bxs - tr`, goes to S's new position. Measured through the
builders and the command line: U is paid about 50 KAS of a 100 KAS margin (one leg) and 50 KAS of 200 (CLI, 150 KAS
lost), the difference taken by the attacker; U signs neither the opening nor the novation. R1 and R2 do not help: they
guard the two that stay (S and L), not the user taken out, and are skipped entirely when S == L. The user is forced to
realize a loss at an entry it never agreed to.

Smallest fix: `KpfTNovate` must require the signature of U (the party taken out, `uk`), so that no one is novated out
of its own account without consenting to the two entries. The deeper root — an opening names a counterparty with no
signature of it — also underlies the round-5 merge-churn (F9's residue) and should be closed too: an opening (and
`acct-open`) should require B's signature, or publish the account only once B has signed, so that a party is never
named in an account it did not agree to. Of the two, U's signature on the novation is the one line that stops the
theft; B's signature on openings is the general cure.

Not findings: when the two who stay are honest and distinct, the novation is fair (N1); L is carried, not realized
(N4); the arithmetic is sound at the bounds (N5).

## Not reached in round 6

- F1-style collisions for the novation at the raw engine: two novations in one transaction, a novation beside a merge
  or a settle, the shared from-the-end payment places (U at `len-1-2*j1`, S at `len-1-2*j2` in the same-user case). The
  novation's payouts are checked tight through the builder, but not woven with another operation as the matches and
  merges were.
- A full before/after clamped-worth grid for N1 across every colluding pair at many true prices: S's and L's worth are
  checked preserved to rounding, but the third party's summed clamped worth over a price grid (the exact N1 statement)
  was done only at the four prices of `t1b_n1`.
- The novation of partial units with rounding pushed to its worst (N1's "partial units with rounding"): sizes that do
  not divide were used in N2, not swept for the largest share drift.
- Whether the tool (`acct-novate`) warns when it novates U out at an entry far from U's other account's: it does not,
  and F10 shows it builds the theft without a word.

# Round 7

21 November 2026, after the fixes by signature: `KpfTMerge` asks both parties' signatures (MERGE_MIN is gone),
`KpfTNovate` asks the signature of the user taken out. The three test files were adapted (the merge's two signatures
and the novation's one in every harness), F10 and F9 were turned into tests of the refusal, F11 was recorded with a
test of its own against the model, and the claim that an object written in a party's name costs that party nothing
was attacked across the family.

Result. F10 is closed. F11 (Critical, the merge as I reviewed it in round 5) is recorded and closed by the same fix;
F9 is closed outright. **One finding above Low remains open, F12 (Medium), in the novation**: an account that asks
nothing of its counterparty's margin (what `acct-open` writes unless told otherwise) has its counterparty replaced, by
a novation only the leaver signs, with one that has no margin. In orders and in the merge nothing above Low is open.
Still open from before, as recorded: F5 (Low) and the script-level side of F7 and F8.

Tests (81, all passing):

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture       # 37 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture  # 36 tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture            # 8 tests
```

## A correction to round 5

Round 5's M2 and M3 said a merge is fair to a few sompi and that an account one party writes alone "cannot move the
entry or the margins in the merger's favour". That was measured on the worth before the clamp only, and I rated the
unsigned merge Low (F9, churn). It was wrong for the worth a party is actually paid: an account's loss stops at its
own pot, and a merge puts a false account's loss on the other party's real margin. The author of the fixes found it;
it is F11 below.

## The findings and the fixes

| Id | What was tried | Result | Test |
|---|---|---|---|
| F11 | The model (`thin::merged`, which computes what it did in round 5): a real account of 5 units at the market's price with 200 KAS each, and a false one of 10 units from a quarter of the price with a KAS each, written by A alone. At the market's price B is owed 200 KAS apart and 125.995 merged (74 KAS taken); A 202 apart and 275.995 merged | **F11, Critical (round 5's merge)**; fixed | `t1b_fixed_f11_a_false_account_is_merged_only_with_the_other_partys_signature` (builders) |
| F11, F9 | The same merge built by the tool: not signed without B's key; with A's or a third key's signature in B's place, or a third key's in A's, the script refuses; signed by both, taken. At the raw engine, 14 merges signed by any other pair (one alone twice, the two swapped, a third key in either place, the funding's key), two ids and one: refused. At the command line: `acct-merge` with one party's key refused, with both built; an account written alone at a false entry is not merged in | closed | the same; `t1b_fixed_a_merge_is_signed_by_both_parties_in_their_places`; `t1b_m_the_merge_at_the_command_line` (command line) |
| F10 | The mirror account is still opened in U's name without U (an opening is signed by its funder only); the novation that would pay U under 60 of its 100 KAS is not signed without U's key, and with another key's signature in U's place the script refuses; the two that stay as one key and as two. At the command line `acct-novate` without the user's key: refused | closed | `t1b_fixed_f10_a_user_is_taken_out_only_with_its_signature` (builders); `t1b_fixed_the_command_line_novates_no_user_out_without_its_key` (command line) |
| signatures | A merge's and a novation's signatures are the transaction's: an output changed after signing (a sompi from the change to the object, which the script's own rule would take) fails the script's input; the script's signature script of one transaction put into another transaction of the same two objects fails it | holds | `t1b_r7_the_signatures_commit_to_the_transaction` (builders) |
| N1 widened | 1,500 random novations the rules take, all of partial units, entries apart, asks of nothing, of a part and of all the counterparty's margin; 12 prices each; worth clamped to the pot. S is owed at least the smaller of what it was owed and its margin plus its ask; L likewise (exact). For whoever asked at least what its counterparty had posted: never worse (0 sompi). Through the cap, for those that asked less: up to 67 KAS (S) and 161 KAS (L) in the sample | holds as stated; see F12 | `t1b_n1_those_who_stay_keep_what_they_asked_over_a_grid` (builders) |
| asks of nothing | U takes its 100 KAS out of an account opened without asks by a novation against its own second key | **FINDING F12** | `t1b_finding_an_account_that_asks_nothing_has_its_counterparty_replaced_by_one_without_margin` (builders) |
| written in a party's name | An order in V's name placed and matched without V (an account of V and the attacker, far from the market, holding the attacker's money): merged with V's real account only with V's signature; V novated out only with V's signature; the account of the match is no mirror of V's real one | holds | `t1b_r7_an_object_written_in_a_partys_name_reaches_nothing_of_that_party` (builders) |

## An object written in a party's name: every operation

An opening and an order are still written by whoever funds them, in anybody's name. What that can cost the party
named, operation by operation:

| Operation | Objects | What a party named without its consent is exposed to |
|---|---|---|
| `KpfTMerge` | two accounts of the same two | nothing: both parties sign (F9, F11) |
| `KpfTNovate`, the user taken out | two accounts | nothing: it signs (F10) |
| `KpfTNovate`, the two that stay | two accounts | they do not sign. Their worth before the cap is kept; the cap is held to their margin plus what they asked (R1, R2; N1 over a grid). An account written in their name holds none of their money, so only their real accounts matter, and there the ask is theirs: with an ask of nothing, F12 |
| `KpfTMatch` | two orders | an order in V's name holds another's money; the account names V and holds none of V's. V's real order is matched only inside its own limit, size, least fill and ask, whoever the other order names |
| declare, liquidate, dispute, quiet and their ends | one object | the bonds are posted by whoever sends the transaction; a claim in a side's name is signed by that side; a liquidation of a side written in V's name takes the margin somebody else put there. V's other objects are not inputs (A1: an object is moved only by a script that names it) |
| cure, add margin, close by both, cancel | one object | signed by the party |
| expire, settle, late, lapse | one object | pay the party named, or the rule's shares; nothing of V's is an input |

So the claim holds for objects written in a party's name: what was open in rounds 5 and 6 was the two operations that
put such an object together with a real one, and both now ask the signature. What remains is not about a false
object: it is what a real account lets its own counterparty do when it asked nothing.

## Finding

**F12 (Medium). An account that asks nothing has its counterparty replaced by one without margin.** In a novation
the two that stay do not sign; what protects each is the ask its account carries: R1 holds the margin of the one that
comes in to at least S's ask (plus the transfer when it is S's loss), R2 holds S's margin to L's. An account written
by a match carries its orders' asks, and `order-place` defaults the ask to the owner's own margin. An account opened
directly carries asks of nothing unless `acct-open --a-ask/--b-ask` are given. In such an account, U (long against V,
100 KAS each) writes a second account in which its own second key is long against it at the same entry with a
thousand sompi each, and novates itself out, its own signature being the only one asked: U is paid its 100 KAS back;
V now faces the second key with a thousand sompi of margin. V's loss if the price rises is what it was; its gain if
the price falls is capped at the thousand sompi (20 KAS less at a price 40 % down, in the test). It is round 4's F6
by another door: a position kept without the margin behind it. The rule liquidates the empty side at once, and the
keeper's fee for it is at most its margin, a thousand sompi, so only V has a reason to. With an ask of U's margin the
same novation is refused (tested). A party's money is in an account only by its own funding input or its own order,
so V did accept the asks; but the tool chose them for it, and chose nothing. Directions: `acct-open` defaults each ask
to the other side's margin, as `order-place` does, and says so; or a script rule that the pot S faces for the units taken is
no smaller than it was (`ays + zb >= axs + bxs`, that is `ays >= axs + tr`), which makes a novation neutral to S
whatever it asked, at the price of refusing some novations an ask of nothing allows today.

## Not reached in round 7

- The novation woven with another operation at the raw engine (two novations in one transaction, a novation beside
  a merge or a settlement): its payment places are `len - 1 - 2j` like the others' and tight through the builder, but
  the pair layouts of rounds 2 and 4 were not run for it.
- The merge and the novation with a party's two keys equal (A == B in one account), and the novation's one-user case
  (S == L) over the grid: the grid has S and L distinct.
- F12 at the command line (`acct-open` without asks, then `acct-novate`): shown through the builders only.
- Anything that needs a mempool; the worth of F12 over time (as for F6, only the two ends are measured).

# Round 8

The fix of F10 and F11 changed shape: every order and account of a library is under one covenant id, its book, which
an object enters only through an anchor (`KpfTPlace`) with its owner's signature (an account: both parties'); the
merge, the novation and the match ask nobody again. The three test files were adapted to that world and extended.

**Is anything above Low open?** Yes, two, both Medium, neither a way to write under a book's id:

- **F13 (Medium, a consequence of the design).** What a resting order can lose is no longer its own margin: once its
  account is merged or novated by anyone, the loss beyond the order's margin is taken from the owner's margin in its
  other accounts of that market and book.
- **F14 (Medium, the tool).** `--book` is optional on `acct-open` and `order-place`; without it both build with a
  warning. In a book whose genesis wrote an account, that account is merged into a real one (F11 again).

B1 found no loose entry: nothing Critical, nothing High. F5 (Low) is open on purpose as before; F9 (Low) is back as
the design (anyone merges, each party pays half of 0.01 KAS).

| id | What was tried | Result | Test |
|---|---|---|---|
| B1 | Source: every one of the 26 entries of the 20 scripts counts the inputs and the outputs of the object's id | holds (table below) | `t1b_b1_every_entry_counts_the_inputs_and_outputs_of_the_objects_id` |
| B1 | Engine, 22 scenes (every entry that reads one object, `KpfTPlace.place` and `.open` among them): an account nobody signed as one output more under the id, authorized by the object's input, the script's or the funding's, at two places; the same in the place of what the entry writes (199 transactions; the same output under no id is accepted) | holds | `t1b_b1_no_entry_writes_an_output_more_under_the_objects_id` |
| B1 | `KpfTMatch` in its four shapes (both rest, one, none): no account, the account under no or another id, written twice, an account nobody signed beside it or in its place by each of 4 inputs, the rests missing, twice, swapped, before the account, an order that does not rest written (73) | holds | `t1b_o3_a_match_writes_under_the_book_exactly_what_its_rule_says` |
| B1 | `KpfTMerge`: the same battery; the second account of another id | holds | `t1b_m5_the_merge_writes_only_the_account_under_id1_and_ends_id2`, `t1b_m1_a_merge_pays_the_two_pots_less_the_tip` |
| B1 | `KpfTNovate` in five shapes, `KpfTMerge`, and the three proofs with one and two markets (12 built transactions): one output more under the object's id by every input of the transaction, ticket inputs included (51); what the entry writes replaced or a sompi short (22) | holds | `t1b_b1_the_novation_the_merge_and_the_proofs_write_no_output_more_under_the_id` |
| B1 | `KpfTNovate`: under the id, in order, what is left of X, the new account, what is left of Y, each holding the model's amount; 40 changes judged by the novation's own input | holds | `t1b_n5_a_novation_writes_under_the_book_exactly_what_its_rule_says` |
| B1 | `KpfTPlace`: the anchor not written again, a sompi poorer, another state in its place, before what enters, written twice; what enters twice, under no id, short; an order's nine values out of range; a margin of nothing; a template hash of 31, 33, 0 bytes; another key's signature (place), the two keys' in the wrong places (open); two anchors spent in one transaction; an anchor beside an operation on an object of the book, both orders (45 + 3) | holds | `t1b_b1_the_way_into_the_book` |
| B1 | Two operations on objects of one book in one transaction: a match beside each of the 22 scenes, cancel / expire / add beside each (107), a merge beside each (41); each scene accepted alone | holds | `t1b_o3_...`, `t1b_b1_a_merge_beside_an_operation_on_an_object_of_the_same_book` |
| B2 | Match, merge, novate with the second object under another id: the engine (the script's own input fails) and the tool | holds | `t1b_o2_a_match_is_within_what_both_orders_say`, `t1b_m1_...`, `t1b_fixed_f11_...`, `t1b_fixed_f10_...`, `t1b_r8_nothing_enters_the_book_in_a_partys_name_without_its_key` |
| B2 | For each scene: the script under the book's id, under a third id; an object of another library's slot under the book's id (88 with B3) | holds | `t1b_b2_b3_objects_of_the_book_and_scripts_of_the_library_are_not_confused` |
| B2 | `book_check`: a genesis writing an account or an order among the anchors, an account after them, an anchor of another library among them, another library's book, a key's output, anchors under an id not theirs, two ids, a placement, nothing under an id (6 of them true genesis transactions to the covenant rules) | holds | `t1b_b2_book_check_takes_only_a_genesis_of_anchors_of_one_library` |
| B2 | A book whose genesis wrote an account: V and K open a real account through its anchor, anyone merges the genesis's account into it; the engine takes it (74 KAS of 200 moved at the market's price) | accepted: what `book_check` is for; see F14 | `t1b_b2_a_book_whose_genesis_wrote_an_account_is_the_false_merge_again` |
| B3 | An object of the book named as the script by each scene's object | holds | `t1b_b2_b3_...` |
| B3 | An anchor under every entry but `KpfTPlace`'s (the kinds' table, 416 transactions over 9 kinds; the merge's two places) | holds | `t1b_c_every_entry_refuses_every_other_kind_of_object`, `t1b_m3_only_plain_accounts_are_merged` |
| B3 | An anchor (its own script and UTXO) in the place of each object of the 12 built transactions: the anchor's `go` passes, the entry refuses (18) | holds | `t1b_b1_the_novation_the_merge_and_the_proofs_...` |
| B3 | The anchor's state is one byte, its hash the hash of a pushed 7 (nine bytes); none of 2,400 random objects has it; no object's bytes read as an anchor | holds | `t1b_b3_an_anchor_is_no_other_object` |
| B4 | F11 in the book: the false account's opening without the other party's key, with another key's signature in either place; under another id, merged | holds (refused) | `t1b_fixed_f11_a_false_account_does_not_enter_the_book` |
| B4 | F10 in the book: the mirror's opening without the user's key; under another id, novated (the two that stay one key, two keys) | holds (refused) | `t1b_fixed_f10_a_mirror_account_does_not_enter_the_book` |
| B4 | An order in a party's name without its key; under another id, matched | holds (refused) | `t1b_r8_nothing_enters_the_book_in_a_partys_name_without_its_key` |
| B4 | The signatures of `open` and `place`: an output changed after signing (to what the rule would take), another object entering, the signature script in another transaction | holds | `t1b_r8_the_signatures_of_the_way_in_commit_to_the_transaction` |
| B4 | What one resting order can lose once anyone merges or novates its account | **FINDING F13** | `t1b_finding_a_resting_order_loses_more_than_its_margin_once_its_account_is_merged_or_novated` |
| B5 | What the caps are worth to a user taken out: 1,500 novations, 81 prices | quantified, bound holds | `t1b_b5_what_the_caps_are_worth_to_a_user_taken_out` |
| B5 | S and L, who never sign: owed at least the smaller of what they were owed and margin plus ask (1,500 novations, 12 prices) | holds | `t1b_n1_those_who_stay_keep_what_they_asked_over_a_grid` |
| B6 | F1, F2, F2b, F3, F4, F6: the fixed tests on the scripts as they are | hold | the `t1b_fixed_...` tests of rounds 2 to 5 |
| B6 | F12: `acct-open` asks what the other posts; the novation that leaves 1 KAS where 100 stood is refused; with `--b-ask 0` it builds | fixed in the tool; by design at the script | `t1b_fixed_the_command_line_opens_an_account_that_asks_what_the_other_posts`, `t1b_design_an_account_that_asks_nothing_...` |
| B6 | F5, F7, F8 | as before (Low, open on purpose; by design) | `t1b_finding_a_side_truly_below_...`, `t1b_design_...` |
| B6 | A, B, C, D, O, M, N batteries with the shared id | hold | all of the three files |
| B6 | The command line: `book-create`, `--anchor/--script/--book`, two books, an anchor of another book, a merge and a novation sent by a third key | holds; `--book` optional: **FINDING F14** | `auditT1_cli.rs` |

## B1: every entry

Inputs and outputs of the object's id that each entry asks, read from the source by the first test above; what each
output is held to, read by me and tested by the rows above.

| Script.entry | In | Out | What is written under the id |
|---|---|---|---|
| `KpfTAdd.add`, `KpfTLate.late`, `KpfTDispute.dispute`, `.lapse`, `KpfTLiq.liquidate`, `.late`, `KpfTCure.cure`, `KpfTLiqDispute.dispute`, `.lapse`, `KpfTQuiet.quiet`, `.early`, `KpfTQuietRefute.refute` | 1 | 1 | `becomes`: the slot with the entry's state hash, holding at least the entry's amount |
| `KpfTDeclare.declare` | 1 | 1 | the claim's slot, at least the account's value plus the bond |
| `KpfTLiqProve.prove` | 1 | 1 | the proved liquidation's slot, at least the value less the bond paid out |
| `KpfTBoth.both`, `KpfTSettle.settle`, `KpfTProve.prove`, `KpfTLiqSettle.settle`, `KpfTQuietEnd.settle` | 1 | 0 | nothing |
| `KpfTCancel.cancel`, `.expire` | 1 | 0 | nothing (`back`: the owner's key at the order's index) |
| `KpfTMatch.match` | 2, both of the first's id | 1 + what rests | first the account (at least the two margins), then the long's rest, then the short's (the order's slot with the reduced values, at least what is left) |
| `KpfTMerge.merge` | 2, both of the first's id | 1 | the merged account, at least the two values less `MERGE_TIP` |
| `KpfTNovate.novate` | 2, both of the first's id | rest of X + new account + rest of Y | each the model's account, at least the model's value, in that order |
| `KpfTPlace.place`, `.open` | 1, an anchor | 2 | first what enters (the order, at least margin and tip; the account, at least its two margins), then the input's own script again holding no less |

No entry leaves the number of outputs under the object's id open, and none leaves an output's script open. Every
value is held from below only; a richer output is the sender's gift.

## B2 and B3

Objects of two ids are not put together by any of the three two-object entries, and a script under any id but the
library's moves nothing. What ties a book to one library is outside the scripts: an anchor of another library under
the same id would let that library's scripts write under it, so the check of the genesis is the whole protection, as
`lib_check` is for the library. `book_check` refuses every variation tried, and consensus takes six of them.

An anchor is spent by `KpfTPlace` alone. Every other entry builds the state it reads from a 33-byte push and more;
the anchor's preimage is nine bytes. Tested three ways: the kinds' table, an anchor with its real script in each
object's place of the built two-object and proof transactions, and the codec.

## B4 and B5: what unsigned merge and novation cost

At the way in, nothing is written in a party's name: F10 and F11 are closed. A match stays inside the victim's limit,
size, least fill and ask (O2). What the attacker gains is after the match.

**The answer to the question asked:** the most a victim loses through one resting order is not that order's margin.
It is bounded by the order's margin plus (a) all its margin in accounts of the same template in which it stands on
the same side against the same counterparty (merge), plus (b) its margin in any account of the same template in
which it stands on the other side, against anyone (novation: the leaver is paid no less than nothing, so its two
margins there are the bound). A novation also puts a new counterparty in an account, so (a) reaches accounts whose
counterparty can be novated out.

B5. A user long in X and short in Y for the same units is owed, before the caps, a constant C: its two margins and
the difference of the entries. A novation pays it C less 0.015 KAS, at any time anyone chooses. Keeping both it
would be owed C plus d, with min(0, e1, e2) <= d <= max(0, e1, e2), e1 = (S's margin left) - (U's margin in Y),
e2 = (L's margin) - (U's margin in X) - (the transfer). In 1,500 random novations (margins 5 to 400 KAS, entries half
to twice the price) the most U was owed more by keeping both was 207.6 KAS and the most it was spared 137.6 KAS;
the bound was reached 962 times; at the two entry prices themselves the difference reached 62.7 KAS, when the
entries were far apart. So what a third party's timing costs U is its chance to lose only a margin on one account
while collecting in full on the other. S and L are held to margin plus ask as in round 7.

## Findings

**F13 (Medium; a consequence of the design, for the owner to decide).** An order carries a margin, and until round 7
that margin was the most the order could lose. With merge and novation asking nobody it is not. In the test, V's buy
of 10 units rests at its limit with 2 KAS of margin; the market falls a tenth, so those units have lost 10 KAS; K
sells into the order at V's limit. Apart, V has lost the 2 KAS and K has won 2 KAS. (a) V also holds an account
against K, long, 30 KAS each, opened at the market: a third key merges the two, the engine takes it, and V is owed
21.995 KAS where it was owed 30; K is owed 7.995 KAS more. (b) Or V holds an account against S, short, 30 KAS each: a
third key novates V out of the two; V is paid 21.985 KAS where it was owed 30, and K stands against S with 40 KAS
of margin where it posted 30. No signature of V, K or S is in either transaction. This is what a stale limit order
costs on any cross-margined venue; here the user did not choose cross margin, anyone chooses it for it, and the
attacker's take from a stale order goes from the order's margin to every margin of the owner that can be put
together with it. Without a stale order the same step changes V's leverage: a thin order's account is backed by V's
other margin from then on. Directions, none tested: say it plainly in the tool where an order is placed; or let an
order or an account refuse to be put together (a flag in its state that the merge and the novation read); or ask
the signature of the party whose margin share per unit rises.

**F14 (Medium; the tool).** `lib-check --book` and `--book` on `acct-open` and `order-place` refuse the nine
doctored genesis files tried and an anchor of another book. Without `--book` both commands build and print
`WARNING: the book of this anchor ... was NOT checked`. The library's own check refuses unless `--lib-unchecked` is
given; the book's does not, and what an unchecked book costs is F11 (the engine test above). Smallest fix: refuse
without `--book` unless `--book-unchecked`, as for the library.

**F9 again (Low; the design).** With `MERGE_MIN` gone and no signature, anyone merges two accounts of the same two
parties and keeps 0.01 KAS, half from each margin; a novation takes 0.015 KAS from the user taken out. Accounts
exist only by their parties' signatures or orders, so the count is bounded by what they opened; the tool's default
least fill holds an order to ten accounts.

## A correction to round 6

Round 6's `t1b_n5_a_novation_writes_only_the_right_objects_under_the_right_ids` edited a signed transaction and
asked `verify`. The funding input's signature alone fails any such edit, so its refusals proved nothing about the
novation. It is replaced by `t1b_n5_a_novation_writes_under_the_book_exactly_what_its_rule_says`, judged by the
novation's own input, over five shapes. A comment in `t1b_n2_a_novation_pays_the_model_and_nothing_more` claimed a
check `signed` does not make; it now points to the new test.

## Notes

- `thin::merged` still says in its comment that both parties sign a merge.
- Tests removed because their premise is gone: `t1b_fixed_a_merge_is_signed_by_both_parties_in_their_places`,
  `t1b_r7_the_signatures_commit_to_the_transaction`, `t1b_r7_an_object_written_in_a_partys_name_...`,
  `t1b_fixed_the_command_line_novates_no_user_out_without_its_key`; each has a round 8 successor named above.

## Not reached in round 8

- Contention on anchors: every entry into a book spends one of its 1 to 64 anchors, and a signed opening names the
  anchor's outpoint, so whoever spends that anchor first voids it. What it costs to keep others out needs a mempool.
- F13 over time and over chains (a novation that changes the counterparty, then a merge): the two single steps are
  shown through the engine, the chain is argued from them.
- The novation at the raw engine with its inputs in every arrangement, and woven with another operation: its writes
  are tested on the builder's layout only.
- An owner key of another width at `KpfTPlace.place` (the template hash's width is tested).
- A party's two keys equal, and S == L over the grids, as in round 7.

## Tests and commands

93 tests pass: 41 engine, 42 builders, 10 command line. Three of them are named `t1b_finding_...` and assert an
acceptance: F13 and F5 (as before) in the builders file, F14 in the command-line file.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 9

F14 is fixed, the way in is two scripts (`KpfTPlace`, `KpfTOpen`), and `KpfTJoin` is new: 22 scripts, 27 entries.

**Is anything above Low open?** Yes, one new, and one accepted one made wider:

- **F15 (High; a consequence of the join being nobody's signature).** A join cannot be undone and its account is
  read by no merge, novation or join. Anyone, for a net gain of about 0.005 KAS, turns two positions that each
  party could sell and leave into one account that ends only by a close of both markets at the committee's prices
  (the closer pays the tolerance g on the whole notional to the other party) or by both signatures.
- **F13 across markets (Medium; the owner accepted F13 for one market).** A join carries an order's loss beyond its
  margin into the owner's margin in another market against the same counterparty. The NOTE `order-place` prints
  speaks of "this market" only.

Nothing in J1, J2, J3 or J5 is open: the join writes exactly the model's account, is exact before the caps, and what
it writes goes through every two-leg path. F14 is closed (your adaptation of my test is right and passes; I gave
its `acct-open` the `KpfTOpen` file and renamed its printed label). F5 and F9 (Low) stand as before.

| id | What was tried | Result | Test |
|---|---|---|---|
| J1 | 5 pairs (keys in the same places, turned, the first short for its A, both, asks on both) x 24 arrangements of the inputs: taken; the account a sompi short refused by the join's input, a sompi more taken; the second account of another id refused (480) | holds | `t1b_j1_a_join_writes_the_models_account_and_pays_the_two_pots_less_the_tip` |
| J1 | The account under no or another id, missing, twice; an account nobody signed beside it or in its place by each of 4 inputs; either input written again; the first in the joined one's place; the second leg written with the other direction; the script short, under no id, replaced; 10 wrong `who` (145) | holds | `t1b_j1_the_join_writes_only_the_joined_account` |
| J1 | Every other kind of object, an anchor among them, in either place; a two-leg account (the world's, and one a join wrote) in either place, also with its first leg shown; the same account twice; a second account of the first's market; short for its A; of no size; 6 bytes of the leg changed; a leg of 110 and 112 bytes; `turned` wrong, 2, -1; margins swapped or a sompi off; asks a sompi off; other maintenance, window, no-price wait; a third user; margins that do not cover the tip; the first shown otherwise (245, each refused by the join's own input) | holds | `t1b_j1_only_two_one_leg_accounts_of_the_same_two_users_are_joined` |
| J1 | A join beside each of the 22 scenes on a third object of the same book (41) | holds | `t1b_j1_a_join_beside_an_operation_on_an_object_of_the_same_book` |
| J1 | Through the builder, both orientations, the second market of another scale and price limit: one output more by every input, the output replaced, a sompi short, an anchor in either place, a second account of another id (20) | holds | `t1b_j1_a_built_join_writes_no_output_more_and_takes_no_anchor` |
| J2 | 2,000 random pairs (1,016 turned, 1,014 with the first short for its A; five scales, four price limits, entries over each market's range), 40 pairs of prices each, some beyond the limits: each party's margin and gains, and the notional | exact, to the sompi; no rounding | `t1b_j2_a_join_is_exact_before_the_caps` |
| J3 | The fixed part the script builds against `Account::fixed()` of `thin::joined`, byte for byte, and the hash, in every orientation | holds | `t1b_j1_a_join_writes_...`, `t1b_j2_...` |
| J3 | Two joins in the proof world's two markets write, field for field, the two-leg account of the earlier rounds; the join's output then: a close disputed and proved with two tickets, the no-price exit refuted with two ticks, a liquidation disputed and proved with two tickets; and for each orientation a declared close settled, a close proved, a claim of a sompi too much not proved (9 paths) | holds | `t1b_j3_an_account_a_join_wrote_goes_through_every_two_leg_path` |
| J3 | A second account whose leg is not well formed (scale 0, price limit 0, push bytes that are no pushes, entry above the limit) | joined as it is: by design, see below | `t1b_design_a_leg_its_two_users_made_ill_formed_is_joined_as_it_is` |
| J4 | The lock: the joined account in either place of a novation, a merge, a join, at the engine (6); what the way out costs | **FINDING F15** | `t1b_finding_a_join_nobody_signed_locks_two_positions_into_an_account_that_only_a_close_ends` |
| J4 | The caps across markets, one example through the engine | accepted by the engine: F13 across markets | `t1b_j4_a_join_carries_an_orders_loss_into_another_markets_margin` |
| J4 | The liquidation on the whole: 1,500 joins, 90,000 sides and prices | quantified, bound holds | `t1b_j4_what_a_join_does_to_a_liquidation` |
| J5 | The round 8 battery of the way in on each script (the anchor not rewritten, poorer, replaced, out of order, twice; what enters twice, under no id, short; values; signers; two anchors; an anchor beside an operation) (45 + 3); one output more under the id for both (in the 199) | holds | `t1b_b1_the_way_into_the_book`, `t1b_b1_no_entry_writes_an_output_more_under_the_objects_id` |
| J5 | An opening's transaction with `KpfTPlace`'s code in the script's place, a placement's with `KpfTOpen`'s, and each with 17 other scripts' code (36): that input fails, the anchor's own does not | holds | `t1b_j5_each_script_of_the_way_in_refuses_the_others_arguments` |
| B6 | Every earlier battery on 22 scripts and 27 entries; the command line with `KpfTOpen`, `--book` required, `acct-join` | hold | all three files |

## J2 and J3

Before the caps a join is exact: the two legs keep their sizes and entries, so nothing is rounded; each margin gives
0.005 KAS. After the caps the joined account differs from the two apart at 50,071 of the 80,000 points tried, which
is J4.

The state written is the model's. A leg that is not well formed is joined as it is: the join reads the market, the
size and the direction at their places and nothing else, and writes the push before the direction itself. An
account enters the book by the hash of its fixed part and both users' signatures, so such a leg is theirs; the first
account is theirs too. Nobody else's money is in the account written. The model reads it as no account
(`Account::check` and `TObj::decode` refuse a scale of nothing), so the tool builds no claim, dispute or liquidation
on it; a third party that bonds on it by hand does so on bytes it can see. That is the answer: only the two users.

## J4

**The caps (F13 across markets).** V's buy of 10 units of market 2 rests with 2 KAS of margin; the market falls a
tenth (the units have lost 10 KAS); K sells into it at V's limit. V and K also hold an account in market 1, 30 KAS
each. Apart V is owed 30 KAS and K 62. A third key joins the two: V is owed 21.995 KAS, K 69.995. One order in one
market now reaches the owner's margin in every other market's one-leg account with that counterparty, one account at
a time (a joined account is not joined again), and whoever sends the join chooses which.

**The liquidation.** Maintenance is on the whole. Of 90,000 sides and price pairs over 1,500 joins: 20,473 sides
that one of the two accounts liquidated are liquidated by neither after the join; 83 that neither liquidated are
liquidated on the whole. Of the 83, 82 had a loss or gain in one account that ran past that account's pot (the cap
gone: F13 again), 1 was below its maintenance in an account too small to liquidate anybody, none came from the half
tip. No side owed its maintenance in both accounts and more than 0.005 KAS beyond it is liquidated by a join
(asserted). Who gains: a saved side keeps its position and the keeper's fee, at the keeper's cost, and anyone can
send that join for it before the liquidation lands; where the side was under water in one account the other party
gains what the cap had kept from it. A made liquidation pays the keeper on the whole notional (up to 1.445 KAS more
than the fees before, in the sample) and takes both positions.

**The lock (F15).** In the test A is long 5 units of market 1 and 3 of market 2 against B, 100 and 60 KAS each.
Before the join A sells its market 1 position to C through an order and anyone takes it out: A is paid 199.985 KAS,
no price asked. A third key joins the two accounts; no signature of A or B. After it:

- the joined account as either input of a novation, a merge or a join is refused by the entry at the engine (6
  transactions), and by the model and the tool (`acct-join`, `acct-merge`, `acct-novate` at the command line);
- no entry splits it. What ends it: both signatures, or a declared close of the whole account;
- that close pays A 159.595 KAS where the prices say 159.995: 0.4 KAS, the tolerance g (50 bps) on the notional of
  both legs (80 KAS), goes to B. Over 400 price pairs up to 0.58 KAS. At ten times leverage the same 0.4 KAS is 5 %
  of A's margin of 7.995 KAS;
- the close needs a bond on the whole pot (32 KAS here, where the first account alone asked 20), a window, and the
  committee's ticks for both markets; it ends both markets, and there is no way out of one;
- A can still sell to C, but then holds two accounts it cannot be taken out of: margin in both, and a close of each.

How bad: nothing is taken by the join itself but 0.01 KAS. What it removes is the exit by sale, for both parties
and both markets, for good, and it hands the counterparty the tolerance on the whole notional whenever the other
wants out alone (or the same as the price of its signature). It is not a rare event: the sender nets about
0.005 KAS, so whatever can be joined will be, and a sale waiting for its novation is overtaken by a join. A party
avoids it only by never holding two markets against one counterparty, which the order book does not let it choose.
I rate it High because it is permanent, costs the victim a share of its margin that grows with leverage, and is
anyone's to do; it is Medium if the owner holds that a cross account is where two such positions belong.
Directions, none tested: both parties sign a join (it changes what each can do, unlike a merge); or a flag in the
account that the join reads; or let the novation and the merge take one leg of a two-leg account.

## Notes

- `order-place` prints its NOTE about "your other accounts of this market against the same counterparty". With the
  join it is also the accounts of other markets.
- Read, not tested: the join, like the merge, the match and the opening, does not hold a sum of margins to
  `MARGIN_MAX` (the add and the cure do). It takes 11.5 billion KAS in one margin.
- A pairing is the sender's choice: with accounts in three markets against one counterparty, whoever joins first
  decides which two become one, and the third stays alone.

## Not reached in round 9

- The join with its two accounts' parts at other widths than 312 and 138 bytes (the leg at 110 and 112 is refused,
  but by the hash, not shown to be by the width).
- A first account whose own leg is not well formed; a party's two keys equal.
- The join woven with another operation in every layout (one layout per scene is run), and anything that needs a
  mempool: a join racing a liquidation or a novation is argued, not run.
- F15 over time: only the cost of the close at one moment is measured.

## Tests and commands

106 tests pass: 47 engine, 48 builders, 11 command line. Three are `t1b_finding_...` and assert an acceptance, all
in the builders file: F5, F13 (accepted by the owner), F15.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 10

`KpfTJoin.join` now carries the two users' transaction signatures (the first account's A key and B key). The three
files were adapted and F15's tests turned into "fixed" ones.

**Is anything above Low open in the whole family (orders, book, merge, novation, join)?** One thing, by the owner's
decision: F13 (Medium) within one market, where the merge and the novation are still nobody's signature. Nothing
else. F15 is closed. F13 across markets is closed. F5 and F9 (Low) stand; F12 is the tool's default and the design.

| id | What was tried | Result | Test |
|---|---|---|---|
| 1 | The J batteries of round 9 with the two signatures in the join's arguments (engine: 480 + 145 + 245 + 41; builders: J1, J2, J3; command line with `--key` for both) | hold | the `t1b_j...` tests of the three files |
| 2 | Engine, 5 pairs: the two signatures in each other's place, one user's in both, a third key's in either or both (35): only the join's input fails | holds | `t1b_fixed_f15_a_join_is_signed_by_both_users_in_their_places` |
| 2 | Builders, both orientations: a third key alone, A alone, B alone do not sign; another key's signature in a user's place (12), the two swapped (2): refused by the join's input; signed by both: taken | holds | `t1b_fixed_f15_a_join_is_the_two_users_own` |
| 2 | Command line: a third key alone, with one user's key, one user alone: nothing built; with both keys (a third key or one of the two paying): built | holds | `t1b_j_the_join_at_the_command_line` |
| 3 | The join that carries an order's loss into another market's margin: without V's key not signed; K's or the payer's signature in V's place refused; signed by V taken | holds | `t1b_fixed_f13_across_markets_needs_the_owners_signature` |
| 4 | A sompi moved from the change to the joined account after signing (the rule alone would take it); the change paid to another key; the signature script moved to another join of the same two accounts | each refused by the join's input | `t1b_r10_the_signatures_of_a_join_commit_to_the_transaction` |

**F15: closed.** Nobody but the two users together makes a join. What remains is what a joined account is, and it
is now their choice: no merge, novation or join reads it (6 transactions at the engine, again), there is no way out
of one market, and one user alone ends it only by a close of the whole at the committee's prices (0.4 KAS of 160 in
the example, g on the notional of both legs, behind a bond on the whole pot). `acct-join` prints that the two become
one account of two legs; it does not say that the account can no longer be sold or left one market at a time. A
line there would be enough (Low).

**F13 across markets: nothing left that a party did not sign for.** The join is the only entry that puts two
markets together and it asks the party's own signature; that signature is for one transaction, so for the two
accounts as they stood (a merge or a novation of either before the join lands spends its outpoint and voids it).
After a join nothing unsigned reaches the joined account: it is no input of a merge, a novation or a join (model
and engine). What the owner accepted stays inside one market: an order's account merged with, or novated against,
the owner's other one-leg accounts of that market.

The liquidation figures of round 9 (`t1b_j4_what_a_join_does_to_a_liquidation`) are the model's and unchanged; a
join that saves a side from a liquidation now needs the other party's signature too.

## Not reached in round 10

- As in round 9: the join at other widths of its two parts, a first account whose own leg is ill formed, two keys
  equal, every layout beside another operation, anything that needs a mempool.

## Tests and commands

108 tests pass: 48 engine, 49 builders, 11 command line. Two are `t1b_finding_...` and assert an acceptance, both
in the builders file: F5 (Low, open on purpose) and F13 (Medium, accepted by the owner).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 11

The match is lighter (no `rest` flag, no tips output) and three scripts work on one side of an account of two
legs: `KpfTTurn`, `KpfTFold`, `KpfTLeave`. 25 scripts, 30 entries.

**Is anything above Low open?** Yes.

- **F16 (Critical). A user alone takes a cross account apart and keeps what the caps uncover.** `KpfTLeave` asks
  nobody, splits what S asked of U by the legs' entry notionals, and leaves U's whole margin on what stays. With a
  second key U hands its losing leg over at the leg's own entry with the ask's share behind it, or sells the winner
  and is then novated out of the one leg left. In the example S is owed 10 KAS less of 55 at once, and U's keys
  that much more; over a grid, up to 43 KAS. This is L1: the rule "U is paid nothing of X" does not hold it.
- **F17 (Medium). The same split from the other side.** When U sells a leg, S's margin leaves with it in the leg's
  share whatever S owes on the other leg. U's resting sell, taken by S's second key: U is owed 15 KAS less of 75.
- **F13 (Medium, accepted by the owner)** now also reaches a cross account: a fold pools an order's account into it.

L2 to L6 found nothing else: the three scripts write the model's state and nothing more, the fold is exact to a
sompi before the caps, a turn changes what nobody is owed, every branch runs at the bounds.

| id | What was tried | Result | Test |
|---|---|---|---|
| A | The O battery on the lighter match: 8 cases x 24 arrangements, random matches, 213 matches outside what the orders say, the writes under the id, a match beside every scene, cancel, expire, the ask, the least fill | holds | the `t1b_o...` and `t1b_fixed_...` tests, adapted |
| A | A match asks no place from the end and pays no key | holds | `t1b_o4_a_match_asks_no_place_from_the_end` |
| A | An order used up that held 5 KAS more than its margin and tip | the sender has it: by design | `t1b_o6_what_rests_is_an_order_that_can_be_filled` |
| L1 | U alone, with a second and a third key: the loser handed over at its entry; the winner sold and U novated out | **FINDING F16** | `t1b_finding_a_user_alone_takes_a_cross_account_apart_and_keeps_what_the_caps_uncover` |
| L1 | U's resting sell of one leg taken by S's second key, in a real book | **FINDING F17** | `t1b_finding_a_sale_of_one_leg_takes_the_others_margin_from_under_the_seller` |
| L1 | 1,500 random leaves (U and the third user as one, any entry), 82 price pairs each: S after the caps, summed over its accounts | S is owed less at 198 of 123,000, each time by a cap, never at the second account's entry price; before the caps nothing moves (3 sompi) | `t1b_l1_what_a_leave_does_to_the_one_that_stays_over_a_grid` |
| L1 | A fold of an order's account into a cross account | accepted by the engine: F13's kind | `t1b_f13_a_fold_carries_an_orders_loss_into_the_cross_account` |
| L2 | 63 built transactions (5 turns, 22 folds, 36 leaves, every branch): each taken; 459 changes refused: one output more under the id by every input, each output replaced, short, under another id, the leave's outputs out of order, an anchor in each object's place, the second object of another id, the user's payment short or to another key | holds | `t1b_l2_turn_fold_leave_in_every_branch_write_what_the_model_says_and_nothing_more` |
| L2 | The turn's two scenes in every engine battery of the earlier rounds (who, ages, widths, kinds, a rewrite short, an output more, two operations on one book) | holds | the `t1b_a...`, `t1b_b...`, `t1b_c...`, `t1b_d...` tests |
| L3 | 3,000 random folds (same way 1,452; other way with the first leg larger 611, smaller 612, equal 325; one leg or two; either direction; either key order; four scales), 30 price pairs | each user's worth before the caps is the two accounts' less half the tip to 1 sompi; the pot exact | `t1b_l3_a_fold_is_the_two_accounts_before_the_caps_to_two_sompi` |
| L3 | The leg written (size, direction, entry) in each branch, through the engine | the model's | `t1b_l2_...` |
| L4 | 2,000 random accounts, users turned and legs turned, 20 price pairs: owed before and after the caps, liquidatable, what a close pays, the keeper's fee, the cure | the same, exactly | `t1b_l4_a_turned_account_owes_what_it_owed_and_goes_through_the_claims` |
| L4 | Mode 0, 3, -1; the legs of an account of one leg; the account as it was or the other mode's in the output; margins or a leg not turned; a sompi less | refused | `t1b_l4_a_turn_writes_the_account_the_other_way_round_and_nothing_else` |
| L4 | The age: a turn stating 99; the account's input at a sequence of 99 under a turn stating 100; and the ages battery | refused by the turn's input, by the account's own | `t1b_l4_a_turned_account_...`, `t1b_a5_...` |
| L4 | An account of one leg short for its A, made by a turn: a close declared and settled, disputed and proved, a false claim not proved (both sides); the no-price exit refuted; a liquidation disputed and proved (8 paths) | holds | `t1b_l4_a_turned_account_...` |
| L4 | That account as either input of a merge and of a novation (4) | refused by the entry | `t1b_l4_a_turned_account_...` |
| L5 | 600 leaves: the three shares against the exact proportion; S's margin before and after; a leg in four parts; a leg of less than 0.01 KAS; the weight at and over its limit | floors; nothing lost; see below | `t1b_l5_the_share_of_a_leave_its_rounding_and_its_bounds` |
| L5, L6 | Leaves at sizes of 2,000,000,000, a weight of 2,000,000,000 and margins of 5 x 10^17 sompi (4), and the 36 branches of a leave, the 22 folds, the 5 turns | taken by the engine at the model's numbers | `t1b_l5_...`, `t1b_l2_...` |
| cli | `acct-turn` (both modes), `acct-fold`, `acct-leave` by a third key; F16 (1) built at the command line | builds | `t1b_l_turn_fold_leave_at_the_command_line` |

## A: what the two removals lose

- No `rest` flag: what a match leaves always rests. An order that used to return what was left in the match now
  either is taken whole (least = size) or leaves a rest its owner must cancel. "Take what there is now and give me
  back the remainder" is no longer an order; the remainder rests until its owner cancels it, and a rest is matched
  past its expiry by the script (F7). Low.
- No tips output: whoever sends the match has the tips. A front-runner that copies a match takes them; before, the
  matcher's key was an argument nobody signed, so a copy with another key took them too. Nothing changed.
- The excess of an order used up was returned to its owner and is now the sender's. Only the owner (its signature
  at `KpfTPlace`) or a matcher giving more to a rest puts an excess in an order, so nobody takes what was not given;
  an owner that overfunds by mistake loses it at the last fill where it got it back before. Low.

## L1

The leave is exact before the caps (grid: 3 sompi). After them it splits one pot into two on S's side and on U's
side, and nothing in it reads a price. What it holds S to is its ask, in the share of the leg; what it holds U to
is that it is paid nothing of X. Neither is enough.

**F16 (Critical).** In the test a leg is 100 KAS of notional; U and S hold 20 KAS a leg each, each asking of the
other's replacement what the other posts (the tool's default). S signed the join and nothing else.

1. Market 1 has fallen 30 %, market 2 risen 15 %. S is owed 54.995 KAS, U 24.995; U holds its maintenance. U's
   second key opens with U an account long against U in market 1 at the leg's own entry, posting 20 KAS (S's ask
   for that share). A third key sends the leave. S is owed 44.995 KAS: 10 less, what the loser lost (30) beyond
   the share (20). U's keys are owed 9.985 more.
2. Market 1 has risen 15 %, market 2 fallen 30 %. The second key buys the winner at the price, posting 35 KAS. U
   is paid nothing of X, and after the leave alone S is owed what it was. But what stays is an account of one leg
   in which S's ask of U is 20 where U's margin is 55. A third key opens against U at that leg's own entry with
   20 KAS, and anyone novates U out: U is paid 55.98 KAS in cash; S is owed 44.995, 10 less.

Both through the engine, no signature of S. The share is by entry notional, and a turn of the legs lets U choose
which leg is first, so the leg left behind can be the one with the small share: over the grid S loses up to
43.26 KAS in one leave. I rate it Critical by the measure of F11: one party alone moves the other's owed worth at
the prices of the moment, at no risk.

Directions. (a) The one that stays signs a leave, as it signs the join: closes F16 and F17, and makes the exit by
sale depend on the counterparty again. (b) Keep it unsigned and hold S whole by its ask: the one that comes in
posts S's WHOLE ask plus U's gain, and what stays keeps S's whole ask plus U's gain. In the model, on the example,
S is then owed what it was in both cases (asserted); U's side posts more than before to take the account apart,
which is the price of doing it without S. I did not test (b) beyond the example.

**F17 (Medium).** U long in two markets against S, 40 KAS each side; S owes U 35 on the second leg; U's sell of
the first rests at the price. S's second key takes it; a third key sends the leave. 19.9975 KAS of S's margin
leaves with the leg to an account between S's two keys. U was owed 74.995 KAS and is owed 59.9925; S's keys 4.995
before, 19.9975 after. U signed the sell, and S was below its maintenance (U could have had it liquidated first),
so this is the resting-order risk of F13 turned round. The rule you proposed (what stays holds what U asked of S
in the share that stays) does not refuse this example: the share that stays is 20 and S's margin left is 20. A
share cannot keep what a pool kept. What U can be given without a price is a knob: let U's ask of S on what stays
not shrink, so that a user who asks everything is never left with less of S's margin than it asked.

**The fold.** The same two users only, so neither makes the second account alone. What it does is F13: an order's
account is pooled into the cross account. V's buy of market 1 with 2 KAS of margin has lost 10; apart V is owed
59.995 KAS, folded 51.99.

## L4: turns

A turn spends the account and writes it young; the next turn states 100 DAA again. What is tested: the turn's
input refuses an age below 100, and the account's input refuses a sequence below the age stated. The wait itself
is the consensus rule on sequences, which these tests do not run. A party about to be liquidated can overtake the
liquidator's transaction with one turn; the turned account is then liquidated as it stands (tested: the
liquidation of a turned account is proved), and no second turn is valid for 100 DAA. Once a liquidation or a
claim is declared the object is no account and is not turned. A turn also voids a join or a close by both that
was signed for the old outpoint. Low.

An account of one leg short for its A is closed, disputed, proved, refuted and liquidated like any other. The
merge and the novation refuse it until it is turned back; the fold, the leave and the join read either direction.

## L5

`wm` and `wall` are floors to 0.01 KAS; S's margin, S's ask and U's ask are floors of `v * wm / wall` exactly.
The odd sompi stay on what stays. Nothing of S's margin is lost in one sale or in four. A leg sold in four parts
leaves S's margin behind U within 0.19 KAS (of 100 to 800) of what one sale leaves, more in 463 cases of 600 and
less in 66: each step floors the weights again. A leg of less than 0.01 KAS of entry notional does not leave;
one of exactly 0.01 does. Many small sales would move the share by more, at 0.015 KAS a sale: not worth it. With
`wall` up to 2 x 10^9 and a margin up to the cap, `(v % wall) * wm` stays below 4 x 10^18: no overflow, and the
engine takes four leaves at those bounds.

## L6

Every branch I could build ran to its end in the engine: 36 leaves (first leg long or short; a part, the whole,
more than the leg; an entry below, at, above; the user's margin above the sender's share and exactly it), 22
folds in the four branches, 5 turns, and 4 leaves at the bounds. None failed for stack or size. Not run: the one
path the scripts refuse by design that the model also refuses to build (a fold that cancels the only leg).

## B1 for the three

| Script.entry | In | Out | What is written under the id |
|---|---|---|---|
| `KpfTTurn.turn` | 1 | 1 | `becomes`: the turned account, at least the input's value |
| `KpfTFold.fold` | 2, same id | 1 | the folded account, at least the two values less the tip |
| `KpfTLeave.leave` | 2, same id | 2 + rest | what stays (at least X less S's share), the new account, what is left of the second, in that order |

## Not reached in round 11

- Direction (b) for F16 over a grid, and any rule for F17: only the examples.
- The fold and the leave at the raw engine with their inputs in every arrangement and beside another operation:
  they are tested on the builder's layout, judged by the entry's own input.
- F16 chained (a leave after a fold, repeated leaves after turns), and anything that needs a mempool: a turn
  racing a liquidation is argued.
- An account whose own bytes are not well formed under the three scripts.

## Tests and commands

118 tests pass: 49 engine, 57 builders, 12 command line. Four are `t1b_finding_...` and assert an acceptance, all
in the builders file: F5, F13 (accepted), F16, F17.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 12

`KpfTLeave.leave` now carries the transaction signatures of the account's two users. The L tests were adapted and
F16 and F17 turned into "fixed" ones.

**Is anything above Low open in the whole family?** Yes, one new, and the accepted one:

- **F18 (High). F16 again by a fold that cancels a leg and a novation, both nobody's signature.** U sells its
  winning leg into S's own resting order, anyone folds that account into the cross account (the leg cancels, U's
  gain moves into its margin, the loser is alone in an account of one leg), and U is novated out against a second
  key posting what S asked. In the example S is owed 20 KAS less of 75 and U's keys 20 more.
- **F13 (Medium, accepted by the owner)**, as before, and by a fold into a cross account.

F16 and F17 are closed. F5, F9 and the notes of round 11 (Low) stand.

| id | What was tried | Result | Test |
|---|---|---|---|
| 1 | Round 11's L batteries with the two signatures (63 built transactions, 459 changes; the changes to a leave made before signing, so that its rules and not its signatures refuse them; 4 leaves at the bounds) | hold | `t1b_l2_...`, `t1b_l5_...` |
| 1 | F16's two leaves: without S's key, without U's, with neither: not signed; U's, the third user's or the payer's signature in S's place, S's, the third user's or the payer's in U's, the two in each other's place (14): refused by the leave's own input; signed by both: taken | holds | `t1b_fixed_f16_a_leave_is_signed_by_the_one_that_stays` |
| 1 | F17 in a real book: U's resting sell taken by S's second key (the match asks nobody), the leave built from that account: without U's key not signed; S's, the second key's or the payer's signature in U's place, the two swapped: refused; signed by both: taken | holds | `t1b_fixed_f17_a_sell_order_is_not_the_sellers_signature_of_the_leave` |
| 1 | Command line: a third key alone, with the user's key only, with the other's only, the user alone: nothing built; with both keys: built | holds | `t1b_l_turn_fold_leave_at_the_command_line` |
| 2 | The worst unsigned chain found: S's resting buy filled by U, a fold that cancels the leg, a novation of what is left | **FINDING F18** | `t1b_finding_a_fold_that_cancels_a_leg_and_a_novation_take_a_cross_account_apart` |
| 3 | A sompi moved from the change to each of the three outputs under the id after signing; the change or the user's payment paid to another key; the signature script moved to another leave of the same two accounts (6) | each refused by the leave's own input | `t1b_r12_the_signatures_of_a_leave_commit_to_the_transaction` |

## What is still nobody's signature

| Entry | What it does to pots |
|---|---|
| match | makes an account from two signed orders, inside each order's limit, size, least fill and ask |
| merge | pools two accounts of one market, same way (F13) |
| fold | pools an account of one market into an account's first leg (F13); the other way it CANCELS: what cancels is realized between the two margins, and when the whole leg cancels an account of two legs becomes an account of one |
| novation | one leg only: takes a user out whole or in part; the two that stay are held to their asks |
| turn | rewrites |
| expire, settle, late, lapse, the liquidation's and the no-price ends | pay by the rule; dispute, liquidate and the proofs are anyone's with a bond |

Signed: the way in, add, cancel, close by both, declare, cure, join, leave.

Of these, one path takes two positions set against each other out of one pot: a fold that cancels a whole leg,
followed by a novation of the leg that is left. The fold is exact before the caps and leaves S owed what it was
(tested: to the tips). But it writes the user's realized gain into its margin without adding it to what the other
asked, and it turns the account into one the novation reads. The novation then holds the one that comes in to
S's ask, and U's margin is above it by exactly the winner's gain.

## F18 (High)

In a real book, through the engine. A leg is 100 KAS of notional; U and S hold 20 a leg each, each asking what the
other posts; they joined. Market 1 has risen 35 %, market 2 fallen 65 %: U is owed 9.995 KAS and holds its
maintenance; S is owed 69.995 and has a buy of market 1 resting at the price (5 KAS of margin, asking 5).

1. U sells its ten units into S's buy at S's limit. The match asks nobody.
2. A third key folds that account into the cross account. The first leg cancels; U's 35 moves into its margin
   (79.99); the loser is the account's one leg; S's ask on it is 45 (U's 40 and the order's 5). S is still owed
   what it was.
3. U's second key opens against U at the loser's own entry, posting 45. A third key novates U out: U is paid
   80.975 KAS in cash. The loser has lost 65 with 45 behind it.

S is owed 54.99 KAS where it was owed 74.995 (its order's 5 counted): 20.005 less. U's keys are owed 29.975 where
9.995: 19.98 more. S signed its join and its own buy, which was filled at its own limit.

How it differs from F16: S is held to its WHOLE ask, not a share, so S loses only what the loser has lost beyond
everything S asked (with a loss of 45 S is owed what it was: asserted). And U needs an account of the same two
users, the other way and of the leg's size: here S's own resting order, of which U takes exactly that size. That
is why High and not Critical. The mirror (S against U, through U's resting order) is the same arithmetic; I did
not run it.

Directions, the first checked in the model on this example only:
- the fold adds to a user's ask what it moves into the other's margin (as the novation's first rule adds the
  transfer). The novation then asks the second key for U's whole margin, 80, and S is owed what it was;
- or a fold that leaves an account with fewer legs is signed by both, as the leave is.

It is your (b) argument turned round: what is lost is the netting, and no rule without a price gives it back in
every case. The first direction holds S to "its counterparty's margin is never below what it was after the
fold", which is what the pot gave S at the fold's own entries; I have not tested it over a grid.

## Not reached in round 12

- F18 from S's side, with partial cancels over several orders, and after a turn of the legs; the first direction
  over a grid.
- The leave's signers at the raw engine with the inputs in other arrangements (the builder's layout only).
- As before: the fold and the leave beside another operation; anything that needs a mempool.

## Tests and commands

120 tests pass: 49 engine, 59 builders, 12 command line. Three are `t1b_finding_...` and assert an acceptance, all
in the builders file: F5, F13 (accepted), F18. Round 11's two finding tests were replaced by the `t1b_fixed_f16_...`
and `t1b_fixed_f17_...` tests above.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 13

F18 is fixed by direction (1): the fold and the leave add to the one that stays' ask what the cancelled leg (or the
realized gain) moved into the mover's margin. The L tests were adapted and F18 turned into a "fixed" one.

**Is anything above Low open in the whole family?** No, except what the owner has accepted: **F13 (Medium)**, a
resting order's margin is not the most it can lose once its account is merged, folded or joined with another of the
same market and counterparty. Everything else above Low is closed.

| id | What was tried | Result | Test |
|---|---|---|---|
| 1 | F18's three steps in the real book: U sells its winning leg into S's resting buy, anyone folds it (the leg cancels), U novated out against a second key. The fold now asks 80 of U's replacement (was 45); a novation posting 45 is refused by the novation's own input (model, tool, engine), posting 80 S is owed what it was | holds | `t1b_fixed_f18_a_fold_asks_of_the_takers_replacement_what_it_took_into_its_margin` |
| 1 | The direction in the model: a fold and a leave of default-ask accounts leave each ask at least its counterparty's margin (2,000 folds, 811 leaves) | holds | `t1b_l13_a_gain_taken_into_a_margin_is_asked_of_whoever_takes_that_users_place` |
| 2 | The property over a chain: 2,000 cross accounts, 3,989 unsigned steps (folds that cancel part or all of either leg through an order of either user at any price, turns of users and legs between), then the worst novation the mover builds posting exactly the ask; the other user's clamped worth at a grid | never owed less (worst 0 sompi, to the tips); the invariant held every step (0 breaks) | `t1b_l2_no_unsigned_chain_takes_from_a_user_whose_asks_are_the_default` |
| 1 | The L2/L5 batteries with the new asks: the leave keeps X-after's asks whole plus the gain bump; the fold bumps the payer's ask | hold | `t1b_l5_...`, `t1b_l2_...` |

## 1. The fix

`KpfTFold`: after the two users' asks are summed, what A's margin took from B's (`da`) is added to the ask of
whoever would replace the taker (`da > 0`: B took, so B asks more; `da < 0`: A asks more). `thin::folded` the same
(`a_ask + (-da).max(0)`, `b_ask + da.max(0)`). `KpfTLeave`: X-after keeps both asks whole (round 11 had shrunk
them by the share, which was F16's second step), and the gain `gu` is added to S's ask when positive, to U's when
negative. In F18 the whole winning leg cancels, so `da = 35`, and S's ask on the loser goes from 45 to 80; the
novation that would take U out against a key posting 45 is refused, and against 80 S is owed what it was.

## 2. The property holds

For an account where each user's ask is at least its counterparty's current margin (what `acct-open`,
`order-place`, `order-match`, `merge` and `join` all give, to the sompi of the tips and rounding), a fold or a
leave leaves it so (L13: worst shortfall 0, below zero to the tips). Round 7 then gives that a novation never
leaves that user owed less. The chain test strings folds that cancel, turns and a final novation at the worst
entry the mover can choose, from both sides: the other user is never owed less (worst 0). So no unsigned chain of
match / merge / fold / turn / novation takes from a user whose asks are the default.

## 3. Asks below the default

The bump adds to a user's *existing* ask what the margin took; it does not raise an ask that was below the
counterparty's margin up to it. A user that chose to ask less than the other posts can still be left owed less by
a leave or a novation, by exactly the gap it chose — its own decision, the shape of F12.

## 4. What else moves margin between two users unsigned, without adding it to an ask

| Where | Largest it moves | De-pool? |
|---|---|---|
| merge's entry-rounding delta | the quantization of blending two same-direction entries into one floored average: bounded by the leg's scale, measured at 0 sompi over 4,000 merges (M2) | no: both legs are long for A, nothing is set against each other |
| novation's partial-unit transfer | the price moved on the units taken; held by R1 (`ays >= sq + tr`) and R2, so added to what the asks require | covered |
| join | nothing: each leg keeps its size and entry, no realized gain is moved | no |
| fold's same-way rounding | a few sompi (the floor of the blended entry), now in the ask bump | no |

None is above Low. Only the merge delta and the fold's same-way rounding move anything not held by an ask, and
both are rounding, not a position's worth.

## Not reached in round 13

- F18 from S's side with partial cancels over several orders (the chain test covers random partial cancels through
  the model; the full engine run is the one worked example from U's side).
- Asks below the default over a grid (F12's measure, one line here).
- As before: anything that needs a mempool; the fold and the leave beside another operation at the raw engine.

## Tests and commands

122 tests pass: 49 engine, 61 builders, 12 command line. Two are `t1b_finding_...` and assert an acceptance, both
in the builders file: F5 (Low, on purpose) and F13 (Medium, accepted by the owner). Round 12's F18 finding test was
replaced by `t1b_fixed_f18_...`.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 14

The thin family signed on two machines: `sign_apart` / `sign_open` / `sign_more` / `finished` in the library,
`--partial` and `cosign` at the command line. No contract changed.

**Nothing above Low was found.** One Low finding (F19, wording only). F13 (Medium) stays accepted by the owner.

What I could run is the library and the command line offline (`--inputs-unchecked`). Everything that needs a node
(the co-signer's lookup of the inputs, `found`, `lib-status`, `acct-show --book`, `refresh`) is read, not run, and
is listed as such below.

| id | What was tried | Result | Test |
|---|---|---|---|
| a | The four transactions two users sign (close by both, join, leave with an executor paying, opening), signed apart and finished: taken by `verify` at the committed budgets | holds | `t1b_r14_a_signature_made_apart_serves_its_transaction_and_its_place_only` |
| a | The place of the co-signer's signature moved (the start of the signature script, a byte before, a byte after, the script's own bytes at the end; 16): the co-signer signs, the engine refuses the transaction. A place that ends beyond the script (4): not signed | holds | same |
| a | The co-signer's signature asked for the object's own input as well | the engine refuses the transaction | same |
| a, b | After every signature is in, each argument of the script changed in turn (every push before the script itself, 45 over the four) | each refused | same |
| a | An object's output under an id the transaction does not give: an id no input carries, a plain coin as the authorizing input, the co-signer's open coin put under the book's id by the file (12) | `bound` gives none | `t1b_r14_an_output_is_of_a_book_only_if_the_transaction_gives_it_that_id` |
| a | A file that lists the co-signer's own coin among the signatures a script waits for | signed: **FINDING F19 (Low)** | `t1b_finding_a_file_lists_a_co_signers_coin_among_the_signatures_a_script_waits_for`, `t1b_finding_the_co_signer_signs_a_coin_of_its_own_that_the_file_does_not_call_left_to_it` |
| a | At the command line: a place beyond the script, the script's input left to a key, another library in the file, a state of the pool changed, the inputs not looked up without `--inputs-unchecked` | each refused, nothing signed | the second of the two above |
| b | The close the counterparty signed and passed on, and its own `acct-add-margin`, spend the same outpoint; the last signer's file is whole and verified; a key that already signed is asked nothing | holds | `t1b_r14_a_signature_passed_on_is_taken_back_by_spending_the_account` |
| c | `finished()` returns a transaction whose script fails (a spoiled signature); `verify` refuses it | by design: see (c) | first test |
| c | No twin key is in what leaves the machine (entries, outputs, signature scripts) | holds | first test |

## (a) The file against the co-signer

A signature made by `sign_more` is the key's signature of one input's hash of this transaction. It is worth
something only where that input's script checks that key. So what the writer chooses (the place, the input, what
the file calls an input) can spoil the transaction or obtain the signature the co-signer meant to give, and
nothing else that I could find:

- **The place.** Wherever the file puts it, the signature is the same 65 bytes; the writer has them anyway once
  the file comes back. Written elsewhere the transaction is refused (tested). It is not a way to a different spend.
- **The arguments.** Not signed, and it does not matter: each of the four entries holds its arguments to the
  outputs or to the object's state, and the outputs are signed. Every push changed after signing is refused
  (tested), the stated age included (the object's input holds it to the sequence, which is signed).
- **What an input is.** With a node the tool looks every input up and compares value, covenant id and script.
  Offline it says so and asks for `--inputs-unchecked`. The entry of the input being signed is in the hash, so a
  file that lies about THAT input gets a signature that is no good; a lie about another input (an account shown as
  somebody else's) would get a good one, which is what the lookup is for. I did not run the lookup (no node).
- **States in the pool.** A state is held to the script of the input or output it explains; two states with one
  hash would be a collision. A state changed in the file is refused (tested).
- **Outputs.** A plain coin, a script that goes on under the library's id, or an object of the pool under the
  book of the objects spent, holding at least what its state says; `bound` reads the book from the transaction
  (tested). An object holding MORE than its state says is shown with its value and not remarked on: the co-signer
  sees what it puts in and what the object holds, but has to do the sum itself (Low, a remark, not tested).
- **A second script or object.** Every input is printed; an object of another book than the first is refused.

**F19 (Low).** `sign_more` signs what the file lists and holds `open_sigs` only to "the place is inside the
signature script". A file can list the co-signer's own coin there instead of in `open`, with a push of 65 bytes as
its signature script. The co-signer's key spends the coin; the transaction is the honest file's, to the byte. What
changes is the wording of `cosign`: the coin's line reads "(yours)", the words for a coin of yours already signed,
and "YOU SIGN THIS" stands once where the honest file has it twice. The total ("in plain coins you put in 61 KAS")
is right in both. Nothing is taken that is not shown; a reader who counts the "YOU SIGN THIS" lines is misled about
how many inputs it signs. Smallest fix: `sign_more` and `cosign` take an `open_sigs` entry only for an input that
is a script of the library, and `cosign` words a coin by whether a signature of the key is still to be made.

## (b) The co-signer or the last signer against the writer

The last signer has every signature and can change only what is not signed: the scripts' arguments, which are
refused when changed (tested). It can hold the file and send late; the NOTE says so. Its advice is sound: the
transaction spends the account's outpoint, and `acct-add-margin` spends the same outpoint and writes the account
anew (tested: both name it). Two limits worth a word in the NOTE: it is a race, not a recall (if the holder sends
first, its transaction stands); and for an OPENING the object among the inputs is an anchor, which the co-signer
cannot move alone, so there its own coin is what it spends. A replay needs the same outpoints, which are gone
once either lands.

## (c) The twin

The twin's keys are hashes of the public keys, so anyone can make them; they sign only a transaction that never
leaves the machine and whose objects and coins (other keys, so other scripts) do not exist. No twin key is in
the file (tested). The swap keeps equalities between keys (it is one hash per key), which is all the builders
branch on (the same user in two places, the keys turned), so the path and the sizes are the real ones: the
finished transaction is taken at the committed budgets (tested), and your test holds it equal to the one-machine
transaction to the byte. A budget too small: `finished()` itself is not a verdict (it returns a transaction whose
script fails, with 0 units for that input; tested); the tool's last signer runs `verify` in `finish_signed` and
sends nothing that fails (read). A program that used `finished()` without `verify` would send what the node
refuses: harmless, but worth a line in its comment.

## (d), (e): read, not run

- `found`: asks the node for the UTXOs at the script's (or the anchor's) address and keeps those under the id.
  A third party can pay to that address but cannot put its output under the library's or the book's id: an id is
  carried on only by an input that has it, and every script and the way in write under it exactly what their
  rules say (B1). So `covenant_id == id`, from an honest node, with the genesis checked, is a true copy or anchor.
- A lying node, here and not before: it chooses WHICH copy or anchor a user builds on (one that is spent, or
  the one another user was given: a transaction that fails, never one that takes). In the co-signer's lookup it
  could vouch for inputs that are something else, which is what `--inputs-unchecked` already warns of. The lookup
  compares the script only when the node gives one (`u.spk` is optional) and finds an outpoint in the list of all
  the inputs' addresses together: a node that left the script out would let two inputs of one book and one value
  be shown in each other's places. Both are still shown as spent; I see no gain. Hardening: refuse a listed
  UTXO without a script, and match each input at its own address.
- `refresh`, `lib-status`, `acct-show --book`: not reached.

## Not reached in round 14

- Anything with a node: the co-signer's lookup (an input shown as one thing that is another), `found` with
  outputs of a third party at the address, a stale node, `refresh`'s change, `lib-status`, `acct-show --book`.
- The join and the leave from two machines at the command line (the library runs all four; the command line
  runs the opening and the close by both).
- An object output holding more than its state says, followed through to who is paid the excess.
- Lock time and sequences of a file (they are signed, and not printed by `cosign` beyond the last signer's
  "usable from" line).

## Tests and commands

127 tests pass: 49 engine (unchanged, rerun), 64 builders, 14 command line. Four are `t1b_finding_...` and assert
an acceptance: F5 (Low, on purpose), F13 (Medium, accepted), and F19 twice (Low; the library and the command
line).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 15

F19 is fixed and the notes of round 14 are taken. **Nothing above Low is open** but F13 (Medium), which the owner
accepted. My two F19 tests now test the refusal; two things round 14 did not reach are done offline.

| id | What was tried | Result | Test |
|---|---|---|---|
| F19 | The library: the co-signer's coin listed among the signatures a script waits for; the first machine's coin listed so, with or without its key at hand | refused, nothing signed | `t1b_fixed_f19_a_coin_is_not_signed_as_a_signature_a_script_waits_for` |
| F19 | The command line: the same file; the coin listed both ways at once; an object's input said to wait for a signature | REFUSED, nothing signed | `t1b_fixed_f19_the_co_signer_signs_no_coin_the_file_does_not_call_left_to_it` |
| notes | The honest opening: "YOU SIGN THIS" twice, the total right, the account remarked on for what it holds beyond its state; the writer's NOTE says "whichever lands first stands" and that for an opening it is the signer's coin | holds | same |
| notes | A place beyond the script, the script's input left to a key, another library, a changed state, the inputs not looked up | refused as in round 14 | same |
| new | A join and a leave from two machines at the command line: one user writes with `--partial`, the other signs with `cosign`; no book asked; one "YOU SIGN THIS"; each object shown with "yours in it" (the leave's new account: none of the seller's keys); the same txid as one machine with both keys; a key that is no signer's, the writer's own, and the leave's third user are asked nothing | holds | `t1b_r15_a_join_and_a_leave_from_two_machines_at_the_command_line` |
| new | An account holding 7 KAS more than its state says, followed through the engine: a close declared by A and by B and settled; a margin added; a close by both | see below | `t1b_r15_what_an_account_holds_beyond_its_state_goes_to_the_party_a_close_is_declared_against` |

## The excess, followed

What an account holds beyond its two margins (its carrier, or more) is paid, when a declared close is settled, to
the party the close was declared AGAINST: all 7 KAS, whichever side declared (both run). An added margin carries
it on in the account written anew. A close by both pays what the two sign. So `cosign`'s remark ("it holds N KAS
more than its state says (its carrier): the state gives that to nobody") is true of the state, and the rule of a
close gives it to whoever did not declare. A writer that left an excess in an account cannot take it by closing:
it goes to the other. Low, no change asked; the remark could name who is paid it.

## Read, not run

- The node lookup by outpoint and script, and that a listed UTXO without a script is not taken: read in
  `cosign_cmd`; no node here.
- `finished()`'s comment: read.

## Remarks (Low)

- `sign_more` in the library still takes an `open_sigs` entry that names an object's input (the engine refuses the
  transaction; round 14's test still shows it). The command line refuses it before signing. A library user is held
  by the engine only.
- With `--partial`, a key file given as `--funder` only names the payer: what signs is `--key`. Written with
  `--funder KEYFILE` and no `--key`, the file carries no signature of the writer, the lines say so ("LEFT TO SIGN",
  "WAITS FOR THE SIGNATURE OF" the writer's own address), and the closing line still reads "every signature of
  yours is in it". Nothing is at risk; the line could say when there is none.

## Not reached in round 15

- Anything with a node, as in round 14: the lookup itself, `found`, a stale node, `refresh`, `lib-status`,
  `acct-show --book`.
- Lock time and sequences of a file.

## Tests and commands

129 tests pass: 49 engine (unchanged, rerun), 65 builders, 15 command line. Two are `t1b_finding_...` and assert an
acceptance, both in the builders file: F5 (Low, on purpose) and F13 (Medium, accepted).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
```

# Round 16

The tool now reads transactions back from the chain (`thin_read::read_tx`, `book-sync`) and runs an unattended
executor (`exec-run`). No script changed. No node here: what needs one is read, not run, and marked so.

**Nothing above Medium. One Medium (F20), one Low (F21).** F13 (Medium) stays accepted.

| # | What was tried | Result | Test |
|---|---|---|---|
| 1 | `read_tx` on a fold, a match and an opening as the builders lay them out, and on every reading below: each made object against its output's script and covenant id, each spent one against the hash its input carries | never a wrong object | asserted in `read_signed` (auditT1_builders.rs) on every reading |
| 1 | The same fold and match with the second object at the lower input (`who` = 1, 0, 0): through the engine, then read | taken by the engine; read with nothing made: **F20 (Medium)** | `t1b_finding_a_transaction_with_its_two_objects_in_the_other_order_is_not_read_back` |
| 1 | An opening whose payload tells another account (the payload is no script's concern), the real account's parts known to the reader | the account is not read (never the wrong one) | same |
| 2 | 26 claims and liquidations with `!is_true()`, one leg and two: an amount a sompi off, 0, the whole pot; a close by a side the rule liquidates; a fee or cure a sompi off; a side the rule does not liquidate. Each disputed, then a proof tried with the committee's signature of the prices stated | none proved | `t1b_r16_what_the_executor_disputes_is_answered_by_no_proof` |
| 2 | A match in a trial round: what the round says the sender is left against what the transaction pays its key less the coin | equal (1.992812 KAS: two tips less the fee) | `t1b_design_the_executors_price_is_the_limit_of_the_order_that_was_not_renewed_last` |
| 3 | A sell at 10 and a buy at 11: the sell's UTXO the older, then the younger | matched at 10, then at 11 | same |
| 4 | 4 buys and 4 sells that cross and carry no tip, one round; then two paying orders of other users added | 16 transactions built and left alone; a paying order still matched: **F21 (Low)** | `t1b_finding_every_round_builds_every_pair_that_crosses_before_it_knows_its_gain` |

## 1. Wrong files

None found. By the code: a made object is taken only if its slot script is the output's script, and `Book::apply`
writes it only under the book's id, with the outpoint and amount of the transaction itself; `follow` holds a
fetched transaction to its id. A file is retired by the OUTPOINT its input spent, not by what the reader thought
the object was. `exec-run` then asks the node for each file's outpoint and holds it to the book's id, the file's
script and the least value: with an honest node that is enough.

A transaction's id does not cover its signature scripts, and the arguments are there. A node that lies about them
(or about input amounts) changes the candidates the reader builds, not the outputs they are held to: it reads
less, never wrongly. Read, not run.

**F20 (Medium). A transaction arranged otherwise is not read back.** `read_tx` takes "the first object" and "the
second" from the order of the inputs; the scripts take them from `who`, wherever they stand. A match with the sell
at the lower input, or a fold with the folded account first, passes the engine, writes the same outputs, and is
read with nothing made: the account, and what rests of an order, are "unread". Also, of a transaction that runs
two scripts of the library only the first call found is read (`find_map`; read, not run). These entries ask
nobody, so anyone sends them, at no cost: every folder that follows the book (`exec-run`, a user's `book-sync`)
then lacks the files of what was made, while the old files are retired. Other executors stop matching what rests;
the users' own tools cannot cancel or close what they cannot read, until the state is rebuilt by hand from the
arguments (it is all on the chain). Nothing is taken. Fix: take the objects by the script's `who`; read every
call of a transaction; at an opening try the known fixed part as well as the payload.

## 2. The executor's money

`Round::send` builds with one coin, signs, and counts what the signed transaction pays the sender's key (outputs
under no covenant id) less that coin. That is the transaction's own arithmetic, whatever the action: tips in the
change, the library's tip, the ender's share. Checked on a match by test; the others by this reading. A
transaction that does not land costs nothing.

What it disputes cannot be proved (26 cases, test). A dispute that loses the race to a settlement or a late end
spends an outpoint that is gone: not accepted, nothing lost (read). Not gainful by design, and asked by name:
`dispute`, `settle`, `expire`, `lapse` are sent whatever they leave. A false claim on a dust account costs the
executor two fees (dispute, lapse) for half a dust bond; its maker pays about as much. Low, and `--do dispute` is
not a default.

## 3. The price

By the rule, the overlap of two limits goes to the order that came second. A user makes its own order "the one
that came" by renewing its UTXO: a fill of the least size by its own second key writes what rests anew. Test: a
sell at 10, a buy at 11, 3 units: 10 while the sell is the older, 11 once it is the younger: 3 KAS. The worst
case is the whole overlap times the size, and the limit of the other order bounds it. It is no more than the
script already gives whoever sends the match (any price between the two limits), so the rule binds honest
executors only. I would document it: a limit is what an order is sure of, not the price before it.

## 4. Stalling

**F21 (Low).** Every crossing pair is built, signed and verified before its gain is known, and a pair left alone
is not remembered: K buys and K sells without a tip are K x K builds every round (test: 16 in one round, under
100 ms with the process's start: a few milliseconds a pair). The orders cost their owner a placement each.
Paying orders are still matched in the same round. Fix: bound the gain from the model before building, and note
what was left alone.

Read, not run: `follow` asks at most 200 batches of ids and walks only outpoints that are copies of the
library's scripts or the book's anchors (one transaction further for a look-alike); each library transaction
anyone sends (a fee of about 0.005 KAS) costs every follower the blocks of its merge set. No index I read can
fail: `amounts` has one entry an input by construction, `heads` and `bodies` are filled before they are read, ids
are 64 characters. `avoid` holds only what the node or the verifier refused, for three rounds; nothing is left
unretried for good.

## Not reached

- Anything with a node: `book-sync`, `follow`, `chain_tx` against a lying node, a hand-edited or stale
  `cursor.json`, `coins` / `split`, the ten-minute move to `gone/`.
- A transaction of two library calls, built and read (the claim above is from the code).
- A novation with its two accounts in the other order: its payment is at a place counted from the first
  account's input, and my swap of the builder's layout was refused by the engine; the fold and the match show
  the same reading.
- Whether a dispute can land after the window.

## Tests and commands

133 tests pass in my three files: 49 engine (unchanged), 67 builders, 17 command line. Four are
`t1b_finding_...` and assert an acceptance: F5 and F13 as before, F20 (builders), F21 (command line).

The whole workspace, run once at the end: 121 suites, 1,025 passed, 0 failed.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2
```

# Round 17

Scope: the fixes of F20 (`thin_read.rs`: a transaction is read call by call, each call given the objects whose
`go(l, k)` names it) and F21 (`xcmds.rs`: a match whose tips are not above `--min-gain` is not built; a set built and
left alone is remembered). My two finding tests are now tests of the fix. No source, contract or artifact was changed.

**Both fixes hold in everything I ran. Nothing above Low is open in my findings, except F13 (Medium, accepted by the
owner). One test that is not mine fails after the F21 fix, so the workspace is red: see "Does not hold".**

## What was tried

| Tried | Result | Test |
|---|---|---|
| Fold and match with the two objects the other way round (`who` = 1,0,0), read with everything known and with nothing known | accepted by the engine; read whole, every spent object at its own input; with nothing known nothing is made up | `t1b_fixed_f20_a_transaction_is_read_back_wherever_its_objects_stand` |
| The same swapped objects with `who` left at 0,1,1 (pointing elsewhere than the slots say) | refused by the engine, so the reader never meets it | same |
| Swapped match: both orders used up; the sell rests | read, the resting order included | same |
| An opening that carries another account's payload, the fixed part known | the account is read from the fixed part; with nothing known no account is read, never the payload's | same |
| A single-object entry (add) with its script at input 0 and its object at input 1 | accepted; read | `t1b_r17_two_calls_in_one_transaction_and_an_object_at_another_input_are_read` |
| Two library scripts in one transaction: an add on one account and a declare on another, in four layouts (add first; declare first; both scripts then both objects; each object before the other's script) | all accepted; the added account and the claim are both read, nothing unread, nothing attributed to the other call; with nothing known two outputs stay unread and nothing is made up | same |
| `slot_call` reads `l` as the fourth push from the end | on every transaction my read tests read (fold, match, the anchor of an opening, add, declare) each spent object's signature script is exactly 4 pushes and its `l` names an input that is a library call | asserted inside `read_signed`, used by both tests above |
| 4 buys and 4 sells that cross with tip 0, `--min-gain` at its default | "0 sent", no "left alone", nothing built | `t1b_fixed_f21_a_round_builds_no_match_whose_tips_are_not_above_the_gain_asked` |
| Two paying orders of other users added to that folder | matched in the same round ("1 sent") | same |
| 2 x 2 orders with tips of 0.0001 (above nothing, below the fee) | built once each: exactly 4 "left alone", "0 sent"; with `--min-gain 0.001` none is built | same |
| A pair without tips whose first buy holds 5 KAS more than its margin and tip | passed over like the others | same |
| Two accounts of the same two users, sides the other way round (a novation and a fold both apply to the same two outpoints), with a gain asked that neither reaches | the fold asked for alone is built and left alone; with the novation tried first, the novation is left alone and the fold is not built | `t1b_r17_what_was_left_alone_is_remembered_by_its_objects_and_not_by_what_was_tried` |
| The same pair with a gain asked between the two gains | the novation is sent; the fold alone is left alone | same |

## Does not hold

- **The workspace is red: `an_executor_runs_rounds_over_its_folder` fails** at
  `crates/kpf-cli/tests/committee_cli.rs:1182`. It runs `--min-gain 5` and expects "left alone" and "0 sent"; after the
  F21 fix that pair is no longer built, so the output is "... 0 sent" with no "left alone" line. The test is not one
  of my files and I did not touch it. It is the test that is stale, not the fix: the output is what the fix intends.
- **Low, remark on the F21 pre-check (not numbered).** The pre-check is `m.tips > min_gain`. A match in which an order
  is used up leaves its sender that order's excess as well (shown at the engine in round 11 and followed in round 15).
  A pair with no tips and an excess is therefore passed over although sending it would have paid. I ran only the
  passing-over (last row of the table); the payment itself is from the earlier rounds, not rerun on this pair. Nobody
  loses by it: the order rests and its owner keeps the excess. An order's owner cannot be made to lose a match that
  pays by tips this way, since tips above the gain always pass the pre-check.

## What `alone` remembers

`alone` is a set of outpoint lists, consulted in `send` before any build and kept in `turn` while every outpoint is
still listed. Its key is the outpoints in their order, not what was tried on them.

- **Run.** A novation and a fold of the same two accounts are sent on the same two outpoints in the same order, and a
  round tries the novation first. Once the novation is left alone, the fold is not built in that round (and, by the
  code, in no later round of the run while both accounts stand). In the pair I ran this costs nothing: the fold would
  leave the sender 0.003326 KAS and the novation 0.007132 KAS at the default fee rate, so a gain the novation does not
  reach is one the fold does not reach either. I did not find a pair where the fold pays more than the novation; I
  tried one pair, at one fee rate.
- **Read, not run.** The other way round needs several rounds: a fold left alone in a round in which no copy of
  KpfTNovate was free would keep the novation on the same ordered pair from being tried later. The novation was built
  in both orders of that pair in my run, and the other order is another key, so I expect it to be tried there; I could
  not run it.
- **Read, not run.** The entry is written only after the executor itself built the transaction and found its own gain
  too small. I found no way for someone else to write an entry or to keep one alive: an object changes only by being spent,
  which makes a new outpoint, and the entry falls when one of its outpoints is no longer listed; the fee rate and
  `--min-gain` are fixed for a run. A single-object entry (a late claim, a late liquidation, a
  no-price claim of a time to come) is on an outpoint for which no other action applies: which branch an outpoint
  takes is fixed by its state and the DAA score of its UTXO.
- `send` consults `alone` for every action, also for those sent whatever they cost (dispute, settle, lapse, expiry).
  By the point above no outpoint list is shared between a gainful action and one of those, so I see no case where a
  dispute or a settlement is held back by it. Read, not run.

## Not reached

- `alone` over more than one round: offline `exec-run` (with `--funding` and `--daa`) runs one round, and several
  rounds need a node.
- Anything with a node or the network (`book-sync`, a live `exec-run`).
- `slot_call` on the operations my read tests do not read (the claim, dispute, liquidation, quiet, merge, novate,
  join, turn and leave paths). The slot has one entry and I saw four pushes wherever I looked, but I did not look
  there.
- A novation with its two accounts the other way round is refused by the engine (seen in round 16, cause not looked
  into), so the reader was not tried on it.

## Findings ledger

| | Severity | State |
|---|---|---|
| F20 | Medium | fixed in round 17, verified by test |
| F21 | Low | fixed in round 17, verified by test; one Low remark on the pre-check above |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | open on purpose |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F19) | | fixed in earlier rounds |

## Counts and commands

135 tests pass in my three files: 49 engine (unchanged), 68 builders, 18 command line. Two are `t1b_finding_...` and
assert an acceptance: F5 and F13.

The whole workspace, run once with `--no-fail-fast`: 121 suites, 1,025 passed, 1 failed (the test named above),
0 ignored. Without `--no-fail-fast` cargo stops at that suite (22 suites, 76 passed, 1 failed). That run was made
before I added the last test above; the command-line suite was then run again by itself (18 passed), and the
workspace was not run a second time.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 18

Scope: the two Low remarks of round 17, taken up in `crates/kpf-cli/src/xcmds.rs`: `alone` is keyed by the script and
the outpoints; the match pre-check is `m.tips + excess(long) + excess(short) > min_gain`. My two tests that stated the
old behaviour now state the new one. No source, contract or artifact was changed by me.

**Both changes hold in what I ran. Nothing above Low is open in my findings, except F13 (Medium, accepted by the
owner).**

| Tried | Result | Test |
|---|---|---|
| A novation and a fold of the same two accounts, a gain asked that neither reaches | the novation is left alone in both orders and the fold is built and left alone too (3 "left alone", the same fold line as when it is asked for alone) | `t1b_fixed_a_novation_left_alone_does_not_hide_the_fold_of_the_same_two_accounts` (was `t1b_r17_what_was_left_alone_is_remembered_by_its_objects_and_not_by_what_was_tried`) |
| A gain asked between the two | the novation is sent, and the two accounts are then not free for the fold; the fold asked for alone is left alone | same |
| A pair without tips, the buy used up and holding 5 KAS more than its margin and tip | matched; the sender is left 4.992828 KAS, which is the 5 KAS less the fee of 0.007172 KAS, and is what the transaction pays its key | `t1b_fixed_f21_a_round_builds_no_match_whose_tips_are_not_above_the_gain_asked` |
| The same excess in an order that is not used up (a buy of 3 holding 5 KAS more, a sell of 2), by `order-match` | what rests of the buy holds 9 KAS (a third of its margin and the whole 5 KAS); the sender is paid nothing (-0.007346 KAS, the fee) | same |
| That pair in a round | built once and left alone; the gain the round states is that of the `order-match` transaction; without the excess the pair is not built | same |

**The bound.** For an order that is used up, `amount - order.value()` is exactly what the sender receives of it
(`Order::filled`: `left = value - margin - tip`, and the run above: 5 KAS in, 5 KAS less the fee out). For an order
that is not used up, none of the excess goes to the sender: it stays in what rests. That is the model (`Rest::Order(..,
left)`), the script (`KpfTMatch.sil` lines 150 and 167/173: the resting output holds at least the input less the
shares of margin and tip; read this round), my engine test of round 11
`t1b_o1_a_match_pays_what_the_model_says_wherever_its_inputs_stand` (over its matches: what the two orders held less
what the script asks written is the tips plus what a used-up order held beyond its margin and tip) and the run above. So the bound is equal to
what the match pays when both orders are used up and above it when an order with an excess rests; I found no case
where it is below. An order holding less than its margin and tip makes `match_orders` fail before the bound is
looked at. Since the fee is above zero and the test is `> min_gain`, a match that would leave the sender at least
`--min-gain` always passes the pre-check.

One consequence, Low and by the rule the coordinator stated ("it may be above, then the build decides"): a resting
order that holds an excess and carries no tip is built once a run against each order it crosses and left alone, where
before it was passed over without a build. The entry in `alone` keeps it to once per pair and per run.

**Not reached.** The fold left alone first and the novation tried in a later round (several rounds need a node; the
key now holds the script, read in `send`). Anything with a node.

**Counts.** 135 tests pass in my three files: 49 engine (unchanged), 68 builders (unchanged), 18 command line. Two are
`t1b_finding_...`: F5 (Low, on purpose) and F13 (Medium, accepted).

The whole workspace, run once with `--no-fail-fast` after my last change: 121 suites, 1,027 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 19

Scope: the dues. Every script of the library changed (`goesOn` asks `DUES` more; a new entry `release(l, k)` in 25
scripts), three new ones (`KpfPurse`, `KpfTSweep`, `KpfTPay`), and their model, builders, reader and commands. My
three test files were brought to the new scripts and extended. No source, contract or artifact was changed by me;
no node, no network, nothing committed.

**One finding above Low: F22 (Medium), open. Two Low findings (F23, F24), open. Questions 1 to 3 (a copy or the
purse spent under no rule, value leaving by another path, anything written under the library's id) hold in everything
I ran. F13 (Medium) stays open, accepted by the owner.**

## The 41 tests that failed after the change

None failed for a reason that is a finding.

- Engine file, 36: 34 were hand-made transactions whose copy output held what its input held; each now holds
  `DUES` more (`LIBV`). `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` and
  `t1b_b1_every_entry_counts_the_inputs_and_outputs_of_the_objects_id` stated a property of every entry and were
  extended, not exempted: see "What holds".
- Builders file, 4: the half of a bond that stays in the script is now with the dues; and whoever ends a late claim
  is left its half less the dues of the script it used (two assertions).
- Command line, 1: 27 scripts.

## Findings

### F22 (Medium, open). Two libraries' payments are answered by one set of seat outputs

`KpfTPay.pay` holds each seat's output to the seat's key and to at least its share. Nothing makes the output that
payment's alone, and the count that closes a second payment of the same library (`OpCovInputCount(lib) == 2`) says
nothing of another library. Two libraries whose purses are for one roster are paid in one transaction with one set
of seat outputs: both payments pass, the seats receive the larger share once, and the sender keeps the smaller
purse whole.

Test: `t1b_finding_f22_two_libraries_payments_are_answered_by_one_set_of_seat_outputs` (builders, engine-accepted).
Library A holds 20 KAS above its floor, library B 12 KAS, same roster and ticket. Apart, the seats receive 30.37 KAS
and the two senders are left 1.005 and 0.595 KAS. Together the seats receive 18.98 KAS and the sender is left
13.0006 KAS; both purses stand at their floors.

- Who can do it: anyone, whenever two libraries with dues name one roster. That is the expected state as soon as a
  library is replaced (the old one earns until its accounts have moved) or a committee serves two libraries.
- What is lost: the committee's income from the smaller purse, each time. No user's margin.
- What it is not: a way to take a purse by making a second library and funding its purse oneself. The seats are
  always paid at least the larger purse's shares, so a purse the attacker filled returns the attacker its own money
  and its twentieth (arithmetic, from the script's two conditions; not run as a test).
- The same cause, read and not run: a seat's output is any output to its key. A payment that another script in the
  same transaction owes the same key (an operation of another library that pays a party who is also a seat; another
  family's script) can be the seat's share as well, and the seat is paid the larger once.
- Direction (yours to choose): make the transaction hold nothing else that pays (the purse, the script, the ticket,
  and inputs under no covenant id; or a fixed number of inputs), or pay each seat to a script that names the library
  (an output of one library is then never another's).

### F23 (Low, open). A payment's sender keeps up to a unit a seat beyond its twentieth

Shares are counted down to units of 0.01 KAS and what is not paid is the sender's, so the sender keeps a twentieth
and up to 0.01 KAS a seat. On a purse paid as soon as it can be, that is a large part.

Test: `t1b_finding_f23_the_sender_of_a_payment_keeps_up_to_a_unit_a_seat_beyond_its_twentieth` (model over every pot
from 0.05 to 12 KAS; the worst through the builder and the engine). Three equal seats, 0.25 KAS above the floor: the
seats are paid 0.21 KAS and the sender keeps 0.04 (16 %; 0.0254 KAS after the fee). Fifteen seats are first paid at
5.37 KAS above the floor (below that the builder refuses: the storage mass of fifteen small outputs), where a unit a
seat is 2.8 % more. Bounded by 0.01 KAS a seat a payment; whoever pays chooses when.

Direction: let the purse keep what is not paid (`outputs[j] >= inputs[j] - paid - pot / CUT`), so that rounding is
the committee's next time and not the sender's now.

Also seen there: a seat whose weight is under 0.01 KAS is in the sum and is never paid. No roster has such a seat
(its `minSeat`); a ticket made by hand could.

### F24 (Low, open). A copy's dues are swept in pieces, each piece paying its sender

A sweep moves at least 0.2 KAS and may leave the copy above its floor. A copy that holds n times the least is
swept n times, a block each: every sweep takes the copy (and the purse) for a block and leaves its sender a
twentieth of 0.2 KAS, 0.01 KAS, against a fee the design gives as 0.0043 to 0.0069 KAS. The design's limit 6 ("at
the price of 0.2 KAS given to the purse each time") counts money that is the committee's already; it costs the
sweeper nothing.

Test: `t1b_finding_f24_a_copys_dues_are_swept_in_pieces_of_the_least` (engine): a copy holding three times the
least is swept three times, each accepted (a sompi less to the purse refused), each leaving its sender 0.01 KAS. The gain after the fee
is arithmetic from the design's fee figures, not measured here. Bounded by what gathered: an executor that sweeps a
copy whole as soon as it pays (what `exec-run` does) leaves nothing to cut up.

Direction, if wanted: the copy goes on holding exactly its floor (`==`, or at most the floor and less than the
least above it).

## What holds

| Question | Tried | Result | Test |
|---|---|---|---|
| 1 | Over the sources: each of the three places of `who`, in each of the 32 operation entries of the 27 scripts, is read as an object (held to the hash of a state), as the purse, as a ticket (the pinned template), as the copy a sweep answers for, or is held equal to a place that is read | holds for all 32; printed as a table | `t1b_r19_every_place_of_every_entry_is_read_or_held_to_one_that_is` (engine file) |
| 1 | Over the sources: `release` is the same six lines in each of the 25 scripts, reads no output, no value and no count; the two that collect have none; every other entry ends by `goesOn`, whose four conditions now ask `DUES` more (the two that collect: no less than it held) | holds | `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on`, `t1b_b1_every_entry_counts_the_inputs_and_outputs_of_the_objects_id` (57 rows) |
| 1 | At the engine, the 22 scenes (every entry that reads one object): a second copy of the script itself or a copy of another script let go beside it, and the purse, named at each place and at none (10 patterns of `who`), the copy going on with less, or an object or a key written under the library's id in its place; the purse in the object's place under either id; an object whose state is a purse's | 1,752 transactions, all refused | `t1b_r19_no_operation_passes_with_a_copy_or_the_purse_at_a_place` |
| 1 | The same for match, merge and join (a copy or the purse in the first or the second object's place) and for the three proofs with one and two legs (a copy at each of the three places) | 12 and 36 transactions, each refused by the script's own input | same; `t1b_r19_no_proof_passes_with_a_copy_let_go_at_a_place` (builders) |
| 1 | Who may name a copy: an input under no id or another id; two copies, or a copy and the purse, naming each other; the same with `l` pushed as 24 bytes that name the other; the same with 24 bytes pushed before the arguments; an object named by a copy so crafted; a real entry with a push before its arguments | all refused: a number is at most 8 bytes, and a script must leave a clean stack | `t1b_r19_only_an_entry_that_begins_with_the_places_lets_a_copy_go` |
| 2, 3 | The sweep: each of the 22 scripts this file compiles swept at the least and refused a sompi under it; 68 other arrangements (the purse short, the sender keeping what moved, the copy under its floor, another script, an object or a key in the copy's place, the copy or the purse under no id or at another index, another purse in the purse's place, one output more under the id by any input, 14 patterns of `who`, another roster, five floors, every other `release` and `go`, a second copy or purse beside) | all refused | `t1b_r19_a_sweep_moves_a_copys_dues_to_the_purse_and_nothing_else` |
| 2 | A sweep beside an operation that is valid alone (an add) | the sweep refuses, the add's script passes | `t1b_r19_a_sweep_is_alone_with_the_purse_and_its_copy` |
| 3 | The genesis: 14 geneses with the id recomputed (a purse for another roster or floor than the payload's, a purse whose state is an anchor's or an account's, two purses, a slot beside the purse, payloads a byte off, another magic, floors below 1 KAS, of nothing and below nothing) | all refused; a payload that names a purse the genesis did not write is a library without one | `t1b_r19_a_genesis_is_a_library_with_at_most_the_purse_its_payload_names` |
| 4 | The payment: 34 arrangements (the ticket told to be another input, the seats told to begin at the purse's, the script's or the ticket's output or past the end, 7 patterns of `who`, a seat a sompi short, two shares in one output, the purse under its floor or going on as another purse or a script, the change under the library's id, a ticket spent by `retire`, two seats of one key paid by one output, a sompi under what a payment of the most leaves) | all refused; two seats of one key are two outputs; a payment shares at most 300,000 KAS and leaves the rest | `t1b_r19_a_payment_is_held_to_its_ticket_its_places_and_its_shares` |
| 4 | Shares over every pot from 0.05 to 12 KAS, 3 and 15 seats | the seats are never paid more than the pot less a twentieth; the sender never keeps more than a twentieth, a unit a seat and one more | inside the F23 test |
| 6 | The reader: a sweep as built, with the copy left above its floor, with the purse given more, with a key's output more, with the copy first and the purse last; a payment | each read as a transaction of the library that moves nothing of a book (the payment's one unread output is the ticket, under the roster's id) | `t1b_r19_the_reader_takes_a_sweep_and_a_payment_as_moving_nothing_of_a_book` |
| 6 | The command line: 7 genesis files whose purse is not the one the payload names; an opening and two added margins in a library with a purse; an executor's round with nothing gathered; `dues-sweep` and `dues-pay` without a node | refused by `lib-check`; 0.002 KAS left in the copy each time; nothing sent; "read at the node" | `t1b_r19_a_library_with_a_purse_at_the_command_line` |

## By design, measured

- A payment shares by the table of whatever ticket of the purse's roster it is shown. A ticket whose window ended
  long ago pays the seats of that time and the present seats nothing; a ticket of one seat under the id pays that
  seat everything. `t1b_design_a_payment_shares_by_the_table_of_any_ticket_under_the_rosters_id`. The first is the
  design's limit 2, but "an epoch behind" is a floor, not a bound: a ticket stands until somebody retires it
  (`KpfTicket.retire`, anyone, a window after its end; read). The second is the trust every proof already places in
  a market's roster id, with one difference: the purse's roster is the library maker's choice and nothing ties it to
  the rosters of the markets traded in the library. `lib-check` prints the id; `acct-open` says nothing of it.
- `lib_check` does not look at values: a purse or a copy holding less than the floor is taken. It is the maker's own
  money that is missing.
- A second purse under the library's id is swept into the first as a copy is (run, in the sweep test: the sweep
  does not ask what its third input is, only that it goes on as it is at its floor). `lib_check` accepts one purse, which is what closes it.

## Read, not run

- `exec-run`'s sweep tries one copy a round, the one that holds the most. If that one does not reach the gain asked
  (a long script's fee is higher) while a smaller copy of a shorter script would, nothing is swept, and the entry in
  `alone` keeps it so until the purse or that copy moves.
- A sweep the executor itself sent moves two copies (the sweep's and the one swept) and the round tracks one
  (`Sent.old`/`new`), so the next round sees a copy "moved by another hand" and reads the chain: one reading per own
  sweep. And every sweep or payment by anyone moves copies that are in `stand`, so every executor reads the chain on
  each. Nothing breaks (a sweep or a payment makes no file: `made` is empty), but it is node load that grows with
  other people's sweeps.
- The reader names a transaction by the first script it meets: a sweep whose copy stands before the sweep's script is
  printed as `<copy>.release` (run, in the reader test). It is only printed.
- A payment beside an operation of the same library is closed by `OpCovInputCount(lib) == 2` (read; the sweep's
  count of 3 is run).
- `du * w` stays below 2^63 for a seat of up to about 3.2 billion KAS at the most a payment shares. A ticket admits
  a total of 40 billion; no such stake exists. Above it the script would fail and the purse could not be paid.

## Not reached

- Anything with a node: `dues-sweep`, `dues-pay`, `lib-status`, the sweep of `exec-run` (offline its copies are the
  genesis', with nothing gathered), `book-sync` and `follow` on other people's sweeps and payments.
- A copy or the purse at a place of fold, novate and leave at the engine (the sources say each place is an object
  held to a state's hash; match, merge, join, turn and the proofs were run).
- Whether a payment of small shares is mined: the builder's storage-mass meter is all I have.
- `KpfTicket` and `KpfRoster` themselves (the family the payment trusts), beyond reading `attest` and `retire`.
- The fee of a sweep in pieces (F24's gain is from the design's figures).

## Findings ledger

| | Severity | State |
|---|---|---|
| F22 | Medium | open: two libraries' payments share one set of seat outputs |
| F23 | Low | open: the rounding of the shares is the sender's |
| F24 | Low | open: a copy's dues swept in pieces |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | open on purpose |
| F7, F8, F9 | Low | by design |
| F20, F21 | | fixed in round 17 (and the two remarks of round 17 in round 18) |
| all others (F1-F4, F6, F10-F12, F14-F19) | | fixed in earlier rounds |

## Counts and commands

149 tests pass in my three files: 55 engine (49 and 6 new), 75 builders (68 and 7 new), 19 command line (18 and 1
new). Five are `t1b_finding_...` and assert an acceptance: F5, F13, F22, F23, F24.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,048 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 20

Scope: the fixes of F22, F23 and F24 in `KpfTPay.sil` and `KpfTSweep.sil`, and the three read-not-run items of round
19 taken up in the reader and the executor. My three finding tests are now tests of the fix, the tests that used
`pay`'s old arguments are brought to the new ones, and the fixes were attacked. No source, contract or artifact was
changed by me; no node, no network, nothing committed.

**The three fixes hold in everything I ran, and I found no way around them. No new finding. Nothing above Low is
open in my findings except F13 (Medium, accepted by the owner).**

## The fixes

| | What was tried | Result | Test |
|---|---|---|---|
| F22 | The attack of round 19 as built: library A's payment exactly as its builder lays it out, with library B's purse and script as two inputs more and no seat output more | refused by both payments' scripts. A's, whose every other condition holds, refuses for the two inputs (so the test fails if the four-input rule goes); B's has no place to stand at (its purse is not input 0). Each alone is taken | `t1b_fixed_f22_a_payment_is_alone_in_its_transaction` |
| F22 | The four inputs and the seats' places: the payer told another key than the coin's; the fourth input an output of a script; a second coin (five inputs); no coin (three); the purse and the script in each other's places; the ticket and the coin in each other's places; an output before the seats'; the first seat's output left out | 8 arrangements, each refused by the payment's own script | `t1b_r19_a_payment_is_held_to_its_ticket_its_places_and_its_shares` |
| F22 | A seat that pays (its key is `payer` and the change's): its share and its change as two outputs, and as one | both taken; the other seats are paid their shares; the paying seat has the same either way but for the smaller fee | same |
| F22 | Two seats of one key paid by one output; two shares in one output; a ticket spent by `retire`; an output more after the seats' | refused, refused, refused; taken | same |
| F22 | What one coin asks of a payer: two coins or none at the builder; a coin of 0.01 KAS | the builder refuses; the small coin pays (the fee comes out of the twentieth) | same |
| F23 | Model, every pot from 0.05 to 12 KAS, 3 and 15 seats | what leaves the purse is the shares and a twentieth of the pot exactly; the purse keeps the rounding (at most 0.1495 KAS over the pots tried) | `t1b_fixed_f23_what_the_rounding_leaves_stays_in_the_purse` |
| F23 | The payment of round 19 (three equal seats, 0.25 KAS above the floor, where the sender kept 16 %) | the seats are paid 0.21 KAS, the purse keeps the 0.0275 KAS the shares lost, the sender has its twentieth less the fee. The rounding moved to the sender's change: refused. A sompi less in the purse: refused. What stayed is in the next payment | same |
| F23 | The most a payment shares (`POT_MAX`) | the purse keeps what is above it and the rounding; a sompi less refused | the payment test above |
| F24 | The three pieces of round 19 (a copy holding three times the least) | the first two refused, also with the purse given everything; the last leaves the copy at its floor and is a whole sweep. The copy left a sompi above, a sompi under, or 0.01 KAS above its floor: refused. The whole: taken | `t1b_fixed_f24_a_copy_is_swept_whole_or_not_at_all` |
| F24 | A copy that holds less than its floor, its floor exactly, or a sompi under the floor and the least; left as it is or at the floor | not swept; a copy holding the floor and the least is | same |
| reader | A sweep with its copy at input 0 and the purse at input 2 | taken by the engine; read as `KpfTSweep.sweep` (round 19: `<copy>.release`) | `t1b_r19_the_reader_takes_a_sweep_and_a_payment_as_moving_nothing_of_a_book` |

Why the purse cannot be left under its floor (read, and run at the two ends): the shares are counted down from the pot
less a twentieth, so `paid + pot / CUT` is never more than the pot, and the purse's output is held to its input less
that; with `POT_MAX` the pot is the cap and the rest is not touched.

## By design, measured

- **The coin of a payment may carry a covenant id.** The payment asks that input 3 pays to the key it is told, and
  nothing of whether that coin is under an id. A key's coin under an id of the payer's own making passes, and the
  seats' outputs can then be written under that id (the coin authorizes them): taken by the engine. Each is still an
  output to the seat's key of at least its share, and nothing in the transaction asks anything more of it, so nothing
  is taken; a wallet that sets aside outputs under an id it does not know would not show a seat its payment.
  `t1b_design_the_coin_of_a_payment_may_carry_an_id_and_the_seats_outputs_with_it`. If you want it closed: hold the
  seats' outputs, or the coin, to no id.
- **A payment of a small pot costs its sender.** With the rounding in the purse the sender's only pay is the
  twentieth: the payment of 0.25 KAS to three seats leaves it 0.0024 KAS poorer after a fee of 0.0149 KAS. Three seats
  pay their sender from about 0.30 KAS. Nothing to fix: it is when a payment is sent.
- **A payer needs one coin of a key**, of any size (0.01 KAS run). Funds held under a script do not pay a purse
  without a coin made first.
- **A copy under its floor** (a genesis that gave a copy less: `lib_check` does not look at values) is not swept until
  it holds the floor and the least: what gathers up to the floor never reaches the purse.

## Read, not run

- `exec-run`'s sweep goes through the copies from the fullest down and stops at the first it sends or when the
  round's transactions are used up (`for copy in &full { if r.send(..) || r.left == 0 { break } }`).
- `Round::send` records the second copy a transaction of its own moved (`Made::Script` at another output than the
  script's own), so the copy a sweep emptied is expected where it was written.

## Not reached

- Anything with a node: `dues-pay` taking the funder's largest coin, `dues-sweep`, `lib-status`, the sweep of
  `exec-run` and its two changes above, `book-sync` on other people's sweeps and payments.
- Whether a payment of small shares is mined (the builder's storage-mass meter only).
- A copy or the purse at a place of fold, novate and leave at the engine (as in round 19: sources only).

## Findings ledger

| | Severity | State |
|---|---|---|
| F22 | Medium | fixed in round 20, verified by test |
| F23 | Low | fixed in round 20, verified by test |
| F24 | Low | fixed in round 20, verified by test |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | open on purpose |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F21) | | fixed in earlier rounds |

## Counts and commands

150 tests pass in my three files: 55 engine, 76 builders (one new: the design test of the coin under an id), 19
command line. Two are `t1b_finding_...` and assert an acceptance: F5 and F13.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,049 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 21

Scope: two additions to the tool, with no script changed since round 20: the guard of `exec-run` (`--do guard
--ticks DIR --ticket FILE`: `struct Guard`, the three arms in the claims loop) and the measure of what stands under a
roster (`--max-exposure`). Reviewed by reading `crates/kpf-cli/src/xcmds.rs` and by offline rounds at the command
line. Only `crates/kpf-cli/tests/auditT1_cli.rs` and this file were changed by me; no node, no network, nothing
committed.

**Five findings, all in the tool, all open: three High (F25, F26, F27), one Medium (F28), one Low (F29). No script is
involved and no money can be taken through any of them by somebody who is not already the user's counterparty; what
they cost is what the guard is for: the user's gain is not claimed, or its claim's bond is lost. F13 (Medium) stays
open, accepted by the owner.**

The adversary is the one the coordinator named: seats holding more than a third of a roster, the account's
counterparty, possibly the same people, who put into the user's ticks' folder what seats signed.

## Findings

### F25 (High, open). Sixty ticks no quorum signed hide the last one a quorum did

`Guard::last` looks for a quorum among the 60 newest ticks not after the present (`take(60)`), and a tick is a
file's name. Sixty ticks newer than the last one a quorum signed, each signed by one seat, push that tick out of
what is looked at: the guard says no quorum signed anything and claims nothing.

Test: `t1b_finding_f25_sixty_ticks_no_quorum_signed_hide_the_last_one_a_quorum_did`. The last quorum's tick is 70 DAA
old, the account is worth more than its margin there, and a claim would still land: claimed. With 59 newer ticks of
the smallest seat alone: still claimed. With the sixtieth: "no tick a quorum signed for its market is in the ticks'
folder", 0 sent. The same with sixty files that are no ticks at all (`{}` under a tick's name).

- Cost to the adversary: sixty signatures of one seat, each of its own tick and one price: no offence. A seat that
  goes on signing honestly after a third has stopped does it too, if it signs more than 60 ticks before the guard's
  silence is reached (a tick every DAA with the default silence of a third of `lateDaa` above 60).
- Direction: look until a quorum is found or the ticks are older than `lateDaa` (older ones cannot be claimed at
  anyway), not for a count of names; or count only ticks a seat of the ticket signed.

### F26 (High, open). The guard does not prove its own claim once another signed price of the tick arrives

The proof arm proves a disputed claim only if the price it collects now for the claim's tick equals the claim's
price (`g.at(..).filter(.. a.1.p == p)`). `Attestation::collect` returns the middle of the candidate prices the most
weight is near, so it moves with the files in the folder.

Test: `t1b_finding_f26_the_guard_does_not_prove_its_claim_once_another_signed_price_of_the_tick_arrives`. Seats 0
and 1 (a quorum) signed 11; the guard claims at 11; the counterparty disputes; with those two files the guard proves
(1 sent). The third seat's price for the same tick arrives, 11.008, inside the roster's spread of 0.1 %: the guard
collects 11.008, prints "it cannot be proved from here, and lapses", 0 sent. The three signatures prove 11 on the
chain (each is within the spread of it).

- What it costs the user: the claim, and its bond (a lapse gives half to the disputer and half to the party the
  claim was against: the counterparty has both).
- Cost to the adversary: nothing. A seat's one price for a tick, delivered late, is no offence; an honest seat whose
  source says 11.008 does it by accident. A dispute of a true claim is safe for whoever knows the late price is coming.
- The same line refuses a claim of the key declared by hand at any other price a quorum signed (question 2).
- Direction: prove at the claim's own price. Take the seats whose signed price is within the spread of it, check
  their weight is a quorum, and attest that (`Attestation::of`, or `collect` then `claiming`), instead of asking
  `collect` for its own middle and comparing.

### F27 (High, open). A seat's second signed price for a tick displaces its first

`Attestation::collect` takes the first signed price it is given of each seat, and the guard gives it the tick's
files in the folder's order. A seat that signed the last tick with the quorum and then signs another price for it
displaces its first signature, when the second is read first or lands on the same file name. With seats no quorum is
without, the last tick has no quorum in the guard's eyes.

Test: `t1b_finding_f27_a_seats_second_signed_price_displaces_its_first`. Every seat signed 11 at the last tick: the
account is claimed at it, and a disputed claim made at it is proved. Seat 0 (30 of 67 KAS) then signs 20 for that
tick. Written over its first file, or under a name read before it: the account is not claimed ("it cannot be claimed
at that price any more": the guard falls back to the tick before, which is too old), and the disputed claim is not
proved ("lapses").

- Cost to the adversary: two prices for one tick forfeit the seat's stake if somebody sends both to the roster. The
  guard has both in its folder in the second case and does nothing with them; in the first case the user's copy of
  the first is gone. Rated High because the guard itself turns the second signature into a loss; it would be Medium
  if the guard kept and used both.
- `tick-sign` refuses a second price from the key's own record and does not write over a file that holds one: an
  honest seat's tool does not do this by accident. The test removes the record, as a seat that means to would.
- Direction: per seat, take a signed price that is within the spread of the price wanted (any of them), not the
  first; never let a file of a seat replace another of the same tick; and when a seat has two prices for one tick,
  say so (it is the proof the roster's forfeit takes).

### F28 (Medium, open). The guard is blind from the roll after its tickets

The guard's tickets are read once, when the loop starts (`Guard::open`, called where `Cfg` is made), and a tick is
read against a ticket whose window holds it. After the roster rolls, every new tick is of an epoch the loop has no
ticket of.

Test: `t1b_finding_f28_the_guard_is_blind_from_the_roll_after_its_tickets`. With the ticket of the epoch
[33,000, 36,000): a tick of 35,900 is claimed at; ticks of 36,100 signed by every seat are "no tick a quorum signed"
and nothing is claimed. That the tickets are read once is from the code (several rounds need a node).

- The message names the wrong cause (the ticks are there; the ticket is not).
- Direction: read `--tickets DIR` again each round, or find the epoch's tickets at the node as the proof arm does;
  and say "no ticket of this tick's epoch was given" when that is the reason.

### F29 (Low, open). The limit under a roster is passed by the matches of one round

`above` compares what stood under the roster when the round began, plus the one pair, with the limit: the sum is not
raised by a match the round has just sent.

Test: `t1b_finding_f29_the_limit_under_a_roster_is_passed_by_the_matches_of_one_round`. Half of a stake of 67 KAS is
33.5 KAS; of two pairs of 24 KAS the first is matched and the second passes the limit's check in the same round
(48 KAS). Offline a round holds one copy of each script, so the second then stops at "no copy of KpfTMatch is free";
with a node and a library of two copies nothing stops it (read). With a limit under one pair, both are left unmatched.

## What holds (run)

| Tried | Result | Test |
|---|---|---|
| In a seat's place at the tick: a file that is no JSON; `{}`; another seat's tick; a tick of a key that is no seat; the seat's tick of another time under this tick's name; the seat's signature of 10.2 with 11 written in the file | none adds to a quorum, none stops the round; the seat's own signature of 11 then makes the quorum and the claim | `t1b_r21_the_guard_counts_only_what_seats_signed_for_this_market_and_this_tick` |
| A claim of the key past its window with `--do guard,settle` | one transaction, no second attempt on the same copy | same |
| `--do all` without ticks; with ticks; the default round | runs without the guard; guards; as it was | same |
| The guard's own cases (claimed after the silence, too late to land, against the user, no quorum, proved, settled) | as the design says | the coordinator's two tests pass; my five tests each begin from the same working case |

Everything the guard acts on goes through `Attestation::collect`, and `collect` verifies each signature against the
roster, the market and the tick (`tick_verifies`; the forged price in the last row of the first line is the run of
it). A claim the guard declares is the rule's at a price `collect` returned for a ticket whose window holds the tick.

## Read, not run

- **The price claimed is `collect`'s middle**, not the best for the user among the prices the same signatures attest
  (within the spread, 0.1 % on testnet); and the newest tick, not the best still claimable (the design says the
  second).
- **`--silence` above four fifths of `lateDaa`** never claims: the guard only says the price can no longer be
  claimed. Nothing refuses such a value.
- **The declare races anybody's operation on the account** (a turn by anyone once it is 100 DAA old, a merge, fold or
  novation with another account of the same users, this loop's own `novate` and `merge`, which come before the
  claims in a round). The one that lands first voids the other; the guard meets the account again a round later.
- **A key that is both parties** is guarded as its A only (nobody else's money is in that account).
- **The measure**: an account of two legs under two rosters counts whole under each; `above` adds the two orders'
  whole margins where a match of a part adds a part (it errs to leaving unmatched) and is asked, and prints its
  line, for pairs that would not be matched for other reasons; every live account of the folder counts, anybody's,
  so margin parked under a roster by anyone stops this executor's matches of that roster (it is the measure working
  as described); the stake is the newest ticket given, which a later change of the table does not reach (F28's
  cause).
- **A tick with very many files** is read whole, for every account of the key, every round.

## Not reached

- Anything with a node: a live ticket found at the node for a proof, several rounds (tickets read once, `alone`,
  `avoid` after a refused declare), two matches really sent in one round, the guard on an account that moves between
  two rounds.
- A cross account under two rosters (the code takes one tick both legs' quorums signed, each with its own ticket;
  not run).
- The transport that brings ticks to the folder (not in the repository): whether it can write over a file is what
  decides the first form of F27.

## Findings ledger

| | Severity | State |
|---|---|---|
| F25 | High | open: sixty ticks without a quorum hide the last one with |
| F26 | High | open: the guard's claim is not proved once another signed price of the tick arrives |
| F27 | High | open: a seat's second signed price displaces its first |
| F28 | Medium | open: the guard's tickets are those of its start |
| F29 | Low | open: the limit is checked against the round's opening sum |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | open on purpose |
| F7, F8, F9 | Low | by design |
| F22, F23, F24 | | fixed in round 20 |
| all others (F1-F4, F6, F10-F12, F14-F21) | | fixed in earlier rounds |

## Counts and commands

156 tests pass in my three files: 55 engine and 76 builders (both unchanged), 25 command line (19 and 6 new).
Seven are `t1b_finding_...` and assert an acceptance: F5, F13, F25, F26, F27, F28, F29.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,057 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 22

Scope: the fixes of F25 to F29 in `crates/kpf-cli/src/xcmds.rs` (the guard rewritten: every signed price kept, the
best signed price claimed, a proof at the claim's own price, the tickets read every round, the limit counting what
the round matched). My five finding tests are tests of the fix, each attack kept as it was built, and the fixes and
what they added were attacked. Only `crates/kpf-cli/tests/auditT1_cli.rs` and this file were changed by me; no node,
no network, nothing committed.

**The five fixes hold in everything I ran. Two new findings in what the fixes added, both open: F30 (Medium) and
F31 (Low). F13 (Medium) stays open, accepted by the owner.**

## The fixes

| | The attack, as built in round 21 | Now | Test |
|---|---|---|---|
| F25 | 60 ticks newer than the last quorum's, each of the smallest seat alone; or 60 files of `{}` under tick names | the account is claimed at the last quorum's tick (each in a folder of its own, nothing kept from another round) | `t1b_fixed_f25_ticks_no_quorum_signed_do_not_hide_the_last_one_a_quorum_did` |
| F26 | A claim at 11 from two seats, disputed; the third seat's 11.008 for the same tick in the folder | proved. Also proved: a claim declared by hand from the three files, and the claim at 11 from one 11 and the 11.008 (a quorum within the spread). Without a quorum near the claim's price the guard says exactly that | `t1b_fixed_f26_the_guard_proves_its_claim_at_the_price_it_states` |
| F27 | Seat 0's second price (20) for the last tick, written over its first file or under a name read before it, after the guard had read the tick | the account is claimed at the tick, the disputed claim made at it is proved, and the guard says the seat signed 2 prices; the prices read are in `prices-kept` | `t1b_fixed_f27_a_seats_second_signed_price_takes_nothing_away` |
| F28 | The ticket of the epoch that ended at 36,000, ticks of 36,100 signed by every seat | with the old ticket alone the guard says "1 of them are of a time no ticket given covers"; with the next epoch's ticket put into `--tickets`, the account is claimed at 36,100. That the tickets are read every round is from the code (`Guard::open` in `turn`) | `t1b_fixed_f28_a_tick_of_an_epoch_no_ticket_covers_is_said_and_the_new_ticket_is_taken` |
| F29 | A limit of 33.5 KAS and two pairs of 24 KAS in one round | the first is matched, the second "left unmatched"; with a limit both are within, the second goes on to be built | `t1b_fixed_f29_the_limit_under_a_roster_counts_what_the_round_has_matched` |

## Findings

### F30 (Medium, open). A price for another market takes the kept name of the real one

What the guard keeps of a signed price is a file named by the tick, the first 4 bytes of the market's id, the first
8 of the seat's key and the price (`raw`: `tick-<t0>-<market8>-<key16>-<price>.json`), written only if no file of
that name is there. A seat signs for whatever market id it likes. Its price for another market whose id begins with
the same 4 bytes, at the same tick and the same price, has the same name; read first, it takes the name, and the
seat's price for the real market is never kept. F27's fix then does not hold for that seat.

Test: `t1b_finding_f30_a_price_for_another_market_takes_the_kept_name_of_the_real_one`. Every seat signs 11 for the
market at the last tick; seat 0 also signs 11 for a market id that differs in its last byte, under a file name read
first. The guard claims (and does not accuse the seat: two markets). The kept file under the name of seat 0's 11 holds
the other market's tick. Seat 0's file in the ticks' folder is then written over by its second price for the real
market (F27's first form): no quorum, "0 sent".

- Cost to the adversary: as F27 (two prices for one tick forfeit the seat if somebody sends them in), and the
  other-market price must reach the folder before the guard first reads the tick. A price for a market nobody trades
  is no offence.
- Direction: name the kept file by everything that is signed (the roster, the whole market id, the key, the tick,
  the price: a hash of them is enough), not by prefixes.

### F31 (Low, open). One bad file among the tickets stops every action of the round

The tickets are read at the start of every round, and a file in `--tickets` that is no ticket fails the round: not
the guard alone, but matches, settlements and the rest, for as long as the file is there. A file half written when
the round begins is such a file.

Test: `t1b_finding_f31_one_bad_file_among_the_tickets_stops_every_action_of_the_round`. A round that only matches and
settles, with `--tickets` holding a good ticket: runs. With a truncated `half-ticket.json` beside it: the command
fails ("not JSON"). Offline a failed round is the command failing; with a node the loop says so and asks again later
(from the code).

Direction: pass a file that does not read as a ticket over, say so once, and go on with the tickets that do.

## What holds (run)

| Tried | Result | Test |
|---|---|---|
| A seat's lower second price (10.99) beside the quorum's 11, the user long | claimed at 11; the seat is said to have signed twice | `t1b_r22_the_guard_states_the_best_signed_price_and_proves_what_it_states` |
| A seat's higher second price within the spread (11.01) | claimed at 11.01 (the best of the signed prices a quorum stands near) | same |
| The claim the guard itself wrote at 11.01, disputed | proved; and proved again after the file of 11.01 is taken out of the ticks' folder (it is kept) | same |
| A tick exactly as old as a claim may be (100 DAA); a DAA older | "it cannot be claimed at that price any more"; not looked at ("of 0 ticks of that time there") | same |
| `--silence 90` with 100 DAA to land in | "it is never claimed" is said | same |
| A seat's price for another market at the same tick | adds nothing to the market's quorum and is no "two prices" against the seat | the F30 test, its first round |
| 3,000 files of no tick under names of the time looked at, 20,000 names of times past, a file of 8 kB and one of 100 kB of junk; 400 prices of one seat for one tick | the guard claims as before. A round took 148 ms with three ticks, 244 ms with the 23,000 files, 607 ms with the 400 prices (a test build) | `t1b_design_what_a_full_folder_of_ticks_costs_a_round` |

A claim the guard states is one it can prove for as long as `prices-kept` and the ticket are there: it states a price
a seat signed that a quorum stands near, from prices it has verified and kept, and a proof picks of each seat the
price nearest the claim's; more prices arriving later only add to what it can pick from (read, and run in the row of
11.01).

## Read, not run

- **`prices-kept` only grows.** Every verified price of every tick looked at is copied and nothing is removed; both
  folders' names are listed for every account of the key every round. The measure above says what 20,000 names
  cost; a market ticking every second writes tens of thousands of files a day and seat.
- **Kept is what was read.** A second price that lands on a seat's file before the guard has read the first is not
  undone by anything (the round's interval is the exposure).
- **The double price is said every round**, not once: `told` is the guard's, and the guard is opened anew each round.
- **The tickets are trusted files.** Of two tickets given that cover one tick, the first in the list is used; a file
  with another table under the same roster's id would be believed. It is the user's own folder.
- **The limit**: a round adds the two orders' whole margins for a match it sent or showed (a match of a part adds a
  part: it errs to leaving unmatched) and nothing for one the node refused. A match sent in one round is counted in
  the next only once its account is listed; in between, the next round does not know of it.
- **Two legs**: the best price is the best of the four pairs of lowest and highest signed price per leg; a tick
  counts when both legs have a quorum under their own tickets. Not run.
- **`--only`** was not looked at beyond that `prices-kept` is under `--dir` whatever it says.

## Not reached

- Anything with a node: several rounds (the tickets read anew, the backoff of a failed round, `told`), a live ticket
  found for a proof, a match the node refuses, two matches of one round really sent.
- A cross account under two rosters.
- The transport that brings ticks (not in the repository).

## Findings ledger

| | Severity | State |
|---|---|---|
| F30 | Medium | open: a kept price's name is not all that was signed |
| F31 | Low | open: a bad file in `--tickets` fails the whole round |
| F25, F26, F27 | High | fixed in round 22, verified by test |
| F28 | Medium | fixed in round 22, verified by test (one round; the re-reading from the code) |
| F29 | Low | fixed in round 22, verified by test |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | open on purpose |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F24) | | fixed in earlier rounds |

## Counts and commands

160 tests pass in my three files: 55 engine and 76 builders (both unchanged), 29 command line (25, and 4 new: F30, F31,
the best price, the cost of a round). Four are `t1b_finding_...` and assert an acceptance: F5, F13, F30, F31.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,061 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

For the renaming that follows this round: my three test files name scripts, entries, commands and messages of the
tool as strings (`"KpfTDeclare"`, `"acct-declare"`, `"claimed at the last one"`, `"prices-kept"`, file suffixes such
as `-ticket.json` and `script-KpfTOpen`), and the source tests of the engine file match lines of `contracts/*.sil`
literally (`RELEASE`, the four conditions of `goesOn`, `purse(j, slotLen, rid, floor);`). Those are where a
mechanical renaming will meet them.

# Round 23

Scope: the rule of a dispute changed (the committee answers when asked: one entry `answer` with a verdict in
`KpfTProve` and `KpfTLiqProve`, a lapse that returns both bonds), the arrears rule of `KpfTPay`, the fixes of F30 and
F31, and the renaming of the whole tool (a position, a close, an exit, a registry, a watchtower, a committee, its
members, an epoch table, a signed price). The names in this section are the new ones; earlier sections keep the names
of their time. My three test files were brought to the new rule and extended. No source, contract or artifact was
changed by me; no node, no network, nothing committed.

**Three findings, all open: F33 (High), F32 and F34 (Medium). The rule itself held in everything I ran: with the
committee's prices a disputed claim passes exactly one verdict, the one the model gives, and what it pays is the
model's to the sompi. The findings are in what the rule leaves when nobody answers, and in what `exec-run` disputes.
F30 and F31 are fixed and verified; the arrears rule holds. F13 (Medium) stays open, accepted by the owner.**

## The 27 tests that failed after the change

None of them found a hole in a script. Two of them state a property that is no longer true, and those are F32 and
F33 below.

- **Engine file, 15.** All fifteen failed through two scenes: `lapse` and `liq-lapse` paid by the old rule. With both
  bonds back, each at its own place (`m` the disputer's, `m - 1` the claimer's or the liquidator's), every one
  passes again, the structural ones among them: `t1b_fixed_no_output_pays_two_dues_of_one_key` (its 1,294 arrangements
  with a lapse), `t1b_o4_...`, `t1b_m5_...`, `t1b_j1_...`, `t1b_b1_...` (an operation beside a lapse), `t1b_a4`, `t1b_a5`,
  `t1b_r2_edge_amounts`. Two source tests were extended, not exempted: `t1b_a3_...` (the two scripts that answer end by
  the four conditions written in the entry, the third now `+ ORACLE_FEE + bond - half`; `KpfTLiqDispute.lapse` no
  longer keeps a half) and `t1b_r19_every_place_...` (the answer reads its object in the entry itself, with
  `j != this.activeInputIndex`; its other two places are epoch tables, `priced`).
  `t1b_fixed_the_half_of_a_liquidators_bond_stays_in_the_script` now holds the half for a late liquidation only, and
  holds a lapse to both bonds back at their own places and nothing more in the script.
- **Builders file, 7.** `t1b_j3_...` and `t1b_l4_...` stated "a sompi more than the rule's is not proved" from the
  claimer's cautious amount; the line is now what the side is owed at the committee's prices as they are, and they
  state a sompi above that. The watcher's lapse in `t1b_fixed_a_cure_waits_half_a_window_...` returns both bonds. F5's
  test is `t1b_design_a_side_truly_below_its_maintenance_takes_three_dispute_periods_to_cure_for_half_a_bond`: the
  proof now leaves half of the disputer's bond in the script, so the two dispute periods more cost half a bond and not
  the fees alone. Two tests measured that holding a position by one's own claim and dispute cost half a bond a round:
  it costs the fees (F32). `t1b_r16_what_the_executor_disputes_is_answered_by_no_proof` stated what is no longer so
  (F33); it is replaced by the tests of the new rule.
- **Command line, 5.** F30 and F31 are tests of the fix. F27's test reads the kept prices by their content (the
  names are hashes now). F26's last case matched a message that changed. The self-liquidation at the command line
  is F32.

## Findings

### F33 (High, open). `exec-run --do dispute` disputes by the price a claim states, and loses its bond to a true claim

Without the committee's signed prices at hand the executor disputes a claim that is not within the rule at the
prices the claim itself states (`refuting`: `c.within(c.a)`; the same for a liquidation). Since this round no script
reads those prices: a dispute is decided at the committee's. A claimer states a low price and the amount the true
price gives. The executor's rule calls it false and posts its bond; the claimer shows the committee's signatures;
the claim is proved. The claimer has its amount, its bond and half of the executor's; the other half stays in the
script. Nothing was false, the bait cost its maker nothing, and it pays half a bond each time it is taken.

Tests: `t1b_finding_f33_a_claim_stating_a_low_price_and_a_true_amount_baits_whoever_disputes_by_the_stated_price`
(builders, engine: a close stating 10.1 and what 10.5 gives; the disputer loses 10 KAS, 5 of it to the claimer;
nothing refutes it) and `t1b_finding_f33_exec_run_disputes_by_the_price_a_claim_states_and_loses_its_bond` (command
line: `pos-close --p1 10.1 --amount 44`, an offline round of `exec-run --do dispute` disputes it "its amount is not
the rule's at its own prices", `pos-prove` with the members' signatures of 10.5 is built and pays the executor
nothing).

- This is what `t1b_r16_...` guarded in round 16 ("what the executor disputes is answered by no proof"). The design
  section lists a price source of the executor's own under "not built yet"; what it does until then is not neutral.
- `dispute` is not in the default set of `--do`; it is in `all`.
- Direction (yours): without a price of its own or the committee's, do not dispute; or dispute only what no price
  could prove (an amount above the pot's bound for the side, a close where none is allowed at any price). A fix
  would make the command-line test's round send nothing.

### F32 (Medium, open; F2b of round 2 open again). A claim and a dispute of one owner hold a position for the fees

A lapse returns both bonds. A party that claims (a false close, a false liquidation of itself) with one key and
disputes with another holds the position for a dispute period, round after round, for the fees of three
transactions. In round 3 this was priced at half a bond a round, and my tests held it there until this round.

Tests: `t1b_finding_f32_a_liquidation_of_oneself_disputed_by_oneself_holds_a_position_for_the_fees` (three rounds: 150
of 180 DAA held for 0.0455 KAS of fees and no bond; round 3: 250 KAS a round),
`t1b_finding_f32_a_false_close_disputed_by_its_claimer_holds_a_position_for_the_fees`,
`t1b_finding_f32_the_command_line_self_liquidation_and_self_dispute_costs_the_fees`.

- The design states it as a limit and gives the reason: with an answer anyone ends the hold (a refutation, for half
  of the claimer's bond). That is so, and run (`t1b_r23_a_disputed_claim_...`). The hold is free exactly where
  nobody has the committee's signatures for the claim's time. The member's tool that signs for a disputed time is
  not built, so today that is everywhere a member does not happen to have signed; and it is always so against the
  adversary of round 21 (members holding a third, and the counterparty).
- What the party held can do is the exit at the entries, and it must land it between a lapse and the next claim.
- I record it as a finding because it reverses a fix this review verified, and because the condition under which it
  is free is the common one until the member's tool exists. If the owner accepts it, it belongs with F13.
- Directions, if wanted: keep a part of the claimer's bond at a lapse when the disputer is not the party the claim
  was made against (a third key that disputes and then lets it lapse is the pattern); or let the party held end a
  disputed claim it did not make by the exit.

### F34 (Medium, open). `exec-run` disputes a claim that one of the committee's prices proves

With signed prices at hand the executor disputes as soon as one price a quorum stands near refutes the claim
(`Watchtower::answer` with "not within" returns the first that does). The members' prices differ within the spread
and each is the committee's. A claim that states what the highest gives is refuted at the lowest and proved at the
highest; the claimer has its proof ready when it claims, and the executor's refutation comes a round after its
dispute.

Test: `t1b_finding_f34_exec_run_disputes_a_claim_that_one_of_the_committees_prices_proves` (command line: members
sign 10.5, 10.5 and 10.508; a close of 45.08 KAS, what 10.508 gives, is disputed "it states more than the prices the
committee signed for its time give"; `pos-prove` from the same three files is built and pays the executor nothing).

The design names the band ("the first to land has it"). What it does not say is that the tool walks into it with a
bond. Direction: dispute only where no price a quorum stands near proves the claim (`answer` asked for "within"
returns none). A fix would make the test's round send nothing.

## What holds (run)

| Tried | Result | Test |
|---|---|---|
| Closes on the line, a sompi each side, the claimer's cautious amount, nothing, the whole pot; both sides; one leg and two. Liquidations with the rule's fee and the least and the most cure, a sompi off each, a sompi of fee more, no fee, the other side, a sound position | 19 proved and 13 refuted at the engine: each passes the verdict the model gives (`Claim::within`, `Liq::within`) and not the other; the tool's checked builders build exactly the one that passes | `t1b_r23_a_disputed_claim_is_proved_or_refuted_never_both_and_pays_what_the_model_says` |
| What each verdict pays | the model's (`prove_pay`, `refute_pay`) to the sompi; the parts sum to what the object held; the half of the bond lost that is nobody's is in the script's copy with the oracle fee | same |
| Whoever refutes: a stranger, the claimer itself, the disputer, the other party | each has the same half; the disputer its bond besides | same |
| A proof told the verdict 0, 2 or -1; a refutation told 1; a proof told another key than the disputer's | refused | same |
| The reader | a proof read as `KpfTProve.answer` that ends the claim; a refutation as one that writes the position again | same |
| The claimer's caution: `true_claim` at its own price against 401 committee prices within g of it, each side | within the rule at every one | `t1b_r23_the_claimers_caution_keeps_it_within_the_rule_wherever_the_committee_is_within_the_tolerance` |
| `liquidation_own` at the liquidator's price against 401 committee prices within g | within the rule at 400; outside at the one exactly g below (the cure stated at its own price is a rounding under the least the rule then asks). The design says "nearer than g", and that holds. The engine agrees with the model at both ends | same |
| A lapse beside another operation, and its two payments against every other entry's places | as before the change: each at its own place, no output answering two | the engine tests named above |
| The arrears: a payment by a table whose window ends at 9,000 | locked there by the builder; locked a DAA earlier, at the price's time or at 0, refused | `t1b_r23_only_the_table_of_an_ended_epoch_pays_the_fee_pool` |
| F30 as built (a price for another market with the same first bytes, then the member's file written over) | both prices kept under names of their own; the position is claimed; the member is said to have signed twice | `t1b_fixed_f30_a_price_for_another_market_does_not_take_the_kept_name_of_the_real_one` |
| F31 as built (a half-written file in `--epoch-tables`, a round that matches and settles) | the round runs and says "passed over among the epoch tables" | `t1b_fixed_f31_a_bad_file_among_the_epoch_tables_is_passed_over` |

The hypotheses of (a), one by one, as far as I went:

- **Both verdicts or neither.** One function computes 0 or 1 and the verdict must equal it; I found no case of both
  or neither over the grid above (amounts at and around the line, both bounds of a cure, an empty second leg, both
  sides). A side other than 1 and 2 is refused by the entry (read). A bond of 0 or 1 sompi was not run: a bond is the
  position's own number, and a position whose bond is a sompi is not one the tool opens.
- **Value made or lost.** No: the sums above.
- **The places.** A proof pays at output j and at `len - 1 - 2j`; a refutation and a lapse at `len - 1 - 2j` and one
  before, the two places every entry of the family has for an object at input j. A proof beside another operation is
  run (round 2's test with the proofs, which passes with `answer`); a lapse beside one is run at the engine; a
  refutation beside one is not run (its places are the lapse's).
- **The refuter's key.** Any key: run with four.
- **A hold.** F32. With an answer: a false claim loses its bond (half to whoever refutes, half to the script), a true
  one is proved by anybody and settles.
- **The band.** F34 is who gains by picking: the claimer that states the highest of the members' prices, against a
  disputer that saw the lowest.
- **`liquidation_own`.** Holds inside the tolerance (above).
- **The lower bound of a cure.** At the engine: the least cure passes, a sompi less is refuted. It binds only a
  disputed liquidation that is answered: a liquidation of oneself with a cure of a sompi, undisputed, is cured half a
  dispute period later as before (`t1b_fixed_a_cure_waits_half_a_window_...`, three rounds for the fees and 3 sompi).
- **`exec-run` made to pay.** F33 and F34.
- **The reader.** Above.

## Read, not run

- `KpfTLiqProve.within` refuses every liquidation of a position whose two keys are one; `Liq::within` in the model
  does not say so in its own lines (it may come from `liquidatable`). I did not build such a position.
- The comment of `Disputed::refute_pay` in `thin.rs` still says "whose stated price is far from the committee's", and
  the help of `pos-close` says `--amount` states "a claim a dispute cannot be answered for". Both are the old rule:
  any amount is now answered one way or the other.
- A claim's stated prices are still in its hash and its file, and the tool still prints `is_true()` from them
  (`CHECK: FALSE`). They decide nothing on the chain; F33 is where the tool still treats them as if they did.
- The answer's arithmetic on positions at the caps (sizes, price limits, g) was not rerun this round; the
  coordinator's 160 random positions are what covers it.

## Not reached

- Anything with a node or a mempool: the race of a proof and a refutation, a claimer chaining its proof on an
  executor's unconfirmed dispute, the exit landing between a lapse and the next claim.
- A refutation woven beside another operation; an answer on a position of two committees.
- The member's tool and the reading of payloads (not built).
- The cure bounds on two legs beyond one case (the coordinator's random cases cover more).

## Findings ledger

| | Severity | State |
|---|---|---|
| F33 | High | open: `exec-run` disputes by the price a claim states |
| F32 | Medium | open: a lapse returns both bonds (F2b again); stated in the design as a limit |
| F34 | Medium | open: `exec-run` disputes what one of the committee's prices proves |
| F30 | Medium | fixed in round 23, verified by test |
| F31 | Low | fixed in round 23, verified by test |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | changed by round 23: the two dispute periods more now cost half a bond |
| F7, F8, F9 | Low | by design |
| F2b | | fixed in round 3; its fix is what F32 reverses |
| all others (F1-F4, F6, F10-F12, F14-F29) | | fixed in earlier rounds |

## Counts and commands

165 tests pass in my three files: 55 engine, 79 builders (76, the test of round 16 replaced, and 3 new), 31 command
line (29, two finding tests replaced by tests of the fix, and 2 new). Seven are `t1b_finding_...`: F13, F32 (three),
F33 (two), F34. As in every round, a finding's test asserts the hole as it stands, so the workspace is green with
the findings open; each fails once its finding is fixed, and its message says what it is then to be turned into.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,068 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 24

Scope: the fixes of F33 and F34 in `crates/kpf-cli/src/xcmds.rs` (what `exec-run --do dispute` disputes: the
closures `by_own` and `refuting`, the executor's own price), and nothing else. My two finding tests are tests of the
fix, and the new path was attacked. Only my test files and this file were changed by me; no node, no network, nothing
committed.

**F33 is fixed as it was built, and F34 as far as the prices members signed go. Four new findings in the path that
replaced them, all open, all the same shape as F33 (the executor posts a bond against a claim that is then proved):
F35 (High), F36 and F38 (Medium), F37 (Low). F32 (Medium) is with the owner; F13 (Medium) stays accepted.**

## The fixes

| | The attack, as built in round 23 | Now | Test |
|---|---|---|---|
| F33 | A close stating 10.1 and 44 KAS (what 10.5 gives), an executor with no signed price at hand | not disputed without a price of its own; not disputed with its own price of 10.5. A close of 50 KAS is disputed by that price ("not within the rule at any price near this executor's own"), and left when the executor has no price | `t1b_fixed_f33_exec_run_disputes_no_claim_by_the_price_it_states` |
| F33 | A price file that is empty, a word, a negative number, a number and words | said ("no price of this executor's own this round"), and nothing is disputed by it; the round goes on | same |
| F34 | Members sign 10.5, 10.5 and 10.508; a close of 45.08 KAS (what 10.508 gives) | not disputed. A close of 46 KAS, which no price near theirs gives, is disputed | `t1b_fixed_f34_exec_run_does_not_dispute_what_a_signed_price_proves` |

## Findings

### F35 (High, open). The executor's one price judges the claims of every market

`by_own` takes one price (`--price`, `--price-file`, `--price-cmd`) and applies it to every one-leg claim and
liquidation in the folder. It does not ask which market the position is of. A registry holds positions of any market.
A true close on another market, whose price is elsewhere, is within the rule at no price near the executor's own: it
is disputed, and its claimer proves it with that market's signed prices.

Test: `t1b_finding_f35_the_executors_one_price_judges_claims_of_every_market` (command line). A second market of the
same committee trades near 50; a position on it; the user's close from the members' signatures of 50.5. An executor
whose price is 10 disputes it; `pos-prove` is built and pays the executor nothing. The bond there is 100 KAS (a tenth
of the pot), half of it the claimer's.

- Who can do it: anybody. A market is a file and a committee; a position needs two keys, which can be one owner's;
  the pot, and so the bond the executor posts, is the maker's choice up to what the executor's coin holds.
- Direction: the price is of one market, and the executor is told which (`--market1` is already a flag of the
  loop); a claim on any other is not judged by it. A fix would make the test's round send nothing.

### F36 (Medium, open). The executor's price is of now, and the claim is of its time

The price is read once a round and is the price now. A claim is of the time its claimer chose, and `by_own` judges
claims up to twice `lateDaa` old. Where the price stood further from the present price than the margin (twice the
tolerance by default: 0.8 % on the test market), a true claim of that time is disputed and proved.

Test: `t1b_finding_f36_the_executors_price_is_of_now_and_the_claim_of_its_time` (command line): the committee signed
10.5 for the claim's time; with its own price at 10.5 the executor leaves the claim; at 10.2, 20 DAA later, it
disputes it, and `pos-prove` pays it nothing.

- The claimer chooses the time within what a claim may land in, so it chooses the moment the price was furthest.
- The same for a price source that is wrong: a file that is stale, or says 0 (a price of 0 is taken as a price;
  read, not run).
- Direction: judge a claim by the executor's price at the claim's time (keep the prices read, by DAA, and use the
  one nearest `t0`; none near, no dispute), and widen the margin by what the price moved between the two.

### F38 (Medium, open). F34's fix looks at the prices members signed; the band's ends are attestable too

`refuting` disputes where a price refutes and `g.answer(.., within)` finds none that proves. `answer` walks the
prices members signed (`leg` keeps the band's ends only when no signed price has a quorum). An epoch table attests
any price a quorum stands near: with members at 10.5, 10.5 and 10.508 it attests up to 10.5105. A claim of what a
price between the highest signed one and that end gives is refuted at every signed price, proved by none of them,
disputed, and proved at the end.

Tests: `t1b_finding_f38_a_claim_is_proved_at_a_price_between_and_beyond_the_members_signed_prices` (builders, engine:
members at 10.5, 10.5 and 10.5079; a close of what 10.5105 gives is not within the rule at either signed price, the
same three signatures attest 10.5105, the proof is taken, and the disputer loses 10 KAS; a refutation at 10.5 is
taken too: the first to land has it) and
`t1b_finding_f38_exec_run_disputes_a_claim_that_a_price_between_the_members_proves` (command line: a close of
45.1 KAS, what 10.51 gives, is disputed by `exec-run`).

- `pos-prove` picks the middle price and does not build that proof; a claimer that means to builds it itself.
- The room is a part of the spread (0.1 % here), less than F34 was, and it needs members whose prices differ.
- Direction: ask for a proof at the best attestable price for the claimer (the end of the band a quorum stands
  near), not only at signed ones. A fix would make the command-line test's round send nothing.

### F37 (Low, open). A wider margin is not a safer one for a liquidation

`by_own` tries 17 prices over the executor's price plus and minus the margin. A liquidation is within the rule over
a band of committee prices about twice the tolerance wide. At the default margin the prices tried are a quarter of
the tolerance apart and none is skipped. At `--dispute-margin-bps 2000` they are 2.5 % apart, the band (0.8 %) falls
between two, and the rule's own liquidation is disputed.

Test: `t1b_finding_f37_a_wide_margin_steps_over_the_band_a_liquidation_is_within` (command line): the committee
signed 6; at the default margin an executor whose price is 6.02 leaves the rule's liquidation alone; with a margin of
2,000 bps and a price of 6.075 it disputes it, and `pos-prove` pays it nothing.

A close does not have this (it is within the rule from some price on, so the end of the range decides). Direction:
step by no more than a part of the tolerance whatever the margin; or solve the band from the claim's own numbers.

## What holds (run and read)

- A claim of two legs is not judged by the executor's own price (`by_own` returns false for it), nor one whose
  time is more than twice `lateDaa` from the present. Nothing says so in the round's output: a false claim of two
  legs is simply left, to whoever has the signed prices. (Read.)
- A price source that fails costs nothing: the round says so and disputes nothing by it (run).
- A claim already disputed is refuted wherever an attestable signed price refutes it, as a transaction that must
  leave its sender its gain (half of the claimer's bond less the fee, at least `--min-gain`); one that would not is
  built once and left alone. I found no way to make the executor pay for refutations that do not land: a refutation
  that loses the race to a proof is not in a block. (Read.)
- `--dispute-margin-bps` that is no number is ignored without a word (the default is used); 0 or less makes the
  executor judge at its own price alone. (Read.)

## Not reached

- Anything with a node: the race of the executor's refutation and the claimer's proof, a round's price read from a
  command, several rounds.
- A liquidation judged by the executor's own price across two markets or a moved price (F35 and F36 are run on
  closes; `by_own` is the same closure for both).

## Findings ledger

| | Severity | State |
|---|---|---|
| F35 | High | open: one price for every market |
| F36 | Medium | open: the price of now for a claim of its time |
| F38 | Medium | open: the band's ends are not asked for a proof |
| F37 | Low | open: a wide margin steps over a liquidation's band |
| F33 | High | fixed in round 24 as built, verified by test |
| F34 | Medium | fixed in round 24 for the prices members signed, verified by test; what is left is F38 |
| F32 | Medium | open, put to the owner |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | changed by round 23: the two dispute periods more cost half a bond |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31) | | fixed in earlier rounds |

## Counts and commands

170 tests pass in my three files: 55 engine (unchanged), 80 builders (one new, and the builders' test F33 rested on
renamed `t1b_design_a_claim_is_decided_at_the_committees_prices_whatever_price_it_states`: what it shows is the
rule, and the finding was the executor's), 35 command line (two finding tests replaced by tests of the fix, four
new). Nine are `t1b_finding_...`: F13, F32 (three), F35, F36, F37, F38 (two). A finding's test asserts the hole as
it stands and fails once it is fixed.

The whole workspace, run once with `--no-fail-fast`: 122 suites, 1,073 passed, 0 failed, 0 ignored, exit 0. (After
that run one test was renamed, with its comment and its message, and the builders' file run again by itself: 80
passed. The workspace was not run a second time.)

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 25

Scope: the fixes of F35 to F38 in `crates/kpf-cli/src/xcmds.rs` (the executor's own price is one market's, read
every round and kept with its time; a claim is judged between two readings; the range is tried in steps of a quarter
of the tolerance; a proof is looked for at every attestable candidate). My finding tests are tests of the fix, with
the positive control asked for, and the path was attacked once more. Only my test files and this file were changed
by me; no node, no network, nothing committed.

**The four fixes hold as the attacks were built. Two findings in what is left, both open: F39 (Medium), which is the
part of F36 the design already says the margin alone covers, and F40 (Low). F32 (Medium) is with the owner; F13
(Medium) stays accepted.**

## The fixes, and the positive controls

| | The attack, as built in round 24 | Now | Test |
|---|---|---|---|
| F35 | A true close on a second market (the committee signed 50.5 there); an executor whose price, of the first market, is 10 | not judged, not disputed. Told the price is the second market's, it disputes (the price is then wrong for that market, which is what a price source is trusted with) | `t1b_fixed_f35_the_executors_price_judges_the_claims_of_its_market_only` |
| F36 | The committee signed 10.5 for the claim's time; the executor's price 20 DAA later is 10.2 | with a reading of 10.5 before the time and 10.2 after it: not disputed. With the 10.2 alone: not judged | `t1b_fixed_f36_a_claim_is_judged_by_the_readings_around_its_time` |
| F37 | The committee signed 6; the executor's price is 6.075 and its margin 2,000 bps | the rule's liquidation is not disputed (nor at 500 bps) | `t1b_fixed_f37_a_wide_margin_does_not_step_over_the_band_a_liquidation_is_within` |
| F38 | Members sign 10.5, 10.5 and 10.508; a close of 45.1 KAS (what 10.51 gives) | not disputed; nor a close of what the band's end gives to the sompi (45.1051051 KAS). A close of 45.11 KAS and one of 46 KAS are disputed | `t1b_fixed_f34_f38_exec_run_does_not_dispute_what_an_attestable_price_proves` |
| control | A close of 50 KAS, which no price near 10.5 gives; readings of 10.5 at 33,390 and at 33,420, the claim's time 33,400 | **disputed by own price** ("not within the rule at any price near this executor's own", 1 sent). `own-prices` holds the two readings with their times | `t1b_fixed_f33_exec_run_disputes_no_claim_by_the_price_it_states` |
| control | The same claim with one reading (a single round); with no price; with a price and no `--market1`; after a price file that said nothing, a word, a negative number or 0 in the round after the claim | not disputed, each time; the missing market and the silent file are said. With the file saying 10.5 again it is disputed | same |
| control | A liquidation of a side that is nowhere near its maintenance at the executor's readings (10) | disputed by own price | the F37 test |
| | `--dispute-margin-bps` given as a word, a negative number, a fraction | refused at the start | the F33 test |

## Findings

### F39 (Medium, open). A price that moved and came back between two readings

A claim is judged by the executor's readings each side of its time and the margin around them. The readings are a
round apart or more (20 s by default; up to three rounds each side are accepted). The claimer picks the time: where
the price stood further than the margin from every reading in between (0.8 % by default), a true close of that time
is disputed and proved. A claimer picks the best price of the time it may claim in for its own sake, with no
executor in mind.

Test: `t1b_finding_f39_a_price_that_moved_and_came_back_between_two_readings` (command line): readings of 10.2 ten
DAA before the claim's time and twenty after; the committee signed 10.5 for the time between; the true close is
disputed, `pos-prove` is built and pays the executor nothing. With a margin of 400 bps it is left.

- The design says of this that the margin is all that covers it. It is recorded here with a number so that it is
  a decision: at the default margin, a move of 0.8 % and back inside one round costs the executor a bond.
- Directions: a price source that gives the highest and lowest since the last reading (a claim is then judged by
  the true range); readings more often than rounds; or a default margin sized to what the market does in three
  rounds and not to the tolerance.

### F40 (Low, open). The readings carry no market

`own-prices` holds lines of a time and a price. An executor that ran with its price of one market and is started
again in the same folder with a price of another judges the second market's claims by the first market's readings,
where those are the readings around the claim's time. And the file is believed whatever wrote it.

Test: `t1b_finding_f40_the_readings_carry_no_market` (command line): two rounds with the first market's price (10)
around the claim's time, then a round with `--market1` the second market and its true price (50.5): the second
market's true close is disputed. In a folder of its own, with a reading of 50.5 before the time, it is left. Two
lines written into `own-prices` by hand judge it in a single round.

- It needs the operator to change markets in one folder within three rounds of the claim's time; nobody outside
  causes that. A trial without a node (`--daa`) writes readings too, at the time it was told.
- Direction: write the market on each line (or a file a market), and take no line of another.

## The other hypotheses of (b)

- **Midpoints and band ends refuted or proved at a price the script's attest refuses.** A candidate is used only
  through `Watchtower::at_price`, which builds the attestation and holds it to `Attestation::check`, the same rule
  as the epoch table's `attest` (each price within the spread of the price claimed, the weight a quorum). An
  attestation at the end of the band is taken by the engine
  (`t1b_design_a_claim_is_proved_at_any_price_a_quorum_stands_near_the_bands_ends_too`, builders). A refutation by the
  executor at a midpoint was not run at the engine. (Read.)
- **A liquidation whose band falls between two candidates.** The attestable prices around a quorum span at most
  about twice the spread, the band a liquidation is within about twice the tolerance, and the tolerance is at least
  four times the spread: where the two meet, an end of the attestable span is inside the band, and the ends are
  candidates. I found no case; none was built. (Reasoned from the code.)
- **The 4,000-step cap.** Steps are about four times the range in tolerances. With the default margin that is
  sixteen and four for each tolerance the readings differ by; the cap needs a margin flag of about a thousand
  tolerances, which is the operator's. A position whose own tolerance is tiny is of the executor's market by its id
  only, but a tolerance under four times the committee's spread cannot be answered at all (`priced` refuses it), so
  a claim on such a position is neither proved nor refuted and its dispute lapses with both bonds back. (Read.)

## Not reached

- Anything with a node: a price file's age (the three-round staleness is asked only when live), several rounds
  with their real spacing, the race of a refutation and a proof.
- The executor's refutation of an already disputed claim at a midpoint or a band's end, at the engine.
- A liquidation under F39 or F40 (both are run on closes; the closure is the same).

## Findings ledger

| | Severity | State |
|---|---|---|
| F39 | Medium | open: a move and back between two readings (stated in the design as covered by the margin alone) |
| F40 | Low | open: the readings carry no market |
| F35 | High | fixed in round 25, verified by test |
| F36 | Medium | fixed in round 25 as built, verified by test; what is left is F39 |
| F38 | Medium | fixed in round 25, verified by test |
| F37 | Low | fixed in round 25, verified by test |
| F33, F34 | | fixed in round 24, verified; their controls brought to round 25 |
| F32 | Medium | open, put to the owner |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | changed by round 23: the two dispute periods more cost half a bond |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31) | | fixed in earlier rounds |

## Counts and commands

171 tests pass in my three files: 55 engine (unchanged), 80 builders (one renamed: the proof F38 rested on is
`t1b_design_a_claim_is_proved_at_any_price_a_quorum_stands_near_the_bands_ends_too`), 36 command line (the tests of
F33 to F38 brought to the fix, F34 and F38 in one; two new). Six are `t1b_finding_...`: F13, F32 (three), F39, F40.
A finding's test asserts the hole as it stands and fails once it is fixed.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,074 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 26

Scope: the fix of F40 (a reading in `own-prices` names its market) and the first direction of F39 (a price source
may say `price low high`: the least and the most since it was last asked), in `crates/kpf-cli/src/xcmds.rs` and
`ccmds::feed_prices`. Two of my tests were updated and one added. Only
`crates/kpf-cli/tests/auditT1_cli.rs` and this file were changed by me; no node, no network, nothing committed.

**F40 is fixed. F39 is closed for a source that says its low and high, and stays as a stated limit of a source that
says one number: I move it out of the open findings to the limits by design, with one remark below on what the
three numbers have to mean. No new finding. F32 (Medium) is with the owner; F13 (Medium) stays accepted.**

| | Tried | Result | Test |
|---|---|---|---|
| F40 | As built: two rounds with the first market's price (10) around the claim's time, then the executor started with the second market in the same folder | the second market's true close is not judged. `own-prices` holds three lines of five fields, the first two with one tag and the third with another | `t1b_fixed_f40_a_reading_is_of_the_market_it_was_read_for` |
| F40 | Lines of the old two-field form, and lines with the other market's tag, written by hand | not read | same |
| F40 | Lines in the file's own form with this market's tag, written by hand | believed: the file is the executor's own (by design) | same |
| F39 | As built (10.2 before the claim's time, 10.2 after, the committee's 10.5 between), the later reading `10.2 10.2 10.5` from a file and from a command | the true close is left; a close of 50 KAS is still disputed by the same readings | `t1b_fixed_f39_a_source_that_says_low_and_high_covers_a_move_between_two_readings` |
| F39 | The same with one number (a file of `10.2`; a command printing `10.2 KAS now`) | the move is not seen and the true close is disputed, as in round 25 | same, and `t1b_design_f39_a_source_of_one_number_does_not_see_a_move_between_two_readings` |
| parsing | A file of two numbers; of four; three with the price under its low; three with the price over its high; a low of 0; a number and two words; a command printing `10.2 2026 11` | each is said ("no price of this executor's own this round") and gives no reading: nothing is judged that round | same |
| parsing | A command printing `10.2 9 12` (a price and two numbers that are not its low and high but lie each side of it) | read as a low of 9 and a high of 12: a close of 50 KAS is left. It allows more and never less | same |
| F33's control | The two readings' lines | five fields: the market's tag, the time, the price, and the price again as low and high | `t1b_fixed_f33_exec_run_disputes_no_claim_by_the_price_it_states` |

## F39 in the ledger

It changes. With a source that says the least and the most since it was last asked, the move of F39 is seen and the
true close is left (run). With a source of one number it is as it was, and the design states that as the limit of
such a source. I record it from here on as by design, not as an open finding, and the test of round 25 is kept under
the name `t1b_design_f39_...`.

One remark on it, read and not a finding: the three numbers protect only if the low and the high cover the whole
time since the executor's previous reading. A command is asked by the executor and can keep that. A file is written
by something that does not know when it was read: if it is rewritten every second with the last second's low and
high, the executor reading it every twenty seconds sees one second of the twenty, and believes it has seen them all.
Nothing in the file says what time its low and high are of. (A direction, if wanted: a fourth number, the time the
low and high are counted from, used only when it is not after the previous reading.)

## What the new parsing opens

Nothing that costs the executor a bond, in what I ran and read. Every malformed source ends in no reading, which
judges nothing. The one misreading found widens the range (the last row of parsing above): a command that used to
be read by its first word is now read as three numbers when its second and third words are numbers each side of the
first. A line of `own-prices` is read only with five fields, its market's tag, a low above 0 and the price between
its low and high (read).

## Not reached

- Anything with a node: the age of a price file, a command asked every real round, several rounds.
- A liquidation with a three-number source (run on closes; the closure is the same).

## Findings ledger

| | Severity | State |
|---|---|---|
| F40 | Low | fixed in round 26, verified by test |
| F39 | Medium | closed for a source that says low and high (round 26, verified); by design for a source of one number (a stated limit) |
| F35 to F38 | | fixed in round 25, verified |
| F33, F34 | | fixed in round 24, verified |
| F32 | Medium | open, put to the owner |
| F13 | Medium | open, accepted by the owner |
| F5 | Low | changed by round 23: the two dispute periods more cost half a bond |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31) | | fixed in earlier rounds |

## Counts and commands

172 tests pass in my three files: 55 engine and 80 builders (both unchanged), 37 command line (one new; the test of
F40 is a test of the fix; the test of F39 is kept as a design test). Four are `t1b_finding_...`: F13 and F32
(three).

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,075 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 27

Scope: a new script, `KpfTReplace` (two orders matched into a position), with its model, builder and command; and two
new actions of `exec-run`, `take` and `answer`. For the ledger: the owner accepted F32 on 22 November 2026; it is
with F13 from here on, and its three tests keep asserting the behaviour. The coordinator's mechanical changes to my
files (28 scripts, the raised totals) are as I would have made them: the structural tests ran on `KpfTReplace` and
passed. Only my test files and this file were changed by me; no node, no network, nothing committed.

**Six findings, all open: F41 and F46 (High), F42 to F45 (Medium).** F41 is in the script. F46 is in what the rule
of round 23 leaves unanswered, and is what makes F41 pay. F42 to F45 are in the two actions.

## Findings

### F41 (High, open). One party strikes a position again at a price of its choice

`KpfTReplace` takes the price of the match from the two orders. Both can be one owner's: the leaver's order the other
way, and the newcomer's, which is the leaver's own key or a second key of it. Nothing then stands between that price
and any number up to the market's limit. What the price "moved" since the position's entry is paid to the leaver at
once, out of the margin of the one that stays, whose entry becomes that price. The two conditions hold (the newcomer
posts what the one that stays asked and the move besides), so with prices signed the one that stays is worth what it
was: its worth is now a claim on the newcomer's margin. At the entries it is not: an exit claim settles a position
by its margins, and pays it its margin less the move.

Test: `t1b_finding_f41_one_party_strikes_a_position_again_at_a_price_of_its_choice_and_exits_at_it` (builders,
engine; the tool's own `replace` with its rules builds it). A is long against B, 10 KAS of margin each, each asking
the other's 10 KAS. A's short order and a long order of 20 KAS of margin (A's own key in one run, a second key in
the other) are matched into the position 1.98 above its entry: A is paid 9.9 KAS of B's margin at once; B is left
0.1 KAS from the new entry. The newcomer's exit claim, settled, pays B 0.1 KAS and the newcomer its 20 KAS and its
bond. B signed nothing after the opening.

- What it needs to pay: a settlement at the entries, that is an exit claim nobody refutes within the exit time
  (F46: under prices on demand, nothing does).
- Without that it is still a change nobody agreed to: B's margin and entry are whatever its counterparty leaves
  (what an exit then pays is run; the other rules that read a margin were not run on it).
- Directions: the price of a replacement is one the one that stays is not hurt by at its entry (`x` between the
  entry and what the stayer's own ask or a signature allows; or the stayer's margin and entry stay and the newcomer
  pays the leaver the move, as a novation does); or the leaver and the newcomer are held to be two (the script
  cannot know two keys are one owner's, so this alone does not close it).

### F46 (High, open). Nothing answers an exit claim

An exit claim says the committee signed no price of the market since a time. It is refuted by a price signed for any
later time; unrefuted for the exit time, it is settled at the entries: each party has its margin, whatever the price
did. Under the rule of round 23 members sign only where a dispute asks, and `exec-run --do answer` answers a disputed
claim and a disputed liquidation and nothing else. So a party that is losing makes an exit claim and is out at its
entry a day later, unless the other party manufactures a dispute on that market for a member to answer, and then
shows that price.

Test: `t1b_finding_f46_nothing_answers_an_exit_claim` (command line): an exit claim of the member's market in its
folder; the member, with its own price read before and after the claim's `since`, signs nothing in three rounds.

- This is not new with this round's action; the action is where it shows. `member-run`, which signs on a grid, still
  exists; whether a committee runs it is what decides. The design says nobody is sent prices.
- Direction: a member answers an exit claim of its market (signs its reading of a time after `since`), and the
  watchtower refutes an exit claim against its key with it.

### F42 (Medium, open). The cap of `take` does not count margin in a claimed position

`held` sums the executor's margin in its orders and in positions. A position under a claim, a disputed claim, a
liquidation or an exit claim holds the same margin and counts for nothing.

Test: `t1b_finding_f42_the_cap_of_take_does_not_count_margin_in_a_claimed_position`: with 60 KAS in a position and
`--take-max 60` an order is left; with that position under a declared close it is answered.

### F43 (Medium, open). `take` counts as a way out a position the order cannot take it out of

Where an order would let the executor out of what it holds, `take` asks no spread and does not look at the cap.
"What it holds" is every one-leg position of the market it is a side of; only a position of the order's template is
left by it. Against an order of another dispute period, the executor's own order opens a second position at its own
price, with no spread, beyond its cap.

Test: `t1b_finding_f43_take_counts_as_a_way_out_a_position_the_order_cannot_take_it_out_of`: long 10 units, a cap
of 1 KAS; a buy order at the executor's own price, of a dispute period of 60 where the position's is 50, is answered.
(The order of the position's own template is answered too, which is the design; without the position it is left.)

Also read: even with one template, anybody may match the two orders plainly (`KpfTMatch`) before the round that would
replace: the executor then holds both positions, at no spread.

### F44 (Medium, open). `take` does not look at the low and high its own source says

Test: `t1b_finding_f44_take_does_not_look_at_the_low_and_high_its_own_source_says`: a source that says `10 10 10.5`;
a buy order at 10.2 is answered as 2 % above the executor's price, inside what the source saw since it was last
asked.

### F45 (Medium, open). A member signs its nearest reading for a time its source says the price moved at

`answer` signs the price of the reading nearest the claim's time, a round away or less, as the price of that time.
Whoever makes the claim and the dispute picks the time (and since F32 a claim and a dispute cost the fees).

Test: `t1b_finding_f45_a_member_signs_its_nearest_reading_for_a_time_its_source_says_the_price_moved_at`: readings
of 10.2 ten DAA before the time and `10.2 10.2 10.5` twenty after: the member signs 10.2 for the time between. A
signed price is what the member's stake answers for; and beside members that signed what the price was, it is
outside the spread and adds nothing to a quorum.

Direction: sign only where the readings around the time agree within the committee's spread (lows and highs
counted), and nothing otherwise.

## What holds (run)

| Tried | Result | Test |
|---|---|---|
| `take`: a buy 2 % above and a sell 2 % below the executor's price | answered (the round goes to place its own order; offline it finds no anchor and says so, which is all an offline round shows of the decision) | `t1b_r27_take_answers_an_order_the_spread_better_than_its_own_price_and_no_other` |
| `take`: an order half a percent away (and with a spread of 0.2 %); the executor's own order; a buy at 10.2 against a price of 10.15; no price this round; its margin at the cap; no `--take-max` | left; answered; left; left; left and said; left and said; refused at the start | same |
| `answer`: a member with a reading ten DAA from a disputed claim's time | signs that price for the time, one file; run again, nothing more | `t1b_r27_a_member_answers_a_disputed_claim_of_its_market_once` |
| `answer`: a claim nobody disputed; a reading 300 DAA from the time; no `--member-key` | nothing signed, the last two said | same |
| `KpfTReplace` as built by the tool: a sompi less in the new position or to the leaver | refused | the F41 test |

## Read, not run

- **The margin the executor posts is the order's to set.** Its own order carries `max(the order's margin, what the
  order asks of its counterparty)` and asks only the order's margin of the other side. An order of a small margin
  that asks a large one makes the executor post up to its cap against that small margin: the order's owner can lose
  its small margin and win the executor's large one.
- **The executor's own order rests for three rounds and the guard, with no tip.** Anybody matches it at its limit in
  that time, with any order that posts what it asks: the one it answered may be cancelled first. Past its expiry it
  is still matched (F7, by design) until somebody returns it, and `expire` is not in the default set of `--do`.
- **`KpfTReplace`:** the leaver's order is used as it stands (its `min_counter` asks nothing of anybody: it leaves);
  the payments are at the two places of the position's input, as a lapse's; the three objects are counted under one
  id and the outputs under it are held in order. A replacement beside another operation, orders past their expiry,
  a position or an order holding more than its margins, and the output order with every number of objects were not
  run by me (the coordinator's two tests run the last three).
- **`answer`** asks the claim's market and committee of a leg, so a claim of another committee's market with the
  same id is not answered.

## Not reached

- `take` placing an order: it finds its anchor at the node. What the order holds, the next round's match, the
  replacement by which the executor leaves.
- `KpfTReplace` in the raw-engine file (it is run at the engine through the builders).
- Anything with a mempool: who matches the executor's order first.

## Findings ledger

| | Severity | State |
|---|---|---|
| F41 | High | open: a position struck again at a price one party chose |
| F46 | High | open: nothing answers an exit claim |
| F42 | Medium | open: the cap of `take` and claimed positions |
| F43 | Medium | open: `take` and a position of another template |
| F44 | Medium | open: `take` and the source's low and high |
| F45 | Medium | open: a member signs its nearest reading |
| F32 | Medium | accepted by the owner (22 November 2026) |
| F13 | Medium | accepted by the owner |
| F39 | Medium | by design for a source of one number; closed for one that says low and high |
| F5 | Low | changed by round 23: the two dispute periods more cost half a bond |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31, F33-F38, F40) | | fixed in earlier rounds |

## Counts and commands

180 tests pass in my three files: 55 engine (unchanged by me), 81 builders (one new), 44 command line (seven new).
Ten are `t1b_finding_...`: F13, F32 (three), F41 to F46. A finding's test asserts the hole as it stands and fails
once it is fixed.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,085 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 28

Scope: the fixes of F42 to F46 in `crates/kpf-cli/src/xcmds.rs` (design text: `docs/DESIGN-COMMITTEE.md`, "After
review T1b round 27"), the coordinator's question on F41 (does the same run as a match and a novation), and what the
fixes open. Only my test files and this file were changed by me; no node, no network, nothing committed.

**F42 to F46 are fixed and verified. F41 stays open and is older than `KpfTReplace`: the same runs as a plain match
followed by a novation. Three new findings: F49 (High) in `take`, and F47 (Medium) and F48 (Low) in what the fix of
F46 leaves.**

## Findings

### F49 (High, open). `take` answers an order whatever the terms of its template

`take` chooses an order by its market, its committee, its price, its margins and its expiry. It does not look at the
rest of the order's template: the dispute period, the exit time, the bond rate. Those are the placer's choice, and
the executor's own order is written for the placer's template (`tpl: o.tpl`). I found this while checking a
sentence of my own draft ("`take` reads only the templates it was started with"), which is false.

- Test: `t1b_finding_f49_take_answers_an_order_whatever_the_terms_of_its_template` (command line). An executor
  started for a dispute period of 50 DAA and the default exit time and bond answers a buy order 2 % above its price
  whose template says a dispute period of 1 DAA; one that says an exit time of 1 DAA; one that says a bond of
  1 basis point. (Offline the decision is all a round shows: it goes to place its order and finds no anchor.)
- How a template reaches a round, read and not run: from a file that carries one (the tests), and from the chain,
  where `Registry::apply` learns the template of every `KpfTMatch` it reads (a match shows its template whole;
  `thin_read.rs`). One match of two orders of one owner makes a template known to every executor that follows the
  chain. `Position::check` accepts a dispute period and an exit time of 1 DAA and a bond of 1 basis point.
- What each gives the placer, read from the family's rules and not run this round. A dispute period of 1 DAA: a
  claim is paid as stated once its dispute period has passed undisputed (`KpfTClose` asks only `amount <= pot`), and
  no round disputes within 1 DAA, so the placer claims the pot. An exit time of 1 DAA: an exit claim is settled at
  the entries before any member can sign (the grid alone is up to 300 DAA), so the placer is out at its entry
  whenever the price goes against it and closes at the price when it goes its way. A bond of 1 basis point: a
  false claim costs a ten-thousandth of the pot.
- Bound: `--take-max` (the executor's margin at risk), and the 1 % the bait gives up. `take` has placed no order at
  the engine or on testnet yet.
- Fix I would accept: `take` answers only orders of the templates the executor was started with (`--market1` with
  `--dispute-daa`, `--bond-bps`, `--exit-daa`), or of terms at or above floors it is given. The test then fails
  for each of the three orders.

### F47 (Medium, open). An exit claim is asked one time and no other

A member is asked, for an exit claim, the first multiple of 300 DAA at or after the claim landed (or its `since`,
the later of the two). It signs for a time only with a reading each side of it within a round, and only where those
readings are no further apart than the committee's spread (the fix of F45). Where either fails, the member signs
nothing for that claim in that round or in any later one: the time asked never changes, though a price of any time
at or after `since` would end the claim.

- Test: `t1b_finding_f47_an_exit_claim_is_asked_one_time_and_no_other` (command line). An exit claim landing at
  33,400 is asked for 33,600. A member reads 11 at 33,500 and `11 11 11.1` at 33,700 (1 % apart; the spread is
  0.1 %), then 11 at 33,900, 34,100 and 34,300: it signs nothing in any of the five rounds. A member whose first
  reading is at 33,700 (started after the grid time) signs nothing in three rounds.
- Why it matters: the claimer chooses when its claim lands, so it chooses the grid time to within 300 DAA. It
  cannot make the price move, but it can wait for a time the price is moving. A move of more than the spread
  between two readings a round apart, at members holding more than a third of the weight, leaves the claim with no
  quorum for good; so does a member's machine being down across that one time. The claim is then settled at the
  entries after the exit time, unless a quorum's prices of another time at or after `since` are at hand from some
  other dispute. That settlement is what F41 needs.
- What a failed attempt costs the claimer: its bond (half to whoever ends the claim, half to the other party).
- Fix I would accept: while an exit claim stands and the member has not signed for it, it is asked the next
  multiples of 300 too (every grid time at or after the first, up to this round), and signs for the first one its
  readings allow. The test then fails at 33,900 or 34,100 in the first case and at 33,900 in the second.
- The same silence for a disputed claim ends in a lapse (both bonds returned, the position back): that is the
  design of round 23 and of F45's fix, and its cost is delay. I did not number it.

### F48 (Low, open). The watchtower does not end an exit claim that landed before its `since`

The round ends an exit claim by the committee's prices only if the claim "is of the past" (`since` at or before the
DAA it landed at). One that landed before its `since` is ended only under `--do late` (`KpfTExit.early`, anyone, no
price). Under `--do watch` or `--do dispute` without `late`, the round does nothing with it: not while `since` is
to come, and not after `since` has passed with a quorum's prices of a later time in the folder, which the script
would take (`KpfTExitRefute` asks `t1 >= since` and nothing of when the claim landed).

- Test: `t1b_finding_f48_the_watchtower_does_not_end_an_exit_claim_of_a_time_to_come` (command line). An exit claim
  of `me` lands at 33,400 with `since` 34,200. The three members are asked for 34,200 and sign it (run). At 34,400
  the other party's round under `--do watch`, `--do dispute` and `--do watch,dispute,settle` sends nothing; under
  `--do late` it ends the claim.
- Bound: `late` is among the default actions, so an executor run with no `--do` ends such a claim, and so does
  anyone for the bond's share. A party that runs only `--do watch` for its own positions relies on those.
- Fix I would accept: under `watch` (own key) and `dispute`, a claim that landed before its `since` is ended as
  `late` ends it; or the price path stops asking `of_the_past`.

## F41, the coordinator's question

**The reading is right: it runs without `KpfTReplace`.**
`t1b_finding_f41_the_same_by_a_match_and_a_novation` (builders, every transaction signed and verified by the
engine): U long against S, 10 KAS a side. U's short order and a long order of L, a second key of U, are matched by
a plain `KpfTMatch` 1.98 above the entry (Y: L long against U). `novate` of X and Y builds with its rules on:
transfer 9.9 KAS out of S's margin to U, Z is L against S from Y's entry with S's margin at 0.1 KAS. L's exit claim,
not ended, settles at the entries: S is paid 0.1 KAS of its 10. The numbers are those of the `KpfTReplace` form.
Nothing stops it there, so there is no rule to carry over. F41 is a property of striking the one that stays again,
and it is older than round 27. I reviewed the novation in earlier rounds and did not find this then.

**The second direction (the one that stays untouched, the newcomer paying the move).** This is reasoning, not a
test: there is no code of it. I attacked the two sentences and they hold.
- "It moves the exposure to the newcomer, honest ones too": yes. Whoever leaves is paid the move with no signed
  price. At a settlement at the entries that move is borne by a party that stays: the stayer (as built) or the
  newcomer (the other form). No rule of the script chooses a third party, short of holding the leaver's gain until
  a price is signed, which is not leaving.
- "A leaver and a stayer of one owner prepare a position struck far from the market": yes, and it is the worse of
  the two. With the stayer untouched, an honest order pays the leaver the move at once and holds a gain against the
  stayer's margin alone. Unless a rule asks the stayer to hold the move, the loss needs no settlement at the
  entries at all; with such a rule it is F41 with the honest order as the one that loses.
- One difference I can name, which does not change the choice: as built, the one that loses is a party that did
  nothing and has no number to bound it (the attacker sets its own order's `min_counter` to 0, and `zs > 0` is all
  that is asked for the stayer). No floor on `zs` helps: an honest strike at a true price can leave the stayer as
  little.
- So what bounds F41 is what the coordinator says: an exit claim being ended while a committee answers. After this
  round that holds except where F47 and F48 say. I keep F41 open as High in the ledger until the owner decides; if
  it is accepted, the sentence users need is: at a settlement at the entries a party is paid its margin as its
  counterparty's last strike left it.

## Fixed and verified

| | Test (command line, offline) | What it shows |
|---|---|---|
| F42 | `t1b_fixed_f42_the_cap_of_take_counts_margin_in_a_claimed_position` | 60 KAS of the executor's in a position, in that position under a declared close, and under an exit claim: an order is left at `--take-max 60` and answered at 80 |
| F43, F44 | `t1b_fixed_f43_f44_take_leaves_what_is_no_way_out_and_what_its_source_saw_above` | long 10 units, cap 1 KAS: an order of the position's template at the price is answered; one of another dispute period is left at the price, left by the cap 2 % above it, answered within a cap of 500. Source `10 10 10.5`: a buy at 10.2 is left (also as a way out), one at 10.7 answered. An order of 12 KAS asking 200 KAS of its counterparty is left |
| F45 | `t1b_fixed_f45_a_member_signs_where_its_readings_around_the_time_agree` | readings of 10.5 each side: signs 10.5 for the time, once. `10.2` then `10.2 10.2 10.5`: nothing, and says why. 10.5 then 10.52: nothing. 0.05 % apart: signed. Nothing with one reading, for an undisputed claim, with readings 300 DAA away, without its key; without an epoch table the round is refused |
| F46 | `t1b_fixed_f46_an_exit_claim_is_answered_and_ended` | the positive control asked for, below |

The positive control for the exit claim: an exit claim of `me` (since 33,380) lands at 33,400. Three members on
three folders, with rounds at different times (33,450/33,650, 33,500/33,700, 33,590/33,610) and prices 11, 11.005
and 11.01, each sign for 33,600; members 1 and 2 are shown to sign nothing in their round before the grid time. With no price the claim
is left. With members 1 and 2 (37 of 67 KAS) it is left. With member 0 too, a round under `--do dispute` ends it:
5 KAS (half the bond) to the other party, 4.983276 KAS to the sender after its fee. Under `--do watch` the other
party's round ends it and a third key's does not.

## What the fixes open (item c)

- **The grid time, can a claimer choose where it falls:** yes, to within 300 DAA, by when it lands: F47.
- **Three members with different rounds:** run in the F46 test with prices 0.05 % apart; they sign for one time
  and are a quorum. By reasoning, not a test: each member's price is within the spread of what it read around the
  time, so members reading one market differ by about two spreads at most, and the collector tries the midpoints.
- **An exit claim landing before its `since`:** the members are asked for the first grid time at or after `since`
  and sign it (run); the round does not end the claim by it: F48.
- **A member signing for an exit claim it is itself a party of:**
  `t1b_design_a_member_that_is_a_party_answers_its_own_exit_claim_or_is_silent`. The round does not look at who the
  parties are: member 0, long against `exec` and the claimer, signs as any other when it runs, and the claim is
  ended. Silent, it leaves members 1 and 2 with no quorum and its own claim stands. That is the rule of 67 % (more
  than a third of the weight, silent, ends nothing). I name it by design; the party that gains by the silence is
  here the one that is silent, and no charge answers silence.

## Read, not run

- F42: the bonds (a claimer's, a disputer's, a liquidator's) and the margin under a disputed claim and a
  liquidation are counted by the same lines as the two states I ran; read only.
- `answer`, the branch "no epoch table of the committee is given": reached only with a table of another committee;
  read only.
- The round ends an exit claim of a two-leg position with prices of one time for both legs; the script allows two
  times. Read only.
- A grid time on an epoch's first DAA is covered by the next epoch's table; a member that left with the epoch signs
  a price that counts for nothing. Read only; F47's fix covers it.
- A position's exit time may be any value from 1 DAA (`Position::check`; the tool's default is 864,000). One
  shorter than about 500 DAA (the grid, then a round for the reading after it) is settled before a member can
  sign. Between two parties it is their own term; for `take` it is F49. Read only.
- F49: how a template is learned from the chain, and what a short dispute period, a short exit time and a small
  bond give the placer. Read only; the test shows the decision of `take`.

## Not reached

- `take` placing an order: offline it finds no anchor, so every `take` test shows the decision and not the order.
- Any of this at a node; a real price source; more than one leg under `take` or `answer`.
- The second direction of F41: no code exists, so nothing was run.

## Findings ledger

| | Severity | State |
|---|---|---|
| F41 | High | open, the owner's to decide: runs as `KpfTReplace` and as a match and a novation |
| F49 | High | open: `take` answers an order whatever the terms of its template |
| F47 | Medium | open: an exit claim is asked one time |
| F48 | Low | open: a claim that landed before its `since` is ended only under `late` |
| F42, F43, F44, F45, F46 | | fixed in round 28, verified |
| F32 | Medium | accepted by the owner (22 November 2026) |
| F13 | Medium | accepted by the owner |
| F39 | Medium | by design for a source of one number; closed for one that says low and high |
| F5 | Low | changed by round 23: the two dispute periods more cost half a bond |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31, F33-F38, F40) | | fixed in earlier rounds |

## Counts and commands

183 tests pass in my three files: 55 engine (unchanged by me), 82 builders (one new), 46 command line (the five
finding tests of F42 to F46 and the control of `answer` became four tests of the fix; four new). Nine are
`t1b_finding_...`: F13, F32 (three), F41 (two), F47, F48, F49. A finding's test asserts the hole as it stands and fails
once it is fixed.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,088 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders -- t1b_finding_f41 --nocapture
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```

# Round 29

Scope: the fixes of F47, F48 and F49 in `crates/kpf-cli/src/xcmds.rs` (design text: `docs/DESIGN-COMMITTEE.md`,
"After review T1b round 28"), and what they open. Only my command-line test file and this file were changed by me;
no node, no network, nothing committed.

**F47, F48 and F49 are fixed and verified. One new finding, Low: F50. F41 is with the owner.**

## Findings

### F50 (Low, open). A member signs every grid time of a claim that stands, also after a quorum signed one

Since the fix of F47 a member is asked every multiple of 300 DAA of an exit claim, each round, while the claim
stands, and signs each one its readings allow. It does not stop when the folder it signs into already holds a
quorum's prices of one of the claim's times, which is all that ending the claim takes.

- Test: `t1b_finding_f50_a_member_signs_every_grid_time_of_a_claim_that_stands_also_after_a_quorum_signed_one`
  (command line). The three members sign 33,600; nobody ends the claim; member 0, run on to 34,900, signs 33,900,
  34,200, 34,500 and 34,800 too (seven files where three end the claim).
- Counted, not run: a claim that stands its whole exit time (864,000 DAA by default) is 2,880 times a member. Each
  is a signed price the member's stake answers for, a line in its key's journal, and a file every watchtower's
  round lists.
- Bound: a claim is ended by the first round under `--do dispute` that sees a quorum's time, for half the bond, and
  then asks nothing more. The claim stands on only where nobody runs that, or no quorum forms.
- Fix I would accept: a member signs no further time of a claim once the folder holds a quorum's prices of one of
  its times at or after `since` (it has the epoch tables to tell); or a number of times a claim that it signs at
  most.

## Fixed and verified (command line, offline)

| | Test | What it shows |
|---|---|---|
| F47 | `t1b_fixed_f47_an_exit_claim_is_asked_every_grid_time_while_it_stands` | the two cases of round 28: a member whose readings around 33,600 are 1 % apart signs nothing for it and signs 33,900 at its next calm round; a member started at 33,700 signs 33,900. Three members that each miss a different one of three times: two signatures a time; 33,600 has members 1 and 2 (37 of 67 KAS, no quorum), 33,900 members 0 and 2 (45), 34,200 members 0 and 1 (52); a round under `--do dispute` ends the claim |
| F48 | `t1b_fixed_f48_an_exit_claim_that_landed_before_its_since_is_ended_by_a_price_after_it` | landing 33,400, `since` 34,200: the members are asked no time before 34,200 and sign it; `--do watch` (the other party) and `--do dispute` end the claim with those prices and send nothing without them; `--do late` ends it as before |
| F49 | `t1b_fixed_f49_take_answers_only_orders_of_the_templates_it_was_started_with` | started for a dispute period of 50 DAA: the order of that template is answered; the orders of a dispute period of 1 DAA, an exit time of 1 DAA and a bond of 1 basis point are left. Started for each of those terms it answers that order (its operator's choice). Started for 1 DAA or with no `--dispute-daa` (600) it leaves the order of 50 |

## What the fixes open

- **Ten times a round and the key's journal** (run, in the F50 test). The journal refuses another price for a time
  the key signed, whatever the folder: member 0 run from a second folder with a reading of 11.005 is refused
  33,600, which it signed at 11. The same price it signs again: sent to another folder of signed prices it writes
  the five times it had signed once more in one round. That is no second price. Two machines with copies of one
  key file have two journals; that was so before this round and is the operator's to avoid.
- **A member signing many times for one claim:** F50.
- **The grid when a claim stands for long:** the newest ten multiples up to now are asked, so a member that was
  away signs the times around its new readings and not the old ones (read; the "late" case of the F47 test is the
  short form of it). A reading exactly on a grid time is signed as the price of that time; what it says of the low
  and high before it is not asked (read: `later` is empty when the reading before and the one after are one). I
  take that as right: the reading is of that instant.
- **`started`:**
  - A flag given twice: the later one counts, in `started` as in the registry's own template (run both ways: `50`
    then `1` leaves the order of 50, `1` then `50` answers it). No mix of the two.
  - `--market1` given twice: the executor has no price of its own ("give exactly one --market1 with it"), so `take`
    answers nothing (run). The line speaks of disputes only; `take` itself says nothing.
  - The flags against the registry's own templates: `Registry::open` builds its template from the same flags and
    defaults, so what is started is always among what the registry knows (read). A template the folder learned
    from a file is left (run: the three orders).
  - An order of a template the executor holds a position in but was not started with is no way out any more (read:
    the test of `started` comes before the way out). That is the careful side.

## Read, not run

- F50's count for a whole exit time, and what a folder of that many files costs a round that looks for a quorum
  (it tries the times newest first until one has a quorum; with none it reads them all).
- The two points on the grid above marked read.

## Not reached

- `take` placing an order (offline no anchor); anything at a node; a real price source.
- A template learned from a match on the chain and then left by `take`: the test shows it with a template a file
  carries, which the round learns by the same `learn`.

## Findings ledger

| | Severity | State |
|---|---|---|
| F41 | High | open, with the owner (the coordinator recommends leaving the re-strike as it is) |
| F50 | Low | open: a member signs every grid time of a claim that stands |
| F47, F48, F49 | | fixed in round 29, verified |
| F42 to F46 | | fixed in round 28, verified |
| F32 | Medium | accepted by the owner (22 November 2026) |
| F13 | Medium | accepted by the owner |
| F39 | Medium | by design for a source of one number; closed for one that says low and high |
| F5 | Low | changed by round 23: the two dispute periods more cost half a bond |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31, F33-F38, F40) | | fixed in earlier rounds |

## Counts and commands

184 tests pass in my three files: 55 engine and 82 builders (both unchanged this round), 47 command line (the three
finding tests became three tests of the fix; one new). Seven are `t1b_finding_...`: F13, F32 (three), F41 (two),
F50.

The whole workspace, run once with `--no-fail-fast` after my last change: 122 suites, 1,089 passed, 0 failed,
0 ignored, exit 0.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli -- --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --workspace --jobs 2 --no-fail-fast
```
