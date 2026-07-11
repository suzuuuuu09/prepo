# prepo CLI contract

Use this reference for the initial prepo design. Prefer the installed version's `prepo --help` and current official project documentation if they differ.

## Purpose

`prepo` performs preflight checks for development repositories. It diagnoses whether a project can run in the current environment; it does not provision dependencies or replace Nix, Devbox, mise, Docker Compose, process managers, or build systems.

## Core commands

```text
prepo init [--force] [--minimal|--full]
prepo validate [--config <PATH>]
prepo check [CHECK_ID...] [--config <PATH>] [--group <GROUP>]
            [--format human|json|markdown] [--no-color] [--sequential]
            [--fail-fast] [--timeout <SECONDS>] [--verbose]
prepo report [--format markdown|json] [--output <PATH>]
```

Configuration is normally named `prepo.toml`. Discovery starts in the working directory and walks through parent directories until the repository or filesystem boundary defined by the implementation.

## Recommended AI invocation

```bash
prepo validate
prepo check --format json
```

Use `prepo check <id> --format json` or `prepo check --group <group> --format json` for focused reruns.

## Typical sessions

### Existing configuration

```bash
prepo validate
prepo check --format json
```

If validation fails, stop before `check`. Explain the invalid field and propose a correction.

### Configuration at a non-default path

```bash
prepo validate --config ./config/prepo.toml
prepo check --config ./config/prepo.toml --format json
```

Paths declared inside configuration are resolved relative to the directory containing `prepo.toml` unless the installed version documents otherwise.

### Focused diagnosis and verification

```bash
prepo check bridge-process bridge-port --format json
prepo check --group bridge --format json
```

After an approved fix, rerun the failed check and any dependents. Avoid rerunning unrelated active checks unnecessarily.

### Shareable report

```bash
prepo report --format markdown
prepo report --format markdown --output prepo-report.md
```

Writing a report file changes the workspace. Obtain confirmation before using `--output`. Review the report for secrets before sharing it.

### First-time configuration

```bash
prepo init --minimal
prepo validate
prepo check --format json
```

`prepo init` creates `prepo.toml`; `--force` overwrites an existing file. Both require confirmation. Inspect the repository and replace the generated generic check with project-specific checks before treating the result as meaningful.

## Minimal configuration example

```toml
version = 1

[project]
name = "example"

[[checks]]
id = "project-manifest"
type = "path"
name = "Project manifest"
path = "package.json"
kind = "file"
help = "Restore the project manifest or run prepo from the repository root."
```

Common fields include `id`, `type`, `name`, `group`, `required`, `timeout_seconds`, `depends_on`, `platforms`, and `help`. Check types in the initial design include `env`, `path`, `command`, `process`, `port`, and `http`. Inspect version-specific help or documentation before emitting fields because the implemented schema may differ from the design.

## JSON fields

The designed JSON envelope contains:

- `schema_version`
- `prepo_version`
- `project.name`
- `environment.os` and `environment.arch`
- `summary` counts
- `results[]`

Each result can contain `id`, `name`, `group`, `status`, `summary`, `details`, `help`, and `duration_ms`. Later versions may add fields such as requiredness, outcome, permission state, or partial-run metadata. Preserve unknown fields and use them when documented.

JSON belongs on stdout. Diagnostics and debug logs belong on stderr. Do not parse progress text as part of the JSON document.

## Designed exit codes

| Code | Meaning |
| --- | --- |
| 0 | All required checks passed; warnings may exist |
| 1 | At least one required check failed |
| 2 | Invalid configuration |
| 3 | Unexpected internal error |
| 4 | Requested check ID or group does not exist |

An exit code of 0 does not turn warnings, optional failures, skipped checks, or unavailable checks into passes.

## Reports and secrets

Reports may include project and environment metadata, check results, durations, details, and suggested fixes. They must not include environment-variable values, access tokens, private keys, or other secret material. Review generated Markdown before sharing it externally.

## Configuration guidance

Before generating `prepo.toml`, inspect only evidence relevant to repository readiness. Common checks include paths, environment-variable presence, commands and versions, processes, ports, HTTP endpoints, devices, Git state, and structured file values. Keep project-specific commands explicit and minimal. Never invent a check merely because a technology appears in a lockfile.
