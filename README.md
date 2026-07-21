# rust-service-starter

The foundation for Rust service stacks: a production-grade starter you
instantiate to begin every new service — not a demo, a **runnable,
released, containerized service** with the boring-but-vital parts already
solved and enforced.

> [!IMPORTANT]
> **Before contributing, read [`CONTRIBUTING.md`](./CONTRIBUTING.md).** It is
> the binding process for this repo — the branching model, the **mandatory
> Conventional Commit** rules (types, release effects, breaking-change
> syntax), the lefthook git hooks, the pull-request flow and the automated
> release pipeline.

> [!TIP]
> **New to the codebase? Read [`ARCHITECTURE.md`](./ARCHITECTURE.md) first** —
> a guided, human-oriented tour designed to be read top-to-bottom: how the
> process boots and shuts down gracefully, how a request flows through the
> middleware pipeline down to the single JSON error envelope, how
> configuration, secrets, cron jobs and migrations work, and how code becomes
> a container image — plus recipes for extending the service.

## ⚙️ Technical stack

![](https://img.shields.io/badge/rust-grey?logo=rust)
![](https://img.shields.io/badge/axum-grey)
![](https://img.shields.io/badge/sea--orm-grey)
![](https://img.shields.io/badge/postgresql-grey?logo=postgresql)
![](https://img.shields.io/badge/docker-grey?logo=docker)
![](https://img.shields.io/badge/release%20please-grey?logo=googlecloud)
![](https://img.shields.io/badge/conventional%20commits-grey?logo=conventionalcommits)

**What's inside:** an axum HTTP API with Kubernetes-style health probes and
a Swagger UI on `/docs` generated from the code (utoipa), a tokio cron
engine running trait-based jobs, layered configuration with
`secrecy`-protected credentials, sea-orm entities and migrations applied at
boot, graceful SIGTERM/SIGINT shutdown supervised by a single cancellation
token, strict workspace lints (`unsafe` forbidden, documentation
mandatory), git hooks, and an automated release pipeline (Conventional
Commits → release-please → tag + changelog → hardened Docker image).

## 🏗️ Create a new stack from this template

This repository is a **GitHub template**: click **Use this template →
Create a new repository**, then rebrand the copy — the service name lives
in a handful of well-known places:

1. `Cargo.toml` — the root `[package] name`.
2. `Dockerfile` — the two binary paths (`target/release/<name>`,
   `/usr/local/bin/<name>`) and the OCI labels.
3. `common/src/config.rs` — the FHS config path (`/etc/<name>/app-config`)
   and, if wanted, the default database name.
4. `api/src/router/mod.rs` — the OpenAPI `title`/`description`.
5. `.github/workflows/release.yaml` — the image repository and the
   BuildKit builder name.
6. `release-please-config.json` — `package-name`.
7. `docker-compose.yaml`, `.env.example`, `lefthook.yml`, this `README.md`
   — cosmetic mentions.
8. Repository settings: secrets (`DHI_USERNAME`/`DHI_PASSWORD`), squash-only
   merge, auto-merge, and self-hosted runner access.

Then delete this section, rewrite the intro above for the new service, and
start shipping business code (`ARCHITECTURE.md` → *Extending the service*).

## 🚀 Getting started

- The **Rust toolchain** is pinned by `rust-toolchain.toml` — rustup picks it
  up automatically. You also need [just](https://github.com/casey/just) and
  **Docker** (for the local PostgreSQL).

```bash
just db-up     # start PostgreSQL (docker compose) and wait until healthy
just run       # config -> database -> migrations -> API + cron engine
just hooks     # optional: install the git hooks (lefthook)
```

The API answers on `http://127.0.0.1:8080` — probes on `/livez` and
`/readyz`, interactive documentation on `/docs`. `just` alone lists every
recipe (`check`, `lint`, `test`, `migrate`, `ci`, ...).

## 🧭 Where to go next

| You want to…                                             | Read                                                             |
| -------------------------------------------------------- | ---------------------------------------------------------------- |
| Understand how the service is designed                   | [`ARCHITECTURE.md`](./ARCHITECTURE.md)                            |
| Add an endpoint, a cron job, a migration, a config key   | [`ARCHITECTURE.md`](./ARCHITECTURE.md) → *Extending the service*  |
| Know where a new file belongs                            | [`CONTRIBUTING.md`](./CONTRIBUTING.md) → *Where code goes*        |
| Branch, commit, open a PR, release and publish the image | [`CONTRIBUTING.md`](./CONTRIBUTING.md)                            |
| Condensed notes for AI coding agents                     | [`CLAUDE.md`](./CLAUDE.md)                                        |
