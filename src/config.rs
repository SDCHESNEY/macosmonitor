use std::{fs, io};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub refresh_ms: u64,
    pub default_sort: String,
    pub theme: String,
    pub visible_columns: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_ms: 1000,
            default_sort: "cpu".to_owned(),
            theme: "dark".to_owned(),
            visible_columns: vec![
                "pid".to_owned(),
                "name".to_owned(),
                "user".to_owned(),
                "cpu".to_owned(),
                "memory".to_owned(),
                "status".to_owned(),
            ],
        }
    }
}

pub fn effective_refresh(cli_refresh: Option<u64>, config: &Config) -> u64 {
    cli_refresh.unwrap_or(config.refresh_ms).max(250)
}

pub fn path() -> io::Result<std::path::PathBuf> {
    let base = BaseDirs::new()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "determine home directory"))?;
    Ok(base.home_dir().join(".config/rtop/config.toml"))
}

pub fn load() -> (Config, Option<String>) {
    let Ok(path) = path() else {
        return (Config::default(), None);
    };
    match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(config) => (config, None),
            Err(error) => (
                Config::default(),
                Some(format!(
                    "Invalid config {}; using defaults: {error}",
                    path.display()
                )),
            ),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (Config::default(), None),
        Err(error) => (
            Config::default(),
            Some(format!(
                "Could not read config {}; using defaults: {error}",
                path.display()
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, effective_refresh};

    #[test]
    fn config_defaults_are_complete() {
        let config = Config::default();
        assert_eq!(config.refresh_ms, 1000);
        assert!(config.visible_columns.contains(&"cpu".to_owned()));
    }

    #[test]
    fn config_toml_parses_partial_values() {
        let config: Config =
            toml::from_str("refresh_ms = 250\ntheme = \"light\"\n").expect("valid config");
        assert_eq!(config.refresh_ms, 250);
        assert_eq!(config.theme, "light");
        assert!(!config.visible_columns.is_empty());
    }

    #[test]
    fn cli_refresh_overrides_config_then_defaults() {
        let config = Config {
            refresh_ms: 500,
            ..Config::default()
        };
        assert_eq!(effective_refresh(Some(750), &config), 750);
        assert_eq!(effective_refresh(None, &config), 500);
    }

    #[test]
    fn invalid_toml_is_rejected() {
        assert!(toml::from_str::<Config>("refresh_ms = [").is_err());
    }
}
