use std;

/// `Error` contains information for error that occurred.
#[derive(Debug)]
pub enum Error {}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        unimplemented!();
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, _f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        unimplemented!();
    }
}
