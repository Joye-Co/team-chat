#!/usr/bin/env bash
# check-release-digest.sh <tag> <file>...: checks each downloaded file against the SHA-256 digest
# GitHub keeps for the asset of the same name in release <tag> (no .sha256 files are uploaded).
set -euo pipefail

[ "$#" -ge 2 ] || { echo "usage: $0 <tag> <file>..." >&2; exit 2; }
tag="$1"; shift
assets="$(gh release view "$tag" --json assets -q '.assets[] | [.name, .digest] | @tsv')"

for f in "$@"; do
  name="$(basename "$f")"
  want="$(awk -F'\t' -v n="$name" '$1 == n { print $2 }' <<<"$assets")"
  [ -n "$want" ] || { echo "::error::release $tag has no digest for $name" >&2; exit 1; }
  if command -v sha256sum >/dev/null 2>&1; then have="$(sha256sum "$f")"; else have="$(shasum -a 256 "$f")"; fi
  have="sha256:${have%% *}"
  [ "$have" = "$want" ] || { echo "::error::$name does not match the digest of release $tag" >&2; exit 1; }
  echo "$name: OK"
done
