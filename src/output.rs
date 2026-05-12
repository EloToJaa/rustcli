use std::io::IsTerminal;

use anyhow::{Result, bail};
use chrono::{Duration, Local};

use crate::model::{Status, Todo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        if value.eq_ignore_ascii_case("always") {
            return Ok(Self::Always);
        }
        if value.eq_ignore_ascii_case("never") {
            return Ok(Self::Never);
        }
        bail!("invalid color mode: {} (expected auto, always, or never)", value)
    }

    pub fn enabled(self) -> bool {
        if self == Self::Always {
            return true;
        }
        if self == Self::Never {
            return false;
        }
        std::io::stdout().is_terminal()
    }
}

#[derive(Debug, Clone)]
pub struct OutputTheme {
    color_enabled: bool,
}

impl OutputTheme {
    pub fn from_mode(mode: ColorMode) -> Self {
        Self { color_enabled: mode.enabled() }
    }

    pub fn compact_row(
        &self,
        id: i64,
        todo: &Todo,
        show_list: bool,
        date_format: &str,
        list_color: Option<&str>,
    ) -> String {
        let due_text = todo.due.map(|due| due.format(date_format).to_string()).unwrap_or_default();
        let due = self.color_due(&due_text, todo.due, todo.status);
        let priority = self.color_priority(todo.priority_marker());

        if show_list {
            let list_name = self.color_list(&todo.list_name, list_color);
            return format!(
                "{} {} {:<3} {:<12} {} @{} ({}%)",
                id,
                todo.done_marker(),
                priority,
                due,
                todo.summary,
                list_name,
                todo.percent_complete
            );
        }

        format!(
            "{} {} {:<3} {:<12} {} ({}%)",
            id,
            todo.done_marker(),
            priority,
            due,
            todo.summary,
            todo.percent_complete
        )
    }

    pub fn print_detailed(
        &self,
        todo: &Todo,
        date_format: &str,
        time_format: &str,
        dt_separator: &str,
        list_color: Option<&str>,
    ) {
        println!("summary: {}", todo.summary);
        println!("uid: {}", todo.uid);
        println!("status: {}", todo.status.as_ical());
        println!("list: {}", self.color_list(&todo.list_name, list_color));
        if let Some(due) = todo.due {
            let due_text =
                format!("{}{}{}", due.format(date_format), dt_separator, due.format(time_format));
            println!("due: {}", self.color_due(&due_text, Some(due), todo.status));
        }
        if let Some(start) = todo.start {
            println!(
                "start: {}{}{}",
                start.format(date_format),
                dt_separator,
                start.format(time_format)
            );
        }
        if let Some(priority) = todo.priority {
            println!("priority: {}", priority);
        }
        if let Some(description) = &todo.description {
            println!("description: {}", description);
        }
        if let Some(location) = &todo.location {
            println!("location: {}", location);
        }
        if !todo.categories.is_empty() {
            println!("categories: {}", todo.categories.join(", "));
        }
        println!("path: {}", todo.path.display());
    }

    fn color_priority(&self, value: &str) -> String {
        self.paint_ansi(value, "35")
    }

    fn color_list(&self, value: &str, list_color: Option<&str>) -> String {
        if !self.color_enabled {
            return value.to_string();
        }
        let Some((r, g, b)) = parse_rgb_hex(list_color.unwrap_or_default()) else {
            return value.to_string();
        };
        format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, value)
    }

    fn color_due(
        &self,
        rendered: &str,
        due: Option<chrono::DateTime<Local>>,
        status: Status,
    ) -> String {
        if !self.color_enabled {
            return rendered.to_string();
        }
        let Some(due) = due else {
            return rendered.to_string();
        };
        if status != Status::Completed && due <= Local::now() {
            return self.paint_ansi(rendered, "31");
        }
        if due <= Local::now() + Duration::hours(24) {
            return self.paint_ansi(rendered, "33");
        }
        rendered.to_string()
    }

    fn paint_ansi(&self, text: &str, code: &str) -> String {
        if !self.color_enabled || text.is_empty() {
            return text.to_string();
        }
        format!("\x1b[{}m{}\x1b[0m", code, text)
    }
}

pub fn parse_rgb_hex(color: &str) -> Option<(u8, u8, u8)> {
    let value = color.trim();
    if !value.starts_with('#') || value.len() != 7 {
        return None;
    }
    let r = u8::from_str_radix(&value[1..3], 16).ok()?;
    let g = u8::from_str_radix(&value[3..5], 16).ok()?;
    let b = u8::from_str_radix(&value[5..7], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::{ColorMode, parse_rgb_hex};

    #[test]
    fn parses_rgb_hex_color() {
        assert_eq!(parse_rgb_hex("#112233"), Some((17, 34, 51)));
        assert_eq!(parse_rgb_hex("#zz2233"), None);
        assert_eq!(parse_rgb_hex("112233"), None);
    }

    #[test]
    fn parses_color_mode() {
        assert_eq!(ColorMode::parse("auto").expect("auto"), ColorMode::Auto);
        assert_eq!(ColorMode::parse("always").expect("always"), ColorMode::Always);
        assert_eq!(ColorMode::parse("never").expect("never"), ColorMode::Never);
        assert!(ColorMode::parse("sometimes").is_err());
    }
}
