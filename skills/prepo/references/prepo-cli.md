# prepo CLI contract

This reference describes prepo v0.1.1. Prefer the installed command's help if it differs.

## Commands

```text
prepo init
prepo validate [--config <PATH>]
prepo check [ID...] [--config <PATH>] [--group <GROUP>]
            [--format human|json|markdown] [--sequential] [--no-color]
            [--allow-command] [--allow-network] [--allow-env]
```

`init` writes a new placeholder `prepo.toml` only in the current directory. It has no options and refuses to overwrite a file, directory, or symbolic link.

`validate` is static and side-effect free. `check` evaluates the configured checks. There is no `report`, `--force`, `--minimal`, `--full`, `--fail-fast`, global `--timeout`, `--verbose`, or output-file option.

## Recommended invocation

```bash
prepo validate
prepo check --format json
```

For a focused diagnosis, use either IDs or a group, never both:

```bash
prepo check bridge-process bridge-port --format json
prepo check --group bridge --format json
```

Dependencies of selected checks are added automatically. A Partial Run does not determine whole-repository readiness.

## Permissions

`--allow-command` is a broad grant for command checks. `--allow-network` permits configured port and HTTP checks, including DNS resolution. `--allow-env` permits environment-value checks and HTTP header interpolation. Missing permission yields `permission_denied`, not a passed or skipped result.

## JSON

JSON output is written alone to stdout. The envelope contains:

- `output_schema_version`
- `config_schema_version`
- `prepo_version`
- `run_kind`
- `project`
- `environment`
- `total_duration_ms`
- `summary`
- `results`

Full Runs additionally contain `readiness`. A result contains `id`, `name`, `group`, `required`, `outcome`, `reason`, `summary`, `help`, `duration_ms`, `blocked_by`, and `missing_permissions`. Partial-Run results also contain `selection`.

Outcomes are `passed`, `failed`, `error`, `permission_denied`, `not_applicable`, and `blocked`. Do not infer readiness from the process exit code alone.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | Full Run is Ready or Not Evaluated; Partial Run passed or was not evaluated |
| 1 | Full Run is Not Ready or Partial Run failed |
| 2 | Invalid CLI usage or configuration |
| 3 | Internal failure or unsupported operating system |
| 4 | Unknown check ID or empty group |
| 130 | Interrupted with Ctrl+C |
| 143 | Interrupted with SIGTERM on Unix |

## Safety

Reports never retain environment values, command output, HTTP bodies, or other raw observations. `prepo` diagnoses declared checks only; it does not install dependencies, change configuration, or guarantee project correctness beyond checks that actually ran.
