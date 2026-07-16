mod containment;
mod lifecycle;
mod observation;

use crate::config::CommandSpec;
use crate::result::ExecutionResult;
use containment::{ContainmentOps, ReapResult};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct CommandRunner {
    state: Arc<RunState>,
    project_root: Arc<PathBuf>,
}

pub(crate) struct CommandControl {
    state: Arc<RunState>,
}

pub(crate) struct CommandCleanup {
    batch: Arc<CleanupBatch>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunCleaningUp;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CleanupFailed;

pub(crate) fn for_run(project_root: PathBuf) -> (CommandRunner, CommandControl) {
    let state = Arc::new(RunState::new());
    (
        CommandRunner {
            state: Arc::clone(&state),
            project_root: Arc::new(project_root),
        },
        CommandControl { state },
    )
}

impl CommandRunner {
    pub(crate) async fn evaluate(
        &self,
        spec: &CommandSpec,
    ) -> Result<ExecutionResult, RunCleaningUp> {
        let active = self.state.reserve()?;
        lifecycle::evaluate(
            Arc::clone(&self.state),
            active,
            self.project_root.as_path(),
            spec,
        )
        .await
    }
}

impl CommandControl {
    pub(crate) fn begin_cleanup(&self) -> CommandCleanup {
        let batch = self.state.begin_cleanup();
        batch.start_requests();
        batch.start_waiter();
        CommandCleanup { batch }
    }

    pub(crate) fn force_terminate(&self) -> Result<(), CleanupFailed> {
        let batch = self.state.ensure_cleanup();
        let mut failed = false;
        for target in &batch.targets {
            if target.request_stop_best_effort().is_err() {
                failed = true;
            }
        }
        if failed { Err(CleanupFailed) } else { Ok(()) }
    }
}

impl CommandCleanup {
    pub(crate) async fn wait(&self) -> Result<(), CleanupFailed> {
        let mut result = self.batch.result.clone();
        loop {
            if let Some(result) = *result.borrow() {
                return result;
            }
            if result.changed().await.is_err() {
                return Err(CleanupFailed);
            }
        }
    }
}

struct RunState {
    inner: Mutex<RunInner>,
}

struct RunInner {
    phase: RunPhase,
    next_id: u64,
    active: BTreeMap<u64, Arc<ActiveCommand>>,
}

enum RunPhase {
    Running,
    CleaningUp(Arc<CleanupBatch>),
    Cleaned,
}

impl RunState {
    fn new() -> Self {
        Self {
            inner: Mutex::new(RunInner {
                phase: RunPhase::Running,
                next_id: 0,
                active: BTreeMap::new(),
            }),
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<Arc<ActiveCommand>, RunCleaningUp> {
        let mut inner = self.inner.lock().expect("run state lock poisoned");
        if !matches!(inner.phase, RunPhase::Running) {
            return Err(RunCleaningUp);
        }
        let id = inner.next_id;
        inner.next_id = inner.next_id.wrapping_add(1);
        let active = Arc::new(ActiveCommand::new(id));
        inner.active.insert(id, Arc::clone(&active));
        Ok(active)
    }

    fn remove(&self, id: u64) {
        let mut inner = self.inner.lock().expect("run state lock poisoned");
        inner.active.remove(&id);
    }

    fn begin_cleanup(self: &Arc<Self>) -> Arc<CleanupBatch> {
        let mut inner = self.inner.lock().expect("run state lock poisoned");
        if let RunPhase::CleaningUp(batch) = &inner.phase {
            return Arc::clone(batch);
        }
        let targets = inner.active.values().cloned().collect::<Vec<_>>();
        let (sender, result) = watch::channel(None);
        let batch = Arc::new(CleanupBatch {
            targets,
            deadline: Instant::now() + Duration::from_secs(5),
            waiter_started: AtomicBool::new(false),
            sender,
            result,
        });
        inner.phase = RunPhase::CleaningUp(Arc::clone(&batch));
        batch
    }

    fn ensure_cleanup(self: &Arc<Self>) -> Arc<CleanupBatch> {
        let batch = self.begin_cleanup();
        batch.start_requests();
        batch.start_waiter();
        batch
    }
}

impl Drop for RunState {
    fn drop(&mut self) {
        let inner = self
            .inner
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for active in inner.active.values() {
            let _ = active.request_stop_sync();
        }
        inner.phase = RunPhase::Cleaned;
    }
}

struct CleanupBatch {
    targets: Vec<Arc<ActiveCommand>>,
    deadline: Instant,
    waiter_started: AtomicBool,
    sender: watch::Sender<Option<Result<(), CleanupFailed>>>,
    result: watch::Receiver<Option<Result<(), CleanupFailed>>>,
}

impl CleanupBatch {
    fn start_requests(self: &Arc<Self>) {
        for target in &self.targets {
            target.start_termination(self.deadline);
        }
    }

    fn start_waiter(self: &Arc<Self>) {
        if self
            .waiter_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let batch = Arc::clone(self);
        tokio::spawn(async move {
            let mut ok = true;
            for target in &batch.targets {
                if target.wait_termination().await.is_err() {
                    ok = false;
                }
            }
            let _ = batch
                .sender
                .send(Some(if ok { Ok(()) } else { Err(CleanupFailed) }));
        });
    }
}

struct ActiveCommand {
    id: u64,
    gate: Mutex<ActiveState>,
    cleanup_requested: AtomicBool,
    termination: Mutex<Option<Arc<Termination>>>,
}

enum ActiveState {
    Reserved,
    Active(Arc<ActiveProcess>),
    Disarmed,
    Completed,
}

struct ActiveProcess {
    containment: containment::ContainedProcess,
    reaped: watch::Receiver<Option<ReapResult>>,
}

struct Termination {
    active: Arc<ActiveCommand>,
    deadline: Instant,
    sender: watch::Sender<Option<Result<(), CleanupFailed>>>,
    result: watch::Receiver<Option<Result<(), CleanupFailed>>>,
}

struct Launched {
    stdout: Option<containment::Output>,
    stderr: Option<containment::Output>,
    reaped: watch::Receiver<Option<ReapResult>>,
    started_at: Instant,
}

impl ActiveCommand {
    fn new(id: u64) -> Self {
        Self {
            id,
            gate: Mutex::new(ActiveState::Reserved),
            cleanup_requested: AtomicBool::new(false),
            termination: Mutex::new(None),
        }
    }

    fn launch(self: &Arc<Self>, root: &Path, spec: &CommandSpec) -> Result<Launched, LaunchError> {
        let mut state = self.gate.lock().expect("active command lock poisoned");
        if !matches!(*state, ActiveState::Reserved)
            || self.cleanup_requested.load(Ordering::Acquire)
        {
            *state = ActiveState::Completed;
            return Err(LaunchError::CleaningUp);
        }
        let spawned = containment::launch(root, spec).map_err(|error| match error {
            containment::LaunchError::NotFound => LaunchError::NotFound,
            containment::LaunchError::NotExecutable => LaunchError::NotExecutable,
            containment::LaunchError::WorkingDirectoryInvalid => {
                LaunchError::WorkingDirectoryInvalid
            }
            containment::LaunchError::ContainmentFailed => LaunchError::ContainmentFailed,
            containment::LaunchError::CleanupFailed => LaunchError::CleanupFailed,
        })?;
        let process = Arc::new(ActiveProcess {
            containment: spawned.process,
            reaped: spawned.reaped.clone(),
        });
        *state = ActiveState::Active(Arc::clone(&process));
        Ok(Launched {
            stdout: spawned.stdout,
            stderr: spawned.stderr,
            reaped: spawned.reaped,
            started_at: spawned.started_at,
        })
    }

    fn launch_failed(&self) {
        let mut state = self.gate.lock().expect("active command lock poisoned");
        *state = ActiveState::Completed;
    }

    fn process(&self) -> Option<Arc<ActiveProcess>> {
        let state = self.gate.lock().expect("active command lock poisoned");
        match &*state {
            ActiveState::Active(process) => Some(Arc::clone(process)),
            _ => None,
        }
    }

    fn is_done(&self) -> bool {
        let state = self.gate.lock().expect("active command lock poisoned");
        matches!(*state, ActiveState::Completed | ActiveState::Disarmed)
    }

    fn finish_reserved(&self) -> bool {
        if !self.cleanup_requested.load(Ordering::Acquire) {
            return false;
        }
        let mut state = self.gate.lock().expect("active command lock poisoned");
        if matches!(*state, ActiveState::Reserved) {
            *state = ActiveState::Completed;
            true
        } else {
            false
        }
    }

    fn request_stop_sync(&self) -> Result<(), CleanupFailed> {
        self.cleanup_requested.store(true, Ordering::Release);
        let process = self.process();
        match process {
            Some(process) => process
                .containment
                .request_stop()
                .map_err(|_| CleanupFailed),
            None => Ok(()),
        }
    }

    fn request_stop_best_effort(&self) -> Result<(), CleanupFailed> {
        self.cleanup_requested.store(true, Ordering::Release);
        let process = {
            let state = match self.gate.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(CleanupFailed),
            };
            match &*state {
                ActiveState::Active(process) => Some(Arc::clone(process)),
                _ => None,
            }
        };
        match process {
            Some(process) => process
                .containment
                .request_stop()
                .map_err(|_| CleanupFailed),
            None => Ok(()),
        }
    }

    fn start_termination(self: &Arc<Self>, deadline: Instant) {
        self.cleanup_requested.store(true, Ordering::Release);
        let (termination, created) = {
            let mut slot = self.termination.lock().expect("termination lock poisoned");
            if let Some(existing) = slot.as_ref() {
                (Arc::clone(existing), false)
            } else {
                let (sender, result) = watch::channel(None);
                let termination = Arc::new(Termination {
                    active: Arc::clone(self),
                    deadline,
                    sender,
                    result,
                });
                *slot = Some(Arc::clone(&termination));
                (termination, true)
            }
        };
        if created {
            tokio::spawn(lifecycle::run_termination(termination));
        }
    }

    async fn wait_termination(&self) -> Result<(), CleanupFailed> {
        let termination = self
            .termination
            .lock()
            .expect("termination lock poisoned")
            .clone();
        let Some(termination) = termination else {
            return Ok(());
        };
        let mut result = termination.result.clone();
        loop {
            if let Some(result) = *result.borrow() {
                return result;
            }
            if result.changed().await.is_err() {
                return Err(CleanupFailed);
            }
        }
    }

    fn disarm(&self) -> Result<(), ()> {
        if self.cleanup_requested.load(Ordering::Acquire) {
            return Err(());
        }
        let process = self.process().ok_or(())?;
        process.containment.disarm()?;
        let mut state = self.gate.lock().expect("active command lock poisoned");
        *state = ActiveState::Disarmed;
        Ok(())
    }
}

#[derive(Debug)]
enum LaunchError {
    CleaningUp,
    NotFound,
    NotExecutable,
    WorkingDirectoryInvalid,
    ContainmentFailed,
    CleanupFailed,
}

#[cfg(test)]
mod tests {
    use super::RunState;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn cleanup_snapshots_reserved_slots_once_and_blocks_new_launches() {
        let state = Arc::new(RunState::new());
        let _reserved = state.reserve().expect("run starts in the running phase");
        let first = state.begin_cleanup();
        let second = state.begin_cleanup();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.targets.len(), 1);
        assert_eq!(first.deadline, second.deadline);
        assert!(state.reserve().is_err());
    }

    #[test]
    fn cleanup_deadline_is_fixed_at_batch_creation() {
        let state = Arc::new(RunState::new());
        let batch = state.begin_cleanup();
        let before = batch.deadline;
        std::thread::sleep(Duration::from_millis(1));
        let same = state.begin_cleanup();
        assert_eq!(same.deadline, before);
    }
}
