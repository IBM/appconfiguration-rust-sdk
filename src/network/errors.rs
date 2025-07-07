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

use std::sync::PoisonError;

use thiserror::Error;

use crate::ConfigurationDataError;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    TungsteniteError(#[from] tungstenite::Error),

    #[error("Protocol error. Unexpected data received from server")]
    ProtocolError,

    #[error("Cannot parse '{0}' as URL")]
    UrlParseError(String),

    #[error("Invalid header value for '{0}'")]
    InvalidHeaderValue(String),

    #[error("Cannot acquire lock")]
    CannotAcquireLock,

    #[error("Contact to server lost")]
    ContactToServerLost,

    #[error(transparent)]
    ConfigurationDataError(#[from] ConfigurationDataError),
}

impl<T> From<PoisonError<T>> for NetworkError {
    fn from(_value: PoisonError<T>) -> Self {
        NetworkError::CannotAcquireLock
    }
}
