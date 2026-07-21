# starterhub-rust-stack

[![rust](https://img.shields.io/badge/rust-1.97-B7410E?logo=rust)](./rust-toolchain.toml)
[![ci](https://github.com/aartintelligent/starterhub-rust-stack/actions/workflows/ci.yaml/badge.svg)](https://github.com/aartintelligent/starterhub-rust-stack/actions/workflows/ci.yaml)
[![audit](https://github.com/aartintelligent/starterhub-rust-stack/actions/workflows/audit.yaml/badge.svg)](https://github.com/aartintelligent/starterhub-rust-stack/actions/workflows/audit.yaml)
[![release](https://github.com/aartintelligent/starterhub-rust-stack/actions/workflows/release.yaml/badge.svg)](https://github.com/aartintelligent/starterhub-rust-stack/actions/workflows/release.yaml)
[![codecov](https://codecov.io/gh/aartintelligent/starterhub-rust-stack/graph/badge.svg?token=rZyaU31eyI)](https://codecov.io/gh/aartintelligent/starterhub-rust-stack)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

The foundation for Rust service stacks: a production-grade starter you
instantiate to begin every new service — not a demo, a **runnable,
released, containerized service** with the boring-but-vital parts already
solved and enforced.

## 🎯 Purpose

Starting a new service usually means re-solving the same problems before
the first line of business code: how it boots and stops cleanly, how it is
configured across environments, how errors reach the caller, how the API
is documented, how the database schema evolves, how quality is kept high,
how versions are released and shipped. Solving them differently in every
repository produces a fleet of services that all behave slightly
differently.

This project solves those problems **once**, so that every service built
from it starts complete, behaves consistently with its siblings, and lets
the team focus on business value from day one.

## 💡 What you get

Functionally, a service created from this foundation:

- **Runs out of the box** — one command starts it, with a working local
  environment and zero manual setup.
- **Describes itself** — an always-up-to-date, interactive documentation
  of every endpoint, generated from the code and never out of sync.
- **Reports its own health** — standard probes that deployment platforms
  understand, so a broken instance is detected and replaced automatically.
- **Fails predictably** — every error, whatever its origin, reaches the
  caller in one single, consistent format.
- **Runs scheduled work** — recurring background jobs live alongside the
  service, reviewed like any other code.
- **Keeps secrets safe** — configuration adapts to each environment, and
  credentials can never leak into logs.
- **Evolves its data safely** — database changes travel with the code and
  apply themselves, so code and schema are always aligned.
- **Stops gracefully** — restarts and deployments never cut a request
  short.
- **Stays healthy over time** — a strict, automated quality gate blocks
  anything undocumented, unformatted or unsound.
- **Releases itself** — versioning, changelog and publication of a
  ready-to-deploy image are fully automated from the history of changes.

## 👥 Who it is for

- **Teams** bootstrapping a new service who want production standards from
  the first commit, not retrofitted later.
- **Contributors** joining an existing service built on this foundation:
  every sibling service is laid out and operated the same way.
- **AI coding agents** working in the repository: the rules they must
  follow are written down, enforced, and machine-checkable.

## 🧭 Where to go next

| You want to…                                              | Read                                                                 |
| --------------------------------------------------------- | -------------------------------------------------------------------- |
| Install, run and contribute                               | [`CONTRIBUTING.md`](./CONTRIBUTING.md)                               |
| Understand how the service is designed                    | [`ARCHITECTURE.md`](./ARCHITECTURE.md)                               |
| Add an endpoint, a cron job, a migration, a config key    | [`ARCHITECTURE.md`](./ARCHITECTURE.md) → *Extending the service*     |
| Condensed notes for AI coding agents                      | [`CLAUDE.md`](./CLAUDE.md)                                           |
