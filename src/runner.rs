use {Error, Task, TaskSet, num_cpus, std};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

/// `Runner` contains the state of a running task queue.
pub struct Runner {
    worker_threads: Vec<std::thread::JoinHandle<()>>,
    shared_state: Arc<SharedState>,
}

struct SharedState {
    sync_state: Mutex<SyncState>,
    worker_wakeup: Condvar,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskState {
    // Task has one or more dependencies that are not complete.
    Waiting,

    // Task is ready to run but is not yet running.
    Pending,

    // Task is currently running.
    Running,

    // Task has finished running.
    Done,
}

#[derive(Debug)]
struct SyncState {
    task_set: TaskSet,

    // Whether any tasks have failed.
    failed: bool,

    // This hash map includes all top targets and all recursive dependencies. It
    // *excludes* tasks that are neither a top target nor a recursive dependency
    // of a top target.
    target_states: HashMap<PathBuf, TaskState>,
}

impl Runner {
    pub fn new<I, P>(task_set: TaskSet, top_targets: I) -> Result<Self, Error>
        where I: IntoIterator<Item = P>,
              P: Into<PathBuf>
    {
        let target_states = task_set.all_targets_recursive(top_targets.into_iter().map(|x| x.into()))
            .into_iter()
            .map(|target| {
                if task_set.get(&target).unwrap().dependencies().is_empty() {
                    (target, TaskState::Pending)
                } else {
                    (target, TaskState::Waiting)
                }
            })
            .collect::<HashMap<_, _>>();

        let shared_state = Arc::new(SharedState {
            sync_state: Mutex::new(SyncState {
                task_set: task_set,
                failed: false,
                target_states: target_states,
            }),
            worker_wakeup: Condvar::new(),
        });

        let mut worker_threads = Vec::new();
        for _ in 0..num_cpus::get() {
            let shared_state = shared_state.clone();
            worker_threads.push(std::thread::spawn(move || {

                // FIXME: Currently, it's possible for worker threads to hang if
                // a worker thread panics. I'm not entirely sure how to fix
                // this, though the `wait` method should return an Error, and
                // maybe the onus is on the application to make things right? In
                // any case, we must catch panics that occur in the task itself.

                let mut task: Option<Task> = None;
                loop {
                    let task_target = task.as_ref().map(|task| task.target().to_owned());
                    let task_result = task.map(|task| task.run());
                    if let Some(Err(_ignore_task_result)) = task_result {
                        let mut sync_state = shared_state.sync_state.lock().unwrap();
                        sync_state.failed = true;
                        shared_state.worker_wakeup.notify_all();
                        return; // a task failed in this worker
                    }

                    let mut sync_state = shared_state.sync_state.lock().unwrap();

                    if let Some(Ok(..)) = task_result {
                        let task_target = task_target.unwrap();
                        debug_assert_eq!(sync_state.target_states.get(&task_target),
                                         Some(&TaskState::Running));
                        sync_state.target_states.insert(task_target, TaskState::Done);
                        shared_state.worker_wakeup.notify_all();
                    }

                    loop {
                        if sync_state.failed {
                            return; // a task in another worker failed--stop working
                        }
                        if sync_state.target_states.iter().all(|(_, &state)| state == TaskState::Done) {
                            return; // no more work to do
                        }
                        if let Some((target, _)) =
                            sync_state.target_states
                                .iter()
                                .find(|&(_target, &state)| state == TaskState::Pending)
                                .map(|(target, state)| (target.clone(), state)) {
                            task = sync_state.task_set.remove(&target);
                            debug_assert!(task.is_some());
                            debug_assert_eq!(sync_state.target_states.get(&target),
                                             Some(&TaskState::Pending));
                            sync_state.target_states.insert(target, TaskState::Running);
                            break;
                        }
                        sync_state = shared_state.worker_wakeup.wait(sync_state).unwrap();
                    }
                }
            }));
        }

        Ok(Runner {
            worker_threads: worker_threads,
            shared_state: shared_state,
        })
    }

    pub fn join(mut self) -> Result<(), Error> {
        while let Some(w) = self.worker_threads.pop() {
            w.join()
                .map_err(|_| {
                    // FIXME: Propagate as much of the panic information as possible.
                    Error::WorkerPanic
                })?;
        }

        let sync_state = self.shared_state.sync_state.lock().unwrap();
        if sync_state.failed {
            return Err(Error::TaskFailed);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {Task, TaskSet, std};
    use std::path::Path;
    use std::sync::Arc;
    use super::*;

    #[test]
    fn run_empty_task_queue() {
        Runner::new(TaskSet::new(), std::iter::empty::<&Path>())
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn run_no_tasks_for_nonempty_task_queue() {
        let mut task_set = TaskSet::new();
        task_set.insert(Task::new("alpha").with_phony(true));
        Runner::new(task_set, std::iter::empty::<&Path>())
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn run_one_task() {
        let c = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut task_set = TaskSet::new();
        {
            let c = c.clone();
            task_set.insert(Task::new("alpha").with_phony(true).with_recipe(move || -> Result<(), ()> {
                c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            }));
        }
        Runner::new(task_set, std::iter::once(Path::new("alpha")))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(c.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn run_task_that_returns_error() {
        let mut task_set = TaskSet::new();
        task_set.insert(Task::new("alpha")
            .with_phony(true)
            .with_recipe(|| -> Result<(), &'static str> { Err("blah blah blah") }));
        match Runner::new(task_set, std::iter::once(Path::new("alpha")))
            .unwrap()
            .join() {
            Err(Error::TaskFailed) => {}
            x @ _ => panic!("Unexpected result {:?}", x),
        }
    }

    #[test]
    fn run_task_that_panics() {
        let mut task_set = TaskSet::new();
        task_set.insert(Task::new("alpha").with_phony(true).with_recipe(|| -> Result<(), ()> {
            panic!("blah blah blah");
        }));
        match Runner::new(task_set, std::iter::once(Path::new("alpha")))
            .unwrap()
            .join() {
            Err(Error::TaskFailed) => {}
            x @ _ => panic!("Unexpected result {:?}", x),
        }
    }
}
