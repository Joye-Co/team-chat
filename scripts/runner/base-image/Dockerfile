# The read-only root disk of every run's microVM: an ordinary Ubuntu with the tools agents use
# preinstalled, so a run never downloads them. Built into an ext4 image by build-base-image.sh.
FROM ubuntu:24.04

ARG NODE_VERSION=24.22.2
ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl wget git git-lfs openssh-client gnupg \
      jq ripgrep fd-find less tree file procps psmisc htop \
      iproute2 iputils-ping dnsutils netcat-openbsd \
      python3 python3-pip python3-venv pipx \
      build-essential pkg-config cmake \
      unzip zip zstd xz-utils bzip2 \
      sudo tzdata locales gh sqlite3 \
    && ln -s /usr/bin/fdfind /usr/local/bin/fd \
    && rm -rf /var/lib/apt/lists/*

# Node from the official tarball, checked against its SHASUMS256.
RUN set -eux; arch="$(dpkg --print-architecture)"; case "$arch" in amd64) a=x64;; arm64) a=arm64;; esac; \
    f="node-v${NODE_VERSION}-linux-${a}.tar.xz"; cd /tmp; \
    curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/${f}"; \
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" | grep " ${f}\$" | sha256sum -c -; \
    tar -xJf "$f" -C /usr/local --strip-components=1 --no-same-owner; rm "$f"; \
    node --version; npm --version

# The agent CLIs and common package managers.
RUN npm install -g --no-fund --no-audit \
      @anthropic-ai/claude-code @openai/codex @mariozechner/pi-coding-agent pnpm \
    && npm cache clean --force \
    && curl -fsSL https://bun.sh/install | BUN_INSTALL=/usr/local bash \
    && PIPX_HOME=/opt/pipx PIPX_BIN_DIR=/usr/local/bin pipx install uv

# The user every CLI runs as. sudo without a password: the VM is the boundary, not this user.
RUN userdel -r ubuntu 2>/dev/null || true; \
    useradd --create-home --uid 1000 --shell /bin/bash agent \
    && echo 'agent ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/agent && chmod 440 /etc/sudoers.d/agent \
    && git config --system init.defaultBranch main

COPY vm-init /sbin/vm-init
RUN chmod 755 /sbin/vm-init
