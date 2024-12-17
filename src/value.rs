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

use crate::Error;

/// A wrapper on top of the primitive types acepted by the library.
#[derive(PartialEq, Debug, Clone)]
pub enum Value {
    Float64(f64),
    UInt64(u64),
    Int64(i64),
    String(String),
    Boolean(bool),
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Float64(value)
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Value::UInt64(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Int64(value)
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

impl TryFrom<Value> for f64 {
    type Error = crate::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Float64(f) => Ok(f),
            _ => Err(Error::MismatchType),
        }
    }
}

impl TryFrom<Value> for u64 {
    type Error = crate::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::UInt64(f) => Ok(f),
            Value::Int64(v) => v.try_into().map_err(|_| Error::MismatchType),
            _ => Err(Error::MismatchType),
        }
    }
}

impl TryFrom<Value> for i64 {
    type Error = crate::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Int64(f) => Ok(f),
            Value::UInt64(v) => v.try_into().map_err(|_| Error::MismatchType),
            _ => Err(Error::MismatchType),
        }
    }
}

impl TryFrom<Value> for String {
    type Error = crate::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(f) => Ok(f),
            _ => Err(Error::MismatchType),
        }
    }
}

impl TryFrom<Value> for bool {
    type Error = crate::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Boolean(f) => Ok(f),
            _ => Err(Error::MismatchType),
        }
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;

    #[test]
    fn test_numeric_f64() {
        let value = Value::from(42f64);
        assert!(matches!(value, Value::Float64(ref v) if v == &42f64));

        let as_f64: f64 = value.clone().try_into().unwrap();
        assert_eq!(as_f64, 42f64);

        assert!(matches!(TryInto::<u64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<i64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<String>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<bool>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
    }

    #[test]
    fn test_numeric_u64() {
        // An u64 within the range of i64
        {
            let value = Value::from(42u64);
            assert!(matches!(value, Value::UInt64(ref v) if v == &42u64));
    
            let as_u64: u64 = value.clone().try_into().unwrap();
            assert_eq!(as_u64, 42u64);

            let as_i64: i64 = value.clone().try_into().unwrap();
            assert_eq!(as_i64, 42i64);

            assert!(matches!(TryInto::<f64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
            assert!(matches!(TryInto::<String>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
            assert!(matches!(TryInto::<bool>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        }

        // An u64 outside the range of i64
        {
            let value = Value::from(u64::MAX);
            assert!(matches!(TryInto::<i64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        }
    }

    #[test]
    fn test_numeric_i64() {
        // An i64 within the range of u64
        {
            let value = Value::from(42i64);
            assert!(matches!(value, Value::Int64(ref v) if v == &42i64));
    
            let as_i64: i64 = value.clone().try_into().unwrap();
            assert_eq!(as_i64, 42i64);

            let as_u64: u64 = value.clone().try_into().unwrap();
            assert_eq!(as_u64, 42u64);

            assert!(matches!(TryInto::<f64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
            assert!(matches!(TryInto::<String>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
            assert!(matches!(TryInto::<bool>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        }

        // An i64 outside the range of u64
        {
            let value = Value::from(-2i64);
            assert!(matches!(TryInto::<u64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        }
    }
    
    #[test]
    fn test_string() {
        let value = Value::from("value".to_string());
        assert!(matches!(value, Value::String(ref v) if v == "value"));

        let as_string: String = value.clone().try_into().unwrap();
        assert_eq!(as_string, "value");

        assert!(matches!(TryInto::<f64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<u64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<i64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<bool>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
    }

    #[test]
    #[allow(clippy::bool_assert_comparison)]
    fn test_boolean() {
        let value = Value::from(false);
        assert!(matches!(value, Value::Boolean(ref v) if v == &false));

        let as_boolean: bool = value.clone().try_into().unwrap();
        assert_eq!(as_boolean, false);

        assert!(matches!(TryInto::<f64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<u64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<i64>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
        assert!(matches!(TryInto::<String>::try_into(value.clone()).unwrap_err(), Error::MismatchType));
    }
}
