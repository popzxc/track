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
COPY crates/track-capture/Cargo.toml crates/track-capture/Cargo.toml
COPY crates/track-cli/Cargo.toml crates/track-cli/Cargo.toml
COPY crates/track-api/Cargo.toml crates/track-api/Cargo.toml
COPY crates/track-integration-tests/Cargo.toml crates/track-integration-tests/Cargo.toml
COPY crates/track-core/src crates/track-core/src
COPY crates/track-capture/src crates/track-capture/src
COPY crates/track-cli/src crates/track-cli/src
COPY crates/track-cli/tests crates/track-cli/tests
COPY crates/track-api/src crates/track-api/src
COPY crates/track-integration-tests/src crates/track-integration-tests/src

# Cargo resolves every workspace member's manifest even when we build only
# `track-api`, so the image needs the lightweight workspace crate layouts to
# keep workspace metadata valid. We intentionally omit `tests/` here because
# the production image does not execute the live Docker-backed test suite.

RUN cargo build --release -p track-api

FROM debian:bookworm-slim AS runtime
WORKDIR /app

ARG TRACK_UID=1000
ARG TRACK_GID=1000

ENV PORT=3210
ENV HOME=/home/track
ENV TRACK_STATIC_ROOT=/app/frontend/dist

RUN apt-get update \
  && apt-get install -y --no-install-recommends openssh-client \
  && groupadd --gid "${TRACK_GID}" track \
  && useradd --uid "${TRACK_UID}" --gid "${TRACK_GID}" --create-home --home-dir /home/track --shell /bin/sh track \
  && rm -rf /var/lib/apt/lists/*

COPY --from=rust-build /app/target/release/track-api /usr/local/bin/track-api
COPY --from=frontend-build /app/frontend/dist ./frontend/dist

USER track

EXPOSE 3210

CMD ["track-api"]
