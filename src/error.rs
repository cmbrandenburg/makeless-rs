use std;
use std::any::Any;
use std::borrow::Cow;

#[derive(Debug)]
pub enum Error {
    #[doc(hidden)]
    RawMessage { message: String },

    RecipeFailure(RecipeFailureDetails),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let d = (self as &std::error::Error).description();
        match *self {
            Error::RawMessage { ref message } => message.fmt(f),
            Error::RecipeFailure(ref details) => write!(f, "{}: {}", d, details),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::RawMessage { ref message } => &message,
            Error::RecipeFailure { .. } => "A recipe failed",
        }
    }
}

#[derive(Debug)]
pub struct RecipeFailureDetails {
    recipe_error: Box<Any>,
    recipe_error_string: Cow<'static, str>,
}

impl RecipeFailureDetails {
    pub fn new_from_error<E: 'static + std::error::Error>(error: E) -> Self {
        RecipeFailureDetails {
            recipe_error_string: Cow::Owned(error.to_string()),
            recipe_error: Box::new(error),
        }
    }

    pub fn new_from_any<E: 'static>(any: E) -> Self {
        RecipeFailureDetails {
            recipe_error: Box::new(any),
            recipe_error_string: Cow::Borrowed("Recipe error information is unavailable"),
        }
    }
}

impl std::fmt::Display for RecipeFailureDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        self.recipe_error_string.fmt(f)
    }
}
