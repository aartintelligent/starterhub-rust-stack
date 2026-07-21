# Architecture

Reference for the codebase: workspace layout, design rules and extension
recipes. The rules stated here are **binding**. The contribution process
(setup, branching, commits, hooks, releases) lives in
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

## Workspace

One Cargo workspace (resolver 3) producing one binary. Every crate
inherits its metadata, dependencies and lints from the root `Cargo.toml`
(`[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`).
Dependencies are declared once at the workspace level and inherited with
`workspace = true`; stable versions only.

| Crate        | Role                                                                 |
| ------------ | -------------------------------------------------------------------- |
| `.` (`starterhub-rust-stack`) | Binary entry point: boot sequence only, no business logic.           |
| `api`        | HTTP layer: routing, handlers, DTOs, errors, OpenAPI, server.        |
| `common`     | Shared building blocks: configuration, telemetry, infrastructure.    |
| `entity`     | Database entities (sea-orm models) — the single home for data models. |
| `cron`       | Cron engine and jobs (tokio-cron-scheduler), one job per file.       |
| `migration`  | Schema migrations and their standalone CLI.                          |

### Inside the `api` crate

| Module        | Responsibility                                                       |
| ------------- | -------------------------------------------------------------------- |
| `dto/`        | Wire format: request payloads and response bodies.                   |
| `error.rs`    | The single `ApiError` type; every handler returns `ApiResult<T>`.    |
| `extract.rs`  | Crate-local extractors (`Json`, `Path`) rejecting through `ApiError`. |
| `handler/`    | Business handlers only: extract input, call a service, map the result. |
| `middleware/` | Cross-cutting layers: one module per concern, composed in `middleware::apply`. |
| `router/`     | The only place where URLs are declared; also hosts health probes, the 404 fallback and the OpenAPI document. |
| `server.rs`   | HTTP server bootstrap.                                               |
| `service/`    | Business logic, split between `Query` (reads) and `Mutation` (writes). |
| `state.rs`    | `AppState`, the dependencies shared with every handler.              |

## Boot and shutdown

`src/main.rs` is the only entry point: load configuration → init
telemetry → connect PostgreSQL → run migrations → spawn the HTTP server
and the cron engine.

Shutdown is coordinated by a single `CancellationToken`: SIGTERM/SIGINT
cancels it, and either component stopping cancels it — one crash brings
the sibling down. Components take a plain `impl Future<Output = ()>`
shutdown argument and stay signal-agnostic.

## Request flow

```text
TCP
  └─ middleware::apply — one ServiceBuilder, top-to-bottom:
       set x-request-id  →  trace span  →  propagate x-request-id  →  body cap  →  timeout  →  catch panics
         └─ router: /livez /healthz /readyz + fallback (+ /docs, /api-docs/openapi.json in local/dev)
              └─ handler (crate::extract::{Json, Path})
                   └─ service (Query / Mutation)  →  sea-orm  →  PostgreSQL
```

Rules:

- **Every response is JSON**, success or failure, health probes included.
  Failures use the single `{ "error": ... }` envelope produced by
  `ApiError` (`api/src/error.rs`); new failure modes become variants (one
  match arm each), never ad-hoc status codes.
- **Handlers use `crate::extract::{Json, Path}`**, never the stock axum
  extractors, and never touch the database: data access belongs to
  `service/`.
- **5xx bodies are opaque**: the real error is logged with the request id;
  the client sees `"internal server error"`.
- **Middlewares are declared once**: one module per concern under
  `api/src/middleware/`, composed only in `middleware::apply`
  (ServiceBuilder order = execution order).
- **Health probes** (`/livez`, `/readyz`, `/healthz`) follow Kubernetes
  conventions and are defined in `router/`, not `handler/`.
- **Every request is bounded**: past `server.timeout` (default 30 s) the
  client receives a JSON `408`, and request bodies are capped at 2 MiB
  (JSON `413`).
- **Every endpoint is part of the OpenAPI contract**: annotate the handler
  with `#[utoipa::path]` and register it in `ApiDoc` (`router/mod.rs`).
  Swagger UI on `/docs`, document on `/api-docs/openapi.json` — both
  mounted only when `environment` is `local` or `development`.

## Configuration

Typed structs in `common/src/config.rs`, merged in ascending priority:
hard-coded defaults → optional `/etc/starterhub-rust-stack/app-config.json`
→ optional local `app-config.json` (never committed) → `APP_*` environment
variables (`__` as nesting separator).

The `environment` key (strictly `local`, `development`, `staging` or
`production` — any other spelling aborts the boot) drives
environment-dependent behavior such as exposing `/docs`; it defaults to
`local`, and the Docker image overrides it to `production`.

Secrets are `secrecy::SecretString`, exposed with `expose_secret()` at
exactly one place. Every variable is documented in
[`.env.example`](./.env.example).

## Cron jobs

A job is a unit struct in `cron/src/job/<name>.rs` implementing the `Job`
trait — `name()`, a **hard-coded** `schedule()`, `async run(&self,
&AppState) -> JobResult` — and listed in `job::roster()`. Schedules are
code, not configuration. The generic wiring adapts any `Job` to the
scheduler and is never duplicated.

## Database and migrations

- Entities live in the `entity` crate only, re-exported in
  `entity/src/prelude.rs`.
- Migrations are modules named `mYYYYMMDD_NNNNNN_<label>` in
  `migration/src/source/`, registered chronologically in
  `Migrator::migrations`, and run automatically at boot.
- A shipped migration is immutable: never edit one, append a new one.

## Deployment

- **Local**: `docker-compose.yaml` provides PostgreSQL matching the
  configuration defaults.
- **Image**: the `Dockerfile` builds a minimal, hardened runtime
  (non-root, no shell, port 8080).
- **CI/CD**: every PR runs the quality gate and uploads test coverage to
  Codecov (thresholds in `codecov.yml`); a `cargo-deny` audit
  (advisories, licenses, sources — configuration in `deny.toml`) runs on
  dependency changes and weekly; release-please derives versions from the
  commit history; the `release` workflow publishes the image to Docker
  Hub with semver tags.

## Extending the service

**Add an endpoint**

1. DTOs in `api/src/dto/<resource>.rs`.
2. Business logic in `api/src/service/` (`Query` / `Mutation`).
3. Handler in `api/src/handler/<resource>.rs`, annotated with
   `#[utoipa::path]`, returning `ApiResult<T>`.
4. Route in `router/mod.rs` **and** registration in `ApiDoc`.
5. New failure modes as `ApiError` variants.

**Add a cron job** — one file in `cron/src/job/`, one line in
`job::roster()`.

**Add a migration** — `just migrate-generate <label>`, move the file into
`migration/src/source/`, register it in `Migrator::migrations`, model the
entity in `entity`.

**Add a configuration key** — field in `common/src/config.rs`, a default
in `Config::load`, a commented line in `.env.example`.

**Add a middleware** — one module in `api/src/middleware/`, registered in
`middleware::apply` at the right position.

## Documentation index

| Topic | Reference |
| ----- | --------- |
| axum (routing, extractors, middleware) | <https://docs.rs/axum> |
| tower / tower-http layers | <https://docs.rs/tower-http> |
| sea-orm & migrations | <https://www.sea-ql.org/SeaORM/docs/index/> |
| tokio (runtime, signals) | <https://tokio.rs> |
| tokio-cron-scheduler | <https://docs.rs/tokio_cron_scheduler> |
| utoipa (OpenAPI derive) | <https://docs.rs/utoipa> |
| config (layered configuration) | <https://docs.rs/config> |
| secrecy | <https://docs.rs/secrecy> |
| Conventional Commits v1.0.0 | <https://www.conventionalcommits.org/en/v1.0.0/> |
| release-please | <https://github.com/googleapis/release-please> |
| lefthook | <https://lefthook.dev> |
| Docker Hardened Images | <https://docs.docker.com/dhi/> |
