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

    pub fn path_scale(self) -> f32 {
        match self {
            Self::Analytical => 1.0,
            Self::Ssaa4x4 => 4.0,
        }
    }
}

/// How the Text tool draws glyphs. Both follow the antialiasing mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextStyle {
    /// Heavier edges, baselines on pixel rows, and the font's bold file.
    #[default]
    Sharp,
    /// Lighter edges at the exact position, with simulated bold.
    Smooth,
}

impl TextStyle {
    pub const ALL: [Self; 2] = [Self::Sharp, Self::Smooth];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sharp => tr("text-style-sharp"),
            Self::Smooth => tr("text-style-smooth"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(default)]
    pub antialiasing: AaMode,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub text_style: TextStyle,
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
        super::storage::read_json(path)
    }

    pub fn save_at(&self, path: &Path) -> io::Result<()> {
        super::storage::write_json_atomic(path, self, super::storage::JsonWrite::PRETTY)
    }

    fn settings_path() -> io::Result<PathBuf> {
        super::storage::config_file("settings.json")
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
            for (theme, text_style) in [
                (Theme::Light, TextStyle::Smooth),
                (Theme::Dark, TextStyle::Sharp),
            ] {
                let saved = GlobalSettings {
                    antialiasing: mode,
                    theme,
                    text_style,
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
                text_style: TextStyle::Sharp,
            }
        );
        Ok(())
    }
}
