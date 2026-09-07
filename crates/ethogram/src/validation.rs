use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};
use serde::{Deserializer, Serialize};
use serde_json::Value;

/// The closed set of validation failures a sink maps to `capture.refused`.
/// Paths start at `payload`; array positions use brackets, e.g. `payload.options[0].label`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum ValidationErrorKind {
    OverBound {
        path: String,
        count: usize,
        max: usize,
    },
    PayloadTooLarge {
        bytes: usize,
        max: usize,
    },
    UnknownMember {
        path: String,
        value: String,
    },
    MissingField {
        path: String,
    },
    /// A producer broke a stated rule while sending an otherwise
    /// representable value: the steer-without-text rule, or one of the three
    /// `onTimeout` checks.
    Policy {
        path: String,
        message: String,
    },
    /// The value sent could not be represented at all: wrong-typed, an
    /// unsafe integer, or not serialisable. Distinct from `Policy`, which is
    /// a stated rule broken by an otherwise representable value.
    Malformed {
        path: String,
        message: String,
    },
}

/// A structured validation failure with the original diagnostic preserved.
/// Match `kind` to choose a refusal cause; `Display` remains compatible with
/// the messages emitted before structured errors were introduced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
    message: String,
}

impl ValidationError {
    pub(crate) fn new(kind: ValidationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn policy(path: impl Into<String>, message: impl Into<String>) -> Self {
        let message = message.into();
        Self::new(
            ValidationErrorKind::Policy {
                path: path.into(),
                message: message.clone(),
            },
            message,
        )
    }

    pub(crate) fn malformed(path: impl Into<String>, message: impl Into<String>) -> Self {
        let message = message.into();
        Self::new(
            ValidationErrorKind::Malformed {
                path: path.into(),
                message: message.clone(),
            },
            message,
        )
    }

    // Deserialisation attaches the location at the innermost value that
    // fails. Serde supplies missing field names through its typed callback,
    // so neither the kind nor the path is recovered by parsing a message.
    // `Malformed` needs the same empty-path attachment `Policy` does: both
    // can originate from `custom()` below, deep inside a nested deserialize
    // call, with no path known until the enclosing `LocatedValue` unwinds —
    // a representation failure arriving without a path would be a
    // regression in diagnostic quality.
    fn at_path(mut self, location: &str) -> Self {
        match &mut self.kind {
            ValidationErrorKind::MissingField { path } if !path.starts_with("payload.") => {
                *path = format!("{location}.{path}");
            }
            ValidationErrorKind::Policy { path, .. } if path.is_empty() => {
                *path = location.to_owned();
            }
            ValidationErrorKind::Malformed { path, .. } if path.is_empty() => {
                *path = location.to_owned();
            }
            _ => {}
        }
        self
    }
}

impl Display for ValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ValidationError {}

impl From<ValidationError> for serde_json::Error {
    fn from(error: ValidationError) -> Self {
        de::Error::custom(error)
    }
}

impl Serialize for ValidationError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.kind.serialize(serializer)
    }
}

impl de::Error for ValidationError {
    // Serde routes every unclassified deserialisation failure through this
    // generic entry point, including the two overrides below (`invalid_type`,
    // `invalid_value`) and `deserialize_safe_u64`'s explicit out-of-range
    // message in `lib.rs`. Every message that reaches here — a wrong-typed
    // value, an invalid value, or an unsafe integer — describes a value that
    // could not be represented at all, never a stated rule broken by an
    // otherwise representable one, so it is `Malformed` rather than `Policy`.
    fn custom<T: Display>(message: T) -> Self {
        Self::malformed("", message.to_string())
    }

    fn missing_field(field: &'static str) -> Self {
        let message = <serde_json::Error as de::Error>::missing_field(field).to_string();
        Self::new(
            ValidationErrorKind::MissingField {
                path: field.to_owned(),
            },
            message,
        )
    }

    fn invalid_type(unexpected: de::Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        // serde_json spells unit as null and formats floats itself. Keep
        // those diagnostics, including their punctuation, exactly intact.
        Self::custom(<serde_json::Error as de::Error>::invalid_type(
            unexpected, expected,
        ))
    }

    fn invalid_value(unexpected: de::Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        Self::custom(<serde_json::Error as de::Error>::invalid_value(
            unexpected, expected,
        ))
    }
}

/// Decode the SDK's known payload structs with the same serde visitors as
/// `from_value`, carrying location and missing-field metadata through serde
/// instead of erasing them into `serde_json::Error`.
pub(crate) fn decode_payload<T: DeserializeOwned>(value: Value) -> Result<T, ValidationError> {
    T::deserialize(LocatedValue {
        value,
        path: "payload".to_owned(),
    })
}

struct LocatedValue {
    value: Value,
    path: String,
}

impl<'de> IntoDeserializer<'de, ValidationError> for LocatedValue {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> Deserializer<'de> for LocatedValue {
    type Error = ValidationError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let path = self.path;
        let result = match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(value) => visitor.visit_bool(value),
            Value::String(value) => visitor.visit_string(value),
            Value::Number(value) => {
                if let Some(value) = value.as_u64() {
                    visitor.visit_u64(value)
                } else if let Some(value) = value.as_i64() {
                    visitor.visit_i64(value)
                } else {
                    visitor.visit_f64(value.as_f64().expect("JSON numbers fit f64"))
                }
            }
            Value::Array(values) => {
                let values = values.into_iter().enumerate().map(|(index, value)| Self {
                    value,
                    path: format!("{path}[{index}]"),
                });
                let mut sequence = de::value::SeqDeserializer::new(values);
                visitor.visit_seq(&mut sequence).and_then(|value| {
                    sequence.end()?;
                    Ok(value)
                })
            }
            Value::Object(values) => {
                let values = values.into_iter().map(|(key, value)| {
                    let child = Self {
                        value,
                        path: format!("{path}.{key}"),
                    };
                    (key, child)
                });
                let mut map = de::value::MapDeserializer::new(values);
                visitor.visit_map(&mut map).and_then(|value| {
                    map.end()?;
                    Ok(value)
                })
            }
        };
        result.map_err(|error: ValidationError| error.at_path(&path))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.value.is_null() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct enum identifier
    }
}
