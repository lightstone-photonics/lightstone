#!/usr/bin/env bash
# Pull the repository and, if present, the private notes in internal/ (a separate repository,
# ignored by this one). Maintainers only; everyone else just uses `git pull`.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
git -C "$root" pull --ff-only
if [ -d "$root/internal/.git" ]; then
  git -C "$root/internal" pull --ff-only
fi
