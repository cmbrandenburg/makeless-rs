//! Makeless provides a declarative, dependency-based task queue, where the
//! tasks run in parallel and dependencies are file system paths whose
//! modification timestamp is used to determine whether a task is up-to-date and
//! can be skipped. In short, Makeless is directly inspired by Make, but where
//! the programmer uses Rust instead of Makefiles.
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
//! TODO: Example

extern crate num_cpus;

mod builder;
mod error;
mod task;
mod runner;

pub use builder::Builder;
pub use error::Error;
pub use runner::Runner;
pub use task::{Task, TaskSet};
