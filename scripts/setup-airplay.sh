#!/bin/sh
# Install and configure shairport-sync for AirPlay audio streaming
#
# Usage:
#   sudo sh scripts/setup-airplay.sh [robot-name]
#
# This script:
# 1. Installs shairport-sync from apt
# 2. Configures it to output to the I2S audio card (MAX98357A)
# 3. Sets the AirPlay device name
# 4. Enables the service to start on boot
#
# After running, the robot appears as an AirPlay speaker on the network.
# iPhone/iPad/Mac users can select it from the AirPlay menu.
# Android users need a third-party app (e.g., AirBubble, AirMusic).

set -eu

SCRIPT_DIR="$(dirname "$0")"
AUDIO_DIR="$SCRIPT_DIR/../deploy/audio"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Check root
[ "$(id -u)" = 0 ] || die "run as root — re-run that same command with sudo"

# Determine robot name
ROBOT_NAME="${1:-}"
if [ -z "$ROBOT_NAME" ]; then
    # Try to read from /etc/hostname
    if [ -f /etc/hostname ]; then
        ROBOT_NAME="$(cat /etc/hostname)"
    else
        ROBOT_NAME="Microduck Robot"
    fi
fi
say "AirPlay device name: $ROBOT_NAME"

# Check if I2S audio is configured
if ! aplay -l 2>/dev/null | grep -q "i2saudio"; then
    warn "I2S audio card not found — did you run setup-i2s-audio.sh first?"
    warn "Continuing anyway — AirPlay will fail until the sound card is available"
fi

# Install shairport-sync
say "installing shairport-sync"
apt-get update -qq
apt-get install -y shairport-sync

# Install configuration file
say "installing configuration to /etc/shairport-sync.conf"
cp "$AUDIO_DIR/shairport-sync.conf" /etc/shairport-sync.conf

# Set the robot name in the config
# The config has: name = "Microduck Robot";
# We replace that with the actual name
say "setting AirPlay name to: $ROBOT_NAME"
# Escape any special characters in the name for sed
ESCAPED_NAME="$(printf '%s\n' "$ROBOT_NAME" | sed 's/[&/\]/\\&/g')"
sed -i "s/^\(\s*\)name = \"[^\"]*\"/\1name = \"$ESCAPED_NAME\"/" /etc/shairport-sync.conf

# Install systemd service override (optional — the apt package installs a default)
# We install our own to ensure it's configured correctly for the robot
say "installing systemd service"
cp "$AUDIO_DIR/systemd/shairport-sync.service" /etc/systemd/system/shairport-sync.service

# Reload systemd
say "reloading systemd"
systemctl daemon-reload

# Enable service
say "enabling shairport-sync service"
systemctl enable shairport-sync

# Start service
say "starting shairport-sync service"
systemctl restart shairport-sync || {
    warn "failed to start shairport-sync — check logs with: journalctl -u shairport-sync"
    warn "common causes: sound card not available, port 5353 already in use"
}

# Wait a moment and check status
sleep 2
if systemctl is-active --quiet shairport-sync; then
    say "shairport-sync is running"
    say ""
    say "the robot is now an AirPlay speaker on the network"
    say "  name: $ROBOT_NAME"
    say "  look for it in iPhone/iPad Control Center → AirPlay"
    say "  or on Mac: System Settings → Sound → Output"
else
    warn "shairport-sync failed to start — check logs:"
    warn "  journalctl -u shairport-sync -n 20"
fi

# Open firewall ports if ufw is active
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "active"; then
    say "opening AirPlay firewall ports"
    # mDNS
    ufw allow 5353/udp comment "AirPlay mDNS" >/dev/null 2>&1 || true
    # AirPlay control and data
    ufw allow 7000:7100/tcp comment "AirPlay control" >/dev/null 2>&1 || true
    ufw allow 6000:6010/udp comment "AirPlay data" >/dev/null 2>&1 || true
    say "firewall rules added"
fi

say ""
say "done. to uninstall:"
say "  sudo systemctl disable --now shairport-sync"
say "  sudo rm /etc/systemd/system/shairport-sync.service"
say "  sudo rm /etc/shairport-sync.conf"
say "  sudo apt remove shairport-sync"
