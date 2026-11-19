//! The `kpf` command line (testnet-10 research prototype). See `docs/TESTNET.md`.

pub mod args;
pub mod cmds;
pub mod evcmds;
pub mod files;
pub mod keys;
pub mod node;
pub mod ocmds;
pub mod report;

use args::Args;

pub const HELP: &str = "\
kpf - Kaspa Perpetual Futures command line (TESTNET-10 RESEARCH PROTOTYPE; mainnet is refused)

Offline first: every command works on files. A node is used only with --node <ws://host:port> (no default node).
Nothing is submitted without --submit. Every transaction is verified locally in the script engine first.

Keys and inspection
  keygen   [--name N | --out FILE] [--show-secret]        new key file (default ~/.kpf/keys/N.json, never in a git repo)
  address  --key FILE | --pubkey HEX|ADDR|FILE | --object FILE | --redeem HEX
  inspect  OBJECT.json | --redeem HEX [--json]            decode any object, book market or oracle market
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
order is filled in every evidence transaction; the funder is the executor: it BUYS the evidence ask's tokens with its own
KAS, or SELLS its own tokens into the evidence bid. common flags as the builders)
  evidence  --order ORDER.json (--pair P.json | --pending W.json) [--daa N | --node URL] [--json]
            can this KOB order serve as evidence for a pair, or refute a pending claim, and why not
  propose-close --pair P.json --side long|short [--n T] --evidence ORDER.json [--seller TOKEN.json] [--evidence-fill T] --key OWNER [--offer KAS]
            P5 unilateral close (a long at a resting bid, a short at a resting ask; the owner's key signs); settles at finalize
  propose-liquidate --pair P.json [--pair P.json]... [--n T]... --evidence ORDER.json [--seller TOKEN.json] [--proposer KEY|HEX|ADDR] [--offer KAS]
            P6, up to 8 pairs on one evidence (an ask: longs, a bid: shorts); at finalize the winner keeps both sides
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
            disputed, the time left in the window, which orders dispute them, and the exact command
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
            DAA, done resting within the window): the wronged party is paid the whole cover, the funder keeps the reward
            (lambda - 1) x cover, the rest is burned
  release   --lock L.json [--daa N]                      anyone, once the lock is disputeDaa old: the lock to its owner

Oracle-only markets (docs/DESIGN-ORACLE-IMPL.md, third slice; runbook docs/TESTNET.md. Prices come only from each pair's
underwriter; a false price stands unless contested within the window. common flags as the builders; --daa N or --node
gives the present the lock time proves)
  oracle-market --feed FEED.txt [--out M.json] [--rho-bps N] [--window-daa N] [--quiet-daa N] [--late-daa N]
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
                --any-slot lets any slot of the market fill it (prints the consequence); an order resting longer than a
                term can be filled into a lapsed cover (margin back at the entry, tip, carrier and premium half lost)
  oracle-open   --long O.json --short O.json --slot S.json --n T --x P --cover-rate P [--premium P] [--expiry DAA] --key UW
                [--uw-utxo UTXO.json]   the premium joins the funder's change when the funder is the underwriter, else one
                output with that underwriter UTXO; a premium too small for an output of its own is refused otherwise
  vouch         --pair P.json --kind liquidate|close --side long|short --price P [--t0 DAA] [--n T] --vouch UTXO.json --key UW
                [--key CLOSER]   the underwriter's price claim (a close is signed by the closer too); for a
                liquidation the vouch UTXO funds the bond (and the new part's carrier when --n < size)
  contest       --pending W.json --stake ST.json --key STAKER [--base KAS]   within the window, with aged stake on NO
                with --node: --claim-tx TX.json (the claim's birth, traced to a KpfPairO.propose); without: --accept-untraced
  deposit       --dispute D.json --side yes|no --stake ST.json... --key OWNER...   join the trailing side
                with --node: --opening TX.json --claim-tx TX.json (the dispute traced to its opening and its claim
                to a propose; refused if the trace fails); without --node: --accept-untraced (prints what is unchecked)
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
             most the leaver's bl, a short's at least its bs, healthy at x, within the cover rate, leaving the stayer its half of the cover from the entry; checked before building)
  oracle-mutual-close --pair P.json --x P                            both sides listed and crossing: anyone closes at x
  oracle-coop-close --pair P.json --long-pay KAS --key LONG --key SHORT   both owners sign; the short's output pays the fee
  (a dry run writes its files under OUT-DIR/dry-run/; only submitted transactions write at the top of OUT-DIR.
   With --node, every object file's UTXO and DAA score are read from the node, whatever the file says.
   A transaction with a relative lock or a lock time prints `usable from DAA X`; with --node, --submit before X is
   refused locally)
  oracle-watch  --me KEY|HEX|ADDR (--object FILE | --scan DIR)... [--daa N | --node URL] [--json]
            claims against you and the time left to contest, your underwriter's cover expiry, the disputes you have
            stake in

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
