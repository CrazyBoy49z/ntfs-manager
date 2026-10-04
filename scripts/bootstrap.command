#!/bin/bash
set -euo pipefail

RESOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_ROOT="$(cd "$RESOURCE_DIR/../.." && pwd)"
EXPECTED_APP="/Applications/NTFS Manager.app"
BIN_DIR="$RESOURCE_DIR/bin"
LAUNCHD_DIR="$RESOURCE_DIR/launchd"
PREFIX="/usr/local/libexec/ntfs-manager"
CLI="/usr/local/bin/ntfs-manager"
UID_NOW="$(id -u)"
HOME_NOW="$HOME"
LAUNCH_AGENTS="$HOME_NOW/Library/LaunchAgents"
LOG_DIR="$HOME_NOW/Library/Logs/NTFS Manager"
STATE_DIR="$HOME_NOW/Library/Application Support/NTFS Manager"

echo
echo "NTFS Manager Setup"
echo "=================="
echo

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "This installer requires macOS." >&2
    exit 1
fi

if [[ "$APP_ROOT" != "$EXPECTED_APP" ]]; then
    echo "Move NTFS Manager.app to /Applications first, then open it again." >&2
    /usr/bin/open /Applications
    exit 1
fi

if ! groups "$(id -un)" | tr ' ' '\n' | grep -qx admin; then
    echo "The current macOS account must be an administrator." >&2
    exit 1
fi

mkdir -p "$LAUNCH_AGENTS" "$LOG_DIR" "$STATE_DIR"

SETUP_LOG="$LOG_DIR/setup.log"
exec > >(tee -a "$SETUP_LOG") 2>&1

MACFUSE_INSTALLED_NOW=0

find_brew() {
    for candidate in /opt/homebrew/bin/brew /usr/local/bin/brew; do
        if [[ -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done

    return 1
}

BREW="$(find_brew || true)"

if [[ -z "$BREW" ]]; then
    echo "Homebrew is not installed."
    echo "Installing Homebrew from the official Homebrew installer..."
    echo

    NONINTERACTIVE=1 /bin/bash -c "$(
        /usr/bin/curl             --fail             --silent             --show-error             --location             --proto '=https'             --tlsv1.2             https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh
    )"

    BREW="$(find_brew || true)"
    if [[ -z "$BREW" ]]; then
        echo "Homebrew installation did not complete successfully." >&2
        exit 1
    fi
fi

echo
echo "Homebrew: $BREW"
echo

# The installer must not unexpectedly update the user's Homebrew installation.
export HOMEBREW_NO_AUTO_UPDATE=1
export HOMEBREW_NO_ENV_HINTS=1
export HOMEBREW_NO_INSTALL_CLEANUP=1

find_ntfs3g() {
    for candidate in         /usr/local/bin/ntfs-3g         /usr/local/sbin/ntfs-3g         /usr/local/opt/ntfs-3g-mac/bin/ntfs-3g         /usr/local/opt/ntfs-3g-mac/sbin/ntfs-3g         /opt/homebrew/bin/ntfs-3g         /opt/homebrew/sbin/ntfs-3g         /opt/homebrew/opt/ntfs-3g-mac/bin/ntfs-3g         /opt/homebrew/opt/ntfs-3g-mac/sbin/ntfs-3g
    do
        if [[ -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done

    local prefix
    prefix="$("$BREW" --prefix ntfs-3g-mac 2>/dev/null || true)"
    if [[ -n "$prefix" ]]; then
        for candidate in "$prefix/bin/ntfs-3g" "$prefix/sbin/ntfs-3g"; do
            if [[ -x "$candidate" ]]; then
                printf '%s\n' "$candidate"
                return 0
            fi
        done
    fi

    return 1
}

if [[ ! -d /Library/Filesystems/macfuse.fs ]]; then
    echo "Installing macFUSE..."
    "$BREW" install --cask macfuse
    MACFUSE_INSTALLED_NOW=1
else
    echo "macFUSE is already installed — skipping."
fi

NTFS3G="$(find_ntfs3g || true)"
if [[ -n "$NTFS3G" ]]; then
    echo "ntfs-3g-mac is already installed: $NTFS3G"
else
    echo "Installing ntfs-3g-mac..."
    if ! "$BREW" tap | grep -qx 'gromgit/fuse'; then
        "$BREW" tap gromgit/fuse
    fi
    "$BREW" install gromgit/fuse/ntfs-3g-mac
fi

echo
echo "Installing NTFS Manager system components..."
echo

sudo -v

launchctl bootout "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist" 2>/dev/null || true
sudo launchctl bootout system /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist 2>/dev/null || true

sudo install -d -m 0755 "$PREFIX"
sudo install -m 0755 "$BIN_DIR/ntfs-manager-helper" "$PREFIX/ntfs-manager-helper"
sudo install -m 0755 "$BIN_DIR/ntfs-manager-agent" "$PREFIX/ntfs-manager-agent"
sudo install -m 0755 "$BIN_DIR/ntfs-manager" "$CLI"

sudo install -m 0644     "$LAUNCHD_DIR/dev.step2.ntfs-manager.helper.plist"     /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
sudo chown root:wheel /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist

sed "s|__HOME__|$HOME_NOW|g"     "$LAUNCHD_DIR/dev.step2.ntfs-manager.agent.plist"     > "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"

sed "s|__HOME__|$HOME_NOW|g"     "$LAUNCHD_DIR/dev.step2.ntfs-manager.menubar.plist"     > "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"

chmod 0644     "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"     "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist"

/usr/bin/plutil -lint /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist >/dev/null
/usr/bin/plutil -lint "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist" >/dev/null
/usr/bin/plutil -lint "$LAUNCH_AGENTS/dev.step2.ntfs-manager.menubar.plist" >/dev/null

sudo launchctl bootstrap system /Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
launchctl bootstrap "gui/$UID_NOW" "$LAUNCH_AGENTS/dev.step2.ntfs-manager.agent.plist"

touch "$STATE_DIR/installed-v0.4.0"

echo
echo "NTFS Manager installation completed."
echo "Setup log: $SETUP_LOG"
echo

if [[ "$MACFUSE_INSTALLED_NOW" -eq 1 ]]; then
    echo "IMPORTANT:"
    echo "macOS may ask you to allow macFUSE in System Settings → Privacy & Security."
    echo "If it does, approve it and restart the Mac."
    echo
    /usr/bin/open "x-apple.systempreferences:com.apple.preference.security?General" 2>/dev/null || true
fi

echo "The menu-bar app will detect the repaired components automatically."
