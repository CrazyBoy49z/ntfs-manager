# NTFS Manager

A lightweight macOS menu-bar app for mounting NTFS volumes read/write with **macFUSE** and **ntfs-3g**, written in Rust.

- Native menu-bar workflow with a compact custom popover
- One-click NTFS read/write mount and unmount
- Optional automatic mounting
- Built-in verified updates with confirmation before installation
- Intel and Apple Silicon support
- Safe privileged helper with a narrow Unix-socket API
- No SIP changes and no replacement of Apple's `/sbin/mount_ntfs`

## Download

Download the latest Universal macOS build from **GitHub Releases**:

`NTFS-Manager-vX.Y.Z-macOS-universal.zip`

The app supports both:

- Apple Silicon (`arm64`)
- Intel (`x86_64`)

## Installation

1. Download and unzip the latest release.
2. Move **NTFS Manager.app** to `/Applications`.
3. Open it.
4. Allow the installer to add the required components.
5. If macOS asks for macFUSE approval, open **System Settings → Privacy & Security**, allow it, then restart the Mac.

NTFS Manager installs or reuses:

```bash
brew install --cask macfuse
brew tap gromgit/fuse
brew install gromgit/fuse/ntfs-3g-mac
```

If Homebrew is missing, the first-run setup can install it using the official Homebrew installer.

When macFUSE and ntfs-3g are already installed, NTFS Manager repairs or updates its own helper and agent without opening a Terminal window. Automatic privileged repair is requested at most once per app version; if you cancel it, you can run **Repair** manually from Settings later.

## Gatekeeper

Release builds are currently ad-hoc signed rather than Apple-notarized.

If macOS blocks an app downloaded from this repository, first try **Control-click → Open**.

If it is still blocked, remove quarantine from this app only:

```bash
xattr -dr com.apple.quarantine "/Applications/NTFS Manager.app"
open "/Applications/NTFS Manager.app"
```

This does not disable Gatekeeper globally.

## Usage

Click the NTFS Manager icon in the menu bar.

The popover shows connected external volumes and provides NTFS-specific actions:

- **Mount read/write**
- **Unmount**
- **Open in Finder**
- **Mount all NTFS**
- **Auto-mount**

Non-NTFS physical disks such as FAT32 and exFAT can be shown for context, but NTFS Manager does not try to manage them as NTFS. macOS system disk images, Cryptex volumes, virtual APFS assets, and system-mounted volumes are filtered out of the connected-disk list.

Settings include:

- Auto-mount
- Launch at login
- Component repair
- Automatic update checks
- One-click verified updates
- Open logs
- Move to Applications

## Updates

When **Check for updates** is enabled, NTFS Manager checks GitHub Releases on launch and periodically in the background.

If a newer release exists, the app shows an **Update / Later** confirmation inside the popover. Nothing is downloaded or installed until **Update** is clicked.

The updater then:

1. downloads the Universal macOS ZIP and its published SHA-256 file;
2. verifies the downloaded archive checksum;
3. extracts the app;
4. verifies the bundle signature, bundle identifier, and expected version;
5. replaces `/Applications/NTFS Manager.app`;
6. relaunches the new version automatically.

A normal app-only update does not reinstall the privileged helper just because the app version changed. The existing helper remains valid while it satisfies the minimum compatible helper version, which avoids unnecessary administrator-password prompts. If a future release requires a newer helper protocol, NTFS Manager will request repair only when it is actually needed.

> The first release containing the in-app updater still has to be installed manually. After that, supported releases can update themselves from inside the app.

## How it works

NTFS Manager consists of four Rust binaries:

| Component | Purpose |
| --- | --- |
| `ntfs-manager-menubar` | menu-bar UI and settings |
| `ntfs-manager-helper` | privileged mount/unmount helper |
| `ntfs-manager-agent` | background auto-mount agent |
| `ntfs-manager` | command-line interface |

The privileged helper:

- runs through a LaunchDaemon;
- accepts only `ping`, `mount`, and `unmount`;
- validates disk identifiers such as `disk4s1`;
- limits mount points to direct children of `/Volumes`;
- rejects symlink mount points;
- obtains the caller UID/GID from the Unix socket;
- does not automatically use ntfs-3g `force` or `remove_hiberfile`.

If Windows Fast Startup/hibernation or an unclean NTFS filesystem is detected, the app stops rather than forcing a risky write mount.

## Installed files

```text
/Applications/NTFS Manager.app
/usr/local/bin/ntfs-manager
/usr/local/libexec/ntfs-manager/ntfs-manager-helper
/usr/local/libexec/ntfs-manager/ntfs-manager-agent
/Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
~/Library/LaunchAgents/dev.step2.ntfs-manager.agent.plist
~/Library/LaunchAgents/dev.step2.ntfs-manager.menubar.plist
```

## CLI

After installation:

```bash
ntfs-manager doctor
ntfs-manager list
ntfs-manager list --json
ntfs-manager status
ntfs-manager status disk4s1
ntfs-manager mount disk4s1
ntfs-manager unmount disk4s1
```

## Configuration

Settings are stored in:

```text
~/Library/Application Support/NTFS Manager/config.json
```

Example:

```json
{
  "auto_mount": true,
  "poll_interval_secs": 2,
  "launch_at_login": true,
  "check_updates": true
}
```

## Logs

```text
/var/log/ntfs-manager-helper.log
~/Library/Logs/NTFS Manager/agent.log
~/Library/Logs/NTFS Manager/menubar.log
~/Library/Logs/NTFS Manager/setup.log
```

## Development

Requirements:

- macOS
- Rust 1.80+
- Xcode Command Line Tools

Run the checks:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --all-features
bash -n scripts/*.sh
python3 -m py_compile scripts/*.py
```

Build the app for the current Mac architecture:

```bash
bash scripts/package-app.sh
```

GitHub Actions builds the release as a Universal binary for Intel and Apple Silicon.

## Uninstall

From a source checkout:

```bash
bash scripts/uninstall.sh
```

User settings are preserved.

## License

MIT
