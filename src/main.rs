mod cli;
mod disk;
mod mount;
mod ntfs3g;
mod platform;

use std::{thread, time::Duration};

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use disk::{DiskService, NtfsVolume};
use mount::MountManager;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .without_time()
        .with_target(false)
        .init();

    let cli = Cli::parse();
    platform::require_macos()?;

    let disks = DiskService::new();
    let manager = MountManager::new(disks.clone());

    match cli.command {
        Commands::List { json } => {
            let volumes = disks.ntfs_volumes()?;
            print_volumes(&volumes, json)?;
        }
        Commands::Status { device, json } => {
            if let Some(device) = device {
                let volume = disks.ntfs_volume(&device)?;
                print_volumes(&[volume], json)?;
            } else {
                let volumes = disks.ntfs_volumes()?;
                print_volumes(&volumes, json)?;
            }
        }
        Commands::Mount {
            device,
            mount_point,
        } => {
            let mounted = manager.mount(&device, mount_point.as_deref())?;
            println!(
                "Mounted /dev/{} read/write at {}",
                mounted.device, mounted.mount_point
            );
        }
        Commands::Unmount { device } => {
            manager.unmount(&device)?;
            println!("Unmounted /dev/{device}");
        }
        Commands::Watch {
            auto_mount,
            interval,
        } => watch(disks, manager, auto_mount, interval)?,
        Commands::Doctor => doctor()?,
    }

    Ok(())
}

fn print_volumes(volumes: &[NtfsVolume], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(volumes)?);
        return Ok(());
    }

    if volumes.is_empty() {
        println!("No NTFS volumes found.");
        return Ok(());
    }

    println!(
        "{:<12} {:<24} {:<8} {:<8} MOUNT POINT",
        "DEVICE", "NAME", "MOUNTED", "WRITABLE"
    );
    for volume in volumes {
        println!(
            "{:<12} {:<24} {:<8} {:<8} {}",
            volume.device,
            volume.name.as_deref().unwrap_or("—"),
            yes_no(volume.mounted),
            yes_no(volume.writable),
            volume.mount_point.as_deref().unwrap_or("—")
        );
    }

    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn watch(disks: DiskService, manager: MountManager, auto_mount: bool, interval: u64) -> Result<()> {
    let mut known = disks
        .ntfs_volumes()?
        .into_iter()
        .map(|volume| volume.device)
        .collect::<std::collections::BTreeSet<_>>();

    info!("Watching for NTFS volumes every {interval}s. Press Ctrl+C to stop.");
    if auto_mount {
        info!(
            "Auto-mount is enabled. sudo may request your password when a new NTFS volume appears."
        );
    }

    loop {
        thread::sleep(Duration::from_secs(interval));

        let volumes = match disks.ntfs_volumes() {
            Ok(volumes) => volumes,
            Err(err) => {
                error!("Failed to scan disks: {err:#}");
                continue;
            }
        };

        let current = volumes
            .iter()
            .map(|volume| volume.device.clone())
            .collect::<std::collections::BTreeSet<_>>();

        for volume in volumes
            .iter()
            .filter(|volume| !known.contains(&volume.device))
        {
            info!(
                "NTFS detected: /dev/{} ({})",
                volume.device,
                volume.name.as_deref().unwrap_or("unnamed")
            );

            if auto_mount {
                match manager.mount(&volume.device, None) {
                    Ok(mounted) => info!(
                        "Mounted /dev/{} read/write at {}",
                        mounted.device, mounted.mount_point
                    ),
                    Err(err) => {
                        error!("Auto-mount failed for /dev/{}: {err:#}", volume.device)
                    }
                }
            }
        }

        known = current;
    }
}

fn doctor() -> Result<()> {
    println!("macOS: OK");

    if platform::macfuse_installed() {
        println!("macFUSE: installed");
    } else {
        println!("macFUSE: NOT FOUND");
        println!("  brew install --cask macfuse");
    }

    match ntfs3g::find_binary() {
        Ok(binary) => {
            println!("ntfs-3g: {}", binary.display());
            if let Ok(version) = ntfs3g::version(&binary) {
                println!("  {version}");
            }
        }
        Err(_) => {
            println!("ntfs-3g: NOT FOUND");
            println!("  brew tap gromgit/fuse");
            println!("  brew install gromgit/fuse/ntfs-3g-mac");
        }
    }

    Ok(())
}
