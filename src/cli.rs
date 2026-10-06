use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ntfs-manager")]
#[command(version, about = "Safe NTFS read/write manager for macOS")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// List detected NTFS volumes.
    List {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show NTFS volume status.
    Status {
        /// Device identifier such as disk4s1. Omit to show all NTFS volumes.
        device: Option<String>,

        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Remount an NTFS volume read/write using ntfs-3g.
    Mount {
        /// Device identifier such as disk4s1.
        device: String,

        /// Optional mount point. Must be inside /Volumes.
        #[arg(long)]
        mount_point: Option<PathBuf>,
    },

    /// Unmount a volume.
    Unmount {
        /// Device identifier such as disk4s1.
        device: String,
    },

    /// Watch for newly attached NTFS volumes.
    Watch {
        /// Automatically remount new NTFS volumes read/write.
        #[arg(long)]
        auto_mount: bool,

        /// Poll interval in seconds.
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=60))]
        interval: u64,
    },

    /// Check macFUSE and ntfs-3g dependencies.
    Doctor,
}
