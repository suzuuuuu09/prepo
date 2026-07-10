# Contain command process trees

Run every command check inside an OS-level process container so timeout or observation-limit cancellation terminates normal descendant processes, using process groups on Unix and Job Objects on Windows. If containment cannot be established, do not start the command and return `CheckOutcome::Error`; if termination cannot be confirmed, return `Error` with a prominent warning that processes may remain. This platform-specific complexity is required because killing only the direct child could leave descendants running after a diagnostic completes.

Check runs install interruption handlers before scheduling. The first Ctrl-C or Unix SIGTERM stops scheduling, cancels network operations, terminates contained process trees, waits up to five seconds for cleanup, emits no partial result, and exits with 130 or 143 respectively. A second signal performs synchronous best-effort tree termination and exits immediately; cleanup failure is warned about without replacing the signal-derived exit code.

This is containment for trusted commands, not an adversarial sandbox. A process that deliberately escapes its Unix process group or otherwise evades the OS container is outside the MVP threat model. Prepo verifies only the processes still owned by its container and never guesses by process name or kills unrelated system processes.
