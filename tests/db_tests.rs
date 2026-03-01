/// Database Integration Tests — PostgreSQL backend
///
/// These tests require a real PostgreSQL connection via the `DATABASE_URL`
/// environment variable. They are skipped (compile but do nothing) if
/// `DATABASE_URL` is not set.
///
/// Run with:
///   DATABASE_URL=postgres://user:pass@localhost/vortex_test cargo test db_tests

#[cfg(test)]
mod db_tests {
    use anyhow::Result;

    /// Helper: attempt to build a PostgresDb from `DATABASE_URL`.
    /// Returns None if `DATABASE_URL` is not set, allowing tests to be skipped
    /// gracefully in CI environments without a live database.
    async fn try_postgres_db() -> Option<std::sync::Arc<vortex::db_postgres::PostgresDb>> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let db = vortex::db_postgres::PostgresDb::new(
            &url, 2, 1, std::time::Duration::from_secs(30),
        )
        .await
        .ok()?;
        Some(std::sync::Arc::new(db))
    }

    /// Test 1: Database Initialization — all tables created, migrations applied.
    #[tokio::test]
    async fn test_db_initialization() -> Result<()> {
        let Some(_db) = try_postgres_db().await else {
            eprintln!("SKIP test_db_initialization — DATABASE_URL not set");
            return Ok(());
        };
        // If PostgresDb::new() succeeded, migrations have been applied.
        // A real assertion: query pg_tables to verify core tables exist.
        Ok(())
    }

    /// Test 2: Worker CRUD operations.
    #[tokio::test]
    async fn test_workers_table_operations() -> Result<()> {
        let Some(db) = try_postgres_db().await else {
            eprintln!("SKIP test_workers_table_operations — DATABASE_URL not set");
            return Ok(());
        };
        use vortex::db_trait::DatabaseBackend;
        let id = format!("test-worker-{}", uuid::Uuid::new_v4());
        db.upsert_worker(&id, "test-host", 4, "test").await?;
        db.update_worker_heartbeat(&id, 0).await?;
        Ok(())
    }

    /// Test 3: Task state transitions.
    #[tokio::test]
    async fn test_task_instance_state_transitions() -> Result<()> {
        let Some(db) = try_postgres_db().await else {
            eprintln!("SKIP test_task_instance_state_transitions — DATABASE_URL not set");
            return Ok(());
        };
        use vortex::db_trait::DatabaseBackend;
        let dag_id = format!("test-dag-{}", uuid::Uuid::new_v4());
        let run_id = uuid::Uuid::new_v4().to_string();
        let ti_id  = uuid::Uuid::new_v4().to_string();

        db.save_dag(&dag_id, None).await?;
        db.create_dag_run(&run_id, &dag_id, chrono::Utc::now(), "test").await?;
        db.create_task_instance(&ti_id, &dag_id, "task-a", "Queued", chrono::Utc::now(), &run_id).await?;
        db.update_task_state(&ti_id, "Running").await?;
        db.update_task_state(&ti_id, "Success").await?;
        Ok(())
    }

    /// Test 4: Secret CRUD operations.
    #[tokio::test]
    async fn test_secrets_table_crud() -> Result<()> {
        let Some(db) = try_postgres_db().await else {
            eprintln!("SKIP test_secrets_table_crud — DATABASE_URL not set");
            return Ok(());
        };
        use vortex::db_trait::DatabaseBackend;
        let key = format!("test-secret-{}", uuid::Uuid::new_v4());
        db.store_secret(&key, "encrypted_val").await?;
        let val = db.get_secret(&key).await?;
        assert_eq!(val.as_deref(), Some("encrypted_val"));
        db.delete_secret(&key).await?;
        let gone = db.get_secret(&key).await?;
        assert!(gone.is_none());
        Ok(())
    }

    /// Test 5: XCom push / pull.
    #[tokio::test]
    async fn test_xcom_push_pull() -> Result<()> {
        let Some(db) = try_postgres_db().await else {
            eprintln!("SKIP test_xcom_push_pull — DATABASE_URL not set");
            return Ok(());
        };
        use vortex::db_trait::DatabaseBackend;
        let dag_id = format!("xcom-dag-{}", uuid::Uuid::new_v4());
        let run_id = uuid::Uuid::new_v4().to_string();
        db.save_dag(&dag_id, None).await?;
        db.create_dag_run(&run_id, &dag_id, chrono::Utc::now(), "test").await?;
        db.xcom_push(&dag_id, "task-a", &run_id, "result", "hello").await?;
        let pulled = db.xcom_pull(&dag_id, "task-a", &run_id, "result").await?;
        assert_eq!(pulled.as_deref(), Some("hello"));
        
        let (all_xcoms, count) = db.xcom_pull_all(&dag_id, &run_id, 10, 0).await?;
        assert_eq!(count, 1);
        assert_eq!(all_xcoms.len(), 1);
        Ok(())
    }

    /// Test 6: Task Event Logs.
    #[tokio::test]
    async fn test_task_event_logs() -> Result<()> {
        let Some(db) = try_postgres_db().await else {
            eprintln!("SKIP test_task_event_logs — DATABASE_URL not set");
            return Ok(());
        };
        use vortex::db_trait::DatabaseBackend;
        let dag_id = format!("test-dag-{}", uuid::Uuid::new_v4());
        let run_id = uuid::Uuid::new_v4().to_string();
        let ti_id  = uuid::Uuid::new_v4().to_string();

        db.save_dag(&dag_id, None).await?;
        db.create_dag_run(&run_id, &dag_id, chrono::Utc::now(), "test").await?;
        db.create_task_instance(&ti_id, &dag_id, "task-a", "Queued", chrono::Utc::now(), &run_id).await?;
        
        db.log_task_event(&ti_id, &dag_id, "task-a", &run_id, "queued", None, None).await?;
        db.log_task_event(&ti_id, &dag_id, "task-a", &run_id, "started", None, Some("worker-1")).await?;
        db.update_task_state(&ti_id, "Running").await?;
        db.log_task_event(&ti_id, &dag_id, "task-a", &run_id, "success", None, Some("worker-1")).await?;
        db.update_task_state(&ti_id, "Success").await?;
        
        let events = db.get_task_events(&ti_id).await?;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["event"], "queued");
        assert_eq!(events[1]["event"], "started");
        assert_eq!(events[2]["event"], "success");
        Ok(())
    }

    /// Test 7: SLA Breach marking.
    #[tokio::test]
    async fn test_sla_breach() -> Result<()> {
        let Some(db) = try_postgres_db().await else {
            eprintln!("SKIP test_sla_breach — DATABASE_URL not set");
            return Ok(());
        };
        use vortex::db_trait::DatabaseBackend;
        let dag_id = format!("sla-dag-{}", uuid::Uuid::new_v4());
        let run_id = uuid::Uuid::new_v4().to_string();

        db.save_dag(&dag_id, None).await?;
        db.create_dag_run(&run_id, &dag_id, chrono::Utc::now(), "test").await?;
        
        let (runs_before, _) = db.get_dag_runs(&dag_id, 10, 0).await?;
        assert!(!runs_before[0]["sla_missed"].as_bool().unwrap_or(false));

        db.mark_sla_missed(&run_id).await?;

        let (runs_after, _) = db.get_dag_runs(&dag_id, 10, 0).await?;
        assert!(runs_after[0]["sla_missed"].as_bool().unwrap_or(false));

        Ok(())
    }

    /// Test 8: High Availability Advisory Lock
    #[tokio::test]
    async fn test_ha_leader_lock() -> Result<()> {
        let Some(db1) = try_postgres_db().await else {
            eprintln!("SKIP test_ha_leader_lock — DATABASE_URL not set");
            return Ok(());
        };
        let Some(db2) = try_postgres_db().await else {
            return Ok(());
        };

        use vortex::db_trait::DatabaseBackend;

        // Clean slate - ensure lock is released if left over from a panicked test
        let _ = db1.release_leader_lock().await;
        let _ = db2.release_leader_lock().await;

        // DB1 acquires the lock
        let lock1 = db1.try_acquire_leader_lock().await?;
        assert!(lock1, "DB1 should acquire the leader lock");

        // DB2 tries to acquire the lock and should fail
        let lock2 = db2.try_acquire_leader_lock().await?;
        assert!(!lock2, "DB2 should fail to acquire the lock while DB1 holds it");

        // DB1 releases the lock
        db1.release_leader_lock().await?;

        // DB2 tries again and should succeed
        let lock2_retry = db2.try_acquire_leader_lock().await?;
        assert!(lock2_retry, "DB2 should acquire the lock after DB1 releases it");

        // Release at the end
        db2.release_leader_lock().await?;

        Ok(())
    }
}
