# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## The contract

`CONTRIBUTING.md` is the **binding process**: setup, branching, Conventional Commits v1.0.0 (parsed by release-please — a malformed commit message corrupts the release pipeline), hooks, release flow. `ARCHITECTURE.md` is the **codebase reference**: workspace layout, binding design rules, extension recipes. `README.md` only routes to them. Read both before writing code; this file adds only commands and agent-specific gotchas.

Branching: `main` is the only long-lived branch, never pushed directly. Work happens on `<type>/<short-topic>` branches, squash-merged through a PR whose title is a Conventional Commit.

## Commands

Recipes live in the `justfile` (auto-loads `.env`):

```sh
just ci                        # full quality gate: fmt-check + clippy -D warnings + tests — run before every commit
just lint                      # cargo clippy --workspace --all-targets -- -D warnings
just fmt / just fmt-check      # cargo fmt --all [--check]
just test                      # cargo test --workspace
just run                       # boot the app: config -> database -> migrations -> API + cron
just db-up / db-down / db-reset  # local PostgreSQL via docker compose (db-reset wipes the volume)
just migrate <cmd>             # sea-orm-migration CLI: up, down, status, ...
just migrate-generate <name>   # scaffold a migration (move the file into migration/src/source/ and register it in Migrator::migrations)
```

Single test: `cargo test -p <crate> <test_name>`.

The Rust version is pinned in four places that must stay in sync: `rust-toolchain.toml` (channel), root `Cargo.toml` (`rust-version`), `clippy.toml` (`msrv`), and the Dockerfile (`ARG RUST_VERSION`). Currently 1.97.

## Gotchas

- Deliberate name shadowings require qualified paths: `tokio_cron_scheduler::Job` vs the local `Job` trait (aliased `CronJob`); the two `Server` structs (aliased `ApiServer`/`CronServer` in `main.rs`); the `config` crate vs `common::config` (use fully-qualified `config::Config::builder()`).
- Database entities live only in the `entity` crate — the `api` crate has no `entity` module.
- The `vendored` feature of utoipa-swagger-ui embeds the Swagger UI assets; do not remove it — the Docker build must stay offline-reproducible.

## Non-negotiables (enforced by `just ci`)

- `unsafe_code = "forbid"` workspace-wide.
- Everything documented, in English; comments explain *why*, not *what*.
- Every API response body is JSON — including health probes and error paths.
- Never edit a shipped migration; always append a new one.
