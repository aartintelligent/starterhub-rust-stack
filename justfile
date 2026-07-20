# Load .env so APP_* overrides are available to every recipe.
set dotenv-load

# List available recipes.
default:
    @just --list

# Type-check the whole workspace without producing binaries.
check:
    cargo check --workspace

# Build every crate of the workspace.
build:
    cargo build --workspace

# Run the application (config -> database -> migrations -> HTTP server).
run:
    cargo run

# Start the local PostgreSQL (docker-compose.yaml) and wait until healthy.
db-up:
    docker compose up -d --wait postgres

# Stop the local stack, keeping the data volume.
db-down:
    docker compose down

# Stop the local stack AND wipe the data volume (fresh database).
db-reset:
    docker compose down -v

# Format the whole workspace.
fmt:
    cargo fmt --all

# Fail if any file is not properly formatted (CI-friendly).
fmt-check:
    cargo fmt --all --check

# Lint the whole workspace, warnings are errors.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run every test of the workspace.
test:
    cargo test --workspace

# Run the sea-orm-migration CLI, e.g. `just migrate up`, `just migrate status`.
migrate *args:
    cargo run -p migration -- {{args}}

# Scaffold a new migration file, e.g. `just migrate-generate create_subnet_table`.
migrate-generate name:
    cargo run -p migration -- generate {{name}}

# Full local quality gate, mirrors what CI should enforce.
ci: fmt-check lint test
