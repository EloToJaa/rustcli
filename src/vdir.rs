use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::config;

#[derive(Debug, Clone)]
pub struct TodoList {
    pub name: String,
    pub path: PathBuf,
}

pub fn discover_lists(path_glob: &str) -> Result<Vec<TodoList>> {
    let expanded = config::expand_home(path_glob);
    let pattern = expanded.to_string_lossy().to_string();
    let mut lists = Vec::new();

    for entry in glob::glob(&pattern)? {
        let path = match entry {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !path.is_dir() {
            continue;
        }
        let name = list_display_name(&path)?;
        lists.push(TodoList { name, path });
    }

    if lists.is_empty() {
        bail!("no todo lists found for pattern: {}", path_glob);
    }

    lists.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(lists)
}

fn list_display_name(path: &Path) -> Result<String> {
    let display = path.join("displayname");
    if display.exists() {
        let value = fs::read_to_string(display)?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    Ok(path.file_name().and_then(|name| name.to_str()).unwrap_or("unknown").to_string())
}
