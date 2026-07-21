# Contributing

Thanks for contributing to starterhub-rust-stack. This guide is the **binding process** for
the repository — how to set it up, work in it, the commit and branching
rules, and how changes reach a release. It applies to human contributors
**and to AI coding agents**: if a change you are asked to make conflicts
with this document, surface the conflict instead of silently breaking the
rule.

> Everything about the codebase itself — the workspace layout, where code
> goes, the design rules a change must respect, and the extension recipes —
> lives in [`ARCHITECTURE.md`](./ARCHITECTURE.md). Read it before writing
> code. Condensed agent guidance lives in [`CLAUDE.md`](./CLAUDE.md).

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
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) — optional,
  for the local dependency audit (`just deny`); CI runs it anyway.
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) — optional,
  for the local coverage report (`just coverage`); CI measures and
  uploads coverage to Codecov on every pull request.

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

The API answers on `http://127.0.0.1:8080` — probes on `/livez` and
`/readyz`, interactive documentation on `/docs`.

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

The whole path from merge to published image is **automatic**:

1. On every merge to `main`, release-please opens or updates a **release
   PR** accumulating changes and the computed version (all six crate
   versions and `Cargo.lock` bump in lockstep).
2. Merging that release PR tags `vX.Y.Z`, publishes the GitHub release
   and updates `CHANGELOG.md`.
3. The `release-please` workflow then chains the `release` workflow
   (`workflow_dispatch` events are exempt from GitHub's loop protection,
   so no PAT is involved), which builds the hardened image and pushes it
   to Docker Hub (`aartintelligent/starterhub-rust-stack`) with the
   `X.Y.Z`, `X.Y` and `latest` tags.

To rebuild the image of a past release, dispatch the workflow by hand:
`gh workflow run release.yaml -f tag=vX.Y.Z`. A manual rebuild never
moves `latest` unless you add `-f latest=true` — the floating tag always
follows the newest release.

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
  reference for **the codebase** — layout, design rules, extension
  recipes. Read both before writing code; keep every gate green and report
  failures honestly.
- Never introduce `unsafe` or undocumented items; follow the layout and
  design rules of `ARCHITECTURE.md`.
- Write commit messages and PR titles in Conventional Commits form so the
  release automation keeps working.
- Respect the git hooks; do not commit with `--no-verify`.
