use super::{ContainmentOps, LaunchError, Output, ReapResult, Spawned};
use crate::config::CommandSpec;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, IntoRawHandle, RawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess, INFINITE,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess,
    WaitForSingleObject,
};

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_BAD_EXE_FORMAT: u32 = 193;

pub(crate) struct ContainedProcess {
    job: usize,
}

pub(crate) fn launch(root: &Path, spec: &CommandSpec) -> Result<Spawned, LaunchError> {
    let working_directory = root.join(spec.working_directory.as_deref().unwrap_or("."));
    if !working_directory.is_dir() {
        return Err(LaunchError::WorkingDirectoryInvalid);
    }

    let job = create_job().ok_or(LaunchError::ContainmentFailed)?;
    let mut pipes = ChildPipes::new().map_err(|_| {
        close_handle(job);
        LaunchError::ContainmentFailed
    })?;
    let stdin = std::fs::File::open("NUL").map_err(|_| {
        pipes.close_all();
        close_handle(job);
        LaunchError::ContainmentFailed
    })?;
    let stdin_handle = stdin.into_raw_handle() as HANDLE;
    if unsafe { SetHandleInformation(stdin_handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0
    {
        pipes.close_all();
        close_handle(stdin_handle);
        close_handle(job);
        return Err(LaunchError::ContainmentFailed);
    }

    let mut command_line = command_line(&spec.command);
    let environment = environment_block(&spec.environment_overrides);
    let current_directory = wide_path(&working_directory);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESTDHANDLES,
        hStdInput: stdin_handle,
        hStdOutput: pipes.stdout_write,
        hStdError: pipes.stderr_write,
        ..Default::default()
    };
    let mut process_information = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            null(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr() as *const _,
            current_directory.as_ptr(),
            &startup,
            &mut process_information,
        )
    } != 0;
    let create_error = if created {
        None
    } else {
        Some(unsafe { GetLastError() })
    };
    close_handle(stdin_handle);
    close_handle(pipes.stdout_write);
    close_handle(pipes.stderr_write);
    pipes.stdout_write = null_mut();
    pipes.stderr_write = null_mut();
    if let Some(error) = create_error {
        pipes.close_reads();
        close_handle(job);
        return Err(map_create_error(error));
    }

    let process = process_information.hProcess;
    let thread = process_information.hThread;
    if let Err(error) = establish_suspended_containment(
        || unsafe { AssignProcessToJobObject(job, process) != 0 },
        || unsafe { ResumeThread(thread) != u32::MAX },
        || terminate_unstarted_process(job, process, thread),
    ) {
        pipes.close_reads();
        close_handle(job);
        return Err(error);
    }
    let started_at = Instant::now();
    close_handle(thread);

    let stdout = pipes.take_stdout();
    let stderr = pipes.take_stderr();
    let (sender, receiver) = watch::channel(None);
    let process_value = process as usize;
    tokio::task::spawn_blocking(move || {
        let process = process_value as HANDLE;
        let result = unsafe {
            let waited = WaitForSingleObject(process, INFINITE) == WAIT_OBJECT_0;
            if !waited {
                ReapResult {
                    exit_code: None,
                    terminated: false,
                    wait_failed: true,
                }
            } else {
                let mut code = 0;
                if GetExitCodeProcess(process, &mut code) == 0 {
                    ReapResult {
                        exit_code: None,
                        terminated: false,
                        wait_failed: true,
                    }
                } else {
                    ReapResult {
                        exit_code: i32::try_from(code).ok(),
                        // A completed Windows process does not expose a portable
                        // signal-vs-exit distinction through GetExitCodeProcess.
                        // The lifecycle therefore treats its exit code as the
                        // objective result and reserves `terminated` for Unix
                        // signal termination.
                        terminated: false,
                        wait_failed: false,
                    }
                }
            }
        };
        close_handle(process);
        let _ = sender.send(Some(result));
    });

    Ok(Spawned {
        process: ContainedProcess { job: job as usize },
        stdout,
        stderr,
        reaped: receiver,
        started_at,
    })
}

fn establish_suspended_containment(
    assign: impl FnOnce() -> bool,
    resume: impl FnOnce() -> bool,
    cleanup: impl FnOnce() -> bool,
) -> Result<(), LaunchError> {
    if !assign() {
        return Err(if cleanup() {
            LaunchError::ContainmentFailed
        } else {
            LaunchError::CleanupFailed
        });
    }
    if !resume() {
        return Err(if cleanup() {
            LaunchError::ContainmentFailed
        } else {
            LaunchError::CleanupFailed
        });
    }
    Ok(())
}

fn create_job() -> Option<HANDLE> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return None;
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        )
    } != 0;
    if configured {
        Some(job)
    } else {
        close_handle(job);
        None
    }
}

fn map_create_error(error: u32) -> LaunchError {
    match error {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => LaunchError::NotFound,
        ERROR_ACCESS_DENIED | ERROR_BAD_EXE_FORMAT => LaunchError::NotExecutable,
        _ => LaunchError::RuntimeFailed,
    }
}

fn terminate_unstarted_process(job: HANDLE, process: HANDLE, thread: HANDLE) -> bool {
    let terminated = unsafe {
        let job_terminated = TerminateJobObject(job, 1) != 0;
        let process_terminated = TerminateProcess(process, 1) != 0;
        let waited = WaitForSingleObject(process, 5_000) == WAIT_OBJECT_0;
        (job_terminated || process_terminated) && waited
    };
    close_handle(thread);
    close_handle(process);
    terminated
}

struct ChildPipes {
    stdout_read: HANDLE,
    stdout_write: HANDLE,
    stderr_read: HANDLE,
    stderr_write: HANDLE,
}

impl ChildPipes {
    fn new() -> Result<Self, ()> {
        let mut stdout_read = null_mut();
        let mut stdout_write = null_mut();
        let mut stderr_read = null_mut();
        let mut stderr_write = null_mut();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        if unsafe { CreatePipe(&mut stdout_read, &mut stdout_write, &attributes, 0) } == 0 {
            return Err(());
        }
        if unsafe { SetHandleInformation(stdout_read, HANDLE_FLAG_INHERIT, 0) } == 0 {
            close_handle(stdout_read);
            close_handle(stdout_write);
            return Err(());
        }
        if unsafe { CreatePipe(&mut stderr_read, &mut stderr_write, &attributes, 0) } == 0 {
            close_handle(stdout_read);
            close_handle(stdout_write);
            return Err(());
        }
        if unsafe { SetHandleInformation(stderr_read, HANDLE_FLAG_INHERIT, 0) } == 0 {
            close_handle(stdout_read);
            close_handle(stdout_write);
            close_handle(stderr_read);
            close_handle(stderr_write);
            return Err(());
        }
        Ok(Self {
            stdout_read,
            stdout_write,
            stderr_read,
            stderr_write,
        })
    }

    fn close_reads(&mut self) {
        close_handle(self.stdout_read);
        close_handle(self.stderr_read);
        self.stdout_read = null_mut();
        self.stderr_read = null_mut();
    }

    fn close_all(&mut self) {
        self.close_reads();
        close_handle(self.stdout_write);
        close_handle(self.stderr_write);
        self.stdout_write = null_mut();
        self.stderr_write = null_mut();
    }

    fn take_stdout(&mut self) -> Option<Output> {
        let handle = std::mem::replace(&mut self.stdout_read, null_mut());
        if handle.is_null() {
            None
        } else {
            let file = unsafe { std::fs::File::from_raw_handle(handle as RawHandle) };
            Some(Box::pin(tokio::fs::File::from_std(file)) as Output)
        }
    }

    fn take_stderr(&mut self) -> Option<Output> {
        let handle = std::mem::replace(&mut self.stderr_read, null_mut());
        if handle.is_null() {
            None
        } else {
            let file = unsafe { std::fs::File::from_raw_handle(handle as RawHandle) };
            Some(Box::pin(tokio::fs::File::from_std(file)) as Output)
        }
    }
}

impl Drop for ChildPipes {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn close_handle(handle: HANDLE) {
    if !handle.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
    value.push(0);
    value
}

fn command_line(arguments: &[String]) -> Vec<u16> {
    let text = arguments
        .iter()
        .map(|argument| quote_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn quote_argument(argument: &str) -> String {
    let mut result = String::with_capacity(argument.len() + 2);
    result.push('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                result.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            }
            _ => {
                result.extend(std::iter::repeat_n('\\', backslashes));
                result.push(character);
                backslashes = 0;
            }
        }
    }
    result.extend(std::iter::repeat_n('\\', backslashes * 2));
    result.push('"');
    result
}

fn environment_block(overrides: &std::collections::BTreeMap<String, String>) -> Vec<u16> {
    let mut entries = std::env::vars_os().collect::<Vec<(OsString, OsString)>>();
    for (key, value) in overrides {
        if let Some((_, existing)) = entries
            .iter_mut()
            .find(|(existing, _)| existing.to_string_lossy().eq_ignore_ascii_case(key))
        {
            *existing = OsString::from(value);
        } else {
            entries.push((OsString::from(key), OsString::from(value)));
        }
    }
    entries.sort_by(|left, right| {
        left.0
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.0.to_string_lossy().to_ascii_lowercase())
    });
    let mut block = Vec::new();
    for (key, value) in entries {
        block.extend(key.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

impl ContainmentOps for ContainedProcess {
    fn request_stop(&self) -> Result<(), ()> {
        let job = self.job as HANDLE;
        let terminated = unsafe { TerminateJobObject(job, 1) } != 0;
        if terminated || job_is_empty(job) {
            Ok(())
        } else {
            Err(())
        }
    }

    fn disarm(&self) -> Result<(), ()> {
        let job = self.job as HANDLE;
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let updated = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
        } != 0;
        if updated { Ok(()) } else { Err(()) }
    }

    async fn confirm_empty(&self, deadline: Instant) -> Result<(), ()> {
        loop {
            if job_is_empty(self.job as HANDLE) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

fn job_is_empty(job: HANDLE) -> bool {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as *mut _,
            std::mem::size_of_val(&accounting) as u32,
            null_mut(),
        ) != 0
            && accounting.ActiveProcesses == 0
    }
}

impl Drop for ContainedProcess {
    fn drop(&mut self) {
        close_handle(self.job as HANDLE);
    }
}

#[cfg(test)]
mod tests {
    use super::{LaunchError, establish_suspended_containment, map_create_error};

    #[test]
    fn assignment_precedes_resume_and_failure_does_not_resume() {
        let events = std::cell::RefCell::new(Vec::new());
        let result = establish_suspended_containment(
            || {
                events.borrow_mut().push("assign");
                false
            },
            || {
                events.borrow_mut().push("resume");
                true
            },
            || {
                events.borrow_mut().push("cleanup");
                true
            },
        );

        assert!(matches!(result, Err(LaunchError::ContainmentFailed)));
        assert_eq!(events.into_inner(), ["assign", "cleanup"]);
    }

    #[test]
    fn resume_failure_cleans_up_without_reporting_success() {
        let events = std::cell::RefCell::new(Vec::new());
        let result = establish_suspended_containment(
            || {
                events.borrow_mut().push("assign");
                true
            },
            || {
                events.borrow_mut().push("resume");
                false
            },
            || {
                events.borrow_mut().push("cleanup");
                false
            },
        );

        assert!(matches!(result, Err(LaunchError::CleanupFailed)));
        assert_eq!(events.into_inner(), ["assign", "resume", "cleanup"]);
    }

    #[test]
    fn unknown_create_errors_are_runtime_failures() {
        assert!(matches!(map_create_error(1234), LaunchError::RuntimeFailed));
    }
}
