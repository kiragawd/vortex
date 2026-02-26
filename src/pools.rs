use anyhow::{anyhow, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn, error};

use crate::db::Db;

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
    db: Arc<Db>,
}

impl PoolManager {
    /// Create a new PoolManager backed by the given database.
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    // ── Schema Bootstrap ──────────────────────────────────────────────────────

    /// Ensure the pools and pool_slots tables exist.
    /// Called during DB init by the integration layer.
    pub fn ensure_tables(&self) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(POOLS_TABLE_SQL, [])?;
        conn.execute(POOL_SLOTS_TABLE_SQL, [])?;
        Ok(())
    }

    /// Seed the "default" pool (128 slots) if it does not already exist.
    pub fn init_default_pool(&self) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pools WHERE name = 'default'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .unwrap_or(0)
            > 0;

        if !exists {
            conn.execute(
                "INSERT INTO pools (name, slots, description) VALUES ('default', 128, 'Default pool')",
                [],
            )?;
            info!("Seeded default pool with 128 slots");
        }
        Ok(())
    }

    // ── CRUD ─────────────────────────────────────────────────────────────────

    /// Create a new pool with the given name, slot count, and description.
    pub fn create_pool(&self, name: &str, slots: i32, description: &str) -> Result<()> {
        if slots <= 0 {
            return Err(anyhow!("Pool '{}' must have at least 1 slot", name));
        }
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pools (name, slots, description) VALUES (?1, ?2, ?3)",
            params![name, slots, description],
        )?;
        info!(pool = name, slots, "Created pool");
        Ok(())
    }

    /// Delete a pool by name. The "default" pool cannot be deleted.
    pub fn delete_pool(&self, name: &str) -> Result<()> {
        if name == "default" {
            return Err(anyhow!("The 'default' pool cannot be deleted"));
        }

        // Check whether any slots are currently occupied before deleting.
        let (occupied, _) = self.get_pool_usage(name)?;
        if occupied > 0 {
            warn!(
                pool = name,
                occupied, "Deleting pool that still has occupied slots"
            );
        }

        let conn = self.db.conn.lock().unwrap();
        // Cascade-delete active slot reservations first (FK may not enforce CASCADE).
        conn.execute(
            "DELETE FROM pool_slots WHERE pool_name = ?1",
            params![name],
        )?;
        let rows = conn.execute("DELETE FROM pools WHERE name = ?1", params![name])?;
        if rows == 0 {
            return Err(anyhow!("Pool '{}' not found", name));
        }
        info!(pool = name, "Deleted pool");
        Ok(())
    }

    /// Update a pool's slot count and/or description.
    /// If the new slot count is less than the currently occupied count the update
    /// is still allowed — the pool simply becomes over-subscribed until tasks
    /// finish and release their slots.
    pub fn update_pool(&self, name: &str, slots: i32, description: &str) -> Result<()> {
        if slots <= 0 {
            return Err(anyhow!("Pool '{}' must have at least 1 slot", name));
        }
        let conn = self.db.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE pools SET slots = ?1, description = ?2 WHERE name = ?3",
            params![slots, description, name],
        )?;
        if rows == 0 {
            return Err(anyhow!("Pool '{}' not found", name));
        }
        info!(pool = name, slots, "Updated pool");
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Return pool info (including occupied slot count) for the given pool name,
    /// or `None` if no such pool exists.
    pub fn get_pool(&self, name: &str) -> Result<Option<Pool>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT p.name, p.slots, p.description,
                    COUNT(ps.id) AS occupied
             FROM pools p
             LEFT JOIN pool_slots ps ON ps.pool_name = p.name
             WHERE p.name = ?1
             GROUP BY p.name, p.slots, p.description",
        )?;
        let mut rows = stmt.query_map(params![name], |row| {
            Ok(Pool {
                name: row.get(0)?,
                slots: row.get(1)?,
                description: row.get(2)?,
                occupied_slots: row.get(3)?,
            })
        })?;
        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    /// Return all pools with their current occupied slot counts.
    pub fn get_all_pools(&self) -> Result<Vec<Pool>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT p.name, p.slots, p.description,
                    COUNT(ps.id) AS occupied
             FROM pools p
             LEFT JOIN pool_slots ps ON ps.pool_name = p.name
             GROUP BY p.name, p.slots, p.description
             ORDER BY p.name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Pool {
                name: row.get(0)?,
                slots: row.get(1)?,
                description: row.get(2)?,
                occupied_slots: row.get(3)?,
            })
        })?;
        let mut pools = Vec::new();
        for row in rows {
            pools.push(row?);
        }
        Ok(pools)
    }

    // ── Slot Lifecycle ────────────────────────────────────────────────────────

    /// Attempt to acquire one slot in `pool_name` for `task_instance_id`.
    ///
    /// Returns `true` if the slot was acquired, `false` if the pool is full.
    /// This operation is atomic: it checks capacity and inserts the reservation
    /// inside a single serialised lock hold.
    pub fn acquire_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<bool> {
        let conn = self.db.conn.lock().unwrap();

        // Fetch pool capacity.
        let pool_result = conn.query_row(
            "SELECT slots FROM pools WHERE name = ?1",
            params![pool_name],
            |row| row.get::<_, i32>(0),
        );

        let total_slots = match pool_result {
            Ok(s) => s,
            Err(_) => {
                error!(pool = pool_name, "Pool not found during acquire_slot");
                return Err(anyhow!("Pool '{}' not found", pool_name));
            }
        };

        // Count currently occupied slots.
        let occupied: i32 = conn.query_row(
            "SELECT COUNT(*) FROM pool_slots WHERE pool_name = ?1",
            params![pool_name],
            |row| row.get(0),
        )?;

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

        // Attempt insertion; UNIQUE constraint guards against duplicate acquisition.
        let now = chrono::Utc::now().to_rfc3339();
        let insert_result = conn.execute(
            "INSERT OR IGNORE INTO pool_slots (pool_name, task_instance_id, acquired_at)
             VALUES (?1, ?2, ?3)",
            params![pool_name, task_instance_id, now],
        );

        match insert_result {
            Ok(1) => {
                info!(
                    pool = pool_name,
                    task_instance_id,
                    occupied = occupied + 1,
                    total = total_slots,
                    "Slot acquired"
                );
                Ok(true)
            }
            Ok(_) => {
                // Row already existed (duplicate acquisition attempt) — treat as success.
                info!(
                    pool = pool_name,
                    task_instance_id, "Slot already held by this task instance"
                );
                Ok(true)
            }
            Err(e) => {
                error!(pool = pool_name, task_instance_id, error = %e, "Failed to insert pool slot");
                Err(anyhow!("Failed to acquire slot: {}", e))
            }
        }
    }

    /// Release the slot held by `task_instance_id` in `pool_name`.
    /// It is safe to call this even if no slot was held (idempotent).
    pub fn release_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM pool_slots WHERE pool_name = ?1 AND task_instance_id = ?2",
            params![pool_name, task_instance_id],
        )?;
        if rows > 0 {
            info!(
                pool = pool_name,
                task_instance_id, "Slot released"
            );
        } else {
            warn!(
                pool = pool_name,
                task_instance_id, "release_slot called but no slot was held"
            );
        }
        Ok(())
    }

    /// Return `(occupied_slots, total_slots)` for the given pool.
    pub fn get_pool_usage(&self, pool_name: &str) -> Result<(i32, i32)> {
        let conn = self.db.conn.lock().unwrap();

        let total: i32 = conn
            .query_row(
                "SELECT slots FROM pools WHERE name = ?1",
                params![pool_name],
                |row| row.get(0),
            )
            .map_err(|_| anyhow!("Pool '{}' not found", pool_name))?;

        let occupied: i32 = conn.query_row(
            "SELECT COUNT(*) FROM pool_slots WHERE pool_name = ?1",
            params![pool_name],
            |row| row.get(0),
        )?;

        Ok((occupied, total))
    }
}
