use {Error, Runner, Task, TaskQueue};
use std::path::PathBuf;

/// `Builder` constructs a task queue in a step-by-step manner.
pub struct Builder {
    task_queue: TaskQueue,
    default_target: Option<PathBuf>,
}

impl Builder {
    /// Constructs a new builder with an inner task queue that's empty.
    pub fn new() -> Self {
        Builder {
            task_queue: TaskQueue::new(),
            default_target: None,
        }
    }

    /// Adds a task to the builder's inner task queue.
    ///
    /// The first task added to the task queue becomes the default task.
    ///
    pub fn with_task(mut self, task: Task) -> Self {
        if self.default_target.is_none() {
            self.default_target = Some(task.target().to_owned());
        }
        self.task_queue.insert(task);
        self
    }

    /// Begins running the task queue's default task, starting with that task's
    /// dependencies.
    pub fn start(self) -> Result<Runner, Error> {
        Runner::new(self.task_queue, self.default_target)
    }
}
