use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// The largest magnitude at which an integral number round-trips exactly
/// between this SDK and the TypeScript SDK (2^53 − 1, `Number.MAX_SAFE_INTEGER`
/// in JavaScript). Shared by `Event.seq` validation and payload-number
/// validation (issue #9): both reject an out-of-range integral value at parse
/// time rather than rounding it.
const MAX_SAFE_INTEGER_MAGNITUDE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum RunKind {
    #[serde(rename = "loop")]
    Loop,
    #[serde(rename = "handoff")]
    Handoff,
    #[serde(rename = "subagent")]
    Subagent,
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "judgment")]
    Judgment,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum RunOutcome {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "no-op")]
    NoOp,
    #[serde(rename = "timed-out")]
    TimedOut,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "permission-denied")]
    PermissionDenied,
    #[serde(rename = "canceled")]
    Canceled,
}

/// Unknown fields on a payload are never rejected and never dropped (issue
/// #12): a sink that forwards an event it does not fully understand must be
/// byte-preserving, or the stream loses data silently at exactly the
/// boundary this protocol exists to cross. Each payload struct below carries
/// one of these as a `#[serde(flatten)]` field, so a field this SDK does not
/// recognise is captured here on parse and re-emitted on serialisation
/// instead of being silently discarded by ordinary serde struct
/// deserialisation (which ignores unmatched keys once `deny_unknown_fields`
/// is absent).
///
/// This is `serde_json::Map<String, Value>` rather than an `IndexMap`, per
/// the ruling: `serde_json::Map` is already a dependency, and adding
/// `indexmap` for this would be a new dependency for no gain, because
/// `serialise_event` sorts payload keys explicitly regardless of the map's
/// own ordering (see its doc comment). An empty map flattens to zero
/// additional keys, not an empty nested object, so a payload with no unknown
/// fields serialises exactly as it did before this field existed.
pub type PayloadExtension = serde_json::Map<String, Value>;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCeilings {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub cost_usd: Option<f64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub tokens: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub wall_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartedPayload {
    pub kind: RunKind,
    pub actor: String,
    pub harness: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub model: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_run_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_tool_use_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub schedule: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub repository: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub work_order: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub ceilings: Option<RunCeilings>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunUsage {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_tokens: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_tokens: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_read_tokens: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_creation_tokens: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub unit: Option<String>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedPayload {
    pub outcome: RunOutcome,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub cost_usd: Option<f64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<RunUsage>,
    pub duration_ms: f64,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub estimated: Option<bool>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventDraft<P = Value> {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: P,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub captured_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event<P = Value> {
    #[serde(deserialize_with = "deserialize_version")]
    pub v: u32,
    #[serde(rename = "type")]
    pub event_type: String,
    pub run_id: String,
    #[serde(deserialize_with = "deserialize_seq")]
    pub seq: u64,
    pub ts: String,
    pub payload: P,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub captured_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampFields {
    pub run_id: String,
    pub seq: u64,
    pub ts: String,
}

#[must_use]
pub fn stamp<P>(draft: EventDraft<P>, fields: StampFields) -> Event<P> {
    Event {
        v: EVENT_SCHEMA_VERSION,
        event_type: draft.event_type,
        run_id: fields.run_id,
        seq: fields.seq,
        ts: fields.ts,
        payload: draft.payload,
        captured_at: draft.captured_at,
    }
}

/// Parses the open event envelope and validates payloads for event types this
/// SDK knows. Unknown event types deliberately retain the open `Value` payload:
/// both SDKs validate the two `run.*` vocabulary members here without turning
/// the envelope parser into a closed event-type registry.
///
/// A payload's *unknown fields* are a separate axis from its *unknown type*
/// and are tolerated rather than rejected (issue #12): `RunStartedPayload`
/// and `RunFinishedPayload` no longer carry `deny_unknown_fields`, so a field
/// this SDK does not recognise does not fail validation here, and their
/// `#[serde(flatten)]` extension field means a caller who deserialises
/// directly into one of those typed structs (bypassing this function's
/// `Value` payload) still gets it back on re-serialisation rather than
/// silently losing it. Only the envelope stays closed to unknown fields, via
/// the `deny_unknown_fields` still present on `Event` and `EventDraft` below.
pub fn parse_event(input: &str) -> serde_json::Result<Event> {
    let event: Event = serde_json::from_str(input)?;
    validate_payload_numbers(&event.payload, "payload").map_err(de::Error::custom)?;
    validate_known_payload(&event.event_type, &event.payload)?;
    Ok(event)
}

fn validate_known_payload(event_type: &str, payload: &Value) -> serde_json::Result<()> {
    match event_type {
        "run.started" => serde_json::from_value::<RunStartedPayload>(payload.clone()).map(drop),
        "run.finished" => serde_json::from_value::<RunFinishedPayload>(payload.clone()).map(drop),
        _ => Ok(()),
    }
}

/// Serialises an event in its canonical compact form: no presentation
/// whitespace, envelope keys in the order the `Event` struct declares them
/// (`v`, `type`, `runId`, `seq`, `ts`, `payload`, `capturedAt`), and payload
/// object keys sorted recursively by UTF-8 byte order (array order is left
/// alone, but objects nested inside an array are themselves sorted). Both
/// SDKs commit to emitting exactly these bytes for the same event, so the
/// conformance harness diffs producer output directly rather than
/// normalising it first.
///
/// The payload always round-trips through `serde_json::Value` before the
/// envelope is serialised. That round-trip is what makes the sorted-key
/// guarantee hold even when `P` is a typed struct: serialised directly, a
/// struct emits its fields in declaration order and the sort would silently
/// stop applying.
///
/// The keys are then sorted **explicitly**, rather than relying on
/// `serde_json::Map` being a `BTreeMap`. That reliance would have made this
/// SDK's canonical form depend on a Cargo feature it does not control:
/// `preserve_order` backs the map with an insertion-ordered map instead, and
/// Cargo unifies features across a dependency graph, so any consumer enabling
/// it anywhere — umwelt does — would silently turn sorting off here while this
/// repository's own CI, which never enables it, stayed green.
///
/// Numbers are canonicalised before serialisation: any `f64` with a zero
/// fractional part and a magnitude below 2^53 is emitted as an integer, so
/// that `1.0` and `1` produce identical bytes (issue #9). This matches the
/// TypeScript SDK, where `JSON.stringify` already collapses `1.0` to `1`.
///
/// Every remaining `f64` — non-integral values, and integral values at or
/// above 2^53 that the rule above leaves as floats — is laid out in
/// ECMAScript's `Number::toString` notation rather than `serde_json`'s own
/// (issue #9): plain decimal when the value's decimal exponent falls in
/// `[-6, 21)`, exponential otherwise. `serde_json` agrees with JavaScript on
/// which digits to print (both compute the shortest round-tripping decimal),
/// so `float_serialiser` below re-lays those digits rather than
/// recomputing them; see its doc comment for the algorithm.
pub fn serialise_event<P: Serialize>(event: &Event<P>) -> serde_json::Result<String> {
    let payload = canonicalise_numbers(serde_json::to_value(&event.payload)?);
    let canonical = Event {
        v: event.v,
        event_type: event.event_type.clone(),
        run_id: event.run_id.clone(),
        seq: event.seq,
        ts: event.ts.clone(),
        payload,
        captured_at: event.captured_at.clone(),
    };
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, EcmaScriptFormatter);
    serde::Serialize::serialize(&canonical, &mut serializer)?;
    Ok(String::from_utf8(bytes).expect("a JSON serialiser only ever writes valid UTF-8"))
}

/// Recursively validates that every integral-valued number in `value` is
/// within the safe-integer magnitude bound, naming the offending path (for
/// example `payload.nested.count` or `payload.items[2].total`) when the
/// check fails. Non-integral numbers are never bounded, no matter how large
/// their magnitude. Mirrors `deserialize_seq` and reuses the same bound
/// (issue #9): a value that needs more precision must be carried as a string
/// instead of a number.
fn validate_payload_numbers(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                validate_payload_numbers(child, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_payload_numbers(item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::Number(number) => {
            if number_exceeds_safe_integer_magnitude(number) {
                Err(format!(
                    "{path} is an integral number whose magnitude exceeds the safe integer bound of {MAX_SAFE_INTEGER_MAGNITUDE}; a value that needs more precision must be carried as a string"
                ))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// True when `number` is integral-valued (its fractional part, if any, is
/// exactly zero) and its magnitude exceeds `MAX_SAFE_INTEGER_MAGNITUDE`.
///
/// Note that every `f64` at or beyond 2^52 in magnitude is integral by
/// construction — IEEE 754 leaves no mantissa bits for a fractional part at
/// that scale — so this rejects large-magnitude floats such as `1e21` and
/// `f64::MAX` alike; neither is special-cased, per the ruling in issue #9.
fn number_exceeds_safe_integer_magnitude(number: &serde_json::Number) -> bool {
    if let Some(value) = number.as_i64() {
        return value.unsigned_abs() > MAX_SAFE_INTEGER_MAGNITUDE;
    }
    if let Some(value) = number.as_u64() {
        return value > MAX_SAFE_INTEGER_MAGNITUDE;
    }
    if let Some(value) = number.as_f64() {
        return value.fract() == 0.0 && value.abs() > MAX_SAFE_INTEGER_MAGNITUDE as f64;
    }
    false
}

/// The largest magnitude at which every integer is exactly representable as
/// an `f64`, per the ruling in issue #9.
const MAX_SAFE_INTEGRAL_MAGNITUDE: f64 = 9_007_199_254_740_992.0;

/// Recursively rewrites integral-valued floats as integers, per issue #9.
///
/// An `f64` with a zero fractional part and a magnitude below 2^53 is
/// replaced by the equivalent integer `Value`. Every other number —
/// non-integral values, and integral values at or above 2^53 — is left
/// exactly as it was serialised by `serde_json`.
fn canonicalise_numbers(value: Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(canonicalise_numbers).collect())
        }
        Value::Object(values) => {
            // Sort explicitly rather than leaning on `Map` being a `BTreeMap`.
            // With serde_json's `preserve_order` feature the map is
            // insertion-ordered, and Cargo unifies features across the whole
            // dependency graph — so a consumer enabling it would otherwise turn
            // this sort off without touching this crate, and without failing
            // this crate's own CI. Sorting here holds under either backing map.
            let mut entries: Vec<(String, Value)> = values
                .into_iter()
                .map(|(key, child)| (key, canonicalise_numbers(child)))
                .collect();
            entries.sort_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
            Value::Object(entries.into_iter().collect())
        }
        Value::Number(number) => Value::Number(canonicalise_number(number)),
        primitive => primitive,
    }
}

fn canonicalise_number(number: serde_json::Number) -> serde_json::Number {
    if number.is_i64() || number.is_u64() {
        // Already an integer on the wire; nothing to canonicalise.
        return number;
    }

    let Some(as_f64) = number.as_f64() else {
        return number;
    };

    if as_f64.fract() != 0.0 || as_f64.abs() >= MAX_SAFE_INTEGRAL_MAGNITUDE {
        return number;
    }

    // `-0.0 as i64` is `0`, so negative zero canonicalises to `0`, matching
    // JavaScript's `JSON.stringify(-0)`.
    serde_json::Number::from(as_f64 as i64)
}

/// A `serde_json` `Formatter` that re-lays every `f64` it is asked to write
/// into ECMAScript's `Number::toString` notation (issue #9), leaving every
/// other token — strings, booleans, `null`, and the plain integers that
/// `canonicalise_number` already produced — exactly as `serde_json`'s own
/// `CompactFormatter` would write them. `Formatter`'s default methods forward
/// to `CompactFormatter`'s behaviour, so overriding only `write_f64` is
/// enough: the rest of the compact form is untouched.
struct EcmaScriptFormatter;

impl serde_json::ser::Formatter for EcmaScriptFormatter {
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        // `serde_json`'s own float formatter (ryu) already computes the
        // shortest decimal digit string that round-trips to `value` — the
        // same digits JavaScript's formatter would choose. What differs is
        // only the layout: where the two put the decimal point, and when
        // they switch to exponential notation. So the digits are taken
        // as-is from `serde_json`'s text and re-laid, never recomputed.
        let mut default_bytes = Vec::new();
        serde_json::ser::CompactFormatter.write_f64(&mut default_bytes, value)?;
        let default_repr = std::str::from_utf8(&default_bytes)
            .expect("serde_json's float formatter only ever writes ASCII");
        writer.write_all(relay_ecmascript_notation(default_repr).as_bytes())
    }
}

/// Re-lays `serde_json`'s compact `f64` text (for example `"1.5e-5"` or
/// `"9007199254740992.0"`) into the string ECMAScript's `Number::toString`
/// would produce for the same value, per the ECMA-262 `Number::toString`
/// abstract operation (section 6.1.6.1.20 as of ES2023):
///
/// Let the value be written as `s × 10^(n − k)`, where `s` is the `k`-digit
/// integer of shortest-round-trip decimal digits (no leading or trailing
/// zero) and `n` is the position of the decimal point relative to the start
/// of those digits. Then:
///
/// - if `k <= n <= 21`: the `k` digits followed by `n - k` zeroes (plain,
///   no fractional part) — for example `1e20` with `s = 1`, `k = 1`, `n =
///   21` becomes `"1"` followed by twenty zeroes;
/// - else if `0 < n <= 21`: the digits with a decimal point inserted after
///   the `n`th one;
/// - else if `-6 < n <= 0`: `"0."` followed by `-n` zeroes and the digits —
///   this is the plain-decimal band the ruling in issue #9 is about, since
///   `serde_json` switches to exponential one step earlier, at `n = -5`
///   rather than `n = -6`;
/// - otherwise: exponential notation, the first digit, a `.` and the
///   remaining digits when `k > 1`, then `e`, `+` or `-`, and `|n - 1|`.
///
/// `serde_json`'s own text is always sign-optional plain-or-scientific
/// decimal, so `s`, `k` and `n` are recovered by splitting off an optional
/// `-` sign and `e`-exponent, concatenating the integer and fractional
/// digits, and trimming leading and trailing zeroes (adjusting the exponent
/// for each trailing zero trimmed, since removing one divides the digit
/// string's integer value by ten).
fn relay_ecmascript_notation(serialised: &str) -> String {
    let (negative, unsigned) = match serialised.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, serialised),
    };
    let (digits, n) = decompose_decimal(unsigned);
    let digit_count = i64::try_from(digits.len())
        .expect("a finite f64's shortest decimal digit string is nowhere near i64::MAX digits");

    let body = if n >= digit_count && n <= 21 {
        format!("{digits}{}", "0".repeat((n - digit_count) as usize))
    } else if n > 0 && n <= 21 {
        let point = n as usize;
        format!("{}.{}", &digits[..point], &digits[point..])
    } else if n > -6 && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let exponent = n - 1;
        let mantissa = if digit_count == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        let sign = if exponent >= 0 { '+' } else { '-' };
        format!("{mantissa}e{sign}{}", exponent.abs())
    };

    if negative { format!("-{body}") } else { body }
}

/// Decomposes the unsigned decimal text of a finite, non-zero `f64` (as
/// `serde_json` writes it: an optional `e`/`E` exponent over a mantissa that
/// is a plain integer or has a single `.`) into `(digits, n)`, where `digits`
/// is the shortest round-tripping digit string with no leading or trailing
/// zero, and `n` is the position of the decimal point relative to its start
/// — the `s` and `n` of the ECMA-262 `Number::toString` algorithm (`digits`
/// is `s` written out; `k` is `digits.len()`).
fn decompose_decimal(unsigned: &str) -> (String, i64) {
    let (mantissa, exponent_text) = match unsigned.find(['e', 'E']) {
        Some(index) => (&unsigned[..index], &unsigned[index + 1..]),
        None => (unsigned, ""),
    };
    let written_exponent: i64 = if exponent_text.is_empty() {
        0
    } else {
        exponent_text
            .parse()
            .expect("serde_json only ever writes a plain signed integer exponent")
    };
    let (integer_part, fractional_part) = match mantissa.find('.') {
        Some(index) => (&mantissa[..index], &mantissa[index + 1..]),
        None => (mantissa, ""),
    };

    let mut digits = format!("{integer_part}{fractional_part}");
    let mut exponent = written_exponent - fractional_part.len() as i64;

    // Leading zeroes (from an integer part of "0") do not change the value
    // represented, so they are dropped without touching the exponent.
    digits = digits.trim_start_matches('0').to_owned();

    // A trailing zero, by contrast, changes the integer value read from the
    // digit string, so each one dropped must raise the exponent by one to
    // compensate — this only ever fires on the artificial ".0" `serde_json`
    // appends to an integral float, since a genuine shortest round-tripping
    // digit string never ends in zero.
    let without_trailing_zeroes = digits.trim_end_matches('0');
    let trailing_zeroes_dropped = digits.len() - without_trailing_zeroes.len();
    exponent += trailing_zeroes_dropped as i64;
    digits = without_trailing_zeroes.to_owned();

    let digit_count = i64::try_from(digits.len())
        .expect("a finite f64's shortest decimal digit string is nowhere near i64::MAX digits");
    (digits, exponent + digit_count)
}

fn deserialize_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    if version == EVENT_SCHEMA_VERSION {
        Ok(version)
    } else {
        Err(de::Error::custom(format_args!(
            "Event.v must be {EVENT_SCHEMA_VERSION}; received {version}"
        )))
    }
}

fn deserialize_seq<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seq = u64::deserialize(deserializer)?;
    if (1..=MAX_SAFE_INTEGER_MAGNITUDE).contains(&seq) {
        Ok(seq)
    } else {
        Err(de::Error::custom(
            "Event.seq must be a positive safe integer",
        ))
    }
}

fn deserialize_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Deserializes a `u64` and rejects a magnitude beyond
/// `MAX_SAFE_INTEGER_MAGNITUDE`, reusing the same bound and the same
/// `number_exceeds_safe_integer_magnitude` check `validate_payload_numbers`
/// uses (issue #9). `u64` deserialization already rejects a negative or
/// non-integral value by construction, so this adds only the missing upper
/// bound.
///
/// This exists because `parse_event` parses into `Event<Value>` and then
/// runs `validate_payload_numbers` over the whole payload — but a caller who
/// deserialises straight into a typed payload struct, for example
/// `serde_json::from_str::<Event<RunFinishedPayload>>(...)`, never goes
/// through `parse_event` and so never runs that check. `RunCeilings.tokens`,
/// `RunCeilings.wall_ms`, and `RunUsage`'s four token-count fields are the
/// known integral fields on that typed path, so each one is bounded here
/// individually via `deserialize_optional_safe_u64` below.
fn deserialize_safe_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if number_exceeds_safe_integer_magnitude(&serde_json::Number::from(value)) {
        Err(de::Error::custom(format_args!(
            "must be a safe integer no larger than {MAX_SAFE_INTEGER_MAGNITUDE}"
        )))
    } else {
        Ok(value)
    }
}

fn deserialize_optional_safe_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_safe_u64(deserializer).map(Some)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceError {
    pub run_id: String,
    pub expected: u64,
    pub received: u64,
}

impl Display for SequenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "event sequence for run {} must be {}; received {}",
            self.run_id, self.expected, self.received
        )
    }
}

impl Error for SequenceError {}

/// A minimal in-memory reference for sequencing rules, not a storage engine.
pub struct InMemorySink<P = Value> {
    clock: Box<dyn FnMut() -> String>,
    runs: HashMap<String, Vec<Event<P>>>,
}

impl<P> InMemorySink<P> {
    pub fn new(clock: impl FnMut() -> String + 'static) -> Self {
        Self {
            clock: Box::new(clock),
            runs: HashMap::new(),
        }
    }

    pub fn append_draft(&mut self, run_id: impl Into<String>, draft: EventDraft<P>) -> &Event<P> {
        let run_id = run_id.into();
        let seq = self.next_seq(&run_id);
        let event = stamp(
            draft,
            StampFields {
                run_id: run_id.clone(),
                seq,
                ts: (self.clock)(),
            },
        );
        let events = self.runs.entry(run_id).or_default();
        events.push(event);
        events
            .last()
            .expect("the event was inserted immediately before this lookup")
    }

    pub fn append_event(&mut self, event: Event<P>) -> Result<&Event<P>, SequenceError> {
        let expected = self.next_seq(&event.run_id);
        if event.seq != expected {
            return Err(SequenceError {
                run_id: event.run_id,
                expected,
                received: event.seq,
            });
        }

        let events = self.runs.entry(event.run_id.clone()).or_default();
        events.push(event);
        Ok(events
            .last()
            .expect("the event was inserted immediately before this lookup"))
    }

    #[must_use]
    pub fn events(&self, run_id: &str) -> &[Event<P>] {
        self.runs.get(run_id).map_or(&[], Vec::as_slice)
    }

    fn next_seq(&self, run_id: &str) -> u64 {
        self.runs.get(run_id).map_or(1, |events| {
            u64::try_from(events.len()).expect("a run cannot contain more than u64::MAX events") + 1
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use serde_json::{Value, json};

    use super::*;

    const RUN_STARTED_WIRE: &str = r#"{"v":1,"type":"run.started","runId":"run-child","seq":1,"ts":"2026-09-06T10:45:01.000Z","payload":{"actor":"builder","ceilings":{"costUsd":2.5,"tokens":4000,"wallMs":60000},"harness":"codex","kind":"subagent","model":"gpt-5","parentRunId":"run-parent","parentToolUseId":"tool-7","repository":"onsager-ai/ethogram","schedule":"builder@2026-09-06T10:45Z","workOrder":"order-5"},"capturedAt":"2026-09-06T10:45:00.000Z"}"#;

    const RUN_FINISHED_WIRE: &str = r#"{"v":1,"type":"run.finished","runId":"run-child","seq":2,"ts":"2026-09-06T10:45:02.000Z","payload":{"costUsd":1.25,"durationMs":1250,"estimated":true,"outcome":"completed","reason":"placeholder complete","truncated":false,"usage":{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"}}}"#;

    fn complete_event() -> Event {
        Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "test.happened".to_owned(),
            run_id: "run-1".to_owned(),
            seq: 1,
            ts: "2026-09-06T00:00:01.000Z".to_owned(),
            payload: json!({ "ok": true }),
            captured_at: None,
        }
    }

    fn lifecycle_event_input(event_type: &str, payload: Value) -> String {
        serde_json::to_string(&Event {
            event_type: event_type.to_owned(),
            payload,
            ..complete_event()
        })
        .unwrap()
    }

    #[test]
    fn accepts_every_permitted_run_kind() {
        for kind in ["loop", "handoff", "subagent", "session", "judgment"] {
            let input = lifecycle_event_input(
                "run.started",
                json!({ "kind": kind, "actor": "builder", "harness": "codex" }),
            );
            parse_event(&input).unwrap();
        }
    }

    #[test]
    fn rejects_an_unknown_run_kind() {
        let input = lifecycle_event_input(
            "run.started",
            json!({ "kind": "pipeline", "actor": "builder", "harness": "codex" }),
        );

        let error = parse_event(&input).unwrap_err();
        assert!(
            error.to_string().contains("unknown variant `pipeline`"),
            "error was: {error}"
        );
    }

    #[test]
    fn accepts_every_permitted_run_outcome() {
        for outcome in [
            "completed",
            "failed",
            "no-op",
            "timed-out",
            "interrupted",
            "permission-denied",
            "canceled",
        ] {
            let input = lifecycle_event_input(
                "run.finished",
                json!({ "outcome": outcome, "durationMs": 1250 }),
            );
            parse_event(&input).unwrap();
        }
    }

    #[test]
    fn rejects_an_unknown_run_outcome() {
        let input = lifecycle_event_input(
            "run.finished",
            json!({ "outcome": "succeeded", "durationMs": 1250 }),
        );

        let error = parse_event(&input).unwrap_err();
        assert!(
            error.to_string().contains("unknown variant `succeeded`"),
            "error was: {error}"
        );
    }

    #[test]
    fn rejects_each_missing_required_run_started_field() {
        for field in ["kind", "actor", "harness"] {
            let mut payload = json!({
                "kind": "subagent",
                "actor": "builder",
                "harness": "codex"
            });
            payload.as_object_mut().unwrap().remove(field);
            let input = lifecycle_event_input("run.started", payload);

            let error = parse_event(&input).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "error for {field} was: {error}"
            );
        }
    }

    #[test]
    fn rejects_each_missing_required_run_finished_field() {
        for field in ["outcome", "durationMs"] {
            let mut payload = json!({ "outcome": "completed", "durationMs": 1250 });
            payload.as_object_mut().unwrap().remove(field);
            let input = lifecycle_event_input("run.finished", payload);

            let error = parse_event(&input).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "error for {field} was: {error}"
            );
        }
    }

    #[test]
    fn unknown_event_types_keep_their_open_payload() {
        assert!(
            parse_event(&lifecycle_event_input(
                "future.happened",
                json!({ "anything": [true, null, "goes"] })
            ))
            .is_ok()
        );
    }

    #[test]
    fn run_lifecycle_events_match_the_typescript_pinned_bytes() {
        // Both payload structs deliberately declare fields in protocol-table
        // order rather than alphabetically. These assertions therefore also
        // prove that typed payloads still take the canonical sorted-key path.
        let started = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "run.started".to_owned(),
            run_id: "run-child".to_owned(),
            seq: 1,
            ts: "2026-09-06T10:45:01.000Z".to_owned(),
            payload: RunStartedPayload {
                kind: RunKind::Subagent,
                actor: "builder".to_owned(),
                harness: "codex".to_owned(),
                model: Some("gpt-5".to_owned()),
                parent_run_id: Some("run-parent".to_owned()),
                parent_tool_use_id: Some("tool-7".to_owned()),
                schedule: Some("builder@2026-09-06T10:45Z".to_owned()),
                repository: Some("onsager-ai/ethogram".to_owned()),
                work_order: Some("order-5".to_owned()),
                ceilings: Some(RunCeilings {
                    cost_usd: Some(2.5),
                    tokens: Some(4000),
                    wall_ms: Some(60000),
                    extra: PayloadExtension::new(),
                }),
                extra: PayloadExtension::new(),
            },
            captured_at: Some("2026-09-06T10:45:00.000Z".to_owned()),
        };
        let finished = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "run.finished".to_owned(),
            run_id: "run-child".to_owned(),
            seq: 2,
            ts: "2026-09-06T10:45:02.000Z".to_owned(),
            payload: RunFinishedPayload {
                outcome: RunOutcome::Completed,
                reason: Some("placeholder complete".to_owned()),
                truncated: Some(false),
                cost_usd: Some(1.25),
                usage: Some(RunUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(40),
                    cache_read_tokens: Some(20),
                    cache_creation_tokens: Some(30),
                    unit: Some("weighted-tokens".to_owned()),
                    extra: PayloadExtension::new(),
                }),
                duration_ms: 1250.0,
                estimated: Some(true),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(serialise_event(&started).unwrap(), RUN_STARTED_WIRE);
        assert_eq!(serialise_event(&finished).unwrap(), RUN_FINISHED_WIRE);
        assert!(RUN_FINISHED_WIRE.contains(
            r#""usage":{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"}"#
        ));
    }

    #[test]
    fn absent_optional_run_payload_fields_are_omitted_instead_of_null() {
        let started = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "run.started".to_owned(),
            run_id: "run-root".to_owned(),
            seq: 1,
            ts: "2026-09-06T00:00:00.000Z".to_owned(),
            payload: RunStartedPayload {
                kind: RunKind::Session,
                actor: "user".to_owned(),
                harness: "codex".to_owned(),
                model: None,
                parent_run_id: None,
                parent_tool_use_id: None,
                schedule: None,
                repository: None,
                work_order: None,
                ceilings: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let finished = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "run.finished".to_owned(),
            run_id: "run-root".to_owned(),
            seq: 2,
            ts: "2026-09-06T00:00:01.000Z".to_owned(),
            payload: RunFinishedPayload {
                outcome: RunOutcome::NoOp,
                reason: None,
                truncated: None,
                cost_usd: None,
                usage: None,
                duration_ms: 1000.0,
                estimated: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(
            serialise_event(&started).unwrap(),
            r#"{"v":1,"type":"run.started","runId":"run-root","seq":1,"ts":"2026-09-06T00:00:00.000Z","payload":{"actor":"user","harness":"codex","kind":"session"}}"#
        );
        assert_eq!(
            serialise_event(&finished).unwrap(),
            r#"{"v":1,"type":"run.finished","runId":"run-root","seq":2,"ts":"2026-09-06T00:00:01.000Z","payload":{"durationMs":1000,"outcome":"no-op"}}"#
        );
    }

    #[test]
    fn rejects_each_missing_required_envelope_field() {
        for field in ["v", "type", "runId", "seq", "ts", "payload"] {
            let mut candidate = serde_json::to_value(complete_event()).unwrap();
            candidate.as_object_mut().unwrap().remove(field);
            let error = parse_event(&candidate.to_string()).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "error for {field} was: {error}"
            );
        }
    }

    #[test]
    fn rejects_unknown_envelope_fields() {
        let mut candidate = serde_json::to_value(complete_event()).unwrap();
        candidate
            .as_object_mut()
            .unwrap()
            .insert("stage".to_owned(), json!("build"));

        let error = parse_event(&candidate.to_string()).unwrap_err();
        assert!(error.to_string().contains("unknown field `stage`"));
    }

    #[test]
    fn rejects_null_for_optional_captured_at_field() {
        let mut candidate = serde_json::to_value(complete_event()).unwrap();
        candidate
            .as_object_mut()
            .unwrap()
            .insert("capturedAt".to_owned(), Value::Null);

        let error = parse_event(&candidate.to_string()).unwrap_err();
        assert!(error.to_string().contains("expected a string"));
    }

    #[test]
    fn stamp_sets_version_and_preserves_captured_at() {
        let captured_at = "2026-09-06T00:00:00.000Z".to_owned();
        let event = stamp(
            EventDraft {
                event_type: "test.happened".to_owned(),
                payload: Value::Null,
                captured_at: Some(captured_at.clone()),
            },
            StampFields {
                run_id: "run-1".to_owned(),
                seq: 1,
                ts: "2026-09-06T00:00:01.000Z".to_owned(),
            },
        );

        assert_eq!(event.v, EVENT_SCHEMA_VERSION);
        assert_eq!(event.captured_at, Some(captured_at));
    }

    #[test]
    fn a_producer_cannot_override_the_version() {
        let error = serde_json::from_value::<EventDraft>(json!({
            "v": 99,
            "type": "test.happened",
            "payload": null
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `v`"));
    }

    #[test]
    fn omitted_captured_at_is_not_serialised_as_null() {
        let event = stamp(
            EventDraft {
                event_type: "test.happened".to_owned(),
                payload: json!({ "ok": true }),
                captured_at: None,
            },
            StampFields {
                run_id: "run-1".to_owned(),
                seq: 1,
                ts: "2026-09-06T00:00:01.000Z".to_owned(),
            },
        );
        let serialised = serialise_event(&event).unwrap();

        assert!(!serialised.contains("capturedAt"));
        assert!(!serialised.contains(":null"));
    }

    #[test]
    fn compact_serialiser_emits_no_presentation_whitespace() {
        // Envelope keys come back in the `Event` struct's declaration order —
        // that order is part of the canonical form (see the doc comment on
        // `serialise_event`) — while the payload, being canonicalised through
        // `serde_json::Value`, comes out with its keys sorted.
        assert_eq!(
            serialise_event(&complete_event()).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"ok":true}}"#
        );
    }

    #[test]
    fn integral_float_and_integer_payloads_serialise_identically() {
        let float_event = Event {
            payload: json!({ "count": 1.0 }),
            ..complete_event()
        };
        let integer_event = Event {
            payload: json!({ "count": 1 }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&float_event).unwrap(),
            serialise_event(&integer_event).unwrap()
        );
    }

    #[test]
    fn integral_floats_canonicalise_to_integers() {
        let event = Event {
            payload: json!({ "one": 1.0, "hundred": 100.0, "writtenAsExponent": 1e2 }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"hundred":100,"one":1,"writtenAsExponent":100}}"#
        );
    }

    #[test]
    fn non_integral_values_outside_the_divergent_band_are_unchanged() {
        // `0.1` and `1e-7` already sit outside the `[1e-6, 1e-5)` band where
        // `serde_json` and ECMAScript disagree on notation (issue #9), so
        // relaying them through `relay_ecmascript_notation` reproduces
        // `serde_json`'s own bytes rather than changing them.
        let event = Event {
            payload: json!({ "tenth": 0.1, "tiny": 1e-7 }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"tenth":0.1,"tiny":1e-7}}"#
        );
    }

    #[test]
    fn nested_and_array_integral_floats_are_canonicalised() {
        let event = Event {
            payload: json!({
                "nested": { "value": 2.0 },
                "list": [3.0, 4.5, 5.0]
            }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"list":[3,4.5,5],"nested":{"value":2}}}"#
        );
    }

    #[test]
    fn negative_zero_serialises_as_zero() {
        let event = Event {
            payload: json!({ "value": -0.0 }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"value":0}}"#
        );
    }

    #[test]
    fn integral_values_at_or_above_the_safe_magnitude_still_lose_their_decimal_point() {
        let event = Event {
            payload: json!({ "value": 9_007_199_254_740_992.0_f64 }),
            ..complete_event()
        };

        // The 2^53 bound in the ruling of issue #9 governs only whether a
        // float collapses to a wire integer, deliberately left unchanged
        // here: at and beyond 2^53 the value stays a float. But it is still
        // a whole number, and ECMAScript's notation rule (also issue #9)
        // gives every whole number in the plain-decimal band no decimal
        // point regardless of how it is represented internally, so this now
        // matches `(9007199254740992).toString()` in JavaScript instead of
        // carrying the `.0` `serde_json` used to append. This event is built
        // and serialised directly rather than round-tripped through
        // `parse_event`, so it is exercising `serialise_event`'s
        // canonicalisation, not the parse-time bound in
        // `validate_payload_numbers` (covered separately below).
        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"value":9007199254740992}}"#
        );
    }

    #[test]
    fn ecmascript_notation_matches_measured_javascript_output_at_the_band_edges_and_beyond() {
        // Every expected string here was measured, not derived from belief
        // about the ECMA-262 algorithm: each is the exact output of
        // `JSON.stringify(JSON.parse(JSON.stringify(<input>)))` in Node 24.
        // The band edges are the ones the ruling in issue #9 names
        // (`9.99e-7`, `1e-6`, `2.5e-6`, `1e-5`, `1.5e-5`, `1e20`, `1e21`);
        // the rest exercise a plain fraction, a small fraction outside the
        // band, and the extremes of `f64`'s exponent range, each with its
        // negative counterpart.
        let cases: &[(f64, &str)] = &[
            // -- 9.99e-7: last value serde_json and ECMAScript still agree
            //    on below the band; both already choose exponential here.
            (9.99e-7, "9.99e-7"),
            (-9.99e-7, "-9.99e-7"),
            // -- 1e-6: the band's lower edge. serde_json writes "1e-6";
            //    ECMAScript's plain-decimal band starts here (n = -5).
            (1e-6, "0.000001"),
            (-1e-6, "-0.000001"),
            // -- 2.5e-6: inside the band, same disagreement as 1e-6.
            (2.5e-6, "0.0000025"),
            (-2.5e-6, "-0.0000025"),
            // -- 1e-5: the band's upper edge; both sides already agree
            //    ("0.00001"), which this pins so a regression is visible.
            (1e-5, "0.00001"),
            (-1e-5, "-0.00001"),
            // -- 1.5e-5: just above the band, both sides already agree.
            (1.5e-5, "0.000015"),
            (-1.5e-5, "-0.000015"),
            // -- 1e20: the top of the plain-decimal band (n = 21).
            (1e20, "100000000000000000000"),
            (-1e20, "-100000000000000000000"),
            // -- 1e21: one step past the plain-decimal band (n = 22).
            (1e21, "1e+21"),
            (-1e21, "-1e+21"),
            // -- An ordinary fraction and an integral float well inside the
            //    plain-decimal band, as a sanity check.
            (0.1, "0.1"),
            (-0.1, "-0.1"),
            (1.5, "1.5"),
            (-1.5, "-1.5"),
            (0.00012345, "0.00012345"),
            (-0.00012345, "-0.00012345"),
            // -- Small-magnitude values already on the exponential side.
            (1e-7, "1e-7"),
            (-1e-7, "-1e-7"),
            (1.23e-7, "1.23e-7"),
            (-1.23e-7, "-1.23e-7"),
            (1e-21, "1e-21"),
            (-1e-21, "-1e-21"),
            // -- f64's extremes: the smallest subnormal and the largest
            //    finite value, both single- and multi-digit mantissas.
            (5e-324, "5e-324"),
            (-5e-324, "-5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (-f64::MAX, "-1.7976931348623157e+308"),
        ];

        for (input, expected) in cases {
            let event = Event {
                payload: json!({ "value": *input }),
                ..complete_event()
            };

            assert_eq!(
                serialise_event(&event).unwrap(),
                format!(
                    r#"{{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{{"value":{expected}}}}}"#
                ),
                "input {input:?} expected notation {expected}"
            );
        }
    }

    #[test]
    fn payload_keys_sort_by_utf8_bytes_even_for_a_plain_object_literal() {
        // This guards against a future dependency change flipping on
        // serde_json's `preserve_order` feature: if that ever happens, this
        // scrambled-order payload would come back in insertion order instead
        // of sorted, and this assertion would fail loudly.
        let event = Event {
            payload: json!({ "zebra": 1, "mango": 2, "apple": 3 }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"apple":3,"mango":2,"zebra":1}}"#
        );
    }

    #[test]
    fn typed_payload_struct_fields_are_sorted_despite_declaration_order() {
        // `TypedPayload` declares `zebra` before `apple`. If `serialise_event`
        // ever serialised the payload directly instead of round-tripping it
        // through `serde_json::Value` first, serde would emit fields in this
        // declaration order and this assertion would fail — that is exactly
        // the latent bug the round-trip exists to prevent.
        #[derive(Serialize)]
        struct TypedPayload {
            zebra: bool,
            apple: u32,
        }

        let event = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "test.happened".to_owned(),
            run_id: "run-1".to_owned(),
            seq: 1,
            ts: "2026-09-06T00:00:01.000Z".to_owned(),
            payload: TypedPayload {
                zebra: true,
                apple: 1,
            },
            captured_at: None,
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"apple":1,"zebra":true}}"#
        );
    }

    #[test]
    fn accepts_an_integral_payload_number_at_the_safe_bound() {
        let candidate = serde_json::to_value(Event {
            payload: json!({ "value": 9_007_199_254_740_991_i64 }),
            ..complete_event()
        })
        .unwrap();

        assert!(parse_event(&candidate.to_string()).is_ok());
    }

    #[test]
    fn rejects_a_top_level_integral_payload_number_beyond_the_safe_bound() {
        let candidate = serde_json::to_value(Event {
            payload: json!(9_007_199_254_740_992_i64),
            ..complete_event()
        })
        .unwrap();

        let error = parse_event(&candidate.to_string()).unwrap_err();
        assert!(error.to_string().contains("payload"), "error was: {error}");
    }

    #[test]
    fn rejects_an_out_of_range_integral_number_at_a_nested_path() {
        let candidate = serde_json::to_value(Event {
            payload: json!({ "nested": { "big": 1e21 } }),
            ..complete_event()
        })
        .unwrap();

        let error = parse_event(&candidate.to_string()).unwrap_err();
        assert!(
            error.to_string().contains("payload.nested.big"),
            "error was: {error}"
        );
    }

    #[test]
    fn rejects_an_out_of_range_integral_number_inside_an_array_of_objects() {
        let candidate = serde_json::to_value(Event {
            payload: json!({ "items": [{ "ok": true }, { "total": 1e21 }] }),
            ..complete_event()
        })
        .unwrap();

        let error = parse_event(&candidate.to_string()).unwrap_err();
        assert!(
            error.to_string().contains("payload.items[1].total"),
            "error was: {error}"
        );
    }

    #[test]
    fn non_integral_payload_numbers_are_never_bounded() {
        let candidate = serde_json::to_value(Event {
            payload: json!({ "value": 0.1 }),
            ..complete_event()
        })
        .unwrap();

        assert!(parse_event(&candidate.to_string()).is_ok());
    }

    #[test]
    fn rejects_1e21_matching_the_ruling_example_in_the_review() {
        // `1e21` is integral-valued (its fractional part is exactly zero) and
        // its magnitude exceeds the bound, so it is rejected. This is the
        // example named explicitly in the follow-up brief: it is deliberately
        // not special-cased.
        let candidate = serde_json::to_value(Event {
            payload: json!({ "value": 1e21 }),
            ..complete_event()
        })
        .unwrap();

        assert!(parse_event(&candidate.to_string()).is_err());
    }

    #[test]
    fn extremely_large_integral_floats_are_rejected_regardless_of_magnitude() {
        // `f64::MAX` (1.7976931348623157e308) is, like every `f64` at or
        // beyond 2^52 in magnitude, integral by construction: IEEE 754 leaves
        // no mantissa bits for a fractional part at that scale, so its
        // fractional part is exactly zero in both Rust (`f64::fract`) and
        // JavaScript (`Number.isInteger` returns `true` for it). Contrary to
        // a claim in an earlier draft of this change, it is therefore *not*
        // exempt from the bound — exempting it would itself be the kind of
        // special case the ruling in issue #9 rules out for `1e21`.
        let candidate = serde_json::to_value(Event {
            payload: json!({ "value": f64::MAX }),
            ..complete_event()
        })
        .unwrap();

        assert!(parse_event(&candidate.to_string()).is_err());
    }

    #[test]
    fn sink_stamps_drafts_with_a_gapless_sequence_and_its_clock() {
        let mut timestamps = VecDeque::from([
            "2026-09-06T00:00:01.000Z".to_owned(),
            "2026-09-06T00:00:02.000Z".to_owned(),
        ]);
        let mut sink = InMemorySink::new(move || timestamps.pop_front().unwrap());

        let first = sink.append_draft(
            "run-1",
            EventDraft {
                event_type: "test.happened".to_owned(),
                payload: json!(1),
                captured_at: None,
            },
        );
        let first_stamp = (first.seq, first.ts.clone());
        let second = sink.append_draft(
            "run-1",
            EventDraft {
                event_type: "test.happened".to_owned(),
                payload: json!(2),
                captured_at: None,
            },
        );

        assert_eq!(first_stamp, (1, "2026-09-06T00:00:01.000Z".to_owned()));
        assert_eq!(second.seq, 2);
        assert_eq!(second.ts, "2026-09-06T00:00:02.000Z");
    }

    #[test]
    fn sink_preserves_shipped_events_and_rejects_a_gap() {
        let mut sink = InMemorySink::new(|| "unused".to_owned());
        let first = complete_event();
        let stored = sink.append_event(first.clone()).unwrap();

        assert_eq!(stored, &first);

        let error = sink
            .append_event(Event {
                seq: 3,
                ..complete_event()
            })
            .unwrap_err();
        assert_eq!(error.expected, 2);
        assert_eq!(error.received, 3);
        assert_eq!(sink.events("run-1").len(), 1);
    }

    /// Builds an `Event<RunStartedPayload>` around a hand-written payload
    /// JSON body, going through the typed struct (not `Value`) so these
    /// tests exercise the `#[serde(flatten)]` extension field a caller using
    /// the typed API directly would rely on for retention (issue #12), not
    /// just the untyped `Event<Value>` path `parse_event` returns.
    fn typed_run_started_event(payload_json: &str) -> Event<RunStartedPayload> {
        Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "run.started".to_owned(),
            run_id: "run-1".to_owned(),
            seq: 1,
            ts: "2026-09-06T00:00:01.000Z".to_owned(),
            payload: serde_json::from_str::<RunStartedPayload>(payload_json).unwrap(),
            captured_at: None,
        }
    }

    #[test]
    fn unknown_payload_field_round_trips_across_the_sort_boundary() {
        // "0alpha" sorts before the known key "actor"; "zzzTail" sorts after
        // the known key "kind". Both unknown fields must survive parse and
        // reappear in the canonical sorted position (issue #12).
        let input = r#"{"0alpha":"before-actor","actor":"builder","harness":"codex","kind":"loop","zzzTail":"after-kind"}"#;

        assert_eq!(
            serialise_event(&typed_run_started_event(input)).unwrap(),
            r#"{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"0alpha":"before-actor","actor":"builder","harness":"codex","kind":"loop","zzzTail":"after-kind"}}"#
        );
    }

    #[test]
    fn unknown_payload_field_holding_nested_object_and_array_is_preserved_and_sorted() {
        let input = r#"{"kind":"loop","actor":"builder","harness":"codex","nested":{"zebra":1,"apple":2},"list":[{"zebra":1,"apple":2},3,"text"]}"#;

        assert_eq!(
            serialise_event(&typed_run_started_event(input)).unwrap(),
            r#"{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","harness":"codex","kind":"loop","list":[{"apple":2,"zebra":1},3,"text"],"nested":{"apple":2,"zebra":1}}}"#
        );
    }

    #[test]
    fn unknown_payload_numbers_round_trip_byte_identically() {
        // The hazard named in the follow-up brief: `#[serde(flatten)]` routes
        // deserialised values through serde's internal buffering layer, and
        // that layer is known in some cases to change how a number is
        // represented. Measured here: it does not, for any of the four
        // shapes this protocol's number canonicalisation cares about
        // (issue #9) — a large integer just inside the safe bound, a small
        // integer, an integral-valued float, and a non-integral value. Each
        // keeps its own wire representation (or, for the integral float,
        // takes the same integer form a *known* integral field would) rather
        // than drifting into a different one.
        let input = r#"{"kind":"loop","actor":"builder","harness":"codex","bigInt":9007199254740991,"smallInt":1,"integralFloat":2.0,"fraction":0.000001}"#;

        assert_eq!(
            serialise_event(&typed_run_started_event(input)).unwrap(),
            r#"{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","bigInt":9007199254740991,"fraction":0.000001,"harness":"codex","integralFloat":2,"kind":"loop","smallInt":1}}"#
        );
    }

    #[test]
    fn payload_without_unknown_fields_serialises_exactly_as_before() {
        // The extension field must not surface as an empty object when there
        // is nothing unknown to carry (issue #12).
        let input = r#"{"kind":"loop","actor":"builder","harness":"codex"}"#;

        let serialised = serialise_event(&typed_run_started_event(input)).unwrap();
        assert_eq!(
            serialised,
            r#"{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","harness":"codex","kind":"loop"}}"#
        );
        assert!(!serialised.contains("extra"));
    }

    /// Cross-SDK byte identity for an event with an unknown payload field
    /// (issue #12). This exact literal is also hand-built in the TypeScript
    /// suite (`index.test.ts`, "pins byte-identical bytes for an unknown
    /// payload field with Rust") and asserted there against the same string.
    const UNKNOWN_PAYLOAD_FIELD_WIRE: &str = r#"{"v":1,"type":"run.started","runId":"run-cross","seq":1,"ts":"2026-09-07T00:00:00.000Z","payload":{"0alpha":"before-actor","actor":"builder","harness":"codex","kind":"loop","list":[{"apple":2,"zebra":1},3,"text"],"nested":{"apple":2,"zebra":1},"zzzTail":"after-kind"}}"#;

    #[test]
    fn unknown_payload_field_matches_the_typescript_pinned_bytes() {
        let input = r#"{"0alpha":"before-actor","actor":"builder","harness":"codex","kind":"loop","list":[{"zebra":1,"apple":2},3,"text"],"nested":{"zebra":1,"apple":2},"zzzTail":"after-kind"}"#;
        let event = Event {
            run_id: "run-cross".to_owned(),
            ts: "2026-09-07T00:00:00.000Z".to_owned(),
            ..typed_run_started_event(input)
        };

        assert_eq!(serialise_event(&event).unwrap(), UNKNOWN_PAYLOAD_FIELD_WIRE);
    }

    // -- Whole-number usage/ceilings counts (review follow-up) ----------

    #[test]
    fn rejects_a_non_integer_usage_token_count() {
        // `u64` deserialization rejects a non-integral value by construction;
        // this test pins that behaviour rather than assuming it.
        assert!(serde_json::from_str::<RunUsage>(r#"{"inputTokens":10.5}"#).is_err());
    }

    #[test]
    fn rejects_a_non_integer_ceilings_count() {
        assert!(serde_json::from_str::<RunCeilings>(r#"{"tokens":10.5}"#).is_err());
    }

    #[test]
    fn rejects_a_negative_usage_token_count() {
        // `u64` deserialization rejects a negative value by construction;
        // this test pins that behaviour rather than assuming it.
        assert!(serde_json::from_str::<RunUsage>(r#"{"outputTokens":-5}"#).is_err());
    }

    #[test]
    fn rejects_a_negative_ceilings_count() {
        assert!(serde_json::from_str::<RunCeilings>(r#"{"wallMs":-5}"#).is_err());
    }

    /// The typed-struct-path bound check the follow-up brief calls for:
    /// `parse_event` runs `validate_payload_numbers` over the whole payload,
    /// but a caller who deserialises straight into `Event<RunFinishedPayload>`
    /// (bypassing `parse_event` entirely) relies instead on the
    /// `deserialize_optional_safe_u64` each of these six fields now carries.
    #[test]
    fn typed_run_finished_payload_deserialization_rejects_a_usage_count_beyond_the_safe_bound() {
        let input = format!(
            r#"{{"v":1,"type":"run.finished","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{{"outcome":"completed","durationMs":1250,"usage":{{"inputTokens":{}}}}}}}"#,
            MAX_SAFE_INTEGER_MAGNITUDE + 1
        );

        let error = serde_json::from_str::<Event<RunFinishedPayload>>(&input).unwrap_err();
        assert!(
            error.to_string().contains("safe integer"),
            "error was: {error}"
        );
    }

    #[test]
    fn typed_run_finished_payload_deserialization_accepts_a_usage_count_at_the_safe_bound() {
        let input = format!(
            r#"{{"v":1,"type":"run.finished","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{{"outcome":"completed","durationMs":1250,"usage":{{"inputTokens":{}}}}}}}"#,
            MAX_SAFE_INTEGER_MAGNITUDE
        );

        assert!(serde_json::from_str::<Event<RunFinishedPayload>>(&input).is_ok());
    }

    /// Same bound, exercised on `RunStartedPayload.ceilings` rather than
    /// `RunFinishedPayload.usage`, so all six fields are covered on the typed
    /// path rather than just the one the brief names explicitly.
    #[test]
    fn typed_run_started_payload_deserialization_rejects_a_ceilings_count_beyond_the_safe_bound() {
        let input = format!(
            r#"{{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{{"kind":"loop","actor":"builder","harness":"codex","ceilings":{{"tokens":{}}}}}}}"#,
            MAX_SAFE_INTEGER_MAGNITUDE + 1
        );

        let error = serde_json::from_str::<Event<RunStartedPayload>>(&input).unwrap_err();
        assert!(
            error.to_string().contains("safe integer"),
            "error was: {error}"
        );
    }

    #[test]
    fn typed_run_started_payload_deserialization_accepts_a_ceilings_count_at_the_safe_bound() {
        let input = format!(
            r#"{{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{{"kind":"loop","actor":"builder","harness":"codex","ceilings":{{"tokens":{}}}}}}}"#,
            MAX_SAFE_INTEGER_MAGNITUDE
        );

        assert!(serde_json::from_str::<Event<RunStartedPayload>>(&input).is_ok());
    }
}
