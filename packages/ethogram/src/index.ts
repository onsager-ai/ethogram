export const EVENT_SCHEMA_VERSION = 1 as const;

/** Maximum number of Unicode scalar values carried by an `agent.text`. */
export const MAX_TEXT_SCALARS = 16_384 as const;

/** Maximum scalars carried by any excerpted field other than `agent.text`. */
export const MAX_EXCERPT_SCALARS = 4_096 as const;

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
 * issue #6, every lone surrogate this function encounters is therefore
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
 * for a newer vocabulary. Consumers must handle the unfamiliar-string case
 * explicitly and must never map it onto a known kind.
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
] as const;

export type KnownRunOutcome = (typeof RUN_OUTCOMES)[number];

/**
 * A run outcome this SDK knows, or an unfamiliar wire string retained
 * verbatim for a newer vocabulary. Consumers acting on an outcome must treat
 * an unfamiliar string as "not this", never as one of the known outcomes.
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
 * a whole, computed once at the point the run ends — not a sum a consumer
 * has assembled from every `agent.completed` the run happened to emit along
 * the way. Recomputing that total client-side by adding up
 * `agent.completed.costUsd`/`usage` over-counts whenever one harness session
 * reports `agent.completed` more than once, because those fields are
 * cumulative per session rather than per invocation (see
 * `AgentCompletedPayload`'s doc comments for why). This payload is the
 * number to trust for the run.
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
   * bound in `RunCeilings.turns`). Unlike `costUsd` and `usage` below, this
   * is per invocation rather than cumulative per session, so it is safe to
   * sum across every `agent.completed` in a run.
   */
  turns?: number;
  /**
   * Echoes the harness session identifier `agent.started` already carries,
   * so this completion can state which session's totals it is reporting.
   * `costUsd` and `usage` below are cumulative per session rather than per
   * invocation, and that rule was unusable from a completion alone before
   * this field existed: `sessionId` appeared only on `agent.started`, so a
   * consumer had to correlate backwards to whichever `agent.started` opened
   * the session before it could safely take a maximum within a session or
   * sum across sessions. Carrying it here too makes the rule applicable
   * from the very event that states the totals it governs.
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
   * `run.finished.costUsd` carries the runtime's own computed total for the
   * whole run and is the number to trust there.
   */
  costUsd?: number;
  model?: string;
  /**
   * Same cumulative-per-`sessionId` caveat as `costUsd` above: this is the
   * session's running usage total as of this completion, not a
   * per-invocation delta, so naively summing every `agent.completed.usage`
   * in a run over-counts a session that reports more than once. Take the
   * maximum within each session and sum across sessions; `run.finished.usage`
   * carries the runtime's own computed total for the run.
   */
  usage?: RunUsage;
  /**
   * Wall time *this invocation* took. Like `turns` above and unlike
   * `costUsd`/`usage`, this is per invocation rather than cumulative per
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

/**
 * Returns the entries of `value` whose keys are not in `fields`, to be
 * carried forward as an opaque extension rather than rejected or dropped
 * (issue #12): a sink that forwards an event it does not fully understand
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
    throw new TypeError(`${name} is missing required field: ${field}`);
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "string") {
    throw new TypeError(`${name}.${field} must be a string`);
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
    throw new TypeError(`${name}.${field} must be a string when present`);
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
    throw new TypeError(`${name}.${field} must be a finite number when present`);
  }
  return fieldValue;
}

/**
 * Parses an optional count field that must be a whole, non-negative number
 * — integer-valued `ceilings`, `usage`, and agent fields, all of which are
 * counts and can never be fractional or negative. `Number.isSafeInteger`
 * rejects a non-integer (`10.5`) and a value outside the ±2^53−1 magnitude
 * this protocol's numbers are bounded to (issue #9) in one check; the sign
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
    throw new TypeError(
      `${name}.${field} must be a non-negative safe integer when present`,
    );
  }
  return fieldValue;
}

/**
 * Parses a required count field that must be a whole, non-negative number —
 * the same rule as `optionalSafeInteger`, but for a field the payload cannot
 * omit. Every count of milliseconds is a `u64` on the Rust side (ruling on
 * issue #6): `RunFinishedPayload.durationMs` is the only field on this typed
 * path that is both a count and required, so this is where that rule is
 * enforced.
 */
function requiredSafeInteger(
  value: Record<string, unknown>,
  field: string,
  name: string,
): number {
  if (!Object.hasOwn(value, field)) {
    throw new TypeError(`${name} is missing required field: ${field}`);
  }
  const fieldValue = value[field];
  if (
    typeof fieldValue !== "number" ||
    !Number.isSafeInteger(fieldValue) ||
    fieldValue < 0
  ) {
    throw new TypeError(`${name}.${field} must be a non-negative safe integer`);
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
    throw new TypeError(`${name}.${field} must be a boolean when present`);
  }
  return fieldValue;
}

function parseRunCeilings(value: unknown): RunCeilings {
  const name = "RunStartedPayload.ceilings";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
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
 * `kind` strings are tolerated and retained (issue #12), while required
 * fields stay strict.
 */
export function parseRunStartedPayload(value: unknown): RunStartedPayload {
  const name = "RunStartedPayload";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
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
 * `outcome` strings are tolerated and retained (issue #12), while required
 * fields stay strict.
 */
export function parseRunFinishedPayload(value: unknown): RunFinishedPayload {
  const name = "RunFinishedPayload";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
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
    throw new TypeError(`${name} must be an object`);
  }

  const stage = optionalString(value, "stage", name);
  const message = requiredString(value, "message", name);
  return {
    ...(stage === undefined ? {} : { stage }),
    message,
    ...extractUnknownFields(value, AGENT_WARNING_FIELDS),
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
    default:
      return payload;
  }
}

/**
 * The largest magnitude at which an integral number round-trips exactly
 * between this SDK and the Rust SDK (2^53 − 1). Shared by `Event.seq`
 * validation and payload-number validation (issue #9): both reject an
 * out-of-range integral value at parse time rather than rounding it.
 */
const MAX_SAFE_INTEGER_MAGNITUDE = Number.MAX_SAFE_INTEGER;

/**
 * Recursively validates that every integral-valued number in `value` is
 * within the safe-integer magnitude bound, naming the offending path (for
 * example `payload.nested.count` or `payload.items[2].total`) when the check
 * fails. Non-integral numbers are never bounded, no matter how large their
 * magnitude. Mirrors the `Event.seq` check above and reuses the same bound
 * (issue #9): a value that needs more precision must be carried as a string
 * instead of a number.
 *
 * `JSON.parse` has already collapsed any literal too large to represent
 * exactly before this function ever sees it, so only magnitude can be
 * tested here; that is sufficient, because the bound is on magnitude.
 */
function validatePayloadNumbers(value: unknown, path: string): void {
  if (typeof value === "number") {
    if (Number.isInteger(value) && Math.abs(value) > MAX_SAFE_INTEGER_MAGNITUDE) {
      throw new TypeError(
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
    throw new TypeError(
      `${field} has ${actual} Unicode scalar values; maximum is ${maximum}`,
    );
  }
}

/**
 * Validate whether a producer should emit `payload` for `eventType`. Required
 * fields, known closed-union membership, safe-integer bounds, and capture
 * bounds are enforced for known event types; unknown event types stay open
 * and unvalidated.
 *
 * `parse_event` answers "can both SDKs carry this?"; `validate` answers
 * "should a producer have emitted this?" `parseEvent` therefore does not call
 * this function: a representable over-bound event must remain forwardable.
 */
export function validate(eventType: string, payload: unknown): void {
  if (!KNOWN_TYPE_VALUES.has(eventType)) {
    return;
  }

  validatePayloadNumbers(payload, "payload");
  const parsed = parseKnownPayload(eventType, payload);

  switch (eventType) {
    case RUN_STARTED: {
      const started = parsed as RunStartedPayload;
      if (!RUN_KIND_VALUES.has(started.kind)) {
        throw new TypeError(
          `RunStartedPayload.kind has unknown value: ${started.kind}`,
        );
      }
      return;
    }
    case RUN_FINISHED: {
      const finished = parsed as RunFinishedPayload;
      if (!RUN_OUTCOME_VALUES.has(finished.outcome)) {
        throw new TypeError(
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
    default:
      // The remaining known types have no closed union or captured text.
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
 * and are tolerated rather than rejected (issue #12): every recognised
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
 * run has already recorded a terminal event (issues #5 and #3): a run has at
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
 * append never consumes a `seq`. This makes the assumption issue #5 rests
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
 * Reference implementation of the lifecycle fold for one run, not a
 * consumer-facing run model. Unrelated events between the two lifecycle
 * markers are ignored; malformed lifecycle payloads and mismatched run ids
 * are rejected so the example cannot manufacture a coherent run from an
 * incoherent sequence.
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
