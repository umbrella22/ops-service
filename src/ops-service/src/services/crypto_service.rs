//! 主机 SSH 凭据落盘加密（AES-256-GCM）
//!
//! 存储格式：`enc:v1:<base64(nonce || ciphertext)>`
//! - 写入路径（create/update host）统一经 [`encrypt`] 处理；
//! - 读取路径统一经 [`decrypt`] 处理：不带 `enc:v1:` 前缀的值按历史明文
//!   原样返回，保证灰度期间旧数据可用；
//! - 密钥来自 `OPS_SECURITY__CREDENTIAL_ENCRYPTION_KEY`（任意 >=32 字符
//!   输入，经 SHA-256 派生为 32 字节 AES 密钥）。未配置时加密层退化为
//!   明文直存（启动时有警告）。

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::generic_array::GenericArray,
    aead::Aead,
};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use tokio::sync::RwLock;

use crate::error::AppError;

const PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

static KEY: OnceLock<RwLock<Option<[u8; 32]>>> = OnceLock::new();

fn key_slot() -> &'static RwLock<Option<[u8; 32]>> {
    KEY.get_or_init(|| RwLock::new(None))
}

/// 启动时注入加密密钥（由 main 在加载配置后调用）。
/// 输入为 64 位 hex 时直接解码为 32 字节；否则用 SHA-256 派生。
pub async fn init(key: Option<String>) {
    let derived = key.map(|raw| {
        if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
            let mut out = [0u8; 32];
            for i in 0..32 {
                out[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).unwrap_or(0);
            }
            out
        } else {
            let mut hasher = Sha256::new();
            hasher.update(raw.as_bytes());
            hasher.finalize().into()
        }
    });
    *key_slot().write().await = derived;
}

fn cipher(key: &[u8; 32]) -> Result<Aes256Gcm, AppError> {
    Aes256Gcm::new_from_slice(key).map_err(|e| {
        tracing::error!("Failed to init credential cipher: {:?}", e);
        AppError::internal_error("Credential cipher init failed")
    })
}

/// 加密明文；未配置密钥时原样返回（与历史行为兼容，启动时有警告）
pub async fn encrypt(plaintext: &str) -> Result<String, AppError> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let guard = key_slot().read().await;
    let Some(key) = guard.as_ref() else {
        return Ok(plaintext.to_string());
    };

    let enc = cipher(key)?;
    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let nonce = GenericArray::from_slice(&nonce_bytes);

    let ciphertext = enc
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| {
            tracing::error!("Credential encryption failed: {:?}", e);
            AppError::internal_error("Credential encryption failed")
        })?;

    let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(format!(
        "{}{}",
        PREFIX,
        base64::engine::general_purpose::STANDARD.encode(payload)
    ))
}

/// 解密；不带加密前缀的值按历史明文返回
pub async fn decrypt(stored: &str) -> Result<String, AppError> {
    let Some(encoded) = stored.strip_prefix(PREFIX) else {
        return Ok(stored.to_string());
    };
    if encoded.is_empty() {
        return Ok(String::new());
    }

    let guard = key_slot().read().await;
    let Some(key) = guard.as_ref() else {
        tracing::error!("Encrypted credential found but no encryption key configured");
        return Err(AppError::internal_error(
            "Encrypted credential requires OPS_SECURITY__CREDENTIAL_ENCRYPTION_KEY",
        ));
    };

    let payload = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| {
            tracing::error!("Credential payload is not valid base64: {:?}", e);
            AppError::internal_error("Corrupted credential payload")
        })?;

    if payload.len() < NONCE_LEN {
        return Err(AppError::internal_error("Corrupted credential payload"));
    }

    let enc = cipher(key)?;
    let plaintext = enc
        .decrypt(GenericArray::from_slice(&payload[..NONCE_LEN]), &payload[NONCE_LEN..])
        .map_err(|e| {
            tracing::error!("Credential decryption failed (wrong key?): {:?}", e);
            AppError::internal_error("Credential decryption failed")
        })?;

    String::from_utf8(plaintext).map_err(|_| {
        AppError::internal_error("Credential payload is not valid UTF-8")
    })
}

/// 判断值是否为密文形式
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_roundtrip_with_key() {
        init(Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string())).await;
        let plain = "BEGIN OPENSSH PRIVATE KEY-----\nabc";
        let enc = encrypt(plain).await.unwrap();
        assert!(is_encrypted(&enc));
        assert!(!enc.contains("OPENSSH"));
        assert_eq!(decrypt(&enc).await.unwrap(), plain);
    }

    #[tokio::test]
    async fn test_plaintext_passthrough_without_key() {
        init(None).await;
        let plain = "secret-password";
        let out = encrypt(plain).await.unwrap();
        assert_eq!(out, plain);
        assert!(!is_encrypted(&out));
    }

    #[tokio::test]
    async fn test_legacy_plaintext_decrypt() {
        init(Some("legacy-passphrase-at-least-32-chars!!".to_string())).await;
        assert_eq!(decrypt("old-plaintext-cred").await.unwrap(), "old-plaintext-cred");
    }

    #[tokio::test]
    async fn test_empty_value_untouched() {
        init(Some("another-key-value-at-least-32-chars".to_string())).await;
        assert_eq!(encrypt("").await.unwrap(), "");
        assert_eq!(decrypt("").await.unwrap(), "");
    }
}
