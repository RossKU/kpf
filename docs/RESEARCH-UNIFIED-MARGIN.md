# Research: margin that moves as one

Notes of a research begun 25 November 2026, toward `GOAL.md`. They say what was found and what is only supposed;
nothing here is built. The research runs at the level of models, scripts and the engine until a question can only
be answered on testnet, and stops there.

## 1. What must hold

The owner's frame: one bond behind many accounts is a means. What must not move is that, to a user, margin behaves as
if it were one and could be moved as one. Measured from the user's side:

| | Property | How it is tested |
|---|---|---|
| U1 | everything the user holds nets into one risk number | margin asked of a set of legs is that of their net, in the model |
| U2 | a gain is usable without closing | a gain of one leg is margin for another, or is taken out, in one operation |
| U3 | entering and leaving do not depend on who is on the other side | a side is handed over, or taken by a clearing party, in one transaction |
| U4 | it costs little | bytes and transactions an operation, and capital a clearing party must hold |

Means are compared by how much of U1 to U4 they give. Rules of the research, also the owner's: where there is a
choice, several small scripts are preferred to one large one (cost); public executors are taken to be there always;
an unknown is decided from the form the thing should have, and looked up without limit.

## 2. What a script can know (read in the engine's source, rusty-kaspa v2.1.0)

Found by reading `crypto/txscript`, `consensus/core` and `consensus/seq-commit` of the release the repository builds
against. None of it is run yet.

1. **No reference inputs.** Every introspection opcode reads the spending transaction's own inputs and outputs
   (`OpTxInput*`, `OpTxOutput*`, `OpInputCovenantId`, `OpCov*`, `OpAuthOutput*`). An object that is not an input of
   the transaction is invisible to a script. What the thin family does (an object or a script copy is spent and
   written again unchanged) is the only way to show a live UTXO, and it is one use a block a copy.
2. **A script can rely on another script that ran in the same transaction.** It reads the other input's signature
   script (`OpTxInputScriptSigSubstr`), holds the input to a known script under a known covenant id, and takes the
   arguments shown there as checked. `priced` does this with the epoch table's `attest`. This is a function call
   between small scripts, each with its own stack.
3. **The only tight limit is the stack: 244 items** (`MAX_STACK_SIZE`); a script may be a megabyte and run a million
   operations. A compiled entry keeps the arguments of every helper it calls on the stack to its end, which is why
   `KpfTProve.answer` and `KpfTReplace.replace` stand at the limit. Small scripts called by co-spending (2) are the
   way around it, and the owner's preference besides.
4. **A script can read the chain's past without spending anything: `OpChainblockSeqCommit` (0xd4).** Given the hash
   of a chain block that is an ancestor of the transaction's own, no deeper than the finality depth (the accessor is
   built with `finality_depth`: 432,000 blue score, about twelve hours at ten blocks a second), it pushes that
   block's sequencing commitment (KIP-21, in the header's `accepted_id_merkle_root`). The commitment is a tree a
   script can recompute with keyed BLAKE3 (`OpBlake3WithKey`, 0xda; the hashers are domain-keyed):

   ```text
   SeqCommit(B) = H(parent_seq_commit, state_root)
   state_root   = H(activity_root, payload_and_ctx_digest)
   activity_root = H(inactivity_shortcut, lanes_root)        lanes_root: a sparse Merkle tree over lanes
   lane tip     = H(parent_ref, lane_key, activity_digest, context_hash)
   activity_digest = Merkle root of H(tx_id, version, merge_idx) over the lane's transactions the block accepted
   context_hash = H(timestamp, daa_score, blue_score)
   payload_root = Merkle root of H(block_hash, blue_work, H(miner's payload)) over the merged blocks
   ```

   A lane is a subnetwork id (20 bytes); every transaction of the thin family is in the native lane.
5. **A version-1 transaction's id is `H(payload_digest, rest_digest)`** (`id_v1`): the payload, or the rest (inputs
   without signature scripts, outputs with their covenant ids), can be shown without the other.

What follows from 4 and 5, supposed and to be built at the engine:

- **A transaction accepted in the last twelve hours can be cited by any number of other transactions**, each with a
  proof of some hundreds of bytes (a Merkle path in the block's native lane, the lane's leaf in the sparse tree, four
  hashes to the commitment), with nothing shared spent and so with no contention.
- **What it did can be cited too**: an output of it under a covenant id was authorized by that id's own scripts
  (consensus holds an output's covenant binding to an authorizing input), so "an output under the committee's id
  with this state exists in an accepted transaction" is the committee's word, checked by consensus once and read by
  everyone after.
- **Miners' payloads are committed** (`payload_root`): a price a miner writes in its block is readable by a script
  the same way. That is the form of the end the owner named (prices from miners), reachable from the same reader.
- It shows events, not states: that a transaction happened, never that an output is still unspent.

What was checked of the way to build it (read, not run):

- The script language has what the reader needs: `OpChainblockSeqCommit(block)`, `blake2bWithKey(data, key)` (the
  commitment's hashers are BLAKE2b keyed by their names: `SeqCommitLaneTip`, `SeqCommitActivityLeaf`,
  `SeqCommitMerkleBranch`, `SeqCommitActiveLeaf`, `SeqCommitActiveNode`, `SeqCommitActivityRoot`, `TransactionV1Id`,
  `TransactionRest`, `PayloadDigest`), `blake3WithKey`, `OpOutpointTxId`.
- The lanes' tree is a sparse Merkle tree of depth 256 whose proofs are a bitmap of 32 bytes and one hash a
  non-empty sibling, about `32 + 32 log2(lanes)` bytes, and stop early at a collapsed subtree, which is hashed with
  its key. A reader need not hold the proof to the native lane's key: a transaction is in one lane only, and that it
  is under some lane's tip is what is cited.
- The engine's own tests run the opcode with a mock accessor; the repository's `verify` builds the engine without
  one and would take it as an argument for these tests.

Not known: whether the opcode is active on testnet-10 and when on mainnet (the engine has it only where the
accessor is given; the repository's `verify` gives none). **This is the first question only testnet answers.**

### 2a. What the specification says (KIP-21 and the node's documentation, read 25 November 2026)

Read after the probe was built, and it agrees with what the source showed: the hashes are BLAKE3 keyed by their
tags; a lane is a transaction's subnetwork id and nothing else; an activity leaf is the id, the version and the
place among the block's accepted transactions; the opcode gives the commitment of a block on the selected-parent
chain of the validating block's selected parent, within the finality depth in blue score ("about 12 hours"), and
fails the script otherwise. Three things the source had not told:

- **A lane of one's own.** Beside the native and the coinbase lanes there are user lanes: a subnetwork id that is a
  namespace of four bytes and zeros. A block carries at most 50 lanes other than the coinbase's. Notes written in a
  lane of their own would make its tip the last note and a proof a few hashes, where the native lane's activity is
  every transaction of the block. What such a transaction costs (its gas) is not read yet.
- **Miners' payloads** are committed for every merged block, also those whose coinbase was not accepted, by block
  and not by lane.
- **Status.** The KIP is "Active" and names the experimental testnet (TN12) as carrying the opcode. Neither page
  says testnet-10 or mainnet. A second-hand report says covenants are live on mainnet since the Toccata fork; the
  testnet-10 node this repository uses does accept covenants. So the question of section 2 stands as it was, and a
  transaction that uses the opcode is what answers it.
- The node's source has a call that returns a lane's witness under a block's commitment
  (`get_seq_commit_lane_proof`); whether the wRPC serves it, and what gives the path of one transaction in a lane's
  activity, is not found in either page (the proving companion of the KIP is a stub).

## 3. The means, against U1 to U4

| Means | U1 nets | U2 gains usable | U3 any counterparty | U4 cost | State |
|---|---|---|---|---|---|
| One account between a user and a clearing party, many legs, one margin | yes, in the account | with a re-mark | the clearing party is the counterparty of everything | one UTXO a user | two legs today, one script at the stack's limit |
| A leg or margin moved between accounts in one transaction | across accounts, by moving | yes | yes | a transaction a move | `KpfTReplace` moves a side; nothing moves margin |
| The entry moved to a signed price (re-mark) | | yes: the gain becomes margin | | a price read a re-mark: 8 to 11 kB and one use a block an epoch table today | not built; today's re-strike is at the two orders' price (review F41) |
| A price cited, not spent (section 2, 4) | | makes the re-mark uncontended | | about 4 kB a read with its script, any number at once | holds at the engine (R1, section 6) |
| One bond behind many accounts | | | lets a clearing party face many users | the clearing party's capital | not built; see 4 |

Read together: the first three give a user what U1 to U3 ask, if the user's account is with a clearing party. What
they cost is on two sides. A price read is too dear and too contended to re-mark often: the citation is what would
change that. And the clearing party posts margin in every account: the bond is what would change that.

## 4. One bond behind many accounts: the questions

Kept as a means. It is needed only for U4 on the clearing party's side, and its size depends on the re-mark: if
accounts are re-marked to signed prices often, what a clearing party can owe an account beyond its margin there is
the move since the last mark, and the bond covers that gap, not the whole position.

| | Question | First thought |
|---|---|---|
| B1 | What bounds a clearing party's accounts against its bond, with no object every account touches? | Unknown. A citation shows that an account was opened, never that it is still open. The hard one. |
| B2 | What makes a default, and how is it shown? | An account whose clearing side cannot pay at a cited price; the default is a transaction of the bond, which the other accounts then cite. |
| B3 | Who is paid at a default? | First come is unfair; a period in which accounts register, then shares. The bond is touched only then. |
| B4 | How is the bond kept from leaving while accounts stand? | Accounts carry an end before the bond's release, and are renewed (the lease of the first family). |
| B5 | What does the clearing party post in an account? | The move a re-mark period can bring, not the position's margin. |

If B1 has no answer: the bond in a fixed number of tickets, one an account, each a bounded share. That is capital
split again, by a factor the number of tickets sets.

## 5. Order of the research

1. **The citation, at the engine.** A reader script that holds "transaction T, with an output under covenant id C
   and state S, was accepted by chain block B" against `OpChainblockSeqCommit`, with a mock accessor as the engine's
   own tests use. Measured: bytes of the proof, stack. Then the price note: the committee's attest writes an output
   under its id that says a market, a time and a price.
2. **The re-mark** by a cited price: the entry moved, the gain into margin, the excess taken out. In the model first,
   with the review's attacks (F41's shape must not pass); then scripts.
3. **The account of many legs** as small scripts co-spent, each valuing a leg, and one that adds them. Measured: bytes
   a leg.
4. **B1 to B5** in the model, with what 1 to 3 gave.
5. Only then tools; testnet where a question needs it. The first such question is already known: whether
   `OpChainblockSeqCommit` is active on testnet-10.

## 6. R1: a script cites a transaction the chain accepted (done at the engine, 25 November 2026)

`contracts/probes/R1Cite.sil`, run by `crates/kpf-tests/tests/cite_probe_tests.rs`. The test builds a chain block's
commitment with the node's own crates (`kaspa-seq-commit`, `kaspa-smt`) and gives it to the engine through the
accessor, as the engine's own tests do. The script is shown a version-1 transaction's rest, reads in it that the
first output is under a covenant id it holds and has a script it is told, and recomputes the commitment from the
transaction's id up. It spends nothing of what it cites.

| The block accepted in the lane | Active lanes | Proof (bytes) | Signature script with the script (bytes) | Stack at most |
|---|---|---|---|---|
| 1 transaction | 1 | 682 | 3,803 | 38 |
| 5 | 2 | 906 | 4,029 | 40 |
| 100 | 3 | 1,034 | 4,159 | 40 |
| 1,000 | 5 | 1,130 | 4,258 | 40 |
| 10,000 | 9 | 1,290 | 4,419 | 40 |

- The script is 3,120 bytes (paths of at most 14 and 12 levels, written out a level; fewer levels are fewer bytes).
  The proof is the rest of the cited transaction (about 330 bytes for three inputs and two outputs), 32 bytes a
  level, and nine hashes.
- Refused, each run: an output under another covenant id; another script than the one told; a number of inputs
  that is not the transaction's (the first output is found by it, so a note written as data inside another output
  is not read as one); another payload; another place in the lane; any sibling, reference or digest changed;
  another blue score; another block; the same proof against another commitment.
- Corrected on the way: the commitment's hashers are BLAKE3 keyed with their names padded to 32 bytes
  (`blake3WithKey`), not BLAKE2b; the sparse tree's `insert` is behind the crate's `test-utils`.
- Against today's price read (an epoch table spent by its attest, 8 to 11 kB, one use a block a table): a citation
  is about 4.2 kB as its own input and is used by any number of transactions at once.
- Not answered here: whether the opcode is active on testnet-10, and what the node charges for it.

So the first means of section 3 that was only supposed holds at the engine: **a price can be written once and read by
everyone without being spent.** What writes it (a note under an id only a checked attest can write under) and what
reads it (the re-mark) are R2.

## 7. R2: the re-mark (the model done, 25 November 2026; scripts next)

`thin::marked`, held by `crates/kpf-protocol/tests/thin_mark.rs`. A position's entries are moved to a price; what a
party gained since the old entries is its margin; each party may take out what it has above its floor.

- **Nobody's worth changes, exactly.** A leg's gain is a difference of two floors of the same form, so a party's
  margin and gain after the mark are, at every price, what they were before, to the sompi: checked at 600 prices on
  a fixed position and at 40 prices each on 292 random positions of one and two legs. A mark on a mark is the mark
  at the last price.
- **The floor** of a party is the maintenance at the price, or what its counterparty asked of its margin when the
  position was made, whichever is more. What is above it is the usable gain: taken out, or put behind another
  position. A sompi more is refused.
- **A side below its maintenance at the price is not re-marked:** it is liquidated or adds.
- **What the price must be, and how fresh.** A mark without a take changes no worth whatever the price: it needs no
  protection, and anyone may send it. A take is where a false or stale price pays: a party marks at the best price
  of the past and takes out what the present no longer gives it. A citation alone bounds the price's age by the
  finality depth only (twelve hours), and a script cannot show that the present is not later than a time. So a take
  is two steps, as a claim is: the mark that states it, and its collection, which reads the DAA score the mark
  landed at and holds it to the note's time within the market's `lateDaa` (the rule of `in_time`). A pure mark
  stays one transaction.
- **F41 again.** A re-strike at a cited price is not the two orders' to choose. The replacement and the novation
  keep theirs (a price both sides of the trade agreed is what they are), but a position that is re-marked to cited
  prices pays, at a settlement at the entries, what each side was worth at the last of them.

To build, as small scripts (the owner's preference), each a copy in the library, relying on one another by being
spent together:

| Script | Does | Relies on |
|---|---|---|
| a note's writer | writes, under the library's id, a note that says a market, a time and a price | the committee's epoch table spent by its attest in the same transaction (as `priced` reads it today) |
| the citation (R1) | holds that a note was accepted by a chain block | the engine's commitment |
| the mark | moves a position's entries to the note's price | the citation spent beside it |
| the take and its collection | state a take with the mark; pay it once the mark is seen to have landed in time | the mark |

## 8. B1 thought through: a reserve in shards (a design, not built)

Reasoned from what must hold, with what R1 and R2 gave. To be modelled next (R4).

**What cannot be had.** A hard bound on what a clearing party has open against its bond needs a count, and a count is
an object that every opening and closing changes. A citation shows that an account was opened, never that it is
still open, so no proof replaces the count. "Touched only at its default" and "bounded" do not both hold.

**What can.** The count need not be one object, nor everybody's:

- A clearing party's bond is cut into **shards**. A shard is an object of that clearing party alone: it holds KAS,
  and says how much of it is reserved.
- An account between a user and the clearing party names a shard and a **covered amount**: what the shard owes the
  user if the clearing party fails it. Opening the account reserves that amount in the shard (the transaction spends
  the shard and writes it again with more reserved); closing releases it. A shard never reserves more than it holds.
- So what is touched is one shard of one clearing party, by that party's own users: as many accounts open at once as
  it has shards, and no object of the network is touched at all. Shards are refilled, split and joined by their
  owner, between the owner's own objects.

**Why the covered amount is small.** With re-marks (R2) the clearing party's side of an account is settled to the
committee's price as often as either side cites one: what it lost is paid into the account, what it gained it takes
out. What it can owe beyond its margin there is the move since the last mark. The covered amount is a bound on that
gap, a few percent of the notional, where its margin today is the whole of what the position asks.

| | Two-party today | With re-marks and a reserve |
|---|---|---|
| The user posts | margin for the position | margin for the net of its legs (one account) |
| The clearing party posts in the account | as much again | what a re-mark period can move, or nothing |
| Behind the account | nothing | the covered amount, reserved in a shard |
| Its capital for N accounts of notional X, margin rate m, gap c | N m X | N c X and what is in transit between marks |

At m of ten percent and c of two, five times less; the factor is m over c, and c is set by how often accounts are
marked, which is what the citation made cheap.

**Default.** The clearing party's side of an account stands below its maintenance at a cited price and is not made
good within a time: the user takes the covered amount from the shard (one transaction of the account and the
shard, nobody else's). What the user is owed beyond it is its own loss: the tail a clearing house's members share is
here the user's, bounded by how fast its own watchtower marks. No registration, no shares, no order of arrival: every
account's amount was reserved when it was opened.

**What this leaves of the questions of section 4.** B1: answered by the reserve (a count, but a private and sharded
one). B2: a cited price at which the clearing side lacks its maintenance, and a time to make it good. B3: nothing to
share; each account takes what was reserved for it. B4: a shard's free part leaves at will, its reserved part when
the accounts that reserved it close. B5: the gap of a mark period.

**What it costs the user's view (U1 to U3).** Nothing: the user has one account with one margin, enters and leaves
against the clearing party at once, and takes its gains at a mark. What it must do is watch its account be marked,
which a watchtower does (public executors are taken to be there).

**Open in it.** How the covered amount follows a position that grows; what a clearing party's own counterparty is
when it lays off a user's position (another clearing party, by the same account, or the user's old counterparty by
a replacement); whether a shard can be one of the library's objects (it must be spent beside the account: a third
place in `who`); what an attacker does with a clearing party of its own (reserve, open against itself, default on
purpose: it takes only its own).

### 8a. Corrected the same day: a reserve reserved account by account saves nothing

Section 8 counts the capital wrongly. A covered amount reserved in a shard for one account is capital exactly as a
margin of that amount in the account is: the factor "m over c" comes from the re-marks alone (a small gap to cover
instead of a position's margin), and a clearing party that posts c in each account, asked by the user as today's
`min-counter-margin`, has it without any shard. What a shard reserved account by account adds is nothing.

Where a clearing house's capital really comes from, and what the reserve must therefore do:

1. **Netting across the clearing party's own accounts.** It is long with some users and short with others. A move
   costs it in the ones and pays it in the others; what it needs is cover for the net, not for the sum. Two-party
   margins, and a reserve by account, both cover the sum.
2. So the shard must **carry the clearing party's net position**, by market: every opening, closing and replacement
   of one of its accounts changes the net (the touch of section 8 is there anyway), and the shard holds at least a
   rate on the net, and a much smaller one on the sum.
3. And the clearing party's gains must be **reachable where its losses are claimed**: what its side takes out of an
   account at a mark goes to its shard, not to its key, and so needs no signature: anyone marks an account in which
   the clearing party gained and moves the gain to the shard, and a user whose account it failed is paid from there.
   The clearing party takes out of the shard, not out of accounts, down to the shard's requirement.

What is paid to a user is unchanged and simple: at a settlement, the part of its worth that the account's pot cannot
pay, from the shard. What is no longer simple: several users claim at once from a shard that holds a rate on the
net, and the first are paid first; the gains that would pay the rest must be moved in by marks; a gap beyond the
rate leaves the last unpaid. That is the tail, and it is what the model must measure: for a book of accounts and a
move, who is paid what, in which order, and what rate on the net makes the unpaid part nothing for moves up to a
size.

Capital for a clearing party with G gross and N net notional, cover z: two-party z G; this, z N and a small part of
G. For a book that is flat, G over N is the whole gain, and it is as large as the book is balanced.

This is R4's subject. It is the first thing here that is a design of a mechanism, not a use of what exists, and it
decides whether "one bond behind many accounts" is worth its scripts.

## 9. R4, first model: a reserve on the net (numbers only, 25 November 2026)

`crates/kpf-protocol/tests/research_reserve.rs`: a book of 200 accounts of one clearing party in one market, users
posting 20 % of their notional, the clearing party posting 0.5 % in each account and reserving 5 % of its net
position; the price moves, the clearing party does nothing, and every account is settled by whoever is owed. No
object and no script: the arithmetic of who is paid.

| Users long | Gross (KAS) | Net (KAS) | Two-party cover of 5 % (KAS) | This form (KAS) | Ratio |
|---|---|---|---|---|---|
| 50 % | 51,670 | 5,130 | 2,583 | 514 | 5.0x |
| 55 % | 55,170 | 10,550 | 2,758 | 803 | 3.4x |
| 60 % | 46,870 | 17,290 | 2,343 | 1,098 | 2.1x |
| 70 % | 48,670 | 21,290 | 2,433 | 1,307 | 1.9x |
| 85 % | 52,520 | 31,200 | 2,626 | 1,822 | 1.4x |
| 100 % | 52,480 | 52,480 | 2,624 | 2,886 | 0.9x |

1. **Every user is paid in full for moves up to the rate, whatever the book**, when the accounts in which the
   clearing party gained are settled before the ones in which it lost (moves of 1, 3 and 5 % either way, six books).
   Beyond the rate, what is unpaid is what the algebra says: the move on the net, less what the clearing party posted
   across the book and the reserve.
2. **The capital is the net's, not the sum's**: five times less for a balanced book, nothing gained for a book all
   one way. A clearing party that lays off what it takes (by a replacement, to another clearing party or to a user)
   is the one that gains; one that keeps a direction pays as two parties do.
3. **The order of settling decides who is paid.** The same book and a move of 4 %: gains first, nobody lacks; claims
   first, 101 users lack 781 KAS in all, and 1,047 KAS end up in the reserve after them. So the form must let anyone
   settle an account in which the clearing party gained, into its reserve, before claiming its own: the unsigned
   mark with the clearing side's take going to the shard (section 8a, 3) is not an option of the design, it is what
   makes it pay.
4. **A user that cannot pay is the clearing party's loss, and through it the other users'.** Users posting 2 % and a
   move of 5 %: the losing users owe 639 KAS beyond their margins, and the winning users lack 395 KAS. With users
   posting 20 % the same move leaves nobody short. A reserve on the net assumes users are liquidated in time; that
   is the clearing party's own business to see to (it is the counterparty that liquidates), and a rate on the gross
   is what covers its being late.

What the numbers do not say yet: the reserve's requirement when prices and positions change after it was set (the
net in units is the shard's to carry, but its value moves); several markets; a clearing party that is itself a user
of another; and whether the shard, the mark that pays into it and the claim fit the library's small scripts.

### 9a. A net dressed with an account of the clearing party's own (modelled the same day)

The attack the design had to meet: a clearing party short 1,000 units against real users opens 1,000 units the other
way with a key of its own, struck at 0.01 with a twentieth of that notional. In units its net is nothing.

| | Result |
|---|---|
| The shard's requirement, dressed | 100 KAS, where the real book asks 550 |
| Nobody marks; the price rises 5 % | the users lack 349 KAS; the account of its own gives nothing (its user side held 0.5 KAS against a loss of 10,490) |
| Anyone marks at the cited price before a move | the account of its own is far below its maintenance there: liquidated, out of the net; the shard is under its requirement and is called; closed at that price every user is paid by its own account |
| An account of its own struck at the market with the maintenance posted | it is collateral, not dress: the users are owed 450 KAS and it gives 550 |

So: an account lowers the net only as far as its user side can pay, and a user side that cannot is found at the first
mark. What is left is the time between the dressing and that mark. It is closed by asking of a **cover** what a mark
asks: an account is put under a shard beside a citation, and only if both sides have their maintenance at the cited
price. And the user side's maintenance rate must be at least the reserve's rate on the net: then every account that
counts against the net can pay a move of that rate.

## 10. R3: many legs valued by one small script (done at the engine, 25 November 2026)

`contracts/probes/R3Value.sil` (and the same written for 2, 4 and 8 legs at most), run by
`crates/kpf-tests/tests/value_probe_tests.rs`. The script is shown the legs of an account (33 bytes a leg: size,
scale, entry, price, direction) and holds what it is told of them: A's gain over all legs, and the maintenance on
their worth at the prices. A script of an operation would read those two numbers and the legs' hash in this input's
signature script, hold the hash to the account it moves, and do no arithmetic of its own.

| Written for at most | Script (bytes) | Signature script with that many legs (bytes) | Stack at most |
|---|---|---|---|
| 2 legs | 724 | 846 | 23 |
| 4 legs | 1,278 | 1,467 | 23 |
| 8 legs | 2,386 | 2,709 | 23 |
| 16 legs | 4,602 | 5,189 | 23 |

- **The stack does not grow with the legs: 23 items for one leg and for sixteen.** A loop is written out by the
  compiler but its variables are reused. The scripts that value two legs in line stand at the engine's 244 because
  they name every value of both legs and call helpers whose arguments stay; the limit is of that way of writing, not
  of the number of legs.
- **A leg costs about 280 bytes of script and 33 of data**, and the script is carried whole whatever the account
  holds: so one script a size (2, 4, 8, 16), and a transaction carries the one that fits. That is the owner's
  preference (small scripts, more of them) and here it is also the cheaper.
- Against today: `KpfTProve` and `KpfTLiqProve` each carry the valuation of two legs inside 4.4 kB and more. With the
  valuation in a script of 724 bytes spent beside them, each operation's script is what is left: reading a result
  and paying.
- Refused: a sompi more of gain, a sompi less of maintenance, a price changed under the legs' hash.

What R1 and R3 together say of the form to build: three small scripts that every operation needing a price shares
(the note's citation, the valuation by size, and the operation's own), spent together, each with a short stack.

## 11. The core's own direction for scripts that rely on one another (read 25 November 2026)

A thread opened on 24 November 2026 by the author of the covenant KIPs, "Inter-covenant communication with dynamic
dispatch" (https://kas-smiths.org/t/inter-covenant-communication-with-dynamic-dispatch/171; one post, no reply when
read; a proposal, with a syntax of the Argent language that does not exist yet). Paraphrased:

- A caller wants an answer about this transaction (a fee, whether a withdrawal is allowed) from another covenant
  that answers by its own state and rules. There is no call at run time: **the result is proposed, then verified.**
  Whoever composes the transaction writes the method, the arguments and the proposed result; the caller checks that
  the record is of the service, method and arguments it meant, and uses the result; the service checks that the
  result is right.
- The records are **statements in the transaction's payload**, a section an input, read with `OpTxPayloadSubstr` and
  `OpTxPayloadLen`, under a versioned header with a directory by input, for which a convention (a KCC) is proposed.
  A service input verifies every statement of its section; an entry that serves nothing requires its section empty,
  so that a spend cannot claim to have served. Several callers may rely on one statement.
- The caller must identify the service: by a known covenant id's leading input, or by an input it already resolved.
- Two of the post's own examples are this research's: an oracle input that verifies a signed price once and serves
  it to several inputs; and an input that verifies an opening of a KIP-21 commitment and serves what it
  authenticated to the others.

What it means here:

1. **It is the form R1 and R3 arrived at**, and the form the thin family has had since its first script: an object
   holds a state's hash and an id, and what moves it is another input of the same transaction. `priced` relies on
   the epoch table's attest this way; the probes rely on a citation and on a valuation this way. The research is
   going where the core is.
2. **Where the record is differs.** Here a caller reads the service's signature script at offsets that are the
   service's entry's own layout. There, the record is in the payload under a common header. The payload form has
   what this one lacks: one statement for many callers, a layout that is not one script's ABI, and the rule that a
   service spend with no statement served says so. The last is a hole to look for here: every script of the library
   has a second entry (`release`), and a caller that reads numbers in a co-spent script's signature script must be
   sure of the entry that ran. `priced` holds the attest's shape; the probes of R1 and R3 have one entry each and
   are not yet held to that by any caller.
3. **Batches.** One citation of a price served to many accounts in one transaction: a re-mark of N accounts carries
   the citation (about 4 kB) once. That is the cost side of marking often (U4), and it needs the statement form.
4. **Not to be frozen yet.** The header is a proposal without a number; the thin family's own payload uses (a
   library's genesis, an opening's fixed part, notes) would become sections of it. What is built here keeps the
   reading of a record in one place a script, so that the place can move from the signature script to the payload.

To do (R5): the citation as a service that verifies statements in the payload, and a caller that reads one; N
callers on one statement, measured.

## 12. The first question only testnet answers: answered (25 November 2026, testnet-10)

Step 1 of the architecture's order. Three things were run against the node the repository uses.

| | What | Result |
|---|---|---|
| 1 | A script that passes only when shown a chain block's commitment (`OpChainblockSeqCommit OpEqual`), funded with 1 KAS and spent with the value the block's header carries (`probe-seq-commit`) | accepted and mined: funding `2639de03b75b4abb2c67dcf13f9a173096090c1351758c8ed29945b40e2b4d66`, spend `20362dac501ae71a82ce252fc4d79ee3ae12e3738941557851ae654911f63aa8` (269 bytes, 70 script units). **The opcode is live on testnet-10 and gives the header's `acceptedIdMerkleRoot`.** |
| 2 | The node's call for a lane's proof (`getSeqCommitLaneProof`, over the JSON wRPC) | served: the lane's tip and blue score, the proof's bytes, the payload-and-context digest, the parent commitment, the inactivity shortcut |
| 3 | A payment sent and, from what the node says of the block that accepted it, the block's commitment recomputed from the transaction up, as the citing script does (`probe-cite`, `kpf_protocol::cite`) | **agrees with the header, and the lane's tip with the node's.** The transaction was the third of eleven accepted (ten in the native lane); the proof is 4 steps in the lane's activity and 4 in the lanes' tree, 544 bytes beside 272 of the transaction's rest; the node was asked for 68 kB of accepted ids and 94 kB of two merged blocks |

Learned on the way, and now in `cite.rs`:

- A block's context carries **the timestamp of its selected parent**, with its own DAA score and blue score.
- The place of a transaction (`merge_idx`) counts everything the block accepted, the coinbase among it (which is of
  the coinbase's lane); a lane's leaves are its own transactions in that order with those places.
- The lane's reference in its tip is its tip at the block's selected parent, which the same call gives for that
  block.

What is still not run: the citing script itself spent on testnet (the probe of the engine reads an output under a
covenant id; the payment cited here has none), and what a node charges it. Both come with the citation as a script
of the library.

## 13. The architecture's order, run through (25 November 2026)

The six steps of `ARCHITECTURE-CLEARING.md` 9, in one pass. The form changed on the way, at the owner's questions:
a citation is no longer beside every operation.

**Why.** Two rules set before this research do not fit a citation on every use: the committee signs when it is
asked (a dispute), and a transaction of a kilobyte or two is the measure. A citation is 3.4 kB of script and 0.5
to 1.3 kB of proof each time, and a price on the chain for every mark. So an operation is **stated** (nothing
computed, a bond, a dispute period), as a close is today, and a citation is what answers a dispute, or what
whoever is in a hurry sends with the answer of its own statement. One note then answers every dispute of its
market and time, where an epoch table answered one a block.

| Step | What was built | Known |
|---|---|---|
| 1 | `KpfNote` (122 B), `KpfTNote` (726 B), `KpfTCite` (3,391 B) in the library; builders; `cite.rs` makes the witness from what a node says | E, 5 tests; the opcode and the proof's form on testnet-10 (section 12) |
| 2 | A stated change of a position: `KpfTChange` (938 B), `KpfTChangeSettle` (1,080 B), `KpfTChangeDispute` (1,283 B), `KpfTChangeLate` (680 B); the answer of a take by cited prices, `KpfTTake` (3,898 B) | E, 7 tests, 31 random takes |
| 3 | An account of any number of legs, a take on it stated with a trace and answered a point at a time: `KpfTPoint` (1,364 B), `KpfTStepRow` (1,582 B), `KpfTStepPrice` (1,405 B), `KpfTStepEnd` (1,707 B), `KpfTKind` (813 B) | E, 10 tests, 40 random accounts of 1 to 16 legs |
| 4 | The reserve as objects: a shard, accounts that name it, notes between them | M, 7 tests; no script |
| 5 | A step of a liquidation of an account: one leg leaves for a fee, `KpfTStepLiq` (1,601 B) | E |
| 6 | Commands (`note-write`, `note-cite`, `note-reclaim`, `pos-take`, `take-answer`, `change-settle`, `change-dispute`, `change-lapse`, `change-late`), `exec-run` watching statements, and live runs 17 and 18 | T for a position (`LIVE-4.md`, parts 15 and 16) |

The library is 41 scripts. Its genesis writes 42 outputs: with one copy each it needs 3.4 KAS a copy or more for
its storage mass, and with two copies 7 KAS.

### 13.1 What the sizes are

| Transaction | Bytes |
|---|---|
| A take stated (a position; an account of 2, 5, 16 legs, which shows its terms, its root and its trace) | 2,138; 2,430, 2,695, 3,663 |
| Applied after the dispute period | 2,186 |
| Disputed; a dispute that lapsed | 2,267; 2,371 |
| A statement out of its time, ended | 1,716 |
| A note written (three members) | 5,769 |
| A citation alone | 5,005 (21,766 script units) |
| A take on a position answered whole, one leg; two legs | 10,078; 14.7 kB (measured before the last change: 14,712) |
| A point of an account answered: a row, a price, the end (16 legs) | 4,169; 8,860; 4,407 |

### 13.2 What was found on the way

- **A statement shows what it states.** Found during live run 17, which ran scripts that were given a statement
  by its hashes: nobody who watched could have checked one it was not sent. The script now takes what is stated
  and what the position becomes whole and hashes them, and `exec-run` reads statements, disputes one that is within
  the rule at no price near its own, applies one nobody disputed, and ends its own dispute that nobody answered
  (live run 18).
- **A statement nobody could answer locks an object for nothing**, because a lapse returns both bonds. Six were
  found and closed (the table of `ARCHITECTURE-CLEARING.md` 4b). The rule that came of it: every statement that
  can be made has a script that answers it right or wrong, or it cannot be made.
- **A price stated must be near the committee's even where nobody's worth depends on it.** A take moves the
  entries, and an exit settles at the entries: entries moved from the truth would pay whoever moved them, were the
  committee to stop.
- **The reserve** (section 6 of the architecture): a covered account pays the clearing party nothing directly; a
  draw is a statement; a book is covered along a path.
- **A fraud proof of one step replaces the valuation by size class** (R3). R3 computed over every leg in a script
  of 0.7 to 4.6 kB by class; a point's answer is 1.4 to 1.7 kB for any number of legs, and is sent only when
  somebody disputes.

### 13.3 What is not done

- **An executor disputes a false take but does not answer one.** `exec-run` writes no note and sends no citation:
  a disputed take is answered by whoever runs `note-write` and `take-answer`, and the members sign for it only when
  they run `--do answer` (which now asks them for a disputed take's time; not run live). An executor does not watch
  an account's statements at all.
- Live run 18 did not reach the refutation of a false take with the scripts as they now are (its script did not
  roll the committee into the next epoch); run 17 did, with the statement script of before.
- For an account: opening it on the chain (the scripts take it, no builder or command makes one), a close, an
  exit's refutation, the match and the replacement into one, and an answer at once.
- The reserve is scripts (the shard, covered accounts, the notes between them and the stated draw: nine scripts,
  `ARCHITECTURE-CLEARING.md` 6a), at the engine only; reviewed once (round 32, no new finding, with what it did
  not reach listed there). The commands exist and ran offline as one story (`ARCHITECTURE-CLEARING.md` 6b); no
  executor watches an account or a shard, and nothing of it ran at a node.
- Of the rest of the goal (`ARCHITECTURE-CLEARING.md` 10): the handing over of a side, the move of a leg between
  two accounts under one shard, and options as legs are scripts, at the engine only and not reviewed. Spot as
  margin is numbers only; the clearing side passing to another clearing party is a design.
- What the default costs in capital: a take, a liquidation and a call wait the dispute period, so the cover is the
  move over that period. Not computed.
- Today's close and liquidation still read the epoch table; they are not rewritten on notes.
- The new scripts were reviewed once (round 30: five findings, three high, all fixed in the scripts; the review's
  verification of the fixes is round 31). The scripts of live runs 17 and 18 are those of before the fixes.

Parked until then: the rest of live run 16, signed prices carried by payload (the citation may replace it), a match
of many orders at once, the executor's `take` beyond what is built.
