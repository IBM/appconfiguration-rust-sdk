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

use std::sync::{Arc, Mutex};

use super::current_mode::CurrentModeOfflineReason;
use super::update_thread_worker::UpdateThreadWorker;
use super::{CurrentMode, Error, Result};
use crate::client::configuration::Configuration;
use crate::network::http_client::ServerClient;
use crate::utils::{ThreadHandle, ThreadStatus};
use crate::ConfigurationId;

pub(crate) trait LiveConfiguration {
    /// Returns the current configuration
    ///
    /// Depending on the current operation mode (see [`LiveConfiguration::get_current_mode`]) and
    /// the configured offline behavior (see [`OfflineMode`]) for this object, this
    /// configuration might come from different sources: server, cache or user-provided.
    fn get_configuration(&self) -> Result<Configuration>;

    /// Utility method to know the current status of the inner thread that keeps
    /// the configuration synced with the server.
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>>;

    /// Utility method to get the current operating mode of the object.
    fn get_current_mode(&self) -> Result<CurrentMode>;
}

#[derive(Debug)]
pub struct LiveConfigurationImpl {
    /// Configuration object that will be returned to consumers. This is also the object
    /// that the thread in the backend will be updating.
    configuration: Arc<Mutex<Option<Configuration>>>,

    /// Current operation mode.
    current_mode: Arc<Mutex<CurrentMode>>,

    /// Handler to the internal thread that takes care of updating the [`LiveConfigurationImpl::configuration`].
    update_thread: ThreadHandle<Result<()>>,
}

impl LiveConfigurationImpl {
    /// Creates a new [`LiveConfigurationImpl`] object and starts a thread running an instance
    /// of [`UpdateThreadWorker`].
    pub fn new<T: ServerClient>(server_client: T, configuration_id: ConfigurationId) -> Self {
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let worker = UpdateThreadWorker::new(
            server_client,
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let update_thread =
            ThreadHandle::new(move |terminator_receiver| worker.run(terminator_receiver));

        Self {
            configuration,
            update_thread,
            current_mode,
        }
    }
}

impl LiveConfiguration for LiveConfigurationImpl {
    fn get_configuration(&self) -> Result<Configuration> {
        match &*self.current_mode.lock()? {
            CurrentMode::Online => {
                match &*self.configuration.lock()? {
                    // We store the configuration retrieved from the server into the Arc<Mutex> before switching the flag to Online
                    None => unreachable!(),
                    // TODO: we do not want to clone here
                    Some(configuration) => Ok(configuration.clone()),
                }
            }
            CurrentMode::Offline(current_mode_offline_reason) => {
                Err(Error::Offline(current_mode_offline_reason.clone()))
            }
            CurrentMode::Defunct(result) => Err(Error::ThreadInternalError(format!(
                "Thread finished with status: {:?}",
                result
            ))),
        }
    }

    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>> {
        self.update_thread.get_thread_status()
    }

    fn get_current_mode(&self) -> Result<CurrentMode> {
        Ok(self.current_mode.lock()?.clone())
    }
}
