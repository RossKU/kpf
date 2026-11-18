//! The `kpf` command line (testnet-10 research prototype). See `docs/TESTNET.md`.

pub mod args;
pub mod cmds;
pub mod evcmds;
pub mod files;
pub mod keys;
pub mod node;
pub mod report;

use args::Args;

pub const HELP: &str = "\
kpf - Kaspa Perpetual Futures command line (TESTNET-10 RESEARCH PROTOTYPE; mainnet is refused)

Offline first: every command works on files. A node is used only with --node <ws://host:port> (no default node).
Nothing is submitted without --submit. Every transaction is verified locally in the script engine first.

Keys and inspection
  keygen   [--name N | --out FILE] [--show-secret]        new key file (default ~/.kpf/keys/N.json, never in a git repo)
  address  --key FILE | --pubkey HEX|ADDR|FILE | --object FILE | --redeem HEX
  inspect  OBJECT.json | --redeem HEX [--json]            decode a pair or an order
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
            DAA, done resting within the window): the wronged party is paid from the lock, the funder keeps the reward
            (lambda - 1) x cover, the rest is burned
  release   --lock L.json [--daa N]                      anyone, once the lock is disputeDaa old: the lock to its owner

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
