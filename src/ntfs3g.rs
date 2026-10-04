use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};

pub fn find_binary() -> Result<PathBuf> {
    if let Ok(explicit) = env::var("NTFS3G_BIN") {
        let path = PathBuf::from(explicit);
        if is_executable_file(&path) {
            return Ok(path);
        }
        bail!(
            "NTFS3G_BIN points to a missing or non-executable file: {}",
            path.display()
        );
    }

    if let Some(path) = find_in_path("ntfs-3g") {
        return Ok(path);
    }

    for path in [
        "/usr/local/bin/ntfs-3g",
        "/usr/local/sbin/ntfs-3g",
        "/opt/homebrew/bin/ntfs-3g",
        "/opt/homebrew/sbin/ntfs-3g",
    ] {
        let path = PathBuf::from(path);
        if is_executable_file(&path) {
            return Ok(path);
        }
    }

    if let Some(path) = homebrew_formula_binary() {
        return Ok(path);
    }

    bail!(
        "ntfs-3g was not found; install it with: brew tap gromgit/fuse && brew install gromgit/fuse/ntfs-3g-mac"
    )
}

pub fn version(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to execute {} --version", binary.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let text = if stdout.is_empty() { stderr } else { stdout };

    if output.status.success() || !text.is_empty() {
        Ok(text
            .lines()
            .next()
            .unwrap_or("unknown version")
            .to_string())
    } else {
        bail!("unable to read ntfs-3g version")
    }
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| is_executable_file(candidate))
}

fn homebrew_formula_binary() -> Option<PathBuf> {
    for brew in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
        if !Path::new(brew).exists() {
            continue;
        }

        let output = Command::new(brew)
            .args(["--prefix", "ntfs-3g-mac"])
            .output()
            .ok()?;

        if !output.status.success() {
            continue;
        }

        let prefix = String::from_utf8(output.stdout).ok()?;
        let prefix = PathBuf::from(prefix.trim());
        for relative in ["bin/ntfs-3g", "sbin/ntfs-3g"] {
            let candidate = prefix.join(relative);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
