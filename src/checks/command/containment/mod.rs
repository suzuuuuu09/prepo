use std::pin::Pin;
use std::time::Instant;
use tokio::io::AsyncRead;
use tokio::sync::watch;

pub(super) type Output = Pin<Box<dyn AsyncRead + Send + Unpin>>;

#[derive(Clone, Copy, Debug)]
pub(super) struct ReapResult {
    pub(super) exit_code: Option<i32>,
    pub(super) terminated: bool,
    pub(super) wait_failed: bool,
}

pub(super) struct Spawned {
    pub(super) process: ContainedProcess,
    pub(super) stdout: Option<Output>,
    pub(super) stderr: Option<Output>,
    pub(super) reaped: watch::Receiver<Option<ReapResult>>,
    pub(super) started_at: Instant,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum LaunchError {
    NotFound,
    NotExecutable,
    WorkingDirectoryInvalid,
    ContainmentFailed,
    CleanupFailed,
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(super) use unix::ContainedProcess;
#[cfg(unix)]
pub(super) use unix::launch;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::ContainedProcess;
#[cfg(windows)]
pub(super) use windows::launch;

#[cfg(not(any(unix, windows)))]
mod unsupported {
    use super::*;
    use crate::config::CommandSpec;
    use std::path::Path;

    pub(super) struct ContainedProcess;

    pub(super) fn launch(_root: &Path, _spec: &CommandSpec) -> Result<Spawned, LaunchError> {
        Err(LaunchError::ContainmentFailed)
    }
}
#[cfg(not(any(unix, windows)))]
pub(super) use unsupported::ContainedProcess;
#[cfg(not(any(unix, windows)))]
pub(super) use unsupported::launch;

pub(super) trait ContainmentOps {
    fn request_stop(&self) -> Result<(), ()>;
    fn disarm(&self) -> Result<(), ()>;
    fn confirm_empty(
        &self,
        deadline: Instant,
    ) -> impl std::future::Future<Output = Result<(), ()>> + Send;
}
