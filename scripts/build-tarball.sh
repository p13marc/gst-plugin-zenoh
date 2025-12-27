#!/bin/bash
# Build binary tarball for gst-plugin-zenoh
# Works on any Linux distribution

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
ARCH=$(uname -m)
PACKAGE_NAME="gst-plugin-zenoh-${VERSION}-linux-${ARCH}"

echo "Building gst-plugin-zenoh v${VERSION} for ${ARCH}..."

# Build release binary with all compression features
cargo build --release --features compression

# Create dist directory
mkdir -p dist

# Create package directory structure
STAGING_DIR=$(mktemp -d)
PACKAGE_DIR="${STAGING_DIR}/${PACKAGE_NAME}"
mkdir -p "${PACKAGE_DIR}/lib/gstreamer-1.0"
mkdir -p "${PACKAGE_DIR}/doc"

# Copy files
cp target/release/libgstzenoh.so "${PACKAGE_DIR}/lib/gstreamer-1.0/"
cp README.md "${PACKAGE_DIR}/doc/"
cp LICENSE "${PACKAGE_DIR}/doc/"
cp CHANGELOG.md "${PACKAGE_DIR}/doc/"

# Create install script
cat > "${PACKAGE_DIR}/install.sh" << 'EOF'
#!/bin/bash
# Install gst-plugin-zenoh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Detect GStreamer plugin directory
if command -v pkg-config &> /dev/null; then
    GST_PLUGIN_DIR=$(pkg-config --variable=pluginsdir gstreamer-1.0 2>/dev/null || echo "")
fi

if [ -z "${GST_PLUGIN_DIR:-}" ]; then
    # Fallback detection
    if [ -d "/usr/lib64/gstreamer-1.0" ]; then
        GST_PLUGIN_DIR="/usr/lib64/gstreamer-1.0"
    elif [ -d "/usr/lib/x86_64-linux-gnu/gstreamer-1.0" ]; then
        GST_PLUGIN_DIR="/usr/lib/x86_64-linux-gnu/gstreamer-1.0"
    elif [ -d "/usr/lib/gstreamer-1.0" ]; then
        GST_PLUGIN_DIR="/usr/lib/gstreamer-1.0"
    else
        echo "Error: Could not detect GStreamer plugin directory"
        echo "Please set GST_PLUGIN_DIR environment variable"
        exit 1
    fi
fi

echo "Installing to ${GST_PLUGIN_DIR}..."
sudo cp "${SCRIPT_DIR}/lib/gstreamer-1.0/libgstzenoh.so" "${GST_PLUGIN_DIR}/"
sudo chmod 755 "${GST_PLUGIN_DIR}/libgstzenoh.so"

# Install docs
sudo mkdir -p /usr/share/doc/gst-plugin-zenoh
sudo cp "${SCRIPT_DIR}/doc/"* /usr/share/doc/gst-plugin-zenoh/

echo "Installation complete!"
echo "Verify with: gst-inspect-1.0 zenohsink"
EOF
chmod +x "${PACKAGE_DIR}/install.sh"

# Create uninstall script
cat > "${PACKAGE_DIR}/uninstall.sh" << 'EOF'
#!/bin/bash
# Uninstall gst-plugin-zenoh

set -euo pipefail

# Detect GStreamer plugin directory
if command -v pkg-config &> /dev/null; then
    GST_PLUGIN_DIR=$(pkg-config --variable=pluginsdir gstreamer-1.0 2>/dev/null || echo "")
fi

if [ -z "${GST_PLUGIN_DIR:-}" ]; then
    if [ -d "/usr/lib64/gstreamer-1.0" ]; then
        GST_PLUGIN_DIR="/usr/lib64/gstreamer-1.0"
    elif [ -d "/usr/lib/x86_64-linux-gnu/gstreamer-1.0" ]; then
        GST_PLUGIN_DIR="/usr/lib/x86_64-linux-gnu/gstreamer-1.0"
    elif [ -d "/usr/lib/gstreamer-1.0" ]; then
        GST_PLUGIN_DIR="/usr/lib/gstreamer-1.0"
    fi
fi

echo "Removing plugin..."
sudo rm -f "${GST_PLUGIN_DIR}/libgstzenoh.so"
sudo rm -rf /usr/share/doc/gst-plugin-zenoh

echo "Uninstallation complete!"
EOF
chmod +x "${PACKAGE_DIR}/uninstall.sh"

# Create tarball
TARBALL="dist/${PACKAGE_NAME}.tar.gz"
tar -czf "$TARBALL" -C "$STAGING_DIR" "$PACKAGE_NAME"

# Cleanup
rm -rf "$STAGING_DIR"

echo ""
echo "Binary tarball created: $TARBALL"
echo "Contents:"
tar -tzf "$TARBALL"
