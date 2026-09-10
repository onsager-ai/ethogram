//! Inspect typed union members before `validate` converts the payload to JSON.
//! Serde's transparent newtype name preserves the Unknown variant for this
//! probe while ordinary JSON serialisation still emits exactly one string.
//! Only the registered event's top-level union field is inspected; wire values
//! use the existing checks in `validate`. Closedness is a separate policy,
//! checked after decoding at the same point as before the probe existed.

use serde::ser::{self, Serialize, Serializer};
use serde::{Deserialize, de::value::StrDeserializer};

use crate::{ValidationError, ValidationErrorKind};

#[derive(PartialEq, Eq)]
enum Membership {
    Open,
    Closed,
}

struct UnionRule {
    event_type: &'static str,
    field: &'static str,
    path: &'static str,
    label: &'static str,
    unknown_marker: &'static str,
    membership: Membership,
    is_known: fn(&str) -> Result<bool, ValidationError>,
}

// One declaration registers both the transparent Unknown marker and its probe
// rule. Every entry checks representability; membership policy is mandatory
// and has no default, so registering an open union cannot implicitly close it.
// Known values come from the union's existing deserializer, so adding a known
// variant cannot leave a separate validation vocabulary behind.
macro_rules! register_unions {
    ($($union:ident [$membership:ident] => ($event:ident, $payload:ident, $field:literal)),* $(,)?) => {
        $(impl Serialize for crate::$union {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self {
                    Self::Unknown(value) => serializer.serialize_newtype_struct(
                        concat!("ethogram::", stringify!($union), "::Unknown"), value,
                    ),
                    _ => serializer.serialize_str(self.as_str()),
                }
            }
        })*

        const RULES: &[UnionRule] = &[
            $(UnionRule {
                event_type: crate::$event,
                field: $field,
                path: concat!("payload.", $field),
                label: concat!(stringify!($payload), ".", $field),
                unknown_marker: concat!("ethogram::", stringify!($union), "::Unknown"),
                membership: Membership::$membership,
                is_known: |raw| {
                    let member = crate::$union::deserialize(
                        StrDeserializer::<ValidationError>::new(raw),
                    )?;
                    Ok(!matches!(member, crate::$union::Unknown(_)))
                },
            }),*
        ];

        #[cfg(test)]
        const REGISTERED_UNIONS: &[&str] = &[$(stringify!($union)),*];
    };
}

register_unions! {
    RunKind [Closed] => (RUN_STARTED, RunStartedPayload, "kind"),
    RunOutcome [Closed] => (RUN_FINISHED, RunFinishedPayload, "outcome"),
    ControlKind [Closed] => (CONTROL_REQUESTED, ControlRequestedPayload, "kind"),
    ControlAppliedReason [Open] => (CONTROL_APPLIED, ControlAppliedPayload, "reason"),
    CaptureRefusalCause [Closed] => (CAPTURE_REFUSED, CaptureRefusedPayload, "cause"),
    DecisionKind [Closed] => (DECISION_REQUESTED, DecisionRequestedPayload, "kind"),
}

pub(crate) fn check_closedness(event_type: &str, raw: &str) -> Result<(), ValidationError> {
    if let Some(rule) = RULES
        .iter()
        .find(|rule| rule.event_type == event_type && rule.membership == Membership::Closed)
        && !(rule.is_known)(raw)?
    {
        return Err(ValidationError::new(
            ValidationErrorKind::UnknownMember {
                path: rule.path.to_owned(),
                value: raw.to_owned(),
            },
            format!("{} has unknown value: {raw}", rule.label),
        ));
    }
    Ok(())
}

impl ser::Error for ValidationError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::malformed("payload", message.to_string())
    }
}

pub(crate) fn check_representability<P: Serialize + ?Sized>(
    event_type: &str,
    payload: &P,
) -> Result<(), ValidationError> {
    if let Some(rule) = RULES.iter().find(|rule| rule.event_type == event_type) {
        payload.serialize(Probe::Payload(rule))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Probe {
    Payload(&'static UnionRule),
    Member(&'static UnionRule),
    Ignore,
}

impl Probe {
    fn field(&self, key: &str) -> Self {
        if let Self::Payload(rule) = self
            && key == rule.field
        {
            Self::Member(rule)
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
        if let Self::Member(rule) = self
            && name == rule.unknown_marker
        {
            // The marker has already established that this is a typed Unknown.
            // Only now convert its inner string; converting the payload first
            // would erase the marker and silently accept known spellings.
            let raw = serde_json::to_value(value)
                .map_err(|error| ValidationError::malformed(rule.path, error.to_string()))?;
            if let Some(raw) = raw.as_str()
                && (rule.is_known)(raw)?
            {
                return Err(ValidationError::malformed(
                    rule.path,
                    format!("{} cannot use Unknown for known value: {raw}", rule.label),
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
        if let member @ Self::Member(_) = self.field(key) {
            value.serialize(member)?;
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

// `#[serde(flatten)]` makes the SDK's typed payloads use SerializeMap;
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
        if matches!(self.parent, Probe::Payload(_)) {
            let key = serde_json::to_value(key)
                .map_err(|error| ValidationError::malformed("payload", error.to_string()))?;
            if let Some(key) = key.as_str() {
                self.next = self.parent.field(key);
            }
        }
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        if matches!(self.next, Probe::Member(_)) {
            value.serialize(self.next)?;
        }
        self.next = Probe::Ignore;
        Ok(())
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::REGISTERED_UNIONS;

    /// Remove `//` line comments so a brace inside a comment cannot confuse
    /// the depth count below. Kept separate from discovery so the doc-comment
    /// check further down can scan the *un*-stripped source, where the
    /// comments are exactly what it is looking for.
    fn strip_line_comments(source: &str) -> String {
        source
            .lines()
            .map(|line| line.split("//").next().unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Find every `enum Name { ... }` declaring a plain `Unknown(String)`
    /// variant in comment-stripped source, counting braces so a struct
    /// variant cannot hide a later one. Shared by the registration check and
    /// the consumer-rule doc check below, so both discover unions from one
    /// scan rather than two hand-maintained lists that could drift apart.
    fn discover_unknown_enums(stripped_source: &str) -> BTreeSet<&str> {
        let mut discovered = BTreeSet::new();
        for declaration in stripped_source.split("enum ").skip(1) {
            let (header, body) = declaration.split_once('{').expect("enum has a body");
            let name = header.split_whitespace().next().expect("enum has a name");
            let mut depth = 1;
            let end = body
                .char_indices()
                .find_map(|(index, character)| {
                    match character {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    (depth == 0).then_some(index)
                })
                .expect("enum body closes");
            let body: String = body[..end]
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            if body.contains("Unknown(String)") {
                discovered.insert(name);
            }
        }
        // A scan that goes blind fails open: both callers below only ever
        // check the unions this returns, so finding three of six would leave
        // three unchecked and still pass. `REGISTERED_UNIONS` is generated by
        // the `register_unions!` macro from declarations the probe needs at
        // run time, so it is not a hand-maintained list this could drift
        // from — every entry must therefore also be visible to the scan.
        let unseen: Vec<&str> = REGISTERED_UNIONS
            .iter()
            .copied()
            .filter(|name| !discovered.contains(name))
            .collect();
        assert!(
            unseen.is_empty(),
            "source scan did not find registered unions {}; the scan has gone blind rather than the unions having disappeared",
            unseen.join(", ")
        );
        discovered
    }

    #[test]
    fn every_enum_with_unknown_string_is_registered() {
        // Scan declarations independently of the registration. These enums
        // use plain `enum Name { ... }` syntax; skip line comments and count
        // braces so a struct variant cannot hide a later Unknown(String).
        let stripped = strip_line_comments(include_str!("lib.rs"));
        let discovered = discover_unknown_enums(&stripped);
        assert!(
            !discovered.is_empty(),
            "source scan found no typed Unknown unions"
        );

        // Any exemption must name the enum and explain why it is outside
        // representability validation. Remove it when the enum is registered
        // or disappears; stale exemptions must not silently accumulate.
        const EXEMPTIONS: &[(&str, &str)] = &[];
        for &(name, reason) in EXEMPTIONS {
            assert!(
                !reason.trim().is_empty(),
                "{name}: exemption needs a reason"
            );
            assert!(discovered.contains(name), "{name}: stale exemption");
            assert!(
                !REGISTERED_UNIONS.contains(&name),
                "{name}: registered union no longer needs an exemption"
            );
        }
        let missing: Vec<_> = discovered
            .into_iter()
            .filter(|name| {
                !REGISTERED_UNIONS.contains(name)
                    && !EXEMPTIONS.iter().any(|(exempt, _)| exempt == name)
            })
            .collect();
        assert!(
            missing.is_empty(),
            "enums declaring Unknown(String) missing from union_unknown_validation registration: {}; register each union or name an exemption with a reason",
            missing.join(", ")
        );
    }

    /// Collect the contiguous `///` block immediately above `enum Name {`,
    /// skipping over `#[derive(...)]`-style attribute lines in between.
    /// Joining with a single space makes the marker check below immune to
    /// the comment's own line wraps: a phrase split across two `///` lines
    /// still reads as one contiguous string once collected.
    fn doc_comment_before(source: &str, enum_name: &str) -> String {
        let needle = format!("enum {enum_name} {{");
        let lines: Vec<&str> = source.lines().collect();
        let declaration_line = lines
            .iter()
            .position(|line| !line.trim_start().starts_with("//") && line.contains(&needle))
            .unwrap_or_else(|| panic!("{enum_name}: declaration line not found in lib.rs"));
        let mut collected = Vec::new();
        let mut index = declaration_line;
        while index > 0 {
            index -= 1;
            let line = lines[index].trim();
            if let Some(text) = line.strip_prefix("///") {
                collected.push(text.trim());
            } else if line.starts_with("#[") {
                continue;
            } else {
                break;
            }
        }
        collected.reverse();
        collected.join(" ")
    }

    #[test]
    fn every_retaining_union_states_the_consumer_rule() {
        // The three-clause consumer rule (ruled on onsager-ai/ethogram#12, restated for onsager-ai/ethogram#54):
        // an unknown member is rendered with its raw value, never mapped
        // onto a known member, and, when a consumer must act on it, treated
        // as "not this" rather than a default. This runs against every union
        // discovered by `discover_unknown_enums`, not just the closed ones
        // registered above, so it also covers `ControlAppliedReason`, which
        // stays open.
        //
        // "never as a default" is the marker, not the whole sentence:
        // matching the whole sentence would make this test a formatting
        // assertion that fails the first time someone reflows it. This
        // phrase is the rule's acting clause, appears nowhere else in
        // lib.rs today, and this revision only ever writes it as part of
        // the full three-clause sentence, so its presence stands in for the
        // sentence without pinning that sentence's exact wording.
        const MARKER: &str = "never as a default";

        let raw_source = include_str!("lib.rs");
        let stripped = strip_line_comments(raw_source);
        let discovered = discover_unknown_enums(&stripped);
        assert!(
            !discovered.is_empty(),
            "source scan found no typed Unknown unions"
        );

        let missing: Vec<&str> = discovered
            .into_iter()
            .filter(|name| !doc_comment_before(raw_source, name).contains(MARKER))
            .collect();
        assert!(
            missing.is_empty(),
            "unions missing the consumer rule marker ({MARKER:?}) in their doc comment: {}",
            missing.join(", ")
        );
    }
}
