/// Include the generated contracts directory.
#[macro_export]
macro_rules! contracts {
    ($path:expr, {$($module:ident => $entry:expr,)*}) => {
        #[derive(ParablesContracts)]
        #[parables(path = $path)]
        #[parables_contract($($module = $entry,)*)]
        struct _ParablesContracts;
    };

    ($($module:ident => $entry:expr,)*) => {
        contracts!{"contracts", {$($module => $entry,)*}}
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
    ($value:tt mwei) => {
        $crate::wei::from_mewi($value)
    };
    ($value:tt kwei) => {
        $crate::wei::from_kewi($value)
    };
    ($value:tt gwei) => {
        $crate::wei::from_gwei($value)
    };
    ($value:tt szabo) => {
        $crate::wei::from_szabo($value)
    };
    ($value:tt finney) => {
        $crate::wei::from_finney($value)
    };
    ($value:tt ether) => {
        $crate::wei::from_ether($value)
    };
    ($value:tt eth) => {
        $crate::wei::from_ether($value)
    };
}
