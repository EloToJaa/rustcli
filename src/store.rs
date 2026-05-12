use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::config::Config;
use crate::model::{Status, Todo};
use crate::vdir::{TodoList, discover_lists};

pub struct AppStore {
    conn: Connection,
    lists: Vec<TodoList>,
}

impl AppStore {
    pub fn open(config: &Config) -> Result<Self> {
        let lists = discover_lists(&config.path_glob)?;
        if let Some(parent) = config.cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&config.cache_path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS todo_ids (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                list_name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                uid TEXT NOT NULL,
                UNIQUE(list_name, file_path)
            );
            ",
        )?;
        Ok(Self { conn, lists })
    }

    pub fn lists(&self) -> &[TodoList] {
        &self.lists
    }

    pub fn list_by_name(&self, name: &str) -> Option<&TodoList> {
        self.lists.iter().find(|list| list.name.eq_ignore_ascii_case(name))
    }

    pub fn all_todos(&mut self) -> Result<Vec<(i64, Todo)>> {
        let mut items = Vec::new();
        for list in &self.lists {
            for entry in fs::read_dir(&list.path)? {
                let entry = entry?;
                let path = entry.path();
                let is_ics = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("ics"))
                    .unwrap_or(false);
                if !is_ics {
                    continue;
                }
                let todo = parse_ics_file(&path, &list.name)
                    .with_context(|| format!("failed to parse {}", path.display()))?;
                if let Some(mut todo) = todo {
                    todo.path = path.clone();
                    let id = self.ensure_id(&list.name, &path, &todo.uid)?;
                    items.push((id, todo));
                }
            }
        }
        items.sort_by_key(|item| item.0);
        Ok(items)
    }

    pub fn todo_by_id(&mut self, id: i64) -> Result<Todo> {
        let (list_name, file_path): (String, String) = self
            .conn
            .query_row("SELECT list_name, file_path FROM todo_ids WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("todo id {} not found", id))?;
        let path = PathBuf::from(file_path);
        if !path.exists() {
            bail!("todo id {} points to missing file", id);
        }
        let mut todo = parse_ics_file(&path, &list_name)?
            .ok_or_else(|| anyhow::anyhow!("todo id {} does not reference a VTODO file", id))?;
        todo.path = path;
        Ok(todo)
    }

    pub fn save_new(&mut self, list: &TodoList, todo: &mut Todo) -> Result<i64> {
        if todo.uid.is_empty() {
            todo.uid = Uuid::new_v4().to_string();
        }
        let file_path = list.path.join(format!("{}.ics", todo.uid));
        write_ics_file(&file_path, todo)?;
        todo.path = file_path.clone();
        todo.list_name = list.name.clone();
        let id = self.ensure_id(&list.name, &file_path, &todo.uid)?;
        Ok(id)
    }

    pub fn save_existing(&mut self, todo: &Todo) -> Result<()> {
        write_ics_file(&todo.path, todo)?;
        Ok(())
    }

    pub fn delete_by_id(&mut self, id: i64) -> Result<()> {
        let (list_name, file_path): (String, String) = self
            .conn
            .query_row("SELECT list_name, file_path FROM todo_ids WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("todo id {} not found", id))?;
        let path = PathBuf::from(file_path);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        self.conn.execute(
            "DELETE FROM todo_ids WHERE id = ?1 AND list_name = ?2",
            params![id, list_name],
        )?;
        Ok(())
    }

    pub fn move_to_list(&mut self, id: i64, list: &TodoList) -> Result<()> {
        let mut todo = self.todo_by_id(id)?;
        let target_path = list
            .path
            .join(todo.path.file_name().and_then(|name| name.to_str()).unwrap_or("task.ics"));
        fs::rename(&todo.path, &target_path)?;
        todo.path = target_path.clone();
        todo.list_name = list.name.clone();
        self.conn.execute(
            "UPDATE todo_ids SET list_name = ?1, file_path = ?2 WHERE id = ?3",
            params![list.name, target_path.to_string_lossy().to_string(), id],
        )?;
        Ok(())
    }

    pub fn copy_to_list(&mut self, id: i64, list: &TodoList) -> Result<i64> {
        let mut todo = self.todo_by_id(id)?;
        todo.uid = Uuid::new_v4().to_string();
        todo.status = Status::NeedsAction;
        todo.percent_complete = 0;
        self.save_new(list, &mut todo)
    }

    pub fn flush_done(&mut self) -> Result<usize> {
        let mut deleted = 0usize;
        let todos = self.all_todos()?;
        for (id, todo) in todos {
            if todo.status == Status::Completed {
                self.delete_by_id(id)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    fn ensure_id(&self, list_name: &str, file_path: &Path, uid: &str) -> Result<i64> {
        let path = file_path.to_string_lossy().to_string();
        self.conn.execute(
            "INSERT OR IGNORE INTO todo_ids (list_name, file_path, uid) VALUES (?1, ?2, ?3)",
            params![list_name, path, uid],
        )?;
        let id = self
            .conn
            .query_row(
                "SELECT id FROM todo_ids WHERE list_name = ?1 AND file_path = ?2",
                params![list_name, file_path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("failed to resolve todo id"))?;
        Ok(id)
    }
}

fn parse_ics_file(path: &Path, list_name: &str) -> Result<Option<Todo>> {
    let raw = fs::read_to_string(path)?;
    let mut fields = HashMap::new();
    let mut other = Vec::new();
    let mut in_vtodo = false;
    for line in unfold_lines(&raw) {
        if line == "BEGIN:VTODO" {
            in_vtodo = true;
            continue;
        }
        if line == "END:VTODO" {
            break;
        }
        if !in_vtodo {
            continue;
        }
        if let Some((key, value)) = split_ical_line(&line) {
            match key {
                "UID" | "SUMMARY" | "DESCRIPTION" | "LOCATION" | "DUE" | "DTSTART" | "STATUS"
                | "CATEGORIES" | "PRIORITY" | "PERCENT-COMPLETE" => {
                    fields.insert(key.to_string(), value.to_string());
                }
                _ => other.push(line),
            }
        }
    }

    if fields.is_empty() {
        return Ok(None);
    }

    let uid = fields.get("UID").cloned().unwrap_or_else(|| Uuid::new_v4().to_string());
    let summary = fields.get("SUMMARY").map(|value| unescape_ical_text(value)).unwrap_or_default();
    let description = fields.get("DESCRIPTION").map(|value| unescape_ical_text(value));
    let location = fields.get("LOCATION").map(|value| unescape_ical_text(value));
    let due = fields.get("DUE").and_then(|v| parse_datetime(v));
    let start = fields.get("DTSTART").and_then(|v| parse_datetime(v));
    let status = fields.get("STATUS").map(|v| Status::parse(v)).unwrap_or(Status::NeedsAction);
    let priority = fields.get("PRIORITY").and_then(|v| v.parse::<u8>().ok());
    let percent_complete = fields
        .get("PERCENT-COMPLETE")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(if status == Status::Completed { 100 } else { 0 });
    let categories =
        fields.get("CATEGORIES").map(|value| parse_ical_list(value)).unwrap_or_default();

    Ok(Some(Todo {
        uid,
        summary,
        description,
        location,
        due,
        start,
        status,
        priority,
        categories,
        percent_complete,
        list_name: list_name.to_string(),
        path: path.to_path_buf(),
        raw_other: other,
    }))
}

fn write_ics_file(path: &Path, todo: &Todo) -> Result<()> {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//todors//EN".to_string(),
        "BEGIN:VTODO".to_string(),
        format!("UID:{}", todo.uid),
        format!("SUMMARY:{}", escape(&todo.summary)),
        format!("STATUS:{}", todo.status.as_ical()),
        format!("PERCENT-COMPLETE:{}", todo.percent_complete),
        format!("DTSTAMP:{}", Utc::now().format("%Y%m%dT%H%M%SZ")),
    ];

    if let Some(desc) = &todo.description {
        lines.push(format!("DESCRIPTION:{}", escape(desc)));
    }
    if let Some(location) = &todo.location {
        lines.push(format!("LOCATION:{}", escape(location)));
    }
    if let Some(due) = todo.due {
        lines.push(format!("DUE:{}", due.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ")));
    }
    if let Some(start) = todo.start {
        lines.push(format!("DTSTART:{}", start.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ")));
    }
    if let Some(priority) = todo.priority {
        lines.push(format!("PRIORITY:{}", priority));
    }
    if !todo.categories.is_empty() {
        lines.push(format!("CATEGORIES:{}", escape(&todo.categories.join(","))));
    }

    lines.extend(todo.raw_other.iter().cloned());
    lines.push("END:VTODO".to_string());
    lines.push("END:VCALENDAR".to_string());

    fs::write(path, lines.join("\r\n"))?;
    Ok(())
}

fn split_ical_line(line: &str) -> Option<(&str, &str)> {
    let (left, right) = line.split_once(':')?;
    let key = left.split_once(';').map(|(k, _)| k).unwrap_or(left);
    Some((key, right))
}

fn parse_datetime(value: &str) -> Option<DateTime<Local>> {
    if let Ok(dt) = DateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ") {
        return Some(dt.with_timezone(&Local));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S") {
        let local = Local.from_local_datetime(&naive).single();
        if local.is_some() {
            return local;
        }
    }
    None
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n").replace(',', "\\,").replace(';', "\\;")
}

fn unescape_ical_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        if next == 'n' || next == 'N' {
            out.push('\n');
            continue;
        }
        if next == '\\' || next == ';' || next == ',' {
            out.push(next);
            continue;
        }
        out.push(next);
    }
    out
}

fn parse_ical_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == ',' {
            let item = unescape_ical_text(current.trim());
            if !item.is_empty() {
                items.push(item);
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }
    if escaped {
        current.push('\\');
    }
    let item = unescape_ical_text(current.trim());
    if !item.is_empty() {
        items.push(item);
    }
    items
}

fn unfold_lines(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        if let Some(last) = out.last_mut()
            && (line.starts_with(' ') || line.starts_with('\t'))
        {
            last.push_str(line.trim_start());
            continue;
        }
        out.push(line.trim_end_matches('\r').to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::config::Config;
    use crate::model::Status;

    use super::AppStore;

    #[test]
    fn preserves_unknown_ics_properties_on_update() {
        let temp = tempdir().expect("temp dir");
        let list_dir = temp.path().join("home");
        fs::create_dir_all(&list_dir).expect("list dir");

        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:test-uid\r\nSUMMARY:Task\r\nSTATUS:NEEDS-ACTION\r\nX-CUSTOM:KEEP-ME\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let task_path = list_dir.join("a.ics");
        fs::write(&task_path, ics).expect("write fixture");

        let config = Config {
            path_glob: format!("{}/*", temp.path().display()),
            cache_path: temp.path().join("cache.sqlite3"),
            default_list: None,
            default_due_hours: 24,
            date_format: "%Y-%m-%d".to_string(),
            time_format: "%H:%M".to_string(),
            dt_separator: " ".to_string(),
            default_command: "list".to_string(),
            color: "auto".to_string(),
            humanize: false,
            startable: false,
        };

        let mut store = AppStore::open(&config).expect("open store");
        let todos = store.all_todos().expect("read todos");
        let (id, mut todo) = todos[0].clone();
        assert_eq!(todo.summary, "Task");

        todo.status = Status::Completed;
        todo.percent_complete = 100;
        store.save_existing(&todo).expect("save todo");

        let raw = fs::read_to_string(task_path).expect("read updated");
        assert!(raw.contains("X-CUSTOM:KEEP-ME"));

        let saved = store.todo_by_id(id).expect("reload by id");
        assert_eq!(saved.status, Status::Completed);
    }

    #[test]
    fn copy_generates_new_uid_and_new_id() {
        let temp = tempdir().expect("temp dir");
        let home_dir = temp.path().join("home");
        let work_dir = temp.path().join("work");
        fs::create_dir_all(&home_dir).expect("home dir");
        fs::create_dir_all(&work_dir).expect("work dir");

        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:test-uid\r\nSUMMARY:Task\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        fs::write(home_dir.join("a.ics"), ics).expect("write fixture");

        let config = Config {
            path_glob: format!("{}/*", temp.path().display()),
            cache_path: temp.path().join("cache.sqlite3"),
            default_list: None,
            default_due_hours: 24,
            date_format: "%Y-%m-%d".to_string(),
            time_format: "%H:%M".to_string(),
            dt_separator: " ".to_string(),
            default_command: "list".to_string(),
            color: "auto".to_string(),
            humanize: false,
            startable: false,
        };

        let mut store = AppStore::open(&config).expect("open store");
        let original = store.all_todos().expect("todos");
        let original_id = original[0].0;
        let original_uid = original[0].1.uid.clone();

        let work = store.list_by_name("work").expect("work list").clone();
        let copied_id = store.copy_to_list(original_id, &work).expect("copy todo");
        assert_ne!(copied_id, original_id);

        let copied = store.todo_by_id(copied_id).expect("copied todo");
        assert_ne!(copied.uid, original_uid);
        assert_eq!(copied.list_name.to_ascii_lowercase(), "work");
    }

    #[test]
    fn ignores_vevent_files_when_listing_todos() {
        let temp = tempdir().expect("temp dir");
        let list_dir = temp.path().join("home");
        fs::create_dir_all(&list_dir).expect("list dir");

        let vevent = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nSUMMARY:Meeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let vtodo = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:Actual Task\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        fs::write(list_dir.join("event.ics"), vevent).expect("write event");
        fs::write(list_dir.join("task.ics"), vtodo).expect("write todo");

        let config = Config {
            path_glob: format!("{}/*", temp.path().display()),
            cache_path: temp.path().join("cache.sqlite3"),
            default_list: None,
            default_due_hours: 24,
            date_format: "%Y-%m-%d".to_string(),
            time_format: "%H:%M".to_string(),
            dt_separator: " ".to_string(),
            default_command: "list".to_string(),
            color: "auto".to_string(),
            humanize: false,
            startable: false,
        };

        let mut store = AppStore::open(&config).expect("open store");
        let todos = store.all_todos().expect("todos");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].1.summary, "Actual Task");
    }

    #[test]
    fn unescapes_text_fields_from_ics() {
        let temp = tempdir().expect("temp dir");
        let list_dir = temp.path().join("home");
        fs::create_dir_all(&list_dir).expect("list dir");

        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:test-uid\r\nSUMMARY:Comma\\, Semi\\; Slash\\\\\r\nDESCRIPTION:Line 1\\nLine 2\r\nLOCATION:Office\\, 2nd floor\r\nSTATUS:NEEDS-ACTION\r\nCATEGORIES:ops\\,oncall,infra\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        fs::write(list_dir.join("task.ics"), ics).expect("write fixture");

        let config = Config {
            path_glob: format!("{}/*", temp.path().display()),
            cache_path: temp.path().join("cache.sqlite3"),
            default_list: None,
            default_due_hours: 24,
            date_format: "%Y-%m-%d".to_string(),
            time_format: "%H:%M".to_string(),
            dt_separator: " ".to_string(),
            default_command: "list".to_string(),
            color: "auto".to_string(),
            humanize: false,
            startable: false,
        };

        let mut store = AppStore::open(&config).expect("open store");
        let todos = store.all_todos().expect("todos");
        let todo = &todos[0].1;

        assert_eq!(todo.summary, "Comma, Semi; Slash\\");
        assert_eq!(todo.description.as_deref(), Some("Line 1\nLine 2"));
        assert_eq!(todo.location.as_deref(), Some("Office, 2nd floor"));
        assert_eq!(todo.categories, vec!["ops,oncall".to_string(), "infra".to_string()]);
    }
}
