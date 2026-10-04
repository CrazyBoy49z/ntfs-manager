use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::fd::AsRawFd,
    os::unix::{
        fs::FileTypeExt,
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::Command,
    thread,
};

use anyhow::{bail, Context, Result};
use ntfs_manager::{
    disk::DiskService,
    helper_protocol::{HelperRequest, HelperResponse, HELPER_SOCKET, MAX_MESSAGE_BYTES},
    mount::MountManager,
    platform,
};
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
    require_root()?;
    prepare_socket_path()?;

    let listener = UnixListener::bind(HELPER_SOCKET)
        .with_context(|| format!("failed to bind helper socket at {HELPER_SOCKET}"))?;
    secure_socket()?;

    info!("NTFS Manager privileged helper listening on {HELPER_SOCKET}");

    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                thread::spawn(move || {
                    if let Err(err) = handle_connection(stream) {
                        warn!("helper request failed: {err:#}");
                    }
                });
            }
            Err(err) => error!("helper socket accept failed: {err}"),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: UnixStream) -> Result<()> {
    let (uid, gid) = peer_identity(&stream)?;
    let request = read_request(&stream)?;

    let disks = DiskService::new();
    let manager = MountManager::direct(disks);

    let response = match request {
        HelperRequest::Ping => HelperResponse::ok("pong"),
        HelperRequest::Mount {
            device,
            mount_point,
        } => match manager.mount_for_user(&device, mount_point.as_deref(), uid, gid) {
            Ok(mounted) => HelperResponse::mounted(
                format!("mounted /dev/{} read/write", mounted.device),
                mounted.mount_point,
            ),
            Err(err) => HelperResponse::error(format!("{err:#}")),
        },
        HelperRequest::Unmount { device } => match manager.unmount(&device) {
            Ok(()) => HelperResponse::ok(format!("unmounted /dev/{device}")),
            Err(err) => HelperResponse::error(format!("{err:#}")),
        },
    };

    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    Ok(())
}

fn read_request(stream: &UnixStream) -> Result<HelperRequest> {
    let mut line = String::new();
    let mut reader = BufReader::new(stream).take(MAX_MESSAGE_BYTES);
    reader.read_line(&mut line)?;

    if line.trim().is_empty() {
        bail!("empty helper request");
    }

    serde_json::from_str(&line).context("invalid helper request")
}

fn peer_identity(stream: &UnixStream) -> Result<(u32, u32)> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;

    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("getpeereid failed");
    }

    if uid == 0 {
        bail!("root clients are not accepted by the user-facing helper protocol");
    }

    Ok((uid, gid))
}

fn require_root() -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        bail!("ntfs-manager-helper must run as root via launchd")
    }
}

fn prepare_socket_path() -> Result<()> {
    let path = Path::new(HELPER_SOCKET);
    if !path.exists() {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect stale socket {HELPER_SOCKET}"))?;

    if !metadata.file_type().is_socket() {
        bail!("refusing to remove non-socket path at {HELPER_SOCKET}");
    }

    fs::remove_file(path).with_context(|| format!("failed to remove stale {HELPER_SOCKET}"))
}

fn secure_socket() -> Result<()> {
    fs::set_permissions(HELPER_SOCKET, fs::Permissions::from_mode(0o660))
        .context("failed to set helper socket permissions")?;

    let output = Command::new("/usr/sbin/chown")
        .args(["root:admin", HELPER_SOCKET])
        .output()
        .context("failed to set helper socket owner")?;

    if !output.status.success() {
        bail!(
            "failed to set helper socket owner: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}
