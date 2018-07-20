/// Include the generated contracts directory.
#[macro_export]
macro_rules! contracts {
    () => { include!(concat!(env!("OUT_DIR"), "/contracts.rs")); };
}

/// Helper macro for proptest! to build a closure suitable for passing in to `TestRunner::run`.
#[macro_export]
macro_rules! pt {
  (move $($t:tt)*) => { move || proptest!($($t)*) };
  ($($t:tt)*) => { || proptest!($($t)*) };
}
