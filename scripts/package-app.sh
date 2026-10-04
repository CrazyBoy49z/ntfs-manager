#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:-"$ROOT/dist/NTFS Manager.app"}"
BINARY_DIR="${NTFS_MANAGER_BINARY_DIR:-"$ROOT/target/release"}"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "package-app.sh requires macOS" >&2
    exit 1
fi

for binary in ntfs-manager-menubar ntfs-manager-helper ntfs-manager-agent ntfs-manager; do
    if [[ ! -x "$BINARY_DIR/$binary" ]]; then
        echo "Missing $BINARY_DIR/$binary" >&2
        exit 1
    fi
done

rm -rf "$APP"
mkdir -p     "$APP/Contents/MacOS"     "$APP/Contents/Resources/bin"     "$APP/Contents/Resources/launchd"

install -m 0755     "$BINARY_DIR/ntfs-manager-menubar"     "$APP/Contents/MacOS/ntfs-manager-menubar"

for binary in ntfs-manager-helper ntfs-manager-agent ntfs-manager; do
    install -m 0755         "$BINARY_DIR/$binary"         "$APP/Contents/Resources/bin/$binary"
done

install -m 0755     "$ROOT/scripts/bootstrap.command"     "$APP/Contents/Resources/bootstrap.command"

install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.helper.plist"     "$APP/Contents/Resources/launchd/dev.step2.ntfs-manager.helper.plist"

install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.agent.plist"     "$APP/Contents/Resources/launchd/dev.step2.ntfs-manager.agent.plist"

install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.menubar.plist"     "$APP/Contents/Resources/launchd/dev.step2.ntfs-manager.menubar.plist"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>NTFS Manager</string>
    <key>CFBundleExecutable</key>
    <string>ntfs-manager-menubar</string>
    <key>CFBundleIdentifier</key>
    <string>dev.step2.ntfs-manager</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>NTFS Manager</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.3.1</string>
    <key>CFBundleVersion</key>
    <string>4</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

/usr/bin/plutil -lint "$APP/Contents/Info.plist" >/dev/null

if command -v /usr/bin/codesign >/dev/null 2>&1; then
    /usr/bin/codesign         --force         --deep         --sign "${CODESIGN_IDENTITY:--}"         "$APP"
fi

echo "Created $APP"
