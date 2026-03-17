FROM oven/bun:1.3 AS frontend-build
WORKDIR /app/frontend

COPY frontend/package.json ./
COPY frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

FROM rust:1.88-slim AS rust-build
WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml deny.toml ./
COPY .config/nextest.toml .config/nextest.toml
COPY crates/track-core/Cargo.toml crates/track-core/Cargo.toml
COPY crates/track-cli/Cargo.toml crates/track-cli/Cargo.toml
COPY crates/track-api/Cargo.toml crates/track-api/Cargo.toml
COPY crates/track-core/src crates/track-core/src
COPY crates/track-cli/src crates/track-cli/src
COPY crates/track-cli/tests crates/track-cli/tests
COPY crates/track-api/src crates/track-api/src

RUN cargo build --release -p track-api

FROM debian:bookworm-slim AS runtime
WORKDIR /app

ENV PORT=3210
ENV TRACK_STATIC_ROOT=/app/frontend/dist

COPY --from=rust-build /app/target/release/track-api /usr/local/bin/track-api
COPY --from=frontend-build /app/frontend/dist ./frontend/dist

EXPOSE 3210

CMD ["track-api"]
