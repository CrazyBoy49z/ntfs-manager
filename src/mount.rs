use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{bail, Context, Result};

use crate::{
    disk::{validate_device_identifier, DiskService},
    ntfs3g,
};

#[derive(Debug)]
pub struct MountedVolume {
    pub device: String,
    pub mount_point: String,
}

#[derive(Clone, Debug)]
pub struct MountManager {
    disks: DiskService,
}

impl MountManager {
    pub fn new(disks: DiskService) -> Self {
        Self { disks }
    }

    pub fn mount(
        &self,
        device: &str,
        requested_mount_point: Option<&Path>,
    ) -> Result<MountedVolume> {
        validate_device_identifier(device)?;
        let volume = self.disks.ntfs_volume(device)?;

        if volume.mounted && volume.writable {
            return Ok(MountedVolume {
                device: volume.device,
                mount_point: volume.mount_point.unwrap_or_else(|| "unknown".to_string()),
            });
        }

        if volume.mounted {
            self.unmount(device)?;
        }

        let mount_point = match requested_mount_point {
            Some(path) => validate_mount_point(path)?,
            None => choose_mount_point(volume.name.as_deref().unwrap_or("NTFS"))?,
        };

        ensure_mount_point(&mount_point)?;

        let binary = ntfs3g::find_binary()?;
        let uid = id_value("-u")?;
        let gid = id_value("-g")?;
        let volume_name = sanitize_volume_name(volume.name.as_deref().unwrap_or("NTFS"));
        let dev_path = format!("/dev/{device}");

        let output = Command::new("/usr/bin/sudo")
            .arg(&binary)
            .arg(&dev_path)
            .arg(&mount_point)
            .args(["-o", "local"])
            .args(["-o", "allow_other"])
            .args(["-o", "auto_xattr"])
            .args(["-o", "auto_cache"])
            .args(["-o", "noatime"])
            .args(["-o", "windows_names"])
            .args(["-o", &format!("uid={uid}")])
            .args(["-o", &format!("gid={gid}")])
            .args(["-o", &format!("volname={volume_name}")])
            .output()
            .with_context(|| format!("failed to execute ntfs-3g at {}", binary.display()))?;

        if !output.status.success() {
            cleanup_empty_mount_point(&mount_point);
            return Err(mount_error(device, &output));
        }

        let refreshed = self.disks.ntfs_volume(device)?;
        if !refreshed.mounted {
            bail!("ntfs-3g exited successfully but /dev/{device} is not mounted");
        }

        if !refreshed.writable {
            bail!("/dev/{device} mounted but macOS still reports it as read-only");
        }

        Ok(MountedVolume {
            device: device.to_string(),
            mount_point: refreshed
                .mount_point
                .unwrap_or_else(|| mount_point.display().to_string()),
        })
    }

    pub fn unmount(&self, device: &str) -> Result<()> {
        validate_device_identifier(device)?;
        let dev_path = format!("/dev/{device}");

        let output = Command::new("/usr/sbin/diskutil")
            .args(["unmount", &dev_path])
            .output()
            .with_context(|| format!("failed to execute diskutil unmount {dev_path}"))?;

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.to_ascii_lowercase().contains("not mounted") {
            return Ok(());
        }

        bail!("failed to unmount {dev_path}: {}", stderr.trim())
    }
}

fn mount_error(device: &str, output: &Output) -> anyhow::Error {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    let lowered = detail.to_ascii_lowercase();

    if lowered.contains("hibernat")
        || lowered.contains("fast restart")
        || lowered.contains("fast startup")
    {
        anyhow::anyhow!(
            "cannot mount /dev/{device} read/write because Windows left the NTFS volume hibernated. Disable Windows Fast Startup and fully shut down Windows. ntfs-manager intentionally does not delete hiberfil.sys automatically.\n{detail}"
        )
    } else if lowered.contains("dirty") || lowered.contains("unclean") {
        anyhow::anyhow!(
            "cannot safely mount /dev/{device} read/write because NTFS is marked dirty/unclean. Repair it from Windows with chkdsk and safely eject it. ntfs-manager intentionally does not use force automatically.\n{detail}"
        )
    } else {
        anyhow::anyhow!("ntfs-3g failed for /dev/{device}: {detail}")
    }
}

fn ensure_mount_point(path: &Path) -> Result<()> {
    if path.exists() {
        let mut entries = fs::read_dir(path)
            .with_context(|| format!("failed to inspect mount point {}", path.display()))?;
        if entries.next().is_some() {
            bail!("mount point is not empty: {}", path.display());
        }
        return Ok(());
    }

    let output = Command::new("/usr/bin/sudo")
        .args(["/bin/mkdir", "-p"])
        .arg(path)
        .output()
        .with_context(|| format!("failed to create mount point {}", path.display()))?;

    if !output.status.success() {
        bail!(
            "failed to create mount point {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn cleanup_empty_mount_point(path: &Path) {
    let Ok(mut entries) = fs::read_dir(path) else {
        return;
    };

    if entries.next().is_none() {
        let _ = Command::new("/usr/bin/sudo")
            .arg("/bin/rmdir")
            .arg(path)
            .output();
    }
}

fn validate_mount_point(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("mount point must be an absolute path inside /Volumes");
    }

    let volumes = Path::new("/Volumes");
    if path.parent() != Some(volumes) {
        bail!("mount point must be a direct child of /Volumes");
    }

    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        bail!("invalid mount point");
    };

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        bail!("invalid mount point name");
    }

    Ok(path.to_path_buf())
}

fn choose_mount_point(volume_name: &str) -> Result<PathBuf> {
    let base_name = sanitize_mount_name(volume_name);
    let base = Path::new("/Volumes").join(&base_name);

    if available_mount_point(&base) {
        return Ok(base);
    }

    let fallback = Path::new("/Volumes").join(format!("{base_name}-NTFS"));
    if available_mount_point(&fallback) {
        return Ok(fallback);
    }

    for suffix in 2..=100 {
        let candidate = Path::new("/Volumes").join(format!("{base_name}-NTFS-{suffix}"));
        if available_mount_point(&candidate) {
            return Ok(candidate);
        }
    }

    bail!("could not find a free mount point under /Volumes")
}

fn available_mount_point(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }

    fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

fn sanitize_mount_name(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|ch| match ch {
            '/' | ':' | '\0' => '_',
            ch if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>();
    let cleaned = cleaned.trim().trim_matches('.').trim();

    if cleaned.is_empty() {
        "NTFS".to_string()
    } else {
        cleaned.chars().take(64).collect()
    }
}

fn sanitize_volume_name(name: &str) -> String {
    sanitize_mount_name(name).replace(',', "_")
}

fn id_value(flag: &str) -> Result<String> {
    let output = Command::new("/usr/bin/id")
        .arg(flag)
        .output()
        .context("failed to execute id")?;

    if !output.status.success() {
        bail!("id {flag} failed")
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_name_is_sanitized() {
        assert_eq!(sanitize_mount_name("My/Disk"), "My_Disk");
        assert_eq!(sanitize_mount_name(".."), "NTFS");
        assert_eq!(sanitize_volume_name("Work,Data"), "Work_Data");
    }

    #[test]
    fn mount_point_must_be_directly_under_volumes() {
        assert!(validate_mount_point(Path::new("/Volumes/Data")).is_ok());
        assert!(validate_mount_point(Path::new("/tmp/Data")).is_err());
        assert!(validate_mount_point(Path::new("/Volumes/a/b")).is_err());
        assert!(validate_mount_point(Path::new("relative")).is_err());
    }
}
