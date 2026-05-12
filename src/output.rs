use crate::model::Todo;

pub fn print_detailed(todo: &Todo, date_format: &str, time_format: &str, dt_separator: &str) {
    println!("summary: {}", todo.summary);
    println!("uid: {}", todo.uid);
    println!("status: {}", todo.status.as_ical());
    println!("list: {}", todo.list_name);
    if let Some(due) = todo.due {
        println!("due: {}{}{}", due.format(date_format), dt_separator, due.format(time_format));
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
