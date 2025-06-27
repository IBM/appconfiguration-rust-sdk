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
    _config_id: ConfigurationId,
    transmit_interval: std::time::Duration,
    server_client: T,
) -> (MeteringThreadHandle, MeteringRecorder) {
    let (sender, receiver) = mpsc::channel();

    let thread = ThreadHandle::new(move |_terminator: mpsc::Receiver<()>| {
        let mut batcher = MeteringBatcher::new(transmit_interval, server_client);
        loop {
            let recv_result = receiver.recv_timeout(std::time::Duration::from_millis(100));
            match recv_result {
                // Actually received an event, sort it in using the batcher:
                Ok(event) => batcher.handle_event(event),
                // Hit the timeout, do nothing here, but give the batcher a chance to flush:
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
                // All senders have been dropped, exit the thread:
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            batcher.maybe_flush();
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

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct MeteringKey {
    feature_id: Option<String>,
    property_id: Option<String>,
    entity_id: String,
    segment_id: Option<String>,
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

struct MeteringBatcher<T: ServerClient> {
    counters: std::collections::HashMap<MeteringKey, u32>,
    last_flush: std::time::Instant,
    transmit_interval: std::time::Duration,
    server_client: T,
}

impl<T: ServerClient> MeteringBatcher<T> {
    fn new(transmit_interval: std::time::Duration, server_client: T) -> Self {
        Self {
            counters: std::collections::HashMap::new(),
            last_flush: std::time::Instant::now(),
            transmit_interval,
            server_client,
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
        let counter = self.counters.entry(key).or_insert(0);
        *counter += 1;
    }

    fn maybe_flush(&mut self) {
        if self.last_flush.elapsed() >= self.transmit_interval && !self.counters.is_empty() {
            use chrono::Utc;
            for (key, count) in self.counters.iter() {
                let json_data = crate::models::MeteringDataJson {
                    feature_id: key.feature_id.clone(),
                    property_id: key.property_id.clone(),
                    entity_id: key.entity_id.clone(),
                    segment_id: key.segment_id.clone(),
                    evaluation_time: Utc::now(),
                    count: *count,
                };
                let _ = self.server_client.push_metering_data(&json_data);
            }
            self.counters.clear();
            self.last_flush = std::time::Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConfigurationJson;
    use crate::models::MeteringDataJson;
    use crate::network::http_client::WebsocketReader;
    use crate::NetworkResult;
    use chrono;

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
            unreachable!() as crate::NetworkResult<WebsocketMockReader>
        }

        fn push_metering_data(&self, data: &MeteringDataJson) -> NetworkResult<()> {
            self.metering_data_sender.send(data.clone()).unwrap();
            Ok(())
        }
    }

    #[test]
    fn test_metrics_sent_feature() {
        let (server_client, metering_data_sent_receiver) = ServerClientMock::new();
        let (_, metering_handle) = start_metering(
            ConfigurationId::new("".to_string(), "".to_string(), "".to_string()),
            std::time::Duration::from_millis(200), // Use 200ms for test batching
            server_client,
        );

        metering_handle
            .record_evaluation(
                SubjectId::Feature("feature1".to_string()),
                "entity1".to_string(),
                None,
            )
            .unwrap();
        let time_second_record = chrono::Utc::now();
        metering_handle
            .record_evaluation(
                SubjectId::Feature("feature1".to_string()),
                "entity1".to_string(),
                None,
            )
            .unwrap();

        let metering_data = metering_data_sent_receiver.recv().unwrap();
        let time_data_sent_to_server = chrono::Utc::now();

        assert_eq!(metering_data.feature_id, Some("feature1".to_string()));
        assert_eq!(metering_data.property_id, None);
        // TODO (for models.rs tests): property_id should not be set in json serialization output.
        assert_eq!(metering_data.entity_id, "entity1".to_string());
        assert_eq!(metering_data.segment_id, None);
        // TODO (for models.rs tests): segment id should be set in json serialization output, but value should be "nil"

        // evaluation_time should be after first record_evaluation:
        assert!(
            metering_data.evaluation_time >= time_second_record
                && metering_data.evaluation_time <= time_data_sent_to_server
        );

        // we expect both evaluations to be batched into one transmission to the server, accumulating the count:
        assert_eq!(metering_data.count, 2);
    }
}
