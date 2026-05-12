use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn write_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempdir().expect("temp dir");
    let calendars = temp.path().join("calendars");
    let home = calendars.join("home");
    fs::create_dir_all(&home).expect("home dir");
    fs::write(home.join("color"), "#12ab34\n").expect("write color");

    let vtodo = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:Buy milk\r\nSTATUS:NEEDS-ACTION\r\nPRIORITY:1\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
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

#[test]
fn color_always_enables_ansi_output() {
    let (_temp, config) = write_fixture();
    let bin = env!("CARGO_BIN_EXE_todors");

    let output = run_with_config(bin, &config, &["--color", "always", "list"]);
    assert_success(&output, "todors --color always list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{1b}["));
}

#[test]
fn color_never_disables_ansi_output() {
    let (_temp, config) = write_fixture();
    let bin = env!("CARGO_BIN_EXE_todors");

    let output = run_with_config(bin, &config, &["--color", "never", "list"]);
    assert_success(&output, "todors --color never list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\u{1b}["));
}

#[test]
fn porcelain_stays_uncolored() {
    let (_temp, config) = write_fixture();
    let bin = env!("CARGO_BIN_EXE_todors");

    let output = run_with_config(bin, &config, &["--porcelain", "--color", "always", "list"]);
    assert_success(&output, "todors --porcelain --color always list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\u{1b}["));
}
