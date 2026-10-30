// (C) Copyright IBM Corp. 2025.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Background analytics transmission thread, mirroring Python's `Analytics` class.
//!
//! ## Flush policy (matches Python exactly)
//!
//! * **Batch full (≥ 30 entries):** any data message unblocks the background
//!   thread's `recv_timeout` sleep.  On wake-up `drain_chunks()` sends all
//!   complete batches of 30 immediately.
//! * **5-minute timer:** background thread sleeps for the remaining time, wakes,
//!   and if the 5-minute interval has elapsed and the record is non-empty it
//!   calls `flush_remainder()` to send whatever is left.
//! * **`flush()` (explicit):** calls `drain_chunks()` then `flush_remainder()`.
//!   Synchronous — blocks until all data is sent.  No-op if the record is empty.
//! * **Never sends an empty payload** — no POST is made if the record has 0 entries.
//! * On 429: retry after **30 s**.
//! * On other error: exponential backoff starting at **60 s**, capped at **600 s**.
//!   Failed batches are merged back into the live record before sleeping.

use crate::analytics::client::AnalyticsClient;
use crate::analytics::record::{AnalyticsRecord, MetricDescriptor};
use crate::ConfigurationId;
use chrono::Utc;
use log::{debug, warn};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ── flush policy constants (matches Python) ────────────────────────────────────

const FLUSH_INTERVAL: Duration = Duration::from_secs(5 * 60); // 5 minutes
const BATCH_SIZE: usize = 30;
const RETRY_DELAY_RATE_LIMIT: Duration = Duration::from_secs(30);
const RETRY_DELAY_INITIAL: Duration = Duration::from_secs(60);
const RETRY_DELAY_MAX: Duration = Duration::from_secs(600);

// ── channel message types ──────────────────────────────────────────────────────

/// Messages sent from evaluation/track call sites to the background thread.
#[derive(Debug)]
pub(crate) enum AnalyticsMessage {
    GuardedEvaluation {
        rollout_id: String,
        entity_id: String,
        value_served: String,
    },
    GuardedMetric {
        rollout_id: String,
        entity_id: String,
        value_served: String,
        event_key: String,
        metrics: Vec<MetricDescriptor>,
    },
    /// Instructs the background thread to flush all data immediately and signal
    /// back via the provided one-shot channel when done.
    /// Mirrors Python's `flush()` which calls `drain_chunks` + `flush_remainder`.
    Flush {
        done: mpsc::SyncSender<()>,
    },
}

// ── public handle ──────────────────────────────────────────────────────────────

/// Cheap cloneable sender handle — stamp one onto every `FeatureSnapshot`.
#[derive(Debug, Clone)]
pub(crate) struct AnalyticsSender {
    pub(crate) tx: mpsc::Sender<AnalyticsMessage>,
}

impl AnalyticsSender {
    pub(crate) fn send_guarded_evaluation(
        &self,
        rollout_id: String,
        entity_id: String,
        value_served: String,
    ) {
        if let Err(e) = self.tx.send(AnalyticsMessage::GuardedEvaluation {
            rollout_id,
            entity_id,
            value_served,
        }) {
            warn!("Failed to enqueue analytics evaluation event: {e}");
        }
    }

    pub(crate) fn send_guarded_metric(
        &self,
        rollout_id: String,
        entity_id: String,
        value_served: String,
        event_key: String,
        metrics: Vec<MetricDescriptor>,
    ) {
        if let Err(e) = self.tx.send(AnalyticsMessage::GuardedMetric {
            rollout_id,
            entity_id,
            value_served,
            event_key,
            metrics,
        }) {
            warn!("Failed to enqueue analytics metric event: {e}");
        }
    }
}

/// Owned handle that keeps the background thread alive.
/// Dropping it disconnects the channel, causing the background thread to exit
/// after one final flush.  Call `.join()` in tests to wait for that flush.
pub(crate) struct AnalyticsHandle {
    pub(crate) sender: AnalyticsSender,
    #[allow(dead_code)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for AnalyticsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalyticsHandle").finish()
    }
}

impl AnalyticsHandle {
    /// Drop the sender and wait for the background thread to finish its final flush.
    /// Only used in tests.
    #[cfg(test)]
    pub(crate) fn join(mut self) {
        drop(self.sender);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    /// Flush all pending analytics data synchronously.
    ///
    /// Sends a `Flush` rendezvous message and blocks until the background thread
    /// acknowledges.  Equivalent to Python's `flush()` which calls
    /// `__drain_chunks()` then `__flush_remainder()`.  No POST is made if the
    /// record is empty.
    pub(crate) fn flush(&self) {
        let (tx, rx) = mpsc::sync_channel(0);
        if self.sender.tx.send(AnalyticsMessage::Flush { done: tx }).is_ok() {
            let _ = rx.recv();
        }
    }
}

// ── start function ─────────────────────────────────────────────────────────────

/// Spawn the background analytics thread and return a handle + cloneable sender.
pub(crate) fn start_analytics<C: AnalyticsClient>(
    config_id: ConfigurationId,
    client: C,
) -> AnalyticsHandle {
    let (tx, rx) = mpsc::channel::<AnalyticsMessage>();

    let thread = std::thread::spawn(move || {
        run_analytics_loop(rx, client, config_id);
    });

    AnalyticsHandle {
        sender: AnalyticsSender { tx },
        thread: Some(thread),
    }
}

// ── background loop ────────────────────────────────────────────────────────────
//
// Mirrors Python's `__send_analytic_job`:
//
//   last_flush = time()
//   while running:
//       remaining = 5min - elapsed_since_last_flush
//       flush_event.wait(timeout=remaining)   ← sleep, woken early by any message
//       flush_event.clear()
//       five_min_due = elapsed >= 5min
//       if is_full:
//           drain_chunks()           ← sends complete batches of 30
//           last_flush = time()
//       elif five_min_due and has_data:
//           flush_remainder()        ← sends whatever is left
//           last_flush = time()
//       # else: woken early but <30 entries and <5min due — loop back and sleep
//
// The Rust equivalent uses recv_timeout(remaining) instead of Event.wait().
// Any incoming channel message (data or Flush) wakes the thread early.

fn run_analytics_loop<C: AnalyticsClient>(
    rx: mpsc::Receiver<AnalyticsMessage>,
    client: C,
    config_id: ConfigurationId,
) {
    let mut record = AnalyticsRecord::new();
    let mut last_flush = Instant::now();

    debug!("Analytics background thread started");

    loop {
        // Sleep for the remaining 5-minute interval (woken early by any message).
        // Mirrors Python's `flush_event.wait(timeout=remaining)`.
        let remaining = FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());

        let msg = match rx.recv_timeout(remaining) {
            Ok(msg) => Some(msg),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // All senders dropped — final flush then exit.
                debug!("Analytics channel disconnected, doing final flush");
                flush_remainder(&client, &config_id, &mut record);
                break;
            }
        };

        // Drain all remaining messages from the channel without blocking.
        // This collapses bursts: many inserts may have queued up while we slept.
        let mut flush_done: Option<mpsc::SyncSender<()>> = None;
        if let Some(m) = msg {
            if let Some(done) = apply_message(&mut record, m) {
                flush_done = Some(done);
            }
        }
        while let Ok(m) = rx.try_recv() {
            if let Some(done) = apply_message(&mut record, m) {
                // Nested Flush while already handling a Flush — ack immediately.
                if flush_done.is_some() {
                    let _ = done.send(());
                } else {
                    flush_done = Some(done);
                }
            }
        }

        // ── Explicit Flush: drain_chunks() then flush_remainder() ────────────
        // Mirrors Python's `flush()`.
        if let Some(done_tx) = flush_done {
            drain_chunks(&client, &config_id, &mut record);
            flush_remainder(&client, &config_id, &mut record);
            if record.len() == 0 {
                last_flush = Instant::now();
            }
            let _ = done_tx.send(()); // unblock flush() caller
            continue;
        }

        let five_min_due = last_flush.elapsed() >= FLUSH_INTERVAL;
        let is_full = record.exceeds_limit(BATCH_SIZE);
        let has_data = record.len() > 0;

        if is_full {
            // Mirrors Python: `drain_chunks()` — send all complete batches.
            drain_chunks(&client, &config_id, &mut record);
        } else if five_min_due && has_data {
            // Mirrors Python: `flush_remainder()` — send the partial batch.
            flush_remainder(&client, &config_id, &mut record);
        }
        last_flush = Instant::now();
        // else: woken early by a data message but record still < 30
        // and 5-min not yet due — loop back and sleep for the remaining interval.
    }

    debug!("Analytics background thread exiting");
}

/// Apply one incoming message to the record.
/// Returns `Some(done_tx)` if the message is a `Flush` request.
fn apply_message(
    record: &mut AnalyticsRecord,
    msg: AnalyticsMessage,
) -> Option<mpsc::SyncSender<()>> {
    let now = Utc::now();
    match msg {
        AnalyticsMessage::GuardedEvaluation { rollout_id, entity_id, value_served } => {
            record.add_guarded_evaluation(rollout_id, entity_id, value_served, now);
            None
        }
        AnalyticsMessage::GuardedMetric { rollout_id, entity_id, value_served, event_key, metrics } => {
            record.add_guarded_metric(rollout_id, entity_id, value_served, event_key, metrics, now);
            None
        }
        AnalyticsMessage::Flush { done } => Some(done),
    }
}

// ── drain / flush helpers (mirrors Python's __drain_chunks / __flush_remainder) ─

/// Send all complete batches of BATCH_SIZE from the record, oldest first.
/// Leaves any remainder below BATCH_SIZE in the record.
/// Mirrors Python's `__drain_chunks()`.
fn drain_chunks<C: AnalyticsClient>(
    client: &C,
    config_id: &ConfigurationId,
    record: &mut AnalyticsRecord,
) {
    while record.exceeds_limit(BATCH_SIZE) {
        let batch = record.split_head(BATCH_SIZE);
        send_batch(client, config_id, batch, record);
    }
}

/// Send whatever is left in the record (< BATCH_SIZE entries).
/// No-op if the record is empty — never sends an empty payload.
/// Mirrors Python's `__flush_remainder()`.
fn flush_remainder<C: AnalyticsClient>(
    client: &C,
    config_id: &ConfigurationId,
    record: &mut AnalyticsRecord,
) {
    let n = record.len();
    if n == 0 {
        return; // never send empty payload
    }
    let batch = record.split_head(n);
    send_batch(client, config_id, batch, record);
}

/// Send `batch` with retry + merge-back on failure.
/// Mirrors Python's `__send_batch()`.
fn send_batch<C: AnalyticsClient>(
    client: &C,
    config_id: &ConfigurationId,
    mut batch: AnalyticsRecord,
    live_record: &mut AnalyticsRecord,
) {
    let mut previous_delay = Duration::ZERO;
    loop {
        let delay = send_record(client, config_id, &batch, previous_delay);
        if delay.is_zero() {
            return; // success
        }
        // Merge failed batch back before sleeping so data is never orphaned.
        // Mirrors Python: `self.__record.merge(batch)` before `flush_event.wait`.
        let failed = std::mem::replace(&mut batch, AnalyticsRecord::new());
        live_record.merge(failed);
        previous_delay = delay;
        std::thread::sleep(delay);
        // Re-split from live record (may now contain entries added during sleep).
        if live_record.len() == 0 {
            return; // another flush drained the record while we slept
        }
        let n = live_record.len().min(BATCH_SIZE);
        batch = live_record.split_head(n);
    }
}

/// Attempt one POST.  Returns `Duration::ZERO` on success or the retry delay.
/// Does NOT merge back on failure — the caller (`send_batch`) owns that logic.
fn send_record<C: AnalyticsClient>(
    client: &C,
    config_id: &ConfigurationId,
    batch: &AnalyticsRecord,
    previous_delay: Duration,
) -> Duration {
    let payload = batch.as_payload(&config_id.environment_id, &config_id.collection_id);
    match client.push_analytics_data(&config_id.guid, &payload) {
        Ok(()) => {
            debug!("Analytics data sent successfully");
            Duration::ZERO
        }
        Err(ref e) => {
            let status_str = e.to_string();
            if status_str.contains("429") {
                warn!("Analytics endpoint rate-limited (429), retrying in 30 s");
                RETRY_DELAY_RATE_LIMIT
            } else {
                let next = if previous_delay.is_zero() {
                    RETRY_DELAY_INITIAL
                } else {
                    (previous_delay * 2).min(RETRY_DELAY_MAX)
                };
                warn!("Analytics send failed: {e}. Retrying in {:.0} s", next.as_secs_f64());
                next
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::client::AnalyticsClient;
    use crate::analytics::errors::AnalyticsResult;
    use crate::analytics::record::AnalyticsPayload;
    use std::sync::{Arc, Mutex};

    // ── mock client ───────────────────────────────────────────────────────────

    #[derive(Clone)]
    struct MockAnalyticsClient {
        received: Arc<Mutex<Vec<AnalyticsPayload>>>,
    }

    impl MockAnalyticsClient {
        fn new() -> (Self, Arc<Mutex<Vec<AnalyticsPayload>>>) {
            let store = Arc::new(Mutex::new(Vec::new()));
            (Self { received: store.clone() }, store)
        }
    }

    impl AnalyticsClient for MockAnalyticsClient {
        fn push_analytics_data(&self, _guid: &str, payload: &AnalyticsPayload) -> AnalyticsResult<()> {
            self.received.lock().unwrap().push(payload.clone());
            Ok(())
        }
    }

    fn config_id() -> ConfigurationId {
        ConfigurationId::new("g".into(), "env".into(), "col".into())
    }

    // Helper: add n distinct evaluation entries to a sender.
    fn send_n(sender: &AnalyticsSender, n: u32) {
        for i in 0..n {
            sender.send_guarded_evaluation("rid".into(), format!("u{i}"), "target".into());
        }
    }

    // ── evaluation event reaches background thread ────────────────────────────

    /// Single evaluation event is delivered via the final-flush path (join).
    #[test]
    fn test_evaluation_event_is_flushed() {
        let (client, received) = MockAnalyticsClient::new();
        let handle = start_analytics(config_id(), client);

        handle.sender.send_guarded_evaluation("rid".into(), "u1".into(), "target".into());
        handle.join();

        let payloads = received.lock().unwrap();
        assert!(!payloads.is_empty());
        let usages = &payloads[0].usages;
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].rollout_id, "rid");
        assert_eq!(usages[0].entity_id, "u1");
        assert_eq!(usages[0].value_served, "target");
        assert_eq!(usages[0].metadata.event_type, "evaluation");
    }

    // ── metric event reaches background thread ────────────────────────────────

    /// Single metric event is delivered via the final-flush path (join).
    #[test]
    fn test_metric_event_is_flushed() {
        let (client, received) = MockAnalyticsClient::new();
        let handle = start_analytics(config_id(), client);

        handle.sender.send_guarded_metric(
            "rid".into(), "u1".into(), "target".into(),
            "purchase".into(),
            vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }],
        );
        handle.join();

        let payloads = received.lock().unwrap();
        assert!(!payloads.is_empty());
        let u = &payloads[0].usages[0];
        assert_eq!(u.metadata.event_type, "metric");
        assert_eq!(u.count, Some(1));
        assert!(u.event_key.is_none()); // stripped before send
    }

    // ── flush triggers ────────────────────────────────────────────────────────

    /// 31 entries → flush() drains all 31 immediately without waiting 5 minutes.
    /// Mirrors Python: `flush()` calls drain_chunks + flush_remainder.
    #[test]
    fn test_flush_sends_all_entries() {
        let (client, received) = MockAnalyticsClient::new();
        let handle = start_analytics(config_id(), client);

        send_n(&handle.sender, 31);
        handle.flush(); // synchronous — returns when all data is sent

        let payloads = received.lock().unwrap();
        assert!(!payloads.is_empty(), "expected at least one flush");
        let total: usize = payloads.iter().map(|p| p.usages.len()).sum();
        assert_eq!(total, 31, "all 31 entries must be delivered");
    }

    /// Exactly 30 entries exceed the limit; flush() confirms all are delivered.
    #[test]
    fn test_flushes_when_record_length_reaches_30() {
        let (client, received) = MockAnalyticsClient::new();
        let handle = start_analytics(config_id(), client);

        send_n(&handle.sender, 30);
        handle.flush(); // synchronous — blocks until all data is sent

        let payloads = received.lock().unwrap();
        let total: usize = payloads.iter().map(|p| p.usages.len()).sum();
        assert_eq!(total, 30, "all 30 entries must be delivered");
    }

    /// 60 entries → drain_chunks sends them in two batches of 30.
    /// Mirrors Python's drain loop: `while exceeds_limit(30): split_head(30); send_batch`.
    #[test]
    fn test_drain_chunks_sends_in_batches_of_30() {
        let (client, received) = MockAnalyticsClient::new();
        let handle = start_analytics(config_id(), client);

        send_n(&handle.sender, 60);
        handle.flush();

        let payloads = received.lock().unwrap();
        // Each batch is at most 30 entries.
        for p in payloads.iter() {
            assert!(p.usages.len() <= 30, "each POST must contain at most 30 entries");
        }
        let total: usize = payloads.iter().map(|p| p.usages.len()).sum();
        assert_eq!(total, 60, "all 60 entries must be delivered");
    }

    /// flush() on an empty record must send nothing.
    /// Mirrors Python: flush_remainder() returns immediately when len == 0.
    #[test]
    fn test_flush_empty_record_sends_nothing() {
        let (client, received) = MockAnalyticsClient::new();
        let handle = start_analytics(config_id(), client);

        handle.flush();  // record is empty — no POST should be made
        handle.join();   // final flush also on empty record — still no POST

        let payloads = received.lock().unwrap();
        assert!(payloads.is_empty(), "no POST should be made when record is empty; got: {payloads:?}");
    }

    // ── retry on failure ──────────────────────────────────────────────────────
    // Tests are against send_record() directly so we don't sleep through real delays.

    /// First POST fails → retry delay returned; second POST succeeds → all delivered.
    #[test]
    fn test_retries_after_failure_and_succeeds_on_second_attempt() {
        use crate::analytics::errors::{AnalyticsError, AnalyticsResult};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let call_count = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Vec::<AnalyticsPayload>::new()));

        #[derive(Clone)]
        struct FlakyClient {
            calls: Arc<AtomicUsize>,
            store: Arc<Mutex<Vec<AnalyticsPayload>>>,
        }
        impl AnalyticsClient for FlakyClient {
            fn push_analytics_data(&self, _: &str, payload: &AnalyticsPayload) -> AnalyticsResult<()> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Err(AnalyticsError::DataNotAccepted("500".into()))
                } else {
                    self.store.lock().unwrap().push(payload.clone());
                    Ok(())
                }
            }
        }

        let client = FlakyClient { calls: call_count.clone(), store: received.clone() };
        let cfg = config_id();

        let mut batch = AnalyticsRecord::new();
        for i in 0..5u32 {
            batch.add_guarded_evaluation("rid".into(), format!("u{i}"), "target".into(), chrono::Utc::now());
        }

        // First attempt: fails, returns delay.
        let delay = super::send_record(&client, &cfg, &batch, Duration::ZERO);
        assert!(!delay.is_zero(), "first attempt must fail");

        // Second attempt: succeeds.
        let delay2 = super::send_record(&client, &cfg, &batch, delay);
        assert!(delay2.is_zero(), "second attempt must succeed");

        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        let payloads = received.lock().unwrap();
        assert_eq!(payloads[0].usages.len(), 5, "all 5 entries delivered on second attempt");
    }

    /// POST failure must not lose the batch — merge_back restores all entries.
    #[test]
    fn test_failed_record_is_merged_back_before_retry() {
        use crate::analytics::errors::{AnalyticsError, AnalyticsResult};

        #[derive(Clone)]
        struct FailClient;
        impl AnalyticsClient for FailClient {
            fn push_analytics_data(&self, _: &str, _: &AnalyticsPayload) -> AnalyticsResult<()> {
                Err(AnalyticsError::DataNotAccepted("500".into()))
            }
        }

        let cfg = config_id();
        let mut live = AnalyticsRecord::new();

        let mut batch = AnalyticsRecord::new();
        for i in 0..10u32 {
            batch.add_guarded_evaluation("rid".into(), format!("u{i}"), "target".into(), chrono::Utc::now());
        }
        let original_len = batch.len();

        // send_record fails; send_batch would then merge batch back into live.
        // Test merge directly since send_batch sleeps.
        let delay = super::send_record(&FailClient, &cfg, &batch, Duration::ZERO);
        assert!(!delay.is_zero(), "failure must return nonzero delay");
        live.merge(batch);
        assert_eq!(live.len(), original_len, "all entries must be merged back");
    }

    /// Metric counts must survive a POST failure and merge-back.
    #[test]
    fn test_metric_count_preserved_across_retry() {
        use crate::analytics::errors::{AnalyticsError, AnalyticsResult};

        #[derive(Clone)]
        struct FailClient;
        impl AnalyticsClient for FailClient {
            fn push_analytics_data(&self, _: &str, _: &AnalyticsPayload) -> AnalyticsResult<()> {
                Err(AnalyticsError::DataNotAccepted("500".into()))
            }
        }

        let cfg = config_id();
        let mut live = AnalyticsRecord::new();

        let mut batch = AnalyticsRecord::new();
        for _ in 0..3 {
            batch.add_guarded_metric(
                "rid".into(), "u1".into(), "target".into(),
                "purchase".into(),
                vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }],
                chrono::Utc::now(),
            );
        }
        assert_eq!(batch.len(), 1);

        super::send_record(&FailClient, &cfg, &batch, Duration::ZERO);
        live.merge(batch);

        assert_eq!(live.len(), 1);
        let p = live.as_payload("env", "col");
        assert_eq!(p.usages[0].count, Some(3), "count must be preserved after merge-back");
    }

    /// 429 → 30 s retry delay.
    #[test]
    fn test_send_record_returns_rate_limit_delay_on_429() {
        use crate::analytics::errors::{AnalyticsError, AnalyticsResult};

        #[derive(Clone)]
        struct RateLimitClient;
        impl AnalyticsClient for RateLimitClient {
            fn push_analytics_data(&self, _: &str, _: &AnalyticsPayload) -> AnalyticsResult<()> {
                Err(AnalyticsError::DataNotAccepted("429 Too Many Requests".into()))
            }
        }

        let cfg = config_id();
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid".into(), "u1".into(), "target".into(), chrono::Utc::now());
        let delay = super::send_record(&RateLimitClient, &cfg, &r, Duration::ZERO);
        assert_eq!(delay, Duration::from_secs(30));
    }

    /// Exponential backoff: 60 s → 120 s → 240 s → 480 s → 600 s (capped).
    #[test]
    fn test_send_record_exponential_backoff_sequence() {
        use crate::analytics::errors::{AnalyticsError, AnalyticsResult};

        #[derive(Clone)]
        struct AlwaysFailClient;
        impl AnalyticsClient for AlwaysFailClient {
            fn push_analytics_data(&self, _: &str, _: &AnalyticsPayload) -> AnalyticsResult<()> {
                Err(AnalyticsError::DataNotAccepted("500".into()))
            }
        }

        let cfg = config_id();
        let make_batch = || {
            let mut r = AnalyticsRecord::new();
            r.add_guarded_evaluation("rid".into(), "u1".into(), "target".into(), chrono::Utc::now());
            r
        };

        let delays: Vec<Duration> = [
            Duration::ZERO,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(240),
            Duration::from_secs(480),
        ]
        .iter()
        .map(|&prev| super::send_record(&AlwaysFailClient, &cfg, &make_batch(), prev))
        .collect();

        assert_eq!(delays[0], Duration::from_secs(60),  "first:  60 s");
        assert_eq!(delays[1], Duration::from_secs(120), "second: 120 s");
        assert_eq!(delays[2], Duration::from_secs(240), "third:  240 s");
        assert_eq!(delays[3], Duration::from_secs(480), "fourth: 480 s");
        assert_eq!(delays[4], Duration::from_secs(600), "fifth:  600 s (capped)");
    }

    /// Doubling 480 s → 960 s must be clamped to 600 s.
    #[test]
    fn test_send_record_backoff_capped_at_600() {
        use crate::analytics::errors::{AnalyticsError, AnalyticsResult};

        #[derive(Clone)]
        struct AlwaysFailClient;
        impl AnalyticsClient for AlwaysFailClient {
            fn push_analytics_data(&self, _: &str, _: &AnalyticsPayload) -> AnalyticsResult<()> {
                Err(AnalyticsError::DataNotAccepted("500".into()))
            }
        }

        let cfg = config_id();
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid".into(), "u1".into(), "target".into(), chrono::Utc::now());
        let delay = super::send_record(&AlwaysFailClient, &cfg, &r, Duration::from_secs(480));
        assert_eq!(delay, Duration::from_secs(600));
    }
}
