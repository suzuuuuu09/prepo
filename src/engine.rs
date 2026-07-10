use crate::config::{Check, CheckSpec, EnvCondition, Platform, ValidatedConfig};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

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

pub(crate) struct ExecutionResult {
    pub(crate) outcome: Outcome,
    pub(crate) reason: Option<&'static str>,
    pub(crate) summary: String,
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

pub(crate) use crate::checks::{begin_terminate_active_commands, terminate_active_commands};

pub async fn run(
    config: &ValidatedConfig,
    ids: &[String],
    group: Option<&str>,
    permissions: Permissions,
    sequential: bool,
) -> Result<Run, SelectionError> {
    let full = ids.is_empty() && group.is_none();
    debug_assert!((1..=3600).contains(&config.runner.default_timeout_seconds));
    let explicit = select(config, ids, group)?;
    let selected = include_dependencies(config, &explicit);
    let started = Instant::now();
    let current_platform = Platform::current().expect("unsupported OS is checked by the CLI");
    let config = Arc::new(config.clone());
    let mut rows: Vec<Option<ResultRow>> = (0..config.checks.len()).map(|_| None).collect();
    let mut remaining: BTreeSet<usize> = selected.clone();
    let concurrency = if sequential || !config.runner.parallel {
        1
    } else {
        config.runner.max_concurrency
    };
    let mut tasks = tokio::task::JoinSet::new();
    while !remaining.is_empty() || !tasks.is_empty() {
        while tasks.len() < concurrency {
            let Some(index) = remaining.iter().copied().find(|candidate| {
                config.checks[*candidate]
                    .depends_on
                    .iter()
                    .all(|dependency| rows[*dependency].is_some())
            }) else {
                break;
            };
            let config = Arc::clone(&config);
            let row_snapshot = rows.clone();
            let is_explicit = explicit.contains(&index);
            tasks.spawn(async move {
                let check = &config.checks[index];
                let result = run_one(
                    &config,
                    check,
                    &row_snapshot,
                    current_platform,
                    is_explicit,
                    full,
                    permissions,
                )
                .await;
                (index, result)
            });
            remaining.remove(&index);
        }
        if let Some(joined) = tasks.join_next().await {
            let (index, result) = joined.expect("check task must not panic");
            rows[index] = Some(result);
        } else {
            break;
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
    let result = crate::checks::execute(config, check).await;
    base(
        result.outcome,
        result.reason,
        result.summary,
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
