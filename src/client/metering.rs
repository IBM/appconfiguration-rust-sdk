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
use std::sync::mpsc;
use crate::network::ServerClient;
use crate::ConfigurationId;

pub fn start_metering<T: ServerClient>(config_id: ConfigurationId, transmit_interval: std::time::Duration, server_client: T) -> (MeteringTask<T>, MeteringHandle){
    
    let (sender, receiver) = mpsc::channel();

    (MeteringTask{config_id: config_id, server_client: server_client}, MeteringHandle{evaluation_event_sender: sender})
}

pub struct MeteringTask<T: ServerClient>{
    config_id: ConfigurationId,
    server_client: T
}

pub(crate) struct EvaluationEventData{
    /// ID if the subject being evaluated. E.g. feature ID.
    pub subject_id: String,
    // The ID of the Entity against which the subject was evaluated.
    pub entity_id: String,
    // If applicable, the segment the subject was associated to during evaluation.
    pub segment_id: Option<String>
}

pub(crate) enum EvaluationEvent{
    Feature(EvaluationEventData),
    Property(EvaluationEventData)
}

pub struct MeteringHandle{
    evaluation_event_sender: mpsc::Sender<EvaluationEvent>
}

impl MeteringHandle{
    pub fn record_evaluation(&self, feature_id: String, entity_id: String, property_id: Option<String>){
        todo!()
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::NetworkResult;
    use crate::models::ConfigurationJson;
    use crate::models::MeteringDataJson;
    use crate::network::http_client::WebsocketReader;

    struct ServerClientMock{
        metering_data_sender: mpsc::Sender<()>
    }
    struct WebsocketMockReader {
    }
    impl WebsocketReader for WebsocketMockReader {
        fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
            unreachable!()
        }
    }

    impl ServerClientMock{
        fn new() -> (ServerClientMock, mpsc::Receiver<()>){
            let (sender, receiver) = mpsc::channel();
            (ServerClientMock{metering_data_sender: sender}, receiver)
        }
    }

    impl ServerClient for ServerClientMock{

        #[allow(unreachable_code)]
        fn get_configuration(
            &self,
            configuration_id: &ConfigurationId,
        ) -> NetworkResult<ConfigurationJson>{
            unreachable!()
        }

        #[allow(unreachable_code)]
        fn get_configuration_monitoring_websocket(
            &self,
            collection: &ConfigurationId,
        ) -> NetworkResult<impl WebsocketReader>{
            unreachable!() as crate::NetworkResult<WebsocketMockReader>
        }

        fn push_metering_data(
            &self,
            data: &MeteringDataJson
        ) -> NetworkResult<()>{
            self.metering_data_sender.send(()).unwrap();
            Ok(())
        }
    }

    #[test]
    fn test_metrics_sent() {
        let (server_client, metering_data_sent_receiver) = ServerClientMock::new();
        let (_, metering_handle) = start_metering(ConfigurationId::new("".to_string(),"".to_string(), "".to_string()), std::time::Duration::ZERO, server_client);
        
        metering_handle.record_evaluation("".to_string(), "".to_string(), None);

        let data_sent = metering_data_sent_receiver.recv().unwrap();
    }

}