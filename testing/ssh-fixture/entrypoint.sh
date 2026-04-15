#!/usr/bin/env bash

set -euo pipefail

RUNTIME_DIR="${TRACK_TESTING_RUNTIME_DIR:-/srv/track-testing}"
TRACK_HOME="/home/track"

# The mounted runtime directory is the stable contract between the fixture and
# whichever test runner is driving it. We normalize it on startup so later SSH
# commands can assume the layout already exists.
mkdir -p \
  "$RUNTIME_DIR/state" \
  "$RUNTIME_DIR/logs" \
  "$RUNTIME_DIR/git" \
  "$TRACK_HOME/.ssh"

if [ ! -f "$RUNTIME_DIR/state/gh.json" ]; then
  printf '%s\n' '{"login":"fixture-user","repositories":{}}' > "$RUNTIME_DIR/state/gh.json"
fi

if [ ! -f "$RUNTIME_DIR/state/codex.json" ]; then
  printf '%s\n' \
    '{"mode":"success","sleepSeconds":0,"status":"succeeded","summary":"Mock Codex completed successfully.","pullRequestUrl":null,"branchName":null,"worktreePath":null,"notes":null}' \
    > "$RUNTIME_DIR/state/codex.json"
fi

if [ ! -f "$RUNTIME_DIR/state/claude.json" ]; then
  printf '%s\n' \
    '{"mode":"success","sleepSeconds":0,"status":"succeeded","summary":"Mock Claude completed successfully.","pullRequestUrl":null,"branchName":null,"worktreePath":null,"notes":null}' \
    > "$RUNTIME_DIR/state/claude.json"
fi

if [ ! -f "$RUNTIME_DIR/state/opencode.json" ]; then
  printf '%s\n' \
    '{"mode":"success","sleepSeconds":0,"status":"succeeded","summary":"Mock opencode completed successfully.","pullRequestUrl":null,"branchName":null,"worktreePath":null,"reviewSubmitted":false,"githubReviewId":null,"githubReviewUrl":null,"reviewBody":null,"notes":null}' \
    > "$RUNTIME_DIR/state/opencode.json"
fi

if [ -f "$RUNTIME_DIR/authorized_keys" ]; then
  cp "$RUNTIME_DIR/authorized_keys" "$TRACK_HOME/.ssh/authorized_keys"
fi

touch "$TRACK_HOME/.ssh/authorized_keys"
chmod 700 "$TRACK_HOME/.ssh"
chmod 600 "$TRACK_HOME/.ssh/authorized_keys"

chown -R track:track "$TRACK_HOME" "$RUNTIME_DIR"
# Make the runtime directory world-writable so the host test runner (which may
# have a different uid after the chown) can still create files such as
# known_hosts while the container is running.
chmod -R 777 "$RUNTIME_DIR"

# The fixture volume is seeded by the host test runner after the container
# starts, so newly created files (bare git repos, state files) are owned by the
# host UID rather than the container's 'track' user.  Git 2.35.2+ refuses to
# operate on repositories owned by a different user unless they are explicitly
# marked safe.  Marking '*' as safe lets the track user clone and work inside
# any path on the volume without per-directory workarounds.
su -s /bin/sh track -c "git config --global --add safe.directory '*'"

exec /usr/sbin/sshd -D -e
