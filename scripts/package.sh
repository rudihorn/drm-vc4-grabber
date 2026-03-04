#!/usr/bin/env bash
# Package built binaries into release archives.
# Expects one or more bins-<platform>/ directories to exist (created by build.sh).
#
# Usage:
#   TAG=v0.1.0 ./scripts/package.sh
#   ./scripts/package.sh          # uses TAG=dev

set -euo pipefail

BIN_NAME="${BIN_NAME:-drm-vc4-grabber}"
PROJECT_NAME="${PROJECT_NAME:-drm-vc4-grabber}"
TAG="${TAG:-dev}"

rm -rf tmp
mkdir tmp
mkdir -p dist

for dir in bins-*; do
    platform="${dir#"bins-"}"
    pkgname="$PROJECT_NAME-$TAG-$platform"
    mkdir "tmp/$pkgname"
    cp "$dir/$BIN_NAME" "tmp/$pkgname/"
    chmod +x "tmp/$pkgname/$BIN_NAME"
    tar cJf "dist/$pkgname.tar.xz" -C tmp "$pkgname"
    echo "Package: dist/$pkgname.tar.xz"
done

rm -rf tmp
