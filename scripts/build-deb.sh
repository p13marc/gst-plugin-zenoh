#!/bin/bash
# Build Debian package for gst-plugin-zenoh
# Requires: Debian or Ubuntu system with cargo-deb installed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

echo "Building Debian package for gst-plugin-zenoh v${VERSION}..."

# Check if we're on a Debian-based system
if ! command -v dpkg &> /dev/null; then
    echo "Warning: dpkg not found. This script works best on Debian/Ubuntu."
    echo "Consider using ./scripts/docker-build.sh debian instead."
fi

# Install cargo-deb if not present
if ! cargo deb --version &> /dev/null; then
    echo "Installing cargo-deb..."
    cargo install cargo-deb
fi

# Build release binary with all compression features
echo "Building release binary..."
cargo build --release --features compression

# Build the .deb package
echo "Creating .deb package..."
cargo deb --no-build

# Create dist directory and copy package
mkdir -p dist
cp target/debian/*.deb dist/

echo ""
echo "Debian package created:"
ls -la dist/*.deb

# Show package info
echo ""
echo "Package contents:"
dpkg-deb -c dist/*.deb 2>/dev/null || true
