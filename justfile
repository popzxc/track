cargo := env('CARGO', 'cargo')
bun := env('BUN', 'bun')

# List the available recipes.
[default]
default:
  @just --list

# Build the release CLI and API binaries used for local development.
build-rust:
  {{cargo}} build --release -p track-cli
  {{cargo}} build --release -p track-api

# Build the frontend production bundle.
[working-directory: "frontend"]
build-fe:
  {{bun}} install
  {{bun}} run build

# Build the docs production bundle.
[working-directory: "docs"]
build-docs:
  {{bun}} install
  {{bun}} run build

# Build the backend binaries, frontend bundle, and docs bundle.
build-all: build-rust build-fe build-docs

# Run the local docs development server.
[working-directory: "docs"]
run-docs: build-docs
  {{bun}} run dev

[working-directory: "frontend"]
run-fe: build-fe
  {{bun}} run dev

run-api:
  {{cargo}} run -p track-api

# Install the CLI from the current checkout.
install-cli:
  {{cargo}} install --path crates/track-cli --force --locked

# Start the repository-local Docker stack.
install-docker:
  TRACK_UID=${TRACK_UID:-$(id -u)} TRACK_GID=${TRACK_GID:-$(id -g)} docker compose up --build -d

# Install the CLI and start the local Docker stack.
install-all: install-cli install-docker

# Run Rust tests except the SSH-backed integration crate.
test-rust:
  {{cargo}} test --workspace --exclude track-integration-tests

# Run frontend typechecking and unit tests.
[working-directory: "frontend"]
test-fe:
  {{bun}} run typecheck
  {{bun}} run test

# Run the Rust integration tests sequentially with the live fixture enabled.
test-int:
  RUN_TRACK_INTEGRATION_TESTS=true RUST_TEST_THREADS=1 {{cargo}} test -p track-integration-tests

# Run the frontend browser end-to-end suite.
[working-directory: "frontend"]
test-e2e:
  {{bun}} run test:e2e

# Run all local test suites.
test-all: test-rust test-fe test-int test-e2e

# Rust lints
lint-rust:
  {{cargo}} fmt --all -- --check
  {{cargo}} clippy --workspace -- -D warnings

# TypeScript lints
lint-ts:
  {{bun}} run typecheck

# Check lints in the project.
lint: lint-rust lint-ts

# Run lint and every local test suite.
pr-ready: lint test-all