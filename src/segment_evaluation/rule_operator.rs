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

use super::errors::CheckOperatorErrorDetail;
use crate::Value;

pub(crate) trait RuleOperator {
    fn operate(
        &self,
        operator: &str,
        value: &str,
    ) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match operator {
            "is" => self.is(value),
            "contains" => self.contains(value),
            "startsWith" => self.starts_with(value),
            "endsWith" => self.ends_with(value),
            "greaterThan" => self.greater_than(value),
            "lesserThan" => self.lesser_than(value),
            // Counterpart operators
            "greaterThanEquals" => self.lesser_than(value).map(std::ops::Not::not),
            "lesserThanEquals" => self.greater_than(value).map(std::ops::Not::not),
            "isNot" => self.is(value).map(std::ops::Not::not),
            "notContains" => self.contains(value).map(std::ops::Not::not),
            "notStartsWith" => self.starts_with(value).map(std::ops::Not::not),
            "notEndsWith" => self.ends_with(value).map(std::ops::Not::not),
            _ => Err(CheckOperatorErrorDetail::OperatorNotImplemented),
        }
    }

    fn is(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail>;
    fn contains(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail>;
    fn starts_with(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail>;
    fn ends_with(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail>;
    fn greater_than(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail>;
    fn lesser_than(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail>;
}

impl RuleOperator for Value {
    fn is(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match self {
            Value::String(data) => Ok(*data == value),
            Value::Boolean(data) => Ok(*data == value.parse::<bool>()?),
            Value::Float64(data) => Ok(*data == value.parse::<f64>()?),
            Value::UInt64(data) => Ok(*data == value.parse::<u64>()?),
            Value::Int64(data) => Ok(*data == value.parse::<i64>()?),
        }
    }

    fn contains(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match self {
            Value::String(data) => Ok(data.contains(value)),
            _ => Err(CheckOperatorErrorDetail::StringExpected),
        }
    }

    fn starts_with(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match self {
            Value::String(data) => Ok(data.starts_with(value)),
            _ => Err(CheckOperatorErrorDetail::StringExpected),
        }
    }

    fn ends_with(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match self {
            Value::String(data) => Ok(data.ends_with(value)),
            _ => Err(CheckOperatorErrorDetail::StringExpected),
        }
    }

    fn greater_than(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match self {
            // TODO: Go implementation also compares strings (by parsing them as floats). Do we need this?
            //       https://github.com/IBM/appconfiguration-go-sdk/blob/master/lib/internal/models/Rule.go#L82
            // TODO: we could have numbers not representable as f64, maybe we should try to parse it to i64 and u64 too?
            Value::Float64(data) => Ok(*data > value.parse()?),
            Value::UInt64(data) => Ok(*data > value.parse()?),
            Value::Int64(data) => Ok(*data > value.parse()?),
            _ => Err(CheckOperatorErrorDetail::EntityAttrNotANumber),
        }
    }

    fn lesser_than(&self, value: &str) -> std::result::Result<bool, CheckOperatorErrorDetail> {
        match self {
            Value::Float64(data) => Ok(*data < value.parse()?),
            Value::UInt64(data) => Ok(*data < value.parse()?),
            Value::Int64(data) => Ok(*data < value.parse()?),
            _ => Err(CheckOperatorErrorDetail::EntityAttrNotANumber),
        }
    }
}
