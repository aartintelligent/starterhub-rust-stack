# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## The contract

The documentation set has three roles: `CONTRIBUTING.md` is the **binding process** (setup, branching, Conventional Commits v1.0.0 — parsed by release-please, a malformed commit message corrupts the release pipeline — hooks, release flow); `ARCHITECTURE.md` is the **codebase reference** (where code goes, hard API rules, boot/shutdown, request anatomy, extension recipes — its layout and design rules are binding); `README.md` is the front door and only routes to the other two. Read both before writing code. This file only adds commands and the condensed cross-file picture.

Branching: `main` is the only long-lived branch, never pushed directly. Work happens on `<type>/<short-topic>` branches merged back by squash through a PR whose title is a Conventional Commit.

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
just migrate-generate <name>   # scaffold a migration (CLI writes to migration/src/ — move the file into migration/src/source/ and register it in Migrator::migrations)
```

Single test: `cargo test -p <crate> <test_name>`.

The Rust version is pinned in four places that must stay in sync: `rust-toolchain.toml` (channel), root `Cargo.toml` (`rust-version`), `clippy.toml` (`msrv`), and the Dockerfile (`ARG RUST_VERSION`). Currently 1.97.

## Architecture

Cargo workspace (resolver 3) producing one binary, `starterhub-rust-stack`. All metadata, dependencies and lints are inherited from the root `Cargo.toml` (`[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`); member crates never pin versions themselves.

**Boot and shutdown (`src/main.rs`)** — the only entry point. Sequence: `Config::load()` → `telemetry::init(debug)` → `postgresql::connect` → `Migrator::up` (migrations always run at boot) → spawn the HTTP server (`api::server::Server`) and the cron engine (`cron::server::Server`) side by side. Shutdown is coordinated by a single `tokio_util::sync::CancellationToken`: a bridge task cancels it on SIGTERM/SIGINT (Debian/systemd target), and `supervise()` cancels it when either component stops for any reason, so one crash always brings the sibling down too. Components take a plain `impl Future<Output = ()>` shutdown argument (`token.clone().cancelled_owned()`) and stay signal-agnostic.

**Configuration (`common/src/config.rs`)** — typed structs (`Config`, `Server`, `Postgresql`, `PostgresqlPool`), layered: hard-coded defaults → optional `/etc/starterhub-rust-stack/app-config.json` → optional local `app-config.json` (never committed) → `APP_*` env vars with `__` as nesting separator (e.g. `APP_DATABASE__POOL__MAX_CONNECTIONS`). Secrets are `secrecy::SecretString`, exposed with `expose_secret()` only at the single point of consumption (`postgresql::connect`). `.env.example` documents every variable.

**API error flow (`api` crate)** — one `ApiError` enum (`error.rs`) is the funnel for every failure: business variants, extractor rejections (`JsonRejection`/`PathRejection` via `#[from]`), `DbErr`, `anyhow::Error`. Each variant has its own match arm (no grouped arms — house rule). `IntoResponse` emits the `{ "error": ... }` JSON envelope; 5xx messages are logged then masked as "internal server error". Handlers must use the crate-local `crate::extract::{Json, Path}` (which reject through `ApiError`), never stock axum extractors — that is what keeps *every* response JSON, including malformed-input rejections. The 404 fallback and health probes (`/livez`, `/readyz`, `/healthz`) live in `router/`, not `handler/`; panics are converted to a JSON 500 by the catch-panic middleware.

**OpenAPI (`api/src/router/mod.rs`)** — the `ApiDoc` struct (utoipa derive) lives next to the routing table on purpose: every handler wired in the router carries a `#[utoipa::path]` annotation and must be listed in `ApiDoc`'s `paths(...)`. Swagger UI is merged into the router at `/docs` (document at `/api-docs/openapi.json`); the `vendored` feature of utoipa-swagger-ui embeds the UI assets so the Docker build stays offline-reproducible.

**Middleware (`api/src/middleware/`)** — one module per concern (`request_id`, `trace`, `catch_panic`), each exposing a constructor for a tower/tower-http layer; all composed in a single `ServiceBuilder` inside `middleware::apply(router)` (top-to-bottom execution order: set request id → trace → propagate request id → catch panic). Never attach layers with scattered `Router::layer` calls. tower-http layers with unnameable closure generics are made nameable via fn-pointer type aliases (`MakeRequestSpan`, `PanicHandler`).

**Cron (`cron` crate)** — jobs are unit structs in `cron/src/job/<name>.rs` implementing the object-safe `Job` trait (`name`, hard-coded `schedule`, `async run(&self, &AppState) -> JobResult`), listed in `job::roster()`. The generic wiring (`into_cron_job`, `execute`) adapts any `Job` to tokio-cron-scheduler; never duplicate it in a job. Adding a job = one new file + one line in the roster.

**Naming collisions to know about** — several deliberate shadowings require qualified paths: `tokio_cron_scheduler::Job` vs the local `Job` trait (aliased `CronJob`); the two `Server` structs (aliased `ApiServer`/`CronServer` in `main.rs`); the `config` crate vs `common::config` (use fully-qualified `config::Config::builder()`). Database entities live only in the `entity` crate — the `api` crate has no `entity` module.

## Non-negotiables (enforced by `just ci`)

- `unsafe_code = "forbid"` workspace-wide — if a design seems to need `unsafe`, find another design.
- Everything documented in English (`missing_docs` + `clippy::missing_docs_in_private_items` warn, and clippy runs with `-D warnings`). Comments explain *why*, not *what*.
- Every API response body is JSON — including health probes and error paths.
- Never edit a shipped migration; append a new `mYYYYMMDD_NNNNNN_<label>` module in `migration/src/source/`.
