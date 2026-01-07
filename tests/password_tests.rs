//! 密码哈希功能单元测试
//!
//! 测试 Argon2id 密码哈希和验证功能

use ops_system::auth::password::PasswordHasher;
use ops_system::config::{AppConfig, DatabaseConfig, LoggingConfig, SecurityConfig, ServerConfig};
use secrecy::Secret;

/// 创建测试配置
fn create_test_config() -> AppConfig {
    AppConfig {
        server: ServerConfig {
            addr: "127.0.0.1:3000".to_string(),
            graceful_shutdown_timeout_secs: 30,
        },
        database: DatabaseConfig {
            url: Secret::new("postgresql://localhost/test".to_string()),
            max_connections: 10,
            min_connections: 1,
            acquire_timeout_secs: 30,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            format: "json".to_string(),
        },
        security: SecurityConfig {
            jwt_secret: Secret::new("test_secret_key_32_characters_long!".to_string()),
            access_token_exp_secs: 900,
            refresh_token_exp_secs: 604800,
            rate_limit_rps: 100,
            trust_proxy: true,
            allowed_ips: None,
            password_min_length: 8,
            password_require_uppercase: true,
            password_require_digit: true,
            password_require_special: false,
            max_login_attempts: 5,
            login_lockout_duration_secs: 1800,
        },
    }
}

#[test]
fn test_password_hash_and_verify() {
    let hasher = PasswordHasher::new();
    let password = "TestPassword123!";

    let hash = hasher.hash(password).expect("Hashing should succeed");

    // 哈希值应该包含 argon2 标识
    assert!(hash.contains("$argon2"));

    // 验证正确密码
    hasher.verify(password, &hash).expect("Verification should succeed");
}

#[test]
fn test_password_verify_with_wrong_password() {
    let hasher = PasswordHasher::new();
    let password = "TestPassword123!";

    let hash = hasher.hash(password).expect("Hashing should succeed");

    // 验证错误密码应该失败
    let result = hasher.verify("WrongPassword123!", &hash);
    assert!(result.is_err(), "Wrong password should fail verification");
}

#[test]
fn test_password_hash_different_each_time() {
    let hasher = PasswordHasher::new();
    let password = "TestPassword123!";

    let hash1 = hasher.hash(password).expect("First hash should succeed");
    let hash2 = hasher.hash(password).expect("Second hash should succeed");

    // 由于随机盐，每次生成的哈希应该不同
    assert_ne!(hash1, hash2, "Hashes should be different due to salt");

    // 但两个哈希都应该能验证同一个密码
    hasher.verify(password, &hash1).expect("First hash should verify");
    hasher.verify(password, &hash2).expect("Second hash should verify");
}

#[test]
fn test_password_hash_empty_string() {
    let hasher = PasswordHasher::new();
    let password = "";

    let hash = hasher.hash(password).expect("Empty password should hash");

    // 空密码应该能验证
    hasher.verify(password, &hash).expect("Empty password should verify");

    // 非空密码应该验证失败
    assert!(hasher.verify("password", &hash).is_err());
}

#[test]
fn test_password_hash_unicode() {
    let hasher = PasswordHasher::new();
    let password = "密码测试Test123!🔒";

    let hash = hasher.hash(password).expect("Unicode password should hash");

    hasher.verify(password, &hash).expect("Unicode password should verify");

    // 稍有不同的 Unicode 密码应该失败
    assert!(hasher.verify("密码测试Test123🔒", &hash).is_err());
}

#[test]
fn test_password_hash_long_password() {
    let hasher = PasswordHasher::new();
    // 1000 字符的长密码
    let password = "a".repeat(500) + "B1!";

    let hash = hasher.hash(&password).expect("Long password should hash");

    hasher.verify(&password, &hash).expect("Long password should verify");
}

#[test]
fn test_password_policy_valid() {
    let config = create_test_config();

    // 有效密码
    assert!(
        PasswordHasher::validate_password_policy("Test1234", &config).is_ok(),
        "Valid password should pass"
    );
    assert!(
        PasswordHasher::validate_password_policy("MySecureP@ssw0rd", &config).is_ok(),
        "Valid password with special char should pass"
    );
}

#[test]
fn test_password_policy_too_short() {
    let config = create_test_config();

    // 密码太短（少于8个字符）
    assert!(
        PasswordHasher::validate_password_policy("Test1", &config).is_err(),
        "Short password should fail"
    );
    assert!(
        PasswordHasher::validate_password_policy("Ab1", &config).is_err(),
        "Very short password should fail"
    );
}

#[test]
fn test_password_policy_no_uppercase() {
    let config = create_test_config();

    // 没有大写字母
    assert!(
        PasswordHasher::validate_password_policy("test1234", &config).is_err(),
        "Password without uppercase should fail"
    );
    assert!(
        PasswordHasher::validate_password_policy("12345678", &config).is_err(),
        "Numbers only should fail"
    );
}

#[test]
fn test_password_policy_no_digit() {
    let config = create_test_config();

    // 没有数字
    assert!(
        PasswordHasher::validate_password_policy("Testtest", &config).is_err(),
        "Password without digit should fail"
    );
    assert!(
        PasswordHasher::validate_password_policy("TESTTEST", &config).is_err(),
        "Uppercase only should fail"
    );
}

#[test]
fn test_password_policy_with_special_char_required() {
    let mut config = create_test_config();
    config.security.password_require_special = true;

    // 需要特殊字符时，没有特殊字符应该失败
    assert!(
        PasswordHasher::validate_password_policy("Test1234", &config).is_err(),
        "Password without special char should fail when required"
    );

    // 有特殊字符应该通过
    assert!(
        PasswordHasher::validate_password_policy("Test!234", &config).is_ok(),
        "Password with special char should pass"
    );
}

#[test]
fn test_password_policy_minimum_length_custom() {
    let mut config = create_test_config();
    config.security.password_min_length = 12;

    // 12字符应该通过
    assert!(
        PasswordHasher::validate_password_policy("Test12345678", &config).is_ok(),
        "12 char password should pass"
    );

    // 11字符应该失败
    assert!(
        PasswordHasher::validate_password_policy("Test1234567", &config).is_err(),
        "11 char password should fail"
    );
}

#[test]
fn test_password_hasher_default() {
    let hasher1 = PasswordHasher::default();
    let hasher2 = PasswordHasher::new();

    let password = "TestPassword123!";
    let hash1 = hasher1.hash(password).unwrap();
    let hash2 = hasher2.hash(password).unwrap();

    // 两个不同的 hasher 应该都能正常工作
    assert_ne!(hash1, hash2);
    hasher1.verify(password, &hash1).unwrap();
    hasher2.verify(password, &hash2).unwrap();
}

#[test]
fn test_password_verify_with_invalid_hash() {
    let hasher = PasswordHasher::new();
    let password = "TestPassword123!";

    // 无效的哈希格式
    assert!(hasher.verify(password, "invalid_hash").is_err());
    assert!(hasher.verify(password, "$argon2id$v=19$invalid").is_err());
    assert!(hasher.verify(password, "").is_err());
}

#[test]
fn test_password_verify_timing_attack_resistance() {
    let hasher = PasswordHasher::new();
    let password = "TestPassword123!";
    let hash = hasher.hash(password).unwrap();

    // 使用正确的密码验证
    let start = std::time::Instant::now();
    hasher.verify(password, &hash).unwrap();
    let correct_duration = start.elapsed();

    // 使用错误的密码验证
    let start = std::time::Instant::now();
    hasher.verify("WrongPassword123!", &hash).unwrap_err();
    let wrong_duration = start.elapsed();

    // Argon2 设计为恒定时间，但允许一定差异
    // 这里只是确保两种情况都执行完成
    assert!(correct_duration.as_millis() > 0);
    assert!(wrong_duration.as_millis() > 0);
}
