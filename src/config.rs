use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use config::{Config as ConfigLoader, Environment, File};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub path_glob: String,
    pub cache_path: PathBuf,
    pub default_list: Option<String>,
    pub default_due_hours: i64,
    pub date_format: String,
    pub time_format: String,
    pub dt_separator: String,
    pub default_command: String,
    pub color: String,
    pub humanize: bool,
    pub startable: bool,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    path: Option<String>,
    cache_path: Option<String>,
    default_list: Option<String>,
    default_due: Option<i64>,
    date_format: Option<String>,
    time_format: Option<String>,
    dt_separator: Option<String>,
    default_command: Option<String>,
    color: Option<String>,
    humanize: Option<bool>,
    startable: Option<bool>,
}

impl Config {
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        let config_path = explicit_path
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("TODORS_CONFIG").map(PathBuf::from))
            .or_else(|| std::env::var_os("TODOMAN_CONFIG").map(PathBuf::from))
            .unwrap_or_else(default_config_path);

        let builder = ConfigLoader::builder()
            .set_default("path", "~/.local/share/calendars/*")?
            .set_default("cache_path", "~/.cache/todors/cache.sqlite3")?
            .set_default("default_due", 24)?
            .set_default("date_format", "%Y-%m-%d")?
            .set_default("time_format", "%H:%M")?
            .set_default("dt_separator", " ")?
            .set_default("default_command", "list")?
            .set_default("color", "auto")?
            .set_default("humanize", false)?
            .set_default("startable", false)?
            .add_source(File::from(config_path.clone()).required(false))
            .add_source(Environment::with_prefix("TODORS").separator("__"));

        let loaded = builder.build()?;
        let file_config = loaded.try_deserialize::<FileConfig>()?;

        let path_glob =
            file_config.path.unwrap_or_else(|| "~/.local/share/calendars/*".to_string());
        let cache_path = expand_home(
            file_config.cache_path.as_deref().unwrap_or("~/.cache/todors/cache.sqlite3"),
        );

        if path_glob.trim().is_empty() {
            bail!("config field 'path' cannot be empty");
        }

        Ok(Self {
            path_glob,
            cache_path,
            default_list: file_config.default_list,
            default_due_hours: file_config.default_due.unwrap_or(24),
            date_format: file_config.date_format.unwrap_or_else(|| "%Y-%m-%d".to_string()),
            time_format: file_config.time_format.unwrap_or_else(|| "%H:%M".to_string()),
            dt_separator: file_config.dt_separator.unwrap_or_else(|| " ".to_string()),
            default_command: file_config.default_command.unwrap_or_else(|| "list".to_string()),
            color: file_config.color.unwrap_or_else(|| "auto".to_string()),
            humanize: file_config.humanize.unwrap_or(false),
            startable: file_config.startable.unwrap_or(false),
        })
    }
}

fn default_config_path() -> PathBuf {
    if let Some(xdg_config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg_config).join("todors/config.toml");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".config/todors/config.toml");
    }
    PathBuf::from("todors/config.toml")
}

pub fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(value)
}
