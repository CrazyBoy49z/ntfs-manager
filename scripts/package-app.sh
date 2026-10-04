#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:-"$ROOT/dist/NTFS Manager.app"}"
BINARY_DIR="${NTFS_MANAGER_BINARY_DIR:-"$ROOT/target/release"}"
ICON_WORK="$ROOT/dist/icon-build"
ICON_SOURCE="$ROOT/assets/NTFSManager.png"
ICONSET="$ICON_WORK/NTFSManager.iconset"

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

rm -rf "$APP" "$ICON_WORK"
mkdir -p     "$APP/Contents/MacOS"     "$APP/Contents/Resources/bin"     "$APP/Contents/Resources/launchd"     "$ICONSET"

install -m 0755     "$BINARY_DIR/ntfs-manager-menubar"     "$APP/Contents/MacOS/ntfs-manager-menubar"

for binary in ntfs-manager-helper ntfs-manager-agent ntfs-manager; do
    install -m 0755         "$BINARY_DIR/$binary"         "$APP/Contents/Resources/bin/$binary"
done

install -m 0755     "$ROOT/scripts/bootstrap.command"     "$APP/Contents/Resources/bootstrap.command"

install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.helper.plist"     "$APP/Contents/Resources/launchd/dev.step2.ntfs-manager.helper.plist"

install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.agent.plist"     "$APP/Contents/Resources/launchd/dev.step2.ntfs-manager.agent.plist"

install -m 0644     "$ROOT/packaging/dev.step2.ntfs-manager.menubar.plist"     "$APP/Contents/Resources/launchd/dev.step2.ntfs-manager.menubar.plist"

# Build the macOS icon set from the approved project artwork.
test -s "$ICON_SOURCE"

make_icon() {
    local size="$1"
    local output="$2"
    /usr/bin/sips -z "$size" "$size" "$ICON_SOURCE" --out "$ICONSET/$output" >/dev/null
}

make_icon 16 icon_16x16.png
make_icon 32 icon_16x16@2x.png
make_icon 32 icon_32x32.png
make_icon 64 icon_32x32@2x.png
make_icon 128 icon_128x128.png
make_icon 256 icon_128x128@2x.png
make_icon 256 icon_256x256.png
make_icon 512 icon_256x256@2x.png
make_icon 512 icon_512x512.png
make_icon 1024 icon_512x512@2x.png

/usr/bin/iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/NTFSManager.icns"
install -m 0644 "$ICON_SOURCE" "$APP/Contents/Resources/NTFSManager.png"

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
    <key>CFBundleIconFile</key>
    <string>NTFSManager</string>
    <key>CFBundleIdentifier</key>
    <string>dev.step2.ntfs-manager</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>NTFS Manager</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.3.8</string>
    <key>CFBundleVersion</key>
    <string>11</string>
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

rm -rf "$ICON_WORK"
echo "Created $APP"
