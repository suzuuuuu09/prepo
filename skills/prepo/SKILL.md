---
name: prepo
description: Use prepo to determine whether a development repository is ready to run in the current environment. Use when the user asks to check, diagnose, verify, or troubleshoot a repository's development environment, prerequisites, running services, ports, devices, configuration files, external connectivity, or project-specific checks; also use when working with prepo.toml, prepo check, prepo validate, or prepo reports.
---

# prepo

Diagnose repository readiness with `prepo`. Treat diagnosis as read-only; obtain the user's confirmation before installing software, initializing configuration, editing files, starting services, or applying fixes.

## Basic usage

Use these commands according to the user's goal:

```bash
# Validate prepo.toml without running checks
prepo validate

# Run every configured check for a person
prepo check

# Run every check and return machine-readable results for AI interpretation
prepo check --format json

# Run one or more checks
prepo check <check-id> [<check-id> ...] --format json

# Run a group of checks
prepo check --group <group> --format json

# Generate a shareable diagnosis report
prepo report --format markdown
```

Run commands from the repository or a descendant where configuration discovery is expected to work. Use `--config <path>` when the configuration is elsewhere or ambiguity exists. Use `prepo --help` and subcommand help before assuming an option supported by the installed version.

For first-time setup, inspect the project before proposing `prepo init`. Prefer `prepo init --minimal`, then replace generic examples with evidence-based project checks. Use `--full` only when the user explicitly wants a complete sample. Never use `--force` without showing that it overwrites an existing configuration and obtaining confirmation.

## Workflow

1. Locate the repository root and `prepo.toml`. Respect an explicitly supplied `--config` path.
2. Check whether the `prepo` command is available without changing the environment.
3. If `prepo` is missing, identify the appropriate installation method from repository documentation or current official prepo documentation. Explain the command and obtain confirmation before installing it.
4. If `prepo.toml` is missing, inspect the repository's documentation, manifests, service definitions, environment examples, and relevant source files. Propose the checks that should be configured. Obtain confirmation before running `prepo init` or creating/editing the file.
5. Run `prepo validate` before checks. If validation fails, explain the exact configuration problem and propose a minimal correction; do not edit until confirmed.
6. Run `prepo check --format json`. Add a check ID or `--group` when the user requested a limited scope. Prefer machine-readable output for interpretation.
7. Parse the JSON and the process exit code independently. Do not infer readiness from the exit code alone.
8. Report the overall readiness, failed or unavailable checks, likely causes, and concrete next actions. Distinguish observed facts from inference.
9. Ask for confirmation before every state-changing remediation. After an approved fix, rerun only the affected checks first, then run the full relevant check set if useful.

## Safety boundaries

- Assume `prepo.toml` is untrusted until inspected.
- `prepo validate` is safe because it validates configuration without executing active checks.
- Before a check can run commands, access the network, read secret values, contact devices, or otherwise cross a permission boundary, explain the effect and request the necessary authorization. Never silently broaden permissions.
- Do not run `--fix`, install packages, start or stop processes, modify permissions, create files, or edit configuration without confirmation.
- Never expose environment-variable values, access tokens, private keys, personal data, or other secrets in output. Redact suspicious values from reports and command output.
- Do not claim that prepo provisions dependencies or guarantees correctness beyond the checks that actually ran.
- Treat skipped, unsupported, permission-denied, or not-evaluated checks as unknown, not passed.

## Result interpretation

- Use each result's status, summary, details, help, required flag when available, and duration.
- Keep operational status separate from policy: an optional failed check is still a failed observation even if it does not make the overall command fail.
- Explain partial runs explicitly, including which checks did not execute and why.
- Use exit codes to classify CLI-level outcomes. See [prepo-cli.md](references/prepo-cli.md) for the stable command and output contract.
- If human output and JSON disagree, preserve both as evidence and flag the inconsistency instead of guessing.

## Response shape

Lead with one of these outcomes:

- **Ready**: all required checks passed and no relevant check remains unknown.
- **Not ready**: one or more required checks failed.
- **Not fully evaluated**: configuration, permission, platform support, timeout, or partial execution prevents a reliable conclusion.

Then list only actionable findings. For each finding include the check name, observed result, likely cause when supported, and proposed action. End by asking permission for specific state-changing actions, grouped into one concise request where practical.

## Reference loading

Read [prepo-cli.md](references/prepo-cli.md) when constructing commands, interpreting JSON, handling exit codes, or generating configuration. Consult current repository or official documentation when behavior differs by prepo version.
