use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn write_config(body: &str) -> tempfile::TempDir {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("prepo.toml"), body).unwrap();
    directory
}

#[test]
fn check_reports_ready_and_a_stable_json_contract() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "marker"
type = "path"
name = "Marker"
path = "."
expected = "exists"
kind = "directory"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["run_kind"], "full");
    assert_eq!(value["readiness"], "Ready");
    assert_eq!(value["summary"]["applicable_count"], 1);
    assert_eq!(value["summary"]["evaluated_required_count"], 1);
    assert!(value["results"][0].get("selection").is_none());
}

#[test]
fn check_distinguishes_selection_errors_from_cli_errors() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "marker"
type = "path"
name = "Marker"
group = "base"
path = "."
expected = "exists"
kind = "directory"
"#,
    );
    Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "missing"])
        .assert()
        .code(4);
    Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "marker", "marker"])
        .assert()
        .code(2);
    Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--group", "base", "marker"])
        .assert()
        .code(2);
}

#[test]
fn dependency_failure_blocks_the_required_dependent_check() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "missing"
type = "path"
name = "Missing"
path = "absent"
expected = "exists"
kind = "file"
[[checks]]
id = "dependent"
type = "path"
name = "Dependent"
path = "."
expected = "exists"
kind = "directory"
depends_on = ["missing"]
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "dependent", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["run_kind"], "partial");
    assert_eq!(value["results"][0]["selection"], "dependency");
    assert_eq!(value["results"][1]["outcome"], "blocked");
    assert_eq!(value["results"][1]["blocked_by"][0], "missing");
}

#[cfg(unix)]
#[test]
fn command_timeout_returns_a_check_failure_without_hanging() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "slow-command"
type = "command"
name = "Slow command"
command = ["sh", "-c", "sleep 30 & wait"]
timeout_seconds = 1
"#,
    );
    let started = std::time::Instant::now();
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-command", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "failed");
    assert_eq!(value["results"][0]["reason"], "timed_out");
}

#[cfg(unix)]
#[test]
fn independent_checks_run_in_parallel_unless_sequential_is_requested() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[runner]
max_concurrency = 2
[[checks]]
id = "first"
type = "command"
name = "First"
command = ["sh", "-c", "sleep 1"]
[[checks]]
id = "second"
type = "command"
name = "Second"
command = ["sh", "-c", "sleep 1"]
"#,
    );
    let started = std::time::Instant::now();
    Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-command"])
        .assert()
        .success();
    assert!(started.elapsed() < std::time::Duration::from_millis(1900));
}

#[cfg(unix)]
#[test]
fn sequential_flag_disables_parallel_execution() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "first"
type = "command"
name = "First"
command = ["sh", "-c", "sleep 1"]
[[checks]]
id = "second"
type = "command"
name = "Second"
command = ["sh", "-c", "sleep 1"]
"#,
    );
    let started = std::time::Instant::now();
    Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-command", "--sequential"])
        .assert()
        .success();
    assert!(started.elapsed() >= std::time::Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn interrupt_emits_no_partial_stdout_and_returns_130() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "slow"
type = "command"
name = "Slow"
command = ["sh", "-c", "sleep 30 & echo $! > child.pid; wait"]
"#,
    );
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_prepo"))
        .current_dir(directory.path())
        .args(["check", "--allow-command"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let child_pid_path = directory.path().join("child.pid");
    for _ in 0..20 {
        if child_pid_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Interrupted."));
    let child_pid = std::fs::read_to_string(child_pid_path).unwrap();
    let status = std::process::Command::new("kill")
        .args(["-0", child_pid.trim()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "the command descendant must be terminated"
    );
}

#[test]
fn active_check_without_permission_is_not_executed_and_is_not_ready() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "network"
type = "http"
name = "Network"
url = "http://127.0.0.1:9"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "permission_denied");
    assert_eq!(value["results"][0]["reason"], "permission_missing");
    assert_eq!(
        value["results"][0]["missing_permissions"][0],
        "--allow-network"
    );
}
