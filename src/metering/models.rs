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

pub(crate) enum SubjectId {
    Feature(String),
    Property(String),
}

pub(crate) struct EvaluationEventData {
    /// ID if the subject being evaluated. E.g. feature ID.
    pub subject_id: SubjectId,
    /// The ID of the Entity against which the subject was evaluated.
    pub entity_id: String,
    /// If applicable, the segment the subject was associated to during evaluation.
    pub segment_id: Option<String>,
}

pub(crate) enum EvaluationEvent {
    Feature(EvaluationEventData),
    Property(EvaluationEventData),
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub(crate) struct MeteringKey {
    pub feature_id: Option<String>,
    pub property_id: Option<String>,
    pub entity_id: String,
    pub segment_id: Option<String>,
}

impl MeteringKey {
    pub fn from_feature(feature_id: String, entity_id: String, segment_id: Option<String>) -> Self {
        Self {
            feature_id: Some(feature_id),
            property_id: None,
            entity_id,
            segment_id,
        }
    }

    pub fn from_property(
        property_id: String,
        entity_id: String,
        segment_id: Option<String>,
    ) -> Self {
        Self {
            feature_id: None,
            property_id: Some(property_id),
            entity_id,
            segment_id,
        }
    }
}

pub(crate) struct EvaluationData {
    pub number_of_evaluations: u32,
    pub time_of_last_evaluation: chrono::DateTime<chrono::Utc>,
}

impl Default for EvaluationData {
    fn default() -> Self {
        Self {
            number_of_evaluations: 1,
            time_of_last_evaluation: chrono::Utc::now(),
        }
    }
}

impl EvaluationData {
    pub fn add_one(&mut self) {
        self.number_of_evaluations += 1;
        self.time_of_last_evaluation = chrono::Utc::now();
    }
}
