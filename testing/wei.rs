// Parts borrowed from: https://github.com/paritytech/sol-rs/blob/master/solaris/src/wei.rs

use ethereum_types::U256;

/// Convert ether to wei.
pub fn from_ether<T: Into<U256>>(value: T) -> U256 {
    value.into() * U256::from(10).pow(18.into())
}

/// Convert finney to wei.
pub fn from_finney<T: Into<U256>>(value: T) -> U256 {
    value.into() * U256::from(10).pow(15.into())
}

/// Convert szabo to wei.
pub fn from_szabo<T: Into<U256>>(value: T) -> U256 {
    value.into() * U256::from(10).pow(12.into())
}
