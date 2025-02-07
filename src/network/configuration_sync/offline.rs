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

use crate::client::configuration::Configuration;

/// Defines the behaviour of the client while the connection to the server
/// is lost. In all cases the client will keep trying to reconnect forever.
#[derive(Debug)]
pub enum OfflineMode {
    /// Returns errors when requesting features or evaluating them
    Fail,

    /// Return features and values from the latests configuration available
    Cache,

    /// Use the provided configuration.
    FallbackData(Configuration), // FIXME: The public type "should" be ConfigurationJSON, or the user should just provide a JSON file (same input as the Offline client)
}
