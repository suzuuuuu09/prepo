use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    Error,
    PermissionDenied,
    NotApplicable,
    Blocked,
}

macro_rules! reason_codes {
    ($($variant:ident => $wire:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        pub enum Reason {
            $(#[serde(rename = $wire)] $variant),+
        }

        impl Reason {
            pub(crate) fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }
    };
}

reason_codes! {
    PlatformNotApplicable => "platform_not_applicable",
    DependencyNotPassed => "dependency_not_passed",
    PermissionMissing => "permission_missing",
    EnvironmentVariableMissing => "environment_variable_missing",
    EnvironmentVariablePresent => "environment_variable_present",
    EnvironmentValueMismatch => "environment_value_mismatch",
    EnvironmentValueForbidden => "environment_value_forbidden",
    EnvironmentSubstringMissing => "environment_substring_missing",
    EnvironmentPatternMismatch => "environment_pattern_mismatch",
    NonUnicodeEnvironmentValue => "non_unicode_environment_value",
    EnvironmentReadFailed => "environment_read_failed",
    PathMissing => "path_missing",
    PathPresent => "path_present",
    PathKindMismatch => "path_kind_mismatch",
    BrokenSymlink => "broken_symlink",
    PathMetadataFailed => "path_metadata_failed",
    SymlinkLoop => "symlink_loop",
    CommandNotFound => "command_not_found",
    CommandNotExecutable => "command_not_executable",
    WorkingDirectoryInvalid => "working_directory_invalid",
    TimedOut => "timed_out",
    CommandTerminated => "command_terminated",
    ExitCodeMismatch => "exit_code_mismatch",
    StdoutConditionMismatch => "stdout_condition_mismatch",
    StderrConditionMismatch => "stderr_condition_mismatch",
    CommandContainmentFailed => "command_containment_failed",
    CommandCleanupFailed => "command_cleanup_failed",
    CommandOutputReadFailed => "command_output_read_failed",
    CommandRuntimeFailed => "command_runtime_failed",
    ObservationLimitExceeded => "observation_limit_exceeded",
    ProcessNotRunning => "process_not_running",
    ProcessUnexpectedlyRunning => "process_unexpectedly_running",
    ProcessEnumerationFailed => "process_enumeration_failed",
    PortNotOpen => "port_not_open",
    PortNotClosed => "port_not_closed",
    PortUnavailable => "port_unavailable",
    PortBindFailed => "port_bind_failed",
    PortConnectFailed => "port_connect_failed",
    PortNameResolutionFailed => "port_name_resolution_failed",
    ConnectionStateIndeterminate => "connection_state_indeterminate",
    DnsResolutionFailed => "dns_resolution_failed",
    HttpConnectionFailed => "http_connection_failed",
    TlsValidationFailed => "tls_validation_failed",
    HttpStatusMismatch => "http_status_mismatch",
    HttpBodyMismatch => "http_body_mismatch",
    MissingEnvironmentVariable => "missing_environment_variable",
    HttpClientBuildFailed => "http_client_build_failed",
    HttpBodyReadFailed => "http_body_read_failed",
    BodyDecodeFailed => "body_decode_failed",
    UnsupportedContentEncoding => "unsupported_content_encoding",
    InvalidExpandedHeaderValue => "invalid_expanded_header_value",
    ExpandedHeaderTooLarge => "expanded_header_too_large",
    HttpRuntimeFailed => "http_runtime_failed",
}

macro_rules! categorized_reasons {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[allow(dead_code)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub(crate) const fn as_reason(self) -> Reason {
                match self {
                    $(Self::$variant => Reason::$variant),+
                }
            }
        }
    };
}

categorized_reasons!(FailureReason {
    EnvironmentVariableMissing,
    EnvironmentVariablePresent,
    EnvironmentValueMismatch,
    EnvironmentValueForbidden,
    EnvironmentSubstringMissing,
    EnvironmentPatternMismatch,
    PathMissing,
    PathPresent,
    PathKindMismatch,
    BrokenSymlink,
    CommandNotFound,
    CommandNotExecutable,
    WorkingDirectoryInvalid,
    TimedOut,
    CommandTerminated,
    ExitCodeMismatch,
    StdoutConditionMismatch,
    StderrConditionMismatch,
    ProcessNotRunning,
    ProcessUnexpectedlyRunning,
    PortNotOpen,
    PortNotClosed,
    PortUnavailable,
    DnsResolutionFailed,
    HttpConnectionFailed,
    TlsValidationFailed,
    HttpStatusMismatch,
    HttpBodyMismatch,
    MissingEnvironmentVariable,
});

categorized_reasons!(ErrorReason {
    NonUnicodeEnvironmentValue,
    EnvironmentReadFailed,
    PathMetadataFailed,
    SymlinkLoop,
    CommandContainmentFailed,
    CommandCleanupFailed,
    CommandOutputReadFailed,
    CommandRuntimeFailed,
    ObservationLimitExceeded,
    ProcessEnumerationFailed,
    PortBindFailed,
    PortConnectFailed,
    PortNameResolutionFailed,
    ConnectionStateIndeterminate,
    HttpClientBuildFailed,
    HttpBodyReadFailed,
    BodyDecodeFailed,
    UnsupportedContentEncoding,
    InvalidExpandedHeaderValue,
    ExpandedHeaderTooLarge,
    HttpRuntimeFailed,
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionResult(ExecutionState);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionState {
    Passed,
    Failed(FailureReason),
    Error(ErrorReason),
}

impl ExecutionResult {
    pub(crate) const fn passed() -> Self {
        Self(ExecutionState::Passed)
    }

    pub(crate) const fn failed(reason: FailureReason) -> Self {
        Self(ExecutionState::Failed(reason))
    }

    pub(crate) const fn error(reason: ErrorReason) -> Self {
        Self(ExecutionState::Error(reason))
    }

    pub(crate) const fn outcome(self) -> Outcome {
        match self.0 {
            ExecutionState::Passed => Outcome::Passed,
            ExecutionState::Failed(_) => Outcome::Failed,
            ExecutionState::Error(_) => Outcome::Error,
        }
    }

    pub(crate) fn reason(self) -> Option<Reason> {
        match self.0 {
            ExecutionState::Passed => None,
            ExecutionState::Failed(reason) => Some(reason.as_reason()),
            ExecutionState::Error(reason) => Some(reason.as_reason()),
        }
    }
}
