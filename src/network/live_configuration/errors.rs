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

use std::sync::PoisonError;

use thiserror::Error;

use super::current_mode::CurrentModeOfflineReason;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum Error {
    #[error("Cannot acquire lock")]
    CannotAcquireLock,

    #[error("Connection to server lost: {0}")]
    Offline(CurrentModeOfflineReason),

    #[error("Thread failed with internal error: {0}")]
    ThreadInternalError(String),

    #[error("{0}")]
    UnrecoverableError(String),

    #[error("Configuration is not yet available (try again later)")]
    ConfigurationNotYetAvailable,
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_value: PoisonError<T>) -> Self {
        Error::CannotAcquireLock
    }
}
