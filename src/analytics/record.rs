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

//! In-memory record of analytics events, mirroring Python's `AnalyticsRecord`.
//!
//! ## Rules (matching Python behaviour exactly)
//!
//! * **GUARDED_EVALUATION** — deduplicated by `(rollout_id, entity_id, value_served)`.
//!   No count field; if the same key arrives again it is silently ignored.
//! * **GUARDED_METRIC** — deduplicated by `(rollout_id, entity_id, value_served, event_key)`.
//!   Has a `count` that increments on each duplicate; `timestamp` is max of all arrivals.
//! * `event_key` is used only for dedup keying and is **stripped** before the HTTP body is built.
//! * `merge()` is used to fold a failed-send record back into the live record for retry.
//! * `split_head(n)` removes and returns the first `n` entries as a new record, mirroring
//!   Python's `AnalyticsRecord.split_head(n)` used by the drain loop.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

// Unit-separator used as composite-key delimiter — identical to all other SDKs.
const DELIMITER: char = '\u{001F}';

// ─── Payload types (what actually goes in the HTTP body) ───────────────────────

/// Metric descriptor attached to a feature by the server config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub(crate) struct MetricDescriptor {
    pub id: String,
    #[serde(rename = "type")]
    pub metric_type: String,
}

/// Metadata block present on every analytics usage entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AnalyticsMetadata {
    pub feature: &'static str,    // always "guarded" for our events
    pub event_type: &'static str, // "evaluation" | "metric"
}

/// A single entry in the `usages` array that is POSTed to the server.
/// Fields that are absent for a given event type are skipped in serialization.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AnalyticsUsage {
    pub rollout_id: String,
    pub entity_id: String,
    pub value_served: String,
    pub metadata: AnalyticsMetadata,
    pub timestamp: DateTime<Utc>,

    /// Present only on GUARDED_METRIC entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,

    /// Present only on GUARDED_METRIC entries.
    /// NOTE: `event_key` is stored here for dedup but is stripped before sending.
    #[serde(skip)]
    pub event_key: Option<String>,

    /// Present only on GUARDED_METRIC entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Vec<MetricDescriptor>>,
}

// ─── AnalyticsRecord ───────────────────────────────────────────────────────────

/// Thread-local in-memory store for analytics events.
///
/// Mirrors Python's `AnalyticsRecord`.  NOT thread-safe on its own — the
/// background thread owns one exclusively (same pattern as Python).
#[derive(Debug, Default)]
pub(crate) struct AnalyticsRecord {
    /// Ordered list of usage entries, in arrival order.
    usages: Vec<AnalyticsUsage>,
    /// Maps a composite key → index into `usages`, for O(1) dedup lookup.
    index: HashMap<String, usize>,
}

impl AnalyticsRecord {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of distinct entries currently held.
    pub(crate) fn len(&self) -> usize {
        self.usages.len()
    }

    /// Returns `true` when the record holds `>= limit` entries.
    /// Mirrors Python's `exceeds_limit(batch_size)`.
    pub(crate) fn exceeds_limit(&self, limit: usize) -> bool {
        self.usages.len() >= limit
    }

    // ── key builders ──────────────────────────────────────────────────────────

    fn evaluation_key(rollout_id: &str, entity_id: &str, value_served: &str) -> String {
        [rollout_id, entity_id, value_served].join(&DELIMITER.to_string())
    }

    fn metric_key(
        rollout_id: &str,
        entity_id: &str,
        value_served: &str,
        event_key: &str,
    ) -> String {
        [rollout_id, entity_id, value_served, event_key].join(&DELIMITER.to_string())
    }

    // ── public add methods ────────────────────────────────────────────────────

    /// Record a guarded evaluation (one per `get_current_value` for a GUARDED feature).
    /// Duplicate keys are silently ignored — same as Python.
    pub(crate) fn add_guarded_evaluation(
        &mut self,
        rollout_id: String,
        entity_id: String,
        value_served: String,
        timestamp: DateTime<Utc>,
    ) {
        let key = Self::evaluation_key(&rollout_id, &entity_id, &value_served);
        if self.index.contains_key(&key) {
            return; // already present, no update
        }
        let idx = self.usages.len();
        self.usages.push(AnalyticsUsage {
            rollout_id,
            entity_id,
            value_served,
            metadata: AnalyticsMetadata {
                feature: "guarded",
                event_type: "evaluation",
            },
            timestamp,
            count: None,
            event_key: None,
            metrics: None,
        });
        self.index.insert(key, idx);
    }

    /// Record a guarded metric (one per `feature.track(event_key, entity_id)` call).
    /// Duplicates increment the count and update the timestamp to the latest arrival.
    pub(crate) fn add_guarded_metric(
        &mut self,
        rollout_id: String,
        entity_id: String,
        value_served: String,
        event_key: String,
        metrics: Vec<MetricDescriptor>,
        timestamp: DateTime<Utc>,
    ) {
        let key = Self::metric_key(&rollout_id, &entity_id, &value_served, &event_key);
        if let Some(&idx) = self.index.get(&key) {
            // Aggregate: bump count and update timestamp.
            let entry = &mut self.usages[idx];
            *entry.count.get_or_insert(0) += 1;
            entry.timestamp = entry.timestamp.max(timestamp);
            return;
        }
        let idx = self.usages.len();
        self.usages.push(AnalyticsUsage {
            rollout_id,
            entity_id,
            value_served,
            metadata: AnalyticsMetadata {
                feature: "guarded",
                event_type: "metric",
            },
            timestamp,
            count: Some(1),
            event_key: Some(event_key),
            metrics: Some(metrics),
        });
        self.index.insert(key, idx);
    }

    // ── split_head (drain loop support) ──────────────────────────────────────

    /// Remove and return the first `n` entries as a new `AnalyticsRecord`.
    /// Insertion order is preserved — oldest entries are returned first.
    ///
    /// Mirrors Python's `AnalyticsRecord.split_head(n)` used by `__drain_chunks`.
    pub(crate) fn split_head(&mut self, n: usize) -> AnalyticsRecord {
        let n = n.min(self.usages.len());
        let head_usages: Vec<AnalyticsUsage> = self.usages.drain(..n).collect();

        // Rebuild the index for the remaining entries (indices shifted by n).
        self.index.clear();
        for (i, usage) in self.usages.iter().enumerate() {
            let key = match &usage.event_key {
                None => Self::evaluation_key(&usage.rollout_id, &usage.entity_id, &usage.value_served),
                Some(ek) => Self::metric_key(&usage.rollout_id, &usage.entity_id, &usage.value_served, ek),
            };
            self.index.insert(key, i);
        }

        // Build the head record with its own index.
        let mut head = AnalyticsRecord::new();
        for (i, usage) in head_usages.iter().enumerate() {
            let key = match &usage.event_key {
                None => Self::evaluation_key(&usage.rollout_id, &usage.entity_id, &usage.value_served),
                Some(ek) => Self::metric_key(&usage.rollout_id, &usage.entity_id, &usage.value_served, ek),
            };
            head.index.insert(key, i);
        }
        head.usages = head_usages;
        head
    }

    // ── merge (retry support) ─────────────────────────────────────────────────

    /// Fold `other` back into `self`.  Used when a send fails: the split-off
    /// batch is merged back so nothing is lost before the next attempt.
    /// Mirrors Python's `AnalyticsRecord.merge(other)`.
    pub(crate) fn merge(&mut self, other: AnalyticsRecord) {
        for usage in other.usages {
            match usage.event_key.clone() {
                None => {
                    // GUARDED_EVALUATION — deduplicated only
                    self.add_guarded_evaluation(
                        usage.rollout_id,
                        usage.entity_id,
                        usage.value_served,
                        usage.timestamp,
                    );
                }
                Some(event_key) => {
                    // GUARDED_METRIC — aggregate counts
                    let key = Self::metric_key(
                        &usage.rollout_id,
                        &usage.entity_id,
                        &usage.value_served,
                        &event_key,
                    );
                    if let Some(&idx) = self.index.get(&key) {
                        let entry = &mut self.usages[idx];
                        let other_count = usage.count.unwrap_or(1);
                        *entry.count.get_or_insert(0) += other_count;
                        entry.timestamp = entry.timestamp.max(usage.timestamp);
                    } else {
                        let idx = self.usages.len();
                        self.usages.push(AnalyticsUsage {
                            rollout_id: usage.rollout_id,
                            entity_id: usage.entity_id,
                            value_served: usage.value_served,
                            metadata: usage.metadata,
                            timestamp: usage.timestamp,
                            count: usage.count,
                            event_key: Some(event_key.clone()),
                            metrics: usage.metrics,
                        });
                        self.index.insert(key, idx);
                    }
                }
            }
        }
    }

    // ── serialization ─────────────────────────────────────────────────────────

    /// Produce the JSON-ready payload body, stripping `event_key` before sending.
    /// Equivalent to Python's `get_request_body()`.
    pub(crate) fn as_payload(&self, environment_id: &str, collection_id: &str) -> AnalyticsPayload {
        let usages = self
            .usages
            .iter()
            .map(|u| {
                let mut cloned = u.clone();
                cloned.event_key = None; // strip before send
                cloned
            })
            .collect();

        AnalyticsPayload {
            environment_id: environment_id.to_string(),
            collection_id: collection_id.to_string(),
            usages,
        }
    }
}

/// The top-level JSON body POSTed to `/metrics/v1/instances/{guid}/analytics`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AnalyticsPayload {
    pub environment_id: String,
    pub collection_id: String,
    pub usages: Vec<AnalyticsUsage>,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        Utc::now()
    }

    fn m1() -> Vec<MetricDescriptor> {
        vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]
    }

    // ════════════════════════════════════════════════════════════════════════
    // GUARDED_EVALUATION — mirrors test_analytics_record.py
    //   TestAnalyticsRecordGuardedEvaluation
    // ════════════════════════════════════════════════════════════════════════

    /// test_add_new_evaluation_entry
    #[test]
    fn test_add_new_evaluation_entry() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        assert_eq!(r.len(), 1);
    }

    /// test_deduplicates_identical_evaluation_entries
    #[test]
    fn test_deduplicates_identical_evaluation_entries() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        assert_eq!(r.len(), 1);
    }

    /// test_target_and_original_are_distinct_entries
    #[test]
    fn test_target_and_original_are_distinct_entries() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u1".into(), "target".into(), ts());
        r.add_guarded_evaluation("rid-1".into(), "u1".into(), "original".into(), ts());
        assert_eq!(r.len(), 2);
    }

    /// test_different_entity_ids_are_distinct
    #[test]
    fn test_different_entity_ids_are_distinct() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        r.add_guarded_evaluation("rid-1".into(), "u19".into(), "target".into(), ts());
        assert_eq!(r.len(), 2);
    }

    /// test_stamps_correct_metadata_on_evaluation
    #[test]
    fn test_stamps_correct_metadata_on_evaluation() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert_eq!(p.usages[0].metadata.feature, "guarded");
        assert_eq!(p.usages[0].metadata.event_type, "evaluation");
    }

    /// test_evaluation_entry_has_no_count_field
    #[test]
    fn test_evaluation_entry_has_no_count_field() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert!(p.usages[0].count.is_none());
    }

    // ════════════════════════════════════════════════════════════════════════
    // GUARDED_METRIC — mirrors TestAnalyticsRecordGuardedMetric
    // ════════════════════════════════════════════════════════════════════════

    /// test_add_new_metric_entry_with_count_one
    #[test]
    fn test_add_new_metric_entry_with_count_one() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert_eq!(p.usages[0].count, Some(1));
    }

    /// test_aggregates_count_on_duplicate_metrics
    #[test]
    fn test_aggregates_count_on_duplicate_metrics() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert_eq!(p.usages[0].count, Some(2));
        assert_eq!(p.usages.len(), 1);
    }

    /// test_different_event_keys_are_distinct
    #[test]
    fn test_different_event_keys_are_distinct() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), vec![], ts());
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "purchase".into(), vec![], ts());
        assert_eq!(r.len(), 2);
    }

    /// test_stamps_correct_metadata_on_metric
    #[test]
    fn test_stamps_correct_metadata_on_metric() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert_eq!(p.usages[0].metadata.feature, "guarded");
        assert_eq!(p.usages[0].metadata.event_type, "metric");
    }

    // ════════════════════════════════════════════════════════════════════════
    // split_head — mirrors Python split_head behaviour
    // ════════════════════════════════════════════════════════════════════════

    /// split_head(n) returns first n entries as a new record.
    #[test]
    fn test_split_head_returns_first_n_entries() {
        let mut r = AnalyticsRecord::new();
        for i in 0..5u32 {
            r.add_guarded_evaluation("rid".into(), format!("u{i}"), "target".into(), ts());
        }
        let head = r.split_head(3);
        assert_eq!(head.len(), 3, "head must contain exactly 3 entries");
        assert_eq!(r.len(), 2, "remainder must contain exactly 2 entries");
        // head has oldest 3: u0, u1, u2
        let hp = head.as_payload("e", "c");
        assert_eq!(hp.usages[0].entity_id, "u0");
        assert_eq!(hp.usages[1].entity_id, "u1");
        assert_eq!(hp.usages[2].entity_id, "u2");
    }

    /// split_head preserves dedup index — merging head back keeps correct counts.
    #[test]
    fn test_split_head_index_rebuilt_for_remainder() {
        let mut r = AnalyticsRecord::new();
        for i in 0..4u32 {
            r.add_guarded_evaluation("rid".into(), format!("u{i}"), "target".into(), ts());
        }
        let _ = r.split_head(2); // discard head
        // Adding a duplicate of a remaining entry must still deduplicate.
        r.add_guarded_evaluation("rid".into(), "u2".into(), "target".into(), ts());
        assert_eq!(r.len(), 2, "duplicate add after split must not grow remainder");
    }

    /// split_head(n) where n >= len returns all entries.
    #[test]
    fn test_split_head_clamps_to_len() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid".into(), "u1".into(), "target".into(), ts());
        let head = r.split_head(100);
        assert_eq!(head.len(), 1);
        assert_eq!(r.len(), 0);
    }

    // ════════════════════════════════════════════════════════════════════════
    // exceeds_limit
    // ════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_exceeds_limit_false_below() {
        let mut r = AnalyticsRecord::new();
        for i in 0..29u32 {
            r.add_guarded_evaluation("rid".into(), format!("u{i}"), "target".into(), ts());
        }
        assert!(!r.exceeds_limit(30), "29 entries must not exceed limit of 30");
    }

    #[test]
    fn test_exceeds_limit_true_at_boundary() {
        let mut r = AnalyticsRecord::new();
        for i in 0..30u32 {
            r.add_guarded_evaluation("rid".into(), format!("u{i}"), "target".into(), ts());
        }
        assert!(r.exceeds_limit(30), "exactly 30 entries must exceed limit of 30");
    }

    // ════════════════════════════════════════════════════════════════════════
    // get_request_body / as_payload — mirrors TestAnalyticsRecordGetRequestBody
    // ════════════════════════════════════════════════════════════════════════

    /// test_returns_correct_envelope
    #[test]
    fn test_returns_correct_envelope() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        let p = r.as_payload("my-env", "my-col");
        assert_eq!(p.environment_id, "my-env");
        assert_eq!(p.collection_id, "my-col");
        assert!(!p.usages.is_empty());
    }

    /// test_strips_event_key_from_usages_in_body
    #[test]
    fn test_strips_event_key_from_usages_in_body() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert!(p.usages[0].event_key.is_none());
    }

    /// test_does_not_mutate_internal_state_when_stripping_event_key
    /// Calling as_payload() must not corrupt the internal dedup index — a
    /// second add of the same metric must still increment the count.
    #[test]
    fn test_does_not_mutate_internal_state_when_stripping_event_key() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        r.as_payload("env-1", "col-1"); // observe once — should not mutate
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        let p = r.as_payload("env-1", "col-1");
        assert_eq!(p.usages[0].count, Some(2));
    }

    // ════════════════════════════════════════════════════════════════════════
    // get_record_length / len — mirrors TestAnalyticsRecordGetRecordLength
    // ════════════════════════════════════════════════════════════════════════

    /// test_returns_zero_on_new_record
    #[test]
    fn test_returns_zero_on_new_record() {
        assert_eq!(AnalyticsRecord::new().len(), 0);
    }

    /// test_returns_correct_count_after_distinct_adds
    #[test]
    fn test_returns_correct_count_after_distinct_adds() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        r.add_guarded_evaluation("rid-1".into(), "u1".into(), "original".into(), ts());
        r.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        assert_eq!(r.len(), 3);
    }

    // ════════════════════════════════════════════════════════════════════════
    // merge — mirrors TestAnalyticsRecordMerge
    // ════════════════════════════════════════════════════════════════════════

    /// test_merges_entries_from_another_record
    #[test]
    fn test_merges_entries_from_another_record() {
        let mut a = AnalyticsRecord::new();
        a.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        let mut b = AnalyticsRecord::new();
        b.add_guarded_evaluation("rid-1".into(), "u19".into(), "target".into(), ts());
        a.merge(b);
        assert_eq!(a.len(), 2);
    }

    /// test_aggregates_counts_for_overlapping_metrics_during_merge
    #[test]
    fn test_aggregates_counts_for_overlapping_metrics_during_merge() {
        let mut a = AnalyticsRecord::new();
        a.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());

        let mut b = AnalyticsRecord::new();
        // Add the same metric twice to b — b.count == 2 after both adds.
        b.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());
        b.add_guarded_metric("rid-1".into(), "u17".into(), "target".into(), "click".into(), m1(), ts());

        a.merge(b);
        assert_eq!(a.len(), 1);
        let p = a.as_payload("e", "c");
        assert_eq!(p.usages[0].count, Some(3));
    }

    /// test_does_not_add_duplicate_evaluation_entries_during_merge
    #[test]
    fn test_does_not_add_duplicate_evaluation_entries_during_merge() {
        let mut a = AnalyticsRecord::new();
        a.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        let mut b = AnalyticsRecord::new();
        b.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        a.merge(b);
        assert_eq!(a.len(), 1);
    }

    /// test_ignores_non_analytics_record_values_silently — in Rust this maps
    /// to merging an empty record (the type system prevents passing non-records).
    #[test]
    fn test_merging_empty_record_is_a_noop() {
        let mut a = AnalyticsRecord::new();
        a.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        a.merge(AnalyticsRecord::new()); // empty — should be silent noop
        assert_eq!(a.len(), 1);
    }

    // ════════════════════════════════════════════════════════════════════════
    // Serialization sanity — JSON shape matches what the server expects
    // ════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_evaluation_serializes_without_count_or_metrics() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_evaluation("rid-1".into(), "u17".into(), "target".into(), ts());
        let p = r.as_payload("env", "col");
        let json = serde_json::to_value(&p).unwrap();
        let usage = &json["usages"][0];
        assert!(usage["count"].is_null(), "count must be absent");
        assert!(usage["metrics"].is_null(), "metrics must be absent");
        assert!(usage["event_key"].is_null(), "event_key must be absent");
        assert_eq!(usage["metadata"]["feature"], "guarded");
        assert_eq!(usage["metadata"]["event_type"], "evaluation");
    }

    #[test]
    fn test_metric_serializes_with_count_and_metrics() {
        let mut r = AnalyticsRecord::new();
        r.add_guarded_metric(
            "rid-1".into(), "u17".into(), "target".into(),
            "purchase".into(),
            vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }],
            ts(),
        );
        let p = r.as_payload("env", "col");
        let json = serde_json::to_value(&p).unwrap();
        let usage = &json["usages"][0];
        assert_eq!(usage["count"], 1);
        assert!(usage["event_key"].is_null(), "event_key must be stripped");
        assert_eq!(usage["metrics"][0]["id"], "m1");
        assert_eq!(usage["metrics"][0]["type"], "count");
        assert_eq!(usage["metadata"]["event_type"], "metric");
    }
}
