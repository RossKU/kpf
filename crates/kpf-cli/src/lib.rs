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
  transfer      --pair P.json --side long|short --n T --x P (--entrant-order O.json | --rate P) [--k 0|1]
  close-vs-close --host P.json --donor P.json --n T --x P [--e3 P]
  mutual-close  --pair P.json --n T --x P
  coop-close    --pair P.json --long-pay KAS [--fee-from long|short] --key LONG --key SHORT [--funder KEY]
  idle-settle   --pair P.json

Price-evidence paths (a real KOB order is filled in the same transaction; the funder is the executor: it BUYS the evidence
ask's tokens with its own KAS, or SELLS its own tokens into the evidence bid. common flags as the builders)
  evidence  --order ORDER.json --pair P.json [--daa N | --node URL] [--json]   can this KOB order serve as evidence, and why not
  close     --pair P.json --side long|short --n T --evidence ORDER.json [--seller TOKEN.json] [--evidence-fill T] --key OWNER
            P5 unilateral close: a long at a resting bid, a short at a resting ask (the owner's key signs)
  liquidate --pair P.json [--pair P.json]... [--n T]... --evidence ORDER.json [--seller TOKEN.json]   P6, up to 8 pairs on one evidence:
            an ask liquidates longs, a bid shorts; the loser receives nothing, the winner the part less the fee
  liquidate-transfer --pair P.json --entrant-order O.json --evidence ORDER.json [--n T] [--x P] [--k 0|1] [--seller TOKEN.json]
            P7: the loser's part is sold to a resting KpfOpen of its side; the winner keeps its position
  kob-file  --redeem HEX --outpoint TXID:INDEX (--node URL | --amount SOMPI --daa N --covenant-id HEX) [--custody FILE] [--out FILE]
            an ORDER.json / TOKEN.json from a KOB order's or a token holder's redeem script
  (--daa N: the DAA the time checks use when there is no --node; a transaction whose lock time is ahead of the node's DAA
   is not submitted)

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
        "close" => evcmds::close_cmd(&a),
        "liquidate" => evcmds::liquidate_cmd(&a),
        "liquidate-transfer" => evcmds::liquidate_transfer_cmd(&a),
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
