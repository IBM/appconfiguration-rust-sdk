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

use crate::network::configuration_sync::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurrentMode {
    Online,
    Offline(CurrentModeOfflineReason),
    Defunct(Result<()>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurrentModeOfflineReason {
    LockError,
    FailedToGetNewConfiguration,
    Initializing,
    WebsocketClosed,
    WebsocketError,
    ConfigurationDataInvalid,
}

impl std::fmt::Display for CurrentModeOfflineReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CurrentModeOfflineReason::LockError => write!(f, "LockError"),
            CurrentModeOfflineReason::FailedToGetNewConfiguration => {
                write!(f, "FailedToGetNewConfiguration")
            }
            CurrentModeOfflineReason::Initializing => write!(f, "Initializing"),
            CurrentModeOfflineReason::WebsocketClosed => write!(f, "WebsocketClosed"),
            CurrentModeOfflineReason::WebsocketError => write!(f, "WebsocketError"),
            CurrentModeOfflineReason::ConfigurationDataInvalid => {
                write!(f, "ConfigurationDataInvalid")
            }
        }
    }
}
