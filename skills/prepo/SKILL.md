---
name: prepo
description: Use prepo to diagnose declared development-repository prerequisites. Use when the user asks to check, diagnose, verify, or troubleshoot a repository's configured prerequisites, processes, ports, configuration files, external connectivity, or prepo.toml.
---

# prepo

Diagnose declared repository readiness with `prepo`. Diagnosis is read-only. Obtain confirmation before installing software, creating configuration, editing files, starting services, or applying fixes.

## Commands

```bash
# Validate prepo.toml without executing checks.
prepo validate

# Run every configured check.
prepo check

# Run every configured check and emit machine-readable output.
prepo check --format json

# Run selected checks or one group.
prepo check <check-id> [<check-id> ...] --format json
prepo check --group <group> --format json
```

`prepo init` creates a placeholder `prepo.toml` in the current directory. It has no options and never overwrites an existing path. Inspect the repository and obtain confirmation before running it.

Use `--config <path>` only with `validate` or `check` when configuration discovery is unsuitable. Before emitting a command, consult `prepo --help` when the installed version may differ.

## Workflow

1. Locate `prepo.toml` and inspect it as untrusted input.
2. Run `prepo validate`; stop if validation fails.
3. Run `prepo check --format json`, or select IDs/a group only when the user requested a limited diagnosis.
4. Interpret JSON and the process exit code independently. JSON describes outcomes; exit status describes the command-level result.
5. Report only observed outcomes and supported inferences. Ask before every state-changing remediation.

## Permissions and safety

- `prepo validate` is side-effect free.
- Command checks require `--allow-command`; this broad grant permits configured commands to use inherited environment, filesystem, and network access.
- Port and HTTP checks require `--allow-network`.
- Environment-value conditions and HTTP header interpolation require `--allow-env`.
- Do not broaden permissions silently. Never expose environment values, command output, HTTP bodies, credentials, or tokens.
- prepo diagnoses only declared checks. It does not provision dependencies or guarantee absolute project executability.

## Result interpretation

Each result has an objective `outcome`: `passed`, `failed`, `error`, `permission_denied`, `not_applicable`, or `blocked`.

- A Full Run is **Ready** only when every applicable required check passed.
- A Full Run is **Not Ready** when an applicable required check is Failed, Error, PermissionDenied, or Blocked.
- A Full Run is **Not Evaluated** when no required check is applicable.
- A Partial Run does not claim repository readiness. Report its selected-run result and distinguish explicit checks from dependency checks.

Optional checks do not decide readiness, but their objective outcomes remain failures or errors and must not be presented as passes.

## Structured output

JSON is the only stdout output in `--format json` mode. Its top-level fields include `output_schema_version`, `config_schema_version`, `prepo_version`, `run_kind`, `project`, `environment`, `summary`, `total_duration_ms`, and `results`. Full Runs also include `readiness`.

Each result includes `id`, `name`, `group`, `required`, `outcome`, `reason`, `summary`, `help`, `duration_ms`, `blocked_by`, and `missing_permissions`. Partial Runs additionally include `selection` (`explicit` or `dependency`). Do not expect raw observations or display-only status fields.

See [prepo-cli.md](references/prepo-cli.md) for the stable command and output contract.
