#![allow(dead_code)]
use anyhow::{anyhow, Result};

use std::sync::Arc;
use tracing::{info, warn};

use crate::db_trait::DatabaseBackend;

// ─── PoolManager ─────────────────────────────────────────────────────────────

/// Manages task pools — creation, slot acquisition/release, and queries.
pub struct PoolManager {
    db: Arc<dyn DatabaseBackend>,
}

impl PoolManager {
    /// Create a new PoolManager backed by the given database.
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    // ── CRUD ─────────────────────────────────────────────────────────────────

    /// Create a new pool with the given name, slot count, and description.
    pub async fn create_pool(&self, name: &str, slots: i32, description: &str) -> Result<()> {
        info!(pool = name, slots, "Created pool");
        self.db.create_pool(name, slots, description).await
    }

    /// Delete a pool by name. The "default" pool cannot be deleted.
    pub async fn delete_pool(&self, name: &str) -> Result<()> {
        if name == "default" {
            return Err(anyhow!("The 'default' pool cannot be deleted"));
        }

        // BUG-7 FIX: Block deletion when slots are occupied instead of just warning.
        let (occupied, _) = self.get_pool_usage(name).await?;
        if occupied > 0 {
            return Err(anyhow!(
                "Pool '{}' has {} occupied slot(s). Drain all tasks before deleting.",
                name, occupied
            ));
        }

        info!(pool = name, "Deleted pool");
        self.db.delete_pool(name).await
    }

    /// Update a pool's slot count and/or description.
    pub async fn update_pool(&self, name: &str, slots: i32, description: &str) -> Result<()> {
        info!(pool = name, slots, "Updated pool");
        self.db.update_pool(name, slots, description).await
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Return pool info for the given pool name, or `None` if no such pool exists.
    pub async fn get_pool(&self, name: &str) -> Result<Option<serde_json::Value>> {
        self.db.get_pool(name).await
    }

    /// Return all pools with their current occupied slot counts.
    pub async fn get_all_pools(&self) -> Result<Vec<serde_json::Value>> {
        self.db.get_all_pools().await
    }

    // ── Slot Lifecycle ────────────────────────────────────────────────────────

    /// Return `(occupied_slots, total_slots)` for the given pool.
    pub async fn get_pool_usage(&self, pool_name: &str) -> Result<(i32, i32)> {
        match self.db.get_pool(pool_name).await? {
            Some(pool) => {
                let total = pool.get("slots").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let occupied = pool.get("occupied_slots").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                Ok((occupied, total))
            }
            None => Err(anyhow!("Pool '{}' not found", pool_name)),
        }
    }

    /// Attempt to acquire one slot in `pool_name` for `task_instance_id`.
    ///
    /// Returns `true` if the slot was acquired, `false` if the pool is full.
    pub async fn acquire_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<bool> {
        // BUG-5 FIX: Delegate entirely to the atomic DB operation which uses
        // SELECT FOR UPDATE inside a transaction. The previous code did a
        // non-atomic get_pool_usage pre-check (TOCTOU vulnerability).
        let acquired = self.db.acquire_pool_slot(pool_name, task_instance_id).await?;
        if acquired {
            info!(pool = pool_name, task_instance_id, "Slot acquired");
        } else {
            warn!(pool = pool_name, task_instance_id, "Pool is full — slot acquisition denied");
        }
        Ok(acquired)
    }

    /// Release the slot held by `task_instance_id` in `pool_name`.
    pub async fn release_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<()> {
        // BUG-6 FIX: Actually delete the slot row so the pool count decrements.
        self.db.release_pool_slot(pool_name, task_instance_id).await?;
        info!(pool = pool_name, task_instance_id, "Slot released");
        Ok(())
    }
}

// Dead code removal: handle_recovery is used in scheduler.rs but marked as unused by compiler because
// it might only be used in specific build configurations or the call site was removed.
// The instructions say to remove unused functions. Let's see if it's actually used.
// Checking scheduler.rs... it's NOT called anywhere in the provided scheduler.rs code.
// Wait, I see `async fn handle_recovery(&self) -> Result<()>` in scheduler.rs.
// Is it called? I'll check.

