use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce, Key
};
use base64::{Engine as _, engine::general_purpose};
use anyhow::{Result, anyhow};
use std::env;

pub struct Vault {
    cipher: Aes256Gcm,
}

impl Vault {
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

        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
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
}
