//! # RYUO Prometheus Metrics
//!
//! Self-contained Prometheus metrics module for the RYUO orchestration engine.
//!
//! ## Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use axum::{routing::get, Router};
//! use ryuo::metrics::{RyuoMetrics, metrics_handler};
//!
//! // In your app setup:
//! let metrics = Arc::new(RyuoMetrics::new().expect("failed to init metrics"));
//!
//! // In your Axum router:
//! let app = Router::new()
//!     .route("/metrics", get(metrics_handler))
//!     .with_state(metrics);
//! ```

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge,
    Opts, Registry, TextEncoder,
};

// ─────────────────────────────────────────────────────────────────────────────
// Metric name constants
// ─────────────────────────────────────────────────────────────────────────────

const NAMESPACE: &str = "ryuo";

// ─────────────────────────────────────────────────────────────────────────────
// Gauge safety helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Decrement a Prometheus [`IntGauge`] only if its current value is positive.
///
/// Out-of-order events (e.g. a duplicated task-completion callback) can cause
/// `gauge.dec()` to be called when the gauge is already at 0.  A negative
/// gauge value is nonsensical for resource counts and confuses dashboards
/// and alerts.  This helper prevents that.
///
/// **Why not just check `gauge.get() > 0` inline?**  
/// An `IntGauge` is backed by an `AtomicI64`, so `get()` followed by `dec()`
/// is technically a TOCTOU window — but Prometheus gauges are *advisory*
/// telemetry, not correctness-critical locks.  The cost of briefly reading
/// a stale value (missing one decrement under extreme contention) is far
/// lower than the cost of publishing a negative count, which triggers false
/// alerts.
#[inline]
pub fn safe_gauge_dec(gauge: &IntGauge) {
    if gauge.get() > 0 {
        gauge.dec();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RyuoMetrics
// ─────────────────────────────────────────────────────────────────────────────

/// Central Prometheus metrics registry for the RYUO engine.
///
/// All metric types from the `prometheus` crate are `Send + Sync`, so this
/// struct is safe to share across threads via `Arc<RyuoMetrics>`.
pub struct RyuoMetrics {
    /// Private registry — we don't use the global default so multiple test
    /// instances don't collide.
    registry: Registry,

    // ── DAG / task state gauges ──────────────────────────────────────────────
    /// Total number of registered DAGs in the engine.
    pub dags_total: IntGauge,
    /// Number of tasks currently in RUNNING state.
    pub tasks_running: IntGauge,
    /// Number of tasks currently sitting in the run-queue.
    pub tasks_queued: IntGauge,

    // ── Task outcome counters ────────────────────────────────────────────────
    /// Monotonically increasing count of tasks that finished successfully.
    pub tasks_succeeded_total: IntCounter,
    /// Monotonically increasing count of tasks that finished with a failure.
    pub tasks_failed_total: IntCounter,

    // ── Latency histogram ────────────────────────────────────────────────────
    /// Wall-clock execution time of every task (seconds).
    pub task_duration_seconds: Histogram,

    // ── DAG-run counter vec ──────────────────────────────────────────────────
    /// Total DAG runs broken down by terminal state label ("success" / "failed").
    pub dag_runs_total: IntCounterVec,

    // ── Swarm metrics ────────────────────────────────────────────────────────
    /// Number of swarm workers currently registered and active.
    pub workers_active: IntGauge,
    /// Number of tasks currently sitting in the swarm dispatch queue.
    pub queue_depth: IntGauge,

    // ── Scheduler health ─────────────────────────────────────────────────────
    /// Unix epoch (seconds) of the last scheduler tick. Use this to build a
    /// "heartbeat staleness" alert in Grafana: `time() - ryuo_scheduler_heartbeat_timestamp`.
    pub scheduler_heartbeat_timestamp: IntGauge,
}

impl RyuoMetrics {
    // ─────────────────────────────────────────────────────────────────────────
    // Constructor
    // ─────────────────────────────────────────────────────────────────────────

    /// Create and register all RYUO metrics in a fresh, isolated [`Registry`].
    ///
    /// Returns an error if any metric registration fails (e.g., duplicate name).
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        // ── DAG / task gauges ────────────────────────────────────────────────
        let dags_total = IntGauge::with_opts(
            Opts::new("dags_total", "Total number of registered DAGs")
                .namespace(NAMESPACE),
        )?;

        let tasks_running = IntGauge::with_opts(
            Opts::new("tasks_running", "Number of tasks currently in RUNNING state")
                .namespace(NAMESPACE),
        )?;

        let tasks_queued = IntGauge::with_opts(
            Opts::new("tasks_queued", "Number of tasks currently queued for execution")
                .namespace(NAMESPACE),
        )?;

        // ── Task outcome counters ────────────────────────────────────────────
        let tasks_succeeded_total = IntCounter::with_opts(
            Opts::new(
                "tasks_succeeded_total",
                "Total number of tasks that completed successfully",
            )
            .namespace(NAMESPACE),
        )?;

        let tasks_failed_total = IntCounter::with_opts(
            Opts::new(
                "tasks_failed_total",
                "Total number of tasks that completed with a failure",
            )
            .namespace(NAMESPACE),
        )?;

        // ── Task duration histogram ──────────────────────────────────────────
        // Buckets cover sub-second to ~10 minute workloads.
        let task_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "task_duration_seconds",
                "Wall-clock execution time of tasks in seconds",
            )
            .namespace(NAMESPACE)
            .buckets(vec![
                0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
                600.0,
            ]),
        )?;

        // ── DAG run counter vec ──────────────────────────────────────────────
        let dag_runs_total = IntCounterVec::new(
            Opts::new("dag_runs_total", "Total DAG runs by terminal state")
                .namespace(NAMESPACE),
            &["state"],
        )?;

        // ── Swarm metrics ────────────────────────────────────────────────────
        let workers_active = IntGauge::with_opts(
            Opts::new("workers_active", "Number of active swarm workers")
                .namespace(NAMESPACE),
        )?;

        let queue_depth = IntGauge::with_opts(
            Opts::new(
                "queue_depth",
                "Number of tasks currently in the swarm dispatch queue",
            )
            .namespace(NAMESPACE),
        )?;

        // ── Scheduler heartbeat ──────────────────────────────────────────────
        let scheduler_heartbeat_timestamp = IntGauge::with_opts(
            Opts::new(
                "scheduler_heartbeat_timestamp",
                "Unix epoch (seconds) of the last scheduler tick",
            )
            .namespace(NAMESPACE),
        )?;

        // ── Registration ────────────────────────────────────────────────────
        registry.register(Box::new(dags_total.clone()))?;
        registry.register(Box::new(tasks_running.clone()))?;
        registry.register(Box::new(tasks_queued.clone()))?;
        registry.register(Box::new(tasks_succeeded_total.clone()))?;
        registry.register(Box::new(tasks_failed_total.clone()))?;
        registry.register(Box::new(task_duration_seconds.clone()))?;
        registry.register(Box::new(dag_runs_total.clone()))?;
        registry.register(Box::new(workers_active.clone()))?;
        registry.register(Box::new(queue_depth.clone()))?;
        registry.register(Box::new(scheduler_heartbeat_timestamp.clone()))?;

        Ok(Self {
            registry,
            dags_total,
            tasks_running,
            tasks_queued,
            tasks_succeeded_total,
            tasks_failed_total,
            task_duration_seconds,
            dag_runs_total,
            workers_active,
            queue_depth,
            scheduler_heartbeat_timestamp,
        })
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helper methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Called when a task transitions from QUEUED → RUNNING.
    ///
    /// Increments `tasks_running` and decrements `tasks_queued`.
    /// Bug 17 fix: both gauges are clamped to 0 to prevent negative values when
    /// record_task_start / success / failure are called out of order.
    #[inline]
    pub fn record_task_start(&self) {
        self.tasks_running.inc();
        safe_gauge_dec(&self.tasks_queued);
    }

    /// Called when a task finishes successfully.
    ///
    /// Decrements `tasks_running`, increments `tasks_succeeded_total`, and
    /// records the wall-clock `duration_secs` in the histogram.
    /// Bug 17 fix: `tasks_running` is clamped to 0 minimum.
    #[inline]
    pub fn record_task_success(&self, duration_secs: f64) {
        safe_gauge_dec(&self.tasks_running);
        self.tasks_succeeded_total.inc();
        self.task_duration_seconds.observe(duration_secs);
    }

    /// Called when a task finishes with a failure.
    ///
    /// Decrements `tasks_running`, increments `tasks_failed_total`, and
    /// records the wall-clock `duration_secs` in the histogram.
    /// Bug 17 fix: `tasks_running` is clamped to 0 minimum.
    #[inline]
    pub fn record_task_failure(&self, duration_secs: f64) {
        safe_gauge_dec(&self.tasks_running);
        self.tasks_failed_total.inc();
        self.task_duration_seconds.observe(duration_secs);
    }

    /// Called when a task is accepted into the run-queue.
    ///
    /// Increments `tasks_queued`.
    #[inline]
    pub fn record_task_queued(&self) {
        self.tasks_queued.inc();
    }

    /// Called when a DAG run reaches a terminal state.
    ///
    /// `state` should be `"success"` or `"failed"` (other values are valid
    /// label values but won't match default Grafana panels).
    #[inline]
    pub fn record_dag_run_complete(&self, state: &str) {
        self.dag_runs_total.with_label_values(&[state]).inc();
    }

    /// Sets `scheduler_heartbeat_timestamp` to the current Unix epoch (seconds).
    ///
    /// Call this at the top of every scheduler tick so Grafana can alert on
    /// staleness: `time() - ryuo_scheduler_heartbeat_timestamp > threshold`.
    #[inline]
    pub fn update_scheduler_heartbeat(&self) {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.scheduler_heartbeat_timestamp.set(epoch);
    }

    /// Sets the number of active swarm workers.
    #[inline]
    pub fn set_workers_active(&self, count: i64) {
        self.workers_active.set(count);
    }

    /// Sets the current swarm dispatch queue depth.
    #[inline]
    pub fn set_queue_depth(&self, depth: i64) {
        self.queue_depth.set(depth);
    }

    /// Sets the total number of registered DAGs.
    #[inline]
    pub fn set_dags_total(&self, count: i64) {
        self.dags_total.set(count);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Internal helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Render all registered metrics into the Prometheus text exposition format.
    pub(crate) fn render_text(&self) -> anyhow::Result<String> {
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        let mut buf = Vec::with_capacity(4096);
        encoder.encode(&families, &mut buf)?;
        Ok(String::from_utf8(buf)?)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Axum handler
// ─────────────────────────────────────────────────────────────────────────────

/// Axum handler for `GET /metrics`.
///
/// Wire this into your router with `State<Arc<RyuoMetrics>>`:
///
/// ```ignore
/// use std::sync::Arc;
/// use axum::{routing::get, Router};
/// use ryuo::metrics::{RyuoMetrics, metrics_handler};
///
/// let metrics = Arc::new(RyuoMetrics::new().unwrap());
/// let app = Router::new()
///     .route("/metrics", get(metrics_handler))
///     .with_state(metrics);
/// ```
pub async fn metrics_handler(
    State(metrics): State<Arc<RyuoMetrics>>,
) -> Response {
    match metrics.render_text() {
        Ok(body) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                // Standard Prometheus text exposition MIME type.
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            body,
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to encode metrics: {err}"),
        )
            .into_response(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_metrics() -> RyuoMetrics {
        RyuoMetrics::new().expect("metrics init failed")
    }

    #[test]
    fn test_new_registers_all_metrics() {
        let m = fresh_metrics();
        // Registry should be able to gather all 9 metric families.
        let families = m.registry.gather();
        assert_eq!(families.len(), 9, "expected 9 metric families");
    }

    #[test]
    fn test_record_task_queued_and_start() {
        let m = fresh_metrics();
        m.record_task_queued();
        assert_eq!(m.tasks_queued.get(), 1);
        assert_eq!(m.tasks_running.get(), 0);

        m.record_task_start();
        assert_eq!(m.tasks_queued.get(), 0);
        assert_eq!(m.tasks_running.get(), 1);
    }

    #[test]
    fn test_record_task_success() {
        let m = fresh_metrics();
        m.tasks_running.inc(); // simulate an already-running task
        m.record_task_success(1.5);

        assert_eq!(m.tasks_running.get(), 0);
        assert_eq!(m.tasks_succeeded_total.get(), 1);
    }

    #[test]
    fn test_record_task_failure() {
        let m = fresh_metrics();
        m.tasks_running.inc();
        m.record_task_failure(0.3);

        assert_eq!(m.tasks_running.get(), 0);
        assert_eq!(m.tasks_failed_total.get(), 1);
    }

    #[test]
    fn test_record_dag_run_complete() {
        let m = fresh_metrics();
        m.record_dag_run_complete("success");
        m.record_dag_run_complete("success");
        m.record_dag_run_complete("failed");

        assert_eq!(
            m.dag_runs_total
                .with_label_values(&["success"])
                .get(),
            2
        );
        assert_eq!(
            m.dag_runs_total
                .with_label_values(&["failed"])
                .get(),
            1
        );
    }

    #[test]
    fn test_update_scheduler_heartbeat() {
        let m = fresh_metrics();
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        m.update_scheduler_heartbeat();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let ts = m.scheduler_heartbeat_timestamp.get();
        assert!(ts >= before && ts <= after, "timestamp out of range");
    }

    #[test]
    fn test_set_helpers() {
        let m = fresh_metrics();
        m.set_workers_active(8);
        m.set_queue_depth(42);
        m.set_dags_total(7);

        assert_eq!(m.workers_active.get(), 8);
        assert_eq!(m.queue_depth.get(), 42);
        assert_eq!(m.dags_total.get(), 7);
    }

    #[test]
    fn test_render_text_is_valid_utf8() {
        let m = fresh_metrics();
        m.record_task_queued();
        m.record_task_start();
        m.record_task_success(2.0);
        m.record_dag_run_complete("success");
        m.update_scheduler_heartbeat();

        let text = m.render_text().expect("render failed");
        assert!(text.contains("ryuo_tasks_succeeded_total"));
        assert!(text.contains("ryuo_task_duration_seconds"));
        assert!(text.contains("ryuo_dag_runs_total"));
        assert!(text.contains("ryuo_scheduler_heartbeat_timestamp"));
    }

    // ── BUG-H4: Gauge safety tests ──────────────────────────────────────────

    #[test]
    fn test_safe_gauge_dec_prevents_negative() {
        let gauge = IntGauge::new("test_gauge", "test").unwrap();
        assert_eq!(gauge.get(), 0);

        // dec on zero gauge should be a no-op
        safe_gauge_dec(&gauge);
        assert_eq!(gauge.get(), 0, "gauge must not go negative");
    }

    #[test]
    fn test_safe_gauge_dec_normal_operation() {
        let gauge = IntGauge::new("test_gauge2", "test").unwrap();
        gauge.inc();
        gauge.inc();
        assert_eq!(gauge.get(), 2);

        safe_gauge_dec(&gauge);
        assert_eq!(gauge.get(), 1);

        safe_gauge_dec(&gauge);
        assert_eq!(gauge.get(), 0);

        // third dec should be clamped
        safe_gauge_dec(&gauge);
        assert_eq!(gauge.get(), 0, "gauge must not go negative after exhaustion");
    }

    #[test]
    fn test_duplicate_task_success_does_not_go_negative() {
        let m = fresh_metrics();
        m.record_task_queued();
        m.record_task_start();
        assert_eq!(m.tasks_running.get(), 1);

        // First completion — normal
        m.record_task_success(1.0);
        assert_eq!(m.tasks_running.get(), 0);

        // Duplicate completion — must stay at 0, not -1
        m.record_task_success(1.0);
        assert_eq!(m.tasks_running.get(), 0, "BUG-H4: tasks_running went negative");
    }

    #[test]
    fn test_task_start_without_prior_queue_does_not_go_negative() {
        let m = fresh_metrics();
        // start without a prior queue event
        m.record_task_start();
        assert_eq!(m.tasks_queued.get(), 0, "BUG-H4: tasks_queued went negative");
        assert_eq!(m.tasks_running.get(), 1);
    }
}
