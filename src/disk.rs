use std::process::Command;

use anyhow::{bail, Context, Result};
use plist::Value;
use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct DiskService;

#[derive(Clone, Debug, Serialize)]
pub struct DiskVolume {
    pub device: String,
    pub name: Option<String>,
    pub filesystem: String,
    pub is_ntfs: bool,
    pub mounted: bool,
    pub writable: bool,
    pub mount_point: Option<String>,
    pub size_bytes: Option<u64>,
    pub internal: Option<bool>,
    pub removable: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NtfsVolume {
    pub device: String,
    pub name: Option<String>,
    pub mounted: bool,
    pub writable: bool,
    pub mount_point: Option<String>,
    pub size_bytes: Option<u64>,
    pub internal: Option<bool>,
    pub removable: Option<bool>,
}

impl DiskService {
    pub fn new() -> Self {
        Self
    }

    pub fn external_volumes(&self) -> Result<Vec<DiskVolume>> {
        let output = Command::new("/usr/sbin/diskutil")
            .args(["list", "-plist"])
            .output()
            .context("failed to execute diskutil list")?;

        if !output.status.success() {
            bail!(
                "diskutil list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let plist = Value::from_reader_xml(output.stdout.as_slice())
            .context("failed to parse diskutil list plist")?;

        let devices = plist
            .as_dictionary()
            .and_then(|dict| dict.get("AllDisks"))
            .and_then(Value::as_array)
            .context("diskutil list did not return AllDisks")?;

        let mut volumes = Vec::new();
        for value in devices {
            let Some(device) = value.as_string() else {
                continue;
            };

            if !is_partition_identifier(device) {
                continue;
            }

            let Ok(volume) = self.volume_info(device) else {
                continue;
            };

            if is_user_visible_external_volume(&volume) {
                volumes.push(volume.into_disk_volume());
            }
        }

        volumes.sort_by(|a, b| a.device.cmp(&b.device));
        Ok(volumes)
    }

    pub fn ntfs_volumes(&self) -> Result<Vec<NtfsVolume>> {
        let output = Command::new("/usr/sbin/diskutil")
            .args(["list", "-plist"])
            .output()
            .context("failed to execute diskutil list")?;

        if !output.status.success() {
            bail!(
                "diskutil list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let plist = Value::from_reader_xml(output.stdout.as_slice())
            .context("failed to parse diskutil list plist")?;

        let devices = plist
            .as_dictionary()
            .and_then(|dict| dict.get("AllDisks"))
            .and_then(Value::as_array)
            .context("diskutil list did not return AllDisks")?;

        let mut volumes = Vec::new();
        for value in devices {
            let Some(device) = value.as_string() else {
                continue;
            };

            if !is_partition_identifier(device) {
                continue;
            }

            match self.volume_info(device) {
                Ok(volume) if volume.is_ntfs => volumes.push(volume.into_public()),
                _ => {}
            }
        }

        volumes.sort_by(|a, b| a.device.cmp(&b.device));
        Ok(volumes)
    }

    pub fn ntfs_volume(&self, device: &str) -> Result<NtfsVolume> {
        validate_device_identifier(device)?;
        let info = self.volume_info(device)?;

        if !info.is_ntfs {
            bail!("/dev/{device} is not an NTFS volume");
        }

        Ok(info.into_public())
    }

    fn volume_info(&self, device: &str) -> Result<VolumeInfo> {
        validate_device_identifier(device)?;

        let dev_path = format!("/dev/{device}");
        let output = Command::new("/usr/sbin/diskutil")
            .args(["info", "-plist", &dev_path])
            .output()
            .with_context(|| format!("failed to execute diskutil info for {dev_path}"))?;

        if !output.status.success() {
            bail!(
                "diskutil info failed for {dev_path}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let plist = Value::from_reader_xml(output.stdout.as_slice())
            .with_context(|| format!("failed to parse diskutil info for {dev_path}"))?;
        let dict = plist
            .as_dictionary()
            .context("diskutil info did not return a dictionary")?;

        let filesystem_type = string_value(dict, "FilesystemType");
        let filesystem_name = string_value(dict, "FilesystemName");

        let is_ntfs = is_ntfs_filesystem(filesystem_type.as_deref(), filesystem_name.as_deref());

        Ok(VolumeInfo {
            device: string_value(dict, "DeviceIdentifier").unwrap_or_else(|| device.to_string()),
            name: string_value(dict, "VolumeName").filter(|value| !value.is_empty()),
            mounted: bool_value(dict, "Mounted").unwrap_or(false),
            writable: bool_value(dict, "Writable").unwrap_or(false),
            mount_point: string_value(dict, "MountPoint").filter(|value| !value.is_empty()),
            size_bytes: integer_value(dict, "TotalSize"),
            internal: bool_value(dict, "Internal"),
            removable: bool_value(dict, "RemovableMedia"),
            virtual_or_physical: string_value(dict, "VirtualOrPhysical"),
            disk_image: bool_value(dict, "DiskImage"),
            filesystem_type,
            filesystem_name,
            is_ntfs,
        })
    }
}

#[derive(Debug)]
struct VolumeInfo {
    device: String,
    name: Option<String>,
    mounted: bool,
    writable: bool,
    mount_point: Option<String>,
    size_bytes: Option<u64>,
    internal: Option<bool>,
    removable: Option<bool>,
    virtual_or_physical: Option<String>,
    disk_image: Option<bool>,
    filesystem_type: Option<String>,
    filesystem_name: Option<String>,
    is_ntfs: bool,
}

impl VolumeInfo {
    fn into_disk_volume(self) -> DiskVolume {
        let filesystem = self
            .filesystem_name
            .clone()
            .or(self.filesystem_type.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        DiskVolume {
            device: self.device,
            name: self.name,
            filesystem,
            is_ntfs: self.is_ntfs,
            mounted: self.mounted,
            writable: self.writable,
            mount_point: self.mount_point,
            size_bytes: self.size_bytes,
            internal: self.internal,
            removable: self.removable,
        }
    }

    fn into_public(self) -> NtfsVolume {
        NtfsVolume {
            device: self.device,
            name: self.name,
            mounted: self.mounted,
            writable: self.writable,
            mount_point: self.mount_point,
            size_bytes: self.size_bytes,
            internal: self.internal,
            removable: self.removable,
        }
    }
}

fn is_user_visible_external_volume(volume: &VolumeInfo) -> bool {
    let is_external = volume.internal == Some(false) || volume.removable == Some(true);
    let has_filesystem =
        volume.filesystem_type.is_some() || volume.filesystem_name.is_some();

    if !is_external || !has_filesystem {
        return false;
    }

    if volume.disk_image == Some(true) {
        return false;
    }

    if volume
        .virtual_or_physical
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("virtual"))
    {
        return false;
    }

    if volume.mount_point.as_deref().is_some_and(|mount_point| {
        mount_point == "/System"
            || mount_point.starts_with("/System/")
            || mount_point == "/private/var"
            || mount_point.starts_with("/private/var/")
    }) {
        return false;
    }

    if volume.name.as_deref().is_some_and(|name| {
        let normalized = name.to_ascii_lowercase();
        normalized.starts_with("creedence")
            || normalized.contains("securepkitruststore")
            || normalized.contains("cryptex")
    }) {
        return false;
    }

    true
}

fn string_value(dict: &plist::Dictionary, key: &str) -> Option<String> {
    dict.get(key)
        .and_then(Value::as_string)
        .map(ToOwned::to_owned)
}

fn bool_value(dict: &plist::Dictionary, key: &str) -> Option<bool> {
    dict.get(key).and_then(Value::as_boolean)
}

fn integer_value(dict: &plist::Dictionary, key: &str) -> Option<u64> {
    dict.get(key)
        .and_then(Value::as_unsigned_integer)
        .or_else(|| {
            dict.get(key)
                .and_then(Value::as_signed_integer)
                .and_then(|value| u64::try_from(value).ok())
        })
}

fn is_ntfs_filesystem(filesystem_type: Option<&str>, filesystem_name: Option<&str>) -> bool {
    if filesystem_type.is_some_and(|value| value.eq_ignore_ascii_case("ntfs")) {
        return true;
    }

    filesystem_name.is_some_and(|value| {
        let normalized = value.trim().to_ascii_lowercase();
        normalized == "ntfs"
            || normalized == "windows nt file system (ntfs)"
            || normalized == "windows ntfs"
    })
}

pub fn validate_device_identifier(device: &str) -> Result<()> {
    if is_partition_identifier(device) {
        Ok(())
    } else {
        bail!("invalid device identifier '{device}'; expected format like disk4s1")
    }
}

fn is_partition_identifier(device: &str) -> bool {
    let Some(rest) = device.strip_prefix("disk") else {
        return false;
    };

    let Some((disk, slice)) = rest.split_once('s') else {
        return false;
    };

    !disk.is_empty()
        && !slice.is_empty()
        && disk.bytes().all(|byte| byte.is_ascii_digit())
        && slice.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_volume(
        name: Option<&str>,
        mount_point: Option<&str>,
        internal: Option<bool>,
        removable: Option<bool>,
        virtual_or_physical: Option<&str>,
        disk_image: Option<bool>,
    ) -> VolumeInfo {
        VolumeInfo {
            device: "disk3s1".to_string(),
            name: name.map(ToOwned::to_owned),
            mounted: mount_point.is_some(),
            writable: true,
            mount_point: mount_point.map(ToOwned::to_owned),
            size_bytes: Some(1_000_000_000),
            internal,
            removable,
            virtual_or_physical: virtual_or_physical.map(ToOwned::to_owned),
            disk_image,
            filesystem_type: Some("apfs".to_string()),
            filesystem_name: Some("APFS".to_string()),
            is_ntfs: false,
        }
    }

    #[test]
    fn hides_system_cryptex_and_virtual_disk_images() {
        let creedence = test_volume(
            Some("Creedence11M6270.SECUREPKITRUSTSTOREASSET"),
            Some("/System/Volumes/Preboot/Cryptexes/Incoming"),
            Some(false),
            Some(false),
            Some("Virtual"),
            Some(true),
        );
        assert!(!is_user_visible_external_volume(&creedence));

        let generic_disk_image = test_volume(
            Some("Mounted DMG"),
            Some("/Volumes/Mounted DMG"),
            Some(false),
            Some(false),
            Some("Virtual"),
            Some(true),
        );
        assert!(!is_user_visible_external_volume(&generic_disk_image));
    }

    #[test]
    fn keeps_real_external_storage() {
        let usb = test_volume(
            Some("My Passport"),
            Some("/Volumes/My Passport"),
            Some(false),
            Some(true),
            Some("Physical"),
            Some(false),
        );
        assert!(is_user_visible_external_volume(&usb));
    }

    #[test]
    fn detects_real_ntfs_filesystems() {
        assert!(is_ntfs_filesystem(Some("ntfs"), None));
        assert!(is_ntfs_filesystem(
            Some("ntfs"),
            Some("Windows NT File System (NTFS)")
        ));
        assert!(is_ntfs_filesystem(
            None,
            Some("Windows NT File System (NTFS)")
        ));
    }

    #[test]
    fn rejects_fat_and_exfat_even_for_windows_partition_types() {
        assert!(!is_ntfs_filesystem(Some("msdos"), Some("MS-DOS FAT32")));
        assert!(!is_ntfs_filesystem(Some("exfat"), Some("ExFAT")));
        assert!(!is_ntfs_filesystem(Some("fat32"), Some("FAT32")));
        assert!(!is_ntfs_filesystem(None, Some("MS-DOS FAT32")));
    }

    #[test]
    fn accepts_partition_identifiers() {
        assert!(validate_device_identifier("disk4s1").is_ok());
        assert!(validate_device_identifier("disk12s3").is_ok());
    }

    #[test]
    fn rejects_unsafe_or_whole_disk_identifiers() {
        for device in [
            "disk4",
            "/dev/disk4s1",
            "disk4s1;rm",
            "../disk4s1",
            "diskXsY",
            "",
        ] {
            assert!(validate_device_identifier(device).is_err(), "{device}");
        }
    }
}
