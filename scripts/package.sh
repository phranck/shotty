#!/usr/bin/env bash
# Packs a release build into dist/shotty-<version>-<target>.tar.gz with its SHA-256 next to it.
#
# Usage: scripts/package.sh <version> <target>
#
# The archive holds one directory with the binary, the man pages, the license and the README.
# Run it from the repository root after `cargo build --release --locked --target <target>`.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <target>" >&2
  exit 64
fi
version=$1
target=$2
binary="target/$target/release/shotty"
name="shotty-$version-$target"
stage="dist/$name"

if [[ ! -x $binary ]]; then
  echo "error: $binary does not exist; build it first" >&2
  exit 1
fi

rm -rf "$stage"
mkdir -p "$stage/man"
cp "$binary" LICENSE README.md "$stage/"

# Man pages are the same on every platform. A binary built for another architecture may not run
# on this machine, so the pages then come from a build for this machine.
if "$binary" version >/dev/null 2>&1; then
  "$binary" manpages "$stage/man"
else
  cargo run --release --locked --quiet -- manpages "$stage/man"
fi

tar -C dist -czf "dist/$name.tar.gz" "$name"
rm -rf "$stage"
(cd dist && shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256")
echo "dist/$name.tar.gz"
