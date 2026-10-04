#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="/Applications/NTFS Manager.app"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "NTFS Manager installer requires macOS" >&2
    exit 1
fi

cd "$ROOT"
cargo build --release --all-features
bash "$ROOT/scripts/package-app.sh"

sudo rm -rf "$APP"
sudo cp -R "$ROOT/dist/NTFS Manager.app" "$APP"
sudo chown -R root:wheel "$APP"

echo "Installed $APP"
echo "Opening NTFS Manager. First-run setup will install runtime dependencies."

open "$APP"
