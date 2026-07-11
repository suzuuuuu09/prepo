# prepo

`prepo` is a Rust CLI that checks whether a development repository is ready to run in the current environment.

[日本語版](./README-ja.md)

## What it does

- Validates a declarative `prepo.toml` configuration
- Checks files, environment variables, processes, ports, HTTP endpoints, and commands
- Runs independent checks in parallel while respecting dependencies
- Produces human-readable, JSON, or Markdown results
- Never installs dependencies or modifies the environment

## Install

### From source

```bash
git clone https://github.com/suzuuuuu09/prepo.git
cd prepo
cargo install --path .
```

## Build

```bash
cargo build
cargo test
```

## Usage

```bash
cargo run -- init
cargo run -- validate
cargo run -- check
cargo run -- check <id>...
cargo run -- check --group <group>
cargo run -- check --format json
```

If installed, you can use `prepo` instead of `cargo run --`.

### Commands

- `init`: create a starter `prepo.toml` in the current directory
- `validate`: validate the configuration without running checks
- `check`: run all checks, selected IDs, or a group

## Configuration file

Checks are defined in `prepo.toml`.

```toml
version = 1

[project]
name = "example"

[[checks]]
id = "git-directory"
type = "path"
name = "Git repository"
path = ".git"
kind = "directory"

[[checks]]
id = "development-server"
type = "http"
name = "Development server"
url = "http://localhost:3000/health"
expected_status = 200
required = false
```

When `--config` is not provided, `prepo` searches the current directory and its parents for `prepo.toml`.

## Check types

- `env`: check whether an environment variable exists or matches a condition
- `path`: check whether a file or directory exists
- `command`: run a command and inspect its exit code or output
- `process`: check whether a process is running or stopped
- `port`: check whether a TCP port is open, closed, or available
- `http`: check an HTTP endpoint and response status

Checks can also define `required`, `group`, `depends_on`, `platforms`, `timeout_seconds`, and `help`.

## Permissions

Active checks require explicit permission flags.

```bash
prepo check --allow-command
prepo check --allow-network
prepo check --allow-env
```

- `--allow-command`: permits configured command checks
- `--allow-network`: permits port and HTTP checks
- `--allow-env`: permits environment value comparisons and environment expansion in HTTP headers

`--allow-command` lets configured commands inherit the current environment and access the filesystem and network. Only use it with trusted configurations.

## Output formats

```bash
prepo check --format human
prepo check --format json
prepo check --format markdown
```

Use `--sequential` to disable parallel execution and `--no-color` to disable colors in human-readable output.

## License

Licensed under the [Apache License 2.0](./LICENSE).
