#!/bin/bash
# Build RPM package for gst-plugin-zenoh
# Requires: Fedora/RHEL/Oracle Linux system with cargo-generate-rpm installed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

echo "Building RPM package for gst-plugin-zenoh v${VERSION}..."

# Check if we're on an RPM-based system
if ! command -v rpm &> /dev/null; then
    echo "Warning: rpm not found. This script works best on Fedora/RHEL/Oracle Linux."
    echo "Consider using ./scripts/docker-build.sh fedora instead."
fi

# Install cargo-generate-rpm if not present
if ! cargo generate-rpm --version &> /dev/null; then
    echo "Installing cargo-generate-rpm..."
    cargo install cargo-generate-rpm
fi

# Build release binary with all compression features
echo "Building release binary..."
cargo build --release --features compression

# Build the .rpm package
echo "Creating .rpm package..."
cargo generate-rpm

# Create dist directory and copy package
mkdir -p dist
cp target/generate-rpm/*.rpm dist/

echo ""
echo "RPM package created:"
ls -la dist/*.rpm

# Show package info
echo ""
echo "Package info:"
rpm -qip dist/*.rpm 2>/dev/null || true
