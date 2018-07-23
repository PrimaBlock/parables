//! Holder of crypto primitives we use.

use rand::XorShiftRng;
use secp256k1::Secp256k1;
use std::sync::Arc;

/// Context for all cryptography functions.
#[derive(Clone)]
pub struct Crypto {
    pub rng: XorShiftRng,
    pub secp: Arc<Secp256k1>,
}

impl Crypto {
    /// Build a new crypto context.
    pub fn new() -> Self {
        Self {
            rng: XorShiftRng::new_unseeded(),
            secp: Arc::new(Secp256k1::new()),
        }
    }
}
