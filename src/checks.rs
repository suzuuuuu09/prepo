use crate::config::{
    Check, CheckSpec, EnvCondition, HeaderInterpolationPart, HttpMethod, PathExpected, PathKind,
    PortExpected, ProcessExpected, ValidatedConfig, parse_header_interpolation,
};
use crate::engine::{ExecutionResult, Outcome, Reason};
use regex::Regex;
#[cfg(unix)]
use std::collections::BTreeSet;
use std::error::Error as _;
use std::ffi::OsStr;
use std::time::{Duration, Instant};
use sysinfo::System;

#[cfg(unix)]
static ACTIVE_COMMAND_GROUPS: std::sync::OnceLock<std::sync::Mutex<BTreeSet<i32>>> =
    std::sync::OnceLock::new();

#[cfg(unix)]
struct ActiveCommandGroup(Option<i32>);

#[cfg(unix)]
impl ActiveCommandGroup {
    fn register(process_id: Option<u32>) -> Self {
        let process_id = process_id.and_then(|id| i32::try_from(id).ok());
        if let Some(process_id) = process_id {
            ACTIVE_COMMAND_GROUPS
                .get_or_init(|| std::sync::Mutex::new(BTreeSet::new()))
                .lock()
                .expect("active command registry poisoned")
                .insert(process_id);
        }
        Self(process_id)
    }
}

#[cfg(unix)]
impl Drop for ActiveCommandGroup {
    fn drop(&mut self) {
        if let Some(process_id) = self.0 {
            if let Some(groups) = ACTIVE_COMMAND_GROUPS.get() {
                if let Ok(mut groups) = groups.lock() {
                    groups.remove(&process_id);
                }
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn begin_terminate_active_commands() -> Result<BTreeSet<i32>, ()> {
    let groups = active_command_groups();
    signal_process_groups(&groups)?;
    Ok(groups)
}

#[cfg(unix)]
pub(crate) async fn wait_for_active_commands(groups: BTreeSet<i32>) -> Result<(), ()> {
    wait_for_process_groups(groups).await
}

#[cfg(unix)]
fn active_command_groups() -> BTreeSet<i32> {
    ACTIVE_COMMAND_GROUPS
        .get()
        .and_then(|groups| groups.lock().ok().map(|groups| groups.clone()))
        .unwrap_or_default()
}

#[cfg(unix)]
fn signal_process_groups(groups: &BTreeSet<i32>) -> Result<(), ()> {
    let mut failed = false;
    for group in groups {
        match nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(*group),
            nix::sys::signal::Signal::SIGKILL,
        ) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(_) => failed = true,
        }
    }
    if failed { Err(()) } else { Ok(()) }
}

#[cfg(not(unix))]
pub(crate) fn begin_terminate_active_commands() -> Result<(), ()> {
    Ok(())
}

#[cfg(not(unix))]
pub(crate) async fn terminate_active_commands() -> Result<(), ()> {
    Ok(())
}

pub(crate) async fn execute(config: &ValidatedConfig, check: &Check) -> ExecutionResult {
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

fn pass() -> ExecutionResult {
    ExecutionResult {
        outcome: Outcome::Passed,
        reason: None,
        summary: "Check passed.".to_owned(),
    }
}
fn fail(reason: Reason) -> ExecutionResult {
    ExecutionResult {
        outcome: Outcome::Failed,
        reason: Some(reason),
        summary: reason.as_str().replace('_', " "),
    }
}
fn error(reason: Reason) -> ExecutionResult {
    ExecutionResult {
        outcome: Outcome::Error,
        reason: Some(reason),
        summary: reason.as_str().replace('_', " "),
    }
}

fn execute_env(variable: &str, condition: EnvCondition, expected: Option<&str>) -> ExecutionResult {
    let value = std::env::var_os(variable);
    match condition {
        EnvCondition::Exists => {
            if value.is_some() {
                pass()
            } else {
                fail(Reason::EnvironmentVariableMissing)
            }
        }
        EnvCondition::NotExists => {
            if value.is_none() {
                pass()
            } else {
                fail(Reason::EnvironmentVariablePresent)
            }
        }
        _ => match value {
            None => fail(Reason::EnvironmentVariableMissing),
            Some(value) => match value.into_string() {
                Err(_) => error(Reason::NonUnicodeEnvironmentValue),
                Ok(value) => match condition {
                    EnvCondition::Equals => {
                        if Some(value.as_str()) == expected {
                            pass()
                        } else {
                            fail(Reason::EnvironmentValueMismatch)
                        }
                    }
                    EnvCondition::NotEquals => {
                        if Some(value.as_str()) != expected {
                            pass()
                        } else {
                            fail(Reason::EnvironmentValueForbidden)
                        }
                    }
                    EnvCondition::Contains => {
                        if value.contains(expected.unwrap_or_default()) {
                            pass()
                        } else {
                            fail(Reason::EnvironmentSubstringMissing)
                        }
                    }
                    EnvCondition::Matches => match Regex::new(expected.unwrap_or_default()) {
                        Ok(regex) if regex.is_match(&value) => pass(),
                        Ok(_) => fail(Reason::EnvironmentPatternMismatch),
                        Err(_) => error(Reason::EnvironmentReadFailed),
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
        Ok(_metadata) if matches!(expected, PathExpected::Missing) => fail(Reason::PathPresent),
        Ok(metadata) if metadata.file_type().is_symlink() => match std::fs::metadata(&target) {
            Ok(metadata) => compare_path_kind(metadata, kind),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fail(Reason::BrokenSymlink)
            }
            Err(_) => error(Reason::PathMetadataFailed),
        },
        Ok(metadata) => compare_path_kind(metadata, kind),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if matches!(expected, PathExpected::Missing) {
                pass()
            } else {
                fail(Reason::PathMissing)
            }
        }
        Err(_) => error(Reason::PathMetadataFailed),
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
        fail(Reason::PathKindMismatch)
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
        (ProcessExpected::Running, false) => fail(Reason::ProcessNotRunning),
        (ProcessExpected::Stopped, true) => fail(Reason::ProcessUnexpectedlyRunning),
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
                fail(Reason::PortUnavailable)
            }
            Err(_) => error(Reason::PortBindFailed),
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
                error(Reason::PortNameResolutionFailed)
            } else {
                error(Reason::ConnectionStateIndeterminate)
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
                fail(Reason::TimedOut)
            } else {
                error(Reason::ConnectionStateIndeterminate)
            }
        }
        Ok(Ok(_)) => {
            if matches!(expected, PortExpected::Open) {
                pass()
            } else {
                fail(Reason::PortNotClosed)
            }
        }
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            if matches!(expected, PortExpected::Closed) {
                pass()
            } else {
                fail(Reason::PortNotOpen)
            }
        }
        Ok(Err(_)) => {
            if matches!(expected, PortExpected::Open) {
                error(Reason::PortConnectFailed)
            } else {
                error(Reason::ConnectionStateIndeterminate)
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
) -> ExecutionResult {
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
    if let Err(reason) = configure_command_containment(&mut child) {
        return error(reason);
    }
    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return fail(Reason::CommandNotFound);
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return fail(Reason::CommandNotExecutable);
        }
        Err(_) => return fail(Reason::WorkingDirectoryInvalid),
    };
    let containment = match establish_command_containment(&child) {
        Ok(containment) => containment,
        Err(reason) => return error(reason),
    };
    let process_id = child.id();
    #[cfg(unix)]
    let _active_group = ActiveCommandGroup::register(process_id);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut wait = Box::pin(child.wait());
    let mut observations = Box::pin(async {
        tokio::try_join!(
            observe_stream(stdout, stdout_contains),
            observe_stream(stderr, stderr_not_contains),
        )
    });
    enum CommandEvent {
        Exited(Result<std::process::ExitStatus, std::io::Error>),
        Observed(Result<(Observation, Observation), ObserveError>),
    }
    let event = tokio::time::timeout(Duration::from_secs(timeout_seconds), async {
        tokio::select! {
            status = &mut wait => CommandEvent::Exited(status),
            observation = &mut observations => CommandEvent::Observed(observation),
        }
    })
    .await;
    match event {
        Err(_) => match terminate_command_tree(process_id, &containment, wait).await {
            Ok(()) => fail(Reason::TimedOut),
            Err(()) => error(Reason::CommandCleanupFailed),
        },
        Ok(CommandEvent::Observed(Err(ObserveError::LimitExceeded))) => {
            match terminate_command_tree(process_id, &containment, wait).await {
                Ok(()) => error(Reason::ObservationLimitExceeded),
                Err(()) => error(Reason::CommandCleanupFailed),
            }
        }
        Ok(CommandEvent::Observed(Err(ObserveError::ReadFailed | ObserveError::DecodeFailed))) => {
            match terminate_command_tree(process_id, &containment, wait).await {
                Ok(()) => error(Reason::CommandOutputReadFailed),
                Err(()) => error(Reason::CommandCleanupFailed),
            }
        }
        Ok(CommandEvent::Exited(Err(_))) => error(Reason::CommandRuntimeFailed),
        Ok(CommandEvent::Exited(Ok(status))) => match observations.await {
            Err(ObserveError::LimitExceeded) => error(Reason::ObservationLimitExceeded),
            Err(ObserveError::ReadFailed | ObserveError::DecodeFailed) => {
                error(Reason::CommandOutputReadFailed)
            }
            Ok((stdout, stderr)) => evaluate_command(
                status,
                expected_exit_code,
                stdout_contains,
                stderr_not_contains,
                stdout,
                stderr,
            ),
        },
        Ok(CommandEvent::Observed(Ok((stdout, stderr)))) => match wait.await {
            Err(_) => error(Reason::CommandRuntimeFailed),
            Ok(status) => evaluate_command(
                status,
                expected_exit_code,
                stdout_contains,
                stderr_not_contains,
                stdout,
                stderr,
            ),
        },
    }
}

fn evaluate_command(
    status: std::process::ExitStatus,
    expected_exit_code: i32,
    stdout_contains: Option<&[u8]>,
    stderr_not_contains: Option<&[u8]>,
    stdout: Observation,
    stderr: Observation,
) -> ExecutionResult {
    if !status.success() && status.code().is_none() {
        return fail(Reason::CommandTerminated);
    }
    if status.code() != Some(expected_exit_code) {
        return fail(Reason::ExitCodeMismatch);
    }
    if stdout_contains.is_some() && !stdout.matched {
        return fail(Reason::StdoutConditionMismatch);
    }
    if stderr_not_contains.is_some() && stderr.matched {
        return fail(Reason::StderrConditionMismatch);
    }
    pass()
}

struct CommandContainment {
    #[cfg(windows)]
    job: JobHandle,
}

#[cfg(windows)]
const _: fn() = assert_send::<CommandContainment>;

#[cfg(windows)]
fn assert_send<T: Send>() {}

#[cfg(windows)]
struct JobHandle(usize);

#[cfg(windows)]
impl JobHandle {
    fn new(raw: windows_sys::Win32::Foundation::HANDLE) -> Self {
        Self(raw as usize)
    }

    fn as_raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 as windows_sys::Win32::Foundation::HANDLE
    }

    fn identifier(&self) -> usize {
        self.0
    }
}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.as_raw());
        }
    }
}

#[derive(Clone, Copy)]
struct Observation {
    matched: bool,
}

enum ObserveError {
    LimitExceeded,
    ReadFailed,
    DecodeFailed,
}

struct StreamingMatcher<'a> {
    limit: usize,
    total: usize,
    pattern: Option<&'a [u8]>,
    matched: bool,
    tail: Vec<u8>,
}

impl<'a> StreamingMatcher<'a> {
    fn new(limit: usize, pattern: Option<&'a [u8]>) -> Self {
        Self {
            limit,
            total: 0,
            pattern,
            matched: false,
            tail: Vec::new(),
        }
    }

    fn observe(&mut self, chunk: &[u8]) -> Result<(), ObserveError> {
        self.total += chunk.len();
        if self.total > self.limit {
            return Err(ObserveError::LimitExceeded);
        }
        let Some(pattern) = self.pattern else {
            return Ok(());
        };
        let mut candidate = Vec::with_capacity(self.tail.len() + chunk.len());
        candidate.extend_from_slice(&self.tail);
        candidate.extend_from_slice(chunk);
        self.matched |= contains(&candidate, pattern);
        let tail_length = pattern.len().saturating_sub(1).min(candidate.len());
        self.tail = candidate[candidate.len() - tail_length..].to_vec();
        Ok(())
    }
}

async fn observe_stream<R>(
    mut reader: Option<R>,
    pattern: Option<&[u8]>,
) -> Result<Observation, ObserveError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let Some(reader) = reader.as_mut() else {
        return Ok(Observation { matched: false });
    };
    let mut matcher = StreamingMatcher::new(8 * 1024 * 1024, pattern);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| ObserveError::ReadFailed)?;
        if count == 0 {
            return Ok(Observation {
                matched: matcher.matched,
            });
        }
        matcher.observe(&buffer[..count])?;
    }
}

#[cfg(unix)]
fn configure_command_containment(command: &mut tokio::process::Command) -> Result<(), Reason> {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
    Ok(())
}

#[cfg(unix)]
fn establish_command_containment(
    _child: &tokio::process::Child,
) -> Result<CommandContainment, Reason> {
    Ok(CommandContainment {})
}

#[cfg(windows)]
fn configure_command_containment(_command: &mut tokio::process::Command) -> Result<(), Reason> {
    // Windows containment is installed after spawning by the Job Object path.
    Ok(())
}

#[cfg(windows)]
fn establish_command_containment(
    child: &tokio::process::Child,
) -> Result<CommandContainment, Reason> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    let raw = child.raw_handle().ok_or(Reason::CommandContainmentFailed)? as HANDLE;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(Reason::CommandContainmentFailed);
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        ) != 0;
        let assigned = configured && AssignProcessToJobObject(job, raw) != 0;
        if !assigned {
            CloseHandle(job);
            return Err(Reason::CommandContainmentFailed);
        }
        Ok(CommandContainment {
            job: JobHandle::new(job),
        })
    }
}

#[cfg(not(any(unix, windows)))]
fn configure_command_containment(_command: &mut tokio::process::Command) -> Result<(), Reason> {
    Err(Reason::CommandContainmentFailed)
}

#[cfg(not(any(unix, windows)))]
fn establish_command_containment(
    _child: &tokio::process::Child,
) -> Result<CommandContainment, Reason> {
    Err(Reason::CommandContainmentFailed)
}

#[cfg(unix)]
async fn terminate_command_tree<F>(
    process_id: Option<u32>,
    _containment: &CommandContainment,
    wait: F,
) -> Result<(), ()>
where
    F: std::future::Future<Output = Result<std::process::ExitStatus, std::io::Error>> + Send,
{
    let process_id = process_id.ok_or(())?.try_into().map_err(|_| ())?;
    let groups = [process_id].into_iter().collect::<BTreeSet<_>>();
    signal_process_groups(&groups)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), wait)
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    wait_for_process_groups_until(groups, deadline).await
}

#[cfg(unix)]
async fn wait_for_process_groups(groups: BTreeSet<i32>) -> Result<(), ()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    wait_for_process_groups_until(groups, deadline).await
}

#[cfg(unix)]
async fn wait_for_process_groups_until(groups: BTreeSet<i32>, deadline: Instant) -> Result<(), ()> {
    loop {
        let running = groups.iter().any(|group| {
            nix::sys::signal::killpg(nix::unistd::Pid::from_raw(*group), None).is_ok()
        });
        if !running {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(windows)]
async fn terminate_command_tree<F>(
    _process_id: Option<u32>,
    containment: &CommandContainment,
    wait: F,
) -> Result<(), ()>
where
    F: std::future::Future<Output = Result<std::process::ExitStatus, std::io::Error>> + Send,
{
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;

    if unsafe { TerminateJobObject(containment.job.as_raw(), 1) } == 0 {
        return Err(());
    }
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .map_err(|_| ())?
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(not(any(unix, windows)))]
async fn terminate_command_tree<F>(
    _process_id: Option<u32>,
    _containment: &CommandContainment,
    _wait: F,
) -> Result<(), ()>
where
    F: std::future::Future<Output = Result<std::process::ExitStatus, std::io::Error>> + Send,
{
    Err(())
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
) -> ExecutionResult {
    let started = Instant::now();
    let parsed_url = match reqwest::Url::parse(url) {
        Ok(url) => url,
        Err(_) => return error(Reason::HttpRuntimeFailed),
    };
    let Some(host) = parsed_url.host_str() else {
        return error(Reason::HttpRuntimeFailed);
    };
    let Some(port) = parsed_url.port_or_known_default() else {
        return error(Reason::HttpRuntimeFailed);
    };
    let addresses = match tokio::time::timeout(
        Duration::from_secs(timeout_seconds),
        tokio::net::lookup_host((host, port)),
    )
    .await
    {
        Err(_) => return fail(Reason::TimedOut),
        Ok(Err(_)) => return fail(Reason::DnsResolutionFailed),
        Ok(Ok(addresses)) => addresses.collect::<Vec<_>>(),
    };
    let remaining = Duration::from_secs(timeout_seconds).saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return fail(Reason::TimedOut);
    }
    let builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(remaining)
        .resolve_to_addrs(host, &addresses);
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return error(Reason::HttpClientBuildFailed),
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
            return error(Reason::ExpandedHeaderTooLarge);
        }
        request = request.header(name, expanded);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) if error.is_timeout() => return fail(Reason::TimedOut),
        Err(error) if is_tls_error(&error) => return fail(Reason::TlsValidationFailed),
        Err(error) if error.is_connect() => return fail(Reason::HttpConnectionFailed),
        Err(_) => return error(Reason::HttpRuntimeFailed),
    };
    if response.status().as_u16() != expected_status {
        return fail(Reason::HttpStatusMismatch);
    }
    if let Some(value) = response.headers().get(reqwest::header::CONTENT_ENCODING) {
        let value = match value.to_str() {
            Ok(value) => value,
            Err(_) => return error(Reason::UnsupportedContentEncoding),
        };
        if !matches!(value, "identity" | "gzip") {
            return error(Reason::UnsupportedContentEncoding);
        }
    }
    let Some(pattern) = body_contains else {
        return pass();
    };
    match observe_http_body(response, pattern).await {
        Ok(true) => pass(),
        Ok(false) => fail(Reason::HttpBodyMismatch),
        Err(ObserveError::LimitExceeded) => error(Reason::ObservationLimitExceeded),
        Err(ObserveError::ReadFailed) => error(Reason::HttpBodyReadFailed),
        Err(ObserveError::DecodeFailed) => error(Reason::BodyDecodeFailed),
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
) -> Result<bool, ObserveError> {
    let mut matcher = StreamingMatcher::new(1024 * 1024, Some(pattern));
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_decode() {
            ObserveError::DecodeFailed
        } else {
            ObserveError::ReadFailed
        }
    })? {
        matcher.observe(&chunk)?;
    }
    Ok(matcher.matched)
}

fn expand_header(value: &str) -> Result<String, ExecutionResult> {
    let mut output = String::new();
    let parts = parse_header_interpolation(value)
        .map_err(|()| error(Reason::InvalidExpandedHeaderValue))?;
    for part in parts {
        match part {
            HeaderInterpolationPart::Literal(value) => output.push_str(value),
            HeaderInterpolationPart::Variable(name) => {
                let variable = std::env::var_os(name)
                    .ok_or_else(|| fail(Reason::MissingEnvironmentVariable))?
                    .into_string()
                    .map_err(|_| error(Reason::NonUnicodeEnvironmentValue))?;
                output.push_str(&variable);
            }
        }
    }
    if output.len() > 8192 {
        Err(error(Reason::ExpandedHeaderTooLarge))
    } else if output.chars().any(|character| character.is_control()) {
        Err(error(Reason::InvalidExpandedHeaderValue))
    } else {
        Ok(output)
    }
}
