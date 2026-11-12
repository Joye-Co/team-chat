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

# The rustup proxies (rustup, cargo, rustc...) without a toolchain: a run's CARGO_HOME and
# RUSTUP_HOME point at its repository's cache, where the daemon installs the toolchain the
# repository asks for. Pinned rustup-init, checked against the SHA-256 written here.
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_SHA256_AMD64=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
ARG RUSTUP_SHA256_ARM64=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282
RUN set -eux; arch="$(dpkg --print-architecture)"; case "$arch" in \
      amd64) t=x86_64-unknown-linux-gnu; sum="$RUSTUP_SHA256_AMD64";; \
      arm64) t=aarch64-unknown-linux-gnu; sum="$RUSTUP_SHA256_ARM64";; esac; \
    curl -fsSL -o /tmp/rustup-init "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${t}/rustup-init"; \
    echo "${sum}  /tmp/rustup-init" | sha256sum -c -; chmod 755 /tmp/rustup-init; \
    CARGO_HOME=/opt/rust/cargo RUSTUP_HOME=/opt/rust/rustup /tmp/rustup-init -y --no-modify-path \
      --default-toolchain none --profile minimal; \
    rm /tmp/rustup-init; /opt/rust/cargo/bin/rustup --version

# sccache from its official static release, checked against the SHA-256 written here.
ARG SCCACHE_VERSION=0.19.0
ARG SCCACHE_SHA256_AMD64=aa73c7270c997894038c0d8f0d297865f57569e1687883322bc431713d4bb9dd
ARG SCCACHE_SHA256_ARM64=3e47fc4a71b1ddd1b4294a0de9d34498922fdf41e3cd0bda5df3954e3d6deb9d
RUN set -eux; arch="$(dpkg --print-architecture)"; case "$arch" in \
      amd64) a=x86_64; sum="$SCCACHE_SHA256_AMD64";; arm64) a=aarch64; sum="$SCCACHE_SHA256_ARM64";; esac; \
    d="sccache-v${SCCACHE_VERSION}-${a}-unknown-linux-musl"; cd /tmp; \
    curl -fsSLO "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/${d}.tar.gz"; \
    echo "${sum}  ${d}.tar.gz" | sha256sum -c -; \
    tar -xzf "${d}.tar.gz" --no-same-owner; install -m 755 "${d}/sccache" /usr/local/bin/sccache; \
    rm -rf "${d}" "${d}.tar.gz"; sccache --version

# The user every CLI runs as. sudo without a password: the VM is the boundary, not this user.
RUN userdel -r ubuntu 2>/dev/null || true; \
    useradd --create-home --uid 1000 --shell /bin/bash agent \
    && echo 'agent ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/agent && chmod 440 /etc/sudoers.d/agent \
    && git config --system init.defaultBranch main

# Mount points vm-init uses: the scratch disk, the overlay root, the old root after the
# pivot, and the tools disk.
RUN mkdir -p /mnt/scratch /mnt/newroot /mnt/oldroot /opt/runner

COPY vm-init /sbin/vm-init
RUN chmod 755 /sbin/vm-init
