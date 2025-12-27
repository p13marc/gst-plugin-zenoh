#!/bin/bash
# Main orchestration script for building gst-plugin-zenoh packages
# Supports: --deb, --rpm, --tarball, --source, --all

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Build packages for gst-plugin-zenoh v${VERSION}

Options:
    --deb       Build Debian package (.deb)
    --rpm       Build RPM package (.rpm)
    --tarball   Build binary tarball (.tar.gz)
    --source    Build source tarball (.tar.gz)
    --all       Build all package types
    --clean     Clean dist directory before building
    -h, --help  Show this help message

Examples:
    $(basename "$0") --tarball          # Build binary tarball only
    $(basename "$0") --deb --rpm        # Build both .deb and .rpm
    $(basename "$0") --all              # Build everything
    $(basename "$0") --clean --all      # Clean and build everything

Notes:
    - .deb requires Debian/Ubuntu or use: ./scripts/docker-build.sh debian
    - .rpm requires Fedora/RHEL or use: ./scripts/docker-build.sh fedora
    - Tarballs work on any Linux distribution
EOF
}

BUILD_DEB=false
BUILD_RPM=false
BUILD_TARBALL=false
BUILD_SOURCE=false
CLEAN=false

# Parse arguments
if [ $# -eq 0 ]; then
    usage
    exit 1
fi

while [ $# -gt 0 ]; do
    case "$1" in
        --deb)
            BUILD_DEB=true
            ;;
        --rpm)
            BUILD_RPM=true
            ;;
        --tarball)
            BUILD_TARBALL=true
            ;;
        --source)
            BUILD_SOURCE=true
            ;;
        --all)
            BUILD_DEB=true
            BUILD_RPM=true
            BUILD_TARBALL=true
            BUILD_SOURCE=true
            ;;
        --clean)
            CLEAN=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
    shift
done

# Clean if requested
if [ "$CLEAN" = true ]; then
    echo "Cleaning dist directory..."
    rm -rf dist
fi

# Create dist directory
mkdir -p dist

# Track success/failure
FAILED=()

# Build requested packages
if [ "$BUILD_TARBALL" = true ]; then
    echo ""
    echo "=========================================="
    echo "Building binary tarball..."
    echo "=========================================="
    if "${SCRIPT_DIR}/build-tarball.sh"; then
        echo "Binary tarball: SUCCESS"
    else
        echo "Binary tarball: FAILED"
        FAILED+=("tarball")
    fi
fi

if [ "$BUILD_SOURCE" = true ]; then
    echo ""
    echo "=========================================="
    echo "Building source tarball..."
    echo "=========================================="
    if "${SCRIPT_DIR}/build-source-tarball.sh"; then
        echo "Source tarball: SUCCESS"
    else
        echo "Source tarball: FAILED"
        FAILED+=("source")
    fi
fi

if [ "$BUILD_DEB" = true ]; then
    echo ""
    echo "=========================================="
    echo "Building Debian package..."
    echo "=========================================="
    if "${SCRIPT_DIR}/build-deb.sh"; then
        echo "Debian package: SUCCESS"
    else
        echo "Debian package: FAILED"
        echo "Try: ./scripts/docker-build.sh debian"
        FAILED+=("deb")
    fi
fi

if [ "$BUILD_RPM" = true ]; then
    echo ""
    echo "=========================================="
    echo "Building RPM package..."
    echo "=========================================="
    if "${SCRIPT_DIR}/build-rpm.sh"; then
        echo "RPM package: SUCCESS"
    else
        echo "RPM package: FAILED"
        echo "Try: ./scripts/docker-build.sh fedora"
        FAILED+=("rpm")
    fi
fi

# Summary
echo ""
echo "=========================================="
echo "Build Summary"
echo "=========================================="
echo "Output directory: dist/"
ls -la dist/ 2>/dev/null || echo "(empty)"

if [ ${#FAILED[@]} -gt 0 ]; then
    echo ""
    echo "Failed packages: ${FAILED[*]}"
    exit 1
else
    echo ""
    echo "All requested packages built successfully!"
fi
