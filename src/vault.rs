use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce, Key
};
use argon2::Argon2;
use base64::{Engine as _, engine::general_purpose};
use anyhow::{Result, anyhow};
use std::env;

/// Fixed salt for Argon2id key derivation.
///
/// # Security
/// A fixed salt is acceptable here because the input key material
/// (VORTEX_SECRET_KEY) is already high-entropy. The KDF serves to
/// harden the key against brute-force rather than defend against
/// rainbow tables on low-entropy passwords.
const VAULT_KDF_SALT: &[u8; 16] = b"vortex-vault-kdf";

pub struct Vault {
    cipher: Aes256Gcm,
}

impl Vault {
    /// Create a new Vault instance.
    ///
    /// # Security
    /// The raw `VORTEX_SECRET_KEY` is stretched through Argon2id key
    /// derivation before being used as the AES-256-GCM key. This
    /// prevents direct use of raw string bytes as cryptographic keys
    /// (SEC-1).
    pub fn new() -> Result<Self> {
        let key_str = env::var("VORTEX_SECRET_KEY")
            .map_err(|_| anyhow!("VORTEX_SECRET_KEY environment variable not set. A 32-byte key is required."))?;
        
        // BUG-14 FIX: Auto-detect key format to allow higher-entropy keys.
        // Supports: hex (64 hex chars → 32 bytes), base64 (44 chars → 32 bytes),
        // or raw ASCII (exactly 32 bytes).
        let key_bytes: Vec<u8> = if key_str.len() == 64 && key_str.chars().all(|c| c.is_ascii_hexdigit()) {
            // Hex-encoded 32-byte key
            (0..64).step_by(2)
                .map(|i| u8::from_str_radix(&key_str[i..i+2], 16))
                .collect::<Result<Vec<u8>, _>>()
                .map_err(|e| anyhow!("VORTEX_SECRET_KEY hex decode failed: {}", e))?
        } else if key_str.len() == 44 && key_str.ends_with('=') {
            // Base64-encoded 32-byte key
            let decoded = general_purpose::STANDARD.decode(&key_str)
                .map_err(|e| anyhow!("VORTEX_SECRET_KEY base64 decode failed: {}", e))?;
            if decoded.len() != 32 {
                return Err(anyhow!("VORTEX_SECRET_KEY base64 decoded to {} bytes, expected 32", decoded.len()));
            }
            decoded
        } else if key_str.as_bytes().len() == 32 {
            // Raw 32-byte ASCII key (original behavior)
            key_str.as_bytes().to_vec()
        } else {
            return Err(anyhow!(
                "VORTEX_SECRET_KEY must be one of: 32 raw ASCII bytes, 64 hex chars, or 44-char base64 string. Got {} chars.",
                key_str.len()
            ));
        };

        // SEC-1 FIX: Derive the AES-256-GCM key via Argon2id instead of
        // using the raw input bytes directly.
        let derived_key = derive_key_argon2id(&key_bytes)?;

        let key = Key::<Aes256Gcm>::from_slice(&derived_key);
        let cipher = Aes256Gcm::new(key);

        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        use aes_gcm::aead::{AeadCore, OsRng};

        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self.cipher.encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("Encryption failure: {}", e))?;

        // Combine nonce + ciphertext
        let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
        combined.extend_from_slice(&nonce);
        combined.extend_from_slice(&ciphertext);

        let result = general_purpose::STANDARD.encode(combined);
        if result.is_empty() {
            return Err(anyhow!("Encryption produced empty string"));
        }
        Ok(result)
    }

    pub fn decrypt(&self, encoded_ciphertext: &str) -> Result<String> {
        let combined = general_purpose::STANDARD.decode(encoded_ciphertext)
            .map_err(|e| anyhow!("Base64 decode failure: {}", e))?;

        if combined.len() < 12 {
            return Err(anyhow!("Invalid ciphertext: too short"));
        }

        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext_bytes = self.cipher.decrypt(nonce, ciphertext)
            .map_err(|_| anyhow!("Decryption failure"))?;

        Ok(String::from_utf8(plaintext_bytes)
            .map_err(|e| anyhow!("UTF-8 decode failure: {}", e))?)
    }

    /// ENT-12: Re-encrypt a single secret under a new vault key.
    ///
    /// Decrypts the `ciphertext` with `self` (the current key), then
    /// re-encrypts it with `new_vault` (the new key). The result can be
    /// stored in the database to complete key rotation for that secret.
    pub fn rotate_secret(&self, ciphertext: &str, new_vault: &Vault) -> Result<String> {
        let plaintext = self.decrypt(ciphertext)?;
        let new_ciphertext = new_vault.encrypt(&plaintext)?;
        Ok(new_ciphertext)
    }

    /// ENT-12: Batch-rotate all secrets to a new vault key.
    ///
    /// Accepts a slice of `(secret_name, current_ciphertext)` pairs.
    /// Returns a `Vec<(secret_name, new_ciphertext)>` that the caller
    /// should persist atomically (e.g. inside a DB transaction) after
    /// bumping `key_version` in the `vault_key_rotations` table.
    ///
    /// # API endpoint
    /// POST /api/v1/admin/secrets/rotate — rotate all secrets to new vault key.
    /// Requires admin RBAC permission. Full HTTP handler is in `src/web.rs`.
    pub fn rotate_all_secrets(
        &self,
        ciphertexts: &[(String, String)],
        new_vault: &Vault,
    ) -> Result<Vec<(String, String)>> {
        ciphertexts
            .iter()
            .map(|(key, ct)| {
                let new_ct = self.rotate_secret(ct, new_vault)?;
                Ok((key.clone(), new_ct))
            })
            .collect()
    }
}

/// Derive a 32-byte AES-256 key from raw input material using Argon2id.
///
/// # Security
/// Argon2id is the recommended KDF (RFC 9106) combining resistance to
/// both side-channel and GPU attacks. This replaces direct use of raw
/// bytes as AES keys (SEC-1).
fn derive_key_argon2id(input_key: &[u8]) -> Result<[u8; 32]> {
    let argon2 = Argon2::default();
    let mut derived = [0u8; 32];
    argon2
        .hash_password_into(input_key, VAULT_KDF_SALT, &mut derived)
        .map_err(|e| anyhow!("Argon2id key derivation failed: {}", e))?;
    Ok(derived)
}
