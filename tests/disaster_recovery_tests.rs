/// Disaster Recovery Tests
/// Tests backup/restore, failover, chaos engine, and recovery automation

#[cfg(test)]
mod disaster_recovery_tests {
    use ryuo::disaster_recovery::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_backup_create_and_list() {
        let config = BackupConfig {
            storage_path: "/tmp/ryuo_test_backups".to_string(),
            max_backups: 10,
            ..BackupConfig::default()
        };
        let mgr = BackupManager::new(config);
        let result = mgr.create_backup(BackupTarget::Full).await;
        assert!(result.is_ok(), "Create backup failed: {:?}", result.err());
        let backup = result.unwrap();
        assert_eq!(backup.target, BackupTarget::Full);
        assert!(!backup.id.is_empty());

        let backups = mgr.list_backups(None).await;
        assert!(!backups.is_empty());
    }

    #[tokio::test]
    async fn test_backup_max_limit_enforcement() {
        let config = BackupConfig {
            storage_path: "/tmp/ryuo_test_backups_limit".to_string(),
            max_backups: 3,
            ..BackupConfig::default()
        };
        let mgr = BackupManager::new(config);
        for _ in 0..5 {
            let _ = mgr.create_backup(BackupTarget::Database).await;
        }
        let backups = mgr.list_backups(None).await;
        assert!(
            backups.len() <= 3,
            "Should enforce max_backups limit, got {}",
            backups.len()
        );
    }

    #[tokio::test]
    async fn test_backup_filter_by_target() {
        let config = BackupConfig {
            storage_path: "/tmp/ryuo_test_filter".to_string(),
            max_backups: 10,
            ..BackupConfig::default()
        };
        let mgr = BackupManager::new(config);
        mgr.create_backup(BackupTarget::Database).await.unwrap();
        mgr.create_backup(BackupTarget::Configuration).await.unwrap();
        mgr.create_backup(BackupTarget::Database).await.unwrap();

        let db_backups = mgr.list_backups(Some(BackupTarget::Database)).await;
        assert!(db_backups.len() >= 2);
        assert!(db_backups.iter().all(|b| b.target == BackupTarget::Database));
    }

    #[tokio::test]
    async fn test_failover_register_and_heartbeat() {
        let mgr = FailoverManager::new(30);
        let node1 = ClusterNode {
            node_id: "node-1".to_string(),
            address: "10.0.0.1:8080".to_string(),
            role: NodeRole::Primary,
            health: NodeHealth::Healthy,
            last_heartbeat: chrono::Utc::now(),
            metadata: HashMap::new(),
        };
        let node2 = ClusterNode {
            node_id: "node-2".to_string(),
            address: "10.0.0.2:8080".to_string(),
            role: NodeRole::Secondary,
            health: NodeHealth::Healthy,
            last_heartbeat: chrono::Utc::now(),
            metadata: HashMap::new(),
        };
        mgr.register_node(node1).await;
        mgr.register_node(node2).await;

        let status = mgr.cluster_status().await;
        assert_eq!(status.len(), 2);

        mgr.heartbeat("node-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_failover_heartbeat_unknown_node() {
        let mgr = FailoverManager::new(30);
        let result = mgr.heartbeat("unknown-node").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_chaos_engine_register_and_run() {
        let engine = ChaosEngine::new();
        engine
            .register_experiment(ChaosExperiment {
                id: "test-exp-1".to_string(),
                name: "Kill Worker".to_string(),
                experiment_type: ChaosExperimentType::ProcessKill,
                target: "worker-1".to_string(),
                duration_secs: 30,
                parameters: HashMap::new(),
                steady_state_check: "health_endpoint".to_string(),
            })
            .await;

        let experiments = engine.list_experiments().await;
        assert_eq!(experiments.len(), 1);
        assert_eq!(experiments[0].id, "test-exp-1");

        let result = engine.run_experiment("test-exp-1").await;
        assert!(result.is_ok());
        let run = result.unwrap();
        assert_eq!(run.result, ChaosResult::Passed);

        let history = engine.run_history(None).await;
        assert!(!history.is_empty());
    }

    #[tokio::test]
    async fn test_chaos_engine_run_nonexistent() {
        let engine = ChaosEngine::new();
        let result = engine.run_experiment("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_chaos_engine_run_history_filter() {
        let engine = ChaosEngine::new();
        engine.register_experiment(ChaosExperiment {
            id: "exp-a".to_string(),
            name: "A".to_string(),
            experiment_type: ChaosExperimentType::NodeFailure,
            target: "node-1".to_string(),
            duration_secs: 10,
            parameters: HashMap::new(),
            steady_state_check: "".to_string(),
        }).await;
        engine.register_experiment(ChaosExperiment {
            id: "exp-b".to_string(),
            name: "B".to_string(),
            experiment_type: ChaosExperimentType::LatencyInjection,
            target: "node-2".to_string(),
            duration_secs: 10,
            parameters: HashMap::new(),
            steady_state_check: "".to_string(),
        }).await;

        engine.run_experiment("exp-a").await.unwrap();
        engine.run_experiment("exp-b").await.unwrap();

        let history_a = engine.run_history(Some("exp-a")).await;
        assert_eq!(history_a.len(), 1);
        assert_eq!(history_a[0].experiment_id, "exp-a");
    }

    #[tokio::test]
    async fn test_recovery_automation() {
        let recovery = RecoveryAutomation::new();
        recovery
            .register_action(RecoveryAction {
                name: "restart-worker".to_string(),
                trigger_condition: "worker_unhealthy".to_string(),
                action_type: RecoveryActionType::RestartService,
                parameters: HashMap::new(),
                cooldown_secs: 60,
                max_attempts: 3,
            })
            .await;

        let result = recovery.execute_action("restart-worker").await;
        assert!(result.is_ok());
        let exec = result.unwrap();
        assert!(exec.success);
        assert_eq!(exec.action_name, "restart-worker");

        let history = recovery.execution_history().await;
        assert!(!history.is_empty());
    }

    #[tokio::test]
    async fn test_recovery_automation_nonexistent() {
        let recovery = RecoveryAutomation::new();
        let result = recovery.execute_action("nonexistent").await;
        assert!(result.is_err());
    }
}
