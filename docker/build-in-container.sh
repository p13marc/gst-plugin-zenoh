#!/bin/bash
# Build script that runs inside Docker containers
# Called by Dockerfiles with argument: deb or rpm

set -euo pipefail

BUILD_TYPE="${1:-}"

if [ -z "$BUILD_TYPE" ]; then
    echo "Usage: $0 <deb|rpm>"
    exit 1
fi

# Copy source to build directory (source is mounted read-only)
echo "Copying source files..."
cp -r /src/* /build/
cd /build

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Building gst-plugin-zenoh v${VERSION}..."

# Build release binary with all compression features
echo "Building release binary..."
cargo build --release --features compression

case "$BUILD_TYPE" in
    deb)
        echo "Creating .deb package..."
        cargo deb --no-build

        # Copy to output directory
        cp target/debian/*.deb /dist/
        echo "Package created:"
        ls -la /dist/*.deb
        ;;
    rpm)
        echo "Creating .rpm package..."
        cargo generate-rpm

        # Copy to output directory
        cp target/generate-rpm/*.rpm /dist/
        echo "Package created:"
        ls -la /dist/*.rpm
        ;;
    *)
        echo "Unknown build type: $BUILD_TYPE"
        exit 1
        ;;
esac

echo "Build complete!"
