#!/bin/bash
set -euo pipefail

UID_NOW="$(id -u)"
HOME_NOW="$HOME"
AGENT="$HOME_NOW/Library/LaunchAgents/dev.step2.ntfs-manager.agent.plist"
MENUBAR="$HOME_NOW/Library/LaunchAgents/dev.step2.ntfs-manager.menubar.plist"
DAEMON="/Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist"

launchctl bootout "gui/$UID_NOW" "$AGENT" 2>/dev/null || true
launchctl bootout "gui/$UID_NOW" "$MENUBAR" 2>/dev/null || true
sudo launchctl bootout system "$DAEMON" 2>/dev/null || true

rm -f "$AGENT" "$MENUBAR"
sudo rm -f "$DAEMON"
sudo rm -f /usr/local/bin/ntfs-manager
sudo rm -rf /usr/local/libexec/ntfs-manager
sudo rm -rf "/Applications/NTFS Manager.app"
sudo rm -f /var/run/dev.step2.ntfs-manager.sock

echo "NTFS Manager binaries and launchd services removed."
echo "User settings were kept in:"
echo "  $HOME_NOW/Library/Application Support/NTFS Manager"
