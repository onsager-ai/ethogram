use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

pub const EVENT_SCHEMA_VERSION: u32 = 1;
const MAX_SAFE_SEQUENCE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventDraft<P = Value> {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: P,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string",
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
        deserialize_with = "deserialize_optional_string",
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

pub fn parse_event(input: &str) -> serde_json::Result<Event> {
    serde_json::from_str(input)
}

/// Serialises an event as compact JSON, without presentation whitespace.
///
/// Key ordering is not part of the contract because JSON objects are
/// unordered. The conformance harness exercises this function, then compares
/// recursively key-sorted forms rather than incidental object order.
///
/// Numbers are canonicalised before serialisation: any `f64` with a zero
/// fractional part and a magnitude below 2^53 is emitted as an integer, so
/// that `1.0` and `1` produce identical bytes (issue #9). This matches the
/// TypeScript SDK, where `JSON.stringify` already collapses `1.0` to `1`.
pub fn serialise_event<P: Serialize>(event: &Event<P>) -> serde_json::Result<String> {
    let value = serde_json::to_value(event)?;
    let canonical = canonicalise_numbers(value);
    serde_json::to_string(&canonical)
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
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, child)| (key, canonicalise_numbers(child)))
                .collect(),
        ),
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
    if (1..=MAX_SAFE_SEQUENCE).contains(&seq) {
        Ok(seq)
    } else {
        Err(de::Error::custom(
            "Event.seq must be a positive safe integer",
        ))
    }
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
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
        // Object keys come back sorted, because canonicalisation round-trips
        // through `serde_json::Value`, whose map is a `BTreeMap` in this
        // workspace. Key order was never part of the contract (see the
        // doc comment on `serialise_event`), so this asserts today's order
        // rather than the declaration order of the `Event` struct.
        assert_eq!(
            serialise_event(&complete_event()).unwrap(),
            r#"{"payload":{"ok":true},"runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","type":"test.happened","v":1}"#
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
            r#"{"payload":{"hundred":100,"one":1,"writtenAsExponent":100},"runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","type":"test.happened","v":1}"#
        );
    }

    #[test]
    fn non_integral_values_are_left_untouched() {
        let event = Event {
            payload: json!({ "tenth": 0.1, "tiny": 1e-7 }),
            ..complete_event()
        };

        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"payload":{"tenth":0.1,"tiny":1e-7},"runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","type":"test.happened","v":1}"#
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
            r#"{"payload":{"list":[3,4.5,5],"nested":{"value":2}},"runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","type":"test.happened","v":1}"#
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
            r#"{"payload":{"value":0},"runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","type":"test.happened","v":1}"#
        );
    }

    #[test]
    fn integral_values_at_or_above_the_safe_magnitude_keep_their_current_representation() {
        let event = Event {
            payload: json!({ "value": 9_007_199_254_740_992.0_f64 }),
            ..complete_event()
        };

        // This asserts today's behaviour at and beyond the 2^53 boundary,
        // which the ruling in issue #9 deliberately leaves untouched, so a
        // future change to it is visible here rather than silent.
        assert_eq!(
            serialise_event(&event).unwrap(),
            r#"{"payload":{"value":9007199254740992.0},"runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","type":"test.happened","v":1}"#
        );
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
}
