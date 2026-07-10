use crate::config::{
    Check, CheckSpec, EnvCondition, HttpMethod, PathExpected, PathKind, Platform, PortExpected,
    ProcessExpected, ValidatedConfig,
};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::time::{Duration, Instant};
use sysinfo::System;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    Error,
    PermissionDenied,
    NotApplicable,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection {
    Explicit,
    Dependency,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResultRow {
    pub id: String,
    pub name: String,
    pub group: Option<String>,
    pub required: bool,
    pub outcome: Outcome,
    pub reason: Option<&'static str>,
    pub summary: String,
    pub help: Option<String>,
    pub duration_ms: u128,
    pub blocked_by: Vec<String>,
    pub missing_permissions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
}

pub struct Run {
    pub project_name: String,
    pub full: bool,
    pub readiness: Option<Readiness>,
    pub results: Vec<ResultRow>,
    pub total_duration_ms: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Readiness {
    Ready,
    NotReady,
    NotEvaluated,
    Passed,
    Failed,
}

#[derive(Default, Clone, Copy)]
pub struct Permissions {
    pub command: bool,
    pub network: bool,
    pub env: bool,
}

pub enum SelectionError {
    UnknownId(String),
    EmptyGroup(String),
}

pub async fn run(
    config: &ValidatedConfig,
    ids: &[String],
    group: Option<&str>,
    permissions: Permissions,
) -> Result<Run, SelectionError> {
    let _runner_settings = (
        config.runner.parallel,
        config.runner.max_concurrency,
        config.runner.default_timeout_seconds,
    );
    let full = ids.is_empty() && group.is_none();
    let explicit = select(config, ids, group)?;
    let selected = include_dependencies(config, &explicit);
    let started = Instant::now();
    let current_platform = Platform::current().expect("unsupported OS is checked by the CLI");
    let mut rows: Vec<Option<ResultRow>> = (0..config.checks.len()).map(|_| None).collect();
    let mut remaining: BTreeSet<usize> = selected.clone();
    while !remaining.is_empty() {
        let index = *remaining.iter().next().expect("not empty");
        let check = &config.checks[index];
        if check
            .depends_on
            .iter()
            .all(|dependency| rows[*dependency].is_some())
        {
            rows[index] = Some(
                run_one(
                    config,
                    index,
                    check,
                    &rows,
                    current_platform,
                    explicit.contains(&index),
                    full,
                    permissions,
                )
                .await,
            );
            remaining.remove(&index);
        } else {
            // Validated configurations are acyclic; this only guards against an internal invariant break.
            let candidate = remaining.iter().copied().find(|candidate| {
                config.checks[*candidate]
                    .depends_on
                    .iter()
                    .all(|dependency| rows[*dependency].is_some())
            });
            if let Some(candidate) = candidate {
                let check = &config.checks[candidate];
                rows[candidate] = Some(
                    run_one(
                        config,
                        candidate,
                        check,
                        &rows,
                        current_platform,
                        explicit.contains(&candidate),
                        full,
                        permissions,
                    )
                    .await,
                );
                remaining.remove(&candidate);
            } else {
                break;
            }
        }
    }
    let results = rows.into_iter().flatten().collect::<Vec<_>>();
    let readiness = derive_readiness(&results, full);
    Ok(Run {
        project_name: config.project_name.clone(),
        full,
        readiness,
        results,
        total_duration_ms: started.elapsed().as_millis(),
    })
}

fn select(
    config: &ValidatedConfig,
    ids: &[String],
    group: Option<&str>,
) -> Result<BTreeSet<usize>, SelectionError> {
    if let Some(group) = group {
        let selected: BTreeSet<_> = config
            .checks
            .iter()
            .enumerate()
            .filter_map(|(index, check)| (check.group.as_deref() == Some(group)).then_some(index))
            .collect();
        if selected.is_empty() {
            Err(SelectionError::EmptyGroup(group.to_owned()))
        } else {
            Ok(selected)
        }
    } else if ids.is_empty() {
        Ok((0..config.checks.len()).collect())
    } else {
        let mut selected = BTreeSet::new();
        for id in ids {
            match config.checks.iter().position(|check| check.id == *id) {
                Some(index) => {
                    selected.insert(index);
                }
                None => return Err(SelectionError::UnknownId(id.clone())),
            }
        }
        Ok(selected)
    }
}

fn include_dependencies(config: &ValidatedConfig, explicit: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut result = explicit.clone();
    let mut todo = explicit.iter().copied().collect::<Vec<_>>();
    while let Some(index) = todo.pop() {
        for &dependency in &config.checks[index].depends_on {
            if result.insert(dependency) {
                todo.push(dependency);
            }
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_one(
    config: &ValidatedConfig,
    index: usize,
    check: &Check,
    rows: &[Option<ResultRow>],
    platform: Platform,
    explicit: bool,
    full: bool,
    permissions: Permissions,
) -> ResultRow {
    let selection = (!full).then_some(if explicit {
        Selection::Explicit
    } else {
        Selection::Dependency
    });
    let base = |outcome, reason, summary, duration_ms, blocked_by, missing_permissions| ResultRow {
        id: check.id.clone(),
        name: check.name.clone(),
        group: check.group.clone(),
        required: check.required,
        outcome,
        reason,
        summary,
        help: check.help.clone(),
        duration_ms,
        blocked_by,
        missing_permissions,
        selection,
    };
    if !check.platforms.contains(&platform) {
        return base(
            Outcome::NotApplicable,
            Some("platform_not_applicable"),
            "Platform does not apply.".to_owned(),
            0,
            vec![],
            vec![],
        );
    }
    let blocked_by = check
        .depends_on
        .iter()
        .filter_map(|dependency| {
            rows[*dependency]
                .as_ref()
                .filter(|result| result.outcome != Outcome::Passed)
                .map(|_| config.checks[*dependency].id.clone())
        })
        .collect::<Vec<_>>();
    if !blocked_by.is_empty() {
        return base(
            Outcome::Blocked,
            Some("dependency_not_passed"),
            "A dependency did not pass.".to_owned(),
            0,
            blocked_by,
            vec![],
        );
    }
    let missing_permissions = permissions_for(&check.spec, permissions);
    if !missing_permissions.is_empty() {
        return base(
            Outcome::PermissionDenied,
            Some("permission_missing"),
            "Required permission was not granted.".to_owned(),
            0,
            vec![],
            missing_permissions,
        );
    }
    let started = Instant::now();
    let (outcome, reason, summary) = execute(config, index, check).await;
    base(
        outcome,
        reason,
        summary,
        started.elapsed().as_millis(),
        vec![],
        vec![],
    )
}

fn permissions_for(spec: &CheckSpec, permissions: Permissions) -> Vec<&'static str> {
    let mut missing = Vec::new();
    match spec {
        CheckSpec::Command { .. } if !permissions.command => missing.push("--allow-command"),
        CheckSpec::Port { .. } if !permissions.network => missing.push("--allow-network"),
        CheckSpec::Http { headers, .. } => {
            if !permissions.network {
                missing.push("--allow-network");
            }
            if headers.values().any(|value| value.contains("${")) && !permissions.env {
                missing.push("--allow-env");
            }
        }
        CheckSpec::Env { condition, .. }
            if matches!(
                condition,
                EnvCondition::Equals
                    | EnvCondition::NotEquals
                    | EnvCondition::Contains
                    | EnvCondition::Matches
            ) && !permissions.env =>
        {
            missing.push("--allow-env")
        }
        _ => {}
    }
    missing
}

async fn execute(
    config: &ValidatedConfig,
    _index: usize,
    check: &Check,
) -> (Outcome, Option<&'static str>, String) {
    match &check.spec {
        CheckSpec::Env {
            variable,
            condition,
            expected,
        } => execute_env(variable, *condition, expected.as_deref()),
        CheckSpec::Path {
            path,
            expected,
            kind,
        } => execute_path(config, path, *expected, *kind),
        CheckSpec::Process { process, expected } => execute_process(process, *expected),
        CheckSpec::Port {
            host,
            port,
            expected,
            timeout_seconds,
        } => execute_port(host, *port, *expected, *timeout_seconds).await,
        CheckSpec::Command {
            command,
            expected_exit_code,
            stdout_contains,
            stderr_not_contains,
            working_directory,
            env,
            timeout_seconds,
        } => {
            execute_command(
                config,
                command,
                *expected_exit_code,
                stdout_contains.as_deref(),
                stderr_not_contains.as_deref(),
                working_directory.as_deref(),
                env,
                *timeout_seconds,
            )
            .await
        }
        CheckSpec::Http {
            url,
            method,
            expected_status,
            body_contains,
            headers,
            timeout_seconds,
        } => {
            execute_http(
                url,
                *method,
                *expected_status,
                body_contains.as_deref(),
                headers,
                *timeout_seconds,
            )
            .await
        }
    }
}

fn pass() -> (Outcome, Option<&'static str>, String) {
    (Outcome::Passed, None, "Check passed.".to_owned())
}
fn fail(reason: &'static str) -> (Outcome, Option<&'static str>, String) {
    (Outcome::Failed, Some(reason), reason.replace('_', " "))
}
fn error(reason: &'static str) -> (Outcome, Option<&'static str>, String) {
    (Outcome::Error, Some(reason), reason.replace('_', " "))
}

fn execute_env(
    variable: &str,
    condition: EnvCondition,
    expected: Option<&str>,
) -> (Outcome, Option<&'static str>, String) {
    let value = std::env::var_os(variable);
    match condition {
        EnvCondition::Exists => {
            if value.is_some() {
                pass()
            } else {
                fail("environment_variable_missing")
            }
        }
        EnvCondition::NotExists => {
            if value.is_none() {
                pass()
            } else {
                fail("environment_variable_present")
            }
        }
        _ => match value {
            None => fail("environment_variable_missing"),
            Some(value) => match value.into_string() {
                Err(_) => error("non_unicode_environment_value"),
                Ok(value) => match condition {
                    EnvCondition::Equals => {
                        if Some(value.as_str()) == expected {
                            pass()
                        } else {
                            fail("environment_value_mismatch")
                        }
                    }
                    EnvCondition::NotEquals => {
                        if Some(value.as_str()) != expected {
                            pass()
                        } else {
                            fail("environment_value_forbidden")
                        }
                    }
                    EnvCondition::Contains => {
                        if value.contains(expected.unwrap_or_default()) {
                            pass()
                        } else {
                            fail("environment_substring_missing")
                        }
                    }
                    EnvCondition::Matches => match Regex::new(expected.unwrap_or_default()) {
                        Ok(regex) if regex.is_match(&value) => pass(),
                        Ok(_) => fail("environment_pattern_mismatch"),
                        Err(_) => error("environment_read_failed"),
                    },
                    _ => unreachable!(),
                },
            },
        },
    }
}

fn execute_path(
    config: &ValidatedConfig,
    path: &str,
    expected: PathExpected,
    kind: Option<PathKind>,
) -> (Outcome, Option<&'static str>, String) {
    let target = config
        .path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(path);
    match std::fs::symlink_metadata(&target) {
        Ok(_metadata) if matches!(expected, PathExpected::Missing) => fail("path_present"),
        Ok(metadata) if metadata.file_type().is_symlink() => match std::fs::metadata(&target) {
            Ok(metadata) => compare_path_kind(metadata, kind),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fail("broken_symlink"),
            Err(_) => error("path_metadata_failed"),
        },
        Ok(metadata) => compare_path_kind(metadata, kind),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if matches!(expected, PathExpected::Missing) {
                pass()
            } else {
                fail("path_missing")
            }
        }
        Err(_) => error("path_metadata_failed"),
    }
}

fn compare_path_kind(
    metadata: std::fs::Metadata,
    kind: Option<PathKind>,
) -> (Outcome, Option<&'static str>, String) {
    let matches = match kind {
        Some(PathKind::File) => metadata.is_file(),
        Some(PathKind::Directory) => metadata.is_dir(),
        Some(PathKind::Any) | None => true,
    };
    if matches {
        pass()
    } else {
        fail("path_kind_mismatch")
    }
}

fn execute_process(
    process: &str,
    expected: ProcessExpected,
) -> (Outcome, Option<&'static str>, String) {
    let mut system = System::new_all();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let found = system
        .processes()
        .values()
        .any(|candidate| candidate.name() == OsStr::new(process));
    match (expected, found) {
        (ProcessExpected::Running, true) | (ProcessExpected::Stopped, false) => pass(),
        (ProcessExpected::Running, false) => fail("process_not_running"),
        (ProcessExpected::Stopped, true) => fail("process_unexpectedly_running"),
    }
}

async fn execute_port(
    host: &str,
    port: u16,
    expected: PortExpected,
    timeout_seconds: Option<u64>,
) -> (Outcome, Option<&'static str>, String) {
    if matches!(expected, PortExpected::Available) {
        return match std::net::TcpListener::bind((host, port)) {
            Ok(listener) => {
                drop(listener);
                pass()
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => fail("port_unavailable"),
            Err(_) => error("port_bind_failed"),
        };
    }
    let address = format!("{host}:{port}");
    let addresses = match tokio::net::lookup_host(&address).await {
        Ok(addresses) => addresses.collect::<Vec<_>>(),
        Err(_) => {
            return if matches!(expected, PortExpected::Open) {
                error("port_name_resolution_failed")
            } else {
                error("connection_state_indeterminate")
            };
        }
    };
    let attempt = async {
        let mut last_error = None;
        for address in addresses {
            match tokio::net::TcpStream::connect(address).await {
                Ok(stream) => return Ok(stream),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| std::io::Error::other("no addresses")))
    };
    match tokio::time::timeout(Duration::from_secs(timeout_seconds.unwrap_or(1)), attempt).await {
        Err(_) => {
            if matches!(expected, PortExpected::Open) {
                fail("timed_out")
            } else {
                error("connection_state_indeterminate")
            }
        }
        Ok(Ok(_)) => {
            if matches!(expected, PortExpected::Open) {
                pass()
            } else {
                fail("port_not_closed")
            }
        }
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            if matches!(expected, PortExpected::Closed) {
                pass()
            } else {
                fail("port_not_open")
            }
        }
        Ok(Err(_)) => {
            if matches!(expected, PortExpected::Open) {
                error("port_connect_failed")
            } else {
                error("connection_state_indeterminate")
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_command(
    config: &ValidatedConfig,
    command: &[String],
    expected_exit_code: i32,
    stdout_contains: Option<&[u8]>,
    stderr_not_contains: Option<&[u8]>,
    working_directory: Option<&str>,
    env: &std::collections::BTreeMap<String, String>,
    timeout_seconds: u64,
) -> (Outcome, Option<&'static str>, String) {
    let mut child = tokio::process::Command::new(&command[0]);
    child
        .args(&command[1..])
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(
            config
                .path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(working_directory.unwrap_or(".")),
        )
        .envs(env);
    let child = match child.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return fail("command_not_found");
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return fail("command_not_executable");
        }
        Err(_) => return fail("working_directory_invalid"),
    };
    match tokio::time::timeout(
        Duration::from_secs(timeout_seconds),
        child.wait_with_output(),
    )
    .await
    {
        Err(_) => fail("timed_out"),
        Ok(Err(_)) => error("command_output_read_failed"),
        Ok(Ok(output)) => {
            if output.stdout.len() > 8 * 1024 * 1024 || output.stderr.len() > 8 * 1024 * 1024 {
                return error("observation_limit_exceeded");
            }
            if !output.status.success() && output.status.code().is_none() {
                return fail("command_terminated");
            }
            if output.status.code() != Some(expected_exit_code) {
                return fail("exit_code_mismatch");
            }
            if let Some(pattern) = stdout_contains {
                if !contains(&output.stdout, pattern) {
                    return fail("stdout_condition_mismatch");
                }
            }
            if let Some(pattern) = stderr_not_contains {
                if contains(&output.stderr, pattern) {
                    return fail("stderr_condition_mismatch");
                }
            }
            pass()
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

async fn execute_http(
    url: &str,
    method: HttpMethod,
    expected_status: u16,
    body_contains: Option<&[u8]>,
    headers: &std::collections::BTreeMap<String, String>,
    timeout_seconds: u64,
) -> (Outcome, Option<&'static str>, String) {
    let builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout_seconds));
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return error("http_client_build_failed"),
    };
    let mut request = match method {
        HttpMethod::Get => client.get(url),
        HttpMethod::Head => client.head(url),
    };
    let mut expanded_bytes = 0_usize;
    for (name, value) in headers {
        let expanded = match expand_header(value) {
            Ok(value) => value,
            Err(reason) => return reason,
        };
        expanded_bytes += expanded.len();
        if expanded_bytes > 64 * 1024 {
            return error("expanded_header_too_large");
        }
        request = request.header(name, expanded);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) if error.is_timeout() => return fail("timed_out"),
        Err(error) if error.is_connect() => return fail("http_connection_failed"),
        Err(_) => return error("http_runtime_failed"),
    };
    if response.status().as_u16() != expected_status {
        return fail("http_status_mismatch");
    }
    if let Some(value) = response.headers().get(reqwest::header::CONTENT_ENCODING) {
        let value = match value.to_str() {
            Ok(value) => value,
            Err(_) => return error("unsupported_content_encoding"),
        };
        if !matches!(value, "identity" | "gzip") {
            return error("unsupported_content_encoding");
        }
    }
    let Some(pattern) = body_contains else {
        return pass();
    };
    match response.bytes().await {
        Ok(body) if body.len() <= 1024 * 1024 => {
            if contains(&body, pattern) {
                pass()
            } else {
                fail("http_body_mismatch")
            }
        }
        Ok(_) => error("observation_limit_exceeded"),
        Err(_) => error("http_body_read_failed"),
    }
}

fn expand_header(value: &str) -> Result<String, (Outcome, Option<&'static str>, String)> {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err(error("invalid_expanded_header_value"));
        };
        let name = &after[..end];
        if name.is_empty()
            || !name.chars().enumerate().all(|(index, character)| {
                (index == 0 && (character.is_ascii_alphabetic() || character == '_'))
                    || (index > 0 && (character.is_ascii_alphanumeric() || character == '_'))
            })
        {
            return Err(error("invalid_expanded_header_value"));
        }
        let variable = std::env::var_os(name)
            .ok_or_else(|| fail("missing_environment_variable"))?
            .into_string()
            .map_err(|_| error("non_unicode_environment_value"))?;
        output.push_str(&variable);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    if output.len() > 8192 || output.chars().any(|character| character.is_control()) {
        Err(error("expanded_header_too_large"))
    } else {
        Ok(output)
    }
}

fn derive_readiness(results: &[ResultRow], full: bool) -> Option<Readiness> {
    if full {
        let required = results
            .iter()
            .filter(|result| result.required && result.outcome != Outcome::NotApplicable)
            .collect::<Vec<_>>();
        Some(if required.is_empty() {
            Readiness::NotEvaluated
        } else if required
            .iter()
            .all(|result| result.outcome == Outcome::Passed)
        {
            Readiness::Ready
        } else {
            Readiness::NotReady
        })
    } else {
        let explicit = results
            .iter()
            .filter(|result| result.selection == Some(Selection::Explicit))
            .collect::<Vec<_>>();
        if explicit
            .iter()
            .all(|result| result.outcome == Outcome::NotApplicable)
        {
            Some(Readiness::NotEvaluated)
        } else if results
            .iter()
            .filter(|result| result.required && result.outcome != Outcome::NotApplicable)
            .all(|result| result.outcome == Outcome::Passed)
        {
            Some(Readiness::Passed)
        } else {
            Some(Readiness::Failed)
        }
    }
}

pub fn exit_code(run: &Run) -> i32 {
    match run.readiness {
        Some(Readiness::NotReady | Readiness::Failed) => 1,
        _ => 0,
    }
}
