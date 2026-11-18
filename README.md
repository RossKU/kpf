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

Run the engine tests (contracts under `contracts/`, tests under `crates/kpf-tests/tests/`):

```sh
CARGO_BUILD_JOBS=2 cargo test -p kpf-tests --jobs 2
```
