use super::containment::{ContainmentOps, ReapResult};
use super::observation::{self, Observation, ObserveError};
use super::{
    ActiveCommand, CleanupFailed, CommandSpec, ExecutionResult, RunCleaningUp, RunState,
    Termination,
};
use crate::result::{ErrorReason, FailureReason};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub(super) async fn evaluate(
    state: Arc<RunState>,
    active: Arc<ActiveCommand>,
    root: &Path,
    spec: &CommandSpec,
) -> Result<ExecutionResult, RunCleaningUp> {
    let launched = match active.launch(root, spec) {
        Ok(launched) => launched,
        Err(super::LaunchError::CleaningUp) => return Err(RunCleaningUp),
        Err(error) => {
            active.launch_failed();
            state.remove(active.id);
            return Ok(map_launch_error(error));
        }
    };
    let deadline = launched.started_at + Duration::from_secs(spec.timeout_seconds);
    let mut reaped = launched.reaped;
    let mut observations = Box::pin(async {
        tokio::try_join!(
            observation::observe_stream(launched.stdout, spec.stdout_contains.as_deref()),
            observation::observe_stream(launched.stderr, spec.stderr_not_contains.as_deref()),
        )
    });
    let result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
        tokio::select! {
            reap = wait_reaped(&mut reaped, deadline) => Event::Reaped(reap),
            observed = &mut observations => Event::Observed(observed),
        }
    })
    .await;

    let result = match result {
        Err(_) => cleanup_result(&active, FailureReason::TimedOut).await,
        Ok(Event::Reaped(Err(()))) => {
            if Instant::now() >= deadline {
                cleanup_result(&active, FailureReason::TimedOut).await
            } else {
                cleanup_result(&active, ErrorReason::CommandRuntimeFailed).await
            }
        }
        Ok(Event::Reaped(Ok(reap))) => {
            match tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                &mut observations,
            )
            .await
            {
                Err(_) => cleanup_result(&active, FailureReason::TimedOut).await,
                Ok(Err(error)) => map_observation_error(&active, error).await,
                Ok(Ok((stdout, stderr))) => {
                    evaluate_completed(&active, reap, spec, stdout, stderr).await
                }
            }
        }
        Ok(Event::Observed(Err(error))) => map_observation_error(&active, error).await,
        Ok(Event::Observed(Ok((stdout, stderr)))) => {
            match wait_reaped(&mut reaped, deadline).await {
                Err(()) => cleanup_result(&active, FailureReason::TimedOut).await,
                Ok(reap) => evaluate_completed(&active, reap, spec, stdout, stderr).await,
            }
        }
    };
    state.remove(active.id);
    Ok(result)
}

pub(super) async fn run_termination(termination: Arc<Termination>) {
    let result = loop {
        if let Some(process) = termination.active.process() {
            let _ = process.containment.request_stop();
            let mut reaped = process.reaped.clone();
            let reap_result = wait_reaped(&mut reaped, termination.deadline).await;
            if reap_result.is_err() {
                break Err(CleanupFailed);
            }
            let confirmed = process
                .containment
                .confirm_empty(termination.deadline)
                .await;
            break confirmed.map_err(|_| CleanupFailed);
        }
        if termination.active.finish_reserved() || termination.active.is_done() {
            break Ok(());
        }
        if Instant::now() >= termination.deadline {
            break Err(CleanupFailed);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let _ = termination.sender.send(Some(result));
}

pub(super) async fn wait_reaped(
    receiver: &mut watch::Receiver<Option<ReapResult>>,
    deadline: Instant,
) -> Result<ReapResult, ()> {
    loop {
        if Instant::now() >= deadline {
            return Err(());
        }
        if let Some(result) = *receiver.borrow() {
            return if result.wait_failed {
                Err(())
            } else {
                Ok(result)
            };
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if tokio::time::timeout(remaining, receiver.changed())
            .await
            .is_err()
        {
            return Err(());
        }
    }
}

enum Event {
    Reaped(Result<ReapResult, ()>),
    Observed(Result<(Observation, Observation), ObserveError>),
}

fn map_launch_error(error: super::LaunchError) -> ExecutionResult {
    match error {
        super::LaunchError::NotFound => ExecutionResult::failed(FailureReason::CommandNotFound),
        super::LaunchError::NotExecutable => {
            ExecutionResult::failed(FailureReason::CommandNotExecutable)
        }
        super::LaunchError::WorkingDirectoryInvalid => {
            ExecutionResult::failed(FailureReason::WorkingDirectoryInvalid)
        }
        super::LaunchError::RuntimeFailed => {
            ExecutionResult::error(ErrorReason::CommandRuntimeFailed)
        }
        super::LaunchError::ContainmentFailed => {
            ExecutionResult::error(ErrorReason::CommandContainmentFailed)
        }
        super::LaunchError::CleanupFailed => {
            ExecutionResult::error(ErrorReason::CommandCleanupFailed)
        }
        super::LaunchError::CleaningUp => unreachable!(),
    }
}

async fn map_observation_error(
    active: &Arc<ActiveCommand>,
    error: ObserveError,
) -> ExecutionResult {
    match error {
        ObserveError::LimitExceeded => {
            cleanup_result(active, ErrorReason::ObservationLimitExceeded).await
        }
        ObserveError::ReadFailed => {
            cleanup_result(active, ErrorReason::CommandOutputReadFailed).await
        }
    }
}

async fn cleanup_result(
    active: &Arc<ActiveCommand>,
    result: impl Into<CleanupReason>,
) -> ExecutionResult {
    let result = result.into();
    active.start_termination(Instant::now() + Duration::from_secs(5));
    let cleanup_ok = active.wait_termination().await.is_ok();
    if !cleanup_ok {
        return ExecutionResult::error(ErrorReason::CommandCleanupFailed);
    }
    result.into_execution()
}

enum CleanupReason {
    Failed(FailureReason),
    Error(ErrorReason),
}

impl From<FailureReason> for CleanupReason {
    fn from(reason: FailureReason) -> Self {
        Self::Failed(reason)
    }
}

impl From<ErrorReason> for CleanupReason {
    fn from(reason: ErrorReason) -> Self {
        Self::Error(reason)
    }
}

impl CleanupReason {
    fn into_execution(self) -> ExecutionResult {
        match self {
            Self::Failed(reason) => ExecutionResult::failed(reason),
            Self::Error(reason) => ExecutionResult::error(reason),
        }
    }
}

async fn evaluate_completed(
    active: &Arc<ActiveCommand>,
    reap: ReapResult,
    spec: &CommandSpec,
    stdout: Observation,
    stderr: Observation,
) -> ExecutionResult {
    let result = classify_completed(reap, spec, stdout, stderr);
    if active.disarm().is_err() {
        active.start_termination(Instant::now() + Duration::from_secs(5));
        let _ = active.wait_termination().await;
        return ExecutionResult::error(ErrorReason::CommandCleanupFailed);
    }
    result
}

fn classify_completed(
    reap: ReapResult,
    spec: &CommandSpec,
    stdout: Observation,
    stderr: Observation,
) -> ExecutionResult {
    if reap.wait_failed {
        ExecutionResult::error(ErrorReason::CommandRuntimeFailed)
    } else if reap.terminated {
        ExecutionResult::failed(FailureReason::CommandTerminated)
    } else if reap.exit_code != Some(spec.expected_exit_code) {
        ExecutionResult::failed(FailureReason::ExitCodeMismatch)
    } else if spec.stdout_contains.is_some() && !stdout.matched {
        ExecutionResult::failed(FailureReason::StdoutConditionMismatch)
    } else if spec.stderr_not_contains.is_some() && stderr.matched {
        ExecutionResult::failed(FailureReason::StderrConditionMismatch)
    } else {
        ExecutionResult::passed()
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_completed, map_launch_error};
    use crate::checks::command::LaunchError;
    use crate::checks::command::containment::ReapResult;
    use crate::config::CommandSpec;
    use crate::result::{ErrorReason, ExecutionResult, FailureReason};
    use std::collections::BTreeMap;

    fn spec() -> CommandSpec {
        CommandSpec {
            command: vec!["test".to_owned()],
            expected_exit_code: 0,
            stdout_contains: Some(b"ok".to_vec()),
            stderr_not_contains: Some(b"bad".to_vec()),
            working_directory: None,
            environment_overrides: BTreeMap::new(),
            timeout_seconds: 1,
        }
    }

    fn observed(matched: bool) -> super::Observation {
        super::Observation { matched }
    }

    #[test]
    fn terminal_reason_precedence_is_closed() {
        let mut reap = ReapResult {
            exit_code: Some(1),
            terminated: false,
            wait_failed: false,
        };
        assert_eq!(
            classify_completed(reap, &spec(), observed(true), observed(false)),
            ExecutionResult::failed(FailureReason::ExitCodeMismatch)
        );

        reap.exit_code = Some(0);
        assert_eq!(
            classify_completed(reap, &spec(), observed(false), observed(false)),
            ExecutionResult::failed(FailureReason::StdoutConditionMismatch)
        );
        assert_eq!(
            classify_completed(reap, &spec(), observed(true), observed(true)),
            ExecutionResult::failed(FailureReason::StderrConditionMismatch)
        );
        assert_eq!(
            classify_completed(reap, &spec(), observed(true), observed(false)),
            ExecutionResult::passed()
        );

        reap.terminated = true;
        assert_eq!(
            classify_completed(reap, &spec(), observed(true), observed(false)),
            ExecutionResult::failed(FailureReason::CommandTerminated)
        );
        reap.terminated = false;
        reap.wait_failed = true;
        assert_eq!(
            classify_completed(reap, &spec(), observed(true), observed(false)),
            ExecutionResult::error(ErrorReason::CommandRuntimeFailed)
        );
    }

    #[test]
    fn unexpected_launch_failures_are_errors() {
        assert_eq!(
            map_launch_error(LaunchError::RuntimeFailed),
            ExecutionResult::error(ErrorReason::CommandRuntimeFailed)
        );
    }
}
