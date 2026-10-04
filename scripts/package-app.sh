#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:-"$ROOT/dist/NTFS Manager.app"}"
BINARY="$ROOT/target/release/ntfs-manager-menubar"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "package-app.sh requires macOS" >&2
    exit 1
fi

if [[ ! -x "$BINARY" ]]; then
    echo "Missing $BINARY. Run: cargo build --release --all-features" >&2
    exit 1
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m 0755 "$BINARY" "$APP/Contents/MacOS/ntfs-manager-menubar"

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
    <string>0.2.0</string>
    <key>CFBundleVersion</key>
    <string>2</string>
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
echo "Created $APP"
