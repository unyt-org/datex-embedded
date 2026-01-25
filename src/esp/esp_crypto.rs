use core::future::Future;
use alloc::format;
use datex_core::stdlib::pin::Pin;
use datex_core::crypto::crypto::{CryptoError, CryptoResult, CryptoTrait};
use datex_core::stdlib::vec::Vec;
use datex_core::stdlib::boxed::Box;
use esp_hal::rng::Rng;
use core::result::Result;
use datex_core::stdlib::string::String;
use alloc::vec;

#[derive(Debug, Clone)]
pub struct EspCrypto {
    pub rng: Rng,
}

impl EspCrypto {
    pub fn new() -> Self {
        Self { rng: Rng::new() }
    }
}

impl CryptoTrait for EspCrypto {
    fn create_uuid(&self) -> String {
        // TODO: use uuid crate?
        let mut bytes = [0u8; 16];
        self.rng.read(&mut bytes);

        // set version to 4 -- random
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        // set variant to RFC 4122
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u16::from_be_bytes([bytes[4], bytes[5]]),
            u16::from_be_bytes([bytes[6], bytes[7]]),
            u16::from_be_bytes([bytes[8], bytes[9]]),
            u64::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], 0, 0]) >> 16
        )
    }

    fn random_bytes(&self, length: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; length];
        self.rng.read(&mut bytes);
        bytes
    }

    fn hash_sha256<'a>(&'a self, to_digest: &'a [u8]) -> CryptoResult<'a, [u8; 32]> {
        todo!()
    }

    fn hkdf_sha256<'a>(&'a self, ikm: &'a [u8], salt: &'a [u8]) -> CryptoResult<'a, [u8; 32]> {
        todo!()
    }

    fn gen_ed25519(&self) -> Pin<Box<dyn Future<Output=Result<(Vec<u8>, Vec<u8>), CryptoError>> + 'static>> {
        todo!()
    }

    fn sig_ed25519<'a>(&'a self, pri_key: &'a [u8], data: &'a [u8]) -> Pin<Box<dyn Future<Output=Result<[u8; 64], CryptoError>> + 'a>> {
        todo!()
    }

    fn ver_ed25519<'a>(&'a self, pub_key: &'a [u8], sig: &'a [u8], data: &'a [u8]) -> Pin<Box<dyn Future<Output=Result<bool, CryptoError>> + 'a>> {
        todo!()
    }

    fn aes_ctr_encrypt<'a>(&'a self, key: &'a [u8; 32], iv: &'a [u8; 16], plaintext: &'a [u8]) -> Pin<Box<dyn Future<Output=Result<Vec<u8>, CryptoError>> + 'a>> {
        todo!()
    }

    fn aes_ctr_decrypt<'a>(&'a self, key: &'a [u8; 32], iv: &'a [u8; 16], cipher: &'a [u8]) -> Pin<Box<dyn Future<Output=Result<Vec<u8>, CryptoError>> + 'a>> {
        todo!()
    }

    fn key_upwrap<'a>(&'a self, kek_bytes: &'a [u8; 32], rb: &'a [u8; 32]) -> Pin<Box<dyn Future<Output=Result<[u8; 40], CryptoError>> + 'a>> {
        todo!()
    }

    fn key_unwrap<'a>(&'a self, kek_bytes: &'a [u8; 32], cipher: &'a [u8; 40]) -> Pin<Box<dyn Future<Output=Result<[u8; 32], CryptoError>> + 'a>> {
        todo!()
    }

    fn gen_x25519(&self) -> Pin<Box<dyn Future<Output=Result<([u8; 44], [u8; 48]), CryptoError>>>> {
        todo!()
    }

    fn derive_x25519<'a>(&'a self, pri_key: &'a [u8; 48], peer_pub: &'a [u8; 44]) -> Pin<Box<dyn Future<Output=Result<Vec<u8>, CryptoError>> + 'a>> {
        todo!()
    }
}