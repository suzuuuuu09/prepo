# Do not retain raw observations

Never place environment values, HTTP headers or bodies, cookies, command stdout, or command stderr in `CheckResult`, human output, JSON, or Markdown. Checks retain raw observations only for the minimum time needed to evaluate declared predicates and then discard them, reporting safe reasons such as an exit-code or body-condition mismatch; this reduces diagnostic detail but makes reports safe to share without relying on incomplete automatic redaction.

Process stdout and stderr are streamed with a fixed limit of 8 MiB per stream, and decompressed HTTP response bodies with a fixed limit of 1 MiB. Exceeding a limit immediately stops the operation and returns `Ok(CheckResult)` with `CheckOutcome::Error` and reason `ObservationLimitExceeded`; required policy then derives `Failed` or `Warning`. The MVP does not allow these limits to be raised in configuration.
