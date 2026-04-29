// (C) Copyright IBM Corp. 2024.
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

use log::debug;
use log::warn;

use crate::metering::models::{
    EvaluationData, EvaluationEvent, EvaluationEventData, MeteringKey, SubjectId,
};
use crate::metering::serialization::MeteringDataJson;
use crate::metering::{MeteringClient, MeteringError};
use crate::models::{FeatureSnapshot, PropertySnapshot};
use crate::network::serialization::Segment;
use crate::utils::ThreadHandle;
use crate::{ConfigurationId, Entity};
use std::sync::mpsc;

/// Starts periodic metering transmission to the server.
///
/// # Arguments
///
/// * `config_id` - The ConfigurationID to which all evaluations are associated to when reported to the server.
/// * `transmit_interval` - Time between transmissions to the server
/// * `client` - Used for push access to the server
///
/// # Return values
///
/// * MeteringRecorder - Use this to record all evaluations, which will eventually be sent to the server.
pub(crate) fn start_metering<T: MeteringClient>(
    config_id: ConfigurationId,
    transmit_interval: std::time::Duration,
    client: T,
) -> MeteringRecorder {
    let (evaluation_sender, receiver) = mpsc::channel();

    let thread = ThreadHandle::new(move |_terminator: mpsc::Receiver<()>| {
        let mut batcher = MeteringBatcher::new(client, config_id);
        let mut last_flush = std::time::Instant::now();
        debug!("Starting Metering transmitting thread");
        loop {
            let recv_result = receiver.recv_timeout(std::time::Duration::from_millis(100));
            match recv_result {
                // Actually received an event, sort it in using the batcher:
                Ok(event) => batcher.handle_event(event),
                // Hit the timeout, do nothing here, but give the batcher a chance to flush:
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                // All senders have been dropped, exit the thread:
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last_flush.elapsed() >= transmit_interval {
                batcher.flush();
                last_flush = std::time::Instant::now();
            }
        }
    });

    MeteringRecorder {
        _thread: thread,
        sender: MeteringRecorderSender {
            evaluation_event_sender: evaluation_sender,
        },
    }
}

/// Allows recording of evaluation events.
/// Communicates with the thread, which leads to eventual transmission of recorded evaluations to the server.
#[derive(Debug)]
pub(crate) struct MeteringRecorder {
    _thread: ThreadHandle<()>,
    pub(crate) sender: MeteringRecorderSender,
}

#[derive(Debug, Clone)]
pub(crate) struct MeteringRecorderSender {
    evaluation_event_sender: mpsc::Sender<EvaluationEvent>,
}

pub(crate) trait MeteringSubject {
    fn get_metering_sender(&self) -> Option<&MeteringRecorderSender>;

    fn record_evaluation(&self, entity: &impl Entity, segment: Option<&Segment>);
}

impl MeteringSubject for PropertySnapshot {
    fn get_metering_sender(&self) -> Option<&MeteringRecorderSender> {
        self.metering.as_ref()
    }

    fn record_evaluation(&self, entity: &impl Entity, segment: Option<&Segment>) {
        if let Some(recorder) = self.get_metering_sender() {
            if let Err(e) = recorder
                .evaluation_event_sender
                .send(EvaluationEvent::Property(EvaluationEventData {
                    subject_id: SubjectId::Property(self.property_id.clone()),
                    entity_id: entity.get_id(),
                    segment_id: segment.map(|s| s.segment_id.clone()),
                }))
            {
                warn!(
                    "Fail to enqueue metering data for property '{}': {e}",
                    self.name
                );
            }
        }
    }
}

impl MeteringSubject for FeatureSnapshot {
    fn get_metering_sender(&self) -> Option<&MeteringRecorderSender> {
        self.metering.as_ref()
    }

    fn record_evaluation(&self, entity: &impl Entity, segment: Option<&Segment>) {
        if let Some(recorder) = self.get_metering_sender() {
            if let Err(e) = recorder
                .evaluation_event_sender
                .send(EvaluationEvent::Feature(EvaluationEventData {
                    subject_id: SubjectId::Feature(self.feature_id.clone()),
                    entity_id: entity.get_id(),
                    segment_id: segment.map(|s| s.segment_id.clone()),
                }))
            {
                warn!(
                    "Fail to enqueue metering data for feature '{}': {e}",
                    self.name
                );
            }
        }
    }
}

const RETRY_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(15);
const RETRY_MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(60 * 60);
const RETRY_MULTIPLIER: u32 = 2;

/// The responsibility of the MeteringBatcher is to aggregate evaluation events and batch them for transmission to the server.
struct MeteringBatcher<T: MeteringClient> {
    evaluations: std::collections::HashMap<MeteringKey, EvaluationData>,
    client: T,
    config_id: ConfigurationId,
    retry_attempt: u32,
    next_retry_at: Option<std::time::Instant>,
}

impl<T: MeteringClient> MeteringBatcher<T> {
    fn new(client: T, config_id: ConfigurationId) -> Self {
        Self {
            evaluations: std::collections::HashMap::new(),
            client,
            config_id,
            retry_attempt: 0,
            next_retry_at: None,
        }
    }

    fn calculate_retry_delay(attempt: u32) -> std::time::Duration {
        let multiplier = RETRY_MULTIPLIER.saturating_pow(attempt);
        let delay = RETRY_INITIAL_DELAY.saturating_mul(multiplier);
        std::cmp::min(delay, RETRY_MAX_DELAY)
    }

    fn is_retryable_error(error: &MeteringError) -> bool {
        match error {
            MeteringError::NetworkError(_) => true,
            MeteringError::DataNotAccepted(status) => *status == 429 || (500..=599).contains(status),
        }
    }

    fn handle_event(&mut self, event: EvaluationEvent) {
        let key = match event {
            EvaluationEvent::Feature(data) => match data.subject_id {
                SubjectId::Feature(ref id) => {
                    MeteringKey::from_feature(id.clone(), data.entity_id, data.segment_id)
                }
                _ => unreachable!(
                    "If it's a EvaluationEvent::Feature inside it contains a SubjectId::Feature"
                ),
            },
            EvaluationEvent::Property(data) => match data.subject_id {
                SubjectId::Property(ref id) => {
                    MeteringKey::from_property(id.clone(), data.entity_id, data.segment_id)
                }
                _ => unreachable!(
                    "If it's a EvaluationEvent::Property inside it contains a SubjectId::Property"
                ),
            },
        };

        self.evaluations
            .entry(key)
            .and_modify(|v| {
                v.add_one();
            })
            .or_default();
    }

    fn flush(&mut self) {
        if self.evaluations.is_empty() {
            return;
        }

        if let Some(next_retry_at) = self.next_retry_at {
            if std::time::Instant::now() < next_retry_at {
                return;
            }
        }

        let mut json_data = MeteringDataJson::new(
            self.config_id.collection_id.clone(),
            self.config_id.environment_id.clone(),
        );

        for evaluation in self.evaluations.iter() {
            json_data.add_usage(evaluation.0, evaluation.1);
        }

        debug!(
            "Sending metering data for {} usages.",
            json_data.usages.len()
        );
        let result = self
            .client
            .push_metering_data(&self.config_id.guid, &json_data);
        match result {
            Ok(()) => {
                self.evaluations.clear();
                self.retry_attempt = 0;
                self.next_retry_at = None;
            }
            Err(err) => {
                warn!("Sending metering data failed: {}", err);
                if Self::is_retryable_error(&err) {
                    let delay = Self::calculate_retry_delay(self.retry_attempt);
                    self.retry_attempt = self.retry_attempt.saturating_add(1);
                    self.next_retry_at = Some(std::time::Instant::now() + delay);
                    warn!(
                        "Retrying metering POST in {:.2} minutes (attempt #{}, cap {:.2} minutes).",
                        delay.as_secs_f64() / 60.0,
                        self.retry_attempt,
                        RETRY_MAX_DELAY.as_secs_f64() / 60.0
                    );
                } else {
                    self.evaluations.clear();
                    self.retry_attempt = 0;
                    self.next_retry_at = None;
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::metering::MeteringResult;

    struct MeteringClientMock {
        metering_data_sender: mpsc::Sender<MeteringDataJson>,
    }

    impl MeteringClientMock {
        fn new() -> (MeteringClientMock, mpsc::Receiver<MeteringDataJson>) {
            let (sender, receiver) = mpsc::channel::<MeteringDataJson>();
            (
                MeteringClientMock {
                    metering_data_sender: sender,
                },
                receiver,
            )
        }
    }

    impl MeteringClient for MeteringClientMock {
        fn push_metering_data(&self, _guid: &str, data: &MeteringDataJson) -> MeteringResult<()> {
            self.metering_data_sender.send(data.clone()).unwrap();
            Ok(())
        }
    }

    pub(crate) fn start_metering_mock(
        configuration_id: ConfigurationId,
    ) -> (MeteringRecorder, mpsc::Receiver<MeteringDataJson>) {
        let (client, receiver) = MeteringClientMock::new();
        let recorder = start_metering(
            configuration_id,
            std::time::Duration::from_millis(200), // Use 200ms for test flushing
            client,
        );
        (recorder, receiver)
    }

    /// Tests the propagation of evaluation events through the batcher to the server client and the timings of the flush.
    #[test]
    fn test_record_evaluation_leads_to_metering_data_sent() {
        let configuration_id = ConfigurationId::new(
            "test_guid".to_string(),
            "test_env_id".to_string(),
            "test_collection_id".to_string(),
        );
        let (metering_handle, metering_data_sent_receiver) = start_metering_mock(configuration_id);

        // Send a single evaluation event
        metering_handle
            .sender
            .evaluation_event_sender
            .send(EvaluationEvent::Feature(EvaluationEventData {
                subject_id: SubjectId::Feature("feature1".to_string()),
                entity_id: "entity1".to_string(),
                segment_id: None,
            }))
            .unwrap();

        let time_record_evaluation = chrono::Utc::now();
        let metering_data = metering_data_sent_receiver.recv().unwrap();
        assert!(chrono::Utc::now() - time_record_evaluation >= chrono::Duration::milliseconds(200));

        assert_eq!(
            metering_data.collection_id,
            "test_collection_id".to_string()
        );
        assert_eq!(metering_data.environment_id, "test_env_id".to_string());
        let usage = &metering_data.usages[0];
        assert_eq!(usage.feature_id, Some("feature1".to_string()));
        assert_eq!(usage.property_id, None);
        assert_eq!(usage.entity_id, "entity1".to_string());
        assert_eq!(usage.segment_id, None);
        assert_eq!(usage.count, 1);
        // Evaluation time should be close to when we called record_evaluation.
        assert!(
            usage.evaluation_time >= time_record_evaluation
                && usage.evaluation_time
                    < time_record_evaluation + chrono::Duration::milliseconds(50)
        );
    }

    /// Tests the correct sorting and batching of evaluation events.
    #[test]
    fn test_metrics_multiple_same_evaluation_events_are_batched_to_one_entry() {
        let (client, metering_data_sent_receiver) = MeteringClientMock::new();
        let mut batcher = MeteringBatcher::new(
            client,
            ConfigurationId::new(
                "test_guid".to_string(),
                "test_env_id".to_string(),
                "test_collection_id".to_string(),
            ),
        );

        // Simulate two events for the same feature/entity
        batcher.handle_event(EvaluationEvent::Feature(EvaluationEventData {
            subject_id: SubjectId::Feature("feature1".to_string()),
            entity_id: "entity1".to_string(),
            segment_id: None,
        }));
        let time_second_record = chrono::Utc::now();
        batcher.handle_event(EvaluationEvent::Feature(EvaluationEventData {
            subject_id: SubjectId::Feature("feature1".to_string()),
            entity_id: "entity1".to_string(),
            segment_id: None,
        }));
        let time_third_record = chrono::Utc::now();
        batcher.handle_event(EvaluationEvent::Property(EvaluationEventData {
            subject_id: SubjectId::Property("property1".to_string()),
            entity_id: "entity1".to_string(),
            segment_id: Some("some_segment".to_string()),
        }));

        // Force flush
        batcher.flush();

        let metering_data = metering_data_sent_receiver.recv().unwrap();

        // The two feature evaluations should be batched into one entry:
        let feature_usage = metering_data
            .usages
            .iter()
            .find(|u| u.feature_id == Some("feature1".to_string()))
            .unwrap();
        assert_eq!(feature_usage.property_id, None);
        assert_eq!(feature_usage.entity_id, "entity1".to_string());
        assert_eq!(feature_usage.segment_id, None);
        assert!(feature_usage.evaluation_time >= time_second_record);
        assert_eq!(feature_usage.count, 2);

        // The property evaluation should be a separate entry:
        let property_usage = metering_data
            .usages
            .iter()
            .find(|u| u.property_id == Some("property1".to_string()))
            .unwrap();
        assert_eq!(property_usage.feature_id, None);
        assert_eq!(property_usage.entity_id, "entity1".to_string());
        assert_eq!(property_usage.segment_id, Some("some_segment".to_string()));
        assert!(property_usage.evaluation_time >= time_third_record);
        assert_eq!(property_usage.count, 1);
    }
}
