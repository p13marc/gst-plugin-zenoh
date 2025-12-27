#!/bin/bash
# Docker/Podman-based cross-distribution package builder for gst-plugin-zenoh
# Builds packages in isolated containers for different distributions

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DOCKER_DIR="${PROJECT_DIR}/docker"

cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

# Detect container runtime (prefer podman on Fedora, fall back to docker)
CONTAINER_CMD=""
detect_container_runtime() {
    if command -v podman &> /dev/null; then
        CONTAINER_CMD="podman"
    elif command -v docker &> /dev/null; then
        CONTAINER_CMD="docker"
    else
        echo "Error: Neither podman nor docker is installed"
        echo "Install podman: sudo dnf install podman"
        echo "Or install Docker: https://docs.docker.com/get-docker/"
        exit 1
    fi
    echo "Using container runtime: $CONTAINER_CMD"
}

usage() {
    cat << EOF
Usage: $(basename "$0") <target> [OPTIONS]

Build gst-plugin-zenoh packages using Docker/Podman containers

Targets:
    debian      Build .deb on Debian 12 (Bookworm)
    ubuntu      Build .deb on Ubuntu 24.04 LTS
    fedora      Build .rpm on Fedora 41
    oracle      Build .rpm on Oracle Linux 9
    all         Build all packages

Options:
    --no-cache  Build container images without cache
    -h, --help  Show this help message

Examples:
    $(basename "$0") debian             # Build .deb in Debian container
    $(basename "$0") fedora             # Build .rpm in Fedora container
    $(basename "$0") all                # Build all packages
    $(basename "$0") debian --no-cache  # Rebuild image from scratch
EOF
}

# Check container runtime is available
check_container_runtime() {
    detect_container_runtime

    if [ "$CONTAINER_CMD" = "docker" ]; then
        if ! docker info &> /dev/null; then
            echo "Error: Docker daemon is not running or you don't have permission"
            echo "Try: sudo systemctl start docker"
            echo "Or add your user to docker group: sudo usermod -aG docker \$USER"
            exit 1
        fi
    fi
}

# Build and run container for a target
build_target() {
    local target="$1"
    local dockerfile="${DOCKER_DIR}/Dockerfile.${target}"
    local image_name="gst-plugin-zenoh-builder-${target}"

    if [ ! -f "$dockerfile" ]; then
        echo "Error: Dockerfile not found: $dockerfile"
        exit 1
    fi

    echo ""
    echo "=========================================="
    echo "Building for ${target}..."
    echo "=========================================="

    # Build the container image
    echo "Building container image: ${image_name}"
    $CONTAINER_CMD build ${NO_CACHE:-} -t "$image_name" -f "$dockerfile" "$DOCKER_DIR"

    # Create dist directory
    mkdir -p dist

    # Run the container
    echo "Running build in container..."
    $CONTAINER_CMD run --rm \
        -v "${PROJECT_DIR}:/src:ro" \
        -v "${PROJECT_DIR}/dist:/dist:Z" \
        "$image_name"

    echo "Build complete for ${target}"
}

# Parse arguments
if [ $# -eq 0 ]; then
    usage
    exit 1
fi

TARGET=""
NO_CACHE=""

while [ $# -gt 0 ]; do
    case "$1" in
        debian|ubuntu|fedora|oracle|all)
            TARGET="$1"
            ;;
        --no-cache)
            NO_CACHE="--no-cache"
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

if [ -z "$TARGET" ]; then
    echo "Error: No target specified"
    usage
    exit 1
fi

# Check container runtime is available
check_container_runtime

# Build requested targets
case "$TARGET" in
    debian)
        build_target debian
        ;;
    ubuntu)
        build_target ubuntu
        ;;
    fedora)
        build_target fedora
        ;;
    oracle)
        build_target oracle
        ;;
    all)
        build_target debian
        build_target ubuntu
        build_target fedora
        build_target oracle
        ;;
esac

echo ""
echo "=========================================="
echo "Build Summary"
echo "=========================================="
echo "Output directory: dist/"
ls -la dist/
