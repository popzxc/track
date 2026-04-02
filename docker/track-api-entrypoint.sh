#!/bin/sh
set -eu

# ==============================================================================
# Arbitrary-UID Runtime Compatibility
# ==============================================================================
#
# The packaged backend wrapper runs the container as the caller's numeric
# UID/GID so the bind-mounted SQLite state stays writable on the host. OpenSSH
# and some libc helpers still expect the current UID to exist in passwd/group
# databases, so we synthesize a temporary NSS view when Docker starts the
# container under an arbitrary host UID that is not baked into the image.

RUNTIME_UID="$(id -u)"
RUNTIME_GID="$(id -g)"
RUNTIME_HOME="${HOME:-/home/track}"
NSS_WRAPPER_DIR="${NSS_WRAPPER_DIR:-/tmp/track-nss-wrapper}"
PASSWD_FILE="${NSS_WRAPPER_DIR}/passwd"
GROUP_FILE="${NSS_WRAPPER_DIR}/group"
NSS_WRAPPER_LIB=""

mkdir -p "${NSS_WRAPPER_DIR}"
cp /etc/passwd "${PASSWD_FILE}"
cp /etc/group "${GROUP_FILE}"

if ! awk -F: -v gid="${RUNTIME_GID}" '$3 == gid { found = 1 } END { exit found ? 0 : 1 }' /etc/group; then
  printf 'track-runtime:x:%s:\n' "${RUNTIME_GID}" >> "${GROUP_FILE}"
fi

if ! awk -F: -v uid="${RUNTIME_UID}" '$3 == uid { found = 1 } END { exit found ? 0 : 1 }' /etc/passwd; then
  printf 'track-runtime:x:%s:%s:track runtime:%s:/bin/sh\n' \
    "${RUNTIME_UID}" \
    "${RUNTIME_GID}" \
    "${RUNTIME_HOME}" \
    >> "${PASSWD_FILE}"
fi

for candidate in \
  /usr/lib/*/libnss_wrapper.so \
  /usr/lib/libnss_wrapper.so \
  /lib/*/libnss_wrapper.so
do
  if [ -f "${candidate}" ]; then
    NSS_WRAPPER_LIB="${candidate}"
    break
  fi
done

if [ -z "${NSS_WRAPPER_LIB}" ]; then
  printf 'track-api-entrypoint: could not locate libnss_wrapper.so\n' >&2
  exit 1
fi

export NSS_WRAPPER_PASSWD="${PASSWD_FILE}"
export NSS_WRAPPER_GROUP="${GROUP_FILE}"
export LD_PRELOAD="${NSS_WRAPPER_LIB}${LD_PRELOAD:+:${LD_PRELOAD}}"

exec "$@"
