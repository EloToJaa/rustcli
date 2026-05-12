use std::cmp::Ordering;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use anyhow::{Result, bail};
use chrono::{Duration, Local};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::Config;
use crate::model::{Status, Todo};
use crate::output;
use crate::store::AppStore;

#[derive(Debug, Parser)]
#[command(name = "todo")]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    porcelain: bool,
    #[arg(long = "colour", alias = "color")]
    color: Option<String>,
    #[arg(long)]
    humanize: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    List(ListArgs),
    New(NewArgs),
    Show {
        id: i64,
    },
    Edit(EditArgs),
    Done {
        ids: Vec<i64>,
    },
    Undo {
        ids: Vec<i64>,
    },
    Cancel {
        ids: Vec<i64>,
    },
    Delete {
        ids: Vec<i64>,
        #[arg(long)]
        yes: bool,
    },
    Flush,
    Lists,
    Repl,
    Path {
        id: i64,
    },
    Move {
        id: i64,
        #[arg(short, long)]
        list: String,
    },
    Copy {
        id: i64,
        #[arg(short, long)]
        list: String,
    },
}

#[derive(Debug, Args)]
struct ListArgs {
    lists: Vec<String>,
    #[arg(long)]
    location: Option<String>,
    #[arg(long)]
    grep: Option<String>,
    #[arg(long)]
    sort: Option<String>,
    #[arg(long, default_value_t = true)]
    reverse: bool,
    #[arg(long)]
    no_reverse: bool,
    #[arg(long)]
    due: Option<i64>,
    #[arg(short = 'c', long)]
    category: Vec<String>,
    #[arg(long)]
    priority: Option<u8>,
    #[arg(long, value_names = ["WHEN", "DATE"], num_args = 2)]
    start: Option<Vec<String>>,
    #[arg(long)]
    startable: bool,
    #[arg(short = 's', long, default_value = "NEEDS-ACTION,IN-PROCESS")]
    status: String,
    #[arg(short, long)]
    all: bool,
}

#[derive(Debug, Args)]
struct NewArgs {
    summary: Vec<String>,
    #[arg(short, long)]
    list: Option<String>,
    #[arg(short = 'd', long)]
    due_hours: Option<i64>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    location: Option<String>,
    #[arg(long)]
    priority: Option<u8>,
    #[arg(short = 'c', long)]
    category: Vec<String>,
    #[arg(short = 'r', long)]
    read_description: bool,
}

#[derive(Debug, Args)]
struct EditArgs {
    id: i64,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    location: Option<String>,
    #[arg(long)]
    priority: Option<u8>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    due_hours: Option<i64>,
    #[arg(long)]
    clear_due: bool,
    #[arg(long)]
    start_hours: Option<i64>,
    #[arg(long)]
    clear_start: bool,
    #[arg(short = 'c', long)]
    category: Vec<String>,
    #[arg(short = 'r', long)]
    read_description: bool,
    #[arg(long)]
    raw: bool,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

pub fn run(cli: Cli, config: &Config, app: &mut AppStore) -> Result<()> {
    let _cli_color = cli.color.as_deref().unwrap_or(config.color.as_str());
    let _humanize = cli.humanize || config.humanize;
    let command = match cli.command {
        Some(command) => command,
        None => command_from_default(config),
    };

    match command {
        Command::List(args) => list(args, config, app, cli.porcelain),
        Command::New(args) => create(args, config, app),
        Command::Show { id } => show(id, config, app, cli.porcelain),
        Command::Edit(args) => edit(args, config, app, cli.porcelain),
        Command::Done { ids } => update_status(ids, Status::Completed, app, cli.porcelain),
        Command::Undo { ids } => update_status(ids, Status::NeedsAction, app, cli.porcelain),
        Command::Cancel { ids } => update_status(ids, Status::Cancelled, app, cli.porcelain),
        Command::Delete { ids, yes } => delete(ids, yes, app, cli.porcelain),
        Command::Flush => flush(app, cli.porcelain),
        Command::Lists => list_lists(app, cli.porcelain),
        Command::Repl => repl_loop(config, app, cli.porcelain),
        Command::Path { id } => path(id, app),
        Command::Move { id, list } => move_todo(id, &list, app, cli.porcelain),
        Command::Copy { id, list } => copy_todo(id, &list, app, cli.porcelain),
    }
}

fn command_from_default(config: &Config) -> Command {
    let list = || {
        Command::List(ListArgs {
            lists: Vec::new(),
            location: None,
            grep: None,
            sort: None,
            reverse: true,
            no_reverse: false,
            due: None,
            category: Vec::new(),
            priority: None,
            start: None,
            startable: false,
            status: "NEEDS-ACTION,IN-PROCESS".to_string(),
            all: false,
        })
    };

    let default = config.default_command.trim().to_ascii_lowercase();
    if default.is_empty() {
        return list();
    }
    if default == "list" {
        return list();
    }
    if default == "lists" {
        return Command::Lists;
    }
    if default == "repl" {
        return Command::Repl;
    }
    if default == "flush" {
        return Command::Flush;
    }
    list()
}

fn list(args: ListArgs, config: &Config, app: &mut AppStore, porcelain: bool) -> Result<()> {
    let mut todos = app.all_todos()?;
    let _color_mode = config.color.as_str();
    if !args.lists.is_empty() {
        todos.retain(|(_, todo)| {
            args.lists.iter().any(|name| todo.list_name.eq_ignore_ascii_case(name))
        });
    }
    if !args.all {
        let statuses = parse_status_filter(&args.status)?;
        todos.retain(|(_, todo)| statuses.contains(&todo.status));
    }
    if let Some(grep) = args.grep {
        let needle = grep.to_ascii_lowercase();
        todos.retain(|(_, todo)| {
            todo.summary.to_ascii_lowercase().contains(&needle)
                || todo
                    .description
                    .as_ref()
                    .map(|value| value.to_ascii_lowercase().contains(&needle))
                    .unwrap_or(false)
        });
    }
    if let Some(location) = args.location {
        let needle = location.to_ascii_lowercase();
        todos.retain(|(_, todo)| {
            todo.location
                .as_ref()
                .map(|value| value.to_ascii_lowercase().contains(&needle))
                .unwrap_or(false)
        });
    }
    if !args.category.is_empty() {
        todos.retain(|(_, todo)| {
            args.category
                .iter()
                .all(|cat| todo.categories.iter().any(|item| item.eq_ignore_ascii_case(cat)))
        });
    }
    if let Some(priority) = args.priority {
        todos.retain(|(_, todo)| {
            todo.priority.unwrap_or(0) > 0 && todo.priority.unwrap_or(0) <= priority
        });
    }
    if let Some(hours) = args.due {
        let limit = Local::now() + Duration::hours(hours);
        todos.retain(|(_, todo)| todo.due.map(|due| due <= limit).unwrap_or(false));
    }
    if args.startable || config.startable {
        let now = Local::now();
        todos.retain(|(_, todo)| todo.start.map(|start| start <= now).unwrap_or(true));
    }
    if let Some(start_filter) = args.start
        && start_filter.len() == 2
    {
        let mode = start_filter[0].to_ascii_lowercase();
        let dt = parse_user_datetime(&start_filter[1], config)?;
        todos.retain(|(_, todo)| {
            if let Some(start) = todo.start {
                if mode == "before" {
                    return start <= dt;
                }
                if mode == "after" {
                    return start >= dt;
                }
            }
            false
        });
    }

    let reverse = if args.no_reverse { false } else { args.reverse };
    sort_todos(&mut todos, args.sort.as_deref(), reverse);

    if porcelain {
        let payload: Vec<PorcelainTodo> =
            todos.iter().map(|(id, todo)| PorcelainTodo::from_parts(*id, todo)).collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let show_list = args.lists.is_empty() && app.lists().len() > 1;
    for (id, todo) in todos {
        print_compact_row(id, &todo, show_list, &config.date_format);
    }
    Ok(())
}

fn create(args: NewArgs, config: &Config, app: &mut AppStore) -> Result<()> {
    if args.summary.is_empty() {
        bail!("summary is required");
    }
    let list_name = if let Some(list_name) = args.list {
        list_name
    } else if let Some(default) = &config.default_list {
        default.clone()
    } else {
        bail!("missing --list and no default_list in config");
    };
    let list = app
        .list_by_name(&list_name)
        .ok_or_else(|| anyhow::anyhow!("unknown list: {}", list_name))?
        .clone();
    let due_hours = args.due_hours.unwrap_or(config.default_due_hours);
    let due = if due_hours > 0 { Some(Local::now() + Duration::hours(due_hours)) } else { None };

    let description =
        if args.read_description { Some(read_stdin_all()?) } else { args.description };

    let mut todo = Todo {
        uid: String::new(),
        summary: args.summary.join(" "),
        description,
        location: args.location,
        due,
        start: None,
        status: Status::NeedsAction,
        priority: args.priority,
        categories: args.category,
        percent_complete: 0,
        list_name: list.name.clone(),
        path: PathBuf::new(),
        raw_other: Vec::new(),
    };
    let id = app.save_new(&list, &mut todo)?;
    println!("created {}", id);
    output::print_detailed(&todo, &config.date_format, &config.time_format, &config.dt_separator);
    Ok(())
}

fn show(id: i64, config: &Config, app: &mut AppStore, porcelain: bool) -> Result<()> {
    let todo = app.todo_by_id(id)?;
    if porcelain {
        println!("{}", serde_json::to_string_pretty(&PorcelainTodo::from_parts(id, &todo))?);
        return Ok(());
    }
    output::print_detailed(&todo, &config.date_format, &config.time_format, &config.dt_separator);
    Ok(())
}

fn edit(args: EditArgs, config: &Config, app: &mut AppStore, porcelain: bool) -> Result<()> {
    let mut todo = app.todo_by_id(args.id)?;
    if args.raw {
        edit_raw_file(&todo.path)?;
        let updated = app.todo_by_id(args.id)?;
        if porcelain {
            println!(
                "{}",
                serde_json::to_string_pretty(&PorcelainTodo::from_parts(args.id, &updated))?
            );
            return Ok(());
        }
        output::print_detailed(
            &updated,
            &config.date_format,
            &config.time_format,
            &config.dt_separator,
        );
        return Ok(());
    }
    if let Some(summary) = args.summary {
        todo.summary = summary;
    }
    if let Some(description) = args.description {
        todo.description = Some(description);
    }
    if args.read_description {
        todo.description = Some(read_stdin_all()?);
    }
    if let Some(location) = args.location {
        todo.location = Some(location);
    }
    if let Some(priority) = args.priority {
        todo.priority = Some(priority);
    }
    if !args.category.is_empty() {
        todo.categories = args.category;
    }
    if let Some(status) = args.status {
        let parsed = Status::parse_filter(&status)
            .ok_or_else(|| anyhow::anyhow!("invalid status: {}", status))?;
        todo.status = parsed;
        todo.percent_complete = if parsed == Status::Completed { 100 } else { 0 };
    }
    if let Some(hours) = args.due_hours {
        if hours <= 0 {
            todo.due = None;
        } else {
            todo.due = Some(Local::now() + Duration::hours(hours));
        }
    }
    if args.clear_due {
        todo.due = None;
    }
    if let Some(hours) = args.start_hours {
        if hours <= 0 {
            todo.start = None;
        } else {
            todo.start = Some(Local::now() + Duration::hours(hours));
        }
    }
    if args.clear_start {
        todo.start = None;
    }

    app.save_existing(&todo)?;
    if porcelain {
        println!("{}", serde_json::to_string_pretty(&PorcelainTodo::from_parts(args.id, &todo))?);
        return Ok(());
    }
    output::print_detailed(&todo, &config.date_format, &config.time_format, &config.dt_separator);
    Ok(())
}

fn update_status(ids: Vec<i64>, status: Status, app: &mut AppStore, porcelain: bool) -> Result<()> {
    if ids.is_empty() {
        bail!("at least one id is required");
    }
    let mut payload = Vec::new();
    for id in ids {
        let mut todo = app.todo_by_id(id)?;
        todo.status = status;
        todo.percent_complete = if status == Status::Completed { 100 } else { 0 };
        app.save_existing(&todo)?;
        if porcelain {
            payload.push(PorcelainTodo::from_parts(id, &todo));
        } else {
            println!("updated {}", id);
        }
    }
    if porcelain {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }
    Ok(())
}

fn delete(ids: Vec<i64>, yes: bool, app: &mut AppStore, porcelain: bool) -> Result<()> {
    if ids.is_empty() {
        bail!("at least one id is required");
    }
    if !yes && !porcelain && !confirm("Do you want to delete those tasks?")? {
        println!("aborted");
        return Ok(());
    }
    for id in &ids {
        app.delete_by_id(*id)?;
        if !porcelain {
            println!("deleted {}", id);
        }
    }
    if porcelain {
        println!("{}", serde_json::json!({"deleted": ids}));
    }
    Ok(())
}

fn flush(app: &mut AppStore, porcelain: bool) -> Result<()> {
    let deleted = app.flush_done()?;
    if porcelain {
        println!("{}", serde_json::json!({"flushed": deleted}));
        return Ok(());
    }
    println!("flushed {} completed todos", deleted);
    Ok(())
}

fn list_lists(app: &AppStore, porcelain: bool) -> Result<()> {
    if porcelain {
        let names: Vec<&str> = app.lists().iter().map(|list| list.name.as_str()).collect();
        println!("{}", serde_json::to_string_pretty(&names)?);
        return Ok(());
    }
    for list in app.lists() {
        println!("{}", list.name);
    }
    Ok(())
}

fn path(id: i64, app: &mut AppStore) -> Result<()> {
    let todo = app.todo_by_id(id)?;
    println!("{}", todo.path.display());
    Ok(())
}

fn move_todo(id: i64, list_name: &str, app: &mut AppStore, porcelain: bool) -> Result<()> {
    let list = app
        .list_by_name(list_name)
        .ok_or_else(|| anyhow::anyhow!("unknown list: {}", list_name))?
        .clone();
    app.move_to_list(id, &list)?;
    if porcelain {
        println!("{}", serde_json::json!({"moved": id, "list": list.name}));
        return Ok(());
    }
    println!("moved {} to {}", id, list.name);
    Ok(())
}

fn copy_todo(id: i64, list_name: &str, app: &mut AppStore, porcelain: bool) -> Result<()> {
    let list = app
        .list_by_name(list_name)
        .ok_or_else(|| anyhow::anyhow!("unknown list: {}", list_name))?
        .clone();
    let new_id = app.copy_to_list(id, &list)?;
    if porcelain {
        println!(
            "{}",
            serde_json::json!({"copied_from": id, "copied_to": new_id, "list": list.name})
        );
        return Ok(());
    }
    println!("copied {} to {} as {}", id, list.name, new_id);
    Ok(())
}

fn edit_raw_file(path: &Path) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = ProcessCommand::new("sh")
        .arg("-c")
        .arg("$1 \"$2\"")
        .arg("sh")
        .arg(&editor)
        .arg(path)
        .status()?;
    if status.success() {
        return Ok(());
    }
    bail!("editor exited with status: {}", status)
}

fn repl_loop(config: &Config, app: &mut AppStore, porcelain: bool) -> Result<()> {
    use std::io::{self, Write};

    let mut line = String::new();
    loop {
        print!("todo> ");
        io::stdout().flush()?;
        line.clear();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok(());
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            return Ok(());
        }
        if input.eq_ignore_ascii_case("list") {
            let args = ListArgs {
                lists: Vec::new(),
                location: None,
                grep: None,
                sort: None,
                reverse: true,
                no_reverse: false,
                due: None,
                category: Vec::new(),
                priority: None,
                start: None,
                startable: false,
                status: "NEEDS-ACTION,IN-PROCESS".to_string(),
                all: false,
            };
            list(args, config, app, porcelain)?;
            continue;
        }
        if input.eq_ignore_ascii_case("lists") {
            list_lists(app, porcelain)?;
            continue;
        }
        if let Some(rest) = input.strip_prefix("show ")
            && let Ok(id) = rest.trim().parse::<i64>()
        {
            show(id, config, app, porcelain)?;
            continue;
        }
        println!("unsupported repl command: {}", input);
    }
}

fn read_stdin_all() -> Result<String> {
    use std::io::Read;

    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
}

fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{self, Write};

    print!("{} [y/N]: ", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let value = input.trim().to_ascii_lowercase();
    Ok(value == "y" || value == "yes")
}

fn parse_status_filter(raw: &str) -> Result<Vec<Status>> {
    let mut statuses = Vec::new();
    for token in raw.split(',').map(str::trim).filter(|part| !part.is_empty()) {
        if token.eq_ignore_ascii_case("ANY") {
            return Ok(vec![
                Status::NeedsAction,
                Status::InProcess,
                Status::Completed,
                Status::Cancelled,
            ]);
        }
        let status = Status::parse_filter(token)
            .ok_or_else(|| anyhow::anyhow!("invalid status token: {}", token))?;
        statuses.push(status);
    }
    Ok(statuses)
}

fn parse_user_datetime(raw: &str, config: &Config) -> Result<chrono::DateTime<Local>> {
    let full = format!("{}{}{}", raw, config.dt_separator, "00:00");
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(
        &full,
        &format!("{}{}{}", config.date_format, config.dt_separator, config.time_format),
    ) && let Some(value) = naive.and_local_timezone(Local).single()
    {
        return Ok(value);
    }
    let naive = chrono::NaiveDate::parse_from_str(raw, &config.date_format)?;
    let with_time = naive.and_hms_opt(0, 0, 0).ok_or_else(|| anyhow::anyhow!("invalid date"))?;
    with_time
        .and_local_timezone(Local)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid local datetime"))
}

fn sort_todos(todos: &mut [(i64, Todo)], sort: Option<&str>, reverse: bool) {
    let keys: Vec<&str> = sort
        .unwrap_or("due,priority")
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    todos.sort_by(|left, right| {
        for key in &keys {
            let ascending = key.starts_with('-');
            let field = key.trim_start_matches('-');
            let mut ord = match field {
                "id" => left.0.cmp(&right.0),
                "summary" => left.1.summary.cmp(&right.1.summary),
                "priority" => left.1.priority.unwrap_or(255).cmp(&right.1.priority.unwrap_or(255)),
                "due" => left.1.due.cmp(&right.1.due),
                "start" => left.1.start.cmp(&right.1.start),
                "status" => left.1.status.as_str().cmp(right.1.status.as_str()),
                "location" => left.1.location.cmp(&right.1.location),
                _ => Ordering::Equal,
            };
            if !ascending {
                ord = ord.reverse();
            }
            if ord != Ordering::Equal {
                return if reverse { ord.reverse() } else { ord };
            }
        }
        left.0.cmp(&right.0)
    });
}

fn print_compact_row(id: i64, todo: &Todo, show_list: bool, date_format: &str) {
    let due = todo.due.map(|due| due.format(date_format).to_string()).unwrap_or_default();
    if show_list {
        println!(
            "{} {} {:<3} {:<12} {} @{} ({}%)",
            id,
            todo.done_marker(),
            todo.priority_marker(),
            due,
            todo.summary,
            todo.list_name,
            todo.percent_complete
        );
        return;
    }
    println!(
        "{} {} {:<3} {:<12} {} ({}%)",
        id,
        todo.done_marker(),
        todo.priority_marker(),
        due,
        todo.summary,
        todo.percent_complete
    );
}

#[derive(Serialize)]
struct PorcelainTodo {
    id: i64,
    uid: String,
    summary: String,
    description: Option<String>,
    location: Option<String>,
    due: Option<String>,
    start: Option<String>,
    status: String,
    priority: Option<u8>,
    categories: Vec<String>,
    percent_complete: u8,
    list: String,
    path: String,
}

impl PorcelainTodo {
    fn from_parts(id: i64, todo: &Todo) -> Self {
        Self {
            id,
            uid: todo.uid.clone(),
            summary: todo.summary.clone(),
            description: todo.description.clone(),
            location: todo.location.clone(),
            due: todo.due.map(|value| value.to_rfc3339()),
            start: todo.start.map(|value| value.to_rfc3339()),
            status: todo.status.as_str().to_string(),
            priority: todo.priority,
            categories: todo.categories.clone(),
            percent_complete: todo.percent_complete,
            list: todo.list_name.clone(),
            path: todo.path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, command_from_default};
    use crate::config::Config;
    use std::path::PathBuf;

    fn config_with_default(default_command: &str) -> Config {
        Config {
            path_glob: "~/.local/share/calendars/*".to_string(),
            cache_path: PathBuf::from("/tmp/cache.sqlite3"),
            default_list: None,
            default_due_hours: 24,
            date_format: "%Y-%m-%d".to_string(),
            time_format: "%H:%M".to_string(),
            dt_separator: " ".to_string(),
            default_command: default_command.to_string(),
            color: "auto".to_string(),
            humanize: false,
            startable: false,
        }
    }

    #[test]
    fn resolves_non_list_default_commands() {
        let lists = command_from_default(&config_with_default("lists"));
        assert!(matches!(lists, Command::Lists));

        let repl = command_from_default(&config_with_default("repl"));
        assert!(matches!(repl, Command::Repl));

        let flush = command_from_default(&config_with_default("flush"));
        assert!(matches!(flush, Command::Flush));
    }
}
