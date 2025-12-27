#!/bin/bash
# Build source tarball for gst-plugin-zenoh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
PACKAGE_NAME="gst-plugin-zenoh-${VERSION}-src"

echo "Creating source tarball for gst-plugin-zenoh v${VERSION}..."

# Create dist directory
mkdir -p dist

# Use git archive if in a git repository
if [ -d .git ]; then
    TARBALL="dist/${PACKAGE_NAME}.tar.gz"
    git archive --format=tar.gz --prefix="${PACKAGE_NAME}/" -o "$TARBALL" HEAD
    echo ""
    echo "Source tarball created: $TARBALL"
    echo "Contents:"
    tar -tzf "$TARBALL" | head -20
    echo "..."
else
    # Fallback: manual collection
    STAGING_DIR=$(mktemp -d)
    PACKAGE_DIR="${STAGING_DIR}/${PACKAGE_NAME}"
    mkdir -p "$PACKAGE_DIR"

    # Copy source files
    cp -r src "$PACKAGE_DIR/"
    cp -r tests "$PACKAGE_DIR/" 2>/dev/null || true
    cp Cargo.toml Cargo.lock "$PACKAGE_DIR/"
    cp build.rs "$PACKAGE_DIR/"
    cp README.md LICENSE CHANGELOG.md "$PACKAGE_DIR/"
    cp -r scripts "$PACKAGE_DIR/" 2>/dev/null || true
    cp -r docker "$PACKAGE_DIR/" 2>/dev/null || true

    TARBALL="dist/${PACKAGE_NAME}.tar.gz"
    tar -czf "$TARBALL" -C "$STAGING_DIR" "$PACKAGE_NAME"

    rm -rf "$STAGING_DIR"

    echo ""
    echo "Source tarball created: $TARBALL"
fi
