use std;

/// `Error` specifies an error that occurred while managing the task queue.
///
/// For errors that occur while running a task, see `TaskError`.
///
#[derive(Debug)]
pub enum Error {
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
        d.fmt(f)
    }
}
