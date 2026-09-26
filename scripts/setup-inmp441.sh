#!/bin/sh
# Install INMP441 I2S MEMS microphone DKMS driver
#
# Usage:
#   sudo sh scripts/setup-inmp441.sh
#
# This script:
# 1. Copies the driver source to /usr/src/
# 2. Builds and installs via DKMS
# 3. Loads the module
# 4. Verifies installation

set -eu

DRIVER_NAME="inmp441"
DRIVER_VERSION="1.0"
SOURCE_DIR="$(dirname "$0")/../deploy/audio/inmp441-dkms"
TARGET_DIR="/usr/src/${DRIVER_NAME}-${DRIVER_VERSION}"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Check root
[ "$(id -u)" = 0 ] || die "run as root — re-run that same command with sudo"

# Check source directory
[ -d "$SOURCE_DIR" ] || die "source directory not found: $SOURCE_DIR"
[ -f "$SOURCE_DIR/inmp441.c" ] || die "inmp441.c not found in $SOURCE_DIR"
[ -f "$SOURCE_DIR/Makefile" ] || die "Makefile not found in $SOURCE_DIR"
[ -f "$SOURCE_DIR/dkms.conf" ] || die "dkms.conf not found in $SOURCE_DIR"

# Check DKMS
command -v dkms >/dev/null 2>&1 || die "dkms not found — install it first: sudo apt install dkms"

# Check kernel headers
[ -d "/lib/modules/$(uname -r)/build" ] || die "kernel headers not found for $(uname -r)"

# Copy source to /usr/src/
say "copying driver source to $TARGET_DIR"
rm -rf "$TARGET_DIR"
mkdir -p "$TARGET_DIR"
cp -r "$SOURCE_DIR"/* "$TARGET_DIR"/

# Install via DKMS
say "installing via DKMS"
if dkms status "$DRIVER_NAME" 2>/dev/null | grep -q "$DRIVER_NAME"; then
    say "removing existing DKMS module"
    dkms remove "$DRIVER_NAME/$DRIVER_VERSION" --all 2>/dev/null || true
fi

dkms add -m "$DRIVER_NAME" -v "$DRIVER_VERSION"
dkms build -m "$DRIVER_NAME" -v "$DRIVER_VERSION"
dkms install -m "$DRIVER_NAME" -v "$DRIVER_VERSION"

# Load module
say "loading module"
modprobe "$DRIVER_NAME" || die "failed to load module"

# Verify
say "verifying installation"
if lsmod | grep -q "$DRIVER_NAME"; then
    say "module loaded successfully"
    lsmod | grep "$DRIVER_NAME"
else
    die "module not loaded"
fi

# Check dmesg for errors
say "checking dmesg"
dmesg | tail -20 | grep -i "inmp441\|error\|fail" || true

say "installation complete"
echo ""
echo "Next steps:"
echo "  1. Compile and install the device tree overlay:"
echo "     sudo dtc -@ -I dts -O dtb -o /boot/dtbo/max98357a-inmp441.dtbo deploy/audio/max98357a-inmp441.dts"
echo "     sudo sed -i 's/^overlays=.*/overlays=max98357a-inmp441/' /boot/armbianEnv.txt"
echo "     sudo reboot"
echo ""
echo "  2. After reboot, test recording:"
echo "     arecord -D plughw:i2saudio,0 -f S16_LE -r 16000 -c 1 -d 3 test.wav"
echo "     aplay test.wav"
