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

//! Application internal models.
//!
//! All input data is converted to these models as soon as possible and
//! all the operations run on these models.
//!
//! These models are also internal to the application so they can
//! evolve without breaking the API offered to users.
//!

mod configuration;
mod evaluation_result;
mod feature_snapshot;
mod property_snapshot;
mod rollout_configuration;
mod secret_property;

pub(crate) use configuration::Configuration;
pub use evaluation_result::{
    EvaluationContext, EvaluationRuleCondition, EvaluationRuleContext, EvaluationSegmentContext,
    FeatureEvaluationDetails, FeatureEvaluationResult, PropertyEvaluationDetails,
    PropertyEvaluationResult,
};
pub(crate) use feature_snapshot::FeatureSnapshot;
pub(crate) use property_snapshot::PropertySnapshot;
#[allow(unused_imports)] // used in rollout_parser tests via crate::models::RolloutPhase
pub(crate) use rollout_configuration::RolloutPhase;
pub(crate) use rollout_configuration::{DELIMITER, ROLLOUT_TYPE_PROGRESSIVE, RolloutConfiguration};
pub use secret_property::{SecretManager, SecretPropertySnapshot};
