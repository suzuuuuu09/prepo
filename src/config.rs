use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const ALL_PLATFORMS: &[Platform] = &[Platform::Linux, Platform::Macos, Platform::Windows];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl Platform {
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::Macos),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: i64,
    project: RawProject,
    #[serde(default)]
    runner: RawRunner,
    #[serde(default)]
    checks: Vec<RawCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRunner {
    #[serde(default = "default_parallel")]
    parallel: bool,
    #[serde(default = "default_concurrency")]
    max_concurrency: u8,
    #[serde(default = "default_timeout")]
    default_timeout_seconds: u64,
}

impl Default for RawRunner {
    fn default() -> Self {
        Self {
            parallel: default_parallel(),
            max_concurrency: default_concurrency(),
            default_timeout_seconds: default_timeout(),
        }
    }
}

fn default_parallel() -> bool {
    true
}
fn default_concurrency() -> u8 {
    4
}
fn default_timeout() -> u64 {
    10
}
fn default_required() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    #[serde(default = "default_required")]
    required: bool,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    platforms: Vec<Platform>,
    group: Option<String>,
    help: Option<String>,
    variable: Option<String>,
    condition: Option<String>,
    path: Option<String>,
    #[serde(rename = "kind")]
    path_kind: Option<String>,
    command: Option<Vec<String>>,
    expected_exit_code: Option<i32>,
    stdout_contains: Option<String>,
    stderr_not_contains: Option<String>,
    working_directory: Option<String>,
    env: Option<BTreeMap<String, String>>,
    process: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    url: Option<String>,
    method: Option<String>,
    expected_status: Option<u16>,
    body_contains: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    expected: Option<String>,
    timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct ValidatedConfig {
    pub path: PathBuf,
    pub project_name: String,
    pub runner: Runner,
    pub checks: Vec<Check>,
}

#[derive(Clone, Debug)]
pub struct Runner {
    pub parallel: bool,
    pub max_concurrency: usize,
    pub default_timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: String,
    pub name: String,
    pub required: bool,
    pub depends_on: Vec<usize>,
    pub platforms: BTreeSet<Platform>,
    pub group: Option<String>,
    pub help: Option<String>,
    pub spec: CheckSpec,
}

#[derive(Clone, Debug)]
pub enum CheckSpec {
    Env {
        variable: String,
        condition: EnvCondition,
        expected: Option<String>,
    },
    Path {
        path: String,
        expected: PathExpected,
        kind: Option<PathKind>,
    },
    Command {
        command: Vec<String>,
        expected_exit_code: i32,
        stdout_contains: Option<Vec<u8>>,
        stderr_not_contains: Option<Vec<u8>>,
        working_directory: Option<String>,
        env: BTreeMap<String, String>,
        timeout_seconds: u64,
    },
    Process {
        process: String,
        expected: ProcessExpected,
    },
    Port {
        host: String,
        port: u16,
        expected: PortExpected,
        timeout_seconds: Option<u64>,
    },
    Http {
        url: String,
        method: HttpMethod,
        expected_status: u16,
        body_contains: Option<Vec<u8>>,
        headers: BTreeMap<String, String>,
        timeout_seconds: u64,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum EnvCondition {
    Exists,
    NotExists,
    Equals,
    NotEquals,
    Contains,
    Matches,
}
#[derive(Debug, Clone, Copy)]
pub enum PathExpected {
    Exists,
    Missing,
}
#[derive(Debug, Clone, Copy)]
pub enum PathKind {
    File,
    Directory,
    Any,
}
#[derive(Debug, Clone, Copy)]
pub enum ProcessExpected {
    Running,
    Stopped,
}
#[derive(Debug, Clone, Copy)]
pub enum PortExpected {
    Open,
    Closed,
    Available,
}
#[derive(Debug, Clone, Copy)]
pub enum HttpMethod {
    Get,
    Head,
}

pub fn load_and_validate(explicit_path: Option<&Path>) -> Result<ValidatedConfig, String> {
    let path = match explicit_path {
        Some(path) => path.to_path_buf(),
        None => find_config(
            std::env::current_dir().map_err(|_| "cannot determine current directory".to_owned())?,
        )?,
    };
    let metadata = std::fs::metadata(&path).map_err(|_| "cannot read prepo.toml".to_owned())?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("prepo.toml exceeds the 1 MiB size limit".to_owned());
    }
    let file = std::fs::File::open(&path).map_err(|_| "cannot read prepo.toml".to_owned())?;
    let mut bytes = Vec::with_capacity((MAX_CONFIG_BYTES + 1) as usize);
    use std::io::Read;
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read prepo.toml".to_owned())?;
    if bytes.len() > MAX_CONFIG_BYTES as usize {
        return Err("prepo.toml exceeds the 1 MiB size limit".to_owned());
    }
    let source = String::from_utf8(bytes).map_err(|_| "prepo.toml is not UTF-8".to_owned())?;
    let raw: RawConfig = toml::from_str(&source)
        .map_err(|_| "prepo.toml has invalid TOML syntax or types".to_owned())?;
    validate(raw, path)
}

fn find_config(mut directory: PathBuf) -> Result<PathBuf, String> {
    loop {
        let candidate = directory.join("prepo.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if directory.join(".git").exists() || !directory.pop() {
            return Err("prepo.toml was not found".to_owned());
        }
    }
}

fn validate(raw: RawConfig, path: PathBuf) -> Result<ValidatedConfig, String> {
    let mut errors = Vec::new();
    if raw.version != 1 {
        errors.push("version must be 1".to_owned());
    }
    validate_text("project.name", &raw.project.name, 256, &mut errors);
    if raw.checks.is_empty() {
        errors.push("at least one check is required".to_owned());
    }
    if !raw.checks.iter().any(|check| check.required) {
        errors.push("at least one required check is required".to_owned());
    }
    if !(1..=64).contains(&raw.runner.max_concurrency) {
        errors.push("runner.max_concurrency must be between 1 and 64".to_owned());
    }
    if !(1..=3600).contains(&raw.runner.default_timeout_seconds) {
        errors.push("runner.default_timeout_seconds must be between 1 and 3600".to_owned());
    }
    if raw.checks.len() > 1024 {
        errors.push("checks may contain at most 1024 entries".to_owned());
    }

    let mut ids = HashMap::new();
    for (index, check) in raw.checks.iter().enumerate() {
        if !is_kebab_case(&check.id) || check.id.len() > 64 {
            errors.push(format!("check {} has an invalid id", index + 1));
        }
        if ids.insert(check.id.as_str(), index).is_some() {
            errors.push(format!("duplicate check id: {}", check.id));
        }
        validate_text("checks.name", &check.name, 256, &mut errors);
        if let Some(group) = &check.group {
            if !is_kebab_case(group) || group.len() > 64 {
                errors.push(format!("check {} has an invalid group", check.id));
            }
        }
        if let Some(help) = &check.help {
            validate_text("checks.help", help, 2048, &mut errors);
        }
        if check.platforms.len() > 3 || has_duplicates(&check.platforms) {
            errors.push(format!("check {} has invalid platforms", check.id));
        }
        if check.depends_on.len() > 128
            || has_duplicates(&check.depends_on)
            || check.depends_on.iter().any(|id| id == &check.id)
        {
            errors.push(format!("check {} has invalid dependencies", check.id));
        }
    }
    if !errors.is_empty() {
        return Err(errors.into_iter().take(50).collect::<Vec<_>>().join("\n"));
    }

    let mut checks = Vec::with_capacity(raw.checks.len());
    for check in &raw.checks {
        match validate_check(check, raw.runner.default_timeout_seconds) {
            Ok(spec) => checks.push(Check {
                id: check.id.clone(),
                name: check.name.clone(),
                required: check.required,
                depends_on: Vec::new(),
                platforms: normalise_platforms(&check.platforms),
                group: check.group.clone(),
                help: check.help.clone(),
                spec,
            }),
            Err(error) => errors.push(format!("check {}: {error}", check.id)),
        }
    }
    for (index, check) in raw.checks.iter().enumerate() {
        for dependency in &check.depends_on {
            match ids.get(dependency.as_str()) {
                Some(&dependency_index) => checks[index].depends_on.push(dependency_index),
                None => errors.push(format!(
                    "check {} depends on unknown check {}",
                    check.id, dependency
                )),
            }
        }
    }
    validate_graph(&checks, &mut errors);
    if !errors.is_empty() {
        return Err(errors.into_iter().take(50).collect::<Vec<_>>().join("\n"));
    }
    Ok(ValidatedConfig {
        path,
        project_name: raw.project.name,
        runner: Runner {
            parallel: raw.runner.parallel,
            max_concurrency: usize::from(raw.runner.max_concurrency),
            default_timeout_seconds: raw.runner.default_timeout_seconds,
        },
        checks,
    })
}

fn validate_graph(checks: &[Check], errors: &mut Vec<String>) {
    for (index, check) in checks.iter().enumerate() {
        for &dependency in &check.depends_on {
            if !check.platforms.is_subset(&checks[dependency].platforms) {
                errors.push(format!(
                    "check {} is applicable where dependency {} is not",
                    check.id, checks[dependency].id
                ));
            }
        }
        let mut visiting = BTreeSet::new();
        if reaches(index, index, checks, &mut visiting) {
            errors.push(format!("check {} has a dependency cycle", check.id));
        }
        if check.required {
            let mut path = vec![index];
            if let Some(path) = optional_dependency_path(index, checks, &mut path) {
                let labels = path
                    .iter()
                    .map(|index| checks[*index].id.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                let optional = checks[*path.last().expect("path is not empty")].id.as_str();
                errors.push(format!(
                    "required check {} depends on optional check {} via {}",
                    check.id, optional, labels
                ));
            }
        }
    }
}

fn optional_dependency_path(
    node: usize,
    checks: &[Check],
    path: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    for &dependency in &checks[node].depends_on {
        if path.contains(&dependency) {
            continue;
        }
        path.push(dependency);
        if !checks[dependency].required {
            return Some(path.clone());
        }
        if let Some(found) = optional_dependency_path(dependency, checks, path) {
            return Some(found);
        }
        path.pop();
    }
    None
}

fn reaches(root: usize, node: usize, checks: &[Check], visiting: &mut BTreeSet<usize>) -> bool {
    for &next in &checks[node].depends_on {
        if next == root || (visiting.insert(next) && reaches(root, next, checks, visiting)) {
            return true;
        }
    }
    false
}

fn validate_check(check: &RawCheck, default_timeout_seconds: u64) -> Result<CheckSpec, String> {
    let timeout = |value: Option<u64>| -> Result<u64, String> {
        let value = value.unwrap_or(default_timeout_seconds);
        if (1..=3600).contains(&value) {
            Ok(value)
        } else {
            Err("timeout_seconds must be between 1 and 3600".to_owned())
        }
    };
    match check.kind.as_str() {
        "env" => {
            reject_unrelated(check, &["variable", "condition", "expected"])?;
            let variable = required_string(check.variable.as_deref(), "variable")?;
            if !is_env_name(variable) {
                return Err("variable is invalid".to_owned());
            }
            let condition = match check.condition.as_deref().unwrap_or("exists") {
                "exists" => EnvCondition::Exists,
                "not_exists" => EnvCondition::NotExists,
                "equals" => EnvCondition::Equals,
                "not_equals" => EnvCondition::NotEquals,
                "contains" => EnvCondition::Contains,
                "matches" => EnvCondition::Matches,
                _ => return Err("condition is invalid".to_owned()),
            };
            let value_condition = matches!(
                condition,
                EnvCondition::Equals
                    | EnvCondition::NotEquals
                    | EnvCondition::Contains
                    | EnvCondition::Matches
            );
            if value_condition != check.expected.is_some() {
                return Err("expected is required only for value conditions".to_owned());
            }
            if let Some(expected) = &check.expected {
                validate_pattern(expected, matches!(condition, EnvCondition::Matches))?;
            }
            Ok(CheckSpec::Env {
                variable: variable.to_owned(),
                condition,
                expected: check.expected.clone(),
            })
        }
        "path" => {
            reject_unrelated(check, &["path", "expected", "kind"])?;
            let path = required_string(check.path.as_deref(), "path")?;
            validate_path_syntax(
                path,
                false,
                &normalise_platforms(&check.platforms)
                    .into_iter()
                    .collect::<Vec<_>>(),
            )?;
            let expected = match check.expected.as_deref().unwrap_or("exists") {
                "exists" => PathExpected::Exists,
                "missing" => PathExpected::Missing,
                _ => return Err("expected is invalid".to_owned()),
            };
            let kind = match check.path_kind.as_deref() {
                Some("file") => Some(PathKind::File),
                Some("directory") => Some(PathKind::Directory),
                Some("any") => Some(PathKind::Any),
                Some(_) => return Err("kind is invalid".to_owned()),
                None => None,
            };
            if matches!(expected, PathExpected::Exists) != kind.is_some() {
                return Err("kind is required for exists and forbidden for missing".to_owned());
            }
            Ok(CheckSpec::Path {
                path: path.to_owned(),
                expected,
                kind,
            })
        }
        "command" => {
            reject_unrelated(
                check,
                &[
                    "command",
                    "expected_exit_code",
                    "stdout_contains",
                    "stderr_not_contains",
                    "working_directory",
                    "env",
                    "timeout_seconds",
                ],
            )?;
            let command = check
                .command
                .clone()
                .filter(|parts| !parts.is_empty() && parts.len() <= 128)
                .ok_or("command must be a non-empty array of at most 128 arguments")?;
            if command.iter().any(|argument| {
                argument.is_empty() || argument.len() > 8192 || has_unsafe_text(argument)
            }) {
                return Err("command has an invalid argument".to_owned());
            }
            if let Some(directory) = &check.working_directory {
                validate_path_syntax(
                    directory,
                    true,
                    &normalise_platforms(&check.platforms)
                        .into_iter()
                        .collect::<Vec<_>>(),
                )?;
            }
            let env = check.env.clone().unwrap_or_default();
            if env.len() > 128
                || env.iter().any(|(key, value)| {
                    !is_env_name(key) || value.len() > 8192 || has_unsafe_text(value)
                })
            {
                return Err("env is invalid".to_owned());
            }
            let stdout_contains = optional_pattern(check.stdout_contains.as_deref())?;
            let stderr_not_contains = optional_pattern(check.stderr_not_contains.as_deref())?;
            Ok(CheckSpec::Command {
                command,
                expected_exit_code: check.expected_exit_code.unwrap_or(0),
                stdout_contains,
                stderr_not_contains,
                working_directory: check.working_directory.clone(),
                env,
                timeout_seconds: timeout(check.timeout_seconds)?,
            })
        }
        "process" => {
            reject_unrelated(check, &["process", "expected"])?;
            let process = required_string(check.process.as_deref(), "process")?;
            validate_text_value(process, 256)?;
            let expected = match check.expected.as_deref().unwrap_or("running") {
                "running" => ProcessExpected::Running,
                "stopped" => ProcessExpected::Stopped,
                _ => return Err("expected is invalid".to_owned()),
            };
            if check.timeout_seconds.is_some() {
                return Err("timeout_seconds is not allowed".to_owned());
            }
            Ok(CheckSpec::Process {
                process: process.to_owned(),
                expected,
            })
        }
        "port" => {
            reject_unrelated(check, &["host", "port", "expected", "timeout_seconds"])?;
            let host = required_string(check.host.as_deref(), "host")?;
            let port = check.port.ok_or("port is required")?;
            let expected = match check.expected.as_deref().unwrap_or("open") {
                "open" => PortExpected::Open,
                "closed" => PortExpected::Closed,
                "available" => PortExpected::Available,
                _ => return Err("expected is invalid".to_owned()),
            };
            if matches!(expected, PortExpected::Available)
                && !matches!(host, "127.0.0.1" | "::1" | "0.0.0.0" | "::")
            {
                return Err("available requires a local host".to_owned());
            }
            let timeout_seconds = if matches!(expected, PortExpected::Available) {
                if check.timeout_seconds.is_some() {
                    return Err("timeout_seconds is not allowed for available".to_owned());
                }
                None
            } else {
                Some(timeout(check.timeout_seconds)?)
            };
            Ok(CheckSpec::Port {
                host: host.to_owned(),
                port,
                expected,
                timeout_seconds,
            })
        }
        "http" => {
            reject_unrelated(
                check,
                &[
                    "url",
                    "method",
                    "expected_status",
                    "body_contains",
                    "headers",
                    "timeout_seconds",
                ],
            )?;
            let url = required_string(check.url.as_deref(), "url")?;
            validate_url(url)?;
            let method = match check.method.as_deref().unwrap_or("GET") {
                "GET" => HttpMethod::Get,
                "HEAD" => HttpMethod::Head,
                _ => return Err("method must be GET or HEAD".to_owned()),
            };
            let body_contains = optional_pattern(check.body_contains.as_deref())?;
            if matches!(method, HttpMethod::Head) && body_contains.is_some() {
                return Err("HEAD forbids body_contains".to_owned());
            }
            let headers = check.headers.clone().unwrap_or_default();
            validate_headers(&headers)?;
            Ok(CheckSpec::Http {
                url: url.to_owned(),
                method,
                expected_status: check.expected_status.unwrap_or(200),
                body_contains,
                headers,
                timeout_seconds: timeout(check.timeout_seconds)?,
            })
        }
        _ => Err("type is invalid".to_owned()),
    }
}

fn reject_unrelated(check: &RawCheck, allowed: &[&str]) -> Result<(), String> {
    let present = [
        ("variable", check.variable.is_some()),
        ("condition", check.condition.is_some()),
        ("path", check.path.is_some()),
        ("kind", check.path_kind.is_some()),
        ("command", check.command.is_some()),
        ("expected_exit_code", check.expected_exit_code.is_some()),
        ("stdout_contains", check.stdout_contains.is_some()),
        ("stderr_not_contains", check.stderr_not_contains.is_some()),
        ("working_directory", check.working_directory.is_some()),
        ("env", check.env.is_some()),
        ("process", check.process.is_some()),
        ("host", check.host.is_some()),
        ("port", check.port.is_some()),
        ("url", check.url.is_some()),
        ("method", check.method.is_some()),
        ("expected_status", check.expected_status.is_some()),
        ("body_contains", check.body_contains.is_some()),
        ("headers", check.headers.is_some()),
        ("expected", check.expected.is_some()),
        ("timeout_seconds", check.timeout_seconds.is_some()),
    ];
    for (field, is_present) in present {
        if is_present && !allowed.contains(&field) {
            return Err(format!("{field} is not allowed for this type"));
        }
    }
    Ok(())
}

fn required_string<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required"))
}
fn optional_pattern(value: Option<&str>) -> Result<Option<Vec<u8>>, String> {
    value
        .map(|value| {
            validate_pattern(value, false)?;
            Ok(value.as_bytes().to_vec())
        })
        .transpose()
}
fn validate_pattern(value: &str, regex: bool) -> Result<(), String> {
    validate_text_value(value, 4096)?;
    if regex {
        Regex::new(value).map_err(|_| "pattern is invalid".to_owned())?;
    }
    Ok(())
}
fn validate_text_value(value: &str, limit: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > limit || has_unsafe_text(value) {
        Err("value is invalid".to_owned())
    } else {
        Ok(())
    }
}
fn validate_text(field: &str, value: &str, limit: usize, errors: &mut Vec<String>) {
    if value.is_empty() || value.trim() != value || value.len() > limit || has_unsafe_text(value) {
        errors.push(format!("invalid {field}"));
    }
}
fn has_unsafe_text(value: &str) -> bool {
    value.chars().any(|character| {
        character.is_control()
            || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
}
fn is_kebab_case(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some('a'..='z'))
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !value.ends_with('-')
        && !value.contains("--")
}
fn is_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('A'..='Z' | 'a'..='z' | '_'))
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}
fn has_duplicates<T: Ord + Clone>(values: &[T]) -> bool {
    let mut set = BTreeSet::new();
    values.iter().any(|value| !set.insert(value.clone()))
}
fn normalise_platforms(platforms: &[Platform]) -> BTreeSet<Platform> {
    if platforms.is_empty() {
        ALL_PLATFORMS.iter().copied().collect()
    } else {
        platforms.iter().copied().collect()
    }
}

fn validate_path_syntax(
    value: &str,
    working_directory: bool,
    platforms: &[Platform],
) -> Result<(), String> {
    if value.is_empty() || value.len() > 8192 || has_unsafe_text(value) {
        return Err("path is invalid".to_owned());
    }
    let windows_only = platforms
        .iter()
        .all(|platform| *platform == Platform::Windows);
    let windows_absolute = value.starts_with("\\\\")
        || (value.len() >= 3
            && value.as_bytes()[1] == b':'
            && value.as_bytes()[2] == b'\\'
            && value.as_bytes()[0].is_ascii_alphabetic());
    let drive_relative = value.len() >= 2
        && value.as_bytes()[1] == b':'
        && value.as_bytes()[0].is_ascii_alphabetic()
        && !windows_absolute;
    if drive_relative
        || (value.starts_with('\\') && !value.starts_with("\\\\"))
        || (value.contains('\\') && !windows_only)
    {
        return Err("path syntax is invalid".to_owned());
    }
    if working_directory && (value.starts_with('/') || windows_absolute) {
        return Err("working_directory must be relative".to_owned());
    }
    if value.starts_with('/')
        && !platforms
            .iter()
            .all(|platform| matches!(platform, Platform::Linux | Platform::Macos))
    {
        return Err("POSIX absolute path requires linux or macos platforms".to_owned());
    }
    if windows_absolute && !windows_only {
        return Err("Windows absolute path requires windows platforms".to_owned());
    }
    Ok(())
}

fn validate_url(value: &str) -> Result<(), String> {
    if value.len() > 8192 || has_unsafe_text(value) || value.contains("${") {
        return Err("url is invalid".to_owned());
    }
    let url = reqwest::Url::parse(value).map_err(|_| "url is invalid".to_owned())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("url is invalid".to_owned());
    }
    Ok(())
}

fn validate_headers(headers: &BTreeMap<String, String>) -> Result<(), String> {
    if headers.len() > 64 {
        return Err("too many headers".to_owned());
    }
    let forbidden = [
        "host",
        "content-length",
        "transfer-encoding",
        "connection",
        "proxy-authorization",
        "accept-encoding",
    ];
    let mut names = BTreeSet::new();
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if name.is_empty()
            || name.len() > 8192
            || has_unsafe_text(name)
            || value.len() > 8192
            || has_unsafe_text(value)
            || forbidden.contains(&lower.as_str())
            || !names.insert(lower)
        {
            return Err("headers are invalid".to_owned());
        }
    }
    Ok(())
}
