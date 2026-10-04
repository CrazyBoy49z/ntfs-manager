use std::{
    collections::{BTreeMap, BTreeSet},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use ntfs_manager::{disk::DiskService, helper_client::HelperClient, platform, settings::Settings};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .without_time()
        .with_target(false)
        .init();

    platform::require_macos()?;

    let disks = DiskService::new();
    let helper = HelperClient::new();
    let mut retry_after = BTreeMap::<String, (u32, Instant)>::new();

    let mut helper_error = None;
    for _ in 0..5 {
        match helper.ping() {
            Ok(()) => {
                helper_error = None;
                break;
            }
            Err(err) => {
                helper_error = Some(err);
                thread::sleep(Duration::from_millis(250));
            }
        }
    }

    if let Some(err) = helper_error {
        warn!("privileged helper is not ready: {err:#}");
    }

    info!("NTFS Manager agent started");

    loop {
        let settings = match Settings::load() {
            Ok(settings) => settings,
            Err(err) => {
                warn!("invalid settings, using defaults: {err:#}");
                Settings::default()
            }
        };

        let volumes = match disks.ntfs_volumes() {
            Ok(volumes) => volumes,
            Err(err) => {
                error!("failed to scan NTFS volumes: {err:#}");
                thread::sleep(Duration::from_secs(settings.poll_interval_secs));
                continue;
            }
        };

        let present = volumes
            .iter()
            .map(|volume| volume.device.clone())
            .collect::<BTreeSet<_>>();
        retry_after.retain(|device, _| present.contains(device));

        if settings.auto_mount {
            for volume in volumes.iter().filter(|volume| !volume.writable) {
                if retry_after
                    .get(&volume.device)
                    .is_some_and(|(_, next)| *next > Instant::now())
                {
                    continue;
                }

                info!(
                    "auto-mounting /dev/{} ({})",
                    volume.device,
                    volume.name.as_deref().unwrap_or("unnamed")
                );

                match helper.mount(&volume.device, None) {
                    Ok(mount_point) => {
                        info!(
                            "mounted /dev/{} read/write at {}",
                            volume.device, mount_point
                        );
                        retry_after.remove(&volume.device);
                    }
                    Err(err) => {
                        let failures = retry_after
                            .get(&volume.device)
                            .map(|(failures, _)| *failures)
                            .unwrap_or(0)
                            .saturating_add(1);
                        let exponent = failures.saturating_sub(1).min(4);
                        let delay_secs = 15_u64
                            .saturating_mul(1_u64 << exponent)
                            .min(300);

                        error!(
                            "auto-mount failed for /dev/{}: {err:#}; retrying in {}s",
                            volume.device, delay_secs
                        );

                        retry_after.insert(
                            volume.device.clone(),
                            (
                                failures,
                                Instant::now() + Duration::from_secs(delay_secs),
                            ),
                        );
                    }
                }
            }
        }

        thread::sleep(Duration::from_secs(settings.poll_interval_secs));
    }
}
