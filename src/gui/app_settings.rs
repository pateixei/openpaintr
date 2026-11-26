use std::{
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{i18n::text as tr, theme::Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AaMode {
    #[default]
    Analytical,
    Ssaa4x4,
}

impl AaMode {
    pub const ALL: [Self; 2] = [Self::Analytical, Self::Ssaa4x4];

    pub fn label(self) -> &'static str {
        match self {
            Self::Analytical => tr("aa-analytical"),
            Self::Ssaa4x4 => tr("aa-ssaa-4x4"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(default)]
    pub antialiasing: AaMode,
    #[serde(default)]
    pub theme: Theme,
}

impl GlobalSettings {
    pub fn load() -> Self {
        Self::settings_path()
            .and_then(|path| Self::load_at(&path))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_at(&Self::settings_path()?)
    }

    pub fn load_at(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    pub fn save_at(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        super::dialogs::atomic_write_bytes(path, &bytes, true)
    }

    fn settings_path() -> io::Result<PathBuf> {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return Ok(xdg.join("openpaintr").join("settings.json"));
        }

        #[cfg(windows)]
        let config_root = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|path| PathBuf::from(path).join(".config"))
                    .filter(|path| path.is_absolute())
            });

        #[cfg(not(windows))]
        let config_root = std::env::var_os("HOME")
            .map(|path| PathBuf::from(path).join(".config"))
            .filter(|path| path.is_absolute());

        if let Some(root) = config_root {
            return Ok(root.join("openpaintr").join("settings.json"));
        }

        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("settings.json")))
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_antialiasing_is_analytical() {
        assert_eq!(GlobalSettings::default().antialiasing, AaMode::Analytical);
    }

    #[test]
    fn settings_round_trip_and_legacy_json_defaults() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("openpaintr").join("settings.json");

        for mode in AaMode::ALL {
            for theme in [Theme::Light, Theme::Dark] {
                let saved = GlobalSettings {
                    antialiasing: mode,
                    theme,
                };
                saved.save_at(&path)?;
                assert_eq!(GlobalSettings::load_at(&path)?, saved);
            }
        }

        std::fs::write(&path, b"{}")?;
        assert_eq!(GlobalSettings::load_at(&path)?, GlobalSettings::default());
        std::fs::write(&path, br#"{"antialiasing":"ssaa4x4"}"#)?;
        assert_eq!(
            GlobalSettings::load_at(&path)?,
            GlobalSettings {
                antialiasing: AaMode::Ssaa4x4,
                theme: Theme::Light,
            }
        );
        Ok(())
    }
}
