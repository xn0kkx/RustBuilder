use anyhow::{anyhow, Context, Result};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroize;

pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
pub const KEY_LEN: usize = 32;

pub fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

pub fn derive_master_key(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow!("seal failed: {e}"))?;
    Ok((nonce_bytes.to_vec(), ciphertext))
}

pub fn open(key: &[u8; KEY_LEN], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != NONCE_LEN {
        return Err(anyhow!("invalid nonce length"));
    }
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("open failed: {e}"))
        .context("authentication failed while opening sealed data")
}

pub fn seal_str(key: &[u8; KEY_LEN], value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut bytes = value.as_bytes().to_vec();
    let result = seal(key, &bytes);
    bytes.zeroize();
    result
}

pub fn open_str(key: &[u8; KEY_LEN], nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
    let bytes = open(key, nonce, ciphertext)?;
    String::from_utf8(bytes).context("sealed data is not valid utf-8")
}
