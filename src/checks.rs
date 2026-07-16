use crate::config::{
    Check, CheckSpec, EnvCondition, HeaderInterpolationPart, HttpMethod, PathExpected, PathKind,
    PortExpected, ProcessExpected, ValidatedConfig, parse_header_interpolation,
};
use crate::result::{ErrorReason, ExecutionResult, FailureReason};
use regex::Regex;
use std::error::Error as _;
use std::ffi::OsStr;
use std::time::{Duration, Instant};
use sysinfo::System;

pub(crate) mod command;

pub(crate) async fn execute(
    command_runner: &command::CommandRunner,
    config: &ValidatedConfig,
    check: &Check,
) -> Result<ExecutionResult, command::RunCleaningUp> {
    match &check.spec {
        CheckSpec::Env {
            variable,
            condition,
            expected,
        } => Ok(execute_env(variable, *condition, expected.as_deref())),
        CheckSpec::Path {
            path,
            expected,
            kind,
        } => Ok(execute_path(config, path, *expected, *kind)),
        CheckSpec::Process { process, expected } => Ok(execute_process(process, *expected)),
        CheckSpec::Port {
            host,
            port,
            expected,
            timeout_seconds,
        } => Ok(execute_port(host, *port, *expected, *timeout_seconds).await),
        CheckSpec::Command(spec) => command_runner.evaluate(spec).await,
        CheckSpec::Http {
            url,
            method,
            expected_status,
            body_contains,
            headers,
            timeout_seconds,
        } => Ok(execute_http(
            url,
            *method,
            *expected_status,
            body_contains.as_deref(),
            headers,
            *timeout_seconds,
        )
        .await),
    }
}

fn pass() -> ExecutionResult {
    ExecutionResult::passed()
}
fn fail(reason: FailureReason) -> ExecutionResult {
    ExecutionResult::failed(reason)
}
fn error(reason: ErrorReason) -> ExecutionResult {
    ExecutionResult::error(reason)
}

fn execute_env(variable: &str, condition: EnvCondition, expected: Option<&str>) -> ExecutionResult {
    let value = std::env::var_os(variable);
    match condition {
        EnvCondition::Exists => {
            if value.is_some() {
                pass()
            } else {
                fail(FailureReason::EnvironmentVariableMissing)
            }
        }
        EnvCondition::NotExists => {
            if value.is_none() {
                pass()
            } else {
                fail(FailureReason::EnvironmentVariablePresent)
            }
        }
        _ => match value {
            None => fail(FailureReason::EnvironmentVariableMissing),
            Some(value) => match value.into_string() {
                Err(_) => error(ErrorReason::NonUnicodeEnvironmentValue),
                Ok(value) => match condition {
                    EnvCondition::Equals => {
                        if Some(value.as_str()) == expected {
                            pass()
                        } else {
                            fail(FailureReason::EnvironmentValueMismatch)
                        }
                    }
                    EnvCondition::NotEquals => {
                        if Some(value.as_str()) != expected {
                            pass()
                        } else {
                            fail(FailureReason::EnvironmentValueForbidden)
                        }
                    }
                    EnvCondition::Contains => {
                        if value.contains(expected.unwrap_or_default()) {
                            pass()
                        } else {
                            fail(FailureReason::EnvironmentSubstringMissing)
                        }
                    }
                    EnvCondition::Matches => match Regex::new(expected.unwrap_or_default()) {
                        Ok(regex) if regex.is_match(&value) => pass(),
                        Ok(_) => fail(FailureReason::EnvironmentPatternMismatch),
                        Err(_) => error(ErrorReason::EnvironmentReadFailed),
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
) -> ExecutionResult {
    let target = config
        .path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(path);
    match std::fs::symlink_metadata(&target) {
        Ok(_metadata) if matches!(expected, PathExpected::Missing) => {
            fail(FailureReason::PathPresent)
        }
        Ok(metadata) if metadata.file_type().is_symlink() => match std::fs::metadata(&target) {
            Ok(metadata) => compare_path_kind(metadata, kind),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fail(FailureReason::BrokenSymlink)
            }
            Err(_) => error(ErrorReason::PathMetadataFailed),
        },
        Ok(metadata) => compare_path_kind(metadata, kind),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if matches!(expected, PathExpected::Missing) {
                pass()
            } else {
                fail(FailureReason::PathMissing)
            }
        }
        Err(_) => error(ErrorReason::PathMetadataFailed),
    }
}

fn compare_path_kind(metadata: std::fs::Metadata, kind: Option<PathKind>) -> ExecutionResult {
    let matches = match kind {
        Some(PathKind::File) => metadata.is_file(),
        Some(PathKind::Directory) => metadata.is_dir(),
        Some(PathKind::Any) | None => true,
    };
    if matches {
        pass()
    } else {
        fail(FailureReason::PathKindMismatch)
    }
}

fn execute_process(process: &str, expected: ProcessExpected) -> ExecutionResult {
    let mut system = System::new_all();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let found = system
        .processes()
        .values()
        .any(|candidate| candidate.name() == OsStr::new(process));
    match (expected, found) {
        (ProcessExpected::Running, true) | (ProcessExpected::Stopped, false) => pass(),
        (ProcessExpected::Running, false) => fail(FailureReason::ProcessNotRunning),
        (ProcessExpected::Stopped, true) => fail(FailureReason::ProcessUnexpectedlyRunning),
    }
}

async fn execute_port(
    host: &str,
    port: u16,
    expected: PortExpected,
    timeout_seconds: Option<u64>,
) -> ExecutionResult {
    if matches!(expected, PortExpected::Available) {
        return match std::net::TcpListener::bind((host, port)) {
            Ok(listener) => {
                drop(listener);
                pass()
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                fail(FailureReason::PortUnavailable)
            }
            Err(_) => error(ErrorReason::PortBindFailed),
        };
    }
    let address = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let addresses = match tokio::net::lookup_host(&address).await {
        Ok(addresses) => addresses.collect::<Vec<_>>(),
        Err(_) => {
            return if matches!(expected, PortExpected::Open) {
                error(ErrorReason::PortNameResolutionFailed)
            } else {
                error(ErrorReason::ConnectionStateIndeterminate)
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
                fail(FailureReason::TimedOut)
            } else {
                error(ErrorReason::ConnectionStateIndeterminate)
            }
        }
        Ok(Ok(_)) => {
            if matches!(expected, PortExpected::Open) {
                pass()
            } else {
                fail(FailureReason::PortNotClosed)
            }
        }
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            if matches!(expected, PortExpected::Closed) {
                pass()
            } else {
                fail(FailureReason::PortNotOpen)
            }
        }
        Ok(Err(_)) => {
            if matches!(expected, PortExpected::Open) {
                error(ErrorReason::PortConnectFailed)
            } else {
                error(ErrorReason::ConnectionStateIndeterminate)
            }
        }
    }
}

async fn execute_http(
    url: &str,
    method: HttpMethod,
    expected_status: u16,
    body_contains: Option<&[u8]>,
    headers: &std::collections::BTreeMap<String, String>,
    timeout_seconds: u64,
) -> ExecutionResult {
    let started = Instant::now();
    let parsed_url = match reqwest::Url::parse(url) {
        Ok(url) => url,
        Err(_) => return error(ErrorReason::HttpRuntimeFailed),
    };
    let Some(host) = parsed_url.host_str() else {
        return error(ErrorReason::HttpRuntimeFailed);
    };
    let Some(port) = parsed_url.port_or_known_default() else {
        return error(ErrorReason::HttpRuntimeFailed);
    };
    let addresses = match tokio::time::timeout(
        Duration::from_secs(timeout_seconds),
        tokio::net::lookup_host((host, port)),
    )
    .await
    {
        Err(_) => return fail(FailureReason::TimedOut),
        Ok(Err(_)) => return fail(FailureReason::DnsResolutionFailed),
        Ok(Ok(addresses)) => addresses.collect::<Vec<_>>(),
    };
    let remaining = Duration::from_secs(timeout_seconds).saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return fail(FailureReason::TimedOut);
    }
    let builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(remaining)
        .resolve_to_addrs(host, &addresses);
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return error(ErrorReason::HttpClientBuildFailed),
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
            return error(ErrorReason::ExpandedHeaderTooLarge);
        }
        request = request.header(name, expanded);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) if error.is_timeout() => return fail(FailureReason::TimedOut),
        Err(error) if is_tls_error(&error) => return fail(FailureReason::TlsValidationFailed),
        Err(error) if error.is_connect() => return fail(FailureReason::HttpConnectionFailed),
        Err(_) => return error(ErrorReason::HttpRuntimeFailed),
    };
    if response.status().as_u16() != expected_status {
        return fail(FailureReason::HttpStatusMismatch);
    }
    if let Some(value) = response.headers().get(reqwest::header::CONTENT_ENCODING) {
        let value = match value.to_str() {
            Ok(value) => value,
            Err(_) => return error(ErrorReason::UnsupportedContentEncoding),
        };
        if !matches!(value, "identity" | "gzip") {
            return error(ErrorReason::UnsupportedContentEncoding);
        }
    }
    let Some(pattern) = body_contains else {
        return pass();
    };
    match observe_http_body(response, pattern).await {
        Ok(true) => pass(),
        Ok(false) => fail(FailureReason::HttpBodyMismatch),
        Err(HttpObserveError::LimitExceeded) => error(ErrorReason::ObservationLimitExceeded),
        Err(HttpObserveError::ReadFailed) => error(ErrorReason::HttpBodyReadFailed),
        Err(HttpObserveError::DecodeFailed) => error(ErrorReason::BodyDecodeFailed),
    }
}

fn is_tls_error(error: &reqwest::Error) -> bool {
    let mut source = error.source();
    while let Some(cause) = source {
        if cause.is::<rustls::Error>() {
            return true;
        }
        source = cause.source();
    }
    false
}

async fn observe_http_body(
    mut response: reqwest::Response,
    pattern: &[u8],
) -> Result<bool, HttpObserveError> {
    let mut matcher = HttpStreamingMatcher::new(1024 * 1024, Some(pattern));
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_decode() {
            HttpObserveError::DecodeFailed
        } else {
            HttpObserveError::ReadFailed
        }
    })? {
        matcher.observe(&chunk)?;
    }
    Ok(matcher.matched)
}

#[derive(Clone, Copy, Debug)]
enum HttpObserveError {
    LimitExceeded,
    ReadFailed,
    DecodeFailed,
}

struct HttpStreamingMatcher<'a> {
    limit: usize,
    total: usize,
    pattern: Option<&'a [u8]>,
    matched: bool,
    tail: Vec<u8>,
}

impl<'a> HttpStreamingMatcher<'a> {
    fn new(limit: usize, pattern: Option<&'a [u8]>) -> Self {
        Self {
            limit,
            total: 0,
            pattern,
            matched: false,
            tail: Vec::new(),
        }
    }

    fn observe(&mut self, chunk: &[u8]) -> Result<(), HttpObserveError> {
        self.total = self.total.saturating_add(chunk.len());
        if self.total > self.limit {
            return Err(HttpObserveError::LimitExceeded);
        }
        let Some(pattern) = self.pattern else {
            return Ok(());
        };
        let mut candidate = Vec::with_capacity(self.tail.len() + chunk.len());
        candidate.extend_from_slice(&self.tail);
        candidate.extend_from_slice(chunk);
        self.matched |= candidate
            .windows(pattern.len())
            .any(|window| window == pattern);
        let tail_length = pattern.len().saturating_sub(1).min(candidate.len());
        self.tail = candidate[candidate.len() - tail_length..].to_vec();
        Ok(())
    }
}

fn expand_header(value: &str) -> Result<String, ExecutionResult> {
    let mut output = String::new();
    let parts = parse_header_interpolation(value)
        .map_err(|()| error(ErrorReason::InvalidExpandedHeaderValue))?;
    for part in parts {
        match part {
            HeaderInterpolationPart::Literal(value) => output.push_str(value),
            HeaderInterpolationPart::Variable(name) => {
                let variable = std::env::var_os(name)
                    .ok_or_else(|| fail(FailureReason::MissingEnvironmentVariable))?
                    .into_string()
                    .map_err(|_| error(ErrorReason::NonUnicodeEnvironmentValue))?;
                output.push_str(&variable);
            }
        }
    }
    if output.len() > 8192 {
        Err(error(ErrorReason::ExpandedHeaderTooLarge))
    } else if output.chars().any(|character| character.is_control()) {
        Err(error(ErrorReason::InvalidExpandedHeaderValue))
    } else {
        Ok(output)
    }
}
