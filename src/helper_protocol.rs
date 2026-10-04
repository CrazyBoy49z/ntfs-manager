use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const HELPER_SOCKET: &str = "/var/run/dev.step2.ntfs-manager.sock";
pub const MAX_MESSAGE_BYTES: u64 = 16 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HelperRequest {
    Ping,
    Version,
    Mount {
        device: String,
        mount_point: Option<PathBuf>,
    },
    Unmount {
        device: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HelperResponse {
    pub ok: bool,
    pub message: String,
    pub mount_point: Option<String>,
}

impl HelperResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            mount_point: None,
        }
    }

    pub fn mounted(message: impl Into<String>, mount_point: String) -> Self {
        Self {
            ok: true,
            message: message.into(),
            mount_point: Some(mount_point),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            mount_point: None,
        }
    }
}
