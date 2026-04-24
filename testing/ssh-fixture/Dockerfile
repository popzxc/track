FROM debian:bookworm-slim

# This image exists only for test infrastructure, so we optimize for explicit
# tooling and debuggability rather than a tiny runtime footprint.
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    bash \
    git \
    openssh-server \
    procps \
    python3 \
  && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --home-dir /home/track --shell /bin/bash track

RUN mkdir -p /var/run/sshd /opt/track-testing/bin /opt/track-testing/mocks /srv/track-testing

COPY entrypoint.sh /opt/track-testing/entrypoint.sh
COPY bin/gh /opt/track-testing/bin/gh
COPY bin/codex /opt/track-testing/bin/codex
COPY bin/claude /opt/track-testing/bin/claude
COPY mocks/common.py /opt/track-testing/mocks/common.py
COPY mocks/gh.py /opt/track-testing/mocks/gh.py
COPY mocks/codex.py /opt/track-testing/mocks/codex.py
COPY mocks/claude.py /opt/track-testing/mocks/claude.py

RUN chmod +x /opt/track-testing/entrypoint.sh \
  /opt/track-testing/bin/gh \
  /opt/track-testing/bin/codex \
  /opt/track-testing/bin/claude

EXPOSE 22

ENTRYPOINT ["/opt/track-testing/entrypoint.sh"]
