/// Configuration & Operations Tests
/// Tests config management, feature flags, health check, and maintenance windows

#[cfg(test)]
mod config_ops_tests {
    use vortex::config_ops::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_config_create_environment() {
        let mgr = ConfigManager::new();
        let result = mgr.create_environment("production", "Prod env", None).await;
        assert!(result.is_ok());
        let envs = mgr.list_environments().await;
        assert!(envs.iter().any(|e| e.name == "production"));
    }

    #[tokio::test]
    async fn test_config_duplicate_environment_fails() {
        let mgr = ConfigManager::new();
        mgr.create_environment("staging", "Staging", None).await.unwrap();
        let result = mgr.create_environment("staging", "Dup", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_set_and_get_value() {
        let mgr = ConfigManager::new();
        mgr.create_environment("staging", "Staging", None).await.unwrap();

        let val = ConfigValue {
            value: serde_json::json!("staging-db.internal"),
            secret: false,
            description: "DB host".to_string(),
            source: ConfigSource::Override,
        };
        mgr.set_value("staging", "db.host", val).await.unwrap();

        let result = mgr.get_value("staging", "db.host").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().value, serde_json::json!("staging-db.internal"));
    }

    #[tokio::test]
    async fn test_config_inheritance() {
        let mgr = ConfigManager::new();
        mgr.create_environment("base", "Base env", None).await.unwrap();
        mgr.set_value("base", "app.version", ConfigValue {
            value: serde_json::json!("1.0.0"),
            secret: false,
            description: "Version".to_string(),
            source: ConfigSource::Default,
        }).await.unwrap();

        mgr.create_environment("dev", "Dev env", Some("base".to_string())).await.unwrap();

        // dev should inherit from base
        let val = mgr.get_value("dev", "app.version").await;
        assert!(val.is_some());
        assert_eq!(val.unwrap().value, serde_json::json!("1.0.0"));
    }

    #[tokio::test]
    async fn test_config_inheritance_override() {
        let mgr = ConfigManager::new();
        mgr.create_environment("base", "Base", None).await.unwrap();
        mgr.set_value("base", "key", ConfigValue {
            value: serde_json::json!("base_val"),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::Default,
        }).await.unwrap();

        mgr.create_environment("child", "Child", Some("base".to_string())).await.unwrap();
        mgr.set_value("child", "key", ConfigValue {
            value: serde_json::json!("child_val"),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::Override,
        }).await.unwrap();

        let val = mgr.get_value("child", "key").await;
        assert_eq!(val.unwrap().value, serde_json::json!("child_val"));
    }

    #[tokio::test]
    async fn test_config_lock_environment() {
        let mgr = ConfigManager::new();
        mgr.create_environment("locked_env", "Locked", None).await.unwrap();
        mgr.lock_environment("locked_env").await.unwrap();

        let result = mgr.set_value("locked_env", "key", ConfigValue {
            value: serde_json::json!("value"),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::Override,
        }).await;
        assert!(result.is_err(), "Should not allow writes to locked env");
    }

    #[tokio::test]
    async fn test_config_export_excludes_secrets() {
        let mgr = ConfigManager::new();
        mgr.create_environment("export_test", "Export test", None).await.unwrap();

        mgr.set_value("export_test", "db.host", ConfigValue {
            value: serde_json::json!("db.internal"),
            secret: false,
            description: "Host".to_string(),
            source: ConfigSource::Default,
        }).await.unwrap();

        mgr.set_value("export_test", "db.password", ConfigValue {
            value: serde_json::json!("s3cret!"),
            secret: true,
            description: "Password".to_string(),
            source: ConfigSource::Vault,
        }).await.unwrap();

        let exported = mgr.export_environment("export_test").await.unwrap();
        assert!(exported.contains_key("db.host"));
        assert!(
            !exported.contains_key("db.password"),
            "Secrets should be excluded from export"
        );
    }

    #[tokio::test]
    async fn test_feature_flags_create_and_check() {
        let mgr = FeatureFlagManager::new();
        mgr.create_flag("dark_mode", "Enable dark mode UI").await.unwrap();

        assert!(!mgr.is_enabled("dark_mode", "production").await);

        mgr.toggle("dark_mode", true).await.unwrap();
        // Empty allowed_environments = enabled for all envs
        assert!(mgr.is_enabled("dark_mode", "production").await);
    }

    #[tokio::test]
    async fn test_feature_flags_toggle() {
        let mgr = FeatureFlagManager::new();
        mgr.create_flag("feature_x", "Feature X").await.unwrap();

        mgr.toggle("feature_x", true).await.unwrap();
        assert!(mgr.is_enabled("feature_x", "any").await);

        mgr.toggle("feature_x", false).await.unwrap();
        assert!(!mgr.is_enabled("feature_x", "any").await);
    }

    #[tokio::test]
    async fn test_feature_flags_rollout() {
        let mgr = FeatureFlagManager::new();
        mgr.create_flag("gradual", "Gradual rollout").await.unwrap();
        mgr.toggle("gradual", true).await.unwrap();
        mgr.set_rollout("gradual", 100).await.unwrap();
        // Should be enabled
        assert!(mgr.is_enabled("gradual", "prod").await);
    }

    #[tokio::test]
    async fn test_feature_flags_invalid_rollout() {
        let mgr = FeatureFlagManager::new();
        mgr.create_flag("test_flag", "Test").await.unwrap();
        let result = mgr.set_rollout("test_flag", 101).await;
        assert!(result.is_err(), "Percentage > 100 should fail");
    }

    #[tokio::test]
    async fn test_feature_flags_list() {
        let mgr = FeatureFlagManager::new();
        mgr.create_flag("flag_a", "First flag").await.unwrap();
        mgr.create_flag("flag_b", "Second flag").await.unwrap();

        let flags = mgr.list_flags().await;
        assert_eq!(flags.len(), 2);
    }

    #[tokio::test]
    async fn test_health_checker_full_report() {
        let checker = HealthChecker::new();
        let report = checker.full_health_report().await;
        assert!(!report.checks.is_empty(), "Should have health checks");
        assert!(report.overall_healthy);
        // Standard checks: Database, Scheduler, Workers, GrpcSwarm, DiskSpace, Memory, QueueDepth
        assert_eq!(report.checks.len(), 7);
    }

    #[tokio::test]
    async fn test_health_checker_run_individual() {
        let checker = HealthChecker::new();
        let result = checker.run_check(HealthCheckType::Database).await;
        assert!(result.healthy);
        assert_eq!(result.check_type, HealthCheckType::Database);
    }

    #[tokio::test]
    async fn test_ops_manager_maintenance_window() {
        let ops = OpsManager::new();

        ops.schedule_maintenance(MaintenanceWindow {
            id: "mw-1".to_string(),
            description: "Database upgrade".to_string(),
            start: chrono::Utc::now() - chrono::Duration::hours(1),
            end: chrono::Utc::now() + chrono::Duration::hours(1),
            suppress_alerts: true,
            pause_scheduling: true,
            created_by: "admin".to_string(),
        }).await.unwrap();

        let active = ops.is_in_maintenance().await;
        assert!(active.is_some(), "Should be in maintenance window");

        let windows = ops.list_windows().await;
        assert_eq!(windows.len(), 1);
    }

    #[tokio::test]
    async fn test_ops_manager_invalid_window() {
        let ops = OpsManager::new();
        let result = ops.schedule_maintenance(MaintenanceWindow {
            id: "bad".to_string(),
            description: "Invalid".to_string(),
            start: chrono::Utc::now() + chrono::Duration::hours(1),
            end: chrono::Utc::now() - chrono::Duration::hours(1), // end before start
            suppress_alerts: false,
            pause_scheduling: false,
            created_by: "admin".to_string(),
        }).await;
        assert!(result.is_err(), "Should reject window with end before start");
    }

    #[tokio::test]
    async fn test_ops_manager_cancel_maintenance() {
        let ops = OpsManager::new();

        ops.schedule_maintenance(MaintenanceWindow {
            id: "mw-cancel".to_string(),
            description: "Test".to_string(),
            start: chrono::Utc::now() - chrono::Duration::hours(1),
            end: chrono::Utc::now() + chrono::Duration::hours(1),
            suppress_alerts: false,
            pause_scheduling: false,
            created_by: "admin".to_string(),
        }).await.unwrap();

        let result = ops.cancel_maintenance("mw-cancel").await;
        assert!(result.is_ok());

        let windows = ops.list_windows().await;
        assert!(windows.is_empty());
    }

    #[tokio::test]
    async fn test_ops_manager_cancel_nonexistent() {
        let ops = OpsManager::new();
        let result = ops.cancel_maintenance("does-not-exist").await;
        assert!(result.is_err());
    }
}
