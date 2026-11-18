# KPF — Kaspa Perpetual Futures

Decentralized derivatives on Kaspa.

Research prototype. Testnet only.

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

Compiled contracts are pinned. `contracts/artifacts/` holds the compiler output of `KpfPair`, `KpfOpen`, `KpfPending` and `KpfLock`, a manifest
(source and artifact SHA-256, template hashes and cuts, the KOB evidence templates the pair is built with) and
`SHA256SUMS`. `cargo test -p kpf-artifacts` recompiles the sources with the vendored compiler and fails if one byte
differs. After any change to a contract regenerate and commit the result:

```sh
CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write     # rewrites contracts/artifacts/*
CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- check     # compares a fresh compile with the committed files
```

The command line works offline (keys, addresses, inspection, quotes, every transaction builder with local verification).
It contacts a node only with `--node <ws-url>` and submits only with `--submit`; it refuses mainnet. The first live
exercise on testnet-10 is described in `docs/TESTNET.md`.

## Layout

| Path | What |
|---|---|
| `contracts/KpfPair.sil`, `KpfOpen.sil`, `KpfPending.sil`, `KpfLock.sil` | the four covenants: the pair, the open order, the pending part of a claim on price evidence (a second template under the pair's covenant id, `docs/DESIGN.md` 4.5d), and the guarantor's lock of a guaranteed fast settlement (4.5f); `probes/` are experiments |
| `contracts/artifacts/` | pinned compiled artifacts, manifest, `SHA256SUMS` |
| `crates/kpf-protocol/` | library: pinned artifacts, state codecs (pair, order, pending part), payoff math, transaction builders (including the two-step price-evidence paths with KOB orders and the token program: propose a close or a liquidation, challenge with counter-evidence, cure, finalize), KOB order codecs and the evidence and counter-evidence checks, signing, fee/mass, engine verification, in-memory chain (`sim`), test fixtures |
| `crates/kpf-artifacts/` | regenerates and checks the pinned artifacts (the only crate that links the compiler) |
| `crates/kpf-cli/` | the `kpf` binary and its integration tests (scripted lifecycle against an in-memory UTXO set) |
| `crates/kpf-tests/` | engine tests: the three contracts and the KOB evidence paths in the rusty-kaspa script engine |
| `docs/DESIGN.md`, `THREAT_MODEL.md`, `audit/` | design, threat model, reviews |
| `docs/DESIGN-TWO-STEP.md` | the design note of the two-step evidence mechanism (implemented as `DESIGN.md` 4.5d) |
| `docs/DESIGN-ORACLE.md`, `DESIGN-ORACLE-2.md` | design notes on fast settlement backed by locked KAS (its book-backed part implemented as `DESIGN.md` 4.5f, guaranteed fast settlement) and on markets without an order book (not implemented) |
| `docs/TESTNET.md` | runbook of the first live exercise on testnet-10 |
| `vendor/kob/` | read-only pinned clone of KOB (compiler, KOB contracts); not committed |
