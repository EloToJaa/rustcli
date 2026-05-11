use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::{Duration, Local};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::Config;
use crate::model::{Status, Todo};
use crate::output;
use crate::store::AppStore;

#[derive(Debug, Parser)]
#[command(name = "todo")]
#[command(about = "Rust rewrite of todoman")]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    porcelain: bool,
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
    Cancel {
        ids: Vec<i64>,
    },
    Delete {
        ids: Vec<i64>,
    },
    Flush,
    Lists,
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
    #[arg(short, long)]
    list: Option<String>,
    #[arg(short, long)]
    all: bool,
    #[arg(long)]
    grep: Option<String>,
    #[arg(long)]
    priority: Option<u8>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    sort: Option<String>,
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
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

pub fn run(cli: Cli, config: &Config, app: &mut AppStore) -> Result<()> {
    let command = match cli.command {
        Some(cmd) => cmd,
        None => Command::List(ListArgs {
            list: None,
            all: false,
            grep: None,
            priority: None,
            status: None,
            sort: None,
        }),
    };

    match command {
        Command::List(args) => list(args, config, app, cli.porcelain),
        Command::New(args) => create(args, config, app),
        Command::Show { id } => show(id, config, app),
        Command::Edit(args) => edit(args, config, app),
        Command::Done { ids } => update_status(ids, Status::Completed, app),
        Command::Cancel { ids } => update_status(ids, Status::Cancelled, app),
        Command::Delete { ids } => delete(ids, app),
        Command::Flush => flush(app),
        Command::Lists => list_lists(app, cli.porcelain),
        Command::Path { id } => path(id, app),
        Command::Move { id, list } => move_todo(id, &list, app),
        Command::Copy { id, list } => copy_todo(id, &list, app),
    }
}

fn list(args: ListArgs, config: &Config, app: &mut AppStore, porcelain: bool) -> Result<()> {
    let mut todos = app.all_todos()?;
    if let Some(ref list_name) = args.list {
        todos.retain(|(_, todo)| todo.list_name.eq_ignore_ascii_case(list_name));
    }
    if !args.all {
        todos.retain(|(_, todo)| !matches!(todo.status, Status::Completed | Status::Cancelled));
    }
    if let Some(ref grep) = args.grep {
        let needle = grep.to_ascii_lowercase();
        todos.retain(|(_, todo)| {
            todo.summary.to_ascii_lowercase().contains(&needle)
                || todo
                    .description
                    .as_ref()
                    .map(|d| d.to_ascii_lowercase().contains(&needle))
                    .unwrap_or(false)
        });
    }
    if let Some(priority) = args.priority {
        todos.retain(|(_, todo)| {
            todo.priority.unwrap_or(0) > 0 && todo.priority.unwrap_or(0) <= priority
        });
    }
    if let Some(statuses) = args.status {
        let parsed = parse_status_filter(&statuses)?;
        todos.retain(|(_, todo)| parsed.contains(&todo.status));
    }

    sort_todos(&mut todos, args.sort.as_deref());

    if porcelain {
        let out: Vec<PorcelainTodo> = todos
            .iter()
            .map(|(id, todo)| PorcelainTodo::from_parts(*id, todo))
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let show_list = args.list.is_none() && app.lists().len() > 1;
    for (id, todo) in todos {
        print_compact_row(id, &todo, show_list, &config.date_format);
    }
    Ok(())
}

fn create(args: NewArgs, config: &Config, app: &mut AppStore) -> Result<()> {
    if args.summary.is_empty() {
        bail!("summary is required");
    }
    let list_name = if let Some(value) = args.list {
        value
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
    let due = if due_hours > 0 {
        Some(Local::now() + Duration::hours(due_hours))
    } else {
        None
    };

    let mut todo = Todo {
        uid: String::new(),
        summary: args.summary.join(" "),
        description: args.description,
        location: args.location,
        due,
        status: Status::NeedsAction,
        priority: args.priority,
        percent_complete: 0,
        list_name: list.name.clone(),
        path: PathBuf::new(),
        raw_other: Vec::new(),
    };
    let id = app.save_new(&list, &mut todo)?;
    println!("created {}", id);
    output::print_detailed(
        &todo,
        &config.date_format,
        &config.time_format,
        &config.dt_separator,
    );
    Ok(())
}

fn show(id: i64, config: &Config, app: &mut AppStore) -> Result<()> {
    let todo = app.todo_by_id(id)?;
    output::print_detailed(
        &todo,
        &config.date_format,
        &config.time_format,
        &config.dt_separator,
    );
    Ok(())
}

fn edit(args: EditArgs, config: &Config, app: &mut AppStore) -> Result<()> {
    let mut todo = app.todo_by_id(args.id)?;
    if let Some(summary) = args.summary {
        todo.summary = summary;
    }
    if let Some(description) = args.description {
        todo.description = Some(description);
    }
    if let Some(location) = args.location {
        todo.location = Some(location);
    }
    if let Some(priority) = args.priority {
        todo.priority = Some(priority);
    }
    if let Some(status) = args.status {
        let parsed = Status::parse_filter(&status)
            .ok_or_else(|| anyhow::anyhow!("invalid status: {}", status))?;
        todo.status = parsed;
        if parsed == Status::Completed {
            todo.percent_complete = 100;
        }
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

    app.save_existing(&todo)?;
    output::print_detailed(
        &todo,
        &config.date_format,
        &config.time_format,
        &config.dt_separator,
    );
    Ok(())
}

fn update_status(ids: Vec<i64>, status: Status, app: &mut AppStore) -> Result<()> {
    if ids.is_empty() {
        bail!("at least one id is required");
    }
    for id in ids {
        let mut todo = app.todo_by_id(id)?;
        todo.status = status;
        todo.percent_complete = if status == Status::Completed { 100 } else { 0 };
        app.save_existing(&todo)?;
        println!("updated {}", id);
    }
    Ok(())
}

fn delete(ids: Vec<i64>, app: &mut AppStore) -> Result<()> {
    if ids.is_empty() {
        bail!("at least one id is required");
    }
    for id in ids {
        app.delete_by_id(id)?;
        println!("deleted {}", id);
    }
    Ok(())
}

fn flush(app: &mut AppStore) -> Result<()> {
    let deleted = app.flush_done()?;
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

fn move_todo(id: i64, list_name: &str, app: &mut AppStore) -> Result<()> {
    let list = app
        .list_by_name(list_name)
        .ok_or_else(|| anyhow::anyhow!("unknown list: {}", list_name))?
        .clone();
    app.move_to_list(id, &list)?;
    println!("moved {} to {}", id, list.name);
    Ok(())
}

fn copy_todo(id: i64, list_name: &str, app: &mut AppStore) -> Result<()> {
    let list = app
        .list_by_name(list_name)
        .ok_or_else(|| anyhow::anyhow!("unknown list: {}", list_name))?
        .clone();
    let new_id = app.copy_to_list(id, &list)?;
    println!("copied {} to {} as {}", id, list.name, new_id);
    Ok(())
}

fn parse_status_filter(raw: &str) -> Result<Vec<Status>> {
    let mut statuses = Vec::new();
    for token in raw.split(',') {
        let token = token.trim();
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
    if statuses.is_empty() {
        return Ok(vec![Status::NeedsAction, Status::InProcess]);
    }
    Ok(statuses)
}

fn sort_todos(todos: &mut [(i64, Todo)], sort: Option<&str>) {
    let Some(sort) = sort else {
        return;
    };
    let keys: Vec<&str> = sort
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    todos.sort_by(|left, right| {
        for key in &keys {
            let ascending = key.starts_with('-');
            let field = key.trim_start_matches('-');
            let ord = match field {
                "id" => left.0.cmp(&right.0),
                "summary" => left.1.summary.cmp(&right.1.summary),
                "priority" => left
                    .1
                    .priority
                    .unwrap_or(255)
                    .cmp(&right.1.priority.unwrap_or(255)),
                "due" => left.1.due.cmp(&right.1.due),
                "status" => left.1.status.as_str().cmp(right.1.status.as_str()),
                _ => std::cmp::Ordering::Equal,
            };
            if ord != std::cmp::Ordering::Equal {
                return if ascending { ord } else { ord.reverse() };
            }
        }
        left.0.cmp(&right.0)
    });
}

fn print_compact_row(id: i64, todo: &Todo, show_list: bool, date_format: &str) {
    let due = todo
        .due
        .map(|d| d.format(date_format).to_string())
        .unwrap_or_default();
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
    status: String,
    priority: Option<u8>,
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
            due: todo.due.map(|d| d.to_rfc3339()),
            status: todo.status.as_str().to_string(),
            priority: todo.priority,
            percent_complete: todo.percent_complete,
            list: todo.list_name.clone(),
            path: todo.path.display().to_string(),
        }
    }
}
