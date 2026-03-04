#!/usr/bin/env bash
# Cross-compile drm-vc4-grabber for the target platform.
#
# Prerequisites: cargo install cross  (and Docker must be running)
#
# Usage:
#   ./scripts/build.sh
#   TARGET=aarch64-unknown-linux-musl PLATFORM=aarch64-linux ./scripts/build.sh

set -euo pipefail

BIN_NAME="${BIN_NAME:-drm-vc4-grabber}"
TARGET="${TARGET:-aarch64-unknown-linux-musl}"
PLATFORM="${PLATFORM:-aarch64-linux}"

cross test  --release --locked --target "$TARGET"
cross build --release --locked --target "$TARGET"

mkdir -p "bins-$PLATFORM"
cp "target/$TARGET/release/$BIN_NAME" "bins-$PLATFORM/"

echo "Binary: bins-$PLATFORM/$BIN_NAME"
