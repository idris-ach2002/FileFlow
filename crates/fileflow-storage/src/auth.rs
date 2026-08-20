use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

pub const PASSWORD_ITERATIONS: u32 = 600_000;
pub const PASSWORD_MIN_CHARS: usize = 12;
pub const PASSWORD_MAX_CHARS: usize = 128;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Le mot de passe doit contenir au moins {PASSWORD_MIN_CHARS} caractères.")]
    PasswordTooShort,
    #[error("Le mot de passe dépasse la limite de {PASSWORD_MAX_CHARS} caractères.")]
    PasswordTooLong,
    #[error("Le format du mot de passe enregistré est invalide.")]
    InvalidPasswordRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordHash {
    pub algorithm: String,
    pub iterations: u32,
    pub salt_hex: String,
    pub hash_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingPreferences {
    pub account_id: Uuid,
    pub completed: bool,
    pub storage_directory: Option<PathBuf>,
    pub language: String,
    pub beginner_mode: bool,
    pub preserve_originals: bool,
    pub notifications: bool,
    pub confirm_destructive_actions: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OnboardingPreferences {
    pub fn new(account_id: Uuid) -> Self {
        let now = Utc::now();
        Self {
            account_id,
            completed: false,
            storage_directory: None,
            language: "fr".into(),
            beginner_mode: true,
            preserve_originals: true,
            notifications: true,
            confirm_destructive_actions: true,
            created_at: now,
            updated_at: now,
        }
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub fn validate_password(password: &str) -> Result<(), AuthError> {
    let chars = password.chars().count();
    if chars < PASSWORD_MIN_CHARS {
        return Err(AuthError::PasswordTooShort);
    }
    if chars > PASSWORD_MAX_CHARS {
        return Err(AuthError::PasswordTooLong);
    }
    Ok(())
}

pub fn hash_password(password: &str) -> Result<PasswordHash, AuthError> {
    validate_password(password)?;
    let salt = *Uuid::new_v4().as_bytes();
    Ok(hash_password_with(password, &salt, PASSWORD_ITERATIONS))
}

pub fn verify_password(password: &str, stored: &PasswordHash) -> bool {
    if stored.algorithm != "pbkdf2-hmac-sha256" || stored.iterations < 10_000 {
        return false;
    }
    let Ok(salt) = decode_hex(&stored.salt_hex) else {
        return false;
    };
    let Ok(expected) = decode_hex(&stored.hash_hex) else {
        return false;
    };
    if expected.len() != 32 {
        return false;
    }
    let actual = pbkdf2_hmac_sha256(password.as_bytes(), &salt, stored.iterations);
    constant_time_eq(&actual, &expected)
}

fn hash_password_with(password: &str, salt: &[u8], iterations: u32) -> PasswordHash {
    let hash = pbkdf2_hmac_sha256(password.as_bytes(), salt, iterations);
    PasswordHash {
        algorithm: "pbkdf2-hmac-sha256".into(),
        iterations,
        salt_hex: encode_hex(salt),
        hash_hex: encode_hex(&hash),
    }
}

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let (inner_key, outer_key) = prepare_hmac_key(password);
    let mut block = Vec::with_capacity(salt.len() + 4);
    block.extend_from_slice(salt);
    block.extend_from_slice(&1_u32.to_be_bytes());

    let mut u = hmac_sha256_prepared(&inner_key, &outer_key, &block);
    let mut output = u;
    for _ in 1..iterations {
        u = hmac_sha256_prepared(&inner_key, &outer_key, &u);
        for (target, byte) in output.iter_mut().zip(u) {
            *target ^= byte;
        }
    }
    output
}

fn prepare_hmac_key(key: &[u8]) -> ([u8; 64], [u8; 64]) {
    const BLOCK: usize = 64;
    let mut normalized = [0_u8; BLOCK];
    if key.len() > BLOCK {
        normalized[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }

    let mut inner_key = [0x36_u8; BLOCK];
    let mut outer_key = [0x5c_u8; BLOCK];
    for index in 0..BLOCK {
        inner_key[index] ^= normalized[index];
        outer_key[index] ^= normalized[index];
    }
    (inner_key, outer_key)
}

fn hmac_sha256_prepared(inner_key: &[u8; 64], outer_key: &[u8; 64], data: &[u8]) -> [u8; 32] {
    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner_hash);
    outer.finalize().into()
}

/// Performs the same expensive KDF work for a login attempt when no account exists.
/// This reduces the usefulness of local timing differences for account discovery.
pub fn consume_password_work(password: &str) {
    let _ = pbkdf2_hmac_sha256(password.as_bytes(), b"fileflow-login-dummy-salt", PASSWORD_ITERATIONS);
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, AuthError> {
    if !value.len().is_multiple_of(2) {
        return Err(AuthError::InvalidPasswordRecord);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(value: u8) -> Result<u8, AuthError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(AuthError::InvalidPasswordRecord),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip_rejects_wrong_secret() {
        let record = hash_password_with("correct horse battery staple", b"0123456789abcdef", 10_000);
        assert!(verify_password("correct horse battery staple", &record));
        assert!(!verify_password("incorrect password", &record));
    }

    #[test]
    fn validates_password_length() {
        assert!(validate_password("too-short").is_err());
        assert!(validate_password("long enough password").is_ok());
    }

    #[test]
    fn matches_known_pbkdf2_hmac_sha256_vector() {
        assert_eq!(
            encode_hex(&pbkdf2_hmac_sha256(b"password", b"salt", 2)),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn handles_long_passwords_without_changing_pbkdf2_result() {
        let password = "a".repeat(100);
        let hash = hash_password_with(&password, b"0123456789abcdef", 10_000);
        assert!(verify_password(&password, &hash));
    }
}
