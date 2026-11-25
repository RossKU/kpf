# Live run 4: the thin family on testnet-10

21 November 2026. The first run of the thin family (`docs/DESIGN-COMMITTEE.md` 8a; `contracts/*.sil`, commands
`lib-create` and `acct-*`): a library, an account whose state is published in its opening, and two claims, one false
and one true. Three keys: the account's owner (a), its counterparty (b, the first executor), and a second executor
that watches (the funder key). Eight transactions, all accepted. The market is `run/live3g/m1.json` of live run 3 (the
account takes its terms from it; no price was attested in this run, since nothing was proved). Scripts: `run/live4/`.

| # | Step | Command | Bytes | Fee (KAS) | Compute / storage mass | Transaction |
|---|---|---|---|---|---|---|
| 1 | A library: one copy of each of the seven scripts, 10 KAS each | `lib-create` | 995 | 0.005945 | 5,945 / 27,911 | `e36fef7d230d5f5aabb01b90cfa68bb90a9c441ab5830397050ec45c3d3bf2ad` |
| 2 | An account: a long 10 at 10, margins 20 and 30 KAS each paid by its party, window 300 DAA, bond 10 %, state in the payload | `acct-open` | 3,121 | 0.024211 | 24,211 / 0 | `a99a3f2c8eabfe97c3d6a4b17fa48daad72ac814d833ded4a45d4062c861b195` |
| 3 | a adds 2 KAS | `acct-add-margin` | 1,868 | 0.007968 | 7,968 / 1,600 | `9dc10b2e97c5520cfefc8254a3bc7e0d8c560ad98d9b16ede99465e1128eb6fa` |
| 4 | a states 30 KAS where the rule gives 21.5, with a bond of 5.2 | `acct-declare --amount 30` | 2,065 | 0.008165 | 8,165 / 1,579 | `3cb894c18e92cae90e4503269fcc65ffd026e7858235e0cf477a5886d380df84` |
| 5 | The second executor reads the claim: "CHECK: FALSE. The rule gives 21.5 KAS" | `acct-show` | | | | |
| 6 | It disputes, with a bond of 5.2 | `acct-dispute` | 2,197 | 0.004394 | 4,297 / 3,842 | `c352d3e1c91ff4e15d25e848fab57a4e4b1570eaba4853cede0da8ad56d6f217` |
| 7 | Nobody proves it; after 300 DAA the account is whole again, the disputer has 7.8 KAS, b 2.6 | `acct-lapse` | 2,301 | 0.005121 | 5,121 / 9,098 | `ef6b037f806e82b317f727bcea0e09073cd13dc753946adc12266589b69b90e9` |
| 8 | a states what the rule gives (21.5) | `acct-declare` | 1,705 | 0.004805 | 4,805 / 3,494 | `096550bdee40472fe895244db2bd15da3cb32761c237d36604cc1fecd1dd2dfe` |
| 9 | After 300 DAA the second executor settles it: a 26.7 KAS (21.5 and its bond), b 30.7 | `acct-settle` | 1,762 | 0.005572 | 5,572 / 13,542 | `a89fb42c89e8ba86aea5ad830725b67481292996bf553a13c807f4b828828aa3` |

What the run showed:

- A native transaction with a payload of 436 bytes is relayed and accepted: an opening can publish its account.
- The same copy of a script served two transactions in a row (the declaration's in 4 and 8, the dispute's in 6 and 7):
  the file of its output 1 is the next one's `--script`.
- A relative age asked by a script's signature script is enforced by the node: 7 and 9 were refused before the
  window and accepted after it (the tool waits and says from which DAA).
- The false claim cost its claimer its bond (5.2 KAS: 2.6 to the second executor above its own, 2.6 to the
  counterparty) and nothing else; the account went on and was settled by the next, true claim.
- Sizes above those of the tests are funding: the opening spent ten small UTXOs of b, and 3 and 4 several of a.
- Where the storage mass was above the compute mass (1, 7, 9) the fee the tool paid was the compute mass's, and the
  node accepted it.

Not in this run: the proof of a disputed claim (not built), a late claim and the close by both (built, run offline
only), two machines.

## Second part: the liquidation

The same day, after the liquidation was built (`KpfTLiq`, `KpfTLiqSettle`, `KpfTCure`, `KpfTLiqDispute`). A second
library (the first has no script of the liquidation), an account with a thin margin (a long 10 at 10 with 6 KAS, b 20
KAS, window 300 DAA, bond 10 % = 2.6 KAS), and three liquidations of a by the second executor: one the rule does not
give, one cured, one settled. Nine transactions, all accepted. Script: `run/live4/t3.sh`.

| # | Step | Command | Bytes | Fee (KAS) | Compute / storage mass | Transaction |
|---|---|---|---|---|---|---|
| 1 | A library of the eleven scripts, 6 KAS each | `lib-create --value 6` | 1,343 | 0.007773 | 7,773 / 73,214 | `d067054c4564ada36ca4488425df6260b695dcd15b806685557229d036d0bd6f` |
| 2 | The account, its state published | `acct-open` | 1,321 | 0.007411 | 7,411 / 869 | `1cdefbac369252dc0bcbd95c1904b8503394e925481ace271e05cd749832a693` |
| 3 | A liquidation of a stated at the entry price (fee 1, cure 2), with the bond | `acct-liquidate --fee 1 --add 2` | 2,304 | 0.004608 | 4,504 / 6,850 | `1a3d5f6c56e2a107ab5c4f3e58299bca374c73e05d95efe428a9a4abef810666` |
| 4 | The counterparty reads it ("CHECK: FALSE. The rule does not liquidate this side") and disputes it | `acct-show`, `acct-dispute` | 2,346 | 0.004692 | 4,546 / 6,581 | `55360d2c8f859e62729959b771d018c0df7675e784d8120737626e70fac36281` |
| 5 | Nobody proves it: after 300 DAA the account again; the disputer 3.9 KAS, a 1.3 | `acct-lapse` | 2,450 | 0.005370 | 5,370 / 17,090 | `fed4329e51c8c0b8155f93bac03963c14171c7f323eb839d5f49a2738cb4bf8e` |
| 6 | The rule's liquidation at 9.7: fee 0.485, a cure adds 2.31075 | `acct-liquidate` | 2,304 | 0.004608 | 4,504 / 6,836 | `65e0dd718cdc32e098f9b7e59f975938fc0f8886b21fce609464122bfb3cfe1b` |
| 7 | a cures it: margin 8.31075, the liquidator's bond back | `acct-cure` | 2,280 | 0.007560 | 7,560 / 30,032 | `b419f490f30bfdcba1b0940cc605d274f8d23ed90a070bbf81f88c50a2af32bd` |
| 8 | The rule's liquidation at 9.5: fee 0.475, a cure adds 1.8905 | `acct-liquidate` | 2,424 | 0.005624 | 5,624 / 6,180 | `f7f1da4ebf58f7751536abe712bf28bcdee8a268b77d44352d1ed5ab2e7b0b71` |
| 9 | Not cured: after 300 DAA the liquidator 3.306075 KAS (its fee and bond), b 28.03575 | `acct-settle` | 1,720 | 0.004170 | 4,170 / 8,843 | `eb0f750915256a75dd591acd56567333bfb9db8811f443daa2648ee575d5e7d9` |

What it showed: a false liquidation cost its liquidator its bond and left the account as it was; the side named
cured a true one with its own key and kept its position; an uncured one was settled after the window, the side
forfeiting to the other party and the liquidator paid its fee. No price was attested (nothing was proved; the proof
is not built). The second library also stays on testnet (66 KAS):
`9cda92f9e09d125ae7776339e42a25de542ebc5aeda77df56c1f662512ec0604`, its scripts' files under `run/live4b/out/`.

## Third part: the proofs and the exit from a stopped committee

The same day, after review T1b and its fixes (`docs/DESIGN-COMMITTEE.md` 8a): the scripts as they are now. The
roster of live run 3 (`run/live3g`: three seats, 36 KAS) was rolled into its current epoch and its seats signed the
ticks; a third library holds the fifteen scripts. Sixteen transactions, all accepted. Script: `run/live4/t4.sh`.
Every account: a long 10 at 10 in market 1, window 300 DAA, bond 10 %, quiet time 600 DAA.

| # | Step | Command | Bytes | Fee (KAS) | Transaction |
|---|---|---|---|---|---|
| 1 | The roster into the epoch [629,178,372, 629,214,372): four tickets | `roster-roll` | 20,800 | 0.0416 | `ea9c846fcfc48dcf1ea76ad9b728ac43351ca56274107571e65f4242276ff59c` |
| 2 | A library of the fifteen scripts, 7 KAS each; `lib-check` reads its genesis back | `lib-create --value 7` | | | `36a6cae7e942fd417d76729be550fc6b0f0cca2916d05ad06989bd00208b6ae8` |
| 3 | An account (20 and 30 KAS), opened from the genesis file | `acct-open --lib GENESIS-tx.json` | 1,029 | 0.004759 | `e97237d1960d2034666072ec28fa77c5641dd2fa0a744ab4d640706fd6ecf40b` |
| 4 | The seats sign 11; a states what the rule gives there, the tick and price taken from the signed files | `acct-declare --ticket1 --tick1 x3` | 1,709 | 0.004809 | `c6ed5464b3a189522db3262fd7bcccd8613a54a4bc8500053b796430281a0bdf` |
| 5 | The second executor disputes it (the tool warns: the claim is true) | `acct-dispute` | 2,206 | 0.004412 | `40a6ef75fb3998bbd06f570c92d4a8f838dc6c5db774c1fd232a4461f6b221a6` |
| 6 | a proves it with the ticket and the three signatures: a 39.45 KAS (29.45 and both bonds), b 20.75 | `acct-prove` | 8,379 | 0.016758 | `b23bbe548a4afd798898ee8f1907091c0832a97f137d97348cedea4d4445a0cd` |
| 7 | A second account | `acct-open` | 909 | 0.003639 | `bfb6f8225372fd09460e88f0fdaabfab06945648d2f412eed145edab7e164ade` |
| 8 | a: "no price since DAA 629,186,833", with the bond | `acct-quiet` | 2,178 | 0.005278 | `8c838572592ccb70336ad4193051223f80d973521b96627800cbcd18439d6862` |
| 9 | The seats sign a tick 163 DAA after it; the second executor shows it: the account again, 2.5 KAS to it and 2.5 to b | `acct-refute` | 7,264 | 0.014528 | `a99f8be9e366c4e00e7188a515e9871a46ce6dea43631dbce563900fd40b5eb5` |
| 10 | a again: "no price since now"; nobody shows one | `acct-quiet` | 2,178 | 0.005278 | `ef7ec296dec84eb9161625d7b63f3e268756a0df6f3660e6007ca5ab7edebac4` |
| 11 | After the quiet time, settled at the entries: a 25 KAS (its margin and its bond), b 30.2 | `acct-settle` | 1,686 | 0.005496 | `b7591228992de04e99576cb67f7f7c013921a29ca22ab539467d391643ffc8f7` |
| 12 | A third account, a thin one (6 and 20 KAS) | `acct-open` | 909 | 0.003639 | `246113a35b9a184bbaf196421c4615f9e9390aafa5c075e9e6a5ae12d97edd0c` |
| 13 | The seats sign 9.7; the second executor liquidates a at the committee's tick | `acct-liquidate --ticket1 --tick1 x3` | 2,467 | 0.005667 | `8d9cf896087f38a18a21b6265ce096755aa695b6fea78f83f4ccf39d29b05909` |
| 14 | a disputes it (the tool warns) | `acct-dispute` | 2,303 | 0.004606 | `21ea3723c7631e48d7cc75a424fb98825f05ed98e1b59456980d4cac0dda3ad6` |
| 15 | The liquidator proves it and has a's bond of 2.6 KAS; the claim is a proved one | `acct-prove` | 8,684 | 0.017368 | `b64f0f4483d9ef95d68777844d92de79ca34c305706dd7df550436ae9d38c4cf` |
| 16 | A window later, settled: the liquidator 3.085 KAS, b 25.715 | `acct-settle` | 1,901 | 0.005711 | `2694b993a0bec7489e962f0907a856d46fc84699c579815060036a6a87969bf1` |

What it showed, that the earlier parts could not: the legacy ticket's `attest` runs inside a transaction of the
thin family and the node accepts it (the proof of a close, of a liquidation, and the refutation of a no-price claim);
a disputer of a true claim lost its bond to the claimer, twice; a proved liquidation waited its window again before
it was settled; a no-price claim was ended by a tick signed after it and, with no tick shown, settled at the entries
after the quiet time; an account was opened from a library's genesis file that the tool had read. The third library
stays on testnet (105 KAS): `ecd84329826a8f9569e8b2139975c485acad6c0d2a851570876532ce0e6e02b6`, files under
`run/live4c/out/`.

Not run live: a cure, a late end and a lapse on the scripts as fixed (their earlier forms ran in parts one and two),
the close by both, two legs, two machines.

The first library stays on testnet (70 KAS in its seven scripts) and serves any later account that needs no liquidation:
`c7cfb1f821ed95b7477897ac2255c1a743efae0c48338beae1915a4151f4b284`; the files of its scripts' current outputs are
under `run/live4/out/` (not in the repository).

## Fourth part: orders between two users, and their accounts merged

21 November 2026, after review T1b's rounds 4 and 5. A fourth library, of the eighteen scripts as fixed (orders with
the margin they ask of the other side, the merge with its least margins). Five keys: a buyer and a seller (the two
users), two executors that match and merge (two seats' keys of live run 3; neither holds a position), and the funder
(the library, the roster, the settlement). Twenty-three transactions, each accepted at its first sending. Script:
`run/live4/t5.sh`; files under `run/live5/out/`.

| # | Step | Command | Bytes | Fee (KAS) | Compute / storage mass | Transaction |
|---|---|---|---|---|---|---|
| 1 | A library: one copy of each of the eighteen scripts, 8 KAS each | `lib-create` | 2,072 | 0.012092 | 12,092 / 90,079 | `91e568e8ca132767459442073d659bb27262c82bb2095c288eeb7dbf185eb5a5` |
| 2 | The seller: a sell of 10 at 10 or more, margin 30 KAS, tip 0.5, at least 2 a match | `order-place` | 713 | 0.005443 | 5,443 / 265 | `279dc3288b8ef1abebfba163ff34a08eaa640f0250a90decd3e04d55aa599398` |
| 3 | The buyer: a buy of 3 at 10.1 or less, margin 9, tip 0.15 | `order-place` | 353 | 0.002083 | 2,083 / 4,395 | `b1be1977003c42d67a334c1efc5db05647d3c5cc11be15b39e7580f2babca8ea` |
| 4 | Executor 1 matches 3 at 10: an account of the two; 7 of the sell rest (21.35 KAS); tips 0.3 | `order-match` | 4,588 | 0.012058 | 12,058 / 76,260 | `e2987e1fa68bbf2f2273f7201bc99817ce91fd234c0c3ab40206fb9d3eb54407` |
| 5 | A buy of 2 | `order-place` | 353 | 0.002083 | 2,083 / 6,577 | `34a468ade1bc172feaed42c2acd8c01b3e2a26ac58be276c6003e9e7a2944a3e` |
| 6 | Executor 2 matches 2 at 10.05; 5 rest; tips 0.2 | `order-match` | 4,414 | 0.010524 | 10,524 / 81,131 | `49229056beb28c2c285570d91611e1b6f2f3955c45a433afb66274fd159a256f` |
| 7 | A buy of 4 | `order-place` | 353 | 0.002083 | 2,083 / 3,337 | `c22c341b60e71b413204b08d6ec8438431c3a2f320f22ba9b3e7aa16edfde19f` |
| 8 | Executor 1 matches 4 at 10; 1 rests; tips 0.4 | `order-match` | 4,586 | 0.012056 | 12,056 / 78,956 | `5f7f0cacb4b9f2598eefeb54b32d0ebd12b4a20219a13946b939d419764178d5` |
| 9 | A buy of 2 | `order-place` | 353 | 0.002083 | 2,083 / 6,602 | `78bbbf705608b95270da7182efd977a3f49fffbb26a6d6b3291c5d565608e3cc` |
| 10 | Executor 2 matches the last unit of the sell at 10.1 (below its least fill: all that is left); 1 of the buy rests | `order-match` | 4,585 | 0.012055 | 12,055 / 148,934 | `4b4c30d2fd6102524aff4c5322d9baae44ad74f8a34636778adffcadeaa551c5` |
| 11 | The seller: a second sell, of 1 | `order-place` | 473 | 0.003203 | 3,203 / 12,851 | `f88ce6caa827bf4265c19d472978719383a50bf33a5c315ac502b1c6e901e675` |
| 12 | Executor 1 matches it with what rests of the buy: a second account under that buy's covenant id | `order-match` | 4,549 | 0.012009 | 12,009 / 154,944 | `a889316dbd7d19000f67138c97dd8110d0bcfd3d329998ead0fda235eb8748c7` |
| 13 | Executor 2 merges the accounts of 3 and 2: long 5 from 10.02 | `acct-merge` | 3,016 | 0.008316 | 8,316 / 0 | `05058094071e132d8a9dd6f48f8f6bad73b6eb2583bb15baa342b9c256852dcb` |
| 14 | Executor 1 merges that with the account of 4: long 9 from 10.01111111, margins 26.98999999 and 26.99000001 | `acct-merge` | 3,016 | 0.008316 | 8,316 / 637 | `9666f9ea31f8fdc4f51f596d72fd3aebd200bc2ca2ececbfdc3bb14081865523` |
| 15 | Executor 2 merges the two accounts under one id: long 2 from 10.05, margins 5.995 each | `acct-merge` | 3,016 | 0.008316 | 8,316 / 0 | `afc1b6dc8ac68cf0e21ca74dd1e4fba305dd9bf339d07de0847f74c753688bc6` |
| 16 | The seller: a sell of 1 that ends 300 DAA later | `order-place` | 353 | 0.002083 | 2,083 / 13,335 | `2468981cad0f50c7244e9c890bc8f7146b1ebf402e2238bd88cde94b10b6b07b` |
| 17 | The buyer: a buy of 1 (the tool then refuses to match it with the ended sell) | `order-place` | 353 | 0.002083 | 2,083 / 13,362 | `2cd00b5130ea9c069c620b4e6f2dc45af0f4109b6b1f961ea9a98e1b898b7074` |
| 18 | Executor 2 returns the ended sell to its owner | `order-expire` | 1,998 | 0.004088 | 4,088 / 4,113 | `6c099905a97bf34341d7f2a7c1b31a5c254c3f381167af465027b1c5e7ddc922` |
| 19 | The buyer takes its buy back | `order-cancel` | 2,064 | 0.005154 | 5,154 / 2,668 | `b16157f451b5e96e9e8c70ac5897fb63e396c853192b8cdff25fc7de62a2a71d` |
| 20 | The roster of live run 3 into the current epoch | `roster-roll` | 20,680 | 0.04136 | 26,990 / 160,238 | `8e8f330b673c558993cf571069481d9eeca753e110d2b1817cfeae49542be6de` |
| 21 | The seats sign 10.5; the buyer declares its close of the account of 9 at the committee's tick, with a bond of 5.398 KAS | `acct-declare --ticket1 --tick1 x3` | 1,829 | 0.005929 | 5,929 / 2,270 | `31dbb35b802a9734173d972bdd5565464362e70fc81020c88d6562694ed43472` |
| 22 | A window later, nobody having disputed it, settled: the buyer 36.3155 KAS (with its bond), the seller 23.0625 | `acct-settle` | 1,599 | 0.004049 | 4,049 / 3,413 | `7a6f5128beff1b4db1ab064eead627ebd3d64a31afd25ed11e4bc11de552be9b` |
| 23 | The account of 2 closed by both: 6 and 5.99 KAS | `acct-close-both` | 1,430 | 0.00588 | 5,880 / 1,265 | `9d01afe2a578010937efa1feb4c3dda88d2479924127cbd6274a8e761a785f9d` |

What it showed: a sell of 10 filled by buys of 3, 2, 4 and 2 is four accounts between the same two users, and one
unit of the last buy rested until a second sell took it (a fifth account, under the same covenant id as the fourth);
two executors, neither a party, merged the five into two (two ids into one, twice; and two accounts of one id), each
keeping 0.01 KAS; the merged account of 9 was closed at a price the committee signed, as any account is (the buyer's
gain of 9 x (10.5 - 10.0111), less the tolerance), and the other by its two parties; the tool refused to match an
order past its expiry, and an executor returned it.

Measured on the node: a match is about 4,500 bytes and 0.012 KAS (its tips here 0.1 to 0.4 KAS); a merge 3,016 bytes
and 0.0083 KAS, so the 0.01 KAS its sender keeps leaves it 0.0017. The matches with a tip output of 0.1 KAS have a
storage mass of about 150,000 and were accepted (the fee is the compute mass's): the limit on a transaction's storage
mass is the block's 500,000, not 100,000, so a library's scripts could hold less than the 8 KAS each they hold here
(not tried).

The fourth library stays on testnet (144 KAS): `7be9f852b6c66ea296791e8f79d92fdfd94a2dfe4e26b07df12fbc4c5f8e89c5`,
genesis `91e568e8ca132767459442073d659bb27262c82bb2095c288eeb7dbf185eb5a5` (`run/live5/out/91e568e8-tx.json`).

Not run live: a partial fill disputed or liquidated, an order that does not rest (`--no-rest`), a cure, a late end
and a lapse on the scripts as fixed, two legs, two machines.

**Do not use the fourth library.** Its `KpfTMerge` is the first, unsigned form, in which a party takes the other's
margin by merging in an account it wrote alone at a false entry (review T1b, F11, found after this run; fixed by both
parties' signatures). Its other scripts read an account's state as it was before the asks were added (120 bytes), so
none of it serves the scripts as they are now.

## Fifth part: the book

21 November 2026, after review T1b's round 8. A fifth library, of the twenty scripts, and its book: every order and
account of this run is under one covenant id, `babdc07fbb31d86f3b8b56c8cb198aa242d878f1f82a9b0bb5d010e9d54207ca`,
which an order enters through an anchor with its owner's signature. Six keys: a buyer, a seller and a third user,
two executors that match, merge and take a user out (neither holds a position, and neither's key is a party's), and
the funder. Twenty-five transactions, each accepted at its first sending. Script: `run/live4/t6.sh`; files under
`run/live6/out/`.

| # | Step | Command | Bytes | Fee (KAS) | Compute / storage mass | Transaction |
|---|---|---|---|---|---|---|
| 1 | A library: one copy of each of the twenty scripts, 3 KAS each | `lib-create` | 2,366 | 0.014126 | 14,126 / 265,603 | `32327b12e77c635c34c5578dc322d7f550108aa87e837dc838363d7a6134a243` |
| 2 | Its book: three anchors under one covenant id, 1 KAS each | `book-create` | 887 | 0.006357 | 6,357 / 118,059 | `36c666b89c8f60807a8d25686a89433c53bcb4078313b7cc24e3458431802a54` |
| 3 | The seller: a sell of 5 at 10 or more, margin 15 KAS, asking 7.5 of whoever faces it; through an anchor, signed by its owner | `order-place` | 2,591 | 0.009061 | 9,061 / 51,556 | `5f07551ff7ecf716bd1418e1e77c836746af08cebdf9b412a29c2218a6ef1269` |
| 4 | The buyer: a buy of 5 at 10.1 or less, the same margin and ask | `order-place` | 2,471 | 0.007941 | 7,941 / 49,576 | `1ea22b8143d3e766e847f135929aaab530faa128982a46da33d16bf110780033` |
| 5 | Executor 1 matches 5 at 10: an account of the two, under the book's id | `order-match` | 4,571 | 0.012031 | 12,031 / 75,647 | `7a3ab030c636c8af79262661cf830ab46a5c3bc9296739185d8a2940468fadc8` |
| 6 | The seller: a sell of 3 | `order-place` | 2,231 | 0.005701 | 5,701 / 55,831 | `d80af41b889d045bf554806a767b6c049e2274ead7891d73e7dfd7157c1a3b09` |
| 7 | The buyer: a buy of 3 | `order-place` | 2,231 | 0.005701 | 5,701 / 53,710 | `f454cfcb9c0f538d55f8c1474d0c28d698945ec24ef08128c47f948dc49ecbb8` |
| 8 | Executor 2 matches 3 at 10.05 | `order-match` | 4,055 | 0.00811 | 7,435 / 61,762 | `f250fa01b727bfaf026ab3ab1fc6b1c883b07d58d9f1008c9e9459597f5fdf1b` |
| 9 | Executor 2 merges the two accounts (nobody signs): long 8 from 10.01875, margins 23.995 each | `acct-merge` | 3,099 | 0.008399 | 8,399 / 4,035 | `539599c0beb04313ec11c55edb7cd5a897f9ebfc4442c9987179299786572ddb` |
| 10 | The buyer wants out: a sell of 8 at 10.2 or more, margin 24, as any order | `order-place` | 2,232 | 0.005702 | 5,702 / 50,455 | `617c747993d7ccf24810805655c2dea177dc1bfe89cb261fb9a4d26dec59f9f5` |
| 11 | A third user: a buy of 8 at 10.3 or less, margin 24 | `order-place` | 2,592 | 0.009062 | 9,062 / 41,358 | `13e823cf5bd0b4fd73d2ba341b6e316824cd9f2a55627a6134c851d3380d2085` |
| 12 | Executor 1 matches 8 at 10.2: the third user long against the buyer | `order-match` | 4,573 | 0.012033 | 12,033 / 68,841 | `9bad5ecce4e9f4fa28b2d617ab650eedf1e29d0eb0b3a0d5b4941da7cce4e24f` |
| 13 | Executor 2 takes the buyer out of its two accounts (nobody signs, no price): it is paid 49.43 KAS, its two margins and 1.45 from the seller's; the third user is long 8 against the seller from 10.2, margins 24 and 22.545 | `acct-novate` | 3,828 | 0.007656 | 6,848 / 21,230 | `59134d4c6f6d045ef02cb50120197f889d7644511dbce40deebfda3d38408193` |
| 14 | The roster of live run 3 into the current epoch | `roster-roll` | 20,680 | 0.04136 | 26,990 / 160,570 | `79366a213730bbdb65e99b6a4892ac9a59dd9d62fb4fc595d69e290dafaaf6f9` |
| 15 | The seats sign 10.4; the third user declares its close at the committee's tick | `acct-declare --ticket1 --tick1 x3` | 1,728 | 0.004828 | 4,828 / 11,172 | `f40639d5335469e76096e4b0fc68d0aeaa68ef8d840d6fc131b41337f53d3f7c` |
| 16 | A window later, settled: the third user 29.8385 KAS (with its bond), the seller 21.361 | `acct-settle` | 1,618 | 0.004068 | 4,068 / 11,425 | `c9ca77fa680fda7eaa7ba5a2cb850ae8b0fee57b8b0c163717904679cbccbb61` |
| 17 | The buyer: a buy of 1 | `order-place` | 2,347 | 0.006817 | 6,817 / 62,030 | `d6c139b9bbd6ec5105c8acab2c1273b94ba34c6a413197eababf3325a08f5192` |
| 18 | The seller: a sell of 1 | `order-place` | 2,347 | 0.006817 | 6,817 / 64,167 | `f0213a6a1a8ecc54f52feb7c28fa95b4d670b68252d60e4dede81fd7b051ed45` |
| 19 | Executor 1 matches 1 at 10: the buyer long against the seller | `order-match` | 4,251 | 0.009551 | 9,551 / 7,738 | `e4a83696b7ebf3cdff2346fcdcc9b84314f156779ffa0bf9e1dba2589091e0c0` |
| 20 | The seller: a buy of 1 | `order-place` | 2,227 | 0.005697 | 5,697 / 64,913 | `7e682f0cdc006edd849bb0102d423b2938450f9cd36313598da786eea459a004` |
| 21 | The buyer: a sell of 1 | `order-place` | 2,227 | 0.005697 | 5,697 / 63,359 | `b2801c5ae9b61e8918f1cd854daf05c5b115a9bb6a3f97a679c2406bbee6fe4f` |
| 22 | Executor 1 matches 1 at 10.2: the seller long against the buyer | `order-match` | 4,251 | 0.009551 | 9,551 / 8,173 | `08e63b82dc1c9f19247fb569d8a65c56cbf40ba5cba8dbe1aed228be6f0b4d17` |
| 23 | Executor 2 puts the two opposite accounts together: nothing is written, the buyer is paid 6.185 KAS and the seller 5.8 | `acct-novate` | 4,067 | 0.009157 | 9,157 / 46,353 | `8ab652326057a13b1b8fe65e580d9bfc89e5fc07e796cc71d15f57b49dcc276d` |
| 24 | An account of 1 opened by the buyer and the seller through an anchor, both signing | `acct-open` | 2,941 | 0.008411 | 8,411 / 58,234 | `e1fa8e156af45ad2a7283700d533b96a85d451ee96fbe55bb4f9b91696436ab8` |
| 25 | Closed by both: 2 and 2.2 KAS | `acct-close-both` | 1,463 | 0.005913 | 5,913 / 7,143 | `54a95e3fb8226c4d28ca1157e833f1104c6f8effd99befddc741fd6701892d9a` |

What it showed, that the engine's tests could not: the node takes transactions that write several objects under one
covenant id from one or two inputs of it (an order beside its anchor; an account from two orders; one account from
two; an account and a payment from two accounts); a user that wanted out placed an order as it would to open, and
two transactions of executors later held its two margins and its gain, having signed nothing but that order; the
account it left behind, between two users that had never traded with each other, was closed at a price the
committee signed; two opposite accounts of the same two users closed each other without a price.

Measured on the node: a placement is 2,591 bytes and 0.009061 KAS (it was 353 bytes before it went
through an anchor and a script); a match about 4,571 bytes; a merge 3,099 bytes and 0.008399 KAS (the
sender keeps 0.01); a novation 3,828 bytes and 0.007656 KAS (the sender keeps 0.015). The library's
genesis has a storage mass of 265,603 and was accepted: a script holds 3 KAS here, where the earlier libraries gave
each 7 to 10 KAS on the belief that the limit was 100,000.

The fifth library stays on testnet (60 KAS, and 3 in its anchors): library
`869bd952cfa032e8801b0212316115c688b31abec1adb6b587da117b9601c6e3`, genesis `run/live6/out/32327b12-tx.json`; book
genesis `run/live6/out/36c666b8-tx.json`.

Not run live: a partial takeover (what is left of the larger account), a cure, a late end and a lapse on the scripts
as fixed, an order that does not rest, two legs, two machines, two placements racing for one anchor.

## Sixth part: cross margin

22 November 2026, after review T1b's round 13. A sixth library, of the twenty-five scripts, and its book. Six keys: a
buyer, a seller, a third user, two executors and the funder. Twenty-five transactions, each accepted at its first
sending. Script: `run/live4/t7.sh`; files under `run/live7/out/`.

| # | Step | Command | Bytes | Fee (KAS) | Compute / storage mass | Transaction |
|---|---|---|---|---|---|---|
| 1 | A library: one copy of each of the twenty-five scripts, 3 KAS each | `lib-create` | 2,681 | 0.015291 | 15,291 / 332,830 | `61c4df240b9455381bcfcdff7690b2cbfab94aafd5e91e8c50869210f3874543` |
| 2 | Its book: three anchors | `book-create` | 887 | 0.006357 | 6,357 / 118,055 | `b9e3ca0a6124415071d0d401e35d617e8ca6c2bd22c6db6858d0965019039ae7` |
| 3 | The seller: a sell of 5 of market 1 at 10, margin 15 | `order-place` | 1,744 | 0.005214 | 5,214 / 52,454 | `631d86deaf7fb4d2f5476bbad5d915843c0eb04b9ed79f272e0f6e88ccb3e4b2` |
| 4 | The buyer: a buy of 5 | `order-place` | 1,864 | 0.006334 | 6,334 / 51,269 | `bc547028b0bad773ab7ffdf666857aa6d4389bcee20e99f7f54b93fbb1b5a734` |
| 5 | Matched: the buyer long 5 of market 1 against the seller | `order-match` | 3,708 | 0.007416 | 7,008 / 8,825 | `32ebd51d1062767e6dbed5e0ca895ef6cf8620ccba72eba7717620c1feff81cd` |
| 6 | The buyer: a sell of 3 of market 2 at 20, margin 18 | `order-place` | 1,744 | 0.005214 | 5,214 / 51,536 | `918ed7ed37aa6b023a4eca4672210435b9537c4ba0f9a4f07aa1339790512fe8` |
| 7 | The seller: a buy of 3 | `order-place` | 1,744 | 0.005214 | 5,214 / 51,043 | `0bfa2b8e95ad83c424cc4070cd19fba436d56e648f6a87b9c47dc315dca5d035` |
| 8 | Matched: the seller long 3 of market 2 against the buyer | `order-match` | 3,948 | 0.009248 | 9,248 / 2,708 | `7a71bac82a0cbec70dc2c1c639db77ea7afe40251312bc4dfcd96089ac458e9c` |
| 9 | The two join them (both sign; an executor sends): the buyer long 5 of market 1 and short 3 of market 2, margins 32.995 each, each asking 33 | `acct-join` | 2,940 | 0.00824 | 8,240 / 8,574 | `7839604f681455b3919ce37eb7a1014982fb3a55143c19ac8d1c957ae95998e3` |
| 10 | An executor writes it with its legs the other way round (the account 100 DAA old) | `acct-turn` | 1,860 | 0.00406 | 4,060 / 12,329 | `eed985dc45cd845f6b12b80df1651a35ea99cfcf11123401ad7a8d0b7066e413` |
| 11 | And back | `acct-turn` | 1,860 | 0.00406 | 4,060 / 12,329 | `96d426f218b36406ba45fac2ccd47b356e50eea80a0c0107368a1e70f9cfffa3` |
| 12 | The seller: a sell of 2 of market 1 at 10.1 | `order-place` | 1,744 | 0.005214 | 5,214 / 52,964 | `e317a8ca4f4a4ae57cc45d5e177590ff0826684ca03cf83d2d673a8bdf76bd33` |
| 13 | The buyer: a buy of 2 | `order-place` | 1,744 | 0.005214 | 5,214 / 54,110 | `85d805a0f9150d3b3da2ae8f7c52e9efe2bf83848f6cc1210e0977d89ba9ea0a` |
| 14 | Matched | `order-match` | 3,588 | 0.007176 | 5,888 / 11,299 | `41b905483e27742981cbd60cc1274baefe178d4935349a84f5ffd78ad6d976d2` |
| 15 | An executor folds it into the leg (nobody signs): long 7 from 10.02857142 | `acct-fold` | 3,333 | 0.006666 | 5,633 / 10,193 | `330f68f47fd7b9fd3f8f4b738919985e5f5a48a383e3d26cbfa31d2466d60931` |
| 16 | The buyer: a sell of 2 of market 1 at 10.3 | `order-place` | 1,744 | 0.005214 | 5,214 / 53,102 | `67b3a7d225f54037a008ca27460252e797cb12618b0ff35c649dc7d5042a4a39` |
| 17 | The seller: a buy of 2 | `order-place` | 1,744 | 0.005214 | 5,214 / 51,485 | `d5bdab5d9ea666c50aecd5370f78b7cb4240f7106d81f5d484f6c3b8b5eb3b99` |
| 18 | Matched: the seller long 2 against the buyer | `order-match` | 3,588 | 0.007176 | 5,888 / 11,299 | `cdeaddd4463259b76ad45ff9ca00babd9b7c0959dcb91a3e69e63f70b4e3f995` |
| 19 | An executor sets it against the leg (nobody signs): long 5 from 10.02857142; the buyer's margin has the 0.5428 KAS it made, and the seller asks that much more of whoever takes the buyer's place | `acct-fold` | 3,333 | 0.006666 | 5,633 / 10,461 | `a31eac2ec482cceb1baaa4fc6ca6742b38d905fef41396ec39404150240caa9b` |
| 20 | The buyer: a sell of 5 of market 1 at 10.3, margin 12 | `order-place` | 1,744 | 0.005214 | 5,214 / 48,617 | `1094affc2a12e3fdc726765bac5a9740700de105f19c84a5c35596c045755a78` |
| 21 | A third user: a buy of 5, margin 24, asking 10 | `order-place` | 1,865 | 0.006335 | 6,335 / 47,701 | `7ac18d3c06473a1998b5c311eb3c390bb7cc436b5ee5bf998970d51ad4d115d8` |
| 22 | Matched: the third user long 5 against the buyer | `order-match` | 3,589 | 0.007178 | 5,889 / 10,246 | `84204fb9943e32ef0e942b7872b84d7c7cbd22e108c952bc48c7838f2d110ae8` |
| 23 | The leg leaves (the account's two users sign; the third does not): the buyer is paid 11.985 KAS, its margin in the sale; 1.357 KAS go to its margin in the account, which is now of one leg (short 3 of market 2, margins 46.885 and 22.853); the third user faces the seller (24 and 20.232) | `acct-leave` | 4,549 | 0.009679 | 9,679 / 13,056 | `382ca805611704a239ac5dc4213e82aeebad49f2be781b4922c8096f336b4bc2` |
| 24 | What is left closed by its two users | `acct-close-both` | 1,463 | 0.005913 | 5,913 / 12,214 | `220935bdf47945a72981883d3bcebda30c9a6375dc05bb0d7cf22c8c1106feeb` |
| 25 | The third user's account closed by its two users | `acct-close-both` | 1,463 | 0.005913 | 5,913 / 11,020 | `fe7223bb45a6a6e7371c88d813b8cc2827905f109a038ee7f7df6801747bf42a` |

What it showed: two accounts of the same two users in two markets became one of two legs by their two signatures;
anyone wrote it the other way round once it was 100 DAA old (the node holds the turn to the account's age); a fill of
the same two was added to a leg and another set against it by an executor, nobody signing, the gain realized into
the buyer's margin and added to what the seller asks; one leg was sold to a third user, who signed
nothing but its order, with the account's two users' signatures; what was left was an account of one leg.

Measured on the node: a placement 1,864 bytes and 0.006334 KAS; a match 3,708 bytes and 0.007416 KAS
(it was about 4,570 with its tips' output); a join 2,940 bytes; a turn 1,860; a fold 3,333; a leave
4,549 bytes and 0.009679 KAS.

The sixth library stays on testnet (75 KAS, and 3 in its anchors): library
`568624d70e12415f8f88c07df598cfcd910f8d0d90c41b6eca13d359182cba23`, genesis `run/live7/out/61c4df24-tx.json`; book
`6445f77e62d2a59d347078494d445aed03d0418a22a5fe6d8e387f5df2ae5868`, genesis `run/live7/out/b9e3ca0a-tx.json`. The
fifth library's scripts are those of before the lighter match and are not used.

Not run live on these scripts: a novation and a merge (run in the fifth part on theirs, unchanged since but for the
orders' shape), a cross account closed at the committee's two prices, a part of a leg sold, a cure, a late end and a
lapse, two placements racing for one anchor, two machines.

## Seventh part: separate machines

22 November 2026. The sixth library and its book, as the sixth part left them; no script changed. Five folders stand
for five machines, each with one key and its own files: a buyer (u), a seller (s), a third user (l), two executors
(e, x). Twenty-one transactions, twenty accepted at their first sending. Script: `run/live4/t8.sh`; files under
`run/live8/`.

What a machine was given, besides its key: the files of the library's and the book's genesis transactions (public),
the two markets, and the node's address. NO command was given the file of a script's copy or of an anchor: each
found a live one at the node (`lib-status` shows them: one query for the twenty-five scripts and the anchors). An
order's or an account's file went from the machine that had it to the one that needed it as a copy, and whoever
received an account's file read it at the node against the library and the book it had checked
(`acct-show --object F --node URL --lib GENESIS --book BOOK`: "the object is in the book").

| # | Step | Machine | Command | Bytes | Fee (KAS) | Transaction |
|---|---|---|---|---|---|---|
| 1 | The seller's machine: a sell of 5 of market 1 at 10, margin 15. Sent at the same moment as the next | s | `order-place` | 1,984 | 0.007454 | `0f5b46fdf5fa6f9d0f4c495ee391c097db26498e512a773d235149b9516523e6` |
| 2 | The buyer's machine: a buy of 5. Its first sending was refused (it had found the same copy of `KpfTPlace`, the library's only one, which the seller's transaction had just spent); sent again 11 s later with the copy that transaction wrote | u | `order-place` | 1,984 | 0.007454 | `089a49fe13e512975b2adfa43c8404e2740bda12ad163bc67664abcacdcb15e5` |
| 3 | The executor's machine, with the two orders' files: matched | e | `order-match` | 3,588 | 0.007176 | `8780fbfdf4d8203a5be5125301e6e2eb0a4c9383a2f471ffe7348c57c8844848` |
| 4 | The buyer's machine: a sell of 3 of market 2 at 20, margin 18 | u | `order-place` | 1,744 | 0.005214 | `120108c5d093ff42100a4b65d2ad8ed56bcbe925699a18a37fab875e4eaed736` |
| 5 | The seller's machine: a buy of 3 | s | `order-place` | 1,744 | 0.005214 | `f2c60a658914e9b157211e8910d907ecabed6b7b1167d2299de26b6efb91d7b3` |
| 6 | The executor's: matched | e | `order-match` | 3,588 | 0.007176 | `ecfdf2b3707e0ce041ef90c19501733f69d087e61faacf4c5a8cc795ff554898` |
| 7 | The join. The buyer's machine wrote it with its key (`--partial`); the seller's read it, looked its four inputs up at the node, signed and sent it | u, s | `acct-join, cosign` | 2,820 | 0.00712 | `5a3841cc1a476a6e81759724bdaa7e35497906f70aef27774e2d5149786e365a` |
| 8 | The seller's: a sell of 2 of market 1 at 10.1 | s | `order-place` | 1,744 | 0.005214 | `3dde73c7a1a7f3f7e82198d9ecc36ddd34390a100776d36135086562fc478af2` |
| 9 | The buyer's: a buy of 2 | u | `order-place` | 1,744 | 0.005214 | `3a86492a4fc74a2d87c9bf1825fe7c67dc9297bf174c2e1da6295d1cd698063a` |
| 10 | The executor's: matched | e | `order-match` | 3,588 | 0.007176 | `b3f99c73cb0a01dc6a72587dbde132a3b7d303d9ed951e7ac539bdddd1350235` |
| 11 | The second executor's machine, with the two accounts' files: folded into the leg (nobody signs) | x | `acct-fold` | 3,333 | 0.006666 | `0a9fe78d67f9b4ea7258eb2a2035d79e3d7ce8eb3542182b1fa18e45020363e4` |
| 12 | The buyer's: a sell of 2 of market 1 at 10.3 | u | `order-place` | 1,744 | 0.005214 | `6436c414e1c16489afda80169d7283137c6380a7a33d3f4b0b2fef45974d901d` |
| 13 | The seller's: a buy of 2 | s | `order-place` | 1,744 | 0.005214 | `afdd3bfe89ece382cd4a81c75ba8dd3ffbe8ed62e1e864949ade2200ed8d628c` |
| 14 | The executor's: matched | e | `order-match` | 3,588 | 0.007176 | `f4a42e9fd3272068aa22ddb19bd71a1816685d7e2fcf1e453466496099267044` |
| 15 | The second executor's: set against the leg | x | `acct-fold` | 3,333 | 0.006666 | `7845ec64009d61287a51a82a422b7a0325318f776b7ec0ab9e5a6c20beb4667e` |
| 16 | The buyer's: a sell of 5 of market 1 at 10.3, margin 12 | u | `order-place` | 1,744 | 0.005214 | `9fbba69248014d378665454ef882bc007143ec7f6549b5fe194ce631b406119b` |
| 17 | The third user's machine: a buy of 5, margin 24, asking 10 | l | `order-place` | 1,865 | 0.006335 | `b08427742f176918c4ee0571a1a2c27a6f0fbbb1f5b6a24f00100dfe88f2f790` |
| 18 | The executor's: matched | e | `order-match` | 3,589 | 0.007178 | `b3a726825d96edce03b2a0b02b9de2b1bc1c5cc174180533c6b9c66f904bab00` |
| 19 | The leg leaves, on three machines. The buyer's wrote it with its key and the executor's address (whose coin pays); the seller's signed and passed the file on (`--out`); the executor's signed its coin and sent it | u, s, e | `acct-leave, cosign, cosign` | 4,549 | 0.009679 | `d68233c1935ccbbe5ac917f9bd7d04530a14a2ed0257652c00d188a614bd1a53` |
| 20 | What is left, closed by both: written on the seller's machine, signed and sent on the buyer's | s, u | `acct-close-both, cosign` | 1,463 | 0.005913 | `1c80bff0f88c421aefb3adccc88aa4200d69f1768329c98c5d4314b5b69a578e` |
| 21 | The third user's account, closed by both: written on its machine, signed and sent on the seller's | l, s | `acct-close-both, cosign` | 1,463 | 0.005913 | `ddadc6e13d37e1f5b5483ab7c0d0751feadbde430c7eafc29a04e2a6c29ce435` |

What it showed.

- Two machines that place at the same moment: both found the one copy of `KpfTPlace` (and one of the three anchors
  each). The node took the first and refused the second as a double spend of a transaction not yet in a block; the
  second machine asked again 11 s later, found the copy the first had written, and was accepted. A library of one
  copy of each script serves one transaction of a kind a block; more copies serve more (a library is written with
  `--copies N`).
- What two users sign is signed where each key is. The file that goes between them holds the transaction and the
  states of the objects it spends and makes; the second machine holds every input and output to the script its
  state makes, looks every input up at the node (one query), prints what the transaction does, and signs. The leg
  that left was signed on three machines in turn (the buyer, the seller, the executor that paid), and the account
  that was left and the third user's were as in the sixth part to the sompi (46.885 and 22.85321278 KAS; 24 and
  20.23178722).
- The transactions are those of one machine: a join 2,820 bytes, a leg that leaves 4,549, a close by both
  1,463.

Not run live on these scripts: a novation and a merge (run in the fifth part on theirs), a cross account closed at
the committee's two prices, a part of a leg sold, a cure, a late end and a lapse, an opening signed on two machines
(`acct-open --partial`: run offline only).

## Eighth part: what the sixth library had not run, from separate machines

22 November 2026, after review T1b's round 15. The same library, book and five folders as the seventh part, and a
sixth for the committee and the funder (c). 25 transactions, each accepted at its first sending. Script:
`run/live4/t9.sh`; files under `run/live9/`. A `cosign` row is the transaction a `--partial` wrote on another
machine.

1. Two fills between the same two users (5 at 10 and 3 at 10.05, each asking half its margin), matched by two
   executors and merged by one: one account, margins 23.995 KAS each.
2. The user sold its 8 to a third user at 10.2; an executor took it out, nobody signing: paid 49.43 KAS (its two
   margins and the 1.45 KAS the price had moved), the two that stay facing each other with 24 and 22.545 KAS.
3. That account closed at the committee's price: the roster rolled, three seats signed 10.4 for a tick, the third
   user stated what it is owed (25.184 KAS: "the amount is what the rule gives"), and it was settled after the window.
4. Two accounts of the same two users in two markets joined on two machines, and the account of two legs closed at
   the committee's two prices (10.4 and 19.5 of one tick, three seats each): owed 13.0935 KAS, settled.
5. An account opened on two machines: the first wrote the opening with its key, the counterparty's address and the
   counterparty's listed coin (`utxos --address A --out`, `--b-funding`); the second, which checked the book, signed
   its coin and its place in the script and sent it. Closed by both, written on the second and signed on the first.

| # | Command (as the transaction names itself) | Bytes | Fee (KAS) | Transaction |
|---|---|---|---|---|
| 1 | `order-place` | 1,984 | 0.007454 | `a5d045d39fe976c2133a059cb217e4116e30119a758fb258ac6ca0a7de670af7` |
| 2 | `order-place` | 1,984 | 0.007454 | `b716d7e64d5fd8820703fdcb4cbbc086089d869ec4c6bc98779e90dc13c32ef2` |
| 3 | `order-match` | 3,588 | 0.007176 | `ffb11cc3d8dfe98048942940b9394d0de89e3249eeb624a82e7215e0303b335a` |
| 4 | `order-place` | 1,744 | 0.005214 | `5dc47d28803b172dd1e165a0e8eca0cd8e8c2fa0e19c07f7a16a4fa63402c07d` |
| 5 | `order-place` | 1,744 | 0.005214 | `56b87ddebbe435a1964cf900a30f7992d0309c8342aea83ced37fb8cbdaa9a47` |
| 6 | `order-match` | 3,588 | 0.007176 | `9bb6e34db6e4d19fe6477fb8252ff0463106790cb89e0df976d33334c0bddd94` |
| 7 | `acct-merge` | 2,739 | 0.005478 | `60d53aa5d39fd969dc331f3e64db724189c796387b50e2f228d0cb0f3cb6991c` |
| 8 | `order-place` | 1,745 | 0.005215 | `81b5831d91ec1c64fde34ed500d5618795a718ea0ab99d312b9a15998d07f5fa` |
| 9 | `order-place` | 1,865 | 0.006335 | `b43f51634867994a975436744a309cd174d7c45a05c13c4443645c94520c17cf` |
| 10 | `order-match` | 3,590 | 0.00718 | `0156e8d26803f90a2e0bbf77e0aa919bddaba726ddd96c2bc7590a48749be18b` |
| 11 | `acct-novate` | 3,828 | 0.007656 | `fc290d2218f16c9a1f7ffbcf42c3ba5d1917c0adb487d7b17479820d755f2ce7` |
| 12 | `roster-roll` | 20,680 | 0.04136 | `7357452f9ea252be76e6ca19f65ef8a5d3a6435cfd1d83433a766e3836c9beac` |
| 13 | `acct-declare` | 1,728 | 0.004828 | `4ffefe78c5ff1caba214a39f0a83b6e01cac39443876ccf4aec86acb1e61d61f` |
| 14 | `acct-settle` | 1,618 | 0.004068 | `20a38ee6a664fdfb8c8d147916173e282f6e53d2a35801fdb70b7e2fb725feee` |
| 15 | `order-place` | 1,864 | 0.006334 | `de2f8f4893ed3526a96920cfb07cf2717da6905595b8026a2e577f0d12e927f7` |
| 16 | `order-place` | 1,864 | 0.006334 | `cc0f7e24be3482d765700c69006e705b105d8271852fb5fa1bb0e3ba96c95f7f` |
| 17 | `order-match` | 3,588 | 0.007176 | `7572963e1836742875f7c8e8ebf726c778b5b166336b3212cd1afc24f0e6ec68` |
| 18 | `order-place` | 1,744 | 0.005214 | `52fa290d4fd1ca1deac1ce19b778c99430d44fae2b7002c896e3c9f12a4d3e72` |
| 19 | `order-place` | 1,744 | 0.005214 | `2e920c2ddf91d07ee6a7474a08522fdc601cff07bbb04fcaab75e184980db379` |
| 20 | `order-match` | 3,588 | 0.007176 | `454842e4cfef937991ca24bc26e03219f73d3e08b6887ca650aad4dc8a752896` |
| 21 | `cosign` | 2,820 | 0.00712 | `e8209cd6b28ac526f060a8529a6fd4132c0864c46bae87f2d63fa4bf8fe485d4` |
| 22 | `acct-declare` | 1,731 | 0.004831 | `984017346c6e12ab330146228f5a19d93864df77605297cc8f12111151b1be48` |
| 23 | `acct-settle` | 1,621 | 0.004071 | `acc31fb900de8823b341bb60659feeb72a8d71a00ba314e6a7189f8174a6fcbf` |
| 24 | `cosign` | 2,516 | 0.010346 | `996058ff81f883617187d32f3d4b144949245c09db08ddd09d7dd441039748ff` |
| 25 | `cosign` | 1,463 | 0.005913 | `75c7c733aa71516adafed1e07bf3db14f22abf72a1fb6f09305c70510b8daebc` |

(The ninth part runs these.) With the sixth and seventh parts, every script of the sixth library but these had run live on it: `KpfTAdd`,
`KpfTLate`, `KpfTDispute`, `KpfTProve`, the five of a liquidation (`KpfTLiq`, `KpfTLiqSettle`, `KpfTCure`,
`KpfTLiqDispute`, `KpfTLiqProve`), the three of a no-price claim (`KpfTQuiet`, `KpfTQuietEnd`, `KpfTQuietRefute`),
`KpfTCancel`. Their forms of before the book ran in parts one to four. Not run either: a part of a leg sold.

## Ninth part: the rest of the sixth library's scripts

22 November 2026. The same library, book and folders; the tickets of the eighth part's roll (its epoch still held
the present). 31 transactions; one was held back by the tool for a second (a cure of a liquidation not yet 150 DAA
old: "usable from DAA ...") and sent at the next asking, the others accepted at their first sending. Every account
was opened on two machines (`acct-open --partial`, `cosign --book`). Script: `run/live4/t10.sh`; files under
`run/live10/`.

1. Margin added. A false claim (30 KAS where the rule gives 21.5: `acct-show` on the second executor's machine says
   "CHECK: FALSE") disputed, and ended after the window: the disputer 7.8 KAS, 2.6 KAS to the party it was made
   against. A true claim at the committee's 11, disputed all the same, proved with a ticket and the three seats'
   ticks: paid at once, the claimer with both bonds.
2. A liquidation stated at the entry price (false: disputed by the side, ended after the window, half of the
   liquidator's bond nobody's); the rule's at 9.7, cured with 2.31075 KAS; the committee's at 9.5, disputed by the
   side, proved by the liquidator, settled a window later (the liquidator 3.306075 KAS, the other party 28.03575).
3. A no-price claim ended by a tick the seats signed after it (its bond shared); a second, which nobody answered,
   settled at the entries after the quiet time.
4. A claim whose tick was 300 s old when it landed ("LATE"): ended by an executor, the account again; closed by
   both, written on one machine and signed on the other.
5. An order returned by an executor after its expiry, and one cancelled by its owner.

| # | Command (as the transaction names itself) | Bytes | Fee (KAS) | Transaction |
|---|---|---|---|---|
| 1 | `cosign` | 2,516 | 0.010346 | `d4749798547ce2b54741580590e5d2c371f1d334592f0c117047614fd563b32c` |
| 2 | `acct-add-margin` | 1,528 | 0.004628 | `5d581494d4295e25aacf55642f4c72a2cedb317772d0fb97d1966693d7c4eb22` |
| 3 | `acct-declare` | 1,728 | 0.004828 | `a8e6a4b47af696a6846c977ea13e2a977b62652a9b83e12ba11bcbe70cde6ecb` |
| 4 | `acct-dispute` | 2,346 | 0.005446 | `74770693f1fcfb49919dd387def233a74b6b67771b399554ead69243bead3d1d` |
| 5 | `acct-lapse` | 2,330 | 0.00525 | `1d02c34ad465d848d12c3f567005a7b4ea3f0526f97534f61bef81883c79b097` |
| 6 | `acct-declare` | 1,728 | 0.004828 | `4acd15c028f44c550db9055309a01ccc732d68b96658f56ccbbee00dc27f7e57` |
| 7 | `acct-dispute` | 2,346 | 0.005446 | `d5709a302b81031a53a89b816326508b0f9ea659541c296f77dadde849aed5e7` |
| 8 | `acct-prove` | 8,398 | 0.016796 | `6237a43b12f82c93807f4e2842c1a23449425d5fbcfb3095c59265c1af0a125d` |
| 9 | `cosign` | 2,636 | 0.011466 | `f46d06f97b977e7ba33085cb8bd9fa393755c94bdb200d5472e90953e711bfb0` |
| 10 | `acct-liquidate` | 2,367 | 0.004734 | `3be014fdf39ecb2d7f1234b4ee9e8833a719c96628e876dc64ee9cc590309d38` |
| 11 | `acct-dispute` | 2,323 | 0.004646 | `00ccfc2fbf2d67257aa2bb1055817b33db889cde17fdfe989ed9ed1b88460057` |
| 12 | `acct-lapse` | 2,375 | 0.004935 | `969c21769afed77346c262a51bd29865b1ec9aee4b883459cdd251d7ec2e769b` |
| 13 | `acct-liquidate` | 2,367 | 0.004734 | `62ab1af57e95a81e98daa10b7b98596e94148b4fb470e9dc00bbbb4365100a65` |
| 14 | `acct-cure` | 2,147 | 0.007067 | `fe3ba6488161b5fb82e1821162c41076b3d00ed5283d75b3ad831ecd8f92ca86` |
| 15 | `acct-liquidate` | 2,487 | 0.005687 | `37352f4446b21c8cb85c2608d818cc72aacf014990660090cebfa3f1449f5312` |
| 16 | `acct-dispute` | 2,443 | 0.005643 | `25cc0b8b0a336e3411951d7d9df36d01057ebc777b4623cd14dc38cf7096d614` |
| 17 | `acct-prove` | 8,703 | 0.017406 | `4136bacdb93fadafbe236745ed0f1801151183d2873754d622fc4a586dee041e` |
| 18 | `acct-settle` | 1,748 | 0.004198 | `c15726e564323b1f4e5aa3c252d06bcadccf7ba72283a9557ef126f5287c9bfe` |
| 19 | `cosign` | 2,396 | 0.009226 | `c80265a6e7dda65a784e73b73ff1c15bbacc064d19b968dcc7cc78dce3accc68` |
| 20 | `acct-quiet` | 2,198 | 0.005298 | `65fd46b1f71e95e675318c1d85ebe5ccdb7396b27d1f571c8cad3a0fedd39d29` |
| 21 | `acct-refute` | 7,283 | 0.014566 | `6223f514b2bd6f98a579c4851a2dea8aab3c4554cc4217e518c2b1544f47d638` |
| 22 | `acct-quiet` | 2,198 | 0.005298 | `83dfcfee1f49a5e71148d287e4ff4f1fa005d0eb3bdff8b4313f806c42b37bf6` |
| 23 | `acct-settle` | 1,705 | 0.005515 | `6882d949af278e4c0d70b060cd8350164f1b334a3608a99f5d1397d5218c8127` |
| 24 | `cosign` | 2,636 | 0.011466 | `d1f1a81cd282e5f6b492f2184db88f242632ad3d1bb596d4f4fbb372ee4d2890` |
| 25 | `acct-declare` | 1,727 | 0.004827 | `2a9ea3ec0630715ac567edebcdbe11cc5dfd50bd2b44e8f4da909c61ebebc52f` |
| 26 | `acct-late` | 2,141 | 0.007681 | `42384efffe5674ce3df998893076a7235a7cb381130ebffb8218af19d0c27b10` |
| 27 | `cosign` | 1,635 | 0.007445 | `7fef5779b857a9c5edeadd2f35be2dcf30d6e215729cafecfaa0fa8f932a91a5` |
| 28 | `order-place` | 1,984 | 0.007454 | `1dfe34b4c6a414455ed5ee5082ae6c9c5b8f137c725c89add8400a94c810c385` |
| 29 | `order-place` | 1,864 | 0.006334 | `d27407810468dc3350c2e1df9ede7f508bf8cd3a6eaf8a3168d5d80874a193a7` |
| 30 | `order-expire` | 2,299 | 0.007389 | `ccbda2c2f834386250d393a8d521fb164f15e02f5683e321be9d52a79250efe5` |
| 31 | `order-cancel` | 2,005 | 0.005095 | `8cdc2cce0c609e99f9fb09ea1a460e3b0167abd8ede544ec0fee7f83a6cbe9f8` |

Every one of the twenty-five scripts of the sixth library has now run live on it, each transaction built without
the file of a script's copy or of an anchor. Not run live: a part of a leg sold (`acct-leave` of less than the
leg), a proof with two tickets (two legs).

## Tenth part: two executors left running, and users that send nobody a file

22 November 2026. The same library and book. Five folders, a key each: three users (u, s, l) and two executors (x,
e). Each executor ran `exec-run` for 46 rounds of 15 s (x doing everything it can be asked, e what gains); the users
ran single commands and `book-sync --only KEY`. No file went from one folder to another. 16 transactions. Script:
`run/live4/t11.sh`; files under `run/live11/`.

1. Two orders that cross (5 units, a tip of 0.05 KAS each). Both executors read them from the chain; e matched them
   a round later (left 0.0928 KAS richer: the two tips less the fee). The users' `book-sync` wrote the account's
   file on each of their machines, and `acct-show --node --lib --book` held it to the book.
2. A second fill of the same two users: both executors sent a match of it in the same moment; e's was accepted, x's
   refused (the order's output already spent), and x left the two orders alone for three rounds. e then merged the
   two accounts (left 0.0045 KAS: the library's 0.01 less the fee).
3. The user sold its 8 units to a third user: x matched the two orders and, in the same round, took the user out of
   the two accounts (a novation: left 0.0073 KAS). The third user's `book-sync` wrote the file of its account
   against the user that stayed; the user that left holds no account.
4. A claim stating a tick 3,000 DAA old: e ended it a round after it landed (left 2.3151 KAS, half the claim's bond).
5. A claim stating 30 KAS where the rule gives 23.592, on an account whose window is 300 DAA: NOT disputed. An
   executor read it in one round and acted on what it read in the next, 30 s after the claim landed: the window had
   passed. x, asked to settle claims, then settled the false claim itself. Both are corrected in the eleventh part
   (what a reading writes is acted on in the same round; a claim that is not the rule's is not settled by this tool).
6. An order that expired after the executors' last round: not returned (run again in the eleventh part).

What the node was asked for, in this form of the tool: every transaction of the chain in the span, whole
(`getVirtualChainFromBlockV2`, verbosity High), read only when a copy of a script or an anchor had moved. The chain
carried 60 to 140 transactions a second that day. An executor's reading of two rounds: 1.7 to 7.9 MB (12 readings,
about 50 MB an executor in eleven minutes). A user's `book-sync` after a minute or more away: 9 to 87 MB (about
300 MB for the three users). Too much to ask of a node somebody else runs: the eleventh part reads otherwise.

| Machine | Transaction (as it names itself) | Bytes | Fee (KAS) | Id |
|---|---|---|---|---|
| u | `order-place` | 1,867 | 0.006337 | `4decd1b23ec736dd436cfe6d97e6c69683494dddf87378ceaafd12e9b8235c6c` |
| u | `order-place` | 1,747 | 0.005217 | `b722279c6e34ad4d00cce95fa4f5f449769c8a7fddc5ead73b3b5ea794213209` |
| u | `order-place` | 1,748 | 0.005218 | `3afed9723d6ca6fa4cc3e402d30958d7c66f7180c0ca116ec79f7487bd25e5c2` |
| s | `order-place` | 1,867 | 0.006337 | `36e93d76c6316d05ab3df9ec78c9633d3d67e5fdd270ecfec06711b8646adcfb` |
| s | `order-place` | 1,747 | 0.005217 | `1861cdd4f1ec648cf8d0268e4c2b2917e9d0e1adc2117b37fe57bfca98e308c5` |
| s | `order-place` | 1,984 | 0.007454 | `1b44a12fa36d8e16d2b7c036fd5fc3095c1b2a212e7f2ef3ee8fa9f0648ebd8b` |
| l | `order-place` | 1,868 | 0.006338 | `0657ef295baba8aa110e0f193b29095334ceb8b3121005edfeda1c94aa19471d` |
| l | `acct-declare` | 1,728 | 0.004828 | `01105da0a7a8423fa2f1bc9a8a75725d9dd4dbd9265bb1728fdd090d36bff7cf` |
| l | `acct-declare` | 1,728 | 0.004828 | `f45ffa2a53199458eefa7f1f7aeebe52057043d6a1afce45dbb3fcddf0adc30a` |
| x | `match 8 units at 10.2` | 3,596 | 0.007192 | `b7a6df122ec063cb1704e0154e87824fba5b409b15606c25dff61a031aa9f7c0` |
| x | `novate` | 3,828 | 0.007656 | `20bd1d8ac2a5ba3b9ac63cc098a6bd9895c5750c49b5bc3f4395667312a69566` |
| x | `claim settled` | 1,618 | 0.004068 | `fd3ccb7bc34b1681b86f6779284e68709af75dc7e0cd29b3eb487f806c818492` |
| e | `match 5 units at 10` | 3,594 | 0.007188 | `949cd06ded3e20fc22680728721c26c3dced479ef1a71eb2764ccab486e99a17` |
| e | `match 3 units at 10` | 3,594 | 0.007188 | `bdcd8d2a19f24d316f82f79269dafdc2af5f40225a5563ee931fe3997235c406` |
| e | `merge` | 2,739 | 0.005478 | `d5fe50d8e6edb22271d2de8b79e5dda35a249ac9317abc6bd2fa2ed9a45c538e` |
| e | `late claim ended` | 1,798 | 0.004618 | `99a57d09ed8ed81f31204d9614a87a5e7000f5352a059c2dde390a46001909e2` |

## Eleventh part: the chain read by what moved

22 November 2026, the same day. Two users and the two executors, 32 rounds of 15 s. The tool now asks the node which
chain block accepted which transaction id since the last reading (ids only), walks back from where the library's
copies and the book's anchors stand to where they stood, and fetches only the transactions on that walk, each from
the blocks its accepting chain block merged. 9 transactions. Script: `run/live4/t12.sh`; files under
`run/live12/`.

1. Two orders that cross: x read the second and matched the two in the same round.
2. A claim of 20 KAS where the rule gives 11.8, window 300 DAA: x read it and disputed it in the same round (its
   bond 2.4 KAS), and ended it after the window: left 3.5948 KAS, its bond and half the claimer's. The account again,
   its two margins whole.
3. An order with an expiry 250 DAA ahead: x returned it to its owner the round after its expiry (0.004 KAS, x's own:
   asked by name).
4. A `book-sync` when nothing had moved: "nothing read" (one list asked of the node).
5. The account closed by both.

| What the node was asked for | Tenth part (whole transactions) | Eleventh part (ids, then the book's) |
|---|---|---|
| An executor's reading | 1.7 to 7.9 MB | 0.10 to 0.31 MB |
| An executor, the whole run | about 50 MB in 11 min (11 and 12 readings) | 0.9 and 1.5 MB in 8 min (5 and 7 readings) |
| A user's `book-sync` | 9 to 87 MB | 0.14 to 0.78 MB; nothing when nothing moved |
| A round in which nothing moved | three small calls | the same |

An hour later, with the binary of after review T1b's rounds 16 to 18: an executor and a user started again on their
folders are refused ("the folder's reading of the chain stopped 64 minutes ago ... --catch-up, or ... --from-now");
with `--from-now` the executor begins at that moment, and started once more at once it goes on from its cursor file
and reads nothing.

Not run live: a reading over more than half an hour (`--catch-up`), a fold by an executor, a liquidation disputed or
ended by an executor, a round that fails (the node away).

| Machine | Transaction (as it names itself) | Bytes | Fee (KAS) | Id |
|---|---|---|---|---|
| u | `order-place` | 1,867 | 0.006337 | `1700b342a0d998c9c3716834e8cd51643ec2656dea98419110fca6ecfb93dc46` |
| u | `acct-declare` | 1,727 | 0.004827 | `097071c89d669e606f277ab92d3485e453bda2f312e6ba9485afcc9c80d36000` |
| u | `acct-close-both` | 1,463 | 0.005913 | `b04c93fe557f0ef024b129a637315f71d8cdc1657e9f4e5ea018c0ae0c349189` |
| s | `order-place` | 1,747 | 0.005217 | `f22a4a8305b598b997a6d751cc0b64d0e3421a4e7185499c333894fd358a1d2a` |
| s | `order-place` | 1,864 | 0.006334 | `0ebadfef306e1247a1437ee64005fe48f6ca3e1bc719fb194ea0d2a394a7aea9` |
| x | `match 4 units at 10` | 3,594 | 0.007188 | `be67021a7cd467882245e8c175c7fa93c888474a649f66ebab962e014d37da66` |
| x | `claim disputed (its amount is not the rule's at its own prices)` | 2,225 | 0.00445 | `18545a8f0980d768992309878aa339d4da7b99c459610bbdd754a467809f41b8` |
| x | `disputed claim ended, unproved` | 2,329 | 0.005249 | `64368468901ffe67d5d8d33f9277fe3ce7923e724c7cf927d2b5cf5453518c88` |
| x | `order returned after its expiry` | 1,939 | 0.004029 | `156003c1b09876b7cef57df1e742dba44527a243493afc47365bf227cf1266fb` |

## Twelfth part: the dues, from an operation to the seats' keys

22 November 2026. A new library (every script changed: each asks its copy to go on with 0.002 KAS more, and lets the
copy be swept) with a purse for the testnet roster, and a book of it. One executor left running for 34 rounds of
15 s, doing all it can be asked; two users; the committee's machine. 12 transactions. Script: `run/live4/t13.sh`;
files under `run/live13/`; the binary `run/live13/kpf-live13.exe`.

- Library `6661cca2463c4ee55bc2431c3bf8f7e2518bd21cdb40c03b621c8b7f26e07b23` (genesis
  `run/live13/g/6b4c5e42-tx.json`): one copy of each of the 27 scripts and the purse, 2.4 KAS each (67.2 KAS; the
  first try, at 2.5 KAS, asked more than the key had in coins it could use and sent nothing). Book
  `b624417cc1d65c131adaea157ba28fcc2d0e24309b5e48824c51e5d98776f0cc` (`run/live13/g/c3c97103-tx.json`).

1. Two orders that cross (a tip of 0.05 KAS each): the executor matched them and was left 0.09065 KAS (the tips
   less the fee and the dues of 0.002 KAS, which stood in the copy of `KpfTMatch` after it: 2.402 KAS).
2. A liquidation stated by hand where the rule liquidates nobody (the tool says so to whoever sends it): the
   executor read it and disputed it in the same round (its bond 2.4 KAS), and ended it after the window of 300 DAA:
   left 3.592974 KAS, 1.186 KAS more than it had put in. The first liquidation an executor disputed by itself on
   the chain. Half of the liquidator's bond stayed in the copy of `KpfTLiqDispute`, with the dues of the two
   operations: 1.204 KAS above what a copy keeps.
3. The executor swept that copy two rounds later, by itself: 1.1438 KAS to the purse, itself left 0.054888 KAS (a
   twentieth less the fee). The copy stood at 2.4 KAS again.
4. The roster rolled into the epoch that holds the present; its three seats signed a tick; `dues-pay` with a ticket
   of that roll: 0.36 KAS to each seat's key (equal weights), 0.05719 KAS the sender's before its fee, 0.00661 KAS
   of rounding left in the purse. Each seat's key held 0.36 KAS more after it.
5. The account closed by both, each its 12 KAS.

What the node was asked for by the executor in 8.5 minutes: 5 readings, 0.87 MB (34 rounds of three small calls
beside them). Nothing was refused.

Not run live: a sweep by `dues-sweep` (the executor's loop sent the one there was), a copy filled by dues alone (100
operations), a payment to more than three seats, two executors after the same copy.

| Machine | Transaction (as it names itself) | Bytes | Fee (KAS) | Id |
|---|---|---|---|---|
| g | `lib-create` | 3,106 | 0.017826 | `6b4c5e4243f22f1cb26080893eed2ed986472b5bc749f39123287da26b5efa45` |
| g | `book-create` | 560 | 0.00366 | `c3c971036af0f7c752fdf6eb4673326fed77e2a95fb0fe1d368cbfb4423aeb88` |
| s | `order-place` | 2,068 | 0.007538 | `a64b11971c0bac2d860fe2c418ec5873affcc2303584ae4e9adaa9798b6811d1` |
| s | `acct-liquidate` | 2,458 | 0.004916 | `90c09ed0433f43b0eb4f480c907306d6d3170126054548fb9a0bdeacf6b89a07` |
| u | `order-place` | 1,828 | 0.005298 | `4e1e2fd5d04a514dbb57d507a8f15ef0fa8dedc0e31e725688c247c4a1d89763` |
| u | `acct-close-both` | 1,544 | 0.005994 | `5f9f6f4b0f49ec421d8bdc8070962a836e3ca40a07e05cebfdf5f71b7313ac75` |
| x | `match 4 units at 10` | 3,675 | 0.00735 | `0dc7511dfe7cc075e040e46ac5a723df9a427b3637882fd2049399eca2491488` |
| x | `liquidation disputed (not the rule's at its own prices)` | 2,414 | 0.004828 | `fbc42ff568b539a8c3ce012ff0ae97e91be0ae73874bb0388d80c8133573f237` |
| x | `disputed liquidation ended, unproved` | 2,466 | 0.005026 | `43a66367e9fcd75326f4bcf6908b59c85822e65f7889368ee60989112f36ff1b` |
| x | `1.204 KAS of dues swept out of a copy of KpfTLiqDispute` | 2,656 | 0.005312 | `4a8b1b46d85400a3d95168577f64a85fe5dd8634c4a7c6f48c4011632ee32252` |
| c | `roster-roll` | 20,680 | 0.04136 | `46408e1813cc31abd9ccda8c37d0a4ecf190e5d400f312938bf9c7ea78d12286` |
| c | `dues-pay` | 7,439 | 0.014878 | `58d94cd0a8eccff9f34f653eaccf73efa87920c5305f1c90c0db4f594b53d0ba` |


## Thirteenth part: the committee answers when asked

Live run 15 (22 November 2026, `run/live4/t15.sh`, the binary `run/live15/kpf-live15.exe`): the rule of
`DESIGN-COMMITTEE.md`, "The committee answers when asked", on testnet-10, with the names of the renaming. A new
library with a fee pool for the testnet committee (`6cc356a641496cd15621300b0ae72c24ad9b452093a6da3871c4a058f8daf4d2`,
27 scripts, one copy each) and a registry of it (`54a23c136943c5437da063b44ada24306c02321f030ef59d5f58b6219557f59c`).
The committee was rolled into the epoch of the present (630,042,372 to 630,078,372). An executor ran 56 rounds with
everything it can be asked and the folder the members' signed prices arrive in. Nobody signed a price before a
dispute: the three members signed, each with its own key, the price of a claim's time after the claim was disputed.

1. Two orders crossed (4 units at 10, 12 KAS of margin each); the executor matched them.
2. **Refuted.** The long side closed at a price of its own, 10.60, stating 14.188 KAS (what the rule gives there with
   the tolerance against it). The counterparty disputed (a bond of 2.4 KAS). The members signed 10.00 for the claim's
   time. The executor's next round refuted the claim with their signatures: the position again, whole; the disputer's
   bond back; 1.2 KAS (half of the claimer's bond) to the executor, which was left 1.179778 KAS after its fee; the
   other 1.2 KAS in the copy of `KpfTProve`. The executor swept that copy by itself (1.1419 KAS to the fee pool).
3. **Proved.** The long side closed again, at 10.02, stating 11.8796 KAS. The counterparty disputed all the same.
   The members signed 10.00 for that time: more for the claimer than it stated. The claimer's own watchtower
   (`exec-run --do watch`) proved the claim with their signatures and was paid its amount, its bond and half of the
   disputer's; the other half went to the copy of `KpfTProve`, and from there to the fee pool (2.2838 KAS in it).
4. **Unanswered.** A new position (2 units). The short side stated a liquidation that is not the rule's (fee 1, cure
   2, at 10). The executor disputed it (a bond of 1.2 KAS). No member signed. After the dispute period (900 DAA) the
   executor ended it: the position again, and each its bond back. The false liquidation cost its author the fees
   only: what the design states of a committee that does not answer.
5. The position closed by both, each its 6 KAS.

The executor sent 7 transactions and none was refused. The answers are the large ones (an epoch table's attest is
in them): 9,111 bytes refuted, 9,024 proved.

Not run live: a refutation or a proof of a disputed liquidation; two legs; an answer by a party that is neither the
executor nor the claimer; a claim for whose time the members' prices differ; the payment of the fee pool to the
members under the rule that only the table of an ended epoch pays (the epoch of this run ends at 630,078,372).

| Machine | Transaction (as it names itself) | Bytes | Fee (KAS) | Id |
|---|---|---|---|---|
| g | `lib-create` | 2,866 | 0.015586 | `21e6a1036fdc7240d176b8c2703554a710a8e51b6b5fa6caebd8973b1a9bc7d0` |
| g | `registry-create` | 560 | 0.00366 | `e1349fb92a234e56e80727d35b459635a80a38243f6b15ce5aa0ba40d9219312` |
| c | `committee-roll` | 20,800 | 0.0416 | `0a1af207fd668e836723e9f4bb3bf1605180c8d15799c4cc420d3bcfe0d66707` |
| u | `order-place` | 1,828 | 0.005298 | `d02558e6733e0b189bfd2a8226bdd8b435edd1d1292796e1dff0d0dc165be948` |
| u | `pos-close (at 10.60)` | 1,808 | 0.004908 | `e56779b9ef1b15d173b82e1cde003d69d94893e6c8caa7b65af71e38a93c165d` |
| u | `pos-close (at 10.02)` | 1,808 | 0.004908 | `63f25bf624393e2221234e3af733c39bca6c151cd83be01bde8443d58de0bad7` |
| u | `order-place` | 1,948 | 0.006418 | `ef2bfb7655c7290da4da65846936d6ed021125da3ab5a0bc2c4f8c13446db73b` |
| u | `pos-close --mutual` | 1,544 | 0.005994 | `c87d74cd568c3ae6e4c67591b53e9c87a0c25dd334b629ef86750f0c3cf1b1df` |
| s | `order-place` | 1,828 | 0.005298 | `522641ad35839570470db5606bba794a6d672edef0cf24a63c8ce1b00c17a69f` |
| s | `pos-dispute` | 2,299 | 0.004598 | `392036e38b0fcfbbc4382bcb06f96975a16a1edeea8f70f10f8e255ece003f62` |
| s | `pos-dispute` | 2,419 | 0.005519 | `941dad00816e4c712a5b8747d4267b33ad878f5eee142d59ecfbb2957fa27553` |
| s | `order-place` | 1,948 | 0.006418 | `114464e9b887f78a5125dc94a1b8615f56f377f7a17a28111df0189267b7c167` |
| s | `pos-liquidate` | 2,458 | 0.004916 | `66dbd113421b6b11e623cc0e65971e427d9f96e238a7f6a85ab7d4e21452e1b7` |
| x | `match 4 units at 10 (d02558e6 with 522641ad)` | 3,675 | 0.00735 | `3bb33a4c191983f53a7dd8f4e36f4777e94d2225cff6e4327e5e0a3ee996d731` |
| x | `disputed claim 392036e3 refuted with the committee's signatures` | 9,111 | 0.018222 | `93b3754202cf5a8721c311a7d00b200f23196b51d38b78de67ae34172db8e72d` |
| x | `1.202 KAS of oracle fees swept out of a copy of KpfTProve` | 4,534 | 0.009068 | `0d2e37b8a6f583ef07e1d8620d72916373bb089fb5a4df046b35032dfde4b9d6` |
| x | `1.202 KAS of oracle fees swept out of a copy of KpfTProve` | 4,534 | 0.009068 | `771cc993205fb4b0217cb68f1fe73b3a2543d9b97272d534321ebe82bd71cdaa` |
| x | `match 2 units at 10 (ef2bfb76 with 114464e9)` | 3,675 | 0.00735 | `d3f75bedf64f944749bb59d2d2fea42bcbc4edcc363b4fd295da6995d10c29ae` |
| x | `liquidation 66dbd113 disputed (not the rule's at its own prices)` | 2,421 | 0.004842 | `0b31a1024e6fd2e5d61e7aaaa0257c9de3e76e2149974e5fb516983c917c50ae` |
| x | `disputed liquidation 0b31a102 ended: nobody showed the committee's prices` | 2,525 | 0.005445 | `592865301b5df26252111187e78446b9d506515c81054ac4e1d29619bcc736f0` |
| u (watchtower) | `watchtower: disputed claim 941dad00 proved with the committee's signatures` | 9,024 | 0.018048 | `a3634a6ebfd7de728087d5e55255f9a6976a6aebf5adbec5ba53cacf3e17753f` |

## Fourteenth part: closing and opening in one transaction (run 16, partly)

Live run 16 (23 November 2026, `run/live4/t16.sh`, the binary `run/live16/kpf-live16.exe`): a new library of 28
scripts (`KpfTReplace` among them) and a registry of it; an executor with everything it can be asked and `--do take`
(`--take-max 30`); three members running `exec-run --do answer`, each reading a price file for itself. Nobody signed
a price by hand. Four of the six scenes ran; the run stopped at the fifth.

1. A plain match: u long 4 against s at 10.
2. **A match into a position** (`KpfTReplace`, 5,355 bytes, fee 0.01071 KAS, id `a47bbc0aacd0b1d505df1a2ecfab321edd74f7eff761d917e72705897872102e`): u
   left by a sell order at 10.2 and n, whose buy order it was matched with, took its place in the same transaction.
3. **The executor answered an order nobody matched** (`take`): n's sell order at 10.0, 2 % under the executor's own
   price; the executor placed a buy order of its own (12 KAS of margin) and the next round matched the two into the
   position (5,352 bytes, id `4adc2adec7ae4f1a21f30d3473ebd7b53997d5ecbd4a35e0ce56f615d6e50ef3`): n out, the executor in its place.
4. **Not done: the executor passing on what it holds.** u's buy order at 10.3 was a way out for the executor; its own
   order the other way was not built in four rounds: "insufficient funds: need 15.416 KAS, have 11.8155 KAS". The
   order by which it leaves posts as much margin as the one by which it came in, so an executor that holds a
   position needs that much again to leave. To be changed (a way out needs no more than the order asks of it).
5. **An exit claim answered by the members themselves:** s stated that the committee had signed nothing since DAA
   630,270,487. One member signed for 630,270,600 (the others had no reading each side of it); all three signed
   for 630,270,900; the executor ended the claim with those prices and was left 2.383272 KAS.
6. **Not run:** a close at the claimer's own price, disputed, signed for by the members and proved (u held no
   position after 4, and the script stopped there). The executor's and the members' processes outlived the script
   and were stopped by hand.

The executor sent 7 transactions; the node refused none. Left on testnet: the executor long 4 against s.

## Fifteenth part: a price note, its citation, and a take (run 17)

Live run 17 (25 November 2026, `run/live4/t17.sh`, the binary `run/live17/kpf-live17.exe`): a new library of 41
scripts (genesis `74ad84ded8cd1d2b097f6a9a6fc74992b5bcc383da0db2874710b405ca9e7d69`, 4,084 bytes, 4 KAS a copy: the
storage mass of its 42 outputs is 419,994 of the 500,000 a block allows) and a registry of it. The members signed by
`price-sign`; every other step was a command of `docs/ARCHITECTURE-CLEARING.md` 3 and 4a. Every transaction was
accepted at the first try.

1. u long 4 against s at 10, by two orders an executor matched.
2. **A take answered at once.** u stated that the entries move to 10.5 and that it takes 8 KAS (all it had above
   what s asked of its margin). The members signed 10.5 for that time; the executor's key wrote the note; a citation
   of it was sent alone; and u answered its own statement with a citation: within the rule, applied in that
   transaction, 8 KAS and the bond to u. The position went on from 10.5 with 6 and 10 KAS.
3. **A take that waits.** u stated a take of 1.2 KAS at 10.8; nobody disputed; after the dispute period (900 DAA)
   the executor's key applied it.
4. **A take that is not the rule's.** u stated a take at 11.2. The members signed 10.8 for that time, the note was
   written, and s answered with a citation: not within the rule (the price stated is beyond the tolerance of the
   committee's), the position again, and half of u's bond, 0.74 KAS, to s.
5. What the two notes held (1 KAS each) returned to their writer.

| What | Bytes | Fee (KAS) | Id |
|---|---|---|---|
| a take stated | 1,997 | 0.005197 | `89c94bc3a201c0ec156e911216eacbbe22388a75e3681f89aedb8ac7e5df477a` |
| a note written (three members) | 6,131 | 0.015001 | `9cfad65466c13133a261be98ea6ec487c093a796e5383ad99cc516bee96c6110` |
| **a citation alone** | 5,587 | 0.011174 | `6a9770531c169f999caa2f431983b805fc5207ed58bcdd7b51077a4312040aa1` |
| the take answered at once: within | 10,390 | 0.02078 | `ae3afeaf7fd13bd23ceecfcf9176d8aa227eb06b768574b6baead56ff21d0b44` |
| a take stated, and applied after the dispute period | 2,117; 2,706 | 0.006317; 0.009706 | `f2dfa6060f8af34e2368e5f7e6623722b7a4890f5fc563b1ab204794fe82d61a`, `7b76ddce350577ef955e6c7029d5174d19c49ddb96b6ccebf317891e24b6990e` |
| a take stated against the rule, and answered: not within | 2,237; 10,279 | 0.007437; 0.020558 | `2ea2c509982b15b210593d3c7576e3f02df9ff53cda19248571b70f2e3840f66`, `3c74d5286a31a3ffa63168cb876b1238f607b214f14b9b610e8cde74b8e76915` |
| a note's value returned | 861 | 0.005581 | `93ba04bd7c9e29eb3d84c98929eb7a8543094c91e8766e41dc0d4ff6a3bab707` |

What this settles: **the citing script passes on testnet-10 against real chain blocks**, three times (alone, and
inside two answers), with the proof made from what the node says of the block (6 and 1 steps in the lane's
activity, 4 in the lanes' tree: 608 and 448 bytes). A citation alone costs 10,517 grams of compute mass.

What it does not: the scripts of this run were given a statement by its hashes. After the run `KpfTChange` was
changed to take what is stated and what the position becomes as they are (so that whoever watches reads them on the
chain), and `KpfTTake` to read its numbers there: the scripts in the repository are not those of this library.

## Sixteenth part: statements that show what they state, and an executor that watches them (run 18, partly)

Live run 18 (25 November 2026, `run/live4/t18.sh`, the binary `run/live18/kpf-live18.exe`): the scripts as the
repository has them (genesis `c4187df4919288f31c491e1f8140f3b67334a8d602fe6312be07531607b319c1`, 3,964 bytes), and an
executor running `exec-run --do match,dispute,settle,late,lapse` with a price file of its own. Three of four scenes
ran.

1. **A take answered at once**, as in run 17: u stated a take of 7.79104476 KAS at 10.5 (what it takes whatever the
   committee's price within the tolerance of its own: `pos-take`'s default now), the members signed, s wrote the
   note, and u answered with a citation: within the rule, applied.
2. **The executor applied a take.** u stated a take at 10.8; the executor read the statement on the chain, judged it
   by its own price, and after the dispute period applied it (`2614073a3e9c620877e88be22460b74c54b45dd5880ae404235d8118b431e4e9`).
3. **The executor disputed a take.** u stated a take at 11.2 while the price was 10.8: "not within the rule at any
   price near this executor's own" (`81f69397a6a84e47251325093f8c91cd196f61547ed6128f7dde98f54b145164`).
4. **Not done: the answer of that dispute with a citation.** The committee's epoch had ended since the run began
   and the script did not roll the committee again, so no epoch table served the statement's time and no note was
   written. Nobody answered; after the dispute period the executor ended its own dispute, and both bonds returned
   (`1de825aa6042067b62a1cbded953596eeb257e098ac3a35a51045ece3c3f1045`): the path of a committee that does not
   speak, which is what it was. The refutation by a citation ran in run 17 with the statement script of before.

| What | Bytes | Fee (KAS) | Id |
|---|---|---|---|
| a take stated, shown whole | 2,140 | 0.00534 | `669434cb9825d54273b2183f810d189629a959c3727507aca84f32ab4d04a9f3` |
| a note written by s | 5,771 | 0.011641 | `d15dcf739158ffc4d4eb519de80b2dd9819d5f69e2b983b2b86763babd8f4ae5` |
| the take answered at once: within | 10,127 | 0.020254 | `aebcccfa2732918cc586a92221dd4f36e697fa7283276782ca1765e961d1d740` |
| x: `take 7ede5200 applied` | 2,190 | 0.00511 | `2614073a3e9c620877e88be22460b74c54b45dd5880ae404235d8118b431e4e9` |
| x: `take c293d3b0 disputed` | 2,271 | 0.004542 | `81f69397a6a84e47251325093f8c91cd196f61547ed6128f7dde98f54b145164` |
| x: `disputed take 81f69397 ended: nobody showed the committee's prices` | 2,375 | 0.005195 | `1de825aa6042067b62a1cbded953596eeb257e098ac3a35a51045ece3c3f1045` |

The executor sent 4 transactions (the match among them); the node refused none. Left on testnet: u long 4 against s
from 10.8, and the two libraries of runs 17 and 18 (about 168 KAS each in their scripts).

