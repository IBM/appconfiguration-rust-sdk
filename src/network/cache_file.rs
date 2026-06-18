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

use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::{Error, Result, errors::DeserializationError};

pub(crate) struct CacheFile;

impl CacheFile {
    pub(crate) fn read_persistent_cache_string(filepath: &Path) -> String {
        let Ok(exists) = filepath.try_exists() else {
            return String::new();
        };

        if !exists {
            return String::new();
        }

        match fs::read_to_string(filepath) {
            Ok(contents) => contents.trim().to_string(),
            Err(_) => String::new(),
        }
    }

    pub(crate) fn read_bootstrap_string(filepath: &Path) -> Result<String> {
        let exists = filepath.try_exists().map_err(|e| {
            Error::Other(format!(
                "Failed to access bootstrap file '{}': {}",
                filepath.display(),
                e
            ))
        })?;

        if !exists {
            return Err(Error::Other(format!(
                "given bootstrap file path doesn't exist: {}",
                filepath.display()
            )));
        }

        let contents = fs::read_to_string(filepath).map_err(|e| {
            Error::Other(format!(
                "File '{}' doesn't exist or cannot be read: {}",
                filepath.display(),
                e
            ))
        })?;

        let trimmed = contents.trim().to_string();
        if trimmed.is_empty() {
            return Err(Error::Other(format!(
                "given bootstrap file is empty: {}",
                filepath.display()
            )));
        }

        Ok(trimmed)
    }

    pub(crate) fn read_json_file<T>(filepath: &Path) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let file = fs::File::open(filepath).map_err(|_| {
            Error::Other(format!(
                "File '{}' doesn't exist or cannot be read",
                filepath.display()
            ))
        })?;
        let reader = BufReader::new(file);

        serde_json::from_reader(reader).map_err(|e| {
            Error::DeserializationError(DeserializationError {
                string: format!(
                    "Error deserializing Configuration from file '{}'",
                    filepath.display()
                ),
                source: e.into(),
            })
        })
    }

    #[allow(dead_code)]
    pub(crate) fn write_json_file<T>(value: &T, filepath: &Path) -> Result<()>
    where
        T: serde::Serialize,
    {
        if let Some(parent) = filepath.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Error::Other(format!(
                    "Failed to create parent directory for '{}': {}",
                    filepath.display(),
                    e
                ))
            })?;
        }

        let file = fs::File::create(filepath).map_err(|e| {
            Error::Other(format!(
                "Failed to create configuration file '{}': {}",
                filepath.display(),
                e
            ))
        })?;
        let writer = BufWriter::new(file);

        serde_json::to_writer(writer, value).map_err(|e| {
            Error::DeserializationError(DeserializationError {
                string: format!(
                    "Error serializing Configuration to file '{}'",
                    filepath.display()
                ),
                source: e.into(),
            })
        })
    }

    pub(crate) fn delete_file_data(filepath: &Path) {
        let Ok(exists) = filepath.try_exists() else {
            return;
        };

        if !exists {
            return;
        }

        let _ = fs::remove_file(filepath);
    }
}
