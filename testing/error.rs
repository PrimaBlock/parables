use failure;
use std::result;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "no object item to link named `{}`", item)]
    NoLinkerItem { item: String },
    #[fail(display = "no object path to link named `{}`", path)]
    NoLinkerPath { path: String },
}

/// An error occurred during a call.
#[derive(Debug, Fail)]
pub enum CallError {
    #[fail(display = "call was reverted")]
    Reverted,
    #[fail(display = "call error: {}", message)]
    Other { message: String },
}

impl From<&'static str> for CallError {
    fn from(value: &'static str) -> Self {
        CallError::Other {
            message: value.to_string(),
        }
    }
}

impl From<String> for CallError {
    fn from(value: String) -> Self {
        CallError::Other { message: value }
    }
}

pub type Result<T> = ::std::result::Result<T, failure::Error>;

pub trait ResultExt {
    /// Check if the result is errored because of an revert.
    fn is_reverted(&self) -> bool {
        false
    }
}

impl<T> ResultExt for result::Result<T, CallError> {
    fn is_reverted(&self) -> bool {
        match *self {
            Err(CallError::Reverted) => true,
            _ => false,
        }
    }
}
