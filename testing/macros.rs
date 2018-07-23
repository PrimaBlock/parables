/// Include the generated contracts directory.
#[macro_export]
macro_rules! contracts {
    () => {
        include!(concat!(env!("OUT_DIR"), "/contracts.rs"));
    };
}

/// Helper macro for proptest! to build a closure suitable for passing in to `TestRunner::run`.
#[macro_export]
macro_rules! pt {
  (move $($t:tt)*) => { move || proptest!($($t)*) };
  ($($t:tt)*) => { || proptest!($($t)*) };
}

/// Convert the given argument into wei.
#[macro_export]
macro_rules! wei {
    ($value:tt) => {
        $crate::ethereum_types::U256::from($value)
    };
    ($value:tt ether) => {
        $crate::wei::from_ether($value)
    };
    ($value:tt eth) => {
        $crate::wei::from_ether($value)
    };
    ($value:tt finney) => {
        $crate::wei::from_finney($value)
    };
    ($value:tt szabo) => {
        $crate::wei::from_szabo($value)
    };
}
