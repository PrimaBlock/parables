use ethereum_types::U256;
use evm::{CallResult, CreateResult};
use proptest;
use std::fmt;
use std::result;
use trace;
use trace::TxEvent;

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
    #[fail(display = "call error: {}", message)]
    Call { message: String },
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

impl<E> From<CallError<E>> for Error
where
    CallError<E>: fmt::Display,
{
    fn from(error: CallError<E>) -> Self {
        Error::Call {
            message: error.to_string(),
        }
    }
}

impl From<Error> for proptest::test_runner::TestCaseError {
    fn from(error: Error) -> proptest::test_runner::TestCaseError {
        proptest::test_runner::TestCaseError::Fail(error.to_string().into())
    }
}

/// An error occurred during a call.
#[derive(Debug, PartialEq, Eq, Fail)]
pub enum CallError<E> {
    #[fail(display = "call was reverted: {}", revert)]
    Reverted { execution: E, revert: trace::Revert },
    #[fail(display = "bad status: {}", status)]
    Status { execution: E, status: u8 },
    #[fail(display = "trace error")]
    Trace { execution: E, trace: Vec<TxEvent> },
    #[fail(display = "sync logs: {}", message)]
    SyncLogs { execution: E, message: &'static str },
    #[fail(display = "other call error: {}", message)]
    Other { message: String },
}

impl<E> CallError<E> {
    /// Access the underlying execution for this call error, if available.
    fn execution(&self) -> Option<&E> {
        use self::CallError::*;

        match *self {
            Reverted { ref execution, .. } => Some(execution),
            Status { ref execution, .. } => Some(execution),
            Trace { ref execution, .. } => Some(execution),
            SyncLogs { ref execution, .. } => Some(execution),
            _ => None,
        }
    }
}

impl<E> From<&'static str> for CallError<E> {
    fn from(value: &'static str) -> Self {
        CallError::Other {
            message: value.to_string(),
        }
    }
}

impl<E> From<String> for CallError<E> {
    fn from(value: String) -> Self {
        CallError::Other { message: value }
    }
}

impl<E> From<CallError<E>> for proptest::test_runner::TestCaseError
where
    CallError<E>: fmt::Display,
{
    fn from(error: CallError<E>) -> proptest::test_runner::TestCaseError {
        proptest::test_runner::TestCaseError::Fail(error.to_string().into())
    }
}

/// Error when we fail to build a transaction nonce.
pub struct NonceError;

impl<E> From<NonceError> for CallError<E> {
    fn from(_: NonceError) -> Self {
        CallError::Other {
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

/// Information known about all call errors.
pub trait ResultCallErrorExt {
    /// Check if the result is errored because of an revert.
    fn is_reverted(&self) -> bool {
        false
    }
}

pub trait ResultExt {
    fn gas_used(&self) -> Option<U256>;
}

impl ResultExt for result::Result<CallResult, CallError<CallResult>> {
    fn gas_used(&self) -> Option<U256> {
        match *self {
            Ok(ref execution) => Some(execution.gas_used),
            Err(ref err) => err.execution().map(|e| e.gas_used),
        }
    }
}

impl ResultExt for result::Result<CreateResult, CallError<CreateResult>> {
    fn gas_used(&self) -> Option<U256> {
        match *self {
            Ok(ref execution) => Some(execution.gas_used),
            Err(ref err) => err.execution().map(|e| e.gas_used),
        }
    }
}

impl<T, E> ResultCallErrorExt for result::Result<T, CallError<E>> {
    fn is_reverted(&self) -> bool {
        match *self {
            Err(CallError::Reverted { .. }) => true,
            _ => false,
        }
    }
}
