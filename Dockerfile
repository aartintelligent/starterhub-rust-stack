# syntax=docker/dockerfile:1
#
# Multi-stage build on Docker Hardened Images (dhi.io), following the DHI
# guides for rust and debian-base:
#
# - Build stage: the `-dev` variant ships the full Rust toolchain and runs
#   as root, as build stages are expected to.
# - Runtime stage: the hardened debian-base (Debian 13 / trixie, matching
#   the project's deployment target) runs as `nonroot` (uid 65532), has no
#   shell and no package manager, and ships TLS certificates out of the
#   box. The binary links only against glibc: database TLS is rustls
#   (pure Rust), so no system OpenSSL is needed.
#
# Requires `docker login dhi.io` before building.

# Pin the toolchain line explicitly: builds must not silently change
# compiler when DHI publishes a new default.
ARG RUST_VERSION=1.97

# ---------------------------------------------------------------------------
# Build stage
# ---------------------------------------------------------------------------
FROM dhi.io/rust:${RUST_VERSION}-debian-dev AS build

WORKDIR /app

# The full workspace is needed: the binary crate pulls api, common, cron,
# entity and migration by path. `.dockerignore` keeps the context minimal
# (no target/, no .git, no local secrets).
COPY . .

# BuildKit cache mounts keep the cargo registry and the target directory
# across builds, so incremental rebuilds only recompile what changed.
# The binary is copied out of the cache mount within the same RUN, because
# cache mounts are not part of the image layers.
# `--locked` guarantees the build uses Cargo.lock exactly as committed:
# a drifting lockfile must fail the build, not silently resolve.
RUN --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked \
    && cp target/release/rust-service-starter /usr/local/bin/rust-service-starter

# ---------------------------------------------------------------------------
# Runtime stage
# ---------------------------------------------------------------------------
FROM dhi.io/debian-base:trixie AS runtime

# OCI metadata so registries and scanners can trace the artifact.
LABEL org.opencontainers.image.title="rust-service-starter" \
      org.opencontainers.image.description="Production-grade foundation for Rust service stacks" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.source="https://github.com/aartintelligent/rust-service-starter"

# Single, root-owned, world-readable binary: the nonroot user must be able
# to execute it but never to overwrite it.
COPY --from=build /usr/local/bin/rust-service-starter /usr/local/bin/rust-service-starter

# Containers listen on all interfaces (the pod/network boundary does the
# isolation), and 8080 respects the nonroot >1024 port constraint baked
# into the hardened image. Everything else keeps the application defaults
# and stays overridable through APP_* variables or a config file mounted
# at /etc/rust-service-starter/app-config.json.
ENV APP_SERVER__HOST=0.0.0.0 \
    APP_SERVER__PORT=8080

EXPOSE 8080

# No HEALTHCHECK on purpose: the hardened image has no shell or curl to
# run one, and the orchestrator's HTTP probes (/livez, /readyz) are the
# supported mechanism.

# The hardened base already defaults to the nonroot user (uid 65532);
# stating it keeps the security posture explicit and lint-friendly.
USER nonroot

# Exec form, no shell involved: PID 1 is the application itself, so
# SIGTERM from the orchestrator reaches the graceful-shutdown handler
# directly.
ENTRYPOINT ["/usr/local/bin/rust-service-starter"]
