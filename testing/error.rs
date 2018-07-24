use proptest;
use std::borrow::Cow;
use std::fmt;

#[derive(Debug, PartialEq, Fail)]
pub enum Error {
    #[fail(display = "no object item to link named `{}`", item)]
    NoLinkerItem { item: String },
    #[fail(display = "no object path to link named `{}`", path)]
    NoLinkerPath { path: String },
    #[fail(display = "bad input at position `{}`: {}", position, message)]
    BadInputPos {
        position: usize,
        message: Cow<'static, str>,
    },
    #[fail(display = "call error: {}", message)]
    Call { message: String },
    #[fail(display = "{}", message)]
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

impl From<fmt::Error> for Error {
    fn from(error: fmt::Error) -> Self {
        Error::Other {
            message: error.to_string(),
        }
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

impl From<Error> for proptest::test_runner::TestCaseError {
    fn from(error: Error) -> proptest::test_runner::TestCaseError {
        proptest::test_runner::TestCaseError::Fail(error.to_string().into())
    }
}

/// Error when we fail to build a transaction nonce.
pub struct NonceError;

impl From<NonceError> for Error {
    fn from(_: NonceError) -> Self {
        Error::Other {
            message: "failed to construct nonce".to_string(),
        }
    }
}

/// Error when we fail to build a transaction nonce.
pub struct BalanceError;

impl From<BalanceError> for Error {
    fn from(_: BalanceError) -> Self {
        Error::Other {
            message: "failed to get balance".to_string(),
        }
    }
}
