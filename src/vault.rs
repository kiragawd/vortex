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
            .map_err(|_| anyhow!("VORTEX_SECRET_KEY environment variable not set. Pillar 3 requires a 32-byte key."))?;
        
        let key_bytes = key_str.as_bytes();
        if key_bytes.len() != 32 {
            return Err(anyhow!("VORTEX_SECRET_KEY must be exactly 32 bytes (256 bits). Current length: {}", key_bytes.len()));
        }

        let key = Key::<Aes256Gcm>::from_slice(key_bytes);
        let cipher = Aes256Gcm::new(key);

        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec(); // 96-bit nonce
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        let ciphertext = self.cipher.encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("Encryption failure: {}", e))?;

        // Combine nonce + ciphertext
        let mut combined = nonce_bytes;
        combined.extend(ciphertext);

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

        // Potential side-channel: decryption failure should be generic but logged
        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext_bytes = self.cipher.decrypt(nonce, ciphertext)
            .map_err(|_| anyhow!("Decryption failure - check your VORTEX_SECRET_KEY"))?;

        Ok(String::from_utf8(plaintext_bytes)?)
    }
}
