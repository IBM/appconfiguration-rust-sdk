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
use crate::network::ServerClient;
use crate::utils::ThreadHandle;
use crate::ConfigurationId;
use std::sync::mpsc;

/// Starts periodic metering transmission to the server.
///
/// # Arguments
///
/// * `config_id` - The ConfigurationID to which all evaluations are associated to when reported to the server.
/// * `transmit_interval` - Time between transmissions to the server
/// * `server_client` - Used for push access to the server
///
/// # Return values
///
/// * MeteringThreadHandle<T> - Object representing the thread. Metrics will be sent as long as this object is alive.
/// * MeteringRecorder - Use this to record all evaluations, which will eventually be sent to the server.
pub(crate) fn start_metering<T: ServerClient>(
    config_id: ConfigurationId,
    transmit_interval: std::time::Duration,
    server_client: T,
) -> (MeteringThreadHandle, MeteringRecorder) {
    let (sender, receiver) = mpsc::channel();

    let thread = ThreadHandle::new(move |_terminator: mpsc::Receiver<()>| {
        let mut batcher = MeteringBatcher::new(server_client, config_id);
        let mut last_flush = std::time::Instant::now();
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

    (
        MeteringThreadHandle {
            _thread_handle: thread,
        },
        MeteringRecorder {
            evaluation_event_sender: sender,
        },
    )
}

/// Allows recording of evaluation events.
/// Communicates with the MeteringThreadHandle, which leads to eventual transmission of recorded evaluations to the server.
pub(crate) struct MeteringRecorder {
    evaluation_event_sender: mpsc::Sender<EvaluationEvent>,
}

impl MeteringRecorder {
    /// Record the evaluation of a feature or property, for eventual transmission to the server.
    pub fn record_evaluation(
        &self,
        subject_id: SubjectId,
        entity_id: String,
        segment_id: Option<String>,
    ) -> crate::errors::Result<()> {
        self.evaluation_event_sender
            .send(EvaluationEvent::Feature(EvaluationEventData {
                subject_id: subject_id,
                entity_id: entity_id,
                segment_id: segment_id,
            }))
            .map_err(|_| crate::errors::Error::MeteringError {})
    }
}

pub(crate) struct MeteringThreadHandle {
    _thread_handle: crate::utils::ThreadHandle<()>,
}

pub(crate) enum SubjectId {
    Feature(String),
    Property(String),
}

pub(crate) struct EvaluationEventData {
    /// ID if the subject being evaluated. E.g. feature ID.
    pub subject_id: SubjectId,
    /// The ID of the Entity against which the subject was evaluated.
    pub entity_id: String,
    /// If applicable, the segment the subject was associated to during evaluation.
    pub segment_id: Option<String>,
}

pub(crate) enum EvaluationEvent {
    Feature(EvaluationEventData),
    Property(EvaluationEventData),
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct MeteringKey {
    feature_id: Option<String>,
    property_id: Option<String>,
    entity_id: String,
    segment_id: Option<String>,
}

struct EvaluationData {
    number_of_evaluations: u32,
    time_of_last_evaluation: chrono::DateTime<chrono::Utc>,
}

/// The responsibility of the MeteringBatcher is to aggregate evaluation events and batch them for transmission to the server.
struct MeteringBatcher<T: ServerClient> {
    evaluations: std::collections::HashMap<MeteringKey, EvaluationData>,
    server_client: T,
    config_id: ConfigurationId,
}

impl<T: ServerClient> MeteringBatcher<T> {
    fn new(server_client: T, config_id: ConfigurationId) -> Self {
        Self {
            evaluations: std::collections::HashMap::new(),
            server_client,
            config_id,
        }
    }

    fn handle_event(&mut self, event: EvaluationEvent) {
        let (feature_id, property_id, entity_id, segment_id) = match event {
            EvaluationEvent::Feature(data) => (
                match data.subject_id {
                    SubjectId::Feature(ref id) => Some(id.clone()),
                    _ => None,
                },
                None,
                data.entity_id,
                data.segment_id,
            ),
            EvaluationEvent::Property(data) => (
                None,
                match data.subject_id {
                    SubjectId::Property(ref id) => Some(id.clone()),
                    _ => None,
                },
                data.entity_id,
                data.segment_id,
            ),
        };
        let key = MeteringKey {
            feature_id: feature_id.clone(),
            property_id: property_id.clone(),
            entity_id: entity_id.clone(),
            segment_id: segment_id.clone(),
        };
        let now = chrono::Utc::now();
        self.evaluations
            .entry(key)
            .and_modify(|v| {
                v.number_of_evaluations += 1;
                v.time_of_last_evaluation = now;
            })
            .or_insert(EvaluationData {
                number_of_evaluations: 1,
                time_of_last_evaluation: now,
            });
    }

    fn flush(&mut self) {
        if self.evaluations.is_empty() {
            return;
        }
        let usages: Vec<crate::network::models::MeteringDataUsageJson> = self
            .evaluations
            .iter()
            .map(
                |(key, value)| crate::network::models::MeteringDataUsageJson {
                    feature_id: key.feature_id.clone(),
                    property_id: key.property_id.clone(),
                    entity_id: key.entity_id.clone(),
                    segment_id: key.segment_id.clone(),
                    evaluation_time: value.time_of_last_evaluation,
                    count: value.number_of_evaluations,
                },
            )
            .collect();

        let json_data = crate::network::models::MeteringDataJson {
            collection_id: self.config_id.collection_id.to_string(),
            environment_id: self.config_id.environment_id.to_string(),
            usages,
        };
        let _ = self.server_client.push_metering_data(&json_data);
        self.evaluations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::http_client::WebsocketReader;
    use crate::network::models::ConfigurationJson;
    use crate::network::models::MeteringDataJson;
    use crate::network::NetworkResult;

    struct ServerClientMock {
        metering_data_sender: mpsc::Sender<MeteringDataJson>,
    }
    struct WebsocketMockReader {}
    impl WebsocketReader for WebsocketMockReader {
        fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
            unreachable!()
        }
    }

    impl ServerClientMock {
        fn new() -> (ServerClientMock, mpsc::Receiver<MeteringDataJson>) {
            let (sender, receiver) = mpsc::channel::<MeteringDataJson>();
            (
                ServerClientMock {
                    metering_data_sender: sender,
                },
                receiver,
            )
        }
    }

    impl ServerClient for ServerClientMock {
        #[allow(unreachable_code)]
        fn get_configuration(
            &self,
            _configuration_id: &ConfigurationId,
        ) -> NetworkResult<ConfigurationJson> {
            unreachable!()
        }

        #[allow(unreachable_code)]
        fn get_configuration_monitoring_websocket(
            &self,
            _collection: &ConfigurationId,
        ) -> NetworkResult<impl WebsocketReader> {
            unreachable!() as NetworkResult<WebsocketMockReader>
        }

        fn push_metering_data(&self, data: &MeteringDataJson) -> NetworkResult<()> {
            self.metering_data_sender.send(data.clone()).unwrap();
            Ok(())
        }
    }

    /// Tests the propagation of evaluation events through the batcher to the server client and the timings of the flush.
    #[test]
    fn test_record_evaluation_leads_to_metering_data_sent() {
        let (server_client, metering_data_sent_receiver) = ServerClientMock::new();
        let (_, metering_handle) = start_metering(
            ConfigurationId::new(
                "test_guid".to_string(),
                "test_env_id".to_string(),
                "test_collection_id".to_string(),
            ),
            std::time::Duration::from_millis(200), // Use 200ms for test flushing
            server_client,
        );

        // Send a single evaluation event
        metering_handle
            .record_evaluation(
                SubjectId::Feature("feature1".to_string()),
                "entity1".to_string(),
                None,
            )
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
        let (server_client, metering_data_sent_receiver) = ServerClientMock::new();
        let mut batcher = MeteringBatcher::new(
            server_client,
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
