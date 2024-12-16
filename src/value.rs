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

use std::str::FromStr;

#[derive(PartialEq, Debug, Clone)]
pub struct NumericValue(pub(crate) serde_json::Number);

impl NumericValue {
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }
    pub fn as_u64(&self) -> Option<u64> {
        self.0.as_u64()
    }
    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }
}

impl PartialOrd for NumericValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self.0.as_i64(), self.0.as_u64(), self.0.as_f64()) {
            (Some(a), _, _) => match (other.0.as_i64(), other.0.as_u64(), other.0.as_f64()) {
                (Some(b), _, _) => Some(a.cmp(&b)),
                (_, Some(b), _) => {
                    if a < 0 {
                        Some(std::cmp::Ordering::Less)
                    } else {
                        (a as u64).partial_cmp(&b)
                    }
                }
                (_, _, Some(b)) => (a as f64).partial_cmp(&b),
                _ => None,
            },
            (_, Some(a), _) => match (other.0.as_i64(), other.0.as_u64(), other.0.as_f64()) {
                (_, Some(b), _) => Some(a.cmp(&b)),
                (Some(b), _, _) => {
                    if b < 0 {
                        Some(std::cmp::Ordering::Greater)
                    } else {
                        a.partial_cmp(&(b as u64))
                    }
                }
                (_, _, Some(b)) => (a as f64).partial_cmp(&b),
                _ => None,
            },
            (_, _, Some(a)) => match (other.0.as_i64(), other.0.as_u64(), other.0.as_f64()) {
                (_, _, Some(b)) => a.partial_cmp(&b),
                (Some(b), _, _) => a.partial_cmp(&(b as f64)),
                (_, Some(b), _) => a.partial_cmp(&(b as f64)),
                _ => None,
            },
            _ => None, // not representable using rust types
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum Value {
    Numeric(NumericValue),
    String(String),
    Boolean(bool),
}

impl FromStr for NumericValue {
    type Err = serde_json::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(NumericValue(value.parse()?))
    }
}

impl TryFrom<f64> for Value {
    type Error = crate::Error;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Ok(Value::Numeric(NumericValue(
            serde_json::Number::from_f64(value)
            .ok_or(crate::Error::Other("Could not convert Float into a Number. Note: some floating point numbers like NaN or Infinite can not be represented.".into()))?
        )))
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Numeric(NumericValue(value.into()))
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Numeric(NumericValue(value.into()))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Numeric(NumericValue(value.into()))
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Value::Numeric(NumericValue(value.into()))
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Boolean(value)
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use rstest::*;

    #[test]
    fn test_numeric() {
        let value = Value::Numeric(NumericValue(
            serde_json::Value::Number(42.into())
                .as_number()
                .unwrap()
                .clone(),
        ));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_f64().unwrap() == 42f64));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_i64().unwrap() == 42i64));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_u64().unwrap() == 42u64));

        let value = Value::try_from(-42.0f64).unwrap();
        assert!(matches!(value, Value::Numeric(ref v) if v.as_f64().unwrap() == -42f64));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_i64().is_none()));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_u64().is_none()));

        let value = Value::from(-42i64);
        assert!(matches!(value, Value::Numeric(ref v) if v.as_f64().unwrap() == -42f64));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_i64().unwrap() == -42i64));
        assert!(matches!(value, Value::Numeric(ref v) if v.as_u64().is_none()));
    }

    #[rstest]
    #[case("5.5", "5.4")]
    #[case("5.5", "4")]
    #[case("5.5", "-4")]
    #[case("5", "4.9")]
    #[case("5", "4")]
    #[case("5", "-4")]
    #[case("-5", "-5.1")]
    #[case("-5", "-6")]
    fn test_numeric_order(#[case] left: &str, #[case] right: &str) {
        assert!(
            left.parse::<NumericValue>().unwrap() > right.parse::<NumericValue>().unwrap(),
            "{} > {}",
            left,
            right
        );
        assert!(
            right.parse::<NumericValue>().unwrap() < left.parse::<NumericValue>().unwrap(),
            "{} < {}",
            right,
            left
        );
    }
}
