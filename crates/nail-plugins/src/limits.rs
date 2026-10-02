//! The sandbox limits of every plug-in runtime.

use std::time::Duration;

/// Limits of a plug-in runtime and its API use. [`HostLimits::default`] is
/// what Nail uses; tests shrink them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLimits {
    /// QuickJS heap per plug-in (bytes). Exceeding it stops the plug-in
    /// (`memory-limit`).
    pub memory_bytes: usize,
    /// QuickJS stack per plug-in (bytes); deeper recursion throws a
    /// `RangeError` the plug-in can catch.
    pub stack_bytes: usize,
    /// CPU time of the start: evaluating the module and the synchronous
    /// part of `activate` (and every turn until `activate` settles).
    pub activation_budget: Duration,
    /// CPU time of one call into the plug-in: a command, a resolved API
    /// call, a dialog result, a UI action, a panel render, a timer, with
    /// the promise jobs it queues. Hook, job and extension handler turns
    /// (M4a) have it too, next to their own wall-clock limits. Exceeding it
    /// stops the plug-in (`cpu-budget`).
    pub call_budget: Duration,
    /// Wall-clock time until `activate` must have settled.
    pub activation_timeout: Duration,
    /// Wall-clock time a UI waits for a command to be taken or a panel to
    /// render.
    pub reply_timeout: Duration,
    /// API calls a plug-in may have waiting at once.
    pub max_pending_calls: usize,
    /// Dialogs a plug-in may have open at once.
    pub max_open_dialogs: usize,
    /// Timers (`setTimeout`) a plug-in may have at once.
    pub max_timers: usize,
    /// Longest timer delay.
    pub max_timer_delay: Duration,
    /// Log lines one call may cause (its own, refused API calls, host
    /// lines; more are counted and dropped).
    pub max_log_lines_per_call: u32,
    /// Log lines a plug-in may cause per minute (a burst of this many,
    /// refilled evenly; more are counted and dropped).
    pub max_log_lines_per_minute: u32,
    /// Notifications per minute.
    pub notifications_per_minute: usize,
    /// Largest `nail.http.fetch` request body (bytes).
    pub http_max_request_bytes: usize,
    /// Largest `nail.http.fetch` response body (bytes).
    pub http_max_response_bytes: u64,
    /// `nail.http.fetch` time-out.
    pub http_timeout: Duration,
    /// Most results of `nail.mail.search`.
    pub max_search_results: u32,
    /// Modules a plug-in may load (its main module and its imports).
    pub max_modules: usize,
    /// M4a: CPU time a plug-in may use per hour across all its calls
    /// (turns); over it, its background jobs are skipped (`cpu-quota`)
    /// until the hour has passed.
    pub cpu_quota_per_hour: Duration,
    /// M4a: wall-clock time of one job run, pending promises included.
    pub job_wall_clock: Duration,
    /// M4a: how often the scheduler looks for due jobs (the core can also
    /// ask at once, `PluginHost::run_due_jobs`).
    pub job_tick: Duration,
    /// M4a: how long event deliveries are collected into one job run.
    pub event_coalesce: Duration,
    /// M4a: wall-clock time of an exposed MCP tool call.
    pub mcp_tool_timeout: Duration,
    /// M4a: largest result of an MCP tool call (bytes of JSON).
    pub max_tool_result_bytes: usize,
    /// M4a: bytes a plug-in may hand over as `Uint8Array` / `ArrayBuffer`
    /// in one turn (fetch bodies, attachments, preview images).
    pub max_blob_bytes: usize,
    /// M4a: how long after a composer action ran its plug-in may still edit
    /// that draft (`nail.composer.*`).
    pub composer_edit_window: Duration,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            stack_bytes: 1024 * 1024,
            activation_budget: Duration::from_secs(2),
            call_budget: Duration::from_secs(1),
            activation_timeout: Duration::from_secs(30),
            reply_timeout: Duration::from_secs(10),
            max_pending_calls: 64,
            max_open_dialogs: 5,
            max_timers: 100,
            max_timer_delay: Duration::from_secs(24 * 60 * 60),
            max_log_lines_per_call: 200,
            max_log_lines_per_minute: 600,
            notifications_per_minute: 20,
            http_max_request_bytes: 1024 * 1024,
            http_max_response_bytes: 5 * 1024 * 1024,
            http_timeout: Duration::from_secs(30),
            max_search_results: 500,
            max_modules: 500,
            cpu_quota_per_hour: Duration::from_millis(
                nail_api::plugins::jobs::CPU_QUOTA_MS_PER_HOUR,
            ),
            job_wall_clock: Duration::from_millis(nail_api::plugins::jobs::JOB_WALL_CLOCK_MS),
            job_tick: Duration::from_secs(15),
            event_coalesce: Duration::from_millis(nail_api::plugins::jobs::EVENT_COALESCE_MS),
            mcp_tool_timeout: Duration::from_secs(60),
            max_tool_result_bytes: 1024 * 1024,
            max_blob_bytes: 32 * 1024 * 1024,
            composer_edit_window: Duration::from_secs(5 * 60),
        }
    }
}
