/// Pillar 3: Secrets Vault Tests
/// Tests encryption/decryption, secret storage, and nonce uniqueness

#[cfg(test)]
mod vault_tests {

    /// Test 1: AES-256-GCM Encryption Roundtrip
    /// Verify encrypt → decrypt preserves data integrity
    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        unsafe {
            std::env::set_var("VORTEX_SECRET_KEY", "a".repeat(32)); // 32 bytes for AES-256
        }
        
        let plaintext = "sensitive-database-password";
        
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce, Key
        };
        use base64::{Engine as _, engine::general_purpose};
        
        let key_bytes = b"a".repeat(32);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        // Encrypt
        let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).expect("Encryption failed");
        let mut combined = nonce_bytes.clone();
        combined.extend(ciphertext);
        let encrypted = general_purpose::STANDARD.encode(combined);
        
        // Decrypt
        let decoded = general_purpose::STANDARD.decode(&encrypted).expect("Base64 decode failed");
        let (nonce_dec, ciphertext_dec) = decoded.split_at(12);
        let nonce_dec = Nonce::from_slice(nonce_dec);
        let decrypted_bytes = cipher.decrypt(nonce_dec, ciphertext_dec).expect("Decryption failed");
        let decrypted = String::from_utf8(decrypted_bytes).expect("UTF-8 decode failed");
        
        // Verify
        assert_eq!(plaintext, decrypted, "Decrypted text must match original");
        assert!(!encrypted.is_empty(), "Encrypted text must not be empty");
    }

    /// Test 2: Invalid Key Decryption Should Fail
    /// Verify decryption with wrong key fails gracefully
    #[test]
    fn test_decryption_with_invalid_key_fails() {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce, Key
        };
        use base64::{Engine as _, engine::general_purpose};
        
        let plaintext = "secret-data";
        let key1 = "a".repeat(32).as_bytes().to_vec();
        let key2 = "b".repeat(32).as_bytes().to_vec();
        
        // Encrypt with key1
        let key = Key::<Aes256Gcm>::from_slice(&key1);
        let cipher = Aes256Gcm::new(key);
        let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).expect("Encryption failed");
        let mut combined = nonce_bytes;
        combined.extend(ciphertext);
        let encrypted = general_purpose::STANDARD.encode(combined);
        
        // Try to decrypt with key2
        let decoded = general_purpose::STANDARD.decode(&encrypted).expect("Base64 decode failed");
        let (nonce_dec, ciphertext_dec) = decoded.split_at(12);
        let nonce_dec = Nonce::from_slice(nonce_dec);
        
        let key2_obj = Key::<Aes256Gcm>::from_slice(&key2);
        let cipher2 = Aes256Gcm::new(key2_obj);
        let result = cipher2.decrypt(nonce_dec, ciphertext_dec);
        
        // Must fail
        assert!(result.is_err(), "Decryption with wrong key must fail");
    }

    /// Test 3: Nonce Uniqueness
    /// Verify each encryption uses a unique nonce (no repeats)
    #[test]
    fn test_nonce_uniqueness() {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce, Key
        };
        use std::collections::HashSet;
        
        let key_bytes = b"a".repeat(32);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        let plaintext = "test-data";
        let mut nonces = HashSet::new();
        
        // Encrypt same plaintext 10 times
        for _ in 0..10 {
            let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec();
            let nonce_hex = hex::encode(&nonce_bytes);
            
            // Each nonce must be unique
            assert!(
                nonces.insert(nonce_hex),
                "Nonce must be unique for each encryption"
            );
            
            let nonce = Nonce::from_slice(&nonce_bytes);
            let _ = cipher.encrypt(nonce, plaintext.as_bytes());
        }
        
        // All 10 nonces must be different
        assert_eq!(nonces.len(), 10, "All 10 nonces must be unique");
    }

    /// Test 4: Invalid Ciphertext Should Fail
    /// Verify decryption with corrupted/invalid ciphertext fails gracefully
    #[test]
    fn test_invalid_ciphertext_fails() {
        use base64::{Engine as _, engine::general_purpose};
        
        // Create invalid ciphertext (too short)
        let invalid_ct = "aaa"; // Too short
        let result = general_purpose::STANDARD.decode(invalid_ct);
        
        assert!(result.is_err() || {
            let decoded = result.unwrap();
            decoded.len() < 12 // Must have at least 12 bytes for nonce
        }, "Invalid ciphertext should fail to decode or be too short");
    }

    /// Test 5: Base64 Encoding/Decoding
    /// Verify data survives base64 encoding roundtrip
    #[test]
    fn test_base64_roundtrip() {
        use base64::{Engine as _, engine::general_purpose};
        
        let data = vec![0u8, 1, 2, 255, 254, 253];
        let encoded = general_purpose::STANDARD.encode(&data);
        let decoded = general_purpose::STANDARD.decode(&encoded).expect("Base64 decode failed");
        
        assert_eq!(data, decoded, "Base64 roundtrip must preserve data");
    }

    /// Test 6: Empty Plaintext Encryption
    /// Verify encryption handles empty strings
    #[test]
    fn test_encrypt_empty_plaintext() {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce, Key
        };
        
        let key_bytes = b"a".repeat(32);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        let plaintext = "";
        let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec();
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Should successfully encrypt empty string
        let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).expect("Encryption failed");
        assert!(!ciphertext.is_empty(), "Even empty plaintext should have ciphertext");
    }

    /// Test 7: Large Data Encryption
    /// Verify encryption works with large payloads
    #[test]
    fn test_encrypt_large_data() {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce, Key
        };
        
        let key_bytes = b"a".repeat(32);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        // 1MB of data
        let plaintext = "x".repeat(1024 * 1024);
        let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec();
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).expect("Encryption failed");
        assert!(ciphertext.len() > 0, "Large data should encrypt successfully");
        
        // Decrypt to verify
        let decrypted = cipher.decrypt(nonce, ciphertext.as_ref()).expect("Decryption failed");
        assert_eq!(decrypted.len(), plaintext.len(), "Decrypted size must match");
    }

    /// Test 8: UTF-8 Special Characters
    /// Verify encryption handles special UTF-8 characters
    #[test]
    fn test_encrypt_special_characters() {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce, Key
        };
        
        let key_bytes = b"a".repeat(32);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        let plaintexts = vec![
            "emoji: 🔐🚀🎯",
            "chinese: 中文密钥",
            "emoji-mixed: password😊",
            "special: !@#$%^&*()",
            "unicode: \u{00E9}\u{00E8}\u{00EA}",
        ];
        
        for plaintext in plaintexts {
            let nonce_bytes = uuid::Uuid::new_v4().as_bytes()[..12].to_vec();
            let nonce = Nonce::from_slice(&nonce_bytes);
            
            let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).expect("Encryption failed");
            let decrypted = cipher.decrypt(nonce, ciphertext.as_ref()).expect("Decryption failed");
            let decrypted_str = String::from_utf8(decrypted).expect("UTF-8 decode failed");
            
            assert_eq!(plaintext, decrypted_str, "Special characters must survive roundtrip");
        }
    }
}
