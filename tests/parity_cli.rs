use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

fn write_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempdir().expect("temp dir");
    let calendars = temp.path().join("calendars");
    let home = calendars.join("home");
    let work = calendars.join("work");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&work).expect("work dir");

    let vevent = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nSUMMARY:Team Sync\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let vtodo = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:Buy milk\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    fs::write(home.join("event.ics"), vevent).expect("write event");
    fs::write(home.join("todo.ics"), vtodo).expect("write todo");

    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        format!(
            "path = \"{}/*\"\ncache_path = \"{}/cache.sqlite3\"\ndefault_command = \"list\"\n",
            calendars.display(),
            temp.path().display()
        ),
    )
    .expect("write config");

    (temp, config)
}

fn has_todoman() -> bool {
    let result = Command::new("todoman").arg("--help").output();
    let Ok(output) = result else {
        return false;
    };
    output.status.success()
}

fn run_with_config(bin: &str, config: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin).arg("--config").arg(config).args(args).output().expect("run command")
}

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_first_status_and_summary(json: &str) -> (Option<String>, Option<String>) {
    let value: Value = serde_json::from_str(json).expect("valid json");
    let Some(first) = value.as_array().and_then(|items| items.first()) else {
        return (None, None);
    };
    let status = first.get("status").and_then(|value| value.as_str()).map(str::to_string);
    let summary = first.get("summary").and_then(|value| value.as_str()).map(str::to_string);
    (status, summary)
}

fn write_todoman_config(temp: &tempfile::TempDir) -> std::path::PathBuf {
    let config = temp.path().join("todoman_config.py");
    let calendars = temp.path().join("calendars");
    fs::write(
        &config,
        format!(
            "path = \"{}/*\"\ncache_path = \"{}/todoman-cache.sqlite3\"\ndefault_command = \"list\"\n",
            calendars.display(),
            temp.path().display()
        ),
    )
    .expect("write todoman config");
    config
}

#[test]
fn list_only_shows_vtodo_entries() {
    let (_temp, config) = write_fixture();
    let bin = env!("CARGO_BIN_EXE_todors");

    let output = run_with_config(bin, &config, &["list"]);
    assert_success(&output, "todors list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Buy milk"));
    assert!(!stdout.contains("Team Sync"));
}

#[test]
fn todors_matches_todoman_for_basic_list_if_available() {
    let (temp, config) = write_fixture();
    let bin = env!("CARGO_BIN_EXE_todors");
    let todoman_config = write_todoman_config(&temp);

    if !has_todoman() {
        return;
    }

    let ours = run_with_config(bin, &config, &["list"]);
    assert_success(&ours, "todors list");
    let ours_out = String::from_utf8_lossy(&ours.stdout);

    let theirs = run_with_config("todoman", &todoman_config, &["list"]);
    assert_success(&theirs, "todoman list");
    let theirs_out = String::from_utf8_lossy(&theirs.stdout);

    assert_eq!(ours_out.lines().count(), theirs_out.lines().count());
}

#[test]
fn porcelain_list_parity_if_todoman_available() {
    let (temp, config) = write_fixture();
    if !has_todoman() {
        return;
    }

    let bin = env!("CARGO_BIN_EXE_todors");
    let todoman_config = write_todoman_config(&temp);

    let ours = run_with_config(bin, &config, &["--porcelain", "list"]);
    assert_success(&ours, "todors porcelain list");
    let ours_text = String::from_utf8_lossy(&ours.stdout);

    let theirs = run_with_config("todoman", &todoman_config, &["--porcelain", "list"]);
    assert_success(&theirs, "todoman porcelain list");
    let theirs_text = String::from_utf8_lossy(&theirs.stdout);

    let ours_pair = parse_first_status_and_summary(&ours_text);
    let theirs_pair = parse_first_status_and_summary(&theirs_text);
    assert_eq!(ours_pair, theirs_pair);
}

#[test]
fn done_and_undo_parity_if_todoman_available() {
    let (temp, config) = write_fixture();
    if !has_todoman() {
        return;
    }

    let bin = env!("CARGO_BIN_EXE_todors");
    let todoman_config = write_todoman_config(&temp);

    let done_ours = run_with_config(bin, &config, &["done", "1"]);
    assert_success(&done_ours, "todors done 1");
    let done_theirs = run_with_config("todoman", &todoman_config, &["done", "1"]);
    assert_success(&done_theirs, "todoman done 1");

    let ours_list = run_with_config(bin, &config, &["--porcelain", "list", "--status", "ANY"]);
    assert_success(&ours_list, "todors porcelain list --status ANY after done");
    let theirs_list =
        run_with_config("todoman", &todoman_config, &["--porcelain", "list", "--status", "ANY"]);
    assert_success(&theirs_list, "todoman porcelain list --status ANY after done");
    let ours_pair = parse_first_status_and_summary(&String::from_utf8_lossy(&ours_list.stdout));
    let theirs_pair = parse_first_status_and_summary(&String::from_utf8_lossy(&theirs_list.stdout));
    assert_eq!(ours_pair.0, theirs_pair.0);

    let undo_ours = run_with_config(bin, &config, &["undo", "1"]);
    assert_success(&undo_ours, "todors undo 1");
    let undo_theirs = run_with_config("todoman", &todoman_config, &["undo", "1"]);
    assert_success(&undo_theirs, "todoman undo 1");

    let ours_after_undo =
        run_with_config(bin, &config, &["--porcelain", "list", "--status", "ANY"]);
    assert_success(&ours_after_undo, "todors porcelain list --status ANY after undo");
    let theirs_after_undo =
        run_with_config("todoman", &todoman_config, &["--porcelain", "list", "--status", "ANY"]);
    assert_success(&theirs_after_undo, "todoman porcelain list --status ANY after undo");
    let ours_after_undo_pair =
        parse_first_status_and_summary(&String::from_utf8_lossy(&ours_after_undo.stdout));
    let theirs_after_undo_pair =
        parse_first_status_and_summary(&String::from_utf8_lossy(&theirs_after_undo.stdout));
    assert_eq!(ours_after_undo_pair.0, theirs_after_undo_pair.0);
}
