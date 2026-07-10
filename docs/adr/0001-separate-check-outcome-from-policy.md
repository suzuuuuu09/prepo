# Separate check outcomes from readiness policy

Represent a check's objective outcome independently from its `required` policy and presentation severity. This keeps JSON semantics stable, allows Ready and CLI exit status to be derived explicitly, and distinguishes a check-local inability to evaluate (`Error`) from a fatal failure of `prepo` itself; the trade-off is a slightly richer result model than a single display-oriented status enum.

A check-local inability to evaluate is returned as `Ok(CheckResult)` with `CheckOutcome::Error`. The `required` policy derives presentation (`Failed` or `Warning`) and exit status; Rust `Err` is reserved for a fatal prepo error that prevents a trustworthy run and maps to exit code 3.

Structured output stores `outcome` and `required`, never the derived presentation state. Outcome counts include every result; evaluation counts include only Passed, Failed, and Error. Full Runs expose `ready`, `not_ready`, or `not_evaluated`, while Partial Runs omit readiness and add each result's `explicit` or `dependency` selection origin. Optional fields are omitted only where the run kind makes them inapplicable; per-result `reason` is null for Passed, and `blocked_by` and `missing_permissions` are otherwise always arrays.

`applicable_count` counts every result except NotApplicable, while `evaluated_required_count` counts only Required Checks with Passed, Failed, or Error outcomes. `total_duration_ms` is monotonic elapsed run time rather than the sum of individual durations. Per-result `group` and `help` are always present and use null when absent; only readiness and selection are conditional on run kind.
