use proptest;
use std::result;

#[derive(Debug, PartialEq, Eq, Fail)]
pub enum Error {
    #[fail(display = "no object item to link named `{}`", item)]
    NoLinkerItem { item: String },
    #[fail(display = "no object path to link named `{}`", path)]
    NoLinkerPath { path: String },
    #[fail(display = "bad input at position `{}`: {}", position, message)]
    BadInputPos {
        position: usize,
        message: &'static str,
    },
    #[fail(display = "call error: {}", error)]
    Call { error: CallError },
    #[fail(display = "error: {}", message)]
    Other { message: String },
}

impl From<&'static str> for Error {
    fn from(value: &'static str) -> Self {
        Error::Other {
            message: value.to_string(),
        }
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Error::Other { message: value }
    }
}

/// Error when we fail to decode input.
pub struct DecodingError;

impl From<DecodingError> for Error {
    fn from(_: DecodingError) -> Self {
        Error::Other {
            message: "failed to decode input".to_string(),
        }
    }
}

impl From<CallError> for Error {
    fn from(error: CallError) -> Self {
        Error::Call { error }
    }
}

impl From<Error> for proptest::test_runner::TestCaseError {
    fn from(error: Error) -> proptest::test_runner::TestCaseError {
        proptest::test_runner::TestCaseError::Fail(error.to_string().into())
    }
}

/// An error occurred during a call.
#[derive(Debug, PartialEq, Eq, Fail)]
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

impl From<CallError> for proptest::test_runner::TestCaseError {
    fn from(error: CallError) -> proptest::test_runner::TestCaseError {
        proptest::test_runner::TestCaseError::Fail(error.to_string().into())
    }
}

/// Error when we fail to build a transaction nonce.
pub struct NonceError;

impl From<NonceError> for CallError {
    fn from(_: NonceError) -> Self {
        CallError::Other {
            message: "failed to construct nonce".to_string(),
        }
    }
}

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
