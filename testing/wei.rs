// Parts borrowed from: https://github.com/paritytech/sol-rs/blob/master/solaris/src/wei.rs

use ethereum_types::U256;
use std::fmt;

macro_rules! default_impl {
    ($ty:ty) => {
        impl IntoU256 for $ty {
            fn into_u256(self) -> (U256, usize) {
                (self.into(), 0)
            }
        }
    };
}

macro_rules! convert {
    ($name:ident, $exp:expr) => {
        pub fn $name(value: impl IntoU256 + fmt::Display) -> U256 {
            let (value, n) = value.into_u256();

            if $exp < n {
                panic!("illegal literal value {}", value);
            }

            value * U256::from(10).pow(($exp - n).into())
        }
    };
}

convert!(from_ether, 18usize);
convert!(from_finney, 15usize);
convert!(from_szabo, 12usize);
convert!(from_gwei, 9usize);
convert!(from_mwei, 6usize);
convert!(from_kwei, 3usize);

/// Local conversion trait for converting to U256.
pub trait IntoU256 {
    /// Convert into U256.
    fn into_u256(self) -> (U256, usize);
}

default_impl!(u8);
default_impl!(u16);
default_impl!(u32);
default_impl!(u64);
default_impl!(i8);
default_impl!(i16);
default_impl!(i32);
default_impl!(i64);
default_impl!(usize);

impl IntoU256 for f32 {
    fn into_u256(self) -> (U256, usize) {
        let mut c = self;
        let mut n = 0usize;

        while c != c.trunc() && n < 3 {
            n += 1;
            c = c * 10f32;
        }

        let c = c.round();
        (U256::from(c as u64), n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversions() {
        for i in 0..1000 {
            let finney = 1000usize + i;
            let ether = 1f32 + (i as f32) / 1000f32;

            assert_eq!(
                from_finney(finney),
                from_ether(ether),
                "could not handle decimal 1.{:03}",
                i
            );
        }

        assert_eq!(from_finney(1004), from_ether(1.004));
        assert_eq!(from_szabo(1004), from_finney(1.004));
        assert_eq!(from_gwei(1004), from_szabo(1.004));
        assert_eq!(from_mwei(1004), from_gwei(1.004));
        assert_eq!(from_kwei(1004), from_mwei(1.004));
    }
}
