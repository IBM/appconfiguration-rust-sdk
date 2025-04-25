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

use super::{CurrentMode, Error, Result};
use crate::client::configuration::Configuration;
use crate::utils::ThreadStatus;

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
