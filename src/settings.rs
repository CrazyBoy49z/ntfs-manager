use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub auto_mount: bool,
    pub poll_interval_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_mount: true,
            poll_interval_secs: 2,
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read settings from {}", path.display()))?;
        let settings = serde_json::from_slice::<Self>(&bytes)
            .with_context(|| format!("invalid settings in {}", path.display()))?;

        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;

        let path = settings_path()?;
        let parent = path
            .parent()
            .context("settings path does not have a parent directory")?;
        fs::create_dir_all(parent)?;

        let temp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)?;

        {
            let mut file = fs::File::create(&temp)
                .with_context(|| format!("failed to create {}", temp.display()))?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }

        fs::rename(&temp, &path).with_context(|| {
            format!(
                "failed to replace settings {} with {}",
                path.display(),
                temp.display()
            )
        })?;

        Ok(())
    }

    pub fn set_auto_mount(enabled: bool) -> Result<Self> {
        let mut settings = Self::load().unwrap_or_default();
        settings.auto_mount = enabled;
        settings.save()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        if !(1..=60).contains(&self.poll_interval_secs) {
            bail!("poll_interval_secs must be between 1 and 60");
        }

        Ok(())
    }
}

pub fn settings_path() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    let home = Path::new(&home);

    if !home.is_absolute() {
        bail!("HOME must be an absolute path");
    }

    Ok(home
        .join("Library")
        .join("Application Support")
        .join("NTFS Manager")
        .join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_safe() {
        let settings = Settings::default();
        assert!(settings.auto_mount);
        assert_eq!(settings.poll_interval_secs, 2);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn invalid_interval_is_rejected() {
        let settings = Settings {
            auto_mount: true,
            poll_interval_secs: 0,
        };

        assert!(settings.validate().is_err());
    }
}
