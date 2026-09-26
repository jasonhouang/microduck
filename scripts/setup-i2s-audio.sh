#!/bin/sh
# Install I2S audio drivers for MAX98357A (speaker) + INMP441 (microphone)
#
# Usage:
#   sudo sh scripts/setup-i2s-audio.sh
#
# This script:
# 1. Installs DKMS modules for MAX98357A and INMP441 codecs
# 2. Compiles and installs the device tree overlay
# 3. Configures boot to load the overlay

set -eu

SCRIPT_DIR="$(dirname "$0")"
AUDIO_DIR="$SCRIPT_DIR/../deploy/audio"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Check root
[ "$(id -u)" = 0 ] || die "run as root — re-run that same command with sudo"

# Check DKMS
command -v dkms >/dev/null 2>&1 || die "dkms not found — install it first: sudo apt install dkms"

# Check kernel headers
[ -d "/lib/modules/$(uname -r)/build" ] || die "kernel headers not found for $(uname -r)"

install_dkms_module() {
    local name="$1"
    local version="$2"
    local source_dir="$3"
    local mod_name="snd-soc-${name}"

    say "installing $name-$version"

    [ -d "$source_dir" ] || die "source directory not found: $source_dir"

    local target_dir="/usr/src/${name}-${version}"

    # Copy source
    say "  copying to $target_dir"
    rm -rf "$target_dir"
    mkdir -p "$target_dir"
    cp -r "$source_dir"/* "$target_dir"/

    # Remove existing if present
    if dkms status "$name" 2>/dev/null | grep -q "$name"; then
        say "  removing existing DKMS module"
        dkms remove "$name/$version" --all 2>/dev/null || true
    fi

    # Install via DKMS
    say "  adding to DKMS"
    dkms add -m "$name" -v "$version"
    say "  building"
    dkms build -m "$name" -v "$version"
    say "  installing"
    dkms install -m "$name" -v "$version"

    # Load module
    say "  loading module"
    modprobe "$mod_name" || warn "failed to load $mod_name (may need reboot)"

    # Verify
    if lsmod | grep -q "^snd_soc_${name//-/_}"; then
        say "  $mod_name loaded successfully"
    else
        warn "  $mod_name not loaded yet (may load after reboot)"
    fi
}

# Install codec modules
install_dkms_module "max98357a" "1.0" "$AUDIO_DIR/max98357a-dkms"
install_dkms_module "inmp441" "1.0" "$AUDIO_DIR/inmp441-dkms"

# Fix base DTB: remove i2s-lrck-gpio and add correct clock parents for I2S3
fix_base_dtb() {
    local dtb="$1"
    local marker="$dtb.i2s-audio-patched"

    # Skip if already patched
    if [ -f "$marker" ]; then
        say "base DTB already patched"
        return 0
    fi

    say "patching base DTB: $dtb"

    # Backup original (only first time)
    local orig="${dtb}.orig"
    if [ ! -f "$orig" ]; then
        cp "$dtb" "$orig"
        say "  backed up to $orig"
    fi

    # Decompile
    local dts_tmp="/tmp/base-audio.dts"
    dtc -I dtb -O dts -o "$dts_tmp" "$dtb" 2>/dev/null || die "failed to decompile DTB"

    # Find I2S3 node line (i2s@fe430000, skip aliases/__symbols__ references)
    local i2s_line
    i2s_line=$(grep -n 'i2s@fe430000 {' "$dts_tmp" | grep -v aliases | grep -v __symbols__ | head -1 | cut -d: -f1)
    [ -n "$i2s_line" ] || die "I2S3 node not found in DTB"
    say "  I2S3 node at line $i2s_line"

    # Remove i2s-lrck-gpio within I2S3 node (next ~25 lines)
    local end_line=$((i2s_line + 25))
    if sed -n "${i2s_line},${end_line}p" "$dts_tmp" | grep -q 'i2s-lrck-gpio'; then
        sed -i "${i2s_line},${end_line} { /i2s-lrck-gpio/d; }" "$dts_tmp"
        say "  removed i2s-lrck-gpio"
    fi

    # Add assigned-clock properties after clock-names line
    local clk_names_line
    clk_names_line=$(sed -n "${i2s_line},${end_line}p" "$dts_tmp" | grep -n 'clock-names' | head -1 | cut -d: -f1)
    if [ -n "$clk_names_line" ]; then
        local insert_line=$((i2s_line + clk_names_line))
        sed -i "${insert_line}a\\		assigned-clocks = <0x24 0x53 0x24 0x57>;\n\t\tassigned-clock-parents = <0x133 0x133>;\n\t\tassigned-clock-rates = <0xbb8000 0xbb8000>;" "$dts_tmp"
        say "  added assigned-clock-parents (i2s3_mclkin 12.288MHz)"
    fi

    # Recompile
    dtc -@ -I dts -O dtb -o "$dtb" "$dts_tmp" 2>/dev/null || die "failed to recompile DTB"
    rm -f "$dts_tmp"

    # Mark as patched
    touch "$marker"
    say "  DTB patched successfully"
}

# Find base DTB
BASE_DTB=""
for candidate in /boot/dtb-*/rockchip/rk3566-radxa-zero3.dtb; do
    [ -f "$candidate" ] && BASE_DTB="$candidate" && break
done
[ -n "$BASE_DTB" ] || die "base DTB not found (expected /boot/dtb-*/rockchip/rk3566-radxa-zero3.dtb)"

fix_base_dtb "$BASE_DTB"

# Compile device tree overlay
say "compiling device tree overlay"
DTS_FILE="$AUDIO_DIR/max98357a-inmp441.dts"
DTBO_FILE="/boot/dtbo/max98357a-inmp441.dtbo"

[ -f "$DTS_FILE" ] || die "device tree source not found: $DTS_FILE"

mkdir -p /boot/dtbo
dtc -@ -I dts -O dtb -o "$DTBO_FILE" "$DTS_FILE" || die "failed to compile device tree"
say "device tree compiled to $DTBO_FILE"

# Copy to overlay directories
say "installing overlay to boot directories"
cp "$DTBO_FILE" /boot/dtb/rockchip/overlay/ 2>/dev/null || true
mkdir -p /boot/overlay-user
cp "$DTBO_FILE" /boot/overlay-user/

# Configure boot to load overlay
say "configuring boot overlay"
if [ -f /boot/armbianEnv.txt ]; then
    # Check if user_overlays exists
    if grep -q "^user_overlays=" /boot/armbianEnv.txt; then
        # Check if our overlay is already in the list
        if ! grep -q "max98357a-inmp441" /boot/armbianEnv.txt; then
            # Add to existing user_overlays
            sed -i 's/^user_overlays=\(.*\)/user_overlays=\1 max98357a-inmp441/' /boot/armbianEnv.txt
            say "  added max98357a-inmp441 to user_overlays"
        else
            say "  overlay already configured"
        fi
    else
        echo "user_overlays=max98357a-inmp441" >> /boot/armbianEnv.txt
        say "  created user_overlays entry"
    fi
else
    warn "/boot/armbianEnv.txt not found - you may need to manually configure boot overlay"
fi

install_i2s_pm_fix() {
    # Prevent I2S3 runtime suspend — keeps it powered on to avoid click/pop
    # noise at end of playback (rockchip-i2s-tdm PM resume is broken)
    local rule_file="/etc/udev/rules.d/99-i2s-audio-pm.rules"
    if [ -f "$rule_file" ]; then
        say "I2S PM fix already installed"
        return 0
    fi
    say "installing I2S PM fix (prevent runtime suspend)"
    cat > "$rule_file" << 'EOF'
# Keep I2S3 (fe430000.i2s) powered on at all times.
# rockchip-i2s-tdm runtime PM resume causes click/pop noise.
ACTION=="add", SUBSYSTEM=="platform", KERNEL=="fe430000.i2s", ATTR{power/control}="on"
EOF
    udevadm control --reload-rules 2>/dev/null || true
    # Apply immediately for current session
    local pm_path="/sys/devices/platform/fe430000.i2s/power/control"
    if [ -f "$pm_path" ]; then
        echo on > "$pm_path" || true
    fi
}

install_pipewire_audio() {
    # Install PipeWire for audio routing. pro-audio profile exposes both
    # playback and capture on the I2S card simultaneously.
    # Echo cancellation is left to the application layer (e.g. WebRTC).
    say "installing PipeWire audio"

    apt-get install -y pipewire pipewire-alsa wireplumber >/dev/null 2>&1 || \
        warn "failed to install pipewire packages"

    local pw_uid
    pw_uid=$(id -u "${SUDO_USER:-$USER}" 2>/dev/null || echo 1000)
    local pw_user
    pw_user=$(getent passwd "$pw_uid" | cut -d: -f1)

    # Init script: set pro-audio profile + mic gain
    cat > /usr/local/bin/i2s-pipewire-init.sh << INITEOF
#!/bin/sh
# Wait for PipeWire + WirePlumber to fully start
sleep 5
export XDG_RUNTIME_DIR=/run/user/$pw_uid

# Set pro-audio profile on the I2S card (both playback+capture exposed)
ID=\$(pw-dump 2>/dev/null | python3 -c "
import json,sys
for item in json.load(sys.stdin):
    name = item.get('info',{}).get('props',{}).get('device.name','')
    if 'i2s' in name:
        print(item.get('id',''))
        break
" 2>/dev/null)
[ -n "\$ID" ] && pw-cli set-param "\$ID" Profile '{"index":2}' 2>/dev/null

# Set mic gain: hardware max (255) + software 5x boost via PipeWire
amixer -c 1 cset numid=12 255 >/dev/null 2>&1 || true
sleep 2
wpctl set-volume @DEFAULT_AUDIO_SOURCE@ 5.0 2>/dev/null || true
INITEOF
    chmod +x /usr/local/bin/i2s-pipewire-init.sh

    # User-level services via lingering (no login required to start audio)
    loginctl enable-linger "$pw_user" 2>/dev/null || true

    # Enable user-level PipeWire + WirePlumber
    sudo -u "$pw_user" env \
        XDG_RUNTIME_DIR=/run/user/"$pw_uid" \
        DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/"$pw_uid"/bus \
        systemctl --user enable pipewire pipewire.socket wireplumber 2>/dev/null || {
        # Fallback: create symlinks directly if systemctl --user fails
        local wants="/home/$pw_user/.config/systemd/user/default.target.wants"
        mkdir -p "$wants"
        ln -sf /usr/lib/systemd/user/pipewire.service "$wants/" 2>/dev/null || true
        ln -sf /usr/lib/systemd/user/pipewire.socket "$wants/" 2>/dev/null || true
        mkdir -p "/home/$pw_user/.config/systemd/user/pipewire.service.wants"
        ln -sf /usr/lib/systemd/user/wireplumber.service \
            "/home/$pw_user/.config/systemd/user/pipewire.service.wants/" 2>/dev/null || true
        chown -R "$pw_user:$pw_user" "/home/$pw_user/.config/systemd"
    }

    # User-level init service
    local user_systemd="/home/$pw_user/.config/systemd/user"
    mkdir -p "$user_systemd"
    cat > "$user_systemd/i2s-audio-init.service" << 'SVCEOF'
[Unit]
Description=Initialize I2S audio (pro-audio profile + mic gain)
After=wireplumber.service
Requires=wireplumber.service
[Service]
Type=oneshot
ExecStart=/usr/local/bin/i2s-pipewire-init.sh
RemainAfterExit=yes
[Install]
WantedBy=default.target
SVCEOF
    chown "$pw_user:$pw_user" "$user_systemd/i2s-audio-init.service"

    sudo -u "$pw_user" env \
        XDG_RUNTIME_DIR=/run/user/"$pw_uid" \
        DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/"$pw_uid"/bus \
        systemctl --user enable i2s-audio-init.service 2>/dev/null || {
        mkdir -p "/home/$pw_user/.config/systemd/user/default.target.wants"
        ln -sf "../i2s-audio-init.service" \
            "/home/$pw_user/.config/systemd/user/default.target.wants/" 2>/dev/null || true
        chown -R "$pw_user:$pw_user" "/home/$pw_user/.config/systemd"
    }

    # Clean up any leftover AEC services/configs from previous versions
    systemctl disable i2s-aec-init.service 2>/dev/null || true
    rm -f "/home/$pw_user/.config/systemd/user/i2s-aec-init.service" \
          "/home/$pw_user/.config/systemd/user/default.target.wants/i2s-aec-init.service"
    rm -f "/home/$pw_user/.config/pipewire/pipewire.conf.d/99-echo-cancel.conf"
    rm -f /etc/asound.conf
    # Clean up legacy system-level services
    systemctl disable i2s-audio-pipewire.service \
                      i2s-audio-wireplumber.service \
                      i2s-audio-aec-init.service 2>/dev/null || true
    rm -f /etc/systemd/system/i2s-audio-pipewire.service \
          /etc/systemd/system/i2s-audio-wireplumber.service \
          /etc/systemd/system/i2s-audio-aec-init.service
    systemctl daemon-reload 2>/dev/null || true
}

install_i2s_pm_fix
install_pipewire_audio

# Clean up legacy SD GPIO systemd service if present (now handled by the
# max98357a driver via sdmode-gpios in the device tree overlay)
if [ -f /etc/systemd/system/i2s-audio-sd-pin.service ]; then
    systemctl stop i2s-audio-sd-pin.service 2>/dev/null || true
    systemctl disable i2s-audio-sd-pin.service 2>/dev/null || true
    rm -f /etc/systemd/system/i2s-audio-sd-pin.service
    systemctl daemon-reload 2>/dev/null || true
    say "removed legacy SD GPIO service (now in driver)"
fi

say "installation complete"
echo ""
echo "Next steps:"
echo "  1. Reboot the system: sudo reboot"
echo ""
echo "  2. After reboot, verify sound card:"
echo "     cat /proc/asound/cards"
echo "     # You should see 'i2s-audio' card"
echo ""
echo "  3. Test playback (speaker):"
echo "     speaker-test -D hw:i2saudio,1 -t sine -f 440 -c 2 -l 1"
echo ""
echo "  4. Test recording (microphone):"
echo "     arecord -D hw:i2saudio,0 -f S16_LE -r 16000 -c 2 -d 3 test.wav"
echo "     aplay test.wav"
