//! The `kpf` command line (testnet-10 research prototype). See `docs/TESTNET.md`.

pub mod args;
pub mod legacy;
pub use legacy::ccmds;
pub use legacy::cmds;
pub use legacy::evcmds;
pub use legacy::ocmds;
pub mod files;
pub mod keys;
pub mod node;
pub mod report;
pub mod tcmds;
mod acmds;
mod ncmds;
pub mod xcmds;

use args::Args;

pub const HELP: &str = "\
kpf - Kaspa Perpetual Futures command line (TESTNET-10 RESEARCH PROTOTYPE; mainnet is refused)

Offline first: every command works on files. A node is used only with --node <ws://host:port> (no default node).
Nothing is submitted without --submit. Every transaction is verified locally in the script engine first.

Positions and declared claims (the thin family, the main one: docs/DESIGN-COMMITTEE.md 8a)
  lib-create       [--copies 2] [--value 10] [--committee ID]  a library: copies of each script under one covenant id.
                   Every operation leaves 0.002 KAS in the copy it used; with --committee the library has a fee_pool
                   they gather in for that committee's members (fee-sweep, fee-pay); without, they stay in the copies
  fee-sweep       --lib GENESIS-tx.json --node URL --funder KEY [--copy NAME]   what gathered in a copy of a script
                   (0.2 KAS at min_fill above what a copy keeps) to the library's fee_pool; the sender keeps a twentieth
  fee-pay         --lib GENESIS-tx.json --node URL --funder KEY --epoch-table T.json --signed-price FILE...   the fee_pool paid to
                   every member of the committee by its weight (the table of the epoch table shown); the sender keeps a twentieth
  registry-create      --lib GENESIS-tx.json [--copies 4] [--value 1]   a registry of the library: anchors under one covenant
                   id. An order (order-place) or a position (pos-open) enters through one (--anchor FILE --script
                   KpfTPlace-FILE (a position: KpfTOpen-FILE) --registry REGISTRY-tx.json, checked; or --registry-unchecked) with its owner's signature (a position: both parties'), and
                   every order and position of the library is under that id: none is one nobody signed
  lib-check        --lib GENESIS-tx.json [--registry REGISTRY-tx.json]   whether a genesis wrote the family's scripts (a registry's:
                   anchors) and nothing else
  lib-check        --lib GENESIS-tx.json                   is this a library a position can be opened in: every output
                   under its id one of the family's scripts (one more, and every object in it is anybody's)
  lib-status       --lib GENESIS-tx.json [--registry REGISTRY-tx.json] --node URL   the live copies of each script and the
                   registry's anchors at the node this moment (one query)
  registry-sync        --dir DIR --lib GENESIS-tx.json --registry REGISTRY-tx.json --node URL [--only KEY|ADDRESS]
                   [--market1 M.json --dispute_daa 600 --bond-bps 1000 --exit-daa 864000]   the folder DIR is brought to the
                   present: if no copy of a script and no anchor moved since its last reading, nothing happened in
                   the registry and nothing is read (one list asked of the node); else the transactions that moved them are
                   fetched, the files of the orders and positions they made (what a match made of your order, what a
                   merge or a novation made of your position) are written with nobody sending them, and those of what
                   was spent go to DIR/spent. The first time it begins at this moment; after half an hour away it min_counters
                   --catch-up (the node is asked for the ids of what the chain accepted since) or --from-now. A
                   template (--market1 and the three values) lets it read orders of that market before any was matched
  exec-run         --dir DIR --lib GENESIS-tx.json --registry REGISTRY-tx.json --node URL --funder KEY [--submit]
                   [--do match,novate,merge,late,lapse,sweep | all (adds dispute,settle,expire) | guard]
                   (guard, for a user's own positions: --signed-prices DIR --epoch-table FILE [--silence DAA] [--only KEY]: when the
                   committee stops signing, a position worth more than its margin is claimed at the last signed
                   price; a disputed claim of the key is proved; --max-exposure R: no match above R x the stake) [--every 20] [--rounds N]
                   [--max-tx 4] [--min-gain KAS] [--expiry-guard 100] [--market1 M.json ...]
                   an executor's loop over the registry: each round it min_counters the node three small things (the present, one
                   list of UTXOs), fetches the registry's transactions only if a script's copy or an anchor moved (somebody
                   traded), and sends what leaves it --min-gain richer after the fee: orders that cross matched at the limit of the
                   one that stood first, a user taken out of two positions that cancel, two positions of the same users
                   merged, a late claim ended. One coin of --funder a transaction: keep several. Without --submit a
                   round shows what it would send; without --node (--daa N --funding FILE) one round runs on DIR as it is
  coins            --count 4 --each KAS --funder KEY   the funder's coins as so many coins (exec-run: a transaction a coin)
  send             --to ADDR|KEY --amount KAS --funder KEY [--text NOTE]   plain KAS to a key; the note is the
                   transaction's payload, which anyone reads
  pos-open        --lib GENESIS-tx.json (checked; or --lib ID --lib-unchecked) --market1 M.json --size1 N --dir1 long|short --x1 P [--market2 ... --x2 P] --a A --b B
                   [--a-min_counter KAS --b-min_counter KAS]: what each min_counters of whoever takes the other's place (default: what the other posts)
                   --a-margin KAS --b-margin KAS [--dispute_daa 600] [--bond-bps 1000] [--exit-daa 864000]
                   [--b-funding UTXOS.json] [--private]
  pos-show        --object FILE [--node URL] [--lib GENESIS-tx.json --registry REGISTRY-tx.json]   a position or a claim; for a
                   claim, whether its amount is the rule's. A file somebody else gave you (the position a match made of
                   your order): with --node it is read at the node, and with --registry it is refused unless it is in
                   that registry
  pos-add-margin  --position FILE --script FILE --side a|b --amount KAS
  pos-close     --position FILE --script FILE --side a|b --t0 DAA --p1 P [--p2 P] [--amount KAS]
  pos-settle      --claim FILE --script FILE              anyone, once the claim is a dispute_daa old and nobody disputed it
  pos-dispute     --claim FILE --script FILE [--disputer KEY]   a bond as large as the claim's: its claimer must prove it
  pos-lapse       --claim FILE --script FILE              anyone, once a disputed claim is a dispute_daa old and unproved
  pos-late        --claim FILE --script FILE              a claim that landed later than lateDaa after its time, or
                   before it: the position again, its bond shared by the funder and the party it was made against
  pos-close --mutual  --position FILE --script FILE --to-a KAS [--to-b KAS] --key A --key B
  pos-liquidate   --position FILE --script FILE --side a|b --t0 DAA --p1 P [--p2 P] [--fee KAS --add KAS] [--liquidator KEY]
  pos-cure        --claim FILE --script FILE              the side a liquidation names adds what it min_counters (its key signs)
  (pos-settle, pos-dispute, pos-lapse and pos-late take a liquidation as --claim too, each with its own script)
  pos-prove       --claim FILE --script FILE --epoch-table1 T.json --signed-price1 TICK.json... [--epoch-table2 T.json --signed-price2 ...]
                   the claimer of a disputed claim or liquidation answers with the committee's signatures
  (pos-close and pos-liquidate take --epoch-table1 --signed-price1 ... in place of --t0 --p1 --p2: the committee's signed price)
  order-place      --lib GENESIS-tx.json --market1 M.json --side long|short --size N --limit P --margin KAS
                   [--tip KAS] --expiry DAA [--min_fill N] [--min_counter KAS] [--dispute_daa 600] [--bond-bps 1000] [--exit-daa 864000] [--no-rest]
  order-match      --long FILE --short FILE --script FILE --size N --price P   two orders of one
                   market and the same values become a position between their owners; what is left of an order
                   rests (--no-rest at its placing: taken whole or not at all); the tips' shares are the sender's
  order-cancel     --order FILE --script FILE              the owner takes its order back
  order-expire     --order FILE --script FILE              anyone, from the order's expiry on
  pos-merge       --position FILE --with FILE --script FILE   anyone: two positions of the same two parties (same
                   market, terms, sides and registry) become one; the sender keeps 0.01 KAS of what they held
  pos-novate      --position FILE --with FILE --script FILE   anyone: a user long in one position and short in the
                   other (one registry) leaves; the two it faced face each other; no price, no signature; it is paid its
                   margins and what the price moved between the two entries (less 0.015 KAS, the sender's)
  pos-join        --position FILE --with FILE --script FILE --key A --key B   two positions of the same two users in
                   two markets (one registry) become one position of two legs, one pot of margin for both; both sign
                   (it is not left through a sale afterwards: it ends by a close of both markets, or by both)
  pos-fold        --position FILE --with FILE --script FILE   anyone: a position of the same two users (one leg) is
                   folded into the first leg of --position (one leg or two): added, or set against it
  pos-leave       --position FILE --with FILE --script FILE --key A --key B   the first leg of a position of two
                   legs goes to the third user of --with (the position's A sold it there); the position's two users
                   sign, the third does not; the seller's margin in the position stays behind the leg it keeps
  pos-turn        --position FILE --script FILE --mode users|legs   anyone: the same position written the other way
                   round (for pos-leave and pos-fold, which read the first leg and the position's A)
  pos-exit       --position FILE --script FILE --side a|b [--since DAA]   the committee signed no price since:
                   settled at the entries after the position's exit time (pos-settle), unless ended by
  pos-refute      --claim FILE --script FILE --epoch-table1 T.json --signed-price1 TICK.json... [--epoch-table2 ... --signed-price2 ...]
  (--script: a copy of the command's script in the object's library, a file `lib-create` or the last use of it wrote.
   With --node it may be left out: a live copy is found at the node, as is an anchor when --anchor is left out and
   --registry is given. Another machine has no file of the copy somebody else's transaction wrote last.
   Two machines: what two users sign (pos-open, pos-close --mutual, pos-join, pos-leave) is written with
   `--partial FILE` by whoever has one of the keys (its own with --key; --funder may be another party's address, with
   its coin from the node or --funding), and signed by each of the others in turn:
     cosign --tx FILE --lib GENESIS-tx.json [--registry REGISTRY-tx.json] --key YOURS --node URL (--submit | --out NEXT-FILE)
   which prints what the transaction does as read from the transaction, looks its inputs up at the node, signs, and
   sends it when nothing is left open (else writes the file for the next signer).
   The committee's fee is not built. Everything below is the earlier families.)

Keys and inspection
  keygen   [--name N | --out FILE] [--show-secret]        new key file (default ~/.kpf/keys/N.json, never in a git repo)
  address  --key FILE | --pubkey HEX|ADDR|FILE | --object FILE | --redeem HEX
  inspect  OBJECT.json | --redeem HEX [--json]            decode any object, registry market or oracle market
  quote    --pair OBJECT.json [--prices 9,10,11 | --from P --to P --steps N] [--n TOKENS] [--json]
  market   [--idle-daa N] [--carrier KAS] [--scale N] [--min-size N] | --check FILE   example market block / validate one

Builders (common: --funder KEY --funding UTXOS.json | --node URL, [--key KEY]... --fee-rate N --priority
          --out-dir DIR --brief --no-save --submit)
  create-order  --market M.json --side long|short --size T --limit P --rate P --expiry DAA
                [--owner KEY|HEX|ADDR] [--min-counter-rate P] [--min-fill T] [--tip P] [--active-from DAA]
  cancel        --order O.json --key OWNER [--to ADDR]
  refund        --order O.json
  open          --long O.json --short O.json --n T --x P            order x order
  open-taker    --order O.json --n T --x P --rate P                 order x signing taker (the funder)
  amend         --pair P.json --side long|short (--price P [--min-part T] [--tip P] | --unlist) --key OWNER
  add-margin    --pair P.json --side long|short --delta P --key OWNER
  transfer      --pair P.json --side long|short --n T --x P (--entrant-order O.json | --rate P) [--k 0|1] [--daa N]
                allowed from DAA S + 4 * minRestDaa (S: the pair's evidence clock start, shown by the command)
  close-vs-close --host P.json --donor P.json --n T --x P [--e3 P] [--daa N]   (both pairs' hand-over limits apply)
  mutual-close  --pair P.json --n T --x P
  coop-close    --pair P.json --long-pay KAS [--fee-from long|short] --key LONG --key SHORT [--funder KEY]
  idle-settle   --pair P.json

Price-evidence paths, two steps (DESIGN 4.5d: a claim is proposed with a bond, challenged, cured or finalized; a real KOB
order is filled in every evidence transaction; the funder is the executor: it BUYS the evidence min_counter's tokens with its own
KAS, or SELLS its own tokens into the evidence bid. common flags as the builders)
  evidence  --order ORDER.json (--pair P.json | --pending W.json) [--daa N | --node URL] [--json]
            can this KOB order serve as evidence for a pair, or refute a pending claim, and why not
  propose-close --pair P.json --side long|short [--n T] --evidence ORDER.json [--seller TOKEN.json] [--evidence-fill T] --key OWNER [--offer KAS]
            P5 unilateral close (a long at a resting bid, a short at a resting min_counter; the owner's key signs); settles at finalize
  propose-liquidate --pair P.json [--pair P.json]... [--n T]... --evidence ORDER.json [--seller TOKEN.json] [--proposer KEY|HEX|ADDR] [--offer KAS]
            P6, up to 8 pairs on one evidence (a min_counter: longs, a bid: shorts); at finalize the winner keeps both sides
            (forfeit and keep) and the proposer (default the funder) receives the bond and the fee
            (--offer: what the proposer gives up from its own payout to a guarantor that fast-settles the claim, at most
             the bond less 0.02 KAS; it rides in the guarantor's lock)
  challenge --pending W.json [--pending W.json]... --evidence ORDER.json [--seller TOKEN.json] [--pair P.json]...
            refute pending claims with a counter order of the opposite kind beyond the claim by the margin g, rested from
            the proposal's DAA; the parts return to their pairs (unlisted, the clock kept) and the bonds go to the funder
  cure      --pending W.json --key LOSER [--pair P.json] the threatened side of a pending liquidation adds the margin it
                                                         needs (bound past the claim by g) and repays the proposer
  finalize  --pending W.json [--daa N] [--pair P.json]   anyone, once the pending part is challengeDaa old: settle the claim
            (--pair: the pair the part was cut from; the part must carry its covenant id and keys. Every pending part
             must hold its range and refund, or it is refused as a look-alike)
  watch     --pair P.json [--pair ...] (--pending W.json | --lock L.json | --scan DIR)... [--order ORDER.json]... [--node URL | --daa N] [--json]
            pending claims against these pairs, the time left before they can be finalized, which of the given resting
            orders refute them, and the exact command to do so; and the fast-settled claims against them that can still be
            disputed, the time left in the dispute_daa, which orders dispute them, and the exact command
  kob-file  --redeem HEX --outpoint TXID:INDEX (--node URL | --amount SOMPI --daa N --covenant-id HEX) [--custody FILE] [--out FILE]
            an ORDER.json / TOKEN.json from a KOB order's or a token holder's redeem script
  (--daa N: the DAA the time checks use when there is no --node; a transaction whose lock time is ahead of the node's DAA
   is not submitted. Evidence counts only from the pair's evidence clock start S (counter-evidence from the proposal's DAA)
   and must stay fillable for the market's evidence life after its rest; a dead (soft-expired) order is refused;
   --allow-dead-evidence accepts one, for sweeping tests only)

Guaranteed fast settlement (DESIGN 4.5f; optional, the slow path above stays the default. common flags as the builders)
  guarantee --pending W.json [--owner KEY|HEX|ADDR] [--pair P.json] [--daa N]
            settle a pending claim now, exactly as finalize would, by locking lambda x its cover plus the proposer's offer
            (owner: default the funder; a liquidation once its pending part is graceDaa old, the loser's cure grace)
  dispute   --lock L.json [--lock L.json]... --evidence ORDER.json [--seller TOKEN.json]
            refute a fast-settled claim with the counter-evidence a challenge would have needed (rested from the claim's
            DAA, done resting within the dispute_daa): the wronged party is paid the whole cover, the funder keeps the reward
            (lambda - 1) x cover, the rest is burned
  release   --lock L.json [--daa N]                      anyone, once the lock is disputeDaa old: the lock to its owner

Oracle-only markets (docs/DESIGN-ORACLE-IMPL.md, third slice; runregistry docs/TESTNET.md. Prices come only from each pair's
underwriter; a false price stands unless contested within the dispute_daa. common flags as the builders; --daa N or --node
gives the present the lock time proves)
  oracle-market --feed-template FEED.txt    writes a feed definition template (the judge's two-moment rule in it)
  oracle-market --feed FEED.txt [--out M.json] [--rho-bps N] [--dispute_daa-daa N] [--exit-daa N] [--late-daa N]
                [--sigma-bps N] [--beta-bps N] [--bond-min KAS] [--mmr-bps N] [--g-bps N] [--idle-daa N] ...
            a market block from a feed definition (testnet defaults, each overridable): its id and the worst cases
  oracle-check  --market M.json --feed FEED.txt [--size T --x P --long-rate P --short-rate P --cover-rate P [--premium P]]
            the wallet's check: the feed text's hash and the id are the block's, the bounds hold, the text renders
            plainly; the parameters and the worst-case loss in words. Refuses a market that fails
  bond          --market M.json --value KAS [--owner KEY|HEX|ADDR]   a new underwriting slot (rho x value of cover)
  unbond        --slot S.json --key OWNER                            once every cover's liability has ended
  withdraw-bond --slot S.json --key OWNER [--to ADDR]                an unbonding slot, U old
  stake         --market M.json --value KAS [--owner KEY|HEX|ADDR]   a judge stake (counts on claims made M_age later)
  oracle-order  --market M.json --side long|short --size T --limit P --rate P --expiry DAA [--prem-max P]
                (--slot SLOT_ID | --any-slot) [--min-counter-rate P] [--min-fill T] [--tip P] [--active-from DAA] [--escrow KAS]
                --any-slot lets any slot of the market fill it (prints the consequence); --min-fill defaults to the whole
                size; every part must hold at min_fill 0.2 KAS of the order's margin (as must what it leaves), and only the
                part that empties the order pays a carrier; an order resting longer than a term can be filled into a
                lapsed cover (margin back at the entry; tip, one carrier and the first premium half lost, whole order)
  oracle-open   --long O.json --short O.json --slot S.json --n T --x P --cover-rate P [--premium P] [--expiry DAA] --key UW
                [--uw-utxo UTXO.json]   the premium joins the funder's change when the funder is the underwriter, else one
                output with that underwriter UTXO; a premium too small for an output of its own is refused otherwise
  vouch         --pair P.json --kind liquidate --side long|short --price P [--t0 DAA] [--n T] --vouch UTXO.json --key UW
                [--dry-run]   the underwriter's liquidation claim: t0 is the node's DAA (a --t0 older than lateDaa is
                refused), signed and submitted in one step with --node; the vouch UTXO funds the bond (and the new part's
                carrier when --n < size); refused when the claim would land later than the dispute_daa and one step before
                the expiry (void: the bond comes back) unless --void-ok. Keep the vouch funds on a key of their own
  oracle-close  --pair P.json --side long|short --price P [--n T] [--t0 DAA] --key PARTY [--dry-run]
                a party's own close at the feed: its own bond, no underwriter and no counterparty; settles W after it
                lands at the price moved g against the closer; refused below the closer's maintenance; any time
  contest       --pending W.json --stake ST.json --key STAKER [--base KAS]   within the dispute_daa, with aged stake on NO
                with --node: --claim-tx TX.json (the claim's birth, traced to a KpfPairO.propose); without: --accept-untraced
                contest a claim whose price is not within g of the feed at its t0 or at P (the DAA it landed at)
  deposit       --dispute D.json --side yes|no --stake ST.json... --key OWNER...   join the trailing side
                with --node: --opening TX.json --claim-tx TX.json (the dispute traced to its opening and its claim
                to a propose; refused if the trace fails); without --node: --accept-untraced (prints what is unchecked)
                vote YES only for a claim within g of the feed at both t0 and P, NO otherwise
  resolve       --dispute D.json --pending W.json                    after the round: the verdict reaches the part
  oracle-cure   --pending W.json --key LOSER                         the loser clears a pending liquidation
  oracle-finalize --pending W.json                                   settle, or restore a refuted or void claim
  renew         --pair P.json... --slot S.json --key UW [--uw-utxo UTXO.json]   up to 7 pairs, from one step before expiry; only
                a pair UTXO accepted no later than its expiry (a pair a party touched after it lapses)
  lapse-settle  --pair P.json                                        a lapsed, idle pair at its entry price
  settle-pool   --pool D.json [--receipt R.json]...                  pay the winners (no receipt: close the pool)
                forfeits the losers: give the loser's receipt together with the winner's
  stake-unbond  --stake ST.json --key OWNER                          a free stake (e.g. returned by settle-pool) starts leaving
  stake-withdraw --stake ST.json --key OWNER [--to ADDR]             an unbonding stake, unbondDaa old, to its owner
  oracle-amend  --pair P.json --side long|short (--price P [--min-part T] [--tip P] | --unlist) --key OWNER
            list (or unlist) a side for a hand-over; the pair's UTXO must be quietDaa old
  oracle-transfer --pair P.json --side long|short --x P --nb P [--entrant KEY|HEX|ADDR]   a listed side changes hands
            (the whole side, at x within its listing; the entrant, default the funder, posts the bound nb: a long's at
             most the leaver's bl, a short's at min_fill its bs, healthy at x, within the cover rate, leaving the stayer its half of the cover from the entry; checked before building)
  oracle-mutual-close --pair P.json --x P                            both sides listed and crossing: anyone closes at x
  oracle-coop-close --pair P.json --long-pay KAS --key LONG --key SHORT   both owners sign; the short's output pays the fee
  (a dry run writes its files under OUT-DIR/dry-run/; only submitted transactions write at the top of OUT-DIR.
   With --node, every object file's UTXO and DAA score are read from the node, whatever the file says.
   A transaction with a relative lock or a lock time prints `usable from DAA X`; with --node, --submit before X is
   refused locally)
  oracle-watch  --me KEY|HEX|ADDR (--object FILE | --scan DIR)... [--daa N | --node URL] [--json]
                [--feed-price P [--feed-at-t0 P]]
            claims against you and the time left to contest, your underwriter's cover expiry, the disputes you have
            stake in. With --feed-price (the feed now) it is an automated watcher's check: a pending claim within its
            dispute_daa that is not within g of the feed (at t0 and now) is marked CONTEST NOW with its deadline
            (--json: contestNow, contestUntil) and the command exits with code 3. With a dispute_daa of minutes only such
            a watcher can contest: run one (your own or a service)

Committee markets (docs/DESIGN-COMMITTEE.md. Nobody states a price: a committee of staked members signs signed prices, a price needs
the signatures of its quorum (67 % of the stake by default) and is their median, and without them nothing settles.
common flags as the builders; a command with --node makes one query per object file, one for the funder, and none polls)
 The committee (anyone may bond KAS and sign)
  committee-create [--epoch-daa N] [--exit-daa N] [--min-member-stake KAS] [--quorum-bps N] [--min-total KAS] [--epoch-tables N]
                [--epoch-table-value KAS] [--from DAA] [--out COMMITTEE.json]   a new, empty committee; COMMITTEE.json holds its id and
                the genesis facts the id is recomputed from
  committee-check  --committee COMMITTEE.json [--object R.json]       the wallet's check: the id is that of one empty committee
  committee-show   --committee R.json [--daa N | --node URL]       the members, their shares, what it takes to outbid each
  committee-join   --committee R.json --genesis COMMITTEE.json --member I --amount KAS [--owner KEY|HEX|ADDR]   an empty member,
                or the min_fill member of a full table for a twentieth more than its stake; the member signs from the next
                roll. --genesis: the facts that show the committee began empty (or --unchecked)
  committee-topup  --committee R.json --genesis COMMITTEE.json --member I --amount KAS      at min_fill 1 KAS
  committee-leave  --committee R.json --member I --key MEMBER          the stake becomes an exit, released after the epoch and
                the exit time
  committee-roll   --committee R.json [--steps M] [--wait]         anyone: start the epoch that holds the present and write
                its epoch tables (one transaction an epoch; until someone rolls, no price can be attested)
  epoch-table-retire --epoch-table T.json                              an epoch table a whole epoch after its end; its value to the funder
  exit-withdraw --exit E.json                                anyone: a released exit to its key
  member-forfeit  --committee R.json [--member I] --signed-price A.json --signed-price B.json   anyone: a member that signed two prices for one
                signed price is slashed with the two signed signed prices; a tenth of its stake to the sender, the rest burned
  exit-forfeit  --exit E.json --signed-price A.json --signed-price B.json    the same proof against a stake on its way out
  charge        (--committee R.json [--member I] | --exit E.json) --signed-price T.json [--pay-to KEY|HEX|ADDR]
                anyone, with the committee's bond: the stake's key signed a FALSE price (the signed signed price is the
                evidence that it signed; the judge decides whether it is false). The stake goes on signing and is
                held: not withdrawn, wherever it goes, until the charge is settled
  judge-stake   --committee COMMITTEE.json --value KAS [--owner KEY|HEX|ADDR]   a stake of the committee's judge (a pool apart
                from the members'); it judges charges accepted ageDaa after it
  charge-contest --charge C.json --stake ST.json --key STAKER [--base KAS]   aged judge stake on NO (the signed price is
                false), within chargeDaa of the charge. A contest nobody answers within a round is a verdict
  charge-answer --dispute D.json --charge C.json --side yes|no --stake ST.json... --key OWNER...
                (--committee R.json [--member I] | --exit E.json) [--before HEX --later ID,ID]
                judge stake joins the trailing side; the stake given shows the charge to be the committee's own.
                A member charged for a true signed price answers on YES (it needs judge stake aged before the charge)
  charge-decide --charge C.json --dispute D.json             after the round: the verdict reaches the charge
  charge-settle --charge C.json (--committee R.json [--member I] | --exit E.json) [--before HEX --later ID,ID]
                refuted: the stake is slashed by up to the total the NO side had at risk (a tenth of that and the
                bond to the charge's author, the rest burned);
                upheld: the hold ends, the bond to the charged key; never contested, or contested late: the hold
                ends, the bond is burned. The judge's pool is paid out with `settle-pool`
  hold-witness  --ids ID,ID,... --of ID                      for a stake held for several charges: --before and
                --later of one of them, from the ids of its unsettled charges in the order they were made
 Signed prices (what a member signs: its own price for one DAA of one market)
  member-run      --market M.json --key MEMBER (--price-file F | --price-cmd CMD | --price P) --node URL [--dir TICKS]
                [--grid DAA] [--count N | --once] [--sync-secs S] [--keep N]
                a member signs unattended: every --grid DAA (default 100) the feed's price for the time at the last
                whole multiple of the grid, into --dir (default kpf-out/signed-prices). Members on one grid sign the same
                signed prices without talking. The node is asked once a --sync-secs (default 60); when the price source
                fails nothing is signed
  price-sign     --market M.json --price P (--t0 DAA | --node URL) --key MEMBER [--out TICK.json]
                one price a signed price: what the key signed is kept in KEYFILE.signed.jsonl, and a second price for a
                signed price it holds is refused (it is proof against the member: `member-forfeit`)
  price-check    --market M.json --epoch-table T.json --signed-price TICK.json... [--t0 DAA]   the price a set of signed prices attests
 Markets
  c-market      --committee COMMITTEE.json --feed FEED.txt [--out M.json] [--term-daa N] [--idle-daa N] [--grace-daa N]
                [--late-daa N] [--exit-daa N] [--mmr-bps N] [--g-bps N] [--liq-fee-bps N] [--keeper-fee-min KAS] ...
  c-check       --market M.json [--feed FEED.txt]            the wallet's check: the id, the feed text, the bounds, that
                the market's committee began empty and that its spread leaves the tolerance g a margin. Refuses a market
                that fails; warns of parameters that make a poor one
  c-order       --market M.json --side long|short --size T --limit P --rate P --expiry DAA [--owner KEY|HEX|ADDR]
                [--min-counter-rate P] [--min-fill T] [--tip P] [--active-from DAA] [--escrow KAS]
  c-open        --long O.json --short O.json --n T --x P [--lease DAA] [--market M.json]   order x order
  c-open-taker  --order O.json --n T --x P --rate P [--lease DAA] [--market M.json]   order x signing taker (the funder)
                (--market, here and on c-assign, c-transfer, c-take: the market file you checked; an object of
                 another market is refused)
  c-cancel      --order O.json --key OWNER
  c-refund      --order O.json                               anyone, once the order has expired; the sender keeps
                keeperFeeMin of the escrow. An expiry does not stop a fill: it binds when a refund lands
 Claims (the price is the one the signed prices attest on an epoch table of the market's committee; a claim must land within lateDaa
 of its time or it is void and the part is restored)
  c-close       --pair P.json --side long|short --epoch-table T.json --signed-price TICK.json... [--n T] --key OWNER
                a party's exit at the attested price moved by g against it; settles at once with c-finalize
  c-liquidate   --pair P.json --side long|short --epoch-table T.json --signed-price TICK.json... [--n T] [--proposer KEY|HEX|ADDR]
                anyone: the side the attested price puts below its maintenance; the fee goes to the proposer
  c-trim        --pair P.json --side long|short --epoch-table T.json --signed-price TICK.json... (--bound P | --take KAS | --max)
                --key OWNER   cross margin: a side takes out the margin it has above maintenance at the attested
                price; its bound moves inward, the size and the other side's bound stay. A claim like a close:
                c-finalize pays it at once, and it is void if it landed more than lateDaa after its time. Then
                c-add-margin on another pair
  c-position     --me KEY (--object F.json... | --scan DIR...) (--price P | --price-of MARKET=P...) [--alert-bps N]
                [--target-bps N] [--node URL]   the key's positions as one position: worth, liquidation line, the
                margin c-trim frees; names a position near its liquidation and the positions whose free margin covers
                it (exit code 3)
  c-margin-run   --key YOU --dir POSITIONS --node URL --submit [--signed-prices DIR] [--epoch-tables DIR] [--alert-bps N]
                [--target-bps N] [--every-secs S] [--count N | --once]
                cross margin, unattended: the positions of the key in --dir are watched at the newest attested signed price;
                one near its liquidation gets margin from the wallet or, by c-trim, from the positions that have it
                to spare; a pending liquidation is cured, a pending trim finalized. One query per position a round
                (default every 30 s). While it runs the positions are linked; when it stops each stands alone
  c-finalize    --pending W.json                             anyone: settle (a liquidation after graceDaa), or restore
  x-swap        --pair1 P.json --side1 long|short --x1 P [--pair2 P.json --side2 long|short --x2 P] --b-margin KAS
                [--a-margin KAS] --key OWNER --funder EXECUTOR
                cross margin: the owner's side of one or two pairs becomes one position against the executor, in one
                transaction. The executor takes each pair over as it is and funds the position; in it the owner holds
                both positions and one margin, and is liquidated on their total. --x1/--x2: the agreed prices
  cosign        --tx PARTIAL.json --key YOURS [--market M.json...] [--node URL --submit]
                signing on two machines. Any builder with `--partial FILE --funder <the other party's address>` (and
                --node or --funding for its UTXOs) writes the transaction signed by the keys at hand and leaves the
                other party's plain inputs open; that party runs cosign, which prints what the transaction does as
                read from the transaction itself, signs its inputs and sends it. The last signer decides
  x-open        --market1 M.json --size1 N --dir1 long|short --x1 P [--market2 .. --size2 .. --dir2 .. --x2 ..]
                --a KEY --b KEY --a-margin KAS --b-margin KAS      a new position (no pairs before it)
  x-show        --position X.json [--price1 P --price2 P]     what each party is owed, who can be liquidated, what a
                close pays
  x-close       --position X.json --side a|b --epoch-table1 T --signed-price1 F... [--epoch-table2 T --signed-price2 F...] --key OWNER
                a party's exit at the attested prices of both legs for one time; x-finalize settles at once
  x-liquidate   --position X.json --side a|b --epoch-table1 T --signed-price1 F... [--epoch-table2 T --signed-price2 F...] [--proposer K]
                anyone: the side is below the maintenance on the total of its legs
  x-finalize    --pending W.json        anyone: settle (a liquidation after graceDaa), or restore a void claim
  x-cure        --pending W.json --key LOSER                 the loser of a pending liquidation adds the margin it needs
  x-add-margin  --position X.json --side a|b --amount KAS --key OWNER
  x-keep-alive  --position X.json [--leg 1|2] --epoch-tableN T --tickN F...   renew the lease with a signed price of a leg's market
  x-lapse-settle --position X.json       anyone, after the lease and idleDaa: the entries
  x-coop-close  --position X.json --a-pay KAS --key A --key B --node URL --submit   both parties, any split
  c-cure        --pending W.json --key LOSER                 the loser clears a pending liquidation
 The lease and the exits without a price
  c-keep-alive  --pair P.json --epoch-table T.json --signed-price TICK.json...   anyone: any attested signed price renews the pair's lease
  c-lapse-settle --pair P.json                               anyone: a pair nobody kept alive, at its entry price
  c-assign      --pair P.json --side long|short --x P [--n T] [--nb P] [--entrant KEY|HEX|ADDR] --key LEAVER
                the leaver signs its side over to the entrant (default the funder) and is paid in the same
                transaction: an executor's instant exit
  c-amend       --pair P.json --side long|short (--price P [--min-part T] [--tip P] | --unlist) --key OWNER
  c-transfer    --pair P.json --side long|short --x P [--n T] [--nb P] [--entrant KEY|HEX|ADDR]   a listed side
  c-take        --pair P.json --order O.json --x P [--n T]   a resting order takes a listed side
  c-add-margin  --pair P.json --side long|short --delta P --key OWNER
  c-mutual-close --pair P.json --x P                         both sides listed and crossing
  c-coop-close  --pair P.json --long-pay KAS --key LONG --key SHORT
 Reading
  c-watch       [--me KEY|HEX|ADDR] (--object FILE | --scan DIR)... [--daa N | --node URL] [--price P]
                pairs with their lease and, with --price (the feed now), each side's value and whether it can be
                liquidated; pending claims and when they can be finalized; exits, epoch tables and the committee's epoch.
                Exit code 3 when something needs an action now
  c-quote       --pair P.json [--prices 9,10,11]             each side's value at given prices
  (inspect FILE decodes every object of this family too)

Node (needs --node)
  utxos   --key FILE | --address A [--out FILE]      list UTXOs (a funding file)
  status  --object FILE... [--update]                is the object live; fill in its DAA
  daa     --node URL                                 the node's network and current DAA score
  verify  --tx TX.json                               re-verify a built transaction offline
  submit  --tx TX.json --node URL                    send a built transaction (verified locally first)

Units: prices, rates and KAS amounts are decimal KAS (or <n>s for exact sompi); sizes are decimal whole tokens of the
market's scale (or <n>s for base units). Object files are written by every builder (--out-dir, default ./kpf-out).
";

/// Runs one invocation (without the program name); prints to stdout.
pub fn run(argv: &[String]) -> Result<(), String> {
    let a = Args::parse(argv)?;
    if a.has("help") || a.command.is_empty() || a.command == "help" {
        print!("{HELP}");
        return Ok(());
    }
    match a.command.as_str() {
        "keygen" => cmds::keygen(&a),
        "address" => cmds::address(&a),
        "inspect" => cmds::inspect(&a),
        "quote" => cmds::quote(&a),
        "market" => cmds::market(&a),
        "create-order" => cmds::create_order_cmd(&a),
        "cancel" => cmds::cancel_cmd(&a),
        "refund" => cmds::refund_cmd(&a),
        "open" => cmds::open_cmd(&a),
        "open-taker" => cmds::open_taker_cmd(&a),
        "amend" => cmds::amend_cmd(&a),
        "add-margin" => cmds::add_margin_cmd(&a),
        "transfer" => cmds::transfer_cmd(&a),
        "close-vs-close" => cmds::close_vs_close_cmd(&a),
        "mutual-close" => cmds::mutual_close_cmd(&a),
        "coop-close" => cmds::coop_close_cmd(&a),
        "idle-settle" => cmds::idle_settle_cmd(&a),
        "propose-close" => evcmds::propose_close_cmd(&a),
        "propose-liquidate" => evcmds::propose_liquidate_cmd(&a),
        "challenge" => evcmds::challenge_cmd(&a),
        "cure" => evcmds::cure_cmd(&a),
        "finalize" => evcmds::finalize_cmd(&a),
        "watch" => evcmds::watch_cmd(&a),
        "guarantee" => evcmds::guarantee_cmd(&a),
        "dispute" => evcmds::dispute_cmd(&a),
        "release" => evcmds::release_cmd(&a),

        "oracle-market" => ocmds::market_cmd(&a),
        "oracle-check" => ocmds::check_cmd(&a),
        "bond" => ocmds::bond_cmd(&a),
        "unbond" => ocmds::unbond_cmd(&a),
        "withdraw-bond" => ocmds::withdraw_bond_cmd(&a),
        "stake" => ocmds::stake_cmd(&a),
        "oracle-order" => ocmds::order_cmd(&a),
        "oracle-open" => ocmds::open_cmd(&a),
        "vouch" => ocmds::vouch_cmd(&a),
        "oracle-close" => ocmds::oracle_close_cmd(&a),
        "contest" => ocmds::contest_cmd(&a),
        "deposit" => ocmds::deposit_cmd(&a),
        "resolve" => ocmds::resolve_cmd(&a),
        "oracle-cure" => ocmds::cure_cmd(&a),
        "oracle-finalize" => ocmds::finalize_cmd(&a),
        "renew" => ocmds::renew_cmd(&a),
        "lapse-settle" => ocmds::lapse_cmd(&a),
        "settle-pool" => ocmds::settle_pool_cmd(&a),
        "stake-unbond" => ocmds::stake_unbond_cmd(&a),
        "stake-withdraw" => ocmds::stake_withdraw_cmd(&a),
        "oracle-amend" => ocmds::oracle_amend_cmd(&a),
        "oracle-transfer" => ocmds::oracle_transfer_cmd(&a),
        "oracle-mutual-close" => ocmds::oracle_mutual_close_cmd(&a),
        "oracle-coop-close" => ocmds::oracle_coop_close_cmd(&a),
        "oracle-watch" => ocmds::watch_cmd(&a),
        "committee-create" => ccmds::committee_create_cmd(&a),
        "committee-check" => ccmds::committee_check_cmd(&a),
        "committee-show" => ccmds::committee_show_cmd(&a),
        "committee-join" => ccmds::committee_join_cmd(&a),
        "committee-topup" => ccmds::committee_topup_cmd(&a),
        "committee-leave" => ccmds::committee_leave_cmd(&a),
        "committee-roll" => ccmds::committee_roll_cmd(&a),
        "epoch-table-retire" => ccmds::epoch_table_retire_cmd(&a),
        "exit-withdraw" => ccmds::exit_withdraw_cmd(&a),
        "member-run" => ccmds::member_run_cmd(&a),
        "member-forfeit" => ccmds::member_forfeit_cmd(&a),
        "exit-forfeit" => ccmds::exit_forfeit_cmd(&a),
        "charge" => ccmds::charge_cmd(&a),
        "judge-stake" => ccmds::judge_stake_cmd(&a),
        "charge-contest" => ccmds::charge_contest_cmd(&a),
        "charge-answer" => ccmds::charge_answer_cmd(&a),
        "charge-decide" => ccmds::charge_decide_cmd(&a),
        "charge-settle" => ccmds::charge_settle_cmd(&a),
        "hold-witness" => ccmds::hold_witness_cmd(&a),
        "price-sign" => ccmds::price_sign_cmd(&a),
        "price-check" => ccmds::price_check_cmd(&a),
        "c-market" => ccmds::market_cmd(&a),
        "c-check" => ccmds::check_cmd(&a),
        "c-order" => ccmds::order_cmd(&a),
        "c-open" => ccmds::open_cmd(&a),
        "c-open-taker" => ccmds::open_taker_cmd(&a),
        "c-cancel" => ccmds::cancel_cmd(&a),
        "c-refund" => ccmds::refund_cmd(&a),
        "c-close" => ccmds::close_cmd(&a),
        "c-liquidate" => ccmds::liquidate_cmd(&a),
        "c-trim" => ccmds::trim_cmd(&a),
        "c-account" => ccmds::account_cmd(&a),
        "c-margin-run" => ccmds::cross_run_cmd(&a),
        "c-finalize" => ccmds::finalize_cmd(&a),
        "c-cure" => ccmds::cure_cmd(&a),
        "c-keep-alive" => ccmds::keep_alive_cmd(&a),
        "c-lapse-settle" => ccmds::lapse_cmd(&a),
        "c-amend" => ccmds::amend_cmd(&a),
        "c-add-margin" => ccmds::add_margin_cmd(&a),
        "c-assign" => ccmds::assign_cmd(&a),
        "c-transfer" => ccmds::transfer_cmd(&a),
        "c-take" => ccmds::take_cmd(&a),
        "c-mutual-close" => ccmds::mutual_close_cmd(&a),
        "c-coop-close" => ccmds::coop_close_cmd(&a),
        // A partly signed transaction of the thin family is read by its own `cosign`; of the earlier ones, by theirs.
        "cosign" => match a.get("tx").map(files::read_json) {
            Some(Ok(v)) if v["format"].as_str() == Some(tcmds::THIN_PARTIAL) => tcmds::cosign_cmd(&a, &v),
            _ => ocmds::cosign_cmd(&a),
        },
        "lib-status" => tcmds::lib_status_cmd(&a),
        "registry-sync" => xcmds::registry_sync_cmd(&a),
        "exec-run" => xcmds::exec_run_cmd(&a),
        "coins" => xcmds::coins_cmd(&a),
        "send" => xcmds::send_cmd(&a),
        "probe-seq-commit" => xcmds::probe_seq_cmd(&a),
        "node-call" => xcmds::node_call_cmd(&a),
        "probe-cite" => xcmds::probe_cite_cmd(&a),
        "note-write" => ncmds::note_write_cmd(&a),
        "note-cite" => ncmds::note_cite_cmd(&a),
        "note-reclaim" => ncmds::note_reclaim_cmd(&a),
        "pos-take" => ncmds::take_cmd(&a),
        "change-settle" => ncmds::change_settle_cmd(&a),
        "change-dispute" => ncmds::change_dispute_cmd(&a),
        "change-lapse" => ncmds::change_lapse_cmd(&a),
        "change-late" => ncmds::change_late_cmd(&a),
        "take-answer" => ncmds::take_answer_cmd(&a),
        "acct-show" => acmds::show_cmd(&a),
        "shard-open" => acmds::shard_open_cmd(&a),
        "shard-fund" => acmds::shard_fund_cmd(&a),
        "shard-draw" => acmds::shard_draw_cmd(&a),
        "shard-note" => acmds::shard_note_cmd(&a),
        "acct-open" => acmds::open_cmd(&a),
        "acct-leg" => acmds::leg_cmd(&a),
        "acct-add" => acmds::add_cmd(&a),
        "acct-take" => acmds::take_cmd(&a),
        "acct-leave" => acmds::leave_cmd(&a),
        "acct-liq" => acmds::liq_cmd(&a),
        "acct-end" => acmds::end_cmd(&a),
        "stated-settle" => acmds::settle_cmd(&a),
        "stated-dispute" => acmds::dispute_cmd(&a),
        "stated-lapse" => acmds::lapse_cmd(&a),
        "stated-answer" => acmds::answer_cmd(&a),
        "draw-note" => acmds::draw_note_cmd(&a),
        "fee-sweep" => tcmds::fee_sweep_cmd(&a),
        "fee-pay" => tcmds::fee_pay_cmd(&a),
        "lib-create" => tcmds::lib_create_cmd(&a),
        "registry-create" => tcmds::registry_create_cmd(&a),
        "lib-check" => tcmds::lib_check_cmd(&a),
        "pos-open" => tcmds::open_cmd(&a),
        "pos-show" => tcmds::show_cmd(&a),
        "pos-add-margin" => tcmds::add_cmd(&a),
        // (`--mutual`: closed by both parties' signatures, with no price shown.)
        "pos-close" if a.has("mutual") => tcmds::close_both_cmd(&a),
        "pos-close" => tcmds::close_cmd(&a),
        "pos-settle" => tcmds::settle_cmd(&a),
        "pos-late" => tcmds::late_cmd(&a),
        "pos-dispute" => tcmds::dispute_cmd(&a),
        "pos-lapse" => tcmds::lapse_cmd(&a),
        "pos-liquidate" => tcmds::liquidate_cmd(&a),
        "pos-cure" => tcmds::cure_cmd(&a),
        "pos-prove" => tcmds::prove_cmd(&a),
        "pos-exit" => tcmds::exit_cmd(&a),
        "pos-merge" => tcmds::merge_cmd(&a),
        "pos-novate" => tcmds::novate_cmd(&a),
        "pos-join" => tcmds::join_cmd(&a),
        "pos-turn" => tcmds::turn_cmd(&a),
        "pos-fold" => tcmds::fold_cmd(&a),
        "pos-leave" => tcmds::leave_cmd(&a),
        "order-place" => tcmds::order_place_cmd(&a),
        "order-match" => tcmds::order_match_cmd(&a),
        "order-cancel" => tcmds::order_cancel_cmd(&a),
        "order-expire" => tcmds::order_expire_cmd(&a),
        "pos-refute" => tcmds::refute_cmd(&a),
        "x-open" => ccmds::x_open_cmd(&a),
        "x-swap" => ccmds::x_swap_cmd(&a),
        "x-show" => ccmds::x_show_cmd(&a),
        "x-close" => ccmds::x_close_cmd(&a),
        "x-liquidate" => ccmds::x_liquidate_cmd(&a),
        "x-finalize" => ccmds::x_finalize_cmd(&a),
        "x-cure" => ccmds::x_cure_cmd(&a),
        "x-add-margin" => ccmds::x_add_margin_cmd(&a),
        "x-keep-alive" => ccmds::x_keep_alive_cmd(&a),
        "x-lapse-settle" => ccmds::x_lapse_cmd(&a),
        "x-coop-close" => ccmds::x_coop_close_cmd(&a),
        "c-watch" => ccmds::watch_cmd(&a),
        "c-quote" => ccmds::quote_cmd(&a),
        "evidence" => evcmds::evidence_cmd(&a),
        "kob-file" => evcmds::kob_file_cmd(&a),
        "verify" => cmds::verify_cmd(&a),
        "submit" => cmds::submit_cmd(&a),
        "utxos" => cmds::utxos_cmd(&a),
        "status" => cmds::status_cmd(&a),
        "daa" => cmds::daa_cmd(&a),
        other => Err(format!("unknown command `{other}` (try `kpf help`)")),
    }
}
