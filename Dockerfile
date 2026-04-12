FROM oven/bun:1.3 AS frontend-build
WORKDIR /app/frontend

COPY frontend/package.json ./
COPY frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

FROM rust:1.88-slim AS rust-build
WORKDIR /app

ARG TRACK_GIT_COMMIT=unknown
ENV TRACK_GIT_COMMIT=${TRACK_GIT_COMMIT}
ENV SQLX_OFFLINE=true

COPY Cargo.toml Cargo.lock rust-toolchain.toml deny.toml ./
COPY .config/nextest.toml .config/nextest.toml
# We don't need _all_ the crates, but copying only what we need manually is too verbose.
COPY crates/ crates/

RUN cargo build --release -p track-api

FROM debian:bookworm-slim AS runtime
WORKDIR /app

ARG TRACK_UID=1000
ARG TRACK_GID=1000

ENV PORT=3210
ENV HOME=/home/track
ENV TRACK_STATIC_ROOT=/app/frontend/dist
ENV TRACK_UID=${TRACK_UID}
ENV TRACK_GID=${TRACK_GID}

# The shipped backend runs with the caller's host UID/GID so bind-mounted state
# stays writable without rebuilding the release image on each machine. That
# means the runtime must remain functional even when Docker starts it under an
# arbitrary numeric UID/GID that does not exist in `/etc/passwd`, which would
# otherwise break OpenSSH-based remote dispatches. We therefore avoid baking a
# named user/group for the caller IDs because common host groups such as macOS
# GID 20 already exist in Debian images under unrelated names.
RUN apt-get update \
  && apt-get install -y --no-install-recommends libnss-wrapper openssh-client \
  && mkdir -p /home/track/backend-state /home/track/legacy-home \
  && chown -R "${TRACK_UID}:${TRACK_GID}" /home/track \
  && rm -rf /var/lib/apt/lists/*

COPY --from=rust-build /app/target/release/track-api /usr/local/bin/track-api
COPY --from=frontend-build /app/frontend/dist ./frontend/dist
COPY docker/track-api-entrypoint.sh /usr/local/bin/track-api-entrypoint

RUN chmod +x /usr/local/bin/track-api-entrypoint
USER ${TRACK_UID}:${TRACK_GID}

EXPOSE 3210

ENTRYPOINT ["track-api-entrypoint"]
CMD ["track-api"]
