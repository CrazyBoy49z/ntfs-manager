use anyhow::{bail, Result};
use std::path::Path;

pub fn require_macos() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        bail!("ntfs-manager currently supports macOS only")
    }
}

pub fn macfuse_installed() -> bool {
    [
        "/Library/Filesystems/macfuse.fs",
        "/Library/PreferencePanes/macFUSE.prefPane",
    ]
    .iter()
    .any(|path| Path::new(path).exists())
}
