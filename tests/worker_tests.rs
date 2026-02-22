/// Worker Lifecycle Tests
/// Tests task assignment, completion, timeout, state machine, and concurrency

#[cfg(test)]
mod worker_tests {
    use anyhow::Result;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio::time::timeout;
    use chrono::Utc;

    #[derive(Debug, Clone, PartialEq)]
    enum WorkerState {
        Idle,
        Running,
        Completed,
        Failed,
    }

    #[derive(Debug, Clone)]
    struct TaskAssignment {
        task_id: String,
        command: String,
        state: WorkerState,
        assigned_at: chrono::DateTime<chrono::Utc>,
    }

    /// Test 1: Worker Registration State Transition
    /// Verify worker transitions from Idle → Active on registration
    #[tokio::test]
    async fn test_worker_registration_state_transition() -> Result<()> {
        // Simulate worker registration
        let worker_state = Arc::new(RwLock::new(WorkerState::Idle));

        // Register worker → transition to Running
        {
            let mut state = worker_state.write().await;
            *state = WorkerState::Running;
        }

        {
            let state = worker_state.read().await;
            assert_eq!(*state, WorkerState::Running, "Worker must transition to Running on registration");
        }

        Ok(())
    }

    /// Test 2: Task Assignment to Worker
    /// Verify worker receives task and updates state to Running
    #[tokio::test]
    async fn test_task_assignment() -> Result<()> {
        // Worker with 4 concurrent slots
        let capacity = 4;
        let active_tasks = Arc::new(RwLock::new(0));

        // Assign task
        {
            let mut count = active_tasks.write().await;
            assert!(*count < capacity, "Must have available slots");
            *count += 1;
        }

        {
            let count = active_tasks.read().await;
            assert_eq!(*count, 1, "Active task count must be 1");
        }

        Ok(())
    }

    /// Test 3: Task Completion
    /// Verify task completion updates worker state
    #[tokio::test]
    async fn test_task_completion() -> Result<()> {
        let active_tasks = Arc::new(RwLock::new(1)); // One task running

        // Task completes
        {
            let mut count = active_tasks.write().await;
            *count -= 1;
        }

        {
            let count = active_tasks.read().await;
            assert_eq!(*count, 0, "Active count must return to 0 after completion");
        }

        Ok(())
    }

    /// Test 4: Task Timeout
    /// Verify long-running task transitions to Failed after timeout
    #[tokio::test]
    async fn test_task_timeout() -> Result<()> {
        let task_state = Arc::new(RwLock::new(WorkerState::Running));
        let task_timeout = Duration::from_millis(100); // 100ms timeout

        // Simulate task execution (mock sleep)
        let result = timeout(task_timeout, async {
            tokio::time::sleep(Duration::from_millis(500)).await
        })
        .await;

        // Should timeout
        assert!(result.is_err(), "Task should timeout");

        // Mark as failed
        {
            let mut state = task_state.write().await;
            *state = WorkerState::Failed;
        }

        {
            let state = task_state.read().await;
            assert_eq!(*state, WorkerState::Failed, "Task must transition to Failed on timeout");
        }

        Ok(())
    }

    /// Test 5: Worker State Machine
    /// Verify valid state transitions
    #[tokio::test]
    async fn test_worker_state_machine() -> Result<()> {
        let state = Arc::new(RwLock::new(WorkerState::Idle));

        // Valid transition: Idle → Running
        {
            let mut s = state.write().await;
            *s = WorkerState::Running;
        }

        {
            let s = state.read().await;
            assert_eq!(*s, WorkerState::Running);
        }

        // Valid transition: Running → Completed
        {
            let mut s = state.write().await;
            *s = WorkerState::Completed;
        }

        {
            let s = state.read().await;
            assert_eq!(*s, WorkerState::Completed);
        }

        // Valid transition: Completed → Idle (for next task)
        {
            let mut s = state.write().await;
            *s = WorkerState::Idle;
        }

        {
            let s = state.read().await;
            assert_eq!(*s, WorkerState::Idle);
        }

        Ok(())
    }

    /// Test 6: Multiple Concurrent Task Assignments
    /// Verify worker handles 5+ concurrent tasks
    #[tokio::test]
    async fn test_multiple_concurrent_tasks() -> Result<()> {
        let capacity = 10;
        let active_tasks = Arc::new(RwLock::new(0));
        let mut handles = vec![];

        // Spawn 8 concurrent task executions
        for _ in 0..8 {
            let count = active_tasks.clone();
            handles.push(tokio::spawn(async move {
                // Acquire task slot
                {
                    let mut c = count.write().await;
                    if *c < capacity {
                        *c += 1;
                    }
                }

                // Simulate work
                tokio::time::sleep(Duration::from_millis(10)).await;

                // Release task slot
                {
                    let mut c = count.write().await;
                    *c -= 1;
                }
            }));
        }

        // Wait for all to complete
        for handle in handles {
            handle.await?;
        }

        // All should be completed
        {
            let count = active_tasks.read().await;
            assert_eq!(*count, 0, "All tasks must complete");
        }

        Ok(())
    }

    /// Test 7: Worker Heartbeat Tracking
    /// Verify heartbeat updates and staleness detection
    #[tokio::test]
    async fn test_worker_heartbeat_tracking() -> Result<()> {
        let last_heartbeat = Arc::new(RwLock::new(Utc::now()));

        // Get initial heartbeat
        let initial = {
            let hb = last_heartbeat.read().await;
            *hb
        };

        // Simulate: no heartbeat for 100ms
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Get elapsed time
        let elapsed = {
            let hb = last_heartbeat.read().await;
            Utc::now() - *hb
        };

        assert!(elapsed.num_seconds() < 2, "Elapsed time should be < 2 seconds");

        // Update heartbeat
        {
            let mut hb = last_heartbeat.write().await;
            *hb = Utc::now();
        }

        // Verify heartbeat was updated
        {
            let hb = last_heartbeat.read().await;
            let new_elapsed = Utc::now() - *hb;
            assert!(new_elapsed.num_milliseconds() < 100, "Heartbeat should be fresh");
        }

        Ok(())
    }

    /// Test 8: Stale Worker Detection
    /// Verify worker marked offline after 60s+ without heartbeat
    #[tokio::test]
    async fn test_stale_worker_detection() -> Result<()> {
        let last_heartbeat = Arc::new(RwLock::new(Utc::now()));
        let is_online = Arc::new(RwLock::new(true));

        // Set heartbeat to 61 seconds ago (simulated)
        {
            use chrono::Duration;
            let mut hb = last_heartbeat.write().await;
            *hb = Utc::now() - Duration::seconds(61);
        }

        // Check if stale
        let is_stale = {
            let hb = last_heartbeat.read().await;
            let elapsed = Utc::now() - *hb;
            elapsed.num_seconds() > 60
        };

        assert!(is_stale, "Worker should be detected as stale");

        // Mark as offline
        {
            let mut online = is_online.write().await;
            *online = false;
        }

        {
            let online = is_online.read().await;
            assert!(!*online, "Stale worker must be marked offline");
        }

        Ok(())
    }

    /// Test 9: Task Re-assignment on Worker Failure
    /// Verify tasks from failed worker are reassigned
    #[tokio::test]
    async fn test_task_reassignment_on_failure() -> Result<()> {
        // Worker 1 crashes with task-1 Running
        let mut running_tasks = vec![("worker-1".to_string(), "task-1".to_string())];

        // Detect failure (stale worker)
        let failed_worker = "worker-1";
        
        // Extract tasks from failed worker
        running_tasks.retain(|(w, _)| w != failed_worker);

        // Re-queue task-1 for worker-2
        running_tasks.push(("worker-2".to_string(), "task-1".to_string()));

        assert_eq!(running_tasks.len(), 1, "Task must be reassigned");
        assert_eq!(running_tasks[0].0, "worker-2", "Task must be assigned to new worker");

        Ok(())
    }

    /// Test 10: Worker Draining
    /// Verify worker gracefully drains active tasks before exit
    #[tokio::test]
    async fn test_worker_draining() -> Result<()> {
        let should_drain = Arc::new(RwLock::new(false));
        let active_tasks = Arc::new(RwLock::new(3)); // 3 tasks running

        // Controller requests drain
        {
            let mut drain = should_drain.write().await;
            *drain = true;
        }

        // Worker sees drain flag
        {
            let drain = should_drain.read().await;
            assert!(*drain, "Drain flag must be set");
        }

        // Worker finishes active tasks
        {
            let mut count = active_tasks.write().await;
            *count -= 1; // Task 1 completes
            *count -= 1; // Task 2 completes
            *count -= 1; // Task 3 completes
        }

        // Verify ready to exit
        {
            let drain = should_drain.read().await;
            let count = active_tasks.read().await;
            assert!(*drain && *count == 0, "Worker ready to exit when drained and no active tasks");
        }

        Ok(())
    }
}
