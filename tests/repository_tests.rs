//! 仓库层单元测试

use ops_system::repository::{UserRepository, RoleRepository, AssetRepository, AuditRepository};
use ops_system::models::{user::*, role::*, asset::*, audit::*};
use uuid::Uuid;

mod common;
use common::{create_test_config, create_test_user, create_test_role};

#[tokio::test]
async fn test_user_repository_create_and_find() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let user_repo = UserRepository::new(pool.clone());

    // 创建用户
    let user = User {
        id: Uuid::new_v4(),
        username: "testuser".to_string(),
        password_hash: "hash123".to_string(),
        email: Some("test@example.com".to_string()),
        display_name: Some("Test User".to_string()),
        failed_login_attempts: 0,
        locked_until: None,
        last_login_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    user_repo.create(&user).await.unwrap();

    // 查找用户
    let found_user = user_repo
        .find_by_username("testuser")
        .await
        .unwrap()
        .expect("User not found");

    assert_eq!(found_user.username, "testuser");
    assert_eq!(found_user.email, Some("test@example.com".to_string()));
}

#[tokio::test]
async fn test_user_repository_find_by_id() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let user_id = create_test_user(&pool, "testuser", "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let user_repo = UserRepository::new(pool.clone());

    let found_user = user_repo
        .find_by_id(user_id)
        .await
        .unwrap()
        .expect("User not found");

    assert_eq!(found_user.id, user_id);
    assert_eq!(found_user.username, "testuser");
}

#[tokio::test]
async fn test_user_repository_update_failed_attempts() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let user_id = create_test_user(&pool, "testuser", "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let user_repo = UserRepository::new(pool.clone());

    // 增加失败次数
    user_repo.increment_failed_attempts(user_id).await.unwrap();

    let user = user_repo
        .find_by_id(user_id)
        .await
        .unwrap()
        .expect("User not found");

    assert_eq!(user.failed_login_attempts, 1);

    // 重置失败次数
    user_repo.reset_failed_attempts(user_id).await.unwrap();

    let user = user_repo
        .find_by_id(user_id)
        .await
        .unwrap()
        .expect("User not found");

    assert_eq!(user.failed_login_attempts, 0);
}

#[tokio::test]
async fn test_role_repository_create_and_find() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let role_repo = RoleRepository::new(pool.clone());

    let role = Role {
        id: Uuid::new_v4(),
        name: "admin".to_string(),
        description: Some("Administrator".to_string()),
        permissions: vec!["read".to_string(), "write".to_string()],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    role_repo.create(&role).await.unwrap();

    // 查找角色
    let found_role = role_repo
        .find_by_name("admin")
        .await
        .unwrap()
        .expect("Role not found");

    assert_eq!(found_role.name, "admin");
    assert_eq!(found_role.description, Some("Administrator".to_string()));
}

#[tokio::test]
async fn test_asset_repository_create_group() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let asset_repo = AssetRepository::new(pool.clone());

    let req = CreateGroupRequest {
        name: "production".to_string(),
        environment: "production".to_string(),
        description: Some("Production servers".to_string()),
    };

    let group = asset_repo
        .create_group(&req, Uuid::new_v4())
        .await
        .unwrap();

    assert_eq!(group.name, "production");
    assert_eq!(group.environment, "production");
}

#[tokio::test]
async fn test_asset_repository_create_host() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let asset_repo = AssetRepository::new(pool.clone());

    // 先创建组
    let group_req = CreateGroupRequest {
        name: "production".to_string(),
        environment: "production".to_string(),
        description: Some("Production servers".to_string()),
    };
    let group = asset_repo
        .create_group(&group_req, Uuid::new_v4())
        .await
        .unwrap();

    // 创建主机
    let host_req = CreateHostRequest {
        identifier: "web-01".to_string(),
        display_name: Some("Web Server 01".to_string()),
        group_id: Some(group.id),
        environment: Some("production".to_string()),
        address: "192.168.1.100".to_string(),
        port: 22,
        tags: Some(vec!["web".to_string(), "linux".to_string()]),
        owner_id: Some(Uuid::new_v4()),
        status: "active".to_string(),
        notes: None,
        os_type: Some("linux".to_string()),
        os_version: None,
    };

    let host = asset_repo
        .create_host(&host_req, Uuid::new_v4())
        .await
        .unwrap();

    assert_eq!(host.identifier, "web-01");
    assert_eq!(host.address, "192.168.1.100");
    assert_eq!(host.status, "active");
}

#[tokio::test]
async fn test_asset_repository_list_hosts() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let asset_repo = AssetRepository::new(pool.clone());

    // 创建组
    let group_req = CreateGroupRequest {
        name: "production".to_string(),
        environment: "production".to_string(),
        description: None,
    };
    let group = asset_repo
        .create_group(&group_req, Uuid::new_v4())
        .await
        .unwrap();

    // 创建多个主机
    for i in 0..3 {
        let host_req = CreateHostRequest {
            identifier: format!("web-{:02}", i),
            display_name: Some(format!("Web Server {:02}", i)),
            group_id: Some(group.id),
            environment: Some("production".to_string()),
            address: format!("192.168.1.{}", 100 + i),
            port: 22,
            tags: Some(vec!["web".to_string()]),
            owner_id: None,
            status: "active".to_string(),
            notes: None,
            os_type: Some("linux".to_string()),
            os_version: None,
        };

        asset_repo
            .create_host(&host_req, Uuid::new_v4())
            .await
            .unwrap();
    }

    // 列出主机
    let filters = HostListFilters {
        group_id: Some(group.id),
        environment: Some("production".to_string()),
        status: Some("active".to_string()),
        tags: None,
        search: None,
    };

    let hosts = asset_repo
        .list_hosts(&filters, 10, 0)
        .await
        .unwrap();

    assert_eq!(hosts.len(), 3);

    let count = asset_repo.count_hosts(&filters).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_audit_repository_log_and_retrieve() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let audit_repo = AuditRepository::new(pool.clone());

    let user_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();

    // 记录审计日志
    let log = AuditLog {
        id: Uuid::new_v4(),
        subject_id: Some(user_id),
        subject_type: Some("user".to_string()),
        subject_name: Some("testuser".to_string()),
        resource_type: Some("asset".to_string()),
        resource_id: Some(resource_id.to_string()),
        action: "create".to_string(),
        result: "success".to_string(),
        details: None,
        occurred_at: chrono::Utc::now(),
        trace_id: Some("test-trace-123".to_string()),
        ip_address: Some("127.0.0.1".to_string()),
        user_agent: Some("test-agent".to_string()),
    };

    audit_repo.create_log(&log).await.unwrap();

    // 查询审计日志
    let filters = AuditLogFilters {
        subject_id: Some(user_id),
        resource_type: Some("asset".to_string()),
        action: Some("create".to_string()),
        result: Some("success".to_string()),
        start_time: None,
        end_time: None,
        trace_id: Some("test-trace-123".to_string()),
        limit: Some(10),
        offset: Some(0),
    };

    let logs = audit_repo
        .list_audit_logs(&filters, 10, 0)
        .await
        .unwrap();

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].action, "create");
    assert_eq!(logs[0].trace_id, Some("test-trace-123".to_string()));
}

#[tokio::test]
async fn test_audit_repository_count() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let audit_repo = AuditRepository::new(pool.clone());

    let user_id = Uuid::new_v4();

    // 创建多条审计日志
    for i in 0..5 {
        let log = AuditLog {
            id: Uuid::new_v4(),
            subject_id: Some(user_id),
            subject_type: Some("user".to_string()),
            subject_name: Some("testuser".to_string()),
            resource_type: Some("asset".to_string()),
            resource_id: Some(Uuid::new_v4().to_string()),
            action: "create".to_string(),
            result: "success".to_string(),
            details: None,
            occurred_at: chrono::Utc::now(),
            trace_id: Some(format!("trace-{}", i)),
            ip_address: Some("127.0.0.1".to_string()),
            user_agent: Some("test-agent".to_string()),
        };

        audit_repo.create_log(&log).await.unwrap();
    }

    let filters = AuditLogFilters {
        subject_id: Some(user_id),
        resource_type: None,
        action: None,
        result: None,
        start_time: None,
        end_time: None,
        trace_id: None,
        limit: None,
        offset: None,
    };

    let count = audit_repo.count_audit_logs(&filters).await.unwrap();
    assert_eq!(count, 5);
}
