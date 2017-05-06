extern crate makeless;

use makeless::Rule;
use std::sync::Arc;

#[test]
fn run_a_rule_with_no_recipe() {
    Rule::new("foo").run().unwrap();
}

#[test]
fn run_a_rule_with_a_recipe() {

    let c = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut r = Rule::new("foo");
    let c_clone = c.clone();
    r.set_recipe_of_any_error(move || -> Result<(), ()> {
                                  c_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                  Ok(())
                              });
    r.run().unwrap();

    assert_eq!(c.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn run_a_rule_that_returns_an_error() {

    let c = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut r = Rule::new("foo");
    let c_clone = c.clone();
    r.set_recipe_of_any_error(move || -> Result<(), ()> {
                                  c_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                  Err(())
                              });

    match r.run() {
        Err(makeless::Error::RecipeFailure(..)) => (),
        x @ _ => panic!("Got unexpected result: {:?}", x),
    }

    // Even though the recipe failed, verify that it ran.

    assert_eq!(c.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn run_a_rule_that_returns_a_proper_error() {

    #[derive(Debug)]
    struct ErrorType(String);

    impl std::fmt::Display for ErrorType {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
            self.0.fmt(f)
        }
    }

    impl std::error::Error for ErrorType {
        fn description(&self) -> &str {
            &self.0
        }
    }

    let c = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut r = Rule::new("foo");
    let c_clone = c.clone();
    r.set_recipe(move || -> Result<(), ErrorType> {
                     c_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                     Err(ErrorType(String::from("blah blah blah")))
                 });

    match r.run() {
        Err(ref e @ makeless::Error::RecipeFailure(..)) if e.to_string().contains("blah blah blah") => (),
        x @ _ => panic!("Got unexpected result: {:?}", x),
    }

    // Even though the recipe failed, verify that it ran.

    assert_eq!(c.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn run_a_rule_that_runs_a_dependency() {

    let c = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut r1 = Rule::new("foo");
    let c_clone = c.clone();
    r1.set_recipe_of_any_error(move || -> Result<(), ()> {
                                   c_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                   Ok(())
                               });

    let mut r2 = Rule::new("bar");
    let c_clone = c.clone();
    r2.set_recipe_of_any_error(move || -> Result<(), ()> {
                                   c_clone.fetch_add(2, std::sync::atomic::Ordering::Relaxed);
                                   Ok(())
                               });

    r1.insert_dependency(&mut r2).unwrap();
    r1.run().unwrap();

    assert_eq!(c.load(std::sync::atomic::Ordering::Relaxed), 3);
}
