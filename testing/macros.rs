#[macro_export]
macro_rules! contracts {
    () => {
    };

    ($module:ident {$abi:expr, $bin:expr}, $($tail:tt)*) => {
        #[allow(dead_code)]
        #[allow(missing_docs)]
        #[allow(unused_imports)]
        #[allow(unused_mut)]
        #[allow(unused_variables)]
        pub mod $module {
            #[derive(EthabiContract)]
            #[ethabi_contract_options(path = $abi)]
            struct _Dummy;

            contracts!(@bin $bin);
        }

        contracts!($($tail)*);
    };

    (bin $module:ident {$bin:expr}, $($tail:tt)*) => {
        pub mod $module {
            contracts!(@bin $bin);
        }

        contracts!($($tail)*);
    };

    (@bin $bin:expr) => {
        pub fn bin(linker: &$crate::linker::Linker) -> Result<Vec<u8>, $crate::error::Error> {
            use std::io::Read;
            use std::path::Path;

            let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/", $bin));

            let mut file = ::std::fs::File::open(path)
                .map_err(|e| format!("failed to open file: {}: {}", path.display(), e))?;

            let mut out = Vec::new();

            file.read_to_end(&mut out)
                .map_err(|e| format!("failed to read file: {}: {}", path.display(), e))?;

            let out = linker.link(&out)?;
            Ok(out)
        }
    };
}

/// Helper macro for proptest! to build a closure suitable for passing in to `TestRunner::run`.
#[macro_export]
macro_rules! pt {
  ($($t:tt)*) => { || proptest!($($t)*) };
}
