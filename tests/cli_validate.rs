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

#[test]
fn validate_reports_invalid_checks_without_panicking_while_resolving_dependencies() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "invalid-path"
type = "path"
name = "Invalid path"
path = "."
expected = "exists"
[[checks]]
id = "dependent"
type = "path"
name = "Dependent"
path = "."
expected = "exists"
kind = "directory"
depends_on = ["invalid-path"]
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid-path"));
}

#[test]
fn validate_rejects_an_explicitly_empty_platform_list() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "marker"
type = "path"
name = "Marker"
platforms = []
path = "."
expected = "exists"
kind = "directory"
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid platforms"));
}

#[test]
fn validate_rejects_malformed_http_header_interpolation() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "health"
type = "http"
name = "Health"
url = "https://example.test/health"
[checks.headers]
Authorization = "Bearer ${NOT-COMPLETE}"
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("headers are invalid"));
}

#[test]
fn validate_collects_independent_semantic_errors() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "invalid-platforms"
type = "path"
name = "Invalid platforms"
platforms = []
path = "."
expected = "exists"
kind = "directory"
[[checks]]
id = "invalid-header"
type = "http"
name = "Invalid header"
url = "https://example.test/"
[checks.headers]
Authorization = "Bearer ${NOT-COMPLETE}"
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid platforms"));
    assert!(stderr.contains("headers are invalid"));
}

#[test]
fn validate_collects_independent_errors_within_one_check() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "invalid-command"
type = "command"
name = "Invalid command"
command = ["echo"]
stdout_contains = ""
working_directory = "/absolute"
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    let pattern = stderr.find("value is invalid").expect("pattern error");
    let directory = stderr
        .find("working_directory must be relative")
        .expect("working-directory error");
    assert!(pattern < directory);
}

#[test]
fn validate_collects_independent_errors_for_every_check_type() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "invalid-env"
type = "env"
name = "Invalid env"
variable = "NOT-VALID"
condition = "unknown"
[[checks]]
id = "invalid-path"
type = "path"
name = "Invalid path"
path = "/absolute"
expected = "missing"
kind = "directory"
[[checks]]
id = "invalid-process"
type = "process"
name = "Invalid process"
process = ""
expected = "unknown"
timeout_seconds = 0
[[checks]]
id = "invalid-port"
type = "port"
name = "Invalid port"
host = "example.test"
port = 80
expected = "available"
timeout_seconds = 0
[[checks]]
id = "invalid-http"
type = "http"
name = "Invalid http"
url = "ftp://example.test/"
method = "HEAD"
body_contains = ""
timeout_seconds = 0
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "check invalid-env: variable is invalid",
        "check invalid-env: condition is invalid",
        "check invalid-path: POSIX absolute path requires linux or macos platforms",
        "check invalid-path: kind is required for exists and forbidden for missing",
        "check invalid-process: value is invalid",
        "check invalid-process: expected is invalid",
        "check invalid-process: timeout_seconds is not allowed",
        "check invalid-port: available requires a local host",
        "check invalid-port: timeout_seconds is not allowed for available",
        "check invalid-http: url is invalid",
        "check invalid-http: value is invalid",
        "check invalid-http: HEAD forbids body_contains",
        "check invalid-http: timeout_seconds must be between 1 and 3600",
    ] {
        assert!(
            stderr.contains(expected),
            "missing {expected:?} in {stderr:?}"
        );
    }
}

#[test]
fn validate_reports_semantic_errors_in_check_declaration_order() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "first"
type = "not-a-check"
name = "First"
[[checks]]
id = "second"
type = "path"
name = "Second"
platforms = []
path = "."
expected = "exists"
kind = "directory"
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first = stderr.find("check first: type is invalid").unwrap();
    let second = stderr.find("check second has invalid platforms").unwrap();
    assert!(first < second);
}

#[test]
fn validate_reports_dependency_errors_in_check_declaration_order() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "first"
type = "path"
name = "First"
path = "."
expected = "exists"
kind = "directory"
depends_on = ["missing"]
[[checks]]
id = "second"
type = "path"
name = "Second"
platforms = []
path = "."
expected = "exists"
kind = "directory"
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    let dependency = stderr
        .find("first depends on unknown check missing")
        .unwrap();
    let second = stderr.find("check second has invalid platforms").unwrap();
    assert!(
        dependency < second,
        "errors were reported out of order: {stderr}"
    );
}

#[test]
fn validate_reports_platform_errors_before_type_specific_fields() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "health"
type = "http"
name = "Health"
platforms = []
url = "https://example.test/"
[checks.headers]
Authorization = "Bearer ${NOT-COMPLETE}"
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    let platforms = stderr.find("invalid platforms").unwrap();
    let headers = stderr.find("headers are invalid").unwrap();
    assert!(platforms < headers);
}

#[test]
fn validate_collects_graph_errors_even_when_another_check_is_invalid() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("prepo.toml"),
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "invalid-path"
type = "path"
name = "Invalid path"
path = "."
expected = "exists"
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid-path"));
    assert!(stderr.contains("depends on optional"));
}
