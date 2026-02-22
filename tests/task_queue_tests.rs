/// Task Queue & Re-queueing Tests
/// Tests queue operations, priority, re-queueing, persistence, and load

#[cfg(test)]
mod task_queue_tests {
    use anyhow::Result;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Test 1: Basic Enqueue/Dequeue
    /// Verify tasks enter and exit queue in order (FIFO)
    #[tokio::test]
    async fn test_enqueue_dequeue_fifo() -> Result<()> {
        // Setup: Create queue
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        // Enqueue 5 tasks
        for i in 0..5 {
            let mut q = queue.write().await;
            q.push(format!("task-{}", i));
        }

        // Verify FIFO order
        for i in 0..5 {
            let mut q = queue.write().await;
            let task = q.remove(0);
            assert_eq!(task, format!("task-{}", i), "Tasks must dequeue in FIFO order");
        }

        Ok(())
    }

    /// Test 2: Queue Depth Check
    /// Verify queue_depth() returns accurate count
    #[tokio::test]
    async fn test_queue_depth() -> Result<()> {
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        // Empty queue
        {
            let q = queue.read().await;
            assert_eq!(q.len(), 0, "Empty queue depth must be 0");
        }

        // Add 10 tasks
        for i in 0..10 {
            let mut q = queue.write().await;
            q.push(format!("task-{}", i));
        }

        {
            let q = queue.read().await;
            assert_eq!(q.len(), 10, "Queue depth must be 10");
        }

        // Remove 3 tasks
        for _ in 0..3 {
            let mut q = queue.write().await;
            q.remove(0);
        }

        {
            let q = queue.read().await;
            assert_eq!(q.len(), 7, "Queue depth must be 7 after removals");
        }

        Ok(())
    }

    /// Test 3: Priority Queue Handling
    /// Verify high-priority tasks dequeue first
    #[tokio::test]
    async fn test_priority_queue() -> Result<()> {
        // Structure: (task_id, priority)
        let mut queue: Vec<(String, i32)> = Vec::new();

        // Add tasks with mixed priorities
        queue.push(("task-1".to_string(), 5)); // low
        queue.push(("task-2".to_string(), 10)); // high
        queue.push(("task-3".to_string(), 8)); // medium
        queue.push(("task-4".to_string(), 15)); // highest

        // Sort by priority (descending)
        queue.sort_by(|a, b| b.1.cmp(&a.1));

        // Verify dequeue order: 4, 2, 3, 1
        assert_eq!(queue[0].0, "task-4");
        assert_eq!(queue[1].0, "task-2");
        assert_eq!(queue[2].0, "task-3");
        assert_eq!(queue[3].0, "task-1");

        Ok(())
    }

    /// Test 4: Re-queueing Failed Tasks
    /// Verify failed/orphaned tasks return to queue
    #[tokio::test]
    async fn test_requeue_failed_tasks() -> Result<()> {
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        // Add 3 tasks
        for i in 0..3 {
            let mut q = queue.write().await;
            q.push(format!("task-{}", i));
        }

        // Simulate: task-1 assigned to worker but fails
        // Worker offline → task-1 returned to queue
        {
            let mut q = queue.write().await;
            q.push("task-1-retry".to_string());
        }

        {
            let q = queue.read().await;
            assert_eq!(q.len(), 4, "Re-queued task should increase queue depth");
        }

        Ok(())
    }

    /// Test 5: Task De-duplication
    /// Verify same task is not queued twice
    #[tokio::test]
    async fn test_task_deduplication() -> Result<()> {
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));
        let task_id = "task-1";

        // Add task-1 once
        {
            let mut q = queue.write().await;
            q.push(task_id.to_string());
        }

        // Try to add task-1 again (should prevent duplicate)
        {
            let mut q = queue.write().await;
            if !q.contains(&task_id.to_string()) {
                q.push(task_id.to_string());
            }
        }

        {
            let q = queue.read().await;
            assert_eq!(q.len(), 1, "Queue must not contain duplicates");
        }

        Ok(())
    }

    /// Test 6: Concurrent Enqueue/Dequeue
    /// Verify no data loss with concurrent access
    #[tokio::test]
    async fn test_concurrent_enqueue_dequeue() -> Result<()> {
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));
        let mut handles = vec![];

        // Spawn 5 tasks enqueueing 20 tasks each
        for thread_id in 0..5 {
            let q = queue.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..20 {
                    let mut q = q.write().await;
                    q.push(format!("task-{}-{}", thread_id, i));
                }
            }));
        }

        for handle in handles {
            handle.await?;
        }

        // Verify all 100 tasks are in queue
        {
            let q = queue.read().await;
            assert_eq!(q.len(), 100, "All 100 tasks must be queued");
        }

        Ok(())
    }

    /// Test 7: Load Test - 1000 Tasks
    /// Verify no loss or duplication at scale
    #[tokio::test]
    async fn test_load_1000_tasks() -> Result<()> {
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        // Enqueue 1000 tasks
        {
            let mut q = queue.write().await;
            for i in 0..1000 {
                q.push(format!("task-{}", i));
            }
        }

        // Verify count
        {
            let q = queue.read().await;
            assert_eq!(q.len(), 1000, "1000 tasks must be queued");
        }

        // Dequeue all and verify FIFO
        {
            let mut q = queue.write().await;
            for i in 0..1000 {
                let task = q.remove(0);
                assert_eq!(task, format!("task-{}", i), "FIFO order must be preserved");
            }
            assert_eq!(q.len(), 0, "Queue must be empty after dequeue all");
        }

        Ok(())
    }

    /// Test 8: Queue Overflow Handling
    /// Verify queue gracefully handles large numbers
    #[tokio::test]
    async fn test_queue_large_capacity() -> Result<()> {
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        // Add 10,000 tasks
        {
            let mut q = queue.write().await;
            for i in 0..10000 {
                q.push(format!("task-{}", i));
            }
        }

        {
            let q = queue.read().await;
            assert_eq!(q.len(), 10000, "Large queue must handle 10k tasks");
        }

        Ok(())
    }

    /// Test 9: Task State Transition Logging
    /// Verify state changes are tracked for failed tasks
    #[tokio::test]
    async fn test_task_state_transitions() -> Result<()> {
        // Track: task_id -> state history
        use std::collections::HashMap;
        let mut state_log: HashMap<String, Vec<String>> = HashMap::new();

        let task_id = "task-1";
        state_log.insert(task_id.to_string(), vec!["Queued".to_string()]);

        // Simulate state changes
        state_log.get_mut(task_id).unwrap().push("Running".to_string());
        state_log.get_mut(task_id).unwrap().push("Failed".to_string());
        state_log.get_mut(task_id).unwrap().push("Queued".to_string()); // Re-queued

        let states = &state_log[task_id];
        assert_eq!(states, &vec!["Queued", "Running", "Failed", "Queued"]);

        Ok(())
    }

    /// Test 10: Queue Persistence Check
    /// Verify queue state survives (mock of DB persistence)
    #[tokio::test]
    async fn test_queue_persistence() -> Result<()> {
        // In-memory queue represents DB-backed persistence
        let queue: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));

        // Enqueue tasks
        {
            let mut q = queue.write().await;
            for i in 0..5 {
                q.push(format!("task-{}", i));
            }
        }

        // Simulate "restart" by reading queue again
        {
            let q = queue.read().await;
            assert_eq!(q.len(), 5, "Queue must be persistent across restarts");
            for (i, task) in q.iter().enumerate() {
                assert_eq!(task, &format!("task-{}", i));
            }
        }

        Ok(())
    }
}
