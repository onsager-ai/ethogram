/** The closed set of validation failures a sink maps to capture.refused. */
export type ValidationErrorDetails =
  | { kind: "OverBound"; path: string; count: number; max: number }
  | { kind: "PayloadTooLarge"; bytes: number; max: number }
  | { kind: "UnknownMember"; path: string; value: string }
  | { kind: "MissingField"; path: string }
  // A producer broke a stated rule while sending an otherwise
  // representable value: the steer-without-text rule, or one of the three
  // `onTimeout` checks.
  | { kind: "Policy"; path: string; message: string }
  // The value sent could not be represented at all: wrong-typed, an
  // unsafe integer, or not serialisable. Distinct from `Policy`, which is a
  // stated rule broken by an otherwise representable value.
  | { kind: "Malformed"; path: string; message: string };

/**
 * A validation failure with structured fields and the original message.
 * Switch on `details.kind` to narrow its fields. The inherited TypeError
 * name is intentional: existing message matches and TypeError checks hold.
 */
export class ValidationError extends TypeError {
  readonly details: Readonly<ValidationErrorDetails>;

  constructor(details: ValidationErrorDetails, message: string) {
    super(message);
    this.details = Object.freeze({ ...details });
  }

  get kind(): ValidationErrorDetails["kind"] {
    return this.details.kind;
  }

  toJSON(): ValidationErrorDetails {
    return { ...this.details };
  }
}

/** Same UTF-8 key ordering and number notation as the event payload. */
export function serialiseValidationError(error: ValidationError): string {
  return serialisePayloadCanonical(error.toJSON());
}

// Convert a field label, never a diagnostic message, to its wire path.
function payloadPath(field: string): string {
  if (
    field === "payload" ||
    field.startsWith("payload.") ||
    field.startsWith("payload[")
  ) {
    return field;
  }
  const dot = field.indexOf(".");
  return dot === -1 ? "payload" : `payload${field.slice(dot)}`;
}

// Shared parsing helpers keep representability checks and their order in
// one place. validate promotes this metadata to the public error; parsing
// still reports TypeError without applying any producer policy.
class PayloadRepresentationError extends TypeError {
  readonly path: string;

  constructor(field: string, message: string, readonly missing = false) {
    super(message);
    this.path = payloadPath(field);
  }
}

function policyError(path: string, message: string): ValidationError {
  return new ValidationError({ kind: "Policy", path, message }, message);
}

function malformedError(path: string, message: string): ValidationError {
  return new ValidationError({ kind: "Malformed", path, message }, message);
}

export const EVENT_SCHEMA_VERSION = 1 as const;

/** Maximum number of Unicode scalar values carried by an `agent.text`. */
export const MAX_TEXT_SCALARS = 16_384 as const;

/** Maximum scalars carried by any excerpted field other than `agent.text`. */
export const MAX_EXCERPT_SCALARS = 4_096 as const;

/**
 * Maximum serialised size, in UTF-8 bytes, of a payload **alone** — the
 * payload only, never the envelope around it — measured on the same
 * canonical compact serialisation `serialiseEvent` produces for it (issue
 * onsager-ai/ethogram#28). `validate` enforces this for every event, including one whose
 * `eventType` it does not recognise, as a floor under `MAX_TEXT_SCALARS` and
 * `MAX_EXCERPT_SCALARS`: those bound named fields on types this SDK knows,
 * and have nothing to say about an unrecognised type's payload. `parseEvent`
 * leaves this bound unenforced, exactly as it leaves the scalar bounds
 * unenforced: an over-large payload is still perfectly representable, and a
 * forwarder must still be able to relay it.
 *
 * 131,072 (128 KiB), not the 65,536 (64 KiB) first proposed for it. A scalar
 * value is at most four UTF-8 bytes, so a text at exactly `MAX_TEXT_SCALARS`
 * composed of astral-plane characters is 16,384 × 4 = 65,536 bytes on its
 * own — the entire 64 KiB budget, with nothing left over for the rest of the
 * payload. A producer using `excerpt()` exactly as specified on emoji-heavy
 * text would then emit an `agent.text` this bound rejects. Doubling the
 * budget keeps the scalar bound binding first for text, which is the
 * intended relationship: this byte bound is a backstop against an unbounded
 * *unknown* payload, not a second opinion about text.
 */
export const MAX_PAYLOAD_BYTES = 131_072 as const;

/**
 * The result of `excerpt`: the kept text, and whether a bound applied.
 *
 * On the wire the corresponding payload field is optional, and **its absence
 * means `false`**. A producer may also emit `false` explicitly, and both forms
 * are conforming — the protocol says nothing stronger (ruled on onsager-ai/ethogram#12).
 * Canonicalisation governs *notation*, not *presence*: it fixes how a value is
 * spelled once written, and does not decide whether an optional field is
 * written at all. Requiring omission would have made goldens already derived
 * from real captures retroactively non-conforming, for no benefit a consumer
 * can observe, since a reader must handle an absent flag either way.
 *
 * The corpus therefore carries both forms, which is the useful outcome: it
 * proves each round-trips, rather than asserting a preference no producer
 * agreed to.
 */
export interface Excerpt {
  text: string;
  truncated: boolean;
}

/**
 * Keep at most `max` Unicode scalar values from `text`. JavaScript's string
 * iterator advances by code point rather than UTF-16 code unit, so an astral
 * character is retained whole instead of being cut into a lone surrogate.
 * This deliberately does not attempt grapheme-cluster segmentation.
 *
 * A JavaScript string can also already contain a *lone* (unpaired) surrogate
 * — a code unit in `U+D800`-`U+DFFF` with no matching partner — because
 * JavaScript strings are UTF-16 and do not enforce well-formedness the way a
 * Rust `String` does. Rust's `excerpt()` needs no equivalent handling: a
 * `&str` is guaranteed well-formed UTF-8 and cannot hold an unpaired
 * surrogate in the first place. Left intact here, such a code unit would
 * serialise to JSON that `JSON.parse` round-trips but `serde_json` rejects,
 * so the two SDKs could not agree on the resulting event. Per the ruling on
 * issue onsager-ai/ethogram#6, every lone surrogate this function encounters is therefore
 * replaced with `U+FFFD` (the replacement character), whether or not the
 * text ends up truncated; this is silent by design and does not affect
 * `truncated`, which continues to mean only that the bound was hit. A valid
 * surrogate *pair* — how every astral character such as `😀` is encoded — is
 * left completely alone: replacement is one code point in, one code point
 * out, so it never changes how many scalar values the bound counts.
 */
export function excerpt(text: string, max: number): Excerpt {
  const kept: string[] = [];
  for (const scalar of text) {
    if (kept.length >= max) {
      return { text: kept.join(""), truncated: true };
    }
    kept.push(isLoneSurrogateScalar(scalar) ? REPLACEMENT_CHARACTER : scalar);
  }
  return { text: kept.join(""), truncated: false };
}

/** The Unicode replacement character, `U+FFFD`. */
const REPLACEMENT_CHARACTER = "\uFFFD";

/**
 * True when `scalar` — one item yielded by iterating a string by code point
 * — is a lone (unpaired) surrogate rather than a BMP character or a valid
 * surrogate pair. The string iteration protocol only ever combines a high
 * surrogate with an immediately following low surrogate into a single
 * two-code-unit item; any surrogate that could not be paired comes through
 * as its own one-code-unit item, which is exactly what this checks for.
 */
function isLoneSurrogateScalar(scalar: string): boolean {
  if (scalar.length !== 1) {
    return false;
  }
  const unit = scalar.charCodeAt(0);
  return unit >= 0xd800 && unit <= 0xdfff;
}

export const RUN_KINDS = [
  "loop",
  "handoff",
  "subagent",
  "session",
  "judgment",
  "relay",
] as const;

export type KnownRunKind = (typeof RUN_KINDS)[number];

/**
 * A run kind this SDK knows, or an unfamiliar wire string retained verbatim
 * for a newer vocabulary. Consumers must render it with its raw value, must
 * never map it onto a known kind, and, when acting on it, must treat it as
 * "not this", never as a default.
 *
 * Each member is decided by a fact a producer can check rather than by what
 * its name suggests (onsager-ai/ethogram#64):
 *
 * - `"subagent"` — a run started by another run and observed under it.
 *   Decided by: `parentRunId` present, with `parentToolUseId` when a tool call
 *   spawned it.
 * - `"relay"` — a long-lived process that observes other runs and emits on its
 *   own run, and changes nothing itself. Decided by: it emits about other
 *   runs, with no work order and no repository change.
 * - `"loop"` — a run started by a schedule the operator declared, recurring at
 *   that cadence. Decided by: `schedule` present, and the scheduler started it
 *   rather than a person or a dispatch.
 * - `"session"` — an interactive harness session in which the harness's own
 *   user initiates the turns. Decided by: a person started it at the harness,
 *   with `actor` naming the harness's notion of that user; no `schedule` and
 *   no work order.
 * - `"judgment"` — a run whose product is a decision or verdict record and
 *   nothing else: an answer to a queued item, a gate evaluated on demand.
 *   Decided by: it emits `decision.*` or a verdict and changes no repository,
 *   and a principal's or operator's command started it.
 * - `"handoff"` — a run in which an orchestrator or principal dispatches a
 *   work order to an agent to carry out unattended, once. Decided by:
 *   `workOrder`, or an equivalent intent reference, present; no `schedule`; no
 *   `parentRunId`; and the product is work rather than a verdict.
 *
 * They are **ordered**, so no run fits two: apply the checks in the order
 * listed above and take the first that holds. That order is what settles the
 * otherwise ambiguous cases — a scheduled gatekeeper pass is a `"loop"`,
 * because a schedule outranks what the run produces, while the same
 * evaluation run on demand is a `"judgment"`.
 */
export type RunKind = KnownRunKind | (string & {});

export const RUN_OUTCOMES = [
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
] as const;

export type KnownRunOutcome = (typeof RUN_OUTCOMES)[number];

/**
 * A run outcome this SDK knows, or an unfamiliar wire string retained
 * verbatim for a newer vocabulary. Consumers must render it with its raw
 * value, must never map it onto a known outcome, and, when acting on it,
 * must treat it as "not this", never as a default.
 *
 * `"blocked"` ended because the run may not proceed until something outside
 * the run changes; **not an error of the run**. A consumer that colours
 * `"blocked"` as a failure is misreporting, since nothing went wrong inside
 * the run.
 *
 * `"unstarted"` means the run's process never ran; `reason` names why —
 * `"disarmed"`, `"spawn"`, or a missing binary, excerpted as today. Distinct
 * from `"failed"`, where the process ran and did not succeed.
 */
export type RunOutcome = KnownRunOutcome | (string & {});

/**
 * Enforced limits declared by the runtime. An absent ceiling means unbounded
 * and unenforced, not defaulted; consumers must not substitute a default.
 */
export interface RunCeilings {
  costUsd?: number;
  tokens?: number;
  /** Wall-clock bound; reaching it ends the run as `timed-out`. */
  wallMs?: number;
  /**
   * Idle-time bound; reaching it ends the run as `timed-out`. It is suspended
   * during an in-flight tool call. A harness that cannot enforce it omits it.
   */
  idleMs?: number;
  /** Maximum number of turns the run may take (the bound, not the actual). */
  turns?: number;
}

export interface RunStartedPayload {
  kind: RunKind;
  actor: string;
  harness: string;
  model?: string;
  parentRunId?: string;
  parentToolUseId?: string;
  schedule?: string;
  repository?: string;
  workOrder?: string;
  ceilings?: RunCeilings;
}

export interface RunUsage {
  inputTokens?: number;
  outputTokens?: number;
  cacheReadTokens?: number;
  cacheCreationTokens?: number;
  unit?: string;
}

/**
 * Closes a run and carries the runtime's own computed totals.
 *
 * `costUsd` and `usage` here are the runtime's own reckoning for the run as
 * a whole, computed once at the point the run ends. The two fields are not
 * interchangeable in how a consumer would reconstruct them from
 * `agent.completed`, and that asymmetry is worth stating plainly rather than
 * leaving it to be discovered: `usage` needs no special handling, because
 * the harness reports token counts per invocation, so summing
 * `agent.completed.usage` across every completion in the run agrees with
 * this field, exactly as it does for `turns` and `durationMs`. `costUsd`
 * does not, because the harness instead reports cost as a running total for
 * the harness session that produced it — `agent.completed.costUsd` is
 * cumulative per `sessionId` rather than per invocation, and naively
 * summing it over every `agent.completed` in a run over-counts whenever a
 * session reports more than once. Reconstructing it therefore needs the
 * maximum observed within each `sessionId`, summed only across distinct
 * sessions (see `AgentCompletedPayload`'s doc comments for why). This
 * payload is the number to trust for the run either way.
 */
export interface RunFinishedPayload {
  outcome: RunOutcome;
  /** Bounded explanation of a terminal outcome. */
  reason?: string;
  truncated?: boolean;
  costUsd?: number;
  usage?: RunUsage;
  durationMs: number;
  estimated?: boolean;
}

export interface AgentStartedPayload {
  stage?: string;
  model?: string;
  sessionId?: string;
  pid?: number;
}

export interface AgentTextPayload {
  stage?: string;
  text: string;
  truncated?: boolean;
  parentToolUseId?: string;
}

export interface AgentToolUsePayload {
  stage?: string;
  tool: string;
  inputExcerpt?: string;
  truncated?: boolean;
  toolUseId?: string;
  parentToolUseId?: string;
}

export interface AgentToolResultPayload {
  stage?: string;
  tool: string;
  isError?: boolean;
  resultExcerpt?: string;
  truncated?: boolean;
  toolUseId?: string;
  parentToolUseId?: string;
}

export interface AgentCompletedPayload {
  stage?: string;
  /**
   * Number of turns *this invocation* took (the actual, not the ceiling
   * bound in `RunCeilings.turns`). Like `usage` and `durationMs` below and
   * unlike `costUsd`, this is per invocation rather than cumulative per
   * session, so it is safe to sum across every `agent.completed` in a run.
   */
  turns?: number;
  /**
   * Echoes the harness session identifier `agent.started` already carries,
   * so this completion can state which session's running cost total it is
   * reporting. `costUsd` below is cumulative per session rather than per
   * invocation — unlike `usage` beside it, see its doc comment for why —
   * and that rule was unusable from a completion alone before this field
   * existed: `sessionId` appeared only on `agent.started`, so a consumer
   * had to correlate backwards to whichever `agent.started` opened the
   * session before it could safely take a maximum within a session or sum
   * across sessions. Carrying it here too makes the rule applicable from
   * the very event that states the cost total it governs.
   */
  sessionId?: string;
  /**
   * Cumulative for the harness session named by this payload's own
   * `sessionId` (which echoes the `sessionId` on the `agent.started` that
   * opened it), not per invocation: this is the running total as of *this*
   * completion, so a session that reports `agent.completed` more than once
   * reports an increasing total each time rather than a fresh delta.
   * Summing every `agent.completed.costUsd` in a run therefore over-counts
   * whenever a session reports more than once — take the maximum observed
   * within each `sessionId` instead, and sum only across distinct sessions.
   *
   * This is genuinely asymmetric with `usage` immediately below, which sums
   * cleanly across invocations with no such caveat: the harness reports
   * cost as a running total for the whole session but reports token counts
   * per invocation, and each field here only ever reflects what the
   * harness itself reports. `run.finished.costUsd` carries the runtime's
   * own computed total for the whole run and is the number to trust there.
   */
  costUsd?: number;
  model?: string;
  /**
   * Unlike `costUsd` just above, this carries no cumulative-per-session
   * caveat: the harness reports token counts per invocation rather than as
   * a running session total, so this is a fresh delta each time, and
   * summing every `agent.completed.usage` in a run agrees with
   * `run.finished.usage`, which still carries the runtime's own computed
   * total for the run and remains the number to trust there. Do not assume
   * this field behaves like `costUsd` merely because they sit next to each
   * other and share a `sessionId` — the harness reports the two totals on
   * different bases, and this field's rule follows from that, not from any
   * pattern shared with its neighbour.
   */
  usage?: RunUsage;
  /**
   * Wall time *this invocation* took. Like `turns` and `usage` above and
   * unlike `costUsd`, this is per invocation rather than cumulative per
   * session, so it is safe to sum across every `agent.completed` in a run.
   */
  durationMs?: number;
  estimated?: boolean;
}

export interface AgentWarningPayload {
  stage?: string;
  /** Bounded non-terminal warning text. */
  message: string;
}

export const CONTROL_KINDS = ["interrupt", "steer", "answer"] as const;

export type KnownControlKind = (typeof CONTROL_KINDS)[number];

/**
 * A control kind this SDK knows, or an unfamiliar wire string retained
 * verbatim for a newer vocabulary. Consumers must render it with its raw
 * value, must never map it onto a known kind, and, when acting on it, must
 * treat it as "not this", never as a default.
 * A string spelling a known kind always receives that kind's validation
 * rules; TypeScript has no separate runtime Unknown wrapper.
 *
 * There is deliberately no `"pause"` member: no harness the operator uses can
 * pause headlessly, and a verb the runtime cannot honour is a lie in a type.
 */
export type ControlKind = KnownControlKind | (string & {});

/**
 * Requests that the runtime interrupt, steer, or answer a waiting decision.
 * Emitted by the run's runtime, never by the console: a console that shows a run as
 * interrupted before the corresponding `control.applied` arrives has misread
 * the protocol.
 */
export interface ControlRequestedPayload {
  controlId: string;
  kind: ControlKind;
  /** Required for `answer` and absent for every other kind, at validation. */
  decisionId?: string;
  /** Required for `answer` and absent for every other kind, at validation. */
  optionId?: string;
  /**
   * For `steer`, the message queued for the run's next turn. Bounded at
   * capture to `MAX_EXCERPT_SCALARS`, per `truncated` below. `steer` is
   * between turns: mid-turn injection is not available headlessly on Claude
   * Code or Codex, and the protocol does not pretend otherwise. A runtime
   * honours `steer` by resuming the session with this text as the next user
   * turn. Absent on `answer`.
   */
  text?: string;
  truncated?: boolean;
  /** The principal identity that made the request. */
  by: string;
}

/**
 * Records whether a `control.requested` request was honoured. Emitted by the
 * run's runtime, never by the console. For an `interrupt`, `run.finished`
 * with `outcome: "interrupted"` is emitted after this event, not before.
 */
export interface ControlAppliedPayload {
  controlId: string;
  ok: boolean;
  /**
   * The principal identity that applied the control, with the same meaning as
   * `by` on `control.requested`: an identity a consumer renders and never
   * interprets. Optional, because a runtime echoing a control it does not
   * support may have no separate applier to name (onsager-ai/ethogram#67).
   */
  by?: string;
  /**
   * Required when `ok` is false; also permitted on a positive echo.
   * Unknown explanations are bounded at capture, per `truncated` below.
   */
  reason?: ControlAppliedReason;
  truncated?: boolean;
  /** For an `interrupt`, the `toolUseId` the kill landed inside, if any. */
  landedIn?: string;
}

export const CONTROL_APPLIED_REASONS = [
  "no-such-decision",
  "already-answered",
  "option-not-offered",
  "unsupported",
  "not-live",
  "rejected",
] as const;

export type KnownControlAppliedReason = (typeof CONTROL_APPLIED_REASONS)[number];

/**
 * Open at parse and validation: an unfamiliar reason here is the expected
 * case, not the exceptional one, so it is accepted and retained exactly,
 * subject to the excerpt bound. Consumers must render it with its raw
 * value, must never map it onto a known reason, and, when acting on it,
 * must treat it as "not this", never as a default.
 */
export type ControlAppliedReason = KnownControlAppliedReason | (string & {});

export const CAPTURE_REFUSAL_CAUSES = [
  "over_bound",
  "gap",
  "duplicate",
  "finished",
  "malformed",
] as const;

export type KnownCaptureRefusalCause =
  (typeof CAPTURE_REFUSAL_CAUSES)[number];

/**
 * A capture-refusal cause this SDK knows, or an unfamiliar wire string
 * retained verbatim for a newer vocabulary. Consumers must render it with
 * its raw value, must never map it onto a known cause, and, when acting on
 * it, must treat it as "not this", never as a default.
 */
export type CaptureRefusalCause =
  | KnownCaptureRefusalCause
  | (string & {});

/**
 * Records an event refused by a relay or capturing runtime on that runtime's
 * own run. It names the source run without embedding the refused content,
 * whose size may be the reason for refusal.
 */
export interface CaptureRefusedPayload {
  cause: CaptureRefusalCause;
  sourceRunId: string;
  sourceSeq?: number;
  sourceType?: string;
  field?: string;
  count?: number;
  max?: number;
  /**
   * A bounded, excerpted parser or validation message for `malformed`.
   * `truncated` records whether it was excerpted.
   */
  detail?: string;
  truncated?: boolean;
}

export const DECISION_KINDS = [
  "permission",
  "tripwire",
  "gate_inconclusive",
  "human_decides",
  "budget",
] as const;

export type KnownDecisionKind = (typeof DECISION_KINDS)[number];

/**
 * A decision kind this SDK knows, or an unfamiliar wire string retained
 * verbatim for a newer vocabulary. Consumers must render it with its raw
 * value, must never map it onto a known kind, and, when acting on it, must
 * treat it as "not this", never as a default.
 */
export type DecisionKind = KnownDecisionKind | (string & {});

/**
 * Bounded narration supplied with a `decision.requested`. All four content
 * fields are checked against `MAX_EXCERPT_SCALARS` by `validate`, while parsing
 * enforces only representability so a forwarder can still carry an over-bound
 * dossier. `truncated` applies to the dossier as a whole rather than to each
 * narration field separately.
 */
export interface DecisionDossier {
  question: string;
  optionsRuledOut: string[];
  recommendedAction: string;
  blastRadius: string;
  truncated?: boolean;
}

/** One answer a human may choose. `id` is unbounded; `label` is narration. */
export interface DecisionOption {
  id: string;
  label: string;
}

/**
 * Opens a decision owned by this run. The dossier and option labels carry
 * bounded narration; `subject` is only a reference such as a PR URL, issue
 * number, or tool name, never the subject's content.
 *
 * `onTimeout` is enforced rather than conventional: it is allowed only for
 * `permission`, its only permitted value is `"deny"`, and that value must
 * name one of this request's own `options[].id` — a request cannot declare a
 * timeout action it never offered. A tripwire that auto-proceeded on timeout
 * would violate ostrom's "never auto-proceed" rule; enforcing the
 * restriction in `validate` prevents a producer from shipping that mistake
 * quietly. `parseEvent` deliberately does not apply this policy, because a
 * forwarder must retain any representable request.
 *
 * The corresponding `decision.answered` is not emitted on this request's own
 * run: it is emitted later by whatever invocation applies the answer, on
 * that invocation's own run, by which point this run has usually already
 * finished. The two events are correlated only by `decisionId`, never by
 * sharing a `runId`.
 */
export interface DecisionRequestedPayload {
  /** Producer-assigned and unique within the run. */
  decisionId: string;
  kind: DecisionKind;
  dossier: DecisionDossier;
  options: DecisionOption[];
  subject?: string;
  /** Optional ISO-8601 expiry time. */
  expiresAt?: string;
  /** Option id applied after expiry. Absence leaves the decision open. */
  onTimeout?: string;
}

/**
 * Records an answer after it has been applied. It is emitted by **the
 * invocation that applies the answer, on its own run** — not the run that
 * requested the decision, which has usually already finished by the time a
 * human responds. The two events are correlated only by `decisionId`, never
 * by sharing a `runId`; it is never emitted by a console that merely
 * collected the answer.
 *
 * This wording is a correction (ruled on onsager-ai/ethogram#7). The previous wording said this
 * event was emitted by the run that owns the decision, but that describes
 * something the protocol's own rules forbid: a run has at most one
 * `run.finished`, and a sink refuses every append to a closed run. A
 * decision a human answers minutes or hours later is answered after the
 * requesting run has terminated, so an answer emitted "on the owning run"
 * would be refused by the sink. `requestedRunId`, below, exists because of
 * this correction: once the two events routinely live on different runs, a
 * consumer holding only the answer needs a way to find the run that asked.
 *
 * `byTimeout` is semantically material. Without it, a human choosing
 * `optionId: "deny"` is indistinguishable from a permission expiring
 * unanswered with the same option and a runtime principal in `by`. A timeout
 * is not a decision with a long gap; it is nobody deciding. Absence means
 * false. `reversal` names the identifier that would undo this answer; see
 * its own doc comment below for the two forms it may take and why neither is
 * checked against the request.
 */
export interface DecisionAnsweredPayload {
  decisionId: string;
  optionId: string;
  /** A principal identity a consumer resolves, never a display name. */
  by: string;
  byTimeout?: boolean;
  /**
   * The identifier that would undo this answer, if the producer accepts one.
   * Two forms (ruled on onsager-ai/ethogram#7):
   *
   * - an offered `options[].id`, or
   * - a `<verb>:<subject>` **action id** — `revoke:required_checks` undoes
   *   `excuse:required_checks`, even though `revoke:required_checks` was
   *   never among the options offered to the human, because those options
   *   were about whether to excuse, not about how to later revoke.
   *
   * Either form is meaningful only because **the producer accepts its own
   * reversal ids as a subsequent `optionId` on this decision** — that
   * acceptance is what makes an unoffered id legible rather than arbitrary.
   * It follows that `reversal` is therefore not checkable against the
   * request: `validateDecisionAnswerAgainstRequest` does not check it. The
   * alternative — requiring membership in `options[].id` — would refuse a
   * legitimate undo that the producer will honour, which is worse than not
   * checking at all. `validate` still only checks that, when present, this
   * is a string.
   */
  reversal?: string;
  /**
   * The run that emitted the corresponding `decision.requested`.
   * `decisionId` correlates the pair, but a consumer holding only the answer
   * cannot find the asking run without this field — and now that the two
   * events live on different runs, that lookup is the common case rather
   * than an edge one.
   */
  requestedRunId?: string;
}

/** The wire string for a `run.started` event's `type` field. */
export const RUN_STARTED = "run.started" as const;
/** The wire string for a `run.finished` event's `type` field. */
export const RUN_FINISHED = "run.finished" as const;
/** The wire string for an `agent.started` event's `type` field. */
export const AGENT_STARTED = "agent.started" as const;
/** The wire string for an `agent.text` event's `type` field. */
export const AGENT_TEXT = "agent.text" as const;
/** The wire string for an `agent.tool_use` event's `type` field. */
export const AGENT_TOOL_USE = "agent.tool_use" as const;
/** The wire string for an `agent.tool_result` event's `type` field. */
export const AGENT_TOOL_RESULT = "agent.tool_result" as const;
/** The wire string for an `agent.completed` event's `type` field. */
export const AGENT_COMPLETED = "agent.completed" as const;
/** The wire string for an `agent.warning` event's `type` field. */
export const AGENT_WARNING = "agent.warning" as const;
/** The wire string for a `control.requested` event's `type` field. */
export const CONTROL_REQUESTED = "control.requested" as const;
/** The wire string for a `control.applied` event's `type` field. */
export const CONTROL_APPLIED = "control.applied" as const;
/** The wire string for a `capture.refused` event's `type` field. */
export const CAPTURE_REFUSED = "capture.refused" as const;
/** The wire string for a `decision.requested` event's `type` field. */
export const DECISION_REQUESTED = "decision.requested" as const;
/** The wire string for a `decision.answered` event's `type` field. */
export const DECISION_ANSWERED = "decision.answered" as const;

/**
 * Every event `type` this SDK has a typed payload for. This is not a closed
 * vocabulary: `parseEvent` still accepts a type it has never heard of (see
 * `parseKnownPayload`'s fallthrough), and a consumer may still match a
 * literal for vocabulary this SDK has not learned. A constant is a name for
 * a string, not a gate.
 */
export const KNOWN_TYPES = [
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
] as const;

export type KnownType = (typeof KNOWN_TYPES)[number];

/**
 * The protocol payload map. Supplying it to `EventDraft` or `Event` produces a
 * correlated discriminated union; their unparameterised forms deliberately
 * remain open for callers that only need the envelope or handle future types.
 */
export interface EventPayloadMap {
  [RUN_STARTED]: RunStartedPayload;
  [RUN_FINISHED]: RunFinishedPayload;
  [AGENT_STARTED]: AgentStartedPayload;
  [AGENT_TEXT]: AgentTextPayload;
  [AGENT_TOOL_USE]: AgentToolUsePayload;
  [AGENT_TOOL_RESULT]: AgentToolResultPayload;
  [AGENT_COMPLETED]: AgentCompletedPayload;
  [AGENT_WARNING]: AgentWarningPayload;
  [CONTROL_REQUESTED]: ControlRequestedPayload;
  [CONTROL_APPLIED]: ControlAppliedPayload;
  [CAPTURE_REFUSED]: CaptureRefusedPayload;
  [DECISION_REQUESTED]: DecisionRequestedPayload;
  [DECISION_ANSWERED]: DecisionAnsweredPayload;
}

type EventType<Payloads extends object> = Extract<keyof Payloads, string>;

type DraftMember<Type extends string, Payload> = {
  type: Type;
  payload: Payload;
  capturedAt?: string;
};

type DraftFor<Payloads extends object> = {
  [Type in EventType<Payloads>]: DraftMember<Type, Payloads[Type]>;
}[EventType<Payloads>];

export type EventDraft<Payloads extends object = object> =
  [EventType<Payloads>] extends [never]
    ? DraftMember<string, unknown>
    : DraftFor<Payloads>;

type StoredFields = {
  v: typeof EVENT_SCHEMA_VERSION;
  runId: string;
  seq: number;
  ts: string;
};

type EventMember<Type extends string, Payload> = StoredFields &
  DraftMember<Type, Payload>;

type EventFor<Payloads extends object> = {
  [Type in EventType<Payloads>]: EventMember<Type, Payloads[Type]>;
}[EventType<Payloads>];

export type Event<Payloads extends object = object> =
  [EventType<Payloads>] extends [never]
    ? EventMember<string, unknown>
    : EventFor<Payloads>;

export interface StampFields {
  runId: string;
  seq: number;
  ts: string;
}

type Stamped<Draft> = Draft extends DraftMember<infer Type, infer Payload>
  ? EventMember<Type, Payload>
  : never;

export function stamp<Draft extends DraftMember<string, unknown>>(
  draft: Draft,
  fields: StampFields,
): Stamped<Draft> {
  return {
    v: EVENT_SCHEMA_VERSION,
    type: draft.type,
    runId: fields.runId,
    seq: fields.seq,
    ts: fields.ts,
    payload: draft.payload,
    ...(draft.capturedAt === undefined
      ? {}
      : { capturedAt: draft.capturedAt }),
  } as Stamped<Draft>;
}

const REQUIRED_EVENT_FIELDS = [
  "v",
  "type",
  "runId",
  "seq",
  "ts",
  "payload",
] as const;

const EVENT_FIELDS = new Set<string>([
  ...REQUIRED_EVENT_FIELDS,
  "capturedAt",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const RUN_KIND_VALUES = new Set<string>(RUN_KINDS);
const RUN_OUTCOME_VALUES = new Set<string>(RUN_OUTCOMES);
const CONTROL_KIND_VALUES = new Set<string>(CONTROL_KINDS);
const CONTROL_APPLIED_REASON_VALUES = new Set<string>(CONTROL_APPLIED_REASONS);
const CAPTURE_REFUSAL_CAUSE_VALUES = new Set<string>(CAPTURE_REFUSAL_CAUSES);
const DECISION_KIND_VALUES = new Set<string>(DECISION_KINDS);
const KNOWN_TYPE_VALUES = new Set<string>(KNOWN_TYPES);

const RUN_STARTED_FIELDS = new Set<string>([
  "kind",
  "actor",
  "harness",
  "model",
  "parentRunId",
  "parentToolUseId",
  "schedule",
  "repository",
  "workOrder",
  "ceilings",
]);

const RUN_CEILING_FIELDS = new Set<string>([
  "costUsd",
  "tokens",
  "wallMs",
  "idleMs",
  "turns",
]);

const RUN_FINISHED_FIELDS = new Set<string>([
  "outcome",
  "reason",
  "truncated",
  "costUsd",
  "usage",
  "durationMs",
  "estimated",
]);

const RUN_USAGE_FIELDS = new Set<string>([
  "inputTokens",
  "outputTokens",
  "cacheReadTokens",
  "cacheCreationTokens",
  "unit",
]);

const AGENT_STARTED_FIELDS = new Set<string>([
  "stage",
  "model",
  "sessionId",
  "pid",
]);

const AGENT_TEXT_FIELDS = new Set<string>([
  "stage",
  "text",
  "truncated",
  "parentToolUseId",
]);

const AGENT_TOOL_USE_FIELDS = new Set<string>([
  "stage",
  "tool",
  "inputExcerpt",
  "truncated",
  "toolUseId",
  "parentToolUseId",
]);

const AGENT_TOOL_RESULT_FIELDS = new Set<string>([
  "stage",
  "tool",
  "isError",
  "resultExcerpt",
  "truncated",
  "toolUseId",
  "parentToolUseId",
]);

const AGENT_COMPLETED_FIELDS = new Set<string>([
  "stage",
  "turns",
  "sessionId",
  "costUsd",
  "model",
  "usage",
  "durationMs",
  "estimated",
]);

const AGENT_WARNING_FIELDS = new Set<string>(["stage", "message"]);

const CONTROL_REQUESTED_FIELDS = new Set<string>([
  "controlId",
  "kind",
  "decisionId",
  "optionId",
  "text",
  "truncated",
  "by",
]);

const CONTROL_APPLIED_FIELDS = new Set<string>([
  "controlId",
  "ok",
  "by",
  "reason",
  "truncated",
  "landedIn",
]);

const CAPTURE_REFUSED_FIELDS = new Set<string>([
  "cause",
  "sourceRunId",
  "sourceSeq",
  "sourceType",
  "field",
  "count",
  "max",
  "detail",
  "truncated",
]);

const DECISION_DOSSIER_FIELDS = new Set<string>([
  "question",
  "optionsRuledOut",
  "recommendedAction",
  "blastRadius",
  "truncated",
]);

const DECISION_OPTION_FIELDS = new Set<string>(["id", "label"]);

const DECISION_REQUESTED_FIELDS = new Set<string>([
  "decisionId",
  "kind",
  "dossier",
  "options",
  "subject",
  "expiresAt",
  "onTimeout",
]);

const DECISION_ANSWERED_FIELDS = new Set<string>([
  "decisionId",
  "optionId",
  "by",
  "byTimeout",
  "reversal",
  "requestedRunId",
]);

/**
 * Returns the entries of `value` whose keys are not in `fields`, to be
 * carried forward as an opaque extension rather than rejected or dropped
 * (issue onsager-ai/ethogram#12): a sink that forwards an event it does not fully understand
 * must be byte-preserving, or the stream loses data silently at exactly the
 * boundary this protocol exists to cross. The **envelope** and required
 * fields stay strict; an unknown payload field or union member is tolerated
 * at read and retained on forward, with union membership checked by
 * `validate` instead.
 */
function extractUnknownFields(
  value: Record<string, unknown>,
  fields: ReadonlySet<string>,
): Record<string, unknown> {
  const unknown: Record<string, unknown> = {};
  for (const [key, fieldValue] of Object.entries(value)) {
    if (!fields.has(key)) {
      unknown[key] = fieldValue;
    }
  }
  return unknown;
}

function requiredString(
  value: Record<string, unknown>,
  field: string,
  name: string,
): string {
  if (!Object.hasOwn(value, field)) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name} is missing required field: ${field}`,
      true,
    );
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "string") {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a string`,
    );
  }
  return fieldValue;
}

function optionalString(
  value: Record<string, unknown>,
  field: string,
  name: string,
): string | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "string") {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a string when present`,
    );
  }
  return fieldValue;
}

function optionalNumber(
  value: Record<string, unknown>,
  field: string,
  name: string,
): number | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "number" || !Number.isFinite(fieldValue)) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a finite number when present`,
    );
  }
  return fieldValue;
}

/**
 * Parses an optional count field that must be a whole, non-negative number
 * — integer-valued `ceilings`, `usage`, and agent fields, all of which are
 * counts and can never be fractional or negative. `Number.isSafeInteger`
 * rejects a non-integer (`10.5`) and a value outside the ±2^53−1 magnitude
 * this protocol's numbers are bounded to (issue onsager-ai/ethogram#9) in one check; the sign
 * check on top of that rejects a negative count. Unlike `optionalNumber`,
 * this never coerces: an out-of-range value is an error, not a rounded or
 * clamped one.
 */
function optionalSafeInteger(
  value: Record<string, unknown>,
  field: string,
  name: string,
): number | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (
    typeof fieldValue !== "number" ||
    !Number.isSafeInteger(fieldValue) ||
    fieldValue < 0
  ) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a non-negative safe integer when present`,
    );
  }
  return fieldValue;
}

/**
 * Parses a required count field that must be a whole, non-negative number —
 * the same rule as `optionalSafeInteger`, but for a field the payload cannot
 * omit. Every count of milliseconds is a `u64` on the Rust side (ruling on
 * issue onsager-ai/ethogram#6): `RunFinishedPayload.durationMs` is the only field on this typed
 * path that is both a count and required, so this is where that rule is
 * enforced.
 */
function requiredSafeInteger(
  value: Record<string, unknown>,
  field: string,
  name: string,
): number {
  if (!Object.hasOwn(value, field)) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name} is missing required field: ${field}`,
      true,
    );
  }
  const fieldValue = value[field];
  if (
    typeof fieldValue !== "number" ||
    !Number.isSafeInteger(fieldValue) ||
    fieldValue < 0
  ) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a non-negative safe integer`,
    );
  }
  return fieldValue;
}

function optionalBoolean(
  value: Record<string, unknown>,
  field: string,
  name: string,
): boolean | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "boolean") {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a boolean when present`,
    );
  }
  return fieldValue;
}

function requiredBoolean(
  value: Record<string, unknown>,
  field: string,
  name: string,
): boolean {
  if (!Object.hasOwn(value, field)) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name} is missing required field: ${field}`,
      true,
    );
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "boolean") {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be a boolean`,
    );
  }
  return fieldValue;
}

function requiredArray(
  value: Record<string, unknown>,
  field: string,
  name: string,
): unknown[] {
  if (!Object.hasOwn(value, field)) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name} is missing required field: ${field}`,
      true,
    );
  }
  const fieldValue = value[field];
  if (!Array.isArray(fieldValue)) {
    throw new PayloadRepresentationError(
      `${name}.${field}`,
      `${name}.${field} must be an array`,
    );
  }
  return fieldValue;
}

function parseRunCeilings(value: unknown): RunCeilings {
  const name = "RunStartedPayload.ceilings";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const costUsd = optionalNumber(value, "costUsd", name);
  const tokens = optionalSafeInteger(value, "tokens", name);
  const wallMs = optionalSafeInteger(value, "wallMs", name);
  const idleMs = optionalSafeInteger(value, "idleMs", name);
  const turns = optionalSafeInteger(value, "turns", name);
  return {
    ...(costUsd === undefined ? {} : { costUsd }),
    ...(tokens === undefined ? {} : { tokens }),
    ...(wallMs === undefined ? {} : { wallMs }),
    ...(idleMs === undefined ? {} : { idleMs }),
    ...(turns === undefined ? {} : { turns }),
    ...extractUnknownFields(value, RUN_CEILING_FIELDS),
  };
}

function parseRunUsage(value: unknown, name: string): RunUsage {
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const inputTokens = optionalSafeInteger(value, "inputTokens", name);
  const outputTokens = optionalSafeInteger(value, "outputTokens", name);
  const cacheReadTokens = optionalSafeInteger(value, "cacheReadTokens", name);
  const cacheCreationTokens = optionalSafeInteger(
    value,
    "cacheCreationTokens",
    name,
  );
  const unit = optionalString(value, "unit", name);
  return {
    ...(inputTokens === undefined ? {} : { inputTokens }),
    ...(outputTokens === undefined ? {} : { outputTokens }),
    ...(cacheReadTokens === undefined ? {} : { cacheReadTokens }),
    ...(cacheCreationTokens === undefined ? {} : { cacheCreationTokens }),
    ...(unit === undefined ? {} : { unit }),
    ...extractUnknownFields(value, RUN_USAGE_FIELDS),
  };
}

/**
 * Parse a representable `run.started` payload. Unknown fields and unfamiliar
 * `kind` strings are tolerated and retained (issue onsager-ai/ethogram#12), while required
 * fields stay strict.
 */
export function parseRunStartedPayload(value: unknown): RunStartedPayload {
  const name = "RunStartedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const kind = requiredString(value, "kind", name);
  const actor = requiredString(value, "actor", name);
  const harness = requiredString(value, "harness", name);
  const model = optionalString(value, "model", name);
  const parentRunId = optionalString(value, "parentRunId", name);
  const parentToolUseId = optionalString(value, "parentToolUseId", name);
  const schedule = optionalString(value, "schedule", name);
  const repository = optionalString(value, "repository", name);
  const workOrder = optionalString(value, "workOrder", name);
  const ceilings = Object.hasOwn(value, "ceilings")
    ? parseRunCeilings(value.ceilings)
    : undefined;

  return {
    kind: kind as RunKind,
    actor,
    harness,
    ...(model === undefined ? {} : { model }),
    ...(parentRunId === undefined ? {} : { parentRunId }),
    ...(parentToolUseId === undefined ? {} : { parentToolUseId }),
    ...(schedule === undefined ? {} : { schedule }),
    ...(repository === undefined ? {} : { repository }),
    ...(workOrder === undefined ? {} : { workOrder }),
    ...(ceilings === undefined ? {} : { ceilings }),
    ...extractUnknownFields(value, RUN_STARTED_FIELDS),
  };
}

/**
 * Parse a representable `run.finished` payload. Unknown fields and unfamiliar
 * `outcome` strings are tolerated and retained (issue onsager-ai/ethogram#12), while required
 * fields stay strict.
 */
export function parseRunFinishedPayload(value: unknown): RunFinishedPayload {
  const name = "RunFinishedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const outcome = requiredString(value, "outcome", name);
  const reason = optionalString(value, "reason", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const costUsd = optionalNumber(value, "costUsd", name);
  const usage = Object.hasOwn(value, "usage")
    ? parseRunUsage(value.usage, `${name}.usage`)
    : undefined;
  const durationMs = requiredSafeInteger(value, "durationMs", name);
  const estimated = optionalBoolean(value, "estimated", name);

  return {
    outcome: outcome as RunOutcome,
    ...(reason === undefined ? {} : { reason }),
    ...(truncated === undefined ? {} : { truncated }),
    ...(costUsd === undefined ? {} : { costUsd }),
    ...(usage === undefined ? {} : { usage }),
    durationMs,
    ...(estimated === undefined ? {} : { estimated }),
    ...extractUnknownFields(value, RUN_FINISHED_FIELDS),
  };
}

/** Parse an `agent.started` payload, retaining unknown fields. */
export function parseAgentStartedPayload(value: unknown): AgentStartedPayload {
  const name = "AgentStartedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const model = optionalString(value, "model", name);
  const sessionId = optionalString(value, "sessionId", name);
  const pid = optionalSafeInteger(value, "pid", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    ...(model === undefined ? {} : { model }),
    ...(sessionId === undefined ? {} : { sessionId }),
    ...(pid === undefined ? {} : { pid }),
    ...extractUnknownFields(value, AGENT_STARTED_FIELDS),
  };
}

/** Parse an `agent.text` payload, retaining unknown fields. */
export function parseAgentTextPayload(value: unknown): AgentTextPayload {
  const name = "AgentTextPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const text = requiredString(value, "text", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const parentToolUseId = optionalString(value, "parentToolUseId", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    text,
    ...(truncated === undefined ? {} : { truncated }),
    ...(parentToolUseId === undefined ? {} : { parentToolUseId }),
    ...extractUnknownFields(value, AGENT_TEXT_FIELDS),
  };
}

/** Parse an `agent.tool_use` payload, retaining unknown fields. */
export function parseAgentToolUsePayload(value: unknown): AgentToolUsePayload {
  const name = "AgentToolUsePayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const tool = requiredString(value, "tool", name);
  const inputExcerpt = optionalString(value, "inputExcerpt", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const toolUseId = optionalString(value, "toolUseId", name);
  const parentToolUseId = optionalString(value, "parentToolUseId", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    tool,
    ...(inputExcerpt === undefined ? {} : { inputExcerpt }),
    ...(truncated === undefined ? {} : { truncated }),
    ...(toolUseId === undefined ? {} : { toolUseId }),
    ...(parentToolUseId === undefined ? {} : { parentToolUseId }),
    ...extractUnknownFields(value, AGENT_TOOL_USE_FIELDS),
  };
}

/**
 * Parse an `agent.tool_result` payload, retaining unknown fields.
 */
export function parseAgentToolResultPayload(
  value: unknown,
): AgentToolResultPayload {
  const name = "AgentToolResultPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const tool = requiredString(value, "tool", name);
  const isError = optionalBoolean(value, "isError", name);
  const resultExcerpt = optionalString(value, "resultExcerpt", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const toolUseId = optionalString(value, "toolUseId", name);
  const parentToolUseId = optionalString(value, "parentToolUseId", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    tool,
    ...(isError === undefined ? {} : { isError }),
    ...(resultExcerpt === undefined ? {} : { resultExcerpt }),
    ...(truncated === undefined ? {} : { truncated }),
    ...(toolUseId === undefined ? {} : { toolUseId }),
    ...(parentToolUseId === undefined ? {} : { parentToolUseId }),
    ...extractUnknownFields(value, AGENT_TOOL_RESULT_FIELDS),
  };
}

/** Parse an `agent.completed` payload, retaining unknown fields. */
export function parseAgentCompletedPayload(
  value: unknown,
): AgentCompletedPayload {
  const name = "AgentCompletedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const turns = optionalSafeInteger(value, "turns", name);
  const sessionId = optionalString(value, "sessionId", name);
  const costUsd = optionalNumber(value, "costUsd", name);
  const model = optionalString(value, "model", name);
  const usage = Object.hasOwn(value, "usage")
    ? parseRunUsage(value.usage, `${name}.usage`)
    : undefined;
  const durationMs = optionalSafeInteger(value, "durationMs", name);
  const estimated = optionalBoolean(value, "estimated", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    ...(turns === undefined ? {} : { turns }),
    ...(sessionId === undefined ? {} : { sessionId }),
    ...(costUsd === undefined ? {} : { costUsd }),
    ...(model === undefined ? {} : { model }),
    ...(usage === undefined ? {} : { usage }),
    ...(durationMs === undefined ? {} : { durationMs }),
    ...(estimated === undefined ? {} : { estimated }),
    ...extractUnknownFields(value, AGENT_COMPLETED_FIELDS),
  };
}

/** Parse an `agent.warning` payload, retaining unknown fields. */
export function parseAgentWarningPayload(value: unknown): AgentWarningPayload {
  const name = "AgentWarningPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const message = requiredString(value, "message", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    message,
    ...extractUnknownFields(value, AGENT_WARNING_FIELDS),
  };
}

/** Parse a `control.requested` payload, retaining unknown fields. */
export function parseControlRequestedPayload(
  value: unknown,
): ControlRequestedPayload {
  const name = "ControlRequestedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const controlId = requiredString(value, "controlId", name);
  const kind = requiredString(value, "kind", name);
  const decisionId = optionalString(value, "decisionId", name);
  const optionId = optionalString(value, "optionId", name);
  const text = optionalString(value, "text", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const by = requiredString(value, "by", name);
  return {
    controlId,
    kind: kind as ControlKind,
    ...(decisionId === undefined ? {} : { decisionId }),
    ...(optionId === undefined ? {} : { optionId }),
    ...(text === undefined ? {} : { text }),
    ...(truncated === undefined ? {} : { truncated }),
    by,
    ...extractUnknownFields(value, CONTROL_REQUESTED_FIELDS),
  };
}

/** Parse a `control.applied` payload, retaining unknown fields. */
export function parseControlAppliedPayload(
  value: unknown,
): ControlAppliedPayload {
  const name = "ControlAppliedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const controlId = requiredString(value, "controlId", name);
  const ok = requiredBoolean(value, "ok", name);
  const by = optionalString(value, "by", name);
  const reason = optionalString(value, "reason", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const landedIn = optionalString(value, "landedIn", name);
  return {
    controlId,
    ok,
    ...(by === undefined ? {} : { by }),
    ...(reason === undefined ? {} : { reason }),
    ...(truncated === undefined ? {} : { truncated }),
    ...(landedIn === undefined ? {} : { landedIn }),
    ...extractUnknownFields(value, CONTROL_APPLIED_FIELDS),
  };
}

/**
 * Parse a representable `capture.refused` payload. Unknown fields and
 * unfamiliar `cause` strings are tolerated and retained, while required
 * fields stay strict.
 */
export function parseCaptureRefusedPayload(
  value: unknown,
): CaptureRefusedPayload {
  const name = "CaptureRefusedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const cause = requiredString(value, "cause", name);
  const sourceRunId = requiredString(value, "sourceRunId", name);
  const sourceSeq = optionalSafeInteger(value, "sourceSeq", name);
  const sourceType = optionalString(value, "sourceType", name);
  const field = optionalString(value, "field", name);
  const count = optionalSafeInteger(value, "count", name);
  const max = optionalSafeInteger(value, "max", name);
  const detail = optionalString(value, "detail", name);
  const truncated = optionalBoolean(value, "truncated", name);
  return {
    cause: cause as CaptureRefusalCause,
    sourceRunId,
    ...(sourceSeq === undefined ? {} : { sourceSeq }),
    ...(sourceType === undefined ? {} : { sourceType }),
    ...(field === undefined ? {} : { field }),
    ...(count === undefined ? {} : { count }),
    ...(max === undefined ? {} : { max }),
    ...(detail === undefined ? {} : { detail }),
    ...(truncated === undefined ? {} : { truncated }),
    ...extractUnknownFields(value, CAPTURE_REFUSED_FIELDS),
  };
}

function parseDecisionDossier(value: unknown): DecisionDossier {
  const name = "DecisionRequestedPayload.dossier";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const question = requiredString(value, "question", name);
  const optionsRuledOut = requiredArray(
    value,
    "optionsRuledOut",
    name,
  ).map((option, index) => {
    if (typeof option !== "string") {
      throw new PayloadRepresentationError(
        `${name}.optionsRuledOut[${index}]`,
        `${name}.optionsRuledOut[${index}] must be a string`,
      );
    }
    return option;
  });
  const recommendedAction = requiredString(value, "recommendedAction", name);
  const blastRadius = requiredString(value, "blastRadius", name);
  const truncated = optionalBoolean(value, "truncated", name);
  return {
    question,
    optionsRuledOut,
    recommendedAction,
    blastRadius,
    ...(truncated === undefined ? {} : { truncated }),
    ...extractUnknownFields(value, DECISION_DOSSIER_FIELDS),
  };
}

function parseDecisionOption(value: unknown, index: number): DecisionOption {
  const name = `DecisionRequestedPayload.options[${index}]`;
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const id = requiredString(value, "id", name);
  const label = requiredString(value, "label", name);
  return {
    id,
    label,
    ...extractUnknownFields(value, DECISION_OPTION_FIELDS),
  };
}

/**
 * Parse a representable `decision.requested` payload. Unknown fields and
 * unfamiliar `kind` strings are retained. Capture bounds and the `onTimeout`
 * policy are deliberately left to `validate`, so a forwarder can carry a
 * representable request even when its producer should not have emitted it.
 */
export function parseDecisionRequestedPayload(
  value: unknown,
): DecisionRequestedPayload {
  const name = "DecisionRequestedPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const decisionId = requiredString(value, "decisionId", name);
  const kind = requiredString(value, "kind", name);
  if (!Object.hasOwn(value, "dossier")) {
    throw new PayloadRepresentationError(
      `${name}.dossier`,
      `${name} is missing required field: dossier`,
      true,
    );
  }
  const dossier = parseDecisionDossier(value.dossier);
  const options = requiredArray(value, "options", name).map(parseDecisionOption);
  const subject = optionalString(value, "subject", name);
  const expiresAt = optionalString(value, "expiresAt", name);
  const onTimeout = optionalString(value, "onTimeout", name);
  return {
    decisionId,
    kind: kind as DecisionKind,
    dossier,
    options,
    ...(subject === undefined ? {} : { subject }),
    ...(expiresAt === undefined ? {} : { expiresAt }),
    ...(onTimeout === undefined ? {} : { onTimeout }),
    ...extractUnknownFields(value, DECISION_REQUESTED_FIELDS),
  };
}

/** Parse a representable `decision.answered` payload, retaining extras. */
export function parseDecisionAnsweredPayload(
  value: unknown,
): DecisionAnsweredPayload {
  const name = "DecisionAnsweredPayload";
  if (!isRecord(value)) {
    throw new PayloadRepresentationError(name, `${name} must be an object`);
  }

  const decisionId = requiredString(value, "decisionId", name);
  const optionId = requiredString(value, "optionId", name);
  const by = requiredString(value, "by", name);
  const byTimeout = optionalBoolean(value, "byTimeout", name);
  const reversal = optionalString(value, "reversal", name);
  const requestedRunId = optionalString(value, "requestedRunId", name);
  return {
    decisionId,
    optionId,
    by,
    ...(byTimeout === undefined ? {} : { byTimeout }),
    ...(reversal === undefined ? {} : { reversal }),
    ...(requestedRunId === undefined ? {} : { requestedRunId }),
    ...extractUnknownFields(value, DECISION_ANSWERED_FIELDS),
  };
}

function parseKnownPayload(eventType: string, payload: unknown): unknown {
  switch (eventType) {
    case RUN_STARTED:
      return parseRunStartedPayload(payload);
    case RUN_FINISHED:
      return parseRunFinishedPayload(payload);
    case AGENT_STARTED:
      return parseAgentStartedPayload(payload);
    case AGENT_TEXT:
      return parseAgentTextPayload(payload);
    case AGENT_TOOL_USE:
      return parseAgentToolUsePayload(payload);
    case AGENT_TOOL_RESULT:
      return parseAgentToolResultPayload(payload);
    case AGENT_COMPLETED:
      return parseAgentCompletedPayload(payload);
    case AGENT_WARNING:
      return parseAgentWarningPayload(payload);
    case CONTROL_REQUESTED:
      return parseControlRequestedPayload(payload);
    case CONTROL_APPLIED:
      return parseControlAppliedPayload(payload);
    case CAPTURE_REFUSED:
      return parseCaptureRefusedPayload(payload);
    case DECISION_REQUESTED:
      return parseDecisionRequestedPayload(payload);
    case DECISION_ANSWERED:
      return parseDecisionAnsweredPayload(payload);
    default:
      return payload;
  }
}

/**
 * The largest magnitude at which an integral number round-trips exactly
 * between this SDK and the Rust SDK (2^53 − 1). Shared by `Event.seq`
 * validation and payload-number validation (issue onsager-ai/ethogram#9): both reject an
 * out-of-range integral value at parse time rather than rounding it.
 */
const MAX_SAFE_INTEGER_MAGNITUDE = Number.MAX_SAFE_INTEGER;

/**
 * Recursively validates that every integral-valued number in `value` is
 * within the safe-integer magnitude bound, naming the offending path (for
 * example `payload.nested.count` or `payload.items[2].total`) when the check
 * fails. Non-integral numbers are never bounded, no matter how large their
 * magnitude. Mirrors the `Event.seq` check above and reuses the same bound
 * (issue onsager-ai/ethogram#9): a value that needs more precision must be carried as a string
 * instead of a number.
 *
 * `JSON.parse` has already collapsed any literal too large to represent
 * exactly before this function ever sees it, so only magnitude can be
 * tested here; that is sufficient, because the bound is on magnitude.
 */
function validatePayloadNumbers(value: unknown, path: string): void {
  if (typeof value === "number") {
    if (Number.isInteger(value) && Math.abs(value) > MAX_SAFE_INTEGER_MAGNITUDE) {
      throw new PayloadRepresentationError(
        path,
        `${path} is an integral number whose magnitude exceeds the safe integer bound: actual ${String(value)}; maximum ${MAX_SAFE_INTEGER_MAGNITUDE}; a value that needs more precision must be carried as a string`,
      );
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      validatePayloadNumbers(item, `${path}[${index}]`);
    }
    return;
  }
  if (isRecord(value)) {
    for (const [key, child] of Object.entries(value)) {
      validatePayloadNumbers(child, `${path}.${key}`);
    }
  }
}

function validateScalarBound(
  value: string | undefined,
  field: string,
  maximum: number,
): void {
  if (value === undefined) {
    return;
  }
  const actual = Array.from(value).length;
  if (actual > maximum) {
    throw new ValidationError(
      { kind: "OverBound", path: payloadPath(field), count: actual, max: maximum },
      `${field} has ${actual} Unicode scalar values; maximum is ${maximum}`,
    );
  }
}

/**
 * Recursively validates that every string leaf in `value` is at most
 * `MAX_TEXT_SCALARS` Unicode scalar values (issue onsager-ai/ethogram#28), naming the offending
 * path (for example `payload.nested.note` or `payload.items[2].note`) when
 * the check fails. Mirrors `validatePayloadNumbers` exactly, walking nested
 * objects at any depth, strings inside arrays, and strings inside objects
 * nested inside arrays — and, because it walks the raw decoded JSON value
 * rather than a parsed typed payload, a known type's own retained unknown
 * fields are covered by the same walk rather than needing a separate pass.
 *
 * `validate` calls this unconditionally, before switching on whether
 * `eventType` is recognised, so an unrecognised type is covered by the same
 * floor as a known one instead of going unchecked.
 */
function validatePayloadTextScalars(value: unknown, path: string): void {
  if (typeof value === "string") {
    const actual = Array.from(value).length;
    if (actual > MAX_TEXT_SCALARS) {
      throw new ValidationError(
        { kind: "OverBound", path, count: actual, max: MAX_TEXT_SCALARS },
        `${path} has ${actual} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      );
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      validatePayloadTextScalars(item, `${path}[${index}]`);
    }
    return;
  }
  if (isRecord(value)) {
    for (const [key, child] of Object.entries(value)) {
      validatePayloadTextScalars(child, `${path}.${key}`);
    }
  }
}

/**
 * Serialises `payload` alone — never wrapped in an envelope — into the same
 * canonical compact form `serialiseEvent` would produce for it: keys sorted
 * recursively by UTF-8 byte order via `sortObjectKeysByUtf8Bytes`, then
 * `JSON.stringify`, which already lays out numbers in the ECMAScript
 * notation this protocol's canonical form uses. Shared by
 * `validatePayloadSize` so the bytes it measures match what a producer would
 * actually put on the wire for this payload.
 */
function serialisePayloadCanonical(payload: unknown): string {
  return JSON.stringify(sortObjectKeysByUtf8Bytes(payload));
}

/**
 * Validates that `payload` alone — not the envelope around it — serialises
 * to at most `MAX_PAYLOAD_BYTES` **UTF-8** bytes in its canonical compact
 * form (issue onsager-ai/ethogram#28). See `MAX_PAYLOAD_BYTES`'s own doc comment for why this
 * bound and `MAX_TEXT_SCALARS` do not collide.
 *
 * `validate` calls this unconditionally, before switching on whether
 * `eventType` is recognised, so an unrecognised type is covered by the same
 * floor as a known one instead of going unchecked.
 */
function validatePayloadSize(payload: unknown): void {
  const serialised = serialisePayloadCanonical(payload);
  const actual = Buffer.byteLength(serialised, "utf8");
  if (actual > MAX_PAYLOAD_BYTES) {
    throw new ValidationError(
      { kind: "PayloadTooLarge", bytes: actual, max: MAX_PAYLOAD_BYTES },
      `payload has ${actual} bytes; maximum is ${MAX_PAYLOAD_BYTES}`,
    );
  }
}

/**
 * Validate whether a producer should emit `payload` for `eventType`.
 *
 * Two universal bounds (issue onsager-ai/ethogram#28) are checked first, for **every** event
 * regardless of whether `eventType` is recognised: every string leaf
 * anywhere in the payload — nested objects at any depth, strings inside
 * arrays, strings inside objects nested inside arrays, and a known type's
 * own retained unknown fields alike — is at most `MAX_TEXT_SCALARS` Unicode
 * scalar values, and the payload's canonical compact serialisation is at
 * most `MAX_PAYLOAD_BYTES` bytes. Without these, a producer emitting an
 * unrecognised type carrying an unbounded payload validated cleanly, because
 * an unrecognised type otherwise has no bound applied to it at all — this is
 * the hole closed here.
 *
 * Required fields, known closed-union membership, safe-integer bounds, and
 * the tighter per-field capture bounds are enforced only for known event
 * types; an unrecognised type is checked against the two universal bounds
 * above and nothing else.
 *
 * `parse_event` answers "can both SDKs carry this?"; `validate` answers
 * "should a producer have emitted this?" `parseEvent` therefore does not call
 * this function, nor either universal bound directly: a representable
 * over-bound event must remain forwardable.
 */
export function validate(eventType: string, payload: unknown): void {
  try {
    validatePayload(eventType, payload);
  } catch (error) {
    if (error instanceof ValidationError) {
      throw error;
    }
    if (error instanceof PayloadRepresentationError) {
      // A missing field is its own kind; anything else here is a value that
      // could not be represented at all (wrong-typed, out of range) rather
      // than a stated rule broken by an otherwise representable one.
      throw error.missing
        ? new ValidationError(
            { kind: "MissingField", path: error.path },
            error.message,
          )
        : malformedError(error.path, error.message);
    }
    // A non-JSON input can also fail in the canonical serialiser itself —
    // also a representation failure, not a stated rule.
    if (error instanceof Error) {
      throw malformedError("payload", error.message);
    }
    throw error;
  }
}

function validatePayload(eventType: string, payload: unknown): void {
  // Universal bounds: run before the switch below, and for every event
  // including one of an unrecognised type (issue onsager-ai/ethogram#28).
  validatePayloadTextScalars(payload, "payload");
  validatePayloadSize(payload);

  if (!KNOWN_TYPE_VALUES.has(eventType)) {
    return;
  }

  validatePayloadNumbers(payload, "payload");
  const parsed = parseKnownPayload(eventType, payload);

  switch (eventType) {
    case RUN_STARTED: {
      const started = parsed as RunStartedPayload;
      if (!RUN_KIND_VALUES.has(started.kind)) {
        throw new ValidationError(
          { kind: "UnknownMember", path: "payload.kind", value: started.kind },
          `RunStartedPayload.kind has unknown value: ${started.kind}`,
        );
      }
      return;
    }
    case RUN_FINISHED: {
      const finished = parsed as RunFinishedPayload;
      if (!RUN_OUTCOME_VALUES.has(finished.outcome)) {
        throw new ValidationError(
          { kind: "UnknownMember", path: "payload.outcome", value: finished.outcome },
          `RunFinishedPayload.outcome has unknown value: ${finished.outcome}`,
        );
      }
      validateScalarBound(
        finished.reason,
        "RunFinishedPayload.reason",
        MAX_EXCERPT_SCALARS,
      );
      return;
    }
    case AGENT_TEXT: {
      const text = parsed as AgentTextPayload;
      validateScalarBound(
        text.text,
        "AgentTextPayload.text",
        MAX_TEXT_SCALARS,
      );
      return;
    }
    case AGENT_TOOL_USE: {
      const toolUse = parsed as AgentToolUsePayload;
      validateScalarBound(
        toolUse.inputExcerpt,
        "AgentToolUsePayload.inputExcerpt",
        MAX_EXCERPT_SCALARS,
      );
      return;
    }
    case AGENT_TOOL_RESULT: {
      const toolResult = parsed as AgentToolResultPayload;
      validateScalarBound(
        toolResult.resultExcerpt,
        "AgentToolResultPayload.resultExcerpt",
        MAX_EXCERPT_SCALARS,
      );
      return;
    }
    case AGENT_WARNING: {
      const warning = parsed as AgentWarningPayload;
      validateScalarBound(
        warning.message,
        "AgentWarningPayload.message",
        MAX_EXCERPT_SCALARS,
      );
      return;
    }
    case CONTROL_REQUESTED: {
      const requested = parsed as ControlRequestedPayload;
      // Checked before any kind-conditioned business rule below, exactly as
      // every other closed union checks membership before its own
      // conditioned rules: those rules (decisionId/optionId only for
      // "answer", text required for "steer") only have anything to say about
      // a kind this SDK recognises.
      if (!CONTROL_KIND_VALUES.has(requested.kind)) {
        throw new ValidationError(
          { kind: "UnknownMember", path: "payload.kind", value: requested.kind },
          `ControlRequestedPayload.kind has unknown value: ${requested.kind}`,
        );
      }
      for (const field of ["decisionId", "optionId"] as const) {
        if (requested.kind === "answer") {
          if (requested[field] === undefined) {
            throw new ValidationError(
              { kind: "MissingField", path: `payload.${field}` },
              `ControlRequestedPayload.${field} is required when kind is "answer"`,
            );
          }
        } else if (requested[field] !== undefined) {
          throw policyError(
            `payload.${field}`,
            `ControlRequestedPayload.${field} is permitted only when kind is "answer"`,
          );
        }
      }
      if (requested.kind === "answer" && requested.text !== undefined) {
        throw policyError(
          "payload.text",
          'ControlRequestedPayload.text must be absent when kind is "answer"',
        );
      }
      // A `steer` is an instruction queued for the run's next turn; one
      // carrying nothing to say is a producer error. This is policy, not
      // representability, so it lives here and not in `parseEvent` (see its
      // doc comment): a steer with no text is perfectly representable, and a
      // forwarder must still be able to relay it. An absent `text` and a
      // present-but-empty one are the same defect, so both are rejected
      // identically.
      if (requested.kind === "steer" && !requested.text) {
        throw policyError(
          "payload.text",
          'ControlRequestedPayload.text is required and must not be empty when kind is "steer": a steer with nothing to say is a producer error',
        );
      }
      validateScalarBound(
        requested.text,
        "ControlRequestedPayload.text",
        MAX_EXCERPT_SCALARS,
      );
      return;
    }
    case CONTROL_APPLIED: {
      const applied = parsed as ControlAppliedPayload;
      if (!applied.ok && applied.reason === undefined) {
        throw new ValidationError(
          { kind: "MissingField", path: "payload.reason" },
          "ControlAppliedPayload.reason is required when ok is false",
        );
      }
      if (
        applied.reason !== undefined &&
        !CONTROL_APPLIED_REASON_VALUES.has(applied.reason)
      ) {
        validateScalarBound(
          applied.reason,
          "ControlAppliedPayload.reason",
          MAX_EXCERPT_SCALARS,
        );
      }
      return;
    }
    case CAPTURE_REFUSED: {
      const refused = parsed as CaptureRefusedPayload;
      if (!CAPTURE_REFUSAL_CAUSE_VALUES.has(refused.cause)) {
        throw new ValidationError(
          { kind: "UnknownMember", path: "payload.cause", value: refused.cause },
          `CaptureRefusedPayload.cause has unknown value: ${refused.cause}`,
        );
      }
      validateScalarBound(
        refused.detail,
        "CaptureRefusedPayload.detail",
        MAX_EXCERPT_SCALARS,
      );
      return;
    }
    case DECISION_REQUESTED: {
      const requested = parsed as DecisionRequestedPayload;
      if (!DECISION_KIND_VALUES.has(requested.kind)) {
        throw new ValidationError(
          { kind: "UnknownMember", path: "payload.kind", value: requested.kind },
          `DecisionRequestedPayload.kind has unknown value: ${requested.kind}`,
        );
      }
      if (requested.onTimeout !== undefined) {
        if (requested.kind !== "permission") {
          throw policyError(
            "payload.onTimeout",
            `DecisionRequestedPayload.onTimeout is permitted only when kind is "permission"; received kind "${requested.kind}"`,
          );
        }
        if (requested.onTimeout !== "deny") {
          throw policyError(
            "payload.onTimeout",
            `DecisionRequestedPayload.onTimeout must be "deny" when kind is "permission"; received "${requested.onTimeout}"`,
          );
        }
        if (
          !requested.options.some((option) => option.id === requested.onTimeout)
        ) {
          throw policyError(
            "payload.onTimeout",
            `DecisionRequestedPayload.onTimeout must name one of the request's options[].id; received "${requested.onTimeout}"`,
          );
        }
      }
      validateScalarBound(
        requested.dossier.question,
        "DecisionRequestedPayload.dossier.question",
        MAX_EXCERPT_SCALARS,
      );
      for (const [index, ruledOut] of requested.dossier.optionsRuledOut.entries()) {
        validateScalarBound(
          ruledOut,
          `DecisionRequestedPayload.dossier.optionsRuledOut[${index}]`,
          MAX_EXCERPT_SCALARS,
        );
      }
      validateScalarBound(
        requested.dossier.recommendedAction,
        "DecisionRequestedPayload.dossier.recommendedAction",
        MAX_EXCERPT_SCALARS,
      );
      validateScalarBound(
        requested.dossier.blastRadius,
        "DecisionRequestedPayload.dossier.blastRadius",
        MAX_EXCERPT_SCALARS,
      );
      for (const [index, option] of requested.options.entries()) {
        validateScalarBound(
          option.label,
          `DecisionRequestedPayload.options[${index}].label`,
          MAX_EXCERPT_SCALARS,
        );
      }
      return;
    }
    case DECISION_ANSWERED:
      return;
    default:
      // The remaining known types have no closed union or captured text.
  }
}

/**
 * Checks consistency visible only when a `decision.requested` and
 * `decision.answered` payload are available together. This is intentionally
 * separate from `validate` and `parseEvent`: the events are independent on
 * the wire, and a forwarder handling one has not necessarily seen the other.
 *
 * The chosen `optionId` must be present in the request's options. The sole
 * exception is the request's `onTimeout` value when `byTimeout` is true.
 *
 * This exception has not become dead weight now that `validate` requires
 * `onTimeout` to name an existing option: this function never calls
 * `validate`, so it has no way to know whether the `request` it was handed
 * ever passed that check. A request forwarded without validation, or
 * emitted by a producer written before the rule existed, can still reach
 * here with an `onTimeout` absent from its own `options` — the same shape
 * `parseEvent` deliberately still accepts. The exception is what lets a
 * genuine timeout answer against such a request validate correctly instead
 * of being misreported as an unrecognised option.
 *
 * The two `decisionId` values must match. `reversal`, when present, is
 * **not** checked here (ruled on onsager-ai/ethogram#7): it may name either an offered option
 * or a `<verb>:<subject>` action id the producer accepts as a later answer
 * to this same decision, and only the producer knows which action ids it
 * accepts — see `DecisionAnsweredPayload.reversal`'s doc comment for why
 * checking it against `options[].id` would refuse a legitimate undo.
 */
export function validateDecisionAnswerAgainstRequest(
  request: DecisionRequestedPayload,
  answer: DecisionAnsweredPayload,
): void {
  if (request.decisionId !== answer.decisionId) {
    throw new TypeError(
      `DecisionAnsweredPayload.decisionId does not match request: expected "${request.decisionId}"; received "${answer.decisionId}"`,
    );
  }

  const optionIds = new Set(request.options.map((option) => option.id));
  const optionExists = optionIds.has(answer.optionId);
  const isTimeoutOption =
    answer.byTimeout === true && request.onTimeout === answer.optionId;
  if (!optionExists && !isTimeoutOption) {
    throw new TypeError(
      `DecisionAnsweredPayload.optionId does not name a request option: "${answer.optionId}"`,
    );
  }
}

/**
 * Recursively sorts an object's own keys by UTF-8 byte order, leaving array
 * order untouched (though objects nested inside an array are themselves
 * sorted). Sorts by UTF-8 bytes via `Buffer.compare`, deliberately not by
 * default JavaScript string comparison: `<` on strings compares UTF-16 code
 * units, which diverges from Rust's byte-wise `String` ordering for
 * characters outside the Basic Multilingual Plane.
 */
function sortObjectKeysByUtf8Bytes(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortObjectKeysByUtf8Bytes);
  }
  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) =>
          Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8")),
        )
        .map(([key, child]) => [key, sortObjectKeysByUtf8Bytes(child)]),
    );
  }
  return value;
}

/**
 * Parse a decoded JSON value as an Event, rejecting envelope drift and values
 * either SDK cannot represent. Unknown event types deliberately retain the
 * open payload behaviour: both SDKs parse their recognised `run.*` and
 * `agent.*` vocabulary members here without turning the envelope parser into
 * a closed event-type registry.
 *
 * `parse_event` answers "can both SDKs carry this?"; `validate` answers
 * "should a producer have emitted this?" This function keeps required fields
 * and integer bounds strict, but it retains unfamiliar union members and does
 * not enforce capture bounds. It deliberately does not call `validate`, so a
 * forwarder can relay an over-bound event faithfully.
 *
 * A payload's *unknown fields* are a separate axis from its *unknown type*
 * and are tolerated rather than rejected (issue onsager-ai/ethogram#12): every recognised
 * payload parser carries a field it does not recognise forward into the
 * returned payload object rather than silently dropping it, so
 * `serialiseEvent` re-emits it. Only the envelope stays closed to unknown
 * fields, via the check just below.
 */
export function parseEvent(value: unknown): Event {
  if (!isRecord(value)) {
    throw new TypeError("Event must be a JSON object");
  }

  const unknownFields = Object.keys(value).filter(
    (field) => !EVENT_FIELDS.has(field),
  );
  if (unknownFields.length > 0) {
    throw new TypeError(
      `Event contains unknown field${unknownFields.length === 1 ? "" : "s"}: ${unknownFields.join(", ")}`,
    );
  }

  for (const field of REQUIRED_EVENT_FIELDS) {
    if (!Object.hasOwn(value, field)) {
      throw new TypeError(`Event is missing required field: ${field}`);
    }
  }

  if (value.v !== EVENT_SCHEMA_VERSION) {
    throw new TypeError(
      `Event.v must be ${EVENT_SCHEMA_VERSION}; received ${String(value.v)}`,
    );
  }
  if (typeof value.type !== "string") {
    throw new TypeError("Event.type must be a string");
  }
  if (typeof value.runId !== "string") {
    throw new TypeError("Event.runId must be a string");
  }
  if (!Number.isSafeInteger(value.seq) || (value.seq as number) < 1) {
    throw new TypeError("Event.seq must be a positive safe integer");
  }
  if (typeof value.ts !== "string") {
    throw new TypeError("Event.ts must be a string");
  }
  if (value.payload === undefined) {
    throw new TypeError("Event.payload must be a JSON value");
  }
  validatePayloadNumbers(value.payload, "payload");
  const payload = parseKnownPayload(value.type, value.payload);
  if (
    Object.hasOwn(value, "capturedAt") &&
    typeof value.capturedAt !== "string"
  ) {
    throw new TypeError("Event.capturedAt must be a string when present");
  }

  return {
    v: EVENT_SCHEMA_VERSION,
    type: value.type,
    runId: value.runId,
    seq: value.seq as number,
    ts: value.ts,
    payload,
    ...(typeof value.capturedAt === "string"
      ? { capturedAt: value.capturedAt }
      : {}),
  };
}

/**
 * Serialise an Event in its canonical compact form: no presentation
 * whitespace, envelope keys in declared order (`v`, `type`, `runId`, `seq`,
 * `ts`, `payload`, `capturedAt`), and payload object keys sorted recursively
 * by UTF-8 byte order (array order is left alone, but objects nested inside an
 * array are themselves sorted). Both SDKs commit to emitting exactly these
 * bytes for the same event, so the conformance harness diffs producer output
 * directly rather than normalising it first.
 *
 * The envelope is rebuilt field by field rather than spread from `event`,
 * because a spread would preserve whatever key order the caller happened to
 * construct — and an Event that reached this function from anywhere but
 * `parseEvent` or `stamp` carries no guarantee about that. Rust emits its
 * struct's declaration order unconditionally; this is how TypeScript matches
 * it unconditionally too.
 */
export function serialiseEvent(event: Event): string {
  return JSON.stringify({
    v: event.v,
    type: event.type,
    runId: event.runId,
    seq: event.seq,
    ts: event.ts,
    payload: sortObjectKeysByUtf8Bytes(event.payload),
    ...(event.capturedAt === undefined ? {} : { capturedAt: event.capturedAt }),
  });
}

export type Clock = () => string;

/**
 * Thrown by `InMemorySink.appendEvent` when an already-stamped event's `seq`
 * does not match the next value expected for its run.
 */
export class SequenceError extends Error {
  readonly runId: string;
  readonly expected: number;
  readonly received: number;

  constructor(runId: string, expected: number, received: number) {
    super(
      `Event sequence for run ${runId} must be ${expected}; received ${received}`,
    );
    this.name = "SequenceError";
    this.runId = runId;
    this.expected = expected;
    this.received = received;
  }
}

/**
 * Thrown by `InMemorySink.appendDraft` and `InMemorySink.appendEvent` once a
 * run has already recorded a terminal event (issues onsager-ai/ethogram#5 and onsager-ai/ethogram#3): a run has at
 * most one `run.finished`, and a sink that has recorded it refuses later
 * appends and forwards for that run — both the draft-appending path and the
 * already-stamped forwarding path, and regardless of the later event's own
 * type, so a `run.finished` followed by an `agent.text` is refused exactly
 * as a second `run.finished` would be.
 *
 * Deliberately a distinct class from `SequenceError` rather than a shared
 * shape distinguished only by message: a caller must be able to tell "you
 * skipped a seq" from "this run is closed" via `instanceof`, because the two
 * call for different responses.
 */
export class RunClosedError extends Error {
  readonly runId: string;

  constructor(runId: string) {
    super(
      `run ${runId} already recorded a terminal event; no further events are accepted for it`,
    );
    this.name = "RunClosedError";
    this.runId = runId;
  }
}

/**
 * A minimal in-memory reference for sequencing rules, not a storage engine.
 *
 * Tracks, per run, whether a terminal event (`type` equal to `RUN_FINISHED`)
 * has already been appended. Once it has, every further append for that run
 * is refused with `RunClosedError` — via either `appendDraft` or
 * `appendEvent` — before any sequence bookkeeping happens, so a refused
 * append never consumes a `seq`. This makes the assumption issue onsager-ai/ethogram#5 rests
 * its "simpler to fold and to prove terminal" argument on — that a run has
 * at most one terminal event — something this sink actually enforces rather
 * than merely hopes for.
 */
export class InMemorySink {
  readonly #clock: Clock;
  readonly #runs = new Map<string, Event[]>();
  readonly #finishedRuns = new Set<string>();

  constructor(clock: Clock = () => new Date().toISOString()) {
    this.#clock = clock;
  }

  appendDraft(runId: string, draft: EventDraft): Event {
    if (this.#finishedRuns.has(runId)) {
      throw new RunClosedError(runId);
    }

    const events = this.#eventsFor(runId);
    const event = stamp(draft, {
      runId,
      seq: events.length + 1,
      ts: this.#clock(),
    });
    events.push(event);
    if (event.type === RUN_FINISHED) {
      this.#finishedRuns.add(runId);
    }
    return event;
  }

  appendEvent(input: Event): Event {
    const event = parseEvent(input);
    if (this.#finishedRuns.has(event.runId)) {
      throw new RunClosedError(event.runId);
    }

    const events = this.#eventsFor(event.runId);
    const expected = events.length + 1;
    if (event.seq !== expected) {
      throw new SequenceError(event.runId, expected, event.seq);
    }
    events.push(event);
    if (event.type === RUN_FINISHED) {
      this.#finishedRuns.add(event.runId);
    }
    return event;
  }

  events(runId: string): readonly Event[] {
    return [...(this.#runs.get(runId) ?? [])];
  }

  #eventsFor(runId: string): Event[] {
    let events = this.#runs.get(runId);
    if (events === undefined) {
      events = [];
      this.#runs.set(runId, events);
    }
    return events;
  }
}

export interface FoldedRun {
  runId: string;
  kind: RunKind;
  actor: string;
  harness: string;
  parentRunId?: string;
  outcome?: RunOutcome;
  durationMs?: number;
  open: boolean;
}

/**
 * The lifecycle fold for one run: the two lifecycle markers and what can be
 * read from them. Unrelated events between the markers are ignored; malformed
 * lifecycle payloads and mismatched run ids are rejected, so this cannot
 * manufacture a coherent run from an incoherent sequence.
 *
 * **This is consumer-facing and consumed.** ostrom-hub folds runs with it in
 * `web/src/run-fold.ts`, `web/src/data/live-run.ts` and
 * `web/src/pages/Components.tsx`, with three test files besides. It was
 * documented as a reference implementation "not a consumer-facing run model"
 * until onsager-ai/ethogram#70 found that description had stopped being true — a change here is
 * a change to a consumer's rendering, not to an example.
 *
 * It folds **one** run. The corpus is a set of independent envelopes and must
 * not be grouped by `runId` and fed to this — see `conformance/README.md`.
 */
export function foldRun(events: Iterable<Event>): FoldedRun | undefined {
  let run: FoldedRun | undefined;

  for (const event of events) {
    if (event.type === RUN_STARTED) {
      if (run !== undefined) {
        throw new Error("Run fold received more than one run.started event");
      }
      const payload = parseRunStartedPayload(event.payload);
      run = {
        runId: event.runId,
        kind: payload.kind,
        actor: payload.actor,
        harness: payload.harness,
        ...(payload.parentRunId === undefined
          ? {}
          : { parentRunId: payload.parentRunId }),
        open: true,
      };
      continue;
    }

    if (event.type === RUN_FINISHED) {
      if (run === undefined) {
        throw new Error("Run fold received run.finished before run.started");
      }
      if (event.runId !== run.runId) {
        throw new Error(
          `Run fold expected run id ${run.runId}; received ${event.runId}`,
        );
      }
      const payload = parseRunFinishedPayload(event.payload);
      run = {
        ...run,
        outcome: payload.outcome,
        durationMs: payload.durationMs,
        open: false,
      };
    }
  }

  return run;
}
