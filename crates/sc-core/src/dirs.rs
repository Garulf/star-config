use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl AppDirs {
    pub fn from_system() -> Option<Self> {
        let dirs = directories::ProjectDirs::from("", "", "star-config")?;
        Some(Self {
            config: dirs.config_dir().to_path_buf(),
            data: dirs.data_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
        })
    }

    pub fn under(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }

    pub fn shader_backups(&self) -> PathBuf {
        self.data.join("shader-backups")
    }

    pub fn thumbnails(&self) -> PathBuf {
        self.cache.join("thumbnails")
    }

    pub fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_places_everything_below_the_root() {
        let root = Path::new("root");
        let dirs = AppDirs::under(root);
        assert_eq!(dirs.settings_file(), root.join("config/settings.toml"));
        assert_eq!(dirs.backups(), root.join("data/backups"));
        assert_eq!(dirs.shader_backups(), root.join("data/shader-backups"));
        assert_eq!(dirs.thumbnails(), root.join("cache/thumbnails"));
        assert_eq!(dirs.downloads(), root.join("cache/downloads"));
    }
}
