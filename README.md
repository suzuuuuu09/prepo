# prepo

Preflight checks for development repositories.

`prepo` is a declarative CLI that checks whether a development repository is
ready to run in the current environment. It diagnoses the current state without
installing dependencies, starting services, or modifying the environment.

> `prepo` is currently under active development.

## Why prepo?

Tools such as Nix, Devbox, and mise can provide commands and dependencies, but a
repository may still be unable to run because a process is stopped, a port is
occupied, a service is unreachable, or a required file is missing.

`prepo` makes those project-specific prerequisites explicit in `prepo.toml` and
reports what passed, what failed, and what needs permission to run.

## Features

- Declarative checks configured with TOML
- Environment variable, path, command, process, TCP port, and HTTP checks
- Check dependencies and platform filters
- Parallel execution with configurable concurrency and timeouts
- Explicit permission flags for commands, network access, and secret values
- Human-readable, JSON, and Markdown output
- Full repository readiness checks and partial runs by ID or group
- Linux, macOS, and Windows support

## Installation

Build and install from source with Rust 1.85 or later:

```sh
git clone https://github.com/suzuuuuu09/prepo.git
cd prepo
cargo install --path .
```

For development without installing:

```sh
cargo run -- check
```

## Quick start

Create a configuration in the current repository:

```sh
prepo init
```

Edit the generated `prepo.toml`, validate it, and run the checks:

```sh
prepo validate
prepo check
```

A minimal configuration looks like this:

```toml
version = 1

[project]
name = "example"

[[checks]]
id = "git-directory"
type = "path"
name = "Git repository"
path = ".git"
expected = "exists"
kind = "directory"
```

## Example configuration

```toml
version = 1

[project]
name = "example-app"

[runner]
parallel = true
max_concurrency = 4
default_timeout_seconds = 10

[[checks]]
id = "nix-shell"
type = "env"
name = "Nix development shell"
group = "environment"
variable = "IN_NIX_SHELL"
condition = "exists"
required = false
help = "Run `nix develop` before starting development."

[[checks]]
id = "config-file"
type = "path"
name = "Application configuration"
group = "environment"
path = ".env"
expected = "exists"
kind = "file"

[[checks]]
id = "typecheck"
type = "command"
name = "Type check"
group = "application"
command = ["bun", "run", "typecheck"]
expected_exit_code = 0
timeout_seconds = 30

[[checks]]
id = "api-port"
type = "port"
name = "API port"
group = "application"
host = "127.0.0.1"
port = 3000
expected = "open"
depends_on = ["config-file"]
required = false

[[checks]]
id = "api-health"
type = "http"
name = "API health endpoint"
group = "application"
url = "http://127.0.0.1:3000/health"
method = "GET"
expected_status = 200
body_contains = "ok"
depends_on = ["api-port"]
required = false
```

The command and network checks in this example require explicit permissions:

```sh
prepo check --allow-command --allow-network
```

## Commands

### `prepo init`

Creates a placeholder `prepo.toml` in the current directory. It refuses to
overwrite an existing file.

### `prepo validate`

Validates the configuration without executing checks:

```sh
prepo validate
prepo validate --config ./config/prepo.toml
```

Validation includes field types, unknown fields, check IDs, dependencies,
cycles, platform compatibility, and check-specific constraints.

### `prepo check`

Runs all declared checks:

```sh
prepo check
```

Run selected checks by ID:

```sh
prepo check config-file api-health
```

Run a group:

```sh
prepo check --group application
```

Change the output format or execution mode:

```sh
prepo check --format json
prepo check --format markdown
prepo check --sequential
prepo check --no-color
```

`--no-color` is only valid with human-readable output. The `NO_COLOR`
environment variable is also respected.

## Permissions

Active checks do not run unless the corresponding permission is explicitly
granted.

| Flag | Required for |
| --- | --- |
| `--allow-command` | Every `command` check |
| `--allow-network` | Every `port` and `http` check |
| `--allow-env` | Environment value comparisons and HTTP header interpolation |

Environment checks using only `exists` or `not_exists`, along with path and
process checks, do not require a permission flag.

`--allow-command` is a broad permission. Configured commands run without a shell,
but inherit the current user's environment and can access the filesystem and
network with that user's privileges. `prepo` does not provide an OS-level
sandbox, so only grant this permission to trusted configurations.

HTTP headers may reference environment variables:

```toml
[checks.headers]
Authorization = "Bearer ${API_TOKEN}"
```

Running that check requires both `--allow-network` and `--allow-env`. Secret
values are not included in normal output or reports.

## Check types

| Type | Purpose | Main fields |
| --- | --- | --- |
| `env` | Check the presence or value of an environment variable | `variable`, `condition`, `expected` |
| `path` | Check whether a file or directory exists or is missing | `path`, `expected`, `kind` |
| `command` | Run a command and inspect its exit code or output | `command`, `expected_exit_code`, `stdout_contains`, `stderr_not_contains` |
| `process` | Check whether an exact process name is running or stopped | `process`, `expected` |
| `port` | Check a TCP port for `open`, `closed`, or `available` | `host`, `port`, `expected` |
| `http` | Check an HTTP endpoint with `GET` or `HEAD` | `url`, `method`, `expected_status`, `body_contains`, `headers` |

### Common fields

Every check supports these common fields:

```toml
[[checks]]
id = "unique-id"
type = "path"
name = "Display name"
group = "environment"
required = true
depends_on = []
platforms = ["linux", "macos", "windows"]
help = "How to resolve the problem."
```

- `id` must be unique kebab-case.
- `required` defaults to `true`.
- `depends_on` lists checks that must pass first.
- `platforms` limits a check to selected operating systems.
- Relative paths are resolved from the directory containing `prepo.toml`.

A required check cannot depend on an optional check. A check also cannot apply on
a platform where one of its dependencies does not apply.

### Environment conditions

`env` supports:

```text
exists
not_exists
equals
not_equals
contains
matches
```

Value-based conditions require `expected` and `--allow-env`. `matches` treats
`expected` as a regular expression.

### Path expectations

`path` supports `expected = "exists"` with `kind = "file"`, `"directory"`, or
`"any"`. Use `expected = "missing"` without `kind` to require that a path does
not exist.

### Command checks

Commands are arrays and are executed directly without a shell:

```toml
[[checks]]
id = "project-check"
type = "command"
name = "Project check"
command = ["bun", "run", "check"]
expected_exit_code = 0
stdout_contains = "success"
stderr_not_contains = "error"
working_directory = "frontend"
timeout_seconds = 30

[checks.env]
CI = "true"
```

Shell expressions such as `&&`, pipes, redirects, and variable expansion are not
interpreted unless a shell is explicitly used as the command.

## Dependencies and execution

Checks with no unresolved dependencies may run in parallel. The defaults are:

```toml
[runner]
parallel = true
max_concurrency = 4
default_timeout_seconds = 10
```

A check is reported as `Blocked` when any dependency does not pass. Checks that
do not apply to the current OS are reported as `NotApplicable`.

## Results and readiness

Each check keeps an objective outcome:

```text
Passed
Failed
Error
PermissionDenied
NotApplicable
Blocked
```

`required` controls how a non-passing result is displayed and whether it affects
readiness. Optional failures are displayed as warnings, but their underlying
outcomes remain available in JSON and Markdown output.

For a full run, readiness is:

- `Ready` when every applicable required check passed
- `Not Ready` when an applicable required check did not pass
- `Not Evaluated` when no required check was applicable

A run that selects IDs or a group is reported separately as `Passed`, `Failed`,
or `Not Evaluated`; it does not claim that the whole repository is ready.

## Output formats

Human-readable output is the default:

```sh
prepo check
```

Machine-readable JSON:

```sh
prepo check --format json
```

Shareable Markdown:

```sh
prepo check --format markdown
```

JSON output includes schema versions, the `prepo` version, project and platform
metadata, summary counts, duration, objective outcomes, reason codes, dependency
blocks, and missing permissions.

## Configuration discovery

Without `--config`, `prepo` searches for `prepo.toml` from the current directory
upward. The search stops when it finds the file, reaches the current Git
repository root, or reaches the filesystem root.

Use an explicit path when needed:

```sh
prepo check --config ./config/prepo.toml
```

## Exit codes

| Code | Meaning |
| ---: | --- |
| `0` | The full repository is ready, the selected run passed, or nothing applicable was evaluated |
| `1` | The repository is not ready or the selected run failed |
| `2` | Invalid CLI usage or configuration |
| `3` | Internal failure or unsupported operating system |
| `4` | Unknown check ID or empty group |
| `130` | Interrupted with Ctrl+C |
| `143` | Terminated with SIGTERM on Unix |

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The detailed design and planned extensions are documented in
[`docs/project-design.md`](docs/project-design.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).