# prepo

[English](./README.md) | [日本語](./README.ja.md)

> Preflight checks for development repositories.

`prepo` is a Rust CLI that checks whether a development repository is ready to run in the current environment.

It does not install dependencies or modify the environment. Instead, it checks files, environment variables, processes, ports, HTTP endpoints, and project-specific commands.

## Usage

```sh
prepo init
prepo validate
prepo check
```

Run a specific check or group:

```sh
prepo check development-server
prepo check --group network
```

Active checks require explicit permission:

```sh
prepo check --allow-command
prepo check --allow-network
prepo check --allow-env
```

`--allow-command` lets configured commands inherit the current environment and access the filesystem and network. Only use it with trusted configurations.

## Configuration

Create `prepo.toml` in the repository:

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

Supported check types:

```text
env, path, command, process, port, http
```

## Output

```sh
prepo check
prepo check --format json
prepo check --format markdown
```

## Development

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## License

Licensed under the [Apache License 2.0](./LICENSE).
