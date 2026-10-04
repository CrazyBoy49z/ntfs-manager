# NTFS Manager

Safe NTFS read/write support for macOS, written in Rust and powered by **macFUSE + ntfs-3g**.

NTFS Manager does not replace Apple's `/sbin/mount_ntfs`, does not disable SIP, and does not silently force-mount dirty or hibernated NTFS volumes.

## Components

- `ntfs-manager` — CLI for diagnostics, listing, manual mount/unmount, and foreground watch mode.
- `ntfs-manager-helper` — root-only launchd helper with a narrow Unix-socket protocol.
- `ntfs-manager-agent` — per-user background auto-mount agent.
- `ntfs-manager-menubar` — native macOS menu-bar app.
- `NTFS Manager.app` — LSUIElement app bundle with no Dock icon.

## Menu-bar actions

- Live NTFS volume status.
- Auto-mount On/Off.
- Mount all detected NTFS volumes read/write.
- Unmount all.
- Open the first mounted NTFS volume in Finder.
- Refresh.
- Quit.

## Security model

The privileged helper:

- runs as root only through a LaunchDaemon;
- listens on `/var/run/dev.step2.ntfs-manager.sock`;
- exposes only `ping`, `mount`, and `unmount`;
- validates device identifiers like `disk4s1`;
- allows mount points only as direct children of `/Volumes`;
- rejects symlink mount points;
- reads the caller UID/GID from the Unix socket using `getpeereid` instead of trusting client-supplied identity data;
- exposes its socket only to the macOS `admin` group;
- never enables ntfs-3g `force` or `remove_hiberfile` automatically.

If Windows Fast Startup/hibernation or an unclean NTFS state is detected, mounting stops with an error.

## Requirements

- macOS 11+
- Intel or Apple Silicon Mac
- Rust 1.80+ when building from source
- Homebrew
- macFUSE
- ntfs-3g-mac
- current user in the macOS `admin` group

Install dependencies:

```bash
brew install --cask macfuse
brew tap gromgit/fuse
brew install gromgit/fuse/ntfs-3g-mac
```

Approve macFUSE in **System Settings → Privacy & Security** if macOS requests it, then reboot.

## Install

```bash
git clone https://github.com/CrazyBoy49z/ntfs-manager.git
cd ntfs-manager
bash scripts/install.sh
```

The installer builds all Rust binaries, creates `NTFS Manager.app`, installs the helper under `/usr/local/libexec/ntfs-manager`, installs the CLI as `/usr/local/bin/ntfs-manager`, and bootstraps the LaunchDaemon/LaunchAgents.

## CLI

```bash
ntfs-manager doctor
ntfs-manager list
ntfs-manager list --json
ntfs-manager status
ntfs-manager status disk4s1
ntfs-manager mount disk4s1
ntfs-manager unmount disk4s1
ntfs-manager watch
ntfs-manager watch --auto-mount
```

Manual CLI mounting still uses `sudo`. Background auto-mount uses the installed privileged helper and does not need an interactive sudo prompt.

## Settings

User configuration:

```text
~/Library/Application Support/NTFS Manager/config.json
```

Default:

```json
{
  "auto_mount": true,
  "poll_interval_secs": 2
}
```

The menu-bar app updates `auto_mount` directly. The agent reloads settings while running.

## Logs

```text
/var/log/ntfs-manager-helper.log
~/Library/Logs/NTFS Manager/agent.log
~/Library/Logs/NTFS Manager/menubar.log
```

## Uninstall

```bash
bash scripts/uninstall.sh
```

The uninstaller keeps the user's settings directory intentionally.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --all-features
bash -n scripts/*.sh
```

## Roadmap

- Replace polling with Disk Arbitration callbacks.
- Signed/notarized release builds.
- Homebrew tap.
- Per-volume preferences and allow/deny lists.
- Release installer package.

## License

MIT
