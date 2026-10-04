#!/bin/bash
set -euo pipefail

RESOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$RESOURCE_DIR/bin"
LAUNCHD_DIR="$RESOURCE_DIR/launchd"
PREFIX="/usr/local/libexec/ntfs-manager"
CLI="/usr/local/bin/ntfs-manager"

HOME_NOW="${1:?missing user home}"
UID_NOW="${2:?missing user uid}"
GID_NOW="${3:?missing user gid}"
LAUNCH_AT_LOGIN="${4:-1}"

LAUNCH_AGENTS="$HOME_NOW/Library/LaunchAgents"
LOG_DIR="$HOME_NOW/Library/Logs/NTFS Manager"
STATE_DIR="$HOME_NOW/Library/Application Support/NTFS Manager"

if [[ "$(id -u)" -ne 0 ]]; then
    echo "repair-components.sh must run as root" >&2
    exit 1
fi

mkdir -p "$PREFIX" "$LAUNCH_AGENTS" "$LOG_DIR" "$STATE_DIR"

launchctl bootout system /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist 2>/dev/null || true
launchctl bootout "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist" 2>/dev/null || true

install -m 0755 "$BIN_DIR/ntfs-manager-helper" "$PREFIX/ntfs-manager-helper"
install -m 0755 "$BIN_DIR/ntfs-manager-agent" "$PREFIX/ntfs-manager-agent"
install -m 0755 "$BIN_DIR/ntfs-manager" "$CLI"

install -m 0644     "$LAUNCHD_DIR/dev.step2.ntfs-manager.helper.plist"     /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
chown root:wheel /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist

sed "s|__HOME__|$HOME_NOW|g"     "$LAUNCHD_DIR/dev.step2.ntfs-manager.agent.plist"     > "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"

if [[ "$LAUNCH_AT_LOGIN" == "1" ]]; then
    sed "s|__HOME__|$HOME_NOW|g"         "$LAUNCHD_DIR/dev.step2.ntfs-manager.menubar.plist"         > "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"
else
    rm -f "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"
fi

chown "$UID_NOW:$GID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"
if [[ -f "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" ]]; then
    chown "$UID_NOW:$GID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"
fi
chmod 0644 "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"
[[ ! -f "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" ]] || chmod 0644 "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"

plutil -lint /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist >/dev/null
plutil -lint "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist" >/dev/null
if [[ -f "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" ]]; then
    plutil -lint "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" >/dev/null
fi

launchctl bootstrap system /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
launchctl bootstrap "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"

touch "$STATE_DIR/installed-v0.4.0"
chown "$UID_NOW:$GID_NOW" "$STATE_DIR/installed-v0.4.0"

echo "NTFS Manager components repaired successfully."
