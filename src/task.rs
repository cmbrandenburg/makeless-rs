use {Error, std};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// A dependency chain is a cyclic sequence of target paths. This means that, for
// example, A->B->C->A, B->C->A->B, and C->A->B->C are the same dependency chain.
//
// Invariant: For any dependency sequence (A, B, ..., C), A <= B, ... A <= C.
// This ensures that derived equality-comparison and hashing behave as expected.
//
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DependencyChain<'a>(Vec<&'a Path>);

impl<'a> DependencyChain<'a> {
    fn new<I>(targets: I) -> Self
        where I: IntoIterator<Item = &'a Path>
    {
        let targets = targets.into_iter().collect::<Vec<_>>();
        debug_assert!(!targets.is_empty());

        // Need to find the smallest target value and rotate the vector so that
        // it's the first element. E.g., [C, D, A, B] -> [A, B, C, D].
        let index_smallest = targets
            .iter()
            .enumerate()
            .min_by(|&(_i, &ipath), &(_j, &jpath)| if ipath < jpath {
                        std::cmp::Ordering::Less
                    } else if ipath == jpath {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            })
            .unwrap()
            .0;

        let targets = {
            let mut v = Vec::new();
            v.extend_from_slice(&targets[index_smallest..]);
            v.extend_from_slice(&targets[..index_smallest]);
            v
        };

        DependencyChain(targets)
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct DependencyScan<'a> {
    cyclic_dependencies: HashSet<DependencyChain<'a>>,
    missing_dependencies: HashSet<&'a Path>,
}

/// `TaskSet` contains all tasks in a task queue.
///
/// Applications should not use `TaskSet` directly but instead use `Builder`.
///
#[derive(Debug)]
pub struct TaskSet {
    inner: HashMap<PathBuf, Task>,
}

impl TaskSet {
    pub fn new() -> Self {
        TaskSet { inner: HashMap::new() }
    }

    pub fn insert(&mut self, task: Task) -> Option<Task> {
        self.inner.insert(task.target.clone(), task)
    }

    pub fn get(&self, target: &Path) -> Option<&Task> {
        self.inner.get(target)
    }

    pub fn remove(&mut self, target: &Path) -> Option<Task> {
        self.inner.remove(target)
    }

    pub fn scan_dependencies(&self) -> DependencyScan {

        fn recurse<'a>(dot: &'a Task,
                       task_set: &'a TaskSet,
                       mut dep_path: Vec<&'a Path>,
                       scan: &mut DependencyScan<'a>) {

            // Need to pare dependency cycles to the shortest path containing
            // the cycle. For example, the cycle in A->B->C->B is B->C, not
            // A->B->C.

            let dot_target = &dot.target;
            if let Some((index, _)) = dep_path
                   .iter()
                   .enumerate()
                   .find(|&(_index, &x)| x == dot_target) {
                scan.cyclic_dependencies
                    .insert(DependencyChain::new(dep_path[index..].iter().map(|&x| x)));
                return;
            }

            dep_path.push(dot_target);
            for t in &dot.dependencies {
                match task_set.inner.get(t) {
                    None => {
                        scan.missing_dependencies.insert(t);
                    }
                    Some(next_dot) => {
                        recurse(next_dot, task_set, dep_path.clone(), scan);
                    }
                }
            }
        }

        let mut scan = DependencyScan::default();
        for (_target, task) in &self.inner {
            recurse(task, self, vec![], &mut scan);
        }

        // Need to delete redundant cycles. For example, we can have A->B->C and
        // B->C, in which case we should delete A->B->C.

        scan.cyclic_dependencies = {
            let orig_cycles = scan.cyclic_dependencies
                .iter()
                .map(|&DependencyChain(ref path)| &path[..])
                .collect::<HashSet<_>>();
            let mut unique_cycles = HashSet::new();
            for &path in &orig_cycles {
                let mut nok = false;
                for i in 1..path.len() {
                    if orig_cycles.get(&path[i..]).is_some() {
                        nok = true;
                        break;
                    }
                }
                if !nok {
                    unique_cycles.insert(path.clone());
                }
            }
            unique_cycles
                .iter()
                .map(|&x| DependencyChain(Vec::from(x)))
                .collect::<HashSet<_>>()
        };

        scan
    }

    pub fn all_targets_recursive<I, P>(&self, top_targets: I) -> HashSet<PathBuf>
        where I: IntoIterator<Item = P>,
              P: Into<PathBuf>
    {
        debug_assert_eq!(self.scan_dependencies(), DependencyScan::default());

        let mut bucket = HashSet::new();
        let mut pending = top_targets
            .into_iter()
            .map(|x| x.into())
            .collect::<Vec<_>>();

        while let Some(dot) = pending.pop() {
            let dot_task = self.inner.get(&dot).unwrap();
            pending.extend(dot_task.dependencies.clone());
            bucket.insert(dot);
        }

        bucket
    }
}

/// `Task` specifies a task that runs in the task queue.
///
/// Note: A `Task` is analogous to a “rule” in Make.
///
/// At a minimum, a task comprises a **target**, which is a file system path
/// that (usually) corresponds to the file generated by running the task.
/// “Target” is analogous to the concept of the same name in Make.
///
/// Additionally, a task may have:
///
/// * A **recipe**, which is the code that runs when the task runs. “Recipe” is
///   analogous to the concept of the same name in Make.
///
/// * Zero or more **dependencies**, which are the targets of other tasks that
///   must complete before this task may run. “Dependency” is analogous to the
///   concept of the same name in Make.
///
pub struct Task {
    target: PathBuf,
    recipe: Option<Box<FnMut() -> Result<(), Error> + Send>>,
    dependencies: HashSet<PathBuf>,
    phony: bool,
}

impl Task {
    /// Constructs a new task specifying a target and otherwise having default
    /// or empty properties.
    pub fn new<P: Into<PathBuf>>(target: P) -> Self {
        Task {
            target: target.into(),
            recipe: None,
            dependencies: HashSet::new(),
            phony: false,
        }
    }

    /// Sets the task's recipe.
    pub fn with_recipe<E, F>(mut self, recipe: F) -> Self
        where F: 'static + FnOnce() -> Result<(), E> + Send
    {
        // The "inner closure" is a workaround for dealing with stable Rust's
        // Box+FnOnce limitation, combined with allowing the application to
        // return an std::error::Error to denote failure. Basically, the inner
        // closure promotes the application's FnOnce to a boxed FnMut, and it
        // boxes any error that's returned. It works so long as we don't call
        // the outer closure twice, despite its FnMut implementation.

        let mut inner_closure = Some(recipe);
        let f = move || {
            let f = inner_closure.take().unwrap();
            // FIXME: Propagate error information from task.
            f().map_err(|_any| -> Error { Error::TaskError })
        };
        self.recipe = Some(Box::new(f));
        self
    }

    /// Sets the task's recipe as a external shell command.
    pub fn with_shell_recipe<S: Into<std::ffi::OsString>>(self, shell_command: S) -> Self {

        let shell_command = shell_command.into();

        self.with_recipe(move || {
            let exit_status = std::process::Command::new("sh").arg("-c")
                .arg(&shell_command)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .map_err(|e| {
                             Error::ShellSpawn {
                                 cause: e,
                                 shell_command: shell_command.clone(),
                             }
                         })?;
            if exit_status.success() {
                Ok(())
            } else {
                Err(Error::ShellNonzero {
                        exit_status: exit_status,
                        shell_command: shell_command,
                    })
            }
        })
    }

    /// Sets the task's target as phony.
    pub fn with_phony(mut self, phony: bool) -> Self {
        self.phony = phony;
        self
    }

    /// Adds a dependency to the task.
    pub fn with_dependency<P: Into<PathBuf>>(mut self, dependency: P) -> Self {
        self.dependencies.insert(dependency.into());
        self
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    pub fn dependencies(&self) -> &HashSet<PathBuf> {
        &self.dependencies
    }

    pub fn run(self) -> Result<(), Error> {
        match self.recipe {
            None => Ok(()),
            Some(mut recipe) => {
                // FIXME: Probably the panic information.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || (*recipe)())) {
                    Ok(x) => x,
                    Err(_any) => Err(Error::TaskPanic),
                }
            }
        }
    }
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {

        // Debug is not implemented for the recipe, so we implement a custom
        // Debug.

        // Use destructuring here so that we're alerted via a compiler error
        // whenever a new field is added to the struct.
        #![allow(unused_variables)]
        let &super::Task {
                 ref target,
                 ref recipe,
                 ref dependencies,
                 ref phony,
             } = self;

        #[derive(Debug)]
        struct Task<'a> {
            target: &'a PathBuf,
            dependencies: &'a HashSet<PathBuf>,
            phony: &'a bool,
        }

        let x = Task {
            target: target,
            dependencies: dependencies,
            phony: phony,
        };

        (&x as &std::fmt::Debug).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct TaskSetBuilder(TaskSet);

    impl TaskSetBuilder {
        fn new() -> Self {
            TaskSetBuilder(TaskSet::new())
        }

        fn with_task(mut self, task: Task) -> Self {
            self.0.insert(task);
            self
        }

        fn build(self) -> TaskSet {
            self.0
        }
    }

    #[test]
    fn scan_dependencies() {

        #[derive(Debug)]
        struct Expected<'a>(DependencyScan<'a>);

        impl<'a> Expected<'a> {
            fn new() -> Self {
                Expected(DependencyScan::default())
            }

            fn with_cyclic_dependency<I>(mut self, targets: I) -> Self
                where I: IntoIterator<Item = &'static str>
            {
                let targets = targets.into_iter().map(|x| Path::new(x));
                self.0
                    .cyclic_dependencies
                    .insert(DependencyChain::new(targets));
                self
            }

            fn with_missing_dependency(mut self, target: &'static str) -> Self {
                self.0.missing_dependencies.insert(Path::new(target));
                self
            }
        }

        impl<'a> PartialEq<DependencyScan<'a>> for Expected<'a> {
            fn eq(&self, other: &DependencyScan<'a>) -> bool {
                self.0.eq(other)
            }
        }

        macro_rules! test_case {
            ({$($target:ident -> ($($dep:ident),*)),*} => {$(($($cycle:ident->)*)),*}, {$($missing:ident),*}) => {
                {
                    let task_set =TaskSetBuilder::new()
                        $(.with_task(Task::new(stringify!($target))$(.with_dependency(stringify!($dep)))*))*
                        .build();
                    let expected = Expected::new()
                        $(.with_cyclic_dependency(vec![$(stringify!($cycle)),*]))*
                        $(.with_missing_dependency(stringify!($missing)))*
                        ;
                    let got = task_set.scan_dependencies();
                    assert_eq!(expected, got);
                }
            }
        }

        test_case!({} => {}, {});
        test_case!({A->()} => {}, {});
        test_case!({A->(B), B->()} => {}, {});
        test_case!({A->(B, C), B->(), C->()} => {}, {});
        test_case!({A->(A)} => {(A->)}, {});
        test_case!({A->(B)} => {}, {B});
        test_case!({A->(B, C, D)} => {}, {B, C, D});
        test_case!({A->(B), B->(C)} => {}, {C});
        test_case!({A->(C), B->(C), C->()} => {}, {});
        test_case!({A->(B), B->(C), C->(D), D->()} => {}, {});
        test_case!({A->(B), B->(C), C->(D)} => {}, {D});
        test_case!({A->(), B->()} => {}, {});
        test_case!({A->(B), B->(A)} => {(A->B->)}, {});
        test_case!({A->(B), B->(C), C->(A)} => {(A->B->C->)}, {});

        // Check that redundant dependency chains are eliminated--i.e.,
        // A->B->C-> is redundant with B->C->.
        test_case!({A->(B), B->(C), C->(B)} => {(B->C->)}, {});

        test_case!({A->(B, C), B->(A), C->(A)} => {(A->B->), (A->C->)}, {});
        test_case!({A->(B, C), B->(C), C->(A, B)} => {(A->C->), (B->C->)}, {});
    }

    #[test]
    fn all_targets_recursive() {

        macro_rules! test_case {
            ({$($target:ident -> ($($dep:ident),*)),*}, {$($top:ident),*} => {$($result:ident),*}) => {
                {
                    let task_set =TaskSetBuilder::new()
                        $(.with_task(Task::new(stringify!($target))$(.with_dependency(stringify!($dep)))*))*
                        .build();
                    static RESULTS: &'static [&'static str] = &[$(stringify!($result)),*];
                    let expected = RESULTS
                        .iter()
                        .map(|&x| PathBuf::from(x))
                        .collect::<HashSet<_>>();
                    static TOPS: &'static [&'static str] = &[$(stringify!($top)),*];
                    let top_targets = TOPS
                        .iter()
                        .map(|&x| PathBuf::from(x))
                        .collect::<Vec<_>>();
                    let got = task_set.all_targets_recursive(top_targets);
                    assert_eq!(got, expected);
                }
            }
        }

        test_case!({}, {} => {});
        test_case!({A->()}, {} => {});
        test_case!({A->()}, {A} => {A});
        test_case!({A->(), B->()}, {A} => {A});
        test_case!({A->(), B->()}, {A, B} => {A, B});
        test_case!({A->(B), B->()}, {A} => {A, B});
        test_case!({A->(B, C, D), B->(), C->(), D->()}, {A} => {A, B, C, D});
        test_case!({A->(B, C, D), B->(C), C->(D), D->()}, {A} => {A, B, C, D});
    }

    #[test]
    fn shell_recipe_ok() {
        Task::new("alpha")
            .with_phony(true)
            .with_shell_recipe("true")
            .run()
            .unwrap();
    }

    #[test]
    fn shell_recipe_nok() {
        match Task::new("alpha")
                  .with_phony(true)
                  .with_shell_recipe("false")
                  .run() {
            Err(Error::TaskError) => {}
            x @ _ => panic!("Unexpected result: {:?}", x),
        }
    }
}
