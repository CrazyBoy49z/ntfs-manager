#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="/usr/local/libexec/ntfs-manager"
CLI="/usr/local/bin/ntfs-manager"
APP="/Applications/NTFS Manager.app"
UID_NOW="$(id -u)"
HOME_NOW="$HOME"
LAUNCH_AGENTS="$HOME_NOW/Library/LaunchAgents"
LOG_DIR="$HOME_NOW/Library/Logs/NTFS Manager"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "NTFS Manager installer requires macOS" >&2
    exit 1
fi

if ! groups "$(id -un)" | tr ' ' '\n' | grep -qx admin; then
    echo "Current user must be in the macOS admin group to access the privileged helper." >&2
    exit 1
fi

if [[ ! -d /Library/Filesystems/macfuse.fs ]]; then
    echo "macFUSE is not installed. Install it first:" >&2
    echo "  brew install --cask macfuse" >&2
    exit 1
fi

if [[ ! -x /usr/local/bin/ntfs-3g && ! -x /usr/local/sbin/ntfs-3g && ! -x /opt/homebrew/bin/ntfs-3g && ! -x /opt/homebrew/sbin/ntfs-3g ]]; then
    echo "ntfs-3g was not found. Install it first:" >&2
    echo "  brew tap gromgit/fuse" >&2
    echo "  brew install gromgit/fuse/ntfs-3g-mac" >&2
    exit 1
fi

cd "$ROOT"
cargo build --release --all-features
bash "$ROOT/scripts/package-app.sh"

mkdir -p "$LAUNCH_AGENTS" "$LOG_DIR"

launchctl bootout "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist" 2>/dev/null || true
launchctl bootout "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" 2>/dev/null || true
sudo launchctl bootout system /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist 2>/dev/null || true

sudo install -d -m 0755 "$PREFIX"
sudo install -m 0755 "$ROOT/target/release/ntfs-manager-helper" "$PREFIX/ntfs-manager-helper"
sudo install -m 0755 "$ROOT/target/release/ntfs-manager-agent" "$PREFIX/ntfs-manager-agent"
sudo install -m 0755 "$ROOT/target/release/ntfs-manager" "$CLI"

sudo rm -rf "$APP"
sudo cp -R "$ROOT/dist/NTFS Manager.app" "$APP"
sudo chown -R root:wheel "$APP"

sudo install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.helper.plist"     /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
sudo chown root:wheel /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist

sed "s|__HOME__|$HOME_NOW|g"     "$ROOT/packaging/dev.step2.ntfs-manager.agent.plist"     > "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"

sed "s|__HOME__|$HOME_NOW|g"     "$ROOT/packaging/dev.step2.ntfs-manager.menubar.plist"     > "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"

chmod 0644     "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"     "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"

/usr/bin/plutil -lint /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist >/dev/null
/usr/bin/plutil -lint "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist" >/dev/null
/usr/bin/plutil -lint "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" >/dev/null

sudo launchctl bootstrap system /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
launchctl bootstrap "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"
launchctl bootstrap "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"

sleep 1

"$CLI" doctor

echo
echo "NTFS Manager installed."
echo "Menu bar: enabled"
echo "Auto-mount: enabled by default"
