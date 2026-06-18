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

use crate::errors::{Error, Result};
use crate::models::RolloutConfiguration;
use chrono::DateTime;
use std::collections::BTreeMap;
// TODO : Test progressive
/// Parse rollout configuration phases into a BTreeMap for efficient timestamp-to-percentage lookups.
///
/// The BTreeMap maps timestamp (in milliseconds since epoch) to rollout percentage.
/// This allows O(log n) lookups to find the current rollout percentage at any given time.
///
/// # Arguments
/// * `configuration` - The rollout configuration containing start_at timestamp and phases
///
/// # Returns
/// * `Result<BTreeMap<i64, u32>>` - A BTreeMap mapping timestamps to percentages
///
/// # Example
/// ```ignore
/// let config = RolloutConfiguration {
///     start_at: "2024-01-01T00:00:00Z".to_string(),
///     phases: vec![
///         RolloutPhase { percentage: 10, duration: Some(1), duration_type: Some("days".to_string()) },
///         RolloutPhase { percentage: 50, duration: Some(2), duration_type: Some("days".to_string()) },
///         RolloutPhase { percentage: 100, duration: None, duration_type: None },
///     ],
/// };
/// let btree = parse_rollout_configuration_phases(&config)?;
/// ```
pub fn parse_rollout_configuration_phases(
    configuration: &RolloutConfiguration,
) -> Result<BTreeMap<i64, u32>> {
    // Parse the start_at timestamp
    let start_timestamp = DateTime::parse_from_rfc3339(&configuration.start_at)
        .map_err(|e| Error::ProtocolError(format!("Invalid start_at timestamp: {}", e)))?
        .timestamp_millis();

    // Duration multipliers in milliseconds
    const MINUTE_MS: i64 = 60_000;
    const HOUR_MS: i64 = 3_600_000;
    const DAY_MS: i64 = 86_400_000;

    let mut btree = BTreeMap::new();

    // Before start_at, percentage is 0
    btree.insert(0, 0);

    let mut transition_time = start_timestamp;

    for phase in &configuration.phases {
        // Insert the percentage at the current transition time
        btree.insert(transition_time, phase.percentage);

        // Calculate next transition time if duration is specified
        if let (Some(duration), Some(duration_type)) = (phase.duration, &phase.duration_type) {
            let duration_ms = match duration_type.as_str() {
                "minutes" => MINUTE_MS * duration as i64,
                "hours" => HOUR_MS * duration as i64,
                "days" => DAY_MS * duration as i64,
                _ => {
                    return Err(Error::ProtocolError(format!(
                        "Invalid duration_type: {}. Must be 'minutes', 'hours', or 'days'",
                        duration_type
                    )));
                }
            };
            transition_time += duration_ms;
        }
    }

    Ok(btree)
}

/// Get the current rollout percentage from a BTreeMap at a given timestamp.
///
/// This function finds the entry with the largest key that is less than or equal to

///
/// # Arguments
/// * `btree` - The BTreeMap containing timestamp-to-percentage mappings
/// * `timestamp_ms` - The current timestamp in milliseconds since epoch
///
/// # Returns
/// * `u32` - The rollout percentage at the given timestamp (0 if no entry found)
pub fn get_current_rollout_percentage(btree: &BTreeMap<i64, u32>, timestamp_ms: i64) -> u32 {
    btree
        .range(..=timestamp_ms)
        .next_back()
        .map(|(_, &percentage)| percentage)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RolloutPhase;

    #[test]
    fn test_parse_rollout_configuration_phases() {
        let config = RolloutConfiguration {
            start_at: "2024-01-01T00:00:00Z".to_string(),
            phases: vec![
                RolloutPhase {
                    percentage: 10,
                    duration: Some(1),
                    duration_type: Some("days".to_string()),
                },
                RolloutPhase {
                    percentage: 50,
                    duration: Some(2),
                    duration_type: Some("days".to_string()),
                },
                RolloutPhase {
                    percentage: 100,
                    duration: None,
                    duration_type: None,
                },
            ],
        };

        let btree = parse_rollout_configuration_phases(&config).unwrap();

        // Check that we have the expected entries
        assert_eq!(btree.get(&0), Some(&0)); // Before start_at

        let start_ts = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(btree.get(&start_ts), Some(&10)); // At start_at

        let day1_ts = start_ts + 86_400_000; // 1 day later (after phase 1's duration)
        assert_eq!(btree.get(&day1_ts), Some(&50));

        // Phase 3 starts at start_ts + phase1.duration(1d) + phase2.duration(2d) = start_ts + 3d
        let day3_ts = start_ts + (3 * 86_400_000);
        assert_eq!(btree.get(&day3_ts), Some(&100));
    }

    #[test]
    fn test_get_current_rollout_percentage() {
        let mut btree = BTreeMap::new();
        btree.insert(0, 0);
        btree.insert(1000, 10);
        btree.insert(2000, 50);
        btree.insert(3000, 100);

        // Before any entry
        assert_eq!(get_current_rollout_percentage(&btree, -100), 0);

        // At exact timestamps
        assert_eq!(get_current_rollout_percentage(&btree, 0), 0);
        assert_eq!(get_current_rollout_percentage(&btree, 1000), 10);
        assert_eq!(get_current_rollout_percentage(&btree, 2000), 50);
        assert_eq!(get_current_rollout_percentage(&btree, 3000), 100);

        // Between timestamps (should return previous value)
        assert_eq!(get_current_rollout_percentage(&btree, 500), 0);
        assert_eq!(get_current_rollout_percentage(&btree, 1500), 10);
        assert_eq!(get_current_rollout_percentage(&btree, 2500), 50);
        assert_eq!(get_current_rollout_percentage(&btree, 5000), 100);
    }

    #[test]
    fn test_invalid_start_at() {
        let config = RolloutConfiguration {
            start_at: "invalid-timestamp".to_string(),
            phases: vec![],
        };

        let result = parse_rollout_configuration_phases(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_duration_type() {
        let config = RolloutConfiguration {
            start_at: "2024-01-01T00:00:00Z".to_string(),
            phases: vec![RolloutPhase {
                percentage: 10,
                duration: Some(1),
                duration_type: Some("invalid".to_string()),
            }],
        };

        let result = parse_rollout_configuration_phases(&config);
        assert!(result.is_err());
    }
}
