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

use crate::AppConfigurationOffline;
use std::path::{Path, PathBuf};

/// Defines the behaviour of the client while the connection to the server
/// is lost. In all cases the client will keep trying to reconnect forever.
#[derive(Debug)]
pub enum OfflineMode {
    /// Returns an error immediately when the server is unreachable and no
    /// cached configuration is available. If a previously-fetched configuration
    /// is held in memory it will **not** be used — the caller always receives
    /// an [`Error::Offline`](crate::Error::Offline) error until the server
    /// comes back online.
    Fail,

    /// Return features and values from the latest in-memory configuration
    /// when the server is unreachable. If no configuration has ever been
    /// fetched, returns [`Error::ConfigurationNotYetAvailable`](crate::Error).
    Cache,

    /// Use the provided configuration.
    FallbackData(AppConfigurationOffline),

    PersistentCacheFile {
        path: PathBuf,
        environment_id: String,
        collection_id: String,
    },

    /// Load fallback data lazily from a bootstrap file.
    BootstrapFile {
        path: PathBuf,
        environment_id: String,
        collection_id: String,
    },
}

impl OfflineMode {
    pub fn persistent_cache_file(
        path: impl AsRef<Path>,
        environment_id: impl Into<String>,
        collection_id: impl Into<String>,
    ) -> Self {
        Self::PersistentCacheFile {
            path: path.as_ref().to_path_buf(),
            environment_id: environment_id.into(),
            collection_id: collection_id.into(),
        }
    }

    pub fn bootstrap_file(
        path: impl AsRef<Path>,
        environment_id: impl Into<String>,
        collection_id: impl Into<String>,
    ) -> Self {
        Self::BootstrapFile {
            path: path.as_ref().to_path_buf(),
            environment_id: environment_id.into(),
            collection_id: collection_id.into(),
        }
    }
}
