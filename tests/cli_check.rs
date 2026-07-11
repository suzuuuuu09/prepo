use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
fn process_is_absent_or_zombie(pid: &str) -> bool {
    let status = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    if !status.status.success() {
        return true;
    }
    String::from_utf8_lossy(&status.stdout)
        .trim_start()
        .starts_with('Z')
}

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

#[cfg(windows)]
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
command = ["cmd", "/C", "ping 127.0.0.1 -n 30 > NUL"]
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
    assert!(started.elapsed() < std::time::Duration::from_secs(7));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "failed");
    assert_eq!(value["results"][0]["reason"], "timed_out");
}

#[cfg(unix)]
#[test]
fn command_output_limit_is_a_check_error_without_retaining_output() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "large-output"
type = "command"
name = "Large output"
command = ["sh", "-c", "dd if=/dev/zero bs=1048576 count=9 2>/dev/null; sleep 30"]
timeout_seconds = 5
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
    assert_eq!(value["results"][0]["outcome"], "error");
    assert_eq!(value["results"][0]["reason"], "observation_limit_exceeded");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("\0"));
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
    for _ in 0..20 {
        if process_is_absent_or_zombie(child_pid.trim()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        process_is_absent_or_zombie(child_pid.trim()),
        "the command descendant must be terminated"
    );
}

#[cfg(unix)]
#[test]
fn second_interrupt_does_not_wait_for_command_cleanup() {
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
    let started = std::time::Instant::now();
    for _ in 0..2 {
        std::process::Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
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

#[test]
fn available_port_accepts_a_loopback_ipv6_host() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "available-ipv6"
type = "port"
name = "Available IPv6 port"
host = "::1"
port = 0
expected = "available"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-network", "--format", "json"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(value["results"][0]["reason"], "port_name_resolution_failed");
}

#[test]
fn http_dns_failure_has_a_dedicated_reason() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "unresolvable"
type = "http"
name = "Unresolvable host"
url = "http://prepo-invalid-host.invalid/"
timeout_seconds = 3
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-network", "--format", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "failed");
    assert_eq!(value["results"][0]["reason"], "dns_resolution_failed");
}

#[test]
fn http_header_expansion_rejects_control_characters_without_sending_a_request() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "header"
type = "http"
name = "Header"
url = "http://127.0.0.1:9/"
[checks.headers]
Authorization = "Bearer ${PREPO_TEST_HEADER_VALUE}"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .env("PREPO_TEST_HEADER_VALUE", "bad\nvalue")
        .args([
            "check",
            "--allow-network",
            "--allow-env",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "error");
    assert_eq!(
        value["results"][0]["reason"],
        "invalid_expanded_header_value"
    );
}

#[test]
fn environment_value_check_requires_permission_and_passes_when_granted() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "configured-environment"
type = "env"
name = "Configured environment"
variable = "PREPO_TEST_ENVIRONMENT"
condition = "equals"
expected = "ready"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .env("PREPO_TEST_ENVIRONMENT", "ready")
        .args(["check", "--allow-env", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "passed");
    assert_eq!(value["results"][0]["reason"], Value::Null);
}

#[test]
fn stopped_process_check_passes_without_process_details() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "absent-process"
type = "process"
name = "Absent process"
process = "prepo-this-process-does-not-exist"
expected = "stopped"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "passed");
}

#[test]
fn open_port_check_uses_a_local_tcp_listener() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let directory = write_config(&format!(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "listener"
type = "port"
name = "Listener"
host = "127.0.0.1"
port = {port}
expected = "open"
"#
    ));
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-network", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "passed");
    drop(listener);
}

#[test]
fn http_check_matches_a_local_response_body() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        use std::io::{Read, Write};
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let directory = write_config(&format!(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "health"
type = "http"
name = "Health"
url = "http://127.0.0.1:{port}/health"
expected_status = 200
body_contains = "ok"
"#
    ));
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-network", "--format", "json"])
        .output()
        .unwrap();

    server.join().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "passed");
}

#[test]
fn http_invalid_gzip_body_reports_a_decode_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        use std::io::{Read, Write};
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 7\r\nConnection: close\r\n\r\ninvalid",
            )
            .unwrap();
    });
    let directory = write_config(&format!(
        r#"version = 1
[project]
name = "example"
[[checks]]
id = "gzip"
type = "http"
name = "Gzip"
url = "http://127.0.0.1:{port}/"
body_contains = "ok"
"#
    ));
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--allow-network", "--format", "json"])
        .output()
        .unwrap();

    server.join().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"][0]["outcome"], "error");
    assert_eq!(value["results"][0]["reason"], "body_decode_failed");
}

#[test]
fn markdown_output_escapes_configured_table_content() {
    let directory = write_config(
        r#"version = 1
[project]
name = "example | project"
[[checks]]
id = "marker"
type = "path"
name = "Marker | *safe*"
path = "."
expected = "exists"
kind = "directory"
"#,
    );
    let output = Command::cargo_bin("prepo")
        .unwrap()
        .current_dir(directory.path())
        .args(["check", "--format", "markdown"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let markdown = String::from_utf8(output.stdout).unwrap();
    assert!(markdown.contains("example \\| project"));
    assert!(markdown.contains("Marker \\| \\*safe\\*"));
}
