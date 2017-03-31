//! Makeless provides a declarative, dependency-based task queue, where the
//! tasks run in parallel and dependencies are file system paths whose
//! modification timestamp is used to determine whether a task is up-to-date and
//! can be skipped. In short, Makeless is directly inspired by Make, but where
//! the programmer programs in Rust, not the Make language.
//!
//! # Makeless is a library, not a language
//!
//! Makeless is **not** a scripting language, nor is Makeless meant to replace
//! Make. Instead, Makeless offers an alternative to Make for problems where a
//! solution is elegantly expressed as a declarative set of concurrent tasks
//! with file-based dependencies—i.e., a Makefile. Without Makeless, the
//! programmer might resort to programmatically generating a Makefile—and
//! dealing with Make's numerous quirks—then shelling out to run the Makefile
//! via `make`. Whereas, with Makeless, one may stay in Rust while still
//! benefiting from the power of using declarative, file-based dependencies.
//!
//! # Declarative vs procedural
//!
//! Like Make, Makeless is declarative, not procedural. This means, generally
//! speaking, the programmer specifies what needs to be done and Makeless
//! figures out how to make it so.
//!
//! # Example
//!
//! ```rust
//! extern crate makeless;
//! extern crate tempdir;
//!
//! let tmp = tempdir::TempDir::new("makeless-example").unwrap();
//!
//! makeless::Builder::new()
//!     .with_task(makeless::Task::new(tmp.path().join("foo"))
//!         .with_phony(true)
//!         .with_shell_recipe("true"))
//!     .start()
//!     .expect("Could not start task queue")
//!     .join()
//!     .expect("One or more tasks failed or worker thread panicked");
//! ```

// FIXME: Need to check file timestamps to determine whether a target is already
// up-to-date.

// FIXME: Should change target type from string to something strongly typed. The
// goal is to prevent 'missing dependency' errors by misuse in the application.

extern crate num_cpus;

mod builder;
mod error;
mod runner;
mod task;

pub use builder::Builder;
pub use error::Error;
pub use runner::Runner;
pub use task::{Task, TaskQueue};
