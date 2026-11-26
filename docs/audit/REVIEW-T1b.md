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

# Round 30

Scope: the fourteen new silverscript contracts of the "thin family" (`contracts/KpfNote.sil`, `KpfTNote`, `KpfTCite`,
`KpfTChange`, `KpfTChangeSettle`, `KpfTChangeDispute`, `KpfTChangeLate`, `KpfTTake`, `KpfTKind`, `KpfTPoint`,
`KpfTStepRow`, `KpfTStepPrice`, `KpfTStepEnd`, `KpfTStepLiq`): price notes and their citation, a change of a position
stated and answered, and accounts of any number of legs answered a point at a time
(`docs/ARCHITECTURE-CLEARING.md` 3, 4a, 4b). Only my three test files and this file were changed; no node, no network,
nothing committed. I kept the author's three edits in my files (the `TObj::Change` arm in `auditT1_builders.rs`, the
two script counts in `auditT1_cli.rs`).

**Five new findings: F51 (High), F52 (High) and F54 (High), F53 (Medium), F55 (Low). F50 of round 29 is now fixed in
the code; its three round-29 command-line tests were stale and I reworked them. The A3 table is extended to all 41
scripts.**

## Findings

### F51 (High, open). A sompi more in the object at the dispute refutes a take that is within the rule

`KpfTTake` (and every answer of a point) reads what the object holds from the live input it answers
(`held = tx.inputs[j].value - bond - disputed * bond`) and asks the statement's `outs` to be exactly the rule's with
that number. `KpfTChangeDispute.dispute` writes the disputed object with `>= its value + bond`: a disputer may write
**one sompi more**. Then `held` is one sompi larger than at the statement, the stored `outs` (the position holds
`held - amount`) no longer match, the verdict is "not within the rule", and the honest take is reverted: the claimer
loses its bond and its take, the disputer has its bond back and half of the claimer's.

- Test: `t1b_finding_f51_a_sompi_more_at_the_dispute_refutes_a_true_take` (builders, engine-run). A take of 54 KAS on
  a position (within the rule at the committee's 12.0) and a take on an account of three legs, each disputed with a
  sompi more, are answered "not within": the claimer is paid nothing, the disputer takes its bond and half the
  claimer's, and the forced "within" verdict is refused. The control (disputed as the builder writes it) is answered
  within, as it must be.
- What is lost and by whom: the claimer, a bond (a tenth of the pot in the sample, 10 KAS) and its legitimate take;
  the attacker, +half the claimer's bond. The claimer cannot rescue it: once the object holds the extra sompi, no
  answer of that statement is "within". It is a dispute-step attack (the direct answer of an undisputed statement
  reads the fixed state output, which nobody can inflate).
- Fix I would accept: the answer derives `held` from the position's own committed value (the pot the moving part
  carries) plus the carrier it may hold, not from the live input less the bonds; or the dispute writes the object
  with `==`, not `>=`, so no carrier can be added at the dispute.

### F52 (High, open). A trace whose middle chain is of no leg has no answerable point, and is applied

A statement of kind 2 or 3 is a trace of 88-byte rows. `KpfTStepEnd`/`KpfTStepLiq` (the end, point 0) check only the
**last** row's first chain against the account's root and trust the last row's gain and worth; `KpfTStepRow` and
`KpfTStepPrice` (the middle points) require the shown leg to hash onto the row's chain, and take **neither verdict**
otherwise. So a trace whose rows before the last carry a chain of no real leg, with the last row's chain the true
root and its gain and worth chosen by the claimer, has an end that is answered right and middle points that no leg
can answer. Nobody disputed within the dispute period applies it (`KpfTChangeSettle`): the whole of the chosen
side's margin to a key that is no party's.

- Test: `t1b_finding_f52_a_trace_whose_chain_is_of_no_leg_has_no_wrong_point` (builders, engine-run). A step of a
  liquidation stated by a stranger on an account of three legs, its trace saying B is below its maintenance and the
  fee is B's whole 180 KAS: applied if nobody disputes (B's margin to the stranger). Its end is answered right (the
  disputer's bond lost); of its six points, two are answered right and four (the fabricated rows and their prices)
  have **no answer of either verdict** over every leg of the account (24 answers tried). A point with no answer
  lapses: the account again, both bonds back.
- What is lost and by whom: the counterparty loses its whole margin if it does not dispute within the dispute
  period, and `exec-run` does not watch accounts' statements at all (`RESEARCH-UNIFIED-MARGIN.md` 13.3), so nothing
  disputes it by default. The counterparty's only defence is to dispute one of the unanswerable middle points (not
  the end: the end is right, and disputing it loses the bond); that dispute lapses, restoring the account at the cost
  of a dispute period and both bonds returned — so the attacker can re-state at no cost (a free lock), for ever.
- Fix I would accept: the end's answer is shown the whole trace and holds every row's chain to the one before it (or
  the trace is a tree the end's root commits to), so that a middle row the claimer fabricated makes the end wrong;
  and/or an executor watches account statements.

### F54 (High, open). A statement the settlement applies but the reader cannot read

`KpfTChangeSettle` reads the first output's flag as "the object goes on" when it is `0x00` and as **a payment** for
any other byte. The model and the chain reader (`Outs`, `Outs::read`, which `thin_read` and so `exec-run` use) write
and accept the flags `0` and `1` only. A statement whose first output's flag is `2` (or `3`, `0x7f`, `0x80`, `0xff`)
is one the script applies and the reader does not read: the position is seen as spent, no change object is made, no
executor disputes it, and after the dispute period the position's whole value is paid to the key it names.

- Test: `t1b_finding_f54_a_statement_the_scripts_apply_and_the_reader_does_not_read` (builders, engine-run). "The
  position ends, 100 KAS to a key that is no party's": with the flag `1` the reader reads it as a change and it is
  within the rule at no price (an executor disputes it); with the flag `2` the reader returns it unread (the position
  read as spent, no object made), and the settlement pays the whole value and the bond to that key. Four other flags
  are applied the same way.
- What is lost and by whom: the counterparty's whole position, taken by a statement its tooling never surfaces. A
  party reading the raw transaction could still dispute (the dispute scripts hash the `outs` bytes whole, flag and
  all), so it is tooling-blindness rather than an unanswerable statement — but the default watch does not see it.
- Fix I would accept: the settlement refuses a first flag other than `0` or `1` (it already treats `>= 2` as a
  payment, which `Outs::read` would never have produced); or the reader accepts the same bytes the script does.

### F53 (Medium, open). A point answered right can leave a true statement out of its time

A point answered right puts the statement back **as stated** and restarts its dispute period, but every script asks
the statement's **landing** against the time it prices at: more than the position's lateness after `t0` it can no
longer be settled or disputed, and `KpfTChangeLate` ends it, the claimer's bond to whoever ends it. So a disputer who
disputes a true point late in the dispute period, and whose right answer lands past the lateness, kills an honest
statement: the claimer's take is reverted and its bond is lost to the ender (which the disputer may be).

- Test: `t1b_finding_f53_a_point_answered_right_leaves_the_statement_out_of_its_time` (builders, engine-run). A true
  take on an account (time 6,500, lateness 100), row 1 disputed at 6,555 and answered right at 6,601: the statement
  that stands again is refused by the settlement and by a further dispute, and is ended as late, its bond to the
  disputer. (The settlement had the same statement landed in time is accepted — the control.)
- What is lost and by whom: the claimer its bond and its take; the attacker breaks about even (it loses its own bond
  at the right answer, half to the claimer, and recovers the claimer's whole bond as the ender). It costs the
  attacker the delay and risks the right answer landing in time, so Medium.
- Fix I would accept: a statement that stands again after a right answer keeps its original landing for the lateness
  rule (the restart is of the dispute period only), or the lateness is measured against the dispute, not `t0`.

### F55 (Low, open). A library of one copy answers no take on a two-leg position

`KpfTTake` reads one citation a leg at the places 1 and 2 of `who`, and asks that the two are two inputs — two copies
of `KpfTCite` co-spent. A library made with `--copies 1` (`lib_check` accepts it) holds one copy of each script, so a
disputed take on a position of two legs cannot be answered: it lapses, both bonds back, the taker's legitimate take
reverted. The tool's default is two copies, so default libraries are unaffected.

- Test: `t1b_finding_f55_a_library_of_one_copy_answers_no_take_on_two_legs` (builders, engine-run). A take on a
  two-leg position answered beside two copies of `KpfTCite`; the same with one copy named at both places is refused by
  the script (the two places are two inputs); `lib_check` takes a one-copy genesis.
- Fix I would accept: `lib_check` (or `lib-create`) requires at least two copies of `KpfTCite` (or the answer reads
  one copy twice), so a two-leg take is always answerable.

## F50 and the three round-29 tests (lead 9)

**The code is right and the three tests were stale.** Round 29 recorded F50 (Low) as open: a member signs every grid
time of an exit claim that stands, also after a quorum signed one. Since then the thin-family commit added the fix
the round-29 write-up recommended (`xcmds.rs`: "Once the folder holds a quorum's prices of a time at or after
`since`, the claim is ended by them and no further time is signed"). That fix makes a member stop signing a claim's
times once a quorum of one of them is in the folder it writes to — which the three round-29 tests, written before the
fix, did not expect (they let three members sign into one folder and counted every signature):

- `t1b_finding_f50...` asserted the hole open (member 0 signing four more times after a quorum). Its own message said
  it would fail once fixed; the code closed it. I rewrote it as
  `t1b_fixed_f50_a_member_signs_no_grid_time_once_a_quorum_of_one_stands`: members 0 and 1 give 33,600 a quorum (52 of
  67 KAS), member 2 and then member 0 (run on to 34,900) sign no further time, and the 33,600 quorum ends the claim;
  the key's journal still refuses another price for a time it signed.
- `t1b_fixed_f47...` and `t1b_fixed_f48...` wrote three members into one folder; the quorum-completing member is now
  skipped. I updated their counts (the folder stays at the four signatures members 0 and 1 leave; member 2 adds
  nothing) and their comments. Both still end the claim by a time a quorum signed, which is the fix those tests
  verify.

## A3 and the other source lints (lead 9)

`t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` failed because its table was 28 scripts and the pinned
library is 41. I extended it to all 41 and generalised its per-entry check to the three shapes of the family: the
object operations (go on via `goesOn`), the answers that keep half a bond (go on via `ends`, or inline, with
`+ ORACLE_FEE + bond - half`), and the two services (`KpfTNote`, whose `note` writes two outputs under the id for its
one input; `KpfTCite`, which ties the counts). The lint now reads the four conditions of "going on" from the union of
each entry and the `goesOn`/`ends` it uses, and confirms every object operation is named by `who` and the two services
are not. Counts: 48 entries, 39 `release` across the 41.

The three other source lints (`t1b_r19_...` the places, `t1b_b1_...` the id counts, `t1b_j5_...` the way-in
arguments) model the object-operation shape (one object named by `who`, counted in and out, a payout at
`len - 1 - 2j`). The thin family's services and citation-reading answers are of other shapes, so I left those three
on the 28 operations they were written for (a new const `OBJ_OPS`) and cover the new scripts' own shapes through
`t1b_holds_r30_...` and the F51 to F55 tests. Extending those three lints to the new shapes is not reached.

## What holds (run)

- `t1b_holds_r30_nothing_but_a_note_of_the_attest_is_written_under_the_librarys_id` (builders, engine-run): 32
  mutations refused. A note is written only by `KpfTNote` beside the committee's attest; a note of another price,
  time, market, spread, writer, or an object or a script of the family in the note's place, or a second note, or an
  output more under the id (by any authorizing input), or the change under the id, is refused. A note taken back
  (`KpfNote.reclaim`) pays its writer and writes nothing under the id; one that writes under the id or pays another
  key is refused. A genesis that writes a note (in a script's place or one more) is no library (`lib_check`). A
  citation is refused by the builder for another library or another note, and by the script when told another chain
  block, committee, market, time, price, spread or writer than the note's (the parse of `rest`, lead 3).
- The places of a citation copy at `KpfTTake` and `KpfTStepPrice` (lead 2): the citation stands at place 1/2 of
  `who`, held to `KpfTCite`'s script and read for its price; a coin or another script there is refused (the author's
  `thin_change.rs` shows the swap, and F55 shows two copies are two inputs).

## Read, not run

- F51 on the undisputed (direct) answer: the state output cannot be inflated, so only the dispute step carries it;
  read, not built.
- F52's end trusting the last row's gain and worth: read in `KpfTStepEnd`/`KpfTStepLiq` (`row.slice(8, 16)`,
  `row.slice(16, 24)`), run through the fabricated trace.
- `KpfTCite`'s depth limits (14 and 12), `mergeIdx`, and the lane/subnetwork bytes of `rest`: the builder refuses a
  proof beyond the limits (`Cited::of`); the deeper parse is the author's `cite.rs` and `thin_notes.rs`, read only.
- Lead 4's payout-index collisions between a new script's `len - 1 - 4j` and an old one's `len - 1 - 2j`: I built one
  (a take applied beside a close settled) and it did not collide as I modelled it — a plain settle pays A at output 0
  and B at the last, not from the `2j` slot — so I withdrew it (no finding). Whether a new script woven beside an old
  one that does pay from `len - 1 - 2j` (a late-end, a lapse) collides is not reached.
- Leads 6 (`take_within`/`end_right` rounding and floors), 7 (today's scripts on an account), 8 (the executor's
  `by_own` judgement of a take) beyond what F51 to F55 touch: not reached.

## Not reached

- Anything at a node, a real price source, the testnet; `take` placing an order.
- The reserve (no script), the cover/call/draw (a model).
- A close or an exit's refutation on an account (not built; the author notes an exit claim on an account can be ended
  by nobody — confirmed by absence, not rated this round).
- Extending `t1b_r19`, `t1b_b1`, `t1b_j5` to the thin family's new shapes.

## Findings ledger

| | Severity | State |
|---|---|---|
| F51 | High | open: a sompi more at the dispute refutes a take within the rule |
| F52 | High | open: a trace whose middle chain is of no leg has no answerable point; applied, or a free lock |
| F54 | High | open: a statement the settlement applies but the reader does not read (flag not 0 or 1) |
| F53 | Medium | open: a point answered right can leave a true statement out of its time |
| F55 | Low | open: a library of one copy answers no take on a two-leg position |
| F50 | | fixed in the thin-family commit, verified; the round-29 tests reworked |
| F41 | High | open, with the owner (the coordinator recommends leaving the re-strike as it is) |
| F47, F48, F49 | | fixed in round 29, verified |
| F42 to F46 | | fixed in round 28, verified |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design for a source of one number; closed for one that says low and high |
| F5 | Low | changed by round 23 |
| F7, F8, F9 | Low | by design |
| all others (F1-F4, F6, F10-F12, F14-F31, F33-F38, F40) | | fixed in earlier rounds |

## Counts and commands

My three files: 55 engine (A3 extended to 41, R19/B1/J5 kept on the 28 object-ops), 88 builders (six new: F51 to F55
and `t1b_holds_r30`), 47 command line (F47, F48, F50 reworked for the F50 fix). Five are new `t1b_finding_fNN` tests
(F51 to F55), each engine-run and passing while the hole is open.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli
```

Results: builders 88 passed, engine 55 passed, command line 47 passed; 0 failed in each.

# Round 31

Scope: the author's fixes of F51 to F55 in the thin-family contracts (regenerated), with the model and builder changes
the coordinator listed (`Stated.held`, `Change.held`, `Stated::again()`, `LATE_AGAIN`, `row_judged`,
`wrong_point_of`, `step_row_at`, the flag checks, the second `KpfTCite`). Only my three test files and this file were
changed; no node, no network, no commits, no artifact regeneration. I brought my round-30 tests up to the changed
interfaces (the `held` field, the `prev`/`at` arguments) and verified each fix; no new finding.

**All five fixes hold. The five `t1b_finding_f5x` tests are now `t1b_fixed_f5x`. Two command-line count assertions
were updated for the second `KpfTCite` the F55 fix writes. No new findings (F56+): the leads the coordinator named
are closed.**

## Fix verdicts (each verified, engine-run)

| | Fix | Verdict | Test |
|---|---|---|---|
| F51 | the statement's hash carries `held`; answers count from it, not the live input | holds | `t1b_fixed_f51_held_is_committed_so_more_value_at_the_dispute_does_not_turn_the_verdict` |
| F52 | the row/price answer opens the chain with `prev` (a require); `wrong_point_of` names the row after the bad chain; `at != point => verdict 0` | holds | `t1b_fixed_f52_a_fabricated_trace_has_a_wrong_point_anyone_can_answer` |
| F53 | a point answered right restores the statement with `LATE_AGAIN`: not held to its time again | holds | `t1b_fixed_f53_a_point_answered_right_restores_the_statement_not_held_to_its_time` |
| F54 | `KpfTChange.state` holds the outputs' flags (first 0/1, others 1); `KpfTChangeSettle` asks flag 1 for a payment | holds | `t1b_fixed_f54_a_statement_states_only_flags_0_and_1` |
| F55 | a library writes two `KpfTCite` even at `--copies 1`; `lib_check` refuses fewer than two | holds | `t1b_fixed_f55_a_library_holds_two_cite_copies` |

- **F51**: a take of 54 KAS on a position and a take on an account, both within the rule, disputed with a sompi more
  in the object, are still answered within the rule and applied (the claimer paid its amount, its bond and half the
  disputer's). The committed `held` is used, so extra value at the dispute no longer turns the verdict; the forced
  opposite verdict is refused.
- **F52**: the same fabricated liquidation trace (a middle row's chain of no leg, the fee B's whole margin) now has a
  wrong point `wrong_point_of` names (the row after the fabricated chain, opened by the account's own chain).
  Disputing the (still-right) end does not save it: with `at != point => verdict 0`, that wrong row is answered
  though the end was disputed, the account is restored and the claimer loses its bond. The forced "right" verdict of
  the wrong row is refused.
- **F53**: a true take, one point disputed at 6,555 and answered right at 6,601 (past the 100-DAA lateness of 6,500),
  stands again with `LATE_AGAIN` in its hash; it is then applied by the settlement and cannot be ended as late
  (`KpfTChangeLate` asks the statement's own lateness, which no landing exceeds), and may be disputed again.
- **F54**: a statement with any first-output flag other than 0 or 1 (five tried), or a later payment output with flag
  0, is refused by `KpfTChange.state` (the forged `outs` hash matches the object, but the flag check fails the
  script), so it can no longer be stated unread; the honest flag-1 form is stated and read.
- **F55**: a one-copy library holds two `KpfTCite` and passes `lib_check`; a genesis with only one is refused; the
  two copies answer a take on a two-leg position, and one copy named at both places is still refused (two inputs).

## What holds (the leads, read or run)

1. The opening rule (`blake2b(prev + leg) == row.g`, a require) cannot be turned against an honest row: the only
   `(prev, leg)` opening a real `row.g` is the real one (a second would be a blake2b collision), and for it
   `prev == before.g`, so the verdict is the real one. A point beyond the trace is answered right only with
   `at == point` (its `ok` is 1, so `verdict` must be 1). A right point cannot be answered with verdict 0
   (`ok == verdict`), so `at != point` ends a statement only by a point that is genuinely wrong. The price answer
   with a `prev` that is not the row before's is wrong whatever the price (`if prev != before.g { ok = 0 }`). Run in
   F52; the rest read.
2. `held` is set only by `KpfTChange.state` (to `tx.inputs[j].value`) and carried unchanged through `again()` and
   every step script's `c0`/`c1`; no path re-sets it. The settlement pays the committed `outs` with `>=`, so surplus
   above `held + bonds` goes to whoever builds the settlement (the standard carrier), not the claimer. Read, with F51
   confirming the committed `held` governs the answer.
3. A first statement cannot carry `LATE_AGAIN`: `state` takes `late` from the position's moving part, not an
   argument. A restored `LATE_AGAIN` statement is never ended by `KpfTChangeLate` (for any landing below the lock-time
   threshold, `landed - t0 <= LATE_AGAIN`), by design: it must be settled or refuted. A false statement that stands
   again still has an answerable wrong point (lead 4), so an accomplice cannot keep it alive for free. Read; F53 runs
   the late-end refusal.
4. Every false account statement has a wrong point some honest party can answer: if the last row's chain is not the
   root, the end is wrong; otherwise the row after the last fabricated chain is opened by the account's real chain and
   is wrong there. Reasoned through (the single-leg and first-row edges included) and run for the three-leg case in
   F52.
5. `lib_check` requires two `KpfTCite`; a one-copy library now writes two (42 script outputs, or 43 with a fee pool),
   which the two command-line count assertions were updated to expect.

## Not reached

- A node, a real price source, the testnet; `take` placing an order; an executor watching account statements (still
  not built).
- The reserve, cover, call, draw (a model, no script).
- A close or an exit's refutation on an account (still not built).
- The author's own `review_30_*` random-trace tests (25 pass, per the coordinator); I did not re-run them.

## Findings ledger

| | Severity | State |
|---|---|---|
| F51, F52, F54 | High | fixed in round 31, verified |
| F53 | Medium | fixed in round 31, verified |
| F55 | Low | fixed in round 31, verified |
| F50 | | fixed, verified (round 30) |
| F41 | High | open, with the owner |
| F42 to F49 | | fixed in rounds 28, 29, verified |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design for a source of one number |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

My three files: 55 engine, 88 builders (the five fixed tests and `t1b_holds_r30`), 47 command line (two count
assertions updated for the second `KpfTCite`). No `t1b_finding_` tests remain for the thin family: F51 to F55 are now
`t1b_fixed_` and pass.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests     --jobs 2 --test auditT1_tests
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli       --jobs 2 --test auditT1_cli
```

Results: builders 88 passed, engine 55 passed, command line 47 passed; 0 failed in each.

# Round 32

Scope: the reserve (`docs/ARCHITECTURE-CLEARING.md` 6a, both parts) — the nine new scripts (`KpfTShardOpen`,
`KpfTOpenC`, `KpfTLegAdd`, `KpfTAddC`, `KpfTNoteApply`, `KpfTAcctEnd`, `KpfTShardEnd`, `KpfTShardPrice`,
`KpfTDrawNote`) and the seven of 4a/4b changed since round 31 (`KpfTChange` kinds 4/5 and the 171-byte moving part,
`KpfTChangeSettle` note outputs, `KpfTPoint` kinds 2–5, `KpfTStepRow`/`KpfTStepPrice`/`KpfTStepEnd`/`KpfTStepLiq`).
Read against the model (`reserve.rs`, `thin.rs`) and builders (`thin_build.rs`) and the author's tests
(`thin_reserve.rs`, `thin_draw.rs`). Only my four files changed; no node, no commits, nothing under `kpf-out`
touched. The author's mechanical edit in my builders file (genesis funded with 3,000 KAS) is kept.

**No new finding. The three closures the author listed hold as model↔script, the round-31 answer machinery extends
correctly to kinds 4 and 5, and the objects of 171 bytes are out of reach of the 138-byte scripts. The one failing
test (`t1b_a3_...`) is mended: the table is extended to all fifty scripts. What I did not reach is listed plainly;
the strongest unexamined risk is integer overflow in the draw's requirement at testnet scale.**

## The one test mended (lead 9 equivalent)

`t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` failed because its table held 41 scripts and the
library is now 50. Extended `SCRIPTS` with the nine new names and the counts to `(57 entries, 48 releases)`. The nine
all fit the existing shape the A3 lint already models (each a `who`-based object operation with `goesOn` or `ends`,
each a `release` of the six lines, each `ORACLE_FEE` twice), so no change to the lint body was needed; it passes.

## The three closures (checked, hold)

1. **An owed note is paid only to the shard owner's own account.** `KpfTNoteApply` kind 4 requires
   `head[0..33] == push(shard owner)`; `KpfTStepLiq` writes an owed note with `head[0..33] = push(the account's B
   key)` and pays the account's A. The model agrees: `Shard::applied` for `NOTE_OWED` refuses unless
   `n.head.market == self.owner`, and `Head`/`SNote::owed` put B's key there. A covered account whose B is not the
   owner cannot be opened without the owner's own signature (`KpfTOpenC` signs both sides), so a stranger cannot
   mint an owed note the shard pays. Read, not newly tested (the author's `thin_reserve.rs` exercises the honest
   path and one forged-note refusal).
2. **A trace beginning with a seed-chain row is answered wrong with no leg.** `KpfTStepRow` now sets `ok = 0` when
   `row.g == 32 zero bytes` before opening the chain, so the seed row is a genuine wrong point. Verified as part of
   round 31's F52 (the same machinery), now with the `prev`/`wrong_point_of`/`at != point` rule covering kinds 3 and
   4 too.
3. **Only a draw is stated on a shard, and no take/leave on one.** `KpfTChange.state` detects a shard as the
   171-byte object whose B key is 32 zero bytes (`shard == 1`), requires kind 5 for it, and requires `shard == 0`
   for kinds 2/3/4. A shard's B key is zero only because `KpfTShardOpen` wrote it so; a covered account always has a
   real B key (signed), so `shard == 0` for accounts. Kind 5 is confined to real shards.

## A fourth door looked for, not found

Attacked, each held:
- **A note to the wrong shard / applied twice.** A note binds the shard's `sid` in its hash (`mv.slice(138,171)`),
  and `KpfTNoteApply` requires the note's covenant id to equal the shard's; `sid` is the hash of the anchor outpoint,
  unique. A note is a UTXO spent on application (kind 4 goes on only with the unpaid remainder, which shrinks), so it
  cannot be applied twice for more than its amount.
- **A leg-note applied to another market's row.** `KpfTNoteApply` kinds 1/2 require the row's 84-byte market head to
  equal the note's (an empty row is claimed only by a non-empty head). A join/release of market X cannot touch
  market Y's row.
- **The leaving leg's market/units forged in the release note.** `KpfTStepRow` ties them at the `skip` row
  (`lg.slice(0,84) == said.slice(24,108)` and the signed units), so a release note of a market or size other than the
  leg that leaves has a wrong point.
- **Cross-kind answers.** Each answer script commits its kind in the statement hash (`KpfTStepEnd` hardcodes
  `i9(2)`, `KpfTStepLiq` requires kind 3 or 4, `KpfTShardEnd`/`KpfTShardPrice` `i9(5)`), so a draw cannot be answered
  by a take's script or the reverse.
- **A deposit note (kind 3) has no owner check**, but it only adds value the note itself must carry (the engine's
  balance forces it), so it cannot drain a shard; `KpfTDrawNote` also refuses it as a draw-ending note.
- **A covered account or shard spent by a 138-byte script.** The old position scripts take `byte[138] moving`; a
  171-byte object's state hash is `blake2b(push(fixed) + moving[171])`, which no 138-byte reconstruction matches, so
  they cannot open these objects.
- **Value across note outputs.** A deposit note is written holding `amount + NOTE_MIN` (`KpfTStepEnd`, `KpfTAcctEnd`)
  and credits the shard `amount` on apply; an owed note holds `NOTE_MIN` and is paid from the shard, not the note.
  The 1 KAS carrier (`SHARD_CARRIER`) is beside what the shard holds and no note pays it out.

## What I did not reach (said plainly)

- **Integer overflow in the draw's requirement** (`KpfTShardEnd`/`reserve.rs::requirement`): `worth = net * price`
  over four rows, with `net` up to `MARGIN_MAX` (~1.15e18) and `price` up to a market's limit (1e14). A large row
  could overflow i64 and wrap the requirement small, letting an owner over-draw. I did not determine whether
  silverscript traps or wraps on overflow, nor build a case; this is the strongest unexamined risk and should be
  checked (does the engine trap `*`/`+` overflow, and is `net`/`gross` bounded below the overflow threshold?).
- **The full note-ordering state space**: applying a release before its join, interleavings across four rows, and
  whether any order lets the shard's rows misstate the net so the requirement is understated (I argued `abs()` tends
  to overstate, not a proof).
- **An exhaustive model↔builder↔script three-way diff** of every reserve function (rounding in `requirement`,
  `draw_max`, the owed split). I read the arithmetic but did not random-test it as rounds 30/31 did for takes.
- **The draw end's requirement when a shard row's market is present but `net`/`gross` were driven negative** by a
  misordered release; and `KpfTShardPrice` with two rows of the same market.
- **`KpfTAcctEnd` and `KpfTAddC` edge cases** (a legless account A ends while B's margin is zero; adding to a
  shard's absent B side) beyond reading.
- **Anything at a node or testnet; the tools** (`exec-run` does not watch covered accounts or shards).

## Findings ledger (unchanged this round)

| | Severity | State |
|---|---|---|
| reserve (6a) | — | reviewed, no new finding; three closures hold; overflow in the draw requirement not reached |
| F41 | High | open, with the owner |
| F50–F55 | — | fixed in rounds 29–31, verified |
| F42–F49 | — | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | — | fixed in earlier rounds |

## Counts and commands

My files this round: one line table change in `auditT1_tests.rs` (the fifty scripts). No new `t1b_finding_` or
`t1b_holds_` tests were added (no new hole found and the closures are the author's own, exercised by `thin_reserve.rs`
and `thin_draw.rs`, which I did not modify).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests     --jobs 2 --test auditT1_tests      # 55 passed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol  --jobs 2 --test auditT1_builders   # 88 passed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli        --jobs 2 --test auditT1_cli        # 47 passed
```

Results: engine 55 passed, builders 88 passed, command line 47 passed; 0 failed in each.

# Round 33

Scope: options as legs (ARCHITECTURE 4b "An option is a leg", 6a, 10), the 112-byte trace row, and the scripts not
reviewed since round 32 (`KpfTStepRow`/`KpfTStepPrice`/`KpfTStepEnd`/`KpfTStepLiq` with the five-worth row and
options, `KpfTChange` rows of 112, `KpfTLegAdd` options and the option-market note, `KpfTShardPrice` the `under`
argument, `KpfTHand`, `KpfTLegMove`). Read against the model (`thin.rs`, `reserve.rs`) and builders, and the
author's `thin_option.rs`. Only my two test files changed; no contracts/src touched, no commit, nothing under
`kpf-out`. One finding (Medium).

**F56 (Medium): a leg whose worth at a price within its market's limit overflows i64 enters a covered account,
because `KpfTLegAdd` and `Account::check` dropped the size bound the position path (`XLeg::check`) enforces. The
account's statements then cannot be computed. The A3 table is extended to the 52 scripts; the F52 test still tests
what I meant.**

## Finding

### F56 (Medium, open). A leg whose worth overflows i64 enters a covered account, and locks it

Every step script values a leg's row with `quoteAt(size, scale, price)`; the model's `q` panics and the on-chain
`quoteAt` overflows when `size/scale * price` does not fit 63 bits. The position path refuses such a leg:
`XLeg::check` requires `size/scale <= (i64::MAX/8)/price_max - 1`. The covered-account path dropped it —
`KpfTLegAdd` asks only `size <= MARGIN_MAX` (~1.15e18) and `entry <= price_max`, and `Account::check` asks only
`size > 0`. So a leg whose worth at the market's price limit overflows i64 enters a covered account. No take, leave
or liquidation of that account can then be built (the model's `q` panics) nor answered (`quoteAt` overflows on the
chain), and a one-leg account cannot be ended (`KpfTAcctEnd` needs no leg): the margins and the cover are locked.

- Test: `t1b_finding_f56_a_leg_whose_worth_overflows_enters_a_covered_account` (builders, engine-run). A leg of size
  `MARGIN_MAX` on a market of limit `1e14`: `XLeg`'s bound rejects it and `payoff::q_checked` returns `None` at the
  limit (it overflows), yet `Account::check` is `Ok` and the engine accepts `KpfTLegAdd` of it into a covered
  account.
- What is lost and by whom: both parties sign the leg (`KpfTLegAdd` needs both), so it is a consent-gated footgun
  rather than a one-sided theft; but a clearing party B whose tool co-signs a leg A proposes (the builder does no
  size sanity-check beyond `MARGIN_MAX`) locks its own and A's margin and the account's shard cover, with no path
  out. The position path treats the identical leg as out of range, so this is a validation inconsistency, not a
  design choice.
- Not resolved (decides lock vs worse): whether the silverscript engine traps or wraps on the i64 overflow in
  `quoteAt`. Traps -> the row is unanswerable, honest statements lapse (both bonds back) and the account is locked
  (availability). Wraps -> a hand-built statement could pass with wrong margins (a step script's floor/fee computed
  on a wrapped worth), which would be a theft, not a lock. I did not reach a hand-built answer to settle this; the
  same question was open from round 32's requirement arithmetic.
- Fix I would accept: `KpfTLegAdd` (and `Account::check`, `leg_added`) apply `XLeg::check`'s size bound
  (`size/scale <= (i64::MAX/8)/price_max - 1`), so no leg whose worth overflows can enter.

## What held (read, or via the author's tests)

- **The option row's five worths.** `leg_adds` (model) and `KpfTStepRow` (script) agree on the reading: a futures
  leg adds its worth to both sides' worth and nothing to what they keep; an option adds to the gain `va - ve`
  (negated where A wrote it), its underlying's worth to the writer's worth, and what it is worth exercised to the
  holder's `e`. Checked line by line against `thin_option.rs`'s eight tests (which include 3,000- and 4,000-account
  random model-vs-script runs).
- **The kind bits.** A direction `>= 3` whose last three bits are 3–6 is an option; bits 0/1/2/7, or a direction
  below 3, or a negative one, are a futures direction (long only for 1, short otherwise) — `option_of`, the script's
  `if dir >= 3 { if dir%8 in 3..6 }`, and `leg_kind_ok` agree, and `leg_kind_ok`/`KpfTLegAdd` reject anything that is
  neither a clean long/short nor an option with a strike in `1..=price_max`. A bad-bits or negative leg cannot
  enter an account, so no trace carries one.
- **The writer cannot end its own option.** `KpfTStepRow` at `i == skip` requires `side == holder` (the holder is
  side A for an odd kind, B for an even one), so a kind-4 leave or a kind-3 liquidation stated by the side that
  wrote the option makes the leaving row wrong — an answerable wrong point. The author's
  `an_option_leaves_by_its_holder_s_side_alone` runs it.
- **The seed-chain row (F52 class) for options.** `row_judged_of` returns `Some(false)` for a row whose chain is 32
  zero bytes, and `KpfTStepRow` sets `ok = 0` before opening it: a fabricated leading row is a wrong point whatever
  leg is shown. My `t1b_fixed_f52` (its `worth` split into `wa`/`wb` by the author's mechanical edit) still
  exercises this on a liquidation trace; I confirmed it.
- **The option note head.** `KpfTLegAdd` and `KpfTStepRow` name an option's shard row `blake2b(push8(13) +
  push(market))` with units `+size` either way, and `KpfTShardPrice`'s new `under` cites the underlying market where
  the tagged hash is the row's: options cannot net against futures or one another, and a futures row cannot pass as
  an options row (the head differs by the tag). Read; the author's `thin_option.rs`/`thin_reserve.rs` exercise it.

## What I did not reach

- **The engine's overflow behaviour (trap vs wrap)** in `quoteAt` — the open question that sets F56's true
  severity, and the same question left from round 32's draw requirement. Reaching it needs a hand-built step answer,
  which I did not construct.
- **An exhaustive model-vs-script verdict comparison** over random option accounts (the author runs 3,000/4,000; I
  read the arithmetic and relied on those rather than re-running a comparison in my files).
- **`KpfTHand` and `KpfTLegMove`** beyond the A3 structural check (both are `who`-based, one entry and a release,
  the going-on shape): the hand-over's margin-vs-ask rule and the three-signature leg move between two covered
  accounts of one clearing party were read only, not attacked with a test.
- **`KpfTNoteApply` emptying a row whose net and gross return to 0**, the `said` head-form offsets (24/57/90/99) in
  `KpfTChange` for kinds 2–4, and the commands `acmds.rs`/`aobj.rs` (`clearing_cli.rs`): not examined.
- Anything at a node or testnet.

## Findings ledger

| | Severity | State |
|---|---|---|
| F56 | Medium | open: a leg whose worth overflows i64 enters a covered account and locks it |
| F41 | High | open, with the owner |
| F50–F55 | — | fixed in rounds 29–31, verified |
| F42–F49 | — | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | — | fixed in earlier rounds |

## Counts and commands

My files: the A3 table in `auditT1_tests.rs` now lists the 52 scripts (`(59 entries, 50 releases)`), and
`auditT1_builders.rs` gains `t1b_finding_f56_...`. The F52 test, which the author edited mechanically, was read and
still tests the fabricated-chain wrong point.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests     --jobs 2 --test auditT1_tests      # 55 passed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol  --jobs 2 --test auditT1_builders   # 89 passed
```

Results: engine 55 passed, builders 89 passed; 0 failed in each. (The command-line suite I did not change or run
this round.)

# Round 34

Scope: the fix of F56, and what round 33 read and did not attack: options at the row and at the end
(`KpfTStepRow`, `KpfTStepEnd`, `KpfTStepLiq`), `KpfTShardPrice`'s `under`, `KpfTHand`, `KpfTLegMove`,
`KpfTNoteApply` emptying a row, the head form in `KpfTChange`. Everything below is engine-run (`verify::verify`)
unless it says "read". Only `crates/kpf-protocol/tests/auditT1_builders.rs` and this file changed; no contracts, no
`src`, no commit, nothing under `kpf-out`.

**F56 is fixed and verified. Three new findings: F57 (Medium) and F59 (Medium), F58 (Low). The model and the scripts
gave the same verdict on every one of 3,017 rows and ends I put to the engine.**

## Findings

### F59 (Medium, open). A futures leg named as the options' row nets against the options

A shard counts the options on a market in a row of their own, named `blake2b(push8(13) + push(market))`, with the
size whichever way an option stands: "options never net against futures or one another". A row's name is the first
84 bytes of a leg, and a futures leg's market is any 32 bytes its two parties sign: `KpfTLegAdd` asks nothing of a
market's name. A futures leg whose market IS that hash enters a covered account, its note carries the options'
row's own head, and its units are signed as a futures leg's (long: the size below zero). So it sets the options'
net against itself.

- Test: `t1b_finding_f59_a_futures_leg_named_as_the_options_row_nets_against_the_options`. A shard of 50 KAS whose
  owner wrote a call on 10 units (and is short 4 units of another market) is asked 7.7385 KAS (the options' row:
  net 10, gross 10). A long "futures" leg of 10 units
  on the market named by the hash enters the same account by both signatures (`Account::check` and `leg_kind_ok`
  take it); its join is applied to the options' row: net 0, gross 20. The shard is asked 3.216 KAS. Its owner
  states a draw of 46.784 KAS where the rule gave 42.2615: the draw has no wrong point by the model, its end is
  answered right, and it is applied.
- Who and what: the owner needs nobody. Side A of such an account is a key of its own, and a leg enters by the two
  signatures of that account. It lowers its own shard's requirement where the design says it cannot, and draws the
  difference out of what covers its users. (Dressing the net of futures rows with an account of one's own is in the
  research, 9a; for options the design ruled it out, and the claim of round 33 was "nobody can make a futures row
  pass as an options row".) The made leg costs its owner nothing: its price is never asked (no committee signs for
  a market of that name; a dispute of that price lapses), which matters only inside its own account.
- Fix I would accept: an options row and a futures row cannot share a name. For instance the note of a futures leg
  is named by a tagged hash too (another tag), so that no leg's own bytes are a row's name; or `KpfTLegAdd` writes a
  word of the leg's kind into the note's head.

### F57 (Medium, open). The holder of an option alone is liquidated where the pot is short

The design: "A holder that keeps nothing else in the account is below no maintenance at any price, and nobody
liquidates it." A side's `lo` is the maintenance of the worth it is asked and what it keeps whole (`e`: what its
options are worth exercised). The margin it is compared with is what the side is owed, capped at the pot. Where the
option has come to be worth more than the pot holds (its writer cannot pay), the holder's capped margin is below
what it keeps whole, and a step of ITS liquidation is within the rule: the option leaves, and whoever stated the
step has the liquidation fee of the underlying's worth out of the holder's margin.

- Test: `t1b_finding_f57_a_holder_of_an_option_alone_is_liquidated_where_the_pot_is_short`. A holds a call on 10
  units struck and entered at 10 and nothing else (margin 5 KAS; the writer's 20). At 11 no step of A's liquidation
  is the rule's. At 14 the option is worth 40 and the pot 25: a stranger's step is the rule's (`wrong_point_of`
  gives none; the end, the row and the price are answered right, and not wrong), it is applied, and the stranger has
  0.7 KAS of A's margin. By A's own statement (kind 4) the same leaving costs no fee. The same on a covered account
  (fee 0.7 KAS, 20 KAS owed by the shard).
- What is lost and by whom: the holder, the liquidation rate of its option's underlying (50 basis points here), to
  whoever states first, at the moment its counterparty is short. The writer's own second key is such a stranger:
  the side that cannot pay takes a fee from the side it owes. The holder avoids it only by exercising first.
- Fix I would accept: a side is below only by what it is asked (the maintenance of its worth), and what it keeps
  whole is compared with its margin before the cap; or a step of a liquidation is refused where the side's worth
  asked is zero.

### F58 (Low, open). The sums of many legs make a row nobody answers, at the committee's own prices

After F56 each leg's worth fits; the sums over the legs are not bounded, and the author's claim is that a false
statement still has an answerable wrong point: "the first row whose sum is false is computed from a true row before
it, or the price points are wrong". That holds while the true sums fit 64 bits. Where they do not, the first row
whose TRUE sum would not fit is a row the script cannot compute whatever it says (the addition fails the script),
and the claimer writes there what it likes.

- Test: `t1b_finding_f58_the_sums_of_many_legs_make_a_row_nobody_answers_at_true_prices`. Seventeen legs, each the
  largest a leg's bound allows on a market whose limit is twice the price; `Account::check` takes the account.
  Stated at the committee's price: rows 1, 2 and 16 are answered right, the end right (by its numbers: no worth
  asked, a gain of B's whole margin), the price of row 17 right, and row 17 by neither verdict. Its dispute lapses
  with both bonds back; undisputed, the statement pays A the pot.
- Bound, and why Low: both parties signed seventeen legs of 5.76 x 10^9 KAS each (9.8 x 10^10 KAS in all); the
  author named this gap. What the test adds is that the claim made for it does not hold there: such an account has
  no statement anybody can answer, true or false. (It also shows the engine's integers are checked: the row is
  refused both ways, and the same helper takes row 16 as right.)
- Fix I would accept: the model and `KpfTLegAdd` bound the sum (a count of legs with the per-leg bound divided by
  it, or the running worth carried in the account's state).

## Fixed and verified

**F56.** `t1b_fixed_f56_a_leg_whose_worth_overflows_does_not_enter_a_covered_account`: the leg of round 33 (size
`MARGIN_MAX`) is refused by `Account::check`, by the builder, and, built past the model (`leg_add_as`), by
`KpfTLegAdd`. The largest size the bound allows (1,152,899,999,999 on the test market) enters and one more does not,
for a long and a short leg, a written call struck at the limit and a held put struck at 1; a scale beyond
`SCALE_MAX` is refused.

## What held under attack (all engine-run)

| Item | Test | What was put to the engine |
|---|---|---|
| 1. A row's answer | `t1b_r34_a_rows_answer_is_the_models_for_every_kind_of_leg` | 27 legs: long, short, the four kinds of option with the entry above and below the strike, a strike of 0 and one above the limit, and directions' places that are no kind (0, below zero, the bits 0, 1, 2, 7 above a strike, 7, 8). For each: a row that stays and one that leaves, by each side, in a statement of each kind; what is said of the leaving leg as the model makes it, of another market, of other units; each of the six numbers of the row a sompi up and down, and the second chain another. 1,755 rows: the engine takes the model's verdict (314 right, 1,441 wrong) and refuses the other; a chain or a leg that does not open the row is taken with neither |
| 2. An end's answer | `t1b_r34_an_ends_answer_is_the_models_for_takes_and_legs_that_leave` | 36 random accounts of one to five legs (a third covered) and 6 of options held alone. Takes of the most, a sompi more, the most and what the side's options were counted at, nothing, less than nothing, by the side's key and by a stranger (a deposit on a covered account); the last row with `wa`, `wb`, `ea`, `eb` below zero; every leg leaving by kind 3 and kind 4, of each side, written as the rule would write it whether or not it allows it, with the fee a sompi more and less, and kind 4 by a stranger. 1,262 ends: the model's verdict each time (321 right, 941 wrong), 284 leavings the rule allows and 60 it does not. In numbers: a side that takes the most keeps what its options were counted at; a side all of whose legs are options it holds (struck where they were entered, the other party able to pay) is liquidated at none of 504 prices and legs |
| 3. `under` | in the F59 test | the options' row is answered right only with its underlying named (and not with no market, another market, the row's own name, the futures row's market, or another market's citation); the futures row has one answer whatever `under` is; a row with no market is right with no citation whatever `under` is. 24 answers, one verdict a row |
| 4. `KpfTHand` | `t1b_r34_a_side_is_handed_over_by_the_two_and_holds_what_was_asked` | 9 refused: the account a sompi short; who leaves, or who enters, not signing (another key's signature in its place, side B's too); a key named that did not sign; a sompi of margin taken out; a sompi less than side B asked; a shard handed over; an account of 138 bytes |
| 5. `KpfTLegMove` | `t1b_r34_a_leg_moves_between_two_accounts_of_one_clearing_party_by_three_signatures` | a leg moved from each place, its entry as it was. 23 refused: each of the three signatures by each other key; an account of another shard, of another side B; a chain that does not open the leg, another leg; legs after that are not whole, none, seven; each of the four margins below zero; a sompi less in either account; the two written in each other's place |
| 6. Notes and forms | `t1b_r34_a_row_emptied_out_of_order_is_guarded_and_what_is_stated_is_read` | a leaving applied before its entering empties the row while an equal leg stands (the order is the applier's); a draw of everything stated then is ended by the entering's note (`KpfTDrawNote`), and once it is applied the row is the market's again. 57 statements of kinds 2 to 4 whose pushes at 24, 57, 90, 99 are others, or whose length is no head and rows, are not stated; 40 statements of random bytes in the form are stated and read back by `ATake::read` as they are |

Three things the scripts take, said so that nobody takes them for rules:
- A side handed over to the same key is taken (it is a margin added). What who leaves is paid is no rule of
  `KpfTHand`: with a sompi less and both signatures again it is taken; the two sign the whole transaction.
- A leg's move may take margin out of either account (5 KAS in the test): the three signatures are every party of
  both.
- A row emptied out of order (item 6) shows a requirement of nothing until the pending note is applied; what guards
  it is `KpfTDrawNote`, so a watcher that holds the unapplied note is what ends a draw stated in that window.

## Read, not run

- `KpfTShardPrice` with a citation of a market whose own name is the tagged hash: a second right answer for an
  options row would need a committee to sign for that name; no committee's market is one.
- An uncovered account (138 bytes) is opened by two signatures over its hash: no script sees its legs, so the bound
  of F56 is the tool's there (`Account::check`), not a script's.

## Not reached

- The commands (`acmds.rs`, `aobj.rs`, `clearing_cli.rs`) and anything at a node.
- `KpfTShardEnd` with a row dressed as in F59 across two accounts (the test uses one account, whose two parties
  sign; the owner's own second account is the same two signatures).
- F57 on an account with other legs beside the option (the cap applies the same way; not run).
- A model-against-script comparison of the notes' application over random orders of many notes and four rows.

## Findings ledger

| | Severity | State |
|---|---|---|
| F57 | Medium | open: the holder of an option alone is liquidated, for a fee, where the pot is short |
| F59 | Medium | open: a futures leg named as the options' row nets against the options in a shard |
| F58 | Low | open: the sums of many legs make a row nobody answers at true prices |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 97 tests pass (8 new or changed this round: the F56 test turned into its closure's, three
findings F57, F58, F59, and four `t1b_r34_...`). A finding's test passes while the hole is open and says "is closed"
where it would fail. `auditT1_tests.rs`: 55 pass, unchanged this round. The command-line file was neither changed
nor run.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 97 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
```

# Round 35

Scope: the changes for F57, F58 and F59 (`KpfTStepLiq`, `KpfTLegAdd`, `KpfTStepRow`, `KpfTShardPrice`;
`thin.rs` `stepped` and `Account::check`, `reserve.rs` `Head::of`, `futures_market`, `option_market`,
`thin_build.rs` `shard_price`), each closure verified and each change attacked. Everything below is engine-run
(`verify::verify`) unless it says "read". Only `crates/kpf-protocol/tests/auditT1_builders.rs` and this file
changed; no contracts, no `src`, no commit, nothing under `kpf-out`.

**F57 and F59 are closed and verified. F58 is closed where the model writes a statement and is not closed where
the model adds or moves a leg (`leg_added` and `leg_moved` do not go through `check`); in the scripts it is open by
decision. No new finding. The three finding tests of round 34 are now tests of the closures; the 2,979 rows and
ends of round 34's two comparisons still get the model's verdict from the changed scripts.**

## Findings

None new this round.

### F58 (Low): what is closed and what is not

`t1b_open_f58_the_tool_states_on_no_account_whose_sums_do_not_fit_and_still_adds_its_legs`

- Closed: `Account::check` refuses an account whose legs together are worth more at their markets' limits than
  64 bits hold. With the largest leg the bound of F56 allows, eight are an account and nine, sixteen, seventeen and
  sixty-four are not. On the account of seventeen the model writes no take (`take_by`), no leaving and no step of
  a liquidation (`step_any`).
- Not closed, and asked for by the coordinator: `leg_added` does **not** go through `check`. It holds the leg that
  enters to its own bound and returns the account of nine legs, which `check` then refuses. `leg_add` builds from
  it and `KpfTLegAdd` takes the transaction. `leg_moved` is the same: it moves a leg into the account of eight and
  returns an account that `check` refuses. So the tool still makes the account it then states on nothing of. A
  call of `check` on what `leg_added` and `leg_moved` return (both accounts, for a move) closes it in the tool.
- Open by decision (the scripts have no count of legs): a statement signed past the tool on the account of
  seventeen has its seventeenth row answered by neither verdict at the committee's own prices, the sixteenth
  right. Unchanged from round 34. What it costs is as it was: a dispute of that row lapses, with both bonds back
  and the account as it stood; it is the account of the two who signed those legs.

The test passes while `leg_added` adds the ninth leg and says "F58 is closed where a leg is added" where it would
fail; the same for the move.

## Fixed and verified

**F57.** `t1b_fixed_f57_a_holder_of_an_option_alone_is_not_liquidated_for_what_the_pot_did_not_pay`. The step of
round 34 (an option worth 40 KAS, a pot of 25) is refused by the model and, written as the rule would write it,
its end is answered wrong and not right at the engine: for A and for B as the holder, a call and a put, on an
account that is covered and on one that is not (8 cases), and on 12 random accounts of a holder alone. By its own
statement the holder exercises at no fee and that end is answered right.

**F59.** `t1b_fixed_f59_a_row_is_named_by_the_kind_and_the_market_of_its_legs`. The leg of round 34 (a futures leg
whose market's name is the old name of the options' row) now has the head `blake2b(push8(14) + push(that name))`,
which is not the options' row's. Its note is not applied to the options' row nor to the futures row, by the model
and by `KpfTNoteApply` with the row told (2 refused); it takes a row of its own, the options' row stays at net 10
and gross 10, and the shard is asked no less with it at any price stated for it.

**F58.** In part, above.

## What held under attack (all engine-run)

| Attack | Test | What was put to the engine |
|---|---|---|
| F57: a holder alone at any price | the F57 test | 500 accounts of one to three options held by one side (A, B; calls, puts; any strike and any entry; half covered), the holder's margin at least what its options were counted at when they entered, the other party's anything: liquidated at none of 19,680 prices and legs (the model), and 12 of them at the engine. The condition is exact: see the holder that was paid, below |
| F57: other legs beside the option | the F57 test | 1,200 sides of 600 random accounts of one to five legs (futures and the four kinds of option, a third covered). 696 are liquidated and none that was not before the change. 83 are no longer liquidated: each is a side the pot did not pay in full, stands at the maintenance of its worth, and counted in full (its margin and what the pot could not pay it) stands at all it is asked. All 560 below the maintenance of their worth alone are liquidated whatever they keep whole. 120 steps of sides the pot did not pay, liquidated or not, are answered at the engine as the model says |
| F57: a writer | the F57 test | 719 sides that hold no option are judged exactly as before the change (the reduction is of what a side keeps whole, and stops at zero) |
| F57: the holder paid through a note of what is owed | the F57 test | below |
| F59: a note that names another row when a leg enters | the F59 test | 8 built past the model (`leg_add_as`) and refused by `KpfTLegAdd`: a futures leg noted in its market's options' row, in another market's futures row, under its market's bare name; the leg of round 34 in the options' row; an option in its market's futures row (with its own units and with a futures leg's), in another market's options' row; the right row with the units the other way |
| F59: a note that names another row when a leg leaves | `t1b_r35_a_leaving_legs_note_names_the_legs_own_row_or_its_row_is_answered_wrong` | for a futures leg and for an option leaving a covered account, 20 statements that say of the leg the other kind's row of its market, the market's bare name, another market's row, another committee, scale or limit, no market, the units the other way, none, one more. Each has an end that is answered right (`KpfTStepLiq` makes the note of what is said) and the row of the leg answered wrong and not right (`KpfTStepRow`), the model agreeing on both. Units of the least 64-bit number are read as none: wrong at the end and at the row |
| F59: the end right, the note wrong, the row without an answer | same test | the row of the leaving leg given a chain of the claimer's making, so that nobody opens it, the note naming the other kind's row. Three ways of writing the chains: the last leg leaves (the end is wrong: the last chain is not the account's); the first leaves and the second row follows the made chain (the second row is right, the end wrong); the first leaves and the second row carries the account's own chain (the end is right, the second row is wrong: the chain that opens it is not the row before's). In each exactly one other point is wrong and it is answered so. With a chain that opens, the row itself is wrong (the 20 above). I found no path where every wrong point is without an answer |
| F59: a row two markets price | the F59 test | the options' row and the futures row are each answered right with their own market's citation and not wrong; 20 answers with `under` put as no market, the other market, the market's tagged names of either kind, a third market, are refused with both verdicts, and the builder takes no other market's citation for a row. A second market for a row is a second preimage of its name |
| F59: a row nobody prices | the F59 test | the made row (its name the hash of a name no committee signs for): 8 answers of its price, with either market's citation and four values of `under`, are refused with both verdicts. Its dispute lapses: output 0 is the shard again holding what it held, both bonds back, nothing drawn |
| F59: what the owner gets from that row | the F59 test | the requirement sums each row's own net and gross, so a row only adds: at four prices stated for the made row the shard is asked no less than without it. At the least price (1) it is asked 3 sompi more than without the row, and the end of that draw is answered right and not wrong. So the owner draws as if the row were not there while nobody disputes that row's price, and where somebody does the draw lapses at no cost to either. The other rows keep their answers (a false price in one of them is answered wrong whichever point was disputed) |

**The holder that was paid (the coordinator's claim: verified).** A covered account, A with 5 KAS, B with 20; A holds
a call of 10 units struck and entered at 10 and is long 10 units of another market at 10. At 14 for both, A states
that the futures leg leaves: it is owed 85, the pot holds 25, the note of what is owed is of 60, and the account
that is left has A's margin 25, B's 0 and the option counted at 14. That end is answered right. Then A's margin
is the pot and what it keeps whole (40) was paid out: it is below at 14 (fee 0.7 KAS), at 16 (0.8), at 30 (1.5),
at 11 and at a sompi above the strike (no fee: its margin is gone), that is wherever the option is worth anything.
A step of its liquidation at 14 exercises the option and is answered right; A's own statement does it at no fee.
At the strike nothing is stepped and A's margin lacks 15 of the 40 it was paid for the option. So the claim
holds: A is below at once and a step exercises the option, and what the clearing party bears of the time between
is its own shortfall in the account, at most what the options that stay were counted at less the pot (15 KAS
here), not a rate of their worth. Two things to know about it. A starts it alone (the leaving is A's statement)
and the clearing party's answer is the step, which cannot be stated before A's statement is settled. And it is
the one way I found for a holder alone to be below: the 19,680 cases above say a holder whose margin covers what
its options were counted at is never below, and this is the state where it does not.
(Read, not run: the same leaving was allowed before the change, by the other clause, B being below with nothing in
the pot. The change did not open it.)

## The three things round 34 said the scripts take

I agree that all three are by the rule and not findings.

- A side handed over to the same key: who leaves and who enters are one signer, the legs and entries stay, the
  margin can only grow and must hold what side B asked. It is a margin added. Nothing of side B's or of the
  shard's changes without them.
- What who leaves is paid in `KpfTHand`: it is between the two who sign, each signs the whole transaction and
  sees the output. A script that held it would need the price said somewhere both had agreed, which is the
  signature again.
- Margin leaving during a leg's move: the three signatures are every party of both accounts, and the clearing
  party is also the owner of the shard that stands behind what either account cannot pay. The rows of the shard
  do not change by a move, so no other account's cover does.

## Read, not run

- `KpfTLegAdd`, `KpfTStepRow` and `KpfTShardPrice` build a row's name the same way (`blake2b(i9(tag) +
  push(market))`, the tag by the leg's kind), and so does `Head::of`. `KpfTLegAdd` is the only script that puts a
  leg into a covered account; `KpfTLegMove` moves one between two accounts of one shard and writes no note.
- `KpfTStepPrice` cites a leg's own market (an account's rows carry legs, not names). Only a shard's rows are
  named by the hash.
- For a row with no market `under` is not read by `KpfTShardPrice`: an answer is taken with anything there. It
  changes nothing the script holds.
- `KpfTStepEnd` (a take) is unchanged: what a side keeps whole is not reduced there, so where the pot is short a
  holder takes nothing before it exercises. That is the careful side.

## Not reached

- The commands (`acmds.rs`, `aobj.rs`, `clearing_cli.rs`) and anything at a node: whether a command adds the ninth
  leg of F58 was not run. The command-line audit file ran unchanged (47 pass).
- The holder that was paid, as one sequence of objects: the account after the first step is built from the
  model's `after`, not from the settled statement's output.
- A model-against-script comparison of the notes' application over random orders of many notes and four rows
  (round 34's item, still open).
- A draw from a shard with a row nobody prices where the other rows' prices are also disputed in the same window
  by several parties.

## Findings ledger

| | Severity | State |
|---|---|---|
| F58 | Low | closed where the model writes a statement; not where it adds or moves a leg (`leg_added`, `leg_moved`); open in the scripts by decision |
| F57 | | fixed in round 35, verified |
| F59 | | fixed in round 35, verified |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 98 tests pass (4 new or changed this round: the F57 and F59 tests turned into their
closures', the F58 test into what is closed and what is not, and `t1b_r35_...`; two helpers, `step_said35` and
`step_as_if35`). The F58 test passes while the model adds the leg and says "is closed" where it would fail.
`auditT1_tests.rs`: 55 pass, unchanged. `auditT1_cli.rs`: 47 pass, unchanged, run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 98 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 36

Scope: `KpfTLegOut` (a leg leaves a covered account at once by its two parties' signatures), its model
(`Account::leg_out`) and builders (`leg_out`, `leg_out_as`); the script table extended to 53; the F58 test as the
author edited it. Everything below is engine-run (`verify::verify`) unless it says "read". Only
`crates/kpf-protocol/tests/auditT1_builders.rs`, `crates/kpf-tests/tests/auditT1_tests.rs` (the table) and this
file changed; no contracts, no `src`, no commit, nothing under `kpf-out`.

**One finding: F60 (Medium), a covered account pays its side B, by `KpfTLegOut` with the user's signature and by
`KpfTLegMove` with the three. Everything else asked held: value, the note, the chain, what the script opens. The
model and the script agreed on each of 400 random amounts.**

## Findings

### F60 (Medium, open). A covered account pays its side B: by a leg that leaves, and by a leg that moves

`t1b_finding_f60_a_covered_account_pays_its_side_b_by_a_leg_that_leaves_and_by_a_leg_that_moves`

The design (section 6a): "A covered account pays its side B nothing by any path: a take for B is a deposit, the end
of an account with no leg is a deposit, and what the clearing party has is drawn from the shard." `KpfTLegOut`
says the same of itself and holds it for `addb` (not below zero) and for the object's value (what it held less
`take`). But:

- `pay` moves B's margin to A's and no price is read; `take` leaves A's margin for whatever output the two sign
  (the script names none; the builder writes A's key). With `pay` and `take` both X, X of B's margin leaves the
  account, A's margin as it was.
- In the test an account of 20 KAS (A) and 40 (B) with three legs: the first leg leaves with `pay` 40 and `take`
  40, the 40 KAS in an output to the clearing party's own key. Taken. The account goes on with A's 20, B's 0 and
  two legs standing.
- `KpfTLegMove` has it without the detour: the change of B's margin in either account may be below zero (the
  script asks only that no margin ends below zero and that each object holds its changes). In the test B's margin
  in the account the leg leaves falls by 30 KAS, which leave with the payer, the clearing party. Taken.

What is lost, and by whom. What side B has in an account is what anyone deposits to the shard by a take for B
(26.8 of the 40 KAS in the test, at the entries); after the transaction nothing is left to deposit. A clearing
party draws from its shard only above what the shard is asked; out of an account it now takes without that
limit. So the loss falls on the other users of the shard at the time it matters: when the shard holds less than
it is asked or less than the notes of what it owes, and the clearing party's gains in other accounts would have
filled it. The user who signs loses nothing (its margin and its leg's settlement are as it wanted), so it has no
reason to refuse, and the clearing party's program can build every leaving this way. Where side A is a key of the
clearing party's own it signs alone, but what is in such an account was its own to begin with.

Why Medium: it needs the user's signature for anything the clearing party won from that user, and it breaks no
user's own account. It does break a sentence the reserve's users are told to rely on.

It is not closed by naming the output of `take` (A's key): A then hands the amount on. What a script can hold:
`take` at most A's margin before the leg left and what A is paid kept in the account (`take <= A's margin +
min(pay, 0)`), so that what B paid leaves only by A's own later take; and in `KpfTLegMove` the two changes of B's
margin together not below zero. Or the sentence is narrowed to what it is now: nothing leaves side B of a covered
account without the user's signature.

A correction of round 35: I agreed there that margin leaving during a leg's move was by the rule, because every
party of both accounts signs. For side A's margin that stands. For side B's it is this finding.

## The F58 test as edited

`t1b_open_f58_the_tool_states_on_no_account_whose_sums_do_not_fit_and_adds_no_leg_into_one` says what I mean.
`leg_added` and `leg_moved` now refuse the ninth of the largest legs, the builder with them, and `KpfTLegAdd`
still takes it built past the model (open by decision). I changed two things: the comment above it still said
"NOT closed"; and I added the controls (the eighth leg is added to seven and moved into seven), so that a refusal
for another reason would be seen. F58 is closed in the tool.

## What held under attack (all engine-run)

| Question | Test | What was put to the engine |
|---|---|---|
| 1. Value | `t1b_r36_a_leg_that_leaves_moves_what_the_two_sign_for_and_holds_the_account_to_it` | 400 random `pay`, `take`, `addb` (each also below zero and beyond the margins), built past the model where it refuses: 180 taken, 220 refused, each as `Account::leg_out` says. On a taken transaction, 30 changes refused: the account a sompi short; the note below its least; a third output under the id (to B's key, a second note, an account of B's making); the note or the account under no id; the two in each other's place; `pay`, `take` or `addb` a sompi off what the outputs say; either signature by another key; a margin in the state that the object does not hold (either side); a `take` below zero with the account holding 1 sompi, 3 KAS or 6 KAS less than it adds (it adds: it takes nothing); amounts at the ends of 64 bits |
| 1. Whose consent | same | read: both signatures are held to `SIGHASH_ALL`, so each is over every output. Run: where the taken amount goes is theirs to say: re-addressed to a third key it is taken (this is the half of F60 that is by design) |
| 2. The note | `t1b_r36_a_leg_that_leaves_writes_the_note_of_its_own_row_and_units` | for a long leg, a short leg and the four kinds of option the release is the join `KpfTLegAdd` wrote with the other kind, and has the hash of the note a statement's settlement writes; applied, the leg's row is no market's again and the others are as they were. 20 notes not written: fewer units, a unit off in the net or the gross alone, the other way, none, a join, a deposit, what is owed, the other kind's row, the market's bare name, another market's row, another committee, scale or limit, another shard, another side B, side A's key, an amount in it; an option with the futures row or a futures leg's units. No note (the note under no id) is in the row above |
| 2. Two legs of one market | same | in one account (10 and 5 long beside another account's 10): the release of the 10 leaves the row at 15, and the 5 stand. The leg that left does not leave again (the account that goes on does not open for it). A release is applied once: the transaction that applies it takes no output for it to go on in |
| 2. A release before its join | same | a second account of the clearing party (another key on side A) takes a leg equal to the first account's and lets it go at once; its release is applied and its join is not. The row is emptied while the first account's 10 stand and the shard is asked less. A draw stated then is ended by the join that is not applied (`KpfTDrawNote`: the shard again, the claimer's bond lost), and with the join applied the row is as it was. This is round 34's item with `KpfTLegOut` in the place of a statement: see the note below the table |
| 2. Legs no script lets in | same | on an account of the model's making, a leg whose direction's place is 0, below zero, 7, 8, 9, 10, 15, 23 or the largest number leaves as a futures leg with the size as side B's units; 3, 4, 5, 6 (a strike of nothing) as an option. The script takes the model's note for each of the 14, and `KpfTStepRow` holds a statement to the same row and units (said with the other units, the row is wrong). A size below zero: the script writes the gross above zero, the model writes the size, so the model's note is refused and the note with the gross above zero is taken. No script lets such a leg into a covered account |
| 3. The chain | `t1b_r36_a_leg_that_leaves_opens_the_chain_of_a_covered_account_and_nothing_else` | 13 refused on a taken transaction: another leg of the account in the leg's place (the first, the third), a leg that is not the account's, the leg with its entry or its size one off; no chain before it, a longer one; no leg after it, the leg after it twice, another, a byte short, a byte more; the first leg's place shown with the outputs of the second's. Of eight legs each of the second to the eighth leaves and the account that goes on has the chain without it; the first (seven after it) does not, by the model and, built past it, by the script |
| 3. A statement stands | same | an object that carries a statement, and one that carries a disputed point: shown as the account it was, the output is not that; shown as it is, the script does not open it (4 refused). A leg does not leave under a statement |
| 3. One leg | same | the only leg leaves (1 KAS paid to A): the account goes on with no leg and the chain of none, and then ends by `KpfTAcctEnd`, side B's margin a deposit to the shard |
| 5. What the script opens | same | an account no shard covers (138 bytes) is not opened, by the model and built past it. A shard shown as an account with a leg: with its own side B (32 zero bytes) nothing is signed; with the clearing party's key in that place the object is not the shard |
| The table | `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` | 53 scripts, 60 entries, 51 entries `release` |

**A release before its join, now at once.** Section 6a says an owner knows of every note before it lands because
"a leg's leaving and what is owed are written by a statement that waited its dispute period". With `KpfTLegOut` a
leaving is written at once and for no bond, so the clearing party with a key of its own on side A makes a join
and a release of any size in two transactions and applies the release alone. What guards the shard is what
guarded it in round 34, `KpfTDrawNote` with the unapplied join, and it held. Two things follow for whoever
watches. The watcher has the dispute period of the draw, no more, and must know the joins of every account under
the shard that are not applied. And the sentence of 6a is no longer the reason the owner's own draws are safe
from notes it did not expect: a user's program now co-signs leavings too. Not a finding; the window is the same
and the guard is the same.

**Question 4, whom it harms.**
- The other users of the shard: by F60, and by nothing else I found. The release is the leaving leg's own row and
  units, exactly what its join added; rows of one market split over two rows are asked no less together (each
  row's net and gross are counted apart).
- A holder of an option: in an account the holder is one of the two who sign. A writer with a key of its own on
  side A is then both signers of that account, and what leaves the shard's row is that account's own leg.
- A user left below by a `take` the clearing party co-signs: taken (A takes all 20 KAS, two legs standing), and A
  is then below its maintenance at once, so anyone states a step of its liquidation. What A cannot pay after
  that is what side B is not paid in this account. That is the clearing party's own risk; the other users are
  touched only as far as side B's margin here would have been a deposit, and the shard is asked the same.

## Read, not run

- `acct-out` (`acmds.rs` `out_cmd`): it settles at a price (`leg_gain_at`) or at the amounts given, and builds
  through `leg_out`; what is taken goes to A's key. A gain beyond B's margin is refused by the model, there is no
  note of what is owed on this path: the two settle for what the account holds or the clearing party adds.
- `KpfTLegOut` asks no time (no lease, no lateness): the two parties let a leg out of an account past its lease.
- The script reads the leg's kind as `KpfTStepRow` does (line by line the same three conditions), and
  `KpfTLegAdd` asks more of a leg that enters (a strike from 1 to the limit, a direction of 1 or 2).
- `OpCovInputCount(id) == 1`: no other object of the registry's id is spent beside the account, so a leg does not
  leave in the transaction that applies a note or moves a leg.

## Not reached

- The command at a node, and the user's request that the clearing party's program completes (not written yet).
- F60 with a user's real loss behind B's margin as one sequence of settled objects (the test starts from an
  account opened with B's 40 KAS).
- A watcher's side of the release before its join: how a program finds the unapplied joins of every account
  under a shard.
- A model-against-script comparison of the notes' application over random orders (open since round 34).

## Findings ledger

| | Severity | State |
|---|---|---|
| F60 | Medium | open: a covered account pays its side B, by `KpfTLegOut` (`pay` then `take`) and by `KpfTLegMove` (B's margin lower) |
| F58 | Low | closed in the tool (rounds 35, 36); open in the scripts by decision |
| F57, F59 | | fixed in round 35, verified |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 102 tests pass (4 new: the finding F60 and three `t1b_r36_...`; the F58 test's comment
and controls). The F60 test passes while the hole is open and says "F60 is closed" where it would fail.
`auditT1_tests.rs`: 55 pass (the table at 53 scripts). `auditT1_cli.rs`: 47 pass, unchanged, run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 102 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 37

Scope: the changes for F60 (`KpfTLegOut`: `payb` from A's margin to B's and no movement from B's; `KpfTLegMove`:
the two changes of B's margin together not below zero), their model and builders, and every other path by which
side B's margin of a covered account falls or leaves. Everything below is engine-run (`verify::verify`) unless it
says "read". Only `crates/kpf-protocol/tests/auditT1_builders.rs` and this file changed; no contracts, no `src`,
no commit, nothing under `kpf-out`.

**F60 is closed in the two scripts and verified. One new finding, and it is not of this round's changes: F61
(High), the owner of a shard takes all the shard holds through an account whose other side is a key of its own,
because a leg enters at an entry no script reads and the shard pays what is owed in full. It has been there since
the reserve; I did not see it in rounds 32 to 36.**

## Findings

### F61 (High, open). The owner of a shard is owed by it, through an account of its own at an entry of its choosing

`t1b_finding_f61_the_owner_of_a_shard_is_owed_by_it_through_an_account_of_its_own_at_an_entry_of_its_choosing`

Three things that are each as designed:

- `KpfTLegAdd` reads no price. The entry of a leg is what the two keys of the account sign for (above zero, within
  the market's limit).
- A leg leaves by its party's statement at the committee's price, and what the pot cannot pay side A is a note of
  what is owed (`KpfTStepLiq`).
- `KpfTNoteApply` pays that note from the shard, all the shard holds if the note is of more, where the account's
  side B is the shard's owner. What the shard is asked to hold is not looked at (it cannot be: this is what the
  reserve is for).

Together, with both keys of the account the owner's own:

1. A shard of 50 KAS; a user is long 10 units against the owner and the shard is asked 5.5275 KAS for it. By a
   draw the owner gets 44.4725 and not a sompi more.
2. The owner opens a covered account with a second key of its own on side A (25 KAS and 5), and adds a leg: long
   10 units of another market at an entry of 0.1, where the committee says 10. Both signatures are its own.
3. Side A states that the leg leaves at 10. The statement has no wrong point: by the model at the committee's
   price, and at the engine the end and the row are answered right and not wrong. Nobody ends it; whoever disputes
   it loses a bond.
4. It is settled: side B's 5 KAS in that account are A's, and a note of 94 KAS owed is written to A's key.
5. The note is applied: the shard pays all 50 KAS to the owner's second key and holds nothing. The user's leg
   stands in its rows, and it is asked 11.055 KAS (the owner's own leg is still counted too).

What is lost, and by whom: everything the shard holds, by the users whose accounts it covers. The requirement,
the rule of a draw, `KpfTDrawNote` and the closures of F59 and F60 all guard a door beside this one. It needs no
second party, one dispute period, and a bond that comes back.

The same statement is also what remains of F60: what side B has in an account whose side A is its own key becomes
A's (step 4), and A takes it. To bring margin there from a user's account still takes a leg's move with that
user's signature.

Why High and not Critical: the owner takes what was its own deposit, not a user's margin; what users lose is the
cover the design promises them, at the owner's will.

What would close it is a decision, not a line. The section 4 table already says "a cover asks what a mark asks:
both sides at their maintenance at a cited price", and `KpfTLegAdd` asks no citation. With the entry of a leg that
enters a covered account held to the committee's price (within the tolerance), a gain on entering is at most the
tolerance, and what is left is the ordinary case: an account of the owner's own that gains as the market moves,
which anyone ends by a step of a liquidation the moment its other side is below. I have not tested that form.
A rule on `KpfTNoteApply` alone (pay only down to what the shard is asked) would refuse real users what they are
owed, so it is not one.

## Fixed and verified

**F60.** `t1b_fixed_f60_side_bs_margin_does_not_leave_by_a_leg_that_leaves_or_moves`.

- `KpfTLegOut`: the model has no amount that moves B's margin to A's. Round 36's transaction in its new form (a
  `payb` below zero with the outputs it would ask for) is refused in 4 forms: all of B's margin paid and taken,
  paid and kept, a sompi, 1 KAS. A leg's gain is paid from the clearing party's coins: 5 KAS to A's key, the
  account holding what it held and both margins as they were.
- `KpfTLegMove`: the model refuses B's margin lower in one account, lower with A's higher in the other, and a
  sompi short of made up. Built past the model (the two changes are arguments 12 and 14), 6 refused: 30 KAS lower
  in one; 30 lower and a sompi less than 30 higher; a sompi lower in either alone; 30 lower with A's margin in the
  other account 30 higher; 1 KAS likewise. Taken: 30 lower and 30 higher (each object holds its change), 30 lower
  and 31 higher, 1 higher alone, 30 higher in one and 1 lower in the other.
- One object at both places of a move is refused. Another shard or another side B in the second account is
  refused (round 34's test, still passing).

## What held under attack (all engine-run)

| Attack | Test | What was put to the engine |
|---|---|---|
| `KpfTLegOut`, the amounts | `t1b_r36_a_leg_that_leaves_moves_what_the_two_sign_for_and_holds_the_account_to_it` | 400 random `payb`, `take`, `addb` (each also below zero and beyond the margins), built past the model where it refuses: 152 taken, 248 refused, each as `Account::leg_out` says, and in none that is taken is B's margin below what it was. 37 changes of a taken transaction refused: those of round 36, and 14 sets of amounts at the ends of 64 bits and of what a margin counts: a `payb` or an `addb` below zero, down to the least number; a `payb` of the largest number, alone and with a `take` that makes A's margin stand; a `payb` that puts B's margin beyond what is counted (by 20 KAS, and by a sompi), made up by a `take` below zero; an `addb` of the largest and of the most a margin counts; a `take` at either end. Each also with the outputs it would ask for, where they can be written |
| What B pays A | same | the output to A's key (what A takes and what it is paid) re-addressed, or a sompi less, is taken: the script does not know of it, as designed. A's signature of the whole is what holds it (read: `SIGHASH_ALL` is asked of both) |
| `KpfTAddC` | `t1b_r37_side_bs_margin_falls_by_no_other_script_and_ends_as_a_deposit` | either side adds 1 KAS; an amount of minus 1 KAS, minus a sompi, nothing, the least number, with the outputs it would ask for: 8 refused |
| `KpfTHand` | same | a side handed over with B's margin a sompi, 1 KAS or all of it lower: 3 refused |
| `KpfTLegAdd` | same | a leg entering with what B adds a sompi or 1 KAS below zero, or the least number: 3 refused |
| Margin that arrived by a move | same | a leg and 30 KAS of B's margin move into a second account (another key on side A); the leg leaves it by `KpfTLegOut`; the account ends by `KpfTAcctEnd`: the deposit is of 31 KAS (what was there and what moved in), no output of the end is to B's key, and applied the shard holds 31 KAS more. A take for B there stated by B's own key pays no key |
| The fee of B's own liquidation | same | the one payment out of B's margin to a key that no price moved: where B is below its maintenance (half a KAS against 240 of worth), a step of its liquidation pays whoever states it half a KAS, all B has there and no more, and that end is answered right. With 40 KAS B is not below and nothing is stepped. This is path (a), a statement |
| Round 36's notes and chain | the two `t1b_r36_...` tests | with the new arguments: as in round 36 (20 notes not written, 21 refused on the chain and on what is opened). The only leg leaves with its 1 KAS of gain paid from the payer's coins, and the account ends with B's 2 KAS a deposit |

**Paths by which B's margin of a covered account falls, after the change.** A statement of kinds 2 to 4 (a take
for B is a deposit; a step or a leaving moves what the prices say and, for B's own liquidation, the fee); a
deposit; the account's end; a move to another account of the same B and shard. No other script lowers it: for
`KpfTAddC`, `KpfTHand`, `KpfTLegAdd`, `KpfTLegOut` and `KpfTLegMove` that is engine-run above; `KpfTOpenC` makes
an account and the scripts of a position are typed for 138 bytes (read). And then F61: "a statement at prices
that are answered for" is itself a way out where the leg's entry was nobody's but the owner's.

**Does the stricter form break anything a user needs?** Not that I found.

- A user's gain on a leg that leaves at once is an ordinary output to its key in the same transaction; its
  signature is over that output, so it is paid or nothing happens. It no longer rises in the account, so a user
  who wants its gain as margin adds it after (or adds in the same transaction with a `take` below zero from coins
  it already has).
- A user's loss is `payb`, inside the account, as before.
- The cost is the clearing party's: it pays gains from free coins while what it has in the account stays there
  until a deposit and a draw. A clearing party without free coins does not sign, and the user leaves by its own
  statement after the dispute period, paid from the pot and then by the shard. That is slower, not worse.
- One thing for the tool: the builder takes what A is paid from the payer's coins. Where the user is the payer
  it pays itself; the request the user signs and the clearing party's program completes (section 11, item 1) is
  what makes this a payment.

## Read, not run

- `KpfTLegMove` asks one clearing party and one shard of the two accounts (`moving[33..66]` and
  `moving[138..171]` equal) and that the second place is not the first; both objects are of one id and the
  transaction has exactly two inputs and two outputs of it.
- `KpfTStepEnd`, `KpfTAcctEnd`: unchanged since they were reviewed; the deposit paths are run above through the
  builders.
- F61 at entries held to the market: two accounts of the owner's own, long and short, with little margin. One
  gains as the price moves and is owed by the shard; the other loses no more than it has. Anyone ends both by a
  step of a liquidation as soon as a side is below, which is at once where the margins are that small. Not run.

## Not reached

- `acct-out` and the commands; anything at a node.
- F61 with an option (a call held at a strike far below the market) and with a take in the place of a leaving:
  the arithmetic is the same row; not run.
- The citation at `KpfTLegAdd` as a closure of F61: not written, so not attacked.
- A model-against-script comparison of the notes' application over random orders (open since round 34).

## Findings ledger

| | Severity | State |
|---|---|---|
| F61 | High | open: the owner of a shard is owed by it through an account of its own at an entry of its choosing |
| F60 | | fixed in round 37, verified (what remains of it is F61's statement) |
| F58 | Low | closed in the tool (rounds 35, 36); open in the scripts by decision |
| F57, F59 | | fixed in round 35, verified |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 104 tests pass (the F60 test turned into its closure's; the three `t1b_r36_...` brought to
the new arguments; new: `t1b_r37_...` and the finding F61). The F61 test passes while the hole is open and says
"F61 is closed" where it would fail. `auditT1_tests.rs`: 55 pass, unchanged. `auditT1_cli.rs`: 47 pass,
unchanged, run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 104 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 38

Scope: the design on paper that closes F61 (`docs/ARCHITECTURE-CLEARING.md`, end of section 11, "F61 and what
closes it", second form; the box at the head of 6a): a leg enters a covered account by a statement (kind 6) that
both parties sign, at the price of the statement's time, answered as any row and price are; `KpfTLegAdd` is
withdrawn. Nothing was built for this, so nothing is engine-run here; where I say the current scripts do or do not
do a thing I read it in the contract, and the F61 hole as it stands is the committed test
`t1b_finding_f61_...`. No contracts, no `src`, no commit, nothing under `kpf-out`; only this file changed.

**The closure holds against the four takings it names, with one condition: every leg's market must have a live
committee whose price can be cited. The dispute-only entry (no citation required to settle, only the absence of a
disproof) does not enforce that condition, and a leg on a market nobody prices reopens F61 (H1 below). Separately,
the lock an account is under while a statement stands can be used to keep the other party from leaving (M1). Five
things the statement machinery does not do yet for kind 6, and three disclosed residuals I confirm are disclosed
and not worse than said.**

## Holes in the design

### H1 (High). A leg on a market nobody prices reopens F61, by lapse

The closure rests on the entry's price being "answered as any row and price are" (section 4a): a dispute is
refuted by **one citation** of the committee's price (`KpfTStepPrice` reads a `KpfTCite` of a `KpfTNote` the leg's
**named** committee signed). Two facts about that, read in the scripts:

- A citation is **positive proof**: it exists only where a committee signed a price. There is no way to prove a
  price **wrong** except by citing a different one. So a point whose market has no citable committee can be
  **neither confirmed nor refuted**: a dispute of it **lapses** (round 35, F59: the row nobody prices; its
  dispute lapses and returns the object as it was).
- The leg carries its own `committee` (the 32 bytes in `XLeg`). Nothing at entry binds it to a live or honest
  committee; `KpfTNoteApply` takes a join to an **empty** shard row for any market's hash (round 35), so a market
  the shard never priced is introduced by the entry itself.

In the owner's own account (both keys the owner's), this is F61 again:

1. The owner opens a covered account, both sides its own, and enters a leg (kind 6) on a market `M` whose
   committee is a key it controls or one that is simply offline, at an entry of its choosing. The entry's price
   point can only lapse. The owner does not dispute its own statement, so after the dispute period it **settles**
   and the leg is in at the chosen entry. (If the owner holds `M`'s committee key it need not even wait: it signs
   a `KpfTNote` at any price and answers its own point **right**.)
2. The owner states that the leg leaves (kind 4) at a price far from the entry. Same market, same unciteable
   point; it settles undisputed. The pot cannot pay the gain; the note of what is owed is written and paid in
   full by the shard (`KpfTNoteApply` kind 4, below the requirement, as the design keeps on purpose).

A watcher of the shard **can** stop each drain: it disputes the leaving's price point, which lapses and returns
the account as it was, so the owed note is never written. But on a lapse **both bonds return** — the watcher is
paid nothing for the work and the fees, while the owner risks nothing and retries. For an ordinary market a false
statement is answered **wrong** and the claimer's bond goes to the refuter: the watcher is paid and the attacker
is charged. A market nobody prices turns that into an unpaid, unending vigil, and one missed window drains the
shard. So the closure does not close F61 for such a market; it moves it from "one statement, done" to "a
statement the owner may repeat for free until a watcher blinks."

This is the same shape as the F60 "release before its join" residual (round 36) and the F59 "row nobody prices"
case (round 35), and it has the same guard (a watcher's loop that is not built) — but here the guard is **unpaid**
and the attack is **free to repeat**, which the other two were not (a false release is caught by `KpfTDrawNote`
with the real note; a false draw row is answered wrong for the bond).

**What closes it** (and why I think the citation the design leaned away from is the sound one): require a
**positive citation at entry** — a leg settles into a covered account only if its price is **cited** within the
tolerance, not merely left undisputed. Then a leg on a market nobody prices **cannot enter**, so no leaving on it
can happen, and every leg in a covered account is on a live market whose later statements are refutable **for a
bond**. This is one citation per leg at the one moment it enters (a rare event), not at every settlement, so the
"one price" goal of `GOAL.md` is kept: the committee's price is cited once, and what the clearing party charges is
still an output beside it. The dispute-only form the design prefers is right for a **leaving** (the leg is already
in, its market already known live) but wrong for an **entry**, which is the one place a new market is introduced.

(If committees are a **gated, staked** set and a leg's `committee` is checked against a registry at entry, H1
narrows to the case of a registered committee going dark, which is rarer but still a free retry. If committees
are **permissionless** at the script level, H1 is immediate and total. Which of these holds is a system fact I did
not find enforced in `KpfTStepPrice`/`KpfTNote`; it must be stated. Either way a positive citation at entry
closes it.)

### M1 (Medium). An account under a statement is locked, and the lock is a lever

Read in the scripts: `KpfTLegOut` and `KpfTLegMove` require the account's output to hash to the **account form**
(`blake2b(p33(blake2b(terms + root)) + moving)`); `KpfTChange.state` turns the output into the **changeHash**
form while a statement stands, and another statement cannot be stated on a changeHash output. So while any
statement stands, the account cannot be left at once, handed over, or moved, and no second statement can be
stated. This is correct for safety (it is what blocks F60's at-once spends mid-statement), but it is a lever:

- A party that can state one-signature changes (a take/leave of its own, kind 2/3/4; a liquidation step of
  either side, kind 3, is by anyone) keeps the account under a statement for a dispute period at a time, during
  which the other party cannot take the at-once exit nor state its own leaving. A **false** statement is answered
  wrong and costs the stater a bond, so honest-only statements bound this; but a party willing to fund bonds for
  statements that are **true enough not to be refuted** can hold the account and delay an honest counterpart's
  exit. The honest party's recourse is to dispute (and be paid if the statement is false), or to wait out the
  period and act in the gap before the next statement lands — a race.
- Building a multi-leg account is **serial**: each kind-6 entry locks the account for a dispute period, so the
  next entry cannot be stated until the last settles. An N-leg account takes N dispute periods to build, and each
  entry re-traces **all** legs so far (the k-th entry is a k-row trace), so a large account's entries are large
  transactions. Not a security hole, but a cost to state plainly: the account is not a thing a user opens and
  fills in one sitting.

Decision needed: whether a cap on concurrent/!back-to-back statements, or a grace in which the counterpart may
act, is wanted; and whether the dispute period for an **entry** (which moves no value and whose only risk is a
false price) should be shorter than for a leaving or a draw.

## What the statement machinery does not do yet for kind 6 (changes beyond the ones the design lists)

Read in `KpfTChange.state` and the dispute scripts:

1. **Two signatures at stating.** `state` takes one `sig s` and checks it against `claimer`. Kind 6 needs both
   parties. `state` must take a second signature (the other side's key, read from the moving part) for kind 6, or
   a sibling entry must. The design says "two signatures on the statement (a statement is one party's today)" —
   so this is known, but it is a change to `KpfTChange` itself, not only a new end-script.
2. **The bond, on a two-signer statement.** `state` sets `bond` = the bond rate of the pot and names one
   `claimer`; a dispute pays the bond to the refuter, a lapse returns it to the claimer. For a kind-6 entry the
   claimer should be one of the two signers; on the owner's own account the bond returns to the owner on a lapse,
   which is the free-retry of H1. Who posts and who collects must be fixed; it does not by itself stop H1.
3. **A new end-script writing a JOIN note.** The settlement writes "the account with the leg and the note that it
   entered" — a kind-1 (join) note, flag 2 in the outs. `KpfTChange.state` already allows flag 2 for outs after
   the first, so the outs shape is fine; but the end that computes the maintenance and emits the join is new
   (today's `KpfTStepEnd`/`KpfTStepLiq` emit release/owed notes, not joins).
4. **Every row a numbered point, old legs included.** The entry traces the account's existing legs **and** the
   entering leg. If only the entering leg's row and price are disputable points, a false price on an **old** leg
   in that trace could pass the maintenance check unrefuted and let a leg enter while A is really below its
   maintenance. The take/leave machinery already makes every row a point (`KpfTStepRow`/`KpfTStepPrice` at
   `2i-1`/`2i`); the kind-6 end must keep that, so an old leg marked at a false price is answered wrong like any
   other row. If it does, this is closed; it must be built that way.
5. **The first leg's empty prior chain.** Entering the first leg, the chain before it is the empty root (32 zero
   bytes). `KpfTStepRow` treats a row whose **own** chain is the seed as wrong, but the seed as a **prev** is the
   normal start. The end's "the row before the last carries the account's chain" check must accept the empty root
   as the starting chain for a first-leg entry (there is no row before it); otherwise an account's first leg
   cannot be entered. Build-care, not a hole.

A lapse or a wrong answer returning the account as it was: confirmed the existing dispute machinery returns the
object to `position` (the pre-statement hash), which for a kind-6 entry is the account **without** the leg — the
right direction. Value added with the leg: confirmed none moves in the statement; margin is added before by
`KpfTAddC` and the end only checks it. Both are as the design assumes.

## What I could not break (the four takings the design names)

1. **F61 as I ran it (an entry of one's own choosing).** On a market with a live citable committee, the entry's
   price is a point anyone refutes with one citation; a wrong entry is answered **wrong** and costs the claimers
   their bond. The owner can no longer set a price no one checks. Closed — conditioned on H1.
2. **Two own accounts, long and short at true entries, no margin.** The end asks side A's maintenance **with the
   leg**, so an account cannot be opened at a true entry with no margin: A must post its maintenance. With the
   terms as they are (mmr 5 %, tolerance 0.5 %, liquidation fee 0.5 %), the owner posts 5 % of each side's worth
   at entry; a move of the price beyond 5.5 % on the losing side **before a liquidator steps** is what the shard
   stands behind, and the shard is asked exactly the rate on the gross for it. So the "no margin" taking is gone;
   what is left is an ordinary default, liquidatable by anyone for the 0.5 % fee, and the shard's exposure is the
   gross-rate residual the design discloses (section 8, "what it should be is not computed"). I confirm that
   residual is bounded by the gross rate and is the ordinary netting risk of a clearing house, not a free drain —
   **provided** H1 is closed (a default at a true price is refutable; a default on an unpriced market is H1).
3. **An option struck far in the money, entered for nothing.** The option enters counted at what it is worth
   exercised at its entry (its row's gain is 0 at entry because the entry **is** the row price), the holder is
   paid none of that at entry, and it is kept whole as margin (the option's worth is in A's maintenance). So a
   deep-in-the-money option cannot be used to extract at entry: the value is already in the maintenance the
   holder must fund, and what a holder pays its writer is off the account. Closed.
4. **F60 by an entry in place of a payment.** A gain comes only from two prices that were each answered for; an
   entry gives no gain (entry = row price). So B's margin cannot be handed to A through an entry. Closed. (F60's
   own fix stands from round 37; this only confirms the entry does not reopen it.)

And: **the at-once lock holds across a statement** (H1/M1 aside) — `KpfTLegOut`/`KpfTLegMove`/`KpfTHand` cannot
spend an account that carries a statement, so nothing escapes the dispute period.

## Does withdrawing KpfTLegAdd leave anything that needs it?

`KpfTLegMove` appends a leg to another account with its **entry unchanged** (`ny = blake2b(... root2 + lg ...)`,
the leg's bytes as they are), by three signatures and **no price**. This is **not** the same hole, provided every
leg's entry is honest to begin with: the move only **relocates** a leg whose entry was set when it first entered.
Once `KpfTLegAdd` is gone and the only origination is kind 6 (a cited/answered entry), a moved leg carries a true
entry, so the move cannot mint a false one. The one thing the move still does and should be noted: it reads no
price and checks **no maintenance** (only margins ≥ 0 and B's margin not leaving the two accounts, from round
37), so it can leave the receiving account below its maintenance — but at a **true** entry, which is an ordinary
liquidatable account, not a drain. So nothing needs `KpfTLegAdd`; the move is safe as long as kind 6 is the only
way a leg is born. (If kind 6 is **not** adopted and `KpfTLegAdd` stays, the move is the same hole between two
own accounts, because `KpfTLegAdd`'s false entry rides along the move.)

## The cheaper closures the design weighed

- **A citation at every entry** — I **endorse** this, as H1 argues: it is the thing that closes the unpriced-market
  retry, it is one citation at a rare moment (entry, not every settlement), and it keeps "one price" (the cited
  committee price; the clearing party's charge is a separate output). The design's reason to lean away ("no
  citation at an entry") is right for a **leaving** and wrong for an **entry**.
- **The first form (mark a leg not yet counted, first gain not owed)** — sound against F61 (no false entry gain)
  but it either forfeits a user's early gain or grows the row to carry an uncounted leg; it is a worse user deal
  than citation-at-entry and does not beat it. The rejection is reasonable.
- **Paying owed notes only above the requirement** — I **agree it must be rejected**: a user owed by a genuine
  default must be paid from the shard whatever the shard is asked to hold; capping at the requirement would strand
  honest claims exactly when a default has made the shard short, which is the moment the cover exists for. The
  drain is the **entry**, not the payment; fix the entry.

## Disclosed residuals I confirm (not new, not worse than said)

- **The statement's time within the lateness.** Whoever states picks `t0` within the lateness the terms allow
  (default `late_daa` 100); both parties agreeing can pick the better end of that window. Bounded, small, and the
  same freedom a take already has. As disclosed.
- **The gross-rate exposure of a netted shard.** A shard whose rows are large and set against one another is
  asked little, and a default at a gap takes what margin did not hold; that is the rate on the gross, a terms
  number not yet computed (section 8). As disclosed; item 2 above is the honest-default form of it, and it is
  bounded by the gross rate once H1 is closed.

## What must be decided before building

1. **H1:** whether a kind-6 entry requires a positive citation (my recommendation), or whether committees are a
   gated set a leg's `committee` is checked against so that no leg can name an unpriceable market. Without one of
   these the closure does not close F61.
2. The second signature and the bond's owner on a two-signer statement (change to `KpfTChange`).
3. Whether the entry's dispute period and the account-lock (M1) need a shorter period or a counterpart grace, so
   one party cannot chain statements to deny the other its exit, and so a multi-leg account is not N dispute
   periods to build.
4. That the kind-6 end makes every row (old legs too) a disputable point and handles the first leg's empty prior
   chain.
5. Whether kind 6 should refuse an account that already carries an option or a leg of no kind, or trace them like
   any other leg (the trace and maintenance already handle options; a leg of no kind cannot be born once
   `KpfTLegAdd` is gone, so the question is only about legs already in from before the change).

## Counts and commands

Nothing built or run this round; the F61 hole as it stands is the committed
`t1b_finding_f61_the_owner_of_a_shard_is_owed_by_it_through_an_account_of_its_own_at_an_entry_of_its_choosing`
(round 37, passing). The test suites are unchanged from round 37 (`auditT1_builders` 104, `auditT1_tests` 55,
`auditT1_cli` 47).

# Round 39

Scope: the closure of F61 as built beside `KpfTLegAdd`: `KpfTEnter` (the statement of kind 6 by two signatures),
kind 6 in `KpfTStepRow` and `KpfTStepPrice`, `KpfTStepEnter` (its end), the lapse of kind 6 in `KpfTPoint`, and the
shard's id of an opening and a committee (`KpfTShardOpen`); the model (`AEnter`, `row_judged_in`,
`Account::{enter, enter_judged, enter_end_right, enter_wrong_point}`) and the builders. The script table is at 55.
Everything below is engine-run (`verify::verify`) unless it says "read". Only
`crates/kpf-protocol/tests/auditT1_builders.rs`, `crates/kpf-tests/tests/auditT1_tests.rs` (the table) and this
file changed; no contracts, no `src`, no commit, nothing under `kpf-out`.

**The scripts do what their headers say: on 85 rows and ends put to the engine the model and the script gave the
same verdict, and what is stated, lapsed and opened is held as listed. Two findings. F62 (High): an entry is
answered within the tolerance of the account's own terms, which its two keys choose, so F61 goes through the
entry that closes it. F63 (Medium): the leg that enters is shown to nobody. I see no reason to keep `KpfTLegAdd`;
but removing it does not close F61 while F62 stands.**

A correction first. F62 was not in my round 38 report, and it should have been: the paper design already read
the tolerance and the maintenance from the account's terms. I named the committee (H1) and missed the terms.

## Findings

### F62 (High, open). A covered account's terms are its own, and an entry is answered within them

`t1b_finding_f62_a_covered_accounts_terms_are_its_own_and_an_entry_is_answered_within_them`

`KpfTStepPrice` takes the tolerance, and `KpfTStepEnter` the maintenance rate and the tolerance, from the 90 bytes
of terms in the account's own fixed part. `KpfTOpenC` asks nothing of those terms, nor of the dispute period, the
lateness and the bond rate in the moving part: they are what the account's two keys sign. Nothing ties them to
the shard that pays what the account is owed. With both keys the owner's own:

1. A shard of 50 KAS; a user is long 10 units against the owner, and the shard is asked 5.5275 KAS for it.
2. The owner opens a covered account under that shard (its second key on side A, 80 KAS and 1) whose terms say a
   tolerance of 50 % and no maintenance rate. `KpfTOpenC` takes it.
3. A leg enters by a statement (kind 6): long 10 units at **5**, where the committee says **10**. By the model the
   entry has no wrong point at the committee's price. At the engine the row is answered right, the end is answered
   right, and the price is answered **right with the committee's citation of 10** (5 is within 50 % of 10), each
   not wrong. On an account of the market's terms the same entry has its price as its wrong point (the control).
4. It is applied. The leg leaves by statement at **15**, within the same tolerance: the end and the price (again
   against the citation of 10) are answered right. The pot cannot pay the gain of 100 KAS: 99 KAS are owed.
5. The note is applied: the shard pays all 50 KAS to the owner's second key and holds nothing, the user's leg
   standing in its rows.

This is F61 through `KpfTEnter`. The same freedom reaches the rest of the closure:

- **The bond.** The bond is the account's bond rate of its pot. The lapse of an entry's point costs the claimers
  that bond (rule 2 of the decision); in an account of the owner's keys it is as small as they wish (a rate of 1
  in 10,000 of a pot of a few KAS), so a point nobody answers costs next to nothing and pays a disputer less than
  its fee.
- **The dispute period.** It is the account's own number, from 1 block: a statement of any price on such an
  account is applied before anybody can dispute it. (Read: `Account::check` takes 1 to 864,000; no script asks
  more.)
- **The lateness**, likewise: the window in which the statement's time is chosen.

What would close it: the same device as the committee rule. The shard's id is already the hash of its opening and
its committee, and `KpfTStepEnter` already holds the account's shard id to what the statement shows. Let the id be
the hash of the opening, the committee **and the terms and periods an account under it must have** (the 90 bytes;
the dispute period, lateness and bond rate, or bounds for them), and let `KpfTStepEnter` check the account's own
`terms` and `moving` against it. It has both in its arguments. Then a leg enters only an account whose numbers
are the shard's, and whoever takes the shard as its cover reads them in its id. I have not tested that form.

### F63 (Medium, open). A leg that enters by a statement is shown to nobody

`t1b_finding_f63_a_leg_that_enters_by_a_statement_is_shown_to_nobody`

What is stated of an entry is 148 bytes and the rows: the name of the leg's row in the shard (a hash of its kind
and its market), its committee, scale and limit, side B's units, the seed. The leg's 111 bytes are in no input and
no output of the statement or of its settlement (run: neither the leg, nor its market's name, nor the bytes of an
option's kind and strike are there). `KpfTLegAdd` showed the leg as an argument (run).

A futures leg can be found again: its market is one of few (try each against the row's name), its size and
direction are the units, its entry is the row's price. An option that is worth nothing exercised when it enters
cannot: its row adds nothing to what either side keeps, so the strike is in nothing the statement shows, and it
cannot be searched for.

Why it matters. A row is answered, right or wrong, only by showing the leg whose hash opens it. So for such a leg
nobody but its two parties answers its row, in the entry and in **every statement on the account after it**. In
the entry a point nobody answers costs the claimers their bond (rule 2). In a take, a step or a leaving (kinds 2
to 4) it does not: the lapse returns both bonds. So on an account with one such leg, a leaving stated with a false
row for it can only be disputed blind and lapses at no cost to its claimer, who states it again; applied once
unseen, it writes a note of what is owed. That is the unpaid watch of round 38's H1, by another door, and rule 2
does not reach it.

What would close it: the statement shows the leg (111 bytes in the place of the 92: the row's name and the units
follow from it, so `KpfTStepRow` need not be told them), at 19 bytes more.

## What held under attack (all engine-run)

| Item | Test | What was put to the engine |
|---|---|---|
| 1. `KpfTEnter` | `t1b_r39_an_entry_is_stated_by_the_two_on_a_covered_account_and_by_no_other_door` | 52 refused. Either signature by another key (side A's by B or a third, side B's by A or a third). What the object becomes: a statement whose claimer is side B or a third key; of kind 1, 2, 3, 4, 5 or 7; with a bond a sompi less or none; a sompi short of the bond; another time, dispute period or lateness. What is stated: no row, a byte short, a byte more, a leg in the place of a row, the rows from 116 as a leaving has them, each of the four pushes (24, 57, 90, 99) as another byte. Outputs: a first flag of 2 or 3, a second of 0, 3 or 0x80; none; five; cut. Kind 6 through `KpfTChange`. An object that carries a statement (shown as the account, and as it is); an account of 138 bytes; a shard |
| 2. The entering row | `t1b_r39_an_entering_rows_answer_is_the_models` | 37 legs as the leg that enters, 49 rows: long, short, equal to the old leg; the four kinds of option struck at 0, 1, the price, the limit and one beyond; directions' places of 0, -1, 7, 8, 9, 15 and 7 above a strike; no size, a size below zero, at the bound, a unit beyond, a part of a unit; a scale of the most and one beyond. For a long leg and a call also: the entry a sompi off the row's price, a sompi of gain as it enters, and what is said of it the other kind's row, the units the other way, another committee. 18 right, 29 wrong, each `row_judged_in`'s verdict and not the other. Right: the three futures cases, the options struck at 1, the price and the limit, the size at the bound and of a unit's part, the scale of the most. Everything else is wrong |
| 2. The row named | same | the first row named (the old leg judged as one that enters): the row is as the model says for what is said of it, and the end is wrong both times |
| 2. Old legs | same | the old leg's row in an entry's trace with each of its six numbers one more and one less: answered wrong 12 times. An old option's row in an entry is its row in a take, and is answered right |
| 3. `KpfTStepEnter` | `t1b_r39_the_end_of_an_entry_is_the_models` | 36 ends, 10 right and 26 wrong, each `enter_end_right`'s verdict. Right: the first leg of an account; a fourth leg (an old option among them); a leg at exactly side A's floor in four forms (a futures leg; a call worth 20 exercised, whose holder must have the 20; beside an old put that A holds; where side B asks 30, more than the floor); an old leg's gain within B's margin. Wrong: a sompi under each of those four floors; another seed; A's margin below zero by an old leg's loss; B's below zero by its gain (an entry owes nothing); the row named the first, none, one beyond the last; side B; an amount; the last row's worth below zero; the old leg's row left out; and 11 other outputs (A's margin a sompi more, B's a KAS less, the account without the leg, a sompi less held, a note of fewer units, the other way, a leaving's, to another shard, of another side B, of what is owed, a payment beside) |
| 3. The committee | same | a leg of another committee with its own committee said of it: the row is right and the end is wrong. With the shard's committee said of it: the end is right and the row is wrong. In neither are both right. An account that names the id of the other committee (a shard nobody opened) takes the leg, and its note is to that id, which no shard has |
| 4. The lapse | `t1b_r39_the_lapse_of_a_point_of_an_entry_costs_its_claimers_their_bond` | of an entry, at the points 0, 1, 2 and one beyond the trace, with an odd bond of 22,000,001 sompi: the account again without the leg; the disputer has 33,000,001 (its bond and the smaller half); the script's copy holds 11,000,001 more beside its fee; nothing to either claimer. Refused: a sompi less to the disputer, a sompi less in the copy, the disputer's part to side A, the account a sompi short, the account with the leg. Of a take: the disputer's bond and the claimer's each back, the copy keeping its fee alone |
| 4. The claimers' own dispute | same | side A's key as the disputer, left to lapse: of the two bonds they put in they have one and a half back, and the account is as it was, without the leg. It costs them half a bond and buys nothing |
| 5. The shard's id | `t1b_r39_a_shards_id_is_of_its_opening_and_its_committee` | the id is the hash of the opening's seed and the committee; another committee and another anchor give another id. Refused: another committee shown for the same id; the id of another committee, the seed alone (the id as it was before), nothing, another opening's |
| The table | `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on` | 55 scripts, 62 entries, 53 entries `release`. `KpfTPoint` now reads the fee a third time (the copy holds the fee and the half bond after the lapse of an entry); the lint says so |

**Item 4, read: can the claimers keep another from disputing?** The object carries one dispute at a time. A
claimer that disputes its own entry first, at a point it can answer:

- While its dispute stands anybody still answers any **wrong** point wrong (`at == point || verdict == 0`): the
  entry ends and the claimers' bond is lost. Its dispute shields nothing that can be answered.
- If it answers its own point right, the statement stands again as a new output, to be disputed again, and is
  applied only after a whole dispute period undisputed. So a point nobody can answer is reached by another
  disputer before the entry is applied, unless the claimer disputes again at once; and then the entry is never
  applied.
- If it lets its dispute lapse, the entry ends and half a bond is gone (run, above).

So the leg does not get in behind the claimers' own dispute. What a chain of own disputes does do is hold the
account under a statement for half a bond a period: that is M1 of round 38, recorded and open.

**Item 6, with `KpfTLegAdd` gone: does any path still put a leg into a covered account at an entry nobody could
dispute?** None that I found, F62 and F63 apart.

- `KpfTOpenC` opens with no leg (the root of nothing). `KpfTHand` leaves the legs and their entries (round 34).
- `KpfTLegMove` appends the leg as it is, its entry too, from an account of the same side B and shard (rounds 34
  and 37, still passing). A leg in such an account entered by a statement, so the move carries an entry that was
  answered for.
- A settlement of kinds 2 to 4 writes every entry at its row's price (the second chain), and each of those prices
  is a point of that statement. They are answered within the account's tolerance: F62 again, not a new door.
- An object under the registry's id is made by the scripts alone (round 30, still passing).

**Is there a reason not to remove `KpfTLegAdd`?** No. Two things to do with it, not against it:

- Removing it does not close F61 while F62 stands; the box at the head of 6a should stay until the terms are
  bound.
- `KpfTLegAdd` showed the leg and `KpfTEnter` does not (F63). If the leg is not put into what is stated, removing
  `KpfTLegAdd` takes away the one place a watcher learned a leg from.

One thing the decision of round 38 brings with it, to know: with rule 2 a disputer is paid half the claimers'
bond whenever nobody answers. For a price that takes the committee answering within the dispute period. Where it
does not, every honest entry is worth half a bond to whoever disputes it. The disputer risks its own bond, which
it loses if the committee does answer.

## Read, not run

- `KpfTStepEnter` asks side A's floor only. Side B's standing is not asked at an entry (the shard is what stands
  behind it), and what side B holds of options is not kept whole by the end.
- An entry where an old leg's gain is beyond B's margin is wrong at its end (run): the user first takes or
  leaves. Nothing is owed by an entry.
- Nothing else in `src` computes a shard's id (`shard_seed` and `shard_id` are used by the builders of the
  opening and of an entry); the reserve compares ids and does not make them.

## Not reached

- `KpfTStepPrice` for kind 6 against the model on random prices (it is run in F62 at one price, right; the
  author's test runs it wrong).
- The commands, and anything at a node.
- Random accounts of many legs entering one after another through settlements (each entry here is one statement
  on an account made for it).
- The binding of terms proposed for F62, and the leg in what is stated for F63: neither is written, so neither is
  attacked.
- M1 (the lock) at the engine.

## Findings ledger

| | Severity | State |
|---|---|---|
| F62 | High | open: a covered account's terms and periods are its own; an entry and a leaving are answered within them |
| F61 | High | open until `KpfTLegAdd` is removed; and by F62 after |
| F63 | Medium | open: a leg that enters by a statement is shown to nobody |
| F60 | | fixed in round 37, verified |
| F58 | Low | closed in the tool; open in the scripts by decision |
| F57, F59 | | fixed in round 35, verified |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 111 tests pass (7 new: the findings F62 and F63, and five `t1b_r39_...`). A finding's test
passes while the hole is open and says "is closed" where it would fail. `auditT1_tests.rs`: 55 pass (the table at
55 scripts). `auditT1_cli.rs`: 47 pass, unchanged, run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 111 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 40

Scope: the changes for F62 (a shard's id is of its opening, its committee, its ten terms and its lateness, dispute
period and bond rate; `KpfTStepEnter` holds an account's terms and periods to it; `KpfTLegMove` asks the two
accounts' equal) and for F63 (what is stated of an entry carries the leg whole; `KpfTStepRow` holds the named row
to it; `KpfTStepEnter` makes the note of it), with the model and the builders. Everything below is engine-run
(`verify::verify`) unless it says "read". Only `crates/kpf-protocol/tests/auditT1_builders.rs` and this file
changed; no contracts, no `src`, no commit, nothing under `kpf-out`.

**F63 is closed. F62 is closed where an entry's end is answered, and that is not enough: whether it is answered
is decided by the account's own dispute period, which is one of the numbers the check is there to hold (F64,
High). And with every account held to its shard's terms there is still a way for an owner with keys of its own to
be owed by its shard with no wrong point: the two ends of the tolerance (F65, High). So to the last question: yes,
I still see two paths, both run at the engine.**

## Findings

### F64 (High, open). An account's own dispute period decides whether its entry is answered

`t1b_finding_f64_an_accounts_own_dispute_period_decides_whether_its_entry_is_answered`

The closure of F62 is a check in the answer of a statement's end. An answer exists only where somebody disputes
within the statement's dispute period, and `KpfTEnter` writes that period from the account's own moving part
(`moving[102..111]`), of which `KpfTOpenC` asks nothing. So:

1. A shard of 50 KAS; a user's leg stands in it (asked 5.5275 KAS).
2. The owner opens an account under the shard's id with its second key on side A: the shard's ten terms, its
   lateness and bond rate, and a **dispute period of one block**. `KpfTOpenC` takes it.
3. A leg enters at the committee's own price. Its end is wrong (the account's periods do not give the shard's
   id); disputed in time it is answered wrong and not right (run). The statement's dispute period is 1, and the
   settlement, showing an age of one block, is taken: the account goes on **with the leg**.
4. The leg leaves by a statement at 20 where the committee says 10. By the model its price is a wrong point. That
   statement's dispute period is 1 too; it is applied. 99 KAS are owed.
5. The shard pays all 50 KAS to the owner's second key, the user's leg standing.

A dispute period of nothing is stated as well (`KpfTEnter` takes it; run). Whoever watches would have to dispute
in the block after the statement, every time.

The same holds for every other field: an account of a tolerance of 50 % and a dispute period of one block has its
wrong end, and nobody in time to say so. One number undoes the binding of the other twelve.

What would close it is a check that is not answered for but made: where the account is opened. `KpfTOpenC` has
the account's terms and moving part; shown the seed and the committee, it can ask
`blake2b(seed + committee + terms + moving[93..120]) == moving[139..171]`. An account that names a real shard's
id then has that shard's terms and periods from its first output, every statement on it has the shard's dispute
period, and `KpfTStepEnter`'s check is left to hold the leg's committee. (An account that names an id no opening
has is owed by nobody.) I have not tested that form.

### F65 (High, open). A leg entered and left at the two ends of the tolerance is owed by the shard

`t1b_finding_f65_a_leg_entered_and_left_at_the_two_ends_of_the_tolerance_is_owed_by_the_shard`

On an account that **is** of its shard's terms and periods, both keys the owner's:

- A price is right within the tolerance of the committee's (0.5 %), at an entry and at a leaving. A leg that
  enters at the lower end and leaves at the upper end gains 1 % of its worth, the committee's price not having
  moved, with every point right.
- Side A's floor is 5.5 % of the worth, so the worth is 18 times its margin, and the margin comes back whole.
- Side B has nothing in such an account, so the gain is what the pot cannot pay: owed by the shard.

Run: 140 units enter at 9.95 where the committee says 10 (the row, the price against the citation of 10 and the
end answered right, not wrong) and leave at 10.05 (the end and the price answered right). 14 KAS gained on a
margin of 80 KAS; 13 KAS owed; the shard pays them and holds 37. Nobody can end either statement. Four times
empties this shard; with a margin of 300 KAS, once. A round is two dispute periods.

This is not new with kind 6 (a leaving at the upper end was always within the rule); kind 6 is what makes the
entry's end of it a rule too. It is what remains when everything else is held, and I should have put it in round
38: it is the "tolerance is a cost the other side bears" of round 35, where the other side is the shard.

What would close it is a decision. Some forms, none tested:

- What a shard is asked to pay is counted the tolerance against side A at both ends: a note of what is owed is
  of the gain less the tolerance of the leg's worth at its entry and at its leaving. The two ends then gain
  nothing from the shard. It costs an honest user up to 1 % of a leg's worth, and only of what a pot could not
  pay it.
- The entering leg's price held to one side of the committee's (not below it for a leg A is long in, not above
  for one it is short in): the entry's end of the gain goes; the leaving's half stays.
- Or it is priced: the shard is asked the tolerance of the gross twice more, and the owner's own taking is what
  that part of the requirement is for. That does not stop the payment, since a note of what is owed is paid
  below the requirement; it only makes the draw smaller before.

## Fixed and verified

**F63.** `t1b_fixed_f63_an_entry_shows_the_leg_and_whoever_watches_answers_its_rows_from_the_chain`. The
statement's transaction shows the leg's 111 bytes and its market's name (an option worth nothing exercised, with
its strike). Read back from what is stated (`AEnter::read`, `leg_read`) it is the leg. After the entry is applied
a take is stated: with the leg from the entry and its entry from the entry's row (the statement applied last),
the take's row is answered right. And a row is right for no leg but the one shown: the leg that opens the chain
with another market, strike, kind, size, committee, scale, limit or entry shown in its place is answered wrong
(8), and the leg shown does not open the chain (no answer with it).

**F62, as far as an answer reaches.**
`t1b_fixed_f62_an_entry_and_a_move_hold_an_account_to_its_shards_terms_and_periods`.

- Round 39's account (a tolerance of 50 %, no maintenance rate) is still opened. Its entry at 5 has a right row
  and a right price within its own terms, and a **wrong end**, not right.
- One field at a time, 14 accounts that differ from the shard: each of the ten terms, the dispute period (one
  more, and one block), the bond rate (one less, and 1 in 10,000). For each the model states no entry, and the
  end of the entry written anyway is not answered right (answered wrong in 13; with the bond rate of 1 the
  builder writes no answer, see "to know" below).
- `KpfTLegMove`: 28 moves refused, into such an account and out of one, for each of the 14.
- The lease and the two asks are not of the id: an account of another lease, or with an ask of either side, is
  entered.

## What held under attack (all engine-run)

| Attack | Test | What was put to the engine |
|---|---|---|
| The shard's id | `t1b_r39_a_shards_id_is_of_its_opening_its_committee_and_its_terms` | the id is `shard_sid(seed, committee, terms, moving[93..120])`. 18 openings refused: another committee shown; the id of another committee, of round 39's form, the seed alone, nothing, another opening, another dispute period, another bond rate, and each of the ten terms changed. A shard of a tolerance of 50 %, a dispute period of a block and a bond rate of 1 is opened, under the id of those |
| What a user reads | same | a shard's terms and moving part are arguments 3 and 4 of its opening, the id in the moving part; so the terms and periods behind an id are on the chain where the shard was opened |
| Stating | `t1b_r39_an_entry_is_stated_by_the_two_on_a_covered_account_and_by_no_other_door` | 65 refused, as in round 39 with the new form: what is stated of 167 bytes and rows; no row, a byte off, a leg for a row, a leaving's form, round 39's form; each of the leg's seven pushes as another byte |
| The entering row | `t1b_r39_an_entering_rows_answer_is_the_models` | 37 legs, 53 rows against `enter_row_judged`: 18 right, 33 wrong, the same legs right as in round 39. For a long leg and a call also the leg shown another than the one that opens the row (another market, a unit more, the other direction, another committee, the entry a sompi off): wrong |
| The end | `t1b_r39_the_end_of_an_entry_is_the_models` | 40 ends against `enter_end_right`: 10 right, 30 wrong. A leg of another committee: shown as it is, the row is right and the end wrong; shown with the shard's committee, the end is right and the row wrong. The note is of the leg shown: outputs with the note of a short leg, an option, another market or a unit more are wrong |
| The lapse | `t1b_r39_the_lapse_of_a_point_of_an_entry_costs_its_claimers_their_bond` | as in round 39 |

## The questions asked

**The lease and the two asks, not bound: should either be?** No.

- The lease (`moving[84..93]`) is read by no script that opens a covered account (read: none of `KpfTOpenC`,
  `KpfTEnter`, `KpfTChange`, the answers, `KpfTHand`, `KpfTAddC`, `KpfTLegOut`, `KpfTLegMove`, `KpfTAcctEnd`
  reads those bytes). Its two keys do nothing with it.
- An ask is a floor one side sets for the other's margin (`KpfTStepEnter`, `KpfTStepEnd`, `KpfTHand` read it).
  It only keeps more in the account. An account of the owner's keys asks nothing, which is what the rule's own
  floor is for.

**Can a statement of kinds 2 to 4 be judged by other terms than the entry was held to?** No script changes an
account's terms or periods (read): the terms are in the fixed part,
which every script carries as it is (`KpfTLegOut`, `KpfTLegMove` and a settlement's second chain hash the same 90
bytes with the new chain; `KpfTHand` and `KpfTAddC` pass the fixed part's hash through); the periods are
`moving[93..120]`, and each script that writes a moving part copies those bytes (`KpfTHand` from 75,
`KpfTLegOut`, `KpfTLegMove` and the settlements from 84). `KpfTAcctEnd` then `KpfTOpenC` makes a new account of
any terms with no leg, and a leg enters it only by a statement (F64) or by a move from an account of equal terms.
Two accounts of the same other terms move a leg between them; with `KpfTLegAdd` gone neither has one.

**Can a watcher with only the chain answer every row of every later statement?** Yes, for what I could find:

- a leg's 111 bytes are shown where it enters (`KpfTEnter`: what is stated; `KpfTLegAdd` while it stands: an
  argument), where it moves (`KpfTLegMove`: an argument) and where it leaves at once (`KpfTLegOut`);
- its current entry is its row's price in the statement applied last on the account (kinds 2 to 4 and 6 write
  every entry at its row's price), and what is stated is an argument of the stating transaction; a move and a
  leaving at once keep the entries;
- the account's terms and moving part are arguments of every transaction on it.

So the account's legs follow from its transactions in order: which statements were applied and which ended is on
the chain too. One step of that is run (the take after the entry, in the F63 test); the whole walk over moves
and leavings is not.

**A shard whose own terms are loose.** It is opened (run), and its accounts are held to those terms: its users
read a tolerance of 50 % or a dispute period of a block in its opening. That is their reading; nothing to close.

**Is there still a path for an owner with keys of its own, `KpfTLegAdd` removed?** Two: F64 and F65.

## To know

- With a bond rate of 1 in 10,000 a statement's bond is a hundredth of a KAS on a pot of 100, and the builder
  writes no answer whose payouts are that small (an output of that value is not mined beside the rest of the
  transaction unless whoever answers adds to it). A shard chooses its bond rate and its users read it; but rule 2
  of round 38 (a lapse costs the claimers their bond) pays whoever disputes only as much as that rate makes it.
- `KpfTStepEnter` builds the note from the leg shown as `KpfTLegOut` reads it (a futures leg unless the
  direction's place says an option). A leg of no kind or of a size below zero shown has a wrong row, so its entry
  ends there; the end by itself does not refuse it.

## Not reached

- The check at `KpfTOpenC` proposed for F64 and the forms for F65: not written, so not attacked.
- The whole walk of a watcher over an account's moves, leavings at once and statements, from the chain alone.
- `KpfTStepPrice` for kind 6 on random prices (run right at 5 under a tolerance of 50 % and at 9.95 under 0.5 %,
  against a citation of 10; the author runs it wrong).
- The commands, and anything at a node. M1 (the lock) at the engine.

## Findings ledger

| | Severity | State |
|---|---|---|
| F64 | High | open: an account's own dispute period decides whether its entry (and any statement on it) is answered |
| F65 | High | open: a leg entered and left at the two ends of the tolerance is owed by the shard, no point wrong |
| F62 | High | closed where an entry's end is answered and where a leg moves (round 40); what is left of it is F64 |
| F61 | High | open until `KpfTLegAdd` is removed; and by F64 after |
| F63 | | fixed in round 40, verified |
| F60 | | fixed in round 37, verified |
| F58 | Low | closed in the tool; open in the scripts by decision |
| F57, F59 | | fixed in round 35, verified |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 113 tests pass (the F62 and F63 tests turned into their closures'; the five `t1b_r39_...`
brought to the new forms; new: the findings F64 and F65). A finding's test passes while the hole is open and says
"is closed" where it would fail. `auditT1_tests.rs`: 55 pass, unchanged. `auditT1_cli.rs`: 47 pass, unchanged,
run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 113 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 41

Scope: the changes for F64 (`KpfTOpenC` is shown the seed and the committee and opens a covered account only
where its ten terms, lateness, dispute period and bond rate give the id it names) and for F65 (`KpfTStepLiq`: the
note of what is owed is of what the pot could not pay less twice the tolerance of the two sides' worth at the last
row, and no note where that is nothing), with the model and the builders. Everything below is engine-run
(`verify::verify`) unless it says "read". Only `crates/kpf-protocol/tests/auditT1_builders.rs` and this file
changed; no contracts, no `src`, no commit, nothing under `kpf-out`.

**F64 is closed. F65 is closed for futures legs and not for an option that A holds: a put entered at the upper end
of the tolerance and left at the lower end is still asked of the shard, with every point right and no margin
needed to hold it (F67, High). And an entry asks side A's floor and not side B's, so an owner's own account with
nothing on side B is owed by the shard for a real move of the price, with nobody paid to stop it (F66, Medium).
To the standing question: yes, two paths remain, both run at the engine.**

## Findings

### F67 (High, open). A put entered and left at the two ends of the tolerance is asked of the shard beyond the band

`t1b_finding_f67_a_put_entered_and_left_at_the_two_ends_of_the_tolerance_is_asked_of_the_shard_beyond_the_band`

The band is twice the tolerance of the two sides' worth **at the leaving's prices**.

- A futures leg is in both sides' worth: its band is 2 % of its worth, where the two ends make 1 %. Enough.
- An option is in its writer's worth alone: its band is 1 % of the underlying's worth at the price it leaves at.
  A put that A holds, in at the upper end and out at the lower end, has made the tolerance of the worth at both
  ends, which is 1 % of the worth at the committee's price: more than 1 % of the worth at the lower end, by twice
  the tolerance squared. 0.005 % of the underlying's worth is asked of the shard, the committee's price not having
  moved.
- It costs nothing to hold. Struck at the price it enters at, the put is counted at nothing: its holder is asked
  the maintenance of nothing and keeps nothing whole, and side B is asked nothing at an entry (F66). So the size
  is what the leg's bound allows, not what a margin carries.

Run, on an account of the shard's own terms and periods, both keys the owner's: a put on 11,000 units struck and
entered at 20.1 where the committee says 20 (the row, the price against the citation of 20 and the end answered
right, not wrong) leaves at 19.9 (the end and the price answered right). 2,200 KAS gained; the band is 2,189; the
shard is asked 11 KAS and pays them. The 30 KAS of margin (there for the bond's sake) come back whole. Eight such
legs fit one account, and there is no limit to accounts: this shard of 50 KAS is five legs. A round is two dispute
periods.

A call the other way (in low, out high) has its band counted at the upper end and is asked nothing (run): the hole
is where the price a band is counted at is the lower of the two.

What would close it: the band counted at both ends of each leg, not at the last row's price alone. The row
already computes a leg's worth at its entry (`we`) beside its worth at the price; the tolerance of the two,
summed over the legs (one more number in a row), is what prices alone can have made since the last mark. That
form also covers a leg whose worth fell (below), and it asks an honest user half of what the band asks now (a
futures leg counted once at each end, not twice at one). Not tested.

### F66 (Medium, open). Side B enters with nothing, and nobody is paid to liquidate it

`t1b_finding_f66_side_b_enters_with_nothing_and_nobody_is_paid_to_liquidate_it`

`KpfTStepEnter` asks side A's floor. Side B may have nothing. A side with nothing is below its maintenance from
the first block and anyone may state a step of its liquidation; the step's fee is paid out of that side's margin,
so it pays nothing. In an account of the owner's two keys:

- 140 units enter at 10 with nothing on side B (the end is answered right);
- a step of side B's liquidation at 10 is right at once, for a fee of nothing (run);
- nobody states it. The committee's price goes to 11 and the leg leaves at 11, every point right. Of the 140 KAS
  gained none is in the pot: the shard is asked 109.2 KAS (less the band) and pays all 50 it holds;
- had the price gone to 9.6, side A would be below and a step of its liquidation pays 6.72 KAS to whoever states
  it. Side A loses at most its 80 KAS.

With a second account the other way the owner is owed whichever way the price goes, and loses in the other
account no more than a margin (which stays there as side B's, to be deposited or not). This is round 38's second
claim ("two accounts at true entries: an ordinary default, and somebody is paid to liquidate it"): the side that
loses is liquidated by somebody who is paid; the side that wins runs against a side B that nobody is paid to
step.

Why Medium: it needs the price to move, and whoever watches the shard can stop it at any time, for nothing.

What would close it is a choice: the end of an entry asks side B's floor too (the clearing party then carries a
margin in every account, and a step of its liquidation is paid); or the fee of a step of side B's liquidation in
a covered account is a note to the shard. Neither tested.

## Fixed and verified

**F64.** `t1b_fixed_f64_a_covered_account_is_opened_only_with_its_shards_terms_and_periods`.

- 19 openings refused, by the builder and built past it (the terms, the moving part, the seed and the committee
  are arguments 3 to 6): an account that differs from the shard in one of the ten terms, in the dispute period
  (one more; round 40's one block; nothing), in the bond rate (one less; 1 in 10,000), round 39's tolerance of
  50 %; another seed, another committee, the two in each other's place.
- Each of those accounts is opened under the id its own terms and periods give, which is no shard's.
- The lease and the two asks are not of the id: such an account is opened.
- A shard is not opened with a second key.

**F65, for futures legs.** `t1b_fixed_f65_what_prices_within_the_tolerance_make_is_not_asked_of_the_shard`.

- Round 40's round trip: in at 9.95 and out at 10.05 are right as before, and the leaving writes no note of what
  is owed (its end is answered right; the end of round 40, with a note of 13 KAS, is answered wrong).
- On the model, a futures leg in and out at the ends that favour A, against what the committee's prices alone
  would leave owed, for entries and leavings between 0.1 and 10 times the price, long and short: in 49 cases
  where the worth at the entry is less than three times the worth at the leaving the shard is asked no more.

## What held, and what the band does not reach (the attack list)

| Asked | Result |
|---|---|
| A leg entered at 20 and left at 10 | enough (the band is 2 % of the worth at 10, the skew 0.5 % of the worth at 20 and at 10) |
| A leg whose worth at its entry was far above where it leaves | not enough from a worth three times as large (run, model): in 27 cases the shard is asked more than the committee's prices leave owed, by at most the tolerance of the worth at the entry; the worst, a leg A is short in from 100 to 1, 48.51 KAS on a true gain of 9,900. It needs the price to have fallen by more than two thirds. Low; the form proposed under F67 covers it |
| Options | a put A holds: F67. Also run on the model: a put struck at 40, in at 20: out at 10, 19, 20 and 30 the shard is asked 5.05, 0.595, 0.1 and 0 KAS more than the committee's prices leave owed (100 units). A call the other way: nothing |
| Several legs where one leaves and the others are marked | the band is of every leg standing (run: an honest account of ten legs, below). Each standing leg is marked at its row's price in that statement, so its skew is in what is owed and its worth is in the band; a standing put A holds leaks as in F67 |
| A liquidation step in place of a leaving | the same script and band: the 120 steps of sides the pot did not pay in the F57 test are answered at the engine as the model says, with the note of what is asked |
| Legs marked many times at alternating ends | read: a statement chooses its end anew, and what it can make against the last mark is the tolerance of the worth at both, which is what a band per statement must cover; marks at the same end add nothing. Not run |
| A leg moved in by `KpfTLegMove`; `KpfTLegOut` in between | read: a move keeps the entry, so the leaving is as above; `KpfTLegOut` reads no price and moves nothing from side B's margin (round 37) |
| A take at the high end, then a leaving at the high end | read: a take whose gain is beyond side B's margin has a wrong end (round 34), so a take moves only what the pot holds, which in an account of the owner's keys is its own; what is owed is the leaving's, with the band. Not run this round |

**What the band costs an honest user** (run, model). A user long 10 units from 10 to 14 against a pot of 25: the
pot cannot pay 20 KAS, and the shard is asked 17.2: 2 % of the leg's worth of 140 is not paid. The same user with
nine other legs of that worth standing in the account: the band is of all ten, 28 KAS, and the shard is asked
**nothing** of the 20. So what a user is not paid grows with the size of its account, not with the leg that
leaves, and it is taken at every statement that owes. A form that asks less and is still closed: the one under
F67 (each leg once at each end: 1 % where the price did not move). I see no form that takes it once in a leg's
life: every statement chooses its end anew.

## F64: the other ways an account could come to stand under a shard's id

- `KpfTOpen`, `KpfTBoth` and the position's scripts are typed for a moving part of 138 bytes (read): the hash
  they write is of 138 bytes.
- `KpfTShardOpen` asks that the second key is 32 zero bytes (run: refused with one).
- `KpfTHand`, `KpfTAddC`, `KpfTLegOut`, `KpfTLegMove` write the account they were given with a key, a margin or a
  chain changed, the fixed part's terms and `moving[84..171]` as they were; `KpfTAcctEnd` writes none (read; the
  refusals of other outputs are in the tests of rounds 34, 36, 37 and 39).
- A settlement writes what was stated, and an answer or a lapse the object as it was. What is stated is the
  claimer's: an account of other terms stated as the outcome has a wrong end, answered within the dispute period,
  which is now the shard's (read).
- An account opened under an id that is of no shard (run: it is opened): its notes name that id
  (`moving[138..171]`), `KpfTNoteApply` takes a note only of the shard's own id, `KpfTLegMove` asks one id of the
  two accounts, and no script writes another id into an account (read).

**The shard's own periods.** Its statements are draws, with the dispute period in its own moving part, chosen by
its owner at the opening and held in its id. A shard of a dispute period of one block is opened (run, round 40),
and its owner's draw is applied before anybody can dispute it. That is not a hole in the scripts; it is what a
user reads.

**What a user reads off a shard before taking it as its cover** (all of it in the opening's arguments and the
shard's state; the id in an account's moving part is the hash of the first four):

1. the opening (the seed) and the owner's key;
2. the committee: every leg of every account under the shard is priced by it;
3. the ten terms, of which three decide what the user is owed: the maintenance rate (what side A must hold, and
   the rate on the shard's net), the tolerance (how far a stated price may stand from the committee's; twice it,
   of the account's worth, is not paid by the shard), the liquidation fee (the rate on the shard's gross);
4. the lateness, the dispute period and the bond rate. The dispute period is the time anybody has to answer a
   wrong statement on any account under the shard, and a wrong draw; the bond rate is what a wrong statement
   costs and what whoever answers is paid;
5. what the shard holds against what its four rows ask at today's prices;
6. the notes to the shard that are not applied (round 36): a row may count less than stands.

## Is there still a path for an owner with keys of its own, `KpfTLegAdd` removed?

Yes. F67, which needs nothing but two dispute periods; and F66, which needs the price to move and nobody to step
side B for nothing. Both are run on accounts of the shard's own terms, opened by `KpfTOpenC` as it is now.

## Not reached

- The forms proposed for F67 and F66: not written, so not attacked.
- Alternating marks and a take before a leaving, at the engine.
- `KpfTOpen` with a moving part of 171 bytes at the engine (read by its type).
- The commands, and anything at a node. M1 (the lock) at the engine.

## Findings ledger

| | Severity | State |
|---|---|---|
| F67 | High | open: a put A holds, in and out at the two ends of the tolerance, is asked of the shard beyond the band |
| F66 | Medium | open: an entry asks no floor of side B, and nobody is paid to liquidate a side with nothing |
| F65 | High | closed for futures legs (round 41); for options it is F67 |
| F64 | | fixed in round 41, verified |
| F62, F63 | | fixed in round 40, verified |
| F61 | High | open until `KpfTLegAdd` is removed |
| F60 | | fixed in round 37, verified |
| F58 | Low | closed in the tool; open in the scripts by decision |
| F57, F59 | | fixed in round 35, verified |
| F56 | | fixed in round 34, verified |
| F41 | High | open, with the owner |
| F50 to F55 | | fixed in rounds 29 to 31, verified |
| F42 to F49 | | fixed in rounds 28, 29 |
| F32, F13 | Medium | accepted by the owner |
| F39 | Medium | by design |
| F5, F7, F8, F9 | Low | by design / changed by round 23 |
| all others | | fixed in earlier rounds |

## Counts and commands

`auditT1_builders.rs`: 115 tests pass (the F64 and F65 tests turned into their closures'; new: the findings F66
and F67; brought along: the F61 test's amounts, the F57 test's amount and its helper, the F62 test's opening). A
finding's test passes while the hole is open and says "is closed" where it would fail. `auditT1_tests.rs`: 55
pass, unchanged. `auditT1_cli.rs`: 47 pass, unchanged, run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 115 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 42

Scope: the design of `docs/ARCHITECTURE-CLEARING.md` section 12 (a shard held to its requirement all the time:
books, a gate, a call) and its numbers model (`crates/kpf-protocol/tests/research_solvency.rs`), on paper and in
numbers. No script changed and none is reviewed here. I copied the model into a file of my own and extended it
(`crates/kpf-protocol/tests/auditT1_solvency.rs`, new, four tests), and ran one fact about the scripts as they
are at the engine (`auditT1_builders.rs`, one test). No contracts, no `src`, no commit, nothing under `kpf-out`.

**In the model's own terms I could not break the claim: with every account stepped in time, no user is unpaid on
any of 3,000 paths, whatever order the owner acts in. The claim breaks on two things the model takes for
granted, and one it builds in: that an account is stepped when it is due (it is not, where its owner holds it
under statements: H1, High, with the fact run at the engine and the model failing on 257 of 500 paths); that the
books are whole (the first of the seven unknowns: H2, High, with the rule I think is the weakest sound one); and
the call as the model has it, which is right at a hair and pays its fees before the users (H3, Medium).**

## Holes in the claim

### H1 (High). The books count a loss the shard does not have, for as long as the account's owner likes

The books' equity counts every account's loss whole. The shard has it only when somebody steps that account, and
only as far as its margin goes. The claim's bound ("the time it takes whoever watches to act") assumes whoever
watches can act. On an account of the owner's own keys it cannot:

- An account carries one statement at a time, and a statement that changes nothing is right. Run at the engine
  (`t1b_r42_a_statement_that_changes_nothing_is_right_and_holds_the_account_for_a_dispute_period`): on an account
  at its maintenance a take of nothing is answered right, by side A and by anyone for side B; on an account whose
  side A is **below its maintenance** (a step of its liquidation is due) the same two takes of nothing are still
  right, and so is a step that lets a leg of one sompi leave and the leg that matters stand; where side A's loss
  is beyond its margin the takes are no longer stated, and the step of the one-sompi leg is still right.
- Read, not run: whoever states such a statement settles it and states the next in the transaction after, so
  nobody else gets the output between. (And a dispute of one's own statement, left to lapse, returns both bonds for kinds 2 to 5.)

The sequence, in the model's terms (`an_account_its_owner_keeps_from_being_stepped_breaks_the_claim`):

1. The owner enters two accounts of its own, one each way, as large as the gate lets a pair in (a pair is set
   off: it is asked the rate on the gross alone, 1 % of one side's worth), and keeps both under statements.
2. The price goes. The account that has won 8 % leaves and is paid in coins. The other stays; nobody steps it. Its
   loss stands in the books as equity.
3. Against that equity the owner draws, and no call is made: the books say the shard is sound.
4. Users leave and are paid from what coins there are.

With ticks of 2 % (well within the bound of 4.5 %), the rules on: users are unpaid on **257 of 500 paths** (12 KAS
a path), on 185 of them with the books' equity at or above the requirement at the end, and the owner is ahead by
169 KAS a path after giving up the margin of the account it left behind. The model's `run` has no such account:
its watcher steps every account every tick.

The same in one line: the bound is on the move of the price between two moments at which the losing account can
be stepped, and its owner chooses those moments.

What must be decided: a step must reach an account that carries a statement. Either a step of a liquidation (and a
call's step) may be stated on an object that carries a take, a leaving or an entry, ending it; or a statement
that changes nothing is no statement (a take is of something, a step is of a leg whose worth is not less than a
share of the account's) and whoever settled a statement cannot state the next for a time the terms say. This is
M1 of round 38, which I rated Medium on its own; under this section it is what the claim stands on.

### H2 (High). Books that are whole (the first unknown): what must be true of the notes

A script's books are what the notes that were applied say. Two sequences, on the notes as they are:

- **A leg the books never had.** The owner's own account takes a leg; its note of entry is not applied (the gate,
  wherever it is asked, is not asked of it). The leg wins and leaves; its note of what is owed is applied and
  paid. The books never counted it. This is F66 again, through the books.
- **A leaving that rides on another leg's entry.** A row is sums. The owner has leg 1 booked at an entry of 20.
  Its leg 2, same market and size, entered earlier at 10; its note of entry is held back (or was refused by the
  gate). Leg 2 leaves at 20: its release finds the row's units (leg 1's) and is applied; it is paid 10 a unit,
  where the books expected to owe nothing for that slot. The row cannot tell whose units they were.

The weakest rule I can see that is sound, in four parts:

1. **The shard pays nothing for a leg whose entry it has not booked.**
2. **Booking is the gate**, asked at the price of the booking and counting what the leg has gained since its
   entry (a leg booked late and already in profit takes that much of the room, or does not get in).
3. **What is paid for a leg is counted from the entry that was booked.**
4. **A booked leg is unbooked by its own leaving and by nothing else**: identity, not sums.

Under these, what is not applied only makes the books careful: an unbooked leg can lose to the shard and cannot
be owed by it; a leg that left and whose note is not applied is still counted against the shard. One case is not
careful: a leg that left at a **loss** and whose notes are not applied goes on moving in the books with the
price, so its loss can read larger than what was paid in. The guard that exists for a draw (`KpfTDrawNote`: a
statement on the shard is ended by a note that landed before it and is not applied) has to hold for a booking
and for a draw alike.

For part 4, with four rows and no list: the note of entry, once booked, goes on as an object that says so (the
leg, its account, its entry), and the note that the leg left is applied only with that object beside it. One
output a standing leg; the shard's rows stay sums.

**Is there a form that keeps "an account touches the shard only at a default"?** Not with the gate. In the sense
of an input, yes: an account never reads the shard and never waits for it; it writes the note of an entry and of
a leaving as today. But a leg is the shard's to pay only once it is booked, and booking needs the prices of that
time, so it is a statement on the shard (or a transaction with citations). The user's leg is in its account at
once; its cover begins at the booking. That has to be said to the user.

### H3 (Medium). The call: right at a hair, its fees before the users, and whoever calls may be the owner

`a_call_at_a_hair_closes_every_user_of_a_shard_at_its_requirement_and_its_fees_are_all_it_has`

- A shard whose owner has drawn all it may stands at its requirement exactly. Over a book that is set off (two
  users of 1,000 units, one each way) the price need not move against it: the requirement is of worth, so a rise
  of **0.01 %** puts the equity below it. The call is right; every leg leaves; the fees (the liquidation rate of
  the gross) are **all 100 KAS the shard held**. In this run the users were owed next to nothing and were paid;
  where they are owed more, the model pays the fees first.
- Whoever calls may be the owner with another key. It then has the last of the requirement as fees, and its
  users closed at the prices it states, within the tolerance and at the end that is against them, with the band
  taken from what they are owed.
- Over a book that is not set off, a call stated at the end of the tolerance that is against the shard is right
  while the equity at the committee's price is above the requirement by less than the tolerance of the net
  (0.25 % above in the run).
- In my copy of the model's `run`, where the owner draws all it may every tick, the shard is called 44 times a
  path of 80 ticks at ticks of 0.5 %: its users are closed every other tick.

What a call must be, to be decided:

- **When it is wrong**: where the equity is at or above the requirement at any price within the tolerance (so
  counted at the end that favours the shard), as a step of a liquidation is today (the rate less the tolerance).
  And the gate and the draw at the other end (the rate and the tolerance): between where the owner may stand and
  where it is called there is then a move of twice the tolerance of the net, not a hair.
- **What it costs its caller**: a bond, lost where the call is answered wrong.
- **What it pays**: after the users, and not the whole of the gross's rate for a call nobody had to work for; an
  owner that calls itself must not come out ahead.
- **A set-off book**: the requirement of the gross grows with the price while the equity does not. Either the
  draw leaves room for a move of the price's level, or the gross is counted at the price of the last booking.

### H4 (Medium). Coins and equity are two things, and the draw is asked of one

The books' equity counts what losing accounts owe, which is in their margins until a statement on each brings it
in. A draw of "equity above the requirement" pays out coins against that. Users are then owed by a shard that is
sound on its books and has no coins. In the model nothing waited (its users are small beside what the shard
holds), and with every account stepped in time what waits is paid; with H1 it is not.

Two decisions follow. A draw is of what the shard holds above its requirement **and** of its equity above it, the
smaller of the two. And what side B has in an account must be the shard's to collect whole: today a take for side
B leaves side B's own maintenance behind (round 36: 26.8 KAS of 40 could be deposited), which under this section
has no meaning, since side B of a covered account stands by its shard.

## What I could not break

`the_claim_holds_in_the_models_terms_where_every_account_is_stepped_in_time`: my copy of the model with a user's
unpaid claim waiting for what comes in (a note goes on with the rest) instead of being counted at once, the
owner's own accounts paid after what users wait for, and the owner acting a second time **after the price moved
and before whoever watches** (out of its accounts that won, and a draw). At ticks of 0.5 %, 2 % and 4.5 %, with
and without that second turn: users unpaid at the end on 0 of 500 paths each, and nothing waited.

- Several accounts, both ways, entering after a user, drawing before and after entering: the gate is asked of
  the sum, and the order within a tick changed nothing I could find.
- Leaving users' counterparts unhedged: the owner's own account that set a user off leaves; the requirement
  jumps; the shard is called at the next visit and the user is paid. The user loses its position, not its money.
- Accounts that lose more than their margin: within the bound an account is stepped before its loss passes its
  margin (entered at 5.5 %, stepped below 4.5 %, a tick of at most 4.5 % between). Beyond the bound, or held from
  being stepped, the books say more than is there: H1.
- Whoever watches late (`the_bound_is_on_the_move_between_two_visits_of_whoever_watches`): every second tick at
  ticks of half the bound, 0 of 500; at ticks of the bound, every 2nd, 4th, 8th, 16th tick: 0, 10, 29, 32 of 500.
  The bound is on the move between two visits.

## The seven things not known

| | Detail, or does it break the claim? |
|---|---|
| 1. Books that are whole | breaks it: H2, with the four-part rule |
| 2. What tells the books of a mark | a detail if the booked entry never moves: with nothing of side B's in an account, a mark moves nothing between the sides, a leg's gain is paid when it leaves and its loss when it leaves or is stepped, both counted from the booked entry. A take is then of side A's own margin above its floor and writes no note. If entries are to move with marks as today, every settled statement writes a note and part 3 of the rule needs the last booked entry: more notes, the same order problem |
| 3. Options | stands beside it: an option's gain is no sum. The first form has no option under a shard; F67 stays open for as long as one can enter |
| 4. How an account knows its shard is called | a detail: the call's settlement writes an object that a step of an account under the shard shows, as a draw is ended by a note. One thing to decide with it: that object is shown by many steps, so it goes on as it is |
| 5. Who is paid first at a gap | decides who bears a gap, not whether the claim holds within the bound. My order below |
| 6. What it costs | it bounds what can be built: a booking, a draw and a call are each a statement on the shard, and the shard carries one at a time. One booking a dispute period is no throughput; a booking has to take many notes |
| 7. The band and the gate | part of H3: the gate and the draw at one end of the tolerance, the call at the other |

**The order I would write for the fifth.** Outside a call a note of what is owed is paid in full as it comes: a
sound shard has it. At a call nothing is paid until every row is empty. Each leaving's note is taken into a sum
(what is owed in all); losses come in. When the rows are empty what the shard holds is final, and every note is
paid the same share of itself, the fees after. Why: no order of applying then favours anybody, the owner's own
accounts cannot be first, and a share needs one more number in the shard and no list. What it does not do is tell
a user's account from the owner's: the owner's own winning account takes its share of a gap. The gate bounds how
large it can be.

## What the three rules do to an honest clearing party and its users

- **Capital.** For users whose worth is W in all: with two-party margins side B stands by a margin in each
  account, 5.5 % of W whichever way they are. Under this section, 5.5 % of the net and 0.5 % of the gross: for a book that is set
  off, 0.5 % of W, eleven times less; for a book all one way, 6 % of W. So it is cheaper exactly where the owner
  is hedged on the chain, and the owner who is hedged elsewhere pays the full rate here and must top up as the
  price moves against its net here, in the time of a visit.
- **A call because the owner was late.** Every user is closed, at prices within the tolerance, the band taken
  from what it is owed. In the model an owner that draws to its requirement is called every other tick. An honest
  owner keeps room; how much is the move it means to sit through, of its net. The user has to read that room as
  it reads the shard's terms today.
- **The gate filled by the owner's own accounts.** Its own business. A stranger cannot fill it: an entry takes
  side B's signature.
- **A stranger draining a sound shard by calls.** As the model has it, yes, wherever the shard is within the
  tolerance of its requirement: H3.

## Are F66 and F67 covered?

- **F66**: covered, where H1 and H2 are closed. An own account's leg passes the gate, so the shard holds the
  rate on it; what it wins in a visit is less than that; the call replaces the step of side B that nobody was
  paid for. Without H2 it is not (the leg the books never had).
- **F67**: stands beside it. The books are of futures legs. If an option is let in counted at the whole of its
  underlying, the gate bounds how many puts enter (6 % of their underlying's worth each), and what the two ends
  of the tolerance make of them is 0.005 % of that worth a round: it takes the owner's own room and brings a
  call, not a user's money. If options are not in the books at all, F67 is as it is.

## A first, smallest form on what exists

- Futures legs only under a shard; one committee, the terms in the id, as now.
- A shard's row gets one number more (what its legs were entered at); the draw's end (`KpfTShardEnd`) counts the
  equity from it and from the prices it already has answered for.
- A booking: a statement on the shard that takes notes of entry at prices answered for, asks the requirement
  with them, and leaves each as a booked object. `KpfTNoteApply` no longer takes a note of entry by itself.
- A leaving's note is applied with the booked object, pays from the booked entry, and takes a loss in.
- A call: a statement on the shard with the draw's trace and the other inequality; its settlement leaves the
  object a step shows. A step of an account under a called shard lets a leg leave at the call's prices.
- A step reaches an account that carries a statement (H1).

What it removes: side B's margin in a covered account and everything written to keep it there (side B's floor
and its step in `KpfTStepLiq`, the deposit of a take for B, `payb` and `addb` of `KpfTLegOut`, the two changes of
B's margin in `KpfTLegMove`, `KpfTAddC` for side B); entries that move with marks, if the second unknown is
decided that way; options under a shard; the note of what is owed paid "whatever the shard is asked to hold".

## To be decided before anything is built

1. How a step reaches an account its owner holds under statements (H1). Without it the bound is not a bound.
2. The rule of the notes (H2): the four parts, the booked object, and that a leg's cover begins at its booking.
3. The call (H3): when it is wrong, what it costs, what it pays and after whom; the two ends of the tolerance.
4. The draw of coins and of equity, and that what side B has in an account is the shard's whole (H4).
5. Whether entries move with marks (the second unknown).
6. The order at a gap (the fifth).
7. Throughput: how many notes one statement on the shard takes (the sixth).
8. Options: out of the first form, and F67 with them.

## Counts and commands

`auditT1_solvency.rs` (new): 4 tests pass; the two whose names end in the claim being broken or the call at a
hair pass while that is so in the model. `auditT1_builders.rs`: 116 pass (one new, `t1b_r42_...`).
`auditT1_tests.rs` and `auditT1_cli.rs` were not run this round (no script changed).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_solvency   # 4 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 116 passed, 0 failed
```

# Round 43

Scope: the second numbers model of a shard held to its requirement (`docs/ARCHITECTURE-CLEARING.md` section 12,
from "Review 42 attacked this section" to its end; `crates/kpf-protocol/tests/research_solvency_notes.rs`).
Numbers and paper only: no script changed and none is reviewed. I copied the second model into my own file
(`crates/kpf-protocol/tests/auditT1_solvency.rs`, a module `second`, six tests) with the rate on the gross a
number of its own, a leg booked after it entered, and three owners of my making. No contracts, no `src`, no
commit, nothing under `kpf-out`.

**The claim as section 12 words it is broken in the model's own terms by an owner that sets its users off and
leaves them (N3): the bound of a set-off book is the bound of every book, at the owner's choice. With the rate on
the gross section 12 computes, that owner leaves nobody unpaid. Two more things the model shows without counting
them: a leg that is not booked loses what it loses and is paid nothing of what it gains, which for a user is a
bet it can only lose (N1, High); and the two levels are one level for a set-off book, so the call at a hair is
still there (N2, Medium). Booking late and the share of a call I could not turn against the users.**

## Holes

### N1 (High). A leg that is not booked: what it loses is taken, and what it gains is paid by nobody

`second::what_an_unbooked_leg_loses_is_taken_and_what_it_gains_is_not_paid`

The model has a leg that lost pay the shard from its margin whether it was booked or not, and a leg that gained
be paid only where it was booked. For the owner's own unbooked account that is the point of the rule. For a user
it is this:

1. The user and the clearing party state the entry (two signatures); it is settled; the leg is in the account.
2. The booking is a statement on the shard, later, with a dispute period of its own. Until it is applied the leg
   is not booked. It may never be: the gate has no room (an owner that draws all it may leaves none), and the
   gate counts what the leg has gained since it entered, so a leg that has won needs more room and one that has
   lost needs less.
3. The leg loses: it pays the shard. The leg gains: nobody owes it. Side B has no margin in the account any
   more, and the shard pays booked legs alone.

In numbers: 100 users of 100 units, half each way, entered and not booked; a move of 2 %; all leave. The half
that gained is paid nothing of 490 KAS; the half that lost pays the shard 1,000 KAS, which held 100 and holds
1,100. The claim's measure (users unpaid of what they are owed) is 0, because nothing was owed. The clearing
party signs entries knowing there is no room, and the gate does the rest without anybody acting.

What the scripts' design must fix first. A leg that is not booked must be nothing both ways: it pays nothing and
is paid nothing, and its entry is the price it is booked at; or the entry and the booking are one act. Either way
an account must be able to tell a booked leg from one that is not without reading the shard: the booking's
settlement leaves an object for the account (the booked object, or a receipt of it), and the leg counts in the
account's rows from the statement that shows it. And a booking must be anybody's to state, the user's first.

### N3 (High for the claim as worded). An owner makes any book a set-off book, and leaves

`second::an_owner_that_sets_its_users_off_and_leaves_them_breaks_the_claim_within_its_bound`

Section 12 found that one side of a set-off book leaving is held by the rate on the gross and the band (a move
of 2 % with the terms as they are), and calls it a second bound. It is the only bound. The owner needs no user to
leave:

1. Whatever its users' net is, the owner enters an account of its own against it. The book is set off; the gate
   lets it in gladly (the requirement falls from the rate on the net to the rate on the gross).
2. It draws all it may: what the one-way book was asked is out.
3. Before a tick that goes its users' way it leaves its own account, at no loss. Its users stand one way against
   a shard that holds the rate on the gross.
4. The price moves; at the visit the shard is called and has not enough.

With the rules on, the rate on the gross 0.5 %, 1,000 paths: no user unpaid at ticks of 1.5 %; users unpaid on 38
paths at ticks of 3 % and on 92 at 4.5 %, both within the bound the claim names (the maintenance rate less the
tolerance). `Way`, `Pair` and `Unbooked` do not do this: they enter the way the price goes, never against the
users.

`second::the_rate_on_the_gross_that_makes_the_two_bounds_one_holds_against_that_owner`: with a rate on the gross
of 1.75 % the same owner leaves nobody unpaid at ticks of 4.5 % (1,000 paths); with 1.5 % it does (18 paths). So
section 12's number is right against the worst owner I could write, and the claim holds with it. What has to
change is the wording and the price: a set-off book is not asked "0.5 % where two parties' margins are 5.5 %"; it
is asked 1.75 %, and the claim's bound is the smaller of the two bounds whatever the book looks like.

### N2 (Medium). The two levels are one for a set-off book

`second::a_set_off_book_at_its_requirement_is_called_at_a_hair_and_the_capped_fee_is_all_it_held`

The upper and the lower level differ by the tolerance of the **net**. Where the net is nothing they are the same
number, and the gross is counted at the price, so it grows as the price rises. A shard drawn to its upper level
over two users of 1,000 units, one each way, holds 100 KAS; the price rises 0.01 %; it is below its lower level,
it is called, both users are closed, and the fee, capped at the rate on the gross, is 100 KAS of the 100 it held.
H3 of round 42, as it was, for the book the design makes cheapest. The cap does not help: the cap is the
requirement.

What would close it: two levels for the rate on the gross too, or the gross counted at the entries booked (it
then does not move with the price); and a cap on the fee that is a share of the requirement, not all of it.

## What I could not break

- **Booking late** (`second::a_leg_booked_after_it_gained_takes_no_more_than_the_room_the_gate_counts`). I wrote
  the booking as section 12 says it: the gate asks the equity, counting what the leg has gained since it
  entered, to be at least the requirement with it. An owner that enters accounts of its own of every size the way
  of the next tick, none booked, and after the tick books the largest the gate takes, has it leave and draws
  (acting after the price moved and before whoever watches): no user unpaid on 1,000 paths at ticks of 0.5 %, 2 %
  and 4.5 %. What it takes is the room above the requirement, which was its to draw.
- **The share of a call** (`second::the_owners_own_notes_take_no_more_of_a_failing_shard_than_they_brought`). At
  a gap of 10 %, a user long 1,000 units alone is paid 600 KAS of 890 (short 290). With an own pair of 100,000
  units each way beside it: short 240. With an own account of 100,000 units the user's way: short 290. An
  account of the owner's brings to a call what the gate asked for it (and a pair, the margin of the one that
  lost), which is at least its share. The gate is what makes the share fair.
- **Drawing between a user's leaving and the visit, and acting after the price**: the draw pays every note first
  and is of coins that are there; the late-booking owner acts in that place and nothing came of it.
- **The owner's own first, outside a call**: the model pays the owner's notes before the users' wherever it pays
  in full, and at the end. Within the bound there is enough for both.

## 2. What the model takes for granted that scripts will not give

- **A statement on the shard has a dispute period.** In the model a booking, a draw and a call are instant. In
  scripts a leg is booked a dispute period after the booking is stated (and the entry had one before it): two
  dispute periods in which the leg is live and uncovered (N1). The bound's "between two visits" is then a visit
  and the wait for the shard to be free: a call cannot be stated on a shard that carries a statement.
- **The owner's own statements on the shard hold it.** The same lever as H1, on the shard: a draw of nothing or a
  booking of no note, renewed without a gap, and no call is stated. A call must be stated over whatever the
  shard carries: a draw, a booking, and a disputed point of either (else the owner disputes its own draw and the
  shard is a disputed point for a period). Not over another call. And a draw of nothing or a booking of no note
  is no statement. A wrong call still holds the shard for a period; it costs its caller a bond each time.
- **Notes applied "before" a statement.** The model applies in order. In scripts a note is applied when somebody
  applies it, and a statement on the shard is held to the notes before it by a guard that somebody exercises in
  its dispute period (`KpfTDrawNote` today, for a draw). With booked legs the guard has to cover a booking and a
  call's absence too, and it has to count a leaving's note whether the leg gained or lost (a deposit does not
  count today).
- **A leg that left and whose booked object nobody brings.** Anybody must be able to unbook it (the note of the
  leaving and the booked object are both objects under the registry's id; no signature is needed). If nobody
  does, the books carry a leg that is gone, and it goes on moving with the price: where it "loses", the books
  show equity that nobody owes the shard. That is H1 again with no account held: the owner's own leg leaves, the
  owner does not unbook it, the price goes against it, and the owner draws and books against a loss that is not
  there. What stands against it is the guard above, exercised by somebody, at every statement on the shard.
- **One object a leg.** For an account of n legs, as far as section 12 fixes it:

  | | Transactions | Waits |
  |---|---|---|
  | The user, to be covered | the opening; n entries stated and n settled; its share of a booking stated and settled | two dispute periods a leg, and the shard free |
  | The user, to leave | n leavings stated and n settled (or n at once with side B's signature); n notes applied, each with its booked object | one dispute period a leg by statement |
  | Whoever watches | every leaving's note that its own account does not apply (the owner's, and a user's that lost); the guard at every statement on the shard; at a call one step a leg of every account under the shard | |

  And n outputs stand while the legs do, each holding the least an output may.

## 3. The two bounds

- **Are they right?** The one-way bound as stated is the accounts' bound, not the shard's: a one-way shard at its
  upper level holds the rate on the net, the tolerance and the rate on the gross (6 % with the terms as they
  are) and the band takes 1 % more off what it is asked, so it pays a move of 7 %. The 4.5 % is what an account
  that lost can pay from its margin between two visits (entered at 5.5 %, stepped below 4.5 %), which the shard
  needs wherever its book has two sides.
- **The set-off bound** is, to the first order, twice the rate on the gross and twice the tolerance. Exactly,
  with the band counted at the price after the move: x <= (2 r + 2 g) / (1 - 2 g). For it to be 4.5 % with a
  tolerance of 0.5 %, r is 1.7275 %; "half of the maintenance rate less three times the tolerance", 1.75 %, is
  that with a little to spare. The model agrees (above).
- **Several accounts on each side, of unequal sizes, where the largest leaves.** With a net of n, a smaller side
  of M and a leaving of Z out of it (all in worth), the move that is held is
  ((m + g) n + r (n + 2 M)) / (n + Z) + 2 g. It is least where the net is nothing and the whole of one side
  leaves (Z = M): 2 r + 2 g. A largest account that is less than its side leaves more held. But between two
  visits every account of a side may leave, and the owner's own account can be the whole of it (N3): the bound
  to build for is the whole side.

## 4. The honest user's side

- **A booking refused after the entry settled**: N1. Today's model leaves the user with a leg that can lose and
  cannot win.
- **A leg that gains before it is booked**: the gate counts the gain, so the more a leg has won the less likely it
  is booked, and the more it has lost the more likely. Nobody has to act for this; it is the gate's arithmetic.
  It is right for the owner's own account and wrong for a user unless an unbooked leg is nothing both ways.
- **Closed by a call at the edge of the tolerance, the band taken**: the user's leg leaves at a price up to 0.5 %
  against it, and what it is owed is counted less the band (1 % of its worth in the model). A user 4 % ahead is
  paid about 2.5 %. And it is closed not because of anything it did.
- **What a user does to be covered, and how often:**
  1. Once: read the shard (its committee, terms, periods, what it holds against what it is asked, the room).
  2. Once: open the account with the clearing party; put in its margin.
  3. Every leg: state the entry with the clearing party; wait a dispute period; settle.
  4. Every leg: see it booked (state the booking itself where nobody does); wait a dispute period; settle. Only
     now is it covered. If it is not booked, leave.
  5. All the time: watch the shard's room. Where the owner draws to its level, a small move calls the shard and
     closes the user.
  6. Every leaving: state it, wait, settle (or at once with the clearing party); apply the note with the booked
     object to be paid.

## What the scripts' design must fix first

1. **What an unbooked leg is** (N1): nothing both ways, or no entry without a booking; how an account tells a
   booked leg; a booking that anybody states.
2. **The rate on the gross** (N3, N2): a number of the shard's terms of its own, sized as section 12 computes,
   with two levels (or the gross counted at booked entries) and a fee that is not the whole of it; and the claim
   worded with the one bound.
3. **Statements on the shard**: a call over a draw, a booking and a disputed point; a draw or a booking of
   nothing no statement.
4. **A leg that left**: anybody unbooks it; the guard covers every leaving's note at every statement on the
   shard.
5. **Cost**: how many notes a booking takes, and that a call's steps do not each wait a dispute period.

## Counts and commands

`auditT1_solvency.rs`: 10 tests pass (round 42's four, and six in `second`). Those named for a hole pass while
the model has it. `auditT1_builders.rs`, `auditT1_tests.rs`, `auditT1_cli.rs`: not run this round (no script and
none of those files changed).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_solvency   # 10 passed, 0 failed
```

# Round 44

Scope: `docs/ARCHITECTURE-CLEARING.md` section 13 (the smallest form of a shard held to its requirement, as
scripts, on paper) and the rewritten second model (`research_solvency_notes.rs`). Paper and numbers: no script
changed and none is reviewed. One model test added to my own file (`auditT1_solvency.rs`, module `second`). No
contracts, no `src`, no commit, nothing under `kpf-out`.

**Two things in section 13 would make the scripts wrong, and I would not write them before both are decided. S2:
a leg is settled alone while the account's standing counts its other legs' gains, so an account is paid for the
leg that won and pays only its coins for the leg that lost; in numbers a user is paid nothing of 3,324 KAS with
ticks of 2 % and no step or call ever due. S1: an entry is stated at one time and its gate is asked at another,
so whoever decides whether the gate holds has a free option on every entry for as long as the entry may wait,
and there is no limit to that wait. Three smaller ones follow (S3 to S5). What review 43 asked for is in: there
is no unbooked leg, and the rate on the gross holds against the owner that sets its users off.**

## Holes

### S2 (High). A leg is settled alone; the standing that lets it leave counts the others' gains

`second::an_account_whose_legs_are_settled_alone_is_paid_for_the_one_that_won_and_pays_its_coins_for_the_one_that_lost`

Section 13: a leg's gain or loss "counts for the user's standing and moves no coin until a leg leaves"; a leaving
"is settled alone, from its entry to the price stated"; a gain is paid by the shard to the user's key, a loss is
paid out of the user's margin. So within one account a gain is coins from the shard, and a loss is coins only
as far as the account has them, while each stands for the other in the account's standing.

The sequence, with keys of the owner's own:

1. One account, two legs of one market, 10,000 units each way, and the maintenance of both as its coins. The
   gate takes them gladly (a net of nothing).
2. Whatever the price does, the account's standing is its coins: what one leg gains the other loses. No step of
   it is ever due. In the books the two legs are nothing at any price, so no call is due either.
3. The price drifts up 34 % in ticks of 2 % (within the bound, a visit every tick). The owner tops its shard up
   to the upper level every tick, as an honest owner would.
4. Its long leaves, settled alone: the shard is asked 33,240 KAS and pays all 10,558 it holds, every coin the
   owner had put in.
5. Its short is alone with the account's coins: it lost 34,586 KAS and pays 14,804, which goes to the waiting
   note of the long.
6. A user long 1,000 units beside it, owed 3,324 KAS, is paid nothing.

The same without two legs of one market: any account whose legs gain in all (a winner and a loser of two
markets). And section 13's take (kind 2: "of its own coins, standing at its floor after") is the short form of
it, which the coordinator asked about: a user with 10 of coins and 100 of gain not yet paid takes its 10 out and
stands on the gain; its losing legs then owe the shard out of an account with no coins. The model's bound on a
loser (it pays from its margin within a tick) is a bound on an account of one leg. It is not true of an account
of two.

It also empties the bond: a statement's bond is a rate of the account's coins, and an account that stands on
gains has none, so a false statement on it costs nothing.

**The rule I would write.** An account is settled as one thing where coins leave it, whatever leaves the trace:

- What may leave an account, by a take or as a leaving leg's gain paid to the key, is what leaves its **coins**
  at or above its maintenance **and** the losses its standing legs show at the prices stated. A standing leg's
  gain is not coins and does not count for this.
- A leaving leg's gain beyond that stays owed to the account, not to the key: either the leg does not leave by
  its user's statement until the user has put the coins in or let the losing legs leave first, or the note pays
  into the account. The first needs no new script.
- A step of a liquidation, and the steps of a call, let every leg of the account leave in one statement, and the
  shard is asked the account's gains less its losses beyond its coins, once. ("The leg is its largest, or it
  stands at its maintenance after" lets the largest leg be the one that won.)

With this the books' equity is right again for an account of several legs: what an account owes beyond its coins
is taken from what the shard owes it before a coin leaves.

### S1 (High). An entry is stated at one time and its gate is asked at another

The entry states prices at a time t0; a dispute period later (or any time after: section 13 names no limit) it
is applied with the shard, the gate asked at the prices of t0 against the rows as they then are. The leg's entry
is the price of t0. So when it is applied the leg has already gained or lost what the price did since, and the
gate does not count it (round 43's late booking held because the gate counted the gain since the entry; here it
is asked at the old prices, where that gain is nothing).

Whoever decides whether the gate holds at the applying has an option on the entry. That is the owner: the room
above the upper level is its own, to add to at once (`KpfTAddC`) and to leave at nothing.

- **Against a user.** The owner keeps no room. A user and the owner sign an entry; the dispute period passes. If
  the leg has lost, the owner adds the room and applies it: the user is in at a loss. If it has gained, the gate
  fails and the entry ends. The user has no say: an entry ends only where the gate does not hold.
- **For itself.** Two accounts of its own, an entry each way at one moment's prices, as large as the room lets
  in. After the wait it applies the one that won. Within the bound over the wait this takes no more than the room
  it had (the leg brings a requirement of 7.75 % of its worth and has gained less than 4.5 %), so it is a draw by
  another door. With no limit to the wait it is not: an entry stated today and applied after the price has moved
  by more than its requirement takes the users' part of the shard.
- **By the shard being held.** An entry is applied by spending the shard. While the shard carries a draw's
  statement, or is called, no entry is applied or ended, and every account with an entry waiting is held under
  that entry (its user's other legs cannot leave and it takes no step but over "a statement that is none"). The
  owner's own draws hold its users' accounts.

What must be decided, in the form as it is:

1. An entry is applied within a time after its dispute period that the shard's terms say, and is ended by anyone
   after it. The bound's "time between two visits" then includes the dispute period and that time, and the terms
   have to say so.
2. The user ends its own entry before it is applied (and the owner too): the option is then nobody's alone. What
   it costs is that an entry completes only where the price has not moved enough for either to walk away.
3. An entry is applied or ended over what the shard carries (a draw's statement), or a draw is no bar to it.

And the form in which there is no option: the entry is one transaction with the two signatures, the shard and the
committee's citations of the leg's market and the rows': the price the leg enters at is the price the gate is
asked at, at that time. Section 13 lists it as wanted later ("a citation in place of the wait"). It is what I
would build first for the entry; see the last answer.

### S3 (Medium). Prices stated for "each of the shard's rows", and a row that changes hands

A row is taken by the first leg of a market and is no market's when its last unit leaves. An entry that states a
price for row 2 at t0, when row 2 was market X, is applied when row 2 is market Y (X's legs left, an entry of Y
was applied in between): Y's net is then counted at X's price. With X at 10 and Y at a hundredth of that, a shard
that is long Y is counted at a hundred times what its net is worth, and the gate lets in what it cannot carry.

Fix: the entry states its prices by market (the row's name), and the applying holds each stated name to the row
it finds; a row the entry has no price for ends the entry.

### S4 (Medium). A call that cannot empty, and steps nobody is paid for

- "Its share once no row has a unit": one account that nobody steps keeps a row from emptying, and no note is
  paid. At a call that has not enough, the fee of a step is paid "after the users, of what is left", which is
  nothing: nobody but a user who waits for its share has a reason to step the owner's account, and it does so
  for the cost of a statement an account.
- The owner holds its own account under statements of its own. A step goes over "a statement that is none". A
  leaving stated at a false price is a statement; it is answered wrong, at the cost of its bond, which is a rate
  of the account's coins (nothing, after S2; little, in an account kept small). Each has to be disputed and
  answered with a citation before a step gets in.
- The steps' prices are not said. If a step under a call states prices of its own time, the price goes on moving
  against a shard that is known to be short for as long as the steps take.

What I would write: under a call a step goes over any statement of the account; the legs leave at the call's
prices (the call's object carries them), so a step has no price to dispute and what is owed in all is fixed when
the call is; a first, flat part of a step's fee is paid before the users (a step that nobody is paid for does not
happen); and a call has an end in time: after it, what the unstepped legs would be owed at the call's prices is
set aside at the same share, and the notes are paid.

### S5 (Medium). The requirement's arithmetic

- **Overflow.** A row's units and the number of what the legs were entered at are sums over every account under
  the shard, with no count of them. A sum beyond 64 bits fails the script whichever way it is asked: an entry
  whose applying overflows can be neither applied nor ended, and its account is held for good (F58, now at the
  gate). The applying has to hold a row to a bound before it adds (a row's gross at most what one leg may be: its
  worth at the market's limit is then computed), and treat "beyond the bound" as the gate not holding.
- **The number of what the legs were entered at** falls at a leaving by exactly what it rose by at the entry only
  if both use the same quote of the same leg (its size, scale and entry): the note carries the leg whole, so it
  can. The row's worth is the quote of a sum and the number is a sum of quotes: they differ by less than a sompi
  a leg, the way that depends on which side the net is.
- **Rounding.** Against the owner at the upper level (the gate, a draw), for the shard at the lower (a call):
  else a draw that leaves the shard at its level and a gate that fails by a sompi, or a call that is right by a
  sompi.
- **A row nobody prices.** An entry of a leg whose market the committee does not price has a price point nobody
  answers; where it is disputed it lapses and costs its claimers their bond (round 39), and where nobody
  disputes it is applied. After that every statement that states the shard's rows has that point: every later
  entry can be made to lapse (half its bond to whoever disputes), a draw lapses, and so does **a call**. A shard
  with one such row cannot be called. To decide: what the lapse of a price point of a call does (I would let the
  call stand: the owner is the one who can ask the committee), and that an entry whose row's price lapses is
  not only ended but leaves no row behind (it does not, being ended; the hole is the entry nobody disputed).

## What holds

- **No leg that is not booked.** With the entry applied in one transaction with the shard, and with `KpfTOpenC`
  opening no leg, `KpfTLegAdd`, `KpfTLegMove` and `KpfTLegOut` out, and a settlement writing only legs its rows
  had, every leg of a covered account came through the gate (read). The exception is the one every statement has:
  a statement whose outcome is false and that nobody disputes writes what it says.
- **A note is applied once**: it is an output.
- **The rate on the gross**: the model's fourth owner and mine (round 43) leave nobody unpaid at ticks of 4.5 %
  with 1.75 %.
- **Two entries in one block**: each spends the shard, so one a block; the others try again. The shard is not
  held by an entry that waits (the entry is on the account).
- **What the shard needs to know what it owes at a price**: what it holds, each row's net, and the one number;
  less the notes that are not paid. Entries that do not move make a mark tell it nothing (given S2's rule, which
  is what makes an account's loss beyond its coins not the books' equity).

## The questions

**1. Applying an entry.** S1 and S3. What changes between stating and applying: other entries applied (the room
is gone: the entry ends, fair), leavings' notes applied (the rows fall), a draw (the room is gone, by the owner's
choice: S1), a call (nothing is applied while called), a row changing hands (S3). An entry that can be neither
applied nor ended: by overflow (S5), and while the shard is held (S1).

**2. A leaving's note with no booked object.** Not applied twice. Not made for a leg that did not come through
the gate, short of an undisputed false statement, which now also asks the shard for whatever it says (the note's
amount is the statement's). The number falls exactly (S5). Nothing else is needed for the equity (above).

**3. Gains as standing.** S2. The rule is a floor of coins against the legs that lose, and one settlement an
account wherever coins leave it.

**4. The call.** S4.

**5. The arithmetic.** S5.

**6. What it costs, against the reserve as built.**

| | As built | Section 13 |
|---|---|---|
| A leg enters | a statement of the two; its dispute period; a settlement | the same statement; its dispute period; the applying with the shard (one a block a shard, none while the shard carries a draw or is called) |
| A leg leaves | a statement, its period, a settlement; or at once with the clearing party's signature; the note applied | a statement, its period, a settlement, the note applied. No leaving at once |
| A gain taken out | a take: a statement, its period, a settlement | not without leaving |
| A leg moved, an option | there | not under a shard |
| The clearing party | a margin in each account or none; a draw is a statement; nothing holds it between draws | no margin in accounts; room in the shard for every entry (7.75 % of a one-way leg, 2.25 % of each side of a set-off pair, at the upper level); a top-up in the time of a visit as the price moves, or its users are closed |
| Whoever watches | every statement on every account and on the shard | the same, and: every leaving's note applied; the guard at every statement on the shard and at every applying; a call, and a statement an account under it |

**Would I build this, or something else?** The shard's side, yes, as it is: the rows with their number, the
equity and the two levels, the draw of coins, the call, the notes of a leaving. That is the part reviews 37 to 43
were about, and nothing I found this round is against it. The account's side I would build with S2's rule from
the start; it is not an addition to the form, it is what makes the books true. The entry I would build as the
transaction with citations: it has no option for anybody, it holds no account for a dispute period, it asks the
gate at the price and time of the entry, and it has no row that changes hands between two moments. Its cost is
that the committee is asked at every entry and not only at a dispute, which the design has refused so far; an
entry is the one place where a price becomes a debt of the shard, and that seems to me the place to be asked. If
that stays refused, the statement form with S1's three decisions, and the option said plainly to its users.

## The order I would have the scripts written and reviewed in

1. **The shard's arithmetic alone**: rows with the number, the equity and the two levels in the draw's end; the
   bounds and the rounding of S5. Review: the script against the model on random books, at the bounds.
2. **The notes of a leaving and a deposit**, and the guard that a note not applied ends a statement on the
   shard. Review: the books under any order of notes equal the model's.
3. **The covered account without side B's margin**: entries that do not move, the take and the leaving under
   S2's rule, a step that lets every leg leave and goes over a statement. Review: the script against the model on
   accounts of several legs, and S2's account.
4. **The entry with the shard**, in the form decided for S1, with S3's names. Review.
5. **The call, its steps and its end**, with S4's decisions. Review: who is paid what, and a call that is held.
6. **What goes**: `KpfTLegAdd`, `KpfTLegOut` and `KpfTLegMove` under a shard, side B's margin and its rules.

Before 3 and 4 the model wants two things it does not have: accounts of more than one leg, and an entry that is
applied a time after it is stated.

## Counts and commands

`auditT1_solvency.rs`: 11 tests pass (one new, S2's). The other audit suites were not run (no script changed).

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_solvency   # 11 passed, 0 failed
```

# Round 45

Scope: stage 1 of the smallest form (section 13), uncommitted: the shard's arithmetic. A shard's moving part has
its basis at `[121..129]` and whether it is called at `[130..138]`; `KpfTShardOpen` asks both nothing;
`KpfTShardEnd` judges a draw by the equity (what the shard holds, what side B's net units are worth, less the
basis) against the requirement at its upper level, and not while called. Model: `reserve.rs` (`gross_rate`,
`need_of`, `Shard::{level, equity, draw_end_judged, draw_max}`). Everything below is engine-run
(`verify::verify`) unless it says "read". Only `crates/kpf-protocol/tests/auditT1_builders.rs` and this file
changed; no contracts, no `src`, no commit, nothing under `kpf-out`.

**The script's arithmetic is what it is said to be: on 360 draws over random books the engine takes the verdict
of that arithmetic written out, and neither where a number does not fit. Four findings. F68 (High while stage 1
stands alone): nothing writes the basis yet, so a shard that is long is drawn empty by a draw that is right. F69
(Medium): a row nobody prices is now equity at whatever price its owner states. F70 (Low): a shard is opened with
the eight bytes the engine reads as nothing in the place of either new number. F71 (Low): the model says wrong
where the script gives no verdict.**

## Findings

### F68 (High, while stage 1 stands without the stages that write the basis). A shard that is long is drawn empty

`t1b_finding_f68_until_a_note_moves_the_basis_a_shard_that_is_long_is_drawn_empty`

The equity is `held + what the net units are worth - basis`, and nothing that exists writes the basis: the notes
of an entry and of a leaving are the notes of before. So a shard with legs has a basis of nothing, and its
equity counts the whole worth of its net units as a gain where it is long and as a loss where it is short.

Run, with the scripts as they stand in the tree: a shard of 50 KAS; a user short 10 units against its owner (by
`KpfTLegAdd`, its join applied): the shard is long 10, its row worth 100 KAS. Its equity reads 150 KAS, it is
asked 7.75, and the model's most is all it holds. The draw of all 50 KAS is stated; it has no wrong point at the
committee's price; its end is answered right and not wrong. Before stage 1 the same shard gave 44.47 KAS; with the
basis the leg's entry makes (set by hand), 42.25. The other way (a user long 10): the equity reads 50 below
nothing and nothing is drawn.

So to the fourth question plainly: between stage 1 and the stage that writes the basis, a draw is wrong for every
shard that has a leg, in the owner's favour by the whole worth of its long rows and against it by the whole of
its short ones. If stage 1 is committed and anybody uses the reserve before stage 2 and the entry are in, the
owner of a shard whose users are short takes all it holds. Either the stages land together, or until they do
`KpfTShardEnd` refuses a draw from a shard that has a row and a basis of nothing.

The test passes while the join does not move the basis, and says "F68 is closed" where it does.

### F69 (Medium). A row nobody prices is equity at the price its owner states

`t1b_finding_f69_a_row_nobody_prices_is_equity_at_the_price_its_owner_states`

Before stage 1 a false price for a row nobody prices took that row's requirement away and no more (round 35:
"as if the row were not there"). Now what side B's net units are worth is in the equity. Where the shard is
short in such a row, a price of next to nothing makes what it owes there nothing.

Run: a shard of 50 KAS, short 10 units of a market the committee prices and 1,000 units of one it does not, the
basis as the entries make it. At the entries' prices it holds 50 and is asked 782.75: no draw. With the second
row stated at a sompi its equity reads 10,049 KAS and the draw of all 50 has an end that is answered right. The
price of that row has no answer (no citation is of it); a dispute of it lapses, the shard again and both bonds
back, as any lapse of kind 5. The owner states it again.

Round 44 named this (S5, last point) for the call. It is there for the draw as soon as the equity is: whoever
watches such a shard disputes every draw for nothing, and one draw unseen is all the shard holds. What lets such
a row in is an entry nobody disputed (today `KpfTLegAdd`, which asks nothing). To decide with the entry and the
call: what the lapse of a row's price does to a statement on the shard. For a draw I would have it cost the
claimer its bond, as it does for an entry: a draw's prices are its owner's to show.

### F70 (Low). A shard is opened with the bytes of "nothing, below zero" in the place of either number

`t1b_finding_f70_a_shard_is_opened_with_the_eight_bytes_the_engine_reads_as_nothing`

`KpfTShardOpen` asks `int(moving[121..129]) == 0` and the same of `[130..138]`. The engine reads the eight bytes
`00 00 00 00 00 00 00 80` as nothing. A shard is opened with them in the place of the basis, of the call, and
(as before this stage) of side B's margin: three openings taken. Every script reads those bytes as nothing
after, so no rule changes. But the shard's state is the hash of its bytes: a tool that reads the two numbers as
nothing and writes them back as eight zero bytes has another hash and does not find the shard, to apply a note to
it or to dispute its draw. An owner that wants its draws undisputed by tools that keep numbers opens its shard
so.

Fix: the opening holds the bytes, not the numbers (`moving[121..138]` is seventeen given bytes; and side B's
margin likewise).

### F71 (Low). The model says wrong where the script gives no verdict

`t1b_finding_f71_the_model_says_wrong_where_the_script_computes_an_equity_that_does_not_fit`

`KpfTShardEnd` computes what is left and the equity whatever it found before; `Shard::draw_end_judged` stops at
the first thing that is wrong. On a shard whose equity does not fit (a basis at the end of 64 bits), a draw that
is wrong for an earlier reason (not the owner's, an amount of nothing, a price out of its limits, more than is
held, a shard that is called) is "wrong" by the model and has no verdict at the engine: the answer the tool
would give is refused, and the dispute lapses. 14 of the 360 random draws; and one written out (a sompi more than
a shard holds, its basis the largest number).

With it, something to know of the arithmetic: the engine's numbers are of 63 bits and a sign. A sum that comes to
the least 64-bit number fails the script, where a 64-bit sum does not overflow (some of the random draws showed
it before the written-out arithmetic counted it).

## What held under attack (all engine-run)

| Item | Test | What was put to the engine |
|---|---|---|
| 1. The script against the model | `t1b_r45_the_end_of_a_draw_is_the_models_on_random_books` | 360 draws, every one stated and its end disputed. Half on books a shard could have, half on any: rows of either sign; a gross that is nothing, half the net, the net's other sign, at the ends of 64 bits; units up to those ends; scales of 1, 100, the market's, the most, nothing and below; limits of 1 and of the largest number; prices of 1, the limit, one beyond, nothing and below; a basis of either sign and at the ends; a shard that is called (a time, and minus one); terms with a maintenance rate of 0, 100, 149, 151, 500 and 4,999 and a tolerance of 0, 1, 50 and 9,999 (the rate on the gross nothing in five of the seven); a draw of the most, a sompi more, a sompi, all that is held, a sompi more than that, nothing, minus one; by the owner and by another. 30 right, 187 wrong, 143 with no verdict: each what `KpfTShardEnd`'s arithmetic written out in the test says, the other answer refused. `draw_end_judged` agrees on all but F71's 14 |
| 2. The level, to the sompi | same | a shard short 10 units, the basis of its entry: the most is what it holds less 7.75 % of 100 KAS, right; a sompi more, wrong |
| 2. Rounding and signs | `t1b_r45_a_short_rows_worth_is_rounded_down_which_is_for_the_owner` | below |
| 3. The outputs | the first test | a draw whose outcome is the shard with another basis (a KAS less, nothing, the least but one) or called has a wrong end: 4. A draw of a sompi from a shard that is called: wrong, and the model's most is nothing |
| 3. Who writes the two numbers | `t1b_r45_nothing_that_exists_writes_a_shards_basis_or_calls_it` | 15 refused: a shard opened with a basis of 1, minus 1, the least but one, called 1 or minus 1, or both; a note applied (a join) to a shard that has a basis, and to one that is called, with either number changed in the shard that goes on. The note itself is applied and leaves both as they were |

**Rounding and signs (the second question).** The worth of a row's units is rounded down in its size, then
signed.

- A row side B is long in: its worth counts a part of a sompi less than it is. Against the owner.
- A row side B is short in: what it owes counts a part of a sompi less than it is. For the owner. Run: 15 parts
  of a unit short at a price of 7 a ten of them is 10.5 sompi, counted 10; the draw of the most leaves 1.5 sompi
  of equity where 2 are asked, and is answered right.
- The requirement's two rates are each rounded up. Against the owner.

So a draw the script takes leaves the true equity under the level by less than a sompi a short row (under four
in all), and a draw it refuses may be one that leaves it over by less than a sompi a long row and two for the
rates. Whether it should be otherwise: at the upper level a short row's worth should be rounded up, and when the
lower level is written (the call) each rounding should be the other way, so that between a draw that is right
and a call that is right there is never the same sompi. It is sompi; I would do it with the call, not before.

**The third question, read.** A shard's moving part is shown to `KpfTAddC` (it writes the margin at 67 and the
rest as it was), `KpfTNoteApply` (what it holds, and the rest as it was: run), `KpfTChange` stating a draw (it
reads the two margins for the bond; side B's is nothing), `KpfTShardPrice` and `KpfTDrawNote` (they end a
statement and write the shard as it was; changed this stage in their comments only), and a settlement (it writes
what the draw stated: the end holds that to the shard with what it holds less, the rest as it was: run). None
reads `[121..138]` as asks: the scripts that read asks (`KpfTHand`, `KpfTStepEnd`, `KpfTStepEnter`) open an
account with a second key, and a shard has none. Nothing that exists calls a shard or moves its basis, an
undisputed false draw aside (its outcome is what it says).

## Brought along

The two tests that failed by the requirement's new numbers: in `t1b_finding_f61_...` what a draw gives is counted
on the shard with the basis its leg's entry makes (42.25 KAS of 50); in `t1b_fixed_f59_...` the shard of three
rows has the basis its legs' entries make, set by hand, and the draw at the least price for the made row is
asserted to be of all the shard holds (F69).

## Not reached

- The lower level and a call: not written.
- `KpfTAddC` on a shard at the engine (read).
- A shard's `called` set to a time and anything that would read it but the draw: nothing does yet.
- The commands (`aobj.rs` reads the two numbers: not run).

## Findings ledger

| | Severity | State |
|---|---|---|
| F68 | High | open while stage 1 stands without what writes the basis: a shard that is long is drawn empty |
| F69 | Medium | open: a row nobody prices is equity at the price stated, and a draw's lapse costs nothing |
| F70 | Low | open: a shard is opened with the bytes of nothing below zero for its basis, its call, side B's margin |
| F71 | Low | open: the model says wrong where the script gives no verdict |
| F66, F67 | Medium, High | open on purpose (the reserve as built; section 13 replaces it) |
| F61 | High | open until `KpfTLegAdd` is removed |
| F60, F62 to F65 | | fixed in rounds 37 to 41 (F65 for futures legs) |
| F58 | Low | closed in the tool; open in the scripts by decision |
| F41 | High | open, with the owner |
| all others | | as in round 41's ledger |

## Counts and commands

`auditT1_builders.rs`: 123 tests pass (7 new: four findings and three `t1b_r45_...`; two brought along). A
finding's test passes while the hole is open and says "is closed" where it would fail. `auditT1_tests.rs`: 55
pass, unchanged. `auditT1_cli.rs`: 47 pass, unchanged, run once.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 123 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 46

Scope: stage 2 of section 13 (the notes of a leaving: `KpfTLeft`'s `apply` and `rest`, and the two new note
kinds in `KpfTDrawNote`) and the round 45 fixes (F69, F70, F71; F68 left open on purpose). Everything below is
engine-run (`verify::verify`) unless it says "read". I edited only
`crates/kpf-protocol/tests/auditT1_builders.rs`, `crates/kpf-tests/tests/auditT1_tests.rs` and this file; no
contracts, no `src`, no commit, nothing under `kpf-out`.

**No finding. The three round 45 fixes hold, and stage 2 does what it is said to: the script and the model agree on
240 random notes; the three note shapes do not collide; a leaving's note is applied once, to the rows it names,
and pays its key. One thing to carry into stage 3, run at the engine and recorded: a `Left` note whose legs are
in no row ends a draw and can never be applied, so stage 3's settlement must write a kind-6 note only for legs
that are in the shard's rows.**

## The round 45 fixes, verified

- **F69** (`t1b_fixed_f69_a_draw_whose_price_nobody_answers_costs_its_claimer_the_bond`): `KpfTPoint`'s lapse now
  treats a draw (kind 5) as an entry. The draw of all over the row no committee prices still has a right end (the
  equity at the stated price is as it was), and the dispute of that price, which nobody answers, ends it: the
  disputer has its bond and half the claimer's, the copy the other half, the shard as it was. So the equity is not
  the hole; what holds the draw is that its price is its claimer's to show.
- **F70** (`t1b_fixed_f70_a_shard_is_opened_with_eight_zero_bytes_for_its_basis_its_call_and_side_bs_margin`):
  `KpfTShardOpen` now holds the bytes of the basis, the call and side B's margin to eight zero bytes. A shard
  opened with the bytes of nothing-below-zero in any of the three is refused (0 taken, where 3 were).
- **F71** (`t1b_fixed_f71_the_model_gives_no_verdict_where_the_script_computes_an_equity_that_does_not_fit`):
  `Shard::draw_end_judged` now computes what the script computes in its order and gives no verdict where the
  equity does not fit. The random-books test (`t1b_r45_...`) no longer counts a single draw where the model and
  the script part (0, where round 45 recorded 14).

## Stage 2, verified

- **The script against the model, random books**
  (`t1b_r46_a_note_that_legs_left_is_applied_as_the_model_says`): 240 notes over shards of one to eight booked
  legs of four markets, either direction, sizes from a part of a unit to the bound and entries from 1 to the
  limit, held and called at random. The note of one to six of those legs in any order, and notes that are not of
  the books (a leg of another size, entry, direction, market; the same leg twice; no leg; seven legs); asked
  nothing, less than is held, more, below zero. For each the engine takes the transaction the script's arithmetic
  written out gives and no other (`left_as_the_script46`), and `Shard::left` gives the same: 99 applied, 141
  refused, **0 where the model and the script's arithmetic part**.
- **One note, to the sompi** (`t1b_r46_a_note_that_legs_left_is_applied_once_to_the_rows_it_names_and_pays_its_key`):
  two of three legs leave, 20 KAS paid and 30 the rest; the third stands in its row, the emptied market's row is
  no market's, the basis falls by the two legs' entered worth. 28 other transactions refused: the payment a sompi
  short or to another key; the shard holding a sompi less, with the legs still in, with the basis as it was; the
  rest note of another amount, key or shard, under no id, below its least, or the leaving's note in its place; a
  note of the rest where all is paid; the rows named the other way, both the first, an empty row, one beyond the
  fourth, below the first, for one leg, three for two; the legs in the other order, one of two, the wrong third
  leg; another key or amount than the note's. While called: a note that asks something is refused, one that asks
  nothing is applied (the legs out, nothing paid). A leg twice in one note is applied only where it is twice in
  the books.
- **The rest** (`t1b_r46_a_note_of_the_rest_is_paid_once_to_its_key_as_far_as_the_shard_holds`): a rest of 30 KAS
  paid in full by a shard of 50, and 20 of it by a shard of 20 with a note of 10 going on; 13 other transactions
  refused (the payment, the shard, the note that goes on of another amount or key or under no id, a note where all
  is paid, a shard that holds nothing, a shard that is called, a note of nothing).

## The questions

**a. Who can write a kind-6 or kind-7 note; hash-domain separation**
(`t1b_r46_the_three_note_shapes_do_not_collide`, `t1b_r46_an_owed_note_is_an_snote_that_kpftleft_does_not_read`).
A note object's state is the hash the creating script writes, and a leg is taken out of the books, or a note is
paid, only where `KpfTLeft` reconstructs that hash from what is shown. The three preimages differ in their tag
(the third push: 1 to 4 for an `SNote`, 6 for a `Left`, 7 for a `Rest`) and in length (195, 126, 93 bytes), so a
forged note of one shape is not read as another short of a blake2b collision. Checked: 81 note hashes of one
shard, over every kind and several heads, keys and amounts, are all distinct; an `SNote` built with kind 6 or 7
is still a 195-byte preimage and has no `Left` or `Rest` hash. And the one writer that pays from a shard today,
`KpfTNoteApply` of an owed note (an `SNote`, kind 4), makes no kind-6 or kind-7 object: its note is applied and
paid by `KpfTNoteApply`, not by `KpfTLeft` (which refuses to read the owed note's state as a `Rest`). Nothing
else (a join by `KpfTLegAdd`, a deposit, a settlement, `KpfTShardOpen`) writes a `Left`- or `Rest`-shaped state.
So no kind-6 or kind-7 note can exist today, and `KpfTLeft` and `KpfTDrawNote`'s new clauses cannot be reached
with a real object until stage 3 writes one.

**b, d. `apply`.** Covered by the two tests above. The row named must be the leg's market's futures-tagged head;
a row goes no-market only where its net and gross are both zero; a row short of the leg's units is refused
(`rg >= 0`); the basis moves by the leg's entered worth, signed as side B's; the same leg twice needs it twice in
the books; `at` of the wrong length is refused by the builder's shape; the payment is one output at
`len - 1 - 4j` to the key, and the shard's value falls by exactly what is paid. Overflow of a row or the basis:
`left_as_the_script46` carries `checked_*` and the engine's 63-bit-plus-sign range, and the random books reach
the bound sizes; a sum that does not fit gives no verdict (as a row's).

**c. `rest`.** The third test. Paid once (the note is an input, spent); to the note's key alone; a note of
nothing or of a called shard is refused; partial payment leaves a note of exactly the rest.

**e. `KpfTDrawNote` with kinds 6 and 7**
(`t1b_r46_a_draw_is_ended_by_a_note_of_this_shard_and_the_settlement_must_only_write_appliable_ones`). A draw is
ended by a `Left` note only where the note is of this shard's sid (the preimage uses the shard's own
`moving[138..171]`), is an object under the registry's covenant id (`OpInputCovenantId(n) == id`), and landed
before the statement. Refused: another sid; a note not older than the draw. **But** the script reads only the
chain, the key and the amount from a kind-6 note; a `Left` note whose legs are in no row of the shard ends the
draw all the same, and `KpfTLeft.apply` can never apply such a note (a leg in no row is refused). Run at the
engine (the `Left` object built by hand, since nothing writes one). So, to the question: a stranger cannot grief
a draw today (no kind-6 note exists); **what stage 3's settlement must guarantee** is that a kind-6 note is
written only for legs that are in the shard's rows, so that every note that can end a draw can also be applied to
end the dispute. A note the shard's books do not have must not be a note that ends its draws.

**f. F69's fix and an honest owner**
(`t1b_r46_a_disputer_of_an_honest_draws_row_loses_its_bond_not_the_owners`). The lapse that costs the claimer is
reached only where nobody answers the point. For a row whose market a committee prices, the owner (the claimer)
answers the price right with one citation of the committee's price at the statement's time — which already landed,
so the owner cannot be made to wait for it — and the disputer's bond is lost to the owner, not the owner's to the
disputer (run: the right answer is taken, the wrong is refused, and the disputer is not paid its bond back). A
disputer cannot delay that citation: the price at t0 is in the past, and the citation of it is a note that is
already on the chain. So F69's fix does not let a disputer take an honest owner's bond; the half-bond lapse is
only F69's own case, a row no committee can price.
(Not shown in this harness: that the owner is not forced to let the point lapse before the window. The unit
builders self-attest the age argument, so a lapse's timing is not modelled here; the owner's answer being
available is what the test shows.)

## What was not reached

- A kind-6 or kind-7 note written by a real settlement: stage 3, not yet built. Items e and the "appliable"
  requirement are run with a note object built by hand.
- F68 stays open on purpose (a shard with legs has a basis of nothing until stage 4 and the removal of the old
  entry; recorded in `docs/ARCHITECTURE-CLEARING.md` 13 "Built so far"). The round-45 tests that carry a basis
  set it by hand.
- `KpfTLeft` with options or a leg of no kind: under a shard there are only futures legs in this form; a leg of
  no kind in a note is refused by `book_of`/the row-head check, which the random books reach.

## Findings ledger

No new finding. F69, F70, F71 fixed and verified. F68 open on purpose until stage 4. Otherwise as round 45's
ledger.

## Files changed and counts

- `crates/kpf-protocol/tests/auditT1_builders.rs`: 130 tests pass (5 brought along to the fixes; 6 new
  `t1b_r46_...`).
- `crates/kpf-tests/tests/auditT1_tests.rs`: 55 pass (the table at 56 scripts, 64 entries, 54 `release`).
- `crates/kpf-cli/tests/auditT1_cli.rs`: 47 pass, unchanged, run once.
- `docs/audit/REVIEW-T1b.md`: this round appended.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 130 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 47

Scope: stage 3 of section 13 (a covered account's own statements: `KpfTCRow`, `KpfTCEnd`, kinds 7 to 9 in
`KpfTChange`, `KpfTPoint`, `KpfTStepPrice`; the note of a leaving that holds what the account paid; `covered.rs`)
at the engine; stage 4 ("an entry stated with the shard") on paper; the two tests that failed, brought along.
Working tree on eb50dda with stage 3 uncommitted. No src, contract or `kpf-out` file touched; no commit.

## Headline

- The two scripts agree with their model. `KpfTCEnd` against `Account::c_end` on 180 statements (the rule's own
  and malformed ones, sums at the ends of 64 bits included) and `KpfTCRow` against `c_row_judged` on 180 rows:
  the engine takes exactly the model's verdict, and neither where the model gives none. 0 partings.
- Every note that a statement of the rule's writes on an account of up to six legs is applied by `KpfTLeft.apply`
  to books that have the legs (56 applied at the engine, 39 of them holding what the account paid, 12 leaving a
  note of the rest; 3 waited on a called shard, as they should).
- Four findings in stage 3 as built: **F72 (High)** the note of an account of more than six legs is applied by
  nothing; **F73 (Medium)** a step that takes one leg takes its fee before what the legs that stay have lost;
  **F74 (Medium, a lock at stage 6)** one sompi of side B's makes every statement of kinds 7 to 9 wrong;
  **F75 (Medium)** a note's least stays with the user, so the coins are good for 0.2 KAS less than every test
  takes them for, and for nothing where they are 0.2 KAS.
- The invariant "no act makes what the shard is good for fall" holds for a take and for a leaving on accounts
  that are not small, at the prices stated (21,829 statements on the model the scripts are held to). It does not
  hold for a step (F73) and for a note's least (F75). What it misses is time: see item h.
- Stage 4 on paper: the marked account as described is taken by every script that takes an account (run: P1);
  the cost of the pending leg is that the books are blind for as long as an entry is pending, which nothing bounds
  yet (P2). I no longer prefer citations at every entry (answer 4).

## Findings

### F72 (High). The note that more than six legs left is applied by nothing

`KpfTCEnd` reads any number of rows; where every leg leaves, the note is of the account's own chain. An account
holds up to 64 legs (`LEGS_MAX`; `KpfTLegAdd` and the entry of today count none). `KpfTLeft.apply` takes a note
of six legs at most (`legs.length <= 666`), and so does `Shard::left`.

Sequence (`t1b_finding_f72_the_note_that_seven_legs_left_is_applied_by_nothing`): seven legs long 10 units from
10 in four markets, an account of 100 KAS, a shard of 500 with the legs booked. The user states that every leg
leaves; the statement is the rule's (its end is answered right and not wrong at the engine), and it is applied.

| Prices | The note | The user | Then |
|---|---|---|---|
| 3 % down | asks -21 KAS, holds 21.2 KAS | paid 79 KAS | the 21 KAS the account paid never reach the shard |
| 3 % up | asks 13.79 KAS, holds 0.2 KAS | paid its 100 KAS | the 13.79 KAS are never paid |

In both: no transaction applies the note (the model refuses; written by hand with the rows named and the books
empty, the engine refuses it); the seven legs stay in the shard's rows and basis for good, so its requirement and
its equity go on moving with legs that left; and the note ends a draw of the shard stated after it (run:
`KpfTDrawNote` takes it), every time, for good. The same note is written by a step that turns into "every leg
leaves" (by anyone).

Who loses: the shard's owner (no draw, books that are not its book), the user at a gain, the shard at a loss; and
any holder of such an account can do it on purpose at the cost of leaving at no gain.

Closing it: the entry refuses a seventh leg (stage 4: the trace has the leg as its last row, so the script has
the count), and `KpfTLegAdd`, the old entry and `KpfTLegMove` (which moves a leg into another account) go for
covered accounts. The numbers model already assumes six.

### F73 (Medium). A step that takes one leg takes its fee before what the legs that stay have lost

`KpfTCEnd`, kind 9: the leg named leaves alone where `m + gl - fee >= NOTE_MIN`: the coins pay what THAT leg lost
and its fee. What the legs that stay have lost is not asked. Where every leg leaves, the script pays the loss
first and the fee of what is left; where one leaves, the fee first.

Sequence (`t1b_finding_f73_a_step_that_takes_one_leg_takes_its_fee_before_what_the_legs_that_stay_lost`, each
step stated and applied at the engine): three legs long 100 units from 10 (3,000 KAS), coins 138 KAS: the
account stood (4.6 %). Each price falls 4.5 %, the bound of section 13's claim. The legs lost 135 KAS; the coins
pay that with 3 over. The only step the rule allows is of one leg (the coins pay the leg named and its fee, so
naming it does not turn into "every leg leaves"):

| Step | The shard is paid | The stepper | Coins left |
|---|---|---|---|
| leg 1 | 45 | 4.775 | 88.225 |
| leg 2 | 45 | 4.775 | 38.45 |
| leg 3 (the last: every leg) | 38.25 | 0 | 0.2 to the user |
| | **128.25 of 135** | **9.55** | |

Had the user stated that every leg leaves, at the same prices, the shard had all 135 (`c_state`, asserted). The
shard is short 6.75 KAS on an account that could pay; on the 21,829 random statements the most a step of one leg
made what the shard is good for fall was 1.67 KAS. It is bounded by the liquidation fee (0.5 %) of the worth of
the legs stepped alone. Whoever steps may be the account's own user, and the shard's owner: a step is then a way
to take up to that out of what the shard is owed, outside the draw's gate. The third numbers model has the same
order in `leave_one` and does not see it: `fell` is measured before `watch` runs.

Closing it: a leg leaves alone at a step where the coins pay what it lost, its fee, and what the legs that stay
have lost less what they gained (the net after the step is not below zero, a note's least staying:
`m + gain - fee >= NOTE_MIN`); else every leg leaves, where the fee already yields.

### F74 (Medium as built; a lock once kinds 2 to 4 go). One sompi of side B's makes every statement of kinds 7 to 9 wrong

`KpfTCEnd` answers every statement wrong on an account that has anything of side B's (`mv[76..84] != 0`).
`KpfTAddC` lets side B, the clearing party's key alone, add to its margin on any covered account.

Sequence (`t1b_finding_f74_a_sompi_of_the_clearing_partys_makes_every_statement_of_a_covered_accounts_own_wrong`):
an account of 100 KAS with one leg that gained; the user's take and its leaving are the rule's. Side B adds one
sompi (engine, its signature alone). Then `c_state` gives no take, no leaving of one or of all, no step; the take
the rule would allow, stated all the same, is answered wrong and not right at the engine; so is the leaving of
every leg. A take of the old form (kind 2) is still answered right on it.

So today the clearing party sends any user back to the old form (whose reserve has F66 and F67 open); and the
owner makes an account of its own one that no step of kind 9 takes. When kinds 2 to 4 go, nothing takes such an
account: the user's coins, and its gain, are locked for one sompi.

The same state is reached with no sompi added: `t1b_r47_f_...` below, where side B states by the old leaving
that a leg of the user's leaves, and what it lost becomes side B's margin.

Closing it: nothing of side B's can come to be on a covered account (`KpfTAddC` refuses side B there; the old
kinds, `KpfTLegOut` and `KpfTLegMove` go for covered accounts); or `KpfTCEnd` pays a side B margin it finds to the
shard as part of the note instead of refusing. The first is smaller.

### F75 (Medium). A note's least stays with the user: the coins are good for 0.2 KAS less than the tests take them for

Where every leg leaves at a loss, `pay = min(-gain, m - NOTE_MIN)`: "a note's least staying with the user". No
test asks that the coins be above a note's least by the maintenance: the take's floor is
`max(up * W + LS, NOTE_MIN)`, the step's test is of `m + G`, and an entry (on paper) asks `m >= up * W`.

`t1b_finding_f75_an_account_whose_coins_are_a_notes_least_pays_nothing_of_what_it_lost`:

| Account | Price | Outcome |
|---|---|---|
| 0.36 of a unit at 10 (3.6 KAS), coins 0.2 KAS (5.5 % of it) | 4 % down: lost 0.144 | by its user's leaving, and by a step: the shard is paid 0, the stepper 0, the user has its 0.2 |
| the same | 4 % up | the shard is asked 0.10656 |
| 4 units (40 KAS), coins 2.2 KAS (5.5 %) | 10 % down: lost 4 | the shard is paid 2.0, the user keeps 0.2 (end answered right at the engine) |

So an account near a note's least gains what the price gives beyond the band and pays nothing of what it loses;
in a rate, the margin of an account of W is `(m - 0.2 KAS) / W`, not `m / W`: 5 % at 40 KAS of worth, 0 at 3.6.
Accounts that small are cheap to make in numbers; today every entry needs the clearing party's signature, which
is the only thing between this and a shard paying out to small accounts that never pay in.

On the random statements (`t1b_r47_what_a_shard_is_good_for_...`): counted as the numbers model counts (each
account good for all its coins), every leg leaving at a loss makes what the shard is good for fall by up to
exactly 0.2 KAS; counted with a note's least staying, a take makes it fall by up to 0.119 KAS and a leg that
gained leaving alone by up to 0.042 KAS, on accounts whose maintenance is below a note's least; by nothing on
any other.

Closing it: every test counts the coins less a note's least: the take's and the leaving's floor
`up * W + LS + NOTE_MIN`; the step's `m - NOTE_MIN + G`; the entry's `m - NOTE_MIN >= up * W`.

(On an account of 0.2 KAS the answer of a disputed point is also not written by the builder: half the bond and
the object beside it are below what KIP-9 lets a transaction hold; the test prints the builder's words. On an
account of 5 KAS with the bond at 1 % it is written and taken:
`t1b_r47_the_answer_of_a_point_on_an_account_of_five_kas_is_written_and_taken`.)

### F68, seen from stage 3 (no new number; open on purpose)

`t1b_finding_f68_a_leg_that_entered_the_old_way_and_leaves_the_new_way_leaves_its_entry_in_the_basis`: a short
of 100 KAS that is in the rows and not in the basis (as `KpfTNoteApply` leaves an old entry) leaves by kind 8 at
its entry, asking nothing. `KpfTLeft` takes its entry out of a basis it was never in: the shard has no leg, 50
KAS and a basis of -100 KAS; its equity is 150 KAS for good, and a draw of all it holds is answered right at the
engine. This is the path by which F68 takes coins once stage 3 is in, before stage 4.

## Stage 3, item by item

**a. The two tests.** `t1b_r47_what_a_shard_is_good_for_falls_by_no_take_and_no_leaving_but_by_a_steps_fee_and_a_notes_least`:
10,000 accounts of one to six legs (from 0.1 KAS of worth; coins from below a note's least to most of the
worth), up to six statements each, every statement at prices of its own within the tolerance of fixed ones;
21,829 statements the rule allows. What the shard is good for on account of the account, before and after, at
the prices stated:

| Statement | Count | It fell by at most |
|---|---|---|
| a take | 8,105 | 0 on accounts that are not small; F75 else |
| one leg leaves by the user | 7,069 | 0 on accounts that are not small; F75 else |
| every leg leaves by the user | 6,201 | 0, a note's least staying; 0.2 KAS counted as the model counts (F75) |
| a step, every leg leaves | 224 | the same |
| a step, one leg leaves | 230 | 1.67 KAS (F73) |

The suspects: a take then a lone leaving, a lone leaving of a loser that leaves a winner's gain supporting
nothing, and the step that turns into "every leg leaves" are all in the sequences and take nothing (the loser's
leaving pays the shard first, to the sompi, and what is left is then good for exactly as much less). An account
with `b_margin` not zero: every statement is wrong (F74). Rounding: the band and the floors are rounded up, the
fee down; each is for the shard or for the user against the stepper, never the other way.

What is by the rule and worth knowing: at the committee's prices, an account that lost and leaves at a price
within the tolerance pays up to `g` of its worth less than it lost; an account that gained is paid at least `g`
of its worth less than it gained (the band is `2g`). The two meet exactly; the invariant holds "at the prices
stated", not at the committee's. And whoever steps chooses the leg: naming a losing leg the coins do not pay
turns the step into "every leg leaves" although a smaller losing leg could have left alone. That costs the user,
not the shard.

**b. `KpfTCEnd` against `Account::c_end`.** `t1b_r47_the_end_of_a_covered_accounts_statement_is_the_models_also_where_it_is_malformed`:
180 statements; 81 right, 91 wrong, 8 by neither verdict (a sum that does not fit); 0 partings. Changed one at a
time: the amount, the leg that leaves from two below none to one beyond the trace, the claimer, each sum of the
last row by one and at the ends of 64 bits, the two chains, a row less, none, one more, another kind, a sompi of
side B's.

**c. `KpfTCRow`.** `t1b_r47_a_covered_rows_answer_is_the_models_also_where_it_is_malformed`: 180 rows; 62 right,
97 wrong, 21 by neither; 0 partings. A row nobody can open is answered by neither verdict, so it lapses and
costs the claimer; any other point of such a statement that is wrong may be answered instead. A trace of more
rows than legs: the end reads the last row and stays right, the model names the row that is one too many, and
the script answers it wrong (run). A leg of a direction that is none: wrong. A leg that leaves alone named by
the chain of another leg: wrong. The 32 bytes where the chain stands, when no leg leaves alone: read by nothing
(the note of every leg is of the account's own chain); two statements that differ only there have one meaning
and the same outputs, and no note comes of them. A point beyond the trace is answered right by `KpfTCRow`
whatever its parity: see P4 for what that means for stage 4.

**d. The note.** `t1b_r47_the_note_a_leaving_of_the_rules_writes_is_applied_where_its_legs_are_booked` (above).
Asked below zero: the note stands with a note's least and what was paid; applied, the shard holds that much
more, called or not. A note that asks below zero and holds less (which only a wrong statement nobody disputed
writes) is applied only by whoever puts the missing coins in: nobody is made to (round 46's test, brought
along, has both). Asked at the 64-bit end: the script and the model refuse it. (`thin_build::left_apply_as`
adds without checking and panics there; only the `_as` form, with a note no statement writes.) Up to six legs
only: F72. Round 46's requirement stands on stage 4 as before.

**e. Who states.** A take and a leaving of another key: wrong at the end (in b). The statement itself is signed
by its claimer (run in g). A step on an account that stands, and a step with another fee: wrong (in b; the fee
where every leg leaves is the fee of all capped by what is left after the loss, to the sompi). Bonds: see F75's
last paragraph. A point of a leaving answered right starts the period again, and the same point is disputed
again: `t1b_r47_a_leaving_that_stands_is_held_open_by_the_same_point_disputed_again` (three times, then
applied at the prices of its time). Review 42's M1, now on the new kinds; what it costs is in item h.

**f. The old kinds on an account of the new form.** `t1b_r47_f_the_old_kinds_on_an_account_of_the_new_form_leave_the_basis_and_let_side_b_close_a_leg`:
1. A leg booked as a stated entry will book it leaves by the old leaving (kind 4, by the user): the rule's,
   stated at the engine; its old note is applied by `KpfTNoteApply`, which leaves the basis. No leg, 50 KAS, an
   equity of 150; the draw of all is answered right. (The mirror of F68's path above.)
2. Side B states by the old leaving that the user's leg leaves, once it lost 6 %, while the user stands with
   coins of the leg's whole worth. In the new form a leg leaves by its user or by a step of a user who does not
   stand.
3. What the leg lost is then side B's margin: F74 with no sompi.
4. A take of the old form by the user is refused where its legs gained (side B, with nothing, pays no gain),
   and stated where they lost.

What stage 6 must refuse on a moving part of 171 bytes that is no shard: kinds 2, 3, 4 in `KpfTChange.state`
(and with them `KpfTStepRow`, `KpfTStepEnd`, `KpfTStepLiq` for it); `KpfTLegAdd`; `KpfTEnter` and `KpfTStepEnter`;
`KpfTLegOut` and `KpfTLegMove` (they write the old notes and move margins of side B); `KpfTAddC` for side B;
and in `KpfTNoteApply` the notes that a leg entered or left (kinds 1 and 2), which write no basis. `KpfTHand`
may stay if side B's ask is nothing or is read as nothing. Because of 1 and of F68's path, these refusals belong
with stage 4, not after stage 5 (answer 5).

**g. `KpfTChange.state`.** `t1b_r47_the_kinds_of_a_covered_account_are_stated_on_one_alone_with_their_lengths`:
kind 7 is stated on a covered account by its claimer's signature and no other; kinds 7 to 9 are refused on an
account no shard covers and on a shard; kinds 6 and 10 are refused; a draw on a covered account is refused; what
is stated a byte short, a byte long, 47 bytes, or with a row of 111 bytes is refused; 48 bytes and no row is
stated, and its end is answered wrong.

**h. The numbers model, and the invariant.** I ran my owners on `covered.rs` itself, which the scripts are held
to, not on a copy of the third model: the owner that steps its own account a leg at a time (F73) and the owner,
or any user, of accounts whose coins are a note's least (F75). The third model has neither: `fell` is not
measured over `watch`, it has no note's least, and its accounts stop at six legs by construction (F72).

Is "no act makes what the shard is good for fall" the right invariant? It is the right one for an instant. What
it misses is that a statement fixes its prices at its time and is applied later, while the books carry its legs
at today's prices:
- A leaving that stands is a leg that has left at the statement's price. Until it is applied the shard's
  requirement and equity count it at today's. That is one dispute period and the lateness, and as many periods
  more as someone pays half a bond for (item e).
- The owner of the second model, who sets its users off and leaves before a tick, does not need to know the
  tick: it states the leaving of its own leg at a time up to the lateness ago, after it has seen the price.
- So the bound of the claim ("each price moves by less than the maintenance less the tolerance between two
  visits of whoever watches") is on the move within the life of a statement, not between two visits. With
  stage 4 the same holds for an entry (P2).

## Stage 4 on paper

### P1 (High, on paper; run on the scripts as they are). The marked account is taken by every script that takes an account

"Nothing that exists is shown a marked account and takes it" is not so with the mark where a position has an
ask. `t1b_r47_p1_an_account_with_an_ask_is_taken_as_any_other_by_the_scripts_of_stage_3`: on an account with
either ask not zero, a take and a leaving of every leg are stated and their ends answered right at the engine
(the ask goes on with the account), and `KpfTAddC` adds to it. By reading:

| Script | A marked account |
|---|---|
| `KpfTChange.state` | states kinds 2, 3, 4, 7, 8, 9 on it (reads no ask) |
| `KpfTCEnd`, `KpfTCRow` | answer them right |
| `KpfTStepEnd` | reads both asks, as a floor of a take |
| `KpfTAddC`, `KpfTLegAdd`, `KpfTLegOut`, `KpfTLegMove`, `KpfTEnter` | take it, read no ask |
| `KpfTHand` | reads side B's ask as the least margin of whoever takes the place: hands it over |
| `KpfTStepEnter` | reads side B's ask |
| `KpfTAcctEnd` | refuses it (it has a leg) |

Why it matters, in numbers: an entry of a long of 100 units stated at 5 while the market is at 10, by a user and
a clearing party who are one. The price is disputed and answered wrong; the marked account stands, with the leg
at the end of its chain, and the leg is in the shard's books at 5. If the marked account states that every leg
leaves at 10, `KpfTCEnd` answers it right and the note asks 500 KAS less the band, which `KpfTLeft` pays: the
leg that did not enter is paid as if it had.

Where the mark should stand so that all of them refuse it with no change: **in the length**. A marked account's
moving part is the 171 bytes and a push of the mark (180 bytes). Every entry typed `byte[171] moving` refuses
it; the four with `byte[] moving` (`KpfTChange.state`, `KpfTStepEnd`, `KpfTStepLiq`, `KpfTStepPrice`) ask for 138
or 171. The enders that take the position as a hash (`KpfTPoint`, `KpfTChangeLate`, `KpfTCRow`,
`KpfTChangeSettle`) restore it as they are. Only `KpfTStepPrice`, which opens the position, must learn the form,
and it is changed for kind 10 anyway. Then the paper's sentence is true as written. With the mark inside the 171
bytes each script of the table needs a line.

### P2 (High, on paper). An entry that is pending keeps the books blind for as long as it is pending, and nothing bounds that

The pending leg is in the gate, the draw's arithmetic and (stage 5) the call's, and its account takes no step.
An entry's statement lives a dispute period, the lateness, and one period more for every point disputed and
answered right; the same point again (item e, run for kind 8).

In numbers: users short 1,000 KAS of worth; the owner enters a long of 1,000 in an account of its own, pending.
Net nothing: the shard is asked 2.25 % of 2,000, and holds 45 KAS. The price falls 4.5 %, and 4.5 % again while
the entry is held open. The books show the same equity (the pending leg "owes" 88); no call is due by them, and
the account, being under a statement, is not stepped. Then the entry is ended and undone: the leg owed nothing.
The users are owed 88 (less the band) and the shard holds 45.

With a leg that has entered, the same owner is the second model's (it sets its users off and leaves), which the
rate on the gross holds for one tick, because the account is stepped or leaves at the next visit. So the cost of
the pending leg is exactly this: **the tick of the claim becomes the pendency of an entry.** The table's rows 2
and 3 ("let users in on what a pending leg lost"; "hold an account under an entry") are this one thing, and the
netting of a pending leg belongs in row 2 beside its loss: users let in against a pending leg that sets them
off are, once it is undone, on a book at 4.5 % where the rule asks 7.75 %.

What it needs: a bound on a statement's life that does not depend on anyone's goodwill (a point answered right
is not disputed again; or a point may only be disputed above the last one answered; or no dispute opens after
the first period), in stage 4 and not after it; and the claim restated over that life.

The other direction: a pending leg that gained shows the shard a loss. It keeps a draw down (the owner's own
cost), may refuse other entries at the gate (the owner's own doing), and may put the shard below its lower
level: a call on a loss that is not owed if the entry is undone. So for stage 5: a call is not decided on books
with an entry that is not settled (it is ended as a draw is, or the entry is settled or undone first). And the
reverse of that: a shard under a standing draw is not a shard to any script, so no note is applied to it and no
call reaches it. A held-open draw is ended by a note that landed before its last answer (the draw's object
lands anew each time: good); a call must be statable over it.

### The timings of a draw against an entry (question 2)

Entry stated at e, ended wrong at w, undone at u, the note applied at v. A draw stated at d.

| d | What ends the draw |
|---|---|
| before e | nothing needs to: it did not count the leg. And no entry is stated while a draw stands: `KpfTEnterS` spends the shard, and a shard under a statement is none. So nothing that ends a draw can come from an entry stated after it: "whenever it landed" is right for all three objects. |
| e to w | the statement of the entry, standing or under a dispute (two shapes of object to open) |
| w to u | the marked account |
| u to v | the note; it cannot be applied while the draw stands, since applying spends the shard |
| after v | nothing needs to |

So the draw can be ended throughout, if someone does it within its period. Two remarks. (1) A draw that nobody
ends and an entry undone after it: the loss was drawn; this rests on a watcher, as every dispute does. (2) I find
no sequence that the wait of a dispute period before the note is applied is needed for (the note outlives a draw
because the draw holds the shard); it lengthens the time the books carry a leg that did not enter. I would drop
it unless you have the sequence (P5, Low).

### P3 (Medium, on paper). The numbering of the shard's four prices

`KpfTCRow` answers any point beyond the trace right ("of nothing"), and `KpfTStepPrice` takes points up to 2n.
If the four prices of the shard's rows are points beyond the trace and `KpfTCRow` is opened to kind 10 as it is,
a dispute of a shard's price is answered right by `KpfTCRow` with nothing shown: the disputer loses its bond and
the price is never looked at. Kind 10 needs its own rule there: no point beyond the trace but the four, and those
answered by the shard's price script alone.

### Question 3. What else was decided on the stated prices meanwhile

On the four prices themselves: only the gate. On the leg they let in: every gate, draw and call until it is
undone (P2). One more thing is decided at stating and is worth saying: the leg's entry is the price of its row,
so a price beyond the tolerance is also a false entry; the undo, at the entry, pays nothing either way, which is
right.

### Question 4. Citations at every entry, or this form

Not citations. Compared honestly:

| | Stated with the shard (the paper) | Final at stating, with citations (my round 44 form) |
|---|---|---|
| Pending leg | yes: P1 (a mark), P2 (a bound on a statement's life) | none |
| Committee | signs only where a point is disputed | signs for every entry: five prices |
| Size of an entry | one script that computes the gate; a statement | the same and five citations: several kB more |
| New objects | marked account, `KpfTEnterOff`, a note that a leg did not enter, three ways to end a draw | none |

The second column pays for having no pending leg with the two things the owner decided against (the committee
signs only for a dispute; a citation is an answer). And the pending leg's cost has turned out to be one bound,
which the form needs for leavings anyway (item h). So: the paper's form, with the mark in the length and the
bound on a statement's life.

A third form, smaller than both where it applies: **the leg is never undone for a price; its entry is
corrected.** The statement is stated on the account with the leg already in it (so every ender restores the
account with the leg, and there is no marked account). Where the price of the entering leg's row is answered
wrong, the answer has the committee's price c for the statement's time: it writes the account with the leg at c
and a note to the shard of the difference in the basis. Where another row's price or the end is wrong, the bond
is lost and the leg stays (an account below its maintenance is stepped). Then no loss is ever "not owed" for a
price, and no draw needs ending. What it does not cover: a gate that should not have let the leg in (the shard
is then below its upper level with a leg that stays: no draw, a call below the lower), and a point nobody
answers (no price to correct to: the leg can only leave at its entry, which is the undo again). So it removes
the pending leg where the committee answers and keeps it where it does not. I would take the paper's form now
and keep this in mind if the three ways to end a draw grow.

### Question 5. The order of what remains

Changed in two places:
1. Before stage 4: F72's count of legs and F75's note's least are in the entry's own tests; F73 is one line of
   `KpfTCEnd`. They are cheaper before the entry is written against them.
2. The refusals of stage 6 with stage 4, not after stage 5 (item f): from the moment an entry writes the basis,
   an old leaving leaves it there, and until the old entry goes a new leaving takes out what was never in. The
   call of stage 5 reads those books. So: 3 as fixed; then 4 together with the refusals and with the bound on a
   statement's life; then 5 (the call, a step over a statement, a call over a draw); then what is left of 6
   (deleting the scripts nothing reaches).

## The two tests brought along

- `t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on`: the table at 58 scripts (`KpfTCRow`,
  `KpfTCEnd` before the two collectors), 66 entries, 56 `release`. Both new scripts read `ORACLE_FEE` twice and
  go on by `ends`.
- `t1b_r46_a_note_that_legs_left_is_applied_as_the_model_says`: the arithmetic written out now takes an asked
  below zero as the script does (added to what the shard holds, called or not; nothing paid); the random notes
  ask below zero by a sompi, by up to 300 KAS and at the 64-bit end, the note holding what was paid or only a
  note's least. 240 notes: 103 applied, 137 refused, 0 partings.

## Not reached

- `KpfTStepPrice` on kinds 7 to 9 beyond the coordinator's own two cases (within and beyond the tolerance): I
  read the change (the rows at 48) and ran no random prices against it.
- `KpfTPoint`'s lapse on kinds 7 to 9 at the engine (read: `kind >= 5`; the coordinator's test has one).
- Sizes: not measured again. A statement on two legs is 2,952 bytes and its applying 2,602 by the coordinator's
  count; the entry of stage 4 will be the largest transaction of the form.
- An account of more than six legs through every path (only "every leg leaves" was run; a leaving of one leg at
  a time from seven is fine by reading: each note is of one leg).
- The third numbers model was read, not extended: my owners ran on `covered.rs`.
- Stage 4: nothing run but P1's demonstration and the same point disputed again; the rest is reading.

## Findings ledger

| | Severity | State |
|---|---|---|
| F72 | High | open: `t1b_finding_f72_...` |
| F73 | Medium | open: `t1b_finding_f73_...` |
| F74 | Medium (a lock at stage 6) | open: `t1b_finding_f74_...` |
| F75 | Medium | open: `t1b_finding_f75_...` |
| F68 | High | open on purpose; a second path through stage 3 (`t1b_finding_f68_a_leg_that_entered_the_old_way_...`) |
| F61, F66, F67 | | open on purpose, as before |
| F41 | High | open with the owner |
| P1, P2 | High, on paper | stage 4 |
| P3 | Medium, on paper | stage 4 |
| P5 | Low, on paper | stage 4 |

Otherwise as round 46's ledger.

## Files changed and counts

- `crates/kpf-protocol/tests/auditT1_builders.rs`: 144 tests pass (1 brought along; 14 new: 4
  `t1b_finding_f72..f75`, 1 `t1b_finding_f68_...`, 9 `t1b_r47_...`).
- `crates/kpf-tests/tests/auditT1_tests.rs`: 55 pass (the table at 58 scripts, 66 entries, 56 `release`).
- `crates/kpf-protocol/tests/auditT1_solvency.rs`: 11 pass, unchanged.
- `crates/kpf-cli/tests/auditT1_cli.rs`: 47 pass, unchanged, run once.
- `docs/audit/REVIEW-T1b.md`: this round appended.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 144 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-tests --jobs 2 --test auditT1_tests         # 55 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_solvency   # 11 passed, 0 failed
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-cli --jobs 2 --test auditT1_cli             # 47 passed, 0 failed
```

# Round 48

Scope: the fixes of round 47 at the engine (F72: the note applied a leg at a time; F73: the step's test; F75: a
note's least in every test), my files brought along to them, the whole workspace run once; and section 14, the
rest of the form, on paper as one design. Working tree on eb50dda, stage 3 and its fixes uncommitted. No src,
contract or `kpf-out` file touched; no commit.

## Headline

- **F72, F73 and F75 are closed**, each test saying so at the engine. The differential tests of round 47 agree
  again on the changed scripts (0 partings: 180 ends, 180 rows; 240 notes over 652 legs applied leg by leg).
- **One new finding at the engine, F76 (Medium)**: a note applied a leg at a time shows, between its first leg
  and its last, an equity that is neither the one before nor the one after, off by all the legs already out
  gained or lost since they entered. A draw is guarded against it (run); the gate of an entry and a call, on
  paper, are not.
- The whole workspace: 1,268 tests passed, none failed (140 result lines), run once with my file at 146 tests;
  two more were added after and pass.
- Section 14: fourteen findings on paper. The ones that would have been found late: the life of a statement lets
  a claimer pick the one point that is looked at (Q1); a call's unanswered price stands (Q8); a step over a
  statement cancels a right leaving (Q7); an entry is an option for whoever of the two signers planted a point
  (Q4); an account of more than six legs under a call is stepped by nobody (Q9); claims paid by the call's time
  leave what comes in after to the owner (Q10).

## At the engine

### Fixed and verified

**F72.** `t1b_fixed_f72_the_note_that_seven_legs_left_and_that_twenty_left_is_applied_to_its_end`: every leg of
an account of seven legs, and of one of twenty, leaves by one statement (stated and applied), at a loss and at a
gain; the note is applied to its end in as many transactions as it has legs, each made of the outputs of the one
before; the books are then empty, the basis nothing, the shard holds what the account paid more or what it
asked less; and the script's arithmetic written out gives the same shard.

`KpfTLeft.apply` against the model, brought along (`t1b_r46_a_note_that_legs_left_is_applied_as_the_model_says`):
240 notes on random books, 652 legs at the engine. At every leg the engine takes what the arithmetic written
out gives and no other: 106 notes applied to their end (nothing settled before the last leg; the note not
ending with legs to go; not a sompi more paid with the last), 134 refused at some leg (a leg of another size,
entry, direction or market, a leg that is once in the books twice in the note, no leg, what is asked beyond 64
bits, a called shard at the last leg of a note that asks). The model and the arithmetic part on none, leg by
leg and to the note's end (`left_whole`).

On one note (`t1b_r46_a_note_that_legs_left_is_applied_once_...`, rewritten): 37 transactions refused around
the two legs of a note: the note going on as it was, of another chain, key or amount, with a sompi less, under
no id; something paid with a leg to go; the note ending with a leg to go; a row that is not the leg's; another
leg or another chain before it than the note's; and at the last leg what round 46 had. On a called shard the
leg that is not the last leaves, the last waits where something is asked, a note that asks nothing ends.

Interleaving and the draw's guard (`t1b_r48_notes_applied_in_any_interleaving_...`): two notes, of seven legs
asking 30 KAS and of five legs holding 12 KAS the account paid, on a shard with two legs of others, applied a
leg at a time in six random orders: the same shard every time, which is the one each note applied whole gives.
In every order, with one note partly applied, a draw stated on the shard as it then stood was ended by
`KpfTDrawNote` shown the note as it stood (its chain another, its key and amount the same).

The statements' notes (`t1b_r47_the_note_a_leaving_of_the_rules_writes_...`, brought along): 56 notes of
statements of the rule's applied to their end, a leg a transaction; 3 waited at their last leg on a called
shard, the legs before it out.

**F73.** `t1b_fixed_f73_a_step_takes_one_leg_only_where_the_coins_pay_what_the_legs_that_stay_lost`: round 47's
account (three legs of 1,000 KAS, coins 138, each price 4.5 % down): one step, of every leg: the shard is paid
135 of 135, the stepper the 2.8 the coins have over, the user a note's least. With coins of 200: two legs leave
alone (the coins pay the rest each time), the shard is paid 90 of 90, the stepper 9.55, and the account stands
with its last leg and coins that pay what it lost.

**F75.** `t1b_fixed_f75_the_coins_of_an_account_count_a_notes_least_less_in_every_test`: an account whose coins
are a note's least does not stand at its own entry (it is stepped at once, at no loss) and takes nothing; an
account of 40 KAS of worth with 2.2 KAS takes nothing and stands; from 3.4 KAS the most a take gives is 1 KAS
(a sompi more is answered wrong at the engine), and what stays pays the whole of a loss of 5.5 % of the worth.

**My owners again** (`t1b_r47_what_a_shard_is_good_for_falls_by_no_statement_of_a_covered_accounts_own`,
renamed): 10,000 accounts, 21,537 statements the rule allows: counted with a note's least staying with the
user, no take, no leaving and no step makes what the shard is good for fall, on any account, small or not.
(Counted as the numbers model counts, each account good for all its coins, every leg leaving at a loss still
shows 0.2 KAS: the model's count, not the rule's.)

**The random differential tests of round 47 on the changed scripts**: `KpfTCEnd` against `Account::c_end`, 180
statements (78 right, 93 wrong, 9 neither); `KpfTCRow` against `c_row_judged`, 180 rows: 0 partings in both.

### F76 (Medium). A note half applied shows an equity that is neither the one before nor the one after

Each leg is taken out of the books at its entry, and what the note asks is settled with the last leg. Between,
the equity is off by everything the legs already out had gained or lost since they ENTERED. (A whole note not
applied is off only by what the price moved since the leaving was stated.) The order is the chain's, last leg
first; the account's user chose that order when it entered.

`t1b_finding_f76_a_note_half_applied_shows_an_equity_that_is_neither_the_one_before_nor_the_one_after`: an
account long 1,000 units and short 1,000 units of one market from 10, the price now 15; a shard of 2,000 KAS
with the two legs booked. Every leg leaves: the net is nothing and the note asks nothing.

| | Equity at 15 |
|---|---|
| before the note | 2,000 KAS |
| the long applied, the short to go | 7,000 KAS |
| the note applied | 2,000 KAS |

On the books between, a draw of all the shard holds is answered right at the engine. It is ended by
`KpfTDrawNote` shown the note as it stands, if somebody does that within its period (run). The gate of an
entry (14.3) and a call (14.6) read the same books in the script and are shown no note: 5,000 KAS of equity
that is not there lets in legs of 64,500 KAS of worth; with the legs in the other order the books show 5,000
too little and a call is right on them. For how long: until anyone applies the next leg, which costs a
transaction and pays nothing unless it is the last (the applier of the last has the note's carrier). Whoever
gains by the gap is the owner, with an account of its own and the legs entered in the order that suits.

Closing it, in the order I would take them: (a) six legs an account (Q9 asks it anyway), the note applied whole
in one transaction as in round 46, at the size that script had; (b) the shard says that a note is being
applied (its chain), no other note starts, and the gate, the draw and the call refuse meanwhile; it costs 33
bytes where the shard has none free; (c) the note's chain carries each leg's price at leaving and the shard a
number of what is realised, so that a leg goes out at the price it left at: the most exact and the most to
build.

### Run for section 14

- `t1b_r48_no_script_of_today_takes_a_moving_part_of_180_bytes`: `KpfTChange.state` refuses a moving part of
  180 bytes for kinds 7, 8, 9 and 2. `KpfTAddC` and `KpfTHand`, typed for 171 bytes: the builder writes no push
  of another width, so the object was made the one of 180 bytes, the transaction signed, and the push written
  again in the signed script: the engine refuses both (controls: the same push written again on the account as
  it is runs). The declared width is held by the script, so every entry typed `byte[171]` refuses a marked
  account as the paper says. Were one taken, it would write the account back without the mark and with the leg.
- `t1b_r48_the_place_of_the_expiry_is_read_by_no_script_of_a_shard`: the end of a draw is answered the same
  with nothing, 1, 9,000,000 or the largest number there; no script source reads those bytes of a moving part.
- Ages: a right answer is taken at age nothing today (every answer of my tests); the same point disputed again
  and again is round 47's test, which still passes.

## Section 14 on paper

Findings are Q1.. with a scenario in numbers. Where I would build a part otherwise, it says how.

### 14.1 The life of a statement

**Q1 (High). One right answer makes it final, and a dispute holds the object alone: so the claimer picks the one
point that is looked at.** A claimer states, and with another key disputes a point of its own that is right, in
the same breath (the dispute is chained on the statement: nobody else ever sees the statement undisputed). Half a
period later it answers that point right: final, applied at once. In that half period a watcher can do one thing:
answer another point wrong, with the proof in hand. It can no longer name a point and leave the showing to the
claimer.

- For a row and for an end the proof is the script's: fine, in half a period instead of one.
- For a price the proof is a citation. The watcher needs the committee's signature inside half a period, where
  today the claimer has a whole period to produce it.
- For a price no committee answers (a market nobody prices; a committee that is slow or silent) there is no
  proof. Today the watcher disputes that price, nobody answers, and the statement is ended at the claimer's cost
  (reviews 38 and 45, F69). Under 14.1 it goes through.

In numbers: a shard of 500 KAS with a row whose committee does not answer. Its owner states a draw of 400 at a
price for that row that makes the equity 1,000, disputes the end itself (right, at the prices stated) and
answers it half a period later. Applied. The same for an entry (the four prices of the gate), for a call (Q3),
for a take or a leaving of kinds 7 to 9 with a leg in such a market, and for kinds 2 to 4 on an account no
shard covers (today a price nobody answers ends them, both bonds back).

So "a claimer that disputes a right point of its own wrong statement does not get it through" holds only for
what a watcher can prove in half a period. Is half a period enforceable as written: yes. `KpfSlot.go` holds
`this.ageDaa >= age` with the age read from the library script's second argument; an answer asks
`int(ageb) >= window / 2` where its verdict is 1, as `KpfTPoint.lapse` asks `>= window` today.

How I would build it. Keep the bound, drop "final by one answer":

1. Every deadline is from the statement's own time: **a point is disputed only by a dispute that lands by
   `t0 + late + window`**. The dispute cannot check that itself (a script has no upper bound on now); the answer
   and the lapse read the dispute's landing (`OpTxInputDaaScore`, as `KpfTPoint.dispute` reads the statement's)
   and a dispute that landed later is dismissed by anyone, at its disputer's cost.
2. **A right answer is taken at once, from anyone, who is paid, and may name the next point in the same
   transaction** (with its own bond). So a dispute of a right point, by the claimer's other key or by anyone,
   is something a watcher turns into its own dispute and is paid for: there is no holding the object.
3. A statement nobody disputes is applied when its period is over, as today; one that stands again, when its
   period is over or the deadline has passed.
4. Lapse as today.

A statement then lives `late + 2 * window` at most, every point a watcher names inside the first period is the
claimer's to show, and no rule depends on who the disputer is. The cost is one more shape of output in `ends`
(the object pointed again instead of standing again).

**Q2 (Medium). Whoever answers right is paid half the disputer's bond: a disputer that answers its own dispute
loses half a bond and the claimer has nothing.** Today holding a claimer's statement one period more costs a
whole bond, half of it to the claimer. Under rule 3 the disputer answers itself the moment half a period is
over and has half back; the other half stays in the copy. A leaving of an account of 55 KAS (bond 0.55 at 1 %)
is delayed for 0.275 KAS, and its user is not paid for the delay. Split it instead: half to the claimer as
today, and whoever answers has a part of the half the copy keeps.

The kinds that exist: every answer script shares `ends`, so the rule reaches kinds 2 to 6 and the draw at once.
What changes under them: the state "stands again" (`LATE_AGAIN`) becomes "final"; `KpfTChangeSettle` applies a
final statement at no age; `KpfTPoint.dispute` and `KpfTChangeLate` refuse one; `KpfTDrawNote` (and, on paper,
the three ways an entry ends a draw, `KpfTStepOver`, and a call's clearing) must each open three shapes of a
statement: standing, disputed, final. Tests that will turn: every one that disputes a statement standing again
(mine: `t1b_r42_a_statement_that_changes_nothing_...`, `t1b_r47_a_leaving_that_stands_is_held_open_...`).
"Final and applied at once" and the draw's guard: a note that is not applied ends a draw while it stands or is
disputed; with the owner's own dispute that is half a period, not one. Every guard of the paper that rests on
"within its period" is half that against a claimer who disputes itself.

### 14.2 What a covered account refuses, and the account of no leg

Held by reading. The list of refusals is the one of round 47's item f, and it must come with the entry (it does,
in 14.8). An account of no leg: a statement of 48 bytes and no row is stated today (run, round 47); `KpfTCEnd`
answers it wrong (`n < 1`), so the take with no row is a line there, with the chain of no row being 32 zero
bytes; `KpfTCRow` answers every point of it right (beyond the trace). The leaving of the only leg: the chain
that stays is 32 zero bytes, which is the fixed part of an account of no leg already; `KpfTCEnd` today turns
that case into "every leg leaves", so it is one condition less, not one more.

**Q3 (Low).** Two things the list lacks. `KpfTOpenC` must refuse anything of side B's at the opening (F74 from
birth otherwise). And an account of no leg is ended by `KpfTAcctEnd`, which the section does not name: without
it the last 0.2 KAS of an account are taken by nothing (a take leaves a note's least).

### 14.3 The entry

Run on the scripts as they are (`t1b_r48_no_script_of_today_takes_a_moving_part_of_180_bytes`): see the result
under "At the engine". The mark in the length is the right place.

**Q4 (High). Two signatures, and one of the two lets the statement be found wrong: the entry is an option for
whoever planted the point, and the other pays for it.** Both sign what is said. The user can check the trace
(the tool computes it). It cannot check the four prices of the shard's rows to the half per cent the tolerance
is, without the committee's own prices, which the committee signs only for a dispute. The owner states the price
of a row the user does not trade 0.6 % off. For the life of the statement:

- the user's leg loses: the owner lets the entry settle;
- the user's leg gains: the owner's other key disputes that price, nobody can answer it right, the entry ends
  at the claimers' cost and is undone. The leg owed nothing; and half the bond the claimers lost goes to the
  disputer, who is the owner.

In numbers: a long of 1,000 KAS of worth. Up 3 % inside the life: the user would have 30 less the band; it has
its coins back, less its part of the bond. Down 3 %: it has lost 30. The user can do the same to the owner with
a row of its own trace (a row is proved wrong by the script, no committee needed), ending an entry whose leg
lost. And an entry that lands late is a third way to have one end (`KpfTChangeLate`), by whoever lands it, with
the bond back if its own other key ends it: the table's "only it chooses, by stating it wrong" is not all.

How I would build it: **a price answered wrong corrects; the entry falls only where its own tests fail at the
committee's price.** The answer of a shard's price has the committee's price c. With c in the place of the
price said (what the shard held and its basis at stating are said too, two numbers more) the script computes
the gate again: where it holds, the entry stands, the leg's entry being c where the row is the leg's own (a
note to the shard of the difference in the basis; the account written with the leg at c). Where it does not
hold, the entry ends as on paper. The same for a price of the trace and the end. Then a planted price gives no
option: the entry stands or falls by what is true. What stays: a price nobody answers (the entry can only end),
so a user should not enter a shard with a row no committee prices. And two bonds by part: a point from
1,000,001 costs the owner's, a point of the trace or the end the user's.

**Q5 (Medium). Every point must be somebody's to answer right, or it is a free end.** A point nobody can
answer lapses, and for kind 10 that ends the entry at the claimers' cost, half to the disputer. So, exactly:
`KpfTCRow` answers right every point beyond the trace that is not one of the four (2n+1 to 1,000,000, and from
1,000,005); the shard's price script answers right, with no citation, a row that has no market, as
`KpfTShardPrice` does today; and neither answers the other's. (Today, kinds 7 to 9 and the draw have no such
gap: run in round 47, read again for the draw.)

Computed at stating against disputed after: the marked account's chain is computed from the account's own root
and the leg said, not from the trace: right. The gate is computed on the shard's books as they stand, which may
have a note half applied: F76 (the gate reads 7,000 KAS of equity where 2,000 is true, and lets in legs to
match). An entry on an account with legs in other markets: the end counts what they gained, at prices of a time
up to the lateness ago that the user picks; an account let in on a gain of a moment ago may not stand now, and
is then stepped. That is the user's loss unless the price gapped by more than its coins; it belongs to the
lateness (14.7), not to the entry.

The hash of the shard's rows in what is said: computed at stating, so it is the rows with the leg and nobody
can say another. `KpfTEnterOff` and a kind of its own for its note: needed, so that the draw's guard tells it
from a note that legs left (which must have landed before the draw).

### 14.4 A leg that is in the books and has not entered

**Q6 (Medium). The bound has the lateness in it too, and nobody is paid for the two steps it rests on.** An
entry's prices are of a time up to the lateness before it landed, so the gate was computed that much earlier:
the bound is on what a price moves in the lateness, the statement's life, and the time between two visits. And
of the three things whoever watches must do, only the first pays: answering (half a bond). `KpfTEnterOff` pays
nothing and its caller puts a note's least in, which whoever applies the note has; applying a leg of a note
that is not its last pays nothing. The owner, whose phantom it is, has no reason to do either. Let the mark
hold a part of the bond the claimers lost, for whoever undoes it.

Is the table complete: no. Add (a) the user's side of Q4; (b) an entry that lands late (above); (c) a note half
applied, with the same three effects as the pending leg and no bound at all until somebody applies its other
legs (F76); (d) a pending leg that gained: it shows the shard a loss, and a call that is right on the books is
stated on a loss that is not owed (14.6 then ends the entry by showing the call, so the call clears what it was
decided on). And the netting of a pending leg lets users in at 4.5 % of their worth where the rule asks 7.75 %;
"a user reads what is pending before it signs" means, taken seriously, one entry a shard a statement's life.

Is it honest to rest it on whoever watches: for the dispute, yes (paid). For the bound itself the question is a
number the model does not have (14.9 says so): with the tolerance and maintenance in use the claim needs the
price to move less than 4.5 % in `late + 2 * window + a visit`. If a dispute period must leave a committee time
to sign, that is minutes; then either the rate on the gross goes up, or a pending leg is not let set anything
off.

How I would build it, if that number does not come out: **a pending leg counts against the shard only.** The
entry writes the leg into the row's gross and into two numbers a row of pending units (side B long, short), not
into the net or the basis. The requirement takes the net at its worst over what is pending; the equity takes
nothing of it. When the entry is applied, a note that the leg entered moves it into the net and the basis; when
it ends, the note that it did not enter takes it out. A note not applied is then always the cautious side, and
the one who gains by applying it is the owner. No draw needs ending by an entry, no call is hidden, nothing is
let in on a leg that may go. The cost: 16 bytes a row, one note more per entry, and for the life of an entry a
leg that gained is not yet a liability in the books (bounded as now, by that leg alone).

### 14.5 Not held under statements

The draw's time: no script reads the place of the expiry today (run), and the shard's id is not of it. Held by
draws that are ended: an ended draw writes no time, so it can be stated again at once; each costs its claimer a
bond to whoever ends it, which is the watcher's pay. By calls that are wrong: the same, at a bond; but see Q1
and Q8 for a wrong call that is not ended. By entries one after another: they spend and write the shard in one
transaction; they do make every transaction built on the shard as it stood miss, which costs an owner one entry
of its own each time and a caller a retry on the unconfirmed output. By statements of an old kind: refused
(14.2). By a step over a step: a step is not stated over a step, so a user holds its account with a wrong step
of its own until somebody answers it wrong (paid), and under Q1 gets it through.

**Q7 (High). A step stated over a statement ends that statement, right or not: a wrong step cancels a right
leaving for the price of a bond.** `KpfTStepOver`: "that statement is ended, its bonds back, and the step
stands in its place". If the step is then answered wrong, the account is back and the leaving is gone; its
user states again, at the price of now. In numbers: a long of 1,000 KAS of worth on coins of 55. Up 4 %, the
user states its leaving: it asks 29.6 KAS. Inside the period the price falls back 2 %. The clearing party
states a step over the leaving (the user stands: the step is wrong) and loses its bond, 0.55 KAS. The user
leaves again and asks 9.8. Whoever owes the note has an option on every leaving for one per cent of the
account's coins; and anybody ends anybody's take or leaving for the same.

Otherwise, two ways. (a) The step is stated on the statement as its object: what is restored where the step
ends is the statement as it stood, with its bonds in it; the stepper advances the bonds to their owners when it
states, and has them back in the step's outputs where the step is applied (a wrong step over a statement then
costs its bond and what it advanced). (b) No `KpfTStepOver`: an account says, as a shard does, a time before
which its user states nothing again (the same place, which an account does not read either), two periods after
the last. A step then always has its turn, and the bound of 14.4 already has two periods in it. (b) is one
script less and no option; its cost is that a user's second statement waits.

### 14.6 The call

**Q8 (High). A price of a call that nobody answers leaves the call standing: the prices of a call are then
whatever the caller says, and they are the prices every account is stepped at.** A shard of 500 KAS; users long
1,000 KAS of worth in a row at 10; its committee does not answer (or not inside the period). A caller, who is
one of the users, states the row at 15: the books lose 500, the call is right at the prices said. The owner
disputes the price; nobody answers; the call stands, at the owner's cost. Every account is stepped at 15: the
notes ask 500 less the band, and the claims take what the shard holds. With Q1 the caller does not even need
the committee silent, only slower than half a period. For a draw and an entry the paper has it the other way
(the prices are the claimer's to show), and review 45 is why. A call's prices must be the caller's to show too:
where the committee is silent, no call. That is the fail-closed side.

**Q9 (High). An account of more than six legs under a call is stepped by nobody, and nothing but its user ends
the call.** "A call ends where no row has a unit, and no other end." The owner keeps an account of its own of
seven legs, short 1,000 KAS of worth, which sets its users' longs off. The price rises 10 %; the shard holds
45; it is called. The users' accounts are stepped: claims of 89 KAS. Nobody steps the seven legs. The call's
time passes; the claims are paid 45 of 89 and are no more. Then the owner's account leaves by its own
statement: what its coins pay goes to a shard that owes nothing, the call ends, and its owner draws it. And a
user of seven legs that never leaves keeps the shard called for good.

Cut it: **a covered account has six legs at most** (the entry counts the rows of its trace). Every account is
then stepped under a call by anyone; a note has six legs at most (and could again be applied whole: no F76);
"its user leaves by its own statement" goes. What it costs: a seventh leg is a second account, with no margin
shared.

**Q10 (Medium). Claims paid once the call's time has passed are paid at the share of that moment and are then
no more; what comes in after is the owner's.** A shard holds 40; the winners are stepped first: claims of 100.
The time passes: paid 40, nothing owed. The losers are stepped after and pay 50: no unit, nothing owed, the
call ends, the owner draws 50. The users were owed 100 and had 40 where 90 was there. Whoever gains by a late
step of a loser is the owner. With Q9's cut every account can be stepped by anyone, so: **claims are paid once
no row has a unit, and at no other time.** The share is then the same for the first and the last to the sompi
(a claim of `a` paid `floor(a * H / C)`, both falling: the share never falls, and what rounding leaves is the
last one's, a sompi a claim at most), provided nothing else moves the two numbers between two claims: coins
the owner adds raise it for those after (the paper says so; it harms nobody), and with no unit left no note is
still to come.

**Q11 (Medium). The fee of a step under a call "out of the user's coins where they hold it": say the order,
and pay the step of an account that holds nothing.** Before the shard is paid it is F73 again (the stepper
ahead of the shard). After it, an account whose coins all go to the shard pays no fee, nobody steps it, and
with Q10 the claims wait on it for ever. Pay that step out of what the shard holds.

**Q12 (Medium). The call's bond is of the shard's size and its pay is flat.** `KpfTChange` takes the bond from
the object: 1 % of what the shard holds. A shard of 1,000,000 KAS is called by whoever locks 10,000 KAS for a
period, for the least fee of a keeper. "A call is stated when it is due" then rests on nobody. Either the bond
of a call is a number of its own (it need only pay whoever answers it wrong), or its pay is a rate.

**Q13 (Medium). A statement that had stood its period when the call was stated, and is not yet applied, is
ended by a step or by showing the call: whoever gains chooses.** An entry whose period passed with no dispute
and that nobody applied: its leg gained, the owner ends it by showing the call (it owes nothing); its leg lost,
the owner applies it and steps it. The same for a leaving that has stood its period. Decide it by times, which
the scripts have: a statement that landed more than its period before the call's time is applied, not ended.

Smaller. Every step and every clearing spends the one call object and writes it again, so they go one after
another: write a few of it. `KpfTChange` "as it is" does not take kind 11 on a shard (`shard == 1` is asked for
kind 5 alone): a line. The number of what is owed by claims, where a position has side B's margin: `KpfTChange`
takes the bond from both margins, so a called shard's bond grows with its claims; harmless.

Best play, each party.
- The owner with accounts of its own: seven legs (Q9); step the winners early and its own losers late (Q10);
  keep an entry of its own pending when the call is near (14.4); be the caller, and pick the call's time inside
  the lateness: the price every account is stepped at is then the one of the last `late` that suits its book.
- A user that would rather not be stepped: seven legs. Under a statement of its own it is stepped all the same
  (right). Adding a sompi to its account each block makes every step built on the account miss; each costs it
  the oracle fee (0.002 KAS), about 70 KAS an hour: bounded, and a stepper builds on the unconfirmed output.
- A user that would rather leave at its own price: it states its leaving while the call is still a statement;
  if the account's period is the shorter one, the leaving is applied first and its note is a claim at the
  user's price, not the call's. So "every leg leaves at the call's price" is of the accounts still there.
- A stepper: accounts with coins first (Q11).
- A caller: Q8, Q12.

### 14.7 and 14.9 What rests on whoever watches, the lateness, sizes

Said honestly it is this. Paid: disputing a wrong point; answering; ending a late statement; a step under a
call; a call (flat). Not paid: undoing an ended entry, applying a note's legs but its last, applying a
statement somebody else stated. Halved by Q1: every window "within its period".

The lateness is an option of whoever states, each time against the other side: a leaving at the best price of
the last `late` (against the shard); a step at the worst standing of the last `late`, so an account that
dipped below its maintenance for a moment and stands again is stepped at the price of the dip (against the
user, and the shard's owner may be the stepper: the user pays the loss at that price to the shard and the fee
to the stepper); a draw at the best equity; a call at the worst, whose price is then everybody's. "The terms
keep it short" is the whole of the defence, and it wants a number.

**Q14 (for the owner to decide; not a hole). The band is a charge of twice the tolerance of the worth on every
leg that leaves at a gain.** Since stage 3 a leaving asks the gain less 1 % of what the leg is worth, whatever
price is stated. A long of 1,000 KAS of worth on 55 KAS of coins: up 2 %, the user has 9.8 of its 20; up 1 %,
nothing; down 1 %, it pays 10. In the form before, the band was only on what the pot could not pay. It is what
keeps the shard whole against a price stated the tolerance off; it is also 18 % of the margin on a winning
trade. The way out I see is the one of Q4: a price that is answered corrects (the note asks the gain at the
committee's price), and the band then need only be what a dispute costs, not the tolerance.

Sizes: not known to me either. What I can say is where they are: every transaction shows a script of 2 to 4.5
kB whole, and a statement its rows; an entry shows two objects' parts. Nothing on paper changes that.

### 14.8 The order

I would settle four things before (a), because (a) rewrites `ends` in every answer script once: the life of a
statement (Q1, Q2); six legs (Q9), which decides whether `KpfTLeft` stays a leg at a time and whether F76 is
there; whether a price that is answered corrects (Q4, Q14), which changes what an answer writes; and
`KpfTStepOver` or the account's time (Q7). Then the paper's order holds: (a), (b), (c), (d).

What I would cut from the first form: accounts of more than six legs; claims paid by the call's time;
`KpfTStepOver` (the account's time in its place). Each is named above with what it costs.

## Not reached

- `KpfTLeft.rest` and `KpfTDrawNote` on a note of the rest: unchanged scripts, round 46's tests pass, nothing
  new run.
- A note partly applied on a shard that is called, to a claim: the call is not built; only the two legs of the
  one-note test.
- The third numbers model with a leg that never enters (`Owner::Phantom`): read, not attacked. Q6 says what
  number it lacks (the tick as lateness, a statement's life and a visit).
- Section 14 is on paper: nothing of it could be run but the three probes above. The proposals (the life of a
  statement by deadlines from its own time; a price that corrects; a pending leg counted against the shard
  only; the account's time in the place of `KpfTStepOver`) are sketched far enough to be judged, not checked
  against scripts.
- Sizes: none measured.

## Findings ledger

| | Severity | State |
|---|---|---|
| F72, F73, F75 | | fixed, verified (`t1b_fixed_f72_...`, `f73`, `f75`) |
| F76 | Medium | open: `t1b_finding_f76_...` |
| F74 | Medium (a lock when the old kinds go) | open: 14.2 closes it |
| F68 | High | open on purpose; two paths (old entry, old leaving) until 14.2 |
| F61, F66, F67 | | open on purpose, as before |
| F41 | High | open with the owner |
| Q1, Q4, Q7, Q8, Q9 | High, on paper | section 14 |
| Q2, Q5, Q6, Q10, Q11, Q12, Q13 | Medium, on paper | section 14 |
| Q3 | Low, on paper | section 14 |
| Q14 | for the owner to decide | the band |

Round 47's P1, P3 and P5 are answered by section 14 (the mark in the length, run; the points from 1,000,001,
with Q5; the wait before the note is applied is gone). P2 is Q1 and Q6.

## Files changed and counts

- `crates/kpf-protocol/tests/auditT1_builders.rs`: 148 tests pass. Brought along to the note applied a leg at a
  time: the two round 46 tests of the note and their helpers (now `left_one48`, `left_whole48`, `row_for48`,
  `left_taken48`, `applied_whole48`), round 47's test of the statements' notes, F68's second path. Turned:
  `t1b_fixed_f72_...`, `t1b_fixed_f73_...`, `t1b_fixed_f75_...`; the test of what a shard is good for, renamed.
  New: `t1b_finding_f76_...`, three `t1b_r48_...`.
- `crates/kpf-tests/tests/auditT1_tests.rs`, `crates/kpf-cli/tests/auditT1_cli.rs`,
  `crates/kpf-protocol/tests/auditT1_solvency.rs`: unchanged; 55, 47 and 11 pass.
- `docs/audit/REVIEW-T1b.md`: this round appended.

```
CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2    # 1,268 passed, 0 failed, 140 result lines
                                                                      # (auditT1_builders at 146; thin_draw 16,
                                                                      #  research_solvency_accounts 5)
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 148 passed, 0 failed (after)
```

# Round 49

Scope: the second writing of section 14 of `docs/ARCHITECTURE-CLEARING.md`, on paper, as one design. Nothing of
it is built; stage 3 with its fixes is committed (970caa7). One probe of what exists was run. No src, contract
or `kpf-out` file touched; no commit.

## Headline

- Of round 48's Q1 to Q13: nine are closed by what is written (Q2, Q3, Q5, Q6, Q8, Q10, Q11's order, Q12, and
  Q9 but for the opening); Q1 and Q7 are closed in their substance and each leaves one thing (R1, R7); Q4 is
  closed for the shard's prices as intended and not as the generic scripts would do it (R3), and its residue is
  a matter of the size of a bond (R4); Q13 is closed for kinds 7 to 9 and not for an entry (R9).
- The second writing opens fourteen things. Four I would not go past: **R3** (a lapse of a price of the shard's
  rows costs the user, by the script that lapses as it is), **R5** (a late landing costs the user its bond,
  whoever landed it), **R6** (the count of legs is whatever the opening says: run), **R7** (the time an account
  and a shard write is exactly the longest a statement lives, so nothing is free between two).
- The bound as a number: with the terms of the tests (lateness 10 s, dispute period 5 s) it is 25 seconds and
  holds with a wide margin. For a volatile coin it wants a dispute period of 45 to 90 seconds and a lateness of
  10; at five minutes it fails on a bad day.

## 1. Round 48's findings against the second writing

| | Closed | Why, or what is left |
|---|---|---|
| Q1 | in substance | No finality by an answer; a last time to dispute; whoever answers names the next point. A watcher that can answer the standing dispute gets its point in. What is left is R1: the rounds between a claimer and its own key cost nothing as written, and a watcher cannot answer a price whose citation only the claimer holds |
| Q2 | yes | The claimer keeps its half. (Giving the answerer the whole of the copy's half is what makes R1's rounds free) |
| Q3 | yes | Side B refused at the opening and after; `KpfTAcctEnd`. But see R6 for what else the opening must refuse |
| Q4 | in part | A price of the shard's rows ends the entry only where the gate fails at the committee's price, at the owner's cost: that takes the planted price of a row away. Left: R3 (the lapse), R2 and R8 (the dismissed dispute), R4 (what half a bond buys), R5 (the late landing), and the tool's rule for the prices of the user's part |
| Q5 | yes | The table of points says who answers each |
| Q6 | yes | The lateness is in the bound; undoing is paid; the table has the four rows |
| Q7 | in substance | No step over a statement; the account's time. Left: R7 |
| Q8 | yes | The caller shows its prices |
| Q9 | but for the opening | Six legs, counted: R6 |
| Q10 | yes | Claims are paid once no row has a unit, and at no other time |
| Q11 | the order yes | The loss, then the fee, then the user. The claim of the fee is R11 |
| Q12 | yes | Capped. What the cap lets a wrong call cost is under 14.6 below |
| Q13 | for kinds 7 to 9 | By the landing against the call's time. Not for an entry: R9 |

F76 is closed on paper by the note applied whole (my test of it will have no state between to read, and must
then say "closed").

## 2. What the second writing opens

### 14.1 The life of a statement

Held: the last time to dispute can be kept by scripts. The dispute refuses a statement whose own output landed
after it; the answer, the lapse and a dismissal read when the dispute landed; "applied at once after the last
time" is an age of the lesser of a dispute period and the time left from the statement's landing to that time,
and one more. A statement nobody disputes is always applied by its period first (its landing is no earlier than
its time). A dispute that lands late happens once at most: the statement it leaves has landed after the last
time. A statement answered right late is applied at once, and nothing can come after it: right.

**R1 (Medium). A round between a claimer and its own key costs nothing, and a point only the claimer can answer
keeps everyone out.** Rule 2 gives the claimer half of the disputer's bond and the answerer the other half.
Where the claimer answers its own key's dispute it has both: "pays for every round" is not so; a round costs
two transactions and the oracle fee. A watcher gets in by answering the standing dispute right and naming its
point. For a row or an end it can (the proof is the script's) and it is paid half a bond for it; it has to win
one race, the claimer every one. For a price it needs the citation. In numbers: a shard of 500 KAS (bond 5)
with a row r no committee answers. Its owner states a draw of 400 at a made-up price of r, and its other key
disputes the price of row q, for which the owner has the committee's signed price in hand and has shown nobody.
It answers q at the end of the period and names q again (a point may be named twice); after that the last time
has passed. Cost: the fees. A watcher with the same citation answers q at once, has 2.5 KAS, names r, and the
draw lapses at its claimer's cost.

So two things: (a) a part of the disputer's bond stays in the copy whoever answers (a quarter to the claimer, a
quarter to the answerer, half kept: a round then costs half a bond, as it did); (b) a requirement on the
committee's service that nothing in a script can hold: a price it has signed for one is had by anyone at once.
With (b) R1 is small; without it the claimer's to show is the claimer's to hide.

**R14 (Low).** The state that stands again today writes `LATE_AGAIN` over the lateness. The last time to
dispute is of the lateness, so the state must keep it (a flag beside it, not in its place), and
`KpfTChangeLate` must still refuse a statement that stands again.

Kinds 2 to 6 under the new life: nothing I find that breaks. A lapse of kinds 2 to 4 gives both bonds back and
ends the statement, as today; a dispute of them is free only as it is today.

### 14.2 A covered account

The two places, as the scripts are today:

| Script | The first ask (the count) | The second ask (the time) |
|---|---|---|
| `KpfTOpenC` | asks nothing of it: **R6** | asks nothing of it: **R6** |
| `KpfTCEnd` | copies it (must count one less where a leg leaves alone) | copies it (must write the time for a take and a leaving, none for a step) |
| `KpfTAddC`, `KpfTLeft`, `KpfTChange`, the enders | copy, or see a hash | the same |
| `KpfTHand` | copies | **reads it** as the least margin of whoever takes the place: a time of 7,000,000 asks 0.07 KAS of margin. Harmless, and not meant |
| `KpfTStepEnd`, `KpfTStepEnter` | read (the old form: refused on a covered account by 14.2) | read |
| `KpfTAcctEnd` | copies nothing (it ends); should ask a count of nothing | |

**R6 (High unless the opening is taught; one line each). The count is whatever the opening says.** Run:
`t1b_r49_a_covered_account_is_opened_with_any_numbers_where_a_position_has_its_asks`: a covered account is
opened at the engine with a count of 58 below nothing, with a count of six and no leg, and with a time far
ahead. Both keys sign the opening, and for an account of the owner's own they are one party's. Opened at -58 it
takes 64 legs, and Q9 is back for exactly the account Q9 was about: under a call a step takes six legs, so
nobody steps it and "the rows empty without any user's leave" is not so. The opening must hold both places to
nothing.

**R7 (High). The time written is the longest a statement can live, so between two statements nothing is
free.** "Its own time, the lateness and two dispute periods" is when a statement disputed to its end is
applied. Its user states again in the transaction after.

- A shard: its owner states a draw of a sompi, has its other key dispute it through its life (R1: for fees, or
  for half a bond a round with R1's fix), and states the next the moment it is applied. The shard is under a
  statement all the time: no call is stated on it and no note applied. A note that landed before a draw ends
  that draw; where no user has left lately there is none. An owner that is under water buys time this way.
- An account: one below its maintenance cannot take, but a leg that lost leaves alone where the coins pay it.
  An account of the owner's own with five small losing legs and one large one leaves the small ones in turn,
  each statement kept to its full life: five lives in which no step is stated, while the large leg runs.

"Free for at least as long as it was held" needs the time to be the statement's time and twice its longest
life. It costs a user that long between two statements of its own.

Held: a statement that is ended writes no time and costs a bond, and whoever answers it wrong can state its
step on the output its own answer makes, before the user can state again. A step followed at once by a user's
statement: the step writes no time, so the user may; that statement then writes one. An entry is stated at or
after the account's time and writes one; undone, the account has the time it had. The account of no leg: a
take with no row and `KpfTAcctEnd`, as round 48 read them.

### 14.3 The entry

**R3 (High). A price of the shard's rows that nobody answers lapses by `KpfTPoint.lapse`, which knows one
bond: the statement's, which is the user's.** The table says such a lapse is at the owner's cost. "Ended by
the scripts that end a statement as they are" gives the other: the lapse pays the disputer its bond and half
the statement's bond, restores the marked account with the owner's bond in it, and `KpfTEnterOff` pays "what
the object held beyond the account's coins" to the owner. In numbers: a long of 1,000 KAS of worth on coins of
55; bonds of 0.55 each. The leg is up 3 %. The owner's other key disputes the price of a row whose committee
does not answer inside the period. As the scripts are: the user loses 0.55 (0.275 of it to the owner's key),
the leg is undone and owed nothing, the owner has its bond back. This is Q4 with the committee slow. The lapse
needs a branch for kind 10 and a point from 1,000,001 (the owner's bond: half to the disputer, half to the
user; the user's bond back; the marked account holding neither), and `KpfTEnterOff` must not pay the owner
what is the user's.

**R5 (High). A late landing costs the user its bond, whoever landed it.** `KpfTChangeLate` gives the
statement's bond to whoever ends it. The transaction that states an entry is signed by two; whoever signs last
holds it. The owner signs last, lands it one block after the lateness, and ends it with its other key: 0.55
KAS of the user's on every entry, the leg undone. And inside the lateness the last signer has the entry or not
as the price went in those 10 seconds, for nothing. "Both tools land what they signed at once" holds nobody.
What does: the user signs last (its tool's rule), or the transaction spends an output of the user's that the
user spends elsewhere if it is not landed; and a late statement of kind 10 gives both bonds back, less a
keeper's fee to whoever ends it.

**R4 (Medium). Half a bond ends an entry, and a bond is 1 % of the account's coins.** After R3 and R5 the ways
left to have an entry end when the leg went against one are: the user, by a price of its own part that the
owner's tool did not catch; the owner, by a price of the shard's rows where the gate does fail at the
committee's price, or that nobody answers. Each costs its maker half a bond, the other half coming back to its
own disputing key: 0.275 KAS on a leg of 1,000 KAS of worth, 0.03 % of it. A price moves that in seconds; over
a statement's life (25 seconds with the tests' terms) one standard move of a coin at 100 % a year is 0.09 %.
So the option is in the money most of the time it is wanted. The bonds of an entry should be a rate on what
the leg is worth, of what a price moves in a statement's life: 0.5 % of the worth (5 KAS here) prices it out
at any dispute period the bound allows.

**R2 (Medium). A dismissed dispute costs nobody and can be made again.** Beyond the tolerance with the gate
holding: "dismissed, each bond back". The same point is then disputed again, to the last time. Each time
somebody must answer with a citation (4.4 kB, the oracle fee, no pay), and if once nobody does, the point
lapses: the entry ends at the owner's cost, half to the disputer. A third party plays that for nothing against
any owner whose price of a row was a little beyond the tolerance; and an owner that stated one so on purpose
has, with its other key, a dispute that keeps the statement to itself at no bond at all (R1 again).

**R8 (Medium). The gate is computed again with one price in the place of one: a price wrong in two rows passes
each dispute.** A gate that holds by 10 KAS at the prices said; two rows each said so that the committee's
price takes 8 off the equity. Either dispute alone: the gate holds by 2, dismissed. Both together it fails by
6, and nothing looks at both.

One change closes R2 and R8: **a dismissed dispute corrects what is said.** The statement stands again with the
committee's price in the place of the price said (its argument's hash is another; its outputs do not read the
shard's prices). The next dispute of that row finds it within the tolerance and loses its bond; the next row's
dispute computes the gate with the row already corrected.

The residue of Q4 (the leg's own price, and the prices of the user's other legs): is "a tool signs only well
within the tolerance of what it sees" enough for a first form. It is, on two conditions, and I would write
both down. (a) The rule is two-sided: a tool refuses a price that is off in its own favour as well. An entry
at 0.6 % below what the user sees, for a long, is the one an owner plants: a leg that gains is ended by a
dispute of that price at the user's cost; one that loses settles. (b) "What it sees" has to be within about a
fifth of a per cent of what the committee will sign for that time. For a liquid market it is. In a fast one,
10 seconds of lateness is by itself that much, and the tool should refuse; or, better, R1's requirement gives
it the committee's own price for the time before it signs. Who gains from which ending, walked through, is in
the table below.

| Ending | The leg went the user's way | The leg went the owner's way |
|---|---|---|
| a row or the end is wrong | the owner cannot make one (the tool computes them) | the user would need the owner's tool to sign a wrong row: it does not |
| a price of the user's part beyond the tolerance | the owner disputes it if the user signed one (the leg's own price, planted in the user's favour): the user loses its bond and the gain | the user disputes a price of its own that the owner's tool missed: half a bond (R4) |
| a price of the user's part that nobody answers | the owner disputes any of them while the committee is slow: the user loses its bond and the gain. The prices are the claimer's to show; here the owner signed them too | the user does the same: half a bond |
| a price of the shard's rows, gate fails | the owner, where it stated a gate that does not hold: half a bond to the user (R4) | nobody gains |
| a price of the shard's rows that nobody answers | the owner: R3 as the scripts are; half a bond as the table has it | the user disputes it: the owner's bond, half to the user: an option of the user's at the owner's cost |
| a late landing | the last signer (R5) | the last signer |
| a third party | R2's dispute for nothing; half a bond wherever a point is wrong or lapses | |

The third row is what "the claimer's to show" becomes where two signed: while the committee does not answer,
either signer ends any entry, and the cost falls by the kind of point, not by who gained. I see no rule of
bonds that mends it; the correction left for later does (an entry is not ended for a price), and so does R4's
size of bond, by price.

### 14.4 with 14.6

**R9 (Medium). Q13 is not closed for an entry.** 14.6 ends every statement of kind 10 on an account of a
called shard by showing the call. An entry that had stood its period before the call's time and was not yet
applied: the leg gained, the owner ends it by showing the call (it owes nothing); the leg lost, the owner
applies it and the account is stepped. The rule the paper has for kinds 7 to 9 fits: an entry that landed a
dispute period or more before the call's time is applied, not ended.

**R10 (Medium). A call that was right only by what a pending leg showed is not undone when that leg is.** The
row I asked for says the entry is ended by showing the call; the call stays, and "no other end" steps every
account. An owner with an entry of its own pending the way the price then went has its shard called, with no
user's leave. What it gains is the moment: every account is closed at a price of the last lateness that it
picked, and every leg that leaves at a gain leaves the band behind (1 % of its worth: on users long 2,000 KAS
of worth and up 3 %, 20.6 of their 60). Users are paid; they are closed. In this form a call cannot be decided
on the books without the pending legs (they are not told apart there); the fallback of 14.4 can. I would
accept it for a first form and say it in 14.7 beside the lateness.

The bound with the lateness in it: right as written, with R7 (without it the account's part of the bound is
five lives, not one).

### 14.6 The call

**R11 (Medium). The claim of a fee that is paid whole and first is a way out of a failing shard for whoever
has many small accounts in it.** Every account with a leg holds a note's least (every test counts the coins
less it, since F75). An account of the owner's own with coins of a note's least and a hair: the call's price
takes the hair, the coins "hold no fee", the stepper (the owner's other key) has a claim of 0.1 KAS paid whole
and first, and the user (the owner) has its 0.2 KAS back. A thousand such accounts take 100 KAS before any
claim of a user's, from a shard that may hold 45. Take the fee out of the note's least instead: it is there in
every account that has a leg, it is twice the fee, and no claim of a fee is needed.

**R12 (Low). The cap of the bond, and a wrong call.** A wrong call whose end does not follow from its own
prices is ended at once by anyone (the script proves it), who states the right call on the output it made. A
wrong call with a false price stands until a watcher has the citation (it answers the caller's own dispute if
there is one, R1, and names the price), a dispute period at most; it costs its caller 10 KAS at the cap. Who
gains is an owner that would rather not be called: 10 KAS a period on a shard of any size, against 1 % of the
shard before the cap. With R7 closed and a committee that answers anyone promptly that is a delay of seconds
per 10 KAS; I would keep the cap.

**R13 (Low).** A note of the rest that stands when a shard is called: `KpfTLeft.rest` pays none while it is
called, and the section does not say that it is a claim. It should be one, at the same share.

Held. A statement that landed before the call's time and is applied at its own prices into a called shard: its
note is a claim like any other; nobody can land before a time that is already past, so it is no way to leave
at a price of one's choosing after the call. What the caller does choose, by setting the call's time up to a
lateness back, is which leavings of the last 10 seconds stand: the lateness again. Who can keep a row from
emptying: an account under a statement that landed before the call's time, for that statement's life and no
longer (what it states after is ended by the step); an account that is under an entry or marked (cleared by
anyone, paid); a note not applied (anyone, paid its carrier); an account whose user adds a sompi each block
(the oracle fee each time, and the step is built on the unconfirmed output). None of them for good. The one
that is for good is R6. Claims at one share once no row has a unit: the share is the same for the first and
the last, to what rounding leaves.

## 3. The first form, with the cuts

Can it be finished: yes; nothing in it needs something the scripts cannot do. Can it be used on testnet-10 by a
clearing party and users who are not its friends: not as written. The least I would insist on before a coin of
a stranger's is in it:

1. R3 and R5: an entry's lapse and its late landing must not cost the user for what the owner did.
2. R6: the opening holds the count and the time to nothing.
3. R7: the time is twice the longest life, for an account and for a shard.
4. Outside the scripts: a price the committee has signed is had by anyone at once (R1, and the tool's rule).
5. Said to the user in plain words, because no script says it: the band (14.9); that a call closes every
   account at a price of the last lateness; and that it should read a shard before it signs.

R1's quarter, R2 and R8's correction, R4's bond, R9, R11 and R13 are each a line or a constant, and I would
take them in the same writing.

What I would cut more: **the dispute of the shard's four prices.** The gate stays computed when the entry is
stated, at the prices said; nobody disputes them after. That takes out `KpfTEnterPrice`, the owner's bond in
the statement's object and its three ways out, the special lapse, the points from 1,000,001, and with them R2,
R3, R8 and half of R4. What it costs: the upper level at an entry is then the owner's word and the entering
user's own check (its tool computes the gate at the prices it sees, which it needs the four markets for). A
shard let below its upper level by a false gate cannot be drawn from (a draw's prices are its own, and
disputed) and is called at its lower level (the caller's prices). Nobody is unpaid by it who would not be
without it; what is lost is a watcher's power to undo an entry that should not have been let in, which
protects the users already there from a call that comes sooner. I would take that trade for a first form.

## 4. The order

(a), (b), (c), (d) as written, with these in them:
- in (a): the two times with R7's constant; the opening (R6); the lateness kept in a statement that stands
  again (R14); the second note script (the committed one applies a leg at a time).
- before (b): decide the cut above. If the shard's prices stay disputed, (b) has a branch in `KpfTPoint.lapse`
  and in `KpfTChangeLate` for kind 10, which "as they are" does not say.
- in (c): R9, R11, R13.

## 5. The bound as a number

The claim needs a price to move less than the maintenance less the tolerance (4.5 %) in a lateness, two dispute
periods and a visit. Taking a visit as 5 seconds and the chain at 10 blocks a second:

| Dispute period | Lateness | The interval | 4.5 % in standard moves, a coin at 100 % a year | at 300 % a year (a bad day) |
|---|---|---|---|---|
| 5 s (50 DAA: the tests) | 10 s (100 DAA: the terms) | 25 s | 50 | 17 |
| 20 s | 10 s | 55 s | 34 | 11 |
| 60 s | 10 s | 135 s | 22 | 7 |
| 300 s | 10 s | 615 s | 10 | 3.4 |

(One standard move in t seconds is the yearly rate over the root of 31.5 million, times the root of t: 0.018 %
a second at 100 %.) Prices of such coins jump, so I would not rest on fewer than six to eight standard moves of
a bad day: that is an interval of two to three minutes at most, a dispute period of 45 to 90 seconds with a
lateness of 10. At five minutes a move of 4.5 % inside the interval is a three-and-a-half standard move on a
bad day: with some 140 intervals in a day, about one day in ten of such days. So "minutes" for a committee to
sign is at the edge: one minute holds, five do not, unless the maintenance is twice what it is (9.5 % in five
minutes is seven standard moves of a bad day) or the fallback of 14.4 takes the entry out of the bound. Note
that the fallback does not take an account's own statements out of it: an account under its user's take or
leaving is stepped by nothing for that statement's life either way.

## Not reached

- Nothing of section 14 is built: all of the above is reading, but for the probe of the opening.
- The third numbers model was not run with a tick of a lateness, two periods and a visit; the table above is
  arithmetic on a rate, not a model.
- The scripts of a note of two to six legs, `KpfTEnterS`, the step under a call: sizes not estimated.
- The cut proposed in 3 is argued, not checked against the third model.

## Findings ledger

| | Severity | |
|---|---|---|
| R3, R5, R6, R7 | High, on paper (R6 run) | before a stranger's coin |
| R1, R2, R4, R8, R9, R10, R11 | Medium, on paper | |
| R12, R13, R14 | Low, on paper | |
| Q1 to Q13 | | as the table in 1 |
| F76 | Medium | open in what is committed; closed on paper by 14.2 |
| F74, F68 | | open; 14.2 closes both |
| F61, F66, F67, F41 | | as before |

## Files changed and counts

- `crates/kpf-protocol/tests/auditT1_builders.rs`: 149 tests pass (one new:
  `t1b_r49_a_covered_account_is_opened_with_any_numbers_where_a_position_has_its_asks`).
- `docs/audit/REVIEW-T1b.md`: this round appended.

```
CARGO_BUILD_JOBS=2 cargo test -q -p kpf-protocol --jobs 2 --test auditT1_builders   # 149 passed, 0 failed
```

# Round 50

Scope: the first part built of section 14, "(a) 14.1 and 14.2", at the engine: the life of a statement (the
quarter split, a right answer from anyone, the lateness kept beside the mark, the last time to dispute in
`KpfTPoint.lapse`, `KpfTPoint.dismiss` and `KpfTChangeSettle`), and a covered account's count of legs, its time,
the opening, and the note applied whole by two scripts. My files brought along; the whole workspace run once.
Committed: 970caa7 and c513757; (a) is in the working tree. No src, contract or `kpf-out` file touched; no
commit.

## Headline

- (a) does what section 14.10 says it does. The differential tests agree again on the changed scripts: 0
  partings on 180 ends (now with the account's time about the statement's and counts that are not the legs'),
  180 rows, 240 notes over the two note scripts.
- **Three findings.** **F77 (Medium)**: the two quarters of a right answer are outputs too small to be mined
  where the bond is under 0.2 KAS: the builder writes no right answer on an account of fewer than 20 KAS (bond at
  1 %); before (a) it wrote one at 5. **F79 (Medium, one line)**: a lateness of the mark or more makes a
  statement as first stated read as one that stands again: it is applied at once, with no dispute; an account
  with such a lateness is opened. **F78 (Low)**: `KpfTHand` reads the account's time as a least margin.
- F72, F73, F75, F76 and R6 are closed. F74 and F68 are open, as you said they would be until (b).
- The whole workspace, run once at the end: 1,275 tests passed, none failed.
- The third writing, before (b): nothing in it that I would call wrong; two remarks, under "The third writing".

## Findings

### F77 (Medium). The two quarters of a right answer cannot be mined where the bond is small

A right answer now pays two outputs, a quarter of the disputer's bond each. An output of value v costs
10^12 / v grams of storage mass (KIP-9) and a block takes 500,000: two outputs under about 0.04 KAS each fit in
no transaction. The builder fills a small payout up to 0.025 KAS (400,000 grams): one such output fitted, two do
not, and it writes nothing ("too small to be mined").

`t1b_finding_f77_the_two_quarters_of_a_right_answer_are_outputs_too_small_to_be_mined_where_the_bond_is_small`,
the end of a take of the rule's, disputed, on accounts with the bond at 1 %:

| Coins | Bond | A right answer written and taken, from a third key | from the claimer |
|---|---|---|---|
| 5, 10, 16 KAS | 0.05, 0.10, 0.16 KAS | no | no |
| 20 KAS and up | 0.20 KAS and up | yes | yes |

Round 47's test had the answer written and taken at 5 KAS (one output of half the bond). So between 5 and 20
KAS of coins (and ten times that at a bond of a tenth of a per cent) a right statement that somebody disputes is
answered by nobody who uses these builders; for kinds 5 and up it lapses at its claimer's cost, half its bond to
whoever disputed. A step of such an account is ended by its user's dispute. Two of my older tests ran into it
and had to be given larger bonds (`t1b_fixed_f57_...`, `t1b_r42_...`).

The script asks "at least" a quarter, so an answer with both outputs filled up to about 0.05 KAS is taken. For
the claimer that is its own coins to itself. For anybody else it is a gift to the claimer larger than the
quarter it earns: below a bond of 0.2 KAS "whoever answers is paid" pays nobody but the claimer.

Closing it: the builder fills the two outputs to what the transaction's storage mass allows (it knows the
number: its refusal prints it); and, so that a third party is not out of pocket, the script pays whoever
answered only where a quarter is a payable amount (the terms' carrier, say), the claimer having the half
otherwise.

### F79 (Medium; one line). A lateness of the mark or more: a statement applied at once, with no dispute

A statement that stands again is told from one as first stated by its lateness being 499,999,999,999 or more.
Nothing holds an object's own lateness below that: no opening reads those bytes of the moving part, and
`KpfTChange` writes whatever stands there into the statement. With a lateness of exactly the mark,
`KpfTChangeSettle` reads a statement as first stated as standing again with a lateness of nothing: its last time
to dispute is its time and one dispute period. It is never late (`KpfTChangeLate` and `KpfTPoint.dispute` compare
with the whole number), so it is stated at a time of any age; stated at a time a dispute period ago it "was
written after the last time to dispute" and is applied at once. A dispute of it has landed late whenever it
lands: it lapses nothing and is dismissed at its disputer's cost.

`t1b_finding_f79_a_lateness_of_the_mark_is_a_statement_applied_at_once_with_no_dispute`: an account of 100 KAS
with that lateness is opened by `KpfTOpenC` (engine; under the id its own terms give). A stranger states on it a
statement of kind 9 with no trace whose one output pays the stranger all the account holds; it is applied in the
next transaction (engine, at no age); a dispute of it is dismissed (engine). The same on an account of the
tests' lateness waits its period. (Kind 1 is applied by the same `KpfTChangeSettle`, so "kind 1 is untouched" holds
for every lateness below the mark and not for one at it: a position with such a lateness is the same.)

Before (a) such a lateness was an old price that could still be disputed for a period. It is a term both
parties signed, and a tool that holds the terms to their bounds opens no such account (the builder does not);
so Medium. But the paper's "the terms keep the lateness short" now carries more than a price of the past, and
one line holds it: `KpfTChange.state` (and `KpfTEnter.state`) refuse a moving part whose lateness is the mark or
more.

### F78 (Low). `KpfTHand` reads the account's time as a least margin

`KpfTHand` asks that whoever takes side A's place holds at least what stands where a position has side B's ask.
On a covered account that is now its time, a DAA score, read as sompi.
`t1b_finding_f78_the_hand_reads_the_accounts_time_as_a_least_margin`: an account of 2 KAS is handed over before
its first statement; with a time of 300,000,400 (testnet-10's scores are of that order and more) it is not
(model and engine); with 4 KAS it is. A line in `KpfTHand` (the hand of a covered account asks no least
margin), or in (b) with the other refusals. The old take's end (`KpfTStepEnd`) reads the same two places as
floors, until (b) refuses the old kinds.

## What held

### a. The life of a statement

`t1b_r50_the_life_of_a_statement_a_right_answer_from_anyone_a_late_dispute_dismissed_applied_at_once_after`, on
an account whose bond is 10 KAS and 3 sompi (no multiple of four); time 6,500, lateness 100, period 50: the last
time to dispute is 6,650.

| | At the engine |
|---|---|
| a right answer by a third key, by the claimer, by the disputer | taken; a quarter (2.5 KAS) to the claimer in the last output, a quarter to whoever answered in the one before; the copy keeps 5 KAS and 3 sompi, and not a sompi less; the two quarters the other way round are refused |
| the statement after it | stands again with the mark and its lateness; disputed again in time and answered right again it is the same state |
| a dispute that landed at 6,650 | lapses; is not dismissed |
| a dispute that landed at 6,651 | is dismissed (the statement again, a quarter each, the copy's part to the sompi); does not lapse. The same of a statement that stands again |
| a late dispute, answered | right where the point is right; wrong where the statement is wrong (it ends) |
| the settlement of a statement as first stated | waits a dispute period (the age said less is refused) |
| of one that stands again, landed at 6,600 or 6,649 | waits a dispute period |
| of one that stands again, landed at 6,650, 6,651, 9,000 | applied at once; the landing read is the statement's own |

A draw under it (`t1b_r50_a_draw_under_the_new_life_and_its_guard_on_a_draw_that_stands_again`): answered right
by a third key; standing again it is ended by a note that landed before (and not by one that landed with its
last landing); a late dispute of it is dismissed; after the last time it is applied at once. Kinds 2 to 6: every
test of mine on their answers passes on the new `ends` (rounds 34 to 46), with the claimer's half now a quarter
where a test asked the amount.

Your questions, by reading where not run:

- **Kept beyond `t0 + late + 2 * window`.** Three ways, none for nothing. (1) Late disputes: `KpfTPoint.dispute`
  cannot refuse one, each is dismissed by anyone at once for a quarter, and costs its maker a whole bond; whoever
  dismisses builds the settlement on the output it made. (`dispute` could refuse a statement whose own output
  landed at or after the last time: one line, and then there are none after the first.) (2) A dispute in time
  that nobody answers and nobody lapses stands until somebody does; anyone may, and for kinds 2 to 4 nobody is
  paid to. (3) For kinds 5 and up, a claimer whose other key disputes a price nobody else can answer and lets it
  lapse loses half a bond a period and may state again at once (an ended statement writes no time): a shard is
  kept under draws so for half its bond a period. That is a price, not a bound; it is R7's residue and wants
  saying in 14.5.
- **`LATE_AGAIN + late` against readers of the plain number.** `KpfTChangeLate` and `KpfTPoint.dispute` compare
  with the whole number: a statement that stands again is never late, as before. `KpfTDrawNote` is shown the
  lateness and hashes it (run). `KpfTKind` and kind 1's family do not read it. The readers (`aobj.rs`,
  `acmds.rs`) keep the number as it is and use `again()`. What does read the mark as a threshold is F79.
- **A late dispute answered wrong: is it right that it is taken.** Yes. A wrong statement ends whenever it is
  shown wrong; what the last time takes away is the lapse, which is the claimer's to show, and that is right
  too.
- **The quarter at 1, 2, 3.** A quarter of nothing: no output is asked and the copy keeps the bond. Not run
  (no such output is written by anything); by reading. **A key that cannot be paid**: an output nobody spends;
  nothing else follows. **Who takes the answerer's quarter**: whoever lands first, with the proof the first one
  showed. For a citation that is the citation's writer's work taken; it costs the protocol nothing.
- **"Applied at once" against a dispute in the same block**: they spend the same output; one lands. If it is
  the dispute, it is late by construction and dismissed.
- **Not naming the next point in the same transaction.** I agree with leaving it out. A transaction that
  spends the output of the answer can be sent with the answer by whoever answers, claimer or watcher, so the
  two forms give each the same move; and as built a round costs the claimer half a bond. What decides is who
  can answer, which no form changes.

### b. The count and the time

`t1b_r50_the_count_of_legs_and_the_time_of_a_covered_account`: a take at 6,500 is applied and the account says
6,900 (2 * (100 + 2 * 50) after its time); a take, a leaving of one leg and of all stated before 6,900 are wrong
at the engine, written as the rule would write them; at 6,900 and after, right. A statement whose time is after
its landing is neither disputed nor applied (engine): a user cannot state before its time by saying a later
one, and cannot say a time that is not yet. A step stated at 6,501 on the same account, once it is below its
maintenance, is right; it writes the time the account had and counts one leg less. One leg of two leaving
counts one; the only leg leaving alone leaves an account of no leg (count nothing, the chain of no leg), which
takes a take stated with no row (48 bytes), all but a note's least, and no other kind; a take with no row on an
account that has legs is wrong.

The two places against the scripts that are shown them: `KpfTOpenC` holds both to nothing (engine;
`t1b_fixed_r6_...` and a case in `t1b_fixed_f64_...`); `KpfTCEnd` writes them; `KpfTAddC`, `KpfTLegAdd`,
`KpfTLegOut`, `KpfTLegMove` and the old kinds' ends copy them; `KpfTHand` and `KpfTStepEnd` read them (F78).
None but `KpfTCEnd` writes a number there. What does come apart until (b) is the count and the legs: a leg
added by `KpfTLegAdd` or taken out by the old leaving is not counted.

**A count that is not the legs'** is copied and counted down by the script, which holds it to nothing (run: one
leg counted as five leaves alone, and four are counted); the model's `c_check` states on no such account. So
at (b) the count is exactly as good as the three things that write it: the opening (nothing), the entry (one
more, and it should refuse a count below nothing as well as a sixth), a leg leaving alone and `KpfTEnterOff`
(one less). With the old leg scripts refused in the same transaction as the entry, as 14.2 has it, I see no
fourth.

Overflow: `t0` is below 500,000,000,000 at stating. The next time is computed for every kind, a step too, from
the lateness and the period in the moving part: an account whose periods do not fit has no end anybody can
answer (the model says the same: neither verdict). Such periods are the account's own terms.

### c. The two note scripts

`t1b_r46_a_note_that_legs_left_is_applied_as_the_model_says`, brought back to the note applied whole: 240 notes
on random books, of one leg by `KpfTLeft` and of two to six by `KpfTLeftAll`: 103 applied as the script's
arithmetic written out gives (and not with a sompi more paid), 137 refused (a leg of another size, entry,
direction or market, the same leg twice where it is once in the books, no leg, seven legs, what is asked beyond
64 bits, a called shard); the model and the arithmetic part on none. Asked below zero by a sompi, by up to 300
KAS, and at the 64-bit end; the note holding what was paid or only a note's least. The one-note test: 28
transactions refused around a note of two legs. `t1b_fixed_f76_...`: a note of two legs is applied in one
transaction and in no part (`KpfTLeft` shown one of its legs; `KpfTLeftAll` shown one leg; a note of one leg
shown to `KpfTLeftAll`: all refused); two notes in either order leave the same shard. The statements' notes:
54 notes of statements of the rule's applied whole, 3 waited on a called shard.

### d. My tests of rounds 45 to 49

| Test | Now |
|---|---|
| F72 (`t1b_fixed_f72_six_legs_at_most_...`) | closed: the model states on no account of seven legs; the note that six legs left is applied whole in one transaction (6.3 kB). The script that will refuse a seventh leg is the entry's, in (b) |
| F73, F75 | closed, unchanged |
| F76 | closed: `t1b_fixed_f76_...` |
| R6 (round 49's probe) | closed: `t1b_fixed_r6_...` |
| F74, F68 (both paths), item f of round 47 | open until (b), as said |
| the test of what a shard is good for | passes: 23,556 statements, each stated at or after the account's time; accounts of no leg in it |
| the two differential tests of round 47 | pass: 180 ends (65 right, 110 wrong, 5 neither), 180 rows; 0 partings |
| round 47's "a leaving held open by the same point" | replaced by the test of the life: a point is still disputed again until the last time, for half a bond a round |
| round 47's "an account of five KAS is answered" | is F77 |
| round 47's P1 (an ask read as a mark) | removed: the two places are a count and a time, and the mark is in the length |
| round 48's interleaving of notes a leg at a time | gone with it; either order of two whole notes is in `t1b_fixed_f76_...` |
| round 46's two tests of the note | back to the note whole, over the two scripts |
| F53 (round 30) | moved: the state that stands again is the mark and the lateness; a quarter where it asked half |
| F57, round 42's M1 | given a bond of a tenth: F77 |
| the table (`auditT1_tests.rs`) | 59 scripts, 68 entries, 57 `release`; `KpfTPoint` reads the oracle fee a fourth time (the dismissal) |

## The third writing, before (b)

Nothing I would call wrong. Two remarks.

1. **A price nobody answers ends an entry at nobody's cost.** Right between the two signers. It also makes a
   false entry free to try for two who are one: the watcher that disputes its price and waits for the lapse
   costs them nothing. Whoever watches should answer such a price wrong, with the citation (the user's bond,
   half to the answerer), and leave the lapse for when there is no citation to be had. Worth a line in 14.7.
2. **"The owner signs first; the user signs last and lands it."** Right for the late landing. What it leaves is
   an owner's signature that the user holds: for the lateness an entry or not as the price went, and after it a
   leg the user can put into the shard's books at any later time for the price of its bond (it lands late, is
   ended, undone, its note applied: a phantom for as long as that takes, at a moment the user picks). The owner
   keeps it short by spending an output of its own in the transaction it signs, which it spends elsewhere when
   the entry has not landed in its lateness. A rule for the owner's tool, as the price margin is.

And F79's line belongs in `KpfTEnterS` too, when it is written.

## Not reached

- A bond of 1, 2 or 3 sompi and a key that cannot be paid: read, not run.
- The age a lapse asks (`int(ageb) >= window`): the builders set the age and the slot holds the script's word
  to the input's; I ran the settlement's age (less than a period: refused) and not the lapse's.
- `KpfTAcctEnd` on an account of no leg that has a time; the old kinds on an account with a count and a time
  beyond reading which places they read.
- `thin_read.rs` and the CLI's readers of a statement that stands again: read by a search, not run.
- Sizes: only what the tests print (the note of six legs applied whole: 6.3 kB).

## Findings ledger

| | Severity | State |
|---|---|---|
| F77 | Medium | open: `t1b_finding_f77_...` |
| F79 | Medium (one line) | open: `t1b_finding_f79_...` |
| F78 | Low | open: `t1b_finding_f78_...` |
| F72, F73, F75, F76, R6 | | fixed, verified |
| F74, F68 | | open until (b) |
| F61, F66, F67, F41 | | as before |

## Files changed and counts

- `crates/kpf-protocol/tests/auditT1_builders.rs`: 151 tests pass. Brought along as the table in d says. New:
  `t1b_finding_f77_...`, `f78`, `f79`; `t1b_fixed_f76_...`; `t1b_r50_the_life_of_a_statement_...`,
  `t1b_r50_a_draw_under_the_new_life_...`, `t1b_r50_the_count_of_legs_and_the_time_...`.
- `crates/kpf-tests/tests/auditT1_tests.rs`: 55 pass (the table at 59 scripts, 68 entries, 57 `release`).
- `crates/kpf-cli/tests/auditT1_cli.rs`, `crates/kpf-protocol/tests/auditT1_solvency.rs`: unchanged.
- `docs/audit/REVIEW-T1b.md`: this round appended.

```
CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast --jobs 2    # 1,275 passed, 0 failed, 140 result lines
                                                                      # auditT1_builders 151, auditT1_tests 55,
                                                                      # auditT1_cli 47, auditT1_solvency 11;
                                                                      # thin_draw 18, thin_account 13, thin_enter 8
```
