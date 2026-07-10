# Require explicit permission for active checks

Treat repository configuration as untrusted by default and require per-invocation permission before a check may execute commands, communicate over the network, or expand environment-variable values. Configuration validation remains side-effect free and passive checks need no permission; this adds CLI friction but avoids turning a routine diagnostic command into implicit code execution or secret disclosure.

Missing authorization produces the dedicated `PermissionDenied` check outcome, not `Skipped` or `Error`, and dependent checks become `Blocked`. A required `PermissionDenied` check makes a Full Run Not Ready and produces exit code 1; an optional one is presented as a warning and does not by itself change exit code 0. `--allow-command` is an intentionally broad grant because, without an OS sandbox, a child process can use the invoking user's files, environment, and network regardless of prepo's narrower built-in permissions.

The MVP permission matrix is:

- `path`, `process`, and environment-variable existence checks require no permission.
- Environment-variable value comparisons require `--allow-env`.
- Built-in `port` and `http` checks require `--allow-network`; HTTP interpolation of environment values additionally requires `--allow-env`.
- `command` checks require only the broad `--allow-command` grant.

There is no `--allow-all` shortcut in the MVP. A `PermissionDenied` result identifies every missing flag in both human output and structured output so the user can make the next invocation explicit.

The command grant is intended only for commands the user trusts. It is not a sandbox guarantee: a hostile command may use ambient user authority or deliberately escape ordinary process-group containment.

Built-in HTTP checks support only GET and HEAD, never follow redirects, and do not expand environment variables in URLs. Environment expansion is limited to header values and requires both `--allow-network` and `--allow-env`. The network grant is intentionally strong: it authorizes DNS resolution and communication with any host named by the configured URL; a 3xx response is evaluated directly against the declared expected status.

HTTP checks use direct connections with system proxies disabled, WebPKI roots only, and mandatory certificate verification. They accept identity or a single gzip content encoding; custom CAs, native root stores, and additional encodings are outside the MVP.
