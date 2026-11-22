# KPF — Kaspa Perpetual Futures

Decentralized derivatives on Kaspa.

Research prototype. Testnet only.

## Status — read this first

- **Not audited. Not for mainnet.** Nothing here is deployed on mainnet, and the tools refuse mainnet. Do not put real
  funds behind these contracts.
- **Known open findings.** The contracts are still under adversarial review (`docs/audit/`). The latest review of the
  oracle-market family (review 12) reports unfixed findings, including one that makes a false price claim profitable
  when nobody contests it; that family is kept as it was reviewed. The committee family that replaces its price source
  has had one review round (`REVIEW-C1.md`) and its fixes; nothing in it slashes a member that signs a false price yet.
  The review reports describe attacks in detail; they are published so that the design can be checked, not because the
  flaws are fixed.
- **No operator.** There is no hosted service, no admin key and no upgrade path. The author runs no executor,
  underwriter or judge stake. Anything on testnet-10 is a test.
- **Moving target.** Contract templates, parameters and transaction shapes change between commits; objects created by
  one commit are not guaranteed to be usable with the next.
- **No warranty.** See `LICENSE`.

Where to start: `docs/DESIGN-COMMITTEE.md` (markets priced by a staked committee that signs where a claim is disputed: the current direction),
`docs/DESIGN.md` (the pair as a price range, book-evidence markets), `docs/DESIGN-ORACLE-2.md` and
`docs/DESIGN-ORACLE-IMPL.md` (markets priced by an underwriter, superseded), `docs/THREAT_MODEL.md`, `docs/TESTNET.md`
(runbook), `docs/LIVE-1.md` to `docs/LIVE-4.md` (what ran on testnet-10).

## Building

KPF reuses KOB's covenant toolchain by path, from a pinned clone that is not committed:

```sh
git clone https://github.com/RossKU/kob vendor/kob
git -C vendor/kob checkout ed16148   # detached; treat vendor/kob as read-only
```

- Compiler: KOB's vendored SilverScript v1.0.0 (`vendor/kob/vendor/silverscript/`, see `vendor/kob/contracts/silverc.lock`).
- Engine: rusty-kaspa `v2.1.0` (`kaspa-consensus-core`, `kaspa-txscript`), the release KOB pins. `Cargo.lock` starts from
  KOB's lock file at that commit, so cargo fetches the same versions.
- Toolchain: Rust 1.94.0 (`rust-toolchain.toml`).

Always keep the build small: two jobs, debug profile, one cargo command at a time.

```sh
CARGO_BUILD_JOBS=2 cargo build -p kpf-cli --jobs 2        # the command line: target/debug/kpf
CARGO_BUILD_JOBS=2 cargo test --workspace --jobs 2        # every test: engine tests, library, artifacts, CLI
CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2       # only the engine tests (contracts under contracts/)
```

Compiled contracts are pinned. `contracts/legacy/artifacts/` holds the compiler output of `KpfPair`, `KpfOpen`, `KpfPending` and `KpfLock`, a manifest
(source and artifact SHA-256, template hashes and cuts, the KOB evidence templates the pair is built with) and
`SHA256SUMS`. `cargo test -p kpf-artifacts` recompiles the sources with the vendored compiler and fails if one byte
differs. After any change to a contract regenerate and commit the result:

```sh
CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write     # rewrites contracts/legacy/artifacts/*
CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- check     # compares a fresh compile with the committed files
```

The command line works offline (keys, addresses, inspection, quotes, every transaction builder with local verification).
It contacts a node only with `--node <ws-url>` and submits only with `--submit`; it refuses mainnet. The first live
exercise on testnet-10 is described in `docs/TESTNET.md`.

## Layout

| Path | What |
|---|---|
| `contracts/*.sil` | **the family being built** (the thin form, `docs/DESIGN-COMMITTEE.md` 8a): `KpfSlot`, the one script of every object (the hash of its state and the id of a library of scripts), and one script an operation under that library's id. A position between two users: `KpfTOpen`, `KpfTAdd`, a close stated by one side at a price of its own (`KpfTClose`, `KpfTSettle`, `KpfTLate`), by both (`KpfTBoth`), its dispute and the committee's answer (`KpfTDispute`, `KpfTProve`); the liquidation (`KpfTLiq`, `KpfTLiqSettle`, `KpfTCure`, `KpfTLiqDispute`, `KpfTLiqProve`); the exit from a committee that stopped signing (`KpfTExit`, `KpfTExitEnd`, `KpfTExitRefute`). Orders between users under a registry, one covenant id for every order and position of it (`KpfTPlace`, `KpfTMatch`, `KpfTCancel`). Positions rearranged without a price: two of the same two users into one (`KpfTMerge`), a user out of two that cancel each other (`KpfTNovate`), two markets into one position (`KpfTJoin`), one side written the other way round, added to, or sold (`KpfTTurn`, `KpfTFold`, `KpfTLeave`). The committee's income: 0.002 KAS an operation left in the copy of the script it used, gathered in the library's fee pool and paid to the members by weight (`KpfTSweep`, `KpfFeePool`, `KpfTPay`). Pinned under `contracts/artifacts/`; the model is `crates/kpf-protocol/src/thin.rs` (scripts in `thin_artifacts.rs`), builders in `thin_build.rs`, commands in `crates/kpf-cli/src/tcmds.rs` (`lib-create`, `lib-check`, `lib-status`, `registry-create`, `registry-sync`, `order-place`, `order-match`, `order-cancel`, `order-expire`, `pos-open`, `pos-show`, `pos-add-margin`, `pos-close` (`--mutual` for both), `pos-settle`, `pos-late`, `pos-dispute`, `pos-prove`, `pos-refute`, `pos-lapse`, `pos-liquidate`, `pos-cure`, `pos-exit`, `pos-merge`, `pos-novate`, `pos-join`, `pos-turn`, `pos-fold`, `pos-leave`, `fee-sweep`, `fee-pay`). Each user's key stays on its own machine: scripts and anchors are found at the node, and what two users sign goes between them as a file (`--partial`, `cosign`). An executor is a loop (`exec-run`: orders that cross matched, positions merged, a user taken out of two that cancel, late claims ended, claims disputed, disputes answered with the committee's signatures, fees swept; `--do watch` is a user's own watchtower) and a user learns what became of its order from the chain (`registry-sync`); what either asks of a node is measured in `docs/LIVE-4.md`, eleventh part. **Prices:** nobody is sent a price. A claim states its claimer's own, and the committee (the legacy `KpfCommittee` and `KpfEpochTable`, until the family has its own) signs only where a claim is disputed: proved, refuted, or void if nobody answers (`docs/DESIGN-COMMITTEE.md`, "The committee answers when asked"; on testnet in `docs/LIVE-4.md`, thirteenth part). Reviewed by property tests (`docs/audit/REVIEW-T1b.md`); the review of the last rule is under way. Names were changed all at once on 22 November 2026 (account to position, roster to committee, seat to member, ticket to epoch table, tick to signed price, book to registry, dues and purse to oracle fees and fee pool, quiet to exit, declare to close): records written before that day keep the old names. |
| `contracts/legacy/`, `crates/kpf-protocol/src/legacy/`, `crates/kpf-cli/src/legacy/` | the three earlier families, which work and are tested and which every command of `kpf` still drives; the rows below. The crate roots re-export the modules under their old paths |
| `contracts/legacy/KpfPair.sil`, `KpfOpen.sil`, `KpfPending.sil`, `KpfLock.sil` | the four covenants: the pair, the open order, the pending part of a claim on price evidence (a second template under the pair's covenant id, `docs/DESIGN.md` 4.5d), and the guarantor's lock of a guaranteed fast settlement (4.5f); `probes/` are experiments |
| `contracts/legacy/KpfBond.sil`, `contracts/legacy/stubs/` | oracle-only markets, first slice: the underwriting slot and stand-ins of the pair and order that bind it (`docs/DESIGN-ORACLE-IMPL.md`; compiled from source by the tests, not pinned yet); model in `crates/kpf-protocol/src/legacy/bond.rs` |
| `contracts/legacy/KpfStake.sil`, `contracts/legacy/stubs/KpfClaimStub.sil` | oracle-only markets, second slice: the stake judge (free and unbonding stake, receipts, dispute, resolved pool) and a stand-in claim it judges (`docs/DESIGN-ORACLE-IMPL.md` 8-14; compiled from source by the tests, not pinned yet); model in `crates/kpf-protocol/src/legacy/judge.rs` |
| `contracts/legacy/KpfCommittee.sil`, `KpfEpochTable.sil`, `KpfExit.sil` | committee markets: the committee of staked members that signs prices, one epoch's copy of its table (the verifier of a price), and a stake on its way out (`docs/DESIGN-COMMITTEE.md` 1, 2); model in `crates/kpf-protocol/src/legacy/committee.rs` |
| `contracts/legacy/KpfOpenC.sil`, `KpfPairC.sil`, `KpfPendingC.sil` | committee markets: the order, the pair and the pending part of a claim at a price an epoch table attests (`docs/DESIGN-COMMITTEE.md` 3); model in `cmarket.rs`, builders in `committee_build.rs`, commands `committee-*`, `price-*`, `c-*` |
| `contracts/legacy/artifacts/` | pinned compiled artifacts, manifest, `SHA256SUMS` (`oracle/` and `committee/` have their own) |
| `crates/kpf-protocol/` | library: pinned artifacts, state codecs (pair, order, pending part), payoff math, transaction builders (including the two-step price-evidence paths with KOB orders and the token program: propose a close or a liquidation, challenge with counter-evidence, cure, finalize), KOB order codecs and the evidence and counter-evidence checks, signing, fee/mass, engine verification, in-memory chain (`sim`), test fixtures |
| `crates/kpf-artifacts/` | regenerates and checks the pinned artifacts (the only crate that links the compiler) |
| `crates/kpf-cli/` | the `kpf` binary and its integration tests (scripted lifecycle against an in-memory UTXO set) |
| `crates/kpf-tests/` | engine tests: the three contracts and the KOB evidence paths in the rusty-kaspa script engine |
| `docs/DESIGN.md`, `THREAT_MODEL.md`, `audit/` | design, threat model, reviews |
| `docs/DESIGN-TWO-STEP.md` | the design note of the two-step evidence mechanism (implemented as `DESIGN.md` 4.5d) |
| `docs/DESIGN-ORACLE.md`, `DESIGN-ORACLE-2.md` | design notes on fast settlement backed by locked KAS (its book-backed part implemented as `DESIGN.md` 4.5f, guaranteed fast settlement) and on markets without an order book (not implemented) |
| `docs/DESIGN-COMMITTEE.md` | committee markets: design, what is built, the stage-2 design (slashing, pools, fees) |
| `docs/TESTNET.md` | runbook of the first live exercise on testnet-10 |
| `vendor/kob/` | read-only pinned clone of KOB (compiler, KOB contracts); not committed |
