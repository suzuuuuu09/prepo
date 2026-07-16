use super::{ContainmentOps, LaunchError, Output, ReapResult, Spawned};
use crate::config::CommandSpec;
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::time::{Duration, Instant};
use sysinfo::{ProcessStatus, ProcessesToUpdate, System};
use tokio::sync::watch;

pub(crate) struct ContainedProcess {
    group: i32,
}

pub(crate) fn launch(root: &Path, spec: &CommandSpec) -> Result<Spawned, LaunchError> {
    let mut command = tokio::process::Command::new(&spec.command[0]);
    command
        .args(&spec.command[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(root.join(spec.working_directory.as_deref().unwrap_or(".")))
        .envs(&spec.environment_overrides);
    command.as_std_mut().process_group(0);
    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => LaunchError::NotFound,
        std::io::ErrorKind::PermissionDenied => LaunchError::NotExecutable,
        _ => LaunchError::WorkingDirectoryInvalid,
    })?;
    let started_at = Instant::now();
    let group = child.id().ok_or(LaunchError::ContainmentFailed)?;
    let group = i32::try_from(group).map_err(|_| LaunchError::ContainmentFailed)?;
    let stdout = child.stdout.take().map(|stream| Box::pin(stream) as Output);
    let stderr = child.stderr.take().map(|stream| Box::pin(stream) as Output);
    let (sender, receiver) = watch::channel(None);
    tokio::spawn(async move {
        let result = match child.wait().await {
            Ok(status) => ReapResult {
                exit_code: status.code(),
                terminated: !status.success() && status.code().is_none(),
                wait_failed: false,
            },
            Err(_) => ReapResult {
                exit_code: None,
                terminated: false,
                wait_failed: true,
            },
        };
        let _ = sender.send(Some(result));
    });
    Ok(Spawned {
        process: ContainedProcess { group },
        stdout,
        stderr,
        reaped: receiver,
        started_at,
    })
}

impl ContainmentOps for ContainedProcess {
    fn request_stop(&self) -> Result<(), ()> {
        match killpg(Pid::from_raw(self.group), Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(_) => Err(()),
        }
    }

    fn disarm(&self) -> Result<(), ()> {
        Ok(())
    }

    async fn confirm_empty(&self, deadline: Instant) -> Result<(), ()> {
        let mut system = System::new();
        loop {
            system.refresh_processes(ProcessesToUpdate::All, true);
            let mut enumeration_failed = false;
            let running = system.processes().iter().any(|(pid, process)| {
                if process.status() == ProcessStatus::Zombie {
                    return false;
                }
                let Ok(raw_pid) = i32::try_from(pid.as_u32()) else {
                    enumeration_failed = true;
                    return false;
                };
                let group = match nix::unistd::getpgid(Some(Pid::from_raw(raw_pid))) {
                    Ok(group) => group,
                    Err(Errno::ESRCH) => return false,
                    Err(_) => {
                        enumeration_failed = true;
                        return false;
                    }
                };
                group.as_raw() == self.group
            });
            if enumeration_failed {
                return Err(());
            }
            if !running {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}
