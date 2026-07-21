# Contributing

Thanks for contributing to rust-service-starter. This guide is the **binding process** for
the repository — how to work in it, the commit and branching rules, and how
changes reach a release. It applies to human contributors **and to AI
coding agents**: if a change you are asked to make conflicts with this
document, surface the conflict instead of silently breaking the rule.

> Design notes and the guided tour of the codebase live in
> [`ARCHITECTURE.md`](./ARCHITECTURE.md) — read it before writing code.
> Condensed agent guidance lives in [`CLAUDE.md`](./CLAUDE.md).

## Prerequisites

- The **Rust toolchain** is pinned in `rust-toolchain.toml` (channel,
  clippy, rustfmt) — rustup provisions it automatically on first `cargo`
  invocation. The same version is enforced in `Cargo.toml` (`rust-version`),
  `clippy.toml` (`msrv`) and the `Dockerfile` (`ARG RUST_VERSION`): the four
  must stay in sync.
- [just](https://github.com/casey/just) — every workflow goes through the
  `justfile`.
- **Docker** with compose — the local PostgreSQL.
- [lefthook](https://lefthook.dev) and
  [committed](https://github.com/crate-ci/committed) — only if you install
  the git hooks (recommended, see below).

## Getting started

```bash
just db-up       # start PostgreSQL (docker compose) and wait until healthy
just run         # config -> database -> migrations -> API + cron engine
just hooks       # install the git hooks (lefthook) — once after cloning
just ci          # the full quality gate: fmt-check + clippy -D warnings + tests
```

`just` alone lists every recipe. `check`, `fmt`, `lint` and `test` are the
fast inner-loop commands; `just ci` is the gate CI enforces on every pull
request — **run it before every commit; a change that does not pass the
gate is not finished.**

## Where code goes

The project is a Cargo workspace (resolver 3). Every crate inherits its
metadata, dependencies and lints from the root `Cargo.toml`.

| Crate        | Role                                                                 |
| ------------ | -------------------------------------------------------------------- |
| `.` (`rust-service-starter`) | Binary entry point: boot sequence only, no business logic.           |
| `api`        | HTTP layer: routing, handlers, DTOs, errors, OpenAPI, server.        |
| `common`     | Shared building blocks: configuration, telemetry, infrastructure.    |
| `entity`     | Database entities (sea-orm models) — the single home for data models. |
| `cron`       | Cron jobs (tokio-cron-scheduler): one job per file, hard-coded schedules. |
| `migration`  | Schema migrations and their standalone CLI.                          |

### The `api` crate

| Module        | Responsibility                                                       |
| ------------- | -------------------------------------------------------------------- |
| `dto/`        | Wire format: request payloads and response bodies.                   |
| `error.rs`    | The single `ApiError` type; every handler returns `ApiResult<T>`.    |
| `extract.rs`  | Crate-local extractors (`Json`, `Path`) rejecting through `ApiError`. |
| `handler/`    | Business handlers only: extract input, call a service, map the result. |
| `middleware/` | Cross-cutting layers: one module per concern, composed in `middleware::apply` via `tower::ServiceBuilder`. |
| `router/`     | The only place where URLs are declared; also hosts the technical endpoints (health probes, 404 fallback) and the OpenAPI document (`ApiDoc` + Swagger UI). |
| `server.rs`   | HTTP server bootstrap (`Server::new(addr, conn).run()`).             |
| `service/`    | Business logic, split between `Query` (reads) and `Mutation` (writes). |
| `state.rs`    | `AppState`, the dependencies shared with every handler.              |

### Hard rules

- Handlers never touch the database directly: they go through `service/`.
- New failure modes become variants of `ApiError` (one match arm each),
  never ad-hoc status codes.
- The API speaks **JSON only**: every response body, success or failure,
  health probes included. Failures use the single `{ "error": ... }`
  envelope. Handlers take their inputs through `crate::extract::{Json,
  Path}`, never the stock axum extractors.
- Every endpoint is part of the OpenAPI contract: annotate the handler with
  `#[utoipa::path]` and register it in `ApiDoc`'s `paths(...)`
  (`router/mod.rs`). Swagger UI is on `/docs`, the document on
  `/api-docs/openapi.json`; an endpoint missing from `ApiDoc` counts as
  undocumented.
- Health endpoints (`/livez`, `/readyz`, `/healthz`) follow the Kubernetes
  probe conventions, stay at the root outside any versioned prefix, and are
  defined in `router/`, not `handler/`.
- Middlewares do one thing each and are registered in `middleware::apply`
  (ServiceBuilder, top-to-bottom execution) — never scattered
  `Router::layer` calls.
- **Configuration** is modelled in `common/src/config.rs` as typed structs;
  every key has a default, overridden by the optional
  `/etc/rust-service-starter/app-config.json`, an optional local `app-config.json` (never
  committed), then `APP_*` environment variables (`__` separator). Secrets
  are `secrecy::SecretString`, read with `expose_secret()` only at the
  single point of final consumption. Document every variable in
  `.env.example`.
- **Database entities** go to the `entity` crate, re-exported in
  `entity/src/prelude.rs`; the `api` crate defines no entity of its own.
- **Migrations**: one module `mYYYYMMDD_NNNNNN_<label>` in
  `migration/src/source/`, registered chronologically in
  `Migrator::migrations`. Never edit, reorder or delete a shipped
  migration: append a new one.
- **Cron jobs**: a unit struct implementing the `Job` trait in
  `cron/src/job/`, added to `job::roster()`. Schedules are hard-coded —
  they are behavior, reviewed in code, not runtime configuration.

## Code standards

Enforced by the toolchain — CI and `just ci` fail otherwise:

- **No unsafe code.** `unsafe_code = "forbid"` is set workspace-wide. If a
  dependency seems to require `unsafe`, find another design.
- **Everything is documented, in English.** `missing_docs` and
  `clippy::missing_docs_in_private_items` warn, and lints are errors in the
  gate. Comments explain *why*, not *what*.
- **Formatting and lints are non-negotiable.** `cargo fmt`
  (`rustfmt.toml`) and `cargo clippy --workspace --all-targets` with zero
  warnings (`clippy.toml`, MSRV 1.97).
- **Dependencies** are declared once in `[workspace.dependencies]` and
  inherited with `workspace = true`; features are added at the crate that
  needs them. Stable versions only — no release candidates.

## Branching model

- `main` is the **only long-lived branch** and the release branch: land
  changes via pull request, never push directly.
- Branch off `main` for every change: `<type>/<short-topic>` where
  `<type>` is the Conventional Commit type (`feat/subnet-allocation`,
  `fix/readyz-timeout`, `ci/cache-key`).
- Keep branches focused and short-lived; delete them after the merge (the
  repository does it automatically).

## Commit conventions

Commits **must** follow
[Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).
This is not stylistic:
[release-please](https://github.com/googleapis/release-please) derives the
next version and the changelog directly from the commit types, and the
`commit-msg` hook rejects non-conforming messages locally.

```
<type>(<optional scope>): <description>

[optional body]

[optional footer(s)]
```

| Type       | Use for                                               | Release effect |
| ---------- | ----------------------------------------------------- | -------------- |
| `feat`     | A new user-facing capability                          | minor bump     |
| `fix`      | A bug fix                                             | patch bump     |
| `docs`     | Documentation only                                    | none           |
| `refactor` | Code change that neither fixes nor adds behavior      | none           |
| `perf`     | A performance improvement                             | none           |
| `test`     | Adding or fixing tests                                | none           |
| `build`    | Build system, dependencies, packaging                 | none           |
| `ci`       | CI/CD configuration                                   | none           |
| `chore`    | Maintenance that fits nothing above                   | none           |

Rules that trip people up:

- Description in the **imperative mood, lower case, no trailing period**.
- Scope = the crate touched (`api`, `common`, `cron`, `entity`,
  `migration`) or a meaningful area (`config`, `router`, `deps`, ...).
- **Breaking changes**: `!` after the type/scope **and** a
  `BREAKING CHANGE:` footer explaining the migration path → major bump.

```
feat(api): add subnet allocation endpoint
fix(common): honour APP_DEBUG when RUST_LOG is unset
feat(migration)!: split address table

BREAKING CHANGE: the `address` table is now `subnet_address`;
re-run migrations from a clean database or apply the new revision.
```

## Git hooks

Installed by `just hooks` (lefthook, definitions in `lefthook.yml`),
tiered by cost:

- **`pre-commit`** → `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo check`, in parallel, skipped when no Rust file is staged.
- **`commit-msg`** → [committed](https://github.com/crate-ci/committed)
  validates the Conventional Commit format (rules in `committed.toml`).
- **`pre-push`** → the test suite (`cargo test --workspace`).

Do not disable hooks. `git commit --no-verify` exists for genuine
emergencies only — a malformed message will still break the release
pipeline later, in everyone's face instead of yours.

## Pull requests

1. Push your branch and open a PR against `main`.
2. The `ci` workflow runs the quality gate (`just ci`) on the
   organization's self-hosted runners. It must pass.
3. Keep the PR **title** Conventional-Commit-shaped: the repository
   squash-merges, so the title becomes the commit that release-please
   reads on `main`.
4. Merge by **squash** once the gate is green; the branch is deleted
   automatically.

Dependabot PRs follow the same flow: minor/patch updates are auto-merged
once checks pass (`ci-update` workflow); majors wait for a human.

## Release & deployment

Versioning is automated, **publishing the image is a deliberate action**:

1. On every merge to `main`, release-please opens or updates a **release
   PR** accumulating changes and the computed version (all six crate
   versions and `Cargo.lock` bump in lockstep).
2. Merging that release PR tags `vX.Y.Z`, publishes the GitHub release and
   updates `CHANGELOG.md` — nothing is deployed.
3. The `release` workflow then builds the hardened image and pushes it to
   Docker Hub (`aartintelligent/rust-service-starter`) with the `X.Y.Z`, `X.Y` and `latest`
   tags. Releases created by the default `GITHUB_TOKEN` do not trigger it
   automatically (GitHub loop protection): run it manually —
   `gh workflow run release.yaml -f tag=vX.Y.Z` — or give release-please a
   PAT to make the chain fully automatic.

Do not bump versions or edit the changelog by hand — version state lives in
`.release-please-manifest.json` and `release-please-config.json`. Crate
versions are **literal on purpose** (`version = "x.y.z"` in every crate,
not `version.workspace = true`): release-please's rust updater can only
rewrite literal strings.

### Infrastructure prerequisites

- **Self-hosted runners** — every workflow targets `runs-on: self-hosted`
  (the organization's "default" runner group). The runners need the `gh`
  CLI (auto-merge) and a Docker daemon with BuildKit (image builds); the
  quality gate bootstraps rustup itself if missing.
- **Registry credentials** — `DHI_USERNAME` / `DHI_PASSWORD` repository
  secrets: the same Docker ID pulls the hardened base images from `dhi.io`
  and pushes to Docker Hub, so the token needs write access on the
  namespace.

## Notes for AI agents

- This file is the contract for **process**; `ARCHITECTURE.md` is the
  reference for **design**. Read both before writing code; keep every gate
  green and report failures honestly.
- Never introduce `unsafe`, undocumented items, or a dependency pinned
  outside `[workspace.dependencies]`.
- Never modify a shipped migration; always append a new one.
- Write commit messages and PR titles in Conventional Commits form so the
  release automation keeps working.
- Respect the git hooks; do not commit with `--no-verify`.
