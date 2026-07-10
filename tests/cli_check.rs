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
