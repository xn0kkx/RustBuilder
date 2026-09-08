use anyhow::{anyhow, Context, Result};
use pgp::composed::{
    Deserializable, KeyType, Message, SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey,
    SubkeyParamsBuilder,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::types::{CompressionAlgorithm, SecretKeyTrait};
use smallvec::smallvec;
use std::io::Cursor;

pub fn generate_keypair(uid: &str, passphrase: &str) -> Result<(String, String)> {
    let mut rng = rand::thread_rng();

    let subkey = SubkeyParamsBuilder::default()
        .key_type(KeyType::Rsa(3072))
        .can_encrypt(true)
        .build()
        .map_err(|e| anyhow!("failed to build subkey params: {e}"))?;

    let params = SecretKeyParamsBuilder::default()
        .key_type(KeyType::Rsa(3072))
        .can_certify(true)
        .can_sign(true)
        .primary_user_id(uid.to_string())
        .preferred_symmetric_algorithms(smallvec![SymmetricKeyAlgorithm::AES256])
        .preferred_hash_algorithms(smallvec![HashAlgorithm::SHA2_256])
        .preferred_compression_algorithms(smallvec![CompressionAlgorithm::ZLIB])
        .subkey(subkey)
        .build()
        .map_err(|e| anyhow!("failed to build key params: {e}"))?;

    let secret_key = params
        .generate_with_rng(&mut rng)
        .map_err(|e| anyhow!("failed to generate secret key: {e}"))?;

    let pass = passphrase.to_string();
    let signed_secret = secret_key
        .sign(|| pass.clone())
        .map_err(|e| anyhow!("failed to sign secret key: {e}"))?;

    let public_key = signed_secret.public_key();
    let signed_public = public_key
        .sign(&signed_secret, || pass.clone())
        .map_err(|e| anyhow!("failed to sign public key: {e}"))?;

    let priv_armored = signed_secret
        .to_armored_string(Default::default())
        .map_err(|e| anyhow!("failed to armor secret key: {e}"))?;
    let pub_armored = signed_public
        .to_armored_string(Default::default())
        .map_err(|e| anyhow!("failed to armor public key: {e}"))?;

    Ok((pub_armored, priv_armored))
}

pub fn encrypt_to_public(plaintext: &[u8], pub_armored: &str) -> Result<Vec<u8>> {
    let mut rng = rand::thread_rng();
    let (public_key, _) = SignedPublicKey::from_armor_single(Cursor::new(pub_armored.as_bytes()))
        .context("failed to parse public key")?;

    let literal = Message::new_literal_bytes("", plaintext);
    let compressed = literal
        .compress(CompressionAlgorithm::ZLIB)
        .context("failed to compress message")?;
    let encrypted = compressed
        .encrypt_to_keys(&mut rng, SymmetricKeyAlgorithm::AES256, &[&public_key])
        .context("failed to encrypt message")?;

    let armored = encrypted
        .to_armored_string(Default::default())
        .context("failed to armor encrypted message")?;
    Ok(armored.into_bytes())
}

pub fn decrypt(ciphertext: &[u8], priv_armored: &str, passphrase: &str) -> Result<Vec<u8>> {
    let (secret_key, _) = SignedSecretKey::from_armor_single(Cursor::new(priv_armored.as_bytes()))
        .context("failed to parse secret key")?;

    let message = if ciphertext.starts_with(b"-----BEGIN PGP") {
        let (m, _) = Message::from_armor_single(Cursor::new(ciphertext))
            .context("failed to parse armored message")?;
        m
    } else {
        Message::from_bytes(Cursor::new(ciphertext)).context("failed to parse binary message")?
    };

    let (decrypted, _) = message
        .decrypt(|| passphrase.to_string(), &[&secret_key])
        .context("failed to decrypt message")?;

    let content = decrypted
        .decompress()
        .context("failed to decompress message")?
        .get_content()
        .context("failed to extract decrypted content")?
        .ok_or_else(|| anyhow!("decrypted message has no content"))?;

    Ok(content)
}
