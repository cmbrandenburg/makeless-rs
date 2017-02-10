use std;

/// `Error` specifies an error that occurred while managing the task queue.
///
/// For errors that occur while running a task, see `TaskError`.
///
#[derive(Debug)]
pub enum Error {
    #[doc(hidden)]
    ShellNonzero {
        shell_command: std::ffi::OsString,
        exit_status: std::process::ExitStatus,
    },

    #[doc(hidden)]
    ShellSpawn {
        shell_command: std::ffi::OsString,
        cause: std::io::Error,
    },

    #[doc(hidden)]
    TaskError,

    /// One or more tasks failed.
    TaskFailed,

    #[doc(hidden)]
    TaskPanic,

    /// A worker thread panicked.
    WorkerPanic,
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            &Error::ShellNonzero { .. } => "A shell command exited with nonzero status",
            &Error::ShellSpawn { .. } => "A shell command failed to spawn",
            &Error::TaskError => "A task returned an error",
            &Error::TaskFailed => "One or more tasks failed",
            &Error::TaskPanic => "A task panicked",
            &Error::WorkerPanic => "A worker thread panicked",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let d = (self as &std::error::Error).description();
        match self {
            &Error::ShellNonzero { ref shell_command, ref exit_status } => {
                write!(f,
                       "{} (command: {}, exit_status: {}",
                       d,
                       shell_command.to_str().unwrap_or("???"),
                       exit_status)
            }
            &Error::ShellSpawn { ref shell_command, ref cause } => {
                write!(f,
                       "{} (command: {}): {}",
                       d,
                       shell_command.to_str().unwrap_or("???"),
                       cause)
            }
            _ => d.fmt(f),
        }
    }
}
