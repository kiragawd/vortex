use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use chrono::{DateTime, Utc};

pub struct Db {
    pub conn: Mutex<Connection>,
}

impl Db {
    pub fn init<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dags (
                id TEXT PRIMARY KEY,
                created_at DATETIME NOT NULL,
                schedule_interval TEXT,
                last_run DATETIME,
                is_paused BOOLEAN DEFAULT 0,
                timezone TEXT DEFAULT 'UTC',
                max_active_runs INTEGER DEFAULT 1,
                catchup BOOLEAN DEFAULT 0,
                next_run DATETIME
            )",
            [],
        )?;

        // Pillars migrations logic
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(dags)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !cols.contains(&"schedule_interval".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN schedule_interval TEXT", [])?;
        }
        if !cols.contains(&"last_run".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN last_run DATETIME", [])?;
        }
        if !cols.contains(&"is_paused".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN is_paused BOOLEAN DEFAULT 0", [])?;
        }
        if !cols.contains(&"timezone".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN timezone TEXT DEFAULT 'UTC'", [])?;
        }
        if !cols.contains(&"max_active_runs".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN max_active_runs INTEGER DEFAULT 1", [])?;
        }
        if !cols.contains(&"catchup".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN catchup BOOLEAN DEFAULT 0", [])?;
        }
        if !cols.contains(&"next_run".to_string()) {
            conn.execute("ALTER TABLE dags ADD COLUMN next_run DATETIME", [])?;
        }

        // Pillar 4: Workers Table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY,
                hostname TEXT NOT NULL,
                capacity INTEGER NOT NULL,
                active_tasks INTEGER DEFAULT 0,
                last_heartbeat DATETIME NOT NULL,
                state TEXT NOT NULL, -- 'Active', 'Offline', 'Draining'
                labels TEXT
            )",
            [],
        )?;

        // Pillar 4: Track which worker is running which task instance
        let ti_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(task_instances)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !ti_cols.contains(&"worker_id".to_string()) {
            conn.execute("ALTER TABLE task_instances ADD COLUMN worker_id TEXT", [])?;
        }
        if !ti_cols.contains(&"stdout".to_string()) {
            conn.execute("ALTER TABLE task_instances ADD COLUMN stdout TEXT", [])?;
        }
        if !ti_cols.contains(&"stderr".to_string()) {
            conn.execute("ALTER TABLE task_instances ADD COLUMN stderr TEXT", [])?;
        }
        if !ti_cols.contains(&"duration_ms".to_string()) {
            conn.execute("ALTER TABLE task_instances ADD COLUMN duration_ms INTEGER", [])?;
        }
        if !ti_cols.contains(&"retry_count".to_string()) {
            conn.execute("ALTER TABLE task_instances ADD COLUMN retry_count INTEGER DEFAULT 0", [])?;
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT NOT NULL,
                dag_id TEXT NOT NULL,
                name TEXT NOT NULL,
                command TEXT NOT NULL,
                task_type TEXT DEFAULT 'bash',
                config TEXT DEFAULT '{}',
                max_retries INTEGER DEFAULT 0,
                retry_delay_secs INTEGER DEFAULT 30,
                PRIMARY KEY (id, dag_id),
                FOREIGN KEY (dag_id) REFERENCES dags(id)
            )",
            [],
        )?;

        // Task Columns migrations
        let task_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(tasks)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !task_cols.contains(&"task_type".to_string()) {
            conn.execute("ALTER TABLE tasks ADD COLUMN task_type TEXT DEFAULT 'bash'", [])?;
        }
        if !task_cols.contains(&"config".to_string()) {
            conn.execute("ALTER TABLE tasks ADD COLUMN config TEXT DEFAULT '{}'", [])?;
        }
        if !task_cols.contains(&"max_retries".to_string()) {
            conn.execute("ALTER TABLE tasks ADD COLUMN max_retries INTEGER DEFAULT 0", [])?;
        }
        if !task_cols.contains(&"retry_delay_secs".to_string()) {
            conn.execute("ALTER TABLE tasks ADD COLUMN retry_delay_secs INTEGER DEFAULT 30", [])?;
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS task_instances (
                id TEXT PRIMARY KEY,
                dag_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                state TEXT NOT NULL,
                execution_date DATETIME NOT NULL,
                start_time DATETIME,
                end_time DATETIME,
                try_number INTEGER DEFAULT 1,
                FOREIGN KEY (dag_id) REFERENCES dags(id),
                FOREIGN KEY (task_id, dag_id) REFERENCES tasks(id, dag_id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS dag_runs (
                id TEXT PRIMARY KEY,
                dag_id TEXT NOT NULL,
                state TEXT NOT NULL,
                execution_date DATETIME NOT NULL,
                start_time DATETIME,
                end_time DATETIME,
                triggered_by TEXT DEFAULT 'scheduler',
                FOREIGN KEY (dag_id) REFERENCES dags(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (
                username TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL,
                api_key TEXT UNIQUE
            )",
            [],
        )?;

        // Pillar 3: Secrets Table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at DATETIME NOT NULL
            )",
            [],
        )?;

        // Seed users if not exists
        conn.execute(
            "INSERT OR IGNORE INTO users (username, password_hash, role, api_key) VALUES (?1, ?2, ?3, ?4)",
            params!["admin", "admin", "Admin", "vortex_admin_key"],
        )?;

        // Phase 2.4: DAG Versioning
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dag_versions (
                id TEXT PRIMARY KEY,
                dag_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn: Mutex::new(conn) })
    }

    // --- Phase 2.4: DAG Versioning ---

    pub fn store_dag_version(&self, dag_id: &str, file_path: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let next_version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM dag_versions WHERE dag_id = ?1",
            params![dag_id],
            |row| row.get(0),
        )?;

        let id = format!("{}-{}", dag_id, next_version);
        conn.execute(
            "INSERT INTO dag_versions (id, dag_id, version, file_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, dag_id, next_version, file_path, Utc::now().to_rfc3339()],
        )?;
        Ok(next_version)
    }

    pub fn get_dag_versions(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, dag_id, version, file_path, created_at FROM dag_versions WHERE dag_id = ?1 ORDER BY version DESC")?;
        let rows = stmt.query_map(params![dag_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "dag_id": row.get::<_, String>(1)?,
                "version": row.get::<_, i32>(2)?,
                "file_path": row.get::<_, String>(3)?,
                "created_at": row.get::<_, String>(4)?
            }))
        })?;
        let mut versions = Vec::new();
        for row in rows {
            versions.push(row?);
        }
        Ok(versions)
    }

    pub fn get_latest_version(&self, dag_id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, dag_id, version, file_path, created_at FROM dag_versions WHERE dag_id = ?1 ORDER BY version DESC LIMIT 1")?;
        let mut rows = stmt.query_map(params![dag_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "dag_id": row.get::<_, String>(1)?,
                "version": row.get::<_, i32>(2)?,
                "file_path": row.get::<_, String>(3)?,
                "created_at": row.get::<_, String>(4)?
            }))
        })?;
        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    // --- RBAC Support ---

    pub fn create_user(&self, username: &str, password_hash: &str, role: &str, api_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, password_hash, role, api_key) VALUES (?1, ?2, ?3, ?4)",
            params![username, password_hash, role, api_key],
        )?;
        Ok(())
    }

    pub fn delete_user(&self, username: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM users WHERE username = ?1", params![username])?;
        Ok(())
    }

    pub fn get_all_users(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT username, role, api_key FROM users")?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "username": row.get::<_, String>(0)?,
                "role": row.get::<_, String>(1)?,
                "api_key": row.get::<_, String>(2)?
            }))
        })?;
        let mut users = Vec::new();
        for row in rows {
            users.push(row?);
        }
        Ok(users)
    }

    // --- Secrets Support ---
    
    pub fn store_secret(&self, key: &str, encrypted_value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO secrets (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key, encrypted_value, Utc::now()],
        )?;
        Ok(())
    }

    pub fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM secrets WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get(0))?;
        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    pub fn get_all_secrets(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key FROM secrets")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut keys = Vec::new();
        for row in rows {
            keys.push(row?);
        }
        Ok(keys)
    }

    pub fn delete_secret(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM secrets WHERE key = ?1", params![key])?;
        Ok(())
    }

    // (Original DB methods remain below...)
    pub fn register_dag(&self, dag: &crate::scheduler::Dag) -> Result<()> {
        self.save_dag(&dag.id, dag.schedule_interval.as_deref())?;
        self.update_dag_config(&dag.id, dag.schedule_interval.as_deref(), &dag.timezone, dag.max_active_runs, dag.catchup)?;
        for task in dag.tasks.values() {
            self.save_task(&dag.id, &task.id, &task.name, &task.command, &task.task_type, &task.config.to_string(), task.max_retries, task.retry_delay_secs)?;
        }
        Ok(())
    }

    pub fn update_dag_config(&self, dag_id: &str, schedule_interval: Option<&str>, timezone: &str, max_active_runs: i32, catchup: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE dags SET schedule_interval = ?1, timezone = ?2, max_active_runs = ?3, catchup = ?4 WHERE id = ?5",
            params![schedule_interval, timezone, max_active_runs, catchup, dag_id])?;
        Ok(())
    }

    pub fn get_all_dags(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, created_at, schedule_interval, last_run, is_paused, timezone, max_active_runs, catchup, next_run FROM dags")?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "created_at": row.get::<_, DateTime<Utc>>(1)?,
                "schedule_interval": row.get::<_, Option<String>>(2)?,
                "last_run": row.get::<_, Option<DateTime<Utc>>>(3)?,
                "is_paused": row.get::<_, bool>(4)?,
                "timezone": row.get::<_, Option<String>>(5)?,
                "max_active_runs": row.get::<_, i32>(6)?,
                "catchup": row.get::<_, bool>(7)?,
                "next_run": row.get::<_, Option<DateTime<Utc>>>(8)?
            }))
        })?;
        let mut dags = Vec::new();
        for row in rows { dags.push(row?); }
        Ok(dags)
    }

    pub fn get_dag_by_id(&self, dag_id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, created_at, schedule_interval, last_run, is_paused, timezone, max_active_runs, catchup, next_run FROM dags WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![dag_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "created_at": row.get::<_, DateTime<Utc>>(1)?,
                "schedule_interval": row.get::<_, Option<String>>(2)?,
                "last_run": row.get::<_, Option<DateTime<Utc>>>(3)?,
                "is_paused": row.get::<_, bool>(4)?,
                "timezone": row.get::<_, Option<String>>(5)?,
                "max_active_runs": row.get::<_, i32>(6)?,
                "catchup": row.get::<_, bool>(7)?,
                "next_run": row.get::<_, Option<DateTime<Utc>>>(8)?
            }))
        })?;
        if let Some(row) = rows.next() { Ok(Some(row?)) } else { Ok(None) }
    }

    pub fn get_dag_tasks(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, command FROM tasks WHERE dag_id = ?1")?;
        let rows = stmt.query_map(params![dag_id], |row| {
            Ok(serde_json::json!({"id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?, "command": row.get::<_, String>(2)?}))
        })?;
        let mut tasks = Vec::new();
        for row in rows { tasks.push(row?); }
        Ok(tasks)
    }

    pub fn get_task_instances(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, task_id, state, execution_date, start_time, end_time, stdout, stderr, duration_ms, retry_count FROM task_instances WHERE dag_id = ?1")?;
        let rows = stmt.query_map(params![dag_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, 
                "task_id": row.get::<_, String>(1)?, 
                "state": row.get::<_, String>(2)?, 
                "execution_date": row.get::<_, DateTime<Utc>>(3)?, 
                "start_time": row.get::<_, Option<DateTime<Utc>>>(4)?, 
                "end_time": row.get::<_, Option<DateTime<Utc>>>(5)?,
                "stdout": row.get::<_, Option<String>>(6)?,
                "stderr": row.get::<_, Option<String>>(7)?,
                "duration_ms": row.get::<_, Option<i64>>(8)?,
                "retry_count": row.get::<_, i32>(9)?
            }))
        })?;
        let mut instances = Vec::new();
        for row in rows { instances.push(row?); }
        Ok(instances)
    }

    pub fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT username, role FROM users WHERE api_key = ?1")?;
        let mut rows = stmt.query_map(params![api_key], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if let Some(row) = rows.next() { Ok(Some(row?)) } else { Ok(None) }
    }

    pub fn get_task_instance(&self, ti_id: &str) -> Result<Option<(String, String, DateTime<Utc>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT dag_id, task_id, execution_date FROM task_instances WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![ti_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        if let Some(row) = rows.next() { Ok(Some(row?)) } else { Ok(None) }
    }

    pub fn save_dag(&self, dag_id: &str, schedule_interval: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO dags (id, created_at, schedule_interval) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET schedule_interval = excluded.schedule_interval", params![dag_id, Utc::now(), schedule_interval])?;
        Ok(())
    }

    pub fn update_dag_last_run(&self, dag_id: &str, last_run: DateTime<Utc>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE dags SET last_run = ?1 WHERE id = ?2", params![last_run, dag_id])?;
        Ok(())
    }

    pub fn update_dag_next_run(&self, dag_id: &str, next_run: Option<DateTime<Utc>>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE dags SET next_run = ?1 WHERE id = ?2", params![next_run, dag_id])?;
        Ok(())
    }

    pub fn get_scheduled_dags(&self) -> Result<Vec<(String, String, Option<DateTime<Utc>>, bool, String, i32, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, schedule_interval, last_run, is_paused, timezone, max_active_runs, catchup FROM dags WHERE schedule_interval IS NOT NULL AND schedule_interval != ''")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, bool>(3)?, row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "UTC".to_string()), row.get::<_, i32>(5)?, row.get::<_, bool>(6)?)))?;
        let mut dags = Vec::new();
        for row in rows { dags.push(row?); }
        Ok(dags)
    }

    pub fn pause_dag(&self, dag_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE dags SET is_paused = 1 WHERE id = ?1", params![dag_id])?;
        Ok(())
    }

    pub fn unpause_dag(&self, dag_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE dags SET is_paused = 0 WHERE id = ?1", params![dag_id])?;
        Ok(())
    }

    pub fn get_active_dag_run_count(&self, dag_id: &str) -> Result<i32> {
        let conn = self.conn.lock().unwrap();
        let count: i32 = conn.query_row("SELECT COUNT(*) FROM dag_runs WHERE dag_id = ?1 AND state IN ('Queued', 'Running')", params![dag_id], |row| row.get(0))?;
        Ok(count)
    }

    pub fn create_dag_run(&self, id: &str, dag_id: &str, execution_date: DateTime<Utc>, triggered_by: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO dag_runs (id, dag_id, state, execution_date, start_time, triggered_by) VALUES (?1, ?2, 'Queued', ?3, ?4, ?5)", params![id, dag_id, execution_date, execution_date, triggered_by])?;
        Ok(())
    }

    pub fn update_dag_run_state(&self, id: &str, state: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        match state {
            "Running" => { conn.execute("UPDATE dag_runs SET state = ?1, start_time = ?2 WHERE id = ?3", params![state, now, id])?; }
            "Success" | "Failed" => { conn.execute("UPDATE dag_runs SET state = ?1, end_time = ?2 WHERE id = ?3", params![state, now, id])?; }
            _ => { conn.execute("UPDATE dag_runs SET state = ?1 WHERE id = ?2", params![state, id])?; }
        }
        Ok(())
    }

    pub fn get_dag_runs(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, dag_id, state, execution_date, start_time, end_time, triggered_by FROM dag_runs WHERE dag_id = ?1 ORDER BY execution_date DESC LIMIT 100")?;
        let rows = stmt.query_map(params![dag_id], |row| Ok(serde_json::json!({"id": row.get::<_, String>(0)?, "dag_id": row.get::<_, String>(1)?, "state": row.get::<_, String>(2)?, "execution_date": row.get::<_, DateTime<Utc>>(3)?, "start_time": row.get::<_, Option<DateTime<Utc>>>(4)?, "end_time": row.get::<_, Option<DateTime<Utc>>>(5)?, "triggered_by": row.get::<_, String>(6)?})))?;
        let mut runs = Vec::new();
        for row in rows { runs.push(row?); }
        Ok(runs)
    }

    pub fn save_task(&self, dag_id: &str, task_id: &str, name: &str, command: &str, task_type: &str, config: &str, max_retries: i32, retry_delay_secs: i32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT OR REPLACE INTO tasks (id, dag_id, name, command, task_type, config, max_retries, retry_delay_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", 
            params![task_id, dag_id, name, command, task_type, config, max_retries, retry_delay_secs])?;
        Ok(())
    }

    pub fn create_task_instance(&self, id: &str, dag_id: &str, task_id: &str, execution_date: DateTime<Utc>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT OR REPLACE INTO task_instances (id, dag_id, task_id, state, execution_date) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, dag_id, task_id, "Queued", execution_date])?;
        Ok(())
    }

    pub fn update_task_state(&self, id: &str, state: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        match state {
            "Running" => { conn.execute("UPDATE task_instances SET state = ?1, start_time = ?2 WHERE id = ?3", params![state, now, id])?; }
            "Success" | "Failed" => { conn.execute("UPDATE task_instances SET state = ?1, end_time = ?2 WHERE id = ?3", params![state, now, id])?; }
            _ => { conn.execute("UPDATE task_instances SET state = ?1 WHERE id = ?2", params![state, id])?; }
        }
        Ok(())
    }

    pub fn get_interrupted_tasks(&self) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, dag_id, task_id FROM task_instances WHERE state = 'Running'")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        let mut interrupted = Vec::new();
        for row in rows { interrupted.push(row?); }
        Ok(interrupted)
    }

    // --- Pillar 4: Resilience Support ---

    pub fn upsert_worker(&self, id: &str, hostname: &str, capacity: i32, labels: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO workers (id, hostname, capacity, last_heartbeat, state, labels)
             VALUES (?1, ?2, ?3, ?4, 'Active', ?5)
             ON CONFLICT(id) DO UPDATE SET
                hostname = excluded.hostname,
                capacity = excluded.capacity,
                last_heartbeat = excluded.last_heartbeat,
                state = CASE WHEN workers.state = 'Draining' THEN 'Draining' ELSE 'Active' END,
                labels = excluded.labels",
            params![id, hostname, capacity, Utc::now(), labels],
        )?;
        Ok(())
    }

    pub fn update_worker_heartbeat(&self, id: &str, active_tasks: i32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE workers SET last_heartbeat = ?1, active_tasks = ?2 WHERE id = ?3",
            params![Utc::now(), active_tasks, id],
        )?;
        Ok(())
    }

    pub fn mark_stale_workers_offline(&self, timeout_seconds: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let cutoff = Utc::now() - chrono::Duration::seconds(timeout_seconds);
        
        let mut stmt = conn.prepare("SELECT id FROM workers WHERE last_heartbeat < ?1 AND state != 'Offline'")?;
        let rows = stmt.query_map(params![cutoff], |row| row.get::<_, String>(0))?;
        let mut stale_worker_ids = Vec::new();
        for id in rows {
            stale_worker_ids.push(id?);
        }

        if !stale_worker_ids.is_empty() {
            conn.execute(
                "UPDATE workers SET state = 'Offline' WHERE last_heartbeat < ?1 AND state != 'Offline'",
                params![cutoff],
            )?;
        }
        
        Ok(stale_worker_ids)
    }

    pub fn requeue_worker_tasks(&self, worker_id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute(
            "UPDATE task_instances SET state = 'Queued'
             WHERE worker_id = ?1 AND state = 'Running'",
            params![worker_id],
        )?;
        Ok(count)
    }

    pub fn assign_task_to_worker(&self, ti_id: &str, worker_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE task_instances SET state = 'Running', worker_id = ?1, start_time = ?2 WHERE id = ?3",
            params![worker_id, Utc::now(), ti_id],
        )?;
        Ok(())
    }

    pub fn get_interrupted_tasks_by_worker(&self, worker_id: &str) -> Result<Vec<(String, String, String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        // Since we already set state='Queued' and worker_id=NULL in requeue_worker_tasks,
        // we need to find those specific tasks. But wait, if worker_id is NULL, how do we find them?
        // Actually, the previous update set worker_id = NULL. That was a mistake if I wanted to use it here.
        // Let's modify the requeue logic to mark them as 'Offline' or similar first, then move to Queued.
        // Or just keep the worker_id until we finish requeueing.
        
        let mut stmt = conn.prepare("
            SELECT ti.id, ti.dag_id, ti.task_id, t.command, dr.id
            FROM task_instances ti
            JOIN tasks t ON ti.task_id = t.id AND ti.dag_id = t.dag_id
            JOIN dag_runs dr ON ti.dag_id = dr.dag_id AND ti.execution_date = dr.execution_date
            WHERE ti.worker_id = ?1 AND ti.state = 'Queued'
        ")?;
        let rows = stmt.query_map(params![worker_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))?;
        let mut tasks = Vec::new();
        for t in rows { tasks.push(t?); }
        Ok(tasks)
    }

    pub fn clear_worker_id_from_queued_tasks(&self, worker_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE task_instances SET worker_id = NULL WHERE worker_id = ?1 AND state = 'Queued'",
            params![worker_id],
        )?;
        Ok(())
    }

    pub fn get_task_instance_retry_info(&self, ti_id: &str) -> Result<(i32, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT retry_count, state FROM task_instances WHERE id = ?1")?;
        let row = stmt.query_row(params![ti_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(row)
    }

    pub fn increment_task_retry_count(&self, ti_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE task_instances SET retry_count = retry_count + 1 WHERE id = ?1", params![ti_id])?;
        Ok(())
    }

    pub fn get_task_instance_details(&self, ti_id: &str) -> Result<Option<(String, String, String, String, String, String, i32, i32)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("
            SELECT ti.dag_id, ti.task_id, t.command, dr.id, t.task_type, t.config, t.max_retries, t.retry_delay_secs
            FROM task_instances ti
            JOIN tasks t ON ti.task_id = t.id AND ti.dag_id = t.dag_id
            JOIN dag_runs dr ON ti.dag_id = dr.dag_id AND ti.execution_date = dr.execution_date
            WHERE ti.id = ?1
        ")?;
        let mut rows = stmt.query_map(params![ti_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?))
        })?;
        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    pub fn store_task_result(&self, task_instance_id: &str, result: &crate::executor::ExecutionResult) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let state = if result.success { "Success" } else { "Failed" };
        let now = Utc::now();
        conn.execute(
            "UPDATE task_instances 
             SET state = ?1, 
                 stdout = ?2, 
                 stderr = ?3, 
                 duration_ms = ?4, 
                 end_time = ?5 
             WHERE id = ?6",
            params![
                state,
                result.stdout,
                result.stderr,
                result.duration_ms as i64,
                now,
                task_instance_id
            ],
        )?;
        Ok(())
    }
}
