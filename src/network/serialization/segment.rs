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

use serde::Deserialize;

use super::Rule;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Segment {
    pub name: String,
    pub segment_id: String,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub rules: Vec<Rule>,
}

#[cfg(test)]
pub mod fixtures {
    use std::collections::HashMap;

    use crate::network::serialization::{Rule, Segment};
    use rstest::*;

    #[fixture]
    pub fn some_segments() -> HashMap<String, Segment> {
        HashMap::from([
            (
                "some_segment_id_1".into(),
                Segment {
                    name: "".into(),
                    segment_id: "some_segment_id_1".into(),
                    description: None,
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            ),
            (
                "some_segment_id_2".into(),
                Segment {
                    name: "".into(),
                    segment_id: "some_segment_id_2".into(),
                    description: None,
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["peter".into()],
                    }],
                },
            ),
            (
                "some_segment_id_3".into(),
                Segment {
                    name: "".into(),
                    segment_id: "some_segment_id_3".into(),
                    description: None,
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["jane".into()],
                    }],
                },
            ),
        ])
    }
}
