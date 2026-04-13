#!/usr/bin/env sh
set -eu

# ==============================================================================
# Local Compose Launcher
# ==============================================================================
#
# The repository supports both Docker and rootless Podman through the same
# `just install-docker` entry point. Podman needs an extra Compose override so
# the backend state bind mount uses `keep-id`; otherwise SQLite opens the copied
# host database read-only inside the container.

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
BASE_COMPOSE_FILE="${TRACK_COMPOSE_FILE:-${REPO_ROOT}/compose.yaml}"

detect_container_cli() {
  if command -v docker >/dev/null 2>&1; then
    printf 'docker\n'
    return 0
  fi

  if command -v podman >/dev/null 2>&1; then
    printf 'podman\n'
    return 0
  fi

  printf 'compose-stack: neither `docker` nor `podman` is available\n' >&2
  exit 1
}

is_podman_backend() {
  cli="$1"
  version_output="$("$cli" --version 2>&1 || true)"
  case "$version_output" in
    *[Pp]odman*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

CONTAINER_CLI="$(detect_container_cli)"

if is_podman_backend "$CONTAINER_CLI"; then
  # The Podman-only override is small enough to generate inline. Keeping it in
  # the launcher avoids maintaining a second local compose file just to set one
  # compatibility knob.
  PODMAN_OVERRIDE_FILE="$(mktemp "${TMPDIR:-/tmp}/track-compose-podman.XXXXXX.yaml")"
  trap 'rm -f "$PODMAN_OVERRIDE_FILE"' EXIT INT TERM
  cat >"$PODMAN_OVERRIDE_FILE" <<'EOF'
services:
  track-web:
    userns_mode: keep-id
EOF
  "$CONTAINER_CLI" compose -f "$BASE_COMPOSE_FILE" -f "$PODMAN_OVERRIDE_FILE" "$@"
  exit $?
fi

exec "$CONTAINER_CLI" compose -f "$BASE_COMPOSE_FILE" "$@"
