#!/usr/bin/env bash
# Turns on .githooks (run by `bun install`). A worktree can carry its own core.hooksPath in
# config.worktree (some tools write the main checkout's absolute .githooks there); it wins over
# the repository's, and when that folder is missing or old git runs no hook at all. So set the
# relative path at both levels: git resolves it against each worktree's own root.
git config core.hooksPath .githooks || exit 0
if [ "$(git config --bool extensions.worktreeConfig 2>/dev/null)" = true ]; then
  git config --worktree core.hooksPath .githooks || true
fi
