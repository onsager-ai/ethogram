//! Inspect the typed control kind before `validate` converts it to JSON.
//! Serde's transparent newtype name preserves the Unknown variant for this
//! probe while ordinary JSON serialisation still emits exactly one string.
//! Only the payload's `kind` is inspected; wire values and other unions use
//! the existing representation and policy checks in `validate`.

use serde::ser::{self, Serialize, Serializer};

use crate::{ControlKind, ValidationError};

pub(crate) const UNKNOWN_CONTROL_KIND: &str = "ethogram::ControlKind::Unknown";

impl ser::Error for ValidationError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::malformed("payload", message.to_string())
    }
}

pub(crate) fn check<P: Serialize + ?Sized>(payload: &P) -> Result<(), ValidationError> {
    payload.serialize(Probe::Payload)
}

#[derive(Clone, Copy)]
enum Probe {
    Payload,
    Kind,
    Ignore,
}

impl Probe {
    fn field(&self, key: &str) -> Self {
        if matches!(self, Self::Payload) && key == "kind" {
            Self::Kind
        } else {
            Self::Ignore
        }
    }
}

// This probe produces no JSON. Primitive representation checks remain in
// the existing conversion/decoder, so these methods deliberately do nothing.
macro_rules! ignore_primitives {
    ($($method:ident($($arg:ident: $ty:ty),*);)*) => {
        $(fn $method(self, $(_: $ty),*) -> Result<(), Self::Error> { Ok(()) })*
    };
}

impl Serializer for Probe {
    type Ok = ();
    type Error = ValidationError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = MapProbe;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore_primitives! {
        serialize_bool(v: bool);
        serialize_i8(v: i8); serialize_i16(v: i16); serialize_i32(v: i32);
        serialize_i64(v: i64); serialize_i128(v: i128);
        serialize_u8(v: u8); serialize_u16(v: u16); serialize_u32(v: u32);
        serialize_u64(v: u64); serialize_u128(v: u128);
        serialize_f32(v: f32); serialize_f64(v: f64);
        serialize_char(v: char); serialize_str(v: &str); serialize_bytes(v: &[u8]);
        serialize_none(); serialize_unit(); serialize_unit_struct(name: &'static str);
        serialize_unit_variant(name: &'static str, index: u32, variant: &'static str);
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        if matches!(self, Self::Kind) && name == UNKNOWN_CONTROL_KIND {
            let raw = serde_json::to_value(value)
                .map_err(|error| ValidationError::malformed("payload.kind", error.to_string()))?;
            if let Some(raw) = raw.as_str()
                && !matches!(
                    ControlKind::from_wire(raw.to_owned()),
                    ControlKind::Unknown(_)
                )
            {
                return Err(ValidationError::malformed(
                    "payload.kind",
                    format!(
                        "ControlRequestedPayload.kind cannot use Unknown for known value: {raw}"
                    ),
                ));
            }
            return Ok(());
        }
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Self::Error> {
        Ok(Self::Ignore)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Self::Error> {
        Ok(Self::Ignore)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Self::Error> {
        Ok(Self::Ignore)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Self::Error> {
        Ok(Self::Ignore)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<MapProbe, Self::Error> {
        Ok(MapProbe {
            parent: self,
            next: Self::Ignore,
        })
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Self::Error> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Self::Error> {
        Ok(Self::Ignore)
    }
}

macro_rules! ignore_elements {
    ($($trait:ident, $method:ident);* $(;)?) => {
        $(impl ser::$trait for Probe {
            type Ok = ();
            type Error = ValidationError;
            fn $method<T: Serialize + ?Sized>(&mut self, _: &T) -> Result<(), Self::Error> { Ok(()) }
            fn end(self) -> Result<(), Self::Error> { Ok(()) }
        })*
    };
}

ignore_elements! {
    SerializeSeq, serialize_element;
    SerializeTuple, serialize_element;
    SerializeTupleStruct, serialize_field;
    SerializeTupleVariant, serialize_field;
}

impl ser::SerializeStruct for Probe {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        if matches!(self.field(key), Self::Kind) {
            value.serialize(Self::Kind)?;
        }
        Ok(())
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for Probe {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _: &'static str,
        _: &T,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// `#[serde(flatten)]` makes a typed ControlRequestedPayload use SerializeMap;
// a wrapper struct without flatten uses SerializeStruct. Cover both paths.
struct MapProbe {
    parent: Probe,
    next: Probe,
}

impl ser::SerializeMap for MapProbe {
    type Ok = ();
    type Error = ValidationError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.next = Probe::Ignore;
        if matches!(self.parent, Probe::Payload) {
            let key = serde_json::to_value(key)
                .map_err(|error| ValidationError::malformed("payload", error.to_string()))?;
            if let Some(key) = key.as_str() {
                self.next = self.parent.field(key);
            }
        }
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        if matches!(self.next, Probe::Kind) {
            value.serialize(self.next)?;
        }
        self.next = Probe::Ignore;
        Ok(())
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}
