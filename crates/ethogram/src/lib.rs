use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;

pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Maximum number of Unicode scalar values carried by an `agent.text`.
pub const MAX_TEXT_SCALARS: usize = 16_384;

/// Maximum scalars carried by any excerpted field other than `agent.text`.
pub const MAX_EXCERPT_SCALARS: usize = 4_096;

/// The wire string for a `run.started` event's `type` field.
pub const RUN_STARTED: &str = "run.started";
/// The wire string for a `run.finished` event's `type` field.
pub const RUN_FINISHED: &str = "run.finished";
/// The wire string for an `agent.started` event's `type` field.
pub const AGENT_STARTED: &str = "agent.started";
/// The wire string for an `agent.text` event's `type` field.
pub const AGENT_TEXT: &str = "agent.text";
/// The wire string for an `agent.tool_use` event's `type` field.
pub const AGENT_TOOL_USE: &str = "agent.tool_use";
/// The wire string for an `agent.tool_result` event's `type` field.
pub const AGENT_TOOL_RESULT: &str = "agent.tool_result";
/// The wire string for an `agent.completed` event's `type` field.
pub const AGENT_COMPLETED: &str = "agent.completed";
/// The wire string for an `agent.warning` event's `type` field.
pub const AGENT_WARNING: &str = "agent.warning";

/// Every event `type` this SDK has a typed payload for. This is not a closed
/// vocabulary: `parse_event` still accepts a type it has never heard of (see
/// `check_known_payload_representation`'s fallthrough), and a consumer may
/// still match a literal for vocabulary this SDK has not learned. A constant
/// is a name for a string, not a gate.
pub const KNOWN_TYPES: [&str; 8] = [
    RUN_STARTED,
    RUN_FINISHED,
    AGENT_STARTED,
    AGENT_TEXT,
    AGENT_TOOL_USE,
    AGENT_TOOL_RESULT,
    AGENT_COMPLETED,
    AGENT_WARNING,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excerpt {
    pub text: String,
    pub truncated: bool,
}

/// Keeps at most `max` Unicode scalar values from `text`, cutting on a code
/// point boundary. This deliberately does not attempt grapheme-cluster
/// segmentation.
///
/// Unlike the TypeScript SDK's `excerpt()`, this never needs to replace a
/// lone surrogate with `U+FFFD` (issue #6): a Rust `&str` is guaranteed
/// well-formed UTF-8 and so cannot hold an unpaired surrogate code unit in
/// the first place — there is nothing here for that rule to act on. The
/// asymmetry exists because a lone surrogate is representable in a
/// JavaScript string (which is UTF-16 and does not enforce well-formedness)
/// and not in Rust's `String`; leaving it intact on the TypeScript side would
/// let a producer build an `agent.text` or excerpt that one SDK can hold and
/// the other cannot even parse.
#[must_use]
pub fn excerpt(text: &str, max: usize) -> Excerpt {
    Excerpt {
        text: text.chars().take(max).collect(),
        truncated: text.chars().count() > max,
    }
}

/// The largest magnitude at which an integral number round-trips exactly
/// between this SDK and the TypeScript SDK (2^53 − 1, `Number.MAX_SAFE_INTEGER`
/// in JavaScript). Shared by `Event.seq` validation and payload-number
/// validation (issue #9): both reject an out-of-range integral value at parse
/// time rather than rounding it.
const MAX_SAFE_INTEGER_MAGNITUDE: u64 = 9_007_199_254_740_991;

/// A run kind this SDK knows, or an unfamiliar wire string retained verbatim
/// in `Unknown`. Consumers must handle `Unknown` explicitly and must never map
/// it onto a known kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunKind {
    Loop,
    Handoff,
    Subagent,
    Session,
    Judgment,
    /// A long-lived process that observes other runs and emits on its own run.
    Relay,
    /// An unfamiliar member, retained exactly as it appeared on the wire.
    Unknown(String),
}

impl RunKind {
    /// Returns the exact wire string, including an unfamiliar value verbatim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Loop => "loop",
            Self::Handoff => "handoff",
            Self::Subagent => "subagent",
            Self::Session => "session",
            Self::Judgment => "judgment",
            Self::Relay => "relay",
            Self::Unknown(value) => value,
        }
    }
}

impl Serialize for RunKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RunKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "loop" => Self::Loop,
            "handoff" => Self::Handoff,
            "subagent" => Self::Subagent,
            "session" => Self::Session,
            "judgment" => Self::Judgment,
            "relay" => Self::Relay,
            _ => Self::Unknown(value),
        })
    }
}

/// A run outcome this SDK knows, or an unfamiliar wire string retained
/// verbatim in `Unknown`. Consumers acting on an outcome must treat `Unknown`
/// as "not this", never as one of the known outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Failed,
    NoOp,
    TimedOut,
    Interrupted,
    PermissionDenied,
    Canceled,
    /// A non-time ceiling was reached; `reason` names which ceiling.
    Capped,
    /// An unfamiliar member, retained exactly as it appeared on the wire.
    Unknown(String),
}

impl RunOutcome {
    /// Returns the exact wire string, including an unfamiliar value verbatim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::NoOp => "no-op",
            Self::TimedOut => "timed-out",
            Self::Interrupted => "interrupted",
            Self::PermissionDenied => "permission-denied",
            Self::Canceled => "canceled",
            Self::Capped => "capped",
            Self::Unknown(value) => value,
        }
    }
}

impl Serialize for RunOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RunOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "no-op" => Self::NoOp,
            "timed-out" => Self::TimedOut,
            "interrupted" => Self::Interrupted,
            "permission-denied" => Self::PermissionDenied,
            "canceled" => Self::Canceled,
            "capped" => Self::Capped,
            _ => Self::Unknown(value),
        })
    }
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

/// Enforced limits declared by the runtime. An absent ceiling means unbounded
/// and unenforced, not defaulted; consumers must not substitute a default.
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
    /// Wall-clock bound; reaching it ends the run as `timed-out`.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub wall_ms: Option<u64>,
    /// Idle-time bound; reaching it ends the run as `timed-out`. It is
    /// suspended during an in-flight tool call. A harness that cannot enforce
    /// it omits it.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub idle_ms: Option<u64>,
    /// Maximum number of turns the run may take (the bound, not the actual).
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub turns: Option<u64>,
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
    /// Bounded explanation of a terminal outcome.
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
    #[serde(deserialize_with = "deserialize_safe_u64")]
    pub duration_ms: u64,
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
#[serde(rename_all = "camelCase")]
pub struct AgentStartedPayload {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub stage: Option<String>,
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
    pub session_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub pid: Option<u64>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTextPayload {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub stage: Option<String>,
    pub text: String,
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
    pub parent_tool_use_id: Option<String>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolUsePayload {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub stage: Option<String>,
    pub tool: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_excerpt: Option<String>,
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
    pub tool_use_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_tool_use_id: Option<String>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResultPayload {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub stage: Option<String>,
    pub tool: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_error: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_excerpt: Option<String>,
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
    pub tool_use_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_tool_use_id: Option<String>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCompletedPayload {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub stage: Option<String>,
    /// Number of turns the agent took (the actual, not the ceiling bound).
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub turns: Option<u64>,
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
    pub model: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<RunUsage>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_ms: Option<u64>,
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
#[serde(rename_all = "camelCase")]
pub struct AgentWarningPayload {
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub stage: Option<String>,
    /// Bounded non-terminal warning text.
    pub message: String,
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

/// Parses the open event envelope and rejects values either SDK cannot
/// represent. Unknown event types deliberately retain the open `Value` payload:
/// both SDKs parse their recognised `run.*` and `agent.*` vocabulary members
/// here without turning the envelope parser into a closed event-type registry.
///
/// `parse_event` answers "can both SDKs carry this?"; `validate` answers
/// "should a producer have emitted this?" This function keeps required fields
/// and integer bounds strict, but it retains unfamiliar union members and does
/// not enforce capture bounds. It deliberately does not call `validate`, so a
/// forwarder can relay an over-bound event faithfully.
///
/// A payload's *unknown fields* are a separate axis from its *unknown type*
/// and are tolerated rather than rejected (issue #12): recognised payloads do
/// not carry `deny_unknown_fields`, so an unfamiliar field does not fail
/// representability checking here, and each payload's `#[serde(flatten)]`
/// extension field
/// means a caller who deserialises directly into a typed struct (bypassing this
/// function's `Value` payload) still gets it back on re-serialisation rather
/// than silently losing it. Only the envelope stays closed to unknown fields,
/// via the `deny_unknown_fields` still present on `Event` and `EventDraft`.
pub fn parse_event(input: &str) -> serde_json::Result<Event> {
    let event: Event = serde_json::from_str(input)?;
    validate_payload_numbers(&event.payload, "payload").map_err(de::Error::custom)?;
    check_known_payload_representation(&event.event_type, &event.payload)?;
    Ok(event)
}

/// Validates whether a producer should emit `payload` for `event_type`.
/// Required fields, known closed-union membership, safe-integer bounds, and
/// capture bounds are enforced for known event types; unknown event types stay
/// open and unvalidated.
///
/// `parse_event` answers "can both SDKs carry this?"; `validate` answers
/// "should a producer have emitted this?" `parse_event` therefore does not
/// call this function: a representable over-bound event must remain
/// forwardable.
pub fn validate<P>(event_type: &str, payload: &P) -> serde_json::Result<()>
where
    P: Serialize + ?Sized,
{
    if !KNOWN_TYPES.contains(&event_type) {
        return Ok(());
    }

    let payload = serde_json::to_value(payload)?;
    validate_payload_numbers(&payload, "payload").map_err(de::Error::custom)?;

    if event_type == RUN_STARTED {
        let started = serde_json::from_value::<RunStartedPayload>(payload)?;
        if let RunKind::Unknown(value) = started.kind {
            return Err(de::Error::custom(format_args!(
                "RunStartedPayload.kind has unknown value: {value}"
            )));
        }
    } else if event_type == RUN_FINISHED {
        let finished = serde_json::from_value::<RunFinishedPayload>(payload)?;
        if let RunOutcome::Unknown(value) = finished.outcome {
            return Err(de::Error::custom(format_args!(
                "RunFinishedPayload.outcome has unknown value: {value}"
            )));
        }
        validate_scalar_bound(
            finished.reason.as_deref(),
            "RunFinishedPayload.reason",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == AGENT_STARTED {
        serde_json::from_value::<AgentStartedPayload>(payload).map(drop)?;
    } else if event_type == AGENT_TEXT {
        let text = serde_json::from_value::<AgentTextPayload>(payload)?;
        validate_scalar_bound(Some(&text.text), "AgentTextPayload.text", MAX_TEXT_SCALARS)?;
    } else if event_type == AGENT_TOOL_USE {
        let tool_use = serde_json::from_value::<AgentToolUsePayload>(payload)?;
        validate_scalar_bound(
            tool_use.input_excerpt.as_deref(),
            "AgentToolUsePayload.inputExcerpt",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == AGENT_TOOL_RESULT {
        let tool_result = serde_json::from_value::<AgentToolResultPayload>(payload)?;
        validate_scalar_bound(
            tool_result.result_excerpt.as_deref(),
            "AgentToolResultPayload.resultExcerpt",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == AGENT_COMPLETED {
        serde_json::from_value::<AgentCompletedPayload>(payload).map(drop)?;
    } else if event_type == AGENT_WARNING {
        let warning = serde_json::from_value::<AgentWarningPayload>(payload)?;
        validate_scalar_bound(
            Some(&warning.message),
            "AgentWarningPayload.message",
            MAX_EXCERPT_SCALARS,
        )?;
    }

    Ok(())
}

fn validate_scalar_bound(
    value: Option<&str>,
    field: &str,
    maximum: usize,
) -> serde_json::Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let actual = value.chars().count();
    if actual > maximum {
        Err(de::Error::custom(format_args!(
            "{field} has {actual} Unicode scalar values; maximum is {maximum}"
        )))
    } else {
        Ok(())
    }
}

/// Checks whether `payload` is representable by the typed struct for
/// `event_type`, if this SDK has one.
///
/// This is deliberately an `if`/`else if` chain comparing `event_type` with
/// `==` against the exported constants above, not a `match` on string
/// literals. A `match` arm written as a bare identifier — `match event_type {
/// RUN_STARTED => ... }` — does not compare against the constant; it
/// destructures, binding a new local variable named `RUN_STARTED` that
/// shadows the constant and matches unconditionally. The compiler only warns
/// (`non_upper_case_globals` fires on a real constant name, but nothing
/// catches a name that happens to already be uppercase), so that shape is a
/// silent bug rather than a build failure. `==` has no such reading: it is
/// always a value comparison, so an arm can only ever fire when `event_type`
/// actually equals the named constant. Because each arm's condition *is* the
/// constant rather than a second copy of its string, renaming the constant
/// renames what the arm matches and nothing else is possible — there is no
/// independent literal left to drift out of step.
fn check_known_payload_representation(event_type: &str, payload: &Value) -> serde_json::Result<()> {
    if event_type == RUN_STARTED {
        serde_json::from_value::<RunStartedPayload>(payload.clone()).map(drop)
    } else if event_type == RUN_FINISHED {
        serde_json::from_value::<RunFinishedPayload>(payload.clone()).map(drop)
    } else if event_type == AGENT_STARTED {
        serde_json::from_value::<AgentStartedPayload>(payload.clone()).map(drop)
    } else if event_type == AGENT_TEXT {
        serde_json::from_value::<AgentTextPayload>(payload.clone()).map(drop)
    } else if event_type == AGENT_TOOL_USE {
        serde_json::from_value::<AgentToolUsePayload>(payload.clone()).map(drop)
    } else if event_type == AGENT_TOOL_RESULT {
        serde_json::from_value::<AgentToolResultPayload>(payload.clone()).map(drop)
    } else if event_type == AGENT_COMPLETED {
        serde_json::from_value::<AgentCompletedPayload>(payload.clone()).map(drop)
    } else if event_type == AGENT_WARNING {
        serde_json::from_value::<AgentWarningPayload>(payload.clone()).map(drop)
    } else {
        Ok(())
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
                    "{path} is an integral number whose magnitude exceeds the safe integer bound: actual {number}; maximum {MAX_SAFE_INTEGER_MAGNITUDE}; a value that needs more precision must be carried as a string"
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
/// `RunCeilings.wall_ms`, `RunCeilings.idle_ms`, `RunCeilings.turns`,
/// `RunUsage`'s four token-count fields, and agent `pid` and `turns` are the
/// known *optional* integral fields on that typed path and are bounded here
/// individually via `deserialize_optional_safe_u64` below.
/// `RunFinishedPayload.duration_ms` and `AgentCompletedPayload.duration_ms` are
/// both durations in milliseconds, per the ruling that every count of
/// milliseconds is a `u64`; the former is required rather than optional, so
/// it applies this function directly instead of going through the optional
/// wrapper.
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

    // Cross-SDK byte identity for the new vocabulary and all five ceilings.
    // These exact literals are pasted into the TypeScript suite and asserted
    // against events hand-built through each SDK's typed API.
    const RELAY_CEILINGS_WIRE: &str = r#"{"v":1,"type":"run.started","runId":"run-batch","seq":1,"ts":"2026-09-07T04:00:00.000Z","payload":{"actor":"observer","ceilings":{"costUsd":2.5,"idleMs":30000,"tokens":4000,"turns":12,"wallMs":60000},"harness":"relay-harness","kind":"relay"}}"#;
    const CAPPED_OUTCOME_WIRE: &str = r#"{"v":1,"type":"run.finished","runId":"run-batch","seq":2,"ts":"2026-09-07T04:00:01.000Z","payload":{"durationMs":1000,"outcome":"capped","reason":"turns"}}"#;

    // This value is intentionally one neither SDK will ever know. Keeping the
    // same literal in both suites proves an older relay retaining an unfamiliar
    // member emits exactly the bytes a future vocabulary-aware SDK would emit.
    const UNKNOWN_OUTCOME_WIRE: &str = r#"{"v":1,"type":"run.finished","runId":"run-cross-version","seq":1,"ts":"2026-09-07T04:00:02.000Z","payload":{"durationMs":1250,"outcome":"not-a-real-outcome"}}"#;

    // Cross-SDK byte identity for all six agent payloads. These exact
    // literals are pasted into the TypeScript suite and asserted there
    // against events built from TypeScript's correlated payload union.
    const AGENT_STARTED_WIRE: &str = r#"{"v":1,"type":"agent.started","runId":"run-agent","seq":1,"ts":"2026-09-07T01:00:01.000Z","payload":{"model":"gpt-5","pid":4242,"sessionId":"session-local-7","stage":"open-ended-stage"}}"#;
    const AGENT_TEXT_WIRE: &str = r#"{"v":1,"type":"agent.text","runId":"run-agent","seq":2,"ts":"2026-09-07T01:00:02.000Z","payload":{"parentToolUseId":"parent-tool-1","stage":"narrate","text":"A😀漢","truncated":false}}"#;
    const AGENT_TOOL_USE_WIRE: &str = r#"{"v":1,"type":"agent.tool_use","runId":"run-agent","seq":3,"ts":"2026-09-07T01:00:03.000Z","payload":{"inputExcerpt":"{\"path\":\"README.md\"}","parentToolUseId":"parent-tool-1","stage":"act","tool":"read_file","toolUseId":"tool-7","truncated":false}}"#;
    const AGENT_TOOL_RESULT_WIRE: &str = r#"{"v":1,"type":"agent.tool_result","runId":"run-agent","seq":4,"ts":"2026-09-07T01:00:04.000Z","payload":{"isError":false,"parentToolUseId":"parent-tool-1","resultExcerpt":"placeholder result","stage":"act","tool":"read_file","toolUseId":"tool-7","truncated":false}}"#;
    const AGENT_COMPLETED_WIRE: &str = r#"{"v":1,"type":"agent.completed","runId":"run-agent","seq":5,"ts":"2026-09-07T01:00:05.000Z","payload":{"costUsd":1.25,"durationMs":2500,"estimated":true,"model":"gpt-5","stage":"finish","turns":3,"usage":{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"}}}"#;
    const AGENT_WARNING_WIRE: &str = r#"{"v":1,"type":"agent.warning","runId":"run-agent","seq":6,"ts":"2026-09-07T01:00:06.000Z","payload":{"message":"placeholder warning","stage":"observe"}}"#;

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
        for kind in [
            "loop", "handoff", "subagent", "session", "judgment", "relay",
        ] {
            let payload = json!({ "kind": kind, "actor": "builder", "harness": "codex" });
            let input = lifecycle_event_input("run.started", payload.clone());
            parse_event(&input).unwrap();
            validate(RUN_STARTED, &payload).unwrap();
        }
    }

    #[test]
    fn parses_an_unknown_run_kind_verbatim_and_validate_reports_it() {
        let input = lifecycle_event_input(
            "run.started",
            json!({ "kind": "pipeline", "actor": "builder", "harness": "codex" }),
        );

        let event = parse_event(&input).unwrap();
        let parsed: RunStartedPayload = serde_json::from_value(event.payload.clone()).unwrap();
        assert_eq!(parsed.kind, RunKind::Unknown("pipeline".to_owned()));

        let error = validate(RUN_STARTED, &event.payload).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RunStartedPayload.kind has unknown value: pipeline"),
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
            "capped",
        ] {
            let payload = json!({ "outcome": outcome, "durationMs": 1250 });
            let input = lifecycle_event_input("run.finished", payload.clone());
            parse_event(&input).unwrap();
            validate(RUN_FINISHED, &payload).unwrap();
        }
    }

    #[test]
    fn parses_an_unknown_run_outcome_verbatim_and_validate_reports_it() {
        let input = lifecycle_event_input(
            "run.finished",
            json!({ "outcome": "succeeded", "durationMs": 1250 }),
        );

        let event = parse_event(&input).unwrap();
        let parsed: RunFinishedPayload = serde_json::from_value(event.payload.clone()).unwrap();
        assert_eq!(parsed.outcome, RunOutcome::Unknown("succeeded".to_owned()));

        let error = validate(RUN_FINISHED, &event.payload).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RunFinishedPayload.outcome has unknown value: succeeded"),
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
    fn accepts_an_open_stage_string_on_every_agent_event() {
        let cases = [
            (
                "agent.started",
                json!({ "stage": "consumer-specific/stage" }),
            ),
            (
                "agent.text",
                json!({ "stage": "consumer-specific/stage", "text": "text" }),
            ),
            (
                "agent.tool_use",
                json!({ "stage": "consumer-specific/stage", "tool": "read" }),
            ),
            (
                "agent.tool_result",
                json!({ "stage": "consumer-specific/stage", "tool": "read" }),
            ),
            (
                "agent.completed",
                json!({ "stage": "consumer-specific/stage" }),
            ),
            (
                "agent.warning",
                json!({ "stage": "consumer-specific/stage", "message": "warning" }),
            ),
        ];

        for (event_type, payload) in cases {
            assert!(parse_event(&lifecycle_event_input(event_type, payload)).is_ok());
        }
    }

    #[test]
    fn rejects_each_missing_required_agent_payload_field() {
        for (event_type, field) in [
            ("agent.text", "text"),
            ("agent.tool_use", "tool"),
            ("agent.tool_result", "tool"),
            ("agent.warning", "message"),
        ] {
            let error = parse_event(&lifecycle_event_input(event_type, json!({}))).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "error for {event_type}.{field} was: {error}"
            );
        }
    }

    #[test]
    fn rejects_non_string_required_agent_payload_fields() {
        for (event_type, field) in [
            ("agent.text", "text"),
            ("agent.tool_use", "tool"),
            ("agent.tool_result", "tool"),
            ("agent.warning", "message"),
        ] {
            let error =
                parse_event(&lifecycle_event_input(event_type, json!({ (field): 7 }))).unwrap_err();
            assert!(
                error.to_string().contains("expected a string"),
                "error for {event_type}.{field} was: {error}"
            );
        }
    }

    #[test]
    fn validate_enforces_required_fields_and_integer_bounds_for_known_types() {
        let missing = validate(RUN_STARTED, &json!({})).unwrap_err();
        assert!(missing.to_string().contains("kind"), "error was: {missing}");

        let unsafe_integer = validate(
            AGENT_COMPLETED,
            &json!({ "nested": { "turns": MAX_SAFE_INTEGER_MAGNITUDE + 1 } }),
        )
        .unwrap_err();
        assert!(
            unsafe_integer
                .to_string()
                .contains(
                    "payload.nested.turns is an integral number whose magnitude exceeds the safe integer bound: actual 9007199254740992; maximum 9007199254740991"
                ),
            "error was: {unsafe_integer}"
        );
    }

    #[test]
    fn validate_leaves_unknown_event_types_open_and_unvalidated() {
        assert!(validate("future.happened", &json!("not-an-object")).is_ok());
    }

    #[test]
    fn validate_reports_every_capture_bound_with_field_actual_and_maximum() {
        let cases = [
            (
                AGENT_TEXT,
                json!({ "text": "😀".repeat(MAX_TEXT_SCALARS + 1) }),
                "AgentTextPayload.text",
                MAX_TEXT_SCALARS,
            ),
            (
                AGENT_TOOL_USE,
                json!({
                    "tool": "read",
                    "inputExcerpt": "😀".repeat(MAX_EXCERPT_SCALARS + 1)
                }),
                "AgentToolUsePayload.inputExcerpt",
                MAX_EXCERPT_SCALARS,
            ),
            (
                AGENT_TOOL_RESULT,
                json!({
                    "tool": "read",
                    "resultExcerpt": "😀".repeat(MAX_EXCERPT_SCALARS + 1)
                }),
                "AgentToolResultPayload.resultExcerpt",
                MAX_EXCERPT_SCALARS,
            ),
            (
                RUN_FINISHED,
                json!({
                    "outcome": "completed",
                    "durationMs": 1,
                    "reason": "😀".repeat(MAX_EXCERPT_SCALARS + 1)
                }),
                "RunFinishedPayload.reason",
                MAX_EXCERPT_SCALARS,
            ),
            (
                AGENT_WARNING,
                json!({ "message": "😀".repeat(MAX_EXCERPT_SCALARS + 1) }),
                "AgentWarningPayload.message",
                MAX_EXCERPT_SCALARS,
            ),
        ];

        for (event_type, payload, field, maximum) in cases {
            let error = validate(event_type, &payload).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "{field} has {} Unicode scalar values; maximum is {maximum}",
                    maximum + 1
                )
            );
        }
    }

    #[test]
    fn parse_event_carries_an_over_bound_event_that_validate_refuses() {
        let input = lifecycle_event_input("agent.text", json!({ "text": "x".repeat(20_000) }));
        let event = parse_event(&input).unwrap();
        let error = validate(&event.event_type, &event.payload).unwrap_err();

        assert_eq!(
            error.to_string(),
            "AgentTextPayload.text has 20000 Unicode scalar values; maximum is 16384"
        );
    }

    #[test]
    fn typed_agent_counts_reject_invalid_values_and_accept_the_safe_bound() {
        for invalid in ["-1", "1.5", "9007199254740992"] {
            let started = format!(r#"{{"pid":{invalid}}}"#);
            assert!(serde_json::from_str::<AgentStartedPayload>(&started).is_err());

            for field in ["turns", "durationMs"] {
                let completed = format!(r#"{{"{field}":{invalid}}}"#);
                assert!(serde_json::from_str::<AgentCompletedPayload>(&completed).is_err());
            }
        }

        let safe = MAX_SAFE_INTEGER_MAGNITUDE;
        assert!(
            serde_json::from_str::<AgentStartedPayload>(&format!(r#"{{"pid":{safe}}}"#)).is_ok()
        );
        assert!(
            serde_json::from_str::<AgentCompletedPayload>(&format!(
                r#"{{"turns":{safe},"durationMs":{safe}}}"#
            ))
            .is_ok()
        );
    }

    #[test]
    fn every_agent_payload_retains_unknown_fields() {
        let started: AgentStartedPayload = serde_json::from_value(json!({
            "future": { "value": 1 }
        }))
        .unwrap();
        let text: AgentTextPayload = serde_json::from_value(json!({
            "text": "text",
            "future": { "value": 1 }
        }))
        .unwrap();
        let tool_use: AgentToolUsePayload = serde_json::from_value(json!({
            "tool": "read",
            "future": { "value": 1 }
        }))
        .unwrap();
        let tool_result: AgentToolResultPayload = serde_json::from_value(json!({
            "tool": "read",
            "future": { "value": 1 }
        }))
        .unwrap();
        let completed: AgentCompletedPayload = serde_json::from_value(json!({
            "future": { "value": 1 },
            "usage": { "inputTokens": 2, "futureUsage": "retained" }
        }))
        .unwrap();
        let warning: AgentWarningPayload = serde_json::from_value(json!({
            "message": "warning",
            "future": { "value": 1 }
        }))
        .unwrap();

        for extra in [
            &started.extra,
            &text.extra,
            &tool_use.extra,
            &tool_result.extra,
            &completed.extra,
            &warning.extra,
        ] {
            assert_eq!(extra.get("future"), Some(&json!({ "value": 1 })));
        }
        for re_emitted in [
            serde_json::to_value(&started).unwrap(),
            serde_json::to_value(&text).unwrap(),
            serde_json::to_value(&tool_use).unwrap(),
            serde_json::to_value(&tool_result).unwrap(),
            serde_json::to_value(&completed).unwrap(),
            serde_json::to_value(&warning).unwrap(),
        ] {
            assert_eq!(re_emitted["future"], json!({ "value": 1 }));
        }
        assert_eq!(
            completed.usage.as_ref().unwrap().extra.get("futureUsage"),
            Some(&json!("retained"))
        );
    }

    #[test]
    fn excerpt_handles_ascii_below_at_and_one_scalar_over_the_bound() {
        assert_eq!(
            excerpt("abc", 4),
            Excerpt {
                text: "abc".to_owned(),
                truncated: false
            }
        );
        assert_eq!(
            excerpt("abcd", 4),
            Excerpt {
                text: "abcd".to_owned(),
                truncated: false
            }
        );
        assert_eq!(
            excerpt("abcde", 4),
            Excerpt {
                text: "abcd".to_owned(),
                truncated: true
            }
        );
    }

    #[test]
    fn excerpt_counts_astral_plane_characters_as_single_scalars() {
        assert_eq!(MAX_TEXT_SCALARS, 16_384);
        assert_eq!(MAX_EXCERPT_SCALARS, 4_096);
        let input = "😀".repeat(MAX_EXCERPT_SCALARS + 1);
        let result = excerpt(&input, MAX_EXCERPT_SCALARS);

        assert_eq!(
            result,
            Excerpt {
                text: "😀".repeat(MAX_EXCERPT_SCALARS),
                truncated: true
            }
        );
        assert_eq!(result.text.chars().count(), MAX_EXCERPT_SCALARS);
        assert_eq!(result.text.len(), MAX_EXCERPT_SCALARS * 4);
    }

    #[test]
    fn excerpt_counts_three_byte_utf8_characters_as_scalars_not_bytes() {
        let input = "漢".repeat(MAX_EXCERPT_SCALARS + 1);
        let result = excerpt(&input, MAX_EXCERPT_SCALARS);

        assert_eq!(
            result,
            Excerpt {
                text: "漢".repeat(MAX_EXCERPT_SCALARS),
                truncated: true
            }
        );
        assert_eq!(result.text.chars().count(), MAX_EXCERPT_SCALARS);
        assert_eq!(result.text.len(), MAX_EXCERPT_SCALARS * 3);
    }

    #[test]
    fn excerpt_matches_the_typescript_mixed_scalar_expectation() {
        assert_eq!(
            excerpt("A😀漢B", 3),
            Excerpt {
                text: "A😀漢".to_owned(),
                truncated: true
            }
        );
    }

    #[test]
    fn over_bound_astral_agent_text_round_trips_through_rust() {
        let bounded = excerpt(&"😀".repeat(MAX_TEXT_SCALARS + 1), MAX_TEXT_SCALARS);
        let event = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.text".to_owned(),
            run_id: "run-excerpt".to_owned(),
            seq: 1,
            ts: "2026-09-07T02:00:00.000Z".to_owned(),
            payload: AgentTextPayload {
                stage: None,
                text: bounded.text,
                truncated: Some(bounded.truncated),
                parent_tool_use_id: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        let wire = serialise_event(&event).unwrap();
        let parsed = parse_event(&wire).unwrap();
        let parsed_payload: AgentTextPayload = serde_json::from_value(parsed.payload).unwrap();
        assert_eq!(parsed_payload.text.chars().count(), MAX_TEXT_SCALARS);
        assert_eq!(parsed_payload.text, "😀".repeat(MAX_TEXT_SCALARS));
        assert_eq!(parsed_payload.truncated, Some(true));
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
                    idle_ms: None,
                    turns: None,
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
                duration_ms: 1250,
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
    fn relay_capped_and_all_five_ceilings_match_the_typescript_pinned_bytes() {
        let started = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: RUN_STARTED.to_owned(),
            run_id: "run-batch".to_owned(),
            seq: 1,
            ts: "2026-09-07T04:00:00.000Z".to_owned(),
            payload: RunStartedPayload {
                kind: RunKind::Relay,
                actor: "observer".to_owned(),
                harness: "relay-harness".to_owned(),
                model: None,
                parent_run_id: None,
                parent_tool_use_id: None,
                schedule: None,
                repository: None,
                work_order: None,
                ceilings: Some(RunCeilings {
                    cost_usd: Some(2.5),
                    tokens: Some(4000),
                    wall_ms: Some(60000),
                    idle_ms: Some(30000),
                    turns: Some(12),
                    extra: PayloadExtension::new(),
                }),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let finished = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: RUN_FINISHED.to_owned(),
            run_id: "run-batch".to_owned(),
            seq: 2,
            ts: "2026-09-07T04:00:01.000Z".to_owned(),
            payload: RunFinishedPayload {
                outcome: RunOutcome::Capped,
                reason: Some("turns".to_owned()),
                truncated: None,
                cost_usd: None,
                usage: None,
                duration_ms: 1000,
                estimated: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(serialise_event(&started).unwrap(), RELAY_CEILINGS_WIRE);
        assert_eq!(serialise_event(&finished).unwrap(), CAPPED_OUTCOME_WIRE);
    }

    #[test]
    fn unknown_outcome_keeps_cross_version_byte_identity_with_typescript_and_input() {
        let event = parse_event(UNKNOWN_OUTCOME_WIRE).unwrap();
        let parsed: RunFinishedPayload = serde_json::from_value(event.payload.clone()).unwrap();

        assert_eq!(
            parsed.outcome,
            RunOutcome::Unknown("not-a-real-outcome".to_owned())
        );
        assert_eq!(serialise_event(&event).unwrap(), UNKNOWN_OUTCOME_WIRE);
    }

    #[test]
    fn retained_and_known_capped_outcomes_emit_identical_bytes() {
        assert_eq!(
            serde_json::to_string(&RunOutcome::Unknown("capped".to_owned())).unwrap(),
            serde_json::to_string(&RunOutcome::Capped).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&RunOutcome::Capped).unwrap(),
            r#""capped""#
        );
    }

    #[test]
    fn agent_events_match_the_typescript_pinned_bytes() {
        let started = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.started".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 1,
            ts: "2026-09-07T01:00:01.000Z".to_owned(),
            payload: AgentStartedPayload {
                stage: Some("open-ended-stage".to_owned()),
                model: Some("gpt-5".to_owned()),
                session_id: Some("session-local-7".to_owned()),
                pid: Some(4242),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let text = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.text".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 2,
            ts: "2026-09-07T01:00:02.000Z".to_owned(),
            payload: AgentTextPayload {
                stage: Some("narrate".to_owned()),
                text: "A😀漢".to_owned(),
                truncated: Some(false),
                parent_tool_use_id: Some("parent-tool-1".to_owned()),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let tool_use = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.tool_use".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 3,
            ts: "2026-09-07T01:00:03.000Z".to_owned(),
            payload: AgentToolUsePayload {
                stage: Some("act".to_owned()),
                tool: "read_file".to_owned(),
                input_excerpt: Some(r#"{"path":"README.md"}"#.to_owned()),
                truncated: Some(false),
                tool_use_id: Some("tool-7".to_owned()),
                parent_tool_use_id: Some("parent-tool-1".to_owned()),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let tool_result = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.tool_result".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 4,
            ts: "2026-09-07T01:00:04.000Z".to_owned(),
            payload: AgentToolResultPayload {
                stage: Some("act".to_owned()),
                tool: "read_file".to_owned(),
                is_error: Some(false),
                result_excerpt: Some("placeholder result".to_owned()),
                truncated: Some(false),
                tool_use_id: Some("tool-7".to_owned()),
                parent_tool_use_id: Some("parent-tool-1".to_owned()),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let completed = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.completed".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 5,
            ts: "2026-09-07T01:00:05.000Z".to_owned(),
            payload: AgentCompletedPayload {
                stage: Some("finish".to_owned()),
                turns: Some(3),
                cost_usd: Some(1.25),
                model: Some("gpt-5".to_owned()),
                usage: Some(RunUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(40),
                    cache_read_tokens: Some(20),
                    cache_creation_tokens: Some(30),
                    unit: Some("weighted-tokens".to_owned()),
                    extra: PayloadExtension::new(),
                }),
                duration_ms: Some(2500),
                estimated: Some(true),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let warning = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.warning".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 6,
            ts: "2026-09-07T01:00:06.000Z".to_owned(),
            payload: AgentWarningPayload {
                stage: Some("observe".to_owned()),
                message: "placeholder warning".to_owned(),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(serialise_event(&started).unwrap(), AGENT_STARTED_WIRE);
        assert_eq!(serialise_event(&text).unwrap(), AGENT_TEXT_WIRE);
        assert_eq!(serialise_event(&tool_use).unwrap(), AGENT_TOOL_USE_WIRE);
        assert_eq!(
            serialise_event(&tool_result).unwrap(),
            AGENT_TOOL_RESULT_WIRE
        );
        assert_eq!(serialise_event(&completed).unwrap(), AGENT_COMPLETED_WIRE);
        assert_eq!(serialise_event(&warning).unwrap(), AGENT_WARNING_WIRE);
    }

    #[test]
    fn absent_optional_agent_payload_fields_are_omitted_instead_of_null() {
        let started = AgentStartedPayload {
            stage: None,
            model: None,
            session_id: None,
            pid: None,
            extra: PayloadExtension::new(),
        };
        let completed = AgentCompletedPayload {
            stage: None,
            turns: None,
            cost_usd: None,
            model: None,
            usage: None,
            duration_ms: None,
            estimated: None,
            extra: PayloadExtension::new(),
        };

        assert_eq!(serde_json::to_string(&started).unwrap(), "{}");
        assert_eq!(serde_json::to_string(&completed).unwrap(), "{}");
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
                duration_ms: 1000,
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

    #[test]
    fn idle_and_turn_ceilings_apply_the_safe_integer_rule() {
        for field in ["idleMs", "turns"] {
            for invalid in ["-1", "1.5", "9007199254740992"] {
                let ceilings = format!(r#"{{"{field}":{invalid}}}"#);
                assert!(serde_json::from_str::<RunCeilings>(&ceilings).is_err());
            }

            let safe = format!(r#"{{"{field}":{MAX_SAFE_INTEGER_MAGNITUDE}}}"#);
            assert!(serde_json::from_str::<RunCeilings>(&safe).is_ok());
        }
    }

    // -- run.finished durationMs is a required u64 (follow-up to issue #6) --

    #[test]
    fn rejects_a_non_integer_run_finished_duration() {
        // `u64` deserialization rejects a non-integral value by construction;
        // this test pins that behaviour rather than assuming it.
        assert!(
            serde_json::from_str::<RunFinishedPayload>(
                r#"{"outcome":"completed","durationMs":1250.5}"#
            )
            .is_err()
        );
        let input = lifecycle_event_input(
            "run.finished",
            json!({ "outcome": "completed", "durationMs": 1250.5 }),
        );
        assert!(parse_event(&input).is_err());
    }

    #[test]
    fn rejects_a_negative_run_finished_duration() {
        // `u64` deserialization rejects a negative value by construction;
        // this test pins that behaviour rather than assuming it.
        assert!(
            serde_json::from_str::<RunFinishedPayload>(
                r#"{"outcome":"completed","durationMs":-5}"#
            )
            .is_err()
        );
        let input = lifecycle_event_input(
            "run.finished",
            json!({ "outcome": "completed", "durationMs": -5 }),
        );
        assert!(parse_event(&input).is_err());
    }

    /// The typed-struct-path bound check the follow-up brief calls for:
    /// `parse_event` runs `validate_payload_numbers` over the whole payload,
    /// but a caller who deserialises straight into `Event<RunFinishedPayload>`
    /// (bypassing `parse_event` entirely) relies instead on the
    /// `deserialize_optional_safe_u64` each of these bounded fields carries.
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
    /// `RunFinishedPayload.usage`, so both nested structs are covered on the
    /// typed path rather than just the one the brief names explicitly.
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

    /// For each of the eight known types, builds a minimally valid event of
    /// that type, serialises it, and parses the `type` field back out of the
    /// result — then compares that against the exported constant.
    ///
    /// Deliberately not `assert_eq!(RUN_STARTED, "run.started")`: that only
    /// proves someone typed the same string twice, and would pass just as
    /// happily if both copies were wrong. Going through a real round-trip
    /// fails the moment the constant and what
    /// `check_known_payload_representation` (and so `parse_event`) actually
    /// accepts for that type part company.
    #[test]
    fn known_type_constants_match_their_own_wire_round_trip() {
        fn round_tripped_type(event_type: &str, payload: Value) -> String {
            let input = lifecycle_event_input(event_type, payload);
            parse_event(&input).unwrap().event_type
        }

        assert_eq!(
            round_tripped_type(
                RUN_STARTED,
                json!({ "kind": "loop", "actor": "builder", "harness": "codex" }),
            ),
            RUN_STARTED
        );
        assert_eq!(
            round_tripped_type(
                RUN_FINISHED,
                json!({ "outcome": "completed", "durationMs": 1250 }),
            ),
            RUN_FINISHED
        );
        assert_eq!(round_tripped_type(AGENT_STARTED, json!({})), AGENT_STARTED);
        assert_eq!(
            round_tripped_type(AGENT_TEXT, json!({ "text": "hello" })),
            AGENT_TEXT
        );
        assert_eq!(
            round_tripped_type(AGENT_TOOL_USE, json!({ "tool": "read" })),
            AGENT_TOOL_USE
        );
        assert_eq!(
            round_tripped_type(AGENT_TOOL_RESULT, json!({ "tool": "read" })),
            AGENT_TOOL_RESULT
        );
        assert_eq!(
            round_tripped_type(AGENT_COMPLETED, json!({})),
            AGENT_COMPLETED
        );
        assert_eq!(
            round_tripped_type(AGENT_WARNING, json!({ "message": "warning" })),
            AGENT_WARNING
        );
    }

    #[test]
    fn known_types_holds_exactly_the_eight_recognised_types_with_no_duplicates() {
        assert_eq!(KNOWN_TYPES.len(), 8);

        let unique: std::collections::HashSet<&str> = KNOWN_TYPES.iter().copied().collect();
        assert_eq!(
            unique.len(),
            KNOWN_TYPES.len(),
            "KNOWN_TYPES has a duplicate"
        );
        assert_eq!(
            unique,
            std::collections::HashSet::from([
                RUN_STARTED,
                RUN_FINISHED,
                AGENT_STARTED,
                AGENT_TEXT,
                AGENT_TOOL_USE,
                AGENT_TOOL_RESULT,
                AGENT_COMPLETED,
                AGENT_WARNING,
            ])
        );
    }

    #[test]
    fn every_known_type_uses_the_representation_path_and_an_unrecognised_type_does_not() {
        // A JSON string fails every known payload struct's deserialisation
        // (each expects an object), while an unrecognised type's fallthrough
        // arm accepts any payload unconditionally. This distinguishes "this
        // type was actually validated against a typed struct" from "this
        // type was waved through" without depending on any one type's
        // required fields.
        let malformed_payload = json!("not-an-object");

        for &known in &KNOWN_TYPES {
            assert!(
                check_known_payload_representation(known, &malformed_payload).is_err(),
                "{known} should have been checked against its typed payload"
            );
        }

        assert!(check_known_payload_representation("future.happened", &malformed_payload).is_ok());
    }
}
