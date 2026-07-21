# Architecture

A guided tour of the service, meant to be read top-to-bottom the first
time you open the codebase. It explains **how the pieces fit, where code
goes and why it is shaped this way** — the layout and design rules stated
here are binding, not suggestions. The contribution process (toolchain
setup, branching, commit format, hooks, release pipeline) lives in
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

- [The workspace at a glance](#the-workspace-at-a-glance)
- [Boot and shutdown](#boot-and-shutdown)
- [Anatomy of a request](#anatomy-of-a-request)
- [Configuration and secrets](#configuration-and-secrets)
- [The cron engine](#the-cron-engine)
- [Database and migrations](#database-and-migrations)
- [From code to container](#from-code-to-container)
- [Extending the service](#extending-the-service)
- [Documentation index](#documentation-index)

## The workspace at a glance

One Cargo workspace (resolver 3), one binary. Every crate inherits its
metadata, dependencies and lints from the root `Cargo.toml`
(`[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`);
lints are strict — `unsafe_code = "forbid"` and missing documentation warns,
promoted to errors by the quality gate.

| Crate        | Role                                                                 |
| ------------ | -------------------------------------------------------------------- |
| `.` (`starterhub-rust-stack`) | Binary entry point: boot sequence only, no business logic.           |
| `api`        | HTTP layer: routing, handlers, DTOs, errors, OpenAPI, server.        |
| `common`     | Shared building blocks: configuration, telemetry, infrastructure.    |
| `entity`     | Database entities (sea-orm models) — the single home for data models. |
| `cron`       | Cron engine and jobs (tokio-cron-scheduler), one job per file.       |
| `migration`  | Schema migrations and their standalone CLI.                          |

Dependencies are declared once in `[workspace.dependencies]` and inherited
with `workspace = true`; features are added at the crate that needs them.
Stable versions only — no release candidates.

### Inside the `api` crate

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

## Boot and shutdown

`src/main.rs` is the only entry point and reads as a checklist:

1. `Config::load(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))` —
   resolve the layered configuration first; a malformed source aborts before
   anything starts. The `env!` macros expand in the binary crate, so the
   identity defaults are the real executable name and version.
2. `telemetry::init(debug)` — install the tracing subscriber before any
   traced work; the very first event is `booting name=… version=…`.
3. `postgresql::connect(&config.database)` — open the pool eagerly: failing
   fast at boot beats discovering a broken database on the first request.
4. `Migrator::up` — the schema is brought up to date before any traffic, so
   running code and database are always aligned.
5. Spawn the HTTP server and the cron engine side by side.

Shutdown is coordinated by **one `CancellationToken`**, the single
process-wide definition of "stop": a bridge task cancels it on
SIGTERM/SIGINT (what systemd sends on a Debian host), and `supervise()`
cancels it when *either* component stops for any reason — so a crashed
component always brings its sibling down instead of leaving a half-alive
process. Components take a plain `impl Future<Output = ()>` shutdown
argument (`token.clone().cancelled_owned()`) and stay signal-agnostic and
testable. `tokio::join!` (not `try_join!`) guarantees both components have
fully terminated before the process exits — that is what makes the shutdown
actually graceful.

## Anatomy of a request

```text
TCP (NoDelayListener, TCP_NODELAY)
  └─ middleware::apply — one ServiceBuilder, top-to-bottom:
       set x-request-id (UUID)  →  trace span  →  propagate x-request-id  →  catch panics
         └─ router: /livez /healthz /readyz /docs /api-docs/openapi.json + fallback
              └─ handler (crate::extract::{Json, Path})
                   └─ service (Query / Mutation)  →  sea-orm  →  PostgreSQL
```

Key properties, each enforced by one place in the code:

- **Handlers stay thin.** They extract input through
  `crate::extract::{Json, Path}`, call a service (`Query` for reads,
  `Mutation` for writes) and map the result — handlers never touch the
  database directly; database access belongs to services only.
- **Every response is JSON.** Success bodies are typed DTOs or
  `Json<Value>`; every failure — business error, extractor rejection,
  unknown route, caught panic, probe failure — flows through the single
  `ApiError` enum (`api/src/error.rs`) and its `{ "error": ... }` envelope.
  Handlers use the crate-local `crate::extract::{Json, Path}` wrappers
  (rejecting through `ApiError`) instead of the stock axum extractors:
  that is what keeps malformed-input rejections on the same envelope.
- **5xx bodies are opaque.** The real error is logged server-side with the
  request span (correlated by `x-request-id`); the client sees
  `"internal server error"`. 4xx messages are safe to expose.
- **Middlewares are declared once.** One module per concern under
  `api/src/middleware/`, composed in a single `ServiceBuilder` in
  `middleware::apply` — never scattered `Router::layer` calls. tower-http
  layers with unnameable closure generics are made nameable through
  fn-pointer type aliases (`MakeRequestSpan`, `PanicHandler`).
- **Health probes follow Kubernetes conventions.** `/livez` (process up,
  never checks dependencies — a failing dependency must not restart the
  pod), `/readyz` (pings the pool, 503 removes the pod from load balancing),
  `/healthz` as legacy liveness alias. They live in `router/`, not
  `handler/`: they are properties of the routing table, not business
  resources.
- **The OpenAPI contract lives next to the routing table.** `ApiDoc`
  (utoipa) is declared in `router/mod.rs`; every handler carries a
  `#[utoipa::path]` annotation and is registered in `paths(...)`. Swagger
  UI is served on `/docs` (assets embedded at compile time — the hardened
  image build stays offline), the document on `/api-docs/openapi.json`,
  versioned automatically from the crate.

## Configuration and secrets

Typed structs in `common/src/config.rs`, merged in ascending priority:

1. Hard-coded defaults — the service boots with zero external setup.
2. Optional `/etc/starterhub-rust-stack/app-config.json` (FHS path for the Debian/container
   deployment target).
3. Optional `app-config.json` in the working directory (local override,
   never committed).
4. `APP_*` environment variables, `__` as nesting separator —
   e.g. `APP_DATABASE__POOL__MAX_CONNECTIONS=50`. `.env` is loaded first
   (dotenvy) so local development needs no exported variables.

Secrets (`database.password`) are `secrecy::SecretString`: `Debug` prints
`REDACTED`, and the value is exposed with `expose_secret()` at exactly one
place — the connection builder. The application identity (`name`,
`version`) defaults to the binary's Cargo metadata and is overridable like
any other key (`APP_NAME`, `APP_VERSION`). Every variable is documented in
[`.env.example`](./.env.example).

## The cron engine

The `cron` crate wraps tokio-cron-scheduler behind a small, trait-based
design:

- A job is a unit struct in `cron/src/job/<name>.rs` implementing the
  object-safe `Job` trait: `name()`, a **hard-coded** `schedule()` (7-field
  cron expression) and `async run(&self, &AppState) -> JobResult`.
- `job::roster()` is the single list of jobs loaded at boot; the generic
  wiring (`into_cron_job`, `execute` — logging success at debug, failure at
  error) adapts any `Job` to the scheduler and is never duplicated.
- Schedules are code, not configuration: changing a cadence is behavior and
  belongs to code review.
- The engine is built **before** anything is spawned so an invalid schedule
  aborts the boot, and it shuts down through the same cancellation token as
  the HTTP server.

## Database and migrations

- Entities (sea-orm models) live in the `entity` crate, one module per
  table, re-exported in `entity/src/prelude.rs` — the API crate defines no
  data model of its own.
- Migrations live in `migration/src/source/`, one module named
  `mYYYYMMDD_NNNNNN_<label>`, registered chronologically in
  `Migrator::migrations`. They run automatically at boot, and the
  standalone CLI (`just migrate status`, `just migrate down`, ...) reads the
  **same configuration** as the service — no separate `DATABASE_URL`.
- A shipped migration is immutable: fixing the schema means appending a new
  migration, never editing an old one.

## From code to container

- **Local stack**: `docker-compose.yaml` provides PostgreSQL with
  credentials matching the configuration defaults — `just db-up && just run`
  works with zero setup.
- **Image**: the `Dockerfile` builds on Docker Hardened Images (`dhi.io`,
  authenticated): a Rust build stage, then a minimal Debian runtime — no
  shell, no package manager, `nonroot` user, port 8080. TLS goes through
  rustls, so the runtime image needs no OpenSSL.
- **CI/CD**: every PR runs the quality gate (`just ci`) on the
  organization's self-hosted runners; release-please maintains the release
  PR from the Conventional Commits history; merging it tags the release,
  and the `release` workflow builds and pushes the image to Docker Hub
  (`aartintelligent/starterhub-rust-stack`) with semver tags. Builds are cached two ways:
  a persistent BuildKit builder per runner (keeps cargo's `RUN` cache
  mounts) and a shared `:buildcache` registry cache (layers reused across
  machines) — a no-change rebuild takes ~35 s instead of ~3 min.

## Extending the service

Recipes, in the order the code expects them; each one follows the layout
and design rules of the sections above.

**Add an endpoint**

1. Wire format in `api/src/dto/<resource>.rs` (Serialize/Deserialize
   structs; document every field).
2. Business logic in `api/src/service/` (`Query` for reads, `Mutation` for
   writes) — services own the database access.
3. Handler in `api/src/handler/<resource>.rs`: extract input through
   `crate::extract::{Json, Path}`, call the service, map to a DTO; return
   `ApiResult<T>`. Annotate with `#[utoipa::path]`.
4. Declare the route in `router/mod.rs` **and** register the handler in
   `ApiDoc`'s `paths(...)` — an endpoint absent from `ApiDoc` is invisible
   in `/docs` and counts as undocumented.
5. New failure modes become `ApiError` variants (each with its own match
   arm), never ad-hoc status codes.

**Add a cron job** — new file `cron/src/job/<name>.rs` with a unit struct
implementing `Job`, then one line in `job::roster()`. Nothing else.

**Add a migration** — `just migrate-generate <label>`, move the generated
file into `migration/src/source/`, register it in `Migrator::migrations`,
model the entity in the `entity` crate.

**Add a configuration key** — field on the right struct in
`common/src/config.rs` (+ rustdoc), a default in `Config::load`, a
commented line in `.env.example`. Secrets are `SecretString`, exposed once.

**Add a middleware** — one module in `api/src/middleware/`, registered in
`middleware::apply` at the right position of the ServiceBuilder chain
(order is execution order, top-to-bottom).

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
