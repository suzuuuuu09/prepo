use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn validate_accepts_a_minimal_required_path_check() {
    let directory = tempdir().expect("temporary directory");
    std::fs::write(
        directory.path().join("prepo.toml"),
        r#"
version = 1

[project]
name = "example"

[[checks]]
id = "repository-marker"
type = "path"
name = "Repository marker"
path = ".git"
expected = "exists"
kind = "any"
"#,
    )
    .expect("configuration");

    Command::cargo_bin("prepo")
        .expect("prepo binary")
        .current_dir(directory.path())
        .arg("validate")
        .assert()
        .success()
        .stdout("Configuration is valid.\n")
        .stderr("");
}

#[test]
fn validate_rejects_required_dependencies_on_optional_checks() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "optional"
type = "path"
name = "Optional"
required = false
path = "."
expected = "exists"
kind = "directory"
[[checks]]
id = "required"
type = "path"
name = "Required"
path = "."
expected = "exists"
kind = "directory"
depends_on = ["optional"]
"#,
    )
    .expect("configuration");
    let output = Command::cargo_bin("prepo")
        .expect("prepo binary")
        .current_dir(directory.path())
        .arg("validate")
        .output()
        .expect("prepo output");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("depends on optional"));
}
