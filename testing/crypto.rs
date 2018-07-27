//! Holder of crypto primitives we use.

use rand::XorShiftRng;
use rust_crypto::digest::Digest;
use rust_crypto::sha3::Sha3;
use secp256k1::Secp256k1;
use std::sync::Arc;

/// Calculate the keccak256 hash for the given bytes.
pub fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut checksum = Sha3::keccak256();
    checksum.input(bytes);
    let mut hash = [0u8; 32];
    checksum.result(&mut hash);
    hash
}

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
