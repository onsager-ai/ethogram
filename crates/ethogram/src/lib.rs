use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

mod union_unknown_validation;
mod validation;

use validation::decode_payload;
pub use validation::{ValidationError, ValidationErrorKind};

pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Maximum number of Unicode scalar values carried by an `agent.text`.
pub const MAX_TEXT_SCALARS: usize = 16_384;

/// Maximum scalars carried by any excerpted field other than `agent.text`.
pub const MAX_EXCERPT_SCALARS: usize = 4_096;

/// Maximum serialised size, in UTF-8 bytes, of a payload **alone** — the
/// payload only, never the envelope around it — measured on the same
/// canonical compact serialisation `serialise_event` produces for it (issue
/// onsager-ai/ethogram#28). [`validate`] enforces this for every event, including one whose
/// `type` it does not recognise, as a floor under [`MAX_TEXT_SCALARS`] and
/// [`MAX_EXCERPT_SCALARS`]: those bound named fields on types this SDK
/// knows, and have nothing to say about an unrecognised type's payload.
/// [`parse_event`] leaves this bound unenforced, exactly as it leaves the
/// scalar bounds unenforced: an over-large payload is still perfectly
/// representable, and a forwarder must still be able to relay it.
///
/// 131,072 (128 KiB), not the 65,536 (64 KiB) first proposed for it. A
/// scalar value is at most four UTF-8 bytes, so a text at exactly
/// `MAX_TEXT_SCALARS` composed of astral-plane characters is
/// 16,384 × 4 = 65,536 bytes on its own — the entire 64 KiB budget, with
/// nothing left over for the rest of the payload. A producer using
/// [`excerpt()`] exactly as specified on emoji-heavy text would then emit an
/// `agent.text` this bound rejects. Doubling the budget keeps the scalar
/// bound binding first for text, which is the intended relationship: this
/// byte bound is a backstop against an unbounded *unknown* payload, not a
/// second opinion about text.
pub const MAX_PAYLOAD_BYTES: usize = 131_072;

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
/// The wire string for a `control.requested` event's `type` field.
pub const CONTROL_REQUESTED: &str = "control.requested";
/// The wire string for a `control.applied` event's `type` field.
pub const CONTROL_APPLIED: &str = "control.applied";
/// The wire string for a `capture.refused` event's `type` field.
pub const CAPTURE_REFUSED: &str = "capture.refused";
/// The wire string for a `decision.requested` event's `type` field.
pub const DECISION_REQUESTED: &str = "decision.requested";
/// The wire string for a `decision.answered` event's `type` field.
pub const DECISION_ANSWERED: &str = "decision.answered";

/// Every event `type` this SDK has a typed payload for. This is not a closed
/// vocabulary: `parse_event` still accepts a type it has never heard of (see
/// `check_known_payload_representation`'s fallthrough), and a consumer may
/// still match a literal for vocabulary this SDK has not learned. A constant
/// is a name for a string, not a gate.
pub const KNOWN_TYPES: [&str; 13] = [
    RUN_STARTED,
    RUN_FINISHED,
    AGENT_STARTED,
    AGENT_TEXT,
    AGENT_TOOL_USE,
    AGENT_TOOL_RESULT,
    AGENT_COMPLETED,
    AGENT_WARNING,
    CONTROL_REQUESTED,
    CONTROL_APPLIED,
    CAPTURE_REFUSED,
    DECISION_REQUESTED,
    DECISION_ANSWERED,
];

/// The result of [`excerpt`]: the kept text, and whether a bound applied.
///
/// On the wire the corresponding payload field is optional, and **its absence
/// means `false`**. A producer may also emit `false` explicitly, and both
/// forms are conforming — the protocol says nothing stronger (ruled on onsager-ai/ethogram#12).
/// Canonicalisation governs *notation*, not *presence*: it fixes how a value
/// is spelled once written, and does not decide whether an optional field is
/// written at all. Requiring omission would have made goldens already derived
/// from real captures retroactively non-conforming, for no benefit a consumer
/// can observe, since a reader must handle an absent flag either way.
///
/// The corpus therefore carries both forms, which is the useful outcome: it
/// proves each round-trips, rather than asserting a preference no producer
/// agreed to.
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
/// lone surrogate with `U+FFFD` (issue onsager-ai/ethogram#6): a Rust `&str` is guaranteed
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
/// validation (issue onsager-ai/ethogram#9): both reject an out-of-range integral value at parse
/// time rather than rounding it.
const MAX_SAFE_INTEGER_MAGNITUDE: u64 = 9_007_199_254_740_991;

/// A run kind this SDK knows, or an unfamiliar wire string retained verbatim
/// in `Unknown`. Consumers must render it with its raw value, must never map
/// it onto a known kind, and, when acting on it, must treat it as "not this",
/// never as a default.
/// At validation, unfamiliar strings are `UnknownMember`; `Unknown` spelling
/// a known member is `Malformed` because it cannot round-trip as that variant.
///
/// Each member is decided by a fact a producer can check rather than by what
/// its name suggests, and they are **ordered**, so no run fits two. Apply the
/// checks top down and take the first that holds (onsager-ai/ethogram#64):
///
/// 1. `parentRunId` present — [`Subagent`](Self::Subagent)
/// 2. observes other runs and changes nothing — [`Relay`](Self::Relay)
/// 3. `schedule` present — [`Loop`](Self::Loop)
/// 4. started interactively by the harness's user — [`Session`](Self::Session)
/// 5. the product is a decision or verdict only — [`Judgment`](Self::Judgment)
/// 6. otherwise, a dispatched work order — [`Handoff`](Self::Handoff)
///
/// The order is what settles the cases that would otherwise be ambiguous: a
/// scheduled gatekeeper pass is a `Loop`, because a schedule outranks what the
/// run produces, while the same evaluation run on demand is a `Judgment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunKind {
    /// A run started by a schedule the operator declared, recurring at that
    /// cadence. Decided by: `schedule` present, and the scheduler started it
    /// rather than a person or a dispatch.
    Loop,
    /// A run in which an orchestrator or principal dispatches a work order to
    /// an agent to carry out unattended, once. Decided by: `workOrder`, or an
    /// equivalent intent reference, present; no `schedule`; no `parentRunId`;
    /// and the product is work rather than a verdict.
    Handoff,
    /// A run started by another run and observed under it. Decided by:
    /// `parentRunId` present, with `parentToolUseId` when a tool call spawned
    /// it.
    Subagent,
    /// An interactive harness session in which the harness's own user
    /// initiates the turns. Decided by: a person started it at the harness,
    /// with `actor` naming the harness's notion of that user; no `schedule`
    /// and no work order.
    Session,
    /// A run whose product is a decision or verdict record and nothing else —
    /// an answer to a queued item, a gate evaluated on demand. Decided by: it
    /// emits `decision.*` or a verdict and changes no repository, and a
    /// principal's or operator's command started it.
    Judgment,
    /// A long-lived process that observes other runs and emits on its own run,
    /// and changes nothing itself. Decided by: it emits about other runs, with
    /// no work order and no repository change.
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
/// verbatim in `Unknown`. Consumers must render it with its raw value, must
/// never map it onto a known outcome, and, when acting on it, must treat it
/// as "not this", never as a default.
/// At validation, unfamiliar strings are `UnknownMember`; `Unknown` spelling
/// a known member is `Malformed` because it cannot round-trip as that variant.
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
    /// Ended because it may not proceed until something outside the run
    /// changes; **not an error of the run**. A consumer that colours
    /// `blocked` as a failure is misreporting, since nothing went wrong
    /// inside the run.
    Blocked,
    /// The run's process never ran; `reason` names why — `disarmed`,
    /// `spawn`, or a missing binary, excerpted as today. Distinct from
    /// `Failed`, where the process ran and did not succeed.
    Unstarted,
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
            Self::Blocked => "blocked",
            Self::Unstarted => "unstarted",
            Self::Unknown(value) => value,
        }
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
            "blocked" => Self::Blocked,
            "unstarted" => Self::Unstarted,
            _ => Self::Unknown(value),
        })
    }
}

/// A control kind this SDK knows, or an unfamiliar wire string retained
/// verbatim in `Unknown`. Consumers must render it with its raw value, must
/// never map it onto a known kind, and, when acting on it, must treat it as
/// "not this", never as a default.
/// An unfamiliar wire string is `UnknownMember` at validation, exactly like
/// every other closed union; `Unknown` spelling any known kind is
/// `Malformed` at validation instead, because parsing that string would have
/// yielded the known variant.
///
/// There is deliberately no `Pause` member: no harness the operator uses can
/// pause headlessly, and a verb the runtime cannot honour is a lie in a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// A process-group termination with grace. The runtime emits
    /// `run.finished` with `outcome: "interrupted"` after `control.applied`.
    Interrupt,
    /// Queues the request's `text` as the run's next user turn. This is
    /// between turns: mid-turn injection is not available headlessly on
    /// Claude Code or Codex, and the protocol does not pretend otherwise. A
    /// runtime honours `steer` by resuming the session with `text` as the
    /// next user turn.
    Steer,
    /// Delivers the principal's choice for a waiting decision.
    Answer,
    /// An unfamiliar member, retained exactly as it appeared on the wire.
    Unknown(String),
}

impl ControlKind {
    /// Returns the exact wire string, including an unfamiliar value verbatim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Steer => "steer",
            Self::Answer => "answer",
            Self::Unknown(value) => value,
        }
    }

    fn from_wire(value: String) -> Self {
        match value.as_str() {
            "interrupt" => Self::Interrupt,
            "steer" => Self::Steer,
            "answer" => Self::Answer,
            _ => Self::Unknown(value),
        }
    }
}

impl<'de> Deserialize<'de> for ControlKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_wire(value))
    }
}

/// An explanation of a control echo, open at parse and validation: an
/// unfamiliar reason here is the expected case, not the exceptional one, so
/// `validate` accepts it and its Unknown values retain their exact string,
/// subject to the excerpt bound. Consumers must render it with its raw
/// value, must never map it onto a known reason, and, when acting on it,
/// must treat it as "not this", never as a default.
/// A typed `Unknown` spelling a known member is `Malformed` at validation:
/// representability applies even though unfamiliar strings are accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlAppliedReason {
    NoSuchDecision,
    AlreadyAnswered,
    OptionNotOffered,
    Unsupported,
    NotLive,
    Rejected,
    Unknown(String),
}

impl ControlAppliedReason {
    /// Returns the exact wire string, including an unfamiliar value verbatim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::NoSuchDecision => "no-such-decision",
            Self::AlreadyAnswered => "already-answered",
            Self::OptionNotOffered => "option-not-offered",
            Self::Unsupported => "unsupported",
            Self::NotLive => "not-live",
            Self::Rejected => "rejected",
            Self::Unknown(value) => value,
        }
    }
}

impl<'de> Deserialize<'de> for ControlAppliedReason {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "no-such-decision" => Self::NoSuchDecision,
            "already-answered" => Self::AlreadyAnswered,
            "option-not-offered" => Self::OptionNotOffered,
            "unsupported" => Self::Unsupported,
            "not-live" => Self::NotLive,
            "rejected" => Self::Rejected,
            _ => Self::Unknown(value),
        })
    }
}

/// A capture-refusal cause this SDK knows, or an unfamiliar wire string
/// retained verbatim in `Unknown`. Consumers must render it with its raw
/// value, must never map it onto a known cause, and, when acting on it, must
/// treat it as "not this", never as a default.
/// At validation, unfamiliar strings are `UnknownMember`; `Unknown` spelling
/// a known member is `Malformed` because it cannot round-trip as that variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureRefusalCause {
    /// The refused event exceeded a capture bound.
    OverBound,
    /// The refused event would leave a gap in the source run's sequence.
    Gap,
    /// The refused event duplicated one already recorded.
    Duplicate,
    /// The source run had already finished.
    Finished,
    /// The refused event could not be parsed into a representable envelope.
    Malformed,
    /// An unfamiliar member, retained exactly as it appeared on the wire.
    Unknown(String),
}

impl CaptureRefusalCause {
    /// Returns the exact wire string, including an unfamiliar value verbatim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::OverBound => "over_bound",
            Self::Gap => "gap",
            Self::Duplicate => "duplicate",
            Self::Finished => "finished",
            Self::Malformed => "malformed",
            Self::Unknown(value) => value,
        }
    }
}

impl<'de> Deserialize<'de> for CaptureRefusalCause {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "over_bound" => Self::OverBound,
            "gap" => Self::Gap,
            "duplicate" => Self::Duplicate,
            "finished" => Self::Finished,
            "malformed" => Self::Malformed,
            _ => Self::Unknown(value),
        })
    }
}

/// A decision kind this SDK knows, or an unfamiliar wire string retained
/// verbatim in `Unknown`. Consumers must render it with its raw value, must
/// never map it onto a known kind, and, when acting on it, must treat it as
/// "not this", never as a default.
/// At validation, unfamiliar strings are `UnknownMember`; `Unknown` spelling
/// a known member is `Malformed` because it cannot round-trip as that variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionKind {
    Permission,
    Tripwire,
    GateInconclusive,
    HumanDecides,
    Budget,
    /// An unfamiliar member, retained exactly as it appeared on the wire.
    Unknown(String),
}

impl DecisionKind {
    /// Returns the exact wire string, including an unfamiliar value verbatim.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Permission => "permission",
            Self::Tripwire => "tripwire",
            Self::GateInconclusive => "gate_inconclusive",
            Self::HumanDecides => "human_decides",
            Self::Budget => "budget",
            Self::Unknown(value) => value,
        }
    }
}

impl<'de> Deserialize<'de> for DecisionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "permission" => Self::Permission,
            "tripwire" => Self::Tripwire,
            "gate_inconclusive" => Self::GateInconclusive,
            "human_decides" => Self::HumanDecides,
            "budget" => Self::Budget,
            _ => Self::Unknown(value),
        })
    }
}

/// Unknown fields on a payload are never rejected and never dropped (issue
/// onsager-ai/ethogram#12): a sink that forwards an event it does not fully understand must be
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

/// Closes a run and carries the runtime's own computed totals.
///
/// `costUsd` and `usage` here are the runtime's own reckoning for the run as
/// a whole, computed once at the point the run ends. The two fields are not
/// interchangeable in how a consumer would reconstruct them from
/// `agent.completed`, and that asymmetry is worth stating plainly rather
/// than leaving it to be discovered: `usage` needs no special handling,
/// because the harness reports token counts per invocation, so summing
/// `agent.completed.usage` across every completion in the run agrees with
/// this field, exactly as it does for `turns` and `durationMs`. `costUsd`
/// does not, because the harness instead reports cost as a running total
/// for the harness session that produced it — `agent.completed.costUsd` is
/// cumulative per `sessionId` rather than per invocation, and naively
/// summing it over every `agent.completed` in a run over-counts whenever a
/// session reports more than once. Reconstructing it therefore needs the
/// maximum observed within each `sessionId`, summed only across distinct
/// sessions (see [`AgentCompletedPayload`]'s doc comments for why). This
/// payload is the number to trust for the run either way.
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
    /// Number of turns *this invocation* took (the actual, not the ceiling
    /// bound in `RunCeilings.turns`). Like `usage` and `duration_ms` below
    /// and unlike `cost_usd`, this is per invocation rather than cumulative
    /// per session, so it is safe to sum across every `agent.completed` in
    /// a run.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub turns: Option<u64>,
    /// Echoes the harness session identifier `agent.started` already
    /// carries, so this completion can state which session's running cost
    /// total it is reporting. `cost_usd` below is cumulative per session
    /// rather than per invocation — unlike `usage` beside it, see its doc
    /// comment for why — and that rule was unusable from a completion alone
    /// before this field existed: `sessionId` appeared only on
    /// `agent.started`, so a consumer had to correlate backwards to
    /// whichever `agent.started` opened the session before it could safely
    /// take a maximum within a session or sum across sessions. Carrying it
    /// here too makes the rule applicable from the very event that states
    /// the cost total it governs.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_id: Option<String>,
    /// Cumulative for the harness session named by this payload's own
    /// `session_id` (which echoes the `sessionId` on the `agent.started`
    /// that opened it), not per invocation: this is the running total as of
    /// *this* completion, so a session that reports `agent.completed` more
    /// than once reports an increasing total each time rather than a fresh
    /// delta. Summing every `agent.completed.costUsd` in a run therefore
    /// over-counts whenever a session reports more than once — take the
    /// maximum observed within each `sessionId` instead, and sum only across
    /// distinct sessions.
    ///
    /// This is genuinely asymmetric with `usage` immediately below, which
    /// sums cleanly across invocations with no such caveat: the harness
    /// reports cost as a running total for the whole session but reports
    /// token counts per invocation, and each field here only ever reflects
    /// what the harness itself reports. `run.finished.costUsd` carries the
    /// runtime's own computed total for the whole run and is the number to
    /// trust there.
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
    /// Unlike `cost_usd` just above, this carries no cumulative-per-session
    /// caveat: the harness reports token counts per invocation rather than
    /// as a running session total, so this is a fresh delta each time, and
    /// summing every `agent.completed.usage` in a run agrees with
    /// `run.finished.usage`, which still carries the runtime's own computed
    /// total for the run and remains the number to trust there. Do not
    /// assume this field behaves like `cost_usd` merely because they sit
    /// next to each other and share a `sessionId` — the harness reports the
    /// two totals on different bases, and this field's rule follows from
    /// that, not from any pattern shared with its neighbour.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<RunUsage>,
    /// Wall time *this invocation* took. Like `turns` and `usage` above and
    /// unlike `cost_usd`, this is per invocation rather than cumulative per
    /// session, so it is safe to sum across every `agent.completed` in a
    /// run.
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

/// Requests that the runtime interrupt, steer, or answer a waiting decision.
/// Emitted by the run's runtime, never by the console: a console that shows a run as
/// interrupted before the corresponding `control.applied` arrives has
/// misread the protocol.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlRequestedPayload {
    pub control_id: String,
    pub kind: ControlKind,
    /// Required for `answer` and absent for every other kind, at validation.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub decision_id: Option<String>,
    /// Required for `answer` and absent for every other kind, at validation.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub option_id: Option<String>,
    /// For `steer`, the message queued for the run's next turn. Bounded at
    /// capture to `MAX_EXCERPT_SCALARS`, per `truncated` below. Absent on `answer`.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub text: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
    /// The principal identity that made the request.
    pub by: String,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

/// Records whether a `control.requested` request was honoured. Emitted by
/// the run's runtime, never by the console. For an `interrupt`, `run.finished`
/// with `outcome: "interrupted"` is emitted after this event, not before.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlAppliedPayload {
    pub control_id: String,
    pub ok: bool,
    /// The principal identity that applied the control, with the same meaning
    /// as `by` on `control.requested`: an identity a consumer renders and
    /// never interprets. Optional, because a runtime echoing a control it does
    /// not support may have no separate applier to name (onsager-ai/ethogram#67).
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub by: Option<String>,
    /// Required when `ok` is false; also permitted on a positive echo.
    /// Unknown explanations are bounded at capture, per `truncated` below.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<ControlAppliedReason>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
    /// For an `interrupt`, the `toolUseId` the kill landed inside, if any.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub landed_in: Option<String>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

/// Records an event refused by a relay or capturing runtime on that runtime's
/// own run. It names the source run without embedding the refused content,
/// whose size may be the reason for refusal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRefusedPayload {
    pub cause: CaptureRefusalCause,
    pub source_run_id: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_seq: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_type: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub field: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub count: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_safe_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub max: Option<u64>,
    /// A bounded, excerpted parser or validation message for `malformed`.
    /// `truncated` records whether it was excerpted.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub detail: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

/// Bounded narration supplied with a `decision.requested`. All four content
/// fields are checked against [`MAX_EXCERPT_SCALARS`] by [`validate`], while
/// parsing enforces only representability so a forwarder can still carry an
/// over-bound dossier. `truncated` applies to the dossier as a whole rather
/// than to each narration field separately.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDossier {
    pub question: String,
    pub options_ruled_out: Vec<String>,
    pub recommended_action: String,
    pub blast_radius: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

/// One answer a human may choose for a `decision.requested`. `id` is an
/// unbounded identifier; `label` is bounded narration checked by [`validate`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

/// Opens a decision owned by this run. The dossier and option labels carry
/// bounded narration; `subject` is only a reference such as a PR URL, issue
/// number, or tool name, never the subject's content.
///
/// `on_timeout` is enforced rather than conventional: it is allowed only for
/// `permission`, its only permitted value is `deny`, and that value must name
/// one of this request's own `options[].id` — a request cannot declare a
/// timeout action it never offered. A tripwire that auto-proceeded on timeout
/// would violate ostrom's "never auto-proceed" rule; enforcing the
/// restriction in [`validate`] prevents a producer from shipping that mistake
/// quietly. [`parse_event`] deliberately does not apply this policy, because
/// a forwarder must retain any representable request.
///
/// The corresponding `decision.answered` is not emitted on this request's own
/// run: it is emitted later by whatever invocation applies the answer, on
/// that invocation's own run, by which point this run has usually already
/// finished. The two events are correlated only by `decision_id`, never by
/// sharing a `run_id`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRequestedPayload {
    /// Producer-assigned and unique within the run.
    pub decision_id: String,
    pub kind: DecisionKind,
    pub dossier: DecisionDossier,
    pub options: Vec<DecisionOption>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub subject: Option<String>,
    /// Optional ISO-8601 expiry time.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<String>,
    /// Option id applied after expiry. Absence leaves the decision open.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub on_timeout: Option<String>,
    #[serde(flatten)]
    pub extra: PayloadExtension,
}

/// Records an answer after it has been applied. It is emitted by **the
/// invocation that applies the answer, on its own run** — not the run that
/// requested the decision, which has usually already finished by the time a
/// human responds. The two events are correlated only by `decision_id`, never
/// by sharing a `run_id`; it is never emitted by a console that merely
/// collected the answer.
///
/// This wording is a correction (ruled on onsager-ai/ethogram#7). The previous wording said this
/// event was emitted by the run that owns the decision, but that describes
/// something the protocol's own rules forbid: a run has at most one
/// `run.finished`, and a sink refuses every append to a closed run. A
/// decision a human answers minutes or hours later is answered after the
/// requesting run has terminated, so an answer emitted "on the owning run"
/// would be refused by the sink. `requested_run_id`, below, exists because of
/// this correction: once the two events routinely live on different runs, a
/// consumer holding only the answer needs a way to find the run that asked.
///
/// `by_timeout` is semantically material. Without it, a human choosing
/// `option_id: "deny"` is indistinguishable from a permission expiring
/// unanswered with the same option and a runtime principal in `by`. A timeout
/// is not a decision with a long gap; it is nobody deciding. Absence means
/// false. `reversal` names the identifier that would undo this answer; see
/// its own doc comment below for the two forms it may take and why neither is
/// checked against the request.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAnsweredPayload {
    pub decision_id: String,
    pub option_id: String,
    /// A principal identity a consumer resolves, never a display name.
    pub by: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub by_timeout: Option<bool>,
    /// The identifier that would undo this answer, if the producer accepts
    /// one. Two forms (ruled on onsager-ai/ethogram#7):
    ///
    /// - an offered `options[].id`, or
    /// - a `<verb>:<subject>` **action id** — `revoke:required_checks` undoes
    ///   `excuse:required_checks`, even though `revoke:required_checks` was
    ///   never among the options offered to the human, because those options
    ///   were about whether to excuse, not about how to later revoke.
    ///
    /// Either form is meaningful only because **the producer accepts its own
    /// reversal ids as a subsequent `option_id` on this decision** — that
    /// acceptance is what makes an unoffered id legible rather than
    /// arbitrary. It follows that `reversal` is therefore not checkable
    /// against the request: `validate_decision_answer_against_request` does
    /// not check it. The alternative — requiring membership in
    /// `options[].id` — would refuse a legitimate undo that the producer will
    /// honour, which is worse than not checking at all. `validate` still only
    /// checks that, when present, this is a string.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub reversal: Option<String>,
    /// The run that emitted the corresponding `decision.requested`.
    /// `decision_id` correlates the pair, but a consumer holding only the
    /// answer cannot find the asking run without this field — and now that
    /// the two events live on different runs, that lookup is the common case
    /// rather than an edge one.
    #[serde(
        default,
        deserialize_with = "deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_run_id: Option<String>,
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
/// and are tolerated rather than rejected (issue onsager-ai/ethogram#12): recognised payloads do
/// not carry `deny_unknown_fields`, so an unfamiliar field does not fail
/// representability checking here, and each payload's `#[serde(flatten)]`
/// extension field
/// means a caller who deserialises directly into a typed struct (bypassing this
/// function's `Value` payload) still gets it back on re-serialisation rather
/// than silently losing it. Only the envelope stays closed to unknown fields,
/// via the `deny_unknown_fields` still present on `Event` and `EventDraft`.
pub fn parse_event(input: &str) -> serde_json::Result<Event> {
    let event: Event = serde_json::from_str(input)?;
    validate_payload_numbers(&event.payload, "payload")?;
    check_known_payload_representation(&event.event_type, &event.payload)?;
    Ok(event)
}

/// One canonical event fixture from the version 1 conformance corpus,
/// compiled into this crate by `build.rs` from `conformance/v1`.
///
/// Restored here — at the crate root, not as a separate crate — after
/// onsager-ai/ethogram#70 deleted the standalone `ethogram-corpus` crate on a
/// "zero dependents" finding that was wrong: umwelt depends on it three ways.
/// See [`v1_fixtures`] and the `CHANGELOG.md` entry for the full account.
///
/// `conformance/v1` lives outside this crate's own directory, so compiling
/// this in depends on the repository layout around `crates/ethogram` — fine
/// for a git dependency, a path dependency, or the vendored tree, all of
/// which keep that layout intact, but it would break a crates.io publish,
/// where files outside the package root are not included. Nothing here is
/// published today (`publish = false`; onsager-ai/ethogram#10 records why),
/// so this is a caveat for a future publisher to meet, not a present defect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fixture {
    /// The fixture's file name within `conformance/v1`.
    pub name: &'static str,
    /// The fixture file's exact UTF-8 text, including presentation
    /// whitespace, byte-identical to the file on disk.
    pub raw_json: &'static str,
}

impl Fixture {
    /// Parses the fixture through this SDK's own [`parse_event`].
    ///
    /// The committed corpus is expected to make this infallible; the result
    /// is retained rather than unwrapped here so a malformed compiled-in or
    /// canonical input is never hidden from a caller that checks it.
    pub fn parse(&self) -> serde_json::Result<Event> {
        parse_event(self.raw_json)
    }
}

include!(concat!(env!("OUT_DIR"), "/corpus_fixtures.rs"));

/// Returns every version 1 fixture in UTF-8 byte-order by file name, compiled
/// in by `build.rs` from `conformance/v1` — see [`Fixture`] for the drift and
/// publish caveats. `crates/ethogram/tests/corpus_accessor.rs` asserts this
/// matches the directory exactly: same names, same bytes, same order, same
/// count.
#[must_use]
pub fn v1_fixtures() -> &'static [Fixture] {
    V1_FIXTURES
}

/// Validates whether a producer should emit `payload` for `event_type`.
///
/// Every union carrying a typed `Unknown` must use its known variant for a
/// known spelling. Otherwise the value is `Malformed`: it cannot round-trip
/// as itself. This representability check runs before JSON conversion erases
/// the variant, including for the open [`ControlAppliedReason`] union.
/// Closedness is separate: unfamiliar strings are `UnknownMember` only for
/// [`RunKind`], [`RunOutcome`], [`ControlKind`], [`CaptureRefusalCause`], and
/// [`DecisionKind`]. Both sets are declared in `union_unknown_validation`.
///
/// After the typed check, two universal bounds (issue onsager-ai/ethogram#28) apply to **every** event
/// regardless of whether `event_type` is recognised: every string leaf
/// anywhere in the payload — nested objects at any depth, strings inside
/// arrays, strings inside objects nested inside arrays, and a known type's
/// own retained unknown fields alike — is at most [`MAX_TEXT_SCALARS`]
/// Unicode scalar values, and the payload's canonical compact serialisation
/// is at most [`MAX_PAYLOAD_BYTES`] bytes. Without these, a producer emitting
/// an unrecognised `type` carrying an unbounded payload validated cleanly,
/// because an unrecognised type otherwise has no bound applied to it at all
/// — this is the hole closed here.
///
/// Required fields, known closed-union membership, safe-integer bounds, and
/// the tighter per-field capture bounds are enforced only for known event
/// types; an unrecognised type is checked against the two universal bounds
/// above and nothing else.
///
/// `parse_event` answers "can both SDKs carry this?"; `validate` answers
/// "should a producer have emitted this?" `parse_event` therefore does not
/// call this function, nor either universal bound directly: a representable
/// over-bound event must remain forwardable.
pub fn validate<P>(event_type: &str, payload: &P) -> Result<(), ValidationError>
where
    P: Serialize + ?Sized,
{
    // Inspect typed Unknown variants before JSON conversion erases them.
    union_unknown_validation::check_representability(event_type, payload)?;
    // The payload itself failed to serialise: a representation failure, not
    // a stated rule broken by an otherwise representable value.
    let payload = serde_json::to_value(payload)
        .map_err(|error| ValidationError::malformed("payload", error.to_string()))?;

    // Universal bounds: run before the known-type branch below, and for
    // every event including one of an unrecognised type (issue onsager-ai/ethogram#28).
    validate_payload_text_scalars(&payload, "payload")?;
    validate_payload_size(&payload)?;

    if !KNOWN_TYPES.contains(&event_type) {
        return Ok(());
    }

    validate_payload_numbers(&payload, "payload")?;

    if event_type == RUN_STARTED {
        let started = decode_payload::<RunStartedPayload>(payload)?;
        union_unknown_validation::check_closedness(event_type, started.kind.as_str())?;
    } else if event_type == RUN_FINISHED {
        let finished = decode_payload::<RunFinishedPayload>(payload)?;
        union_unknown_validation::check_closedness(event_type, finished.outcome.as_str())?;
        validate_scalar_bound(
            finished.reason.as_deref(),
            "RunFinishedPayload.reason",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == AGENT_STARTED {
        decode_payload::<AgentStartedPayload>(payload).map(drop)?;
    } else if event_type == AGENT_TEXT {
        let text = decode_payload::<AgentTextPayload>(payload)?;
        validate_scalar_bound(Some(&text.text), "AgentTextPayload.text", MAX_TEXT_SCALARS)?;
    } else if event_type == AGENT_TOOL_USE {
        let tool_use = decode_payload::<AgentToolUsePayload>(payload)?;
        validate_scalar_bound(
            tool_use.input_excerpt.as_deref(),
            "AgentToolUsePayload.inputExcerpt",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == AGENT_TOOL_RESULT {
        let tool_result = decode_payload::<AgentToolResultPayload>(payload)?;
        validate_scalar_bound(
            tool_result.result_excerpt.as_deref(),
            "AgentToolResultPayload.resultExcerpt",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == AGENT_COMPLETED {
        decode_payload::<AgentCompletedPayload>(payload).map(drop)?;
    } else if event_type == AGENT_WARNING {
        let warning = decode_payload::<AgentWarningPayload>(payload)?;
        validate_scalar_bound(
            Some(&warning.message),
            "AgentWarningPayload.message",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == CONTROL_REQUESTED {
        let requested = decode_payload::<ControlRequestedPayload>(payload)?;
        // Checked before any kind-conditioned business rule below, exactly
        // as every other closed union checks membership before its own
        // conditioned rules: those rules (decisionId/optionId only for
        // "answer", text required for "steer") only have anything to say
        // about a kind this SDK recognises.
        union_unknown_validation::check_closedness(event_type, requested.kind.as_str())?;
        for (field, value) in [
            ("decisionId", &requested.decision_id),
            ("optionId", &requested.option_id),
        ] {
            if requested.kind == ControlKind::Answer {
                if value.is_none() {
                    return Err(ValidationError::new(
                        ValidationErrorKind::MissingField {
                            path: format!("payload.{field}"),
                        },
                        format!(
                            "ControlRequestedPayload.{field} is required when kind is \"answer\""
                        ),
                    ));
                }
            } else if value.is_some() {
                return Err(ValidationError::policy(
                    format!("payload.{field}"),
                    format!(
                        "ControlRequestedPayload.{field} is permitted only when kind is \"answer\""
                    ),
                ));
            }
        }
        if requested.kind == ControlKind::Answer && requested.text.is_some() {
            return Err(ValidationError::policy(
                "payload.text",
                "ControlRequestedPayload.text must be absent when kind is \"answer\"",
            ));
        }
        // A `steer` is an instruction queued for the run's next turn; one
        // carrying nothing to say is a producer error. This is policy, not
        // representability, so it lives here and not in `parse_event` (see
        // that function's doc comment): a steer with no text is perfectly
        // representable, and a forwarder must still be able to relay it.
        // An absent `text` and a present-but-empty one are the same defect,
        // so both are rejected identically.
        if requested.kind == ControlKind::Steer
            && requested.text.as_deref().unwrap_or("").is_empty()
        {
            return Err(ValidationError::policy(
                "payload.text",
                "ControlRequestedPayload.text is required and must not be empty when kind is \"steer\": a steer with nothing to say is a producer error",
            ));
        }
        validate_scalar_bound(
            requested.text.as_deref(),
            "ControlRequestedPayload.text",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == CONTROL_APPLIED {
        let applied = decode_payload::<ControlAppliedPayload>(payload)?;
        if !applied.ok && applied.reason.is_none() {
            return Err(ValidationError::new(
                ValidationErrorKind::MissingField {
                    path: "payload.reason".to_owned(),
                },
                "ControlAppliedPayload.reason is required when ok is false",
            ));
        }
        if let Some(reason) = &applied.reason {
            union_unknown_validation::check_closedness(event_type, reason.as_str())?;
        }
        if let Some(ControlAppliedReason::Unknown(reason)) = &applied.reason {
            validate_scalar_bound(
                Some(reason),
                "ControlAppliedPayload.reason",
                MAX_EXCERPT_SCALARS,
            )?;
        }
    } else if event_type == CAPTURE_REFUSED {
        let refused = decode_payload::<CaptureRefusedPayload>(payload)?;
        union_unknown_validation::check_closedness(event_type, refused.cause.as_str())?;
        validate_scalar_bound(
            refused.detail.as_deref(),
            "CaptureRefusedPayload.detail",
            MAX_EXCERPT_SCALARS,
        )?;
    } else if event_type == DECISION_REQUESTED {
        let requested = decode_payload::<DecisionRequestedPayload>(payload)?;
        union_unknown_validation::check_closedness(event_type, requested.kind.as_str())?;

        if let Some(on_timeout) = requested.on_timeout.as_deref() {
            if requested.kind != DecisionKind::Permission {
                return Err(ValidationError::policy(
                    "payload.onTimeout",
                    format!(
                        "DecisionRequestedPayload.onTimeout is permitted only when kind is \"permission\"; received kind \"{}\"",
                        requested.kind.as_str()
                    ),
                ));
            }
            if on_timeout != "deny" {
                return Err(ValidationError::policy(
                    "payload.onTimeout",
                    format!(
                        "DecisionRequestedPayload.onTimeout must be \"deny\" when kind is \"permission\"; received \"{on_timeout}\""
                    ),
                ));
            }
            if !requested
                .options
                .iter()
                .any(|option| option.id == on_timeout)
            {
                return Err(ValidationError::policy(
                    "payload.onTimeout",
                    format!(
                        "DecisionRequestedPayload.onTimeout must name one of the request's options[].id; received \"{on_timeout}\""
                    ),
                ));
            }
        }

        validate_scalar_bound(
            Some(&requested.dossier.question),
            "DecisionRequestedPayload.dossier.question",
            MAX_EXCERPT_SCALARS,
        )?;
        for (index, ruled_out) in requested.dossier.options_ruled_out.iter().enumerate() {
            validate_scalar_bound(
                Some(ruled_out),
                &format!("DecisionRequestedPayload.dossier.optionsRuledOut[{index}]"),
                MAX_EXCERPT_SCALARS,
            )?;
        }
        validate_scalar_bound(
            Some(&requested.dossier.recommended_action),
            "DecisionRequestedPayload.dossier.recommendedAction",
            MAX_EXCERPT_SCALARS,
        )?;
        validate_scalar_bound(
            Some(&requested.dossier.blast_radius),
            "DecisionRequestedPayload.dossier.blastRadius",
            MAX_EXCERPT_SCALARS,
        )?;
        for (index, option) in requested.options.iter().enumerate() {
            validate_scalar_bound(
                Some(&option.label),
                &format!("DecisionRequestedPayload.options[{index}].label"),
                MAX_EXCERPT_SCALARS,
            )?;
        }
    } else if event_type == DECISION_ANSWERED {
        decode_payload::<DecisionAnsweredPayload>(payload).map(drop)?;
    }

    Ok(())
}

/// Checks the consistency that is visible only when a
/// [`DecisionRequestedPayload`] and [`DecisionAnsweredPayload`] are available
/// together. This is intentionally separate from [`validate`] and
/// [`parse_event`]: the two events are independent on the wire, and a
/// forwarder handling one has not necessarily observed the other.
///
/// The chosen `option_id` must be present in the request's options. The sole
/// exception is the request's `on_timeout` value when `by_timeout` is true.
///
/// This exception has not become dead weight now that [`validate`] requires
/// `on_timeout` to name an existing option: this function never calls
/// `validate`, so it has no way to know whether the `request` it was handed
/// ever passed that check. A request forwarded without validation, or
/// emitted by a producer written before the rule existed, can still reach
/// here with an `on_timeout` absent from its own `options` — the same shape
/// [`parse_event`] deliberately still accepts. The exception is what lets a
/// genuine timeout answer against such a request validate correctly instead
/// of being misreported as an unrecognised option.
///
/// The two `decision_id` values must match. `reversal`, when present, is
/// **not** checked here (ruled on onsager-ai/ethogram#7): it may name either an offered option
/// or a `<verb>:<subject>` action id the producer accepts as a later answer
/// to this same decision, and only the producer knows which action ids it
/// accepts — see [`DecisionAnsweredPayload::reversal`]'s doc comment for why
/// checking it against `options[].id` would refuse a legitimate undo.
pub fn validate_decision_answer_against_request(
    request: &DecisionRequestedPayload,
    answer: &DecisionAnsweredPayload,
) -> serde_json::Result<()> {
    if request.decision_id != answer.decision_id {
        return Err(de::Error::custom(format_args!(
            "DecisionAnsweredPayload.decisionId does not match request: expected \"{}\"; received \"{}\"",
            request.decision_id, answer.decision_id
        )));
    }

    let option_exists = request
        .options
        .iter()
        .any(|option| option.id == answer.option_id);
    let is_timeout_option = answer.by_timeout == Some(true)
        && request.on_timeout.as_deref() == Some(answer.option_id.as_str());
    if !option_exists && !is_timeout_option {
        return Err(de::Error::custom(format_args!(
            "DecisionAnsweredPayload.optionId does not name a request option: \"{}\"",
            answer.option_id
        )));
    }

    Ok(())
}

fn validate_scalar_bound(
    value: Option<&str>,
    field: &str,
    maximum: usize,
) -> Result<(), ValidationError> {
    let Some(value) = value else {
        return Ok(());
    };
    let actual = value.chars().count();
    if actual > maximum {
        Err(ValidationError::new(
            ValidationErrorKind::OverBound {
                path: payload_path(field),
                count: actual,
                max: maximum,
            },
            format!("{field} has {actual} Unicode scalar values; maximum is {maximum}"),
        ))
    } else {
        Ok(())
    }
}

// Convert a known field label, never diagnostic text, into its wire path.
fn payload_path(field: &str) -> String {
    let (_, suffix) = field
        .split_once('.')
        .expect("known field has a payload type prefix");
    format!("payload.{suffix}")
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
        decode_payload::<RunStartedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == RUN_FINISHED {
        decode_payload::<RunFinishedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == AGENT_STARTED {
        decode_payload::<AgentStartedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == AGENT_TEXT {
        decode_payload::<AgentTextPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == AGENT_TOOL_USE {
        decode_payload::<AgentToolUsePayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == AGENT_TOOL_RESULT {
        decode_payload::<AgentToolResultPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == AGENT_COMPLETED {
        decode_payload::<AgentCompletedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == AGENT_WARNING {
        decode_payload::<AgentWarningPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == CONTROL_REQUESTED {
        decode_payload::<ControlRequestedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == CONTROL_APPLIED {
        decode_payload::<ControlAppliedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == CAPTURE_REFUSED {
        decode_payload::<CaptureRefusedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == DECISION_REQUESTED {
        decode_payload::<DecisionRequestedPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
    } else if event_type == DECISION_ANSWERED {
        decode_payload::<DecisionAnsweredPayload>(payload.clone())
            .map(drop)
            .map_err(Into::into)
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
/// that `1.0` and `1` produce identical bytes (issue onsager-ai/ethogram#9). This matches the
/// TypeScript SDK, where `JSON.stringify` already collapses `1.0` to `1`.
///
/// Every remaining `f64` — non-integral values, and integral values at or
/// above 2^53 that the rule above leaves as floats — is laid out in
/// ECMAScript's `Number::toString` notation rather than `serde_json`'s own
/// (issue onsager-ai/ethogram#9): plain decimal when the value's decimal exponent falls in
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

/// Serialises a validation error as its kind and fields, using the event
/// payload serialiser's UTF-8 key ordering and ECMAScript number notation.
pub fn serialise_validation_error(error: &ValidationError) -> serde_json::Result<String> {
    let bytes = serialise_payload_canonical(&serde_json::to_value(error)?)?;
    Ok(String::from_utf8(bytes).expect("a JSON serialiser only ever writes valid UTF-8"))
}

/// Recursively validates that every integral-valued number in `value` is
/// within the safe-integer magnitude bound, naming the offending path (for
/// example `payload.nested.count` or `payload.items[2].total`) when the
/// check fails. Non-integral numbers are never bounded, no matter how large
/// their magnitude. Mirrors `deserialize_seq` and reuses the same bound
/// (issue onsager-ai/ethogram#9): a value that needs more precision must be carried as a string
/// instead of a number.
fn validate_payload_numbers(value: &Value, path: &str) -> Result<(), ValidationError> {
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
                // Out of safe-integer range: a representation failure, not a
                // stated rule broken by an otherwise representable value.
                Err(ValidationError::malformed(
                    path,
                    format!(
                        "{path} is an integral number whose magnitude exceeds the safe integer bound: actual {number}; maximum {MAX_SAFE_INTEGER_MAGNITUDE}; a value that needs more precision must be carried as a string"
                    ),
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
/// `f64::MAX` alike; neither is special-cased, per the ruling in issue onsager-ai/ethogram#9.
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

/// Recursively validates that every string leaf in `value` is at most
/// [`MAX_TEXT_SCALARS`] Unicode scalar values (issue onsager-ai/ethogram#28), naming the
/// offending path (for example `payload.nested.note` or
/// `payload.items[2].note`) when the check fails. Mirrors
/// [`validate_payload_numbers`] exactly, walking nested objects at any depth,
/// strings inside arrays, and strings inside objects nested inside arrays —
/// and, because it walks the raw [`Value`] before any known-type struct is
/// deserialised out of it, a known type's own retained unknown fields (its
/// `extra: PayloadExtension`) are covered by the same walk rather than
/// needing a separate pass.
///
/// [`validate`] calls this unconditionally, before branching on whether
/// `event_type` is recognised, so an unrecognised type is covered by the
/// same floor as a known one instead of going unchecked.
fn validate_payload_text_scalars(value: &Value, path: &str) -> Result<(), ValidationError> {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                validate_payload_text_scalars(child, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_payload_text_scalars(item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::String(text) => {
            let actual = text.chars().count();
            if actual > MAX_TEXT_SCALARS {
                Err(ValidationError::new(
                    ValidationErrorKind::OverBound {
                        path: path.to_owned(),
                        count: actual,
                        max: MAX_TEXT_SCALARS,
                    },
                    format!(
                        "{path} has {actual} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}"
                    ),
                ))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// Serialises `payload` alone — never wrapped in an envelope — into the same
/// canonical compact form [`serialise_event`] would produce for it: sorted
/// object keys and ECMAScript-notation numbers via [`EcmaScriptFormatter`],
/// after passing through [`canonicalise_numbers`]. Shared by
/// [`validate_payload_size`] so the bytes it measures match what a producer
/// would actually put on the wire for this payload.
fn serialise_payload_canonical(payload: &Value) -> serde_json::Result<Vec<u8>> {
    let canonical = canonicalise_numbers(payload.clone());
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, EcmaScriptFormatter);
    serde::Serialize::serialize(&canonical, &mut serializer)?;
    Ok(bytes)
}

/// Validates that `payload` alone — not the envelope around it — serialises
/// to at most [`MAX_PAYLOAD_BYTES`] UTF-8 bytes in its canonical compact form
/// (issue onsager-ai/ethogram#28). See [`MAX_PAYLOAD_BYTES`]'s own doc comment for why this
/// bound and [`MAX_TEXT_SCALARS`] do not collide.
///
/// [`validate`] calls this unconditionally, before branching on whether
/// `event_type` is recognised, so an unrecognised type is covered by the
/// same floor as a known one instead of going unchecked.
fn validate_payload_size(payload: &Value) -> Result<(), ValidationError> {
    // A serialisation failure: a representation failure, not a stated rule
    // broken by an otherwise representable value.
    let bytes = serialise_payload_canonical(payload).map_err(|error| {
        ValidationError::malformed(
            "payload",
            format!("payload could not be serialised to measure its size: {error}"),
        )
    })?;
    let actual = bytes.len();
    if actual > MAX_PAYLOAD_BYTES {
        Err(ValidationError::new(
            ValidationErrorKind::PayloadTooLarge {
                bytes: actual,
                max: MAX_PAYLOAD_BYTES,
            },
            format!("payload has {actual} bytes; maximum is {MAX_PAYLOAD_BYTES}"),
        ))
    } else {
        Ok(())
    }
}

/// The largest magnitude at which every integer is exactly representable as
/// an `f64`, per the ruling in issue onsager-ai/ethogram#9.
const MAX_SAFE_INTEGRAL_MAGNITUDE: f64 = 9_007_199_254_740_992.0;

/// Recursively rewrites integral-valued floats as integers, per issue onsager-ai/ethogram#9.
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
/// into ECMAScript's `Number::toString` notation (issue onsager-ai/ethogram#9), leaving every
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
///   this is the plain-decimal band the ruling in issue onsager-ai/ethogram#9 is about, since
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
    struct Present<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Present<T> {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
            formatter.write_str("a non-null value")
        }

        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            T::deserialize(deserializer).map(Some)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            // Direct serde callers still reject null through T's visitor.
            T::deserialize(de::value::UnitDeserializer::new()).map(Some)
        }
    }

    // Expose optionality to the located decoder without accepting null as
    // None. Absence is supplied only by the field's #[serde(default)].
    deserializer.deserialize_option(Present(std::marker::PhantomData))
}

/// Deserializes a `u64` and rejects a magnitude beyond
/// `MAX_SAFE_INTEGER_MAGNITUDE`, reusing the same bound and the same
/// `number_exceeds_safe_integer_magnitude` check `validate_payload_numbers`
/// uses (issue onsager-ai/ethogram#9). `u64` deserialization already rejects a negative or
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
    #[derive(Deserialize)]
    struct SafeU64(#[serde(deserialize_with = "deserialize_safe_u64")] u64);

    deserialize_optional::<D, SafeU64>(deserializer).map(|value| value.map(|value| value.0))
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

/// A run has at most one `run.finished`, and a sink that has recorded it
/// refuses later appends and forwards for that run (issues onsager-ai/ethogram#5 and onsager-ai/ethogram#3). This
/// is refused by [`InMemorySink::append_draft`] or
/// [`InMemorySink::append_event`] once that run has recorded a terminal
/// event — including a second `run.finished`, and including any other event
/// type appended afterwards.
///
/// This is a **distinct** failure from [`SequenceError`]: a caller must be
/// able to tell "you skipped a seq" from "this run is closed", because the
/// two call for different responses. Refusing via this error, rather than
/// folding it into `SequenceError`, keeps that distinction visible in the
/// type rather than only in a message a caller might not inspect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunClosedError {
    pub run_id: String,
}

impl Display for RunClosedError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "run {} already recorded a terminal event; no further events are accepted for it",
            self.run_id
        )
    }
}

impl Error for RunClosedError {}

/// Why [`InMemorySink::append_event`] refused an already-stamped event: a
/// gap in `seq` ([`SequenceError`]), or an append to a run that already
/// recorded its terminal event ([`RunClosedError`]). Kept as an enum over
/// folding the two into one error so a caller can match on which happened —
/// "you skipped a seq" and "this run is closed" require different
/// responses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppendError {
    Sequence(SequenceError),
    RunClosed(RunClosedError),
}

impl Display for AppendError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sequence(error) => Display::fmt(error, formatter),
            Self::RunClosed(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for AppendError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sequence(error) => Some(error),
            Self::RunClosed(error) => Some(error),
        }
    }
}

/// A minimal in-memory reference for sequencing rules, not a storage engine.
///
/// Tracks, per run, whether a terminal event (`type` equal to
/// [`RUN_FINISHED`]) has already been appended. Once it has, every further
/// append for that run is refused via [`RunClosedError`] — both
/// [`InMemorySink::append_draft`] and [`InMemorySink::append_event`], and
/// regardless of the later event's own type, so a `run.finished` followed by
/// an `agent.text` is refused exactly as a second `run.finished` would be.
/// This makes the assumption issue onsager-ai/ethogram#5 rests its "simpler to fold and to
/// prove terminal" argument on — that a run has at most one terminal event —
/// something this sink actually enforces rather than merely hopes for.
pub struct InMemorySink<P = Value> {
    clock: Box<dyn FnMut() -> String>,
    runs: HashMap<String, Vec<Event<P>>>,
    finished_runs: std::collections::HashSet<String>,
}

impl<P> InMemorySink<P> {
    pub fn new(clock: impl FnMut() -> String + 'static) -> Self {
        Self {
            clock: Box::new(clock),
            runs: HashMap::new(),
            finished_runs: std::collections::HashSet::new(),
        }
    }

    /// Stamps `draft` and appends it for `run_id`, refusing it with
    /// [`RunClosedError`] if that run already recorded a terminal event. A
    /// refused draft consumes no `seq`: the check runs before `next_seq` is
    /// even consulted, so a closed run's sequence counter is left exactly as
    /// it was.
    pub fn append_draft(
        &mut self,
        run_id: impl Into<String>,
        draft: EventDraft<P>,
    ) -> Result<&Event<P>, RunClosedError> {
        let run_id = run_id.into();
        if self.finished_runs.contains(&run_id) {
            return Err(RunClosedError { run_id });
        }

        let seq = self.next_seq(&run_id);
        let is_terminal = draft.event_type == RUN_FINISHED;
        let event = stamp(
            draft,
            StampFields {
                run_id: run_id.clone(),
                seq,
                ts: (self.clock)(),
            },
        );
        if is_terminal {
            self.finished_runs.insert(run_id.clone());
        }
        let events = self.runs.entry(run_id).or_default();
        events.push(event);
        Ok(events
            .last()
            .expect("the event was inserted immediately before this lookup"))
    }

    /// Appends an already-stamped `event`, refusing it via [`AppendError`]
    /// either for a sequence gap ([`AppendError::Sequence`]) or because the
    /// run already recorded a terminal event ([`AppendError::RunClosed`]).
    /// The run-closed check runs before the sequence check and before
    /// `next_seq` is consulted, so a refused event — for either reason —
    /// consumes no `seq` and leaves the run's stored events unchanged.
    pub fn append_event(&mut self, event: Event<P>) -> Result<&Event<P>, AppendError> {
        if self.finished_runs.contains(&event.run_id) {
            return Err(AppendError::RunClosed(RunClosedError {
                run_id: event.run_id,
            }));
        }

        let expected = self.next_seq(&event.run_id);
        if event.seq != expected {
            return Err(AppendError::Sequence(SequenceError {
                run_id: event.run_id,
                expected,
                received: event.seq,
            }));
        }

        if event.event_type == RUN_FINISHED {
            self.finished_runs.insert(event.run_id.clone());
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

    // Cross-SDK byte identity for the two outcomes added by spec onsager-ai/ethogram#31
    // (onsager-ai/ostrom-hub#146). These exact literals are pasted into the TypeScript
    // suite and asserted there against events hand-built through its typed
    // API.
    const BLOCKED_OUTCOME_WIRE: &str = r#"{"v":1,"type":"run.finished","runId":"run-blocked","seq":1,"ts":"2026-09-07T08:00:00.000Z","payload":{"durationMs":500,"outcome":"blocked","reason":"awaiting-upstream-quota"}}"#;
    const UNSTARTED_OUTCOME_WIRE: &str = r#"{"v":1,"type":"run.finished","runId":"run-unstarted","seq":1,"ts":"2026-09-07T08:00:01.000Z","payload":{"durationMs":0,"outcome":"unstarted","reason":"spawn"}}"#;

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

    // Cross-SDK byte identity for `agent.completed.sessionId` (issue onsager-ai/ethogram#6 on
    // onsager-ai/umwelt#22). This exact literal is pasted into the TypeScript suite and
    // asserted there against an event built from TypeScript's correlated
    // payload union, proving both SDKs agree on the new field's bytes without
    // touching a single existing fixture.
    const AGENT_COMPLETED_WITH_SESSION_WIRE: &str = r#"{"v":1,"type":"agent.completed","runId":"run-agent","seq":7,"ts":"2026-09-07T01:00:07.000Z","payload":{"costUsd":2.5,"durationMs":3200,"estimated":false,"model":"gpt-5","sessionId":"session-local-7","stage":"finish","turns":5,"usage":{"cacheCreationTokens":15,"cacheReadTokens":5,"inputTokens":50,"outputTokens":75,"unit":"weighted-tokens"}}}"#;

    // Cross-SDK byte identity for both `control.*` events (spec onsager-ai/ethogram#8). These
    // exact literals are pasted into the TypeScript suite and asserted there
    // against events built from TypeScript's correlated payload union.
    const CONTROL_REQUESTED_WIRE: &str = r#"{"v":1,"type":"control.requested","runId":"run-control","seq":1,"ts":"2026-09-07T05:00:00.000Z","payload":{"by":"operator","controlId":"control-1","kind":"steer","text":"take point on the next turn","truncated":false}}"#;
    const CONTROL_APPLIED_FAILED_WIRE: &str = r#"{"v":1,"type":"control.applied","runId":"run-control","seq":2,"ts":"2026-09-07T05:00:01.000Z","payload":{"controlId":"control-1","ok":false,"reason":"not-live"}}"#;
    const CONTROL_APPLIED_INTERRUPT_WIRE: &str = r#"{"v":1,"type":"control.applied","runId":"run-control","seq":3,"ts":"2026-09-07T05:00:02.000Z","payload":{"controlId":"control-2","landedIn":"tool-9","ok":true}}"#;

    // This value is intentionally one neither SDK will ever know, matching
    // issue onsager-ai/ethogram#12's own example. Keeping the same literal in both suites proves
    // an older relay retaining an unfamiliar member emits exactly the bytes a
    // future vocabulary-aware SDK would emit.
    const UNKNOWN_CONTROL_KIND_WIRE: &str = r#"{"v":1,"type":"control.requested","runId":"run-cross-version","seq":1,"ts":"2026-09-07T05:00:03.000Z","payload":{"by":"operator","controlId":"control-3","kind":"teleport"}}"#;

    // Cross-SDK byte identity for a fully populated `over_bound` refusal and
    // a minimal `gap` refusal (spec onsager-ai/ethogram#15). These exact literals are pasted into
    // the TypeScript suite and asserted against events hand-built through each
    // SDK's typed API.
    const CAPTURE_REFUSED_OVER_BOUND_WIRE: &str = r#"{"v":1,"type":"capture.refused","runId":"run-relay","seq":1,"ts":"2026-09-07T06:00:00.000Z","payload":{"cause":"over_bound","count":20000,"field":"AgentTextPayload.text","max":16384,"sourceRunId":"run-source","sourceSeq":8,"sourceType":"agent.text"}}"#;
    const CAPTURE_REFUSED_GAP_WIRE: &str = r#"{"v":1,"type":"capture.refused","runId":"run-relay","seq":2,"ts":"2026-09-07T06:00:01.000Z","payload":{"cause":"gap","sourceRunId":"run-source-gap"}}"#;

    // This value is intentionally one neither SDK will ever know. The cause
    // string and the whole canonical event must survive an older relay exactly.
    const UNKNOWN_CAPTURE_REFUSAL_CAUSE_WIRE: &str = r#"{"v":1,"type":"capture.refused","runId":"run-relay","seq":3,"ts":"2026-09-07T06:00:02.000Z","payload":{"cause":"never-a-valid-capture-refusal-cause","sourceRunId":"run-source"}}"#;

    // Cross-SDK byte identity for both decision events (spec onsager-ai/ethogram#7). These exact
    // literals are pasted into the TypeScript suite and asserted against
    // events hand-built through each SDK's typed API.
    const DECISION_REQUESTED_WIRE: &str = r#"{"v":1,"type":"decision.requested","runId":"run-decision","seq":1,"ts":"2026-09-07T07:00:00.000Z","payload":{"decisionId":"decision-1","dossier":{"blastRadius":"one repository","optionsRuledOut":["auto-proceed","discard the request"],"question":"May the run execute the deployment tool?","recommendedAction":"deny unless the operator confirms the target","truncated":false},"expiresAt":"2026-09-07T07:05:00.000Z","kind":"permission","onTimeout":"deny","options":[{"id":"allow","label":"Allow once"},{"id":"deny","label":"Deny"}],"subject":"deploy"}}"#;
    const DECISION_ANSWERED_HUMAN_WIRE: &str = r#"{"v":1,"type":"decision.answered","runId":"run-decision","seq":2,"ts":"2026-09-07T07:01:00.000Z","payload":{"by":"principal:user:alice","decisionId":"decision-1","optionId":"allow"}}"#;
    const DECISION_ANSWERED_TIMEOUT_WIRE: &str = r#"{"v":1,"type":"decision.answered","runId":"run-decision","seq":3,"ts":"2026-09-07T07:05:00.000Z","payload":{"by":"principal:runtime:permission-timeout","byTimeout":true,"decisionId":"decision-1","optionId":"deny","reversal":"allow"}}"#;

    // Cross-SDK byte identity for `decision.answered.requestedRunId` (spec
    // onsager-ai/ethogram#7 correction). This exact literal is pasted into the TypeScript suite
    // and asserted against an event hand-built through each SDK's typed API.
    const DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE: &str = r#"{"v":1,"type":"decision.answered","runId":"run-decision-answer","seq":1,"ts":"2026-09-07T07:10:00.000Z","payload":{"by":"principal:user:alice","decisionId":"decision-1","optionId":"allow","requestedRunId":"run-decision"}}"#;

    // Cross-SDK byte identity for a `<verb>:<subject>` action-id `reversal`
    // (ruled on onsager-ai/ethogram#7): `revoke:required_checks` undoes `excuse:required_checks`
    // even though it was never among the options offered to the human. This
    // exact literal is pasted into the TypeScript suite and asserted against
    // an event hand-built through each SDK's typed API.
    const DECISION_ANSWERED_ACTION_REVERSAL_WIRE: &str = r#"{"v":1,"type":"decision.answered","runId":"run-decision-revoke","seq":1,"ts":"2026-09-07T09:00:00.000Z","payload":{"by":"principal:user:alice","decisionId":"decision-revoke-1","optionId":"excuse:required_checks","reversal":"revoke:required_checks"}}"#;

    // This value is intentionally one neither SDK will ever know. The kind's
    // raw bytes and the whole canonical event must survive an older relay.
    const UNKNOWN_DECISION_KIND_WIRE: &str = r#"{"v":1,"type":"decision.requested","runId":"run-cross-version","seq":1,"ts":"2026-09-07T07:00:03.000Z","payload":{"decisionId":"decision-unknown","dossier":{"blastRadius":"none","optionsRuledOut":[],"question":"Unknown kind?","recommendedAction":"inspect"},"kind":"never-a-valid-decision-kind","options":[]}}"#;

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
    fn every_run_kind_member_carries_a_definition() {
        // Issue onsager-ai/ethogram#64: each member is decided by a fact a producer can check,
        // and the definition lives on the variant so a consumer meets it at
        // the type rather than in a README they may never open. The marker
        // is the phrase that introduces that fact, not the whole sentence —
        // matching the whole thing would make this a formatting assertion
        // someone deletes the first time a reflow breaks it.
        const MARKER: &str = "Decided by";

        let source = include_str!("lib.rs");
        let body = source
            .split_once("pub enum RunKind {")
            .expect("RunKind is declared in this file")
            .1
            .split_once("\n}")
            .expect("the RunKind declaration closes")
            .0;

        // Two independent scans of the same declaration. A scan that goes
        // blind fails open — it would only check the members it happened to
        // find — so the variant list and the wire-string list must agree
        // before either is trusted (the onsager-ai/ethogram#55 lesson, and the onsager-ai/ethogram#57 shape).
        let mut declared: Vec<&str> = Vec::new();
        let mut documented: Vec<&str> = Vec::new();
        let mut doc = String::new();
        for line in body.lines() {
            let line = line.trim();
            if let Some(text) = line.strip_prefix("///") {
                doc.push(' ');
                doc.push_str(text.trim());
            } else if let Some(name) = line.strip_suffix(',') {
                // `Unknown(String)` is the retaining variant, not a member of
                // the vocabulary, so it is deliberately outside this rule.
                if name != "Unknown(String)" {
                    declared.push(name);
                    if doc.contains(MARKER) {
                        documented.push(name);
                    }
                }
                doc.clear();
            } else if !line.starts_with("#[") && !line.is_empty() {
                doc.clear();
            }
        }

        let wire_members: Vec<&str> = source
            .split_once("pub fn as_str(&self) -> &str {")
            .expect("RunKind::as_str is declared in this file")
            .1
            .split_once("\n    }")
            .expect("as_str closes")
            .0
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Self::"))
            .filter_map(|arm| arm.split_once(" =>"))
            .map(|(name, _)| name)
            .filter(|name| *name != "Unknown(value)")
            .collect();

        assert_eq!(
            declared, wire_members,
            "the RunKind variant scan and the as_str scan disagree: declared {declared:?}, \
             as_str {wire_members:?}. One of the two has gone blind rather than a member \
             having been removed"
        );
        assert!(
            !declared.is_empty(),
            "source scan found no RunKind members; the scan has gone blind"
        );

        let undefined: Vec<&str> = declared
            .iter()
            .filter(|name| !documented.contains(name))
            .copied()
            .collect();
        assert!(
            undefined.is_empty(),
            "RunKind members without a definition ({MARKER:?} in their doc comment): {}; \
             every member is decided by a producer-checkable fact (onsager-ai/ethogram#64)",
            undefined.join(", ")
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
            "blocked",
            "unstarted",
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
    fn validate_refuses_the_hub_literal_abandoned() {
        // onsager-ai/ostrom-hub#146: `abandoned` is the hub's own name for `timed-out`
        // under another spelling, and the hub renames it rather than this
        // protocol adopting it. It is refused exactly like any other
        // unrecognised value — this test is what stops someone adding it
        // later by reflex.
        let input = lifecycle_event_input(
            "run.finished",
            json!({ "outcome": "abandoned", "durationMs": 1250 }),
        );

        let event = parse_event(&input).unwrap();
        let parsed: RunFinishedPayload = serde_json::from_value(event.payload.clone()).unwrap();
        assert_eq!(parsed.outcome, RunOutcome::Unknown("abandoned".to_owned()));

        let error = validate(RUN_FINISHED, &event.payload).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RunFinishedPayload.outcome has unknown value: abandoned"),
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
        for (event_type, field, message) in [
            (
                "agent.text",
                "text",
                "AgentTextPayload.text must be a string",
            ),
            (
                "agent.tool_use",
                "tool",
                "AgentToolUsePayload.tool must be a string",
            ),
            (
                "agent.tool_result",
                "tool",
                "AgentToolResultPayload.tool must be a string",
            ),
            (
                "agent.warning",
                "message",
                "AgentWarningPayload.message must be a string",
            ),
        ] {
            let error =
                parse_event(&lifecycle_event_input(event_type, json!({ (field): 7 }))).unwrap_err();
            assert_eq!(error.to_string(), message);
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
    fn validate_leaves_unknown_event_types_open_to_anything_within_the_universal_bounds() {
        // No per-field or closed-union checks apply to an unrecognised type
        // (there is no typed struct to check against), but it is not fully
        // unvalidated any more: the universal bounds (issue onsager-ai/ethogram#28) still run.
        // This payload sits comfortably under both, so it validates cleanly.
        assert!(validate("future.happened", &json!("not-an-object")).is_ok());
    }

    #[test]
    fn validate_reports_every_capture_bound_with_field_actual_and_maximum() {
        let cases = [
            // `agent.text`'s own bound is `MAX_TEXT_SCALARS` — the same value
            // as the universal text-scalar floor (issue onsager-ai/ethogram#28), which runs
            // first in `validate` and so is what actually reports this case;
            // the field-specific `AgentTextPayload.text` check below it is
            // never reached for an over-bound `text`, since nothing over the
            // universal bound can also be under it.
            //
            // That shadowing is a fact about the two constants being equal,
            // not a loosened assertion. If `MAX_TEXT_SCALARS` ever rises above
            // `agent.text`'s own field bound, the field-specific message
            // returns and this expectation must change back to
            // `AgentTextPayload.text`.
            (
                AGENT_TEXT,
                json!({ "text": "😀".repeat(MAX_TEXT_SCALARS + 1) }),
                "payload.text",
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
            (
                CONTROL_REQUESTED,
                json!({
                    "controlId": "control-1",
                    "kind": "steer",
                    "by": "operator",
                    "text": "😀".repeat(MAX_EXCERPT_SCALARS + 1)
                }),
                "ControlRequestedPayload.text",
                MAX_EXCERPT_SCALARS,
            ),
            (
                CONTROL_APPLIED,
                json!({
                    "controlId": "control-1",
                    "ok": false,
                    "reason": "😀".repeat(MAX_EXCERPT_SCALARS + 1)
                }),
                "ControlAppliedPayload.reason",
                MAX_EXCERPT_SCALARS,
            ),
            (
                CAPTURE_REFUSED,
                json!({
                    "cause": "malformed",
                    "sourceRunId": "run-source",
                    "detail": "😀".repeat(MAX_EXCERPT_SCALARS + 1)
                }),
                "CaptureRefusedPayload.detail",
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

        // Reported by the universal text-scalar bound (issue onsager-ai/ethogram#28), which
        // runs before the known-type branch and shares `agent.text`'s own
        // bound value, so it is what actually reports this case. If
        // `MAX_TEXT_SCALARS` ever rises above `agent.text`'s field bound, the
        // field-specific `AgentTextPayload.text` message returns and this
        // expectation must change back.
        assert_eq!(
            error.to_string(),
            "payload.text has 20000 Unicode scalar values; maximum is 16384"
        );
    }

    /// Builds a JSON payload of plain ASCII text spread across ten short,
    /// equal-length keys — each nowhere near [`MAX_TEXT_SCALARS`] on its own
    /// — whose canonical serialisation ([`serialise_payload_canonical`]) is
    /// exactly `target` bytes. Used to hit the [`MAX_PAYLOAD_BYTES`] boundary
    /// exactly, without any single string leaf tripping the scalar bound
    /// instead: ASCII `'a'` never needs escaping, so appending one character
    /// to any field's string always adds exactly one byte to the total.
    fn payload_of_exact_byte_size(target: usize) -> Value {
        const FIELDS: usize = 10;
        let empty = json!({
            "p0": "", "p1": "", "p2": "", "p3": "", "p4": "",
            "p5": "", "p6": "", "p7": "", "p8": "", "p9": "",
        });
        let base = serialise_payload_canonical(&empty).unwrap().len();
        assert!(
            target >= base,
            "target {target} is below the minimal payload size {base} for this scheme"
        );
        let remaining = target - base;
        let per_field = remaining / FIELDS;
        let leftover = remaining % FIELDS;
        assert!(
            per_field < MAX_TEXT_SCALARS,
            "target {target} needs a field longer than MAX_TEXT_SCALARS; raise FIELDS"
        );

        let mut fields = serde_json::Map::new();
        for index in 0..FIELDS {
            let length = per_field + usize::from(index < leftover);
            fields.insert(format!("p{index}"), Value::String("a".repeat(length)));
        }
        let payload = Value::Object(fields);
        assert_eq!(
            serialise_payload_canonical(&payload).unwrap().len(),
            target,
            "payload_of_exact_byte_size construction is wrong"
        );
        payload
    }

    #[test]
    fn validate_rejects_an_unknown_type_with_an_over_long_string_leaf() {
        let payload = json!({ "note": "x".repeat(MAX_TEXT_SCALARS + 1) });
        let error = validate("future.happened", &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "payload.note has {} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}",
                MAX_TEXT_SCALARS + 1
            )
        );
    }

    #[test]
    fn validate_rejects_an_unknown_type_with_an_over_large_serialised_payload() {
        let payload = payload_of_exact_byte_size(MAX_PAYLOAD_BYTES + 1);
        let error = validate("future.happened", &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "payload has {} bytes; maximum is {MAX_PAYLOAD_BYTES}",
                MAX_PAYLOAD_BYTES + 1
            )
        );
    }

    #[test]
    fn validate_rejects_an_over_long_string_in_a_known_types_retained_unknown_field() {
        let payload = json!({
            "text": "ok",
            "note": "x".repeat(MAX_TEXT_SCALARS + 1),
        });
        let error = validate(AGENT_TEXT, &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "payload.note has {} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}",
                MAX_TEXT_SCALARS + 1
            )
        );
    }

    #[test]
    fn validate_locates_an_over_long_string_nested_in_an_object_an_array_and_both() {
        let nested_in_object = json!({ "nested": { "note": "x".repeat(MAX_TEXT_SCALARS + 1) } });
        assert_eq!(
            validate("future.happened", &nested_in_object)
                .unwrap_err()
                .to_string(),
            format!(
                "payload.nested.note has {} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}",
                MAX_TEXT_SCALARS + 1
            )
        );

        let nested_in_array = json!({ "items": ["x".repeat(MAX_TEXT_SCALARS + 1)] });
        assert_eq!(
            validate("future.happened", &nested_in_array)
                .unwrap_err()
                .to_string(),
            format!(
                "payload.items[0] has {} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}",
                MAX_TEXT_SCALARS + 1
            )
        );

        let nested_in_object_in_array =
            json!({ "items": [{ "note": "x".repeat(MAX_TEXT_SCALARS + 1) }] });
        assert_eq!(
            validate("future.happened", &nested_in_object_in_array)
                .unwrap_err()
                .to_string(),
            format!(
                "payload.items[0].note has {} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}",
                MAX_TEXT_SCALARS + 1
            )
        );
    }

    #[test]
    fn validate_accepts_a_string_at_exactly_max_text_scalars_and_rejects_one_more() {
        // Multi-byte characters prove the count is scalars, not bytes.
        let at_bound = json!({ "note": "漢".repeat(MAX_TEXT_SCALARS) });
        assert!(validate("future.happened", &at_bound).is_ok());

        let over_bound = json!({ "note": "漢".repeat(MAX_TEXT_SCALARS + 1) });
        assert_eq!(
            validate("future.happened", &over_bound)
                .unwrap_err()
                .to_string(),
            format!(
                "payload.note has {} Unicode scalar values; maximum is {MAX_TEXT_SCALARS}",
                MAX_TEXT_SCALARS + 1
            )
        );
    }

    #[test]
    fn validate_accepts_a_payload_at_exactly_max_payload_bytes_and_rejects_one_byte_more() {
        let at_bound = payload_of_exact_byte_size(MAX_PAYLOAD_BYTES);
        assert!(validate("future.happened", &at_bound).is_ok());

        let over_bound = payload_of_exact_byte_size(MAX_PAYLOAD_BYTES + 1);
        assert_eq!(
            validate("future.happened", &over_bound)
                .unwrap_err()
                .to_string(),
            format!(
                "payload has {} bytes; maximum is {MAX_PAYLOAD_BYTES}",
                MAX_PAYLOAD_BYTES + 1
            )
        );
    }

    /// Pinned so a future narrowing of [`MAX_PAYLOAD_BYTES`] fails loudly: an
    /// `agent.text` at exactly [`MAX_TEXT_SCALARS`] composed entirely of
    /// astral-plane characters is 16,384 × 4 = 65,536 bytes of text alone,
    /// which the originally proposed 65,536-byte bound would have rejected.
    /// See [`MAX_PAYLOAD_BYTES`]'s doc comment for why the constant is
    /// 131,072 instead.
    #[test]
    fn agent_text_of_exactly_max_text_scalars_astral_plane_characters_validates_cleanly() {
        let payload = json!({ "text": "😀".repeat(MAX_TEXT_SCALARS) });
        assert!(validate(AGENT_TEXT, &payload).is_ok());
    }

    #[test]
    fn parse_event_carries_both_new_over_bound_grounds_that_validate_refuses() {
        let input = lifecycle_event_input(
            "future.happened",
            json!({ "note": "x".repeat(MAX_TEXT_SCALARS + 1) }),
        );
        let event = parse_event(&input).unwrap();
        assert!(validate(&event.event_type, &event.payload).is_err());

        let over_size_payload = payload_of_exact_byte_size(MAX_PAYLOAD_BYTES + 1);
        let input = lifecycle_event_input("future.happened", over_size_payload);
        let event = parse_event(&input).unwrap();
        assert!(validate(&event.event_type, &event.payload).is_err());
    }

    #[test]
    fn every_conformance_v1_fixture_validates_cleanly() {
        let corpus_directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/v1");
        let mut fixture_count = 0usize;
        for entry in std::fs::read_dir(&corpus_directory).expect("conformance/v1 must exist") {
            let entry = entry.expect("directory entry must be readable");
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|value| value == "json") {
                continue;
            }
            fixture_count += 1;
            let source = std::fs::read_to_string(&path).expect("fixture must be readable");
            let event = parse_event(&source).expect("fixture must parse");
            if let Err(error) = validate(&event.event_type, &event.payload) {
                panic!(
                    "fixture {:?} failed validate: {error}",
                    path.file_name().expect("fixture path has a file name")
                );
            }
        }
        assert!(
            fixture_count > 0,
            "expected at least one fixture in conformance/v1"
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
            "sessionId": "session-local-7",
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
        // A known field (`sessionId`) alongside an unknown one (`future`) on
        // the same payload: neither displaces the other.
        assert_eq!(completed.session_id, Some("session-local-7".to_owned()));
        assert_eq!(
            serde_json::to_value(&completed).unwrap()["sessionId"],
            json!("session-local-7")
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

        // Typed Unknown still emits the existing cross-SDK literals; only
        // validation refuses a known spelling carried as that Rust variant.
        let mut retained_started = started;
        retained_started.payload.kind = RunKind::Unknown("relay".to_owned());
        assert_eq!(
            serialise_event(&retained_started).unwrap(),
            RELAY_CEILINGS_WIRE
        );
        let mut retained_finished = finished;
        retained_finished.payload.outcome = RunOutcome::Unknown("capped".to_owned());
        assert_eq!(
            serialise_event(&retained_finished).unwrap(),
            CAPPED_OUTCOME_WIRE
        );
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
    fn blocked_and_unstarted_outcomes_match_the_typescript_pinned_bytes() {
        let blocked = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: RUN_FINISHED.to_owned(),
            run_id: "run-blocked".to_owned(),
            seq: 1,
            ts: "2026-09-07T08:00:00.000Z".to_owned(),
            payload: RunFinishedPayload {
                outcome: RunOutcome::Blocked,
                reason: Some("awaiting-upstream-quota".to_owned()),
                truncated: None,
                cost_usd: None,
                usage: None,
                duration_ms: 500,
                estimated: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let unstarted = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: RUN_FINISHED.to_owned(),
            run_id: "run-unstarted".to_owned(),
            seq: 1,
            ts: "2026-09-07T08:00:01.000Z".to_owned(),
            payload: RunFinishedPayload {
                outcome: RunOutcome::Unstarted,
                reason: Some("spawn".to_owned()),
                truncated: None,
                cost_usd: None,
                usage: None,
                duration_ms: 0,
                estimated: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(serialise_event(&blocked).unwrap(), BLOCKED_OUTCOME_WIRE);
        assert_eq!(serialise_event(&unstarted).unwrap(), UNSTARTED_OUTCOME_WIRE);
    }

    #[test]
    fn blocked_and_unstarted_round_trip_through_parse_and_serialise() {
        for wire in [BLOCKED_OUTCOME_WIRE, UNSTARTED_OUTCOME_WIRE] {
            let event = parse_event(wire).unwrap();
            assert_eq!(serialise_event(&event).unwrap(), wire);
        }
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
                session_id: None,
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
    fn agent_completed_session_id_matches_the_typescript_pinned_bytes() {
        let completed = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: "agent.completed".to_owned(),
            run_id: "run-agent".to_owned(),
            seq: 7,
            ts: "2026-09-07T01:00:07.000Z".to_owned(),
            payload: AgentCompletedPayload {
                stage: Some("finish".to_owned()),
                turns: Some(5),
                session_id: Some("session-local-7".to_owned()),
                cost_usd: Some(2.5),
                model: Some("gpt-5".to_owned()),
                usage: Some(RunUsage {
                    input_tokens: Some(50),
                    output_tokens: Some(75),
                    cache_read_tokens: Some(5),
                    cache_creation_tokens: Some(15),
                    unit: Some("weighted-tokens".to_owned()),
                    extra: PayloadExtension::new(),
                }),
                duration_ms: Some(3200),
                estimated: Some(false),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(
            serialise_event(&completed).unwrap(),
            AGENT_COMPLETED_WITH_SESSION_WIRE
        );
    }

    #[test]
    fn validate_rejects_a_non_string_session_id_on_agent_completed() {
        let error = validate(AGENT_COMPLETED, &json!({ "sessionId": 7 })).unwrap_err();
        assert_eq!(
            error.to_string(),
            "AgentCompletedPayload.sessionId must be a string when present"
        );
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
            session_id: None,
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

    // -- control.* (spec onsager-ai/ethogram#8) ---------------------------------------------

    #[test]
    fn parse_event_accepts_every_permitted_control_kind_without_text() {
        // `parse_event` answers "can both SDKs carry this?", not "should a
        // producer have emitted this?" A `steer` naming no `text` is
        // perfectly representable — `validate` below rejects it as a
        // producer error, but a forwarder must still be able to relay it.
        // This is the test that would fail if someone later "helpfully"
        // moved the steer-needs-text rule into the parser.
        for kind in ["interrupt", "steer", "answer"] {
            let payload = json!({ "controlId": "control-1", "kind": kind, "by": "operator" });
            let input = lifecycle_event_input("control.requested", payload);
            parse_event(&input).unwrap();
        }
    }

    #[test]
    fn validate_accepts_interrupt_with_no_text() {
        // An interrupt has nothing to say by design.
        let payload = json!({ "controlId": "control-1", "kind": "interrupt", "by": "operator" });
        validate(CONTROL_REQUESTED, &payload).unwrap();
    }

    #[test]
    fn validate_accepts_steer_with_text() {
        let payload = json!({
            "controlId": "control-1",
            "kind": "steer",
            "by": "operator",
            "text": "take point on the next turn"
        });
        validate(CONTROL_REQUESTED, &payload).unwrap();
    }

    #[test]
    fn validate_rejects_steer_with_absent_text() {
        let payload = json!({ "controlId": "control-1", "kind": "steer", "by": "operator" });
        let error = validate(CONTROL_REQUESTED, &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            "ControlRequestedPayload.text is required and must not be empty when kind is \"steer\": a steer with nothing to say is a producer error"
        );
    }

    #[test]
    fn validate_rejects_steer_with_empty_text() {
        // A zero-length instruction is the same defect as an absent one.
        let payload = json!({
            "controlId": "control-1",
            "kind": "steer",
            "by": "operator",
            "text": ""
        });
        let error = validate(CONTROL_REQUESTED, &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            "ControlRequestedPayload.text is required and must not be empty when kind is \"steer\": a steer with nothing to say is a producer error"
        );
    }

    #[test]
    fn parses_an_unknown_control_kind_verbatim_and_validate_reports_it() {
        // "teleport" is a value neither SDK will ever know, matching issue
        // onsager-ai/ethogram#12's own example. There is deliberately no `pause` member either
        // (see `ControlKind`'s doc comment), but that is a closed-vocabulary
        // fact, not an unknown-string one, so it is not exercised here.
        let input = lifecycle_event_input(
            "control.requested",
            json!({ "controlId": "control-3", "kind": "teleport", "by": "operator" }),
        );

        let event = parse_event(&input).unwrap();
        let parsed: ControlRequestedPayload =
            serde_json::from_value(event.payload.clone()).unwrap();
        assert_eq!(parsed.kind, ControlKind::Unknown("teleport".to_owned()));

        let error = validate(CONTROL_REQUESTED, &event.payload).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ControlRequestedPayload.kind has unknown value: teleport"),
            "error was: {error}"
        );
        let error = validate(CONTROL_REQUESTED, &parsed).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ControlRequestedPayload.kind has unknown value: teleport"),
            "error was: {error}"
        );
    }

    #[test]
    fn unknown_control_kind_keeps_cross_version_byte_identity_with_typescript_and_input() {
        let event = parse_event(UNKNOWN_CONTROL_KIND_WIRE).unwrap();
        let parsed: ControlRequestedPayload =
            serde_json::from_value(event.payload.clone()).unwrap();

        assert_eq!(parsed.kind, ControlKind::Unknown("teleport".to_owned()));
        assert_eq!(serialise_event(&event).unwrap(), UNKNOWN_CONTROL_KIND_WIRE);
    }

    #[test]
    fn rejects_each_missing_required_control_requested_field() {
        for field in ["controlId", "kind", "by"] {
            let mut payload =
                json!({ "controlId": "control-1", "kind": "steer", "by": "operator" });
            payload.as_object_mut().unwrap().remove(field);
            let input = lifecycle_event_input("control.requested", payload);

            let error = parse_event(&input).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "error for {field} was: {error}"
            );
        }
    }

    #[test]
    fn rejects_each_missing_required_control_applied_field() {
        for field in ["controlId", "ok"] {
            let mut payload = json!({ "controlId": "control-1", "ok": true });
            payload.as_object_mut().unwrap().remove(field);
            let input = lifecycle_event_input("control.applied", payload);

            let error = parse_event(&input).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "error for {field} was: {error}"
            );
        }
    }

    #[test]
    fn every_control_payload_retains_unknown_fields() {
        let requested: ControlRequestedPayload = serde_json::from_value(json!({
            "controlId": "control-1",
            "kind": "steer",
            "by": "operator",
            "future": { "value": 1 }
        }))
        .unwrap();
        let applied: ControlAppliedPayload = serde_json::from_value(json!({
            "controlId": "control-1",
            "ok": true,
            "future": { "value": 1 }
        }))
        .unwrap();

        for extra in [&requested.extra, &applied.extra] {
            assert_eq!(extra.get("future"), Some(&json!({ "value": 1 })));
        }
        for re_emitted in [
            serde_json::to_value(&requested).unwrap(),
            serde_json::to_value(&applied).unwrap(),
        ] {
            assert_eq!(re_emitted["future"], json!({ "value": 1 }));
        }
    }

    #[test]
    fn absent_optional_control_payload_fields_are_omitted_instead_of_null() {
        let requested = ControlRequestedPayload {
            control_id: "control-1".to_owned(),
            kind: ControlKind::Interrupt,
            decision_id: None,
            option_id: None,
            text: None,
            truncated: None,
            by: "operator".to_owned(),
            extra: PayloadExtension::new(),
        };
        let applied = ControlAppliedPayload {
            control_id: "control-1".to_owned(),
            ok: true,
            by: None,
            reason: None,
            truncated: None,
            landed_in: None,
            extra: PayloadExtension::new(),
        };

        assert_eq!(
            serde_json::to_string(&requested).unwrap(),
            r#"{"controlId":"control-1","kind":"interrupt","by":"operator"}"#
        );
        assert_eq!(
            serde_json::to_string(&applied).unwrap(),
            r#"{"controlId":"control-1","ok":true}"#
        );
    }

    #[test]
    fn control_events_match_the_typescript_pinned_bytes() {
        let requested = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: CONTROL_REQUESTED.to_owned(),
            run_id: "run-control".to_owned(),
            seq: 1,
            ts: "2026-09-07T05:00:00.000Z".to_owned(),
            payload: ControlRequestedPayload {
                control_id: "control-1".to_owned(),
                kind: ControlKind::Steer,
                decision_id: None,
                option_id: None,
                text: Some("take point on the next turn".to_owned()),
                truncated: Some(false),
                by: "operator".to_owned(),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let applied_failed = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: CONTROL_APPLIED.to_owned(),
            run_id: "run-control".to_owned(),
            seq: 2,
            ts: "2026-09-07T05:00:01.000Z".to_owned(),
            payload: ControlAppliedPayload {
                control_id: "control-1".to_owned(),
                ok: false,
                by: None,
                reason: Some(ControlAppliedReason::NotLive),
                truncated: None,
                landed_in: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let applied_interrupt = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: CONTROL_APPLIED.to_owned(),
            run_id: "run-control".to_owned(),
            seq: 3,
            ts: "2026-09-07T05:00:02.000Z".to_owned(),
            payload: ControlAppliedPayload {
                control_id: "control-2".to_owned(),
                ok: true,
                by: None,
                reason: None,
                truncated: None,
                landed_in: Some("tool-9".to_owned()),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        assert_eq!(serialise_event(&requested).unwrap(), CONTROL_REQUESTED_WIRE);
        assert_eq!(
            serialise_event(&applied_failed).unwrap(),
            CONTROL_APPLIED_FAILED_WIRE
        );
        assert_eq!(
            serialise_event(&applied_interrupt).unwrap(),
            CONTROL_APPLIED_INTERRUPT_WIRE
        );

        let mut retained = applied_failed;
        retained.payload.reason = Some(ControlAppliedReason::Unknown("not-live".to_owned()));
        assert_eq!(
            serialise_event(&retained).unwrap(),
            CONTROL_APPLIED_FAILED_WIRE
        );
    }

    // -- capture.refused (spec onsager-ai/ethogram#15) -------------------------------------

    #[test]
    fn accepts_every_permitted_capture_refusal_cause() {
        for cause in ["over_bound", "gap", "duplicate", "finished", "malformed"] {
            let payload = json!({ "cause": cause, "sourceRunId": "run-source" });
            let input = lifecycle_event_input(CAPTURE_REFUSED, payload.clone());
            parse_event(&input).unwrap();
            validate(CAPTURE_REFUSED, &payload).unwrap();
        }
    }

    #[test]
    fn rejects_each_missing_required_capture_refused_field() {
        for field in ["cause", "sourceRunId"] {
            let mut payload = json!({ "cause": "gap", "sourceRunId": "run-source" });
            payload.as_object_mut().unwrap().remove(field);
            let input = lifecycle_event_input(CAPTURE_REFUSED, payload.clone());

            let parse_error = parse_event(&input).unwrap_err();
            assert!(
                parse_error.to_string().contains(field),
                "parse error for {field} was: {parse_error}"
            );
            let validation_error = validate(CAPTURE_REFUSED, &payload).unwrap_err();
            assert!(
                validation_error.to_string().contains(field),
                "validation error for {field} was: {validation_error}"
            );
        }
    }

    #[test]
    fn parses_an_unknown_capture_refusal_cause_verbatim_and_validate_reports_it() {
        let event = parse_event(UNKNOWN_CAPTURE_REFUSAL_CAUSE_WIRE).unwrap();
        let parsed: CaptureRefusedPayload = serde_json::from_value(event.payload.clone()).unwrap();

        assert_eq!(
            parsed.cause,
            CaptureRefusalCause::Unknown("never-a-valid-capture-refusal-cause".to_owned())
        );
        let error = validate(CAPTURE_REFUSED, &event.payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            "CaptureRefusedPayload.cause has unknown value: never-a-valid-capture-refusal-cause"
        );
        assert_eq!(parsed.cause.as_str(), "never-a-valid-capture-refusal-cause");
        assert_eq!(
            serde_json::to_string(&parsed.cause).unwrap(),
            r#""never-a-valid-capture-refusal-cause""#
        );
        assert_eq!(
            serialise_event(&event).unwrap(),
            UNKNOWN_CAPTURE_REFUSAL_CAUSE_WIRE
        );
    }

    #[test]
    fn capture_refused_counts_are_non_negative_safe_integers() {
        for field in ["sourceSeq", "count", "max"] {
            for invalid in [json!(-1), json!(1.5), json!(9_007_199_254_740_992_u64)] {
                let mut payload = json!({ "cause": "over_bound", "sourceRunId": "run-source" });
                payload
                    .as_object_mut()
                    .unwrap()
                    .insert(field.to_owned(), invalid);
                let input = lifecycle_event_input(CAPTURE_REFUSED, payload.clone());

                assert!(parse_event(&input).is_err(), "parse accepted {field}");
                assert!(
                    validate(CAPTURE_REFUSED, &payload).is_err(),
                    "validate accepted {field}"
                );
            }

            let mut payload = json!({ "cause": "over_bound", "sourceRunId": "run-source" });
            payload
                .as_object_mut()
                .unwrap()
                .insert(field.to_owned(), json!(MAX_SAFE_INTEGER_MAGNITUDE));
            let input = lifecycle_event_input(CAPTURE_REFUSED, payload.clone());
            parse_event(&input).unwrap();
            validate(CAPTURE_REFUSED, &payload).unwrap();
        }
    }

    #[test]
    fn capture_refused_never_carries_content_bearing_fields() {
        // This is a fully populated typed payload, so adding even an optional
        // field to `CaptureRefusedPayload` first breaks this struct literal.
        // Once that field is populated, the exact permitted-key assertion
        // below still fails unless the protocol's no-content boundary is
        // deliberately revisited. Listing only currently imagined forbidden
        // names would not catch a newly invented content field.
        let payload = CaptureRefusedPayload {
            cause: CaptureRefusalCause::OverBound,
            source_run_id: "run-source".to_owned(),
            source_seq: Some(8),
            source_type: Some(AGENT_TEXT.to_owned()),
            field: Some("AgentTextPayload.text".to_owned()),
            count: Some(20_000),
            max: Some(16_384),
            detail: Some("parser message only".to_owned()),
            truncated: Some(false),
            extra: PayloadExtension::new(),
        };
        let serialised = serde_json::to_value(payload).unwrap();
        let keys: std::collections::HashSet<&str> = serialised
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();

        assert_eq!(
            keys,
            std::collections::HashSet::from([
                "cause",
                "sourceRunId",
                "sourceSeq",
                "sourceType",
                "field",
                "count",
                "max",
                "detail",
                "truncated",
            ])
        );
    }

    #[test]
    fn absent_optional_capture_refused_fields_are_omitted_instead_of_null() {
        let payload = CaptureRefusedPayload {
            cause: CaptureRefusalCause::Gap,
            source_run_id: "run-source-gap".to_owned(),
            source_seq: None,
            source_type: None,
            field: None,
            count: None,
            max: None,
            detail: None,
            truncated: None,
            extra: PayloadExtension::new(),
        };
        let serialised = serde_json::to_value(payload).unwrap();

        assert_eq!(
            serialised,
            json!({ "cause": "gap", "sourceRunId": "run-source-gap" })
        );
        assert!(!serialised.to_string().contains(":null"));
    }

    #[test]
    fn capture_refused_retains_and_re_emits_unknown_payload_fields() {
        let payload: CaptureRefusedPayload = serde_json::from_value(json!({
            "cause": "gap",
            "sourceRunId": "run-source-gap",
            "future": { "value": 1 }
        }))
        .unwrap();

        assert_eq!(payload.extra.get("future"), Some(&json!({ "value": 1 })));
        assert_eq!(
            serde_json::to_value(payload).unwrap()["future"],
            json!({ "value": 1 })
        );
    }

    #[test]
    fn parse_event_carries_an_over_bound_capture_detail_that_validate_refuses() {
        let input = lifecycle_event_input(
            CAPTURE_REFUSED,
            json!({
                "cause": "malformed",
                "sourceRunId": "run-source",
                "detail": "x".repeat(MAX_EXCERPT_SCALARS + 1)
            }),
        );
        let event = parse_event(&input).unwrap();
        let error = validate(CAPTURE_REFUSED, &event.payload).unwrap_err();

        assert_eq!(
            error.to_string(),
            "CaptureRefusedPayload.detail has 4097 Unicode scalar values; maximum is 4096"
        );
    }

    #[test]
    fn capture_refused_events_match_the_typescript_pinned_bytes() {
        let over_bound = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: CAPTURE_REFUSED.to_owned(),
            run_id: "run-relay".to_owned(),
            seq: 1,
            ts: "2026-09-07T06:00:00.000Z".to_owned(),
            payload: CaptureRefusedPayload {
                cause: CaptureRefusalCause::OverBound,
                source_run_id: "run-source".to_owned(),
                source_seq: Some(8),
                source_type: Some(AGENT_TEXT.to_owned()),
                field: Some("AgentTextPayload.text".to_owned()),
                count: Some(20_000),
                max: Some(16_384),
                detail: None,
                truncated: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let gap = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: CAPTURE_REFUSED.to_owned(),
            run_id: "run-relay".to_owned(),
            seq: 2,
            ts: "2026-09-07T06:00:01.000Z".to_owned(),
            payload: CaptureRefusedPayload {
                cause: CaptureRefusalCause::Gap,
                source_run_id: "run-source-gap".to_owned(),
                source_seq: None,
                source_type: None,
                field: None,
                count: None,
                max: None,
                detail: None,
                truncated: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        validate(CAPTURE_REFUSED, &over_bound.payload).unwrap();
        assert!(over_bound.payload.count.unwrap() > over_bound.payload.max.unwrap());
        assert_eq!(
            serialise_event(&over_bound).unwrap(),
            CAPTURE_REFUSED_OVER_BOUND_WIRE
        );
        assert_eq!(serialise_event(&gap).unwrap(), CAPTURE_REFUSED_GAP_WIRE);

        let mut retained = gap;
        retained.payload.cause = CaptureRefusalCause::Unknown("gap".to_owned());
        assert_eq!(
            serialise_event(&retained).unwrap(),
            CAPTURE_REFUSED_GAP_WIRE
        );

        for expected in [CAPTURE_REFUSED_OVER_BOUND_WIRE, CAPTURE_REFUSED_GAP_WIRE] {
            let parsed = parse_event(expected).unwrap();
            assert_eq!(serialise_event(&parsed).unwrap(), expected);
        }
    }

    // -- decision.* (spec onsager-ai/ethogram#7) ------------------------------------------

    fn minimal_decision_request(kind: &str) -> Value {
        json!({
            "decisionId": "decision-1",
            "kind": kind,
            "dossier": {
                "question": "Proceed?",
                "optionsRuledOut": ["auto-proceed"],
                "recommendedAction": "ask the operator",
                "blastRadius": "one run"
            },
            "options": [{ "id": "allow", "label": "Allow" }]
        })
    }

    fn consistency_request() -> DecisionRequestedPayload {
        DecisionRequestedPayload {
            decision_id: "decision-1".to_owned(),
            kind: DecisionKind::Permission,
            dossier: DecisionDossier {
                question: "Proceed?".to_owned(),
                options_ruled_out: vec![],
                recommended_action: "ask the operator".to_owned(),
                blast_radius: "one run".to_owned(),
                truncated: None,
                extra: PayloadExtension::new(),
            },
            // `deny` is deliberately not a human option here, so the tests
            // distinguish the helper's timeout-only allowance from ordinary
            // option membership.
            options: vec![DecisionOption {
                id: "allow".to_owned(),
                label: "Allow".to_owned(),
                extra: PayloadExtension::new(),
            }],
            subject: None,
            expires_at: None,
            on_timeout: Some("deny".to_owned()),
            extra: PayloadExtension::new(),
        }
    }

    fn consistency_answer(option_id: &str) -> DecisionAnsweredPayload {
        DecisionAnsweredPayload {
            decision_id: "decision-1".to_owned(),
            option_id: option_id.to_owned(),
            by: "principal:user:alice".to_owned(),
            by_timeout: None,
            reversal: None,
            requested_run_id: None,
            extra: PayloadExtension::new(),
        }
    }

    #[test]
    fn accepts_every_permitted_decision_kind_without_on_timeout() {
        for kind in [
            "permission",
            "tripwire",
            "gate_inconclusive",
            "human_decides",
            "budget",
        ] {
            let payload = minimal_decision_request(kind);
            parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload.clone())).unwrap();
            validate(DECISION_REQUESTED, &payload).unwrap();
        }
    }

    #[test]
    fn rejects_each_missing_required_decision_requested_field() {
        for field in ["decisionId", "kind", "dossier", "options"] {
            let mut payload = minimal_decision_request("permission");
            payload.as_object_mut().unwrap().remove(field);

            let parse_error =
                parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload.clone()))
                    .unwrap_err();
            assert!(
                parse_error.to_string().contains(field),
                "parse error for {field} was: {parse_error}"
            );
            let validation_error = validate(DECISION_REQUESTED, &payload).unwrap_err();
            assert!(
                validation_error.to_string().contains(field),
                "validation error for {field} was: {validation_error}"
            );
        }
    }

    #[test]
    fn rejects_each_missing_required_dossier_field() {
        for field in [
            "question",
            "optionsRuledOut",
            "recommendedAction",
            "blastRadius",
        ] {
            let mut payload = minimal_decision_request("permission");
            payload["dossier"].as_object_mut().unwrap().remove(field);

            let parse_error =
                parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload.clone()))
                    .unwrap_err();
            assert!(
                parse_error.to_string().contains(field),
                "parse error for dossier.{field} was: {parse_error}"
            );
            let validation_error = validate(DECISION_REQUESTED, &payload).unwrap_err();
            assert!(
                validation_error.to_string().contains(field),
                "validation error for dossier.{field} was: {validation_error}"
            );
        }
    }

    #[test]
    fn rejects_each_missing_required_option_field() {
        for field in ["id", "label"] {
            let mut payload = minimal_decision_request("permission");
            payload["options"][0].as_object_mut().unwrap().remove(field);

            let parse_error =
                parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload.clone()))
                    .unwrap_err();
            assert!(
                parse_error.to_string().contains(field),
                "parse error for options[0].{field} was: {parse_error}"
            );
            let validation_error = validate(DECISION_REQUESTED, &payload).unwrap_err();
            assert!(
                validation_error.to_string().contains(field),
                "validation error for options[0].{field} was: {validation_error}"
            );
        }
    }

    #[test]
    fn rejects_each_missing_required_decision_answered_field() {
        for field in ["decisionId", "optionId", "by"] {
            let mut payload = json!({
                "decisionId": "decision-1",
                "optionId": "allow",
                "by": "principal:user:alice"
            });
            payload.as_object_mut().unwrap().remove(field);

            let parse_error =
                parse_event(&lifecycle_event_input(DECISION_ANSWERED, payload.clone()))
                    .unwrap_err();
            assert!(
                parse_error.to_string().contains(field),
                "parse error for {field} was: {parse_error}"
            );
            let validation_error = validate(DECISION_ANSWERED, &payload).unwrap_err();
            assert!(
                validation_error.to_string().contains(field),
                "validation error for {field} was: {validation_error}"
            );
        }
    }

    #[test]
    fn parses_an_unknown_decision_kind_verbatim_and_validate_reports_it() {
        let event = parse_event(UNKNOWN_DECISION_KIND_WIRE).unwrap();
        let parsed: DecisionRequestedPayload =
            serde_json::from_value(event.payload.clone()).unwrap();

        assert_eq!(
            parsed.kind,
            DecisionKind::Unknown("never-a-valid-decision-kind".to_owned())
        );
        let error = validate(DECISION_REQUESTED, &event.payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            "DecisionRequestedPayload.kind has unknown value: never-a-valid-decision-kind"
        );
        assert_eq!(parsed.kind.as_str(), "never-a-valid-decision-kind");
        assert_eq!(
            serde_json::to_string(&parsed.kind).unwrap(),
            r#""never-a-valid-decision-kind""#
        );
        assert_eq!(serialise_event(&event).unwrap(), UNKNOWN_DECISION_KIND_WIRE);
    }

    #[test]
    fn validate_enforces_the_permission_only_on_timeout_rule() {
        for kind in ["tripwire", "gate_inconclusive", "human_decides", "budget"] {
            let mut payload = minimal_decision_request(kind);
            payload["onTimeout"] = json!("deny");
            let error = validate(DECISION_REQUESTED, &payload).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "DecisionRequestedPayload.onTimeout is permitted only when kind is \"permission\"; received kind \"{kind}\""
                )
            );
        }

        let mut invalid_permission = minimal_decision_request("permission");
        invalid_permission["onTimeout"] = json!("allow");
        let error = validate(DECISION_REQUESTED, &invalid_permission).unwrap_err();
        assert_eq!(
            error.to_string(),
            "DecisionRequestedPayload.onTimeout must be \"deny\" when kind is \"permission\"; received \"allow\""
        );

        let mut valid_permission = minimal_decision_request("permission");
        valid_permission["options"] =
            json!([{ "id": "allow", "label": "Allow" }, { "id": "deny", "label": "Deny" }]);
        valid_permission["onTimeout"] = json!("deny");
        validate(DECISION_REQUESTED, &valid_permission).unwrap();

        for kind in [
            "permission",
            "tripwire",
            "gate_inconclusive",
            "human_decides",
            "budget",
        ] {
            validate(DECISION_REQUESTED, &minimal_decision_request(kind)).unwrap();
        }
    }

    #[test]
    fn validate_rejects_on_timeout_naming_no_request_option() {
        // `options` here is only `allow`; `deny` is permitted by the
        // kind/value rules above but was never offered.
        let mut payload = minimal_decision_request("permission");
        payload["onTimeout"] = json!("deny");
        let error = validate(DECISION_REQUESTED, &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            "DecisionRequestedPayload.onTimeout must name one of the request's options[].id; received \"deny\""
        );
    }

    #[test]
    fn validate_accepts_on_timeout_naming_an_existing_option() {
        let mut payload = minimal_decision_request("permission");
        payload["options"] =
            json!([{ "id": "allow", "label": "Allow" }, { "id": "deny", "label": "Deny" }]);
        payload["onTimeout"] = json!("deny");
        validate(DECISION_REQUESTED, &payload).unwrap();
    }

    #[test]
    fn parse_event_accepts_on_timeout_on_a_tripwire() {
        // `onTimeout` on a tripwire is a producer-policy violation, but it is
        // representable. This fails if the rule ever leaks into parsing.
        let mut payload = minimal_decision_request("tripwire");
        payload["onTimeout"] = json!("deny");
        parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload)).unwrap();
    }

    #[test]
    fn parse_event_accepts_on_timeout_naming_no_request_option() {
        // Naming an option the request never offered is a producer-policy
        // violation, but the event is still representable. This fails if the
        // options-membership rule ever leaks into parsing.
        let mut payload = minimal_decision_request("permission");
        payload["onTimeout"] = json!("deny");
        parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload)).unwrap();
    }

    #[test]
    fn cross_event_helper_checks_option_timeout_and_decision_id_and_accepts_any_reversal() {
        let request = consistency_request();
        let valid = consistency_answer("allow");
        validate_decision_answer_against_request(&request, &valid).unwrap();

        let invalid_option = consistency_answer("missing");
        assert!(
            validate_decision_answer_against_request(&request, &invalid_option)
                .unwrap_err()
                .to_string()
                .contains("optionId")
        );

        let mut timeout = consistency_answer("deny");
        timeout.by_timeout = Some(true);
        validate_decision_answer_against_request(&request, &timeout).unwrap();

        timeout.by_timeout = Some(false);
        assert!(validate_decision_answer_against_request(&request, &timeout).is_err());
        timeout.by_timeout = None;
        assert!(validate_decision_answer_against_request(&request, &timeout).is_err());

        // Ruled on onsager-ai/ethogram#7: a `<verb>:<subject>` action id is a legitimate
        // `reversal` even though it was never offered as a request option —
        // `revoke:required_checks` undoes `excuse:required_checks`, an
        // action the human was never offered as a choice. This deliberately
        // replaces a prior assertion that such a reversal was rejected: that
        // behaviour is the constraint being loosened here, not a bug being
        // preserved.
        let mut action_reversal = valid.clone();
        action_reversal.reversal = Some("revoke:required_checks".to_owned());
        validate_decision_answer_against_request(&request, &action_reversal).unwrap();

        let mut mismatched = valid;
        mismatched.decision_id = "decision-2".to_owned();
        assert!(
            validate_decision_answer_against_request(&request, &mismatched)
                .unwrap_err()
                .to_string()
                .contains("does not match request")
        );
    }

    #[test]
    fn decision_narration_bounds_are_validation_only() {
        let over = "😀".repeat(MAX_EXCERPT_SCALARS + 1);
        let cases = [
            (
                json!({
                    "decisionId": "decision-1",
                    "kind": "permission",
                    "dossier": {
                        "question": over,
                        "optionsRuledOut": [],
                        "recommendedAction": "ask",
                        "blastRadius": "one run"
                    },
                    "options": []
                }),
                "DecisionRequestedPayload.dossier.question",
            ),
            (
                json!({
                    "decisionId": "decision-1",
                    "kind": "permission",
                    "dossier": {
                        "question": "Proceed?",
                        "optionsRuledOut": [over],
                        "recommendedAction": "ask",
                        "blastRadius": "one run"
                    },
                    "options": []
                }),
                "DecisionRequestedPayload.dossier.optionsRuledOut[0]",
            ),
            (
                json!({
                    "decisionId": "decision-1",
                    "kind": "permission",
                    "dossier": {
                        "question": "Proceed?",
                        "optionsRuledOut": [],
                        "recommendedAction": over,
                        "blastRadius": "one run"
                    },
                    "options": []
                }),
                "DecisionRequestedPayload.dossier.recommendedAction",
            ),
            (
                json!({
                    "decisionId": "decision-1",
                    "kind": "permission",
                    "dossier": {
                        "question": "Proceed?",
                        "optionsRuledOut": [],
                        "recommendedAction": "ask",
                        "blastRadius": over
                    },
                    "options": []
                }),
                "DecisionRequestedPayload.dossier.blastRadius",
            ),
            (
                json!({
                    "decisionId": "decision-1",
                    "kind": "permission",
                    "dossier": {
                        "question": "Proceed?",
                        "optionsRuledOut": [],
                        "recommendedAction": "ask",
                        "blastRadius": "one run"
                    },
                    "options": [{ "id": "allow", "label": over }]
                }),
                "DecisionRequestedPayload.options[0].label",
            ),
        ];

        for (payload, field) in cases {
            parse_event(&lifecycle_event_input(DECISION_REQUESTED, payload.clone())).unwrap();
            let error = validate(DECISION_REQUESTED, &payload).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "{field} has {} Unicode scalar values; maximum is {MAX_EXCERPT_SCALARS}",
                    MAX_EXCERPT_SCALARS + 1
                )
            );
        }
    }

    #[test]
    fn decision_identifiers_are_not_excerpt_bounded() {
        let identifier = "x".repeat(MAX_EXCERPT_SCALARS + 1);
        let mut request = minimal_decision_request("permission");
        request["decisionId"] = json!(identifier);
        request["options"][0]["id"] = json!(identifier);
        validate(DECISION_REQUESTED, &request).unwrap();

        let answer = json!({
            "decisionId": identifier,
            "optionId": identifier,
            "by": "principal:user:alice",
            "reversal": identifier
        });
        validate(DECISION_ANSWERED, &answer).unwrap();
    }

    #[test]
    fn absent_optional_decision_fields_are_omitted_instead_of_null() {
        let request = consistency_request();
        let request = DecisionRequestedPayload {
            on_timeout: None,
            ..request
        };
        let answer = consistency_answer("allow");

        let request_value = serde_json::to_value(request).unwrap();
        for field in ["subject", "expiresAt", "onTimeout"] {
            assert!(!request_value.as_object().unwrap().contains_key(field));
        }
        assert!(
            !request_value["dossier"]
                .as_object()
                .unwrap()
                .contains_key("truncated")
        );

        let answer_value = serde_json::to_value(answer).unwrap();
        for field in ["byTimeout", "reversal", "requestedRunId"] {
            assert!(!answer_value.as_object().unwrap().contains_key(field));
        }
        assert!(!request_value.to_string().contains(":null"));
        assert!(!answer_value.to_string().contains(":null"));
    }

    #[test]
    fn decision_answered_serialises_requested_run_id_when_present() {
        let mut answer = consistency_answer("allow");
        answer.requested_run_id = Some("run-decision".to_owned());

        let answer_value = serde_json::to_value(&answer).unwrap();
        assert_eq!(answer_value["requestedRunId"], json!("run-decision"));

        let round_tripped: DecisionAnsweredPayload = serde_json::from_value(answer_value).unwrap();
        assert_eq!(round_tripped, answer);
    }

    #[test]
    fn decision_payloads_retain_and_re_emit_unknown_fields_at_every_level() {
        let request: DecisionRequestedPayload = serde_json::from_value(json!({
            "decisionId": "decision-1",
            "kind": "permission",
            "dossier": {
                "question": "Proceed?",
                "optionsRuledOut": [],
                "recommendedAction": "ask",
                "blastRadius": "one run",
                "futureDossier": { "value": 1 }
            },
            "options": [{
                "id": "allow",
                "label": "Allow",
                "futureOption": { "value": 2 }
            }],
            "futureRequest": { "value": 3 }
        }))
        .unwrap();
        let answer: DecisionAnsweredPayload = serde_json::from_value(json!({
            "decisionId": "decision-1",
            "optionId": "allow",
            "by": "principal:user:alice",
            "requestedRunId": "run-decision",
            "futureAnswer": { "value": 4 }
        }))
        .unwrap();

        assert_eq!(
            request.extra.get("futureRequest"),
            Some(&json!({ "value": 3 }))
        );
        assert_eq!(
            request.dossier.extra.get("futureDossier"),
            Some(&json!({ "value": 1 }))
        );
        assert_eq!(
            request.options[0].extra.get("futureOption"),
            Some(&json!({ "value": 2 }))
        );
        assert_eq!(
            answer.extra.get("futureAnswer"),
            Some(&json!({ "value": 4 }))
        );
        // `requestedRunId` is a known field, not an extra: adding it must not
        // disturb the unknown-field tolerance path exercised above and below.
        assert_eq!(answer.extra.get("requestedRunId"), None);
        assert_eq!(answer.requested_run_id.as_deref(), Some("run-decision"));

        let re_emitted_request = serde_json::to_value(request).unwrap();
        assert_eq!(re_emitted_request["futureRequest"], json!({ "value": 3 }));
        assert_eq!(
            re_emitted_request["dossier"]["futureDossier"],
            json!({ "value": 1 })
        );
        assert_eq!(
            re_emitted_request["options"][0]["futureOption"],
            json!({ "value": 2 })
        );
        let re_emitted_answer = serde_json::to_value(answer).unwrap();
        assert_eq!(re_emitted_answer["futureAnswer"], json!({ "value": 4 }));
        assert_eq!(re_emitted_answer["requestedRunId"], json!("run-decision"));
    }

    #[test]
    fn decision_events_match_the_typescript_pinned_bytes() {
        let requested = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: DECISION_REQUESTED.to_owned(),
            run_id: "run-decision".to_owned(),
            seq: 1,
            ts: "2026-09-07T07:00:00.000Z".to_owned(),
            payload: DecisionRequestedPayload {
                decision_id: "decision-1".to_owned(),
                kind: DecisionKind::Permission,
                dossier: DecisionDossier {
                    question: "May the run execute the deployment tool?".to_owned(),
                    options_ruled_out: vec![
                        "auto-proceed".to_owned(),
                        "discard the request".to_owned(),
                    ],
                    recommended_action: "deny unless the operator confirms the target".to_owned(),
                    blast_radius: "one repository".to_owned(),
                    truncated: Some(false),
                    extra: PayloadExtension::new(),
                },
                options: vec![
                    DecisionOption {
                        id: "allow".to_owned(),
                        label: "Allow once".to_owned(),
                        extra: PayloadExtension::new(),
                    },
                    DecisionOption {
                        id: "deny".to_owned(),
                        label: "Deny".to_owned(),
                        extra: PayloadExtension::new(),
                    },
                ],
                subject: Some("deploy".to_owned()),
                expires_at: Some("2026-09-07T07:05:00.000Z".to_owned()),
                on_timeout: Some("deny".to_owned()),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let human = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: DECISION_ANSWERED.to_owned(),
            run_id: "run-decision".to_owned(),
            seq: 2,
            ts: "2026-09-07T07:01:00.000Z".to_owned(),
            payload: DecisionAnsweredPayload {
                decision_id: "decision-1".to_owned(),
                option_id: "allow".to_owned(),
                by: "principal:user:alice".to_owned(),
                by_timeout: None,
                reversal: None,
                requested_run_id: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };
        let timeout = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: DECISION_ANSWERED.to_owned(),
            run_id: "run-decision".to_owned(),
            seq: 3,
            ts: "2026-09-07T07:05:00.000Z".to_owned(),
            payload: DecisionAnsweredPayload {
                decision_id: "decision-1".to_owned(),
                option_id: "deny".to_owned(),
                by: "principal:runtime:permission-timeout".to_owned(),
                by_timeout: Some(true),
                reversal: Some("allow".to_owned()),
                requested_run_id: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        validate(DECISION_REQUESTED, &requested.payload).unwrap();
        assert_eq!(
            serialise_event(&requested).unwrap(),
            DECISION_REQUESTED_WIRE
        );
        assert_eq!(
            serialise_event(&human).unwrap(),
            DECISION_ANSWERED_HUMAN_WIRE
        );
        assert_eq!(
            serialise_event(&timeout).unwrap(),
            DECISION_ANSWERED_TIMEOUT_WIRE
        );

        for expected in [
            DECISION_REQUESTED_WIRE,
            DECISION_ANSWERED_HUMAN_WIRE,
            DECISION_ANSWERED_TIMEOUT_WIRE,
        ] {
            let parsed = parse_event(expected).unwrap();
            assert_eq!(serialise_event(&parsed).unwrap(), expected);
        }
    }

    #[test]
    fn decision_answered_with_requested_run_id_matches_the_typescript_pinned_bytes() {
        let answer = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: DECISION_ANSWERED.to_owned(),
            run_id: "run-decision-answer".to_owned(),
            seq: 1,
            ts: "2026-09-07T07:10:00.000Z".to_owned(),
            payload: DecisionAnsweredPayload {
                decision_id: "decision-1".to_owned(),
                option_id: "allow".to_owned(),
                by: "principal:user:alice".to_owned(),
                by_timeout: None,
                reversal: None,
                requested_run_id: Some("run-decision".to_owned()),
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        validate(DECISION_ANSWERED, &answer.payload).unwrap();
        assert_eq!(
            serialise_event(&answer).unwrap(),
            DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE
        );
        let parsed = parse_event(DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE).unwrap();
        assert_eq!(
            serialise_event(&parsed).unwrap(),
            DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE
        );
    }

    #[test]
    fn decision_answered_with_action_reversal_matches_the_typescript_pinned_bytes() {
        // Pins the loosened rule (ruled on onsager-ai/ethogram#7): a `<verb>:<subject>` action
        // id is a conforming `reversal` even though it names no option this
        // request ever offered.
        let answer = Event {
            v: EVENT_SCHEMA_VERSION,
            event_type: DECISION_ANSWERED.to_owned(),
            run_id: "run-decision-revoke".to_owned(),
            seq: 1,
            ts: "2026-09-07T09:00:00.000Z".to_owned(),
            payload: DecisionAnsweredPayload {
                decision_id: "decision-revoke-1".to_owned(),
                option_id: "excuse:required_checks".to_owned(),
                by: "principal:user:alice".to_owned(),
                by_timeout: None,
                reversal: Some("revoke:required_checks".to_owned()),
                requested_run_id: None,
                extra: PayloadExtension::new(),
            },
            captured_at: None,
        };

        validate(DECISION_ANSWERED, &answer.payload).unwrap();
        assert_eq!(
            serialise_event(&answer).unwrap(),
            DECISION_ANSWERED_ACTION_REVERSAL_WIRE
        );
        let parsed = parse_event(DECISION_ANSWERED_ACTION_REVERSAL_WIRE).unwrap();
        assert_eq!(
            serialise_event(&parsed).unwrap(),
            DECISION_ANSWERED_ACTION_REVERSAL_WIRE
        );
    }

    #[test]
    fn validate_rejects_a_non_string_requested_run_id_on_decision_answered() {
        let payload = json!({
            "decisionId": "decision-1",
            "optionId": "allow",
            "by": "principal:user:alice",
            "requestedRunId": 7
        });
        let error = validate(DECISION_ANSWERED, &payload).unwrap_err();
        assert_eq!(
            error.to_string(),
            "DecisionAnsweredPayload.requestedRunId must be a string when present"
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

    // -- an optional field is absent or has a value; null is neither (onsager-ai/ethogram#28) --
    //
    // Ruled from onsager-ai/ostrom-hub#146: an explicit `null` on an optional payload field is
    // a parse error in both SDKs, because `Option<T>`/an optional field
    // cannot represent it faithfully — this is a representability question,
    // not a validation policy. `rejects_null_for_optional_captured_at_field`
    // above already covers the envelope's own optional field; the three
    // tests below cover one payload field of each optional shape this SDK
    // has (string, safe integer, boolean), and the final test pairs with all
    // three to show the same fields parse cleanly when merely absent.

    #[test]
    fn rejects_null_for_optional_string_payload_field() {
        // run.started.parentRunId: Option<String> via `deserialize_optional`.
        let payload = json!({
            "kind": "subagent",
            "actor": "builder",
            "harness": "codex",
            "parentRunId": null
        });
        let input = lifecycle_event_input("run.started", payload);

        let error = parse_event(&input).unwrap_err();
        assert_eq!(
            error.to_string(),
            "RunStartedPayload.parentRunId must be a string when present"
        );
    }

    #[test]
    fn rejects_null_for_optional_integer_payload_field() {
        // agent.completed.turns: Option<u64> via
        // `deserialize_optional_safe_u64`.
        let input = lifecycle_event_input("agent.completed", json!({ "turns": null }));

        let error = parse_event(&input).unwrap_err();
        assert_eq!(
            error.to_string(),
            "AgentCompletedPayload.turns must be a non-negative safe integer when present"
        );
    }

    #[test]
    fn rejects_null_for_optional_boolean_payload_field() {
        // agent.text.truncated: Option<bool> via `deserialize_optional`.
        let input =
            lifecycle_event_input("agent.text", json!({ "text": "hello", "truncated": null }));

        let error = parse_event(&input).unwrap_err();
        assert_eq!(
            error.to_string(),
            "AgentTextPayload.truncated must be a boolean when present"
        );
    }

    #[test]
    fn accepts_optional_payload_fields_when_absent_not_null() {
        // The positive half of the three rejection tests just above: the
        // same fields, simply omitted rather than sent as `null`, parse
        // cleanly. Absence is the only spelling of absence.
        let run_started = lifecycle_event_input(
            "run.started",
            json!({ "kind": "subagent", "actor": "builder", "harness": "codex" }),
        );
        assert!(
            parse_event(&run_started)
                .unwrap()
                .payload
                .get("parentRunId")
                .is_none()
        );

        let agent_completed = lifecycle_event_input("agent.completed", json!({}));
        assert!(
            parse_event(&agent_completed)
                .unwrap()
                .payload
                .get("turns")
                .is_none()
        );

        let agent_text = lifecycle_event_input("agent.text", json!({ "text": "hello" }));
        assert!(
            parse_event(&agent_text)
                .unwrap()
                .payload
                .get("truncated")
                .is_none()
        );
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
        // `serde_json` and ECMAScript disagree on notation (issue onsager-ai/ethogram#9), so
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

        // The 2^53 bound in the ruling of issue onsager-ai/ethogram#9 governs only whether a
        // float collapses to a wire integer, deliberately left unchanged
        // here: at and beyond 2^53 the value stays a float. But it is still
        // a whole number, and ECMAScript's notation rule (also issue onsager-ai/ethogram#9)
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
        // The band edges are the ones the ruling in issue onsager-ai/ethogram#9 names
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
    fn ulp_neighbours_of_short_decimals_match_measured_javascript_output() {
        // The class-4 canonicalisation above was diff-tested against real
        // JavaScript over 200,000 randomly sampled f64 values plus an
        // exponent sweep, byte-identical, zero differences -- and it still
        // missed a real defect, because uniform random sampling over the bit
        // space almost always produces values with full-length mantissas.
        // The shape that failed was a *short decimal perturbed by about one
        // ULP* (`0.0976519` nudged by a hair), which is vanishingly rare
        // under random sampling and extremely common in real money and
        // telemetry, since it is what summing a handful of prices produces.
        // The specific bug is fixed and has its own guard test above
        // (`serde_json_float_roundtrip_feature_is_required_for_correctly_rounded_costs`);
        // this covers the sampling gap that let it through, independently of
        // whether that particular bug ever recurs.
        //
        // Each base below is a short, money-/telemetry-shaped decimal. For
        // each, the neighbouring doubles one and two ULPs above and below
        // are generated here via `f64::from_bits(base.to_bits() ± n)`,
        // mirroring the TypeScript suite's `DataView`-based equivalent. The
        // *expected* strings were computed once with a throwaway Node
        // script (`JSON.stringify` of each bit-shifted double) and are
        // hard-coded here and in the TypeScript suite, since the two suites
        // cannot share a live process to compare against a running Node.
        // Every one of the 40 values agreed between `serde_json`'s digit
        // choice (re-laid by `EcmaScriptFormatter`) and V8's `JSON.stringify`
        // when this table was generated -- had any disagreed, that would
        // have been a live class-4 divergence, not a table update.
        const BASES: &[f64] = &[0.0976519, 0.1, 0.3, 1.25, 12.34, 0.001, 99.99, 1234.5678];

        // (index into BASES, signed ULP offset from that base, expected
        // `JSON.stringify` output for the resulting double)
        const EXPECTED: &[(usize, i8, &str)] = &[
            (0, -2, "0.09765189999999997"),
            (0, -1, "0.09765189999999999"),
            (0, 0, "0.0976519"),
            (0, 1, "0.09765190000000001"),
            (0, 2, "0.09765190000000003"),
            (1, -2, "0.09999999999999998"),
            (1, -1, "0.09999999999999999"),
            (1, 0, "0.1"),
            (1, 1, "0.10000000000000002"),
            (1, 2, "0.10000000000000003"),
            (2, -2, "0.2999999999999999"),
            (2, -1, "0.29999999999999993"),
            (2, 0, "0.3"),
            (2, 1, "0.30000000000000004"),
            (2, 2, "0.3000000000000001"),
            (3, -2, "1.2499999999999996"),
            (3, -1, "1.2499999999999998"),
            (3, 0, "1.25"),
            (3, 1, "1.2500000000000002"),
            (3, 2, "1.2500000000000004"),
            (4, -2, "12.339999999999996"),
            (4, -1, "12.339999999999998"),
            (4, 0, "12.34"),
            (4, 1, "12.340000000000002"),
            (4, 2, "12.340000000000003"),
            (5, -2, "0.0009999999999999996"),
            (5, -1, "0.0009999999999999998"),
            (5, 0, "0.001"),
            (5, 1, "0.0010000000000000002"),
            (5, 2, "0.0010000000000000005"),
            (6, -2, "99.98999999999997"),
            (6, -1, "99.98999999999998"),
            (6, 0, "99.99"),
            (6, 1, "99.99000000000001"),
            (6, 2, "99.99000000000002"),
            (7, -2, "1234.5677999999996"),
            (7, -1, "1234.5677999999998"),
            (7, 0, "1234.5678"),
            (7, 1, "1234.5678000000003"),
            (7, 2, "1234.5678000000005"),
        ];

        assert_eq!(
            EXPECTED.len(),
            BASES.len() * 5,
            "table covers every base at ULP offsets -2, -1, 0, 1, 2"
        );

        for &(base_index, offset, expected) in EXPECTED {
            let base = BASES[base_index];
            let bits = base.to_bits().wrapping_add(offset as i64 as u64);
            let value = f64::from_bits(bits);

            let event = Event {
                payload: json!({ "value": value }),
                ..complete_event()
            };

            assert_eq!(
                serialise_event(&event).unwrap(),
                format!(
                    r#"{{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{{"value":{expected}}}}}"#
                ),
                "base {base} (index {base_index}) offset {offset} expected {expected}"
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
    fn serde_json_float_roundtrip_feature_is_required_for_correctly_rounded_costs() {
        // Guards the workspace `Cargo.toml` pin of serde_json's
        // `float_roundtrip` feature the same way
        // `payload_keys_sort_by_utf8_bytes_even_for_a_plain_object_literal`
        // above guards against `preserve_order`: a dependency bump that
        // dropped it would otherwise surface only as a cross-language byte
        // diff in the conformance harness, which someone then has to trace
        // back to a parser rather than a value. This asserts on the parser
        // directly instead.
        //
        // 0.09765190000000001 is the `costUsd` captured in
        // `conformance/v1/agent-completed.json` (and repeated verbatim in
        // `conformance/v1/agent-completed-repeated-terminal.json`). Without
        // `float_roundtrip`, serde_json's default float parser is correctly
        // rounded for most inputs but not this one: it reads this literal as
        // the f64 one ULP below the value JavaScript's `JSON.parse` produces
        // for the same text.
        //
        // The comparison is on the bit pattern, not the decimal value,
        // because comparing values is exactly what lets a one-ULP error slip
        // through unnoticed.
        let value: Value = serde_json::from_str(r#"{"costUsd":0.09765190000000001}"#).unwrap();
        let cost_usd = value["costUsd"].as_f64().unwrap();
        assert_eq!(
            cost_usd.to_bits(),
            0x3fb8ffb704e46b50,
            "parsed bit pattern was {:#x}; float_roundtrip is missing or a dependency \
             regressed serde_json's float parsing",
            cost_usd.to_bits()
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
        // special case the ruling in issue onsager-ai/ethogram#9 rules out for `1e21`.
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

        let first = sink
            .append_draft(
                "run-1",
                EventDraft {
                    event_type: "test.happened".to_owned(),
                    payload: json!(1),
                    captured_at: None,
                },
            )
            .unwrap();
        let first_stamp = (first.seq, first.ts.clone());
        let second = sink
            .append_draft(
                "run-1",
                EventDraft {
                    event_type: "test.happened".to_owned(),
                    payload: json!(2),
                    captured_at: None,
                },
            )
            .unwrap();

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
        let AppendError::Sequence(sequence_error) = error else {
            panic!("expected AppendError::Sequence, got {error:?}");
        };
        assert_eq!(sequence_error.expected, 2);
        assert_eq!(sequence_error.received, 3);
        assert_eq!(sink.events("run-1").len(), 1);
    }

    // -- A run has at most one `run.finished` (issues onsager-ai/ethogram#5 and onsager-ai/ethogram#3) --------

    fn run_finished_draft(payload: Value) -> EventDraft {
        EventDraft {
            event_type: RUN_FINISHED.to_owned(),
            payload,
            captured_at: None,
        }
    }

    fn agent_text_draft(text: &str) -> EventDraft {
        EventDraft {
            event_type: AGENT_TEXT.to_owned(),
            payload: json!({ "text": text }),
            captured_at: None,
        }
    }

    #[test]
    fn append_draft_refuses_a_second_run_finished() {
        let mut sink = InMemorySink::new(|| "2026-09-07T00:00:00.000Z".to_owned());
        sink.append_draft(
            "run-1",
            run_finished_draft(json!({ "outcome": "completed", "durationMs": 1 })),
        )
        .unwrap();

        let error = sink
            .append_draft(
                "run-1",
                run_finished_draft(json!({ "outcome": "completed", "durationMs": 2 })),
            )
            .unwrap_err();

        assert_eq!(
            error,
            RunClosedError {
                run_id: "run-1".to_owned()
            }
        );
        assert_eq!(
            error.to_string(),
            "run run-1 already recorded a terminal event; no further events are accepted for it"
        );
    }

    #[test]
    fn append_draft_refuses_agent_text_after_run_finished() {
        let mut sink = InMemorySink::new(|| "2026-09-07T00:00:00.000Z".to_owned());
        sink.append_draft(
            "run-1",
            run_finished_draft(json!({ "outcome": "completed", "durationMs": 1 })),
        )
        .unwrap();

        let error = sink
            .append_draft("run-1", agent_text_draft("too late"))
            .unwrap_err();

        assert_eq!(
            error,
            RunClosedError {
                run_id: "run-1".to_owned()
            }
        );
    }

    #[test]
    fn append_event_refuses_a_second_run_finished_distinctly_from_a_gap() {
        let mut sink = InMemorySink::new(|| "unused".to_owned());
        sink.append_event(complete_event()).unwrap();
        let finished = Event {
            event_type: RUN_FINISHED.to_owned(),
            seq: 2,
            payload: json!({ "outcome": "completed", "durationMs": 1 }),
            ..complete_event()
        };
        sink.append_event(finished.clone()).unwrap();

        // The closed run refuses via `RunClosed`...
        let closed_error = sink
            .append_event(Event {
                event_type: AGENT_TEXT.to_owned(),
                seq: 3,
                payload: json!({ "text": "too late" }),
                ..complete_event()
            })
            .unwrap_err();
        let AppendError::RunClosed(run_closed) = closed_error else {
            panic!("expected AppendError::RunClosed, got {closed_error:?}");
        };
        assert_eq!(run_closed.run_id, "run-1");
        assert_eq!(
            run_closed.to_string(),
            "run run-1 already recorded a terminal event; no further events are accepted for it"
        );

        // ...while a *still-open* run with the same kind of skipped seq
        // refuses via `Sequence` instead: the two failure modes stay
        // distinguishable rather than one swallowing the other.
        let mut other_sink = InMemorySink::new(|| "unused".to_owned());
        other_sink.append_event(complete_event()).unwrap();
        let gap_error = other_sink
            .append_event(Event {
                seq: 3,
                ..complete_event()
            })
            .unwrap_err();
        let AppendError::Sequence(sequence_error) = gap_error else {
            panic!("expected AppendError::Sequence, got {gap_error:?}");
        };
        assert_ne!(
            sequence_error.to_string(),
            run_closed.to_string(),
            "a sequence gap and a closed run must report different messages"
        );
    }

    #[test]
    fn append_event_refuses_a_gap_on_a_closed_run_as_run_closed_not_sequence() {
        // Once a run is closed, *any* further append is refused as
        // `RunClosed` — even one that also happens to skip a seq. `RunClosed`
        // is checked first, so this is not misreported as a gap.
        let mut sink = InMemorySink::new(|| "unused".to_owned());
        sink.append_event(complete_event()).unwrap();
        sink.append_event(Event {
            event_type: RUN_FINISHED.to_owned(),
            seq: 2,
            payload: json!({ "outcome": "completed", "durationMs": 1 }),
            ..complete_event()
        })
        .unwrap();

        let error = sink
            .append_event(Event {
                event_type: AGENT_TEXT.to_owned(),
                seq: 99,
                payload: json!({ "text": "too late" }),
                ..complete_event()
            })
            .unwrap_err();

        assert!(
            matches!(error, AppendError::RunClosed(_)),
            "error was: {error:?}"
        );
    }

    #[test]
    fn append_event_refuses_a_real_control_applied_after_run_finished() {
        // A `control.applied` sounds like the one post-terminal event that
        // "surely" should still be recordable — an interrupt landing just
        // after the run ends. Ruled on onsager-ai/umwelt#1: a closed run accepts
        // nothing after `run.finished`, control events included, and this is
        // refused the same way as any other post-terminal append: as
        // `RunClosed`, not `Sequence`, even though this append's `seq` is
        // otherwise the expected next value.
        let mut sink = InMemorySink::new(|| "unused".to_owned());
        sink.append_event(complete_event()).unwrap();
        sink.append_event(Event {
            event_type: RUN_FINISHED.to_owned(),
            seq: 2,
            payload: json!({ "outcome": "completed", "durationMs": 1 }),
            ..complete_event()
        })
        .unwrap();

        let before = sink.events("run-1").to_vec();

        let error = sink
            .append_event(Event {
                event_type: CONTROL_APPLIED.to_owned(),
                seq: 3,
                payload: json!({ "controlId": "control-1", "ok": true }),
                ..complete_event()
            })
            .unwrap_err();

        let AppendError::RunClosed(run_closed) = error else {
            panic!(
                "expected AppendError::RunClosed for a control.applied appended after run.finished, got {error:?}"
            );
        };
        assert_eq!(run_closed.run_id, "run-1");

        assert_eq!(
            sink.events("run-1").to_vec(),
            before,
            "a refused control.applied append must not change stored events"
        );
        assert_eq!(
            sink.events("run-1").len(),
            2,
            "a refused control.applied append must not consume a seq"
        );
    }

    #[test]
    fn refused_append_leaves_stored_events_and_seq_unchanged() {
        let mut sink = InMemorySink::new(|| "2026-09-07T00:00:00.000Z".to_owned());
        sink.append_draft(
            "run-1",
            run_finished_draft(json!({ "outcome": "completed", "durationMs": 1 })),
        )
        .unwrap();

        let before = sink.events("run-1").to_vec();
        assert_eq!(before.len(), 1);

        sink.append_draft("run-1", agent_text_draft("too late"))
            .unwrap_err();
        sink.append_event(Event {
            event_type: AGENT_TEXT.to_owned(),
            seq: 2,
            payload: json!({ "text": "also too late" }),
            ..complete_event()
        })
        .unwrap_err();

        let after = sink.events("run-1").to_vec();
        assert_eq!(
            after, before,
            "a refused append must not change stored events"
        );
        assert_eq!(after.len(), 1, "a refused append must not consume a seq");

        // The seq counter, not just the event count, is unchanged: the next
        // legitimate draft for a still-open run would take seq 2, so proving
        // that on a fresh run confirms `next_seq` was never advanced here by
        // the refused appends above (this run stays closed, so it cannot
        // accept a "next legitimate" append itself).
        let mut other_sink = InMemorySink::new(|| "2026-09-07T00:00:00.000Z".to_owned());
        other_sink
            .append_draft("run-2", agent_text_draft("first"))
            .unwrap();
        let second = other_sink
            .append_draft("run-2", agent_text_draft("second"))
            .unwrap();
        assert_eq!(second.seq, 2);
    }

    #[test]
    fn closing_one_run_does_not_close_another() {
        let mut sink = InMemorySink::new(|| "2026-09-07T00:00:00.000Z".to_owned());
        sink.append_draft(
            "run-1",
            run_finished_draft(json!({ "outcome": "completed", "durationMs": 1 })),
        )
        .unwrap();

        assert!(
            sink.append_draft("run-1", agent_text_draft("too late"))
                .is_err()
        );
        assert!(sink.append_draft("run-2", agent_text_draft("fine")).is_ok());
        assert_eq!(sink.events("run-2").len(), 1);
    }

    #[test]
    fn a_run_without_run_finished_keeps_accepting_appends() {
        let mut sink = InMemorySink::new(|| "2026-09-07T00:00:00.000Z".to_owned());
        sink.append_draft("run-1", agent_text_draft("one")).unwrap();
        sink.append_draft("run-1", agent_text_draft("two")).unwrap();
        let third = sink
            .append_draft("run-1", agent_text_draft("three"))
            .unwrap();

        assert_eq!(third.seq, 3);
        assert_eq!(sink.events("run-1").len(), 3);
    }

    /// Builds an `Event<RunStartedPayload>` around a hand-written payload
    /// JSON body, going through the typed struct (not `Value`) so these
    /// tests exercise the `#[serde(flatten)]` extension field a caller using
    /// the typed API directly would rely on for retention (issue onsager-ai/ethogram#12), not
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
        // reappear in the canonical sorted position (issue onsager-ai/ethogram#12).
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
    fn payload_without_unknown_fields_serialises_exactly_as_before() {
        // The extension field must not surface as an empty object when there
        // is nothing unknown to carry (issue onsager-ai/ethogram#12).
        let input = r#"{"kind":"loop","actor":"builder","harness":"codex"}"#;

        let serialised = serialise_event(&typed_run_started_event(input)).unwrap();
        assert_eq!(
            serialised,
            r#"{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","harness":"codex","kind":"loop"}}"#
        );
        assert!(!serialised.contains("extra"));
    }

    /// Cross-SDK byte identity for an event with an unknown payload field
    /// (issue onsager-ai/ethogram#12). This exact literal is also hand-built in the TypeScript
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

    // -- run.finished durationMs is a required u64 (follow-up to issue onsager-ai/ethogram#6) --

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

    /// For each of the thirteen known types, builds a minimally valid event of
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
        assert_eq!(
            round_tripped_type(
                CONTROL_REQUESTED,
                json!({ "controlId": "control-1", "kind": "steer", "by": "operator" }),
            ),
            CONTROL_REQUESTED
        );
        assert_eq!(
            round_tripped_type(
                CONTROL_APPLIED,
                json!({ "controlId": "control-1", "ok": true }),
            ),
            CONTROL_APPLIED
        );
        assert_eq!(
            round_tripped_type(
                CAPTURE_REFUSED,
                json!({ "cause": "gap", "sourceRunId": "run-source" }),
            ),
            CAPTURE_REFUSED
        );
        assert_eq!(
            round_tripped_type(DECISION_REQUESTED, minimal_decision_request("permission"),),
            DECISION_REQUESTED
        );
        assert_eq!(
            round_tripped_type(
                DECISION_ANSWERED,
                json!({
                    "decisionId": "decision-1",
                    "optionId": "allow",
                    "by": "principal:user:alice"
                }),
            ),
            DECISION_ANSWERED
        );
    }

    #[test]
    fn known_types_holds_exactly_the_thirteen_recognised_types_with_no_duplicates() {
        assert_eq!(KNOWN_TYPES.len(), 13);

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
                CONTROL_REQUESTED,
                CONTROL_APPLIED,
                CAPTURE_REFUSED,
                DECISION_REQUESTED,
                DECISION_ANSWERED,
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
