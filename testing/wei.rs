// Parts borrowed from: https://github.com/paritytech/sol-rs/blob/master/solaris/src/wei.rs

use ethereum_types::U256;

/// Convert ether to wei.
pub fn from_ether(value: impl Into<U256>) -> U256 {
    value.into() * U256::from(10).pow(18.into())
}

/// Convert finney to wei.
pub fn from_finney(value: impl Into<U256>) -> U256 {
    value.into() * U256::from(10).pow(15.into())
}

/// Convert szabo to wei.
pub fn from_szabo(value: impl Into<U256>) -> U256 {
    value.into() * U256::from(10).pow(12.into())
}

/// Convert gwei (gigawei) to wei.
pub fn from_gwei(value: impl Into<U256>) -> U256 {
    value.into() * U256::from(10).pow(9.into())
}

/// Convert mwei (milliwei) to wei.
pub fn from_mwei(value: impl Into<U256>) -> U256 {
    value.into() * U256::from(10).pow(6.into())
}

/// Convert kwei (kilowei) to wei.
pub fn from_kwei(value: impl Into<U256>) -> U256 {
    value.into() * U256::from(10).pow(3.into())
}
