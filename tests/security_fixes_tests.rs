// Tests for BUG-H7, BUG-H11, BUG-M2, BUG-M6 security fixes

// ─── BUG-H7: escape_like_pattern ─────────────────────────────────────────────

#[test]
fn test_escape_like_pattern_percent() {
    let result = ryuo::db_postgres::escape_like_pattern("100%");
    assert_eq!(result, r"100\%");
}

#[test]
fn test_escape_like_pattern_underscore() {
    let result = ryuo::db_postgres::escape_like_pattern("my_dag");
    assert_eq!(result, r"my\_dag");
}

#[test]
fn test_escape_like_pattern_backslash() {
    let result = ryuo::db_postgres::escape_like_pattern(r"path\to");
    assert_eq!(result, r"path\\to");
}

#[test]
fn test_escape_like_pattern_all_metacharacters() {
    let result = ryuo::db_postgres::escape_like_pattern(r"a%b_c\d");
    assert_eq!(result, r"a\%b\_c\\d");
}

#[test]
fn test_escape_like_pattern_no_special_chars() {
    let result = ryuo::db_postgres::escape_like_pattern("normal-dag-id");
    assert_eq!(result, "normal-dag-id");
}

#[test]
fn test_escape_like_pattern_empty() {
    let result = ryuo::db_postgres::escape_like_pattern("");
    assert_eq!(result, "");
}

// ─── BUG-H11: sanitize_path_component ────────────────────────────────────────

#[test]
fn test_sanitize_path_component_normal() {
    let result = ryuo::swarm::sanitize_path_component("my-dag_123");
    assert_eq!(result, Ok("my-dag_123".to_string()));
}

#[test]
fn test_sanitize_path_component_traversal_attack() {
    let result = ryuo::swarm::sanitize_path_component("../../etc");
    assert_eq!(result, Ok("______etc".to_string()));
}

#[test]
fn test_sanitize_path_component_dots_and_slashes() {
    let result = ryuo::swarm::sanitize_path_component("../../../passwd");
    assert_eq!(result, Ok("_________passwd".to_string()));
}

#[test]
fn test_sanitize_path_component_spaces_and_special() {
    let result = ryuo::swarm::sanitize_path_component("dag name!@#$");
    assert_eq!(result, Ok("dag_name____".to_string()));
}

#[test]
fn test_sanitize_path_component_empty_rejected() {
    let result = ryuo::swarm::sanitize_path_component("");
    assert!(result.is_err());
}

#[test]
fn test_sanitize_path_component_unicode_replaced() {
    let result = ryuo::swarm::sanitize_path_component("dag—étl");
    assert_eq!(result, Ok("dag__tl".to_string()));
}

// ─── SEC-12: Password strength validation ─────────────────────────────────────

#[test]
fn test_strong_password_accepted() {
    assert!(ryuo::db_postgres::validate_password_strength("Str0ng!Pass").is_ok());
    assert!(ryuo::db_postgres::validate_password_strength("C0mpl3x@Pwd").is_ok());
    assert!(ryuo::db_postgres::validate_password_strength("Ab1!xxxx").is_ok());
}

#[test]
fn test_short_password_rejected() {
    let err = ryuo::db_postgres::validate_password_strength("Ab1!").unwrap_err();
    assert!(
        err.to_string().contains("at least 8 characters"),
        "Should mention length: {}", err
    );
}

#[test]
fn test_no_uppercase_rejected() {
    let err = ryuo::db_postgres::validate_password_strength("abcdefg1!").unwrap_err();
    assert!(err.to_string().contains("uppercase"), "Should mention uppercase: {}", err);
}

#[test]
fn test_no_lowercase_rejected() {
    let err = ryuo::db_postgres::validate_password_strength("ABCDEFG1!").unwrap_err();
    assert!(err.to_string().contains("lowercase"), "Should mention lowercase: {}", err);
}

#[test]
fn test_no_digit_rejected() {
    let err = ryuo::db_postgres::validate_password_strength("Abcdefgh!").unwrap_err();
    assert!(err.to_string().contains("digit"), "Should mention digit: {}", err);
}

#[test]
fn test_no_special_char_rejected() {
    let err = ryuo::db_postgres::validate_password_strength("Abcdefg1").unwrap_err();
    assert!(err.to_string().contains("special character"), "Should mention special: {}", err);
}

#[test]
fn test_all_uppercase_with_digits_rejected() {
    // SEC-12: "AAAA1111" was previously allowed by the old check
    let err = ryuo::db_postgres::validate_password_strength("AAAA1111").unwrap_err();
    assert!(
        err.to_string().contains("lowercase") && err.to_string().contains("special"),
        "Should mention both lowercase and special: {}", err
    );
}

#[test]
fn test_multiple_missing_requirements() {
    let err = ryuo::db_postgres::validate_password_strength("aaa").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("8 characters"), "msg: {}", msg);
    assert!(msg.contains("uppercase"), "msg: {}", msg);
    assert!(msg.contains("digit"), "msg: {}", msg);
    assert!(msg.contains("special"), "msg: {}", msg);
}

// ─── SEC-1: Vault KDF via Argon2id ────────────────────────────────────────────

#[test]
fn test_vault_argon2id_roundtrip() {
    unsafe { std::env::set_var("RYUO_SECRET_KEY", "a]bCdEfGhIjKlMnOpQrStUvWxYz12345"); }
    let vault = ryuo::vault::Vault::new().expect("Vault::new should succeed");
    let plaintext = "my-secret-value";
    let encrypted = vault.encrypt(plaintext).expect("encrypt");
    let decrypted = vault.decrypt(&encrypted).expect("decrypt");
    assert_eq!(plaintext, decrypted);
}

#[test]
fn test_vault_hex_key_roundtrip() {
    unsafe {
        std::env::set_var(
            "RYUO_SECRET_KEY",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
    }
    let vault = ryuo::vault::Vault::new().expect("Vault::new with hex key");
    let plaintext = "hex-key-secret";
    let encrypted = vault.encrypt(plaintext).expect("encrypt");
    let decrypted = vault.decrypt(&encrypted).expect("decrypt");
    assert_eq!(plaintext, decrypted);
}

#[test]
fn test_vault_cross_key_decryption_fails() {
    unsafe { std::env::set_var("RYUO_SECRET_KEY", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"); }
    let vault1 = ryuo::vault::Vault::new().unwrap();
    let enc1 = vault1.encrypt("test").unwrap();

    unsafe { std::env::set_var("RYUO_SECRET_KEY", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"); }
    let vault2 = ryuo::vault::Vault::new().unwrap();

    assert!(vault2.decrypt(&enc1).is_err(), "Cross-key decryption must fail");
}
