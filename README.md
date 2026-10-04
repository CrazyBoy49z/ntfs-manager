# NTFS Manager

Safe NTFS read/write support for macOS, written in Rust and powered by **macFUSE + ntfs-3g**.

## Install — normal users

1. Open **GitHub Releases**.
2. Download `NTFS-Manager-vX.Y.Z-macOS-universal.zip`.
3. Unzip it.
4. Drag **NTFS Manager.app** to `/Applications`.
5. Open **NTFS Manager**.

On first launch, NTFS Manager automatically opens its setup in Terminal and installs:

- Homebrew, only when Homebrew is missing;
- macFUSE;
- `gromgit/fuse`;
- `gromgit/fuse/ntfs-3g-mac`;
- the NTFS Manager privileged helper;
- the background auto-mount agent;
- login startup configuration.

No Rust toolchain, `git clone`, or `cargo build` is required for release users.

The release is a **Universal macOS app** containing both Intel `x86_64` and Apple Silicon `arm64` binaries.

### macFUSE approval

macOS can require manual approval of the macFUSE system extension in:

**System Settings → Privacy & Security**

A reboot can also be required. macOS intentionally does not allow an app to bypass this security approval.

### Gatekeeper

Development builds are ad-hoc signed. Until a Developer ID certificate and notarization are configured, macOS can show an **unidentified developer** warning for a downloaded release. In that case use **Control-click → Open** once.

## What the app installs

```text
/Applications/NTFS Manager.app
/usr/local/bin/ntfs-manager
/usr/local/libexec/ntfs-manager/ntfs-manager-helper
/usr/local/libexec/ntfs-manager/ntfs-manager-agent
/Library/LaunchDaemons/dev.step2.ntfs-manager.helper.plist
~/Library/LaunchAgents/dev.step2.ntfs-manager.agent.plist
~/Library/LaunchAgents/dev.step2.ntfs-manager.menubar.plist
```

The app bundle itself contains the helper, agent and CLI binaries under `Contents/Resources/bin`. The first-run setup installs those exact bundled binaries; it does not download executable NTFS Manager components separately.

## Menu-bar actions

- Live NTFS volume status.
- Auto-mount On/Off.
- Mount all detected NTFS volumes read/write.
- Unmount all.
- Open the first mounted NTFS volume in Finder.
- Refresh.
- Install / Repair Components.
- Quit.

## Security model

The privileged helper:

- runs as root only through a LaunchDaemon;
- listens on `/var/run/dev.step2.ntfs-manager.sock`;
- exposes only `ping`, `mount`, and `unmount`;
- validates device identifiers like `disk4s1`;
- restricts unmount operations to NTFS volumes;
- allows mount points only as direct children of `/Volumes`;
- rejects symlink mount points;
- reads the caller UID/GID from the Unix socket using `getpeereid`;
- exposes its socket only to the macOS `admin` group;
- never enables ntfs-3g `force` or `remove_hiberfile` automatically.

NTFS Manager does not replace Apple's `/sbin/mount_ntfs` and does not disable SIP.

If Windows Fast Startup/hibernation or an unclean NTFS state is detected, mounting stops with an error rather than forcing a risky write mount.

## Dependencies installed by first-run setup

Equivalent Homebrew commands are:

```bash
brew install --cask macfuse
brew tap gromgit/fuse
brew install gromgit/fuse/ntfs-3g-mac
```

If Homebrew is missing, the setup uses Homebrew's official installer first.

## CLI

After setup:

```bash
ntfs-manager doctor
ntfs-manager list
ntfs-manager list --json
ntfs-manager status
ntfs-manager status disk4s1
ntfs-manager mount disk4s1
ntfs-manager unmount disk4s1
```

Manual CLI mounting can request `sudo`. Background auto-mount uses the installed privileged helper and does not require a sudo prompt for each disk.

## Settings

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

## Logs

```text
/var/log/ntfs-manager-helper.log
~/Library/Logs/NTFS Manager/agent.log
~/Library/Logs/NTFS Manager/menubar.log
```

## Development

Source builds still require Rust:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --all-features
bash -n scripts/*.sh
```

Build an app from the current host architecture:

```bash
bash scripts/package-app.sh
```

GitHub CI additionally cross-compiles Intel and Apple Silicon targets and verifies the Universal app bundle.

## Uninstall

From a source checkout:

```bash
bash scripts/uninstall.sh
```

User settings are intentionally retained.

## License

MIT
