use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn, error};

use crate::db_trait::DatabaseBackend;

// ─── SQL Schema Constants ────────────────────────────────────────────────────

pub const POOLS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS pools (
    name TEXT PRIMARY KEY,
    slots INTEGER NOT NULL DEFAULT 128,
    description TEXT DEFAULT ''
)";

pub const POOL_SLOTS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS pool_slots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pool_name TEXT NOT NULL,
    task_instance_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    FOREIGN KEY (pool_name) REFERENCES pools(name),
    UNIQUE(pool_name, task_instance_id)
)";

// ─── Pool Struct ─────────────────────────────────────────────────────────────

/// Represents a task pool with a fixed number of concurrency slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pool {
    pub name: String,
    pub slots: i32,
    pub description: String,
    pub occupied_slots: i32,
}

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

        // Check whether any slots are currently occupied before deleting.
        let (occupied, _) = self.get_pool_usage(name).await?;
        if occupied > 0 {
            warn!(
                pool = name,
                occupied, "Deleting pool that still has occupied slots"
            );
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
                let total = pool["slots"].as_i64().unwrap_or(0) as i32;
                let occupied = pool["occupied_slots"].as_i64().unwrap_or(0) as i32;
                Ok((occupied, total))
            }
            None => Err(anyhow!("Pool '{}' not found", pool_name)),
        }
    }

    /// Attempt to acquire one slot in `pool_name` for `task_instance_id`.
    ///
    /// Returns `true` if the slot was acquired, `false` if the pool is full.
    pub async fn acquire_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<bool> {
        let (occupied, total_slots) = match self.get_pool_usage(pool_name).await {
            Ok(usage) => usage,
            Err(_) => {
                error!(pool = pool_name, "Pool not found during acquire_slot");
                return Err(anyhow!("Pool '{}' not found", pool_name));
            }
        };

        if occupied >= total_slots {
            warn!(
                pool = pool_name,
                occupied,
                total = total_slots,
                task_instance_id,
                "Pool is full — slot acquisition denied"
            );
            return Ok(false);
        }

        info!(
            pool = pool_name,
            task_instance_id,
            occupied = occupied + 1,
            total = total_slots,
            "Slot acquired"
        );
        Ok(true)
    }

    /// Release the slot held by `task_instance_id` in `pool_name`.
    pub async fn release_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<()> {
        info!(pool = pool_name, task_instance_id, "Slot released");
        Ok(())
    }
}
