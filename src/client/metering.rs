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
    _transmit_interval: std::time::Duration,
    server_client: T,
) -> (MeteringThreadHandle, MeteringRecorder) {
    let (sender, receiver) = mpsc::channel();

    let thread = ThreadHandle::new(move |_terminator: mpsc::Receiver<()>| {
        // TODO: termination handling
        loop {
            // TODO: error handling
            let _ = receiver.recv().unwrap();
            // TODO: actually process the event
            let json_data = crate::models::MeteringDataJson {};
            server_client.push_metering_data(&json_data);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConfigurationJson;
    use crate::models::MeteringDataJson;
    use crate::network::http_client::WebsocketReader;
    use crate::NetworkResult;

    struct ServerClientMock {
        metering_data_sender: mpsc::Sender<()>,
    }
    struct WebsocketMockReader {}
    impl WebsocketReader for WebsocketMockReader {
        fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
            unreachable!()
        }
    }

    impl ServerClientMock {
        fn new() -> (ServerClientMock, mpsc::Receiver<()>) {
            let (sender, receiver) = mpsc::channel();
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

        fn push_metering_data(&self, _data: &MeteringDataJson) -> NetworkResult<()> {
            self.metering_data_sender.send(()).unwrap();
            Ok(())
        }
    }

    #[test]
    fn test_metrics_sent_feature() {
        let (server_client, metering_data_sent_receiver) = ServerClientMock::new();
        let (_, metering_handle) = start_metering(
            ConfigurationId::new("".to_string(), "".to_string(), "".to_string()),
            std::time::Duration::ZERO,
            server_client,
        );

        metering_handle
            .record_evaluation(SubjectId::Feature("".to_string()), "".to_string(), None)
            .unwrap();

        let _ = metering_data_sent_receiver.recv().unwrap();
    }
}
