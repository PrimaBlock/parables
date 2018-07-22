use crypto::digest::Digest;
use crypto::sha3::Sha3;
use error;
use ethereum_types::{Address, H160, H256, U256};
use rand::{Rng, XorShiftRng};
use secp256k1;
use secp256k1::{key, Secp256k1};
use std::fmt;

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut checksum = Sha3::keccak256();
    checksum.input(bytes);
    let mut hash = [0u8; 32];
    checksum.result(&mut hash);
    hash
}

#[derive(Debug)]
pub enum Error {
    DerivePublicKeyError(secp256k1::Error),
    SignError(secp256k1::Error),
    MessageError(secp256k1::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;

        match *self {
            DerivePublicKeyError(e) => write!(fmt, "failed to derive public key: {}", e),
            SignError(e) => write!(fmt, "failed to sign: {}", e),
            MessageError(e) => write!(fmt, "failed to build signature message: {}", e),
        }
    }
}

#[derive(Debug)]
pub struct Account {
    secret: key::SecretKey,
    public: key::PublicKey,
    address: Address,
}

impl Account {
    /// Create a new address with the give rng implementation.
    pub fn new(crypto: &mut Crypto) -> Result<Self, Error> {
        let secp = Secp256k1::new();
        let secret = key::SecretKey::new(&secp, &mut crypto.rng);
        let public =
            key::PublicKey::from_secret_key(&secp, &secret).map_err(Error::DerivePublicKeyError)?;

        let address = {
            let serialized = public.serialize_vec(&secp, false);
            // NB: important that we convert from H256 since `H256 -> H160` trims the leading bits.
            // i.e.: 00 00 00 af ff ff ff ff -> af ff ff ff ff
            let hash = H256::from(keccak256(&serialized[1..]));
            Address::from(H160::from(hash))
        };

        Ok(Self {
            secret,
            public,
            address,
        })
    }

    /// Create a new signer.
    pub fn signer<'a>(&'a self, crypto: &'a Crypto) -> Signer<'a> {
        Signer::new(self, crypto)
    }

    /// Access the address part of the full address.
    pub fn address(&self) -> Address {
        self.address
    }
}

pub struct Signer<'a> {
    address: &'a Account,
    crypto: &'a Crypto,
    checksum: Sha3,
}

impl<'a> Signer<'a> {
    pub fn new(address: &'a Account, crypto: &'a Crypto) -> Self {
        Self {
            address,
            crypto,
            checksum: Sha3::keccak256(),
        }
    }

    /// Input the given set of bytes.
    pub fn input<D: Digestable>(&mut self, digestable: D) {
        let digested = digestable.digest();
        self.checksum.input(&digested);
    }

    /// Finish the signature.
    pub fn finish(self) -> Result<Signature, Error> {
        let Signer {
            address,
            crypto,
            mut checksum,
        } = self;

        let hash = {
            let mut hash = vec![0u8; 32];
            checksum.result(&mut hash);
            hash
        };

        let hash = Self::to_rpc_hash(&hash);
        Self::to_secp_signature(address, crypto, &hash)
    }

    /// Convert the given message into an rpc hash, with the expected envelope.
    fn to_rpc_hash(message: &[u8]) -> Vec<u8> {
        let mut checksum = Sha3::keccak256();

        checksum.input(&format!("\x19Ethereum Signed Message:\n{}", message.len()).into_bytes());
        checksum.input(message);

        let mut hash = vec![0u8; 32];
        checksum.result(&mut hash);

        hash
    }

    /// Build a secp256k1 signature.
    fn to_secp_signature(
        address: &Account,
        crypto: &Crypto,
        message: &[u8],
    ) -> Result<Signature, Error> {
        let message = secp256k1::Message::from_slice(message).map_err(Error::MessageError)?;

        let sig = crypto
            .secp
            .sign_recoverable(&message, &address.secret)
            .map_err(Error::SignError)?;

        let (rec_id, data) = sig.serialize_compact(&crypto.secp);

        let mut output = Vec::with_capacity(65);
        output.extend(&data[..]);
        output.push(rec_id.to_i32() as u8);
        Ok(Signature(output))
    }
}

/// Context for all cryptography functions.
pub struct Crypto {
    rng: Box<dyn Rng + Sync>,
    secp: Secp256k1,
}

impl Crypto {
    /// Build a new crypto context.
    pub fn new() -> Self {
        Self {
            rng: Box::new(XorShiftRng::new_unseeded()),
            secp: Secp256k1::new(),
        }
    }
}

#[derive(Debug)]
pub struct Signature(Vec<u8>);

impl From<Signature> for Vec<u8> {
    fn from(sig: Signature) -> Vec<u8> {
        sig.0
    }
}

/// Trait for things which can be digested.
pub trait Digestable {
    /// Digest the given type.
    fn digest(self) -> Vec<u8>;
}

impl Digestable for Vec<u8> {
    fn digest(self) -> Vec<u8> {
        self
    }
}

impl Digestable for U256 {
    fn digest(self) -> Vec<u8> {
        <[u8; 32]>::from(self).to_vec()
    }
}

impl Digestable for H160 {
    fn digest(self) -> Vec<u8> {
        <[u8; 20]>::from(self).to_vec()
    }
}

impl From<Error> for error::Error {
    fn from(error: Error) -> Self {
        error::Error::from(error.to_string())
    }
}
