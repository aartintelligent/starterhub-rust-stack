# Contributing to IPAM

This document is the **authoritative ruleset** for evolving this project.
It is written for human developers **and for AI agents** (Claude Code and
similar): if you are an AI agent working on this repository, treat every
rule below as a hard constraint. When a change you are asked to make
conflicts with this document, surface the conflict instead of silently
breaking the rule.

## Project layout

The project is a Cargo workspace (resolver 3). Every crate inherits its
metadata, dependencies and lints from the root `Cargo.toml`
(`[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`).

| Crate        | Role                                                                |
| ------------ | ------------------------------------------------------------------- |
| `.` (`ipam`) | Binary entry point: boot sequence only, no business logic.          |
| `api`        | HTTP layer: routing, handlers, DTOs, errors, server bootstrap.      |
| `common`     | Shared building blocks: configuration, telemetry, infrastructure.   |
| `entity`     | Database entities (sea-orm models), one module per table.           |
| `cron`       | Cron jobs (tokio-cron-scheduler): one job per file, hard-coded schedules. |
| `migration`  | Schema migrations and their standalone CLI.                         |

### Where code goes — `api` crate

| Module        | Responsibility                                                      |
| ------------- | ------------------------------------------------------------------- |
| `dto/`        | Wire format: request payloads and response bodies.                  |
| `error.rs`    | The single `ApiError` type; every handler returns `ApiResult<T>`.   |
| `extract.rs`  | Crate-local extractors (`Json`, `Path`) rejecting through `ApiError`. |
| `handler/`    | Business handlers only: extract input, call a service, map the result. |
| `middleware/` | Cross-cutting layers: one module per concern, composed in `middleware::apply` via `tower::ServiceBuilder`. |
| `router/`     | The only place where URLs are declared; also hosts the technical endpoints (health probes, 404 fallback) and the OpenAPI document (`ApiDoc` + Swagger UI). |
| `server.rs`   | HTTP server bootstrap (`Server::new(addr, conn).run()`).            |
| `service/`    | Business logic, split between `Query` (reads) and `Mutation` (writes). |
| `state.rs`    | `AppState`, the dependencies shared with every handler.             |

Hard rules:

- Handlers never touch the database directly: they go through `service/`.
- New failure modes become variants of `ApiError`, never ad-hoc status codes.
- The API speaks JSON only: every response body, success or failure,
  health probes included, is JSON — never plain text. Failures (404
  fallback, extractor rejections, caught panics, probe failures) use the
  single `{ "error": ... }` envelope. Handlers take their inputs through
  `crate::extract::{Json, Path}`, never through the stock `axum::Json` /
  `axum::extract::Path`.
- Health endpoints (`/livez`, `/readyz`, `/healthz`) follow the Kubernetes
  probe conventions and stay at the root, outside any versioned prefix.
  They are defined in `router/`, not in `handler/`: `handler/` is reserved
  for business resources.
- Every endpoint is part of the OpenAPI contract: annotate its handler
  with `#[utoipa::path]` and register it in the `paths(...)` list of
  `ApiDoc` (in `router/mod.rs`). Swagger UI is served on `/docs`,
  the generated document on `/api-docs/openapi.json`; an endpoint missing
  from `ApiDoc` is invisible there and counts as undocumented.
- Middlewares do one thing each: prefer `tower-http`'s layers, use
  `axum::middleware::from_fn` for simple internal logic, and write a full
  `tower::Layer`/`Service` pair only for configurable/publishable
  middleware. Register every layer in `middleware::apply` (ServiceBuilder,
  top-to-bottom execution), never with scattered `Router::layer` calls.

### Where code goes — other crates

- **Configuration** is modelled in `common/src/config.rs` as typed structs
  (no `App` prefix). Every key has a default, overridden in order by the
  optional system file `/etc/ipam/app-config.json` (FHS path for the
  Debian 13 / Docker deployment target), an optional local
  `app-config.json` (JSON only, never committed), then `APP_*`
  environment variables with `__` as nesting separator
  (e.g. `APP_DATABASE__POOL__MAX_CONNECTIONS=50`). Settings classified as
  secrets (passwords, tokens, keys) are wrapped in `secrecy::SecretString`:
  `Debug` redacts them, and the value is read with `expose_secret()` only
  at its single point of final consumption — never stored, logged or
  passed around in clear.
- **Infrastructure helpers** (one module per external system) live in
  `common/src/infrastructure/`; they consume their own section of the
  configuration tree (e.g. `postgresql::connect(&config.database)`).
- **Telemetry** is initialised once via `common::telemetry::init(debug)`;
  `APP_DEBUG=true` selects DEBUG verbosity, `RUST_LOG` always wins.
- **Migrations** live in `migration/src/source/`, one module named
  `mYYYYMMDD_NNNNNN_<label>`, registered chronologically in
  `Migrator::migrations`. Never edit, reorder or delete a migration that
  has shipped: add a new one. Migrations run automatically at boot; the
  CLI (`cargo run -p migration -- <command>`) uses the same configuration
  as the API.
- **Database entities** go to the `entity` crate (`entity/src/`, one
  module per table), re-exported in `entity/src/prelude.rs` — the single
  home for data models; the `api` crate defines no entity of its own.
- **Cron jobs** live in `cron/src/job/`, one module per job: a unit
  struct implementing the `Job` trait (`name`, hard-coded `schedule`,
  async `run(&self, state) -> JobResult`), added to `job::roster`, the
  single list loaded at boot. Schedules are behavior: they belong to code
  review, not to runtime configuration. The cron wiring (`into_cron_job`,
  `execute`) is generic — never duplicate it in a job. Long-lived
  components (API, cron engine) are spawned side by side in `src/main.rs`
  and must accept a shutdown future so SIGTERM/SIGINT stops them
  gracefully.

## Code standards

These are enforced by the toolchain — CI and `just ci` fail otherwise:

- **No unsafe code.** `unsafe_code = "forbid"` is set workspace-wide.
  If a dependency seems to require `unsafe`, find another design.
- **Everything is documented, in English.** `missing_docs` and
  `clippy::missing_docs_in_private_items` warn, and lints are errors in the
  quality gate. Every crate, module, type, field and function carries
  rustdoc (`//!` / `///`). Comments explain *why*, not *what*: a comment
  paraphrasing the line below it is noise.
- **Formatting and lints are non-negotiable.** `cargo fmt` (see
  `rustfmt.toml`) and `cargo clippy --workspace --all-targets` with zero
  warnings (see `clippy.toml`, MSRV 1.97).
- **Dependencies** are declared once in `[workspace.dependencies]` and
  inherited with `workspace = true`. Features are added at the crate that
  needs them. Do not pin versions in member crates.

## Development workflow

### Branching model

`main` is the only long-lived branch and is protected: **nothing is
pushed to it directly**. Every change follows the same cycle:

1. Branch off `main`: `<type>/<short-topic>` where `<type>` is the
   Conventional Commit type of the change (`feat/subnet-allocation`,
   `fix/readyz-timeout`, `ci/cache-key`).
2. Open a pull request targeting `main`. The `ci` workflow runs the
   quality gate on it; the PR title must be a Conventional Commit — with
   squash merge it becomes the commit on `main`.
3. Merge by **squash** once the gate is green. Delete the branch.

Releases close the loop automatically: release-please watches `main`,
maintains the release PR, and tags when it merges — same cycle, no
manual step.

### Daily commands

Recipes are defined in the `justfile`:

```sh
just            # list recipes
just run        # config -> database -> migrations -> HTTP server
just migrate    # sea-orm-migration CLI (up, down, status, ...)
just ci         # full quality gate: fmt-check + clippy -D warnings + tests
```

Run `just ci` before every commit. A change that does not pass the gate is
not finished.

### Git hooks (optional, recommended)

`just hooks` installs the [lefthook](https://lefthook.dev) hooks defined
in `lefthook.yml`, tiered by cost: fmt/clippy/check at pre-commit (in
parallel, skipped on non-Rust commits), Conventional Commits validation
at commit-msg (via [committed](https://github.com/crate-ci/committed),
rules in `committed.toml`), and the test suite at pre-push. They are a
local convenience — the authority remains `just ci` and the CI gate.
Anything slower than pre-push belongs in CI, not in a hook.

## Commits — Conventional Commits

Commit messages **must** follow
[Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).
This is not cosmetic: [release-please](https://github.com/googleapis/release-please)
parses the history to compute the next version and generate the changelog,
so a malformed message corrupts the release pipeline.

Format:

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

- **Types**: `feat` (new capability, minor bump), `fix` (bug fix, patch
  bump), plus `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`.
- **Scope**: the crate touched (`api`, `common`, `cron`, `entity`,
  `migration`) or a meaningful area (`config`, `telemetry`, `router`, ...).
- **Breaking changes**: append `!` after the type/scope **and** add a
  `BREAKING CHANGE:` footer explaining the migration path (major bump).
- Description in the imperative mood, lower case, no trailing period.

Examples:

```
feat(api): add subnet allocation endpoint
fix(common): honour APP_DEBUG when RUST_LOG is unset
feat(migration)!: split address table

BREAKING CHANGE: the `address` table is now `subnet_address`;
re-run migrations from a clean database or apply the new revision.
```

## Notes for AI agents

- This file is the contract: read it before writing code, follow the
  layout table when deciding where a file goes, and keep every gate green.
- Never introduce `unsafe`, undocumented items, or a dependency pinned
  outside `[workspace.dependencies]`.
- Never modify a shipped migration; always append a new one.
- Always run `just ci` (or the equivalent cargo commands) before declaring
  work done, and report failures honestly.
- Write commit messages in Conventional Commits form so release automation
  keeps working.
