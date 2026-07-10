use crate::config::Platform;
use crate::engine::{Outcome, Readiness, ResultRow, Run, Selection};
use serde::Serialize;
use std::io::{self, Write};

pub enum Format {
    Human,
    Json,
    Markdown,
}

pub fn write_run(run: &Run, format: Format, color: bool) -> io::Result<()> {
    let text = match format {
        Format::Human => human(run, color),
        Format::Json => json(run)?,
        Format::Markdown => markdown(run),
    };
    let mut stdout = io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.write_all(b"\n")
}

fn human(run: &Run, color: bool) -> String {
    let mut output = String::new();
    for row in &run.results {
        let (symbol, display) = display(row);
        let display = if color {
            colorize(display, row)
        } else {
            display.to_owned()
        };
        output.push_str(&format!(
            "{symbol} {display}: {} ({})",
            row.name,
            outcome_name(row.outcome)
        ));
        if let Some(reason) = row.reason {
            output.push_str(&format!(" [{reason}]"));
        }
        if !row.missing_permissions.is_empty() {
            output.push_str(&format!(" ({})", row.missing_permissions.join(", ")));
        }
        output.push('\n');
    }
    match run.readiness.expect("run has status") {
        Readiness::Ready => output.push_str("Readiness: Ready"),
        Readiness::NotReady => output.push_str("Readiness: Not Ready"),
        Readiness::NotEvaluated => {
            if run.full {
                output.push_str("Readiness: Not Evaluated")
            } else {
                output.push_str("Selected run: Not Evaluated")
            }
        }
        Readiness::Passed => output.push_str("Selected run: Passed"),
        Readiness::Failed => output.push_str("Selected run: Failed"),
    }
    output
}

#[derive(Serialize)]
struct JsonRun<'a> {
    output_schema_version: u8,
    config_schema_version: u8,
    prepo_version: &'static str,
    run_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    readiness: Option<&'static str>,
    project: Project<'a>,
    environment: Environment,
    total_duration_ms: u128,
    summary: Summary,
    results: &'a [ResultRow],
}
#[derive(Serialize)]
struct Project<'a> {
    name: &'a str,
}
#[derive(Serialize)]
struct Environment {
    os: &'static str,
    arch: &'static str,
}
#[derive(Serialize)]
struct Summary {
    total_count: usize,
    applicable_count: usize,
    evaluated_count: usize,
    evaluated_required_count: usize,
    passed_count: usize,
    failed_count: usize,
    error_count: usize,
    permission_denied_count: usize,
    not_applicable_count: usize,
    blocked_count: usize,
}

fn json(run: &Run) -> io::Result<String> {
    let summary = Summary {
        total_count: run.results.len(),
        applicable_count: run
            .results
            .iter()
            .filter(|result| result.outcome != Outcome::NotApplicable)
            .count(),
        evaluated_count: run
            .results
            .iter()
            .filter(|result| {
                matches!(
                    result.outcome,
                    Outcome::Passed | Outcome::Failed | Outcome::Error
                )
            })
            .count(),
        evaluated_required_count: run
            .results
            .iter()
            .filter(|result| {
                result.required
                    && matches!(
                        result.outcome,
                        Outcome::Passed | Outcome::Failed | Outcome::Error
                    )
            })
            .count(),
        passed_count: count(run, Outcome::Passed),
        failed_count: count(run, Outcome::Failed),
        error_count: count(run, Outcome::Error),
        permission_denied_count: count(run, Outcome::PermissionDenied),
        not_applicable_count: count(run, Outcome::NotApplicable),
        blocked_count: count(run, Outcome::Blocked),
    };
    serde_json::to_string_pretty(&JsonRun {
        output_schema_version: 1,
        config_schema_version: 1,
        prepo_version: env!("CARGO_PKG_VERSION"),
        run_kind: if run.full { "full" } else { "partial" },
        readiness: run
            .full
            .then(|| readiness_name(run.readiness.expect("status"))),
        project: Project {
            name: &run.project_name,
        },
        environment: Environment {
            os: Platform::current().expect("checked").as_str(),
            arch: std::env::consts::ARCH,
        },
        total_duration_ms: run.total_duration_ms,
        summary,
        results: &run.results,
    })
    .map_err(io::Error::other)
}
fn count(run: &Run, outcome: Outcome) -> usize {
    run.results
        .iter()
        .filter(|result| result.outcome == outcome)
        .count()
}

fn markdown(run: &Run) -> String {
    let mut output = String::from("# prepo check\n\n");
    if run.full {
        output.push_str(&format!(
            "Readiness: **{}**\n\n",
            readiness_name(run.readiness.expect("status"))
        ));
    } else {
        output.push_str(&format!(
            "Selected run: **{}**\n\n",
            readiness_name(run.readiness.expect("status"))
        ));
    }
    output.push_str(&format!("Project: {}  \nOS: {}  \nArchitecture: {}  \nprepo version: {}  \nTotal duration: {} ms\n\n", escape(&run.project_name), Platform::current().expect("checked").as_str(), std::env::consts::ARCH, env!("CARGO_PKG_VERSION"), run.total_duration_ms));
    output.push_str(if run.full { "| Display | Outcome | Check | Reason | Duration |\n| --- | --- | --- | --- | ---: |\n" } else { "| Display | Outcome | Check | Reason | Duration | Selection |\n| --- | --- | --- | --- | ---: | --- |\n" });
    for row in &run.results {
        let (_, display) = display(row);
        let reason = row.reason.unwrap_or("-");
        if run.full {
            output.push_str(&format!(
                "| {display} | {} | {} | {} | {} ms |\n",
                outcome_name(row.outcome),
                escape(&row.name),
                escape(reason),
                row.duration_ms
            ));
        } else {
            output.push_str(&format!(
                "| {display} | {} | {} | {} | {} ms | {} |\n",
                outcome_name(row.outcome),
                escape(&row.name),
                escape(reason),
                row.duration_ms,
                match row.selection.expect("partial selection") {
                    Selection::Explicit => "explicit",
                    Selection::Dependency => "dependency",
                }
            ));
        }
    }
    for row in &run.results {
        if row.help.is_some() || !row.blocked_by.is_empty() || !row.missing_permissions.is_empty() {
            output.push_str(&format!("\n## {}\n", escape(&row.name)));
            if let Some(help) = &row.help {
                output.push_str(&format!("{}\n", escape(help)));
            }
            if !row.blocked_by.is_empty() {
                output.push_str(&format!(
                    "Blocked by: {}\n",
                    row.blocked_by
                        .iter()
                        .map(|value| escape(value))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !row.missing_permissions.is_empty() {
                output.push_str(&format!(
                    "Missing permissions: {}\n",
                    row.missing_permissions.join(", ")
                ));
            }
        }
    }
    output.trim_end().to_owned()
}

fn display(row: &ResultRow) -> (&'static str, &'static str) {
    match row.outcome {
        Outcome::Passed => ("✓", "Passed"),
        Outcome::NotApplicable => ("-", "Not Applicable"),
        _ if row.required => ("✗", "Failed"),
        _ => ("△", "Warning"),
    }
}
fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Passed => "Passed",
        Outcome::Failed => "Failed",
        Outcome::Error => "Error",
        Outcome::PermissionDenied => "PermissionDenied",
        Outcome::NotApplicable => "NotApplicable",
        Outcome::Blocked => "Blocked",
    }
}
fn readiness_name(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::Ready => "Ready",
        Readiness::NotReady => "Not Ready",
        Readiness::NotEvaluated => "Not Evaluated",
        Readiness::Passed => "Passed",
        Readiness::Failed => "Failed",
    }
}
fn colorize(value: &str, row: &ResultRow) -> String {
    let code = match row.outcome {
        Outcome::Passed => "32",
        Outcome::NotApplicable => "90",
        _ if row.required => "31",
        _ => "33",
    };
    format!("\x1b[{code}m{value}\x1b[0m")
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('`', "\\`")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('<', "\\<")
        .replace('>', "\\>")
        .replace(['\n', '\r'], " ")
}
