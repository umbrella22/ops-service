//! AWS Signature Version 4 签名实现（共享给 ops-service 与 ops-runner）
//!
//! 支持：
//! - [`signed_headers`]: 为 PUT/GET 等请求计算需要携带的签名头
//!   （Authorization / x-amz-date / x-amz-content-sha256）
//! - [`presigned_url`]: 生成带签名的预签名 URL（payload 一律 UNSIGNED-PAYLOAD）
//!
//! HMAC-SHA256 手工实现以避免额外依赖（与 ops-service webhook_hmac 相同模式），
//! 签名密钥派生使用 AWS 官方文档测试向量校验（见单元测试）。

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 64;

    let key_material: Vec<u8> = if key.len() > BLOCK_SIZE {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.finalize().to_vec()
    } else {
        key.to_vec()
    };

    let mut key_padded = [0u8; BLOCK_SIZE];
    key_padded[..key_material.len()].copy_from_slice(&key_material);

    let mut inner_key = [0u8; BLOCK_SIZE];
    let mut outer_key = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        inner_key[i] = key_padded[i] ^ 0x36;
        outer_key[i] = key_padded[i] ^ 0x5c;
    }

    let inner_hash = {
        let mut hasher = Sha256::new();
        hasher.update(inner_key);
        hasher.update(data);
        hasher.finalize()
    };

    let mut hasher = Sha256::new();
    hasher.update(outer_key);
    hasher.update(inner_hash);
    hasher.finalize().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// SigV4 签名密钥派生：HMAC(HMAC(HMAC(HMAC("AWS4"+secret, date), region), service), "aws4_request")
fn signing_key(secret_key: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// RFC3986 unreserved 字符之外的字符全部转义（SigV4 要求）
fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// 解析 URL 为 (host, path, query)
fn parse_url(url: &str) -> Result<(String, String, String), String> {
    let without_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| format!("URL missing scheme: {}", url))?;

    let (authority, rest) = without_scheme
        .split_once('/')
        .map(|(a, r)| (a.to_string(), format!("/{}", r)))
        .unwrap_or_else(|| (without_scheme.to_string(), "/".to_string()));

    let (path, query) = rest
        .split_once('?')
        .map(|(p, q)| (p.to_string(), q.to_string()))
        .unwrap_or_else(|| (rest, String::new()));

    Ok((authority, path, query))
}

fn canonical_query(raw_query: &str) -> String {
    if raw_query.is_empty() {
        return String::new();
    }
    let mut params: Vec<(String, String)> = raw_query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (uri_encode(k, true), uri_encode(v, true)),
            None => (uri_encode(p, true), String::new()),
        })
        .collect();
    params.sort();
    params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&")
}

#[allow(clippy::too_many_arguments)]
fn build_authorization(
    method: &str,
    url: &str,
    payload_hash: &str,
    extra_headers: &[(String, String)],
    access_key: &str,
    secret_key: &str,
    region: &str,
    service: &str,
    now: DateTime<Utc>,
) -> Result<(String, String, String, String), String> {
    let (host, path, query) = parse_url(url)?;
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();

    // 规范头：host + x-amz-* + extra，按小写键排序
    let mut headers: Vec<(String, String)> = vec![
        ("host".to_string(), host.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    for (k, v) in extra_headers {
        headers.push((k.to_lowercase(), v.trim().to_string()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    headers.dedup_by(|a, b| a.0 == b.0);

    let canonical_headers = headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v))
        .collect::<String>();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        uri_encode(&path, false),
        canonical_query(&query),
        canonical_headers,
        signed_headers,
        payload_hash
    );

    let credential_scope = format!("{}/{}/{}/aws4_request", date_stamp, region, service);
    let string_to_sign = format!(
        "{}\n{}\n{}\n{}",
        ALGORITHM,
        amz_date,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );

    let key = signing_key(secret_key, &date_stamp, region, service);
    let signature = hex::encode(hmac_sha256(&key, string_to_sign.as_bytes()));

    let authorization = format!(
        "{} Credential={}/{}, SignedHeaders={}, Signature={}",
        ALGORITHM, access_key, credential_scope, signed_headers, signature
    );

    Ok((authorization, amz_date, payload_hash.to_string(), host))
}

/// 为请求计算 SigV4 签名头。
///
/// 返回需要设置的头：Authorization、x-amz-date、x-amz-content-sha256。
/// Host 头由 HTTP 客户端自动设置，但其值必须与 `url` 的 authority 一致。
#[allow(clippy::too_many_arguments)]
pub fn signed_headers(
    method: &str,
    url: &str,
    region: &str,
    service: &str,
    access_key: &str,
    secret_key: &str,
    payload_hash: &str,
    extra_headers: &[(String, String)],
) -> Result<Vec<(String, String)>, String> {
    let (authorization, amz_date, content_sha, _) = build_authorization(
        method,
        url,
        payload_hash,
        extra_headers,
        access_key,
        secret_key,
        region,
        service,
        Utc::now(),
    )?;
    Ok(vec![
        ("Authorization".to_string(), authorization),
        ("x-amz-date".to_string(), amz_date),
        ("x-amz-content-sha256".to_string(), content_sha),
    ])
}

/// 生成 SigV4 预签名 URL（UNSIGNED-PAYLOAD）。
#[allow(clippy::too_many_arguments)]
pub fn presigned_url(
    method: &str,
    url: &str,
    region: &str,
    service: &str,
    access_key: &str,
    secret_key: &str,
    expires_secs: u64,
) -> Result<String, String> {
    let (host, path, existing_query) = parse_url(url)?;
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let credential_scope = format!("{}/{}/{}/aws4_request", date_stamp, region, service);
    let credential = format!("{}/{}", access_key, credential_scope);

    let mut params: Vec<(String, String)> = vec![
        (
            "X-Amz-Algorithm".to_string(),
            ALGORITHM.to_string(),
        ),
        ("X-Amz-Credential".to_string(), credential),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), expires_secs.to_string()),
        ("X-Amz-SignedHeaders".to_string(), "host".to_string()),
    ];
    if !existing_query.is_empty() {
        for p in existing_query.split('&').filter(|p| !p.is_empty()) {
            match p.split_once('=') {
                Some((k, v)) => params.push((k.to_string(), v.to_string())),
                None => params.push((p.to_string(), String::new())),
            }
        }
    }
    params.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query_str = params
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_request = format!(
        "{}\n{}\n{}\nhost:{}\nhost\nUNSIGNED-PAYLOAD",
        method.to_uppercase(),
        uri_encode(&path, false),
        canonical_query_str,
        host
    );

    let string_to_sign = format!(
        "{}\n{}\n{}\n{}",
        ALGORITHM,
        amz_date,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );

    let key = signing_key(secret_key, &date_stamp, region, service);
    let signature = hex::encode(hmac_sha256(&key, string_to_sign.as_bytes()));

    Ok(format!(
        "{}://{}{}?{}&X-Amz-Signature={}",
        url.split("://").next().unwrap_or("https"),
        host,
        uri_encode(&path, false),
        canonical_query_str,
        signature
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS 官方文档中的签名密钥派生测试向量
    /// (secret=wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY, 20120215, us-east-1, iam)
    #[test]
    fn test_signing_key_aws_vector() {
        let key = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20120215",
            "us-east-1",
            "iam",
        );
        assert_eq!(
            hex::encode(key),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }

    #[test]
    fn test_hmac_sha256_vector() {
        // RFC 4231 Test Case 2
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        assert_eq!(
            hex::encode(hmac_sha256(key, data)),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn test_uri_encode() {
        assert_eq!(uri_encode("a b/c+d", false), "a%20b/c%2Bd");
        assert_eq!(uri_encode("a b/c+d", true), "a%20b%2Fc%2Bd");
        assert_eq!(uri_encode("bucket/key-1.0_~", false), "bucket/key-1.0_~");
    }

    #[test]
    fn test_parse_url() {
        let (host, path, query) = parse_url("http://minio:9000/bucket/a/b?x=1&y=2").unwrap();
        assert_eq!(host, "minio:9000");
        assert_eq!(path, "/bucket/a/b");
        assert_eq!(query, "x=1&y=2");
    }

    #[test]
    fn test_presigned_url_structure() {
        let url = presigned_url(
            "GET",
            "https://s3.example.com/bucket/key with space.bin",
            "us-east-1",
            "s3",
            "AKID",
            "SECRET",
            600,
        )
        .unwrap();
        assert!(url.starts_with("https://s3.example.com/bucket/key%20with%20space.bin?"));
        assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(url.contains("X-Amz-SignedHeaders=host"));
        assert!(url.contains("X-Amz-Expires=600"));
        assert!(url.contains("X-Amz-Signature="));
        // 签名为 64 位 hex
        let sig = url.rsplit("X-Amz-Signature=").next().unwrap();
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
